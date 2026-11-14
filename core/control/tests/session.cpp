// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/session.h"
#include "oscairn/midi_input.h"
#include "oscairn/realtime.h"
#include "AllocationGuard.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <limits>
#include <stdexcept>
#include <thread>
#include <vector>
#include <unistd.h>

namespace
{
    constexpr double rate = 48000;
    constexpr uint32_t block = 512;
    int failures = 0;

    void check( bool ok, const char* what )
    {
        if( !ok )
        {
            std::fprintf( stderr, "FAIL: %s\n", what );
            ++failures;
        }
    }

    template<class F> void rejects( F&& f, const char* what )
    {
        try { f(); check( false, what ); }
        catch( const std::invalid_argument& ) {}
    }

    struct Audio
    {
        std::array<float, block> left{}, right{};
        float* out[2]{ left.data(), right.data() };
    };

    bool render( oscairn::Session& session, Audio& audio, uint32_t frames = block,
                 std::span<const kick::clap::Event> events = {},
                 std::span<const oscairn::Session::MidiClockEvent> clocks = {} )
    {
        forbidAllocation = true;
        const bool ok = session.process( audio.out, frames, events, clocks );
        forbidAllocation = false;
        return ok;
    }

    oscairn::NodeId instrument( oscairn::Session& s, const char* path )
    {
        const auto id = s.add( "synth", path );
        s.connectNotes( oscairn::graphInput, id );
        s.connectAudio( id, oscairn::graphOutput );
        s.commit();
        return id;
    }

    void contract( const char* synth )
    {
        rejects( [] { oscairn::Session bad( 0, block ); }, "zero sample rate" );
        rejects( [] { oscairn::Session bad( rate, 0 ); }, "zero block size" );
        oscairn::Session session( rate, block );
        auto& control = session.control();
        const auto id = instrument( session, synth );
        check( control.nodeId( "synth" ) == id, "named and numeric node addressing agree" );
        check( control.parameters( id ).size() == 14, "all synth parameters, including oscillators, pitch LFO, wheels and envelope, are exposed" );
        const auto pressure = control.parameterInfo( id, 3 );
        check( pressure.name == "Pressure" && pressure.defaultValue == 0.5 && pressure.maximum == 1,
               "parameter metadata crosses the interface" );
        check( control.parameterText( id, 3, 0.5 ) == "50%", "plugin parameter formatting" );
        rejects( [&] { session.add( "synth", synth ); }, "duplicate name rejected before loading" );
        rejects( [&] { control.nodeId( "missing" ); }, "unknown node" );
        rejects( [&] { control.setParameter( id, 12345, 1 ); }, "unknown parameter" );
        rejects( [&] { control.setParameter( id, 3, std::numeric_limits<double>::quiet_NaN() ); }, "NaN parameter" );
        rejects( [&] { control.setTempo( 0 ); }, "zero tempo" );
        rejects( [&] { control.setTempo( std::numeric_limits<double>::infinity() ); }, "infinite tempo" );
        rejects( [&] { control.note( true, 128, 1 ); }, "out-of-range key" );
        rejects( [&] { control.note( true, 60, 1, 16 ); }, "out-of-range channel" );
        check( !control.hasParameter( 99999, 1 ), "hasParameter on missing node" );

        Audio audio;
        check( render( session, audio ), "empty instrument renders" );
        check( std::all_of( audio.left.begin(), audio.left.end(), [](float v) { return v == 0; } ), "no unsolicited notes" );
        check( control.note( true, 57, 0.8, 2 ), "UI note queued" );
        check( render( session, audio ), "queued note renders while transport is stopped" );
        check( std::any_of( audio.left.begin(), audio.left.end(), [](float v) { return std::abs(v) > 0.01f; } ), "UI note is audible" );

        // A lost note-off must not leave a UI note sounding forever. Fill the queue
        // with unrelated note-offs, then force the held note's release to overflow.
        for( size_t i = 0; i < oscairn::Session::noteQueueCapacity; ++i )
            check( control.note( false, 40, 0, 2 ), "queue fills to its documented capacity" );
        check( !control.note( false, 57, 0, 2 ) && session.droppedNotes() == 1, "overflow is reported" );
        // The synth has an exponential release; allow two seconds for its tail.
        for( uint32_t samples = 0; samples < 2 * (uint32_t)rate; samples += block )
            check( render( session, audio ), "overflow recovery renders" );
        check( std::all_of( audio.left.begin(), audio.left.end(), [](float v) { return std::abs(v) < 1e-6f; } ),
               "overflow releases the held note" );
        check( control.note( true, 60, 1 ) && render( session, audio ), "queue recovers after overflow" );
        check( std::any_of( audio.left.begin(), audio.left.end(), [](float v) { return std::abs(v) > 0.01f; } ),
               "new notes play after overflow" );

        std::array<kick::clap::Event, oscairn::Session::maxInputEvents + 1> tooMany{};
        check( !render( session, audio, block, tooMany ), "over-capacity input rejected" );
        check( std::all_of( audio.left.begin(), audio.left.end(), [](float v) { return v == 0; } ), "rejected render clears audio" );
    }

