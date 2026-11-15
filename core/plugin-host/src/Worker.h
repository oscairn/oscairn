// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Plugin worker processes (RFC 0010): the block shared between the engine and a worker,
// cross-process wait/wake, and the control-socket encoding. Internal to oscairn_plugin_host.
#pragma once
#include <kick_clap/Host.h>
#include "oscairn/realtime.h"
#include "worker_shared.h"
#include <atomic>
#include <chrono>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>
#include <unistd.h>

namespace oscairn::worker
{
    // The layout is worker_shared.h's, the one definition engine, worker and the Rust bindings share.
    constexpr uint32_t maxEvents = OSCAIRN_WORKER_MAX_EVENTS;
    constexpr uint32_t maxOutputEvents = OSCAIRN_WORKER_MAX_OUTPUT_EVENTS;
    constexpr uint32_t maxParameters = OSCAIRN_WORKER_MAX_PARAMETERS;
    constexpr uint32_t maxVoiceNotes = OSCAIRN_WORKER_MAX_VOICE_NOTES;
    static_assert(maxEvents == kick::clap::Host::maxProcessEvents && maxVoiceNotes == kick::clap::Host::maxBlockEvents);
    static_assert(sizeof(kick::clap::Event) == 64 && alignof(kick::clap::Event) == 8);
    static_assert(sizeof(oscairn_atomic_u32) == 4 && sizeof(oscairn_atomic_u64) == 8 && sizeof(oscairn_atomic_f64) == 8 &&
                  alignof(oscairn_atomic_u64) == 8 && alignof(oscairn_atomic_f64) == 8); // as the C view (bindgen) lays them out

    // The shared block's header (worker_shared.h), zeroed but for the sample rate; audio follows it.
    struct Shared : oscairn_worker_shared
    {
        Shared() noexcept : oscairn_worker_shared{} { sampleRate = 48000; }
        kick::clap::SysexPool* inputBytes() noexcept { return pool(inputPool); }
        kick::clap::SysexPool* outputBytes() noexcept { return pool(outputPool); }
        float* input(uint32_t channel) noexcept { return reinterpret_cast<float*>(this + 1) + size_t(channel) * maxFrames; }
        float* output(uint32_t channel) noexcept { return input(inputChannels + channel); }
        // The audio's end, then each pool there is, 8-byte aligned.
        static size_t audioEnd(uint32_t inputs, uint32_t outputs, uint32_t frames) { return (sizeof(Shared) + sizeof(float) * size_t(inputs + outputs) * frames + 7) & ~size_t(7); }
        static size_t bytes(uint32_t inputs, uint32_t outputs, uint32_t frames, bool sysexIn = false, bool sysexOut = false)
        {
            return audioEnd(inputs, outputs, frames) + (size_t(sysexIn) + size_t(sysexOut)) * sizeof(kick::clap::SysexPool);
        }
    private:
        kick::clap::SysexPool* pool(uint32_t at) noexcept { return at ? reinterpret_cast<kick::clap::SysexPool*>(reinterpret_cast<char*>(this) + at) : nullptr; }
    };
    // Writes every page of a shared mapping once, keeping what is there, so no first write on an audio
    // thread faults. Locking (mlockall) maps a shared page on a read, clean, and the first write to it
    // then faults to mark it dirty where the CPU does not track dirty pages itself (ARMv8.0 has no
    // hardware dirty bits: the FANTOM-07's Cortex-A53). In the Linux container the audio threads took
    // 29 to 30 such faults from the first block (two of them the first time a held note was struck
    // again), the engine's thread 1 to 2. An atomic add of 0 to each page's first word: safe beside the
    // other process's writes. Off the audio path, before the mapping is used there.
    inline void dirtyPages(void* memory, size_t bytes) noexcept
    {
        const size_t page = size_t(sysconf(_SC_PAGESIZE));
        for(size_t at = 0; at < bytes; at += page)
            std::atomic_ref<uint32_t>(*reinterpret_cast<uint32_t*>(static_cast<char*>(memory) + at)).fetch_add(0, std::memory_order_relaxed);
    }
    // SysEx bytes cross the shared block in its pools, where the event's buffer holds an offset:
    // share copies them into the pool and stores the offset; unshare points the event at the
    // pool in this process's mapping, false when the offset or size does not fit the pool (the
    // other process wrote garbage). Other events pass unchanged; share is false when the bytes
    // do not fit (the event is to be dropped).
    // No pool (a plugin that cannot take or send SysEx): SysEx is not shared, to be dropped.
    inline bool share(kick::clap::Event& e, kick::clap::SysexPool* pool) noexcept
    {
        auto* h = reinterpret_cast<clap_event_header_t*>(e.bytes.data());
        if(h->space_id != CLAP_CORE_EVENT_SPACE_ID || h->type != CLAP_EVENT_MIDI_SYSEX) return true;
        if(!pool || !pool->hold(e)) return false;
        auto& x = *reinterpret_cast<clap_event_midi_sysex_t*>(h);
        x.buffer = reinterpret_cast<const uint8_t*>(uintptr_t(x.buffer - pool->bytes));
        return true;
    }
    inline bool unshare(kick::clap::Event& e, const kick::clap::SysexPool* pool) noexcept
    {
        auto* h = reinterpret_cast<clap_event_header_t*>(e.bytes.data());
        if(h->space_id != CLAP_CORE_EVENT_SPACE_ID || h->type != CLAP_EVENT_MIDI_SYSEX) return true;
        if(!pool || h->size != sizeof(clap_event_midi_sysex_t)) return false;
        auto& x = *reinterpret_cast<clap_event_midi_sysex_t*>(h);
        const auto offset = uintptr_t(x.buffer);
        if(offset > kick::clap::SysexPool::maxBlock || x.size > kick::clap::SysexPool::maxBlock - offset) return false;
        x.buffer = pool->bytes + offset;
        return true;
    }
    static_assert(std::atomic<uint32_t>::is_always_lock_free && std::atomic<double>::is_always_lock_free && std::atomic<uint64_t>::is_always_lock_free);

