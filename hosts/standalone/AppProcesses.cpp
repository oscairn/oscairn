// SPDX-License-Identifier: AGPL-3.0-or-later
#include "AppProcesses.h"
#include "EngineProcess.h"
#include "Theme.h"
#include "oscairn/library.h"
#include "oscairn/package.h"
#include <algorithm>
#include <cerrno>
#include <csignal>
#include <cstdio>
#include <fcntl.h>
#include <fstream>
#include <sys/wait.h>
#include <unistd.h>
#ifdef __APPLE__
#include <libproc.h>
#endif

namespace oscairn::standalone
{
    namespace
    {
        constexpr uint32_t frontSilence = 5000, behindSilence = 10000; // ms, RFC 0022's hang rule
        constexpr size_t restartsPerMinute = 3, logTail = 64 * 1024;
        constexpr int64_t logRotation = 1024 * 1024;
        // Memory counts once it has lasted this long (seconds): a shorter spike, such as the graphics
        // memory a window takes for about a second when much of it is drawn afresh, is not an app's use.
        constexpr double sustained = 3;
        double seconds() { return juce::Time::getMillisecondCounterHiRes() / 1000.0; }
        bool named(const juce::String& s)
        {
            return s.isNotEmpty() && s.length() <= 128 && s.containsOnly("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-_");
        }
        juce::File dataFolder() { return juce::File(juce::String::fromUTF8(dataHome().string().c_str())); }
        // What the app uses of the machine's memory now (RFC 0025's footprint on macOS, its
        // resident set on Linux); 0 when it cannot be read.
        uint64_t footprint(pid_t pid)
        {
#ifdef __APPLE__
            rusage_info_v4 info{};
            return proc_pid_rusage(pid, RUSAGE_INFO_V4, reinterpret_cast<rusage_info_t*>(&info)) == 0 ? info.ri_phys_footprint : 0;
#else
            std::ifstream statm("/proc/" + std::to_string(pid) + "/statm");
            uint64_t size = 0, resident = 0;
            return statm >> size >> resident ? resident * uint64_t(::sysconf(_SC_PAGESIZE)) : 0;
#endif
        }
        juce::File measuredFile() { return dataFolder().getChildFile("app-memory.json"); }
    }

    AppManifest readAppManifest(const juce::File& folder)
    {
        const auto file = folder.getChildFile("app.json");
        if(!file.existsAsFile()) throw std::runtime_error("no app.json in " + folder.getFullPathName().toStdString());
        if(file.getSize() > 256 * 1024) throw std::runtime_error("app.json is too large");
        juce::var doc;
        if(juce::JSON::parse(file.loadFileAsString(), doc).failed() || !doc.isObject()) throw std::runtime_error("app.json is not a JSON object");
        if(doc["format"] != juce::var("oscairn.app") || !doc["version"].isInt() || int(doc["version"]) != 1) throw std::runtime_error("app.json is not a version 1 app manifest");
        AppManifest m;
        m.folder = folder;
        const auto id = doc["id"].toString(), name = doc["name"].toString();
        if(!doc["id"].isString() || !named(id)) throw std::runtime_error("an app's id is 1 to 128 letters, digits, dots, hyphens and underscores");
        if(!doc["name"].isString() || name.trim().isEmpty() || name.length() > 64) throw std::runtime_error("an app's name is 1 to 64 characters");
        m.id = id.toStdString(); m.name = name.toStdString(); m.version = doc["appVersion"].toString().toStdString();
        if(const auto* builds = doc["builds"].getDynamicObject())
            if(const auto path = builds->getProperty(juce::String(platform())).toString(); path.isNotEmpty())
            {
                // Within its folder only: no absolute paths, no way out.
                if(juce::File::isAbsolutePath(path) || path.contains("..")) throw std::runtime_error("an app's build is a path within its folder");
                m.executable = folder.getChildFile(path);
            }
        if(const auto* offers = doc["offers"].getArray())
            for(const auto& o : *offers)
            {
                if(!named(o["action"].toString())) throw std::runtime_error("an offer names its action");
                m.offers.push_back({o["action"].toString().toStdString(), o["name"].toString().toStdString()});
            }
        if(const auto* asks = doc["asks"].getArray())
            for(const auto& a : *asks) if(named(a.toString())) m.asks.push_back(a.toString().toStdString());
        if(const auto* permissions = doc["permissions"].getArray())
            for(const auto& p : *permissions) m.permissions.push_back(p.toString().toStdString()); // checked by the engine, which refuses a name it does not know
        const auto memory = doc["memory"];
        if(memory["reserve"].isInt()) m.reserve = uint64_t(std::max(0, int(memory["reserve"])));
        if(memory["max"].isInt()) m.maximum = uint64_t(std::max(0, int(memory["max"])));
        return m;
    }

