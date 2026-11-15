// SPDX-License-Identifier: AGPL-3.0-or-later
// kick_clap/bridge.h's windows: detail::Window (Window.mm on macOS, none elsewhere) for the Rust host. An object file of
// its own, so a test can link a fake window in its place (these calls defined in the test) and drive the host's editor
// path where there is no display.
#include <kick_clap/bridge.h>
#include "Window.h"

struct kick_clap_window
{
    std::unique_ptr<kick::clap::detail::Window> window;
};

extern "C"
{
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
