// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Phase 3b's "done when" (docs/roadmap.md), scripted end to end on the Panel layout (1024 × 600,
// touch) against a real engine session over the control socket, with every plugin in a worker
// and no audio or MIDI device: a Performance with two parts layered on the low keys and a third
// split above them, a bus of shared effects the parts send to, two scenes and a control tree
// across parts is built, played, edited through Oscairn's own editors and undone; a layered
// part's plugin is killed mid-play while the others keep sounding without a silent block, and
// it comes back with its parameters; then the Performance is saved and reopened. Every edit goes
// through the Parts views (their buttons, sliders, menus picked as a screen reader picks them,
// rows and boxes tapped, the keyboard and the rich views dragged). Past them: the notes, played
// into the session as the keys would; each part recorded through its tap (RFC 0020) to measure
// it block by block; Save wired as the app wires it; and reopening asked of the engine as the
// Library app does. What the views lack at this size is printed as a GAP line.
//
// Plugins: the in-repo ones (the views test plugin, made audible, as "FM"; the Basic Synth as
// "Analog" and the upper part; Delay on the bus), or with --engines the real engines from the
// folder OSCAIRN_PHASE3B_ENGINES names (oscairn-fm.clap, oscairn-analog.clap and
// oscairn-effects.clap: FX Reverb and FX Delay on the bus), skipped (77) when it is unset.
// With SNAPSHOTS, each screen is written to SNAPSHOTS-<name>.png.
//   oscairn_phase3b_test PLUGIN_DIR SCANNER THEMES [SNAPSHOTS] [--engines]
#include "../ParameterGrid.h"
#include "../PartsView.h"
#include "../PlayView.h"
#include "../Theme.h"
#include "../ZoneEditor.h"
#include "oscairn/local_socket.h"
#include "oscairn/session.h"
#include <juce_audio_formats/juce_audio_formats.h>
#include <juce_gui_extra/juce_gui_extra.h>
#include <array>
#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <deque>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <thread>
#include <unistd.h>
#ifdef __APPLE__
#include <libproc.h>
#endif

using namespace oscairn;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { std::fprintf(stderr, "%s: %s\n", ok ? "ok" : "FAIL", what.c_str()); if(!ok) ++failures; }
    [[maybe_unused]] void gap(const std::string& what) { std::fprintf(stderr, "GAP: %s\n", what.c_str()); }
    std::string num(double v) { char b[32]; std::snprintf(b, sizeof b, "%.4g", v); return b; }

    // The worker process that has a plugin's binary loaded: a child of this one (the engine runs
    // here), found by its mapped files, so killing it is a plugin crashing.
    std::vector<pid_t> workersWith(const std::string& file)
    {
        std::vector<pid_t> found;
#ifdef __APPLE__
        std::vector<pid_t> pids(4096);
        const int n = proc_listallpids(pids.data(), int(pids.size() * sizeof(pid_t)));
        for(int i = 0; i < n; ++i)
        {
            proc_bsdinfo bsd{};
            if(proc_pidinfo(pids[size_t(i)], PROC_PIDTBSDINFO, 0, &bsd, sizeof bsd) != sizeof bsd || pid_t(bsd.pbi_ppid) != ::getpid()) continue;
            proc_regionwithpathinfo region{};
            for(uint64_t address = 0; proc_pidinfo(pids[size_t(i)], PROC_PIDREGIONPATHINFO, address, &region, sizeof region) == sizeof region;
                address = region.prp_prinfo.pri_address + region.prp_prinfo.pri_size)
                if(std::string(region.prp_vip.vip_path).find(file) != std::string::npos) { found.push_back(pids[size_t(i)]); break; }
        }
#else
        for(const auto& e : std::filesystem::directory_iterator("/proc"))
        {
            const auto name = e.path().filename().string();
            if(name.find_first_not_of("0123456789") != std::string::npos) continue;
            std::ifstream stat(e.path() / "stat"); std::string line; std::getline(stat, line);
            const auto close = line.rfind(')');
            if(close == std::string::npos) continue;
            int ppid = 0; char state = 0;
            if(std::sscanf(line.c_str() + close + 1, " %c %d", &state, &ppid) != 2 || ppid != ::getpid()) continue;
            std::ifstream maps(e.path() / "maps");
            for(std::string m; std::getline(maps, m);) if(m.find(file) != std::string::npos) { found.push_back(pid_t(std::stoi(name))); break; }
        }
#endif
        return found;
    }

    template<class T> std::vector<T*> all(juce::Component& in, std::function<bool(T&)> want = {})
    {
        std::vector<T*> out;
        std::function<void(juce::Component&)> look = [&](juce::Component& c) {
            for(auto* child : c.getChildren())
            {
                if(auto* t = dynamic_cast<T*>(child); t && child->isVisible() && (!want || want(*t))) out.push_back(t);
                look(*child);
            }
        };
        look(in);
        return out;
    }
    // A rich view by name (not the page button of that name).
    juce::Component* named(juce::Component& in, const juce::String& name) { const auto f = all<juce::Component>(in, [&](juce::Component& c) { return c.getName() == name && !dynamic_cast<juce::Button*>(&c); }); return f.empty() ? nullptr : f.front(); }
    juce::Button* button(juce::Component& in, const juce::String& text)
    {
        const auto f = all<juce::Button>(in, [&](juce::Button& b) { return b.getButtonText() == text || (b.getButtonText().isEmpty() && b.getTitle() == text); });
        return f.empty() ? nullptr : f.front();
    }

    // A popup menu the app shows: its item by text, pressed as a screen reader presses it, each
    // name opening the next submenu. True when the last was pressed.
    bool pick(const std::vector<juce::String>& path)
    {
        for(const auto& want : path)
        {
            juce::AccessibilityHandler* item = nullptr;
            auto& desktop = juce::Desktop::getInstance();
            for(int i = desktop.getNumComponents(); --i >= 0 && !item;)
            {
                std::function<void(juce::Component&)> look = [&](juce::Component& c) {
                    for(auto* child : c.getChildren())
                    {
                        if(item) return;
                        if(auto* h = child->getAccessibilityHandler(); h && h->getRole() == juce::AccessibilityRole::menuItem && h->getTitle() == want) item = h;
                        look(*child);
                    }
                };
                look(*desktop.getComponent(i));
            }
            if(!item)
            {
                std::fprintf(stderr, "  menu: no item %s among", want.toRawUTF8());
                for(int i = 0; i < desktop.getNumComponents(); ++i)
                {
                    std::function<void(juce::Component&)> list = [&](juce::Component& c) { for(auto* child : c.getChildren()) { if(auto* h = child->getAccessibilityHandler()) std::fprintf(stderr, " [%s]", h->getTitle().toRawUTF8()); list(*child); } };
                    list(*desktop.getComponent(i));
                }
                std::fputc('\n', stderr);
                juce::PopupMenu::dismissAllActiveMenus(); return false;
            }
            item->getActions().invoke(juce::AccessibilityActionType::press);
        }
        return true;
    }
    void drag(juce::Component* c, juce::Point<float> from, juce::Point<float> to)
    {
        auto mouse = juce::Desktop::getInstance().getMainMouseSource();
        const auto now = juce::Time::getCurrentTime();
        auto event = [&](juce::Point<float> at, bool dragged) { return juce::MouseEvent(mouse, at, juce::ModifierKeys::leftButtonModifier, 1, 0, 0, 0, 0, c, c, now, from, now, 1, dragged); };
        c->mouseDown(event(from, false)); if(to != from) c->mouseDrag(event(to, true)); c->mouseUp(event(to, to != from));
    }
}

