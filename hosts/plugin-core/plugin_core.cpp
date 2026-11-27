// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/plugin_core.h"

#include "oscairn/midi_input.h"
#include "oscairn/plugin_host.h"
#include "oscairn/session.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <condition_variable>
#include <deque>
#include <filesystem>
#include <fstream>
#include <functional>
#include <future>
#include <limits>
#include <mutex>
#include <random>
#include <stdexcept>
#include <thread>

namespace fs = std::filesystem;

namespace oscairn::plugin
{
    namespace
    {
        // The state: "OSPS", its version, the Performance's ID and name, then the files of the Performance's
        // folder as Session::savePerformance writes it, each by its path in the folder and its bytes.
        // Numbers are 32-bit little-endian; a string is its length and its bytes.
        constexpr char magic[4] = {'O', 'S', 'P', 'S'};
        constexpr uint32_t stateVersion = 1;
        constexpr uint32_t maxFiles = 65536;

        struct State
        {
            Performance performance;
            std::vector<std::pair<std::string, std::string>> files; // path in the folder, contents
        };

        void put(std::vector<uint8_t>& out, uint32_t n) { for(int i = 0; i < 4; ++i) out.push_back(uint8_t(n >> (8 * i))); }
        void put(std::vector<uint8_t>& out, const std::string& s)
        {
            if(s.size() > std::numeric_limits<uint32_t>::max()) throw std::runtime_error("a file of the Performance is too large for a plugin state");
            put(out, uint32_t(s.size()));
            out.insert(out.end(), s.begin(), s.end());
        }

        std::vector<uint8_t> pack(const State& state)
        {
            std::vector<uint8_t> out(magic, magic + 4);
            put(out, stateVersion);
            put(out, state.performance.id);
            put(out, state.performance.name);
            put(out, uint32_t(state.files.size()));
            for(const auto& [path, contents] : state.files) { put(out, path); put(out, contents); }
            return out;
        }

        // Untrusted: a project file can hold anything. Every length is checked against what is left, and a path
        // must stay inside the folder it is written to.
        State unpack(const std::vector<uint8_t>& in)
        {
            size_t at = 0;
            auto bad = [](const std::string& why) { return std::invalid_argument("not an Oscairn plugin state: " + why); };
            auto number = [&] {
                if(in.size() - at < 4) throw bad("it ends early");
                uint32_t n = 0;
                for(int i = 0; i < 4; ++i) n |= uint32_t(in[at++]) << (8 * i);
                return n;
            };
            auto string = [&] {
                const auto n = number();
                if(in.size() - at < n) throw bad("it ends early");
                std::string s(in.begin() + std::ptrdiff_t(at), in.begin() + std::ptrdiff_t(at + n));
                at += n;
                return s;
            };
            if(in.size() < 4 || !std::equal(magic, magic + 4, in.begin())) throw bad("no state header");
            at = 4;
            if(const auto version = number(); version != stateVersion)
                throw std::invalid_argument("this plugin state is version " + std::to_string(version) + "; this Oscairn reads version " + std::to_string(stateVersion));
            State state;
            state.performance.id = string();
            state.performance.name = string();
            const auto count = number();
            if(count > maxFiles) throw bad("too many files");
            for(uint32_t i = 0; i < count; ++i)
            {
                auto path = string();
                const fs::path p(path);
                if(path.empty() || p.is_absolute() || p.has_root_name() || std::any_of(p.begin(), p.end(), [](const fs::path& c) { return c == ".." || c == "." || c.empty(); }))
                    throw bad("a file outside its folder");
                state.files.emplace_back(std::move(path), string());
            }
            if(at != in.size()) throw bad("bytes after its end");
            return state;
        }

        std::string read(const fs::path& file)
        {
            std::ifstream in(file, std::ios::binary);
            std::string s((std::istreambuf_iterator<char>(in)), {});
            if(!in.good() && !in.eof()) throw std::runtime_error("could not read " + file.string());
            return s;
        }

        // Where an instance's states are written and read: each its own folder under one that stays, so the asset
        // store's roots of a save folder deleted are let go at its next save (assets.cpp keeps those of a folder
        // whose parent is gone, as a drive away).
        fs::path workFolder()
        {
            std::random_device random;
            char name[32];
            std::snprintf(name, sizeof name, "%08x%08x", random(), random());
            return fs::temp_directory_path() / "oscairn-plugin" / name;
        }
    }

