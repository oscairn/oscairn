// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0025: resource budgets. Each plugin build's cost is measured on this device while it plays
// (its share of each block's time, its memory) and kept in the data folder, so a Performance can
// be checked against the device's budget before it loads.
#include "oscairn/session.h"
#include "oscairn/library.h"
#include "oscairn/package.h"
#include "oscairn/sound_context.h"
#include "oscairn/realtime.h"
#include "../../plugin-host/src/StateFiles.h"
#include "sheredom/json.h"
#include "JsonInput.h"
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <fstream>
#include <memory>
#include <optional>
#include <ranges>
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
        // is not refused, and marked, so the user knows it is a guess.
        constexpr double unmeasuredCpu = .02;
        constexpr uint64_t unmeasuredMemory = 32ull << 20;
        // The measured cost (Session::measure): at rest and per voice, a running mean of the CPU time
        // of sampled blocks, each sample weighing its blocks over restBlocks (about 10 s of blocks at 128
        // frames and 48 kHz, and at least 1/n while n is small); cpu, the busiest sustained load, a
        // maximum of a short mean (fastBlocks, about a second) falling with a time constant of
        // fallBlocks (a few minutes). A share is at most 1 (a block taking longer than itself is a miss).
        // enoughSamples at rest before the rest is taken off a voiced sample; a value loaded from the
        // file counts as seedSamples, so what is played now outweighs it quickly. quietSeconds without a
        // note before a block counts as at rest (a release tail is not).
        constexpr double restBlocks = 4000, fastBlocks = 400, fallBlocks = 60000, quietSeconds = .5;
        constexpr uint32_t enoughSamples = 100, seedSamples = 4;
        constexpr int measurementsVersion = 3; // 1: the old estimator's, a high-water mark of wall times, discarded; 2: bounded by histogram buckets' tops, its memory kept
        fs::path measurementsFile() { return dataHome() / "measurements.json"; }
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

        // A plugin never measured here (its key "<id>@<version>..."): its package's declared
        // estimate for this platform, kept beside its build in the store (estimate.json, RFC 0025
        // step 5), else the default. Still marked unmeasured.
        Control::Cost declaredCost(const std::string& key, const std::string& name, const std::string& plugin)
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
                        if(cpu >= 0 && cpu <= 64 && memory >= 0 && memory <= double(1ull << 40)) return {name, plugin, cpu, uint64_t(memory), false};
                    }
                    catch(const std::exception&) {} // none for that format
            }
            return {name, plugin, unmeasuredCpu, unmeasuredMemory, false, {}, false}; // not certain: the default guess, which never refuses (estimateOf)
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
    // without a declaration (a Performance not open) counts at the most of what its declarations
    // were measured at; one without a digest (not here), at the most of that version's builds.
    Control::Cost Session::costOf(const std::string& key, const std::string& name, const std::string& plugin, uint32_t limit) const
    {
        Measurement m; bool found = false;
        if(const auto it = measured_.find(key); it != measured_.end()) { m = it->second; found = true; }
        else if(key.find('#') == std::string::npos)
            for(auto it = measured_.lower_bound(key); it != measured_.end() && it->first.compare(0, key.size(), key) == 0; ++it)
            {
                const char next = it->first.size() > key.size() ? it->first[key.size()] : 0;
                if(next != '#' && (next != ' ' || key.find(' ') != std::string::npos)) continue;
                found = true;
                const auto& x = it->second;
                m = {std::max(m.cpu, x.cpu), std::max(m.rest, x.rest), std::max(m.voice, x.voice), std::max(m.voices, x.voices), std::max(m.memory, x.memory), std::max(m.samples, x.samples)};
            }
        if(!found) return declaredCost(key, name, plugin);
        // Its CPU forgotten, or none sampled yet: the guess, with the memory as measured.
        if(m.cpu <= 0 && m.rest <= 0 && m.voice <= 0)
        {
            const auto guess = declaredCost(key, name, plugin);
            return {name, plugin, guess.cpu, std::max(m.memory, guess.memory), m.memory > 0};
        }
        const double cpu = limit && m.voice > 0 ? m.rest + m.voice * limit : m.cpu;
        return {name, plugin, cpu, m.memory, true, {}, m.samples >= enoughSamples};
    }

    // Memory: the larger of the measurement and the declaration (RFC 0025: never the plugin's word
    // alone); a plugin not yet measured, its declaration instead of the default. An effect context
    // made from another counts its context bytes, from oscairn.memory or else effect contexts'
    // resources, since its engine's shared bytes are counted with the engine.
    Control::Cost Session::nodeCost(const Graph& graph, NodeId node, const std::string& name, uint32_t limit) const
    {
        const auto& host = graph.host(node);
        auto c = costOf(pluginKey(graph, node), name, host.descriptor().id, limit);
        // A shared instance's measurement is the instance's, all its parts': counted once, on its lead context.
        if(const auto* context = dynamic_cast<const SoundContext*>(&host); context && !context->leads()) c.cpu = 0;
        const auto declared = declaredBy(host);
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
            if(read != measurementsVersion && read != 2) return;
            const bool memoryOnly = read == 2; // version 2's CPU figures were overstated: measured again; its memory stands
            const auto* plugins = detail::member(doc.get(), "plugins");
            if(!plugins || plugins->type != json_type_object) return;
            for(auto* e = static_cast<const json_object_s*>(plugins->payload)->start; e; e = e->next)
            {
                auto number = [&](const char* key) {
                    const auto* v = detail::member(e->value, key);
                    return v && v->type == json_type_number ? std::strtod(static_cast<const json_number_s*>(v->payload)->number, nullptr) : 0.0;
                };
                const double cpu = number("cpu"), rest = number("rest"), voice = number("voice"), voices = number("voices"), memory = number("memory"), samples = number("samples");
                if(!std::isfinite(memory) || memory < 0 || memory > double(1ull << 40)) continue;
                // An impossible cost (a share above the audio time itself, negative, not a number) is
                // dropped and measured again; the memory stands.
                const bool possible = !memoryOnly && std::isfinite(cpu) && cpu >= 0 && cpu <= 1 && std::isfinite(rest) && rest >= 0 && rest <= 1 &&
                                      std::isfinite(voice) && voice >= 0 && voice <= 1 && std::isfinite(voices) && voices >= 0 && voices <= 4096;
                const std::string key(e->name->string, e->name->string_size);
                measured_[key] = possible ? Measurement{cpu, rest, voice, uint32_t(voices), uint64_t(memory), uint32_t(std::clamp(samples, 0.0, 1e6))} : Measurement{0, 0, 0, 0, uint64_t(memory), 0};
                if(possible) setPluginCost(key.substr(0, key.find('@')), cpu); // RFC 0045, before its instances play: as last measured here
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
            char numbers[200]; std::snprintf(numbers, sizeof(numbers), "{\"cpu\":%.6g,\"rest\":%.6g,\"voice\":%.6g,\"voices\":%u,\"memory\":%llu,\"samples\":%u}", m.cpu, m.rest, m.voice, m.voices, static_cast<unsigned long long>(m.memory), m.samples);
            text += std::string(first ? "" : ",") + detail::quote(key) + ":" + numbers; first = false;
        }
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

    // Memory classes (RFC 0025, "Admission's arithmetic"). An app counts once, running or always available.
    uint64_t Session::reservedMemory(bool closedAlwaysAvailable) const
    {
        uint64_t n = budget_.system;
        for(const auto& a : running_) n += a.reserve;
        if(closedAlwaysAvailable)
            for(const auto& a : alwaysAvailable_)
                if(std::none_of(running_.begin(), running_.end(), [&](const AppMemory& r) { return r.app == a.app; })) n += a.reserve;
        return n;
    }

    uint64_t Session::memoryCeiling() const
    {
        if(!memoryClasses()) return budget_.memory;
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
        m.running = running_; m.alwaysAvailable = alwaysAvailable_;
        m.nextPrepared = prepared_ != nullptr; m.nextCommitted = prepared_ && !preparedOpportunistic_;
        return m;
    }

    void Session::forgetMeasurements()
    {
        loadMeasurements();
        for(auto& [key, m] : measured_) m = {0, 0, 0, 0, m.memory, 0};
        learning_.clear();
        for(const auto& [node, name] : nodes_) graph_.seedNoteCost(node, 0, 0);
        measurementsChanged_ = true;
        saveMeasurements();
    }

    // Each service tick, once a budget is set: every plugin's last block as a share of the block's
    // time, in a running mean per plugin build (see the constants above), its memory every second.
    // A tick's sample is left out when the plugin did not run since the last tick, and in its first 32
    // blocks. For a plugin in a worker it is also held to the CPU time the worker's audio thread
    // used for the blocks around it (a block longer than the largest bucket's top counts as a
    // typical block of the window), so a block the thread was not scheduled for (a kernel stall, whether the
    // engine then silenced the block or not) counts for no more than the work, and one that
    // computed counts in full. Split by the notes sounding through the plugin: with none for
    // quietSeconds, at rest; with some, per voice.
    // shortcut: one sample a tick, and a bucket's top for the CPU bound; read the worker's CPU time per block itself if admission must be exact.
    void Session::measure()
    {
        if(budget_.cpu <= 0) { graph_.setVoiceBudget(loadGuard_ ? -1 : 0); return; } // RFC 0045's load guard: notes give way before a block is missed
        const auto frames = blockFrames_.load(std::memory_order_relaxed);
        if(!frames) return;
        const double blockMicroseconds = frames / sampleRate_ * 1e6;
        // RFC 0018's shared budget steals notes within the same CPU budget, from these measurements.
        graph_.setVoiceBudget(budget_.cpu * blockMicroseconds);
        const bool memoryNow = ++measureTicks_ % 200 == 0;
        const uint64_t quietBlocks = uint64_t(quietSeconds * sampleRate_ / frames) + 1;
        auto sample = [&](NodeId node) {
            auto& host = graph_.host(node);
            if(!host.failure().empty()) { learning_.erase(node); return; } // a failed plugin, or a stand-in for one not here, costs nothing to learn from
            const auto st = host.statistics();
            if(st.blocks < 32) return;
            auto& m = measured_[pluginKey(node)];
            auto& L = learning_[node];
            const double was = m.cpu, wasRest = m.rest, wasVoice = m.voice;
            const uint64_t blocks = st.blocks > L.blocks ? st.blocks - L.blocks : 0;
            const bool clean = L.blocks && blocks; // ran since the last tick
            L.blocks = st.blocks;
            const auto voices = graph_.sounding(node);
            L.quiet = voices == 0 ? L.quiet + blocks : 0;
            auto accept = [&](const Learning::Pending& s, double boundMicroseconds) {
                const double share = std::min(1.0, std::min(s.microseconds, boundMicroseconds) / blockMicroseconds);
                auto weight = [&](uint32_t& n) { return std::max(1.0 / (n++ + 1), std::min(1.0, s.blocks / restBlocks)); };
                L.fast = L.samples++ ? L.fast + (share - L.fast) * std::min(1.0, s.blocks / fastBlocks) : share;
                m.cpu = std::max(L.fast, m.cpu * std::exp(-double(s.blocks) / fallBlocks));
                if(s.voices == 0)
                {
                    if(!s.quiet) return;
                    if(!L.restN && m.rest > 0) L.restN = seedSamples;
                    m.rest += (share - m.rest) * weight(L.restN);
                    m.samples = std::min<uint32_t>(m.samples + 1, 1'000'000);
                    return;
                }
                else if(s.voices != UINT32_MAX)
                {
                    if(!L.voiceN && m.voice > 0) L.voiceN = seedSamples;
                    // The rest comes off only once it is known; until then the whole sample is the voices' (an upper bound).
                    m.voice += (std::max(0.0, share - (L.restN >= enoughSamples ? m.rest : 0.0)) / s.voices - m.voice) * weight(L.voiceN);
                    m.voices = std::max(m.voices, s.voices);
                }
                else return;
                m.samples = std::min<uint32_t>(m.samples + 1, 1'000'000);
            };
            const Learning::Pending now{st.lastMicroseconds, voices, blocks, L.quiet >= quietBlocks};
            if(auto* timing = host.workerTiming())
            {
                const auto counts = timing->cpu.read().steady.counts;
                if(std::ranges::any_of(std::views::iota(size_t(0), counts.size()), [&](size_t i) { return counts[i] < L.was[i]; })) // reset or restarted
                {
                    L.was = L.before = counts; L.pending = false;
                }
                else
                {
                    // The window's CPU times: the top of the largest bucket (what could explain a long block) and
                    // of the middle one (what a block takes). A block longer than any CPU time of the window is
                    // not explained by work, so it counts as a middle one; a noisy block elsewhere cannot lend it its time.
                    uint64_t total = 0; size_t top = counts.size(), middle = counts.size();
                    for(size_t i = 0; i < counts.size(); ++i) if(counts[i] > L.before[i]) { total += counts[i] - L.before[i]; top = i; }
                    for(size_t i = 0, seen = 0; i < counts.size() && total; ++i)
                        if(counts[i] > L.before[i] && (seen += counts[i] - L.before[i]) * 2 >= total) { middle = i; break; }
                    auto topOf = [&](size_t i) { return i + 1 < counts.size() ? double(Histogram::upperMicroseconds(i)) : double(INFINITY); };
                    if(L.pending && total)
                        accept(L.held, L.held.microseconds <= topOf(top) ? L.held.microseconds : topOf(middle));
                    L.pending = clean; L.held = now;
                    L.before = L.was; L.was = counts;
                }
            }
            else if(clean) accept(now, INFINITY);
            if(m.voices) graph_.seedNoteCost(node, m.rest * blockMicroseconds, m.voice * blockMicroseconds);
            if(std::abs(m.rest - wasRest) > 1e-4 || std::abs(m.voice - wasVoice) > 1e-5) measurementsChanged_ = true;
            if(memoryNow)
            {
                auto bytes = host.residentBytes();
                if(!bytes) if(const auto it = loadedBytes_.find(node); it != loadedBytes_.end()) bytes = it->second;
                if(bytes && bytes != m.memory) { m.memory = bytes; measurementsChanged_ = true; }
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
    // chain the budget's share of one block. Figures from too few blocks that would refuse it let it
    // open, said (uncertain).
    Control::Estimate Session::estimateOf(const std::vector<Cost>& plugins, const std::vector<Cost>& playing) const
    {
        Estimate e;
        e.plugins = plugins; e.budget = budget_;
        e.cores = std::max(1u, worker::usableCores() - 1);
        std::map<std::string, double> chains;
        for(const auto& c : plugins)
        {
            e.cpu += c.cpu; e.memory += c.memory;
            e.longest = std::max(e.longest, c.part.empty() ? c.cpu : chains[c.part] += c.cpu);
        }
        if(budget_.cpu <= 0) return e;
        double cpu = e.cpu; uint64_t memory = e.memory;
        for(const auto& c : playing) { cpu += c.cpu; memory += c.memory; }
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
                total += x; longest = std::max(longest, c.part.empty() ? x : quartered[c.part] += x);
            }
            e.uncertain = memoryFits && total <= budget_.cpu * e.cores && longest <= budget_.cpu;
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
                total += x; longest = std::max(longest, c.part.empty() ? x : known[c.part] += x);
            }
            e.uncertain = total <= budget_.cpu * e.cores && longest <= budget_.cpu;
        }
        e.fits = (memoryFits && cpuFits) || e.uncertain;
        // shortcut: a change is seamless only while both Performances' totals fit one block's budget, as before the
        // per-core model; hold them against budget x cores once a device has measured changes ringing out on several cores.
        e.seamless = e.fits && cpu <= budget_.cpu && (!ceiling || memory + recording <= ceiling);
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

    Control::Estimate Session::estimate(const std::string& directory)
    {
        if(budget_.cpu > 0) loadMeasurements();
        // What plays now: this Performance's plugins, and any still ringing out from the last.
        std::vector<Cost> open, ringing;
        for(const auto& [node, name] : nodes_)
        {
            open.push_back(nodeCost(graph_, node, name, noteLimit(node)));
            for(const auto& p : parts_) if(nodesOf(p).count(node)) { open.back().part = "part " + std::to_string(p.id); break; }
        }
        for(const auto node : leaving_) if(graph_.contains(node)) ringing.push_back(nodeCost(graph_, node));
        // The open one fits on its own; what rings out from the last only makes it not seamless,
        // so adding a plugin right after a change is not refused for a tail.
        if(directory.empty()) return estimateOf(open, ringing);
        auto playing = open; playing.insert(playing.end(), ringing.begin(), ringing.end());
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
        size_t partIndex = 0;
        if(const auto* parts = detail::member(doc.get(), "parts"); parts && parts->type == json_type_array)
            for(auto* p = static_cast<const json_array_s*>(parts->payload)->start; p; p = p->next)
            {
                const auto* limit = detail::member(detail::member(p->value, "input"), "maxNotes");
                const uint32_t n = limit && limit->type == json_type_number ? uint32_t(std::strtoul(static_cast<const json_number_s*>(limit->payload)->number, nullptr, 10)) : 0;
                const auto part = text(detail::member(p->value, "name")) + "#" + std::to_string(partIndex++);
                auto chain = [&](const json_value_s* c) {
                    if(c && c->type == json_type_array) for(auto* k = static_cast<const json_array_s*>(c->payload)->start; k; k = k->next) { limits[text(k->value)] = n; partOf[text(k->value)] = part; }
                };
                chain(detail::member(p->value, "chain"));
                if(const auto* splits = detail::member(p->value, "splits"); splits && splits->type == json_type_array)
                    for(auto* s = static_cast<const json_array_s*>(splits->payload)->start; s; s = s->next)
                        if(const auto* lanes = detail::member(s->value, "lanes"); lanes && lanes->type == json_type_array)
                            for(auto* l = static_cast<const json_array_s*>(lanes->payload)->start; l; l = l->next) chain(detail::member(l->value, "chain"));
            }
        std::vector<Cost> coming;
        for(auto* e = static_cast<const json_array_s*>(nodes->payload)->start; e; e = e->next)
        {
            const auto* plugin = detail::member(e->value, "plugin");
            const auto id = text(detail::member(plugin, "id")), key = text(detail::member(e->value, "key"));
            const auto digest = buildDigest(text(detail::member(plugin, "pathHint"))); // the build it will load, where it is here
            coming.push_back(costOf(id + "@" + text(detail::member(plugin, "version")) + (digest.empty() ? std::string() : " " + digest), text(detail::member(e->value, "name")), id, limits.count(key) ? limits.at(key) : 0));
            if(partOf.count(key)) coming.back().part = partOf.at(key);
            // A node sharing another's instance (its engine another node's key): the instance is counted once, on that node.
            if(const auto engine = text(detail::member(e->value, "engine")); !engine.empty() && engine != key) coming.back().cpu = 0;
        }
        return estimateOf(coming, playing);
    }
}
