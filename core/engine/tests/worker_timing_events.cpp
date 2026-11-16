// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0045: the beat and a harmony every block, to a plugin that never asked for them, cost a worker 2 events a block
// each (375 a second, 22 workers on the unit) that its host parses and drops. The engine's side of a worker does not
// send them to a plugin whose host says it did not ask (PluginHost::wantsEvents), and goes on sending them to one that
// did. Told apart by invalid ones: a plugin's host counts the events it drops as malformed before it knows whether the
// plugin asked, so an invalid beat or harmony that reaches a host shows in its dropped events.
//   oscairn-worker-timing-events-test WORKER NORMAL.clap FOLLOWER.clap
#include <oscairn/plugin_host.h>
#include <oscairn-clap/beat.h>
#include <oscairn-clap/harmony.h>
#include <cmath>
#include <cstdio>
#include <stdexcept>
#include <string>
#include <vector>

static void check(bool ok, const std::string& why) { if(!ok) throw std::runtime_error(why); }

namespace
{
    constexpr uint32_t frames = 128, blocks = 20;
    // Blocks of silence into the plugin with an invalid beat and an invalid harmony in each; what its host dropped.
    uint64_t dropped(oscairn::PluginHost& host)
    {
        oscairn_event_beat_t beat{};
        beat.header = {sizeof beat, 0, kick::clap::Host::beatSpace, OSCAIRN_EVENT_BEAT, 0};
        beat.beats = 0; beat.tempo = std::nan(""); // not a tempo
        oscairn_event_harmony_t harmony{};
        harmony.header = {sizeof harmony, 0, kick::clap::Host::harmonySpace, OSCAIRN_EVENT_HARMONY, 0};
        harmony.harmony.key_root = 99; // not a pitch class
        const kick::clap::Event events[]{kick::clap::Event::from(beat), kick::clap::Event::from(harmony)};
        uint32_t in = 0, out = 0;
        for(const auto& p : host.inputPorts()) in += p.channels;
        for(const auto& p : host.outputPorts()) out += p.channels;
        std::vector<std::vector<float>> input(in, std::vector<float>(frames)), output(out, std::vector<float>(frames));
        std::vector<const float*> inputs; std::vector<float*> outputs;
        for(auto& c : input) inputs.push_back(c.data());
        for(auto& c : output) outputs.push_back(c.data());
        for(uint32_t b = 0; b < blocks; ++b) check(host.process(inputs.data(), in, outputs.data(), out, frames, events, 2, nullptr), "rendered");
        return host.statistics().droppedEvents;
    }
}

int main(int argc, char** argv)
try
{
    if(argc != 4) return 2;
    oscairn::PlacementPolicy policy; policy.worker = argv[1]; oscairn::setPlacementPolicy(policy);
    auto plain = oscairn::loadPlugin(argv[2], "org.kick.test.effect", 48000, frames);
    auto follower = oscairn::loadPlugin(argv[3], "org.oscairn.test.follower", 48000, frames);
    check(plain->inWorker() && follower->inWorker(), "both in workers");
    check(plain->wantsEvents() == 0, "the effect asked for neither: " + std::to_string(plain->wantsEvents()));
    check(follower->wantsEvents() == 3, "the follower asked for both: " + std::to_string(follower->wantsEvents()));
    const auto none = dropped(*plain), some = dropped(*follower);
    std::printf("dropped by their hosts in %u blocks of an invalid beat and harmony: %llu for the effect, %llu for the follower\n", blocks,
                (unsigned long long)none, (unsigned long long)some);
    check(none == 0, "nothing sent to a plugin that asked for neither: " + std::to_string(none));
    check(some == 2 * blocks, "both sent to a plugin that asked: " + std::to_string(some));
    std::puts("ok: the beat and the harmony go to the plugins that asked");
    return 0;
}
catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
