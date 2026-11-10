// SPDX-License-Identifier: AGPL-3.0-or-later
// A profile's hardware section (docs/profile.schema.json, RFC 0024), read into capability records.
#include <oscairn/hardware.h>
#include "sheredom/json.h"
#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstdlib>
#include <set>
#include <stdexcept>

namespace oscairn::hardware {
namespace {
[[noreturn]] void invalid(const std::string& why) { throw std::invalid_argument("hardware: " + why); }

const json_value_s* member(const json_value_s* v, std::string_view key) {
    if (!v || v->type != json_type_object) return nullptr;
    for (auto* e = static_cast<const json_object_s*>(v->payload)->start; e; e = e->next)
        if (std::string_view(e->name->string, e->name->string_size) == key) return e->value;
    return nullptr;
}
void fields(const json_value_s* v, std::initializer_list<std::string_view> allowed, const std::string& what) {
    if (!v || v->type != json_type_object) invalid(what + " is an object");
    for (auto* e = static_cast<const json_object_s*>(v->payload)->start; e; e = e->next)
        if (std::find(allowed.begin(), allowed.end(), std::string_view(e->name->string, e->name->string_size)) == allowed.end())
            invalid("unknown field " + std::string(e->name->string, e->name->string_size) + " in " + what);
}
std::string text(const json_value_s* v, const std::string& what, size_t most) {
    if (!v || v->type != json_type_string) invalid(what + " is a string");
    const auto* s = static_cast<const json_string_s*>(v->payload);
    if (s->string_size == 0 || s->string_size > most) invalid(what + " holds 1 to " + std::to_string(most) + " characters");
    return {s->string, s->string_size};
}
double number(const json_value_s* v, const std::string& what) {
    if (!v || v->type != json_type_number) invalid(what + " is a number");
    const auto* n = static_cast<const json_number_s*>(v->payload);
    const std::string s(n->number, n->number_size);
    char* end = nullptr;
    const double d = std::strtod(s.c_str(), &end);
    if (end != s.c_str() + s.size() || !std::isfinite(d)) invalid(what + " is a number");
    return d;
}
uint64_t integer(const json_value_s* v, const std::string& what, uint64_t most) {
    const double d = number(v, what);
    if (d < 0 || d > double(most) || std::floor(d) != d) invalid(what + " is a whole number from 0 to " + std::to_string(most));
    return uint64_t(d);
}
bool boolean(const json_value_s* v, const std::string& what) {
    if (!v || (v->type != json_type_true && v->type != json_type_false)) invalid(what + " is true or false");
    return v->type == json_type_true;
}
std::vector<const json_value_s*> array(const json_value_s* v, const std::string& what, size_t most) {
    if (!v || v->type != json_type_array) invalid(what + " is an array");
    std::vector<const json_value_s*> items;
    for (auto* e = static_cast<const json_array_s*>(v->payload)->start; e; e = e->next) items.push_back(e->value);
    if (items.size() > most) invalid(what + " holds at most " + std::to_string(most));
    return items;
}
bool identifier(const std::string& s) {
    return std::all_of(s.begin(), s.end(), [](char c) { return std::isalnum(static_cast<unsigned char>(c)) || c == '.' || c == '-'; });
}
// "<family>/<version>": a family of lowercase letters, digits, dots and hyphens; a version number.
bool versioned(const std::string& s) {
    const auto slash = s.rfind('/');
    if (slash == std::string::npos || slash == 0 || slash + 1 == s.size()) return false;
    return std::all_of(s.begin(), s.begin() + long(slash), [](char c) { return std::islower(static_cast<unsigned char>(c)) || std::isdigit(static_cast<unsigned char>(c)) || c == '.' || c == '-'; }) &&
           std::all_of(s.begin() + long(slash) + 1, s.end(), [](char c) { return std::isdigit(static_cast<unsigned char>(c)); });
}
std::vector<std::string> names(const json_value_s* v, const std::string& what) {
    std::vector<std::string> out;
    for (auto* item : array(v, what, 256)) {
        out.push_back(text(item, what + "' items", 63));
        if (std::count(out.begin(), out.end(), out.back()) > 1) invalid(what + " lists " + out.back() + " twice");
    }
    return out;
}
Evidence evidence(const json_value_s* v, const std::string& what) {
    if (!v) return Evidence::documented;
    const auto e = text(v, what, 16);
    if (e == "documented") return Evidence::documented;
    if (e == "measured") return Evidence::measured;
    invalid(what + " is documented or measured");
}
} // namespace

Capabilities fromProfile(std::string_view profileJson) {
    std::unique_ptr<json_value_s, decltype(&std::free)> root(json_parse(profileJson.data(), profileJson.size()), &std::free);
    if (!root) invalid("the profile is not valid JSON");
    Capabilities c;
    const auto* h = member(root.get(), "hardware");
    if (!h) return c;
    fields(h, {"accelerators", "clocks"}, "hardware");
    if (const auto* clocks = member(h, "clocks"))
        for (auto* v : array(clocks, "hardware.clocks", 16)) {
            fields(v, {"id", "rate", "driftPpm", "leadMicros", "queue", "cancel", "evidence"}, "a clock");
            ClockDomain d;
            d.id = text(member(v, "id"), "a clock's id", 63);
            if (!identifier(d.id) || d.id == "audio") invalid("a clock's id holds letters, digits, dots and hyphens, and is not audio (the audio clock's)");
            for (const auto& other : c.clocks) if (other.id == d.id) invalid("two clocks are named " + d.id);
            d.rate = number(member(v, "rate"), "clock " + d.id + "'s rate");
            if (d.rate <= 0) invalid("clock " + d.id + "'s rate is above 0");
            if (const auto* x = member(v, "driftPpm")) { d.driftPpm = number(x, "clock " + d.id + "'s driftPpm"); if (*d.driftPpm < 0) invalid("clock " + d.id + "'s driftPpm is 0 or more"); }
            if (const auto* x = member(v, "leadMicros")) d.leadMicros = uint32_t(integer(x, "clock " + d.id + "'s leadMicros", 10000000));
            if (const auto* x = member(v, "queue")) d.queue = uint32_t(integer(x, "clock " + d.id + "'s queue", 1u << 20));
            if (const auto* x = member(v, "cancel")) d.cancel = boolean(x, "clock " + d.id + "'s cancel");
            d.evidence = evidence(member(v, "evidence"), "clock " + d.id + "'s evidence");
            c.clocks.push_back(std::move(d));
        }
    if (const auto* accelerators = member(h, "accelerators"))
        for (auto* v : array(accelerators, "hardware.accelerators", 16)) {
            fields(v, {"id", "kind", "abi", "device", "firmware", "operations", "banks", "regions", "voices", "parts", "memory", "clock",
                       "pipelineFrames", "offline", "evidence"}, "an accelerator");
            Accelerator a;
            a.id = text(member(v, "id"), "an accelerator's id", 63);
            if (!identifier(a.id)) invalid("an accelerator's id holds letters, digits, dots and hyphens");
            for (const auto& other : c.accelerators) if (other.id == a.id) invalid("two accelerators are named " + a.id);
            const std::string what = "accelerator " + a.id + "'s ";
            a.kind = text(member(v, "kind"), what + "kind", 31);
            if (a.kind != "dsp" && a.kind != "fpga" && a.kind != "sound-chip" && a.kind != "realtime-core" && a.kind != "simulated")
                invalid(what + "kind is dsp, fpga, sound-chip, realtime-core or simulated");
            a.abi = text(member(v, "abi"), what + "abi", 63);
            if (!versioned(a.abi)) invalid(what + "abi is <family>/<version>");
            if (const auto* x = member(v, "device")) a.device = text(x, what + "device", 128);
            if (const auto* f = member(v, "firmware")) {
                fields(f, {"id", "version"}, what + "firmware");
                a.firmware.id = text(member(f, "id"), what + "firmware id", 63);
                a.firmware.version = text(member(f, "version"), what + "firmware version", 31);
            }
            a.operations = names(member(v, "operations"), what + "operations");
            if (a.operations.empty()) invalid(what + "operations lists at least one");
            const auto family = a.abi.substr(0, a.abi.rfind('/'));
            for (const auto& o : a.operations)
                if (!versioned(o) || o.rfind(family + ".", 0) != 0) invalid(what + "operation " + o + " is " + family + ".<name>/<version>");
            if (const auto* x = member(v, "banks")) a.banks = names(x, what + "banks");
            if (const auto* x = member(v, "regions")) a.regions = names(x, what + "regions");
            if (const auto* x = member(v, "voices")) a.voices = uint32_t(integer(x, what + "voices", 1000000));
            if (const auto* x = member(v, "parts")) a.parts = uint32_t(integer(x, what + "parts", 1024));
            if (const auto* x = member(v, "memory")) a.memory = integer(x, what + "memory", uint64_t(1) << 50);
            if (const auto* x = member(v, "clock")) {
                a.clock = text(x, what + "clock", 63);
                if (std::none_of(c.clocks.begin(), c.clocks.end(), [&](const ClockDomain& d) { return d.id == a.clock; }))
                    invalid(what + "clock " + a.clock + " is not in hardware.clocks");
            }
            if (const auto* x = member(v, "pipelineFrames")) a.pipelineFrames = uint32_t(integer(x, what + "pipelineFrames", 65536));
            if (const auto* x = member(v, "offline")) a.offline = boolean(x, what + "offline");
            a.evidence = evidence(member(v, "evidence"), what + "evidence");
            c.accelerators.push_back(std::move(a));
        }
    return c;
}

} // namespace oscairn::hardware
