// SPDX-License-Identifier: AGPL-3.0-or-later
// The sample store (samples.h): one-record packs made from synthetic audio are verified and
// appended into the store's own packs, found by id and decoded back to their audio; installing
// again adds nothing, a second set sharing ids adds only the new ones; a damaged record is
// refused with nothing written; a process killed in the middle of an append leaves the earlier
// records whole and its own cut away at the next start; an export opens as a one-record pack of
// the same bytes. Waveforms are checked and named by their id.
//   oscairn-sample-store-test SAMPLEPACK_CLI
#include "oscairn/assets.h"
#include "../../plugin-host/src/StateFiles.h"
#include "sample_source.h"
#include <cstdio>
#include <functional>
#include <iostream>
#include <sys/wait.h>
#include <unistd.h>

using namespace oscairn;
namespace fs = std::filesystem;
namespace ss = sample_source;

static int failures = 0;
static void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
static std::string refusal(const std::function<void()>& f) { try { f(); } catch(const std::exception& e) { return e.what(); } return {}; }
static bool decodes(assets::SamplePacks& packs, const ss::Sample& s)
{
    const auto at = packs.find(s.id);
    if(!at) return false;
    const auto block = ss::firstBlock(at->pack, at->offset, at->length, at->preload, s.id);
    return !block.empty() && block == ss::channel0(s, block.size());
}
static size_t held(assets::SamplePacks& packs) { size_t n = 0; for(const auto& p : packs.packs()) n += p.samples; return n; }

