// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Worker.h"
#include "oscairn/plugin_host.h"
#include <algorithm>
#include <cerrno>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <thread>
#include <poll.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>
#ifdef __APPLE__
#include <mach/mach.h>
#include <mach/mach_time.h>
#include <mach/thread_policy.h>
#include <os/os_sync_wait_on_address.h>
#else
#include <climits>
#include <linux/futex.h>
#include <pthread.h>
#include <sched.h>
#include <sys/syscall.h>
#endif

namespace oscairn::worker
{
    // The calling thread's CPU time in nanoseconds (0 if the clock fails).
    static int64_t threadCpuNanoseconds() noexcept
    {
        timespec t{};
        return clock_gettime(CLOCK_THREAD_CPUTIME_ID, &t) == 0 ? int64_t(t.tv_sec) * 1'000'000'000 + t.tv_nsec : 0;
    }

    bool waitWhileEqual(std::atomic<uint32_t>& word, uint32_t value, std::chrono::nanoseconds timeout) noexcept
    {
        const auto until = std::chrono::steady_clock::now() + timeout;
        while(word.load(std::memory_order_acquire) == value)
        {
            const auto left = until - std::chrono::steady_clock::now();
            if(left <= std::chrono::nanoseconds::zero()) return false;
            const auto ns = uint64_t(std::chrono::duration_cast<std::chrono::nanoseconds>(left).count());
#ifdef __APPLE__
            mach_timebase_info_data_t base; mach_timebase_info(&base);
            os_sync_wait_on_address_with_timeout(&word, value, sizeof(uint32_t), OS_SYNC_WAIT_ON_ADDRESS_SHARED, OS_CLOCK_MACH_ABSOLUTE_TIME, ns * base.denom / base.numer);
#else
            timespec t{time_t(ns / 1000000000u), long(ns % 1000000000u)};
            syscall(SYS_futex, reinterpret_cast<uint32_t*>(&word), FUTEX_WAIT, value, &t, nullptr, 0);
#endif
        }
        return true;
    }

    void wake(std::atomic<uint32_t>& word, int waiters) noexcept
    {
#ifdef __APPLE__
        if(waiters == INT_MAX) { os_sync_wake_by_address_all(&word, sizeof(uint32_t), OS_SYNC_WAKE_BY_ADDRESS_SHARED); return; }
        for(int i = 0; i < waiters; ++i)
            if(os_sync_wake_by_address_any(&word, sizeof(uint32_t), OS_SYNC_WAKE_BY_ADDRESS_SHARED) != 0) break; // none left waiting
#else
        syscall(SYS_futex, reinterpret_cast<uint32_t*>(&word), FUTEX_WAKE, waiters, nullptr, nullptr, 0);
#endif
    }

    uint64_t cpuMask(const char* list) noexcept
    {
        if(!list || !*list) return 0;
        uint64_t mask = 0;
        for(const char* p = list;;)
        {
            char* end = nullptr;
            const long first = std::strtol(p, &end, 10);
            if(end == p || first < 0 || first > 63) return 0;
            long last = first;
            if(*end == '-') { p = end + 1; last = std::strtol(p, &end, 10); if(end == p || last < first || last > 63) return 0; }
            for(long c = first; c <= last; ++c) mask |= uint64_t(1) << c;
            if(!*end) return mask;
            if(*end != ',') return 0;
            p = end + 1;
        }
    }

    std::chrono::nanoseconds workerAllowance(uint32_t frames, double rate, const char* overrideMicroseconds) noexcept
    {
        if(overrideMicroseconds && *overrideMicroseconds)
        {
            char* end = nullptr; errno = 0;
            const long long us = std::strtoll(overrideMicroseconds, &end, 10);
            if(!*end && errno == 0 && us > 0 && us <= 60'000'000) return std::chrono::microseconds(us);
        }
        return std::chrono::nanoseconds(int64_t(0.5e9 * double(frames) / rate));
    }

    std::chrono::nanoseconds spinWindow(const char* microseconds, std::chrono::nanoseconds allowance, uint32_t threads, uint32_t cores) noexcept
    {
        if(!microseconds || !*microseconds || threads > cores) return {};
        char* end = nullptr; errno = 0;
        const long long us = std::strtoll(microseconds, &end, 10);
        if(*end || errno || us <= 0 || us > 1'000'000) return {};
        return std::min<std::chrono::nanoseconds>(std::chrono::microseconds(us), allowance / 2);
    }

    bool deadlineAllowance() noexcept
    {
        const char* v = std::getenv("OSCAIRN_WORKER_DEADLINE");
        return v && *v && std::string(v) != "0";
    }

    uint32_t usableCores() noexcept
    {
#ifdef __linux__
        if(cpu_set_t set; sched_getaffinity(0, sizeof(set), &set) == 0) return std::max(1, CPU_COUNT(&set));
#endif
        return std::max(1u, std::thread::hardware_concurrency());
    }

