// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The Setlist app (RFC 0022 step 2): a program of its own, which the shell starts from the
// app.json beside it, handing it a connection to the engine (OSCAIRN_CONTROL_FD). It reaches
// the engine only through that connection: picks go to the shell, which asks whoever offers
// them; Performances sent to it come the same way. Its state (the entry playing) is kept in its
// data folder (OSCAIRN_APP_DATA), so started again after a crash it comes back where it was.
//   Oscairn Setlist [--snapshot out.png --size WxH [--mode 1|2]]
#include "../../DeviceScreen.h"
#include "../../TextKeyboard.h"
#include "../../Setlist.h"
#include "../../Engine.h"
#include "../../Theme.h"
#include <cstdlib>
#include <vector>
#include <unistd.h>

namespace oscairn::standalone
{
    class SetlistWindow final : public juce::DocumentWindow
    {
    public:
        explicit SetlistWindow(juce::Component& content) : DocumentWindow("Oscairn - Setlist", ink(Ink::Background), DocumentWindow::allButtons)
        {
            setUsingNativeTitleBar(true); setContentNonOwned(&content, false); setResizable(true, false); centreWithSize(700, 760);
        }
        // Closed by the musician: the app quits, which the shell does not take for a crash.
        void closeButtonPressed() override { juce::JUCEApplication::getInstance()->systemRequestedQuit(); }
    };

    class SetlistApp final : public juce::JUCEApplication
    {
    public:
        const juce::String getApplicationName() override { return "Oscairn Setlist"; }
        const juce::String getApplicationVersion() override { return OSCAIRN_RELEASE; }
        bool moreThanOneInstanceAllowed() override { return true; }

        void initialise(const juce::String& commandLine) override
        {
            const auto* given = std::getenv("OSCAIRN_CONTROL_FD");
            if(!given || !*given) { std::fprintf(stderr, "oscairn-setlist: Oscairn starts this app; it has no connection to the engine\n"); setApplicationReturnValue(2); quit(); return; }
            const auto themes = juce::SystemStats::getEnvironmentVariable("OSCAIRN_THEMES", {});
            theme_ = std::make_unique<Theme>(juce::File(), themes.isEmpty() ? juce::File() : juce::File(themes));
            engine_ = std::make_unique<Engine>(std::atoi(given));
            engine_->onLost = [this] { std::fprintf(stderr, "oscairn-setlist: the engine's connection closed\n"); setApplicationReturnValue(1); quit(); };
            // The actions the shell answers, asked of it; it decides who answers.
            for(const auto& action : juce::StringArray::fromTokens(juce::SystemStats::getEnvironmentVariable("OSCAIRN_ACTIONS", {}), ",", ""))
                if(action.isNotEmpty() && action != sendPerformance && action != stepSetlist) // not its own
                    actions_.offer(action.toStdString(), "shell", [this, a = action.toStdString()](const std::string& argument, Actions::Answer answer) { engine_->appRequest({}, a, argument, std::move(answer)); });
            if(juce::SystemStats::getEnvironmentVariable("OSCAIRN_TEXT_ENTRY", {}) == "onScreen") setTextEntryOnScreen(true); // the shell's screen has no typing keyboard
            pane_ = std::make_unique<SetlistPane>(*engine_, actions_);
            setDesktopMode(*pane_, 1);
            restore();
            pane_->onCurrent = [this] { keep(); playing(); };
            playing();
            engine_->onAppRequest = [this](uint64_t request, const std::string&, const std::string& action, const std::string& argument) {
                if(action == showApp) { show(argument); engine_->appAnswer(request, "shown"); }
                else if(action == sendPerformance) engine_->appAnswer(request, pane_->add(argument).toStdString());
                else if(action == stepSetlist) { pane_->step(argument == "-1" ? -1 : 1); engine_->appAnswer(request, current()); }
                else engine_->appAnswer(request, {}); // not offered: canceled
            };
            if(!juce::StringArray::fromTokens(commandLine, true).contains("--snapshot")) window_ = std::make_unique<SetlistWindow>(*pane_);
#if JUCE_MAC
            else juce::Process::setDockIconVisible(false); // rendered off screen: no window, never the active app, nor in the Dock
#endif
            std::fprintf(stderr, "oscairn-setlist: started (pid %d) with %s\n", int(::getpid()), pane_->setlist().info.name.empty() ? "no setlist" : pane_->setlist().info.name.c_str());
            // For tests (desktop-apps), which cannot press Add in another process: ask for a pick once started.
            if(juce::SystemStats::getEnvironmentVariable("OSCAIRN_TEST_PICK", {}) == "1") juce::MessageManager::callAsync([this] { pane_->addFromLibrary(); });
            // For tests: leak that many MB once (the first run with this data folder), so its shell stops it and it comes back.
            if(const int mb = juce::SystemStats::getEnvironmentVariable("OSCAIRN_TEST_LEAK", {}).getIntValue(); mb > 0 && stateFile() != juce::File())
                if(const auto marker = stateFile().getSiblingFile("leaked"); !marker.exists() && marker.create().wasOk())
                    { static std::vector<std::unique_ptr<uint64_t[]>> leaked; uint64_t x = 88172645463325252ull; // varied, so the system cannot compress it away
                      for(int i = 0; i < mb; ++i) { leaked.emplace_back(new uint64_t[1 << 17]); for(int j = 0; j < (1 << 17); ++j) { x ^= x << 13; x ^= x >> 7; x ^= x << 17; leaked.back()[size_t(j)] = x; } } }
            // For tests: hold that many MB for a second after each start, then let it go: a spike, not a use.
            if(const int mb = juce::SystemStats::getEnvironmentVariable("OSCAIRN_TEST_SPIKE", {}).getIntValue(); mb > 0)
                { auto spike = std::make_shared<std::vector<std::unique_ptr<uint64_t[]>>>(); uint64_t x = 88172645463325252ull;
                  for(int i = 0; i < mb; ++i) { spike->emplace_back(new uint64_t[1 << 17]); for(int j = 0; j < (1 << 17); ++j) { x ^= x << 13; x ^= x >> 7; x ^= x << 17; spike->back()[size_t(j)] = x; } }
                  juce::Timer::callAfterDelay(1000, [spike] { spike->clear(); std::fprintf(stderr, "oscairn-setlist: the test's spike let go\n"); }); }
            const auto args = juce::StringArray::fromTokens(commandLine, true);
            if(const int at = args.indexOf("--snapshot"); at >= 0 && at + 1 < args.size())
            {
                const auto size = juce::StringArray::fromTokens(args[args.indexOf("--size") + 1], "x", "");
                const int mode = args.contains("--mode") ? args[args.indexOf("--mode") + 1].getIntValue() : 1;
                const juce::File out = juce::File::getCurrentWorkingDirectory().getChildFile(args[at + 1].unquoted());
                juce::Timer::callAfterDelay(500, [this, out, w = size[0].getIntValue(), h = size[1].getIntValue(), mode] {
                    setDesktopMode(*pane_, mode); pane_->setSize(std::max(1, w), std::max(1, h));
                    out.deleteFile();
                    juce::FileOutputStream stream(out);
                    juce::PNGImageFormat().writeImageToStream(pane_->createComponentSnapshot(pane_->getLocalBounds()), stream);
                    quit();
                });
            }
        }

