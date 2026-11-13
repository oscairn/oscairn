// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Profile.h"
#include "ProfileData.h"
#include <juce_core/juce_core.h>
#include <algorithm>
#include <map>
#include <set>
#include <stdexcept>
#include <tuple>

namespace oscairn::standalone
{
    namespace
    {
        constexpr int maxProfileBytes = 64 * 1024;
        void require(bool ok, const std::string& why) { if(!ok) throw std::invalid_argument("profile: " + why); }
        const juce::DynamicObject& object(const juce::var& v, const std::string& what)
        {
            const auto* o = v.getDynamicObject(); require(o != nullptr, what + " must be an object"); return *o;
        }
        void fields(const juce::DynamicObject& o, const juce::StringArray& allowed, const std::string& what)
        {
            for(const auto& p : o.getProperties()) require(allowed.contains(p.name.toString()), "unknown field " + p.name.toString().toStdString() + " in " + what);
        }
        std::string text(const juce::DynamicObject& o, const char* key, bool required = true)
        {
            const auto v = o.getProperty(key);
            if(v.isVoid() && !required) return {};
            require(v.isString() && v.toString().length() <= 256 && !v.toString().containsChar(0), std::string(key) + " must be a short string");
            return v.toString().toStdString();
        }
        int count(const juce::DynamicObject& o, const char* key, int high)
        {
            const auto v = o.getProperty(key);
            if(v.isVoid()) return 0;
            require(v.isInt() && int(v) >= 0 && int(v) <= high, std::string(key) + " must be a whole number from 0 to " + std::to_string(high));
            return int(v);
        }
        bool flag(const juce::DynamicObject& o, const char* key)
        {
            const auto v = o.getProperty(key);
            if(v.isVoid()) return false;
            require(v.isBool(), std::string(key) + " must be true or false");
            return bool(v);
        }
        std::vector<std::string> names(const juce::DynamicObject& o, const char* key)
        {
            std::vector<std::string> out;
            const auto v = o.getProperty(key);
            if(v.isVoid()) return out;
            require(v.isArray() && v.size() <= 64, std::string(key) + " must be a list");
            for(const auto& e : *v.getArray()) { require(e.isString() && e.toString().isNotEmpty() && e.toString().length() <= 128, std::string(key) + " holds names"); out.push_back(e.toString().toStdString()); }
            return out;
        }
        const juce::StringArray controlKinds{"keys", "pads", "encoders", "knobs", "faders", "buttons", "pedal", "footswitches", "expression", "jog", "touchstrip", "wheels", "aftertouch", "breath"};
        const juce::StringArray controlRoles{"notes", "screen", "assign", "fixed"};
        const juce::StringArray banks{"assign", "filter", "ampEG", "eq", "levels", "effects"};
        const juce::StringArray surfaces{"desktop", "panel", "compact"};
        const juce::StringArray reliabilities{"desktop", "instrument", "pass-through"};
        const juce::StringArray sections{"platform", "compute", "audio", "controls", "screens", "midi", "cv", "network", "power"};
        const juce::StringArray standardControllers{"pitchBend", "modulation", "breath", "foot", "expression", "sustain", "portamento", "sostenuto", "soft", "aftertouch"};
        const juce::StringArray encodingNames{"absolute", "twosComplement", "binaryOffset", "signMagnitude"};
        std::vector<int> numbers(const juce::DynamicObject& o, const char* key, int high)
        {
            std::vector<int> out;
            const auto v = o.getProperty(key);
            if(v.isVoid()) return out;
            require(v.isArray() && v.size() <= 1024, std::string(key) + " must be a list");
            for(const auto& e : *v.getArray()) { require(e.isInt() && int(e) >= 0 && int(e) <= high, std::string(key) + " holds numbers from 0 to " + std::to_string(high)); out.push_back(int(e)); }
            return out;
        }
    }

    bool Profile::offers(const std::string& app) const { return std::find(apps.begin(), apps.end(), app) != apps.end(); }

    void Profile::configureAudio(host::AudioConfiguration& config) const
    {
        const auto& d = audio.device;
        if(d.name.empty()) return;
        auto list = [](const Channels& logical) {
            std::string out;
            for(const auto& entry : logical) for(auto c : entry.second) out += (out.empty() ? "" : ",") + std::to_string(c);
            return out;
        };
        config.setOption("--output-device", d.name);
        if(!d.outputs.empty()) config.setOption("--audio-outputs", list(d.outputs));
        if(!d.inputs.empty()) { config.setOption("--input-device", d.name); config.setOption("--audio-inputs", list(d.inputs)); }
        if(d.channels) config.setOption("--device-channels", std::to_string(d.channels));
        if(d.rate) config.setOption("--sample-rate", std::to_string(d.rate));
    }

