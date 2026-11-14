/* SPDX-License-Identifier: MIT */
#pragma once
#include <clap/clap.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Sound assets as Performance dependencies (docs/rfcs/0009-sound-assets-and-dependencies.md).
 * A reference names a library, a path relative to it and the file's SHA-256 (64 lowercase hex
 * digits, or empty when not yet known). Plugin state stores references, never local paths. */
typedef struct oscairn_asset_ref {
    char library[64];
    char path[1024];
    char sha256[65];
} oscairn_asset_ref_t;

/* Host extension. Main thread. resolve writes a local, verified file path for ref. When
 * ref->sha256 is empty it is filled in with the file's hash; when given, the file must have
 * that content (identical bytes in another library also satisfy it). On failure returns false
 * and writes a NUL-terminated reason into message.
 *
 * Draft 2 adds library_root: the folder holding an installed asset library's files at their
 * paths, read-only, when a version at least min_version (NULL or "": any) is installed. It writes
 * the NUL-terminated path into path and returns true; false when the library is not installed,
 * is older, or the path does not fit. `owner` is what the table came from: the clap_host_t, or
 * the clap_preset_discovery_indexer_t, whose get_extension answers OSCAIRN_EXT_ASSETS too, so a
 * preset provider can declare the library's folder as a location in its init. From an indexer,
 * resolve is NULL. A host answers draft 1 with the same table (resolve only).
 *
 * Draft 3 adds create: store a file the plugin produced (a recording, an edited wavetable, a
 * converted waveform), given as a finished file at `file`, an absolute path. It fills ref with the
 * file's library, path and SHA-256 and returns true: a file already in a registered or installed
 * library is referenced where it is, and must stay there; any other is copied, and the plugin may
 * delete it once create returns. The copy goes into the user library (`user`, `<first 16 hex
 * digits of the SHA-256>/<name>`). The plugin then keeps ref in its state and lists it in oscairn.asset-dependencies. On failure it returns
 * false with a NUL-terminated reason in message. From an indexer, create is NULL. A host answers
 * drafts 1 and 2 with the same table.
 *
 * Draft 4: items kept by content and named by their id rather than files. A reference whose path
 * is "sample:<id>" or "waveform:<id>" (64 lowercase hex digits; library empty; sha256 empty or
 * that id, which is its digest) names a sample in an Oscairn Sample Pack or a waveform
 * (github.com/oscairn/samplepack). resolve answers a waveform reference with the path of its
 * .owf file, once every sample it names is installed, and a sample reference with its pack's
 * path. resolve_sample answers a sample reference with where its record is: the pack's path in
 * path, read-only, and in location the record's offset, length and preload span, which with
 * the id make the sp_index_entry the samplepack library's sp_record_* functions take. Both stay
 * valid while the plugin holds the sound prepared: packs are appended to, never rewritten. An
 * in-memory lookup, cheap enough for every sample of a sound. On failure it returns false with
 * a NUL-terminated reason in message. From an indexer, resolve_sample is NULL. A host answers
 * drafts 1 to 3 with the same table. */
static const char OSCAIRN_EXT_ASSETS[] = "oscairn.assets/draft-4";
static const char OSCAIRN_EXT_ASSETS_DRAFT_3[] = "oscairn.assets/draft-3";
static const char OSCAIRN_EXT_ASSETS_DRAFT_2[] = "oscairn.assets/draft-2";
static const char OSCAIRN_EXT_ASSETS_DRAFT_1[] = "oscairn.assets/draft-1";
typedef struct oscairn_sample_location {
    uint64_t offset;  /* the record's offset in the pack, bytes */
    uint64_t length;  /* the record's length, bytes */
    uint64_t preload; /* bytes from the record's start to the end of its preload window: read these first */
} oscairn_sample_location_t;
typedef struct oscairn_host_assets {
    bool (CLAP_ABI *resolve)(const clap_host_t*, oscairn_asset_ref_t* ref, char* path, uint32_t path_capacity,
                             char* message, uint32_t message_capacity);
    bool (CLAP_ABI *library_root)(const void* owner, const char* library_id, const char* min_version, char* path, uint32_t path_capacity); /* draft 2 */
    bool (CLAP_ABI *create)(const clap_host_t*, const char* file, oscairn_asset_ref_t* ref, char* message, uint32_t message_capacity); /* draft 3 */
    bool (CLAP_ABI *resolve_sample)(const clap_host_t*, const oscairn_asset_ref_t* ref, char* path, uint32_t path_capacity,
                                    oscairn_sample_location_t* location, char* message, uint32_t message_capacity); /* draft 4 */
} oscairn_host_assets_t;

/* Plugin extension. Main thread. Lists every asset the plugin's current state uses, so a host
 * can record a Performance's dependencies and check them before loading it. Draft 2 adds
 * optional: true for an asset the plugin can do without (it keeps a fallback, as the arpeggiator
 * embeds its phrases), so a host loads the plugin with that asset missing, its resolve failing,
 * and reports a notice instead of refusing. A host also accepts draft 1 (count and get only:
 * every asset required). */
static const char OSCAIRN_EXT_ASSET_DEPENDENCIES[] = "oscairn.asset-dependencies/draft-2";
static const char OSCAIRN_EXT_ASSET_DEPENDENCIES_DRAFT_1[] = "oscairn.asset-dependencies/draft-1";
typedef struct oscairn_plugin_asset_dependencies {
    uint32_t (CLAP_ABI *count)(const clap_plugin_t*);
    bool (CLAP_ABI *get)(const clap_plugin_t*, uint32_t index, oscairn_asset_ref_t* ref);
    bool (CLAP_ABI *optional)(const clap_plugin_t*, uint32_t index); /* draft 2 only; null: every asset required */
} oscairn_plugin_asset_dependencies_t;

#ifdef __cplusplus
}
#endif
