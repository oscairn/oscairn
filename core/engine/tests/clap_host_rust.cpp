// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The Rust CLAP host (rust_clap_*, core/clap-host/rust) against what its C++ reference did, recorded (kick::clap::Host,
// deleted since; core/clap-host/tests/vectors, one file per target, as failure messages and the fixtures' builds differ
// between systems), through clap_host.h: every plugin of each library loaded, described and played the same way, in a
// process of its own (a binary is opened once per process), writing down everything the engine could see: the
// description, flags, parameter texts, each block's audio, its output events, parameter values and outputs, sleep and
// reset, the counters, limits and failures as kind and message. The Rust host's transcript must match the recording
// item by item, the first difference named, and its blocks must allocate nothing (malloc and operator new counted
// while they run). Paths are written as $SCRATCH and $PLUGINS (the plugins' folder), so a recording replays from another
// checkout. A recording holds the test's fixtures, not Oscairn's plugins, whose every release would change it.
//   oscairn-clap-host-rust-test --replay FILE PLUGIN.clap...   (exits 77, skipped, where FILE does not exist)
//   oscairn-clap-host-rust-test --record FILE PLUGIN.clap...   (the Rust host's transcript, for a deliberate change)
#include "../../clap-host/tests/AllocationGuard.h"
#include "../../plugin-host/src/clap_host.h"
#include <kick_clap/Discovery.h>
#include <oscairn/transport.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <tuple>
#include <oscairn-clap/beat.h>
#include <oscairn-clap/harmony.h>
#include <oscairn-clap/param-target.h>
#include <oscairn-clap/voice-budget.h>
#include <vector>
#include <sys/wait.h>
#include <unistd.h>
#ifdef __APPLE__
#include <malloc/malloc.h>
#endif

// Every malloc this process makes while `counting`: the Rust host's (the System allocator) and C++'s alike.
static thread_local bool counting = false;
static thread_local size_t allocations = 0;
// A sanitizer owns malloc (its runtime calls it before this program's code can run): there the count is not made, and the Rust blocks' not allocating is
// checked by the builds without one.
#if defined(__SANITIZE_THREAD__) || defined(__SANITIZE_ADDRESS__)
#define OSCAIRN_NO_MALLOC_COUNT 1
#elif defined(__has_feature)
#if __has_feature(thread_sanitizer) || __has_feature(address_sanitizer)
#define OSCAIRN_NO_MALLOC_COUNT 1
#endif
#endif
#ifdef OSCAIRN_NO_MALLOC_COUNT
#elif defined(__APPLE__)
extern "C" void* malloc(size_t n) { if(counting) ++allocations; return malloc_zone_malloc(malloc_default_zone(), n); }
extern "C" void* calloc(size_t c, size_t n) { if(counting) ++allocations; return malloc_zone_calloc(malloc_default_zone(), c, n); }
extern "C" void* realloc(void* p, size_t n) { if(counting) ++allocations; return malloc_zone_realloc(malloc_default_zone(), p, n); }
extern "C" int posix_memalign(void** p, size_t a, size_t n) { if(counting) ++allocations; *p = malloc_zone_memalign(malloc_default_zone(), a, n); return *p ? 0 : 12; }
#elif defined(__GLIBC__)
extern "C" void* __libc_malloc(size_t); extern "C" void* __libc_calloc(size_t, size_t); extern "C" void* __libc_realloc(void*, size_t);
extern "C" void* __libc_memalign(size_t, size_t);
extern "C" void* malloc(size_t n) { if(counting) ++allocations; return __libc_malloc(n); }
extern "C" void* calloc(size_t c, size_t n) { if(counting) ++allocations; return __libc_calloc(c, n); }
extern "C" void* realloc(void* p, size_t n) { if(counting) ++allocations; return __libc_realloc(p, n); }
extern "C" int posix_memalign(void** p, size_t a, size_t n) { if(counting) ++allocations; *p = __libc_memalign(a, n); return *p ? 0 : 12; }
#endif

namespace
{
    // The interface, as either implementation provides it.
    struct Api
    {
        decltype(&rust_clap_error) error; decltype(&rust_clap_new) make; decltype(&rust_clap_free) free; decltype(&rust_clap_revision) revision;
        decltype(&rust_clap_describe) describe; decltype(&rust_clap_note_input_ports) noteInputs; decltype(&rust_clap_note_output_ports) noteOutputs;
        decltype(&rust_clap_uses_clap_notes) usesClapNotes; decltype(&rust_clap_takes_sysex) takesSysex; decltype(&rust_clap_emits_sysex) emitsSysex;
        decltype(&rust_clap_supports_state) supportsState; decltype(&rust_clap_supports_sound_snapshot) supportsSoundSnapshot; decltype(&rust_clap_has_window) hasWindow;
        decltype(&rust_clap_voice_budget) voiceBudget; decltype(&rust_clap_honors_volume) honorsVolume; decltype(&rust_clap_service) service;
        decltype(&rust_clap_reset) reset; decltype(&rust_clap_declared_memory) declaredMemory; decltype(&rust_clap_effect_resources) effectResources;
        decltype(&rust_clap_parameter_value) value; decltype(&rust_clap_parameter_text) text; decltype(&rust_clap_set_parameter) set;
        decltype(&rust_clap_parameter_gesture) gesture; decltype(&rust_clap_parameter_reports) reports; decltype(&rust_clap_send_event) send;
        decltype(&rust_clap_pop_output) pop; decltype(&rust_clap_output_event_drops) drops; decltype(&rust_clap_statistics) statistics;
        decltype(&rust_clap_sound_context_capacity) capacity; decltype(&rust_clap_prepares_snapshots) preparesSnapshots; decltype(&rust_clap_saves_sounds) savesSounds;
        decltype(&rust_clap_rings_out) ringsOut; decltype(&rust_clap_process) process; decltype(&rust_clap_output_events) outputEvents;
        decltype(&rust_clap_panic) panic; decltype(&rust_clap_asleep) asleep; decltype(&rust_clap_restful) restful; decltype(&rust_clap_rested) rested;
        decltype(&rust_clap_was_reset) wasReset;
        decltype(&rust_clap_save_state) saveState; decltype(&rust_clap_load_state) loadState; decltype(&rust_clap_effect_context) effectContext;
        decltype(&rust_clap_sound_contexts) soundContexts; decltype(&rust_clap_prepare_sound) prepareSound; decltype(&rust_clap_prepare_preset) preparePreset;
        decltype(&rust_clap_select_sound) selectSound; decltype(&rust_clap_discard_sound) discardSound; decltype(&rust_clap_prepare_snapshot) prepareSnapshot;
        decltype(&rust_clap_save_sound) saveSound; decltype(&rust_clap_sound_values) soundValues; decltype(&rust_clap_choke_retiring) chokeRetiring;
        decltype(&rust_clap_asset_dependencies) assetDependencies; decltype(&rust_clap_load_preset) loadPreset;
        decltype(&rust_clap_presets) presets; decltype(&rust_clap_set_assets) setAssets;
        decltype(&rust_clap_set_voice_limits) setVoiceLimits; decltype(&rust_clap_voice_report) voiceReport; decltype(&rust_clap_mirror_wake) mirrorWake;
        decltype(&rust_clap_wants_events) wantsEvents; decltype(&rust_clap_value_rescans) valueRescans;
        decltype(&rust_clap_reconfigure) reconfigure; decltype(&rust_clap_set_active_outputs) setActiveOutputs; decltype(&rust_clap_show_window) showWindow;
    };
#define OSCAIRN_API(p) Api{p##clap_error, p##clap_new, p##clap_free, p##clap_revision, p##clap_describe, p##clap_note_input_ports, p##clap_note_output_ports, \
    p##clap_uses_clap_notes, p##clap_takes_sysex, p##clap_emits_sysex, p##clap_supports_state, p##clap_supports_sound_snapshot, p##clap_has_window, \
    p##clap_voice_budget, p##clap_honors_volume, p##clap_service, p##clap_reset, p##clap_declared_memory, p##clap_effect_resources, \
    p##clap_parameter_value, p##clap_parameter_text, p##clap_set_parameter, p##clap_parameter_gesture, p##clap_parameter_reports, p##clap_send_event, \
    p##clap_pop_output, p##clap_output_event_drops, p##clap_statistics, p##clap_sound_context_capacity, p##clap_prepares_snapshots, p##clap_saves_sounds, \
    p##clap_rings_out, p##clap_process, p##clap_output_events, p##clap_panic, p##clap_asleep, p##clap_restful, p##clap_rested, p##clap_was_reset, \
    p##clap_save_state, p##clap_load_state, p##clap_effect_context, p##clap_sound_contexts, p##clap_prepare_sound, p##clap_prepare_preset, \
    p##clap_select_sound, p##clap_discard_sound, p##clap_prepare_snapshot, p##clap_save_sound, p##clap_sound_values, p##clap_choke_retiring, \
    p##clap_asset_dependencies, p##clap_load_preset, p##clap_presets, p##clap_set_assets, p##clap_set_voice_limits, p##clap_voice_report, p##clap_mirror_wake, p##clap_wants_events, p##clap_value_rescans, \
    p##clap_reconfigure, p##clap_set_active_outputs, p##clap_show_window}

