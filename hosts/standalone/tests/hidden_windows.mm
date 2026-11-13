// SPDX-License-Identifier: AGPL-3.0-or-later
//
// macOS: loaded into the tests that open windows (DYLD_INSERT_LIBRARIES, which the apps they
// start inherit), so no test puts a window on the screen of whoever runs them. Every window is
// fully transparent and lets clicks through, and the process never makes itself or another app
// active, so it takes no keys either. The windows are still there for the tests: their peers,
// sizes, focus within the process and rendering are unchanged.
#import <AppKit/AppKit.h>
#import <objc/runtime.h>

namespace
{
    template<typename F> F replace(Class c, SEL selector, id block)
    {
        return reinterpret_cast<F>(method_setImplementation(class_getInstanceMethod(c, selector), imp_implementationWithBlock(block)));
    }
    void (*setAlpha)(id, SEL, CGFloat);
    void (*setIgnoresMouse)(id, SEL, BOOL);
    void (*order)(id, SEL, NSWindowOrderingMode, NSInteger);
    void (*setPolicy)(id, SEL, NSApplicationActivationPolicy);

    __attribute__((constructor)) void hideWindows()
    {
        setAlpha = replace<decltype(setAlpha)>([NSWindow class], @selector(setAlphaValue:), ^(NSWindow* w, CGFloat) { setAlpha(w, @selector(setAlphaValue:), 0); });
        setIgnoresMouse = replace<decltype(setIgnoresMouse)>([NSWindow class], @selector(setIgnoresMouseEvents:), ^(NSWindow* w, BOOL) { setIgnoresMouse(w, @selector(setIgnoresMouseEvents:), YES); });
        // orderFront:, makeKeyAndOrderFront: and the rest order a window through here.
        order = replace<decltype(order)>([NSWindow class], @selector(orderWindow:relativeTo:), ^(NSWindow* w, NSWindowOrderingMode place, NSInteger other) {
            setAlpha(w, @selector(setAlphaValue:), 0);
            setIgnoresMouse(w, @selector(setIgnoresMouseEvents:), YES);
            order(w, @selector(orderWindow:relativeTo:), place, other);
        });
        // No Dock icon, and activation does nothing.
        setPolicy = replace<decltype(setPolicy)>([NSApplication class], @selector(setActivationPolicy:), ^BOOL(NSApplication* a, NSApplicationActivationPolicy) {
            reinterpret_cast<BOOL (*)(id, SEL, NSApplicationActivationPolicy)>(setPolicy)(a, @selector(setActivationPolicy:), NSApplicationActivationPolicyAccessory);
            return YES;
        });
        replace<IMP>([NSApplication class], @selector(activateIgnoringOtherApps:), ^(NSApplication*, BOOL) {});
        if(class_getInstanceMethod([NSApplication class], @selector(activate))) replace<IMP>([NSApplication class], @selector(activate), ^(NSApplication*) {});
        replace<IMP>([NSRunningApplication class], @selector(activateWithOptions:), ^BOOL(NSRunningApplication*, NSApplicationActivationOptions) { return NO; });
        // The window server finds no window that takes no clicks: this process's own windows are
        // found first, front to back, so what a test hit-tests through a window still works.
        static NSInteger (*windowAt)(id, SEL, NSPoint, NSInteger);
        windowAt = reinterpret_cast<decltype(windowAt)>(method_setImplementation(class_getClassMethod([NSWindow class], @selector(windowNumberAtPoint:belowWindowWithWindowNumber:)),
            imp_implementationWithBlock(^NSInteger(id c, NSPoint p, NSInteger below) {
                if(below == 0)
                    for(NSWindow* w in [NSApp orderedWindows])
                        if(w.isVisible && NSPointInRect(p, w.frame)) return w.windowNumber;
                return windowAt(c, @selector(windowNumberAtPoint:belowWindowWithWindowNumber:), p, below);
            })));
    }
}
