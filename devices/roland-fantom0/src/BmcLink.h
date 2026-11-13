// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// The FANTOM-0's message link between its Linux side and its sound chips (the BMCs) as Roland's
// stock BMC firmware speaks it, on /dev/ttyGS0 (docs/fantom-0.md, "The message link"): the keys,
// and MIDI from the instrument's MIDI ports, arrive there as MIDI words in Roland's frames.

#include <algorithm>
#include <array>
#include <atomic>
#include <bitset>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <functional>
#include <mutex>
#include <string>
#include <thread>
#include <utility>

namespace oscairn::host
{
    /// One frame's header, as it came. A short frame is all of it: byte 0 the kind times two (bit 0
    /// clear), byte 1 the sender's address, byte 2 the destination's, byte 3 a sub-type, bytes 4 to
    /// 10 up to seven payload bytes, byte 11 how many. A long frame has bit 0 of byte 0 set and a
    /// 16-byte header whose bytes 12 to 15 are its payload's length, low byte first; the payload
    /// follows and is not kept.
    struct BmcFrame
    {
        std::array<uint8_t, 16> raw{};
        bool isLong = false;
        uint32_t length = 0; // a long frame's payload length; a short frame's payload count
        uint8_t kind() const { return raw[0] >> 1; }
        uint8_t from() const { return raw[1]; }
        uint8_t to() const { return raw[2]; }
    };

    /// The MIDI message in a frame of kind 3 (one MIDI word, laid out like a USB MIDI event: byte 0's
    /// low four bits the code index, bytes 1 to 3 the message): a channel message, its size; 0 for
    /// anything else. The code index must agree with the status, as USB MIDI's does.
    inline size_t bmcMidi( const BmcFrame& f, uint8_t out[3] )
    {
        if( f.isLong || f.kind() != 3 || f.length != 4 ) return 0;
        const uint8_t* w = f.raw.data() + 4;
        if( w[1] < 0x80 || w[1] >= 0xF0 || ( w[1] >> 4 ) != ( w[0] & 0x0F ) ) return 0;
        const size_t size = ( w[1] & 0xE0 ) == 0xC0 ? 2 : 3; // program change and channel pressure: one data byte
        for( size_t i = 1; i < size; ++i ) if( w[1 + i] > 0x7F ) return 0;
        std::copy_n( w + 1, size, out );
        return size;
    }

    /// The note in a key's own frame: kind 6 from the keyboard (0x02), a panel word of 0x88 (on) or
    /// 0x87 (off), the note, 0, the velocity (a release velocity on an off). One comes for every key
    /// event, whatever Roland's zones and Local Switch are; the frame carries no channel, so the note
    /// is on channel 1. Its size, 3; 0 for any other frame.
    inline size_t bmcKey( const BmcFrame& f, uint8_t out[3] )
    {
        if( f.isLong || f.kind() != 6 || f.from() != 0x02 || f.length != 4 ) return 0;
        const uint8_t* w = f.raw.data() + 4;
        if( ( w[0] != 0x88 && w[0] != 0x87 ) || w[1] > 0x7F || w[3] > 0x7F ) return 0;
        out[0] = w[0] == 0x88 ? 0x90 : 0x80;
        out[1] = w[1];
        out[2] = w[3];
        return 3;
    }

    /// The link's byte stream cut into frames. It has no delimiter and no checksum, so a reader that
    /// starts mid-stream (or loses its place) finds it again: it takes a position as a frame's start
    /// only where a frame it knows begins (a MIDI word, kind 3, whose code index agrees with its
    /// status; a panel word, kind 6, with bit 7 of its first byte set; a long frame of kind 9 no
    /// longer than `lookahead`) and the next frame's header after it is plausible too (a short
    /// frame of a kind above 0 with at most seven payload bytes, or a long one no longer than
    /// `maxLong`). Bytes before that are skipped. Once placed, it follows the headers; a header
    /// that is not plausible loses the place, and the search starts at the next byte.
    ///
    /// No allocation; memory bounded by `lookahead`. One parser per stream, used by one thread.
    class BmcLinkParser
    {
    public:
        static constexpr uint32_t maxLong = 1u << 20;  // a long frame's length beyond this loses the place
        static constexpr uint32_t lookahead = 4096;    // the longest frame the search can place itself on
        struct Stats { uint64_t frames = 0, skipped = 0, lost = 0; }; // skipped: bytes outside any frame; lost: places lost

