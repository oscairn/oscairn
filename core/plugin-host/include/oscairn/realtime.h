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
    // else, or a worker's spin with more audio threads than cores to give them (`threads` > `cores`,
    // as a spinning thread keeps its core from another of its priority), none.
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
