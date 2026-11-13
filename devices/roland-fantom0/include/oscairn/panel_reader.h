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
#include <unordered_map>
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
        int64_t analogDeadband = 0;                // steps: an analog control turning back by this much or less is chatter (PanelEvent)
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
        // An analog control's value that only chatters (set by PanelReader, with the map's
        // analogDeadband): the same value again, or a turn back by the dead band or less, short of
        // either end. A move on in the same direction is never chatter, so a slow move arrives step
        // by step and the ends are always reached. The engine plays no chatter; the dump shows it.
        bool chatter = false;
    };
    PanelEvent decode(const PanelMap&, const std::array<uint8_t, 4>& message);

    // The panel's device is held by one long-lived process, oscairn-panel-holder (docs/fantom-0.md,
    // "The panel holder"): Roland's driver registers the panel's address only while the device is
    // open, the panel stops reporting after it has been left unheld for a while, and a second open
    // can crash the kernel. Readers attach to the holder's socket instead of opening the device:
    // an abstract name on Linux ("@" first), or a path (tests).
    inline constexpr const char* panelHolderSocket = "@oscairn-panel";
    // A stream of the holder's 4-byte messages; -1 and errno when no holder listens there.
    int connectPanelHolder(const std::string& socket);
    // The processes other than this one with `path` open (by /proc; none where there is no /proc).
    std::vector<int> processesHolding(const std::string& path);
    // How the holder opens the device and waits on it; empty: ::open and ioctl(fd, 0, 0). A test's
    // seam: a driver's ioctl cannot be had without its kernel module.
    struct PanelDevice
    {
        std::function<int(const std::string& path, int flags)> open;
        std::function<int(int fd)> wait; // the driver's returns 0 for a message and for a signal alike
        int patience = 2000;             // milliseconds to wait for another holder of the device to let go
    };
    // The holder: binds `socket` first, as its lock, and refuses (1) if another holder has it; then
    // opens `path` once for reading and writing, only while no other process has it open, retrying
    // a refused open (EFAULT: the last holder not gone yet) for `patience`; then, until SIGTERM or
    // SIGINT, waits on it, reads 4 bytes at a time until a read gives less, and sends each message
    // to every attached reader (none attached: dropped). It never writes to the device, polls it or
    // opens it again. A reader attaching raises SIGIO, which wakes the wait to take it in (or the
    // next message does). Returns 0 when stopped, 1 when refused or failed; logs each step.
    int holdPanel(const std::string& path, const std::string& socket, const std::function<void(const std::string&)>& log, PanelDevice device = {});

    // Reads a panel on a thread of its own, so it never blocks anything else; frames its bytes into
    // messages (a byte with bit 7 set starts one; anything else before it is skipped, so it
    // resynchronizes after garbage or a lost byte), decodes each through the map and hands it to
    // `sink` on that thread, unknown ones too. Unknown messages are also logged with their bytes,
    // at most ten a second.
    class PanelReader
    {
    public:
        using Sink = std::function<void(const PanelEvent&)>;
        using Log = std::function<void(const std::string&)>; // empty: no log
        static void standardError(const std::string& line);
        // The panel holder at `socket` (panelHolderSocket on the unit): attached to again a second
        // after it cannot be reached or ends. The device itself is never opened here.
        PanelReader(std::string socket, PanelMap map, Sink sink, Log log = standardError);
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
        void bytes(const uint8_t* data, size_t size);
        bool pause(); // a second, or until stopped; false when stopped

        std::string path_; // the holder's socket
        int fd_ = -1;
        bool owned_ = false;
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
        struct Analog { int64_t value = -1; int direction = 0; }; // the last value passed on, and which way it moved
        std::unordered_map<uint32_t, Analog> analog_;
        int64_t window_ = 0; uint32_t logged_ = 0; uint64_t hidden_ = 0; // the log's rate limit, per second
        std::thread thread_; // last: started once the rest is ready
    };
}
