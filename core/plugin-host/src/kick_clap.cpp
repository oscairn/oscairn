// SPDX-License-Identifier: AGPL-3.0-or-later
// kick_clap/Host.h's and Discovery.h's functions for C++ callers, over the Rust CLAP host (core/clap-host/rust, through
// clap_host.h's rust_clap_* calls): its process-wide hardware and activation observer, and its discovery, where a scan
// crosses as the scan wire format, which this file reads and writes for the scan helpers as the Rust side does. And
// oscairn_clap_assets_of, an AssetResolver as the table the Rust host calls.
#include "clap_host.h"
#include <kick_clap/Discovery.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <stdexcept>

namespace kick::clap {
namespace {
// The process's hardware and observer, as the table and the call the Rust host takes (services.rs).
std::atomic<HardwareAccess*> hardwareAccess{nullptr};
Host::ActivationObserver activationObserver = nullptr;
void* activationContext = nullptr;
HardwareAccess* access() noexcept { return hardwareAccess.load(); }
const oscairn_clap_hardware hardwareTable{nullptr,
    [](void*) { return access()->count(); },
    [](void*, uint32_t index, oscairn_hardware_accelerator_t* out) { return access()->get(index, *out); },
    [](void*, uint32_t index, const char* operation) { return access()->supports(index, operation); },
    [](void*, const void* owner, uint32_t index, const oscairn_hardware_request_t* request, oscairn_hardware_lease_t* lease) { return access()->reserve(owner, index, *request, *lease); },
    [](void*, const void* owner, uint32_t lease) { access()->release(owner, lease); },
    [](void*, const void* owner) { access()->releaseAll(owner); },
    [](void*, const void* owner, uint32_t epoch, uint32_t lease, const oscairn_hardware_job_t* job) { return access()->submit(owner, epoch, lease, *job); },
    [](void*, const void* owner, uint32_t epoch, uint32_t lease, oscairn_hardware_result_t* result) { return access()->collect(owner, epoch, lease, *result); }};
// The observer called inside a try: what it throws is the refusal's text, kept until this thread's next activation.
const char* activating(void*, const oscairn_memory_t* declared) {
    static thread_local std::string refused;
    try { activationObserver(activationContext, declared); return nullptr; }
    catch (const std::exception& e) { refused = e.what(); }
    catch (...) { refused = "Activation refused"; }
    return refused.c_str();
}
constexpr size_t maxWireBytes = 4 * 1024 * 1024;
constexpr const char* magic = "KICKCLAP1\n";
void number(std::string& out, uint32_t n) { for (int i = 0; i < 4; ++i) out.push_back(static_cast<char>((n >> (8 * i)) & 255)); }
void field(std::string& out, const std::string& s) { number(out, static_cast<uint32_t>(s.size())); out += s; }
struct Reader {
    const std::string& source;
    size_t position;
    uint32_t number() {
        if (source.size() - position < 4) throw std::runtime_error("Truncated scanner output");
        uint32_t n = 0;
        for (int i = 0; i < 4; ++i) n |= static_cast<uint32_t>(static_cast<uint8_t>(source[position++])) << (8 * i);
        return n;
    }
    std::string field() {
        const auto n = number();
        if (n > maxWireBytes || n > source.size() - position) throw std::runtime_error("Invalid scanner field");
        auto s = source.substr(position, n); position += n; return s;
    }
};
// The bytes a rust_clap_* call gives, kept.
void keep(void* to, const char* bytes, size_t size) { static_cast<std::string*>(to)->assign(bytes, size); }
void add(void* to, const char* bytes, size_t size) { static_cast<std::vector<std::filesystem::path>*>(to)->emplace_back(std::string(bytes, size)); }
ScanResult decoded(const std::string& wire, const std::string& path) {
    ScanResult result;
    if (!decodeScan(wire, result)) result.failures.push_back({path, "Invalid scanner response"});
    return result;
}
struct Strings {
    std::vector<std::string> owned; std::vector<const char*> pointers;
    template<class List> explicit Strings(const List& list) {
        for (const auto& s : list) owned.push_back(std::filesystem::path(s).string());
        for (const auto& s : owned) pointers.push_back(s.c_str());
    }
};
}

void Host::observeActivations(ActivationObserver observer, void* context) noexcept {
    activationObserver = observer; activationContext = context;
    rust_clap_observe_activations(observer ? activating : nullptr, nullptr);
}
void Host::setHardware(HardwareAccess* hardware) noexcept { hardwareAccess.store(hardware); rust_clap_set_hardware(hardware ? &hardwareTable : nullptr); }

std::vector<std::filesystem::path> defaultSearchPaths() {
    std::vector<std::filesystem::path> paths;
    rust_clap_search_paths(add, &paths);
    return paths;
}
std::vector<std::filesystem::path> findBinaries(const std::vector<std::filesystem::path>& roots, const std::string& extension) {
    const Strings list(roots);
    std::vector<std::filesystem::path> found;
    rust_clap_find_binaries(list.pointers.data(), uint32_t(list.pointers.size()), extension.c_str(), add, &found);
    return found;
}
ScanResult inspect(const std::filesystem::path& binary, bool presets, const AssetResolver* assets) {
    const auto table = oscairn_clap_assets_of(const_cast<AssetResolver*>(assets));
    std::string wire;
    rust_clap_inspect(binary.c_str(), presets, assets ? &table : nullptr, keep, &wire);
    return decoded(wire, binary.string());
}
ScanResult scan(const std::vector<std::filesystem::path>& roots, const std::filesystem::path& helper, uint32_t timeoutMs,
                const std::atomic<bool>* canceled, const std::string& extension, uint32_t presetTimeoutMs) {
    const Strings list(roots);
    std::string wire;
    rust_clap_scan(list.pointers.data(), uint32_t(list.pointers.size()), helper.c_str(), timeoutMs, canceled, extension.c_str(), presetTimeoutMs, keep, &wire);
    return decoded(wire, helper.string());
}
bool indexPresetsWithHelper(const std::filesystem::path& helper, const std::filesystem::path& binary, uint32_t timeoutMs, ScanResult& result, std::string& failure) {
    std::string given;
    if (!rust_clap_index_presets_with_helper(helper.c_str(), binary.c_str(), timeoutMs, keep, &given)) { failure = given; return false; }
    if (decodeScan(given, result)) return true;
    failure = "Invalid scanner response";
    return false;
}
PresetIndex indexPresets(const clap_plugin_entry_t* entry, const std::vector<std::string>& pluginIds, uint32_t budgetMs, const AssetResolver* assets) {
    const Strings ids(pluginIds);
    const auto table = oscairn_clap_assets_of(const_cast<AssetResolver*>(assets));
    PresetIndex index;
    rust_clap_index_presets(entry, ids.pointers.data(), uint32_t(ids.pointers.size()), budgetMs, assets ? &table : nullptr,
        [](void* c, const char* id, const oscairn_clap_preset* p) { static_cast<PresetIndex*>(c)->presets[id].push_back({p->name, p->kind, p->location, p->load_key}); },
        [](void* c, const char* text) { static_cast<PresetIndex*>(c)->problems.emplace_back(text); }, &index);
    return index;
}
IsolatedRun runIsolated(const std::vector<std::string>& arguments, uint32_t timeoutMs, size_t maxOutput, const std::atomic<bool>* canceled,
                        uint64_t memoryLimit, uint64_t (*memoryOf)(long)) {
    const Strings list(arguments);
    struct Taken { IsolatedRun run; } taken;
    const auto outcome = rust_clap_run_isolated(list.pointers.data(), uint32_t(list.pointers.size()), timeoutMs, maxOutput, canceled, memoryLimit, memoryOf,
        [](void* c, const char* b, size_t n) { static_cast<Taken*>(c)->run.output.assign(b, n); },
        [](void* c, const char* b, size_t n) { static_cast<Taken*>(c)->run.stderrTail.assign(b, n); }, &taken);
    taken.run.outcome = static_cast<IsolatedRun::Outcome>(outcome);
    return taken.run;
}
void watchParentPipe() { rust_clap_watch_parent_pipe(); }

std::string encodeScan(const ScanResult& result) {
    std::string out = magic;
    number(out, static_cast<uint32_t>(result.plugins.size()));
    for (const auto& p : result.plugins) {
        field(out, p.path); field(out, p.id); field(out, p.name); field(out, p.vendor); field(out, p.version); field(out, p.description);
        number(out, static_cast<uint32_t>(p.features.size()));
        for (const auto& f : p.features) field(out, f);
        number(out, p.presetDiscovery); number(out, static_cast<uint32_t>(p.presets.size()));
        for (const auto& preset : p.presets) { field(out, preset.name); number(out, preset.kind); field(out, preset.location); field(out, preset.loadKey); }
    }
    number(out, static_cast<uint32_t>(result.failures.size()));
    for (const auto& f : result.failures) { field(out, f.path); field(out, f.message); }
    if (out.size() > maxWireBytes) throw std::runtime_error("Scanner result exceeds size limit");
    return out;
}
bool decodeScan(const std::string& source, ScanResult& result) {
    if (source.size() > maxWireBytes) return false;
    const auto start = source.rfind(magic);
    if (start == std::string::npos) return false;
    ScanResult decoded;
    try {
        Reader reader{source, start + std::strlen(magic)};
        auto count = reader.number(); if (count > 4096) return false;
        for (uint32_t i = 0; i < count; ++i) {
            Descriptor p;
            p.path = reader.field(); p.id = reader.field(); p.name = reader.field(); p.vendor = reader.field();
            p.version = reader.field(); p.description = reader.field();
            auto features = reader.number(); if (features > 128) return false;
            for (uint32_t j = 0; j < features; ++j) p.features.push_back(reader.field());
            const auto discovery = reader.number(), presets = reader.number();
            if (discovery > 1 || presets > maxPresets) return false;
            p.presetDiscovery = discovery;
            for (uint32_t j = 0; j < presets; ++j) {
                Preset preset; preset.name = reader.field(); preset.kind = reader.number(); preset.location = reader.field(); preset.loadKey = reader.field();
                if (preset.kind > CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN || preset.name.empty() || preset.name.size() > maxPresetNameBytes ||
                    preset.location.size() > maxPresetLocationBytes || preset.loadKey.size() > maxPresetLocationBytes) return false;
                p.presets.push_back(std::move(preset));
            }
            if (p.path.empty() || p.id.empty() || p.name.empty()) return false;
            decoded.plugins.push_back(std::move(p));
        }
        count = reader.number(); if (count > 8192) return false;
        for (uint32_t i = 0; i < count; ++i) decoded.failures.push_back({reader.field(), reader.field()});
        if (reader.position != source.size()) return false;
        result = std::move(decoded); return true;
    } catch (...) { return false; }
}
} // namespace kick::clap

