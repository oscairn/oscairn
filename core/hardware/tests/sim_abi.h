// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// The service ABI oscairn.sim/1, the simulated accelerator's (tests only): what its operations'
// job input and output bytes are, which a plugin's backend for it and its service share.
//
// oscairn.sim.sine/1: input a SineJob; output frames floats, gain * sin(phase + i * increment)
// for frame i.
#include <cstdint>

namespace oscairn::hardware::test {
struct SineJob { double phase = 0, increment = 0; float gain = 0; uint32_t reserved = 0; };
inline constexpr char simAbi[] = "oscairn.sim/1";
inline constexpr char sineOperation[] = "oscairn.sim.sine/1";
}
