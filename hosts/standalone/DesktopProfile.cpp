// SPDX-License-Identifier: AGPL-3.0-or-later
// The desktop's own profile, read from the computer and its display: the app's, apart from
// Profile.cpp, which the engine reads too and which needs no display.
#include "Profile.h"
#include <juce_gui_basics/juce_gui_basics.h>
#include <algorithm>
#include <stdexcept>
#include <thread>

namespace oscairn::standalone
{
    Profile Profile::desktop()
    {
        Profile p;
        p.id = "desktop"; p.name = "This computer";
#if JUCE_MAC
        p.platform.os = "macos";
#elif JUCE_WINDOWS
        p.platform.os = "windows";
#else
        p.platform.os = "linux";
#endif
#if JUCE_ARM
        p.platform.cpu = "arm64";
#else
        p.platform.cpu = "x86_64";
#endif
        p.compute.cores = int(std::max(1u, std::thread::hardware_concurrency()));
        p.compute.memory = juce::SystemStats::getMemorySizeInMegabytes();
        // The main display, if there is one yet (a test or a headless run may have none).
        Screen s;
        if(const auto* display = juce::Desktop::getInstance().getDisplays().getPrimaryDisplay())
        {
            s.width = display->userArea.getWidth(); s.height = display->userArea.getHeight();
            s.touch = juce::Desktop::getInstance().getMainMouseSource().isTouch();
        }
        else { s.width = 1440; s.height = 900; }
        p.screens.push_back(s);
        p.reliability = "desktop";
        p.apps = {"oscairn.performance", "oscairn.groovebox", "oscairn.library", "oscairn.setlist", "oscairn.recordings", "oscairn.settings"};
        p.alwaysAvailable = {"oscairn.performance", "oscairn.library"};
        return p;
    }

    Profile loadProfile(const std::string& nameOrFile)
    {
        if(nameOrFile.empty() || nameOrFile == "desktop") return Profile::desktop();
        auto p = readProfile(nameOrFile);
        if(p.controller()) throw std::invalid_argument("profile: " + p.id + " is a controller's profile: it joins the device's when connected");
        return p;
    }
}
