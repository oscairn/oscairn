// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A device panel's own messages to its controls (panel_reader.h).
#include "oscairn/panel_reader.h"
#include "JsonInput.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstddef>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <poll.h>
#include <set>
#include <stdexcept>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

namespace oscairn
{
    namespace
    {
        using detail::member;
        void require(bool ok, const std::string& why) { if(!ok) throw std::invalid_argument("panel map: " + why); }
        void fields(const json_value_s* v, std::initializer_list<std::string_view> allowed, const std::string& what)
        {
            require(v && v->type == json_type_object, what + " must be an object");
            for(auto* e = static_cast<const json_object_s*>(v->payload)->start; e; e = e->next)
            {
                const std::string_view name(e->name->string, e->name->string_size);
                require(std::find(allowed.begin(), allowed.end(), name) != allowed.end(), "unknown field " + std::string(name) + " in " + what);
            }
        }
        std::string text(const json_value_s* v, const std::string& what)
        {
            require(v && v->type == json_type_string, what + " must be a string");
            const auto* s = static_cast<const json_string_s*>(v->payload);
            require(s->string_size > 0 && s->string_size <= 256, what + " must be a short string");
            return {s->string, s->string_size};
        }
        double number(const json_value_s* v, const std::string& what, double low, double high)
        {
            require(v && v->type == json_type_number, what + " must be a number");
            const auto* n = static_cast<const json_number_s*>(v->payload);
            const double d = std::strtod(std::string(n->number, n->number_size).c_str(), nullptr);
            require(std::isfinite(d) && d >= low && d <= high, what + " must be from " + std::to_string(low) + " to " + std::to_string(high));
            return d;
        }
        int64_t whole(const json_value_s* v, const std::string& what, double low, double high)
        {
            const double d = number(v, what, low, high);
            require(d == std::floor(d), what + " must be a whole number");
            return int64_t(d);
        }
        template<class F> void each(const json_value_s* v, const std::string& what, size_t most, F&& f)
        {
            require(v && v->type == json_type_array, what + " must be a list");
            const auto* a = static_cast<const json_array_s*>(v->payload);
            require(a->length <= most, what + " is too long");
            for(auto* e = a->start; e; e = e->next) f(e->value);
        }
        PanelMap::Field field(const json_value_s* v, const std::string& what)
        {
            fields(v, {"bytes", "bits", "shift", "mask", "signed", "parts"}, what);
            PanelMap::Field f;
            if(const auto* parts = member(v, "parts"))
            {
                require(!member(v, "bytes") && !member(v, "bits") && !member(v, "shift") && !member(v, "mask"), what + " has parts, or bytes with bits, shift and mask");
                each(parts, what + ".parts", 4, [&](const json_value_s* p) {
                    fields(p, {"byte", "shift", "bits"}, what + "'s part");
                    PanelMap::Part part;
                    part.byte = uint8_t(whole(member(p, "byte"), what + "'s part's byte", 0, 3));
                    if(const auto* s = member(p, "shift")) part.shift = uint8_t(whole(s, what + "'s part's shift", 0, 7));
                    part.bits = uint8_t(whole(member(p, "bits"), what + "'s part's bits", 1, 8));
                    require(part.shift + part.bits <= 8, what + "'s part ends within its byte");
                    f.parts.push_back(part);
                });
                require(!f.parts.empty(), what + " has at least one part");
                if(const auto* s = member(v, "signed")) { require(s->type == json_type_true || s->type == json_type_false, what + ".signed must be true or false"); f.sign = s->type == json_type_true; }
                return f;
            }
            each(member(v, "bytes"), what + ".bytes", 4, [&](const json_value_s* b) { f.bytes.push_back(uint8_t(whole(b, what + ".bytes", 0, 3))); });
            require(!f.bytes.empty(), what + " names at least one byte");
            if(const auto* b = member(v, "bits")) f.bits = uint32_t(whole(b, what + ".bits", 1, 8));
            if(const auto* s = member(v, "shift")) f.shift = uint32_t(whole(s, what + ".shift", 0, 31));
            if(const auto* m = member(v, "mask")) f.mask = uint32_t(whole(m, what + ".mask", 1, 4294967295.0));
            if(const auto* s = member(v, "signed")) { require(s->type == json_type_true || s->type == json_type_false, what + ".signed must be true or false"); f.sign = s->type == json_type_true; }
            require(!f.sign || (f.mask & (uint64_t(f.mask) + 1)) == 0, what + ": a signed field's mask is a run of low bits");
            return f;
        }
        constexpr std::array<const char*, 4> kindNames{"encoder", "analog", "switchOn", "switchOff"};

