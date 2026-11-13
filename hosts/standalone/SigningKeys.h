// SPDX-License-Identifier: AGPL-3.0-or-later
// Signing keys, shared by the Library, the Catalog and the Settings app's page (RFC 0006, RFC
// 0022): all ask the same way, and inline here keeps it one copy (as Icons.h does).
#pragma once
#include "Engine.h"
#include <juce_gui_basics/juce_gui_basics.h>

namespace oscairn::standalone
{
    inline juce::String keyText(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }

    /// A key's ID as people read it (RFC 0006's fingerprint): its 16 hex digits in groups of four.
    inline juce::String fingerprint(const std::string& id)
    {
        juce::String out;
        for(size_t i = 0; i < id.size(); i += 4) out << (i ? " " : "") << keyText(id.substr(i, 4));
        return out;
    }

    /// Asks for a new signing key's name and makes it (RFC 0006); `made` gets its name once it is
    /// made, and an error is shown instead if it is not.
    inline void askToMakeKey(Engine& engine, juce::Component* over, std::function<void(const std::string&)> made)
    {
        auto* w = new juce::AlertWindow("Make a signing key", "Its name, which only you see: letters, digits, '.', '-' and '_'. The private key stays on this device.", juce::MessageBoxIconType::NoIcon, over);
        w->addTextEditor("name", "", "Name");
        w->addButton("Make", 1, juce::KeyPress(juce::KeyPress::returnKey)); w->addButton("Cancel", 0, juce::KeyPress(juce::KeyPress::escapeKey));
        keepInScreen(*w);
        w->enterModalState(true, juce::ModalCallbackFunction::create([&engine, w, then = std::move(made)](int result) {
            if(result != 1) return;
            Control::SigningKey key;
            if(const auto error = engine.makeSigningKey(w->getTextEditorContents("name").trim().toStdString(), key); !error.empty())
            {
                juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon, "No key made", keyText(error));
                return;
            }
            if(then) then(key.name);
        }), true);
    }
}
