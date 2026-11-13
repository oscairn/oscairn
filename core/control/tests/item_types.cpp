// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0023's item types. Step 1: Oscairn's own types are manifests, read by the same rules as
// any other, and manifests that would name paths, reuse a folder Oscairn keeps or claim another
// type's ID are refused. Step 2: a type added here is indexed, queried, exported and installed by
// type, with no change to the library's code.
#include "oscairn/item_types.h"
#include "oscairn/package.h"
#include "oscairn/session.h"
#include "../../plugin-host/src/StateFiles.h"
#include <cstdlib>
#include <fstream>
#include <cstdio>
#include <stdexcept>
#include <string>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    // A manifest with one field replaced: `key` (a JSON member, as written below) by `with`.
    std::string manifest(const std::string& key = {}, const std::string& with = {})
    {
        std::string m = R"({"format":"oscairn.type","version":1,"id":"org.example.pattern","name":"Pattern","plural":"Patterns",)"
                        R"("recognize":{"folder":".opattern","document":"pattern.json","format":"org.example.pattern","version":1},)"
                        R"("storage":"managed","folder":"Patterns","identity":"id","fields":[{"id":"steps","name":"Steps","facet":true}]})";
        if(!key.empty()) { const auto at = m.find(key); if(at == std::string::npos) throw std::logic_error("no " + key); m.replace(at, key.size(), with); }
        return m;
    }
    void write(const fs::path& file, const std::string& text) { fs::create_directories(file.parent_path()); std::ofstream(file, std::ios::binary) << text; }
    bool refused(const std::string& m) { try { readItemType(m); return false; } catch(const std::invalid_argument&) { return true; } }
}