    bool makeRealtime(double periodUs, int linuxPriority) noexcept
    {
#ifdef __APPLE__
        (void)linuxPriority;
        mach_timebase_info_data_t base; mach_timebase_info(&base);
        const auto ticks = [&](double us) { return uint32_t(us * 1000.0 * base.denom / base.numer); };
        thread_time_constraint_policy_data_t policy{ticks(periodUs), ticks(periodUs * .5), ticks(periodUs * .9), 1};
        return thread_policy_set(mach_thread_self(), THREAD_TIME_CONSTRAINT_POLICY, reinterpret_cast<thread_policy_t>(&policy), THREAD_TIME_CONSTRAINT_POLICY_COUNT) == KERN_SUCCESS;
#else
        (void)periodUs; sched_param p{}; p.sched_priority = linuxPriority;
        if(const char* list = std::getenv("OSCAIRN_AUDIO_THREAD_CORES"); list && *list)
        {
            cpu_set_t cpus; CPU_ZERO(&cpus);
            const auto mask = cpuMask(list);
            for(int c = 0; c < 64; ++c) if(mask >> c & 1) CPU_SET(c, &cpus);
            // Once per audio thread, at its start; said, not fatal: the thread runs anywhere then.
            if(const int e = mask ? pthread_setaffinity_np(pthread_self(), sizeof cpus, &cpus) : EINVAL; e != 0)
                std::fprintf(stderr, "oscairn: OSCAIRN_AUDIO_THREAD_CORES=%s not applied to an audio thread: %s\n", list, std::strerror(e));
        }
        return pthread_setschedparam(pthread_self(), SCHED_FIFO, &p) == 0;
#endif
    }

    namespace
    {
        void writeAll(int socket, const char* data, size_t size)
        {
            while(size)
            {
#ifdef MSG_NOSIGNAL
                const auto n = ::send(socket, data, size, MSG_NOSIGNAL);
#else
                const auto n = ::send(socket, data, size, 0); // SO_NOSIGPIPE is set on the socket
#endif
                if(n < 0 && errno == EINTR) continue;
                if(n <= 0) throw std::runtime_error("Plugin worker connection lost");
                data += n; size -= size_t(n);
            }
        }
        void readAll(int socket, char* data, size_t size)
        {
            while(size)
            {
                const auto n = ::recv(socket, data, size, 0);
                if(n < 0 && errno == EINTR) continue;
                if(n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) throw WorkerTimeout();
                if(n <= 0) throw std::runtime_error(n == 0 ? "Plugin worker connection closed" : "Plugin worker connection lost");
                data += n; size -= size_t(n);
            }
        }
    }

    void send(int socket, const std::string& message)
    {
        const auto n = uint32_t(message.size());
        writeAll(socket, reinterpret_cast<const char*>(&n), 4);
        writeAll(socket, message.data(), message.size());
    }

    std::string receive(int socket, size_t maxBytes)
    {
        uint32_t n = 0; readAll(socket, reinterpret_cast<char*>(&n), 4);
        if(n > maxBytes) throw std::runtime_error("Plugin worker message too large");
        std::string data(n, '\0'); readAll(socket, data.data(), n);
        return data;
    }

    void sendDescriptor(int socket, int fd)
    {
        char byte = 0; iovec io{&byte, 1};
        alignas(cmsghdr) char control[CMSG_SPACE(sizeof(int))]{};
        msghdr m{}; m.msg_iov = &io; m.msg_iovlen = 1; m.msg_control = control; m.msg_controllen = sizeof(control);
        auto* c = CMSG_FIRSTHDR(&m); c->cmsg_level = SOL_SOCKET; c->cmsg_type = SCM_RIGHTS; c->cmsg_len = CMSG_LEN(sizeof(int));
        std::memcpy(CMSG_DATA(c), &fd, sizeof(int));
        if(sendmsg(socket, &m, 0) != 1) throw std::runtime_error("Cannot pass shared memory to the plugin worker");
    }

    int receiveDescriptor(int socket)
    {
        char byte = 0; iovec io{&byte, 1};
        alignas(cmsghdr) char control[CMSG_SPACE(sizeof(int))]{};
        msghdr m{}; m.msg_iov = &io; m.msg_iovlen = 1; m.msg_control = control; m.msg_controllen = sizeof(control);
        if(recvmsg(socket, &m, 0) != 1) throw std::runtime_error("No shared memory from the engine");
        auto* c = CMSG_FIRSTHDR(&m);
        if(!c || c->cmsg_type != SCM_RIGHTS) throw std::runtime_error("No shared memory from the engine");
        int fd; std::memcpy(&fd, CMSG_DATA(c), sizeof(int));
        return fd;
    }

#ifdef OSCAIRN_HOSTILE_WORKER
    void hostileReport(Shared& s) noexcept; // a test's worker only: overwrites what the plugin reported
#endif
#ifdef OSCAIRN_HOSTILE_SIZES
    void hostileSizes(Shared& s, uint32_t frames, uint32_t inputs, uint32_t outputs, bool wreck) noexcept; // a test's worker only: writes lies into the block's sizes, and takes them back
#endif
#ifdef OSCAIRN_STALLING_WORKER
    void stall() noexcept; // a test's worker only: one block taking long, whatever the plugin
#endif

