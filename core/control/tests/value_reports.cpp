// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Values a plugin changes with no output event reach params.changed (Session::service) without the control thread
// comparing every parameter every tick: a quiet second of a 5,000-parameter plugin (4,000 in a worker) compares none;
// a values rescan, a sound selected on a shared instance and a plugin's own output event are each published, as the plugin's.
//   oscairn-value-reports-test CONTROL_FIXTURE.clap BASIC_SYNTH.clap   (in a worker too: OSCAIRN_PLUGIN_WORKER)
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"
#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <stdexcept>
#include <string>
#include <thread>

using namespace oscairn;

namespace
{
    void check(bool ok, const std::string& message) { if(!ok) throw std::runtime_error(message); }
    using Kind = Control::ChangeKind;

    struct Player
    {
        Session& s;
        std::array<float, 128> l{}, r{};
        // A tick of the control thread's, every 5 ms as the engine's.
        void tick() { float* out[2]{l.data(), r.data()}; s.service(); s.process(nullptr, 0, out, 2, 128); std::this_thread::sleep_for(std::chrono::milliseconds(5)); }
        // Ticks until `until` holds among the changes since `from`, at most two seconds of them.
        bool waitFor(uint64_t from, const std::function<bool(const Control::Change&)>& until)
        {
            for(int i = 0; i < 400; ++i)
            {
                tick();
                for(const auto& c : s.changesAfter(from).events) if(until(c)) return true;
            }
            return false;
        }
    };
}

int main(int argc, char** argv)
{
    if(argc != 3) { std::fprintf(stderr, "usage: %s CONTROL_FIXTURE.clap BASIC_SYNTH.clap\n", argv[0]); return 2; }
    state_files::TemporaryDirectory home;
    ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
    try
    {
        // A quiet second: values compared at a node's first tick only, not 5,000 a tick (a million a second before).
        {
            const size_t count = std::getenv("OSCAIRN_PLUGIN_WORKER") ? 4000 : 5000; // a worker's most is 4,096 (OSCAIRN_WORKER_MAX_PARAMETERS)
            ::setenv("OSCAIRN_CONTROL_FIXTURE_PARAMETERS", std::to_string(count).c_str(), 1);
            Session s(48000, 128);
            const auto a = s.add("a", argv[1]);
            s.connectNotes(graphInput, a); s.connectAudio(a, graphOutput); s.commit();
            check(s.parameters(a).size() == count, "the fixture's parameters");
            Player p{s};
            for(int i = 0; i < 10; ++i) p.tick();
            const auto compared = s.parametersCompared();
            for(int i = 0; i < 200; ++i) p.tick(); // a second of ticks, every 5 ms
            std::printf("a quiet second of %zu parameters: %llu values compared (every tick: %zu)\n", count, (unsigned long long)(s.parametersCompared() - compared), 200 * count);
            check(s.parametersCompared() == compared, "a quiet second compares no values");

            // A values rescan (the plugin's own preset browser, say): parameter 2, published as the plugin's.
            const auto before = s.changeSequence();
            s.note(true, 127, 1); p.tick(); s.note(false, 127, 0);
            check(p.waitFor(before, [&](const Control::Change& c) { return c.kind == Kind::Value && c.node == a && c.param == 2 && c.value == .25 && c.plugin; }),
                  "a value read again after a rescan is published");
            check(s.parameter(a, 2) == .25, "and read");

            // A set of the Session's: published once, as the control's; the plugin's echo of it (the fixture's output event) as the plugin's.
            const auto set = s.changeSequence();
            check(s.setParameter(a, 1, .3), "a set");
            check(p.waitFor(set, [&](const Control::Change& c) { return c.kind == Kind::Value && c.node == a && c.param == 1 && c.value == .3 && c.plugin; }), "the plugin's own event is published");
            size_t controls = 0;
            for(const auto& c : s.changesAfter(set).events) if(c.kind == Kind::Value && !c.plugin) ++controls;
            check(controls == 1, "the set published once as the control's");
        }
        ::unsetenv("OSCAIRN_CONTROL_FIXTURE_PARAMETERS");
        // A sound selected on a part sharing an instance shows that sound's values: published as the plugin's.
        {
            Session s(48000, 128);
            const auto a = s.add("a", argv[2]);
            const auto b = s.shareInstrument("b", a);
            s.connectNotes(graphInput, a); s.connectAudio(a, graphOutput); s.connectAudio(b, graphOutput); s.commit();
            Player p{s};
            for(int i = 0; i < 10; ++i) p.tick();
            const double was = s.parameter(b, 1), next = was == .1 ? .2 : .1;
            const auto sound = s.prepareSound(b, "Other", {{1, next}});
            const auto before = s.changeSequence();
            check(s.selectSound(b, sound), "select the sound");
            check(p.waitFor(before, [&](const Control::Change& c) { return c.kind == Kind::Value && c.node == b && c.param == 1 && c.value == next && c.plugin; }),
                  "a sound selected publishes its values");
            const auto compared = s.parametersCompared();
            for(int i = 0; i < 200; ++i) p.tick();
            check(s.parametersCompared() == compared, "and then nothing is compared");
        }
        std::puts("value reports ok");
        return 0;
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
}
