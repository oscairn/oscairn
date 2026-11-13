// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Desktop controls for what was reachable only through the control API, against a real engine
// session over the control socket, without an audio device or any MIDI device. With SNAPSHOTS,
// each new view is written to SNAPSHOTS-<name>.png.
// - A part's MIDI out on the Part page, and clock out beside the transport (RFC 0012).
// - Signing an export and its signer (RFC 0006).
// - Asking before an app opens (RFC 0025).
// - The rich views a plugin describes, on the Panel and the Desktop (RFC 0019).
// - Installing from a repository by its https:// address, or its folder (RFC 0031).
//   oscairn_desktop_gaps_test PLUGIN_DIR SCANNER THEMES [SNAPSHOTS]
#include "../Catalog.h"
#include "../DesktopShell.h"
#include "../Launcher.h"
#include "../PlayView.h"
#include "../Profile.h"
#include "../LibraryPane.h"
#include "../Panel.h"
#include "../ParameterGrid.h"
#include "../PartsView.h"
#include "../Setlist.h"
#include "../SigningKeys.h"
#include "EngineProcess.h"
#include "oscairn/local_socket.h"
#include "oscairn/package.h"
#include "oscairn/repository.h"
#include "oscairn/session.h"
#include "oscairn/signing.h"
#include <juce_gui_extra/juce_gui_extra.h>
#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <filesystem>
#include <functional>
#include <map>
#include <mutex>
#include <optional>
#include <set>
#include <thread>
#include <tuple>
#include <typeinfo>
#include <poll.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

using namespace oscairn;

namespace
{
    // Between the Engine and the server: passes every byte on and counts the requests by
    // method, so a test can say which round trips a view or an edit made. drop() closes every
    // connection, as an engine timing out does.
    class SocketTap
    {
    public:
        SocketTap(std::string listenPath, std::string serverPath) : path_(std::move(listenPath)), server_(std::move(serverPath))
        {
            ::unlink(path_.c_str());
            listener_ = ::socket(AF_UNIX, SOCK_STREAM, 0);
            const auto address = addressOf(path_);
            if(::bind(listener_, reinterpret_cast<const sockaddr*>(&address), sizeof address) < 0 || ::listen(listener_, 16) < 0) throw std::runtime_error("tap listen");
            thread_ = std::thread([this] { run(); });
        }
        ~SocketTap()
        {
            running_ = false; thread_.join();
            for(auto& p : pairs_) { ::close(p.client); ::close(p.server); }
            ::close(listener_); ::unlink(path_.c_str());
        }
        int count(const std::string& method) const { std::lock_guard lock(mutex_); const auto at = counts_.find(method); return at == counts_.end() ? 0 : at->second; }
        void reset() { std::lock_guard lock(mutex_); counts_.clear(); kept_.clear(); pushed_.reset(); ++epoch_; }
        // The requests on connections open since before reset(), leaving out the new ones a job opens.
        int countKept(const std::string& method) const { std::lock_guard lock(mutex_); const auto at = kept_.find(method); return at == kept_.end() ? 0 : at->second; }
        // When the engine's first controls.changed since reset() passed, if one has.
        std::optional<std::chrono::steady_clock::time_point> pushed() const { std::lock_guard lock(mutex_); return pushed_; }
        // Answer a method with an error from here on, as the engine refuses it; empty passes it on again.
        void refuse(const std::string& method, const std::string& message) { std::lock_guard lock(mutex_); if(message.empty()) refusals_.erase(method); else refusals_[method] = message; }
        // Hold a method's requests, and every request after one, until stall(method, false): its
        // reply waits, as an engine busy with it does, and nothing overtakes it.
        void stall(const std::string& method, bool on) { std::lock_guard lock(mutex_); if(on) stalls_.insert(method); else stalls_.erase(method); }
        void drop() { dropping_ = true; while(dropping_) std::this_thread::sleep_for(std::chrono::milliseconds(1)); }

    private:
        struct Pair { int client, server; std::string sent, held; int epoch = 0; };
        static sockaddr_un addressOf(const std::string& path)
        {
            sockaddr_un a{}; a.sun_family = AF_UNIX; std::snprintf(a.sun_path, sizeof a.sun_path, "%s", path.c_str()); return a;
        }
        static void quiet([[maybe_unused]] int fd)
        {
#ifdef SO_NOSIGPIPE
            int yes = 1; ::setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &yes, sizeof yes);
#else // Linux has no per-socket option
            std::signal(SIGPIPE, SIG_IGN);
#endif
        }
        static bool writeAll(int to, const std::string& bytes)
        {
            for(size_t done = 0; done < bytes.size();) { const auto w = ::write(to, bytes.data() + done, bytes.size() - done); if(w <= 0) return false; done += size_t(w); }
            return true;
        }
        static std::string field(const std::string& line, const std::string& key, char end)
        {
            const auto at = line.find(key);
            if(at == std::string::npos) return {};
            const auto stop = line.find(end, at + key.size());
            return stop == std::string::npos ? std::string() : line.substr(at + key.size(), stop - at - key.size());
        }
        bool pass(int from, int to) // false once either side is gone
        {
            char buffer[65536];
            const auto n = ::read(from, buffer, sizeof buffer);
            if(n > 0 && std::string_view(buffer, size_t(n)).find("\"method\":\"controls.changed\"") != std::string_view::npos)
                { std::lock_guard lock(mutex_); if(!pushed_) pushed_ = std::chrono::steady_clock::now(); }
            return n > 0 && writeAll(to, std::string(buffer, size_t(n)));
        }
        // The client's requests, by line: counted, and passed on, or answered with the error
        // refuse() set for the method.
        bool forward(Pair& p)
        {
            char buffer[65536];
            const auto n = ::read(p.client, buffer, sizeof buffer);
            if(n <= 0) return false;
            p.sent.append(buffer, size_t(n));
            for(size_t end; (end = p.sent.find('\n')) != std::string::npos; p.sent.erase(0, end + 1))
            {
                const auto line = p.sent.substr(0, end + 1);
                const auto method = field(line, "\"method\":\"", '"'), id = field(line, "\"id\":", ',');
                std::string refusal;
                bool hold = false;
                { std::lock_guard lock(mutex_); ++counts_[method]; if(p.epoch < epoch_) ++kept_[method]; if(const auto at = refusals_.find(method); at != refusals_.end()) refusal = at->second; hold = !p.held.empty() || stalls_.count(method); }
                if(hold) { p.held += line; continue; }
                const bool refused = !refusal.empty() && !id.empty();
                if(!writeAll(refused ? p.client : p.server, refused ? "{\"jsonrpc\":\"2.0\",\"id\":" + id + ",\"error\":{\"code\":-32005,\"message\":\"" + refusal + "\"}}\n" : line)) return false;
            }
            return true;
        }
        void run()
        {
            while(running_)
            {
                if(dropping_) { for(auto& p : pairs_) { ::close(p.client); ::close(p.server); } pairs_.clear(); dropping_ = false; }
                if(std::lock_guard lock(mutex_); stalls_.empty()) for(auto& p : pairs_) if(!p.held.empty()) { writeAll(p.server, p.held); p.held.clear(); }
                std::vector<pollfd> fds{{listener_, POLLIN, 0}};
                for(const auto& p : pairs_) { fds.push_back({p.client, POLLIN, 0}); fds.push_back({p.server, POLLIN, 0}); }
                if(::poll(fds.data(), nfds_t(fds.size()), 5) <= 0) continue;
                const size_t polled = pairs_.size(); // a connection accepted now is polled next time
                if(fds[0].revents & POLLIN)
                {
                    const int client = ::accept(listener_, nullptr, nullptr);
                    const int server = ::socket(AF_UNIX, SOCK_STREAM, 0);
                    const auto address = addressOf(server_);
                    if(client >= 0 && server >= 0 && ::connect(server, reinterpret_cast<const sockaddr*>(&address), sizeof address) == 0) { quiet(client); quiet(server); { std::lock_guard lock(mutex_); pairs_.push_back({client, server, {}, {}, epoch_}); } }
                    else { if(client >= 0) ::close(client); if(server >= 0) ::close(server); }
                }
                std::vector<size_t> gone;
                for(size_t i = 0; i < polled; ++i)
                {
                    auto& p = pairs_[i];
                    bool alive = true;
                    if(fds[1 + 2 * i].revents & (POLLIN | POLLHUP | POLLERR)) alive = forward(p);
                    if(alive && (fds[2 + 2 * i].revents & (POLLIN | POLLHUP | POLLERR))) alive = pass(p.server, p.client);
                    if(!alive) gone.push_back(i);
                }
                for(auto i = gone.rbegin(); i != gone.rend(); ++i) { ::close(pairs_[*i].client); ::close(pairs_[*i].server); pairs_.erase(pairs_.begin() + std::ptrdiff_t(*i)); }
            }
        }
        std::string path_, server_;
        int listener_ = -1;
        std::vector<Pair> pairs_;
        std::atomic<bool> running_ = true, dropping_ = false;
        mutable std::mutex mutex_;
        std::map<std::string, int> counts_, kept_;
        int epoch_ = 0;
        std::map<std::string, std::string> refusals_; // method: the error it is answered with
        std::set<std::string> stalls_;
        std::optional<std::chrono::steady_clock::time_point> pushed_;
        std::thread thread_;
    };

    // An engine session of its own, served on a socket from a thread, as the engine process does.
    struct Served
    {
        std::unique_ptr<Session> session = std::make_unique<Session>(48000, 128);
        std::unique_ptr<LocalServer> server = std::make_unique<LocalServer>(*session);
        std::atomic<bool> running = true;
        std::thread thread;
        Served(const std::string& path, const std::function<void(Session&)>& setUp = {})
        {
            if(setUp) setUp(*session);
            server->listen(path);
            thread = std::thread([this] {
                std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
                while(running)
                {
                    server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                    session->service(); session->process(nullptr, 0, out, 2, 128);
                    std::this_thread::sleep_for(std::chrono::milliseconds(1));
                }
            });
        }
        ~Served() { running = false; thread.join(); server.reset(); session.reset(); }
    };
}

