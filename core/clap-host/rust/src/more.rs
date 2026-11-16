// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rest of kick::clap::Host (Host.cpp): state, effect contexts, native sounds and snapshots, asset dependencies,
//! presets loaded; and what clap_host_cpp.cpp does around them (state bundles, StateFiles.h). All on the main thread.
use crate::event::Event;
use crate::host::*;
use crate::state::{self, Bundle};
use clap_sys::events::*;
use clap_sys::ext::preset_load::*;
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
pub fn save_state(s: H) -> Option<Vec<u8>> {
    let save = state(&s).and_then(|st| st.save);
    if !main(&s) || s.plugin.get().is_null() || save.is_none() {
        s.message.set("Plugin does not support state saving".into());
        return None;
    }
    let save = save.unwrap();
    let p = s.live();
    // Parameter changes still queued for the audio thread must reach the plugin before it saves. While audio runs,
    // wait briefly for process() to deliver them, then save without the gate so audio keeps playing.
    if s.pending_commands.load(SeqCst) > 0 {
        let audio_blocks = || s.blocks.load(SeqCst) + s.idle_blocks.load(SeqCst) + s.control_silences.load(SeqCst);
        let mut seen = audio_blocks();
        let mut progress = Instant::now();
        let deadline = progress + Duration::from_millis(100);
        while s.pending_commands.load(SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_micros(500));
            if audio_blocks() != seen {
                seen = audio_blocks();
                progress = Instant::now();
            } else if progress.elapsed() > Duration::from_millis(10) {
                break; // audio is not running
            }
        }
    }
    if s.pending_commands.load(SeqCst) == 0 && !exclusive_state(&s) {
        let Some(bytes) = saved(|o| p.call(save, (o,))) else {
            s.message.set("Plugin state saving failed".into());
            return None;
        };
        s.dirty_clear();
        return Some(bytes);
    }
    // Audio is not running: deliver the queued changes through a parameter flush under the gate, then save.
    let _gate = Gate::new(&s.gate);
    stop_processing(s);
    flush_commands(s);
    let Some(bytes) = saved(|o| p.call(save, (o,))) else {
        s.message.set("Plugin state saving failed".into());
        return None;
    };
    s.dirty_clear();
    Some(bytes)
}

/// Host::loadState.
pub fn load_state(s: H, bytes: Vec<u8>) -> bool {
    let load = state(&s).and_then(|st| st.load);
    if !main(&s) || s.plugin.get().is_null() || load.is_none() || bytes.len() as u64 > MAX_STATE_BYTES {
        return false;
    }
    let _gate = Gate::new(&s.gate);
    deactivate(s);
    s.last_logged_error_clear();
    let p = s.live();
    if !loaded(bytes, |i| p.call(load, (i,))) {
        if let Err(e) = activate(s) {
            fail(&s, format!("Plugin state restoration failed: {e}"));
            return false;
        }
        let logged = s.last_logged_error();
        s.message.set(if logged.is_empty() { "Plugin rejected state".into() } else { format!("Plugin rejected state: {logged}") });
        return false;
    }
    drain_commands(&s);
    s.carry_count.store(0, Relaxed);
    if let Err(e) = activate(s) {
        fail(&s, format!("Plugin state restoration failed: {e}"));
        return false;
    }
    s.dirty_clear();
    s.failed.store(false, SeqCst);
    true
}

/// cpp_clap_save_state: the state in a new directory (plugin.id, state.clap).
pub fn save_state_files(s: H, directory: &str) -> Result<(), Failure> {
    let bundle = Bundle::new(directory)?;
    let bytes = save_state(s).ok_or_else(|| Failure::runtime(s.message.with(|m| m.clone())))?;
    let id = s.info.with(|i| i.id.as_bytes().to_vec());
    state::write(&bundle.temporary.join("plugin.id"), &id)?;
    state::write(&bundle.temporary.join("state.clap"), &bytes)?;
    bundle.publish()
}
/// cpp_clap_load_state.
pub fn load_state_files(s: H, directory: &str) -> Result<(), Failure> {
    let path = state::canonical(Path::new(directory))?;
    let id = state::read(&path.join("plugin.id"), 65536)?;
    if !s.info.with(|i| id == i.id.as_bytes()) {
        return Err(Failure::runtime("State belongs to a different CLAP plugin"));
    }
    let bytes = state::read(&path.join("state.clap"), MAX_STATE_BYTES)?;
    if load_state(s, bytes) {
        return Ok(());
    }
    let why = s.message.with(|m| m.clone());
    Err(Failure::runtime(format!("CLAP state restore failed: {why}")))
}