        /// Calls `frame(const BmcFrame&)` for each frame `data` completes, long ones too.
        template<class F> void feed( const uint8_t* data, size_t size, F&& frame )
        {
            while( size > 0 )
            {
                if( skip_ ) { const auto n = size_t( std::min<uint64_t>( skip_, size ) ); skip_ -= n; data += n; size -= n; continue; }
                const auto n = std::min( size, buffer_.size() - have_ );
                std::memcpy( buffer_.data() + have_, data, n );
                have_ += n; data += n; size -= n;
                drain( frame );
            }
        }
        const Stats& stats() const { return stats_; }
        /// The stream starts again (the device opened again): what was buffered is not joined to it.
        void reset() { have_ = 0; skip_ = 0; placed_ = false; }

    private:
        static uint32_t length( const uint8_t* h ) { return uint32_t( h[12] ) | uint32_t( h[13] ) << 8 | uint32_t( h[14] ) << 16 | uint32_t( h[15] ) << 24; }
        static bool plausibleShort( const uint8_t* h ) { return ( h[0] >> 1 ) > 0 && h[11] <= 7; }
        static bool known( const uint8_t* h ) // a short frame the search may place itself on
        {
            if( h[11] != 4 ) return false;
            if( h[0] == 3 << 1 ) return h[5] >= 0x80 && h[5] < 0xF0 && ( h[5] >> 4 ) == ( h[4] & 0x0F );
            return h[0] == 6 << 1 && ( h[4] & 0x80 );
        }
        // Whether a frame the search may place itself on starts at `at`: 1 yes, 0 no, -1 not yet known.
        int candidate( size_t at ) const
        {
            const uint8_t* h = buffer_.data() + at;
            const size_t have = have_ - at;
            size_t size = 12;
            if( h[0] & 1 )
            {
                if( h[0] != ( 9 << 1 | 1 ) ) return 0;
                if( have < 16 ) return -1;
                if( length( h ) > lookahead ) return 0;
                size = 16 + length( h );
            }
            else
            {
                if( h[0] != 3 << 1 && h[0] != 6 << 1 ) return 0;
                if( have < 12 ) return -1;
                if( !known( h ) ) return 0;
            }
            if( have < size + 1 ) return -1;
            const uint8_t* next = h + size;
            if( next[0] & 1 ) return have < size + 16 ? -1 : length( next ) <= maxLong;
            return have < size + 12 ? -1 : plausibleShort( next );
        }
        template<class F> void drain( F& frame )
        {
            size_t at = 0;
            while( at < have_ )
            {
                const uint8_t* h = buffer_.data() + at;
                const size_t have = have_ - at;
                if( !placed_ )
                {
                    const int c = candidate( at );
                    if( c < 0 ) break;
                    if( c == 0 ) { ++at; ++stats_.skipped; continue; }
                    placed_ = true;
                }
                BmcFrame f;
                if( !( h[0] & 1 ) )
                {
                    if( have < 12 ) break;
                    if( !plausibleShort( h ) ) { placed_ = false; ++stats_.lost; ++stats_.skipped; ++at; continue; }
                    std::copy_n( h, 12, f.raw.begin() );
                    f.length = h[11];
                    at += 12;
                }
                else
                {
                    if( have < 16 ) break;
                    if( length( h ) > maxLong ) { placed_ = false; ++stats_.lost; ++stats_.skipped; ++at; continue; }
                    std::copy_n( h, 16, f.raw.begin() );
                    f.isLong = true;
                    f.length = length( h );
                    at += 16;
                    const auto n = size_t( std::min<uint64_t>( f.length, have_ - at ) );
                    at += n;
                    skip_ = f.length - n; // the rest is skipped as it arrives, never buffered
                }
                ++stats_.frames;
                frame( std::as_const( f ) );
            }
            std::memmove( buffer_.data(), buffer_.data() + at, have_ - at );
            have_ -= at;
        }

