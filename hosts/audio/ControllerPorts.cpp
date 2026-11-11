// SPDX-License-Identifier: AGPL-3.0-or-later
#include "ControllerPorts.h"
#include "oscairn/library.h"
#include <juce_core/juce_core.h>
#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <stdexcept>

namespace oscairn::host
{
    namespace
    {
        std::filesystem::path keptFile() { return dataHome() / "controllers.json"; }
        // Where the app kept the choices before the engine did: its configuration folder, found as Theme finds it.
        juce::File appChoices()
        {
            juce::File root;
            if(const auto env = juce::SystemStats::getEnvironmentVariable("OSCAIRN_CONFIG_HOME", {}); env.isNotEmpty()) root = juce::File(env);
#if JUCE_MAC
            else root = juce::File::getSpecialLocation(juce::File::userApplicationDataDirectory).getChildFile("Application Support/Oscairn");
#elif JUCE_LINUX
            else { const auto xdg = juce::SystemStats::getEnvironmentVariable("XDG_CONFIG_HOME", {}); root = (xdg.isNotEmpty() ? juce::File(xdg) : juce::File::getSpecialLocation(juce::File::userHomeDirectory).getChildFile(".config")).getChildFile("Oscairn"); }
#else
            else root = juce::File::getSpecialLocation(juce::File::userApplicationDataDirectory).getChildFile("Oscairn");
#endif
            return root.getChildFile("controller-profiles.json");
        }
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
    }

    ControllerPorts::ControllerPorts(const std::vector<std::string>& files) : profiles_(standalone::controllerProfiles(files))
    {
        try
        {
            const auto file = keptFile();
            if(std::filesystem::exists(file))
            {
                const auto v = juce::JSON::parse(juce::File(file.string()).loadFileAsString());
                if(!v.isObject() || v["format"] != "oscairn.controllers") return; // unreadable: none
                if(const auto* chosen = v["choices"].getDynamicObject())
                    for(const auto& [port, ids] : chosen->getProperties())
                        if(const auto* list = ids.getArray())
                        {
                            auto& kept = choices_[port.toString().toStdString()]; // an empty list kept: none chosen
                            for(const auto& id : *list) if(id.isString()) kept.push_back(id.toString().toStdString());
                        }
                if(const auto* list = v["ignored"].getArray())
                    for(const auto& name : *list) if(name.isString()) ignored_.push_back(name.toString().toStdString());
                return;
            }
            // None kept yet: the app's, taken over once (the file it kept them in stays, untouched).
            const auto old = appChoices();
            if(!old.existsAsFile()) return;
            choices_ = standalone::readProfileChoices(old.getFullPathName().toStdString());
            write();
            std::fprintf(stderr, "oscairn: the controller profiles chosen for %zu ports, taken over from %s\n", choices_.size(), old.getFullPathName().toRawUTF8());
        }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: the controller profiles chosen are not read: %s\n", e.what()); }
    }

    void ControllerPorts::write() const
    {
        auto* o = new juce::DynamicObject;
        const juce::var v(o);
        o->setProperty("format", "oscairn.controllers");
        o->setProperty("version", 1);
        auto* chosen = new juce::DynamicObject;
        o->setProperty("choices", juce::var(chosen));
        for(const auto& [port, ids] : choices_)
        {
            juce::Array<juce::var> list;
            for(const auto& id : ids) list.add(text(id));
            chosen->setProperty(text(port), list);
        }
        juce::Array<juce::var> ignored;
        for(const auto& name : ignored_) ignored.add(text(name));
        o->setProperty("ignored", ignored);
        writeAtomically(keptFile(), juce::JSON::toString(v).toStdString() + "\n");
    }

    void ControllerPorts::setInputs(std::vector<std::string> names)
    {
        std::lock_guard lock(lock_);
        if(names != inputs_) { inputs_ = std::move(names); ++changes_; }
    }

