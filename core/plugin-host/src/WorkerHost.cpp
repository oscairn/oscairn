// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Engine side of plugin workers (RFC 0010): PluginHosts whose plugins run in a child process.
// A Process is one worker. It hosts one plugin, or in a plugin host (RFC 0045) several, and the
// native effect contexts created from each, which share its engine; each of those is a WorkerHost,
// a graph node with its own shared block for audio and events. Control calls travel over the
// worker's socket. A failed worker is restarted, and each node gets back its state, prepared
// sounds and parameter values.
#include "Worker.h"
#include "oscairn/plugin_host.h"
#include <algorithm>
#include <array>
#include <cerrno>
#include <csignal>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <fcntl.h>
#include <iterator>
#include <cmath>
#include <condition_variable>
#include <cstdlib>
#include <cstring>
#include <map>
#include <set>
#include <mutex>
#include <spawn.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>
#include <string_view>
#include <unordered_map>
#ifdef __APPLE__
#include <libproc.h>
#include <mach/mach_time.h>
#else
#include <ctime>
#endif

extern char** environ;

namespace oscairn
{
    namespace
    {
        using namespace worker;

        std::atomic<uint64_t> requests{0}; // the requests made to any worker (workerRequests)
        std::atomic<uint64_t> failures{0}; // worker processes failed (workerFailures)
        // Held from making a descriptor until it is close-on-exec, and around each spawn: a worker started by another thread
        // meanwhile (a supervisor's restart beside a load) inherited another worker's socket, so that worker's death closed
        // nothing and a request to it waited out the whole reply timeout (control-performance-switch-stress-hosts hung).
        std::mutex& descriptorMutex() { static std::mutex m; return m; }

        // OSCAIRN_WORKER_REST (realtime.h's workerRest), read as the engine starts: 0 calls every worker every block, as
        // before resting; which of the others may rest, the graph says (Graph's rest call).
        const bool workerRest = worker::workerRest() != WorkerRest::never;

        uint32_t channels(const std::vector<kick::clap::Port>& ports) { uint32_t n = 0; for(const auto& p : ports) n += p.channels; return n; }

        std::vector<kick::clap::Port> readPorts(Reader& r)
        {
            std::vector<kick::clap::Port> ports(r.count(16));
            for(auto& p : ports) { p.id = r.u32(); p.name = r.str(); p.channels = r.u32(); p.flags = r.u32(); }
            return ports;
        }

        // A /proc file of the worker's, read whole; empty where it cannot be read.
        [[maybe_unused]] std::string procText(pid_t pid, const char* name)
        {
            std::ifstream f("/proc/" + std::to_string(pid) + "/" + name);
            return std::string(std::istreambuf_iterator<char>(f), {});
        }

        // A worker's memory (macOS: its physical footprint, compressed pages included, clean code not; Linux:
        // resident less the file pages it held at its start, `startFile`, which all workers share: workerOwnBytes)
        // and CPU time in seconds; zero memory when it cannot be read.
        struct Usage { uint64_t memory = 0; double cpu = 0; };
        Usage usage(pid_t pid, [[maybe_unused]] uint64_t startFile)
        {
            Usage u;
#ifdef __APPLE__
            rusage_info_current ri{};
            if(proc_pid_rusage(pid, RUSAGE_INFO_CURRENT, reinterpret_cast<rusage_info_t*>(&ri)) != 0) return u;
            mach_timebase_info_data_t base; mach_timebase_info(&base); // the times are in Mach ticks on Apple silicon
            u.memory = ri.ri_phys_footprint;
            u.cpu = double(ri.ri_user_time + ri.ri_system_time) * base.numer / base.denom * 1e-9;
#else
            u.memory = workerOwnBytes(procText(pid, "statm"), startFile, uint64_t(sysconf(_SC_PAGESIZE)));
            // Nanoseconds; /proc's clock ticks are too coarse for a light plugin.
            clockid_t clock; timespec t{};
            if(clock_getcpuclockid(pid, &clock) == 0 && clock_gettime(clock, &t) == 0) u.cpu = double(t.tv_sec) + t.tv_nsec * 1e-9;
#endif
            return u;
        }

        std::vector<kick::clap::Parameter> readParameters(Reader& r)
        {
            std::vector<kick::clap::Parameter> parameters(r.count(84));
            for(auto& p : parameters)
            {
                p.id = r.u32(); p.name = r.str(); p.module = r.str(); p.minimum = r.f64(); p.maximum = r.f64(); p.defaultValue = r.f64(); p.flags = r.u32(); p.widget = r.u32(); p.priority = r.u32(); p.rangeHigh = r.u32(); p.group = r.u32(); p.role = r.u32();
                p.view = r.u32(); p.field = r.u32(); p.index = r.u32();
                const auto routings = r.u32();
                if(routings > 128) throw std::runtime_error("Worker sent too many algorithms");
                p.algorithms.resize(routings);
                for(auto& a : p.algorithms) for(auto& b : a) b = r.u8();
                const auto seconds = r.u32();
                if(seconds > 128) throw std::runtime_error("Worker sent too many times");
                p.seconds.resize(seconds);
                for(auto& s : p.seconds) s = r.f64();
            }
            return parameters;
        }

        class WorkerHost;

        // A plugin loaded into a worker and the effect contexts made from it: what a restarted worker
        // loads again to make them, and the description it must repeat. Its nodes hold it.
        struct Engine { std::string path, id, description; };

        // One worker process and what its nodes share: the control socket, the supervisor, the
        // block budget and the restart policy. Requests and node changes hold `control`.
        struct Process
        {
            static constexpr size_t maxRestarts = 3;
            std::string program;
            double rate; uint32_t frames;
            std::atomic<double> currentRate{0}; // rate, for the plugin host pool's choice without control
            // RFC 0045: one of the pool's plugin hosts, holding the plugins placed in it (else a worker of one
            // plugin's own); its doorbell, which its audio thread sleeps on, kept across restarts.
            bool pooled = false;
            int slot = -1; // a `chain` or `pool` process's number, its core among the workers' audio cores (audioThreadMask); -1: none
            static inline std::atomic<int> slots{0};
            bool alone = false; // a pool's host holding a group placed alone (RFC 0045's plugin groups); under the pool's mutex
            int bellFd = -1; Doorbell* bell = nullptr;
            // The worker's index of the node a crash or hang is put down to (UINT32_MAX: none), read as it failed.
            uint32_t blamed = UINT32_MAX; // under failureMutex
            const Engine* making = nullptr; // under control: the engine an effect context is being made from (reserve)
            std::atomic<bool> broken{false}; // the connection closed under a request: the worker died serving it
            std::chrono::nanoseconds hangLimit{};
            // The allowance per block, and the blocks in a row that end a worker, follow the block length the engine
            // runs (block, from follow), not the most a block can be (frames): a device's buffer is not its period.
            std::atomic<int64_t> budgetNs{0};
            std::atomic<uint32_t> block{0}, lastBlock{0};
            int64_t allowanceOverrideNs = 0;
            std::chrono::nanoseconds budget() const noexcept { return std::chrono::nanoseconds(budgetNs.load(std::memory_order_relaxed)); }
            // RFC 0045's spin (spinWindow): OSCAIRN_WORKER_SPIN_US as read with the rate, and the window it gives
            // the engine's wait for a block; real time only. Every worker with a node, one audio thread each,
            // counts toward whether workers may spin too (audioThreads, against the cores).
            std::string spinUs;
            std::atomic<int64_t> spinNs{0};
            static inline std::atomic<uint32_t> audioThreads{0};
            static uint32_t cores() noexcept { static const uint32_t n = usableCores(); return n; }
            long replySeconds = 30; // how long a control request may take (a big preset load)
            std::atomic<uint32_t> missLimit{UINT32_MAX}; // the supervisor reads it too
            const uint64_t policyLimit; // the placement policy's, in bytes; 0: none
            // What the supervisor holds it to: the policy's, or its plugins' declarations plus a margin (RFC 0025, fitLimit).
            std::atomic<uint64_t> memoryLimit;
            const ProcessingMode mode; // the plugins' clap.render mode, given them again at each restart
            std::filesystem::path cgroup; // Linux, with OSCAIRN_CGROUP: this worker's cgroup
            // The supervisor's latest reading of the worker, and the CPU time of the workers it replaced.
            std::atomic<uint64_t> memory{0};
            std::atomic<uint64_t> startFile{0}; // Linux: its file pages before any plugin loaded (usage), read at its start
            std::atomic<double> cpu{0}, cpuBefore{0};
            std::mutex control;
            int socket = -1; pid_t pid = 0;
            std::atomic<bool> reaped{true}; // fail() sets it from the supervisor without control, which a hung request may hold
            std::mutex nodesMutex;
            std::vector<WorkerHost*> nodes; // changed under control and nodesMutex
            std::atomic<kick::clap::AssetResolver*> assets{nullptr};
            std::atomic<bool> failed{false};
            std::atomic<uint32_t> generation{0};
            std::atomic<uint64_t> restarted{0};
            mutable std::mutex failureMutex;
            std::string failureText, reason; // reason: supervisor thread
            std::atomic<bool> gaveUp{false};
            bool stopping = false;
            bool restarting = false; // under control: the nodes' worker indexes are the old worker's
            std::vector<std::chrono::steady_clock::time_point> restarts;
            std::atomic<bool> restartGrace{true}; // the restart under way is the first within a minute: its start has the grace (Node::start)
            std::filesystem::path records; // recovery records of this worker's nodes
            uint32_t saves = 0;
            ServiceThread supervisor; // off the audio path: costs the stack it touches (RFC 0025)
            std::mutex superviseMutex;
            std::condition_variable wakeSupervisor;
            static inline std::atomic<uint32_t> counter{0};

            Process(std::string program_, double rate_, uint32_t frames_, ProcessingMode mode, uint64_t memoryLimit_)
                : program(std::move(program_)), rate(rate_), frames(frames_), policyLimit(memoryLimit_), memoryLimit(memoryLimit_), mode(mode)
            {
                // Read here, on the creating thread: OSCAIRN_WORKER_REPLY_SECONDS shortens it for tests.
                if(const char* s = std::getenv("OSCAIRN_WORKER_REPLY_SECONDS")) replySeconds = std::max(1L, std::atol(s));
                hangLimit = mode == ProcessingMode::Offline ? std::chrono::nanoseconds(std::chrono::seconds(10)) : std::chrono::nanoseconds(std::chrono::milliseconds(250));
                setRate(rate);
                records = std::filesystem::temp_directory_path() / ("oscairn-worker-" + std::to_string(getpid()) + "-" + std::to_string(counter.fetch_add(1)));
                const std::string name = "/osc-" + std::to_string(getpid()) + "-bell-" + std::to_string(counter.fetch_add(1));
                {
                    std::lock_guard descriptors(descriptorMutex());
                    bellFd = shm_open(name.c_str(), O_CREAT | O_EXCL | O_RDWR, 0600);
                    if(bellFd < 0) throw std::runtime_error("Cannot create a worker's doorbell");
                    fcntl(bellFd, F_SETFD, FD_CLOEXEC); // other plugins' workers must not inherit it
                }
                shm_unlink(name.c_str());
                void* memory = ftruncate(bellFd, off_t(sizeof(Doorbell))) == 0 ? mmap(nullptr, sizeof(Doorbell), PROT_READ | PROT_WRITE, MAP_SHARED, bellFd, 0) : MAP_FAILED;
                if(memory == MAP_FAILED) { close(bellFd); throw std::runtime_error("Cannot map a worker's doorbell"); }
                dirtyPages(memory, sizeof(Doorbell)); // no first write faults on the audio thread (Worker.h)
                bell = new(memory) Doorbell();
            }

            // The rate a (re)started worker loads its plugin at, and the block budget it sets. Under
            // control, while nothing plays (the constructor, or a rate change with the graph paused).
            void setRate(double rate_)
            {
                rate = rate_; currentRate.store(rate_);
                // Each block may take the worker's allowance (realtime.h) before it is silenced; a
                // worker that misses a quarter second of blocks in a row, or leaves one unanswered
                // that long, has failed. Offline rendering waits instead of silencing.
                allowanceOverrideNs = workerAllowance(0, rate, std::getenv("OSCAIRN_WORKER_BUDGET_US")).count(); // 0 frames: only a valid override is not 0
                spinUs = std::getenv("OSCAIRN_WORKER_SPIN_US") ? std::getenv("OSCAIRN_WORKER_SPIN_US") : "";
                retime(block.load() ? block.load() : frames);
            }
            // The budget and the miss limit for blocks of n frames.
            void retime(uint32_t n) noexcept
            {
                block = n;
                if(mode == ProcessingMode::Offline) { budgetNs = std::chrono::nanoseconds(std::chrono::seconds(10)).count(); return; }
                budgetNs = allowanceOverrideNs ? allowanceOverrideNs : workerAllowance(n, rate, nullptr).count();
                spinNs = spinWindow(spinUs.c_str(), budget()).count();
                missLimit = std::max<uint32_t>(8, uint32_t(std::ceil(0.25 / (double(n) / rate))));
            }
            // The audio thread, with each block's length: one that comes twice in a row is the running length (the odd
            // tail of a callback longer than the engine's block does not move it); none longer than `frames` counts.
            void follow(uint32_t n) noexcept
            {
                n = std::min(n, frames);
                if(n && lastBlock.exchange(n, std::memory_order_relaxed) == n && n != block.load(std::memory_order_relaxed)) retime(n);
            }

            ~Process()
            {
                if(supervisor.joinable())
                {
                    { std::lock_guard lock(superviseMutex); stopping = true; }
                    wakeSupervisor.notify_all();
                    supervisor.join();
                }
                const bool asked = socket >= 0 && !failed.load();
                if(asked)
                {
                    timeval t{1, 0}; setsockopt(socket, SOL_SOCKET, SO_RCVTIMEO, &t, sizeof t);
                    try { std::lock_guard lock(control); exchange(Writer().u8(uint8_t(Op::Shutdown)).data); } catch(const std::exception&) {}
                }
                if(socket >= 0) { close(socket); socket = -1; }
                if(pid > 0 && !reaped)
                {
                    int status = 0;
                    for(int i = 0; asked && i < 100 && waitpid(pid, &status, WNOHANG) == 0; ++i) std::this_thread::sleep_for(std::chrono::milliseconds(10));
                    if(waitpid(pid, &status, WNOHANG) == 0) { kill(pid, SIGKILL); waitpid(pid, &status, 0); }
                    reaped = true;
                }
                leaveCgroup();
                std::error_code ignored;
                std::filesystem::remove_all(records, ignored);
                munmap(bell, sizeof(Doorbell)); close(bellFd);
            }

            // Starts a worker process with no plugin in it, its audio thread waiting at the doorbell. Under control.
            void start()
            {
                int pair[2];
                int spawned = 0;
                {
                    // Both ends close-on-exec before any spawn can see them; the worker's end reaches it as descriptor 3 (a dup2 target is not close-on-exec).
                    std::lock_guard descriptors(descriptorMutex());
                    if(socketpair(AF_UNIX, SOCK_STREAM, 0, pair) != 0) throw std::runtime_error("Cannot create a worker socket");
                    fcntl(pair[0], F_SETFD, FD_CLOEXEC); fcntl(pair[1], F_SETFD, FD_CLOEXEC);
#ifdef SO_NOSIGPIPE
                    const int one = 1; setsockopt(pair[0], SOL_SOCKET, SO_NOSIGPIPE, &one, sizeof one); setsockopt(pair[1], SOL_SOCKET, SO_NOSIGPIPE, &one, sizeof one);
#endif
                    socket = pair[0];
                    posix_spawn_file_actions_t actions; posix_spawn_file_actions_init(&actions);
                    posix_spawn_file_actions_adddup2(&actions, pair[1], 3);
                    // The policy's memory limit also goes to the worker, which on Linux makes it an address-space backstop.
                    std::string fd = "3", limit = std::to_string(policyLimit), place = std::to_string(slot); char* argv[]{program.data(), fd.data(), limit.data(), place.data(), nullptr};
                    spawned = posix_spawn(&pid, program.c_str(), &actions, nullptr, argv, environ);
                    posix_spawn_file_actions_destroy(&actions);
                    close(pair[1]);
                }
                if(spawned != 0) { close(socket); socket = -1; pid = 0; throw std::runtime_error("Cannot start plugin worker " + program); }
                reaped = false;
                joinCgroup(); // before it loads anything, so the plugin is charged to its own cgroup
                timeval t{replySeconds, 0}; setsockopt(socket, SOL_SOCKET, SO_RCVTIMEO, &t, sizeof t);
                broken.store(false);
                bell->sleeping.store(0); bell->rendering.store(UINT32_MAX); bell->serving.store(UINT32_MAX); bell->faulted.store(UINT32_MAX);
                send(socket, Writer().u8(uint8_t(Op::Init)).f64(rate).u32(frames).u8(uint8_t(mode)).data);
                sendDescriptor(socket, bellFd);
                try { expect(receive(socket)); } catch(const WorkerTimeout& e) { lost(e); }
#ifdef __linux__
                startFile.store(residentFileBytes(procText(pid, "status"))); // initialized, its memory locked, no plugin yet
#endif
            }
            // Loads a plugin into the worker. The reply is its node's index and description; the caller
            // then attaches it. A load gets the full 30 s, whatever the requests' timeout. Under control.
            Reader load(const std::string& path, const std::string& id, uint64_t memoryRoom)
            {
                timeval start{std::max(30L, replySeconds), 0}; setsockopt(socket, SOL_SOCKET, SO_RCVTIMEO, &start, sizeof start);
                struct Shorter { int socket; long seconds; ~Shorter() { timeval t{seconds, 0}; setsockopt(socket, SOL_SOCKET, SO_RCVTIMEO, &t, sizeof t); } } shorter{socket, replySeconds};
                return exchange(Writer().u8(uint8_t(Op::Load)).str(path).str(id).u64(memoryRoom).data);
            }
            // The worker's index of the node a failure is put down to (UINT32_MAX: none known), from its doorbell:
            // the thread that crashed (faulted), the audio thread that hung (rendering), the request not
            // answered (serving); for blocks too slow, the costliest node's.
            void blame(const std::atomic<uint32_t>& index) { std::lock_guard lock(failureMutex); blamed = index.load(std::memory_order_relaxed); }
            void blameCostliest();

            // A reply that did not come in time: the worker is alive, and its late reply would be
            // read as the next request's, so it is failed and killed, and the supervisor reaps and
            // restarts it. A closed connection is a worker dying, which the supervisor sees and
            // names. Under control.
            [[noreturn]] void lost(const WorkerTimeout& e)
            {
                const std::string why = std::string("Plugin worker stopped answering: ") + e.what();
                blame(bell->serving);
                { std::lock_guard lock(failureMutex); reason = why; failureText = why; }
                failed.store(true); failures.fetch_add(1, std::memory_order_relaxed);
                if(pid > 0) kill(pid, SIGKILL);
                throw std::runtime_error(why);
            }

