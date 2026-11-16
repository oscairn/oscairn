// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What the undo list says over the protocol (docs/control-protocol.md, Undo): a step for each edit,
// there when the edit's request is answered, a change that needs no commit one step.
//   oscairn-edit-steps-test BASIC_SYNTH.clap
#include "oscairn/json_rpc.h"
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <array>
#include <iostream>
#include <string>
#include <cstdlib>

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

    // One params.set of two values is one step: one undo takes both back, one redo puts both back.
    void oneRequestOneStep(const std::string& synth)
    {
        Fixture f(synth); f.render();
        const auto node = f.session.nodeId("synth");
        std::vector<uint32_t> ids;
        for(const auto& p : f.session.parameters(node)) if(!(p.flags & CLAP_PARAM_IS_READONLY) && !p.stepped && ids.size() < 2) ids.push_back(p.id);
        check(ids.size() == 2, "two parameters to set");
        const double was[]{f.session.parameter(node, ids[0]), f.session.parameter(node, ids[1])};
        const auto before = f.undo();
        auto set = [&](double a, double b) {
            f.call("params.set", "{\"values\":[{\"node\":" + std::to_string(node) + ",\"param\":" + std::to_string(ids[0]) + ",\"value\":" + std::to_string(a) +
                                 "},{\"node\":" + std::to_string(node) + ",\"param\":" + std::to_string(ids[1]) + ",\"value\":" + std::to_string(b) + "}]}");
        };
        set(.3, .6);
        check(f.undo() != before, "the request is a step: " + f.undo());
        set(.35, .65); // a drag: the same two again, merged
        f.call("edit.undo");
        check(f.undo() == before, "one undo takes back the step before it: " + f.undo());
        check(f.session.parameter(node, ids[0]) == was[0] && f.session.parameter(node, ids[1]) == was[1], "both values are back");
        f.call("edit.redo");
        check(f.session.parameter(node, ids[0]) == .35 && f.session.parameter(node, ids[1]) == .65, "and one redo puts both back");
    }

    // A part made from a Sound is one step, though it adds the part and its plugins.
    void fromSoundOneStep(const std::string& synth)
    {
        Fixture f(synth); f.render();
        const auto part = f.session.parts().front().id;
        const auto saved = f.call("parts.saveSound", "{\"part\":" + std::to_string(part) + R"(,"name":"Check Lead","new":true})");
        const auto id = Fixture::field(saved, "id");
        check(!id.empty(), "the Sound saved: " + saved);
        const auto before = f.undo();
        f.call("parts.fromSound", "{\"id\":\"" + id + "\"}");
        check(f.session.parts().size() == 2 && f.undo() != before, "a part from the Sound is a step: " + f.undo());
        f.call("edit.undo");
        check(f.session.parts().size() == 1 && f.undo() == before, "one undo takes the whole part back: " + std::to_string(f.session.parts().size()) + " parts, " + f.undo());
        f.call("edit.redo");
        check(f.session.parts().size() == 2, "and one redo puts it back");
    }

    // The selected scene is one that is there: after an undone scenes.add it is not the removed one.
    void sceneAfterUndo(const std::string& synth)
    {
        Fixture f(synth); f.render();
        f.call("scenes.add", R"({"name":"One"})"); f.undo(); f.call("scenes.add", R"({"name":"Two"})"); f.undo(); // each a step of its own
        const auto added = Fixture::number(f.call("scenes.add", R"({"name":"Three"})"), "scene");
        check(f.session.graphState().scene == added, "the scene stored is selected");
        f.call("edit.undo");
        const auto now = f.session.graphState();
        check(now.scenes.size() == 2, "the add is undone");
        const auto selected = now.scene;
        check(selected == 0 || std::any_of(now.scenes.begin(), now.scenes.end(), [&](const Scene& s) { return s.id == uint32_t(selected); }),
              "graph.get names a scene that is there, or none: " + std::to_string(selected));
        check(Fixture::number(f.call("graph.get"), "scene") == selected, "as the protocol reads it");
    }

    // A commit with nothing staged compiles a plan and is a new revision; one that only commits a live level
    // compiles none and leaves the revision (RFC 0011: the move reaches clients as controls.changed).
    void revisions(const std::string& synth)
    {
        Fixture f(synth); f.render();
        const auto first = Fixture::number(f.call("graph.commit"), "revision");
        const auto second = Fixture::number(f.call("graph.commit"), "revision");
        check(second == first + 1, "an unchanged commit is a new revision: " + std::to_string(first) + " then " + std::to_string(second));
        const auto part = f.session.parts().front().id;
        f.call("parts.level", "{\"part\":" + std::to_string(part) + R"(,"level":0.5,"mute":false})");
        const auto live = Fixture::number(f.call("graph.commit"), "revision");
        check(live == second, "a commit of a live level compiles no plan and keeps the revision: " + std::to_string(second) + " then " + std::to_string(live));
    }

    // harmony.set is an edit (RFC 0033: undo covers both keys): undone, the key is back; redone, it is set again.
    void harmonyUndo(const std::string& synth)
    {
        Fixture f(synth); f.render();
        const auto part = f.session.parts().front().id;
        const auto before = f.session.parts().front().key;
        f.call("harmony.set", "{\"part\":" + std::to_string(part) + R"(,"key":{"root":2,"scale":"Minor"}})");
        f.call("graph.commit");
        const auto set = f.session.parts().front().key;
        check(set && set->root == 2 && f.undo() != "Add synth", "the part's key is set: " + f.undo());
        f.call("edit.undo");
        check(f.session.parts().front().key == before, "undone, the part has its key back");
        f.call("edit.redo");
        check(f.session.parts().front().key == set, "and redone");
        const auto whole = f.session.key();
        f.call("harmony.set", R"({"key":{"root":7,"scale":"Major"}})");
        f.call("graph.commit");
        check(f.session.key().root == 7, "the Performance's key is set");
        f.call("edit.undo");
        check(f.session.key() == whole, "undone, the Performance has its key back");
    }
}

int main(int argc, char** argv)
{
    if(argc != 2) return 2;
    state_files::TemporaryDirectory home;
    ::setenv("OSCAIRN_DATA_HOME", home.path.c_str(), 1); // a library of its own
    try
    {
        historyIsCurrent(argv[1]);
        oneRequestOneStep(argv[1]);
        fromSoundOneStep(argv[1]);
        sceneAfterUndo(argv[1]);
        revisions(argv[1]);
        harmonyUndo(argv[1]);
    }
    catch(const std::exception& e) { std::cerr << "FAIL: " << e.what() << '\n'; return 1; }
    std::cout << "ok: edit steps over the protocol\n";
    return 0;
}