    namespace
    {
        uint32_t channels(const std::vector<kick::clap::Port>& ports) { uint32_t n = 0; for(const auto& p : ports) n += p.channels; return n; }

        void reply(int socket, const Writer& w) { send(socket, std::string(1, Done) + w.data); }
        void fail(int socket, char kind, const std::string& error) { send(socket, std::string(1, kind) + error); }

        void putPorts(Writer& w, const std::vector<kick::clap::Port>& ports)
        {
            w.u32(uint32_t(ports.size()));
            for(const auto& p : ports) w.u32(p.id).str(p.name).u32(p.channels).u32(p.flags);
        }

        void putParameters(Writer& w, const std::vector<kick::clap::Parameter>& params)
        {
            if(params.size() > maxParameters) throw std::runtime_error("Plugin has too many parameters for a worker");
            w.u32(uint32_t(params.size()));
            for(const auto& q : params)
            {
                w.u32(q.id).str(q.name).str(q.module).f64(q.minimum).f64(q.maximum).f64(q.defaultValue).u32(q.flags).u32(q.widget).u32(q.priority).u32(q.rangeHigh).u32(q.group).u32(q.role).u32(q.view).u32(q.field).u32(q.index);
                w.u32(uint32_t(q.algorithms.size()));
                for(const auto& a : q.algorithms) for(const auto b : a) w.u8(b);
                w.u32(uint32_t(q.seconds.size()));
                for(const auto s : q.seconds) w.f64(s);
            }
        }

        // What the engine needs to know about a plugin node; WorkerHost reads it in this order.
        void describe(Writer& w, const PluginHost& host)
        {
            const auto& d = host.descriptor();
            w.str(d.path).str(d.id).str(d.name).str(d.vendor).str(d.version).str(d.description).u32(uint32_t(d.features.size()));
            for(const auto& f : d.features) w.str(f);
            w.u32(uint32_t(d.lanes.size()));
            for(const auto& [lane, name] : d.lanes) w.u32(lane).str(name);
            putParameters(w, host.parameters());
            putPorts(w, host.inputPorts()); putPorts(w, host.outputPorts());
            w.u32(host.noteInputPorts()).u32(host.noteOutputPorts());
            for(uint32_t i = 0; i < host.noteInputPorts(); ++i) w.u8(host.usesClapNotes(i));
            w.u8(host.supportsState()).u32(host.soundContextCapacity()).u8(host.effectResources().has_value());
            w.u8(host.hasWindow());
            w.u8(host.preparesSnapshots()); w.u8(host.ringsOut()); w.u8(host.savesSounds());
            w.u8(host.voiceBudget()); w.u8(host.honorsVolume());
        }

        // A crashing worker may take longer to exit than the hang limit (sanitizers report first), so
        // its signal handler marks every shared block: the engine then names a crash, not a hang.
        constexpr size_t maxNodes = 256;
        std::atomic<Shared*> mapped[maxNodes];
        // It also says which node the crashing thread was in (Doorbell::faulted): the audio thread's or the
        // control thread's, by the thread; none for a thread of a plugin's own, which could be any plugin's.
        enum class Role : uint8_t { Other, Audio, Control };
        thread_local Role role = Role::Other;
        std::atomic<Doorbell*> faultBell{nullptr};
        extern "C" void crashed(int signal)
        {
            if(auto* b = faultBell.load())
                b->faulted.store(role == Role::Audio ? b->rendering.load(std::memory_order_relaxed) : role == Role::Control ? b->serving.load(std::memory_order_relaxed) : UINT32_MAX);
            for(auto& m : mapped) if(auto* s = m.load()) s->crashed.store(uint32_t(signal));
            std::signal(signal, SIG_DFL);
            std::raise(signal);
        }