    // shortcut: RFC 0025's illustrative figures, not measured; measure each app on each profile and read them from manifests (step 8).
    int Profile::appMemory(const std::string& app)
    {
        static const std::map<std::string, int> own{{"oscairn.performance", 150}, {"oscairn.library", 120}, {"oscairn.setlist", 60}, {"oscairn.groovebox", 200}, {"oscairn.recordings", 100}};
        const auto it = own.find(app);
        return it == own.end() ? 0 : it->second;
    }

    Profile Profile::parse(const std::string& json)
    {
        require(json.size() <= maxProfileBytes, "larger than 64 KiB");
        juce::var value;
        require(juce::JSON::parse(juce::String::fromUTF8(json.data(), int(json.size())), value).wasOk(), "not valid JSON");
        const auto& root = object(value, "a profile");
        fields(root, {"format", "version", "id", "name", "platform", "compute", "audio", "controls", "screens", "midi", "cv", "network", "power", "panel", "keyboard", "inputKeys", "hardware", "reliability", "apps", "alwaysAvailable", "repositories", "onScreenKeyboard", "estimated"}, "the profile");
        require(text(root, "format") == "oscairn.profile", "format must be oscairn.profile");
        require(root.getProperty("version").isInt() && int(root.getProperty("version")) == 2, "version must be 2");
        Profile p;
        // A controller's profile (it names ports) holds its controls and ports only.
        if(const auto m = root.getProperty("midi"); m.getDynamicObject() && m.getDynamicObject()->hasProperty("ports"))
            for(const auto* key : {"platform", "compute", "audio", "screens", "cv", "network", "power", "panel", "keyboard", "inputKeys", "hardware", "reliability", "apps", "alwaysAvailable", "repositories", "onScreenKeyboard"})
                require(!root.hasProperty(key), std::string("a controller's profile has no ") + key);
        const bool controller = root.getProperty("midi").getDynamicObject() && root.getProperty("midi").getDynamicObject()->hasProperty("ports");
        p.id = text(root, "id"); p.name = text(root, "name");
        require(!p.id.empty() && std::all_of(p.id.begin(), p.id.end(), [](char c) { return std::isalnum(static_cast<unsigned char>(c)) || c == '-' || c == '.'; }), "id holds letters, digits, dots and hyphens");
        if(const auto v = root.getProperty("platform"); !v.isVoid())
        {
            const auto& o = object(v, "platform"); fields(o, {"os", "cpu", "runtime", "board"}, "platform");
            p.platform = {text(o, "os"), text(o, "cpu"), text(o, "runtime", false), text(o, "board", false)};
        }
        if(const auto v = root.getProperty("compute"); !v.isVoid())
        {
            const auto& o = object(v, "compute"); fields(o, {"cores", "realtimeCores", "memory", "storage", "osMemory", "performanceMemory", "audioThreadCores", "workerAllowance", "workerSpin"}, "compute");
            p.compute = {count(o, "cores", 1024), count(o, "realtimeCores", 1024), count(o, "memory", 1 << 24), count(o, "storage", 1 << 30),
                         count(o, "osMemory", 1 << 24), count(o, "performanceMemory", 1 << 24), numbers(o, "audioThreadCores", 63),
                         count(o, "workerAllowance", 1'000'000), count(o, "workerSpin", 1'000'000)};
            require(p.compute.realtimeCores <= p.compute.cores, "more realtime cores than cores");
            for(const auto c : p.compute.audioThreadCores) require(c < p.compute.cores, "audioThreadCores names a core past cores");
            require(o.hasProperty("osMemory") == o.hasProperty("performanceMemory"), "osMemory and performanceMemory come together");
            require(p.compute.osMemory || !o.hasProperty("osMemory"), "osMemory is more than 0");
        }
        if(const auto v = root.getProperty("audio"); !v.isVoid())
        {
            const auto& o = object(v, "audio"); fields(o, {"inputs", "outputs", "rates", "block", "headphones", "device"}, "audio");
            p.audio.inputs = count(o, "inputs", 256); p.audio.outputs = count(o, "outputs", 256);
            p.audio.block = count(o, "block", 8192); p.audio.headphones = flag(o, "headphones");
            if(p.audio.block) require(p.audio.block >= 16, "block is at least 16 frames");
            if(const auto r = o.getProperty("rates"); !r.isVoid())
            {
                require(r.isArray(), "rates must be a list");
                for(const auto& e : *r.getArray()) { require(e.isInt() && int(e) >= 8000 && int(e) <= 384000, "rates are sample rates in Hz"); p.audio.rates.push_back(int(e)); }
            }
            if(const auto d = o.getProperty("device"); !d.isVoid())
            {
                const auto& od = object(d, "audio.device"); fields(od, {"name", "channels", "rate", "outputs", "inputs"}, "audio.device");
                auto& device = p.audio.device;
                device.name = text(od, "name");
                require(!device.name.empty(), "audio.device names its device");
                device.channels = count(od, "channels", 256); device.rate = count(od, "rate", 384000);
                require(device.channels > 0 || !od.hasProperty("channels"), "audio.device's channels are at least 1");
                require(device.rate >= 8000 || !od.hasProperty("rate"), "audio.device's rate is a sample rate in Hz");
                for(auto [key, list] : {std::pair{"outputs", &device.outputs}, std::pair{"inputs", &device.inputs}})
                {
                    const auto m = od.getProperty(key);
                    if(m.isVoid()) continue;
                    std::set<int> used;
                    for(const auto& [name, channels] : object(m, std::string("audio.device.") + key).getProperties())
                    {
                        require(channels.isArray() && channels.size() >= 1 && channels.size() <= 256, std::string("each of audio.device.") + key + " lists its device channels");
                        std::vector<int> mapped;
                        for(const auto& c : *channels.getArray())
                        {
                            require(c.isInt() && int(c) >= 0 && int(c) < (device.channels ? device.channels : 256), "audio.device's channels are numbered from 0, below its channel count");
                            require(used.insert(int(c)).second, "audio.device maps a device channel twice");
                            mapped.push_back(int(c));
                        }
                        list->emplace_back(name.toString().toStdString(), std::move(mapped));
                    }
                    const auto main = std::find_if(list->begin(), list->end(), [](const auto& e) { return e.first == "main"; });
                    require(main != list->end() && main->second.size() == 2, std::string("audio.device.") + key + " has main, left and right");
                    std::rotate(list->begin(), main, main + 1); // main first, the others in order
                }
            }
        }
        if(const auto v = root.getProperty("controls"); !v.isVoid())
        {
            require(v.isArray() && v.size() <= 64, "controls must be a list");
            for(const auto& e : *v.getArray())
            {
                const auto& o = object(e, "a control"); fields(o, {"kind", "count", "sensing", "role", "rows", "paging", "modes", "functions", "target", "midi"}, "a control");
                Control c{text(o, "kind"), count(o, "count", 1024), text(o, "sensing", false), text(o, "role")};
                require(controlKinds.contains(juce::String(c.kind)), "unknown control kind " + c.kind);
                require(c.count > 0, "a control's count is at least 1");
                require(controlRoles.contains(juce::String(c.role)), "a control's role is notes, screen, assign or fixed");
                require((c.role == "notes") == (c.kind == "keys" || c.kind == "pads"), "keys and pads, and only they, play notes");
                if(const auto r = o.getProperty("rows"); !r.isVoid())
                {
                    require(r.isArray() && r.size() > 0 && r.size() <= 16, "rows must be a list of 1 to 16");
                    for(const auto& row : *r.getArray())
                    {
                        const auto& ro = object(row, "a row"); fields(ro, {"name", "bank"}, "a row");
                        c.rows.push_back({text(ro, "name"), text(ro, "bank")});
                        require(banks.contains(juce::String(c.rows.back().bank)), "a row's bank is assign, filter, ampEG, eq, levels or effects");
                    }
                }
                c.paging = text(o, "paging", false);
                require(c.paging.empty() || c.paging == "button" || c.paging == "screen", "paging is button or screen");
                c.modes = names(o, "modes");
                for(const auto& m : c.modes) require(m == "part" || m == "element" || m == "audio", "a fader mode is part, element or audio");
                c.functions = names(o, "functions");
                c.target = text(o, "target", false);
                const bool knobs = c.kind == "knobs" || c.kind == "encoders", levers = c.kind == "faders" || c.kind == "buttons";
                const bool assign = c.role == "assign";
                // Knobs play banks by rows or pages; other kinds of assign group may page banks, pass a function through, or wait to be assigned.
                require(!assign || !knobs || c.rows.empty() != c.paging.empty(), "an assign group of " + c.kind + " has rows or paging, not both");
                require((assign && !levers) || (c.rows.empty() && c.paging.empty()), "only an assign group of knobs, wheels or pedals has rows or paging");
                require(c.rows.empty() || c.paging.empty(), "a group has rows or paging, not both");
                require(c.modes.empty() || c.kind == "faders", "only faders have modes");
                if(c.role == "fixed") require(c.functions.empty() || int(c.functions.size()) == c.count, "a fixed group's functions are one per control");
                else if(!c.functions.empty())
                {
                    require(assign && !knobs && !levers && c.rows.empty() && c.paging.empty() && int(c.functions.size()) == c.count,
                            "an assign group of wheels, pedals, a ribbon, footswitches or a jog has a function per control, and no rows or paging");
                    for(const auto& f : c.functions) require(standardControllers.contains(juce::String(f)), "an assign group's function is a standard controller: " + f);
                }
                if(!c.target.empty())
                    require(assign && ((knobs && c.target == "banks") || (c.kind == "faders" && c.target == "partLevels") || (c.kind == "buttons" && (c.target == "partMutes" || c.target == "scenes" || c.target == "switches" || c.target == "panic"))),
                            "target is banks for knobs, partLevels for faders, partMutes, scenes, switches or panic for buttons, in an assign group");
                if(const auto m = o.getProperty("midi"); !m.isVoid())
                {
                    require(controller, "only a controller's controls say how they talk MIDI");
                    const auto& mo = object(m, "a control's midi"); fields(mo, {"port", "channel", "cc", "notes", "pitchBend", "pressure", "encoding", "button", "feedback"}, "a control's midi");
                    auto& midi = c.midi;
                    midi.port = mo.hasProperty("port") ? count(mo, "port", 16) : 1;
                    if(mo.getProperty("channel").isArray())
                    {
                        midi.channels = numbers(mo, "channel", 16);
                        require(int(midi.channels.size()) == c.count, "channel is one, or one per control");
                        midi.channel = midi.channels.front();
                    }
                    else midi.channel = count(mo, "channel", 16);
                    require(midi.channel >= 1 && std::all_of(midi.channels.begin(), midi.channels.end(), [](int n) { return n >= 1; }), "a control's MIDI channel is 1 to 16");
                    midi.cc = numbers(mo, "cc", 127); midi.notes = numbers(mo, "notes", 127); midi.pitchBend = flag(mo, "pitchBend"); midi.pressure = flag(mo, "pressure");
                    const int kinds = int(!midi.cc.empty()) + int(!midi.notes.empty()) + int(midi.pitchBend) + int(midi.pressure);
                    require(kinds == 1 || (kinds == 0 && c.kind == "keys"), "a control's midi has one of cc, notes, pitchBend or pressure (keys only a channel)");
                    require(!midi.pressure || (c.kind == "aftertouch" && c.count == 1), "pressure is a group of one aftertouch");
                    const auto perRow = c.count * int(std::max<size_t>(1, c.rows.size()));
                    require(midi.cc.empty() || int(midi.cc.size()) == c.count || (!c.rows.empty() && int(midi.cc.size()) == perRow), "cc has a number per control, or per control per row");
                    require(midi.notes.empty() || int(midi.notes.size()) == c.count, "notes has a number per control");
                    require(!midi.pitchBend || (c.kind == "wheels" && c.count == 1), "pitchBend is a group of one wheel");
                    if(const auto encoding = mo.getProperty("encoding"); !encoding.isVoid())
                    {
                        require(!midi.cc.empty(), "only CCs have an encoding");
                        // A name, or {increment, decrement}: the values for one step up and one down.
                        auto one = [&](const juce::var& e) {
                            if(e.isString())
                            {
                                require(encodingNames.contains(e.toString()), "an encoding is absolute, twosComplement, binaryOffset, signMagnitude or {increment, decrement}");
                                midi.encodings.push_back(e.toString().toStdString()); midi.steps.push_back({0, 0}); return;
                            }
                            const auto& so = object(e, "an encoding"); fields(so, {"increment", "decrement"}, "an encoding");
                            require(so.hasProperty("increment") && so.hasProperty("decrement"), "an encoding's steps have an increment and a decrement");
                            const int up = count(so, "increment", 127), down = count(so, "decrement", 127);
                            require(up != down, "an encoding's increment and decrement differ");
                            midi.encodings.push_back("steps"); midi.steps.push_back({up, down});
                        };
                        if(encoding.isArray()) { require(encoding.size() <= 1024, "encoding must be a list"); for(const auto& e : *encoding.getArray()) one(e); }
                        else for(int k = 0; k < c.count; ++k) one(encoding);
                        require(int(midi.encodings.size()) == c.count, "encoding is one, or one per control");
                    }
                    if(mo.hasProperty("button")) { midi.button = text(mo, "button"); require((c.kind == "buttons" || c.kind == "footswitches") && (midi.button == "momentary" || midi.button == "latch"), "a button or footswitch is momentary or latch"); }
                    midi.feedback = flag(mo, "feedback");
                    require(!midi.feedback || (!midi.cc.empty() && std::all_of(midi.encodings.begin(), midi.encodings.end(), [](const std::string& e) { return e == "absolute"; })),
                            "feedback is for controls sending absolute CCs");
                }
                p.controls.push_back(c);
            }
        }
        if(const auto v = root.getProperty("screens"); !v.isVoid())
        {
            require(v.isArray() && v.size() <= 8, "screens must be a list");
            for(const auto& e : *v.getArray())
            {
                const auto& o = object(e, "a screen"); fields(o, {"width", "height", "touch", "diagonal", "surface", "layout", "theme", "textEntry"}, "a screen");
                Screen s{count(o, "width", 16384), count(o, "height", 16384), flag(o, "touch"), text(o, "surface"), text(o, "layout"), count(o, "diagonal", 10000), text(o, "theme", false)};
                if(o.hasProperty("textEntry")) { s.textEntry = text(o, "textEntry"); require(s.textEntry == "keyboard" || s.textEntry == "onScreen", "a screen's textEntry is keyboard or onScreen"); }
                require(s.theme.empty() || juce::String(s.theme).containsOnly("abcdefghijklmnopqrstuvwxyz0123456789-"), "a screen's theme is a theme's ID");
                require(s.width > 0 && s.height > 0, "a screen has a size");
                require(surfaces.contains(juce::String(s.surface)), "surface is desktop, panel or compact");
                p.screens.push_back(s);
            }
        }
        if(const auto v = root.getProperty("midi"); !v.isVoid())
        {
            const auto& o = object(v, "midi"); fields(o, controller ? juce::StringArray{"ports"} : juce::StringArray{"din", "usbDevice", "usbHost"}, controller ? "a controller's midi" : "midi");
            p.midi = {count(o, "din", 64), flag(o, "usbDevice"), flag(o, "usbHost"), {}};
            if(controller)
            {
                const auto ports = o.getProperty("ports");
                require(ports.isArray() && ports.size() >= 1 && ports.size() <= 16, "ports must be a list of 1 to 16");
                for(const auto& e : *ports.getArray())
                {
                    const auto& po = object(e, "a port"); fields(po, {"name", "index", "protocol", "ignore", "display"}, "a port");
                    Port port{text(po, "name"), po.hasProperty("index") ? count(po, "index", 64) : 1, po.hasProperty("protocol") ? text(po, "protocol") : std::string("midi"), flag(po, "ignore"), flag(po, "display")};
                    require(!port.name.empty() && port.index >= 1, "a port has a name and an index from 1");
                    require(port.protocol == "midi" || port.protocol == "mackie", "a port's protocol is midi or mackie");
                    require(!port.ignore || !po.hasProperty("protocol"), "an ignored port has no protocol");
                    require(!port.display || port.protocol == "mackie", "only a Mackie port has a display");
                    p.midi.ports.push_back(port);
                }
            }
        }
        // Each control's MIDI on a midi port of its controller, and no message from two controls.
        std::set<std::tuple<int, int, int, int>> messages; // port, channel, kind (0 cc, 1 note, 2 pitch bend), number
        for(const auto& c : p.controls)
        {
            if(!c.midi.channel) continue;
            require(c.midi.port >= 1 && c.midi.port <= int(p.midi.ports.size()) && p.midi.ports[size_t(c.midi.port - 1)].protocol == "midi" && !p.midi.ports[size_t(c.midi.port - 1)].ignore,
                    "a control's port is one of the controller's midi ports, not an ignored one");
            auto add = [&](int kind, int number, size_t k) { require(messages.insert({c.midi.port, c.midi.channelOf(k % size_t(c.count)), kind, number}).second, "two controls send the same message"); };
            for(size_t k = 0; k < c.midi.cc.size(); ++k) add(0, c.midi.cc[k], k);
            if(c.role != "notes") for(size_t k = 0; k < c.midi.notes.size(); ++k) add(1, c.midi.notes[k], k);
            if(c.midi.pitchBend) add(2, 0, 0);
        }
        if(const auto v = root.getProperty("cv"); !v.isVoid())
        {
            const auto& o = object(v, "cv"); fields(o, {"inputs", "outputs"}, "cv");
            p.cv = {count(o, "inputs", 256), count(o, "outputs", 256)};
        }
        if(const auto v = root.getProperty("network"); !v.isVoid())
        {
            const auto& o = object(v, "network"); fields(o, {"ethernet", "wifi"}, "network");
            p.network = {flag(o, "ethernet"), flag(o, "wifi")};
        }
        if(const auto v = root.getProperty("power"); !v.isVoid())
        {
            const auto& o = object(v, "power"); fields(o, {"battery"}, "power");
            p.battery = flag(o, "battery");
        }
        // RFC 0004: its accelerators and device clocks, read by the core's parser.
        if(const auto v = root.getProperty("hardware"); !v.isVoid())
        {
            try { p.hardware = hardware::fromProfile("{\"hardware\":" + juce::JSON::toString(v, true).toStdString() + "}"); }
            catch(const std::invalid_argument& e) { require(false, e.what()); }
        }
        if(const auto v = root.getProperty("panel"); !v.isVoid())
        {
            const auto& o = object(v, "panel"); fields(o, {"protocol", "device", "map"}, "panel");
            p.panel = {text(o, "protocol"), text(o, "device"), text(o, "map")};
            require(p.panel.protocol == "roland-i2c-slave", "a panel's protocol is roland-i2c-slave");
            require(p.panel.device.starts_with("/dev/"), "a panel's device is in /dev");
            require(!p.panel.map.empty(), "a panel names its message map");
        }
        if(const auto v = root.getProperty("keyboard"); !v.isVoid())
        {
            const auto& o = object(v, "keyboard"); fields(o, {"protocol", "device", "localOff"}, "keyboard");
            p.keyboard = {text(o, "protocol"), text(o, "device"), flag(o, "localOff")};
            require(p.keyboard.protocol == "roland-bmc-stock", "a keyboard's protocol is roland-bmc-stock");
            require(p.keyboard.device.starts_with("/dev/"), "a keyboard's device is in /dev");
        }
        if(const auto v = root.getProperty("inputKeys"); !v.isVoid())
        {
            const auto& o = object(v, "inputKeys"); fields(o, {"protocol", "name", "keys"}, "inputKeys");
            p.inputKeys.protocol = text(o, "protocol"); p.inputKeys.name = text(o, "name");
            require(p.inputKeys.protocol == "linux-input", "inputKeys' protocol is linux-input");
            require(!p.inputKeys.name.empty(), "inputKeys names its input device");
            const auto& keys = object(o.getProperty("keys"), "inputKeys' keys");
            std::set<int> codes;
            for(const auto& k : keys.getProperties())
            {
                require(k.value.isInt() && int(k.value) >= 0 && int(k.value) <= 767 && !k.name.toString().containsAnyOf("=,"), "an input key is a function and its code, 0 to 767");
                require(codes.insert(int(k.value)).second, "two input keys share a code");
                p.inputKeys.keys.push_back({k.name.toString().toStdString(), int(k.value)});
            }
            require(!p.inputKeys.keys.empty() && p.inputKeys.keys.size() <= 64, "inputKeys has 1 to 64 keys");
        }
        if(!controller)
        {
            p.reliability = text(root, "reliability");
            require(reliabilities.contains(juce::String(p.reliability)), "reliability is desktop, instrument or pass-through");
        }
        else p.reliability.clear();
        p.apps = names(root, "apps");
        if(root.hasProperty("onScreenKeyboard")) p.onScreenKeyboard = flag(root, "onScreenKeyboard");
        else p.onScreenKeyboard = std::none_of(p.controls.begin(), p.controls.end(), [](const Control& c) { return c.kind == "keys" && c.role == "notes"; });
        if(root.hasProperty("alwaysAvailable"))
        {
            p.alwaysAvailable = names(root, "alwaysAvailable");
            for(const auto& a : p.alwaysAvailable) require(p.offers(a), "alwaysAvailable names an app in apps: " + a);
        }
        else for(const auto* a : {"oscairn.performance", "oscairn.library"}) if(p.offers(a)) p.alwaysAvailable.push_back(a);
        // RFC 0024: the OS, the floor and the always-available apps' reservations must fit its memory.
        if(p.memoryClasses())
        {
            int64_t need = int64_t(p.compute.osMemory) + p.compute.performanceMemory;
            for(const auto& a : p.alwaysAvailable) need += appMemory(a);
            require(need <= p.compute.memory, "osMemory, performanceMemory and the always-available apps' reservations (" + std::to_string(need) +
                                              " MiB) exceed memory (" + std::to_string(p.compute.memory) + " MiB)");
        }
        if(const auto v = root.getProperty("repositories"); !v.isVoid())
        {
            const auto& o = object(v, "repositories"); fields(o, {"list", "hideOfficial"}, "repositories");
            p.hideOfficial = flag(o, "hideOfficial");
            if(const auto list = o.getProperty("list"); !list.isVoid())
            {
                require(list.isArray() && list.size() <= 64, "repositories.list must be a list");
                for(const auto& e : *list.getArray())
                {
                    const auto& r = object(e, "a repository"); fields(r, {"address", "key", "name"}, "a repository");
                    const auto address = r.getProperty("address").toString();
                    require(r.getProperty("address").isString() && address.startsWith("https://") && address.length() > 8 && address.length() <= 4096 &&
                                !address.containsAnyOf(juce::String::charToString(0) + " \t\r\n"), "a repository's address is https://");
                    p.repositories.push_back({address.toStdString(), text(r, "key"), text(r, "name")});
                    require(p.repositories.back().key.size() == 64 && std::all_of(p.repositories.back().key.begin(), p.repositories.back().key.end(), [](char c) { return std::isdigit(static_cast<unsigned char>(c)) || (c >= 'a' && c <= 'f'); }),
                            "a repository's key is its root key, 64 lowercase hex digits");
                    require(!p.repositories.back().name.empty(), "a repository needs a name");
                }
            }
        }
        p.estimated = names(root, "estimated");
        for(const auto& s : p.estimated) require(sections.contains(juce::String(s)), "estimated names a section: " + s);
        return p;
    }

