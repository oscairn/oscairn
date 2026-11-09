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
    // at linuxPriority on Linux.
    bool makeRealtime(double periodMicroseconds, int linuxPriority = priority::workerAudio) noexcept;
}
