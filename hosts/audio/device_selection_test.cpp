// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The audio device chosen through the control API (audio.devices, audio.set), on a fake device
// type the engine opens instead of the system's, so nothing reaches a speaker. The test plays the
// device's callbacks itself. Checks: the listing; a new rate activating the plugins again at it
// (key 57 sounds at 220 Hz at 96 kHz); invalid rates, buffer sizes and devices refused; a held note
// playing on across a switch and released after it; a device that fails to open leaving the last
// one playing; the choice restored at the next start, the profile's device and then the default
// when the device chosen is missing; a profile's fixed device; devices coming and going announced.
//   oscairn-audio-devices-test PLUGIN-DIR [WORKER]
#include "AudioEngine.h"
#include "oscairn/json_rpc.h"

#include <juce_events/juce_events.h>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <memory>
#include <string>
#include <vector>
#include <unistd.h>

namespace
{
    int failures = 0;
    void check( bool ok, const std::string& what )
    {
        if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what.c_str() ); ++failures; }
    }

    struct FakeDevice;
    FakeDevice* playing = nullptr; // the device started, whose callbacks the test plays

    struct FakeDevice : juce::AudioIODevice
    {
        FakeDevice( const juce::String& name, juce::Array<double> rates, juce::Array<int> sizes )
            : juce::AudioIODevice( name, "Fake" ), rates_( rates ), sizes_( sizes ) {}
        ~FakeDevice() override { close(); }
        juce::StringArray getOutputChannelNames() override { return { "Left", "Right" }; }
        juce::StringArray getInputChannelNames() override { return {}; }
        juce::Array<double> getAvailableSampleRates() override { return rates_; }
        juce::Array<int> getAvailableBufferSizes() override { return sizes_; }
        int getDefaultBufferSize() override { return sizes_[0]; }
        juce::String open( const juce::BigInteger&, const juce::BigInteger& outputs, double rate, int size ) override
        {
            if( getName() == "Broken" ) return "the fake device refuses to open";
            outputs_ = outputs & juce::BigInteger( 3 ); rate_ = rate; size_ = size; open_ = true;
            return {};
        }
        void close() override { stop(); open_ = false; }
        bool isOpen() override { return open_; }
        void start( juce::AudioIODeviceCallback* callback ) override
        {
            if( !callback || !open_ ) return;
            callback->audioDeviceAboutToStart( this );
            callback_ = callback; playing = this;
        }
        void stop() override
        {
            if( auto* c = std::exchange( callback_, nullptr ) ) c->audioDeviceStopped();
            if( playing == this ) playing = nullptr;
        }
        bool isPlaying() override { return callback_ != nullptr; }
        juce::String getLastError() override { return {}; }
        int getCurrentBufferSizeSamples() override { return size_; }
        double getCurrentSampleRate() override { return rate_; }
        int getCurrentBitDepth() override { return 32; }
        juce::BigInteger getActiveOutputChannels() const override { return outputs_; }
        juce::BigInteger getActiveInputChannels() const override { return {}; }
        int getOutputLatencyInSamples() override { return 0; }
        int getInputLatencyInSamples() override { return 0; }
        // One callback's left channel, appended to `out`.
        void render( std::vector<float>& out )
        {
            std::vector<float> left( static_cast<size_t>( size_ ) ), right( static_cast<size_t>( size_ ) );
            float* outputs[] = { left.data(), right.data() };
            callback_->audioDeviceIOCallbackWithContext( nullptr, 0, outputs, 2, size_, {} );
            out.insert( out.end(), left.begin(), left.end() );
        }
        juce::Array<double> rates_;
        juce::Array<int> sizes_;
        juce::BigInteger outputs_;
        double rate_ = 0;
        int size_ = 0;
        bool open_ = false;
        juce::AudioIODeviceCallback* callback_ = nullptr;
    };

    struct FakeType : juce::AudioIODeviceType
    {
        explicit FakeType( juce::StringArray names ) : juce::AudioIODeviceType( "Fake" ), names_( names ) {}
        void scanForDevices() override {}
        juce::StringArray getDeviceNames( bool input ) const override { return input ? juce::StringArray() : names_; }
        int getDefaultDeviceIndex( bool ) const override { return 0; }
        int getIndexOfDevice( juce::AudioIODevice* device, bool input ) const override { return input || !device ? -1 : names_.indexOf( device->getName() ); }
        bool hasSeparateInputsAndOutputs() const override { return true; }
        juce::AudioIODevice* createDevice( const juce::String& output, const juce::String& ) override
        {
            if( !names_.contains( output ) ) return nullptr;
            if( output == "Headphones" ) return new FakeDevice( output, { 48000 }, { 128, 256 } );
            return new FakeDevice( output, { 44100, 48000, 96000 }, { 64, 128, 256, 512 } );
        }
        void change( juce::StringArray names ) { names_ = names; callDeviceChangeListeners(); } // devices plugged in or out
        juce::StringArray names_;
    };

    juce::var call( oscairn::JsonRpc& rpc, const std::string& method, const std::string& params = "{}" )
    {
        const auto reply = rpc.dispatch( R"({"jsonrpc":"2.0","id":1,"method":")" + method + R"(","params":)" + params + "}" );
        return juce::JSON::parse( juce::String( reply.value_or( "" ) ) );
    }
    int code( const juce::var& reply ) { return int( reply["error"]["code"] ); }
    std::string text( const juce::var& v ) { return v.toString().toStdString(); }

    // Seconds of the device's left channel, playing its callbacks.
    std::vector<float> render( double seconds )
    {
        std::vector<float> out;
        if( !playing ) return out;
        while( out.size() < size_t( seconds * playing->rate_ ) ) playing->render( out );
        return out;
    }
    double rms( const std::vector<float>& x, size_t from )
    {
        double sum = 0;
        for( size_t i = from; i < x.size(); ++i ) sum += double( x[i] ) * x[i];
        return x.size() > from ? std::sqrt( sum / double( x.size() - from ) ) : 0;
    }
    // The frequency between 150 and 400 Hz whose period repeats best (autocorrelation), from 0.2 s on.
    double pitch( const std::vector<float>& x, double rate )
    {
        const size_t from = size_t( rate * .2 ), length = size_t( rate * .1 );
        if( x.size() < from + length + size_t( rate / 150 ) ) return 0;
        double best = -1; size_t lag = 0;
        for( size_t l = size_t( rate / 400 ); l <= size_t( rate / 150 ); ++l )
        {
            double sum = 0, a = 0, b = 0;
            for( size_t i = from; i < from + length; ++i ) { sum += double( x[i] ) * x[i + l]; a += double( x[i] ) * x[i]; b += double( x[i + l] ) * x[i + l]; }
            const double r = sum / std::sqrt( a * b + 1e-30 );
            if( r > best ) { best = r; lag = l; }
        }
        return rate / double( lag );
    }
    std::vector<std::string> names( const juce::var& list )
    {
        std::vector<std::string> out;
        for( const auto& d : *list.getArray() ) out.push_back( text( d["name"] ) );
        return out;
    }
}