    // A transcript: labeled items, each its bytes.
    struct Transcript
    {
        std::string out;
        void item(const std::string& label, const void* data, size_t size)
        {
            const uint32_t l = uint32_t(label.size()), n = uint32_t(size);
            out.append(reinterpret_cast<const char*>(&l), 4).append(label).append(reinterpret_cast<const char*>(&n), 4).append(static_cast<const char*>(data), size);
        }
        template<class T> void put(const std::string& label, const T& v) { item(label, &v, sizeof v); }
        void text(const std::string& label, const char* s) { item(label, s, s ? std::strlen(s) : 0); }
    };
    struct Visit { Transcript* t; std::string prefix; };

    constexpr double rate = 48000; constexpr uint32_t frames = 256, blocks = 400;

    void failure(Transcript& t, const Api& a, const std::string& label) { int kind = 0; const char* m = a.error(&kind); t.put(label + " kind", kind); t.text(label + " message", m); }
    // A call's result, and its failure when it failed.
    void result(Transcript& t, const Api& a, const std::string& label, int r) { t.put(label, r); if(r < 0 || r == 0) failure(t, a, label); }
    std::string scratch; // the same folder for each implementation's run, emptied between them: paths in messages agree
    std::string file(const std::string& path) { FILE* f = std::fopen(path.c_str(), "rb"); std::string r; if(f) { char b[4096]; size_t n; while((n = std::fread(b, 1, sizeof b, f)) > 0) r.append(b, n); std::fclose(f); } return r; }

    // A sound asset store for the hosts, given as the engine gives its own: libraries under /assets, made files refused,
    // samples in one pack.
    struct Store final : kick::clap::AssetResolver
    {
        std::string resolve(const std::string& library, const std::string& path, std::string& sha256) override
        {
            if(sha256.empty()) sha256 = std::string(64, '4');
            return "/assets/" + library + "/" + path;
        }
        std::string libraryRoot(const std::string& library, const std::string&) const override { return "/roots/" + library; }
        SampleLocation resolveSample(const std::string& id) override { return {"/packs/" + id.substr(0, 4) + ".pack", 4096, 512, 64}; }
    } resolver;
    const oscairn_clap_assets store = oscairn_clap_assets_of(&resolver);
    // Refuses an activation declaring more than 256 KiB shared (as a worker's engine with no room), while set.
    void refuseLarge(void*, const oscairn_memory_t* declared) { if(declared && declared->shared_bytes > (1u << 18)) throw std::runtime_error("no room for it here"); }

