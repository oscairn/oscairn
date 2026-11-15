// SPDX-License-Identifier: AGPL-3.0-or-later
//! A CLAP binary, opened once per process however many instances use it (Internal.h's Library and openLibrary).
use clap_sys::entry::clap_plugin_entry;
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::version::clap_version_is_compatible;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}
const RTLD_NOW: c_int = 2;
#[cfg(target_os = "macos")]
const RTLD_LOCAL: c_int = 4;
#[cfg(not(target_os = "macos"))]
const RTLD_LOCAL: c_int = 0;

pub struct Library {
    handle: *mut c_void,
    pub entry: *const clap_plugin_entry,
    pub path: String,
    pub cpath: CString,
}
// SAFETY: the entry's calls are the plugin's to make thread-safe (CLAP); the handle is only closed on drop.
unsafe impl Send for Library {}
// SAFETY: as Send.
unsafe impl Sync for Library {}

/// A macOS bundle's binary: Contents/MacOS/<its stem>, else the first file there.
fn executable(path: &Path) -> PathBuf {
    if cfg!(target_os = "macos") && path.is_dir() {
        let directory = path.join("Contents/MacOS");
        let conventional = directory.join(path.file_stem().unwrap_or_default());
        if conventional.is_file() {
            return conventional;
        }
        if let Ok(entries) = std::fs::read_dir(&directory) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    return entry.path();
                }
            }
        }
    }
    path.to_path_buf()
}

impl Library {
    fn open(source: &str) -> Result<Library, String> {
        let cpath = CString::new(source).map_err(|e| e.to_string())?;
        let binary = CString::new(executable(Path::new(source)).to_string_lossy().into_owned()).map_err(|e| e.to_string())?;
        // SAFETY: NUL-terminated strings; the library's initializers run, as loading a plugin means.
        let handle = unsafe { dlopen(binary.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        // SAFETY: a handle dlopen gave.
        let entry = if handle.is_null() { std::ptr::null() } else { unsafe { dlsym(handle, c"clap_entry".as_ptr()) }.cast::<clap_plugin_entry>() };
        let mut library = Library { handle, entry: std::ptr::null(), path: source.to_owned(), cpath };
        // SAFETY: clap_entry, when found, is the plugin's entry structure.
        let usable = !entry.is_null()
            && unsafe {
                clap_version_is_compatible((*entry).clap_version) && (*entry).init.is_some() && (*entry).deinit.is_some() && (*entry).get_factory.is_some()
            };
        if !usable {
            return Err(format!("Cannot load a compatible native CLAP binary: {source}"));
        }
        // SAFETY: checked above.
        if !unsafe { ((*entry).init.unwrap())(library.cpath.as_ptr()) } {
            return Err(format!("CLAP entry initialization failed: {source}"));
        }
        library.entry = entry;
        Ok(library)
    }
    pub fn factory(&self) -> Option<&clap_plugin_factory> {
        // SAFETY: an initialized entry; the factory it gives lives as long as the library.
        let f = unsafe { ((*self.entry).get_factory.unwrap())(CLAP_PLUGIN_FACTORY_ID.as_ptr()) }.cast::<clap_plugin_factory>();
        // SAFETY: as above.
        let f = unsafe { f.as_ref()? };
        (f.get_plugin_count.is_some() && f.get_plugin_descriptor.is_some() && f.create_plugin.is_some()).then_some(f)
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        if !self.entry.is_null() {
            // SAFETY: initialized; deinit once.
            unsafe { ((*self.entry).deinit.unwrap())() };
        }
        if !self.handle.is_null() {
            // SAFETY: dlopen's handle, closed once.
            unsafe { dlclose(self.handle) };
        }
    }
}

/// entry.init and deinit are per binary, not per instance: every host holds the library it uses, and the next
/// instance of a binary in use shares it.
pub fn open(source: &str) -> Result<Arc<Library>, String> {
    static LIBRARIES: Mutex<Option<HashMap<String, Weak<Library>>>> = Mutex::new(None);
    let mut libraries = LIBRARIES.lock().unwrap_or_else(|e| e.into_inner());
    let path = std::fs::canonicalize(source).map_err(|e| canonical_error(source, &e))?.to_string_lossy().into_owned();
    let map = libraries.get_or_insert_with(HashMap::new);
    if let Some(existing) = map.get(&path).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    let library = Arc::new(Library::open(&path)?);
    map.insert(path, Arc::downgrade(&library));
    Ok(library)
}

/// What std::filesystem::canonical's filesystem_error says: libc++'s words on macOS, libstdc++'s elsewhere (Linux).
fn canonical_error(path: &str, e: &std::io::Error) -> String {
    let what = e.raw_os_error().map(|n| {
        // SAFETY: strerror's string, copied at once.
        unsafe { std::ffi::CStr::from_ptr(strerror(n)) }.to_string_lossy().into_owned()
    });
    let what = what.unwrap_or_else(|| e.to_string());
    if cfg!(target_os = "macos") {
        format!("filesystem error: in canonical: {what} [\"{path}\"] [\"\"]")
    } else {
        format!("filesystem error: cannot make canonical path: {what} [{path}]")
        // shortcut: libstdc++'s wording, unchecked on Linux yet
    }
}
extern "C" {
    fn strerror(n: c_int) -> *const c_char;
}