        void shutdown() override { window_.reset(); pane_.reset(); engine_.reset(); theme_.reset(); }
        void systemRequestedQuit() override { quit(); }

    private:
        // Shown as the shell says: its own window, or placed where the shell puts it (a panel's
        // frame under the top bar), with no title bar.
        void show(const std::string& argument)
        {
            if(!window_) return; // --snapshot
            const auto how = juce::JSON::parse(juce::String::fromUTF8(argument.c_str()));
            setDesktopMode(*pane_, int(how.getProperty("mode", 1)) == 2 ? 2 : 1);
            placeAppWindow(*window_, how);
            pane_->resized();
        }
        std::string current() const
        {
            const auto slots = slotsOf(pane_->setlist());
            return pane_->current() >= 0 && size_t(pane_->current()) < slots.size() ? slots[size_t(pane_->current())].id : std::string();
        }
        // The entry playing, sent to whoever takes it (Play, which then shows Previous and Next).
        void playing() { actions_.request(sendSetlistEntry, current(), [](const std::string&) {}); }
        juce::File stateFile() const
        {
            const auto data = juce::SystemStats::getEnvironmentVariable("OSCAIRN_APP_DATA", {});
            return data.isEmpty() ? juce::File() : juce::File(data).getChildFile("state.json");
        }
        // The setlist open (its library ID) and the slot playing, written when it changes.
        void keep()
        {
            const auto file = stateFile();
            if(file == juce::File()) return;
            auto* state = new juce::DynamicObject();
            state->setProperty("setlist", juce::String::fromUTF8(pane_->setlist().info.id.c_str())); state->setProperty("current", pane_->current());
            file.getParentDirectory().createDirectory();
            juce::TemporaryFile temporary(file);
            if(!temporary.getFile().replaceWithText(juce::JSON::toString(juce::var(state))) || !temporary.overwriteTargetFileWithTemporary())
                std::fprintf(stderr, "oscairn-setlist: cannot keep its state in %s\n", file.getFullPathName().toRawUTF8());
        }
        void restore()
        {
            const auto file = stateFile();
            if(file == juce::File() || !file.existsAsFile()) return;
            const auto state = juce::JSON::parse(file.loadFileAsString());
            const auto id = state["setlist"].toString().toStdString();
            if(!id.empty() && id != pane_->setlist().info.id) pane_->open(id);
            if(!id.empty() && id == pane_->setlist().info.id && state["current"].isInt()) pane_->mark(int(state["current"]));
        }

        std::unique_ptr<Theme> theme_;
        std::unique_ptr<Engine> engine_;
        Actions actions_;
        std::unique_ptr<SetlistPane> pane_;
        std::unique_ptr<SetlistWindow> window_;
    };
}

START_JUCE_APPLICATION(oscairn::standalone::SetlistApp)