    struct Instance::Impl
    {
        Config config;
        std::unique_ptr<Session> session; // made, used and destroyed on the control thread; process() on the audio thread
        std::atomic<bool> active{false};
        std::array<kick::clap::Event, Session::maxInputEvents> events{}; // audio thread only

        std::thread thread;
        std::mutex mutex;
        std::condition_variable wake;
        std::deque<std::function<void()>> tasks;
        bool stopping = false;

        mutable std::mutex statusMutex;
        Status current;

        // Control thread only.
        fs::path work = workFolder();
        uint32_t folders = 0;
        std::vector<uint8_t> unopened; // a restored state that did not open, given back as it came

        explicit Impl(const Config& c) : config(c)
        {
            thread = std::thread([this] { run(); });
        }

        // The control thread: tasks in order, and the Session's service at each tick (5 ms, as the desktop's).
        void run()
        {
            for(;;)
            {
                std::function<void()> task;
                {
                    std::unique_lock lock(mutex);
                    wake.wait_for(lock, std::chrono::milliseconds(5), [this] { return stopping || !tasks.empty(); });
                    if(!tasks.empty()) { task = std::move(tasks.front()); tasks.pop_front(); }
                    else if(stopping) return;
                }
                if(task) task();
                else if(session) try { session->service(); } catch(const std::exception& e) { std::fprintf(stderr, "oscairn plugin: %s\n", e.what()); }
            }
        }

        void post(std::function<void()> task)
        {
            { std::lock_guard lock(mutex); tasks.push_back(std::move(task)); }
            wake.notify_one();
        }

        // Runs f on the control thread and waits for it, its exception passed on.
        template<class F> auto call(F f) -> decltype(f())
        {
            auto task = std::make_shared<std::packaged_task<decltype(f())()>>(std::move(f)); // shared: the waiter may return before the task's call has
            auto result = task->get_future();
            post([task] { (*task)(); });
            return result.get();
        }

        void stop()
        {
            {
                std::lock_guard lock(mutex);
                tasks.clear(); // picks and restores not started yet are not wanted now
                tasks.push_back([this] { session.reset(); });
                stopping = true;
            }
            wake.notify_one();
            thread.join();
            std::error_code e;
            fs::remove_all(work, e);
        }

        void set(Status s) { std::lock_guard lock(statusMutex); current = std::move(s); }
        void loading() { std::lock_guard lock(statusMutex); current.load = Load::Loading; current.error.clear(); current.missing.clear(); }

        // Control thread: a load, and what it found missing.
        void load(const std::function<void()>& open, Performance performance)
        {
            const auto notices = session->sessionStatus().notices.size();
            Status s = status();
            try { open(); }
            catch(const std::exception& e)
            {
                s.load = Load::Failed;
                s.error = e.what();
                set(std::move(s));
                throw;
            }
            unopened.clear();
            const auto now = session->sessionStatus();
            s.load = Load::Ready;
            s.error.clear();
            s.missing.clear();
            // Right after the load a failure is a plugin that is not here, or did not load; later ones (a worker that
            // crashed) are the session's status, not this report.
            for(const auto& f : now.failures) s.missing.push_back(f.name + ": " + f.reason);
            for(size_t i = notices; i < now.notices.size(); ++i) s.missing.push_back(now.notices[i]);
            if(performance.name.empty()) performance.name = session->performanceInfo().name;
            s.performance = std::move(performance);
            set(std::move(s));
        }

        Status status() const { std::lock_guard lock(statusMutex); return current; }

        // A save's folder beside the work folder (see workFolder), a restore's in it, gone with the instance.
        fs::path saveFolder() { return work.parent_path() / (work.filename().string() + "-save-" + std::to_string(++folders)); }
        fs::path stateFolder() { return work / ("state-" + std::to_string(++folders)); }
    };

    Instance::Instance(const Config& config) : impl_(std::make_unique<Impl>(config))
    {
        try
        {
            impl_->call([this, &config] {
                if(!config.worker.empty() && placementPolicy().worker.empty())
                {
                    auto policy = placementPolicy();
                    policy.worker = config.worker;
                    setPlacementPolicy(policy);
                }
                impl_->session = std::make_unique<Session>(config.sampleRate, config.maxFrames);
                if(!config.bundledPlugins.empty()) impl_->session->setBundledPlugins(config.bundledPlugins);
                fs::create_directories(impl_->work);
            });
        }
        catch(...)
        {
            impl_->stop();
            throw;
        }
    }

    Instance::~Instance()
    {
        impl_->active = false;
        impl_->stop();
    }