    // Compare the new session against the existing Graph + Transport path using real
    // plugins, uneven blocks, parameter locks, tempo changes and sample-offset pressure.
    // With the load guard off: this tests rendering, and the reference Graph has no guard, so a
    // machine too slow for real time (a sanitizer build) would otherwise have notes given up.
    void referenceRender( const char* seq, const char* synth, const char* delay )
    {
        {
            oscairn::Session usual( rate, block ); usual.service();
            check( usual.loadGuard(), "a Session's load guard is on by default" );
        }
        setenv( "OSCAIRN_LOAD_GUARD", "off", 1 );
        oscairn::Session session( rate, block );
        unsetenv( "OSCAIRN_LOAD_GUARD" );
        session.service();
        check( !session.loadGuard(), "OSCAIRN_LOAD_GUARD=off switches it off" );
        oscairn::Graph reference( rate, block );
        oscairn::Transport transport( rate );
        const auto a = session.add( "sequencer", seq );
        const auto b = session.add( "synth", synth );
        const auto c = session.add( "delay", delay );
        session.connectNotes( a, b ); session.connectNotes( oscairn::graphInput, b );
        session.connectAudio( b, c ); session.connectAudio( c, oscairn::graphOutput );
        session.mapParameter( a, 0, b, 1 ); session.commit();
        const auto ra = reference.add( seq ), rb = reference.add( synth ), rc = reference.add( delay );
        reference.connectNotes( ra, rb ); reference.connectNotes( oscairn::graphInput, rb );
        reference.connectAudio( rb, rc ); reference.connectAudio( rc, oscairn::graphOutput );
        reference.mapParamTarget( ra, 0, rb, 1 ); reference.commit();
        auto& control = session.control();
        control.setPlaying( true );
        Audio actual, expected;
        double energy = 0;
        bool identical = true;
        const uint32_t sizes[] = { 512, 37, 128, 251 };
        for( int n = 0; n < 600; ++n )
        {
            const auto frames = sizes[n % 4];
            std::vector<kick::clap::Event> events, referenceEvents;
            if( n == 4 )
            {
                control.note( true, 57, 0.7, 2 );
                referenceEvents.push_back( kick::clap::Event::note( true, 57, 0.7, 0, 2 ) );
            }
            if( n == 5 || n == 8 )
            {
                const uint8_t poly[] = { 0xA2, 57, 100 }, channel[] = { 0xD2, 30 };
                kick::clap::Event e;
                check( oscairn::midiToClap( n == 5 ? poly : channel, n == 5 ? 3 : 2, 17, e ), "pressure conversion" );
                events.push_back( e );
            }
            if( n == 30 )
            {
                control.note( false, 57, 0, 2 );
                referenceEvents.push_back( kick::clap::Event::note( false, 57, 0, 0, 2 ) );
            }
            if( n == 100 )
            {
                control.setTempo( 137 ); transport.setTempo( 137 );
                check( control.setParameter( b, 3, 0.8 ) && reference.host( rb ).setParameter( 3, 0.8 ), "pressure amount change" );
            }
            if( n == 400 ) { control.setPlaying( false ); transport.setPlaying( false ); }
            referenceEvents.insert( referenceEvents.end(), events.begin(), events.end() );
            const auto t = transport.current();
            session.service();
            check( render( session, actual, frames, events ), "session renders demo graph" );
            check( reference.process( expected.out, frames, &t, referenceEvents.data(), (uint32_t)referenceEvents.size() ), "reference renders" );
            identical &= std::equal( actual.left.begin(), actual.left.begin() + frames, expected.left.begin() )
                      && std::equal( actual.right.begin(), actual.right.begin() + frames, expected.right.begin() );
            for( uint32_t i = 0; i < frames; ++i ) energy += actual.left[i] * actual.left[i];
            const auto state = control.transportState();
            check( state.playing == transport.playing() && state.tempo == t.tempo
                && state.beat == (double)t.song_pos_beats / (double)CLAP_BEATTIME_FACTOR, "transport telemetry matches rendered block" );
            transport.advance( frames );
            if( n % 10 == 0 ) session.service();
        }
        check( identical && energy > 1, "session audio matches existing graph exactly, including control changes and pressure" );
    }

