// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <kick_clap/Host.h>
#include "../../plugin-host/include/oscairn/bounded_queue.h"
#include <atomic>
#include <cstring>
#include <filesystem>
#include <map>
#include <mutex>
#include <stdexcept>
#ifdef _WIN32
#include <windows.h>
#else
#include <dlfcn.h>
#endif
// AddressSanitizer keeps stale poisoning for an unloaded library's globals, which turns the next
// library mapped at that address into false global-buffer-overflow reports, so ASan builds never unload.
#if defined(__SANITIZE_ADDRESS__)
#define OSCAIRN_ASAN 1
#elif defined(__has_feature)
#if __has_feature(address_sanitizer)
#define OSCAIRN_ASAN 1
#endif
#endif
#ifndef OSCAIRN_ASAN
#define OSCAIRN_ASAN 0
#endif

namespace kick::clap::detail {
inline std::string safeString(const char* text) { return text ? text : ""; }
inline std::filesystem::path executablePath(const std::filesystem::path& path) {
#ifdef __APPLE__
    if (std::filesystem::is_directory(path)) {
        const auto directory = path / "Contents/MacOS";
        const auto conventional = directory / path.stem();
        if (std::filesystem::is_regular_file(conventional)) return conventional;
        for (const auto& entry : std::filesystem::directory_iterator(directory))
            if (entry.is_regular_file()) return entry.path();
    }
#endif
    return path;
}
struct Library {
    void* handle = nullptr;
    const clap_plugin_entry_t* entry = nullptr;
    std::string path;
    explicit Library(const std::string& source) : path(source) {
        const auto binary = executablePath(source).string();
#ifdef _WIN32
        handle = LoadLibraryW(std::filesystem::path(binary).wstring().c_str());
        if (handle) entry = reinterpret_cast<const clap_plugin_entry_t*>(GetProcAddress(static_cast<HMODULE>(handle), "clap_entry"));
#else
        handle = dlopen(binary.c_str(), RTLD_NOW | RTLD_LOCAL);
        if (handle) entry = static_cast<const clap_plugin_entry_t*>(dlsym(handle, "clap_entry"));
#endif
        if (!handle || !entry || !clap_version_is_compatible(entry->clap_version) ||
            !entry->init || !entry->deinit || !entry->get_factory) {
            entry = nullptr;
            close();
            throw std::runtime_error("Cannot load a compatible native CLAP binary: " + source);
        }
        if (!entry->init(path.c_str())) {
            entry = nullptr;
            close();
            throw std::runtime_error("CLAP entry initialization failed: " + source);
        }
    }
    ~Library() { close(); }
    void close() {
        if (entry) entry->deinit();
        entry = nullptr;
        if (handle) {
#ifdef _WIN32
            FreeLibrary(static_cast<HMODULE>(handle));
#elif !OSCAIRN_ASAN
            dlclose(handle);
#endif
        }
        handle = nullptr;
    }
    const clap_plugin_factory_t* factory() const {
        const auto* f = static_cast<const clap_plugin_factory_t*>(entry->get_factory(CLAP_PLUGIN_FACTORY_ID));
        return f && f->get_plugin_count && f->get_plugin_descriptor && f->create_plugin ? f : nullptr;
    }
};
inline std::shared_ptr<Library> openLibrary(const std::string& source) {
    // entry.init/deinit are per binary, not per instance. Keep a strong reference
    // in every host and coordinate init when a second program uses that binary.
    static std::mutex mutex;
    static std::map<std::string, std::weak_ptr<Library>> libraries;
    std::lock_guard<std::mutex> guard(mutex);
    auto path = std::filesystem::canonical(source).string();
    if (auto existing = libraries[path].lock()) return existing;
    auto library = std::make_shared<Library>(path);
    libraries[path] = library;
    return library;
}
inline Descriptor describe(const std::string& path, const clap_plugin_descriptor_t& d) {
    Descriptor result{path, safeString(d.id), safeString(d.name), safeString(d.vendor),
                      safeString(d.version), safeString(d.description), {}};
    if (d.features)
        for (size_t i = 0; i < 128 && d.features[i]; ++i) result.features.emplace_back(d.features[i]);
    return result;
}

// The bounded queue the C++ host's command and output queues are.
template<class T, size_t Capacity> using Queue = oscairn::BoundedQueue<T, Capacity>;
// oscairn.assets' library_root, for the host and the preset indexer: the plugin's strings bounded,
// the folder written whole with its NUL or not at all.
inline bool writeLibraryRoot(const AssetResolver& assets, const char* library, const char* minVersion, char* path, uint32_t capacity) {
    try {
        const auto root = assets.libraryRoot(std::string(library, strnlen(library, 256)), minVersion ? std::string(minVersion, strnlen(minVersion, 128)) : std::string());
        if (root.empty() || root.size() + 1 > capacity) return false;
        std::memcpy(path, root.c_str(), root.size() + 1);
        return true;
    } catch (...) { return false; }
}
} // namespace kick::clap::detail
