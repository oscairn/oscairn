// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Apps.h"
#include "Engine.h"
#include "Theme.h"
#include <juce_gui_basics/juce_gui_basics.h>
#include <deque>
#include <sys/types.h>

namespace oscairn::standalone
{
    /// An app's manifest (RFC 0022): app.json in its folder, read so the shell knows an app
    /// without starting it. Only what steps 2 and 4 use is read; the rest of the format is accepted.
    struct AppManifest
    {
        std::string id, name, version;
        juce::File folder, executable;            // this platform's build; none: it does not run here
        struct Offer { std::string action, name; };
        std::vector<Offer> offers;
        std::vector<std::string> asks;
        uint64_t reserve = 0, maximum = 0;        // memory, MiB; maximum 0: none declared
        std::vector<std::string> permissions;     // what the engine lets its connection do
    };
    AppManifest readAppManifest(const juce::File& folder); // throws std::runtime_error saying what is wrong
    std::vector<AppManifest> findApps(const juce::File& folder); // its folders holding an app.json; one that cannot be read is left out, said on stderr
#ifdef __APPLE__
    void activateApp(pid_t pid); // the process becomes the active app, its windows in front (AppProcesses_mac.mm)
#endif

    /// The apps the shell runs as processes, which it supervises (RFC 0022's containment): each
    /// started with a connection the engine grants (descriptor 3, OSCAIRN_CONTROL_FD), its
    /// stdout and stderr appended to Logs/apps/<id>.log in the data folder, its state kept in
    /// AppData/<id>/. A crash (or its memory over the manifest's maximum) is said and, for the
    /// app in front, started again at once, at most 3 times a minute; then it stays closed with
    /// Open again. The engine pings each app every second: silent 5 seconds in front, the
    /// shell says it is not responding (Wait or Close); 10 behind, it is stopped. Each action
    /// a manifest offers is offered in `actions`, answered by starting that app and asking it.
    /// Message thread; nothing here waits on an app.
    class AppProcesses : private juce::Timer
    {
    public:
        AppProcesses(Engine& engine, Actions& actions, std::vector<AppManifest> apps, std::vector<std::string> environment = {});
        ~AppProcesses() override; // stops every app it started

        enum class State { Closed, Running, NotResponding, GaveUp };
        const std::vector<AppManifest>& apps() const { return manifests_; }
        const AppManifest* find(const std::string& id) const;
        // Started if it is not running, and shown: where the shell says on the screen (a panel's
        // frame, in screen coordinates), or as its own window when bounds are empty. mode: 1
        // mouse, 2 touch. False when it cannot run (no build here, or no connection).
        bool open(const std::string& id, juce::Rectangle<int> bounds = {}, int mode = 1);
        void hide(const std::string& id);
        void setFront(const std::string& id); // the app in front (the hang rule's 5 seconds), or none
        State state(const std::string& id) const;
        pid_t pid(const std::string& id) const;
        juce::File logFile(const std::string& id) const;
        juce::String log(const std::string& id) const; // the last 64 KiB of its log
        // What it reserves when it starts (RFC 0025), in bytes: its manifest's reservation, or the
        // most it was measured using here (app-memory.json in the data folder) where that is more,
        // up to its maximum. The engine admits it before it starts and counts it until it closes.
        uint64_t reservation(const std::string& id) const;

        // What to tell the musician, and what may be chosen; an empty text takes it away.
        struct Notice
        {
            std::string app;
            juce::String text;
            enum class Choice { None, WaitOrClose, OpenAgain } choice = Choice::None;
        };
        std::function<void(const Notice&)> onNotice;
        void wait(const std::string& id);      // not responding: give it longer
        void close(const std::string& id);     // not responding: stopped, and treated as a crash
        void openAgain(const std::string& id); // after it stayed closed
        void stop(const std::string& id);      // closed by the shell (its switcher), not a crash

    private:
        struct Running
        {
            pid_t pid = -1;
            State state = State::Closed;
            bool shown = false, waited = false, stopping = false;
            juce::Rectangle<int> bounds; int mode = 1;
            std::deque<double> crashes;       // when, in seconds, within the last minute
            juce::String why;                 // a stop of ours to report as the crash's reason
            std::deque<std::pair<double, uint64_t>> readings; // its memory, when read (seconds) and bytes, over the last few seconds
        };
        bool start(const std::string& id);
        void show(const std::string& id);
        void exited(const std::string& id, int status);
        void say(const std::string& id, const juce::String& text, Notice::Choice choice = Notice::Choice::None);
        void status(const std::map<std::string, uint32_t>& silent);
        void timerCallback() override;

        Engine& engine_;
        Actions& actions_;
        std::vector<std::string> offered_; // the actions offered here, withdrawn when this goes
        std::vector<AppManifest> manifests_;
        std::vector<std::string> environment_;
        std::map<std::string, Running> running_;
        std::map<std::string, uint64_t> measured_; // each app's peak past its reservation, bytes
        std::string front_;
    };

    /// Where an app process draws on a panel's screen: the shell's frame keeps this place under
    /// its top bar, and the app's own window is put over it while it shows. Until the device's
    /// display path (RFC 0022 step 6) this is a window placed over the shell's.
    class AppPlace : public juce::Component, private juce::ComponentMovementWatcher
    {
    public:
        AppPlace(AppProcesses& apps, std::string id) : ComponentMovementWatcher(this), apps_(apps), id_(std::move(id)) {}
        ~AppPlace() override { if(placed_) apps_.hide(id_); }
        void paint(juce::Graphics& g) override { g.fillAll(ink(Ink::Background)); }
        const std::string& id() const { return id_; }

    private:
        void componentMovedOrResized(bool, bool) override { place(); }
        void componentPeerChanged() override { place(); }
        void componentVisibilityChanged() override { place(); }
        using ComponentMovementWatcher::componentVisibilityChanged;
        using ComponentMovementWatcher::componentMovedOrResized;
        void place();
        AppProcesses& apps_;
        std::string id_;
        bool placed_ = false;
        juce::Rectangle<int> where_;
    };

    /// The supervisor's notice across the top of the window (RFC 0022): what happened to an
    /// app, with Details (its log), and Wait and Close, or Open again.
    class AppNotice : public juce::Component
    {
    public:
        explicit AppNotice(AppProcesses& apps);
        void show(const AppProcesses::Notice& notice);
        const juce::String& text() const { return notice_.text; }
        void paint(juce::Graphics&) override;
        void resized() override;

    private:
        AppProcesses& apps_;
        AppProcesses::Notice notice_;
        juce::TextButton details_{"Details"}, wait_{"Wait"}, close_{"Close"}, again_{"Open again"}, dismiss_{"OK"};
    };
}
