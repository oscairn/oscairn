// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Real-time thread primitives shared by plugin workers (RFC 0010) and the graph's audio
// helpers (RFC 0034): futex (Linux) or os_sync (macOS) waits on a 32-bit word, valid between
// processes, and real-time scheduling for the calling thread.
#pragma once
#include <atomic>
#include <chrono>
#include <climits>
#include <cstdint>

namespace oscairn::worker
{
    // Waits while word == value; false on timeout. Shared between processes.
    bool waitWhileEqual(std::atomic<uint32_t>& word, uint32_t value, std::chrono::nanoseconds timeout) noexcept;
    // Wakes up to `waiters` threads waiting on word (every one by default).
    void wake(std::atomic<uint32_t>& word, int waiters = INT_MAX) noexcept;
    // RFC 0035's priority table for Oscairn's threads: SCHED_FIFO priorities on Linux, above the
    // kernel's 50 for interrupt threads and below the audio interface's (90, the session's to set).
    namespace priority
    {
        constexpr int engineAudio = 80; // the engine's audio thread
        constexpr int workerAudio = 70; // workers' and the graph's helpers' audio threads
    }
    // The calling thread at real-time priority: a time constraint of the period on macOS, SCHED_FIFO
    // at linuxPriority on Linux. On Linux, with OSCAIRN_AUDIO_THREAD_CORES set (a CPU list as the kernel
    // writes one: "1-3", "2,3"), also kept to those CPUs; every audio thread calls this (the engine's,
    // the graph's helpers', each worker's), and no other thread does.
    bool makeRealtime(double periodMicroseconds, int linuxPriority = priority::workerAudio) noexcept;
    // A CPU list's CPUs as bits; 0 for an empty or malformed list or a CPU past 63.
    // shortcut: CPUs 0 to 63 only; use cpu_set_t throughout for a part with more.
    uint64_t cpuMask(const char* list) noexcept;
    // The CPUs an audio thread keeps to: its own side's list (OSCAIRN_ENGINE_AUDIO_CORES for the engine's audio thread and
    // helpers, OSCAIRN_WORKER_AUDIO_CORES for a worker's), else the shared one (OSCAIRN_AUDIO_THREAD_CORES); with its own
    // list and a slot (0 and up: a worker process of `chain` or `pool`, numbered as they start), only the slot-th of its
    // CPUs, round-robin, so groups of different parts land on different cores. 0: anywhere.
    uint64_t audioThreadMask(const char* shared, const char* own, int slot) noexcept;
    // This process's worker slot (a worker process: its number from the engine, -1 for none; the engine: -2).
    void setWorkerSlot(int slot) noexcept;
    // A plugin worker's allowance (RFC 0010, RFC 0045): how long the engine waits for a block it
    // handed a worker, counted from the hand-over, before the block is missed (silenced). Half the
    // block's period: the graph's longest usual chain is two workers one after the other (an
    // instrument, then its effect), and both waiting their whole allowance still fit in the block.
    // `overrideMicroseconds` (OSCAIRN_WORKER_BUDGET_US, the profile's compute.workerAllowance), a
    // whole number of microseconds above 0, replaces it; anything else leaves the derived one.
    std::chrono::nanoseconds workerAllowance(uint32_t frames, double rate, const char* overrideMicroseconds) noexcept;
    // RFC 0045's bounded spin: a waiting audio thread polls a word for up to this long before it
    // sleeps, so a block done (or handed over) within it costs no wake-up. OSCAIRN_WORKER_SPIN_US (the
    // profile's compute.workerSpin), whole microseconds above 0, held to half the allowance; anything
    // else, or a spin with more audio threads than cores to give them (`threads` > `cores`, as a
    // spinning thread keeps its core from another of its priority), none: a worker's, and since
    // 2026-11-13 the engine's for a worker's result too.
    std::chrono::nanoseconds spinWindow(const char* microseconds, std::chrono::nanoseconds allowance, uint32_t threads = 1, uint32_t cores = 1) noexcept;
    // The CPUs this process may run on (its affinity on Linux), at least 1.
    uint32_t usableCores() noexcept;
    // A spinning thread's pause between polls.
    inline void relax() noexcept
    {
#if defined(__aarch64__)
        __asm__ __volatile__("yield");
#elif defined(__x86_64__) || defined(__i386__)
        __builtin_ia32_pause();
#endif
    }
    // RFC 0045: the time by which the audio callback running on this thread must be done with its
    // workers, its start plus the period less the engine's own reserve (mixingReserve of the period, for
    // what follows the last wave: mixing, taps, meters, the recorder's copy). The host's device callback
    // sets it for its length (a host without a device, a test, leaves it unset: no deadline). The graph
    // sets workerDeadline from it for each wave, the time any wait on a worker of that wave ends at
    // the latest: the callback's deadline, or with OSCAIRN_WORKER_DEADLINE (compute.workerDeadline,
    // step 6) the callback's deadline less what the later waves took in the last block. No wait on a
    // worker goes past it, the allowance's or the extra block's; unset (the epoch), the allowance alone.
    constexpr double mixingReserve = .15;
    inline thread_local std::chrono::steady_clock::time_point callbackDeadline{}, workerDeadline{};
    inline thread_local bool workerDeadlineOnly = false; // the graph's: step 6, workerDeadline in place of the allowance
    // Whether OSCAIRN_WORKER_DEADLINE asks for step 6 (a graph reads it as it is made).
    bool deadlineAllowance() noexcept;
    // OSCAIRN_WORKER_REST, read once at the engine's start: which workers whose plugin is asleep with nothing for it
    // are left uncalled (RFC 0045). disabled (unset, or anything but the others): only one no note can reach in the
    // plan playing (Graph's playable); all ("all"): any, after its hold; never ("0"): none, every worker every block.
    enum class WorkerRest { disabled, all, never };
    WorkerRest workerRest() noexcept;
    // OSCAIRN_WORKER_GROUPS, read as each plan is made: unless "0", nodes of one worker process whose audio flows between
    // them are rendered there in one round trip (RFC 0045's plugin groups, Graph's compile); "0": each its own, as before.
    bool workerGroups() noexcept;
    // What became of a block the engine handed a worker, once it came for it: on time (done when it
    // came, or found done within the allowance); late, found done but past the allowance, its sound
    // played all the same (the engine's own thread woke late); or silent, not done within the
    // allowance and silenced. Late and silent both count toward "too slow".
    enum class BlockOutcome { onTime, late, silent };
    constexpr BlockOutcome blockOutcome(bool doneWhenAsked, bool doneAfterWaiting, std::chrono::nanoseconds sinceHandOver, std::chrono::nanoseconds allowance) noexcept
    {
        if(doneWhenAsked) return BlockOutcome::onTime;
        if(!doneAfterWaiting) return BlockOutcome::silent;
        return sinceHandOver > allowance ? BlockOutcome::late : BlockOutcome::onTime;
    }
}
