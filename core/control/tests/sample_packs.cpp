// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Samples and waveforms as item types kept by handlers (RFC 0023, docs/library-archive.md), across
// devices (each its own OSCAIRN_DATA_HOME). The types package installs; an archive of samples made
// from synthetic audio and a waveform naming two of them installs in its order; installing again
// adds nothing; a second archive sharing ids adds only the new one; a damaged sample, a waveform
// whose id is not its name and one that fails its check are refused with nothing kept; a test
// instrument resolves each sample with resolve_sample and decodes its first block to the source's
// audio; a Sound's export carries exactly the samples and waveform it uses, which a device with an
// empty store installs and plays the same way.
//   oscairn-sample-packs-test FIXTURE.clap SAMPLEPACK_CLI TYPES_FOLDER
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "../../plugin-host/src/StateFiles.h"
#include "../../engine/tests/sample_source.h"
#include <cinttypes>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <set>
#include <sstream>
#include <unistd.h>

using namespace oscairn;
namespace fs = std::filesystem;
namespace ss = sample_source;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    std::string read(const fs::path& f) { std::ifstream in(f, std::ios::binary); std::ostringstream out; out << in.rdbuf(); return out.str(); }
    void device(const fs::path& home) { fs::create_directories(home); ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1); }
    std::string refusal(const std::function<void()>& f) { try { f(); } catch(const std::exception& e) { return e.what(); } return {}; }
    size_t held(Session& s) { size_t n = 0; for(const auto& p : s.assets().samples().packs()) n += p.samples; return n; }
    uint64_t bytes(Session& s) { uint64_t n = 0; for(const auto& p : s.assets().samples().packs()) n += p.bytes; return n; }
    // An archive as a converter writes one: the types it uses, its samples and waveforms in the order given.
    void archive(const fs::path& file, const fs::path& types, const std::vector<fs::path>& files)
    {
        const auto stage = file.parent_path() / (file.stem().string() + ".stage");
        fs::create_directories(stage / "Types");
        fs::copy(types, stage / "Types" / "org.oscairn.samplepack", fs::copy_options::recursive);
        Package p; p.id = file.stem().string(); p.name = p.id; p.kind = "library";
        for(const auto& f : files)
        {
            const auto name = std::string(f.extension() == ".osp" ? "Samples/" : "Waveforms/") + f.filename().string();
            fs::create_directories((stage / name).parent_path()); fs::copy_file(f, stage / name);
            p.contents.push_back(name);
        }
        writePackage(file, p, stage);
    }
    // What the fixture says it decoded, by sample id: "<frames> <hash>".
    std::map<std::string, std::string> decoded(const fs::path& report)
    {
        std::map<std::string, std::string> out; std::istringstream in(read(report)); std::string id, rest;
        while(in >> id && std::getline(in, rest)) out[id] = rest.substr(1);
        return out;
    }
    std::string expected(const ss::Sample& s)
    {
        const auto block = ss::channel0(s, 1024); // samplepack pack writes blocks of 1,024 frames
        uint64_t h = 1469598103934665603ull;
        for(const auto v : block) for(int b = 0; b < 2; ++b) { h ^= uint8_t(uint16_t(v) >> (8 * b)); h *= 1099511628211ull; }
        char text[64]; std::snprintf(text, sizeof text, "%zu %016" PRIx64, block.size(), h);
        return text;
    }
}

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: %s FIXTURE SAMPLEPACK_CLI TYPES_FOLDER\n", argv[0]); return 2; }
    const std::string fixture = argv[1], cli = argv[2];
    const fs::path types = argv[3];
    try
    {
        state_files::TemporaryDirectory temp;
        const auto in = temp.path / "in", report = temp.path / "report.txt";
        ::setenv("OSCAIRN_SAMPLE_FIXTURE_REPORT", report.c_str(), 1);
        std::vector<ss::Sample> s;
        for(uint32_t i = 0; i < 7; ++i) s.push_back(ss::pack(cli, in, i + 1, 2500 + 900 * i, i == 0 ? 2 : 1));
        std::string wave;
        const auto waveFile = ss::waveform(in, {s[2].id, s[3].id}, &wave);

        // The types package: a library declaring the two types, no items.
        const auto typesArchive = temp.path / "types.olib";
        {
            const auto stage = temp.path / "types.stage";
            fs::create_directories(stage / "Types"); fs::copy(types, stage / "Types" / "org.oscairn.samplepack", fs::copy_options::recursive);
            Package p; p.id = "org.oscairn.samplepack"; p.name = "Sample pack types"; p.kind = "library";
            writePackage(typesArchive, p, stage);
        }
        const auto content = temp.path / "content.olib", more = temp.path / "more.olib";
        archive(content, types, {s[1].pack, s[0].pack, waveFile, s[2].pack, s[3].pack});
        archive(more, types, {s[2].pack, s[4].pack});
        const auto export_ = temp.path / "export.olib";

        // A device that knows neither type refuses an archive of samples that does not declare them.
        {
            device(temp.path / "z");
            Session z(48000, 64);
            // As an unpacked package folder: writePackage packs only the folders of types it knows.
            const auto bare = temp.path / "bare";
            const auto name = "Samples/" + s[0].pack.filename().string();
            fs::create_directories(bare / "Samples"); fs::copy_file(s[0].pack, bare / name);
            Package p; p.id = "bare"; p.name = "bare"; p.kind = "library"; p.files[name] = assets::sha256File(bare / name);
            std::ofstream(bare / "library.json", std::ios::binary) << manifestJson(p);
            const auto why = refusal([&] { z.installArchive(bare.string(), false); });
            check(why.find("outside its folders") != std::string::npos, "samples of an unknown type are refused: " + why);
        }
        std::string sound;
        {
            device(temp.path / "a");
            Session a(48000, 64);
            a.installArchive(typesArchive.string(), true);
            check(itemType("org.oscairn.samplepack.sample") && handlerType("waveform"), "the types package installs both types");
            a.installArchive(content.string(), false);
            check(held(a) == 4, "four samples installed");
            uint64_t last = 0; bool ordered = true;
            for(const auto* x : {&s[1], &s[0], &s[2], &s[3]}) { const auto at = a.assets().samples().find(x->id); ordered = ordered && at && at->offset > last; last = at ? at->offset : last; }
            check(ordered, "in the archive's order");
            check(fs::exists(a.assets().waveforms().path(wave)), "the waveform installed");
            const auto size = bytes(a);
            a.installArchive(content.string(), false);
            check(held(a) == 4 && bytes(a) == size, "installing again adds nothing");
            a.installArchive(more.string(), false);
            check(held(a) == 5 && a.assets().samples().find(s[4].id), "a second archive adds only its new sample");

            // Refused, naming what, and nothing kept.
            {
                auto damaged = temp.path / "damaged" / s[5].pack.filename();
                fs::create_directories(damaged.parent_path()); fs::copy_file(s[5].pack, damaged);
                { std::fstream f(damaged, std::ios::in | std::ios::out | std::ios::binary); f.seekp(2 * 4096 + 100); f.put('\x33'); f.put('\x44'); }
                const auto bad = temp.path / "bad.olib";
                archive(bad, types, {s[6].pack, damaged});
                const auto before = bytes(a);
                const auto why = refusal([&] { a.installArchive(bad.string(), false); });
                check(why.find(s[5].id) != std::string::npos && why.find("refused") != std::string::npos, "a damaged sample is refused by name: " + why);
                check(bytes(a) == before && !a.assets().samples().find(s[6].id), "nothing of it kept");
                auto misnamed = temp.path / "misnamed" / (std::string(64, 'c') + ".owf");
                fs::create_directories(misnamed.parent_path()); fs::copy_file(waveFile, misnamed);
                const auto wrongName = temp.path / "wrong-name.olib";
                archive(wrongName, types, {s[6].pack, misnamed});
                check(refusal([&] { a.installArchive(wrongName.string(), false); }).find("is not its name") != std::string::npos && !a.assets().samples().find(s[6].id), "a waveform whose id is not its name is refused, nothing kept");
                auto broken = temp.path / "broken" / waveFile.filename();
                fs::create_directories(broken.parent_path()); fs::copy_file(waveFile, broken);
                { std::fstream f(broken, std::ios::in | std::ios::out | std::ios::binary); f.seekp(24); f.put('\x01'); }
                const auto brokenArchive = temp.path / "broken.olib";
                archive(brokenArchive, types, {broken});
                check(refusal([&] { a.installArchive(brokenArchive.string(), false); }).find("not a valid waveform") != std::string::npos, "a waveform that fails its check is refused");
            }

            // A Sound of the instrument, which loads it as a sampler would: two samples and the waveform's two.
            ::setenv("OSCAIRN_SAMPLE_FIXTURE_REFS", ("sample:" + s[0].id + ",sample:" + s[1].id + ",waveform:" + wave).c_str(), 1);
            const auto node = a.add("sampler", fixture, "org.oscairn.test.sample-fixture");
            ::unsetenv("OSCAIRN_SAMPLE_FIXTURE_REFS");
            const auto part = a.addPart("Keys"); a.insertIntoPart(part, 0, node); a.commit();
            sound = a.savePartAsSound(part, "Uses samples").id;
            check(!sound.empty(), "the Sound is saved");
            fs::remove(report);
            const auto other = a.addPart("More keys"); a.commit();
            a.loadSound(other, sound);
            auto got = decoded(report);
            check(got.size() == 5, "the instrument decoded four samples: " + read(report));
            check((got["pid"] != std::to_string(::getpid())) == (std::getenv("OSCAIRN_PLUGIN_WORKER") != nullptr), "it ran in a worker when the test says so, else in the engine's process");
            for(const auto* x : {&s[0], &s[1], &s[2], &s[3]}) check(got[x->id] == expected(*x), "sample " + x->id.substr(0, 8) + " decodes to its audio: " + got[x->id] + " for " + expected(*x));
            a.exportLibraryItem(sound, export_.string());
            const auto p = readPackage(export_, {});
            const std::vector<std::string> carried{"Samples/" + s[0].id + ".osp", "Samples/" + s[1].id + ".osp", "Waveforms/" + wave + ".owf", "Samples/" + s[2].id + ".osp", "Samples/" + s[3].id + ".osp"};
            check(p.contents == carried, "the export carries exactly the samples and waveform the Sound uses, in its order");
        }
        // A device with an empty store: the export installs, the types with it, and the Sound plays its samples.
        {
            device(temp.path / "b");
            Session b(48000, 64);
            b.installArchive(export_.string(), false, {}, true); // the fixture is unsigned
            check(held(b) == 4 && fs::exists(b.assets().waveforms().path(wave)), "the export installs its four samples and the waveform");
            Control::LibraryQuery q; q.kind = "sound";
            const auto sounds = b.libraryQuery(q).items;
            check(sounds.size() == 1, "its Sound is in the library");
            fs::remove(report);
            if(!sounds.empty())
            {
                const auto part = b.addPart("Keys"); b.commit();
                b.loadSound(part, sounds[0].id);
                auto got = decoded(report);
                for(const auto* x : {&s[0], &s[1], &s[2], &s[3]}) check(got[x->id] == expected(*x), "on the other device, sample " + x->id.substr(0, 8) + " decodes to its audio");
            }
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::puts("sample packs: ok");
    return 0;
}