    struct Doorbell : oscairn_worker_doorbell // see worker_shared.h
    {
        Doorbell() noexcept : oscairn_worker_doorbell{} { rendering.store(UINT32_MAX); serving.store(UINT32_MAX); faulted.store(UINT32_MAX); }
    };

    // The requests and replies (worker_shared.h), by their C++ names.
    enum class Op : uint8_t
    {
        Init = OSCAIRN_WORKER_INIT, Service = OSCAIRN_WORKER_SERVICE, SetParameter = OSCAIRN_WORKER_SET_PARAMETER, Gesture = OSCAIRN_WORKER_GESTURE,
        ParameterText = OSCAIRN_WORKER_PARAMETER_TEXT, SaveState = OSCAIRN_WORKER_SAVE_STATE, LoadState = OSCAIRN_WORKER_LOAD_STATE,
        Shutdown = OSCAIRN_WORKER_SHUTDOWN, CreateEffectContext = OSCAIRN_WORKER_CREATE_EFFECT_CONTEXT, DestroyNode = OSCAIRN_WORKER_DESTROY_NODE,
        EffectResources = OSCAIRN_WORKER_EFFECT_RESOURCES, SoundContexts = OSCAIRN_WORKER_SOUND_CONTEXTS, PrepareSound = OSCAIRN_WORKER_PREPARE_SOUND,
        SelectSound = OSCAIRN_WORKER_SELECT_SOUND, DiscardSound = OSCAIRN_WORKER_DISCARD_SOUND, SaveSnapshot = OSCAIRN_WORKER_SAVE_SNAPSHOT,
        LoadSnapshot = OSCAIRN_WORKER_LOAD_SNAPSHOT, AssetDependencies = OSCAIRN_WORKER_ASSET_DEPENDENCIES, LoadPreset = OSCAIRN_WORKER_LOAD_PRESET,
        Memory = OSCAIRN_WORKER_MEMORY, SetMemoryRoom = OSCAIRN_WORKER_SET_MEMORY_ROOM, Window = OSCAIRN_WORKER_WINDOW, Reset = OSCAIRN_WORKER_RESET,
        PrepareSnapshot = OSCAIRN_WORKER_PREPARE_SNAPSHOT, ChokeRetiring = OSCAIRN_WORKER_CHOKE_RETIRING, SaveSound = OSCAIRN_WORKER_SAVE_SOUND,
        SoundValues = OSCAIRN_WORKER_SOUND_VALUES, Reconfigure = OSCAIRN_WORKER_RECONFIGURE, Load = OSCAIRN_WORKER_LOAD,
        ActiveOutputs = OSCAIRN_WORKER_ACTIVE_OUTPUTS, Event = OSCAIRN_WORKER_EVENT
    };
    enum Reply : char
    {
        Failed = OSCAIRN_WORKER_FAILED, Done = OSCAIRN_WORKER_DONE, ResolveAsset = OSCAIRN_WORKER_RESOLVE_ASSET, Reserve = OSCAIRN_WORKER_RESERVE,
        Invalid = OSCAIRN_WORKER_INVALID, CreateAsset = OSCAIRN_WORKER_CREATE_ASSET, LibraryRoot = OSCAIRN_WORKER_LIBRARY_ROOT, ResolveSample = OSCAIRN_WORKER_RESOLVE_SAMPLE
    };