    std::vector<ControlGroup> controlGroups(const Profile& profile)
    {
        std::vector<ControlGroup> out;
        for(const auto& c : profile.controls)
        {
            ControlGroup g;
            g.role = c.role == "notes" ? ControlGroup::Role::Notes : c.role == "screen" ? ControlGroup::Role::Screen : c.role == "fixed" ? ControlGroup::Role::Fixed : ControlGroup::Role::Assign;
            g.kind = c.kind; g.count = uint32_t(c.count); g.modes = c.modes;
            for(const auto& r : c.rows)
                for(size_t k = 0; k < bankKinds.size(); ++k) if(r.bank == bankKinds[k]) g.rows.push_back({r.name, BankKind(k)});
            // Its default target, and a controller's MIDI (RFC 0024).
            g.scenes = c.target == "scenes"; g.switches = c.target == "switches"; g.panic = c.target == "panic"; g.paged = !c.paging.empty();
            if(c.role != "fixed") g.functions = c.functions;
            if(profile.controller() && c.midi.channel)
            {
                using Message = ControlGroup::Midi::Message;
                g.midi.device = profile.id; g.midi.port = uint32_t(c.midi.port); g.midi.channel = uint32_t(c.midi.channel);
                g.midi.channels.assign(c.midi.channels.begin(), c.midi.channels.end());
                g.midi.message = c.midi.pitchBend ? Message::PitchBend : c.midi.pressure ? Message::Pressure : c.midi.cc.empty() ? Message::Note : Message::CC;
                for(const auto n : c.midi.cc.empty() ? c.midi.notes : c.midi.cc) g.midi.numbers.push_back(uint8_t(n));
                for(const auto& e : c.midi.encodings)
                {
                    g.midi.encodings.push_back(Encoding::Steps); // "steps", unless it is one of the names
                    for(size_t k = 0; k < encodings.size(); ++k) if(e == encodings[k]) g.midi.encodings.back() = Encoding(k);
                }
                for(const auto& [up, down] : c.midi.steps) g.midi.steps.push_back({uint8_t(up), uint8_t(down)});
                g.midi.latch = c.midi.button == "latch";
                g.midi.feedback = c.midi.feedback;
            }
            out.push_back(std::move(g));
        }
        return out;
    }

