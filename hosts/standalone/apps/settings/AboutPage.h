// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Frame.h"

namespace oscairn::standalone::settings
{
    /// About (RFC 0038): Oscairn's version (the release, OSCAIRN_RELEASE from CMake), the commit
    /// its source is (a release build's; a development build says so), the engine's version (it
    /// can be another build's, with --connect: packages.preinstalled's release, read off the
    /// message thread) and the license, with where its texts are.
    class AboutPage : public Page
    {
    public:
        explicit AboutPage(Engine& engine);
        juce::Label version, source, engineVersion, license;
        bool read() const { return read_; } // the engine's version has arrived, or failed to

    private:
        bool read_ = false;
    };
}
