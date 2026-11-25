// SPDX-License-Identifier: AGPL-3.0-or-later
#include "../audio/AudioEngine.h"
#include "oscairn/local_socket.h"
#include "EngineProcess.h"
#include "Pump.h"
#include "oscairn/plugin_host.h"
#include <juce_events/juce_events.h>
#include <csignal>
#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <sys/socket.h>
#include <unistd.h>

namespace
{
    volatile std::sig_atomic_t stopping=0;
}
int main(int argc,char** argv)
{
    oscairn::ignoreTickSignal();
    std::string plugins=OSCAIRN_PLUGIN_DIR,socket;
    bool managed=false,startup=false;
    oscairn::host::AudioConfiguration audioConfig;
    try { for(int i=1;i<argc;++i)
    {
        const std::string arg=argv[i];
        if(oscairn::host::AudioConfiguration::isOption(arg) && i+1<argc)
        {
            audioConfig.setOption(arg,argv[++i]);continue;
        }
        if(arg=="--managed"){managed=true;continue;}
        if(arg=="--startup-performance"){startup=true;continue;}
        if(arg=="--help"){std::cout<<"oscairn-engine --socket /private/directory/control.sock [--plugins DIR]\n  [--input-device NAME] [--output-device NAME]\n  [--audio-inputs none|0,1,...] [--audio-outputs 0,1,...]\n  [--device-channels N] [--sample-rate HZ] [--buffer-frames N]\n  [--panel-device /dev/NAME --panel-map FILE] [--keyboard-device /dev/NAME [--keyboard-local off]] (built with OSCAIRN_ROLAND_FANTOM0)\n  [--input-keys NAME --input-key-map CODE=FUNCTION,...] (an input device's keys, by name; built with OSCAIRN_ROLAND_FANTOM0, Linux)\n  [--device-profile ID|FILE] (the device's control groups, which its panel plays by, until an app sends its own)\n  [--controller FILE]... (a controller's profile, beside the built-in ones)\n  [--audio null] (a device that writes nowhere, at real-time pace, and no MIDI device: for tests)\n  [--startup-performance] (the startup Performance, not the one last open)\nChannel indices are zero-based; list order defines logical graph channels.\n";return 0;}
        if((arg=="--socket" || arg=="--plugins") && i+1<argc){(arg=="--socket"?socket:plugins)=argv[++i];continue;}
        std::cerr<<"Unknown or incomplete argument: "<<arg<<'\n';return 2;
    }
    } catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 2;}
    if(socket.empty()){std::cerr<<"--socket is required; its parent must be a private directory owned by this user\n";return 2;}
    std::signal(SIGINT,[](int){stopping=1;});std::signal(SIGTERM,[](int){stopping=1;});
    oscairn::disableTransparentHugePages("oscairn-engine"); // the workers it starts inherit it
    // RFC 0035: on the audio cores before any thread or worker starts, so all of them are there.
    if(const auto missed=oscairn::applyCpuPolicy();!missed.empty())std::cerr<<"oscairn-engine: "<<missed<<'\n';
    // RFC 0025: at instrument and above, locked and last in the out-of-memory order (not -1000: it may be the leak).
    if(const auto missed=oscairn::applyMemoryPolicy(-999);!missed.empty() && std::getenv("OSCAIRN_RELIABILITY"))std::cerr<<"oscairn-engine: "<<missed<<'\n';
    juce::ScopedJuceInitialiser_GUI initialize; // JUCE event loop initialization; no GUI modules linked
    // RFC 0010: every plugin in a worker process, the worker found beside this program (in the app)
    // or where it was built. OSCAIRN_PLUGIN_WORKER overrides it; set but empty, plugins run in the engine.
    auto policy = oscairn::placementPolicy();
    if(!std::getenv("OSCAIRN_PLUGIN_WORKER"))
    {
        auto worker=juce::File::getSpecialLocation(juce::File::currentExecutableFile).getSiblingFile("oscairn-plugin-worker");
        if(!worker.existsAsFile())worker=juce::File(OSCAIRN_PLUGIN_WORKER_PATH);
        if(worker.existsAsFile())policy.worker=worker.getFullPathName().toStdString();
        else std::cerr<<"oscairn-engine: no plugin worker found; plugins run in the engine\n";
    }
    // RFC 0005: a loaded plugin's preset listing runs the plugin's provider in this scan
    // helper, never on this process's control thread. OSCAIRN_PLUGIN_SCAN overrides it.
    if(policy.scan.empty())
    {
        auto scan=juce::File::getSpecialLocation(juce::File::currentExecutableFile).getSiblingFile("oscairn-plugin-scan");
        if(!scan.existsAsFile())scan=juce::File(OSCAIRN_PLUGIN_SCAN_PATH);
        if(scan.existsAsFile())policy.scan=scan.getFullPathName().toStdString();
    }
    oscairn::setPlacementPolicy(policy);
    try
    {
        std::unique_ptr<oscairn::host::EngineLease> lease;
        if(managed)lease=std::make_unique<oscairn::host::EngineLease>(socket);
        oscairn::host::AudioEngine audio(plugins,std::move(audioConfig));
        oscairn::LocalServer server(audio.control());server.listen(socket);
        oscairn::host::Pump pump(server,managed,stopping);
        audio.setServiced([&pump]{pump.poll();}); // what changed goes out after each service tick
        std::cout<<"oscairn-engine: listening on "<<socket<<std::endl;
        // The Performance last open (performance.reopen), asked as a client would, so the server answers the others while it
        // loads (Long requests in docs/control-protocol.md). A desktop app that started this engine asks it itself.
        if(!managed && !startup)
        {
            int ends[2];
            if(::socketpair(AF_UNIX,SOCK_STREAM,0,ends)!=0)std::cerr<<"oscairn-engine: the last Performance is not reopened: no socket pair\n";
            else if(server.adopt(ends[0]))
            {
                static constexpr char request[]="{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"performance.reopen\",\"params\":{}}\n";
                if(::write(ends[1],request,sizeof request-1)!=ssize_t(sizeof request-1))std::cerr<<"oscairn-engine: the last Performance is not reopened: the request was not sent\n";
                ::shutdown(ends[1],SHUT_WR); // a half-close: the server answers, then lets the connection go; the reply is not read (the engine logs why one did not open)
            }
            else ::close(ends[1]);
        }
#if JUCE_LINUX
        // The message loop sleeps until a message comes (a timer's, a device's), where runDispatchLoopUntil
        // would wake every millisecond to look (about 1,000 wake-ups a second of the main thread at rest);
        // a timer ends it once a signal asks.
        struct Stop final : juce::Timer { void timerCallback() override { if(stopping) juce::MessageManager::getInstance()->stopDispatchLoop(); } } stop;
        stop.startTimer(50);
        juce::MessageManager::getInstance()->runDispatchLoop();
        stop.stopTimer();
#else
        // On macOS runDispatchLoop() calls [NSApp run], but this console host
        // has no NSApplication. The bounded loop pumps the native run loop too,
        // so JUCE timers and device callbacks stay serviced without a GUI app.
        while(!stopping && juce::MessageManager::getInstance()->runDispatchLoopUntil(50)) {}
#endif
        audio.setServiced({});
        return pump.result;
    }
    catch(const std::exception& e){std::cerr<<"oscairn-engine: "<<e.what()<<'\n';return 1;}
}
