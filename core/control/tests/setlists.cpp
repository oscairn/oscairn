// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Setlists as library items (RFC 0023, docs/setlist-format.md), across devices (each its own
// OSCAIRN_DATA_HOME). A two-page setlist with an empty slot, labeled slots and a placeholder,
// whose Performances use a sample and a plugin, is saved and read back through the library,
// and exported with its Performances and everything they need. Installed into the user's own
// storage its slots point at the Performances' copies; installed as a library every ID is kept.
// An archive whose setlist names a Performance it does not hold is refused, but for
// placeholders; one written by hand as the format says installs. A library export carries the
// setlists chosen and no others. The Setlist app's version 1 files are converted at startup.
//   oscairn-setlists-test FIXTURE.clap
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "oscairn/setlist.h"
#include "oscairn/wav_file.h"
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <set>
#include <sstream>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    std::string read(const fs::path& f) { std::ifstream in(f, std::ios::binary); std::ostringstream out; out << in.rdbuf(); return out.str(); }
    void write(const fs::path& f, const std::string& text) { fs::create_directories(f.parent_path()); std::ofstream(f, std::ios::binary | std::ios::trunc) << text; }
    void device(const fs::path& home) { fs::create_directories(home); ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1); }
    std::vector<Control::LibraryItem> ofType(Session& s, const std::string& type) { Control::LibraryQuery q; q.types = {type}; return s.libraryQuery(q).items; }
    const Control::LibraryItem* named(const std::vector<Control::LibraryItem>& items, const std::string& name)
    {
        for(const auto& i : items) if(i.name == name) return &i;
        return nullptr;
    }
    Control::SetlistSlot slot(const std::string& id, const std::string& name, const std::string& color = {}, const std::string& label = {}) { return {id, name, color, label}; }
    bool refused(const std::function<void()>& f) { try { f(); return false; } catch(const std::exception&) { return true; } }
}

