// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CLAP host in Rust: core/plugin-host/src/clap_host.h's rust_clap_* functions, kick::clap::Host's behavior
//! (core/clap-host/src/Host.cpp, the reference) call for call. Being written in stages (agent-notes/rust-host.md);
//! the functions defined so far are the ones core/engine/tests/clap_host_rust.cpp compares with cpp_clap_*.
// The host is reached through a raw pointer and its fields are borrowed one at a time, briefly (host.rs says why).
#![allow(dangerous_implicit_autorefs)]

mod bindings;
pub mod event;
mod host;
mod library;
mod more;
mod queue;
mod state;

use bindings::*;
pub use host::Host;
use host::*;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr::null_mut;
use std::sync::atomic::Ordering::*;

thread_local!(static FAILURE: RefCell<(c_int, CString)> = RefCell::new((0, CString::default())));
fn failed<T>(message: String, result: T) -> T {
    fail_with(host::Failure::runtime(message), result)
}
fn fail_with<T>(f: host::Failure, result: T) -> T {
    let message = CString::new(f.message).unwrap_or_default();
    FAILURE.with(|slot| *slot.borrow_mut() = (f.kind as c_int, message));
    result
}
/// A C string argument as Rust's (NUL-free by C's own terms).
unsafe fn arg<'a>(p: *const c_char) -> &'a str {
    // SAFETY: the caller's NUL-terminated string.
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or_default()
}

/// # Safety
/// kind is null or writable.
#[no_mangle]
pub unsafe extern "C" fn rust_clap_error(kind: *mut c_int) -> *const c_char {
    FAILURE.with(|f| {
        let f = f.borrow();
        if !kind.is_null() {
            // SAFETY: the caller's.
            unsafe { *kind = f.0 };
        }
        f.1.as_ptr() // lives in this thread's slot until its next failure, as clap_host.h says
    })
}

/// kick::clap::inspect's part that loading uses: the binary's plugins, as its scan reads them.
fn inspect(path: &str) -> Result<Vec<CString>, String> {
    let library = library::open(path)?;
    // SAFETY: an initialized entry; the factories it gives live as long as the library.
    unsafe {
        let get = (*library.entry).get_factory.unwrap();
        let _discovery = !get(clap_sys::factory::preset_discovery::CLAP_PRESET_DISCOVERY_FACTORY_ID.as_ptr()).is_null()
            || !get(clap_sys::factory::preset_discovery::CLAP_PRESET_DISCOVERY_FACTORY_ID_COMPAT.as_ptr()).is_null();
        let factory = library.factory().ok_or("No plugin factory")?;
        let count = factory.get_plugin_count.unwrap()(factory);
        if count == 0 || count > 4096 {
            return Err("Invalid factory descriptor count".into());
        }
        let mut ids: Vec<CString> = Vec::new();
        for i in 0..count {
            let d = factory.get_plugin_descriptor.unwrap()(factory, i);
            let bad = |p: *const c_char| p.is_null() || *p == 0;
            if d.is_null() || bad((*d).id) || bad((*d).name) || !clap_sys::version::clap_version_is_compatible((*d).clap_version) {
                return Err("Invalid or incompatible plugin descriptor".into());
            }
            let id = CStr::from_ptr((*d).id).to_owned();
            if ids.contains(&id) {
                return Err("Duplicate plugin ID in factory".into());
            }
            ids.push(id);
        }
        Ok(ids)
    }
}

