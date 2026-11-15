// SPDX-License-Identifier: AGPL-3.0-or-later
// kick_clap/bridge.h: what stays C++ for the Rust host, each as Host.cpp does it. (The hardware and the activation
// observer are at the end of Host.cpp, beside the statics they read; the windows in WindowBridge.cpp.)
#include <kick_clap/bridge.h>
#include <kick_clap/Discovery.h>
#include <cstring>

namespace
{
    // The table's library_root, all a preset index asks of an AssetResolver.
    struct Roots final : kick::clap::AssetResolver
    {
        const oscairn_clap_assets* table;
        explicit Roots(const oscairn_clap_assets* t) : table(t) {}
        std::string resolve(const std::string&, const std::string&, std::string&) override { throw std::runtime_error("assets are not resolved while indexing presets"); }
        std::string libraryRoot(const std::string& library, const std::string& minVersion) const override
        {
            char path[4096];
            return table->library_root(table->context, library.c_str(), minVersion.c_str(), path, sizeof path) ? std::string(path) : std::string();
        }
    };
}

extern "C"
{
const char* kick_clap_presets(const clap_plugin_entry_t* entry, const char* id, const oscairn_clap_assets* assets,
                              void (*each)(void*, const char*, uint32_t, const char*, const char*), void* context)
{
    static thread_local std::string failure;
    try
    {
        const std::string plugin = id;
        Roots roots(assets);
        auto index = kick::clap::indexPresets(entry, {plugin}, 10000, assets ? &roots : nullptr); // providers find asset libraries through library_root
        for(const auto& p : index.presets[plugin]) each(context, p.name.c_str(), p.kind, p.location.c_str(), p.loadKey.c_str());
        return nullptr;
    }
    catch(const std::exception& e) { failure = e.what(); }
    catch(...) { failure = "Unknown error"; }
    return failure.c_str();
}

}