int main()
{
    try
    {
        // Step 1: the built-in types, as manifests.
        const auto performance = itemType("oscairn.performance"), sound = itemType("sound"), sample = itemType("oscairn.sample");
        check(performance && performance->kind() == "performance" && performance->folder == "Performances" && performance->suffix == ".oscairn" &&
              performance->document == "performance.json" && performance->format == "oscairn.performance" && performance->version == 13 && performance->indexed(),
              "the Performance type, by ID");
        check(sound && sound->id == "oscairn.sound" && sound->folder == "Sounds" && sound->suffix == ".osound" && sound->document == "sound.json" && sound->version == 8,
              "the Sound type, by kind");
        check(sample && sample->storage == "content" && sample->identity == "content" && sample->folder == "Assets" && !sample->indexed(), "the sample type, stored by content");
        check(!itemType("pattern") && !itemType("org.example.pattern"), "no other type until one is added");

        // A manifest as RFC 0023 gives it, and the ways one is refused.
        const auto pattern = readItemType(manifest());
        check(pattern.id == "org.example.pattern" && pattern.kind() == "org.example.pattern" && pattern.plural == "Patterns" && pattern.fields.size() == 1 &&
              pattern.fields[0].facet && pattern.indexed(), "a type's manifest is read");
        check(refused(manifest(R"("format":"oscairn.type")", R"("format":"oscairn.app")")), "another format is refused");
        check(refused(manifest(R"("version":1,"id")", R"("version":2,"id")")), "another version is refused");
        check(refused(manifest(R"("id":"org.example.pattern")", R"("id":"Pattern")")), "an ID that is not reverse-DNS is refused");
        check(refused(manifest(R"("id":"org.example.pattern")", R"("id":"org.Example.pattern")")), "an ID with capitals is refused");
        check(refused(manifest(R"("folder":"Patterns")", R"("folder":"../Patterns")")), "a folder that climbs out is refused");
        check(refused(manifest(R"("folder":"Patterns")", R"("folder":"a/b")")), "a folder path is refused");
        check(refused(manifest(R"("folder":"Patterns")", R"("folder":".Patterns")")), "a hidden folder is refused");
        check(refused(manifest(R"("folder":"Patterns")", R"("folder":"Plugins")")), "a folder Oscairn keeps for itself is refused");
        check(refused(manifest(R"("folder":".opattern")", R"("folder":"opattern")")), "a folder suffix without its dot is refused");
        check(refused(manifest(R"("document":"pattern.json")", R"("document":"../pattern.json")")), "a document path is refused");
        check(refused(manifest(R"("storage":"managed")", R"("storage":"cloud")")), "an unknown storage is refused");
        check(refused(manifest(R"("identity":"id")", R"("identity":"path")")), "a managed type identified by anything but its ID is refused");
        check(refused(manifest(R"("identity":"id")", R"("identity":"name")")), "an unknown identity is refused");
        check(refused(R"({"format":"oscairn.type","version":1,"id":"org.example.wave","name":"Wave","plural":"Waves","recognize":{"extensions":["wav"]},"storage":"content","folder":"Waves","identity":"content"})"),
              "an extension without its dot is refused");
        check(!refused(R"({"format":"oscairn.type","version":1,"id":"org.example.wave","name":"Wave","plural":"Waves","recognize":{"extensions":[".wav"]},"storage":"content","folder":"Waves","identity":"content"})"),
              "a type stored by content, recognized by extension");

        // Adding a type: never over another's ID or folder.
        addItemType(pattern);
        check(itemType("org.example.pattern") && itemTypes().size() == 4, "a type added is known");
        bool twice = false; try { addItemType(pattern); } catch(const std::invalid_argument&) { twice = true; }
        check(twice, "a type added twice is refused");
        auto clash = readItemType(manifest(R"("id":"org.example.pattern")", R"("id":"org.example.other")"));
        clash.folder = "sounds";
        bool folder = false; try { addItemType(clash); } catch(const std::invalid_argument&) { folder = true; }
        check(folder, "a folder another type has, in any letter case, is refused");

        // Step 2: a type added here is indexed, queried, exported and installed by type, with no
        // change to the library's code; each item in its type's folder, in the data folder and in archives.
        state_files::TemporaryDirectory temp;
        auto groove = [](const std::string& id, const std::string& name) {
            return "{\"format\":\"org.example.pattern\",\"version\":1,\"info\":{\"id\":\"" + id + "\",\"name\":\"" + name +
                   "\",\"description\":\"\",\"tags\":[\"drums\"],\"color\":\"\",\"revision\":\"r1\",\"parent\":\"\",\"hash\":\"\",\"origin\":null},\"steps\":16}\n";
        };
        const auto archive = temp.path / "Groove.olib";
        {
            ::setenv("OSCAIRN_DATA_HOME", (temp.path / "a").c_str(), 1);
            write(temp.path / "a/Patterns/Groove.opattern/pattern.json", groove("9a8b7c6d5e4f3a2b", "Groove"));
            write(temp.path / "a/Patterns/Fill.opattern/pattern.json", groove("1a2b3c4d5e6f7a8b", "Fill"));
            write(temp.path / "a/Patterns/Wrong.opattern/pattern.json", "{\"format\":\"org.example.pattern\",\"version\":2}"); // another version: not one the type reads
            Session a(48000, 64);
            Control::LibraryQuery q; q.types = {"org.example.pattern"};
            const auto found = a.libraryQuery(q);
            check(found.items.size() == 2 && found.items[0].name == "Fill" && found.items[0].kind == "org.example.pattern" && found.items[1].name == "Groove",
                  "the library indexes a type added, by its manifest: " + std::to_string(found.items.size()));
            q.types = {"org.example.pattern", "oscairn.performance"}; q.tags = {"drums"};
            check(a.libraryQuery(q).items.size() == 2, "a query takes several types, with the rest of its filters");
            q.types = {"oscairn.sound"}; q.tags.clear();
            check(a.libraryQuery(q).items.empty(), "and another type's query does not list them");
            Control::LibraryQuery kind; kind.kind = "org.example.pattern";
            check(a.libraryQuery(kind).items.size() == 2, "by kind as by type");
            check(a.exportLibraryItem("9a8b7c6d5e4f3a2b", archive.string()).empty(), "a pattern exported");
            const auto p = readPackage(archive, {});
            check(p.kind == "org.example.pattern" && p.items.size() == 1 && p.items[0].path == "Patterns/Groove.opattern" && p.files.count("Patterns/Groove.opattern/pattern.json"),
                  "in its type's folder in the archive");
            bool audition = false; try { a.auditionStart("9a8b7c6d5e4f3a2b", 0); } catch(const std::invalid_argument&) { audition = true; }
            check(audition, "an item of another type is not auditioned as a Performance");
        }
        {
            ::setenv("OSCAIRN_DATA_HOME", (temp.path / "b").c_str(), 1);
            Session b(48000, 64);
            const auto installed = b.installArchive(archive.string(), false);
            Control::LibraryQuery q; q.types = {"org.example.pattern"};
            const auto found = b.libraryQuery(q);
            check(installed.added == 1 && found.items.size() == 1 && found.items[0].name == "Groove" && found.items[0].id != "9a8b7c6d5e4f3a2b" &&
                  found.items[0].origin.item == "9a8b7c6d5e4f3a2b" && fs::path(found.items[0].path).parent_path() == temp.path / "b/Patterns",
                  "installed as the user's own: in its type's folder, under a new ID");
            b.installArchive(archive.string(), true);
            q.source = "9a8b7c6d5e4f3a2b";
            check(b.libraryQuery(q).items.size() == 1 && fs::exists(temp.path / "b/Libraries/9a8b7c6d5e4f3a2b/Patterns/Groove.opattern/pattern.json"), "and as a library, in its type's folder there");
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    if(!failures) std::puts("ok: item types read from manifests, built in or added, bad ones refused; a type added is indexed, queried, exported and installed by type");
    return failures ? 1 : 0;
}
