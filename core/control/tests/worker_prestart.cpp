// SPDX-License-Identifier: AGPL-3.0-or-later
//
// An open starts the worker processes its loads need together, not one load after another (issue 9: a Performance
// switch spent half its time starting workers in turn). The worker here takes half a second to start (a wrapper
// that waits, then runs the real one), so four parts in workers of their own open in about one start, not four; and
// no process is left over: every worker running holds a plugin. With `chain`, the same for groups' processes.
//   oscairn-worker-prestart-test WORKER SYNTH.clap DELAY.clap
#include "oscairn/session.h"
#include "oscairn/plugin_host.h"
#include "../../plugin-host/src/StateFiles.h"
#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr uint32_t frames = 128;
    constexpr double startup = .5; // seconds each worker waits before it starts
    void block(Session& s) { std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()}; s.service(); s.process(out, frames, {}); }
}

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: %s WORKER SYNTH DELAY\n", argv[0]); return 2; }
    const std::string worker = argv[1], synth = argv[2], delay = argv[3];
    try
    {
        state_files::TemporaryDirectory temp;
        ::setenv("OSCAIRN_DATA_HOME", temp.path.c_str(), 1);
        const auto slow = temp.path / "slow-worker";
        std::ofstream(slow) << "#!/bin/sh\nsleep " << startup << "\nexec \"" << worker << "\" \"$@\"\n";
        fs::permissions(slow, fs::perms::owner_all);
        const auto empty = temp.path / "empty", four = temp.path / "four";
        { Session s(48000, frames); s.savePerformance(empty.string()); }
        {
            Session s(48000, frames); // in the engine's process: saved as any Performance is
            for(int i = 0; i < 4; ++i)
            {
                const auto n = s.add("synth " + std::to_string(i), synth), fx = s.add("delay " + std::to_string(i), delay);
                const auto part = s.addPart("Part " + std::to_string(i)); s.insertIntoPart(part, 0, n); s.insertIntoPart(part, 1, fx); s.commit();
            }
            s.savePerformance(four.string());
        }
        for(const bool chain : {false, true})
        {
            auto policy = placementPolicy(); policy.worker = slow.string(); policy.hosts = 0; policy.chain = chain; setPlacementPolicy(policy);
            const auto label = std::string(chain ? "chain: " : "a worker each: ");
            Session s(48000, frames);
            s.loadPerformance(empty.string()); block(s);
            const auto began = std::chrono::steady_clock::now();
            s.loadPerformance(four.string());
            const double took = std::chrono::duration<double>(std::chrono::steady_clock::now() - began).count();
            block(s);
            std::printf("%sopened in %.2f s, %zu worker processes\n", label.c_str(), took, pluginHosts().size());
            const auto hosts = pluginHosts();
            // One after another, at least four starts (with chain a part's plugins may share their group's process).
            check(hosts.size() >= 4 && took < startup * 2.5, label + "the workers started together: " + std::to_string(took) + " s for " + std::to_string(hosts.size()) + ", one after another " + std::to_string(startup * double(hosts.size())) + " s");
            for(const auto& h : hosts) check(h.pid > 0 && !h.plugins.empty(), label + "each process runs and holds a plugin: none left spare");
            check(s.sessionStatus().failures.empty(), label + "no plugin failed");
            s.loadPerformance(empty.string()); block(s);
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::printf("worker-prestart: ok\n");
    return 0;
}