        // The plugin's sound assets resolve through the engine's store: a request to the engine,
        // answered before the worker's reply to the request it is serving.
        struct EngineAssets final : kick::clap::AssetResolver
        {
            int socket = -1;
            std::string resolve(const std::string& library, const std::string& path, std::string& sha256) override
            {
                send(socket, std::string(1, ResolveAsset) + Writer().str(library).str(path).str(sha256).data);
                Reader r; r.data = receive(socket);
                if(r.data.empty() || r.data[0] != Done) throw std::runtime_error(r.data.empty() ? "No answer from the engine" : r.data.substr(1));
                r.at = 1; auto local = r.str(); sha256 = r.str(); return local;
            }
            kick::clap::AssetReference create(const std::string& file) override
            {
                send(socket, std::string(1, CreateAsset) + Writer().str(file).data);
                Reader r; r.data = receive(socket);
                if(r.data.empty() || r.data[0] != Done) throw std::runtime_error(r.data.empty() ? "No answer from the engine" : r.data.substr(1));
                r.at = 1; kick::clap::AssetReference ref; ref.library = r.str(); ref.path = r.str(); ref.sha256 = r.str(); return ref;
            }
            std::string libraryRoot(const std::string& library, const std::string& minVersion) const override
            {
                send(socket, std::string(1, LibraryRoot) + Writer().str(library).str(minVersion).data);
                Reader r; r.data = receive(socket);
                if(r.data.empty() || r.data[0] != Done) return {};
                r.at = 1; return r.str();
            }
        };

        // Before a node activates (RFC 0025), what it declares goes to the engine, which refits the
        // worker's memory limit (its cgroup's memory.max included) and answers before the plugin
        // allocates. Activations happen only while a request is served, within its exchange; the
        // node is the request's, or the next index for one being made (Init, an effect context).
        struct Reservations
        {
            int socket = -1;
            uint32_t node = UINT32_MAX; // the node the request being served activates; UINT32_MAX: none
            static void activating(void* context, const oscairn_memory_t* declared)
            {
                auto& self = *static_cast<Reservations*>(context);
                if(self.node == UINT32_MAX) return;
                Writer w; w.u32(self.node).u8(declared != nullptr);
                if(declared) w.bytes(declared, sizeof *declared);
                send(self.socket, std::string(1, Reserve) + w.data);
                const auto answer = receive(self.socket);
                if(answer.empty() || answer[0] != Done) throw std::runtime_error(answer.empty() ? "No answer from the engine" : answer.substr(1));
            }
        };

        // A plugin node of the worker. The audio fields are set before the node is published to the
        // audio thread (Audio::live) and are the audio thread's alone from then on; the parameter IDs
        // are read before, as the main thread may re-read the plugin's parameters meanwhile.
        struct Node
        {
            std::shared_ptr<PluginHost> host;
            Shared* shared = nullptr; size_t bytes = 0;
            uint64_t revision = 0; // the parameter info the engine has
            std::vector<const float*> in; std::vector<float*> out; std::vector<clap_id> ids;
            uint64_t reported = 0; uint32_t seen = 0;
#ifdef OSCAIRN_HOSTILE_SIZES
            uint32_t trueFrames = 0, trueInputs = 0, trueOutputs = 0; // the engine's, true until the first wreck
#endif
            void publish() const
            {
                if(!shared) return;
                const auto& params = host->parameters();
                for(uint32_t i = 0; i < shared->parameterCount; ++i) shared->parameterValues[i].store(host->parameterValue(params[i].id), std::memory_order_relaxed);
            }
        };

        // The worker's one audio thread and what the control thread shares with it: the nodes with a
        // shared block, by index (null: none there), and a count of its passes over them, odd while
        // it is in one, so a node taken out is let go only once no pass can still be using it.
        struct Audio
        {
            Doorbell* bell = nullptr;
            std::atomic<Node*> live[maxNodes]{};
            std::atomic<uint32_t> count{0}, passes{0};
            std::atomic<bool> stop{false};
            double rate = 48000; uint32_t frames = 0;
        };

