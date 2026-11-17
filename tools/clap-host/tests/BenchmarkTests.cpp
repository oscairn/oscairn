// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Benchmark.h"
#include "AllocationGuard.h"
#include <clap/ext/params.h>
#include <filesystem>
#include <unistd.h>
#include <cmath>
#include <iostream>
#include <thread>
#ifndef _WIN32
#include <dlfcn.h>
#endif
using namespace kick::clap;
using namespace kick::clap::bench;
static void check(bool value, const char* why) { if (!value) { std::cerr << "FAIL: " << why << '\n'; std::exit(1); } }
int main(int argc, char** argv) {
    check(argc == 4, "fixture paths");
    Timing timing;
    timing.record(500, 48, 48000); timing.record(1000, 48, 48000); timing.record(1200, 48, 48000);
    check(timing.calls == 3 && timing.frames == 144 && timing.deadlineMisses == 1, "deadline uses current buffer duration, equality is not a miss");
    check(timing.maximumUs == 1200 && timing.maximumLoad == 1.2 && timing.percentileLoadUpper(.5) >= 1., "peak and percentile bounds");
    timing.record(11000, 48, 48000);
    check(std::isinf(timing.percentileLoadUpper(1)), "overflow bin never invents a finite percentile upper bound");
    check(quote("a\"\\\n\t") == "\"a\\\"\\\\\\u000a\\u0009\"", "JSON escaping");
    const char* invalid[]{"bench", "--plugin", "x", "--id", "x", "--seconds", "nan"};
    bool rejected = false; try { parse(7, invalid); } catch (...) { rejected = true; }
    check(rejected, "reject nonfinite configuration");
    Options config; config.path = argv[1]; config.id = "org.kick.test.synth";
    config.instances = 3; config.notes = 4; config.parameter = 7; config.seconds = .1; config.warmup = .02; config.snapshotMs = 10;
    Workload workload(config); check(workload.load(), "load multi-instance workload");
    workload.control(0); workload.beginMeasurement();
    forbidAllocation = true; workload.render(nullptr, 0, 128); forbidAllocation = false;
    check(workload.timing().calls == 1, "allocation-free workload rendering");
    // A fresh run verifies warmup exclusion and exact final partial-block length.
    Workload exact(config); check(exact.load(), "second workload"); runOffline(exact);
    check(exact.timing().frames == 4800 && exact.timing().calls == 38 && !exact.faulted(), "duration excludes warmup and includes final partial block");
    auto report = exact.report();
    check(report.find("\"completed\":true") != std::string::npos && report.find("\"device\":null") != std::string::npos, "offline reports cannot imply device/xrun measurements");
    auto load = [](const char* path, const char* id) { try { return oscairn::loadPlugin(path, id, 48000, 16); } catch (...) { return std::shared_ptr<oscairn::PluginHost>(); } };
    const auto slowHost = load(argv[2], config.id.c_str());
    check(slowHost != nullptr, "slow fixture");
    auto& slow = *slowHost;
    std::array<float, 16> left{}, right{}; float* output[]{left.data(), right.data()};
    forbidAllocation = true; check(slow.process(nullptr, 0, output, 2, 16, nullptr, 0, nullptr), "slow process"); forbidAllocation = false;
    check(slow.statistics().deadlineMisses == 1 && slow.statistics().controlSilences == 0, "core DSP deadline accounting");
#ifndef _WIN32
    auto* library = dlopen(argv[3], RTLD_NOW); check(library != nullptr, "held-state fixture diagnostics");
    auto saving = reinterpret_cast<bool(*)()>(dlsym(library, "fixture_saving_state"));
    auto release = reinterpret_cast<void(*)()>(dlsym(library, "fixture_release_state"));
    check(saving && release, "state barrier exports");
    auto heldHost = load(argv[3], config.id.c_str()); check(heldHost != nullptr, "held-state fixture");
    auto& held = *heldHost;
    std::thread audio([&] {
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
        while (!saving() && std::chrono::steady_clock::now() < deadline) std::this_thread::yield();
        check(saving(), "state save reaches barrier");
        forbidAllocation = true;
        // A state save runs on the main thread while audio keeps processing.
        for (int i = 0; i < 10; ++i) {
            left.fill(1); right.fill(1);
            check(held.process(nullptr, 0, output, 2, 16, nullptr, 0, nullptr), "audio keeps processing during a state save");
        }
        forbidAllocation = false; release();
    });
    const auto folder = std::filesystem::temp_directory_path() / ("kick-clap-benchmark-" + std::to_string(getpid()));
    std::filesystem::remove_all(folder);
    bool saved = false; try { held.saveState(folder.string()); saved = true; } catch (...) {}
    check(saved, "release snapshot barrier"); audio.join(); std::filesystem::remove_all(folder);
    check(held.statistics().controlSilences == 0 && held.statistics().processErrors == 0 && held.statistics().deadlineMisses == 0, "a state save causes no control silences, DSP faults or deadline misses");
    heldHost.reset(); dlclose(library);
#endif
    const auto asleepHost = load(argv[1], "org.kick.test.sleep");
    check(asleepHost != nullptr, "sleep fixture");
    auto& asleep = *asleepHost;
    check(asleep.process(nullptr, 0, output, 2, 16, nullptr, 0, nullptr) && asleep.process(nullptr, 0, output, 2, 16, nullptr, 0, nullptr), "sleep calls");
    check(asleep.statistics().idleBlocks == 1 && asleep.statistics().blocks == 1, "idle skips have their own counter");
    std::cout << "PASS: deadline arithmetic, bounded percentile bins, nonfinite input, warmup, multi-instance stress, snapshots, allocation guard and core gate/idle telemetry\n";
}