    std::vector<std::pair<std::string, std::string>> builtInProfiles()
    {
        std::vector<std::pair<std::string, std::string>> out;
        for(int i = 0; i < ProfileData::namedResourceListSize; ++i)
        {
            int size = 0;
            const auto* data = ProfileData::getNamedResource(ProfileData::namedResourceList[i], size);
            const auto file = juce::String(ProfileData::getNamedResourceOriginalFilename(ProfileData::namedResourceList[i]));
            if(data && file.endsWith(".json")) out.push_back({file.upToLastOccurrenceOf(".json", false, false).toStdString(), std::string(data, size_t(size))});
        }
        return out;
    }

    Profile readProfile(const std::string& nameOrFile)
    {
        for(const auto& [id, json] : builtInProfiles()) if(id == nameOrFile) return Profile::parse(json);
        const auto file = juce::File::getCurrentWorkingDirectory().getChildFile(juce::String(nameOrFile));
        require(file.existsAsFile(), "no profile named " + nameOrFile + " and no such file");
        require(file.getSize() <= maxProfileBytes, "larger than 64 KiB");
        return Profile::parse(file.loadFileAsString().toStdString());
    }

    std::vector<Profile> controllerProfiles(const std::vector<std::string>& files)
    {
        std::vector<Profile> out;
        for(const auto& [id, json] : builtInProfiles()) if(auto p = Profile::parse(json); p.controller()) out.push_back(std::move(p));
        for(const auto& f : files)
        {
            auto p = readProfile(f);
            require(p.controller(), p.id + " is not a controller's profile (it names no ports)");
            std::erase_if(out, [&](const Profile& q) { return q.id == p.id; }); // a file replaces the built-in of its id
            out.push_back(std::move(p));
        }
        std::sort(out.begin(), out.end(), [](const Profile& a, const Profile& b) { return a.id < b.id; });
        return out;
    }

