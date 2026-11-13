// SPDX-License-Identifier: AGPL-3.0-or-later
// Before any JUCE header: JUCE's X11 declarations, for the window manager's hints.
#if defined(__linux__) || defined(__FreeBSD__)
 #define JUCE_GUI_BASICS_INCLUDE_XHEADERS 1
#endif
#include "DeviceScreen.h"

namespace oscairn::standalone
{
#if JUCE_LINUX || JUCE_BSD
    namespace
    {
        // Undecorated, and full screen: in the window's state before it is mapped (EWMH), or
        // asked of the window manager on the root window once it is. Without one, nothing reads either.
        void askFullScreen(::Window window, bool mapped)
        {
            auto* display = juce::XWindowSystem::getInstance()->getDisplay();
            if(display == nullptr || window == 0) return;
            auto& x = *juce::X11Symbols::getInstance();
            juce::XWindowSystemUtilities::ScopedXLock lock;
            const auto motif = x.xInternAtom(display, "_MOTIF_WM_HINTS", False);
            const long hints[5]{ 2, 0, 0, 0, 0 }; // MWM_HINTS_DECORATIONS: none
            x.xChangeProperty(display, window, motif, motif, 32, PropModeReplace, reinterpret_cast<const unsigned char*>(hints), 5);
            const auto state = x.xInternAtom(display, "_NET_WM_STATE", False), full = x.xInternAtom(display, "_NET_WM_STATE_FULLSCREEN", False);
            if(!mapped)
            {
                const long atoms[1]{ long(full) };
                x.xChangeProperty(display, window, state, XA_ATOM, 32, PropModeReplace, reinterpret_cast<const unsigned char*>(atoms), 1);
            }
            else
            {
                XEvent event{};
                event.xclient.type = ClientMessage; event.xclient.window = window; event.xclient.message_type = state; event.xclient.format = 32;
                event.xclient.data.l[0] = 1; event.xclient.data.l[1] = long(full); event.xclient.data.l[3] = 1; // add; from an application
                const auto root = x.xRootWindow(display, x.xDefaultScreen(display));
                x.xSendEvent(display, root, False, SubstructureRedirectMask | SubstructureNotifyMask, &event);
            }
            x.xFlush(display);
        }
    }
#endif

    void fillScreen(juce::Component& window)
    {
        window.getProperties().set("oscairnFullScreen", true);
        if(const auto* display = juce::Desktop::getInstance().getDisplays().getPrimaryDisplay())
            window.setBounds(display->logicalBounds.toNearestInt());
#if JUCE_LINUX || JUCE_BSD
        if(auto* peer = window.getPeer(); peer && window.isOnDesktop())
        {
            const auto handle = ::Window(reinterpret_cast<uintptr_t>(peer->getNativeHandle()));
            askFullScreen(handle, window.isVisible());
            // Shown next (the caller maps it): asked again then, as a mapped window must be.
            juce::MessageManager::callAsync([safe = juce::Component::SafePointer<juce::Component>(&window), handle] {
                if(safe && safe->isOnDesktop() && safe->getPeer() && safe->getPeer()->getNativeHandle() == reinterpret_cast<void*>(handle)) askFullScreen(handle, true);
            });
        }
#endif
    }

    bool fillsScreen(const juce::Component& window) { return window.getProperties().contains("oscairnFullScreen"); }
}
