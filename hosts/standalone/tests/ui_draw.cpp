// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What the device's screens draw (RFC 0045: on the FANTOM-07 the app's drawing slows the audio
// in other processes), counted by PaintCounter: the screens as the app shows them on that
// profile's 1280 x 720 panel (its scale, the panel's layout under Home and the apps' frames; the
// Setlist and Settings in windows of their own, as their processes have), against a real engine
// session over the control socket without an audio device. Each screen is left untouched, when
// it draws nothing; then the Library's list scrolls. Prints a line for each.
//   oscairn_ui_draw_test PLUGIN_DIR SCANNER THEMES LAYOUT PROFILE
#include "../Launcher.h"
#include "../LibraryPane.h"
#include "../PaintCounter.h"
#include "../Panel.h"
#include "../PartsModel.h"
#include "../PlayView.h"
#include "../Profile.h"
#include "../Setlist.h"
#include "../apps/settings/Frame.h"
#include "oscairn/local_socket.h"
#include "oscairn/session.h"
#include <juce_gui_extra/juce_gui_extra.h>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <thread>
#include <unistd.h>
#if defined(__linux__)
 #include <dlfcn.h>
#endif

using namespace oscairn;

int main(int argc, char** argv)
{
    if(argc < 6) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } };
    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-draw-" + std::to_string(::getpid()) + ".sock");
    const auto data = juce::File::createTempFile("oscairn-draw-data"); // a library of its own
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    {
        auto session = std::make_unique<Session>(48000, 128);
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true;
        std::atomic<int> render = 0;
        // No audio is rendered but the blocks asked for (into memory, on this thread): nothing the
        // screens show changes unless they are touched.
        std::thread control([&] {
            std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                for(; render > 0; --render) session->process(nullptr, 0, out, 2, 128);
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset();
        });
        {
            standalone::Engine engine(argv[1], socket.string());
            const auto configuration = juce::File::createTempFile("oscairn-draw-config");
            standalone::Theme theme(configuration, juce::File(argv[3]));
#if JUCE_LINUX
            // As JUCE does for an application, not for a test: an X error (an atom only a window
            // manager makes, without one) does not end it. Xlib is JUCE's, loaded by it.
            if(auto* x = dlopen("libX11.so.6", RTLD_LAZY | RTLD_NOLOAD))
                if(auto* set = reinterpret_cast<void* (*)(int (*)(void*, void*))>(dlsym(x, "XSetErrorHandler"))) set([](void*, void*) { return 0; });
#endif
            const auto profile = standalone::loadProfile(argv[5]);
            const auto& screen = *profile.screen();
            theme.setScreenTheme(screen.theme);
            standalone::setDeviceScale(screen.scale());
            auto pump = [](int ms) { const auto end = juce::Time::getMillisecondCounter() + juce::uint32(ms); while(juce::Time::getMillisecondCounter() < end) juce::MessageManager::getInstance()->runDispatchLoopUntil(5); };
            auto wait = [](auto done) { for(int i = 0; i < 1000 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };

            // A part playing the Basic Synth, as a Performance has; then items enough to scroll.
            const auto plugins = standalone::bundledPlugins(argv[1], argv[2]);
            engine.edit([&](Control& c) {
                const auto part = standalone::addPart(c, "Piano", false);
                for(const auto& p : plugins) if(p.name == "Basic Synth") standalone::insertPlugin(c, part, 0, p);
                c.addScene("Verse");
                for(int i = 0; i < 120; ++i)
                {
                    c.setPerformanceInfo({"", "Performance " + std::to_string(1000 + i), "for the test's list", {"keys", i % 2 ? "live" : "studio"}, i % 3 ? "" : "#3e63dd"});
                    c.savePerformanceToLibrary("", true);
                }
                c.setPerformanceInfo({"", "Untitled", "", {}});
            });
            check(wait([&] { return engine.graph().parts.size() == 1; }) && engine.editError().empty(), "the Performance is made: " + engine.editError());

            // The shell's window: the panel's layout filling it, the screen's view over it.
            const auto layout = standalone::Layout::parse(juce::File(argv[4]).loadFileAsString());
            const int width = juce::roundToInt(screen.width / screen.scale()), height = juce::roundToInt(screen.height / screen.scale());
            juce::Component window;
            window.setOpaque(true);
            standalone::setDense(window);
            standalone::Panel panel(engine, layout);
            panel.setDesignSize(width, height);
            window.addAndMakeVisible(panel);
            window.setBounds(0, 0, width, height);
            panel.setBounds(window.getLocalBounds());
            window.addToDesktop(juce::ComponentPeer::windowIsTemporary);
            window.setVisible(true);
            standalone::PaintCounter counter;

            // Left alone for a while, then counted: frames, pixels (the window's: the panel's 1280 x 720,
            // also on macOS, where JUCE paints a window that is on no screen, as the test's hidden ones
            // are, at one pixel a point) and painting time a second.
            // OSCAIRN_TEST_X_PID (Linux): the X server's CPU time over each count too, in percent of a core.
            const int xPid = juce::SystemStats::getEnvironmentVariable("OSCAIRN_TEST_X_PID", "0").getIntValue();
            auto xTicks = [&] {
                const auto fields = juce::StringArray::fromTokens(juce::File("/proc/" + juce::String(xPid) + "/stat").loadFileAsString().fromLastOccurrenceOf(")", false, false), true);
                return fields.size() > 12 ? fields[11].getLargeIntValue() + fields[12].getLargeIntValue() : juce::int64(0); // utime and stime, after the name
            };
            juce::int64 xStart = 0;
            struct Rate { double frames, pixels, ms, x; };
            auto rate = [&](const standalone::PaintCounter::Counts& c, double seconds) {
                return Rate{c.frames / seconds, double(c.pixels) / seconds, c.ms / seconds, xPid ? double(xTicks() - xStart) / seconds : 0.0}; // ticks of 10 ms: percent
            };
            auto start = [&] { counter.watch(); counter.reset(); if(xPid) xStart = xTicks(); };
            auto measure = [&](juce::Component& top, int ms) {
                start();
                pump(ms);
                return rate(counter.counts(top), ms / 1000.0);
            };
            auto report = [&](const char* what, const Rate& r) {
                std::printf("%-26s %6.1f frames/s %10.0f pixels/s (%5.1f%% of the screen) %7.2f ms/s", what, r.frames, r.pixels, 100.0 * r.pixels / (screen.width * screen.height), r.ms);
                if(xPid) std::printf(", X server %.0f%%", r.x);
                std::printf("\n");
            };
            auto atRest = [&](const char* what, juce::Component& top) {
                pump(1500); // its first readings and layouts
                const auto r = measure(top, 3000);
                report(what, r);
                check(r.frames == 0, std::string(what) + ": nothing changes, so nothing is drawn");
                return r;
            };
            standalone::TopBar::Shell shell;
            shell.home = [] {}; shell.back = [] {}; shell.apps = [] {}; shell.notesOff = [] {}; shell.save = [](bool) {}; shell.record = true; // as the app's barShell

            // Home.
            {
                juce::String library = "121 Performances";
                std::vector<standalone::Launcher::App> apps{
                    {"Performance", "piano", [] { return juce::String("Untitled: play or edit it"); }, [] {}},
                    {"Groovebox", "layout-grid", [] { return juce::String("Patterns for the open Performance"); }, [] {}},
                    {"Library", "library-big", [&] { return library; }, [] {}},
                    {"Setlist", "list-music", [] { return juce::String("Songs in order, for the stage"); }, [] {}},
                    {"Settings", "settings", [] { return juce::String("Audio, MIDI and more"); }, [] {}}};
                standalone::Launcher home(engine, apps, [&] { auto s = shell; s.home = {}; s.back = {}; return s; }());
                standalone::setDesktopMode(home, 2);
                panel.overlay(&home);
                atRest("Home, at rest", window);
                // A value that changes: only it is drawn again, not the bar or the tiles under it.
                // The tempo in the top bar (the engine's transport, which a block reports)...
                start();
                engine.setTempo(133);
                pump(100); render = 4; pump(1000);
                auto c = counter.counts(window);
                const double bar = screen.width * standalone::TopBar::heightFor(home) * screen.scale();
                std::printf("%-26s %6d frames %10lld pixels (the top bar is %.0f)\n", "Home, the tempo changed", c.frames, (long long) c.pixels, bar);
                check(c.frames > 0 && c.pixels < bar / 4, "the tempo changed: only it is drawn again");
                // ...and a tile's line of state.
                start();
                library = "122 Performances";
                pump(1500);
                c = counter.counts(window);
                std::printf("%-26s %6d frames %10lld pixels (a tile is %.0f)\n", "Home, a tile's state", c.frames, (long long) c.pixels, 200.0 * 120 * screen.scale() * screen.scale());
                check(c.frames > 0 && c.pixels < 200.0 * 120 * screen.scale() * screen.scale() / 3, "a tile's state changed: only its line is drawn again");
                panel.overlay(nullptr);
            }

            // The Library, at rest, then its list scrolled a little at a time at 60 Hz, as a finger drags it.
            {
                standalone::Actions actions;
                standalone::LibraryPane library(engine, actions, standalone::LibraryPane::Mode::Play);
                standalone::AppFrame frame(engine, "Library", library, shell);
                standalone::setDesktopMode(frame, 2);
                panel.overlay(&frame);
                check(wait([&] { return library.items().size() == 120; }), "the Library lists the test's Performances");
                atRest("Library, at rest", window);
                auto* box = [&]() -> juce::ListBox* { for(auto* c : library.getChildren()) if(auto* b = dynamic_cast<juce::ListBox*>(c)) return b; return nullptr; }();
                check(box != nullptr && box->getViewport() != nullptr, "the Library's list");
                if(box)
                {
                    start();
                    const auto began = juce::Time::getMillisecondCounterHiRes();
                    for(int step = 1; step <= 120; ++step)
                    {
                        box->getViewport()->setViewPosition(0, step * 6);
                        while(juce::Time::getMillisecondCounterHiRes() < began + step * 1000.0 / 60) juce::MessageManager::getInstance()->runDispatchLoopUntil(1);
                    }
                    const auto seconds = (juce::Time::getMillisecondCounterHiRes() - began) / 1000.0;
                    const auto c = counter.counts(window);
                    report("Library, scrolling", rate(c, seconds));
                    const auto view = box->getViewport()->getScreenBounds();
                    const auto area = view.getWidth() * double(view.getHeight()) * screen.scale() * screen.scale();
                    std::printf("%-26s %6.0f pixels a frame; the list's view is %.0f\n", "", c.frames ? double(c.pixels) / c.frames : 0.0, area);
                }
                panel.overlay(nullptr);
            }

            // The Performance app: Play.
            {
                standalone::PerformanceApp app(engine, argv[2]);
                standalone::AppFrame frame(engine, "Performance", app, shell);
                app.attach(frame.bar(), [] {});
                standalone::setDesktopMode(frame, 2);
                panel.overlay(&frame);
                atRest("Performance, at rest", window);
                panel.overlay(nullptr);
            }
            window.removeFromDesktop();

            // The Setlist and Settings: processes of their own, each with a window over the shell's frame.
            auto own = [&](juce::Component& view) {
                standalone::setDense(view);
                standalone::setDesktopMode(view, 2);
                view.setBounds(0, 0, width, height - standalone::TopBar::heightFor(window));
                view.addToDesktop(juce::ComponentPeer::windowIsTemporary);
                view.setVisible(true);
                counter.watch(); // now: on macOS the change of renderer paints it all again
            };
            {
                standalone::Actions actions;
                standalone::SetlistPane setlist(engine, actions);
                setlist.create("Saturday");
                own(setlist);
                atRest("Setlist, at rest", setlist);
                setlist.removeFromDesktop();
            }
            {
                standalone::settings::Frame settings(engine, profile);
                own(settings);
                atRest("Settings, at rest", settings);
                settings.removeFromDesktop();
            }
            configuration.deleteRecursively();
        }
        running = false;
        control.join();
    }
    std::filesystem::remove(socket);
    data.deleteRecursively();
    if(!failures) std::puts("ok: the device's screens draw what they should");
    return failures ? 1 : 0;
}
