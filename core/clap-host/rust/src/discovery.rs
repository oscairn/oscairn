// SPDX-License-Identifier: AGPL-3.0-or-later
//! Plugin discovery: the CLAP binaries under some folders, what each offers (inspected here, or by the scan helper in a
//! process of its own with a timeout, crash isolation and a memory limit), and the presets a library's preset discovery
//! factory lists (RFC 0005). The helper answers in the scan wire format (`encode`), which is also how a scan reaches
//! the C++ callers (`rust_clap_scan` and the rest, at the end).
use crate::bindings::{oscairn_clap_assets, oscairn_clap_preset, oscairn_clap_text};
use crate::host::Table;
use crate::host::CLAP_VERSION;
use crate::isolated::{run_isolated, watch_parent_pipe, Outcome};
use crate::library;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::factory::preset_discovery::*;
use clap_sys::timestamp::clap_timestamp;
use clap_sys::universal_plugin_id::clap_universal_plugin_id;
use clap_sys::version::clap_version_is_compatible;
use oscairn_clap_sys::ids::{OSCAIRN_EXT_ASSETS, OSCAIRN_EXT_ASSETS_DRAFT_2, OSCAIRN_EXT_ASSETS_DRAFT_3};
use oscairn_clap_sys::oscairn_host_assets_t;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::{Duration, Instant};

/// A preset a library lists for a plugin: inside the plugin (kind 1) or in a file (kind 0, location its path).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    pub kind: u32,
    pub location: String,
    pub load_key: String,
}
/// A plugin a binary offers, as a scan lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plugin {
    pub path: String,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub description: String,
    pub features: Vec<String>,
    pub preset_discovery: bool, // its library offers CLAP preset discovery
    pub presets: Vec<Preset>,   // what a scan indexed
}
/// A binary, or part of one, that could not be listed, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub path: String,
    pub message: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scan {
    pub plugins: Vec<Plugin>,
    pub failures: Vec<Problem>,
}
/// The presets a library lists for each plugin, and what stopped or failed on the way.
#[derive(Debug, Default)]
pub struct PresetIndex {
    pub presets: HashMap<String, Vec<Preset>>,
    pub problems: Vec<String>,
}

pub const MAX_PRESETS: usize = 4096;
pub const MAX_PRESET_NAME: usize = 255;
pub const MAX_PRESET_LOCATION: usize = 4096;
const MAX_WIRE: usize = 4 * 1024 * 1024;
const MAGIC: &[u8] = b"KICKCLAP1\n";

fn string(p: *const c_char) -> String {
    // SAFETY: a plugin's NUL-terminated string, or null.
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|e| extension.strip_prefix('.') == Some(&*e.to_string_lossy()))
}

/// Where CLAP plugins are installed: CLAP_PATH's folders, then the user's and the system's.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> =
        std::env::var_os("CLAP_PATH").map(|v| std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".clap"));
        if cfg!(target_os = "macos") {
            paths.push(home.join("Library/Audio/Plug-Ins/CLAP"));
        }
    }
    if cfg!(target_os = "macos") {
        paths.push("/Library/Audio/Plug-Ins/CLAP".into());
    } else {
        paths.extend(["/usr/lib/clap", "/usr/local/lib/clap", "/opt/kick/plugins/clap"].map(PathBuf::from));
    }
    paths
}

/// The files (or bundles) with `extension` (".clap") under `roots`, at most eight folders down and 8,192 of them,
/// each once, canonical, sorted.
pub fn find_binaries(roots: &[PathBuf], extension: &str) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        if has_extension(root, extension) {
            found.extend(std::fs::canonicalize(root));
            continue;
        }
        let mut walk = Walk::new(root);
        while let Some((entry, depth)) = walk.next() {
            let path = entry.path();
            if depth > 8 {
                continue;
            }
            if has_extension(&path, extension) {
                found.extend(std::fs::canonicalize(&path)); // a matching folder (a bundle) is not entered
                if found.len() >= 8192 {
                    break;
                }
            } else if entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk.enter(&path, depth + 1);
            }
        }
    }
    found.into_iter().collect()
}

/// A folder's entries depth first, each folder's in the order it lists them; entering a folder is the caller's choice.
/// Folders that cannot be read list nothing.
struct Walk(Vec<(std::fs::ReadDir, usize)>);
impl Walk {
    fn new(root: &Path) -> Walk {
        Walk(std::fs::read_dir(root).map(|r| vec![(r, 0)]).unwrap_or_default())
    }
    fn enter(&mut self, folder: &Path, depth: usize) {
        self.0.extend(std::fs::read_dir(folder).map(|r| (r, depth)));
    }
}
impl Iterator for Walk {
    type Item = (std::fs::DirEntry, usize);
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (entries, depth) = self.0.last_mut()?;
            match entries.next() {
                None => drop(self.0.pop()),
                Some(Ok(entry)) => return Some((entry, *depth)),
                Some(Err(_)) => {}
            }
        }
    }
}

