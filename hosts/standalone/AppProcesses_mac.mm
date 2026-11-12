// SPDX-License-Identifier: AGPL-3.0-or-later
#include "AppProcesses.h"
#import <AppKit/AppKit.h>

namespace oscairn::standalone
{
    // macOS brings another process's window forward only when that process is the active app,
    // and since macOS 14 activation passes only from the active app to another, cooperatively.
    void activateApp(pid_t pid)
    {
        NSRunningApplication* app = [NSRunningApplication runningApplicationWithProcessIdentifier: pid];
        if(app == nil) return;
        if(@available(macOS 14, *))
        {
            [NSApp yieldActivationToApplication: app];
            [app activateWithOptions: 0];
        }
        else
        {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations" // the way before macOS 14
            [app activateWithOptions: NSApplicationActivateIgnoringOtherApps];
#pragma clang diagnostic pop
        }
    }
}
