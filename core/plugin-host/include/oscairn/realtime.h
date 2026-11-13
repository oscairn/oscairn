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
}
