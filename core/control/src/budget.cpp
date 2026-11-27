// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025: resource budgets. Each plugin build's cost is measured on this device while it plays
// (its share of each block's time, its memory) and kept in the data folder, so a Performance can
// be checked against the device's budget before it loads.
#include "oscairn/session.h"
#include "oscairn/assets.h"
#include "oscairn/library.h"
#include "oscairn/package.h"
#include "oscairn/sound_context.h"
#include "oscairn/realtime.h"
#include "../../plugin-host/src/StateFiles.h"
#include "sheredom/json.h"
#include "JsonInput.h"
#include <algorithm>
#include <bit>
#include <cmath>
#include <functional>
#include <cstdio>
#include <fstream>
#include <memory>
#include <mutex>
#include <optional>
#include <ranges>
#include <set>
#ifdef __linux__
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <poll.h>
#include <unistd.h>
#elif defined(__APPLE__)
#include <atomic>
#include <dispatch/dispatch.h>
#endif

namespace oscairn
{
    namespace
    {
        namespace fs = std::filesystem;
        // What a plugin never measured here counts as: small, so a desktop with many new plugins
        // is not refused, and marked, so the user knows it is a guess. Its memory, without its own
        // oscairn.memory figure or its package's estimate: a worker's floor and its build's code,
        // which a locked worker holds resident whole. The floor: a fixture plugin's worker holds 8.0 to
        // 9.1 MB resident in the Linux container (aarch64, locked; the figure a measurement here
        // takes), the smallest workers measured on the FANTOM-07 7 MB (5.2 to 5.7 MB PSS).
        constexpr double unmeasuredCpu = .02;
        constexpr uint64_t workerFloor = 8ull << 20;
        // The measured cost (Session::measure), a share of the block period (frames / sample rate): at rest
        // and per voice, a running mean of the plugin's CPU time per block over each service tick's blocks,
        // each tick weighing its blocks over restBlocks (about 10 s of blocks at 128 frames and 48 kHz, and at
        // least 1/n while n is small); cpu, the busiest sustained load, a maximum of a short mean of the median
        // of the last few ticks (BusiestLoad: a spike shorter than half its window moves it not at all) falling
        // with a time constant of fallBlocks (a few minutes). Not recorded: a tick whose blocks took longer than
        // the block period on average (the audio missed, or the time was not the plugin's work: a share above 1
        // cannot be true of a block that was on time), a tick spanning more than longTickSeconds (the control
        // thread held by a load, so its blocks are the load's), a node's first warmupBlocks blocks (its load's
        // first blocks) and the first restartSeconds after its worker restarted. enoughSamples at rest before
        // the rest is taken off a voiced sample; a value loaded from the file counts as seedSamples, so what is
        // played now outweighs it quickly. At rest: no note held and the output silent for quietSeconds (a
        // release tail is not at rest). A figure is certain (admission takes it as it is) once certainBlocks
        // blocks were measured.
        constexpr double restBlocks = 4000, fallBlocks = 60000, quietSeconds = .5, longTickSeconds = .25, restartSeconds = 2;
        constexpr uint32_t enoughSamples = 100, seedSamples = 4, warmupBlocks = 32, certainBlocks = 8000;
        constexpr int measurementsVersion = 6; // 1: the old estimator's, a high-water mark of wall times, discarded; 2: bounded by histogram buckets' tops; 3: a shared instance's CPU all its sounds'; 4: one block's wall time a tick, a shared instance's unbounded and rest taken during release tails (2 to 4's CPU discarded); 5: a worker's memory its whole resident memory on Linux, the code every worker shares counted in each (memory discarded, CPU kept)
        fs::path measurementsFile() { return dataHome() / "measurements.json"; }
        // The worker binary's code, which every worker holds and shares: counted once, beside the plugins, since a
        // worker's measurement leaves it out (workerOwnBytes).
        uint64_t workerCode()
        {
            static std::mutex mutex; static std::string program; static uint64_t bytes = 0; // read once per worker program
            std::lock_guard lock(mutex);
            if(const auto& w = placementPolicy().worker; w != program) { program = w; bytes = w.empty() ? 0 : workerCodeBytes(w); }
            return bytes;
        }
        std::string percent(double share) { char text[32]; std::snprintf(text, sizeof(text), "%.0f%%", share * 100); return text; }
    }

    namespace
    {
        // What a plugin declared at its last activation (oscairn.memory); none where a worker is down.
        std::optional<oscairn_memory_t> declaredBy(const PluginHost& host)
        {
            try { return host.declaredMemory(); } catch(const std::exception&) { return {}; }
        }
        uint64_t total(const oscairn_memory_t& m) { return m.shared_bytes + m.context_bytes + m.asset_bytes; }

        // A build's code: a file's size, or a bundle's binaries' (a macOS bundle's Contents/MacOS, an
        // LV2 bundle's shared objects), not its other files (presets, samples), which are not mapped.
        uint64_t codeBytes(const std::string& path)
        {
            std::error_code e;
            if(path.empty()) return 0;
            if(!fs::is_directory(path, e)) { const auto n = fs::file_size(path, e); return e ? 0 : uint64_t(n); }
            uint64_t n = 0;
            for(fs::recursive_directory_iterator it(path, e), end; !e && it != end; it.increment(e))
            {
                const auto& f = it->path();
                if(!it->is_regular_file(e) || !(f.parent_path().filename() == "MacOS" || f.extension() == ".so" || f.extension() == ".dylib")) continue;
                if(const auto size = fs::file_size(f, e); !e) n += size;
            }
            return n;
        }

        // A key's plugin id with the '@' ("<id>@") and its version ("<id>@<version>[ <digest>][#<declared>]").
        std::string idOf(const std::string& key) { return key.substr(0, key.find('@') + 1); }
        std::string versionOf(const std::string& key) { const auto rest = key.substr(std::min(key.size(), key.find('@') + 1)); return rest.substr(0, rest.find_first_of(" #")); }

        // A plugin never measured here (its key "<id>@<version>..."): its memory what it declared
        // through oscairn.memory when it last activated here (`declared`), else its package's
        // estimate for this platform, kept beside its build in the store (estimate.json, RFC 0025
        // step 5), else the worker's floor and its build's code (`path`); its CPU the package's
        // estimate, else the default. Still marked unmeasured.
        Control::Cost declaredCost(const std::string& key, const std::string& name, const std::string& plugin, const std::string& path, uint64_t declared)
        {
            if(key.size() > plugin.size() && key.compare(0, plugin.size(), plugin) == 0 && key[plugin.size()] == '@')
            {
                const auto rest = key.substr(plugin.size() + 1);
                const auto version = rest.substr(0, rest.find_first_of(" #"));
                for(const char* format : {"clap", "lv2"})
                    try
                    {
                        const auto bytes = state_files::read(dataHome() / "Plugins" / format / pluginFolderName(plugin, version) / platform() / "estimate.json", 4096);
                        const std::unique_ptr<json_value_s, decltype(&std::free)> doc{json_parse(bytes.data(), bytes.size()), &std::free};
                        auto number = [&](const char* k) {
                            const auto* v = detail::member(doc.get(), k);
                            return v && v->type == json_type_number ? std::strtod(static_cast<const json_number_s*>(v->payload)->number, nullptr) : -1.0;
                        };
                        const double cpu = number("cpu"), memory = number("memory");
                        if(cpu >= 0 && cpu <= 64 && memory >= 0 && memory <= double(1ull << 40)) return {name, plugin, cpu, declared ? declared : uint64_t(memory), false, {}, true, cpu}; // its package's figure, rest and all
                    }
                    catch(const std::exception&) {} // none for that format
            }
            // not certain: the default guess, which never refuses for its CPU (estimateOf)
            return {name, plugin, unmeasuredCpu, declared ? declared : workerFloor + codeBytes(path), false, {}, false};
        }
    }

    // A plugin build, by its version and the digest of its files (so a rebuild without a new version
    // is measured again), and what it declared for its state: a plugin whose modes need very
    // different memory (a reverb's algorithms) is measured per declaration, not as one figure, the largest.
    std::string Session::pluginKey(const Graph& graph, NodeId node) const
    {
        const auto& host = graph.host(node);
        const auto& d = host.descriptor();
        const auto declared = declaredBy(host);
        const auto digest = buildDigest(d.path);
        return d.id + "@" + d.version + (digest.empty() ? std::string() : " " + digest) + (declared ? "#" + std::to_string(total(*declared)) : std::string());
    }

