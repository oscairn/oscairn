// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Profile.h"
#include "oscairn/control.h"
#include <juce_core/juce_core.h>

namespace oscairn::standalone
{
    // The session's setup that holds whichever UI runs (docs/rust-ui.md, "Session setup"), from the
    // device profile and the user's settings: the budget and memory classes (RFC 0025), the
    // always-available apps, the device's package repositories (RFC 0031). The engine applies it
    // when it starts with the profile (--device-profile, the start script's engine); the JUCE app
    // applies it to an engine it starts, which has none. `shell`: the applying UI's own resident
    // memory, sent as memory.shell, which each UI sends for itself; 0 sends none (the engine's own).
    void setUpSession(Control& c, const Profile& profile, uint64_t shell = 0);
    /// The retroactive buffer's length (RFC 0020): what was saved in the configuration folder, or
    /// the device's default, its profile's captureSeconds, else 2 minutes with 2 GB or less and 10
    /// minutes above. 0 is off: no buffer is allocated.
    double readCaptureSeconds(const juce::File& configuration, const Profile& profile);
    void writeCaptureSeconds(const juce::File& configuration, double seconds);
    // The user's configuration folder: OSCAIRN_CONFIG_HOME, else the platform's (~/.config/Oscairn,
    // ~/Library/Application Support/Oscairn).
    juce::File configurationFolder();
}
