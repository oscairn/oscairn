// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The engine's ALSA raw MIDI input (RawMidi.h) without a sound card: its reader on pipes and a
// pty in place of devices. Messages from two sources, split across writes; a source that ends
// (a pipe closed) and one that fails (a pty's other side closed: EIO, as an unplugged device's
// ENODEV) are closed once and read no more, without spinning, and the engine is told to open the
// inputs again; the list of inputs changing tells it too, once; nothing changing tells it
// nothing. Then snd-seq-midi's port names, and the enumeration on a machine with no cards.
#include "RawMidi.h"

#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <fcntl.h>
#include <mutex>
#include <string>
#include <thread>
#include <unistd.h>
#include <vector>

using namespace oscairn::host;
using namespace std::chrono_literals;

namespace
{
    int failures = 0;
    void check( bool ok, const std::string& what ) { if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what.c_str() ); ++failures; } }

    struct Heard
    {
        std::mutex lock;
        std::condition_variable moved;
        std::vector<std::pair<size_t, std::vector<uint8_t>>> messages;
        std::vector<int> closed;
        int changed = 0;
        std::vector<std::string> ports{ "hw:1,0,0" }; // what a scan finds
        int changes() { std::lock_guard l( lock ); return changed; }
        template<class F> bool wait( F&& done ) { std::unique_lock l( lock ); return moved.wait_for( l, 2s, [&] { return done(); } ); }
    };
    void put( int fd, std::vector<uint8_t> bytes ) { check( ::write( fd, bytes.data(), bytes.size() ) == ssize_t( bytes.size() ), "write" ); }
    double cpuSeconds() { return double( std::clock() ) / CLOCKS_PER_SEC; }
}

int main()
{
    Heard heard;
    int a[2], b[2];
    check( ::pipe( a ) == 0 && ::pipe( b ) == 0, "pipes" );
    const int pty = ::posix_openpt( O_RDWR | O_NOCTTY );
    check( pty >= 0 && ::grantpt( pty ) == 0 && ::unlockpt( pty ) == 0, "a pty" );
    const int tty = ::open( ::ptsname( pty ), O_RDWR | O_NOCTTY );
    check( tty >= 0, "the pty's other side" );
    for( int fd : { a[0], b[0], pty } ) ::fcntl( fd, F_SETFL, O_NONBLOCK );
    auto source = [&]( int fd, int id ) {
        return RawMidiSource{ fd, [&heard, fd, id] { ::close( fd ); std::lock_guard l( heard.lock ); heard.closed.push_back( id ); heard.moved.notify_all(); } };
    };
    {
        RawMidiReader reader(
            { source( a[0], 0 ), source( b[0], 1 ), source( pty, 2 ) },
            [&]( size_t s, const uint8_t* bytes, size_t size ) { std::lock_guard l( heard.lock ); heard.messages.push_back( { s, { bytes, bytes + size } } ); heard.moved.notify_all(); },
            { "hw:1,0,0" }, [&] { std::lock_guard l( heard.lock ); return heard.ports; },
            [&] { std::lock_guard l( heard.lock ); ++heard.changed; heard.moved.notify_all(); }, 100ms );

        put( a[1], { 0x90, 60 } );
        put( b[1], { 0xB0, 1, 64 } );
        std::this_thread::sleep_for( 20ms );
        put( a[1], { 100, 62, 0xF8, 90 } );
        check( heard.wait( [&] { return heard.messages.size() == 4; } ), "four messages" );
        {
            std::lock_guard l( heard.lock );
            auto from = [&]( size_t s ) { std::vector<std::vector<uint8_t>> out; for( auto& [i, m] : heard.messages ) if( i == s ) out.push_back( m ); return out; };
            check( from( 0 ) == std::vector<std::vector<uint8_t>>{ { 0x90, 60, 100 }, { 0xF8 }, { 0x90, 62, 90 } }, "the first source's messages, joined across writes, with running status and a clock inside" );
            check( from( 1 ) == std::vector<std::vector<uint8_t>>{ { 0xB0, 1, 64 } }, "the second source's message, by its index" );
        }
        std::this_thread::sleep_for( 350ms );
        check( heard.changes() == 0, "nothing changing tells the engine nothing" );

        // A source that ends: closed once, the rest still read, and the engine told once.
        put( a[1], { 0x80, 60 } ); // cut short by the end
        ::close( a[1] );
        check( heard.wait( [&] { return heard.closed == std::vector<int>{ 0 }; } ), "an ended source is closed" );
        check( heard.wait( [&] { return heard.changed == 1; } ), "and the engine told to open the inputs again" );
        const double before = cpuSeconds();
        std::this_thread::sleep_for( 300ms );
        check( cpuSeconds() - before < 0.05, "an ended source is not polled again (no spin)" );
        put( b[1], { 2, 3 } );
        check( heard.wait( [&] { return heard.messages.size() == 5; } ) && heard.messages.back() == std::pair<size_t, std::vector<uint8_t>>{ 1, { 0xB0, 2, 3 } }, "another source still read" );

        // A source that fails mid-read, as a device unplugged.
        put( tty, { 0x90 } );
        std::this_thread::sleep_for( 20ms );
        ::close( tty );
        check( heard.wait( [&] { return heard.closed == std::vector<int>{ 0, 2 }; } ), "a failing source is closed" );
        std::this_thread::sleep_for( 300ms );
        check( heard.changes() == 1, "told once, not again" );
    }
    check( heard.closed == std::vector<int>{ 0, 2, 1 }, "the reader closes the source still open when it stops" );
    ::close( b[1] );

    {
        // Devices coming and going: the scan finds another, and the engine is told, once.
        Heard plugged;
        RawMidiReader reader( {}, []( size_t, const uint8_t*, size_t ) {}, { "hw:1,0,0" }, [&] { std::lock_guard l( plugged.lock ); return plugged.ports; },
                              [&] { std::lock_guard l( plugged.lock ); ++plugged.changed; plugged.moved.notify_all(); }, 50ms );
        std::this_thread::sleep_for( 200ms );
        check( plugged.changes() == 0, "the same inputs: not told" );
        { std::lock_guard l( plugged.lock ); plugged.ports.push_back( "hw:2,0,0" ); }
        check( plugged.wait( [&] { return plugged.changed == 1; } ), "an input plugged in tells the engine" );
        std::this_thread::sleep_for( 200ms );
        check( plugged.changes() == 1, "once" );
    }

    check( sequencerPortName( "SMK-37 MIDI 1", "SMK-37", 1, 2, 0, 0 ) == "SMK-37 MIDI 1", "a subdevice's name" );
    check( sequencerPortName( "", "Virtual Raw MIDI", 4, 1, 0, 2 ) == "Virtual Raw MIDI-2", "the device's name, numbered when it has several" );
    check( sequencerPortName( "", "UM-ONE", 1, 1, 0, 0 ) == "UM-ONE", "the device's name alone" );
    check( sequencerPortName( "", "", 1, 3, 1, 0 ) == "MIDI 3-1", "the last resort" );
    check( sequencerPortName( "", "", 2, 3, 1, 1 ) == "MIDI 3-1-1", "the last resort, numbered" );

    // On this machine, whatever it has: no crash. A container has no cards and no sequencer.
    const auto ports = rawMidiPorts();
    std::printf( "sequencer: %s; raw MIDI inputs: %zu\n", alsaSequencer() ? "yes" : "no", ports.size() );
    for( const auto& p : ports ) std::printf( "  %s  %s\n", p.identifier.c_str(), p.name.c_str() );

    if( failures ) return 1;
    std::printf( "raw midi: ok\n" );
    return 0;
}
