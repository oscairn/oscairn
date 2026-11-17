// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include <clap/clap.h>
#include <oscairn-clap/sound-contexts.h>
#include <oscairn-clap/effect-contexts.h>
#include <oscairn-clap/assets.h>
#include <oscairn-clap/memory.h>
#include <oscairn-clap/voice-budget.h>
#include <oscairn-clap/hardware.h>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

namespace kick::clap {

// A preset a plugin's library lists for it through CLAP preset discovery: inside the plugin
// (kind 1, CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN) or in a file (kind 0, location its path).
struct Preset { std::string name; uint32_t kind = 0; std::string location, loadKey; };

struct Descriptor {
    std::string path, id, name, vendor, version, description;
    std::vector<std::string> features;
    bool presetDiscovery = false; // its library offers CLAP preset discovery
    std::vector<Preset> presets;  // what a scan indexed (RFC 0005)
    std::vector<std::pair<uint32_t, std::string>> lanes; // its oscairn.param-target lanes: number and name, once loaded
};

struct Parameter {
    clap_id id = CLAP_INVALID_ID;
    std::string name, module;
    double minimum = 0, maximum = 1, defaultValue = 0;
    uint32_t flags = 0;
    // The plugin's oscairn.ui hint, checked against the parameter: the control it wants
    // (enum oscairn_widget), its priority (0: none) and, for a range, its high parameter.
    uint32_t widget = 0, priority = 0;
    clap_id rangeHigh = CLAP_INVALID_ID;
    uint32_t group = 0; // drawn joined with the module's other parameters of this group; 0: none
    uint32_t role = 0;  // enum oscairn_role: what it does on every instrument (cutoff, attack); 0: none
    // Its rich view (oscairn.ui draft 4): the view's number (0: none), its enum oscairn_field
    // there, and whose field (an operator, element or zone, 1 to 8; 0: the view's own).
    uint32_t view = 0, field = 0, index = 0;
    // An algorithm parameter's routings, one per value from its minimum: oscairn_fm_algorithm_t's
    // inputs[8], carriers, feedback_from and feedback_to.
    std::vector<std::array<uint8_t, 11>> algorithms{};
    // A time or rate field's seconds at evenly spaced values from its minimum to its maximum (each
    // value of a stepped one of at most 128 values, else 33), from oscairn.ui's seconds() or value
    // texts with units; empty: none.
    std::vector<double> seconds{};
};

struct Port {
    clap_id id = CLAP_INVALID_ID;
    std::string name;
    uint32_t channels = 0, flags = 0;
};

struct Event;
struct SysexPool;
class TransportWatch;
// The Rust host's (core/clap-host/rust/src/event.rs), which the inline functions below call.
extern "C" {
bool rust_clap_event_valid(const clap_event_header_t* header);
void rust_clap_event_clear_padding(Event* event);
void rust_clap_event_midi(Event* event, uint8_t status, uint8_t a, uint8_t b, uint32_t frame, uint16_t port);
void rust_clap_event_note(Event* event, bool on, int32_t key, double velocity, uint32_t frame, int32_t channel, int32_t port, int32_t noteId);
bool rust_clap_sysex_hold(SysexPool* pool, Event* event);
bool rust_clap_transport_changed(TransportWatch* watch, const clap_event_transport_t* current, uint32_t frames, double sampleRate);
}

// Owning, fixed-capacity event. A SysEx event (CLAP_EVENT_MIDI_SYSEX) points at bytes held
// in a block's SysexPool, valid until that block's consumers have run; the command queue
// refuses SysEx, whose bytes could not outlive the caller there.
struct Event {
    // 64 bytes holds every event type the host accepts (the largest, parameter
    // value and modulation events, are 56 bytes).
    static constexpr size_t capacity = 64;
    alignas(8) std::array<uint8_t, capacity> bytes{};
    const clap_event_header_t* header() const noexcept {
        return reinterpret_cast<const clap_event_header_t*>(bytes.data());
    }
    bool valid() const noexcept { return rust_clap_event_valid(header()); }
    template<class T> static Event from(const T& source) noexcept {
        static_assert(sizeof(T) <= capacity, "event too large");
        Event result;
        std::memcpy(result.bytes.data(), &source, sizeof(T));
        result.clearPadding();
        return result;
    }
    // Zeroes the padding between the fields of a note, note expression, parameter or MIDI event: a struct copied here
    // leaves its own padding indeterminate (stack bytes under SRA), and the same event must be the same 64 bytes in
    // every host and on every platform (a recording of it, a Rust host's copy of it, a plugin hashing it).
    void clearPadding() noexcept { rust_clap_event_clear_padding(this); }
    static Event midi(uint8_t status, uint8_t a, uint8_t b, uint32_t frame = 0, uint16_t port = 0) noexcept {
        Event e; rust_clap_event_midi(&e, status, a, b, frame, port); return e;
    }
    static Event note(bool on, int key, double velocity, uint32_t frame = 0, int channel = 0, int port = 0, int noteId = -1) noexcept {
        Event e; rust_clap_event_note(&e, on, key, velocity, frame, channel, port, noteId); return e;
    }
};

// The bytes of one block's SysEx events (RFC 0032): preallocated, emptied each block, never
// grown, so an event's pointer stays valid until the block's consumers have run. A message
// over maxMessage, or one that would take the block past maxBlock, is dropped, never truncated.
struct SysexPool {
    static constexpr uint32_t maxMessage = 64 * 1024;
    static constexpr uint32_t maxBlock = 256 * 1024;
    uint32_t used = 0;
    alignas(8) uint8_t bytes[maxBlock];
    // Copies a SysEx event's bytes in and points the event at the copy. False when they do
    // not fit, and for any other event. Audio thread: no allocation.
    bool hold(Event& event) noexcept { return rust_clap_sysex_hold(this, &event); }
};
static_assert(offsetof(SysexPool, bytes) == 8 && sizeof(SysexPool) == 8 + SysexPool::maxBlock, "the Rust host's SysexPool");

// A sound asset reference (RFC 0009): library ID, library-relative path, SHA-256 hex or empty.
struct AssetReference { std::string library, path, sha256; bool optional = false; }; // optional: the plugin does without it (oscairn.asset-dependencies draft 2)
// Resolves asset references for plugins through the oscairn.assets host extension. Main thread.
struct AssetResolver {
    virtual ~AssetResolver() = default;
    // The local path of library/path. An empty sha256 is filled in; a given one must match.
    // Throws std::exception carrying the reason.
    virtual std::string resolve(const std::string& library, const std::string& path, std::string& sha256) = 0;
    // The folder of an installed asset library holding its files at their paths, when a version at
    // least minVersion (empty: any) is installed; empty otherwise (oscairn.assets draft 2).
    virtual std::string libraryRoot(const std::string&, const std::string&) const { return {}; }
    // A reference for a file a plugin made or chose (oscairn.assets draft 3's create, and LV2
    // state's mapped paths): where it is when a registered or installed library holds it, else a
    // copy in the user library. Throws std::exception carrying the reason.
    virtual AssetReference create(const std::string&) { throw std::runtime_error("this host has no sound asset store"); }
    // Where an installed sample's record is (oscairn.assets draft 4's resolve_sample): its pack and
    // the record's offset, length and preload span, by the sample's id (64 lowercase hex digits).
    // Throws std::exception carrying the reason.
    struct SampleLocation { std::string pack; uint64_t offset = 0, length = 0, preload = 0; };
    virtual SampleLocation resolveSample(const std::string&) { throw std::runtime_error("this host has no sample store"); }
};

// Whether a transport moved other than by playing on since the last call: its flags, tempo or time
// signature changed, it jumped, its loop or bar moved, or it came or went; the first call counts as moved.
// What wakes a plugin asleep (CLAP_PROCESS_SLEEP), here and in a graph that leaves one asleep uncalled.
// The Rust host's (host.rs TransportWatch), its fields in the same order.
class TransportWatch {
public:
    bool changed(const clap_event_transport_t* current, uint32_t frames, double sampleRate) noexcept {
        return rust_clap_transport_changed(this, current, frames, sampleRate);
    }
    void reset() noexcept { observed = false; }
private:
    bool observed = false, had = false, hasPrevious = false;
    uint32_t previousFrames = 0;
    clap_event_transport_t previous{};
};
static_assert(sizeof(TransportWatch) == 8 + sizeof(clap_event_transport_t) && alignof(TransportWatch) == 8, "the Rust host's TransportWatch");

struct Statistics {
    uint64_t blocks = 0, droppedEvents = 0, processErrors = 0;
    double lastMicroseconds = 0, maximumMicroseconds = 0;
    // Cumulative per host. A missed DSP deadline is not an audio-device xrun.
    uint64_t deadlineMisses = 0, controlSilences = 0, idleBlocks = 0;
    uint64_t restarts = 0; // Oscairn plugin workers restarted after failing (RFC 0010)
    // Oscairn plugin workers (RFC 0045): blocks past the allowance that played late, and those
    // silenced; both are in deadlineMisses. The allowance per block, 0 for a plugin in the engine.
    uint64_t lateBlocks = 0, silentBlocks = 0;
    double allowanceMicroseconds = 0;
    // Oscairn plugin workers, apart for the timing report: the silent blocks never handed over because
    // the worker was still busy with the last; the panics the engine sent right after a block the
    // worker did not render (since RFC 0045's miss policy, none for a late or busy block, which loses
    // no events: only where events were lost, as a control silence in the worker); the page faults of the worker's audio thread (Linux; 0 elsewhere).
    // Also the worker's audio thread's involuntary context switches (Linux; 0 elsewhere).
    uint64_t busyBlocks = 0, missPanics = 0, minorFaults = 0, majorFaults = 0, involuntarySwitches = 0;
    // Oscairn plugin workers: the blocks handed to the worker, and those its plugin slept through uncalled (PluginHost::rest).
    uint64_t calledBlocks = 0, restedBlocks = 0;
    // Cumulative over `blocks`: the time the plugin's own blocks took, its process() in the engine's
    // process, and the CPU time of its worker's audio thread for a plugin in a worker (RFC 0025's cost).
    double cpuMicroseconds = 0;
};

// oscairn.hardware (oscairn-clap/hardware.h, RFC 0004): the process's accelerators, which every
// host offers its plugin through that extension (oscairn::hardware::Service implements it).
// owner is the plugin's host, which only its own leases answer to; epoch is its activation, which
// a job is submitted and collected in. The audio-thread calls neither allocate nor block.
struct HardwareAccess {
    virtual ~HardwareAccess() = default;
    virtual uint32_t count() const noexcept = 0;                                          // main thread
    virtual bool get(uint32_t index, oscairn_hardware_accelerator_t&) const noexcept = 0; // main thread
    virtual bool supports(uint32_t index, const char* operation) const noexcept = 0;     // main thread
    virtual int32_t reserve(const void* owner, uint32_t index, const oscairn_hardware_request_t&,
                            oscairn_hardware_lease_t&) noexcept = 0;                       // main thread
    virtual void release(const void* owner, uint32_t lease) noexcept = 0;                 // main thread
    virtual void releaseAll(const void* owner) noexcept = 0;                              // main thread
    virtual int32_t submit(const void* owner, uint32_t epoch, uint32_t lease, const oscairn_hardware_job_t&) noexcept = 0;
    virtual int32_t collect(const void* owner, uint32_t epoch, uint32_t lease, oscairn_hardware_result_t&) noexcept = 0;
};

// The CLAP host's limits and event spaces (the Rust host's, core/clap-host/rust, which a test checks against these), and
// the process-wide settings every host in the process reads, which tell the Rust host (plugin-host/src/kick_clap.cpp).
// One plugin instance is a PluginHost (oscairn/plugin_host.h, oscairn::loadPlugin).
struct Host {
    static constexpr uint32_t maxEvents = 2048;     // capacity of the control-thread queues
    static constexpr uint32_t maxAudioChannels = 64; // in each direction, across all buses
    static constexpr uint32_t maxBlockEvents = 512; // direct graph events per process() call; queued commands have a separate budget
    // A destination's block also carries the graph's own oscairn.beat event and its part's
    // harmony changes, which do not count against maxBlockEvents, and a key's release fans out
    // to each note the router holds for it (at most maxBlockEvents, the router's ledger); hosts
    // must accept up to this many events per process() call.
    static constexpr uint32_t maxProcessEvents = 2 * maxBlockEvents + 1 + 32;
    // The event space this host assigns to oscairn.param-target (oscairn-clap/param-target.h)
    // through CLAP's event-registry. Plugins may send these events; the host passes them
    // to the caller in outputEvents() and never delivers them to a plugin.
    static constexpr uint16_t paramTargetSpace = 1000;
    // oscairn.beat (oscairn-clap/beat.h): the caller sends these events to every host each block;
    // the host delivers them only to a plugin that queried the space, and never counts the rest
    // as dropped.
    static constexpr uint16_t beatSpace = 1001;
    // oscairn.harmony (oscairn-clap/harmony.h): the same, to each plugin in a part, and a harmony
    // equal to the last one delivered is not delivered again. The plugin reads the last on the
    // main thread through the host extension; a block without one means the plugin is in no
    // part, and the extension then gives none.
    static constexpr uint16_t harmonySpace = 1002;
    static constexpr uint32_t maxStateBytes = 64 * 1024 * 1024;
    static constexpr size_t maxPatchBytes = 1024 * 1024;
    // oscairn.memory (RFC 0025). Called on the main thread at every activation (a load, a state
    // load, a reconfigure, an effect context, a restart the plugin asked for), after the plugin
    // declares and before it activates and allocates, with what it declared (nullptr: nothing),
    // so the process hosting it can make room first: a worker's engine refits its limit. Throwing
    // refuses the activation, its text the reason. Process-wide; set once, before any plugin loads. Null: none.
    using ActivationObserver = void (*)(void* context, const oscairn_memory_t* declared);
    static void observeActivations(ActivationObserver, void* context) noexcept;
    // oscairn.hardware (RFC 0004): the accelerators plugins are offered. Process-wide; set once,
    // before any plugin loads, and kept until after the last is unloaded. Null, the default:
    // none, so count() is 0 and every reservation is refused as unavailable.
    static void setHardware(HardwareAccess*) noexcept;
};

// For a process with no event loop of its own (a plugin worker) once a plugin window is open:
// runs the platform's window events on the main thread for up to `milliseconds`, returning
// early when `descriptor` becomes readable. False, at once, where there are none to run here.
bool runWindowEvents(int descriptor, int milliseconds);

} // namespace kick::clap