    // The control side: state, effect contexts, native sounds and snapshots, assets, presets, and their failures.
    void control(Transcript& t, const Api& a, oscairn_clap_host* h, const std::string& n, const std::vector<uint32_t>& ids, const std::vector<double>& minimum,
                 const std::vector<uint32_t>& flags, uint32_t ni, uint32_t no, bool& allocated)
    {
        const auto dir = scratch + "/" + n.substr(0, n.size() - 2);
        result(t, a, n + "save state", a.saveState(h, (dir + "-state").c_str()));
        t.text(n + "saved id", file(dir + "-state/plugin.id").c_str()); { const auto bytes = file(dir + "-state/state.clap"); t.item(n + "saved state", bytes.data(), bytes.size()); }
        result(t, a, n + "save again", a.saveState(h, (dir + "-state").c_str()));
        result(t, a, n + "save nowhere", a.saveState(h, ""));
        for(size_t i = 0; i < ids.size(); ++i) if(!(flags[i] & CLAP_PARAM_IS_READONLY)) { t.put(n + "set before load", a.set(h, ids[i], minimum[i])); break; } // queued, then dropped by the load
        result(t, a, n + "load state", a.loadState(h, (dir + "-state").c_str()));
        result(t, a, n + "load missing", a.loadState(h, (dir + "-missing").c_str()));
        { // a state not its own: the same plugin ID, other bytes
            std::filesystem::create_directories(dir + "-foreign");
            std::error_code none; std::filesystem::copy_file(dir + "-state/plugin.id", dir + "-foreign/plugin.id", std::filesystem::copy_options::overwrite_existing, none);
            FILE* f = std::fopen((dir + "-foreign/state.clap").c_str(), "wb"); if(f) { std::fputs("not a state", f); std::fclose(f); }
            result(t, a, n + "load foreign", a.loadState(h, (dir + "-foreign").c_str()));
        }
        { std::filesystem::create_directories(dir + "-other"); FILE* f = std::fopen((dir + "-other/plugin.id").c_str(), "wb"); if(f) { std::fputs("other.plugin", f); std::fclose(f); }
          result(t, a, n + "load other plugin's", a.loadState(h, (dir + "-other").c_str())); }
        auto* fx = a.effectContext(h);
        t.put(n + "effect context", fx != nullptr);
        kick::clap::Host::observeActivations(refuseLarge, nullptr);
        auto* refused = a.effectContext(h);
        kick::clap::Host::observeActivations(nullptr, nullptr);
        t.put(n + "refused effect context", refused != nullptr); if(refused) a.free(refused); else failure(t, a, n + "refused effect context");
        auto* second = a.effectContext(h);
        t.put(n + "second effect context", second != nullptr); if(second) a.free(second); else failure(t, a, n + "second effect context");
        if(fx)
        {
            t.put(n + "effect revision", a.revision(fx)); oscairn_effect_resources_t r{}; t.put(n + "effect resources", a.effectResources(fx, &r)); for(const uint64_t x : {r.shared_bytes, r.context_bytes, uint64_t(r.capacity), uint64_t(r.prepared), uint64_t(r.tail_active)}) t.put(n + "resources", x); // fields: its padding is the plugin's
            const auto e = kick::clap::Event::note(true, 64, 0.5, 0, 0, 0, 1); std::vector<float> o(4 * frames); float* op[4]{o.data(), o.data() + frames, o.data() + 2 * frames, o.data() + 3 * frames};
            t.put(n + "effect block", a.process(fx, nullptr, 0, op, 2, frames, reinterpret_cast<const oscairn_clap_event*>(&e), 1, nullptr)); t.item(n + "effect audio", o.data(), 2 * frames * sizeof(float));
            a.free(fx);
        } else failure(t, a, n + "effect context");
        auto list = [&](const std::string& label) {
            std::vector<oscairn_sound_context_info_t> all;
            a.soundContexts(h, [](void* c, const oscairn_sound_context_info_t* i) { static_cast<std::vector<oscairn_sound_context_info_t>*>(c)->push_back(*i); }, &all);
            for(const auto& c : all) { t.put(label + " id", c.id); t.text(label + " name", c.name); }
        };
        list(n + "sounds");
        std::vector<oscairn_sound_parameter_t> values;
        for(size_t i = 0; i < ids.size() && values.size() < 2; ++i) if(!(flags[i] & CLAP_PARAM_IS_READONLY))
        {
            oscairn_sound_parameter_t v; std::memset(&v, 0, sizeof v); v.id = ids[i]; v.value = minimum[i]; // zeroed whole: the recorder hashes the struct's padding too, which a braced push_back leaves as stack bytes
            values.push_back(v);
        }
        a.setAssets(h, &store);
        uint32_t sound = 0;
        uint32_t patched = 0;
        result(t, a, n + "prepare patch", a.prepareSound(h, "Patched", nullptr, 0, "region.sfz", "lib.one", "kit", &patched));
        result(t, a, n + "prepare", a.prepareSound(h, "Second", values.data(), uint32_t(values.size()), "", "", "", &sound)); t.put(n + "prepared", sound);
        result(t, a, n + "prepare bad", a.prepareSound(h, "A name far longer than the sixty-three bytes a sound's name may hold, surely", nullptr, 0, "", "", "", &sound));
        const std::string longest(63, 'n'), over(64, 'o');
        uint32_t named = 0;
        result(t, a, n + "prepare 63", a.prepareSound(h, longest.c_str(), nullptr, 0, "", "", "", &named)); t.put(n + "named", named);
        result(t, a, n + "prepare 64", a.prepareSound(h, over.c_str(), nullptr, 0, "", "", "", &named));
        result(t, a, n + "prepare refused", a.prepareSound(h, "Reject", nullptr, 0, "", "", "", &named));
        const oscairn_clap_preset first{"", 1, "", "first"};
        result(t, a, n + "prepare first", a.preparePreset(h, "First", &first, &named)); t.put(n + "named first", named);
        result(t, a, n + "prepare first 64", a.preparePreset(h, over.c_str(), &first, &named));
        const oscairn_clap_preset bogus{"", 0, "/nonexistent/preset", "key"};
        result(t, a, n + "prepare preset", a.preparePreset(h, "From a preset", &bogus, &sound));
        list(n + "sounds after");
        result(t, a, n + "select", a.selectSound(h, sound, 0));
        result(t, a, n + "select bad channel", a.selectSound(h, sound, 16));
        std::vector<float> in(2 * frames), out(std::max<uint32_t>(no, 2) * frames + 4 * frames);
        std::vector<const float*> ip; std::vector<float*> op;
        for(uint32_t c = 0; c < ni; ++c) ip.push_back(in.data() + (c % 2) * frames);
        for(uint32_t c = 0; c < no + 4; ++c) op.push_back(out.data() + c * frames);
        const uint32_t ring = a.ringsOut(h) ? 4 : 0;
        for(int b = 0; b < 12; ++b)
        {
            if(b == 9) result(t, a, n + "select again", a.selectSound(h, sound, 1)); // wakes it, asleep after silent blocks
            if(b == 10) result(t, a, n + "choke early", a.chokeRetiring(h));
            const auto e = kick::clap::Event::note(b % 4 == 0, 60, 0.7, 2, 0, 0, b);
            const uint32_t count = b < 6 ? 1 : 0;
            const size_t before = allocations; counting = true; forbidAllocation = true;
            const bool ok = a.process(h, ip.data(), ni, op.data(), no + (b % 2 ? ring : 0), frames, reinterpret_cast<const oscairn_clap_event*>(&e), count, nullptr);
            forbidAllocation = false; counting = false; allocated = allocated || allocations != before;
            t.put(n + "sound block", ok); t.put(n + "sound asleep", a.asleep(h)); t.item(n + "sound audio", out.data(), (no + ring) * frames * sizeof(float));
        }
        std::vector<oscairn_sound_parameter_t> got;
        result(t, a, n + "sound values", a.soundValues(h, sound, [](void* c, const oscairn_sound_parameter_t* v) { static_cast<std::vector<oscairn_sound_parameter_t>*>(c)->push_back(*v); }, &got));
        for(const auto& v : got) { t.put(n + "sound value id", v.id); t.put(n + "sound value", v.value); }
        result(t, a, n + "save sound", a.saveSound(h, sound, (dir + "-sound").c_str()));
        t.text(n + "sound snapshot", file(dir + "-sound/snapshot").c_str());
        { const auto bytes = file(dir + "-sound/state/state.clap"); t.item(n + "sound state", bytes.data(), bytes.size()); }
        uint32_t chosen[16]{};
        result(t, a, n + "prepare snapshot", a.prepareSnapshot(h, (dir + "-sound").c_str(), chosen)); t.item(n + "snapshot sounds", chosen, sizeof chosen);
        result(t, a, n + "prepare snapshot of state", a.prepareSnapshot(h, (dir + "-state").c_str(), chosen));
        result(t, a, n + "choke", a.chokeRetiring(h));
        result(t, a, n + "discard", a.discardSound(h, sound));
        result(t, a, n + "discard again", a.discardSound(h, sound));
        list(n + "sounds at last");
        std::vector<std::string> assets;
        result(t, a, n + "assets", a.assetDependencies(h, [](void* c, const oscairn_clap_asset* x) { static_cast<std::vector<std::string>*>(c)->push_back(std::string(x->library) + "/" + x->path + "#" + x->sha256 + (x->optional ? "?" : "")); }, &assets));
        for(const auto& x : assets) t.text(n + "asset", x.c_str());
        std::vector<std::string> listed;
        result(t, a, n + "presets", a.presets(h, [](void* c, const oscairn_clap_preset* p) { static_cast<std::vector<std::string>*>(c)->push_back(std::string(p->name) + "|" + std::to_string(p->kind) + "|" + p->location + "|" + p->load_key); }, &listed));
        for(const auto& x : listed) t.text(n + "preset", x.c_str());
        result(t, a, n + "load preset", a.loadPreset(h, &bogus));
        const oscairn_clap_preset inside{"", 1, "", "first"};
        result(t, a, n + "load preset inside", a.loadPreset(h, &inside));
        t.put(n + "value rescans", a.valueRescans(h)); // a preset loaded reads the values again
    }

