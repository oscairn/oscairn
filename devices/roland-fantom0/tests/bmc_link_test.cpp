// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The FANTOM-0's message link (BmcLink.h) without the instrument: frames built here, never bytes
// from a unit. The parser: the keys' notes on and off from their own frames, MIDI from the other
// sources from MIDI words, long frames skipped, a stream split across reads, a start at every byte
// of a stream and after garbage finding its place, with Local Switch on (each key also as one MIDI
// word per zone) and off (the keys' own frames and the status frames only), and a long frame of an
// absurd length losing it. Then the reader on a pty in place of /dev/ttyGS0: raw with echo off
// once open, nothing sent back, the zones' MIDI words ignored, a key with Local Switch off read,
// what comes in its first moments dropped, a held note ended when the device ends, and no spinning
// while it is gone.
//
// With OSCAIRN_BMC_CAPTURE naming a capture of a unit's link (kept off the repository), it also
// parses that from every byte offset of its first 2 KiB and prints the counts.
#include "BmcLink.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <fcntl.h>
#include <fstream>
#include <iterator>
#include <poll.h>
#include <termios.h>
#include <unistd.h>
#include <vector>

using namespace oscairn::host;
using namespace std::chrono_literals;
using Bytes = std::vector<uint8_t>;
using Messages = std::vector<Bytes>;