    void midiClock()
    {
        oscairn::Session session( rate, block );
        auto& control = session.control();
        Audio audio;
        control.setClockSource( oscairn::Transport::ClockSource::MidiClock );
        check( render( session, audio ), "MIDI sync starts without a pulse" );
        check( !control.transportState().playing, "MIDI sync waits for start and clock" );
        const std::array<oscairn::Session::MidiClockEvent, 2> start{{ {0xFA, 0}, {0xF8, 1} }};
        check( render( session, audio, block, {}, start ), "clock source applied before incoming start" );
        check( control.transportState().playing, "external clock starts transport" );
        // One pulse every 1000 samples is 120 BPM at 48 kHz; timestamps are already past.
        for( int i = 0; i < 3; ++i ) check( render( session, audio ), "advance to next clock" );
        const std::array<oscairn::Session::MidiClockEvent, 2> pulses{{ {0xF8, 1001}, {0xF8, 2001} }};
        check( render( session, audio, block, {}, pulses ) && control.transportState().tempo == 120, "timestamped MIDI tempo" );
        const std::array<oscairn::Session::MidiClockEvent, 1> stop{{ {0xFC, 2500} }};
        check( render( session, audio, block, {}, stop ) && !control.transportState().playing, "external stop" );
        control.setTempo( 95 ); control.setPlaying( true );
        control.setClockSource( oscairn::Transport::ClockSource::Internal );
        check( render( session, audio ) && control.transportState().playing && control.transportState().tempo == 95,
               "switching to internal restores requested transport" );
    }

    // RFC 0012 step 2: clock out, measured offline from the timestamps made. At 133 BPM a pulse
    // falls every 902.26 samples, between samples; over blocks of uneven sizes each pulse lands
    // within half a sample of its ideal time, so the jitter is the rounding alone and nothing
    // drifts. Start from the top, Stop, Song Position and Continue; only to the outputs chosen.
    void clockOut()
    {
        using namespace oscairn;
        Session session( rate, block );
        auto& control = session.control();
        Control::SessionStatus status;
        status.midiOutputs = { { MidiEndpoint::Kind::Usb, 3, "TR-8S" }, { MidiEndpoint::Kind::Usb, 4, "SMK-37 Elite -Master" } };
        session.setHostStatus( status );
        control.setClockOutputs( { { DeviceRule::Kind::UsbNamed, DeviceRule::Match::Contains, 0, "tr-8" } } );
        check( control.transportState().clockOutputs.size() == 1, "the clock's outputs read back" );
        control.setTempo( 133 );
        control.setPlaying( true );
        Audio audio;
        struct Sent { int64_t time; std::array<uint8_t, 3> bytes; uint8_t size; uint64_t ports = 0; };
        std::vector<Sent> sent;
        int64_t now = 0;
        bool chosen = true;
        auto run = [&]( uint32_t frames ) {
            check( render( session, audio, frames ), "render with clock out" );
            for( const auto& m : session.midiOutput() ) { sent.push_back( { now + m.time, m.bytes, m.size, m.ports } ); chosen = chosen && m.ports == 1u << 3; }
            now += frames;
        };
        for( uint32_t i = 0; i < 400; ++i ) run( 37 + ( i * 97 ) % 476 );
        const double period = 60.0 * rate / ( 133 * 24 );
        check( sent.size() > 2 && sent[0].bytes[0] == 0xFA && sent[0].time == 0 && sent[0].size == 1, "Start first, at the top" );
        double worst = 0;
        int64_t pulses = 0;
        bool ordered = true;
        for( size_t i = 1; i < sent.size(); ++i )
        {
            ordered = ordered && sent[i].bytes[0] == 0xF8 && sent[i].size == 1;
            worst = std::max( worst, std::abs( (double)sent[i].time - pulses * period ) );
            ++pulses;
        }
        check( ordered && chosen, "then only clock, to the TR-8S only" );
        check( pulses == (int64_t)std::floor( ( now - 1 ) / period ) + 1, "24 pulses a beat, none missed or doubled" );
        std::printf( "clock out: %lld pulses over %lld samples, worst %.3f samples from ideal\n", (long long)pulses, (long long)now, worst );
        check( worst <= 0.5 + 1e-6, "each pulse within half a sample of its ideal time" );

        sent.clear();
        control.setPlaying( false );
        run( 100 ); run( 100 );
        check( sent.size() == 1 && sent[0].bytes[0] == 0xFC, "Stop once, then nothing while stopped" );
        sent.clear();
        const double beats = (double)( now - 200 ) / ( 60.0 * rate / 133 );
        control.setPlaying( true );
        run( 64 );
        const auto sixteenths = (uint16_t)std::floor( beats * 4 );
        check( sent.size() >= 2 && sent[0].bytes[0] == 0xF2 && sent[0].size == 3 && ( sent[0].bytes[1] | sent[0].bytes[2] << 7 ) == sixteenths && sent[1].bytes[0] == 0xFB,
               "playing again from where it stopped: Song Position, then Continue" );
        check( std::all_of( sent.begin() + 2, sent.end(), []( const Sent& m ) { return m.bytes[0] == 0xF8; } ), "then clock" );

        sent.clear();
        control.setClockOutputs( {} );
        run( 512 );
        check( sent.size() == 1 && sent[0].bytes[0] == 0xFC && sent[0].ports == 1u << 3 && control.transportState().clockOutputs.empty(),
               "no outputs chosen: the device it left gets Stop, then no clock" );
        sent.clear();
        run( 512 );
        check( sent.empty(), "and nothing after" );
        // Outputs chosen while the transport plays start the device: Song Position and Continue,
        // not bare pulses (RFC 0012 step 2).
        control.setClockOutputs( { { DeviceRule::Kind::UsbNamed, DeviceRule::Match::Contains, 0, "tr-8" } } );
        run( 64 );
        check( sent.size() >= 2 && sent[0].bytes[0] == 0xF2 && sent[0].size == 3 && sent[1].bytes[0] == 0xFB &&
               std::all_of( sent.begin(), sent.end(), []( const Sent& m ) { return m.ports == 1u << 3; } ),
               "an output picked mid-song gets Song Position, then Continue" );
        check( std::all_of( sent.begin() + 2, sent.end(), []( const Sent& m ) { return m.bytes[0] == 0xF8; } ), "then clock to the new output" );
        // Switching to another device while playing starts that one too.
        control.setClockOutputs( { { DeviceRule::Kind::UsbNamed, DeviceRule::Match::Contains, 0, "smk" } } );
        sent.clear();
        run( 64 );
        // What went to the outputs in mask, other than clock pulses.
        auto to = [&]( uint64_t mask ) {
            std::vector<uint8_t> out;
            for( const auto& m : sent ) if( ( m.ports & mask ) && m.bytes[0] != 0xF8 ) out.push_back( m.bytes[0] );
            return out;
        };
        check( to( 1u << 4 ) == std::vector<uint8_t>{ 0xF2, 0xFB } && to( 1u << 3 ) == std::vector<uint8_t>{ 0xFC },
               "the next device is started as well, and the one it left is stopped" );
        // Adding a device while playing starts that one only: the one already running keeps its
        // place and gets just clock. Removing a device stops that one only.
        control.setClockOutputs( { { DeviceRule::Kind::UsbNamed, DeviceRule::Match::Contains, 0, "smk" },
                                   { DeviceRule::Kind::UsbNamed, DeviceRule::Match::Contains, 0, "tr-8" } } );
        sent.clear();
        for( int i = 0; i < 8; ++i ) run( 256 );
        check( to( 1u << 3 ) == std::vector<uint8_t>{ 0xF2, 0xFB } && to( 1u << 4 ).empty(), "a second device: Song Position and Continue to it alone" );
        check( std::count_if( sent.begin(), sent.end(), []( const Sent& m ) { return m.bytes[0] == 0xF8 && m.ports == ( 1u << 3 | 1u << 4 ); } ) > 0,
               "then clock to both" );
        control.setClockOutputs( { { DeviceRule::Kind::UsbNamed, DeviceRule::Match::Contains, 0, "smk" } } );
        sent.clear();
        for( int i = 0; i < 8; ++i ) run( 256 );
        check( to( 1u << 3 ) == std::vector<uint8_t>{ 0xFC } && to( 1u << 4 ).empty(), "a device removed: Stop to it alone" );
        rejects( [&] { control.setClockOutputs( { { DeviceRule::Kind::UsbNamed, DeviceRule::Match::Regex, 0, "(" } } ); }, "an invalid pattern" );
    }

