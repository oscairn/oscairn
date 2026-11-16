// SPDX-License-Identifier: AGPL-3.0-or-later
// A test instrument that uses samples as a sampler does, without being one: its state is a list
// of references, "sample:<id>" and "waveform:<id>", which it lists as its asset dependencies. On
// each state it loads it resolves every sample (a waveform's through resolve and the samples it names) with
// oscairn.assets' resolve_sample, maps the pack and decodes the record's first block with the
// samplepack library, and appends a line per sample to the file OSCAIRN_SAMPLE_FIXTURE_REPORT
// names, after "pid <its process>": "<sample id> <frames> <FNV-1a 64 of the block's first channel>", or "failed <ref> <reason>".
// A new instance takes its references from OSCAIRN_SAMPLE_FIXTURE_REFS (comma-separated), resolving
// none until a state is loaded, as the host's asset store is set after a plugin is activated.
// Like a Sampler with no sound it is silent until content is loaded: once a state resolved and
// decoded a sample, a held note sounds a tone; a state naming a sample the store lacks leaves it silent.
#include <clap/clap.h>
#include <oscairn-clap/assets.h>
#include <samplepack.h>
#include <cinttypes>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <vector>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

namespace
{
    const char* features[]{CLAP_PLUGIN_FEATURE_INSTRUMENT, nullptr};
    const clap_plugin_descriptor_t descriptor{CLAP_VERSION, "org.oscairn.test.sample-fixture", "Sample fixture", "Oscairn", "", "", "", "1", "Resolves and decodes samples", features};

    void report(const std::string& line)
    {
        if(const char* file = std::getenv("OSCAIRN_SAMPLE_FIXTURE_REPORT"))
            if(FILE* f = std::fopen(file, "a")) { std::fprintf(f, "%s\n", line.c_str()); std::fclose(f); }
    }
    bool unhex(const std::string& s, uint8_t* out)
    {
        if(s.size() != 64) return false;
        for(int i = 0; i < 32; ++i) out[i] = uint8_t(std::strtoul(s.substr(2 * i, 2).c_str(), nullptr, 16));
        return true;
    }
    std::string hex(const uint8_t* id) { std::string s; char b[3]; for(int i = 0; i < 32; ++i) { std::snprintf(b, sizeof b, "%02x", id[i]); s += b; } return s; }

    struct Plugin
    {
        clap_plugin_t api{};
        const clap_host_t* host;
        std::vector<std::string> refs;
        bool ready = false;      // a sample decoded and none failed: held notes sound
        int held = 0;            // note ons without their note off
        uint64_t phase = 0;
        std::unique_ptr<sp_flac_scratch> scratch = std::make_unique<sp_flac_scratch>();

