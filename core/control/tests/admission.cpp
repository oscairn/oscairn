// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025's admission per core: plugins render in parallel on the audio cores, a part's chain one
// plugin after another. Each plugin measured at 40 % of a block, with a 75 % budget: three parts of
// one plugin each fit (120 % in all, spread over the cores), where one sum against one block refused
// them; a part with two in a chain (80 % in one block) is refused, saying so; the same from figures
// of too few blocks opens, with a notice.
//   oscairn-admission-test FIXTURE.clap
#include "oscairn/session.h"
#include "oscairn/realtime.h"
#include "../../plugin-host/src/StateFiles.h"
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <string>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    std::string read(const fs::path& file) { std::ifstream in(file); return {std::istreambuf_iterator<char>(in), {}}; }
}

int main(int argc, char** argv)
{
    if(argc != 2) return 2;
    const char* fixture = argv[1];
    state_files::TemporaryDirectory temp;
    const auto home = temp.path / "home";
    fs::create_directories(home);
    ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
    // Two Performances of the fixture's effect: three parts of one each; and a part of two in a chain beside one of one.
    const auto spread = temp.path / "spread", chained = temp.path / "chained";
    std::string key;
    {
        Session s(rate, frames);
        s.setBudget({.75, 0});
        for(const char* name : {"A", "B", "C"}) { const auto part = s.addPart(name); s.insertIntoPart(part, 0, s.add(name, fixture, "org.kick.test.effect")); }
        s.commit();
        float l[frames], r[frames]; float* out[]{l, r};
        for(int i = 0; i < 300; ++i) { s.service(); s.process(out, frames); }
        s.savePerformance(spread.string());
        s.saveMeasurementsNow();
        const auto text = read(home / "measurements.json");
        const auto at = text.find("\"plugins\":{\"") + 12;
        key = text.substr(at, text.find('"', at) - at);
    }
    {
        Session s(rate, frames);
        const auto one = s.addPart("One"); s.insertIntoPart(one, 0, s.add("one", fixture, "org.kick.test.effect"));
        const auto two = s.addPart("Two"); s.insertIntoPart(two, 0, s.add("first", fixture, "org.kick.test.effect")); s.insertIntoPart(two, 1, s.add("second", fixture, "org.kick.test.effect"));
        s.commit();
        s.savePerformance(chained.string());
    }
    check(!key.empty(), "the fixture's measurement key");
    const auto cores = std::max(1u, worker::usableCores() - 1);
    auto figures = [&](int samples) {
        std::ofstream(home / "measurements.json") << "{\"version\":5,\"plugins\":{\"" << key << "\":{\"cpu\":0.4,\"rest\":0.4,\"voice\":0,\"voices\":0,\"memory\":1000,\"samples\":" << samples << "}}}";
    };
    auto open = [&](const fs::path& performance, std::string& notices) {
        Session s(rate, frames);
        s.setBudget({.75, 0});
        std::string why;
        try { s.loadPerformance(performance.string()); } catch(const std::exception& e) { why = e.what(); }
        for(const auto& n : s.sessionStatus().notices) notices += n + "\n";
        return why;
    };
    figures(10000);
    {
        Session s(rate, frames);
        s.setBudget({.75, 0});
        const auto e = s.estimate(spread.string());
        std::printf("spread: %.2f in all on %u cores, longest %.2f, fits %d\n", e.cpu, e.cores, e.longest, int(e.fits));
        check(std::abs(e.cpu - 1.2) < 1e-6 && std::abs(e.longest - .4) < 1e-6 && e.cores == cores, "120 % in all, 40 % its longest chain");
        check(cores < 2 || (e.fits && !e.uncertain), "three parts at 40 % fit, spread over the cores, where one sum against one block refused them");
        const auto c = s.estimate(chained.string());
        std::printf("chained: %.2f in all, longest %.2f, fits %d\n", c.cpu, c.longest, int(c.fits));
        check(std::abs(c.longest - .8) < 1e-6 && !c.fits, "a chain of two at 40 % each (80 % of one block) does not fit");
    }
    std::string notices;
    const auto refused = open(chained, notices);
    check(refused.find("costliest part") != std::string::npos && refused.find("needs 80% of the audio time") != std::string::npos, "refused, saying its costliest part needs 80 %: " + refused);
    figures(5);
    notices.clear();
    const auto opened = open(chained, notices);
    check(opened.empty(), "from figures of too few blocks it opens: " + opened);
    check(notices.find("too few blocks") != std::string::npos, "with a notice: " + notices);
    return failures ? 1 : 0;
}
