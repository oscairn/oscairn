// SPDX-License-Identifier: AGPL-3.0-or-later
//
// An audio device that stops calling back (its thread ended on a failed write, as JUCE's ALSA
// device's does, and as on the FANTOM-07 when a burst of kernel messages froze the DMA) is opened
// again by the engine, the Performance playing on as it was; one that will not open
// again is tried three times at once, then every 10 s, said in the status meanwhile. On the test's
// null devices (--audio null, played at real-time pace by their own threads): nothing reaches a
// speaker, and no MIDI device is opened.
//   oscairn-audio-recovery-test PLUGIN-DIR
#include "AudioEngine.h"
#include "NullDevice.h"
#include "oscairn/json_rpc.h"

#include <juce_events/juce_events.h>
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <unistd.h>

namespace
{
    int failures = 0;
    void check( bool ok, const std::string& what ) { if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what.c_str() ); ++failures; } }

    struct Dying;
    Dying* latest = nullptr; // the device opened last
    int opened = 0;
    bool refuse = false; // the next opens fail, as a device that is not back yet
    struct Dying : oscairn::host::NullDevice
    {
        Dying( const juce::String& name ) : NullDevice( name, { 48000 }, { 128 }, 2, 0, true ) { latest = this; }
        ~Dying() override { close(); if( latest == this ) latest = nullptr; }
        juce::String open( const juce::BigInteger& inputs, const juce::BigInteger& outputs, double rate, int size ) override
        {
            if( refuse ) return "not back yet";
            ++opened;
            return NullDevice::open( inputs, outputs, rate, size );
        }
    };
    juce::var call( oscairn::JsonRpc& rpc, const std::string& method, const std::string& params = "{}" )
    {
        return juce::JSON::parse( juce::String( rpc.dispatch( R"({"jsonrpc":"2.0","id":1,"method":")" + method + R"(","params":)" + params + "}" ).value_or( "" ) ) );
    }
    void wait( int ms ) { juce::MessageManager::getInstance()->runDispatchLoopUntil( ms ); }
}

int main( int argc, char** argv )
{
    if( argc < 2 ) { std::fprintf( stderr, "usage: oscairn-audio-recovery-test PLUGIN-DIR\n" ); return 2; }
    char folder[] = "/tmp/oscairn-audio-recovery-XXXXXX";
    if( !::mkdtemp( folder ) ) return 2;
    ::setenv( "OSCAIRN_DATA_HOME", folder, 1 );
    ::setenv( "OSCAIRN_CONFIG_HOME", folder, 1 );
    juce::ScopedJuceInitialiser_GUI juce;
    try
    {
        oscairn::host::AudioConfiguration null;
        null.setOption( "--audio", "null" );
        auto type = std::make_unique<oscairn::host::NullType>( juce::StringArray{ "null" }, juce::StringArray{}, []( const juce::String& name ) -> juce::AudioIODevice* { return new Dying( name ); } );
        oscairn::host::AudioEngine engine( argv[1], null, std::move( type ) );
        oscairn::JsonRpc rpc( engine.control() );
        auto beat = [&] { return double( call( rpc, "transport.get" )["result"]["beat"] ); };
        auto status = [&] { return call( rpc, "session.status" )["result"]; };
        auto notices = [&] { return juce::JSON::toString( status()["notices"] ).toStdString(); };
        call( rpc, "transport.set", R"({"tempo":120,"playing":true})" );
        wait( 300 );
        const int first = opened;
        const double before = beat();
        wait( 300 );
        check( first == 1 && beat() > before && bool( status()["audioReady"] ), "the device plays" );

        const auto graph = juce::JSON::toString( call( rpc, "graph.get" )["result"] );
        // Its thread ends: within a second it is opened again and plays on.
        latest->freeze();
        wait( 1500 );
        const double after = beat();
        wait( 300 );
        check( opened == 2 && beat() > after, "a device that stopped calling back is opened again and plays: opened " + std::to_string( opened ) );
        check( juce::JSON::toString( call( rpc, "graph.get" )["result"] ) == graph, "the Performance as it was, its plugins kept" );
        check( bool( status()["audioReady"] ) && notices().find( "stopped and was opened again (1 try)" ) != std::string::npos, "and the status says so: " + notices() );

        // One that will not open again: three tries at once, half a second apart, then every 10 s.
        refuse = true;
        latest->freeze();
        wait( 2500 );
        const auto meanwhile = status();
        check( !bool( meanwhile["audioReady"] ) && juce::JSON::toString( meanwhile["notices"] ).contains( "opening it again, try 3" ), "tried three times, said meanwhile: " + juce::JSON::toString( meanwhile["notices"] ).toStdString() );
        wait( 4000 );
        check( juce::JSON::toString( status()["notices"] ).contains( "try 3" ), "then not for 10 s" );
        refuse = false;
        wait( 7000 );
        const double back = beat();
        wait( 300 );
        check( opened == 3 && beat() > back && bool( status()["audioReady"] ) && notices().find( "(4 tries)" ) != std::string::npos, "and back at the slow try once it opens: " + notices() );
    }
    catch( const std::exception& e ) { std::fprintf( stderr, "FAIL: %s\n", e.what() ); ++failures; }
    std::filesystem::remove_all( folder );
    if( failures ) return 1;
    std::printf( "ok: a device that stops calling back is opened again, the Performance playing on\n" );
    return 0;
}
