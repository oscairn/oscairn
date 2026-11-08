// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oscairn LFO: a low-frequency oscillator with no sound of its own, sent on an oscairn.param-target
// lane (RFC 0002) to whatever the host maps it to: one parameter, or a control of the Performance
// and every target under it (RFC 0011). Rate free in Hz or synced to the transport's beat; sine,
// triangle, saw, square or sample-and-hold; depth; a start phase, restarted by each note when
// Retrigger is on; bipolar or unipolar. It sends MODULATE events (an amount around where the
// target is) by default, or SET events (a position).
//
// Rate runs 0.01 Hz to 1,357 Hz (Yamaha's Extended LFO) on a log scale: the parameter is the
// knob's position, 0..1, and its value text is in Hz. It sends a point every 32 samples of its own
// running count, whatever the host's block size, closer at high rates to keep at least 16 a cycle
// (down to every sample), and one at each note that restarts it, each the shape's value at that
// exact sample. Saw and square are band-limited with polyBLEP once the points come closer.

#include <clap/clap.h>
#include <oscairn-clap/param-target.h>
#include <oscairn-clap/ui.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace
{
    constexpr double pi = 3.14159265358979323846;
    constexpr uint32_t interval = 32; // most samples between points: 1.5 kHz at 48 kHz, smooth for low rates
    constexpr double perCycle = 16;   // fewest points a cycle, up to a point every sample
    constexpr double lowest = .01, highest = 1357; // Hz, the rate's ends
    double hertz( double position ) { return lowest * std::pow( highest / lowest, position ); }
    double position( double hz ) { return std::log( hz / lowest ) / std::log( highest / lowest ); }

    const char* const features[] = { CLAP_PLUGIN_FEATURE_NOTE_EFFECT, CLAP_PLUGIN_FEATURE_UTILITY, nullptr };
    const clap_plugin_descriptor_t descriptor = {
        CLAP_VERSION_INIT,
        "org.oscairn.lfo",
        "Oscairn LFO",
        "Oscairn",
        "https://github.com/oscairn/oscairn",
        "",
        "",
        "0.1.0",
        "A low-frequency oscillator that modulates other plugins' parameters and controls",
        features,
    };

    enum Id : clap_id { Rate = 1, Sync, Division, Shape, Depth, Phase, Retrigger, Polarity, Mode, Count = Mode };
    enum ShapeKind { Sine, Triangle, Saw, Square, SampleHold };
    constexpr std::array<const char*, 5> shapes{ "Sine", "Triangle", "Saw", "Square", "Sample and hold" };
    // Synced: a cycle as so many beats (quarter notes).
    constexpr std::array<const char*, 8> divisions{ "4 bars", "2 bars", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/32" };
    constexpr std::array<double, 8> divisionBeats{ 16, 8, 4, 2, 1, .5, .25, .125 };
    constexpr std::array<const char*, 2> syncs{ "Free", "Synced" }, retriggers{ "Off", "On" }, polarities{ "Bipolar", "Unipolar" }, modes{ "Modulate", "Set" };

    // The most played first (oscairn.ui), rate and its sync drawn joined.
    struct Parameter { const char* name; double minimum, maximum, initial; bool stepped; uint32_t widget, priority, group; };
    const std::array<Parameter, Count> parameters{{
        { "Rate", 0, 1, position( 1 ), false, OSCAIRN_WIDGET_KNOB, 1, 1 }, // a position: hertz() gives Hz
        { "Sync", 0, 1, 0, true, OSCAIRN_WIDGET_SWITCH, 4, 1 },
        { "Synced rate", 0, 7, 4, true, OSCAIRN_WIDGET_LIST, 5, 1 },
        { "Shape", 0, 4, 0, true, OSCAIRN_WIDGET_BUTTONS, 3, 0 },
        { "Depth", 0, 1, 1, false, OSCAIRN_WIDGET_KNOB, 2, 0 },
        { "Phase", 0, 1, 0, false, OSCAIRN_WIDGET_KNOB, 6, 0 },
        { "Retrigger", 0, 1, 0, true, OSCAIRN_WIDGET_SWITCH, 7, 0 },
        { "Polarity", 0, 1, 0, true, OSCAIRN_WIDGET_BUTTONS, 8, 0 },
        { "Mode", 0, 1, 0, true, OSCAIRN_WIDGET_BUTTONS, 9, 0 },
    }};

    struct Lfo
    {
        clap_plugin_t plugin{};
        const clap_host_t* host = nullptr;
        bool canSend = false;
        uint16_t space = 0;
        double sampleRate = 48000;
        std::array<std::atomic<double>, Count> values{}; // the main thread reads them while audio runs
        double phase = 0;        // audio thread: where in its cycle, 0..1, at the next sample
        uint32_t wait = 0;       // samples until its next point, on its own count
        double held = 0;         // sample and hold: this cycle's value
        uint32_t random = 0x9e3779b9u;
        Lfo() { for(size_t i = 0; i < values.size(); ++i) values[i] = parameters[i].initial; }
        double value( Id id ) const { return values[id - 1].load( std::memory_order_relaxed ); }
        void start() { phase = value( Phase ); wait = 0; random = 0x9e3779b9u; hold(); }
        void hold() { random ^= random << 13; random ^= random >> 17; random ^= random << 5; held = random / 2147483647.5 - 1; }
    };
    Lfo& self( const clap_plugin_t* p ) { return *static_cast<Lfo*>( p->plugin_data ); }

    // polyBLEP: the correction near a step of 2 at phase 0, for points `dt` of a cycle apart.
    double blep( double p, double dt )
    {
        if( p < dt ) { p /= dt; return p + p - p * p - 1; }
        if( p > 1 - dt ) { p = ( p - 1 ) / dt; return p * p + p + p + 1; }
        return 0;
    }

    // The shape at phase p, -1..1, its steps smoothed over `dt` of a cycle (0: sharp).
    double wave( const Lfo& s, double p, double dt )
    {
        switch( int( s.value( Shape ) ) )
        {
        case Triangle: return p < .5 ? 4 * p - 1 : 3 - 4 * p;
        case Saw: return 2 * p - 1 - ( dt > 0 ? blep( p, dt ) : 0 );
        case Square: return ( p < .5 ? 1 : -1 ) + ( dt > 0 ? blep( p, dt ) - blep( p < .5 ? p + .5 : p - .5, dt ) : 0 );
        case SampleHold: return s.held;
        default: return std::sin( 2 * pi * p );
        }
    }

    void applyParam( Lfo& s, const clap_event_header_t* h )
    {
        if( h->space_id != CLAP_CORE_EVENT_SPACE_ID || h->type != CLAP_EVENT_PARAM_VALUE || h->size < sizeof( clap_event_param_value_t ) )
            return;
        const auto& e = *reinterpret_cast<const clap_event_param_value_t*>( h );
        if( e.param_id < 1 || e.param_id > Count || !std::isfinite( e.value ) )
            return;
        const auto& p = parameters[e.param_id - 1];
        double v = std::clamp( e.value, p.minimum, p.maximum );
        if( p.stepped )
            v = std::round( v );
        s.values[e.param_id - 1] = v;
    }

    bool send( const Lfo& s, const clap_output_events_t* out, uint32_t time, double dt )
    {
        const double w = wave( s, s.phase, dt ), depth = s.value( Depth );
        const bool unipolar = s.value( Polarity ) != 0, set = s.value( Mode ) != 0;
        oscairn_event_param_target_t e{};
        e.header  = { sizeof( e ), time, s.space, OSCAIRN_EVENT_PARAM_TARGET, 0 };
        e.lane    = 0;
        e.mode    = set ? OSCAIRN_PARAM_TARGET_SET : OSCAIRN_PARAM_TARGET_MODULATE;
        e.note_id = e.port_index = e.channel = e.key = -1;
        // Modulate: an amount, -depth..depth (bipolar) or 0..depth. Set: a position around the middle, or up from 0.
        const double amount = unipolar ? depth * ( w + 1 ) / 2 : depth * w;
        e.value = set && !unipolar ? .5 + amount / 2 : amount;
        return out->try_push( out, &e.header );
    }

    clap_process_status process( const clap_plugin_t* p, const clap_process_t* block )
    {
        auto& s = self( p );
        const uint32_t frames = block->frames_count, count = block->in_events ? block->in_events->size( block->in_events ) : 0;
        const auto* t = block->transport;
        const bool playing = t && ( t->flags & CLAP_TRANSPORT_IS_PLAYING ) && ( t->flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE );
        const double tempo = t && ( t->flags & CLAP_TRANSPORT_HAS_TEMPO ) && std::isfinite( t->tempo ) && t->tempo >= 1 ? t->tempo : 120;
        const double beat = playing ? double( t->song_pos_beats ) / CLAP_BEATTIME_FACTOR : 0;
        uint32_t input = 0;
        for( uint32_t i = 0; i < frames; ++i )
        {
            bool restart = false;
            for( ; input < count; ++input )
            {
                const auto* h = block->in_events->get( block->in_events, input );
                if( h->time > i )
                    break;
                applyParam( s, h );
                if( h->space_id == CLAP_CORE_EVENT_SPACE_ID && s.value( Retrigger ) != 0 &&
                    ( h->type == CLAP_EVENT_NOTE_ON || ( h->type == CLAP_EVENT_MIDI && ( reinterpret_cast<const clap_event_midi_t*>( h )->data[0] & 0xf0 ) == 0x90 &&
                                                         reinterpret_cast<const clap_event_midi_t*>( h )->data[2] ) ) )
                    restart = true;
            }
            const bool synced = s.value( Sync ) != 0;
            const double cycleBeats = divisionBeats[size_t( s.value( Division ) )];
            if( restart )
            {
                s.phase = s.value( Phase );
                s.hold();
            }
            else if( synced && playing ) // where the transport's beat puts it
            {
                const double at = beat + i * tempo / 60 / s.sampleRate;
                const double next = at / cycleBeats + s.value( Phase );
                s.phase = next - std::floor( next );
            }
            const double rate = synced ? tempo / 60 / cycleBeats : hertz( s.value( Rate ) );
            const auto spacing = uint32_t( std::clamp( std::floor( s.sampleRate / ( rate * perCycle ) + 1e-6 ), 1.0, double( interval ) ) ); // 1e-6: a round rate from its position, a hair over, keeps its spacing
            const bool point = !s.wait;
            if( point )
                s.wait = spacing;
            if( s.canSend && ( restart || point ) )
                send( s, block->out_events, i, spacing < interval ? rate * spacing / s.sampleRate : 0 ); // a full queue counts the point as dropped
            --s.wait;
            s.phase += rate / s.sampleRate;
            if( s.phase >= 1 )
            {
                s.phase -= std::floor( s.phase );
                s.hold(); // a new cycle, a new held value
            }
        }
        return CLAP_PROCESS_CONTINUE;
    }

    bool paramsInfo( const clap_plugin_t*, uint32_t index, clap_param_info_t* info )
    {
        if( index >= Count )
            return false;
        const auto& p = parameters[index];
        *info = {};
        info->id = index + 1;
        info->flags = CLAP_PARAM_IS_AUTOMATABLE | ( p.stepped ? CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM : 0u );
        info->min_value = p.minimum;
        info->max_value = p.maximum;
        info->default_value = p.initial;
        std::snprintf( info->name, sizeof( info->name ), "%s", p.name );
        return true;
    }

    const char* const* names( clap_id id, size_t& size )
    {
        switch( id )
        {
        case Sync: size = syncs.size(); return syncs.data();
        case Division: size = divisions.size(); return divisions.data();
        case Shape: size = shapes.size(); return shapes.data();
        case Retrigger: size = retriggers.size(); return retriggers.data();
        case Polarity: size = polarities.size(); return polarities.data();
        case Mode: size = modes.size(); return modes.data();
        default: size = 0; return nullptr;
        }
    }

    bool toText( const clap_plugin_t*, clap_id id, double value, char* text, uint32_t size )
    {
        if( id < 1 || id > Count || !std::isfinite( value ) )
            return false;
        size_t count = 0;
        if( const auto* list = names( id, count ) )
            std::snprintf( text, size, "%s", list[std::min( count - 1, size_t( std::max( 0.0, std::round( value ) ) ) )] );
        else if( id == Rate )
        {
            const double hz = hertz( value );
            std::snprintf( text, size, hz < 10 ? "%.2f Hz" : hz < 100 ? "%.1f Hz" : "%.0f Hz", hz );
        }
        else if( id == Phase )
            std::snprintf( text, size, "%.0f°", value * 360 );
        else
            std::snprintf( text, size, "%.0f%%", value * 100 );
        return true;
    }

    bool fromText( const clap_plugin_t*, clap_id id, const char* text, double* value )
    {
        if( id < 1 || id > Count )
            return false;
        size_t count = 0;
        if( const auto* list = names( id, count ) )
        {
            for( size_t i = 0; i < count; ++i )
                if( !std::strcmp( text, list[i] ) ) { *value = double( i ); return true; }
            return false;
        }
        char* end = nullptr;
        double v = std::strtod( text, &end );
        if( end == text || !std::isfinite( v ) )
            return false;
        while( *end == ' ' ) ++end;
        if( id == Phase ) v /= 360;
        else if( id == Rate ) v = v >= lowest && v <= highest ? std::clamp( position( v ), 0.0, 1.0 ) : -1;
        else v /= 100;
        const auto& p = parameters[id - 1];
        if( v < p.minimum || v > p.maximum )
            return false;
        *value = v;
        return true;
    }

    const clap_plugin_params_t params{
        []( const clap_plugin_t* ) -> uint32_t { return Count; },
        paramsInfo,
        []( const clap_plugin_t* p, clap_id id, double* value ) -> bool {
            if( id < 1 || id > Count ) return false;
            *value = self( p ).values[id - 1];
            return true;
        },
        toText,
        fromText,
        []( const clap_plugin_t* p, const clap_input_events_t* in, const clap_output_events_t* ) {
            for( uint32_t i = 0; i < in->size( in ); ++i ) applyParam( self( p ), in->get( in, i ) );
        },
    };
    // Notes only restart it (Retrigger).
    const clap_plugin_note_ports_t notePorts{
        []( const clap_plugin_t*, bool input ) -> uint32_t { return input ? 1 : 0; },
        []( const clap_plugin_t*, uint32_t index, bool input, clap_note_port_info_t* info ) -> bool {
            if( !input || index ) return false;
            *info = {};
            info->supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI;
            info->preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
            std::strcpy( info->name, "Retrigger" );
            return true;
        },
    };
    const oscairn_plugin_param_target_lanes_t lanes{
        []( const clap_plugin_t* ) -> uint32_t { return 1; },
        []( const clap_plugin_t*, uint32_t index, oscairn_param_target_lane_info_t* info ) -> bool {
            if( index ) return false;
            info->lane = 0;
            std::snprintf( info->name, sizeof( info->name ), "LFO" );
            return true;
        },
    };
    const oscairn_plugin_ui_t ui{
        []( const clap_plugin_t*, clap_id id, oscairn_param_hint_t* hint ) {
            if( id < 1 || id > Count ) return false;
            const auto& p = parameters[id - 1];
            *hint = { p.widget, p.priority, CLAP_INVALID_ID, p.group, OSCAIRN_ROLE_NONE, 0, OSCAIRN_FIELD_NONE, 0 };
            return true;
        },
        nullptr, // no FM algorithms
        nullptr, // no envelope stages
    };

    const clap_plugin_t* create( const clap_host_t* host )
    {
        auto* s = new Lfo;
        s->host = host;
        s->plugin.desc = &descriptor;
        s->plugin.plugin_data = s;
        s->plugin.init = []( const clap_plugin_t* p ) {
            auto& s = self( p );
            const auto* r = static_cast<const clap_host_event_registry_t*>( s.host->get_extension( s.host, CLAP_EXT_EVENT_REGISTRY ) );
            s.canSend = r && r->query && r->query( s.host, OSCAIRN_EVENT_SPACE_PARAM_TARGET, &s.space );
            return true;
        };
        s->plugin.destroy = []( const clap_plugin_t* p ) { delete &self( p ); };
        s->plugin.activate = []( const clap_plugin_t* p, double rate, uint32_t, uint32_t ) {
            if( !std::isfinite( rate ) || rate < 8000 ) return false;
            self( p ).sampleRate = rate;
            self( p ).start();
            return true;
        };
        s->plugin.deactivate = []( const clap_plugin_t* ) {};
        s->plugin.start_processing = []( const clap_plugin_t* ) { return true; };
        s->plugin.stop_processing = []( const clap_plugin_t* ) {};
        s->plugin.reset = []( const clap_plugin_t* p ) { self( p ).start(); };
        s->plugin.process = process;
        s->plugin.get_extension = []( const clap_plugin_t*, const char* id ) -> const void* {
            if( !std::strcmp( id, CLAP_EXT_PARAMS ) ) return &params;
            if( !std::strcmp( id, CLAP_EXT_NOTE_PORTS ) ) return &notePorts;
            if( !std::strcmp( id, OSCAIRN_EXT_PARAM_TARGET_LANES ) ) return &lanes;
            if( !std::strcmp( id, OSCAIRN_EXT_UI ) ) return &ui;
            return nullptr;
        };
        s->plugin.on_main_thread = []( const clap_plugin_t* ) {};
        return &s->plugin;
    }

    const clap_plugin_factory_t factory{
        []( const clap_plugin_factory_t* ) -> uint32_t { return 1; },
        []( const clap_plugin_factory_t*, uint32_t index ) -> const clap_plugin_descriptor_t* { return index ? nullptr : &descriptor; },
        []( const clap_plugin_factory_t*, const clap_host_t* host, const char* id ) -> const clap_plugin_t* {
            return !std::strcmp( id, descriptor.id ) ? create( host ) : nullptr;
        },
    };
}

extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION_INIT,
    []( const char* ) { return true; },
    []() {},
    []( const char* id ) -> const void* { return !std::strcmp( id, CLAP_PLUGIN_FACTORY_ID ) ? &factory : nullptr; },
};
