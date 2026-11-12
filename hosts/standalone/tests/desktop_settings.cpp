// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The Settings app (RFC 0022's built-in apps): its pages shown by where they apply (a profile or
// OS without a page hides it, and a page not built is never shown); the frame's list and page on
// a wide and on a touch screen; the Audio page on an app connection granted `system` alone,
// against a socket session whose audio host is a stand-in for AudioEngine (no device is opened):
// the listing, a change applied, two refusals said under what was changed, a listing again when
// the status changes, and a fixed device; the Signing keys page on the same kind of connection:
// "No keys yet", a key made through the old dialog's ask and listed by name and fingerprint; the
// MIDI controllers page against the engine's own ControllerPorts on input names (no MIDI device is
// opened): each input with its profile menu and Ignore switch, None, a profile, each of two by
// channel, an input ignored, a refusal said under its port, a listing again when an input comes,
// and the choices kept in the data folder; the Package repositories page against the same session
// with the web a stand-in transport serving a signed folder, so nothing touches the network: the
// official repository listed with no Remove, a switch turning it off, a non-https address refused
// before the engine is asked, and an https repository asked to be trusted by its root key's
// fingerprint before it is added, listed with Remove, and removed again with its key's trust; the
// app's connection allowed audio.set and refused
// graph.add; and the app started as a process of its own from its manifest. With SNAPSHOTS, the
// app renders itself to SNAPSHOTS-settings-1400x900.png, -900x700.png, and on the KICK's
// 1024 x 600 screen under the shell's top bar, -kick-list.png and -kick-audio.png.
//   oscairn_desktop_settings_test PLUGIN_DIR APPS THEMES [SNAPSHOTS]
#include "../AppProcesses.h"
#include "../Launcher.h"
#include "../SigningKeys.h"
#include "../apps/settings/AudioPage.h"
#include "../apps/settings/MidiControllersPage.h"
#include "../apps/settings/RepositoriesPage.h"
#include "../apps/settings/SigningKeysPage.h"
#include "../../audio/ControllerPorts.h"
#include "EngineProcess.h"
#include "miniz/miniz.h"
#include "oscairn/assets.h"
#include "oscairn/local_socket.h"
#include "oscairn/repository.h"
#include "oscairn/session.h"
#include "oscairn/signing.h"
#include <juce_gui_extra/juce_gui_extra.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <fcntl.h>
#include <filesystem>
#include <mutex>
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
    auto ids = [](const std::vector<const standalone::settings::PageInfo*>& shown) { std::vector<std::string> out; for(const auto* p : shown) out.push_back(p->id); return out; };

    // Where pages apply: a page for any OS, one for Linux alone, one for a device's image, one not built.
    {
        using standalone::settings::PageInfo;
        auto make = [](standalone::Engine&) { return std::make_unique<standalone::settings::Page>(); };
        const std::vector<PageInfo> list{
            {"everywhere", "Everywhere", nullptr, make},
            {"linux", "Linux only", [](const standalone::Profile& p) { return p.platform.os == "linux"; }, make},
            {"device", "Device only", [](const standalone::Profile& p) { return !p.platform.runtime.empty(); }, make},
            {"unbuilt", "Not built", nullptr, nullptr}};
        auto computer = standalone::Profile::desktop();
        computer.platform.os = "macos";
        check(ids(standalone::settings::shownPages(list, computer)) == std::vector<std::string>{"everywhere"}, "a Mac shows the page for every OS, not Linux's, a device's or one not built");
        computer.platform.os = "linux";
        check(ids(standalone::settings::shownPages(list, computer)) == std::vector<std::string>{"everywhere", "linux"}, "a Linux computer shows Linux's too");
        check(ids(standalone::settings::shownPages(list, standalone::loadProfile("kick"))) == std::vector<std::string>{"everywhere", "linux", "device"}, "the KICK's image shows the device's page");
        // The real list's declarations: Network only on a device with a network, and nothing not built shown.
        const auto& all = standalone::settings::pages();
        const auto network = std::find_if(all.begin(), all.end(), [](const PageInfo& p) { return p.id == "network"; });
        check(network != all.end() && network->applies(standalone::loadProfile("kick")) && !network->applies(standalone::Profile::desktop()), "Network applies on the KICK (Wi-Fi), not on this computer");
        check(ids(standalone::settings::shownPages(all, standalone::Profile::desktop())) == std::vector<std::string>{"audio", "repositories", "keys", "controllers"}, "built so far: Audio, Package repositories, Signing keys and MIDI controllers");
        check(ids(standalone::settings::shownPages(all, standalone::loadProfile("kick"))) == std::vector<std::string>{"audio", "repositories", "keys", "controllers"}, "all of them on a device too");
#if !OSCAIRN_BLUETOOTH
        check(std::none_of(all.begin(), all.end(), [](const PageInfo& p) { return p.id == "bluetooth"; }), "Bluetooth is left out of a build without it");
#endif
    }

    const auto socket = std::filesystem::temp_directory_path() / ("oscairn-settings-" + std::to_string(::getpid()) + ".sock");
    const auto data = juce::File::createTempFile("oscairn-settings-data"), configuration = juce::File::createTempFile("oscairn-settings-config");
    configuration.createDirectory();
    ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
    ::setenv("OSCAIRN_CONFIG_HOME", configuration.getFullPathName().toRawUTF8(), 1);
    const std::string themes = "OSCAIRN_THEMES=" + std::string(argv[3]);
    {
        auto session = std::make_unique<Session>(48000, 128);
        // The audio host's stand-in: devices, the choice and the rules audio.set keeps, on the
        // control thread (the engine's), and read here too: under the lock.
        std::mutex lock;
        Control::AudioDevices devices{{{"Studio", {44100, 48000, 96000}, {64, 128, 256}}, {"Headphones", {44100, 48000}, {128, 256, 512}}, {"Broken", {48000}, {128}}},
                                      {}, "Studio", "", 48000, 128, "default", "", false};
        uint64_t deviceList = 1;
        std::atomic<bool> statusChanged = true;
        session->setAudioHost([&] { std::lock_guard<std::mutex> l(lock); return devices; },
            [&](const Control::AudioSetting& s) {
                std::lock_guard<std::mutex> l(lock);
                const auto output = s.output.value_or(devices.output);
                const auto d = std::find_if(devices.outputs.begin(), devices.outputs.end(), [&](const auto& o) { return o.name == output; });
                if(d == devices.outputs.end()) throw std::invalid_argument(output + " is not an output device");
                if(s.sampleRate && std::find(d->sampleRates.begin(), d->sampleRates.end(), *s.sampleRate) == d->sampleRates.end())
                    throw std::invalid_argument(std::to_string(int(*s.sampleRate)) + " Hz is not a sample rate " + output + " offers");
                if(output == "Broken") throw std::runtime_error("Broken did not open: the stand-in refuses it; " + devices.output + " plays again, as it was");
                devices.output = output; devices.source = "user";
                if(s.sampleRate) devices.sampleRate = *s.sampleRate;
                if(s.bufferSize) devices.bufferSize = *s.bufferSize;
                statusChanged = true;
                return devices;
            });
        // The engine's controllers, on input names alone: the built-in profiles and a second
        // SMK37 profile (custom firmware) from a file, wired as AudioEngine wires them.
        std::string stock;
        for(const auto& [id, json] : standalone::builtInProfiles()) if(id == "smk37-elite") stock = json;
        const auto custom = configuration.getChildFile("smk37-custom.json");
        custom.replaceWithText(juce::String(stock).replace("\"smk37-elite\"", "\"smk37-elite-custom\"").replace("(stock firmware)", "(custom firmware)"));
        host::ControllerPorts ports({custom.getFullPathName().toStdString()});
        ports.setInputs({"IAC Bus 1", "SMK-37 Elite -Master"});
        uint64_t portList = 0;
        session->setControllerHost({[&] { return ports.list(); },
            [&](const std::string& input, const std::optional<std::vector<std::string>>& profiles) { return ports.choose(input, profiles); },
            [&](const std::string& input, bool ignored) { return ports.ignore(input, ignored); }});
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true;
        std::thread control([&] {
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                if(ports.changes() != portList) { portList = ports.changes(); statusChanged = true; }
                if(statusChanged.exchange(false)) // as AudioEngine says it: the device, its rate and size, the lists' counters
                {
                    std::lock_guard<std::mutex> l(lock);
                    auto status = session->sessionStatus();
                    status.audioDevice = devices.output; status.sampleRate = devices.sampleRate; status.bufferSize = devices.bufferSize; status.audioDeviceList = deviceList; status.midiPortList = portList; status.audioReady = true;
                    session->setHostStatus(status);
                }
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset();
        });
        auto change = [&](auto edit) { { std::lock_guard<std::mutex> l(lock); edit(); ++deviceList; } statusChanged = true; };
        {
            standalone::Engine engine(argv[1], socket.string());
            standalone::Theme theme(configuration, juce::File(argv[3]));
            juce::LookAndFeel::setDefaultLookAndFeel(&theme);
            auto wait = [&](auto done, int ms = 10000) { for(int i = 0; i < ms / 10 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
            const auto apps = standalone::findApps(juce::File(argv[2]));
            const auto manifest = std::find_if(apps.begin(), apps.end(), [](const auto& a) { return a.id == "oscairn.settings"; });
            check(manifest != apps.end() && manifest->executable.existsAsFile() && manifest->permissions == std::vector<std::string>{"system"}, "the Settings manifest is found, with its build for this platform and `system` alone");
            if(manifest != apps.end()) [&] { // the rest needs it; a return leaves the rest out
            // Its connection, as the shell asks for it: audio.set allowed, graph.add refused naming the permission.
            {
                LocalClient app;
                app.adopt(engine.appConnection("oscairn.settings", manifest->permissions));
                const auto set = juce::JSON::parse(juce::String(app.call("audio.set", R"({"bufferSize":256})")));
                check(set["result"]["bufferSize"].toString() == "256", "the app's connection may call audio.set");
                const auto add = juce::JSON::parse(juce::String(app.call("graph.add", R"({"name":"x","path":"/nowhere.clap"})")));
                check(int(add["error"]["code"]) == -32008 && add["error"]["message"].toString().contains("edit"), "and is refused graph.add, naming edit: " + add["error"]["message"].toString().toStdString());
                const auto subscribe = juce::JSON::parse(juce::String(app.call("subscribe", R"({"topics":["graph.changed"]})")));
                check(int(subscribe["error"]["code"]) == -32008, "and the graph's topic");
                std::lock_guard<std::mutex> l(lock); devices.bufferSize = 128; devices.source = "default"; // as before it
            }
            statusChanged = true;

            // The Audio page in the frame, on an app's connection granted `system` alone, as the app has it.
            {
                standalone::Engine app(engine.appConnection("oscairn.settings", manifest->permissions), false);
                standalone::settings::Frame frame(app, standalone::Profile::desktop());
                standalone::setDesktopMode(frame, 1);
                frame.setSize(1000, 640);
                auto* page = dynamic_cast<standalone::settings::AudioPage*>(frame.page());
                check(page && frame.current()->id == "audio" && frame.shown().size() == 4, "wide, with a mouse: the list at the left and the first page, Audio, beside it");
                if(!page) return;
                check(wait([&] { return page->listings() >= 1 && !page->busy(); }), "the devices are listed");
                check(page->output.getNumItems() == 3 && page->output.getText() == "Studio" && page->output.isEnabled(), "the output devices, the one open chosen");
                check(page->rate.getNumItems() == 3 && page->rate.getText() == "48000 Hz" && page->size.getText().startsWith("128 samples"), "its rates and sizes: " + page->rate.getText().toStdString() + ", " + page->size.getText().toStdString());
                check(page->outputRow().explanation() == "The system's default device." && !page->outputRow().danger(), "where it came from, in plain words: " + page->outputRow().explanation().toStdString());
                check(!page->inputRow().isVisible(), "no input row where the engine opens no inputs");

                page->rate.setSelectedId(3, juce::sendNotificationSync); // 96000 Hz
                check(page->busy() && !page->rate.isEnabled(), "a change disables the choices while it is applied");
                check(wait([&] { return !page->busy() && page->devices().sampleRate == 96000; }), "applied with audio.set");
                check(page->rate.getText() == "96000 Hz" && page->outputRow().explanation().startsWith("Chosen here"), "shown as the user's choice");
                check(wait([&] { return app.sessionStatus().sampleRate == 96000; }), "and the status says so");
                // The status catching up lists again; a change is made once that is done (the choices are disabled meanwhile).
                auto settle = [&] { wait([&] { return !page->busy(); }); wait([] { return false; }, 400); wait([&] { return !page->busy(); }); };

                settle();
                page->output.setSelectedId(3, juce::sendNotificationSync); // Broken
                check(wait([&] { return !page->busy() && page->outputRow().danger(); }) && page->outputRow().explanation().contains("Broken did not open"), "a device that does not open: said under it: " + page->outputRow().explanation().toStdString());
                check(page->output.getText() == "Studio", "and the one before shown again");
                settle();
                Control::AudioSetting odd; odd.sampleRate = 12345;
                page->apply(odd, const_cast<standalone::settings::Row&>(page->rateRow()));
                check(wait([&] { return !page->busy() && page->rateRow().danger(); }) && page->rateRow().explanation().contains("12345 Hz is not a sample rate"), "a rate refused: said under it: " + page->rateRow().explanation().toStdString());

                // Changed elsewhere: devices came (an input) and the device notes one passed over; the status's counter brings a listing.
                settle();
                const int before = page->listings();
                change([&] { devices.inputs = {{"Studio In", {48000}, {128}}, {"Line In", {48000}, {128}}}; devices.input = "Studio In"; devices.notice = "Headphones went away; Studio plays"; });
                check(wait([&] { return page->listings() > before && !page->busy(); }), "a status change lists the devices again");
                check(page->inputRow().isVisible() && page->input.getText() == "Studio In", "the input device, now offered");
                check(page->outputRow().danger() && page->outputRow().explanation().contains("Headphones went away"), "the notice, said: " + page->outputRow().explanation().toStdString());

                // Fixed by the profile: the only device, its rate the only rate; neither can be chosen.
                change([&] { devices.outputs = {{"Studio", {48000}, {128, 256}}}; devices.fixed = true; devices.source = "profile"; devices.notice.clear(); devices.sampleRate = 48000; });
                check(wait([&] { return !page->busy() && page->devices().fixed; }), "a fixed device is listed");
                check(!page->output.isEnabled() && page->outputRow().explanation().startsWith("Fixed by the device profile") && !page->rate.isEnabled() && page->size.isEnabled(),
                      "shown fixed by the profile and not choosable; the buffer size still is: " + page->outputRow().explanation().toStdString());
                // --audio null: fixed too, and said so plainly.
                change([&] { devices.outputs = {{"null", {48000}, {128}}}; devices.output = "null"; devices.source = "null"; });
                check(wait([&] { return !page->busy() && page->devices().source == "null"; }), "the null device is listed");
                check(!page->output.isEnabled() && page->outputRow().explanation() == "No audio device: started with --audio null.",
                      "the null device, said plainly: " + page->outputRow().explanation().toStdString());
                change([&] { devices.outputs = {{"Studio", {48000}, {128, 256}}}; devices.output = "Studio"; devices.source = "profile"; });
                check(wait([&] { return !page->busy() && page->devices().output == "Studio"; }), "the profile's device again");

                // Narrow, or with touch: the list, then a page in its place with Back.
                standalone::setDesktopMode(frame, 2);
                frame.back();
                frame.setSize(600, 900);
                check(frame.listing() && frame.page() == nullptr, "with touch, the list alone");
                check(frame.open("audio") && frame.page() && !frame.listing(), "a page in its place");
                frame.back();
                check(frame.listing(), "Back: the list again");
                check(!frame.open("network"), "a page not shown here is not opened");
            }

            // The Signing keys page on an app's connection granted `system` alone: keys.list and
            // keys.make reach the session's Keys/ folder under OSCAIRN_DATA_HOME, never the user's.
            {
                standalone::Engine app(engine.appConnection("oscairn.settings", manifest->permissions), false);
                standalone::settings::Frame frame(app, standalone::Profile::desktop());
                standalone::setDesktopMode(frame, 1);
                frame.setSize(1000, 640);
                check(frame.open("keys"), "the list opens Signing keys");
                auto* keys = dynamic_cast<standalone::settings::SigningKeysPage*>(frame.page());
                if(!keys) return;
                check(wait([&] { return keys->listings() >= 1 && !keys->busy(); }), "the keys are listed when the page opens");
                check(keys->keys().empty() && keys->noKeys.isVisible() && keys->noKeys.getText() == "No keys yet" && !keys->makeRow().danger(), "with none: No keys yet");

                // Make: the old dialog's ask, answered once it is up; the trigger blocks in its
                // modal loop, so the answer is queued and typed from inside it.
                auto alert = [] { return dynamic_cast<juce::AlertWindow*>(juce::ModalComponentManager::getInstance()->getModalComponent(0)); };
                const int before = keys->listings();
                std::atomic<bool> answered = false;
                std::function<void()> answer;
                answer = [&] {
                    if(answered) return;
                    if(auto* w = alert(); w && w->getName() == "Make a signing key")
                    { w->getTextEditor("name")->setText("stage"); w->exitModalState(1); answered = true; return; }
                    juce::Timer::callAfterDelay(20, answer);
                };
                juce::Timer::callAfterDelay(20, answer);
                keys->makeKey.triggerClick();
                check(wait([&] { return answered.load(); }), "Make asks for the key's name");
                check(wait([&] { return keys->listings() > before && !keys->busy(); }), "and lists the keys again once it is made");
                check(keys->keys().size() == 1 && keys->keys()[0].name == "stage" && keys->keys()[0].key.size() == 64 && !keys->noKeys.isVisible(), "the key made is listed by name, never its private half");
                check(keys->keyRow(0).label() == "stage" && keys->prints[0]->getText() == standalone::fingerprint(keys->keys()[0].id),
                      "and by fingerprint, in groups of four: " + keys->prints[0]->getText().toStdString());
            }

            // The MIDI controllers page on an app's connection granted `system` alone: the engine's
            // choices, kept in the data folder under OSCAIRN_DATA_HOME, never the user's.
            {
                standalone::Engine app(engine.appConnection("oscairn.settings", manifest->permissions), false);
                standalone::settings::Frame frame(app, standalone::Profile::desktop());
                standalone::setDesktopMode(frame, 1);
                frame.setSize(1000, 640);
                check(frame.open("controllers"), "the list opens MIDI controllers");
                auto* page = dynamic_cast<standalone::settings::MidiControllersPage*>(frame.page());
                if(!page) return;
                check(wait([&] { return page->listings() >= 1 && !page->busy(); }), "the inputs are listed when the page opens");
                check(page->ports().size() == 2 && page->portRow(0).label() == "IAC Bus 1" && page->portRow(1).label() == "SMK-37 Elite -Master" && !page->noPorts.isVisible(), "a row for each input, by its name");
                auto& iac = page->controls(0);
                auto& master = page->controls(1);
                check(iac.profile.getNumItems() == 2 && iac.profile.getText() == "Automatic" && page->portRow(0).explanation().startsWith("No controller profile fits"),
                      "an input no profile fits: Automatic or None: " + page->portRow(0).explanation().toStdString());
                check(master.profile.getNumItems() == 5 && master.profile.getText() == "Automatic (SMK37 Elite (stock firmware))" && master.profile.isEnabled() && master.ignore.isEnabled() && !master.ignore.getToggleState(),
                      "-Master: Automatic, None, the two profiles that fit and each of them; not ignored");
                check(page->portRow(1).explanation() == "Played by SMK37 Elite (stock firmware): its controls join.", "what the port is: " + page->portRow(1).explanation().toStdString());
                // Each change counts in the status, which lists again; the next is made once that is done.
                auto settle = [&] { wait([&] { return !page->busy(); }); wait([] { return false; }, 400); wait([&] { return !page->busy(); }); };

                master.profile.setSelectedId(2, juce::sendNotificationSync); // None
                check(page->busy() && !master.profile.isEnabled() && !iac.ignore.isEnabled(), "a change disables the controls while it is made");
                check(wait([&] { return !page->busy() && !page->ports()[1].automatic && page->ports()[1].chosen.empty(); }), "None: chosen with controllers.choose");
                check(page->controls(1).profile.getText() == "None" && page->portRow(1).explanation().startsWith("No profile"), "shown so: " + page->portRow(1).explanation().toStdString());
                settle();
                page->controls(1).profile.setSelectedId(4, juce::sendNotificationSync); // the custom firmware's
                check(wait([&] { return !page->busy() && page->ports()[1].chosen == std::vector<std::string>{"smk37-elite-custom"}; }) && page->controls(1).profile.getText() == "SMK37 Elite (custom firmware)",
                      "a profile chosen: the custom firmware's");
                settle();
                page->controls(1).profile.setSelectedId(5, juce::sendNotificationSync); // each of them
                check(wait([&] { return !page->busy() && page->ports()[1].playing.size() == 2; }) && page->controls(1).profile.getText() == "Each of these, by MIDI channel",
                      "each of them, by MIDI channel, for identical controllers: both play it");
                settle();
                page->controls(0).ignore.triggerClick();
                check(wait([&] { return !page->busy() && page->ports()[0].ignored; }) && page->controls(0).ignore.getToggleState() && page->portRow(0).explanation().startsWith("Ignored") && !page->controls(0).profile.isEnabled(),
                      "Ignore: the port left out, said so");
                // Refused: said under its port, in the danger color.
                settle();
                page->choose(1, std::vector<std::string>{"nonesuch"});
                check(wait([&] { return !page->busy() && page->portRow(1).danger(); }) && page->portRow(1).explanation().contains("nonesuch"), "a profile refused, said under it: " + page->portRow(1).explanation().toStdString());
                // An input comes: the status's counter brings a listing, with the Mackie port read as such.
                settle();
                const int before = page->listings();
                ports.setInputs({"IAC Bus 1", "SMK-37 Elite -Master", "SMK-37 Elite -MCP"});
                check(wait([&] { return page->listings() > before && !page->busy() && page->ports().size() == 3; }), "a status change lists the inputs again");
                check(page->portRow(2).label() == "SMK-37 Elite -MCP" && page->portRow(2).explanation().startsWith("Read as Mackie Control"), "the new port, read as Mackie Control: " + page->portRow(2).explanation().toStdString());
                const auto kept = data.getChildFile("controllers.json").loadFileAsString();
                check(kept.contains("smk37-elite-custom") && kept.contains("IAC Bus 1"), "the choice and the ignored port kept in the data folder");
            }

            // The Package repositories page on an app's connection granted `system` alone. The web
            // is a stand-in transport serving a folder signed with keys made here, so nothing
            // touches the network: the official repository listed with no Remove, a switch
            // turning it off, a non-https address refused before the engine is asked, an https
            // repository asked to be trusted by its root key's fingerprint before it is added,
            // then listed by its name with Remove, and removed again with its key's trust.
            {
                const auto rootKeyFile = data.getChildFile("root.key"), indexKeyFile = data.getChildFile("index.key");
                const auto root = signing::makeKey(rootKeyFile.getFullPathName().toStdString());
                const auto indexKey = signing::makeKey(indexKeyFile.getFullPathName().toStdString());
                const auto site = "https://packages.example.test";
                const auto repo = data.getChildFile("repository-site");
                repo.getChildFile("index").createDirectory();
                // An index of no packages, gzipped (a zlib stream rewrapped as gzip: the reader
                // checks the trailer's CRC and length), named in the root by its hash, as a
                // repository's files are; then the root, then its signature by the index key.
                const std::string indexJson = "{\"format\":\"oscairn.repository-index\",\"version\":1,\"repository\":\"org.example.test\",\"serial\":1,\"packages\":[]}";
                std::string zlib((indexJson.size() + 128) * 2, '\0');
                mz_ulong made = mz_ulong(zlib.size());
                if(mz_compress2(reinterpret_cast<unsigned char*>(zlib.data()), &made, reinterpret_cast<const unsigned char*>(indexJson.data()), mz_ulong(indexJson.size()), MZ_DEFAULT_LEVEL) != MZ_OK) return;
                std::string gz("\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03", 10);
                gz += zlib.substr(2, made - 6); // the deflate stream: past zlib's header, before its adler
                const auto crc = mz_crc32(MZ_CRC32_INIT, reinterpret_cast<const unsigned char*>(indexJson.data()), indexJson.size());
                for(const uint32_t word : {uint32_t(crc), uint32_t(indexJson.size())}) for(int b = 0; b < 4; ++b) gz += char((word >> (8 * b)) & 0xff);
                const auto indexName = "index/" + assets::sha256(gz) + ".json.gz";
                const auto write = [](const juce::File& f, const std::string& bytes) { juce::FileOutputStream out(f); out.write(bytes.data(), bytes.size()); }; // binary: gzip holds bytes no text survives
                write(repo.getChildFile(indexName), gz);
                // Its revocation list, empty, named by its hash too: a root naming none is refused (RFC 0031).
                const std::string revocations = "{\"format\":\"oscairn.repository-revocations\",\"version\":1,\"keys\":[],\"versions\":[],\"builds\":[]}";
                const auto revocationsName = "revocations/" + assets::sha256(revocations) + ".json";
                repo.getChildFile("revocations").createDirectory();
                write(repo.getChildFile(revocationsName), revocations);
                const std::string rootJson = "{\"format\":\"oscairn.repository\",\"version\":1,\"repository\":\"org.example.test\",\"name\":\"Test repository\",\"serial\":1,\"rootKey\":\"" + root.publicKey +
                                             "\",\"indexKey\":{\"key\":\"" + indexKey.publicKey + "\",\"signature\":\"" + signing::sign(root, signing::indexMessage(indexKey.publicKey + "\n")) +
                                             "\"},\"index\":{\"path\":\"" + indexName + "\",\"sha256\":\"" + assets::sha256(gz) + "\",\"size\":" + std::to_string(gz.size()) + "},\"revocations\":{\"path\":\"" + revocationsName + "\",\"sha256\":\"" + assets::sha256(revocations) +
                                             "\",\"size\":" + std::to_string(revocations.size()) + "}}";
                write(repo.getChildFile("repository.json"), rootJson);
                const auto rootSig = signing::sign(indexKey, signing::indexMessage(rootJson));
                write(repo.getChildFile("repository.sig"), "[{\"key\":\"" + indexKey.publicKey + "\",\"signature\":\"" + rootSig + "\"}]");
                repository::setTransport([&](const std::string& url, uint64_t, const std::function<bool(int, const char*, size_t)>& sink) {
                    const std::string base = std::string(site) + "/";
                    const auto file = repo.getChildFile(url.rfind(base, 0) == 0 ? url.substr(base.size()) : "missing");
                    juce::MemoryBlock bytes;
                    if(!file.existsAsFile() || !file.loadFileAsData(bytes)) return 404; // binary: its size and CRC must survive
                    sink(200, static_cast<const char*>(bytes.getData()), bytes.getSize());
                    return 200;
                });

                standalone::Engine app(engine.appConnection("oscairn.settings", manifest->permissions), false);
                standalone::settings::Frame frame(app, standalone::Profile::desktop());
                standalone::setDesktopMode(frame, 1);
                frame.setSize(1000, 640);
                check(frame.open("repositories"), "the list opens Package repositories");
                auto* page = dynamic_cast<standalone::settings::RepositoriesPage*>(frame.page());
                if(!page) return;
                check(wait([&] { return page->listings() >= 1 && !page->busy(); }), "the repositories are listed when the page opens");
                const auto official = std::string("https://packages.oscairn.org");
                check(page->repositories().size() == 1 && page->repositories()[0].address == official && page->repositories()[0].origin == "official" && page->repositories()[0].enabled &&
                          !page->controls(0).remove.isVisible() && page->controls(0).on.getToggleState(),
                      "the official repository, on, with no Remove");
                check(page->repositoryRow(0).label() == "Oscairn" && page->repositoryRow(0).explanation().contains(official) &&
                          page->repositoryRow(0).explanation().contains(standalone::fingerprint(signing::keyId(page->repositories()[0].key))),
                      "its address and root key fingerprint: " + page->repositoryRow(0).explanation().toStdString());
                page->controls(0).on.triggerClick();
                check(wait([&] { return !page->busy() && page->repositories().size() == 1 && !page->repositories()[0].enabled; }), "its switch turns the official one off");
                page->controls(0).on.triggerClick(); // on again: added repositories are compared against it
                check(wait([&] { return !page->busy() && page->repositories()[0].enabled; }), "and on again");

                page->addRepository.triggerClick(); // a click is a message: waited for
                check(wait([&] { return page->adding(); }) && page->askRow().isVisible() && !page->buttonRow().isVisible(), "Add a repository asks for its address");
                for(const auto* refused : {"http://packages.example.test", "/tmp/wherever", "packages.example.test"})
                {
                    page->ask.address.setText(refused);
                    page->ask.add.triggerClick();
                    wait([&] { return page->askRow().danger(); });
                    check(!page->busy() && page->askRow().danger() && page->askRow().explanation().contains("Not an https address"),
                          std::string("an address that is not https:// is refused, and why: ") + refused + " (" + page->askRow().explanation().toStdString() + ")");
                    page->ask.cancel.triggerClick(); // Cancel clears the refusal; Add asks again
                    wait([&] { return !page->adding() && !page->askRow().danger(); });
                    page->addRepository.triggerClick();
                    wait([&] { return page->adding(); });
                }

                // The engine is asked, and its answer decides: an untrusted root asks to be trusted
                // by fingerprint; trusted, it is read again and kept, and kept as the user's own.
                auto alert = [] { return dynamic_cast<juce::AlertWindow*>(juce::ModalComponentManager::getInstance()->getModalComponent(0)); };
                std::atomic<bool> answered = false;
                juce::String trustSaid;
                std::function<void()> answer;
                answer = [&] {
                    if(answered) return;
                    if(auto* w = alert(); w && w->getName() == "Trust this repository?")
                    {
                        for(auto* c : w->getChildren()) if(auto* l = dynamic_cast<juce::Label*>(c)) trustSaid << l->getText();
                        w->exitModalState(1); answered = true; return;
                    }
                    juce::Timer::callAfterDelay(20, answer);
                };
                juce::Timer::callAfterDelay(20, answer);
                page->ask.address.setText(site);
                page->ask.add.triggerClick();
                check(wait([&] { return answered.load(); }), "an https repository whose root is not trusted asks to be trusted");
                check(trustSaid.contains(standalone::fingerprint(signing::keyId(root.publicKey))) && trustSaid.contains("\"Test repository\""), "naming the root key's fingerprint and the name it signed: " + trustSaid.toStdString());
                check(wait([&] { return !page->busy() && page->repositories().size() == 2 && !page->adding(); }) &&
                          page->repositories()[1].address == site && page->repositories()[1].name == "Test repository" &&
                          page->repositories()[1].origin == "user" && page->controls(1).remove.isVisible(),
                      "trusted, it is read, added as the user's and listed with Remove");
                check(page->repositoryRow(1).explanation().contains(site) && page->repositoryRow(1).explanation().contains(standalone::fingerprint(signing::keyId(root.publicKey))),
                      "by address and fingerprint: " + page->repositoryRow(1).explanation().toStdString());
                std::vector<Control::TrustedKey> keys;
                engine.packages([&](Control& c) { keys = c.trustedKeys(); });
                check(std::any_of(keys.begin(), keys.end(), [&](const auto& k) { return k.key == root.publicKey && k.scope == std::vector<std::string>{"repositories"}; }),
                      "its root key trusted for repositories alone");

                page->controls(1).on.triggerClick();
                check(wait([&] { return !page->busy() && !page->repositories()[1].enabled; }), "its switch turns it off");
                page->controls(1).on.triggerClick();
                check(wait([&] { return !page->busy() && page->repositories()[1].enabled; }), "and on again");
                // Remove asks first, and takes its root key's trust with it.
                answered = false;
                answer = [&] {
                    if(answered) return;
                    if(auto* w = alert(); w && w->getName() == "Remove Test repository?") { w->exitModalState(1); answered = true; return; }
                    juce::Timer::callAfterDelay(20, answer);
                };
                juce::Timer::callAfterDelay(20, answer);
                page->controls(1).remove.triggerClick();
                check(wait([&] { return answered.load(); }), "Remove asks first");
                check(wait([&] { return !page->busy() && page->repositories().size() == 1 && page->repositories()[0].origin == "official"; }), "removed: only the official one is listed");
                engine.packages([&](Control& c) { keys = c.trustedKeys(); });
                check(std::none_of(keys.begin(), keys.end(), [&](const auto& k) { return k.key == root.publicKey; }), "and its root key no longer trusted");
                repository::setTransport({});
            }

            // The app as a process of its own, started from its manifest by the shell's supervisor.
            {
                standalone::Actions actions;
                standalone::AppProcesses processes(engine, actions, apps, {themes, "OSCAIRN_PROFILE=desktop"});
                check(processes.open("oscairn.settings") && processes.pid("oscairn.settings") > 0 && processes.pid("oscairn.settings") != ::getpid(), "Settings runs as a process of its own");
                check(wait([&] { return processes.log("oscairn.settings").contains("oscairn-settings: started"); }), "its stderr is in its log");
                check(processes.log("oscairn.settings").contains("with 4 pages for desktop"), "with this computer's pages");
                check(wait([&] { return processes.state("oscairn.settings") == standalone::AppProcesses::State::Running; }, 2000), "and runs");
                processes.stop("oscairn.settings");
                check(wait([&] { return processes.pid("oscairn.settings") < 0; }), "stopped by the shell");
                std::fprintf(stderr, "  its log:\n%s\n", processes.log("oscairn.settings").toRawUTF8());
            }

            // Snapshots: the app renders itself on its own connection, as on screen.
            if(argc >= 5)
            {
                change([&] { devices.outputs = {{"Studio", {44100, 48000, 96000}, {64, 128, 256}}, {"Headphones", {44100, 48000}, {128, 256, 512}}}; devices.fixed = false; devices.source = "user"; devices.inputs.clear(); devices.input.clear(); });
                auto render = [&](std::vector<std::string> arguments, int w, int h) {
                    const auto shot = juce::File::createTempFile(".png");
                    arguments.insert(arguments.begin(), manifest->executable.getFullPathName().toStdString());
                    for(auto a : {std::string("--snapshot"), shot.getFullPathName().toStdString(), std::string("--size"), std::to_string(w) + "x" + std::to_string(h)}) arguments.push_back(a);
                    const int connection = engine.appConnection("oscairn.settings", manifest->permissions);
                    const int output = ::open(data.getChildFile("snapshot.log").getFullPathName().toRawUTF8(), O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
                    const auto child = host::spawnChild(arguments, output, {themes, "OSCAIRN_CONTROL_FD=3"}, connection);
                    ::close(connection); ::close(output);
                    int status = -1;
                    for(int i = 0; i < 1500 && ::waitpid(child, &status, WNOHANG) == 0; ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10);
                    auto image = juce::ImageFileFormat::loadFrom(shot);
                    shot.deleteFile();
                    check(image.isValid() && image.getWidth() == w && image.getHeight() == h, "the app rendered itself at " + std::to_string(w) + " x " + std::to_string(h));
                    if(!image.isValid()) std::fprintf(stderr, "  it wrote:\n%s\n", data.getChildFile("snapshot.log").loadFileAsString().toRawUTF8());
                    return image;
                };
                auto write = [&](const juce::Image& image, const char* name) {
                    const juce::File out{juce::String(argv[4]) + "-settings-" + name + ".png"};
                    out.deleteFile();
                    juce::FileOutputStream stream(out);
                    juce::PNGImageFormat().writeImageToStream(image, stream);
                    std::fprintf(stderr, "  wrote %s\n", out.getFullPathName().toRawUTF8());
                };
                write(render({"--mode", "1"}, 1400, 900), "1400x900");
                write(render({"--mode", "1"}, 900, 700), "900x700");
                // The KICK: the shell's frame under its top bar, the app's window where the shell puts it.
                standalone::Actions actions;
                standalone::AppProcesses processes(engine, actions, apps, {themes});
                auto place = std::make_unique<standalone::AppPlace>(processes, "oscairn.settings");
                standalone::AppFrame shell(engine, "Settings", *place, {});
                standalone::setDesktopMode(shell, 2); standalone::setDesktopMode(*place, 2);
                shell.setSize(1024, 600);
                wait([] { return false; }, 300);
                const auto area = place->getBounds();
                for(const auto& [name, page] : {std::pair<const char*, const char*>{"kick-list", nullptr}, {"kick-audio", "audio"}, {"kick-repositories", "repositories"}})
                {
                    auto image = shell.createComponentSnapshot(shell.getLocalBounds());
                    std::vector<std::string> arguments{"--mode", "2", "--profile", "kick"};
                    if(page) arguments.insert(arguments.end(), {"--page", page});
                    const auto app = render(arguments, area.getWidth(), area.getHeight());
                    { juce::Graphics g(image); g.drawImageAt(app, area.getX(), area.getY()); }
                    write(image, name);
                }
            }
            }();
        }
        running = false;
        control.join();
    }
    std::filesystem::remove(socket);
    data.deleteRecursively(false); configuration.deleteRecursively(false);
    if(failures) std::fprintf(stderr, "%d failed\n", failures);
    return failures ? 1 : 0;
}
