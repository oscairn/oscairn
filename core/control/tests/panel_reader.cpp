// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A non-MIDI device panel (panel_reader.h): 4-byte messages through a pipe, framed on bit 7 of
// the first byte after garbage, a cut-short message and a read split in two; each kind decoded
// through the map's layout (fields of whole bytes, or of parts of them), an unknown ID reported
// with its bytes, and a new map taking effect;
// then the controls they name played in a Session as a MIDI controller's are (controller_input.h),
// heard at the next block; and oscairn-panel-dump reading a pty. Also: the FANTOM-07's map
// decodes messages in Roland's layout, and a broken map is refused.
#include "oscairn/panel_reader.h"
#include "oscairn/session.h"

#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <fcntl.h>
#include <fstream>
#include <mutex>
#include <sstream>
#include <string>
#include <termios.h>
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
        check(fantom.controls.size() == 112 && fantom.estimated.empty(), "the FANTOM-07's map parses, every entry with its id");
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
        auto refused = [](const std::string& json) { try { PanelMap::parse(json); } catch(const std::invalid_argument&) { return true; } return false; };
        check(!refused(mapJson(controls)), "the test's map parses");
        check(refused(mapJson(controls + R"(, {"kind": "analog", "id": 3, "control": "fader:9"})")), "two analog controls on one id");
        check(refused(mapJson(R"({"kind": "lever", "id": 1, "control": "wheel:1"})")), "an unknown kind");
        check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "button:1", "max": 3})")), "a max on a switch");
        check(refused(mapJson(R"({"kind": "switch", "id": 1, "control": "button:1", "colour": 3})")), "an unknown field");
        check(refused(R"({"format": "oscairn.panel-map", "version": 2})"), "another version");
        auto bytes = mapJson(controls); bytes.replace(bytes.find("\"bytes\": [0]"), 12, "\"bytes\": [4]");
        check(refused(bytes), "a byte past the message");
        auto parts = mapJson(controls); parts.replace(parts.find("\"bytes\": [1], \"mask\": 127"), 26, "\"parts\": [{\"byte\": 1, \"shift\": 2, \"bits\": 7}]");
        check(refused(parts), "a part past its byte's end");
        parts = mapJson(controls); parts.replace(parts.find("\"bytes\": [1]"), 12, "\"bytes\": [1], \"parts\": [{\"byte\": 1, \"bits\": 7}]");
        check(refused(parts), "a field with bytes and parts");
        // Fields from parts of bytes: byte 2 holds an ID's top bits (three, or an analog one's two)
        // and, above them, the value's top five or the step's top four.
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

    // oscairn-panel-dump on a pty: each message on a line, with the control or "-", then the table.
    void dump(const char* tool)
    {
        const int master = ::posix_openpt(O_RDWR | O_NOCTTY);
        check(master >= 0 && ::grantpt(master) == 0 && ::unlockpt(master) == 0, "a pty");
        const std::string path = ::ptsname(master);
        const int slave = ::open(path.c_str(), O_RDWR | O_NOCTTY);
        termios raw{}; ::tcgetattr(slave, &raw); ::cfmakeraw(&raw); ::tcsetattr(slave, TCSANOW, &raw); // bytes as they are
        char mapFile[] = "/tmp/oscairn-panel-map-XXXXXX";
        const int mapFd = ::mkstemp(mapFile);
        const auto json = mapJson(controls);
        check(::write(mapFd, json.data(), json.size()) == ssize_t(json.size()), "the map written");
        ::close(mapFd);
        put(master, std::vector<uint8_t>{0x05});
        put(master, message(1, 3, 250)); put(master, message(2, 77, 0));
        FILE* out = ::popen((std::string("'") + tool + "' " + path + " " + mapFile + " --seconds 1").c_str(), "r");
        std::string text; char line[256];
        while(out && std::fgets(line, sizeof line, out)) text += line;
        check(out && ::pclose(out) == 0, "the dump ran");
        check(text.find("81 03 01 7a  analog    id 3     value 250    fader:1") != std::string::npos, "a decoded message's line:\n" + text);
        check(text.find("82 4d 00 00  switchOn  id 77    value 0      -") != std::string::npos, "an unknown one's line");
        check(text.find("analog     3      1       250     250     fader:1") != std::string::npos, "the table");
        ::unlink(mapFile); ::close(slave); ::close(master);
    }
}

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: oscairn-panel-reader-test FIXTURE MAP PANEL_DUMP\n"); return 2; }
    maps(argv[2]);
    framing();
    session(argv[1]);
    dump(argv[3]);
    if(failures) { std::fprintf(stderr, "%d failure(s)\n", failures); return 1; }
    std::puts("panel reader: ok");
    return 0;
}
