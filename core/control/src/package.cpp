// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/package.h"
#include "sheredom/json.h"
#include "JsonInput.h"
#include "miniz/miniz.h"
#include "oscairn/assets.h"
#include "oscairn/item_types.h"
#include "oscairn/setlist.h"
#include "oscairn/library.h"
#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <memory>
#include <fstream>
#include <set>
#include <stdexcept>
#include <sys/stat.h>

namespace oscairn
{
    namespace
    {
        namespace fs = std::filesystem;
        using V = const json_value_s*;
        constexpr size_t maxEntries = 1u << 20, maxPath = 1024; // a library of thousands of items, each a few files
        constexpr uint64_t maxEntry = 2ull << 30, maxTotal = 8ull << 30, maxNameBytes = 64ull << 20;
        // The top-level folders: each item type's (RFC 0023: Performances/, Sounds/, Assets/...), Plugins/ and Licenses/.
        std::set<std::string> tops()
        {
            std::set<std::string> out{"Plugins", "Licenses", "Types"};
            for(const auto& t : itemTypes()) out.insert(t.folder);
            return out;
        }
        // A license expression naming the GPL, LGPL or AGPL, any version: object code under them
        // goes with its source, or directions to it (their §6), and its notices.
        bool copyleft(const std::string& license)
        {
            for(size_t at = 0; at < license.size();)
            {
                const auto end = std::min(license.find_first_of(" ()", at), license.size());
                const auto word = license.substr(at, end - at);
                if(word.rfind("GPL-", 0) == 0 || word.rfind("LGPL-", 0) == 0 || word.rfind("AGPL-", 0) == 0) return true;
                at = end + 1;
            }
            return false;
        }
        bool licenseRef(const std::string& license) { return license.find("LicenseRef-") != std::string::npos; }
        bool hidden(const fs::path& relative) { for(const auto& part : relative) if(!part.empty() && part.string()[0] == '.') return true; return false; }

        [[noreturn]] void bad(const std::string& why) { throw std::runtime_error("not a usable Oscairn archive: " + why); }
        std::string text(V v)
        {
            if(!v || v->type != json_type_string) bad("the manifest is missing a name, ID or path");
            const auto* s = static_cast<const json_string_s*>(v->payload);
            return {s->string, s->string_size};
        }
        // The package's ID becomes the library's folder name, and an exported item's ID is its
        // package's, so both are plain: 1 to 64 letters, digits, ".", "-" and "_", starting with a
        // letter or digit and not ending in "." (Windows drops it from a folder's name).
        bool plainId(std::string_view id)
        {
            return !id.empty() && id.size() <= 64 && std::isalnum(static_cast<unsigned char>(id.front())) && id.back() != '.' &&
                   std::all_of(id.begin(), id.end(), [](char c) { return std::isalnum(static_cast<unsigned char>(c)) || c == '.' || c == '-' || c == '_'; });
        }
        // A safe relative path: plain components, none empty, "." or "..", hidden or with a
        // separator, drive or control character in it.
        bool safe(const std::string& name)
        {
            if(name.empty() || name.size() > maxPath) return false;
            size_t start = 0;
            for(;;)
            {
                const auto end = name.find('/', start);
                const auto part = name.substr(start, end == std::string::npos ? std::string::npos : end - start);
                if(part.empty() || part[0] == '.' || part.size() > 255) return false;
                if(std::any_of(part.begin(), part.end(), [](char c) { return c == '\\' || c == ':' || static_cast<unsigned char>(c) < 32; })) return false;
                if(end == std::string::npos) return true;
                start = end + 1;
            }
        }
        struct Zip
        {
            mz_zip_archive zip{};
            bool reading;
            explicit Zip(bool r) : reading(r) {}
            ~Zip() { reading ? mz_zip_reader_end(&zip) : mz_zip_writer_end(&zip); }
        };
    }

    std::string platform()
    {
#if defined(__APPLE__)
        const std::string os = "macos";
#elif defined(_WIN32)
        const std::string os = "windows";
#else
        const std::string os = "linux";
#endif
#if defined(__aarch64__) || defined(_M_ARM64)
        return os + "-arm64";
#elif defined(__x86_64__) || defined(_M_X64)
        return os + "-x86_64";
#else
        return os + "-other";
#endif
    }

    bool knownPlatform(const std::string& name)
    {
        static const std::set<std::string> platforms{"macos-arm64", "macos-x86_64", "linux-arm64", "linux-x86_64", "windows-x86_64"};
        return platforms.count(name) != 0;
    }

