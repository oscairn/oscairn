// SPDX-License-Identifier: AGPL-3.0-or-later
//
// oscairn-panel-holder: holds a FANTOM-0's panel device for as long as the system runs, and
// passes each of its messages to the readers attached to its socket: the engine, and
// oscairn-panel-dump (holdPanel in panel_reader.h; docs/fantom-0.md, "The panel holder"). One
// runs at a time; SIGTERM stops it. It leaves the session it was started in, so the shell that
// started it can go.
//   oscairn-panel-holder DEVICE [--socket NAME]
#include "oscairn/panel_reader.h"
#include "oscairn/plugin_host.h"

#include <cstdio>
#include <ctime>
#include <string>
#include <sys/stat.h>
#include <unistd.h>

namespace
{
    // The log is the file the start script gave as stderr (appended to). One line a second was 11 MB
    // in four days once; whatever fills it again, it never passes this: past 256 KiB it is emptied,
    // and says so.
    constexpr off_t logLimit = 256 * 1024;
    void emptyLogIfFull()
    {
        struct stat s{};
        if(::fstat(STDERR_FILENO, &s) == 0 && S_ISREG(s.st_mode) && s.st_size > logLimit && ::ftruncate(STDERR_FILENO, 0) == 0)
            std::fprintf(stderr, "oscairn-panel-holder: the log passed %lld KiB and was emptied\n", (long long)(logLimit / 1024));
    }
}

int main(int argc, char** argv)
{
    oscairn::ignoreTickSignal();
    if(!(argc == 2 || (argc == 4 && std::string(argv[2]) == "--socket")))
    {
        std::fprintf(stderr, "usage: oscairn-panel-holder DEVICE [--socket NAME]\n");
        return 2;
    }
    ::setsid();
    return oscairn::holdPanel(argv[1], argc == 4 ? argv[3] : oscairn::panelHolderSocket, [](const std::string& line) {
        emptyLogIfFull();
        char when[32];
        const auto now = std::time(nullptr);
        std::strftime(when, sizeof when, "%Y-%m-%d %H:%M:%S", std::localtime(&now));
        std::fprintf(stderr, "%s oscairn-panel-holder: %s\n", when, line.c_str());
    });
}