fn preset_factory(entry: &clap_plugin_entry) -> Option<&clap_preset_discovery_factory> {
    let get = entry.get_factory?;
    [CLAP_PRESET_DISCOVERY_FACTORY_ID, CLAP_PRESET_DISCOVERY_FACTORY_ID_COMPAT].into_iter().find_map(|id| {
        // SAFETY: an initialized entry; a factory lives as long as its library.
        unsafe { get(id.as_ptr()).cast::<clap_preset_discovery_factory>().as_ref() }
    })
}

/// What a binary offers, loading it in this process (call it in the scan helper, or with a trusted fixture). With
/// presets, each plugin's presets are indexed too (`index_presets`), its providers finding asset libraries in `assets`.
pub fn inspect(binary: &Path, presets: bool, assets: Option<&oscairn_clap_assets>) -> Scan {
    let path = binary.to_string_lossy().into_owned();
    inspect_library(&path, presets, assets).unwrap_or_else(|message| Scan { plugins: Vec::new(), failures: vec![Problem { path, message }] })
}
fn inspect_library(path: &str, presets: bool, assets: Option<&oscairn_clap_assets>) -> Result<Scan, String> {
    let library = library::open(path)?;
    // SAFETY: an initialized entry, alive as long as the library.
    let entry = unsafe { &*library.entry };
    let discovery = preset_factory(entry).is_some();
    let factory = library.factory().ok_or("No plugin factory")?;
    let (count, describe) = (factory.get_plugin_count.ok_or("No plugin factory")?, factory.get_plugin_descriptor.ok_or("No plugin factory")?);
    // SAFETY: the factory's calls, checked present.
    let count = unsafe { count(factory) };
    if count == 0 || count > 4096 {
        return Err("Invalid factory descriptor count".into());
    }
    let mut scan = Scan::default();
    for i in 0..count {
        // SAFETY: as above; a descriptor lives as long as the library.
        let d = unsafe { describe(factory, i).as_ref() };
        let named = |p: *const c_char| !p.is_null() && !string(p).is_empty();
        let Some(d) = d.filter(|d| named(d.id) && named(d.name) && clap_version_is_compatible(d.clap_version)) else {
            return Err("Invalid or incompatible plugin descriptor".into());
        };
        let id = string(d.id);
        if scan.plugins.iter().any(|p| p.id == id) {
            return Err("Duplicate plugin ID in factory".into());
        }
        let mut features = Vec::new();
        // SAFETY: the plugin's NULL-terminated feature list, at most 128 read.
        while !d.features.is_null() && features.len() < 128 && !unsafe { *d.features.add(features.len()) }.is_null() {
            // SAFETY: as above.
            features.push(string(unsafe { *d.features.add(features.len()) }));
        }
        scan.plugins.push(Plugin {
            path: library.path.clone(),
            id,
            name: string(d.name),
            vendor: string(d.vendor),
            version: string(d.version),
            description: string(d.description),
            features,
            preset_discovery: discovery,
            presets: Vec::new(),
        });
    }
    if presets && discovery {
        let ids: Vec<String> = scan.plugins.iter().map(|p| p.id.clone()).collect();
        let mut index = index_presets(entry, &ids, Duration::from_secs(10), assets);
        for p in &mut scan.plugins {
            p.presets = index.presets.remove(&p.id).unwrap_or_default();
        }
        scan.failures.extend(index.problems.into_iter().map(|message| Problem { path: path.to_owned(), message }));
    }
    Ok(scan)
}

