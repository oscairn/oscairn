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
#include <string_view>
#include <stdexcept>
#include <sys/ioctl.h>
#include <sys/time.h>
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
        volatile std::sig_atomic_t woken = 0;
        void wakeHolder(int) { woken = 1; } // a reader attaching or going: the wait ends, and the holder takes it in or drops it

        // A reader's connection to the holder: what it has sent (a handover request is the only
        // thing read), and whether it has sent something that is not one.
        struct Reader
        {
            int fd;
            std::string in;
            bool plain;
        };
        constexpr std::string_view handoverTag = "OSCAIRN-PANEL-HANDOVER ";
        constexpr size_t handoverMost = 64; // bytes of a request kept; a longer one is malformed

        // `text` and descriptors, in one message (SCM_RIGHTS: the receiver gets its own descriptors
        // for the same open files).
        bool sendWithFds(int sock, const std::string& text, const std::vector<int>& fds)
        {
            alignas(cmsghdr) char control[CMSG_SPACE(sizeof(int) * 16)] = {};
            if(fds.size() > 16) { errno = E2BIG; return false; }
            iovec v{const_cast<char*>(text.data()), text.size()};
            msghdr m{};
            m.msg_iov = &v;
            m.msg_iovlen = 1;
            m.msg_control = control;
            m.msg_controllen = CMSG_SPACE(sizeof(int) * fds.size());
            cmsghdr* c = CMSG_FIRSTHDR(&m);
            c->cmsg_level = SOL_SOCKET;
            c->cmsg_type = SCM_RIGHTS;
            c->cmsg_len = CMSG_LEN(sizeof(int) * fds.size());
            std::memcpy(CMSG_DATA(c), fds.data(), sizeof(int) * fds.size());
            return ::sendmsg(sock, &m, MSG_NOSIGNAL) == ssize_t(text.size());
        }
        // Up to `size` bytes and the descriptors that came with them (at most 16; more or a cut-off
        // list closes them all and fails). Every descriptor is close-on-exec.
        ssize_t receiveWithFds(int sock, char* buffer, size_t size, std::vector<int>& fds)
        {
            alignas(cmsghdr) char control[CMSG_SPACE(sizeof(int) * 16)];
            iovec v{buffer, size};
            msghdr m{};
            m.msg_iov = &v;
            m.msg_iovlen = 1;
            m.msg_control = control;
            m.msg_controllen = sizeof control;
            const ssize_t n = ::recvmsg(sock, &m, 0);
            if(n < 0) return n;
            for(cmsghdr* c = CMSG_FIRSTHDR(&m); c; c = CMSG_NXTHDR(&m, c))
            {
                if(c->cmsg_level != SOL_SOCKET || c->cmsg_type != SCM_RIGHTS) continue;
                const size_t count = (c->cmsg_len - CMSG_LEN(0)) / sizeof(int);
                for(size_t i = 0; i < count; ++i)
                {
                    int one;
                    std::memcpy(&one, CMSG_DATA(c) + i * sizeof(int), sizeof one);
                    ::fcntl(one, F_SETFD, FD_CLOEXEC);
                    fds.push_back(one);
                }
            }
            if(m.msg_flags & MSG_CTRUNC) { for(const int f : fds) ::close(f); fds.clear(); errno = EMSGSIZE; return -1; }
            return n;
        }
        // True when the other end closed within `ms` (anything it sends meanwhile is read and dropped).
        bool endOfStream(int sock, int ms)
        {
            for(const auto end = std::chrono::steady_clock::now() + std::chrono::milliseconds(ms); std::chrono::steady_clock::now() < end;)
            {
                pollfd p{sock, POLLIN, 0};
                if(::poll(&p, 1, 50) != 1) continue;
                char junk[16];
                const ssize_t n = ::recv(sock, junk, sizeof junk, 0);
                if(n == 0 || (n < 0 && errno != EINTR && errno != EAGAIN)) return true;
            }
            return false;
        }
        // The new holder's side of a handover: asks the holder on `conn` for the panel and, if it
        // answers "OK <version>\n" with descriptors (device, socket, then readers), leaves them in
        // `fds` and returns 0. Returns panelHolderRefused when the holder refused (the same or a
        // newer version), panelHolderSilent when it did not answer in 1.5 seconds (a holder that
        // predates the handover reads the request as junk and says nothing), 1 for anything else.
        int requestHandover(int conn, int version, std::vector<int>& fds, const std::function<void(const std::string&)>& say)
        {
            const std::string request = std::string(handoverTag) + std::to_string(version) + "\n";
            if(::send(conn, request.data(), request.size(), MSG_NOSIGNAL) != ssize_t(request.size())) { say(std::string("the handover request was not sent: ") + std::strerror(errno)); return 1; }
            pollfd p{conn, POLLIN, 0};
            int ready;
            while((ready = ::poll(&p, 1, 1500)) < 0 && errno == EINTR) {}
            if(ready == 0) { say("the panel holder that runs did not answer a handover request: it is older than the handover; it keeps the panel"); return panelHolderSilent; }
            char reply[handoverMost];
            const ssize_t n = ready > 0 ? receiveWithFds(conn, reply, sizeof reply, fds) : -1;
            if(n == 0) { say("the panel holder that runs closed the connection at the handover request: it is older than the handover; it keeps the panel"); return panelHolderSilent; }
            if(n < 0) { say(std::string("the handover reply was not read: ") + std::strerror(errno)); return 1; }
            const std::string_view said(reply, size_t(n));
            auto closeAll = [&] { for(const int f : fds) ::close(f); fds.clear(); };
            if(said.substr(0, 8) == "REFUSED ") { closeAll(); say("the panel holder that runs refused the handover (its version is not below " + std::to_string(version) + "); it keeps the panel"); return panelHolderRefused; }
            // Device, socket, and up to eight readers; nothing else is accepted, and each must be what it should.
            struct stat sd{}, sl{};
            if(said.substr(0, 3) != "OK " || fds.size() < 2 || fds.size() > 10 || ::fstat(fds[0], &sd) != 0 || ::fstat(fds[1], &sl) != 0 || !S_ISSOCK(sl.st_mode))
            {
                closeAll();
                say("the handover reply was not understood");
                return 1;
            }
            return 0;
        }

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
            fields(v, {"kind", "id", "control", "max", "step", "invert", "navigate"}, "a control");
            Control c;
            const auto kind = text(member(v, "kind"), "a control's kind");
            c.kind = kind == "encoder" ? Kind::Encoder : kind == "analog" ? Kind::Analog : kind == "switch" ? Kind::SwitchOn : Kind::Unknown;
            require(c.kind != Kind::Unknown, "a control's kind is encoder, analog or switch");
            c.control = text(member(v, "control"), "a control's control");
            if(const auto* m = member(v, "max")) { require(c.kind == Kind::Analog, "only an analog control has a max"); c.max = whole(m, "a control's max", 1, 4294967295.0); }
            if(const auto* s = member(v, "step")) { require(c.kind == Kind::Encoder, "only an encoder has a step"); c.step = number(s, "a control's step", -1, 1); require(c.step != 0, "a step is not 0"); }
            if(const auto* i = member(v, "invert"))
            {
                require(c.kind != Kind::Encoder, "an encoder is not inverted (give it a negative step)");
                require(i->type == json_type_true || i->type == json_type_false, "a control's invert is true or false");
                c.invert = i->type == json_type_true;
            }
            if(const auto* n = member(v, "navigate"))
            {
                require(!c.invert, "an inverted control does not navigate");
                require(c.kind != Kind::Analog, "an analog control cannot navigate: an absolute position has no steps");
                fields(n, {"action", "pane", "steps"}, "a control's navigate");
                Navigation nav;
                const auto action = text(member(n, "action"), "a navigate's action");
                require(action == "move" || action == "confirm" || action == "cancel", "a navigate's action is move, confirm or cancel");
                nav.action = action == "move" ? Navigation::Action::Move : action == "confirm" ? Navigation::Action::Confirm : Navigation::Action::Cancel;
                require(c.kind != Kind::Encoder || nav.action == Navigation::Action::Move, "an encoder's navigate is move");
                if(const auto* pane = member(n, "pane")) nav.pane = int(whole(pane, "a navigate's pane", 0, 15));
                if(const auto* s = member(n, "steps"))
                {
                    require(c.kind == Kind::SwitchOn && nav.action == Navigation::Action::Move, "only a switch that moves has steps");
                    nav.steps = int(number(s, "a navigate's steps", -64, 64));
                    require(nav.steps != 0, "a navigate's steps is not 0");
                }
                c.navigate = nav;
            }
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
        case Kind::Analog: { const double p = std::clamp(double(e.value) / double(c->max ? c->max : map.analogMax), 0.0, 1.0); e.input = {ControlInput::Kind::Absolute, c->invert ? 1 - p : p}; } break;
        case Kind::SwitchOn: e.input = {ControlInput::Kind::Switch, c->invert ? 0.0 : 1.0}; break;
        case Kind::SwitchOff: e.input = {ControlInput::Kind::Switch, c->invert ? 1.0 : 0.0}; break;
        default: break;
        }
        if(c->navigate)
        {
            auto nav = *c->navigate;
            if(e.kind == Kind::Encoder) nav.steps = int(e.value) * (c->step < 0 ? -1 : 1);
            if(e.kind == Kind::SwitchOff || (e.kind == Kind::Encoder && nav.steps == 0)) return e;
            e.navigation = nav;
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
        ::sigaction(SIGALRM, &on, nullptr); // once a second, below: a SIGIO that came between the last look and the wait is not lost for long
        on.sa_handler = SIG_IGN;
        ::sigaction(SIGPIPE, &on, nullptr); // a reader gone: its send fails, and it is dropped
        sigset_t none;
        sigemptyset(&none);
        ::sigprocmask(SIG_SETMASK, &none, nullptr);

        // The handover first, when asked for: the holder that runs is asked to pass on the open
        // device, its socket and its readers. No socket is bound and the device is not opened here,
        // so it is never opened twice and never left unheld.
        int fd = -1, listener = -1;
        std::vector<Reader> readers;
        bool took = false;
        if(device.takeover)
        {
            const int conn = connectPanelHolder(socket);
            if(conn >= 0)
            {
                std::vector<int> fds;
                const int answer = requestHandover(conn, device.version, fds, say);
                if(answer != 0) { ::close(conn); return answer; }
                fd = fds[0];
                listener = fds[1];
                for(size_t i = 2; i < fds.size(); ++i) readers.push_back({fds[i], {}, false});
                ::fcntl(listener, F_SETOWN, ::getpid());
                ::fcntl(listener, F_SETFL, ::fcntl(listener, F_GETFL) | O_NONBLOCK | O_ASYNC);
                for(const auto& r : readers) { ::fcntl(r.fd, F_SETOWN, ::getpid()); ::fcntl(r.fd, F_SETFL, ::fcntl(r.fd, F_GETFL) | O_NONBLOCK | O_ASYNC); }
                // Told only now, when everything is held here; then the old holder ends, and its end
                // of this connection with it. Until it has, it still holds everything too.
                if(::send(conn, "TAKEN\n", 6, MSG_NOSIGNAL) != 6) say("the holder that handed over was not told so (" + std::string(std::strerror(errno)) + ")");
                else if(!endOfStream(conn, 5000)) say("the holder that handed over has not ended after five seconds");
                ::close(conn);
                took = true;
                say("took the panel over: " + path + " (open, never closed), the socket " + socket + " and " + std::to_string(readers.size()) + " readers");
            }
            else say("no panel holder runs at " + socket + ": starting one");
        }

        // The lock, before the device is touched: one holder per socket, so the device is never
        // opened twice.
        sockaddr_un a;
        const auto size = address(socket, a);
        if(!took)
        {
            listener = size ? ::socket(AF_UNIX, SOCK_STREAM, 0) : -1;
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
        }
        auto unbind = [&] { ::close(listener); if(socket[0] != '@') ::unlink(socket.c_str()); };

        // Opened only while no other process has it: a second open while another holds it fails
        // and, by Roland's driver, leaves the panel's next byte to crash the kernel.
        const auto until = std::chrono::steady_clock::now() + std::chrono::milliseconds(device.patience);
        while(!took)
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
        if(!took)
        {
            ::listen(listener, 8);
            ::fcntl(listener, F_SETOWN, ::getpid());
            ::fcntl(listener, F_SETFL, O_NONBLOCK | O_ASYNC);
        }
        say("holding " + path + "; readers attach at " + socket);

        uint64_t dropped = 0, turnedAway = 0;
        bool handedOver = false;
        auto drop = [&](size_t i) {
            ::close(readers[i].fd);
            readers.erase(readers.begin() + ptrdiff_t(i));
            say("a reader detached (" + std::to_string(readers.size()) + " left)");
        };
        // A newer holder's request, from the reader at `i`: granted to a higher version only. The
        // device, the socket and every other reader go to it in one message; this holder goes on
        // holding them, reading nothing from the device, until the new one says it has them (or
        // five seconds pass, and then it simply goes on). The device is not closed here: what is
        // handed over is the same open file, which stays open in the new holder when this one ends.
        auto handover = [&](size_t i, int version) {
            const int conn = readers[i].fd;
            if(version <= device.version)
            {
                say("a handover was asked for by a holder of version " + std::to_string(version) + ", not above this one's " + std::to_string(device.version) + ": refused");
                const auto no = "REFUSED " + std::to_string(device.version) + "\n";
                if(::send(conn, no.data(), no.size(), MSG_NOSIGNAL) < 0) {}
                drop(i);
                return;
            }
            std::vector<int> fds{fd, listener};
            for(size_t j = 0; j < readers.size(); ++j) if(j != i) fds.push_back(readers[j].fd);
            say("handing the panel over to a holder of version " + std::to_string(version));
            if(!sendWithFds(conn, "OK " + std::to_string(device.version) + "\n", fds)) { say(std::string("the handover could not be sent: ") + std::strerror(errno)); drop(i); return; }
            char said[16];
            ssize_t n = -1;
            for(const auto end = std::chrono::steady_clock::now() + std::chrono::seconds(5); std::chrono::steady_clock::now() < end;)
            {
                pollfd p{conn, POLLIN, 0};
                if(::poll(&p, 1, 100) != 1) continue;
                n = ::recv(conn, said, sizeof said, 0);
                if(n < 0 && (errno == EAGAIN || errno == EINTR)) continue;
                break;
            }
            if(n == 6 && std::memcmp(said, "TAKEN\n", 6) == 0) { handedOver = true; return; }
            say("the handover was not completed: this holder goes on holding the panel");
            drop(i);
        };
        // What a reader sends: nothing, in practice, and it is read and dropped; or a handover
        // request, "OSCAIRN-PANEL-HANDOVER <version>\n", the only thing read as one. At most 64 bytes
        // are kept; anything else, malformed or too long, ends that connection.
        auto heard = [&](size_t i, const char* data, size_t count) {
            auto& r = readers[i];
            if(r.plain) return;
            r.in.append(data, count);
            const std::string_view in = r.in, tag = handoverTag;
            if(in.substr(0, tag.size()) != tag.substr(0, in.size())) { r.plain = true; r.in.clear(); return; } // not a request: junk, as before
            const auto end = in.find('\n');
            if(end == std::string_view::npos && in.size() < handoverMost) return; // the rest is on its way
            int64_t version = -1;
            if(end != std::string_view::npos && end == in.size() - 1 && end > tag.size() && end - tag.size() <= 6)
            {
                version = 0;
                for(size_t k = tag.size(); k < end; ++k)
                {
                    if(in[k] < '0' || in[k] > '9') { version = -1; break; }
                    version = version * 10 + (in[k] - '0');
                }
            }
            if(version < 0) { say("a malformed handover request refused"); drop(i); return; }
            handover(i, int(version));
        };
        // Readers that have gone: their end of the socket closed. Readers send nothing; anything
        // they do send is read and dropped, a handover request apart.
        auto reap = [&] {
            for(size_t i = 0; i < readers.size() && !handedOver;)
            {
                pollfd p{readers[i].fd, POLLIN, 0};
                char junk[64];
                const ssize_t n = ::poll(&p, 1, 0) == 1 ? ::recv(readers[i].fd, junk, sizeof junk, 0) : 1;
                if(n == 0 || (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR)) { drop(i); continue; }
                const size_t before = readers.size();
                if(n > 0 && p.revents) heard(i, junk, size_t(n));
                if(readers.size() == before) ++i; // not dropped by what it sent
            }
        };
        // Every accept first clears the readers that have gone, so dead ones never fill the eight.
        // A reader turned away is said once, then counted: the count is said when a reader next attaches.
        auto attach = [&] {
            for(int c; (c = ::accept(listener, nullptr, nullptr)) >= 0;)
            {
                reap();
                if(handedOver) { ::close(c); return; }
                if(readers.size() >= 8)
                {
                    ::close(c);
                    if(!turnedAway++) say("a ninth reader turned away (eight live readers are attached); further ones are counted, and said when one attaches");
                    continue;
                }
                ::fcntl(c, F_SETFD, FD_CLOEXEC);
                ::fcntl(c, F_SETOWN, ::getpid());
                ::fcntl(c, F_SETFL, O_NONBLOCK | O_ASYNC); // its hang-up raises SIGIO, as an attach does
                readers.push_back({c, {}, false});
                say("a reader attached (" + std::to_string(readers.size()) + " now)" + (turnedAway ? "; " + std::to_string(turnedAway) + " turned away meanwhile" : ""));
                turnedAway = 0;
            }
        };
        int status = 0;
        const itimerval second{{1, 0}, {1, 0}};
        ::setitimer(ITIMER_REAL, &second, nullptr);
        while(!holderStop && !handedOver)
        {
            woken = 0;
            attach();
            reap();
            if(handedOver) break;
            if(woken) continue; // a reader came or went meanwhile: look again before waiting
            if(device.wait(fd) < 0 && errno != EINTR) { say(std::string("waiting on ") + path + ": " + std::strerror(errno)); status = 1; break; }
            if(holderStop) break;
            attach();
            reap();
            if(handedOver) break;
            uint8_t message[4]; // the driver copies 4 bytes whatever the count asks
            while(::read(fd, message, sizeof message) == 4) // one message a read; 0 once none is queued
                for(size_t i = 0; i < readers.size();)
                {
                    const auto n = ::send(readers[i].fd, message, sizeof message, 0);
                    if(n == ssize_t(sizeof message)) { ++i; continue; }
                    if(n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) { if(!dropped++) say("a reader is not keeping up; its messages are dropped"); ++i; continue; }
                    drop(i);
                }
        }
        const itimerval off{};
        ::setitimer(ITIMER_REAL, &off, nullptr);
        if(handedOver)
        {
            // Nothing is released: the device is an open file the new holder has too, and the socket
            // is still bound there. Closing this process's descriptors only drops this process's
            // references; the file is closed when the last one goes, and one is held. No termios, no
            // ioctl, no write, and no unlink of the socket's path.
            for(const auto& r : readers) ::close(r.fd);
            ::close(listener);
            ::close(fd);
            say("handed the panel over; ending");
            return 0;
        }
        for(const auto& r : readers) ::close(r.fd);
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
