/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* What the Rust CLAP host (core/clap-host/rust) reaches in kick_clap, which stays C++ behind these calls: the host's own
 * plugin windows (Window.h).
 * C, so bindgen reads it (core/clap-host/rust/src/bindings.rs, with clap_host.h). */
#pragma once
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The host's own window holding a plugin's embedded view (detail::Window): null where this platform has none. */
typedef struct kick_clap_window kick_clap_window;
const char* kick_clap_window_api(void);
void kick_clap_start_windows(void);
kick_clap_window* kick_clap_window_create(const char* title, uint32_t width, uint32_t height, bool resizable, void* context,
                                          void (*closing)(void* context), void (*resized)(void* context, uint32_t* width, uint32_t* height));
void* kick_clap_window_view(kick_clap_window* window);
void kick_clap_window_title(kick_clap_window* window, const char* title);
void kick_clap_window_size(kick_clap_window* window, uint32_t width, uint32_t height);
void kick_clap_window_resizable(kick_clap_window* window, bool resizable);
void kick_clap_window_show(kick_clap_window* window, bool show);
void kick_clap_window_free(kick_clap_window* window);

#ifdef __cplusplus
}
#endif
