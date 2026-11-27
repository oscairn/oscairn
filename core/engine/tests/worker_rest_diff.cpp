// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045, sleeping plugins: leaving a worker uncalled while its plugin sleeps changes nothing it plays. One session, run
// three times in processes of its own (OSCAIRN_WORKER_REST is read once at the engine's start): all (any worker asleep with
// nothing for it rests, playable or not), unset (disabled: none here, every node can be played) and 0 (every worker called
// every block, its host leaving the plugin uncalled). Every node's outputs and the graph's, every block, must be the same,
// sample for sample. The nodes, each in its own worker, all playable: the fixture's sleep instrument into its effect
// (sleeping on silent input), the sound-snapshots example (a 5 ms release, then asleep) and the follower (asleep after every
// block, showing the free-running beat). The session: notes after long silence and at the hold's edge (the blocks before,
// at and after the one where resting starts), a note in the block right after a tail ends, parameter changes and a
// panic while asleep, the instrument waking itself (request_process from its main thread, KICK_FIXTURE_SELF_WAKE), the
// transport starting, crossing a bar and stopping while they rest. With all, the rested counts rise at rest and each note's
// block calls the instrument (no block late).
//   oscairn-worker-rest-diff-test WORKER KICK_FIXTURE SOUND_SNAPSHOTS_FIXTURE FOLLOWER_FIXTURE
#include <oscairn/graph.h>
#include <oscairn/plugin_host.h>
#include <oscairn/transport.h>
#include <clap/clap.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <string>
#include <unistd.h>
#include <vector>

namespace
{
constexpr uint32_t frames = 64;
constexpr double rate = 48000;
constexpr int hold = 192; // WorkerHost's: blocks asleep still called before a worker rests

// One run: every block's outputs to OUT, the counts to OUT.txt. Returns the failures of its own checks.
int render(char** argv, const std::string& out)
{
    const char* restVar = std::getenv("OSCAIRN_WORKER_REST");
    const bool resting = restVar && std::string(restVar) == "all";
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL (rest %s): %s\n", restVar ? restVar : "disabled", what.c_str()); ++failures; } };
    setenv("KICK_FIXTURE_EFFECT_SLEEP", "1", 1); setenv("KICK_FIXTURE_SELF_WAKE", "1", 1); setenv("FOLLOWER_SLEEPS", "1", 1); // the workers inherit them
    const auto mode = oscairn::ProcessingMode::Offline;
    auto synth = oscairn::loadPluginInWorker(argv[1], argv[2], "org.kick.test.sleep", rate, frames, mode);
    auto effect = oscairn::loadPluginInWorker(argv[1], argv[2], "org.kick.test.effect", rate, frames, mode);
    auto sine = oscairn::loadPluginInWorker(argv[1], argv[3], "org.oscairn.example.sound-snapshots", rate, frames, mode);
    auto follower = oscairn::loadPluginInWorker(argv[1], argv[4], "org.oscairn.test.follower", rate, frames, mode);
    check(sine->selectSound(sine->prepareSound("A", {})), "the sine's sound selected");
    oscairn::Graph g(rate, frames);
    const auto a = g.add(synth), b = g.add(effect), c = g.add(sine), d = g.add(follower);
    g.connectNotes(oscairn::graphInput, a); g.connectAudio(a, b); g.connectAudio(b, oscairn::graphOutput);
    g.connectNotes(oscairn::graphInput, c); g.connectAudio(c, oscairn::graphOutput);
    g.connectAudio(d, oscairn::graphOutput);
    g.commit();
    const std::vector<std::pair<oscairn::NodeId, uint32_t>> shown{{a, 2}, {b, 2}, {c, 2}, {d, 3}};
    const std::vector<std::shared_ptr<oscairn::PluginHost>> hosts{synth, effect, sine, follower};
    oscairn::Transport transport(rate, 120);
    transport.setPlaying(false);
    std::vector<float> l(frames), r(frames); float* outs[]{l.data(), r.data()};
    std::ofstream file(out, std::ios::binary);
    int block = 0;
    uint64_t notes = 0, notesCalled = 0;
    auto run = [&](std::vector<kick::clap::Event> e = {}) {
        const auto before = synth->statistics().calledBlocks;
        const auto t = transport.current();
        g.process(outs, frames, &t, e.data(), uint32_t(e.size()));
        transport.advance(frames);
        file.write(reinterpret_cast<const char*>(l.data()), frames * sizeof(float));
        file.write(reinterpret_cast<const char*>(r.data()), frames * sizeof(float));
        for(const auto& [n, channels] : shown) for(uint32_t ch = 0; ch < channels; ++ch) file.write(reinterpret_cast<const char*>(g.audioOutput(n, ch)), frames * sizeof(float));
        ++block;
        return synth->statistics().calledBlocks - before;
    };
    auto wait = [&](int n) { for(int i = 0; i < n; ++i) run(); };
    // A note to both instruments at a sample in the block, the instrument called on that very block; let go the next block.
    auto note = [&](int key, uint32_t at) {
        ++notes; notesCalled += run({kick::clap::Event::note(true, key, .8, at)});
        run({kick::clap::Event::note(false, key, 0, at / 2)});
    };
    auto rested = [&] { uint64_t n = 0; for(auto& h : hosts) n += h->statistics().restedBlocks; return n; };
    auto settle = [&] { for(int i = 0; i < 400 && !(synth->asleep() && sine->asleep()); ++i) run(); };

    wait(600); // asleep long past the hold
    const auto restedAtRest = rested();
    check(!resting || restedAtRest > 4 * 300, "at rest every worker rests: " + std::to_string(restedAtRest) + " rested blocks");
    note(60, 17); // the first note after long silence
    // At the hold's edge: blocks since both instruments went to sleep before the next note.
    for(const int k : {1, 2, hold - 1, hold, hold + 1, hold + 2, 3 * hold})
    {
        settle(); wait(k); note(62 + k % 7, uint32_t(k * 7 % frames));
    }
    // The block right after the sine's tail ends (its release done, it said it is asleep).
    settle(); note(67, 0);
    // Parameter changes while asleep: the instrument's gain and the effect's, from the control thread, then a note.
    settle(); wait(hold + 50);
    check(synth->setParameter(7, .8) && effect->setParameter(7, .3), "parameters set while asleep");
    wait(hold + 50); note(69, 31);
    // A panic while asleep, then a note.
    settle(); wait(hold + 50); synth->panic(); sine->panic(); wait(hold + 50); note(71, 5);
    // The instrument waking itself: its block sounds .25 through the effect.
    settle(); wait(hold + 50);
    const auto selfBefore = synth->statistics().calledBlocks;
    synth->parameterText(7, .5);
    run();
    check(synth->statistics().calledBlocks == selfBefore + 1, "the instrument woke itself and was called on the next block");
    check(g.audioOutput(a, 0)[0] == .25f, "and sounded in it: " + std::to_string(g.audioOutput(a, 0)[0]));
    // The transport starting, crossing a bar (4/4 at 120: 2 s) and stopping, with the workers resting between.
    wait(hold + 50); transport.setPlaying(true); wait(1600); note(72, 9); wait(hold + 50); transport.setPlaying(false); wait(hold + 50);
    const auto restedAfter = rested();
    check(!resting || restedAfter > restedAtRest + 4 * 1000, "resting went on through the session: " + std::to_string(restedAfter));
    check(notesCalled == notes, "each note's own block called the instrument: " + std::to_string(notesCalled) + " of " + std::to_string(notes));
    uint64_t lost = 0; for(auto& h : hosts) lost += h->statistics().lateBlocks + h->statistics().silentBlocks;
    check(lost == 0, "no block lost: " + std::to_string(lost));
    std::ofstream(out + ".txt") << block << " blocks, " << restedAfter << " rested\n";
    std::printf("rest %s: %d blocks, %llu rested (%llu at the first note), %llu of %llu note blocks called\n", restVar ? restVar : "disabled", block,
                (unsigned long long)restedAfter, (unsigned long long)restedAtRest, (unsigned long long)notesCalled, (unsigned long long)notes);
    return failures;
}

std::string slurp(const std::string& path)
{
    std::ifstream f(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(f), std::istreambuf_iterator<char>()};
}
}

