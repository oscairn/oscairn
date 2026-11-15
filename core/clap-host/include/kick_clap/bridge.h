/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* What the Rust CLAP host (core/clap-host/rust) reaches in kick_clap, which stays C++ behind these calls: the process's
 * hardware and activation observer (set through kick::clap::Host, as the C++ host reads them), preset discovery
 * (Discovery.cpp, indexPresets) and the host's own plugin windows (Window.h). Each does what Host.cpp does there.
 * C, so bindgen reads it (core/clap-host/rust/src/bindings.rs, with clap_host.h). */
#pragma once
#include <clap/entry.h>
#include <oscairn-clap/assets.h>
#include <oscairn-clap/hardware.h>
#include <oscairn-clap/memory.h>
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Where plugins resolve sound assets (kick::clap::AssetResolver, RFC 0009), as oscairn.assets answers a plugin
 * (oscairn-clap/assets.h) once the host checked the thread and the arguments: a failure's reason written into message. */
typedef struct
{
    void* context;
    bool (*resolve)(void* context, oscairn_asset_ref_t* ref, char* path, uint32_t path_capacity, char* message, uint32_t message_capacity);
    bool (*library_root)(void* context, const char* library, const char* min_version, char* path, uint32_t path_capacity);
    bool (*create)(void* context, const char* file, oscairn_asset_ref_t* ref, char* message, uint32_t message_capacity);
    bool (*resolve_sample)(void* context, const oscairn_asset_ref_t* ref, char* path, uint32_t path_capacity, oscairn_sample_location_t* location,
                           char* message, uint32_t message_capacity);
} oscairn_clap_assets;

/* oscairn.hardware through kick::clap::Host::hardware() (none: no accelerators); owner is the plugin's host. */
uint32_t kick_clap_hardware_count(void);
bool kick_clap_hardware_get(uint32_t index, oscairn_hardware_accelerator_t* accelerator);
bool kick_clap_hardware_supports(uint32_t index, const char* operation);
int32_t kick_clap_hardware_reserve(const void* owner, uint32_t index, const oscairn_hardware_request_t* request, oscairn_hardware_lease_t* lease);
void kick_clap_hardware_release(const void* owner, uint32_t lease);
void kick_clap_hardware_release_all(const void* owner);
int32_t kick_clap_hardware_submit(const void* owner, uint32_t epoch, uint32_t lease, const oscairn_hardware_job_t* job);
int32_t kick_clap_hardware_collect(const void* owner, uint32_t epoch, uint32_t lease, oscairn_hardware_result_t* result);

/* kick::clap::Host::observeActivations' observer, if one is set, told of an activation: null to go on, else why it is
 * refused (valid until this thread's next call). */
const char* kick_clap_activating(const oscairn_memory_t* declared);

/* indexPresets for one plugin of a loaded binary (10 s budget), each preset given in turn; assets may be null. Null,
 * else what it threw (valid until this thread's next call). */
const char* kick_clap_presets(const clap_plugin_entry_t* entry, const char* id, const oscairn_clap_assets* assets,
                       void (*each)(void* context, const char* name, uint32_t kind, const char* location, const char* load_key), void* context);

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
