// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Frame.h"
#include <memory>

namespace oscairn::standalone::settings
{
    /// Package repositories (RFC 0031): each by name, address and root key fingerprint, with a
    /// switch (packages.enableRepository) and Remove for the user's own (packages.removeRepository;
    /// the official and the device's can be turned off, never removed). "Add a repository..."
    /// takes an https:// address alone: the engine lists it (packages.list, then packages.job
    /// until it ends, all off the message thread in one Engine::job, not the old dialog's poll
    /// there), its root key's fingerprint and the name that key signed are shown, and it is
    /// added (packages.addRepository) only once the user trusts that key; an address that is not
    /// https:// is refused before the engine is asked, since an app may name no folder on the
    /// engine's filesystem — a file goes through the Library's Install... > File. A failure is
    /// said in the danger color under what it was about, and what is kept is listed again after
    /// a change.
    class RepositoriesPage : public Page
    {
    public:
        explicit RepositoriesPage(Engine& engine);
        // A repository's controls, side by side as its row's one control.
        struct RowControls : juce::Component
        {
            RowControls();
            juce::ToggleButton on{"On"};
            juce::TextButton remove{"Remove"}; // hidden where it cannot be removed
            void resized() override;
        };
        // The https:// address, with Add and Cancel.
        struct AskControls : juce::Component
        {
            AskControls();
            juce::TextEditor address;
            juce::TextButton add{"Add"}, cancel{"Cancel"};
            void resized() override;
        };
        juce::TextButton addRepository{"Add a repository..."};
        AskControls ask;
        const std::vector<Control::Repository>& repositories() const { return repositories_; }
        RowControls& controls(size_t i) { return *controls_[i]; }         // in repositories()'s order
        const Row& repositoryRow(size_t i) const { return *rows_[i]; }
        const Row& buttonRow() const { return *buttonRow_; }              // Add, and where its refusals are said
        const Row& askRow() const { return *askRow_; }                    // the address, shown while adding
        bool adding() const { return asking_; }
        bool busy() const { return busy_; }                               // a listing or a change is out
        int listings() const { return listings_; }                        // how many listings have arrived
        void list();                                                      // read the repositories again
        void openRepository(const juce::String& address);                 // https:// alone; any other refused before the engine is asked

    private:
        void show();
        void listThenAdd(const std::string& address, bool trusted);       // trusted: its key was just trusted by the user
        Engine& engine_;
        std::vector<Control::Repository> repositories_;
        std::vector<std::unique_ptr<RowControls>> controls_;
        std::vector<Row*> rows_;                                          // the repository rows
        Row *buttonRow_ = nullptr, *askRow_ = nullptr;                    // one shows at a time: the button, or the address
        bool busy_ = false, asking_ = false;
        int listings_ = 0;
        std::string refused_;                                             // the address a refusal is about; empty: the Add row's own
        juce::String refusal_, progress_, listError_;
    };
}
