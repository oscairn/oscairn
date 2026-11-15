// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rest of kick::clap::Host (Host.cpp): state, effect contexts, native sounds and snapshots, asset dependencies,
//! presets loaded; and what clap_host_cpp.cpp does around them (state bundles, StateFiles.h).
use crate::event::Event;
use crate::host::*;
use crate::state::{self, Bundle};
use clap_sys::events::*;
use clap_sys::ext::preset_load::*;
use clap_sys::ext::state::clap_plugin_state;
use clap_sys::factory::preset_discovery::CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN;
use clap_sys::id::CLAP_INVALID_ID;
use clap_sys::stream::{clap_istream, clap_ostream};
use oscairn_clap_sys::*;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::ptr::null;
use std::sync::atomic::Ordering::*;
use std::time::{Duration, Instant};

pub const MAX_STATE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PATCH_BYTES: usize = 1024 * 1024;
const MAX_PRESET_LOCATION: usize = 4096; // kick::clap::maxPresetLocationBytes

/// Host.cpp's Stream: a state written into bytes (bounded) or read from them.
struct Stream {
    bytes: Vec<u8>,
    offset: usize,
    failed: bool,
}
unsafe extern "C" fn stream_write(s: *const clap_ostream, data: *const c_void, size: u64) -> i64 {
    // SAFETY: the host's own stream; data readable for size (the plugin's).
    unsafe {
        let me = &mut *(*s).ctx.cast::<Stream>();
        if size > MAX_STATE_BYTES - me.bytes.len() as u64 || me.bytes.try_reserve(size as usize).is_err() {
            me.failed = true;
            return -1;
        }
        me.bytes.extend_from_slice(std::slice::from_raw_parts(data.cast::<u8>(), size as usize));
        size as i64
    }
}
unsafe extern "C" fn stream_read(s: *const clap_istream, data: *mut c_void, size: u64) -> i64 {
    // SAFETY: as stream_write; data writable for size.
    unsafe {
        let me = &mut *(*s).ctx.cast::<Stream>();
        let count = (size as usize).min(me.bytes.len() - me.offset);
        std::ptr::copy_nonoverlapping(me.bytes.as_ptr().add(me.offset), data.cast::<u8>(), count);
        me.offset += count;
        count as i64
    }
}
fn saved(save: impl FnOnce(&clap_ostream) -> bool) -> Option<Vec<u8>> {
    let mut stream = Stream { bytes: Vec::new(), offset: 0, failed: false };
    let output = clap_ostream { ctx: std::ptr::from_mut(&mut stream).cast(), write: Some(stream_write) };
    (save(&output) && !stream.failed).then_some(stream.bytes)
}
fn loaded(bytes: Vec<u8>, load: impl FnOnce(&clap_istream) -> bool) -> bool {
    let mut stream = Stream { bytes, offset: 0, failed: false };
    let input = clap_istream { ctx: std::ptr::from_mut(&mut stream).cast(), read: Some(stream_read) };
    load(&input)
}
fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

/// Host::saveState.
///
/// # Safety
/// As host.rs's functions (main thread).
pub unsafe fn save_state(s: *mut Host) -> Option<Vec<u8>> {
    // SAFETY: the caller's.
    unsafe {
        let st: *const clap_plugin_state = state(s);
        if !main(s) || (*s).plugin.is_null() || st.is_null() || (*st).save.is_none() {
            (*s).message = "Plugin does not support state saving".into();
            return None;
        }
        let save = (*st).save.unwrap();
        let p = (*s).plugin;
        // Parameter changes still queued for the audio thread must reach the plugin before it saves. While audio runs,
        // wait briefly for process() to deliver them, then save without the gate so audio keeps playing.
        if (*s).pending_commands.load(SeqCst) > 0 {
            let audio_blocks = || (*s).blocks.load(SeqCst) + (*s).idle_blocks.load(SeqCst) + (*s).control_silences.load(SeqCst);
            let mut seen = audio_blocks();
            let mut progress = Instant::now();
            let deadline = progress + Duration::from_millis(100);
            while (*s).pending_commands.load(SeqCst) > 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_micros(500));
                if audio_blocks() != seen {
                    seen = audio_blocks();
                    progress = Instant::now();
                } else if progress.elapsed() > Duration::from_millis(10) {
                    break; // audio is not running
                }
            }
        }
        if (*s).pending_commands.load(SeqCst) == 0 && !exclusive_state(s) {
            let Some(bytes) = saved(|o| save(p, o)) else {
                (*s).message = "Plugin state saving failed".into();
                return None;
            };
            (*s).dirty_clear();
            return Some(bytes);
        }
        // Audio is not running: deliver the queued changes through a parameter flush under the gate, then save.
        let _gate = Gate::new(&(*s).gate);
        stop_processing(s);
        flush_commands(s);
        let Some(bytes) = saved(|o| save(p, o)) else {
            (*s).message = "Plugin state saving failed".into();
            return None;
        };
        (*s).dirty_clear();
        Some(bytes)
    }
}

