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
#include <cstring>
#include <fcntl.h>
#include <poll.h>
#include <set>
#include <stdexcept>
#include <sys/ioctl.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>
#ifdef __linux__
#include <sys/prctl.h>
#endif

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
        fields(layout, {"kind", "kinds", "id", "analogId", "value", "delta", "analogMax", "encoderStep"}, "layout");
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

    void PanelReader::standardError(const std::string& line) { std::fprintf(stderr, "oscairn: panel: %s\n", line.c_str()); }

    PanelReader::PanelReader(std::string path, PanelMap map, Sink sink, Log log, Device device)
        : path_(std::move(path)), device_(std::move(device)), owned_(true), roland_(true), sink_(std::move(sink)), log_(std::move(log)),
          map_(std::make_shared<const PanelMap>(std::move(map)))
    {
        if(!device_.open) device_.open = [](const std::string& p, int flags) { return ::open(p.c_str(), flags); };
        if(!device_.wait) device_.wait = [](int fd) { return ::ioctl(fd, 0, 0); };
        thread_ = std::thread([this] { run(); });
    }

    PanelReader::PanelReader(int fd, PanelMap map, Sink sink, Log log)
        : fd_(fd), sink_(std::move(sink)), log_(std::move(log)), map_(std::make_shared<const PanelMap>(std::move(map))), thread_([this] { run(); }) {}

    PanelReader::~PanelReader()
    {
        { std::lock_guard lock(lock_); stop_ = true; }
        stopped_.notify_all();
        thread_.join();
        if(helper_ > 0) { ::kill(helper_, SIGKILL); ::waitpid(helper_, nullptr, 0); } // a fatal signal ends the driver's wait
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

    // The helper process, after fork: system calls only (the engine's other threads are not here).
    // It ends with what went wrong as its exit status: an open's errno; 128 and the ioctl's errno;
    // 254 a failed read; 255 a hundred wakes in a row with nothing to read.
    void PanelReader::helper(int out)
    {
#ifdef __linux__
        ::prctl(PR_SET_PDEATHSIG, SIGKILL); // not left waiting in the driver when the engine goes
#endif
        struct sigaction fallback{};
        fallback.sa_handler = SIG_DFL;
        ::sigaction(SIGPIPE, &fallback, nullptr); // the engine gone: the next write ends it
        const int fd = device_.open(path_, O_RDWR | O_NOCTTY | O_CLOEXEC); // read and write, as Roland's programs open it; never written
        if(fd < 0) ::_exit(errno & 127 ? errno & 127 : 127);
        rlimit files{};
        const int most = ::getrlimit(RLIMIT_NOFILE, &files) == 0 && files.rlim_cur < 65536 ? int(files.rlim_cur) : 65536;
        for(int i = 3; i < most; ++i) // the engine's descriptors (audio devices, sockets) are not held open here
            if(i != fd && i != out && std::find(device_.keep.begin(), device_.keep.end(), i) == device_.keep.end()) ::close(i);
        for(int empty = 0;;)
        {
            if(device_.wait(fd) < 0)
            {
                if(errno == EINTR) continue;
                ::_exit(128 + (errno & 63));
            }
            uint8_t message[4];
            const auto n = ::read(fd, message, sizeof message); // one message after each wait; a second read gives 0
            if(n > 0) { empty = 0; if(::write(out, message, size_t(n)) != n) ::_exit(0); continue; }
            if(n < 0 && errno != EAGAIN && errno != EINTR) ::_exit(254);
            if(++empty == 100) ::_exit(255);
        }
    }

    bool PanelReader::spawn()
    {
        int p[2];
        if(::pipe(p) != 0) return false;
        ::fcntl(p[0], F_SETFD, FD_CLOEXEC);
        ::fcntl(p[1], F_SETFD, FD_CLOEXEC);
        const pid_t pid = ::fork();
        if(pid == 0) { ::close(p[0]); helper(p[1]); }
        ::close(p[1]);
        if(pid < 0) { ::close(p[0]); return false; }
        helper_ = pid;
        fd_ = p[0];
        return true;
    }

    // The helper ended (its pipe did): why, from its exit status. False when it could not open the device.
    bool PanelReader::reap()
    {
        int status = 0;
        const auto pid = helper_;
        helper_ = -1;
        if(::waitpid(pid, &status, 0) != pid || !WIFEXITED(status))
        {
            if(log_) log_("the helper reading " + path_ + " ended; starting it again");
            return true;
        }
        const int code = WEXITSTATUS(status);
        if(code >= 128)
        {
            if(log_) log_(code == 255 ? "ioctl 0 on " + path_ + " woke 100 times with nothing to read; reading it read-only and polling instead"
                          : code == 254 ? path_ + ": a read failed; reading it read-only and polling instead"
                          : "ioctl 0 on " + path_ + ": " + std::strerror(code - 128) + "; reading it read-only and polling instead");
            roland_ = false;
            return true;
        }
        errno = code;
        return false;
    }

    void PanelReader::run()
    {
        bool reported = false; // an open failure, once until it opens
        while(!stop_)
        {
            if(fd_ < 0)
            {
                if(roland_ ? !spawn() : (fd_ = device_.open(path_, O_RDONLY | O_NONBLOCK | O_CLOEXEC)) < 0)
                {
                    if(!reported && log_) log_("cannot open " + path_ + ": " + std::strerror(errno));
                    reported = true;
                    if(!pause()) return;
                    continue;
                }
                have_ = 0; // a message cut by the reopen is not joined to the next
            }
            pollfd p{fd_, POLLIN, 0};
            const int ready = ::poll(&p, 1, 100); // the timeout bounds how long stopping waits
            if(ready == 0 || (ready < 0 && errno == EINTR)) continue;
            uint8_t buffer[256];
            const auto n = ready > 0 ? ::read(fd_, buffer, sizeof buffer) : -1;
            if(n > 0)
            {
                if(reported && log_) log_("reading " + path_);
                reported = false;
                bytes(buffer, size_t(n));
                continue;
            }
            if(n < 0 && (errno == EAGAIN || errno == EINTR)) continue;
            if(!owned_) return; // a descriptor given: its end is the end
            const int error = errno;
            ::close(fd_); fd_ = -1;
            if(helper_ > 0)
            {
                const bool roland = roland_;
                if(!reap())
                {
                    if(!reported && log_) log_("cannot open " + path_ + ": " + std::strerror(errno));
                    reported = true;
                }
                if(roland && !roland_) continue; // fell back: opened again at once, read-only
            }
            else if(log_) log_(path_ + (n == 0 ? " ended" : std::string(": ") + std::strerror(error)) + "; opening it again");
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
            const auto e = decode(*map, frame_);
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