/// What loading a CLAP plugin in this process did (clap_host_cpp.cpp's cpp_clap_new).
///
/// # Safety
/// path and id are NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn rust_clap_new(path: *const c_char, id: *const c_char, rate: f64, frames: u32, offline: bool, memory_room: u64) -> *mut Host {
    // SAFETY: the caller's.
    let (path, id) = unsafe { (CStr::from_ptr(path).to_string_lossy().into_owned(), CStr::from_ptr(id)) };
    let found = match inspect(&path) {
        Ok(ids) => ids.into_iter().find(|p| id.is_empty() || p.as_c_str() == id),
        Err(e) => return failed(e, null_mut()),
    };
    let Some(found) = found else { return failed(format!("No matching CLAP plugin in {path}"), null_mut()) };
    let host = Box::into_raw(Host::new());
    // SAFETY: a new host on this thread, its main one.
    unsafe {
        (*host).memory_room = memory_room;
        (*host).offline = offline;
        if !load(host, &path, &found, rate, frames) {
            let message = format!("Cannot load {}: {}", found.to_string_lossy(), (*host).message);
            drop(Box::from_raw(host));
            return failed(message, null_mut());
        }
        (*host).memory_room = 0;
        lock_audio_layout(host);
        // The first service (the plugin's init callback, its parameter flush) holds the gate; run it before any audio
        // thread can see the plugin, or its first block is refused as a control silence.
        service(host);
    }
    host
}
/// Host::load.
unsafe fn load(s: *mut Host, path: &str, id: &CStr, rate: f64, frames: u32) -> bool {
    // SAFETY: load's caller's.
    unsafe {
        if !main(s) {
            return false;
        }
        let gate = host::Gate::new(&(*s).gate);
        close(s);
        (*s).message.clear();
        match host::load(s, path, id, rate, frames) {
            Ok(()) => return true,
            Err(e) => (*s).message = e,
        }
        close(s);
        drop(gate);
        (*s).failed.store(true, SeqCst);
        false
    }
}
/// # Safety
/// host is rust_clap_new's, freed once, on its main thread.
#[no_mangle]
pub unsafe extern "C" fn rust_clap_free(host: *mut Host) {
    if host.is_null() {
        return;
    }
    // SAFETY: the caller's.
    unsafe {
        let gate = host::Gate::new(&(*host).gate);
        close(host);
        drop(gate);
        drop(Box::from_raw(host));
    }
}