/// Host::loadState.
///
/// # Safety
/// As save_state.
pub unsafe fn load_state(s: *mut Host, bytes: Vec<u8>) -> bool {
    // SAFETY: the caller's.
    unsafe {
        let st: *const clap_plugin_state = state(s);
        if !main(s) || (*s).plugin.is_null() || st.is_null() || (*st).load.is_none() || bytes.len() as u64 > MAX_STATE_BYTES {
            return false;
        }
        let _gate = Gate::new(&(*s).gate);
        deactivate(s);
        (*s).last_logged_error_clear();
        let p = (*s).plugin;
        if !loaded(bytes, |i| ((*st).load.unwrap())(p, i)) {
            if let Err(e) = activate(s) {
                fail(s, format!("Plugin state restoration failed: {e}"));
                return false;
            }
            let logged = (*s).last_logged_error();
            (*s).message = if logged.is_empty() { "Plugin rejected state".into() } else { format!("Plugin rejected state: {logged}") };
            return false;
        }
        drain_commands(s);
        (*s).carry_count = 0;
        if let Err(e) = activate(s) {
            fail(s, format!("Plugin state restoration failed: {e}"));
            return false;
        }
        (*s).dirty_clear();
        (*s).failed.store(false, SeqCst);
        true
    }
}

/// cpp_clap_save_state: the state in a new directory (plugin.id, state.clap).
///
/// # Safety
/// As save_state.
pub unsafe fn save_state_files(s: *mut Host, directory: &str) -> Result<(), Failure> {
    let bundle = Bundle::new(directory)?;
    // SAFETY: the caller's.
    let bytes = unsafe { save_state(s) }.ok_or_else(|| Failure::runtime(unsafe { (*s).message.clone() }))?;
    // SAFETY: as above.
    let id = unsafe { (*s).info.id.as_bytes() };
    state::write(&bundle.temporary.join("plugin.id"), id)?;
    state::write(&bundle.temporary.join("state.clap"), &bytes)?;
    bundle.publish()
}
/// cpp_clap_load_state.
///
/// # Safety
/// As save_state.
pub unsafe fn load_state_files(s: *mut Host, directory: &str) -> Result<(), Failure> {
    let path = state::canonical(Path::new(directory))?;
    let id = state::read(&path.join("plugin.id"), 65536)?;
    // SAFETY: the caller's.
    if id != unsafe { (*s).info.id.as_bytes() } {
        return Err(Failure::runtime("State belongs to a different CLAP plugin"));
    }
    let bytes = state::read(&path.join("state.clap"), MAX_STATE_BYTES)?;
    // SAFETY: as above.
    if unsafe { load_state(s, bytes) } {
        return Ok(());
    }
    // SAFETY: as above.
    let why = unsafe { (*s).message.clone() };
    Err(Failure::runtime(format!("CLAP state restore failed: {why}")))
}

