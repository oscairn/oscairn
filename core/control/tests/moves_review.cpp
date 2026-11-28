// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The independent review's cases for RFC 0043, each a finding the stack had to fix (from branch
// moves-review, 91b30e50):
//  - D2: another key's archive of the same version must not replace the publisher's recorded claim,
//    and plugins.list never shows a claim key that signed no build in the store.
//   oscairn-moves-review-test FIXTURE-V1 FIXTURE-V2 OSCAIRN-SIGN
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "../../plugin-host/src/StateFiles.h"
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    constexpr double rate = 48000; constexpr uint32_t frames = 128;
    const std::string id = "org.kick.test.synth";
    std::string sign;
    fs::path temp;

    std::string shell(const fs::path& p) { return "'" + p.string() + "'"; }
    int run(const std::string& command) { return std::system((command + " >/dev/null 2>&1").c_str()); }
    void home(const std::string& name) { fs::create_directories(temp / name); ::setenv("OSCAIRN_DATA_HOME", (temp / name).c_str(), 1); }
    // A build of the synth at `version`, signed, packed with or without the claim to open version 1's state, installed.
    void install(Session& s, const char* fixture, const std::string& version, bool claim)
    {
        const auto folder = temp / ("build-" + version + "-" + std::to_string(std::rand()));
        fs::create_directories(folder);
        const auto build = folder / "Synth.clap";
        fs::copy_file(fixture, build);
        check(run(sign + " build " + shell(temp / "author.key") + " clap " + id + " " + version + " " + platform() + " " + shell(build)) == 0, "the build is signed");
        const auto archive = folder / "Synth.olib";
        check(run(sign + " plugin-package --license MIT --key " + shell(temp / "author.key") + (claim ? " --opens-state-from " + id + " 1" : std::string()) + " " + shell(archive) +
                  " clap " + id + " " + version + " Synth " + platform() + "=" + shell(build)) == 0, "the package is packed");
        s.installArchive(archive.string(), false, {}, true);
    }
}

int main(int argc, char** argv)
{
    if(argc < 4) return 2;
    const char *fixture = argv[1], *v2 = argv[2];
    sign = shell(argv[3]);
    state_files::TemporaryDirectory scratch;
    temp = scratch.path;
    check(run(sign + " key " + shell(temp / "author.key")) == 0, "a key is made");
    check(run(sign + " key " + shell(temp / "other.key")) == 0, "another key is made");

    // D2: another key's archive with the same build bytes must not replace the publisher's claim, as that key never signed the installed build.
    {
        home("b");
        Session s(rate, frames);
        install(s, fixture, "1", false);
        install(s, v2, "2", true);
        const auto folder = temp / "other-build";
        fs::create_directories(folder);
        fs::copy_file(v2, folder / "Synth.clap");
        check(run(sign + " build " + shell(temp / "other.key") + " clap " + id + " 2 " + platform() + " " + shell(folder / "Synth.clap")) == 0, "signed by the other key");
        check(run(sign + " plugin-package --license MIT --key " + shell(temp / "other.key") + " --opens-state-from " + id + " 0 " + shell(folder / "O.olib") + " clap " + id + " 2 Synth " + platform() + "=" + shell(folder / "Synth.clap")) == 0, "packed");
        s.installArchive((folder / "O.olib").string(), false, {}, true);
        const auto p = s.installedPlugins().at(1);
        check(p.opensStateFrom == "1", "another publisher's archive must not replace the recorded claim: now opensStateFrom=" + p.opensStateFrom);
        // A claim whose key signed no stored build (plugin.json edited, or its builds replaced since) is shown as none.
        const auto json = fs::path(std::getenv("OSCAIRN_DATA_HOME")) / "Plugins" / "clap" / (id + "@2") / "plugin.json";
        std::ofstream(json) << R"({"format":"clap","id":")" << id << R"(","version":"2","name":"Synth","license":"MIT","opensStateFrom":"1","claimKey":")" << std::string(64, 'a') << "\"}\n";
        const auto shown = s.installedPlugins().at(1);
        check(shown.opensStateFrom.empty() && shown.claimKey.empty(), "plugins.list shows no claim under a key that signed no stored build: " + shown.claimKey);
    }
    return failures ? 1 : 0;
}
