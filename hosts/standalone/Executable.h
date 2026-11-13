// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// This program's own file, which its helpers (oscairn-engine, oscairn-plugin-scan), its themes
// and its plugins are found beside. JUCE's currentExecutableFile on Linux is argv[0] as given,
// taken against the working directory: started by its bare name through PATH ("Oscairn"), it is
// a file in the working directory that is not there. Linux's own record of it is used there.

#include <juce_core/juce_core.h>
#ifdef __linux__
#include <unistd.h>
#endif

namespace oscairn::standalone
{
    inline juce::File executableFile()
    {
#ifdef __linux__
        char path[4096];
        if(const auto n = ::readlink("/proc/self/exe", path, sizeof path - 1); n > 0 && path[0] == '/')
            return juce::File(juce::String::fromUTF8(path, int(n)));
#endif
        return juce::File::getSpecialLocation(juce::File::currentExecutableFile);
    }
}