    void concurrentControl( const char* synth )
    {
        oscairn::Session session( rate, block );
        const auto id = instrument( session, synth );
        auto& control = session.control();
        std::atomic<bool> run{ true }, ok{ true };
        std::atomic<uint32_t> blocks{ 0 };
        std::thread audioThread( [&] {
            Audio audio;
            while( run.load() )
            {
                if( !render( session, audio, 64 ) ) ok = false;
                for( auto v : audio.left ) if( !std::isfinite(v) ) ok = false;
                ++blocks;
            }
        } );
        while( blocks.load() == 0 ) std::this_thread::yield();
        for( int i = 0; i < 1000; ++i )
        {
            control.setPlaying( i % 2 ); control.setTempo( 80 + i % 80 );
            control.setParameter( id, 1, (i % 100) / 100.0 );
            control.note( i % 2 == 0, 60, 0.8 );
            control.transportState();
            session.service();
        }
        run = false;
        audioThread.join();
        check( ok && blocks > 0, "concurrent control and rendering remain valid" );
    }

    // Parameter-info revisions go on past those an earlier engine reserved, whatever the clock says.
    void revisionsNeverRepeat( const char* synth )
    {
        const auto home = std::filesystem::temp_directory_path() / ( "oscairn-session-revisions-" + std::to_string( getpid() ) );
        std::filesystem::create_directories( home );
        setenv( "OSCAIRN_DATA_HOME", home.c_str(), 1 );
        const uint64_t ahead = uint64_t( 1 ) << 52; // the clock's microseconds reach it in 2112: as if the clock went back since
        std::ofstream( home / "parameter-revision" ) << ahead << "\n";
        uint64_t first = 0;
        {
            oscairn::Session session( rate, block );
            const auto id = instrument( session, synth );
            first = session.graphState().parameterRevisions.at( id );
            check( first > ahead, "a new engine's parameter revisions start past those reserved before, not from the clock" );
        }
        {
            oscairn::Session session( rate, block );
            const auto id = instrument( session, synth );
            check( session.graphState().parameterRevisions.at( id ) > first, "and the next engine's past the last one's" );
        }
        unsetenv( "OSCAIRN_DATA_HOME" );
        std::filesystem::remove_all( home );
    }