        // One node's block, as the engine handed it.
        void render(Node& n, uint32_t index, Doorbell& bell)
        {
            auto& s = *n.shared; auto& host = *n.host;
            const auto woke = std::chrono::steady_clock::now();
            const auto cpuAtWake = threadCpuNanoseconds();
            if(s.panic.exchange(0, std::memory_order_acq_rel)) host.panic();
            // oscairn.voice-budget: the engine's new limits, before the block, as kick::clap::Host takes them.
            if(s.setLimits) host.setVoiceLimits(s.voiceLimit, s.voiceContexts, std::min(s.voiceContextCount, OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS));
            uint32_t events = 0; // SysEx pointed at this process's mapping of the pool
            for(uint32_t i = 0; i < std::min(s.eventCount, maxEvents); ++i) if(unshare(s.events[i], s.inputBytes)) s.events[events++] = s.events[i];
#ifdef OSCAIRN_HOSTILE_SIZES
            hostileSizes(s, n.trueFrames, n.trueInputs, n.trueOutputs, false); // the plugin's host reads them
#endif
#ifdef OSCAIRN_STALLING_WORKER
            stall();
#endif
            bell.rendering.store(index, std::memory_order_relaxed);
            const bool ok = host.process(n.in.data(), s.inputChannels, n.out.data(), s.outputChannels - (s.ringOut ? 0 : s.ringOutChannels), s.frames,
                                         events ? s.events : nullptr, events, s.hasTransport ? &s.transport : nullptr);
            bell.rendering.store(UINT32_MAX, std::memory_order_relaxed);
            // Values the plugin reported while processing are published with the block, so the
            // engine reads them right after it, as from a plugin in the engine.
            if(const auto reports = host.parameterReports(); reports != n.reported)
            {
                n.reported = reports;
                for(uint32_t i = 0; i < s.parameterCount; ++i) s.parameterValues[i].store(host.parameterValue(n.ids[i]), std::memory_order_relaxed);
            }
            uint32_t count = 0; const auto* emitted = host.outputEvents(count);
            uint32_t kept = 0;
            s.outputBytes.used = 0;
            for(uint32_t i = 0; i < count && kept < maxOutputEvents; ++i)
            {
                s.outputs[kept] = emitted[i];
                if(share(s.outputs[kept], s.outputBytes)) ++kept;
            }
            s.outputCount = kept; s.ok = ok;
            s.outputDrops.store(host.outputEventDrops() + (count - kept), std::memory_order_relaxed);
            const auto st = host.statistics();
            s.blocks.store(st.blocks); s.droppedEvents.store(st.droppedEvents); s.processErrors.store(st.processErrors);
            s.deadlineMisses.store(st.deadlineMisses); s.controlSilences.store(st.controlSilences); s.idleBlocks.store(st.idleBlocks);
            s.lastMicroseconds.store(st.lastMicroseconds); s.maximumMicroseconds.store(st.maximumMicroseconds);
            s.asleep.store(host.asleep() ? 1 : 0, std::memory_order_relaxed);
            s.reset = host.wasReset();
            {
                double cost = 0; const oscairn_voice_budget_note_t* notes = nullptr; uint32_t noteCount = 0;
                s.activeVoices = host.voiceReport(cost, notes, noteCount); s.voiceCost = cost;
                s.voiceNoteCount = notes ? std::min(noteCount, maxVoiceNotes) : 0;
                std::copy_n(notes, s.voiceNoteCount, s.voiceNotes);
            }
#ifdef OSCAIRN_HOSTILE_WORKER
            hostileReport(s);
#endif
#ifdef OSCAIRN_HOSTILE_SIZES
            hostileSizes(s, n.trueFrames, n.trueInputs, n.trueOutputs, true); // lies, for the engine's audio thread
#endif
            s.wokeAt = std::chrono::nanoseconds(woke.time_since_epoch()).count();
            s.doneAt = std::chrono::nanoseconds(std::chrono::steady_clock::now().time_since_epoch()).count();
            s.cpuNs = threadCpuNanoseconds() - cpuAtWake;
            s.done.store(n.seen, std::memory_order_release);
            // The engine's thread is woken only when it said it sleeps on done (Shared::waiting): the
            // fence orders the store above before that read, as the engine's orders its flag before its read of done.
            std::atomic_thread_fence(std::memory_order_seq_cst);
            if(s.waiting.load(std::memory_order_relaxed)) wake(s.done);
        }