    std::vector<Control::MidiPort> ControllerPorts::list() const
    {
        std::lock_guard lock(lock_);
        const auto midi = standalone::matchPorts(profiles_, inputs_, "midi", choices_), mackie = standalone::matchPorts(profiles_, inputs_, "mackie", choices_);
        const auto duplicates = standalone::ignoredInputs(profiles_, inputs_, choices_);
        auto matched = [](const std::vector<standalone::PortMatch>& matches, size_t i) { return std::any_of(matches.begin(), matches.end(), [&](const auto& m) { return m.input == i; }); };
        std::vector<Control::MidiPort> out;
        for(size_t i = 0; i < inputs_.size(); ++i)
        {
            Control::MidiPort port;
            port.name = inputs_[i];
            for(const auto p : standalone::profilesFor(profiles_, inputs_, i)) port.profiles.push_back({profiles_[p].id, profiles_[p].name});
            if(const auto chosen = choices_.find(port.name); chosen != choices_.end())
            {
                port.automatic = false; port.chosen = chosen->second;
                for(const auto& id : chosen->second) // chosen for a name it does not carry: a port renamed
                    for(const auto& p : profiles_)
                        if(p.id == id && std::none_of(port.profiles.begin(), port.profiles.end(), [&](const auto& q) { return q.id == id; })) port.profiles.push_back({p.id, p.name});
            }
            for(const auto p : standalone::profilesOf(profiles_, inputs_, i, choices_)) port.playing.push_back(profiles_[p].id);
            port.use = matched(midi, i) ? "controls" : matched(mackie, i) ? "mackie" : !port.playing.empty() && std::find(duplicates.begin(), duplicates.end(), port.name) != duplicates.end() ? "duplicate" : "";
            if(port.use.empty()) port.playing.clear(); // fits by name, but none of its ports
            port.ignored = std::find(ignored_.begin(), ignored_.end(), port.name) != ignored_.end();
            out.push_back(std::move(port));
        }
        return out;
    }

    std::vector<Control::MidiPort> ControllerPorts::choose(const std::string& input, const std::optional<std::vector<std::string>>& profiles)
    {
        {
            std::lock_guard lock(lock_);
            if(input.empty() || input.size() > 256) throw std::invalid_argument("A MIDI input is named by up to 256 bytes");
            if(profiles)
                for(const auto& id : *profiles)
                    if(std::none_of(profiles_.begin(), profiles_.end(), [&](const auto& p) { return p.id == id; })) throw std::invalid_argument("No controller profile is named " + id);
            const auto before = choices_;
            if(profiles) choices_[input] = *profiles; else choices_.erase(input);
            try { write(); }
            catch(const std::exception& e) { choices_ = before; throw std::runtime_error(std::string("Not kept, so not changed: ") + e.what()); }
            ++changes_;
        }
        return list();
    }

    std::vector<Control::MidiPort> ControllerPorts::ignore(const std::string& input, bool ignored)
    {
        {
            std::lock_guard lock(lock_);
            if(input.empty() || input.size() > 256) throw std::invalid_argument("A MIDI input is named by up to 256 bytes");
            const auto before = ignored_;
            std::erase(ignored_, input);
            if(ignored) ignored_.push_back(input);
            try { write(); }
            catch(const std::exception& e) { ignored_ = before; throw std::runtime_error(std::string("Not kept, so not changed: ") + e.what()); }
            ++changes_;
        }
        return list();
    }
    uint64_t ControllerPorts::changes() const { std::lock_guard lock(lock_); return changes_; }

    ControllerPorts::Applied ControllerPorts::applied() const
    {
        std::lock_guard lock(lock_);
        Applied a;
        std::vector<bool> joined(profiles_.size());
        for(const auto& m : standalone::matchPorts(profiles_, inputs_, "midi", choices_))
        {
            a.ports.push_back({inputs_[m.input], profiles_[m.profile].id, uint32_t(m.port)});
            joined[m.profile] = true;
        }
        a.ignored = standalone::ignoredInputs(profiles_, inputs_, choices_);
        for(const auto& name : inputs_) if(std::find(ignored_.begin(), ignored_.end(), name) != ignored_.end()) a.ignored.push_back(name);
        for(const auto& m : standalone::matchPorts(profiles_, inputs_, "mackie", choices_)) { a.ignored.push_back(inputs_[m.input]); joined[m.profile] = true; } // the app reads it; its buttons are notes
        for(size_t p = 0; p < profiles_.size(); ++p)
            if(joined[p]) for(auto& g : standalone::controlGroups(profiles_[p])) a.groups.push_back(std::move(g));
        return a;
    }
}
