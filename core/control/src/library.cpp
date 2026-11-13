// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/library.h"
#include "sheredom/json.h"
#include "JsonInput.h"
#include "oscairn/assets.h"
#include "oscairn/item_types.h"
#include "oscairn/setlist.h"
#include "oscairn/package.h"
#include "oscairn/plugin_host.h"
#include <kick_clap/Discovery.h>
#include <algorithm>
#include <atomic>
#include <cctype>
#include <cerrno>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <chrono>
#include <cstdlib>
#include <deque>
#include <fstream>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <random>
#include <set>
#include <sstream>
#include <stdexcept>
#include <thread>
#include <tuple>
#include <fcntl.h>
#include <unistd.h>

namespace oscairn
{
    namespace
    {
        namespace fs = std::filesystem;
        using V = const json_value_s*;
        using Doc = std::unique_ptr<json_value_s, decltype(&std::free)>;
        constexpr size_t maxFile = 4 * 1024 * 1024, maxTags = 32, maxTag = 64, maxRecent = 50;
        // shortcut: at most this many entries walked in the folders indexed in place at each rescan; the database index (step 5) scans incrementally.
        constexpr size_t maxInPlace = 100000;

        std::string slurp(const fs::path& file, size_t limit = maxFile)
        {
            std::ifstream in(file, std::ios::binary);
            if(!in) throw std::runtime_error("cannot read " + file.string());
            std::ostringstream out; out << in.rdbuf();
            auto text = out.str();
            if(text.size() > limit) throw std::runtime_error(file.string() + " is too large");
            return text;
        }
        Doc parse(const std::string& text)
        {
            if(!detail::jsonPreflight(text, 32).empty()) return {nullptr, &std::free};
            return {json_parse(text.data(), text.size()), &std::free};
        }
        std::string str(V v)
        {
            if(!v || v->type != json_type_string) throw std::runtime_error("expected a string");
            const auto* s = static_cast<const json_string_s*>(v->payload);
            return {s->string, s->string_size};
        }
        std::vector<std::string> strings(V v)
        {
            std::vector<std::string> out;
            if(!v || v->type != json_type_array) throw std::runtime_error("expected a list");
            for(auto* e = static_cast<const json_array_s*>(v->payload)->start; e; e = e->next) out.push_back(str(e->value));
            return out;
        }
        std::string list(const std::vector<std::string>& values)
        {
            std::string out = "[";
            for(const auto& v : values) { if(out.size() > 1) out += ','; out += detail::quote(v); }
            return out + ']';
        }
        int64_t stamp(const fs::path& file) { return int64_t(fs::last_write_time(file).time_since_epoch().count()); }
        std::string randomId() { std::random_device random; constexpr char hex[] = "0123456789abcdef"; std::string id; for(int i = 0; i < 16; ++i) id += hex[random() & 15]; return id; }
        std::string lower(std::string s) { for(auto& c : s) c = char(std::tolower(static_cast<unsigned char>(c))); return s; }

