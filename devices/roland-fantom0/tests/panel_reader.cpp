// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A non-MIDI device panel (panel_reader.h): 4-byte messages through a pipe, framed on bit 7 of
// the first byte after garbage, a cut-short message and a read split in two; each kind decoded
// through the map's layout (fields of whole bytes, or of parts of them), an unknown ID reported
// with its bytes, and a new map taking effect;
// then the controls they name played in a Session as a MIDI controller's are (controller_input.h),
// heard at the next block; an analog control's chatter marked by the dead band; the panel holder,
// through a seam: it waits, then reads until nothing is left, passes each message to the readers
// attached, keeps the device while readers come and go, refuses to start beside another holder
// or while another process has the device open, and ends promptly on SIGTERM; and
// oscairn-panel-dump attached to a holder, or refusing without one. Also: the FANTOM-07's map
// decodes messages in Roland's layout, with each control's scale, its pads and its jacks' sensing,
// and a broken map is refused.
#include "oscairn/panel_reader.h"
#include "oscairn/session.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <csignal>
#include <poll.h>
#include <sstream>
#include <string>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>
#include <vector>

using namespace oscairn;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    bool near(double a, double b) { return std::abs(a - b) < 1e-5; }

    // A layout for the test: byte 0 is 0x80 and the kind, byte 1 the ID, bytes 2 and 3 a 14-bit
    // value (seven bits each), byte 2 the encoder's signed step.
    std::string mapJson(const std::string& controls)
    {
        return R"({"format": "oscairn.panel-map", "version": 1, "protocol": "roland-i2c-slave",
            "layout": {"kind": {"bytes": [0], "mask": 15}, "kinds": {"encoder": 0, "analog": 1, "switchOn": 2, "switchOff": 3},
                       "id": {"bytes": [1], "mask": 127}, "value": {"bytes": [2, 3], "bits": 7}, "delta": {"bytes": [2], "mask": 127, "signed": true},
                       "analogMax": 1000, "encoderStep": 0.01},
            "controls": [)" + controls + "]}";
    }
    const std::string controls = R"({"kind": "analog", "id": 3, "control": "fader:1"}, {"kind": "encoder", "id": 4, "control": "knob:1"},
        {"kind": "switch", "id": 5, "control": "button:101"}, {"kind": "analog", "id": 6, "control": "fader:2", "max": 500})";

    std::array<uint8_t, 4> message(int kind, int id, int value) { return {uint8_t(0x80 | kind), uint8_t(id), uint8_t((value >> 7) & 0x7F), uint8_t(value & 0x7F)}; }
    std::array<uint8_t, 4> step(int id, int delta) { return {0x80, uint8_t(id), uint8_t(delta & 0x7F), 0}; } // an encoder
    void put(int fd, const std::vector<uint8_t>& bytes) { check(::write(fd, bytes.data(), bytes.size()) == ssize_t(bytes.size()), "write to the pipe"); }
    void put(int fd, const std::array<uint8_t, 4>& m) { put(fd, std::vector<uint8_t>(m.begin(), m.end())); }

    // What the sink heard, from the reader's thread.
    struct Heard
    {
        std::mutex lock;
        std::vector<PanelEvent> events;
        std::vector<std::string> log;
        bool wait(size_t count)
        {
            for(int i = 0; i < 300; ++i) { { std::lock_guard g(lock); if(events.size() >= count) return true; } std::this_thread::sleep_for(std::chrono::milliseconds(10)); }
            return false;
        }
        PanelEvent at(size_t i) { std::lock_guard g(lock); return events.at(i); }
    };

    void maps(const char* shipped)
    {
        std::ifstream in(shipped); std::stringstream text; text << in.rdbuf();
        const auto fantom = PanelMap::parse(text.str());
        check(fantom.controls.size() == 130 && fantom.estimated.empty() && fantom.analogDeadband == 1, "the FANTOM-07's map parses, every entry with its id");
        // Roland's layout (docs/fantom-0.md). Slider 1 (analog 9) at 1000: byte 2 holds the ID's top
        // two bits and, from bit 2, the value's top five, which must not read as ID bits.
        auto f = decode(fantom, {0x84, 9, uint8_t((1000 >> 7) << 2), uint8_t(1000 & 0x7F)});
        check(f.kind == PanelMap::Kind::Analog && f.id == 9 && f.value == 1000 && f.control == "fader:1", "the FANTOM's slider 1 at 1000: id " + std::to_string(f.id));
        // The VALUE dial (encoder 6) three steps back: -3 is 0xFD, its top four bits in byte 2 from bit 3, its low four in byte 3.
        f = decode(fantom, {0x83, 6, 0xF << 3, 0xD});
        check(f.kind == PanelMap::Kind::Encoder && f.id == 6 && f.value == -3 && f.control == "data", "the FANTOM's VALUE dial, three steps back: " + std::to_string(f.value));
        // ENTER (switch 62) pressed and released; bits 5 and 6 of byte 0 say who sent it, not the kind.
        f = decode(fantom, {0xA2, 62, 0, 0});
        check(f.kind == PanelMap::Kind::SwitchOn && f.control == "enter" && decode(fantom, {0x81, 62, 0, 0}).kind == PanelMap::Kind::SwitchOff, "the FANTOM's ENTER, on and off");
        // Each control's scale, as seen on a unit: sliders and knobs 0 to 127, the lever and wheels 0 to 1023.
        auto analog = [&](uint32_t id, int value) { return decode(fantom, {0x84, uint8_t(id & 0x7F), uint8_t((value >> 7) << 2 | id >> 7), uint8_t(value & 0x7F)}); };
        check(analog(16, 127).control == "fader:8" && near(analog(16, 127).input.value, 1) && near(analog(17, 64).input.value, 64 / 127.) && analog(17, 64).control == "knob:1",
              "slider 8 (analog 16) and knob 1 on 0 to 127");
        check(near(analog(7, 1023).input.value, 1) && analog(7, 1023).control == "lever:1" && analog(8, 5).control == "lever:2" && near(analog(5, 512).input.value, 512 / 1023.) && analog(5, 512).control == "wheel:1" && analog(6, 5).control == "wheel:2", "WHEEL1, WHEEL2 and the lever's two axes on 0 to 1023");
        // WHEEL2's raw value falls as it is pushed away from the player (the owner, 2026-11-25: read as it came, the top was off):
        // at the top (raw 0) full modulation, at the bottom (raw 1023) none. WHEEL1 and the lever are read as they come.
        check(near(analog(6, 0).input.value, 1) && near(analog(6, 1023).input.value, 0) && near(analog(6, 256).input.value, 767 / 1023.), "WHEEL2 at the top is full modulation, at the bottom none");
        check(near(analog(5, 0).input.value, 0) && near(analog(5, 1023).input.value, 1) && near(analog(7, 0).input.value, 0) && near(analog(8, 1023).input.value, 1) && near(analog(8, 0).input.value, 0), "WHEEL1 and the lever's axes are not inverted");
        check(analog(2, 127).control == "footswitch:1" && near(analog(2, 127).input.value, 1) && near(analog(2, 64).input.value, 64 / 127.), "the HOLD jack (analog 2) is footswitch:1, a position on 0 to 127");
        check(analog(0, 1023).control == "knob:9" && near(analog(0, 1023).input.value, 1) && near(analog(1, 512).input.value, 512 / 1023.), "CUTOFF and RESONANCE on 0 to 1023");
        // The pads as the owner pressed them on November 12: from the bottom left, left to right,
        // row by row upward, switches 101, 103, 102, 100, then 98, 97, 96, 99, 92, 94, 95, 93, 91,
        // 88, 89, 90; the pad map counts from the bottom left too.
        const std::vector<uint32_t> pressed{101, 103, 102, 100, 98, 97, 96, 99, 92, 94, 95, 93, 91, 88, 89, 90};
        bool inOrder = true;
        for(size_t i = 0; i < pressed.size(); ++i) inOrder = inOrder && decode(fantom, {0x82, uint8_t(pressed[i]), 0, 0}).control == "pad:" + std::to_string(i + 1);
        check(inOrder && decode(fantom, {0x81, 90, 0, 0}).input.value == 0, "pad 1 the bottom left, pad 16 the top right");
        check(decode(fantom, {0x82, 23, 0, 0}).control == "sampling" && decode(fantom, {0x82, 22, 0, 0}).control == "padMode", "SAMPLING is switch 23, PAD MODE 22");
        check(decode(fantom, {0x82, 104, 0, 0}).control.empty() && decode(fantom, {0x82, 105, 0, 0}).control.empty() && decode(fantom, {0x82, 106, 0, 0}).control.empty(),
              "WRITE, MENU and TEMPO are not on the panel bus");
        check(decode(fantom, {0x82, 35, 0, 0}).control == "ctrl1Sense" && decode(fantom, {0x82, 38, 0, 0}).control == "ctrl2Ring", "the CTRL jacks' sensing, fixed");
        auto refused = [](const std::string& json) { try { PanelMap::parse(json); } catch(const std::invalid_argument&) { return true; } return false; };
        check(!refused(mapJson(controls)), "the test's map parses");
        check(refused(mapJson(controls + R"(, {"kind": "analog", "id": 3, "control": "fader:9"})")), "two analog controls on one id");
        check(refused(mapJson(R"({"kind": "lever", "id": 1, "control": "wheel:1"})")), "an unknown kind");
        check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "button:1", "max": 3})")), "a max on a switch");
        check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "button:1", "colour": 3})")), "an unknown field");
        {
            // A normally-closed pedal: `invert` reads an analog position or a switch's press the other way round.
            const auto inv = PanelMap::parse(mapJson(R"({"kind": "analog", "id": 3, "control": "footswitch:1", "invert": true}, {"kind": "switch", "id": 5, "control": "footswitch:2", "invert": true},
                {"kind": "analog", "id": 4, "control": "footswitch:3", "invert": false})"));
            check(near(decode(inv, {0x81, 3, 0, 0}).input.value, 1) && near(decode(inv, {0x81, 3, 7, 0x68}).input.value, 0), "an inverted analog: up is down");
            check(decode(inv, {0x82, 5, 0, 0}).input.value == 0 && decode(inv, {0x83, 5, 0, 0}).input.value == 1, "an inverted switch: pressed reads off");
            check(near(decode(inv, {0x81, 4, 0, 0}).input.value, 0), "invert false changes nothing");
        }
        check(refused(mapJson(R"({"kind": "encoder", "id": 1, "control": "knob:1", "invert": true})")), "invert on an encoder");
        check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "button:1", "invert": 1})")), "a non-boolean invert");
        check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "button:1", "invert": true, "navigate": {"action": "confirm"}})")), "invert with navigate");
        check(refused(R"({"format": "oscairn.panel-map", "version": 2})"), "another version");
        auto bytes = mapJson(controls); bytes.replace(bytes.find("\"bytes\": [0]"), 12, "\"bytes\": [4]");
        check(refused(bytes), "a byte past the message");
        auto parts = mapJson(controls); parts.replace(parts.find("\"bytes\": [1], \"mask\": 127"), 26, "\"parts\": [{\"byte\": 1, \"shift\": 2, \"bits\": 7}]");
        check(refused(parts), "a part past its byte's end");
        parts = mapJson(controls); parts.replace(parts.find("\"bytes\": [1]"), 12, "\"bytes\": [1], \"parts\": [{\"byte\": 1, \"bits\": 7}]");
        check(refused(parts), "a field with bytes and parts");
        // Fields from parts of bytes: byte 2 holds an ID's top bits (three, or an analog one's two)
        // and, above them, the value's top five or the step's top four.
        // List navigation, as the map says (its `navigate`): the FANTOM's dial moves the pane last touched by the
        // steps of its message, the first and last knobs under the screen the first and second panes, their pushes
        // confirm there, ENTER confirms, EXIT cancels, and DEC and INC step by one a press (a screen's own previous and next).
        {
            auto nav = [&](std::array<uint8_t, 4> m) { return decode(fantom, m).navigation; };
            const auto dial = nav({0x83, 6, 0xF << 3, 0xD});    // the VALUE dial, a step of -3
            check(dial && dial->action == Navigation::Action::Move && dial->pane == -1 && dial->steps == -1, "the VALUE dial moves the focused list one item, whatever its step (a fast turn's -3)");
            const auto left = nav({0x83, 0, 0, 2}), right = nav({0x83, 5, 0, 1});
            check(left && left->pane == 0 && left->steps == 1 && right && right->pane == 1 && right->steps == 1, "E1 moves pane 0 and E6 pane 1");
            check(decode(fantom, {0x83, 0, 0, 4}).navigation->steps == 1 && decode(fantom, {0x83, 0, 0, 4}).input.value == 4 * fantom.encoderStep, "a step of 4 moves a list one item and a value by its accelerated amount");
            {
                using namespace std::chrono;
                TurnAcceleration turn;
                const auto t = steady_clock::now();
                check(turn.steps(6, 1, t) == 1 && turn.steps(6, 1, t + 20ms) == kTurnFastSteps && turn.steps(6, 1, t + 40ms) == kTurnFastSteps, "a burst of fast detents steps by the accelerated amount after the first");
                check(turn.steps(6, 1, t + 40ms + kTurnFastGap) == 1 && turn.steps(6, 1, t + 50ms + kTurnFastGap) == kTurnFastSteps, "a slower detent steps by one at once");
                check(turn.steps(6, -1, t + 60ms + kTurnFastGap) == -1 && turn.steps(0, 1, t + 61ms + kTurnFastGap) == 1, "a turn the other way, or another knob, starts at one");
            }
            check(!nav({0x83, 2, 0, 1}) && !nav({0x83, 0, 0, 0}), "E3 has none, and an encoder step of 0 is not a move");
            check(nav({0xA2, 40, 0, 0}) && nav({0xA2, 40, 0, 0})->action == Navigation::Action::Confirm && nav({0xA2, 40, 0, 0})->pane == 0 && nav({0xA2, 57, 0, 0})->pane == 1, "E1's push confirms pane 0 and E6's pane 1");
            check(nav({0xA2, 62, 0, 0})->action == Navigation::Action::Confirm && nav({0xA2, 62, 0, 0})->pane == -1 && nav({0xA2, 55, 0, 0})->action == Navigation::Action::Cancel, "ENTER confirms and EXIT cancels");
            check(nav({0xA2, 58, 0, 0})->steps == 1 && nav({0xA2, 59, 0, 0})->steps == -1 && !nav({0x81, 58, 0, 0}) && !nav({0x81, 62, 0, 0}), "INC and DEC by one a press, a release does nothing");
            check(nav({0xA2, 58, 0, 0})->action == Navigation::Action::Step && nav({0xA2, 59, 0, 0})->action == Navigation::Action::Step && nav({0xA2, 46, 0, 0})->action == Navigation::Action::Move, "INC and DEC step, CURSOR UP moves");
            check(!nav({0xA2, 60, 0, 0}) && !nav({0xA2, 17, 0, 0}), "CURSOR LEFT and PLAY have none");
            check(decode(fantom, {0xA2, 18, 0, 0}).control == "record", "the FANTOM's REC button is the record function");
        }
        // Another device: different names and counts, one jog dial and two buttons, four knobs for four panes, an OK and a BACK.
        // Nothing in the app names a control; this map is all a device needs.
        {
            const auto other = PanelMap::parse(mapJson(R"(
                {"kind": "encoder", "id": 1, "control": "jog", "navigate": {"action": "move"}},
                {"kind": "encoder", "id": 10, "control": "dial:1", "navigate": {"action": "move", "pane": 0}},
                {"kind": "encoder", "id": 11, "control": "dial:2", "step": -0.5, "navigate": {"action": "move", "pane": 1}},
                {"kind": "encoder", "id": 12, "control": "dial:3", "navigate": {"action": "move", "pane": 2}},
                {"kind": "encoder", "id": 13, "control": "dial:4", "navigate": {"action": "move", "pane": 3}},
                {"kind": "switch", "id": 20, "control": "up", "navigate": {"action": "move", "steps": -1}},
                {"kind": "switch", "id": 21, "control": "down", "navigate": {"action": "move", "steps": 4}},
                {"kind": "switch", "id": 22, "control": "ok", "navigate": {"action": "confirm"}},
                {"kind": "switch", "id": 23, "control": "back", "navigate": {"action": "cancel"}},
                {"kind": "switch", "id": 24, "control": "pad:1"},
                {"kind": "switch", "id": 25, "control": "record"})"));
            auto nav = [&](std::array<uint8_t, 4> m) { return decode(other, m).navigation; };
            check(nav({0x80, 1, 0x7F, 0}) && nav({0x80, 1, 0x7F, 0})->steps == -1 && nav({0x80, 1, 5, 0})->steps == 1 && nav({0x80, 1, 5, 0})->pane == -1, "a jog dial's turns move the focused list");
            check(nav({0x80, 13, 2, 0})->pane == 3 && nav({0x80, 12, 2, 0})->pane == 2 && nav({0x80, 10, 1, 0})->pane == 0, "four knobs, four panes");
            check(nav({0x80, 11, 3, 0})->steps == -1, "a knob that turns the wrong way has a negative step, in the map");
            check(nav({0x82, 20, 0, 0})->steps == -1 && nav({0x82, 21, 0, 0})->steps == 4, "buttons that move by the steps the map gives");
            check(nav({0x82, 22, 0, 0})->action == Navigation::Action::Confirm && nav({0x82, 23, 0, 0})->action == Navigation::Action::Cancel && !nav({0x82, 24, 0, 0}), "OK confirms, BACK cancels");
            check(decode(other, {0x82, 25, 0, 0}).control == "record", "any button can be the record button: the map names it");
            check(refused(mapJson(R"({"kind": "analog", "id": 1, "control": "slider:1", "navigate": {"action": "move"}})")), "an absolute control cannot navigate");
            check(refused(mapJson(R"({"kind": "encoder", "id": 1, "control": "jog", "navigate": {"action": "confirm"}})")), "an encoder only moves");
            check(refused(mapJson(R"({"kind": "encoder", "id": 1, "control": "jog", "navigate": {"action": "step"}})")), "an encoder does not step");
            check(refused(mapJson(R"({"kind": "encoder", "id": 1, "control": "jog", "navigate": {"action": "move", "steps": 2}})")), "steps are a switch's");
            check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "x", "navigate": {"action": "jump"}})")), "an unknown action");
            check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "x", "navigate": {"action": "move", "steps": 0}})")), "steps of 0");
            check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "x", "navigate": {"action": "move", "pane": -1}})")), "a pane from 0");
        }
        const auto split = PanelMap::parse(R"({"format": "oscairn.panel-map", "version": 1, "protocol": "roland-i2c-slave",
            "layout": {"kind": {"bytes": [0], "mask": 31}, "kinds": {"encoder": 3, "analog": 4, "switchOn": 2, "switchOff": 1},
                       "id": {"parts": [{"byte": 2, "bits": 3}, {"byte": 1, "bits": 7}]}, "analogId": {"parts": [{"byte": 2, "bits": 2}, {"byte": 1, "bits": 7}]},
                       "value": {"parts": [{"byte": 2, "shift": 2, "bits": 5}, {"byte": 3, "bits": 7}]},
                       "delta": {"parts": [{"byte": 2, "shift": 3, "bits": 4}, {"byte": 3, "bits": 4}], "signed": true}},
            "controls": [{"kind": "analog", "id": 300, "control": "fader:1"}, {"kind": "encoder", "id": 900, "control": "knob:1"}]})");
        auto e = decode(split, {0x84, 300 & 0x7F, uint8_t((1000 >> 7) << 2 | 300 >> 7), 1000 & 0x7F});
        check(e.kind == PanelMap::Kind::Analog && e.id == 300 && e.value == 1000 && e.control == "fader:1", "an analog ID of its own, beside the value's bits: id " + std::to_string(e.id));
        e = decode(split, {0x83, 900 & 0x7F, uint8_t(0xF << 3 | 900 >> 7), 0xD}); // -3 is 0xFD
        check(e.kind == PanelMap::Kind::Encoder && e.id == 900 && e.value == -3 && e.control == "knob:1", "a signed step from two parts: " + std::to_string(e.value));
    }

    void framing()
    {
        int fds[2]; check(::pipe(fds) == 0, "a pipe");
        Heard heard;
        {
            PanelReader reader(fds[0], PanelMap::parse(mapJson(controls)),
                               [&](const PanelEvent& e) { std::lock_guard g(heard.lock); heard.events.push_back(e); },
                               [&](const std::string& line) { std::lock_guard g(heard.lock); heard.log.push_back(line); });
            // Garbage, then a message cut short by the next message's first byte.
            put(fds[1], std::vector<uint8_t>{0x01, 0x7F, 0x00, 0x81, 0x03});
            put(fds[1], message(1, 3, 500)); // analog: fader 1 at half
            // A read split in two.
            const auto split = step(4, -2); // encoder: knob 1, two steps down
            put(fds[1], std::vector<uint8_t>{split[0], split[1]});
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
            put(fds[1], std::vector<uint8_t>{split[2], split[3]});
            put(fds[1], message(2, 5, 0)); put(fds[1], message(3, 5, 0)); // a switch pressed and released
            put(fds[1], message(1, 6, 250));                               // an analog control with its own max
            put(fds[1], message(1, 99, 7));                                // an unknown ID
            put(fds[1], message(9, 3, 1));                                 // an unknown kind
            check(heard.wait(7), "seven messages heard");
            auto e = heard.at(0);
            check(e.kind == PanelMap::Kind::Analog && e.id == 3 && e.value == 500 && e.control == "fader:1" && e.input.kind == ControlInput::Kind::Absolute && near(e.input.value, .5),
                  "after garbage and a cut message: fader 1 at 500 of 1000");
            e = heard.at(1);
            check(e.kind == PanelMap::Kind::Encoder && e.value == -2 && e.control == "knob:1" && e.input.kind == ControlInput::Kind::Relative && near(e.input.value, -.02),
                  "a split read: knob 1, two steps down, of .01 each");
            check(heard.at(2).input.kind == ControlInput::Kind::Switch && heard.at(2).input.value == 1 && heard.at(3).input.value == 0 && heard.at(3).control == "button:101",
                  "a switch: on, then off, one entry for both");
            check(near(heard.at(4).input.value, .5), "an entry's own max scales its value");
            e = heard.at(5);
            check(e.control.empty() && e.id == 99 && e.kind == PanelMap::Kind::Analog, "an unknown ID: decoded, played by nothing");
            check(heard.at(6).kind == PanelMap::Kind::Unknown, "an unknown kind");
            {
                std::lock_guard g(heard.lock);
                check(heard.log.size() == 2 && heard.log[0].find("81 63 00 07") != std::string::npos, "unknown messages logged with their bytes: " + (heard.log.empty() ? "" : heard.log[0]));
            }
            const auto stats = reader.stats();
            check(stats.messages == 7 && stats.unknown == 2 && stats.skipped == 5, "seven messages, two unknown, five bytes skipped: " + std::to_string(stats.skipped));

            // A new map: id 99 is fader 1 now.
            reader.setMap(PanelMap::parse(mapJson(R"({"kind": "analog", "id": 99, "control": "fader:1"})")));
            put(fds[1], message(1, 99, 1000)); put(fds[1], message(1, 3, 10));
            check(heard.wait(9), "two more heard");
            check(heard.at(7).control == "fader:1" && near(heard.at(7).input.value, 1) && heard.at(8).control.empty(), "after the map changes, its entries decode");

            // A burst of unknown messages is logged at most ten a second.
            for(int i = 0; i < 40; ++i) put(fds[1], message(2, 120, 0));
            check(heard.wait(49), "the burst heard");
            std::lock_guard g(heard.lock);
            check(heard.log.size() <= 2 + 10 + 10, "the log is rate-limited: " + std::to_string(heard.log.size()));
        }
        ::close(fds[1]); ::close(fds[0]);
    }

    // The dead band: a fader (0 to 1000) with analogDeadband 1. Turning back a step, or the same
    // value again, is chatter; a slow move on is not; the ends always play.
    void deadband()
    {
        int fds[2]; check(::pipe(fds) == 0, "a pipe");
        auto json = mapJson(controls);
        json.replace(json.find("\"encoderStep\": 0.01"), 19, "\"encoderStep\": 0.01, \"analogDeadband\": 1");
        Heard heard;
        const std::vector<int> values{500, 501, 500, 502, 503, 502, 503, 501, 1, 0, 0, 1, 999, 1000};
        const std::vector<bool> chatter{false, true, true, false, false, true, true, false, false, false, true, true, false, false};
        {
            PanelReader reader(fds[0], PanelMap::parse(json), [&](const PanelEvent& e) { std::lock_guard g(heard.lock); heard.events.push_back(e); }, {});
            for(const int v : values) put(fds[1], message(1, 3, v));
            check(heard.wait(values.size()), "every value heard");
            std::string got;
            for(size_t i = 0; i < values.size(); ++i) got += heard.at(i).chatter ? "c" : "-";
            std::string want;
            for(const bool c : chatter) want += c ? "c" : "-";
            check(got == want, "chatter marked " + want + ": " + got);
        }
        ::close(fds[1]); ::close(fds[0]);
    }

    // The panel holder, forked, on a stand-in device: a pipe of messages, and a doorbell the seam's
    // wait reads one byte from, so each ring is one wake of the driver's ioctl. The seam's calls
    // and the holder's log go to a trace pipe.
    struct Bench
    {
        int data[2]{-1, -1}, bell[2]{-1, -1}, trace[2]{-1, -1};
        std::string socket = "/tmp/oscairn-panel-test-" + std::to_string(::getpid()) + ".sock";
        std::string device;
        std::string traced;
        Bench()
        {
            check(::pipe(data) == 0 && ::pipe(bell) == 0 && ::pipe(trace) == 0, "pipes");
            ::fcntl(data[0], F_SETFL, O_NONBLOCK); // a read with nothing queued gives up at once, as the driver's gives 0
            ::fcntl(trace[0], F_SETFL, O_NONBLOCK);
            char path[] = "/tmp/oscairn-panel-device-XXXXXX";
            const int fd = ::mkstemp(path);
            ::close(fd);
            device = path;
        }
        ~Bench() { for(int fd : {data[0], data[1], bell[0], bell[1], trace[0], trace[1]}) ::close(fd); ::unlink(device.c_str()); ::unlink(socket.c_str()); }
        PanelDevice seam(int patience = 2000)
        {
            const int t = trace[1], b = bell[0], d = data[0];
            auto note = [t](const std::string& line) { const auto text = line + "\n"; if(::write(t, text.data(), text.size()) < 0) {} };
            return {[note, d](const std::string&, int flags) { note((flags & O_ACCMODE) == O_RDWR ? "seam: open read-write" : "seam: open other"); return ::dup(d); },
                    [note, b](int) { note("wait"); char c; return ::read(b, &c, 1) == 1 ? 0 : -1; }, patience};
        }
        // Forked before the test starts a thread of its own.
        pid_t start(PanelDevice device)
        {
            const pid_t pid = ::fork();
            if(pid == 0)
            {
                const int t = trace[1];
                ::_exit(holdPanel(this->device, socket, [t](const std::string& line) { const auto text = line + "\n"; if(::write(t, text.data(), text.size()) < 0) {} }, device));
            }
            return pid;
        }
        const std::string& read()
        {
            char b[512]; ssize_t n;
            while((n = ::read(trace[0], b, sizeof b)) > 0) traced.append(b, size_t(n));
            return traced;
        }
        bool saw(const std::string& what, int ms = 3000)
        {
            for(int i = 0; i < ms / 10; ++i) { if(read().find(what) != std::string::npos) return true; std::this_thread::sleep_for(std::chrono::milliseconds(10)); }
            return false;
        }
        size_t count(const std::string& what) { size_t n = 0; for(auto at = read().find(what); at != std::string::npos; at = traced.find(what, at + 1)) ++n; return n; }
        void ring() { check(::write(bell[1], "!", 1) == 1, "the doorbell"); }
    };
    int ended(pid_t pid, int ms) // its exit status, or -1 if it did not end in time
    {
        for(int i = 0; i < ms; ++i)
        {
            int status = 0;
            if(::waitpid(pid, &status, WNOHANG) == pid) return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
        return -1;
    }

    void holder()
    {
        Bench bench;
        const pid_t pid = bench.start(bench.seam());
        check(bench.saw("holding " + bench.device), "the holder holds the device:\n" + bench.read());
        check(bench.read().find("open read-write\n") != std::string::npos, "opened for reading and writing");
        // A second holder on the same socket refuses before it touches the device.
        const pid_t second = bench.start(bench.seam());
        check(ended(second, 3000) == 1 && bench.saw("another panel holder runs") && bench.count("seam: open") == 1, "a second holder refuses, without opening the device:\n" + bench.read());

        Heard heard;
        auto sink = [&](const PanelEvent& e) { std::lock_guard g(heard.lock); heard.events.push_back(e); };
        {
            PanelReader reader(bench.socket, PanelMap::parse(mapJson(controls)), sink, {});
            std::this_thread::sleep_for(std::chrono::milliseconds(200)); // connected; the holder takes it in at the next wake
            // Three messages queued, one wake: all three read before the next wait.
            std::vector<uint8_t> burst;
            for(const auto& m : {message(1, 3, 100), message(1, 3, 200), message(2, 5, 0)}) burst.insert(burst.end(), m.begin(), m.end());
            put(bench.data[1], burst);
            bench.ring();
            check(heard.wait(3), "one wake: the three queued messages read and passed on");
            check(heard.at(0).value == 100 && heard.at(1).value == 200 && heard.at(2).control == "button:101", "in order");
        }
        // The reader gone: its hang-up wakes the wait, and the holder drops it with no message
        // from the device.
        check(bench.saw("a reader detached (0 left)", 1000), "a reader that goes is dropped at once:\n" + bench.read());
        check(bench.count("seam: open") == 1, "the device still open");
        // The engine restarting: its reader goes, a new one attaches, the device stays open.
        {
            PanelReader reader(bench.socket, PanelMap::parse(mapJson(controls)), sink, {});
            std::this_thread::sleep_for(std::chrono::milliseconds(200));
            put(bench.data[1], message(1, 3, 300));
            bench.ring();
            check(heard.wait(4) && heard.at(3).value == 300, "a reader attached after the last one went hears the next message");
        }
        check(bench.count("seam: open") == 1, "the device opened once in all:\n" + bench.read());
        // SIGTERM wakes the wait (here a read the signal interrupts) and the holder lets go.
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
        const auto stopping = std::chrono::steady_clock::now();
        ::kill(pid, SIGTERM);
        check(ended(pid, 2000) == 0, "SIGTERM ends a waiting holder, cleanly");
        check(std::chrono::steady_clock::now() - stopping < std::chrono::milliseconds(500) && bench.saw("released " + bench.device), "promptly, releasing the device");

        // A wait that fails (not Roland's driver): the holder ends, opening nothing else.
        Bench failing;
        auto broken = failing.seam();
        broken.wait = [](int) { errno = ENOTTY; return -1; };
        check(ended(failing.start(broken), 3000) == 1 && failing.saw("waiting on") && failing.count("seam: open") == 1, "a failed wait ends the holder; no other way of reading:\n" + failing.read());
    }

    // The takeover guard: while another process has the device open the holder does not open it.
    // Linux: it reads /proc.
    void guard()
    {
#ifdef __linux__
        Bench bench;
        int ready[2]; check(::pipe(ready) == 0, "a pipe");
        const pid_t other = ::fork();
        if(other == 0) { const int fd = ::open(bench.device.c_str(), O_RDONLY); if(::write(ready[1], "!", 1) < 0) {} ::pause(); ::_exit(fd < 0); }
        char c; check(::read(ready[0], &c, 1) == 1, "the other process has the device open");
        const auto holders = processesHolding(bench.device);
        check(holders.size() == 1 && holders[0] == other, "the process holding it found");
        const pid_t pid = bench.start(bench.seam(300));
        check(ended(pid, 3000) == 1 && bench.saw("holds " + bench.device) && bench.count("seam: open") == 0, "the holder refuses, without opening it:\n" + bench.read());
        ::kill(other, SIGKILL); ::waitpid(other, nullptr, 0);
        ::close(ready[0]); ::close(ready[1]);
#endif
    }

    // The panel's controls played in a Session the way a MIDI controller's are: two faders on the
    // parts' levels, a knob (an encoder) on A's gain, the zone buttons on the parts' mutes.
    void session(const char* fixture)
    {
        constexpr uint32_t frames = 64;
        Session s(48000, frames);
        auto render = [&] {
            std::array<float, frames> l{}, r{};
            float* out[2]{l.data(), r.data()};
            check(s.process(nullptr, 0, out, 2, frames), "render");
            return l[frames - 1];
        };
        const auto a = s.add("a", fixture, "org.kick.test.synth"), b = s.add("b", fixture, "org.kick.test.synth");
        s.setParameter(a, 7, .5); s.setParameter(b, 7, .25);
        const auto pa = s.addPart("A"), pb = s.addPart("B");
        s.insertIntoPart(pa, 0, a); s.insertIntoPart(pb, 0, b); s.commit();
        s.assign("knob:1", {Assignment::Kind::Parameter, a, 7, 0});
        // A device's own groups, as its profile gives them: no MIDI.
        using Role = ControlGroup::Role;
        auto group = [](const char* kind, uint32_t count) { ControlGroup g; g.role = Role::Assign; g.kind = kind; g.count = count; return g; };
        auto knobs = group("knobs", 1); knobs.paged = true;
        s.setControllers({group("faders", 2), knobs, group("buttons", 2)}, {});
        s.service();
        s.note(true, 60, 1); render(); render();
        check(near(render(), .75), "both parts sound: A .5, B .25");

        int fds[2]; check(::pipe(fds) == 0, "a pipe");
        Heard heard;
        {
            PanelReader reader(fds[0], PanelMap::parse(mapJson(controls)), [&](const PanelEvent& e) {
                if(!e.control.empty()) s.controllerControl(e.control, e.input); // the engine's sink
                std::lock_guard g(heard.lock); heard.events.push_back(e);
            }, {});
            // Fader 1 (A's level) picked up at 1, then down to .4.
            put(fds[1], message(1, 3, 1000)); put(fds[1], message(1, 3, 400));
            check(heard.wait(2), "fader moves heard");
            check(near(render(), .5 * .4 + .25), "fader 1 moves A's level within one block: " + std::to_string(render()));
            s.service();
            check(near(s.parts()[0].level, .4), "and the control thread brings the model up to date");
            check(s.controllerState().touched == "fader:1", "the control moved last, for learn");
            // Knob 1, an encoder, on A's gain: ten steps of .01 up.
            put(fds[1], step(4, 10));
            check(heard.wait(3), "the encoder heard");
            s.service();
            const auto info = s.parameterInfo(a, 7);
            check(near((s.parameter(a, 7) - info.minimum) / (info.maximum - info.minimum), .6), "an encoder steps its parameter from where it is: " + std::to_string(s.parameter(a, 7)));
            // Zone button 1 (button:101): a press mutes A, its release does nothing, the next press unmutes it.
            const double level = render();
            put(fds[1], message(2, 5, 0)); put(fds[1], message(3, 5, 0));
            check(heard.wait(5), "the button heard");
            check(near(render(), .25), "a press mutes A within one block: " + std::to_string(render()));
            s.service();
            put(fds[1], message(2, 5, 0));
            check(heard.wait(6), "the next press heard");
            check(near(render(), level), "the next press unmutes it");
            // A fixed control's name plays nothing through the controller layer.
            check(!s.controllerControl("enter", {ControlInput::Kind::Switch, 1}), "a fixed control's name plays nothing");
        }
        ::close(fds[1]); ::close(fds[0]);
    }

    // The HOLD jack as the unit sends it (analog 2, a ramp of 0 to 127 steps when pressed): the shipped FANTOM-07 map
    // through a reader into a Session whose profile group makes it sustain. Its position reaches the parts as CC 64
    // with the value (0 to 127, so a half-damper pedal passes as one): a part that receives sustain holds the released
    // key while it is down; a part set to none does not.
    void holdJack(const char* fixture, const char* shipped)
    {
        char home[] = "/tmp/oscairn-hold-jack-XXXXXX"; // the session's revisions there, never the real data folder
        if(!::mkdtemp(home)) { check(false, "a data folder for the test"); return; }
        const char* kept = std::getenv("OSCAIRN_DATA_HOME"); const std::string was = kept ? kept : "";
        ::setenv("OSCAIRN_DATA_HOME", home, 1);
        constexpr uint32_t frames = 64;
        Session s(48000, frames);
        auto render = [&] {
            std::array<float, frames> l{}, r{};
            float* out[2]{l.data(), r.data()};
            check(s.process(nullptr, 0, out, 2, frames), "render");
            return l[frames - 1];
        };
        const auto a = s.add("a", fixture, "org.kick.test.midi"), b = s.add("b", fixture, "org.kick.test.midi");
        s.setParameter(a, 7, .5); s.setParameter(b, 7, .25);
        const auto pa = s.addPart("A"), pb = s.addPart("B");
        s.insertIntoPart(pa, 0, a); s.insertIntoPart(pb, 0, b); s.commit();
        auto input = s.parts()[1].input; input.pedals = withPedalReceive(0, Pedal::Sustain, PedalReceive::None); s.setPartInput(pb, input); s.commit();
        ControlGroup hold; hold.role = ControlGroup::Role::Assign; hold.kind = "footswitches"; hold.count = 1; hold.functions = {"sustain"};
        s.setControllers({hold}, {}); s.service();
        std::ifstream in(shipped); std::stringstream text; text << in.rdbuf();
        int fds[2]; check(::pipe(fds) == 0, "a pipe");
        Heard heard;
        {
            PanelReader reader(fds[0], PanelMap::parse(text.str()), [&](const PanelEvent& e) {
                if(!e.control.empty()) s.controllerControl(e.control, e.input);
                std::lock_guard g(heard.lock); heard.events.push_back(e);
            }, {});
            auto jack = [&](int value) { put(fds[1], std::array<uint8_t, 4>{0x84, 2, uint8_t((value >> 7) << 2), uint8_t(value & 0x7F)}); };
            size_t sent = 0;
            auto ramp = [&](const std::vector<int>& values) { for(const int v : values) { jack(v); ++sent; } check(heard.wait(sent), "the ramp heard"); s.service(); };
            ramp({4, 8, 9, 12, 62, 79, 99, 113, 120, 123, 125, 126, 127}); // pressed, as dumped on the unit
            s.note(true, 60, 1); render(); s.note(false, 60, 0); render(); render();
            check(near(render(), .5), "the jack pressed holds the key on the part that takes sustain, not the one set to none: " + std::to_string(render()));
            ramp({41, 21, 0});
            render();
            check(near(render(), 0), "the jack up lets it go: " + std::to_string(render()));
        }
        ::close(fds[1]); ::close(fds[0]);
        ::setenv("OSCAIRN_DATA_HOME", was.c_str(), 1);
        std::error_code e; std::filesystem::remove_all(home, e);
    }

    // The pitch and mod wheels and the lever as the unit sends them (analog 5 and 6: WHEEL1, WHEEL2; 7 and 8: the lever's side to side and push;
    // 0 to 1023, 512 at rest): the shipped FANTOM-07 map through a reader into a Session whose wheel and lever groups are the profile's
    // (WHEEL1 pitchBend sprung to the middle, WHEEL2 modulation unsprung, the lever's axes following them, sprung to the middle and to 0).
    // The MIDI fixture (KICK_FIXTURE_WHEELS) plays the bend it hears on its left channel and CC 1 on its right, so each part is heard alone
    // (the others at level 0). The ends are 0 and 16383, the rest exactly 8192, and the spring returns there. Two inputs on one control: a
    // sprung one takes over only when it reaches or crosses the value, an unsprung one at once, and a released one returns the control to its
    // rest. A part with a Sound's own assignment of the control is driven by either input and its instruments get no CC 1; a part without
    // one gets it; one whose assignment passes gets both. A part's receive setting and a raw CC 1 follow the same filter. A wheel the
    // Performance assigned plays that and does not also bend. A Performance can unlink the lever, which is then assignable on its own.
    // Moves are counted by the input's own identity. Settings are undone, saved and read back, and kept on a device without the controls.
    void lever(const char* fixture, const char* shipped)
    {
        char home[] = "/tmp/oscairn-lever-XXXXXX";
        if(!::mkdtemp(home)) { check(false, "a data folder for the test"); return; }
        const char* kept = std::getenv("OSCAIRN_DATA_HOME"); const std::string was = kept ? kept : "";
        ::setenv("OSCAIRN_DATA_HOME", home, 1);
        ::setenv("KICK_FIXTURE_WHEELS", "1", 1);
        constexpr uint32_t frames = 64;
        Session s(48000, frames);
        std::array<float, 2> heard2{};
        auto render = [&] {
            std::array<float, frames> l{}, r{};
            float* out[2]{l.data(), r.data()};
            check(s.process(nullptr, 0, out, 2, frames), "render");
            heard2 = {l[frames - 1], r[frames - 1]};
        };
        const auto a = s.add("a", fixture, "org.kick.test.midi"), b = s.add("b", fixture, "org.kick.test.midi"), c = s.add("c", fixture, "org.kick.test.midi");
        const auto pa = s.addPart("A"), pb = s.addPart("B"), pc = s.addPart("C"), pd = s.addPart("D");
        s.insertIntoPart(pa, 0, a); s.insertIntoPart(pb, 0, b); s.insertIntoPart(pc, 0, c); s.commit();
        std::array<uint32_t, 3> parts{pa, pb, pc};
        auto solo = [&](size_t which) { for(size_t i = 0; i < 3; ++i) s.setPartLevel(parts[i], i == which ? 1 : 0, false); s.service(); render(); render(); };
        auto wheels = [](std::vector<std::string> functions, std::vector<double> rest) { ControlGroup g; g.role = ControlGroup::Role::Assign; g.kind = "wheels"; g.count = 1; g.functions = std::move(functions); g.rest = std::move(rest); return g; };
        ControlGroup lever; lever.role = ControlGroup::Role::Assign; lever.kind = "lever"; lever.count = 2; lever.follows = {"wheel:1", "wheel:2"}; lever.rest = {.5, 0};
        s.setControllers({wheels({"pitchBend"}, {.5}), wheels({"modulation"}, {}), lever}, {}); s.service();
        std::ifstream in(shipped); std::stringstream text; text << in.rdbuf();
        int fds[2]; check(::pipe(fds) == 0, "a pipe");
        Heard heard;
        {
            PanelReader reader(fds[0], PanelMap::parse(text.str()), [&](const PanelEvent& e) {
                if(!e.control.empty()) s.controllerControl(e.control, e.input);
                std::lock_guard g(heard.lock); heard.events.push_back(e);
            }, {});
            size_t sent = 0;
            auto move = [&](int id, int value, size_t who = 0) {
                put(fds[1], std::array<uint8_t, 4>{0x84, uint8_t(id), uint8_t((value >> 7) << 2), uint8_t(value & 0x7F)});
                check(heard.wait(++sent), "the move heard"); s.service(); solo(who);
            };
            const float center = 8192 / 16383.f;
            auto cc = [&] { return int(std::lround(heard2[1] * 127)); };
            auto bend = [&] { return int(std::lround(heard2[0] * 16383)); };
            solo(0);
            check(near(heard2[0], center), "no bend heard before anything moves");
            // Pitch: the wheel, then the lever.
            for(const int id : {5, 7})
            {
                const std::string name = id == 5 ? "WHEEL1" : "the lever";
                move(id, 512);
                check(bend() == 8192, name + " at rest (512 of 1023) is a bend of exactly 8192: " + std::to_string(bend()));
                move(id, 0);
                check(bend() == 0, name + " left is a bend of 0: " + std::to_string(bend()));
                move(id, 512);
                check(bend() == 8192, name + " springs back to exactly 8192: " + std::to_string(bend()));
                move(id, 1023);
                check(bend() == 16383, name + " right is a bend of 16383: " + std::to_string(bend()));
                move(id, 509);
                check(bend() == 8192, name + " rests a few steps off the middle at 8192: " + std::to_string(bend()));
                move(id, 512);
                move(id, 800);
                check(bend() > 8192 && bend() < 16383, name + " between rest and the end bends partway");
                move(id, 512);
                check(bend() == 8192, "and back");
            }
            // Two sprung inputs on pitch: the wheel is left bent, the lever does not drag it back until it passes it, and released it returns to the middle.
            move(5, 800); const int held = bend();
            check(held > 8192 && held < 16383, "the wheel left bent");
            move(7, 600);
            check(bend() == held, "the lever moved short of the wheel's bend changes nothing: " + std::to_string(bend()));
            move(7, 1023);
            check(bend() == 16383, "past it, the lever takes over");
            move(7, 512);
            check(bend() == 8192, "released, it returns the bend to the middle, though the wheel is where it was left");
            move(5, 790);
            check(bend() > 8192 && bend() < 16383, "and the wheel moved takes over again, at the middle");
            move(5, 512);
            check(bend() == 8192, "and the wheel springs back");
            // Modulation: the owner's example, step by step.
            // WHEEL2 is inverted in the map: its raw value is 1023 less the position (511 is halfway, 0 the top).
            check(cc() == 0, "no mod wheel yet");
            move(6, 511);
            check(cc() == 64, "WHEEL2 halfway sets CC 1 halfway: " + std::to_string(cc()));
            move(8, 307);
            check(cc() == 64, "the lever pushed to 0.3 changes nothing: " + std::to_string(cc()));
            move(8, 512);
            move(8, 800);
            check(std::abs(cc() - 99) <= 1, "pushed past the wheel's value it takes over and follows: " + std::to_string(cc()));
            move(8, 0);
            check(cc() == 0, "released to 0, the value is 0 though the wheel is still at 0.5: " + std::to_string(cc()));
            move(6, 493);
            check(cc() == 66, "the wheel nudged takes over at once: " + std::to_string(cc()));
            move(6, 0);
            check(cc() == 127, "WHEEL2 at the top (raw 0) is CC 1 at 127: " + std::to_string(cc()));
            move(6, 1023);
            check(cc() == 0, "WHEEL2 at the bottom (raw 1023) is CC 1 at 0: " + std::to_string(cc()));
            move(8, 1023);
            check(cc() == 127 && s.controllerState().touched == "lever:2", "the lever alone reaches CC 1 at 127, and controllers.get names it by its own id: " + s.controllerState().touched);
            move(8, 0);
            check(cc() == 0, "and back to 0");
            check(bend() == 8192, "while pitch is untouched");
            const auto touches = s.controllerState().touches;
            check(touches >= 20, "moves are counted: " + std::to_string(touches));

            // A Sound's own assignment of the mod wheel replaces CC 1 for its part unless it passes, whichever input drives it.
            s.setParameter(a, 7, 0); s.setParameter(c, 7, 0);
            Assignment ga; ga.kind = Assignment::Kind::Parameter; ga.node = a; ga.param = 7;
            Assignment gc = ga; gc.node = c;
            s.assign("wheel:2", ga, pa); s.assign("wheel:2", gc, pc, true); s.commit(); s.service();
            move(8, 5); // picked up: near the values
            move(8, 1023);
            check(s.parameter(a, 7) > .9 && s.parameter(c, 7) > .9, "a part's own assignment of the mod wheel is driven by the lever too");
            solo(0); check(cc() == 0, "and that part's instruments get no CC 1: " + std::to_string(cc()));
            solo(1); check(cc() == 127, "a part without one still gets it");
            solo(2); check(cc() == 127, "a part whose assignment passes gets both");
            solo(0); check(bend() == 8192, "pitch bend is not touched by it");
            move(8, 5);
            check(s.assignments().size() == 2 && s.assignments()[1].passes && !s.assignments()[0].passes, "the flag is reported with the assignments");

            // A part's receive setting, and the same filter for a raw CC 1 and a bend through notes.controller.
            auto input = s.parts()[1].input; input.functions = withFunctionReceive(0, Function::Modulation, PedalReceive::None); s.setPartInput(pb, input); s.commit(); s.service();
            s.controller(1, 100); solo(1);
            check(cc() < 50, "a part that does not receive the mod wheel gets no CC 1, whatever sent it: " + std::to_string(cc()));
            input.functions = withFunctionReceive(0, Function::PitchBend, PedalReceive::None); s.setPartInput(pb, input); s.commit(); s.service();
            s.controller(1, 100); solo(1);
            check(cc() == 100, "and a part that does not receive pitch bend still gets CC 1");
            s.pitchBend(0); solo(1);
            check(bend() == 8192, "but not the bend");
            solo(2); check(bend() == 0, "while a part that receives it gets it");
            input.functions = 0; s.setPartInput(pb, input); s.commit();
            s.pitchBend(8192);

            // A wheel the Performance assigned plays that and does not also bend; the lever that follows it plays that too.
            Assignment level; level.kind = Assignment::Kind::PartLevel; level.id = pd;
            s.setPartLevel(pd, .5, false);
            s.assign("wheel:1", level); s.commit(); s.service();
            move(5, 515); // picked up
            move(5, 0);
            check(bend() == 8192 && s.controllerState().touched == "wheel:1" && s.parts()[3].level < .1, "a wheel assigned to something else plays that and does not also bend");
            s.assign("wheel:1", {}); s.commit();
            // The owner's case: the mod wheel set to a part's volume plays that and the engines get no CC 1, unless set to do both (passes).
            s.setPartLevel(pd, .5, false);
            s.assign("wheel:2", level); s.commit(); s.service();
            move(6, 511); move(6, 0); solo(1);
            check(s.parts()[3].level > .9 && cc() == 100, "the mod wheel set to a part's volume moves it and sends no CC 1: " + std::to_string(cc()));
            s.assign("wheel:2", level, 0, true); s.commit(); s.service();
            move(6, 1023); solo(1);
            check(s.parts()[3].level < .1 && cc() == 0, "set to do both, it moves the volume and every part gets CC 1 too: " + std::to_string(cc()));
            check(s.assignments().back().passes && s.assignments().back().part == 0, "the Performance's own assignment reports passes");
            s.assign("wheel:2", {}); s.commit();

            // Unlinked, the lever's push is a control of its own: it no longer drives the mod wheel's, and can be assigned.
            s.setParameter(a, 7, 0);
            s.link("lever:2", false); s.commit(); s.service();
            check(s.unlinked() == std::vector<std::string>{"lever:2"}, "unlinked is reported");
            move(8, 5); move(8, 1023);
            check(s.parameter(a, 7) < .1 && cc() == 0 && s.controllerState().touched == "lever:2", "an unlinked lever axis does not drive the mod wheel's control or its CC 1: " + std::to_string(s.parameter(a, 7)));
            move(8, 0);
            Assignment own = ga; own.param = 7;
            s.assign("lever:2", own); s.commit(); s.service();
            move(8, 5); move(8, 1023);
            check(s.parameter(a, 7) > .9 && cc() == 0, "and plays what it is assigned on its own, the mod wheel unmoved: " + std::to_string(s.parameter(a, 7)) + " " + std::to_string(cc()));
            move(8, 0);
            s.assign("lever:2", {}); s.commit();
            s.link("lever:2", true); s.commit();
            check(s.unlinked().empty(), "linked again");
            check(s.undo() && s.unlinked() == std::vector<std::string>{"lever:2"}, "undone: unlinked again");
            check(s.redo() && s.unlinked().empty(), "and redone");
            check(s.undo() && s.unlinked().size() == 1, "undone once more for the saving");
            s.assign("lever:2", own); s.commit();
            const auto saved = std::filesystem::path(home) / "saved";
            s.savePerformance(saved.string());
            s.link("lever:2", true); s.commit();
            s.loadPerformance(saved.string()); s.service();
            check(s.unlinked() == std::vector<std::string>{"lever:2"}, "the unlink is saved with the Performance and read back");
            {
                Session t(48000, frames); // a device with no lever
                t.loadPerformance(saved.string()); t.service();
                bool kept = false; for(const auto& o : t.assignments()) kept = kept || o.control == "lever:2";
                check(t.unlinked() == std::vector<std::string>{"lever:2"} && kept, "opened on a device without the lever the unlink and its assignment are kept");
            }
        }
        ::close(fds[1]); ::close(fds[0]);
        ::unsetenv("KICK_FIXTURE_WHEELS");
        ::setenv("OSCAIRN_DATA_HOME", was.c_str(), 1);
        std::error_code e; std::filesystem::remove_all(home, e);
    }

    // oscairn-panel-dump attached to a holder: each message on a line, with the control or "-",
    // then the table. Without a holder it refuses.
    void dump(const char* tool)
    {
        Bench bench;
        char mapFile[] = "/tmp/oscairn-panel-map-XXXXXX";
        const int mapFd = ::mkstemp(mapFile);
        const auto json = mapJson(controls);
        check(::write(mapFd, json.data(), json.size()) == ssize_t(json.size()), "the map written");
        ::close(mapFd);
        const auto command = std::string("'") + tool + "' " + mapFile + " --socket " + bench.socket + " --seconds 1 2>&1";
        check(std::system(command.c_str()) != 0, "the dump refuses without a holder");
        const pid_t pid = bench.start(bench.seam());
        check(bench.saw("holding"), "a holder");
        FILE* out = ::popen(command.c_str(), "r");
        std::this_thread::sleep_for(std::chrono::milliseconds(300));
        put(bench.data[1], message(1, 3, 250)); put(bench.data[1], message(2, 77, 0));
        bench.ring();
        std::string text; char line[256];
        while(out && std::fgets(line, sizeof line, out)) text += line;
        check(out && ::pclose(out) == 0, "the dump ran");
        check(text.find("81 03 01 7a  analog    id 3     value 250    fader:1") != std::string::npos, "a decoded message's line:\n" + text);
        check(text.find("82 4d 00 00  switchOn  id 77    value 0      -") != std::string::npos, "an unknown one's line");
        check(text.find("analog     3      1       250     250     fader:1") != std::string::npos, "the table");
        check(bench.saw("a reader detached (0 left)", 1000) && bench.count("a reader attached") == 1, "the dump is one reader on the holder, dropped as it ends:\n" + bench.read());
        ::kill(pid, SIGTERM);
        check(ended(pid, 2000) == 0, "the holder stopped");
        ::unlink(mapFile);
    }
}