    std::string pluginFolderName(const std::string& id, const std::string& version)
    {
        std::string safe;
        for(const char c : id + "@" + version) safe += (std::isalnum(static_cast<unsigned char>(c)) || c == '.' || c == '-' || c == '@' || c == '_') ? c : '_';
        return safe;
    }

    std::vector<fs::path> pluginStoreFolders()
    {
        std::vector<fs::path> out;
        std::error_code e;
        for(const auto* format : {"clap", "lv2"})
            for(const auto& entry : fs::directory_iterator(dataHome() / "Plugins" / format, e))
                if(entry.path().filename().string()[0] != '.' && fs::is_directory(entry.path() / platform(), e)) out.push_back(entry.path() / platform());
        std::sort(out.begin(), out.end());
        return out;
    }

    std::string bundleDigest(const fs::path& build)
    {
        if(fs::is_regular_file(build)) return assets::sha256File(build);
        std::vector<fs::path> files;
        for(const auto& e : fs::recursive_directory_iterator(build))
        {
            const auto relative = fs::relative(e.path(), build);
            if(e.is_symlink()) throw std::runtime_error("a link in a plugin build: " + e.path().string());
            if(e.is_regular_file() && !hidden(relative)) files.push_back(relative);
        }
        std::sort(files.begin(), files.end());
        std::string lines;
        for(const auto& f : files) lines += f.generic_string() + '\n' + assets::sha256File(build / f) + '\n';
        return assets::sha256(lines);
    }

    bool safeArchivePath(const std::string& name) { return safe(name); }

    void unpackPiece(const fs::path& zip, const fs::path& into, const std::string& under, uint64_t most)
    {
        Zip z(true);
        if(!mz_zip_reader_init_file(&z.zip, zip.string().c_str(), 0)) bad("a piece is not a zip: " + under);
        const auto count = mz_zip_reader_get_num_files(&z.zip);
        if(count > maxEntries) bad("a piece holds too many files: " + under);
        uint64_t total = 0;
        for(mz_uint i = 0; i < count; ++i)
        {
            mz_zip_archive_file_stat st;
            if(!mz_zip_reader_file_stat(&z.zip, i, &st)) bad("an entry of a piece cannot be read: " + under);
            if(mz_zip_reader_is_file_a_directory(&z.zip, i)) continue;
            const std::string name = st.m_filename;
            if(!safe(name) || (name != under && name.rfind(under + "/", 0) != 0)) bad("a piece holds an entry outside its path: " + name);
            if(((st.m_external_attr >> 16) & S_IFMT) == S_IFLNK) bad("a link: " + name);
            if(st.m_uncomp_size > maxEntry || (total += st.m_uncomp_size) > most) bad("a piece is larger than its list says: " + under);
            const auto out = into / name;
            std::error_code e;
            if(fs::exists(out, e)) bad("an entry twice: " + name);
            fs::create_directories(out.parent_path());
            if(!mz_zip_reader_extract_to_file(&z.zip, i, out.string().c_str(), 0)) bad("cannot extract " + name);
            if(fs::file_size(out) != st.m_uncomp_size) bad("an entry is not the size it says: " + name);
        }
    }

    std::string manifestJson(const Package& package)
    {
        std::string items, plugins, files;
        auto signatures = [](const std::vector<signing::Signature>& list) { std::string out; for(const auto& s : list) out += (out.empty() ? "" : ",") + signing::json(s); return "[" + out + "]"; };
        for(const auto& i : package.items)
            items += std::string(items.empty() ? "" : ",") + "{\"kind\":" + detail::quote(i.kind) + ",\"id\":" + detail::quote(i.id) + ",\"name\":" + detail::quote(i.name) + ",\"path\":" + detail::quote(i.path) +
                     ",\"signatures\":" + signatures(i.signatures) + '}';
        for(const auto& p : package.plugins)
        {
            std::string builds;
            for(const auto& b : p.builds)
            {
                char estimate[96] = "";
                if(b.cpu > 0 || b.memory > 0) std::snprintf(estimate, sizeof(estimate), ",\"estimate\":{\"cpu\":%.6g,\"memory\":%llu}", b.cpu, static_cast<unsigned long long>(b.memory));
                builds += std::string(builds.empty() ? "" : ",") + "{\"platform\":" + detail::quote(b.platform) + ",\"path\":" + detail::quote(b.path) + ",\"sha256\":" + detail::quote(b.sha256) +
                          ",\"signatures\":" + signatures(b.signatures) + estimate + '}';
            }
            plugins += std::string(plugins.empty() ? "" : ",") + "{\"format\":" + detail::quote(p.format) + ",\"id\":" + detail::quote(p.id) + ",\"version\":" + detail::quote(p.version) +
                       ",\"name\":" + detail::quote(p.name) + ",\"builds\":[" + builds + "]}";
        }
        for(const auto& [path, sha] : package.files) files += std::string(files.empty() ? "" : ",") + detail::quote(path) + ":" + detail::quote(sha);
        const auto source = package.sourceUrl.empty() ? std::string("null") : "{\"url\":" + detail::quote(package.sourceUrl) + ",\"revision\":" + detail::quote(package.sourceRevision) + "}";
        std::string assets = "null", dependencies;
        if(package.kind == "asset")
        {
            std::string list;
            for(const auto& a : package.assets)
                list += std::string(list.empty() ? "" : ",") + "{\"path\":" + detail::quote(a.path) + ",\"sha256\":" + detail::quote(a.sha256) + ",\"size\":" + std::to_string(a.size) + '}';
            assets = "{\"version\":" + detail::quote(package.assetVersion) + ",\"files\":[" + list + "]}";
        }
        for(const auto& d : package.assetDependencies) dependencies += std::string(dependencies.empty() ? "" : ",") + "{\"id\":" + detail::quote(d.id) + ",\"version\":" + detail::quote(d.version) + '}';
        return "{\"format\":\"oscairn.library\",\"version\":3,\"id\":" + detail::quote(package.id) + ",\"name\":" + detail::quote(package.name) +
               ",\"kind\":" + detail::quote(package.kind) + ",\"license\":" + detail::quote(package.license) + ",\"source\":" + source +
               ",\"items\":[" + items + "],\"plugins\":[" + plugins + "],\"assets\":" + assets + ",\"dependencies\":{\"assets\":[" + dependencies + "]},\"files\":{" + files + "}}\n";
    }

