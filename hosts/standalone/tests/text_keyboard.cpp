// SPDX-License-Identifier: AGPL-3.0-or-later
//
// On-screen text entry (RFC 0019): on the FANTOM-07's screen, in design units, a text field's focus
// brings up a keyboard that types through the editor, in the Library and in an alert over the Setlist
// app's pane (modal); on a desktop profile it does not. Keys are pressed by mouse events sent to
// them. With SNAPSHOTS (or OSCAIRN_TEST_SNAPSHOTS in the environment), the screen with the keyboard up
// is written to SNAPSHOTS-<name>.png.
//   oscairn_text_keyboard_test PLUGIN_DIR THEMES [SNAPSHOTS]
#include "../Apps.h"
#include "../LibraryPane.h"
#include "../Profile.h"
#include "../Setlist.h"
#include "../TextKeyboard.h"
#include "../Theme.h"
#include "oscairn/local_socket.h"
#include "oscairn/session.h"
#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <thread>
#include <unistd.h>

using namespace oscairn;

int main(int argc, char** argv)
{
    if(argc < 3) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } };
    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-text-" + std::to_string(::getpid()) + ".sock");
    const auto data = juce::File::createTempFile("oscairn-text-data");
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    {
        auto session = std::make_unique<Session>(48000, 128);
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true;
        std::thread control([&] {
            std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                session->process(nullptr, 0, out, 2, 128);
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset();
        });
        {
            standalone::Engine engine(argv[1], socket.string());
            const auto configuration = juce::File::createTempFile("oscairn-text-config");
            standalone::Theme theme(configuration, juce::File(argv[2]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            theme.apply("oscairn-block");
            auto settle = [] { for(int i = 0; i < 20; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); };
            auto wait = [](auto done) { for(int i = 0; i < 300 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            auto snapshot = [&](juce::Component& c, const juce::String& name) {
                const auto prefix = argc >= 4 ? juce::String(argv[3]) : juce::SystemStats::getEnvironmentVariable("OSCAIRN_TEST_SNAPSHOTS", {});
                if(prefix.isEmpty()) return;
                const juce::File out{prefix + "-" + name + ".png"};
                out.deleteFile();
                juce::FileOutputStream stream(out);
                juce::PNGImageFormat().writeImageToStream(c.createComponentSnapshot(c.getLocalBounds()), stream);
            };
            auto named = [](auto&& self, juce::Component& in, const juce::String& name) -> juce::Component* {
                if(in.getName() == name) return &in;
                for(auto* c : in.getChildren()) if(auto* found = self(self, *c, name)) return found;
                return nullptr; };
            auto editorIn = [](auto&& self, juce::Component& in) -> juce::TextEditor* {
                if(auto* e = dynamic_cast<juce::TextEditor*>(&in)) return e;
                for(auto* c : in.getChildren()) if(auto* found = self(self, *c)) return found;
                return nullptr; };
            // A press and release, as the mouse sends them to the key.
            auto tap = [&](juce::Component* key) {
                if(!key) return false;
                auto mouse = juce::Desktop::getInstance().getMainMouseSource();
                const auto now = juce::Time::getCurrentTime();
                const auto p = key->getLocalBounds().getCentre().toFloat();
                const juce::MouseEvent down(mouse, p, juce::ModifierKeys::leftButtonModifier, 1, 0, 0, 0, 0, key, key, now, p, now, 1, false);
                key->mouseDown(down); key->mouseUp(down);
                settle();
                return true; };
            auto key = [&](const juce::String& name) { auto* kb = standalone::textKeyboard(); return kb ? named(named, *kb, name) : nullptr; };

            const auto fantom = standalone::loadProfile("fantom-07");
            const auto desktop = standalone::Profile::desktop();
            check(fantom.screen()->textOnScreen() && !desktop.screen()->textOnScreen(), "the FANTOM-07's screen types on screen; the desktop's does not");
            check(standalone::loadProfile("kick").screen()->textOnScreen() && standalone::loadProfile("ignite").screen()->textOnScreen() && standalone::loadProfile("modx-m").screen()->textOnScreen(), "so do the other touch screens");
            check(!standalone::loadProfile("imx8mm-evk").screen()->textOnScreen() && !standalone::loadProfile("qemu-aarch64").screen()->textOnScreen(), "and the ones without touch do not");

            const int touch = standalone::Profile::Screen::touchTarget;
            juce::DocumentWindow screen("screen", standalone::ink(standalone::Ink::Background), 0);
            screen.setUsingNativeTitleBar(false); screen.setTitleBarHeight(0);
            const auto* s = fantom.screen();
            const int screenWidth = juce::roundToInt(s->width / s->scale()), screenHeight = juce::roundToInt(s->height / s->scale()); // 1280 x 720 pixels at the device's scale, in design units
            standalone::setDeviceScreen(&screen, touch);
            standalone::setDense(screen);
            standalone::Actions actions;
            standalone::LibraryPane library(engine, actions, standalone::LibraryPane::Mode::Play);
            standalone::setDesktopMode(library, 2);
            screen.setContentNonOwned(&library, false); screen.setSize(screenWidth, screenHeight);
            screen.setVisible(true); screen.toFront(true);
            library.showKind("sound");
            check(wait([&] { return library.empty.isVisible(); }), "an empty Library to filter");
            auto* search = editorIn(editorIn, library);
            check(search != nullptr, "the Library has a search field");
            if(!search) return 1;

            // Off, as on the desktop: focusing the field shows nothing.
            standalone::setTextEntryOnScreen(desktop.screen()->textOnScreen());
            check(!standalone::textEntryOnScreen(), "a desktop profile leaves it off");
            search->grabKeyboardFocus(); settle();
            check(search->hasKeyboardFocus(false), std::string("the search field takes the focus (window ") + (screen.getPeer() && screen.getPeer()->isFocused() ? "active" : "not active") + ")");
            check(standalone::textKeyboard() == nullptr, "focused, no keyboard on a desktop profile");
            search->giveAwayKeyboardFocus(); settle();

            // On the FANTOM-07's screen.
            standalone::setTextEntryOnScreen(fantom.screen()->textOnScreen());
            search->grabKeyboardFocus();
            check(wait([&] { return standalone::textKeyboard() != nullptr; }), "focusing the search field shows the keyboard");
            auto* kb = standalone::textKeyboard();
            if(!kb) return 1;
            check(kb->getParentComponent() == &screen && screen.getLocalBounds().contains(kb->getBounds()), "docked inside the screen: " + kb->getBounds().toString().toStdString() + " in " + screen.getLocalBounds().toString().toStdString());
            check(kb->getBottom() == screen.getHeight() && kb->getWidth() == screen.getWidth() && kb->getHeight() * 100 <= screen.getHeight() * 55, "along its bottom, full width, about half its height or less");
            int keys = 0; bool big = true;
            for(auto* c : kb->getChildren()) if(auto* b = dynamic_cast<juce::TextButton*>(c)) { ++keys; big = big && b->getWidth() >= touch && b->getHeight() >= touch && b->getTitle().isNotEmpty() && b->isVisible(); }
            check(keys > 40 && big, "every key at least a touch target, each with a name: " + std::to_string(keys) + " keys");
            check(key("Done") && key("Cancel") && key("Clear") && key("Backspace") && key("Shift") && key("Caps lock") && key("Space") && key("Cursor left") && key("Cursor right") && key("Numbers and symbols"), "Done, Cancel, Clear, Backspace, Shift, Caps, Space, the cursor keys and the symbols layer");
            snapshot(screen, "device-letters");

            // Taps type through the editor: the Library's list filters as for real typing.
            tap(key("t"));
            check(search->getText() == "t" && !library.empty.isVisible(), "a tap types into the editor, which filters the list: '" + search->getText().toStdString() + "'");
            tap(key("Shift")); tap(key("A")); tap(key("b")); // the keys are named as they read
            check(search->getText() == "tAb", "Shift capitalizes one letter: '" + search->getText().toStdString() + "'");
            tap(key("Caps lock")); tap(key("C")); tap(key("D")); tap(key("Caps lock"));
            check(search->getText() == "tAbCD", "Caps capitalizes until it is off: '" + search->getText().toStdString() + "'");
            tap(key("Backspace")); tap(key("Backspace")); tap(key("Backspace"));
            check(search->getText() == "tA", "Backspace deletes: '" + search->getText().toStdString() + "'");
            tap(key("Cursor left")); tap(key("x"));
            check(search->getText() == "txA", "the cursor keys move the caret: '" + search->getText().toStdString() + "'");
            tap(key("Numbers and symbols"));
            check(key("slash") && key("at sign") && key("period") && key("hyphen") && key("underscore") && key("colon") && key("number sign") && key("ampersand") && key("apostrophe") && key("quotation mark") && key("1") && key("0"), "the symbols layer has . / : - _ @ # & ' \" and digits");
            snapshot(screen, "device-symbols");
            tap(key("at sign")); tap(key("7"));
            check(search->getText() == "tx@7A", "symbols and digits type: '" + search->getText().toStdString() + "'");
            tap(key("Letters"));
            check(key("q") != nullptr, "back to letters");
            tap(key("Clear"));
            check(search->getText().isEmpty() && library.empty.isVisible(), "Clear empties the field and the filter");

            // Done keeps the text and hides; focusing again shows it again; Cancel restores.
            tap(key("h")); tap(key("i"));
            tap(key("Done"));
            check(wait([&] { return standalone::textKeyboard() == nullptr; }) && search->getText() == "hi" && !search->hasKeyboardFocus(false), "Done hides it and keeps the text");
            search->grabKeyboardFocus();
            check(wait([&] { return standalone::textKeyboard() != nullptr; }), "the field focused again, it shows again");
            tap(key("z")); tap(key("z"));
            check(search->getText() == "hizz", "typed after reopening");
            tap(key("Cancel"));
            check(wait([&] { return standalone::textKeyboard() == nullptr; }) && search->getText() == "hi", "Cancel hides it and puts back the text it had when it came up: '" + search->getText().toStdString() + "'");
            search->grabKeyboardFocus();
            check(wait([&] { return standalone::textKeyboard() != nullptr; }), "and it can come up again");
            search->giveAwayKeyboardFocus();
            check(wait([&] { return standalone::textKeyboard() == nullptr; }), "focus leaving the field hides it");
            library.search("");

            // The Setlist app: an alert over its pane (modal) asks for a name; the keyboard works over it.
            {
                standalone::SetlistPane pane(engine, actions);
                standalone::setDesktopMode(pane, 2);
                screen.setContentNonOwned(&pane, false); screen.setSize(screenWidth, screenHeight);
                settle();
                juce::TextButton* add = nullptr;
                for(auto* c : pane.getChildren()) if(auto* b = dynamic_cast<juce::TextButton*>(c); b && b->getButtonText() == "New") add = b;
                check(add != nullptr, "the Setlist pane has New");
                if(add)
                {
                    add->triggerClick();
                    juce::AlertWindow* alert = nullptr;
                    check(wait([&] { for(auto* c : screen.getChildren()) if(auto* a = dynamic_cast<juce::AlertWindow*>(c)) alert = a; return alert != nullptr; }), "New asks for a name in an alert inside the screen");
                    auto* name = alert ? alert->getTextEditor("name") : nullptr;
                    if(name && !name->hasKeyboardFocus(false)) name->grabKeyboardFocus();
                    check(wait([&] { return standalone::textKeyboard() != nullptr; }), "the alert's field brings up the keyboard");
                    if(alert && name && standalone::textKeyboard())
                    {
                        check(alert->getBottom() <= standalone::textKeyboard()->getY(), "the alert is above the keys: " + alert->getBounds().toString().toStdString());
                        snapshot(screen, "device-alert");
                        tap(key("Shift")); tap(key("M")); tap(key("y"));
                        check(name->getText() == "My", "the keys type into the alert's field, over the modal alert: '" + name->getText().toStdString() + "'");
                        tap(key("Done"));
                        check(wait([&] { return pane.setlist().info.name == "My"; }), "Done is Return: the alert's default button made the setlist: '" + pane.setlist().info.name + "'");
                        check(wait([&] { return standalone::textKeyboard() == nullptr; }), "and the keyboard is gone");
                    }
                }
                settle();
                screen.clearContentComponent();
            }
            standalone::setTextEntryOnScreen(false);
            standalone::setDeviceScreen(nullptr);
            screen.clearContentComponent();
            configuration.deleteRecursively();
            juce::LookAndFeel::setDefaultLookAndFeel(nullptr);
        }
        running = false;
        control.join();
    }
    std::filesystem::remove(socket);
    data.deleteRecursively();
    if(!failures) std::puts("ok: on-screen text entry");
    return failures ? 1 : 0;
}
