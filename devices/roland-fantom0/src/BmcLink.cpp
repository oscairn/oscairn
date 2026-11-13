// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The FANTOM-0's message link, read for its keys and MIDI (BmcLink.h).
#include "BmcLink.h"

#include <cerrno>
#include <cstdio>
#include <fcntl.h>
#include <poll.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <unistd.h>

namespace oscairn::host
{
    std::string bmcInputName( size_t input )
    {
        if( input == 0 ) return "FANTOM keys";
        char name[32];
        std::snprintf( name, sizeof name, "FANTOM MIDI 0x%02X", unsigned( bmcSources[input] ) );
        return name;
    }

    void BmcLinkReader::standardError( const std::string& line ) { std::fprintf( stderr, "oscairn: keys: %s\n", line.c_str() ); }

    BmcLinkReader::BmcLinkReader( std::string path, Message message, Log log, std::chrono::milliseconds settle )
        : path_( std::move( path ) ), message_( std::move( message ) ), log_( std::move( log ) ), settle_( settle ), thread_( [this] { run(); } ) {}

    BmcLinkReader::~BmcLinkReader()
    {
        { std::lock_guard lock( lock_ ); stop_ = true; }
        stopped_.notify_all();
        thread_.join();
        if( fd_ >= 0 ) ::close( fd_ );
    }

    BmcLinkReader::Stats BmcLinkReader::stats() const
    {
        return { opened_.load(), frames_.load(), messages_.load(), stale_.load(), unknown_.load(), skipped_.load(), lost_.load() };
    }

    bool BmcLinkReader::pause()
    {
        std::unique_lock lock( lock_ );
        return !stopped_.wait_for( lock, std::chrono::seconds( 1 ), [this] { return stop_.load(); } );
    }

    bool BmcLinkReader::open( bool report )
    {
        const int fd = ::open( path_.c_str(), O_RDONLY | O_NOCTTY | O_NONBLOCK | O_CLOEXEC );
        if( fd < 0 ) { if( report && log_ ) log_( "cannot open " + path_ + ": " + std::strerror( errno ) ); return false; }
        // Echo off before any read: with it on, the tty layer would send each byte received back to the sound chips.
        termios t{};
        bool raw = ::tcgetattr( fd, &t ) == 0;
        if( raw )
        {
            ::cfmakeraw( &t );
            t.c_lflag &= ~tcflag_t( ECHO | ECHOE | ECHOK | ECHONL | ICANON | ISIG | IEXTEN );
            t.c_iflag &= ~tcflag_t( IXON | IXOFF | IXANY ); // IXOFF would send XOFF and XON when the queue fills
            termios back{};
            raw = ::tcsetattr( fd, TCSANOW, &t ) == 0 && ::tcgetattr( fd, &back ) == 0 && !( back.c_lflag & ( ECHO | ECHOE | ECHOK | ECHONL | ICANON ) ) &&
                  !( back.c_iflag & ( IXON | IXOFF | IXANY ) );
        }
        if( !raw )
        {
            if( report && log_ ) log_( path_ + " cannot be made raw with echo off; not reading it, so nothing read is sent back" );
            ::close( fd );
            return false;
        }
        fd_ = fd;
        parser_.reset();
        settling_ = true;
        drained_ = false;
        openedAt_ = std::chrono::steady_clock::now();
        staleSinceOpen_ = 0;
        ++opened_;
        return true;
    }

