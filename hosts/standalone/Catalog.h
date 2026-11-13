// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Engine.h"
#include "Theme.h"
#include <juce_gui_basics/juce_gui_basics.h>
#include <atomic>
#include <future>
#include <map>
#include <optional>

namespace oscairn::standalone
{
    /// One package a repository offers, as the Catalog lists it: where from, and the version this
    /// device would install (the newest stable one it can, not yanked), else the newest, with why;
    /// and the version installed here, if it is, offered as an update when `version` is newer.
    /// Installed at a version the repository lists but not as its build (another archive, by the
    /// SHA-256 recorded at install, or a different build kept): otherBuild, with `version` that
    /// version, offered as Install the repository's build. A record with no SHA-256 (from before
    /// installs kept one) tells nothing, so it counts as installed.
    struct Offer
    {
        std::string source, repository; // the repository's address and this device's name for it (never the name its root claims)
        Control::RepositoryPackage package;
        Control::RepositoryVersion version;
        bool installable = false;
        std::optional<std::string> installed; // the version installed here, empty when not known
        bool update = false;                  // installed, and `version` installable and newer
        bool otherBuild = false;              // installed at `version`, but not this repository's build
        bool builtIn = false;                 // not installed, but its plugins come with the app, `installed` their version
        Control::InstalledPackage record;     // what is installed, as packages.installed says
        std::string with;                     // an asset library installed only as a dependency: the installed package listed as needing it, by name
        bool holds(const std::string& kind) const; // "performance", "sound", "plugin" or "asset"
        bool offered() const { return installable && (!installed || update || otherBuild); } // Install, Update, or the repository's build
    };
    /// The offers of these listings, each once per repository (`installed`: the newest version
    /// installed of each package, by ID). A plugin package not installed whose plugins all come
    /// with the app (`bundled`: their versions, by plugin ID) is built in, at the oldest of their
    /// versions: shown as Built in, or offered as Update to a newer version the repository has,
    /// which installs beside the app's (RFC 0031: plugins install side by side; nothing moves on
    /// its own). An asset library installed only as a dependency names the installed package
    /// whose listed versions need it (`with`).
    std::vector<Offer> offers(const std::vector<std::pair<std::string, Control::PackageJob>>& listings, const std::map<std::string, Control::InstalledPackage>& installed,
                              const std::map<std::string, std::string>& bundled = {});
    /// A row's second line: its state (Update to, Built in, Installed, Installed with <the package
    /// needing it>, Installed (a different build), or why it cannot be installed here, a withdrawn version included), then its publisher
    /// and summary; for what is installed and withdrawn since (packages.installed's withdrawn), who
    /// withdrew it and why, and Update to when there is a newer version.
    juce::String describe(const Offer& offer);
    /// "<name> <version> was withdrawn by <repository>: <reason>" for what is installed and
    /// withdrawn since (RFC 0031), else empty. Shown in the Catalog only: nothing interrupts, and
    /// nothing is removed or disabled; its details say how to replace or remove it.
    juce::String withdrawnSaying(const Offer& offer);
    /// The question before installing the repository's build over a different one: both builds,
    /// each by its archive's SHA-256 and signer, where known.
    juce::String replaceQuestion(const Offer& offer);
    /// The question when an install is refused for plugin code this device does not trust (RFC
    /// 0006, RFC 0031), told apart by each build's signer in the refusal (`installed.code`):
    /// every one signed by `publisherKey`, the key the repository's signed index names as the
    /// publisher's (vouched): Trust and install, trusting it for plugins, or Cancel; one signed by a
    /// key no one vouches for: Trust this key, Install anyway (this once) or Cancel; one unsigned:
    /// Install anyway or Cancel. Keys by their fingerprint. trust and anyway: the result of the
    /// button that does it, -1 when there is none; keys: the public keys trusting trusts.
    struct CodeQuestion { juce::String title, message; juce::StringArray buttons; int trust = -1, anyway = -1; std::vector<std::string> keys; };
    CodeQuestion codeQuestion(const Control::PackageJob& refused, const std::string& publisher, const std::string& publisherKey);

    /// The Catalog (RFC 0031), an interim view in the Library until the Catalog app
    /// (oscairn.catalog) exists: the enabled repositories' packages holding Performances, Sounds,
    /// plugins or content (asset libraries: samples, cartridges, patches; each read as packages.list reads it: the root, and the index
    /// kept from last time while it is unchanged), searched by
    /// name, summary, tag or publisher. What is installed says so, with its version, or offers
    /// Update to a newer one this device can install; one whose plugins come with the app says
    /// Built in, the same way; a different build at the version listed
    /// says so, offering Install the repository's build. What this device cannot install is shown
    /// grayed with the reason. Opening one shows its details: name, summary, version, what is
    /// installed, license, publisher and key fingerprint, repository, sizes and the packages it
    /// needs, with Install, Update, or neither when what is installed is current.
    class Catalog : public juce::Component, private juce::ListBoxModel
    {
    public:
        // scanner: the plugin scanner, to find the plugins that come with the app (the engine's
        // plugin folder) and their versions; none: none are told apart.
        Catalog(Engine& engine, const std::string& kind, std::map<std::string, Control::InstalledPackage> installed, std::string scanner = {});
        ~Catalog() override { canceled_ = true; } // a scan still going stops
        // Reads each enabled repository in turn, one engine job at a time, the app's plugins
        // scanned meanwhile. Opened on a tab with nothing to show, it then shows the first that has.
        void read();
        const std::map<std::string, std::string>& bundled() const { return bundled_; } // the app's plugins found, by ID: their versions
        void showKind(const std::string& kind);
        void open(int row);  // its details
        void back();         // to the list
        const std::vector<Offer>& shown() const { return shown_; }
        const Offer* opened() const { return opened_ ? &*opened_ : nullptr; }
        const juce::String& status() const { return status_; }
        bool reading() const { return reading_; }
        // Why nothing is listed, once read: no repository on, none read, a search, or a tab with
        // nothing, pointing to the tabs that have something; empty while something is listed.
        juce::String emptySaying() const;
        std::function<void(const Offer&)> onInstall;
        juce::TextEditor search;
        juce::TextButton performances{"Performances"}, sounds{"Sounds"}, plugins{"Plugins"}, content{"Content"}, close{"Close"}, backButton{"Back"}, install{"Install"};
        juce::ListBox list{"Catalog", this};
        std::vector<std::pair<juce::String, juce::String>> facts() const; // the opened package's details, each named
        void paint(juce::Graphics&) override;
        void paintOverChildren(juce::Graphics&) override; // why nothing is listed, over the list
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
        std::map<std::string, Control::InstalledPackage> installed_;
        std::string scanner_;
        std::atomic<bool> canceled_ = false; // before scan_, which waits for the scan when it goes
        std::future<std::map<std::string, std::string>> scan_;
        std::map<std::string, std::string> bundled_;
        std::vector<std::pair<std::string, Control::PackageJob>> listings_;
        std::vector<Offer> all_, shown_;
        std::optional<Offer> opened_;
        juce::StringArray read_, failed_;
        juce::String status_;
        bool reading_ = false, noneOn_ = false;
        juce::Rectangle<int> headingArea_, statusArea_;
    };
}
