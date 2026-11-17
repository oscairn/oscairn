// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045's plugin groups and the output guard, through a Session: two parts whose instruments are loud (9e5 each,
// under the guard's +120 dBFS, in every block for 200) send in full to a bus, whose first effect thus runs away past
// it (1.8e6) into its second, both effects in one plugin host (OSCAIRN_PLUGIN_HOSTS=1), so the second's input is
// mixed in their worker. The worker guards what one node mixes from another as the engine guards what it reads
// (Worker.cpp's guard), so the runaway never reaches the second effect: the first's 200 blocks are not heard and none
// of the second's. Fed the runaway, the second's blocks would be past the bound too. (NaN and Inf never get this far:
// the CLAP host stops the plugin first, output-guard.)
//   oscairn-group-guard-test VOICE_FIXTURE.clap FIXTURE.clap
#include "oscairn/session.h"
#include "oscairn/plugin_host.h"
#include "../../plugin-host/src/StateFiles.h"
#include <array>
#include <cstdio>
#include <cstdlib>
#include <string>

using namespace oscairn;

int main(int argc, char** argv)
{
    if(argc != 3) { std::fprintf(stderr, "usage: %s VOICE_FIXTURE.clap FIXTURE.clap\n", argv[0]); return 2; }
    if(placementPolicy().worker.empty()) { std::printf("skipped: no plugin worker (OSCAIRN_PLUGIN_WORKER)\n"); return 0; }
    try
    {
        state_files::TemporaryDirectory home;
        ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
        constexpr uint32_t frames = 128;
        Session s(48000, frames);
        // Before any plugin loads: the one plugin host is started with the first, and its plugins read it there.
        ::setenv("OSCAIRN_FIXTURE_OUTPUT", "51:9e5:200", 1); // calls from 1: blocks 50 to 249
        const auto bus = s.addPart("Bus", {PartInput::Kind::Bus});
        const auto runaway = s.add("runaway", argv[2], "org.kick.test.effect"), after = s.add("after", argv[2], "org.kick.test.effect");
        s.insertIntoPart(bus, 0, runaway); s.insertIntoPart(bus, 1, after);
        for(int p = 0; p < 2; ++p)
        {
            const auto part = s.addPart("Loud " + std::to_string(p));
            s.insertIntoPart(part, 0, s.add("loud " + std::to_string(p), argv[1]));
            s.setSend(part, bus, 1);
        }
        ::unsetenv("OSCAIRN_FIXTURE_OUTPUT");
        s.commit();
        if(!s.setParameter(runaway, 7, 1) || !s.setParameter(after, 7, 1)) { std::fprintf(stderr, "FAIL: the effects' gain\n"); return 1; }
        s.note(true, 60, .8);
        std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()};
        for(int i = 0; i < 300; ++i) { s.service(); s.process(out, frames); }
        const auto bad = s.sessionStatus().badOutputBlocks;
        std::printf("%llu blocks not heard\n", (unsigned long long)bad);
        if(bad != 200) { std::fprintf(stderr, "FAIL: only the runaway effect's 200 blocks are not heard: %llu\n", (unsigned long long)bad); return 1; }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    std::printf("ok: a runaway stays in the plugin that made it\n");
    return 0;
}
