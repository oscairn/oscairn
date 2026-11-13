// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0021's libraries and exports, across devices (each its own OSCAIRN_DATA_HOME): an exported
// Performance carries its unedited part's Sound and the samples they use; installed into the
// user's own storage it gets new IDs, its part pointing at the Sound's copy, and installing again
// skips what is unchanged; installed as a library it is locked, comes back when deleted and goes
// when uninstalled, leaving the user's copies and the samples; a new version of it replaces its
// revised items and removes those it no longer has. The plugin it uses goes with it
// (RFC 0006) and installs into the store, where a Performance whose plugin is not at its saved
// path finds it. A build for another platform added on one device goes along too. The store
// lists each plugin with what uses it, and removing one is refused while the open Performance
// plays it; what used it then opens with a stand-in until it is installed again.
// Signatures (RFC 0006): oscairn-sign signs a build and an archive, an export carries the build's
// signature and signs its items and the whole archive when asked.
// A plugin package carries one bundle holding several plugins (Oscairn's Splitters) once.
// Asset libraries (RFC 0009 step 4): redistribution checks on export, and garbage collection
// from explicit roots (with the Legacy Sampler playing one).
//   oscairn-archives-test FIXTURE.clap OSCAIRN-SIGN [SPLITTERS.clap [SAMPLER.clap]]
#include "oscairn/session.h"
#include "oscairn/package.h"
#include "oscairn/assets.h"
#include "oscairn/discovery.h"
#include "miniz/miniz.h"
#include "oscairn/wav_file.h"
#include "../../plugin-host/src/StateFiles.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <fstream>
#include <sstream>
#include <sys/stat.h>

using namespace oscairn;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;
    void check(bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } }
    std::string read(const fs::path& f) { std::ifstream in(f, std::ios::binary); std::ostringstream out; out << in.rdbuf(); return out.str(); }
    void device(const fs::path& home) { fs::create_directories(home); ::setenv("OSCAIRN_DATA_HOME", home.c_str(), 1); }
    std::vector<Control::LibraryItem> items(Session& s, const std::string& kind) { Control::LibraryQuery q; q.kind = kind; return s.libraryQuery(q).items; }
    std::string shell(const fs::path& p) { std::string out = "'"; for(char c : p.string()) out += c == '\'' ? std::string("'\\''") : std::string(1, c); return out + "'"; }
    // A command's exit status and what it printed.
    std::pair<int, std::string> run(const std::string& command)
    {
        std::string out;
        FILE* p = ::popen(command.c_str(), "r");
        if(!p) return {-1, out};
        char buffer[4096];
        while(const auto n = std::fread(buffer, 1, sizeof buffer, p)) out.append(buffer, n);
        return {::pclose(p), out};
    }
    // A copy of an archive whose manifest each(a) rewrote; other entries are copied whole, those keep() takes.
    void rewriteArchive(const fs::path& from, const fs::path& to, const std::function<std::string(std::string)>& each, const std::function<bool(const std::string&)>& keep = {})
    {
        mz_zip_archive in{}, out{};
        mz_zip_reader_init_file(&in, from.c_str(), 0);
        mz_zip_writer_init_file(&out, to.c_str(), 0);
        for(mz_uint i = 0; i < mz_zip_reader_get_num_files(&in); ++i)
        {
            mz_zip_archive_file_stat st; mz_zip_reader_file_stat(&in, i, &st);
            if(std::string(st.m_filename) != "library.json") { if(!keep || keep(st.m_filename)) mz_zip_writer_add_from_zip_reader(&out, &in, i); continue; }
            std::string m(size_t(st.m_uncomp_size), '\0'); mz_zip_reader_extract_to_mem(&in, i, m.data(), m.size(), 0);
            m = each(m);
            mz_zip_writer_add_mem(&out, "library.json", m.data(), m.size(), MZ_DEFAULT_COMPRESSION);
        }
        mz_zip_writer_finalize_archive(&out); mz_zip_writer_end(&out); mz_zip_reader_end(&in);
    }
}

