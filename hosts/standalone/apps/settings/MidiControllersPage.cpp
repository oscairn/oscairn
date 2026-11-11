// SPDX-License-Identifier: AGPL-3.0-or-later
#include "MidiControllersPage.h"

namespace oscairn::standalone::settings
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
        // A profile's name by its id, as the port lists it.
        juce::String named(const Control::MidiPort& port, const std::string& id)
        {
            for(const auto& p : port.profiles) if(p.id == id) return text(p.name);
            return text(id);
        }
        // What the port is, in plain words (controllers.inputs' use).
        juce::String said(const Control::MidiPort& port)
        {
            if(port.ignored) return "Ignored: everything it sends is left out.";
            const auto by = port.playing.empty() ? juce::String() : named(port, port.playing.front());
            if(port.use == "controls") return "Played by " + by + (port.playing.size() > 1 ? " and the others chosen, by MIDI channel" : juce::String()) + ": its controls join.";
            if(port.use == "mackie") return "Read as Mackie Control by " + by + ".";
            if(port.use == "duplicate") return "Left out: " + by + " sends the same on another port.";
            if(port.profiles.empty()) return "No controller profile fits it: it plays notes as any MIDI input does.";
            return "No profile: it plays notes as any MIDI input does.";
        }
        // The menu's items: Automatic, None, each profile that fits, then each of them together where several do.
        constexpr int automatic = 1, none = 2, first = 3;
    }

    MidiControllersPage::PortControls::PortControls()
    {
        profile.setTooltip("The controller profile that plays this port. Automatic: the first that fits");
        ignore.setTooltip("Leave out everything this port sends");
        addAndMakeVisible(profile); addAndMakeVisible(ignore);
    }

    void MidiControllersPage::PortControls::resized()
    {
        auto area = getLocalBounds();
        ignore.setBounds(area.removeFromRight(std::min(100, area.getWidth() / 3)));
        area.removeFromRight(8);
        profile.setBounds(area);
    }

    MidiControllersPage::MidiControllersPage(Engine& engine) : engine_(engine)
    {
        noPorts.setText("No MIDI input is connected", juce::dontSendNotification);
        noPorts.setColour(juce::Label::textColourId, ink(Ink::Muted));
        seen_ = engine_.sessionStatus().midiPortList;
        list();
        startTimer(250);
    }

    void MidiControllersPage::list()
    {
        busy_ = true; show();
        auto listed = std::make_shared<std::vector<Control::MidiPort>>();
        engine_.job([listed](Control& c) { *listed = c.midiPorts(); },
            [safe = juce::Component::SafePointer<MidiControllersPage>(this), listed](const std::string& error) {
                if(!safe) return;
                safe->busy_ = false; ++safe->listings_;
                safe->listError_ = text(error);
                if(error.empty()) safe->ports_ = *listed;
                safe->show();
            });
    }

    void MidiControllersPage::change(size_t port, std::function<std::vector<Control::MidiPort>(Control&)> work)
    {
        if(busy_ || port >= ports_.size()) return;
        busy_ = true;
        refused_.clear(); refusal_.clear();
        show();
        auto changed = std::make_shared<std::vector<Control::MidiPort>>();
        engine_.job([work = std::move(work), changed](Control& c) { *changed = work(c); },
            [safe = juce::Component::SafePointer<MidiControllersPage>(this), changed, name = ports_[port].name](const std::string& error) {
                if(!safe) return;
                safe->busy_ = false;
                if(error.empty()) { safe->ports_ = *changed; safe->show(); return; }
                // Refused: said under its port, and listed again as it is.
                safe->refused_ = name; safe->refusal_ = text(error);
                safe->list();
            });
    }

    void MidiControllersPage::choose(size_t port, const std::optional<std::vector<std::string>>& profiles)
    {
        if(port < ports_.size()) change(port, [input = ports_[port].name, profiles](Control& c) { return c.chooseMidiPort(input, profiles); });
    }

    void MidiControllersPage::ignore(size_t port, bool ignored)
    {
        if(port < ports_.size()) change(port, [input = ports_[port].name, ignored](Control& c) { return c.ignoreMidiPort(input, ignored); });
    }

    void MidiControllersPage::show()
    {
        clearRows(); rows_.clear();
        noPorts.setVisible(ports_.empty());
        for(size_t i = 0; i < ports_.size(); ++i)
        {
            const auto& port = ports_[i];
            if(controls_.size() <= i) controls_.push_back(std::make_unique<PortControls>());
            auto& c = *controls_[i];
            c.profile.clear(juce::dontSendNotification);
            c.profile.addItem(port.profiles.empty() ? juce::String("Automatic") : "Automatic (" + text(port.profiles.front().name) + ")", automatic);
            c.profile.addItem("None", none);
            for(size_t k = 0; k < port.profiles.size(); ++k) c.profile.addItem(text(port.profiles[k].name), first + int(k));
            const int each = first + int(port.profiles.size());
            if(port.profiles.size() > 1) c.profile.addItem("Each of these, by MIDI channel", each); // identical controllers, a profile each
            int selected = automatic;
            if(!port.automatic && port.chosen.empty()) selected = none;
            else if(!port.automatic && port.chosen.size() > 1 && port.profiles.size() > 1) selected = each;
            else if(!port.automatic)
                for(size_t k = 0; k < port.profiles.size(); ++k) if(port.profiles[k].id == port.chosen.front()) selected = first + int(k);
            c.profile.setSelectedId(selected, juce::dontSendNotification);
            c.profile.setTitle("Profile for " + text(port.name));
            c.profile.onChange = [this, i, each, ids = port.profiles, raw = &c.profile] {
                const int id = raw->getSelectedId();
                std::vector<std::string> chosen;
                for(size_t k = 0; k < ids.size(); ++k) if(id == each || id == first + int(k)) chosen.push_back(ids[k].id);
                if(id == automatic) choose(i, std::nullopt);
                else choose(i, chosen); // None: empty
            };
            c.ignore.setToggleState(port.ignored, juce::dontSendNotification);
            c.ignore.setTitle("Ignore " + text(port.name));
            c.ignore.onClick = [this, i, raw = &c.ignore] { ignore(i, raw->getToggleState()); };
            c.profile.setEnabled(!busy_ && !port.ignored);
            c.ignore.setEnabled(!busy_);
            auto& row = addRow(text(port.name), c, said(port));
            if(port.name == refused_) row.setExplanation(refusal_, true);
            rows_.push_back(&row);
        }
        controls_.resize(ports_.size());
        if(ports_.empty())
        {
            auto& row = addRow("MIDI inputs", noPorts);
            if(listError_.isNotEmpty()) row.setExplanation("The inputs could not be read: " + listError_, true);
            rows_.push_back(&row);
        }
        heightChanged();
    }

    void MidiControllersPage::timerCallback()
    {
        // The inputs came or went, or a choice changed (here or elsewhere): read again.
        if(const auto now = engine_.sessionStatus().midiPortList; now != seen_ && !busy_) { seen_ = now; list(); }
    }
}