    void writePackage(const fs::path& archive, const Package& package, const fs::path& root, const signing::Key* key)
    {
        if(fs::exists(archive)) throw std::runtime_error(archive.string() + " already exists");
        // Every file, and its SHA-256 for the manifest; hidden files are not packed.
        std::vector<fs::path> files;
        // The folders of the types it declares too (RFC 0023 step 3), which this device may not know yet.
        auto folders = tops();
        std::error_code error;
        for(const auto& owner : fs::directory_iterator(root / "Types", error))
            for(const auto& f : fs::directory_iterator(owner.path(), error))
                try
                {
                    std::ifstream in(f.path(), std::ios::binary);
                    const std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
                    folders.insert(readItemType(text).folder);
                }
                catch(const std::exception&) {} // packed as it is; a reader refuses it
        for(const auto& top : folders)
            if(fs::is_directory(root / top))
                for(const auto& e : fs::recursive_directory_iterator(root / top))
                {
                    if(e.is_symlink()) throw std::runtime_error("a link in what is being packed: " + e.path().string());
                    if(e.is_regular_file() && !hidden(fs::relative(e.path(), root))) files.push_back(e.path());
                }
        std::sort(files.begin(), files.end());
        auto listed = package;
        for(const auto& f : files) listed.files[fs::relative(f, root).generic_string()] = assets::sha256File(f);
        const auto manifest = manifestJson(listed);
        const auto temporary = archive.parent_path() / ("." + archive.filename().string() + ".tmp");
        fs::remove(temporary);
        {
            Zip z(false);
            if(!mz_zip_writer_init_file(&z.zip, temporary.string().c_str(), 0)) throw std::runtime_error("cannot write " + archive.string());
            if(!mz_zip_writer_add_mem(&z.zip, "library.json", manifest.data(), manifest.size(), MZ_DEFAULT_COMPRESSION)) throw std::runtime_error("cannot write the manifest");
            if(key)
            {
                const auto sig = signing::json({key->publicKey, signing::sign(*key, signing::archiveMessage(manifest))}) + '\n';
                if(!mz_zip_writer_add_mem(&z.zip, "library.sig", sig.data(), sig.size(), MZ_DEFAULT_COMPRESSION)) throw std::runtime_error("cannot write the archive's signature");
            }
            for(const auto& f : files)
            {
                const auto name = fs::relative(f, root).generic_string();
                if(!safe(name)) throw std::runtime_error("a file that cannot be packed: " + name);
                if(!mz_zip_writer_add_file(&z.zip, name.c_str(), f.string().c_str(), nullptr, 0, MZ_DEFAULT_COMPRESSION))
                    throw std::runtime_error("cannot pack " + name);
            }
            if(!mz_zip_writer_finalize_archive(&z.zip)) throw std::runtime_error("cannot finish " + archive.string());
        }
        if(fs::exists(archive)) { fs::remove(temporary); throw std::runtime_error(archive.string() + " already exists"); }
        fs::rename(temporary, archive);
    }

