// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0023 step 5's gate: 50,000 items indexed in place. A rescan with nothing changed reads no
// file (every file is made unreadable first, and all stay listed); a search answers within 50 ms;
// an index damaged or out of date is rebuilt from the files, not trusted. Prints what it measured.
// With --sanitized (a sanitizer slows it several times over) it lists 5,000 items and checks no time.
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "oscairn/plugin_host.h"
#include "oscairn/assets.h"
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <sys/resource.h>
#include <sys/stat.h>

using namespace oscairn;
namespace fs = std::filesystem;
using Clock = std::chrono::steady_clock;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    void write(const fs::path& f, const std::string& text) { fs::create_directories(f.parent_path()); std::ofstream(f, std::ios::binary | std::ios::trunc) << text; }
    // The fastest of n runs, in milliseconds.
    double best(int n, const std::function<void()>& f)
    {
        double out = 1e9;
        for(int i = 0; i < n; ++i) { const auto t = Clock::now(); f(); out = std::min(out, std::chrono::duration<double, std::milli>(Clock::now() - t).count()); }
        return out;
    }
    double mib(uint64_t bytes) { return double(bytes) / (1 << 20); }
    uint64_t peak()
    {
        rusage u{}; getrusage(RUSAGE_SELF, &u);
#ifdef __APPLE__
        return uint64_t(u.ru_maxrss);
#else
        return uint64_t(u.ru_maxrss) * 1024;
#endif
    }
    Control::LibraryResult tracks(Session& s, const std::string& text = {}, uint32_t limit = 200)
    {
        Control::LibraryQuery q; q.types = {"com.example.scale.track"}; q.text = text; q.limit = limit;
        return s.libraryQuery(q);
    }
    void chmodAll(const fs::path& root, fs::perms p)
    {
        for(const auto& e : fs::recursive_directory_iterator(root)) if(e.is_regular_file()) fs::permissions(e.path(), p);
    }
}

int main(int argc, char** argv)
{
    const bool sanitized = argc > 1 && std::string(argv[1]) == "--sanitized";
    const size_t folders = sanitized ? 50 : 500, perFolder = 100, total = folders * perFolder;
    try
    {
        state_files::TemporaryDirectory temp;
        const auto music = temp.path / "music", home = temp.path / "home";
        for(size_t f = 0; f < folders; ++f)
            for(size_t n = 0; n < perFolder; ++n) write(music / ("folder " + std::to_string(f)) / ("track " + std::to_string(f * perFolder + n) + ".wav"), "audio " + std::to_string(f * perFolder + n));
        {
            state_files::TemporaryDirectory stage;
            write(stage.path / "Types/com.example.scale/track.json", R"({"format":"oscairn.type","version":1,"id":"com.example.scale.track","name":"Track","plural":"Tracks",
                "recognize":{"extensions":[".wav"]},"storage":"in-place","folder":"ScaleTracks","identity":"content"})");
            Package p; p.id = "com.example.scale"; p.name = "Scale"; p.kind = "library";
            writePackage(temp.path / "Scale.olib", p, stage.path);
        }
        fs::create_directories(home);
        ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        auto policy = placementPolicy(); policy.scan.clear(); setPlacementPolicy(policy); // no extractors here
        const auto before = residentBytes();
        {
            Session s(48000, 64);
            s.installArchive((temp.path / "Scale.olib").string(), true);
            auto t = Clock::now();
            s.addLibraryFolder(music.string());
            const double first = std::chrono::duration<double, std::milli>(Clock::now() - t).count();
            check(tracks(s).total == total, "every file listed: " + std::to_string(tracks(s).total));
            const auto scanned = residentBytes();
            const double rescan = best(3, [&] { s.libraryRescan(); });
            // Nothing changed, nothing read: unreadable files stay listed by what the index holds.
            chmodAll(music, fs::perms::none);
            s.libraryRescan();
            const auto unread = tracks(s).total;
            chmodAll(music, fs::perms::owner_read | fs::perms::owner_write);
            check(unread == total, "a rescan with nothing changed reads no file: " + std::to_string(unread) + " listed with every file unreadable");
            const double all = best(5, [&] { tracks(s); });
            const double text = best(5, [&] { tracks(s, "track 4242"); });
            const double none = best(5, [&] { tracks(s, "no such thing"); });
            check(tracks(s, "track 4242").total == (total > 42429 ? 1 + 10 : 1), "a search finds what it should"); // 4242 and 42420..42429
            check(sanitized || std::max({all, text, none}) < 50, "a search answers within 50 ms");
            std::printf("measured: %zu items: first scan %.0f ms; rescan, nothing changed %.0f ms; query, all %.1f ms, text %.1f ms, no match %.1f ms\n", total, first, rescan, all, text, none);
            std::printf("measured: resident memory %.1f MiB before the library, %.1f MiB after the first scan (+%.1f), peak %.1f MiB\n", mib(before), mib(scanned), mib(scanned) - mib(before), mib(peak()));
            std::error_code e;
            std::uintmax_t index = 0;
            for(const auto& f : fs::directory_iterator(home / "library", e)) if(f.path().filename() != "user.json") index += f.file_size();
            std::printf("measured: index on disk %.1f MiB\n", mib(index));
        }
        // A damaged index is rebuilt from the files.
        for(const auto& f : fs::directory_iterator(home / "library"))
            if(f.path().filename() != "user.json") write(f.path(), std::string(4096, 'x'));
        {
            Session s(48000, 64);
            check(tracks(s).total == total, "a damaged index is rebuilt: " + std::to_string(tracks(s).total));
        }
        // One out of date is not trusted: a file changed and one gone while the library was closed.
        write(music / "folder 0/track 0.wav", "other audio");
        fs::last_write_time(music / "folder 0/track 0.wav", fs::last_write_time(music / "folder 0/track 0.wav") + std::chrono::seconds(5));
        fs::remove(music / "folder 1/track 100.wav");
        {
            Session s(48000, 64);
            const auto changed = tracks(s, "track 0", 1000);
            const auto zero = std::find_if(changed.items.begin(), changed.items.end(), [](const auto& i) { return i.name == "track 0"; });
            const auto hundred = tracks(s, "track 100", 1000).items;
            check(tracks(s).total == total - 1 && zero != changed.items.end() && zero->id == assets::sha256("other audio") &&
                  std::none_of(hundred.begin(), hundred.end(), [](const auto& i) { return i.name == "track 100"; }),
                  "an index out of date is corrected from the files");
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    if(!failures) std::puts("ok: 50,000 items in place: a rescan reads no file unchanged, a search within 50 ms, a damaged or stale index rebuilt");
    return failures ? 1 : 0;
}
