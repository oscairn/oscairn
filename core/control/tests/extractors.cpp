// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0023 step 4: extractors. Files of an in-place type with an extractor are read out of
// process, off the control thread, one process per file, with a timeout, a memory limit, an
// output limit and its stderr kept. One that hangs, crashes, floods, grows or lies costs that
// file its fields only: the scan goes on and the rest have theirs. A rescan with nothing changed
// runs no extractor. Then Oscairn's own audio extractor (in the scan helper): its identity is the
// audio's, so retagging a WAV, AIFF, FLAC or MP3 file keeps it.
//
// oscairn-extractors-test <hostile extractor fixture> <oscairn-plugin-scan>
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "oscairn/plugin_host.h"
#include "oscairn/assets.h"
#include "oscairn/item_types.h"
#include "../../plugin-host/src/StateFiles.h"
#include <kick_clap/Discovery.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <memory>
#include <thread>

using namespace oscairn;
namespace fs = std::filesystem;
using Clock = std::chrono::steady_clock;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    void write(const fs::path& f, const std::string& text) { fs::create_directories(f.parent_path()); std::ofstream(f, std::ios::binary | std::ios::trunc) << text; }
    double seconds(Clock::time_point since) { return std::chrono::duration<double>(Clock::now() - since).count(); }
    std::vector<Control::LibraryItem> tracks(Session& s) { Control::LibraryQuery q; q.types = {"com.example.tracks.track"}; return s.libraryQuery(q).items; }
    std::string field(const Control::LibraryItem& i, const std::string& k)
    {
        for(const auto& [key, value] : i.fields) if(key == k) return value;
        return {};
    }
    size_t lines(const fs::path& f) { std::ifstream in(f); std::string l; size_t n = 0; while(std::getline(in, l)) ++n; return n; }

    // Little-endian and big-endian numbers, for the audio files below.
    std::string le(uint32_t v, int n) { std::string s; for(int i = 0; i < n; ++i) s += char(v >> (8 * i) & 255); return s; }
    std::string be(uint32_t v, int n) { std::string s; for(int i = n - 1; i >= 0; --i) s += char(v >> (8 * i) & 255); return s; }
    std::string wav(const std::string& title, const std::string& audio)
    {
        std::string info = "INFO" + std::string("INAM") + le(uint32_t(title.size() + 1), 4) + title + '\0';
        if(info.size() & 1) info += '\0';
        const std::string body = "WAVE" + std::string("fmt ") + le(16, 4) + le(1, 2) + le(2, 2) + le(44100, 4) + le(44100 * 4, 4) + le(4, 2) + le(16, 2) +
                                 "LIST" + le(uint32_t(info.size()), 4) + info + "data" + le(uint32_t(audio.size()), 4) + audio;
        return "RIFF" + le(uint32_t(body.size()), 4) + body;
    }
    std::string aiff(const std::string& title, const std::string& audio)
    {
        // 44100 as an 80-bit extended number.
        const std::string rate = std::string("\x40\x0e\xac\x44", 4) + std::string(6, '\0');
        std::string name = title; if(name.size() & 1) name += '\0';
        const std::string body = "AIFF" + std::string("COMM") + be(18, 4) + be(1, 2) + be(uint32_t(audio.size() / 2), 4) + be(16, 2) + rate +
                                 "NAME" + be(uint32_t(title.size()), 4) + name + "SSND" + be(uint32_t(audio.size() + 8), 4) + be(0, 4) + be(0, 4) + audio;
        return "FORM" + be(uint32_t(body.size()), 4) + body;
    }
    std::string flac(const std::string& title, const std::string& frames)
    {
        // STREAMINFO: 48000 Hz, 2 channels, 16 bits, 96000 samples (2 seconds).
        std::string info(34, '\0');
        info[10] = char(0x0b); info[11] = char(0xb8); info[12] = char(0x00 | (1 << 1) | 0); info[13] = char(0xf0);
        info.replace(14, 4, be(96000, 4));
        const std::string comment = "TITLE=" + title;
        const std::string vorbis = le(6, 4) + "Oscair" + le(1, 4) + le(uint32_t(comment.size()), 4) + comment;
        return "fLaC" + std::string(1, '\0') + be(34, 3) + info + std::string(1, char(0x84)) + be(uint32_t(vorbis.size()), 3) + vorbis + frames;
    }
    std::string mp3(const std::string& title, const std::string& frames, bool v1)
    {
        const std::string text = std::string(1, '\3') + title;
        const std::string frame = "TIT2" + be(uint32_t(text.size()), 4) + std::string(2, '\0') + text;
        std::string out = "ID3" + std::string("\3\0\0", 3) + be(uint32_t(frame.size()), 4) + frame + frames; // sizes under 128: syncsafe as they are
        if(v1) out += "TAG" + (title + std::string(30, '\0')).substr(0, 30) + std::string(95, '\0');
        return out;
    }
    struct Extracted { bool ok = false; std::string identity, title, output; };
    Extracted extract(const std::string& helper, const fs::path& file)
    {
        const auto r = kick::clap::runIsolated({helper, "--extract", "audio", file.string()}, 10000, 64 * 1024);
        Extracted e; e.ok = r.outcome == kick::clap::IsolatedRun::Exited; e.output = r.output;
        auto value = [&](const std::string& key) {
            const auto at = r.output.find("\"" + key + "\":\"");
            return at == std::string::npos ? std::string() : r.output.substr(at + key.size() + 4, r.output.find('"', at + key.size() + 4) - at - key.size() - 4);
        };
        e.identity = value("identity"); e.title = value("title");
        return e;
    }
}

