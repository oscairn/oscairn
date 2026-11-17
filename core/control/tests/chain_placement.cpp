// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's `chain` placement (compute.workers, OSCAIRN_WORKERS=chain) through a Session, as the engine opens a
// Performance: parts built with an instrument and three effects, saved, and opened again; the plugins of a part's
// insert chain must share one worker process, each instrument its own. And three stacked parts of a plugin that can
// share an instance (the Basic Synth, as the Sampler): an instance each by default, each in a worker of its own under
// either placement; one instance in one worker with OSCAIRN_SHARING=memory.
//   oscairn-chain-placement-test INSTRUMENT.clap EFFECT.clap SHARING-SYNTH.clap
#include "oscairn/session.h"
#include "oscairn/plugin_host.h"
#include "../../plugin-host/src/StateFiles.h"
#include <cstdio>
#include <cstdlib>
#include <set>
#include <string>

using namespace oscairn;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr uint32_t frames = 128;
    constexpr uint32_t parts = 3, inserts = 3;

    // The processes the plugins of the open Performance run in.
    std::set<std::string> workers(Session& s) { std::set<std::string> w; for(const auto& i : s.engineTiming(false).instances) w.insert(i.worker); return w; }
}

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: %s INSTRUMENT.clap EFFECT.clap SHARING-SYNTH.clap\n", argv[0]); return 2; }
    try
    {
        state_files::TemporaryDirectory home;
        ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
        const auto file = home.path / "chains";
        {
            Session s(48000, frames);
            for(uint32_t p = 0; p < parts; ++p)
            {
                const auto n = s.add("instrument " + std::to_string(p), argv[1]);
                PartInput keys; keys.zones = {NoteZone{uint8_t(32 * p), uint8_t(32 * p + 31)}};
                const auto part = s.addPart("Part " + std::to_string(p), keys);
                s.insertIntoPart(part, 0, n);
                for(uint32_t i = 0; i < inserts; ++i) s.insertIntoPart(part, 1 + i, s.add("effect " + std::to_string(p) + "-" + std::to_string(i), argv[2]));
                s.commit();
            }
            s.savePerformance(file.string());
        }
        Session s(48000, frames);
        s.loadPerformance(file.string());
        const auto n = workers(s).size();
        const bool chain = placementPolicy().chain;
        std::printf("%s: %zu worker processes for %u plugins\n", chain ? "chain" : "plugin", n, parts * (1 + inserts));
        // chain: each instrument alone and each part's inserts together; plugin: one each.
        check(n == (chain ? 2 * parts : parts * (1 + inserts)), "worker processes: " + std::to_string(n));

        // Three parts of the sharing synth over the whole keyboard, saved with an instance each, then opened.
        const auto stacked = home.path / "stacked";
        {
            ::setenv("OSCAIRN_SHARING", "cpu", 1);
            Session t(48000, frames);
            for(uint32_t p = 0; p < parts; ++p) { const auto part = t.addPart("Layer " + std::to_string(p)); t.insertIntoPart(part, 0, t.add("layer " + std::to_string(p), argv[3])); t.commit(); }
            t.savePerformance(stacked.string());
        }
        for(const char* bias : {"", "memory"})
        {
            if(*bias) ::setenv("OSCAIRN_SHARING", bias, 1); else ::unsetenv("OSCAIRN_SHARING");
            Session t(48000, frames);
            t.loadPerformance(stacked.string());
            const auto timing = t.engineTiming(false);
            const auto w = workers(t).size();
            const bool shared = *bias;
            std::printf("OSCAIRN_SHARING=%s: %zu instances, %zu workers for %u stacked parts\n", bias, timing.instances.size(), w, parts);
            check(timing.instances.size() == (shared ? 1 : parts) && w == timing.instances.size() && t.graphState().shared.size() == (shared ? parts : 0),
                  std::string("OSCAIRN_SHARING=") + bias + ": " + std::to_string(timing.instances.size()) + " instances in " + std::to_string(w) + " workers");
        }
        ::unsetenv("OSCAIRN_SHARING");
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    return failures ? 1 : 0;
}
