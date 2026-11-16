// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Plugin workers follow the plugins: added, removed and undone, or a Performance loaded again, the processes
// this one has stay bounded by what the Performance and its undo history hold, not by the cycles.
//   oscairn-worker-count-test DELAY.clap BASIC_SYNTH.clap       (with OSCAIRN_PLUGIN_WORKER set)
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <array>
#include <filesystem>
#include <iostream>
#include <string>
#include <thread>
#include <vector>
#include <unistd.h>

using namespace oscairn;

namespace
{
    void check(bool ok, const std::string& why) { if(!ok) throw std::runtime_error(why); }
    // The oscairn-plugin-worker processes that are children of this one.
    size_t workers()
    {
        const auto mine = std::to_string(getpid());
        FILE* ps = popen("ps -A -o ppid= -o command=", "r");
        check(ps, "ps");
        size_t count = 0; char line[4096];
        while(std::fgets(line, sizeof line, ps))
        {
            const std::string text(line);
            const auto first = text.find_first_not_of(' ');
            if(first == std::string::npos || text.compare(first, mine.size(), mine) != 0 || text[first + mine.size()] != ' ') continue;
            count += text.find("oscairn-plugin-worker") != std::string::npos;
        }
        pclose(ps);
        return count;
    }
    // Workers end in the background after their instance goes: counted once they stop going.
    size_t settled()
    {
        auto last = workers();
        for(int i = 0; i < 20; ++i)
        {
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
            const auto now = workers();
            if(now == last && i >= 2) break;
            last = now;
        }
        return last;
    }
}

int main(int argc, char** argv)
{
    if(argc != 3) return 2;
    if(!std::getenv("OSCAIRN_PLUGIN_WORKER")) { std::cout << "ok: no worker set, nothing to count\n"; return 0; }
    state_files::TemporaryDirectory home;
    ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
    try
    {
        Session s(48000, 128);
        const auto synth = s.add("synth", argv[2]);
        const auto lead = s.addPart("Lead");
        s.insertIntoPart(lead, 0, synth); s.commit();
        const auto saved = (std::filesystem::path(home.path) / "saved").string();
        s.savePerformance(saved);
        std::array<float, 128> l{}, r{}; float* out[]{l.data(), r.data()};
        auto tick = [&] { s.service(); check(s.process(nullptr, 0, out, 2, 128), "render"); s.service(); };
        tick();
        const auto start = settled();
        std::vector<size_t> seen;
        for(int cycle = 0; cycle < 6; ++cycle)
        {
            const auto effect = s.add("echo", argv[1]);
            s.insertIntoPart(lead, 1, effect); s.commit(); tick();
            s.remove(effect); s.commit(); tick();
            s.undo(); s.undo(); tick();
            s.loadPerformance(saved); for(int i = 0; i < 4; ++i) tick();
            seen.push_back(settled());
        }
        std::string all;
        for(const auto n : seen) all += ' ' + std::to_string(n);
        std::cout << "workers: " << start << " at the start, then" << all << '\n';
        // What may be alive: the Performance's own, the plugins its undo history holds (8 at most), the ones
        // kept warm (32 at most, and none with memory classes off here); the cycles add none beyond that.
        check(seen.back() <= seen[1], "the workers grew with the cycles: " + all);
    }
    catch(const std::exception& e) { std::cerr << "FAIL: " << e.what() << '\n'; return 1; }
    std::cout << "ok: workers bounded across add, remove, undo and load\n";
    return 0;
}