        // A socket's address: "@name" an abstract one (Linux), otherwise a path. 0 when it does not fit.
        socklen_t address(const std::string& name, sockaddr_un& a)
        {
            a = {};
            a.sun_family = AF_UNIX;
            if(name.empty() || name.size() >= sizeof a.sun_path) { errno = ENAMETOOLONG; return 0; }
            std::memcpy(a.sun_path, name.data(), name.size());
            if(name[0] != '@') return sizeof a;
            a.sun_path[0] = 0;
            return socklen_t(offsetof(sockaddr_un, sun_path) + name.size());
        }

        volatile std::sig_atomic_t holderStop = 0;
        void stopHolder(int) { holderStop = 1; }
        void wakeHolder(int) {} // a reader attaching or going: the wait ends, and the holder takes it in or drops it

    }

    const char* kindName(PanelMap::Kind k)
    {
        switch(k)
        {
        case PanelMap::Kind::Encoder: return "encoder";
        case PanelMap::Kind::Analog: return "analog";
        case PanelMap::Kind::SwitchOn: return "switchOn";
        case PanelMap::Kind::SwitchOff: return "switchOff";
        default: return "unknown";
        }
    }

    int64_t PanelMap::Field::read(const std::array<uint8_t, 4>& m) const
    {
        uint64_t v = 0;
        if(!parts.empty())
        {
            uint32_t width = 0;
            for(const auto& p : parts) { v = (v << p.bits) | ((m[p.byte] >> p.shift) & ((1u << p.bits) - 1)); width += p.bits; }
            return sign && ((v >> (width - 1)) & 1) ? int64_t(v) - (int64_t(1) << width) : int64_t(v);
        }
        for(const auto b : bytes) v = (v << bits) | (m[b] & ((1u << bits) - 1));
        v = (v >> shift) & mask;
        if(sign && mask != 0xFFFFFFFF && (v & ((uint64_t(mask) + 1) >> 1))) return int64_t(v) - int64_t(uint64_t(mask) + 1);
        if(sign && mask == 0xFFFFFFFF) return int32_t(uint32_t(v));
        return int64_t(v);
    }

