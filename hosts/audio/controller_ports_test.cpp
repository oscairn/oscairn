// SPDX-License-Identifier: AGPL-3.0-or-later
//
// External controllers kept by the engine (RFC 0024): ControllerPorts wired to a session as
// AudioEngine wires it, over the control protocol: the inputs listed with the profiles that fit
// them; a profile, none, each of two and automatic chosen; a port ignored; what is played (the
// ports bound, the groups joined, the inputs left out); a renamed port; refusals; the choices
// kept across a restart; the app's old file taken over once; and an app without `system`
// refused. Inputs are names only: no MIDI device is opened. Every file is in temporary folders
// (OSCAIRN_DATA_HOME, OSCAIRN_CONFIG_HOME).
#include "ControllerPorts.h"
#include "oscairn/json_rpc.h"
#include "oscairn/session.h"
#include <juce_core/juce_core.h>
#include <cstdio>
#include <cstdlib>

using namespace oscairn;

int main()
{
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { std::fprintf(stderr, "%s: %s\n", ok ? "ok" : "FAIL", what.c_str()); if(!ok) ++failures; };
    const auto root = juce::File::createTempFile("oscairn-controller-ports");
    const auto data = root.getChildFile("data"), configuration = root.getChildFile("config");
    data.createDirectory(); configuration.createDirectory();
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    ::setenv("OSCAIRN_CONFIG_HOME", configuration.getFullPathName().toRawUTF8(), 1);

    // A second profile for the SMK37 (custom firmware, its duplicate port not ignored), given as a file.
    std::string stock;
    for(const auto& [id, json] : standalone::builtInProfiles()) if(id == "smk37-elite") stock = json;
    const auto custom = root.getChildFile("smk37-custom.json");
    custom.replaceWithText(juce::String(stock).replace("\"smk37-elite\"", "\"smk37-elite-custom\"").replace("(stock firmware)", "(custom firmware)").replace("\"ignore\": true", "\"ignore\": false"));
    const std::vector<std::string> files{custom.getFullPathName().toStdString()};
    // The choices as the app kept them before the engine did.
    const auto old = configuration.getChildFile("controller-profiles.json");
    old.replaceWithText(R"({"SMK-37 Elite -Master": ["smk37-elite-custom"]})");
    const std::vector<std::string> inputs{"IAC Bus 1", "SMK-37 Elite -Private", "SMK-37 Elite -Master", "SMK-37 Elite -MCP"};

    bool refused = false;
    try { host::ControllerPorts bad({"kick"}); } catch(const std::invalid_argument&) { refused = true; }
    check(refused, "a device's profile is refused as a controller's");

    auto ports = std::make_unique<host::ControllerPorts>(files);
    const auto kept = data.getChildFile("controllers.json");
    check(kept.existsAsFile() && kept.loadFileAsString().contains("smk37-elite-custom") && old.existsAsFile(), "the app's choices taken over into the data folder, its file left as it was");
    ports->setInputs(inputs);

    Session session(48000, 128);
    auto wire = [&] {
        session.setControllerHost({[&] { return ports->list(); },
            [&](const std::string& input, const std::optional<std::vector<std::string>>& profiles) { return ports->choose(input, profiles); },
            [&](const std::string& input, bool ignored) { return ports->ignore(input, ignored); }});
    };
    wire();
    JsonRpc rpc(session);
    int id = 0;
    auto call = [&](JsonRpc& to, const std::string& method, const std::string& params = "{}") {
        const auto reply = to.dispatch(R"({"jsonrpc":"2.0","id":)" + std::to_string(++id) + R"(,"method":")" + method + R"(","params":)" + params + "}");
        return juce::JSON::parse(juce::String(reply.value_or("")));
    };
    auto port = [&](const juce::var& reply, const juce::String& name) {
        for(const auto& p : *reply["result"]["inputs"].getArray()) if(p["name"] == name) return p;
        return juce::var();
    };
    auto ids = [](const juce::var& list) { juce::StringArray out; if(const auto* a = list.getArray()) for(const auto& v : *a) out.add(v.toString()); return out.joinIntoString(","); };

    auto listed = call(rpc, "controllers.inputs");
    check(listed["result"]["inputs"].size() == 4, "every input listed, in the system's order");
    auto iac = port(listed, "IAC Bus 1"), master = port(listed, "SMK-37 Elite -Master"), duplicate = port(listed, "SMK-37 Elite -Private"), mcp = port(listed, "SMK-37 Elite -MCP");
    check(iac["profiles"].size() == 0 && bool(iac["automatic"]) && iac["use"] == "" && !bool(iac["ignored"]), "an input no profile fits: none, automatic");
    check(master["profiles"].size() == 2 && master["profiles"][0]["id"] == "smk37-elite" && master["profiles"][1]["name"] == "SMK37 Elite (custom firmware)" &&
          !bool(master["automatic"]) && ids(master["chosen"]) == "smk37-elite-custom" && ids(master["playing"]) == "smk37-elite-custom" && master["use"] == "controls",
          "-Master: both profiles fit, the custom one taken over from the app's file and played");
    check(bool(duplicate["automatic"]) && ids(duplicate["playing"]) == "smk37-elite" && duplicate["use"] == "duplicate", "-Private: the first by id, its duplicate port");
    check(ids(mcp["playing"]) == "smk37-elite" && mcp["use"] == "mackie", "-MCP: Mackie Control");

    // What the engine plays: -Master bound to the custom profile, both profiles' groups joined (by id), -Private and -MCP left out.
    auto played = ports->applied();
    const auto stockGroups = standalone::controlGroups(standalone::readProfile("smk37-elite")).size();
    check(played.ports.size() == 1 && played.ports[0].input == "SMK-37 Elite -Master" && played.ports[0].device == "smk37-elite-custom" && played.ports[0].port == 1,
          "the bound port: -Master, the custom profile's port 1");
    check(played.groups.size() == 2 * stockGroups && played.ignored == std::vector<std::string>{"SMK-37 Elite -Private", "SMK-37 Elite -MCP"},
          "both profiles' groups joined, the duplicate and the Mackie port left out");
    session.setControllers({ControlGroup{}}, {});
    session.setHostControllers(played.groups, played.ports, played.ignored);
    check(call(rpc, "controllers.get")["result"]["positions"].size() == int(1 + 2 * stockGroups), "the session plays the device's group, then the controllers'");

    // Choosing: none, each of the two, back to automatic.
    auto changes = ports->changes();
    auto chosen = port(call(rpc, "controllers.choose", R"({"input":"SMK-37 Elite -Master","profiles":[]})"), "SMK-37 Elite -Master");
    check(!bool(chosen["automatic"]) && chosen["chosen"].size() == 0 && chosen["playing"].size() == 0 && chosen["use"] == "", "None: no profile plays it");
    check(ports->changes() > changes && ports->applied().ports.empty(), "and nothing is bound; the change counted, for status's midiPortList");
    chosen = port(call(rpc, "controllers.choose", R"({"input":"SMK-37 Elite -Master","profiles":["smk37-elite","smk37-elite-custom"]})"), "SMK-37 Elite -Master");
    check(ids(chosen["playing"]) == "smk37-elite,smk37-elite-custom" && ports->applied().ports.size() == 2, "each of the two, by MIDI channel: both bind it");
    chosen = port(call(rpc, "controllers.choose", R"({"input":"SMK-37 Elite -Master","automatic":true})"), "SMK-37 Elite -Master");
    check(bool(chosen["automatic"]) && ids(chosen["playing"]) == "smk37-elite", "Automatic: the first that fits");
    auto refusal = call(rpc, "controllers.choose", R"({"input":"SMK-37 Elite -Master","profiles":["nonesuch"]})");
    check(int(refusal["error"]["code"]) == -32602 && refusal["error"]["message"].toString().contains("nonesuch"), "an unknown profile refused: " + refusal["error"]["message"].toString().toStdString());
    refusal = call(rpc, "controllers.choose", R"({"input":"SMK-37 Elite -Master","profiles":["smk37-elite"],"automatic":true})");
    check(int(refusal["error"]["code"]) == -32602, "profiles and automatic together refused");
    call(rpc, "controllers.choose", R"({"input":"SMK-37 Elite -Master","profiles":["smk37-elite-custom"]})");
    // A port renamed: the profile chosen for its new name plays it, though the name does not carry the port's.
    call(rpc, "controllers.choose", R"({"input":"Left keyboard","profiles":["smk37-elite-custom"]})");
    auto withRenamed = inputs; withRenamed.push_back("Left keyboard");
    ports->setInputs(withRenamed);
    auto renamed = port(call(rpc, "controllers.inputs"), "Left keyboard");
    check(renamed["profiles"].size() == 1 && renamed["profiles"][0]["id"] == "smk37-elite-custom" && renamed["use"] == "controls", "a renamed port, chosen before it was connected, plays as chosen");
    ports->setInputs(inputs);

    // Ignoring: every message from it left out, beside the duplicate and Mackie ports.
    changes = ports->changes();
    iac = port(call(rpc, "controllers.ignore", R"({"input":"IAC Bus 1","ignored":true})"), "IAC Bus 1");
    played = ports->applied();
    check(bool(iac["ignored"]) && ports->changes() > changes && std::find(played.ignored.begin(), played.ignored.end(), "IAC Bus 1") != played.ignored.end(), "a port ignored is left out");
    check(port(call(rpc, "controllers.ignore", R"({"input":"SMK-37 Elite -MCP","ignored":true})"), "SMK-37 Elite -MCP")["ignored"], "a second");
    check(!bool(port(call(rpc, "controllers.ignore", R"({"input":"SMK-37 Elite -MCP","ignored":false})"), "SMK-37 Elite -MCP")["ignored"]), "and unignored");

    // A restart: the choices and the ignored port kept; the app's file, changed since, not taken over again.
    old.replaceWithText(R"({"SMK-37 Elite -Master": []})");
    session.setControllerHost({});
    ports = std::make_unique<host::ControllerPorts>(files);
    ports->setInputs(inputs);
    wire();
    listed = call(rpc, "controllers.inputs");
    check(ids(port(listed, "SMK-37 Elite -Master")["chosen"]) == "smk37-elite-custom" && bool(port(listed, "IAC Bus 1")["ignored"]) && !bool(port(listed, "SMK-37 Elite -MCP")["ignored"]),
          "after a restart: -Master's choice and the ignored port, as they were");

    // An app's connection: `system` lists and changes; play, edit and library are refused.
    {
        JsonRpc app(session);
        app.restrict(1 | 2 | 4);
        for(const auto& [method, params] : {std::pair<std::string, std::string>{"controllers.inputs", "{}"}, {"controllers.choose", R"({"input":"IAC Bus 1","automatic":true})"}, {"controllers.ignore", R"({"input":"IAC Bus 1","ignored":false})"}})
        {
            const auto reply = call(app, method, params);
            check(int(reply["error"]["code"]) == -32008 && reply["error"]["message"].toString().contains("system"), method + " refused to an app without system: " + reply["error"]["message"].toString().toStdString());
        }
        check(bool(port(listed, "IAC Bus 1")["ignored"]) && bool(port(call(rpc, "controllers.inputs"), "IAC Bus 1")["ignored"]), "and nothing changed");
        JsonRpc settings(session);
        settings.restrict(8);
        check(call(settings, "controllers.inputs")["result"]["inputs"].size() == 4, "an app granted system lists them");
    }
    // A session with no host's controllers says so.
    session.setControllerHost({});
    check(int(call(rpc, "controllers.inputs")["error"]["code"]) == -32005, "a session without them refuses, saying so");

    root.deleteRecursively(false);
    if(failures) std::fprintf(stderr, "%d failed\n", failures);
    return failures ? 1 : 0;
}