    namespace
    {
        bool carries(const std::string& input, const Profile::Port& port) { return juce::String(input).containsIgnoreCase(juce::String(port.name)); }
        // The port (its place among the profile's) that the input at `input` is: of the ports whose
        // name it carries, the one whose index is its turn among the inputs carrying that name,
        // counted round the indexes, so any number of devices with one name each give every
        // index; ports.size(): none. Not by system IDs or order across devices: RFC 0024.
        size_t portOf(const Profile& p, const std::vector<std::string>& inputs, size_t input)
        {
            const auto& ports = p.midi.ports;
            for(size_t k = 0; k < ports.size(); ++k)
            {
                if(!carries(inputs[input], ports[k])) continue;
                int indexes = 1, turn = 0;
                for(const auto& q : ports) if(q.name == ports[k].name) indexes = std::max(indexes, q.index);
                for(size_t i = 0; i < input; ++i) turn += carries(inputs[i], ports[k]);
                if(ports[k].index == turn % indexes + 1) return k;
            }
            return ports.size();
        }
    }

    ProfileChoices readProfileChoices(const std::string& file)
    {
        ProfileChoices out;
        const auto root = juce::JSON::parse(juce::File(juce::String::fromUTF8(file.c_str())).loadFileAsString()); // kept: the object lives in it
        if(const auto* o = root.getDynamicObject())
            for(const auto& p : o->getProperties())
                if(const auto* ids = p.value.getArray())
                {
                    std::vector<std::string> list;
                    for(const auto& id : *ids) if(id.isString()) list.push_back(id.toString().toStdString());
                    if(!list.empty()) out[p.name.toString().toStdString()] = std::move(list);
                }
        return out;
    }