    void Instance::activate(double sampleRate)
    {
        auto apply = [this, sampleRate] {
            if(sampleRate != impl_->session->sampleRate()) impl_->session->setSampleRate(sampleRate);
            impl_->active = true;
        };
        // Behind a load in progress the control thread is busy: do not make the caller (a host's main thread)
        // wait for it. The rate is applied and process() starts playing when the load is done.
        if(impl_->status().load == Load::Loading) impl_->post(apply);
        else impl_->call(apply);
    }

    void Instance::deactivate()
    {
        impl_->active = false;
        impl_->post([this] { impl_->active = false; }); // after an activation still waiting behind a load
    }

    bool Instance::process(const Block& block) noexcept
    {
        auto& d = *impl_;
        const bool bus = block.main[0] && block.main[1];
        if(!d.active.load(std::memory_order_acquire) || !bus || block.frames == 0 || block.frames > d.config.maxFrames)
        {
            if(bus) for(auto* channel : block.main) std::fill(channel, channel + std::min(block.frames, d.config.maxFrames), 0.0f);
            return false;
        }
        uint32_t n = 0;
        for(uint32_t i = 0; i < block.eventCount && n < d.events.size(); ++i)
        {
            const auto& m = block.events[i];
            const auto frame = std::min(m.frame, block.frames - 1);
            kick::clap::Event event;
            if(m.size == 0 || m.size > 3 || !midiToClap(m.bytes, m.size, frame, event)) continue;
            // Notes go as MIDI, as the device's inputs give them: the graph gives them note IDs (MidiBlock).
            if(m.size == 3 && ((m.bytes[0] & 0xf0) == 0x80 || (m.bytes[0] & 0xf0) == 0x90)) event = kick::clap::Event::midi(m.bytes[0], m.bytes[1], m.bytes[2], frame);
            d.events[n++] = event;
        }
        return d.session->process(block.main, block.frames, {d.events.data(), n});
    }

    std::vector<Item> Instance::library()
    {
        return impl_->call([this] {
            Control::LibraryQuery query;
            query.kind = "performance";
            query.limit = std::numeric_limits<uint32_t>::max();
            std::vector<Item> items;
            for(auto& i : impl_->session->libraryQuery(query).items) items.push_back({i.id, i.name, i.library, i.tags});
            return items;
        });
    }

    void Instance::pick(const std::string& id)
    {
        impl_->loading();
        impl_->post([this, id] {
            try { impl_->load([&] { impl_->session->loadPerformanceFromLibrary(id); }, {id, {}}); }
            catch(const std::exception&) {} // in the status
        });
    }

    Status Instance::status() const { return impl_->status(); }

    std::vector<uint8_t> Instance::state()
    {
        return impl_->call([this] {
            auto& d = *impl_;
            if(!d.unopened.empty()) return d.unopened;
            const auto folder = d.saveFolder();
            State state;
            state.performance = d.status().performance;
            try
            {
                d.session->savePerformance(folder.string());
                for(const auto& f : fs::recursive_directory_iterator(folder))
                    if(f.is_regular_file()) state.files.emplace_back(fs::relative(f.path(), folder).generic_string(), read(f.path()));
                std::sort(state.files.begin(), state.files.end());
            }
            catch(...) { std::error_code e; fs::remove_all(folder, e); throw; }
            std::error_code e;
            fs::remove_all(folder, e);
            return pack(state);
        });
    }

    void Instance::setState(const std::vector<uint8_t>& bytes)
    {
        auto state = std::make_shared<State>(unpack(bytes));
        impl_->loading();
        impl_->post([this, state, bytes] {
            auto& d = *impl_;
            // Kept while the instance lives: a stand-in's saved state is read from it again at the next save, and the
            // Performance it replaces may ring out from its own.
            const auto folder = d.stateFolder();
            try
            {
                for(const auto& [path, contents] : state->files)
                {
                    const auto file = folder / fs::path(path);
                    fs::create_directories(file.parent_path());
                    std::ofstream out(file, std::ios::binary);
                    out.write(contents.data(), std::streamsize(contents.size()));
                    if(!out) throw std::runtime_error("could not write " + file.string());
                }
                d.load([&] { d.session->loadPerformance(folder.string()); }, state->performance);
            }
            catch(const std::exception& e)
            {
                if(d.status().load != Load::Failed) { auto s = d.status(); s.load = Load::Failed; s.error = e.what(); d.set(std::move(s)); }
                d.unopened = bytes;
            }
        });
    }
}