    // The audio channels of its input and output ports.
    void channels(const Api& a, oscairn_clap_host* h, uint32_t& ni, uint32_t& no)
    {
        struct P { uint32_t* ni; uint32_t* no; } c{&ni, &no};
        const oscairn_clap_description ports{&c, [](void*, const oscairn_clap_descriptor*) {}, [](void*, const char*) {}, [](void*, const oscairn_clap_preset*) {},
            [](void*, uint32_t, const char*) {}, [](void* c, bool input, const oscairn_clap_port* p) { auto& x = *static_cast<P*>(c); *(input ? x.ni : x.no) += p->channels; },
            [](void*, const oscairn_clap_parameter*) {}};
        a.describe(h, &ports);
    }

    // The limits and refusals: offline rendering, the command queue filled past its capacity, a block bringing more events
    // than a process call takes, values and parameters it refuses, a rate change, its outputs activated, a window where
    // there is none, and a load with no memory room.
    void limits(Transcript& t, const Api& a, const std::string& path, const std::string& id)
    {
        const std::string n = id + " limits: ";
        auto* h = a.make(path.c_str(), id.c_str(), rate, frames, true, 0);
        t.put(n + "loaded offline", h != nullptr);
        if(!h) { failure(t, a, n + "load"); return; }
        uint32_t ni = 0, no = 0;
        channels(a, h, ni, no);
        std::vector<std::vector<float>> channel(ni + no, std::vector<float>(frames + 3, 0.25f));
        std::vector<const float*> in; std::vector<float*> out;
        for(uint32_t c = 0; c < ni + no; ++c) if(c < ni) in.push_back(channel[c].data()); else out.push_back(channel[c].data());
        auto block = [&](const std::string& label, uint32_t count, const kick::clap::Event* events, uint32_t length) {
            t.put(n + label, a.process(h, in.data(), ni, out.data(), no, length, reinterpret_cast<const oscairn_clap_event*>(events), count, nullptr));
            for(uint32_t c = 0; c < no; ++c) t.item(n + label + " audio", out[c], length * sizeof(float));
            uint32_t emitted = 0; const auto* e = a.outputEvents(h, &emitted); t.item(n + label + " events", e, emitted * sizeof(oscairn_clap_event));
            t.put(n + label + " reset", a.wasReset(h));
        };
        uint32_t queued = 0;
        for(uint32_t i = 0; i < kick::clap::Host::maxEvents + 8; ++i) { const auto e = kick::clap::Event::note(i % 2 == 0, 40 + int(i % 40), 0.5, 0, 0, 0, int(i)); queued += a.send(h, reinterpret_cast<const oscairn_clap_event*>(&e)); }
        t.put(n + "queued", queued);
        for(int b = 0; b < 6; ++b) block("queued block", 0, nullptr, frames);
        const auto bad = kick::clap::Event::note(true, 200, 0.5); t.put(n + "queued bad note", a.send(h, reinterpret_cast<const oscairn_clap_event*>(&bad)));
        const std::vector<kick::clap::Event> many(kick::clap::Host::maxProcessEvents + 1, kick::clap::Event::note(true, 60, 0.5, 1));
        block("too many events", uint32_t(many.size()), many.data(), frames);
        block("most events", kick::clap::Host::maxProcessEvents, many.data(), frames);
        block("longer block", 1, many.data(), frames + 3); // split in two, events rebased
        for(const auto& [param, value] : {std::pair{0u, std::nan("")}, std::pair{0u, HUGE_VAL}, std::pair{0xfffffff0u, 0.5}, std::pair{3u, 1.0}})
            t.put(n + "set refused", a.set(h, param, value));
        t.put(n + "gesture unknown", a.gesture(h, 0xfffffff0u, true));
        { std::string s = "untouched"; t.put(n + "text unknown", a.text(h, 0xfffffff0u, 0.5, [](void* c, const char* p, size_t size) { static_cast<std::string*>(c)->assign(p, size); }, &s)); t.text(n + "text unknown is", s.c_str()); }
        result(t, a, n + "reconfigure", a.reconfigure(h, 44100, frames / 2));
        block("block at 44.1 kHz", 0, nullptr, frames / 2);
        a.setActiveOutputs(h, 1);
        t.put(n + "service with outputs", a.service(h));
        block("block with outputs", 0, nullptr, frames / 2);
        if(!a.hasWindow(h)) result(t, a, n + "window", a.showWindow(h, true, "None"));
        oscairn_clap_statistics s{}; a.statistics(h, &s);
        for(const uint64_t c : {s.blocks, s.droppedEvents, s.processErrors, s.controlSilences}) t.put(n + "counter", c);
        a.free(h);
        auto* tight = a.make(path.c_str(), id.c_str(), rate, frames, false, 1);
        t.put(n + "loaded in a byte", tight != nullptr);
        if(tight) a.free(tight); else failure(t, a, n + "loaded in a byte");
    }

