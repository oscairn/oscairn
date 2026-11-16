// SPDX-License-Identifier: AGPL-3.0-or-later
//
// oscairn-plugin-check on an instrument that is silent until it has content (the sample fixture,
// whose state lists the samples it plays): without content it fails; with --state naming a sample
// and that sample as --asset it passes every step, the export carrying the sample to a fresh
// device (so the check's own "plays there" has it); a state naming a sample that was not given
// fails, saying the content made no sound; an asset that is not a sample or waveform is refused.
//   oscairn-plugin-check-content-test CHECK_TOOL FIXTURE.clap SAMPLEPACK_CLI
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
    if(failures) return 1;
    std::puts("plugin check content: ok");
    return 0;
}