    // Length-prefixed messages over a stream socket.
    struct Writer
    {
        std::string data;
        Writer& u8(uint8_t v) { data.push_back(char(v)); return *this; }
        Writer& u32(uint32_t v) { data.append(reinterpret_cast<const char*>(&v), 4); return *this; }
        Writer& f64(double v) { data.append(reinterpret_cast<const char*>(&v), 8); return *this; }
        Writer& u64(uint64_t v) { data.append(reinterpret_cast<const char*>(&v), 8); return *this; }
        Writer& str(const std::string& v) { u32(uint32_t(v.size())); data += v; return *this; }
        Writer& bytes(const void* p, size_t n) { data.append(static_cast<const char*>(p), n); return *this; }
    };
    struct Reader
    {
        std::string data; size_t at = 0;
        void need(size_t n) { if(data.size() - at < n) throw std::runtime_error("Truncated worker message"); }
        uint8_t u8() { need(1); return uint8_t(data[at++]); }
        uint32_t u32() { uint32_t v; need(4); std::memcpy(&v, data.data() + at, 4); at += 4; return v; }
        double f64() { double v; need(8); std::memcpy(&v, data.data() + at, 8); at += 8; return v; }
        uint64_t u64() { uint64_t v; need(8); std::memcpy(&v, data.data() + at, 8); at += 8; return v; }
        // A record count, refused when the rest of the message cannot hold that many records of at least `minimum` bytes.
        uint32_t count(size_t minimum) { const auto n = u32(); if(n > (data.size() - at) / minimum) throw std::runtime_error("Worker sent an impossible count"); return n; }
        std::string str() { const auto n = u32(); need(n); std::string v = data.substr(at, n); at += n; return v; }
        void bytes(void* p, size_t n) { need(n); std::memcpy(p, data.data() + at, n); at += n; }
    };
    // Throw on a closed or failed socket; receive enforces maxBytes.
    void send(int socket, const std::string& message);
    // A reply that did not come within the socket's timeout: the worker is alive but late.
    struct WorkerTimeout : std::runtime_error { WorkerTimeout() : std::runtime_error("Plugin worker did not answer in time") {} };
    std::string receive(int socket, size_t maxBytes = 64u << 20); // throws WorkerTimeout, or runtime_error when the connection is gone
    void sendDescriptor(int socket, int fd);
    int receiveDescriptor(int socket);

    // The worker process: hosts the plugins loaded into it and their effect contexts, serving control
    // requests on `socket`, every node's blocks rendered by one audio thread.
    int run(int socket);
}