    PanelMap PanelMap::parse(std::string_view json)
    {
        require(json.size() <= 256 * 1024, "larger than 256 KiB");
        const auto bad = detail::jsonPreflight(json, 16);
        require(bad.empty(), std::string(bad));
        const std::unique_ptr<json_value_s, decltype(&std::free)> doc{json_parse(json.data(), json.size()), &std::free};
        require(doc != nullptr, "not valid JSON");
        const auto* root = doc.get();
        fields(root, {"format", "version", "protocol", "estimated", "layout", "controls"}, "the map");
        require(text(member(root, "format"), "format") == "oscairn.panel-map", "format must be oscairn.panel-map");
        require(whole(member(root, "version"), "version", 1, 1) == 1, "version must be 1");
        require(text(member(root, "protocol"), "protocol") == "roland-i2c-slave", "protocol must be roland-i2c-slave");
        PanelMap p;
        if(const auto* e = member(root, "estimated"))
            each(e, "estimated", 2, [&](const json_value_s* v) {
                p.estimated.push_back(text(v, "estimated"));
                require(p.estimated.back() == "layout" || p.estimated.back() == "controls", "estimated names layout or controls");
            });
        const auto* layout = member(root, "layout");
        fields(layout, {"kind", "kinds", "id", "analogId", "value", "delta", "analogMax", "encoderStep", "analogDeadband"}, "layout");
        p.kind = field(member(layout, "kind"), "layout.kind");
        p.id = field(member(layout, "id"), "layout.id");
        if(const auto* a = member(layout, "analogId")) p.analogId = field(a, "layout.analogId");
        p.value = field(member(layout, "value"), "layout.value");
        p.delta = field(member(layout, "delta"), "layout.delta");
        const auto* kinds = member(layout, "kinds");
        fields(kinds, {"encoder", "analog", "switchOn", "switchOff"}, "layout.kinds");
        std::set<int64_t> codes;
        for(size_t k = 0; k < kindNames.size(); ++k)
        {
            p.kinds[k] = whole(member(kinds, kindNames[k]), std::string("layout.kinds.") + kindNames[k], 0, 4294967295.0);
            require(codes.insert(p.kinds[k]).second, "two kinds share a value");
        }
        if(const auto* m = member(layout, "analogMax")) p.analogMax = whole(m, "layout.analogMax", 1, 4294967295.0);
        if(const auto* s = member(layout, "encoderStep")) p.encoderStep = number(s, "layout.encoderStep", 1e-6, 1);
        if(const auto* d = member(layout, "analogDeadband")) p.analogDeadband = whole(d, "layout.analogDeadband", 0, 4095);
        std::set<std::pair<int, uint32_t>> seen;
        each(member(root, "controls"), "controls", 1024, [&](const json_value_s* v) {
            fields(v, {"kind", "id", "control", "max", "step"}, "a control");
            Control c;
            const auto kind = text(member(v, "kind"), "a control's kind");
            c.kind = kind == "encoder" ? Kind::Encoder : kind == "analog" ? Kind::Analog : kind == "switch" ? Kind::SwitchOn : Kind::Unknown;
            require(c.kind != Kind::Unknown, "a control's kind is encoder, analog or switch");
            c.control = text(member(v, "control"), "a control's control");
            if(const auto* m = member(v, "max")) { require(c.kind == Kind::Analog, "only an analog control has a max"); c.max = whole(m, "a control's max", 1, 4294967295.0); }
            if(const auto* s = member(v, "step")) { require(c.kind == Kind::Encoder, "only an encoder has a step"); c.step = number(s, "a control's step", -1, 1); require(c.step != 0, "a step is not 0"); }
            const auto* id = member(v, "id");
            if(!id) return; // not mapped yet
            c.id = uint32_t(whole(id, "a control's id", 0, 4294967295.0));
            require(seen.insert({int(c.kind), c.id}).second, "two controls of one kind share id " + std::to_string(c.id));
            p.controls.push_back(std::move(c));
        });
        return p;
    }

    PanelEvent decode(const PanelMap& map, const std::array<uint8_t, 4>& message)
    {
        using Kind = PanelMap::Kind;
        PanelEvent e;
        e.raw = message;
        const auto k = map.kind.read(message);
        for(size_t i = 0; i < map.kinds.size(); ++i) if(map.kinds[i] == k) e.kind = Kind(i + 1);
        if(e.kind == Kind::Unknown) return e;
        const bool own = e.kind == Kind::Analog && !(map.analogId.bytes.empty() && map.analogId.parts.empty());
        e.id = uint32_t((own ? map.analogId : map.id).read(message));
        e.value = e.kind == Kind::Encoder ? map.delta.read(message) : map.value.read(message);
        const auto entry = e.kind == Kind::SwitchOff ? Kind::SwitchOn : e.kind; // one entry for a switch's two kinds
        const auto c = std::find_if(map.controls.begin(), map.controls.end(), [&](const PanelMap::Control& c) { return c.kind == entry && c.id == e.id; });
        if(c == map.controls.end()) return e;
        e.control = c->control;
        switch(e.kind)
        {
        case Kind::Encoder: e.input = {ControlInput::Kind::Relative, double(e.value) * (c->step ? c->step : map.encoderStep)}; break;
        case Kind::Analog: e.input = {ControlInput::Kind::Absolute, std::clamp(double(e.value) / double(c->max ? c->max : map.analogMax), 0.0, 1.0)}; break;
        case Kind::SwitchOn: e.input = {ControlInput::Kind::Switch, 1}; break;
        case Kind::SwitchOff: e.input = {ControlInput::Kind::Switch, 0}; break;
        default: break;
        }
        return e;
    }

    int connectPanelHolder(const std::string& socket)
    {
        sockaddr_un a;
        const auto size = address(socket, a);
        if(!size) return -1;
        const int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
        if(fd < 0) return -1;
        ::fcntl(fd, F_SETFD, FD_CLOEXEC);
        if(::connect(fd, reinterpret_cast<const sockaddr*>(&a), size) != 0) { const int error = errno; ::close(fd); errno = error; return -1; }
        return fd;
    }

