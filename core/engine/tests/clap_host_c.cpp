// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The CLAP host's C interface (plugin-host/src/clap_host.h) against the old path: each plugin loaded twice, once
// as kick::clap::Host straight (what ClapHost wrapped call for call) and once through loadPlugin (ClapHost over the
// C interface), driven alike. Everything the engine reads must be the same: the description (descriptor, ports,
// parameters with every hint), the flags, parameter texts, each block's audio bit for bit, its output events,
// parameter values and outputs, the counters, the state saved, and failures as the same exception and message.
// The blocks through the C interface allocate nothing.
//   oscairn-clap-host-c-test PLUGIN.clap...
#include "../../clap-host/tests/AllocationGuard.h"
#include <oscairn/plugin_host.h>
#include <oscairn/transport.h>
#include <kick_clap/Discovery.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <typeinfo>
#include <unistd.h>
#include <vector>

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    bool same(double a, double b) { return std::memcmp(&a, &b, sizeof a) == 0; }
    bool same(const kick::clap::Parameter& a, const kick::clap::Parameter& b)
    {
        return a.id == b.id && a.name == b.name && a.module == b.module && same(a.minimum, b.minimum) && same(a.maximum, b.maximum) &&
               same(a.defaultValue, b.defaultValue) && a.flags == b.flags && a.widget == b.widget && a.priority == b.priority && a.rangeHigh == b.rangeHigh &&
               a.group == b.group && a.role == b.role && a.view == b.view && a.field == b.field && a.index == b.index && a.algorithms == b.algorithms &&
               a.seconds.size() == b.seconds.size() && std::equal(a.seconds.begin(), a.seconds.end(), b.seconds.begin(), [](double x, double y) { return same(x, y); });
    }
    bool same(const std::vector<kick::clap::Port>& a, const std::vector<kick::clap::Port>& b)
    {
        return std::equal(a.begin(), a.end(), b.begin(), b.end(), [](const auto& x, const auto& y) { return x.id == y.id && x.name == y.name && x.channels == y.channels && x.flags == y.flags; });
    }
    bool same(const kick::clap::Descriptor& a, const kick::clap::Descriptor& b)
    {
        return a.path == b.path && a.id == b.id && a.name == b.name && a.vendor == b.vendor && a.version == b.version && a.description == b.description &&
               a.features == b.features && a.presetDiscovery == b.presetDiscovery && a.lanes == b.lanes &&
               std::equal(a.presets.begin(), a.presets.end(), b.presets.begin(), b.presets.end(),
                          [](const auto& x, const auto& y) { return x.name == y.name && x.kind == y.kind && x.location == y.location && x.loadKey == y.loadKey; });
    }
    bool same(const kick::clap::Event& a, const kick::clap::Event& b) { return a.bytes == b.bytes; }
    // The counters that do not depend on time.
    bool same(const kick::clap::Statistics& a, const kick::clap::Statistics& b)
    {
        return a.blocks == b.blocks && a.droppedEvents == b.droppedEvents && a.processErrors == b.processErrors && a.controlSilences == b.controlSilences &&
               a.idleBlocks == b.idleBlocks && a.restarts == b.restarts && a.lateBlocks == b.lateBlocks && a.silentBlocks == b.silentBlocks &&
               a.busyBlocks == b.busyBlocks && a.missPanics == b.missPanics && a.calledBlocks == b.calledBlocks && a.restedBlocks == b.restedBlocks;
    }
    std::vector<uint8_t> file(const std::filesystem::path& p) { std::ifstream in(p, std::ios::binary); return {std::istreambuf_iterator<char>(in), {}}; }
    // What f throws: its type and message.
    template<class F> std::string thrown(F&& f)
    {
        try { f(); return "nothing"; }
        catch(const std::invalid_argument& e) { return std::string("invalid_argument: ") + e.what(); }
        catch(const std::logic_error& e) { return std::string("logic_error: ") + e.what(); }
        catch(const std::runtime_error& e) { return std::string("runtime_error: ") + e.what(); }
    }

    constexpr double rate = 48000; constexpr uint32_t frames = 256, blocks = 400;
    void compare(const std::string& path, const kick::clap::Descriptor& plugin, const std::filesystem::path& scratch)
    {
        const auto& name = plugin.id;
        kick::clap::Host old;
        if(!old.load(plugin, rate, frames))
        {
            check(thrown([&] { oscairn::loadPlugin(path, name, rate, frames); }) == "runtime_error: Cannot load " + name + ": " + old.error(), name + ": its load's failure");
            return;
        }
        old.lockAudioLayout(); old.service(); // as loadPlugin did
        const auto now = oscairn::loadPlugin(path, name, rate, frames);

        check(same(old.descriptor(), now->descriptor()), name + ": descriptor");
        check(same(old.inputPorts(), now->inputPorts()) && same(old.outputPorts(), now->outputPorts()), name + ": audio ports");
        const auto& p = old.parameters(); const auto& q = now->parameters();
        check(p.size() == q.size() && std::equal(p.begin(), p.end(), q.begin(), q.end(), [](const auto& a, const auto& b) { return same(a, b); }), name + ": parameters");
        check(old.metadataRevision() == now->parameterRevision(), name + ": revision");
        check(old.noteInputPorts() == now->noteInputPorts() && old.noteOutputPorts() == now->noteOutputPorts(), name + ": note ports");
        for(uint32_t i = 0; i < old.noteInputPorts(); ++i) check(old.usesClapNotes(i) == now->usesClapNotes(i), name + ": note dialect");
        check(old.takesSysex() == now->takesSysex() && old.emitsSysex() == now->emitsSysex() && old.supportsState() == now->supportsState() &&
              old.soundContextCapacity() == now->soundContextCapacity() && old.voiceBudget() == now->voiceBudget() && old.honorsVolume() == now->honorsVolume() &&
              old.ringsOut() == now->ringsOut() && old.preparesSoundSnapshots() == now->preparesSnapshots() && old.savesSounds() == now->savesSounds() &&
              old.hasGui() == now->hasWindow(), name + ": flags");
        if(old.soundContextCapacity()) check(old.soundContexts().size() == now->soundContexts().size(), name + ": sound contexts");
        for(const auto& a : p)
            for(const double v : {a.minimum, a.defaultValue, a.maximum, 1e308})
                check(old.parameterText(a.id, v) == now->parameterText(a.id, v), name + ": text of " + a.name);

        // The same blocks: notes, a playing transport, a parameter change halfway.
        uint32_t ni = 0, no = 0;
        for(const auto& port : old.inputPorts()) ni += port.channels;
        for(const auto& port : old.outputPorts()) no += port.channels;
        std::vector<std::vector<float>> input(ni, std::vector<float>(frames)), outA(no, std::vector<float>(frames)), outB(no, std::vector<float>(frames));
        std::vector<const float*> in; std::vector<float*> a, b;
        for(auto& c : input) in.push_back(c.data());
        for(auto& c : outA) a.push_back(c.data());
        for(auto& c : outB) b.push_back(c.data());
        oscairn::Transport transport(rate, 120); transport.setPlaying(true);
        const auto writable = std::find_if(p.begin(), p.end(), [](const auto& x) { return !(x.flags & CLAP_PARAM_IS_READONLY); });
        bool audio = true, events = true, values = true, flags = true, outputs = true;
        for(uint32_t block = 0; block < blocks; ++block)
        {
            for(uint32_t c = 0; c < ni; ++c) for(uint32_t f = 0; f < frames; ++f) input[c][f] = float(std::sin(0.01 * (block * frames + f) + c));
            std::vector<kick::clap::Event> sent;
            if(block % 20 == 1) sent.push_back(kick::clap::Event::note(true, 48 + int(block % 24), 0.8, 7, 0, 0, int(block)));
            if(block % 20 == 9) sent.push_back(kick::clap::Event::note(false, 48 + int((block - 8) % 24), 0, 3, 0, 0, int(block - 8)));
            if(block == blocks / 2 && writable != p.end())
            {
                const double v = writable->minimum + (writable->maximum - writable->minimum) / 3;
                check(old.setParameter(writable->id, v) == now->setParameter(writable->id, v), name + ": set parameter");
                old.service(); now->service();
            }
            const auto t = transport.current();
            const bool okA = old.process(in.data(), ni, a.data(), no, frames, sent.data(), uint32_t(sent.size()), &t);
            forbidAllocation = true;
            const bool okB = now->process(in.data(), ni, b.data(), no, frames, sent.data(), uint32_t(sent.size()), &t);
            uint32_t countA = 0, countB = 0;
            const auto* ea = old.outputEvents(countA); const auto* eb = now->outputEvents(countB);
            const bool asleep = now->asleep(), wasReset = now->wasReset(), restful = now->restful();
            const auto reports = now->parameterReports();
            forbidAllocation = false;
            transport.advance(frames);
            for(uint32_t c = 0; c < no; ++c) audio = audio && std::memcmp(outA[c].data(), outB[c].data(), frames * sizeof(float)) == 0;
            audio = audio && okA == okB;
            events = events && countA == countB && std::equal(ea, ea + countA, eb, [](const auto& x, const auto& y) { return same(x, y); });
            flags = flags && asleep == old.asleep() && wasReset == old.wasReset() && restful == old.restful() && reports == old.parameterReports();
            for(const auto& x : p) values = values && same(old.parameterValue(x.id), now->parameterValue(x.id));
            if(block % 50 == 0)
            {
                old.service(); now->service();
                kick::clap::Event x, y; bool more = true;
                while(more) { const bool m = old.popOutput(x), n = now->popOutput(y); outputs = outputs && m == n && (!m || same(x, y)); more = m && n; }
                outputs = outputs && old.outputEventDrops() == now->outputEventDrops();
            }
        }
        check(audio, name + ": audio, bit for bit"); check(events, name + ": output events"); check(flags, name + ": sleep, reset and reports");
        check(values, name + ": parameter values"); check(outputs, name + ": parameter outputs");
        check(same(old.statistics(), now->statistics()), name + ": counters");

        // State: the same bytes saved, and loading them back.
        if(old.supportsState())
        {
            std::vector<uint8_t> bytes; check(old.saveState(bytes), name + ": old state");
            const auto dir = scratch / (name + ".state");
            now->saveState(dir.string());
            const auto saved = file(dir / "state.clap");
            check(saved == bytes, name + ": state bytes");
            check(thrown([&] { now->loadState(dir.string()); }) == "nothing", name + ": state loads back");
            // Failures as before: the same exception and message ClapHost threw.
            std::string expected;
            try { (void)std::filesystem::canonical(scratch / "missing"); } catch(const std::exception& e) { expected = std::string("runtime_error: ") + e.what(); }
            check(thrown([&] { now->loadState((scratch / "missing").string()); }) == expected, name + ": a missing state's failure");
            check(thrown([&] { now->saveState(""); }) == "invalid_argument: Invalid state directory", name + ": an empty directory's failure");
            check(thrown([&] { now->saveState(dir.string()); }) == "runtime_error: State destination already exists: " + std::filesystem::absolute(dir).lexically_normal().string(),
                  name + ": an existing directory's failure");
        }
        if(!old.loadPreset({"", 1, "x", "y"}))
            check(thrown([&] { now->loadPreset("clap-preset:1:x#y"); }) == "runtime_error: " + old.error(), name + ": a preset's failure");
    }
}

int main(int argc, char** argv)
{
    if(argc < 2) { std::fprintf(stderr, "usage: oscairn-clap-host-c-test PLUGIN.clap...\n"); return 2; }
    oscairn::setPlacementPolicy({"", oscairn::Placement::Engine}); // every plugin in this process
    const auto scratch = std::filesystem::temp_directory_path() / ("oscairn-clap-host-c-" + std::to_string(::getpid()));
    std::filesystem::create_directories(scratch);
    for(int i = 1; i < argc; ++i)
    {
        const auto scan = kick::clap::inspect(argv[i]);
        check(!scan.plugins.empty(), std::string("plugins in ") + argv[i]);
        for(const auto& plugin : scan.plugins) compare(argv[i], plugin, scratch);
        check(thrown([&] { oscairn::loadPlugin(argv[i], "no.such.plugin", rate, frames); }) == std::string("runtime_error: No matching CLAP plugin in ") + argv[i], "a missing plugin's failure");
    }
    std::filesystem::remove_all(scratch);
    if(failures) return 1;
    std::puts("clap host C interface: same as kick::clap::Host straight");
}