        // A frame the search places itself on and the header after it fit, whatever came before.
        std::array<uint8_t, 16 + lookahead + 16> buffer_{};
        size_t have_ = 0;
        uint64_t skip_ = 0; // a long frame's payload still to come
        bool placed_ = false;
        Stats stats_;
    };

    /// The link's MIDI sources, by sender address, in the order their inputs are listed: the keys
    /// (0x02), then the addresses Roland's app takes MIDI from its MIDI ports on (docs/fantom-0.md;
    /// which is DIN and which USB is not known, so each is named by its address).
    inline constexpr std::array<uint8_t, 6> bmcSources{ 0x02, 0x81, 0xA1, 0xB2, 0xC8, 0xD8 };
    std::string bmcInputName( size_t input ); // "FANTOM keys", "FANTOM MIDI 0x81", ...

    /// Reads the link on a thread of its own and hands each MIDI message to `message` there, by
    /// input (an index into bmcSources): the keys from their own frames (bmcKey), the other sources
    /// from their MIDI words (bmcMidi). It never writes to the device: it opens it read-only, and
    /// before reading makes it raw with echo and flow control off, checked by reading the settings
    /// back; where they do not hold it does not read. It polls with a timeout (so stopping waits at
    /// most that), and reads without pause while open: the sound chips queue what is not read, and
    /// Roland's engine then holds its notes. A device that fails or ends is opened again a second
    /// later.
    ///
    /// What the link queued before it was opened comes out at once and is stale: MIDI words read in
    /// the first `settle` after an open, and until the device has nothing more waiting, are dropped
    /// (counted, logged once). Notes it passed on and has not seen end are ended (note off, and
    /// sustain off) when the device fails or ends, so none is left held across a reopen.
    ///
    /// Frames the link is known to carry are passed over quietly: MIDI words from the keyboard (0x02:
    /// the notes Roland's engine plays for them, one per active zone, none with Local Switch off)
    /// and the sound engine's status (kind 9 from 0xE1, long).
    /// Anything else is counted and logged with its header bytes, at most ten a second.
    class BmcLinkReader
    {
    public:
        using Message = std::function<void( size_t input, const uint8_t* bytes, size_t size )>;
        using Log = std::function<void( const std::string& )>; // empty: no log
        static void standardError( const std::string& line );
        BmcLinkReader( std::string path, Message message, Log log = standardError, std::chrono::milliseconds settle = std::chrono::milliseconds( 500 ) );
        ~BmcLinkReader(); // stops and joins
        BmcLinkReader( const BmcLinkReader& ) = delete;
        BmcLinkReader& operator=( const BmcLinkReader& ) = delete;

        struct Stats { uint64_t opened = 0, frames = 0, messages = 0, stale = 0, unknown = 0, skipped = 0, lost = 0; };
        Stats stats() const;

    private:
        void run();
        bool open( bool report ); // false: not opened (logged if `report`), fd_ stays -1
        void frame( const BmcFrame& );
        void deliver( size_t input, const uint8_t* bytes, size_t size );
        void release(); // ends the notes and sustain passed on
        bool pause();   // a second, or until stopped; false when stopped

        std::string path_;
        Message message_;
        Log log_;
        std::chrono::milliseconds settle_;
        std::mutex lock_; // the pause's wake
        std::condition_variable stopped_;
        std::atomic<bool> stop_{ false };
        std::atomic<uint64_t> opened_{ 0 }, frames_{ 0 }, messages_{ 0 }, stale_{ 0 }, unknown_{ 0 }, skipped_{ 0 }, lost_{ 0 };
        // The reader thread's own.
        int fd_ = -1;
        BmcLinkParser parser_;
        bool settling_ = false, drained_ = false; // dropping what was queued; the last read left nothing waiting
        std::chrono::steady_clock::time_point openedAt_;
        uint64_t staleSinceOpen_ = 0;
        std::array<std::array<std::bitset<128>, 16>, bmcSources.size()> held_{}; // keys passed on and not ended, per input and channel
        std::array<std::bitset<16>, bmcSources.size()> sustained_{};
        int64_t window_ = 0; uint32_t logged_ = 0; uint64_t hidden_ = 0; // the log's rate limit, per second
        std::thread thread_; // last: started once the rest is ready
    };
}