/// Host::loadEffectContext; the new host made from `parent`.
///
/// # Safety
/// parent is a live host; this on its main thread.
pub unsafe fn effect_context(parent: *mut Host) -> Result<Box<Host>, Failure> {
    let mut child = Host::new();
    let s: *mut Host = &mut *child;
    // SAFETY: the caller's; s is new.
    unsafe {
        if !main(s) || (*parent).plugin.is_null() || !(*parent).loaded.load(SeqCst) {
            return Err(Failure::runtime((*s).message.clone()));
        }
        let gate = Gate::new(&(*s).gate);
        close(s);
        (*s).message.clear();
        let made = (|| -> R<()> {
            let pp = (*parent).plugin;
            let ext =
                ((*pp).get_extension.unwrap())(pp, oscairn_clap_sys::ids::OSCAIRN_EXT_EFFECT_CONTEXTS.as_ptr()).cast::<oscairn_plugin_effect_contexts_t>();
            let Some(create) = ext.as_ref().filter(|e| e.resources.is_some()).and_then(|e| e.create) else {
                return Err("Plugin has no native effect contexts".into());
            };
            inherit(s, parent);
            (*s).plugin = create(pp, &(*s).host);
            if (*s).plugin.is_null() {
                return Err("Effect context capacity exhausted".into());
            }
            let (rate, frames) = (*parent).timing();
            initialize_context(s, rate, frames)?;
            (*s).memory_room = 0; // the source's room was for this first activation; later ones are given their own (RFC 0025)
            Ok(())
        })();
        if let Err(e) = made {
            (*s).message = e;
            close(s);
            (*s).failed.store(true, SeqCst);
            drop(gate);
            return Err(Failure::runtime((*s).message.clone()));
        }
        drop(gate);
        lock_audio_layout(s);
    }
    Ok(child)
}

/// Host::assetDependencies.
///
/// # Safety
/// As save_state.
pub unsafe fn asset_dependencies(s: *const Host, mut each: impl FnMut(&CStr, &CStr, &CStr, bool)) -> Result<(), Failure> {
    // SAFETY: the caller's.
    unsafe {
        let Some(d) = (*s).asset_dependencies().as_ref() else { return Ok(()) };
        let p = (*s).plugin;
        for i in 0..d.count.unwrap()(p).min(65536) {
            let mut r: oscairn_asset_ref_t = std::mem::zeroed();
            if !d.get.unwrap()(p, i, &mut r) {
                return Err(Failure::runtime("Plugin did not describe an asset it uses"));
            }
            let optional = (*s).asset_optional() && d.optional.unwrap()(p, i);
            each(&field_c(&r.library), &field_c(&r.path), &field_c(&r.sha256), optional);
        }
        Ok(())
    }
}
fn field_c(p: &[c_char]) -> CString {
    CString::new(p.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect::<Vec<u8>>()).unwrap_or_default()
}

/// Host::loadPreset.
///
/// # Safety
/// As save_state.
pub unsafe fn load_preset(s: *mut Host, kind: u32, location: &CStr, key: &CStr) -> bool {
    // SAFETY: the caller's.
    unsafe {
        if !main(s) || (*s).plugin.is_null() {
            return false;
        }
        let p = (*s).plugin;
        let mut load = ((*p).get_extension.unwrap())(p, CLAP_EXT_PRESET_LOAD.as_ptr()).cast::<clap_plugin_preset_load>();
        if load.is_null() {
            load = ((*p).get_extension.unwrap())(p, CLAP_EXT_PRESET_LOAD_COMPAT.as_ptr()).cast();
        }
        let Some(from_location) = load.as_ref().and_then(|l| l.from_location) else {
            (*s).message = "Plugin cannot load presets".into();
            return false;
        };
        let _gate = Gate::new(&(*s).gate);
        (*s).message.clear();
        let location = if kind == CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN { null() } else { location.as_ptr() };
        let key = if key.is_empty() { null() } else { key.as_ptr() };
        if !from_location(p, kind, location, key) {
            if (*s).message.is_empty() {
                (*s).message = "Plugin did not load the preset".into();
            }
            return false;
        }
        // The preset changed the plugin's parameters: read them back.
        read_values(s);
        (*s).dirty_set();
        true
    }
}

// ---- Native sounds (oscairn.sound-contexts, sound-snapshots) ----