    // The app's start (RFC 0028): the Performance last opened or saved comes back, once per engine;
    // one that cannot open leaves Untitled, saying why.
    void reopensLast( const char* synth )
    {
        namespace fs = std::filesystem;
        const auto home = fs::temp_directory_path() / ( "oscairn-session-reopen-" + std::to_string( getpid() ) );
        fs::remove_all( home );
        fs::create_directories( home );
        setenv( "OSCAIRN_DATA_HOME", home.c_str(), 1 );
        const auto last = home / "library" / "last.json", saved = home / "Saved", other = home / "Other";
        auto read = []( const fs::path& file ) { std::ifstream in( file ); return std::string( std::istreambuf_iterator<char>( in ), {} ); };
        auto write = []( const fs::path& file, const std::string& text ) { std::ofstream( file, std::ios::trunc ) << text; };
        auto remembers = [&]( const fs::path& folder ) { return read( last ).find( fs::weakly_canonical( folder ).string() ) != std::string::npos; };
        auto nodes = []( const oscairn::Session& s ) { std::vector<oscairn::NodeId> ids; for( const auto& [id, name] : s.graphState().names ) ids.push_back( id ); return ids; };
        auto edit = [&]( const fs::path& file, const std::string& from, const std::string& to ) {
            auto text = read( file ); const auto at = text.find( from ); check( at != std::string::npos, "the saved document holds what the test changes" );
            if( at != std::string::npos ) write( file, text.replace( at, from.size(), to ) ); };
        {
            oscairn::Session session( rate, block );
            instrument( session, synth );
            session.savePerformance( saved.string() );
            check( !fs::exists( last ), "an engine never asked to reopen remembers nothing in the data folder" );
        }
        {
            oscairn::Session session( rate, block );
            check( session.reopenPerformance().empty() && nodes( session ).empty(), "with nothing remembered, Untitled stays and nothing is said" );
            session.loadPerformance( saved.string() );
            check( remembers( saved ), "once asked, an open is remembered" );
            session.savePerformance( other.string() );
            check( remembers( other ), "and a save" );
        }
        {
            oscairn::Session session( rate, block );
            check( session.reopenPerformance().empty() && nodes( session ).size() == 1, "the next start opens the last one saved" );
            const auto opened = nodes( session );
            check( session.reopenPerformance().empty() && nodes( session ) == opened, "an engine still running when the app comes back keeps it, not reloaded" );
            check( session.history().undo.empty(), "undo starts from the reopened Performance" );
            check( remembers( other ), "reopening it leaves it remembered" );
        }
        {
            oscairn::Session session( rate, block );
            session.loadPerformance( saved.string() );
            const auto opened = nodes( session );
            check( session.reopenPerformance().empty() && nodes( session ) == opened && remembers( other ), "nor is one that opened something before the app asked" );
        }
        fs::remove_all( other );
        {
            oscairn::Session session( rate, block );
            const auto notice = session.reopenPerformance();
            check( notice.find( "Other" ) != std::string::npos && notice.find( "no longer there" ) != std::string::npos && nodes( session ).empty(), "a missing one leaves Untitled, saying why" );
            const auto notices = session.sessionStatus().notices;
            check( std::find( notices.begin(), notices.end(), notice ) != notices.end(), "and the session's notices say so too" );
        }
        const auto broken = home / "Broken";
        fs::copy( saved, broken, fs::copy_options::recursive );
        edit( broken / "performance.json", "\"version\":13", "\"version\":99" );
        write( last, "{\"performance\":\"" + broken.string() + "\"}\n" );
        {
            oscairn::Session session( rate, block );
            check( session.reopenPerformance().find( "unsupported format version" ) != std::string::npos && nodes( session ).empty(), "one in a format this build does not read leaves Untitled, saying why" );
        }
        const auto missing = home / "Missing";
        fs::copy( saved, missing, fs::copy_options::recursive );
        const auto document = read( missing / "performance.json" );
        const auto hint = document.find( "\"pathHint\":\"" ) + 12;
        edit( missing / "performance.json", document.substr( hint, document.find( '"', hint ) - hint ), ( home / "gone.clap" ).string() );
        write( last, "{\"performance\":\"" + missing.string() + "\"}\n" );
        {
            oscairn::Session session( rate, block );
            const auto notice = session.reopenPerformance();
            check( notice.find( "not installed" ) != std::string::npos && nodes( session ).empty(), "one needing a plugin that is not here leaves Untitled, not stand-ins, saying why" );
            session.loadPerformance( missing.string() );
            check( nodes( session ).size() == 1, "while opening it by hand still gives its stand-in (RFC 0006)" );
        }
        unsetenv( "OSCAIRN_DATA_HOME" );
        fs::remove_all( home );
    }
}

