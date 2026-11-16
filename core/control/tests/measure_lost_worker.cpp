// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A worker that stops answering while the engine measures its plugins: Session::service() goes on,
// the worker the supervisor's to fail and restart. The budget's measurements ask a shared instance's
// worker how many sounds it holds (to learn how many parts it carries); a worker gone between the
// failure check and that request threw out of service() ("Plugin worker connection lost" in the
// switch-stress test, which kills workers at random). Here the worker is stopped, so the request
// times out every time, not by chance.
//   oscairn-measure-lost-worker-test SYNTH.clap   (with OSCAIRN_PLUGIN_WORKER set)
#include "oscairn/session.h"
#include "oscairn/plugin_host.h"
#include "../../plugin-host/src/StateFiles.h"

#include <array>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>
#include <unistd.h>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr uint32_t frames = 128;
    void render(Session& s, int n)
    {
        std::array<float, frames> l{}, r{};
        float* out[2]{l.data(), r.data()};
        for(int i = 0; i < n; ++i) { s.service(); s.process(out, frames); }
    }
    std::vector<int> workers()
    {
        std::vector<int> pids;
        if(FILE* list = popen(("pgrep -P " + std::to_string(::getpid()) + " -f oscairn-plugin-worker").c_str(), "r"))
        {
            char line[32];
            while(std::fgets(line, sizeof line, list)) pids.push_back(std::atoi(line));
            pclose(list);
        }
        return pids;
    }
}

int main(int argc, char** argv)
{
    if(argc != 2) { std::fprintf(stderr, "usage: %s SYNTH.clap\n", argv[0]); return 2; }
    if(placementPolicy().worker.empty()) { std::printf("skipped: no plugin worker (OSCAIRN_PLUGIN_WORKER)\n"); return 0; }
    // A request unanswered fails in a second; the plugins' own service requests wait, so measuring is the first to ask.
    ::setenv("OSCAIRN_WORKER_REPLY_SECONDS", "1", 1);
    ::setenv("OSCAIRN_WORKER_SERVICE_MS", "1000", 1);
    try
    {
        state_files::TemporaryDirectory home; // a budget's measurements, not the user's
        ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
        state_files::TemporaryDirectory temp;
        const auto file = temp.path / "two";
        {
            // Two Basic Synth parts, each a sound of its own: they open in one shared instance.
            Session s(48000, frames);
            for(uint32_t i = 0; i < 2; ++i)
            {
                const auto n = s.add("synth " + std::to_string(i), argv[1]);
                PartInput keys; keys.zones = {NoteZone{uint8_t(64 * i), uint8_t(64 * i + 63)}};
                const auto part = s.addPart("Part " + std::to_string(i), keys);
                s.insertIntoPart(part, 0, n); s.commit();
                check(s.selectSound(n, s.prepareSound(n, "Saw " + std::to_string(i), {{4, double(i)}})), "select a sound");
            }
            render(s, 2);
            s.savePerformance(file.string());
        }
        Session s(48000, frames);
        s.setBudget({.5, 0}); // measured: the budget's learning runs at each service
        s.loadPerformance(file.string());
        render(s, 40);
        const auto pids = workers();
        check(!pids.empty(), "the Performance plays in a worker");
        for(const auto pid : pids) ::kill(pid, SIGSTOP);
        std::string thrown;
        for(int i = 0; i < 3 && thrown.empty(); ++i)
            try { s.service(); } catch(const std::exception& e) { thrown = e.what(); }
        for(const auto pid : pids) ::kill(pid, SIGCONT);
        check(thrown.empty(), "service goes on past a worker that stopped answering: " + thrown);
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::printf("ok: measuring survives a worker that stopped answering\n");
    return 0;
}