/// Host::loadEffectContext; the new host made from `parent`, freed again where it fails.
pub fn effect_context(parent: H) -> Result<H, Failure> {
    let s = Host::create();
    let made = make_effect_context(s, parent);
    if made.is_err() {
        // SAFETY: Host::create's, closed, reached by nothing now: freed once, as the C++ unique_ptr is.
        drop(unsafe { Box::from_raw(s.ptr()) });
    }
    made.map(|()| s)
}
fn make_effect_context(s: H, parent: H) -> Result<(), Failure> {
    if !main(&s) || parent.plugin.get().is_null() || !parent.loaded.load(SeqCst) {
        return Err(Failure::runtime(s.message.with(|m| m.clone())));
    }
    let gate = Gate::new(&s.gate);
    close(s);
    s.message.with(String::clear);
    let made = (|| -> R<()> {
        let pp = parent.live();
        let ext = pp.ext::<oscairn_plugin_effect_contexts_t>(oscairn_clap_sys::ids::OSCAIRN_EXT_EFFECT_CONTEXTS);
        let Some(create) = ext.filter(|e| e.resources.is_some()).and_then(|e| e.create) else {
            return Err("Plugin has no native effect contexts".into());
        };
        inherit(&s, &parent);
        s.plugin.set(pp.call(create, (&s.host,))); // the new host's own clap_host, which outlives the context
        if s.plugin.get().is_null() {
            return Err("Effect context capacity exhausted".into());
        }
        let (rate, frames) = parent.timing();
        initialize_context(s, rate, frames)?;
        s.memory_room.set(0); // the source's room was for this first activation; later ones are given their own (RFC 0025)
        Ok(())
    })();
    if let Err(e) = made {
        s.message.set(e);
        close(s);
        s.failed.store(true, SeqCst);
        drop(gate);
        return Err(Failure::runtime(s.message.with(|m| m.clone())));
    }
    drop(gate);
    lock_audio_layout(&s);
    Ok(())
}

/// Host::assetDependencies.
pub fn asset_dependencies(s: &Host, mut each: impl FnMut(&CStr, &CStr, &CStr, bool)) -> Result<(), Failure> {
    // optional() is read only from draft 2 (asset_optional).
    let Some(d) = s.asset_dependencies() else { return Ok(()) };
    let p = s.live();
    for i in 0..p.call(d.count, ()).min(65536) {
        // SAFETY: plain old data, all zeros valid.
        let mut r: oscairn_asset_ref_t = unsafe { std::mem::zeroed() };
        if !p.call(d.get, (i, &mut r)) {
            return Err(Failure::runtime("Plugin did not describe an asset it uses"));
        }
        let optional = s.asset_optional() && p.call(d.optional, (i,));
        each(&field_c(&r.library), &field_c(&r.path), &field_c(&r.sha256), optional);
    }
    Ok(())
}
fn field_c(p: &[c_char]) -> CString {
    CString::new(p.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect::<Vec<u8>>()).unwrap_or_default()
}

/// Host::loadPreset.
pub fn load_preset(s: H, kind: u32, location: &CStr, key: &CStr) -> bool {
    if !main(&s) || s.plugin.get().is_null() {
        return false;
    }
    let p = s.live();
    let load = p.ext::<clap_plugin_preset_load>(CLAP_EXT_PRESET_LOAD).or_else(|| p.ext(CLAP_EXT_PRESET_LOAD_COMPAT));
    let Some(from_location) = load.and_then(|l| l.from_location) else {
        s.message.set("Plugin cannot load presets".into());
        return false;
    };
    let _gate = Gate::new(&s.gate);
    s.message.with(String::clear);
    let location = if kind == CLAP_PRESET_DISCOVERY_LOCATION_PLUGIN { null() } else { location.as_ptr() };
    let key = if key.is_empty() { null() } else { key.as_ptr() };
    if !p.call(from_location, (kind, location, key)) {
        if s.message.with(|m| m.is_empty()) {
            s.message.set("Plugin did not load the preset".into());
        }
        return false;
    }
    // The preset changed the plugin's parameters: read them back.
    read_values(&s);
    s.dirty_set();
    true
}

// ---- Native sounds (oscairn.sound-contexts, sound-snapshots) ----

