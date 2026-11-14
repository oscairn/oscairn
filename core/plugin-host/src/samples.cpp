// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/samples.h"
#include <algorithm>
#include <cerrno>
#include <csignal>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <memory>
#include <stdexcept>
#include <fcntl.h>
#include <sys/file.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

namespace oscairn::assets
{
    namespace fs = std::filesystem;
    namespace
    {
        constexpr uint64_t page = SP_PACK_PAGE, maxPack = SP_PACK_MAX_BYTES;
        [[noreturn]] void fail(const std::string& why) { throw std::runtime_error(why); }
        [[noreturn]] void failErrno(const std::string& what) { fail(what + ": " + std::strerror(errno)); }

        bool hexId(const std::string& s) { return s.size() == 64 && std::all_of(s.begin(), s.end(), [](char c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'); }); }
        std::string hex(const uint8_t* id)
        {
            constexpr char digits[] = "0123456789abcdef"; std::string out;
            for(int i = 0; i < SP_SAMPLE_ID_BYTES; ++i) { out += digits[id[i] >> 4]; out += digits[id[i] & 15]; }
            return out;
        }
        void unhex(const std::string& s, uint8_t* id) { for(int i = 0; i < SP_SAMPLE_ID_BYTES; ++i) id[i] = uint8_t(std::stoi(s.substr(2 * i, 2), nullptr, 16)); }

        // The library's status codes by name (samplepack.h), for messages people read.
        std::string status(int code)
        {
            static const char* const decoder[] = {"OK", "INVALID_ARGUMENT", "BUFFER_TOO_SMALL", "TRUNCATED", "BAD_SYNC", "RESERVED_VALUE", "UNSUPPORTED", "HEADER_CRC", "BAD_SUBFRAME", "BAD_RESIDUAL", "OVERRUN", "FRAME_CRC", "BAD_STREAM"};
            static const char* const pack[] = {"NOT_PACK", "UNSUPPORTED_VERSION", "NO_FOOTER", "TOO_LARGE", "BAD_CHECKSUM", "BAD_ENTRY", "BAD_FOOTER", "BAD_TRAILER", "BAD_OFFSET", "OVERLAP", "RESERVED_NONZERO", "BAD_FORMAT", "BAD_LAYOUT",
                "BAD_FRAME_TABLE", "BAD_CREDIT", "DUPLICATE_ID", "NOT_LIVE", "MISMATCH", "NOT_FOUND", "FRAME_MISMATCH", "PADDING", "WINDOW_MISMATCH", "MD5_MISMATCH", "ID_MISMATCH", "TRAILER_MISMATCH", "NOT_RECORD", "RECOVER_LIMIT"};
            static const char* const waveform[] = {"NOT_WAVEFORM", "WF_VERSION", "WF_FLAGS", "WF_LENGTH", "WF_SIZE", "WF_CHECKSUM", "WF_SAMPLE_COUNT", "WF_BANK_COUNT", "WF_RESERVED", "WF_RANGE", "WF_LOW_ABOVE_HIGH", "WF_SAMPLE_INDEX",
                "WF_LOOP", "WF_NOT_SORTED", "WF_DUPLICATE_BANK", "WF_SAMPLE_ORDER", "WF_UNUSED_SAMPLE", "WF_DUPLICATE_SAMPLE", "WF_END"};
            const char* name = code >= 0 && code <= 12 ? decoder[code] : code >= 32 && code <= 58 ? pack[code - 32] : code >= 64 && code <= 82 ? waveform[code - 64] : "unknown";
            return std::string("SP_ERR_") + name + " (" + std::to_string(code) + ")";
        }

        // A read-only mapping of a whole file; empty for an empty file.
        struct Mapping
        {
            const uint8_t* data = nullptr; size_t size = 0;
            explicit Mapping(const fs::path& file)
            {
                const int fd = ::open(file.c_str(), O_RDONLY | O_CLOEXEC);
                if(fd < 0) failErrno("cannot read " + file.string());
                struct stat info{};
                if(::fstat(fd, &info) != 0) { ::close(fd); failErrno("cannot read " + file.string()); }
                size = size_t(info.st_size);
                void* p = size ? ::mmap(nullptr, size, PROT_READ, MAP_SHARED, fd, 0) : nullptr;
                ::close(fd);
                if(p == MAP_FAILED) failErrno("cannot map " + file.string());
                data = static_cast<const uint8_t*>(p);
            }
            Mapping(Mapping&& o) noexcept : data(o.data), size(o.size) { o.data = nullptr; o.size = 0; }
            Mapping(const Mapping&) = delete;
            ~Mapping() { if(data) ::munmap(const_cast<uint8_t*>(data), size); }
        };

        // Durable on the storage itself: macOS's fsync leaves data in the drive's cache.
        void sync(int fd, const fs::path& file)
        {
#ifdef __APPLE__
            if(::fcntl(fd, F_FULLFSYNC) == 0) return;
#endif
            if(::fsync(fd) != 0) failErrno("cannot sync " + file.string());
        }
        void syncFolder(const fs::path& folder)
        {
            const int fd = ::open(folder.c_str(), O_RDONLY | O_CLOEXEC);
            if(fd < 0) failErrno("cannot open " + folder.string());
            ::fsync(fd); ::close(fd);
        }
        void writeAt(int fd, const uint8_t* data, size_t size, uint64_t at, const fs::path& file)
        {
            while(size)
            {
                const auto n = ::pwrite(fd, data, size, off_t(at));
                if(n < 0) { if(errno == EINTR) continue; failErrno("cannot write " + file.string()); }
                data += n; size -= size_t(n); at += uint64_t(n);
            }
        }
        struct Fd { int fd; ~Fd() { if(fd >= 0) ::close(fd); } };
        // Writers in every process take turns on the folder's lock file.
        struct FolderLock
        {
            Fd lock{-1};
            explicit FolderLock(const fs::path& folder)
            {
                fs::create_directories(folder);
                lock.fd = ::open((folder / ".lock").c_str(), O_RDWR | O_CREAT | O_CLOEXEC, 0600);
                if(lock.fd < 0) failErrno("cannot lock " + folder.string());
                while(::flock(lock.fd, LOCK_EX) < 0) if(errno != EINTR) failErrno("cannot lock " + folder.string());
            }
        };

        // The two entries a writer makes beside the records it copies (docs/format.md sections 5
        // and 8). shortcut: written here because samplepack's C interface has no writer yet;
        // replace with its write functions when it gains them. Every pack written is opened by the
        // library before it is used, so a fault here refuses the write instead of passing on.
        uint32_t crc32c(const uint8_t* p, size_t n, uint32_t crc = 0xFFFFFFFFu)
        {
            while(n--) { crc ^= *p++; for(int k = 0; k < 8; ++k) crc = crc >> 1 ^ (0x82F63B78u & (0u - (crc & 1))); }
            return crc;
        }
        void put32(uint8_t* p, uint32_t v) { std::memcpy(p, &v, 4); } // the format is little-endian, as every target is
        void put64(uint8_t* p, uint64_t v) { std::memcpy(p, &v, 8); }
        void prefix(uint8_t* e, const char* magic, uint64_t length, uint32_t checked)
        {
            std::memcpy(e, magic, 4); e[4] = 1; e[5] = 0; e[6] = 0; e[7] = 0; // major 1, minor 0
            put64(e + 8, length); put32(e + 16, checked); put32(e + 20, 0);
            put32(e + 20, ~crc32c(e, checked));
        }
        std::vector<uint8_t> header() { std::vector<uint8_t> h(page); prefix(h.data(), "OSPH", page, 24); return h; }
        uint64_t trailerLength(size_t count) { return (32 + 56 * uint64_t(count) + 32 + page - 1) / page * page; }
        // A trailer at `at` listing `entries` (in file order) with its footer.
        std::vector<uint8_t> trailer(const std::vector<sp_index_entry>& entries, uint64_t at)
        {
            const auto length = trailerLength(entries.size());
            std::vector<uint8_t> t(length);
            put32(t.data() + 24, uint32_t(entries.size()));
            for(size_t i = 0; i < entries.size(); ++i)
            {
                auto* e = t.data() + 32 + 56 * i;
                std::memcpy(e, entries[i].id, 32); put64(e + 32, entries[i].offset); put64(e + 40, entries[i].length); put64(e + 48, entries[i].preload);
            }
            prefix(t.data(), "OSPI", length, uint32_t(32 + 56 * entries.size()));
            auto* f = t.data() + length - 32;
            std::memcpy(f, "OSPF", 4); put64(f + 8, at); put64(f + 16, length); put32(f + 28, ~crc32c(f, 28));
            return t;
        }

        // A pack's live records, sorted by id, as its trailer lists them.
        std::vector<sp_index_entry> indexOf(const uint8_t* data, size_t size)
        {
            size_t count = 0;
            auto s = sp_pack_index(data, size, nullptr, 0, &count);
            std::vector<sp_index_entry> out(count);
            if(s == SP_ERR_BUFFER_TOO_SMALL || s == SP_OK) s = sp_pack_index(data, size, out.data(), out.size(), &count);
            if(s != SP_OK) fail("its trailer does not read: " + status(s));
            return out;
        }
        // The test suite stops a process in the middle of an append here (OSCAIRN_TEST_KILL_SAMPLE_APPEND=records|trailer).
        bool killAt(const char* point) { const char* at = std::getenv("OSCAIRN_TEST_KILL_SAMPLE_APPEND"); return at && !std::strcmp(at, point); }
    }

    bool contentRef(const std::string& path, std::string& handler, std::string& id)
    {
        const auto colon = path.find(':');
        if(colon == std::string::npos) return false;
        handler = path.substr(0, colon); id = path.substr(colon + 1);
        return (handler == "sample" || handler == "waveform") && hexId(id);
    }

    SamplePacks::SamplePacks(fs::path folder) : folder_(std::move(folder)) {}

    void SamplePacks::refresh()
    {
        std::vector<fs::path> files; std::error_code e;
        for(const auto& entry : fs::directory_iterator(folder_, e))
        {
            const auto name = entry.path().filename().string();
            if(name.starts_with(".pack-")) fs::remove(entry.path(), e); // a pack being made when a process stopped
            else if(name.starts_with("pack-") && name.ends_with(".osp")) files.push_back(entry.path());
        }
        std::sort(files.begin(), files.end());
        std::vector<Pack> fresh;
        for(const auto& file : files)
        {
            const auto size = fs::file_size(file);
            const auto known = std::find_if(packs_.begin(), packs_.end(), [&](const Pack& p) { return p.path == file; });
            if(known != packs_.end() && known->length == size) { fresh.push_back(std::move(*known)); continue; }
            Mapping m(file);
            sp_pack_info info{};
            auto s = sp_pack_open(m.data, m.size, &info);
            if(s == SP_ERR_NO_FOOTER)
            {
                // An append this device was making when it stopped: cut back to its last commit.
                uint64_t committed = 0;
                if(const auto r = sp_pack_recover(m.data, m.size, &committed); r != SP_OK) fail("the sample pack " + file.string() + " is damaged, not torn: " + status(r));
                Fd fd{::open(file.c_str(), O_RDWR | O_CLOEXEC)};
                if(fd.fd < 0 || ::ftruncate(fd.fd, off_t(committed)) != 0) failErrno("cannot recover " + file.string());
                sync(fd.fd, file);
                Mapping cut(file);
                Pack p{file, cut.size, {}};
                if((s = sp_pack_open(cut.data, cut.size, &info)) != SP_OK) fail("the sample pack " + file.string() + " does not open after recovery: " + status(s));
                try { p.index = indexOf(cut.data, cut.size); } catch(const std::exception& x) { fail(file.string() + ": " + x.what()); }
                fresh.push_back(std::move(p));
                continue;
            }
            if(s != SP_OK) fail("the sample pack " + file.string() + " does not open: " + status(s));
            Pack p{file, m.size, {}};
            try { p.index = indexOf(m.data, m.size); } catch(const std::exception& x) { fail(file.string() + ": " + x.what()); }
            fresh.push_back(std::move(p));
        }
        packs_ = std::move(fresh);
        loaded_ = true;
    }

    std::optional<SamplePacks::Location> SamplePacks::lookup(const std::string& id) const
    {
        uint8_t key[SP_SAMPLE_ID_BYTES]; unhex(id, key);
        for(const auto& p : packs_)
            if(size_t at = 0; sp_index_find(p.index.data(), p.index.size(), key, &at) == SP_OK)
                return Location{p.path, p.index[at].offset, p.index[at].length, p.index[at].preload};
        return std::nullopt;
    }

    std::optional<SamplePacks::Location> SamplePacks::find(const std::string& id)
    {
        if(!hexId(id)) return std::nullopt;
        std::lock_guard lock(mutex_);
        if(loaded_) if(auto found = lookup(id)) return found;
        // Not seen yet: another process may have added it since.
        FolderLock folder(folder_);
        refresh();
        return lookup(id);
    }

    std::vector<SamplePacks::PackInfo> SamplePacks::packs()
    {
        std::lock_guard lock(mutex_);
        if(!loaded_) { FolderLock folder(folder_); refresh(); }
        std::vector<PackInfo> out;
        for(const auto& p : packs_) out.push_back({p.path, p.length, p.index.size()});
        return out;
    }

    SamplePacks::Pack& SamplePacks::create()
    {
        unsigned number = 1;
        if(!packs_.empty()) number = unsigned(std::stoul(packs_.back().path.filename().string().substr(5, 6))) + 1;
        char name[32]; std::snprintf(name, sizeof name, "pack-%06u.osp", number);
        const auto file = folder_ / name, temporary = folder_ / ("." + std::string(name));
        // An empty pack (its header alone), made whole under another name and renamed into place.
        {
            Fd fd{::open(temporary.c_str(), O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC, 0644)};
            if(fd.fd < 0) failErrno("cannot create " + temporary.string());
            const auto h = header();
            writeAt(fd.fd, h.data(), h.size(), 0, temporary);
            sync(fd.fd, temporary);
        }
        fs::rename(temporary, file);
        syncFolder(folder_);
        packs_.push_back({file, page, {}});
        return packs_.back();
    }

    // docs/format.md 10.2: the records at the committed length, synced, then a trailer listing
    // every live record, synced. Only then is the index changed, from what the library reads back.
    void SamplePacks::commit(Pack& pack, std::vector<Record>& records)
    {
        Fd fd{::open(pack.path.c_str(), O_RDWR | O_CLOEXEC)};
        if(fd.fd < 0) failErrno("cannot write " + pack.path.string());
        const auto committed = pack.length;
        try
        {
            auto at = committed;
            for(auto& r : records) { r.entry.offset = at; writeAt(fd.fd, r.bytes, r.entry.length, at, pack.path); at += r.entry.length; }
            sync(fd.fd, pack.path);
            if(killAt("records")) std::raise(SIGKILL);
            auto live = pack.index;
            for(const auto& r : records) live.push_back(r.entry);
            std::sort(live.begin(), live.end(), [](const auto& a, const auto& b) { return a.offset < b.offset; });
            const auto t = trailer(live, at);
            writeAt(fd.fd, t.data(), killAt("trailer") ? t.size() / 2 : t.size(), at, pack.path);
            if(killAt("trailer")) std::raise(SIGKILL);
            sync(fd.fd, pack.path);
            Mapping m(pack.path);
            sp_pack_info info{};
            if(const auto s = sp_pack_open(m.data, m.size, &info); s != SP_OK || m.size != at + t.size()) fail("the sample pack " + pack.path.string() + " does not open after an append: " + status(s));
            auto index = indexOf(m.data, m.size);
            pack.index = std::move(index);
            pack.length = m.size;
        }
        catch(...)
        {
            // Back to the last commit, as recovery would.
            if(::ftruncate(fd.fd, off_t(committed)) == 0) ::fsync(fd.fd);
            throw;
        }
    }

    size_t SamplePacks::install(const std::vector<fs::path>& files)
    {
        // Every file verified first, from wherever it came: it opens as it is (never recovered),
        // holds one live record, which passes full verification, and is named by its id.
        struct Verified { Mapping file; sp_index_entry entry; };
        std::vector<Verified> verified;
        auto scratch = std::make_unique<sp_verify_scratch>();
        for(const auto& file : files)
        {
            const auto name = file.filename().string();
            auto refuse = [&](const std::string& why) { fail("the sample " + name + " is refused: " + why); };
            if(!name.ends_with(".osp") || !hexId(name.substr(0, name.size() - 4))) refuse("its name is not its id and .osp");
            Mapping m(file);
            sp_pack_info info{};
            if(const auto s = sp_pack_open(m.data, m.size, &info); s != SP_OK) refuse("it is not a whole sample pack: " + status(s));
            if(info.records != 1) refuse("it holds " + std::to_string(info.records) + " samples, not one");
            size_t entries = 0;
            sp_pack_rebuild_index(m.data, m.size, nullptr, 0, &entries); // its records and tombstones, for the table full verification needs
            std::vector<sp_index_entry> table(std::max<size_t>(entries, 1));
            if(const auto s = sp_pack_verify(m.data, m.size, scratch.get(), table.data(), table.size()); s != SP_OK) refuse("it does not pass verification: " + status(s));
            const auto index = indexOf(m.data, m.size);
            if(index.size() != 1 || hex(index[0].id) != name.substr(0, 64)) refuse("its record's id is not its name");
            verified.push_back({std::move(m), index[0]});
        }
        std::lock_guard lock(mutex_);
        FolderLock folder(folder_);
        refresh();
        size_t added = 0;
        Pack* pack = packs_.empty() ? nullptr : &packs_.back();
        std::vector<Record> pending;
        uint64_t end = pack ? pack->length : 0;
        for(const auto& v : verified)
        {
            const auto id = hex(v.entry.id);
            if(lookup(id) || std::any_of(pending.begin(), pending.end(), [&](const Record& r) { return !std::memcmp(r.entry.id, v.entry.id, 32); })) continue;
            const auto fits = [&](const Pack* p) { return p && end + v.entry.length + trailerLength(p->index.size() + pending.size() + 1) <= maxPack; };
            if(!fits(pack))
            {
                if(!pending.empty()) { added += pending.size(); commit(*pack, pending); pending.clear(); }
                pack = &create();
                end = pack->length;
            }
            pending.push_back({v.file.data + v.entry.offset, {{}, 0, v.entry.length, v.entry.preload}});
            std::memcpy(pending.back().entry.id, v.entry.id, 32);
            end += v.entry.length;
        }
        if(!pending.empty()) { added += pending.size(); commit(*pack, pending); }
        return added;
    }

    void SamplePacks::exportSample(const std::string& id, const fs::path& to)
    {
        const auto at = find(id);
        if(!at) fail("sample " + id + " is not installed");
        Mapping m(at->pack);
        sp_index_entry entry{{}, at->offset, at->length, at->preload};
        unhex(id, entry.id);
        sp_record_info info{};
        if(const auto s = sp_pack_record(m.data, m.size, &entry, &info); s != SP_OK) fail("sample " + id + " does not read in " + at->pack.string() + ": " + status(s));
        // A pack header, the record byte for byte, and a trailer listing it.
        Fd fd{::open(to.c_str(), O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0644)};
        if(fd.fd < 0) failErrno("cannot write " + to.string());
        const auto h = header();
        writeAt(fd.fd, h.data(), h.size(), 0, to);
        writeAt(fd.fd, m.data + at->offset, at->length, page, to);
        entry.offset = page;
        const auto t = trailer({entry}, page + at->length);
        writeAt(fd.fd, t.data(), t.size(), page + at->length, to);
        Mapping written(to);
        sp_pack_info check{};
        if(const auto s = sp_pack_open(written.data, written.size, &check); s != SP_OK || check.records != 1) fail("the exported sample " + to.string() + " does not open: " + status(s));
    }

    Waveforms::Waveforms(fs::path folder) : folder_(std::move(folder)) {}

    namespace
    {
        // A waveform file's bytes, checked and its id recomputed by the library; `why` says what is wrong, empty when nothing.
        std::string readWaveform(const fs::path& file, const std::string& id, std::string& why)
        {
            std::error_code e;
            const auto size = fs::file_size(file, e);
            if(e) { why = "it cannot be read"; return {}; }
            if(size > SP_WAVEFORM_MAX_BYTES) { why = "it is larger than a waveform can be"; return {}; }
            std::ifstream in(file, std::ios::binary);
            std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
            const auto* data = reinterpret_cast<const uint8_t*>(bytes.data());
            size_t samples = 0, banks = 0;
            uint8_t computed[SP_SAMPLE_ID_BYTES];
            if(const auto s = sp_waveform_check(data, bytes.size(), &samples, &banks); s != SP_OK) why = "it is not a valid waveform: " + status(s);
            else if(const auto t = sp_waveform_id(data, bytes.size(), computed); t != SP_OK) why = "its id cannot be computed: " + status(t);
            else if(hex(computed) != id) why = "its id is not its name";
            return bytes;
        }
    }

    size_t Waveforms::install(const std::vector<fs::path>& files)
    {
        for(const auto& file : files)
        {
            const auto name = file.filename().string();
            std::string why;
            if(!name.ends_with(".owf") || !hexId(name.substr(0, name.size() - 4))) why = "its name is not its id and .owf";
            else readWaveform(file, name.substr(0, 64), why);
            if(!why.empty()) fail("the waveform " + name + " is refused: " + why);
        }
        size_t added = 0;
        fs::create_directories(folder_);
        for(const auto& file : files)
        {
            const auto to = folder_ / file.filename();
            if(fs::exists(to)) continue;
            // Whole or not at all: copied beside it under a name of its own, then renamed.
            const auto temporary = folder_ / ("." + file.filename().string() + "." + std::to_string(::getpid()));
            fs::copy_file(file, temporary, fs::copy_options::overwrite_existing);
            fs::rename(temporary, to);
            ++added;
        }
        return added;
    }

    fs::path Waveforms::path(const std::string& id) const
    {
        if(!hexId(id)) fail("not a waveform id: " + id);
        const auto file = folder_ / (id + ".owf");
        std::error_code e;
        if(!fs::is_regular_file(file, e)) fail("waveform " + id + " is not installed");
        std::string why;
        readWaveform(file, id, why);
        if(!why.empty()) fail("the installed waveform " + id + " is refused: " + why);
        return file;
    }

    std::vector<std::string> Waveforms::samples(const std::string& id) const
    {
        std::string why;
        const auto bytes = readWaveform(path(id), id, why);
        const auto* data = reinterpret_cast<const uint8_t*>(bytes.data());
        size_t count = 0, banks = 0;
        sp_waveform_check(data, bytes.size(), &count, &banks);
        std::vector<std::string> out;
        uint8_t sample[SP_SAMPLE_ID_BYTES];
        for(size_t i = 0; i < count; ++i) if(sp_waveform_sample(data, bytes.size(), i, sample) == SP_OK) out.push_back(hex(sample));
        return out;
    }
}