    void play(Transcript& t, const Api& a, const std::string& path, const std::string& id, bool& allocated)
    {
        const std::string n = id + ": ";
        auto* h = a.make(path.c_str(), id.c_str(), rate, frames, false, 0);
        t.put(n + "loaded", h != nullptr);
        if(!h) { failure(t, a, n + "load"); return; }
        // The description, item by item.
        Visit v{&t, n};
        std::vector<uint32_t> ids; std::vector<double> minimum, maximum, defaults; std::vector<uint32_t> flags;
        struct Ctx { Visit* v; std::vector<uint32_t>* ids; std::vector<double>* lo; std::vector<double>* hi; std::vector<double>* def; std::vector<uint32_t>* flags; } ctx{&v, &ids, &minimum, &maximum, &defaults, &flags};
        const oscairn_clap_description to{&ctx,
            [](void* c, const oscairn_clap_descriptor* d) { auto& x = *static_cast<Ctx*>(c)->v; for(const char* s : {d->path, d->id, d->name, d->vendor, d->version, d->description}) x.t->text(x.prefix + "descriptor", s); x.t->put(x.prefix + "preset discovery", d->preset_discovery); },
            [](void* c, const char* f) { auto& x = *static_cast<Ctx*>(c)->v; x.t->text(x.prefix + "feature", f); },
            [](void* c, const oscairn_clap_preset* p) { auto& x = *static_cast<Ctx*>(c)->v; x.t->text(x.prefix + "preset", p->name); x.t->put(x.prefix + "preset kind", p->kind); x.t->text(x.prefix + "preset location", p->location); x.t->text(x.prefix + "preset key", p->load_key); },
            [](void* c, uint32_t number, const char* name) { auto& x = *static_cast<Ctx*>(c)->v; x.t->put(x.prefix + "lane", number); x.t->text(x.prefix + "lane name", name); },
            [](void* c, bool input, const oscairn_clap_port* p) { auto& x = *static_cast<Ctx*>(c)->v; x.t->put(x.prefix + "port", input); x.t->put(x.prefix + "port id", p->id); x.t->text(x.prefix + "port name", p->name); x.t->put(x.prefix + "port channels", p->channels); x.t->put(x.prefix + "port flags", p->flags); },
            [](void* c, const oscairn_clap_parameter* p) {
                auto& k = *static_cast<Ctx*>(c); auto& x = *k.v; const auto q = x.prefix + "parameter " + std::to_string(p->id);
                x.t->text(q + " name", p->name); x.t->text(q + " module", p->module);
                for(const double d : {p->minimum, p->maximum, p->default_value}) x.t->put(q + " range", d);
                for(const uint32_t u : {p->flags, p->widget, p->priority, p->range_high, p->group, p->role, p->view, p->field, p->index}) x.t->put(q + " hint", u);
                x.t->item(q + " algorithms", p->algorithms, p->algorithm_count * 11); x.t->item(q + " seconds", p->seconds, p->second_count * sizeof(double));
                k.ids->push_back(p->id); k.lo->push_back(p->minimum); k.hi->push_back(p->maximum); k.def->push_back(p->default_value); k.flags->push_back(p->flags);
            }};
        a.describe(h, &to);
        t.put(n + "revision", a.revision(h));
        t.put(n + "note inputs", a.noteInputs(h)); t.put(n + "note outputs", a.noteOutputs(h));
        for(uint32_t i = 0; i < a.noteInputs(h); ++i) t.put(n + "clap notes", a.usesClapNotes(h, i));
        for(const bool b : {a.takesSysex(h), a.emitsSysex(h), a.supportsState(h), a.supportsSoundSnapshot(h), a.hasWindow(h), a.voiceBudget(h), a.honorsVolume(h),
                            a.preparesSnapshots(h), a.savesSounds(h), a.ringsOut(h)}) t.put(n + "flag", b);
        t.put(n + "wants", a.wantsEvents(h));
        t.put(n + "capacity", a.capacity(h));
        oscairn_memory_t m{}; t.put(n + "declared", a.declaredMemory(h, &m)); t.put(n + "memory", m);
        oscairn_effect_resources_t r{}; t.put(n + "effect resources", a.effectResources(h, &r));
        for(size_t i = 0; i < ids.size(); ++i)
            for(const double x : {minimum[i], defaults[i], maximum[i], 1e308})
            {
                std::string s; a.text(h, ids[i], x, [](void* c, const char* p, size_t size) { static_cast<std::string*>(c)->assign(p, size); }, &s);
                t.text(n + "text", s.c_str());
            }

        // The same blocks: input, notes, a playing transport, a parameter change halfway, a gesture, events sent straight.
        uint32_t ni = 0, no = 0;
        channels(a, h, ni, no);
        std::vector<std::vector<float>> input(ni, std::vector<float>(frames)), output(no, std::vector<float>(frames));
        std::vector<const float*> in; std::vector<float*> out;
        for(auto& c : input) in.push_back(c.data());
        for(auto& c : output) out.push_back(c.data());
        oscairn::Transport transport(rate, 120); transport.setPlaying(true);
        size_t writable = 0; while(writable < ids.size() && (flags[writable] & CLAP_PARAM_IS_READONLY)) ++writable;
        // The wake word a worker's engine reads: set by every wake, cleared by the block that takes it.
        oscairn_clap_wake wake{7};
        a.mirrorWake(h, &wake); t.put(n + "wake mirrored", wake.load());
        for(uint32_t block = 0; block < blocks; ++block)
        {
            // A sawtooth of exact values (no libm, whose last bits differ between systems), silent 40 blocks of 100.
            for(uint32_t c = 0; c < ni; ++c) for(uint32_t f = 0; f < frames; ++f) input[c][f] = float(int((block * frames + f + 37 * c) % 199) - 99) / 128 * (block % 100 < 60);
            std::vector<kick::clap::Event> sent;
            if(block % 20 == 1) sent.push_back(kick::clap::Event::note(true, 48 + int(block % 24), 0.8, 7, 0, 0, int(block)));
            if(block % 20 == 9) sent.push_back(kick::clap::Event::note(false, 48 + int((block - 8) % 24), 0, 3, 0, 0, int(block - 8)));
            if(block % 37 == 5) sent.push_back(kick::clap::Event::midi(0x90, 60, 100, 11));
            if(block % 37 == 6) sent.push_back(kick::clap::Event::midi(0x80, 60, 0, 2));
            // The other note port: MIDI to a CLAP-only port and CLAP notes to a MIDI-only one, translated; offs fanned out.
            if(block % 23 == 2) { sent.push_back(kick::clap::Event::midi(0x91, 62, 90, 5, 0)); sent.push_back(kick::clap::Event::note(true, 65, 0.8, 9, 3, 1, 77)); } // 0.8: velocity 101 in MIDI, 100 at a scale of 126
            if(block % 23 == 4) { sent.push_back(kick::clap::Event::note(false, 65, 0.3, 1, 3, -1, -1)); sent.push_back(kick::clap::Event::midi(0x81, 62, 0, 0, 0)); }
            if(block % 23 == 6) { auto choke = kick::clap::Event::note(false, -1, 0, 4, -1, -1, -1); reinterpret_cast<clap_event_header_t*>(choke.bytes.data())->type = CLAP_EVENT_NOTE_CHOKE; sent.push_back(choke); }
            // The graph's beat every block, a part's harmony (the same again, then gone a while).
            { oscairn_event_beat_t beat{}; beat.header = {sizeof beat, 0, kick::clap::Host::beatSpace, OSCAIRN_EVENT_BEAT, 0}; beat.beats = int64_t(block) << 20; beat.tempo = 120; sent.push_back(kick::clap::Event::from(beat)); }
            if(block % 50 < 30)
            {
                oscairn_event_harmony_t x{}; x.header = {sizeof x, 3, kick::clap::Host::harmonySpace, OSCAIRN_EVENT_HARMONY, 0};
                x.harmony.key_root = uint8_t(block / 50 % 12); x.harmony.chord_root = 2; x.harmony.chord_bass = 2; x.harmony.scale = 0xAB5; x.harmony.held[0] = block / 10;
                sent.push_back(kick::clap::Event::from(x));
            }
            // What the host must drop: bad notes, a bad MIDI byte, a late event, an unknown type, a lane's event.
            if(block % 45 == 7)
            {
                for(const auto& [velocity, channel, key] : {std::tuple{1.5, 0, 60}, std::tuple{0.5, 16, 60}, std::tuple{0.5, 0, 128}}) sent.push_back(kick::clap::Event::note(true, key, velocity, 1, channel, 0, 5));
                sent.push_back(kick::clap::Event::midi(0x90, 200, 1, 2)); sent.push_back(kick::clap::Event::note(true, 61, 0.5, frames, 0, 0, 6));
                auto odd = kick::clap::Event::note(true, 61, 0.5, 1, 0, 0, 6); reinterpret_cast<clap_event_header_t*>(odd.bytes.data())->type = 99; sent.push_back(odd);
                oscairn_event_param_target_t lane{}; lane.header = {sizeof lane, 0, kick::clap::Host::paramTargetSpace, OSCAIRN_EVENT_PARAM_TARGET, 0}; lane.value = 0.5; sent.push_back(kick::clap::Event::from(lane));
            }
            if(block == blocks / 2 && writable < ids.size())
            {
                t.put(n + "gesture", a.gesture(h, ids[writable], true));
                t.put(n + "set", a.set(h, ids[writable], minimum[writable] + (maximum[writable] - minimum[writable]) / 3));
                t.put(n + "gesture end", a.gesture(h, ids[writable], false));
                a.service(h);
            }
            if(block == blocks / 4) { const auto e = kick::clap::Event::note(true, 64, 0.5, 0, 0, 0, 999); t.put(n + "send", a.send(h, &e)); }
            // A parameter queued, then a panic: the block's parameter events survive the reset, delivered after it (notes and controllers do not).
            if(block == 3 * blocks / 4 && writable < ids.size()) t.put(n + "set before panic", a.set(h, ids[writable], maximum[writable]));
            if(block == 3 * blocks / 4) a.panic(h);
            if(block > 300 && block < 340 && a.restful(h)) { a.rested(h, frames, 1); t.put(n + "rested", block); transport.advance(frames); continue; }
            const auto now = transport.current();
            t.put(n + "wake before", wake.load());
            const oscairn_voice_budget_context_t contexts[2]{{OSCAIRN_VOICE_BUDGET_SELECTED, block % 7, 1, uint8_t(block % 11), uint8_t(block % 3)}, {5, 2, 0, 3, 0}};
            const size_t before = allocations; counting = true; forbidAllocation = true;
            if(block % 5 == 0) a.setVoiceLimits(h, block % 9, contexts, block % 3);
            const bool ok = a.process(h, in.data(), ni, out.data(), no, frames, reinterpret_cast<const oscairn_clap_event*>(sent.data()), uint32_t(sent.size()), &now);
            uint32_t count = 0; const auto* events = a.outputEvents(h, &count);
            double cost = 0; const oscairn_voice_budget_note_t* voiceNotes = nullptr; uint32_t voiceCount = 0;
            const auto voices = a.voiceReport(h, &cost, &voiceNotes, &voiceCount);
            const bool asleep = a.asleep(h), reset = a.wasReset(h), restful = a.restful(h);
            const auto reports = a.reports(h);
            forbidAllocation = false; counting = false;
            allocated = allocated || allocations != before;
            transport.advance(frames);
            const auto b = n + "block " + std::to_string(block);
            t.put(b + " ok", ok);
            for(uint32_t c = 0; c < no; ++c) t.item(b + " audio", output[c].data(), frames * sizeof(float));
            t.item(b + " events", events, count * sizeof(oscairn_clap_event));
            t.put(b + " voices", voices); t.put(b + " voice cost", cost); t.put(b + " wake after", wake.load());
            for(uint32_t i = 0; i < voiceCount; ++i) { const auto& v = voiceNotes[i]; t.put(b + " voice note", v.note_id); for(const int x : {v.port_index, v.channel, v.key}) t.put(b + " voice address", x); t.put(b + " voice count", v.voices); t.put(b + " loudness", v.loudness); }
            t.put(b + " asleep", asleep); t.put(b + " reset", reset); t.put(b + " restful", restful); t.put(b + " reports", reports);
            for(const auto id : ids) t.put(b + " value", a.value(h, id));
            if(block % 50 == 0)
            {
                a.service(h);
                oscairn_clap_event e; while(a.pop(h, &e)) t.put(b + " output", e);
                t.put(b + " drops", a.drops(h));
                t.put(b + " value rescans", a.valueRescans(h));
            }
        }
        control(t, a, h, n, ids, minimum, flags, ni, no, allocated);
        oscairn_clap_statistics s{};
        a.statistics(h, &s);
        for(const uint64_t c : {s.blocks, s.droppedEvents, s.processErrors, s.controlSilences, s.idleBlocks, s.restarts, s.lateBlocks, s.silentBlocks, s.busyBlocks,
                                s.missPanics, s.calledBlocks, s.restedBlocks}) t.put(n + "counter", c);
        t.put(n + "reset", a.reset(h));
        a.mirrorWake(h, nullptr);
        a.free(h);
    }