        // As a sampler loads one: where the record is, then its first block decoded from a read-only mapping of the pack.
        bool decode(const oscairn_host_assets_t* assets, const std::string& id)
        {
            oscairn_asset_ref_t ref{};
            std::snprintf(ref.path, sizeof ref.path, "sample:%s", id.c_str());
            char path[4096], why[512] = "";
            oscairn_sample_location_t at{};
            if(!assets->resolve_sample || !assets->resolve_sample(host, &ref, path, sizeof path, &at, why, sizeof why)) { report("failed sample:" + id + " " + why); return false; }
            const int fd = ::open(path, O_RDONLY);
            struct stat info{};
            if(fd < 0 || ::fstat(fd, &info) != 0) { report("failed sample:" + id + " cannot open " + path); if(fd >= 0) ::close(fd); return false; }
            void* map = ::mmap(nullptr, size_t(info.st_size), PROT_READ, MAP_SHARED, fd, 0);
            ::close(fd);
            if(map == MAP_FAILED) { report("failed sample:" + id + " cannot map"); return false; }
            sp_index_entry entry{{}, at.offset, at.length, at.preload};
            unhex(id, entry.id);
            std::vector<int16_t> ch0(SP_FLAC_MAX_BLOCK), ch1(SP_FLAC_MAX_BLOCK);
            uint64_t frames = 0;
            const auto s = sp_record_decode(static_cast<const uint8_t*>(map), size_t(info.st_size), &entry, 0, 1, 0, scratch.get(), ch0.data(), ch1.data(), ch0.size(), &frames);
            ::munmap(map, size_t(info.st_size));
            if(s != SP_OK) { report("failed sample:" + id + " decode " + std::to_string(s)); return false; }
            uint64_t h = 1469598103934665603ull;
            for(uint64_t i = 0; i < frames; ++i) for(int b = 0; b < 2; ++b) { h ^= uint8_t(uint16_t(ch0[i]) >> (8 * b)); h *= 1099511628211ull; }
            char line[160];
            std::snprintf(line, sizeof line, "%s %" PRIu64 " %016" PRIx64, id.c_str(), frames, h);
            report(line);
            return true;
        }
        void load()
        {
            const auto* assets = static_cast<const oscairn_host_assets_t*>(host->get_extension(host, OSCAIRN_EXT_ASSETS));
            if(!assets) { report("failed all no oscairn.assets draft 4"); return; }
            report("pid " + std::to_string(::getpid())); // which process it runs in: the engine's or a worker's
            ready = false;
            bool failed = false, any = false;
            auto one = [&](const std::string& id) { const bool ok = decode(assets, id); any = any || ok; failed = failed || !ok; };
            for(const auto& r : refs)
            {
                if(r.rfind("sample:", 0) == 0) { one(r.substr(7)); continue; }
                oscairn_asset_ref_t ref{};
                std::snprintf(ref.path, sizeof ref.path, "%s", r.c_str());
                char path[4096], why[512] = "";
                if(!assets->resolve(host, &ref, path, sizeof path, why, sizeof why)) { report("failed " + r + " " + why); failed = true; continue; }
                // A waveform: its samples, by the library's reading of the file.
                std::vector<uint8_t> bytes(SP_WAVEFORM_MAX_BYTES);
                FILE* f = std::fopen(path, "rb");
                const size_t n = f ? std::fread(bytes.data(), 1, bytes.size(), f) : 0;
                if(f) std::fclose(f);
                size_t samples = 0, banks = 0;
                if(sp_waveform_check(bytes.data(), n, &samples, &banks) != SP_OK) { report("failed " + r + " not a waveform"); failed = true; continue; }
                uint8_t id[32];
                for(size_t i = 0; i < samples; ++i) if(sp_waveform_sample(bytes.data(), n, i, id) == SP_OK) one(hex(id));
            }
            ready = any && !failed;
        }

