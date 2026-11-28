// SPDX-License-Identifier: AGPL-3.0-or-later
//
// issue #19: the capture buffer as a setting. capture.buffer reads the saved choice and the
// memory each discrete choice costs at the current sample rate and channels, sets and persists
// one; Off (0) turns the rings off and a capture from the past is refused with the reason it is
// off; a choice past the device's maximum is refused, and one that does not fit the memory
// budget keeps the buffer there (RFC 0025).
//   oscairn-capture-test BASIC_SYNTH.clap
#include "oscairn/session.h"
#include "oscairn/json_rpc.h"
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <unistd.h>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }

    void playable(Session& s, const char* synth)
    {
        const auto node = s.add("synth", synth);
        const auto part = s.addPart("Lead");
        s.insertIntoPart(part, 0, node);
        s.commit();
    }

    // get/set over the protocol, and the memory the answer carries at the current sample rate.
    void getSet(const char* synth)
    {
        Session s(48000, 256);
        playable(s, synth);
        JsonRpc rpc(s);
        auto call = [&](const std::string& body) { return rpc.dispatch(R"({"jsonrpc":"2.0","id":1,)" + body + "}").value_or(""); };
        const auto got = call(R"("method":"capture.buffer","params":{})");
        check(got.find(R"("seconds":30)") != std::string::npos, "the answer starts at the default 30: " + got);
        check(got.find(R"("default":30)") != std::string::npos && got.find(R"("max":120)") != std::string::npos, "the answer names the default and maximum");
        check(got.find(R"({"seconds":0,"memory":0})") != std::string::npos, "Off costs no memory");
        // Each choice carries what its rings cost at this sample rate.
        check(got.find(R"({"seconds":10,"memory":)") != std::string::npos && got.find(R"({"seconds":30,"memory":)") != std::string::npos, "each choice carries the memory its rings cost");
        const auto set = call(R"("method":"capture.buffer","params":{"seconds":60})");
        check(set.find(R"("accepted":true)") != std::string::npos && set.find(R"("seconds":60)") != std::string::npos, "set arms 60 seconds");
        check(call(R"("method":"record.get")").find(R"("retro":{"seconds":60})") != std::string::npos, "the armed buffer reads back");
        const auto odd = call(R"("method":"capture.buffer","params":{"seconds":7})");
        check(odd.find(R"("code":-32602)") != std::string::npos && odd.find(R"("reason":"invalid")") != std::string::npos, "a length that is not a choice is -32602 invalid: " + odd);
    }

    // The saved choice survives an engine restart, from the engine's config folder.
    void persisted(const char* synth)
    {
        const auto home = fs::temp_directory_path() / ("oscairn-capture-test-" + std::to_string(getpid()));
        fs::remove_all(home);
        setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
        {
            Session s(48000, 256);
            playable(s, synth);
            JsonRpc rpc(s);
            auto call = [&](const std::string& body) { return rpc.dispatch(R"({"jsonrpc":"2.0","id":1,)" + body + "}").value_or(""); };
            check(call(R"("method":"capture.buffer","params":{})").find(R"("seconds":30)") != std::string::npos, "a fresh engine answers the default before anything is saved");
            call(R"("method":"capture.buffer","params":{"seconds":60})");
        }
        Session again(48000, 256);
        playable(again, synth);
        JsonRpc rpc(again);
        const auto got = rpc.dispatch(R"({"jsonrpc":"2.0","id":1,"method":"capture.buffer","params":{}})").value_or("");
        check(got.find(R"("seconds":60)") != std::string::npos, "the choice is there again after an engine restart: " + got);
        fs::remove_all(home);
    }

    // Off turns the rings off, and a capture from the past is refused with the reason it is off.
    void offRefuses(const char* synth)
    {
        Session s(48000, 256);
        playable(s, synth);
        JsonRpc rpc(s);
        auto call = [&](const std::string& body) { return rpc.dispatch(R"({"jsonrpc":"2.0","id":1,)" + body + "}").value_or(""); };
        call(R"("method":"capture.buffer","params":{"seconds":0})");
        check(call(R"("method":"record.get")").find(R"("retro":{"seconds":0})") != std::string::npos, "Off arms no buffer");
        const auto got = call(R"("method":"record.start","params":{"from":{"secondsAgo":1},"name":"Off"})");
        check(got.find(R"("code":-32007)") != std::string::npos && got.find(R"("reason":"off")") != std::string::npos, "a capture with the buffer off is refused, and says so: " + got);
    }

    // A choice past the device's maximum is refused; the answer carries the device's default and
    // maximum and the choices stop there.
    void policyDefaults(const char* synth)
    {
        Session s(48000, 256);
        playable(s, synth);
        s.setCaptureBufferPolicy(15, 30); // the profile's own default and maximum
        JsonRpc rpc(s);
        auto call = [&](const std::string& body) { return rpc.dispatch(R"({"jsonrpc":"2.0","id":1,)" + body + "}").value_or(""); };
        const auto got = call(R"("method":"capture.buffer","params":{})");
        check(got.find(R"("default":15)") != std::string::npos && got.find(R"("max":30)") != std::string::npos, "the answer carries the device's default and maximum: " + got);
        check(got.find(R"({"seconds":60,"memory":)") == std::string::npos, "the choices stop at the maximum: " + got);
        const auto past = call(R"("method":"capture.buffer","params":{"seconds":60})");
        check(past.find(R"("code":-32602)") != std::string::npos && past.find(R"("reason":"invalid")") != std::string::npos, "past the maximum is refused: " + past);
        check(call(R"("method":"capture.buffer","params":{"seconds":30})").find(R"("accepted":true)") != std::string::npos, "a choice at the maximum is accepted");
    }

    // A length that does not fit the memory budget beside the Performance is refused, and the
    // buffer there is kept (RFC 0025).
    void fitRefuses(const char* synth)
    {
        Session s(48000, 256);
        playable(s, synth);
        s.setBudget({0.9, 1ull << 25}); // 32 MiB: 10 s fits, 120 s does not
        JsonRpc rpc(s);
        auto call = [&](const std::string& body) { return rpc.dispatch(R"({"jsonrpc":"2.0","id":1,)" + body + "}").value_or(""); };
        check(call(R"("method":"capture.buffer","params":{"seconds":10})").find(R"("accepted":true)") != std::string::npos, "10 s fits the budget");
        const auto big = call(R"("method":"capture.buffer","params":{"seconds":120})");
        check(big.find(R"("code":-32007)") != std::string::npos && big.find(R"("reason":"memory")") != std::string::npos, "a buffer past the budget is -32007 memory: " + big);
        check(call(R"("method":"record.get")").find(R"("retro":{"seconds":10})") != std::string::npos, "the buffer that fits is kept");
    }
}

int main(int argc, char** argv)
{
    if(argc < 2) return 2;
    const auto home = fs::temp_directory_path() / ("oscairn-capture-test-" + std::to_string(getpid()));
    fs::remove_all(home);
    setenv("OSCAIRN_DATA_HOME", home.c_str(), 1);
    try
    {
        getSet(argv[1]);
        persisted(argv[1]);
        offRefuses(argv[1]);
        policyDefaults(argv[1]);
        fitRefuses(argv[1]);
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    fs::remove_all(home);
    if(failures) return 1;
    std::puts("capture: ok");
    return 0;
}