    void signPackage(const fs::path& archive, const signing::Key& key)
    {
        const auto unpacked = archive.parent_path() / ("." + archive.filename().string() + ".signing");
        const auto signedCopy = archive.parent_path() / ("." + archive.filename().string() + ".signed");
        std::error_code ignored;
        fs::remove_all(unpacked, ignored); fs::remove(signedCopy, ignored);
        try
        {
            const auto p = readPackage(archive, unpacked, "");
            // Rewritten from what it holds, its manifest would lose the builds it leaves out: sign the whole release instead.
            if(std::any_of(p.plugins.begin(), p.plugins.end(), [](const auto& q) { return !q.absent.empty(); })) bad("it leaves out builds its manifest lists (an archive made for one device); sign the whole release");
            writePackage(signedCopy, p, unpacked, &key);
            fs::rename(signedCopy, archive);
        }
        catch(...) { fs::remove_all(unpacked, ignored); fs::remove(signedCopy, ignored); throw; }
        fs::remove_all(unpacked, ignored);
    }

    Package readPackage(const fs::path& archive, const fs::path& into, const std::string& device)
    {
        const bool extract = !into.empty();
        // A folder read where it is, as a release restored from its pieces is (RFC 0031): checked, nothing copied.
        const bool inPlace = extract && archive == into;
        if(extract && !inPlace && fs::exists(into)) throw std::runtime_error(into.string() + " already exists");
        if(extract && !inPlace) fs::create_directories(into);
        try
        {
            // Its entries: a zip's, read in place, or an unpacked folder's (RFC 0031), whose hidden
            // files, which desktops add, are skipped. Each is checked before any is written.
            struct Entry { std::string name; uint64_t size; bool directory, link; mz_uint index; };
            std::vector<Entry> entries;
            Zip z(true);
            const bool folder = fs::is_directory(archive);
            if(folder)
            {
                for(auto it = fs::recursive_directory_iterator(archive); it != fs::recursive_directory_iterator(); ++it)
                {
                    const auto relative = fs::relative(it->path(), archive);
                    if(hidden(relative)) { it.disable_recursion_pending(); continue; }
                    const bool link = it->is_symlink(), directory = !link && it->is_directory();
                    entries.push_back({relative.generic_string(), directory || link ? 0 : it->file_size(), directory, link, 0});
                    if(entries.size() > maxEntries) bad("it holds too many files");
                }
            }
            else
            {
                if(!mz_zip_reader_init_file(&z.zip, archive.string().c_str(), 0)) bad("it is not a zip archive");
                const auto count = mz_zip_reader_get_num_files(&z.zip);
                if(count > maxEntries) bad("it holds too many files");
                for(mz_uint i = 0; i < count; ++i)
                {
                    mz_zip_archive_file_stat st;
                    if(!mz_zip_reader_file_stat(&z.zip, i, &st)) bad("an entry cannot be read");
                    std::string name = st.m_filename;
                    const bool directory = mz_zip_reader_is_file_a_directory(&z.zip, i);
                    if(directory && !name.empty() && name.back() == '/') name.pop_back();
                    entries.push_back({name, st.m_uncomp_size, directory, ((st.m_external_attr >> 16) & S_IFMT) == S_IFLNK, i});
                }
            }
            // A small entry, whole: from the zip, or the folder's file read and checked for its size.
            auto small = [&](const Entry& e) {
                std::string bytes(size_t(e.size), '\0');
                if(folder)
                {
                    std::ifstream in(archive / e.name, std::ios::binary);
                    if(!in.read(bytes.data(), std::streamsize(bytes.size())) || in.peek() != EOF) bad("cannot read " + e.name);
                }
                else if(!mz_zip_reader_extract_to_mem(&z.zip, e.index, bytes.data(), bytes.size(), 0)) bad("cannot read " + e.name);
                return bytes;
            };
            // Item types the archive declares (RFC 0023 step 3), each Types/<package ID>/<file>.json,
            // its ID under that package's: known for this read, and installed with it.
            std::vector<ItemType> declared;
            auto lower = [](std::string s) { for(auto& c : s) c = char(std::tolower(static_cast<unsigned char>(c))); return s; };
            for(const auto& e : entries)
            {
                if(e.directory || e.name.rfind("Types/", 0) != 0) continue;
                const auto owner = e.name.substr(6, e.name.find('/', 6) == std::string::npos ? std::string::npos : e.name.find('/', 6) - 6);
                if(!safe(e.name) || std::count(e.name.begin(), e.name.end(), '/') != 2 || !plainId(owner) || e.name.size() < 5 || e.name.compare(e.name.size() - 5, 5, ".json") != 0 || e.size > 64 * 1024)
                    bad("an item type that is not Types/<package ID>/<file>.json: " + e.name);
                ItemType t;
                try { t = readDeclaredType(small(e), owner); } catch(const std::invalid_argument& x) { bad(x.what()); }
                if(std::any_of(declared.begin(), declared.end(), [&](const ItemType& d) { return d.id == t.id || lower(d.folder) == lower(t.folder); })) bad("two item types with one ID or folder: " + t.id);
                // The first package to declare a type keeps it (RFC 0023's trust rules).
                for(const auto& known : itemTypes())
                    if((known.id == t.id && known.owner != owner) || (known.id != t.id && lower(known.folder) == lower(t.folder)))
                        bad("the item type " + t.id + " is another package's here, or takes another type's folder (" + known.id + ")");
                declared.push_back(t);
            }
            auto typeOf = [&](const std::string& kind) -> std::optional<ItemType> {
                for(const auto& t : declared) if(t.kind() == kind) return t;
                return itemType(kind);
            };
            std::set<std::string> names;
            auto folders = tops();
            for(const auto& t : declared) folders.insert(t.folder);
            uint64_t total = 0, nameBytes = 0, largest = 0;
            std::string manifest, archiveSignature;
            for(const auto& e : entries)
            {
                const auto& name = e.name;
                if(!safe(name)) bad("an entry with an unsafe name: " + name);
                const auto top = name.substr(0, name.find('/'));
                if(name != "library.json" && name != "library.sig" && !folders.count(top)) bad("an entry outside its folders: " + name);
                if(e.link) bad("a link: " + name);
                if((nameBytes += name.size()) > maxNameBytes) bad("its names are too long");
                if(!names.insert(name).second) bad("an entry twice: " + name);
                if(e.directory) continue;
                if(e.size > maxEntry || (total += e.size) > maxTotal) bad("it is too large");
                largest = std::max(largest, e.size);
                if(name == "library.json")
                {
                    if(e.size > maxManifestBytes) bad("the manifest is too large");
                    manifest = small(e);
                }
                if(name == "library.sig")
                {
                    if(e.size > 4096) bad("the archive's signature is too large");
                    archiveSignature = small(e);
                }
            }
            if(manifest.empty()) bad("it has no library.json");
            // Room for what it holds and one entry more (RFC 0031), before anything is written.
            if(extract && !inPlace && fs::space(into).available < total + largest) bad("there is not room for it on this device: it needs " + std::to_string((total + largest) >> 20) + " MiB");
            if(extract && !inPlace)
                for(const auto& e : entries)
                {
                    const auto out = into / e.name;
                    if(e.directory) { fs::create_directories(out); continue; }
                    if(e.name == "library.json" || e.name == "library.sig") continue;
                    fs::create_directories(out.parent_path());
                    if(folder) fs::copy_file(archive / e.name, out);
                    else if(!mz_zip_reader_extract_to_file(&z.zip, e.index, out.string().c_str(), 0)) bad("cannot extract " + e.name);
                    if(fs::file_size(out) != e.size) bad("an entry is not the size it says: " + e.name);
                }
            if(!detail::jsonPreflight(manifest, 16).empty()) bad("its manifest is not valid JSON");
            const std::unique_ptr<json_value_s, decltype(&std::free)> doc{json_parse(manifest.data(), manifest.size()), &std::free};
            V d = doc.get();
            if(text(detail::member(d, "format")) != "oscairn.library") bad("it is not an Oscairn library");
            const V version = detail::member(d, "version");
            if(!version || version->type != json_type_number || std::string(static_cast<const json_number_s*>(version->payload)->number, static_cast<const json_number_s*>(version->payload)->number_size) != "3")
                bad("its version is not 3");
            Package p;
            p.manifestSha256 = assets::sha256(manifest);
            // Signatures, each checked against what it signs; one that does not verify is refused like a wrong hash.
            auto signatures = [](V list) { try { return signing::signatures(list); } catch(const std::runtime_error& e) { bad(e.what()); } };
            if(!archiveSignature.empty())
            {
                if(!detail::jsonPreflight(archiveSignature, 2).empty()) bad("its signature is not valid JSON");
                const std::unique_ptr<json_value_s, decltype(&std::free)> sig{json_parse(archiveSignature.data(), archiveSignature.size()), &std::free};
                try { p.signature = signing::signatureFrom(sig.get()); } catch(const std::runtime_error& e) { bad(e.what()); }
                if(!signing::verify(p.signature, signing::archiveMessage(manifest))) bad("its signature does not verify");
            }
            p.id = text(detail::member(d, "id")); p.name = text(detail::member(d, "name")); p.kind = text(detail::member(d, "kind"));
            // An exported item's kind is its type's (RFC 0023).
            const auto exported = typeOf(p.kind);
            if(!plainId(p.id) || (p.kind != "library" && p.kind != "plugin" && p.kind != "asset" && !(exported && exported->kind() == p.kind && exported->indexed()))) bad("its ID or kind is wrong");
            if(!declared.empty() && (p.kind == "plugin" || p.kind == "asset")) bad("a plugin or asset package declares no item types");
            p.types = declared;
            const V items = detail::member(d, "items");
            if(!items || items->type != json_type_array) bad("it lists no items");
            std::set<std::string> ids;
            for(auto* e = static_cast<const json_array_s*>(items->payload)->start; e; e = e->next)
            {
                Package::Item item{text(detail::member(e->value, "kind")), text(detail::member(e->value, "id")), text(detail::member(e->value, "name")), text(detail::member(e->value, "path"))};
                // Each in its type's folder, as the library keeps it (RFC 0023).
                const auto type = typeOf(item.kind);
                if(!type || type->kind() != item.kind || !type->indexed() || !plainId(item.id) || !ids.insert(item.id).second) bad("an item's kind or ID is wrong");
                if(!safe(item.path) || item.path.substr(0, item.path.find('/')) != type->folder || item.path.find('/', item.path.find('/') + 1) != std::string::npos ||
                   !names.count(item.path + "/" + type->document))
                    bad("an item is not in the archive: " + item.path);
                item.signatures = signatures(detail::member(e->value, "signatures"));
                p.items.push_back(std::move(item));
            }
            // Plugins: each build in the archive, under Plugins/, but another platform's, which an
            // archive made for one device may leave out (RFC 0031).
            std::set<std::string> absentPaths;
            const V plugins = detail::member(d, "plugins");
            if(!plugins || plugins->type != json_type_array) bad("it lists no plugins");
            for(auto* e = static_cast<const json_array_s*>(plugins->payload)->start; e; e = e->next)
            {
                Package::Plugin plugin{text(detail::member(e->value, "format")), text(detail::member(e->value, "id")), text(detail::member(e->value, "version")), text(detail::member(e->value, "name")), {}};
                if((plugin.format != "clap" && plugin.format != "lv2") || plugin.id.empty() || plugin.id.size() > 256 || plugin.version.size() > 128) bad("a plugin's format, ID or version is wrong");
                if(std::any_of(p.plugins.begin(), p.plugins.end(), [&](const auto& q) { return q.format == plugin.format && q.id == plugin.id && q.version == plugin.version; })) bad("a plugin listed twice: " + plugin.id);
                const V builds = detail::member(e->value, "builds");
                if(!builds || builds->type != json_type_array) bad("a plugin lists no builds");
                for(auto* b = static_cast<const json_array_s*>(builds->payload)->start; b; b = b->next)
                {
                    Package::Build build{text(detail::member(b->value, "platform")), text(detail::member(b->value, "path")), text(detail::member(b->value, "sha256"))};
                    const auto inside = names.lower_bound(build.path + "/");
                    const bool there = names.count(build.path) || (inside != names.end() && inside->rfind(build.path + "/", 0) == 0);
                    if(!safe(build.path) || build.path.rfind("Plugins/", 0) != 0 || build.sha256.size() != 64)
                        bad("a plugin build is not in the archive: " + build.path);
                    // The store only finds .clap and .lv2 bundles; a bare library file installs but is never loaded.
                    const std::string suffix = plugin.format == "lv2" ? ".lv2" : ".clap";
                    const std::string leaf { build.path.substr(build.path.rfind('/') + 1) };
                    if(leaf.size() < suffix.size() || leaf.compare(leaf.size() - suffix.size(), suffix.size(), suffix) != 0)
                        bad("a plugin build is not a .clap or .lv2 bundle: " + build.path);
                    if(!knownPlatform(build.platform)) bad("a plugin build for no known platform: " + build.platform);
                    build.signatures = signatures(detail::member(b->value, "signatures"));
                    // Its declared estimate (RFC 0025), optional: a share of the block's time up to 64, and at most 1 TiB.
                    if(const V estimate = detail::member(b->value, "estimate"))
                    {
                        auto number = [&](const char* key) {
                            const V v = detail::member(estimate, key);
                            if(!v || v->type != json_type_number) bad("a plugin build's estimate needs its cpu and memory: " + build.path);
                            return std::strtod(std::string(static_cast<const json_number_s*>(v->payload)->number, static_cast<const json_number_s*>(v->payload)->number_size).c_str(), nullptr);
                        };
                        const double cpu = number("cpu"), memory = number("memory");
                        if(!(cpu >= 0 && cpu <= 64) || !(memory >= 0 && memory <= double(1ull << 40))) bad("a plugin build's estimate is out of range: " + build.path);
                        build.cpu = cpu; build.memory = uint64_t(memory);
                    }
                    for(const auto& s : build.signatures)
                        if(!signing::verify(s, signing::buildMessage(plugin.format, plugin.id, plugin.version, build.platform, build.sha256))) bad("a plugin build's signature does not verify: " + build.path);
                    if(!there)
                    {
                        if(device.empty() || build.platform != device) { plugin.absent.push_back(build.platform); absentPaths.insert(build.path); continue; }
                        bad("it has no " + device + " build of " + plugin.id + " though its manifest lists one (" + build.path + "): it may be the archive made for another device");
                    }
                    plugin.builds.push_back(std::move(build));
                }
                p.plugins.push_back(std::move(plugin));
            }
            // A plugin package carries plugin builds and nothing else.
            if(p.kind == "plugin" && (!p.items.empty() || p.plugins.empty())) bad("a plugin package holds plugins and no items");
            // Its license and source: a plugin package names its license, and object code under the
            // GPL, LGPL or AGPL comes with directions to its source and its license texts.
            if(const V license = detail::member(d, "license"); license && license->type != json_type_null) p.license = text(license);
            if(const V source = detail::member(d, "source"); source && source->type != json_type_null)
            {
                p.sourceUrl = text(detail::member(source, "url")); p.sourceRevision = text(detail::member(source, "revision"));
                const auto plain = [](const std::string& s) { return std::none_of(s.begin(), s.end(), [](char c) { return static_cast<unsigned char>(c) <= ' ' || c == 127; }); };
                if(p.sourceUrl.rfind("https://", 0) != 0 || p.sourceUrl.size() > 2048 || !plain(p.sourceUrl) || p.sourceRevision.empty() || p.sourceRevision.size() > 128 || !plain(p.sourceRevision))
                    bad("its source is not an https:// address and a commit or tag");
            }
            if(p.license.size() > 256) bad("its license is too long");
            if((p.kind == "plugin" || p.kind == "asset") && p.license.empty()) bad("a plugin or asset package names no license");
            // An asset package: its library's version and every file under Assets/, by path, SHA-256 and size.
            const V assets = detail::member(d, "assets");
            const bool assetKind = p.kind == "asset";
            if(assetKind != (assets && assets->type == json_type_object)) bad("an asset package lists its assets, and only an asset package does");
            if(assetKind)
            {
                if(!p.items.empty() || !p.plugins.empty()) bad("an asset package holds assets and no items or plugins");
                p.assetVersion = text(detail::member(assets, "version"));
                if(!plainId(p.assetVersion)) bad("an asset package's version is not plain");
                const V list = detail::member(assets, "files");
                if(!list || list->type != json_type_array) bad("an asset package lists no files");
                std::set<std::string> listed;
                for(auto* e = static_cast<const json_array_s*>(list->payload)->start; e; e = e->next)
                {
                    Package::AssetFile a{text(detail::member(e->value, "path")), text(detail::member(e->value, "sha256"))};
                    const V size = detail::member(e->value, "size");
                    if(!size || size->type != json_type_number) bad("an asset without its size: " + a.path);
                    try { a.size = std::stoull(std::string(static_cast<const json_number_s*>(size->payload)->number, static_cast<const json_number_s*>(size->payload)->number_size)); }
                    catch(const std::exception&) { bad("an asset's size is wrong: " + a.path); }
                    if(!safe(a.path) || a.sha256.size() != 64 || !listed.insert(a.path).second || !names.count("Assets/" + a.path)) bad("an asset that is not in the archive, or listed twice: " + a.path);
                    p.assets.push_back(std::move(a));
                }
                if(p.assets.empty()) bad("an asset package lists no files");
                std::set<std::string> folders; // nothing under Assets/ but what it lists and their folders
                for(const auto& l : listed) for(auto at = l.find('/'); at != std::string::npos; at = l.find('/', at + 1)) folders.insert(l.substr(0, at));
                for(const auto& n : names)
                    if(n.rfind("Assets/", 0) == 0 && !listed.count(n.substr(7)) && !folders.count(n.substr(7))) bad("a file under Assets/ the asset package does not list: " + n);
            }
            // A plugin package's asset libraries: each by ID and the least version it needs.
            if(const V dependencies = detail::member(d, "dependencies"); dependencies && dependencies->type == json_type_object)
                if(const V list = detail::member(dependencies, "assets"); list && list->type == json_type_array)
                    for(auto* e = static_cast<const json_array_s*>(list->payload)->start; e; e = e->next)
                    {
                        Package::Dependency dependency{text(detail::member(e->value, "id")), text(detail::member(e->value, "version"))};
                        if(!plainId(dependency.id) || !plainId(dependency.version) || p.assetDependencies.size() >= 256) bad("an asset dependency is wrong: " + dependency.id);
                        p.assetDependencies.push_back(std::move(dependency));
                    }
            if(!p.assetDependencies.empty() && p.kind != "plugin") bad("only a plugin package names asset dependencies");
            // RFC 0031: a license of the publisher's own (LicenseRef-) comes with its text.
            if((p.kind == "plugin" || p.kind == "asset") && licenseRef(p.license))
            {
                const auto texts = names.lower_bound("Licenses/");
                if(texts == names.end() || texts->rfind("Licenses/", 0) != 0) bad("a package under " + p.license + " carries no license text under Licenses/");
            }
            if(p.kind == "plugin" && copyleft(p.license))
            {
                const auto texts = names.lower_bound("Licenses/");
                if(p.sourceUrl.empty() || texts == names.end() || texts->rfind("Licenses/", 0) != 0)
                    bad("a plugin package under " + p.license + " carries no source (an https:// address and a commit or tag) or no license texts under Licenses/");
            }
            // Every file's SHA-256, as the manifest lists it, and each build's digest.
            const V files = detail::member(d, "files");
            if(!files || files->type != json_type_object) bad("it lists no files");
            for(auto* e = static_cast<const json_object_s*>(files->payload)->start; e; e = e->next)
                p.files[std::string(e->name->string, e->name->string_size)] = text(e->value);
            // An absent build's files are absent with it: only what the archive holds is listed here.
            std::erase_if(p.files, [&](const auto& f) {
                return std::any_of(absentPaths.begin(), absentPaths.end(), [&](const auto& a) { return f.first == a || f.first.rfind(a + "/", 0) == 0; });
            });
            for(const auto& a : p.assets)
            {
                const auto listed = p.files.find("Assets/" + a.path);
                if(listed == p.files.end() || listed->second != a.sha256) bad("an asset that is not what the manifest says: " + a.path);
                std::error_code e;
                if(extract && fs::file_size(into / "Assets" / a.path, e) != a.size) bad("an asset that is not the size the manifest says: " + a.path);
            }
            if(extract)
            {
                size_t found = 0;
                for(const auto& e : fs::recursive_directory_iterator(into))
                {
                    if(!e.is_regular_file()) continue;
                    const auto name = fs::relative(e.path(), into).generic_string();
                    if(inPlace && (name == "library.json" || name == "library.sig")) continue;
                    const auto listed = p.files.find(name);
                    if(listed == p.files.end()) bad("a file the manifest does not list: " + name);
                    if(assets::sha256File(e.path()) != listed->second) bad("a file that is not what the manifest says: " + name);
                    ++found;
                }
                if(found != p.files.size()) bad("a file the manifest lists is not in it");
                for(const auto& plugin : p.plugins)
                    for(const auto& build : plugin.builds)
                        if(bundleDigest(into / build.path) != build.sha256) bad("a plugin build that is not what the manifest says: " + build.path);
                for(const auto& item : p.items)
                {
                    if(item.signatures.empty()) continue;
                    std::string hash;
                    try { hash = contentHash(into / item.path / typeOf(item.kind)->document); } catch(const std::runtime_error&) { bad("a signed item without its info: " + item.path); }
                    for(const auto& s : item.signatures) if(!signing::verify(s, signing::itemMessage(hash))) bad("an item's signature does not verify: " + item.path);
                }
                // A setlist's slots name Performances the archive holds; a placeholder names none (docs/setlist-format.md).
                for(const auto& item : p.items)
                {
                    if(item.kind != "setlist") continue;
                    const auto file = into / item.path / "setlist.json";
                    std::error_code e;
                    if(fs::file_size(file, e) > (1u << 20)) bad("a setlist larger than 1 MiB: " + item.path);
                    std::ifstream in(file, std::ios::binary);
                    const std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
                    Control::Setlist setlist;
                    try { setlist = readSetlist(bytes); } catch(const std::runtime_error& x) { bad("the setlist " + item.path + " cannot be read: " + x.what()); }
                    for(const auto& id : setlistPerformances(setlist))
                        if(std::none_of(p.items.begin(), p.items.end(), [&](const Package::Item& i) { return i.kind == "performance" && i.id == id; }))
                            bad("the setlist " + item.name + " names a Performance the archive does not hold: " + id);
                }
            }
            return p;
        }
        catch(...) { std::error_code ignored; if(extract) fs::remove_all(into, ignored); throw; }
    }
}
