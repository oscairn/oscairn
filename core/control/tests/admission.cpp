// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025's admission per core: plugins render in parallel on the audio cores, a part's chain one
// plugin after another. Each plugin measured at 40 % of a block, with a 75 % budget: three parts of
// one plugin each fit (120 % in all, spread over the cores), where one sum against one block refused
// them; a part with two in a chain (80 % in one block) is refused, saying so; the same from figures
// of too few blocks opens, with a notice. And a part sending to a bus counts the bus's chain after its own (RFC 0045's
// guard, 12e): a part of one at 40 % sending at 40 % to a bus of one at 40 % needs 80 % of one block, and is refused.
// A busiest figure of a load played before (40 %, certain) whose rest fits (5 %) opens it, with a notice: CPU refuses only
// past the plugins' cost at rest (the unit's fm2 at 0.339 refused Sick tines, 232 %). An impossible figure (above the block)
// is dropped as the file is read. The busiest load does not move for a spike shorter than half its window of ticks.
//   oscairn-admission-test FIXTURE.clap
#include "oscairn/session.h"
#include "oscairn/realtime.h"
#include "oscairn/busiest_load.h"
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
    const auto spread = temp.path / "spread", chained = temp.path / "chained", sent = temp.path / "sent";
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
    {
        Session s(rate, frames);
        const auto bus = s.addPart("Bus", {PartInput::Kind::Bus}); s.insertIntoPart(bus, 0, s.add("reverb", fixture, "org.kick.test.effect"));
        const auto part = s.addPart("Part"); s.insertIntoPart(part, 0, s.add("instrument", fixture, "org.kick.test.effect"));
        s.setSend(part, bus, .4);
        s.commit();
        s.savePerformance(sent.string());
    }
    check(!key.empty(), "the fixture's measurement key");
    const auto cores = std::max(1u, worker::usableCores() - 1);
    auto figures = [&](int samples, double cpu = .4, double rest = .4) {
        std::ofstream(home / "measurements.json") << "{\"version\":5,\"plugins\":{\"" << key << "\":{\"cpu\":" << cpu << ",\"rest\":" << rest << ",\"voice\":0,\"voices\":0,\"memory\":1000,\"samples\":" << samples << "}}}";
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
        const auto b = s.estimate(sent.string());
        std::printf("sent: %.2f in all, longest %.2f, fits %d\n", b.cpu, b.longest, int(b.fits));
        check(std::abs(b.longest - .8) < 1e-6 && !b.fits, "a part sending to a bus counts the bus's chain after its own: 80 % of one block does not fit");
    }
    std::string notices;
    const auto refused = open(chained, notices);
    check(refused.find("costliest part") != std::string::npos && refused.find("needs 80% of the audio time") != std::string::npos, "refused, saying its costliest part needs 80 %: " + refused);
    figures(5);
    notices.clear();
    const auto opened = open(chained, notices);
    check(opened.empty(), "from figures of too few blocks it opens: " + opened);
    check(notices.find("too few blocks") != std::string::npos, "with a notice: " + notices);

    // A busiest figure of 40 % from many blocks (certain) whose rest is 5 %: it opens, said; the load guard sheds voices.
    figures(100000, .4, .05);
    notices.clear();
    const auto busy = open(chained, notices);
    check(busy.empty(), "a chain fitting at rest opens past its busiest figures: " + busy);
    check(notices.find("may not fit") != std::string::npos && notices.find("at rest") != std::string::npos, "with a notice: " + notices);
    // Its rest past one block's budget (2 x 40 %) is refused as before.
    figures(100000, .4, .4);
    check(open(chained, notices).find("costliest part") != std::string::npos, "a chain past the budget at rest is refused");
    // An impossible figure (a share above the block, which a block on time cannot cost) is dropped as it is read: the guess.
    figures(100000, 1.5, .05);
    {
        Session s(rate, frames);
        s.setBudget({.75, 0});
        const auto c = s.estimate(chained.string());
        check(c.fits && !c.uncertain && c.longest < .1, "an impossible figure is dropped as the file is read: longest " + std::to_string(c.longest));
    }

    // The busiest load (BusiestLoad), ticks of 8 blocks at 5 %: a spike of two ticks at the whole block moves it not at
    // all; one of three by at most its blocks over fastBlocks (24 / 400 of the difference).
    {
        BusiestLoad b;
        for(int i = 0; i < 2000; ++i) b.add(.05, 8);
        const double steady = b.fast;
        double most = 0;
        for(int i = 0; i < 2; ++i) b.add(1, 8);
        for(int i = 0; i < 500; ++i) { b.add(.05, 8); most = std::max(most, b.fast); }
        check(std::abs(steady - .05) < 1e-9 && most <= steady + 1e-12, "a spike of two ticks leaves the busiest load at " + std::to_string(steady) + ": " + std::to_string(most));
        for(int i = 0; i < 3; ++i) b.add(1, 8);
        most = 0;
        for(int i = 0; i < 500; ++i) { b.add(.05, 8); most = std::max(most, b.fast); }
        check(most <= .05 + .95 * 24 / BusiestLoad::fastBlocks + 1e-9, "a spike of three ticks moves it by at most their blocks' weight: " + std::to_string(most));
    }
    return failures ? 1 : 0;
}