int main(int argc, char** argv)
{
    if(argc < 3) { std::fprintf(stderr, "usage: %s FIXTURE OSCAIRN-SIGN [SPLITTERS.clap [SAMPLER.clap]]\n", argv[0]); return 2; }
    const char* fixture = argv[1];
    const std::string tool = shell(argv[2]);
    const char* splitters = argc > 3 ? argv[3] : nullptr;
    const char* sampler = argc > 4 ? argv[4] : nullptr;
    try
    {
        state_files::TemporaryDirectory temp;
        const auto archive = temp.path / "Set.olib";
        const auto soundArchive = temp.path / "Bass.olib";
        kick::clap::AssetReference sample;
        std::string soundId, performanceId;
        const std::string other = platform() == "linux-arm64" ? "macos-arm64" : "linux-arm64";
        // Device A: a part saved as a Sound that uses a sample, then the Performance exported.
        {
            device(temp.path / "a");
            Session a(48000, 64);
            const auto synth = a.add("synth", fixture, "org.kick.test.synth");
            const auto part = a.addPart("Bass"); a.insertIntoPart(part, 0, synth); a.commit();
            soundId = a.savePartAsSound(part, "Bass").id;
            const auto wav = temp.path / "bass.wav"; writeWavFile(wav, std::vector<float>(480, .5f), 1, 48000);
            sample = a.importAsset(wav.string());
            // The fixture uses no samples, so its Sound's document is given one, as a sampler's lists it.
            const auto doc = fs::path(a.libraryQuery({}).items.front().path) / "sound.json";
            auto text = read(doc);
            const auto at = text.find("\"assets\":[]"); check(at != std::string::npos, "the Sound's plugin lists its samples");
            text.replace(at, 11, "\"assets\":[{\"library\":\"user\",\"path\":\"" + sample.path + "\",\"sha256\":\"" + sample.sha256 + "\"}]");
            std::ofstream(doc, std::ios::binary | std::ios::trunc) << text;
            a.libraryRescan();
            performanceId = a.savePerformanceToLibrary("Set");
            // The fixture stands in for another platform's build: it cannot be loaded here to check either way.
            const auto version = a.graphState().topology.nodes.at(0).plugin.version;
            bool here = false; try { a.addPluginBuild("clap", "org.kick.test.synth", version, platform(), fixture); } catch(const std::exception&) { here = true; }
            check(here, "this device's build is not added: it is the one it loads");
            a.addPluginBuild("clap", "org.kick.test.synth", version, other, fixture);
            check(a.exportPerformance(archive.string()).empty(), "nothing left out");
            const auto packed = readPackage(archive, {});
            check(packed.plugins.size() == 1 && packed.plugins[0].id == "org.kick.test.synth" && packed.plugins[0].builds.size() == 2 &&
                  packed.plugins[0].builds[0].sha256 == bundleDigest(fixture) && packed.plugins[0].builds[1].sha256 == bundleDigest(fixture),
                  "the plugin it uses goes with it, this device's build and the other platform's");
            bool again = false; try { a.exportPerformance(archive.string()); } catch(const std::exception&) { again = true; }
            check(again, "an export never overwrites a file");
            const auto contents = a.archiveContents(archive.string());
            check(contents.kind == "performance" && contents.name == "Set" && contents.items.size() == 2, "the export holds the Performance and its part's Sound: " + std::to_string(contents.items.size()));
            // From the library, without opening: the Performance as saved with its part's Sound, and a Sound alone.
            a.exportLibraryItem(performanceId, (temp.path / "Saved.olib").string());
            const auto saved = a.archiveContents((temp.path / "Saved.olib").string());
            check(saved.kind == "performance" && saved.id == performanceId && saved.items.size() == 2 && saved.items[1].id == soundId, "a library Performance exported with its part's Sound");
            a.exportLibraryItem(soundId, soundArchive.string());
            const auto sound = a.archiveContents(soundArchive.string());
            check(sound.kind == "sound" && sound.items.size() == 1 && sound.items[0].id == soundId, "a library Sound exported alone");
            bool missing = false; try { a.exportLibraryItem("nothing", (temp.path / "None.olib").string()); } catch(const std::exception&) { missing = true; }
            check(missing, "an item not in the library is refused");
        }
        // Device B: into the user's own storage, then as a library.
        {
            device(temp.path / "b");
            Session b(48000, 64);
            // Its plugin builds are unsigned (RFC 0006): listed first, refused without a yes, leaving nothing.
            const auto contents = b.archiveContents(archive.string());
            check(contents.code.size() == 2 && contents.code[0].status == "unsigned" && contents.code[0].kind == "plugin" && contents.code[0].version == "1" && contents.signer.status == "unsigned",
                  "an archive's contents say its plugin builds and the archive are unsigned");
            std::string why; try { b.installArchive(archive.string(), false); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("unknown source") != std::string::npos && why.find("KICK fixture 1 (" + platform() + ") unsigned") != std::string::npos && items(b, "sound").empty() &&
                  !fs::exists(temp.path / "b" / "Plugins"), "an unsigned plugin build is refused without a yes, listing it, installing nothing: " + why);
            const auto installed = b.installArchive(archive.string(), false, {}, true);
            check(installed.code.size() == 2 && installed.code[0].status == "unsigned", "and installed with one, saying so");
            check(installed.added == 2 && installed.skipped == 0 && installed.notes.empty(), "both copied in: " + std::to_string(installed.added));
            const auto store = temp.path / "b" / "Plugins" / "clap";
            check(fs::is_directory(store) && std::distance(fs::directory_iterator(store), fs::directory_iterator()) == 1, "its plugin is in the store");
            const auto listed = b.installedPlugins();
            check(listed.size() == 1 && listed[0].id == "org.kick.test.synth" && listed[0].platforms.size() == 2 && listed[0].bytes > 0 && listed[0].usedBy.size() == 2 && !listed[0].open,
                  "the store lists it with both platforms' builds and the two items that use it");
            // Re-exported from here, both builds go again: this device's and the other from the store.
            check(b.exportLibraryItem(items(b, "sound").at(0).id, (temp.path / "Again.olib").string()).empty() && readPackage(temp.path / "Again.olib", {}).plugins.at(0).builds.size() == 2,
                  "a re-export carries every platform's build");
            const auto sounds = items(b, "sound"), performances = items(b, "performance");
            check(sounds.size() == 1 && performances.size() == 1 && sounds[0].id != soundId && performances[0].id != performanceId && sounds[0].library.empty(),
                  "under new IDs, as the user's own");
            check(sounds[0].origin.item == soundId && sounds[0].origin.hash == sounds[0].hash, "each records where it came from and its hash as imported");
            b.loadPerformanceFromLibrary(performances[0].id);
            check(b.graphState().parts.size() == 1 && b.graphState().parts[0].from.sound == sounds[0].id, "the Performance's part points at the Sound's copy");
            auto copy = sample; check(!assets::Store(temp.path / "b" / "assets").resolve(copy).empty(), "the sample came with it");
            const auto again = b.installArchive(archive.string(), false);
            check(again.added == 0 && again.skipped == 2 && items(b, "sound").size() == 1, "installing again adds nothing");
            b.deleteLibraryItem(sounds[0].id);
            const auto back = b.installArchive(archive.string(), false);
            check(back.added == 1 && back.skipped == 1, "a deleted copy comes again, the unchanged one is skipped");

            // As a library: locked, under its own IDs.
            const auto library = b.installArchive(archive.string(), true);
            check(library.added == 2 && b.libraries().size() == 1 && b.libraries()[0].items == 2, "installed as a library");
            // An installed library's manifest past the old 4 MiB limit still reads (a converted
            // MONTAGE M library's does): padded with JSON whitespace before its closing brace.
            for(const auto& folder : fs::directory_iterator(temp.path / "b" / "Libraries"))
            {
                auto text = read(folder.path() / "library.json");
                text.insert(text.rfind('}'), std::string((4u << 20) + 1, ' '));
                std::ofstream(folder.path() / "library.json", std::ios::binary | std::ios::trunc) << text;
            }
            b.libraryRescan();
            check(b.libraries().size() == 1 && b.libraries()[0].items == 2, "an installed library with a manifest over 4 MiB is listed");
            bool locked = false;
            for(const auto& i : items(b, "sound")) if(i.id == soundId) locked = !i.library.empty();
            check(locked, "its items keep their IDs, locked");
            b.loadPerformanceFromLibrary(performanceId);
            const auto saved = b.savePerformanceToLibrary("");
            check(saved != performanceId, "saving a locked Performance makes a new one");
            const auto part = b.graphState().parts[0].id;
            check(b.savePartAsSound(part, "").id != soundId, "and saving a part from a locked Sound makes a new Sound");
            b.deleteLibraryItem(soundId);
            const auto restored = b.installArchive(archive.string(), true);
            check(restored.added == 1 && restored.skipped == 1, "installing again restores only what was deleted");
            bool refusedEdit = false; try { b.libraryEdit(soundId, {.name = "Mine"}); } catch(const std::exception&) { refusedEdit = true; }
            check(refusedEdit, "a locked item's info is not edited");
            const auto lockedId = b.libraries()[0].id;
            Control::LibraryQuery inLibrary; inLibrary.source = lockedId;
            check(b.libraryQuery(inLibrary).total == 2, "a library's items by source");
            const auto own = b.libraryCopy(soundId);
            auto find = [&](const std::string& id) { for(const auto& i : items(b, "sound")) if(i.id == id) return i; throw std::runtime_error("not in the library: " + id); };
            const auto ownItem = find(own);
            check(ownItem.library.empty() && ownItem.name == "Bass" && ownItem.origin.library == lockedId && ownItem.origin.item == soundId && ownItem.origin.hash == ownItem.hash,
                  "a locked item's copy is the user's own, under its name, recording where it came from");
            b.libraryEdit(own, {.name = "Mine", .tags = {"bass"}});
            check(find(own).name == "Mine", "and is edited");
            b.deleteLibraryItem(own);
            b.uninstallLibrary(b.libraries()[0].id);
            check(b.libraries().empty() && items(b, "performance").size() == 2 && items(b, "sound").size() == 2, "uninstalled: its items go, the user's stay");
            copy = sample; check(!assets::Store(temp.path / "b" / "assets").resolve(copy).empty(), "and so does the sample");
        }
        // Device C: chosen items only.
        {
            device(temp.path / "c");
            Session c(48000, 64);
            const auto installed = c.installArchive(archive.string(), false, {soundId}, true);
            check(installed.added == 1 && items(c, "performance").empty() && items(c, "sound").size() == 1, "only the chosen Sound");
            check(c.installArchive(soundArchive.string(), false).added == 1 && items(c, "sound").size() == 2, "an exported Sound installs as the user's own");
            bool refused = false; try { c.installArchive(archive.string(), false, {"nothing"}); } catch(const std::exception&) { refused = true; }
            check(refused, "an item the archive does not have is refused");
            // Unpacking happens in the data folder, never the OS temp folder: on a tmpfs /tmp a
            // big archive would fill RAM on the way in. The OS temp folder is read-only here, so
            // an install that unpacked there could not run. Root writes there anyway, so as root
            // this proves nothing and is skipped.
            if(::geteuid() != 0)
            {
                const auto fakeTemp = temp.path / "c" / "fake-temp";
                fs::create_directories(fakeTemp);
                ::setenv("TMPDIR", fakeTemp.c_str(), 1);
                ::chmod(fakeTemp.c_str(), 0500);
                const auto again = c.installArchive(soundArchive.string(), false); // the same export again: skipped
                ::chmod(fakeTemp.c_str(), 0700);
                ::unsetenv("TMPDIR");
                check(again.skipped == 1 && items(c, "sound").size() == 2, "installing unpacks inside the data folder, not the read-only OS temp folder");
            }
            else std::puts("skipped: unpacking outside a read-only OS temp folder (root writes there anyway)");
        }
        // Device C2 (RFC 0031): an archive made for another device, as a repository exports one:
        // its manifest lists only the other platform's build, which it leaves out. Its items
        // install, and its plugin is said to have no build for this device, as before.
        {
            device(temp.path / "c2");
            const auto made = temp.path / "Elsewhere.olib";
            std::string here, there; // the two builds' paths
            const auto exported = readPackage(archive, {}); // kept: a range over a temporary's member would dangle
            for(const auto& b : exported.plugins.at(0).builds) (b.platform == platform() ? here : there) = b.path;
            rewriteArchive(archive, made, [&](std::string m) {
                auto cut = [&](size_t start, size_t end) { if(m[end] == ',') ++end; else if(m[start - 1] == ',') --start; m.erase(start, end - start); };
                const auto build = m.find("{\"platform\":\"" + platform() + "\"");
                cut(build, m.find('}', build) + 1); // unsigned: no braces inside
                for(auto file = m.find("\"" + here); file != std::string::npos; file = m.find("\"" + here)) // its files (a bundle's several)
                    cut(file, m.find('"', m.find("\":\"", file) + 3) + 1);
                return m;
            }, [&](const std::string& name) { return name.rfind(here, 0) != 0 && name.rfind(there, 0) != 0; });
            const auto read = readPackage(made, {});
            check(read.plugins.at(0).builds.empty() && read.plugins[0].absent == std::vector<std::string>{other}, "another platform's build may be left out of an archive");
            Session c(48000, 64);
            const auto installed = c.installArchive(made.string(), false, {}, true);
            check(installed.added == 2 && std::any_of(installed.notes.begin(), installed.notes.end(), [](const auto& n) { return n.find("has no build for this device in the archive") != std::string::npos; }) &&
                  !fs::exists(temp.path / "c2" / "Plugins" / "clap"), "its items install, and its plugin has no build for this device");
            // This device's build listed and left out: the archive made for another device, refused.
            rewriteArchive(archive, temp.path / "Other.olib", [](std::string m) { return m; }, [&](const std::string& name) { return name.rfind(here, 0) != 0; });
            std::string why; try { c.installArchive((temp.path / "Other.olib").string(), false, {}, true); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("no " + platform() + " build") != std::string::npos && why.find("another device") != std::string::npos, "this device's build left out is refused: " + why);
        }
        // An install or export cut short by a crash leaves its hidden folder in the data folder;
        // the library clears those over a day old when it starts, and leaves newer ones, which
        // may be another process's install in progress.
        {
            const auto home = temp.path / "o";
            device(home);
            const auto old = fs::file_time_type::clock::now() - std::chrono::hours(25);
            for(const auto* name : {".install-1", ".export-1", ".install-2", ".other-1"})
            {
                fs::create_directories(home / name / "Sounds");
                std::ofstream(home / name / "Sounds" / "big.bin") << "x";
                if(std::string(name) != ".install-2") fs::last_write_time(home / name, old);
            }
            Session o(48000, 64);
            check(items(o, "sound").empty(), "the library starts");
            check(!fs::exists(home / ".install-1") && !fs::exists(home / ".export-1"), "an install's and an export's folder over a day old are cleared");
            check(fs::is_directory(home / ".install-2") && fs::is_directory(home / ".other-1"), "a newer one, and any other hidden folder, stay");
        }
        // Device E: the library's index past the old 4 MiB limit is still used, as a library of
        // thousands of items has: a name only the index holds comes back.
        {
            device(temp.path / "e");
            Session e(48000, 64);
            e.savePerformanceToLibrary("Indexed");
            e.libraryRescan();
            const auto index = temp.path / "e" / "library" / "index.json";
            auto text = read(index);
            const auto at = text.find("\"name\":\"Indexed\"");
            check(at != std::string::npos, "the index holds the Performance");
            text.replace(at, 16, "\"name\":\"From the index\"");
            text.insert(text.rfind('}'), std::string((4u << 20) + 1, ' '));
            std::ofstream(index, std::ios::binary | std::ios::trunc) << text;
            e.libraryRescan();
            check(items(e, "performance").at(0).name == "From the index", "an index over 4 MiB is used");
        }
        // Device F: a plugin rebuilt in place while it plays goes with an export as rebuilt.
        {
            device(temp.path / "f");
            const auto build = temp.path / "f" / "Synth.clap";
            fs::copy_file(fixture, build);
            Session f(48000, 64);
            f.add("synth", build.string(), "org.kick.test.synth"); f.commit();
            check(f.exportPerformance((temp.path / "Before.olib").string()).empty(), "a plugin at its own path goes with an export");
            // Rebuilt as a linker does: written beside it and moved over it.
            fs::copy_file(fixture, build.string() + ".new");
            std::ofstream(build.string() + ".new", std::ios::binary | std::ios::app) << "rebuilt";
            fs::rename(build.string() + ".new", build);
            fs::last_write_time(build, fs::file_time_type::clock::now() + std::chrono::minutes(1));
            const auto notes = f.exportPerformance((temp.path / "After.olib").string());
            check(notes.empty() && readPackage(temp.path / "After.olib", {}).plugins.at(0).builds.at(0).sha256 == bundleDigest(build),
                  "and once rebuilt, as rebuilt: " + (notes.empty() ? std::string() : notes[0]));
        }
        // Device D: the plugin is not where the Performance last found it, so it opens from the store.
        {
            device(temp.path / "d");
            Session d(48000, 64);
            d.installArchive(archive.string(), false, {}, true);
            const auto performance = items(d, "performance").at(0);
            const auto doc = fs::path(performance.path) / "performance.json";
            auto text = read(doc);
            for(auto at = text.find(fixture); at != std::string::npos; at = text.find(fixture)) text.replace(at, std::strlen(fixture), "/nowhere/Synth.clap");
            std::ofstream(doc, std::ios::binary | std::ios::trunc) << text;
            d.libraryRescan();
            d.loadPerformanceFromLibrary(performance.id);
            const auto nodes = d.graphState().topology.nodes;
            check(nodes.size() == 1 && nodes[0].plugin.path.find((temp.path / "d" / "Plugins").string()) == 0 && d.sessionStatus().failures.empty(), "the plugin opens from the store");
            // Installing again with the build there already adds nothing and says nothing.
            check(d.installArchive(archive.string(), false).notes.empty(), "the same build is not installed twice");
            const auto plugin = d.installedPlugins().at(0);
            check(plugin.open, "the store says the open Performance plays it");
            bool refused = false; try { d.removePlugin(plugin.format, plugin.id, plugin.version); } catch(const std::exception&) { refused = true; }
            check(refused && !d.installedPlugins().empty(), "removing a plugin the open Performance plays is refused");
            d.removePlugin(plugin.format, plugin.id, plugin.version, other);
            check(d.installedPlugins().at(0).platforms == std::vector<std::string>{platform()}, "one platform's build removed alone");
            fs::create_directories(temp.path / "d" / "Victim");
            bool climbed = false; try { d.removePlugin(plugin.format, plugin.id, plugin.version, platform() + "/../../../../Victim"); } catch(const std::exception&) { climbed = true; }
            check(climbed && fs::is_directory(temp.path / "d" / "Victim"), "a platform that climbs out of the store is refused, removing nothing");
            d.remove(nodes[0].id); d.commit();
            d.removePlugin(plugin.format, plugin.id, plugin.version);
            check(d.installedPlugins().empty(), "removed once nothing open plays it");
            const auto cutShort = temp.path / "d" / "Plugins" / "clap" / ".synth@1.removing";
            fs::create_directories(cutShort / "bundle");
            check(d.installedPlugins().empty() && !fs::exists(cutShort), "a removal cut short is cleared away");
            d.loadPerformanceFromLibrary(performance.id);
            check(d.sessionStatus().failures.size() == 1, "what used it opens with a stand-in");
            d.installArchive(archive.string(), false, {}, true);
            d.loadPerformanceFromLibrary(performance.id);
            check(d.sessionStatus().failures.empty(), "and plays again once it is installed again");
        }
        // Device G: a broken build in the store, where the Performance last found it, opens as a
        // stand-in, which plays nothing: the build can be removed.
        {
            device(temp.path / "g");
            Session g(48000, 64);
            g.installArchive(archive.string(), false, {}, true);
            const auto plugins = temp.path / "g" / "Plugins" / "clap";
            const auto stored = fs::directory_iterator(plugins)->path() / platform() / fs::path(fixture).filename();
            std::ofstream(stored, std::ios::binary | std::ios::trunc) << "not a plugin";
            const auto performance = items(g, "performance").at(0);
            const auto doc = fs::path(performance.path) / "performance.json";
            auto text = read(doc);
            for(auto at = text.find(fixture); at != std::string::npos; at = text.find(fixture, at + 1)) text.replace(at, std::strlen(fixture), stored.string());
            std::ofstream(doc, std::ios::binary | std::ios::trunc) << text;
            g.libraryRescan();
            g.loadPerformanceFromLibrary(performance.id);
            check(g.sessionStatus().failures.size() == 1 && !g.installedPlugins().at(0).open, "a stand-in for the store's broken build does not play it");
            const auto plugin = g.installedPlugins().at(0);
            g.removePlugin(plugin.format, plugin.id, plugin.version);
            check(g.installedPlugins().empty(), "so it is removed");
        }
        // Device H (RFC 0006): a plugin author signs a build with oscairn-sign; an export carries
        // that signature, and signs its items and the whole archive with a key of the user's.
        const auto signedArchive = temp.path / "Signed.olib";
        std::string authorKey, version;
        {
            device(temp.path / "h");
            const auto author = temp.path / "author.key";
            const auto made = run(tool + " key " + shell(author));
            authorKey = signing::loadKey(author).publicKey;
            check(made.first == 0 && made.second.find(signing::keyId(authorKey)) != std::string::npos && made.second.find(authorKey) != std::string::npos, "oscairn-sign makes a key and shows its ID and public key");
            const auto build = temp.path / "h" / "Synth.clap";
            fs::copy_file(fixture, build);
            Session h(48000, 64);
            h.add("synth", build.string(), "org.kick.test.synth"); h.commit();
            version = h.graphState().topology.nodes.at(0).plugin.version;
            check(run(tool + " build " + shell(author) + " clap org.kick.test.synth " + shell(version) + " " + platform() + " " + shell(build)).first == 0 &&
                  signing::readSignatures(build.string() + ".sig").size() == 1, "oscairn-sign signs a build, beside it");
            const auto mine = h.makeSigningKey("mine");
            struct stat st{};
            check(::stat((temp.path / "h" / "Keys" / "mine.key").c_str(), &st) == 0 && (st.st_mode & 0777) == 0600, "a key made in the data folder is readable only by the user");
            check(h.signingKeys().size() == 1 && h.signingKeys()[0].id == mine.id && mine.id == signing::keyId(mine.key), "the data folder's keys are listed by name, ID and public key");
            bool twice = false; try { h.makeSigningKey("mine"); } catch(const std::exception&) { twice = true; }
            check(twice, "a key is not made over another");
            h.savePerformanceToLibrary("Signed");
            check(h.exportPerformance(signedArchive.string(), false, {"mine", true, true}).empty(), "a signed export leaves nothing out");
            const auto packed = readPackage(signedArchive, temp.path / "h-out");
            check(packed.signature.key == mine.key && !packed.items.empty() && packed.items[0].signatures.size() == 1 && packed.items[0].signatures[0].key == mine.key,
                  "the export signs its items and the whole archive with the chosen key");
            check(packed.plugins.size() == 1 && packed.plugins[0].builds.size() == 1 && packed.plugins[0].builds[0].signatures.size() == 1 &&
                  packed.plugins[0].builds[0].signatures[0].key == authorKey, "the export carries the build's signature from beside it");
            h.exportPerformance((temp.path / "Plain.olib").string());
            const auto plain = readPackage(temp.path / "Plain.olib", {});
            check(plain.signature.key.empty() && plain.items[0].signatures.empty(), "an export is signed only when the exporter chooses");
            bool unnamed = false; try { h.exportPerformance((temp.path / "None.olib").string(), false, {"nobody", false, true}); } catch(const std::exception&) { unnamed = true; }
            check(unnamed && !fs::exists(temp.path / "None.olib"), "an export signed with a key that is not there is refused");
            // oscairn-sign: an archive signed afterwards, and its signatures checked.
            check(run(tool + " archive " + shell(author) + " " + shell(temp.path / "Plain.olib")).first == 0 && readPackage(temp.path / "Plain.olib", {}).signature.key == authorKey,
                  "oscairn-sign signs an archive whole");
            const auto verified = run(tool + " verify " + shell(signedArchive));
            check(verified.first == 0 && verified.second.find("archive " + packed.id + " " + mine.id) != std::string::npos &&
                  verified.second.find("build clap org.kick.test.synth " + version + " " + platform() + " " + signing::keyId(authorKey)) != std::string::npos,
                  "oscairn-sign says who signed what: " + verified.second);
            // --into: unpacked to check on a file system of the caller's, and cleaned after.
            const auto into = temp.path / "h" / "verify-into";
            const auto local = run(tool + " verify --into " + shell(into) + " " + shell(signedArchive));
            check(local.first == 0 && local.second == verified.second && !fs::exists(into), "oscairn-sign verifies into a folder of the caller's");
            bool refusedInto = false;
            fs::create_directories(into);
            refusedInto = run(tool + " verify --into " + shell(into) + " " + shell(signedArchive)).first != 0;
            check(refusedInto, "a --into folder that exists already is refused");
            fs::remove(into);
            // The manifest's name changed under the archive's signature, every other entry as it was.
            {
                mz_zip_archive in{}, out{};
                mz_zip_reader_init_file(&in, signedArchive.c_str(), 0);
                mz_zip_writer_init_file(&out, (temp.path / "Changed.olib").c_str(), 0);
                for(mz_uint i = 0; i < mz_zip_reader_get_num_files(&in); ++i)
                {
                    mz_zip_archive_file_stat st; mz_zip_reader_file_stat(&in, i, &st);
                    if(std::string(st.m_filename) != "library.json") { mz_zip_writer_add_from_zip_reader(&out, &in, i); continue; }
                    std::string m(size_t(st.m_uncomp_size), '\0'); mz_zip_reader_extract_to_mem(&in, i, m.data(), m.size(), 0);
                    m.replace(m.find("\"Signed\""), 8, "\"Signer\"");
                    mz_zip_writer_add_mem(&out, "library.json", m.data(), m.size(), MZ_DEFAULT_COMPRESSION);
                }
                mz_zip_writer_finalize_archive(&out); mz_zip_writer_end(&out); mz_zip_reader_end(&in);
            }
            check(run(tool + " verify " + shell(temp.path / "Changed.olib")).first != 0, "oscairn-sign refuses an archive changed after it was signed");
            h.exportPerformance((temp.path / "Unsigned.olib").string());
        }
        // Device I: the build's publisher unknown, then trusted for plugins; data installs whether signed or not.
        {
            device(temp.path / "i");
            Session i(48000, 64);
            const auto unknown = i.archiveContents(signedArchive.string());
            check(unknown.code.size() == 1 && unknown.code[0].status == "unknown" && unknown.code[0].key == signing::keyId(authorKey) && unknown.signer.status == "unknown",
                  "a build signed by a key not trusted here is unknown");
            std::string why; try { i.installArchive(signedArchive.string(), false); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("by an unknown key " + signing::keyId(authorKey)) != std::string::npos, "and refused without a yes: " + why);
            i.setTrustedKey({"", authorKey, "Author", {"plugins"}});
            const auto trusted = i.archiveContents(signedArchive.string());
            check(trusted.code.at(0).status == "trusted" && trusted.code[0].keyName == "Author", "once its key is trusted for plugins, it is trusted");
            const auto installed = i.installArchive(signedArchive.string(), false);
            check(installed.added == 1 && installed.notes.empty() && installed.code.at(0).status == "trusted", "and installs without asking");
            check(signing::readSignatures(temp.path / "i" / "Plugins" / "clap" / "org.kick.test.synth@1" / platform() / "Synth.clap.sig").size() == 1,
                  "the store keeps the build's signature beside it");
            check(i.exportLibraryItem(items(i, "performance").at(0).id, (temp.path / "Again.signed.olib").string()).empty() &&
                  readPackage(temp.path / "Again.signed.olib", {}).plugins.at(0).builds.at(0).signatures.at(0).key == authorKey, "a re-export carries the signature from the store");
            const auto plain = i.archiveContents((temp.path / "Unsigned.olib").string());
            check(plain.code.empty() && plain.signer.status == "unsigned", "an unsigned archive whose build is here brings no code");
            check(i.installArchive((temp.path / "Unsigned.olib").string(), true).added == 1, "unsigned data installs without asking");
        }
        // Device J: the publisher's key revoked: its build is never installed, the items still are.
        {
            device(temp.path / "j");
            Session j(48000, 64);
            j.setTrustedKey({"", authorKey, "Author", {"plugins"}, true});
            check(j.archiveContents(signedArchive.string()).code.at(0).status == "revoked", "a revoked key's build is said to be revoked");
            const auto installed = j.installArchive(signedArchive.string(), false, {}, true);
            check(installed.added == 1 && installed.code.at(0).status == "revoked" && installed.notes.size() == 1 && installed.notes[0].find("revoked") != std::string::npos &&
                  !fs::exists(temp.path / "j" / "Plugins"), "a revoked key's build is refused even with a yes, and noted");
        }
        // Built-in publisher keys (RFC 0031, decided 2026-11-11): the official publisher's is
        // trusted out of the box, listed without being in trust.json. Its private key is no one
        // else's, so the installs here stand the author's key in for it (setBuiltInKeys).
        {
            const std::string official = "db87e97dba5b24564c782f473c543681bd87c0818bc2e41f37d1f100f3360d69";
            const Control::TrustedKey builtIn{"", authorKey, "Author", {"apps", "content", "plugins"}};
            auto refusal = [](Session& s, const fs::path& archive) { std::string why; try { s.installArchive(archive.string(), false); } catch(const std::exception& e) { why = e.what(); } return why; };
            {
                device(temp.path / "builtin");
                Session k(48000, 64);
                const auto keys = k.trustedKeys();
                check(std::any_of(keys.begin(), keys.end(), [&](const auto& t) { return t.key == official && t.id == "b284be6bea2fadb1" && t.name == "Oscairn" && !t.revoked &&
                                                                                       t.scope == std::vector<std::string>{"apps", "content", "plugins"}; }) &&
                          !fs::exists(temp.path / "builtin" / "trust.json"),
                      "a fresh device trusts the official publisher for plugins, apps and content, built in, not written to trust.json");
                check(refusal(k, signedArchive).find("by an unknown key " + signing::keyId(authorKey)) != std::string::npos, "a build signed by another key still asks");
                k.setBuiltInKeys({builtIn});
                const auto installed = k.installArchive(signedArchive.string(), false);
                check(installed.added == 1 && installed.code.at(0).status == "trusted" && installed.code[0].keyName == "Author" && !fs::exists(temp.path / "builtin" / "trust.json"),
                      "a build by a built-in key installs with no yes, and nothing is written to trust.json");
                check(refusal(k, temp.path / "Changed.olib").find("signature") != std::string::npos, "an archive that does not verify is refused all the same: " + refusal(k, temp.path / "Changed.olib"));
            }
            {
                device(temp.path / "builtin-revoked");
                Session k(48000, 64);
                k.setBuiltInKeys({builtIn});
                k.setTrustedKey({"", authorKey, "Author", {"plugins"}, true});
                const auto installed = k.installArchive(signedArchive.string(), false, {}, true);
                check(installed.code.at(0).status == "revoked" && !fs::exists(temp.path / "builtin-revoked" / "Plugins"), "revoked in trust.json, the built-in key's build is refused");
            }
            {
                device(temp.path / "builtin-removed");
                Session k(48000, 64);
                k.setBuiltInKeys({builtIn});
                k.removeTrustedKey(signing::keyId(authorKey));
                const auto keys = k.trustedKeys();
                check(std::any_of(keys.begin(), keys.end(), [&](const auto& t) { return t.key == authorKey && t.scope.empty(); }) &&
                          refusal(k, signedArchive).find("by an unknown key") != std::string::npos,
                      "removed, it stays in trust.json trusted for nothing, and its build asks");
            }
            {
                device(temp.path / "builtin-profile");
                Session k(48000, 64);
                k.setBuiltInKeys({builtIn});
                k.setDeviceRepositories({}, true);
                const auto keys = k.trustedKeys();
                check(std::none_of(keys.begin(), keys.end(), [&](const auto& t) { return t.key == authorKey; }) && refusal(k, signedArchive).find("by an unknown key") != std::string::npos,
                      "a profile hiding the official repository leaves the built-in publisher key out, and its build asks");
            }
        }
        // Device P: a plugin package, packed by oscairn-sign from a signed build, carries the plugin
        // and no items; it installs into the store, where a plugin scan looks, under the same
        // signature rules, and never as a library.
        {
            device(temp.path / "p");
            const auto author = temp.path / "author.key";
            const auto build = temp.path / "p-build" / "Synth.clap";
            fs::create_directories(build.parent_path());
            fs::copy_file(fixture, build);
            check(run(tool + " build " + shell(author) + " clap org.kick.test.synth " + shell(version) + " " + platform() + " " + shell(build)).first == 0, "the build is signed");
            const auto package = temp.path / "Synth.olib";
            const auto packed = run(tool + " plugin-package --license MIT --key " + shell(author) + " " + shell(package) + " clap org.kick.test.synth " + shell(version) + " 'KICK Synth' " + platform() + "=" + shell(build));
            check(packed.first == 0 && packed.second.find("with 1 builds, signed by " + signing::keyId(authorKey)) != std::string::npos, "oscairn-sign packs a plugin package: " + packed.second);
            const auto wrongPlatform = run(tool + " plugin-package --license MIT " + shell(temp.path / "Wrong.olib") + " clap org.kick.test.synth " + shell(version) + " Synth " + other + "=" + shell(build) + " 2>&1");
            check(wrongPlatform.first != 0 && wrongPlatform.second.find("not over this build") != std::string::npos && !fs::exists(temp.path / "Wrong.olib"),
                  "a build whose signature is for another platform is not packed: " + wrongPlatform.second);
            const auto read = readPackage(package, {});
            check(read.kind == "plugin" && read.items.empty() && read.plugins.size() == 1 && read.plugins[0].name == "KICK Synth" && read.signature.key == authorKey &&
                  read.plugins[0].builds.at(0).signatures.at(0).key == authorKey, "it holds the plugin, the build's signature and the whole archive's");
            Session p(48000, 64);
            const auto contents = p.archiveContents(package.string());
            check(contents.kind == "plugin" && contents.items.empty() && contents.code.size() == 1 && contents.code[0].name == "KICK Synth" && contents.code[0].status == "unknown",
                  "its contents list the build it adds, by a key not trusted here");
            std::string why; try { p.installArchive(package.string(), true); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("by an unknown key") != std::string::npos && !fs::exists(temp.path / "p" / "Plugins"), "refused without a yes, installing nothing: " + why);
            p.setTrustedKey({"", authorKey, "Author", {"plugins"}});
            const auto installed = p.installArchive(package.string(), true);
            check(installed.added == 0 && installed.notes.empty() && installed.code.size() == 1 && installed.code[0].status == "trusted", "installed once its key is trusted");
            const auto listed = p.installedPlugins();
            check(listed.size() == 1 && listed[0].name == "KICK Synth" && listed[0].platforms == std::vector<std::string>{platform()} && listed[0].usedBy.empty(),
                  "its build is in the store, used by nothing yet");
            const auto folders = pluginStoreFolders();
            check(folders.size() == 1 && fs::exists(folders[0] / "Synth.clap") && fs::exists(folders[0] / "Synth.clap.sig"), "where a plugin scan looks, with its signature beside it");
            check(p.libraries().empty() && !fs::exists(temp.path / "p" / "Libraries" / read.id), "asked as a library, it is not one");
            p.add("synth", (folders[0] / "Synth.clap").string(), "org.kick.test.synth"); p.commit();
            check(p.sessionStatus().failures.empty() && p.graphState().topology.nodes.size() == 1, "the installed build plays");
        }
        // Device E (RFC 0025 step 5): a plugin package declares its build's estimate, as
        // oscairn-plugin-check measures it; a device that has not measured the plugin counts it at
        // that, not at the default. Installing what will not fit says so, and the library shows it.
        {
            device(temp.path / "e");
            const auto package = temp.path / "Estimated.olib";
            const auto packed = run(tool + " plugin-package --license MIT --estimate " + platform() + " 0.95 100663296 " + shell(package) + " clap org.kick.test.synth " + shell(version) + " Synth " + platform() + "=" + shell(fixture));
            const auto read = readPackage(package, {});
            check(packed.first == 0 && read.plugins.at(0).builds.at(0).cpu == .95 && read.plugins[0].builds[0].memory == 100663296, "the package carries its build's estimate");
            const auto wild = run(tool + " plugin-package --license MIT --estimate " + platform() + " 99 1 " + shell(temp.path / "Wild.olib") + " clap org.kick.test.synth " + shell(version) + " Synth " + platform() + "=" + shell(fixture) + " 2>&1");
            check(wild.first != 0 && wild.second.find("estimate is out of range") != std::string::npos && !fs::exists(temp.path / "Wild.olib"), "an estimate past any block is refused: " + wild.second);
            Session e(48000, 64);
            constexpr uint64_t MiB = 1ull << 20;
            e.setBudget({.9, 512 * MiB});
            e.installArchive(package.string(), true, {}, true);
            check(fs::exists(pluginStoreFolders().at(0) / "estimate.json"), "the estimate is kept beside the build in the store");
            const auto installed = e.installArchive(archive.string(), true, {}, true);
            const auto said = [&](const std::string& name) { return std::any_of(installed.notes.begin(), installed.notes.end(), [&](const auto& n) { return n.rfind(name + " does not fit this device: it needs 95% of the audio time where this device allows 90%", 0) == 0; }); };
            check(said("Set") && said("Bass"), "installing a Performance and a Sound that will not fit says so: " + (installed.notes.empty() ? std::string() : installed.notes[0]));
            Control::LibraryQuery installedItems; installedItems.source = e.libraries().at(0).id; // the Set library, not the user's own
            const auto listed = e.libraryQuery(installedItems);
            check(listed.items.size() == 2 && std::all_of(listed.items.begin(), listed.items.end(), [](const auto& i) { return i.unfit.find("needs 95% of the audio time") == 0; }),
                  "the library shows each as not fitting, before it is opened");
            const auto set = std::find_if(listed.items.begin(), listed.items.end(), [](const auto& i) { return i.kind == "performance"; });
            const auto estimate = e.estimate(listed.items.at(0).path);
            check(estimate.plugins.size() == 1 && estimate.plugins[0].cpu == .95 && estimate.plugins[0].memory == 100663296 && !estimate.plugins[0].measured,
                  "counted at the package's estimate, still marked unmeasured");
            std::string why; try { if(set != listed.items.end()) e.loadPerformanceFromLibrary(set->id); } catch(const std::exception& x) { why = x.what(); }
            check(why.find("needs 95% of the audio time") != std::string::npos, "and opening it is refused: " + why);
            e.setBudget({2, 512 * MiB});
            const auto roomy = e.libraryQuery(installedItems);
            check(std::all_of(roomy.items.begin(), roomy.items.end(), [](const auto& i) { return i.unfit.empty(); }), "on a device it fits, nothing is shown");
        }
        // Device S: one bundle holding two plugins, Oscairn's Splitters (Split and Join): packed once,
        // each plugin signed, both installed into the store and listed, and both play from it.
        if(splitters)
        {
            device(temp.path / "s");
            const auto author = temp.path / "author.key";
            const auto build = temp.path / "s-build" / "oscairn-splitters.clap";
            fs::create_directories(build.parent_path());
            fs::copy(splitters, build, fs::copy_options::recursive);
            const auto signedBoth = run(tool + " build " + shell(author) + " clap org.oscairn.split,org.oscairn.join 0.1.0 " + platform() + " " + shell(build));
            check(signedBoth.first == 0 && signing::readSignatures(build.string() + ".sig").size() == 2, "the bundle is signed for each plugin: " + signedBoth.second);
            const auto package = temp.path / "Splitters.olib";
            // Under the AGPL: its license text and source travel with it (§6), and it is refused without them.
            const auto copying = temp.path / "s-build" / "COPYING";
            std::ofstream(copying) << "GNU AFFERO GENERAL PUBLIC LICENSE\n";
            const auto pack = [&](const fs::path& to, const std::string& options) {
                return run(tool + " plugin-package --key " + shell(author) + " --license AGPL-3.0-or-later " + options + " --plugin org.oscairn.split 'Oscairn Split' --plugin org.oscairn.join 'Oscairn Join' " +
                           shell(to) + " clap org.oscairn.splitters 0.1.0 'Oscairn Splitters' " + platform() + "=" + shell(build) + " 2>&1");
            };
            const auto noSource = pack(temp.path / "Without.olib", "--license-file " + shell(copying));
            check(noSource.first != 0 && noSource.second.find("carries no source") != std::string::npos && !fs::exists(temp.path / "Without.olib"), "an AGPL plugin package without its source is refused: " + noSource.second);
            const auto noText = pack(temp.path / "Without.olib", "--source https://github.com/oscairn/oscairn v0.1.0");
            check(noText.first != 0 && noText.second.find("no license texts") != std::string::npos && !fs::exists(temp.path / "Without.olib"), "an AGPL plugin package without its license text is refused: " + noText.second);
            const auto packed = pack(package, "--license-file " + shell(copying) + " --source https://github.com/oscairn/oscairn 0123456789abcdef0123456789abcdef01234567");
            check(packed.first == 0 && packed.second.find("packed clap org.oscairn.split, org.oscairn.join 0.1.0 with 1 builds") != std::string::npos, "oscairn-sign packs a bundle with two plugins: " + packed.second);
            const auto read = readPackage(package, {});
            check(read.id == "org.oscairn.splitters" && read.plugins.size() == 2 && read.plugins[0].builds.at(0).path == read.plugins[1].builds.at(0).path &&
                  read.plugins[0].builds[0].signatures.size() == 1 && read.plugins[1].builds[0].signatures.size() == 1, "the bundle is in it once, listed and signed for each plugin");
            check(read.license == "AGPL-3.0-or-later" && read.sourceUrl == "https://github.com/oscairn/oscairn" && read.sourceRevision == "0123456789abcdef0123456789abcdef01234567" &&
                  read.files.count("Licenses/COPYING"), "it carries its license, its license text and its source");
            mz_zip_archive z{}; mz_zip_reader_init_file(&z, package.c_str(), 0);
            const auto entries = mz_zip_reader_get_num_files(&z); mz_zip_reader_end(&z);
            check(entries == 4, "the archive holds the bundle once, and the license text: " + std::to_string(entries) + " entries");
            // A signature over another ID, such as a preset-discovery provider's, is not packed.
            const auto provider = temp.path / "s-provider" / "oscairn-splitters.clap";
            fs::create_directories(provider.parent_path());
            fs::copy(build, provider);
            run(tool + " build " + shell(author) + " clap org.oscairn.split,org.oscairn.join,org.oscairn.splitters.presets 0.1.0 " + platform() + " " + shell(provider));
            const auto extra = run(tool + " plugin-package --license MIT --plugin org.oscairn.split Split --plugin org.oscairn.join Join " + shell(temp.path / "Provider.olib") + " clap org.oscairn.splitters 0.1.0 Splitters " + platform() + "=" + shell(provider) + " 2>&1");
            check(extra.first != 0 && extra.second.find("not over this build of org.oscairn.split, org.oscairn.join") != std::string::npos && !fs::exists(temp.path / "Provider.olib"),
                  "a signature over an ID not packed is refused: " + extra.second);
            Session s(48000, 64);
            s.setTrustedKey({"", authorKey, "Author", {"plugins"}});
            const auto contents = s.archiveContents(package.string());
            check(contents.license == "AGPL-3.0-or-later" && contents.sourceUrl == "https://github.com/oscairn/oscairn" && contents.sourceRevision.size() == 40, "its contents give its license and source");
            const auto installed = s.installArchive(package.string(), false);
            check(installed.notes.empty() && installed.code.size() == 2 && installed.code[0].status == "trusted" && installed.code[1].status == "trusted", "both plugins install, each trusted");
            const auto listed = s.installedPlugins();
            check(listed.size() == 2 && listed[0].id == "org.oscairn.join" && listed[1].id == "org.oscairn.split" && listed[0].platforms == std::vector<std::string>{platform()} &&
                  listed[1].platforms == std::vector<std::string>{platform()}, "the store lists each plugin");
            const auto stored = [&](const char* id) { return temp.path / "s" / "Plugins" / "clap" / pluginFolderName(id, "0.1.0") / platform() / build.filename(); };
            const auto split = stored("org.oscairn.split"), join = stored("org.oscairn.join");
            check(fs::exists(split) && fs::exists(join) && bundleDigest(split) == bundleDigest(build) && bundleDigest(join) == bundleDigest(build), "each has the whole bundle in the store");
            check(fs::exists(split.parent_path().parent_path() / "Licenses" / "COPYING") && fs::exists(join.parent_path().parent_path() / "Licenses" / "COPYING"), "and the license text beside it");
            s.add("split", split.string(), "org.oscairn.split"); s.add("join", join.string(), "org.oscairn.join"); s.commit();
            check(s.sessionStatus().failures.empty() && s.graphState().topology.nodes.size() == 2, "both load from the store");
        }
        // Device V: the plugins in one bundle at their own versions (setBfree's organ and Leslie),
        // each given as <id>@<version>: signed, packed, installed and listed each at its own.
        if(splitters)
        {
            device(temp.path / "v");
            const auto author = temp.path / "author.key";
            const auto build = temp.path / "v-build" / "oscairn-splitters.clap";
            fs::create_directories(build.parent_path());
            fs::copy(splitters, build, fs::copy_options::recursive);
            const auto signedBoth = run(tool + " build " + shell(author) + " clap org.oscairn.split,org.oscairn.join@0.2.1 0.4.4 " + platform() + " " + shell(build));
            check(signedBoth.first == 0 && signedBoth.second.find("org.oscairn.split 0.4.4") != std::string::npos && signedBoth.second.find("org.oscairn.join 0.2.1") != std::string::npos,
                  "the bundle is signed for each plugin at its own version: " + signedBoth.second);
            const auto package = temp.path / "Versions.olib";
            const auto packed = run(tool + " plugin-package --key " + shell(author) + " --license MIT --plugin org.oscairn.split Split --plugin org.oscairn.join@0.2.1 Join " +
                                    shell(package) + " clap org.oscairn.splitters 0.4.4 Splitters " + platform() + "=" + shell(build) + " 2>&1");
            check(packed.first == 0 && packed.second.find("packed clap org.oscairn.split, org.oscairn.join@0.2.1 0.4.4 with 1 builds") != std::string::npos, "oscairn-sign packs them: " + packed.second);
            const auto read = readPackage(package, {});
            check(read.plugins.size() == 2 && read.plugins[0].version == "0.4.4" && read.plugins[1].version == "0.2.1" && read.plugins[0].builds.at(0).path == read.plugins[1].builds.at(0).path &&
                  read.plugins[0].builds[0].signatures.size() == 1 && read.plugins[1].builds[0].signatures.size() == 1, "each is listed and signed at its own version, the bundle once");
            Session s(48000, 64);
            s.setTrustedKey({"", authorKey, "Author", {"plugins"}});
            const auto installed = s.installArchive(package.string(), false);
            check(installed.notes.empty() && installed.code.size() == 2 && installed.code[0].status == "trusted" && installed.code[1].status == "trusted", "both install, each trusted");
            const auto listed = s.installedPlugins();
            check(listed.size() == 2 && listed[0].id == "org.oscairn.join" && listed[0].version == "0.2.1" && listed[1].id == "org.oscairn.split" && listed[1].version == "0.4.4", "the store lists each at its own version");
            const auto join = temp.path / "v" / "Plugins" / "clap" / pluginFolderName("org.oscairn.join", "0.2.1") / platform() / build.filename();
            const auto split = temp.path / "v" / "Plugins" / "clap" / pluginFolderName("org.oscairn.split", "0.4.4") / platform() / build.filename();
            check(bundleDigest(split) == bundleDigest(build) && bundleDigest(join) == bundleDigest(build), "each has the whole bundle in its own folder");
        }
        // Device L: an asset package (RFC 0009), packed by oscairn-sign from a folder: installed into
        // the asset store by hash with a read-only tree of links, found by path and by its root;
        // a newer version replaces it; a tampered file is refused; a plugin package needing it says
        // so while it is missing.
        {
            device(temp.path / "l");
            const auto library = temp.path / "l-library";
            fs::create_directories(library / "patches" / "Leads"); fs::create_directories(library / "wavetables");
            std::ofstream(library / "patches" / "Leads" / "Saw Lead.fxp", std::ios::binary) << "CcnK patch bytes";
            std::ofstream(library / "wavetables" / "basic.wt", std::ios::binary) << "vawt table bytes";
            std::ofstream(library / ".DS_Store") << "hidden";
            const auto notice = temp.path / "NOTICE"; std::ofstream(notice) << "Surge factory content";
            const auto pack = [&](const fs::path& to, const std::string& version) {
                return run(tool + " asset-package --key " + shell(temp.path / "author.key") + " --license GPL-3.0-or-later --license-file " + shell(notice) + " " + shell(to) +
                           " org.oscairn.analog-factory " + version + " 'Analog factory content' " + shell(library) + " 2>&1");
            };
            const auto packed = pack(temp.path / "Factory-1.0.olib", "1.0");
            check(packed.first == 0 && packed.second.find("packed asset library org.oscairn.analog-factory 1.0 with 2 files") != std::string::npos, "oscairn-sign packs an asset package: " + packed.second);
            const auto factory = readPackage(temp.path / "Factory-1.0.olib", {});
            check(factory.kind == "asset" && factory.assetVersion == "1.0" && factory.assets.size() == 2 && factory.assets[0].path == "patches/Leads/Saw Lead.fxp" && factory.assets[0].size == 16 &&
                  factory.assets[0].sha256 == assets::sha256("CcnK patch bytes") && factory.license == "GPL-3.0-or-later" && factory.files.count("Licenses/NOTICE"), "it lists each file by path, SHA-256 and size");
            // A plugin package that needs it, installed first: it says the library is missing.
            const auto needs = temp.path / "l-build" / "Analog.clap";
            fs::create_directories(needs.parent_path()); fs::copy_file(fixture, needs);
            check(run(tool + " plugin-package --license MIT --asset-dependency org.oscairn.analog-factory 1.0 " + shell(temp.path / "Analog.olib") + " clap org.kick.test.synth 1 Analog " + platform() + "=" + shell(needs)).first == 0,
                  "a plugin package names its asset dependency");
            check(readPackage(temp.path / "Analog.olib", {}).assetDependencies.size() == 1, "and carries it");
            Session l(48000, 64);
            const auto early = l.installArchive((temp.path / "Analog.olib").string(), false, {}, true);
            check(early.notes.size() == 1 && early.notes[0].find("needs the asset library org.oscairn.analog-factory 1.0 or later, which is not installed") != std::string::npos,
                  "installed from a file, it says its asset library is missing: " + (early.notes.empty() ? std::string() : early.notes[0]));
            const auto installed = l.installArchive((temp.path / "Factory-1.0.olib").string(), false);
            const auto tree = temp.path / "l" / "AssetLibraries" / "org.oscairn.analog-factory" / "1.0";
            struct stat a{}, b{};
            ::stat((tree / "wavetables" / "basic.wt").c_str(), &a); ::stat((temp.path / "l" / "assets" / "store" / assets::sha256("vawt table bytes")).c_str(), &b);
            check(installed.added == 2 && fs::exists(tree / "patches" / "Leads" / "Saw Lead.fxp") && !fs::exists(tree / ".DS_Store") && a.st_ino == b.st_ino && a.st_ino != 0 && (a.st_mode & 0222) == 0,
                  "installed by hash, its tree read-only links to the store's files");
            assets::Store store(temp.path / "l" / "assets");
            kick::clap::AssetReference ref{"org.oscairn.analog-factory", "wavetables/basic.wt", ""};
            check(store.resolve(ref) == fs::canonical(tree / "wavetables" / "basic.wt") && ref.sha256 == assets::sha256("vawt table bytes"), "a reference resolves by the library's ID and path");
            check(store.libraryRoot("org.oscairn.analog-factory", "1.0") == tree.string() && store.libraryRoot("org.oscairn.analog-factory", "").size() && store.libraryRoot("org.oscairn.analog-factory", "1.1").empty() &&
                  store.libraryRoot("org.oscairn.other", "").empty(), "its root, when a version at least the one asked for is installed");
            check(l.installArchive((temp.path / "Analog.olib").string(), false, {}, true).notes.empty(), "with it installed, the plugin package says nothing missing");
            // A newer version: the new tree replaces the old.
            std::ofstream(library / "wavetables" / "basic.wt", std::ios::binary | std::ios::trunc) << "vawt table bytes, fixed";
            check(pack(temp.path / "Factory-1.1.olib", "1.1").first == 0 && l.installArchive((temp.path / "Factory-1.1.olib").string(), false).added == 2, "a newer version installs");
            const auto newer = tree.parent_path() / "1.1";
            check(fs::exists(newer / "wavetables" / "basic.wt") && !fs::exists(tree) && store.libraryRoot("org.oscairn.analog-factory", "1.1") == newer.string() && store.libraryVersion("org.oscairn.analog-factory") == "1.1",
                  "and replaces the old tree");
            // A tampered file: the archive copied with one asset's bytes changed, its manifest as signed.
            {
                mz_zip_archive in{}, out{};
                mz_zip_reader_init_file(&in, (temp.path / "Factory-1.1.olib").c_str(), 0);
                mz_zip_writer_init_file(&out, (temp.path / "Tampered.olib").c_str(), 0);
                for(mz_uint i = 0; i < mz_zip_reader_get_num_files(&in); ++i)
                {
                    mz_zip_archive_file_stat st; mz_zip_reader_file_stat(&in, i, &st);
                    if(std::string(st.m_filename) != "Assets/wavetables/basic.wt") { mz_zip_writer_add_from_zip_reader(&out, &in, i); continue; }
                    const std::string changed = "vawt table bytes, FIXED";
                    mz_zip_writer_add_mem(&out, st.m_filename, changed.data(), changed.size(), MZ_DEFAULT_COMPRESSION);
                }
                mz_zip_writer_finalize_archive(&out); mz_zip_writer_end(&out); mz_zip_reader_end(&in);
            }
            std::string why; try { l.installArchive((temp.path / "Tampered.olib").string(), false); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("not what the manifest says: Assets/wavetables/basic.wt") != std::string::npos, "a tampered asset is refused: " + why);
            check(store.libraryVersion("org.oscairn.analog-factory") == "1.1" && read(newer / "wavetables" / "basic.wt") == "vawt table bytes, fixed", "and the installed library is as it was");
            // A preset provider finds an installed library's folder through oscairn.assets'
            // library_root on the indexer (the fixture asks for org.kick.test.factory 1.0 or later)
            // and its presets are indexed; before it is installed there is no such location.
            auto presets = [&] {
                std::string names;
                for(const auto& plugin : inspectPlugin(fixture, true).plugins)
                    if(plugin.id == "org.kick.test.midi") for(const auto& p : plugin.presets) names += p.name + ";";
                return names;
            };
            check(presets() == "Quiet;Bright;", "without the library, only the plugin's own presets: " + presets());
            fs::create_directories(temp.path / "l-presets" / "Pads");
            std::ofstream(temp.path / "l-presets" / "Pads" / "Warm.kickpreset") << "preset";
            check(run(tool + " asset-package --license MIT " + shell(temp.path / "Presets.olib") + " org.kick.test.factory 1.2 Presets " + shell(temp.path / "l-presets")).first == 0 &&
                  l.installArchive((temp.path / "Presets.olib").string(), false).added == 1, "an asset library of presets installs");
            const auto found = presets();
            check(found == "Quiet;Bright;Warm;", "with it, the provider's location from library_root is indexed: " + found);
            // The same through a node's presets, which without a scan helper index in this process.
            const auto node = l.add("presets", fixture, "org.kick.test.midi");
            std::string listed;
            for(const auto& p : l.presets(node)) listed += p.name + ";";
            check(listed == "Quiet;Bright;Warm;", "a node's presets come from the installed library too: " + listed);
        }
        // Device R (RFC 0009 step 4): redistribution. An asset library under an SPDX license goes into
        // an export made from it, its license texts with it; one under the publisher's own terms
        // (LicenseRef-, whose text it must carry, RFC 0031) stays out unless its package says it may
        // be passed on, and the export says so; the destination then needs it installed.
        {
            device(temp.path / "r");
            const auto freeFiles = temp.path / "r-free", vendorFiles = temp.path / "r-vendor";
            fs::create_directories(freeFiles); fs::create_directories(vendorFiles);
            std::ofstream(freeFiles / "free.wav", std::ios::binary) << "free sample bytes";
            std::ofstream(vendorFiles / "piano.wav", std::ios::binary) << "vendor piano bytes";
            const auto terms = temp.path / "VENDOR-LICENSE", attribution = temp.path / "ATTRIBUTION";
            std::ofstream(terms) << "Not to be passed on"; std::ofstream(attribution) << "Free sample by its recordist";
            const auto refused = run(tool + " asset-package --license LicenseRef-Vendor " + shell(temp.path / "NoText.olib") + " org.test.vendor 2 Vendor " + shell(vendorFiles) + " 2>&1");
            check(refused.first != 0 && refused.second.find("carries no license text") != std::string::npos && !fs::exists(temp.path / "NoText.olib"),
                  "a LicenseRef- asset package without its license text is refused: " + refused.second);
            check(run(tool + " asset-package --license CC-BY-4.0 --license-file " + shell(attribution) + " " + shell(temp.path / "Free.olib") + " org.test.free 1 Free " + shell(freeFiles)).first == 0 &&
                  run(tool + " asset-package --license LicenseRef-Vendor --license-file " + shell(terms) + " " + shell(temp.path / "Vendor.olib") + " org.test.vendor 2 'Vendor piano' " + shell(vendorFiles)).first == 0 &&
                  run(tool + " asset-package --license LicenseRef-Vendor --license-file " + shell(terms) + " --redistribute yes " + shell(temp.path / "Shared.olib") + " org.test.shared 1 Shared " + shell(vendorFiles)).first == 0,
                  "oscairn-sign packs them");
            check(!readPackage(temp.path / "Free.olib", {}).redistribute && !readPackage(temp.path / "Vendor.olib", {}).redistribute && readPackage(temp.path / "Shared.olib", {}).redistribute == std::optional<bool>(true),
                  "only a package that says so declares redistribute");
            check(redistributable("CC-BY-4.0", {}) && redistributable("MIT OR Apache-2.0", {}) && !redistributable("LicenseRef-Vendor", {}) && redistributable("LicenseRef-Vendor", true) && !redistributable("MIT", false),
                  "an SPDX license lets it be passed on, the publisher's own terms do not, and a package's word decides");
            Session m(48000, 64);
            m.installArchive((temp.path / "Free.olib").string(), false); m.installArchive((temp.path / "Vendor.olib").string(), false);
            const auto freeTerms = m.assets().libraryTerms("org.test.free"), vendorTerms = m.assets().libraryTerms("org.test.vendor");
            check(freeTerms.redistribute && freeTerms.license == "CC-BY-4.0" && read(freeTerms.licenses / "ATTRIBUTION") == "Free sample by its recordist" && !vendorTerms.redistribute && !freeTerms.dependency,
                  "the device keeps each library's terms and license texts");
            const auto synth = m.add("synth", fixture, "org.kick.test.synth");
            const auto part = m.addPart("Keys"); m.insertIntoPart(part, 0, synth); m.commit();
            const auto keysId = m.savePartAsSound(part, "Keys").id;
            const auto doc = fs::path(m.libraryQuery({}).items.front().path) / "sound.json";
            auto text = read(doc);
            const auto freeHash = assets::sha256("free sample bytes"), vendorHash = assets::sha256("vendor piano bytes");
            text.replace(text.find("\"assets\":[]"), 11, "\"assets\":[{\"library\":\"org.test.free\",\"path\":\"free.wav\",\"sha256\":\"" + freeHash +
                         "\"},{\"library\":\"org.test.vendor\",\"path\":\"piano.wav\",\"sha256\":\"" + vendorHash + "\"}]");
            std::ofstream(doc, std::ios::binary | std::ios::trunc) << text;
            m.libraryRescan();
            const auto notes = m.exportLibraryItem(keysId, (temp.path / "Keys.olib").string());
            const auto keys = readPackage(temp.path / "Keys.olib", {});
            check(notes.size() == 1 && notes[0].find("1 file of the asset library org.test.vendor 2 was left out: its license (LicenseRef-Vendor) does not let it be passed on") != std::string::npos,
                  "the export says what it left out: " + (notes.empty() ? std::string() : notes[0]));
            check(keys.files.count("Assets/" + freeHash.substr(0, 16) + "/free.wav") && !keys.files.count("Assets/" + vendorHash.substr(0, 16) + "/piano.wav") &&
                  keys.files.count("Licenses/org.test.free/ATTRIBUTION"), "the free file goes in with its attribution, the vendor's does not");
            device(temp.path / "r2");
            Session m2(48000, 64);
            const auto bare = m2.installArchive((temp.path / "Keys.olib").string(), false, {}, true);
            check(bare.notes.size() == 1 && bare.notes[0].find("not in the archive or on this device: piano.wav") != std::string::npos, "a device without the vendor library is told");
            m2.installArchive((temp.path / "Vendor.olib").string(), false);
            kick::clap::AssetReference piano{"org.test.vendor", "piano.wav", vendorHash}, free{"org.test.free", "free.wav", freeHash};
            check(!m2.assets().resolve(piano).empty() && !m2.assets().resolve(free).empty(), "with it installed both references resolve, the free one by its bytes");
        }
        // Device Q (RFC 0009 step 4): garbage collection from explicit roots. Collection removes the
        // asset libraries a plugin package brought that nothing uses, and stored files no library
        // links: never one a library item, the open Performance, a Performance saved outside the
        // library (also while its drive is away) or a plugin package in the store still uses, nor
        // one installed by hand. Uninstalling a used library is refused, naming what uses it.
        if(sampler)
        {
            device(temp.path / "q");
            Session q(48000, 64);
            auto& store = q.assets();
            int made = 0;
            auto install = [&](const std::string& id, bool dependency) {
                const auto from = temp.path / ("q-" + id); fs::create_directories(from);
                writeWavFile(from / "tone.wav", std::vector<float>(480, .1f * float(++made)), 1, 48000);
                std::ofstream(from / "notes.txt") << id;
                std::vector<assets::Store::LibraryFile> files;
                for(const auto* f : {"tone.wav", "notes.txt"}) files.push_back({f, assets::sha256File(from / f), from / f});
                store.installLibrary(id, "1", id, files, {"MIT", true, dependency, {}}); // as a plugin package's install (dependency) or one by hand leaves it
            };
            for(const auto* id : {"org.test.unused", "org.test.item", "org.test.gig", "org.test.needed"}) install(id, true);
            install("org.test.chosen", false);
            const auto blob = [&](const std::string& id) { return fs::exists(store.root() / "store" / assets::sha256(id)); }; // each notes.txt holds its library's ID
            // A library Sound uses org.test.item.
            const auto synth = q.add("synth", fixture, "org.kick.test.synth");
            const auto part = q.addPart("Keys"); q.insertIntoPart(part, 0, synth); q.commit();
            q.savePartAsSound(part, "Keys");
            const auto doc = fs::path(q.libraryQuery({}).items.front().path) / "sound.json";
            auto text = read(doc);
            text.replace(text.find("\"assets\":[]"), 11, "\"assets\":[{\"library\":\"org.test.item\",\"path\":\"notes.txt\",\"sha256\":\"" + assets::sha256("org.test.item") + "\"}]");
            std::ofstream(doc, std::ios::binary | std::ios::trunc) << text;
            q.libraryRescan();
            q.remove(synth); q.commit();
            // The Legacy Sampler plays org.test.gig, and the Performance is saved outside the library.
            const auto player = q.add("player", sampler);
            q.selectSound(player, q.prepareSound(player, "Gig", {}, {}, {}, "<region> sample=tone.wav\n", "org.test.gig", ""));
            q.commit();
            const auto drive = temp.path / "q-drive", gig = drive / "Shows" / "Gig";
            fs::create_directories(gig.parent_path());
            q.savePerformance(gig.string());
            check(read(gig / "performance.json").find("org.test.gig") != std::string::npos, "the saved Performance lists the library");
            // A plugin package needing org.test.needed.
            const auto build = temp.path / "q-build" / "Needs.clap";
            fs::create_directories(build.parent_path()); fs::copy_file(fixture, build);
            check(run(tool + " plugin-package --license MIT --asset-dependency org.test.needed 1 " + shell(temp.path / "Needs.olib") + " clap org.kick.test.needs 1 Needs " + platform() + "=" + shell(build)).first == 0,
                  "a plugin package needing a library");
            q.installArchive((temp.path / "Needs.olib").string(), false, {}, true);
            auto users = [&](const std::string& id) { std::string all; for(const auto& u : q.assetLibraryUsers(id)) all += u + ";"; return all; };
            check(users("org.test.item") == "Keys;" && users("org.test.needed") == "Needs 1;" && users("org.test.unused").empty(), "what uses each library: " + users("org.test.item") + " " + users("org.test.needed"));
            check(users("org.test.gig") == "the open Performance;" + gig.string() + ";", "the open Performance and the saved one use the sampler's: " + users("org.test.gig"));
            auto preview = q.collectAssets(false);
            check(preview.libraries == std::vector<std::string>{"org.test.unused"} && preview.bytes > 0 && blob("org.test.unused"), "a preview names only the unused dependency, removing nothing");
            std::string why; try { q.uninstallAssetLibrary("org.test.item"); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("the asset library org.test.item is used by Keys") != std::string::npos && fs::exists(temp.path / "q" / "AssetLibraries" / "org.test.item"), "uninstalling a used library is refused, naming what uses it: " + why);
            // Only the saved Performance holds org.test.gig now, also while its drive is away.
            q.remove(player); q.commit();
            check(users("org.test.gig") == gig.string() + ";", "the saved Performance alone: " + users("org.test.gig"));
            fs::rename(drive, temp.path / "q-away");
            check(q.collectAssets(true).libraries == std::vector<std::string>{"org.test.unused"} && !blob("org.test.unused") && blob("org.test.gig") && blob("org.test.item") && blob("org.test.needed") && blob("org.test.chosen"),
                  "collection removes the unused dependency and its stored files, keeping the rest: " + users("org.test.gig"));
            fs::rename(temp.path / "q-away", drive);
            // Its Performance deleted and the plugin package removed: then they go too.
            fs::remove_all(gig);
            q.removePlugin("clap", "org.kick.test.needs", "1");
            const auto second = q.collectAssets(true);
            check(second.libraries == std::vector<std::string>{"org.test.gig", "org.test.needed"} && !blob("org.test.gig") && !blob("org.test.needed") && blob("org.test.item") && blob("org.test.chosen") &&
                  store.libraryVersion("org.test.gig").empty(), "with their last root gone they are collected");
            kick::clap::AssetReference kept{"org.test.item", "notes.txt", assets::sha256("org.test.item")};
            check(!store.resolve(kept).empty(), "what a Sound uses still resolves");
            // One installed by hand stays until uninstalled, which takes its stored files with it.
            q.uninstallAssetLibrary("org.test.chosen");
            check(store.libraryVersion("org.test.chosen").empty() && !blob("org.test.chosen") && q.collectAssets(false).libraries.empty(), "a library installed by hand goes only when uninstalled");
            why.clear(); try { q.uninstallAssetLibrary("org.test.chosen"); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("no asset library org.test.chosen is installed") != std::string::npos, "and is then not there: " + why);
        }
        // A build path without its .clap or .lv2: the store keeps bundles by extension, so such a
        // build installs into the store and is never found again. readPackage refuses it.
        {
            const auto build = temp.path / "bare" / "Plugins" / "clap" / "x@1" / platform() / "x";
            fs::create_directories(build.parent_path());
            fs::copy_file(fixture, build);
            Package p; p.id = "bare"; p.name = "Bare";
            p.plugins.push_back({"clap", "x", "1", "X", {{platform(), "Plugins/clap/x@1/" + platform() + "/x", bundleDigest(build), {}}}});
            const auto bare = temp.path / "Bare.olib";
            writePackage(bare, p, temp.path / "bare");
            std::string why;
            try { readPackage(bare, temp.path / "bare-out"); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("not a .clap or .lv2 bundle") != std::string::npos && !fs::exists(temp.path / "bare-out"),
                  "a build that is a bare file, not a .clap or .lv2 bundle, is refused: " + why);
        }
        // Device K: an item whose ID needs escaping in JSON (an `"`). Installing into the user's own
        // storage rewrites a Performance's part pointers as text, and an exported item's ID is its
        // package's, so such an ID is refused, installing nothing.
        {
            device(temp.path / "k");
            const auto root = temp.path / "k-root";
            fs::create_directories(root / "Sounds" / "esc");
            fs::create_directories(root / "Performances" / "esc-perf");
            std::ofstream(root / "Sounds" / "esc" / "sound.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.sound","version":8,"info":{"id":"ab\"cd","name":"Bass","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")"
                << std::string(64, '0') << R"("},"nodes":[]})";
            std::ofstream(root / "Performances" / "esc-perf" / "performance.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.performance","version":13,"info":{"id":"perf","name":"Set","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")"
                << std::string(64, '0') << R"("},"nodes":[{"key":"n1","plugin":{"format":"clap","id":"x","version":"1","path":"p"},"sound":"ab\"cd"}]})";
            Package p; p.id = "k.esc"; p.name = "Escaped";
            p.items.push_back({"sound", "ab\"cd", "Bass", "Sounds/esc", {}});
            p.items.push_back({"performance", "perf", "Set", "Performances/esc-perf", {}});
            const auto escaped = temp.path / "Escaped.olib";
            writePackage(escaped, p, root);
            Session k(48000, 64);
            std::string why; try { k.installArchive(escaped.string(), false); } catch(const std::exception& e) { why = e.what(); }
            check(why.find("an item's kind or ID is wrong") != std::string::npos && items(k, "sound").empty() && items(k, "performance").empty(),
                  "an archive whose Sound's ID needs escaping is refused, installing nothing: " + why);
        }
        // A reverse-DNS library ID (RFC 0031's normal case) installs as a library: the folder is
        // the ID itself, so rescan lists it, its items show up, and it can be uninstalled.
        {
            device(temp.path / "l");
            const auto root = temp.path / "l-root";
            fs::create_directories(root / "Sounds" / "tone");
            std::ofstream(root / "Sounds" / "tone" / "sound.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.sound","version":8,"info":{"id":"tone","name":"Tone","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")"
                << std::string(64, '0') << R"("},"nodes":[]})";
            Package p; p.id = "com.example.pack"; p.name = "Dotted";
            p.items.push_back({"sound", "tone", "Tone", "Sounds/tone", {}});
            const auto dotted = temp.path / "Dotted.olib";
            writePackage(dotted, p, root);
            Session l(48000, 64);
            check(l.installArchive(dotted.string(), true).added == 1, "a dotted ID installs as a library");
            check(l.libraries().size() == 1 && l.libraries()[0].id == "com.example.pack" && l.libraries()[0].items == 1,
                  "and is listed, under its own ID: " + std::to_string(l.libraries().size()));
            Control::LibraryQuery inLibrary; inLibrary.source = "com.example.pack";
            check(l.libraryQuery(inLibrary).total == 1, "its items can be queried");
            check(fs::is_directory(temp.path / "l" / "Libraries" / "com.example.pack"), "the folder is the ID, dots and all");
            l.uninstallLibrary("com.example.pack");
            check(l.libraries().empty() && !fs::exists(temp.path / "l" / "Libraries" / "com.example.pack"), "and it is uninstalled");
            // An ID that differs only in case: on a case-insensitive file system (macOS, Windows)
            // it names the same folder, so it is refused there, never mixed into the other library.
            // On a case-sensitive one both install, and which lists the shared item is the folder order.
            check(l.installArchive(dotted.string(), true).added == 1, "the dotted library installs again");
            Package upper = p; upper.id = "Com.Example.Pack";
            const auto cased = temp.path / "Cased.olib";
            writePackage(cased, upper, root);
            std::string why; try { l.installArchive(cased.string(), true); } catch(const std::exception& e) { why = e.what(); }
            const auto listed = l.libraries();
            const auto items = [&](const char* id) { for(const auto& x : listed) if(x.id == id) return int(x.items); return -1; };
            const bool refused = why.find("another library") != std::string::npos;
            check(refused ? listed.size() == 1 && items("com.example.pack") == 1
                          : listed.size() == 2 && items("com.example.pack") >= 0 && items("Com.Example.Pack") >= 0 && items("com.example.pack") + items("Com.Example.Pack") == 1,
                  "an ID differing only in case is refused or installed apart, the first library still listed: " + why + ", "
                      + std::to_string(listed.size()) + " listed");
            l.uninstallLibrary("com.example.pack");
        }
        // An install cut short by a failed copy leaves nothing half behind: the item folders this
        // call made come back out, as the unpacked folder does, and a library whose install never
        // finished is not listed. What was there before stays, a link included.
        {
            device(temp.path / "m");
            const auto root = temp.path / "m-root";
            fs::create_directories(root / "Sounds" / "one");
            fs::create_directories(root / "Performances" / "two");
            std::ofstream(root / "Sounds" / "one" / "sound.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.sound","version":8,"info":{"id":"one","name":"One","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")"
                << std::string(64, '0') << R"("},"nodes":[]})";
            std::ofstream(root / "Performances" / "two" / "performance.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.performance","version":13,"info":{"id":"two","name":"Two","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")"
                << std::string(64, '0') << R"("},"nodes":[]})";
            Package p; p.id = "m.cut"; p.name = "Cut";
            p.items.push_back({"sound", "one", "One", "Sounds/one", {}});
            p.items.push_back({"performance", "two", "Two", "Performances/two", {}});
            const auto cut = temp.path / "Cut.olib";
            writePackage(cut, p, root);
            Session m(48000, 64);
            const auto home = temp.path / "m";
            const auto library = home / "Libraries" / "m.cut";
            // An item that cannot be moved in, nor copied in its place, as a full disk or a
            // permission makes it: nothing can be added to the folder it goes in or to what is made
            // there (a macOS ACL, on the folder and inherited; root is not held by it).