/// What a provider declares while it initializes, and the presets it then reports.
struct Indexing {
    plugins: Vec<String>,
    assets: Option<oscairn_clap_assets>,
    state: RefCell<IndexState>,
}
#[derive(Default)]
struct IndexState {
    index: PresetIndex,
    extensions: Vec<String>,
    locations: Vec<(u32, String)>,
    kind: u32,
    location: String,
    count: usize,
    stopped: bool,
    open: bool,
    named: Vec<String>,
    current: Preset,
}
impl IndexState {
    /// The preset begun, kept under each plugin it names (one naming none, under all).
    fn finish(&mut self, plugins: &[String]) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        let mut kept = false;
        for id in plugins.iter().filter(|id| self.named.is_empty() || self.named.contains(id)) {
            self.index.presets.entry(id.clone()).or_default().push(self.current.clone());
            kept = true;
        }
        self.count += usize::from(kept);
    }
}
/// The indexing behind a plugin's indexer or receiver (their data, the Indexing below), its state lent to f; `none`
/// when it is in use already (the plugin called back into a callback).
fn indexing<R>(data: *mut c_void, none: R, f: impl FnOnce(&Indexing, &mut IndexState) -> R) -> R {
    // SAFETY: index_presets' Indexing, alive while the provider runs.
    let d = unsafe { &*data.cast::<Indexing>() };
    match d.state.try_borrow_mut() {
        Ok(mut s) => f(d, &mut s),
        Err(_) => none,
    }
}
unsafe extern "C" fn declare_filetype(i: *const clap_preset_discovery_indexer, t: *const clap_preset_discovery_filetype) -> bool {
    // SAFETY: the indexer we made; the plugin's filetype, or null.
    let (data, t) = unsafe { ((*i).indexer_data, t.as_ref()) };
    indexing(data, false, |_, s| {
        let Some(t) = t.filter(|_| s.extensions.len() < 64) else { return false };
        s.extensions.push(string(t.file_extension));
        true
    })
}
unsafe extern "C" fn declare_location(i: *const clap_preset_discovery_indexer, l: *const clap_preset_discovery_location) -> bool {
    // SAFETY: as declare_filetype.
    let (data, l) = unsafe { ((*i).indexer_data, l.as_ref()) };
    indexing(data, false, |_, s| {
        let Some(l) = l else { return false };
        let file = l.kind == CLAP_PRESET_DISCOVERY_LOCATION_FILE;
        let location = string(l.location);
        if l.kind > CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN || file == l.location.is_null() || location.len() > MAX_PRESET_LOCATION || s.locations.len() >= 64 {
            return false;
        }
        s.locations.push((l.kind, location));
        true
    })
}
unsafe extern "C" fn declare_soundpack(_: *const clap_preset_discovery_indexer, _: *const clap_preset_discovery_soundpack) -> bool {
    true
}
unsafe extern "C" fn indexer_extension(i: *const clap_preset_discovery_indexer, id: *const c_char) -> *const c_void {
    // SAFETY: the indexer we made, its Indexing; the plugin's ID, or null.
    let (d, id) = unsafe { (&*(*i).indexer_data.cast::<Indexing>(), (!id.is_null()).then(|| CStr::from_ptr(id))) };
    // oscairn.assets for library_root only (resolve and create need a plugin instance's host); not draft 1, whose resolve this has not.
    let asked = id.is_some_and(|id| [OSCAIRN_EXT_ASSETS, OSCAIRN_EXT_ASSETS_DRAFT_3, OSCAIRN_EXT_ASSETS_DRAFT_2].contains(&id));
    if asked && d.assets.is_some() {
        std::ptr::from_ref(&LIBRARIES.0).cast()
    } else {
        std::ptr::null()
    }
}
static LIBRARIES: Table<oscairn_host_assets_t> =
    Table(oscairn_host_assets_t { resolve: None, library_root: Some(library_root), create: None, resolve_sample: None });
unsafe extern "C" fn library_root(owner: *const c_void, library: *const c_char, min: *const c_char, path: *mut c_char, capacity: u32) -> bool {
    let i = owner.cast::<clap_preset_discovery_indexer>();
    if i.is_null() || library.is_null() || path.is_null() || capacity == 0 {
        return false;
    }
    // SAFETY: the owner is the indexer the provider was given (oscairn.assets draft 2), its Indexing alive.
    let d = unsafe { &*(*i).indexer_data.cast::<Indexing>() };
    // SAFETY: the resolver's table answers within the plugin's buffer.
    d.assets.is_some_and(|a| a.library_root.is_some_and(|f| unsafe { f(a.context, library, min, path, capacity) }))
}
unsafe extern "C" fn begin_preset(r: *const clap_preset_discovery_metadata_receiver, name: *const c_char, key: *const c_char) -> bool {
    // SAFETY: the receiver we made.
    let data = unsafe { (*r).receiver_data };
    indexing(data, false, |d, s| {
        s.finish(&d.plugins);
        if s.stopped {
            return false;
        }
        if s.count >= MAX_PRESETS {
            s.stopped = true;
            s.index.problems.push(format!("Presets: stopped at {MAX_PRESETS}"));
            return false;
        }
        // A container names its presets and gives each a load key; a file that is one preset may leave both out.
        let mut title = string(name);
        if title.is_empty() && s.kind == CLAP_PRESET_DISCOVERY_LOCATION_FILE {
            title = Path::new(&s.location).file_stem().map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
        }
        let load_key = string(key);
        s.open = !title.is_empty()
            && title.len() <= MAX_PRESET_NAME
            && load_key.len() <= MAX_PRESET_LOCATION
            && (s.kind == CLAP_PRESET_DISCOVERY_LOCATION_FILE || !load_key.is_empty());
        s.named.clear();
        s.current = Preset { name: title, kind: s.kind, location: s.location.clone(), load_key };
        true
    })
}
unsafe extern "C" fn add_plugin_id(r: *const clap_preset_discovery_metadata_receiver, id: *const clap_universal_plugin_id) {
    // SAFETY: the receiver we made; the plugin's ID, or null.
    let (data, id) = unsafe { ((*r).receiver_data, id.as_ref()) };
    indexing(data, (), |_, s| {
        let Some(id) = id.filter(|_| s.named.len() < 256) else { return };
        // Another format's plugin is not ours, yet the preset is named: it is not listed for all.
        s.named.push(if string(id.abi) == "clap" { string(id.id) } else { String::new() });
    });
}
unsafe extern "C" fn on_error(_: *const clap_preset_discovery_metadata_receiver, _: i32, _: *const c_char) {}
unsafe extern "C" fn ignore_text(_: *const clap_preset_discovery_metadata_receiver, _: *const c_char) {}
unsafe extern "C" fn ignore_flags(_: *const clap_preset_discovery_metadata_receiver, _: u32) {}
unsafe extern "C" fn ignore_times(_: *const clap_preset_discovery_metadata_receiver, _: clap_timestamp, _: clap_timestamp) {}
unsafe extern "C" fn ignore_pair(_: *const clap_preset_discovery_metadata_receiver, _: *const c_char, _: *const c_char) {}

