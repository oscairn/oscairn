// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A raw MIDI byte stream cut into messages (MidiStream.h), with no device: running status,
// real-time bytes inside other messages, a message split across reads, SysEx kept whole, garbage
// before the first status, an over-long SysEx dropped, and a message cut short by another.
#include "MidiStream.h"

#include <cstdio>
#include <string>
#include <vector>

using namespace oscairn::host;
using Messages = std::vector<std::vector<uint8_t>>;

namespace
{
    int failures = 0;
    void check( bool ok, const std::string& what ) { if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what.c_str() ); ++failures; } }

    // Each read fed in turn to one stream.
    Messages feed( MidiStream& stream, const std::vector<std::vector<uint8_t>>& reads )
    {
        Messages out;
        for( const auto& r : reads ) stream.feed( r.data(), r.size(), [&]( const uint8_t* b, size_t n ) { out.emplace_back( b, b + n ); } );
        return out;
    }
    Messages feed( const std::vector<std::vector<uint8_t>>& reads ) { MidiStream s; return feed( s, reads ); }
}

int main()
{
    check( feed( { { 0x90, 60, 100, 62, 101, 64, 0 } } ) == Messages{ { 0x90, 60, 100 }, { 0x90, 62, 101 }, { 0x90, 64, 0 } }, "running status repeats a note on" );
    check( feed( { { 0xC3, 5, 6, 0xD0, 70, 71 } } ) == Messages{ { 0xC3, 5 }, { 0xC3, 6 }, { 0xD0, 70 }, { 0xD0, 71 } }, "one data byte for program change and channel pressure" );
    check( feed( { { 0x90, 0xF8, 60, 0xFE, 100, 61, 0xF8, 90 } } ) == Messages{ { 0xF8 }, { 0xFE }, { 0x90, 60, 100 }, { 0xF8 }, { 0x90, 61, 90 } },
           "real-time bytes inside a message come out at once and leave it whole" );
    check( feed( { { 0xB0 }, { 1 }, { 64, 2 }, { 3 } } ) == Messages{ { 0xB0, 1, 64 }, { 0xB0, 2, 3 } }, "a message split across reads, with running status" );
    check( feed( { { 0xF0, 0x7E, 0x7F }, { 0x06, 0xF8, 0x01 }, { 0xF7 } } ) == Messages{ { 0xF8 }, { 0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7 } }, "SysEx whole across reads, a clock inside it" );
    check( feed( { { 0x90, 60, 100, 0xF0, 1, 0xF7, 61, 100 } } ) == Messages{ { 0x90, 60, 100 }, { 0xF0, 1, 0xF7 } }, "SysEx cancels running status" );
    check( feed( { { 0x12, 0x34, 0xF7, 0x80, 60, 0 } } ) == Messages{ { 0x80, 60, 0 } }, "garbage before the first status is skipped" );
    check( feed( { { 0x90, 60, 0xB0, 7, 100 } } ) == Messages{ { 0xB0, 7, 100 } }, "a message cut short by another status is dropped" );
    check( feed( { { 0xF0, 1, 2, 0x90, 60, 100 } } ) == Messages{ { 0x90, 60, 100 } }, "a SysEx cut short by a status is dropped; the status plays" );
    check( feed( { { 0xF2, 0x10, 0x20, 0xF1, 0x33, 0xF6, 0xF3, 4, 5 } } ) == Messages{ { 0xF2, 0x10, 0x20 }, { 0xF1, 0x33 }, { 0xF6 }, { 0xF3, 4 } },
           "system common messages, and no running status after them" );
    check( feed( { { 0xF4, 1, 0xF5, 2, 0xE0, 0, 64 } } ) == Messages{ { 0xE0, 0, 64 } }, "undefined system common bytes are skipped" );
    {
        std::vector<uint8_t> fits( MidiStream::maxSysEx, 0x11 ), long_( MidiStream::maxSysEx, 0x11 );
        fits.front() = 0xF0; fits.back() = 0xF7;
        long_.front() = 0xF0; long_.push_back( 0xF7 );
        check( feed( { fits } ) == Messages{ fits }, "a SysEx of maxSysEx bytes is kept" );
        MidiStream s;
        check( feed( s, { long_, { 0x90, 60, 100 } } ) == Messages{ { 0x90, 60, 100 } }, "an over-long SysEx is dropped whole, and the stream goes on" );
        check( feed( s, { { 61, 101 } } ) == Messages{ { 0x90, 61, 101 } }, "running status after it" );
    }
    if( failures ) return 1;
    std::printf( "midi stream: ok\n" );
    return 0;
}
