// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045, sleeping plugins: a plugin in a worker that says it is asleep (CLAP_PROCESS_SLEEP) is not
// called while nothing comes for it, and is called on the very block something does: a note, a host
// command, a panic; with native sound contexts (oscairn-clap's sound-snapshots example), a select and a
// choke_retiring. engine.timing's counts (calledBlocks, restedBlocks) say which blocks were which, and its
// steady time goes on through the blocks it slept.
//   oscairn-worker-sleep-test WORKER FIXTURE SOUND-SNAPSHOTS-FIXTURE
#include <oscairn/graph.h>
#include <oscairn/plugin_host.h>
#include <clap/clap.h>
#include <cstdio>
#include <string>
#include <vector>

namespace
{
int failures = 0;
void check(bool ok, const std::string& what)
{
    if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; }
}
}

int main(int argc, char** argv)
{
    if(argc < 4) { std::fprintf(stderr, "usage: %s WORKER FIXTURE SOUND-SNAPSHOTS-FIXTURE\n", argv[0]); return 2; }
    constexpr uint32_t frames = 64;
    constexpr double rate = 48000;
    try
    {
        auto host = oscairn::loadPluginInWorker(argv[1], argv[2], "org.kick.test.sleep", rate, frames, oscairn::ProcessingMode::Offline);
        oscairn::Graph g(rate, frames);
        const auto n = g.add(host);
        g.connectNotes(oscairn::graphInput, n); g.connectAudio(n, oscairn::graphOutput); g.commit();
        std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
        auto block = [&](std::vector<kick::clap::Event> e = {}) {
            const auto before = host->statistics();
            g.process(out, frames, nullptr, e.data(), uint32_t(e.size()));
            const auto after = host->statistics();
            return std::pair{after.calledBlocks - before.calledBlocks, after.restedBlocks - before.restedBlocks};
        };
        check(block().first == 1, "its first block is called (it has not slept yet)");
        check(host->asleep(), "silent, it says it is asleep");
        uint32_t rested = 0;
        for(int i = 0; i < 20; ++i) rested += block().second;
        check(rested == 20, "asleep with nothing for it: not called, " + std::to_string(rested) + " of 20 blocks slept through");
        check(l[0] == 0 && r[frames - 1] == 0, "its outputs are silence");
        check(block({kick::clap::Event::note(true, 60, 1)}) == std::pair<uint64_t, uint64_t>{1, 0}, "a note wakes it on that very block");
        check(!host->asleep() && l[0] != 0, "and it sounds");
        block({kick::clap::Event::note(false, 60, 0)});
        for(int i = 0; i < 3; ++i) block();
        check(host->asleep() && block().second == 1, "let go, it sleeps again");
        // A host command (a parameter set) reaching the plugin wakes it for the next block, as in the engine's process.
        clap_event_param_value_t v{}; v.header = {sizeof v, 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, 0};
        v.param_id = 7; v.note_id = -1; v.port_index = v.channel = v.key = -1; v.value = .3;
        check(host->sendEvent(kick::clap::Event::from(v)), "the command is taken");
        check(block().first == 1, "a host command wakes it on the next block");
        check(block().second == 1, "then it sleeps again");
        host->panic();
        check(block().first == 1, "a panic wakes it on the next block");
        check(block().second == 1, "and after it, asleep again");
        // Its steady time went on through the blocks it slept: the worker took them as its own idle blocks, told with
        // the next block it was called for.
        block({kick::clap::Event::note(true, 62, 1)});
        const auto s = host->statistics();
        check(s.idleBlocks >= s.restedBlocks, "the blocks slept through count as the plugin's idle blocks: " + std::to_string(s.idleBlocks) + " of " + std::to_string(s.restedBlocks));
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    // A select (an accepted one) and a queued choke_retiring reaching a plugin asleep wake it for the next block.
    try
    {
        auto host = oscairn::loadPluginInWorker(argv[1], argv[3], "org.oscairn.example.sound-snapshots", rate, frames, oscairn::ProcessingMode::Offline);
        check(host->soundContextCapacity() > 0 && host->ringsOut(), "the example has sound contexts and ring-out");
        const auto a = host->prepareSound("A", {}), b = host->prepareSound("B", {});
        check(host->selectSound(a), "A selected");
        oscairn::Graph g(rate, frames);
        const auto n = g.add(host);
        g.connectNotes(oscairn::graphInput, n); g.connectAudio(n, oscairn::graphOutput); g.commit();
        std::vector<float> l(frames), r(frames); float* out[]{l.data(), r.data()};
        auto block = [&] {
            const auto before = host->statistics();
            g.process(out, frames, nullptr, nullptr, 0);
            const auto after = host->statistics();
            return std::pair{after.calledBlocks - before.calledBlocks, after.restedBlocks - before.restedBlocks};
        };
        for(int i = 0; i < 3; ++i) block();
        check(host->asleep() && block().second == 1, "idle, the example sleeps and is not called");
        check(host->selectSound(b), "B selected");
        check(block().first == 1, "a select wakes it on the next block");
        check(block().second == 1, "then it sleeps again");
        check(host->chokeRetiring(), "choke_retiring queued");
        check(block().first == 1, "a choke_retiring wakes it on the next block");
        check(block().second == 1, "and it sleeps again");
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(!failures) std::printf("worker sleep: not called while asleep, called on the block a note, a command, a panic, a select or a choke_retiring comes\n");
    return failures ? 1 : 0;
}
