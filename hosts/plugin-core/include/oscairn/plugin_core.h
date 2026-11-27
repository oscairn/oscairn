// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

// RFC 0047 step 1: the core every DAW plugin format wraps, one Performance per plugin instance.
// No format's types here: each format (CLAP, VST3, AU, AAX) is a thin shell over Instance.
namespace oscairn::plugin
{
    struct Config
    {
        double sampleRate = 48000;
        uint32_t maxFrames = 512; // the most frames a block brings
        // The plugins bundled with Oscairn, where a Performance's plugin not at its saved path or in the
        // store is looked for by format and ID (Session::setBundledPlugins).
        std::string bundledPlugins;
        // The oscairn-plugin-worker executable: plugins run in worker processes, as on the device (RFC 0010).
        // Empty: as OSCAIRN_PLUGIN_WORKER says, else in the DAW's process. Placement is process-wide: the
        // first instance to give one sets it for every instance in the process (#303 takes this further).
        std::string worker;
    };

    // A MIDI message at its frame in the block: notes, controllers, pitch bend, pressure. MIDI comes in
    // by part channel, as on the device.
    struct MidiEvent
    {
        uint32_t frame = 0;
        uint8_t size = 0;
        uint8_t bytes[3] = {};
    };

    // One block, as the DAW gives it: the DAW is the clock, there is no device underneath.
    struct Block
    {
        uint32_t frames = 0;               // 1 to Config::maxFrames, any number each block
        const MidiEvent* events = nullptr; // sorted by frame, frames within the block
        uint32_t eventCount = 0;           // at most 256 are played; the rest are dropped
        float* main[2] = {};               // the main stereo bus, left and right, written whole
        // The seams of the later steps: #302's 16 optional stereo outs and #305's host transport come in here.
    };

    // A Performance in the installed library, for the editor's list. library: the installed library
    // it is in, empty for the user's own.
    struct Item
    {
        std::string id, name, library;
        std::vector<std::string> tags;
    };

    // The Performance an instance plays: its library ID (empty for none picked) and name.
    struct Performance
    {
        std::string id, name;
    };

    enum class Load { None, Loading, Ready, Failed };

    // Where the last pick or restore is. Ready: performance plays; missing lists what it uses that is
    // not here (a plugin not installed plays as a silent stand-in keeping its saved state, an optional
    // sample left out), in the engine's words. Failed: error says why; the Performance before it plays on.
    struct Status
    {
        Load load = Load::None;
        Performance performance;
        std::string error;
        std::vector<std::string> missing;
    };

    // Threads: the host's main thread calls everything but process(); one audio thread calls process(),
    // only between activate() and deactivate(). The engine's Session runs on a control thread of the
    // instance's own, where picks and restores load while process() keeps playing the Performance
    // before them (the engine's switch rules: its notes ring out, the new one starts when ready).
    class Instance
    {
    public:
        explicit Instance(const Config& config); // throws when the engine cannot start
        ~Instance();
        Instance(const Instance&) = delete;
        Instance& operator=(const Instance&) = delete;

        void activate(double sampleRate); // the plugins are activated again at a new rate, keeping their state
        void deactivate();
        // Audio thread: no locks, no allocation. False for a block it could not play (frames 0 or past
        // maxFrames, no main bus, or not active), which then is silence where there is a bus to write.
        bool process(const Block& block) noexcept;

        std::vector<Item> library();              // every Performance installed
        void pick(const std::string& id);         // loads it from the library; returns at once (status())
        Status status() const;

        // The instance's state for the DAW's project, versioned: the Performance's identity and the whole
        // Performance as the engine saves it, packages and samples by reference, not embedded. A restore
        // that did not open is kept and given back as it came, so saving the project loses nothing.
        std::vector<uint8_t> state();
        // Loads a state as pick() does, returning at once; throws std::invalid_argument for bytes that
        // are not a state this build reads (nothing changes then).
        void setState(const std::vector<uint8_t>& state);

    private:
        struct Impl;
        std::unique_ptr<Impl> impl_;
    };
}