/// CLAP preset discovery: the presets the library lists for each of `plugins`, inside the plugin and in files of the
/// declared types in the declared places, each under the plugins it names (one naming none under all). Runs plugin
/// code: off the audio thread. Stops at MAX_PRESETS, and starts no read after `budget`; either, and a provider that
/// does not initialize, is a problem. With `assets`, the indexer answers oscairn.assets' library_root (draft 2), so a
/// provider may declare an installed asset library's folder as a location.
pub fn index_presets(entry: &clap_plugin_entry, plugins: &[String], budget: Duration, assets: Option<&oscairn_clap_assets>) -> PresetIndex {
    let Some(factory) = preset_factory(entry) else { return PresetIndex::default() };
    let (Some(count), Some(get_descriptor), Some(create)) = (factory.count, factory.get_descriptor, factory.create) else { return PresetIndex::default() };
    let deadline = Instant::now() + budget;
    let data = Indexing { plugins: plugins.to_vec(), assets: assets.copied(), state: RefCell::default() };
    let data_ptr = std::ptr::from_ref(&data).cast_mut().cast::<c_void>();
    let indexer = clap_preset_discovery_indexer {
        clap_version: CLAP_VERSION,
        name: c"Oscairn".as_ptr(),
        vendor: c"Oscairn".as_ptr(),
        url: c"https://github.com/oscairn/oscairn".as_ptr(),
        version: c"1".as_ptr(),
        indexer_data: data_ptr,
        declare_filetype: Some(declare_filetype),
        declare_location: Some(declare_location),
        declare_soundpack: Some(declare_soundpack),
        get_extension: Some(indexer_extension),
    };
    let receiver = clap_preset_discovery_metadata_receiver {
        receiver_data: data_ptr,
        on_error: Some(on_error),
        begin_preset: Some(begin_preset),
        add_plugin_id: Some(add_plugin_id),
        set_soundpack_id: Some(ignore_text),
        set_flags: Some(ignore_flags),
        add_creator: Some(ignore_text),
        set_description: Some(ignore_text),
        set_timestamps: Some(ignore_times),
        add_feature: Some(ignore_text),
        add_extra_info: Some(ignore_pair),
    };
    let state = || data.state.borrow_mut(); // never while the provider runs: its callbacks borrow it
    let read = |provider: &clap_preset_discovery_provider, kind: u32, location: &str| {
        let Some(get) = provider.get_metadata else { return };
        {
            let mut s = state();
            if s.stopped {
                return;
            }
            if Instant::now() >= deadline {
                s.stopped = true;
                s.index.problems.push(format!("Presets: stopped after {} s", budget.as_secs()));
                return;
            }
            if location.len() > MAX_PRESET_LOCATION {
                return;
            }
            (s.kind, s.location) = (kind, location.to_owned());
        }
        let file = CString::new(location).ok();
        if kind != CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN && file.is_none() {
            return;
        }
        let at = if kind == CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN { std::ptr::null() } else { file.as_ref().map_or(std::ptr::null(), |f| f.as_ptr()) };
        // SAFETY: the provider's call, given our receiver and the location, alive for the call.
        unsafe { get(provider, kind, at, &receiver) };
        state().finish(plugins);
    };
    // SAFETY: the factory's calls, checked present.
    let providers = unsafe { count(factory) }.min(64);
    for p in 0..providers {
        if state().stopped {
            break;
        }
        // SAFETY: as above; a descriptor lives as long as the factory.
        let Some(d) = (unsafe { get_descriptor(factory, p).as_ref() }).filter(|d| !d.id.is_null()) else { continue };
        let id = string(d.id);
        // SAFETY: as above, with our indexer, which outlives the provider.
        let Some(provider) = (unsafe { create(factory, &indexer, d.id).as_ref() }) else {
            state().index.problems.push(format!("Presets: provider {id} was not created"));
            continue;
        };
        {
            let mut s = state();
            s.locations.clear();
            s.extensions.clear();
        }
        // SAFETY: the provider's own call.
        let initialized = provider.init.is_some_and(|init| unsafe { init(provider) });
        match provider.get_metadata.filter(|_| initialized) {
            None => state().index.problems.push(format!("Presets: provider {id} did not initialize")),
            Some(_) => {
                let (locations, extensions) = {
                    let s = state();
                    (s.locations.clone(), s.extensions.clone())
                };
                let matches = |file: &Path| {
                    let extension = file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                    extensions.is_empty() || extensions.iter().any(|e| e.is_empty() || *e == extension)
                };
                for (kind, location) in locations {
                    if kind == CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN {
                        read(provider, kind, "");
                    } else if Path::new(&location).is_file() {
                        read(provider, kind, &location);
                    } else {
                        // A folder: its preset files, at most 10,000 of them, 16 folders deep.
                        let (mut files, mut walk) = (0, Walk::new(Path::new(&location)));
                        while let Some((entry, depth)) = walk.next() {
                            if files >= 10000 || state().stopped {
                                break;
                            }
                            let path = entry.path();
                            if path.is_file() && matches(&path) {
                                files += 1;
                                read(provider, kind, &path.to_string_lossy());
                            } else if depth < 16 && entry.file_type().is_ok_and(|t| t.is_dir()) {
                                walk.enter(&path, depth + 1);
                            }
                        }
                    }
                }
            }
        }
        state().open = false;
        if let Some(destroy) = provider.destroy {
            // SAFETY: the provider's own call, its last.
            unsafe { destroy(provider) };
        }
    }
    data.state.into_inner().index
}