    // Everything one implementation makes of the plugins, in a process of its own; "allocated" is set when its blocks allocated.
    std::string run(const Api& a, int argc, char** argv, bool& allocated)
    {
        int fds[2]; if(pipe(fds) != 0) return {};
        const pid_t child = fork();
        if(child == 0)
        {
            close(fds[0]);
            Transcript t; bool any = false;
            for(int i = 1; i < argc; ++i)
            {
                t.text("library", std::filesystem::path(argv[i]).filename().string().c_str());
                for(const auto& plugin : kick::clap::inspect(argv[i]).plugins)
                {
                    t.text("plugin", (plugin.id + " " + plugin.version).c_str());
                    play(t, a, argv[i], plugin.id, any);
                    limits(t, a, argv[i], plugin.id);
                }
                auto* none = a.make(argv[i], "no.such.plugin", rate, frames, false, 0);
                t.put("missing loaded", none != nullptr); if(!none) failure(t, a, "missing");
            }
            auto* nowhere = a.make("/nonexistent/x.clap", "", rate, frames, false, 0);
            t.put("nowhere loaded", nowhere != nullptr); if(!nowhere) failure(t, a, "nowhere");
            t.out.insert(0, 1, any ? '1' : '0');
            for(size_t done = 0; done < t.out.size();) { const auto w = write(fds[1], t.out.data() + done, t.out.size() - done); if(w <= 0) _exit(3); done += size_t(w); }
            _exit(0);
        }
        close(fds[1]);
        std::string all; char buffer[65536];
        for(ssize_t r; (r = read(fds[0], buffer, sizeof buffer)) > 0;) all.append(buffer, size_t(r));
        close(fds[0]);
        int status = 0; waitpid(child, &status, 0);
        if(!WIFEXITED(status) || WEXITSTATUS(status) != 0 || all.empty()) { std::fprintf(stderr, "FAIL: a run ended with status %d\n", status); return {}; }
        allocated = all[0] == '1';
        return all.substr(1);
    }
    // The items of a transcript: label and bytes.
    std::vector<std::pair<std::string, std::string>> items(const std::string& s)
    {
        std::vector<std::pair<std::string, std::string>> r;
        for(size_t at = 0; at + 4 <= s.size();)
        {
            uint32_t l, n; std::memcpy(&l, s.data() + at, 4); at += 4; auto label = s.substr(at, l); at += l;
            std::memcpy(&n, s.data() + at, 4); at += 4; r.emplace_back(std::move(label), s.substr(at, n)); at += n;
        }
        return r;
    }
}