/// The live plugin's sound-contexts table, checked complete at initialization.
fn sounds(s: &Host) -> Option<&oscairn_plugin_sound_contexts_t> {
    s.sounds.get()
}
pub fn sound_contexts(s: &Host) -> Vec<oscairn_sound_context_info_t> {
    let mut result = Vec::new();
    let Some(x) = sounds(s) else { return result };
    for i in 0..s.live().call(x.capacity, ()) {
        // SAFETY: plain old data, all zeros valid.
        let mut info: oscairn_sound_context_info_t = unsafe { std::mem::zeroed() };
        if s.live().call(x.get, (i, &mut info)) {
            info.name[63] = 0;
            result.push(info);
        }
    }
    result
}
/// RFC 0025: each sound context prepared on the running engine costs the context bytes the plugin declared at its last
/// activation; more than the memory room is refused.
fn admit_sounds(s: &Host, count: u64) -> Result<(), Failure> {
    if !s.has_declared.get() || s.memory_room.get() == 0 {
        return Ok(());
    }
    let bytes = s.declared.get().context_bytes.checked_mul(count).ok_or_else(|| Failure::runtime("Plugin's memory declaration is not credible"))?;
    if bytes > s.memory_room.get() {
        return Err(Failure::runtime(refusal(s, bytes)));
    }
    Ok(())
}
fn message_text(m: &[c_char; 512]) -> String {
    CStr::from_bytes_until_nul(&m.map(|c| c as u8)).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()
}
/// Host::prepareSound with parameters and a patch.
pub fn prepare_sound(s: &Host, name: &str, parameters: &[oscairn_sound_parameter_t], patch: &str, library: &str, directory: &str) -> Result<u32, Failure> {
    let Some(x) = sounds(s) else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
    // SAFETY: plain old data, all zeros valid.
    let r: oscairn_asset_ref_t = unsafe { std::mem::zeroed() };
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
        let bad = s.param_info.with(|info| {
            info.iter().find(|p| p.id == v.id).is_none_or(|p| {
                p.flags & clap_sys::ext::params::CLAP_PARAM_IS_READONLY != 0 || !v.value.is_finite() || v.value < p.minimum || v.value > p.maximum
            })
        });
        if bad {
            return Err(Failure::invalid("Invalid sound parameter"));
        }
        if parameters[..i].iter().any(|w| w.id == v.id) {
            return Err(Failure::invalid("Duplicate sound parameter"));
        }
    }
    admit_sounds(s, 1)?;
    // SAFETY: plain old data, all zeros valid.
    let mut base: oscairn_asset_ref_t = unsafe { std::mem::zeroed() };
    base.library[..library.len()].copy_from_slice(&library.as_bytes().iter().map(|&b| b as c_char).collect::<Vec<_>>());
    base.path[..directory.len()].copy_from_slice(&directory.as_bytes().iter().map(|&b| b as c_char).collect::<Vec<_>>());
    let mut message = [0 as c_char; 512];
    let (cname, cpatch) = (cstr(name), cstr(patch));
    let id = s.live().call(
        x.prepare,
        (
            cname.as_ptr(),
            parameters.as_ptr(),
            parameters.len() as u32,
            if patch.is_empty() { null() } else { cpatch.as_ptr() },
            if patch.is_empty() { null() } else { &base },
            message.as_mut_ptr(),
            512,
        ),
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
/// Host::prepareSound from a preset (draft 5's prepare_preset).
pub fn prepare_preset(s: &Host, name: &str, kind: u32, location: &str, key: &str) -> Result<u32, Failure> {
    let Some(x) = sounds(s) else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
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
    let id = s.live().call(
        prepare,
        (
            cname.as_ptr(),
            kind,
            if location.is_empty() { null() } else { cloc.as_ptr() },
            if key.is_empty() { null() } else { ckey.as_ptr() },
            message.as_mut_ptr(),
            512,
        ),
    );
    message[511] = 0;
    if id == CLAP_INVALID_ID {
        let m = message_text(&message);
        return Err(Failure::runtime(format!("Preset not prepared: {}", if m.is_empty() { "rejected, or no free sound context".into() } else { m })));
    }
    Ok(id)
}
pub fn select_sound(s: &Host, id: u32, channel: i16) -> Result<bool, Failure> {
    if !(-1..=15).contains(&channel) {
        return Err(Failure::invalid("Invalid sound channel"));
    }
    let Some(x) = sounds(s) else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
    let accepted = s.live().call(x.select, (id, channel));
    if accepted {
        wake_up(s);
    }
    Ok(accepted)
}
pub fn discard_sound(s: &Host, id: u32) -> Result<bool, Failure> {
    let Some(x) = sounds(s) else { return Err(Failure::invalid("Plugin has no native sound contexts")) };
    Ok(s.live().call(x.discard, (id,)))
}
/// Host::chokeRetiringSounds.
pub fn choke_retiring(s: &Host) -> bool {
    if s.ring_out_channels.get() == 0 {
        return false;
    }
    // choke_retiring is present with ring_out (checked at initialization)
    let queued = s.live().call(s.sound_snapshots.get().expect("the plugin's sound snapshots").choke_retiring, ());
    if queued {
        wake_up(s);
    }
    queued
}
/// Host::prepareSoundSnapshot.
pub fn prepare_sound_snapshot(s: &Host, bytes: Vec<u8>) -> Result<[u32; 16], Failure> {
    let Some(x) = s.sound_snapshots.get() else {
        return Err(Failure::invalid("Plugin cannot prepare a snapshot's sounds beside the playing ones"));
    };
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(Failure::invalid("Snapshot too large"));
    }
    let mut selected = [CLAP_INVALID_ID; 16];
    let mut message = [0 as c_char; 512];
    // Its sound count is the plugin's to read: at least its Default before, all of them after, its sounds discarded
    // again when they do not fit (RFC 0025).
    admit_sounds(s, 1)?;
    let before: Vec<u32> = sound_contexts(s).iter().map(|c| c.id).collect();
    let ok = loaded(bytes, |i| s.live().call(x.prepare, (i, selected.as_mut_ptr(), message.as_mut_ptr(), 512)));
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
            // a plugin with sound snapshots has sound contexts
            s.live().call(s.sounds.get().expect("the plugin's sound contexts").discard, (id,));
        }
        return Err(e);
    }
    Ok(selected)
}
pub fn saves_sounds(s: &Host) -> bool {
    s.sound_snapshots.get().is_some_and(|x| x.save_sound.is_some() && x.sound_values.is_some())
}
/// Host::saveSoundState.
pub fn save_sound_state(s: H, id: u32) -> Result<Vec<u8>, Failure> {
    if !saves_sounds(&s) || !main(&s) {
        return Err(Failure::invalid("Plugin cannot save one sound on its own"));
    }
    let _gate = Gate::new(&s.gate);
    stop_processing(s);
    let save = s.sound_snapshots.get().expect("the plugin's sound snapshots").save_sound;
    saved(|o| s.live().call(save, (id, o))).ok_or_else(|| Failure::runtime("Plugin did not save the sound on its own"))
}
pub fn sound_values(s: &Host, id: u32) -> Vec<oscairn_sound_parameter_t> {
    if !saves_sounds(s) {
        return Vec::new();
    }
    // SAFETY: plain old data, all zeros valid.
    let mut values: Vec<oscairn_sound_parameter_t> = (0..s.param_info.with(|v| v.len())).map(|_| unsafe { std::mem::zeroed() }).collect();
    let n = s.live().call(s.sound_snapshots.get().expect("the plugin's sound snapshots").sound_values, (id, values.as_mut_ptr(), values.len() as u32));
    values.truncate(n as usize);
    values
}

