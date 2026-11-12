// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Unsaved changes asked about before quitting, and before the Library opens another, against a
// real engine session over the control socket, without an audio device: Save, Don't Save and
// Cancel; a save that fails stays; nothing asked when nothing is unsaved.
//   oscairn_desktop_quit_test PLUGIN_DIR
#include "../LibraryPane.h"
#include "../PartsModel.h"
#include "oscairn/local_socket.h"
#include "oscairn/session.h"
#include <juce_gui_extra/juce_gui_extra.h>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <thread>
#include <unistd.h>

using namespace oscairn;

int main(int argc, char** argv)
{
    if(argc < 2) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } };
    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-quit-" + std::to_string(::getpid()) + ".sock");
    // A data folder of its own, so the user's library is never read or written.
    const auto data = juce::File::createTempFile("oscairn-quit-data");
    data.createDirectory();
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    {
        // The session, as the engine helper runs it: its startup graph is no edit.
        Session session(48000, 128);
        session.clearHistory();
        LocalServer server(session);
        server.listen(socket.string());
        std::atomic<bool> running = true;
        std::thread control([&] {
            while(running)
            {
                server.poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session.service();
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
        });
        {
            standalone::Engine engine(argv[1], socket.string());
            auto wait = [](auto done) { for(int i = 0; i < 1000 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            juce::Component over; over.setSize(600, 400);
            auto modal = [] { return dynamic_cast<juce::AlertWindow*>(juce::ModalComponentManager::getInstance()->getModalComponent(0)); };
            auto answer = [&](int chosen) {
                auto* w = modal();
                if(!w) return;
                juce::Component::SafePointer<juce::Component> gone(w);
                w->exitModalState(chosen);
                wait([&] { return !gone; });
            };
            auto unsaved = [&] { const auto info = engine.performanceNow(); return info && info->unsaved; };
            int quits = 0;
            auto quit = [&] { return standalone::askToSave(engine, &over, "quitting", [&] { ++quits; }); };
            auto edit = [&](const std::string& part) {
                engine.edit([part](Control& c) { standalone::addPart(c, part, false); });
                return wait([&] { return unsaved(); });
            };
            check(wait([&] { return engine.connected() && !engine.jobRunning(); }), "the engine is up");
            check(!quit() && quits == 1 && !modal(), "nothing unsaved in a new Untitled: it quits without asking");
            check(edit("Keys"), "an edit is unsaved");
            quits = 0;
            check(quit() && modal() && modal()->getName() == "Save changes to Untitled before quitting?", "quitting with changes asks, by the Performance's name");
            if(auto* w = modal()) check(w->getNumButtons() == 3 && w->getButton("Save") && w->getButton("Don't Save") && w->getButton("Cancel"), "with Save, Don't Save and Cancel");
            answer(0);
            check(quits == 0 && !modal(), "Cancel stays");
            quit(); answer(2);
            check(quits == 1 && !modal(), "Don't Save quits");
            // Save on Untitled asks for a name, as Save does; a save that fails stays and says why
            // (a file where the library's folder goes: the engine cannot write it).
            quits = 0;
            const auto performances = data.getChildFile("Performances");
            performances.replaceWithText("in the way");
            auto saveNamed = [&](const juce::String& name) {
                quit(); answer(1);
                const bool asked = wait([&] { auto* w = modal(); return w && w->getName() == "Save" && w->getTextEditor("name"); });
                if(asked) modal()->getTextEditor("name")->setText(name);
                answer(1);
                return asked;
            };
            // A message box runs its own modal loop here (JUCE_MODAL_LOOPS_PERMITTED), so a timer
            // answers it from inside that loop.
            struct Answer : juce::Timer
            {
                bool seen = false;
                void timerCallback() override
                {
                    auto* w = dynamic_cast<juce::AlertWindow*>(juce::ModalComponentManager::getInstance()->getModalComponent(0));
                    if(w && w->getName() == "Not saved") { seen = true; stopTimer(); w->exitModalState(0); }
                }
            } notSaved;
            notSaved.startTimer(20);
            check(saveNamed("Kept"), "Save on Untitled asks for its name");
            // The alert's message is drawn, not a child to read: the error it shows is the engine's.
            check(wait([&] { return notSaved.seen; }) && engine.soundMessage().find("Performances") != std::string::npos && quits == 0, "a failed save stays, and says why: " + engine.soundMessage());
            notSaved.stopTimer();
            wait([&] { return !modal(); });
            check(unsaved(), "still unsaved");
            performances.deleteFile();
            saveNamed("Kept");
            check(wait([&] { return quits == 1; }) && !modal(), "Save saves, then quits");
            check(!unsaved() && engine.performanceNow() && engine.performanceNow()->name == "Kept", "saved as Kept");
            check(!quit() && quits == 2, "nothing unsaved once saved: no prompt");
            // One with a place in the library: Save writes over it, asking nothing more.
            check(edit("Bass"), "another edit");
            quits = 0;
            check(quit() && modal() && modal()->getName() == "Save changes to Kept before quitting?", "asked about Kept");
            answer(1);
            check(wait([&] { return quits == 1; }) && !modal() && !unsaved(), "Save saved over it and quit, with no name asked");
            // Opening another from the Library asks the same; Cancel keeps the open one, Don't Save opens it.
            standalone::Actions actions;
            standalone::LibraryPane pane(engine, actions, standalone::LibraryPane::Mode::Play);
            pane.search("Kept");
            check(wait([&] { pane.refresh(); return pane.items().size() == 1; }), "the Library lists Kept");
            check(edit("Pads"), "an edit before opening another");
            pane.choose(0);
            check(modal() && modal()->getName() == "Save changes to Kept before opening Kept?", "opening one from the Library asks first");
            answer(0);
            check(!modal() && !engine.jobRunning() && unsaved(), "Cancel opens nothing");
            pane.choose(0); answer(2);
            check(wait([&] { return !engine.jobRunning() && !unsaved(); }), "Don't Save opens it, its changes gone");
        }
        running = false;
        control.join();
    }
    std::filesystem::remove(socket);
    data.deleteRecursively();
    if(!failures) std::puts("ok: unsaved changes are asked about before quitting and before opening another");
    return failures ? 1 : 0;
}
