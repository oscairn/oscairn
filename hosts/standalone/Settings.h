// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Engine.h"
#include "Profile.h"
#include "SigningKeys.h"
#include "Theme.h"
#include <juce_gui_basics/juce_gui_basics.h>

namespace oscairn::standalone
{
    /// A package repository's address, asked for (RFC 0031): its https:// address, or a folder
    /// chosen or typed by its full path. The field is touch-sized.
    class RepositoryAddress : public juce::Component
    {
    public:
        RepositoryAddress();
        juce::TextEditor address;
        juce::TextButton add{"Add"}, folder{"Choose folder..."}, cancel{"Cancel"};
        void paint(juce::Graphics&) override;
        void resized() override;
    };

    /// Settings: the device's signing keys (RFC 0006), which sign exports, each by name and
    /// fingerprint (never the private key), and Make a key; the connected MIDI controllers
    /// (RFC 0024), each port with the profile it is played by, a choice where several match; and
    /// the package repositories (RFC 0031), each by name, address and root key fingerprint with a
    /// switch, Remove for the user's (the official one and the device's can be turned off, not
    /// removed), and Add a repository..., which trusts a root key only when the user says so.
    /// Wide enough, the repositories take a column of their own.
    class Settings : public juce::Component
    {
    public:
        explicit Settings(Engine& engine);
        void refresh(); // reads the keys and the ports again
        const std::vector<Control::SigningKey>& keys() const { return keys_; }
        juce::Button& makeKeyButton() { return makeKey_; }
        // The controllers' side: the profiles, the MIDI inputs' names now, the choices kept and
        // where a choice goes (kept for the port's name).
        struct ControllerPorts
        {
            std::vector<Profile> profiles;
            std::function<std::vector<std::string>()> inputs;
            std::function<ProfileChoices()> choices;
            std::function<void(const std::string& port, const std::vector<std::string>& profiles)> choose; // several: identical controllers by channel
        };
        void setControllers(ControllerPorts ports);
        // A port a profile matches: the profiles that do (their places), the one in use, and what
        // it does with the port.
        struct PortRow { std::string port; std::vector<size_t> profiles; size_t chosen = 0; juce::String note; bool all = false; }; // all: every profile that fits, by channel
        const std::vector<PortRow>& ports() const { return ports_; }
        juce::ComboBox* profileBox(size_t row) { return row < boxes_.size() ? boxes_[row].get() : nullptr; }
        const std::vector<Control::Repository>& repositories() const { return repositories_; }
        juce::ToggleButton* repositorySwitch(size_t row) { return row < switches_.size() ? switches_[row].get() : nullptr; }
        juce::Button* removeButton(size_t row) { return row < removes_.size() ? removes_[row].get() : nullptr; } // null for the official and the device's
        juce::Button& addRepositoryButton() { return addRepository_; }
        void addRepository();                              // asks for its address
        void openRepository(const juce::String& address); // https:// or a folder's full path; any other refused before the engine is asked

        void paint(juce::Graphics&) override;
        void resized() override;

    private:
        int mode() const { return std::max(1, desktopMode(*this)); }
        Engine& engine_;
        std::vector<Control::SigningKey> keys_;
        juce::TextButton makeKey_{"Make a key..."};
        juce::Rectangle<int> keysHeading_, keysArea_, portsHeading_, portsArea_, reposHeading_, reposArea_;
        ControllerPorts controllers_;
        std::vector<PortRow> ports_;
        std::vector<std::unique_ptr<juce::ComboBox>> boxes_;
        std::vector<Control::Repository> repositories_;
        std::vector<std::unique_ptr<juce::ToggleButton>> switches_;
        std::vector<std::unique_ptr<juce::TextButton>> removes_; // null where it cannot be removed
        juce::TextButton addRepository_{"Add a repository..."};
        std::unique_ptr<juce::FileChooser> chooser_;
        void readPorts();
        void readRepositories();
        void listThenAdd(const std::string& address, bool trusted); // trusted: its key was just trusted by the user
    };
}