#ifdef __APPLE__
            if(::geteuid() != 0)
            {
                auto failCopiesInto = [&](const fs::path& folder) { fs::create_directories(folder); return run("chmod +a 'everyone deny add_file,add_subdirectory,file_inherit,directory_inherit' " + shell(folder)).first == 0; };
                auto clear = [&](const fs::path& folder) { run("chmod -N " + shell(folder)); };
                check(failCopiesInto(home / "Sounds"), "an ACL makes copies into the user's Sounds fail");
                bool threw = false;
                try { m.installArchive(cut.string(), false); } catch(const std::exception&) { threw = true; }
                check(threw && !fs::exists(home / "Sounds" / "One.osound") && items(m, "sound").empty() && items(m, "performance").empty(),
                      "an own-storage install whose copy fails partway leaves no half-copied item behind");
                clear(home / "Sounds");
                // Into a library folder that is there already, so it is not removed whole.
                check(failCopiesInto(library / "Sounds"), "an ACL makes copies into the library's Sounds fail");
                threw = false;
                try { m.installArchive(cut.string(), true); } catch(const std::exception&) { threw = true; }
                check(threw && fs::is_directory(library) && !fs::exists(library / "Sounds" / "one") && !fs::exists(library / "library.json") && m.libraries().empty(),
                      "a library install whose copy fails partway leaves no half-copied item and no listed library");
                clear(library / "Sounds");
            }
            else std::puts("skipped: a copy that fails partway (an ACL does not hold root)");
