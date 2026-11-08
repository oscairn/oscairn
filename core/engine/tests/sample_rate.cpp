// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A new sample rate (the audio device's changed): every plugin is activated again at it, in the
// engine and in workers, keeping its state, and never while it processes. Rendered offline:
// - the Basic Synth holding key 57 sounds at 220 Hz at 48, 44.1 and 96 kHz, in the engine and in
//   a worker, measured by its rising zero crossings at the rate rendered; a parameter set before
//   the change keeps its value;
// - the KICK fixture reports activating at each new rate, and no CLAP thread rule is broken while
//   a thread renders blocks throughout the changes;
// - a change asked for while a block is inside the plugin waits for the block to finish;
// - the Session's transport keeps its beat across a change, and its blocks go on.
//   oscairn-sample-rate-test WORKER BASIC-SYNTH.clap KICK-FIXTURE.clap
#include "oscairn/graph.h"
#include "oscairn/midi_input.h"
#include "oscairn/session.h"
#include "oscairn/transport.h"

#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <dlfcn.h>
#include <string>
#include <thread>
#include <vector>

namespace
{
    constexpr uint32_t block = 256;
    constexpr clap_id bendRangeParam = 9;
    int failures = 0;
    void check( bool ok, const std::string& what )
    {
        if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what.c_str() ); ++failures; }
    }

    // 0.5 s of key 57 held from the start, at the graph's rate; the frequency from 0.3 s on.
    double pitch( oscairn::Graph& g, double rate )
    {
        oscairn::Transport transport( rate, 120.0 );
        transport.setPlaying( false );
        std::vector<float> left( block ), right( block ), out;
        float* outputs[] = { left.data(), right.data() };
        const uint8_t on[] = { 0x90, 57, 100 }, off[] = { 0x80, 57, 0 };
        const auto total = size_t( rate / 2 );
        for( size_t pos = 0; pos < total; pos += block )
        {
            kick::clap::Event e;
            const bool first = pos == 0 && oscairn::midiToClap( on, 3, 0, e );
            const auto t = transport.current();
            if( !g.process( outputs, block, &t, first ? &e : nullptr, first ? 1 : 0 ) ) return 0;
            out.insert( out.end(), left.begin(), left.end() );
            transport.advance( block );
        }
        kick::clap::Event e; // released, so the next render starts from silence
        const auto t = transport.current();
        if( oscairn::midiToClap( off, 3, 0, e ) ) g.process( outputs, block, &t, &e, 1 );
        std::vector<double> crossings;
        for( size_t i = size_t( rate * .3 ); i < out.size(); ++i )
            if( out[i - 1] < 0 && out[i] >= 0 ) crossings.push_back( double( i ) - 1 + out[i - 1] / ( out[i - 1] - out[i] ) );
        return crossings.size() < 2 ? 0 : rate * double( crossings.size() - 1 ) / ( crossings.back() - crossings.front() );
    }

    // The synth at 48 kHz, then at each other rate, in the engine or in a worker.
    void synth( const std::string& worker, const std::string& path, bool inWorker )
    {
        const std::string where = inWorker ? "worker: " : "engine: ";
        oscairn::setPlacementPolicy( { inWorker ? worker : std::string(), oscairn::Placement::Worker } );
        // Offline: a worker's block waits for it, so a loaded machine cannot silence one.
        oscairn::Graph g( 48000, block, oscairn::ProcessingMode::Offline );
        const auto node = g.add( path );
        g.connectNotes( oscairn::graphInput, node );
        g.connectAudio( node, oscairn::graphOutput );
        g.commit();
        check( g.host( node ).inWorker() == inWorker, where + "placed as asked" );
        g.host( node ).setParameter( bendRangeParam, 12 );
        for( const double rate : { 48000.0, 44100.0, 96000.0, 48000.0 } )
        {
            g.setSampleRate( rate );
            const double f = pitch( g, rate );
            std::printf( "%s%.0f Hz: %.2f Hz\n", where.c_str(), rate, f );
            check( std::abs( f / 220 - 1 ) < .002, where + "key 57 is 220 Hz at " + std::to_string( int( rate ) ) + " Hz" );
            check( g.host( node ).parameterValue( bendRangeParam ) == 12, where + "the Pitch bend range set before the change is kept" );
        }
    }
}