int main( int argc, char** argv )
{
    if( argc < 2 ) { std::fprintf( stderr, "usage: oscairn-audio-devices-test PLUGIN-DIR [WORKER]\n" ); return 2; }
    const std::string plugins = argv[1];
    if( argc > 2 ) ::setenv( "OSCAIRN_PLUGIN_WORKER", argv[2], 1 ); // every plugin in a worker process
    char folder[] = "/tmp/oscairn-audio-devices-XXXXXX";
    if( !::mkdtemp( folder ) ) return 2;
    ::setenv( "OSCAIRN_DATA_HOME", folder, 1 ); // the saved choice: this test's, never the user's
    juce::ScopedJuceInitialiser_GUI juce;
    const juce::StringArray all{ "Studio", "Headphones", "Broken" };
    try
    {
        {
            oscairn::host::AudioEngine engine( plugins, {}, std::make_unique<FakeType>( all ) );
            oscairn::JsonRpc rpc( engine.control() );
            auto devices = call( rpc, "audio.devices" )["result"];
            check( names( devices["outputs"] ) == std::vector<std::string>{ "Studio", "Headphones", "Broken" }, "every output device is listed" );
            check( devices["inputs"].getArray()->isEmpty() && text( devices["input"] ).empty(), "no input devices for an engine that opens no inputs" );
            check( text( devices["output"] ) == "Studio" && text( devices["source"] ) == "default" && !bool( devices["fixed"] ), "the default device, not fixed" );
            const auto studio = devices["outputs"][0];
            check( studio["sampleRates"].size() == 3 && double( studio["sampleRates"][2] ) == 96000 && studio["bufferSizes"].size() == 4, "a device's rates and buffer sizes" );
            check( devices["outputs"][1]["sampleRates"].size() == 1 && devices["outputs"][1]["bufferSizes"].size() == 2, "another device's, asked without opening it" );

            // A new rate: every plugin activated again at it, key 57 still at 220 Hz.
            auto set = call( rpc, "audio.set", R"({"sampleRate":96000})" )["result"];
            juce::MessageManager::getInstance()->runDispatchLoopUntil( 20 ); // a service tick: the status reports it
            check( double( set["sampleRate"] ) == 96000 && double( call( rpc, "session.status" )["result"]["sampleRate"] ) == 96000, "the device runs at 96 kHz" );
            check( static_cast<oscairn::Session&>( engine.control() ).sampleRate() == 96000, "the Session runs at 96 kHz" );
            check( ( argc > 2 ) == !call( rpc, "session.status" )["result"]["workers"].getArray()->isEmpty(), "the plugins run in workers only when given one" );
            call( rpc, "graph.enable", R"({"node":"delay","enabled":false})" );
            call( rpc, "notes.send", R"({"on":true,"key":57,"velocity":0.8})" );
            const auto at96 = render( .5 );
            const auto measured = pitch( at96, 96000 );
            check( std::abs( measured - 220 ) < 3, "key 57 at 220 Hz at 96 kHz, measured " + std::to_string( measured ) );
            call( rpc, "notes.send", R"({"on":false,"key":57})" );
            render( 1 );

            // Refused, with nothing changed.
            for( const auto* bad : { R"({"sampleRate":12345})", R"({"bufferSize":100})", R"({"output":"Nowhere"})", R"({"input":"Microphone"})", R"({"rate":48000})", R"({"output":"Headphones","sampleRate":96000})" } )
                check( code( call( rpc, "audio.set", bad ) ) == -32602, std::string( "refused: " ) + bad );
            const auto refused = call( rpc, "audio.set", R"({"sampleRate":12345})" );
            check( text( refused["error"]["message"] ) == "12345 Hz is not a sample rate Studio offers (44100, 48000, 96000)", "the refusal says what is offered: " + text( refused["error"]["message"] ) );
            check( text( call( rpc, "audio.devices" )["result"]["output"] ) == "Studio" && playing && playing->rate_ == 96000, "a refusal changes nothing" );

            // A switch with a note held: silent between, then the note plays on and is released.
            check( double( call( rpc, "audio.set", R"({"sampleRate":48000,"bufferSize":256})" )["result"]["bufferSize"] ) == 256, "the buffer size set" );
            call( rpc, "notes.send", R"({"on":true,"key":57,"velocity":0.8})" );
            const auto held = rms( render( .3 ), size_t( 48000 * .1 ) );
            const auto list = call( rpc, "session.status" )["result"]["audioDeviceList"];
            set = call( rpc, "audio.set", R"({"output":"Headphones"})" )["result"];
            check( text( set["output"] ) == "Headphones" && text( set["source"] ) == "user" && double( set["sampleRate"] ) == 48000 && double( set["bufferSize"] ) == 256, "switched to Headphones" );
            check( playing && playing->getName() == "Headphones", "Headphones plays" );
            const auto after = rms( render( .2 ), 0 );
            check( held > .01 && after > held * .5, "the held note plays on after the switch: " + std::to_string( held ) + ", " + std::to_string( after ) );
            call( rpc, "notes.send", R"({"on":false,"key":57})" );
            const auto released = rms( render( 2 ), size_t( 48000 * 1.8 ) );
            check( released < held * .01, "the note is released after the switch: " + std::to_string( released ) );

            // A device that fails to open: the last one plays again and the error says so.
            const auto failed = call( rpc, "audio.set", R"({"output":"Broken"})" );
            check( code( failed ) == -32005 && text( failed["error"]["message"] ).find( "Broken did not open: the fake device refuses to open; Headphones plays again" ) == 0,
                   "a failing device is reported: " + text( failed["error"]["message"] ) );
            check( text( call( rpc, "audio.devices" )["result"]["output"] ) == "Headphones" && playing && playing->getName() == "Headphones", "the last device opened again" );
            juce::MessageManager::getInstance()->runDispatchLoopUntil( 20 );
            check( bool( call( rpc, "session.status" )["result"]["audioReady"] ) && text( call( rpc, "session.status" )["result"]["audioDevice"] ) == "Headphones", "and plays" );
            call( rpc, "notes.send", R"({"on":true,"key":60,"velocity":0.8})" );
            check( rms( render( .3 ), size_t( 48000 * .1 ) ) > .01, "it sounds" );
            call( rpc, "notes.send", R"({"on":false,"key":60})" );
        }
        const auto saved = std::filesystem::path( folder ) / "audio.json";
        check( std::filesystem::exists( saved ), "the choice is saved in the data folder" );

        // The next start opens the saved choice.
        {
            oscairn::host::AudioEngine engine( plugins, {}, std::make_unique<FakeType>( all ) );
            oscairn::JsonRpc rpc( engine.control() );
            const auto devices = call( rpc, "audio.devices" )["result"];
            check( text( devices["output"] ) == "Headphones" && text( devices["source"] ) == "user" && double( devices["sampleRate"] ) == 48000 && double( devices["bufferSize"] ) == 256 &&
                   text( devices["notice"] ).empty(), "the saved choice restored at the next start" );
        }
        // The device chosen is missing: the profile's, then the default, each saying why.
        {
            oscairn::host::AudioConfiguration profile;
            profile.setOption( "--output-device", "Studio" );
            oscairn::host::AudioEngine engine( plugins, profile, std::make_unique<FakeType>( juce::StringArray{ "Studio", "Broken" } ) );
            oscairn::JsonRpc rpc( engine.control() );
            const auto devices = call( rpc, "audio.devices" )["result"];
            check( text( devices["output"] ) == "Studio" && text( devices["source"] ) == "profile" && text( devices["notice"] ).find( "the device chosen Headphones did not open" ) == 0,
                   "a missing choice falls back to the profile's device: " + text( devices["notice"] ) );
        }
        {
            oscairn::host::AudioConfiguration profile;
            profile.setOption( "--output-device", "Gone" );
            auto fake = std::make_unique<FakeType>( juce::StringArray{ "Studio" } );
            auto* type = fake.get();
            oscairn::host::AudioEngine engine( plugins, profile, std::move( fake ) );
            oscairn::JsonRpc rpc( engine.control() );
            auto devices = call( rpc, "audio.devices" )["result"];
            check( text( devices["output"] ) == "Studio" && text( devices["source"] ) == "default" && text( devices["notice"] ).find( "the profile's device Gone did not open" ) != std::string::npos,
                   "then the default: " + text( devices["notice"] ) );
            // Devices coming or going reach status subscribers as audioDeviceList.
            const auto before = double( call( rpc, "session.status" )["result"]["audioDeviceList"] );
            type->change( { "Studio", "Headphones" } );
            juce::MessageManager::getInstance()->runDispatchLoopUntil( 100 );
            check( double( call( rpc, "session.status" )["result"]["audioDeviceList"] ) > before, "a device coming changes audioDeviceList" );
            check( names( call( rpc, "audio.devices" )["result"]["outputs"] ) == std::vector<std::string>{ "Studio", "Headphones" }, "and is listed" );
        }
        // A profile's device with its channel map is fixed: the only one, and a choice of another is refused.
        {
            oscairn::host::AudioConfiguration profile;
            profile.setOption( "--output-device", "Studio" );
            profile.setOption( "--audio-outputs", "0,1" );
            profile.setOption( "--sample-rate", "48000" );
            oscairn::host::AudioEngine engine( plugins, profile, std::make_unique<FakeType>( all ) );
            oscairn::JsonRpc rpc( engine.control() );
            const auto devices = call( rpc, "audio.devices" )["result"];
            check( bool( devices["fixed"] ) && text( devices["source"] ) == "profile" && names( devices["outputs"] ) == std::vector<std::string>{ "Studio" } &&
                   devices["outputs"][0]["sampleRates"].size() == 1, "the profile's device and rate are the only ones" );
            check( code( call( rpc, "audio.set", R"({"output":"Headphones"})" ) ) == -32602, "another device refused" );
            check( code( call( rpc, "audio.set", R"({"sampleRate":96000})" ) ) == -32602, "another rate refused" );
            check( double( call( rpc, "audio.set", R"({"bufferSize":128})" )["result"]["bufferSize"] ) == 128, "its buffer size can change" );
        }
    }
    catch( const std::exception& e ) { std::fprintf( stderr, "FAIL: %s\n", e.what() ); ++failures; }
    std::filesystem::remove_all( folder );
    if( failures ) return 1;
    std::printf( "audio devices: listed, set, refused, fallen back, restored and fixed\n" );
    return 0;
}
