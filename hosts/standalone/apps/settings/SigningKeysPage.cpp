// SPDX-License-Identifier: AGPL-3.0-or-later
#include "SigningKeysPage.h"
#include "../../SigningKeys.h"

namespace oscairn::standalone::settings
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
    }

    SigningKeysPage::SigningKeysPage(Engine& engine) : engine_(engine)
    {
        makeKey.setTooltip("A new key to sign exports with; its private half never leaves this device");
        makeKey.onClick = [this] { askToMakeKey(engine_, this, [safe = juce::Component::SafePointer<SigningKeysPage>(this)](const std::string&) { if(safe) safe->list(); }); };
        noKeys.setText("No keys yet", juce::dontSendNotification);
        noKeys.setColour(juce::Label::textColourId, ink(Ink::Muted));
        list();
    }

    void SigningKeysPage::list()
    {
        busy_ = true; makeKey.setEnabled(false);
        auto listed = std::make_shared<std::vector<Control::SigningKey>>();
        engine_.job([listed](Control& c) { *listed = c.signingKeys(); },
            [safe = juce::Component::SafePointer<SigningKeysPage>(this), listed](const std::string& error) {
                if(!safe) return;
                safe->busy_ = false; ++safe->listings_;
                safe->listError_ = text(error);
                if(error.empty()) safe->keys_ = *listed;
                safe->show();
            });
    }

    void SigningKeysPage::show()
    {
        clearRows(); rows_.clear();
        noKeys.setVisible(keys_.empty());
        for(size_t i = 0; i < keys_.size(); ++i)
        {
            if(prints.size() <= i)
            {
                auto print = std::make_unique<juce::Label>();
                print->setColour(juce::Label::textColourId, ink(Ink::Muted));
                prints.push_back(std::move(print));
            }
            prints[i]->setText(standalone::fingerprint(keys_[i].id), juce::dontSendNotification);
            rows_.push_back(&addRow(text(keys_[i].name), *prints[i]));
        }
        if(keys_.empty()) rows_.push_back(&addRow("Signing keys", noKeys));
        makeRow_ = &addRow("Make a signing key", makeKey, "Exports signed with one of these show others they came from you. Each key is shown by its fingerprint.");
        makeKey.setEnabled(!busy_);
        if(listError_.isNotEmpty()) makeRow_->setExplanation("The keys could not be read: " + listError_, true);
        heightChanged();
    }
}