#else
            std::puts("skipped: a copy that fails partway (made with a macOS ACL)");
#endif
            // A link where a new item would go is not overwritten, followed or removed.
            fs::create_directories(home / "Performances");
            fs::create_symlink(temp.path / "nowhere", home / "Performances" / "Two.oscairn");
            check(m.installArchive(cut.string(), false).added == 2 && fs::is_symlink(home / "Performances" / "Two.oscairn") && items(m, "performance").size() == 1,
                  "an own-storage install goes beside a dangling link, keeping it");
            fs::create_directories(library / "Performances");
            fs::create_symlink(temp.path / "nowhere", library / "Performances" / "two");
            const auto linked = m.installArchive(cut.string(), true);
            check(linked.added == 1 && linked.skipped == 1 && fs::is_symlink(library / "Performances" / "two") && m.libraries().size() == 1,
                  "a library install keeps a link where an item goes, as what is there");
            fs::remove(library / "Performances" / "two");
            const auto rest = m.installArchive(cut.string(), true); // once the way is clear, it installs whole
            check(m.libraries().size() == 1 && m.libraries()[0].items == 2 && rest.added == 1,
                  "and the missing item comes back once the way is clear");
            // A manifest whose move in fails leaves no temporary file behind.
            fs::create_directories(home / "blocked" / "f.json" / "inside");
            bool threw = false;
            try { writeAtomically(home / "blocked" / "f.json", "{}"); } catch(const std::exception&) { threw = true; }
            check(threw && !fs::exists(home / "blocked" / ".f.json.tmp"), "a failed atomic write removes its temporary file");
        }
        // A skipped Sound whose copy's ID is the archive's own (a copy edited by hand, or synced):
        // the part-pointer rewrite replaces a needle with itself and must still end.
        {
            device(temp.path / "n");
            const auto root = temp.path / "n-root";
            fs::create_directories(root / "Sounds" / "same");
            fs::create_directories(root / "Performances" / "set");
            const std::string zeros(64, '0');
            std::ofstream(root / "Sounds" / "same" / "sound.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.sound","version":8,"info":{"id":"same","name":"Same","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")" << zeros << R"("},"nodes":[]})";
            std::ofstream(root / "Performances" / "set" / "performance.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.performance","version":13,"info":{"id":"set","name":"Set","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")" << zeros
                << R"("},"nodes":[{"key":"n1","plugin":{"format":"clap","id":"x","version":"1","path":"p"},"sound":"same"}]})";
            Package p; p.id = "n.same"; p.name = "Same";
            p.items.push_back({"sound", "same", "Same", "Sounds/same", {}});
            p.items.push_back({"performance", "set", "Set", "Performances/set", {}});
            const auto same = temp.path / "Same.olib";
            writePackage(same, p, root);
            fs::create_directories(temp.path / "n" / "Sounds" / "Same.osound");
            std::ofstream(temp.path / "n" / "Sounds" / "Same.osound" / "sound.json", std::ios::binary | std::ios::trunc)
                << R"({"format":"oscairn.sound","version":8,"info":{"id":"same","name":"Same","description":"","tags":[],"color":"","revision":"r","parent":"","origin":{"library":"n.same","item":"same","hash":")" << zeros
                << R"("},"hash":")" << zeros << R"("},"nodes":[]})";
            Session n(48000, 64);
            check(items(n, "sound").size() == 1 && items(n, "sound")[0].id == "same", "the user's own copy is there under the archive's ID");
            const auto installed = n.installArchive(same.string(), false);
            check(installed.added == 1 && installed.skipped == 1, "the Sound is skipped, the Performance added");
            const auto doc = fs::path(items(n, "performance").at(0).path) / "performance.json";
            check(read(doc).find("\"sound\":\"same\"") != std::string::npos, "and its part points at the copy, whose ID is the archive's");
        }
        // A new version of a locked library (RFC 0031's Updates): its items are replaced with their
        // new revisions, each swapped in whole, and what it no longer has goes. The open Performance
        // from it plays on as it was, the user's favorite stays, and the same version again replaces nothing.
        {
            device(temp.path / "o");
            const std::string zeros(64, '0');
            auto sound = [&](const fs::path& root, const std::string& id, const std::string& name) {
                fs::create_directories(root / "Sounds" / id);
                std::ofstream(root / "Sounds" / id / "sound.json", std::ios::binary | std::ios::trunc)
                    << R"({"format":"oscairn.sound","version":8,"info":{"id":")" << id << R"(","name":")" << name << R"(","description":"","tags":[],"color":"","revision":"r","parent":"","origin":null,"hash":")" << zeros << R"("},"nodes":[]})";
            };
            // A Performance that opens: one saved here, under the library's ID and its version's name.
            Session o(48000, 64);
            const auto saved = o.savePerformanceToLibrary("Set");
            const auto savedDoc = fs::path(o.libraryQuery({}).items.at(0).path) / "performance.json";
            const auto document = read(savedDoc);
            o.deleteLibraryItem(saved);
            auto performance = [&](const fs::path& root, const std::string& name) {
                auto text = document;
                text.replace(text.find("\"id\":\"" + saved + '"'), 7 + saved.size(), "\"id\":\"set\"");
                text.replace(text.find("\"name\":\"Set\""), 12, "\"name\":\"" + name + '"');
                fs::create_directories(root / "Performances" / "set");
                std::ofstream(root / "Performances" / "set" / "performance.json", std::ios::binary | std::ios::trunc) << text;
            };
            // Version 1: Changed, Kept, Removed and the Performance Set; version 2: Changed and Set revised, Kept as it was, Added new, Removed gone.
            const auto v1 = temp.path / "o-v1", v2 = temp.path / "o-v2";
            sound(v1, "changed", "Changed 1"); sound(v1, "kept", "Kept"); sound(v1, "removed", "Removed"); performance(v1, "Set 1");
            sound(v2, "changed", "Changed 2"); sound(v2, "kept", "Kept"); sound(v2, "added", "Added"); performance(v2, "Set 2");
            Package p1; p1.id = "o.pack"; p1.name = "Pack";
            p1.items = {{"sound", "changed", "Changed 1", "Sounds/changed"}, {"sound", "kept", "Kept", "Sounds/kept"}, {"sound", "removed", "Removed", "Sounds/removed"}, {"performance", "set", "Set 1", "Performances/set"}};
            Package p2 = p1;
            p2.items = {{"sound", "changed", "Changed 2", "Sounds/changed"}, {"sound", "kept", "Kept", "Sounds/kept"}, {"sound", "added", "Added", "Sounds/added"}, {"performance", "set", "Set 2", "Performances/set"}};
            writePackage(temp.path / "Pack1.olib", p1, v1);
            writePackage(temp.path / "Pack2.olib", p2, v2);
            const auto library = temp.path / "o" / "Libraries" / "o.pack";
            auto inode = [](const fs::path& p) { struct stat s{}; ::stat(p.c_str(), &s); return s.st_ino; };
            auto name = [&](Session& s, const std::string& id) { for(const auto& i : s.libraryQuery({}).items) if(i.id == id) return i.name; return std::string("(none)"); };
            check(o.installArchive((temp.path / "Pack1.olib").string(), true).added == 4, "version 1 installs");
            o.libraryFavorite("changed", true);
            o.loadPerformanceFromLibrary("set");
            const auto kept = inode(library / "Sounds" / "kept");
            const auto again = o.installArchive((temp.path / "Pack1.olib").string(), true);
            check(again.added == 0 && again.skipped == 4 && inode(library / "Sounds" / "kept") == kept && name(o, "changed") == "Changed 1",
                  "the same version again replaces nothing: " + std::to_string(again.added) + " added");
            const auto changed = inode(library / "Sounds" / "changed");
            const auto updated = o.installArchive((temp.path / "Pack2.olib").string(), true);
            check(name(o, "changed") == "Changed 2" && name(o, "set") == "Set 2" && inode(library / "Sounds" / "changed") != changed, "a revised item is replaced with its new revision");
            check(name(o, "added") == "Added" && name(o, "kept") == "Kept" && inode(library / "Sounds" / "kept") == kept, "a new item is added, an unchanged one left in place");
            check(name(o, "removed") == "(none)" && !fs::exists(library / "Sounds" / "removed"), "an item the new version no longer has goes");
            check(updated.added == 3 && updated.skipped == 1 && o.libraries().at(0).items == 4, "three put in place, one kept: " + std::to_string(updated.added) + " added, " + std::to_string(updated.skipped) + " skipped");
            bool leftover = false;
            for(const auto& e : fs::recursive_directory_iterator(library)) leftover |= e.path().filename().string().starts_with(".");
            check(!leftover, "nothing half-made is left beside the items");
            Control::LibraryQuery favorites; favorites.favorites = true;
            const auto favored = o.libraryQuery(favorites).items;
            check(favored.size() == 1 && favored[0].id == "changed", "the user's favorite stays on the replaced item");
            check(o.performanceInfo().name == "Set 1", "the open Performance plays on as it was opened: " + o.performanceInfo().name);
            check(o.savePerformanceToLibrary("") != "set" && name(o, "set") == "Set 2", "and saving it makes a new one, the library's left as installed");
            const auto revised = inode(library / "Sounds" / "changed");
            const auto twice = o.installArchive((temp.path / "Pack2.olib").string(), true);
            check(twice.added == 0 && twice.skipped == 4 && inode(library / "Sounds" / "changed") == revised, "version 2 again replaces nothing");
        }
    }
    catch(const std::exception& e) { std::fprintf(stderr, "FAIL: %s\n", e.what()); ++failures; }
    if(!failures) std::puts("ok: export, install as the user's own and as a library, reinstall, uninstall, chosen items and plugins from the store");
    return failures ? 1 : 0;
}
