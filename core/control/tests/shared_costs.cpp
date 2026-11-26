// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045: a plugin's measured cost is recorded per sound, so what a sound costs does not depend on
// how many parts shared the instance it was measured on, and admission's verdict for a Performance
// does not depend on what was played before it. One Basic Synth part alone, and three sharing one
// instance, each first at rest and then holding eight notes: the recorded cost per voice is the
// same within the noise of timing (it was the instance's, three parts' voices over one part's, when
// the instance's figure was recorded). A Performance of three such parts, saved with an instance
// each (so they share as it opens), is estimated after each history: the same verdict, against a
// budget the instance's figure did not fit.
//   oscairn-shared-costs-test BASIC-SYNTH.clap
#include <oscairn/session.h>
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <memory>
#include <string>
#include <vector>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr uint32_t frames = 128;
    const char* synth = nullptr;

    std::string read(const fs::path& file) { std::ifstream in(file); return {std::istreambuf_iterator<char>(in), {}}; }
    // A figure of the file's first plugin (the Basic Synth is the only one).
    double figure(const fs::path& home, const char* name)
    {
        const auto text = read(home / "measurements.json");
        const auto at = text.find(std::string("\"") + name + "\":");
        return at == std::string::npos ? -1 : std::strtod(text.c_str() + at + std::string(name).size() + 3, nullptr);
    }
    PartInput keys(uint8_t low, uint8_t high, uint16_t maxNotes = 0) { PartInput k; k.zones = {NoteZone{low, high}}; k.maxNotes = maxNotes; return k; }
    // `n` Basic Synth parts, a third of the keys each (all of them for one), each eight notes from `firsts`.
    void parts(Session& s, uint32_t n, uint16_t maxNotes = 0)
    {
        for(uint32_t i = 0; i < n; ++i)
        {
            const auto node = s.add("synth " + std::to_string(i), synth);
            const auto part = s.addPart("Part " + std::to_string(i), n == 1 ? keys(0, 127, maxNotes) : keys(uint8_t(43 * i), uint8_t(i + 1 == n ? 127 : 43 * i + 42), maxNotes));
            s.insertIntoPart(part, 0, node); s.commit();
        }
    }
    // At rest long enough for the cost at rest to be known (a second of quiet blocks, 100 samples of them), then
    // eight notes held in each of the parts for `blocks` blocks, a service tick each. Every session plays each block
    // in turn, so the figures compared are taken on the same machine at the same moments, not seconds apart.
    void play(const std::vector<std::pair<Session*, uint32_t>>& sessions, int blocks)
    {
        std::array<float, frames> l{}, r{}; float* out[2]{l.data(), r.data()};
        auto block = [&](bool keys) {
            for(const auto& [s, n] : sessions)
            {
                std::vector<kick::clap::Event> notes;
                if(keys) for(uint32_t p = 0; p < n; ++p) for(int k = 0; k < 8; ++k) notes.push_back(kick::clap::Event::note(true, (n == 1 ? 60 : 43 * int(p) + 17) + k, .8));
                s->service(); s->process(out, frames, notes);
            }
        };
        for(int i = 0; i < 1200; ++i) block(false);
        block(true);
        for(int i = 0; i < blocks; ++i) block(false);
    }
    // The Basic Synth's figures after playing one part alone and three parts sharing, each in a data folder of its own.
    struct Figures { double rest, voice, cpu; };
    std::pair<Figures, Figures> measuredTogether(const fs::path& aloneHome, const fs::path& sharedHome)
    {
        std::vector<std::unique_ptr<Session>> sessions;
        const fs::path homes[]{aloneHome, sharedHome};
        const uint32_t counts[]{1, 3};
        for(int i = 0; i < 2; ++i)
        {
            ::setenv("OSCAIRN_DATA_HOME", homes[i].c_str(), 1);
            fs::create_directories(homes[i]);
            sessions.push_back(std::make_unique<Session>(48000, frames));
            sessions[i]->setBudget({.9, 0});
            parts(*sessions[i], counts[i]);
            check(sessions[i]->graphState().shared.size() == (counts[i] > 1 ? counts[i] : 0), std::to_string(counts[i]) + " parts share one instance as they should");
        }
        play({{sessions[0].get(), 1}, {sessions[1].get(), 3}}, 3000);
        for(int i = 0; i < 2; ++i)
        {
            ::setenv("OSCAIRN_DATA_HOME", homes[i].c_str(), 1);
            sessions[i]->saveMeasurementsNow();
        }
        for(int i = 0; i < 2; ++i) { ::setenv("OSCAIRN_DATA_HOME", homes[i].c_str(), 1); sessions[i].reset(); }
        return {{figure(aloneHome, "rest"), figure(aloneHome, "voice"), figure(aloneHome, "cpu")},
                {figure(sharedHome, "rest"), figure(sharedHome, "voice"), figure(sharedHome, "cpu")}};
    }
}

