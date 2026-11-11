// SPDX-License-Identifier: AGPL-3.0-or-later
// oscairn-size-hostile-worker: the plugin worker, but after every block overwriting the sizes the
// shared block carries (maxFrames, inputChannels, outputChannels) with huge numbers, as a crashed
// or compromised worker might (worker-host-sizes). They are the engine's own: they place its audio
// buffers in the mapping, so the engine must keep the ones it wrote and never read these back.
// The worker keeps serving: it puts the true sizes back just before its plugin's host reads them
// and wrecks them again before answering, so every block after the first reaches the engine with
// the lies in the block.
#include "Worker.h"
#include <cstdint>

namespace oscairn::worker
{
    void hostileSizes(Shared& s, uint32_t frames, uint32_t inputs, uint32_t outputs, bool wreck) noexcept
    {
        if(wreck)
        {
            s.maxFrames = 1u << 30; // 4 GiB a channel: every buffer this places lands past the mapping
            s.inputChannels = s.outputChannels = UINT32_MAX / 2;
        }
        else { s.maxFrames = frames; s.inputChannels = inputs; s.outputChannels = outputs; }
    }
}