    // Measured: at rest, plus its cost per voice for as many notes as its part may play at once;
    // with no limit, or before its voices are measured, the busiest block seen. A build asked for
    // without a declaration (a Performance not open, its part's own sound not measured: estimate)
    // counts its memory at the most of what its declarations were measured at, and its CPU at the
    // median of theirs, figure by figure: one sound's extreme (pianos stacked, 24 voices each) does
    // not price every other sound of the plugin; one without a digest (not here), over that version's builds.
    std::optional<Session::Measurement> Session::measurementFor(const std::string& key) const
    {
        if(const auto it = measured_.find(key); it != measured_.end()) return it->second;
        std::optional<Measurement> m;
        std::vector<double> cpu, rest, voice, voices;
        if(key.find('#') == std::string::npos)
            for(auto it = measured_.lower_bound(key); it != measured_.end() && it->first.compare(0, key.size(), key) == 0; ++it)
            {
                const char next = it->first.size() > key.size() ? it->first[key.size()] : 0;
                if(next != '#' && (next != ' ' || key.find(' ') != std::string::npos)) continue;
                const auto& x = it->second;
                if(x.cpu > 0 || x.rest > 0 || x.voice > 0) { cpu.push_back(x.cpu); rest.push_back(x.rest); voice.push_back(x.voice); voices.push_back(x.voices); } // its CPU forgotten: not a figure
                if(!m) { m = x; continue; }
                const auto contexts = x.contexts && (!m->contexts || x.contexts < m->contexts) ? x.contexts : m->contexts; // the fewest seen: never more sharing than was
                const auto sounds = x.sounds && (!m->sounds || x.sounds < m->sounds) ? x.sounds : m->sounds; // the fewest, with the most memory: never more in it than was
                m = Measurement{0, 0, 0, 0, std::max(m->memory, x.memory), std::max(m->samples, x.samples), std::max(m->declared, x.declared), contexts, std::max(m->perContext, x.perContext), sounds, std::max(m->day, x.day)};
            }
        if(m && key.find('#') == std::string::npos)
        {
            auto median = [](std::vector<double> v) { if(v.empty()) return 0.0; std::nth_element(v.begin(), v.begin() + (v.size() - 1) / 2, v.end()); return v[(v.size() - 1) / 2]; };
            m->cpu = median(cpu); m->rest = median(rest); m->voice = median(voice); m->voices = uint32_t(median(voices));
        }
        if(m || key.find('@') == std::string::npos) return m;
        // No record of this version (RFC 0025): the newest OLDER version's of the same plugin, merged the same way, marked `earlier`
        // until this version has a record of its own. Never a newer one's. shortcut: a key with a declaration borrows the older
        // version's merge over all its declarations, not its same declaration; refine if a plugin's declarations differ by version.
        const auto id = idOf(key), mine = versionOf(key);
        std::string best;
        for(auto it = measured_.lower_bound(id); it != measured_.end() && it->first.compare(0, id.size(), id) == 0; ++it)
            if(const auto v = versionOf(it->first); assets::older(v, mine) && (best.empty() || assets::older(best, v))) best = v;
        if(best.empty()) return m;
        if(m = measurementFor(id + best)) m->earlier = true;
        return m;
    }

    Control::Cost Session::costOf(const std::string& key, const std::string& name, const std::string& plugin, uint32_t limit, const std::string& path) const
    {
        const auto found = measurementFor(key);
        if(!found) return declaredCost(key, name, plugin, path, 0);
        const auto& m = *found;
        auto mark = [&](Control::Cost c) { c.earlier = m.earlier; return c; };
        // Its CPU forgotten, or none sampled yet: the guess, with the memory as measured.
        if(m.cpu <= 0 && m.rest <= 0 && m.voice <= 0)
        {
            const auto guess = declaredCost(key, name, plugin, path, m.declared);
            return mark({name, plugin, guess.cpu, m.memory ? m.memory : guess.memory, m.memory > 0, {}, true, guess.rest});
        }
        // The busiest block seen wears off while the sound is not played (it falls only while an instance plays it: measure), halving
        // each week unused, never below what its voices were measured to cost: one stacked test's figure does not live on for good.
        double busiest = m.cpu;
        if(const auto today = uint32_t(std::chrono::duration_cast<std::chrono::hours>(std::chrono::system_clock::now().time_since_epoch()).count() / 24); m.day && today > m.day + 7)
            busiest = std::max(m.rest + m.voice * m.voices, m.cpu * std::exp2(-double(today - m.day) / 7));
        const double cpu = limit && m.voice > 0 ? m.rest + m.voice * limit : busiest;
        // Its memory not measured (only an instance playing one sound records it): the guess, not 0.
        return mark({name, plugin, cpu, m.memory ? m.memory : declaredCost(key, name, plugin, path, m.declared).memory, true, {}, m.samples >= certainBlocks, m.rest > 0 ? std::min(m.rest, cpu) : cpu}); // rest not measured: all of it
    }

    // Memory: the larger of the measurement and the declaration (RFC 0025: never the plugin's word
    // alone); a plugin not yet measured, its declaration instead of the default. An effect context
    // made from another counts its context bytes, from oscairn.memory or else effect contexts'
    // resources, since its engine's shared bytes are counted with the engine.
    // RFC 0025, a further part of a shared instance (an open Performance's and one not open alike): nothing while its
    // instance's measured memory held as many sounds (its parts measured together), then what a sound context was measured
    // to add, else the plugin's word times 2.5 (plugins under-declare: the Sampler's 324 KiB context measured 433 to 753
    // KiB); never above an instance of its own as measured, never below its word.
    std::optional<uint64_t> Session::furtherPart(const std::optional<Measurement>& m, uint32_t rank, uint64_t declared)
    {
        if(m && m->memory && rank < m->sounds) return 0;
        if(!declared) return std::nullopt;
        const auto guess = m && m->perContext ? m->perContext : declared * 5 / 2;
        return std::max(declared, m && m->memory ? std::min(m->memory, guess) : guess);
    }

    Control::Cost Session::nodeCost(const Graph& graph, NodeId node, const std::string& name, uint32_t limit) const
    {
        const auto& host = graph.host(node);
        // RFC 0045: the CPU measured is a sound's (Session::measure), so each node sharing an instance counts its own.
        const auto key = pluginKey(graph, node);
        auto c = costOf(key, name, host.descriptor().id, limit, host.descriptor().path);
        const auto declared = declaredBy(host);
        // A part sharing an instance past its first (the lowest channel, whose node counts the instance as measure() reads it):
        // as a Performance not open counts it, not its context's and the instance's assets' declaration on top of the instance.
        if(const auto* context = dynamic_cast<const SoundContext*>(&host); context && !context->leads())
        {
            const auto rank = uint32_t(std::popcount(context->engine()->channels & ((1u << context->channel()) - 1)));
            const auto m = measurementFor(key);
            if(const auto x = furtherPart(m, rank, declared ? total(*declared) : m ? m->declared : 0)) c.memory = *x;
            return c;
        }
        std::optional<oscairn_effect_resources_t> resources;
        if(!declared) try { resources = host.effectResources(); } catch(const std::exception&) {}
        if(graph.effectSource(node) != node)
        {
            if(declared) c.memory = declared->context_bytes + declared->asset_bytes;
            else if(resources) c.memory = resources->context_bytes;
            return c;
        }
        const uint64_t own = declared ? total(*declared) : resources ? resources->shared_bytes + resources->context_bytes : 0;
        if(declared || resources) c.memory = c.measured ? std::max(c.memory, own) : own;
        return c;
    }

    Control::Cost Session::ringingCost(const Graph& graph, NodeId node, const std::string& name) const
    {
        auto c = nodeCost(graph, node, name);
        const auto& host = graph.host(node);
        if(!host.noteInputPorts()) return c; // an effect plays what reaches it: as measured
        const auto voices = graph.sounding(node);
        if(voices == UINT32_MAX) return c; // just set retiring, not yet counted
        if(!voices) { c.cpu = c.rest > 0 ? std::min(c.rest, c.cpu) : c.cpu; return c; }
        if(const auto m = measurementFor(pluginKey(graph, node)); m && m->voice > 0)
            c.cpu = std::min(c.cpu, (c.rest > 0 ? std::min(c.rest, c.cpu) : 0) + m->voice * voices);
        return c;
    }

