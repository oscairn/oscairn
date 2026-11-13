// SPDX-License-Identifier: AGPL-3.0-or-later
//
// oscairn-panel-dump: prints every message from a non-MIDI device panel (panel_reader.h), as the
// panel holder passes them on (it never opens the device itself), one line each: its four bytes, the kind, ID and value the map's layout reads from them, and the control
// the map gives that kind and ID, or "-" for none; then, on Ctrl-C or after --seconds, a table
// with a row per kind and ID: how often it came and its lowest and highest value. Move each
// control in turn and fill in the map's entries from the table (docs/fantom-0.md).
//   oscairn-panel-dump MAP [--socket NAME] [--seconds N]
#include "oscairn/panel_reader.h"

#include <chrono>
#include <csignal>
#include <cstdio>
#include <fstream>
#include <map>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <unistd.h>

namespace
{
    volatile std::sig_atomic_t stopping = 0;
    struct Seen { uint64_t count = 0; int64_t low = 0, high = 0; std::string control; };
}

int main(int argc, char** argv)
{
    using namespace oscairn;
    std::string socket = panelHolderSocket;
    double seconds = 0;
    bool usage = argc < 2;
    for(int i = 2; i < argc && !usage; i += 2)
    {
        const std::string option = argv[i];
        if(i + 1 >= argc) usage = true;
        else if(option == "--socket") socket = argv[i + 1];
        else if(option == "--seconds") seconds = std::atof(argv[i + 1]);
        else usage = true;
    }
    if(usage)
    {
        std::fprintf(stderr, "usage: oscairn-panel-dump MAP [--socket NAME] [--seconds N]\n");
        return 2;
    }
    PanelMap map;
    try
    {
        std::ifstream in(argv[1], std::ios::binary);
        if(!in) throw std::invalid_argument(std::string("cannot read ") + argv[1]);
        std::stringstream text; text << in.rdbuf();
        map = PanelMap::parse(text.str());
    }
    catch(const std::exception& e) { std::fprintf(stderr, "oscairn-panel-dump: %s\n", e.what()); return 2; }
    // The device is the holder's alone: a second open can crash the kernel, and a reader that
    // opened and closed it would leave the panel unheld after. The connection made here is the
    // reader's, so the holder has one reader from the dump, not a probe and then a reader.
    const int attached = connectPanelHolder(socket);
    if(attached < 0)
    {
        std::fprintf(stderr, "oscairn-panel-dump: no panel holder at %s; start oscairn-panel-holder first (docs/fantom-0.md, \"The panel holder\")\n", socket.c_str());
        return 1;
    }
    std::signal(SIGINT, [](int) { stopping = 1; });
    std::signal(SIGTERM, [](int) { stopping = 1; });

    std::mutex lock;
    std::map<std::pair<int, uint32_t>, Seen> seen;
    {
        PanelReader reader(socket, map, [&](const PanelEvent& e) {
            std::printf("%02x %02x %02x %02x  %-9s id %-5u value %-6lld %s\n", e.raw[0], e.raw[1], e.raw[2], e.raw[3], kindName(e.kind), e.id, (long long)e.value,
                        e.control.empty() ? "-" : e.control.c_str());
            std::fflush(stdout);
            std::lock_guard g(lock);
            auto& s = seen[{int(e.kind), e.id}];
            if(!s.count++) s.low = s.high = e.value;
            s.low = std::min(s.low, e.value); s.high = std::max(s.high, e.value); s.control = e.control;
        }, [](const std::string& line) { if(!line.starts_with("unknown message") && !line.starts_with("attached")) std::fprintf(stderr, "oscairn-panel-dump: %s\n", line.c_str()); }, // unknown ones are on stdout already
           attached);
        const auto start = std::chrono::steady_clock::now();
        while(!stopping && (seconds <= 0 || std::chrono::steady_clock::now() - start < std::chrono::duration<double>(seconds)))
            std::this_thread::sleep_for(std::chrono::milliseconds(50));
    } // the reader stops here
    std::printf("\nkind       id     count   low     high    control\n");
    for(const auto& [key, s] : seen)
        std::printf("%-9s  %-5u  %-6llu  %-6lld  %-6lld  %s\n", kindName(PanelMap::Kind(key.first)), key.second, (unsigned long long)s.count, (long long)s.low, (long long)s.high,
                    s.control.empty() ? "-" : s.control.c_str());
    return 0;
}