        // The worker's audio thread: every node with a block pending, in node order, behind the one
        // doorbell. RFC 0045's spin: asleep until shortly before the next period's first block is due
        // (a period after the last period's came), then polling until one comes or as long again
        // after; only while blocks keep coming.
        void audio(Audio& a)
        {
            auto& bell = *a.bell;
            role = Role::Audio;
            makeRealtime(1e6 * a.frames / a.rate); // the block period, not the larger shared buffer
            std::chrono::steady_clock::time_point periodStart{};
            std::chrono::nanoseconds period{};
            uint32_t spin = 0; // the spinNs of the last pass's requests
            Node* rendered[maxNodes];
            auto sleep = [&](uint32_t rung, std::chrono::nanoseconds timeout) {
                bell.sleeping.store(1, std::memory_order_seq_cst);
                if(bell.ring.load(std::memory_order_seq_cst) == rung) waitWhileEqual(bell.ring, rung, timeout);
                bell.sleeping.store(0, std::memory_order_relaxed);
            };
            for(;;)
            {
                // Read before the pass: a block handed after it moves the ring, so it is never slept through.
                const uint32_t rung = bell.ring.load(std::memory_order_seq_cst);
                uint32_t n = 0;
                a.passes.fetch_add(1, std::memory_order_seq_cst); // seq_cst with the live loads: detach's wait relies on it
                const uint32_t count = std::min<uint32_t>(a.count.load(std::memory_order_acquire), maxNodes);
                for(uint32_t i = 0; i < count; ++i)
                {
                    Node* node = a.live[i].load(std::memory_order_seq_cst);
                    if(!node) continue;
                    const auto next = node->shared->request.load(std::memory_order_acquire);
                    if(next == node->seen) continue;
                    node->seen = next;
                    const auto& s = *node->shared;
                    period = std::chrono::nanoseconds(int64_t(1e9 * std::min(s.frames, a.frames) / (s.sampleRate > 0 ? s.sampleRate : a.rate)));
                    // A later wave of the same period does not move its start.
                    if(const auto now = std::chrono::steady_clock::now(); now - periodStart > period / 2) periodStart = now;
                    spin = std::max(n ? spin : 0u, std::min<uint32_t>(s.spinNs, 2'000'000));
                    render(*node, i, bell);
                    rendered[n++] = node;
                }
#ifdef RUSAGE_THREAD
                // After the blocks are handed back, so the system call is not in their round trips; once a pass.
                if(rusage u{}; n && getrusage(RUSAGE_THREAD, &u) == 0)
                    for(uint32_t i = 0; i < n; ++i)
                    {
                        auto& s = *rendered[i]->shared;
                        s.minorFaults.store(uint64_t(u.ru_minflt), std::memory_order_relaxed);
                        s.majorFaults.store(uint64_t(u.ru_majflt), std::memory_order_relaxed);
                        s.involuntarySwitches.store(uint64_t(u.ru_nivcsw), std::memory_order_relaxed);
                    }
#else
                (void)rendered;
#endif
                a.passes.fetch_add(1, std::memory_order_acq_rel);
                if(n) continue; // another node's block may have come meanwhile
                if(a.stop.load(std::memory_order_acquire)) return;
                if(spin && periodStart.time_since_epoch().count())
                {
                    const auto due = periodStart + period;
                    if(const auto early = due - std::chrono::nanoseconds(spin) - std::chrono::steady_clock::now(); early > std::chrono::nanoseconds::zero())
                        sleep(rung, early);
                    for(const auto until = due + std::chrono::nanoseconds(spin); bell.ring.load(std::memory_order_acquire) == rung && std::chrono::steady_clock::now() < until;) relax();
                    if(bell.ring.load(std::memory_order_acquire) != rung) continue;
                    spin = 0; // blocks stopped coming: no more spinning until they come again
                }
                sleep(rung, std::chrono::seconds(1));
            }
        }
    }