    std::vector<int> processesHolding(const std::string& path)
    {
        std::vector<int> pids;
        struct stat target{};
        DIR* proc = ::stat(path.c_str(), &target) == 0 ? ::opendir("/proc") : nullptr;
        if(!proc) return pids;
        while(const auto* e = ::readdir(proc))
        {
            const int pid = std::atoi(e->d_name);
            if(pid <= 0 || pid == ::getpid()) continue;
            const std::string fds = std::string("/proc/") + e->d_name + "/fd";
            DIR* dir = ::opendir(fds.c_str());
            if(!dir) continue;
            while(const auto* f = ::readdir(dir))
            {
                struct stat s{}; // the file the descriptor is, compared by device and inode, so a path through a chroot's /dev matches too
                if(f->d_name[0] != '.' && ::stat((fds + "/" + f->d_name).c_str(), &s) == 0 && s.st_dev == target.st_dev && s.st_ino == target.st_ino) { pids.push_back(pid); break; }
            }
            ::closedir(dir);
        }
        ::closedir(proc);
        return pids;
    }

    int holdPanel(const std::string& path, const std::string& socket, const std::function<void(const std::string&)>& log, PanelDevice device)
    {
        if(!device.open) device.open = [](const std::string& p, int flags) { return ::open(p.c_str(), flags); };
        if(!device.wait) device.wait = [](int fd) { return ::ioctl(fd, 0, 0); };
        auto say = [&](const std::string& line) { if(log) log(line); };
        // A handled signal ends the driver's wait (it then returns 0): SIGTERM and SIGINT to stop,
        // SIGIO for a reader attaching. No SA_RESTART, and every signal unblocked, so nothing
        // inherited decides whether the wait can be woken. One thread, so the signal reaches it.
        holderStop = 0;
        struct sigaction on{};
        sigemptyset(&on.sa_mask);
        on.sa_handler = stopHolder;
        ::sigaction(SIGTERM, &on, nullptr);
        ::sigaction(SIGINT, &on, nullptr);
        on.sa_handler = wakeHolder;
        ::sigaction(SIGIO, &on, nullptr);
        on.sa_handler = SIG_IGN;
        ::sigaction(SIGPIPE, &on, nullptr); // a reader gone: its send fails, and it is dropped
        sigset_t none;
        sigemptyset(&none);
        ::sigprocmask(SIG_SETMASK, &none, nullptr);

        // The lock, before the device is touched: one holder per socket, so the device is never
        // opened twice.
        sockaddr_un a;
        const auto size = address(socket, a);
        const int listener = size ? ::socket(AF_UNIX, SOCK_STREAM, 0) : -1;
        if(listener < 0) { say("cannot make the socket " + socket + ": " + std::strerror(errno)); return 1; }
        ::fcntl(listener, F_SETFD, FD_CLOEXEC);
        auto bound = [&] { return ::bind(listener, reinterpret_cast<const sockaddr*>(&a), size) == 0; };
        bool ok = bound();
        if(!ok && errno == EADDRINUSE && socket[0] != '@')
        {
            // shortcut: a path's socket left by a holder that died is taken over, and two holders starting at the same moment could both do it; paths are for tests: the unit's abstract name is freed by the kernel with its holder
            const int other = connectPanelHolder(socket);
            if(other >= 0) { ::close(other); errno = EADDRINUSE; }
            else ok = ::unlink(socket.c_str()) == 0 && bound();
        }
        if(!ok)
        {
            say(errno == EADDRINUSE ? "another panel holder runs (" + socket + " is taken); not opening " + path : "cannot bind " + socket + ": " + std::strerror(errno));
            ::close(listener);
            return 1;
        }
        auto unbind = [&] { ::close(listener); if(socket[0] != '@') ::unlink(socket.c_str()); };

        // Opened only while no other process has it: a second open while another holds it fails
        // and, by Roland's driver, leaves the panel's next byte to crash the kernel.
        int fd = -1;
        const auto until = std::chrono::steady_clock::now() + std::chrono::milliseconds(device.patience);
        for(;;)
        {
            const bool late = std::chrono::steady_clock::now() >= until;
            const auto others = processesHolding(path);
            if(!others.empty())
            {
                if(late)
                {
                    std::string pids;
                    for(const int pid : others) pids += (pids.empty() ? "" : ", ") + std::to_string(pid);
                    say("process " + pids + " holds " + path + "; not opening it (a second open can crash the kernel)");
                    unbind();
                    return 1;
                }
            }
            else
            {
                fd = device.open(path, O_RDWR | O_NOCTTY | O_CLOEXEC); // read and write, as Roland's programs open it; never written
                if(fd >= 0) break;
                if((errno != EFAULT && errno != EBUSY) || late) { say("cannot open " + path + ": " + std::strerror(errno)); unbind(); return 1; }
            }
            if(holderStop) { unbind(); return 0; }
            ::usleep(50000);
        }
        ::listen(listener, 8);
        ::fcntl(listener, F_SETOWN, ::getpid());
        ::fcntl(listener, F_SETFL, O_NONBLOCK | O_ASYNC);
        say("holding " + path + "; readers attach at " + socket);

        std::vector<int> readers;
        uint64_t dropped = 0;
        auto attach = [&] {
            for(int c; (c = ::accept(listener, nullptr, nullptr)) >= 0;)
            {
                if(readers.size() >= 8) { ::close(c); say("a ninth reader turned away"); continue; }
                ::fcntl(c, F_SETFD, FD_CLOEXEC);
                ::fcntl(c, F_SETOWN, ::getpid());
                ::fcntl(c, F_SETFL, O_NONBLOCK | O_ASYNC); // its hang-up raises SIGIO, as an attach does
                readers.push_back(c);
                say("a reader attached (" + std::to_string(readers.size()) + " now)");
            }
        };
        auto drop = [&](size_t i) {
            ::close(readers[i]);
            readers.erase(readers.begin() + ptrdiff_t(i));
            say("a reader detached (" + std::to_string(readers.size()) + " left)");
        };
        // Readers that have gone: their end of the socket closed. Readers send nothing; anything
        // they do send is read and dropped.
        auto reap = [&] {
            for(size_t i = 0; i < readers.size();)
            {
                pollfd p{readers[i], POLLIN, 0};
                uint8_t junk[64];
                const ssize_t n = ::poll(&p, 1, 0) == 1 ? ::recv(readers[i], junk, sizeof junk, 0) : 1;
                if(n == 0 || (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR)) drop(i);
                else ++i;
            }
        };
        int status = 0;
        while(!holderStop)
        {
            attach();
            if(device.wait(fd) < 0 && errno != EINTR) { say(std::string("waiting on ") + path + ": " + std::strerror(errno)); status = 1; break; }
            if(holderStop) break;
            attach();
            reap();
            uint8_t message[4]; // the driver copies 4 bytes whatever the count asks
            while(::read(fd, message, sizeof message) == 4) // one message a read; 0 once none is queued
                for(size_t i = 0; i < readers.size();)
                {
                    const auto n = ::send(readers[i], message, sizeof message, 0);
                    if(n == ssize_t(sizeof message)) { ++i; continue; }
                    if(n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) { if(!dropped++) say("a reader is not keeping up; its messages are dropped"); ++i; continue; }
                    drop(i);
                }
        }
        for(const int r : readers) ::close(r);
        ::close(fd);
        unbind();
        say("released " + path);
        return status;
    }