/// cpp_clap_prepare_snapshot: what saveSnapshot wrote for a native engine ("state 1" and its state bundle).
pub fn prepare_snapshot_files(s: &Host, directory: &str) -> Result<[u32; 16], Failure> {
    let path = Path::new(directory);
    if state::read(&path.join("snapshot"), 64)? != b"state 1\n" {
        return Err(Failure::runtime("Not a native sound snapshot"));
    }
    let id = state::read(&path.join("state").join("plugin.id"), 65536)?;
    if !s.info.with(|i| id == i.id.as_bytes()) {
        return Err(Failure::runtime("State belongs to a different CLAP plugin"));
    }
    let bytes = state::read(&path.join("state").join("state.clap"), MAX_STATE_BYTES)?;
    prepare_sound_snapshot(s, bytes)
}
/// cpp_clap_save_sound.
pub fn save_sound_files(s: H, id: u32, directory: &str) -> Result<(), Failure> {
    let bundle = Bundle::new(directory)?;
    let bytes = save_sound_state(s, id)?;
    let state_dir = bundle.temporary.join("state");
    std::fs::create_dir(&state_dir).map_err(|e| state::fs_error("create_directory", &e, &[&state_dir]))?;
    state::write(&state_dir.join("plugin.id"), &s.info.with(|i| i.id.as_bytes().to_vec()))?;
    state::write(&state_dir.join("state.clap"), &bytes)?;
    state::write(&bundle.temporary.join("snapshot"), b"state 1\n")?;
    bundle.publish()
}

/// The events queued for the plugin taken now, parameters through a flush, the rest carried to its next block
/// (saveState's path without audio). Under the gate.
fn flush_commands(s: H) {
    let mut chunk = [Event::default(); MAX_BLOCK_EVENTS];
    let mut n = 0;
    while let Some(e) = pop_command(&s) {
        s.pending_commands.fetch_sub(1, SeqCst);
        let t = e.header().type_;
        if t == CLAP_EVENT_PARAM_VALUE || t == CLAP_EVENT_PARAM_GESTURE_BEGIN || t == CLAP_EVENT_PARAM_GESTURE_END {
            if n < MAX_BLOCK_EVENTS {
                chunk[n] = translate_event(&s, e);
                n += 1;
            }
        } else if (s.carry_count.load(Relaxed) as usize) < MAX_BLOCK_EVENTS {
            let at = s.carry_count.load(Relaxed);
            s.carry_events.with(|c| c[at as usize] = e);
            s.carry_count.store(at + 1, Relaxed);
        } else {
            s.panic.store(true, SeqCst);
            s.dropped.fetch_add(1, SeqCst);
        }
    }
    flush_with(s, &chunk[..n]);
}
