// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0004 step 1: capability records read from profiles, and the hardware service's discovery,
// reservations (capacity, exclusive regions, unknown capacity, unavailable accelerators), owners,
// epochs and CPU fallback accounting.
//   oscairn-hardware-service-test simulated-profile.json PROFILES_DIR
#include "simulated.h"
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <stdexcept>

using namespace oscairn::hardware;

namespace {
int failures = 0;
void expect(bool ok, const std::string& what) {
    std::printf("%s: %s\n", ok ? "ok" : "FAIL", what.c_str());
    if (!ok) ++failures;
}
std::string read(const std::filesystem::path& p) { std::ifstream f(p); std::stringstream s; s << f.rdbuf(); return s.str(); }
std::string refusal(const std::string& hardware) {
    try { fromProfile("{\"hardware\":" + hardware + "}"); return ""; }
    catch (const std::invalid_argument& e) { return e.what(); }
}
std::vector<std::unique_ptr<Driver>> sim() { std::vector<std::unique_ptr<Driver>> d; d.push_back(std::make_unique<test::SimulatedDriver>()); return d; }
oscairn_hardware_request_t request(uint32_t voices, uint64_t memory = 0, const char* region = nullptr) {
    oscairn_hardware_request_t r{};
    r.operation = test::sineOperation; r.voices = voices; r.memory_bytes = memory; r.region = region;
    r.max_input_bytes = sizeof(test::SineJob); r.max_output_bytes = 512 * sizeof(float);
    return r;
}
} // namespace

