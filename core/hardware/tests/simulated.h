// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// The simulated accelerator (RFC 0004 step 1): a Driver for the service ABI oscairn.sim/1, for
// tests only. It is built only with the tests and linked into nothing installed.
// Its firmware is "oscairn.sim"; a record naming another is reported unavailable, as a real
// accelerator without its firmware is.
#include "sim_abi.h"
#include <oscairn/hardware.h>
#include <atomic>
#include <climits>

namespace oscairn::hardware::test {

class SimulatedDriver final : public Driver {
public:
    std::string abi() const override { return simAbi; }
    std::string open(const Accelerator&) override;
    int32_t run(const std::string& operation, int64_t time, uint32_t frames, const void* input, uint32_t inputBytes,
                void* output, uint32_t& outputBytes) noexcept override;
    // Jobs at or after failFrom fail, and the accelerator with them; jobs in [lateFrom, lateUntil) miss their deadline.
    std::atomic<int64_t> failFrom{INT64_MAX}, lateFrom{INT64_MAX}, lateUntil{INT64_MAX};
    std::atomic<uint64_t> jobs{0}; // run, whatever the outcome
};

} // namespace oscairn::hardware::test