        explicit Plugin(const clap_host_t* h) : host(h)
        {
            api.desc = &descriptor; api.plugin_data = this;
            api.init = [](const clap_plugin_t* p) {
                auto& self = get(p);
                if(const char* list = std::getenv("OSCAIRN_SAMPLE_FIXTURE_REFS"); list && *list)
                {
                    for(std::string rest = list; !rest.empty();) { const auto comma = rest.find(','); self.refs.push_back(rest.substr(0, comma)); rest = comma == std::string::npos ? "" : rest.substr(comma + 1); }
                }
                return true;
            };
            api.destroy = [](const clap_plugin_t* p) { delete &get(p); };
            api.activate = [](const clap_plugin_t*, double, uint32_t, uint32_t) { return true; };
            api.deactivate = [](const clap_plugin_t*) {};
            api.start_processing = [](const clap_plugin_t*) { return true; };
            api.stop_processing = [](const clap_plugin_t*) {};
            api.reset = [](const clap_plugin_t*) {};
            api.process = [](const clap_plugin_t* p, const clap_process_t* b) -> clap_process_status {
                auto& self = get(p);
                for(uint32_t i = 0; b->in_events && i < b->in_events->size(b->in_events); ++i)
                {
                    const auto* e = b->in_events->get(b->in_events, i);
                    if(e->type == CLAP_EVENT_NOTE_ON) ++self.held;
                    else if((e->type == CLAP_EVENT_NOTE_OFF || e->type == CLAP_EVENT_NOTE_CHOKE) && self.held > 0) --self.held;
                }
                for(uint32_t o = 0; o < b->audio_outputs_count; ++o) for(uint32_t c = 0; c < b->audio_outputs[o].channel_count; ++c) std::memset(b->audio_outputs[o].data32[c], 0, b->frames_count * sizeof(float));
                if(self.ready && self.held > 0 && b->audio_outputs_count)
                    for(uint32_t i = 0; i < b->frames_count; ++i, ++self.phase)
                        for(uint32_t c = 0; c < b->audio_outputs[0].channel_count; ++c) b->audio_outputs[0].data32[c][i] = float(.2 * std::sin(double(self.phase) * 6.283185307179586 * 220 / 48000));
                return CLAP_PROCESS_CONTINUE;
            };
            api.get_extension = [](const clap_plugin_t*, const char* id) -> const void* {
                static const clap_plugin_audio_ports_t ports{
                    [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 0 : 1; },
                    [](const clap_plugin_t*, uint32_t i, bool input, clap_audio_port_info_t* p) -> bool {
                        if(input || i) return false;
                        *p = {}; p->id = 1; std::strcpy(p->name, "Main"); p->flags = CLAP_AUDIO_PORT_IS_MAIN; p->channel_count = 2; p->port_type = CLAP_PORT_STEREO; p->in_place_pair = CLAP_INVALID_ID;
                        return true;
                    }};
                static const clap_plugin_note_ports_t notes{
                    [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 1 : 0; },
                    [](const clap_plugin_t*, uint32_t i, bool input, clap_note_port_info_t* p) -> bool {
                        if(!input || i) return false;
                        *p = {}; p->id = 1; std::strcpy(p->name, "Notes"); p->supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI; p->preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
                        return true;
                    }};
                static const clap_plugin_state_t state{
                    [](const clap_plugin_t* p, const clap_ostream_t* out) {
                        std::string text; for(const auto& r : get(p).refs) text += r + '\n';
                        return text.empty() || out->write(out, text.data(), text.size()) == int64_t(text.size());
                    },
                    [](const clap_plugin_t* p, const clap_istream_t* in) {
                        std::string text; char chunk[4096]; int64_t got;
                        while((got = in->read(in, chunk, sizeof chunk)) > 0) text.append(chunk, size_t(got));
                        auto& self = get(p); self.refs.clear();
                        for(size_t at = 0; at < text.size();) { const auto end = text.find('\n', at); self.refs.push_back(text.substr(at, end - at)); at = end == std::string::npos ? text.size() : end + 1; }
                        self.load();
                        return true;
                    }};
                static const oscairn_plugin_asset_dependencies_t dependencies{
                    [](const clap_plugin_t* p) -> uint32_t { return uint32_t(get(p).refs.size()); },
                    [](const clap_plugin_t* p, uint32_t i, oscairn_asset_ref_t* ref) {
                        const auto& refs = get(p).refs;
                        if(i >= refs.size()) return false;
                        *ref = {};
                        std::snprintf(ref->path, sizeof ref->path, "%s", refs[i].c_str());
                        std::snprintf(ref->sha256, sizeof ref->sha256, "%s", refs[i].substr(refs[i].find(':') + 1).c_str());
                        return true;
                    },
                    nullptr};
                if(!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &ports;
                if(!std::strcmp(id, CLAP_EXT_NOTE_PORTS)) return &notes;
                if(!std::strcmp(id, CLAP_EXT_STATE)) return &state;
                if(!std::strcmp(id, OSCAIRN_EXT_ASSET_DEPENDENCIES)) return &dependencies;
                return nullptr;
            };
            api.on_main_thread = [](const clap_plugin_t*) {};
        }
        static Plugin& get(const clap_plugin_t* p) { return *static_cast<Plugin*>(p->plugin_data); }
    };
    const clap_plugin_factory_t factory{
        [](const clap_plugin_factory_t*) -> uint32_t { return 1; },
        [](const clap_plugin_factory_t*, uint32_t i) { return i ? nullptr : &descriptor; },
        [](const clap_plugin_factory_t*, const clap_host_t* h, const char* id) -> const clap_plugin_t* { return std::strcmp(id, descriptor.id) ? nullptr : &(new Plugin(h))->api; }};
}
extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION, [](const char*) { return true; }, []() {},
    [](const char* id) -> const void* { return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? nullptr : &factory; }};