            // One request and its reply, answering the worker's asset and memory requests in between. Under control.
            Reader exchange(const std::string& message)
            {
                requests.fetch_add(1, std::memory_order_relaxed);
                try { send(socket, message); } catch(const std::exception&) { broken.store(true); throw; }
                for(;;)
                {
                    std::string reply;
                    try { reply = receive(socket); } catch(const WorkerTimeout& e) { lost(e); } catch(const std::exception&) { broken.store(true); throw; }
                    if(!reply.empty() && reply[0] == Reserve)
                    {
                        Reader r; r.data = std::move(reply); r.at = 1;
                        std::string answer(1, Done);
                        try { reserve(r); } catch(const std::exception& e) { answer = std::string(1, Failed) + e.what(); }
                        send(socket, answer);
                        continue;
                    }
                    if(reply.empty() || (reply[0] != ResolveAsset && reply[0] != CreateAsset && reply[0] != LibraryRoot && reply[0] != ResolveSample)) return expect(std::move(reply));
                    const auto kind = reply[0];
                    Reader r; r.data = std::move(reply); r.at = 1;
                    std::string answer;
                    try
                    {
                        // The store locks itself, so a restart may resolve from the supervisor thread.
                        auto* resolver = assets.load();
                        if(!resolver) throw std::runtime_error("No sound asset store");
                        if(kind == LibraryRoot)
                        {
                            const auto library = r.str(), minVersion = r.str();
                            answer = std::string(1, Done) + Writer().str(resolver->libraryRoot(library, minVersion)).data;
                        }
                        else if(kind == ResolveSample)
                        {
                            const auto at = resolver->resolveSample(r.str());
                            answer = std::string(1, Done) + Writer().str(at.pack).u64(at.offset).u64(at.length).u64(at.preload).data;
                        }
                        else if(kind == CreateAsset)
                        {
                            const auto ref = resolver->create(r.str());
                            answer = std::string(1, Done) + Writer().str(ref.library).str(ref.path).str(ref.sha256).data;
                        }
                        else
                        {
                            const auto library = r.str(), file = r.str(); auto sha256 = r.str();
                            const auto local = resolver->resolve(library, file, sha256);
                            answer = std::string(1, Done) + Writer().str(local).str(sha256).data;
                        }
                    }
                    catch(const std::exception& e) { answer = std::string(1, Failed) + e.what(); }
                    send(socket, answer);
                }
            }
            static Reader expect(std::string reply)
            {
                if(reply.empty()) throw std::runtime_error("Empty reply from plugin worker");
                if(reply[0] == Invalid) throw std::invalid_argument(reply.substr(1));
                if(reply[0] != Done) throw std::runtime_error(reply.substr(1));
                Reader r; r.data = std::move(reply); r.at = 1; return r;
            }

            std::string failure() const { std::lock_guard lock(failureMutex); return failureText; }
            void fail(const std::string& why)
            {
                { std::lock_guard lock(failureMutex); reason = why; failureText = why; }
                failed.store(true); failures.fetch_add(1, std::memory_order_relaxed);
                if(!reaped) { kill(pid, SIGKILL); int status = 0; waitpid(pid, &status, 0); reaped = true; }
            }

            // A node joins or leaves this worker; a worker with nodes is an audio thread (audioThreads).
            void adopt(WorkerHost* n)
            {
                std::lock_guard lock(nodesMutex);
                if(nodes.empty()) audioThreads.fetch_add(1, std::memory_order_relaxed);
                nodes.push_back(n);
            }
            void disown(WorkerHost* n)
            {
                std::lock_guard lock(nodesMutex);
                if(std::erase(nodes, n) && nodes.empty()) audioThreads.fetch_sub(1, std::memory_order_relaxed);
            }
            void supervise();
            bool exitedNow(); // checkWorkers
            void restart();
            void separate(std::vector<WorkerHost*>& list); // the plugin a shared host's failure is put down to, out to a worker of its own
            void moveOut(std::vector<WorkerHost*>& list, std::shared_ptr<const Engine> engine); // a plugin's nodes, out of the list to a worker of their own
            void fitLimit(bool fresh = false); // under control; fresh: a new process, held to its declarations at once
            // The limit for the nodes' declarations and one more node's (extra), or none where a node
            // declares nothing; ask: query a node whose declaration is not known (else none). Under control.
            std::optional<uint64_t> fitted(bool ask, const oscairn_memory_t* extra = nullptr);
            void reserve(Reader&); // a node about to activate, with what it declares; under control
            void raise();          // nodes about to prepare sounds, counted in their grown(); under control
            void credible(const oscairn_memory_t&) const; // throws for a declaration no worker could hold
            static uint64_t sum(uint64_t, uint64_t);      // throws on overflow, as a declaration not credible

            // RFC 0025, Linux: with a delegated cgroup v2 subtree (OSCAIRN_CGROUP, which the session
            // makes, RFC 0028), each worker in a cgroup of its own whose memory.max is its limit,
            // with no swap. Elsewhere, or without one, the supervisor's reading alone limits it.
            void joinCgroup()
            {
#ifdef __linux__
                leaveCgroup();
                const auto workers = workerCgroups();
                if(workers.empty()) return;
                cgroup = workers / std::to_string(pid);
                std::error_code e;
                std::filesystem::create_directory(cgroup, e);
                if(e || !put(cgroup / "cgroup.procs", std::to_string(pid))) { std::fprintf(stderr, "oscairn: worker %d not in its cgroup\n", int(pid)); cgroup.clear(); return; }
                put(cgroup / "memory.swap.max", "0"); // absent without swap accounting
                limitCgroup();
#endif
            }
            void limitCgroup()
            {
#ifdef __linux__
                if(!cgroup.empty()) put(cgroup / "memory.max", memoryLimit.load() ? std::to_string(memoryLimit.load()) : std::string("max"));
#endif
            }
            void leaveCgroup() // once its process is reaped
            {
#ifdef __linux__
                if(!cgroup.empty()) { std::error_code e; std::filesystem::remove(cgroup, e); cgroup.clear(); }
#endif
            }
#ifdef __linux__
            static bool put(const std::filesystem::path& file, const std::string& text) { std::ofstream out(file); out << text << std::flush; return bool(out); }
            // The engine moves itself into engine/ first: cgroup v2 keeps processes out of a cgroup
            // whose controllers are enabled for its children. Empty when there is no subtree.
            static std::filesystem::path workerCgroups()
            {
                static const std::filesystem::path dir = [] {
                    const char* root = std::getenv("OSCAIRN_CGROUP");
                    if(!root || !*root) return std::filesystem::path();
                    const std::filesystem::path r(root);
                    std::error_code e;
                    std::filesystem::create_directories(r / "engine", e);
                    std::filesystem::create_directories(r / "workers", e);
                    if(!put(r / "engine" / "cgroup.procs", std::to_string(getpid())) || !put(r / "cgroup.subtree_control", "+memory") ||
                       !put(r / "workers" / "cgroup.subtree_control", "+memory"))
                    {
                        std::fprintf(stderr, "oscairn: OSCAIRN_CGROUP %s is not a delegated cgroup v2 subtree; workers have no cgroups\n", root);
                        return std::filesystem::path();
                    }
                    return r / "workers";
                }();
                return dir;
            }
#endif
        };

        class WorkerHost final : public PluginHost
        {
        public:
            // Attaches a node the worker just described (index, description), of the engine a plugin loaded
            // (root) or one an effect context was made from. Under process->control.
            WorkerHost(std::shared_ptr<Process> worker, Reader& r, std::shared_ptr<const Engine> engine, bool root) : owner_(std::move(worker)), proc_(owner_.get()), engine_(std::move(engine)), root_(root)
            {
                node_ = r.u32();
                const auto at = r.at;
                descriptor_.path = r.str(); descriptor_.id = r.str(); descriptor_.name = r.str(); descriptor_.vendor = r.str();
                descriptor_.version = r.str(); descriptor_.description = r.str();
                for(auto n = r.u32(); n--;) descriptor_.features.push_back(r.str());
                for(auto n = std::min(r.u32(), 64u); n--;) { const auto lane = r.u32(); descriptor_.lanes.emplace_back(lane, r.str()); }
                parameters_ = readParameters(r);
                for(uint32_t i = 0; i < parameters_.size(); ++i) { index_[parameters_[i].id] = i; sentIds_.push_back(parameters_[i].id); }
                sentValues_.assign(parameters_.size() * 17, 0); sentModulation_.assign(sentValues_.size(), 0); sentKinds_.assign(sentValues_.size(), 0);
                inputs_ = readPorts(r); outputs_ = readPorts(r);
                noteInputs_ = r.u32(); noteOutputs_ = r.u32();
                for(uint32_t i = 0; i < noteInputs_; ++i) clapNotes_.push_back(r.u8());
                state_ = r.u8(); contexts_ = r.u32(); effects_ = r.u8();
                window_ = r.u8(); snapshots_ = r.u8(); ringOut_ = r.u8(); savesSounds_ = r.u8();
                voiceBudget_ = r.u8(); honorsVolume_ = r.u8();
                sysexIn_ = r.u8(); sysexOut_ = r.u8();
                wants_ = r.u8();
                description_ = r.data.substr(at, r.at - at);

                // Shared block, sized for the ports and maximum block, handed over as a descriptor.
                // Room for large blocks, a multiple of the plugin's maximum, so the worker's host
                // splits them exactly as an in-engine host would. The engine keeps it across restarts.
                const auto frames = process()->frames;
                capacity_ = frames * std::max<uint32_t>(1, (8192 + frames - 1) / frames);
                const uint32_t ring = ringOut_ ? 4 : 0; // the ring-out buffer, after the ports' channels
                bytes_ = Shared::bytes(channels(inputs_), channels(outputs_) + ring, capacity_, sysexIn_, sysexOut_);
                const std::string name = "/osc-" + std::to_string(getpid()) + "-" + std::to_string(Process::counter.fetch_add(1));
                try
                {
                    {
                        std::lock_guard descriptors(descriptorMutex());
                        shm_ = shm_open(name.c_str(), O_CREAT | O_EXCL | O_RDWR, 0600);
                        if(shm_ < 0) throw std::runtime_error("Cannot create worker shared memory");
                        fcntl(shm_, F_SETFD, FD_CLOEXEC); // other plugins' workers must not inherit it
                    }
                    shm_unlink(name.c_str());
                    if(ftruncate(shm_, off_t(bytes_)) != 0) throw std::runtime_error("Cannot size worker shared memory");
#ifdef __linux__
                    // Its pages taken now: with /dev/shm full the plugin is refused here, not the engine
                    // ended by SIGBUS at the first write to a page tmpfs cannot give.
                    if(const int e = posix_fallocate(shm_, 0, off_t(bytes_)); e != 0)
                        throw std::runtime_error(std::string("Cannot reserve worker shared memory: ") + std::strerror(e));
#endif
                    void* memory = mmap(nullptr, bytes_, PROT_READ | PROT_WRITE, MAP_SHARED, shm_, 0);
                    if(memory == MAP_FAILED) throw std::runtime_error("Cannot map worker shared memory");
                    dirtyPages(memory, bytes_); // no first write faults on the audio thread (Worker.h)
                    shared_ = new(memory) Shared();
                    inChannels_ = channels(inputs_); outChannels_ = channels(outputs_) + ring;
                    // SysEx pools only where the plugin can take (send) SysEx: 256 KiB each, locked on both sides.
                    size_t pools = Shared::audioEnd(inChannels_, outChannels_, capacity_);
                    if(sysexIn_) { shared_->inputPool = uint32_t(pools); new(static_cast<char*>(memory) + pools) kick::clap::SysexPool(); pools += sizeof(kick::clap::SysexPool); }
                    if(sysexOut_) { shared_->outputPool = uint32_t(pools); new(static_cast<char*>(memory) + pools) kick::clap::SysexPool(); }
                    if(sysexIn_) carriedBytes_ = std::make_unique<kick::clap::SysexPool>();
                    if(sysexOut_) sysexOutput_ = std::make_unique<kick::clap::SysexPool>();
                    shared_->inputChannels = inChannels_; shared_->outputChannels = outChannels_; shared_->ringOutChannels = ring;
                    shared_->maxFrames = capacity_; shared_->sampleRate = process()->rate; shared_->parameterCount = uint32_t(parameters_.size());
                    for(uint32_t i = 0; i < parameters_.size(); ++i) shared_->parameterValues[i].store(parameters_[i].defaultValue);
                    connect();
                    output_.resize(maxOutputEvents);
                    pending_.reserve(4096);
                    last_.assign(outChannels_, 0.f);
                    fadeFrames_ = faded_ = uint32_t(process()->rate * .01);
                    generation_ = process()->generation.load(); // a host restarted before this node came is not its restart
                    // RFC 0045's extra block, experimental: listed plugins in real time render each block while the
                    // engine plays their last, two buffers of the worker's block so a block that is not started
                    // never overwrites the one being played. Fixed for the node's life.
                    if(process()->mode == ProcessingMode::Realtime && listed(std::getenv("OSCAIRN_WORKER_EXTRA_BLOCK"), descriptor_.id))
                    {
                        extra_ = true;
                        if(sysexIn_) stagedBytes_ = std::make_unique<kick::clap::SysexPool>(); // the staged block's SysEx
                        held_.assign(size_t(2) * outChannels_ * frames, 0.f);
                        for(uint32_t c = 0; c < 2 * outChannels_; ++c) heldPointers_.push_back(held_.data() + size_t(c) * frames);
                        stagedIn_.assign(size_t(std::max(1u, inChannels_)) * frames, 0.f);
                        for(uint32_t c = 0; c < inChannels_; ++c) stagedInPointers_.push_back(stagedIn_.data() + size_t(c) * frames);
                    }
                    // The sounds present from the start keep their IDs.
                    if(contexts_) for(const auto& c : contexts()) remember(c, c.id);
                    publishIds();
                }
                catch(...) { release(); throw; }
                process()->adopt(this);
            }

            ~WorkerHost() override
            {
                {
                    auto control = locked();
                    process()->disown(this);
                    if(!process()->failed.load() && process()->socket >= 0)
                        try { process()->exchange(header(Op::DestroyNode).data); } catch(const std::exception&) {}
                }
                release();
            }

            // A restarted worker described this node again: attach it to the same shared block, the
            // new worker starting idle. Under control.
            void reconnect(Reader& reply)
            {
                node_ = reply.u32();
                if(reply.data.substr(reply.at) != description_) throw std::runtime_error("the plugin changed");
                grown_ = 0; // restore counts the sounds it prepares again
                auto& s = *shared_;
                s.panic.store(0); s.crashed.store(0); s.waiting.store(0);
                s.request.store(requested_.load()); s.done.store(requested_.load());
                connect();
            }

            // After a restart: the recovery record (state or snapshot), then the prepared sounds made
            // since and their selections, then the parameter values the old worker last published.
            // Under control.
            void restore()
            {
                if(!record_.empty()) process()->exchange(header(snapshotRecord_ ? Op::LoadSnapshot : Op::LoadState).str(record_.string()).data);
                if(contexts_)
                {
                    const auto present = contexts();
                    if(present.size() != base_.size()) throw std::runtime_error("the restored sounds differ");
                    std::map<uint32_t, uint32_t> map;
                    for(size_t i = 0; i < base_.size(); ++i) map[base_[i]] = present[i].id;
                    for(const auto id : discarded_) process()->exchange(header(Op::DiscardSound).u32(map.at(id)).u8(1).data);
                    for(const auto& p : prepared_)
                    {
                        const auto worker = prepare(p);
                        if(worker == CLAP_INVALID_ID) throw std::runtime_error("a prepared sound was rejected");
                        map[p.id] = worker;
                    }
                    // Snapshots prepared beside since the record (recordAsleep): prepared again, their new sounds in slot order as
                    // before, each one's sounds discarded since going at once, so they fit as they did. One that no longer fits
                    // (the record's sounds selected then, discarded since, cannot be discarded here: two changes with notes
                    // ringing before the plugin slept) is left out: its parts keep what their channels select, the instance plays.
                    for(const auto& b : pendingBeside_)
                    {
                        std::set<uint32_t> before; for(const auto& c : contexts()) before.insert(c.id);
                        try { process()->exchange(header(Op::PrepareSnapshot).str(b.directory).data); }
                        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: %s: a restarted worker left out the sounds prepared beside from %s: %s\n", descriptor_.id.c_str(), b.directory.c_str(), e.what()); continue; }
                        std::vector<uint32_t> fresh; for(const auto& c : contexts()) if(!before.count(c.id)) fresh.push_back(c.id);
                        std::sort(fresh.begin(), fresh.end());
                        if(fresh.size() != b.ids.size()) throw std::runtime_error("the sounds prepared beside differ");
                        for(size_t i = 0; i < fresh.size(); ++i) map[b.ids[i]] = fresh[i];
                        for(const auto id : b.gone) { process()->exchange(header(Op::DiscardSound).u32(map.at(id)).u8(1).data); map.erase(id); }
                    }
                    for(const auto id : discarded_) map.erase(id);
                    ids_ = map; publishIds();
                    for(int16_t c = 0; c < 16; ++c) if(auto i = ids_.find(selected_[c]); i != ids_.end())
                        process()->exchange(header(Op::SelectSound).u32(i->second).u32(uint32_t(c)).data);
                    fresh_ = true;
                }
                // Only values that differ from what the new worker published, to stay within its event queue.
                // None after a snapshot prepared beside: the values then published follow the selections
                // (the outgoing sound's, or a channel's), and as edits they would overwrite every selected sound.
                // Nor where the channels select different sounds (parts sharing the instance): the values published are
                // one sound's, and as edits to all they would overwrite the others'; what was sent to each channel is replayed instead.
                // shortcut: edits through setParameter (to all channels) made after such a switch, or before the instance came to be
                // shared, are lost; channel-addressed ones are replayed (start). Record them per sound if that matters.
                const bool oneSound = !contexts_ || std::all_of(selected_.begin(), selected_.end(), [&](uint32_t id) { return id == selected_[0]; });
                if(!besideRecord_ && oneSound) for(size_t i = 0; i < values_.size(); ++i)
                    if(!(parameters_[i].flags & CLAP_PARAM_IS_READONLY) && shared_->parameterValues[i].load() != values_[i])
                        process()->exchange(header(Op::SetParameter).u32(parameters_[i].id).f64(values_[i]).data);
                // The plugin's window, as the engine last asked; one that will not open does not stop the restart.
                // shortcut: a window the person closed in the old worker opens again; the worker does not report closing.
                if(!windowTitle_.empty()) try { process()->exchange(header(Op::Window).u8(1).str(windowTitle_).data); } catch(const std::exception&) {}
                if(activeOutputs_) process()->exchange(header(Op::ActiveOutputs).u64(*activeOutputs_).data); // the ports the engine takes sound from, as last told
                valueRevision_.fetch_add(1, std::memory_order_relaxed); // the new worker's values, as restored
            }
            bool hasWindow() const override { return window_; }
            void showWindow(bool show, const std::string& title) override
            {
                auto lock = controlFor();
                windowTitle_.clear();
                process()->exchange(header(Op::Window).u8(show).str(title).data);
                if(show) windowTitle_ = title.empty() ? descriptor_.name : title;
            }