    std::vector<size_t> profilesFor(const std::vector<Profile>& controllers, const std::vector<std::string>& inputs, size_t input)
    {
        std::vector<size_t> out;
        for(size_t c = 0; c < controllers.size(); ++c) if(portOf(controllers[c], inputs, input) < controllers[c].midi.ports.size()) out.push_back(c);
        return out;
    }

    std::vector<size_t> profilesOf(const std::vector<Profile>& controllers, const std::vector<std::string>& inputs, size_t input, const ProfileChoices& choices)
    {
        if(const auto chosen = choices.find(inputs[input]); chosen != choices.end())
        {
            if(chosen->second.empty()) return {}; // none chosen
            std::vector<size_t> out;
            for(const auto& id : chosen->second)
                for(size_t c = 0; c < controllers.size(); ++c) if(controllers[c].id == id && std::find(out.begin(), out.end(), c) == out.end()) out.push_back(c);
            if(!out.empty()) return out;
        }
        const auto found = profilesFor(controllers, inputs, input);
        return found.empty() ? std::vector<size_t>{} : std::vector<size_t>{found.front()};
    }

    size_t profileOf(const std::vector<Profile>& controllers, const std::vector<std::string>& inputs, size_t input, const ProfileChoices& choices)
    {
        const auto all = profilesOf(controllers, inputs, input, choices);
        return all.empty() ? controllers.size() : all.front();
    }

