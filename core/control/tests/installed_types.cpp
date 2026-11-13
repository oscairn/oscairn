// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0023 step 3: item types installed by packages. A library package declaring a pattern type
// installs it with its items, which are listed, picked (queried by type) and packed (an export
// carries the type); uninstalling the package leaves the user's own items of the type listed
// read-only, until none is left and the type is forgotten. A package may declare only types
// under its own ID and never takes another's folder. A type stored in place indexes the files
// of the folders the user chose, where they are, never moving or removing them.
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "../../plugin-host/src/StateFiles.h"
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <functional>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    void write(const fs::path& f, const std::string& text) { fs::create_directories(f.parent_path()); std::ofstream(f, std::ios::binary | std::ios::trunc) << text; }
    void device(const fs::path& home) { fs::create_directories(home); ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1); }
    std::vector<Control::LibraryItem> ofType(Session& s, const std::string& type) { Control::LibraryQuery q; q.types = {type}; return s.libraryQuery(q).items; }
    std::optional<Control::LibraryType> known(Session& s, const std::string& id)
    {
        for(const auto& t : s.libraryTypes()) if(t.id == id) return t;
        return std::nullopt;
    }
    bool refused(const std::function<void()>& f) { try { f(); return false; } catch(const std::exception&) { return true; } }
    const std::string patternType = R"({"format":"oscairn.type","version":1,"id":"com.example.grooves.pattern","name":"Pattern","plural":"Patterns",
        "recognize":{"folder":".ogroove","document":"groove.json","format":"com.example.grooves.pattern","version":1},"storage":"managed","folder":"Grooves","identity":"id"})";
    std::string groove(const std::string& id, const std::string& name)
    {
        return R"({"format":"com.example.grooves.pattern","version":1,"info":{"id":")" + id + R"(","name":")" + name +
               R"(","description":"","tags":[],"color":"","revision":"r1","parent":"","hash":"","origin":null},"steps":16})" "\n";
    }
    // A library package of `id` holding `files` (path -> text) and listing `items`.
    void pack(const fs::path& archive, const std::string& id, const std::vector<std::pair<std::string, std::string>>& files, const std::vector<Package::Item>& items)
    {
        state_files::TemporaryDirectory stage;
        for(const auto& [path, text] : files) write(stage.path / path, text);
        Package p; p.id = id; p.name = id; p.kind = "library"; p.items = items;
        writePackage(archive, p, stage.path);
    }
}