            void capture() { values_.resize(parameters_.size()); for(size_t i = 0; i < values_.size(); ++i) values_[i] = shared_->parameterValues[i].load(); }
            bool busy() const { return busy_.load(); }
            bool isRoot() const { return root_; }
            const std::shared_ptr<const Engine>& engine() const { return engine_; }
            double lastBlockMicroseconds() const { return shared_->lastMicroseconds.load(std::memory_order_relaxed); } // the worker's word, for blame only
            // RFC 0045: into a worker of its own, which starts failed and is restarted with it. Under both
            // workers' control while the node is not busy (Process::restart).
            void moveTo(const std::shared_ptr<Process>& to)
            {
                process()->disown(this);
                owner_ = to; proc_.store(to.get(), std::memory_order_release);
                to->adopt(this);
            }
            // The supervisor's per-node checks: a worker marked as crashing, a block left unanswered
            // past the hang limit, or too many late ones in a row.
            std::string trouble(int64_t now) const
            {
                if(const auto signal = shared_->crashed.load()) return "Plugin worker crashed (signal " + std::to_string(signal) + ")";
                const auto since = requestedAt_.load(std::memory_order_acquire);
                const bool waiting = shared_->done.load(std::memory_order_acquire) != requested_.load(std::memory_order_acquire);
                if(waiting && since && std::chrono::nanoseconds(now - since) > process()->hangLimit)
                    return "Plugin hung: a block was not finished within " + std::to_string(std::chrono::duration_cast<std::chrono::milliseconds>(process()->hangLimit).count()) + " ms";
                if(tooSlow_.load(std::memory_order_acquire)) return "Plugin too slow: " + std::to_string(process()->missLimit) + " blocks in a row missed their deadline";
                return {};
            }
            void recovered() { tooSlow_.store(false); requestedAt_.store(0); }

            std::string failure() const override { return process()->failure(); }

            // Like the in-engine host's, never throws: a worker that fails here is the supervisor's to
            // handle, and throwing would stop the graph servicing its other nodes.
            void service() override
            {
                if(recordDue_.load(std::memory_order_relaxed)) try { auto lock = controlFor(); recordAsleep(); } catch(const std::exception&) {}
                if(const auto interval = workerServiceInterval(); interval.count())
                {
                    // A worker that has had nothing to say for a while (quietServices answers, 0.5 s at the usual 20 ms) is asked
                    // every quietFactor intervals, until it says something or is asked anything else: each is a request, an
                    // answer and two switches of the control thread, which 22 workers at rest spent most of a core on (RFC 0045).
                    const auto now = std::chrono::steady_clock::now();
                    if(now - serviced_ < (quiet_.load(std::memory_order_relaxed) >= quietServices ? quietFactor * interval : interval)) return;
                    serviced_ = now;
                }
                Reader r;
                try { r = request(Op::Service); } catch(const std::exception&) { return; }
                pending_.clear(); next_ = 0;
                for(auto n = r.u32(); n--;) { kick::clap::Event e; r.bytes(e.bytes.data(), e.bytes.size()); pending_.push_back(e); }
                const auto reread = r.u8(); // values the plugin's host read again (Worker.cpp's Service); 2: it cannot tell
                if(reread == 2) unreported_.store(true, std::memory_order_relaxed);
                else if(reread) valueRevision_.fetch_add(1, std::memory_order_relaxed);
                const bool rescanned = r.u8();
                if(pending_.empty() && !rescanned && !reread) { if(quiet_.load(std::memory_order_relaxed) < quietServices) quiet_.fetch_add(1, std::memory_order_relaxed); }
                else quiet_.store(0, std::memory_order_relaxed);
                if(!rescanned) return;
                // An info rescan (names, flags, hints) keeps the parameters and their order, which the
                // shared block's values and index_ rely on. Under control: a restart reads the flags.
                // shortcut: other changes are ignored until the plugin is loaded again; a restarted worker's own info shows only after its next rescan.
                try
                {
                    auto fresh = readParameters(r);
                    auto lock = controlFor();
                    if(!std::equal(fresh.begin(), fresh.end(), parameters_.begin(), parameters_.end(), [](const auto& a, const auto& b) { return a.id == b.id; })) return;
                    parameters_ = std::move(fresh);
                    revision_.fetch_add(1, std::memory_order_relaxed);
                }
                catch(const std::exception&) {}
            }
            uint64_t parameterRevision() const noexcept override { return revision_.load(std::memory_order_relaxed); }
            uint64_t valueRevision() const noexcept override { return unreported_.load(std::memory_order_relaxed) ? unreportedValues : valueRevision_.load(std::memory_order_relaxed); }
            bool supportsState() const noexcept override { return state_; }
            void saveState(const std::string& directory) override { request(Op::SaveState, Writer().str(directory)); }
            void loadState(const std::string& directory) override
            {
                auto lock = controlFor();
                // Its declaration stays known: a re-activation's Reserve replaces it, and a load refused
                // before one leaves the plugin as it was (RFC 0025).
                process()->exchange(header(Op::LoadState).str(directory).data);
                selections_[0].fetch_add(1, std::memory_order_release); // the state holds the values now
                valueRevision_.fetch_add(1, std::memory_order_relaxed);
                record(Op::SaveState);
                process()->fitLimit();
            }
            void saveSnapshot(const std::string& directory) override { request(Op::SaveSnapshot, Writer().str(directory)); }
            // The scan helper's listing (RFC 0005): the plugin's provider never runs in the worker,
            // where a hung one would restart a playing instrument, nor in this process. Without a
            // helper, none, saying why.
            std::vector<PresetInfo> presets() override
            {
                return scanPluginPresets(descriptor_.path, descriptor_.id);
            }
            // A preset changes the plugin's state, so the recovery record is saved again.
            void loadPreset(const std::string& uri) override
            {
                auto lock = controlFor();
                process()->exchange(header(Op::LoadPreset).str(uri).data);
                selections_[0].fetch_add(1, std::memory_order_release);
                valueRevision_.fetch_add(1, std::memory_order_relaxed);
                record(Op::SaveState);
            }
            // Only on a host nothing plays (not yet, or no longer: RFC 0021), so the record can be saved at once.
            void loadSnapshot(const std::string& directory) override
            {
                auto lock = controlFor();
                // Its declaration stays known: a re-activation's Reserve replaces it, and a load refused
                // before one leaves the plugin as it was (RFC 0025).
                process()->exchange(header(Op::LoadSnapshot).str(directory).data);
                selections_[0].fetch_add(1, std::memory_order_release);
                valueRevision_.fetch_add(1, std::memory_order_relaxed);
                if(contexts_)
                {
                    // The snapshot's sounds have fresh IDs.
                    ids_.clear(); base_.clear(); prepared_.clear(); discarded_.clear(); pendingBeside_.clear(); recordDue_.store(false, std::memory_order_relaxed); selected_.fill(UINT32_MAX);
                    for(const auto& c : contexts()) remember(c, allocate(c.id));
                }
                record(Op::SaveSnapshot);
                process()->fitLimit();
            }
            std::shared_ptr<PluginHost> createEffectContext() override
            {
                if(!effects_) throw std::runtime_error("Plugin has no native effect contexts");
                auto lock = controlFor();
                process()->making = engine_.get();
                struct Made { const Engine*& making; ~Made() { making = nullptr; } } made{process()->making};
                Reader r = process()->exchange(header(Op::CreateEffectContext).data);
                auto context = std::make_shared<WorkerHost>(owner_, r, engine_, false);
                process()->fitLimit();
                return context;
            }
            std::optional<oscairn_effect_resources_t> effectResources() const override
            {
                if(!effects_) return {};
                Reader r = const_cast<WorkerHost*>(this)->request(Op::EffectResources);
                if(!r.u8()) return {};
                oscairn_effect_resources_t res{}; r.bytes(&res, sizeof res); return res;
            }
            // What the worker said the plugin declared as it last activated (reserve), or asked of
            // it once after an operation that may have activated it, then kept.
            std::optional<oscairn_memory_t> declaredMemory() const override
            {
                auto lock = const_cast<WorkerHost*>(this)->controlFor();
                return declaredLocked();
            }
            const std::optional<oscairn_memory_t>& declaredLocked() const // under control
            {
                if(!declared_)
                {
                    Reader r = process()->exchange(header(Op::Memory).data);
                    std::optional<oscairn_memory_t> m;
                    if(r.u8()) { m.emplace(); r.bytes(&*m, sizeof *m); }
                    declared_ = m;
                }
                return *declared_;
            }
            // The declaration as known without asking the worker; null where it is not. Under control.
            const std::optional<oscairn_memory_t>* knownDeclaration() const { return declared_ ? &*declared_ : nullptr; }
            void declare(std::optional<oscairn_memory_t> m) { declared_ = m; grown_ = 0; } // under control; an estimate counts the sounds it has
            uint32_t grown() const { return grown_; }                          // under control
            void forget() { declared_.reset(); }                                // under control
            uint32_t workerNode() const { return node_; }                      // under control
            void setMemoryRoom(uint64_t bytes) override { request(Op::SetMemoryRoom, Writer().u64(bytes)); }
            void setActiveOutputs(uint64_t ports) override
            {
                auto lock = locked();
                activeOutputs_ = ports;
                if(!process()->failed.load()) try { process()->exchange(header(Op::ActiveOutputs).u64(ports).data); } catch(const std::exception&) {} // a worker down is told as it restarts (restore)
            }
            uint32_t soundContextCapacity() const noexcept override { return contexts_; }
            std::vector<oscairn_sound_context_info_t> soundContexts() const override
            {
                if(!contexts_) return {};
                auto& self = *const_cast<WorkerHost*>(this);
                auto lock = self.controlFor();
                std::vector<oscairn_sound_context_info_t> result;
                for(auto c : self.contexts())
                    for(const auto& [engine, worker] : ids_) if(worker == c.id) { c.id = engine; result.push_back(c); break; }
                return result;
            }
            uint32_t prepareSound(const std::string& name, const std::vector<oscairn_sound_parameter_t>& values, const std::string& state, const std::string& preset,
                                  const std::string& sfz, const std::string& library, const std::string& directory) override
            {
                auto lock = controlFor();
                Prepared p{0, name, values, state, preset, sfz, library, directory};
                const auto worker = prepare(p);
                if(!contexts_ || worker == CLAP_INVALID_ID) return worker;
                p.id = allocate(worker); ids_[p.id] = worker; publishIds();
                prepared_.push_back(std::move(p));
                return prepared_.back().id;
            }
            bool selectSound(uint32_t id, int16_t channel) override
            {
                auto lock = controlFor();
                const auto i = ids_.find(id);
                if(contexts_ && i == ids_.end()) { if(channel < -1 || channel > 15) throw std::invalid_argument("Invalid sound channel"); return false; }
                if(!process()->exchange(header(Op::SelectSound).u32(contexts_ ? i->second : id).u32(uint32_t(channel)).data).u8()) return false;
                if(channel < 0) selected_.fill(id); else selected_[size_t(channel)] = id;
                selections_[size_t(channel + 1)].fetch_add(1, std::memory_order_release); // what was sent to the channel was for its old sound
                return true;
            }
            bool discardSound(uint32_t id, bool cutTail) override
            {
                auto lock = controlFor();
                const auto i = ids_.find(id);
                if(contexts_ && i == ids_.end()) return false;
                if(!process()->exchange(header(Op::DiscardSound).u32(contexts_ ? i->second : id).u8(cutTail).data).u8()) return false;
                if(contexts_ && grown_) { --grown_; process()->fitLimit(); }
                ids_.erase(id); publishIds();
                // One the recovery record holds is discarded again after a restart; one from before a
                // snapshot prepared beside became the record is not in it.
                if(std::erase_if(prepared_, [&](const auto& p) { return p.id == id; }) == 0 && std::find(base_.begin(), base_.end(), id) != base_.end()) discarded_.push_back(id);
                for(auto& b : pendingBeside_) if(std::find(b.ids.begin(), b.ids.end(), id) != b.ids.end()) b.gone.push_back(id);
                return true;
            }
            // oscairn.voice-budget (RFC 0018): the limits ride the next block's request, the report
            // its answer, as kick::clap::Host takes and gives them. Audio thread.
            bool voiceBudget() const noexcept override { return voiceBudget_; }
            bool honorsVolume() const noexcept override { return honorsVolume_; }
            void setVoiceLimits(uint32_t limit, const oscairn_voice_budget_context_t* contexts, uint32_t count) noexcept override
            {
                if(!voiceBudget_) return;
                voiceLimit_ = limit; voiceContextCount_ = contexts ? std::min(count, OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS) : 0;
                std::copy_n(contexts, voiceContextCount_, voiceContexts_.begin());
                limitsWaiting_ = true;
            }
            uint32_t voiceReport(double& voiceCost, const oscairn_voice_budget_note_t*& notes, uint32_t& count) const noexcept override
            {
                voiceCost = voiceCost_; notes = voiceNotes_.data(); count = voiceNoteCount_; return activeVoices_;
            }
            uint32_t importanceReport(const oscairn_voice_budget_importance_t*& lists) const noexcept override
            {
                lists = importance_.data(); return importanceCount_;
            }
            bool preparesSnapshots() const noexcept override { return snapshots_; }
            bool savesSounds() const noexcept override { return savesSounds_; }
            void saveSound(uint32_t id, const std::string& directory) override
            {
                if(!savesSounds_) throw std::invalid_argument("no such sound to save on its own");
                auto lock = locked();
                const auto i = ids_.find(id);
                if(i == ids_.end()) throw std::invalid_argument("no such sound to save on its own");
                if(process()->failed.load()) throw std::runtime_error(process()->failure());
                process()->exchange(header(Op::SaveSound).u32(i->second).str(directory).data);
            }
            std::vector<oscairn_sound_parameter_t> soundValues(uint32_t id) const override
            {
                if(!savesSounds_) return {};
                auto& self = const_cast<WorkerHost&>(*this);
                auto lock = self.locked();
                const auto i = ids_.find(id);
                if(i == ids_.end()) return {};
                if(self.process()->failed.load()) throw std::runtime_error(self.process()->failure());
                Reader r = self.process()->exchange(header(Op::SoundValues).u32(i->second).data);
                std::vector<oscairn_sound_parameter_t> values(r.count(12));
                for(auto& v : values) { v.id = r.u32(); v.value = r.f64(); }
                return values;
            }
            bool ringsOut() const noexcept override { return ringOut_; }
            bool chokeRetiring() override
            {
                auto lock = controlFor();
                return ringOut_ && process()->exchange(header(Op::ChokeRetiring).data).u8();
            }
            std::array<uint32_t, 16> prepareSnapshot(const std::string& directory) override
            {
                auto lock = controlFor();
                std::set<uint32_t> before;
                for(const auto& c : contexts()) before.insert(c.id);
                // How many sounds the snapshot holds is the plugin's to read: room for every free context until it says.
                const auto free = contexts_ - uint32_t(before.size());
                Reader r = growing(free, [&] { return process()->exchange(header(Op::PrepareSnapshot).str(directory).data); });
                valueRevision_.fetch_add(1, std::memory_order_relaxed); // the worker published values after it
                std::array<uint32_t, 16> worker{};
                for(auto& id : worker) id = r.u32();
                // The new sounds in the order a load of the snapshot lists them, its slots': their IDs ascend.
                std::vector<uint32_t> fresh;
                for(const auto& c : contexts()) if(!before.count(c.id)) fresh.push_back(c.id);
                grown_ = grown_ - free + uint32_t(fresh.size());
                process()->fitLimit();
                std::sort(fresh.begin(), fresh.end());
                std::map<uint32_t, uint32_t> engine;
                for(const auto w : fresh) { const auto id = allocate(w); ids_[id] = w; engine[w] = id; }
                publishIds();
                std::array<uint32_t, 16> selected{};
                for(size_t ch = 0; ch < 16; ++ch)
                {
                    const auto e = engine.find(worker[ch]);
                    if(e == engine.end()) throw std::runtime_error("the worker's prepared snapshot selects a sound it did not prepare");
                    selected[ch] = e->second;
                }
                // The recovery record becomes the whole instance as it is now, the sounds already playing
                // and the incoming ones: on a shared instance the others' sounds stay, so a copy of this
                // snapshot alone would restore one node's sounds on every channel. The selections and each
                // shared sound's values are replayed after it. If the save fails, the old record stays.
                // Saved only while the plugin sleeps: a whole-instance save excludes its rendering, and a block it
                // misses so is reset for, which ended the last Performance's notes ringing in it (2026-11-21). Until
                // then a restart prepares this snapshot again on the old record (restore).
                pendingBeside_.push_back({directory, {}});
                for(const auto w : fresh) pendingBeside_.back().ids.push_back(engine.at(w));
                recordDue_.store(true, std::memory_order_relaxed);
                recordAsleep();
                return selected;
            }
            // The deferred recovery record of the snapshots prepared beside, saved once the plugin sleeps (nothing
            // sounds in it, nothing renders). Under control.
            void recordAsleep()
            {
                if(pendingBeside_.empty() || process()->failed.load() || !shared_->asleep.load(std::memory_order_acquire)) return;
                // No block handed from here (start), none in flight: the save never meets a block.
                struct Saving { std::atomic<bool>& flag; ~Saving() { flag.store(false); } } saving{saving_};
                saving_.store(true);
                const auto until = std::chrono::steady_clock::now() + std::chrono::milliseconds(50);
                while(busy_.load()) { if(std::chrono::steady_clock::now() > until) return; std::this_thread::sleep_for(std::chrono::microseconds(100)); }
                if(!shared_->asleep.load(std::memory_order_acquire)) return; // woken by a block that came meanwhile
                const auto was = record_;
                record(Op::SaveSnapshot);
                if(record_ == was) return;
                base_.clear(); prepared_.clear(); discarded_.clear(); pendingBeside_.clear(); recordDue_.store(false, std::memory_order_relaxed);
                for(const auto& c : contexts()) for(const auto& [engine, worker] : ids_) if(worker == c.id) { base_.push_back(engine); break; }
                besideRecord_ = true;
            }
            void setAssets(kick::clap::AssetResolver* assets) noexcept override { process()->assets.store(assets); }
            std::vector<kick::clap::AssetReference> assetDependencies() const override
            {
                Reader r = const_cast<WorkerHost*>(this)->request(Op::AssetDependencies);
                std::vector<kick::clap::AssetReference> refs(r.count(13));
                for(auto& a : refs) { a.library = r.str(); a.path = r.str(); a.sha256 = r.str(); a.optional = r.u8() != 0; }
                return refs;
            }
            const kick::clap::Descriptor& descriptor() const noexcept override { return descriptor_; }
            const std::vector<kick::clap::Parameter>& parameters() const noexcept override { return parameters_; }
            const std::vector<kick::clap::Port>& inputPorts() const noexcept override { return inputs_; }
            const std::vector<kick::clap::Port>& outputPorts() const noexcept override { return outputs_; }
            uint32_t noteInputPorts() const noexcept override { return noteInputs_; }
            uint32_t noteOutputPorts() const noexcept override { return noteOutputs_; }
            bool usesClapNotes(uint32_t port) const noexcept override { return port < clapNotes_.size() && clapNotes_[port]; }
            bool takesSysex() const noexcept override { return sysexIn_; }
            uint32_t wantsEvents() const noexcept override { return wants_; }
            bool emitsSysex() const noexcept override { return sysexOut_; }
            // While down, a panic would reach the restarted worker and drop its first block's events.
            // With the extra block the worker may still be rendering the last block, which a panic now must not reach: it waits for the next one.
            void panic() noexcept override
            {
                if(process()->failed.load()) return;
                if(missed_.load(std::memory_order_relaxed)) missPanics_.fetch_add(1, std::memory_order_relaxed);
                if(extra_) panicLater_.store(true, std::memory_order_release); else shared_->panic.store(1, std::memory_order_release);
                if(replayUnconfirmed_.load(std::memory_order_relaxed)) replayAgain_.store(true, std::memory_order_relaxed); // it may have dropped the replay
            }
            bool inWorker() const noexcept override { return true; }
            bool reset() override { return request(Op::Reset).u8() != 0; }
            // Every node of the worker is reconfigured by its own call, each setting the worker's rate.
            void reconfigure(double rate, uint32_t frames) override
            {
                if(frames != process()->frames) throw std::invalid_argument("A plugin worker keeps its block size"); // its shared block's
                { auto lock = locked(); process()->setRate(rate); } // first, so a worker restarting meanwhile loads at it
                shared_->sampleRate = rate;
                fadeFrames_ = faded_ = uint32_t(rate * .01);
                request(Op::Reconfigure, Writer().f64(rate));
            }
            // Its share of the CPU time of this worker and those it replaced, as the supervisor last read it.
            double cpuSeconds() const override
            {
                auto* p = process();
                return (p->cpuBefore.load() + p->cpu.load()) / double(sharing(*p));
            }
            // Its share of the worker's memory as the supervisor last read it, the measure its limit applies to.
            uint64_t residentBytes() const override
            {
                auto* p = process();
                const auto bytes = p->memory.load();
                if(!bytes) return resident_.load(); // restarting: the last reading stands
                const auto share = bytes / sharing(*p);
                resident_.store(share);
                return share;
            }
            // The nodes on worker p, under its nodesMutex, which adopt and disown write under: p read once,
            // as moveTo may point this node at another worker meanwhile (its old one's lock then guards nothing here).
            static size_t sharing(Process& p) { std::lock_guard nodes(p.nodesMutex); return std::max<size_t>(1, p.nodes.size()); }
            // As the plugin said in its last block; a failed worker is silent, so asleep too.
            bool asleep() const noexcept override { return process()->failed.load() || shared_->asleep.load(std::memory_order_relaxed) != 0; }
            // A block its plugin would sleep through, taken without the round trip (RFC 0045): asleep after its last
            // block with nothing pending there (restful) and no wake since (a select, choke, command or request_process
            // reaching it sets the wake word; a panic here waits in the shared block). Never a block a call late, or a
            // worker down. The frames go to the worker with the next block, for its steady time. Only after restHold such
            // blocks in a row called: a chain fed a moment ago (an arpeggio's gaps, a phrase's) stays warm, and waking a
            // rested one (its first block after a long sleep costs a cold wake-up, worker-wake) comes only after real silence.
            bool rest(uint32_t frames) noexcept override
            {
                auto& s = *shared_;
                if(!workerRest || extra_ || process()->failed.load() || busy_.load() || panicLater_.load(std::memory_order_relaxed) || carriedCount_ || replay_ ||
                   s.done.load(std::memory_order_acquire) != sequence_ || !s.restful.load(std::memory_order_acquire) || s.wake.load(std::memory_order_acquire) ||
                   s.panic.load(std::memory_order_acquire))
                    return false;
                if(restful_ < restHold) { ++restful_; holding_ = true; return false; }
                outputCount_ = 0; if(sysexOutput_) sysexOutput_->used = 0; reset_ = false; kept_ = playedKept_ = true;
                ++restedBlocks_; restedFrames_ += frames; rested_.store(rested_.load(std::memory_order_relaxed) + 1, std::memory_order_relaxed); // one writer: no locked add
                return true;
            }
            bool wasReset() const noexcept override { return reset_; }
            // RFC 0045's miss policy: a block handed but not done in time, or not handed while the worker was busy
            // with its events carried, lost nothing; with the extra block, of the block played.
            bool eventsKept() const noexcept override { return extra_ ? playedKept_ : kept_; }
            bool oneBlockLate() const noexcept override { return extra_; }
            const void* lane() const noexcept override { return proc_; } // the worker process's audio thread, shared in a plugin host
            long workerPid() const override { auto* p = process(); std::lock_guard control(p->control); return p->reaped ? 0 : long(p->pid); }
            double parameterValue(clap_id id) const noexcept override
            {
                const auto i = index_.find(id);
                return i == index_.end() ? 0.0 : shared_->parameterValues[i->second].load(std::memory_order_relaxed);
            }
            std::string parameterText(clap_id id, double value) const override
            {
                return const_cast<WorkerHost*>(this)->request(Op::ParameterText, Writer().u32(id).f64(value)).str();
            }
            bool setParameter(clap_id id, double value) noexcept override
            {
                try { return request(Op::SetParameter, Writer().u32(id).f64(value)).u8(); } catch(const std::exception&) { return false; }
            }
            bool sendEvent(const kick::clap::Event& e) noexcept override
            {
                try { return request(Op::Event, Writer().bytes(e.bytes.data(), e.bytes.size())).u8(); } catch(const std::exception&) { return false; }
            }
            bool parameterGesture(clap_id id, bool begin) noexcept override
            {
                try { return request(Op::Gesture, Writer().u32(id).u8(begin)).u8(); } catch(const std::exception&) { return false; }
            }
            bool popOutput(kick::clap::Event& event) noexcept override
            {
                if(next_ >= pending_.size()) return false;
                event = pending_[next_++]; return true;
            }
            uint64_t outputEventDrops() const noexcept override { return shared_->outputDrops.load(std::memory_order_relaxed); }
            const kick::clap::Event* outputEvents(uint32_t& count) const noexcept override { count = outputCount_; return output_.data(); }
            WorkerTiming* workerTiming() noexcept override { return &timing_; }
            // The budget's every block: the one value, not statistics()' gathering of every counter.
            double blockMicroseconds() const noexcept override { return shared_->lastMicroseconds.load(std::memory_order_relaxed); }
            // The worker's stamps for the block last handed, once recorded for it (audio thread, as workerTimes).
            bool blockTimes(float& cpu, float& wait) const noexcept override
            {
                const bool known = timed_ == sequence_ && sequence_ != 0;
                cpu = known ? float(lastCpuNs_) / 1000.f : -1.f; wait = known ? float(lastWaitNs_) / 1000.f : -1.f;
                return known;
            }
            uint64_t missedBlocks() const noexcept override { return late_.load(std::memory_order_relaxed) + silent_.load(std::memory_order_relaxed); }
            kick::clap::Statistics statistics() const noexcept override
            {
                kick::clap::Statistics s;
                s.blocks = shared_->blocks.load(); s.droppedEvents = shared_->droppedEvents.load() + sysexDrops_.load(std::memory_order_relaxed); s.processErrors = shared_->processErrors.load();
                s.droppedEvents += carryDrops_.load(std::memory_order_relaxed);
                s.lastMicroseconds = shared_->lastMicroseconds.load(); s.maximumMicroseconds = shared_->maximumMicroseconds.load();
                s.cpuMicroseconds = shared_->cpuMicroseconds.load(std::memory_order_relaxed);
                s.lateBlocks = late_.load(std::memory_order_relaxed); s.silentBlocks = silent_.load(std::memory_order_relaxed);
                s.deadlineMisses = shared_->deadlineMisses.load() + s.lateBlocks + s.silentBlocks;
                s.allowanceMicroseconds = std::chrono::duration<double, std::micro>(process()->budget()).count();
                s.busyBlocks = busyBlocks_.load(std::memory_order_relaxed); s.missPanics = missPanics_.load(std::memory_order_relaxed);
                s.minorFaults = shared_->minorFaults.load(std::memory_order_relaxed); s.majorFaults = shared_->majorFaults.load(std::memory_order_relaxed);
                s.involuntarySwitches = shared_->involuntarySwitches.load(std::memory_order_relaxed);
                s.controlSilences = shared_->controlSilences.load(); s.idleBlocks = shared_->idleBlocks.load();
                s.restarts = process()->restarted.load(std::memory_order_relaxed);
                s.calledBlocks = called_.load(std::memory_order_relaxed); s.restedBlocks = rested_.load(std::memory_order_relaxed);
                return s;
            }