/// The scan wire format: KICKCLAP1, then counts and strings as 32-bit little-endian lengths and bytes.
pub fn encode(scan: &Scan) -> Result<Vec<u8>, String> {
    let mut out = MAGIC.to_vec();
    let number = |out: &mut Vec<u8>, n: usize| out.extend_from_slice(&(n as u32).to_le_bytes());
    let field = |out: &mut Vec<u8>, s: &str| {
        number(out, s.len());
        out.extend_from_slice(s.as_bytes());
    };
    number(&mut out, scan.plugins.len());
    for p in &scan.plugins {
        for s in [&p.path, &p.id, &p.name, &p.vendor, &p.version, &p.description] {
            field(&mut out, s);
        }
        number(&mut out, p.features.len());
        for f in &p.features {
            field(&mut out, f);
        }
        number(&mut out, usize::from(p.preset_discovery));
        number(&mut out, p.presets.len());
        for preset in &p.presets {
            field(&mut out, &preset.name);
            number(&mut out, preset.kind as usize);
            field(&mut out, &preset.location);
            field(&mut out, &preset.load_key);
        }
    }
    number(&mut out, scan.failures.len());
    for f in &scan.failures {
        field(&mut out, &f.path);
        field(&mut out, &f.message);
    }
    if out.len() > MAX_WIRE {
        return Err("Scanner result exceeds size limit".into());
    }
    Ok(out)
}

/// A scan from the wire format, after the last KICKCLAP1 (a helper may print before it); None for anything malformed.
pub fn decode(source: &[u8]) -> Option<Scan> {
    if source.len() > MAX_WIRE {
        return None;
    }
    let start = source.windows(MAGIC.len()).rposition(|w| w == MAGIC)? + MAGIC.len();
    let mut reader = Reader { at: &source[start..] };
    let mut scan = Scan::default();
    let count = reader.number()?;
    if count > 4096 {
        return None;
    }
    for _ in 0..count {
        let mut p = Plugin {
            path: reader.field()?,
            id: reader.field()?,
            name: reader.field()?,
            vendor: reader.field()?,
            version: reader.field()?,
            description: reader.field()?,
            ..Plugin::default()
        };
        let features = reader.number()?;
        if features > 128 {
            return None;
        }
        for _ in 0..features {
            p.features.push(reader.field()?);
        }
        let (discovery, presets) = (reader.number()?, reader.number()?);
        if discovery > 1 || presets as usize > MAX_PRESETS {
            return None;
        }
        p.preset_discovery = discovery == 1;
        for _ in 0..presets {
            let preset = Preset { name: reader.field()?, kind: reader.number()?, location: reader.field()?, load_key: reader.field()? };
            if preset.kind > CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN
                || preset.name.is_empty()
                || preset.name.len() > MAX_PRESET_NAME
                || preset.location.len() > MAX_PRESET_LOCATION
                || preset.load_key.len() > MAX_PRESET_LOCATION
            {
                return None;
            }
            p.presets.push(preset);
        }
        if p.path.is_empty() || p.id.is_empty() || p.name.is_empty() {
            return None;
        }
        scan.plugins.push(p);
    }
    let count = reader.number()?;
    if count > 8192 {
        return None;
    }
    for _ in 0..count {
        scan.failures.push(Problem { path: reader.field()?, message: reader.field()? });
    }
    reader.at.is_empty().then_some(scan)
}
struct Reader<'a> {
    at: &'a [u8],
}
impl Reader<'_> {
    fn number(&mut self) -> Option<u32> {
        let (n, rest) = self.at.split_first_chunk::<4>()?;
        self.at = rest;
        Some(u32::from_le_bytes(*n))
    }
    fn field(&mut self) -> Option<String> {
        let n = self.number()? as usize;
        if n > MAX_WIRE || n > self.at.len() {
            return None;
        }
        let (s, rest) = self.at.split_at(n);
        self.at = rest;
        Some(String::from_utf8_lossy(s).into_owned())
    }
}