// The transcript with its machine's paths as placeholders: the scratch folder, and each plugin's folder.
std::string portable(const std::string& transcript, int argc, char** argv)
{
    auto replace = [&](std::string& s, const std::string& from, const std::string& to) {
        if(from.empty()) return;
        for(size_t at = s.find(from); at != std::string::npos; at = s.find(from, at + to.size())) s.replace(at, from.size(), to);
    };
    Transcript t;
    for(auto [label, value] : items(transcript))
    {
        for(auto* x : {&label, &value})
        {
            replace(*x, scratch, "$SCRATCH");
            for(int i = 1; i < argc; ++i) replace(*x, std::filesystem::path(argv[i]).parent_path().string(), "$PLUGINS");
        }
        t.item(label, value.data(), value.size());
    }
    return t.out;
}

// Recorded vectors (core/clap-host/tests/vectors): a text header, an empty line, then each item: its label (a count of the
// bytes it shares with the last label, then the rest), a kind and bytes. Up to 64 bytes are kept as they are, more as their
// FNV-1a digest and size; a plugin's audio other than the recorder's (whose audio is its hash of everything it was given)
// as its size, nonzero samples, sum of magnitudes and peak, compared within a tolerance: its DSP calls libm, whose last
// bits differ between systems.
namespace
{
    struct Entry { std::string label; char kind; std::string bytes; };
    constexpr char raw = 'r', digest = 'd', audio = 'a';
    template<class T> void append(std::string& s, const T& v) { s.append(reinterpret_cast<const char*>(&v), sizeof v); }
    template<class T> T at(const std::string& s, size_t offset) { T v{}; std::memcpy(&v, s.data() + offset, sizeof v); return v; }
    Entry entry(const std::string& label, const std::string& value)
    {
        const bool sound = label.size() > 6 && !label.compare(label.size() - 6, 6, " audio");
        if(value.size() <= 64 && !sound) return {label, raw, value};
        std::string b;
        append(b, uint32_t(value.size()));
        if(!sound || label.rfind("org.oscairn.test.recorder", 0) == 0)
        {
            uint64_t h = 0xcbf29ce484222325ull;
            for(const unsigned char c : value) h = (h ^ c) * 0x100000001b3ull;
            append(b, h);
            return {label, digest, b};
        }
        uint32_t nonzero = 0; double sum = 0, peak = 0;
        for(size_t i = 0; i + 4 <= value.size(); i += 4) { const double x = std::fabs(at<float>(value, i)); nonzero += x != 0; sum += x; peak = std::max(peak, x); }
        append(b, nonzero); append(b, float(sum)); append(b, float(peak));
        return {label, audio, b};
    }
    std::vector<Entry> entries(const std::string& transcript)
    {
        std::vector<Entry> r;
        for(const auto& [label, value] : items(transcript)) r.push_back(entry(label, value));
        return r;
    }
    // The same audio within a tolerance: a thousandth of its samples zero or not, sums and peaks within 1e-4 of each other.
    bool near(const Entry& a, const Entry& b)
    {
        if(a.kind != audio || b.kind != audio || at<uint32_t>(a.bytes, 0) != at<uint32_t>(b.bytes, 0)) return false;
        const auto close = [](double x, double y) { return std::fabs(x - y) <= 1e-4 * std::max(std::fabs(x), std::fabs(y)) + 1e-6; };
        const int64_t na = at<uint32_t>(a.bytes, 4), nb = at<uint32_t>(b.bytes, 4);
        return std::abs(na - nb) <= std::max<int64_t>(1, na / 1000) && close(at<float>(a.bytes, 8), at<float>(b.bytes, 8)) &&
               close(at<float>(a.bytes, 12), at<float>(b.bytes, 12));
    }
    void varint(std::string& s, size_t v) { for(; v >= 0x80; v >>= 7) s += char(v | 0x80); s += char(v); }
    size_t varint(const std::string& s, size_t& offset)
    {
        size_t v = 0;
        for(int shift = 0; offset < s.size(); shift += 7) { const auto c = uint8_t(s[offset++]); v |= size_t(c & 0x7f) << shift; if(!(c & 0x80)) break; }
        return v;
    }
    std::string vectors(const std::vector<Entry>& all)
    {
        std::string out, last;
        for(const auto& e : all)
        {
            size_t shared = 0;
            while(shared < last.size() && shared < e.label.size() && last[shared] == e.label[shared]) ++shared;
            varint(out, shared); varint(out, e.label.size() - shared); out += e.label.substr(shared);
            out += e.kind; varint(out, e.bytes.size()); out += e.bytes;
            last = e.label;
        }
        return out;
    }
    std::vector<Entry> entries(const std::string& file, size_t offset, std::string& problem)
    {
        std::vector<Entry> r; std::string last;
        while(offset < file.size())
        {
            const size_t shared = varint(file, offset), rest = varint(file, offset);
            if(shared > last.size() || offset + rest + 1 > file.size()) { problem = "the recording is cut short"; break; }
            Entry e{last.substr(0, shared) + file.substr(offset, rest), file[offset + rest], {}};
            offset += rest + 1;
            const size_t size = varint(file, offset);
            if(offset + size > file.size()) { problem = "the recording is cut short"; break; }
            e.bytes = file.substr(offset, size); offset += size;
            last = e.label; r.push_back(std::move(e));
        }
        return r;
    }
    std::string shown(const Entry* e)
    {
        if(!e) return "nothing";
        char text[160];
        if(e->kind == digest) std::snprintf(text, sizeof text, "%u bytes, digest %016llx", at<uint32_t>(e->bytes, 0), (unsigned long long)at<uint64_t>(e->bytes, 4));
        else if(e->kind == audio)
            std::snprintf(text, sizeof text, "%u bytes of audio, %u samples nonzero, sum %.7g, peak %.7g", at<uint32_t>(e->bytes, 0), at<uint32_t>(e->bytes, 4),
                          at<float>(e->bytes, 8), at<float>(e->bytes, 12));
        else
        {
            std::string hex = std::to_string(e->bytes.size()) + " bytes";
            for(size_t i = 0; i < e->bytes.size() && i < 64; ++i) { std::snprintf(text, sizeof text, "%s%02x", i ? " " : ": ", uint8_t(e->bytes[i])); hex += text; }
            return hex;
        }
        return text;
    }
    // Both lists item by item: the first ten items that differ, the first with the item before it (the same in both), and
    // where two items' bytes differ, the first byte that does.
    int compare(const std::vector<Entry>& expected, const std::vector<Entry>& got, const char* what, const char* other, size_t& approximate)
    {
        int failures = 0;
        for(size_t i = 0; i < std::max(expected.size(), got.size()) && failures < 10; ++i)
        {
            const Entry* a = i < expected.size() ? &expected[i] : nullptr;
            const Entry* b = i < got.size() ? &got[i] : nullptr;
            if(a && b && a->label == b->label && a->kind == b->kind)
            {
                if(a->bytes == b->bytes) continue;
                if(near(*a, *b)) { ++approximate; continue; }
            }
            std::fprintf(stderr, "FAIL: item %zu differs: %s \"%s\" (%s) against %s \"%s\" (%s)\n", i, what, a ? a->label.c_str() : "(none)", shown(a).c_str(), other,
                         b ? b->label.c_str() : "(none)", shown(b).c_str());
            if(a && b && a->kind == raw && b->kind == raw)
                for(size_t k = 0; k < std::min(a->bytes.size(), b->bytes.size()); ++k)
                    if(a->bytes[k] != b->bytes[k]) { std::fprintf(stderr, "  first at byte %zu\n", k); break; }
            if(!failures && i) std::fprintf(stderr, "  after item %zu \"%s\", the same in both\n", i - 1, expected[i - 1].label.c_str());
            ++failures;
        }
        return failures;
    }
}

