// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

namespace oscairn::host
{
    /// A MIDI 1.0 byte stream, as a raw MIDI device gives it, cut into whole messages as the ALSA
    /// sequencer would deliver them. A message may arrive across any number of reads.
    ///
    /// - Running status: data bytes after a channel message repeat its status. A system common
    ///   message or SysEx cancels it.
    /// - Real-time bytes (F8 to FF) are messages of their own at once, even inside another.
    /// - Data bytes with no status to belong to are skipped; a message cut short by another
    ///   status is dropped.
    /// - SysEx is kept whole, F0 to F7, up to `maxSysEx` bytes; a longer one, or one cut short
    ///   by another status, is dropped whole. The engine passes SysEx over either way.
    ///
    /// No allocation; one stream per device, used by one thread.
    class MidiStream
    {
    public:
        static constexpr size_t maxSysEx = 1024;

        /// Calls `message(const uint8_t* bytes, size_t size)` for each message `data` completes.
        template<class F> void feed( const uint8_t* data, size_t size, F&& message )
        {
            for( size_t i = 0; i < size; ++i )
            {
                const uint8_t b = data[i];
                if( b >= 0xF8 ) { message( &b, size_t( 1 ) ); continue; }
                if( sysex_ )
                {
                    if( b < 0x80 ) { if( have_ < maxSysEx ) buffer_[have_++] = b; else overflow_ = true; continue; }
                    if( b == 0xF7 && !overflow_ && have_ < maxSysEx ) { buffer_[have_++] = b; message( buffer_.data(), have_ ); }
                    sysex_ = false; have_ = 0;
                    if( b == 0xF7 ) continue;
                }
                if( b >= 0x80 )
                {
                    have_ = 0;
                    if( b == 0xF7 ) { running_ = 0; continue; } // an end with no start
                    if( b == 0xF0 ) { running_ = 0; sysex_ = true; overflow_ = false; buffer_[have_++] = b; continue; }
                    if( b > 0xF0 ) running_ = 0;
                    else running_ = b;
                    need_ = length( b );
                    if( need_ == 0 ) continue; // F4, F5: undefined
                    buffer_[have_++] = b;
                    if( have_ == need_ ) { message( buffer_.data(), have_ ); have_ = 0; }
                    continue;
                }
                if( have_ == 0 )
                {
                    if( !running_ ) continue; // nothing to belong to
                    buffer_[have_++] = running_;
                    need_ = length( running_ );
                }
                buffer_[have_++] = b;
                if( have_ == need_ ) { message( buffer_.data(), have_ ); have_ = 0; }
            }
        }

    private:
        // A message's size by its status byte, status included; 0 for an undefined one.
        static size_t length( uint8_t status )
        {
            if( status < 0xF0 ) return ( status & 0xE0 ) == 0xC0 ? 2 : 3; // program change and channel pressure: one data byte
            switch( status )
            {
            case 0xF1: case 0xF3: return 2;
            case 0xF2: return 3;
            case 0xF6: return 1;
            default: return 0;
            }
        }

        uint8_t running_ = 0; // the channel status data bytes repeat, 0 for none
        bool sysex_ = false, overflow_ = false;
        size_t have_ = 0, need_ = 0; // the message's bytes so far, status included, and its size
        std::array<uint8_t, maxSysEx> buffer_{};
    };
}
