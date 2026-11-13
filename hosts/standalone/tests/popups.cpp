// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Popups on a device's screen (RFC 0019), made as the app's window makes them: a window on the
// desktop at the profile's scale (Main.cpp), dense, touch, the device screen that menus, alerts and
// combo lists open inside, over a board laid out in design units and scaled to the window as the
// Panel's is. Measured in physical pixels at the FANTOM-07's 1280 x 720, where the window's units
// are the profile's scale over the display's own (on X11 JUCE's round(dpi / 96), here PLATFORM):
// a menu item, an alert's buttons and a combo's list rows are one touch target (44 units, 68 px)
// tall, their text the Body size, as the dense UI's own, in submenus too. With SNAPSHOTS (or
// OSCAIRN_TEST_SNAPSHOTS in the environment) each is written, as drawn at 1280 x 720, to
// SNAPSHOTS-<name>.png.
//   oscairn_popups_test THEMES [SNAPSHOTS]
#include "../Profile.h"
#include "../Theme.h"
#include <cstdio>

using namespace oscairn;

namespace
{
    constexpr double PLATFORM = 3.0; // the display's own scale: what X11 JUCE makes of a 7-inch panel's 1280 x 720
}

int main(int argc, char** argv)
{
    if(argc < 2) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { std::fprintf(stderr, "%s: %s\n", ok ? "ok" : "FAIL", what.c_str()); if(!ok) ++failures; };
    const auto configuration = juce::File::createTempFile("oscairn-popups-config");
    standalone::Theme theme(configuration, juce::File(argv[1]));
    juce::LookAndFeel::setDefaultLookAndFeel(&theme);
    theme.apply("oscairn-block");
    const auto fantom = standalone::loadProfile("fantom-07");
    const auto* s = fantom.screen();
    const int touch = standalone::Profile::Screen::touchTarget;
    const int designWidth = juce::roundToInt(s->width / s->scale()), designHeight = juce::roundToInt(s->height / s->scale());

    // As Main.cpp makes the device's window (there, the display's own scale is read; here it is PLATFORM).
    standalone::setDeviceScale(s->scale(), PLATFORM);
    const double pixelsPerUnit = juce::Desktop::getInstance().getGlobalScaleFactor() * PLATFORM; // one unit of the window, in physical pixels
    juce::DocumentWindow screen("screen", standalone::ink(standalone::Ink::Background), 0);
    screen.setUsingNativeTitleBar(false); screen.setTitleBarHeight(0);
    standalone::setDeviceScreen(&screen, touch);
    standalone::setDense(screen);
    screen.setSize(juce::roundToInt(s->width / pixelsPerUnit), juce::roundToInt(s->height / pixelsPerUnit));
    // The Panel's board: design units, scaled to the window.
    juce::Component board;
    standalone::setDesktopMode(board, 2);
    board.setSize(designWidth, designHeight);
    board.setTransform(juce::AffineTransform::scale(screen.getWidth() / float(designWidth)));
    screen.addAndMakeVisible(board);
    juce::TextButton libraries("Libraries..."), audition("Audition"); board.addAndMakeVisible(libraries); board.addAndMakeVisible(audition);
    libraries.setBounds(600, 420, 140, touch); audition.setBounds(300, 420, 140, touch);
    juce::ComboBox combo; combo.addItemList({"First choice", "Second choice", "Third choice"}, 1); combo.setSelectedId(1); board.addAndMakeVisible(combo); combo.setBounds(20, 100, 220, touch);
    screen.setVisible(true);

    auto physical = [&](juce::Component& c) { return screen.getLocalArea(&c, c.getLocalBounds().toFloat()) * float(pixelsPerUnit); };
    const int target = juce::roundToInt(touch * s->scale()); // a touch target on the board, in pixels: 68
    const float body = standalone::textSize(standalone::TextRole::Body, 2) * s->scale(); // the Body size on the board, in pixels
    std::fprintf(stderr, "window %d x %d units at %.3f px per unit; board scale %.3f; a touch target %d px, Body text %.1f px\n", screen.getWidth(), screen.getHeight(),
                 pixelsPerUnit, double(board.getTransform().mat00), target, double(body));
    check(std::abs(physical(audition).getHeight() - target) < 1.5f, "the board's own button is a touch target tall: " + std::to_string(physical(audition).getHeight()) + " px");
    auto shot = [&](const juce::String& name) {
        const auto prefix = argc >= 3 ? juce::String(argv[2]) : juce::SystemStats::getEnvironmentVariable("OSCAIRN_TEST_SNAPSHOTS", {});
        if(prefix.isEmpty()) return;
        const juce::File out{prefix + "-" + name + ".png"}; out.deleteFile();
        juce::FileOutputStream stream(out);
        juce::PNGImageFormat().writeImageToStream(screen.createComponentSnapshot(screen.getLocalBounds(), true, float(pixelsPerUnit)), stream);
    };
    // A menu (or a submenu) opened inside the screen; its items, as the pixels they are drawn in.
    // Looked at before the loop runs: a menu in a window that is not the active one dismisses itself.
    auto menuWindows = [&] { std::vector<juce::Component*> found; for(auto* c : screen.getChildren()) if(c->getName() == "menu" && c->isVisible()) found.push_back(c); return found; };
    auto itemRows = [&](juce::Component* menu) { std::vector<float> h; for(auto* c : menu->getChildren()) if(c->isVisible() && c->getHeight() > 0) h.push_back(physical(*c).getHeight()); return h; };
    auto checkMenu = [&](const std::string& what, juce::Component* menu) {
        if(!menu) { check(false, what + " opens inside the screen"); return; }
        std::string rows; int items = 0; bool right = true;
        for(float h : itemRows(menu)) { rows += " " + std::to_string(int(h)); if(h > target / 2.0f) { ++items; right = right && std::abs(h - target) < 1.5f; } }
        check(items > 0 && right, what + ": each item one touch target (" + std::to_string(target) + " px) tall; heights in px:" + rows);
    };
    auto fontPixels = [&](const juce::Font& f) { return f.getHeight() * float(pixelsPerUnit); }; // a popup's font: window units, in physical pixels
    check(std::abs(fontPixels(theme.getPopupMenuFont()) - body) < 1.0f, "a menu's text is the Body size: " + std::to_string(fontPixels(theme.getPopupMenuFont())) + " px of " + std::to_string(body));

    // The Library's Libraries menu, and one of its submenus.
    {
        juce::PopupMenu menu, folders, hidden, plugins;
        menu.addItem(1, "Export library...");
        folders.addItem(2, "Index a folder in place...");
        folders.addItem(6, "Stop indexing /Users/jacob/Music");
        menu.addSubMenu("Folders indexed in place", folders);
        hidden.addItem(3, "tag");
        menu.addSubMenu("Show a hidden tag", hidden, false);
        plugins.addItem(4, "Basic Synth 1.0");
        menu.addSubMenu("Plugins", plugins);
        menu.addSeparator();
        menu.addItem(5, "Uninstall Jacob's Performances (FANTOM testing...");
        menu.showMenuAsync(juce::PopupMenu::Options().withTargetComponent(&libraries));
        auto windows = menuWindows();
        checkMenu("the Libraries menu", windows.empty() ? nullptr : windows[0]);
        if(!windows.empty())
        {
            const auto where = physical(*windows[0]);
            std::fprintf(stderr, "  the menu is %d x %d px at %d, %d of 1280 x 720\n", int(where.getWidth()), int(where.getHeight()), int(where.getX()), int(where.getY()));
            check(where.getWidth() < 1280 * .75f && where.getHeight() <= 720, "and fits the screen");
            shot("menu");
            // A submenu: asked to open as a pointer resting on its item does, through the item's own action.
            juce::Component* folder = nullptr; int seen = 0;
            for(auto* c : windows[0]->getChildren()) if(c->isVisible() && c->getHeight() > 20 && seen++ == 1) folder = c;
            if(folder && folder->getAccessibilityHandler()) folder->getAccessibilityHandler()->getActions().invoke(juce::AccessibilityActionType::showMenu);
            auto both = menuWindows();
            checkMenu("a submenu", both.size() >= 2 ? both.back() : nullptr);
            shot("submenu");
        }
        juce::PopupMenu::dismissAllActiveMenus();
    }
    // A context menu: no component it comes from, a point.
    {
        juce::PopupMenu menu, colors; colors.addItem(10, "Red"); colors.addItem(11, "Green");
        menu.addItem(7, "Rename..."); menu.addSubMenu("Color", colors); menu.addItem(8, "Delete...");
        menu.showMenuAsync(juce::PopupMenu::Options().withTargetScreenArea(screen.localAreaToGlobal(juce::Rectangle<int>(30, 30, 1, 1))));
        auto windows = menuWindows();
        checkMenu("a context menu", windows.empty() ? nullptr : windows[0]);
        shot("context-menu");
        juce::PopupMenu::dismissAllActiveMenus();
    }
    // A combo box's list.
    {
        combo.showPopup();
        auto windows = menuWindows();
        checkMenu("a combo box's list", windows.empty() ? nullptr : windows[0]);
        shot("combo-list");
        juce::PopupMenu::dismissAllActiveMenus();
    }
    // An alert: its text the Body size, its buttons a touch target.
    {
        juce::AlertWindow::showAsync(juce::MessageBoxOptions().withIconType(juce::MessageBoxIconType::QuestionIcon).withTitle("Delete Sunrise?")
                                         .withMessage("It is removed from your library. Performances that used it keep their parts.").withButton("Delete").withButton("Cancel")
                                         .withAssociatedComponent(&libraries), [](int) {});
        for(int i = 0; i < 20; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10);
        juce::AlertWindow* alert = nullptr;
        for(auto* c : screen.getChildren()) if(auto* a = dynamic_cast<juce::AlertWindow*>(c)) alert = a;
        check(alert != nullptr, "an alert opens inside the screen");
        if(alert)
        {
            std::string sizes; bool buttons = true; int count = 0;
            for(auto* c : alert->getChildren()) if(auto* b = dynamic_cast<juce::TextButton*>(c)) { ++count; const auto h = physical(*b).getHeight(); sizes += " " + std::to_string(int(h)); buttons = buttons && std::abs(h - target) < 1.5f; }
            check(count == 2 && buttons, "its buttons are a touch target tall, px:" + sizes);
            check(std::abs(fontPixels(theme.getAlertWindowMessageFont()) - body) < 1.0f, "its text is the Body size: " + std::to_string(fontPixels(theme.getAlertWindowMessageFont())) + " px of " + std::to_string(body));
            const auto where = physical(*alert);
            check(where.getWidth() <= 1280 && where.getHeight() <= 720, "and it fits the screen: " + std::to_string(int(where.getWidth())) + " x " + std::to_string(int(where.getHeight())) + " px");
            shot("alert");
            alert->exitModalState(0);
        }
    }
    // Main.cpp's call, reading the display's own scale: the display then draws the profile's scale, as it is (here, whatever this one is).
    standalone::setDeviceScale(s->scale());
    if(const auto* display = juce::Desktop::getInstance().getDisplays().getPrimaryDisplay())
        check(std::abs(display->scale - s->scale()) < 0.01, "on this display (scale " + std::to_string(display->scale / juce::Desktop::getInstance().getGlobalScaleFactor()) + " of its own) the device's scale is drawn: " + std::to_string(display->scale));
    standalone::setDeviceScreen(nullptr);
    juce::Desktop::getInstance().setGlobalScaleFactor(1.0f);
    return failures == 0 ? 0 : 1;
}