/// # Safety
/// As save_state.
pub unsafe fn sound_contexts(s: *const Host) -> Vec<oscairn_sound_context_info_t> {
    // SAFETY: the caller's.
    unsafe {
        let mut result = Vec::new();
        let Some(x) = (*s).sounds.as_ref() else { return result };
        for i in 0..x.capacity.unwrap()((*s).plugin) {
            let mut info: oscairn_sound_context_info_t = std::mem::zeroed();
            if x.get.unwrap()((*s).plugin, i, &mut info) {
                info.name[63] = 0;
                result.push(info);
            }
        }
        result
    }
}
/// RFC 0025: each sound context prepared on the running engine costs the context bytes the plugin declared at its last
/// activation; more than the memory room is refused.
unsafe fn admit_sounds(s: *const Host, count: u64) -> Result<(), Failure> {
    // SAFETY: the caller's.
    unsafe {
        if !(*s).has_declared || (*s).memory_room == 0 {
            return Ok(());
        }
        let bytes = (*s).declared.context_bytes.checked_mul(count).ok_or_else(|| Failure::runtime("Plugin's memory declaration is not credible"))?;
        if bytes > (*s).memory_room {
            return Err(Failure::runtime(refusal(s, bytes)));
        }
        Ok(())
    }
}
fn message_text(m: &[c_char; 512]) -> String {
    CStr::from_bytes_until_nul(&m.map(|c| c as u8)).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()
}
/// Host::prepareSound with parameters and a patch.
///
/// # Safety
/// As save_state.
pub unsafe fn prepare_sound(
    s: *mut Host,
    name: &str,
    parameters: &[oscairn_sound_parameter_t],
    patch: &str,
    library: &str,
    directory: &str,
) -> Result<u32, Failure> {
    // SAFETY: the caller's.
    unsafe {
        let Some(x) = (*s).sounds.as_ref() else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
        let r: oscairn_asset_ref_t = std::mem::zeroed();
        if name.len() > 63
            || parameters.len() > 128
            || patch.len() > MAX_PATCH_BYTES
            || patch.is_empty() != library.is_empty()
            || (patch.is_empty() && !directory.is_empty())
            || library.len() >= r.library.len()
            || directory.len() >= r.path.len()
        {
            return Err(Failure::invalid("Invalid sound patch"));
        }
        for (i, v) in parameters.iter().enumerate() {
            let p = (*s).param_info.iter().find(|p| p.id == v.id);
            let bad = p.is_none_or(|p| {
                p.flags & clap_sys::ext::params::CLAP_PARAM_IS_READONLY != 0 || !v.value.is_finite() || v.value < p.minimum || v.value > p.maximum
            });
            if bad {
                return Err(Failure::invalid("Invalid sound parameter"));
            }
            if parameters[..i].iter().any(|w| w.id == v.id) {
                return Err(Failure::invalid("Duplicate sound parameter"));
            }
        }
        admit_sounds(s, 1)?;
        let mut base: oscairn_asset_ref_t = std::mem::zeroed();
        base.library[..library.len()].copy_from_slice(&library.as_bytes().iter().map(|&b| b as c_char).collect::<Vec<_>>());
        base.path[..directory.len()].copy_from_slice(&directory.as_bytes().iter().map(|&b| b as c_char).collect::<Vec<_>>());
        let mut message = [0 as c_char; 512];
        let (cname, cpatch) = (cstr(name), cstr(patch));
        let id = x.prepare.unwrap()(
            (*s).plugin,
            cname.as_ptr(),
            parameters.as_ptr(),
            parameters.len() as u32,
            if patch.is_empty() { null() } else { cpatch.as_ptr() },
            if patch.is_empty() { null() } else { &base },
            message.as_mut_ptr(),
            512,
        );
        message[511] = 0;
        if id == CLAP_INVALID_ID {
            let m = message_text(&message);
            return Err(Failure::runtime(if m.is_empty() {
                "Sound preparation rejected: invalid patch or context capacity exhausted".into()
            } else {
                format!("Sound preparation rejected: {m}")
            }));
        }
        Ok(id)
    }
}
/// Host::prepareSound from a preset (draft 5's prepare_preset).
///
/// # Safety
/// As save_state.
pub unsafe fn prepare_preset(s: *mut Host, name: &str, kind: u32, location: &str, key: &str) -> Result<u32, Failure> {
    // SAFETY: the caller's.
    unsafe {
        let Some(x) = (*s).sounds.as_ref() else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
        let Some(prepare) = x.prepare_preset else { return Err(Failure::invalid("Plugin cannot prepare sounds from presets")) };
        if name.len() > 63
            || kind > CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN
            || (kind == CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN) != location.is_empty()
            || location.len() > MAX_PRESET_LOCATION
            || key.len() > MAX_PRESET_LOCATION
        {
            return Err(Failure::invalid("Invalid sound preset"));
        }
        admit_sounds(s, 1)?;
        let mut message = [0 as c_char; 512];
        let (cname, cloc, ckey) = (cstr(name), cstr(location), cstr(key));
        let id = prepare(
            (*s).plugin,
            cname.as_ptr(),
            kind,
            if location.is_empty() { null() } else { cloc.as_ptr() },
            if key.is_empty() { null() } else { ckey.as_ptr() },
            message.as_mut_ptr(),
            512,
        );
        message[511] = 0;
        if id == CLAP_INVALID_ID {
            let m = message_text(&message);
            return Err(Failure::runtime(format!("Preset not prepared: {}", if m.is_empty() { "rejected, or no free sound context".into() } else { m })));
        }
        Ok(id)
    }
}
/// # Safety
/// As save_state.
pub unsafe fn select_sound(s: *mut Host, id: u32, channel: i16) -> Result<bool, Failure> {
    if !(-1..=15).contains(&channel) {
        return Err(Failure::invalid("Invalid sound channel"));
    }
    // SAFETY: the caller's.
    unsafe {
        let Some(x) = (*s).sounds.as_ref() else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
        let accepted = x.select.unwrap()((*s).plugin, id, channel);
        if accepted {
            wake_up(s);
        }
        Ok(accepted)
    }
}
/// # Safety
/// As save_state.
pub unsafe fn discard_sound(s: *mut Host, id: u32) -> Result<bool, Failure> {
    // SAFETY: the caller's.
    unsafe {
        let Some(x) = (*s).sounds.as_ref() else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
        Ok(x.discard.unwrap()((*s).plugin, id))
    }
}
/// Host::chokeRetiringSounds.
///
/// # Safety
/// As save_state.
pub unsafe fn choke_retiring(s: *mut Host) -> bool {
    // SAFETY: the caller's.
    unsafe {
        if (*s).ring_out_channels == 0 {
            return false;
        }
        let queued = (*(*s).sound_snapshots).choke_retiring.unwrap()((*s).plugin);
        if queued {
            wake_up(s);
        }
        queued
    }
}
/// Host::prepareSoundSnapshot.
///
/// # Safety
/// As save_state.
pub unsafe fn prepare_sound_snapshot(s: *mut Host, bytes: Vec<u8>) -> Result<[u32; 16], Failure> {
    // SAFETY: the caller's.
    unsafe {
        let Some(x) = (*s).sound_snapshots.as_ref() else { return Err(Failure::invalid("Plugin cannot prepare a snapshot's sounds beside the playing ones")) };
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(Failure::invalid("Snapshot too large"));
        }
        let mut selected = [CLAP_INVALID_ID; 16];
        let mut message = [0 as c_char; 512];
        // Its sound count is the plugin's to read: at least its Default before, all of them after, its sounds discarded
        // again when they do not fit (RFC 0025).
        admit_sounds(s, 1)?;
        let before: Vec<u32> = sound_contexts(s).iter().map(|c| c.id).collect();
        let ok = loaded(bytes, |i| x.prepare.unwrap()((*s).plugin, i, selected.as_mut_ptr(), message.as_mut_ptr(), 512));
        message[511] = 0;
        if !ok {
            let m = message_text(&message);
            return Err(Failure::runtime(format!(
                "Snapshot not prepared beside: {}",
                if m.is_empty() { "refused, or too few free sound contexts".into() } else { m }
            )));
        }
        let fresh: Vec<u32> = sound_contexts(s).iter().map(|c| c.id).filter(|id| !before.contains(id)).collect();
        if let Err(e) = admit_sounds(s, fresh.len() as u64) {
            for id in fresh {
                (*(*s).sounds).discard.unwrap()((*s).plugin, id);
            }
            return Err(e);
        }
        Ok(selected)
    }
}
/// # Safety
/// As save_state.
pub unsafe fn saves_sounds(s: *const Host) -> bool {
    // SAFETY: the caller's.
    unsafe { (*s).sound_snapshots.as_ref().is_some_and(|x| x.save_sound.is_some() && x.sound_values.is_some()) }
}
/// Host::saveSoundState.
///
/// # Safety
/// As save_state.
pub unsafe fn save_sound_state(s: *mut Host, id: u32) -> Result<Vec<u8>, Failure> {
    // SAFETY: the caller's.
    unsafe {
        if !saves_sounds(s) || !main(s) {
            return Err(Failure::invalid("Plugin cannot save one sound on its own"));
        }
        let _gate = Gate::new(&(*s).gate);
        stop_processing(s);
        let save = (*(*s).sound_snapshots).save_sound.unwrap();
        saved(|o| save((*s).plugin, id, o)).ok_or_else(|| Failure::runtime("Plugin did not save the sound on its own"))
    }
}
/// # Safety
/// As save_state.
pub unsafe fn sound_values(s: *const Host, id: u32) -> Vec<oscairn_sound_parameter_t> {
    // SAFETY: the caller's.
    unsafe {
        if !saves_sounds(s) {
            return Vec::new();
        }
        let mut values: Vec<oscairn_sound_parameter_t> = (0..(*s).param_info.len()).map(|_| std::mem::zeroed()).collect();
        let n = (*(*s).sound_snapshots).sound_values.unwrap()((*s).plugin, id, values.as_mut_ptr(), values.len() as u32);
        values.truncate(n as usize);
        values
    }
}

