// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Plays an installed plugin offline, as a user would after installing it: a Session holding the
// plugin, its presets listed through presets.list, the first whose name holds PRESET prepared and
// selected as a sound, and a held note rendered for SECONDS. Prints one JSON line (the preset
// count, those in installed asset libraries, the plugins in workers, the preset chosen, each
// channel's RMS and the peak) and fails when the output is silent or
// not finite. Never touches an audio device. OSCAIRN_DATA_HOME and OSCAIRN_PLUGIN_WORKER apply as
// they do in the engine. For tools/live-repository-check.py; not part of ctest.
//   oscairn-installed-play PLUGIN.clap PLUGIN-ID PRESET [SECONDS]
#include "oscairn/json_rpc.h"
#include "oscairn/session.h"
#include <array>
#include <chrono>
#include <cmath>
#include <iostream>
#include <regex>
#include <thread>
using namespace oscairn;

int main(int argc, char** argv)
{
    if(argc != 4 && argc != 5) { std::cerr << "usage: oscairn-installed-play PLUGIN.clap PLUGIN-ID PRESET [SECONDS]\n"; return 2; }
    const std::string want = argv[3];
    const double seconds = argc == 5 ? std::stod(argv[4]) : 3;
    try
    {
        constexpr uint32_t frames = 512;
        Session session{48000, frames}; JsonRpc rpc{session};
        const auto call = [&](const std::string& method, const std::string& params)
        { return rpc.dispatch("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"" + method + "\",\"params\":" + params + '}').value_or(""); };
        const auto n = session.add("synth", argv[1], argv[2]);
        session.connectNotes(graphInput, n); session.connectAudio(n, graphOutput); session.commit();

        // In a worker the scan helper lists them off this thread: asked again until they are in.
        auto listed = call("presets.list", R"({"node":"synth"})");
        for(int tries = 0; listed.find("ask again") != std::string::npos && tries < 3000; ++tries)
        { std::this_thread::sleep_for(std::chrono::milliseconds(20)); listed = call("presets.list", R"({"node":"synth"})"); }
        const std::regex entry(R"re(\{"uri":"((?:[^"\\]|\\.)*)","name":"((?:[^"\\]|\\.)*)"\})re");
        size_t count = 0, fromLibraries = 0; std::string uri, name;
        for(auto i = std::sregex_iterator(listed.begin(), listed.end(), entry); i != std::sregex_iterator(); ++i, ++count)
        {
            fromLibraries += (*i)[1].str().find("/AssetLibraries/") != std::string::npos;
            if(uri.empty() && (*i)[2].str().find(want) != std::string::npos) { uri = (*i)[1]; name = (*i)[2]; }
        }
        if(uri.empty()) throw std::runtime_error("no preset named like \"" + want + "\" among " + std::to_string(count) + ": " + listed.substr(0, 300));

        const auto prepared = call("sounds.prepare", R"({"node":"synth","name":"Played","preset":")" + uri + "\"}");
        const std::smatch context = [&] { std::smatch m; std::regex_search(prepared, m, std::regex(R"("context":(\d+))")); return m; }();
        if(context.empty()) throw std::runtime_error("the preset is not prepared: " + prepared);
        const auto selected = call("sounds.select", R"({"node":"synth","context":)" + context[1].str() + '}');
        if(selected.find("\"accepted\":true") == std::string::npos) throw std::runtime_error("the sound is not selected: " + selected);

        std::array<float, frames> left{}, right{};
        float* out[]{left.data(), right.data()};
        double sum[2]{}, peak = 0; bool finite = true; uint64_t total = 0;
        session.service(); session.process(out, frames); session.service(); // the selection reaches the audio side first
        session.note(true, 60, .8);
        for(uint64_t done = 0; done < uint64_t(seconds * 48000); done += frames)
        {
            session.service();
            if(!session.process(out, frames)) throw std::runtime_error("the block was not rendered");
            for(uint32_t i = 0; i < frames; ++i)
                for(int c = 0; c < 2; ++c)
                {
                    const double v = (c ? right : left)[i];
                    finite = finite && std::isfinite(v); sum[c] += v * v; peak = std::max(peak, std::abs(v));
                }
            total += frames;
        }
        session.note(false, 60, 0); session.service();
        const auto status = call("session.status", "{}");
        size_t workers = 0;
        for(auto at = status.find("\"blockMax\":"); at != std::string::npos; at = status.find("\"blockMax\":", at + 1)) ++workers;
        const double rmsLeft = std::sqrt(sum[0] / double(total)), rmsRight = std::sqrt(sum[1] / double(total));
        std::cout << "{\"presets\":" << count << ",\"fromLibraries\":" << fromLibraries << ",\"workers\":" << workers << ",\"preset\":\"" << name << "\",\"uri\":\"" << uri << "\",\"rms\":[" << rmsLeft << ',' << rmsRight
                  << "],\"peak\":" << peak << ",\"finite\":" << (finite ? "true" : "false") << "}\n";
        if(!finite) throw std::runtime_error("the output is not finite");
        if(rmsLeft + rmsRight < 1e-4) throw std::runtime_error("the output is silent");
        return 0;
    }
    catch(const std::exception& e) { std::cerr << "FAIL: " << e.what() << '\n'; return 1; }
}