            InputEvents inputEvents(bool reset) noexcept override
            {
                return {events_.load(std::memory_order_relaxed), reset ? mostEvents_.exchange(0, std::memory_order_relaxed) : mostEvents_.load(std::memory_order_relaxed)};
            }

            bool process(const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames,
                         const kick::clap::Event* events, uint32_t count, const clap_event_transport_t* transport) noexcept override
            {
                outputCount_ = 0; if(sysexOutput_) sysexOutput_->used = 0; reset_ = false; missed_.store(false, std::memory_order_relaxed); kept_ = playedKept_ = true;
                if(frames <= capacity_) return rendered(block(inputs, ni, outputs, no, frames, events, count, transport, 0));
                // Larger than the shared buffer: split as the in-engine host does, rebasing events,
                // transport and output event times.
                if(count > maxEvents || (count && !events) || (no && !outputs) || ni > 64 || no > 64)
                {
                    if(outputs) for(uint32_t c = 0; c < no; ++c) if(outputs[c]) std::fill_n(outputs[c], frames, 0.f);
                    return rendered(false);
                }
                const float* in[64]; float* out[64]; bool ok = true;
                const double rate = process()->rate;
                for(uint32_t done = 0; done < frames;)
                {
                    const auto n = std::min(capacity_, frames - done);
                    uint32_t chunkCount = 0;
                    for(uint32_t i = 0; i < count; ++i)
                    {
                        const auto t = events[i].header()->time;
                        if(t < done || t >= done + n) continue;
                        chunk_[chunkCount] = events[i];
                        reinterpret_cast<clap_event_header_t*>(chunk_[chunkCount++].bytes.data())->time -= done;
                    }
                    for(uint32_t c = 0; c < ni; ++c) in[c] = inputs && inputs[c] ? inputs[c] + done : nullptr;
                    for(uint32_t c = 0; c < no; ++c) out[c] = outputs[c] ? outputs[c] + done : nullptr;
                    clap_event_transport_t position{};
                    if(transport)
                    {
                        position = *transport;
                        if((position.flags & CLAP_TRANSPORT_IS_PLAYING) && (position.flags & CLAP_TRANSPORT_HAS_SECONDS_TIMELINE))
                            position.song_pos_seconds += clap_sectime((double(done) / rate) * CLAP_SECTIME_FACTOR);
                        if((position.flags & CLAP_TRANSPORT_IS_PLAYING) && (position.flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE) && (position.flags & CLAP_TRANSPORT_HAS_TEMPO))
                            position.song_pos_beats += clap_beattime((double(done) / rate) * position.tempo / 60. * CLAP_BEATTIME_FACTOR);
                    }
                    ok = block(inputs ? in : nullptr, ni, out, no, n, chunk_.data(), chunkCount, transport ? &position : nullptr, done) && ok;
                    if(reset_ && done + n < frames) panic(); // the later chunks' notes end too, as wasReset says
                    done += n;
                }
                return rendered(ok);
            }
            // A block larger than the shared buffer is split, so it is not started: process() renders it.
            bool startProcess(const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames,
                              const kick::clap::Event* events, uint32_t count, const clap_event_transport_t* transport) noexcept override
            {
                if(!frames || frames > capacity_) return false;
                outputCount_ = 0; if(sysexOutput_) sysexOutput_->used = 0; reset_ = false; kept_ = playedKept_ = true;
                if(extraBlock()) { startDelayed(inputs, ni, outputs, no, frames, events, count, transport, 0); return true; }
                started_ = start(inputs, ni, outputs, no, frames, events, count, transport, 0);
                return true;
            }
            bool finishProcess() noexcept override { return rendered(extraBlock() ? finishDelayed() : started_ && finish()); }
            bool groupable() const noexcept override { return !extra_; }
            bool handed() const noexcept override { return started_ && busy_.load(std::memory_order_relaxed); }
            // RFC 0045's plugin groups: fused when a source was handed this block (each source not handed has its engine
            // output, final by now, written into its shared output for the worker to mix); standalone, the engine's input,
            // when none was; lost, as a busy block, when a source is still busy with an old block or no longer in this
            // worker (a crash moved it).
            bool startGrouped(const GroupRoute* routes, uint32_t n, const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames,
                              const kick::clap::Event* events, uint32_t count, const clap_event_transport_t* transport) noexcept override
            {
                if(n > maxFused || std::none_of(routes, routes + n, [](const GroupRoute& r) { return r.source->handed(); }))
                    return startProcess(inputs, ni, outputs, no, frames, events, count, transport);
                bool lost = false;
                for(uint32_t i = 0; i < n; ++i)
                {
                    const auto& source = *static_cast<const WorkerHost*>(routes[i].source); // groupable: a WorkerHost
                    if(source.process() != process() || (!source.handed() && source.shared_->done.load(std::memory_order_acquire) != source.sequence_)) lost = true;
                }
                if(!lost)
                    for(uint32_t i = 0; i < n; ++i)
                    {
                        auto& source = *const_cast<WorkerHost*>(static_cast<const WorkerHost*>(routes[i].source));
                        if(source.handed() || std::any_of(routes, routes + i, [&](const GroupRoute& r) { return r.source == routes[i].source; })) continue;
                        for(uint32_t c = 0; c < source.outChannels_; ++c)
                            if(c < routes[i].sourceChannels && routes[i].sourceOutputs[c]) std::copy_n(routes[i].sourceOutputs[c], frames, source.audioOut(c));
                            else std::fill_n(source.audioOut(c), frames, 0.f);
                        source.shared_->published = source.shared_->published + 1;
                    }
                group_ = routes; groupCount_ = n; groupLost_ = lost;
                const bool started = startProcess(inputs, ni, outputs, no, frames, events, count, transport);
                group_ = nullptr; groupCount_ = 0; groupLost_ = false;
                return started;
            }

        private:
            struct Prepared
            {
                uint32_t id; // the engine's ID for it
                std::string name; std::vector<oscairn_sound_parameter_t> values;
                std::string state, preset, sfz, library, directory;
            };