int main(int argc, char** argv)
{
    if(argc < 4) return 2;
    const bool engines = std::string(argv[argc - 1]) == "--engines";
    if(engines) --argc;
    const char* engineDir = std::getenv("OSCAIRN_PHASE3B_ENGINES");
    if(engines && (!engineDir || !*engineDir)) { std::puts("skipped: OSCAIRN_PHASE3B_ENGINES names no folder of engines"); return 77; }
    const char* views = std::getenv("OSCAIRN_TEST_VIEWS");
    if(!engines && !views) { std::puts("OSCAIRN_TEST_VIEWS is not set"); return 2; }
    if(!std::getenv("OSCAIRN_PLUGIN_WORKER")) { std::puts("OSCAIRN_PLUGIN_WORKER is not set: the crash needs plugins in workers"); return 2; }
    juce::ScopedJuceInitialiser_GUI gui;
    const auto pid = std::to_string(::getpid());
    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-3b-" + pid + ".sock");
    // A data folder and a plugin folder of its own: the user's library is never read or written.
    const auto data = juce::File::createTempFile("oscairn-3b-data");
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    const auto pluginDir = std::filesystem::temp_directory_path() / ("oscairn-3b-plugins-" + pid);
    std::filesystem::create_directories(pluginDir);
    const std::filesystem::path in(argv[1]);
    const std::vector<std::filesystem::path> files = engines
        ? std::vector<std::filesystem::path>{std::filesystem::path(engineDir) / "oscairn-fm.clap", std::filesystem::path(engineDir) / "oscairn-analog.clap", std::filesystem::path(engineDir) / "oscairn-effects.clap"}
        : std::vector<std::filesystem::path>{views, in / "oscairn-basic-synth.clap", in / "oscairn-delay.clap"};
    for(const auto& f : files) std::filesystem::create_symlink(std::filesystem::absolute(f), pluginDir / f.filename());
    // What each role plays, by plugin ID, and the file a crash kills.
    const std::string fmId = engines ? "org.oscairn.fm" : "org.oscairn.test.views", analogId = engines ? "org.oscairn.analog" : "org.oscairn.basic-synth";
    const std::vector<std::string> effectIds = engines ? std::vector<std::string>{"org.oscairn.effects.reverb", "org.oscairn.effects.delay"} : std::vector<std::string>{"org.oscairn.delay"};
    const std::string fmFile = files[0].filename().string();
    const double loud = 1e-4; // a block's peak above this sounds
    {
        auto session = std::make_unique<Session>(48000, 128);
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true;
        std::atomic<uint64_t> rendered = 0; // frames, the engine's timeline
        std::mutex mutex;
        std::deque<std::pair<int, bool>> notes; // keys to press (true) or release, in order
        // The audio thread's part, on the control thread as in the other desktop tests.
        std::thread control([&] {
            std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                { std::lock_guard lock(mutex); for(const auto& [key, on] : notes) session->note(on, key, .8); notes.clear(); }
                session->process(nullptr, 0, out, 2, 128);
                rendered += 128;
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset(); // its plugins go on the thread that made them
        });
        auto play = [&](int key, bool on) { std::lock_guard lock(mutex); notes.emplace_back(key, on); };
        {
            standalone::Engine engine(pluginDir.string(), socket.string());
            const auto configuration = juce::File::createTempFile("oscairn-3b-config");
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            theme.apply("oscairn-dark");
            auto pump = [](int ms) { const auto until = juce::Time::getMillisecondCounter() + uint32_t(ms); while(juce::Time::getMillisecondCounter() < until) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); };
            // The Panel: the Parts views on a 1024 × 600 touch screen, as the Performance app's Edit.
            standalone::PartsView view(engine, argv[2]);
            standalone::setDesktopMode(view, 2); view.setSize(1024, 600);
            // On the desktop, off the screen, so it runs as it does showing (its timer, its plugin list).
            view.addToDesktop(juce::ComponentPeer::windowIsTemporary); view.setTopLeftPosition(-6000, -6000); view.setVisible(true);
            auto wait = [&](auto done, int seconds = 10) {
                const auto until = juce::Time::getMillisecondCounter() + uint32_t(seconds * 1000);
                for(;;) { view.refresh(); if(done() || juce::Time::getMillisecondCounter() >= until) break; juce::MessageManager::getInstance()->runDispatchLoopUntil(10); }
                return done();
            };
            auto snapshot = [&](juce::Component& c, const juce::String& name) {
                if(argc < 5) return;
                pump(300); view.refresh();
                const juce::File out{juce::String(argv[4]) + "-" + name + ".png"};
                out.deleteFile(); juce::FileOutputStream stream(out);
                juce::PNGImageFormat().writeImageToStream(c.createComponentSnapshot(c.getLocalBounds()), stream);
            };
            // Click a button the view shows, as a tap does (at once, so a menu it opens can be picked).
            auto tap = [&](juce::Button* b, const std::string& what) { check(b != nullptr, "the view shows " + what); if(b && b->onClick) b->onClick(); };
            auto graph = [&] { return engine.graph(); };
            auto partNamed = [&](const std::string& name) -> std::optional<Part> { for(const auto& p : graph().parts) if(p.name == name) return p; return std::nullopt; };
            auto instrument = [&](const std::string& part) -> NodeId { const auto p = partNamed(part); return p && !p->chain.empty() ? p->chain.front() : 0; };
            auto nodeName = [&](NodeId n) { const auto g = graph(); return g.names.count(n) ? g.names.at(n) : std::string(); };
            auto values = [&](NodeId n) { std::map<uint32_t, double> v; const auto name = nodeName(n); for(const auto& p : engine.parameters(n)) v[p.id] = engine.param(name, p.id); return v; };
            auto changed = [&](NodeId n, const std::map<uint32_t, double>& before) {
                std::vector<std::string> out; const auto name = nodeName(n);
                for(const auto& p : engine.parameters(n)) if(const auto at = before.find(p.id); at != before.end() && std::abs(engine.param(name, p.id) - at->second) > 1e-9) out.push_back((p.module.empty() ? "" : p.module + "/") + p.name + " " + num(at->second) + " -> " + num(engine.param(name, p.id)));
                return out;
            };
            auto join = [](const std::vector<std::string>& v) { std::string s; for(const auto& x : v) s += (s.empty() ? "" : "; ") + x; return s; };
            check(wait([&] { return engine.connected(); }), "the engine is connected");
            // Taps on the view itself (its rows and chain boxes), and the pages they reach.
            auto tapAt = [&](juce::Point<float> at) {
                auto mouse = juce::Desktop::getInstance().getMainMouseSource();
                const auto when = juce::Time::getCurrentTime();
                const juce::MouseEvent e(mouse, at, juce::ModifierKeys::leftButtonModifier, 1, 0, 0, 0, 0, &view, &view, when, at, when, 1, false);
                view.mouseDown(e); view.mouseUp(e);
                wait([&] { return false; }, 0);
            };
            const auto editScene = juce::String::fromUTF8("Edit…"); // the selected scene's menu: Store, Rename, Scope, Transition
            auto toList = [&] { if(view.page() != 0) tap(button(view, juce::String::fromUTF8("‹ Parts")), "‹ Parts"); wait([&] { return view.page() == 0; }); };
            // A part's row, as the list lays them out (rows of 60 under the scenes and the column titles; the buses after a gap).
            auto openPart = [&](uint32_t id) {
                toList();
                const auto state = standalone::readParts(graph());
                int row = 0, at = -1;
                for(const auto* list : {&state.parts, &state.buses}) for(const auto& p : *list) { if(p.id == id) at = row; ++row; }
                const int gapAbove = at >= int(state.parts.size()) ? 36 : 0;
                tapAt({60.f, 56 + 48 + 28 + at * 60 + gapAbove + 27.f});
                check(view.page() == id, "a tap on its row opens the part");
            };
            auto toControls = [&] { toList(); tap(button(view, "Controls"), "Controls"); wait([&] { return view.page() == standalone::PartsView::controlsPage; }); };

            // ---- 1. Build ----
            // Three parts and a bus from the list's buttons; each part named in its page's name field.
            for(int i = 0; i < 3; ++i) { tap(button(view, "+ Part"), "+ Part"); check(wait([&] { return graph().parts.size() == size_t(i + 1); }), "+ Part adds part " + std::to_string(i + 1)); }
            tap(button(view, "+ Bus"), "+ Bus");
            check(wait([&] { return graph().parts.size() == 4; }), "+ Bus adds a bus");
            std::vector<uint32_t> ids; for(const auto& p : graph().parts) ids.push_back(p.id);
            const std::array<std::string, 4> names{"FM", "Analog", "Upper", "FX"};
            for(size_t i = 0; i < 4; ++i)
            {
                openPart(ids[i]);
                auto editors = all<juce::TextEditor>(view);
                check(!editors.empty(), "the part page has its name field");
                if(editors.empty()) continue;
                editors.front()->setText(names[i]); if(editors.front()->onReturnKey) editors.front()->onReturnKey();
                check(wait([&] { return partNamed(names[i]).has_value(); }), "named " + names[i] + " on its page");
            }
            // Each part's instrument and the bus's effects, from the Part page's + menu.
            const auto plugins = standalone::bundledPlugins(pluginDir.string(), argv[2]);
            auto choice = [&](const std::string& id) { for(const auto& p : plugins) if(p.id == id) return p; return standalone::PluginChoice{}; };
            check(!choice(fmId).id.empty() && !choice(analogId).id.empty(), "the plugins are found: " + std::to_string(plugins.size()));
            wait([&] { return false; }, 1); // the view's own scan, which its menu lists
            auto insert = [&](const std::string& part, size_t at, const std::string& id) {
                openPart(partNamed(part)->id);
                wait([&] { return false; }, 0);
                auto plus = all<juce::Button>(view, [](juce::Button& b) { return b.getTitle() == "Insert"; });
                check(plus.size() > at, part + ": the chain's + buttons");
                if(plus.size() <= at) return;
                const auto count = partNamed(part)->chain.size();
                tap(plus[at], "+");
                check(pick({juce::String::fromUTF8(choice(id).name.c_str())}), part + ": the + menu lists " + choice(id).name);
                check(wait([&] { return partNamed(part)->chain.size() > count; }, 20) && engine.editError().empty(), part + " gets " + choice(id).name + " " + engine.editError());
            };
            insert("FM", 0, fmId); insert("Analog", 0, analogId); insert("Upper", 0, analogId);
            for(size_t i = 0; i < effectIds.size(); ++i) insert("FX", i, effectIds[i]);
            const NodeId fm = instrument("FM"), analog = instrument("Analog"), upper = instrument("Upper");
            check(fm && analog && upper && partNamed("FX")->chain.size() == effectIds.size(), "every part has its plugins");
            // FM and Analog layered on the keys below middle C; Upper above them: each part's keyboard dragged.
            auto zone = [&](const std::string& part, bool high, int key) {
                openPart(partNamed(part)->id);
                auto keyboards = all<standalone::ZoneEditor>(view);
                check(!keyboards.empty(), part + ": the part page's keyboard");
                if(keyboards.empty()) return;
                auto* z = keyboards.front();
                const auto area = z->getLocalBounds().toFloat().withTrimmedTop(18).withTrimmedBottom(26); // keys above velocities
                const float w = area.getWidth() / 75; // white keys
                int whites = 0; for(int k = 0; k < key; ++k) whites += !standalone::ZoneEditor::black(k);
                const float y = area.getBottom() - 4, edge = high ? area.getRight() - 1 : area.getX() + 1;
                drag(z, {edge, y}, {area.getX() + (whites + .5f) * w, y});
            };
            zone("FM", true, 59); zone("Analog", true, 59); zone("Upper", false, 60);
            check(wait([&] { const auto a = partNamed("FM"), b = partNamed("Analog"), c = partNamed("Upper");
                             return a->input.zones.size() == 1 && a->input.zones[0].highKey == 59 && a->input.zones[0].lowKey == 0 && b->input.zones.size() == 1 && b->input.zones[0].highKey == 59 &&
                                    c->input.zones.size() == 1 && c->input.zones[0].lowKey == 60 && c->input.zones[0].highKey == 127; }),
                  "FM and Analog layered on keys 0-59, Upper split above on 60-127, by dragging each keyboard");
            // Sends to the bus from the parts list: each part row's slider under the bus's column.
            toList();
            {
                auto sliders = all<juce::Slider>(view);
                std::sort(sliders.begin(), sliders.end(), [](auto* a, auto* b) { return a->getY() != b->getY() ? a->getY() < b->getY() : a->getX() < b->getX(); });
                // Rows top to bottom: FM, Analog, Upper (a level and a send each), then the bus (its level).
                check(sliders.size() == 7, "the list shows a level and a send per part and the bus's level: " + std::to_string(sliders.size()));
                if(sliders.size() == 7) for(const int s : {1, 3, 5}) sliders[size_t(s)]->setValue(s == 5 ? .3 : .5, juce::sendNotificationSync);
            }
            const auto bus = partNamed("FX")->id;
            check(wait([&] { const auto s = standalone::readParts(graph()); int n = 0; for(const auto& p : s.parts) n += p.sends.count(bus) && p.sends.at(bus) > .2; return n == 3; }), "each part sends to the shared effects");
            snapshot(view, "1-parts");
            // Two scenes from + Scene; Scene 2 stores FM a little lower.
            tap(button(view, "+ Scene"), "+ Scene"); check(wait([&] { return graph().scenes.size() == 1; }), "+ Scene adds Scene 1");
            tap(button(view, "+ Scene"), "+ Scene"); check(wait([&] { return graph().scenes.size() == 2; }), "+ Scene adds Scene 2");
            const auto scenes = graph().scenes;
            {
                auto sliders = all<juce::Slider>(view);
                std::sort(sliders.begin(), sliders.end(), [](auto* a, auto* b) { return a->getY() != b->getY() ? a->getY() < b->getY() : a->getX() < b->getX(); });
                if(!sliders.empty()) sliders[0]->setValue(.4, juce::sendNotificationSync); // FM's level
                check(wait([&] { return std::abs(partNamed("FM")->level - .4) < 1e-6; }), "FM's level slider");
                // Store is in the selected scene's Edit... menu.
                tap(button(view, editScene), "the selected scene's Edit...");
                check(pick({"Store"}), "Edit... stores the scene");
                // The menu's choice arrives after the menu closes: Scene 2 holds FM at .4 once it has.
                check(wait([&] { const auto sc = graph().scenes; const auto fmId = partNamed("FM")->id; return sc.size() == 2 && std::any_of(sc[1].parts.begin(), sc[1].parts.end(), [&](const auto& p) { return p.part == fmId && std::abs(p.level - .4) < 1e-6; }); }),
                      "Scene 2 stores FM's level .4");
                tap(button(view, "Scene 1"), "Scene 1");
                check(wait([&] { return graph().scene == scenes[0].id && std::abs(partNamed("FM")->level - 1) < 1e-6; }), "Scene 1's button brings FM back to its full level: " + num(partNamed("FM")->level));
                // Its scope and transition, from the same menu.
                const auto scopeWas = graph().scenes[0].scope;
                tap(button(view, editScene), "the selected scene's Edit...");
                check(pick({"Scope", "Mutes and solos"}), "Edit... has the scene's scope");
                check(wait([&] { return graph().scenes[0].scope == (scopeWas ^ Scene::Mutes); }), "and switches what selecting it changes");
                tap(button(view, editScene), "the selected scene's Edit...");
                pick({"Scope", "Mutes and solos"});
                check(wait([&] { return graph().scenes[0].scope == scopeWas; }), "and back");
                tap(button(view, editScene), "the selected scene's Edit...");
                check(pick({"Transition", "250 ms"}), "Edit... has the scene's transition");
                check(wait([&] { return graph().scenes[0].glide == 250; }), "and sets its glide");
                tap(button(view, editScene), "the selected scene's Edit...");
                pick({"Transition", "At once"});
                check(wait([&] { return graph().scenes[0].glide == 0; }), "and back");
                // Each scene's button and the selected one's Edit... whole on the screen, side by
                // side, with two scenes and with six.
                auto fits = [&](size_t count) {
                    std::vector<juce::Rectangle<int>> taken;
                    for(const auto& sc : graph().scenes) if(const auto* b = button(view, juce::String::fromUTF8(sc.name.c_str()))) taken.push_back(b->getBounds());
                    if(const auto* b = button(view, editScene)) taken.push_back(b->getBounds());
                    bool ok = taken.size() == count + 1;
                    for(size_t i = 0; i < taken.size(); ++i)
                    {
                        ok = ok && taken[i].getWidth() >= 44 && view.getLocalBounds().contains(taken[i]);
                        for(size_t j = 0; j < i; ++j) ok = ok && !taken[i].intersects(taken[j]);
                    }
                    check(ok, "the scenes and Edit... fit at 1024 wide with " + std::to_string(count) + " scenes");
                };
                fits(2);
                snapshot(view, "1-scenes");
                for(int n = 3; n <= 6; ++n) { tap(button(view, "+ Scene"), "+ Scene"); wait([&] { return graph().scenes.size() == size_t(n); }); }
                fits(6);
                snapshot(view, "1-six-scenes");
                for(const auto& sc : graph().scenes) if(sc.id != scenes[0].id && sc.id != scenes[1].id) engine.edit([id = sc.id](Control& c) { c.removeScene(id); });
                check(wait([&] { return graph().scenes.size() == 2; }), "the four extra scenes removed");
                tap(button(view, "Scene 1"), "Scene 1");
                check(wait([&] { return graph().scene == scenes[0].id; }), "Scene 1 selected again");
            }
            // The control tree: Control 1 (a Super Knob) drives Control 2 (an Assign knob), which drives a parameter in FM and one in Analog.
            toControls();
            for(int i = 0; i < 2; ++i) { tap(button(view, "+ Control"), "+ Control"); check(wait([&] { return graph().macros.size() == size_t(i + 1); }), "+ Control adds a control"); }
            // A parameter whose name is its plugin's only one of that name, preferring a smooth one.
            auto target = [&](NodeId n) {
                const auto ps = engine.parameters(n);
                auto unique = [&](const Control::ParamInfo& p) { return std::count_if(ps.begin(), ps.end(), [&](const auto& q) { return q.name == p.name; }) == 1 && !(p.flags & CLAP_PARAM_IS_READONLY); };
                for(const auto& p : ps) if(unique(p) && !p.stepped) return p;
                for(const auto& p : ps) if(unique(p)) return p;
                return Control::ParamInfo{};
            };
            if(!fm || !analog || !upper) { std::fprintf(stderr, "FAIL: no plugins to go on with\n"); std::fflush(stderr); std::_Exit(1); } // the engine's thread still runs
            const auto fmTarget = target(fm), analogTarget = target(analog);
            auto plusTarget = [&](size_t row) { auto b = all<juce::Button>(view, [](juce::Button& x) { return x.getButtonText() == "+ Target"; }); std::sort(b.begin(), b.end(), [](auto* a, auto* c) { return a->getY() < c->getY(); }); return row < b.size() ? b[row] : nullptr; };
            tap(plusTarget(0), "Control 1's + Target");
            check(pick({"Another control", "Control 2"}), "Control 1 targets Control 2 from its menu");
            check(wait([&] { return graph().macros[0].targets.size() == 1; }), "Control 1 drives Control 2");
            const auto fmUnset = engine.param(nodeName(fm), fmTarget.id), analogUnset = engine.param(nodeName(analog), analogTarget.id);
            tap(plusTarget(1), "Control 2's + Target");
            check(pick({"Plugin parameter", juce::String::fromUTF8(nodeName(fm).c_str()), juce::String::fromUTF8(fmTarget.name.c_str())}), "Control 2 targets FM's " + fmTarget.name);
            check(wait([&] { return graph().macros[1].targets.size() == 1; }), "Control 2 drives FM");
            tap(plusTarget(1), "Control 2's + Target");
            check(pick({"Plugin parameter", juce::String::fromUTF8(nodeName(analog).c_str()), juce::String::fromUTF8(analogTarget.name.c_str())}), "Control 2 targets Analog's " + analogTarget.name);
            check(wait([&] { return graph().macros[1].targets.size() == 2; }), "Control 2 drives a parameter in two parts");
            // A new target starts from its parameter as it is: adding it moves nothing.
            pump(300);
            check(engine.param(nodeName(fm), fmTarget.id) == fmUnset && engine.param(nodeName(analog), analogTarget.id) == analogUnset,
                  "adding the targets leaves FM's " + fmTarget.name + " at " + num(fmUnset) + " (" + num(engine.param(nodeName(fm), fmTarget.id)) + ") and Analog's " + analogTarget.name + " at " + num(analogUnset) + " (" + num(engine.param(nodeName(analog), analogTarget.id)) + ")");
            snapshot(view, "1-controls");

            // ---- 2. Play ----
            // Each part recorded after its fader (RFC 0020's part taps), so every block of what it
            // plays can be measured: a block sounds when its peak passes `loud`.
            const uint32_t fmPart = partNamed("FM")->id, analogPart = partNamed("Analog")->id, upperPart = partNamed("Upper")->id;
            const std::vector<uint32_t> taps{fmPart, analogPart, upperPart, bus};
            struct Recording { uint64_t start = 0; std::vector<std::vector<float>> peaks; }; // per tap, each block's peak
            auto record = [&] {
                auto take = std::make_shared<uint32_t>(0);
                engine.edit([take, taps](Control& c) {
                    std::vector<Control::RecordTap> list;
                    for(const auto part : taps) { Control::RecordTap t; t.kind = Control::RecordTap::Kind::Part; t.part = part; list.push_back(t); }
                    *take = c.recordStart(list, "Phase 3b");
                });
                check(*take != 0, "a take of each part: " + engine.editError());
                return *take;
            };
            auto finish = [&](uint32_t take) {
                engine.edit([take](Control& c) { c.recordStop(take); });
                Recording out; Control::Take done;
                wait([&] { engine.refreshTakes(); for(const auto& t : engine.takes()) if(t.take == take && t.state == "done") { done = t; return true; } return false; }, 20);
                out.start = done.start;
                juce::WavAudioFormat wav;
                for(const auto& f : done.files)
                {
                    std::vector<float> peaks;
                    if(std::unique_ptr<juce::AudioFormatReader> reader{wav.createReaderFor(new juce::FileInputStream(juce::File(juce::String::fromUTF8((std::filesystem::path(done.folder) / f).string().c_str()))), true)})
                    {
                        juce::AudioBuffer<float> buffer(int(reader->numChannels), int(reader->lengthInSamples));
                        reader->read(&buffer, 0, int(reader->lengthInSamples), 0, true, true);
                        for(int at = 0; at < buffer.getNumSamples(); at += 128)
                        {
                            float most = 0;
                            for(int ch = 0; ch < buffer.getNumChannels(); ++ch) most = std::max(most, buffer.getMagnitude(ch, at, std::min(128, buffer.getNumSamples() - at)));
                            peaks.push_back(most);
                        }
                    }
                    out.peaks.push_back(std::move(peaks));
                }
                check(out.peaks.size() == taps.size() && !out.peaks[0].empty(), "the take is read back: " + std::to_string(out.peaks.size()) + " files, " + done.error);
                return out;
            };
            struct Span { float most = 0, least = 0, mean = 0; size_t blocks = 0, silent = 0; };
            auto span = [&](const std::vector<float>& peaks, size_t from, size_t to) {
                Span r; r.least = 1e9f; to = std::min(to, peaks.size());
                for(size_t i = from; i < to; ++i) { r.most = std::max(r.most, peaks[i]); r.least = std::min(r.least, peaks[i]); r.mean += peaks[i]; r.silent += peaks[i] <= loud; ++r.blocks; }
                if(r.blocks) r.mean /= float(r.blocks); else r.least = 0;
                return r;
            };
            auto ms = [](size_t blocks) { return std::to_string(int(blocks * 128 * 1000 / 48000)) + " ms"; };
            // Key 48 into the layered zone, then key 72 into the split above it, each held half a second.
            for(const int key : {48, 72})
            {
                const auto take = record();
                pump(200); play(key, true); pump(500); play(key, false); pump(100);
                const auto r = finish(take);
                if(r.peaks.size() != taps.size()) continue;
                // The held note's blocks: from 40 ms after it began, for 400 ms, found on the part it plays.
                const auto& lead = r.peaks[key == 48 ? 0 : 2];
                size_t on = 0; while(on < lead.size() && lead[on] <= loud) ++on;
                const size_t from = on + 15, to = on + 15 + 150;
                const auto f = span(r.peaks[0], from, to), a = span(r.peaks[1], from, to), u = span(r.peaks[2], from, to), x = span(r.peaks[3], from, to);
                const auto whole = [&](size_t i) { return span(r.peaks[i], 0, r.peaks[i].size()); };
                if(key == 48)
                {
                    check(f.blocks > 100 && f.silent == 0 && a.silent == 0, "key 48 sounds in FM and Analog in every block of " + std::to_string(f.blocks) + " (" + ms(f.blocks) + "): FM mean " + num(f.mean) + ", least " + num(f.least) + ", " + std::to_string(f.silent) + " silent; Analog mean " + num(a.mean) + ", least " + num(a.least) + ", " + std::to_string(a.silent) + " silent");
                    check(whole(2).most <= loud, "and nowhere in Upper: its loudest block " + num(whole(2).most));
                    check(x.silent == 0, "the shared effects sound with it in every block: bus mean " + num(x.mean));
                }
                else
                {
                    check(u.blocks > 100 && u.silent == 0, "key 72 sounds in Upper in every block of " + std::to_string(u.blocks) + ": mean " + num(u.mean) + ", least " + num(u.least));
                    check(whole(0).most <= loud && whole(1).most <= loud, "and nowhere in FM or Analog: their loudest blocks " + num(whole(0).most) + " and " + num(whole(1).most));
                    check(x.silent == 0, "the shared effects sound with it in every block: bus mean " + num(x.mean));
                }
                pump(1500); // tails ring out before the next
            }

            // ---- 3 and 4. Edit through Oscairn's own editors, and undo each edit ----
            // Each plugin is edited on its Part page: the box selected (a tap on it), its pages below.
            auto open = [&](const std::string& part, NodeId node) -> standalone::ParameterGrid* {
                openPart(partNamed(part)->id);
                tapAt({16 + 110 + 34 + 30.f, 56 + 12 + 48.f}); // its first box, after IN and the first +
                auto grids = all<standalone::ParameterGrid>(view);
                check(grids.size() == 1 && grids.front()->node() == node, part + ": a tap on its plugin shows its pages");
                return grids.empty() ? nullptr : grids.front();
            };
            // The page with a rich view, by the view's name, and the view.
            auto pageWith = [&](standalone::ParameterGrid& grid, const juce::String& viewName) -> juce::Component* {
                for(size_t i = 0; i < grid.pages(); ++i) { grid.showPage(i); if(auto* c = named(grid, viewName)) return c; }
                return nullptr;
            };
            // Undo names what it would undo on the button itself: the engine's own name for its
            // last edit, shown soon after the edit or undo without the test refreshing the view.
            auto undoButton = [&] { const auto f = all<juce::Button>(view, [](juce::Button& b) { return b.getButtonText().startsWith("Undo"); }); return f.empty() ? nullptr : f.front(); };
            auto engineUndo = [&] { auto name = std::make_shared<std::string>(); engine.edit([name](Control& c) { *name = c.history().undo; }); return *name; };
            auto labelled = [&](const std::string& when) {
                const auto want = engineUndo();
                const auto right = [&] { const auto* b = undoButton(); return b && (want.empty() ? !b->isEnabled() : b->isEnabled() && b->getButtonText() == "Undo " + juce::String::fromUTF8(want.c_str())); };
                const auto from = juce::Time::getMillisecondCounter();
                while(!right() && juce::Time::getMillisecondCounter() - from < 2000) juce::MessageManager::getInstance()->runDispatchLoopUntil(5);
                const auto took = juce::Time::getMillisecondCounter() - from;
                check(right() && took <= 150, when + ", the Undo button reads \"" + (undoButton() ? undoButton()->getButtonText().toStdString() : std::string()) + "\" (" + (want.empty() ? std::string("disabled: nothing to undo") : "Undo " + want) + ") within " + std::to_string(took) + " ms");
            };
            auto undo = [&] { tap(undoButton(), "Undo"); };
            const auto fmBase = values(fm), analogBase = values(analog);
            // FM: operator 2's level bar dragged to the top in the operator view.
            if(auto* grid = open("FM", fm))
            {
                std::string pages; for(size_t i = 0; i < grid->pages(); ++i) pages += (i ? ", " : "") + grid->pageName(i);
                std::fprintf(stderr, "  FM pages: %s\n", pages.c_str());
                auto* ops = pageWith(*grid, "FM operators");
                check(ops != nullptr, "FM's operator view is on one of its pages");
                snapshot(view, "3-fm-operators");
                if(ops)
                {
                    const float routing = std::min((ops->getWidth() - 28) * .36f, 360.f), columns = (ops->getWidth() - 28) - routing - 14, x = 14 + routing + 14 + columns / 6 * 1.5f;
                    drag(ops, {x, ops->getHeight() / 2.f}, {x, 0.f});
                    check(wait([&] { return !changed(fm, fmBase).empty(); }), "dragging operator 2's level bar edits FM: " + join(changed(fm, fmBase)));
                    const auto before = engineUndo();
                    labelled("after the drag");
                    snapshot(view, "3-fm-operators-edited");
                    undo();
                    check(wait([&] { return changed(fm, fmBase).empty(); }), "Undo puts FM back: " + join(changed(fm, fmBase)));
                    labelled("after the undo");
                    check(engineUndo() != before, "and Undo then names the edit before it, or nothing: \"" + engineUndo() + "\"");
                }
            }
            // Analog: its envelope's first point dragged along.
            if(auto* grid = open("Analog", analog))
            {
                std::string pages; for(size_t i = 0; i < grid->pages(); ++i) pages += (i ? ", " : "") + grid->pageName(i);
                std::fprintf(stderr, "  Analog pages: %s\n", pages.c_str());
                auto* curve = pageWith(*grid, "Envelope");
                check(curve != nullptr, "Analog's envelope view is on one of its pages");
                snapshot(view, "3-analog-envelope");
                if(curve)
                {
                    // The attack's end: the first point, at the top, somewhere along the curve's first third;
                    // tried from the left until a drag takes.
                    const float width = curve->getWidth() - 28.f, top = 28;
                    // Each drag given half a second to show, so one edit is made, not two.
                    int drags = 0;
                    for(float x = 14; x < 14 + width / 3 && changed(analog, analogBase).empty(); x += 4)
                    {
                        drag(curve, {x, top}, {x + width / 8, top}); ++drags;
                        for(int t = 0; t < 25 && changed(analog, analogBase).empty(); ++t) pump(20);
                    }
                    std::fprintf(stderr, "  Analog's envelope: %d drags from the left to find its first point\n", drags);
                    check(wait([&] { return !changed(analog, analogBase).empty(); }, 2), "dragging its envelope's first point edits Analog: " + join(changed(analog, analogBase)));
                    labelled("after the envelope drag");
                    snapshot(view, "3-analog-envelope-edited");
                    undo();
                    check(wait([&] { return changed(analog, analogBase).empty(); }), "Undo puts Analog back: " + join(changed(analog, analogBase)));
                }
            }
            // Scenes: Scene 2 from its button, and back.
            toList();
            // Switching scenes is playing, not editing (Session::startScene): it leaves Undo as it was.
            // What Undo would undo, asked of the engine (Engine::history() is polled and may trail an Undo).
            auto undoName = engineUndo;
            const auto undoBefore = undoName();
            tap(button(view, "Scene 2"), "Scene 2");
            check(wait([&] { return graph().scene == scenes[1].id && std::abs(partNamed("FM")->level - .4) < 1e-6; }), "Scene 2's button brings its FM level: " + num(partNamed("FM")->level));
            check(undoName() == undoBefore, "and is not an edit to undo: Undo names \"" + undoName() + "\", as before: \"" + undoBefore + "\"");
            tap(button(view, "Scene 1"), "Scene 1");
            check(wait([&] { return graph().scene == scenes[0].id && std::abs(partNamed("FM")->level - 1) < 1e-6; }), "Scene 1's button brings FM back: " + num(partNamed("FM")->level));
            // The control tree: Control 1's slider moves Control 2, which moves both parts' parameters.
            toControls();
            {
                const auto fmWas = engine.param(nodeName(fm), fmTarget.id), analogWas = engine.param(nodeName(analog), analogTarget.id);
                auto sliders = all<juce::Slider>(view, [](juce::Slider& s) { return s.getMaximum() == 1 && s.getWidth() > 100; });
                std::sort(sliders.begin(), sliders.end(), [](auto* a, auto* b) { return a->getY() < b->getY(); });
                // Control 1's own slider is the topmost.
                check(!sliders.empty(), "Control 1 has its slider");
                const double was = sliders.empty() ? 0 : sliders.front()->getValue(), to = was < .5 ? .9 : .1;
                if(!sliders.empty()) sliders.front()->setValue(to, juce::sendNotificationSync);
                auto moved = [&] { return std::abs(engine.param(nodeName(fm), fmTarget.id) - fmWas) > 1e-6 && std::abs(engine.param(nodeName(analog), analogTarget.id) - analogWas) > 1e-6; };
                check(wait(moved), "moving Control 1 moves Control 2 and so FM's " + fmTarget.name + " (" + num(fmWas) + " -> " + num(engine.param(nodeName(fm), fmTarget.id)) + ") and Analog's " + analogTarget.name +
                                   " (" + num(analogWas) + " -> " + num(engine.param(nodeName(analog), analogTarget.id)) + ")");
                snapshot(view, "3-controls-moved");
                // Moving a control is playing, not editing (Session::playMacro), as a scene is: Undo
                // is left as it was. Each target started from its parameter's value when it was
                // added, with the control where it is, so moved back the control puts both back.
                check(undoName() == undoBefore, "moving a control is not an edit to undo: Undo names \"" + undoName() + "\", as before: \"" + undoBefore + "\"");
                if(!sliders.empty()) sliders.front()->setValue(was, juce::sendNotificationSync);
                auto near = [&](NodeId n, const Control::ParamInfo& p) { return std::abs(engine.param(nodeName(n), p.id) - (n == fm ? fmWas : analogWas)) <= (p.maximum - p.minimum) * .02; };
                check(wait([&] { return near(fm, fmTarget) && near(analog, analogTarget); }), "Control 1 moved back to " + num(was) + " moves both with it: FM's " + fmTarget.name + " " +
                      num(engine.param(nodeName(fm), fmTarget.id)) + ", Analog's " + analogTarget.name + " " + num(engine.param(nodeName(analog), analogTarget.id)));
            }
            // Play: the Performance's controls on its knob row, as the stage shows them.
            {
                standalone::PlayView stage(engine);
                standalone::setDesktopMode(stage, 2); stage.setSize(1024, 600);
                snapshot(stage, "3-play");
            }

            // ---- 5. A crash mid-play ----
            // A fresh FM edit through its view to come back with; then notes held in both zones.
            const auto fmBefore = values(fm);
            if(auto* grid = open("FM", fm); grid)
                if(auto* ops = pageWith(*grid, "FM operators"))
                {
                    const float routing = std::min((ops->getWidth() - 28) * .36f, 360.f), columns = (ops->getWidth() - 28) - routing - 14, x = 14 + routing + 14 + columns / 6 * 2.5f;
                    drag(ops, {x, ops->getHeight() / 2.f}, {x, ops->getHeight() * .1f}); // operator 3's level
                }
            wait([&] { return !changed(fm, fmBefore).empty(); }, 3);
            const auto fmEdited = values(fm);
            check(!changed(fm, fmBefore).empty(), "an FM edit to come back with: " + join(changed(fm, fmBefore)));
            const auto take = record();
            play(48, true); play(72, true);
            pump(500);
            const auto victims = workersWith(fmFile);
            check(victims.size() == 1, "FM's plugin is in a worker of its own: " + std::to_string(victims.size()));
            const auto killed = rendered.load();
            for(const auto v : victims) ::kill(v, SIGKILL);
            // Struck again on a fresh key in FM's zone every tenth of a second, so it is heard soon after it is back.
            std::vector<uint64_t> struck;
            for(int k = 0; k < 10; ++k) { pump(100); play(50 + k, true); struck.push_back(rendered.load()); }
            pump(300);
            const auto r = finish(take);
            for(int k = 0; k < 10; ++k) play(50 + k, false);
            play(48, false); play(72, false);
            if(r.peaks.size() == taps.size() && r.start && killed > r.start)
            {
                const size_t kill = size_t((killed - r.start) / 128), end = r.peaks[1].size() - 4; // to the take's end, the notes held throughout
                const auto f = span(r.peaks[0], 20, kill), a = span(r.peaks[1], 20, end), u = span(r.peaks[2], 20, end), x = span(r.peaks[3], 20, end);
                check(f.silent == 0 && f.blocks > 50, "FM sounds in every block before the kill (" + std::to_string(f.blocks) + ")");
                size_t gone = kill; while(gone < r.peaks[0].size() && r.peaks[0][gone] > loud) ++gone;
                size_t back = gone; while(back < r.peaks[0].size() && r.peaks[0][back] <= loud) ++back;
                check(gone < kill + 40, "FM goes silent " + ms(gone - kill) + " of audio after its worker is killed");
                check(a.silent == 0 && u.silent == 0, "Analog and Upper sound in every block across the crash, " + std::to_string(a.blocks) + " blocks (" + ms(a.blocks) + "): least " + num(a.least) + " and " + num(u.least) +
                                                       ", mean " + num(a.mean) + " and " + num(u.mean) + ", silent blocks " + std::to_string(a.silent) + " and " + std::to_string(u.silent));
                check(x.silent == 0, "and so do the shared effects: least " + num(x.least) + ", " + std::to_string(x.silent) + " silent blocks");
                std::string strikes; for(const auto t : struck) strikes += (strikes.empty() ? "" : ", ") + ms(size_t((t - killed) / 128));
                check(back < r.peaks[0].size(), "FM sounds again " + ms(back - kill) + " of audio after the kill (struck again at " + strikes + ")");
            }
            else check(false, "the crash take covers the kill");
            check(engine.status().failures.empty(), "and is not given up: " + (engine.status().failures.empty() ? std::string() : engine.status().failures.front().reason));
            check(changed(fm, fmEdited).empty(), "FM came back with its parameters, the edit included: " + join(changed(fm, fmEdited)));

            // ---- 6. Save and reopen ----
            auto fingerprint = [&] {
                std::string s;
                const auto g = graph();
                for(const auto& p : g.parts)
                {
                    s += p.name + (p.input.zones.empty() ? std::string(" all") : " " + std::to_string(p.input.zones[0].lowKey) + "-" + std::to_string(p.input.zones[0].highKey)) + " level " + num(p.level) + " [";
                    for(const auto n : p.chain) { s += engine.pluginId(g.names.at(n)) + " "; for(const auto& [id, v] : values(n)) s += num(v) + ","; }
                    s += "]";
                }
                for(const auto& p : standalone::readParts(g).parts) for(const auto& [b, level] : p.sends) s += " send " + p.name + " " + num(level);
                for(const auto& sc : g.scenes) { s += " scene " + sc.name; for(const auto& ps : sc.parts) s += " " + num(ps.level); }
                for(const auto& m : g.macros) { s += " control " + m.name + " " + num(m.value); for(const auto& t : m.targets) s += " >" + std::to_string(int(t.kind)) + ":" + (t.node ? g.names.at(t.node) : std::string()) + ":" + std::to_string(t.param); }
                return s;
            };
            const auto saved = fingerprint();
            // The view's Save, wired as the app wires it (Main.cpp's saveToLibrary, past its name dialog).
            view.onSave = [&] { standalone::SoundAction save; save.kind = standalone::SoundAction::SaveToLibrary; save.choice.name = "Phase 3b"; engine.soundAction(save); };
            toList(); view.refresh();
            tap(button(view, "Save"), "Save");
            check(wait([&] { return !engine.soundBusy() && engine.soundMessage() == "Performance saved."; }, 30), "saved to the library: " + engine.soundMessage());
            std::string id;
            wait([&] { const auto info = engine.performanceInfo(); if(info) id = info->id; return !id.empty(); }, 5);
            // Changed after saving, then reopened from the library.
            openPart(partNamed("Upper")->id);
            tap(button(view, "Remove part"), "Remove part");
            check(wait([&] { return !partNamed("Upper"); }), "Upper removed after saving");
            standalone::SoundAction load; load.kind = standalone::SoundAction::LoadFromLibrary; load.libraryId = id;
            engine.soundAction(load);
            bool dropped = false; // the engine taken for gone meanwhile (a stale read thrown, as it was)
            check(wait([&] { dropped = dropped || !engine.connected(); return !engine.soundBusy() && partNamed("Upper").has_value(); }, 30), "reopened from the library: " + engine.soundMessage());
            check(!dropped, "and the desktop stays connected while it loads");
            wait([&] { return fingerprint() == saved; }, 5);
            const auto reopened = fingerprint();
            check(reopened == saved, "the reopened Performance is the one saved");
            if(reopened != saved) std::fprintf(stderr, "  saved:    %s\n  reopened: %s\n", saved.c_str(), reopened.c_str());
            toList();
            snapshot(view, "6-reopened");
        }
        running = false;
        control.join();
    }
    std::filesystem::remove(socket);
    std::filesystem::remove_all(pluginDir);
    data.deleteRecursively();
    if(!failures) std::puts("ok: Phase 3b's Performance built, played, edited, undone, crashed and reopened on the Panel");
    return failures ? 1 : 0;
}
