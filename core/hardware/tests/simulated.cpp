// SPDX-License-Identifier: AGPL-3.0-or-later
#include "simulated.h"
#include <cmath>
#include <cstring>

namespace oscairn::hardware::test {

std::string SimulatedDriver::open(const Accelerator& a) {
    return a.firmware.id == "oscairn.sim" ? std::string() : "firmware oscairn.sim is not installed";
}

int32_t SimulatedDriver::run(const std::string& operation, int64_t time, uint32_t frames, const void* input, uint32_t inputBytes,
                             void* output, uint32_t& outputBytes) noexcept {
    jobs.fetch_add(1);
    if (time >= failFrom.load()) return OSCAIRN_HARDWARE_FAILED;
    if (time >= lateFrom.load() && time < lateUntil.load()) return OSCAIRN_HARDWARE_LATE;
    if (operation != sineOperation || inputBytes != sizeof(SineJob) || outputBytes < frames * sizeof(float)) return OSCAIRN_HARDWARE_INVALID;
    SineJob job;
    std::memcpy(&job, input, sizeof job);
    auto* out = static_cast<float*>(output);
    for (uint32_t i = 0; i < frames; ++i) out[i] = float(job.gain * std::sin(job.phase + double(i) * job.increment));
    outputBytes = frames * sizeof(float);
    return OSCAIRN_HARDWARE_OK;
}

} // namespace oscairn::hardware::test
