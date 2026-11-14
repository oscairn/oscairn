// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0022 step 2: the Setlist app as a program of its own, started from its manifest by the
// shell's supervisor, against a real engine session over the control socket, without an audio
// device. A send from the shell's side and a pick from the app's cross the process boundary
// through the engine; the app killed comes back with its state; hung, the shell says it is not
// responding and Close starts it again; killed again and again, it stays closed; leaking past
// its memory maximum, it is stopped and comes back, while a second's spike past it is neither
// stopped nor measured; hung behind, it is stopped after 10 seconds.
// Through all of it the engine plays on, and the app's stderr is
// in its log. With SNAPSHOTS, the KICK's 1024 × 600 screen with the Setlist shown under the top
// bar is written to SNAPSHOTS-setlist-panel.png.
//   oscairn_desktop_apps_test PLUGIN_DIR APPS THEMES [SNAPSHOTS]
#include "../AppProcesses.h"
#include "../Launcher.h"
#include "../LibraryPane.h"
#include "../Setlist.h"
#include "oscairn/setlist.h"
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
            auto apps = standalone::findApps(juce::File(argv[2]));
            std::erase_if(apps, [](const auto& a) { return a.id != "oscairn.setlist"; }); // the Settings app beside it has a test of its own
            check(apps.size() == 1 && apps[0].id == "oscairn.setlist" && apps[0].executable.existsAsFile() && apps[0].maximum == 400 && apps[0].permissions == std::vector<std::string>{"play", "library"}, "the Setlist's manifest is found in the folder, with its build for this platform and its permissions");
            const auto setlists = data.getChildFile("Setlists");
            // The setlist the app saved through the library (docs/setlist-format.md).
            auto savedSetlist = [&] {
                for(const auto& f : setlists.findChildFiles(juce::File::findDirectories, false, "*.osetlist")) return oscairn::readSetlist(f.getChildFile("setlist.json").loadFileAsString().toStdString());
                throw std::runtime_error("no setlist");
            };
            auto saved = [&] { try { return standalone::slotsOf(savedSetlist()).size(); } catch(const std::exception&) { return size_t(0); } };
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
                const auto first = standalone::slotsOf(savedSetlist());
                check(first.size() == 2 && first[0].name == "Verse Keys" && first[1].name == "Bass Split", "in order: the send, then the pick");
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
                // Stopped once over it for 3 seconds, it may take the send first, or it is canceled with it: either way, answered.
                check(wait([&] { return answer != "?"; }), "the request out to it is answered: " + (answer.empty() ? std::string("canceled") : answer));
                check(wait([&] { return processes.state("oscairn.setlist") == standalone::AppProcesses::State::GaveUp; }), "and, stopped each time it starts, it stays closed");
                check(playing(), "sound went on");
                folder.getParentDirectory().deleteRecursively(false);
            }

            // A spike past its maximum for a second (500 MB held, then let go), as a window's graphics
            // take when much of it is drawn afresh: not stopped, and not measured as its use.
            {
                data.getChildFile("app-memory.json").deleteFile();
                std::vector<standalone::AppProcesses::Notice> notices;
                standalone::AppProcesses processes(engine, actions, apps, {themes, "OSCAIRN_TEST_SPIKE=500"});
                processes.onNotice = [&](const standalone::AppProcesses::Notice& n) { notices.push_back(n); std::fprintf(stderr, "  notice: %s\n", n.text.toRawUTF8()); };
                processes.setFront("oscairn.setlist");
                std::string answer = "?";
                actions.request(standalone::sendPerformance, idOf("Bass Split"), [&](const std::string& a) { answer = a; });
                check(wait([&] { return processes.log("oscairn.setlist").contains("the test's spike let go"); }), "the app spiked past its maximum and let it go");
                const auto pid = processes.pid("oscairn.setlist");
                wait([] { return false; }, 4000);
                check(notices.empty() && processes.pid("oscairn.setlist") == pid && processes.state("oscairn.setlist") == standalone::AppProcesses::State::Running, "a spike past its maximum does not stop it");
                check(processes.reservation("oscairn.setlist") < 400ull << 20, "nor is it measured as its use: " + std::to_string(processes.reservation("oscairn.setlist") >> 20) + " MiB");
                processes.stop("oscairn.setlist");
                wait([&] { return processes.pid("oscairn.setlist") < 0; });
                data.getChildFile("app-memory.json").deleteFile();
            }

            // RFC 0025 step 8: an app that is a process holds its manifest's reservation (the
            // Setlist's 60 MiB) while it runs, and a start that does not fit is refused, saying
            // why, starting nothing; closed, its reservation is let go.
            {
                constexpr uint64_t MiB = 1ull << 20;
                auto reserved = [&](const std::string& app) {
                    Control::MemoryState m;
                    engine.edit([&](Control& c) { m = c.memoryState(); });
                    for(const auto& a : m.running) if(a.app == app) return a.reserve;
                    return uint64_t(0);
                };
                check(reserved("oscairn.setlist") == 0, "the earlier runs let their reservations go");
                // 1 GiB: 300 MiB for the system, the Library always available at 40, a 600 MiB floor and a 50 MiB margin leave 34 for apps.
                engine.edit([](Control& c) { c.setBudget({.9, 0, 1024 * MiB, 300 * MiB, 600 * MiB, 50 * MiB}); c.setAlwaysAvailable({{"oscairn.library", 40 * MiB}}); });
                // Measured afresh (the leak above was measured), and leaking 100 MB on its next run: past its reservation, under its maximum.
                data.getChildFile("app-memory.json").deleteFile();
                data.getChildFile("AppData/oscairn.setlist/leaked").deleteFile();
                std::vector<standalone::AppProcesses::Notice> notices;
                standalone::AppProcesses processes(engine, actions, apps, {themes, "OSCAIRN_TEST_LEAK=100"});
                processes.onNotice = [&](const standalone::AppProcesses::Notice& n) { notices.push_back(n); std::fprintf(stderr, "  notice: %s\n", n.text.toRawUTF8()); };
                check(processes.reservation("oscairn.setlist") == 60 * MiB, "it reserves its manifest's 60 MiB");
                std::string answer = "?";
                actions.request(standalone::sendPerformance, idOf("Bass Split"), [&](const std::string& a) { answer = a; });
                check(answer.empty() && processes.pid("oscairn.setlist") < 0 && !notices.empty() && notices.back().text.contains("Setlist did not open: it needs 60 MiB, 34 free for apps"),
                      "a start past what is free for apps is refused with the reason, and starts nothing: " + (notices.empty() ? std::string() : notices.back().text.toStdString()));
                check(reserved("oscairn.setlist") == 0, "and reserves nothing");
                // 2 GiB: it fits, and holds its manifest's 60 MiB while it runs.
                engine.edit([](Control& c) { c.setBudget({.9, 0, 2048 * MiB, 300 * MiB, 600 * MiB, 50 * MiB}); });
                answer = "?";
                actions.request(standalone::sendPerformance, idOf("Bass Split"), [&](const std::string& a) { answer = a; });
                check(wait([&] { return answer != "?"; }) && answer == "Added to Setlist" && processes.pid("oscairn.setlist") > 0, "with room, it starts and takes the send: " + answer);
                check(reserved("oscairn.setlist") == 60 * MiB, "its manifest's reservation counts while it runs");
                // Using more than it reserved: measured, and reserved at that from its next start.
                check(wait([&] { return processes.reservation("oscairn.setlist") > 100 * MiB; }), "its use past its reservation is measured: " + std::to_string(processes.reservation("oscairn.setlist") >> 20) + " MiB");
                check(data.getChildFile("app-memory.json").loadFileAsString().contains("oscairn.setlist"), "and kept in the data folder");
                processes.stop("oscairn.setlist");
                check(wait([&] { return processes.pid("oscairn.setlist") < 0; }) && reserved("oscairn.setlist") == 0, "closed, its reservation is let go");
                const auto measured = processes.reservation("oscairn.setlist");
                answer = "?";
                actions.request(standalone::sendPerformance, idOf("Verse Keys"), [&](const std::string& a) { answer = a; });
                check(wait([&] { return answer != "?"; }) && answer == "Added to Setlist" && reserved("oscairn.setlist") == measured, "started again, it reserves what it was measured using: " + std::to_string(reserved("oscairn.setlist") >> 20) + " MiB");
                processes.stop("oscairn.setlist");
                wait([&] { return processes.pid("oscairn.setlist") < 0; });
                check(playing(), "sound went on");
                engine.edit([](Control& c) { c.setBudget({}); c.setAlwaysAvailable({}); });
            }

            // A slot whose load takes 6 s (the test fixture's state, slow once): the engine answers
            // everyone meanwhile, so the app lives on; two more steps during it open the last one
            // only; the app killed during another such load is started again and comes back.
            if(const char* fixture = std::getenv("OSCAIRN_TEST_FIXTURE"))
            {
                engine.edit([&](Control& c) {
                    const auto slow = c.add("slow", fixture, "org.kick.test.synth");
                    c.connectNotes(graphInput, slow); c.connectAudio(slow, graphOutput); c.commit();
                    c.setPerformanceInfo({"", "Slow One", "", {}}); c.savePerformanceToLibrary("Slow One", true);
                });
                check(wait([&] { return (library = engine.library(all)) && library->items.size() == 4; }), "the slow one in the library");
                // The setlist ends ... Slow One, Verse Keys, Big Strings; the app starts on the slot before them.
                std::vector<standalone::AppProcesses::Notice> notices;
                standalone::AppProcesses processes(engine, actions, apps, {themes});
                processes.onNotice = [&](const standalone::AppProcesses::Notice& n) { notices.push_back(n); std::fprintf(stderr, "  notice: %s\n", n.text.toRawUTF8()); };
                processes.setFront("oscairn.setlist");
                for(const auto* name : {"Slow One", "Verse Keys", "Big Strings"})
                {
                    std::string answer = "?";
                    actions.request(standalone::sendPerformance, idOf(name), [&](const std::string& a) { answer = a; });
                    check(wait([&] { return answer == "Added to Setlist"; }), std::string("sent ") + name);
                }
                processes.stop("oscairn.setlist");
                wait([&] { return processes.pid("oscairn.setlist") < 0; });
                const auto slots = int(standalone::slotsOf(savedSetlist()).size());
                const auto stateFile = data.getChildFile("AppData/oscairn.setlist/state.json");
                const auto state = juce::JSON::parse(stateFile);
                check(state.getDynamicObject() != nullptr, "the app kept its state");
                if(auto* o = state.getDynamicObject()) { o->setProperty("current", slots - 4); stateFile.replaceWithText(juce::JSON::toString(state)); }
                const auto marker = data.getChildFile("slow-once");
                ::setenv("KICK_FIXTURE_SLOW_LOAD_ONCE", marker.getFullPathName().toRawUTF8(), 1);
                LocalClient probe; probe.connect(socket.string());
                auto busy = [&] { try { return probe.call("session.status", "{}", 1000).find("-32009") != std::string::npos; } catch(const std::exception&) { return false; } };
                std::string answer = "?";
                auto step = [&](const char* by) { answer = "?"; actions.request(standalone::stepSetlist, by, [&](const std::string& a) { answer = a; }); return wait([&] { return answer != "?"; }) ? answer : std::string("?"); };
                auto said = [&](const juce::String& text) { return std::any_of(notices.begin(), notices.end(), [&](const auto& n) { return n.text.contains(text); }); };
                processes.setFront("oscairn.setlist"); // stopped above, which put it behind
                notices.clear();
                marker.replaceWithText("once");
                check(step("1") == idOf("Slow One"), "next, starting the app on the slot before: the slow one");
                const auto pid = processes.pid("oscairn.setlist");
                check(wait([&] { return busy(); }, 5000), "the engine is busy opening it");
                check(step("1") == idOf("Verse Keys") && step("1") == idOf("Big Strings") && busy(), "two more steps during the load answer at once");
                std::string opened;
                check(wait([&] { if(const auto i = engine.performanceInfo(); i) opened = i->name; return opened == "Big Strings" && !busy(); }, 30000), "the last chosen opened: " + opened);
                Control::LibraryQuery recent; recent.kind = "performance"; recent.recent = true;
                uint32_t slowRank = 0, versesRank = 0;
                if(const auto r = engine.library(recent)) for(const auto& i : r->items) { if(i.name == "Slow One") slowRank = i.recent; if(i.name == "Verse Keys") versesRank = i.recent; }
                check(slowRank == 2 && versesRank > slowRank, "the one between was never opened: Slow One " + std::to_string(slowRank) + ", Verse Keys " + std::to_string(versesRank));
                check(processes.pid("oscairn.setlist") == pid && processes.state("oscairn.setlist") == standalone::AppProcesses::State::Running && notices.empty(),
                      "the app lived through the load, with nothing said");
                check(!processes.log("oscairn.setlist").contains("connection closed"), "its connection never closed");
                // Back to the slow one, killed while it loads: started again, it gets its connection and comes back.
                marker.replaceWithText("once");
                check(step("-1") == idOf("Verse Keys") && step("-1") == idOf("Slow One"), "back two slots, to the slow one");
                check(wait([&] { return busy(); }, 5000), "the engine is busy opening it again");
                const auto killed = processes.pid("oscairn.setlist");
                entry = "?";
                ::kill(killed, SIGKILL);
                check(wait([&] { return processes.pid("oscairn.setlist") > 0 && processes.pid("oscairn.setlist") != killed; }, 3000) && busy(), "started again during the load: " + std::to_string(processes.pid("oscairn.setlist")));
                check(wait([&] { return entry == idOf("Slow One"); }, 30000) && processes.state("oscairn.setlist") == standalone::AppProcesses::State::Running, "it got its connection and came back with its state");
                check(!said("did not start") && !said("not responding"), "never 'did not start', nor 'not responding'");
                check(playing(), "sound went on");
                if(failures) std::fprintf(stderr, "  its log:\n%s\n", processes.log("oscairn.setlist").getLastCharacters(6000).toRawUTF8());
                ::unsetenv("KICK_FIXTURE_SLOW_LOAD_ONCE");
                processes.stop("oscairn.setlist");
                wait([&] { return processes.pid("oscairn.setlist") < 0; });
            }
            else std::puts("skipped: a slow load (set OSCAIRN_TEST_FIXTURE to the test fixture)");

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
                const int connection = engine.appConnection("oscairn.setlist", apps[0].permissions);
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
