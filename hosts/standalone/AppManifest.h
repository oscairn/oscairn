// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <juce_core/juce_core.h>
#include <string>
#include <vector>

namespace oscairn::standalone
{
    /// An app's manifest (RFC 0022): app.json in its folder, read so the shell knows an app
    /// without starting it. Only what steps 2 and 4 use is read; the rest of the format is accepted.
    struct AppManifest
    {
        std::string id, name, version;
        juce::File folder, executable;            // this platform's build; none: it does not run here
        struct Offer { std::string action, name; };
        std::vector<Offer> offers;
        std::vector<std::string> asks;
        uint64_t reserve = 0, maximum = 0;        // memory, MiB; maximum 0: none declared
        std::vector<std::string> permissions;     // what the engine lets its connection do
    };
    AppManifest readAppManifest(const juce::File& folder); // throws std::runtime_error saying what is wrong
    std::vector<AppManifest> findApps(const juce::File& folder); // its folders holding an app.json; one that cannot be read is left out, said on stderr
    // The apps that run as processes: Apps/ beside this program, else share/oscairn/apps.
    juce::File appsFolder();
}