int main(int argc, char** argv)
{
    if(argc < 4) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } };
    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-gaps-" + std::to_string(::getpid()) + ".sock");
    // A data folder of its own, so the user's library and keys are never read or written.
    const auto data = juce::File::createTempFile("oscairn-gaps-data");
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    {
        auto session = std::make_unique<Session>(48000, 128);
        // Endpoints described to the session only: no device is opened.
        const MidiEndpoint modx{MidiEndpoint::Kind::Usb, 1, "MODX M"};
        Control::SessionStatus status; status.midiInputs = {{MidiEndpoint::Kind::Usb, 3, "KeyLab 61"}}; status.midiOutputs = {modx};
        session->setHostStatus(status);
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true, holdRequests = false, jobHeld = false, forcedHold = false;
        std::atomic<size_t> leaving = 0; // the last Performance's plugins still ringing out
        std::thread control([&] {
            std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                leaving = session->leaving();
                session->process(nullptr, 0, out, 2, 128);
                // The test leaves a job in flight: while its own connection shows, stop
                // answering, so its calls wait, as a long engine call does. A job asked behind
                // a refresh still gets by: the hold lasts two seconds and then serves one tick,
                // which is longer than any call's wait.
                const auto since = std::chrono::steady_clock::now();
                while(running && (forcedHold.load() || (holdRequests.load() && server->clients() >= 2 && std::chrono::steady_clock::now() - since < std::chrono::milliseconds(2000))))
                    { jobHeld = true; std::this_thread::sleep_for(std::chrono::milliseconds(5)); }
                jobHeld = false;
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset();
        });
        // The Engine reaches the server through the tap, which counts its requests.
        const auto tapPath = socket.string() + "-tap";
        SocketTap tap(tapPath, socket.string());
        {
            standalone::Engine engine(argv[1], tapPath);
            const auto configuration = juce::File::createTempFile("oscairn-gaps-config");
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            theme.apply("oscairn-dark");
            auto settle = [] { for(int i = 0; i < 30; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); };
            auto wait = [](auto done) { for(int i = 0; i < 1000 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            auto snapshot = [&](juce::Component& c, const juce::String& name) {
                if(argc < 5) return;
                const juce::File out{juce::String(argv[4]) + "-" + name + ".png"};
                out.deleteFile();
                juce::FileOutputStream stream(out);
                juce::PNGImageFormat().writeImageToStream(c.createComponentSnapshot(c.getLocalBounds()), stream);
            };
            const auto plugins = standalone::bundledPlugins(argv[1], argv[2]);
            uint32_t lead = 0;
            engine.edit([&](Control& c) {
                lead = standalone::addPart(c, "Lead", false);
                for(const auto& p : plugins) if(p.name == "Basic Synth") standalone::insertPlugin(c, lead, 0, p);
            });
            check(wait([&] { return engine.graph().parts.size() == 1; }) && engine.editError().empty(), "a part to send from: " + engine.editError());
            auto midiOut = [&] { for(const auto& p : engine.graph().parts) if(p.id == lead) return p.midiOut; return PartMidiOut{}; };

            // MIDI out on the Part page (RFC 0012 step 1): its outputs, then its channel.
            {
                standalone::PartsView view(engine, argv[2]);
                standalone::setDesktopMode(view, 1); view.setEmbedded(true); view.setSize(1180, 820);
                view.showPart(lead);
                juce::ComboBox* channel = nullptr;
                juce::TextButton* outputs = nullptr;
                auto channelBox = [&] { channel = nullptr; for(auto* c : view.getChildren()) if(auto* b = dynamic_cast<juce::ComboBox*>(c); b && b->getName() == "MIDI out channel") channel = b; return channel != nullptr; };
                auto outputsButton = [&] { outputs = nullptr; for(auto* c : view.getChildren()) if(auto* b = dynamic_cast<juce::TextButton*>(c); b && b->getTooltip().contains("MIDI outputs")) outputs = b; return outputs != nullptr; };
                check(channelBox() && outputsButton() && outputs->getButtonText() == "Off" && !channel->isEnabled(), "the Part page shows MIDI out, off");
                engine.edit([&](Control& c) { standalone::setMidiOut(c, lead, {standalone::toggled({}, standalone::ruleFor(modx)), 0}); });
                check(wait([&] { view.refresh(); return outputsButton() && outputs->getButtonText() == "MODX M"; }), "an output chosen shows by name");
                check(channelBox() && channel->isEnabled() && channel->getSelectedId() == 1, "and its channel can be set, each message's own at first");
                if(channel) channel->setSelectedId(4, juce::sendNotificationSync);
                check(wait([&] { return midiOut().channel == 3; }) && midiOut().devices.size() == 1, "the channel menu sets channel 3");
                view.refresh(); settle();
                snapshot(view, "part-midi-out");
            }
            // The MIDI channel a part's input takes: Any channel, or one of 1 to 16, beside the
            // devices pick. Only the input zones' channels change; keys and velocities stay.
            {
                standalone::PartsView view(engine, argv[2]);
                standalone::setDesktopMode(view, 1); view.setEmbedded(true); view.setSize(1180, 820);
                view.showPart(lead);
                juce::ComboBox* pick = nullptr;
                auto channelBox = [&] { pick = nullptr; for(auto* c : view.getChildren()) if(auto* b = dynamic_cast<juce::ComboBox*>(c); b && b->getName() == "Input channel") pick = b; return pick != nullptr; };
                auto devicesButton = [&] { for(auto* c : view.getChildren()) if(auto* b = dynamic_cast<juce::TextButton*>(c); b && b->getTooltip().contains("play this part")) return b; return static_cast<juce::TextButton*>(nullptr); };
                auto input = [&] { for(const auto& p : engine.graph().parts) if(p.id == lead) return p.input; return PartInput{}; };
                engine.edit([&](Control& c) { standalone::setZone(c, lead, 48, 72, 0); });
                check(wait([&] { view.refresh(); return channelBox() && pick->getSelectedId() == 1 && pick->getItemText(0) == "Any channel"; }) && input().zones.front().channels == 0xFFFF,
                      "the input takes every channel at first");
                check(devicesButton() && devicesButton()->getTooltip().contains("whatever channel they send on"), "the devices pick says so while any channel plays");
                if(pick) pick->setSelectedId(3, juce::sendNotificationSync); // Channel 2
                check(wait([&] { return input().zones.front().channels == (1u << 1); }) && input().zones.front().lowKey == 48 && input().zones.front().highKey == 72,
                      "Channel 2: only that channel plays, the keys kept");
                check(wait([&] { view.refresh(); return devicesButton() && devicesButton()->getTooltip().contains("on channel 2"); }), "and the devices pick names the channel");
                // One undo step, as any part edit: every channel back, the keys kept.
                engine.edit([](Control& c) { c.undo(); });
                check(wait([&] { view.refresh(); return !input().zones.empty() && input().zones.front().channels == 0xFFFF && input().zones.front().lowKey == 48 && channelBox() && pick->getSelectedId() == 1; }),
                      "one undo puts every channel back, the keys kept, and the choice shows it");
                engine.edit([](Control& c) { c.redo(); });
                check(wait([&] { view.refresh(); return !input().zones.empty() && input().zones.front().channels == (1u << 1) && channelBox() && pick->getSelectedId() == 3; }), "and redo picks channel 2 again");
                // Zones set elsewhere to several channels show Several channels and wait for the user.
                engine.edit([&](Control& c) {
                    auto in = input(); in.zones = {NoteZone{48, 72, 0, 1, uint16_t(1u << 1)}, NoteZone{48, 72, 0, 1, uint16_t(1u << 5)}}; c.setPartInput(lead, in); c.commit();
                });
                check(wait([&] { view.refresh(); return channelBox() && pick->getSelectedId() == 18 && input().zones.size() == 2; }), "several channels shown, kept: " + (pick ? pick->getText().toStdString() : std::string()));
                if(pick) pick->setSelectedId(5, juce::sendNotificationSync); // Channel 4
                check(wait([&] { return input().zones.size() == 1 && input().zones[0].channels == (1u << 3); }), "picking one channel brings them together, keys kept");
                // Saved and opened again: the same choice, the same keys.
                const auto folder = data.getChildFile("part-channel");
                standalone::SoundAction save; save.kind = standalone::SoundAction::SavePerformance; save.directory = folder.getFullPathName().toStdString();
                engine.soundAction(std::move(save));
                check(wait([&] { return !engine.soundBusy() && folder.getChildFile("performance.json").existsAsFile(); }), "the Performance saved: " + engine.soundMessage());
                standalone::SoundAction open; open.kind = standalone::SoundAction::LoadPerformance; open.directory = folder.getFullPathName().toStdString();
                engine.soundAction(std::move(open));
                check(wait([&] { return !engine.soundBusy() && !engine.graph().parts.empty(); }) && input().zones.front().channels == (1u << 3),
                      "opened again with channel 4: " + engine.soundMessage());
                check(wait([&] { view.refresh(); return channelBox() && pick->getSelectedId() == 5; }), "and the Part page shows it");
                if(pick) pick->setSelectedId(1, juce::sendNotificationSync); // Any channel
                check(wait([&] { return input().zones.front().channels == 0xFFFF && input().zones.front().lowKey == 48; }), "Any channel clears the filter, keys kept");
                view.refresh(); settle();
                snapshot(view, "part-channel");
                std::filesystem::remove_all(folder.getFullPathName().toStdString());
            }
            // The same choice in Routing, on a note wire drawn by hand from the keys, following undo and edits from elsewhere.
            {
                NodeId synth = 0;
                for(const auto& p : standalone::readParts(engine.graph()).parts) if(p.id == lead && !p.chain.empty()) synth = p.chain.front().node;
                engine.edit([&](Control& c) { standalone::connectNodes(c, graphInput, synth, WireKind::Notes); });
                auto keys = [&] { for(const auto& w : engine.graph().wires) if(w.owner == WireOwner::User && w.from == graphInput && w.to == synth) return w; return Wire{}; };
                check(wait([&] { return keys().id != 0; }), "a note wire from the keys to the lead's synth");
                standalone::PartsView view(engine, argv[2]);
                standalone::setDesktopMode(view, 1); view.setEmbedded(true); view.setSize(1180, 820);
                view.showRouting(synth);
                juce::ComboBox* pick = nullptr;
                auto wireBox = [&] { pick = nullptr; for(auto* c : view.getChildren()) if(auto* b = dynamic_cast<juce::ComboBox*>(c); b && b->getName() == "Wire channel") pick = b; return pick != nullptr; };
                check(wait([&] { view.refresh(); return wireBox() && pick->getSelectedId() == 1; }), "Routing: the wire takes every channel at first");
                if(pick) pick->setSelectedId(3, juce::sendNotificationSync); // Channel 2
                check(wait([&] { return keys().zones.size() == 1 && keys().zones[0].channels == (1u << 1); }), "Channel 2: the wire passes only that channel");
                engine.edit([](Control& c) { c.undo(); });
                check(wait([&] { view.refresh(); return keys().zones.empty() && wireBox() && pick->getSelectedId() == 1; }), "undo: every channel again, and the choice shows it");
                // Zones set from elsewhere (another app, wires.zones) show too.
                engine.edit([&](Control& c) { c.setZones(keys().id, {NoteZone{0, 127, 0, 1, uint16_t(1u << 4)}}); c.commit(); });
                check(wait([&] { view.refresh(); return wireBox() && pick->getSelectedId() == 6; }), "zones set elsewhere: the choice shows channel 5");
                engine.edit([&](Control& c) { standalone::disconnect(c, keys().id); });
                check(wait([&] { return keys().id == 0; }), "the wire disconnected again");
            }
            // Clock out beside the transport (RFC 0012 step 2): cabling, kept by the engine.
            {
                standalone::SequencerPane pane(engine);
                standalone::setDesktopMode(pane, 1); pane.setSize(1100, 760);
                pane.refresh();
                check(pane.clockOut().isVisible() && pane.clockOut().getButtonText() == "Clock out: Off", "clock out shows beside Follow MIDI clock, off: " + pane.clockOut().getButtonText().toStdString());
                engine.setClockOutputs({standalone::ruleFor(modx)});
                check(wait([&] { pane.refresh(); return pane.clockOut().getButtonText() == "Clock out: MODX M"; }), "an output chosen for clock shows by name");
                settle();
                snapshot(pane, "clock-out");
                engine.setClockOutputs({});
            }
            // Signing on export (RFC 0006): off by default; a key made from the export's choices,
            // chosen, signs the archive, whose signer the install dialog then names.
            {
                standalone::ExportChoices choices(engine, true);
                standalone::setDesktopMode(choices, 1);
                check(choices.key().empty() && choices.signWith.getSelectedId() == 1 && choices.signWith.getNumItems() == 2 && choices.signWith.getItemText(1) == "Make a key...",
                      "signing is off at first, with Make a key... offered");
                Control::SigningKey made;
                check(engine.makeSigningKey("stage", made).empty() && made.id.size() == 16, "a key made");
                choices.chooseKey("stage");
                check(choices.key() == "stage" && choices.signWith.getNumItems() == 3 && choices.signWith.getText() == "Sign with stage", "listed, and chosen once made");
                snapshot(choices, "export-sign");
                auto exportTo = [&](const char* name, const std::string& key) {
                    const auto file = data.getChildFile(name);
                    standalone::SoundAction out; out.kind = standalone::SoundAction::ExportPerformance; out.directory = file.getFullPathName().toStdString(); out.signKey = key;
                    engine.soundAction(std::move(out));
                    Control::Archive archive;
                    check(wait([&] { return !engine.soundBusy(); }) && file.existsAsFile() && engine.archiveContents(file.getFullPathName().toStdString(), archive).empty(), std::string("exported: ") + engine.soundMessage());
                    return archive;
                };
                const auto plain = exportTo("Plain.olib", {}), signedArchive = exportTo("Signed.olib", choices.key());
                check(plain.signer.status == "unsigned" && standalone::describeSigner(plain.signer) == "Not signed", "unsigned unless a key is chosen");
                check(signedArchive.signer.status == "unknown" && signedArchive.signer.key == made.id && engine.soundMessage().find("Signed with stage") != std::string::npos,
                      "signed with the key chosen: " + signedArchive.signer.status + " " + engine.soundMessage());
                const auto said = standalone::describeSigner(signedArchive.signer);
                check(said.contains(standalone::fingerprint(made.id)) && said.contains("not a key you trust"), "the install dialog names its signer by fingerprint: " + said.toStdString());
                // A large archive's install dialog: the theme's background, the items scrolling in a
                // viewport between the options and the buttons, every row full height.
                {
                    Control::Archive big = signedArchive;
                    big.items.clear();
                    for(int i = 0; i < 40; ++i) { Control::LibraryItem item; item.id = "item-" + std::to_string(i); item.kind = "sound"; item.name = "Sound " + std::to_string(i); big.items.push_back(item); }
                    Control::Signer build; build.name = "Synth"; build.version = "1.0"; build.platform = "macos-arm64"; build.status = "unsigned";
                    big.code = {build};
                    standalone::InstallChoices install(big);
                    standalone::setDesktopMode(install, 1); install.resized();
                    const auto view = install.view.getBoundsInParent(), go = install.go.getBoundsInParent(), cancel = install.cancel.getBoundsInParent(), code = install.code.getBoundsInParent();
                    bool inList = install.items.size() == 40, fullRows = true;
                    for(auto* b : install.items) { inList = inList && b->getParentComponent() == &install.list; fullRows = fullRows && b->getHeight() == 30; }
                    check(install.getHeight() <= 630 && install.view.getViewedComponent() == &install.list && inList && fullRows && install.list.getHeight() == 26 + 40 * 30 + 4 + 26 + 30 && view.getHeight() < install.list.getHeight(),
                          "the items scroll in a viewport, every row full height");
                    check(install.headings.size() == 2 && install.headings[1]->getText() == "Plugins" && install.plugins.size() == 1 && install.plugins[0]->getParentComponent() == &install.list &&
                              install.plugins[0]->getText() == "Synth 1.0 (macos-arm64): Not signed" && install.plugins[0]->getBounds().getY() == 26 + 40 * 30 + 4 + 26,
                          "under Plugins, after the items, each build with who signed it: " + install.plugins[0]->getText().toStdString());
                    check(install.getLocalBounds().contains(go) && install.getLocalBounds().contains(cancel) && view.getBottom() <= code.getY() && code.getBottom() <= go.getY() && install.go.getButtonText() == "Install anyway",
                          "the warning and the buttons stay in view below the items");
                    const auto image = install.createComponentSnapshot(install.getLocalBounds());
                    check(image.getPixelAt(2, 2) == standalone::ink(standalone::Ink::Background), "the dialog paints the theme's background");
                    snapshot(install, "install-many");
                }
                // A plugin package: its builds under Plugins, each with its signer, and no choice of
                // how to install, since it has no items.
                {
                    Control::Archive plugin = signedArchive;
                    plugin.kind = "plugin"; plugin.items.clear();
                    Control::Signer trusted; trusted.name = "Synth"; trusted.version = "2.0"; trusted.platform = "macos-arm64"; trusted.status = "trusted"; trusted.key = made.id; trusted.keyName = "Maker";
                    Control::Signer revoked = trusted; revoked.platform = "linux-arm64"; revoked.status = "revoked";
                    plugin.code = {trusted, revoked};
                    standalone::InstallChoices install(plugin);
                    standalone::setDesktopMode(install, 1); install.resized();
                    std::vector<std::string> shown;
                    for(auto* c : install.list.getChildren()) if(auto* l = dynamic_cast<juce::Label*>(c)) shown.push_back(l->getText().toStdString());
                    const auto print = standalone::fingerprint(made.id).toStdString();
                    const std::vector<std::string> want{"Plugins", "Synth 2.0 (macos-arm64): Signed by Maker (" + print + "), a key you trust",
                                                        "Synth 2.0 (linux-arm64): Signed by Maker (" + print + "), which is revoked, not installed"};
                    check(shown == want && install.pluginPackage && install.items.isEmpty() && !install.own.isVisible() && !install.locked.isVisible() && install.go.getButtonText() == "Install",
                          "a plugin package lists its plugins under Plugins with their signers, without a choice of how: " + (shown.size() > 1 ? shown[1] : std::string()));
                    snapshot(install, "install-plugin");
                    // Under the GPL: its license, and a link to its source at the commit it names.
                    plugin.license = "GPL-3.0-or-later"; plugin.sourceUrl = "https://github.com/oscairn/setbfree"; plugin.sourceRevision = "v0.8.13-oscairn.1";
                    standalone::InstallChoices licensed(plugin);
                    standalone::setDesktopMode(licensed, 1); licensed.resized();
                    std::vector<std::string> labels;
                    for(auto* c : licensed.list.getChildren()) if(auto* l = dynamic_cast<juce::Label*>(c)) labels.push_back(l->getText().toStdString());
                    check(labels.size() == 5 && labels[3] == "License" && labels[4] == "GPL-3.0-or-later" && licensed.source.isVisible() &&
                          licensed.source.getURL().toString(false) == "https://github.com/oscairn/setbfree" &&
                          licensed.source.getButtonText() == "Source: https://github.com/oscairn/setbfree at v0.8.13-oscairn.1" && licensed.source.getBottom() <= licensed.list.getHeight(),
                          "a plugin package under the GPL shows its license and a link to its source: " + licensed.source.getButtonText().toStdString());
                    snapshot(licensed, "install-plugin-license");
                }
                // An archive of both kinds, mixed: its Performances under one heading, then its
                // Sounds under another, in the archive's order, no kind after a name; still all ticked.
                {
                    Control::Archive mixed = signedArchive;
                    mixed.items.clear(); mixed.code.clear();
                    for(const auto& [kind, name] : std::vector<std::pair<std::string, std::string>>{{"sound", "Bass"}, {"performance", "Set A"}, {"sound", "Keys"}, {"performance", "Set B"}, {"sound", "Pad"}})
                    {
                        Control::LibraryItem item; item.id = name; item.kind = kind; item.name = name; mixed.items.push_back(item);
                    }
                    standalone::InstallChoices install(mixed);
                    standalone::setDesktopMode(install, 1); install.resized();
                    std::vector<std::string> shown, focus;
                    bool ticked = true, idsInOrder = true;
                    for(auto* c : install.list.getChildren())
                    {
                        if(auto* l = dynamic_cast<juce::Label*>(c)) shown.push_back("# " + l->getText().toStdString());
                        if(auto* b = dynamic_cast<juce::ToggleButton*>(c)) { shown.push_back(b->getButtonText().toStdString()); ticked = ticked && b->getToggleState(); }
                    }
                    for(auto* c : install.createFocusTraverser()->getAllComponents(&install))
                        if(c->getParentComponent() == &install.list) focus.push_back(dynamic_cast<juce::Label*>(c) ? "# " + dynamic_cast<juce::Label*>(c)->getText().toStdString() : dynamic_cast<juce::Button*>(c)->getButtonText().toStdString());
                    const std::vector<std::string> want{"# Performances", "Set A", "Set B", "# Sounds", "Bass", "Keys", "Pad"};
                    for(size_t i = 0; i < install.ids.size() && i < 5; ++i) idsInOrder = idsInOrder && install.ids[i] == std::vector<std::string>{"Set A", "Set B", "Bass", "Keys", "Pad"}[i];
                    check(shown == want && ticked && install.ids.size() == 5 && idsInOrder, "the install items under a heading for each kind, Performances first, no suffixes");
                    std::vector<juce::Component*> keys, toggles(install.items.begin(), install.items.end());
                    for(auto* c : juce::KeyboardFocusTraverser().getAllComponents(&install)) if(c->getParentComponent() == &install.list) keys.push_back(c);
                    check(focus == want && keys == toggles, "Tab goes down the items in order, and a screen reader reads each heading before its items");
                    snapshot(install, "install-mixed");
                }
                // A Performance with many parts and scenes to import from: the dialog no taller than
                // the screen, the headings and toggles scrolling in a viewport, the buttons below it.
                {
                    Control::PerformanceContents held;
                    for(uint32_t i = 1; i <= 30; ++i) held.parts.push_back({i, "Part " + std::to_string(i), i == 30});
                    for(uint32_t i = 1; i <= 20; ++i) held.scenes.push_back({i, "Scene " + std::to_string(i)});
                    standalone::ImportChoices choose(held, 600);
                    standalone::setDesktopMode(choose, 1); choose.resized();
                    const auto view = choose.view.getBoundsInParent(), import = choose.import.getBoundsInParent(), cancel = choose.cancel.getBoundsInParent();
                    bool inList = choose.partsHeading.getParentComponent() == &choose.list && choose.scenesHeading.getParentComponent() == &choose.list;
                    for(auto* b : choose.parts) inList = inList && b->getParentComponent() == &choose.list;
                    for(auto* b : choose.scenes) inList = inList && b->getParentComponent() == &choose.list;
                    check(choose.getHeight() <= 600 && choose.view.getViewedComponent() == &choose.list && inList && choose.list.getHeight() == 26 + 30 * 30 + 30 + 20 * 30 && view.getHeight() < choose.list.getHeight(),
                          "the import list scrolls in a viewport, the dialog within the screen: " + std::to_string(choose.getHeight()));
                    check(choose.getLocalBounds().contains(import) && choose.getLocalBounds().contains(cancel) && import.getBottom() <= 600 && !view.isEmpty() && view.getBottom() <= import.getY() && choose.import.isVisible() && choose.cancel.isVisible(),
                          "the import buttons stay on the screen, below the list");
                    // Tab goes down the toggles in order, then Cancel and Import; a screen reader
                    // reads each heading before its toggles.
                    std::vector<juce::Component*> keys, read{&choose.partsHeading};
                    for(auto* b : choose.parts) { keys.push_back(b); read.push_back(b); }
                    read.push_back(&choose.scenesHeading);
                    for(auto* b : choose.scenes) { keys.push_back(b); read.push_back(b); }
                    for(auto* b : {&choose.cancel, &choose.import}) { keys.push_back(b); read.push_back(b); }
                    auto mine = [&](std::vector<juce::Component*> all) { std::erase_if(all, [&](auto* c) { return std::find(read.begin(), read.end(), c) == read.end(); }); return all; };
                    check(mine(juce::KeyboardFocusTraverser().getAllComponents(&choose)) == keys && mine(choose.createFocusTraverser()->getAllComponents(&choose)) == read,
                          "the import toggles and buttons take focus in order, each heading read before its toggles");
                    Control::PerformanceContents few; few.parts = {{1, "Lead", false}, {2, "Pad", false}};
                    standalone::ImportChoices small(few, 600);
                    check(small.getHeight() == 2 * 30 + 100 && !small.scenesHeading.isVisible(), "a short import list is as tall as it needs");
                    for(const auto* name : {"import-many", "import-many-scrolled"})
                    {
                        if(argc < 5) break;
                        juce::Image image(juce::Image::ARGB, choose.getWidth(), choose.getHeight(), true);
                        juce::Graphics g(image); g.fillAll(standalone::ink(standalone::Ink::Background)); // the dialog's background
                        choose.paintEntireComponent(g, true);
                        const juce::File out{juce::String(argv[4]) + "-" + name + ".png"};
                        out.deleteFile(); juce::FileOutputStream stream(out); juce::PNGImageFormat().writeImageToStream(image, stream);
                        choose.view.setViewPosition(0, choose.scenesHeading.getY() - 120); // the last parts and the Scenes heading
                    }
                }
            }
            // Asking before an app opens (RFC 0025): with memory classes, a launch past what is
            // free is refused with the reason; the prepared next entry in the way can go.
            {
                check(engine.launchApp({"oscairn.library", 120ull << 20}).admitted, "without memory classes every app opens");
                engine.appClosed("oscairn.library");
                // The protocol takes a reservation of at most 4 GiB, so a small device.
                constexpr uint64_t MiB = 1ull << 20;
                // The synth measured at 100 MiB, read when the budget is set, so the next song holds memory.
                // Its ID, version and build digest (Session::pluginKey): the graph over the socket does not carry the version.
                std::string key;
                for(const auto& p : plugins) if(p.name == "Basic Synth") key = "org.oscairn.basic-synth@0.1.0 " + bundleDigest(p.path);
                data.getChildFile("measurements.json").replaceWithText("{\"plugins\":{\"" + key + "\":{\"cpu\":0.01,\"rest\":0.01,\"voice\":0,\"voices\":0,\"memory\":" + std::to_string(100 * MiB) + "}}}");
                engine.edit([](Control& c) { c.setBudget({.9, 0, 3072 * MiB, 256 * MiB, 0, 64 * MiB}); });
                check(engine.launchApp({"oscairn.library", 120ull << 20}).admitted, "the Library fits");
                std::string path;
                engine.edit([&](Control& c) {
                    c.savePerformanceToLibrary("Next song", true);
                    Control::LibraryQuery q; q.text = "Next song";
                    for(const auto& item : c.libraryQuery(q).items) path = item.path;
                });
                Control::MemoryState before, after;
                engine.edit([&](Control& c) { before = c.memoryState(); c.preparePerformance(path); after = c.memoryState(); });
                const auto next = after.committed - before.committed;
                check(!path.empty() && after.nextCommitted && next > 0, "the next song prepared ahead, committed: " + std::to_string(after.committed) + "/" + std::to_string(before.committed) + " " + engine.editError());
                // Just too big beside the next song, and small enough without it.
                const uint64_t reserve = after.device - after.reserved - after.margin - after.committed + next / 2;
                std::unique_ptr<standalone::LaunchNotice> notice;
                bool retried = false;
                auto show = [&](std::unique_ptr<standalone::LaunchNotice> n) { notice = std::move(n); };
                check(!standalone::askToLaunch(engine, {"oscairn.groovebox", reserve}, "Groovebox", [&] { retried = true; }, show) && notice && notice->letGo.isVisible(),
                      "refused, with Let the next song go offered");
                if(notice)
                {
                    standalone::setDesktopMode(*notice, 2); notice->setSize(1024, 600);
                    check(notice->message().contains("letting the next entry"), "it says why: " + notice->message().toStdString());
                    snapshot(*notice, "launch-next-song");
                    notice->letGo.onClick();
                    check(wait([&] { return retried && !notice; }), "letting it go closes the notice and tries again");
                }
                Control::MemoryState now;
                engine.edit([&](Control& c) { now = c.memoryState(); });
                check(!now.nextPrepared && std::any_of(now.running.begin(), now.running.end(), [](const auto& a) { return a.app == "oscairn.groovebox"; }), "the next song let go, the Groovebox admitted");
                check(!standalone::askToLaunch(engine, {"oscairn.setlist", 3500 * MiB}, "Setlist", {}, show) && notice && !notice->letGo.isVisible() &&
                      notice->message().contains("close") && notice->message().contains("Library"), "refused past what closing frees, saying what to close: " + (notice ? notice->message().toStdString() : std::string()));
                if(notice) { standalone::setDesktopMode(*notice, 1); notice->setSize(1024, 600); snapshot(*notice, "launch-refused"); notice->ok.onClick(); wait([&] { return !notice; }); }
                engine.appClosed("oscairn.library");
                engine.edit([&](Control& c) { now = c.memoryState(); });
                check(std::none_of(now.running.begin(), now.running.end(), [](const auto& a) { return a.app == "oscairn.library"; }), "a closed app's reservation is let go");
                engine.edit([](Control& c) { c.setBudget({}); });
            }
            // A refresh in flight (finding: the periodic sound refresh every ~30 ticks) must not
            // make the engine busy: it reads on its own connection and makes only short calls,
            // so Record, Save and the library work while it runs.
            {
                holdRequests = true;
                standalone::SoundAction tick; tick.kind = standalone::SoundAction::Refresh;
                engine.soundAction(std::move(tick)); // the periodic refresh, or this one, next
                check(wait([&] { return jobHeld.load(); }), "a refresh job's own connection is in flight");
                check(engine.controlReady(), "a refresh in flight leaves the controls ready");
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load(); }), "the refresh finished");
            }
            // The polled reads (meters, undo names, the open Performance's name) come from the
            // Engine's cache while a job holds the engine's thread, so the UI thread neither
            // waits on the socket nor drops the connection over a stalled request.
            {
                std::string probe;
                engine.edit([&](Control& c) {
                    c.savePerformanceToLibrary("Cache probe", true);
                    Control::LibraryQuery q; q.text = "Cache probe";
                    for(const auto& item : c.libraryQuery(q).items) probe = item.path;
                });
                check(wait([&] { return !probe.empty() && engine.editError().empty(); }), "a Performance to prepare: " + engine.editError());
                check(wait([&] { return engine.performanceInfo().has_value() && !engine.history().undo.empty(); }), "the reads are primed");
                wait([&] { return !engine.jobRunning(); }); // a refresh held too would time out: the job held is the one asked
                holdRequests = true;
                standalone::SoundAction prepare; prepare.kind = standalone::SoundAction::Prepare; prepare.directory = probe;
                engine.soundAction(std::move(prepare));
                check(wait([&] { return engine.soundBusy() && !engine.controlReady() && jobHeld.load(); }), "a prepare job holds the engine's thread");
                const auto began = std::chrono::steady_clock::now();
                const auto info = engine.performanceInfo(); const auto undo = engine.history().undo; const auto meters = engine.meters();
                const auto waited = std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now() - began).count();
                check(info && !undo.empty(), "the reads answer from the cache: " + std::to_string(meters.size()));
                check(engine.connected(), "and no call waits on the stalled socket");
                check(waited < 100000, "at once, not after a round trip: " + std::to_string(waited) + " us");
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load() && engine.connected(); }), "the prepare finished, the engine answers again");
                engine.edit([](Control& c) { c.addMacro("Cache probe control"); });
                check(wait([&] { return engine.history().undo.find("Cache probe control") != std::string::npos && engine.connected(); }), "the cache follows the engine again once free");
            }
            // A fader dragged sends 60 edits a second: the edits leave the meters' cache alone,
            // so the views polling between them still read the engine at most twenty times a second.
            {
                NodeId synth = 0;
                for(const auto& [id, n] : engine.graph().names) if(n == "Basic Synth") synth = id;
                const auto node = engine.graph().names.at(synth);
                check(wait([&] { return !engine.soundBusy(); }), "the engine is free for a drag");
                engine.meters(); engine.wireMeters(); engine.history();
                tap.reset();
                const auto began = juce::Time::getMillisecondCounter();
                for(int i = 0; i < 60; ++i) { engine.setParam(node, 4, i % 3); engine.meters(); engine.wireMeters(); engine.history(); }
                const int allowed = int((juce::Time::getMillisecondCounter() - began) / 50) + 1;
                // Sent without waiting: the tap has them once its thread has read them.
                check(wait([&] { return tap.count("params.set") == 60; }), "the drag's 60 edits sent: " + std::to_string(tap.count("params.set")));
                check(tap.count("parts.meters") <= allowed && tap.count("wires.meters") <= allowed && tap.count("edit.history") <= allowed,
                      "the meters and undo names read at their own pace, not per edit: " + std::to_string(tap.count("parts.meters")) + " " + std::to_string(tap.count("wires.meters")) + " " +
                      std::to_string(tap.count("edit.history")) + " of at most " + std::to_string(allowed));
                // The periodic refresh reads on its own connection and changes nothing: a note
                // or a fader move asked while it runs goes at once, not after it.
                check(wait([&] { return !engine.jobRunning(); }), "no job before the refresh");
                standalone::SoundAction tick; tick.kind = standalone::SoundAction::Refresh;
                engine.soundAction(std::move(tick));
                check(engine.jobRunning(), "a refresh runs");
                tap.reset();
                engine.setParam(node, 4, 1); engine.note(true, 60, 1.f); engine.note(false, 60, 0);
                check(engine.jobRunning() && tap.count("notes.send") == 2 && wait([&] { return tap.count("params.set") == 1; }), "a fader move and a note go at once during a refresh: " + std::to_string(tap.count("params.set")) + " " + std::to_string(tap.count("notes.send")));
                // The overrides of the physical controls are read at most four times a second, as Play and a controller ask.
                check(wait([&] { return !engine.jobRunning(); }), "the refresh finished");
                engine.assignments(); tap.reset();
                const auto asked = juce::Time::getMillisecondCounter();
                for(int i = 0; i < 20; ++i) engine.assignments();
                const int most = int((juce::Time::getMillisecondCounter() - asked) / 250) + 1;
                check(tap.count("assignments.get") <= most, "the assignments read at their own pace: " + std::to_string(tap.count("assignments.get")) + " of at most " + std::to_string(most));
            }
            // A fader or a mute (RFC 0011's instant controls) shows in the Engine's state at once,
            // before the engine answers, and the edit never waits on the socket: a stalled reply
            // does not hold it, and a drag reads no graph back.
            {
                // The opened Performance's predecessor rings out until idle on the engine's audio
                // timeline, then goes, a graph change every client reads: ended now, and read,
                // not in the edits counted below.
                const bool ringing = leaving > 0; const auto revision = engine.graph().revision;
                engine.allNotesOff();
                check(wait([&] { return leaving == 0 && (!ringing || engine.graph().revision > revision); }), "the last Performance's plugins rung out and read: " + std::to_string(leaving));
                check(wait([&] { return !engine.jobRunning() && !engine.soundBusy(); }), "the engine is free for the faders");
                auto part = [&] { for(const auto& p : engine.graph().parts) if(p.id == lead) return p; return Part{}; };
                auto level = [&](double v, bool m) { engine.edit([&, v, m](Control& c) { c.setPartLevel(lead, v, m); c.commit(); }); };
                auto micros = [](auto since) { return std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now() - since).count(); };
                tap.reset(); tap.stall("parts.level", true);
                auto began = std::chrono::steady_clock::now();
                level(.3, false);
                const auto edited = micros(began);
                const auto shown = part();
                const auto read = micros(began);
                check(edited < 20000 && engine.connected(), "a level edit with its reply stalled returns at once: " + std::to_string(edited) + " us");
                check(std::abs(shown.level - .3) < 1e-9 && [&] { for(const auto& f : engine.liveValues().parts) if(f.part == lead) return std::abs(f.level - .3) < 1e-9; return false; }(), "and the state shows it before any reply, " + std::to_string(read) + " us after the edit");
                began = std::chrono::steady_clock::now();
                level(.3, true);
                check(micros(began) < 20000 && part().mute, "a mute shows at once too, its reply stalled: " + std::to_string(micros(began)) + " us");
                tap.stall("parts.level", false);
                settle();
                check(engine.connected() && part().mute && std::abs(part().level - .3) < 1e-9 && engine.editError().empty(), "and stays after the engine's answers and pushes");
                check(tap.countKept("graph.get") <= 1, "a mute reads the graph once, for its wires, and a level not at all: " + std::to_string(tap.countKept("graph.get")));
                level(1, false); settle();
                tap.reset(); began = std::chrono::steady_clock::now();
                level(.4, false);
                const bool moved = std::abs(part().level - .4) < 1e-9; const auto state = micros(began);
                check(moved && wait([&] { return tap.pushed().has_value(); }), "the engine pushes the level it took");
                if(const auto at = tap.pushed()) std::printf("instant controls: the state %lld us after the edit, the engine's push %lld us\n", (long long)state,
                                                             (long long)std::chrono::duration_cast<std::chrono::microseconds>(*at - began).count());
                settle(); tap.reset();
                for(int i = 0; i < 60; ++i) { level(i / 60.0, false); juce::MessageManager::getInstance()->runDispatchLoopUntil(1); }
                settle();
                check(tap.count("parts.level") == 60 && tap.countKept("graph.get") == 0, "a drag of 60 moves reads no graph: " + std::to_string(tap.count("parts.level")) + " " + std::to_string(tap.countKept("graph.get")));
                check(std::abs(part().level - 59 / 60.0) < 1e-9 && engine.connected(), "and ends where the drag left it: " + std::to_string(part().level));
                // A refusal is said, and the engine's value shown again.
                tap.refuse("parts.level", "no such fader");
                level(.5, false);
                check(std::abs(part().level - .5) < 1e-9, "a refused edit shows until the engine answers");
                check(wait([&] { return engine.editError() == "no such fader"; }) && wait([&] { return std::abs(part().level - 59 / 60.0) < 1e-9; }), "then its refusal is said and the engine's level shown: " + engine.editError());
                tap.refuse("parts.level", "");
                level(1, false); settle();
                // A window showing the part (the desktop's parts list) redraws a mute edited
                // elsewhere at once, not at its next timer (every 200 ms): drawn off screen.
                standalone::DesktopShell shell(engine, argv[2], standalone::InputMode::Mouse);
                shell.setSize(1280, 800); shell.showTab(standalone::DesktopShell::Tab::Mixer); settle(); // Edit: the parts list shows
                auto drawn = [&] { return shell.createComponentSnapshot(shell.getLocalBounds()); };
                auto same = [](const juce::Image& a, const juce::Image& b) {
                    for(int y = 0; y < a.getHeight(); ++y) for(int x = 0; x < a.getWidth(); ++x) if(a.getPixelAt(x, y) != b.getPixelAt(x, y)) return false;
                    return true;
                };
                for(const bool muted : {true, false, true, false})
                {
                    const auto before = drawn();
                    level(1, muted);
                    juce::MessageManager::getInstance()->runDispatchLoopUntil(5);
                    check(!same(before, drawn()), std::string("the parts list shows the part ") + (muted ? "muted" : "unmuted") + " within a frame");
                }
                settle();
            }
            // The Library reads the library again after a job that may change it, not after the
            // periodic refresh, which changes nothing.
            {
                standalone::Actions actions;
                tap.reset();
                standalone::LibraryPane pane(engine, actions, standalone::LibraryPane::Mode::Play);
                check(wait([&] { return tap.count("library.query") > 0 && !engine.jobRunning(); }), "the Library read once");
                holdRequests = true;
                standalone::SoundAction tick; tick.kind = standalone::SoundAction::Refresh;
                engine.soundAction(std::move(tick));
                check(wait([&] { return jobHeld.load(); }), "a refresh in flight");
                juce::MessageManager::getInstance()->runDispatchLoopUntil(400); // the pane looks at least once meanwhile
                holdRequests = false;
                check(wait([&] { return !engine.jobRunning() && !jobHeld.load(); }), "the refresh finished");
                tap.reset();
                juce::MessageManager::getInstance()->runDispatchLoopUntil(700);
                check(tap.count("library.query") == 0, "the Library is not read again after a refresh: " + std::to_string(tap.count("library.query")));
            }
            // Adding a repository at the engine (RFC 0031): the Settings page asks for https://
            // alone (checked in desktop-settings); the engine itself takes a folder too, for the
            // image's own and the tests' stand-ins. The web here is a stand-in transport serving a
            // folder, so nothing touches the network: a root whose index key does not verify fails
            // for that before its key's trust is asked about, so the page never offers it, nor a
            // name it did not sign; trusted anyway, it fails the same, and is taken back out, key
            // and all, when the page gives up.
            {
                const auto rootKey = std::string(64, 'a');
                const auto repo = data.getChildFile("repository-stand-in");
                repo.createDirectory();
                repo.getChildFile("repository.json").replaceWithText("{\"format\":\"oscairn.repository\",\"version\":1,\"repository\":\"org.example.test\",\"name\":\"Test repository\",\"serial\":1,\"rootKey\":\"" + rootKey +
                                                                     "\",\"indexKey\":{\"key\":\"" + std::string(64, 'b') + "\",\"signature\":\"" + std::string(128, 'c') + "\"}}");
                std::mutex fetchedMutex;
                std::vector<std::string> fetched;
                repository::setTransport([&](const std::string& url, uint64_t, const std::function<bool(int, const char*, size_t)>& sink) {
                    { std::lock_guard lock(fetchedMutex); fetched.push_back(url); }
                    const std::string base = "https://packages.example.test/";
                    const auto file = repo.getChildFile(url.rfind(base, 0) == 0 ? url.substr(base.size()) : "missing");
                    if(!file.existsAsFile()) return 404;
                    const auto bytes = file.loadFileAsString().toStdString();
                    sink(200, bytes.data(), bytes.size());
                    return 200;
                });
                auto listed = [&](const std::string& source) {
                    uint64_t job = 0;
                    engine.packages([&](Control& c) { job = c.listRepository(source); });
                    Control::PackageJob state;
                    wait([&] { engine.packages([&](Control& c) { state = c.packageJob(job); }); return state.state != "running"; });
                    return state;
                };
                tap.reset();
                auto state = listed("https://packages.example.test");
                check(state.state == "failed" && state.error.find("index key is not signed") != std::string::npos && state.rootKey == rootKey && state.name.empty() && tap.count("packages.list") == 1,
                      "an untrusted root that does not verify fails for that, with no name: " + state.error);
                { std::lock_guard lock(fetchedMutex); check(!fetched.empty() && fetched[0] == "https://packages.example.test/repository.json", "its root fetched from its address"); }
                engine.packages([&](Control& c) { c.addRepository({"https://packages.example.test", {}, rootKey, {}}); }); // trusted, as the page's Trust does
                state = listed("https://packages.example.test");
                check(state.state == "failed" && state.error.find("index key is not signed") != std::string::npos, "trusted, it is read again and fails the same: " + state.error);
                engine.packages([&](Control& c) { c.removeRepository("https://packages.example.test"); });
                std::vector<Control::TrustedKey> keys;
                std::vector<Control::Repository> known;
                engine.packages([&](Control& c) { keys = c.trustedKeys(); known = c.repositories(); });
                check(std::none_of(keys.begin(), keys.end(), [&](const auto& k) { return k.key == rootKey; }) &&
                          std::none_of(known.begin(), known.end(), [](const auto& r) { return r.address == "https://packages.example.test"; }),
                      "one that does not verify is not kept, nor its key trusted");
                // A folder's full path is read from the folder, not through the web.
                auto fetches = [&] { std::lock_guard lock(fetchedMutex); return fetched.size(); };
                const auto before = fetches();
                state = listed(repo.getFullPathName().toStdString());
                check(state.state == "failed" && state.error.find("index key is not signed") != std::string::npos && fetches() == before, "a folder's path is read from the folder");
                repository::setTransport({});
            }
            // The question when an install is refused for code not trusted here, by the three cases
            // the refusal tells apart: signed by the key the repository lists as the publisher's,
            // by a key no one vouches for, and unsigned. Only the last two offer Install anyway.
            {
                const std::string listed(64, 'a'), other(64, 'b'), listedId = signing::keyId(listed), otherId = signing::keyId(other);
                auto refused = [](std::vector<Control::Signer> code) { Control::PackageJob j; j.state = "failed"; j.name = "Oscairn"; j.installed.code = std::move(code); return j; };
                const Control::Signer byListed{"plugin", "Oscairn Arpeggiator", "0.1.4", "macos-arm64", "unknown", listedId, "", listed}, byOther{"plugin", "Arp", "1.0", "macos-arm64", "unknown", otherId, "", other},
                    bare{"plugin", "Bare", "2.0", "macos-arm64", "unsigned", "", "", ""}, trusted{"plugin", "Synth", "1.0", "macos-arm64", "trusted", otherId, "Mine", other};
                const auto vouched = standalone::codeQuestion(refused({byListed, trusted}), "Oscairn", listedId);
                check(vouched.title == "Trust Oscairn to install plugins?" && vouched.buttons == juce::StringArray{"Trust and install", "Cancel"} && vouched.trust == 1 && vouched.anyway == -1 &&
                          vouched.keys == std::vector<std::string>{listed} && vouched.message.contains("(Oscairn Arpeggiator 0.1.4) is signed by Oscairn") &&
                          vouched.message.contains("The repository Oscairn lists this key as Oscairn's") && vouched.message.contains("Key fingerprint: " + standalone::fingerprint(listedId)) &&
                          !vouched.message.contains(listed) && !vouched.message.contains("Synth 1.0"),
                      "signed by the key the repository lists: trust the publisher and install, by its fingerprint: " + vouched.message.toStdString());
                const auto stranger = standalone::codeQuestion(refused({byOther}), "Oscairn", listedId);
                check(stranger.title == "Install code from an unknown key?" && stranger.buttons == juce::StringArray{"Trust this key", "Install anyway", "Cancel"} && stranger.trust == 1 && stranger.anyway == 2 &&
                          stranger.keys == std::vector<std::string>{other} && stranger.message.contains("signed by key " + standalone::fingerprint(otherId) + ", which Oscairn does not list as Oscairn's") &&
                          stranger.message.contains("Anyone could have made it") && stranger.message.contains("runs as a program on this device"),
                      "signed by a key no one vouches for: trust that key, install anyway, or not, saying the risk: " + stranger.message.toStdString());
                check(standalone::codeQuestion(refused({byListed}), "Oscairn", {}).title == "Install code from an unknown key?", "with no publisher key listed, no key is vouched for");
                const auto unsignedCode = standalone::codeQuestion(refused({byListed, bare}), "Oscairn", listedId);
                check(unsignedCode.title == "Install unsigned code?" && unsignedCode.buttons == juce::StringArray{"Install anyway", "Cancel"} && unsignedCode.anyway == 1 && unsignedCode.trust == -1 &&
                          unsignedCode.message.startsWith("Bare 2.0 in this package is not signed: nothing says who made it") && unsignedCode.message.contains("runs as a program on this device"),
                      "unsigned: install anyway or not, saying the risk: " + unsignedCode.message.toStdString());
            }
            // Built in: a plugin package not installed whose plugins all come with the app says so,
            // with nothing to install, unless the repository has a newer version, offered as an update.
            {
                auto package = [](const std::string& id, const std::string& version) {
                    Control::RepositoryPackage p; p.id = p.name = id; p.publisher = "Oscairn";
                    p.versions.push_back({version, "stable", "", "", "", 0, 0, {{"clap", 1}}, {}});
                    return p;
                };
                Control::PackageJob listing; listing.name = "Oscairn";
                listing.packages = {package("org.oscairn.delay", "0.1.0"), package("org.oscairn.step-sequencer", "0.3.0"), package("org.oscairn.splitters", "0.1.0"), package("org.oscairn.fm", "0.4.8")};
                std::map<std::string, std::string> bundled{{"org.oscairn.delay", "0.1.0"}, {"org.oscairn.step-sequencer", "0.2.0"}, {"org.oscairn.split", "0.1.0"}, {"org.oscairn.join", "0.1.0"}};
                auto offer = [&](const std::vector<standalone::Offer>& all, const std::string& id) {
                    const auto at = std::find_if(all.begin(), all.end(), [&](const auto& o) { return o.package.id == id; });
                    return at == all.end() ? standalone::Offer{} : *at;
                };
                const auto all = standalone::offers({{"https://packages.oscairn.org", listing}}, {}, bundled);
                const auto delay = offer(all, "org.oscairn.delay"), sequencer = offer(all, "org.oscairn.step-sequencer"), splitters = offer(all, "org.oscairn.splitters"), fm = offer(all, "org.oscairn.fm");
                check(delay.builtIn && delay.installed == std::optional<std::string>("0.1.0") && !delay.update && !delay.offered() && standalone::describe(delay).startsWith("Built in  ") && standalone::describe(delay).endsWith("Oscairn"),
                      "a package that comes with the app, at the repository's version: Built in, nothing to install: " + standalone::describe(delay).toStdString());
                check(splitters.builtIn && !splitters.offered(), "one of several plugins, each with the app: Built in");
                check(sequencer.builtIn && sequencer.installed == std::optional<std::string>("0.2.0") && sequencer.update && sequencer.offered() && standalone::describe(sequencer).startsWith("Update to 0.3.0"),
                      "a newer version in the repository is offered as an update, never installed on its own: " + standalone::describe(sequencer).toStdString());
                check(!fm.builtIn && !fm.installed && fm.offered(), "one not with the app is offered to install");
                bundled.erase("org.oscairn.join");
                check(!offer(standalone::offers({{"https://packages.oscairn.org", listing}}, {}, bundled), "org.oscairn.splitters").builtIn, "not built in unless all its plugins come with the app");
                Control::InstalledPackage record; record.id = "org.oscairn.step-sequencer"; record.version = "0.3.0"; record.kind = "plugin";
                const auto installed = offer(standalone::offers({{"https://packages.oscairn.org", listing}}, {{record.id, record}}, bundled), "org.oscairn.step-sequencer");
                check(!installed.builtIn && installed.installed == std::optional<std::string>("0.3.0") && standalone::describe(installed).startsWith("Installed  "),
                      "installed from the repository, it is Installed, as any other: " + standalone::describe(installed).toStdString());
                // Withdrawn by a revocation list (RFC 0031): never the version offered; what is
                // installed and withdrawn says who withdrew it and why, offering the update, nothing more.
                auto withdrawnListing = listing;
                for(auto& p : withdrawnListing.packages)
                    if(p.id == "org.oscairn.fm") { p.versions.push_back({"0.4.9", "stable", "", "", "", 0, 0, {{"clap", 1}}, {}, {"Oscairn", "a test withdrawal", "2026-11-11"}}); }
                const auto newer = offer(standalone::offers({{"https://packages.oscairn.org", withdrawnListing}}, {}, bundled), "org.oscairn.fm");
                check(newer.version.version == "0.4.8" && newer.offered(), "a withdrawn version is never the one offered, a newer one included: " + newer.version.version);
                Control::InstalledPackage gone; gone.id = "org.oscairn.fm"; gone.version = "0.4.7"; gone.kind = "plugin"; gone.withdrawn = {"Oscairn", "a test withdrawal", "2026-11-11"};
                const auto stays = offer(standalone::offers({{"https://packages.oscairn.org", listing}}, {{gone.id, gone}}, bundled), "org.oscairn.fm");
                check(standalone::describe(stays) == "Withdrawn by Oscairn: a test withdrawal  " + juce::String::fromUTF8("·") + "  Update to 0.4.8" &&
                          standalone::withdrawnSaying(stays) == "org.oscairn.fm 0.4.7 was withdrawn by Oscairn: a test withdrawal" && stays.offered(),
                      "an installed version withdrawn since says so, by whom and why, offering the update: " + standalone::describe(stays).toStdString());
                // Content (asset libraries, RFC 0009): no plugin, Performance or Sound, on a tab of
                // its own, in every state a package has; one installed only because a package
                // needed it names that package.
                Control::RepositoryPackage carts; carts.id = "org.oscairn.fm-cartridges"; carts.name = "FM Cartridges"; carts.publisher = "Oscairn";
                carts.versions.push_back({"1.0.0", "stable", "", "", std::string(64, 'a'), 0, 0, {{"oscairn.asset", 1}}, {}});
                carts.versions.push_back({"1.1.0", "stable", "", "", std::string(64, 'c'), 0, 0, {{"oscairn.asset", 1}}, {}});
                auto fmPackage = package("org.oscairn.fm", "0.4.8"); fmPackage.name = "Oscairn FM"; fmPackage.versions[0].dependencies = {{carts.id, "1.0.0"}};
                Control::PackageJob withContent; withContent.name = "Oscairn"; withContent.packages = {fmPackage, carts};
                Control::InstalledPackage fmRecord; fmRecord.id = "org.oscairn.fm"; fmRecord.version = "0.4.8"; fmRecord.kind = "plugin";
                auto asset = [&](const std::string& version, char sha, bool dependency, bool fmToo, Control::Withdrawal withdrawn = {}) {
                    Control::InstalledPackage r; r.id = carts.id; r.version = version; r.kind = "asset"; r.sha256 = std::string(64, sha); r.dependency = dependency; r.withdrawn = withdrawn;
                    std::map<std::string, Control::InstalledPackage> installed{{r.id, r}};
                    if(fmToo) installed[fmRecord.id] = fmRecord;
                    return offer(standalone::offers({{"https://packages.oscairn.org", withContent}}, installed, bundled), carts.id);
                };
                const auto dot = juce::String::fromUTF8("  ·  ");
                const auto fresh = offer(standalone::offers({{"https://packages.oscairn.org", withContent}}, {}, bundled), carts.id);
                check(fresh.holds("asset") && !fresh.holds("plugin") && !fresh.holds("performance") && !fresh.holds("sound") && fresh.offered() && standalone::describe(fresh) == "Oscairn",
                      "an asset library is content, offered to install: " + standalone::describe(fresh).toStdString());
                check(standalone::describe(asset("1.1.0", 'c', false, false)) == "Installed" + dot + "Oscairn", "installed: " + standalone::describe(asset("1.1.0", 'c', false, false)).toStdString());
                check(standalone::describe(asset("1.0.0", 'a', false, false)) == "Update to 1.1.0" + dot + "Oscairn", "an update: " + standalone::describe(asset("1.0.0", 'a', false, false)).toStdString());
                check(standalone::describe(asset("1.1.0", 'e', false, false)) == "Installed (a different build)" + dot + "Oscairn",
                      "a different build: " + standalone::describe(asset("1.1.0", 'e', false, false)).toStdString());
                const auto pulled = asset("1.0.0", 'a', false, false, {"Oscairn", "a test withdrawal", "2026-11-12"});
                check(standalone::describe(pulled) == "Withdrawn by Oscairn: a test withdrawal" + dot + "Update to 1.1.0", "withdrawn: " + standalone::describe(pulled).toStdString());
                check(standalone::describe(asset("1.1.0", 'c', true, true)) == "Installed with Oscairn FM" + dot + "Oscairn",
                      "installed only as a dependency, it names the package needing it: " + standalone::describe(asset("1.1.0", 'c', true, true)).toStdString());
                check(standalone::describe(asset("1.1.0", 'c', true, false)) == "Installed with a package that needs it" + dot + "Oscairn",
                      "the package needing it not listed: " + standalone::describe(asset("1.1.0", 'c', true, false)).toStdString());
            }
            // Install... (RFC 0031): exactly the Catalog and a File. The Catalog lists the enabled
            // repositories' packages of a kind, grayed with the reason where this device cannot
            // install them; a package's details; Install, and for a publisher not trusted for
            // plugins, trusting it by its name and fingerprint, then trying again; what
            // is installed, marked so with its version, and Update to a newer version. The repository is a signed folder made by the index builder; the
            // official one is turned off and the web is a stand-in, so nothing touches the network.
            if(const char* maker = std::getenv("OSCAIRN_TEST_MAKE_REPOSITORY"))
            {
                std::vector<std::string> options;
                const auto menu = standalone::LibraryPane::installOptions();
                for(juce::PopupMenu::MenuItemIterator i(menu); i.next();) options.push_back(i.getItem().text.toStdString());
                check(options == std::vector<std::string>{"Catalog", "File"}, "Install... offers exactly Catalog and File");
                const auto tools = juce::StringArray::fromTokens(maker, "|", "");
                const auto made = data.getChildFile("catalog-repository");
                juce::ChildProcess process;
                const bool started = tools.size() == 5 && process.start(juce::StringArray{tools[0], "-I", tools[1], made.getFullPathName(), tools[2], tools[3], tools[4], data.getChildFile("Plain.olib").getFullPathName()});
                const auto said = started ? process.readAllProcessOutput() : juce::String("not started");
                check(started && process.getExitCode() == 0 && made.getChildFile("keys.json").existsAsFile(), "a signed repository made: " + said.toStdString());
                const auto made_keys = juce::JSON::parse(made.getChildFile("keys.json"));
                const auto rootKey = made_keys["root"].toString().toStdString(), publisherKey = made_keys["publisher"].toString().toStdString();
                const auto site = made.getChildFile("site").getFullPathName().toStdString();
                std::atomic<int> fetched = 0;
                repository::setTransport([&](const std::string&, uint64_t, const std::function<bool(int, const char*, size_t)>&) { ++fetched; return 404; });
                auto modalAlert = [] { return dynamic_cast<juce::AlertWindow*>(juce::ModalComponentManager::getInstance()->getModalComponent(0)); };
                auto answer = [&](juce::AlertWindow* w, int chosen) {
                    if(!w) return;
                    juce::Component::SafePointer<juce::Component> gone(w);
                    w->exitModalState(chosen);
                    wait([&] { return modalAlert() != gone.getComponent() || !gone; });
                };
                // The official repository turned off, and the test repository added by its folder
                // once its root key is trusted (what the Settings page's switches and Add do, at
                // the engine; the page itself is checked in desktop-settings).
                engine.packages([&](Control& c) { c.addRepository({site, "Test repository", rootKey, {}}); }); // its name, as the page has it from listing it
                std::vector<Control::Repository> known;
                check(wait([&] { engine.packages([&](Control& c) { known = c.repositories(); });
                               return known.size() == 2 && known[1].address == site && known[1].name == "Test repository" && known[1].origin == "user" && known[1].enabled; }),
                      "the test repository added as the user's, on, by its name");
                engine.packages([&](Control& c) { c.setRepositoryEnabled("https://packages.oscairn.org", false); });
                engine.packages([&](Control& c) { known = c.repositories(); });
                check(known.size() == 2 && !known[0].enabled && known[1].enabled, "and the official one turned off");
                standalone::Actions actions;
                standalone::LibraryPane pane(engine, actions, standalone::LibraryPane::Mode::Play);
                standalone::setDesktopMode(pane, 2); pane.setSize(1024, 600);
                wait([&] { return !engine.jobRunning(); });
                auto alertText = [](juce::AlertWindow* w) { juce::String out; for(auto* c : w->getChildren()) if(auto* l = dynamic_cast<juce::Label*>(c)) out << l->getText(); if(auto* t = w->getTextEditor({})) out << t->getText(); return out; };
                // Over the Library at 1024 x 600, centered as it shows.
                auto over = [&](juce::Component& c, const juce::String& name) {
                    if(argc < 5) return;
                    juce::Image screen = pane.createComponentSnapshot(pane.getLocalBounds());
                    const auto shown = c.createComponentSnapshot(c.getLocalBounds());
                    juce::Graphics(screen).drawImageAt(shown, (screen.getWidth() - shown.getWidth()) / 2, (screen.getHeight() - shown.getHeight()) / 2);
                    const juce::File out{juce::String(argv[4]) + "-" + name + ".png"};
                    out.deleteFile(); juce::FileOutputStream stream(out); juce::PNGImageFormat().writeImageToStream(screen, stream);
                };
                // The Library's empty state: no Sounds, and nothing narrowing the list, says so in
                // one line, with a button straight to the Catalog; a search that finds nothing does not.
                // Home's Library tile, and the empty Library, by what is installed (a starter
                // library of Performances counted as installed, and again when removed).
                const auto dot = juce::String::fromUTF8(" · ");
                check(standalone::libraryLine(0, 0, 0) == "0 Performances" && standalone::libraryLine(5, 0, 0) == "5 instruments and effects" + dot + "0 Performances" &&
                          standalone::libraryLine(1, 12, 3) == "1 instrument or effect" + dot + "12 Performances" + dot + "3 Sounds",
                      "the Library tile's line: " + standalone::libraryLine(5, 0, 0).toStdString());
                check(standalone::libraryEmpty(false, 0) == "No Performances yet." &&
                          standalone::libraryEmpty(false, 5) == "No Performances yet. 5 instruments and effects are installed, to start one with." &&
                          standalone::libraryEmpty(true, 5) == "No Sounds yet. The presets of the 5 instruments and effects installed are in the sound picker, not here.",
                      "the empty Library's lines: " + standalone::libraryEmpty(false, 5).toStdString());
                check(standalone::installedPluginPackages(engine) == 0, "no plugin package installed yet");
                pane.showKind("sound");
                check(wait([&] { return pane.empty.isVisible(); }) && pane.items().empty() && pane.empty.getText() == "No Sounds yet. An instrument's own presets are in the sound picker, not here." && pane.browse.isVisible() &&
                          !pane.empty.getBounds().isEmpty() && pane.empty.getBottom() <= pane.browse.getY(),
                      "an empty Library says so, over its list, with a way to the Catalog: " + pane.empty.getText().toStdString());
                settle();
                snapshot(pane, "library-empty");
                pane.search("tonewheel");
                check(wait([&] { return !pane.empty.isVisible(); }) && !pane.browse.isVisible(), "a search that finds nothing is not the empty state");
                pane.search("");
                check(wait([&] { return pane.empty.isVisible(); }), "cleared, it is again");
                pane.browse.triggerClick();
                // No repository on has a Sound package: it opens on the first tab with something, the rest still a click away.
                check(wait([&] { return pane.catalog() && !pane.catalog()->reading(); }) && pane.catalog()->performances.getToggleState() && !pane.catalog()->shown().empty() &&
                          pane.catalog()->sounds.isVisible(),
                      "the button opens the Catalog on the first tab with something, Performances, not an empty Sounds");
                if(auto* catalog = pane.catalog())
                {
                    catalog->sounds.triggerClick(); settle();
                    check(catalog->shown().empty() && catalog->emptySaying() == "No Sound packages in the repositories you have on. Look under Performances, Plugins or Content.",
                          "an empty tab says what it is, pointing to the tabs that have something: " + catalog->emptySaying().toStdString());
                    over(*catalog, "catalog-empty-tab");
                    catalog->search.setText("tonewheel organ", true); settle();
                    check(catalog->emptySaying() == "Nothing here matches the search.", "a search that finds nothing says so: " + catalog->emptySaying().toStdString());
                    catalog->search.setText("", true);
                }
                if(auto* dialog = pane.catalog() ? pane.catalog()->findParentComponentOfClass<juce::DialogWindow>() : nullptr) dialog->exitModalState(0);
                settle();
                pane.showKind("performance");
                {
                    // The app's plugins, found by the scanner with their versions as the repositories are read.
                    standalone::Catalog scanned(engine, "plugin", {}, argv[2]);
                    scanned.read();
                    const auto& found = scanned.bundled();
                    check(wait([&] { return !scanned.reading(); }) && found.count("org.oscairn.step-sequencer") && !found.at("org.oscairn.step-sequencer").empty() &&
                              found.count("org.oscairn.split") && scanned.shown().size() == 2,
                          "the plugins that come with the app are found, with their versions: " + std::to_string(found.size()));
                }
                pane.showCatalog("plugin");
                auto* catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }), "the Catalog opens and reads the repositories that are on");
                if(catalog)
                {
                    std::vector<std::string> names;
                    for(const auto& o : catalog->shown()) names.push_back(o.package.name + (o.installable ? "" : " (" + o.version.incompatible + ")"));
                    check(names.size() == 2 && names[0].rfind("Other Synth (it has no build for this device", 0) == 0 && names[1] == "Test Synth" && catalog->plugins.getToggleState(),
                          "under Plugins, each not installed; one for another device grayed with why: " + (names.empty() ? std::string() : names[0]));
                    check(catalog->status() == "From Test repository." && fetched == 0, "it says where they are from, and the official one, off, is not fetched: " + catalog->status().toStdString());
                    catalog->search.setText("tonewheel organ", true); settle();
                    check(catalog->shown().empty(), "searched like the rest: nothing matches");
                    catalog->search.setText("other", true); settle();
                    check(catalog->shown().size() == 1 && catalog->shown()[0].package.id == "org.kick.other", "a word of its name finds it");
                    catalog->search.setText("", true);
                    settle();
                    over(*catalog, "catalog-panel");
                    catalog->performances.triggerClick(); settle();
                    check(catalog->shown().size() == 1 && catalog->shown()[0].package.name == "Test Set" && catalog->shown()[0].installable, "under Performances, the Performance package");
                    catalog->plugins.triggerClick(); settle();
                    catalog->open(1);
                    const auto facts = catalog->facts();
                    std::map<juce::String, juce::String> fact(facts.begin(), facts.end());
                    check(catalog->opened() && fact["Publisher"] == "KICK, key " + standalone::fingerprint(signing::keyId(publisherKey)) && fact["License"] == "MIT" && fact["Version"] == "1.0.0" &&
                              fact["Summary"] == "A test synth" && fact["Repository"] == "Test repository" && fact["Needs"] == "Nothing else" && fact["Size"].contains("installed") && catalog->install.isEnabled(),
                          "its details: summary, version, license, publisher and key fingerprint, repository, size and what it needs: " + fact["Publisher"].toStdString());
                    settle();
                    over(*catalog, "catalog-details");
                    catalog->install.triggerClick();
                }
                // Code signed by the key the repository lists as its publisher's, not yet trusted
                // for plugins here: one question, to trust that publisher, by its fingerprint.
                const juce::String trustKick = "Trust KICK to install plugins?";
                check(wait([&] { auto* w = modalAlert(); return w && w->getName() == trustKick; }), "an install of code by a publisher not trusted here asks first");
                if(auto* w = modalAlert(); w && w->getName() == trustKick)
                {
                    const auto text = alertText(w);
                    check(w->getNumButtons() == 2 && w->getButton(0)->getButtonText() == "Trust and install" && w->getButton(1)->getButtonText() == "Cancel",
                          "it offers to trust the publisher and install, or not; no Install anyway");
                    check(text.contains("signed by KICK") && text.contains("The repository Test repository lists this key as KICK's") &&
                              text.contains("Key fingerprint: " + standalone::fingerprint(signing::keyId(publisherKey))) && !text.contains(publisherKey),
                          "naming the publisher, that the repository lists the key as theirs, and its fingerprint, not the full key: " + text.toStdString());
                    std::vector<Control::TrustedKey> keys;
                    engine.packages([&](Control& c) { keys = c.trustedKeys(); });
                    check(std::none_of(keys.begin(), keys.end(), [&](const auto& k) { return k.key == publisherKey; }), "nothing is trusted before the user says so");
                    over(*w, "catalog-trust-publisher");
                    w->getButton(0)->triggerClick();
                }
                check(wait([&] { return pane.message().contains("installed."); }), "trusted, it is installed: " + pane.message().toStdString());
                std::vector<Control::TrustedKey> keys;
                engine.packages([&](Control& c) { keys = c.trustedKeys(); });
                check(std::any_of(keys.begin(), keys.end(), [&](const auto& k) { return k.key == publisherKey && k.name == "KICK" && k.scope == std::vector<std::string>{"plugins"}; }),
                      "the publisher's key trusted for plugins, by its name");
                const auto plugins = engine.installedPlugins();
                check(std::any_of(plugins.begin(), plugins.end(), [](const auto& p) { return p.id == "org.kick.test.synth"; }), "its plugin in the store");
                // With a plugin package installed, the empty Library says what the user has.
                check(standalone::installedPluginPackages(engine) == 1, "one plugin package installed: " + std::to_string(standalone::installedPluginPackages(engine)));
                pane.showKind("sound");
                check(wait([&] { return pane.empty.isVisible() && pane.empty.getText() == "No Sounds yet. The presets of the 1 instrument or effect installed are in the sound picker, not here."; }),
                      "the empty Sounds name what is installed: " + pane.empty.getText().toStdString());
                settle();
                snapshot(pane, "library-empty-installed");
                pane.showKind("performance");
                // Installed, it is listed as installed with its version, and its details offer
                // nothing to install; the Desktop's Catalog, the same.
                auto synthRow = [&] {
                    const auto& shown = catalog->shown();
                    const auto at = std::find_if(shown.begin(), shown.end(), [](const auto& o) { return o.package.id == "org.kick.test.synth"; });
                    return at == shown.end() ? -1 : int(at - shown.begin());
                };
                auto closeCatalog = [&] { if(auto* dialog = catalog->findParentComponentOfClass<juce::DialogWindow>()) dialog->exitModalState(0); };
                pane.showCatalog("plugin");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && catalog->shown().size() == 2 && synthRow() >= 0, "what is installed is still listed");
                if(catalog && synthRow() >= 0)
                {
                    const auto& synth = catalog->shown()[size_t(synthRow())];
                    check(synth.installed == std::optional<std::string>("1.0.0") && !synth.update && !synth.offered(), "as installed, at its version, and not offered again");
                    catalog->open(synthRow());
                    const auto facts = catalog->facts();
                    std::map<juce::String, juce::String> fact(facts.begin(), facts.end());
                    check(fact["Installed"] == "1.0.0" && !catalog->install.isVisible(), "its details say what is installed, with no Install: " + fact["Installed"].toStdString());
                    catalog->back();
                    standalone::setDesktopMode(*catalog, 1); catalog->setSize(820, 560); settle();
                    snapshot(*catalog, "catalog-desktop");
                    closeCatalog();
                }
                // An older version installed: Update to the newer one installs it, and it is then current.
                // (The record says 0.9.0, as an older version's install would have written it.)
                {
                    const auto recorded = data.getChildFile("packages.json");
                    const auto was = recorded.loadFileAsString();
                    const auto older = was.replace("\"id\":\"org.kick.test.synth\",\"version\":\"1.0.0\",\"kind\"", "\"id\":\"org.kick.test.synth\",\"version\":\"0.9.0\",\"kind\"");
                    check(older != was && recorded.replaceWithText(older), "the install was recorded in packages.json: " + was.toStdString());
                }
                pane.showCatalog("plugin");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && synthRow() >= 0, "the Catalog lists it again");
                if(catalog && synthRow() >= 0)
                {
                    const auto& synth = catalog->shown()[size_t(synthRow())];
                    check(synth.installed == std::optional<std::string>("0.9.0") && synth.update && synth.offered() && synth.version.version == "1.0.0",
                          "an older one installed is offered as an update: " + synth.installed.value_or("none") + " -> " + synth.version.version);
                    standalone::setDesktopMode(*catalog, 1); catalog->setSize(820, 560); settle();
                    snapshot(*catalog, "catalog-update");
                    catalog->open(synthRow());
                    check(catalog->install.isVisible() && catalog->install.isEnabled() && catalog->install.getButtonText() == "Update to 1.0.0", "its details offer Update to the newer version: " + catalog->install.getButtonText().toStdString());
                    settle();
                    snapshot(*catalog, "catalog-update-details");
                    catalog->install.triggerClick();
                    check(wait([&] { return pane.message().startsWith("Installing"); }) && wait([&] { return pane.message().contains("installed."); }), "Update installs it: " + pane.message().toStdString());
                }
                pane.showCatalog("plugin");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && synthRow() >= 0 && catalog->shown()[size_t(synthRow())].installed == std::optional<std::string>("1.0.0") &&
                          !catalog->shown()[size_t(synthRow())].update,
                      "updated, it is installed and current");
                // The same version installed as another build (its archive is not the repository's,
                // by the SHA-256 recorded at install): never shown as Installed, offering the
                // repository's build, which replaces it only once the user says so, having seen both.
                std::string listedSha;
                if(catalog && synthRow() >= 0)
                {
                    const auto& synth = catalog->shown()[size_t(synthRow())];
                    listedSha = synth.version.sha256;
                    check(listedSha.size() == 64 && synth.record.sha256 == listedSha && !synth.otherBuild && standalone::describe(synth).startsWith("Installed  "),
                          "the repository's own build, by its archive's hash, is Installed: " + standalone::describe(synth).toStdString());
                    closeCatalog();
                }
                const auto recorded = data.getChildFile("packages.json");
                const auto was = recorded.loadFileAsString();
                const auto other = was.replace(juce::String(listedSha), juce::String(std::string(64, 'e')));
                check(!listedSha.empty() && other != was && recorded.replaceWithText(other), "an install recorded with another archive's hash: " + was.toStdString());
                const auto written = recorded.loadFileAsString(); // as written, its line ends JUCE's
                pane.showCatalog("plugin");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && synthRow() >= 0, "the Catalog lists it again");
                for(const bool confirm : {false, true})
                    if(catalog && synthRow() >= 0)
                    {
                        const auto& synth = catalog->shown()[size_t(synthRow())];
                        check(synth.otherBuild && synth.installed == std::optional<std::string>("1.0.0") && !synth.update && synth.offered() &&
                                  standalone::describe(synth).startsWith("Installed (a different build)"),
                              "another build at the listed version is not shown as Installed: " + standalone::describe(synth).toStdString());
                        if(!confirm)
                        {
                            standalone::setDesktopMode(*catalog, 1); catalog->setSize(820, 560); settle();
                            snapshot(*catalog, "catalog-other-build");
                        }
                        catalog->open(synthRow());
                        const auto facts = catalog->facts();
                        std::map<juce::String, juce::String> fact(facts.begin(), facts.end());
                        check(fact["Installed"] == "1.0.0, a different build: it does not match this repository's" && catalog->install.isVisible() && catalog->install.isEnabled() &&
                                  catalog->install.getButtonText() == "Install the repository's build",
                              "its details say so, offering the repository's build: " + fact["Installed"].toStdString() + " / " + catalog->install.getButtonText().toStdString());
                        if(!confirm) { settle(); snapshot(*catalog, "catalog-other-build-details"); }
                        catalog->install.triggerClick();
                        check(wait([&] { auto* w = modalAlert(); return w && w->getName() == "Install the repository's build?"; }), "it asks first");
                        if(auto* w = modalAlert(); w && w->getName() == "Install the repository's build?")
                        {
                            const auto said = alertText(w);
                            check(w->getNumButtons() == 2 && w->getButton(0)->getButtonText() == "Install the repository's build" && w->getButton(1)->getButtonText() == "Cancel" &&
                                      said.contains("Installed: archive " + juce::String(std::string(16, 'e'))) && said.contains("Test repository's: archive " + juce::String(listedSha.substr(0, 16))) &&
                                      said.contains(standalone::fingerprint(signing::keyId(publisherKey))),
                                  "naming both builds by their archives and signers: " + said.toStdString());
                            if(!confirm) over(*w, "catalog-other-build-confirm");
                            answer(w, confirm ? 1 : 0);
                        }
                        if(!confirm)
                        {
                            settle();
                            check(!pane.message().startsWith("Installing") && recorded.loadFileAsString() == written, "Cancel installs nothing: " + pane.message().toStdString());
                            pane.showCatalog("plugin");
                            catalog = pane.catalog();
                            check(catalog && wait([&] { return !catalog->reading(); }) && synthRow() >= 0, "the Catalog lists it again");
                        }
                    }
                check(wait([&] { return pane.message().startsWith("Installing"); }) && wait([&] { return pane.message().contains("installed."); }), "confirmed, the repository's build installs: " + pane.message().toStdString());
                std::vector<Control::InstalledPackage> packages;
                engine.packages([&](Control& c) { packages = c.installedPackages(); });
                check(std::any_of(packages.begin(), packages.end(), [&](const auto& p) { return p.id == "org.kick.test.synth" && p.version == "1.0.0" && p.sha256 == listedSha && !p.otherBuild; }),
                      "and is recorded as the repository's build");
                pane.showCatalog("plugin");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && synthRow() >= 0 && !catalog->shown()[size_t(synthRow())].otherBuild &&
                          !catalog->shown()[size_t(synthRow())].offered(),
                      "then it is Installed, with nothing to install");
                if(catalog) closeCatalog();
                // Content: the repository's asset library on a tab of its own, installed from there,
                // then Installed.
                pane.showCatalog("asset");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && catalog->content.getToggleState() && catalog->shown().size() == 1 &&
                          catalog->shown()[0].package.id == "org.kick.test.samples" && catalog->shown()[0].offered(),
                      "under Content, the asset library, offered: " + std::to_string(catalog ? catalog->shown().size() : 0));
                if(catalog && catalog->shown().size() == 1)
                {
                    standalone::setDesktopMode(*catalog, 1); catalog->setSize(820, 560); settle();
                    snapshot(*catalog, "catalog-content");
                    catalog->open(0);
                    catalog->install.triggerClick();
                    check(wait([&] { return pane.message().startsWith("Installing"); }) && wait([&] { return pane.message().contains("installed."); }), "it installs: " + pane.message().toStdString());
                }
                pane.showCatalog("asset");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && catalog->shown().size() == 1 && !catalog->shown()[0].offered() && !catalog->shown()[0].record.dependency &&
                          standalone::describe(catalog->shown()[0]).startsWith("Installed  "),
                      "then it is Installed, installed by hand: " + (catalog && !catalog->shown().empty() ? standalone::describe(catalog->shown()[0]).toStdString() : std::string()));
                if(catalog) closeCatalog();
                // A library of Performances installed (as the starter library is), then removed:
                // counted while it is there, and the count as it was once it is gone (this run's
                // library holds Performances already, so the empty line is checked on Sounds).
                pane.showKind("performance");
                settle();
                const auto before = pane.items().size();
                pane.showCatalog("performance");
                catalog = pane.catalog();
                check(catalog && wait([&] { return !catalog->reading(); }) && catalog->shown().size() == 1, "the Performance package listed");
                if(catalog && catalog->shown().size() == 1)
                {
                    catalog->open(0);
                    catalog->install.triggerClick();
                    check(wait([&] { return pane.message().contains("installed."); }), "it installs: " + pane.message().toStdString());
                }
                pane.refresh();
                check(wait([&] { return pane.items().size() == before + 1; }) && !pane.empty.isVisible(), "with a library of Performances, its Performance counted: " + std::to_string(pane.items().size()));
                check(engine.uninstallLibrary("org.kick.test.set").empty(), "the library removed");
                pane.refresh();
                check(wait([&] { return pane.items().size() == before; }), "removed, the count is as it was: " + std::to_string(pane.items().size()));
                // Every tab empty: no repository on, or none read (the official one's web here a
                // stand-in that finds nothing). Each says so, and where repositories are managed.
                engine.packages([&](Control& c) { c.setRepositoryEnabled(site, false); });
                {
                    standalone::Catalog none(engine, "performance", {});
                    none.read();
                    check(wait([&] { return !none.reading(); }) && none.shown().empty() &&
                              none.emptySaying() == "No package repository is on. Repositories are turned on and added in the Settings app, on its Package repositories page.",
                          "with no repository on, it says so, and where they are: " + none.emptySaying().toStdString());
                    standalone::setDesktopMode(none, 1); settle();
                    snapshot(none, "catalog-none-on");
                }
                engine.packages([&](Control& c) { c.setRepositoryEnabled("https://packages.oscairn.org", true); });
                {
                    standalone::Catalog offline(engine, "performance", {});
                    offline.read();
                    check(wait([&] { return !offline.reading(); }) && offline.shown().empty() && offline.status().startsWith("Not read: ") &&
                              offline.emptySaying() == "No repository could be read: check the connection. Repositories are turned on and added in the Settings app, on its Package repositories page.",
                          "with none read, it says so: " + offline.emptySaying().toStdString());
                    standalone::setDesktopMode(offline, 1); settle();
                    snapshot(offline, "catalog-offline");
                }
                // Removed (the page asks first; the engine's Remove takes its root key's trust
                // with it), and the official one turned back on for the rest of the run.
                engine.packages([&](Control& c) { c.removeRepository(site); c.setRepositoryEnabled("https://packages.oscairn.org", true); });
                std::vector<Control::TrustedKey> trusted;
                std::vector<Control::Repository> rest;
                engine.packages([&](Control& c) { trusted = c.trustedKeys(); rest = c.repositories(); });
                check(rest.size() == 1 && rest[0].origin == "official" && rest[0].enabled &&
                          std::none_of(trusted.begin(), trusted.end(), [&](const auto& k) { return k.key == rootKey; }),
                      "removed with its root key's trust, the official one on again");
                repository::setTransport({});
            }
            else std::printf("OSCAIRN_TEST_MAKE_REPOSITORY not set; the Catalog not checked\n");
            // A Parts view not on screen (the one under Play, a background app's) polls nothing:
            // on the Mixer, shown, it would read the meters twenty times a second.
            {
                standalone::PartsView view(engine, argv[2]);
                view.setSize(1024, 600); view.showPart(standalone::PartsView::mixerPage);
                check(!view.isShowing(), "the view is not on screen");
                tap.reset();
                for(int i = 0; i < 50; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10);
                check(tap.count("parts.meters") == 0, "a hidden Mixer reads no meters: " + std::to_string(tap.count("parts.meters")));
            }
            // An action asked while a job runs is refused, and said: a dropped Save must not
            // look saved.
            {
                // The Performance app's toolbar Save shows it would be refused, and why; every
                // Save also asks Engine::refusal before it sends, and alerts with its reason.
                standalone::DesktopShell shell(engine, argv[2], standalone::InputMode::Mouse);
                shell.onSave = [] {}; shell.setSize(1280, 800);
                juce::TextButton* saveButton = nullptr;
                for(auto* c : shell.getChildren()) if(auto* b = dynamic_cast<juce::TextButton*>(c); b && b->getButtonText() == "Save") saveButton = b;
                check(saveButton && wait([&] { return saveButton->isEnabled(); }), "the toolbar's Save is offered while the engine is free");
                wait([&] { return !engine.jobRunning(); }); // a refresh held too would time out: the job held is the one asked
                holdRequests = true;
                standalone::SoundAction prepare; prepare.kind = standalone::SoundAction::Prepare; prepare.directory = data.getChildFile("nothing").getFullPathName().toStdString();
                engine.soundAction(std::move(prepare));
                check(wait([&] { return engine.soundBusy() && !engine.controlReady() && jobHeld.load(); }), "a prepare of a missing folder holds the engine's thread");
                check(engine.refusal().find("busy") != std::string::npos, "a Save asked now is refused as busy: " + engine.refusal());
                standalone::SoundAction save; save.kind = standalone::SoundAction::SaveToLibrary;
                engine.soundAction(std::move(save));
                check(engine.soundMessage().find("busy") != std::string::npos, "a Save dropped while busy says so: " + engine.soundMessage());
                check(saveButton && wait([&] { return !saveButton->isEnabled() && saveButton->getTooltip().contains("busy"); }),
                      "the toolbar's Save shows it is refused, and why: " + (saveButton ? saveButton->getTooltip().toStdString() : std::string()));
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load() && engine.connected(); }), "the held job finished");
                check(saveButton && wait([&] { return saveButton->isEnabled(); }), "and Save is offered again");
            }
            // Exporting over a file the musician chose to replace: the old archive stays when the
            // export is refused, and goes only once the new one is written.
            {
                const auto file = data.getChildFile("Again.olib");
                file.replaceWithText("last week's");
                standalone::SoundAction out; out.kind = standalone::SoundAction::ExportPerformance; out.directory = file.getFullPathName().toStdString();
                wait([&] { return !engine.jobRunning(); }); // a refresh held too would time out: the job held is the one asked
                holdRequests = true;
                standalone::SoundAction prepare; prepare.kind = standalone::SoundAction::Prepare; prepare.directory = data.getChildFile("nothing").getFullPathName().toStdString();
                engine.soundAction(std::move(prepare));
                check(wait([&] { return engine.soundBusy() && !engine.controlReady() && jobHeld.load(); }), "a job holds the engine's thread");
                engine.soundAction(out);
                check(engine.soundMessage().find("busy") != std::string::npos && file.loadFileAsString() == "last week's", "an export refused while busy keeps the old file: " + engine.soundMessage());
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load() && engine.connected(); }), "the held job finished");
                engine.soundAction(out);
                Control::Archive archive;
                check(wait([&] { return !engine.soundBusy(); }) && engine.archiveContents(file.getFullPathName().toStdString(), archive).empty() && !archive.items.empty(),
                      "an export over the file replaces it once written: " + engine.soundMessage());
                juce::MemoryBlock written; file.loadFileAsData(written);
                out.signKey = "no such key"; // the engine's export fails
                engine.soundAction(out);
                check(wait([&] { return !engine.soundBusy(); }) && !engine.soundMessage().empty() && engine.soundMessage().find("exported") == std::string::npos && [&] { juce::MemoryBlock now; file.loadFileAsData(now); return now == written; }(),
                      "a failed export keeps the archive it would have replaced: " + engine.soundMessage());
                check(data.findChildFiles(juce::File::findDirectories, false, ".oscairn-export*").isEmpty(), "and leaves nothing beside it");
            }
            // Quitting right after an export over a file: written within the quit's wait, it
            // replaces the file rather than stay in the hidden folder beside it.
            {
                const auto file = data.getChildFile("Quit.olib");
                file.replaceWithText("last week's");
                {
                    standalone::Engine quitting(argv[1], tapPath);
                    wait([&] { return !quitting.jobRunning(); });
                    standalone::SoundAction out; out.kind = standalone::SoundAction::ExportPerformance; out.directory = file.getFullPathName().toStdString();
                    quitting.soundAction(std::move(out));
                    check(quitting.jobRunning(), "the export runs as the window quits");
                }
                Control::Archive archive;
                check(file.loadFileAsString() != "last week's" && wait([&] { return engine.controlReady(); }) && engine.archiveContents(file.getFullPathName().toStdString(), archive).empty() && !archive.items.empty(),
                      "an export done within the quit's wait replaces the file");
                check(data.findChildFiles(juce::File::findDirectories, false, ".oscairn-export*").isEmpty(), "and leaves nothing beside it on quit");
            }
            // A view rebuilt while a job runs asks whether each plugin has a window: one not yet
            // read answers no at once, rather than wait on the stalled socket.
            {
                wait([&] { return !engine.jobRunning(); }); // a refresh held too would time out: the job held is the one asked
                holdRequests = true;
                standalone::SoundAction prepare; prepare.kind = standalone::SoundAction::Prepare; prepare.directory = data.getChildFile("nothing").getFullPathName().toStdString();
                engine.soundAction(std::move(prepare));
                check(wait([&] { return engine.soundBusy() && !engine.controlReady() && jobHeld.load(); }), "a job holds the engine's thread");
                tap.reset();
                const auto began = std::chrono::steady_clock::now();
                const bool window = engine.hasPluginWindow(987654);
                const auto waited = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - began).count();
                check(!window && tap.count("window.info") == 0 && waited < 100 && engine.connected(), "a plugin window not yet read is not asked for while busy: " + std::to_string(waited) + " ms");
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load() && engine.connected(); }), "the held job finished");
            }
            // The library's edits, its archives and making a key wait up to 30 s on a call: while a
            // job holds the engine they are refused at once, saying it is busy.
            {
                wait([&] { return !engine.jobRunning(); }); // a refresh held too would time out: the job held is the one asked
                holdRequests = true;
                standalone::SoundAction prepare; prepare.kind = standalone::SoundAction::Prepare; prepare.directory = data.getChildFile("nothing").getFullPathName().toStdString();
                engine.soundAction(std::move(prepare));
                check(wait([&] { return engine.soundBusy() && !engine.controlReady() && jobHeld.load(); }), "a job holds the engine's thread");
                tap.reset();
                const auto began = std::chrono::steady_clock::now();
                std::string copied; Control::Archive archive; Control::SigningKey key;
                const std::vector<std::string> said{engine.editLibraryItem("perf-x", {}), engine.copyLibraryItem("perf-x", copied), engine.deleteLibraryItem("perf-x"),
                                                    engine.uninstallLibrary("lib-x"), engine.archiveContents("/nothing.olib", archive), engine.makeSigningKey("busy", key)};
                const auto waited = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - began).count();
                check(std::all_of(said.begin(), said.end(), [](const std::string& e) { return e.find("busy") != std::string::npos; }), "each is refused as busy: " + said[0]);
                int asked = 0;
                for(const char* m : {"library.edit", "library.copy", "library.delete", "library.uninstall", "library.contents", "keys.make"}) asked += tap.count(m);
                check(asked == 0 && waited < 100 && engine.connected(), "and none waits on the engine: " + std::to_string(asked) + " calls, " + std::to_string(waited) + " ms");
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load() && engine.connected(); }), "the held job finished");
            }
            // While a job holds the engine's thread it cannot answer a launch ask either: the
            // shell admits at once, rather than freeze for the call's timeout and drop.
            {
                wait([&] { return !engine.jobRunning(); }); // a refresh held too would time out: the job held is the one asked
                holdRequests = true;
                standalone::SoundAction prepare; prepare.kind = standalone::SoundAction::Prepare; prepare.directory = data.getChildFile("nothing").getFullPathName().toStdString();
                engine.soundAction(std::move(prepare));
                check(wait([&] { return engine.soundBusy() && !engine.controlReady() && jobHeld.load(); }), "a prepare of a missing folder holds the engine's thread");
                const auto began = std::chrono::steady_clock::now();
                const bool opened = standalone::askToLaunch(engine, {"oscairn.library", 120ull << 20}, "Library", {}, {});
                const auto waited = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - began).count();
                check(opened && engine.connected(), "an app opens at once while a job runs");
                check(waited < 900, "the ask did not wait on the stalled socket: " + std::to_string(waited) + " ms");
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load() && engine.connected(); }), "the held job finished");
                engine.appClosed("oscairn.library");
            }
            // An editor asks for every value of a stepped parameter on every rebuild: the texts
            // are cached per value, so they answer while the next refresh round trips.
            {
                NodeId synth = 0;
                for(const auto& [id, n] : engine.graph().names) if(n == "Basic Synth") synth = id;
                const auto node = engine.graph().names.at(synth);
                check(synth != 0, "the Lead's Basic Synth node");
                check(wait([&] { return !engine.soundBusy(); }), "the engine is free to read texts");
                // Asked without waiting: each shows once its reply has come.
                std::string saw, detuned, square;
                wait([&] { saw = engine.paramText(node, 4, 0); detuned = engine.paramText(node, 4, 1); square = engine.paramText(node, 4, 2); return saw == "Saw" && detuned == "Detuned saws" && square == "Square"; });
                check(saw == "Saw" && detuned == "Detuned saws" && square == "Square", "the oscillator names read, every value kept");
                wait([&] { return !engine.jobRunning(); }); // a refresh held too would time out: the job held is the one asked
                holdRequests = true;
                standalone::SoundAction prepare; prepare.kind = standalone::SoundAction::Prepare; prepare.directory = data.getChildFile("nothing").getFullPathName().toStdString();
                engine.soundAction(std::move(prepare));
                check(wait([&] { return engine.soundBusy() && !engine.controlReady() && jobHeld.load(); }), "a job runs while the editor rebuilds");
                check(engine.paramText(node, 4, 0) == saw && engine.paramText(node, 4, 1) == detuned && engine.paramText(node, 4, 2) == square,
                      "every value's text answers from the cache while the engine works");
                holdRequests = false;
                check(wait([&] { return !engine.soundBusy() && !jobHeld.load() && engine.connected(); }), "the job finished");
                // A continuous knob dragged asks for a new value's text on every move: a
                // parameter keeps a few hundred, then starts again, so the first is asked anew.
                // One out at a time, the latest value asked next: each value asked once its turn comes.
                tap.reset();
                for(int i = 1; i <= 300; ++i) { engine.paramText(node, 4, i / 1000.0); wait([&] { return tap.count("params.text") >= i; }); }
                settle();
                tap.reset();
                engine.paramText(node, 4, 0);
                check(wait([&] { return tap.count("params.text") >= 1; }) && tap.count("params.text") == 1, "a parameter's texts are capped, the oldest asked for again: " + std::to_string(tap.count("params.text")));
            }
            // A setlist marks entries "not in the library" once the mixed Sound+Performance read
            // of 1000 items passes them: entries are Performances, and the read asks for that
            // kind alone.
            {
                std::string lastId;
                engine.edit([&](Control& c) { lastId = c.savePerformanceToLibrary("Z last", true); });
                check(wait([&] { return !engine.soundBusy() && !lastId.empty(); }), "a Performance saved as the library's Z last");
                const auto lib = juce::File(::getenv("OSCAIRN_DATA_HOME"));
                const auto writeItem = [&](const std::string& kind, const std::string& folder, const std::string& id, const std::string& name) {
                    const auto at = lib.getChildFile(kind).getChildFile(folder);
                    at.createDirectory();
                    at.getChildFile(std::string(kind == "Sounds" ? "sound" : "performance") + ".json").replaceWithText(
                        "{\"format\":\"oscairn." + std::string(kind == "Sounds" ? "sound" : "performance") + "\",\"version\":" + std::string(kind == "Sounds" ? "8" : "13") +
                        ",\"info\":{\"id\":\"" + id + "\",\"name\":\"" + name + "\",\"description\":\"\",\"tags\":[],\"color\":\"\",\"revision\":\"r\",\"parent\":\"\",\"hash\":\"h\",\"origin\":null}}");
                };
                for(int i = 0; i < 999; ++i) writeItem("Sounds", "Sound " + std::to_string(i), "sound-" + std::to_string(i), "Sound " + std::to_string(i));
                writeItem("Performances", "Z dummy.oscairn", "perf-dummy", "Z dummy"); // sorted just before Z last: past the 1000-item mixed read
                engine.edit([](Control& c) { c.libraryRescan(); });
                check(wait([&] { const auto all = engine.library({}); return all && all->total >= 1003; }), "the library read holds 1001 staged items");
                standalone::Actions actions;
                standalone::SetlistPane pane(engine, actions, data.getChildFile("Setlists-staged"));
                const std::string setlistName = "Staged";
                standalone::Setlist staged; staged.name = setlistName; staged.entries = {{lastId, "Z last", {}}, {"perf-gone", "Gone", {}}};
                standalone::writeSetlist(data.getChildFile("Setlists-staged"), staged);
                pane.open(standalone::setlistFile(data.getChildFile("Setlists-staged"), setlistName));
                pane.go(0);
                check(pane.current() == 0 && !pane.message().contains("no longer in the library"), "the last Performance opens: " + pane.message().toStdString());
                check(wait([&] { return !engine.soundBusy(); }), "it finished opening");
                pane.go(1);
                check(pane.current() == 0 && pane.message().contains("no longer in the library"), "a gone entry is still marked: " + pane.message().toStdString());
                check(wait([&] { return !engine.soundBusy(); }), "the staged setlist is done opening");
            }
            // A reconnect to an engine still running (a timeout, another window) keeps its Capture
            // buffer: Capture's length goes again only to an engine without it, a fresh one.
            {
                auto reconnect = [&] {
                    tap.drop();
                    engine.edit([](Control& c) { c.history(); }); // the next call finds the connection gone
                    const bool dropped = wait([&] { return !engine.connected(); });
                    return dropped && wait([&] { return engine.connected(); });
                };
                auto retro = [&] { double seconds = -1; wait([&] { return !engine.jobRunning(); }); engine.edit([&](Control& c) { seconds = c.recordRetroSeconds(); }); return seconds; };
                check(wait([&] { return !engine.jobRunning(); }) && engine.setCapture(4).empty() && retro() == 4, "Capture keeps 4 seconds");
                tap.reset();
                check(reconnect(), "the client reconnects after a drop");
                check(tap.count("record.retro") == 0 && retro() == 4, "the running engine's buffer is kept, not started again: " + std::to_string(tap.count("record.retro")));
                engine.edit([](Control& c) { c.recordRetro(0); }); // as a fresh engine has it
                tap.reset();
                check(reconnect(), "the client reconnects again");
                check(tap.count("record.retro") == 1 && retro() == 4, "an engine without Capture gets its length again: " + std::to_string(retro()));
                check(engine.setCapture(0).empty(), "Capture off again");
            }
            // A crash ends the desktop's every control with it; Home and the Performance app now
            // show the Panel's notice (and Restart where a restart is possible), and a setlist
            // entry does not pretend to be playing while the engine is gone.
            {
                // Stall the server from this thread: the engine's next periodic refresh cannot
                // finish its handshake, and its timeout marks the engine disconnected.
                engine.edit([](Control& c) { c.addMacro("Drop probe"); }); // an undo name to read
                check(wait([&] { return !engine.meters().empty() && !engine.history().undo.empty() && engine.performanceInfo(); }), "the meters, undo names and Performance read before the drop: " + std::to_string(engine.meters().size()) + " " + engine.history().undo);
                forcedHold = true;
                check(wait([&] { return engine.jobRunning(); }), "the next refresh starts against the stalled server");
                check(wait([&] { return !engine.connected(); }), "its handshake times out; the engine reads as disconnected");
                standalone::SoundAction save; save.kind = standalone::SoundAction::SaveToLibrary;
                engine.soundAction(std::move(save));
                check(engine.refusal() == "The engine is not running." && engine.soundMessage() == engine.refusal(), "a Save while disconnected says the engine is not running: " + engine.soundMessage());
                check(engine.meters().empty() && engine.wireMeters().empty() && engine.history().undo.empty() && !engine.performanceInfo() && engine.takes().empty(),
                      "nothing read before the drop shows while disconnected");
                const auto folder = data.getChildFile("Setlists-staged");
                standalone::Actions actions;
                standalone::SetlistPane pane(engine, actions, folder);
                check(pane.setlist().entries.size() == 2 && pane.current() == -1, "the staged setlist is open with nothing playing");
                pane.go(0);
                check(pane.current() == -1 && pane.message().contains("not answering"), "a setlist entry says it did not open: " + pane.message().toStdString());
                pane.step(1);
                check(pane.current() == -1, "and stepping does not move on from an entry that never opened");
                standalone::Launcher launcher(engine, {}, {});
                standalone::setDesktopMode(launcher, 1); launcher.setSize(900, 600); launcher.resized();
                launcher.recovery().update();
                check(launcher.recovery().isVisible() && launcher.recovery().notice().contains("Reconnecting"), "Home shows the disconnect notice");
                check(!launcher.recovery().restartShown(), "no Restart while the engine may come back (this engine is not managed)");
                // A device's Panel shows the same notice.
                {
                    standalone::Panel panel(engine, standalone::Layout::parse(R"({"name": "Notice test", "size": [1024, 600], "controls": [{"type": "screen", "rect": [0, 0, 1024, 600]}]})"));
                    panel.setSize(1024, 600);
                    standalone::EngineRecovery* strip = nullptr;
                    check(wait([&] { for(auto* c : panel.getChildren()) if(auto* r = dynamic_cast<standalone::EngineRecovery*>(c)) strip = r; return strip && strip->isVisible(); }) && strip->notice().contains("Reconnecting"),
                          "a Panel shows the engine's notice with EngineRecovery");
                }
                // As after a managed engine's crash: Restart is a real button, in front of the
                // view's own controls at its place, on Home and in the Performance app.
                standalone::DesktopShell shell(engine, argv[2], standalone::InputMode::Mouse);
                shell.setSize(1280, 800); shell.setVisible(true); launcher.setVisible(true);
                for(auto* recovery : {&launcher.recovery(), &shell.recovery()})
                {
                    auto* view = recovery == &shell.recovery() ? static_cast<juce::Component*>(&shell) : &launcher;
                    recovery->show(true, true);
                    bool restarted = false;
                    recovery->onRestart = [&] { restarted = true; };
                    auto& button = recovery->restartButton();
                    auto* hit = view->getComponentAt(view->getLocalPoint(&button, button.getLocalBounds().getCentre()));
                    check(recovery->isVisible() && button.isVisible() && hit == &button,
                          "Restart is what a click at its place reaches: " + std::string(hit ? typeid(*hit).name() : "nothing"));
                    check(button.getWantsKeyboardFocus() && button.isAccessible(), "and it is reachable by keyboard and accessibility: " + std::to_string(button.getWantsKeyboardFocus()));
                    button.triggerClick();
                    check(wait([&] { return restarted; }), "a click on it restarts the engine");
                    recovery->onRestart = {};
                }
                forcedHold = false;
                for(int i = 0; i < 600 && !engine.connected(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); // the engine answers again
                check(engine.connected(), "the client reconnects");
                launcher.recovery().update();
                check(!launcher.recovery().isVisible(), "the notice goes when the engine is back");
            }
            juce::LookAndFeel::setDefaultLookAndFeel(nullptr);
            configuration.deleteRecursively();
        }
        running = false;
        control.join();
    }
    auto wait = [](auto done) { for(int i = 0; i < 1000 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
    // A managed engine restarted: Restart starts it off the message thread, which goes on
    // drawing (and says so) while the engine comes up. The engine here is a session served by
    // this test where the managed engine's socket is, in a runtime folder of the test's own;
    // the program started is a stand-in that only waits.
    {
        const std::string runtime = "/tmp/oscairn-gaps-run-" + std::to_string(::getpid());
        std::filesystem::remove_all(runtime);
        check(::mkdir(runtime.c_str(), 0700) == 0, "a private runtime folder");
        ::setenv("OSCAIRN_RUNTIME_DIR", runtime.c_str(), 1);
        const auto helper = runtime + "/stand-in";
        juce::File(helper).replaceWithText("#!/bin/sh\nsleep 3\n", false, false, "\n");
        ::chmod(helper.c_str(), 0700);
        const auto path = host::EngineProcess(helper, argv[1]).socketPath();
        auto served = std::make_unique<Served>(path);
        {
            standalone::Engine managed(argv[1], {}, helper);
            check(managed.connected(), "the managed engine is reached");
            served.reset(); // as the engine stops
            check(wait([&] { return managed.canRestart(); }), "a stopped managed engine offers Restart");
            standalone::EngineRecovery strip(managed);
            const auto began = std::chrono::steady_clock::now();
            strip.restartButton().onClick(); // as a click, at once
            const auto waited = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - began).count();
            check(waited < 1000 && strip.isVisible() && strip.notice().contains("Starting") && !strip.restartShown(),
                  "Restart does not hold the message thread, and says the engine is starting: " + std::to_string(waited) + " ms, " + strip.notice().toStdString());
            served = std::make_unique<Served>(path); // the engine comes up
            check(wait([&] { return managed.connected(); }), "the restarted engine is used once it answers");
            strip.update();
            check(!strip.isVisible(), "and the notice goes");
        }
        served.reset();
        std::filesystem::remove_all(runtime);
        ::unsetenv("OSCAIRN_RUNTIME_DIR");
    }
    // RFC 0019 step 5: the rich views a plugin describes (oscairn.ui draft 4), drawn from the
    // description and the values, and edited by dragging them, on the Panel's screen (1024 × 600,
    // touch) and on the Desktop. The test plugin describes an FM operator view with DX7's
    // routings, each operator's envelope and key scaling, and two elements' key and velocity map.
    if(const char* views = std::getenv("OSCAIRN_TEST_VIEWS"))
    {
        const auto path = socket.string() + "-views";
        NodeId node = 0;
        Served served(path, [&](Session& s) { node = s.add("Views", views); s.connectNotes(graphInput, node); s.connectAudio(node, graphOutput); s.commit(); });
        {
            standalone::Engine engine(argv[1], path);
            const auto configuration = juce::File::createTempFile("oscairn-gaps-config");
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            theme.apply("oscairn-dark");
            auto wait = [](auto done) { for(int i = 0; i < 1000 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            check(wait([&] { return engine.graph().names.count(node) && !engine.parameters(node).empty(); }), "the views plugin is in the graph");
            auto id = [&](const std::string& module, const std::string& name) { for(const auto& p : engine.parameters(node)) if(p.module == module && p.name == name) return p.id; return 0u; };
            auto value = [&](uint32_t param) { return engine.param("Views", param); };
            juce::Component holder; standalone::setDesktopMode(holder, 2); holder.setSize(1024, 600);
            standalone::ParameterGrid grid(engine, node, "Views");
            holder.addAndMakeVisible(grid); grid.setBounds(holder.getLocalBounds());
            auto page = [&](const std::string& name) { for(size_t i = 0; i < grid.pages(); ++i) if(grid.pageName(i) == name) { grid.showPage(i); return true; } return false; };
            auto find = [&](const juce::String& name) {
                juce::Component* found = nullptr;
                std::function<void(juce::Component&)> look = [&](juce::Component& c) { for(auto* child : c.getChildren()) { if(child->getName() == name && !found) found = child; look(*child); } };
                look(grid);
                return found;
            };
            auto settle = [] { for(int i = 0; i < 30; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); };
            auto save = [&](const juce::String& name) {
                settle();
                if(argc < 5) return;
                const juce::File out{juce::String(argv[4]) + "-" + name + ".png"};
                out.deleteFile(); juce::FileOutputStream stream(out); juce::PNGImageFormat().writeImageToStream(holder.createComponentSnapshot(holder.getLocalBounds()), stream);
            };
            auto drag = [](juce::Component* c, juce::Point<float> from, juce::Point<float> to) {
                auto mouse = juce::Desktop::getInstance().getMainMouseSource();
                const auto now = juce::Time::getCurrentTime();
                auto event = [&](juce::Point<float> at, bool dragged) { return juce::MouseEvent(mouse, at, juce::ModifierKeys::leftButtonModifier, 1, 0, 0, 0, 0, c, c, now, from, now, 1, dragged); };
                c->mouseDown(event(from, false)); if(to != from) c->mouseDrag(event(to, true)); c->mouseUp(event(to, to != from));
            };
            // A page shown once: its tiles show the plugin's value texts once they arrive, not the
            // values' numbers shown meanwhile ("-8.00", not "-8").
            {
                check(page("Amp"), "the ADSR's page");
                settle();
                bool written = false;
                std::function<void(juce::Component&)> look = [&](juce::Component& c) { for(auto* child : c.getChildren()) { if(auto* l = dynamic_cast<juce::Label*>(child); l && l->getText() == "-8.00") written = true; look(*child); } };
                look(grid);
                check(written, "a page shown once shows the plugin's value texts as they arrive");
            }
            // A stepped parameter's buttons and list: each step's own text once the texts arrive,
            // never the text that came last for another step ("-LIN | +LIN | +LIN | +LIN").
            {
                // One value's text already in, as after its tile showed it: the other steps' wait.
                for(const auto* name : {"Left Curve", "Coarse"})
                {
                    const auto param = id("Operator 1", name);
                    engine.paramText("Views", param, value(param));
                    check(wait([&] { return engine.textKnown("Views", param, value(param)); }), std::string("a text read for ") + name);
                }
                check(page("Operator 1"), "an operator's page");
                juce::StringArray buttons, list;
                auto read = [&] {
                    buttons.clear(); list.clear();
                    std::function<void(juce::Component&)> look = [&](juce::Component& c) {
                        for(auto* child : c.getChildren())
                        {
                            if(child->getName() == "Left Curve") buttons = standalone::ParameterGrid::choiceNames(*child);
                            if(auto* b = dynamic_cast<juce::ComboBox*>(child); b && b->getName() == "Coarse") for(int i = 0; i < b->getNumItems(); ++i) list.add(b->getItemText(i));
                            look(*child);
                        }
                    };
                    look(grid);
                    return buttons.joinIntoString(" | ") == "-LIN | -EXP | +EXP | +LIN" && list.size() == 32 && list[0] == "x0.50" && list[1] == "x1.00" && list[2] == "x2.00" && list[31] == "x31.00";
                };
                read();
                auto repeated = [](juce::StringArray names) { const int n = names.size(); names.removeDuplicates(false); return names.size() != n; };
                check(buttons.size() == 4 && !repeated(buttons) && list.size() == 32 && !repeated(list),
                      "before their texts arrive, no step shows another's: " + buttons.joinIntoString(" | ").toStdString() + "; " + list.joinIntoString(" | ").toStdString());
                check(wait(read), "a stepped parameter's buttons and list show each step's own text: " + buttons.joinIntoString(" | ").toStdString() + "; " + list.joinIntoString(" | ").toStdString());
            }
            // The descriptions, read without drawing: an ADSR, a DX7 envelope, an AWM2 envelope
            // from a hold, and DX7 algorithm 1's routing (2 over 1; 6 over 5 over 4 over 3).
            {
                const auto adsr = standalone::envelopeOf(std::vector<uint32_t>{OSCAIRN_FIELD_ATTACK, OSCAIRN_FIELD_DECAY, OSCAIRN_FIELD_SUSTAIN, OSCAIRN_FIELD_RELEASE});
                check(adsr.points.size() == 3 && adsr.points[0].time == 0 && adsr.points[0].fixed == 1 && adsr.points[1].level == 2 && adsr.points[1].sustain && adsr.points[2].time == 3, "an ADSR from its fields");
                std::vector<uint32_t> dx7;
                for(uint32_t k = 0; k < 4; ++k) dx7.push_back(OSCAIRN_FIELD_RATE_1 + k);
                for(uint32_t k = 1; k <= 4; ++k) dx7.push_back(OSCAIRN_FIELD_LEVEL_0 + k);
                const auto e = standalone::envelopeOf(dx7);
                check(e.points.size() == 4 && e.points[0].rate && e.start == 7 && e.points[3].level == 7 && e.points[2].sustain && !e.points[3].sustain, "a DX7 envelope: rates, from level 4, holding at level 3");
                const auto awm = standalone::envelopeOf(std::vector<uint32_t>{OSCAIRN_FIELD_TIME_1, OSCAIRN_FIELD_TIME_1 + 1, OSCAIRN_FIELD_TIME_1 + 2, OSCAIRN_FIELD_LEVEL_0, OSCAIRN_FIELD_LEVEL_0 + 2});
                check(awm.points.size() == 3 && awm.start == 3 && awm.points[0].level == 3 && awm.points[1].level == 4 && awm.points[1].sustain && awm.points[2].level < 0 && awm.points[2].fixed == 0,
                      "an AWM2 envelope: a hold at its initial level, the attack, a release to zero");
                const std::array<uint8_t, 11> one{2, 0, 8, 16, 32, 0, 0, 0, 1 | 4, 6, 6};
                const auto places = standalone::algorithmLayout(one, 6);
                check(places[0].row == 0 && places[2].row == 0 && places[1].row == 1 && places[3].row == 1 && places[4].row == 2 && places[5].row == 3 &&
                      places[1].x == places[0].x && places[5].x == places[2].x && places[2].x > places[0].x, "algorithm 1: two stacks, each modulator over what it modulates");
            }
            // Every parameter keeps its tile beside the views.
            size_t tiles = 0;
            for(size_t i = 0; i < grid.pages(); ++i) { grid.showPage(i); tiles += grid.tilesShown(); }
            check(tiles == engine.parameters(node).size(), "every parameter has its tile beside the views: " + std::to_string(tiles));
            // Each page with an envelope, as the plugin starts (for a real engine's, given its path).
            for(size_t i = 0; i < grid.pages(); ++i)
                if(grid.showPage(i), find("Envelope")) save("views-panel-envelope-" + juce::String(grid.pageName(i)).retainCharacters("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"));
            // Common: the operator view, its routing that of the algorithm the plugin gives.
            check(page("Common") && find("FM operators") != nullptr, "the FM operator view where its algorithm is");
            save("views-panel-fm");
            const auto algorithm = id("Common", "Algorithm");
            const double before = value(algorithm);
            if(auto* fm = find("FM operators"))
            {
                // A tap on the routing's right half steps the algorithm on; undo steps it back.
                const juce::Point<float> right(14 + std::min((fm->getWidth() - 28) * .36f, 360.f) * .8f, fm->getHeight() / 2.f);
                drag(fm, right, right);
                check(wait([&] { return value(algorithm) == before + 1; }), "a tap on the routing steps the algorithm on: " + std::to_string(value(algorithm)));
                engine.edit([](Control& c) { c.undo(); });
                check(wait([&] { return value(algorithm) == before; }), "and undo steps it back");
                engine.setParam("Views", algorithm, 1);
                save("views-panel-fm-algorithm-1");
                // Operator 2's level, dragged to the top of its bar.
                const auto level = id("Operator 2", "Level");
                const float routing = std::min((fm->getWidth() - 28) * .36f, 360.f), columns = (fm->getWidth() - 28) - routing - 14, x = 14 + routing + 14 + columns / 6 * 1.5f;
                drag(fm, {x, fm->getHeight() / 2.f}, {x, 0.f});
                check(wait([&] { return value(level) == 99; }), "dragging operator 2's level bar to its top sets its level: " + std::to_string(value(level)));
            }
            // An operator's page: its envelope (DX7 rates and levels) and its key scaling.
            check(page("Operator 1") && find("Envelope") != nullptr && find("Key map") != nullptr, "an operator's envelope and key scaling");
            save("views-panel-operator");
            if(auto* map = find("Key map"))
            {
                const auto breakpoint = id("Operator 1", "Break Point"), left = id("Operator 1", "Left Depth");
                const double was = value(breakpoint), depth = value(left);
                const float plane = map->getWidth() - 28.f, x = 14 + plane * float(was / 99), y = 28 + (map->getHeight() - 38 - 14) / 2.f;
                drag(map, {x, y}, {x + plane / 4, y});
                check(wait([&] { return value(breakpoint) > was + 15; }) && value(left) == depth, "dragging the breakpoint moves it, and only it: " + std::to_string(value(breakpoint)));
            }
            // An element's page: both elements' zones in one map, its own in the accent.
            check(page("Element 2") && find("Key map") != nullptr && find("Envelope") != nullptr, "an element's map and envelope");
            save("views-panel-element");
            if(auto* map = find("Key map"))
            {
                const auto low = id("Element 2", "Note Low"), high = id("Element 2", "Note High");
                const double was = value(low), top = value(high);
                const float plane = map->getWidth() - 28.f - std::min(90.f, (map->getWidth() - 28.f) / 4), x = 14 + plane * float(was / 127);
                const float height = map->getHeight() - 38.f - 14, y = 28 + height - height * float((value(id("Element 2", "Velocity Low")) + value(id("Element 2", "Velocity High"))) / 127 / 2); // the zone's middle
                drag(map, {x, y}, {x - plane / 4, y});
                check(wait([&] { return value(low) < was - 20; }) && value(high) == top, "dragging the zone's left edge lowers its lowest key, and only that: " + std::to_string(value(low)));
            }
            // Stages drawn by time, from the seconds the plugin gives: Amp's 4 ms attack narrower
            // than its 31 ms release, narrower than its 1 s decay, and still apart to grab.
            // Dragging the release point sets the release, and only it, where the scale puts the
            // point; undo puts it back.
            check(page("Amp") && find("Envelope") != nullptr, "the ADSR's envelope");
            save("views-panel-adsr");
            if(auto* curve = find("Envelope"))
            {
                const float hold = (curve->getWidth() - 28.f) * .14f;
                const auto at = standalone::ParameterGrid::envelopePoints(*curve);
                check(at.size() == 3, "an ADSR's three points");
                if(at.size() == 3)
                {
                    const float attack = at[0].x - 14, decay = at[1].x - at[0].x, release = at[2].x - at[1].x - hold;
                    check(attack >= 10 && attack < release && release < decay,
                          "a shorter stage draws narrower: attack " + std::to_string(attack) + ", release " + std::to_string(release) + ", decay " + std::to_string(decay));
                    const auto a = id("Amp", "Attack"), d = id("Amp", "Decay"), s = id("Amp", "Sustain"), r = id("Amp", "Release");
                    const double was = value(r), da = value(a), dd = value(d), ds = value(s);
                    const juce::Point<float> to(at[2].x + 40, at[2].y);
                    drag(curve, at[2], to);
                    check(wait([&] { return value(r) > was + .5; }) && value(a) == da && value(d) == dd && value(s) == ds,
                          "dragging the release point lengthens the release, and only that: " + std::to_string(value(r)));
                    const auto moved = standalone::ParameterGrid::envelopePoints(*curve);
                    check(moved.size() == 3 && std::abs(moved[2].x - to.x) < 2, "and the point lands where it was dropped: " + std::to_string(moved.size() == 3 ? moved[2].x : 0) + " for " + std::to_string(to.x));
                    engine.edit([](Control& c) { c.undo(); });
                    check(wait([&] { return value(r) == was; }), "and undo puts it back: " + std::to_string(value(r)));
                }
            }
            // A DX7 envelope's rates: 80, 60, 40 and 60, a higher rate a shorter, narrower segment.
            check(page("Operator 1") && find("Envelope") != nullptr, "operator 1's envelope");
            if(auto* curve = find("Envelope"))
            {
                const float hold = (curve->getWidth() - 28.f) * .14f;
                const auto at = standalone::ParameterGrid::envelopePoints(*curve);
                check(at.size() == 4, "a DX7 envelope's four points");
                if(at.size() == 4)
                {
                    const float one = at[0].x - 14, two = at[1].x - at[0].x, three = at[2].x - at[1].x, four = at[3].x - at[2].x - hold;
                    check(one < two && two < three && std::abs(four - two) < 1,
                          "a faster rate draws narrower: " + std::to_string(one) + ", " + std::to_string(two) + ", " + std::to_string(three) + ", " + std::to_string(four));
                    const auto rate = id("Operator 1", "Rate 1"), level = id("Operator 1", "Level 1"), rate2 = id("Operator 1", "Rate 2");
                    const double was = value(rate), l = value(level), r2 = value(rate2);
                    drag(curve, at[0], {at[0].x + 30, at[0].y});
                    check(wait([&] { return value(rate) < was; }) && value(level) == l && value(rate2) == r2,
                          "dragging the first point right slows rate 1, and only it: " + std::to_string(value(rate)));
                    engine.edit([](Control& c) { c.undo(); });
                    check(wait([&] { return value(rate) == was; }), "and undo puts it back: " + std::to_string(value(rate)));
                }
            }
            // The Desktop draws the same views.
            standalone::setDesktopMode(holder, 1); holder.setSize(1180, 820); grid.setBounds(holder.getLocalBounds().reduced(16));
            check(page("Common") && find("FM operators") != nullptr, "the Desktop draws the operator view too");
            save("views-desktop-fm");
            check(page("Operator 3") && find("Envelope") != nullptr && find("Key map") != nullptr, "and an operator's envelope and key scaling");
            save("views-desktop-operator");
        }
        std::filesystem::remove(path);
    }
    // The Basic Synth describes its envelope (an ADSR view): its Envelope page draws the curve on
    // the Panel's screen, and dragging a point sets that point's parameters, undone as any edit.
    {
        const auto path = socket.string() + "-envelope";
        NodeId node = 0;
        Served served(path, [&](Session& s) { node = s.add("Synth", (std::filesystem::path(argv[1]) / "oscairn-basic-synth.clap").string()); s.connectNotes(graphInput, node); s.connectAudio(node, graphOutput); s.commit(); });
        {
            standalone::Engine engine(argv[1], path);
            const auto configuration = juce::File::createTempFile("oscairn-gaps-config");
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            theme.apply("oscairn-dark");
            auto wait = [](auto done) { for(int i = 0; i < 1000 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            check(wait([&] { return engine.graph().names.count(node) && !engine.parameters(node).empty(); }), "the Basic Synth is in the graph");
            auto id = [&](const std::string& name) { for(const auto& p : engine.parameters(node)) if(p.name == name) return p.id; return 0u; };
            const auto attack = id("Attack"), decay = id("Decay"), sustain = id("Sustain"), release = id("Release");
            auto value = [&](uint32_t param) { return engine.param("Synth", param); };
            juce::Component holder; standalone::setDesktopMode(holder, 2); holder.setSize(1024, 600);
            standalone::ParameterGrid grid(engine, node, "Synth");
            holder.addAndMakeVisible(grid); grid.setBounds(holder.getLocalBounds());
            size_t page = grid.pages();
            for(size_t i = 0; i < grid.pages(); ++i) if(grid.pageName(i) == "Envelope") page = i;
            grid.showPage(page);
            juce::Component* curve = nullptr;
            std::function<void(juce::Component&)> look = [&](juce::Component& c) { for(auto* child : c.getChildren()) { if(child->getName() == "Envelope") curve = child; look(*child); } };
            look(grid);
            const auto names = grid.namesShown();
            check(page < grid.pages() && curve && names == std::vector<std::string>{"Attack", "Decay", "Sustain", "Release"} && grid.linkedShown() == 1,
                  "its Envelope page: the curve, and the four stages' tiles linked in order");
            for(int i = 0; i < 30; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10);
            grid.showPage(page); look(grid); // again, with the value texts the engine has sent since
            for(int i = 0; i < 10; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10);
            if(argc >= 5) { const juce::File out{juce::String(argv[4]) + "-basic-synth-panel-envelope.png"}; out.deleteFile(); juce::FileOutputStream stream(out); juce::PNGImageFormat().writeImageToStream(holder.createComponentSnapshot(holder.getLocalBounds()), stream); }
            if(curve)
            {
                auto drag = [&](juce::Point<float> from, juce::Point<float> to) {
                    auto mouse = juce::Desktop::getInstance().getMainMouseSource();
                    const auto now = juce::Time::getCurrentTime();
                    auto event = [&](juce::Point<float> at, bool dragged) { return juce::MouseEvent(mouse, at, juce::ModifierKeys::leftButtonModifier, 1, 0, 0, 0, 0, curve, curve, now, from, now, 1, dragged); };
                    curve->mouseDown(event(from, false)); curve->mouseDrag(event(to, true)); curve->mouseUp(event(to, true));
                };
                // The curve's geometry: three timed segments sharing what the sustain's width leaves,
                // each by its time (its value texts' seconds).
                const float width = curve->getWidth() - 28.f, segment = (width - width * .14f) / 3, top = 28, height = curve->getHeight() - 38.f;
                const double d = value(decay), s = value(sustain), r = value(release);
                auto points = standalone::ParameterGrid::envelopePoints(*curve);
                check(points.size() == 3 && points[0].x - 14 < points[1].x - points[0].x, "the 2 ms attack draws narrower than the 250 ms decay");
                // The attack point (at the top left), dragged right, near the end of its room.
                if(points.size() == 3) drag(points[0], {14 + segment - 2, top});
                check(wait([&] { return value(attack) > .8; }) && value(decay) == d && value(sustain) == s && value(release) == r,
                      "dragging the attack point lengthens the attack, and only that: " + std::to_string(value(attack)));
                // The decay point, where the attack ends and the decay reaches the sustain level, dragged down.
                points = standalone::ParameterGrid::envelopePoints(*curve);
                const float x = points.size() == 3 ? points[1].x : 0, y = top + height - height * float(s);
                drag({x, y}, {x, top + height * .9f});
                check(wait([&] { return value(sustain) < .2; }) && std::abs(value(decay) - d) < .02 && value(release) == r,
                      "dragging the decay point down lowers the sustain level: " + std::to_string(value(sustain)));
                engine.edit([](Control& c) { c.undo(); });
                check(wait([&] { return std::abs(value(sustain) - s) < 1e-9; }), "and undo puts it back: " + std::to_string(value(sustain)));
            }
        }
        std::filesystem::remove(path);
    }
    // RFC 0019: every built-in device profile's screen, and a Korg Kronos's (800 x 600 at 8"), at
    // its size and density, over the Panel of its layout: Home, Play, the Parts view, an editor
    // with rich views (each of its pages), the Library, the Catalog and Settings. The layout fills
    // the screen (no bars), no control falls outside it, none is smaller across than the minimum
    // touch target at that density, and no two overlap. Then the Desktop window at two sizes,
    // every tab. With SNAPSHOTS, each screen size's views go to SNAPSHOTS-screen-<size>-<view>.png.
    if(const char* views = std::getenv("OSCAIRN_TEST_VIEWS"))
    {
        const auto path = socket.string() + "-screens";
        NodeId node = 0;
        Served served(path, [&](Session& s) { node = s.add("Views", views); s.connectNotes(graphInput, node); s.connectAudio(node, graphOutput); s.commit(); });
        {
            standalone::Engine engine(argv[1], path);
            const auto configuration = juce::File::createTempFile("oscairn-screens-config");
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            theme.apply("oscairn-dark");
            const auto plugins = standalone::bundledPlugins(argv[1], argv[2]);
            engine.edit([&](Control& c) {
                for(const char* name : {"Piano", "Strings", "Bass", "Lead"})
                {
                    const auto part = standalone::addPart(c, name, false);
                    for(const auto& p : plugins) if(p.name == "Basic Synth") standalone::insertPlugin(c, part, 0, p);
                }
                c.addMacro("Brightness"); c.addMacro("Swell");
            });
            check(wait([&] { return engine.graph().parts.size() == 4 && !engine.parameters(node).empty(); }) && engine.editError().empty(), "parts and the views plugin to show: " + engine.editError());
            auto settle = [] { for(int i = 0; i < 40; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); };

            // The controls a finger or pointer uses in `top` (the view in front), in root's pixels:
            // none outside the area its parents show (a scrolling view's content excepted), none
            // smaller across than `minimum` pixels, no two overlapping where they show.
            auto audit = [&](juce::Component& root, juce::Component& top, float minimum, const std::string& where) {
                struct Hit { juce::Component* c; juce::Rectangle<float> shown, whole; };
                std::vector<Hit> hits; std::vector<std::string> problems;
                auto describe = [](juce::Component* c) {
                    juce::String text = c->getName();
                    if(auto* b = dynamic_cast<juce::Button*>(c); b && text.isEmpty()) text = b->getButtonText();
                    if(auto* b = dynamic_cast<juce::Button*>(c); b && text.isEmpty()) text = b->getTooltip();
                    return std::string(typeid(*c).name()) + " \"" + text.toStdString() + "\"";
                };
                std::function<void(juce::Component&, juce::Rectangle<float>, bool)> walk = [&](juce::Component& c, juce::Rectangle<float> clip, bool scrolled) {
                    if((&c != &top && !c.isVisible()) || dynamic_cast<juce::ScrollBar*>(&c)) return; // a root on no desktop is never visible
                    const auto r = root.getLocalArea(&c, c.getLocalBounds().toFloat());
                    if(dynamic_cast<juce::Button*>(&c) || dynamic_cast<juce::Slider*>(&c) || dynamic_cast<juce::ComboBox*>(&c) || dynamic_cast<juce::TextEditor*>(&c))
                    {
                        if(r.isEmpty()) // a layout that ran out of room gives what is left: nothing
                            problems.push_back(describe(&c) + " squeezed to " + r.toString().toStdString());
                        else if(!scrolled && !clip.expanded(1.f).contains(r))
                            problems.push_back(describe(&c) + " cut off at " + r.toString().toStdString() + " in " + clip.toString().toStdString());
                        if(const auto shown = r.getIntersection(clip); !shown.isEmpty()) hits.push_back({&c, shown, r});
                        return; // a control's children are its own parts
                    }
                    const auto inner = clip.getIntersection(r);
                    for(auto* child : c.getChildren()) walk(*child, inner, scrolled || dynamic_cast<juce::Viewport*>(&c));
                };
                walk(top, root.getLocalBounds().toFloat(), false);
                for(const auto& h : hits)
                    if(minimum > 0 && std::min(h.whole.getWidth(), h.whole.getHeight()) < minimum - .5f)
                        problems.push_back(describe(h.c) + " is " + h.whole.toString().toStdString() + ", under " + std::to_string(int(minimum)) + " px across");
                for(size_t i = 0; i < hits.size(); ++i)
                    for(size_t j = i + 1; j < hits.size(); ++j)
                        if(const auto both = hits[i].shown.getIntersection(hits[j].shown); both.getWidth() > 1 && both.getHeight() > 1 &&
                           !hits[i].c->isParentOf(hits[j].c) && !hits[j].c->isParentOf(hits[i].c))
                            problems.push_back(describe(hits[i].c) + " overlaps " + describe(hits[j].c) + " at " + both.toString().toStdString());
                std::string list; for(size_t i = 0; i < problems.size() && i < 40; ++i) list += "\n    " + problems[i];
                check(problems.empty(), where + ": " + std::to_string(problems.size()) + " problems, " + std::to_string(hits.size()) + " controls" + list);
                check(!hits.empty(), where + ": controls found");
                return hits.size();
            };
            auto save = [&](juce::Component& c, const std::string& name) {
                if(argc < 5) return;
                const juce::File out{juce::String(argv[4]) + "-screen-" + name + ".png"};
                out.deleteFile(); juce::FileOutputStream stream(out); juce::PNGImageFormat().writeImageToStream(c.createComponentSnapshot(c.getLocalBounds()), stream);
            };

            std::vector<std::pair<std::string, standalone::Profile::Screen>> screens;
            for(const auto& [id, json] : standalone::builtInProfiles())
                if(json.find("\"screens\"") != std::string::npos) // a device's, not a controller's
                    if(const auto p = standalone::loadProfile(id); p.screen() && p.screen()->surface == "panel") screens.push_back({id, *p.screen()});
            check(screens.size() == 6, "six built-in profiles have a panel screen");
            screens.push_back({"kronos", {800, 600, true, "panel", "kick-screen", 203}});
            std::set<std::string> saved;
            for(const auto& [id, screen] : screens)
            {
                const auto layout = standalone::Layout::parse(juce::File(argv[3]).getSiblingFile("layouts").getChildFile(screen.layout + ".json").loadFileAsString());
                const auto scale = screen.scale(), density = float(screen.density());
                // A screen of unknown size is taken as the KICK's density, which the UI is drawn for.
                const float minimum = float(standalone::Profile::Screen::minimumTouchMm) * (density > 0 ? density : 1024 / 151.f);
                const auto size = std::to_string(screen.width) + "x" + std::to_string(screen.height) + (scale > 1 ? "-dense" : "");
                const bool snap = saved.insert(size).second;
                const auto where = id + " (" + size + ")";
                standalone::Panel panel(engine, layout);
                panel.setDesignSize(juce::roundToInt(screen.width / scale), juce::roundToInt(screen.height / scale));
                panel.setSize(screen.width, screen.height);
                auto show = [&](juce::Component& view, const std::string& name, juce::Component* top = nullptr) {
                    panel.overlay(&view); view.resized(); settle();
                    const auto area = panel.getLocalArea(&view, view.getLocalBounds().toFloat());
                    check(area.getX() <= .5f && area.getY() <= .5f && area.getRight() >= screen.width - .5f && area.getBottom() >= screen.height - .5f,
                          where + " " + name + " fills the screen: " + area.toString().toStdString());
                    audit(panel, top ? *top : view, minimum, where + " " + name);
                    if(snap) save(panel, size + "-" + name);
                };
                standalone::TopBar::Shell shell{[] {}, [] {}, [] {}, [] {}, [](bool) {}, true};
                {
                    standalone::Launcher home(engine, {{"Performance", "piano", [] { return juce::String("Untitled: play or edit it"); }, [] {}},
                                                       {"Library", "library-big", [] { return juce::String("3 Performances, 12 Sounds"); }, [] {}},
                                                       {"Setlist", "list-music", [] { return juce::String("Songs in order, for the stage"); }, [] {}},
                                                       {"Groovebox", "layout-grid", [] { return juce::String("Patterns and steps"); }, [] {}}}, shell);
                    standalone::setDesktopMode(home, 2);
                    show(home, "home");
                    panel.overlay(nullptr);
                }
                {
                    standalone::PerformanceApp app(engine, argv[2]);
                    standalone::AppFrame frame(engine, "Performance", app, shell);
                    standalone::setDesktopMode(frame, 2); standalone::setDesktopMode(app, 2);
                    app.attach(frame.bar(), [] {});
                    show(frame, "play");
                    app.edit(true);
                    show(frame, "parts");
                    // A part's page, taller than a short screen: it scrolls.
                    app.edit(false); app.play().select(engine.graph().parts.front().id); app.edit(true);
                    show(frame, "part");
                    panel.overlay(nullptr);
                }
                {
                    standalone::ParameterGrid grid(engine, node, "Views");
                    standalone::AppFrame frame(engine, "Performance", grid, shell);
                    standalone::setDesktopMode(frame, 2); standalone::setDesktopMode(grid, 2);
                    for(size_t p = 0; p < grid.pages(); ++p)
                    {
                        grid.showPage(p);
                        auto name = "editor-" + grid.pageName(p);
                        std::replace(name.begin(), name.end(), ' ', '-');
                        show(frame, name);
                    }
                    panel.overlay(nullptr);
                }
                {
                    standalone::Actions actions;
                    standalone::LibraryPane library(engine, actions, standalone::LibraryPane::Mode::Play);
                    standalone::AppFrame frame(engine, "Library", library, shell);
                    standalone::setDesktopMode(frame, 2); standalone::setDesktopMode(library, 2);
                    show(frame, "library");
                    // The Catalog over it, as LibraryPane::showCatalog sizes it on a panel (its dialog's
                    // title bar aside); not read, so no repository is asked.
                    standalone::Catalog catalog(engine, "plugin", {});
                    standalone::setDesktopMode(catalog, 2);
                    const auto logical = frame.getLocalBounds();
                    catalog.setSize(std::min(984, std::max(600, logical.getWidth() - 40)), std::min(560, std::max(400, logical.getHeight() - 40)));
                    catalog.setCentrePosition(logical.getCentre());
                    frame.addAndMakeVisible(catalog);
                    show(frame, "catalog", &catalog); // a dialog of its own over the Library
                    frame.removeChildComponent(&catalog);
                    panel.overlay(nullptr);
                }
            }
            // The Desktop: any window size, laid out by the shell. Its density is the computer's,
            // unknown here, so no touch minimum is checked.
            for(const auto& [width, height, mode] : {std::tuple{1440, 900, standalone::InputMode::Mouse}, std::tuple{1024, 640, standalone::InputMode::Mouse}, std::tuple{1024, 640, standalone::InputMode::Touch}})
            {
                standalone::DesktopShell desktop(engine, argv[2], mode);
                desktop.setSize(width, height);
                const auto size = "desktop-" + std::to_string(width) + "x" + std::to_string(height) + (mode == standalone::InputMode::Touch ? "-touch" : "");
                for(const char* name : {"Play", "Part", "Mixer", "Routing", "Controls"})
                {
                    standalone::DesktopShell::Tab tab{};
                    standalone::DesktopShell::tabNamed(name, tab);
                    desktop.showTab(tab); settle();
                    audit(desktop, desktop, 0.f, size + " " + name);
                    save(desktop, size + "-" + juce::String(name).toLowerCase().toStdString());
                }
            }
        }
        std::filesystem::remove(path);
    }
    // A plugin's presets still being listed (the engine answers at once and lists them meanwhile):
    // the picker says so, and a later refresh shows the presets and drops the message.
    {
        const auto path = socket.string() + "-presets", tapped = path + "-tap";
        Served served(path, [&](Session& s) {
            const auto synth = s.add("synth", (std::filesystem::path(argv[1]) / "oscairn-basic-synth.clap").string());
            s.connectNotes(graphInput, synth); s.connectAudio(synth, graphOutput); s.commit();
        });
        SocketTap tap(tapped, path);
        tap.refuse("presets.list", "Presets are being listed; ask again");
        {
            standalone::Engine engine(argv[1], tapped);
            check(wait([&] { return engine.soundPicker().presetProblem.find("being listed") != std::string::npos && engine.soundMessage().find("being listed") != std::string::npos; }),
                  "the picker says the presets are being listed: " + engine.soundPicker().presetProblem + " / " + engine.soundMessage());
            check(engine.connected(), "and the engine is still connected");
            tap.refuse("presets.list", {});
            check(wait([&] { return engine.soundPicker().presetProblem.find("being listed") == std::string::npos && engine.soundMessage().find("being listed") == std::string::npos; }),
                  "a later refresh lists them and the message goes: " + engine.soundPicker().presetProblem + " / " + engine.soundMessage());
        }
        std::filesystem::remove(path);
    }
    std::filesystem::remove(socket);
    data.deleteRecursively();
    if(!failures) std::puts("ok: the desktop's controls for what the control API had first");
    return failures ? 1 : 0;
}