    void PanelReader::standardError(const std::string& line) { std::fprintf(stderr, "oscairn: panel: %s\n", line.c_str()); }

    PanelReader::PanelReader(std::string socket, PanelMap map, Sink sink, Log log, int attached)
        : path_(std::move(socket)), fd_(attached), owned_(true), sink_(std::move(sink)), log_(std::move(log)), map_(std::make_shared<const PanelMap>(std::move(map)))
    {
        thread_ = std::thread([this] { run(); });
    }

    PanelReader::PanelReader(int fd, PanelMap map, Sink sink, Log log)
        : fd_(fd), sink_(std::move(sink)), log_(std::move(log)), map_(std::make_shared<const PanelMap>(std::move(map))), thread_([this] { run(); }) {}

    PanelReader::~PanelReader()
    {
        { std::lock_guard lock(lock_); stop_ = true; }
        stopped_.notify_all();
        thread_.join();
        if(owned_ && fd_ >= 0) ::close(fd_);
    }

    void PanelReader::setMap(PanelMap map)
    {
        auto next = std::make_shared<const PanelMap>(std::move(map));
        std::lock_guard lock(lock_);
        map_ = std::move(next);
    }

    PanelReader::Stats PanelReader::stats() const { return {messages_.load(), skipped_.load(), unknown_.load()}; }

    bool PanelReader::pause()
    {
        std::unique_lock lock(lock_);
        return !stopped_.wait_for(lock, std::chrono::seconds(1), [this] { return stop_.load(); });
    }

