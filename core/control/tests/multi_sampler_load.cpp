// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A Performance with several sampler parts on one sample pack still loads, and its memory estimate holds.
// The shape of a stage Performance: four sampler parts, a fifth sampler (an organ's), a synth part and
// twelve effects, all on synthetic data: one pack made by samplepack from generated audio, which a
// sampler instance reads and holds a decoded copy of in its worker's private memory (the fixture reads
// the file; nothing is mapped, as the Oscairn Sampler reads its packs). Parts of one sampler share an
// instance, so one worker and one copy; the estimate of the Performance, which is not open, counts that
// instance in full and each further part at what its sound declares, not an instance each. With the
// parts kept in instances of their own (OSCAIRN_SHARING=cpu) each worker holds a copy and each counts
// in full: a private copy is never counted once for several. Checks that each opens in workers; the
// estimates just said; that neither is below the memory its workers hold; that an allowance under the
// shared one refuses the Performance, saying so, and one above it opens it.
//   oscairn-multi-sampler-load-test WORKER HOLDER.clap SYNTH.clap EFFECT.clap SAMPLEPACK_CLI
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"
#include "../../engine/tests/sample_source.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <thread>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    constexpr uint64_t mib = 1ull << 20, decoded = 12 * mib; // what one sampler instance holds
    constexpr uint64_t slack = 2 * mib; // a worker's memory moves by a few pages between the figure the estimate keeps and the one read after
    const char* const holderId = "org.oscairn.test.pack-holder";
    std::string megabytes(uint64_t bytes) { return std::to_string(bytes / mib) + " MiB"; }
}

