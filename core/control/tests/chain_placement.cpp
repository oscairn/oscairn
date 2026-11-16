// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's `chain` placement (compute.workers, OSCAIRN_WORKERS=chain) through a Session, as the engine opens a
// Performance: parts built with an instrument and three effects, saved, and opened again; the plugins of a part's
// insert chain must share one worker process, each instrument its own.
//   oscairn-chain-placement-test INSTRUMENT.clap EFFECT.clap
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
    if(argc != 3) { std::fprintf(stderr, "usage: %s INSTRUMENT.clap EFFECT.clap\n", argv[0]); return 2; }
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
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    return failures ? 1 : 0;
}
