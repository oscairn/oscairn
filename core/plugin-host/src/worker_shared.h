/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* Plugin worker processes (RFC 0010): the one definition of what the engine and a worker share, the block
 * shared per plugin node, the worker's doorbell and the control socket's request and reply codes, for C++
 * on both sides (Worker.h) and for Rust (core/engine/rust/src/bindings.rs, generated from this file; its
 * test engine_core_h_unchanged fails when this file moves under it).
 *
 * C, so bindgen reads it; C++ sees the same layout with its own types where they are the same bytes:
 * OSCAIRN_ATOMIC(T) is std::atomic<T> (lock-free, T's size), OSCAIRN_WORKER_EVENT_TYPE kick::clap::Event (64
 * bytes, 8-aligned). Worker.h asserts both. Each field's meaning is documented here. */
#pragma once
#include <clap/events.h>
#include <oscairn-clap/voice-budget.h>
#include <stdint.h>

#ifdef __cplusplus
#include <atomic>
#include <kick_clap/Host.h>
#define OSCAIRN_ATOMIC(T) std::atomic<T>
#define OSCAIRN_ALIGN(n) alignas(n)
#define OSCAIRN_WORKER_EVENT_TYPE kick::clap::Event
extern "C" {
#else
#define OSCAIRN_ATOMIC(T) T
#define OSCAIRN_ALIGN(n) _Alignas(n)
#define OSCAIRN_WORKER_EVENT_TYPE oscairn_worker_event
typedef struct { _Alignas(8) uint8_t bytes[64]; } oscairn_worker_event; /* kick::clap::Event */
#endif
typedef OSCAIRN_ATOMIC(uint32_t) oscairn_atomic_u32;
typedef OSCAIRN_ATOMIC(uint64_t) oscairn_atomic_u64;
typedef OSCAIRN_ATOMIC(double) oscairn_atomic_f64;

enum
{
    OSCAIRN_WORKER_MAX_EVENTS = 2 * 512 + 1 + 32, /* kick::clap::Host::maxProcessEvents: a graph block's notes, its beat and harmony */
    OSCAIRN_WORKER_MAX_OUTPUT_EVENTS = 1024,
    OSCAIRN_WORKER_MAX_PARAMETERS = 4096,
    OSCAIRN_WORKER_MAX_VOICE_NOTES = 512 /* kick::clap::Host::maxBlockEvents: the notes a voice-budget report holds */
};

/* Header of the block shared per node; audio follows it (planar floats, inputs then outputs, maxFrames each),
 * then the SysEx pools. request and done carry block sequence numbers; the engine writes everything before
 * request, the worker after done. */
typedef struct oscairn_worker_shared
{
    OSCAIRN_ALIGN(64) oscairn_atomic_u32 request; /* the block 64-aligned, as each hot word */
    OSCAIRN_ALIGN(64) oscairn_atomic_u32 done;
    OSCAIRN_ALIGN(64) oscairn_atomic_u32 panic;
    oscairn_atomic_u32 crashed; /* the signal that is ending the worker, set as it dies */
    oscairn_atomic_u32 asleep;  /* the plugin said, in the last block, that it is asleep (CLAP_PROCESS_SLEEP) */
    /* The worker's: asleep after its last block with nothing pending but input (PluginHost::restful), and a wake
     * pending since (a request_process, command, select, choke or panic reached the plugin; PluginHost::mirrorWake).
     * The engine's, with each request: the blocks it left the plugin asleep uncalled since the last, and their frames. */
    oscairn_atomic_u32 restful, wake;
    uint32_t restedBlocks, restedFrames;
    /* The engine's: its thread is (about to be) asleep on done, so the worker wakes it; otherwise the worker makes
     * no system call to hand a block back (RFC 0045, plugin hosts). */
    oscairn_atomic_u32 waiting;
    uint32_t frames, eventCount, hasTransport, ok, outputCount;
    uint32_t inputChannels, outputChannels, maxFrames, parameterCount;
    /* outputChannels ends with ringOutChannels (4 for a plugin with a ring-out buffer, RFC 0005, else 0); ringOut:
     * this block's caller asked for them. */
    uint32_t ringOutChannels, ringOut;
    uint32_t reset; /* the worker's: the plugin was reset in the last block (wasReset); nonzero is true */
    /* The engine's, with each request (RFC 0045): how long, in nanoseconds, the worker's audio thread may spin
     * around the next block's expected hand-over instead of sleeping until it; 0: sleep (spinWindow, realtime.h). */
    uint32_t spinNs;
    double sampleRate;
    clap_event_transport_t transport;
    /* oscairn.voice-budget (RFC 0018). The engine's: when setLimits, the limits the plugin takes before this block.
     * The worker's: what the plugin reported after it, which the engine bounds. */
    uint32_t setLimits, voiceLimit, voiceContextCount;
    oscairn_voice_budget_context_t voiceContexts[OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS];
    uint32_t activeVoices, voiceNoteCount;
    double voiceCost;
    oscairn_voice_budget_note_t voiceNotes[OSCAIRN_WORKER_MAX_VOICE_NOTES];
    oscairn_atomic_u64 outputDrops, blocks, droppedEvents, processErrors, deadlineMisses, controlSilences, idleBlocks;
    oscairn_atomic_f64 lastMicroseconds, maximumMicroseconds, cpuMicroseconds; /* cpuMicroseconds: the audio thread's CPU time for its blocks so far */
    /* The worker's, for the timing report: when its audio thread ran for the last block and when it was done with
     * it, in steady_clock nanoseconds (the clock is the system's, the same in both processes); the CPU time its
     * audio thread used between the two (CLOCK_THREAD_CPUTIME_ID), in nanoseconds; and its audio thread's page
     * faults and involuntary context switches so far (Linux). */
    int64_t wokeAt, doneAt, cpuNs;
    oscairn_atomic_u64 minorFaults, majorFaults, involuntarySwitches;
    OSCAIRN_WORKER_EVENT_TYPE events[OSCAIRN_WORKER_MAX_EVENTS];
    OSCAIRN_WORKER_EVENT_TYPE outputs[OSCAIRN_WORKER_MAX_OUTPUT_EVENTS];
    oscairn_atomic_f64 parameterValues[OSCAIRN_WORKER_MAX_PARAMETERS];
    /* The events' and outputs' SysEx bytes (see Worker.h's share), after the audio, at these offsets from the
     * block's start; 0: none, for a plugin that cannot receive (send) SysEx (PluginHost::takesSysex), whose SysEx
     * is then dropped and counted on the engine's side (sysexDrops) and in its host (dropped). */
    uint32_t inputPool, outputPool;
} oscairn_worker_shared;

/* A worker process's one doorbell (RFC 0045, plugin hosts): its single audio thread sleeps on ring, which the
 * engine moves after handing any of its nodes a block, and then renders every node with a request pending, in
 * node order. The engine makes the wake-up system call only while the worker says it sleeps. rendering and
 * serving name the node the audio thread renders and the node the control thread serves (UINT32_MAX: none),
 * read by the engine after a hang to tell which plugin it was; faulted, set by the crash handler, the one the
 * thread that crashed was in (its rendering or serving; UINT32_MAX for none, or a thread of the plugins' own).
 * The worker may write anything, so the engine only looks the index up among its own nodes. */
typedef struct oscairn_worker_doorbell
{
    OSCAIRN_ALIGN(64) oscairn_atomic_u32 ring;
    oscairn_atomic_u32 sleeping;
    OSCAIRN_ALIGN(64) oscairn_atomic_u32 rendering, serving, faulted;
} oscairn_worker_doorbell;

/* Requests, the first byte of each. Init, Load and Shutdown concern the worker; every other request is followed
 * by the index of the plugin node it addresses. A worker holds the plugins loaded into it (one, or several in a
 * plugin host, RFC 0045) and the effect contexts created from each. */
enum oscairn_worker_op
{
    OSCAIRN_WORKER_INIT = 1, OSCAIRN_WORKER_SERVICE, OSCAIRN_WORKER_SET_PARAMETER, OSCAIRN_WORKER_GESTURE,
    OSCAIRN_WORKER_PARAMETER_TEXT, OSCAIRN_WORKER_SAVE_STATE, OSCAIRN_WORKER_LOAD_STATE, OSCAIRN_WORKER_SHUTDOWN,
    OSCAIRN_WORKER_CREATE_EFFECT_CONTEXT, OSCAIRN_WORKER_DESTROY_NODE, OSCAIRN_WORKER_EFFECT_RESOURCES,
    OSCAIRN_WORKER_SOUND_CONTEXTS, OSCAIRN_WORKER_PREPARE_SOUND, OSCAIRN_WORKER_SELECT_SOUND, OSCAIRN_WORKER_DISCARD_SOUND,
    OSCAIRN_WORKER_SAVE_SNAPSHOT, OSCAIRN_WORKER_LOAD_SNAPSHOT, OSCAIRN_WORKER_ASSET_DEPENDENCIES, OSCAIRN_WORKER_LOAD_PRESET,
    OSCAIRN_WORKER_MEMORY, OSCAIRN_WORKER_SET_MEMORY_ROOM,
    OSCAIRN_WORKER_WINDOW,           /* the plugin's own window: shown (with its title) or closed */
    OSCAIRN_WORKER_RESET,
    OSCAIRN_WORKER_PREPARE_SNAPSHOT, /* a native snapshot's sounds prepared beside the playing ones (RFC 0021) */
    OSCAIRN_WORKER_CHOKE_RETIRING,   /* sound-snapshots' choke_retiring (RFC 0005) */
    OSCAIRN_WORKER_SAVE_SOUND, OSCAIRN_WORKER_SOUND_VALUES, /* sound-snapshots draft 4: one sound on its own, a sound's values */
    OSCAIRN_WORKER_RECONFIGURE,      /* the plugin activated again at a new sample rate, the block size kept */
    OSCAIRN_WORKER_LOAD,             /* one more plugin in the worker, beside those it holds (Init sets the rate and block first) */
    OSCAIRN_WORKER_ACTIVE_OUTPUTS,   /* the output ports the engine takes sound from (clap.audio-ports-activation) */
    OSCAIRN_WORKER_EVENT             /* one event to the plugin at its next block (PluginHost::sendEvent) */
};
/* The first byte of every message from the worker: a reply, or a request to the engine made while the worker
 * serves one (the plugin resolving a sound asset or finding an asset library; a node about to activate, with
 * what it declares, for the engine to refit the worker's memory limit before it allocates). */
enum oscairn_worker_reply
{
    OSCAIRN_WORKER_FAILED = 0, OSCAIRN_WORKER_DONE = 1, OSCAIRN_WORKER_RESOLVE_ASSET = 2, OSCAIRN_WORKER_RESERVE = 3,
    OSCAIRN_WORKER_INVALID = 4 /* std::invalid_argument */, OSCAIRN_WORKER_CREATE_ASSET = 5, OSCAIRN_WORKER_LIBRARY_ROOT = 6,
    OSCAIRN_WORKER_RESOLVE_SAMPLE = 7
};

#ifdef __cplusplus
}
#endif