namespace {
bool report(char* message, uint32_t capacity, const std::string& text) { if (message && capacity) std::snprintf(message, capacity, "%s", text.c_str()); return false; }
}
// An AssetResolver as clap_host.h's table: Host.cpp's oscairn.assets calls past their thread and argument checks.
oscairn_clap_assets oscairn_clap_assets_of(kick::clap::AssetResolver* assets)
{
    using R = kick::clap::AssetResolver;
    return {assets,
        [](void* c, oscairn_asset_ref_t* ref, char* path, uint32_t capacity, char* message, uint32_t size) {
            const std::string library(ref->library, strnlen(ref->library, sizeof(ref->library))), relative(ref->path, strnlen(ref->path, sizeof(ref->path)));
            std::string hash(ref->sha256, strnlen(ref->sha256, sizeof(ref->sha256)));
            try {
                const auto file = static_cast<R*>(c)->resolve(library, relative, hash);
                if (file.size() + 1 > capacity || hash.size() >= sizeof(ref->sha256)) return report(message, size, "asset path too long");
                std::memcpy(path, file.c_str(), file.size() + 1);
                std::memcpy(ref->sha256, hash.c_str(), hash.size() + 1);
                return true;
            } catch (const std::exception& e) { return report(message, size, e.what()); }
        },
        [](void* c, const char* library, const char* minVersion, char* path, uint32_t capacity) {
            try {
                const auto root = static_cast<R*>(c)->libraryRoot(std::string(library, strnlen(library, 256)), minVersion ? std::string(minVersion, strnlen(minVersion, 128)) : std::string());
                if (root.empty() || root.size() + 1 > capacity) return false;
                std::memcpy(path, root.c_str(), root.size() + 1);
                return true;
            } catch (...) { return false; }
        },
        [](void* c, const char* file, oscairn_asset_ref_t* ref, char* message, uint32_t size) {
            try {
                const auto made = static_cast<R*>(c)->create(std::string(file, strnlen(file, 4096)));
                if (made.library.size() >= sizeof(ref->library) || made.path.size() >= sizeof(ref->path) || made.sha256.size() >= sizeof(ref->sha256)) return report(message, size, "asset reference too long");
                *ref = {};
                std::memcpy(ref->library, made.library.c_str(), made.library.size() + 1);
                std::memcpy(ref->path, made.path.c_str(), made.path.size() + 1);
                std::memcpy(ref->sha256, made.sha256.c_str(), made.sha256.size() + 1);
                return true;
            } catch (const std::exception& e) { return report(message, size, e.what()); }
        },
        [](void* c, const oscairn_asset_ref_t* ref, char* path, uint32_t capacity, oscairn_sample_location_t* location, char* message, uint32_t size) {
            const std::string name(ref->path, strnlen(ref->path, sizeof(ref->path))), digest(ref->sha256, strnlen(ref->sha256, sizeof(ref->sha256)));
            if (name.rfind("sample:", 0) != 0 || (!digest.empty() && digest != name.substr(7))) return report(message, size, "not a sample reference: " + name);
            try {
                const auto at = static_cast<R*>(c)->resolveSample(name.substr(7));
                if (at.pack.size() + 1 > capacity) return report(message, size, "sample path too long");
                std::memcpy(path, at.pack.c_str(), at.pack.size() + 1);
                *location = {at.offset, at.length, at.preload};
                return true;
            } catch (const std::exception& e) { return report(message, size, e.what()); }
        }};
}
