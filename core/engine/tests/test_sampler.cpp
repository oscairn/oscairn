// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Test fixture: the sampler the host's tests use (a stand-in for the Oscairn Sampler, org.oscairn.sampler,
// which lives in its own repository). A sound's patch text is SFZ-shaped, "<region> sample=NAME ...": NAME is a
// file resolved through the host's asset store (oscairn.assets) against the library and folder prepareSound
// gives, decoded and held whole in memory as float audio (touched, so the memory is real; sounds on one file
// share one copy), and played from its start by a note, at its own speed, ending with the sample or, once released,
// its ampeg_release (10 ms if none). transpose is accepted and ignored; any other opcode is refused, as the real one does. A saved state keeps the library, folder and name, and reads the
// file again when it is loaded. No parameters, no presets, no memory declaration: the host measures it.
#include <clap/clap.h>
#include <oscairn-clap/assets.h>
#include <oscairn-clap/sound-contexts-plugin.hpp>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iterator>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <vector>

namespace
{
    const char* const features[] = {CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_SAMPLER, CLAP_PLUGIN_FEATURE_STEREO, nullptr};
    const clap_plugin_descriptor_t descriptor = {CLAP_VERSION_INIT, "org.oscairn.sampler", "Test Sampler", "Oscairn", "https://github.com/oscairn/oscairn", "", "", "1",
                                                 "Plays one WAV sample per sound, for the host's tests", features};

    using Audio = std::shared_ptr<const std::vector<float>>; // mono

    // A 16-bit PCM or 32-bit float WAV, its channels averaged; the rate is not read, the sample plays at the output's.
    Audio readWav(const std::string& path)
    {
        std::ifstream in(path, std::ios::binary);
        const std::string d((std::istreambuf_iterator<char>(in)), {});
        auto u16 = [&](size_t at) { return uint32_t(uint8_t(d[at]) | uint8_t(d[at + 1]) << 8); };
        auto u32 = [&](size_t at) { return u16(at) | u16(at + 2) << 16; };
        if(d.size() < 12 || d.compare(0, 4, "RIFF") || d.compare(8, 4, "WAVE")) return {};
        uint32_t format = 0, channels = 0, bits = 0;
        for(size_t at = 12; at + 8 <= d.size();)
        {
            const size_t size = u32(at + 4), body = at + 8;
            if(!d.compare(at, 4, "fmt ") && body + 16 <= d.size()) { format = u16(body); channels = u16(body + 2); bits = u16(body + 14); }
            else if(!d.compare(at, 4, "data") && channels && ((format == 1 && bits == 16) || (format == 3 && bits == 32)))
            {
                const size_t bytes = std::min(size, d.size() - body), width = bits / 8, frames = bytes / (width * channels);
                auto out = std::make_shared<std::vector<float>>(frames);
                for(size_t f = 0; f < frames; ++f)
                {
                    double sum = 0;
                    for(uint32_t c = 0; c < channels; ++c)
                    {
                        const size_t p = body + (f * channels + c) * width;
                        if(bits == 16) sum += int16_t(u16(p)) / 32768.0;
                        else { const uint32_t v = u32(p); float x; std::memcpy(&x, &v, 4); sum += x; }
                    }
                    (*out)[f] = float(sum / channels);
                }
                return out;
            }
            at = body + size + (size & 1);
        }
        return {};
    }

    struct Voice : oscairn::contexts::VoiceBase
    {
        const float* data = nullptr;
        size_t size = 0, at = 0;
        float gain = 1, fade = 1, step = 0;
        bool releasing = false;
    };

    struct Patch
    {
        std::string library, directory, file, sha; // what the sound names, kept in a saved state (sha: the store's, from resolving it)
        double release = .01;                      // ampeg_release, seconds
        Audio audio;                          // null: silent (nothing named, or the file not found)
    };

    struct Sampler : oscairn::contexts::Engine<Sampler, Voice, Patch>
    {
        const clap_host_t* host = nullptr;
        double sampleRate = 48000;
        std::map<std::string, std::weak_ptr<const std::vector<float>>> loaded; // main thread: one copy per file
        static constexpr const char* stateVersion = "oscairn-test-sampler-1";
        static constexpr size_t maxStateBytes = 16384;

        // Main thread: the patch's file through the asset store, read into memory.
        void load(Patch& patch)
        {
            patch.audio = nullptr;
            const auto* assets = static_cast<const oscairn_host_assets_t*>(host ? host->get_extension(host, OSCAIRN_EXT_ASSETS) : nullptr);
            if(!assets || !assets->resolve || patch.file.empty()) return;
            oscairn_asset_ref_t ref{};
            std::snprintf(ref.library, sizeof ref.library, "%s", patch.library.c_str());
            std::snprintf(ref.path, sizeof ref.path, "%s%s%s", patch.directory.c_str(), patch.directory.empty() ? "" : "/", patch.file.c_str());
            char path[4096], why[256];
            if(!assets->resolve(host, &ref, path, sizeof path, why, sizeof why)) return;
            auto& shared = loaded[path];
            patch.sha = ref.sha256;
            if(!(patch.audio = shared.lock())) { patch.audio = readWav(path); shared = patch.audio; }
        }

