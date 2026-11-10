// SPDX-License-Identifier: AGPL-3.0-or-later
// A plugin with a CPU path and a backend for the simulated accelerator (oscairn.hardware, RFC 0004):
// a 440 Hz sine at a quarter of full scale on both channels of its stereo output. It reserves the
// accelerator when the host offers one that speaks oscairn.sim/1 and renders there; otherwise, when
// rendering offline, when a job is late, and after the accelerator fails with a CPU fallback
// reserved, it renders on its CPU path. After a failure without one, it is silent until it is
// activated again.
#include "sim_abi.h"
#include <clap/clap.h>
#include <oscairn-clap/hardware.h>
#include <cmath>
#include <cstring>

namespace {
using oscairn::hardware::test::SineJob;
constexpr double twoPi = 6.283185307179586, frequency = 440;
constexpr float gain = 0.25f;
const char* features[]{CLAP_PLUGIN_FEATURE_INSTRUMENT, nullptr};
const clap_plugin_descriptor_t descriptor{CLAP_VERSION, "org.oscairn.test.hardware", "Hardware backend fixture", "Oscairn", "", "", "", "1", "", features};

struct Plugin {
    clap_plugin_t api{};
    const clap_host_t* host;
    const oscairn_host_hardware_t* hardware = nullptr;
    oscairn_hardware_lease_t lease{};
    bool offline = false, lost = false, muted = false;
    double phase = 0, increment = 0;

    // The CPU path: the same sine the accelerator's operation renders.
    void cpu(float* out, uint32_t frames) const {
        for (uint32_t i = 0; i < frames; ++i) out[i] = float(gain * std::sin(phase + double(i) * increment));
    }
    void reserve(uint32_t maxFrames) {
        if (!hardware || offline) return; // offline: the CPU reference path (RFC 0004)
        for (uint32_t i = 0; i < hardware->count(host); ++i) {
            oscairn_hardware_accelerator_t a{};
            if (!hardware->get(host, i, &a) || !a.available || std::strcmp(a.abi, oscairn::hardware::test::simAbi) ||
                !hardware->supports(host, i, oscairn::hardware::test::sineOperation))
                continue;
            oscairn_hardware_request_t r{};
            r.operation = oscairn::hardware::test::sineOperation;
            r.region = "osc";
            r.voices = 1;
            r.memory_bytes = 4096;
            r.max_input_bytes = sizeof(SineJob);
            r.max_output_bytes = maxFrames * uint32_t(sizeof(float));
            r.cpu_fallback_us = 20;
            if (hardware->reserve(host, i, &r, &lease) == OSCAIRN_HARDWARE_OK) return;
            lease = {};
        }
    }
    void process(float* out, uint32_t frames, int64_t time) {
        if (muted) { std::memset(out, 0, frames * sizeof(float)); return; }
        if (lease.id && !lost) {
            const SineJob job{phase, increment, gain, 0};
            const oscairn_hardware_job_t j{time, frames, &job, sizeof job};
            oscairn_hardware_result_t result{0, out, frames * uint32_t(sizeof(float))};
            int32_t status = hardware->submit(host, lease.id, &j);
            if (status == OSCAIRN_HARDWARE_OK) status = hardware->collect(host, lease.id, &result);
            if (status == OSCAIRN_HARDWARE_OK && result.output_bytes == frames * sizeof(float)) return;
            if (status == OSCAIRN_HARDWARE_FAILED) {
                lost = true;
                if (!lease.cpu_fallback) { muted = true; std::memset(out, 0, frames * sizeof(float)); return; }
            }
            // Late, failed with a fallback, or refused: this block on the CPU path.
        }
        cpu(out, frames);
    }

    explicit Plugin(const clap_host_t* h) : host(h) {
        api.desc = &descriptor;
        api.plugin_data = this;
        api.init = [](const clap_plugin_t* p) {
            auto& s = *static_cast<Plugin*>(p->plugin_data);
            s.hardware = static_cast<const oscairn_host_hardware_t*>(s.host->get_extension(s.host, OSCAIRN_EXT_HARDWARE));
            return true;
        };
        api.destroy = [](const clap_plugin_t* p) { delete static_cast<Plugin*>(p->plugin_data); };
        api.activate = [](const clap_plugin_t* p, double rate, uint32_t, uint32_t maxFrames) {
            auto& s = *static_cast<Plugin*>(p->plugin_data);
            s.phase = 0; s.increment = twoPi * frequency / rate; s.lost = s.muted = false;
            s.reserve(maxFrames);
            return true;
        };
        api.deactivate = [](const clap_plugin_t* p) {
            auto& s = *static_cast<Plugin*>(p->plugin_data);
            if (s.lease.id) s.hardware->release(s.host, s.lease.id);
            s.lease = {};
        };
        api.start_processing = [](const clap_plugin_t*) { return true; };
        api.stop_processing = [](const clap_plugin_t*) {};
        api.on_main_thread = [](const clap_plugin_t*) {};
        api.reset = [](const clap_plugin_t*) {};
        api.process = [](const clap_plugin_t* p, const clap_process_t* b) -> clap_process_status {
            auto& s = *static_cast<Plugin*>(p->plugin_data);
            float* left = b->audio_outputs[0].data32[0];
            s.process(left, b->frames_count, b->steady_time);
            std::memcpy(b->audio_outputs[0].data32[1], left, b->frames_count * sizeof(float));
            s.phase = std::fmod(s.phase + double(b->frames_count) * s.increment, twoPi);
            return CLAP_PROCESS_CONTINUE;
        };
        api.get_extension = [](const clap_plugin_t*, const char* id) -> const void* {
            static const clap_plugin_audio_ports_t audio{
                [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 0 : 1; },
                [](const clap_plugin_t*, uint32_t i, bool input, clap_audio_port_info_t* p) {
                    if (input || i) return false;
                    *p = {}; p->id = 0; p->channel_count = 2; p->flags = CLAP_AUDIO_PORT_IS_MAIN; p->port_type = CLAP_PORT_STEREO;
                    p->in_place_pair = CLAP_INVALID_ID; std::strcpy(p->name, "Out");
                    return true;
                }};
            static const clap_plugin_render_t render{
                [](const clap_plugin_t*) { return false; },
                [](const clap_plugin_t* p, clap_plugin_render_mode mode) {
                    static_cast<Plugin*>(p->plugin_data)->offline = mode == CLAP_RENDER_OFFLINE;
                    return true;
                }};
            if (!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &audio;
            if (!std::strcmp(id, CLAP_EXT_RENDER)) return &render;
            return nullptr;
        };
    }
};
const clap_plugin_factory_t factory{
    [](const clap_plugin_factory_t*) -> uint32_t { return 1; },
    [](const clap_plugin_factory_t*, uint32_t i) { return i ? nullptr : &descriptor; },
    [](const clap_plugin_factory_t*, const clap_host_t* host, const char* id) -> const clap_plugin_t* {
        return std::strcmp(id, descriptor.id) ? nullptr : &(new Plugin(host))->api;
    }};
} // namespace
extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION, [](const char*) { return true; }, []() {},
    [](const char* id) -> const void* { return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? nullptr : &factory; }};
