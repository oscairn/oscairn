// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// Where the packages that come with a release are (RFC 0031): a repository snapshot in the app's
// Resources, beside Contents/MacOS (macOS), or in Preinstalled/ beside the programs, in bin/ (Linux).

#include "../standalone/Executable.h"

namespace oscairn::host
{
    inline juce::File preinstalledFolder( const juce::File& executable )
    {
        const auto here = executable.getParentDirectory();
        const auto shipped = here.getSiblingFile( "Resources" ).getChildFile( "Preinstalled" );
        return shipped.isDirectory() ? shipped : here.getChildFile( "Preinstalled" );
    }
}