// The rest, one by one: each the C++ reference's call (clap_host_cpp.cpp) on this host.
macro_rules! host_fn {
    ($(fn $name:ident($h:ident $(, $a:ident: $t:ty)*) -> $r:ty $body:block)*) => {$(
        /// # Safety
        /// host is a live rust_clap_new host, on the thread clap_host.h names for this call.
        #[no_mangle]
        pub unsafe extern "C" fn $name($h: *mut Host $(, $a: $t)*) -> $r {
            // SAFETY: the caller's.
            unsafe { $body }
        }
    )*};
}
host_fn! {
    fn rust_clap_revision(h) -> u64 { (*h).metadata_version }
    fn rust_clap_note_input_ports(h) -> u32 { (*h).note_inputs.len() as u32 }
    fn rust_clap_note_output_ports(h) -> u32 { (*h).note_outputs.len() as u32 }
    fn rust_clap_uses_clap_notes(h, port: u32) -> bool {
        (*h).note_inputs.get(port as usize).is_some_and(|p| p.supported_dialects & clap_sys::ext::note_ports::CLAP_NOTE_DIALECT_CLAP != 0)
    }
    fn rust_clap_takes_sysex(h) -> bool { midi_port(&(*h).note_inputs) }
    fn rust_clap_emits_sysex(h) -> bool { midi_port(&(*h).note_outputs) }
    fn rust_clap_supports_state(h) -> bool { supports_state(h) }
    fn rust_clap_supports_sound_snapshot(h) -> bool { exclusive_state(h) && supports_state(h) }
    fn rust_clap_has_window(h) -> bool { !(*h).gui_api.is_null() }
    fn rust_clap_voice_budget(h) -> bool { !(*h).voice_budget.is_null() }
    fn rust_clap_honors_volume(h) -> bool { (*h).honors_volume }
    fn rust_clap_service(h) -> bool { service(h); true }
    fn rust_clap_reset(h) -> bool { reset(h); true }
    fn rust_clap_reconfigure(h, rate: f64, frames: u32) -> bool {
        if reconfigure(h, rate, frames) { true } else { failed((*h).message.clone(), false) }
    }
    fn rust_clap_set_memory_room(h, bytes: u64) -> () { (*h).memory_room = bytes }
    fn rust_clap_set_active_outputs(h, ports: u64) -> () { set_active_outputs(h, ports) }
    fn rust_clap_set_assets(h, assets: *const oscairn_clap_assets) -> () { (*h).assets = assets.as_ref().copied() }
    fn rust_clap_show_window(h, show: bool, title: *const c_char) -> bool {
        if !main(h) { return failed((*h).message.clone(), false) }
        if !show { if !(*h).plugin.is_null() { destroy_gui(h) } return true }
        if (*h).plugin.is_null() || (*h).gui_api.is_null() { return failed("Plugin has no window here".into(), false) }
        match show_gui(h, CStr::from_ptr(title)) { Ok(()) => true, Err(e) => { destroy_gui(h); failed(e, false) } }
    }
    fn rust_clap_presets(h, each: Option<unsafe extern "C" fn(*mut c_void, *const oscairn_clap_preset)>, context: *mut c_void) -> bool {
        let Some(library) = (*h).library.clone() else { return true };
        if !main(h) || library.entry.is_null() { return true }
        struct To { each: unsafe extern "C" fn(*mut c_void, *const oscairn_clap_preset), context: *mut c_void }
        unsafe extern "C" fn one(c: *mut c_void, name: *const c_char, kind: u32, location: *const c_char, key: *const c_char) {
            // SAFETY: the To below, alive for the call; the strings kick_clap's.
            unsafe { let t = &*c.cast::<To>(); (t.each)(t.context, &oscairn_clap_preset { name, kind, location, load_key: key }) }
        }
        let mut to = To { each: each.unwrap(), context };
        let assets = (*h).assets;
        let failure = kick_clap_presets(library.entry.cast(), (*h).info.id.as_ptr(), assets.as_ref().map_or(std::ptr::null(), |a| a), Some(one), std::ptr::from_mut(&mut to).cast());
        if failure.is_null() { true } else { failed(CStr::from_ptr(failure).to_string_lossy().into_owned(), false) }
    }
    fn rust_clap_declared_memory(h, memory: *mut oscairn_clap_sys::oscairn_memory_t) -> c_int {
        if (*h).plugin.is_null() || !(*h).has_declared { return 0 }
        *memory = (*h).declared;
        1
    }
    fn rust_clap_effect_resources(h, resources: *mut oscairn_clap_sys::oscairn_effect_resources_t) -> c_int {
        if (*h).plugin.is_null() { return 0 }
        let p = (*h).plugin;
        let e = ((*p).get_extension.unwrap())(p, oscairn_clap_sys::ids::OSCAIRN_EXT_EFFECT_CONTEXTS.as_ptr()).cast::<oscairn_clap_sys::oscairn_plugin_effect_contexts_t>();
        c_int::from(e.as_ref().and_then(|e| e.resources).is_some_and(|f| f(p, resources)))
    }
    fn rust_clap_parameter_value(h, id: u32) -> f64 {
        let i = param_index(h, id);
        if i < (*h).param_info.len() { f64::from_bits((*h).param_values[i].load(Relaxed)) } else { 0.0 }
    }
    fn rust_clap_parameter_text(h, id: u32, value: f64, text: oscairn_clap_text, context: *mut c_void) -> bool {
        let t = parameter_text(h, id, value);
        text.unwrap()(context, t.as_ptr().cast(), t.len());
        true
    }
    fn rust_clap_set_parameter(h, id: u32, value: f64) -> bool { set_parameter(h, id, value) }
    fn rust_clap_parameter_gesture(h, id: u32, begin: bool) -> bool { parameter_gesture(h, id, begin) }
    fn rust_clap_parameter_reports(h) -> u64 { (*h).parameter_reports.load(Acquire) }
    fn rust_clap_send_event(h, e: *const event::Event) -> bool { enqueue(h, &*e) }
    fn rust_clap_pop_output(h, e: *mut event::Event) -> bool {
        match pop_output(h) { Some(x) => { *e = x; true } None => false }
    }
    fn rust_clap_output_event_drops(h) -> u64 { (*h).output_drops.load(SeqCst) }
    fn rust_clap_statistics(h, out: *mut oscairn_clap_statistics) -> () {
        let s = &*h;
        let f = |a: &std::sync::atomic::AtomicU64| f64::from_bits(a.load(SeqCst));
        *out = std::mem::zeroed();
        (*out).blocks = s.blocks.load(SeqCst);
        (*out).droppedEvents = s.dropped.load(SeqCst);
        (*out).processErrors = s.errors.load(SeqCst);
        (*out).lastMicroseconds = f(&s.last_us);
        (*out).maximumMicroseconds = f(&s.max_us);
        (*out).deadlineMisses = s.deadline_misses.load(SeqCst);
        (*out).controlSilences = s.control_silences.load(SeqCst);
        (*out).idleBlocks = s.idle_blocks.load(SeqCst);
        (*out).cpuMicroseconds = f(&s.total_us);
    }
    fn rust_clap_sound_context_capacity(h) -> u32 {
        (*h).sounds.as_ref().map_or(0, |x| x.capacity.unwrap()((*h).plugin))
    }
    fn rust_clap_prepares_snapshots(h) -> bool { !(*h).sound_snapshots.is_null() }
    fn rust_clap_saves_sounds(h) -> bool {
        (*h).sound_snapshots.as_ref().is_some_and(|x| x.save_sound.is_some() && x.sound_values.is_some())
    }
    fn rust_clap_rings_out(h) -> bool { (*h).ring_out_channels != 0 }
    fn rust_clap_process(h, inputs: *const *const f32, ni: u32, outputs: *const *mut f32, no: u32, frames: u32,
                         events: *const event::Event, count: u32, transport: *const clap_sys::events::clap_event_transport) -> bool {
        process(h, inputs, ni, outputs, no, frames, events, count, transport)
    }
    fn rust_clap_output_events(h, count: *mut u32) -> *const event::Event {
        *count = (*h).published_output_count;
        (*h).block_output.as_ptr()
    }
    fn rust_clap_panic(h) -> () { panic(h) }
    fn rust_clap_asleep(h) -> bool { (*h).sleeping.load(SeqCst) }
    fn rust_clap_restful(h) -> bool { restful(h) }
    fn rust_clap_rested(h, frames: u32, blocks: u32) -> () { rested(h, frames, blocks) }
    fn rust_clap_mirror_wake(h, mirror: *mut std::sync::atomic::AtomicU32) -> () { mirror_wake(h, mirror) }
    fn rust_clap_was_reset(h) -> bool { (*h).reset_in_process }
    fn rust_clap_set_voice_limits(h, limit: u32, contexts: *const oscairn_clap_sys::oscairn_voice_budget_context_t, count: u32) -> () {
        set_voice_limits(h, limit, contexts, count)
    }
    fn rust_clap_voice_report(h, cost: *mut f64, notes: *mut *const oscairn_clap_sys::oscairn_voice_budget_note_t, count: *mut u32) -> u32 {
        voice_report(h, &mut *cost, &mut *notes, &mut *count)
    }
}

