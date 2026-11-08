// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0022 step 2: the Setlist app as a program of its own, started from its manifest by the
// shell's supervisor, against a real engine session over the control socket, without an audio
// device. A send from the shell's side and a pick from the app's cross the process boundary
// through the engine; the app killed comes back with its state; hung, the shell says it is not
// responding and Close starts it again; killed again and again, it stays closed; leaking past
// its memory maximum, it is stopped and comes back; hung behind, it is stopped after 10 seconds.
// Through all of it the engine plays on, and the app's stderr is
// in its log. With SNAPSHOTS, the KICK's 1024 × 600 screen with the Setlist shown under the top
// bar is written to SNAPSHOTS-setlist-panel.png.
//   oscairn_desktop_apps_test PLUGIN_DIR APPS THEMES [SNAPSHOTS]
#include "../AppProcesses.h"
#include "../Launcher.h"
#include "../LibraryPane.h"
#include "../Setlist.h"
#include "EngineProcess.h"
#include "oscairn/local_socket.h"
#include "oscairn/session.h"
#include <juce_gui_extra/juce_gui_extra.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <fcntl.h>
#include <filesystem>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>

using namespace oscairn;

int main(int argc, char** argv)
{
    if(argc < 4) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { std::fprintf(stderr, "%s: %s\n", ok ? "ok" : "FAIL", what.c_str()); if(!ok) ++failures; };
    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-apps-" + std::to_string(::getpid()) + ".sock");
    // A data and a configuration folder of its own, which the app inherits: the user's are never touched.
    const auto data = juce::File::createTempFile("oscairn-apps-data"), configuration = juce::File::createTempFile("oscairn-apps-config");
    configuration.createDirectory();
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    ::setenv("OSCAIRN_CONFIG_HOME", configuration.getFullPathName().toRawUTF8(), 1);
    const std::string themes = "OSCAIRN_THEMES=" + std::string(argv[3]);
    {
        auto session = std::make_unique<Session>(48000, 128);
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true;
        std::atomic<uint64_t> blocks = 0, sounded = 0; // blocks rendered, and the last with sound in it
        // The audio rendered on the same thread, a note held from the keys (pressed again after
        // a Performance opens, which lets held notes go).
        std::thread control([&] {
            std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                if(blocks % 100 == 0 && blocks > sounded + 50) session->note(true, 60, .8);
                session->process(nullptr, 0, out, 2, 128);
                if(std::any_of(l.begin(), l.end(), [](float v) { return std::abs(v) > 1e-3f; })) sounded = blocks.load();
                ++blocks;
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset();
        });
        {
            standalone::Engine engine(argv[1], socket.string());
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            auto wait = [&](auto done, int ms = 10000) { for(int i = 0; i < ms / 10 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            // Sound goes on: the engine renders the held note's sound, now.
            auto playing = [&] { const auto before = blocks.load(); return wait([&] { return sounded > before; }, 3000) && engine.connected(); };
            engine.edit([&](Control& c) {
                const auto synth = c.add("synth", std::string(argv[1]) + "/oscairn-basic-synth.clap");
                c.connectNotes(graphInput, synth); c.connectAudio(synth, graphOutput); c.commit();
                c.setPerformanceInfo({"", "Verse Keys", "", {"keys"}, "#3e63dd"}); c.savePerformanceToLibrary("");
                c.setPerformanceInfo({"", "Big Strings", "", {"pad"}}); c.savePerformanceToLibrary("Big Strings", true);
                c.setPerformanceInfo({"", "Bass Split", "", {"bass"}, "#46a758"}); c.savePerformanceToLibrary("Bass Split", true);
            });
            Control::LibraryQuery all; all.kind = "performance";
            std::optional<Control::LibraryResult> library;
            check(wait([&] { return (library = engine.library(all)) && library->items.size() == 3; }), "three Performances in the library");
            auto idOf = [&](const std::string& name) { for(const auto& i : library->items) if(i.name == name) return i.id; return std::string(); };
            check(playing(), "the synth sounds");

            // The shell's side: the Library picks (once; later picks are canceled), Play hears
            // the setlist's entry playing.
            standalone::Actions actions;
            int picks = 0;
            std::string entry = "?";
            actions.offer(standalone::pickPerformance, "Library", [&](const std::string&, standalone::Actions::Answer answer) { answer(picks++ == 0 ? idOf("Bass Split") : std::string()); });
            actions.offer(standalone::sendSetlistEntry, "Performance", [&](const std::string& id, standalone::Actions::Answer answer) { entry = id; answer("shown"); });
            const auto apps = standalone::findApps(juce::File(argv[2]));
            check(apps.size() == 1 && apps[0].id == "oscairn.setlist" && apps[0].executable.existsAsFile() && apps[0].maximum == 400, "the Setlist's manifest is found in the folder, with its build for this platform");
            const auto setlists = data.getChildFile("Setlists");
            auto saved = [&] { try { return standalone::readSetlist(standalone::setlistFile(setlists, "Setlist")).entries.size(); } catch(const std::exception&) { return size_t(0); } };
            {
                std::vector<standalone::AppProcesses::Notice> notices;
                standalone::AppProcesses processes(engine, actions, apps, {themes, "OSCAIRN_TEST_PICK=1"});
                processes.onNotice = [&](const standalone::AppProcesses::Notice& n) { notices.push_back(n); std::fprintf(stderr, "  notice: %s\n", n.text.toRawUTF8()); };
                auto said = [&](const juce::String& text) { return std::any_of(notices.begin(), notices.end(), [&](const auto& n) { return n.text.contains(text); }); };
                check(actions.offered(standalone::sendPerformance) && actions.offered(standalone::stepSetlist), "the manifest's actions are offered");

                // A send from the shell's side starts the app from its manifest, and it takes it.
                std::string answer = "?";
                check(actions.request(standalone::sendPerformance, idOf("Verse Keys"), [&](const std::string& a) { answer = a; }), "the send is asked");
                check(processes.pid("oscairn.setlist") > 0 && processes.pid("oscairn.setlist") != ::getpid(), "the Setlist runs as a process of its own");
                check(wait([&] { return answer != "?"; }) && answer == "Added to Setlist", "the send crossed into the app: " + answer);
                // The app's pick, asked at its start, crosses back: the shell's Library answers.
                check(wait([&] { return saved() == 2; }) && picks == 1, "the app's pick was answered across the boundary, and filled a slot");
                const auto first = standalone::readSetlist(standalone::setlistFile(setlists, "Setlist"));
                check(first.entries.size() == 2 && first.entries[0].name == "Verse Keys" && first.entries[1].name == "Bass Split", "in order: the send, then the pick");
                // Previous and Next from Play: it steps, and says what plays.
                answer = "?";
                actions.request(standalone::stepSetlist, "1", [&](const std::string& a) { answer = a; });
                check(wait([&] { return answer != "?"; }) && answer == idOf("Verse Keys") && wait([&] { return entry == idOf("Verse Keys"); }), "a step opens the first entry, and Play hears it");
                std::string opened;
                check(wait([&] { if(const auto i = engine.performanceInfo(); i && !engine.soundBusy()) opened = i->name; return opened == "Verse Keys"; }), "the engine opened it, on the app's own job connection: " + opened);
                check(playing(), "and plays on");
                const auto log = processes.logFile("oscairn.setlist");
                check(wait([&] { return processes.log("oscairn.setlist").contains("oscairn-setlist: started"); }), "its stderr is in " + log.getFullPathName().toStdString());

                // Killed, in front: started again at once, and back with its state.
                processes.setFront("oscairn.setlist");
                const auto killed = processes.pid("oscairn.setlist");
                entry = "?";
                ::kill(killed, SIGKILL);
                check(wait([&] { return said("stopped (signal 9); started again"); }), "the shell says it stopped and is started again");
                check(processes.pid("oscairn.setlist") > 0 && processes.pid("oscairn.setlist") != killed, "a new process");
                check(wait([&] { return entry == idOf("Verse Keys"); }), "back with its state: the entry playing, said again");
                check(playing(), "sound went on through the crash");
                answer = "?";
                actions.request(standalone::sendPerformance, idOf("Big Strings"), [&](const std::string& a) { answer = a; });
                check(wait([&] { return answer == "Added to Setlist"; }) && saved() == 3, "the setlist kept its entries and takes more");
                check(processes.log("oscairn.setlist").indexOf("oscairn-setlist: started") != processes.log("oscairn.setlist").lastIndexOf("oscairn-setlist: started"), "both runs' stderr in its log");

                // Hung (stopped by a signal, so it answers no ping): the shell says so; Close stops it and starts it again.
                const auto hung = processes.pid("oscairn.setlist");
                ::kill(hung, SIGSTOP);
                check(wait([&] { return processes.state("oscairn.setlist") == standalone::AppProcesses::State::NotResponding; }, 9000) && said("Setlist is not responding")
                      && notices.back().choice == standalone::AppProcesses::Notice::Choice::WaitOrClose, "a hang is seen by the pings: not responding, Wait or Close");
                check(playing(), "sound goes on while it hangs");
                processes.close("oscairn.setlist");
                check(wait([&] { return said("was not responding; started again"); }) && processes.pid("oscairn.setlist") > 0 && processes.pid("oscairn.setlist") != hung, "Close stopped it; it is started again");
                entry = "?";
                check(wait([&] { return entry == idOf("Verse Keys"); }), "back with its state again");

                // Killed again and again: past 3 restarts a minute it stays closed, with Open again.
                for(int i = 0; i < 2 && processes.state("oscairn.setlist") != standalone::AppProcesses::State::GaveUp; ++i)
                {
                    const auto pid = processes.pid("oscairn.setlist");
                    if(pid > 0) ::kill(pid, SIGKILL);
                    wait([&] { return processes.pid("oscairn.setlist") != pid; });
                }
                check(processes.state("oscairn.setlist") == standalone::AppProcesses::State::GaveUp && processes.pid("oscairn.setlist") < 0 && said("again and stays closed")
                      && notices.back().choice == standalone::AppProcesses::Notice::Choice::OpenAgain, "repeated crashes: it stays closed, with Open again");
                answer = "?";
                actions.request(standalone::sendPerformance, idOf("Big Strings"), [&](const std::string& a) { answer = a; });
                check(answer.empty() && processes.pid("oscairn.setlist") < 0, "a send to it now is canceled, and starts nothing");
                check(playing(), "and sound went on");
                std::fprintf(stderr, "  its log:\n%s\n", processes.log("oscairn.setlist").toRawUTF8());
            }

            // Leaking: past its manifest's 400 MB it is stopped, said, and comes back (it leaks once).
            // Then hung behind another app: after 10 seconds it is stopped, and not started again.
            {
                std::vector<standalone::AppProcesses::Notice> notices;
                standalone::AppProcesses processes(engine, actions, apps, {themes, "OSCAIRN_TEST_LEAK=600"});
                processes.onNotice = [&](const standalone::AppProcesses::Notice& n) { notices.push_back(n); std::fprintf(stderr, "  notice: %s\n", n.text.toRawUTF8()); };
                auto said = [&](const juce::String& text) { return std::any_of(notices.begin(), notices.end(), [&](const auto& n) { return n.text.contains(text); }); };
                processes.setFront("oscairn.setlist");
                std::string answer = "?";
                actions.request(standalone::sendPerformance, idOf("Bass Split"), [&](const std::string& a) { answer = a; });
                check(wait([&] { return said("used more than its 400 MB of memory and was stopped; started again"); }), "leaking past its maximum, it is stopped and the shell says so");
                // The send may be taken before the leak passes the maximum, or canceled with it: either way, answered.
                check(wait([&] { return answer != "?"; }), "the request out to it is answered: " + (answer.empty() ? std::string("canceled") : answer));
                entry = "?";
                check(wait([&] { return entry == idOf("Verse Keys"); }) && processes.state("oscairn.setlist") == standalone::AppProcesses::State::Running, "it came back, with its state");
                check(playing(), "sound went on");
                check(processes.log("oscairn.setlist").contains("oscairn-setlist: started"), "its stderr is in its log");
                processes.setFront({});
                const auto behind = processes.pid("oscairn.setlist");
                ::kill(behind, SIGSTOP);
                check(wait([&] { return said("was not responding."); }, 14000) && processes.pid("oscairn.setlist") < 0
                      && processes.state("oscairn.setlist") == standalone::AppProcesses::State::Closed, "hung behind, it is stopped after 10 seconds, and left closed until opened");
                check(playing(), "sound went on");
            }

            // Over its memory maximum: a manifest the same but for a maximum of 1 MB, beside a link to the build.
            {
                const auto folder = juce::File::createTempFile("oscairn-apps-small").getChildFile("oscairn.setlist");
                folder.createDirectory();
                auto manifest = juce::JSON::parse(juce::File(argv[2]).getChildFile("oscairn.setlist/app.json").loadFileAsString());
                manifest["memory"].getDynamicObject()->setProperty("max", 1);
                folder.getChildFile("app.json").replaceWithText(juce::JSON::toString(manifest));
                for(const auto& f : juce::File(argv[2]).getChildFile("oscairn.setlist").findChildFiles(juce::File::findFilesAndDirectories, false))
                    if(f.getFileName() != "app.json") f.createSymbolicLink(folder.getChildFile(f.getFileName()), true);
                std::vector<standalone::AppProcesses::Notice> notices;
                standalone::AppProcesses processes(engine, actions, standalone::findApps(folder.getParentDirectory()), {themes});
                processes.onNotice = [&](const standalone::AppProcesses::Notice& n) { notices.push_back(n); std::fprintf(stderr, "  notice: %s\n", n.text.toRawUTF8()); };
                processes.setFront("oscairn.setlist");
                std::string answer = "?";
                actions.request(standalone::sendPerformance, idOf("Bass Split"), [&](const std::string& a) { answer = a; });
                check(wait([&] { return !notices.empty(); }) && notices.front().text.contains("used more than its 1 MB of memory and was stopped"), "over its memory maximum it is stopped, and the shell says why");
                check(wait([&] { return answer != "?"; }) && answer.empty(), "the request out to it is canceled");
                check(wait([&] { return processes.state("oscairn.setlist") == standalone::AppProcesses::State::GaveUp; }), "and, stopped each time it starts, it stays closed");
                check(playing(), "sound went on");
                folder.getParentDirectory().deleteRecursively(false);
            }

            // The KICK's screen with the Setlist shown: the shell's frame under its top bar, and the
            // app's own window where the shell puts it (the app renders itself, as it would there).
            if(argc >= 5)
            {
                standalone::AppProcesses processes(engine, actions, apps, {themes});
                auto place = std::make_unique<standalone::AppPlace>(processes, "oscairn.setlist");
                standalone::AppFrame frame(engine, "Setlist", *place, {});
                standalone::setDesktopMode(frame, 2); standalone::setDesktopMode(*place, 2);
                frame.setSize(1024, 600);
                wait([] { return false; }, 300);
                auto image = frame.createComponentSnapshot(frame.getLocalBounds());
                const auto area = place->getBounds();
                const auto shot = juce::File::createTempFile(".png");
                const int connection = engine.appConnection("oscairn.setlist");
                const int output = ::open(processes.logFile("oscairn.setlist").getFullPathName().toRawUTF8(), O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
                const auto child = host::spawnChild({apps[0].executable.getFullPathName().toStdString(), "--snapshot", shot.getFullPathName().toStdString(),
                                                     "--size", std::to_string(area.getWidth()) + "x" + std::to_string(area.getHeight()), "--mode", "2"},
                                                    output, {themes, "OSCAIRN_CONTROL_FD=3", "OSCAIRN_ACTIONS=oscairn.pick.performance", "OSCAIRN_APP_DATA=" + data.getChildFile("AppData/oscairn.setlist").getFullPathName().toStdString()}, connection);
                ::close(connection); ::close(output);
                int status = -1;
                for(int i = 0; i < 1500 && ::waitpid(child, &status, WNOHANG) == 0; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10);
                const auto app = juce::ImageFileFormat::loadFrom(shot);
                check(app.isValid() && app.getWidth() == area.getWidth() && app.getHeight() == area.getHeight(), "the app rendered its window at the place's size");
                { juce::Graphics g(image); g.drawImageAt(app, area.getX(), area.getY()); }
                const juce::File out{juce::String(argv[4]) + "-setlist-panel.png"};
                out.deleteFile();
                juce::FileOutputStream stream(out);
                juce::PNGImageFormat().writeImageToStream(image, stream);
                shot.deleteFile();
            }
        }
        running = false;
        control.join();
    }
    std::filesystem::remove(socket);
    data.deleteRecursively(false); configuration.deleteRecursively(false);
    if(failures) std::fprintf(stderr, "%d failed\n", failures);
    return failures ? 1 : 0;
}
