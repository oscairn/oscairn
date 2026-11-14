// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <samplepack.h>
#include <filesystem>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

// Items stored by content through a host handler (RFC 0023's handlers, docs/rfcs/0023-item-types.md):
// samples kept in Oscairn Sample Packs and waveforms, each named by the id the samplepack library
// computes from its content (docs/library-archive.md, "Samples and waveforms"). The library is the
// only parser of their files; nothing here reads audio.
namespace oscairn::assets
{
    // A reference to such an item rather than a file: "<handler>:<64 lowercase hex digits>", the
    // handler "sample" or "waveform". Its handler and id; false for any other path.
    bool contentRef(const std::string& path, std::string& handler, std::string& id);

    // The device's samples: records copied byte for byte from verified one-record packs into
    // multi-record packs of its own (pack-000001.osp ... in `folder`), appended to, never rewritten,
    // each at most 1 GiB. An index in memory, 56 bytes a sample, rebuilt from each pack's trailer
    // on first use (a torn tail cut back to its last commit, as only this device writes these
    // packs), answers where a sample is. Thread-safe; processes sharing the folder take turns on its
    // lock file to write, and a sample one has not seen is looked for again in the packs.
    class SamplePacks
    {
    public:
        explicit SamplePacks(std::filesystem::path folder);
        struct Location { std::filesystem::path pack; uint64_t offset = 0, length = 0, preload = 0; };
        std::optional<Location> find(const std::string& id);
        // Each file must be a one-record pack whose record passes full verification and whose
        // id is its file's name (<id>.osp); one that is not refuses them all, naming it, before
        // anything is written. Those not held already are appended in the order given, one
        // commit per pack. Returns how many were added.
        size_t install(const std::vector<std::filesystem::path>& files);
        // A held sample as a one-record pack at `to`, its record copied byte for byte.
        void exportSample(const std::string& id, const std::filesystem::path& to);
        struct PackInfo { std::filesystem::path path; uint64_t bytes = 0; size_t samples = 0; };
        std::vector<PackInfo> packs(); // for diagnostics
    private:
        struct Pack { std::filesystem::path path; uint64_t length = 0; std::vector<sp_index_entry> index; }; // index sorted by id
        struct Record { const uint8_t* bytes; sp_index_entry entry; };
        const std::filesystem::path folder_;
        std::mutex mutex_;
        bool loaded_ = false;
        std::vector<Pack> packs_; // in file name order: the last is appended to
        void refresh(); // under mutex_ and the folder's lock
        std::optional<Location> lookup(const std::string& id) const;
        void commit(Pack& pack, std::vector<Record>& records);
        Pack& create();
    };

    // The device's waveforms: <id>.owf files in `folder`, each one checked by the library and its
    // id recomputed whenever it is installed or resolved (at most 73,748 bytes).
    class Waveforms
    {
    public:
        explicit Waveforms(std::filesystem::path folder);
        // Each file must pass sp_waveform_check and have its name's id; one that does not refuses
        // them all. Those not held are copied in. Returns how many were added.
        size_t install(const std::vector<std::filesystem::path>& files);
        // The file of a held waveform, checked again; throws when it is not held or not valid.
        std::filesystem::path path(const std::string& id) const;
        // The ids of the samples a held waveform names, in its order.
        std::vector<std::string> samples(const std::string& id) const;
    private:
        const std::filesystem::path folder_;
    };
}