/// cpp_clap_prepare_snapshot: what saveSnapshot wrote for a native engine ("state 1" and its state bundle).
///
/// # Safety
/// As save_state.
pub unsafe fn prepare_snapshot_files(s: *mut Host, directory: &str) -> Result<[u32; 16], Failure> {
    let path = Path::new(directory);
    if state::read(&path.join("snapshot"), 64)? != b"state 1\n" {
        return Err(Failure::runtime("Not a native sound snapshot"));
    }
    // SAFETY: the caller's.
    if state::read(&path.join("state").join("plugin.id"), 65536)? != unsafe { (*s).info.id.as_bytes() } {
        return Err(Failure::runtime("State belongs to a different CLAP plugin"));
    }
    let bytes = state::read(&path.join("state").join("state.clap"), MAX_STATE_BYTES)?;
    // SAFETY: as above.
    unsafe { prepare_sound_snapshot(s, bytes) }
}
/// cpp_clap_save_sound.
///
/// # Safety
/// As save_state.
pub unsafe fn save_sound_files(s: *mut Host, id: u32, directory: &str) -> Result<(), Failure> {
    let bundle = Bundle::new(directory)?;
    // SAFETY: the caller's.
    let bytes = unsafe { save_sound_state(s, id) }?;
    let state_dir = bundle.temporary.join("state");
    std::fs::create_dir(&state_dir).map_err(|e| state::fs_error("create_directory", &e, &[&state_dir]))?;
    // SAFETY: as above.
    state::write(&state_dir.join("plugin.id"), unsafe { (*s).info.id.as_bytes() })?;
    state::write(&state_dir.join("state.clap"), &bytes)?;
    state::write(&bundle.temporary.join("snapshot"), b"state 1\n")?;
    bundle.publish()
}

/// The events queued for the plugin taken now, parameters through a flush, the rest carried to its next block
/// (saveState's path without audio).
unsafe fn flush_commands(s: *mut Host) {
    // SAFETY: the caller's, under the gate.
    unsafe {
        let mut chunk = [Event::default(); MAX_BLOCK_EVENTS];
        let mut n = 0;
        while let Some(e) = pop_command(s) {
            (*s).pending_commands.fetch_sub(1, SeqCst);
            let t = e.header().type_;
            if t == CLAP_EVENT_PARAM_VALUE || t == CLAP_EVENT_PARAM_GESTURE_BEGIN || t == CLAP_EVENT_PARAM_GESTURE_END {
                if n < MAX_BLOCK_EVENTS {
                    chunk[n] = translate_event(s, e);
                    n += 1;
                }
            } else if ((*s).carry_count as usize) < MAX_BLOCK_EVENTS {
                (*s).carry_events[(*s).carry_count as usize] = e;
                (*s).carry_count += 1;
            } else {
                (*s).panic.store(true, SeqCst);
                (*s).dropped.fetch_add(1, SeqCst);
            }
        }
        flush_with(s, &chunk[..n]);
    }
}
