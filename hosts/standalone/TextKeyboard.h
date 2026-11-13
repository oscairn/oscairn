// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <juce_gui_basics/juce_gui_basics.h>

namespace oscairn::standalone
{
    // On-screen text entry (RFC 0019): on a screen with no typing keyboard (the profile's
    // `textEntry` is "onScreen", RFC 0024), a keyboard docks along the bottom of the window of
    // whichever editable juce::TextEditor gains focus, and goes when focus leaves it, on Done and
    // on Cancel. Keys type through the editor's own key handling, so its listeners, input filter,
    // length limit and password mask apply as to real typing; Done presses Return (the field's
    // onReturnKey and the dialog around it, as the Return key would) and Cancel puts back the text
    // the editor had when the keyboard came up. A line above the keys shows the editor's text and
    // caret, since the keyboard may cover the field. A dialog it covers is moved up while it shows.
    // US QWERTY only. Off until a process turns it on: every process that has text fields (the
    // shell, the Settings and Setlist apps) does so from its profile.
    void setTextEntryOnScreen(bool on);
    bool textEntryOnScreen();
    // The keyboard while it shows; null otherwise. Its keys are buttons named as they are said.
    juce::Component* textKeyboard();
}