        bool accepts(const Patch&, const clap_event_note_t&) { return true; }
        void start(Voice& v, Context& c)
        {
            v.data = c.patch.audio ? c.patch.audio->data() : nullptr; v.size = c.patch.audio ? c.patch.audio->size() : 0;
            v.at = 0; v.gain = float(.25 + .5 * v.velocity); v.fade = 1; v.releasing = false; v.step = float(1.0 / (std::max(c.patch.release, .001) * sampleRate));
        }
        void release(Voice& v) { v.releasing = true; }
        void selectedGlobally(Context&, const clap_output_events_t*) {}
        void applyParam(const clap_event_header_t*) {}
        void render(const clap_process_t* process, uint32_t from, uint32_t to)
        {
            if(!process->audio_outputs_count) return;
            auto& out = process->audio_outputs[0];
            if(out.channel_count < 2 || !out.data32 || !out.data32[0] || !out.data32[1]) return;
            for(uint32_t i = from; i < to; ++i)
            {
                eventTime = i;
                float sum = 0;
                for(auto& v : voices)
                {
                    if(!v.active) continue;
                    if(v.at >= v.size || v.fade <= 0) { finishVoice(v); continue; }
                    sum += v.data[v.at++] * v.gain * v.fade * float(amplitude(v, i - from));
                    if(v.releasing) v.fade -= v.step;
                }
                out.data32[0][i] = out.data32[1][i] = sum;
            }
        }

        // prepareSound's patch text, SFZ-shaped: opcodes name=value, a value running to the next opcode (a file name may hold spaces).
        // sample, ampeg_release (seconds) and transpose (ignored) are known; any other opcode is refused, as a sampler does.
        static uint32_t prepare(const clap_plugin_t* p, const char* name, const oscairn_sound_parameter_t*, uint32_t count, const char* text, const oscairn_asset_ref_t* base, char* message, uint32_t capacity)
        {
            auto fail = [&](const std::string& why) { if(message && capacity) std::snprintf(message, capacity, "%s", why.c_str()); return CLAP_INVALID_ID; };
            if(!name || std::strlen(name) > 63 || count) return fail("a sound takes no parameters");
            Patch patch;
            if(text)
            {
                std::istringstream lines(text); std::string line;
                for(int number = 1; std::getline(lines, line); ++number)
                {
                    std::istringstream words(line); std::string word, key, value; bool open = false;
                    auto close = [&] { if(!open) return true; open = false; if(key == "sample") patch.file = value; else if(key == "ampeg_release") patch.release = std::atof(value.c_str()); else if(key != "transpose") return false; return true; };
                    for(; words >> word;)
                    {
                        const auto eq = word.find('=');
                        if(word[0] != '<' && eq != std::string::npos && eq > 0 && word.find_first_not_of("abcdefghijklmnopqrstuvwxyz_0123456789") >= eq)
                        { if(!close()) return fail("line " + std::to_string(number) + ": unsupported opcode " + key); key = word.substr(0, eq); value = word.substr(eq + 1); open = true; }
                        else if(open) value += " " + word;
                    }
                    if(!close()) return fail("line " + std::to_string(number) + ": unsupported opcode " + key);
                }
                if(patch.file.empty()) return fail("no sample= in the patch");
                if(base) { patch.library = base->library; patch.directory = base->path; }
            }
            auto& s = self(p); auto* c = s.freeSlot(); if(!c) return fail("no free sound context");
            s.load(patch);
            if(text && !patch.audio) return fail("the sample could not be read");
            c->patch = std::move(patch);
            return s.publish(*c, name);
        }
        bool writePatch(std::ostream& out, const Patch& patch) { out << ' ' << std::quoted(patch.library) << ' ' << std::quoted(patch.directory) << ' ' << std::quoted(patch.file) << ' ' << patch.release; return true; }
        bool readPatch(std::istream& in, Patch& patch)
        {
            if(!(in >> std::quoted(patch.library) >> std::quoted(patch.directory) >> std::quoted(patch.file) >> patch.release) || !(patch.release >= 0 && patch.release < 1e4)) return false;
            load(patch); // a file the store lacks leaves the sound silent, as a sampler without it
            return true;
        }
        // oscairn.asset-dependencies: every file the prepared sounds name (main thread).
        std::vector<const Patch*> used() const { std::vector<const Patch*> v; for(const auto& c : contexts) if(c.prepared && !c.patch.file.empty()) v.push_back(&c.patch); return v; }
        bool readPreset(uint32_t, const char*, const char*, Patch&, std::string& error) { error = "no presets"; return false; }
        void alone(Patch&) {}
        uint32_t patchValues(const Patch&, oscairn_sound_parameter_t*, uint32_t) { return 0; }
        void restored() {}
    };

    Sampler& self(const clap_plugin_t* p) { return *static_cast<Sampler*>(p->plugin_data); }

