// SPDX-License-Identifier: AGPL-3.0-or-later
#include <oscairn/hardware.h>
#include <algorithm>
#include <climits>
#include <cstdio>
#include <type_traits>
#include <cstring>
#include <thread>

namespace oscairn::hardware {

struct Service::Device {
    const Accelerator* record = nullptr;
    Driver* driver = nullptr;
    std::string reason;                // why it cannot be used; set at construction
    uint32_t voices = 0, parts = 0;    // reserved (main thread, under the mutex)
    uint64_t memory = 0;
    std::vector<std::string> regions;  // held
    std::atomic<bool> failed{false};   // one failure fails every lease on it: its failure domain
};

struct Service::Lease {
    std::atomic<uint32_t> id{0};       // 0: free
    std::atomic<uint32_t> users{0};    // audio-thread calls inside it
    std::atomic<bool> releasing{false};
    const void* owner = nullptr;
    uint32_t device = 0, generation = 0;
    std::string operation, region;
    uint32_t voices = 0, parts = 0, cpuMicros = 0;
    uint64_t memory = 0;
    bool cpuFallback = false;
    std::vector<uint8_t> input, output;
    std::atomic<int64_t> failedAt{INT64_MIN};
    // The job (audio thread only).
    bool pending = false;
    int32_t status = OSCAIRN_HARDWARE_OK;
    int64_t time = 0;
    uint32_t epoch = 0, outputBytes = 0;
};

namespace {
constexpr uint32_t maxTransferBytes = 16u << 20;
constexpr uint32_t slotSpan = 128; // a lease ID is generation * slotSpan + slot + 1
template<size_t N> void copy(char (&to)[N], const std::string& from) {
    std::memset(to, 0, N);
    std::memcpy(to, from.data(), std::min(from.size(), N - 1));
}
bool listed(const std::vector<std::string>& list, const char* name) {
    return std::find(list.begin(), list.end(), name) != list.end();
}
std::string quote(std::string_view s) {
    std::string r = "\"";
    for (unsigned char c : s) {
        if (c == '"' || c == '\\') { r += '\\'; r += char(c); }
        else if (c < 32) { char b[8]; std::snprintf(b, sizeof b, "\\u%04x", c); r += b; }
        else r += char(c);
    }
    return r + '"';
}
std::string list(const std::vector<std::string>& v) {
    std::string r = "[";
    for (size_t i = 0; i < v.size(); ++i) r += (i ? "," : "") + quote(v[i]);
    return r + "]";
}
template<class T> std::string optional(const std::optional<T>& v) {
    if (!v) return "null";
    if constexpr (std::is_floating_point_v<T>) { char b[32]; std::snprintf(b, sizeof b, "%.17g", *v); return b; }
    else return std::to_string(*v);
}
const char* evidence(Evidence e) { return e == Evidence::measured ? "\"measured\"" : "\"documented\""; }
// An audio-thread call inside a lease, which release waits out. Sequentially consistent with
// release's flag: either the call sees the lease releasing, or release sees the call.
struct Using {
    std::atomic<uint32_t>& users;
    explicit Using(std::atomic<uint32_t>& u) : users(u) { users.fetch_add(1); }
    ~Using() { users.fetch_sub(1); }
};
} // namespace

Service::Service(Capabilities c, std::vector<std::unique_ptr<Driver>> drivers, uint32_t cpuFallbackMicros)
    : capabilities_(std::move(c)), leases_(new Lease[maxLeases]), cpuFallbackMicros_(cpuFallbackMicros) {
    drivers_ = std::move(drivers);
    for (const auto& a : capabilities_.accelerators) {
        auto d = std::make_unique<Device>();
        d->record = &a;
        for (auto& driver : drivers_)
            if (driver->abi() == a.abi) { d->driver = driver.get(); break; }
        if (!d->driver) d->reason = "this build has no service for " + a.abi;
        else d->reason = d->driver->open(a);
        devices_.push_back(std::move(d));
    }
}
Service::~Service() = default;

std::string Service::unavailable(uint32_t index) const {
    if (index >= devices_.size()) return "no such accelerator";
    if (!devices_[index]->reason.empty()) return devices_[index]->reason;
    return devices_[index]->failed.load() ? "failed" : "";
}

uint32_t Service::count() const noexcept { return uint32_t(devices_.size()); }

bool Service::get(uint32_t index, oscairn_hardware_accelerator_t& out) const noexcept {
    if (index >= devices_.size()) return false;
    const auto& a = *devices_[index]->record;
    std::memset(&out, 0, sizeof out);
    copy(out.id, a.id); copy(out.kind, a.kind); copy(out.abi, a.abi);
    copy(out.firmware, a.firmware.id); copy(out.firmware_version, a.firmware.version);
    out.voices = a.voices.value_or(OSCAIRN_HARDWARE_UNKNOWN);
    out.parts = a.parts.value_or(OSCAIRN_HARDWARE_UNKNOWN);
    out.memory_bytes = a.memory.value_or(OSCAIRN_HARDWARE_UNKNOWN_BYTES);
    out.pipeline_frames = a.pipelineFrames;
    out.offline = a.offline;
    out.available = devices_[index]->reason.empty() && !devices_[index]->failed.load();
    return true;
}

bool Service::supports(uint32_t index, const char* operation) const noexcept {
    return index < devices_.size() && operation && listed(devices_[index]->record->operations, operation);
}

int32_t Service::reserve(const void* owner, uint32_t index, const oscairn_hardware_request_t& r, oscairn_hardware_lease_t& out) noexcept {
    if (index >= devices_.size() || !r.operation || r.max_input_bytes > maxTransferBytes || r.max_output_bytes > maxTransferBytes)
        return OSCAIRN_HARDWARE_INVALID;
    std::lock_guard lock(mutex_);
    auto& d = *devices_[index];
    const auto& a = *d.record;
    if (!d.reason.empty() || d.failed.load()) return OSCAIRN_HARDWARE_UNAVAILABLE;
    if (!listed(a.operations, r.operation) || (r.bank && !listed(a.banks, r.bank)) || (r.region && !listed(a.regions, r.region)))
        return OSCAIRN_HARDWARE_UNSUPPORTED;
    if (r.region && listed(d.regions, r.region)) return OSCAIRN_HARDWARE_REGION_BUSY;
    // Capacity a record does not state is not promised: asking for any of it is refused.
    if ((r.voices && !a.voices) || (r.parts && !a.parts) || (r.memory_bytes && !a.memory)) return OSCAIRN_HARDWARE_UNKNOWN_CAPACITY;
    if ((r.voices && uint64_t(d.voices) + r.voices > *a.voices) || (r.parts && uint64_t(d.parts) + r.parts > *a.parts) ||
        (r.memory_bytes && (r.memory_bytes > *a.memory || d.memory > *a.memory - r.memory_bytes)))
        return OSCAIRN_HARDWARE_OVER_CAPACITY;
    Lease* slot = nullptr;
    uint32_t i = 0;
    for (; i < maxLeases; ++i) if (!leases_[i].id.load()) { slot = &leases_[i]; break; }
    if (!slot) return OSCAIRN_HARDWARE_OVER_CAPACITY;
    try {
        slot->operation = r.operation;
        slot->region = r.region ? r.region : "";
        slot->input.assign(r.max_input_bytes, 0);
        slot->output.assign(r.max_output_bytes, 0);
        if (r.region) d.regions.push_back(r.region);
    } catch (...) {
        return OSCAIRN_HARDWARE_OVER_CAPACITY;
    }
    slot->owner = owner; slot->device = index;
    slot->voices = r.voices; slot->parts = r.parts; slot->memory = r.memory_bytes;
    d.voices += r.voices; d.parts += r.parts; d.memory += r.memory_bytes;
    slot->cpuFallback = r.cpu_fallback_us && cpuFallbackReserved_ + uint64_t(r.cpu_fallback_us) <= cpuFallbackMicros_;
    slot->cpuMicros = slot->cpuFallback ? r.cpu_fallback_us : 0;
    cpuFallbackReserved_ += slot->cpuMicros;
    slot->pending = false; slot->failedAt.store(INT64_MIN);
    const uint32_t id = (++slot->generation % (UINT32_MAX / slotSpan - 1)) * slotSpan + i + 1;
    slot->id.store(id, std::memory_order_release);
    out = {id, a.pipelineFrames, slot->cpuFallback};
    return OSCAIRN_HARDWARE_OK;
}

void Service::releaseSlot(Lease& l) {
    l.releasing.store(true);
    while (l.users.load()) std::this_thread::yield(); // an audio-thread call is bounded
    auto& d = *devices_[l.device];
    d.voices -= l.voices; d.parts -= l.parts; d.memory -= l.memory;
    if (!l.region.empty()) d.regions.erase(std::find(d.regions.begin(), d.regions.end(), l.region));
    cpuFallbackReserved_ -= l.cpuMicros;
    if (const auto at = l.failedAt.load(); at != INT64_MIN) history_.push_back({d.record->id, l.id.load(), at, l.cpuFallback});
    l.input.clear(); l.input.shrink_to_fit(); l.output.clear(); l.output.shrink_to_fit();
    l.owner = nullptr;
    l.id.store(0);
    l.releasing.store(false);
}

Service::Lease* Service::find(const void* owner, uint32_t id) noexcept {
    if (!id || (id - 1) % slotSpan >= maxLeases) return nullptr;
    auto& l = leases_[(id - 1) % slotSpan];
    return l.id.load(std::memory_order_acquire) == id && l.owner == owner ? &l : nullptr;
}

void Service::release(const void* owner, uint32_t id) noexcept {
    std::lock_guard lock(mutex_);
    if (auto* l = find(owner, id)) releaseSlot(*l);
}

void Service::releaseAll(const void* owner) noexcept {
    std::lock_guard lock(mutex_);
    for (uint32_t i = 0; i < maxLeases; ++i)
        if (leases_[i].id.load() && leases_[i].owner == owner) releaseSlot(leases_[i]);
}

int32_t Service::submit(const void* owner, uint32_t epoch, uint32_t id, const oscairn_hardware_job_t& job) noexcept {
    if (!id || (id - 1) % slotSpan >= maxLeases) return OSCAIRN_HARDWARE_INVALID;
    auto& l = leases_[(id - 1) % slotSpan];
    Using inside(l.users);
    if (l.releasing.load() || l.id.load(std::memory_order_acquire) != id || l.owner != owner) return OSCAIRN_HARDWARE_INVALID;
    if (job.input_bytes > l.input.size() || (job.input_bytes && !job.input)) return OSCAIRN_HARDWARE_INVALID;
    if (l.pending) return OSCAIRN_HARDWARE_BUSY;
    auto& d = *devices_[l.device];
    if (l.failedAt.load(std::memory_order_relaxed) != INT64_MIN) return OSCAIRN_HARDWARE_FAILED;
    l.pending = true; l.time = job.steady_time; l.epoch = epoch; l.outputBytes = 0;
    if (d.failed.load(std::memory_order_acquire)) l.status = OSCAIRN_HARDWARE_FAILED;
    else {
        if (job.input_bytes) std::memcpy(l.input.data(), job.input, job.input_bytes);
        uint32_t bytes = uint32_t(l.output.size());
        l.status = d.driver->run(l.operation, job.steady_time, job.frames, l.input.data(), job.input_bytes, l.output.data(), bytes);
        l.outputBytes = std::min<uint32_t>(bytes, uint32_t(l.output.size()));
        if (l.status == OSCAIRN_HARDWARE_FAILED) d.failed.store(true, std::memory_order_release);
    }
    if (l.status == OSCAIRN_HARDWARE_FAILED) l.failedAt.store(job.steady_time, std::memory_order_relaxed);
    return OSCAIRN_HARDWARE_OK;
}

int32_t Service::collect(const void* owner, uint32_t epoch, uint32_t id, oscairn_hardware_result_t& result) noexcept {
    if (!id || (id - 1) % slotSpan >= maxLeases) return OSCAIRN_HARDWARE_INVALID;
    auto& l = leases_[(id - 1) % slotSpan];
    Using inside(l.users);
    if (l.releasing.load() || l.id.load(std::memory_order_acquire) != id || l.owner != owner) return OSCAIRN_HARDWARE_INVALID;
    if (!l.pending) return OSCAIRN_HARDWARE_NOTHING;
    l.pending = false;
    result.steady_time = l.time;
    if (l.epoch != epoch) { result.output_bytes = 0; return OSCAIRN_HARDWARE_STALE_EPOCH; }
    if (l.status != OSCAIRN_HARDWARE_OK) { result.output_bytes = 0; return l.status; }
    if (!result.output || result.output_bytes < l.outputBytes) { result.output_bytes = 0; return OSCAIRN_HARDWARE_INVALID; }
    std::memcpy(result.output, l.output.data(), l.outputBytes);
    result.output_bytes = l.outputBytes;
    return OSCAIRN_HARDWARE_OK;
}

std::vector<Service::Failure> Service::failures() const {
    std::lock_guard lock(mutex_);
    auto out = history_;
    for (uint32_t i = 0; i < maxLeases; ++i)
        if (const auto at = leases_[i].failedAt.load(); leases_[i].id.load() && at != INT64_MIN)
            out.push_back({devices_[leases_[i].device]->record->id, leases_[i].id.load(), at, leases_[i].cpuFallback});
    return out;
}

std::string Service::json() const {
    const auto failed = failures();
    std::lock_guard lock(mutex_);
    std::string s = "{\"accelerators\":[";
    for (size_t i = 0; i < devices_.size(); ++i) {
        const auto& d = *devices_[i];
        const auto& a = *d.record;
        const auto reason = !d.reason.empty() ? d.reason : d.failed.load() ? std::string("failed") : std::string();
        s += std::string(i ? "," : "") + "{\"id\":" + quote(a.id) + ",\"kind\":" + quote(a.kind) + ",\"abi\":" + quote(a.abi) +
             ",\"device\":" + quote(a.device) + ",\"firmware\":" + (a.firmware.id.empty() ? std::string("null") : "{\"id\":" + quote(a.firmware.id) + ",\"version\":" + quote(a.firmware.version) + "}") +
             ",\"operations\":" + list(a.operations) + ",\"banks\":" + list(a.banks) + ",\"regions\":" + list(a.regions) +
             ",\"voices\":" + optional(a.voices) + ",\"parts\":" + optional(a.parts) + ",\"memory\":" + optional(a.memory) +
             ",\"clock\":" + (a.clock.empty() ? std::string("null") : quote(a.clock)) + ",\"pipelineFrames\":" + std::to_string(a.pipelineFrames) +
             ",\"offline\":" + (a.offline ? "true" : "false") + ",\"evidence\":" + evidence(a.evidence) +
             ",\"available\":" + (reason.empty() ? "true" : "false") + ",\"reason\":" + (reason.empty() ? std::string("null") : quote(reason)) +
             ",\"reserved\":{\"voices\":" + std::to_string(d.voices) + ",\"parts\":" + std::to_string(d.parts) + ",\"memory\":" + std::to_string(d.memory) +
             ",\"regions\":" + list(d.regions) + "}}";
    }
    s += "],\"clocks\":[";
    for (size_t i = 0; i < capabilities_.clocks.size(); ++i) {
        const auto& c = capabilities_.clocks[i];
        char rate[32]; std::snprintf(rate, sizeof rate, "%.17g", c.rate);
        s += std::string(i ? "," : "") + "{\"id\":" + quote(c.id) + ",\"rate\":" + rate + ",\"driftPpm\":" + optional(c.driftPpm) +
             ",\"leadMicros\":" + optional(c.leadMicros) + ",\"queue\":" + optional(c.queue) + ",\"cancel\":" + (c.cancel ? "true" : "false") +
             ",\"evidence\":" + evidence(c.evidence) + "}";
    }
    s += "],\"cpuFallback\":{\"micros\":" + std::to_string(cpuFallbackMicros_) + ",\"reserved\":" + std::to_string(cpuFallbackReserved_) + "},\"failures\":[";
    for (size_t i = 0; i < failed.size(); ++i)
        s += std::string(i ? "," : "") + "{\"accelerator\":" + quote(failed[i].accelerator) + ",\"time\":" + std::to_string(failed[i].time) +
             ",\"cpuFallback\":" + (failed[i].cpuFallback ? "true" : "false") + "}";
    return s + "]}";
}

namespace {
std::atomic<Service*> currentService{nullptr};
}
Service& current() {
    static Service cpu(cpuOnly(), {});
    auto* s = currentService.load();
    return s ? *s : cpu;
}
void setCurrent(Service* s) {
    currentService.store(s);
    kick::clap::Host::setHardware(s);
}

} // namespace oscairn::hardware