        // An extractor's run (RFC 0023 step 4): a fresh process per file, ended at these.
        constexpr uint32_t extractTimeoutMs = 10000;
        constexpr size_t maxExtracted = 64 * 1024;
        constexpr uint64_t extractMemory = uint64_t(256) << 20;
        using Fields = std::vector<std::pair<std::string, std::string>>;
        struct Extracted { std::string identity; Fields fields; };
        // What an extractor answered, checked as the untrusted text it is: one JSON object, its
        // identity a SHA-256 in lowercase hex (required for a payload identity), its fields text or
        // numbers of at most 256 bytes of UTF-8 with no control characters, each once. Fields its
        // type does not declare are dropped; anything else wrong refuses the whole answer.
        Extracted readExtracted(const std::string& out, const ItemType& type)
        {
            auto refuse = [](const std::string& why) { throw std::runtime_error(why); };
            if(out.size() > maxExtracted) refuse("its answer is too large");
            if(const auto why = detail::jsonPreflight(out, 4); !why.empty()) refuse("its answer is not JSON: " + std::string(why));
            const auto doc = parse(out);
            V d = doc.get();
            if(!d || d->type != json_type_object) refuse("its answer is not a JSON object");
            Extracted e;
            if(V id = detail::member(d, "identity"))
            {
                if(id->type != json_type_string) refuse("its identity is not text");
                e.identity = str(id);
                if(e.identity.size() != 64 || !std::all_of(e.identity.begin(), e.identity.end(), [](char c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'); })) refuse("its identity is not a SHA-256");
            }
            if(type.identity == "payload" && e.identity.empty()) refuse("it gave no identity");
            if(V fields = detail::member(d, "fields"))
            {
                if(fields->type != json_type_object) refuse("its fields are not an object");
                for(auto* m = static_cast<const json_object_s*>(fields->payload)->start; m; m = m->next)
                {
                    const std::string key(m->name->string, m->name->string_size);
                    if(std::none_of(type.fields.begin(), type.fields.end(), [&](const auto& f) { return f.id == key; })) continue;
                    if(std::any_of(e.fields.begin(), e.fields.end(), [&](const auto& f) { return f.first == key; })) refuse("it gave the field " + key + " twice");
                    std::string value;
                    if(m->value->type == json_type_string) value = str(m->value);
                    else if(m->value->type == json_type_number) value.assign(static_cast<const json_number_s*>(m->value->payload)->number, static_cast<const json_number_s*>(m->value->payload)->number_size);
                    else refuse("its field " + key + " is neither text nor a number");
                    // Escapes decoded, the value is checked again: no control characters, and so the preflight checks only its UTF-8.
                    if(value.size() > 256 || std::any_of(value.begin(), value.end(), [](char c) { return static_cast<unsigned char>(c) < 32 || c == 127; }) ||
                       !detail::jsonPreflight(value, SIZE_MAX).empty())
                        refuse("its field " + key + " is too long or not plain UTF-8 text");
                    e.fields.emplace_back(key, std::move(value));
                }
            }
            return e;
        }
        std::string fieldsJson(const Fields& fields)
        {
            std::string out;
            for(const auto& [k, v] : fields) out += std::string(out.empty() ? "" : ",") + detail::quote(k) + ':' + detail::quote(v);
            return '{' + out + '}';
        }
    }

    // Extractors at work (RFC 0023 step 4): one thread, off the control and audio threads, runs
    // each file's extractor in a process of its own, one at a time, and keeps what it answered
    // for the next rescan to take. A run that fails, hangs, grows too large or answers wrongly
    // costs that file its fields (and a payload identity: its path's stands in), nothing else.
    struct Library::Extraction
    {
        struct Job { std::string path; int64_t modified; ItemType type; };
        struct Done { int64_t modified; std::string id; Fields fields; };
        explicit Extraction(std::string program) : helper(std::move(program)), worker([this] { run(); }) {}
        ~Extraction()
        {
            { std::lock_guard lock(mutex); stop = true; } // stop also ends a run under way
            wake.notify_all();
            worker.join();
        }
        void add(Job job)
        {
            std::lock_guard lock(mutex);
            if(!pending.insert(job.path).second) return;
            jobs.push_back(std::move(job));
            wake.notify_one();
        }
        // The answer for path, when there is one for this version of the file.
        std::optional<Done> take(const std::string& path, int64_t modified)
        {
            std::lock_guard lock(mutex);
            const auto it = done.find(path);
            if(it == done.end()) return std::nullopt;
            auto d = std::move(it->second);
            done.erase(it);
            if(d.modified != modified) return std::nullopt;
            return d;
        }
        std::atomic<bool> fresh{false}; // answers the last rescan has not taken

    private:
        void run()
        {
            for(;;)
            {
                Job job;
                {
                    std::unique_lock lock(mutex);
                    wake.wait(lock, [&] { return stop.load() || !jobs.empty(); });
                    if(stop) return;
                    job = std::move(jobs.front());
                    jobs.pop_front();
                }
                Done d{job.modified, {}, {}};
                const auto r = kick::clap::runIsolated({helper, "--extract", job.type.extractor, job.path}, extractTimeoutMs, maxExtracted, &stop, extractMemory, &residentBytes);
                std::string why;
                if(r.outcome == kick::clap::IsolatedRun::Exited)
                    try { auto e = readExtracted(r.output, job.type); d.fields = std::move(e.fields); if(job.type.identity == "payload") d.id = e.identity; }
                    catch(const std::exception& e) { why = e.what(); }
                else why = r.outcome == kick::clap::IsolatedRun::TimedOut ? "it ran too long" : r.outcome == kick::clap::IsolatedRun::Oversized ? "its answer is too large"
                         : r.outcome == kick::clap::IsolatedRun::OverMemory ? "it used too much memory" : r.outcome == kick::clap::IsolatedRun::Failed ? "it failed or crashed" : "it did not start";
                if(stop) return;
                if(!why.empty()) std::fprintf(stderr, "oscairn: no fields for %s: %s%s%s\n", job.path.c_str(), why.c_str(), r.stderrTail.empty() ? "" : ": ", r.stderrTail.c_str());
                if(d.id.empty())
                    try { d.id = job.type.identity == "content" ? assets::sha256File(job.path) : assets::sha256(job.path); }
                    catch(const std::exception&) { d.id = assets::sha256(job.path); }
                std::lock_guard lock(mutex);
                pending.erase(job.path);
                done[job.path] = std::move(d);
                fresh = true;
            }
        }
        const std::string helper; // oscairn-plugin-scan, which runs Oscairn's extractors
        std::mutex mutex; // jobs, pending and done: this thread's and the library's
        std::condition_variable wake;
        std::deque<Job> jobs;
        std::set<std::string> pending; // paths queued or under way
        std::map<std::string, Done> done;
        std::atomic<bool> stop{false};
        std::thread worker; // last: started once the rest is
    };

    Library::~Library() = default;

    void Library::collect()
    {
        if(extraction_ && extraction_->fresh.exchange(false)) rescan();
    }

    void writeAtomically(const std::filesystem::path& file, const std::string& text)
    {
        namespace fs = std::filesystem;
        fs::create_directories(file.parent_path());
        const auto temporary = file.parent_path() / ("." + file.filename().string() + ".tmp");
        // On disk before the move, so a crash after it leaves the new file, not an empty one.
        bool ok = false;
        if(const int fd = ::open(temporary.c_str(), O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644); fd >= 0)
        {
            ok = true;
            for(size_t at = 0; ok && at < text.size();)
            {
                const auto n = ::write(fd, text.data() + at, text.size() - at);
                if(n > 0) at += size_t(n);
                else ok = n < 0 && errno == EINTR;
            }
            ok = ok && ::fsync(fd) == 0;
            ok = ::close(fd) == 0 && ok;
        }
        std::error_code error;
        if(ok) fs::rename(temporary, file, error);
        if(!ok || error) { std::error_code ignored; fs::remove(temporary, ignored); throw std::runtime_error("cannot write " + file.string()); }
        // The folder too, so the rename itself survives a power cut (RFC 0037).
        if(const int fd = ::open(file.parent_path().c_str(), O_RDONLY | O_DIRECTORY | O_CLOEXEC); fd >= 0) { ::fsync(fd); ::close(fd); }
    }

    fs::path dataHome()
    {
        if(const char* home = std::getenv("OSCAIRN_DATA_HOME"); home && *home) return home;
        const char* user = std::getenv("HOME");
        if(!user || !*user) throw std::runtime_error("cannot find the user's home folder for the library");
#ifdef __APPLE__
        return fs::path(user) / "Library/Application Support/Oscairn";
#else
        if(const char* xdg = std::getenv("XDG_DATA_HOME"); xdg && *xdg) return fs::path(xdg) / "oscairn";
        return fs::path(user) / ".local/share/oscairn";
#endif
    }

    bool timeKnown()
    {
        if(const char* state = std::getenv("OSCAIRN_TIME"); state && *state) return std::string_view(state) == "known";
        // shortcut: the floor is this code's date (2026-11-01 UTC), not the image's build time and the
        // time last saved; the supervisor gives both once built (RFC 0035 step 6).
        return std::chrono::system_clock::now() >= std::chrono::system_clock::from_time_t(1793491200);
    }

    bool validColor(const std::string& c)
    {
        return c.empty() || (c.size() == 7 && c[0] == '#' && std::all_of(c.begin() + 1, c.end(), [](char x) { return std::isxdigit(static_cast<unsigned char>(x)) != 0; }));
    }

    std::string contentHash(std::string_view document, const fs::path& nodes)
    {
        std::string summary(document);
        summary += '\n';
        std::vector<fs::path> files;
        std::error_code error;
        if(fs::is_directory(nodes, error))
            for(const auto& e : fs::recursive_directory_iterator(nodes, error)) if(e.is_regular_file()) files.push_back(e.path());
        std::sort(files.begin(), files.end());
        for(const auto& f : files) summary += fs::relative(f, nodes).generic_string() + ' ' + assets::sha256File(f) + '\n';
        return assets::sha256(summary);
    }

    namespace
    {
        // The top-level info object, by its key, from its opening brace to just past its matching
        // one, outside strings.
        std::pair<size_t, size_t> infoSpan(const std::string& text, const fs::path& file)
        {
            const auto key = text.find(",\"info\":{");
            if(key == std::string::npos) throw std::runtime_error("no info block in " + file.string());
            size_t at = key + 8, depth = 0;
            bool quoted = false, escaped = false;
            for(; at < text.size(); ++at)
            {
                const char ch = text[at];
                if(quoted) { if(escaped) escaped = false; else if(ch == '\\') escaped = true; else if(ch == '"') quoted = false; continue; }
                if(ch == '"') quoted = true;
                else if(ch == '{') ++depth;
                else if(ch == '}' && --depth == 0) break;
            }
            if(at >= text.size()) throw std::runtime_error("an unfinished info block in " + file.string());
            return {key + 8, at + 1};
        }
    }

    std::string contentHash(const fs::path& file)
    {
        const auto text = slurp(file);
        const auto [from, to] = infoSpan(text, file);
        return contentHash(text.substr(0, from) + text.substr(to), file.parent_path() / "nodes");
    }

    std::string infoJson(const Control::PerformanceInfo& info)
    {
        const auto& o = info.origin;
        return "{\"id\":" + detail::quote(info.id) + ",\"name\":" + detail::quote(info.name) + ",\"description\":" + detail::quote(info.description) + ",\"tags\":" + list(info.tags) +
               ",\"color\":" + detail::quote(info.color) + ",\"revision\":" + detail::quote(info.revision) + ",\"parent\":" + detail::quote(info.parent) + ",\"hash\":" + detail::quote(info.hash) +
               ",\"origin\":" + (o.library.empty() ? std::string("null") : "{\"library\":" + detail::quote(o.library) + ",\"item\":" + detail::quote(o.item) + ",\"hash\":" + detail::quote(o.hash) + '}') + '}';
    }

    void writeInfo(const fs::path& file, const Control::PerformanceInfo& info)
    {
        auto text = slurp(file);
        const auto [from, to] = infoSpan(text, file);
        writeAtomically(file, text.substr(0, from) + infoJson(info) + text.substr(to));
    }

    Control::PerformanceInfo readInfo(const fs::path& file, const std::string& format) { return readInfo(std::string_view(slurp(file)), format); }

    Control::PerformanceInfo readInfo(std::string_view document, const std::string& format, bool needId)
    {
        if(document.size() > maxFile) throw std::runtime_error("the document is too large");
        const auto doc = parse(std::string(document));
        V d = doc.get();
        if(!d || str(detail::member(d, "format")) != format) throw std::runtime_error("not a " + format);
        // The document version its type reads (RFC 0023), when it names one.
        const auto types = itemTypes();
        const auto type = std::find_if(types.begin(), types.end(), [&](const ItemType& t) { return t.format == format; });
        if(type == types.end()) throw std::runtime_error("no item type reads " + format);
        const V version = detail::member(d, "version");
        if(!version || version->type != json_type_number || (type->version && std::string(static_cast<const json_number_s*>(version->payload)->number, static_cast<const json_number_s*>(version->payload)->number_size) != std::to_string(type->version)))
            throw std::runtime_error("unsupported version");
        const V info = detail::member(d, "info");
        if(!info || info->type != json_type_object) throw std::runtime_error("no info block");
        Control::PerformanceInfo out;
        out.id = str(detail::member(info, "id")); out.name = str(detail::member(info, "name"));
        out.description = str(detail::member(info, "description")); out.tags = strings(detail::member(info, "tags"));
        out.color = str(detail::member(info, "color"));
        out.revision = str(detail::member(info, "revision")); out.parent = str(detail::member(info, "parent")); out.hash = str(detail::member(info, "hash"));
        if(const V o = detail::member(info, "origin"); !o || o->type != json_type_null)
            out.origin = {str(detail::member(o, "library")), str(detail::member(o, "item")), str(detail::member(o, "hash"))};
        if((needId && out.id.empty()) || out.id.size() > 64 || !validColor(out.color) || out.name.size() > 256 || out.description.size() > 4096 || out.tags.size() > maxTags ||
           std::any_of(out.tags.begin(), out.tags.end(), [](const std::string& t) { return t.empty() || t.size() > maxTag; }))
            throw std::runtime_error("invalid info");
        return out;
    }

    Library::Library(fs::path home)
    {
        if(home.empty()) home = dataHome();
        home_ = home; libraries_ = home / "Libraries"; cache_ = home / "library";
        // An install's or export's unpack folder left by a crash. Only those over a day old: a
        // newer one may be another process's still at work.
        std::error_code error;
        const auto dayAgo = fs::file_time_type::clock::now() - std::chrono::hours(24);
        for(const auto& entry : fs::directory_iterator(home, error))
        {
            const auto name = entry.path().filename().string();
            if(!name.starts_with(".install-") && !name.starts_with(".export-")) continue;
            std::error_code unread;
            const bool folder = fs::is_directory(entry.symlink_status(unread));
            const auto written = entry.last_write_time(unread);
            if(folder && !unread && written < dayAgo) fs::remove_all(entry.path(), error);
        }
        readUser();
        // The item types packages installed here (RFC 0023 step 3), Types/<package ID>/<type ID>.json.
        for(const auto& owner : fs::directory_iterator(home / "Types", error))
            for(const auto& file : fs::directory_iterator(owner.path(), error))
                try
                {
                    if(file.path().extension() != ".json" || file.path().filename().string().starts_with(".")) continue;
                    const auto type = readDeclaredType(slurp(file.path(), 64 * 1024), owner.path().filename().string());
                    if(!itemType(type.id)) addItemType(type);
                }
                catch(const std::exception& e) { std::fprintf(stderr, "oscairn: the item type %s is not known: %s\n", file.path().c_str(), e.what()); }
        // The Setlist app's own files from before setlists were library items (version 1), each
        // made a library setlist once, the file removed (docs/setlist-format.md).
        std::vector<fs::path> old;
        for(const auto& entry : fs::directory_iterator(home / "Setlists", error))
            if(entry.is_regular_file() && entry.path().extension() == ".json" && !entry.path().filename().string().starts_with(".")) old.push_back(entry.path());
        for(const auto& file : old)
            try
            {
                auto s = setlistFromVersion1(slurp(file, 1024 * 1024));
                s.info.id = randomId(); s.info.revision = randomId();
                writeSetlist(newFolder(s.info.name, "setlist"), s);
                fs::remove(file);
            }
            catch(const std::exception& e) { std::fprintf(stderr, "oscairn: the setlist %s was not converted: %s\n", file.c_str(), e.what()); }
        rescan(); forgetOrphans();
    }

    uint32_t Library::rescan()
    {
        // What was read before, by folder and the time its file had.
        std::map<std::string, std::pair<int64_t, Control::LibraryItem>> cached;
        try
        {
            const auto doc = parse(slurp(cache_ / "index.json", maxManifestBytes)); // one row per item, as a manifest
            if(V items = detail::member(doc.get(), "items"); items && items->type == json_type_array)
                for(auto* e = static_cast<const json_array_s*>(items->payload)->start; e; e = e->next)
                {
                    Control::LibraryItem item;
                    item.path = str(detail::member(e->value, "path")); item.id = str(detail::member(e->value, "id")); item.kind = str(detail::member(e->value, "kind"));
                    item.name = str(detail::member(e->value, "name")); item.description = str(detail::member(e->value, "description"));
                    item.tags = strings(detail::member(e->value, "tags")); item.color = str(detail::member(e->value, "color")); item.revision = str(detail::member(e->value, "revision")); item.hash = str(detail::member(e->value, "hash"));
                    item.library = str(detail::member(e->value, "library"));
                    item.origin = {str(detail::member(e->value, "originLibrary")), str(detail::member(e->value, "originItem")), str(detail::member(e->value, "originHash"))};
                    if(V fields = detail::member(e->value, "fields"); fields && fields->type == json_type_object) // an extractor's (step 4)
                        for(auto* m = static_cast<const json_object_s*>(fields->payload)->start; m; m = m->next) item.fields.emplace_back(std::string(m->name->string, m->name->string_size), str(m->value));
                    const auto modified = std::stoll(str(detail::member(e->value, "modified")));
                    cached[item.path] = {modified, item};
                }
        }
        catch(const std::exception&) { cached.clear(); } // no index, or a damaged one: read every file
        items_.clear();
        std::string index;
        std::error_code error;
        // The user's own folders, then each installed library's, its items locked: one per type
        // the library indexes (RFC 0023).
        std::vector<ItemType> types;
        for(const auto& t : itemTypes()) if(t.indexed()) types.push_back(t);
        std::vector<std::tuple<fs::path, ItemType, std::string>> folders;
        for(const auto& t : types) folders.push_back({home_ / t.folder, t, ""});
        installed_.clear();
        if(fs::is_directory(libraries_, error))
            for(const auto& entry : fs::directory_iterator(libraries_, error))
            {
                if(!entry.is_directory() || entry.path().filename().string().starts_with(".")) continue;
                try
                {
                    const auto doc = parse(slurp(entry.path() / "library.json", maxManifestBytes));
                    Control::InstalledLibrary l{str(detail::member(doc.get(), "id")), str(detail::member(doc.get(), "name")), 0};
                    if(l.id != entry.path().filename().string()) continue; // not where it was installed
                    installed_.push_back(l);
                    for(const auto& t : types) folders.push_back({entry.path() / t.folder, t, l.id});
                }
                catch(const std::exception&) {} // a damaged library: install it again
            }
        for(const auto& [folder, type, library] : folders)
        if(fs::is_directory(folder, error))
            for(const auto& entry : fs::directory_iterator(folder, error))
            {
                const auto manifest = entry.path() / type.document;
                // Hidden folders are a save in progress or one left by a crash.
                if(!entry.is_directory() || entry.path().filename().string().starts_with(".") || !fs::is_regular_file(manifest, error)) continue;
                const auto path = entry.path().string();
                const auto modified = stamp(manifest);
                Control::LibraryItem item;
                if(const auto it = cached.find(path); it != cached.end() && it->second.first == modified) item = it->second.second;
                else
                {
                    try
                    {
                        const auto info = readInfo(manifest, type.format);
                        item.kind = type.kind(); item.id = info.id; item.name = info.name; item.description = info.description; item.tags = info.tags; item.color = info.color; item.revision = info.revision; item.hash = info.hash; item.path = path;
                        item.origin = info.origin;
                    }
                    catch(const std::exception&) { continue; } // not one the library can show; it stays on disk
                }
                item.library = library;
                if(std::any_of(items_.begin(), items_.end(), [&](const auto& i) { return i.id == item.id; })) continue; // a copied folder: the first wins
                items_.push_back(item);
                for(auto& l : installed_) if(l.id == library) ++l.items;
                index += std::string(index.empty() ? "" : ",") + "{\"path\":" + detail::quote(item.path) + ",\"kind\":" + detail::quote(item.kind) + ",\"modified\":" + detail::quote(std::to_string(modified)) +
                         ",\"id\":" + detail::quote(item.id) + ",\"name\":" + detail::quote(item.name) + ",\"description\":" + detail::quote(item.description) + ",\"tags\":" + list(item.tags) + ",\"color\":" + detail::quote(item.color) + ",\"revision\":" + detail::quote(item.revision) + ",\"hash\":" + detail::quote(item.hash) +
                         ",\"library\":" + detail::quote(item.library) + ",\"originLibrary\":" + detail::quote(item.origin.library) + ",\"originItem\":" + detail::quote(item.origin.item) + ",\"originHash\":" + detail::quote(item.origin.hash) + '}';
            }
        // RFC 0023 step 3: a type whose package is not installed here is known for the items it
        // has, listed read-only.
        const auto all = itemTypes();
        auto orphan = [&](const ItemType& t) { return !t.owner.empty() && std::none_of(installed_.begin(), installed_.end(), [&](const auto& l) { return l.id == t.owner; }); };
        for(auto& item : items_)
            if(const auto t = std::find_if(all.begin(), all.end(), [&](const ItemType& x) { return x.kind() == item.kind; }); t != all.end() && orphan(*t)) item.readOnly = true;
        // Items in place: files of the in-place types in the folders the user chose, by name and
        // identity, never moved or changed. A payload identity is the extractor's (step 4); with
        // none, or none that answers, the path's.
        size_t seen = 0;
        const auto helper = placementPolicy().scan;
        for(const auto& type : all)
        {
            if(!type.inPlace()) continue;
            for(const auto& root : folders_)
            {
                std::error_code walk;
                for(auto it = fs::recursive_directory_iterator(root, fs::directory_options::skip_permission_denied, walk); !walk && it != fs::recursive_directory_iterator(); it.increment(walk))
                {
                    if(++seen > maxInPlace) break;
                    const auto name = it->path().filename().string();
                    if(name.starts_with(".")) { if(it->is_directory(walk)) it.disable_recursion_pending(); continue; }
                    if(it->is_symlink(walk) || !it->is_regular_file(walk)) continue;
                    if(std::find(type.extensions.begin(), type.extensions.end(), lower(it->path().extension().string())) == type.extensions.end()) continue;
                    const auto path = it->path().string();
                    const auto modified = stamp(it->path());
                    Control::LibraryItem item;
                    if(const auto c = cached.find(path); c != cached.end() && c->second.first == modified && c->second.second.kind == type.kind()) item = c->second.second;
                    else if(!type.extractor.empty() && !helper.empty())
                    {
                        // Its extractor reads it off this thread (step 4); it is listed once that answers.
                        if(!extraction_) extraction_ = std::make_unique<Extraction>(helper);
                        auto done = extraction_->take(path, modified);
                        if(!done) { extraction_->add({path, modified, type}); continue; }
                        item.kind = type.kind(); item.name = it->path().stem().string(); item.path = path; item.id = done->id; item.fields = std::move(done->fields);
                    }
                    else
                    {
                        item.kind = type.kind(); item.name = it->path().stem().string(); item.path = path;
                        try { item.id = type.identity == "content" ? assets::sha256File(it->path()) : assets::sha256(path); } catch(const std::exception&) { continue; }
                    }
                    item.inPlace = item.readOnly = true;
                    if(std::any_of(items_.begin(), items_.end(), [&](const auto& i) { return i.id == item.id; })) continue; // the same file in two folders chosen
                    items_.push_back(item);
                    index += std::string(index.empty() ? "" : ",") + "{\"path\":" + detail::quote(item.path) + ",\"kind\":" + detail::quote(item.kind) + ",\"modified\":" + detail::quote(std::to_string(modified)) +
                             ",\"id\":" + detail::quote(item.id) + ",\"name\":" + detail::quote(item.name) + ",\"description\":\"\",\"tags\":[],\"color\":\"\",\"revision\":\"\",\"hash\":\"\",\"library\":\"\",\"originLibrary\":\"\",\"originItem\":\"\",\"originHash\":\"\",\"fields\":" + fieldsJson(item.fields) + "}";
                }
            }
        }
        try { writeAtomically(cache_ / "index.json", "{\"items\":[" + index + "]}\n"); } catch(const std::exception&) {} // a cache: reading works without it
        // Favorites and recents of items that went stay in the user file, for when they return.
        return uint32_t(items_.size());
    }

    Control::LibraryResult Library::query(const Control::LibraryQuery& q) const
    {
        Control::LibraryResult result;
        const auto text = lower(q.text);
        std::vector<Control::LibraryItem> found;
        std::vector<std::string> kinds; // the types asked for, by kind (RFC 0023)
        for(const auto& t : q.types) kinds.push_back(itemType(t) ? itemType(t)->kind() : t);
        for(auto item : items_)
        {
            item.favorite = std::find(favorites_.begin(), favorites_.end(), item.id) != favorites_.end();
            const auto r = std::find(recent_.begin(), recent_.end(), item.id);
            item.recent = r == recent_.end() ? 0 : uint32_t(r - recent_.begin()) + 1;
            // Every chosen tag (AND), the text in its name, tags or description, and the toggles.
            if(!std::all_of(q.tags.begin(), q.tags.end(), [&](const std::string& t) { return std::find(item.tags.begin(), item.tags.end(), t) != item.tags.end(); })) continue;
            if(q.favorites && !item.favorite) continue;
            if(!q.kind.empty() && item.kind != q.kind) continue;
            if(!kinds.empty() && std::find(kinds.begin(), kinds.end(), item.kind) == kinds.end()) continue;
            if(!q.id.empty() && item.id != q.id) continue;
            if(!q.source.empty() && item.library != (q.source == "mine" ? std::string() : q.source)) continue;
            if(q.recent && !item.recent) continue;
            if(!text.empty())
            {
                std::string hay = lower(item.name + ' ' + item.description);
                for(const auto& t : item.tags) hay += ' ' + lower(t);
                if(hay.find(text) == std::string::npos) continue;
            }
            found.push_back(item);
        }
        // Recent first when asked for, else by name.
        std::sort(found.begin(), found.end(), [&](const auto& a, const auto& b) { return q.recent ? a.recent < b.recent : lower(a.name) < lower(b.name); });
        std::map<std::string, uint32_t> counts;
        for(const auto& item : found) for(const auto& t : item.tags) if(std::find(hiddenTags_.begin(), hiddenTags_.end(), t) == hiddenTags_.end()) ++counts[t];
        result.tags.assign(counts.begin(), counts.end());
        result.tagColors.assign(tagColors_.begin(), tagColors_.end());
        result.hiddenTags = hiddenTags_;
        result.total = uint32_t(found.size());
        if(found.size() > q.limit) found.resize(q.limit);
        result.items = std::move(found);
        return result;
    }

    std::vector<Control::InstalledLibrary> Library::installed() const { return installed_; }

    const Control::LibraryItem* Library::find(const std::string& id) const
    {
        const auto it = std::find_if(items_.begin(), items_.end(), [&](const auto& i) { return i.id == id; });
        return it == items_.end() ? nullptr : &*it;
    }

    void Library::setFavorite(const std::string& id, bool favorite)
    {
        if(!find(id)) throw std::invalid_argument("no such library item");
        std::erase(favorites_, id);
        if(favorite) favorites_.push_back(id);
        writeUser();
    }

    void Library::setTag(const std::string& tag, bool hidden, const std::string& color)
    {
        if(tag.empty() || tag.size() > maxTag || tag.find('\0') != std::string::npos) throw std::invalid_argument("invalid tag");
        if(!validColor(color)) throw std::invalid_argument("a tag's color is #rrggbb, or empty for none");
        std::erase(hiddenTags_, tag);
        if(hidden) hiddenTags_.push_back(tag);
        if(color.empty()) tagColors_.erase(tag); else tagColors_[tag] = color;
        writeUser();
    }

    void Library::used(const std::string& id)
    {
        std::erase(recent_, id);
        recent_.insert(recent_.begin(), id);
        if(recent_.size() > maxRecent) recent_.resize(maxRecent);
        writeUser();
    }

    fs::path Library::newFolder(const std::string& name, const std::string& kind) const
    {
        std::string safe;
        for(const char c : name) safe += (std::isalnum(static_cast<unsigned char>(c)) || c == ' ' || c == '-' || c == '_') ? c : '_';
        while(!safe.empty() && safe.back() == ' ') safe.pop_back();
        const auto type = itemType(kind);
        if(!type || !type->indexed()) throw std::invalid_argument("no item type " + kind + " keeps folders in the library");
        if(safe.empty()) safe = type->name;
        const auto root = home_ / type->folder;
        const auto& extension = type->suffix;
        auto folder = root / (safe + extension);
        // A dangling link counts as taken: an install's cleanup would remove it.
        for(int n = 2; fs::exists(fs::symlink_status(folder)); ++n) folder = root / (safe + " " + std::to_string(n) + extension);
        return folder;
    }

    void Library::readUser()
    {
        try
        {
            const auto doc = parse(slurp(cache_ / "user.json"));
            favorites_ = strings(detail::member(doc.get(), "favorites"));
            recent_ = strings(detail::member(doc.get(), "recent"));
            hiddenTags_ = strings(detail::member(doc.get(), "hiddenTags"));
            tagColors_.clear();
            if(V colors = detail::member(doc.get(), "tagColors"); colors && colors->type == json_type_object)
                for(auto* e = static_cast<const json_object_s*>(colors->payload)->start; e; e = e->next)
                {
                    const std::string tag(e->name->string, e->name->string_size), color = str(e->value);
                    if(validColor(color) && !color.empty()) tagColors_[tag] = color;
                }
            folders_.clear();
            if(V folders = detail::member(doc.get(), "folders")) folders_ = strings(folders); // absent in files written before step 3
        }
        catch(const std::exception&) { favorites_.clear(); recent_.clear(); hiddenTags_.clear(); tagColors_.clear(); folders_.clear(); } // none yet
    }

    void Library::writeUser() const
    {
        std::string colors;
        for(const auto& [tag, color] : tagColors_) colors += std::string(colors.empty() ? "" : ",") + detail::quote(tag) + ":" + detail::quote(color);
        writeAtomically(cache_ / "user.json", "{\"favorites\":" + list(favorites_) + ",\"recent\":" + list(recent_) + ",\"hiddenTags\":" + list(hiddenTags_) +
                                              ",\"tagColors\":{" + colors + "},\"folders\":" + list(folders_) + "}\n");
    }

    void Library::installType(const ItemType& type, const std::string& manifest)
    {
        writeAtomically(home_ / "Types" / type.owner / (type.id + ".json"), manifest);
        removeItemType(type.id); // a new version of it from the same package
        addItemType(type);
    }

    void Library::forgetOrphans()
    {
        // Not at every rescan: an install rescans after placing its types and before its items.
        for(const auto& t : itemTypes())
            if(!t.owner.empty() && std::none_of(installed_.begin(), installed_.end(), [&](const auto& l) { return l.id == t.owner; }) &&
               std::none_of(items_.begin(), items_.end(), [&](const auto& i) { return i.kind == t.kind(); }))
            {
                std::error_code ignored;
                fs::remove(home_ / "Types" / t.owner / (t.id + ".json"), ignored);
                fs::remove(home_ / "Types" / t.owner, ignored); // when empty
                removeItemType(t.id);
            }
    }

    void Library::addFolder(const std::string& folder)
    {
        std::error_code error;
        const auto path = fs::canonical(folder, error);
        if(error || !fs::is_directory(path, error)) throw std::invalid_argument("not a folder: " + folder);
        if(std::find(folders_.begin(), folders_.end(), path.string()) != folders_.end()) return;
        folders_.push_back(path.string());
        writeUser();
        rescan();
    }

    void Library::removeFolder(const std::string& folder)
    {
        if(!std::erase(folders_, folder)) throw std::invalid_argument("not a folder the library indexes: " + folder);
        writeUser();
        rescan();
    }
}
