// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// Hardware services (docs/rfcs/0004-hardware-services.md): what a device's accelerators offer, as
// data from its profile, and the host service that mediates plugins' access to them through
// oscairn.hardware (oscairn-clap/hardware.h). No board drivers here: each accelerator's service
// is a Driver, compiled in only for products whose profile declares that accelerator.
#include <kick_clap/Host.h>
#include <atomic>
#include <cstdint>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace oscairn::hardware {

// Where a record's values come from: the maker's documentation, or a measurement on a unit. A value
// a record leaves out is unknown.
enum class Evidence { documented, measured };

// One accelerator, as the profile's hardware.accelerators lists it: what it actually offers, not a
// chip label.
struct Accelerator {
    std::string id;     // unique in the profile ("dsp0")
    std::string kind;   // dsp, fpga, sound-chip, realtime-core or simulated
    std::string abi;    // the service ABI its backends are written for, "<family>/<version>"
    std::string device; // the chip or board and its revision, as identified on the unit; informative
    struct Firmware { std::string id, version; } firmware; // the installed firmware it needs; id empty: none
    std::vector<std::string> operations; // "<family>.<name>/<version>", each offered by its service ABI
    std::vector<std::string> banks;      // resident banks (ROM waveforms, presets), by identity
    std::vector<std::string> regions;    // exclusive regions: an FPGA's partial regions, a chip's parts held whole
    std::optional<uint32_t> voices, parts;
    std::optional<uint64_t> memory;      // bytes of its own memory
    std::string clock;                   // the clock domain its work is timed in (an id in clocks)
    uint32_t pipelineFrames = 0;         // results this many frames after their input; 0: the same block
    bool offline = false;                // it can render faster than real time
    Evidence evidence = Evidence::documented;
};

// A clock other than the audio clock: an accelerator's, or an I/O device's timer.
struct ClockDomain {
    std::string id;
    double rate = 0;                     // nominal ticks per second
    std::optional<double> driftPpm;      // the largest drift from nominal it is specified or measured to have
    std::optional<uint32_t> leadMicros;  // how far ahead of its deadline an event must reach it
    std::optional<uint32_t> queue;       // how many events it can hold queued
    bool cancel = false;                 // queued events can be canceled
    Evidence evidence = Evidence::documented;
};

struct Capabilities {
    std::vector<Accelerator> accelerators;
    std::vector<ClockDomain> clocks;
};

// Every device's: no accelerators and no clock but the audio clock.
inline Capabilities cpuOnly() { return {}; }
// A profile's hardware section (cpuOnly() without one). Throws std::invalid_argument with the reason.
Capabilities fromProfile(std::string_view profileJson);

// An accelerator's service: the board adapter beneath Service, for one service ABI. Constructed by
// the product's build (each under its own option); the simulated one is for tests only.
struct Driver {
    virtual ~Driver() = default;
    virtual std::string abi() const = 0;
    // Main thread, at start: empty when the accelerator is usable (its firmware loaded, its
    // peripheral reachable), else why not.
    virtual std::string open(const Accelerator&) = 0;
    // Audio thread: runs one job of an operation the record lists, within its deadline. Input and
    // output are the service's buffers, already validated against the lease's sizes. Returns an
    // oscairn_hardware_status: OK with outputBytes set, LATE or FAILED. Bounded; never allocates.
    virtual int32_t run(const std::string& operation, int64_t time, uint32_t frames, const void* input,
                        uint32_t inputBytes, void* output, uint32_t& outputBytes) noexcept = 0;
};

// The host side of oscairn.hardware: discovery, capability query, reservations counted across every
// plugin, jobs, and failures. Main-thread calls lock; audio-thread calls use only atomics and the
// buffers reserve() allocated.
class Service final : public kick::clap::HardwareAccess {
public:
    // cpuFallbackMicros: the CPU time per block that leases may reserve for their prepared CPU
    // fallbacks, together. shortcut: a fixed figure, until RFC 0025's budget gives it (step 4).
    Service(Capabilities, std::vector<std::unique_ptr<Driver>>, uint32_t cpuFallbackMicros = 0);
    ~Service() override;

    const Capabilities& capabilities() const noexcept { return capabilities_; }
    // Why accelerator index cannot be used; empty when it can.
    std::string unavailable(uint32_t index) const;
    struct Failure { std::string accelerator; uint32_t lease = 0; int64_t time = 0; bool cpuFallback = false; };
    std::vector<Failure> failures() const;
    // The capabilities, availability, reservations and failures as JSON (the control API's
    // hardware.capabilities).
    std::string json() const;

    static constexpr uint32_t maxLeases = 64;

    uint32_t count() const noexcept override;
    bool get(uint32_t index, oscairn_hardware_accelerator_t&) const noexcept override;
    bool supports(uint32_t index, const char* operation) const noexcept override;
    int32_t reserve(const void* owner, uint32_t index, const oscairn_hardware_request_t&, oscairn_hardware_lease_t&) noexcept override;
    void release(const void* owner, uint32_t lease) noexcept override;
    void releaseAll(const void* owner) noexcept override;
    int32_t submit(const void* owner, uint32_t epoch, uint32_t lease, const oscairn_hardware_job_t&) noexcept override;
    int32_t collect(const void* owner, uint32_t epoch, uint32_t lease, oscairn_hardware_result_t&) noexcept override;

private:
    struct Device;
    struct Lease;
    void releaseSlot(Lease&);
    Lease* find(const void* owner, uint32_t id) noexcept;
    Capabilities capabilities_;
    std::vector<std::unique_ptr<Driver>> drivers_;
    std::vector<std::unique_ptr<Device>> devices_;
    std::unique_ptr<Lease[]> leases_;
    uint32_t cpuFallbackMicros_ = 0, cpuFallbackReserved_ = 0;
    std::vector<Failure> history_; // released leases' failures
    mutable std::mutex mutex_;     // main-thread state: reservations, history
};

// The process's service: CPU-only (no accelerators) until set. set() also offers it to plugins
// (kick::clap::Host::setHardware); once, before any plugin loads.
Service& current();
void setCurrent(Service*);

} // namespace oscairn::hardware
