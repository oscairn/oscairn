// SPDX-License-Identifier: AGPL-3.0-or-later
// The engine thread's cost per block at rest (the owner's figure: a feature not acting costs nothing measurable): PARTS
// instruments (the voice fixture, oscairn.voice-budget on) each through an insert, chain placement, nothing played; with
// `warm`, one more instrument loaded and released from the graph, held as Session keeps an instance warm (RFC 0021).
// The block on the engine thread alone, as on the FANTOM-07 at rest (OSCAIRN_AUDIO_HELPERS=0 unless set). Prints the engine
// thread's CPU time per block (mean, p50) and its user time alone (the kernel's hand-offs left out: steadier), and the
// syscalls and context switches per block (macOS: the task's, at rest nearly all the engine thread's hand-offs; Linux: the
// thread's context switches). This Mac's times are bimodal (P and E cores): compare two builds by alternating runs, by
// their minimum; for counts that do not move run it under callgrind (docs/developer-guide.md).
//   oscairn-rest-bench WORKER VOICE_FIXTURE.clap [PARTS] [SECONDS] [warm]
#include <oscairn/graph.h>
#include <oscairn/realtime.h>
#include <oscairn/plugin_host.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <string>
#include <thread>
#include <vector>
#ifdef __APPLE__
#include <mach/mach.h>
#else
#include <sys/resource.h>
#endif

namespace
{
struct Counts { double syscalls = 0, switches = 0, user = 0; }; // user: the calling thread's user CPU time, us
Counts counts()
{
#ifdef __APPLE__
    task_events_info_data_t e{}; mach_msg_type_number_t n = TASK_EVENTS_INFO_COUNT;
    task_info(mach_task_self(), TASK_EVENTS_INFO, reinterpret_cast<task_info_t>(&e), &n);
    thread_basic_info_data_t t{}; mach_msg_type_number_t tn = THREAD_BASIC_INFO_COUNT;
    const mach_port_t self = mach_thread_self();
    thread_info(self, THREAD_BASIC_INFO, reinterpret_cast<thread_info_t>(&t), &tn);
    mach_port_deallocate(mach_task_self(), self);
    return {double(e.syscalls_mach) + double(e.syscalls_unix), double(e.csw), double(t.user_time.seconds) * 1e6 + double(t.user_time.microseconds)};
#else
    rusage u{}; getrusage(RUSAGE_THREAD, &u);
    return {-1, double(u.ru_nvcsw + u.ru_nivcsw), double(u.ru_utime.tv_sec) * 1e6 + double(u.ru_utime.tv_usec)};
#endif
}
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s WORKER VOICE_FIXTURE.clap [PARTS] [SECONDS] [warm]\n", argv[0]); return 2; }
    const int parts = argc > 3 ? std::atoi(argv[3]) : 5;
    const double seconds = argc > 4 ? std::atof(argv[4]) : 5;
    const bool warm = argc > 5 && std::string(argv[5]) == "warm";
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    oscairn::PlacementPolicy policy; policy.worker = argv[1]; policy.chain = true;
    oscairn::setPlacementPolicy(policy);
    setenv("OSCAIRN_AUDIO_HELPERS", "0", 0);
    setenv("OSCAIRN_FIXTURE_VOICE_US", "2", 1); setenv("OSCAIRN_FIXTURE_EFFECT_US", "5", 1);
    oscairn::Graph g{rate, frames};
    std::vector<std::string> names; std::vector<std::pair<std::string, std::string>> wires;
    for(int p = 0; p <= parts; ++p)
    {
        const auto n = std::to_string(p);
        names.push_back("synth" + n); names.push_back("insert" + n); wires.push_back({"synth" + n, "insert" + n});
    }
    const auto groups = oscairn::placementGroups(names, wires, "rest/");
    std::shared_ptr<oscairn::PluginHost> kept;
    std::vector<oscairn::NodeId> played;
    for(int p = 0; p <= parts; ++p)
    {
        if(p == parts && !warm) break;
        const auto n = std::to_string(p);
        setenv("OSCAIRN_FIXTURE_VOICE_BUDGET", "1", 1);
        oscairn::NodeId synth, insert;
        { const oscairn::PlacementScope scope(groups.at("synth" + n)); synth = g.add(argv[2]); }
        unsetenv("OSCAIRN_FIXTURE_VOICE_BUDGET");
        { const oscairn::PlacementScope scope(groups.at("insert" + n)); insert = g.add(argv[2], "org.oscairn.test.costly-effect"); }
        oscairn::NoteZone z; z.channels = uint16_t(1u << p); z.part = uint32_t(p + 1);
        g.connectNotes(oscairn::graphInput, synth, {z});
        g.connectAudio(synth, insert); g.connectAudio(insert, oscairn::graphOutput);
        if(p == parts) { g.commit(); kept = g.release(synth); g.remove(insert); } // the one kept warm: loaded, out of the plan
        else played.insert(played.end(), {synth, insert});
    }
    g.commit();
    g.setVoiceBudget(-1);

    std::vector<double> cpus; double cpuSum = 0; Counts at{}, to{};
    std::thread([&] {
        oscairn::worker::makeRealtime(frames / rate * 1e6, oscairn::worker::priority::engineAudio);
        std::vector<float> left(frames), right(frames); float* out[]{left.data(), right.data()};
        const auto period = std::chrono::nanoseconds(int64_t(1e9 * frames / rate));
        auto next = std::chrono::steady_clock::now();
        auto cpu = [] { timespec t{}; clock_gettime(CLOCK_THREAD_CPUTIME_ID, &t); return double(t.tv_sec) * 1e6 + double(t.tv_nsec) / 1e3; };
        const auto blocks = uint32_t(seconds * rate / frames);
        for(uint32_t i = 0; i < 800 + blocks; ++i)
        {
            if(i == 800) at = counts();
            const double c = cpu();
            oscairn::worker::callbackDeadline = std::chrono::steady_clock::now() + std::chrono::nanoseconds(int64_t(double(period.count()) * (1 - oscairn::worker::mixingReserve)));
            g.process(out, frames, nullptr, nullptr, 0);
            oscairn::worker::callbackDeadline = {};
            if(i >= 800) { const double used = cpu() - c; cpus.push_back(used); cpuSum += used; }
            std::this_thread::sleep_until(next += period);
        }
        to = counts();
    }).join();
    std::sort(cpus.begin(), cpus.end());
    const double n = double(cpus.size());
    size_t processes = 0; for(const auto& h : oscairn::pluginHosts()) if(h.pid > 0 && !h.plugins.empty()) ++processes;
    uint64_t missed = 0; for(const auto id : played) { const auto st = g.host(id).statistics(); missed += st.lateBlocks + st.silentBlocks; }
    std::printf("%d parts%s, %zu processes; engine thread CPU us/block mean %.2f p50 %.2f, user %.2f; per block: syscalls %.2f, context switches %.2f; "
                "blocks missed %llu\n",
                parts, warm ? " + 1 kept warm" : "", processes, cpuSum / n, cpus[cpus.size() / 2], (to.user - at.user) / n,
                to.syscalls < 0 ? -1.0 : (to.syscalls - at.syscalls) / n, (to.switches - at.switches) / n, static_cast<unsigned long long>(missed));
}