    namespace
    {
        // The port an input is of a profile it is: the port of its name, or for a profile chosen for
        // a name it does not carry (a port renamed), its first midi port; ports.size(): none.
        size_t portIn(const Profile& p, const std::vector<std::string>& inputs, size_t input)
        {
            if(const auto k = portOf(p, inputs, input); k < p.midi.ports.size()) return k;
            for(size_t k = 0; k < p.midi.ports.size(); ++k) if(p.midi.ports[k].protocol == "midi" && !p.midi.ports[k].ignore) return k;
            return p.midi.ports.size();
        }
    }

    std::vector<PortMatch> matchPorts(const std::vector<Profile>& controllers, const std::vector<std::string>& inputs, const std::string& protocol, const ProfileChoices& choices)
    {
        std::vector<PortMatch> out;
        for(size_t i = 0; i < inputs.size(); ++i)
            for(const auto c : profilesOf(controllers, inputs, i, choices))
                if(const auto k = portIn(controllers[c], inputs, i); k < controllers[c].midi.ports.size())
                    if(const auto& port = controllers[c].midi.ports[k]; port.protocol == protocol && !port.ignore) out.push_back({i, c, int(k + 1)});
        return out;
    }

    std::vector<std::string> ignoredInputs(const std::vector<Profile>& controllers, const std::vector<std::string>& inputs, const ProfileChoices& choices)
    {
        std::vector<std::string> out;
        for(size_t i = 0; i < inputs.size(); ++i)
            if(const auto c = profileOf(controllers, inputs, i, choices); c < controllers.size())
                if(const auto k = portIn(controllers[c], inputs, i); k < controllers[c].midi.ports.size() && controllers[c].midi.ports[k].ignore) out.push_back(inputs[i]);
        return out;
    }
}
