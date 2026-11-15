// SPDX-License-Identifier: AGPL-3.0-or-later
// kick_clap/bridge.h: what stays C++ for the Rust host, each as Host.cpp does it. (The hardware and the activation
// observer are at the end of Host.cpp, beside the statics they read.)
#include <kick_clap/bridge.h>
#include <kick_clap/Discovery.h>
#include "Window.h"
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

struct kick_clap_window
{
    std::unique_ptr<kick::clap::detail::Window> window;
};

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

const char* kick_clap_window_api(void) { return kick::clap::detail::Window::api(); }
void kick_clap_start_windows(void) { kick::clap::detail::startWindows(); }
kick_clap_window* kick_clap_window_create(const char* title, uint32_t width, uint32_t height, bool resizable, void* context,
                                          void (*closing)(void*), void (*resized)(void*, uint32_t*, uint32_t*))
{
    auto window = kick::clap::detail::Window::create(title, width, height, resizable);
    if(!window) return nullptr;
    window->closing = [=] { closing(context); };
    window->resized = [=](uint32_t& w, uint32_t& h) { resized(context, &w, &h); };
    return new kick_clap_window{std::move(window)};
}
void* kick_clap_window_view(kick_clap_window* w) { return w->window->view(); }
void kick_clap_window_title(kick_clap_window* w, const char* title) { w->window->setTitle(title); }
void kick_clap_window_size(kick_clap_window* w, uint32_t width, uint32_t height) { w->window->setSize(width, height); }
void kick_clap_window_resizable(kick_clap_window* w, bool resizable) { w->window->setResizable(resizable); }
void kick_clap_window_show(kick_clap_window* w, bool show) { if(show) w->window->show(); else w->window->hide(); }
void kick_clap_window_free(kick_clap_window* w) { delete w; }
}