    uint64_t Session::memoryRoom(const Graph& graph, const std::vector<NodeId>& beside) const
    {
        const auto ceiling = budget_.cpu > 0 ? memoryCeiling() : 0;
        if(!ceiling) return 0;
        uint64_t used = recorder_.committedBytes(); // recording's rings are committed (RFC 0025)
        for(const auto n : beside) if(graph.contains(n)) used += nodeCost(graph, n).memory;
        return used < ceiling ? ceiling - used : 1; // 1 byte: nothing that declares fits
    }

    uint64_t Session::memoryRoom() const
    {
        std::vector<NodeId> open;
        for(const auto& [node, name] : nodes_) open.push_back(node);
        return memoryRoom(graph_, open);
    }

    uint32_t Session::noteLimit(NodeId node) const
    {
        for(const auto& p : parts_) if(nodesOf(p).count(node)) return p.input.maxNotes;
        return 0;
    }

    void Session::loadMeasurements()
    {
        if(measurementsLoaded_) return;
        measurementsLoaded_ = true;
        try
        {
            const auto bytes = state_files::read(measurementsFile(), 4 * 1024 * 1024);
            const std::unique_ptr<json_value_s, decltype(&std::free)> doc{json_parse(bytes.data(), bytes.size()), &std::free};
            const auto* version = detail::member(doc.get(), "version");
            const double read = version && version->type == json_type_number ? std::strtod(static_cast<const json_number_s*>(version->payload)->number, nullptr) : 0;
            if(read != measurementsVersion && read != 5 && read != 4 && read != 3 && read != 2) return;
            // Versions 2 to 4's CPU figures were overstated, and before 6 a worker's memory counted the code all workers
            // share (see measurementsVersion): measured again, each in the meantime the guess for one not measured.
            const bool memoryOnly = read != measurementsVersion && read != 5, keepMemory = read == measurementsVersion;
            const auto* plugins = detail::member(doc.get(), "plugins");
            if(!plugins || plugins->type != json_type_object) return;
            for(auto* e = static_cast<const json_object_s*>(plugins->payload)->start; e; e = e->next)
            {
                auto number = [&](const char* key) {
                    const auto* v = detail::member(e->value, key);
                    return v && v->type == json_type_number ? std::strtod(static_cast<const json_number_s*>(v->payload)->number, nullptr) : 0.0;
                };
                const double cpu = number("cpu"), rest = number("rest"), voice = number("voice"), voices = number("voices"), memory = number("memory"), samples = number("samples"), declared = number("declared"), contexts = number("contexts"), perContext = number("perContext"), held = number("sounds"), day = number("day");
                if(!std::isfinite(memory) || memory < 0 || memory > double(1ull << 40)) continue;
                const uint32_t shares = std::isfinite(contexts) && contexts >= 0 && contexts <= 32 ? uint32_t(contexts) : 0;
                const auto said = std::isfinite(declared) && declared >= 0 && declared <= double(1ull << 40) ? uint64_t(declared) : 0;
                const auto kept = keepMemory ? uint64_t(memory) : 0;
                const uint32_t holds = !kept ? 0 : std::isfinite(held) && held >= 1 && held <= 32 ? uint32_t(held) : 1; // a file before it: one sound
                const auto added = keepMemory && std::isfinite(perContext) && perContext >= 0 && perContext <= double(1ull << 40) ? uint64_t(perContext) : 0;
                // An impossible cost (a share above the audio time itself, negative, not a number) is
                // dropped and measured again; the memory stands (of this version).
                const bool possible = !memoryOnly && std::isfinite(cpu) && cpu >= 0 && cpu <= 1 && std::isfinite(rest) && rest >= 0 && rest <= 1 &&
                                      std::isfinite(voice) && voice >= 0 && voice <= 1 && std::isfinite(voices) && voices >= 0 && voices <= 4096;
                const std::string key(e->name->string, e->name->string_size);
                const uint32_t when = std::isfinite(day) && day >= 0 && day < 1e6 ? uint32_t(day) : 0;
                measured_[key] = possible ? Measurement{cpu, rest, voice, uint32_t(voices), kept, uint32_t(std::clamp(samples, 0.0, 1e9)), said, shares, added, holds, when} : Measurement{0, 0, 0, 0, kept, 0, said, shares, added, holds};
                if(possible) setPluginCost(key.substr(0, key.find('@')), cpu); // RFC 0045, before its instances play: as last measured here
            }
            if(const auto* states = detail::member(doc.get(), "states"); states && states->type == json_type_object)
                for(auto* e = static_cast<const json_object_s*>(states->payload)->start; e; e = e->next)
                    if(e->value->type == json_type_string)
                    {
                        const auto* v = static_cast<const json_string_s*>(e->value->payload);
                        states_[std::string(e->name->string, e->name->string_size)] = std::string(v->string, v->string_size);
                    }
        }
        catch(const std::exception&) {} // none yet, or unreadable: measured afresh
    }

    void Session::saveMeasurements()
    {
        if(!measurementsChanged_) return;
        std::string text = "{\"version\":" + std::to_string(measurementsVersion) + ",\"plugins\":{";
        bool first = true;
        for(const auto& [key, m] : measured_)
        {
            char numbers[360]; std::snprintf(numbers, sizeof(numbers), "{\"cpu\":%.6g,\"rest\":%.6g,\"voice\":%.6g,\"voices\":%u,\"memory\":%llu,\"samples\":%u,\"declared\":%llu,\"contexts\":%u,\"perContext\":%llu,\"sounds\":%u,\"day\":%u}", m.cpu, m.rest, m.voice, m.voices, static_cast<unsigned long long>(m.memory), m.samples, static_cast<unsigned long long>(m.declared), m.contexts, static_cast<unsigned long long>(m.perContext), m.sounds, m.day);
            text += std::string(first ? "" : ",") + detail::quote(key) + ":" + numbers; first = false;
        }
        text += "},\"states\":{";
        first = true;
        for(const auto& [state, key] : states_)
            if(measured_.count(key)) { text += std::string(first ? "" : ",") + detail::quote(state) + ":" + detail::quote(key); first = false; } // one whose figure is gone goes with it
        text += "}}\n";
        try
        {
            writeAtomically(measurementsFile(), text);
            measurementsChanged_ = false;
        }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: measurements not saved: %s\n", e.what()); }
        measurementsSaved_ = std::chrono::steady_clock::now();
    }

    void Session::setBudget(const Budget& budget)
    {
        if(!std::isfinite(budget.cpu) || budget.cpu < 0 || budget.cpu > 64) throw std::invalid_argument("a budget's cpu is a share of the audio thread's time, 0 to 64");
        if(budget.device && (budget.system > budget.device || budget.floor > budget.device - budget.system || budget.margin > budget.device - budget.system - budget.floor))
            throw std::invalid_argument("a budget's system share, floor and margin must fit the device's memory");
        budget_ = budget;
        if(budget_.cpu > 0) loadMeasurements();
        if(memoryClasses()) evictOpportunistic();
        if(memoryClasses() != (pressureFd_ >= 0 || pressureSource_)) armPressure(memoryClasses());
    }

    void Session::setShellMemory(uint64_t bytes)
    {
        if(budget_.device && budget_.system + bytes + budget_.floor + budget_.margin > budget_.device)
            throw std::invalid_argument("the UI's memory does not fit beside the system's share, the floor and the margin");
        shell_ = bytes;
        if(memoryClasses()) evictOpportunistic();
    }

    // Memory classes (RFC 0025, "Admission's arithmetic"). An app counts once, running or always available; the
    // workers' shared code once, as the system's.
    uint64_t Session::reservedMemory(bool closedAlwaysAvailable) const
    {
        uint64_t n = budget_.system + shell_ + workerCode();
        for(const auto& a : running_) n += a.reserve;
        if(closedAlwaysAvailable)
            for(const auto& a : alwaysAvailable_)
                if(std::none_of(running_.begin(), running_.end(), [&](const AppMemory& r) { return r.app == a.app; })) n += a.reserve;
        return n;
    }

    uint64_t Session::memoryCeiling() const
    {
        if(!memoryClasses()) { const auto code = budget_.memory ? workerCode() : 0; return budget_.memory > code ? budget_.memory - code : budget_.memory ? 1 : 0; }
        const auto held = reservedMemory() + budget_.margin;
        return held < budget_.device ? budget_.device - held : 1; // 1 byte: nothing fits
    }

    uint64_t Session::preparedMemory() const
    {
        uint64_t n = 0;
        if(prepared_) for(const auto& [node, name] : prepared_->names) if(prepared_->next->contains(node)) n += nodeCost(*prepared_->next, node).memory;
        return n;
    }