    void PanelReader::run()
    {
        bool reported = false; // the holder not reached, once until it is
        // A holder that ends each connection at once (one that turns a ninth reader away; an older
        // holder kept readers that had gone until the panel next sent): said once, then quiet until
        // a connection holds (a second, or a message), so the log is not a line a second.
        bool refused = false, held = false;
        auto since = std::chrono::steady_clock::now();
        auto holds = [&] {
            if(held) return;
            held = true;
            if(refused && log_) log_("attached to the panel holder at " + path_);
            refused = false;
        };
        while(!stop_)
        {
            if(fd_ < 0)
            {
                if((fd_ = connectPanelHolder(path_)) < 0)
                {
                    if(!reported && log_) log_("no panel holder at " + path_ + " (" + std::strerror(errno) + "); trying again each second");
                    reported = true;
                    if(!pause()) return;
                    continue;
                }
                if(log_ && !refused) log_("attached to the panel holder at " + path_);
                reported = false;
                held = false;
                since = std::chrono::steady_clock::now();
                have_ = 0; // a message cut by the reattach is not joined to the next
            }
            pollfd p{fd_, POLLIN, 0};
            const int ready = ::poll(&p, 1, 100); // the timeout bounds how long stopping waits
            if(!held && std::chrono::steady_clock::now() - since >= std::chrono::seconds(1)) holds();
            if(ready == 0 || (ready < 0 && errno == EINTR)) continue;
            uint8_t buffer[256];
            const auto n = ready > 0 ? ::read(fd_, buffer, sizeof buffer) : -1;
            if(n > 0) { holds(); bytes(buffer, size_t(n)); continue; }
            if(n < 0 && (errno == EAGAIN || errno == EINTR)) continue;
            if(!owned_) return; // a descriptor given: its end is the end
            const int error = errno;
            ::close(fd_); fd_ = -1;
            if(held) { if(log_) log_("the panel holder at " + path_ + (n == 0 ? " ended" : std::string(": ") + std::strerror(error)) + "; attaching again"); }
            else if(!refused)
            {
                refused = true;
                if(log_) log_("the panel holder at " + path_ + " ends this reader's connection at once (it turns readers away: one with eight readers already, or an older holder keeping readers that went); trying again each second, logged once attached");
            }
            if(!pause()) return;
        }
    }

    void PanelReader::bytes(const uint8_t* data, size_t size)
    {
        std::shared_ptr<const PanelMap> map;
        { std::lock_guard lock(lock_); map = map_; }
        for(size_t i = 0; i < size; ++i)
        {
            const uint8_t b = data[i];
            if(b & 0x80) { skipped_ += have_; have_ = 0; } // a new message: one cut short before it is dropped
            else if(have_ == 0) { ++skipped_; continue; }  // not in a message: wait for its start
            frame_[have_++] = b;
            if(have_ < frame_.size()) continue;
            have_ = 0;
            ++messages_;
            auto e = decode(*map, frame_);
            if(e.kind == PanelMap::Kind::Analog && !e.control.empty() && map->analogDeadband > 0)
            {
                auto& last = analog_[e.id];
                const auto d = e.value - last.value;
                const bool end = e.input.value <= 0 || e.input.value >= 1; // the ends are always reached
                const bool back = last.direction == 0 || (d > 0) != (last.direction > 0);
                e.chatter = last.value >= 0 && (d == 0 || (!end && back && std::abs(d) <= map->analogDeadband));
                if(!e.chatter) { if(last.value >= 0) last.direction = d > 0 ? 1 : -1; last.value = e.value; }
            }
            if(e.control.empty())
            {
                ++unknown_;
                const auto second = std::chrono::duration_cast<std::chrono::seconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
                if(second != window_) { window_ = second; logged_ = 0; }
                if(log_ && logged_ < 10)
                {
                    ++logged_;
                    char line[160];
                    std::snprintf(line, sizeof line, "unknown message %02x %02x %02x %02x (%s, id %u, value %lld)%s", e.raw[0], e.raw[1], e.raw[2], e.raw[3], kindName(e.kind), e.id,
                                  (long long)e.value, hidden_ ? (" after " + std::to_string(hidden_) + " not shown").c_str() : "");
                    hidden_ = 0;
                    log_(line);
                }
                else ++hidden_;
            }
            if(sink_) sink_(e);
        }
    }
}
