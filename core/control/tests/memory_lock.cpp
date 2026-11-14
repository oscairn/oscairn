// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025: what instrument level locks. Threads made without a size get threadStack, all of it
// locked; a ServiceThread (off the audio path) keeps locked only what it touches. And locking still
// does its job: across a render of a Performance in workers, notes playing, neither the thread
// rendering (an audio thread's stack, threadStack) nor any worker's audio thread takes a page fault.
// Linux only, where memory can be locked (RLIMIT_MEMLOCK unlimited or high enough); elsewhere it says so.
//   oscairn-memory-lock-test WORKER FIXTURE.clap BASIC-SYNTH.clap
#include <oscairn/session.h>
#include <oscairn/plugin_host.h>
#include "../../plugin-host/src/StateFiles.h"
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <sstream>
#include <string>
#include <thread>
#include <vector>
#ifdef __linux__
#include <dlfcn.h>
#include <pthread.h>
#include <sys/resource.h>
#endif

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
#ifdef __linux__
    // The calling thread's stack: its mapping's Rss and Locked, in kB, from /proc/self/smaps.
    std::pair<uint64_t, uint64_t> stackKb()
    {
        pthread_attr_t a; void* low = nullptr; size_t size = 0;
        pthread_getattr_np(pthread_self(), &a); pthread_attr_getstack(&a, &low, &size); pthread_attr_destroy(&a);
        const auto at = uintptr_t(low) + size - 1;
        std::ifstream smaps("/proc/self/smaps"); std::string line; bool in = false; uint64_t rss = 0, locked = 0;
        while(std::getline(smaps, line))
        {
            unsigned long from = 0, to = 0;
            if(std::sscanf(line.c_str(), "%lx-%lx ", &from, &to) == 2 && line.find(':') > line.find(' ')) { in = at >= from && at < to; continue; }
            if(!in) continue;
            if(line.rfind("Rss:", 0) == 0) rss = std::strtoull(line.c_str() + 4, nullptr, 10);
            if(line.rfind("Locked:", 0) == 0) locked = std::strtoull(line.c_str() + 7, nullptr, 10);
        }
        return {rss, locked};
    }
    uint64_t faults() { rusage u{}; getrusage(RUSAGE_THREAD, &u); return uint64_t(u.ru_minflt + u.ru_majflt); }
#endif
}

int main(int argc, char** argv)
{
    if(argc != 4) return 2;
#ifndef __linux__
    (void)argv;
    std::printf("memory locking is Linux only: nothing to check here\n");
    return 0;
#else
    setenv("OSCAIRN_PLUGIN_WORKER", argv[1], 1);
    setenv("OSCAIRN_RELIABILITY", "instrument", 1);
    oscairn::state_files::TemporaryDirectory home; setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1);
    // A crypto library mapped before the lock, as the engine's libcurl maps one, where the system has one.
    void* crypto = nullptr;
    for(const char* name : {"libcrypto.so.3", "libcrypto.so.1.1", "libgnutls.so.30"}) if(!crypto) crypto = dlopen(name, RTLD_NOW | RTLD_LOCAL);
    const auto missed = oscairn::applyMemoryPolicy(0);
    if(missed.find("not locked") != std::string::npos) { std::printf("memory not locked here (%s): nothing to check\n", missed.c_str()); return 0; }

    pthread_attr_t defaults; size_t stack = 0;
    check(pthread_getattr_default_np(&defaults) == 0 && pthread_attr_getstacksize(&defaults, &stack) == 0 && stack == oscairn::threadStack,
          "threads made after it get threadStack: " + std::to_string(stack));
    pthread_attr_destroy(&defaults);
    std::pair<uint64_t, uint64_t> plain, service;
    std::thread([&] { plain = stackKb(); }).join();
    oscairn::ServiceThread s([&] { service = stackKb(); });
    s.join();
    std::printf("a plain thread's stack: %llu kB resident, %llu locked; a service thread's: %llu resident, %llu locked\n",
                (unsigned long long)plain.first, (unsigned long long)plain.second, (unsigned long long)service.first, (unsigned long long)service.second);
    check(plain.second * 1024 + 64 * 1024 >= oscairn::threadStack, "a plain thread's stack (an audio thread's) is locked whole");
    check(service.first <= 64 && service.second <= 64, "a service thread's stack holds only what it touched");

    // Libraries off the audio path (TLS, crypto: libcurl's) are not locked, where this process maps them.
    {
        std::ifstream smaps("/proc/self/smaps"); std::string line; bool crypto = false; uint64_t locked = 0, mapped = 0;
        while(std::getline(smaps, line))
        {
            unsigned long from = 0, to = 0;
            if(std::sscanf(line.c_str(), "%lx-%lx ", &from, &to) == 2 && line.find(':') > line.find(' ')) { crypto = line.find("/libcrypto.") != std::string::npos || line.find("/libgnutls.") != std::string::npos; if(crypto) ++mapped; continue; }
            if(crypto && line.rfind("Locked:", 0) == 0) locked += std::strtoull(line.c_str() + 7, nullptr, 10);
        }
        std::printf("TLS and crypto libraries: %llu mappings, %llu kB locked\n", (unsigned long long)mapped, (unsigned long long)locked);
        check(locked == 0 && (mapped > 0 || !crypto), "TLS and crypto libraries are not locked: " + std::to_string(locked) + " kB");
    }

    // A Performance's shape in workers: two instruments, one through an effect.
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    oscairn::Session session(rate, frames);
    for(int p = 0; p < 2; ++p)
    {
        const auto part = session.addPart("Part " + std::to_string(p + 1));
        session.insertIntoPart(part, 0, session.add("Synth " + std::to_string(p + 1), argv[3], "org.oscairn.basic-synth"));
        if(p) session.insertIntoPart(part, 1, session.add("Effect", argv[2], "org.kick.test.effect"));
    }
    session.commit();
    uint64_t rendered = 0;
    std::thread audio([&] {
        float l[frames], r[frames]; float* out[]{l, r};
        std::vector<kick::clap::Event> none, on{kick::clap::Event::note(true, 60, .8), kick::clap::Event::note(true, 64, .8)},
            off{kick::clap::Event::note(false, 60, 0), kick::clap::Event::note(false, 64, 0)};
        // Each 100 blocks: notes on, the same notes struck again while held, then off.
        auto play = [&](int blocks) {
            for(int i = 0; i < blocks; ++i)
            {
                session.process(out, frames, i % 100 == 0 || i % 100 == 25 ? on : i % 100 == 50 ? off : none);
                std::this_thread::sleep_for(std::chrono::microseconds(500));
            }
        };
        // From the first block: each plugin's first blocks and notes are the audio path too.
        session.engineTiming(true);
        const auto before = faults();
        play(2000);
        rendered = faults() - before;
    });
    audio.join();
    const auto t = session.engineTiming(false);
    uint64_t workers = 0;
    for(const auto& w : t.workers) workers += w.minorFaults + w.majorFaults;
    std::printf("page faults across 2000 blocks: %llu on the rendering thread, %llu on %zu workers' audio threads\n", (unsigned long long)rendered, (unsigned long long)workers, t.workers.size());
    check(t.workers.size() >= 2, "the plugins play in workers (the two Synths in one): " + std::to_string(t.workers.size()));
    check(rendered == 0, "no page fault on the rendering thread: " + std::to_string(rendered));
    check(workers == 0, "no page fault on a worker's audio thread: " + std::to_string(workers));
    if(failures == 0) std::printf("ok: what is locked, and no page fault on the audio path\n");
    return failures ? 1 : 0;
#endif
}