int main(int argc, char** argv)
{
    if(argc != 2) return 2;
    synth = argv[1];
    state_files::TemporaryDirectory temp;
    const auto alone = temp.path / "alone", shared = temp.path / "shared", performance = temp.path / "three";

    // Three parts with an instance each (OSCAIRN_SHARING=cpu), at most eight notes each: opened with OSCAIRN_SHARING=memory, they share one instance.
    {
        ::setenv("OSCAIRN_DATA_HOME", (temp.path / "build").c_str(), 1);
        ::setenv("OSCAIRN_SHARING", "cpu", 1);
        Session s(48000, frames);
        parts(s, 3, 8);
        check(s.graphState().shared.empty(), "built with an instance each");
        s.savePerformance(performance.string());
        ::setenv("OSCAIRN_SHARING", "memory", 1);
    }

    const auto [one, three] = measuredTogether(alone, shared);
    std::printf("one part alone: rest %.3g, per voice %.3g, busiest %.3g; three sharing, per sound: rest %.3g, per voice %.3g, busiest %.3g\n",
                one.rest, one.voice, one.cpu, three.rest, three.voice, three.cpu);
    check(one.voice > 0 && three.voice > 0, "a cost per voice measured both ways");
    // Recorded as the instance's, three parts' 24 voices over the lead part's 8, it was about three times as much.
    const double ratio = three.voice / one.voice;
    check(ratio > 1 / 1.8 && ratio < 1.8, "a voice costs the same whether its part shared the instance: ratio " + std::to_string(ratio));

    // The Performance estimated after each history, against a budget its parts fit with room to spare when costed per sound.
    std::vector<Control::Estimate> estimates;
    for(const auto& home : {alone, shared})
    {
        ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        Session s(48000, frames);
        s.setBudget({std::max(.001, 1.5 * (one.rest + 8 * one.voice)), 0}); // an instance's figure, three parts' voices, came to about twice
        estimates.push_back(s.estimate(performance.string()));
    }
    for(size_t i = 0; i < estimates.size(); ++i)
    {
        std::string costs;
        for(const auto& c : estimates[i].plugins) costs += " " + std::to_string(c.cpu);
        std::printf("estimated after playing %s: fits %d, costs%s\n", i ? "three sharing" : "one alone", int(estimates[i].fits), costs.c_str());
    }
    check(estimates[0].plugins.size() == 3 && estimates[1].plugins.size() == 3, "three parts estimated");
    check(estimates[0].fits == estimates[1].fits, "the same verdict whatever played before");
    check(estimates[0].fits, "and it fits");
    // The costs themselves differ by the instance's own cost at rest, which three sounds share and one has alone (the
    // Basic Synth's is nearly all of its rest): about 1.5 times here, not a verdict's worth.

    if(failures) { std::fprintf(stderr, "%d failures\n", failures); return 1; }
    std::puts("ok: shared costs recorded per sound");
    return 0;
}