    uint64_t Session::committedMemory() const
    {
        uint64_t n = 0;
        for(const auto& [node, name] : nodes_) n += nodeCost(graph_, node).memory;
        for(const auto node : leaving_) if(graph_.contains(node)) n += nodeCost(graph_, node).memory;
        if(prepared_ && !preparedOpportunistic_) n += preparedMemory();
        return n + recorder_.committedBytes(); // recording: the retroactive rings, a take's own and a playback (RFC 0020)
    }

    // Opportunistic data fills what is neither reserved by a running app nor committed, up to the
    // margin; the closed always-available apps' reservations included. Let go least likely needed
    // first. shortcut: the opportunistic items built are instances kept warm (order 2) and a next entry
    // prepared to cut (order 4); the sample file cache is the kernel's, entries further ahead are not kept yet.
    void Session::evictOpportunistic()
    {
        if(!memoryClasses()) return;
        const auto used = reservedMemory(false) + budget_.margin + committedMemory();
        const auto room = used < budget_.device ? budget_.device - used : 0;
        // Order 2: instances kept warm (RFC 0021), least recently used first.
        const auto next = prepared_ && preparedOpportunistic_ ? preparedMemory() : 0;
        uint64_t bytes = warmMemory();
        letGo([&](const Free& w) { if(bytes + next <= room) return false; bytes -= std::min(bytes, w.host->residentBytes()); return true; });
        if(!next || next + warmMemory() <= room) return;
        std::fprintf(stderr, "oscairn: let go of the next entry prepared to cut: its memory is needed\n");
        prepared_.reset(); preparedOpportunistic_ = false;
    }

    bool Session::memoryPressure()
    {
        if(!warm_.empty())
        {
            std::fprintf(stderr, "oscairn: memory short: let go of %zu plugin instances kept warm\n", warm_.size());
            letGo([](const Free&) { return true; });
            return true;
        }
        if(!prepared_) return false;
        std::fprintf(stderr, preparedOpportunistic_ ? "oscairn: memory short: let go of the next entry prepared to cut\n"
                                                    : "oscairn: memory short, past the estimates: let go of the next entry prepared ahead; it loads when it is cued\n");
        prepared_.reset(); preparedOpportunistic_ = false;
        return true;
    }

    // Linux PSI: a trigger on the delegated subtree's memory.pressure (OSCAIRN_CGROUP), else the
    // system's, firing when tasks stall on memory for 100 ms in a second (2 s windows, which an
    // unprivileged process may ask for, when that is refused). It catches what admission cannot:
    // an app growing past its reservation, or estimates that were wrong.
    void Session::armPressure(bool on)
    {
#ifdef __linux__
        if(pressureFd_ >= 0) { ::close(pressureFd_); pressureFd_ = -1; }
        if(!on) return;
        const char* root = std::getenv("OSCAIRN_CGROUP");
        const auto file = root && *root ? fs::path(root) / "memory.pressure" : fs::path("/proc/pressure/memory");
        for(const char* trigger : {"some 100000 1000000", "some 150000 2000000"})
        {
            const int fd = ::open(file.c_str(), O_RDWR | O_NONBLOCK | O_CLOEXEC);
            if(fd < 0) break;
            if(::write(fd, trigger, std::strlen(trigger) + 1) > 0) { pressureFd_ = fd; return; }
            ::close(fd);
        }
        std::fprintf(stderr, "oscairn: no memory pressure trigger on %s (%s)\n", file.c_str(), std::strerror(errno));
#elif defined(__APPLE__)
        // macOS: the system's warning and critical memory pressure. The source fires on a
        // dispatch queue, never the audio thread, and only counts; the count is read at the next
        // service, which lets go of one item a firing. Its cancel handler frees the count once no
        // firing still runs.
        if(pressureSource_)
        {
            auto* source = static_cast<dispatch_source_t>(pressureSource_);
            dispatch_source_cancel(source); dispatch_release(source);
            pressureSource_ = nullptr;
        }
        if(!on) return;
        auto* source = dispatch_source_create(DISPATCH_SOURCE_TYPE_MEMORYPRESSURE, 0, DISPATCH_MEMORYPRESSURE_WARN | DISPATCH_MEMORYPRESSURE_CRITICAL,
                                              dispatch_get_global_queue(QOS_CLASS_UTILITY, 0));
        if(!source) { std::fprintf(stderr, "oscairn: no memory pressure notifications\n"); return; }
        dispatch_set_context(source, new std::atomic<uint32_t>(0));
        dispatch_source_set_event_handler_f(source, [](void* count) { static_cast<std::atomic<uint32_t>*>(count)->fetch_add(1, std::memory_order_relaxed); });
        dispatch_source_set_cancel_handler_f(source, [](void* count) { delete static_cast<std::atomic<uint32_t>*>(count); });
        dispatch_resume(source);
        pressureSource_ = source;
#else
        (void)on;
#endif
    }

    void Session::pollPressure()
    {
#ifdef __linux__
        if(pressureFd_ < 0) return;
        pollfd p{pressureFd_, POLLPRI, 0};
        if(::poll(&p, 1, 0) > 0 && (p.revents & POLLPRI)) memoryPressure();
#elif defined(__APPLE__)
        if(!pressureSource_) return;
        auto* count = static_cast<std::atomic<uint32_t>*>(dispatch_get_context(static_cast<dispatch_source_t>(pressureSource_)));
        for(auto n = count->exchange(0, std::memory_order_relaxed); n > 0 && memoryPressure(); --n) {}
#endif
    }

    void Session::setAlwaysAvailable(const std::vector<AppMemory>& apps)
    {
        if(memoryClasses())
        {
            // The apps added must fit as a launch would: beside what is reserved, the floor or what is committed, and the margin.
            uint64_t added = 0;
            for(const auto& a : apps)
                if(std::none_of(alwaysAvailable_.begin(), alwaysAvailable_.end(), [&](const AppMemory& x) { return x.app == a.app; }) &&
                   std::none_of(running_.begin(), running_.end(), [&](const AppMemory& x) { return x.app == a.app; })) added += a.reserve;
            const auto need = reservedMemory() + added + std::max(committedMemory(), budget_.floor) + budget_.margin;
            if(added && need > budget_.device)
                throw std::invalid_argument("the always-available apps need " + std::to_string(added >> 20) + " MiB more than this device has beside the Performances' floor and what is reserved");
        }
        alwaysAvailable_ = apps;
        evictOpportunistic();
    }

    Control::Launch Session::launchApp(const AppMemory& app, bool dropNext)
    {
        if(app.app.empty()) throw std::invalid_argument("an app launch names the app");
        Launch result;
        auto same = [&](const AppMemory& a) { return a.app == app.app; };
        if(std::any_of(running_.begin(), running_.end(), same)) { result.admitted = true; return result; } // already running
        const bool always = std::any_of(alwaysAvailable_.begin(), alwaysAvailable_.end(), same);
        if(!memoryClasses() || always)
        {
            // Reserved already: it always opens; opportunistic data in its share goes.
            if(always) running_.push_back(*std::find_if(alwaysAvailable_.begin(), alwaysAvailable_.end(), same));
            else running_.push_back(app);
            evictOpportunistic();
            result.admitted = true; return result;
        }
        const auto reserved = reservedMemory();
        auto fits = [&](uint64_t committed) { return reserved + app.reserve + std::max(committed, budget_.floor) + budget_.margin <= budget_.device; };
        const auto committed = committedMemory();
        if(!fits(committed))
        {
            const auto next = prepared_ && !preparedOpportunistic_ ? preparedMemory() : 0;
            if(next && fits(committed - next))
            {
                if(!dropNext)
                {
                    result.offerNext = true;
                    result.reason = "needs " + std::to_string(app.reserve >> 20) + " MiB; letting the next entry, prepared ahead, go would make room (it then loads when you go to it)";
                    return result;
                }
                prepared_.reset(); preparedOpportunistic_ = false;
            }
            else
            {
                // Free for apps: what neither the reservations, the floor (or what is committed) nor the margin hold.
                const auto held = reserved + std::max(committed, budget_.floor) + budget_.margin;
                const auto free = held < budget_.device ? budget_.device - held : 0;
                const auto short_ = app.reserve - free;
                std::vector<AppMemory> closable;
                for(const auto& a : running_) if(std::none_of(alwaysAvailable_.begin(), alwaysAvailable_.end(), [&](const AppMemory& x) { return x.app == a.app; })) closable.push_back(a);
                std::sort(closable.begin(), closable.end(), [](const AppMemory& a, const AppMemory& b) { return a.reserve < b.reserve; });
                std::string names;
                for(const auto& a : closable) if(a.reserve >= short_) names += (names.empty() ? "" : " or ") + a.app + " (" + std::to_string(a.reserve >> 20) + ")";
                if(names.empty()) for(const auto& a : closable) names += (names.empty() ? "" : " and ") + a.app + " (" + std::to_string(a.reserve >> 20) + ")";
                result.reason = "needs " + std::to_string(app.reserve >> 20) + " MiB, " + std::to_string(free >> 20) + " free for apps" + (names.empty() ? "" : "; close " + names);
                return result;
            }
        }
        running_.push_back(app);
        evictOpportunistic();
        result.admitted = true;
        return result;
    }