            // What the caller is told of the block, remembered so a panic right after a block not rendered is counted.
            bool rendered(bool ok) noexcept { missed_.store(!ok, std::memory_order_relaxed); return ok; }
            // The worker's stamps for the block last handed, once it is done: recorded once, in finish or at the next hand-over.
            void workerTimes(const Shared& s, std::chrono::steady_clock::time_point now) noexcept
            {
                if(timed_ == sequence_) return;
                timed_ = sequence_;
                const int64_t sent = std::chrono::nanoseconds(sent_.time_since_epoch()).count(), at = std::chrono::nanoseconds(now.time_since_epoch()).count();
                timing_.wake.record(s.wokeAt - sent, at);
                timing_.run.record(s.doneAt - s.wokeAt, at, s.cpuNs);
                timing_.cpu.record(s.cpuNs, at);
                lastWaitNs_ = std::clamp<int64_t>(s.wokeAt - sent, 0, 1'000'000'000); lastCpuNs_ = std::clamp<int64_t>(s.cpuNs, 0, 1'000'000'000);
                // What a block usually takes in the worker (an eighth of each new one), for the extra block's catching up; the worker's stamps bounded.
                const auto run = std::clamp<int64_t>(s.doneAt - s.wokeAt, 0, 1'000'000'000);
                runNs_ += (run - runNs_) / 8;
            }
            // The control lock of the worker this node is in. A node may move to a worker of its own while
            // its worker restarts (RFC 0045), so the worker is looked up again once the lock is held.
            std::unique_lock<std::mutex> locked()
            {
                for(;;)
                {
                    auto* p = process();
                    std::unique_lock lock(p->control);
                    if(p == process()) return lock;
                }
            }
            std::unique_lock<std::mutex> controlFor()
            {
                if(process()->failed.load()) throw std::runtime_error(process()->failure());
                auto lock = locked();
                if(process()->failed.load()) throw std::runtime_error(process()->failure());
                return lock;
            }
            // Anything asked of the worker that changes it, not its service nor a plain question, ends the quiet that spaces the
            // services out (service).
            Writer header(Op op) const
            {
                if(op != Op::Service && op != Op::SoundContexts && op != Op::ParameterText && op != Op::SoundValues) quiet_.store(0, std::memory_order_relaxed);
                return Writer().u8(uint8_t(op)).u32(node_);
            }
            Reader request(Op op, const Writer& args = {})
            {
                auto lock = controlFor();
                return process()->exchange(header(op).data + args.data);
            }
            // The worker's sound contexts with its own IDs, in slot order. Under control.
            std::vector<oscairn_sound_context_info_t> contexts()
            {
                Reader r = process()->exchange(header(Op::SoundContexts).data);
                std::vector<oscairn_sound_context_info_t> result(r.count(sizeof(oscairn_sound_context_info_t)));
                for(auto& c : result) r.bytes(&c, sizeof c);
                return result;
            }
            uint32_t prepare(const Prepared& p)
            {
                Writer w = header(Op::PrepareSound); w.str(p.name).u32(uint32_t(p.values.size()));
                for(const auto& v : p.values) w.u32(v.id).f64(v.value);
                w.str(p.state).str(p.preset).str(p.sfz).str(p.library).str(p.directory);
                valueRevision_.fetch_add(1, std::memory_order_relaxed); // the worker publishes values after it
                if(!contexts_) return process()->exchange(w.data).u32();
                const auto id = growing(1, [&] { return process()->exchange(w.data); }).u32();
                if(id == CLAP_INVALID_ID) --grown_;
                process()->fitLimit();
                return id;
            }
            // RFC 0025: a sound context prepared on the running engine costs the context bytes the
            // plugin declared (memory.h), so its worker's limit is raised for `most` more before the
            // plugin allocates; the caller counts those it made and refits. Under control.
            template<class F> Reader growing(uint32_t most, F prepare)
            {
                grown_ += most;
                try { process()->raise(); return prepare(); }
                catch(...) { grown_ -= most; try { process()->fitLimit(); } catch(const std::exception&) {} throw; }
            }
            // Engine sound IDs: the worker's own until a restart, then new ones; never reused.
            uint32_t allocate(uint32_t worker) { const auto id = fresh_ ? nextId_ : worker; nextId_ = std::max(nextId_, id + 1); return id; }
            void remember(const oscairn_sound_context_info_t& c, uint32_t id)
            {
                ids_[id] = c.id; base_.push_back(id); nextId_ = std::max(nextId_, id + 1);
                for(size_t ch = 0; ch < 16; ++ch) if(c.selected_channels & (1u << ch)) selected_[ch] = id;
            }
            // The recovery record, saved by the engine each time state or a snapshot is loaded. If a
            // save fails, the previous record stays. Under control.
            void record(Op save)
            {
                if(save == Op::SaveState && !state_) return;
                try
                {
                    const auto next = process()->records / std::to_string(++process()->saves);
                    process()->exchange(header(save).str(next.string()).data);
                    std::error_code ignored;
                    if(!record_.empty()) std::filesystem::remove_all(record_, ignored);
                    record_ = next; snapshotRecord_ = save == Op::SaveSnapshot; besideRecord_ = false;
                }
                catch(const std::exception&) {}
            }
            // Hands the worker this node's shared block, which it maps, then answers. Under control.
            void connect()
            {
                send(process()->socket, Writer().u8(1).data);
                sendDescriptor(process()->socket, shm_);
                Process::expect(receive(process()->socket));
            }
            void release() noexcept
            {
                if(shared_) { munmap(shared_, bytes_); shared_ = nullptr; }
                if(shm_ >= 0) { close(shm_); shm_ = -1; }
                std::error_code ignored;
                if(!record_.empty()) std::filesystem::remove_all(record_, ignored);
            }

            // One block that fits the shared buffer. Its output events are appended to this
            // call's, their times moved by `offset`, the frames already rendered in the call.
            bool block(const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames,
                       const kick::clap::Event* events, uint32_t count, const clap_event_transport_t* transport, uint32_t offset) noexcept
            {
                if(!frames) return true;
                if(extraBlock()) { startDelayed(inputs, ni, outputs, no, frames, events, count, transport, offset); return finishDelayed(); }
                return start(inputs, ni, outputs, no, frames, events, count, transport, offset) && finish();
            }
            // block's halves: start hands the block to the worker (false: silenced, nothing started);
            // finish waits for it and takes its outputs.
            bool start(const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames,
                       const kick::clap::Event* events, uint32_t count, const clap_event_transport_t* transport, uint32_t offset) noexcept
            {
                auto silence = [&] {
                    fadeOut(outputs, no, frames);
                    busy_.store(false);
                    return false;
                };
                // While busy (until finish), a restart waits; it sets failed first, so a block that starts later sees it.
                busy_.store(true);
                if(!holding_) restful_ = 0; // a block it would not have slept through: the hold starts again
                holding_ = false;
                auto& s = *shared_;
                if(events && count <= maxEvents) track(events, count); // also while it is down: the restart replays them
                if(process()->failed.load() || frames > capacity_ || count > maxEvents || (count && !events) || (no && !outputs)) { kept_ = false; carriedCount_ = 0; if(carriedBytes_) carriedBytes_->used = 0; return silence(); }
                process()->follow(frames);
                if(const auto g = process()->generation.load(); g != generation_) { generation_ = g; fadeFrames_ = uint32_t(process()->rate * .01); faded_ = 0; consecutiveMisses_ = 0; limitsWaiting_ = voiceBudget_; timed_ = sequence_; abandoned_ = false; timing_.warmUp(); noGrace_ = !process()->restartGrace.load(); replay_ = true; } // restarted: the shared block holds the old worker's stamps
                // Loaded, restarted, or taken over by a Performance being opened (each a warmUp): its first two seconds' late
                // blocks are its start's (samples read, pages touched), silenced as any, but none counts toward "too slow",
                // so lateness while loading never restarts it. A hang is still a hang. Not every restart within a minute:
                // only the first has it, so a plugin too slow in itself is given up in about 5 s.
                if(const auto n = timing_.starts.load(std::memory_order_relaxed); n != startsSeen_) { startsSeen_ = n; graceLeft_ = noGrace_ ? 0 : uint32_t(2 * process()->rate / std::max(1u, frames)); noGrace_ = false; consecutiveMisses_ = 0; }
                else if(graceLeft_) --graceLeft_;
                // While the deferred recovery record is saved (recordAsleep), nothing is handed: the plugin asleep, its whole state
                // saved excluding rendering, a block handed would be a control silence and reset it, a note struck then lost.
                // Its silence is the plugin's own; the events are carried, so the note comes after the save.
                if(saving_.load())
                {
                    for(uint32_t i = 0; i < count; ++i) if(!carry(events[i], true)) kept_ = false;
                    return silence();
                }
                // A block the worker has not finished yet: stay silent rather than queue another, its events carried
                // to the next block handed (RFC 0045); only a release lost on the way is reset for.
                if(s.done.load(std::memory_order_acquire) != sequence_ || groupLost_)
                {
                    silent_.fetch_add(1, std::memory_order_relaxed); busyBlocks_.fetch_add(1, std::memory_order_relaxed);
                    for(uint32_t i = 0; i < count; ++i) if(!carry(events[i], true)) kept_ = false;
                    return silence();
                }
                replayUnconfirmed_.store(false, std::memory_order_relaxed); // the block with the replay is done: a panic since did not reach it
                workerTimes(s, std::chrono::steady_clock::now()); // a block the engine gave up on, done since
                // Its sound is gone, its output events come now, at this block's start; one the worker's host did not
                // render (a control silence there) lost its events, so the plugin is reset before this block.
                if(abandoned_)
                {
                    abandoned_ = false;
                    takeOutputs(s, offset, true);
                    reset_ = reset_ || s.reset != 0;
                    if(!s.ok) s.panic.store(1, std::memory_order_relaxed);
                }
                // The events carried, before this block's, all bounded as one block's.
                if(carriedCount_)
                {
                    bool lost = false;
                    for(uint32_t i = 0; i < count; ++i) lost = !carry(events[i], false) || lost;
                    if(lost) s.panic.store(1, std::memory_order_relaxed); // the request below publishes it
                    events = carried_.data(); count = carriedCount_;
                }
                // In a group's block the worker mixes the input from the other nodes' outputs (RFC 0045's plugin groups).
                s.fusedCount = groupCount_;
                for(uint32_t i = 0; i < groupCount_; ++i)
                {
                    const auto& r = group_[i];
                    const auto& source = *static_cast<const WorkerHost*>(r.source);
                    s.fused[i] = {source.node_, r.from, r.to, r.live, r.first, !source.handed(), r.gain, r.start, r.end};
                }
                if(!groupCount_)
                    for(uint32_t c = 0; c < inChannels_; ++c)
                    {
                        if(inputs && c < ni && inputs[c]) std::copy_n(inputs[c], frames, audioIn(c));
                        else std::fill_n(audioIn(c), frames, 0.f);
                    }
                uint32_t kept = 0;
                if(replayAgain_.exchange(false, std::memory_order_relaxed)) replay_ = true;
                // A panic drops a block's controllers, the replay with them: one reaching this block (or the block it was in) has it sent again.
                if(replay_) { track(nullptr, 0); kept = replay(s.events, 0, maxEvents - count); replay_ = s.panic.load(std::memory_order_acquire) != 0; replayUnconfirmed_.store(true, std::memory_order_relaxed); }
                if(auto* pool = s.inputBytes()) pool->used = 0;
                for(uint32_t i = 0; i < count; ++i)
                {
                    // The beat's and the harmony's go to a plugin that asked for them; its host would drop them on arrival.
                    if(const auto space = events[i].header()->space_id; (space == kick::clap::Host::beatSpace && !(wants_ & 1)) || (space == kick::clap::Host::harmonySpace && !(wants_ & 2))) continue;
                    s.events[kept] = events[i];
                    if(share(s.events[kept], s.inputBytes())) ++kept;
                    else sysexDrops_.fetch_add(1, std::memory_order_relaxed);
                }
                carriedCount_ = 0; if(carriedBytes_) carriedBytes_->used = 0;
                s.frames = frames; s.eventCount = kept; s.hasTransport = transport != nullptr;
                if(restedBlocks_ || restedSent_) { s.restedBlocks = restedBlocks_; s.restedFrames = restedFrames_; restedSent_ = restedBlocks_ != 0; restedBlocks_ = restedFrames_ = 0; } // written only while there is something to tell
                called_.store(called_.load(std::memory_order_relaxed) + 1, std::memory_order_relaxed); // one writer: no locked add
                events_.store(events_.load(std::memory_order_relaxed) + kept, std::memory_order_relaxed); // one writer
                for(auto most = mostEvents_.load(std::memory_order_relaxed); kept > most && !mostEvents_.compare_exchange_weak(most, kept, std::memory_order_relaxed);) {} // the reader's reset may race: lock-free, never waits
                s.setLimits = limitsWaiting_ && shareLimits(s);
                if(s.setLimits) limitsWaiting_ = false;
                s.ringOut = s.ringOutChannels && no >= outChannels_;
                if(transport) s.transport = *transport;
                // The worker spins for the next block only while every worker's audio thread and this one have a core each.
                s.spinNs = process()->mode == ProcessingMode::Realtime && Process::audioThreads.load(std::memory_order_relaxed) + 1 <= Process::cores() ? uint32_t(process()->spinNs.load(std::memory_order_relaxed)) : 0;
                sent_ = std::chrono::steady_clock::now();
                requestedAt_.store(sent_.time_since_epoch().count(), std::memory_order_release);
                requested_.store(++sequence_, std::memory_order_release);
                s.request.store(sequence_, std::memory_order_release);
                // One doorbell for all the worker's nodes (RFC 0045): a system call only while its audio thread sleeps.
                auto& bell = *process()->bell;
                bell.ring.fetch_add(1, std::memory_order_seq_cst);
                if(bell.sleeping.load(std::memory_order_seq_cst)) wake(bell.ring);
                startedOutputs_ = outputs; startedChannels_ = no; startedFrames_ = frames; startedOffset_ = offset;
                return true;
            }
            // collect: take the block if it is done, without waiting (the extra block's last block, which had a period).
            bool finish(bool collect = false) noexcept
            {
                struct Idle { std::atomic<bool>& busy; ~Idle() { busy.store(false); } } idle{busy_};
                auto& s = *shared_;
                const auto outputs = startedOutputs_; const auto no = startedChannels_, frames = startedFrames_, offset = startedOffset_; const auto sent = sent_;
                // The block handed lost nothing: the worker renders it, and the plugin keeps its notes (RFC 0045).
                auto silence = [&] {
                    fadeOut(outputs, no, frames);
                    abandoned_ = true;
                    return false;
                };
                // The budget counts from the request, however long the caller did other work since; with the
                // extra block, from the request a call earlier, so a period more.
                auto budget = extraBlock() ? process()->budget() + std::chrono::nanoseconds(int64_t(1e9 * process()->block.load(std::memory_order_relaxed) / process()->rate)) : process()->budget();
                // RFC 0045: never past the wave's deadline (realtime.h's workerDeadline), the callback's own at the latest; with
                // step 6, for the extra block's block in flight, and in a plugin host (whose nodes render one after another,
                // so a node's own allowance from its hand-over does not hold), that deadline in place of the allowance.
                if(const auto deadline = workerDeadline; deadline.time_since_epoch().count() && process()->mode == ProcessingMode::Realtime)
                {
                    const auto toDeadline = std::chrono::duration_cast<std::chrono::nanoseconds>(deadline - sent);
                    budget = collect ? std::chrono::nanoseconds::zero() : workerDeadlineOnly || extraBlock() || process()->pooled ? toDeadline : std::min(budget, toDeadline);
                    budget = std::max(budget, std::chrono::nanoseconds::zero());
                }
                const bool ready = s.done.load(std::memory_order_acquire) == sequence_;
                const auto checked = std::chrono::steady_clock::now();
                // RFC 0045's spin: polled for up to the spin window (within the allowance) before sleeping, only while every
                // worker's audio thread and this one have a core each: otherwise the core this thread spins on is one a
                // worker it waits for may need (ST Final's nine workers on the FANTOM-07's four cores: none).
                bool spun = false;
                if(!ready && process()->mode == ProcessingMode::Realtime && !extraBlock() // a delayed block is waited for late in the callback: no spin
                   && Process::audioThreads.load(std::memory_order_relaxed) + 1 <= Process::cores())
                    for(const auto until = checked + std::min(std::chrono::nanoseconds(process()->spinNs.load(std::memory_order_relaxed)), std::chrono::nanoseconds(budget) - (checked - sent));
                        !(spun = s.done.load(std::memory_order_acquire) == sequence_) && std::chrono::steady_clock::now() < until;) relax();
                const auto left = std::max(std::chrono::nanoseconds(0), std::chrono::nanoseconds(budget) - (std::chrono::steady_clock::now() - sent));
                // The round trip as this thread sees it: done when first found done, or given up.
                // Asleep on done, saying so: the worker makes the wake-up's system call only then (Shared::waiting).
                auto sleep = [&] {
                    s.waiting.store(1, std::memory_order_relaxed);
                    std::atomic_thread_fence(std::memory_order_seq_cst);
                    const bool woke = waitWhileEqual(s.done, sequence_ - 1, left);
                    s.waiting.store(0, std::memory_order_relaxed);
                    return woke;
                };
                const bool done = ready || spun || (sleep() && s.done.load(std::memory_order_acquire) == sequence_);
                // Late by the clock counts toward "too slow" even when the wait itself woke late; a
                // block done before the caller came for it was not late.
                const auto seen = ready ? checked : std::chrono::steady_clock::now();
                const auto at = std::chrono::nanoseconds(seen.time_since_epoch()).count();
                timing_.roundTrip.record(std::chrono::nanoseconds(seen - sent).count(), at);
                timing_.collect.record(std::chrono::nanoseconds(checked - sent).count(), at);
                if(done) workerTimes(s, seen);
                switch(blockOutcome(ready, done, seen - sent, budget))
                {
                case BlockOutcome::silent: missed(silent_); return silence();
                case BlockOutcome::late: missed(late_); break;
                case BlockOutcome::onTime: consecutiveMisses_ = 0; break;
                }
                for(uint32_t c = 0; c < no; ++c)
                {
                    if(!outputs[c]) continue;
                    if(c < outChannels_) std::copy_n(audioOut(c), frames, outputs[c]);
                    else std::fill_n(outputs[c], frames, 0.f);
                    for(uint32_t f = 0; f < frames && faded_ + f < fadeFrames_; ++f) outputs[c][f] *= float(faded_ + f) / float(fadeFrames_);
                    if(c < last_.size()) last_[c] = outputs[c][frames - 1];
                }
                faded_ = std::min(fadeFrames_, faded_ + frames);
                fadedOut_ = 0;
                takeOutputs(s, offset, false);
                // The worker's voice report, copied and bounded: the worker may write anything.
                if(voiceBudget_)
                {
                    activeVoices_ = std::min<uint32_t>(s.activeVoices, maxVoices);
                    const double cost = s.voiceCost;
                    voiceCost_ = std::isfinite(cost) && cost > 0 ? std::min(cost, 1e6) : 0;
                    voiceNoteCount_ = std::min<uint32_t>(s.voiceNoteCount, maxVoiceNotes);
                    for(uint32_t i = 0; i < voiceNoteCount_; ++i)
                    {
                        auto& n = voiceNotes_[i] = s.voiceNotes[i];
                        if(!(std::isfinite(n.loudness) && n.loudness > 0)) n.loudness = 0;
                    }
                    // Each list under the engine's ID for its context (unknown: OSCAIRN_VOICE_BUDGET_SELECTED); none while
                    // the IDs are being republished.
                    const auto version = idVersion_.load(std::memory_order_acquire);
                    const auto known = std::min<uint32_t>(idCount_.load(std::memory_order_relaxed), maxIds);
                    importanceCount_ = version & 1 ? 0 : std::min<uint32_t>(s.importanceCount, OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS);
                    for(uint32_t i = 0; i < importanceCount_; ++i)
                    {
                        auto& c = importance_[i] = s.importance[i];
                        if(contexts_)
                        {
                            uint32_t j = 0;
                            while(j < known && uint32_t(idTable_[j].load(std::memory_order_relaxed)) != c.context) ++j;
                            c.context = j < known ? uint32_t(idTable_[j].load(std::memory_order_relaxed) >> 32) : OSCAIRN_VOICE_BUDGET_SELECTED;
                        }
                        c.voices = uint16_t(std::min<uint32_t>(c.voices, maxVoices));
                        c.count = uint16_t(std::min<uint32_t>(c.count, OSCAIRN_VOICE_BUDGET_LEAST));
                        // A weight is a voice's cost against its others: positive and finite, else 1 (a context's sum, else its voices).
                        auto weight = [](float w, float none) { return std::isfinite(w) && w > 0 ? std::min(w, 1e6f) : none; };
                        c.weight = weight(c.weight, float(c.voices));
                        for(auto& v : c.least)
                        {
                            v.tier = std::min<uint8_t>(v.tier, 2);
                            v.level_db = std::isfinite(v.level_db) ? std::clamp(v.level_db, -120.f, 24.f) : -120.f;
                            v.weight = weight(v.weight, 1);
                        }
                    }
                    std::atomic_thread_fence(std::memory_order_acquire);
                    if(idVersion_.load(std::memory_order_relaxed) != version) importanceCount_ = 0;
                }
                reset_ = reset_ || s.reset != 0;
                if(!s.ok) kept_ = false; // not rendered in the worker (its control gate busy): events lost there
                return s.ok;
            }
            // The worker's output events into this call's. SysEx bytes are copied out of the shared block: the next
            // chunk reuses it, and the worker must not change them under the plugins that read them. Late: a block
            // given up on, its events at the start of this one (`offset`), else moved by it.
            void takeOutputs(Shared& s, uint32_t offset, bool late) noexcept
            {
                for(uint32_t i = 0; i < std::min(s.outputCount, maxOutputEvents) && outputCount_ < maxOutputEvents; ++i)
                {
                    auto& e = output_[outputCount_] = s.outputs[i];
                    if(e.header()->space_id == CLAP_CORE_EVENT_SPACE_ID && e.header()->type == CLAP_EVENT_MIDI_SYSEX &&
                       (!unshare(e, s.outputBytes()) || !sysexOutput_ || !sysexOutput_->hold(e))) { sysexDrops_.fetch_add(1, std::memory_order_relaxed); continue; }
                    auto& time = reinterpret_cast<clap_event_header_t*>(e.bytes.data())->time;
                    time = late ? offset : time + offset;
                    ++outputCount_;
                }
            }
            // A block not heard (RFC 0045): each channel's last sample heard fades to zero over about 1 ms, then
            // silence, and the next block heard fades in over as long (a restart's 10 ms fade-in left as it is).
            void fadeOut(float* const* outputs, uint32_t no, uint32_t frames) noexcept
            {
                const uint32_t n = std::max(1u, uint32_t(process()->rate * .001));
                if(outputs) for(uint32_t c = 0; c < no; ++c) if(outputs[c])
                {
                    const float from = c < last_.size() ? last_[c] : 0.f;
                    for(uint32_t f = 0; f < frames; ++f) outputs[c][f] = fadedOut_ + f < n ? from * float(n - 1 - fadedOut_ - f) / float(n) : 0.f;
                }
                fadedOut_ = std::min(n, fadedOut_ + frames);
                if(faded_ >= fadeFrames_) fadeFrames_ = n;
                faded_ = 0;
            }
            // A release: losing one leaves a note stuck.
            static bool release(const kick::clap::Event& e) noexcept
            {
                const auto* h = e.header();
                if(h->space_id != CLAP_CORE_EVENT_SPACE_ID) return false;
                if(h->type == CLAP_EVENT_NOTE_OFF || h->type == CLAP_EVENT_NOTE_CHOKE) return true;
                if(h->type != CLAP_EVENT_MIDI) return false;
                const auto* m = reinterpret_cast<const clap_event_midi_t*>(h)->data;
                const int kind = m[0] & 0xf0;
                return kind == 0x80 || (kind == 0x90 && !m[2]) || (kind == 0xb0 && ((m[1] == 64 && m[2] < 64) || m[1] == 120 || m[1] == 123));
            }
            // Adds an event to the carry, in order; at the block's start when it waits for a later block (its SysEx
            // bytes copied). Bounded as one block's events: when full, the oldest event that is not a release goes
            // (RFC 0045). False: a release was lost.
            bool carry(const kick::clap::Event& event, bool later) noexcept
            {
                auto e = event;
                if(later)
                {
                    reinterpret_cast<clap_event_header_t*>(e.bytes.data())->time = 0;
                    if(e.header()->space_id == CLAP_CORE_EVENT_SPACE_ID && e.header()->type == CLAP_EVENT_MIDI_SYSEX && (!carriedBytes_ || !carriedBytes_->hold(e)))
                    { carryDrops_.fetch_add(1, std::memory_order_relaxed); return true; }
                }
                if(carriedCount_ == maxEvents)
                {
                    carryDrops_.fetch_add(1, std::memory_order_relaxed);
                    uint32_t i = 0;
                    while(i < carriedCount_ && release(carried_[i])) ++i;
                    if(i == carriedCount_) return !release(e); // all releases: a release in drops itself, anything else goes
                    std::move(carried_.begin() + i + 1, carried_.begin() + carriedCount_, carried_.begin() + i);
                    --carriedCount_;
                }
                carried_[carriedCount_++] = e;
                return true;
            }

            // RFC 0045's extra block: each call plays the block handed a call earlier, so the node sounds exactly one
            // block late. The block in flight is taken at the call's start when it is done, and the call's own block
            // handed at once; otherwise the call's block is kept (staged), and finishDelayed, which the graph calls
            // after the wave's other workers, waits for the one in flight to the deadline the graph sets for it (the
            // callback's, less what the waves after this one took in the last block, so the plugins after it keep
            // their time), then hands the kept block. A block that misses even that is faded; the block kept behind
            // it goes to the worker as soon as the worker is free and is heard in that call when what the worker's
            // blocks usually take fits before the deadline, else its sound is given up too, its events going with the
            // call's own block, handed at once so the part is in step again. Without a device it is waited for, as a
            // block past the allowance always was. Its outputs, events, voice report and reset come with the block
            // played. Audio thread.
            bool extraBlock() const noexcept { return extra_; }
            static bool listed(const char* list, const std::string& id)
            {
                for(std::string_view rest = list ? list : ""; !rest.empty();)
                {
                    const auto comma = rest.find(',');
                    if(rest.substr(0, comma) == id) return true;
                    rest = comma == rest.npos ? std::string_view() : rest.substr(comma + 1);
                }
                return false;
            }
            // The held buffer the block in flight is not in.
            float* const* spare() const noexcept { return heldPointers_.data() + (startedOutputs_ == heldPointers_.data() ? outChannels_ : 0); }
            // Hands the idle worker a block into the spare buffer, a panic waiting for it first.
            void handDelayed(const float* const* inputs, uint32_t ni, uint32_t frames, const kick::clap::Event* events, uint32_t count,
                             const clap_event_transport_t* transport, uint32_t offset) noexcept
            {
                if(panicLater_.exchange(false, std::memory_order_acq_rel) && !process()->failed.load()) shared_->panic.store(1, std::memory_order_release);
                // shortcut: a block longer than the worker's is silenced here; a realtime block never is, only an offline one (not delayed).
                const bool fits = frames <= process()->frames;
                auto* next = spare();
                kept_ = true;
                previous_ = fits && start(inputs, ni, next, std::min(playChannels_, outChannels_), frames, events, count, transport, offset) ? Previous::started : Previous::notStarted;
                if(previous_ == Previous::notStarted)
                {
                    notStartedKept_ = fits && kept_;
                    startedOutputs_ = next; startedFrames_ = std::min(frames, process()->frames);
                }
            }
            // The call's block kept to hand over later: its inputs, events (SysEx bytes copied) and transport.
            void stage(const float* const* inputs, uint32_t ni, uint32_t frames, const kick::clap::Event* events, uint32_t count,
                       const clap_event_transport_t* transport, uint32_t offset) noexcept
            {
                stagedFrames_ = frames; stagedOffset_ = offset;
                const auto n = std::min(frames, process()->frames);
                for(uint32_t c = 0; c < inChannels_; ++c)
                {
                    if(inputs && c < ni && inputs[c]) std::copy_n(inputs[c], n, stagedInPointers_[c]);
                    else std::fill_n(stagedInPointers_[c], n, 0.f);
                }
                if(stagedBytes_) stagedBytes_->used = 0;
                stagedCount_ = 0;
                for(uint32_t i = 0; i < std::min(count, maxEvents) && events; ++i)
                {
                    auto e = events[i];
                    if(e.header()->space_id == CLAP_CORE_EVENT_SPACE_ID && e.header()->type == CLAP_EVENT_MIDI_SYSEX && (!stagedBytes_ || !stagedBytes_->hold(e))) { sysexDrops_.fetch_add(1, std::memory_order_relaxed); continue; }
                    staged_[stagedCount_++] = e;
                }
                stagedTransport_ = transport ? *transport : clap_event_transport_t{}; stagedHasTransport_ = transport != nullptr;
            }
            void handStaged() noexcept { handDelayed(stagedInPointers_.data(), inChannels_, stagedFrames_, staged_.data(), stagedCount_, stagedHasTransport_ ? &stagedTransport_ : nullptr, stagedOffset_); }
            void startDelayed(const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames,
                              const kick::clap::Event* events, uint32_t count, const clap_event_transport_t* transport, uint32_t offset) noexcept
            {
                playOutputs_ = outputs; playChannels_ = no; playFrames_ = frames;
                deferred_ = false; played_ = nullptr;
                // Nothing the worker had before it failed or restarted is heard.
                const bool stale = process()->failed.load() || process()->generation.load() != generation_;
                if(previous_ == Previous::waiting)
                {
                    if(stale) { previous_ = Previous::notStarted; notStartedKept_ = false; }
                    else if(shared_->done.load(std::memory_order_acquire) == sequence_)
                    {
                        // Free again: the block kept behind a missed one goes now and is heard in this call, when what a
                        // block usually takes fits before the deadline.
                        if(!workerDeadline.time_since_epoch().count() || std::chrono::steady_clock::now() + std::chrono::nanoseconds(runNs_) < workerDeadline)
                        {
                            handStaged();
                            if(previous_ == Previous::started) { stage(inputs, ni, frames, events, count, transport, offset); deferred_ = true; return; }
                        }
                        else
                        {
                            // It would miss too, and every block after it: its sound is given up, its events go with this
                            // call's block, handed now with a whole period more, and the part is in step again.
                            silent_.fetch_add(1, std::memory_order_relaxed);
                            bool kept = true;
                            for(uint32_t i = 0; i < stagedCount_; ++i) kept = carry(staged_[i], true) && kept;
                            playedOk_ = false; playedKept_ = kept;
                            handDelayed(inputs, ni, frames, events, count, transport, offset);
                            return;
                        }
                    }
                    else
                    {
                        // Still busy: the kept block is not heard; its events go with the next block handed.
                        silent_.fetch_add(1, std::memory_order_relaxed); busyBlocks_.fetch_add(1, std::memory_order_relaxed);
                        bool kept = true;
                        for(uint32_t i = 0; i < stagedCount_; ++i) kept = carry(staged_[i], true) && kept;
                        playedOk_ = false; playedKept_ = kept;
                        stage(inputs, ni, frames, events, count, transport, offset);
                        return;
                    }
                }
                bool kept = true;
                if(previous_ == Previous::started)
                {
                    busy_.store(true);
                    if(stale) { busy_.store(false); playedOk_ = kept = false; }
                    else if(shared_->done.load(std::memory_order_acquire) == sequence_) { kept_ = true; playedOk_ = finish(true); kept = kept_; played_ = startedOutputs_; playedFrames_ = startedFrames_; }
                    else { stage(inputs, ni, frames, events, count, transport, offset); deferred_ = true; return; } // waited for in finishDelayed
                }
                else if(previous_ == Previous::notStarted) { playedOk_ = false; kept = notStartedKept_; }
                else playedOk_ = true; // a first block is silent
                playedKept_ = kept;
                handDelayed(inputs, ni, frames, events, count, transport, offset);
            }
            bool finishDelayed() noexcept
            {
                if(deferred_)
                {
                    busy_.store(true);
                    kept_ = true;
                    // Waited for to the deadline the graph gives it: the callback's less what the plugins after it took.
                    playedOk_ = finish(false); // its sound, or (missed) the fade
                    playedKept_ = kept_; played_ = startedOutputs_; playedFrames_ = startedFrames_;
                    if(shared_->done.load(std::memory_order_acquire) == sequence_) handStaged();
                    else previous_ = Previous::waiting;
                }
                if(!played_ && !playedOk_) fadeOut(playOutputs_, playChannels_, playFrames_);
                else for(uint32_t c = 0; c < playChannels_; ++c)
                {
                    if(!playOutputs_ || !playOutputs_[c]) continue;
                    // shortcut: a block of another length than the last plays what the last had, then silence; a device's blocks keep one length.
                    const auto n = played_ && c < outChannels_ ? std::min(playedFrames_, playFrames_) : 0;
                    if(n) std::copy_n(played_[c], n, playOutputs_[c]);
                    std::fill(playOutputs_[c] + n, playOutputs_[c] + playFrames_, 0.f);
                }
                return playedOk_;
            }

            // The waiting limits into the block, each context under the worker's ID for it; false
            // while the IDs are being republished, to try again at the next block. A context the
            // worker does not know is left out. Audio thread.
            bool shareLimits(Shared& s) noexcept
            {
                const auto version = idVersion_.load(std::memory_order_acquire);
                if(version & 1) return false;
                const auto known = std::min<uint32_t>(idCount_.load(std::memory_order_relaxed), maxIds);
                uint32_t n = 0;
                for(uint32_t i = 0; i < voiceContextCount_; ++i)
                {
                    auto c = voiceContexts_[i];
                    if(contexts_ && c.context != OSCAIRN_VOICE_BUDGET_SELECTED)
                    {
                        uint32_t j = 0;
                        while(j < known && uint32_t(idTable_[j].load(std::memory_order_relaxed) >> 32) != c.context) ++j;
                        if(j == known) continue;
                        c.context = uint32_t(idTable_[j].load(std::memory_order_relaxed));
                    }
                    s.voiceContexts[n++] = c;
                }
                std::atomic_thread_fence(std::memory_order_acquire);
                if(idVersion_.load(std::memory_order_relaxed) != version) return false;
                s.voiceLimit = voiceLimit_; s.voiceContextCount = n;
                return true;
            }
            // ids_ for the audio thread, under a count that is odd while it changes. Under control.
            void publishIds() noexcept
            {
                idVersion_.fetch_add(1, std::memory_order_relaxed);
                std::atomic_thread_fence(std::memory_order_release);
                uint32_t n = 0;
                for(const auto& [engine, worker] : ids_) if(n < maxIds) idTable_[n++].store(uint64_t(engine) << 32 | worker, std::memory_order_relaxed);
                idCount_.store(n, std::memory_order_relaxed);
                idVersion_.fetch_add(1, std::memory_order_release);
            }

            // A block that ran out of budget, late or silent. Only these count toward "too slow"; a
            // block the worker never finishes is a hang, which the supervisor detects.
            void missed(std::atomic<uint64_t>& count) noexcept
            {
                count.fetch_add(1, std::memory_order_relaxed);
                if(!graceLeft_ && ++consecutiveMisses_ >= process()->missLimit) tooSlow_.store(true, std::memory_order_release);
            }

            // The worker this node is in: owner_ keeps it, proc_ is what the audio thread and the others read. Both
            // change only when the node moves to a worker of its own, under both workers' control, while it is
            // not busy (Process::restart); the worker it leaves is a pooled host, which the pool keeps.
            std::shared_ptr<Process> owner_;
            std::atomic<Process*> proc_;
            Process* process() const noexcept { return proc_.load(std::memory_order_acquire); }
            std::shared_ptr<const Engine> engine_;
            mutable std::atomic<uint64_t> resident_{0}; // the last reading of residentBytes
            mutable std::optional<std::optional<oscairn_memory_t>> declared_; // what it declared at its last activation, as far as known
            uint32_t grown_ = 0; // sound contexts prepared since then, each costing its declared context bytes (RFC 0025)
            bool root_;
            uint32_t node_ = 0; // the worker's index for this node; under control
            std::string description_;
            kick::clap::Descriptor descriptor_;
            std::vector<kick::clap::Parameter> parameters_;
            std::unordered_map<clap_id, uint32_t> index_;
            std::vector<kick::clap::Port> inputs_, outputs_;
            uint32_t noteInputs_ = 0, noteOutputs_ = 0, contexts_ = 0;
            bool sysexIn_ = true, sysexOut_ = true; // from the description: whether it can take, send SysEx
            uint8_t wants_ = 3; // from the description: the events of Oscairn's it asked for (PluginHost::wantsEvents)
            std::vector<uint8_t> clapNotes_;
            bool state_ = false, effects_ = false, window_ = false, snapshots_ = false, ringOut_ = false, savesSounds_ = false, voiceBudget_ = false, honorsVolume_ = false;
            std::string windowTitle_; // the window the engine last opened, for a restarted worker; under control
            std::optional<uint64_t> activeOutputs_; // the output ports the engine last said it takes sound from, for a restarted worker
            int shm_ = -1;
            Shared* shared_ = nullptr; size_t bytes_ = 0;
            uint32_t capacity_ = 0;
            // The block's channel counts, the engine's own: the worker may write anything into the
            // shared block, so the audio thread never reads these back from it (as its unshare and
            // voice-report bounds do). Set with the block, kept for the worker's whole life.
            uint32_t inChannels_ = 0, outChannels_ = 0;
            // The audio buffers, placed by the engine's capacity_, not the block's maxFrames.
            float* audioIn(uint32_t channel) const noexcept { return reinterpret_cast<float*>(shared_ + 1) + size_t(channel) * capacity_; }
            float* audioOut(uint32_t channel) const noexcept { return audioIn(inChannels_ + channel); }
            // Recovery record, under control: state or snapshot, the sounds prepared and discarded
            // since, selections and parameter values.
            std::filesystem::path record_;
            bool snapshotRecord_ = false, fresh_ = false;
            std::map<uint32_t, uint32_t> ids_; // engine sound ID -> worker sound ID
            std::vector<uint32_t> base_, discarded_; // the record's sounds, in slot order; those discarded since
            struct Beside { std::string directory; std::vector<uint32_t> ids, gone; }; // a snapshot prepared beside: its sounds, slot order; those discarded since
            std::vector<Beside> pendingBeside_; // since the record (recordAsleep)
            std::atomic<bool> recordDue_{false}; // pendingBeside_ holds some: service() reads it without control
            std::atomic<bool> saving_{false};    // recordAsleep saving: start() hands no block, carrying its events
            std::vector<Prepared> prepared_;
            std::array<uint32_t, 16> selected_ = [] { std::array<uint32_t, 16> a; a.fill(UINT32_MAX); return a; }();
            uint32_t nextId_ = 1;
            std::vector<double> values_;
            bool besideRecord_ = false; // the record is a snapshot prepared beside (RFC 0021); under control
            std::atomic<uint64_t> revision_{0}; // moves with each info rescan the worker forwards
            std::atomic<uint64_t> valueRevision_{0}; // valueRevision: moves with each request after which the worker's values may differ unannounced
            std::atomic<bool> unreported_{false}; // the worker's host cannot tell (an LV2 plugin's)
            // Audio thread, and what the supervisor reads of it.
            std::atomic<uint32_t> requested_{0};
            std::atomic<int64_t> requestedAt_{0};
            std::atomic<bool> tooSlow_{false}, busy_{false};
            // Blocks past the allowance: played late, or silenced (also those silenced at once while the worker was
            // still busy, which busyBlocks_ counts apart); panics sent right after a block not rendered (missed_).
            std::atomic<uint64_t> late_{0}, silent_{0}, busyBlocks_{0}, missPanics_{0};
            // Blocks handed to the worker, and those its plugin slept through uncalled (rest), for the timing report; and
            // those since the last handed, with their frames, which the next request tells the worker. Audio thread.
            std::atomic<uint64_t> called_{0}, rested_{0};
            // The events handed to the worker in those blocks (replayed and carried ones too): their total, and the most in one
            // block since the last read that reset it (inputEvents). Audio thread writes.
            std::atomic<uint64_t> events_{0}, mostEvents_{0};
            uint32_t restedBlocks_ = 0, restedFrames_ = 0; bool restedSent_ = false;
            // Audio thread: the blocks in a row it could have rested, called while fewer than restHold (holding_: this one
            // was).
            static constexpr uint32_t restHold = 192; // 0.5 s at 128 frames and 48 kHz
            uint32_t restful_ = 0; bool holding_ = false;
            std::atomic<bool> missed_{false};
            // RFC 0045's miss policy, audio thread: the events of blocks not handed (the worker still busy) for the
            // next block handed, and their SysEx bytes; the events the carry dropped, full; whether this call's
            // blocks not heard lost no events (kept_), with the extra block the block played's (playedKept_); a
            // block given up on (abandoned_), its output events to come with the next block handed; each output
            // channel's last sample heard, and how far a silent block has faded it out.
            std::array<kick::clap::Event, maxEvents> carried_{};
            uint32_t carriedCount_ = 0, fadedOut_ = 0;
            std::unique_ptr<kick::clap::SysexPool> carriedBytes_; // made only for a plugin that takes SysEx
            std::atomic<uint64_t> carryDrops_{0};
            bool kept_ = true, playedKept_ = true, abandoned_ = false;
            std::vector<float> last_;
            bool reset_ = false; // audio thread: wasReset, from the worker's blocks in the last call
            int64_t lastWaitNs_ = 0, lastCpuNs_ = 0; // the last block's wait to be woken and CPU time, as workerTimes recorded them
            uint32_t sequence_ = 0, timed_ = 0, outputCount_ = 0, consecutiveMisses_ = 0, generation_ = 0, faded_ = 0, fadeFrames_ = 0;
            uint32_t startsSeen_ = UINT32_MAX, graceLeft_ = 0; bool noGrace_ = false; // timing_'s starts last seen, and the blocks left of the start's grace (start)
            // The block start handed over, for finish.
            float* const* startedOutputs_ = nullptr; uint32_t startedChannels_ = 0, startedFrames_ = 0, startedOffset_ = 0; bool started_ = false;
            std::chrono::steady_clock::time_point sent_{};
            // The extra block (RFC 0045): two buffers of the worker's block, the one the last block went to
            // and the one this block goes to; what became of the last block, and what this call plays.
            bool extra_ = false; std::atomic<bool> panicLater_{false};
            std::vector<float> held_; std::vector<float*> heldPointers_;
            // waiting: the block handed was given up on and the worker is still in it; the call's block is kept (staged).
            enum class Previous { none, started, notStarted, waiting } previous_ = Previous::none;
            bool playedOk_ = true, notStartedKept_ = true, deferred_ = false;
            int64_t runNs_ = 0; // what a block usually takes in the worker (workerTimes)
            std::vector<float> stagedIn_; std::vector<float*> stagedInPointers_;
            std::array<kick::clap::Event, maxEvents> staged_{};
            // RFC 0045's plugin groups, audio thread, for the start under way: this node's routes from others of its worker.
            static constexpr uint32_t maxFused = OSCAIRN_WORKER_MAX_FUSED;
            static_assert(maxFused == maxGroupRoutes);
            const GroupRoute* group_ = nullptr; uint32_t groupCount_ = 0; bool groupLost_ = false;
            uint32_t stagedCount_ = 0, stagedFrames_ = 0, stagedOffset_ = 0;
            std::unique_ptr<kick::clap::SysexPool> stagedBytes_; // made only for a plugin that takes SysEx
            clap_event_transport_t stagedTransport_{}; bool stagedHasTransport_ = false;
            float* const* played_ = nullptr; uint32_t playedFrames_ = 0;
            float* const* playOutputs_ = nullptr; uint32_t playChannels_ = 0, playFrames_ = 0;
            WorkerTiming timing_;
            std::array<kick::clap::Event, maxEvents> chunk_{};
            std::vector<kick::clap::Event> output_, pending_;
            std::unique_ptr<kick::clap::SysexPool> sysexOutput_; // output_'s SysEx bytes; made only for a plugin that sends SysEx
            std::atomic<uint64_t> sysexDrops_{0}; // SysEx that did not fit a block's pool on the way in or out
            // oscairn.voice-budget, audio thread: the limits waiting for the next block (the engine's
            // sound IDs), and the worker's last report, bounded. Voices past maxVoices are not credible.
            static constexpr uint32_t maxVoices = 1u << 16;
            bool limitsWaiting_ = false;
            // What a plugin keeps only in memory, lost with its worker, as it crossed on the way in (audio thread;
            // selections_ is the control thread's count): each channel's last controllers, pitch bend, channel
            // pressure and channel-wide note expressions, and each parameter's last value and modulation sent to a
            // channel (or to all, slot 0) since that channel selected a sound or a state was loaded. Replayed at the
            // start of the first block after a restart, behind the restored sounds and selections the plugin applies
            // first, so each lands on the sound its channel selects. Notes are not replayed: a sounding note is lost.
            static constexpr std::array<uint8_t, 7> keptControllers_{1, 2, 4, 11, 64, 66, 67}; // wheel, breath, foot, expression, the pedals
            struct Channel { std::array<int16_t, 7> cc{-1, -1, -1, -1, -1, -1, -1}; int32_t bend = -1; int16_t pressure = -1; std::array<double, 7> expression{}; uint8_t expressed = 0; };
            std::array<Channel, 16> channels_{};
            std::vector<double> sentValues_, sentModulation_; std::vector<uint8_t> sentKinds_; // parameter index × 17 slots: 1 a value, 2 a modulation
            std::vector<clap_id> sentIds_; // the parameters' IDs by index: the audio thread's own copy
            std::array<std::atomic<uint32_t>, 17> selections_{}; std::array<uint32_t, 17> selectionsSeen_{}; // slot 0: all channels
            bool replay_ = false;
            std::atomic<bool> replayUnconfirmed_{false}, replayAgain_{false}; // the replay's block not yet seen done; a panic met it
            void forgetSent(uint32_t slot) noexcept { for(size_t i = slot; i < sentKinds_.size(); i += 17) sentKinds_[i] = 0; }
            void track(const kick::clap::Event* events, uint32_t count) noexcept
            {
                for(uint32_t slot = 0; slot < 17; ++slot)
                    if(const auto n = selections_[slot].load(std::memory_order_acquire); n != selectionsSeen_[slot])
                    {
                        selectionsSeen_[slot] = n;
                        if(slot) forgetSent(slot); else for(uint32_t all = 0; all < 17; ++all) forgetSent(all);
                    }
                for(uint32_t i = 0; i < count; ++i) track(events[i]);
            }
            void track(const kick::clap::Event& event) noexcept
            {
                const auto* h = event.header();
                if(h->space_id != CLAP_CORE_EVENT_SPACE_ID) return;
                if(h->type == CLAP_EVENT_MIDI)
                {
                    const auto& m = *reinterpret_cast<const clap_event_midi_t*>(h);
                    if(m.port_index) return;
                    auto& c = channels_[m.data[0] & 15];
                    switch(m.data[0] & 0xf0)
                    {
                        case 0xb0: for(size_t i = 0; i < keptControllers_.size(); ++i) if(m.data[1] == keptControllers_[i]) c.cc[i] = int16_t(m.data[2] & 127); break;
                        case 0xd0: c.pressure = int16_t(m.data[1] & 127); break;
                        case 0xe0: c.bend = int32_t((m.data[2] & 127) << 7 | (m.data[1] & 127)); break;
                    }
                }
                else if(h->type == CLAP_EVENT_NOTE_EXPRESSION)
                {
                    const auto& x = *reinterpret_cast<const clap_event_note_expression_t*>(h);
                    if(x.note_id != -1 || x.key != -1 || x.channel < 0 || x.channel > 15 || x.expression_id < 0 || x.expression_id > 6) return;
                    auto& c = channels_[size_t(x.channel)];
                    c.expression[size_t(x.expression_id)] = x.value; c.expressed |= uint8_t(1u << x.expression_id);
                }
                else if(h->type == CLAP_EVENT_PARAM_VALUE || h->type == CLAP_EVENT_PARAM_MOD)
                {
                    const bool value = h->type == CLAP_EVENT_PARAM_VALUE;
                    const auto& v = *reinterpret_cast<const clap_event_param_value_t*>(h);
                    const auto& m = *reinterpret_cast<const clap_event_param_mod_t*>(h);
                    const auto id = value ? v.param_id : m.param_id;
                    const int note = value ? v.note_id : m.note_id, key = value ? v.key : m.key, channel = value ? v.channel : m.channel;
                    if(note != -1 || key != -1 || channel < -1 || channel > 15) return;
                    const auto i = index_.find(id);
                    if(i == index_.end()) return;
                    const auto at = size_t(i->second) * 17 + size_t(channel + 1);
                    if(at >= sentKinds_.size()) return;
                    if(value) { sentValues_[at] = v.value; sentKinds_[at] |= 1; }
                    else { sentModulation_[at] = m.amount; sentKinds_[at] |= 2; }
                }
            }
            // The replay after a restart, into `out` from `at`; returns the new count, at most `capacity`.
            // shortcut: what does not fit the block's events after its own is dropped; carry it to the next block if a plugin ever keeps that much.
            uint32_t replay(kick::clap::Event* out, uint32_t at, uint32_t capacity) noexcept
            {
                auto put = [&](const kick::clap::Event& e) { if(at < capacity) out[at++] = e; };
                for(uint8_t ch = 0; ch < 16; ++ch)
                {
                    const auto& c = channels_[ch];
                    for(size_t i = 0; i < keptControllers_.size(); ++i) if(c.cc[i] >= 0) put(kick::clap::Event::midi(uint8_t(0xb0 | ch), keptControllers_[i], uint8_t(c.cc[i])));
                    if(c.bend >= 0) put(kick::clap::Event::midi(uint8_t(0xe0 | ch), uint8_t(c.bend & 127), uint8_t(c.bend >> 7)));
                    if(c.pressure >= 0) put(kick::clap::Event::midi(uint8_t(0xd0 | ch), uint8_t(c.pressure), 0));
                    for(int16_t x = 0; x < 7; ++x) if(c.expressed >> x & 1)
                    {
                        clap_event_note_expression_t e{};
                        e.header = {sizeof(e), 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_NOTE_EXPRESSION, 0};
                        e.expression_id = x; e.note_id = -1; e.port_index = -1; e.channel = int16_t(ch); e.key = -1; e.value = c.expression[size_t(x)];
                        put(kick::clap::Event::from(e));
                    }
                }
                for(size_t at2 = 0; at2 < sentKinds_.size(); ++at2)
                {
                    const auto id = sentIds_[at2 / 17]; const auto channel = int16_t(int(at2 % 17) - 1);
                    if(sentKinds_[at2] & 1)
                    {
                        clap_event_param_value_t v{};
                        v.header = {sizeof(v), 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, 0};
                        v.param_id = id; v.note_id = -1; v.port_index = -1; v.channel = channel; v.key = -1; v.value = sentValues_[at2];
                        put(kick::clap::Event::from(v));
                    }
                    if((sentKinds_[at2] & 2) && sentModulation_[at2] != 0)
                    {
                        clap_event_param_mod_t m{};
                        m.header = {sizeof(m), 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_MOD, 0};
                        m.param_id = id; m.note_id = -1; m.port_index = -1; m.channel = channel; m.key = -1; m.amount = sentModulation_[at2];
                        put(kick::clap::Event::from(m));
                    }
                }
                return at;
            }
            uint32_t voiceLimit_ = 0, voiceContextCount_ = 0, activeVoices_ = 0, voiceNoteCount_ = 0;
            std::array<oscairn_voice_budget_context_t, OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS> voiceContexts_{};
            double voiceCost_ = 0;
            std::array<oscairn_voice_budget_note_t, maxVoiceNotes> voiceNotes_{};
            uint32_t importanceCount_ = 0;
            std::array<oscairn_voice_budget_importance_t, OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS> importance_{};
            // Engine sound ID << 32 | the worker's, as ids_ last had them (publishIds).
            // shortcut: sounds past maxIds are left out of voice limits; raise it if an instance holds more.
            static constexpr uint32_t maxIds = 64;
            std::array<std::atomic<uint64_t>, maxIds> idTable_{};
            std::atomic<uint32_t> idCount_{0}, idVersion_{0};
            size_t next_ = 0;
            std::chrono::steady_clock::time_point serviced_{}; // main thread: the last service asked of the worker (workerServiceInterval)
            static constexpr uint32_t quietServices = 25, quietFactor = 5; // see service
            mutable std::atomic<uint32_t> quiet_{0}; // the services in a row it answered with nothing, to quietServices
        };