int main(int argc, char** argv)
{
    if(argc < 2) { std::fprintf(stderr, "usage: %s FIXTURE\n", argv[0]); return 2; }
    const char* fixture = argv[1];
    try
    {
        state_files::TemporaryDirectory temp;
        const auto friday = temp.path / "Friday.olib", whole = temp.path / "Library.olib";
        std::string bass, strings, extra, fridayId, sundayId;
        std::string sampleSha;
        // Device A: Performances using a plugin and a sample, two setlists, the exports.
        {
            device(temp.path / "a");
            Session a(48000, 64);
            const auto synth = a.add("synth", fixture, "org.kick.test.synth");
            const auto part = a.addPart("Bass"); a.insertIntoPart(part, 0, synth); a.commit();
            bass = a.savePerformanceToLibrary("Bass Split");
            // The fixture uses no samples, so the Performance's document is given one, as a sampler's lists it.
            const auto wav = temp.path / "bass.wav"; writeWavFile(wav, std::vector<float>(480, .5f), 1, 48000);
            const auto sample = a.importAsset(wav.string());
            sampleSha = sample.sha256;
            Control::LibraryQuery one; one.id = bass;
            const auto doc = fs::path(a.libraryQuery(one).items.at(0).path) / "performance.json";
            auto text = read(doc);
            const auto at = text.find("\"assets\":[]"); check(at != std::string::npos, "the Performance's plugin lists its samples");
            text.replace(at, 11, "\"assets\":[{\"library\":\"user\",\"path\":\"" + sample.path + "\",\"sha256\":\"" + sample.sha256 + "\"}]");
            write(doc, text);
            a.libraryRescan();
            strings = a.savePerformanceToLibrary("Strings Pad", true);
            extra = a.savePerformanceToLibrary("Extra", true);
            check(!bass.empty() && !strings.empty() && !extra.empty() && bass != strings && strings != extra, "three Performances");

            // Saved and read back through the library: two pages, an empty slot, labels, a placeholder.
            Control::Setlist s;
            s.info.name = "Friday"; s.info.tags = {"live"}; s.mode = "grid"; s.columns = 4; s.rows = 3;
            s.pages = {{"Set 1", {slot(bass, "Bass Split", "#e5484d"), {{}, {}, {}, {}, true}, slot(strings, "Strings Pad", "", "Opener")}},
                       {"Encore", {slot("", "Piano Ballad (CP80)"), slot(bass, "Bass Split", "", "Bass again")}}};
            fridayId = a.saveSetlist(s);
            const auto back = a.setlist(fridayId);
            check(back.info.id == fridayId && back.info.name == "Friday" && back.info.tags == std::vector<std::string>{"live"} && back.mode == "grid" && back.columns == 4 && back.rows == 3 &&
                  back.pages.size() == 2 && back.pages[0].name == "Set 1" && back.pages[0].slots.size() == 3 && back.pages[0].slots[0].id == bass &&
                  back.pages[0].slots[0].color == "#e5484d" && back.pages[0].slots[1].empty && back.pages[0].slots[2].label == "Opener" &&
                  back.pages[1].slots[0].id.empty() && back.pages[1].slots[0].name == "Piano Ballad (CP80)" && back.pages[1].slots[1].label == "Bass again",
                  "a setlist saved and read back as it was");
            const auto listed = ofType(a, "oscairn.setlist");
            check(listed.size() == 1 && listed[0].id == fridayId && listed[0].kind == "setlist" && !listed[0].revision.empty() && !listed[0].hash.empty() &&
                  fs::exists(fs::path(listed[0].path) / "setlist.json") && fs::path(listed[0].path).parent_path().filename() == "Setlists" && fs::path(listed[0].path).extension() == ".osetlist",
                  "it is a library item in Setlists/, with a revision and a content hash");
            check(ofType(a, "oscairn.performance").size() == 3, "a query by type lists that type only");
            // Saving again: the next revision of the same item, the one before as its parent.
            auto again = back; again.pages[0].name = "First set";
            check(a.saveSetlist(again) == fridayId && a.setlist(fridayId).pages[0].name == "First set" && a.setlist(fridayId).info.parent == back.info.revision, "saved over as its next revision");
            check(refused([&] { auto bad = again; bad.pages[0].slots[0].color = "red"; a.saveSetlist(bad); }), "a slot's color that is not #rrggbb is refused");
            check(refused([&] { auto bad = again; bad.pages.clear(); a.saveSetlist(bad); }), "a setlist with no page is refused");
            check(refused([&] { auto bad = again; bad.pages[0].slots.push_back(slot("", "")); a.saveSetlist(bad); }), "a placeholder with no name is refused");
            check(refused([&] { a.setlist(bass); }), "a Performance is not a setlist");
            check(refused([&] { a.auditionStart(fridayId, 0); }), "a setlist is not auditioned");
            Control::Setlist sunday; sunday.info.name = "Sunday"; sunday.pages = {{"Page 1", {slot(extra, "Extra")}}};
            sundayId = a.saveSetlist(sunday);

            // Exported: the setlist, the Performances its slots name (each once), the sample and the plugin.
            check(a.exportLibraryItem(fridayId, friday.string()).empty(), "nothing left out of the setlist's export");
            const auto packed = readPackage(friday, {});
            std::vector<std::string> ids;
            for(const auto& i : packed.items) ids.push_back(i.kind + ":" + i.id);
            check(packed.kind == "setlist" && packed.id == fridayId && packed.name == "Friday" &&
                  ids == std::vector<std::string>{"setlist:" + fridayId, "performance:" + bass, "performance:" + strings}, "the export holds the setlist and its two Performances");
            check(packed.items[0].path.starts_with("Setlists/") && packed.files.count(packed.items[0].path + "/setlist.json"), "the setlist in Setlists/ in the archive");
            check(packed.plugins.size() == 1 && packed.plugins[0].id == "org.kick.test.synth", "and the plugin they use");
            check(std::any_of(packed.files.begin(), packed.files.end(), [&](const auto& f) { return f.first.starts_with("Assets/" + sampleSha.substr(0, 16) + "/"); }), "and the sample the first uses");

            // A slot whose Performance has left the library goes as a placeholder, and says so.
            auto gone = again; gone.info.id.clear(); gone.info.name = "Gone"; gone.pages[0].slots.push_back(slot("0123456789abcdef", "Left"));
            const auto goneId = a.saveSetlist(gone);
            const auto notes = a.exportLibraryItem(goneId, (temp.path / "Gone.olib").string());
            check(notes.size() == 1 && notes[0].find("Left") != std::string::npos && notes[0].find("placeholder") != std::string::npos, "a missing Performance is said: " + (notes.empty() ? std::string() : notes[0]));
            const auto unpacked = temp.path / "gone-unpacked";
            const auto g = readPackage(temp.path / "Gone.olib", unpacked);
            const auto goneSet = readSetlist(read(unpacked / g.items[0].path / "setlist.json"));
            check(goneSet.pages[0].slots.back().id.empty() && goneSet.pages[0].slots.back().name == "Left", "and goes as a placeholder, so the archive reads");
            a.deleteLibraryItem(goneId);

            // The library exported with Friday only: every Performance, Friday, not Sunday.
            check(refused([&] { a.exportLibrary((temp.path / "None.olib").string(), "Mine", {bass}); }), "a library export names setlists only");
            a.exportLibrary(whole.string(), "My Library!", {fridayId});
            const auto lib = readPackage(whole, {});
            std::set<std::string> held;
            for(const auto& i : lib.items) held.insert(i.id);
            check(lib.kind == "library" && lib.id == "My-Library-" && lib.name == "My Library!" && held == std::set<std::string>{bass, strings, extra, fridayId},
                  "the library export holds the chosen setlist, not the other, and every Performance");
        }
        // Device B: Friday into the user's own storage: new IDs, the slots pointing at the copies.
        {
            device(temp.path / "b");
            Session b(48000, 64);
            const auto installed = b.installArchive(friday.string(), false, {}, true);
            check(installed.added == 3, "the setlist and its two Performances installed: " + std::to_string(installed.added));
            for(const auto& n : installed.notes) check(n.find("sample") == std::string::npos, "no sample missing: " + n);
            const auto setlists = ofType(b, "oscairn.setlist");
            const auto performances = ofType(b, "oscairn.performance");
            check(setlists.size() == 1 && setlists[0].id != fridayId && setlists[0].origin.item == fridayId, "the setlist copied under a new ID, recording where it came from");
            const auto* copyBass = named(performances, "Bass Split");
            const auto* copyStrings = named(performances, "Strings Pad");
            check(copyBass && copyStrings && copyBass->id != bass && copyStrings->id != strings, "the Performances copied under new IDs");
            if(!setlists.empty() && copyBass && copyStrings)
            {
                const auto s = b.setlist(setlists[0].id);
                check(s.pages.size() == 2 && s.pages[0].slots.size() == 3 && s.pages[0].slots[0].id == copyBass->id && s.pages[0].slots[1].empty &&
                      s.pages[0].slots[2].id == copyStrings->id && s.pages[0].slots[2].label == "Opener" && s.pages[1].slots[0].id.empty() &&
                      s.pages[1].slots[0].name == "Piano Ballad (CP80)" && s.pages[1].slots[1].id == copyBass->id,
                      "its slots point at the copies, the empty slot, labels and placeholder kept");
            }
            const auto again = b.installArchive(friday.string(), false, {}, true);
            check(again.added == 0 && again.skipped == 3, "installed again, nothing is copied twice");
            // Chosen alone, a setlist brings its Performances.
            device(temp.path / "b2");
            Session b2(48000, 64);
            const auto chosen = b2.installArchive(friday.string(), false, {fridayId}, true);
            check(chosen.added == 3 && ofType(b2, "oscairn.performance").size() == 2, "a setlist chosen brings the Performances it names");
        }
        // Device C: as a library, every ID kept.
        {
            device(temp.path / "c");
            Session c(48000, 64);
            const auto installed = c.installArchive(friday.string(), true, {}, true);
            const auto s = c.setlist(fridayId);
            Control::LibraryQuery one; one.id = fridayId;
            const auto item = c.libraryQuery(one).items;
            check(installed.added == 3 && item.size() == 1 && item[0].library == fridayId && s.pages[0].slots[0].id == bass && s.pages[0].slots[2].id == strings,
                  "as a library: locked, IDs kept, slots as they were");
            // A locked setlist saved is a new one of the user's own.
            const auto mine = c.saveSetlist(s);
            check(mine != fridayId && c.setlist(fridayId).pages.size() == 2, "saving a locked setlist makes the user's own copy");
        }
        // By hand, as docs/setlist-format.md says: a setlist naming a Performance the archive
        // does not hold is refused; one whose only other slot is a placeholder installs.
        {
            device(temp.path / "d");
            const auto stage = temp.path / "stage";
            const auto performanceFolder = temp.path / "a" / "Performances";
            fs::path bassFolder;
            for(const auto& e : fs::directory_iterator(performanceFolder)) if(read(e.path() / "performance.json").find("\"id\":\"" + bass + "\"") != std::string::npos) bassFolder = e.path();
            fs::create_directories(stage / "Performances");
            fs::copy(bassFolder, stage / "Performances" / "Bass Split.oscairn", fs::copy_options::recursive);
            auto document = [](const std::string& slots) {
                return R"({"format":"oscairn.setlist","version":2,"info":{"id":"7d0c5e2a9b13f4c8","name":"Hand","description":"","tags":[],"color":"","revision":"","parent":"","hash":"","origin":null},)" "\n"
                       R"( "display": {"mode": "list", "columns": 4, "rows": 3},)" "\n"
                       R"( "pages": [{"name": "Set", "slots": [)" + slots + "]}]}\n";
            };
            Package p; p.id = "hand-made"; p.name = "Hand made"; p.kind = "library";
            p.items = {{"setlist", "7d0c5e2a9b13f4c8", "Hand", "Setlists/Hand.osetlist"}, {"performance", bass, "Bass Split", "Performances/Bass Split.oscairn"}};
            write(stage / "Setlists/Hand.osetlist/setlist.json", document(R"({"id": ")" + bass + R"(", "name": "Bass Split", "color": "", "label": ""}, null, {"id": "", "name": "CP80", "color": "", "label": ""}, {"id": "feedfacefeedface", "name": "Missing", "color": "", "label": ""})"));
            writePackage(temp.path / "Missing.olib", p, stage);
            Session d(48000, 64);
            bool missing = false;
            try { d.installArchive((temp.path / "Missing.olib").string(), false, {}, true); }
            catch(const std::exception& e) { missing = std::string(e.what()).find("does not hold") != std::string::npos; }
            check(missing && ofType(d, "oscairn.setlist").empty() && ofType(d, "oscairn.performance").empty(), "a setlist naming a Performance the archive lacks is refused, leaving nothing");
            write(stage / "Setlists/Hand.osetlist/setlist.json", document(R"({"id": ")" + bass + R"(", "name": "Bass Split", "color": "", "label": ""}, null, {"id": "", "name": "CP80", "color": "", "label": "Placeholder"})"));
            writePackage(temp.path / "Hand.olib", p, stage);
            const auto installed = d.installArchive((temp.path / "Hand.olib").string(), false, {}, true);
            const auto setlists = ofType(d, "oscairn.setlist");
            const auto copies = ofType(d, "oscairn.performance");
            const auto* copy = named(copies, "Bass Split");
            check(installed.added == 2 && setlists.size() == 1 && copy, "a setlist written by hand, with a placeholder, installs");
            if(setlists.size() == 1 && copy)
            {
                const auto s = d.setlist(setlists[0].id);
                check(s.pages[0].slots.size() == 3 && s.pages[0].slots[0].id == copy->id && s.pages[0].slots[1].empty && s.pages[0].slots[2].id.empty() && s.pages[0].slots[2].label == "Placeholder",
                      "its slot points at the copy, the placeholder as written");
            }
            // The library export from device A installs as a library too.
            const auto lib = d.installArchive(whole.string(), true, {}, true);
            check(lib.added == 4, "the library export installs: " + std::to_string(lib.added));
        }
        // Device E: the Setlist app's version 1 files become library setlists when the engine starts.
        {
            device(temp.path / "e");
            write(temp.path / "e/Setlists/Old.json", R"({"format":"oscairn.setlist","version":1,"name":"Old","display":{"mode":"grid","columns":2,"rows":1},"entries":[{"id":"aaaaaaaaaaaaaaaa","name":"One","color":"#e5484d"},{"id":"bbbbbbbbbbbbbbbb","name":"Two","color":""},{"id":"cccccccccccccccc","name":"Three","color":""}]})");
            write(temp.path / "e/Setlists/Broken.json", "{not a setlist");
            Session e(48000, 64);
            const auto setlists = ofType(e, "oscairn.setlist");
            check(setlists.size() == 1 && setlists[0].name == "Old" && !fs::exists(temp.path / "e/Setlists/Old.json"), "a version 1 setlist converted, its file removed");
            if(setlists.size() == 1)
            {
                const auto s = e.setlist(setlists[0].id);
                check(s.mode == "grid" && s.columns == 2 && s.rows == 1 && s.pages.size() == 2 && s.pages[0].name == "Page 1" && s.pages[0].slots.size() == 2 &&
                      s.pages[0].slots[0].id == "aaaaaaaaaaaaaaaa" && s.pages[0].slots[0].color == "#e5484d" && s.pages[1].slots.size() == 1 && s.pages[1].slots[0].name == "Three",
                      "a page per screen of its grid, each entry a slot");
            }
            check(fs::exists(temp.path / "e/Setlists/Broken.json"), "a file that cannot be read stays where it is");
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    if(!failures) std::puts("ok: setlists saved, exported with their Performances, installed with slots remapped or kept, refused when incomplete, and converted from version 1");
    return failures ? 1 : 0;
}