    std::vector<AppManifest> findApps(const juce::File& folder)
    {
        std::vector<AppManifest> out;
        for(const auto& child : folder.findChildFiles(juce::File::findDirectories, false))
        {
            if(!child.getChildFile("app.json").existsAsFile()) continue;
            try { out.push_back(readAppManifest(child)); }
            catch(const std::exception& e) { std::fprintf(stderr, "oscairn: the app in %s: %s\n", child.getFullPathName().toRawUTF8(), e.what()); }
        }
        std::sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return a.id < b.id; });
        return out;
    }

    AppProcesses::AppProcesses(Engine& engine, Actions& actions, std::vector<AppManifest> apps, std::vector<std::string> environment)
        : engine_(engine), actions_(actions), manifests_(std::move(apps)), environment_(std::move(environment))
    {
        if(const auto* measured = juce::JSON::parse(measuredFile()).getDynamicObject())
            for(const auto& p : measured->getProperties())
                if(p.value.isInt() || p.value.isInt64()) measured_[p.name.toString().toStdString()] = uint64_t(std::max<juce::int64>(0, juce::int64(p.value)));
        for(const auto& m : manifests_)
            for(const auto& offer : m.offers)
            {
                if(actions_.offered(offer.action)) continue; // the first to offer it keeps it
                offered_.push_back(offer.action);
                actions_.offer(offer.action, m.name, [this, id = m.id, action = offer.action](const std::string& argument, Actions::Answer answer) {
                    const auto& r = running_[id];
                    if(r.state == State::GaveUp || (r.pid < 0 && !start(id))) { answer({}); return; }
                    engine_.appRequest(id, action, argument, std::move(answer));
                });
            }
        // What apps ask of the shell, which decides who answers.
        engine_.onAppRequest = [this](uint64_t request, const std::string& app, const std::string& action, const std::string& argument) {
            if(app.empty() || !find(app) || !actions_.request(action, argument, [this, request](const std::string& result) { engine_.appAnswer(request, result); }))
                engine_.appAnswer(request, {});
        };
        engine_.onAppStatus = [this](const std::map<std::string, uint32_t>& silent) { status(silent); };
        startTimer(100);
    }

    AppProcesses::~AppProcesses()
    {
        stopTimer();
        engine_.onAppRequest = {}; engine_.onAppStatus = {};
        for(const auto& action : offered_) actions_.withdraw(action); // its handlers hold this
        for(auto& [id, r] : running_) if(r.pid > 0) { ::kill(r.pid, SIGTERM); }
        for(auto& [id, r] : running_) if(r.pid > 0) { host::stopChild(r.pid); engine_.appClosed(id); }
    }

    const AppManifest* AppProcesses::find(const std::string& id) const
    {
        for(const auto& m : manifests_) if(m.id == id) return &m;
        return nullptr;
    }

    uint64_t AppProcesses::reservation(const std::string& id) const
    {
        const auto* m = find(id);
        if(!m) return 0;
        const auto it = measured_.find(id);
        const uint64_t measured = it == measured_.end() ? 0 : m->maximum ? std::min(it->second, m->maximum << 20) : it->second;
        return std::max(m->reserve << 20, measured);
    }

    AppProcesses::State AppProcesses::state(const std::string& id) const { const auto it = running_.find(id); return it == running_.end() ? State::Closed : it->second.state; }
    pid_t AppProcesses::pid(const std::string& id) const { const auto it = running_.find(id); return it == running_.end() ? -1 : it->second.pid; }
    juce::File AppProcesses::logFile(const std::string& id) const { return dataFolder().getChildFile("Logs/apps").getChildFile(juce::String(id) + ".log"); }

    juce::String AppProcesses::log(const std::string& id) const
    {
        juce::FileInputStream in(logFile(id));
        if(!in.openedOk()) return {};
        in.setPosition(std::max<int64_t>(0, in.getTotalLength() - int64_t(logTail)));
        return in.readEntireStreamAsString();
    }

    bool AppProcesses::start(const std::string& id)
    {
        const auto* m = find(id);
        auto& r = running_[id];
        if(!m) return false;
        if(m->executable == juce::File() || !m->executable.existsAsFile()) { say(id, juce::String(m->name) + " does not run on this computer."); return false; }
        // RFC 0025: its manifest's reservation, admitted by the engine first (a restart, still
        // counted, always is), and held until it is closed.
        if(const auto launch = engine_.launchApp({id, reservation(id)}); !launch.admitted)
        {
            say(id, juce::String(m->name) + " did not open: it " + juce::String::fromUTF8(launch.reason.c_str()) + ".");
            return false;
        }
        const int connection = engine_.appConnection(id, m->permissions);
        if(connection < 0) { engine_.appClosed(id); say(id, juce::String(m->name) + " did not start: the engine gave it no connection."); return false; }
        // Its log: appended, and started afresh past 1 MB.
        const auto file = logFile(id);
        file.getParentDirectory().createDirectory();
        if(file.getSize() > logRotation) file.moveFileTo(file.withFileExtension("log.1"));
        const int output = ::open(file.getFullPathName().toRawUTF8(), O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
        if(output < 0) { ::close(connection); engine_.appClosed(id); say(id, juce::String(m->name) + " did not start: its log cannot be written."); return false; }
        const auto started = "--- " + juce::Time::getCurrentTime().toISO8601(true) + " started\n";
        [[maybe_unused]] const auto written = ::write(output, started.toRawUTF8(), started.getNumBytesAsUTF8());
        const auto data = dataFolder().getChildFile("AppData").getChildFile(juce::String(id));
        data.createDirectory();
        std::string offered;
        for(const auto& action : actions_.actions()) offered += (offered.empty() ? "" : ",") + action;
        auto environment = environment_;
        environment.insert(environment.end(), {"OSCAIRN_CONTROL_FD=3", "OSCAIRN_APP_DATA=" + data.getFullPathName().toStdString(), "OSCAIRN_ACTIONS=" + offered});
        try { r.pid = host::spawnChild({m->executable.getFullPathName().toStdString()}, output, environment, connection); }
        catch(const std::exception& e) { r.pid = -1; say(id, juce::String(m->name) + " did not start: " + e.what()); }
        ::close(output); ::close(connection); // the child has its own
        if(r.pid < 0) { engine_.appClosed(id); return false; }
        r.state = State::Running; r.waited = r.stopping = false; r.why = {}; r.readings.clear();
        if(r.shown) show(id);
        return true;
    }

    void AppProcesses::show(const std::string& id)
    {
        const auto& r = running_[id];
        auto* argument = new juce::DynamicObject();
        argument->setProperty("visible", r.shown); argument->setProperty("mode", r.mode);
        if(!r.bounds.isEmpty()) argument->setProperty("bounds", juce::Array<juce::var>{r.bounds.getX(), r.bounds.getY(), r.bounds.getWidth(), r.bounds.getHeight()});
        engine_.appRequest(id, showApp, juce::JSON::toString(juce::var(argument), true).toStdString(), [shown = r.shown, pid = r.pid](const std::string& answer) {
#ifdef __APPLE__
            if(shown && answer == "shown") activateApp(pid); // its window is up: it comes in front of the shell's
#else
            juce::ignoreUnused(shown, pid, answer);
#endif
        });
    }

    bool AppProcesses::open(const std::string& id, juce::Rectangle<int> bounds, int mode)
    {
        if(!find(id)) return false;
        auto& r = running_[id];
        r.shown = true; r.bounds = bounds; r.mode = mode;
        if(r.state == State::GaveUp) r.crashes.clear(); // opened by hand: it may try again
        setFront(id);
        if(r.pid < 0) return start(id);
        show(id);
        return true;
    }

    void AppProcesses::hide(const std::string& id)
    {
        auto& r = running_[id];
        r.shown = false;
        if(r.pid > 0) show(id);
        if(front_ == id) front_.clear();
    }

    void AppProcesses::setFront(const std::string& id) { front_ = id; }

    void AppProcesses::say(const std::string& id, const juce::String& text, Notice::Choice choice)
    {
        if(text.isNotEmpty()) std::fprintf(stderr, "oscairn: %s\n", text.toRawUTF8());
        if(onNotice) onNotice({id, text, choice});
    }

    void AppProcesses::wait(const std::string& id)
    {
        auto& r = running_[id];
        if(r.state == State::NotResponding) { r.state = State::Running; r.waited = true; }
        say(id, {});
    }

    void AppProcesses::close(const std::string& id)
    {
        auto& r = running_[id];
        if(r.pid <= 0) return;
        r.why = "was not responding";
        ::kill(r.pid, SIGKILL); // reaped by the timer, as a crash
    }

    void AppProcesses::openAgain(const std::string& id)
    {
        auto& r = running_[id];
        r.crashes.clear();
        say(id, {});
        if(r.pid < 0) open(id, r.bounds, r.mode);
    }

    void AppProcesses::stop(const std::string& id)
    {
        auto& r = running_[id];
        r.shown = false;
        if(front_ == id) front_.clear();
        if(r.pid <= 0) return;
        r.stopping = true;
        ::kill(r.pid, SIGTERM); ::kill(r.pid, SIGCONT); // reaped by the timer; a stopped one takes the signal too
    }

    void AppPlace::place()
    {
        const bool showing = isShowing() && !getScreenBounds().isEmpty();
        if(showing && (!placed_ || getScreenBounds() != where_)) { where_ = getScreenBounds(); placed_ = apps_.open(id_, where_, 2); }
        else if(!showing && placed_) { placed_ = false; apps_.hide(id_); }
    }

    void AppProcesses::status(const std::map<std::string, uint32_t>& silent)
    {
        for(auto& [id, r] : running_)
        {
            const auto it = silent.find(id);
            if(r.pid <= 0 || it == silent.end()) continue;
            const auto* m = find(id);
            if(it->second < 1000)
            {
                if(r.state == State::NotResponding || r.waited) { r.state = State::Running; r.waited = false; say(id, {}); }
                continue;
            }
            if(id == front_ ? it->second >= frontSilence && r.state == State::Running && !r.waited : it->second >= behindSilence)
            {
                if(id != front_) { r.why = "was not responding"; ::kill(r.pid, SIGKILL); continue; }
                r.state = State::NotResponding;
                say(id, juce::String(m->name) + " is not responding.", Notice::Choice::WaitOrClose);
            }
        }
    }

    void AppProcesses::exited(const std::string& id, int status)
    {
        auto& r = running_[id];
        const auto* m = find(id);
        r.pid = -1;
        // Its reservation goes with it (RFC 0025), but for one started again at once.
        if(r.stopping) { r.state = State::Closed; engine_.appClosed(id); return; }
        // Closed by the musician (its own window): no crash.
        if(WIFEXITED(status) && WEXITSTATUS(status) == 0) { r.state = State::Closed; r.shown = false; engine_.appClosed(id); return; }
        auto why = r.why.isNotEmpty() ? r.why : WIFSIGNALED(status) ? juce::String("stopped (signal ") + juce::String(WTERMSIG(status)) + ")" : "stopped (exit " + juce::String(WEXITSTATUS(status)) + ")";
        const auto now = seconds();
        r.crashes.push_back(now);
        while(!r.crashes.empty() && now - r.crashes.front() > 60) r.crashes.pop_front();
        if(r.crashes.size() > restartsPerMinute)
        {
            r.state = State::GaveUp;
            engine_.appClosed(id);
            say(id, juce::String(m->name) + " " + why + " again and stays closed.", Notice::Choice::OpenAgain);
            return;
        }
        r.state = State::Closed;
        say(id, juce::String(m->name) + " " + why + (id == front_ ? "; started again." : "."));
        // In front: at once, restoring its own state from its data folder. Behind: when next opened.
        if(id == front_) start(id);
        else engine_.appClosed(id);
    }

    void AppProcesses::timerCallback()
    {
        for(auto& [id, r] : running_)
        {
            if(r.pid <= 0) continue;
            int status = 0;
            const auto reaped = ::waitpid(r.pid, &status, WNOHANG);
            if(reaped == r.pid) { exited(id, status); continue; }
            if(reaped < 0 && errno == ECHILD) { exited(id, 0); continue; } // reaped elsewhere: closed
            // Its memory: past its reservation, noted and kept as its reservation from its next
            // start (RFC 0025), saved when it grows by a MiB; over the manifest's maximum, stopped
            // and reported as a crash.
            const auto* m = find(id);
            // What it has used throughout the last `sustained` seconds (the least it read), once it
            // has run that long: what stops it and what it is measured using.
            const auto now = seconds();
            r.readings.emplace_back(now, m ? footprint(r.pid) : 0);
            while(r.readings.size() > 1 && r.readings[1].first <= now - sustained) r.readings.pop_front();
            const auto used = r.readings.front().first <= now - sustained ? std::min_element(r.readings.begin(), r.readings.end(), [](const auto& a, const auto& b) { return a.second < b.second; })->second : 0;
            if(auto& peak = measured_[id]; m && used > (m->reserve << 20) && used >= peak + (1 << 20))
            {
                if(peak <= (m->reserve << 20))
                    std::fprintf(stderr, "oscairn: %s uses more than its %llu MiB reservation; reserved at what it uses from its next start\n", m->name.c_str(), static_cast<unsigned long long>(m->reserve));
                peak = used;
                auto* saved = new juce::DynamicObject();
                for(const auto& [app, bytes] : measured_) if(bytes) saved->setProperty(juce::Identifier(juce::String(app)), juce::int64(bytes));
                measuredFile().replaceWithText(juce::JSON::toString(juce::var(saved)));
            }
            if(m && m->maximum && r.why.isEmpty() && used > (m->maximum << 20))
            {
                r.why = "used more than its " + juce::String(int64_t(m->maximum)) + " MB of memory and was stopped";
                ::kill(r.pid, SIGKILL);
            }
        }
    }

    AppNotice::AppNotice(AppProcesses& apps) : apps_(apps)
    {
        details_.onClick = [this] {
            const auto log = apps_.log(notice_.app);
            auto* text = new juce::TextEditor();
            text->setMultiLine(true); text->setReadOnly(true); text->setText(log.isEmpty() ? "Nothing in its log." : log, false); text->setSize(640, 360);
            text->moveCaretToEnd();
            juce::DialogWindow::LaunchOptions o;
            o.content.setOwned(text); o.dialogTitle = "What it wrote"; o.dialogBackgroundColour = ink(Ink::Background); o.useNativeTitleBar = true; o.resizable = true;
            o.launchAsync();
        };
        wait_.onClick = [this] { apps_.wait(notice_.app); };
        close_.onClick = [this] { const auto app = notice_.app; show({}); apps_.close(app); };
        again_.onClick = [this] { apps_.openAgain(notice_.app); };
        dismiss_.onClick = [this] { show({}); };
        for(auto* b : {&details_, &wait_, &close_, &again_, &dismiss_}) addAndMakeVisible(b);
        setVisible(false);
    }

    void AppNotice::show(const AppProcesses::Notice& notice)
    {
        notice_ = notice;
        using Choice = AppProcesses::Notice::Choice;
        wait_.setVisible(notice.choice == Choice::WaitOrClose); close_.setVisible(notice.choice == Choice::WaitOrClose);
        again_.setVisible(notice.choice == Choice::OpenAgain); dismiss_.setVisible(notice.choice != Choice::WaitOrClose);
        setVisible(notice.text.isNotEmpty());
        if(isVisible()) toFront(false);
        resized(); repaint();
    }

    void AppNotice::resized()
    {
        const int side = desktopMode(*this) == 2 ? 40 : 28;
        auto area = getLocalBounds().reduced(8, std::max(0, (getHeight() - side) / 2));
        for(auto* b : {&dismiss_, &again_, &close_, &wait_, &details_})
            if(b->isVisible()) { b->setBounds(area.removeFromRight(b == &again_ ? 120 : 84)); area.removeFromRight(6); }
    }

    void AppNotice::paint(juce::Graphics& g)
    {
        g.setColour(ink(Ink::Danger));
        g.fillRoundedRectangle(getLocalBounds().toFloat(), 6.f);
        int left = getRight() - 8;
        for(auto* b : {&details_, &wait_, &close_, &again_, &dismiss_}) if(b->isVisible()) left = std::min(left, b->getX() - 8);
        g.setColour(ink(Ink::Danger).contrasting());
        drawInked(g, notice_.text, getLocalBounds().reduced(12, 0).withRight(left),
                  juce::Font(juce::FontOptions(textSize(TextRole::Small, std::max(1, desktopMode(*this))))), juce::Justification::centredLeft);
    }
}
