// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A device panel's slider in the engine started without the app, as oscairn-fantom-start starts
// it on the FANTOM-07 (docs/fantom-0.md): the null device, the startup Performance, and slider 1's
// panel messages (analog 9, 0 to 127) decoded through the shipped map by a PanelReader on a pipe
// and played as the engine's panel sink plays them. Without the profile's control groups, which
// only the app sent, `fader:1` names no control and nothing moves; with --device-profile
// fantom-07 the engine has them, and the slider plays the Synth part's level, picked up where it
// is.
//   oscairn-panel-startup-test PLUGIN-DIR MAP
#include "AudioEngine.h"
#include "oscairn/json_rpc.h"
#include "oscairn/panel_reader.h"

#include <juce_events/juce_events.h>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <sstream>
#include <string>
#include <unistd.h>

namespace
{
    int failures = 0;
    void check( bool ok, const std::string& what ) { std::fprintf( stderr, "%s: %s\n", ok ? "ok" : "FAIL", what.c_str() ); if( !ok ) ++failures; }
    void settle( int ms ) { juce::MessageManager::getInstance()->runDispatchLoopUntil( ms ); }

    struct Run { size_t groups = 0; int played = 0; double level = -1; };
    Run sweep( const std::string& plugins, const oscairn::PanelMap& map, bool profile )
    {
        oscairn::host::AudioConfiguration config;
        config.setOption( "--audio", "null" );
        if( profile ) config.setOption( "--device-profile", "fantom-07" );
        oscairn::host::AudioEngine engine( plugins, config );
        auto& session = dynamic_cast<oscairn::Session&>( engine.control() );
        oscairn::JsonRpc rpc( engine.control() );
        settle( 50 );
        Run run;
        const auto got = rpc.dispatch( R"({"jsonrpc":"2.0","id":1,"method":"controllers.get"})" ).value_or( "" );
        run.groups = size_t( juce::JSON::parse( got )["result"]["positions"].size() );
        int fds[2];
        if( ::pipe( fds ) != 0 ) { check( false, "a pipe" ); return run; }
        std::atomic<int> played = 0, heard = 0;
        {
            // The engine's panel sink (DeviceReaders.cpp's openPanel): a named control, not chatter, played by identity.
            oscairn::PanelReader reader( fds[0], map, [&]( const oscairn::PanelEvent& e ) {
                ++heard;
                if( !e.control.empty() && !e.chatter && session.controllerControl( e.control, e.input ) ) ++played;
            }, {} );
            // Slider 1 bottom to top and back to the middle, as the owner moved it.
            int sent = 0;
            auto slider = [&]( int value ) {
                const uint8_t m[4]{ 0x84, 9, uint8_t( ( value >> 7 ) << 2 ), uint8_t( value & 0x7F ) };
                if( ::write( fds[1], m, 4 ) == 4 ) ++sent;
            };
            for( int v = 0; v <= 127; v += 8 ) slider( v );
            slider( 127 );
            for( int v = 120; v >= 64; v -= 8 ) slider( v );
            for( int i = 0; i < 300 && heard < sent; ++i ) settle( 10 );
            check( heard == sent, "every slider message read: " + std::to_string( heard.load() ) + " of " + std::to_string( sent ) );
            settle( 100 ); // the control thread applies what the reader's thread played
        }
        ::close( fds[0] ); ::close( fds[1] );
        run.played = played;
        for( const auto& p : session.parts() ) if( p.name == "Synth" ) run.level = p.level;
        return run;
    }
}

int main( int argc, char** argv )
{
    if( argc != 3 ) { std::fprintf( stderr, "usage: oscairn-panel-startup-test PLUGIN-DIR MAP\n" ); return 2; }
    juce::ScopedJuceInitialiser_GUI juce;
    const auto root = juce::File::createTempFile( "oscairn-panel-startup" );
    root.getChildFile( "data" ).createDirectory(); root.getChildFile( "config" ).createDirectory();
    ::setenv( "OSCAIRN_DATA_HOME", root.getChildFile( "data" ).getFullPathName().toRawUTF8(), 1 );
    ::setenv( "OSCAIRN_CONFIG_HOME", root.getChildFile( "config" ).getFullPathName().toRawUTF8(), 1 );
    try
    {
        std::ifstream in( argv[2] ); std::stringstream text; text << in.rdbuf();
        const auto map = oscairn::PanelMap::parse( text.str() );
        const auto before = sweep( argv[1], map, false );
        check( before.groups == 0, "started without the app, the engine had no control groups: " + std::to_string( before.groups ) );
        check( before.played == 0 && std::abs( before.level - 1 ) < 1e-9, "so fader:1 played nothing and the Synth part stayed at " + std::to_string( before.level ) );
        const auto after = sweep( argv[1], map, true );
        check( after.groups == 18, "with --device-profile fantom-07, the profile's 18 groups: " + std::to_string( after.groups ) );
        check( after.played > 0 && std::abs( after.level - 64 / 127.0 ) < 1e-6,
               "slider 1 picked up at the top and plays the Synth part's level, down to 64 of 127: " + std::to_string( after.level ) + " (" + std::to_string( after.played ) + " moves played)" );
    }
    catch( const std::exception& e ) { check( false, e.what() ); }
    root.deleteRecursively();
    if( failures ) return 1;
    std::printf( "panel startup: the engine alone plays slider 1 on the Synth part's level\n" );
    return 0;
}
