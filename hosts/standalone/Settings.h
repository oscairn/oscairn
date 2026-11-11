// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Engine.h"
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

    /// Settings' package repositories (RFC 0031), the one section not yet in the Settings app:
    /// each by name, address and root key fingerprint with a switch, Remove for the user's (the
    /// official one and the device's can be turned off, not removed), and Add a repository...,
    /// which trusts a root key only when the user says so. The signing keys and the MIDI
    /// controllers are the app's pages.
    class Settings : public juce::Component
    {
    public:
        explicit Settings(Engine& engine);
        void refresh(); // reads the repositories again
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
        juce::Rectangle<int> reposHeading_, reposArea_;
        std::vector<Control::Repository> repositories_;
        std::vector<std::unique_ptr<juce::ToggleButton>> switches_;
        std::vector<std::unique_ptr<juce::TextButton>> removes_; // null where it cannot be removed
        juce::TextButton addRepository_{"Add a repository..."};
        std::unique_ptr<juce::FileChooser> chooser_;
        void readRepositories();
        void listThenAdd(const std::string& address, bool trusted); // trusted: its key was just trusted by the user
    };
}
