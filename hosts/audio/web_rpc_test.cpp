// SPDX-License-Identifier: AGPL-3.0-or-later
// A control session on stdin and stdout, a JSON-RPC request per line, that fetches package
// repositories from the web as the engine does (WebTransport); no audio device. For
// control-repository-web (core/control/tests/repository.py), and with a repository snapshot's
// folder, preinstalling from it as the engine does at start (core/control/tests/preinstall.py).
#include "WebTransport.h"
#include "oscairn/json_rpc.h"
#include "oscairn/session.h"
#include <juce_events/juce_events.h>
#include <iostream>

int main(int argc, char** argv)
{
    juce::ScopedJuceInitialiser_GUI juce;
    oscairn::host::useWebTransport();
    oscairn::Session session(48000, 64);
    oscairn::JsonRpc rpc(session);
    if(argc > 1) session.preinstall(argv[1]);
    for(std::string line; std::getline(std::cin, line);) std::cout << rpc.dispatch(line).value_or("") << '\n' << std::flush;
}