/// Runs the scan helper on one binary (flag: --worker, or --presets to index presets too): its answer, or why there is
/// none, which starts "Cannot " only when the helper never started (scanPluginPresets asks again after those).
fn run_helper(helper: &Path, flag: &str, binary: &str, timeout: Duration, canceled: Option<&AtomicBool>) -> Result<Scan, String> {
    let arguments = [helper.to_string_lossy().into_owned(), flag.to_owned(), binary.to_owned()];
    let run = run_isolated(&arguments, timeout, MAX_WIRE, canceled, 0, None);
    let detail = if run.stderr_tail.is_empty() { String::new() } else { format!(": {}", run.stderr_tail) };
    Err(match run.outcome {
        Outcome::NoPipe => "Cannot create scanner pipe".into(),
        Outcome::NoLaunch => "Cannot launch kick-clap-scan helper".into(),
        Outcome::Canceled => format!("Scan canceled{detail}"),
        Outcome::TimedOut => format!("Plugin scan timed out{detail}"),
        Outcome::Oversized => format!("Plugin scanner output exceeded size limit{detail}"),
        Outcome::OverMemory | Outcome::Failed => format!("Plugin crashed or failed during scan{detail}"),
        Outcome::Exited => match decode(&run.output) {
            Some(scan) => return Ok(scan),
            None => format!("Invalid scanner response{detail}"),
        },
    })
}

/// A fresh preset index of one binary by the scan helper's --presets mode (its timeout, crash isolation and stderr), so
/// a slow or hung provider runs in another process.
pub fn index_presets_with_helper(helper: &Path, binary: &Path, timeout: Duration) -> Result<Scan, String> {
    run_helper(helper, "--presets", &binary.to_string_lossy(), timeout.max(Duration::from_millis(1)), None)
}

/// Every binary with `extension` under `roots`, each inspected by a fresh helper process (timeout and crash isolation);
/// one whose library offers preset discovery has its presets indexed by a second helper with its own timeout, so a
/// failed index leaves its plugins listed.
pub fn scan(roots: &[PathBuf], helper: &Path, timeout: Duration, canceled: Option<&AtomicBool>, extension: &str, preset_timeout: Duration) -> Scan {
    let mut result = Scan::default();
    let limit = Duration::from_secs(60);
    if timeout.is_zero() || timeout > limit || preset_timeout.is_zero() || preset_timeout > limit {
        result.failures.push(Problem { path: helper.to_string_lossy().into_owned(), message: "Invalid scanner timeout".into() });
        return result;
    }
    for path in find_binaries(roots, extension) {
        if canceled.is_some_and(|c| c.load(Relaxed)) {
            break;
        }
        let binary = path.to_string_lossy().into_owned();
        let mut single = match run_helper(helper, "--worker", &binary, timeout, canceled) {
            Ok(single) => single,
            Err(message) => {
                result.failures.push(Problem { path: binary, message });
                continue;
            }
        };
        // Presets in a second process, so a slow or failing index leaves the plugins listed.
        if single.plugins.iter().any(|p| p.preset_discovery) {
            match run_helper(helper, "--presets", &binary, preset_timeout, canceled) {
                Err(e) => single.failures.push(Problem { path: binary.clone(), message: format!("Presets not indexed: {e}") }),
                Ok(indexed) => {
                    for p in &mut single.plugins {
                        if let Some(q) = indexed.plugins.iter().find(|q| q.id == p.id) {
                            p.presets.clone_from(&q.presets);
                        }
                    }
                    single.failures.extend(indexed.failures.into_iter().filter(|f| f.message.starts_with("Presets: ")));
                }
            }
        }
        result.plugins.append(&mut single.plugins);
        result.failures.append(&mut single.failures);
    }
    result
}

