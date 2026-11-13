// SPDX-License-Identifier: AGPL-3.0-or-later
#include "RawMidi.h"
#include "MidiStream.h"

#include <alsa/asoundlib.h>

#include <algorithm>
#include <cerrno>
#include <fcntl.h>
#include <poll.h>
#include <system_error>
#include <unistd.h>

namespace oscairn::host
{
    bool alsaSequencer()
    {
        snd_seq_t* seq = nullptr;
        if( snd_seq_open( &seq, "default", SND_SEQ_OPEN_INPUT, SND_SEQ_NONBLOCK ) < 0 ) return false;
        snd_seq_close( seq );
        return true;
    }

    std::string sequencerPortName( const std::string& subdeviceName, const std::string& deviceName, unsigned ports, int card, int device, unsigned subdevice )
    {
        // As snd-seq-midi (the kernel's sound/core/seq/seq_midi.c) names the port it makes for each subdevice.
        if( !subdeviceName.empty() ) return subdeviceName;
        const auto suffix = ports > 1 ? "-" + std::to_string( subdevice ) : std::string();
        if( !deviceName.empty() ) return deviceName + suffix;
        return "MIDI " + std::to_string( card ) + "-" + std::to_string( device ) + suffix;
    }

    std::vector<RawMidiPort> rawMidiPorts()
    {
        std::vector<RawMidiPort> ports;
        snd_rawmidi_info_t* info = nullptr;
        if( snd_rawmidi_info_malloc( &info ) < 0 ) return ports;
        for( int card = -1; snd_card_next( &card ) >= 0 && card >= 0; )
        {
            snd_ctl_t* ctl = nullptr;
            if( snd_ctl_open( &ctl, ( "hw:" + std::to_string( card ) ).c_str(), SND_CTL_NONBLOCK ) < 0 ) continue;
            for( int device = -1; snd_ctl_rawmidi_next_device( ctl, &device ) >= 0 && device >= 0; )
            {
                auto select = [&]( snd_rawmidi_stream_t stream, unsigned subdevice ) {
                    snd_rawmidi_info_set_device( info, unsigned( device ) );
                    snd_rawmidi_info_set_stream( info, stream );
                    snd_rawmidi_info_set_subdevice( info, subdevice );
                    return snd_ctl_rawmidi_info( ctl, info ) >= 0;
                };
                const unsigned outputs = select( SND_RAWMIDI_STREAM_OUTPUT, 0 ) ? snd_rawmidi_info_get_subdevices_count( info ) : 0;
                const unsigned inputs = select( SND_RAWMIDI_STREAM_INPUT, 0 ) ? snd_rawmidi_info_get_subdevices_count( info ) : 0;
                const unsigned count = std::min( std::max( inputs, outputs ), 32u ); // snd-seq-midi's ports per device, at most 256 / 8
                for( unsigned sub = 0; sub < std::min( inputs, count ); ++sub )
                {
                    // snd-seq-midi names a port that has an output after the output's subdevice.
                    if( !select( sub < outputs ? SND_RAWMIDI_STREAM_OUTPUT : SND_RAWMIDI_STREAM_INPUT, sub ) ) continue;
                    ports.push_back( { "hw:" + std::to_string( card ) + "," + std::to_string( device ) + "," + std::to_string( sub ),
                                       sequencerPortName( snd_rawmidi_info_get_subdevice_name( info ), snd_rawmidi_info_get_name( info ), count, card, device, sub ) } );
                }
            }
            snd_ctl_close( ctl );
        }
        snd_rawmidi_info_free( info );
        return ports;
    }

    RawMidiSource openRawMidi( const std::string& identifier )
    {
        snd_rawmidi_t* in = nullptr;
        if( snd_rawmidi_open( &in, nullptr, identifier.c_str(), SND_RAWMIDI_NONBLOCK ) < 0 ) return {};
        pollfd p{};
        if( snd_rawmidi_poll_descriptors( in, &p, 1 ) != 1 ) { snd_rawmidi_close( in ); return {}; }
        return { p.fd, [in] { snd_rawmidi_close( in ); } }; // a hw device: its descriptor is read as ALSA reads it
    }

    RawMidiReader::RawMidiReader( std::vector<RawMidiSource> sources, Message message, std::vector<std::string> seen, std::function<std::vector<std::string>()> scan,
                                  std::function<void()> changed, std::chrono::milliseconds rescan )
        : sources_( std::move( sources ) ), message_( std::move( message ) ), seen_( std::move( seen ) ), scan_( std::move( scan ) ), changed_( std::move( changed ) ), rescan_( rescan )
    {
        if( ::pipe2( wake_, O_CLOEXEC ) < 0 ) throw std::system_error( errno, std::generic_category(), "raw MIDI reader" );
        thread_ = std::thread( [this] { run(); } );
    }

    RawMidiReader::~RawMidiReader()
    {
        const char stop = 0;
        while( ::write( wake_[1], &stop, 1 ) < 0 && errno == EINTR ) {}
        thread_.join();
        for( auto& s : sources_ ) if( s.fd >= 0 && s.close ) s.close();
        ::close( wake_[0] );
        ::close( wake_[1] );
    }

    void RawMidiReader::run()
    {
        std::vector<MidiStream> streams( sources_.size() );
        std::vector<pollfd> polls;
        std::vector<size_t> polled; // which source each poll entry after the first is
        bool lost = false, told = false;
        auto next = std::chrono::steady_clock::now() + rescan_;
        for( ;; )
        {
            polls.assign( 1, { wake_[0], POLLIN, 0 } );
            polled.clear();
            for( size_t i = 0; i < sources_.size(); ++i )
                if( sources_[i].fd >= 0 ) { polls.push_back( { sources_[i].fd, POLLIN, 0 } ); polled.push_back( i ); }
            const auto wait = std::chrono::duration_cast<std::chrono::milliseconds>( next - std::chrono::steady_clock::now() ).count();
            const int ready = ::poll( polls.data(), nfds_t( polls.size() ), int( std::max<long long>( wait, 0 ) ) );
            if( ready < 0 && errno == EINTR ) continue;
            if( ready > 0 && polls[0].revents ) return;
            for( size_t k = 0; ready > 0 && k < polled.size(); ++k )
            {
                const auto revents = polls[k + 1].revents;
                if( !revents ) continue;
                auto& source = sources_[polled[k]];
                uint8_t buffer[256];
                const auto n = ::read( source.fd, buffer, sizeof buffer );
                if( n > 0 )
                {
                    streams[polled[k]].feed( buffer, size_t( n ), [&]( const uint8_t* bytes, size_t size ) { message_( polled[k], bytes, size ); } );
                    continue;
                }
                const bool waiting = n < 0 && ( errno == EAGAIN || errno == EINTR );
                if( waiting && !( revents & ( POLLERR | POLLNVAL ) ) ) continue;
                // Ended (unplugged: ENODEV) or failing: read no more, so it cannot spin.
                if( source.close ) source.close();
                source.fd = -1;
                lost = true;
            }
            if( const auto now = std::chrono::steady_clock::now(); now >= next )
            {
                next = now + rescan_;
                if( !told && ( lost || scan_() != seen_ ) ) { told = true; changed_(); }
            }
        }
    }
}
