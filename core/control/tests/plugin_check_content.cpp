// SPDX-License-Identifier: AGPL-3.0-or-later
//
// oscairn-plugin-check on an instrument that is silent until it has content (the sample fixture,
// whose state lists the samples it plays): without content it fails; with --state naming a sample
// and that sample as --asset it passes every step, the export carrying the sample to a fresh
// device (so the check's own "plays there" has it); a state naming a sample that was not given
// fails, saying the content made no sound; an asset that is not a sample or waveform is refused.
// --library and --sound: a Sound of the fixture, exported with its sample and the types, installed as a
// library; the check installs the plugin as a package, opens the Sound in a part and plays it through
// the save, the export and the fresh device.
//   oscairn-plugin-check-content-test CHECK_TOOL FIXTURE.clap SAMPLEPACK_CLI
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "../../engine/tests/sample_source.h"
#include "../../plugin-host/src/StateFiles.h"
#include <cstdio>
#include <sstream>

namespace fs = std::filesystem;
namespace ss = sample_source;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    // The tool's exit status and output.
    int run(const std::string& command, std::string& output, const fs::path& log)
    {
        const int status = std::system((command + " > \"" + log.string() + "\" 2>&1").c_str());
        std::ifstream in(log); std::ostringstream text; text << in.rdbuf(); output = text.str();
        return status;
    }
}

int main(int argc, char** argv)
{
    if(argc != 4) { std::fprintf(stderr, "usage: %s CHECK_TOOL FIXTURE SAMPLEPACK_CLI\n", argv[0]); return 2; }
    const std::string tool = "\"" + std::string(argv[1]) + "\"", fixture = "\"" + std::string(argv[2]) + "\" org.oscairn.test.sample-fixture synth";
    oscairn::state_files::TemporaryDirectory temp;
    const auto sample = ss::pack(argv[3], temp.path / "in", 1, 6000);
    const auto state = temp.path / "state", missing = temp.path / "missing";
    std::ofstream(state) << "sample:" << sample.id << "\n";
    std::ofstream(missing) << "sample:" << std::string(64, 'a') << "\n";
    const auto log = temp.path / "log.txt";
    const auto asset = " --asset \"" + sample.id + "=" + sample.pack.string() + "\"";
    std::string out;

    check(run(tool + " " + fixture, out, log) != 0 && out.find("FAIL: plays: rms 0.000000") != std::string::npos, "silent with no content: fails as before\n" + out);

    check(run(tool + " " + fixture + " --state \"" + state.string() + "\"" + asset, out, log) == 0 && out.find("PASSED") != std::string::npos
          && out.find("ok: plays: rms") != std::string::npos && out.find("ok: plays as saved: rms") != std::string::npos && out.find("ok: plays there: rms") != std::string::npos
          && out.find("ok: opens from the plugin store") != std::string::npos, "state and asset: plays, as saved and from the store\n" + out);

    check(run(tool + " " + fixture + " --state \"" + state.string() + "\"", out, log) != 0 && out.find("did not produce sound") != std::string::npos, "a state naming a sample that is not installed fails\n" + out);
    check(run(tool + " " + fixture + " --state \"" + missing.string() + "\"" + asset, out, log) != 0 && out.find("the content (state and assets) did not produce sound") != std::string::npos, "a state naming another sample fails, saying the content made no sound\n" + out);

    const auto text = temp.path / "notes.txt";
    std::ofstream(text) << "x";
    check(run(tool + " " + fixture + " --asset notes=\"" + text.string() + "\"", out, log) != 0 && out.find("only samples (.osp) and waveforms (.owf)") != std::string::npos, "another kind of file is refused, saying so\n" + out);

    // A Sound of the fixture made on a device of its own: the samples as a sample package installs them, the fixture
    // loaded with a state naming the sample (OSCAIRN_SAMPLE_FIXTURE_REFS), the part saved as a Sound and exported.
    using namespace oscairn;
    const auto soundArchive = temp.path / "sound.olib";
    std::string soundId;
    {
        const auto stage = temp.path / "stage";
        fs::create_directories(stage / "Types");
        fs::copy(OSCAIRN_SAMPLE_TYPES_DIR, stage / "Types" / "org.oscairn.sample-types", fs::copy_options::recursive);
        const auto entry = "Samples/" + sample.pack.filename().string();
        fs::create_directories(stage / "Samples"); fs::copy_file(sample.pack, stage / entry);
        Package p; p.id = "samples"; p.name = "samples"; p.kind = "library"; p.contents.push_back(entry);
        writePackage(temp.path / "samples.olib", p, stage);
        fs::create_directories(temp.path / "mk");
        ::setenv("OSCAIRN_DATA_HOME", (temp.path / "mk").c_str(), 1);
        Session mk(48000, 256);
        mk.installArchive((temp.path / "samples.olib").string(), false, {}, true);
        ::setenv("OSCAIRN_SAMPLE_FIXTURE_REFS", ("sample:" + sample.id).c_str(), 1);
        const auto node = mk.add("plugin", argv[2], "org.oscairn.test.sample-fixture");
        ::unsetenv("OSCAIRN_SAMPLE_FIXTURE_REFS");
        const auto part = mk.addPart("Keys"); mk.insertIntoPart(part, 0, node); mk.commit();
        soundId = mk.savePartAsSound(part, "Init").id;
        mk.exportLibraryItem(soundId, soundArchive.string());
    }
    check(!soundId.empty(), "a Sound of the fixture is made");
    check(run(tool + " " + fixture + " --library \"" + soundArchive.string() + "\" --sound " + soundId, out, log) == 0 && out.find("PASSED") != std::string::npos
          && out.find("ok: the plugin installs in the store as a package") != std::string::npos && out.find("ok: the library installs") != std::string::npos
          && out.find("ok: plays: rms") != std::string::npos && out.find("ok: plays as saved: rms") != std::string::npos && out.find("ok: plays there: rms") != std::string::npos,
          "--library and --sound: the Sound plays, as saved and from a fresh store\n" + out);
    check(run(tool + " " + fixture + " --library \"" + soundArchive.string() + "\" --sound 0000000000000000", out, log) != 0 && out.find("no such Sound") != std::string::npos, "a Sound the library lacks is refused\n" + out);
    if(failures) return 1;
    std::puts("plugin check content: ok");
    return 0;
}
