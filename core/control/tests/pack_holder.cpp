// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Test fixture: a sampler in the way of the Oscairn Sampler. The Basic Synth that, when activated,
// reads the sample pack at OSCAIRN_HOLDER_PACK (read with the file's own reads, never mapped, as the
// Sampler reads its packs) and holds OSCAIRN_HOLDER_BYTES of decoded audio made from it in its own
// private memory, touched so it is resident, and declares those bytes as asset_bytes through
// oscairn.memory before it allocates them (RFC 0025). Each instance holds its own copy, as the
// Sampler's instances in different worker processes do.
#define clap_entry basic_synth_entry
#include "../../../plugins/basic-synth/basic_synth.cpp"
#undef clap_entry
#include <oscairn-clap/memory.h>
#include <fstream>
#include <iterator>
#include <map>
#include <vector>

namespace
{
    uint64_t heldBytes() { const char* n = std::getenv("OSCAIRN_HOLDER_BYTES"); return n ? std::strtoull(n, nullptr, 10) : 0; }
    std::map<const clap_plugin_t*, std::vector<char>> held; // main thread

    // Decoded audio made from the pack's bytes, repeated to the size asked for: every page written.
    void hold(const clap_plugin_t* p)
    {
        const char* path = std::getenv("OSCAIRN_HOLDER_PACK");
        std::vector<char> pack;
        if(path) { std::ifstream in(path, std::ios::binary); pack.assign(std::istreambuf_iterator<char>(in), {}); }
        if(pack.empty()) pack.push_back(1);
        auto& audio = held[p];
        audio.resize(heldBytes());
        for(size_t i = 0; i < audio.size(); ++i) audio[i] = pack[i % pack.size()] ^ char(i >> 12);
    }

    // A plugin that under-declares its sound contexts, as the Sampler did: each holds OSCAIRN_HOLDER_CONTEXT_BYTES of its own,
    // touched, where it declares OSCAIRN_HOLDER_CONTEXT_DECLARED (both 0 unless set).
    uint64_t bytesOf(const char* name) { const char* n = std::getenv(name); return n ? std::strtoull(n, nullptr, 10) : 0; }
    std::map<const clap_plugin_t*, std::vector<char>> contextsHeld; // main thread
    void fitContexts(const clap_plugin_t* p)
    {
        uint32_t n = 0;
        oscairn_sound_context_info_t info{};
        for(uint32_t slot = 0, capacity = soundContexts.capacity(p); slot < capacity; ++slot) n += soundContexts.get(p, slot, &info);
        auto& bytes = contextsHeld[p];
        const auto size = n * bytesOf("OSCAIRN_HOLDER_CONTEXT_BYTES");
        if(size == bytes.size()) return;
        std::vector<char>(size).swap(bytes);
        for(size_t i = 0; i < bytes.size(); i += 4096) bytes[i] = char(1 + (i >> 12));
    }
    const oscairn_plugin_sound_contexts_t holderContexts{
        soundContexts.capacity, soundContexts.get,
        [](const clap_plugin_t* p, const char* name, const oscairn_sound_parameter_t* values, uint32_t count, const char* patch, const oscairn_asset_ref_t* base, char* message, uint32_t capacity) {
            const auto id = soundContexts.prepare(p, name, values, count, patch, base, message, capacity); fitContexts(p); return id; },
        soundContexts.select,
        [](const clap_plugin_t* p, uint32_t id) { const bool ok = soundContexts.discard(p, id); fitContexts(p); return ok; },
        soundContexts.state, soundContexts.prepare_preset};
    const oscairn_plugin_sound_snapshots_t holderSnapshots{
        [](const clap_plugin_t* p, const clap_istream_t* in, uint32_t* selected, char* message, uint32_t capacity) {
            const bool ok = soundSnapshots.prepare(p, in, selected, message, capacity); fitContexts(p); return ok; },
        soundSnapshots.ring_out, soundSnapshots.choke_retiring, soundSnapshots.save_sound, soundSnapshots.sound_values};

    bool estimate(const clap_plugin_t*, double, uint32_t, oscairn_memory_t* out) { *out = {1u << 16, bytesOf("OSCAIRN_HOLDER_CONTEXT_DECLARED"), heldBytes()}; return true; }
    const oscairn_plugin_memory_t memory{estimate, [](const clap_plugin_t* p, oscairn_memory_t* out) { return estimate(p, 0, 0, out); }};

    clap_plugin_descriptor_t holderDescriptor = descriptor;

    const clap_plugin_t* createHolder()
    {
        auto* plugin = const_cast<clap_plugin_t*>(create());
        plugin->desc = &holderDescriptor;
        held[plugin];
        static void (*destroy)(const clap_plugin_t*) = plugin->destroy;
        static bool (*activate)(const clap_plugin_t*, double, uint32_t, uint32_t) = plugin->activate;
        static const void* (*extension)(const clap_plugin_t*, const char*) = plugin->get_extension;
        plugin->destroy = [](const clap_plugin_t* p) { held.erase(p); contextsHeld.erase(p); destroy(p); };
        plugin->activate = [](const clap_plugin_t* p, double rate, uint32_t lo, uint32_t hi) { const bool ok = activate(p, rate, lo, hi); if(ok) { hold(p); fitContexts(p); } return ok; };
        plugin->get_extension = [](const clap_plugin_t* p, const char* id) -> const void* {
            if(!std::strcmp(id, OSCAIRN_EXT_MEMORY)) return &memory;
            if(!std::strcmp(id, OSCAIRN_EXT_SOUND_CONTEXTS)) return &holderContexts;
            if(!std::strcmp(id, OSCAIRN_EXT_SOUND_SNAPSHOTS)) return &holderSnapshots;
            return extension(p, id);
        };
        return plugin;
    }
    const clap_plugin_factory_t holderFactory = {
        [](const clap_plugin_factory_t*) -> uint32_t { return 1; },
        [](const clap_plugin_factory_t*, uint32_t index) -> const clap_plugin_descriptor_t* { holderDescriptor.id = "org.oscairn.test.pack-holder"; return index == 0 ? &holderDescriptor : nullptr; },
        [](const clap_plugin_factory_t*, const clap_host_t*, const char* id) -> const clap_plugin_t* { return std::strcmp(id, "org.oscairn.test.pack-holder") == 0 ? createHolder() : nullptr; },
    };
}

extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry = {
    CLAP_VERSION_INIT,
    [](const char*) { holderDescriptor.id = "org.oscairn.test.pack-holder"; return true; },
    []() {},
    [](const char* id) -> const void* { return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) == 0 ? &holderFactory : nullptr; },
};