namespace
{
    // engine.timing's histogram (timing.h): its buckets, percentiles, reset and startup window.
    void histogram()
    {
        using oscairn::Histogram;
        check( Histogram::bucket( -5 ) == 0 && Histogram::bucket( 15'999 ) == 0 && Histogram::bucket( 16'000 ) == 1 && Histogram::bucket( 31'999 ) == 1 &&
               Histogram::bucket( 32'000 ) == 2 && Histogram::bucket( 2'666'667 ) == 8 && Histogram::bucket( 65'535'999 ) == 12 &&
               Histogram::bucket( 65'536'000 ) == 13 && Histogram::bucket( int64_t( 1 ) << 40 ) == 13, "buckets by powers of two from 16 us, then overflow" );
        check( Histogram::upperMicroseconds( 0 ) == 16 && Histogram::upperMicroseconds( 8 ) == 4096 && Histogram::upperMicroseconds( 12 ) == 65536, "bucket bounds" );
        Histogram h;
        check( h.percentileMicroseconds( .5 ) == 0, "an empty histogram's percentiles are 0" );
        h.counts[1] = 98; h.counts[5] = 1; h.counts[13] = 1; h.maxNs = 100'000'000; // 98 in [16, 32) us, one in [256, 512), one at 100 ms
        check( h.count() == 100 && h.percentileMicroseconds( .5 ) == 32 && h.percentileMicroseconds( .98 ) == 32 && h.percentileMicroseconds( .99 ) == 512 &&
               h.percentileMicroseconds( .999 ) == 100'000 && h.percentileMicroseconds( 1 ) == 100'000, "a percentile is its bucket's upper bound; past 64 ms, the maximum" );
        h = {}; h.counts[8] = 1; h.maxNs = 3'000'000;
        check( h.percentileMicroseconds( .5 ) == 3000, "never more than the maximum" );

        oscairn::BlockTiming t;
        const auto n = oscairn::BlockTiming::warmupBlocks;
        for( uint32_t i = 0; i < n; ++i ) t.record( i == 0 ? 3'000'000 : 20'000, 1'000'000'000 + int64_t( i ) * 1'000'000 ); // a one-off at the start
        for( uint32_t i = 0; i < 100; ++i ) t.record( i == 50 ? 400'000 : 20'000, 1'000'000'000 + int64_t( n + i ) * 1'000'000 );
        auto s = t.read();
        check( s.startup.count() == n && s.startup.maxNs == 3'000'000 && s.startup.maxAt == 0, "the first blocks are startup, the one-off with them" );
        check( s.steady.count() == 100 && s.steady.counts[1] == 99 && s.steady.maxNs == 400'000 && s.steady.maxAt == int64_t( n + 50 ) * 1'000'000, "the steady blocks apart, the time of their maximum from the window's start" );
        t.reset();
        check( t.read().steady.count() == 0 && t.read().startup.count() == 0, "a reset reads empty before the next block" );
        t.record( 50'000, 5'000'000'000 );
        s = t.read();
        check( s.steady.count() == 1 && s.steady.maxNs == 50'000 && s.steady.maxAt == 0 && s.startup.count() == 0, "then a window from that block, the startup over" );
        t.warmUp();
        t.record( 60'000, 5'001'000'000 );
        s = t.read();
        check( s.startup.count() == 1 && s.steady.count() == 1 && s.seconds == .001, "a warm-up's blocks are startup again" );

        // OSCAIRN_AUDIO_THREAD_CORES, as makeRealtime reads it.
        check( oscairn::worker::cpuMask( "1-3" ) == 0b1110 && oscairn::worker::cpuMask( "0,2,5-6" ) == 0b1100101 && oscairn::worker::cpuMask( "63" ) == uint64_t( 1 ) << 63,
               "a CPU list as the kernel writes one" );
        check( !oscairn::worker::cpuMask( "" ) && !oscairn::worker::cpuMask( "3-1" ) && !oscairn::worker::cpuMask( "1," ) && !oscairn::worker::cpuMask( "64" ) &&
               !oscairn::worker::cpuMask( "a" ) && !oscairn::worker::cpuMask( nullptr ), "a malformed one is none" );

        // A worker's allowance: half the block's period, or OSCAIRN_WORKER_BUDGET_US's whole microseconds.
        using oscairn::worker::workerAllowance;
        check( workerAllowance( 128, 48000, nullptr ) == std::chrono::nanoseconds( 1'333'333 ) && workerAllowance( 256, 48000, "" ) == std::chrono::nanoseconds( 2'666'666 ) &&
               workerAllowance( 64, 44100, nullptr ) == std::chrono::nanoseconds( 725'623 ), "half the block's period" );
        check( workerAllowance( 128, 48000, "2000" ) == std::chrono::microseconds( 2000 ) && workerAllowance( 128, 48000, "50000" ) == std::chrono::milliseconds( 50 ),
               "an override in microseconds" );
        check( workerAllowance( 128, 48000, "0" ) == std::chrono::nanoseconds( 1'333'333 ) && workerAllowance( 128, 48000, "-5" ) == std::chrono::nanoseconds( 1'333'333 ) &&
               workerAllowance( 128, 48000, "1.5" ) == std::chrono::nanoseconds( 1'333'333 ) && workerAllowance( 128, 48000, "abc" ) == std::chrono::nanoseconds( 1'333'333 ) &&
               workerAllowance( 128, 48000, "99999999999" ) == std::chrono::nanoseconds( 1'333'333 ), "a malformed or out-of-range one leaves the derived allowance" );

        // RFC 0045's spin window: OSCAIRN_WORKER_SPIN_US, held to half the allowance; none for more audio threads than cores.
        using oscairn::worker::spinWindow;
        const auto allowed = std::chrono::nanoseconds( 1'333'333 );
        check( spinWindow( "200", allowed ) == std::chrono::microseconds( 200 ) && spinWindow( "5000", allowed ) == allowed / 2 &&
               spinWindow( "200", allowed, 3, 4 ) == std::chrono::microseconds( 200 ) && spinWindow( "200", allowed, 4, 4 ) == std::chrono::microseconds( 200 ),
               "a spin window, within half the allowance, while the threads have a core each" );
        check( spinWindow( "200", allowed, 5, 4 ) == std::chrono::nanoseconds::zero() && spinWindow( nullptr, allowed ) == std::chrono::nanoseconds::zero() &&
               spinWindow( "", allowed ) == std::chrono::nanoseconds::zero() && spinWindow( "0", allowed ) == std::chrono::nanoseconds::zero() &&
               spinWindow( "-3", allowed ) == std::chrono::nanoseconds::zero() && spinWindow( "1.5", allowed ) == std::chrono::nanoseconds::zero(),
               "none past the cores, unset, or malformed" );
        check( oscairn::worker::usableCores() >= 1, "at least one core" );

        // A block's outcome from the times the engine saw: done when it came for it is on time, however
        // long since; found done within the allowance on time, past it late (played); never found, silent.
        using oscairn::worker::blockOutcome; using oscairn::worker::BlockOutcome; using std::chrono::microseconds;
        const auto allowance = microseconds( 1333 );
        check( blockOutcome( true, true, microseconds( 5000 ), allowance ) == BlockOutcome::onTime && blockOutcome( false, true, microseconds( 1333 ), allowance ) == BlockOutcome::onTime,
               "a block done when the engine came for it, or by the allowance, is on time" );
        check( blockOutcome( false, true, microseconds( 1334 ), allowance ) == BlockOutcome::late && blockOutcome( false, true, microseconds( 3750 ), allowance ) == BlockOutcome::late,
               "found done past the allowance (the engine's thread woke late) is late, and played" );
        check( blockOutcome( false, false, microseconds( 1333 ), allowance ) == BlockOutcome::silent && blockOutcome( false, false, microseconds( 200 ), allowance ) == BlockOutcome::silent,
               "never found done within the allowance is silent" );

        // The CPU wake-up latency request (RFC 0045), on a file standing in for /dev/cpu_dma_latency.
        {
            const auto stand = std::filesystem::temp_directory_path() / ( "oscairn-cpu-latency-" + std::to_string( getpid() ) );
            const auto file = stand.string();
            auto bytes = [&] { std::ifstream in( stand, std::ios::binary ); return std::string( std::istreambuf_iterator<char>( in ), {} ); };
            auto open = [&] { size_t n = 0; for( const auto& f : std::filesystem::directory_iterator( "/proc/self/fd" ) ) { std::error_code e; if( std::filesystem::read_symlink( f, e ) == stand ) ++n; } return n; };
#ifdef __linux__
            { std::ofstream( stand ) << ""; }
            setenv( "OSCAIRN_CPU_LATENCY", "keep", 1 );
            check( !oscairn::holdCpuWakeLatency( true, file.c_str() ) && bytes().empty(), "OSCAIRN_CPU_LATENCY=keep asks for nothing" );
            unsetenv( "OSCAIRN_CPU_LATENCY" );
            check( oscairn::holdCpuWakeLatency( true, file.c_str() ) && bytes() == std::string( 4, '\0' ) && open() == 1, "held: a 32-bit 0 written, the file kept open" );
            check( oscairn::holdCpuWakeLatency( true, file.c_str() ) && bytes().size() == 4 && open() == 1, "held again (the device restarting): the same request" );
            check( !oscairn::holdCpuWakeLatency( false, file.c_str() ) && open() == 0, "let go: closed, which ends the request" );
            std::filesystem::remove( stand );
            check( !oscairn::holdCpuWakeLatency( true, file.c_str() ) && open() == 0, "a device not here: nothing held, and the engine goes on" );
#else
            (void)bytes; (void)open;
            check( !oscairn::holdCpuWakeLatency( true, file.c_str() ), "Linux only" );
#endif
        }
    }

    // The session's callback timing from times a host gives it (blockTimes): deterministic
    // durations and lateness, no allocation on the audio thread, the startup window after a device
    // start and a Performance load, and the window read and reset through engineTiming.
    void callbackTiming()
    {
        namespace fs = std::filesystem;
        oscairn::Session session( rate, block );
        const int64_t period = int64_t( 1e9 * block / rate ); // 10.67 ms
        std::array<float, block> l{}, r{};
        float* out[] = { l.data(), r.data() };
        int64_t start = 7'000'000'000;
        auto run = [&]( uint32_t blocks, int64_t duration, int64_t late ) {
            for( uint32_t i = 0; i < blocks; ++i )
            {
                start += period + late;
                forbidAllocation = true;
                session.process( out, block );
                session.blockTimes( start, start + duration, period );
                forbidAllocation = false;
            }
        };
        const auto n = oscairn::BlockTiming::warmupBlocks;
        run( n, 5'000'000, 0 );
        run( 99, 100'000, 0 );
        run( 1, 100'000, 3'000'000 ); // one callback 3 ms late
        auto t = session.engineTiming( true );
        check( t.callback.startup.count() == n && t.callback.startup.maxNs == 5'000'000 && t.callback.steady.count() == 100 && t.callback.steady.counts[3] == 100,
               "the callback's durations: the first blocks startup, the rest steady" );
        check( t.lateness.startup.count() == n && t.lateness.startup.maxNs == 0 && t.lateness.steady.count() == 99 && t.lateness.steady.maxNs == 3'000'000 &&
               t.lateness.steady.counts[0] == 98 && t.lateness.steady.counts[8] == 1 && t.workers.empty(), "lateness against the last start plus a period; the first block is late against none" );
        check( session.engineTiming( false ).callback.steady.count() == 0, "a reset reads empty until the next block" );
        run( 1, 100'000, -1'000'000 );
        t = session.engineTiming( false );
        check( t.callback.steady.count() == 1 && t.lateness.steady.count() == 1 && t.lateness.steady.counts[0] == 1 && t.lateness.steady.maxNs == 0, "early counts as on time" );
        session.audioStarted();
        start += 60'000'000'000; // the device was stopped for a minute
        run( 1, 100'000, 0 );
        t = session.engineTiming( true );
        check( t.callback.startup.count() == 1 && t.lateness.steady.count() == 1 && t.lateness.startup.count() == 0, "after a device start, warming up, and late against nothing" );

        const auto home = fs::temp_directory_path() / ( "oscairn-session-timing-" + std::to_string( getpid() ) );
        fs::remove_all( home );
        setenv( "OSCAIRN_DATA_HOME", home.c_str(), 1 );
        run( n, 100'000, 0 );
        session.savePerformance( ( home / "Saved" ).string() );
        session.engineTiming( true );
        session.loadPerformance( ( home / "Saved" ).string() );
        run( 2, 100'000, 0 );
        t = session.engineTiming( false );
        check( t.callback.startup.count() == 2 && t.callback.steady.count() == 0, "a Performance load warms up again" );
        unsetenv( "OSCAIRN_DATA_HOME" );
        fs::remove_all( home );
    }
}

int main( int argc, char** argv )
{
    if( argc != 4 ) return 2;
    histogram();
    callbackTiming();
    contract( argv[2] );
    referenceRender( argv[1], argv[2], argv[3] );
    midiClock();
    clockOut();
    concurrentControl( argv[2] );
    revisionsNeverRepeat( argv[2] );
    reopensLast( argv[2] );
    if( failures == 0 ) std::puts( "ok: control/session audio equivalence, pressure, clock in and out, validation, overflow, concurrent rendering and reopening the last Performance" );
    return failures ? 1 : 0;
}
