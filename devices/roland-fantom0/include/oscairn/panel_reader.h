// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include "oscairn/physical_controls.h" // ControlInput

#include <array>
#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace oscairn
{
    // A device panel that does not speak MIDI (a profile's `panel`, RFC 0024): 4-byte messages from
    // a character device, the first byte of each with bit 7 set (Roland's i2c-slave-rx driver on the
    // FANTOM-0, /dev/i2c-slave-2). Where the kind, ID, value and step sit in a message is the map's
    // `layout`, data and not code (docs/fantom-0.md says how the FANTOM-0's is known); the map then
    // names the control each kind and ID is, by identity ("fader:1", as overrides name them) or, for
    // a fixed control, by its profile function ("enter").
    struct PanelMap
    {
        // A number from a message: `bytes` (indices 0 to 3) joined, most significant first, `bits`
        // of each (from the low end); then shifted right by `shift`, masked, and read as two's
        // complement in the mask's width when `sign`. Or, for a number whose bits are not the low
        // bits of each byte, `parts` joined most significant first: `bits` of a byte from bit
        // `shift` up, two's complement in their total width when `sign`.
        struct Part { uint8_t byte = 0, shift = 0, bits = 8; };
        struct Field
        {
            std::vector<uint8_t> bytes; uint32_t bits = 8, shift = 0, mask = 0xFFFFFFFF; bool sign = false;
            std::vector<Part> parts;
            int64_t read(const std::array<uint8_t, 4>&) const;
        };
        // The message kinds the panel sends (Roland's names: PMDWEncoder, PMDWADChange,
        // PMDWSwitchOn, PMDWSwitchOff).
        enum class Kind { Unknown, Encoder, Analog, SwitchOn, SwitchOff };
        Field kind, id, value, delta;
        Field analogId; // an analog message's ID where it is not `id` (the FANTOM-0's is narrower); unset: `id`
        std::array<int64_t, 4> kinds{0, 1, 2, 3}; // the kind field's value for encoder, analog, switchOn and switchOff
        int64_t analogMax = 1023;                  // an analog control's top value, unless its entry says
        double encoderStep = 1.0 / 127;            // the change for one encoder step, 0 to 1 across a target's range
        // A control: its message kind (encoder, analog, or switch for both switch kinds), ID and
        // identity; `max` and `step` override the layout's.
        struct Control { Kind kind = Kind::Unknown; uint32_t id = 0; std::string control; int64_t max = 0; double step = 0; };
        std::vector<Control> controls;
        std::vector<std::string> estimated; // "layout", "controls": not yet read off a real unit

        // Strict, as profiles are: unknown fields, wrong types and impossible values refuse it
        // (std::invalid_argument). An entry without an id is a control not mapped yet, skipped.
        static PanelMap parse(std::string_view json);
    };
    const char* kindName(PanelMap::Kind);

    // One message decoded: its bytes, kind, ID and value or step, and (with a control) what it
    // plays. `control` empty: no entry has that kind and ID, or the kind is none of the four.
    struct PanelEvent
    {
        std::array<uint8_t, 4> raw{};
        PanelMap::Kind kind = PanelMap::Kind::Unknown;
        uint32_t id = 0;
        int64_t value = 0; // the value field, or for an encoder the delta field
        std::string control;
        ControlInput input;
    };
    PanelEvent decode(const PanelMap&, const std::array<uint8_t, 4>& message);

    // Reads a panel on a thread of its own, so it never blocks anything else; frames its bytes into
    // messages (a byte with bit 7 set starts one; anything else before it is skipped, so it
    // resynchronizes after garbage or a lost byte), decodes each through the map and hands it to
    // `sink` on that thread, unknown ones too. Unknown messages are also logged with their bytes,
    // at most ten a second.
    //
    // A device by path is read as Roland's programs read /dev/i2c-slave-2 (docs/fantom-0.md, "Panel
    // messages"): opened for reading and writing (it is never written to), `ioctl(fd, 0, 0)`, which
    // waits for a message, then one 4-byte read, and the ioctl again. The driver's wait gives way to
    // no signal but a fatal one, so a helper process forked for it does that loop and writes each
    // message to a pipe this reader polls with a timeout; stopping kills it. Where the ioctl fails,
    // or wakes 100 times running with nothing to read, that is logged once and the device is opened
    // again read-only and polled instead, as a descriptor given always is.
    class PanelReader
    {
    public:
        using Sink = std::function<void(const PanelEvent&)>;
        using Log = std::function<void(const std::string&)>; // empty: no log
        static void standardError(const std::string& line);
        // How a device by path is opened and waited on; empty: ::open and ioctl(fd, 0, 0). A test's
        // seam: a driver's ioctl cannot be had without its kernel module.
        struct Device
        {
            std::function<int(const std::string& path, int flags)> open;
            std::function<int(int fd)> wait; // -1 and errno on failure
            std::vector<int> keep;           // descriptors the helper keeps open beside its own (a test's)
        };
        // The device at `path`: opened again a second after it fails or ends.
        PanelReader(std::string path, PanelMap map, Sink sink, Log log = standardError, Device device = {});
        // An open descriptor, not owned (tests: a pipe or a pty): read until it ends.
        PanelReader(int fd, PanelMap map, Sink sink, Log log = standardError);
        ~PanelReader(); // stops and joins
        PanelReader(const PanelReader&) = delete;
        PanelReader& operator=(const PanelReader&) = delete;

        void setMap(PanelMap map); // any thread; the next message decodes through it
        struct Stats { uint64_t messages = 0, skipped = 0, unknown = 0; }; // skipped: bytes outside any message
        Stats stats() const;

    private:
        void run();
        [[noreturn]] void helper(int out); // the forked helper's loop, writing each message to `out`
        bool spawn();                      // forks it; fd_ its pipe's reading end
        bool reap();                       // it ended: why (false: it could not open the device, errno set)
        void bytes(const uint8_t* data, size_t size);
        bool pause(); // a second, or until stopped; false when stopped

        std::string path_;
        Device device_;
        int fd_ = -1;
        bool owned_ = false;
        bool roland_ = false; // reading the device as Roland's programs do, through the helper (reader thread)
        int helper_ = -1;     // the helper's process ID
        Sink sink_;
        Log log_;
        mutable std::mutex lock_; // the map, and the pause's wake
        std::shared_ptr<const PanelMap> map_;
        std::condition_variable stopped_;
        std::atomic<bool> stop_{false};
        std::atomic<uint64_t> messages_{0}, skipped_{0}, unknown_{0};
        // The reader thread's own.
        std::array<uint8_t, 4> frame_{};
        size_t have_ = 0;
        int64_t window_ = 0; uint32_t logged_ = 0; uint64_t hidden_ = 0; // the log's rate limit, per second
        std::thread thread_; // last: started once the rest is ready
    };
}
