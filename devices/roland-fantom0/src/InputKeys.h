// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// Keys of a Linux input device that are not on the panel bus: on the FANTOM-0, WRITE, MENU and
// TEMPO, which are gpio-keys on /dev/input/event1 (docs/fantom-0.md, "WRITE, MENU and TEMPO").
// Read with evdev, read-only and never grabbed, so Weston and anything else reading the device
// still get every event. Linux only.

#include "oscairn/physical_controls.h" // ControlInput

#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <map>
#include <mutex>
#include <string>
#include <thread>

namespace oscairn::host
{
    class InputKeys
    {
    public:
        /// A key pressed (1) or released (0), by the function `keys` gives its code ("menu").
        using Play = std::function<void( const std::string& control, const ControlInput& )>;
        using Log = std::function<void( const std::string& )>;
        static void standardError( const std::string& line );
        /// The input device named `name` (EVIOCGNAME, as `gpio-keys`) among `folder`'s event*
        /// files, found again a second after it cannot be found or fails.
        InputKeys( std::string name, std::map<uint16_t, std::string> keys, Play play, Log log = standardError, std::string folder = "/dev/input" );
        /// An open descriptor of `struct input_event` records, not owned (tests: a pipe): read until it ends.
        InputKeys( int fd, std::map<uint16_t, std::string> keys, Play play, Log log = {} );
        ~InputKeys(); // stops and joins
        InputKeys( const InputKeys& ) = delete;
        InputKeys& operator=( const InputKeys& ) = delete;

    private:
        void run();
        int find(); // the named device, opened read-only; -1: none
        bool pause(); // a second, or until stopped; false when stopped

        std::string name_, folder_;
        std::map<uint16_t, std::string> keys_;
        Play play_;
        Log log_;
        int fd_ = -1;
        bool owned_ = false;
        std::mutex lock_;
        std::condition_variable stopped_;
        std::atomic<bool> stop_{ false };
        std::thread thread_; // last: started once the rest is ready
    };
}