        // RFC 0025: when every node's plugin declares its memory (oscairn.memory), the worker's limit
        // is what they declare (the engine's shared bytes once, each context's own) plus half again
        // and 64 MiB for the worker itself, replacing the policy's flat limit (RFC 0010's 4 GiB),
        // also as its cgroup's memory.max on Linux. A worker whose plugin declares nothing keeps it.
        std::optional<uint64_t> Process::fitted(bool ask, const oscairn_memory_t* extra)
        {
            const auto list = [&] { std::lock_guard nodesLock(nodesMutex); return nodes; }(); // nodes change only under control
            if(list.empty() && !extra) return {};
            uint64_t declared = 0;
            std::map<const Engine*, uint64_t> shared;
            auto add = [&](oscairn_memory_t m, const Engine* engine, uint32_t grown = 0) {
                // Every node of one engine reports the same shared bytes (memory.h), so the largest
                // is the engine's: counted once even when its root node is gone and only its effect
                // contexts remain, still holding those bytes; each plugin loaded in a host (RFC 0045)
                // is an engine of its own. Sounds prepared since its declaration
                // count its context bytes each (at most 256 of credible bytes: no overflow).
                credible(m);
                m.context_bytes += m.context_bytes * grown;
                credible(m);
                declared = sum(declared, m.context_bytes + m.asset_bytes);
                shared[engine] = std::max(shared[engine], m.shared_bytes);
            };
            for(auto* n : list)
            {
                const auto* m = ask ? &n->declaredLocked() : n->knownDeclaration();
                if(!m || !*m) return {};
                add(**m, n->engine().get(), n->grown());
            }
            if(extra) add(*extra, making); // a node being made: an effect context of that engine, or a plugin being loaded (none)
            for(const auto& [engine, bytes] : shared) declared = sum(declared, bytes);
            return sum(sum(declared, declared / 2), uint64_t(64) << 20);
        }
        uint64_t Process::sum(uint64_t a, uint64_t b)
        {
            uint64_t s = 0;
            if(__builtin_add_overflow(a, b, &s)) throw std::runtime_error("Plugin's memory declaration is not credible");
            return s;
        }
        // A declaration no worker could hold is refused before the plugin allocates, never taken as
        // its limit: over four times the policy's limit (the worker's address space on Linux), over
        // 1 TiB without one, or a sum that overflows.
        void Process::credible(const oscairn_memory_t& m) const
        {
            const uint64_t cap = policyLimit ? std::min(policyLimit, uint64_t(1) << 40) * 4 : uint64_t(1) << 40;
            if(sum(sum(m.shared_bytes, m.context_bytes), m.asset_bytes) > cap) throw std::runtime_error("Plugin's memory declaration is not credible");
        }
        // After an operation. A live plugin that just moved to a smaller mode may still hold the pages
        // it let go (an allocator returns them late, a heap may keep them), so the limit is lowered
        // no further than what the worker holds now plus its 64 MiB margin, and never raised by that:
        // it still stops growth past either. A new process is held to its declarations at once.
        void Process::fitLimit(bool fresh)
        {
            const auto limit = fitted(true);
            if(!limit) return;
            auto to = *limit;
            if(const auto now = memoryLimit.load(); !fresh && to < now) to = std::min(now, std::max(to, usage(pid, startFile.load()).memory + (uint64_t(64) << 20)));
            memoryLimit.store(to); limitCgroup();
        }
        // A node is about to activate: its limit is raised, if its declaration needs it, before the
        // plugin allocates. A node the engine has (outside a restart) has its declaration replaced; a
        // node being made (a load, an effect context, any node during a restart) is counted as one
        // more. Never lowered here, while the plugin may still hold its old mode's memory: the
        // fitLimit after the operation does that. The worker waits for the answer (RFC 0025).
        void Process::reserve(Reader& r)
        {
            const auto index = r.u32();
            std::optional<oscairn_memory_t> m;
            if(r.u8()) { m.emplace(); r.bytes(&*m, sizeof *m); }
            if(m) credible(*m); // before it is kept: Failed, and the worker refuses the activation
            WorkerHost* known = nullptr;
            if(!restarting) { std::lock_guard nodesLock(nodesMutex); for(auto* n : nodes) if(n->workerNode() == index) known = n; }
            if(known) known->declare(m);
            if(!known && !m) return;
            if(const auto limit = known ? fitted(false) : fitted(false, &*m); limit && *limit > memoryLimit.load()) { memoryLimit.store(*limit); limitCgroup(); }
        }