/// For the C++ callers that remain (plugin-host's Discovery.h over these, the scan helpers, the library's extractors),
/// declared in plugin-host/src/clap_host.h: a scan arrives as its wire bytes, given once to `out`; each string as bytes
/// and a length.
mod exports {
    use super::*;
    fn strings(list: *const *const c_char, count: u32) -> impl Iterator<Item = String> {
        // SAFETY: the caller's `count` NUL-terminated strings.
        (0..count as usize).map(move |i| string(unsafe { *list.add(i) }))
    }
    /// # Safety
    /// `out` null or a callback taking `context`.
    unsafe fn give(out: oscairn_clap_text, context: *mut c_void, bytes: &[u8]) {
        if let Some(out) = out {
            // SAFETY: the caller's callback, given bytes alive for the call.
            unsafe { out(context, bytes.as_ptr().cast(), bytes.len()) };
        }
    }
    /// A scan's wire bytes; a scan too large for them as its one failure.
    fn wire(scan: &Scan, path: &str) -> Vec<u8> {
        encode(scan).unwrap_or_else(|message| encode(&Scan { plugins: Vec::new(), failures: vec![Problem { path: path.into(), message }] }).unwrap_or_default())
    }
    /// # Safety
    /// `binary` NUL-terminated; `assets` null or a table; `out` as `give`.
    #[no_mangle]
    pub unsafe extern "C" fn rust_clap_inspect(
        binary: *const c_char,
        presets: bool,
        assets: *const oscairn_clap_assets,
        out: oscairn_clap_text,
        context: *mut c_void,
    ) {
        let path = string(binary);
        // SAFETY: the caller's table, or null.
        let scan = inspect(Path::new(&path), presets, unsafe { assets.as_ref() });
        // SAFETY: the caller's.
        unsafe { give(out, context, &wire(&scan, &path)) };
    }
    /// # Safety
    /// `roots` holds `count` NUL-terminated paths; `helper` and `extension` NUL-terminated; `canceled` null or a flag
    /// alive for the call; `out` as `give`.
    #[no_mangle]
    pub unsafe extern "C" fn rust_clap_scan(
        roots: *const *const c_char,
        count: u32,
        helper: *const c_char,
        timeout_ms: u32,
        canceled: *const AtomicBool,
        extension: *const c_char,
        preset_timeout_ms: u32,
        out: oscairn_clap_text,
        context: *mut c_void,
    ) {
        let roots: Vec<PathBuf> = strings(roots, count).map(PathBuf::from).collect();
        let (helper, ms) = (string(helper), |n: u32| Duration::from_millis(u64::from(n)));
        // SAFETY: the caller's flag, or null.
        let result = scan(&roots, Path::new(&helper), ms(timeout_ms), unsafe { canceled.as_ref() }, &string(extension), ms(preset_timeout_ms));
        // SAFETY: the caller's.
        unsafe { give(out, context, &wire(&result, &helper)) };
    }
    /// # Safety
    /// Paths NUL-terminated; `out` as `give`: the wire bytes when it returns true, the reason when false.
    #[no_mangle]
    pub unsafe extern "C" fn rust_clap_index_presets_with_helper(
        helper: *const c_char,
        binary: *const c_char,
        timeout_ms: u32,
        out: oscairn_clap_text,
        context: *mut c_void,
    ) -> bool {
        let indexed = index_presets_with_helper(Path::new(&string(helper)), Path::new(&string(binary)), Duration::from_millis(u64::from(timeout_ms)));
        let (ok, bytes) = match indexed.and_then(|s| encode(&s)) {
            Ok(wire) => (true, wire),
            Err(e) => (false, e.into_bytes()),
        };
        // SAFETY: the caller's.
        unsafe { give(out, context, &bytes) };
        ok
    }
    /// # Safety
    /// `entry` null or an initialized CLAP entry; `ids` holds `count` NUL-terminated IDs; `assets` null or a table; the
    /// callbacks null or taking `context`, given values alive for the call.
    #[no_mangle]
    pub unsafe extern "C" fn rust_clap_index_presets(
        entry: *const clap_plugin_entry,
        ids: *const *const c_char,
        count: u32,
        budget_ms: u32,
        assets: *const oscairn_clap_assets,
        preset: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const oscairn_clap_preset)>,
        problem: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
        context: *mut c_void,
    ) {
        // SAFETY: the caller's entry, or null.
        let Some(entry) = (unsafe { entry.as_ref() }) else { return };
        let ids: Vec<String> = strings(ids, count).collect();
        // SAFETY: the caller's table, or null.
        let index = index_presets(entry, &ids, Duration::from_millis(u64::from(budget_ms)), unsafe { assets.as_ref() });
        let c = |s: &str| CString::new(s).unwrap_or_default();
        for id in &ids {
            let cid = c(id);
            for p in index.presets.get(id).into_iter().flatten() {
                let (name, location, key) = (c(&p.name), c(&p.location), c(&p.load_key));
                let given = oscairn_clap_preset { name: name.as_ptr(), kind: p.kind, location: location.as_ptr(), load_key: key.as_ptr() };
                // SAFETY: the caller's callback.
                preset.inspect(|f| unsafe { f(context, cid.as_ptr(), &given) });
            }
        }
        for text in &index.problems {
            let text = c(text);
            // SAFETY: the caller's callback.
            problem.inspect(|f| unsafe { f(context, text.as_ptr()) });
        }
    }
    /// # Safety
    /// `arguments` holds `count` NUL-terminated strings; `canceled` null or a flag alive for the call; `memory_of` null
    /// or callable here; `output` and `errors` as `give`.
    #[no_mangle]
    pub unsafe extern "C" fn rust_clap_run_isolated(
        arguments: *const *const c_char,
        count: u32,
        timeout_ms: u32,
        max_output: usize,
        canceled: *const AtomicBool,
        memory_limit: u64,
        memory_of: Option<unsafe extern "C" fn(std::ffi::c_long) -> u64>,
        output: oscairn_clap_text,
        errors: oscairn_clap_text,
        context: *mut c_void,
    ) -> c_int {
        let arguments: Vec<String> = strings(arguments, count).collect();
        // SAFETY: the caller's function.
        let memory = memory_of.map(|f| move |pid: u32| unsafe { f(std::ffi::c_long::from(pid as i32)) });
        let memory = memory.as_ref().map(|f| f as &dyn Fn(u32) -> u64);
        // SAFETY: the caller's flag, or null.
        let run = run_isolated(&arguments, Duration::from_millis(u64::from(timeout_ms)), max_output, unsafe { canceled.as_ref() }, memory_limit, memory);
        // SAFETY: the caller's.
        unsafe {
            give(output, context, &run.output);
            give(errors, context, run.stderr_tail.as_bytes());
        }
        run.outcome as c_int
    }
    /// # Safety
    /// `each` null or a callback taking `context`, given each path.
    #[no_mangle]
    pub unsafe extern "C" fn rust_clap_search_paths(each: oscairn_clap_text, context: *mut c_void) {
        for p in default_search_paths() {
            // SAFETY: the caller's.
            unsafe { give(each, context, p.as_os_str().as_encoded_bytes()) };
        }
    }
    /// # Safety
    /// As rust_clap_scan's roots and extension; `each` as rust_clap_search_paths'.
    #[no_mangle]
    pub unsafe extern "C" fn rust_clap_find_binaries(
        roots: *const *const c_char,
        count: u32,
        extension: *const c_char,
        each: oscairn_clap_text,
        context: *mut c_void,
    ) {
        let roots: Vec<PathBuf> = strings(roots, count).map(PathBuf::from).collect();
        for p in find_binaries(&roots, &string(extension)) {
            // SAFETY: the caller's.
            unsafe { give(each, context, p.as_os_str().as_encoded_bytes()) };
        }
    }
    #[no_mangle]
    pub extern "C" fn rust_clap_watch_parent_pipe() {
        watch_parent_pipe();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Scan {
        let preset = Preset { name: "Quiet".into(), kind: CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN, location: String::new(), load_key: "quiet".into() };
        let plugin = Plugin {
            path: "/p.clap".into(),
            id: "a.b".into(),
            name: "A".into(),
            features: vec!["instrument".into()],
            preset_discovery: true,
            presets: vec![preset],
            ..Plugin::default()
        };
        Scan { plugins: vec![plugin], failures: vec![Problem { path: "/q.clap".into(), message: "why".into() }] }
    }
    #[test]
    fn wire_round_trip_after_noise() {
        let mut wire = b"a plugin's own output\n".to_vec();
        wire.extend(encode(&sample()).unwrap());
        assert_eq!(decode(&wire), Some(sample()));
    }
    #[test]
    fn wire_refuses_what_is_malformed() {
        let wire = encode(&sample()).unwrap();
        assert_eq!(decode(b"broken"), None);
        assert_eq!(decode(&wire[..20]), None);
        assert_eq!(decode(&[wire.as_slice(), b"x"].concat()), None); // bytes after the scan
        let mut nameless = sample();
        nameless.plugins[0].presets[0].name.clear();
        assert_eq!(decode(&encode(&nameless).unwrap()), None);
    }
    #[test]
    fn binaries_found_once_sorted_bundles_not_entered() {
        let root = std::env::temp_dir().join(format!("oscairn-find-{}", std::process::id()));
        std::fs::create_dir_all(root.join("x/a.clap/Contents")).unwrap(); // a bundle, with a binary inside it
        for file in ["b.clap", "x/a.clap/Contents/inner.clap", "x/notes.txt"] {
            std::fs::write(root.join(file), b"").unwrap();
        }
        let canonical = std::fs::canonicalize(&root).unwrap();
        let found = find_binaries(&[root.clone(), root.join("b.clap")], ".clap");
        assert_eq!(found, vec![canonical.join("b.clap"), canonical.join("x/a.clap")]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_missing_binary_is_a_failure() {
        let scan = inspect(Path::new("/nonexistent/x.clap"), true, None);
        assert!(scan.plugins.is_empty() && scan.failures.len() == 1 && scan.failures[0].path == "/nonexistent/x.clap");
    }
}
