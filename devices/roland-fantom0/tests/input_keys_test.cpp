// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The FANTOM-0's gpio keys (InputKeys.h): struct input_event records through a pipe, as evdev
// gives them (the unit's kernel has no uinput). MENU, TEMPO and WRITE pressed and released, by
// their functions; a key's autorepeat, a code with no function and the sync records passed over;
// a record split across two reads joined. A device name that is not there is said once.
#include "InputKeys.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <linux/input.h>
#include <mutex>
#include <string>
#include <thread>
#include <unistd.h>
#include <vector>

using namespace oscairn::host;

int main()
{
    int failures = 0;
    auto check = [&]( bool ok, const std::string& what ) { if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what.c_str() ); ++failures; } };
    const std::map<uint16_t, std::string> keys{ { 157, "write" }, { 139, "menu" }, { 152, "tempo" } };
    std::mutex lock;
    std::vector<std::string> heard;
    auto wait = [&]( size_t n ) { for( int i = 0; i < 300; ++i ) { { std::lock_guard g( lock ); if( heard.size() >= n ) return true; } std::this_thread::sleep_for( std::chrono::milliseconds( 10 ) ); } return false; };
    int fds[2];
    check( ::pipe( fds ) == 0, "a pipe" );
    {
        InputKeys reader( fds[0], keys, [&]( const std::string& control, const oscairn::ControlInput& in ) {
            std::lock_guard g( lock );
            heard.push_back( control + ( in.kind == oscairn::ControlInput::Kind::Switch ? in.value >= .5 ? " on" : " off" : " ?" ) );
        } );
        auto record = []( uint16_t type, uint16_t code, int32_t value ) { input_event e{}; e.type = type; e.code = code; e.value = value; return e; };
        auto put = [&]( const std::vector<input_event>& records ) { check( ::write( fds[1], records.data(), records.size() * sizeof( input_event ) ) == ssize_t( records.size() * sizeof( input_event ) ), "write" ); };
        const auto syn = record( EV_SYN, SYN_REPORT, 0 );
        put( { record( EV_KEY, 139, 1 ), syn, record( EV_KEY, 139, 2 ), syn, record( EV_KEY, 139, 0 ), syn } ); // MENU: pressed, repeated, let go
        put( { record( EV_KEY, 30, 1 ), syn, record( EV_MSC, MSC_SCAN, 139 ) } );                            // a key with no function, a scan code
        const auto tempo = record( EV_KEY, 152, 1 );                                                           // TEMPO, in two reads
        const auto* bytes = reinterpret_cast<const char*>( &tempo );
        check( ::write( fds[1], bytes, 10 ) == 10, "the first part" );
        std::this_thread::sleep_for( std::chrono::milliseconds( 50 ) );
        check( ::write( fds[1], bytes + 10, sizeof tempo - 10 ) == ssize_t( sizeof tempo - 10 ), "the rest" );
        put( { syn, record( EV_KEY, 152, 0 ), record( EV_KEY, 157, 1 ), record( EV_KEY, 157, 0 ), syn } );   // TEMPO let go, WRITE pressed and let go
        check( wait( 6 ), "six key events heard" );
        std::this_thread::sleep_for( std::chrono::milliseconds( 50 ) );
        std::lock_guard g( lock );
        std::string got;
        for( const auto& h : heard ) got += h + "; ";
        check( heard == std::vector<std::string>{ "menu on", "menu off", "tempo on", "tempo off", "write on", "write off" }, "by function, press and release, nothing else: " + got );
    }
    ::close( fds[1] ); ::close( fds[0] );

    // By name: none in an empty folder, said once while it looks each second.
    char folder[] = "/tmp/oscairn-input-XXXXXX";
    check( ::mkdtemp( folder ) != nullptr, "a folder" );
    std::vector<std::string> log;
    {
        InputKeys reader( "gpio-keys", keys, {}, [&]( const std::string& line ) { std::lock_guard g( lock ); log.push_back( line ); }, folder );
        std::this_thread::sleep_for( std::chrono::milliseconds( 1300 ) );
    }
    check( log.size() == 1 && log[0].find( "no input device named gpio-keys" ) != std::string::npos, "a missing device said once: " + ( log.empty() ? std::string() : log[0] ) );
    ::rmdir( folder );
    if( failures ) { std::fprintf( stderr, "%d failure(s)\n", failures ); return 1; }
    std::puts( "input keys: ok" );
    return 0;
}