        // Before sounds are prepared on a running engine: raised for them, if they need it, never lowered.
        void Process::raise()
        {
            if(const auto limit = fitted(true); limit && *limit > memoryLimit.load()) { memoryLimit.store(*limit); limitCgroup(); }
        }

        // Watches the worker: a crash, a block left unanswered past the hang limit, too many missed
        // blocks in a row, or memory over its limit stop it and silence its nodes until it is restarted.
        void Process::supervise()
        {
            std::unique_lock lock(superviseMutex);
            uint32_t tick = 0;
            while(!stopping)
            {
                wakeSupervisor.wait_for(lock, std::chrono::milliseconds(10));
                if(stopping || gaveUp) continue;
                if(failed.load()) { restart(); continue; }
                int status = 0;
                if(waitpid(pid, &status, WNOHANG) == pid)
                {
                    reaped = true;
                    // A plugin's crash, not a kill from outside (the engine's own, the system's out-of-memory killer).
                    if(WIFSIGNALED(status) && WTERMSIG(status) != SIGKILL) blame(bell->faulted);
                    fail(WIFSIGNALED(status) ? "Plugin worker crashed (signal " + std::to_string(WTERMSIG(status)) + ")"
                                             : "Plugin worker exited (status " + std::to_string(WEXITSTATUS(status)) + ")");
                    continue;
                }
                // Its memory and CPU time every 100 ms, not each 10 ms tick: reading them (/proc on Linux) was most of
                // the supervisor's cost, about 1 % of a core a worker at rest in the Linux container. A crash, a hang
                // and blocks too slow are still seen at the tick.
                if(const auto u = tick++ % 10 ? Usage{} : usage(pid, startFile.load()); u.memory)
                {
                    memory.store(u.memory); cpu.store(u.cpu);
                    if(memoryLimit && u.memory > memoryLimit)
                    {
                        fail("Plugin used too much memory: " + std::to_string(u.memory >> 20) + " MB, over its limit of " + std::to_string(memoryLimit >> 20) + " MB");
                        continue;
                    }
                }
                const auto now = std::chrono::steady_clock::now().time_since_epoch().count();
                std::string why;
                { std::lock_guard nodesLock(nodesMutex); for(auto* n : nodes) if(why = n->trouble(now); !why.empty()) break; }
                if(why.starts_with("Plugin worker crashed")) blame(bell->faulted);
                else if(why.starts_with("Plugin hung")) blame(bell->rendering);
                else if(why.starts_with("Plugin too slow")) blameCostliest(); // in a plugin host every plugin's blocks are late behind the costliest
                if(!why.empty()) fail(why);
            }
        }

