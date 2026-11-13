// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The on-screen keyboards, a window per MIDI input, against a real engine session over the control
// socket, with a MIDI input described to the session and none opened, and no audio device. With
// SNAPSHOTS, the window and Play's line for a musician with no MIDI device are written to
// SNAPSHOTS-<name>.png.
//   oscairn_keyboard_window_test PLUGIN_DIR SCANNER THEMES [SNAPSHOTS]
#include "../KeyboardWindow.h"
#include "../PartsModel.h"
#include "../PartsView.h"
#include "../PlayView.h"
#include "../Profile.h"
#include "../Theme.h"
#include "../Icons.h"
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
    if(argc < 4) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } };
    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-keys-" + std::to_string(::getpid()) + ".sock");
    const auto data = juce::File::createTempFile("oscairn-keys-data");
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    {
        auto session = std::make_unique<Session>(48000, 128);
        const MidiEndpoint keyLab{MidiEndpoint::Kind::Usb, 3, "KeyLab 61"};
        Control::SessionStatus status; status.midiInputs = {keyLab};
        session->setHostStatus(status);
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true;
        std::atomic<int> plug = -1; // 0: the KeyLab unplugged, 1: plugged in again, on the session's thread
        std::thread control([&] {
            std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                if(const int p = plug.exchange(-1); p >= 0) { auto now = session->sessionStatus(); now.midiInputs = p ? status.midiInputs : std::vector<MidiEndpoint>{}; now.heldNotes.clear(); session->setHostStatus(now); }
                session->process(nullptr, 0, out, 2, 128); // rendered, never played
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset();
        });
        {
            standalone::Engine engine(argv[1], socket.string());
            const auto configuration = juce::File::createTempFile("oscairn-keys-config");
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            theme.apply("oscairn-dark");
            auto settle = [] { for(int i = 0; i < 20; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); };
            auto wait = [](auto done) { for(int i = 0; i < 1000 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            auto snapshot = [&](juce::Component& c, const juce::String& name) {
                if(argc < 5) return;
                const juce::File out{juce::String(argv[4]) + "-" + name + ".png"};
                out.deleteFile();
                juce::FileOutputStream stream(out);
                juce::PNGImageFormat().writeImageToStream(c.createComponentSnapshot(c.getLocalBounds()), stream);
            };
            // Three parts, each a Basic Synth: one any device plays, one only the KeyLab plays (from
            // middle C up), and one only the built-in input plays: the on-screen keyboard.
            const auto plugins = standalone::bundledPlugins(argv[1], argv[2]);
            uint32_t any = 0, keyLabOnly = 0, builtInOnly = 0;
            engine.edit([&](Control& c) {
                for(auto* id : {&any, &keyLabOnly, &builtInOnly})
                {
                    *id = standalone::addPart(c, id == &any ? "Any" : id == &keyLabOnly ? "KeyLab only" : "Keyboard only", false);
                    for(const auto& p : plugins) if(p.name == "Basic Synth") standalone::insertPlugin(c, *id, 0, p);
                }
                standalone::setDevices(c, keyLabOnly, {standalone::ruleFor(keyLab)});
                standalone::setZone(c, keyLabOnly, 60, 127, 0);
                standalone::setDevices(c, builtInOnly, {{DeviceRule::Kind::BuiltIn}});
            });
            check(wait([&] { return engine.graph().parts.size() == 3; }) && engine.editError().empty(), "three parts: " + engine.editError());
            auto peak = [&](uint32_t part) { for(const auto& m : engine.meters()) if(m.part == part) return m.peak; return 0.0; };
            auto quiet = [&] { return wait([&] { return peak(any) < 1e-6 && peak(keyLabOnly) < 1e-6 && peak(builtInOnly) < 1e-6; }); };
            auto held = [&](int source) { for(const auto& h : engine.sessionStatus().heldNotes) if(h.source == source) return h.keys; return std::vector<uint8_t>{}; };
            auto holds = [&](int source, int key) { const auto k = held(source); return std::find(k.begin(), k.end(), uint8_t(key)) != k.end(); };

            standalone::Keyboards keyboards(engine, configuration);
            keyboards.mode = [] { return 1; };
            engine.openKeyboard = [&](const std::string& input) { keyboards.open(input); };
            engine.onNotesOff = [&] { keyboards.releaseAll(); };
            keyboards.open("KeyLab 61");
            keyboards.open({});
            settle();
            auto* keyLabKeys = keyboards.find("KeyLab 61");
            auto* screenKeys = keyboards.find({});
            check(keyLabKeys && screenKeys && keyboards.size() == 2, "a window for the KeyLab and one for the on-screen keyboard");
            if(!keyLabKeys || !screenKeys) return 1;
            check(screenKeys->title() == "On-screen keyboard" && keyLabKeys->title() == "KeyLab 61", "each named for its input");
            check(keyboards.window("KeyLab 61")->isOnDesktop() && keyboards.window({})->isOnDesktop(), "each in a window of its own");

            // A click plays as the window's input: the KeyLab's reaches Any and KeyLab only, not
            // Keyboard only; the on-screen keyboard's reaches Any and Keyboard only, not KeyLab only.
            {
                // The mouse with the window on screen, over every window: JUCE's keyboard hit-tests
                // through the window, so one off screen or covered would hear no click.
                auto* window = keyboards.window("KeyLab 61");
                window->setAlwaysOnTop(true);
                window->setTopLeftPosition(juce::Desktop::getInstance().getDisplays().getPrimaryDisplay()->userArea.getPosition());
                settle();
                auto& board = keyLabKeys->keys().keyboard();
                auto mouse = juce::Desktop::getInstance().getMainMouseSource();
                const auto now = juce::Time::getCurrentTime();
                const auto r = board.getRectangleForKey(64);
                const juce::Point<float> p(r.getCentreX(), r.getY() + r.getHeight() * .8f);
                const juce::MouseEvent down(mouse, p, juce::ModifierKeys::leftButtonModifier, 1, 0, 0, 0, 0, &board, &board, now, p, now, 1, false);
                board.mouseDown(down);
                check(wait([&] { return peak(any) > 1e-3 && peak(keyLabOnly) > 1e-3; }), "a click on the KeyLab's keyboard sounds the parts the KeyLab plays");
                check(peak(builtInOnly) < 1e-6, "and not the part only the built-in input plays");
                // Failing, say whether the window was still active and the keyboard focused: losing either lets go of the key.
                check(wait([&] { return holds(3, 64); }), std::string("the engine has the key held as the KeyLab's (window ") + (window->getPeer() && window->getPeer()->isFocused() ? "active" : "not active") +
                      ", keyboard " + (keyLabKeys->hasKeyboardFocus(true) ? "focused" : "not focused") + ", key " + (keyLabKeys->keys().down(64) ? "down" : "up") + ")");
                check(keyLabKeys->keys().down(64) && keyLabKeys->keys().velocity(64) > .5f, "the key shows down, low on it loud");
                board.mouseUp(down);
                check(wait([&] { return !holds(3, 64); }), "let go, the KeyLab's key is up");
                window->setAlwaysOnTop(false);
            }
            check(quiet(), "the parts fall quiet");
            screenKeys->keys().press(64, true);
            check(wait([&] { return peak(any) > 1e-3 && peak(builtInOnly) > 1e-3; }), "the on-screen keyboard sounds Any and the part limited to it");
            check(peak(keyLabOnly) < 1e-6, "and not the part limited to the KeyLab: " + std::to_string(peak(keyLabOnly)));
            check(wait([&] { return holds(0, 64); }) && !holds(3, 64), "held on the built-in source, not the KeyLab's");
            screenKeys->keys().press(64, false);
            check(wait([&] { return !holds(0, 64); }) && quiet(), "let go");

            // The zones: a band per part the input plays, over its keys.
            using Zone = standalone::KeyboardPanel::Zone;
            check(wait([&] { keyLabKeys->refresh(); return keyLabKeys->zones() == std::vector<Zone>{{"Any", 0, 127}, {"KeyLab only", 60, 127}}; }), "the KeyLab's keyboard shows Any's and KeyLab only's zones, the split from middle C");
            check(wait([&] { screenKeys->refresh(); return screenKeys->zones() == std::vector<Zone>{{"Any", 0, 127}, {"Keyboard only", 0, 127}}; }), "the on-screen keyboard shows Any's and Keyboard only's");
            check(keyLabKeys->getDescription().contains("KeyLab only: C4 to G9"), "the zones are described: " + keyLabKeys->getDescription().toStdString());

            // The KeyLab's own notes light its keys, not the other keyboard's.
            engine.note(true, 67, .8f, -1, "KeyLab 61"); // as the KeyLab sends it
            check(wait([&] { return keyLabKeys->keys().incoming(67); }), "a note arriving from the KeyLab lights its key");
            check(!screenKeys->keys().incoming(67) && !keyLabKeys->keys().down(67), "on its keyboard alone, and not as a key played there");
            engine.note(false, 67, 0, -1, "KeyLab 61");
            check(wait([&] { return !keyLabKeys->keys().incoming(67); }), "and goes out when it is let go");

            // Several at once, each its own: the computer's keys play the one with focus.
            keyLabKeys->grabKeyboardFocus(); settle();
            auto typed = [](juce::Component& w, const juce::KeyPress& k) { return w.getPeer() && w.getPeer()->handleKeyPress(k); };
            typed(*keyboards.window({}), juce::KeyPress('a'));
            check(!screenKeys->keys().down(60), "a computer key does not play the keyboard without focus");
            check(keyLabKeys->hasKeyboardFocus(false), "the KeyLab's keyboard has focus");
            typed(*keyboards.window("KeyLab 61"), juce::KeyPress('a'));
            check(keyLabKeys->keys().down(60) && !screenKeys->keys().down(60), "A plays middle C on the keyboard with focus only");
            check(keyLabKeys->keyPressed(juce::KeyPress('a')) && keyLabKeys->keys().down(60), "its repeat is the keyboard's, and plays nothing more");
            check(!keyLabKeys->keyPressed(juce::KeyPress('s', juce::ModifierKeys::commandModifier, 0)) && !keyLabKeys->keys().down(62), "Cmd-S is left to the menus");
            keyLabKeys->keyStateChanged(false); // A is not down
            check(!keyLabKeys->keys().down(60), "a computer key let go lets its note go");
            keyLabKeys->keyPressed(juce::KeyPress('x'));
            check(keyLabKeys->octave() == 1 && screenKeys->octave() == 0 && keyLabKeys->keys().lowest() > screenKeys->keys().lowest(), "X takes one keyboard an octave up, not the other");
            keyLabKeys->keyPressed(juce::KeyPress('z'));

            // Sustain keeps a key let go sounding until it is off; Panic lets go of everything, sustain included.
            screenKeys->sustainButton().triggerClick(); settle();
            screenKeys->keys().press(62, true); screenKeys->keys().press(62, false);
            check(screenKeys->keys().sustained(62) && wait([&] { return holds(0, 62); }), "sustained, a key let go still sounds");
            screenKeys->sustainButton().triggerClick(); settle();
            check(wait([&] { return !holds(0, 62); }), "sustain off lets it go");
            screenKeys->sustainButton().triggerClick(); settle();
            screenKeys->keys().press(62, true); screenKeys->keys().press(62, false);
            check(wait([&] { return holds(0, 62); }), "sustained again");
            engine.allNotesOff();
            check(!screenKeys->keys().sustained(62) && !screenKeys->sustainButton().getToggleState() && wait([&] { return !holds(0, 62); }), "Panic lets go of the sustained key and turns sustain off");
            screenKeys->fixedVelocity().triggerClick();
            screenKeys->keys().press(65, true, 100 / 127.f);
            check(screenKeys->keys().velocity(65) > .7f && screenKeys->keys().velocity(65) < .9f, "fixed velocity plays at 100");
            screenKeys->keys().press(65, false);

            check(keyboards.window({}) != nullptr, "snapshot: the on-screen keyboard");
            if(auto* w = keyboards.window({})) { w->setSize(900, 230); settle(); screenKeys->keys().press(64, true); engine.note(true, 72, .8f); wait([&] { return screenKeys->keys().incoming(72); }); settle(); snapshot(*screenKeys, "keyboard-window"); screenKeys->keys().press(64, false); engine.note(false, 72, 0); }

            // Closing with its close box lets go of every note it holds; Esc closes too.
            keyLabKeys->keys().press(64, true);
            check(wait([&] { return holds(3, 64); }), "a KeyLab key held");
            if(auto* w = dynamic_cast<juce::DocumentWindow*>(keyboards.window("KeyLab 61"))) w->closeButtonPressed();
            check(wait([&] { return !holds(3, 64); }), "the close box lets go of it");
            check(wait([&] { return keyboards.find("KeyLab 61") == nullptr; }) && keyboards.find({}) != nullptr, "the KeyLab's window goes, the other stays");
            screenKeys = keyboards.find({});
            screenKeys->keys().press(60, true);
            check(wait([&] { return holds(0, 60); }), "an on-screen key held");
            check(screenKeys->keyPressed(juce::KeyPress(juce::KeyPress::escapeKey)), "Esc is the keyboard's");
            check(wait([&] { return !holds(0, 60); }) && wait([&] { return keyboards.size() == 0; }), "Esc closes it, letting go");

            // Its input unplugged with a key held: the key is let go, and its keyboard closes.
            keyboards.open("KeyLab 61"); settle();
            keyLabKeys = keyboards.find("KeyLab 61");
            keyLabKeys->keys().press(64, true);
            check(wait([&] { return holds(3, 64); }), "a KeyLab key held again");
            plug = 0;
            check(wait([&] { return keyboards.size() == 0; }), "unplugged, the KeyLab's keyboard closes");
            check(wait([&] { return held(3).empty(); }) && quiet(), "and its note is let go");

            // Play's line for a new musician with no MIDI device: a link that opens the on-screen
            // keyboard, gone once a device is connected, or dismissed for good.
            {
                standalone::PlayView play(engine);
                standalone::setDesktopMode(play, 1); play.setSize(1100, 700);
                play.addToDesktop(juce::ComponentPeer::windowIsTemporary); play.setTopLeftPosition(-6000, -6000); play.setVisible(true);
                check(!play.hintShown(), "nothing offered: no line");
                keyboards.offerHint(play);
                check(wait([&] { return play.hintShown(); }), "no MIDI device: the line shows");
                settle(); snapshot(play, "play-hint");
                check(keyboards.size() == 0, "and nothing opened by itself");
                play.hintLink().triggerClick(); settle();
                check(keyboards.find({}) != nullptr, "its link opens the on-screen keyboard");
                keyboards.close({}); settle();
                plug = 1;
                check(wait([&] { play.select(0); return !play.hintShown(); }), "a device connected: the line goes");
                plug = 0;
                check(wait([&] { play.select(0); return play.hintShown(); }), "unplugged again: it is back");
                play.hintDismiss().triggerClick(); settle();
                check(!play.hintShown(), "dismissed, it goes");
                standalone::Keyboards later(engine, configuration);
                later.offerHint(play);
                check(!play.hintShown() && configuration.getChildFile("keyboard.txt").loadFileAsString().contains("hint dismissed"), "and stays gone after a restart");
            }

            // The profile decides: no on-screen keyboard, no button anywhere and no line.
            {
                auto profile = [](const std::string& id) { for(const auto& [name, json] : standalone::builtInProfiles()) if(name == id) return standalone::Profile::parse(json); return standalone::Profile{}; };
                check(standalone::Profile::desktop().onScreenKeyboard, "the desktop offers it");
                check(!profile("fantom-07").onScreenKeyboard && !profile("modx-m").onScreenKeyboard, "keyboards with their own keys do not");
                check(profile("kick").onScreenKeyboard && profile("ignite").onScreenKeyboard && profile("imx8mm-evk").onScreenKeyboard && profile("qemu-aarch64").onScreenKeyboard, "keyless devices do");
                const std::string base = R"({"format":"oscairn.profile","version":2,"id":"t","name":"T","reliability":"instrument")";
                check(!standalone::Profile::parse(base + R"(,"controls":[{"kind":"keys","count":61,"role":"notes"}]})").onScreenKeyboard, "absent, keys of its own: not offered");
                check(standalone::Profile::parse(base + R"(,"controls":[{"kind":"pads","count":16,"role":"notes"}]})").onScreenKeyboard, "absent, pads only: offered");
                bool refused = false;
                try { standalone::Profile::parse(R"({"format":"oscairn.profile","version":2,"id":"c","name":"C","midi":{"ports":[{"name":"C"}]},"onScreenKeyboard":true})"); } catch(const std::exception&) { refused = true; }
                check(refused, "a controller's profile has none");

                auto keyboardButtons = [&](juce::Component& view) { int n = 0; for(auto* c : view.getChildren()) if(auto* b = dynamic_cast<standalone::IconButton*>(c); b && b->getTitle() == "Open on-screen keyboard") ++n; return n; };
                standalone::PartsView parts(engine, argv[2]);
                standalone::setDesktopMode(parts, 1); parts.setEmbedded(true); parts.setSize(1180, 820);
                parts.showPart(keyLabOnly); settle();
                check(keyboardButtons(parts) == 1, "offered: a keyboard button beside the part's devices");
                parts.showRouting(graphInput, standalone::notesPort); settle();
                check(keyboardButtons(parts) == 1, "and on the keys and MIDI input in Routing");
                standalone::PlayView play(engine);
                standalone::setDesktopMode(play, 1); play.setSize(1100, 700);
                engine.openKeyboard = {};
                standalone::PartsView none(engine, argv[2]);
                standalone::setDesktopMode(none, 1); none.setEmbedded(true); none.setSize(1180, 820);
                none.showPart(keyLabOnly); settle();
                check(keyboardButtons(none) == 0, "not offered: no button beside the devices");
                none.showRouting(graphInput, standalone::notesPort); settle();
                check(keyboardButtons(none) == 0, "nor in Routing");
                check(!play.hintShown(), "nor Play's line");
            }
            plug = 1;
            keyboards.releaseAll();
            configuration.deleteRecursively();
            juce::LookAndFeel::setDefaultLookAndFeel(nullptr);
        }
        running = false;
        control.join();
    }
    std::filesystem::remove(socket);
    data.deleteRecursively();
    if(!failures) std::puts("ok: an on-screen keyboard per MIDI input");
    return failures ? 1 : 0;
}