int main(int argc, char** argv) {
    if (argc != 3) { std::fprintf(stderr, "usage: oscairn-hardware-service-test simulated-profile.json PROFILES_DIR\n"); return 2; }
    const auto caps = fromProfile(read(argv[1]));
    expect(caps.accelerators.size() == 1 && caps.clocks.size() == 1, "the test profile declares one accelerator and one clock");
    const auto& a = caps.accelerators[0];
    expect(a.kind == "simulated" && a.abi == "oscairn.sim/1" && a.firmware.id == "oscairn.sim" && a.operations.size() == 1 &&
           a.voices == 8u && a.parts == 2u && a.memory == 65536u && a.regions == std::vector<std::string>{"osc"} && a.clock == "sim-timer",
           "its record holds operations, firmware, banks, regions, voices, parts, memory and clock");
    expect(caps.clocks[0].rate == 48e6 && caps.clocks[0].driftPpm == 50.0 && caps.clocks[0].leadMicros == 1000u && caps.clocks[0].cancel,
           "its clock domain: rate, drift, lead time, queue and cancellation");
    // Every built-in product profile reads, and none has an accelerator: they are CPU-only.
    int profiles = 0, accelerated = 0;
    for (const auto& p : std::filesystem::directory_iterator(argv[2]))
        if (p.path().extension() == ".json") { ++profiles; accelerated += !fromProfile(read(p.path())).accelerators.empty(); }
    expect(profiles >= 8 && accelerated == 0, "the " + std::to_string(profiles) + " built-in profiles read as CPU-only");

    // What the parser refuses.
    const std::string good = R"({"id":"x","kind":"dsp","abi":"vendor.dsp/2","operations":["vendor.dsp.filter/1"]})";
    expect(refusal("{\"accelerators\":[" + good + "]}").empty(), "a minimal record: id, kind, abi and an operation");
    expect(refusal(R"({"accelerators":[{"id":"x","kind":"dsp","abi":"vendor.dsp/2","operations":["vendor.dsp.filter/1"],"speed":"fast"}]})").find("unknown field speed") != std::string::npos, "an unknown field");
    expect(refusal(R"({"accelerators":[{"id":"x","kind":"dsp","abi":"VendorDSP","operations":["vendor.dsp.filter/1"]}]})").find("<family>/<version>") != std::string::npos, "an abi without a version");
    expect(refusal(R"({"accelerators":[{"id":"x","kind":"dsp","abi":"vendor.dsp/2","operations":["other.filter/1"]}]})").find("vendor.dsp.<name>") != std::string::npos, "an operation of another family");
    expect(refusal(R"({"accelerators":[{"id":"x","kind":"chip","abi":"vendor.dsp/2","operations":["vendor.dsp.filter/1"]}]})").find("kind") != std::string::npos, "a chip label as the kind");
    expect(refusal(R"({"accelerators":[{"id":"x","kind":"dsp","abi":"vendor.dsp/2","operations":[]}]})").find("at least one") != std::string::npos, "no operations");
    expect(refusal(R"({"accelerators":[{"id":"x","kind":"dsp","abi":"vendor.dsp/2","operations":["vendor.dsp.filter/1"],"clock":"t"}]})").find("not in hardware.clocks") != std::string::npos, "a clock not declared");
    expect(refusal("{\"accelerators\":[" + good + "," + good + "]}").find("twice") == std::string::npos &&
           refusal("{\"accelerators\":[" + good + "," + good + "]}").find("two accelerators") != std::string::npos, "two accelerators of one id");
    expect(refusal(R"({"clocks":[{"id":"audio","rate":48000}]})").find("audio") != std::string::npos, "a clock named audio (the audio clock's)");
    expect(fromProfile(R"({"id":"pedal"})").accelerators.empty(), "a profile without hardware is CPU-only");

    // The CPU-only service: nothing to discover, nothing to reserve.
    {
        Service cpu(cpuOnly(), {});
        oscairn_hardware_lease_t lease{};
        expect(cpu.count() == 0 && cpu.reserve(&cpu, 0, request(1), lease) == OSCAIRN_HARDWARE_INVALID, "CPU-only: no accelerators, every reservation refused");
        expect(cpu.json() == R"({"accelerators":[],"clocks":[],"cpuFallback":{"micros":0,"reserved":0},"failures":[]})", "CPU-only's status: " + cpu.json());
        expect(current().count() == 0, "the process's service is CPU-only until one is set");
    }
    // Declared in the profile, but this build has no service for it, or its firmware is missing.
    {
        Service none(caps, {});
        oscairn_hardware_accelerator_t info{};
        oscairn_hardware_lease_t lease{};
        expect(none.get(0, info) && !info.available && none.unavailable(0) == "this build has no service for oscairn.sim/1",
               "a declared accelerator without its service in the build is listed unavailable, with the reason");
        expect(none.reserve(&none, 0, request(1), lease) == OSCAIRN_HARDWARE_UNAVAILABLE, "and refused as unavailable");
        auto other = caps;
        other.accelerators[0].firmware.id = "oscairn.sim.beta";
        Service wrong(other, sim());
        expect(wrong.unavailable(0) == "firmware oscairn.sim is not installed", "one whose firmware is not installed is unavailable: " + wrong.unavailable(0));
    }
    // Reservations: 8 voices, 2 parts, 64 KiB and the exclusive region osc.
    {
        Service s(caps, sim(), 30);
        const int owner1 = 0, owner2 = 0;
        oscairn_hardware_accelerator_t info{};
        expect(s.count() == 1 && s.get(0, info) && info.available && !std::strcmp(info.abi, "oscairn.sim/1") && info.voices == 8 &&
               info.memory_bytes == 65536 && s.supports(0, test::sineOperation) && !s.supports(0, "oscairn.sim.saw/1"),
               "discovery: the accelerator, its capacity and its operations");
        oscairn_hardware_lease_t l1{}, l2{}, l3{}, l4{};
        expect(s.reserve(&owner1, 0, request(5), l1) == OSCAIRN_HARDWARE_OK && l1.id, "5 of 8 voices reserved");
        expect(s.reserve(&owner2, 0, request(4), l2) == OSCAIRN_HARDWARE_OVER_CAPACITY, "4 more voices refused: over capacity");
        expect(s.reserve(&owner2, 0, request(3), l2) == OSCAIRN_HARDWARE_OK, "3 more fit exactly");
        s.release(&owner1, l1.id);
        expect(s.reserve(&owner2, 0, request(4), l3) == OSCAIRN_HARDWARE_OK, "after the first is released, 4 fit");
        s.release(&owner2, l3.id);
        expect(s.reserve(&owner1, 0, request(0, 40000), l1) == OSCAIRN_HARDWARE_OK &&
               s.reserve(&owner2, 0, request(0, 30000), l3) == OSCAIRN_HARDWARE_OVER_CAPACITY, "memory: 40000 then 30000 of 65536 bytes is over capacity");
        s.release(&owner1, l1.id);
        expect(s.reserve(&owner1, 0, request(0, 0, "osc"), l1) == OSCAIRN_HARDWARE_OK &&
               s.reserve(&owner2, 0, request(0, 0, "osc"), l3) == OSCAIRN_HARDWARE_REGION_BUSY, "the exclusive region is held by one lease");
        auto bank = request(0); bank.bank = "saw";
        auto region = request(0, 0, "fx");
        auto op = request(0); op.operation = "oscairn.sim.saw/1";
        expect(s.reserve(&owner2, 0, bank, l3) == OSCAIRN_HARDWARE_UNSUPPORTED && s.reserve(&owner2, 0, region, l3) == OSCAIRN_HARDWARE_UNSUPPORTED &&
               s.reserve(&owner2, 0, op, l3) == OSCAIRN_HARDWARE_UNSUPPORTED, "a bank, region or operation it lacks is unsupported");
        auto parts = request(0); parts.parts = 3;
        expect(s.reserve(&owner2, 0, parts, l3) == OSCAIRN_HARDWARE_OVER_CAPACITY, "3 parts of 2: over capacity");
        // Owners: one plugin cannot release or use another's lease.
        s.release(&owner2, l1.id);
        expect(s.reserve(&owner2, 0, request(0, 0, "osc"), l3) == OSCAIRN_HARDWARE_REGION_BUSY, "another owner's release is ignored");
        const test::SineJob job{0, 0.1, 0.5f, 0};
        const oscairn_hardware_job_t j{0, 64, &job, sizeof job};
        expect(s.submit(&owner2, 1, l1.id, j) == OSCAIRN_HARDWARE_INVALID, "another owner's submit is refused");
        // Jobs: the epoch, one outstanding job, and nothing to collect.
        float out[64];
        oscairn_hardware_result_t r{0, out, sizeof out};
        expect(s.collect(&owner1, 1, l1.id, r) == OSCAIRN_HARDWARE_NOTHING, "nothing to collect before a submit");
        expect(s.submit(&owner1, 1, l1.id, j) == OSCAIRN_HARDWARE_OK && s.submit(&owner1, 1, l1.id, j) == OSCAIRN_HARDWARE_BUSY, "one job per lease at a time");
        expect(s.collect(&owner1, 1, l1.id, r) == OSCAIRN_HARDWARE_OK && r.output_bytes == sizeof out && out[10] == float(0.5 * std::sin(1.0)),
               "the result is collected in the same epoch");
        expect(s.submit(&owner1, 1, l1.id, j) == OSCAIRN_HARDWARE_OK && s.collect(&owner1, 2, l1.id, r) == OSCAIRN_HARDWARE_STALE_EPOCH,
               "a job from before a reactivation collects as stale");
        const oscairn_hardware_job_t big{0, 64, &job, 4096};
        expect(s.submit(&owner1, 1, l1.id, big) == OSCAIRN_HARDWARE_INVALID, "input larger than the reserved transfer buffer is refused");
        // CPU fallback time: 30 us to share.
        auto fallback = request(1); fallback.cpu_fallback_us = 20;
        expect(s.reserve(&owner1, 0, fallback, l2) == OSCAIRN_HARDWARE_OK && l2.cpu_fallback &&
               s.reserve(&owner2, 0, fallback, l4) == OSCAIRN_HARDWARE_OK && !l4.cpu_fallback,
               "20 us of CPU fallback reserved for the first lease; not the second (30 us in all)");
        s.releaseAll(&owner1);
        s.releaseAll(&owner2);
        expect(s.json().find(R"("reserved":{"voices":0,"parts":0,"memory":0,"regions":[]})") != std::string::npos &&
               s.json().find(R"("cpuFallback":{"micros":30,"reserved":0})") != std::string::npos, "releasing every owner's leases frees everything");
    }
    // Capacity a record does not state is not promised.
    {
        auto unknown = caps;
        unknown.accelerators[0].voices.reset();
        Service s(unknown, sim());
        oscairn_hardware_lease_t l{};
        oscairn_hardware_accelerator_t info{};
        expect(s.get(0, info) && info.voices == OSCAIRN_HARDWARE_UNKNOWN && s.reserve(&s, 0, request(1), l) == OSCAIRN_HARDWARE_UNKNOWN_CAPACITY &&
               s.reserve(&s, 0, request(0), l) == OSCAIRN_HARDWARE_OK, "unknown voices: asking for any is refused, asking for none is not");
    }
    std::printf("%s\n", failures ? "FAIL" : "PASS");
    return failures ? 1 : 0;
}