int main(int argc, char** argv)
{
    if(argc != 6) { std::fprintf(stderr, "usage: %s WORKER HOLDER SYNTH EFFECT SAMPLEPACK_CLI\n", argv[0]); return 2; }
    const std::string worker = argv[1], holder = argv[2], synth = argv[3], effect = argv[4], cli = argv[5];
    try
    {
        state_files::TemporaryDirectory temp;
        const auto home = temp.path / "home";
        fs::create_directories(home);
        ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        ::setenv("OSCAIRN_PLUGIN_WORKER", worker.c_str(), 1);
        ::setenv("OSCAIRN_WORKER_BUDGET_US", "50000", 1);
        // A pack of synthetic audio (about 3 s, stereo), read by every sampler instance.
        const auto sample = sample_source::pack(cli, temp.path / "pack", 1, 3 * 44100, 2);
        ::setenv("OSCAIRN_HOLDER_PACK", sample.pack.c_str(), 1);
        ::setenv("OSCAIRN_HOLDER_BYTES", std::to_string(decoded).c_str(), 1);

        auto play = [&](Session& s) {
            float l[frames]{}, r[frames]{}; float* out[]{l, r};
            for(int i = 0; i < 600; ++i) { s.service(); s.process(out, frames); std::this_thread::sleep_for(std::chrono::milliseconds(1)); }
        };
        // Four sampler parts (two insertion effects each), the organ's fifth (one), a synth part (three): twelve effects.
        // One sampler alone (two effects) when `samplers` is 1.
        auto build = [&](Session& s, int samplers) {
            int effects = 0;
            auto part = [&](const std::string& name, const std::string& path, const char* id, int inserts) {
                const auto p = s.addPart(name);
                s.insertIntoPart(p, 0, s.add(name, path, id));
                for(int i = 0; i < inserts; ++i) { s.insertIntoPart(p, 1 + i, s.add(name + " effect " + std::to_string(i), effect, "org.kick.test.effect")); ++effects; }
            };
            for(int i = 0; i < samplers; ++i) part(samplers == 1 ? "Sampler" : i < 4 ? "Sampler " + std::to_string(i) : "Organ", holder, holderId, samplers == 1 || i < 4 ? 2 : 1);
            if(samplers > 1) part("Synth", synth, "org.oscairn.basic-synth", 3);
            s.commit();
            return effects;
        };
        auto workers = [&](Session& s) {
            const auto status = s.sessionStatus();
            uint64_t held = 0;
            for(const auto& w : status.workers) held += w.memory;
            check(status.failures.empty(), "no plugin failed");
            return std::make_pair(held, status.workers.size());
        };
        // What a sampler counts at when it plays alone: its instance's whole memory, measured.
        uint64_t alone = 0;
        {
            Session s(rate, frames);
            s.setBudget({.9, 0}); // measuring needs a budget; no memory limit
            build(s, 1);
            play(s);
            for(const auto& w : s.sessionStatus().workers) if(w.name == "Sampler") alone = w.memory;
            s.saveMeasurementsNow();
        }
        check(alone >= decoded, "a sampler alone holds its copy: " + megabytes(alone));
        auto estimate = [&](const fs::path& performance, uint64_t& samplers) {
            Session s(rate, frames);
            s.setBudget({.9, 0});
            const auto e = s.estimate(performance.string());
            samplers = 0;
            for(const auto& c : e.plugins) if(c.plugin == holderId) { samplers += c.memory; check(c.memory >= decoded, "a sampler counts at least what it holds, " + megabytes(c.memory)); }
            return e;
        };
        auto open = [&](const fs::path& performance, uint64_t allowance) {
            Session t(rate, frames);
            t.setBudget({.9, allowance});
            try { t.loadPerformance(performance.string()); } catch(const std::exception& x) { return std::string(x.what()); }
            return std::string();
        };

        // Parts sharing an instance: one worker, one copy.
        const auto shared = temp.path / "shared";
        uint64_t held = 0;
        {
            Session s(rate, frames);
            s.setBudget({.9, 0});
            check(build(s, 5) == 12, "twelve effects");
            play(s);
            const auto [bytes, count] = workers(s);
            held = bytes;
            check(count == 14, "one instance of the sampler, one of the synth, twelve effects: " + std::to_string(count) + " workers");
            check(bytes >= decoded, "the shared instance holds its copy: " + megabytes(bytes));
            s.savePerformance(shared.string());
            s.saveMeasurementsNow();
        }
        uint64_t samplers = 0;
        const auto e = estimate(shared, samplers);
        std::printf("shared: estimate %s, its five samplers %s, one alone %s; the workers hold %s\n", megabytes(e.memory).c_str(), megabytes(samplers).c_str(), megabytes(alone).c_str(), megabytes(held).c_str());
        check(samplers >= alone, "the shared instance counts in full: " + megabytes(samplers) + " against " + megabytes(alone));
        check(samplers < 5 * alone, "five parts sharing one instance count less than five instances: " + megabytes(samplers) + " against " + megabytes(5 * alone));
        check(e.memory + slack >= held, "the estimate is not below what the workers hold: " + megabytes(e.memory) + " against " + megabytes(held));
        // The estimate is what decides: an allowance under it refuses the Performance, one above it opens it.
        const auto refused = open(shared, e.memory / 2);
        check(refused.find("of memory") != std::string::npos, "an allowance of half the estimate refuses it, saying so: " + refused);
        const auto opened = open(shared, e.memory + 128 * mib);
        check(opened.empty(), "an allowance above the estimate opens it: " + opened);

        // The same parts each in an instance of their own: five copies in five workers, each counted.
        ::setenv("OSCAIRN_SHARING", "cpu", 1);
        const auto apart = temp.path / "apart";
        uint64_t heldApart = 0;
        {
            Session s(rate, frames);
            s.setBudget({.9, 0});
            build(s, 5);
            play(s);
            const auto [bytes, count] = workers(s);
            heldApart = bytes;
            check(count == 18, "an instance each: " + std::to_string(count) + " workers");
            check(bytes >= 5 * decoded, "five workers hold five copies: " + megabytes(bytes));
            s.savePerformance(apart.string());
            s.saveMeasurementsNow();
        }
        uint64_t samplersApart = 0;
        const auto f = estimate(apart, samplersApart);
        std::printf("apart: estimate %s, its five samplers %s; the workers hold %s\n", megabytes(f.memory).c_str(), megabytes(samplersApart).c_str(), megabytes(heldApart).c_str());
        check(samplersApart >= 5 * decoded, "five private copies count five: " + megabytes(samplersApart) + " against " + megabytes(5 * decoded));
        check(f.memory + slack >= heldApart, "the estimate is not below what the workers hold: " + megabytes(f.memory) + " against " + megabytes(heldApart));
        check(f.memory > e.memory, "private copies cost more than a shared instance");
        check(open(apart, f.memory + 128 * mib).empty(), "and opens within its estimate");
    }
    catch(const std::exception& x) { std::fprintf(stderr, "FAIL: %s\n", x.what()); return 1; }
    return failures ? 1 : 0;
}