/// rust_clap_describe: the descriptor, its lists, the ports and the parameters, one by one.
///
/// # Safety
/// As host_fn's; to's callbacks callable.
#[no_mangle]
pub unsafe extern "C" fn rust_clap_describe(h: *mut Host, to: *const oscairn_clap_description) {
    // SAFETY: the caller's.
    unsafe {
        let to = &*to;
        let c = to.context;
        let d = &(*h).info;
        let descriptor = oscairn_clap_descriptor {
            path: d.path.as_ptr(),
            id: d.id.as_ptr(),
            name: d.name.as_ptr(),
            vendor: d.vendor.as_ptr(),
            version: d.version.as_ptr(),
            description: d.description.as_ptr(),
            preset_discovery: false, // as Host::load's describe leaves it
        };
        to.descriptor.unwrap()(c, &descriptor);
        for f in &d.features {
            to.feature.unwrap()(c, f.as_ptr());
        }
        for (number, name) in &d.lanes {
            to.lane.unwrap()(c, *number, name.as_ptr());
        }
        for (input, ports) in [(true, &(*h).inputs), (false, &(*h).outputs)] {
            for p in ports {
                let port = oscairn_clap_port { id: p.id, name: p.name.as_ptr(), channels: p.channels, flags: p.flags };
                to.port.unwrap()(c, input, &port);
            }
        }
        for p in &(*h).param_info {
            let q = oscairn_clap_parameter {
                id: p.id,
                name: p.name.as_ptr(),
                module: p.module.as_ptr(),
                minimum: p.minimum,
                maximum: p.maximum,
                default_value: p.default_value,
                flags: p.flags,
                widget: p.widget,
                priority: p.priority,
                range_high: p.range_high,
                group: p.group,
                role: p.role,
                view: p.view,
                field: p.field,
                index: p.index,
                algorithms: p.algorithms.as_ptr(),
                algorithm_count: p.algorithms.len() as u32,
                seconds: p.seconds.as_ptr(),
                second_count: p.seconds.len() as u32,
            };
            to.parameter.unwrap()(c, &q);
        }
    }
}

