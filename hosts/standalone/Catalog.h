// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Engine.h"
#include "Theme.h"
#include <juce_gui_basics/juce_gui_basics.h>
#include <map>
#include <optional>

namespace oscairn::standalone
{
    /// One package a repository offers, as the Catalog lists it: where from, and the version this
    /// device would install (the newest stable one it can, not yanked), else the newest, with why;
    /// and the version installed here, if it is, offered as an update when `version` is newer.
    struct Offer
    {
        std::string source, repository; // the repository's address and name
        Control::RepositoryPackage package;
        Control::RepositoryVersion version;
        bool installable = false;
        std::optional<std::string> installed; // the version installed here, empty when not known
        bool update = false;                  // installed, and `version` installable and newer
        bool holds(const std::string& kind) const; // "performance", "sound" or "plugin"
        bool offered() const { return installable && (!installed || update); } // Install, or Update
    };
    /// The offers of these listings, each once per repository (`installed`: the newest version
    /// installed of each package, by ID).
    std::vector<Offer> offers(const std::vector<std::pair<std::string, Control::PackageJob>>& listings, const std::map<std::string, std::string>& installed);

    /// The Catalog (RFC 0031), an interim view in the Library until the Catalog app
    /// (oscairn.catalog) exists: the enabled repositories' packages holding Performances, Sounds
    /// or plugins (each read as packages.list reads it: the root, and the index
    /// kept from last time while it is unchanged), searched by
    /// name, summary, tag or publisher. What is installed says so, with its version, or offers
    /// Update to a newer one this device can install. What this device cannot install is shown
    /// grayed with the reason. Opening one shows its details: name, summary, version, what is
    /// installed, license, publisher and key fingerprint, repository, sizes and the packages it
    /// needs, with Install, Update, or neither when what is installed is current.
    class Catalog : public juce::Component, private juce::ListBoxModel
    {
    public:
        Catalog(Engine& engine, const std::string& kind, std::map<std::string, std::string> installed);
        // Reads each enabled repository in turn, one engine job at a time.
        void read();
        void showKind(const std::string& kind);
        void open(int row);  // its details
        void back();         // to the list
        const std::vector<Offer>& shown() const { return shown_; }
        const Offer* opened() const { return opened_ ? &*opened_ : nullptr; }
        const juce::String& status() const { return status_; }
        bool reading() const { return reading_; }
        std::function<void(const Offer&)> onInstall;
        juce::TextEditor search;
        juce::TextButton performances{"Performances"}, sounds{"Sounds"}, plugins{"Plugins"}, close{"Close"}, backButton{"Back"}, install{"Install"};
        juce::ListBox list{"Catalog", this};
        std::vector<std::pair<juce::String, juce::String>> facts() const; // the opened package's details, each named
        void paint(juce::Graphics&) override;
        void resized() override;

    private:
        int getNumRows() override { return int(shown_.size()); }
        void paintListBoxItem(int row, juce::Graphics&, int width, int height, bool selected) override;
        void listBoxItemClicked(int row, const juce::MouseEvent&) override { open(row); }
        void returnKeyPressed(int row) override { open(row); }
        void filter();
        void readNext(std::vector<Control::Repository> left);
        int mode() const { return std::max(1, desktopMode(*this)); }
        Engine& engine_;
        std::string kind_;
        std::map<std::string, std::string> installed_;
        std::vector<std::pair<std::string, Control::PackageJob>> listings_;
        std::vector<Offer> all_, shown_;
        std::optional<Offer> opened_;
        juce::StringArray read_, failed_;
        juce::String status_;
        bool reading_ = false;
        juce::Rectangle<int> headingArea_, statusArea_;
    };
}
