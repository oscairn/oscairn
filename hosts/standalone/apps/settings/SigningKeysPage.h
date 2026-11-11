// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Frame.h"
#include <memory>

namespace oscairn::standalone::settings
{
    /// Signing keys (RFC 0006): what signs exports, each shown by name and fingerprint (never the
    /// private key), or "No keys yet"; Make a signing key asks for a name and lists the keys again
    /// once one is made. The keys are read off the message thread (Engine::job) when the page
    /// opens and after a key is made; a listing that fails is said in the danger color under the
    /// button. The key itself is made by askToMakeKey (SigningKeys.h), the old dialog's dialog.
    class SigningKeysPage : public Page
    {
    public:
        explicit SigningKeysPage(Engine& engine);
        juce::TextButton makeKey{"Make a signing key"};
        juce::Label noKeys;                                       // shown in place of the list when there is none
        std::vector<std::unique_ptr<juce::Label>> prints;         // each key's fingerprint, in keys()'s order
        const std::vector<Control::SigningKey>& keys() const { return keys_; }
        const Row& keyRow(size_t i) const { return *rows_[i]; }   // key i's row, or "No keys yet"'s when there are none
        const Row& makeRow() const { return *makeRow_; }
        bool busy() const { return busy_; }                       // a listing is out
        int listings() const { return listings_; }                // how many listings have arrived
        void list();                                              // read the keys again

    private:
        void show();
        Engine& engine_;
        std::vector<Control::SigningKey> keys_;
        std::vector<Row*> rows_; // the key rows, then "No keys yet"'s; the button's is makeRow_
        Row* makeRow_ = nullptr;
        bool busy_ = false;
        int listings_ = 0;
        juce::String listError_;
    };
}