int main(int argc, char** argv)
{
    if(argc == 7 && !std::strcmp(argv[5], "--render"))
    {
        try { return render(argv, argv[6]) ? 1 : 0; }
        catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    }
    if(argc < 5) { std::fprintf(stderr, "usage: %s WORKER KICK_FIXTURE SOUND_SNAPSHOTS_FIXTURE FOLLOWER_FIXTURE\n", argv[0]); return 2; }
    const std::string base = std::string(std::getenv("TMPDIR") ? std::getenv("TMPDIR") : "/tmp") + "/oscairn-rest-diff-" + std::to_string(getpid());
    auto quoted = [](const std::string& s) { std::string q = "'"; for(char ch : s) q += ch == '\'' ? std::string("'\\''") : std::string(1, ch); return q + "'"; };
    int failures = 0;
    std::vector<std::string> outputs;
    for(const char* mode : {"all", "", "0"})
    {
        const auto out = base + "-" + (*mode ? mode : "disabled");
        std::string command = std::string(*mode ? "OSCAIRN_WORKER_REST=" + std::string(mode) + " " : "") + quoted(argv[0]);
        for(int i = 1; i < 5; ++i) command += " " + quoted(argv[i]);
        command += " --render " + quoted(out);
        // Unset for the disabled run: the parent's own environment may hold the variable.
        if(!*mode) command = "env -u OSCAIRN_WORKER_REST " + command;
        if(std::system(command.c_str()) != 0) { std::fprintf(stderr, "FAIL: the run with rest %s failed\n", *mode ? mode : "disabled"); ++failures; }
        outputs.push_back(slurp(out));
        std::remove(out.c_str()); std::remove((out + ".txt").c_str());
    }
    const auto& reference = outputs.back(); // every worker called
    const char* names[]{"all", "disabled"};
    for(size_t i = 0; i < 2; ++i)
    {
        if(outputs[i].size() != reference.size() || reference.empty()) { std::fprintf(stderr, "FAIL: rest %s rendered %zu bytes, every worker called %zu\n", names[i], outputs[i].size(), reference.size()); ++failures; continue; }
        if(outputs[i] != reference)
        {
            size_t at = 0; while(outputs[i][at] == reference[at]) ++at;
            const size_t perBlock = frames * sizeof(float) * (2 + 2 + 2 + 2 + 3);
            std::fprintf(stderr, "FAIL: rest %s differs from every worker called at block %zu, byte %zu of it\n", names[i], at / perBlock, at % perBlock);
            ++failures;
        }
    }
    if(!failures) std::printf("worker rest diff: resting changed no sample (all and disabled against every worker called)\n");
    return failures ? 1 : 0;
}
