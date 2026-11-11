// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Frame.h"
#include <memory>
#include <optional>

namespace oscairn::standalone::settings
{
    /// MIDI controllers (RFC 0024): a row for each MIDI input the engine has, with the controller
    /// profile that plays it (Automatic: the first that fits; None; one of those that fit; or,
    /// where several fit, each of them, for identical controllers told apart by MIDI channel) and
    /// an Ignore switch, and under it what the port is to its profile. The engine keeps both per
    /// port name. Read with controllers.inputs and changed with controllers.choose and
    /// controllers.ignore, off the message thread (Engine::job); listed again when the status says
    /// the inputs or a choice changed (midiPortList). A change refused says why under its port.
    class MidiControllersPage : public Page, private juce::Timer
    {
    public:
        explicit MidiControllersPage(Engine& engine);
        // A port's controls, side by side as its row's one control.
        struct PortControls : juce::Component
        {
            PortControls();
            juce::ComboBox profile;
            juce::ToggleButton ignore{"Ignore"};
            void resized() override;
        };
        juce::Label noPorts;                                    // in place of the list when there is no input
        const std::vector<Control::MidiPort>& ports() const { return ports_; }
        PortControls& controls(size_t i) { return *controls_[i]; } // in ports()'s order
        const Row& portRow(size_t i) const { return *rows_[i]; }
        bool busy() const { return busy_; }                     // a listing or a change is out
        int listings() const { return listings_; }              // how many listings have arrived
        // Ignored while busy, when the controls are disabled. profiles: nullopt for Automatic.
        void choose(size_t port, const std::optional<std::vector<std::string>>& profiles);
        void ignore(size_t port, bool ignored);

    private:
        void list();
        void change(size_t port, std::function<std::vector<Control::MidiPort>(Control&)> work);
        void show();
        void timerCallback() override;
        Engine& engine_;
        std::vector<Control::MidiPort> ports_;
        std::vector<std::unique_ptr<PortControls>> controls_;
        std::vector<Row*> rows_;
        bool busy_ = false;
        int listings_ = 0;
        uint64_t seen_ = 0;
        juce::String listError_, refusal_;
        std::string refused_; // the port the last refusal is said under, until a change goes through
    };
}