    const clap_plugin_audio_ports_t audioPorts = {
        [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 0 : 1; },
        [](const clap_plugin_t*, uint32_t index, bool input, clap_audio_port_info_t* info) {
            if(input || index) return false;
            *info = {}; info->id = 0; info->flags = CLAP_AUDIO_PORT_IS_MAIN; info->channel_count = 2; info->port_type = CLAP_PORT_STEREO; info->in_place_pair = CLAP_INVALID_ID;
            std::snprintf(info->name, sizeof info->name, "Main"); return true; }};
    const clap_plugin_note_ports_t notePorts = {
        [](const clap_plugin_t*, bool input) -> uint32_t { return input ? 1 : 0; },
        [](const clap_plugin_t*, uint32_t index, bool input, clap_note_port_info_t* info) {
            if(!input || index) return false;
            info->id = 0; info->supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI; info->preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
            std::snprintf(info->name, sizeof info->name, "Notes"); return true; }};
    const oscairn_plugin_asset_dependencies_t assetDependencies = {
        [](const clap_plugin_t* p) { return uint32_t(self(p).used().size()); },
        [](const clap_plugin_t* p, uint32_t index, oscairn_asset_ref_t* ref) {
            const auto v = self(p).used(); if(index >= v.size()) return false;
            *ref = {}; std::snprintf(ref->library, sizeof ref->library, "%s", v[index]->library.c_str());
            std::snprintf(ref->path, sizeof ref->path, "%s%s%s", v[index]->directory.c_str(), v[index]->directory.empty() ? "" : "/", v[index]->file.c_str());
            std::snprintf(ref->sha256, sizeof ref->sha256, "%s", v[index]->sha.c_str()); return true; },
        nullptr};
    const clap_plugin_state_t snapshot{Sampler::saveSnapshot, Sampler::loadSnapshot};
    const oscairn_plugin_sound_contexts_t soundContexts{Sampler::capacity, Sampler::get, Sampler::prepare, Sampler::select, Sampler::discard, &snapshot, Sampler::preparePreset};
    const oscairn_plugin_sound_snapshots_t soundSnapshots{Sampler::prepareSnapshot, Sampler::setRingOut, Sampler::chokeRetiring, Sampler::saveSound, Sampler::soundValues};
    const oscairn_plugin_voice_budget_t voiceBudget{Sampler::honorsVolume, Sampler::setLimits, Sampler::activeVoices, Sampler::voiceNotes, Sampler::importance};

    const clap_plugin_t* create(const clap_host_t* host)
    {
        auto* s = new Sampler;
        s->host = host;
        s->plugin.desc = &descriptor;
        s->plugin.plugin_data = s;
        s->plugin.init = [](const clap_plugin_t*) { return true; };
        s->plugin.destroy = [](const clap_plugin_t* p) { delete &self(p); };
        s->plugin.activate = [](const clap_plugin_t* p, double rate, uint32_t, uint32_t) { auto& s = self(p); s.sampleRate = rate; s.setSampleRate(rate); s.resetVoices(); return true; };
        s->plugin.deactivate = [](const clap_plugin_t*) {};
        s->plugin.start_processing = [](const clap_plugin_t*) { return true; };
        s->plugin.stop_processing = [](const clap_plugin_t*) {};
        s->plugin.reset = [](const clap_plugin_t* p) { self(p).resetVoices(); };
        s->plugin.process = Sampler::process;
        s->plugin.get_extension = [](const clap_plugin_t*, const char* id) -> const void* {
            if(!std::strcmp(id, OSCAIRN_EXT_SOUND_CONTEXTS)) return &soundContexts;
            if(!std::strcmp(id, OSCAIRN_EXT_SOUND_SNAPSHOTS)) return &soundSnapshots;
            if(!std::strcmp(id, OSCAIRN_EXT_VOICE_BUDGET_DRAFT_2) || !std::strcmp(id, OSCAIRN_EXT_VOICE_BUDGET)) return &voiceBudget;
            if(!std::strcmp(id, OSCAIRN_EXT_ASSET_DEPENDENCIES)) return &assetDependencies;
            if(!std::strcmp(id, CLAP_EXT_AUDIO_PORTS)) return &audioPorts;
            if(!std::strcmp(id, CLAP_EXT_NOTE_PORTS)) return &notePorts;
            return nullptr;
        };
        s->plugin.on_main_thread = [](const clap_plugin_t*) {};
        return &s->plugin;
    }

    const clap_plugin_factory_t factory = {
        [](const clap_plugin_factory_t*) -> uint32_t { return 1; },
        [](const clap_plugin_factory_t*, uint32_t index) -> const clap_plugin_descriptor_t* { return index == 0 ? &descriptor : nullptr; },
        [](const clap_plugin_factory_t*, const clap_host_t* host, const char* id) -> const clap_plugin_t* { return std::strcmp(id, descriptor.id) == 0 ? create(host) : nullptr; },
    };
}

extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry = {
    CLAP_VERSION_INIT,
    [](const char*) { return true; },
    []() {},
    [](const char* id) -> const void* { return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) == 0 ? &factory : nullptr; },
};