// The control calls of the rest of the host (more.rs), each as clap_host_cpp.cpp's.
host_fn! {
    fn rust_clap_save_state(h, directory: *const c_char) -> bool {
        more::save_state_files(h, arg(directory)).map_or_else(|e| fail_with(e, false), |()| true)
    }
    fn rust_clap_load_state(h, directory: *const c_char) -> bool {
        more::load_state_files(h, arg(directory)).map_or_else(|e| fail_with(e, false), |()| true)
    }
    fn rust_clap_load_preset(h, preset: *const oscairn_clap_preset) -> bool {
        let p = &*preset;
        if more::load_preset(h, p.kind, CStr::from_ptr(p.location), CStr::from_ptr(p.load_key)) { true } else { failed((*h).message.clone(), false) }
    }
    fn rust_clap_asset_dependencies(h, each: Option<unsafe extern "C" fn(*mut c_void, *const oscairn_clap_asset)>, context: *mut c_void) -> bool {
        let mut all = Vec::new();
        if let Err(e) = more::asset_dependencies(h, |l, p, s, o| all.push((l.to_owned(), p.to_owned(), s.to_owned(), o))) { return fail_with(e, false) }
        for (l, p, s, o) in &all { each.unwrap()(context, &oscairn_clap_asset { library: l.as_ptr(), path: p.as_ptr(), sha256: s.as_ptr(), optional: *o }) }
        true
    }
    fn rust_clap_sound_contexts(h, each: Option<unsafe extern "C" fn(*mut c_void, *const oscairn_clap_sys::oscairn_sound_context_info_t)>, context: *mut c_void) -> bool {
        for c in more::sound_contexts(h) { each.unwrap()(context, &c) }
        true
    }
    fn rust_clap_prepare_sound(h, name: *const c_char, parameters: *const oscairn_clap_sys::oscairn_sound_parameter_t, count: u32,
                               patch: *const c_char, library: *const c_char, directory: *const c_char, id: *mut u32) -> bool {
        let values = if count == 0 { &[][..] } else { std::slice::from_raw_parts(parameters, count as usize) };
        match more::prepare_sound(h, arg(name), values, arg(patch), arg(library), arg(directory)) { Ok(x) => { *id = x; true } Err(e) => fail_with(e, false) }
    }
    fn rust_clap_prepare_preset(h, name: *const c_char, preset: *const oscairn_clap_preset, id: *mut u32) -> bool {
        let p = &*preset;
        match more::prepare_preset(h, arg(name), p.kind, arg(p.location), arg(p.load_key)) { Ok(x) => { *id = x; true } Err(e) => fail_with(e, false) }
    }
    fn rust_clap_select_sound(h, id: u32, channel: i16) -> c_int {
        more::select_sound(h, id, channel).map_or_else(|e| fail_with(e, -1), c_int::from)
    }
    fn rust_clap_discard_sound(h, id: u32) -> c_int {
        more::discard_sound(h, id).map_or_else(|e| fail_with(e, -1), c_int::from)
    }
    fn rust_clap_choke_retiring(h) -> c_int { c_int::from(more::choke_retiring(h)) }
    fn rust_clap_prepare_snapshot(h, directory: *const c_char, ids: *mut u32) -> bool {
        match more::prepare_snapshot_files(h, arg(directory)) { Ok(x) => { std::ptr::copy_nonoverlapping(x.as_ptr(), ids, 16); true } Err(e) => fail_with(e, false) }
    }
    fn rust_clap_save_sound(h, id: u32, directory: *const c_char) -> bool {
        more::save_sound_files(h, id, arg(directory)).map_or_else(|e| fail_with(e, false), |()| true)
    }
    fn rust_clap_sound_values(h, id: u32, each: Option<unsafe extern "C" fn(*mut c_void, *const oscairn_clap_sys::oscairn_sound_parameter_t)>, context: *mut c_void) -> bool {
        for v in more::sound_values(h, id) { each.unwrap()(context, &v) }
        true
    }
}