    void Session::appClosed(const std::string& app)
    {
        std::erase_if(running_, [&](const AppMemory& a) { return a.app == app; });
    }

    Control::MemoryState Session::memoryState()
    {
        MemoryState m;
        m.device = budget_.device; m.floor = budget_.floor; m.margin = budget_.margin;
        m.reserved = memoryClasses() ? reservedMemory() : 0;
        m.share = memoryCeiling();
        m.committed = committedMemory();
        m.opportunistic = (prepared_ && preparedOpportunistic_ ? preparedMemory() : 0) + warmMemory();
        m.available = budget_.available; m.systemAvailable = systemAvailable();
        m.running = running_; m.alwaysAvailable = alwaysAvailable_; m.shell = shell_;
        m.nextPrepared = prepared_ != nullptr; m.nextCommitted = prepared_ && !preparedOpportunistic_;
        return m;
    }

    void Session::forgetMeasurements()
    {
        loadMeasurements();
        for(auto& [key, m] : measured_) m = {0, 0, 0, 0, m.memory, 0, m.declared};
        learning_.clear();
        for(const auto& [node, name] : nodes_) graph_.seedNoteCost(node, 0, 0);
        measurementsChanged_ = true;
        saveMeasurements();
    }

    // Each service tick, once a budget is set: every plugin's CPU time per block since the last tick
    // (statistics().cpuMicroseconds over its blocks: for a plugin in a worker the CPU time of the worker's
    // audio thread, so a block the thread was not scheduled for, a page fault waiting on storage, a wait
    // for the extra block or a core shared with another worker counts for no more than the work), as a
    // share of the block period, in a running mean per plugin build (see the constants above), its memory
    // every second. Split by the notes sounding through the plugin: with none and its output silent for
    // quietSeconds, at rest; with some, per voice. RFC 0045: an instance several parts share is measured
    // once, on its first context, and recorded per sound (its share over the sounds on it, its voices all
    // its parts'), so a figure does not depend on how many parts shared the instance it was measured on;
    // its memory is recorded only from an instance playing one sound.
    void Session::measure()
    {
        if(budget_.cpu <= 0) { graph_.setVoiceBudget(loadGuard_ ? -1 : 0); return; } // RFC 0045's load guard: notes give way before a block is missed
        const auto frames = blockFrames_.load(std::memory_order_relaxed);
        if(!frames) return;
        const double blockMicroseconds = frames / sampleRate_ * 1e6;
        // RFC 0018's shared budget steals notes within the same CPU budget, from these measurements.
        graph_.setVoiceBudget(budget_.cpu * blockMicroseconds);
        const bool memoryNow = ++measureTicks_ % 200 == 0;
        // The plugins whose memory this tick has read. Every instance of a plugin is counted at its one figure (estimate), so
        // the figure is the most any of its instances holds now, never the last one read: one copy smaller than the others
        // priced them all low (five sampler copies at the organ's, 18.5 MB held each at 17.7).
        std::set<std::string> memoryRead;
        const auto quietFrames = uint64_t(quietSeconds * sampleRate_);
        auto sample = [&](NodeId node) {
            auto& host = graph_.host(node);
            if(!host.failure().empty()) { learning_.erase(node); return; } // a failed plugin, or a stand-in for one not here, costs nothing to learn from
            const auto st = host.statistics();
            const auto key = pluginKey(node);
            auto& m = measured_[key];
            auto& L = learning_[node];
            // The saved state it opened with is this key's sound, for a Performance not open (estimate); asked with its memory.
            if(memoryNow) if(const auto s = stateOf_.find(node); s != stateOf_.end() && !s->second.empty()) if(auto& k = states_[s->second]; k != key) { k = key; measurementsChanged_ = true; }
            const double was = m.cpu, wasRest = m.rest, wasVoice = m.voice;
            // Restarted, or its instances changed: measured from here, its first blocks again its start's.
            if(st.blocks < L.blocks || st.cpuMicroseconds < L.cpu || st.restarts != L.restarts)
            {
                const bool restarted = st.restarts != L.restarts && L.blocks;
                L.blocks = 0; L.seen = 0; L.restarts = st.restarts; L.busiest = {};
                L.settle = restarted ? uint64_t(restartSeconds * sampleRate_ / frames) : 0;
            }
            // Blocks missed in its first two seconds are its load's (a failure storm while it starts): that tick is not recorded.
            const bool starting = st.deadlineMisses != std::exchange(L.misses, st.deadlineMisses) && double(L.seen) * frames < 2 * sampleRate_;
            const uint64_t blocks = L.blocks ? st.blocks - L.blocks : 0;
            const double each = blocks ? (st.cpuMicroseconds - L.cpu) / double(blocks) : 0;
            L.blocks = st.blocks; L.cpu = st.cpuMicroseconds;
            L.seen += blocks;
            auto voices = graph_.sounding(node);
            bool silent = graph_.silent(node, quietFrames);
            uint32_t sounds = 1;
            if(const auto* context = dynamic_cast<const SoundContext*>(&host))
            {
                sounds = std::max(1, std::popcount(context->engine()->channels)); // the outgoing Performance's contexts on it too, which it renders
                voices = 0;
                auto add = [&](NodeId n) {
                    const auto* other = graph_.contains(n) ? dynamic_cast<const SoundContext*>(&graph_.host(n)) : nullptr;
                    if(!other || other->engine() != context->engine()) return;
                    const auto v = graph_.sounding(n);
                    voices = v == UINT32_MAX || voices == UINT32_MAX ? UINT32_MAX : voices + v;
                    silent = silent && graph_.silent(n, quietFrames);
                };
                for(const auto& [n, x] : nodes_) add(n);
                for(const auto n : leaving_) add(n);
            }
            // A node's first blocks are its load's. shortcut: a prepared instrument whose instances change mid-tick gives one sample
            // of their sum's jump (at most the period, as any); keep a base per instance if that is seen to move a figure.
            if(blocks && L.seen >= std::max<uint64_t>(warmupBlocks, L.settle) + blocks && !starting && each <= blockMicroseconds && double(blocks) * frames <= longTickSeconds * sampleRate_)
            {
                const double share = each / blockMicroseconds / sounds; // a sound's
                auto weight = [&](uint32_t& n) { return std::max(1.0 / (n++ + 1), std::min(1.0, blocks / restBlocks)); };
                if(L.busiest.add(share, blocks)) m.cpu = std::max(L.busiest.fast, m.cpu * std::exp(-double(blocks) / fallBlocks));
                m.samples = uint32_t(std::min<uint64_t>(uint64_t(m.samples) + blocks, 1'000'000'000));
                if(const auto today = uint32_t(std::chrono::duration_cast<std::chrono::hours>(std::chrono::system_clock::now().time_since_epoch()).count() / 24); today != m.day) { m.day = today; measurementsChanged_ = true; }
                if(voices == 0 && silent)
                {
                    if(!L.restN && m.rest > 0) L.restN = seedSamples;
                    m.rest += (share - m.rest) * weight(L.restN);
                }
                else if(voices && voices != UINT32_MAX)
                {
                    if(!L.voiceN && m.voice > 0) L.voiceN = seedSamples;
                    // The rest comes off only once it is known; until then the whole sample is the voices' (an upper bound).
                    m.voice += (std::max(0.0, share - (L.restN >= enoughSamples ? m.rest : 0.0)) * sounds / voices - m.voice) * weight(L.voiceN); // a sound's share over its share of the voices
                    m.voices = std::max(m.voices, voices);
                } // else a release tail, or notes not counted: the busiest load only
            }
            if(m.voices) graph_.seedNoteCost(node, m.rest * blockMicroseconds, m.voice * blockMicroseconds);
            // An effect's learned cost with signal, which the load guard counts while it sleeps (RFC 0045's guard, 12e).
            else if(!host.noteInputPorts() && m.cpu > 0) graph_.seedNoteCost(node, m.cpu * blockMicroseconds, -1);
            if(std::abs(m.rest - wasRest) > 1e-4 || std::abs(m.voice - wasVoice) > 1e-5) measurementsChanged_ = true;
            // What it declares (oscairn.memory), kept for estimates of a Performance not open: from its own
            // instance, not an effect context (whose share is its context's).
            if(graph_.effectSource(node) == node)
                if(const auto d = declaredBy(host); d && total(*d) != m.declared) { m.declared = total(*d); measurementsChanged_ = true; }
            // How many parts an instance of it carried at most (its free room and what it holds now): a Performance not open counts
            // the parts of such a plugin beyond that many as sharing instances, as they do when it opens (estimate).
            // Its sounds are asked of its worker once for the instance, by its lead node, with its memory (memoryNow): not each tick
            // for each part, which undid RFC 0045's one shared ask (a request a tick for every part). Each figure comes from a fresh
            // answer; the room changes only as its sounds do. The worker may have died since the failure was read: the
            // supervisor's to handle (as declaredBy).
            if(const auto* shared = dynamic_cast<const SoundContext*>(&host); shared && memoryNow && shared->leads())
                try
                {
                    const auto& e = *shared->engine();
                    const auto held = e.host->soundContexts().size();
                    const auto carries = SoundContext::room(e, held) + uint32_t(std::popcount(e.buses));
                    if(carries != m.contexts) { m.contexts = carries; measurementsChanged_ = true; }
                    // The instance's memory with the fewest sounds seen (its parts loaded together, never one alone: with all of them),
                    // and what each sound past those adds, the most seen, at least a byte: measured though it added nothing, within a
                    // reading's noise (an under-declaring plugin's further parts are priced at it: estimate). Its lead node reads the
                    // instance's memory.
                    const auto holds = uint32_t(std::max<size_t>(held, 1));
                    auto learn = [&](uint64_t more, uint32_t of, uint64_t fewer, uint32_t than) {
                        if(const auto each = std::max<uint64_t>(1, more > fewer ? (more - fewer) / (of - than) : 0); each > m.perContext) { m.perContext = each; measurementsChanged_ = true; }
                    };
                    auto bytes = host.residentBytes();
                    if(bytes && !memoryRead.insert(pluginKey(node)).second && holds == m.sounds) bytes = std::max(bytes, m.memory); // another instance's, this tick
                    if(bytes && (!m.memory || holds <= m.sounds))
                    {
                        if(m.memory && holds < m.sounds) learn(m.memory, m.sounds, bytes, holds); // what the figure it replaces says
                        if(bytes != m.memory || holds != m.sounds) { m.memory = bytes; m.sounds = holds; measurementsChanged_ = true; }
                    }
                    else if(bytes) learn(bytes, holds, m.memory, m.sounds);
                }
                catch(const std::exception&) {}
            if(memoryNow && !dynamic_cast<const SoundContext*>(&host))
            {
                auto bytes = host.residentBytes();
                if(!bytes) if(const auto it = loadedBytes_.find(node); it != loadedBytes_.end()) bytes = it->second;
                if(bytes && !memoryRead.insert(pluginKey(node)).second && m.sounds == 1) bytes = std::max(bytes, m.memory); // another instance's, this tick
                if(bytes && (bytes != m.memory || m.sounds != 1)) { m.memory = bytes; m.sounds = 1; measurementsChanged_ = true; }
            }
            if(std::abs(m.cpu - was) > 1e-4) measurementsChanged_ = true;
            setPluginCost(host.descriptor().id, m.cpu); // RFC 0045: where its next instances go among the plugin hosts
        };
        for(const auto& [node, name] : nodes_) sample(node);
        std::erase_if(learning_, [&](const auto& l) { return !nodes_.count(l.first); });
        if(measurementsChanged_ && std::chrono::steady_clock::now() - measurementsSaved_ > std::chrono::seconds(30)) saveMeasurements();
    }

    // RFC 0025, per core: plugins in workers render in parallel on the audio cores (all but one: the
    // system's and the display's), each part's chain one plugin after another within a block. So a
    // Performance fits when its total fits the budget's share of every audio core, and its costliest
    // chain the budget's share of one block, a part's counting the buses it plays through or sends to after
    // its own (RFC 0045's guard, 12e: `feeds`, each part's buses). Figures from too few blocks that would refuse
    // it let it open, said (uncertain).
    Control::Estimate Session::estimateOf(const std::vector<Cost>& plugins, const std::vector<Cost>& playing, const Feeds& feeds, const Feeds& playingFeeds) const
    {
        Estimate e;
        e.plugins = plugins; e.budget = budget_;
        e.cores = std::max(1u, worker::usableCores() - 1);
        if(const char* n = std::getenv("OSCAIRN_AUDIO_CORES"); n && std::atoi(n) > 0) e.cores = uint32_t(std::atoi(n)); // a small device's on a desktop, as the graph's lanes' total (tests, diagnosis)
        // The costliest chain: a part's own and then the costliest way on through the buses it feeds (no loops: the format refuses them).
        auto longestOf = [&](const std::map<std::string, double>& chains, double alone, const Feeds& feeding = {}, bool own = true) {
            const auto& f = own ? feeds : feeding;
            std::map<std::string, double> way;
            std::function<double(const std::string&)> through = [&](const std::string& part) {
                if(const auto w = way.find(part); w != way.end()) return w->second;
                double next = 0;
                if(const auto x = f.find(part); x != f.end()) for(const auto& bus : x->second) next = std::max(next, through(bus));
                const auto c = chains.find(part);
                return way[part] = (c == chains.end() ? 0 : c->second) + next;
            };
            for(const auto& [part, cpu] : chains) alone = std::max(alone, through(part));
            return alone;
        };
        std::map<std::string, double> chains; double alone = 0;
        for(const auto& c : plugins)
        {
            e.cpu += c.cpu; e.memory += c.memory;
            if(c.part.empty()) alone = std::max(alone, c.cpu); else chains[c.part] += c.cpu;
        }
        e.longest = longestOf(chains, alone);
        if(budget_.cpu <= 0) return e;
        double cpu = e.cpu; uint64_t memory = e.memory;
        std::map<std::string, double> ringing; double ringingAlone = 0; // the playing Performance's chains, beside this one's
        for(const auto& c : playing)
        {
            cpu += c.cpu; memory += c.memory;
            if(c.part.empty()) ringingAlone = std::max(ringingAlone, c.cpu); else ringing[c.part] += c.cpu;
        }
        const auto ceiling = e.budget.memory = memoryCeiling();
        const auto recording = recorder_.committedBytes(); // beside it, as committed
        const bool memoryFits = !ceiling || e.memory + recording <= ceiling;
        const bool cpuFits = e.cpu <= budget_.cpu * e.cores && e.longest <= budget_.cpu;
        // Figures from few blocks were seen three to five times too high on the FANTOM-07 (RFC 0045): one that
        // refuses the Performance only as long as it is not taken at a quarter lets it open, said.
        if(!cpuFits && std::any_of(plugins.begin(), plugins.end(), [](const Cost& c) { return c.measured && !c.certain; }))
        {
            double total = 0, longest = 0; std::map<std::string, double> quartered;
            for(const auto& c : plugins)
            {
                const double x = c.measured && !c.certain ? c.cpu / 4 : c.cpu;
                total += x; if(c.part.empty()) longest = std::max(longest, x); else quartered[c.part] += x;
            }
            e.uncertain = memoryFits && total <= budget_.cpu * e.cores && longestOf(quartered, longest) <= budget_.cpu;
        }
        // A plugin not yet measured and with no estimate of its package's never refuses one for its CPU (the
        // owner, 2026-11-13): its default guess counts while the rest fits, but one whose CPU fits without the
        // guessed plugins opens, said, and they are measured as it plays. Their memory guess still counts: what
        // does not fit in memory ends in killed workers on stage.
        auto guessed = [](const Cost& c) { return !c.measured && !c.certain; };
        if(memoryFits && !cpuFits && !e.uncertain && std::any_of(plugins.begin(), plugins.end(), guessed))
        {
            double total = 0, longest = 0; std::map<std::string, double> known;
            for(const auto& c : plugins)
            {
                if(guessed(c)) continue;
                const double x = !c.certain ? c.cpu / 4 : c.cpu;
                total += x; if(c.part.empty()) longest = std::max(longest, x); else known[c.part] += x;
            }
            e.uncertain = total <= budget_.cpu * e.cores && longestOf(known, longest) <= budget_.cpu;
        }
        // CPU refuses a Performance only past what its plugins cost at rest (measured means, which voices given way cannot
        // lower): one whose rest fits opens, said, and the load guard sheds voices under a real load (RFC 0025, 2026-11-17).
        // Its busiest figures and its voices' are estimates of a load not yet played, and a false refusal on stage costs
        // more than voices shed; memory still refuses (what does not fit ends in killed workers).
        if(memoryFits && !cpuFits && !e.uncertain)
        {
            double total = 0, longest = 0; std::map<std::string, double> resting;
            for(const auto& c : plugins) { total += c.rest; if(c.part.empty()) longest = std::max(longest, c.rest); else resting[c.part] += c.rest; }
            e.uncertain = total <= budget_.cpu * e.cores && longestOf(resting, longest) <= budget_.cpu;
        }
        e.fits = (memoryFits && cpuFits) || e.uncertain;
        // A change is seamless when this Performance and what rings out beside it (priced by the caller: the voices still
        // sounding, not the whole Performance) fit as one Performance does: their total within the budget on every audio
        // core, and each chain, theirs and its, within one block's (RFC 0021, 2026-11-19; it was both totals within one
        // block's budget, so a Performance using more than half a core never rang out). Over in CPU alone, with the memory
        // there, the change still carries the last one over and fades its tail out fast (fades) instead of a cut.
        const bool memoryBoth = !ceiling || memory + recording <= ceiling;
        const bool cpuBoth = cpu <= budget_.cpu * e.cores && std::max(e.longest, longestOf(ringing, ringingAlone, playingFeeds, false)) <= budget_.cpu;
        e.seamless = e.fits && cpuBoth && memoryBoth;
        e.fades = e.fits && !cpuBoth && memoryBoth;
        std::sort(e.plugins.begin(), e.plugins.end(), [](const Cost& a, const Cost& b) { return a.cpu > b.cpu; });
        return e;
    }

    std::string Session::tooMuch(const Estimate& e) const
    {
        std::string why = e.longest > budget_.cpu ? "needs " + percent(e.longest) + " of the audio time where this device allows " + percent(budget_.cpu) + " in one block, for its costliest part, one plugin after another"
                                                  : "needs " + percent(e.cpu) + " of the audio time where this device allows " + percent(budget_.cpu) + " on each of " + std::to_string(e.cores) + " cores";
        const auto ceiling = memoryCeiling();
        const auto recording = recorder_.committedBytes(); // as estimateOf counts it beside the Performance
        const bool cpuFits = e.cpu <= budget_.cpu * e.cores && e.longest <= budget_.cpu;
        const bool memoryOver = ceiling && e.memory + recording > ceiling;
        uint64_t guess = 0; // the default memory guess of plugins not yet measured, counted in e.memory
        for(const auto& c : e.plugins) if(!c.measured && !c.certain) guess += c.memory;
        if(cpuFits && memoryOver) why.clear(); // its CPU fits: only the memory is said
        if(memoryOver)
        {
            why += std::string(why.empty() ? "needs " : ", and ") + std::to_string((e.memory + recording) >> 20) + " MiB of memory where it allows " + std::to_string(ceiling >> 20) +
                   (recording ? " (" + std::to_string(recording >> 20) + " MiB of it recording's, the capture buffer's among them)" : std::string()) +
                   (guess ? " (" + std::to_string(guess >> 20) + " MiB of it a guess for plugins not yet measured)" : std::string());
            // With memory classes, the running apps (not always available) that hold the difference.
            std::vector<AppMemory> closable;
            if(memoryClasses())
                for(const auto& a : running_) if(std::none_of(alwaysAvailable_.begin(), alwaysAvailable_.end(), [&](const AppMemory& x) { return x.app == a.app; })) closable.push_back(a);
            std::sort(closable.begin(), closable.end(), [](const AppMemory& a, const AppMemory& b) { return a.reserve > b.reserve; });
            uint64_t freed = 0; std::string names;
            for(const auto& a : closable) if(ceiling + freed < e.memory + recording) { freed += a.reserve; names += (names.empty() ? "" : " and ") + a.app + " (" + std::to_string(a.reserve >> 20) + " MiB)"; }
            if(!names.empty() && ceiling + freed >= e.memory + recording) why += "; closing " + names + " would free enough";
        }
        std::string most;
        for(size_t i = 0; i < e.plugins.size() && i < 3; ++i)
            most += std::string(i ? ", " : "") + (e.plugins[i].name.empty() ? e.plugins[i].plugin : e.plugins[i].name) + " " + percent(e.plugins[i].cpu) + (e.plugins[i].measured ? "" : " (not yet measured)");
        return why + (most.empty() ? "" : "; most: " + most);
    }

    // The library shows what does not fit this device before it is opened (RFC 0025 step 5).
    // shortcut: each item's document is read on every query; keep the result in the library's index if libraries of thousands make it slow.
    Control::LibraryResult Session::libraryQuery(const LibraryQuery& query)
    {
        library().collect(); // what extractors answered since (RFC 0023 step 4)
        auto result = library().query(query);
        if(budget_.cpu > 0)
            for(auto& item : result.items)
                try { if(const auto e = estimate(item.path); !e.fits) item.unfit = tooMuch(e); }
                catch(const std::exception&) {} // unreadable: opening it says why
        return result;
    }

    // FNV-1a over the folder's files, their paths and bytes in path order; empty past 16 MiB or unreadable (no figure tied to it).
    std::string Session::stateDigest(const fs::path& folder)
    {
        std::error_code e;
        std::vector<fs::path> files;
        for(fs::recursive_directory_iterator it(folder, e), end; !e && it != end; it.increment(e)) if(it->is_regular_file(e)) files.push_back(it->path());
        if(e || files.empty()) return {};
        std::sort(files.begin(), files.end());
        uint64_t h = 1469598103934665603ull, bytes = 0;
        auto add = [&](const char* p, size_t n) { for(size_t i = 0; i < n; ++i) { h ^= uint8_t(p[i]); h *= 1099511628211ull; } };
        for(const auto& f : files)
        {
            const auto name = f.lexically_relative(folder).generic_string();
            add(name.data(), name.size() + 1);
            if((bytes += fs::file_size(f, e)) > 16 * 1024 * 1024 || e) return {};
            try { const auto data = state_files::read(f, 16 * 1024 * 1024); add(reinterpret_cast<const char*>(data.data()), data.size()); }
            catch(const std::exception&) { return {}; } // unreadable: no figure tied to it
        }
        char text[17]; std::snprintf(text, sizeof(text), "%016llx", static_cast<unsigned long long>(h));
        return text;
    }

    bool Session::sharesParts() noexcept
    {
        const char* bias = std::getenv("OSCAIRN_SHARING");
        return bias && (std::string(bias) == "memory" || std::string(bias) == "balanced");
    }

    bool Session::memoryFits(const Estimate& e) const
    {
        const auto ceiling = memoryCeiling();
        return !ceiling || e.memory + recorder_.committedBytes() <= ceiling;
    }

    // RFC 0045 (the owner, 2026-11-17): parts that may sound together play on instances of their own, each its own audio thread,
    // so stacked Sampler parts are not held to one core's time. Each instance loads its samples again (the kernel's file cache is
    // all they share), so a Performance whose parts apart would not fit the memory and shared would opens shared, said: a
    // Performance must load.
    Session::Sharing Session::sharing(const std::string& directory)
    {
        Sharing s;
        s.share = sharesParts();
        s.estimate = estimate(directory, s.share);
        if(s.share || directory.empty() || budget_.cpu <= 0 || memoryFits(s.estimate)) return s;
        auto shared = estimate(directory, true);
        if(!memoryFits(shared)) return s;
        s.notice = "its parts of one plugin share an instance: each in an instance of its own it needs " + std::to_string((s.estimate.memory + recorder_.committedBytes()) >> 20) +
                   " MiB of memory where this device allows " + std::to_string(memoryCeiling() >> 20) + ", so stacked parts share one instance's voices";
        s.estimate = std::move(shared); s.share = true;
        return s;
    }

    Control::Estimate Session::estimate(const std::string& directory) { return sharing(directory).estimate; }

    Control::Estimate Session::estimate(const std::string& directory, bool share)
    {
        if(budget_.cpu > 0) loadMeasurements();
        // What plays now: this Performance's plugins, and any still ringing out from the last.
        std::vector<Cost> open, ringing;
        std::vector<Cost> playing; // the open Performance as it would ring out: by the voices sounding now
        for(const auto& [node, name] : nodes_)
        {
            open.push_back(nodeCost(graph_, node, name, noteLimit(node)));
            playing.push_back(ringingCost(graph_, node, name));
            for(const auto& p : parts_) if(nodesOf(p).count(node)) { open.back().part = playing.back().part = "part " + std::to_string(p.id); break; }
        }
        for(const auto node : leaving_) if(graph_.contains(node)) { ringing.push_back(ringingCost(graph_, node)); playing.push_back(ringing.back()); }
        Feeds feeds; // each part's buses: the one it plays through, and those it sends to
        for(const auto& p : parts_)
        {
            auto& f = feeds["part " + std::to_string(p.id)];
            if(p.bus) f.push_back("part " + std::to_string(p.bus));
            for(const auto& s : p.sends) if(s.level > 0) f.push_back("part " + std::to_string(s.bus));
        }
        // The open one fits on its own; what rings out from the last only makes it not seamless,
        // so adding a plugin right after a change is not refused for a tail.
        if(directory.empty()) return estimateOf(open, ringing, feeds);
        // A Performance or a Sound in a folder: its plugins by identity, from its document.
        const auto file = fs::path(directory) / (fs::exists(fs::path(directory) / "sound.json") ? "sound.json" : "performance.json");
        const auto bytes = state_files::read(file, 4 * 1024 * 1024);
        const std::unique_ptr<json_value_s, decltype(&std::free)> doc{json_parse(bytes.data(), bytes.size()), &std::free};
        const auto* nodes = detail::member(doc.get(), "nodes");
        if(!nodes || nodes->type != json_type_array) throw std::runtime_error("not a Performance or a Sound: " + directory);
        auto text = [](const json_value_s* v) { return v && v->type == json_type_string ? std::string(static_cast<const json_string_s*>(v->payload)->string, static_cast<const json_string_s*>(v->payload)->string_size) : std::string(); };
        // Each node's part's note limit, by its key: in the part's chain or a split's lanes.
        std::map<std::string, uint32_t> limits;
        std::map<std::string, std::string> partOf; // a node's part, by its key, for the chains
        Feeds its; // each part's buses, by its ID
        auto number = [](const json_value_s* v) { return v && v->type == json_type_number ? std::strtod(static_cast<const json_number_s*>(v->payload)->number, nullptr) : 0.0; };
        if(const auto* parts = detail::member(doc.get(), "parts"); parts && parts->type == json_type_array)
            for(auto* p = static_cast<const json_array_s*>(parts->payload)->start; p; p = p->next)
            {
                const auto* limit = detail::member(detail::member(p->value, "input"), "maxNotes");
                const uint32_t n = limit && limit->type == json_type_number ? uint32_t(std::strtoul(static_cast<const json_number_s*>(limit->payload)->number, nullptr, 10)) : 0;
                const auto part = "part " + std::to_string(uint64_t(number(detail::member(p->value, "id"))));
                auto& f = its[part];
                if(const auto bus = uint64_t(number(detail::member(p->value, "bus")))) f.push_back("part " + std::to_string(bus));
                if(const auto* sends = detail::member(p->value, "sends"); sends && sends->type == json_type_array)
                    for(auto* x = static_cast<const json_array_s*>(sends->payload)->start; x; x = x->next)
                        if(number(detail::member(x->value, "level")) > 0) f.push_back("part " + std::to_string(uint64_t(number(detail::member(x->value, "bus")))));
                auto chain = [&](const json_value_s* c) {
                    if(c && c->type == json_type_array) for(auto* k = static_cast<const json_array_s*>(c->payload)->start; k; k = k->next) { limits[text(k->value)] = n; partOf[text(k->value)] = part; }
                };
                chain(detail::member(p->value, "chain"));
                if(const auto* splits = detail::member(p->value, "splits"); splits && splits->type == json_type_array)
                    for(auto* s = static_cast<const json_array_s*>(splits->payload)->start; s; s = s->next)
                        if(const auto* lanes = detail::member(s->value, "lanes"); lanes && lanes->type == json_type_array)
                            for(auto* l = static_cast<const json_array_s*>(lanes->payload)->start; l; l = l->next) chain(detail::member(l->value, "chain"));
            }
        // Which nodes share an instance when it opens (performance.cpp): saved sharing (an engine of another node's key), and the nodes of a
        // plugin that carries several parts, `contexts` of them an instance, in the order saved. An instance's first node counts the
        // memory its measurement holds, with the sounds it held then; each other node a part's sound alone (as an open Performance
        // counts it): see below.
        std::map<std::string, uint32_t> engines; // nodes by their saved engine
        std::set<std::string> offMain; // nodes with an audio wire from another of their outputs than the main one
        std::map<std::string, const json_value_s*> definitions;
        for(auto* e = static_cast<const json_array_s*>(nodes->payload)->start; e; e = e->next) { ++engines[text(detail::member(e->value, "engine"))]; definitions[text(detail::member(e->value, "key"))] = e->value; }
        if(const auto* wires = detail::member(doc.get(), "wires"); wires && wires->type == json_type_array)
            for(auto* w = static_cast<const json_array_s*>(wires->payload)->start; w; w = w->next)
            {
                const auto from = text(detail::member(w->value, "from"));
                const auto* outputs = definitions.count(from) ? detail::member(definitions.at(from), "outputs") : nullptr;
                if(text(detail::member(w->value, "kind")) != "audio" || !outputs || outputs->type != json_type_array) continue;
                for(auto* o = static_cast<const json_array_s*>(outputs->payload)->start; o; o = o->next)
                    if(number(detail::member(o->value, "id")) == number(detail::member(w->value, "fromPort")) && !(uint32_t(number(detail::member(o->value, "flags"))) & CLAP_AUDIO_PORT_IS_MAIN)) offMain.insert(from);
            }
        std::map<std::string, uint32_t> formed; // per plugin build: the nodes that form instances on their own so far
        std::map<std::string, uint32_t> further; // per instance: its nodes past the first so far
        std::vector<Cost> coming;
        for(auto* e = static_cast<const json_array_s*>(nodes->payload)->start; e; e = e->next)
        {
            const auto* plugin = detail::member(e->value, "plugin");
            const auto id = text(detail::member(plugin, "id")), key = text(detail::member(e->value, "key"));
            const auto digest = buildDigest(text(detail::member(plugin, "pathHint"))); // the build it will load, where it is here
            const auto measuredKey = id + "@" + text(detail::member(plugin, "version")) + (digest.empty() ? std::string() : " " + digest);
            // Its own sound's figures where its saved state was measured here (it opened from that state: states_), else the build's.
            // A version with no record of its own follows the link to an older version's key (its figures borrowed).
            auto own = measuredKey;
            if(const auto s = states_.find(stateDigest(fs::path(directory) / "nodes" / key)); s != states_.end() && measured_.count(s->second))
            {
                const auto sameVersion = s->second.compare(0, measuredKey.size(), measuredKey) == 0 && s->second.size() > measuredKey.size() && (s->second[measuredKey.size()] == '#' || s->second[measuredKey.size()] == ' ');
                const auto now = measurementFor(measuredKey);
                if(sameVersion || (now && now->earlier && idOf(s->second) == id + "@" && assets::older(versionOf(s->second), text(detail::member(plugin, "version"))))) own = s->second;
            }
            coming.push_back(costOf(own, text(detail::member(e->value, "name")), id, limits.count(key) ? limits.at(key) : 0, text(detail::member(plugin, "pathHint"))));
            if(own.compare(0, measuredKey.size(), measuredKey) != 0) coming.back().earlier = true; // its sound's figures from an older version
            if(partOf.count(key)) coming.back().part = partOf.at(key); // a node sharing an instance counts its own sound's CPU (RFC 0045)
            const auto engine = text(detail::member(e->value, "engine"));
            const auto* sound = detail::member(e->value, "sound");
            const auto seen = measurementFor(measuredKey);
            bool shares = share && engine != key; // saved sharing an engine's; apart, split as it opens
            auto instance = engine;
            if(!shares && share && engines[engine] == 1 && !offMain.count(key) && (!sound || sound->type == json_type_null) && seen && seen->contexts > 1)
            {
                const auto n = formed[measuredKey]++;
                shares = n % seen->contexts != 0;
                instance = measuredKey + "/" + std::to_string(n / seen->contexts);
            }
            // A further part (furtherPart).
            if(shares && seen) if(const auto x = furtherPart(seen, ++further[instance], seen->declared)) coming.back().memory = *x;
        }
        return estimateOf(coming, playing, its, feeds);
    }
}
