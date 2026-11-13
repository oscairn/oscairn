// SPDX-License-Identifier: AGPL-3.0-or-later
// A hostile extractor for control-extractors (RFC 0023 step 4), run as the library runs the scan
// helper's: --extract <extractor> <file>. What it does depends on the file's name; each run is
// noted in OSCAIRN_FIXTURE_LOG when that is set.
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <memory>
#include <string>
#include <thread>
#include <vector>

int main(int argc, char** argv)
{
    if(argc != 4 || std::string(argv[1]) != "--extract") return 2;
    const auto name = std::filesystem::path(argv[3]).stem().string();
    if(const char* log = std::getenv("OSCAIRN_FIXTURE_LOG")) std::ofstream(log, std::ios::app) << name << '\n';
    auto say = [](const std::string& s) { std::fwrite(s.data(), 1, s.size(), stdout); return 0; };
    const std::string id(64, 'b');
    if(name.starts_with("hang")) for(;;) std::this_thread::sleep_for(std::chrono::hours(1));
    if(name == "crash") { std::fputs("fixture: crashing on purpose\n", stderr); std::fflush(stderr); std::abort(); }
    if(name == "flood") { const std::string block(1 << 16, 'x'); for(;;) std::fwrite(block.data(), 1, block.size(), stdout); }
    if(name == "hog")
    {
        // At most 1 GiB, touched, then a wait the timeout ends: the memory limit should end it first.
        std::vector<std::unique_ptr<char[]>> blocks;
        for(int i = 0; i < 64; ++i) { blocks.emplace_back(new char[16 << 20]); std::memset(blocks.back().get(), 1, 16 << 20); std::this_thread::sleep_for(std::chrono::milliseconds(5)); }
        for(;;) std::this_thread::sleep_for(std::chrono::hours(1));
    }
    if(name == "lie-json") return say("this is not JSON");
    if(name == "lie-utf8") return say("{\"identity\":\"" + id + "\",\"fields\":{\"artist\":\"\xff\xfe\"}}");
    if(name == "lie-escape") return say("{\"identity\":\"" + id + "\",\"fields\":{\"artist\":\"a\\u0001b\"}}");
    if(name == "lie-identity") return say("{\"identity\":\"not a hash\",\"fields\":{\"artist\":\"Fixture\"}}");
    if(name == "lie-none") return say("{\"fields\":{\"artist\":\"Fixture\"}}");
    if(name == "lie-type") return say("{\"identity\":\"" + id + "\",\"fields\":{\"artist\":{\"nested\":1}}}");
    if(name == "lie-long") return say("{\"identity\":\"" + id + "\",\"fields\":{\"artist\":\"" + std::string(300, 'a') + "\"}}");
    if(name == "lie-twice") return say("{\"identity\":\"" + id + "\",\"fields\":{\"artist\":\"A\",\"artist\":\"B\"}}");
    if(name == "fail") { std::fputs("fixture: no such format\n", stderr); return 1; }
    // good-<n>: an identity of its own, its fields, and one its type does not declare.
    return say("{\"identity\":\"" + std::string(63, 'a') + name.back() + "\",\"fields\":{\"artist\":\"Fixture \\u00e9\",\"bpm\":120,\"undeclared\":\"x\"}}\n");
}