/// An effect context made from a loaded instance (cpp_clap_effect_context).
///
/// # Safety
/// As host_fn's.
#[no_mangle]
pub unsafe extern "C" fn rust_clap_effect_context(source: *mut Host) -> *mut Host {
    // SAFETY: the caller's.
    match unsafe { more::effect_context(source) } {
        Ok(h) => Box::into_raw(h),
        Err(e) => fail_with(e, null_mut()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clap_host_h_unchanged() {
        let fnv = |b: &[u8]| b.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
        let now = [fnv(include_bytes!("../../../plugin-host/src/clap_host.h")), fnv(include_bytes!("../../include/kick_clap/bridge.h"))];
        assert_eq!(now, crate::bindings::MADE_FROM, "clap_host.h or kick_clap/bridge.h changed: regenerate src/bindings.rs with core/engine/rust/bindgen");
    }
    /// The vendored oscairn-clap-sys was generated from oscairn-clap's headers as core/clap-ext holds them.
    #[test]
    fn extension_headers_unchanged() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../clap-ext/include/oscairn-clap");
        let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "h")).collect();
        names.sort();
        let fnv = names.iter().flat_map(|p| std::fs::read(p).unwrap()).fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
        assert_eq!(
            fnv, 0x7ab8_910a_98b4_5f79,
            "core/clap-ext/include/oscairn-clap changed: re-vendor oscairn-clap-sys (vendor/README.md) and update this hash"
        );
    }
    /// The CLAP version the host announces is the vendored headers' (clap-sys 0.5.0's own is older).
    #[test]
    fn clap_version_is_the_headers() {
        let h = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../third_party/clap/include/clap/version.h")).unwrap();
        let v = |name: &str| h.lines().find_map(|l| l.strip_prefix(&format!("#define {name} "))).unwrap().trim().parse::<u32>().unwrap();
        let c = crate::host::CLAP_VERSION;
        assert_eq!((c.major, c.minor, c.revision), (v("CLAP_VERSION_MAJOR"), v("CLAP_VERSION_MINOR"), v("CLAP_VERSION_REVISION")));
    }
}