    void BmcLinkReader::run()
    {
        bool reported = false; // a failure to open, once until it opens
        while( !stop_ )
        {
            if( fd_ < 0 )
            {
                if( !open( !reported ) )
                {
                    reported = true;
                    if( !pause() ) return;
                    continue;
                }
                reported = false;
            }
            pollfd p{ fd_, POLLIN, 0 };
            const int ready = ::poll( &p, 1, 100 ); // the timeout bounds how long stopping waits
            if( ready == 0 ) { drained_ = true; continue; }
            if( ready < 0 && errno == EINTR ) continue;
            uint8_t buffer[4096];
            const auto n = ready > 0 ? ::read( fd_, buffer, sizeof buffer ) : -1;
            if( n > 0 )
            {
                // Settled: the window has passed and the last read left nothing waiting, so these bytes are new.
                if( settling_ && drained_ && std::chrono::steady_clock::now() - openedAt_ >= settle_ )
                {
                    settling_ = false;
                    if( staleSinceOpen_ && log_ ) log_( "dropped " + std::to_string( staleSinceOpen_ ) + " MIDI messages queued before " + path_ + " was opened" );
                }
                parser_.feed( buffer, size_t( n ), [this]( const BmcFrame& f ) { frame( f ); } );
                const auto& s = parser_.stats();
                frames_ = s.frames; skipped_ = s.skipped; lost_ = s.lost; // the parser's counts run on across reopens
                int waiting = 0;
                drained_ = ::ioctl( fd_, FIONREAD, &waiting ) == 0 && waiting == 0;
                continue;
            }
            if( n < 0 && ( errno == EAGAIN || errno == EINTR ) ) continue;
            if( log_ ) log_( path_ + ( n == 0 ? " ended" : std::string( ": " ) + std::strerror( errno ) ) + "; opening it again" );
            ::close( fd_ ); fd_ = -1;
            release();
            if( !pause() ) return;
        }
    }

    void BmcLinkReader::frame( const BmcFrame& f )
    {
        uint8_t midi[3];
        size_t size = 0;
        const auto source = std::find( bmcSources.begin(), bmcSources.end(), f.from() );
        if( f.isLong && f.kind() == 9 && f.from() == 0xE1 ) return; // the sound engine's status
        if( !f.isLong && f.kind() == 3 && f.from() == 0x02 ) return; // Roland's zones' notes for a key, not the key
        if( ( size = bmcKey( f, midi ) ) > 0 || ( source != bmcSources.end() && f.from() != 0x02 && ( size = bmcMidi( f, midi ) ) > 0 ) )
        {
            if( settling_ ) { ++stale_; ++staleSinceOpen_; return; }
            deliver( size_t( source - bmcSources.begin() ), midi, size );
            return;
        }
        ++unknown_;
        const auto second = std::chrono::duration_cast<std::chrono::seconds>( std::chrono::steady_clock::now().time_since_epoch() ).count();
        if( second != window_ ) { window_ = second; logged_ = 0; }
        if( !log_ || logged_ >= 10 ) { ++hidden_; return; }
        ++logged_;
        std::string line = f.isLong ? "unknown long frame, " + std::to_string( f.length ) + " bytes:" : std::string( "unknown frame:" );
        char hex[4];
        for( size_t i = 0; i < ( f.isLong ? 16u : 12u ); ++i ) { std::snprintf( hex, sizeof hex, " %02x", f.raw[i] ); line += hex; }
        if( hidden_ ) line += " (after " + std::to_string( hidden_ ) + " not shown)";
        hidden_ = 0;
        log_( line );
    }

    void BmcLinkReader::deliver( size_t input, const uint8_t* bytes, size_t size )
    {
        const auto kind = bytes[0] & 0xF0, channel = bytes[0] & 0x0F;
        if( kind == 0x90 || kind == 0x80 ) held_[input][channel][bytes[1]] = kind == 0x90 && bytes[2] > 0;
        if( kind == 0xB0 && bytes[1] == 64 ) sustained_[input][channel] = bytes[2] >= 64;
        ++messages_;
        if( message_ ) message_( input, bytes, size );
    }

    void BmcLinkReader::release()
    {
        for( size_t input = 0; input < held_.size(); ++input )
            for( uint8_t channel = 0; channel < 16; ++channel )
            {
                for( uint8_t key = 0; key < 128; ++key )
                    if( held_[input][channel][key] ) { const uint8_t off[3]{ uint8_t( 0x80 | channel ), key, 0 }; deliver( input, off, 3 ); }
                if( sustained_[input][channel] ) { const uint8_t off[3]{ uint8_t( 0xB0 | channel ), 64, 0 }; deliver( input, off, 3 ); }
            }
    }
}
