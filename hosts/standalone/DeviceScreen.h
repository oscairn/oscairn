// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <juce_gui_basics/juce_gui_basics.h>

namespace oscairn::standalone
{
    // A device's screen (RFC 0019, RFC 0024): `window` fills the display, at its origin at its
    // size, undecorated (_MOTIF_WM_HINTS) and asking the window manager for full screen
    // (_NET_WM_STATE_FULLSCREEN: set before the window is mapped, and asked again once it is), so
    // a window manager that places and decorates windows (Weston's for Xwayland, a desktop's)
    // shows it whole, and none (Oscairn's images) leaves it where it is put. The window follows
    // the size it is then given. X11 only; elsewhere, the bounds. fillsScreen: whether it was
    // asked, with or without a native window (--snapshot has none).
    void fillScreen(juce::Component& window);
    bool fillsScreen(const juce::Component& window);
}