int main(int argc, char** argv)
{
    if(argc != 2) { std::fprintf(stderr, "usage: %s SAMPLEPACK_CLI\n", argv[0]); return 2; }
    try
    {
        state_files::TemporaryDirectory temp;
        const auto in = temp.path / "in", store = temp.path / "samples";
        std::vector<ss::Sample> first;
        for(uint32_t i = 0; i < 4; ++i) first.push_back(ss::pack(argv[1], in, i + 1, 3000 + 1500 * i, i % 2 + 1, i == 3 ? 48000 : 44100));
        auto files = [](const std::vector<ss::Sample>& list) { std::vector<fs::path> out; for(const auto& s : list) out.push_back(s.pack); return out; };
        {
            assets::SamplePacks packs(store);
            check(packs.install(files(first)) == 4, "four samples installed");
            bool all = true;
            for(const auto& s : first) all = all && decodes(packs, s);
            check(all, "each sample is found and decodes to its audio");
            check(packs.install(files(first)) == 0, "installing again adds nothing");
            check(packs.packs().size() == 1 && held(packs) == 4, "one pack of four");
            // The records sit in the order given.
            uint64_t last = 0; bool ordered = true;
            for(const auto& s : first) { const auto at = packs.find(s.id); ordered = ordered && at->offset > last; last = at->offset; }
            check(ordered, "records in the order installed");
            check(!packs.find(std::string(64, '0')), "an unknown id is not found");
        }
        // A second set sharing two ids adds only the new ones, in a fresh index built from the trailer.
        std::vector<ss::Sample> second{first[1], first[2], ss::pack(argv[1], in, 10), ss::pack(argv[1], in, 11)};
        {
            assets::SamplePacks packs(store);
            check(packs.install(files(second)) == 2, "only the two new samples added");
            check(held(packs) == 6 && decodes(packs, second[2]) && decodes(packs, first[0]), "six held, old and new decode");
        }
        // A damaged record: refused, naming it, and nothing written.
        {
            const auto bad = ss::pack(argv[1], temp.path / "bad", 20), good = ss::pack(argv[1], temp.path / "bad", 21);
            std::fstream f(bad.pack, std::ios::in | std::ios::out | std::ios::binary);
            f.seekp(2 * 4096 + 300); f.put('\x5a'); f.put('\xa5'); f.close(); // inside the window or the frames
            const auto before = fs::file_size(store / "pack-000001.osp");
            assets::SamplePacks packs(store);
            const auto why = refusal([&] { packs.install({good.pack, bad.pack}); });
            check(why.find(bad.id) != std::string::npos && why.find("verification") != std::string::npos, "the damaged sample is refused by name: " + why);
            check(fs::file_size(store / "pack-000001.osp") == before && !packs.find(good.id), "nothing of that install was written");
            const auto renamed = temp.path / "bad" / (std::string(64, 'a') + ".osp");
            fs::copy_file(good.pack, renamed);
            check(refusal([&] { packs.install({renamed}); }).find("is not its name") != std::string::npos, "a sample whose name is not its id is refused");
        }
        // A process killed in the middle of an append, after its records and after half its trailer.
        size_t count = 6;
        for(const char* point : {"records", "trailer"})
        {
            const auto extra = ss::pack(argv[1], temp.path / point, point[0] == 'r' ? 30 : 31);
            const auto before = fs::file_size(store / "pack-000001.osp");
            const pid_t child = fork();
            if(child == 0)
            {
                ::setenv("OSCAIRN_TEST_KILL_SAMPLE_APPEND", point, 1);
                assets::SamplePacks packs(store);
                packs.install({extra.pack});
                _exit(0);
            }
            int status = 0; waitpid(child, &status, 0);
            check(WIFSIGNALED(status) && WTERMSIG(status) == SIGKILL, std::string("the append was killed after its ") + point);
            check(fs::file_size(store / "pack-000001.osp") > before, std::string("it left a torn tail after its ") + point);
            assets::SamplePacks packs(store);
            check(!packs.find(extra.id), std::string("the killed append's sample is not there after its ") + point);
            check(fs::file_size(store / "pack-000001.osp") == before, "the torn tail is cut back to the last commit");
            bool all = true; for(const auto& s : first) all = all && decodes(packs, s);
            check(all && held(packs) == count, "the earlier records are whole");
            check(packs.install({extra.pack}) == 1 && decodes(packs, extra), "and the sample installs after that");
            ++count;
        }
        // An export is a one-record pack holding the record byte for byte, which installs elsewhere.
        {
            assets::SamplePacks packs(store), elsewhere(temp.path / "elsewhere");
            const auto out = temp.path / "export" / (first[3].id + ".osp");
            fs::create_directories(out.parent_path());
            packs.exportSample(first[3].id, out);
            check(elsewhere.install({out}) == 1 && decodes(elsewhere, first[3]), "an exported sample installs and decodes elsewhere");
        }
        // Waveforms: checked, named by their id, their samples listed.
        {
            assets::Waveforms waves(temp.path / "waveforms");
            std::string id;
            const auto file = ss::waveform(temp.path / "wf", {first[0].id, first[1].id}, &id);
            check(waves.install({file}) == 1 && waves.install({file}) == 0, "a waveform installs once");
            check(waves.path(id) == temp.path / "waveforms" / (id + ".owf") && waves.samples(id) == std::vector<std::string>{first[0].id, first[1].id}, "its file and its samples");
            const auto misnamed = temp.path / "wf" / (std::string(64, 'b') + ".owf");
            fs::copy_file(file, misnamed);
            check(refusal([&] { waves.install({misnamed}); }).find("is not its name") != std::string::npos, "a waveform whose id is not its name is refused");
            const auto broken = temp.path / "wf2" / (id + ".owf");
            fs::create_directories(broken.parent_path()); fs::copy_file(file, broken);
            { std::fstream f(broken, std::ios::in | std::ios::out | std::ios::binary); f.seekp(22); f.put('\x7f'); }
            check(refusal([&] { waves.install({broken}); }).find("not a valid waveform") != std::string::npos, "a waveform that fails its check is refused");
        }
        std::printf("%d samples' index entries take %zu bytes\n", 100000, size_t(100000) * sizeof(sp_index_entry));
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); return 1; }
    if(failures) return 1;
    std::puts("sample store: ok");
    return 0;
}