    int run(int socket)
    {
        setPlacementPolicy({}); // the worker hosts the plugin itself
        role = Role::Control;
        for(const int signal : {SIGABRT, SIGSEGV, SIGBUS, SIGILL, SIGFPE}) std::signal(signal, crashed);
        std::vector<std::unique_ptr<Node>> nodes; // by index; null once destroyed, and the index then given to the next node
        Audio sound; std::thread audioThread;
        EngineAssets assets; assets.socket = socket;
        Reservations reservations; reservations.socket = socket;
        kick::clap::Host::observeActivations(&Reservations::activating, &reservations);
        double rate = 48000; uint32_t frames = 0; auto mode = ProcessingMode::Realtime;
        auto node = [&](uint32_t i) -> Node& { if(i >= nodes.size() || !nodes[i]) throw std::runtime_error("No such plugin node"); return *nodes[i]; };
        auto vacant = [&] { uint32_t i = 0; while(i < nodes.size() && nodes[i]) ++i; return i; }; // the index the next node gets
        // Replies with the node's index and description, then maps the shared block the engine
        // sends for it, if any (a node kept only to create contexts from has none), and hands it to
        // the audio thread.
        auto attach = [&](std::shared_ptr<PluginHost> host) {
            const uint32_t index = vacant();
            if(index >= maxNodes) throw std::runtime_error("Too many plugins in one worker");
            host->setAssets(&assets);
            auto n = std::make_unique<Node>(); n->host = std::move(host);
            Writer w; w.u32(index); describe(w, *n->host); reply(socket, w);
            n->revision = n->host->parameterRevision();
            Reader r; r.data = receive(socket);
            if(r.u8())
            {
                const int fd = receiveDescriptor(socket);
                struct stat info{};
                if(fstat(fd, &info) != 0 || size_t(info.st_size) < Shared::bytes(channels(n->host->inputPorts()), channels(n->host->outputPorts()), frames))
                    { close(fd); throw std::runtime_error("Shared memory is too small"); }
                n->bytes = size_t(info.st_size);
                void* memory = mmap(nullptr, n->bytes, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
                close(fd);
                if(memory == MAP_FAILED) throw std::runtime_error("Cannot map shared memory");
                auto& s = *(n->shared = static_cast<Shared*>(memory));
                mapped[index].store(n->shared);
                n->publish();
                n->ids.resize(s.parameterCount);
                for(uint32_t i = 0; i < s.parameterCount; ++i) n->ids[i] = n->host->parameters()[i].id;
                for(uint32_t c = 0; c < s.inputChannels; ++c) n->in.push_back(s.input(c));
                for(uint32_t c = 0; c < s.outputChannels; ++c) n->out.push_back(s.output(c));
                n->reported = n->host->parameterReports();
                n->seen = s.done.load(std::memory_order_acquire); // a request may already be waiting
#ifdef OSCAIRN_HOSTILE_SIZES
                n->trueFrames = s.maxFrames; n->trueInputs = s.inputChannels; n->trueOutputs = s.outputChannels;
#endif
            }
            if(index == nodes.size()) nodes.emplace_back();
            nodes[index] = std::move(n);
            // Published at a block boundary: the audio thread takes it at its next pass.
            if(nodes[index]->shared)
            {
                sound.live[index].store(nodes[index].get(), std::memory_order_release);
                if(sound.count.load(std::memory_order_relaxed) <= index) sound.count.store(index + 1, std::memory_order_release);
            }
            reply(socket, Writer{});
        };
        // Takes a node from the audio thread, waiting out any pass that may still be using it (at most
        // the blocks of one pass), then unloads it: the other nodes' audio goes on throughout.
        auto detach = [&](uint32_t index) {
            auto& n = *nodes[index];
            if(n.shared)
            {
                sound.live[index].store(nullptr, std::memory_order_seq_cst);
                if(const auto pass = sound.passes.load(std::memory_order_seq_cst); pass & 1)
                    while(sound.passes.load(std::memory_order_acquire) == pass) std::this_thread::yield();
                mapped[index].store(nullptr);
                munmap(n.shared, n.bytes); n.shared = nullptr;
            }
            nodes[index].reset();
        };
        auto stopAll = [&] {
            if(audioThread.joinable())
            {
                sound.stop.store(true, std::memory_order_release);
                sound.bell->ring.fetch_add(1, std::memory_order_seq_cst); wake(sound.bell->ring);
                audioThread.join();
            }
            for(uint32_t i = 0; i < nodes.size(); ++i) if(nodes[i]) detach(i);
            nodes.clear();
        };
        bool windows = false; // a plugin window opened here: from then on the platform's window events run between requests
        for(;;)
        {
            pollfd p{socket, POLLIN, 0};
            int ready = poll(&p, 1, windows ? 0 : 10);
            if(ready == 0 && windows && kick::clap::runWindowEvents(socket, 10)) ready = poll(&p, 1, 0);
            else if(ready == 0 && windows) ready = poll(&p, 1, 10); // a platform whose plugins run their windows themselves
            // The plugin is serviced only when the engine asks, as in the engine; values are refreshed meanwhile.
            if(ready == 0) { for(const auto& n : nodes) if(n) n->publish(); continue; }
            if(ready < 0) { if(errno == EINTR) continue; break; }
            Reader r;
            try { r.data = receive(socket); } catch(const std::exception&) { break; }
            struct Served { uint32_t& node; Doorbell* bell; ~Served() { node = UINT32_MAX; if(bell) bell->serving.store(UINT32_MAX, std::memory_order_relaxed); } } served{reservations.node, sound.bell}; // this request only
            try
            {
                const auto op = Op(r.u8());
                // The worker's rate, block and mode, then its doorbell (a descriptor); its audio thread starts.
                if(op == Op::Init)
                {
                    const int fd = receiveDescriptor(socket);
                    void* memory = MAP_FAILED;
                    if(struct stat info{}; fstat(fd, &info) == 0 && size_t(info.st_size) >= sizeof(Doorbell))
                        memory = mmap(nullptr, sizeof(Doorbell), PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
                    close(fd);
                    if(memory == MAP_FAILED) throw std::runtime_error("Cannot map the worker's doorbell");
                    if(audioThread.joinable()) { munmap(memory, sizeof(Doorbell)); throw std::runtime_error("Worker already started"); }
                    rate = r.f64(); frames = r.u32(); mode = r.u8() ? ProcessingMode::Offline : ProcessingMode::Realtime;
                    sound.bell = static_cast<Doorbell*>(memory); sound.rate = rate; sound.frames = frames;
                    served.bell = sound.bell; faultBell.store(sound.bell);
                    audioThread = std::thread(audio, std::ref(sound));
                    reply(socket, Writer{});
                    continue;
                }
                if(op == Op::Load)
                {
                    if(!sound.bell) throw std::runtime_error("Worker not started");
                    const auto path = r.str(), id = r.str(); const auto room = r.u64();
                    reservations.node = vacant(); sound.bell->serving.store(reservations.node, std::memory_order_relaxed);
                    attach(loadPlugin(path, id, rate, frames, mode, {}, room));
                    continue;
                }
                if(op == Op::Shutdown) { stopAll(); reply(socket, Writer{}); return 0; }
                const auto index = r.u32();
                auto& n = node(index);
                auto& host = *n.host;
                reservations.node = op == Op::CreateEffectContext ? vacant() : index;
                if(sound.bell) sound.bell->serving.store(reservations.node, std::memory_order_relaxed);
                Writer w;
                switch(op)
                {
                case Op::Service:
                {
                    host.service(); n.publish();
                    kick::clap::Event e; std::vector<kick::clap::Event> events;
                    while(events.size() < 4096 && host.popOutput(e)) events.push_back(e);
                    w.u32(uint32_t(events.size()));
                    for(const auto& x : events) w.bytes(x.bytes.data(), x.bytes.size());
                    // An info rescan, which the plugin's host takes in service: the new info follows.
                    const auto revision = host.parameterRevision();
                    w.u8(revision != n.revision);
                    if(revision != n.revision) { putParameters(w, host.parameters()); n.revision = revision; }
                    break;
                }
                // Published at once, so the engine reads the new value right after setting it, as in the engine.
                case Op::SetParameter: { const auto id = r.u32(); const auto v = r.f64(); w.u8(host.setParameter(id, v)); n.publish(); break; }
                case Op::Gesture: { const auto id = r.u32(); const bool begin = r.u8(); w.u8(host.parameterGesture(id, begin)); break; }
                case Op::ParameterText: { const auto id = r.u32(); w.str(host.parameterText(id, r.f64())); break; }
                case Op::SaveState: host.saveState(r.str()); break;
                case Op::LoadState: host.loadState(r.str()); n.publish(); break;
                case Op::SaveSnapshot: host.saveSnapshot(r.str()); break;
                case Op::LoadSnapshot: host.loadSnapshot(r.str()); n.publish(); break;
                case Op::CreateEffectContext: attach(host.createEffectContext()); continue;
                case Op::DestroyNode: detach(index); break;
                case Op::EffectResources:
                {
                    const auto res = host.effectResources();
                    w.u8(res.has_value());
                    if(res) w.bytes(&*res, sizeof *res);
                    break;
                }
                case Op::SoundContexts:
                {
                    const auto contexts = host.soundContexts();
                    w.u32(uint32_t(contexts.size()));
                    for(const auto& c : contexts) w.bytes(&c, sizeof c);
                    break;
                }
                case Op::PrepareSound:
                {
                    const auto name = r.str();
                    std::vector<oscairn_sound_parameter_t> values(r.u32());
                    for(auto& v : values) { v.id = r.u32(); v.value = r.f64(); }
                    const auto state = r.str(), preset = r.str(), sfz = r.str(), library = r.str(), directory = r.str();
                    w.u32(host.prepareSound(name, values, state, preset, sfz, library, directory));
                    n.publish();
                    break;
                }
                case Op::SelectSound: { const auto id = r.u32(); const auto channel = int16_t(r.u32()); w.u8(host.selectSound(id, channel)); break; }
                case Op::DiscardSound: { const auto id = r.u32(); const bool cut = r.u8(); w.u8(host.discardSound(id, cut)); break; }
                case Op::ChokeRetiring: w.u8(host.chokeRetiring()); break;
                case Op::PrepareSnapshot: { for(const auto id : host.prepareSnapshot(r.str())) w.u32(id); n.publish(); break; }
                case Op::SaveSound: { const auto id = r.u32(); host.saveSound(id, r.str()); break; }
                case Op::SoundValues:
                {
                    const auto values = host.soundValues(r.u32());
                    w.u32(uint32_t(values.size()));
                    for(const auto& v : values) w.u32(v.id).f64(v.value);
                    break;
                }
                // Presets are not listed here: the engine's side lists them through the isolated
                // scan helper, so this worker's control thread never runs a preset provider.
                case Op::LoadPreset: host.loadPreset(r.str()); n.publish(); break;
                case Op::AssetDependencies:
                {
                    const auto refs = host.assetDependencies();
                    w.u32(uint32_t(refs.size()));
                    for(const auto& a : refs) w.str(a.library).str(a.path).str(a.sha256).u8(a.optional);
                    break;
                }
                case Op::Memory:
                {
                    const auto declared = host.declaredMemory();
                    w.u8(declared.has_value());
                    if(declared) w.bytes(&*declared, sizeof *declared);
                    break;
                }
                case Op::SetMemoryRoom: host.setMemoryRoom(r.u64()); break;
                case Op::Window: { const bool show = r.u8(); host.showWindow(show, r.str()); windows = windows || show; break; }
                case Op::Reset: w.u8(host.reset()); break;
                case Op::Reconfigure: rate = r.f64(); host.reconfigure(rate, frames); break; // and the plugins loaded later load at it
                default: throw std::runtime_error("Unknown worker request");
                }
                reply(socket, w);
            }
            catch(const std::invalid_argument& e) { try { fail(socket, Invalid, e.what()); } catch(const std::exception&) { break; } }
            catch(const std::exception& e) { try { fail(socket, Failed, e.what()); } catch(const std::exception&) { break; } }
        }
        stopAll();
        kick::clap::Host::observeActivations(nullptr, nullptr);
        return 0;
    }
}
