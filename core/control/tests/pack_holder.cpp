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

    bool estimate(const clap_plugin_t*, double, uint32_t, oscairn_memory_t* out) { *out = {1u << 16, 0, heldBytes()}; return true; }
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
        plugin->destroy = [](const clap_plugin_t* p) { held.erase(p); destroy(p); };
        plugin->activate = [](const clap_plugin_t* p, double rate, uint32_t lo, uint32_t hi) { const bool ok = activate(p, rate, lo, hi); if(ok) hold(p); return ok; };
        plugin->get_extension = [](const clap_plugin_t* p, const char* id) -> const void* { return !std::strcmp(id, OSCAIRN_EXT_MEMORY) ? &memory : extension(p, id); };
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
