// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A device panel's own messages to its controls (panel_reader.h).
#include "oscairn/panel_reader.h"
#include "JsonInput.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <poll.h>
#include <set>
#include <stdexcept>
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
            fields(v, {"bytes", "bits", "shift", "mask", "signed"}, what);
            PanelMap::Field f;
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
        fields(layout, {"kind", "kinds", "id", "value", "delta", "analogMax", "encoderStep"}, "layout");
        p.kind = field(member(layout, "kind"), "layout.kind");
        p.id = field(member(layout, "id"), "layout.id");
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
        e.id = uint32_t(map.id.read(message));
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

    PanelReader::PanelReader(std::string path, PanelMap map, Sink sink, Log log)
        : path_(std::move(path)), owned_(true), sink_(std::move(sink)), log_(std::move(log)), map_(std::make_shared<const PanelMap>(std::move(map))), thread_([this] { run(); }) {}

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
        bool reported = false; // an open failure, once until it opens
        while(!stop_)
        {
            if(fd_ < 0)
            {
                fd_ = ::open(path_.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
                if(fd_ < 0)
                {
                    if(!reported && log_) log_("cannot open " + path_ + ": " + std::strerror(errno));
                    reported = true;
                    if(!pause()) return;
                    continue;
                }
                if(reported && log_) log_("opened " + path_);
                reported = false;
                have_ = 0; // a message cut by the reopen is not joined to the next
            }
            pollfd p{fd_, POLLIN, 0};
            const int ready = ::poll(&p, 1, 100); // the timeout bounds how long stopping waits
            if(ready == 0 || (ready < 0 && errno == EINTR)) continue;
            uint8_t buffer[256];
            const auto n = ready > 0 ? ::read(fd_, buffer, sizeof buffer) : -1;
            if(n > 0) { bytes(buffer, size_t(n)); continue; }
            if(n < 0 && (errno == EAGAIN || errno == EINTR)) continue;
            if(!owned_) return; // a descriptor given: its end is the end
            if(log_) log_(path_ + (n == 0 ? " ended" : std::string(": ") + std::strerror(errno)) + "; opening it again");
            ::close(fd_); fd_ = -1;
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
