// SPDX-License-Identifier: AGPL-3.0-or-later
// Before any JUCE header: JUCE's X11 declarations, for the window manager's hints.
#if defined(__linux__) || defined(__FreeBSD__)
 #define JUCE_GUI_BASICS_INCLUDE_XHEADERS 1
#endif
#include "DeviceScreen.h"

namespace oscairn::standalone
{
    void setDeviceScreen(juce::Component* window, int touch); // Theme.h, whose names X11's macros (None) break
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

    void placeAppWindow(juce::DocumentWindow& window, const juce::var& how)
    {
        if(!bool(how.getProperty("visible", true))) { window.setVisible(false); return; }
        if(const auto* b = how["bounds"].getArray(); b && b->size() == 4)
        {
            // The shell's scale, so its coordinates and sizes are this window's.
            if(const auto scale = float(double(how.getProperty("scale", 1.0))); scale > 0 && scale != juce::Desktop::getInstance().getGlobalScaleFactor())
                juce::Desktop::getInstance().setGlobalScaleFactor(scale);
            window.setUsingNativeTitleBar(false); window.setTitleBarHeight(0); window.setResizable(false, false);
#if JUCE_LINUX || JUCE_BSD
            // Inside the shell's window, its bounds in the shell's: a child X window, which a
            // window manager neither moves nor decorates nor puts behind the shell's.
            if(const auto parent = juce::int64(how.getProperty("parent", 0)); parent != 0 && juce::int64(window.getProperties()["oscairnParent"]) != parent)
            {
                // Its own style, unchanged: JUCE adds a window to the desktop again (no parent) when its style changes.
                window.removeFromDesktop();
                window.addToDesktop(window.getDesktopWindowStyleFlags(), reinterpret_cast<void*>(uintptr_t(parent)));
                window.getProperties().set("oscairnParent", parent);
                setDeviceScreen(&window, 44); // its dialogs and menus inside it too, at the touch target (Profile::Screen::touchTarget)
            }
#endif
            capFrameRate(); // as the shell's: JUCE read the displays again for the scale
            window.setBounds(int((*b)[0]), int((*b)[1]), int((*b)[2]), int((*b)[3]));
        }
        window.setVisible(true); window.toFront(true);
    }

    void capFrameRate(double hz)
    {
        // Desktop's own list, which it owns (not const): its windows' paint timers read it.
        auto& displays = const_cast<juce::Displays&>(juce::Desktop::getInstance().getDisplays());
        for(auto& d : displays.displays) d.verticalFrequencyHz = std::min(d.verticalFrequencyHz.value_or(hz), hz);
    }
}
