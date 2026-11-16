// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What the undo list says over the protocol (docs/control-protocol.md, Undo): a step for each edit,
// there when the edit's request is answered, a change that needs no commit one step.
//   oscairn-edit-steps-test BASIC_SYNTH.clap
#include "oscairn/json_rpc.h"
#include "oscairn/session.h"
#include <array>
#include <iostream>
#include <string>

using namespace oscairn;

namespace
{
    constexpr uint32_t frames = 128;
    void check(bool ok, const std::string& why) { if(!ok) throw std::runtime_error(why); }
    struct Fixture
    {
        Session session{48000, frames};
        JsonRpc rpc{session};
        std::array<float, frames> left{}, right{};
        explicit Fixture(const std::string& synth)
        {
            const auto n = session.add("synth", synth);
            const auto part = session.addPart("Lead");
            session.insertIntoPart(part, 0, n); session.commit();
        }
        std::string call(const std::string& method, const std::string& params = "{}")
        {
            const auto answer = rpc.dispatch("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"" + method + "\",\"params\":" + params + '}').value_or("");
            check(answer.find("\"error\"") == std::string::npos, method + ": " + answer);
            return answer;
        }
        void render() { float* out[]{left.data(), right.data()}; session.service(); check(session.process(nullptr, 0, out, 2, frames), "render"); }
        // The labels edit.history answers now, with no tick between the edit and the question.
        std::string undo() { return field(call("edit.history"), "undo"); }
        static std::string field(const std::string& text, const std::string& name)
        {
            const auto at = text.find('"' + name + "\":\"");
            if(at == std::string::npos) return {};
            const auto from = at + name.size() + 4;
            return text.substr(from, text.find('"', from) - from);
        }
        static long number(const std::string& text, const std::string& name)
        {
            const auto at = text.find('"' + name + "\":");
            check(at != std::string::npos, name + " in " + text);
            return std::stol(text.substr(at + name.size() + 3));
        }
    };

    // Edits that need no commit are steps by the time they are answered: scenes, controls, a live level.
    void historyIsCurrent(const std::string& synth)
    {
        Fixture f(synth); f.render();
        const auto scene = Fixture::number(f.call("scenes.add", R"({"name":"Verse"})"), "scene");
        f.call("scenes.name", "{\"scene\":" + std::to_string(scene) + R"(,"name":"Intro"})");
        check(f.undo().find("Intro") != std::string::npos, "a scene renamed shows in the history at once: " + f.undo());
        const auto control = Fixture::number(f.call("controls.add", R"({"name":"Macro"})"), "control");
        f.call("controls.name", "{\"control\":" + std::to_string(control) + R"(,"name":"Sweep"})");
        check(f.undo().find("Sweep") != std::string::npos, "a control renamed shows in the history at once: " + f.undo());
        const auto part = f.session.parts().front().id;
        f.call("parts.level", "{\"part\":" + std::to_string(part) + R"(,"level":0.25,"mute":false})");
        check(f.undo().find("level") != std::string::npos, "a part's level shows in the history at once: " + f.undo());
    }
}

int main(int argc, char** argv)
{
    if(argc != 2) return 2;
    try
    {
        historyIsCurrent(argv[1]);
    }
    catch(const std::exception& e) { std::cerr << "FAIL: " << e.what() << '\n'; return 1; }
    std::cout << "ok: edit steps over the protocol\n";
    return 0;
}