int main(int argc, char** argv)
{
    if(argc != 3) { std::fprintf(stderr, "usage: oscairn-extractors-test <fixture> <oscairn-plugin-scan>\n"); return 2; }
    const std::string fixture = argv[1], helper = argv[2];
    try
    {
        state_files::TemporaryDirectory temp;
        const auto music = temp.path / "music";
        // A type names one of Oscairn's extractors, for files in place.
        const std::string type = R"({"format":"oscairn.type","version":1,"id":"com.example.x.track","name":"T","plural":"Ts","recognize":{"extensions":[".wav"]},"storage":"in-place","folder":"X","identity":"payload","extractor":"audio"})";
        auto refused = [](const std::string& m) { try { readItemType(m); return false; } catch(const std::invalid_argument&) { return true; } };
        check(!refused(type) && readItemType(type).extractor == "audio", "a type names the audio extractor");
        check(refused(std::string(type).replace(type.find("\"audio\""), 7, "\"python\"")), "an extractor Oscairn does not have is refused");
        check(refused(std::string(type).replace(type.find("in-place"), 8, "content")), "an extractor for a type not stored in place is refused");
        // The runs themselves: each failure ends in its own way, its stderr kept.
        {
            using R = kick::clap::IsolatedRun;
            auto run = [&](const std::string& name, uint32_t timeout, size_t output, uint64_t memory) {
                return kick::clap::runIsolated({fixture, "--extract", "audio", (music / name).string()}, timeout, output, nullptr, memory, &residentBytes);
            };
            auto started = Clock::now();
            check(run("hang.wav", 300, 4096, 0).outcome == R::TimedOut && seconds(started) < 3, "a hanging extractor is ended at its timeout");
            const auto crash = run("crash.wav", 5000, 4096, 0);
            check(crash.outcome == R::Failed && crash.stderrTail.find("crashing on purpose") != std::string::npos, "a crash is seen, with what it wrote to stderr: " + crash.stderrTail);
            check(run("flood.wav", 5000, 64 * 1024, 0).outcome == R::Oversized, "one that writes too much is ended");
            started = Clock::now();
            check(run("hog.wav", 20000, 4096, uint64_t(128) << 20).outcome == R::OverMemory && seconds(started) < 15, "one that takes too much memory is ended");
            check(run("good-1.wav", 5000, 4096, 0).outcome == R::Exited, "a good one answers");
        }
        // Through the library: a package's in-place type with an extractor, the fixture standing in for the scan helper.
        auto policy = placementPolicy(); policy.scan = fixture; setPlacementPolicy(policy);
        const auto log = temp.path / "runs.log";
        ::setenv("OSCAIRN_FIXTURE_LOG", log.c_str(), 1);
        const std::vector<std::string> bad = {"hang", "crash", "flood", "hog", "fail", "lie-json", "lie-utf8", "lie-escape", "lie-identity", "lie-none", "lie-type", "lie-long", "lie-twice"};
        for(const auto& b : bad) write(music / (b + ".wav"), b);
        write(music / "good-1.wav", "1"); write(music / "set/good-2.wav", "2");
        fs::create_directories(temp.path / "home");
        ::setenv("OSCAIRN_DATA_HOME", (temp.path / "home").c_str(), 1);
        {
            state_files::TemporaryDirectory stage;
            write(stage.path / "Types/com.example.tracks/track.json", R"({"format":"oscairn.type","version":1,"id":"com.example.tracks.track","name":"Track","plural":"Tracks",
                "recognize":{"extensions":[".wav"]},"storage":"in-place","folder":"Tracks","identity":"payload","extractor":"audio",
                "fields":[{"id":"artist","name":"Artist","facet":true},{"id":"bpm","name":"BPM"}]})");
            Package p; p.id = "com.example.tracks"; p.name = "Tracks"; p.kind = "library";
            writePackage(temp.path / "Tracks.olib", p, stage.path);
        }
        {
            auto session = std::make_unique<Session>(48000, 64);
            auto& s = *session;
            s.installArchive((temp.path / "Tracks.olib").string(), true);
            auto started = Clock::now();
            s.addLibraryFolder(music.string());
            check(seconds(started) < 1, "adding the folder does not wait for its extractors");
            const auto total = bad.size() + 2;
            auto listed = tracks(s);
            check(listed.size() < total, "files are listed as their extractor answers, not before");
            while(listed.size() < total && seconds(started) < 90) { std::this_thread::sleep_for(std::chrono::milliseconds(100)); listed = tracks(s); }
            check(listed.size() == total, "every file is listed once its extractor has answered or failed: " + std::to_string(listed.size()));
            for(const auto& t : listed)
            {
                if(t.name.starts_with("good-"))
                    check(t.id == std::string(63, 'a') + t.name.back() && field(t, "artist") == "Fixture \xc3\xa9" && field(t, "bpm") == "120" && t.fields.size() == 2,
                          t.name + ": its identity and fields are the extractor's, but one its type does not declare");
                else check(t.id == assets::sha256(t.path) && t.fields.empty(), t.name + ": a failed or lying extractor costs it its fields, and its identity is its path's");
            }
            const auto runs = lines(log);
            check(runs == total, "one run per file: " + std::to_string(runs));
            s.libraryRescan();
            check(tracks(s).size() == total && lines(log) == runs, "a rescan with nothing changed runs no extractor");
            // A file changed is read again; one that hangs while the library closes is ended with it.
            write(music / "good-1.wav", "changed");
            fs::last_write_time(music / "good-1.wav", fs::last_write_time(music / "good-1.wav") + std::chrono::seconds(5));
            write(music / "hang-late.wav", "x");
            s.libraryRescan();
            started = Clock::now();
            while(lines(log) < runs + 2 && seconds(started) < 30) { std::this_thread::sleep_for(std::chrono::milliseconds(50)); tracks(s); }
            check(lines(log) == runs + 2, "the changed file and the new one are read");
            started = Clock::now();
            session.reset();
            check(seconds(started) < 2, "closing the library ends the extractor under way");
        }

        // Oscairn's audio extractor, in the scan helper.
        const std::string audio(4000, '\x11'), other(4000, '\x22');
        write(temp.path / "a/one.wav", wav("One", audio)); write(temp.path / "a/two.wav", wav("Second title", audio)); write(temp.path / "a/three.wav", wav("One", other));
        const auto one = extract(helper, temp.path / "a/one.wav"), two = extract(helper, temp.path / "a/two.wav"), three = extract(helper, temp.path / "a/three.wav");
        check(one.ok && one.identity.size() == 64 && one.title == "One" && two.title == "Second title" && one.identity == two.identity && one.identity != three.identity,
              "WAV: retagging keeps the identity, other audio changes it: " + one.output + two.output);
        check(one.output.find("\"rate\":44100") != std::string::npos && one.output.find("\"channels\":2") != std::string::npos && one.output.find("\"duration\":0.023") != std::string::npos,
              "WAV: its rate, channels and duration: " + one.output);
        write(temp.path / "a/one.aiff", aiff("One", audio)); write(temp.path / "a/two.aiff", aiff("Two", audio));
        const auto a1 = extract(helper, temp.path / "a/one.aiff"), a2 = extract(helper, temp.path / "a/two.aiff");
        check(a1.ok && a1.identity == a2.identity && a1.title == "One" && a2.title == "Two" && a1.identity == one.identity && a1.output.find("\"rate\":44100") != std::string::npos,
              "AIFF: retagging keeps the identity: " + a1.output + a2.output);
        write(temp.path / "a/one.flac", flac("One", audio)); write(temp.path / "a/two.flac", flac("A longer title", audio));
        const auto f1 = extract(helper, temp.path / "a/one.flac"), f2 = extract(helper, temp.path / "a/two.flac");
        check(f1.ok && f1.identity == f2.identity && f1.title == "One" && f2.title == "A longer title" && f1.output.find("\"duration\":2.000") != std::string::npos &&
              f1.output.find("\"rate\":48000") != std::string::npos, "FLAC: retagging keeps the identity: " + f1.output + f2.output);
        const std::string frames = std::string("\xff\xfb\x90\x00", 4) + audio;
        write(temp.path / "a/one.mp3", mp3("One", frames, false)); write(temp.path / "a/two.mp3", mp3("Two titles", frames, true));
        const auto m1 = extract(helper, temp.path / "a/one.mp3"), m2 = extract(helper, temp.path / "a/two.mp3");
        check(m1.ok && m1.identity == m2.identity && m1.title == "One" && m2.title == "Two titles" && m1.identity == assets::sha256(frames),
              "MP3: retagging (ID3v2 and v1) keeps the identity, that of its frames: " + m1.output + m2.output);
        write(temp.path / "a/noise.wav", std::string(100, '\x7f'));
        check(!extract(helper, temp.path / "a/noise.wav").ok, "a file it cannot read fails, and says nothing on stdout");
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    if(!failures) std::puts("ok: extractors run out of process with limits; one that fails costs its file only; the audio extractor's identity survives retagging");
    return failures ? 1 : 0;
}