int main(int argc, char** argv)
{
    std::string record, replay;
    while(argc > 2 && (!std::strcmp(argv[1], "--record") || !std::strcmp(argv[1], "--replay")))
    {
        (argv[1][2] == 'r' && argv[1][3] == 'e' && argv[1][4] == 'c' ? record : replay) = argv[2];
        argv += 2; argc -= 2;
    }
    if(argc < 2 || record.empty() == replay.empty()) { std::fprintf(stderr, "usage: oscairn-clap-host-rust-test --record FILE | --replay FILE PLUGIN.clap...\n"); return 2; }
    const std::string magic = "oscairn clap-host vectors 1\n", target = "target: " OSCAIRN_VECTORS_TARGET "\n";
    std::vector<Entry> expected;
    if(!replay.empty())
    {
        std::ifstream in(replay, std::ios::binary); const std::string file(std::istreambuf_iterator<char>(in), {});
        if(!in.is_open()) { std::printf("skipped: no vectors for %s at %s\n", OSCAIRN_VECTORS_TARGET, replay.c_str()); return 77; }
        const auto body = file.find("\n\n");
        if(file.rfind(magic, 0) != 0 || body == std::string::npos) { std::fprintf(stderr, "FAIL: no recording in %s\n", replay.c_str()); return 1; }
        if(file.find(target) > body) { std::fprintf(stderr, "FAIL: %s was recorded for another target than %s", replay.c_str(), target.c_str()); return 1; }
        std::string problem; expected = entries(file, body + 2, problem);
        if(!problem.empty()) { std::fprintf(stderr, "FAIL: %s: %s\n", replay.c_str(), problem.c_str()); return 1; }
    }
    bool allocated = false;
    scratch = (std::filesystem::temp_directory_path() / ("oscairn-clap-host-rust-" + std::to_string(getpid()))).string();
    std::filesystem::create_directories(scratch);
    const auto rust = portable(run(OSCAIRN_API(rust_), argc, argv, allocated), argc, argv);
    std::filesystem::remove_all(scratch);
    if(rust.empty()) return 1;
    const auto got = entries(rust);
    if(!record.empty())
    {
        std::string header = magic + "recorded from: the Rust CLAP host at " OSCAIRN_HOST_REVISION " (a deliberate change; the C++ reference's recording it replaces is in git)\n" + target;
        header += "scenarios: per plugin, load and description; 400 blocks (audio in, notes, MIDI, both note dialects translated, a choke, the beat, "
                  "a harmony, refused events, a gesture, a parameter set, an event sent, a panic, rests, voice limits); state saved and loaded with its refusals; "
                  "effect contexts, one refused by the activation observer; native sounds, presets, snapshots, ring-out, assets; limits (offline, a full queue, "
                  "too many events, refused values, a rate change, outputs activated, a missing window, no memory room); counters; a missing plugin and library\n";
        for(const auto& e : got) if(e.label == "library" || e.label == "plugin") header += e.label + ": " + e.bytes + "\n";
        header += "items: " + std::to_string(got.size()) + "\nreplay: oscairn-clap-host-rust-test --replay FILE LIBRARY... (ctest: clap-host-rust-replay); "
                  "record: cmake --build BUILD --target clap-host-vectors\n\n";
        std::ofstream out(record, std::ios::binary); out << header << vectors(got);
        if(!out) { std::fprintf(stderr, "FAIL: cannot write %s\n", record.c_str()); return 1; }
        std::printf("clap host: the Rust host's vectors recorded, %zu items\n", got.size());
        return 0;
    }
    size_t approximate = 0;
    int failures = compare(expected, got, "recorded", "Rust", approximate);
    if(allocated) { std::fprintf(stderr, "FAIL: the Rust host's blocks allocated\n"); ++failures; }
    if(failures) return 1;
    std::printf("clap host: Rust the same as the recording over %zu items (%zu audio within the tolerance), the Rust blocks allocating nothing\n",
                expected.size(), approximate);
}