namespace
{
    // A holder that turns every reader away (as one with eight readers does, and as the unit's older
    // holder did once eight that had gone filled it): accepts and closes at once. The reader tries
    // again at most once a second and says so once, not a line each time; once the holder keeps
    // it, it says it attached and hears the panel.
    void turnedAway()
    {
        const std::string path = "/tmp/oscairn-panel-away-" + std::to_string(::getpid()) + ".sock";
        ::unlink(path.c_str());
        sockaddr_un a{};
        a.sun_family = AF_UNIX;
        std::memcpy(a.sun_path, path.c_str(), path.size() + 1);
        const int listener = ::socket(AF_UNIX, SOCK_STREAM, 0);
        check(::bind(listener, reinterpret_cast<const sockaddr*>(&a), sizeof a) == 0 && ::listen(listener, 8) == 0, "the stand-in holder listens");
        std::atomic<int> accepted{0};
        std::atomic<bool> keep{false}, done{false};
        int kept = -1;
        std::thread away([&] {
            while(!done)
            {
                pollfd p{listener, POLLIN, 0};
                if(::poll(&p, 1, 50) != 1) continue;
                const int c = ::accept(listener, nullptr, nullptr);
                if(c < 0) continue;
                ++accepted;
                if(!keep) { ::close(c); continue; }
                kept = c;
                const auto m = message(1, 3, 123);
                check(::write(c, m.data(), m.size()) == 4, "the stand-in holder sends");
                done = true;
            }
        });
        Heard heard;
        {
            PanelReader reader(path, PanelMap::parse(mapJson(controls)), [&](const PanelEvent& e) { std::lock_guard g(heard.lock); heard.events.push_back(e); },
                               [&](const std::string& line) { std::lock_guard g(heard.lock); heard.log.push_back(line); });
            std::this_thread::sleep_for(std::chrono::milliseconds(3500));
            {
                std::lock_guard g(heard.lock);
                std::string all;
                for(const auto& l : heard.log) all += l + "\n";
                check(accepted >= 3 && accepted <= 5, "turned away, the reader tries again once a second: " + std::to_string(accepted.load()) + " tries in 3.5 s");
                check(heard.log.size() == 2 && heard.log[1].find("at once") != std::string::npos, "and says so once:\n" + all);
            }
            keep = true;
            check(heard.wait(1) && heard.at(0).value == 123, "kept, it hears the panel");
            std::lock_guard g(heard.lock);
            check(heard.log.size() == 3 && heard.log[2].starts_with("attached"), "and says it attached, once");
        }
        done = true;
        away.join();
        if(kept >= 0) ::close(kept);
        ::close(listener);
        ::unlink(path.c_str());
    }
}

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: oscairn-panel-reader-test FIXTURE MAP PANEL_DUMP\n"); return 2; }
    holder(); // forks: first, before any thread
    guard();
    dump(argv[3]);
    maps(argv[2]);
    framing();
    deadband();
    turnedAway();
    session(argv[1]);
    holdJack(argv[1], argv[2]);
    lever(argv[1], argv[2]);
    if(failures) { std::fprintf(stderr, "%d failure(s)\n", failures); return 1; }
    std::puts("panel reader: ok");
    return 0;
}