namespace
{
    int failures = 0;
    void check( bool ok, const std::string& what ) { if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what.c_str() ); ++failures; } }

    Bytes shortFrame( uint8_t kind, uint8_t from, uint8_t to, Bytes payload )
    {
        Bytes f{ uint8_t( kind << 1 ), from, to, 0, 0, 0, 0, 0, 0, 0, 0, uint8_t( payload.size() ) };
        std::copy( payload.begin(), payload.end(), f.begin() + 4 );
        return f;
    }
    // A key as the keyboard sends it: with Local Switch on, the note Roland's engine plays for it
    // in each of `zones` zones (a MIDI word each, on the zone's channel), then the key's own frame;
    // with it off (zones 0), the key's own frame alone.
    Bytes key( bool on, uint8_t note, uint8_t velocity, int zones = 3 )
    {
        Bytes all;
        for( int z = 0; z < zones; ++z )
        {
            auto f = shortFrame( 3, 0x02, 0x61, { uint8_t( on ? 0x29 : 0x28 ), uint8_t( ( on ? 0x90 : 0x80 ) | z ), note, velocity } );
            f[8] = note; // a stale byte past the count, as a unit leaves there
            all.insert( all.end(), f.begin(), f.end() );
        }
        const auto own = shortFrame( 6, 0x02, 0x60, { uint8_t( on ? 0x88 : 0x87 ), note, 0, velocity } );
        all.insert( all.end(), own.begin(), own.end() );
        return all;
    }
    Bytes longFrame( uint32_t length, uint8_t fill = 0 )
    {
        Bytes f{ 9 << 1 | 1, 0xE1, 0x60, 9, 0x10, 0x20, 0x30, 0x40, 0, 0, 0, 0, uint8_t( length ), uint8_t( length >> 8 ), uint8_t( length >> 16 ), uint8_t( length >> 24 ) };
        f.resize( 16 + length, fill );
        for( uint32_t i = 0; i < length; ++i ) f[16 + i] = uint8_t( fill + i * 7 ); // payload that is not all one byte
        return f;
    }
    Bytes join( std::initializer_list<Bytes> parts ) { Bytes all; for( const auto& p : parts ) all.insert( all.end(), p.begin(), p.end() ); return all; }

    // The MIDI messages a parser finds in `stream`, fed `chunk` bytes at a time, as the reader takes
    // them: the keys from their own frames, other sources from MIDI words.
    Messages midi( const Bytes& stream, size_t chunk = 4096, BmcLinkParser* keep = nullptr )
    {
        BmcLinkParser local;
        auto& parser = keep ? *keep : local;
        Messages out;
        for( size_t at = 0; at < stream.size(); at += chunk )
            parser.feed( stream.data() + at, std::min( chunk, stream.size() - at ), [&]( const BmcFrame& f ) {
                uint8_t m[3];
                if( const auto n = f.from() == 0x02 ? bmcKey( f, m ) : bmcMidi( f, m ) ) out.emplace_back( m, m + n );
            } );
        return out;
    }
    bool endsWith( const Messages& all, const Messages& tail ) { return tail.size() <= all.size() && std::equal( tail.begin(), tail.end(), all.end() - tail.size() ); }

    // Starting at every byte of `stream`: whatever is found is the stream's own tail, never a place
    // lost; the most messages missed from a start inside the first frame.
    bool everyStart( const Bytes& stream, const Messages& played, size_t& worst )
    {
        bool tails = true;
        worst = 0;
        for( size_t start = 0; start < stream.size(); ++start )
        {
            BmcLinkParser q;
            const auto found = midi( Bytes( stream.begin() + long( start ), stream.end() ), 7, &q );
            tails = tails && endsWith( played, found ) && q.stats().lost == 0;
            if( start < 12 ) worst = std::max( worst, played.size() - found.size() );
        }
        return tails;
    }

    void parser()
    {
        const Messages played{ { 0x90, 60, 100 }, { 0x80, 60, 40 }, { 0x90, 64, 1 }, { 0x80, 64, 127 } };
        const auto stream = join( { key( true, 60, 100 ), longFrame( 112 ), key( false, 60, 40 ), longFrame( 3000, 0x80 ), key( true, 64, 1 ), longFrame( 0 ), key( false, 64, 127 ) } );
        BmcLinkParser p;
        check( midi( stream, 4096, &p ) == played, "Local Switch on: one note per key, with release velocity, from its own frame; the zones' words and long frames passed over" );
        check( p.stats().frames == 19 && p.stats().skipped == 0 && p.stats().lost == 0, "every frame counted, nothing skipped from the start" );
        check( midi( stream, 1 ) == played && midi( stream, 5 ) == played && midi( stream, 13 ) == played, "frames split across reads join up" );
        size_t worst = 0;
        check( everyStart( stream, played, worst ), "started at any byte, only the stream's own messages come out" );
        check( worst <= 1, "started inside the first frame, at most that key is lost" );

        // Local Switch off: no MIDI word at all, only the keys' own frames and the status frames.
        const auto quiet = join( { longFrame( 112 ), key( true, 60, 100, 0 ), longFrame( 112 ), key( false, 60, 40, 0 ), longFrame( 112 ), key( true, 64, 1, 0 ), key( false, 64, 127, 0 ), longFrame( 112 ) } );
        BmcLinkParser o;
        check( midi( quiet, 4096, &o ) == played && o.stats().skipped == 0 && o.stats().lost == 0, "Local Switch off: the same notes" );
        check( everyStart( quiet, played, worst ) && worst <= 1, "and started at any byte, the place found on the keys' and status frames alone" );

        Bytes garbage{ 0x06, 0x02, 0x61, 0x00, 0x29, 0x91 }; // a frame's start, cut off
        for( int i = 0; i < 300; ++i ) garbage.push_back( uint8_t( i * 37 + 11 ) );
        BmcLinkParser g;
        check( midi( join( { garbage, stream } ), 64, &g ) == played, "garbage before the stream is skipped" );
        check( g.stats().skipped == garbage.size(), "and counted" );

        // A long frame of an absurd length loses the place; the next key finds it again.
        Bytes absurd = longFrame( 0 );
        absurd[12] = absurd[13] = absurd[14] = absurd[15] = 0xFF;
        BmcLinkParser a;
        check( midi( join( { key( true, 60, 100 ), absurd, key( false, 60, 40 ), key( true, 64, 1 ) } ), 4096, &a ) == Messages{ { 0x90, 60, 100 }, { 0x80, 60, 40 }, { 0x90, 64, 1 } },
               "an absurd length is not followed" );
        check( a.stats().lost == 1, "it loses the place once" );

        uint8_t m[3];
        BmcFrame f;
        const auto word = shortFrame( 3, 0x81, 0x61, { 0x0B, 0xB0, 7, 99 } );
        std::copy( word.begin(), word.end(), f.raw.begin() ); f.length = 4;
        check( bmcMidi( f, m ) == 3 && m[0] == 0xB0 && m[1] == 7 && m[2] == 99, "a control change" );
        f.raw[4] = 0x0C; f.raw[5] = 0xC2; check( bmcMidi( f, m ) == 2 && m[0] == 0xC2 && m[1] == 7, "a program change: one data byte" );
        f.raw[4] = 0x0E; f.raw[5] = 0xE0; check( bmcMidi( f, m ) == 3, "a pitch bend" );
        f.raw[4] = 0x09; f.raw[5] = 0xB0; check( bmcMidi( f, m ) == 0, "a code index that disagrees with the status is not MIDI" );
        f.raw[0] = 6 << 1; f.raw[4] = 0x0B; check( bmcMidi( f, m ) == 0, "a panel word is not MIDI" );
        f.raw[1] = 0x02; f.raw[4] = 0x88; f.raw[5] = 70; f.raw[6] = 0; f.raw[7] = 33;
        check( bmcKey( f, m ) == 3 && m[0] == 0x90 && m[1] == 70 && m[2] == 33, "a key's own frame: note on, channel 1" );
        f.raw[4] = 0x87; check( bmcKey( f, m ) == 3 && m[0] == 0x80 && m[2] == 33, "and off, with its release velocity" );
        f.raw[1] = 0x75; check( bmcKey( f, m ) == 0, "a panel word from elsewhere is no key" );
    }

    // Every frame in a unit's capture: the keys from their own frames, and the place, from offset `start`.
    void capture( const char* path )
    {
        std::ifstream file( path, std::ios::binary );
        const Bytes all{ std::istreambuf_iterator<char>( file ), {} };
        check( !all.empty(), "the capture reads" );
        auto run = [&]( size_t start, BmcLinkParser& p, size_t& keys ) {
            Messages out;
            keys = 0;
            p.feed( all.data() + start, all.size() - start, [&]( const BmcFrame& f ) {
                uint8_t m[3];
                if( const auto n = bmcKey( f, m ) ) { out.emplace_back( m, m + n ); ++keys; }
            } );
            return out;
        };
        BmcLinkParser first;
        size_t keys = 0;
        const auto played = run( 0, first, keys );
        std::printf( "capture: %zu bytes, %llu frames, %zu notes from the keys' own frames, %llu bytes skipped, %llu places lost\n", all.size(),
                     (unsigned long long)first.stats().frames, keys, (unsigned long long)first.stats().skipped, (unsigned long long)first.stats().lost );
        check( keys == 122 && first.stats().skipped == 0 && first.stats().lost == 0, "the capture: 122 notes from the keys, nothing skipped, the place never lost" );
        // Where each key's own frame starts, walking the headers from offset 0 (where the place is known).
        std::vector<size_t> at;
        for( size_t i = 0; i + 12 <= all.size(); )
        {
            if( all[i] & 1 ) { i += 16 + ( size_t( all[i + 12] ) | size_t( all[i + 13] ) << 8 | size_t( all[i + 14] ) << 16 | size_t( all[i + 15] ) << 24 ); continue; }
            if( all[i] == 6 << 1 && all[i + 1] == 0x02 && all[i + 11] == 4 && ( all[i + 4] == 0x88 || all[i + 4] == 0x87 ) ) at.push_back( i );
            i += 12;
        }
        check( at.size() == played.size(), "the walk finds every key's frame" );
        bool exact = true; uint64_t lost = 0, skipped = 0;
        for( size_t start = 1; start < std::min<size_t>( 2048, all.size() ); ++start )
        {
            BmcLinkParser p;
            size_t k = 0;
            const auto found = run( start, p, k );
            const auto after = size_t( at.end() - std::lower_bound( at.begin(), at.end(), start ) );
            exact = exact && found.size() == after && endsWith( played, found );
            lost += p.stats().lost;
            skipped = std::max<uint64_t>( skipped, p.stats().skipped );
        }
        std::printf( "capture from offsets 1 to 2047: the most bytes skipped %llu, places lost %llu, every key whose frame starts at or after the offset found and nothing else: %s\n",
                     (unsigned long long)skipped, (unsigned long long)lost, exact ? "yes" : "no" );
        check( exact && lost == 0, "the capture from any of its first 2 KiB of offsets gives exactly its own messages from there" );
    }

    void reader()
    {
        const int master = ::posix_openpt( O_RDWR | O_NOCTTY );
        check( master >= 0 && ::grantpt( master ) == 0 && ::unlockpt( master ) == 0, "a pty" );
        const std::string path = ::ptsname( master );
        std::mutex lock;
        Messages heard; std::vector<size_t> inputs; std::vector<std::string> logged;
        auto wait = [&]( auto done ) { for( auto until = std::chrono::steady_clock::now() + 3s; std::chrono::steady_clock::now() < until; std::this_thread::sleep_for( 5ms ) ) { std::lock_guard l( lock ); if( done() ) return true; } return false; };
        auto put = [&]( const Bytes& b ) { check( ::write( master, b.data(), b.size() ) == ssize_t( b.size() ), "write to the pty" ); };
        auto quietMaster = [&] { pollfd p{ master, POLLIN, 0 }; return ::poll( &p, 1, 100 ) == 0; };
        std::chrono::steady_clock::time_point stopping;
        {
            BmcLinkReader reader( path, [&]( size_t input, const uint8_t* b, size_t n ) { std::lock_guard l( lock ); heard.emplace_back( b, b + n ); inputs.push_back( input ); },
                                  [&]( const std::string& line ) { std::lock_guard l( lock ); logged.push_back( line ); }, 300ms );
            check( wait( [&] { return reader.stats().opened == 1; } ), "the reader opens the device" );
            termios t{};
            check( ::tcgetattr( master, &t ) == 0 && !( t.c_lflag & ( ECHO | ECHOE | ECHOK | ECHONL | ICANON ) ) && !( t.c_iflag & ( IXON | IXOFF ) ), "raw, with echo and flow control off, once open" );
            put( key( true, 50, 90 ) ); // queued before it settled: stale
            check( wait( [&] { return reader.stats().stale == 1; } ) && heard.empty(), "a key in its first moments is dropped" );
            std::this_thread::sleep_for( 400ms );
            put( join( { longFrame( 112 ), key( true, 60, 100 ), longFrame( 112 ), key( false, 60, 40 ), key( true, 62, 70 ), shortFrame( 3, 0x81, 0x61, { 0x0B, 0xB1, 64, 127 } ) } ) );
            check( wait( [&] { return heard.size() == 4; } ), "then each key is one message, the zones' words ignored" );
            put( join( { key( true, 65, 80, 0 ), key( false, 65, 30, 0 ) } ) ); // Local Switch off
            check( wait( [&] { return heard.size() == 6; } ), "a key with Local Switch off is read too" );
            {
                std::lock_guard l( lock );
                check( heard == Messages{ { 0x90, 60, 100 }, { 0x80, 60, 40 }, { 0x90, 62, 70 }, { 0xB1, 64, 127 }, { 0x90, 65, 80 }, { 0x80, 65, 30 } } &&
                           inputs == std::vector<size_t>{ 0, 0, 0, 1, 0, 0 },
                       "the keys as FANTOM keys on channel 1, MIDI from 0x81 as its own input" );
                check( std::any_of( logged.begin(), logged.end(), []( const std::string& s ) { return s.find( "dropped 1 MIDI messages" ) != std::string::npos; } ), "the drop is logged" );
            }
            put( shortFrame( 5, 0x02, 0x60, { 1, 2 } ) );
            check( wait( [&] { return reader.stats().unknown == 1 && !logged.empty() && logged.back().find( "unknown frame: 0a 02 60 00 01 02" ) != std::string::npos; } ),
                   "an unknown frame is counted and logged with its header" );
            check( quietMaster(), "nothing is sent back to the other side" );
            check( bmcInputName( 0 ) == "FANTOM keys" && bmcInputName( 1 ) == "FANTOM MIDI 0x81", "the inputs' names" );

            ::close( master ); // the device ends
            check( wait( [&] { return heard.size() == 8; } ), "a device that ends ends the notes held" );
            {
                std::lock_guard l( lock );
                check( heard.size() == 8 && heard[6] == Bytes{ 0x80, 62, 0 } && heard[7] == Bytes{ 0xB1, 64, 0 }, "note off for the held key, sustain off for the held pedal" );
            }
            const auto cpu = double( std::clock() ) / CLOCKS_PER_SEC;
            std::this_thread::sleep_for( 1500ms );
            check( double( std::clock() ) / CLOCKS_PER_SEC - cpu < 0.2, "no spinning while the device is gone" );
            stopping = std::chrono::steady_clock::now();
        }
        check( std::chrono::steady_clock::now() - stopping < 300ms, "it stops promptly, in its pause too" );
    }
}

int main()
{
    parser();
    if( const char* path = std::getenv( "OSCAIRN_BMC_CAPTURE" ) ) capture( path );
    else std::printf( "OSCAIRN_BMC_CAPTURE unset: the capture check is skipped\n" );
    reader();
    if( failures ) return 1;
    std::printf( "ok: the link's frames, their place found again, and its reader\n" );
    return 0;
}
