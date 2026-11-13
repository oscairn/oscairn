// SPDX-License-Identifier: AGPL-3.0-or-later
//
// preinstalledFolder (PreinstalledFolder.h): Resources/Preinstalled beside the app's MacOS folder
// when there is one (macOS), otherwise Preinstalled beside the programs (the Linux tarball's bin/).
#include "PreinstalledFolder.h"

#include <cstdio>

static int failures = 0;
static void check( bool ok, const char* what ) { if( !ok ) { std::fprintf( stderr, "FAIL: %s\n", what ); ++failures; } }

int main()
{
    const auto root = juce::File::getSpecialLocation( juce::File::tempDirectory ).getNonexistentChildFile( "oscairn-preinstalled-folder", "" );

    // Linux: <root>/bin/Oscairn, the snapshot in <root>/bin/Preinstalled.
    const auto bin = root.getChildFile( "linux/bin" );
    bin.createDirectory();
    check( oscairn::host::preinstalledFolder( bin.getChildFile( "Oscairn" ) ) == bin.getChildFile( "Preinstalled" ), "Linux: Preinstalled beside the programs" );

    // macOS: Oscairn.app/Contents/MacOS/Oscairn, the snapshot in Contents/Resources/Preinstalled.
    const auto mac = root.getChildFile( "Oscairn.app/Contents" );
    mac.getChildFile( "MacOS" ).createDirectory();
    mac.getChildFile( "Resources/Preinstalled" ).createDirectory();
    check( oscairn::host::preinstalledFolder( mac.getChildFile( "MacOS/Oscairn" ) ) == mac.getChildFile( "Resources/Preinstalled" ), "macOS: Resources/Preinstalled" );

    root.deleteRecursively();
    if( failures == 0 ) std::fprintf( stderr, "ok\n" );
    return failures ? 1 : 0;
}