int main()
{
    try
    {
        state_files::TemporaryDirectory temp;
        const auto grooves = temp.path / "Grooves.olib", exported = temp.path / "Copy.olib";
        pack(grooves, "com.example.grooves",
             {{"Types/com.example.grooves/pattern.json", patternType},
              {"Grooves/Basic.ogroove/groove.json", groove("1111aaaa2222bbbb", "Basic")}, {"Grooves/Shuffle.ogroove/groove.json", groove("3333cccc4444dddd", "Shuffle")}},
             {{"com.example.grooves.pattern", "1111aaaa2222bbbb", "Basic", "Grooves/Basic.ogroove"}, {"com.example.grooves.pattern", "3333cccc4444dddd", "Shuffle", "Grooves/Shuffle.ogroove"}});
        // Only under its own ID, and never over Oscairn's folders.
        pack(temp.path / "Elsewhere.olib", "com.example.other", {{"Types/com.example.other/pattern.json", patternType}}, {});
        check(refused([&] { readPackage(temp.path / "Elsewhere.olib", {}); }), "a type not under its package's ID is refused");
        std::string sounds = patternType;
        sounds.replace(sounds.find("\"folder\":\"Grooves\""), 18, "\"folder\":\"Sounds\"");
        pack(temp.path / "Sounds.olib", "com.example.grooves", {{"Types/com.example.grooves/pattern.json", sounds}}, {});
        check(refused([&] { readPackage(temp.path / "Sounds.olib", {}); }), "a type taking Oscairn's own folder is refused");
        check(readPackage(grooves, {}).types.size() == 1, "the package declares its type");
        std::string copyId;
        // Device A: the package installed as a library, its items listed, picked, copied and packed; then uninstalled.
        {
            device(temp.path / "a");
            Session a(48000, 64);
            const auto installed = a.installArchive(grooves.string(), true);
            const auto type = known(a, "com.example.grooves.pattern");
            const auto items = ofType(a, "com.example.grooves.pattern");
            check(installed.added == 2 && type && type->owner == "com.example.grooves" && !type->readOnly && type->kind == "com.example.grooves.pattern",
                  "the type installed with its package, known and owned by it");
            check(items.size() == 2 && items[0].name == "Basic" && items[0].library == "com.example.grooves" && !items[0].readOnly && fs::exists(temp.path / "a/Types/com.example.grooves/com.example.grooves.pattern.json"),
                  "its items are listed, and the type kept in the data folder");
            // Another package's type in the same folder: the first keeps it.
            std::string clash = patternType;
            clash.replace(clash.find("com.example.grooves.pattern\",\"name\""), 27, "com.example.drums.pattern");
            pack(temp.path / "Clash.olib", "com.example.drums", {{"Types/com.example.drums/pattern.json", clash}}, {});
            bool second = false; try { a.installArchive((temp.path / "Clash.olib").string(), true); } catch(const std::exception& e) { second = std::string(e.what()).find("Grooves") != std::string::npos || std::string(e.what()).find("folder") != std::string::npos; }
            check(second && !known(a, "com.example.drums.pattern"), "a second package's type in its folder is refused, and says so");
            copyId = a.libraryCopy(items[0].id);
            Control::PerformanceInfo edit; edit.name = "Basic mine";
            a.libraryEdit(copyId, edit);
            Control::LibraryQuery mine; mine.id = copyId;
            const auto copy = a.libraryQuery(mine).items;
            check(copy.size() == 1 && copy[0].name == "Basic mine" && fs::path(copy[0].path).parent_path() == temp.path / "a/Grooves", "the user's own copy in the type's folder, editable");
            check(a.exportLibraryItem(copyId, exported.string()).empty(), "an item of the type exported");
            const auto packed = readPackage(exported, {});
            check(packed.types.size() == 1 && packed.types[0].id == "com.example.grooves.pattern" && packed.types[0].owner == "com.example.grooves" &&
                  packed.files.count("Types/com.example.grooves/com.example.grooves.pattern.json"), "the export carries the type, owned by its package");
            a.uninstallLibrary("com.example.grooves");
            const auto left = ofType(a, "com.example.grooves.pattern");
            const auto after = known(a, "com.example.grooves.pattern");
            check(left.size() == 1 && left[0].id == copyId && left[0].readOnly && after && after->readOnly, "uninstalled, the user's own item stays, listed read-only");
            check(refused([&] { a.libraryEdit(copyId, edit); }) && refused([&] { a.libraryCopy(copyId); }), "and is not edited or copied");
            a.deleteLibraryItem(copyId);
            check(!known(a, "com.example.grooves.pattern") && !fs::exists(temp.path / "a/Types/com.example.grooves/com.example.grooves.pattern.json"), "with none left, the type is forgotten");
        }
        // Device B: the export installed: the type comes with it, read-only without its package.
        {
            device(temp.path / "b");
            Session b(48000, 64);
            const auto installed = b.installArchive(exported.string(), false);
            const auto items = ofType(b, "com.example.grooves.pattern");
            const auto type = known(b, "com.example.grooves.pattern");
            check(installed.added == 1 && items.size() == 1 && items[0].name == "Basic mine" && items[0].readOnly && type && type->readOnly,
                  "an item exported with its type installs, read-only until its package is here");
            b.installArchive(grooves.string(), true);
            check(!known(b, "com.example.grooves.pattern")->readOnly && !ofType(b, "com.example.grooves.pattern").front().readOnly, "with the package installed, it is no longer read-only");
        }
        // Device C: a type stored in place indexes the folders chosen, where they are.
        {
            device(temp.path / "c");
            pack(temp.path / "Tracks.olib", "com.example.tracks",
                 {{"Types/com.example.tracks/track.json", R"({"format":"oscairn.type","version":1,"id":"com.example.tracks.track","name":"Track","plural":"Tracks","recognize":{"extensions":[".wav"]},"storage":"in-place","folder":"Tracks","identity":"path"})"}}, {});
            Session c(48000, 64);
            c.installArchive((temp.path / "Tracks.olib").string(), true);
            const auto music = temp.path / "music";
            write(music / "Intro.wav", "a"); write(music / "Loud.WAV", "b"); write(music / "notes.txt", "c"); write(music / ".hidden.wav", "d"); write(music / "set/Outro.wav", "e");
            check(refused([&] { c.addLibraryFolder((temp.path / "nothing").string()); }), "a folder that is not there is refused");
            c.addLibraryFolder(music.string());
            auto tracks = ofType(c, "com.example.tracks.track");
            std::vector<std::string> names;
            for(const auto& t : tracks) names.push_back(t.name);
            check(names == std::vector<std::string>{"Intro", "Loud", "Outro"} && tracks[0].inPlace && tracks[0].readOnly && c.libraryFolders().size() == 1,
                  "its files listed where they are, by name, the rest left out");
            check(refused([&] { c.deleteLibraryItem(tracks[0].id); }) && fs::exists(music / "Intro.wav"), "an item in place is never removed");
            check(refused([&] { c.exportLibraryItem(tracks[0].id, (temp.path / "Track.olib").string()); }), "nor packed");
            c.libraryRescan();
            check(ofType(c, "com.example.tracks.track").size() == 3 && ofType(c, "com.example.tracks.track")[0].id == tracks[0].id, "a rescan keeps each by its identity");
            c.removeLibraryFolder(c.libraryFolders().front());
            check(ofType(c, "com.example.tracks.track").empty() && fs::exists(music / "Intro.wav"), "the folder taken out, its files are no longer listed, and stay");
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    if(!failures) std::puts("ok: item types installed by packages: listed, picked, packed, read-only once their package goes, and stored in place");
    return failures ? 1 : 0;
}
