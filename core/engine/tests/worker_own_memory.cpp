// SPDX-License-Identifier: AGPL-3.0-or-later
// oscairn-worker-own-memory-test: a worker's own memory on Linux (RFC 0025) from fixture /proc text. A worker of
// the C++ CLAP host and one of the Rust host, the same plugin and the same private memory, differ only in the
// file pages of their binaries (the Rust one about 1 MB larger), which every worker shares: both count the same.
#include <oscairn/plugin_host.h>
#include <cstdio>
#include <filesystem>
#include <string>

namespace
{
    int failures = 0;
    void expect(bool ok, const char* what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what); ++failures; } }

    // /proc/PID/status at the worker's start (initialized, locked, no plugin): its binary and libraries.
    std::string status(unsigned fileKb)
    {
        return "Name:\toscairn-plugin-\nVmLck:\t   12000 kB\nRssAnon:\t    1200 kB\nRssFile:\t    " + std::to_string(fileKb) + " kB\nRssShmem:\t      40 kB\n";
    }
    // /proc/PID/statm later, in pages: size resident shared text lib data dt (shared: file-backed and shared memory).
    std::string statm(unsigned resident, unsigned shared) { return "250000 " + std::to_string(resident) + " " + std::to_string(shared) + " 120 0 9000 0\n"; }
}

int main(int, char** argv)
{
    constexpr uint64_t page = 4096;
    // Start: C++ 4700 kB of file pages (1175 pages), Rust 5680 kB (1420 pages: +980 kB, as seen on the FANTOM-07).
    const auto cppStart = oscairn::residentFileBytes(status(4700)), rustStart = oscairn::residentFileBytes(status(5680));
    expect(cppStart == 4700 * 1024 && rustStart == 5680 * 1024, "RssFile read from the status text");
    // Later, the same plugin in each: 1800 pages anonymous, 10 of its shared block, 400 of the plugin's own file;
    // resident = anonymous + shared memory + file, shared = shared memory + file.
    const auto cpp = oscairn::workerOwnBytes(statm(1800 + 10 + 1175 + 400, 10 + 1175 + 400), cppStart, page);
    const auto rust = oscairn::workerOwnBytes(statm(1800 + 10 + 1420 + 400, 10 + 1420 + 400), rustStart, page);
    expect(cpp == (1800 + 10 + 400) * page, "the C++ worker counts its anonymous, shared block and plugin pages");
    expect(rust == cpp, "the Rust worker counts the same as the C++ one");
    // As whole resident memory, they differed by the Rust binary's extra pages.
    expect(oscairn::workerOwnBytes(statm(3630, 1830), 0, page) - oscairn::workerOwnBytes(statm(3385, 1585), 0, page) == 245 * page, "with no start reading, the whole resident memory");
    // Start pages since let go (an unlocked worker): never less than its anonymous pages.
    expect(oscairn::workerOwnBytes(statm(2000, 300), cppStart, page) == 1700 * page, "at least the pages not file-backed");
    expect(oscairn::workerOwnBytes("", cppStart, page) == 0 && oscairn::workerOwnBytes("12", 0, page) == 0, "unreadable statm: 0");
    expect(oscairn::residentFileBytes("Name:\tx\nVmRSS:\t 10 kB\n") == 0, "no RssFile: 0");
    // The binary's code, counted once: on Linux this test's own loadable segments, at most its file and a page a segment.
    const auto code = oscairn::workerCodeBytes(argv[0]);
#ifdef __linux__
    const auto size = std::filesystem::file_size(argv[0]);
    expect(code > 0 && code <= size + 8 * page, "the code of an ELF binary: its loadable segments");
    expect(oscairn::workerCodeBytes("/nonexistent/oscairn-plugin-worker") == 0, "no binary: 0");
#else
    expect(code == 0, "macOS: 0 (the footprint leaves clean code out)");
#endif
    if(!failures) std::printf("worker own memory: C++ %llu KiB, Rust %llu KiB; code %llu KiB\n", (unsigned long long)(cpp >> 10), (unsigned long long)(rust >> 10), (unsigned long long)(code >> 10));
    return failures ? 1 : 0;
}