        // A worker that exited, seen now rather than at its supervisor's next tick: failed as the tick would fail it, and the
        // supervisor woken to restart it. So is one whose connection broke under a request: it is dying, if not yet exited
        // (a killed process takes milliseconds to end on the unit), so it is killed and waited for, which takes as long.
        // Never waits on a worker otherwise: a supervisor in a tick or a restart, or a request under way, has it.
        bool Process::exitedNow()
        {
            std::unique_lock lock(superviseMutex, std::try_to_lock);
            if(!lock || stopping || gaveUp || failed.load()) return false;
            std::unique_lock requests(control, std::try_to_lock);
            int status = 0;
            if(!requests || reaped || pid <= 0) return false;
            auto seen = waitpid(pid, &status, WNOHANG);
            if(seen == 0 && broken.load() && kill(pid, SIGKILL) == 0) seen = waitpid(pid, &status, 0); // dying: ended now
            if(seen != pid) return false;
            reaped = true;
            if(WIFSIGNALED(status) && WTERMSIG(status) != SIGKILL) blame(bell->faulted);
            fail(WIFSIGNALED(status) ? "Plugin worker crashed (signal " + std::to_string(WTERMSIG(status)) + ")" : "Plugin worker exited (status " + std::to_string(WEXITSTATUS(status)) + ")");
            wakeSupervisor.notify_one();
            return true;
        }

        // Brings a failed worker back: a new process with the same plugin and its effect contexts,
        // each node restored and faded in. After maxRestarts within a minute the nodes stay silent.
        void Process::restart()
        {
            const auto now = std::chrono::steady_clock::now();
            std::erase_if(restarts, [&](auto t) { return now - t > std::chrono::minutes(1); });
            auto report = [&](const std::string& text) { std::lock_guard lock(failureMutex); failureText = reason + "; " + text; };
            // A plugin host failing again and again for no plugin known (memory over its limit, a crash on a thread of
            // a plugin's own) is not given up, which would silence every plugin in it: its plugins each go to a worker
            // of their own, which give up alone, and it restarts with the one left.
            const bool split = restarts.size() >= maxRestarts && pooled && [&] {
                std::lock_guard l(nodesMutex);
                return std::any_of(nodes.begin(), nodes.end(), [&](auto* n) { return n->engine() != nodes.front()->engine(); });
            }();
            if(restarts.size() >= maxRestarts && !split)
            {
                gaveUp = true;
                report("not restarted: " + std::to_string(maxRestarts) + " restarts within a minute");
                return;
            }
            if(split) restarts.clear();
            restartGrace.store(restarts.empty());
            restarts.push_back(now);
            std::lock_guard controlLock(control); // the nodes stay as they are while restarting
            auto list = [&] { std::lock_guard l(nodesMutex); return nodes; }();
            for(auto* n : list) while(n->busy()) std::this_thread::sleep_for(std::chrono::milliseconds(1)); // audio leaves its last block
            for(auto* n : list) n->capture();
            if(split)
            {
                { std::lock_guard lock(failureMutex); blamed = UINT32_MAX; }
                while(std::any_of(list.begin(), list.end(), [&](auto* n) { return n->engine() != list.front()->engine(); })) moveOut(list, list.front()->engine());
            }
            else if(pooled) separate(list);
            try
            {
                // A worker failed by a lost reply was killed but not yet reaped.
                if(pid > 0 && !reaped) { kill(pid, SIGKILL); int status = 0; waitpid(pid, &status, 0); reaped = true; }
                if(socket >= 0) { close(socket); socket = -1; }
                cpuBefore.store(cpuBefore.load() + cpu.exchange(0)); memory.store(0);
                restarting = true;
                struct Restarting { bool& flag; ~Restarting() { flag = false; } } guard{restarting};
                start();
                // Each plugin again, in the order its node came, then the effect contexts made from it. One
                // whose own node is gone is loaded only to make its contexts from, then let go.
                std::vector<std::shared_ptr<const Engine>> engines;
                for(auto* n : list) if(std::find(engines.begin(), engines.end(), n->engine()) == engines.end()) engines.push_back(n->engine());
                for(const auto& e : engines)
                {
                    Reader r = load(e->path, e->id, 0); // a restart is not refused for memory (RFC 0025)
                    const auto rootNode = std::find_if(list.begin(), list.end(), [&](auto* n) { return n->isRoot() && n->engine() == e; });
                    uint32_t from = 0;
                    if(rootNode != list.end()) { (*rootNode)->reconnect(r); from = (*rootNode)->workerNode(); }
                    else
                    {
                        from = r.u32();
                        if(r.data.substr(r.at) != e->description) throw std::runtime_error("the plugin changed");
                        send(socket, Writer().u8(0).data); expect(receive(socket));
                    }
                    for(auto* n : list) if(!n->isRoot() && n->engine() == e) { Reader c = exchange(Writer().u8(uint8_t(Op::CreateEffectContext)).u32(from).data); n->reconnect(c); }
                    if(rootNode == list.end()) exchange(Writer().u8(uint8_t(Op::DestroyNode)).u32(from).data);
                }
                restarting = false;
                for(auto* n : list) n->restore();
                // What each node declares now, its restored state's (RFC 0025), not the old worker's.
                for(auto* n : list) n->forget();
                fitLimit(true);
            }
            catch(const std::exception& e)
            {
                if(pid > 0 && !reaped) { kill(pid, SIGKILL); int status = 0; waitpid(pid, &status, 0); reaped = true; }
                report(std::string("restart failed: ") + e.what()); // tried again on the next check
                return;
            }
            for(auto* n : list) n->recovered();
            { std::lock_guard lock(failureMutex); failureText.clear(); }
            generation.fetch_add(1);
            restarted.fetch_add(1, std::memory_order_relaxed);
            failed.store(false);
        }

        // RFC 0045's plugin hosts: a fixed number of worker processes (the placement policy's `hosts`),
        // started as instances need them and kept for the engine's life, each holding any plugins. An
        // instance goes to the host with the least measured cost (its plugins' costs, as the Session last
        // measured them, a share of the block each); a plugin on the policy's own-host list, or one a shared
        // host's crash or hang was put down to, gets a worker of its own instead. Every worker is listed
        // (pluginHosts). Main thread, but for own(), which a restart calls.
        struct Pool
        {
            std::mutex mutex; // hosts, costs; held while a new host's control is taken
            std::vector<std::shared_ptr<Process>> hosts;
            std::map<std::string, double> costs;
            // all, crashed: a restart, under its control, records here, so this lock is taken last and alone.
            std::mutex listed;
            std::vector<std::weak_ptr<Process>> all;
            std::set<std::string> crashed;
            static constexpr double unmeasured = .1; // a plugin not measured yet, a tenth of the block

            void track(const std::shared_ptr<Process>& p)
            {
                std::lock_guard lock(listed);
                std::erase_if(all, [](const auto& w) { return w.expired(); });
                all.push_back(p);
            }
            bool own(const std::string& id, const PlacementPolicy& policy)
            {
                std::lock_guard lock(listed);
                // An entry ending in ".*" names a publisher: every plugin whose ID starts with what comes before it.
                return policy.ownHost.count(id) || crashed.count(id) || std::any_of(policy.ownHost.begin(), policy.ownHost.end(), [&](const std::string& o) {
                    return o.size() > 2 && o.ends_with(".*") && id.starts_with(std::string_view(o).substr(0, o.size() - 1)); });
            }
            void crashedIn(const std::string& id) { std::lock_guard lock(listed); crashed.insert(id); }
            double cost(Process& p) // under mutex
            {
                std::lock_guard nodes(p.nodesMutex);
                double total = 0;
                for(auto* n : p.nodes) { const auto c = costs.find(n->descriptor().id); total += c == costs.end() ? unmeasured : c->second; }
                return total;
            }
            // The host for one more instance: a new one while there are fewer than the policy's, else the
            // one that costs least (then holds fewest nodes); null when every one is restarting or given up.
            // RFC 0045's plugin groups: each group's process, while it holds the group's plugins.
            std::map<std::string, std::weak_ptr<Process>> groups;
            std::shared_ptr<Process> grouped(const PlacementGroup& group, const std::string& program, double rate, uint32_t frames, uint64_t limit) // under mutex
            {
                if(group.key.empty()) return nullptr;
                std::erase_if(groups, [](const auto& g) { return g.second.expired(); });
                const auto g = groups.find(group.key);
                auto p = g == groups.end() ? nullptr : g->second.lock();
                return p && !p->failed.load() && !p->gaveUp.load() && p->program == program && p->frames == frames && p->policyLimit == limit && p->currentRate.load() == rate ? p : nullptr;
            }
            // `chain`: the group's own process, made for its first plugin and gone with its last.
            std::shared_ptr<Process> chain(const PlacementGroup& group, const std::string& program, double rate, uint32_t frames, uint64_t limit)
            {
                std::unique_lock lock(mutex);
                if(auto p = grouped(group, program, rate, frames, limit)) return p;
                auto p = std::make_shared<Process>(program, rate, frames, ProcessingMode::Realtime, limit);
                p->pooled = true; p->slot = Process::slots.fetch_add(1);
                std::unique_lock control(p->control);
                groups[group.key] = p;
                lock.unlock();
                try { p->start(); }
                catch(...) { p->failed.store(true); control.unlock(); std::lock_guard again(mutex); groups.erase(group.key); throw; }
                control.unlock();
                p->supervisor = ServiceThread([q = p.get()] { q->supervise(); });
                track(p);
                return p;
            }
            std::shared_ptr<Process> pick(const std::string& program, double rate, uint32_t frames, uint64_t limit, uint32_t count, const PlacementGroup& group = {})
            {
                std::unique_lock lock(mutex);
                std::erase_if(hosts, [](const auto& p) { return p->gaveUp.load(); }); // its nodes keep it; a new host takes its place
                if(auto p = grouped(group, program, rate, frames, limit)) return p; // a group is never split over hosts
                // A group placed alone takes a new host while there are fewer than the policy's; the others pass over such a
                // host while one not alone is there.
                // Hosts stay when their plugins go: an empty one holds no group, alone or not.
                auto size = [](Process& p) { std::lock_guard nodes(p.nodesMutex); return p.nodes.size(); };
                for(const auto& p : hosts) if(!size(*p)) p->alone = false;
                std::shared_ptr<Process> best; double least = 0; size_t fewest = 0, matching = 0;
                auto usable = [&](const Process& p) { return p.program == program && p.frames == frames && p.policyLimit == limit && p.currentRate.load() == rate; };
                bool others = false;
                for(size_t k = 0, seen = 0; k < hosts.size() && seen < count; ++k) if(usable(*hosts[k])) { ++seen; others = others || (!hosts[k]->alone && !hosts[k]->failed.load()); }
                for(const auto& p : hosts)
                {
                    if(!usable(*p) || matching >= count) continue; // past the policy's count (a policy changed while hosts stay): unused
                    ++matching;
                    if(p->failed.load() || (!group.alone && others && p->alone)) continue;
                    const double c = cost(*p);
                    const size_t n = size(*p);
                    // Alone: an empty host first, then the cheapest; the others: the cheapest.
                    const bool better = !best || (group.alone && (n == 0) != (fewest == 0) ? n == 0 : c < least || (c == least && n < fewest));
                    if(better) { best = p; least = c; fewest = n; }
                }
                if(matching >= count)
                {
                    if(best && group.alone && !fewest && others) best->alone = true; // empty and others left for the rest
                    if(best && !group.key.empty()) groups[group.key] = best;
                    return best;
                }
                auto p = std::make_shared<Process>(program, rate, frames, ProcessingMode::Realtime, limit);
                p->pooled = true; p->alone = group.alone; p->slot = Process::slots.fetch_add(1);
                if(!group.key.empty()) groups[group.key] = p;
                // Listed before it starts, and started without the pool's lock (a process start, its first reply): a pick
                // meanwhile may choose it, and waits for the start on its control.
                std::unique_lock control(p->control);
                hosts.push_back(p);
                lock.unlock();
                try { p->start(); }
                catch(...)
                {
                    p->failed.store(true); // a pick holding it gives up on it (loadPluginInWorker)
                    control.unlock();
                    std::lock_guard again(mutex); std::erase(hosts, p);
                    throw;
                }
                control.unlock();
                p->supervisor = ServiceThread([q = p.get()] { q->supervise(); });
                track(p);
                return p;
            }
        };
        Pool& pool() { static Pool p; return p; }

        void Process::blameCostliest()
        {
            std::lock_guard nodesLock(nodesMutex);
            const WorkerHost* costliest = nullptr;
            for(const auto* n : nodes) if(!costliest || n->lastBlockMicroseconds() > costliest->lastBlockMicroseconds()) costliest = n;
            std::lock_guard lock(failureMutex); blamed = costliest ? costliest->workerNode() : UINT32_MAX;
        }

        // RFC 0045: when a shared host failed by a crash or a hang put down to one of its plugins, that
        // plugin's nodes go to a worker of their own, which starts failed and is restarted with them by its
        // own supervisor, and the plugin gets a worker of its own from then on; the host restarts with the
        // others. Alone in its host, it is restarted there as before. Under control, the nodes idle.
        void Process::separate(std::vector<WorkerHost*>& list)
        {
            uint32_t index;
            { std::lock_guard lock(failureMutex); index = blamed; blamed = UINT32_MAX; }
            const auto culprit = std::find_if(list.begin(), list.end(), [&](auto* n) { return n->workerNode() == index; });
            if(index == UINT32_MAX || culprit == list.end()) return;
            const auto engine = (*culprit)->engine();
            if(std::all_of(list.begin(), list.end(), [&](auto* n) { return n->engine() == engine; })) return;
            pool().crashedIn(engine->id);
            moveOut(list, engine);
        }
        void Process::moveOut(std::vector<WorkerHost*>& list, std::shared_ptr<const Engine> engine)
        {
            auto own = std::make_shared<Process>(program, rate, frames, mode, policyLimit);
            own->generation.store(generation.load() + 1); // its nodes start afresh, faded in
            { std::lock_guard lock(own->failureMutex); own->reason = own->failureText = reason; }
            own->failed.store(true);
            {
                std::lock_guard control(own->control);
                for(auto* n : list) if(n->engine() == engine) n->moveTo(own);
            }
            std::erase_if(list, [&](auto* n) { return n->engine() == engine; });
            own->supervisor = ServiceThread([q = own.get()] { q->supervise(); });
            pool().track(own);
        }
    }

    namespace { std::atomic<int64_t> serviceInterval{0}; }
    uint64_t workerRequests() noexcept { return requests.load(std::memory_order_relaxed); }
    uint64_t workerFailures() noexcept { return failures.load(std::memory_order_relaxed); }
    void setWorkerServiceInterval(std::chrono::milliseconds interval) noexcept { serviceInterval.store(interval.count(), std::memory_order_relaxed); }
    std::chrono::milliseconds workerServiceInterval() noexcept
    {
        static const long long fixed = [] { const char* v = std::getenv("OSCAIRN_WORKER_SERVICE_MS"); char* end = nullptr; const long long n = v && *v ? std::strtoll(v, &end, 10) : -1; return v && *v && !*end && n >= 0 && n <= 1000 ? n : -1; }();
        return std::chrono::milliseconds(fixed >= 0 ? fixed : serviceInterval.load(std::memory_order_relaxed));
    }

    std::shared_ptr<PluginHost> loadPluginInWorker(const std::string& program, const std::string& path, const std::string& id,
                                                   double sampleRate, uint32_t maxFrames, ProcessingMode mode, uint64_t memoryLimit, uint64_t memoryRoom)
    {
        // RFC 0045: in real time, with plugin hosts, into the pool's host that costs least.
        std::shared_ptr<Process> process;
        // With `chain` (RFC 0045's plugin groups), into its placement group's own process.
        if(const auto& policy = placementPolicy(); mode == ProcessingMode::Realtime && (policy.hosts || policy.chain) && !pool().own(id, policy))
            process = policy.hosts ? pool().pick(program, sampleRate, maxFrames, memoryLimit, policy.hosts, placementGroup())
                    : placementGroup().key.empty() ? nullptr : pool().chain(placementGroup(), program, sampleRate, maxFrames, memoryLimit);
        if(process && [&] { std::lock_guard lock(process->control); return process->failed.load(); }())
            process = nullptr; // a host that failed since it was picked: a worker of its own instead
        const bool pooled = process != nullptr;
        if(!pooled) process = std::make_shared<Process>(program, sampleRate, maxFrames, mode, memoryLimit); // a worker of its own
        std::shared_ptr<PluginHost> host;
        try
        {
            std::lock_guard lock(process->control);
            if(process->failed.load()) throw std::runtime_error(process->failure()); // failed in the moment since
            if(!pooled) process->start();
            Reader r = process->load(path, id, memoryRoom);
            auto engine = std::make_shared<const Engine>(Engine{path, id, r.data.substr(r.at + 4)});
            host = std::make_shared<WorkerHost>(process, r, std::move(engine), true);
            process->fitLimit(!pooled);
        }
        catch(...)
        {
            // A plugin that took a shared host down as it loaded, or kept it from answering (failed by the lost reply), gets
            // a worker of its own from then on; the host's supervisor restarts it with the others.
            if(pooled && (process->broken.load() || process->failed.load())) pool().crashedIn(id);
            throw;
        }
        if(!pooled)
        {
            process->supervisor = ServiceThread([p = process.get()] { p->supervise(); });
            pool().track(process);
        }
        return host;
    }

    void setPluginCost(const std::string& pluginId, double share)
    {
        if(!std::isfinite(share) || share < 0) return;
        std::lock_guard lock(pool().mutex);
        pool().costs[pluginId] = std::min(share, 1.0);
    }

    size_t checkWorkers()
    {
        std::vector<std::shared_ptr<Process>> all;
        {
            std::lock_guard lock(pool().listed);
            for(const auto& w : pool().all) if(auto p = w.lock()) all.push_back(std::move(p));
        }
        size_t exited = 0;
        for(const auto& p : all) if(p->exitedNow()) ++exited;
        return exited;
    }

    std::vector<PluginHostInfo> pluginHosts()
    {
        std::vector<std::shared_ptr<Process>> all;
        {
            std::lock_guard lock(pool().listed);
            for(const auto& w : pool().all) if(auto p = w.lock()) all.push_back(std::move(p));
        }
        std::vector<PluginHostInfo> result;
        for(const auto& p : all)
        {
            PluginHostInfo info;
            { std::lock_guard control(p->control); info.pid = p->reaped ? 0 : long(p->pid); }
            info.pooled = p->pooled;
            { std::lock_guard nodes(p->nodesMutex); for(auto* n : p->nodes) info.plugins.push_back(n->descriptor().id); }
            { std::lock_guard lock(pool().mutex); info.cost = pool().cost(*p); }
            result.push_back(std::move(info));
        }
        return result;
    }
}
