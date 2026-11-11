// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The Settings app (RFC 0022's built-in apps, step 7): a program of its own, which the shell
// starts from the app.json beside it, handing it a connection to the engine (OSCAIRN_CONTROL_FD)
// held to its manifest's `system` permission. Its pages are those the device's profile
// (OSCAIRN_PROFILE: a built-in profile's id or a profile file; none: this computer) and this
// build have (Pages.cpp).
//   Oscairn Settings [--profile NAME|FILE] [--snapshot out.png --size WxH [--mode 1|2] [--page ID]]
#include "Frame.h"
#include "../../Apps.h"
#include <cstdlib>
#include <unistd.h>

namespace oscairn::standalone
{
    class SettingsWindow final : public juce::DocumentWindow
    {
    public:
        explicit SettingsWindow(juce::Component& content) : DocumentWindow("Oscairn - Settings", ink(Ink::Background), DocumentWindow::allButtons)
        {
            setUsingNativeTitleBar(true); setContentNonOwned(&content, false); setResizable(true, false); centreWithSize(900, 640);
        }
        // Closed by the musician: the app quits, which the shell does not take for a crash.
        void closeButtonPressed() override { juce::JUCEApplication::getInstance()->systemRequestedQuit(); }
    };

    class SettingsApp final : public juce::JUCEApplication
    {
    public:
        const juce::String getApplicationName() override { return "Oscairn Settings"; }
        const juce::String getApplicationVersion() override { return "0.1.0"; }
        bool moreThanOneInstanceAllowed() override { return true; }

        void initialise(const juce::String& commandLine) override
        {
            const auto* given = std::getenv("OSCAIRN_CONTROL_FD");
            if(!given || !*given) { std::fprintf(stderr, "oscairn-settings: Oscairn starts this app; it has no connection to the engine\n"); setApplicationReturnValue(2); quit(); return; }
            const auto args = juce::StringArray::fromTokens(commandLine, true);
            auto value = [&](const char* name) { const int at = args.indexOf(name); return at >= 0 && at + 1 < args.size() ? args[at + 1].unquoted() : juce::String(); };
            const auto themes = juce::SystemStats::getEnvironmentVariable("OSCAIRN_THEMES", {});
            theme_ = std::make_unique<Theme>(juce::File(), themes.isEmpty() ? juce::File() : juce::File(themes));
            const auto named = args.contains("--profile") ? value("--profile") : juce::SystemStats::getEnvironmentVariable("OSCAIRN_PROFILE", {});
            try { profile_ = loadProfile(named.toStdString()); }
            catch(const std::exception& e) { std::fprintf(stderr, "oscairn-settings: the profile %s: %s; this computer's instead\n", named.toRawUTF8(), e.what()); profile_ = Profile::desktop(); }
            engine_ = std::make_unique<Engine>(std::atoi(given), false); // system only: no graph to read
            engine_->onLost = [this] { std::fprintf(stderr, "oscairn-settings: the engine's connection closed\n"); setApplicationReturnValue(1); quit(); };
            frame_ = std::make_unique<settings::Frame>(*engine_, profile_);
            setDesktopMode(*frame_, profile_.screen() && profile_.screen()->touch ? 2 : 1); // until the shell says how it is shown
            engine_->onAppRequest = [this](uint64_t request, const std::string&, const std::string& action, const std::string& argument) {
                if(action == showApp) { show(argument); engine_->appAnswer(request, "shown"); }
                else engine_->appAnswer(request, {}); // not offered: canceled
            };
            window_ = std::make_unique<SettingsWindow>(*frame_);
            std::fprintf(stderr, "oscairn-settings: started (pid %d) with %d pages for %s\n", int(::getpid()), int(frame_->shown().size()), profile_.id.c_str());
            if(const auto out = value("--snapshot"); out.isNotEmpty())
            {
                const auto size = juce::StringArray::fromTokens(value("--size"), "x", "");
                const int mode = args.contains("--mode") ? value("--mode").getIntValue() : 1;
                setDesktopMode(*frame_, mode == 2 ? 2 : 1);
                if(const auto page = value("--page"); page.isNotEmpty()) frame_->open(page.toStdString());
                // Once the page has read what it shows.
                juce::Timer::callAfterDelay(1500, [this, file = juce::File::getCurrentWorkingDirectory().getChildFile(out), w = size[0].getIntValue(), h = size[1].getIntValue()] {
                    frame_->setSize(std::max(1, w), std::max(1, h));
                    file.deleteFile();
                    juce::FileOutputStream stream(file);
                    juce::PNGImageFormat().writeImageToStream(frame_->createComponentSnapshot(frame_->getLocalBounds()), stream);
                    quit();
                });
            }
        }

        void shutdown() override { window_.reset(); frame_.reset(); engine_.reset(); theme_.reset(); }
        void systemRequestedQuit() override { quit(); }

    private:
        // Shown as the shell says: its own window, or placed where the shell puts it (a panel's
        // frame under the top bar), with no title bar.
        void show(const std::string& argument)
        {
            const auto how = juce::JSON::parse(juce::String::fromUTF8(argument.c_str()));
            if(!bool(how.getProperty("visible", true))) { window_->setVisible(false); return; }
            setDesktopMode(*frame_, int(how.getProperty("mode", 1)) == 2 ? 2 : 1);
            if(const auto* b = how["bounds"].getArray(); b && b->size() == 4)
            {
                window_->setUsingNativeTitleBar(false); window_->setTitleBarHeight(0); window_->setResizable(false, false);
                window_->setBounds(int((*b)[0]), int((*b)[1]), int((*b)[2]), int((*b)[3]));
            }
            frame_->resized();
            window_->setVisible(true); window_->toFront(true);
        }

        std::unique_ptr<Theme> theme_;
        Profile profile_;
        std::unique_ptr<Engine> engine_;
        std::unique_ptr<settings::Frame> frame_;
        std::unique_ptr<SettingsWindow> window_;
    };
}

START_JUCE_APPLICATION(oscairn::standalone::SettingsApp)