int main( int argc, char** argv )
{
    if( argc != 4 ) { std::fprintf( stderr, "usage: sample-rate WORKER BASIC-SYNTH.clap KICK-FIXTURE.clap\n" ); return 2; }
    const std::string worker = argv[1], basic = argv[2], fixture = argv[3];
    synth( worker, basic, false );
    synth( worker, basic, true );
    oscairn::setPlacementPolicy( {} );

    void* module = dlopen( fixture.c_str(), RTLD_NOW );
    auto threadErrors = module ? reinterpret_cast<int ( * )()>( dlsym( module, "fixture_thread_errors" ) ) : nullptr;
    auto activated = module ? reinterpret_cast<double ( * )()>( dlsym( module, "fixture_activated_rate" ) ) : nullptr;
    auto blockProcess = module ? reinterpret_cast<void ( * )()>( dlsym( module, "fixture_block_process" ) ) : nullptr;
    auto entered = module ? reinterpret_cast<bool ( * )()>( dlsym( module, "fixture_entered_process" ) ) : nullptr;
    auto release = module ? reinterpret_cast<void ( * )()>( dlsym( module, "fixture_release_process" ) ) : nullptr;
    if( !threadErrors || !activated || !blockProcess || !entered || !release ) return std::fprintf( stderr, "FAIL: fixture diagnostics\n" ), 1;
    {
        oscairn::Graph g( 48000, block );
        const auto node = g.add( fixture, "org.kick.test.synth" );
        g.connectNotes( oscairn::graphInput, node );
        g.connectAudio( node, oscairn::graphOutput );
        g.commit();
        std::vector<float> left( block ), right( block );
        float* outputs[] = { left.data(), right.data() };
        check( activated() == 48000, "the fixture activates at 48 kHz" );

        // A thread renders blocks all along, as an audio device does, while the rate changes.
        std::atomic<bool> stop{ false };
        std::atomic<uint64_t> blocks{ 0 };
        std::thread audio( [&] { while( !stop ) { g.process( outputs, block, nullptr ); ++blocks; } } );
        for( int i = 0; i < 50; ++i )
        {
            const double rate = i % 2 ? 48000 : 44100;
            g.setSampleRate( rate );
            check( activated() == rate, "the fixture activates again at " + std::to_string( int( rate ) ) + " Hz" );
            std::this_thread::sleep_for( std::chrono::milliseconds( 1 ) );
        }
        stop = true;
        audio.join();
        std::printf( "fixture: 50 changes during %llu blocks, %d thread errors\n", (unsigned long long)blocks.load(), threadErrors() );
        check( blocks > 0, "blocks rendered during the changes" );

        // A block inside the plugin: the change, on the main thread as CLAP requires, waits for it.
        blockProcess();
        std::thread inside( [&] { g.process( outputs, block, nullptr ); } );
        while( !entered() ) std::this_thread::yield();
        std::atomic<bool> changed{ false }, early{ false };
        std::atomic<double> during{ 0 };
        std::thread releaser( [&] {
            std::this_thread::sleep_for( std::chrono::milliseconds( 200 ) );
            during = activated(); early = changed.load();
            release();
        } );
        g.setSampleRate( 96000 );
        changed = true;
        releaser.join();
        inside.join();
        check( !early && during == 48000, "a change waits for the block inside the plugin" );
        check( activated() == 96000, "and then activates the plugin at the new rate" );
        check( threadErrors() == 0, "no CLAP thread rule broken (activate on the main thread, never while processing)" );
    }

    // The Session: its transport keeps its beat; a block after the change renders.
    {
        oscairn::Session s( 48000, block );
        s.setPlaying( true );
        std::vector<float> left( block ), right( block );
        float* outputs[] = { left.data(), right.data() };
        for( int i = 0; i < 375; ++i ) s.process( outputs, block ); // 2 s at 120 BPM: beat 4
        const double before = s.transportState().beat;
        s.setSampleRate( 44100 );
        check( s.sampleRate() == 44100, "the Session's rate is the new one" );
        check( s.process( outputs, block ), "a Session block renders after the change" );
        const double after = s.transportState().beat;
        std::printf( "session: beat %.4f before the change, %.4f at the first block after\n", before, after );
        check( std::abs( after - ( before + block / 48000.0 * 2 ) ) < 1e-3, "the transport keeps its beat across the change" );
    }

    if( failures ) return 1;
    std::printf( "ok: plugins activated again at each new rate, in the engine and in workers, state kept, never mid-block\n" );
    return 0;
}
