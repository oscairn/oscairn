// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What an archive's install costs the engine's process in memory (the FANTOM-07's out-of-memory
// kill installing 6,140 samples): a library archive of samples whose records are far larger than
// the bound installs under the instrument level's memory policy (RFC 0025), as the engine runs it.
// Its samples are verified one file at a time and copied into the device's packs through a fixed
// buffer, and no file is mapped locked, so the process's locked memory does not grow with them
// (Linux, where the policy locks future mappings) and its peak resident memory grows by a few
// megabytes, not by what the archive holds.
//   oscairn-install-memory-test SAMPLEPACK_CLI TYPES_FOLDER
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "oscairn/plugin_host.h"
#include "../../plugin-host/src/StateFiles.h"
#include "../../engine/tests/sample_source.h"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <thread>
#include <sys/resource.h>

using namespace oscairn;
namespace fs = std::filesystem;
namespace ss = sample_source;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    // A field of a /proc/self file in kB; 0 where there is none.
    uint64_t procKb(const char* file, const char* field)
    {
        std::ifstream in(file);
        for(std::string line; std::getline(in, line);) if(line.rfind(field, 0) == 0) return std::strtoull(line.c_str() + std::strlen(field), nullptr, 10);
        return 0;
    }
    uint64_t statusKb(const char* field) { return procKb("/proc/self/status", field); }
    // Locked memory that is resident: VmLck counts locked address space, a new malloc arena's
    // untouched 64 MiB included.
    uint64_t lockedKb() { return procKb("/proc/self/smaps_rollup", "Locked:"); }
    // The process's peak resident memory in kB: since the last resetPeak on Linux, since its start elsewhere.
    uint64_t peakKb()
    {
#ifdef __linux__
        return statusKb("VmHWM:");
#else
        rusage u{}; getrusage(RUSAGE_SELF, &u);
        return uint64_t(u.ru_maxrss) / 1024; // bytes on macOS
#endif
    }
    void resetPeak() { std::ofstream("/proc/self/clear_refs") << "5" << std::flush; }
}

int main(int argc, char** argv)
{
    if(argc != 3) { std::fprintf(stderr, "usage: %s SAMPLEPACK_CLI TYPES_FOLDER\n", argv[0]); return 2; }
    const std::string cli = argv[1];
    const fs::path types = argv[2];
    try
    {
        state_files::TemporaryDirectory temp;
        // The archive: 40 samples of 2.4 million frames, about 2.6 MB of record each.
        constexpr uint32_t count = 40; constexpr size_t frames = 2400000;
        const auto in = temp.path / "in", stage = temp.path / "big.stage", archive = temp.path / "big.olib";
        fs::create_directories(stage / "Types");
        fs::copy(types, stage / "Types" / "org.oscairn.sample-types", fs::copy_options::recursive);
        Package p; p.id = "big"; p.name = "big"; p.kind = "library";
        uint64_t records = 0, largest = 0;
        std::vector<std::string> ids;
        for(uint32_t i = 0; i < count; ++i)
        {
            const auto s = ss::pack(cli, in, i + 1, frames);
            const auto name = "Samples/" + s.pack.filename().string();
            fs::create_directories(stage / "Samples");
            fs::rename(s.pack, stage / name);
            p.contents.push_back(name); ids.push_back(s.id);
            records += fs::file_size(stage / name); largest = std::max<uint64_t>(largest, fs::file_size(stage / name));
        }
        writePackage(archive, p, stage);
        fs::remove_all(stage);

        ::setenv("OSCAIRN_RELIABILITY", "instrument", 1);
        ::setenv("OSCAIRN_DATA_HOME", (temp.path / "home").c_str(), 1);
        fs::create_directories(temp.path / "home");
        const auto missed = applyMemoryPolicy(0);
#ifdef __linux__
        const bool locking = missed.find("not locked") == std::string::npos; // future mappings locked: the device's case
#else
        const bool locking = false;
#endif
        Session session(48000, 64);
        const uint64_t lockedBefore = lockedKb(), residentBefore = residentBytes() >> 10;
        resetPeak();
        const uint64_t peakBefore = std::max(peakKb(), residentBefore);
        // Locked memory has no peak of its own: read every 2 ms while the install runs.
        std::atomic<bool> done{false};
        uint64_t lockedAfter = lockedBefore;
        std::thread watch([&] { while(!done) { lockedAfter = std::max(lockedAfter, lockedKb()); std::this_thread::sleep_for(std::chrono::milliseconds(2)); } });
        try { session.installArchive(archive.string(), true); } catch(...) { done = true; watch.join(); throw; }
        done = true; watch.join();
        lockedAfter = std::max(lockedAfter, lockedKb());
        const uint64_t peakAfter = peakKb();
        size_t held = 0;
        for(const auto& pack : session.assets().samples().packs()) held += pack.samples;
        check(held == count, "every sample installed: " + std::to_string(held));
        for(const auto& id : ids) check(session.assets().samples().find(id).has_value(), "sample " + id + " is held");

        const auto mb = [](uint64_t kb) { return std::to_string(kb / 1024) + "." + std::to_string(kb % 1024 * 10 / 1024) + " MB"; };
        const uint64_t lockedGrowth = lockedAfter > lockedBefore ? lockedAfter - lockedBefore : 0, peakGrowth = peakAfter > peakBefore ? peakAfter - peakBefore : 0;
        std::printf("%u samples, %s of records (largest %s); memory %s: locked %s -> at most %s (+%s), peak resident %s -> %s (+%s)\n", count, mb(records >> 10).c_str(), mb(largest >> 10).c_str(),
                    locking ? "locked, future mappings too" : missed.empty() ? "locked" : ("not all locked (" + missed + ")").c_str(),
                    mb(lockedBefore).c_str(), mb(lockedAfter).c_str(), mb(lockedGrowth).c_str(), mb(peakBefore).c_str(), mb(peakAfter).c_str(), mb(peakGrowth).c_str());
        // The bound: one sample's file and the copy buffer (1 MiB), with room for the session's own work; the records are about 100 MB.
        constexpr uint64_t boundKb = 16 * 1024;
        check(records >> 10 > 4 * boundKb, "the archive's records are far larger than the bound: " + mb(records >> 10));
        if(locking) check(lockedGrowth <= boundKb, "locked memory does not grow with the archive: +" + mb(lockedGrowth));
        check(peakGrowth <= boundKb, "peak resident memory does not grow with the archive: +" + mb(peakGrowth));
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures == 0) std::printf("ok: an archive's install takes bounded memory\n");
    return failures ? 1 : 0;
}
