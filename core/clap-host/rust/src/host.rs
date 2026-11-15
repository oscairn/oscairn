// SPDX-License-Identifier: AGPL-3.0-or-later
//! One plugin instance: kick::clap::Host (core/clap-host/src/Host.cpp), its behavior call for call.
//!
//! Threads as Host.h says: control calls on one main thread, process on one audio thread, enqueue and the counters on
//! any. The plugin calls the host back from inside any of them, so the host is reached through a raw pointer and its
//! fields one at a time (no reference to the whole lives across a call into the plugin); the main/audio split and the
//! gate keep two threads off the same field, as in the C++.
use crate::event::{self, Event, BEAT_SPACE, HARMONY_SPACE, PARAM_TARGET_SPACE};
use crate::library::{self, Library};
use crate::queue::Queue;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::audio_ports_activation::*;
use clap_sys::ext::event_registry::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::log::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::posix_fd_support::*;
use clap_sys::ext::preset_load::*;
use clap_sys::ext::render::*;
use clap_sys::ext::state::*;
use clap_sys::ext::tail::*;
use clap_sys::ext::thread_check::*;
use clap_sys::ext::timer_support::*;
use clap_sys::fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR};
use clap_sys::host::clap_host;
use clap_sys::id::{clap_id, CLAP_INVALID_ID};
use clap_sys::plugin::clap_plugin;
use clap_sys::process::*;
use clap_sys::string_sizes::{CLAP_NAME_SIZE, CLAP_PATH_SIZE};
use clap_sys::version::{clap_version, clap_version_is_compatible};
use oscairn_clap_sys::ids;
use oscairn_clap_sys::oscairn_field::*;
use oscairn_clap_sys::oscairn_role::OSCAIRN_ROLE_OUTPUT_BUS;
use oscairn_clap_sys::oscairn_widget::*;
use oscairn_clap_sys::*;
use std::cell::Cell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const MAX_EVENTS: usize = 2048; // Host::maxEvents
pub const MAX_AUDIO_CHANNELS: u32 = 64;
pub const MAX_BLOCK_EVENTS: usize = 512;
pub const MAX_PROCESS_EVENTS: usize = 2 * MAX_BLOCK_EVENTS + 1 + 32;
const RENDER_EVENTS: usize = 2 * MAX_BLOCK_EVENTS + MAX_PROCESS_EVENTS;
const SYSEX_BLOCK: u32 = 256 * 1024; // SysexPool::maxBlock
/// The CLAP version Oscairn's vendored headers are (clap-sys 0.5.0 says 1.2.2); what the host tells plugins it speaks.
pub const CLAP_VERSION: clap_version = clap_version { major: 1, minor: 2, revision: 10 };

pub struct Parameter {
    pub id: clap_id,
    pub name: CString,
    pub module: CString,
    pub minimum: f64,
    pub maximum: f64,
    pub default_value: f64,
    pub flags: u32,
    pub widget: u32,
    pub priority: u32,
    pub range_high: clap_id,
    pub group: u32,
    pub role: u32,
    pub view: u32,
    pub field: u32,
    pub index: u32,
    pub algorithms: Vec<[u8; 11]>,
    pub seconds: Vec<f64>,
}
impl Clone for Parameter {
    fn clone(&self) -> Self {
        Parameter { name: self.name.clone(), module: self.module.clone(), algorithms: self.algorithms.clone(), seconds: self.seconds.clone(), ..*self }
    }
}
pub struct Port {
    pub id: clap_id,
    pub name: CString,
    pub channels: u32,
    pub flags: u32,
}
#[derive(Default, Clone)]
pub struct Descriptor {
    pub path: CString,
    pub id: CString,
    pub name: CString,
    pub vendor: CString,
    pub version: CString,
    pub description: CString,
    pub features: Vec<CString>,
    pub lanes: Vec<(u32, CString)>,
}

#[derive(Clone, Copy, Default)]
struct Timed {
    event: Event,
    order: u32,
}
#[derive(Clone, Copy)]
struct Timer {
    id: clap_id,
    period: u32,
    next: Instant,
}
#[derive(Clone, Copy)]
struct Fd {
    fd: i32,
    flags: u32,
}

/// TransportWatch (Host.h): whether a transport moved other than by playing on.
#[derive(Default)]
pub struct TransportWatch {
    observed: bool,
    had: bool,
    previous: Option<clap_event_transport>,
    previous_frames: u32,
}
impl TransportWatch {
    pub fn changed(&mut self, current: Option<&clap_event_transport>, frames: u32, rate: f64) -> bool {
        let mut changed = !self.observed || self.had != current.is_some();
        if let (Some(c), true, true, Some(old)) = (current, self.had, self.observed, self.previous.as_ref()) {
            changed |= c.flags != old.flags;
            if c.flags & CLAP_TRANSPORT_HAS_TEMPO != 0 {
                changed |= c.tempo != old.tempo || c.tempo_inc != old.tempo_inc;
            }
            if c.flags & CLAP_TRANSPORT_HAS_TIME_SIGNATURE != 0 {
                changed |= c.tsig_num != old.tsig_num || c.tsig_denom != old.tsig_denom;
            }
            let elapsed = if old.flags & CLAP_TRANSPORT_IS_PLAYING != 0 { self.previous_frames as f64 / rate } else { 0.0 };
            // shortcut: f64 where the C++ uses long double (the same on arm64 and macOS; x86 Linux's is wider), within the 2-unit slack.
            if c.flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0 {
                let expected = old.song_pos_beats as f64 + elapsed * old.tempo / 60.0 * CLAP_BEATTIME_FACTOR as f64;
                changed |= (c.song_pos_beats as f64 - expected).abs() > 2.0;
                changed |= c.loop_start_beats != old.loop_start_beats
                    || c.loop_end_beats != old.loop_end_beats
                    || c.bar_start != old.bar_start
                    || c.bar_number != old.bar_number;
            }
            if c.flags & CLAP_TRANSPORT_HAS_SECONDS_TIMELINE != 0 {
                let expected = old.song_pos_seconds as f64 + elapsed * CLAP_SECTIME_FACTOR as f64;
                changed |= (c.song_pos_seconds as f64 - expected).abs() > 2.0;
                changed |= c.loop_start_seconds != old.loop_start_seconds || c.loop_end_seconds != old.loop_end_seconds;
            }
        }
        if let Some(c) = current {
            self.previous = Some(*c);
        }
        self.had = current.is_some();
        self.observed = true;
        self.previous_frames = frames;
        changed
    }
    pub fn reset(&mut self) {
        self.observed = false;
    }
}

pub struct Host {
    pub host: clap_host,
    main_thread: Cell<Option<std::thread::ThreadId>>,
    pub library: Option<Arc<Library>>,
    pub plugin: *const clap_plugin,
    params: *const clap_plugin_params,
    ui: *const oscairn_plugin_ui_t,
    ui_views: bool,
    ui_seconds: bool,
    state: *const clap_plugin_state,
    exclusive_state: bool,
    pub sounds: *const oscairn_plugin_sound_contexts_t,
    pub sound_snapshots: *const oscairn_plugin_sound_snapshots_t,
    pub voice_budget: *const oscairn_plugin_voice_budget_t,
    pub honors_volume: bool,
    limits_waiting: bool,
    voice_limit: u32,
    voice_context_count: u32,
    active_voices: u32,
    voice_note_count: u32,
    voice_contexts: [oscairn_voice_budget_context_t; OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS as usize],
    voice_cost: f64,
    voice_notes: Box<[oscairn_voice_budget_note_t; MAX_BLOCK_EVENTS]>,
    asset_dependencies: *const oscairn_plugin_asset_dependencies_t,
    asset_optional: bool,
    pub assets: Option<crate::bindings::oscairn_clap_assets>,
    window: *mut crate::bindings::kick_clap_window,
    hardware_epoch: AtomicU32,
    plugin_latency: *const clap_plugin_latency,
    timer_support: *const clap_plugin_timer_support,
    fd_support: *const clap_plugin_posix_fd_support,
    ports_activation: *const clap_plugin_audio_ports_activation,
    activate_while_processing: bool,
    wanted_outputs: AtomicU64,
    outputs_told: AtomicBool,
    applied_outputs: u64,
    applied_told: bool,
    memory: *const oscairn_plugin_memory_t,
    pub has_declared: bool,
    context: bool,
    pub declared: oscairn_memory_t,
    pub memory_room: u64,
    pub offline: bool,
    pub info: Descriptor,
    pub message: String,
    last_logged_error: String,
    pub audio_layout_locked: bool,
    pub param_info: Vec<Parameter>,
    pub metadata_version: u64,
    cookies: Vec<*mut c_void>,
    pub param_values: Box<[AtomicU64]>,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub note_inputs: Vec<clap_note_port_info>,
    pub note_outputs: Vec<clap_note_port_info>,
    input_buffers: Vec<clap_audio_buffer>,
    output_buffers: Vec<clap_audio_buffer>,
    input_pointers: Vec<*mut f32>,
    output_pointers: Vec<*mut f32>,
    scratch: Vec<f32>,
    pub ring_out_channels: u32,
    ring_out_buffer: clap_audio_buffer,
    sample_rate: f64,
    max_frames: u32,
    steady_time: i64,
    active: bool,
    pub sleeping: AtomicBool,
    wants_beat: AtomicBool,
    wants_harmony: AtomicBool,
    last_harmony: oscairn_harmony_t,
    have_harmony: bool,
    harmony_sequence: AtomicU32,
    harmony_words: [AtomicU64; 4],
    processing: AtomicBool,
    pub pending_commands: AtomicU32,
    transport: TransportWatch,
    wake_mirror: *const AtomicU32,
    pub gate: AtomicBool,
    pub loaded: AtomicBool,
    pub failed: AtomicBool,
    dirty: AtomicBool,
    pub parameter_reports: AtomicU64,
    restart: AtomicBool,
    callback: AtomicBool,
    pub wake: AtomicBool,
    pub flush: AtomicBool,
    pub panic: AtomicBool,
    rescan: AtomicU32,
    latency_frames: AtomicU32,
    pub blocks: AtomicU64,
    pub dropped: AtomicU64,
    pub errors: AtomicU64,
    pub last_us: AtomicU64,
    pub max_us: AtomicU64,
    pub total_us: AtomicU64,
    pub deadline_misses: AtomicU64,
    pub control_silences: AtomicU64,
    pub idle_blocks: AtomicU64,
    commands: Queue<Event>,
    emitted: Queue<Event>,
    pub output_drops: AtomicU64,
    ordered: Box<[Timed; RENDER_EVENTS]>,
    chunk_events: Box<[Event; RENDER_EVENTS]>,
    pub carry_events: Box<[Event; MAX_BLOCK_EVENTS]>,
    pub carry_count: u32,
    pub block_output: Box<[Event; MAX_BLOCK_EVENTS]>,
    block_output_count: u32,
    pub published_output_count: u32,
    pub reset_in_process: bool,
    sysex_output: Option<Box<[u8]>>,
    sysex_used: u32,
    in_process: bool,
    chunk_count: u32,
    output_frame_offset: u32,
    current_process_frames: u32,
    timers: Vec<Timer>,
    next_timer: clap_id,
    fds: Vec<Fd>,
    gui: *const clap_plugin_gui,
    pub gui_api: *const c_char,
    gui_floating: bool,
    gui_created: AtomicBool,
    gui_show: AtomicBool,
    gui_hide: AtomicBool,
    gui_gone: AtomicBool,
    gui_hints: AtomicBool,
    gui_size: AtomicU64,
}

thread_local!(static AUDIO_SCOPE: Cell<*const Host> = const { Cell::new(null()) });
/// AudioScope: this thread acts as the plugin's audio thread until it is dropped.
struct AudioScope(*const Host);
impl AudioScope {
    fn new(owner: *const Host) -> AudioScope {
        AudioScope(AUDIO_SCOPE.with(|s| s.replace(owner)))
    }
}
impl Drop for AudioScope {
    fn drop(&mut self) {
        AUDIO_SCOPE.with(|s| s.set(self.0));
    }
}
/// Gate: control operations exclude rendering; the audio thread never waits for it.
pub struct Gate(*const AtomicBool);
impl Gate {
    /// # Safety
    /// flag outlives the gate.
    pub unsafe fn new(flag: *const AtomicBool) -> Gate {
        // SAFETY: the caller's.
        while unsafe { (*flag).swap(true, Acquire) } {
            std::thread::yield_now();
        }
        Gate(flag)
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        // SAFETY: Gate::new's.
        unsafe { (*self.0).store(false, Release) };
    }
}

fn f64_load(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Relaxed))
}
fn f64_store(a: &AtomicU64, v: f64) {
    a.store(v.to_bits(), Relaxed)
}
/// A fixed-size C string field up to its first NUL, as std::string(p, strnlen(p, n)) takes it.
fn field(p: &[c_char]) -> CString {
    let bytes: Vec<u8> = p.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    CString::new(bytes).unwrap_or_default()
}
fn text(p: *const c_char) -> CString {
    if p.is_null() {
        return CString::default(); // safeString's empty
    }
    // SAFETY: a plugin's NUL-terminated string.
    unsafe { CStr::from_ptr(p) }.to_owned()
}
fn same_id(a: *const c_char, b: &CStr) -> bool {
    // SAFETY: NUL-terminated strings, or null.
    !a.is_null() && unsafe { CStr::from_ptr(a) } == b
}

pub type R<T> = Result<T, String>;

/// A failure as clap_host.h passes it: the kind of C++ exception the reference throws, and its message.
pub struct Failure {
    pub kind: u32,
    pub message: String,
}
impl Failure {
    pub fn runtime(message: impl Into<String>) -> Failure {
        Failure { kind: crate::bindings::OSCAIRN_CLAP_RUNTIME_ERROR, message: message.into() }
    }
    pub fn invalid(message: impl Into<String>) -> Failure {
        Failure { kind: crate::bindings::OSCAIRN_CLAP_INVALID_ARGUMENT, message: message.into() }
    }
}
impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure::runtime(message)
    }
}

impl Host {
    pub fn new() -> Box<Host> {
        static NAME: &CStr = c"Oscairn";
        static URL: &CStr = c"https://github.com/oscairn/oscairn";
        // OSCAIRN_RELEASE, as the C++ host is built with it (CMake gives cargo the same).
        static VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
        let version = VERSION.get_or_init(|| CString::new(option_env!("OSCAIRN_RELEASE").unwrap_or("0.0.0")).unwrap_or_default());
        // SAFETY: plain old data and pointers, zeros a valid start (as the C++ value-initializes).
        let mut h: Box<Host> = Box::new(Host {
            host: clap_host {
                clap_version: CLAP_VERSION,
                host_data: null_mut(),
                name: NAME.as_ptr(),
                vendor: NAME.as_ptr(),
                url: URL.as_ptr(),
                version: version.as_ptr(),
                get_extension: Some(extension),
                request_restart: Some(request_restart),
                request_process: Some(request_process),
                request_callback: Some(request_callback),
            },
            main_thread: Cell::new(None),
            library: None,
            plugin: null(),
            params: null(),
            ui: null(),
            ui_views: false,
            ui_seconds: false,
            state: null(),
            exclusive_state: false,
            sounds: null(),
            sound_snapshots: null(),
            voice_budget: null(),
            honors_volume: false,
            limits_waiting: false,
            voice_limit: 0,
            voice_context_count: 0,
            active_voices: 0,
            voice_note_count: 0,
            // SAFETY: plain old data.
            voice_contexts: unsafe { std::mem::zeroed() },
            voice_cost: 0.0,
            // SAFETY: plain old data.
            voice_notes: Box::new(unsafe { std::mem::zeroed() }),
            asset_dependencies: null(),
            asset_optional: false,
            assets: None,
            window: null_mut(),
            hardware_epoch: AtomicU32::new(0),
            plugin_latency: null(),
            timer_support: null(),
            fd_support: null(),
            ports_activation: null(),
            activate_while_processing: false,
            wanted_outputs: AtomicU64::new(0),
            outputs_told: AtomicBool::new(false),
            applied_outputs: 0,
            applied_told: false,
            memory: null(),
            has_declared: false,
            context: false,
            declared: oscairn_memory_t { shared_bytes: 0, context_bytes: 0, asset_bytes: 0 },
            memory_room: 0,
            offline: false,
            info: Descriptor::default(),
            message: String::new(),
            last_logged_error: String::new(),
            audio_layout_locked: false,
            param_info: Vec::new(),
            metadata_version: 0,
            cookies: Vec::new(),
            param_values: Box::new([]),
            inputs: Vec::new(),
            outputs: Vec::new(),
            note_inputs: Vec::new(),
            note_outputs: Vec::new(),
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
            input_pointers: Vec::new(),
            output_pointers: Vec::new(),
            scratch: Vec::new(),
            ring_out_channels: 0,
            // SAFETY: plain old data.
            ring_out_buffer: unsafe { std::mem::zeroed() },
            sample_rate: 48000.0,
            max_frames: 512,
            steady_time: 0,
            active: false,
            sleeping: AtomicBool::new(false),
            wants_beat: AtomicBool::new(false),
            wants_harmony: AtomicBool::new(false),
            // SAFETY: plain old data.
            last_harmony: unsafe { std::mem::zeroed() },
            have_harmony: false,
            harmony_sequence: AtomicU32::new(0),
            harmony_words: Default::default(),
            processing: AtomicBool::new(false),
            pending_commands: AtomicU32::new(0),
            transport: TransportWatch::default(),
            wake_mirror: null(),
            gate: AtomicBool::new(false),
            loaded: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            dirty: AtomicBool::new(false),
            parameter_reports: AtomicU64::new(0),
            restart: AtomicBool::new(false),
            callback: AtomicBool::new(false),
            wake: AtomicBool::new(false),
            flush: AtomicBool::new(false),
            panic: AtomicBool::new(false),
            rescan: AtomicU32::new(0),
            latency_frames: AtomicU32::new(0),
            blocks: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            last_us: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            control_silences: AtomicU64::new(0),
            idle_blocks: AtomicU64::new(0),
            commands: Queue::new(MAX_EVENTS),
            emitted: Queue::new(MAX_EVENTS),
            output_drops: AtomicU64::new(0),
            ordered: Box::new([Timed::default(); RENDER_EVENTS]),
            chunk_events: Box::new([Event::default(); RENDER_EVENTS]),
            carry_events: Box::new([Event::default(); MAX_BLOCK_EVENTS]),
            carry_count: 0,
            block_output: Box::new([Event::default(); MAX_BLOCK_EVENTS]),
            block_output_count: 0,
            published_output_count: 0,
            reset_in_process: false,
            sysex_output: None,
            sysex_used: 0,
            in_process: false,
            chunk_count: 0,
            output_frame_offset: 0,
            current_process_frames: 1,
            timers: Vec::new(),
            next_timer: 1,
            fds: Vec::new(),
            gui: null(),
            gui_api: null(),
            gui_floating: false,
            gui_created: AtomicBool::new(false),
            gui_show: AtomicBool::new(false),
            gui_hide: AtomicBool::new(false),
            gui_gone: AtomicBool::new(false),
            gui_hints: AtomicBool::new(false),
            gui_size: AtomicU64::new(0),
        });
        h.host.host_data = std::ptr::from_mut::<Host>(&mut h).cast();
        h
    }
}

/// The host from the plugin's clap_host.
///
/// # Safety
/// h is a Host's own clap_host.
unsafe fn me(h: *const clap_host) -> *mut Host {
    // SAFETY: the caller's.
    unsafe { (*h).host_data.cast() }
}

/// # Safety
/// Every function below taking `s: *mut Host` is called with a live Host, on the thread Host.h names for its C++
/// counterpart; none keeps a reference into the Host across a call into the plugin.
pub unsafe fn main(s: *mut Host) -> bool {
    // SAFETY: see above.
    unsafe {
        let me = std::thread::current().id();
        if (*s).main_thread.get().is_none() {
            (*s).main_thread.set(Some(me));
        }
        if (*s).main_thread.get() == Some(me) {
            return true;
        }
        (*s).message = "CLAP control operation must run on its main thread".into();
        false
    }
}
fn is_main_of(s: *const Host) -> bool {
    // SAFETY: a live host.
    unsafe { (*s).main_thread.get() == Some(std::thread::current().id()) }
}

// ---- The host's callbacks and extensions ----
unsafe extern "C" fn is_main(h: *const clap_host) -> bool {
    // SAFETY: the plugin's own host.
    is_main_of(unsafe { me(h) })
}
unsafe extern "C" fn is_audio(h: *const clap_host) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    AUDIO_SCOPE.with(|a| a.get() == s.cast_const())
}
unsafe extern "C" fn request_restart(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { (*me(h)).restart.store(true, SeqCst) }
}
unsafe extern "C" fn request_callback(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { (*me(h)).callback.store(true, SeqCst) }
}
unsafe extern "C" fn request_process(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { wake_up(me(h)) }
}
unsafe extern "C" fn request_flush(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe {
        (*me(h)).flush.store(true, SeqCst);
        wake_up(me(h));
    }
}
/// # Safety
/// s is a live host.
pub unsafe fn wake_up(s: *mut Host) {
    // SAFETY: the caller's; the mirror outlives its setting (Host.h).
    unsafe {
        (*s).wake.store(true, SeqCst);
        if !(*s).wake_mirror.is_null() {
            (*(*s).wake_mirror).store(1, Release);
        }
    }
}
unsafe extern "C" fn rescan_params(h: *const clap_host, flags: clap_param_rescan_flags) {
    // SAFETY: as is_main.
    unsafe { (*me(h)).rescan.fetch_or(flags, SeqCst) };
}
unsafe extern "C" fn clear_params(_: *const clap_host, _: clap_id, _: clap_param_clear_flags) {}
unsafe extern "C" fn mark_dirty(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { (*me(h)).dirty.store(true, SeqCst) }
}
unsafe extern "C" fn tail_changed(_: *const clap_host) {}
unsafe extern "C" fn supported_audio_rescan(_: *const clap_host, flags: u32) -> bool {
    flags
        & !(CLAP_AUDIO_PORTS_RESCAN_NAMES
            | CLAP_AUDIO_PORTS_RESCAN_FLAGS
            | CLAP_AUDIO_PORTS_RESCAN_CHANNEL_COUNT
            | CLAP_AUDIO_PORTS_RESCAN_PORT_TYPE
            | CLAP_AUDIO_PORTS_RESCAN_IN_PLACE_PAIR
            | CLAP_AUDIO_PORTS_RESCAN_LIST)
        == 0
}
unsafe extern "C" fn restart_on(h: *const clap_host, _: u32) {
    // SAFETY: as is_main.
    unsafe { request_restart(h) }
}
unsafe extern "C" fn dialects(_: *const clap_host) -> u32 {
    CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE
}
unsafe extern "C" fn log(h: *const clap_host, severity: clap_log_severity, text: *const c_char) {
    // Logging from audio callbacks must not format strings, write files, or lock.
    // SAFETY: as is_main; the text is the plugin's NUL-terminated string.
    unsafe {
        let s = me(h);
        if severity == CLAP_LOG_FATAL || severity == CLAP_LOG_PLUGIN_MISBEHAVING {
            (*s).errors.fetch_add(1, SeqCst);
        }
        if severity == CLAP_LOG_FATAL {
            (*s).failed.store(true, SeqCst);
        }
        if severity == CLAP_LOG_ERROR && !text.is_null() && is_main_of(s) {
            let bytes = std::slice::from_raw_parts(text.cast::<u8>(), (0..512).take_while(|&i| *text.add(i) != 0).count());
            (*s).last_logged_error = String::from_utf8_lossy(bytes).into_owned();
        }
    }
}
unsafe extern "C" fn query_event_space(h: *const clap_host, name: *const c_char, space: *mut u16) -> bool {
    if name.is_null() || space.is_null() {
        return false;
    }
    // SAFETY: as is_main; name and space the plugin's.
    unsafe {
        let s = me(h);
        let name = CStr::from_ptr(name);
        let given = if name == ids::OSCAIRN_EVENT_SPACE_BEAT {
            (*s).wants_beat.store(true, SeqCst);
            BEAT_SPACE
        } else if name == ids::OSCAIRN_EVENT_SPACE_HARMONY {
            (*s).wants_harmony.store(true, SeqCst);
            HARMONY_SPACE
        } else if name == ids::OSCAIRN_EVENT_SPACE_PARAM_TARGET {
            PARAM_TARGET_SPACE
        } else {
            return false;
        };
        *space = given;
        true
    }
}
unsafe extern "C" fn register_timer(h: *const clap_host, ms: u32, id: *mut clap_id) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if !is_main_of(s) || id.is_null() || (*s).timers.len() >= 128 {
            return false;
        }
        *id = (*s).next_timer;
        (*s).next_timer += 1;
        let period = ms.max(1);
        (*s).timers.push(Timer { id: *id, period, next: Instant::now() + Duration::from_millis(period.into()) });
        true
    }
}
unsafe extern "C" fn unregister_timer(h: *const clap_host, id: clap_id) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if !is_main_of(s) {
            return false;
        }
        let before = (*s).timers.len();
        (*s).timers.retain(|t| t.id != id);
        before != (*s).timers.len()
    }
}
unsafe extern "C" fn register_fd(h: *const clap_host, fd: i32, flags: clap_posix_fd_flags) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if !is_main_of(s) || fd < 0 || (*s).fds.len() >= 128 || flags & !(CLAP_POSIX_FD_READ | CLAP_POSIX_FD_WRITE | CLAP_POSIX_FD_ERROR) != 0 {
            return false;
        }
        if (*s).fds.iter().any(|f| f.fd == fd) {
            return false;
        }
        (*s).fds.push(Fd { fd, flags });
        true
    }
}
unsafe extern "C" fn modify_fd(h: *const clap_host, fd: i32, flags: clap_posix_fd_flags) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if !is_main_of(s) {
            return false;
        }
        match (*s).fds.iter_mut().find(|f| f.fd == fd) {
            Some(item) => {
                item.flags = flags;
                true
            }
            None => false,
        }
    }
}
unsafe extern "C" fn unregister_fd(h: *const clap_host, fd: i32) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if !is_main_of(s) {
            return false;
        }
        let before = (*s).fds.len();
        (*s).fds.retain(|f| f.fd != fd);
        before != (*s).fds.len()
    }
}
/// oscairn.assets' failures, written into the plugin's buffer as snprintf would.
unsafe fn report(message: *mut c_char, capacity: u32, text: &str) -> bool {
    if !message.is_null() && capacity > 0 {
        let n = text.len().min(capacity as usize - 1);
        // SAFETY: the plugin's buffer of capacity bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), message.cast::<u8>(), n);
            *message.add(n) = 0;
        }
    }
    false
}
// oscairn.assets: the thread and the plugin's arguments checked here, the rest the resolver's (clap_host.h's table).
unsafe extern "C" fn resolve_asset(
    h: *const clap_host,
    r: *mut oscairn_asset_ref_t,
    path: *mut c_char,
    capacity: u32,
    message: *mut c_char,
    size: u32,
) -> bool {
    // SAFETY: as is_main; the plugin's buffers.
    unsafe {
        let s = me(h);
        if !is_main_of(s) {
            return report(message, size, "assets resolve on the main thread only");
        }
        if r.is_null() || path.is_null() || capacity == 0 {
            return report(message, size, "invalid asset request");
        }
        match (*s).assets {
            Some(a) => a.resolve.unwrap()(a.context, r, path, capacity, message, size),
            None => report(message, size, "this host has no sound asset store"),
        }
    }
}
unsafe extern "C" fn create_asset(h: *const clap_host, file: *const c_char, r: *mut oscairn_asset_ref_t, message: *mut c_char, size: u32) -> bool {
    // SAFETY: as resolve_asset.
    unsafe {
        let s = me(h);
        if !is_main_of(s) {
            return report(message, size, "assets are created on the main thread only");
        }
        if file.is_null() || r.is_null() {
            return report(message, size, "invalid asset request");
        }
        match (*s).assets {
            Some(a) => a.create.unwrap()(a.context, file, r, message, size),
            None => report(message, size, "this host has no sound asset store"),
        }
    }
}
unsafe extern "C" fn resolve_sample(
    h: *const clap_host,
    r: *const oscairn_asset_ref_t,
    path: *mut c_char,
    capacity: u32,
    location: *mut oscairn_sample_location_t,
    message: *mut c_char,
    size: u32,
) -> bool {
    // SAFETY: as resolve_asset.
    unsafe {
        let s = me(h);
        if !is_main_of(s) {
            return report(message, size, "samples resolve on the main thread only");
        }
        if r.is_null() || path.is_null() || capacity == 0 || location.is_null() {
            return report(message, size, "invalid sample request");
        }
        match (*s).assets {
            Some(a) => a.resolve_sample.unwrap()(a.context, r, path, capacity, location, message, size),
            None => report(message, size, "this host has no sample store"),
        }
    }
}
unsafe extern "C" fn library_root(owner: *const c_void, library: *const c_char, min: *const c_char, path: *mut c_char, capacity: u32) -> bool {
    let h = owner.cast::<clap_host>();
    // SAFETY: the owner is the host the plugin was given (oscairn.assets draft 2).
    unsafe {
        if h.is_null() || !is_main(h) || library.is_null() || path.is_null() || capacity == 0 {
            return false;
        }
        (*me(h)).assets.is_some_and(|a| a.library_root.unwrap()(a.context, library, min, path, capacity))
    }
}
unsafe extern "C" fn preset_error(h: *const clap_host, _: u32, _: *const c_char, _: *const c_char, _: i32, message: *const c_char) {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        (*s).message = if message.is_null() { "Preset not loaded".into() } else { format!("Preset not loaded: {}", CStr::from_ptr(message).to_string_lossy()) };
    }
}
unsafe extern "C" fn preset_loaded(h: *const clap_host, _: u32, _: *const c_char, _: *const c_char) {
    // SAFETY: as is_main.
    unsafe { (*me(h)).message.clear() }
}
unsafe extern "C" fn gui_hints_changed(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { (*me(h)).gui_hints.store(true, SeqCst) }
}
unsafe extern "C" fn gui_request_resize(h: *const clap_host, width: u32, height: u32) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if width == 0 || height == 0 || !(*s).gui_created.load(SeqCst) || (*s).gui_floating {
            return false;
        }
        if !is_main_of(s) {
            (*s).gui_size.store((u64::from(width) << 32) | u64::from(height), SeqCst);
            return true;
        }
        if (*s).window.is_null() {
            return false;
        }
        bridge::kick_clap_window_size((*s).window, width, height);
        true
    }
}
unsafe extern "C" fn gui_request_show(h: *const clap_host) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if !(*s).gui_created.load(SeqCst) {
            return false;
        }
        (*s).gui_show.store(true, SeqCst);
        true
    }
}
unsafe extern "C" fn gui_request_hide(h: *const clap_host) -> bool {
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        if !(*s).gui_created.load(SeqCst) {
            return false;
        }
        (*s).gui_hide.store(true, SeqCst);
        true
    }
}
unsafe extern "C" fn gui_closed(h: *const clap_host, destroyed: bool) {
    if destroyed {
        // SAFETY: as is_main.
        unsafe { (*me(h)).gui_gone.store(true, SeqCst) }
    }
}
unsafe extern "C" fn read_harmony(h: *const clap_host, out: *mut oscairn_harmony_t) -> bool {
    if out.is_null() {
        return false;
    }
    // SAFETY: as is_main.
    unsafe {
        let s = me(h);
        let mut words = [0u64; 4];
        loop {
            let before = (*s).harmony_sequence.load(Acquire);
            for (w, a) in words.iter_mut().zip(&(*s).harmony_words) {
                *w = a.load(Relaxed);
            }
            std::sync::atomic::fence(Acquire);
            let after = (*s).harmony_sequence.load(Relaxed);
            if before & 1 == 0 && before == after {
                break;
            }
        }
        if words[3] == 0 {
            return false;
        }
        std::ptr::copy_nonoverlapping(words.as_ptr().cast::<u8>(), out.cast::<u8>(), size_of::<oscairn_harmony_t>());
        true
    }
}
use std::mem::size_of;
/// # Safety
/// s is a live host; null: in no part.
unsafe fn publish_harmony(s: *mut Host, h: Option<&oscairn_harmony_t>) {
    let mut words = [0u64; 4];
    if let Some(h) = h {
        // SAFETY: a harmony is three words.
        unsafe { std::ptr::copy_nonoverlapping(std::ptr::from_ref(h).cast::<u8>(), words.as_mut_ptr().cast::<u8>(), size_of::<oscairn_harmony_t>()) };
        words[3] = 1;
    }
    // SAFETY: the caller's.
    unsafe {
        let n = (*s).harmony_sequence.load(Relaxed);
        (*s).harmony_sequence.store(n + 1, Relaxed);
        std::sync::atomic::fence(Release);
        for (a, w) in (*s).harmony_words.iter().zip(words) {
            a.store(w, Relaxed);
        }
        (*s).harmony_sequence.store(n + 2, Release);
    }
}
// oscairn.hardware: the process's, through kick_clap (bridge.h), with this host as the owner of its plugin's leases.
use crate::bindings as bridge;
unsafe extern "C" fn hardware_count(_: *const clap_host) -> u32 {
    // SAFETY: a plain call.
    unsafe { bridge::kick_clap_hardware_count() }
}
unsafe extern "C" fn hardware_get(_: *const clap_host, index: u32, out: *mut oscairn_hardware_accelerator_t) -> bool {
    // SAFETY: the plugin's pointer, checked there.
    unsafe { bridge::kick_clap_hardware_get(index, out) }
}
unsafe extern "C" fn hardware_supports(_: *const clap_host, index: u32, operation: *const c_char) -> bool {
    // SAFETY: as hardware_get.
    unsafe { bridge::kick_clap_hardware_supports(index, operation) }
}
unsafe extern "C" fn hardware_reserve(
    h: *const clap_host,
    index: u32,
    request: *const oscairn_hardware_request_t,
    lease: *mut oscairn_hardware_lease_t,
) -> i32 {
    // SAFETY: as hardware_get.
    unsafe { bridge::kick_clap_hardware_reserve(me(h).cast(), index, request, lease) }
}
unsafe extern "C" fn hardware_release(h: *const clap_host, lease: u32) {
    // SAFETY: as hardware_get.
    unsafe { bridge::kick_clap_hardware_release(me(h).cast(), lease) }
}
unsafe extern "C" fn hardware_submit(h: *const clap_host, lease: u32, job: *const oscairn_hardware_job_t) -> i32 {
    // SAFETY: as hardware_get.
    unsafe { bridge::kick_clap_hardware_submit(me(h).cast(), (*me(h)).hardware_epoch.load(Relaxed), lease, job) }
}
unsafe extern "C" fn hardware_collect(h: *const clap_host, lease: u32, result: *mut oscairn_hardware_result_t) -> i32 {
    // SAFETY: as hardware_get.
    unsafe { bridge::kick_clap_hardware_collect(me(h).cast(), (*me(h)).hardware_epoch.load(Relaxed), lease, result) }
}

struct Table<T>(T);
// SAFETY: tables of function pointers, never written.
unsafe impl<T> Sync for Table<T> {}
static THREADS: Table<clap_host_thread_check> = Table(clap_host_thread_check { is_main_thread: Some(is_main), is_audio_thread: Some(is_audio) });
static PARAMETERS: Table<clap_host_params> =
    Table(clap_host_params { rescan: Some(rescan_params), clear: Some(clear_params), request_flush: Some(request_flush) });
static STATE: Table<clap_host_state> = Table(clap_host_state { mark_dirty: Some(mark_dirty) });
static LATENCY: Table<clap_host_latency> = Table(clap_host_latency { changed: Some(request_restart) });
static TAIL: Table<clap_host_tail> = Table(clap_host_tail { changed: Some(tail_changed) });
static AUDIO: Table<clap_host_audio_ports> = Table(clap_host_audio_ports { is_rescan_flag_supported: Some(supported_audio_rescan), rescan: Some(restart_on) });
static NOTES: Table<clap_host_note_ports> = Table(clap_host_note_ports { supported_dialects: Some(dialects), rescan: Some(restart_on) });
static LOGGING: Table<clap_host_log> = Table(clap_host_log { log: Some(log) });
static REGISTRY: Table<clap_host_event_registry> = Table(clap_host_event_registry { query: Some(query_event_space) });
static TIMER: Table<clap_host_timer_support> =
    Table(clap_host_timer_support { register_timer: Some(register_timer), unregister_timer: Some(unregister_timer) });
static DESCRIPTORS: Table<clap_host_posix_fd_support> =
    Table(clap_host_posix_fd_support { register_fd: Some(register_fd), modify_fd: Some(modify_fd), unregister_fd: Some(unregister_fd) });
static ASSETS: Table<oscairn_host_assets_t> = Table(oscairn_host_assets_t {
    resolve: Some(resolve_asset),
    library_root: Some(library_root),
    create: Some(create_asset),
    resolve_sample: Some(resolve_sample),
});
static PRESET_LOAD: Table<clap_host_preset_load> = Table(clap_host_preset_load { on_error: Some(preset_error), loaded: Some(preset_loaded) });
static WINDOWS: Table<clap_host_gui> = Table(clap_host_gui {
    resize_hints_changed: Some(gui_hints_changed),
    request_resize: Some(gui_request_resize),
    request_show: Some(gui_request_show),
    request_hide: Some(gui_request_hide),
    closed: Some(gui_closed),
});
static HARMONY: Table<oscairn_host_harmony_t> = Table(oscairn_host_harmony_t { get: Some(read_harmony) });
static ACCELERATORS: Table<oscairn_host_hardware_t> = Table(oscairn_host_hardware_t {
    count: Some(hardware_count),
    get: Some(hardware_get),
    supports: Some(hardware_supports),
    reserve: Some(hardware_reserve),
    release: Some(hardware_release),
    submit: Some(hardware_submit),
    collect: Some(hardware_collect),
});

unsafe extern "C" fn extension(_: *const clap_host, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return null();
    }
    // SAFETY: the plugin's NUL-terminated ID.
    let id = unsafe { CStr::from_ptr(id) };
    fn p<T>(t: &'static Table<T>) -> *const c_void {
        std::ptr::from_ref(&t.0).cast()
    }
    if id == CLAP_EXT_GUI {
        p(&WINDOWS)
    } else if id == ids::OSCAIRN_EXT_ASSETS
        || id == ids::OSCAIRN_EXT_ASSETS_DRAFT_3
        || id == ids::OSCAIRN_EXT_ASSETS_DRAFT_2
        || id == ids::OSCAIRN_EXT_ASSETS_DRAFT_1
    {
        p(&ASSETS) // earlier drafts are draft 4's first members
    } else if id == ids::OSCAIRN_EXT_HARMONY {
        p(&HARMONY)
    } else if id == CLAP_EXT_PRESET_LOAD || id == CLAP_EXT_PRESET_LOAD_COMPAT {
        p(&PRESET_LOAD)
    } else if id == CLAP_EXT_THREAD_CHECK {
        p(&THREADS)
    } else if id == CLAP_EXT_PARAMS {
        p(&PARAMETERS)
    } else if id == CLAP_EXT_STATE {
        p(&STATE)
    } else if id == CLAP_EXT_LATENCY {
        p(&LATENCY)
    } else if id == CLAP_EXT_TAIL {
        p(&TAIL)
    } else if id == CLAP_EXT_AUDIO_PORTS {
        p(&AUDIO)
    } else if id == CLAP_EXT_NOTE_PORTS {
        p(&NOTES)
    } else if id == CLAP_EXT_LOG {
        p(&LOGGING)
    } else if id == CLAP_EXT_EVENT_REGISTRY {
        p(&REGISTRY)
    } else if id == CLAP_EXT_TIMER_SUPPORT {
        p(&TIMER)
    } else if id == ids::OSCAIRN_EXT_HARDWARE {
        p(&ACCELERATORS)
    } else if id == CLAP_EXT_POSIX_FD_SUPPORT {
        p(&DESCRIPTORS)
    } else {
        null()
    }
}

/// The plugin's extension table of type T, or null.
///
/// # Safety
/// s has a live plugin.
unsafe fn ext<T>(s: *mut Host, id: &CStr) -> *const T {
    // SAFETY: the caller's; get_extension checked at initialization or before.
    unsafe { ((*(*s).plugin).get_extension.unwrap())((*s).plugin, id.as_ptr()).cast() }
}

// ---- Loading, activation, the description ----

/// # Safety
/// As main.
pub unsafe fn param_index(s: *const Host, id: clap_id) -> usize {
    // SAFETY: the caller's.
    let info = unsafe { &(*s).param_info };
    info.binary_search_by(|p| p.id.cmp(&id)).unwrap_or(info.len())
}

/// # Safety
/// As main; also run by process (the audio scope).
pub unsafe fn stop(s: *mut Host) {
    let _scope = AudioScope::new(s);
    // SAFETY: the caller's.
    unsafe {
        if (*s).processing.load(SeqCst) {
            ((*(*s).plugin).stop_processing.unwrap())((*s).plugin);
        }
        (*s).processing.store(false, SeqCst);
        (*s).sleeping.store(false, SeqCst);
    }
}
/// # Safety
/// As main.
pub unsafe fn deactivate(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        stop(s);
        if (*s).active {
            ((*(*s).plugin).deactivate.unwrap())((*s).plugin);
        }
        (*s).active = false;
    }
}
/// # Safety
/// As main, under the gate.
pub unsafe fn close(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        (*s).audio_layout_locked = false;
        (*s).loaded.store(false, SeqCst);
        if !(*s).plugin.is_null() {
            destroy_gui(s);
        }
        (*s).gui = null();
        (*s).gui_api = null();
        if !(*s).plugin.is_null() {
            deactivate(s);
            if let Some(destroy) = (*(*s).plugin).destroy {
                destroy((*s).plugin);
            }
        }
        bridge::kick_clap_hardware_release_all(s.cast()); // what the plugin did not release
        (*s).plugin = null();
        (*s).params = null();
        (*s).state = null();
        (*s).sounds = null();
        (*s).asset_dependencies = null();
        (*s).plugin_latency = null();
        (*s).timer_support = null();
        (*s).fd_support = null();
        (*s).memory = null();
        (*s).has_declared = false;
        (*s).ports_activation = null();
        (*s).timers.clear();
        (*s).fds.clear();
        (*s).param_info.clear();
        (*s).param_values = Box::new([]);
        (*s).cookies.clear();
        (*s).inputs.clear();
        (*s).outputs.clear();
        (*s).note_inputs.clear();
        (*s).note_outputs.clear();
        (*s).input_buffers.clear();
        (*s).output_buffers.clear();
        (*s).input_pointers.clear();
        (*s).output_pointers.clear();
        (*s).scratch.clear();
        (*s).library = None;
        while (*s).commands.pop().is_some() {
            (*s).pending_commands.fetch_sub(1, SeqCst);
        }
        while (*s).emitted.pop().is_some() {}
        for f in [&(*s).failed, &(*s).dirty, &(*s).restart, &(*s).callback, &(*s).wake, &(*s).flush, &(*s).panic] {
            f.store(false, SeqCst);
        }
        (*s).rescan.store(0, SeqCst);
        (*s).latency_frames.store(0, SeqCst);
        (*s).steady_time = 0;
        (*s).transport.reset();
        (*s).carry_count = 0;
        (*s).metadata_version += 1;
    }
}

/// Host::load after its gate: the library opened, the plugin made and initialized.
///
/// # Safety
/// As main, under the gate, the host closed.
pub unsafe fn load(s: *mut Host, path: &str, id: &CStr, rate: f64, frames: u32) -> R<()> {
    // SAFETY: the caller's.
    unsafe {
        let library = library::open(path)?;
        (*s).library = Some(library.clone());
        let factory = library.factory().ok_or("No CLAP plugin factory")?;
        let mut chosen = null();
        for i in 0..factory.get_plugin_count.unwrap()(factory).min(4096) {
            let d = factory.get_plugin_descriptor.unwrap()(factory, i);
            if !d.is_null() && !(*d).id.is_null() && !(*d).name.is_null() && same_id((*d).id, id) && clap_version_is_compatible((*d).clap_version) {
                chosen = d;
                break;
            }
        }
        if chosen.is_null() {
            return Err("Saved plugin ID is absent from this binary".into());
        }
        (*s).info = describe(&library.path, &*chosen);
        (*s).plugin = factory.create_plugin.unwrap()(factory, &(*s).host, (*chosen).id);
        initialize(s, rate, frames)
    }
}

/// detail::describe.
pub fn describe(path: &str, d: &clap_sys::plugin::clap_plugin_descriptor) -> Descriptor {
    let mut features = Vec::new();
    if !d.features.is_null() {
        for i in 0..128 {
            // SAFETY: the plugin's NULL-terminated feature list.
            let f = unsafe { *d.features.add(i) };
            if f.is_null() {
                break;
            }
            features.push(text(f));
        }
    }
    Descriptor {
        path: CString::new(path).unwrap_or_default(),
        id: text(d.id),
        name: text(d.name),
        vendor: text(d.vendor),
        version: text(d.version),
        description: text(d.description),
        features,
        lanes: Vec::new(),
    }
}

/// # Safety
/// As load.
pub unsafe fn initialize(s: *mut Host, rate: f64, frames: u32) -> R<()> {
    // SAFETY: the caller's.
    unsafe {
        let p = (*s).plugin;
        if p.is_null() || (*p).init.is_none() || (*p).destroy.is_none() {
            return Err("Incomplete CLAP plugin interface".into());
        }
        if !((*p).init.unwrap())(p) {
            return Err("Plugin initialization failed".into());
        }
        // The rest only once initialized: plugins built on clap-helpers fill them in during init.
        let q = &*p;
        if q.activate.is_none()
            || q.deactivate.is_none()
            || q.start_processing.is_none()
            || q.stop_processing.is_none()
            || q.reset.is_none()
            || q.process.is_none()
            || q.get_extension.is_none()
            || q.on_main_thread.is_none()
        {
            return Err("Incomplete CLAP plugin interface".into());
        }
        (*s).params = ext(s, CLAP_EXT_PARAMS);
        (*s).ui = ext(s, ids::OSCAIRN_EXT_UI);
        (*s).ui_seconds = !(*s).ui.is_null();
        if (*s).ui.is_null() {
            (*s).ui = ext(s, ids::OSCAIRN_EXT_UI_DRAFT_4); // its prefix: no seconds()
        }
        (*s).ui_views = !(*s).ui.is_null();
        if (*s).ui.is_null() {
            (*s).ui = ext(s, ids::OSCAIRN_EXT_UI_DRAFT_3); // its prefix: no views
        }
        if !(*s).ui.is_null() && (*(*s).ui).param_hint.is_none() {
            (*s).ui = null();
        }
        if let Some(params) = (*s).params.as_ref() {
            if params.count.is_none() || params.get_info.is_none() || params.get_value.is_none() || params.flush.is_none() {
                return Err("Incomplete CLAP parameter interface".into());
            }
        }
        (*s).sounds = ext(s, ids::OSCAIRN_EXT_SOUND_CONTEXTS);
        if let Some(x) = (*s).sounds.as_ref() {
            if x.capacity.is_none() || x.get.is_none() || x.prepare.is_none() || x.select.is_none() || x.discard.is_none() || x.capacity.unwrap()(p) > 256 {
                return Err("Invalid sound-context interface".into());
            }
        }
        (*s).state = ext(s, CLAP_EXT_STATE);
        (*s).exclusive_state = (*s).sounds.as_ref().is_some_and(|x| !x.state.is_null());
        if (*s).exclusive_state {
            (*s).state = (*(*s).sounds).state;
            (*s).sound_snapshots = ext(s, ids::OSCAIRN_EXT_SOUND_SNAPSHOTS);
        }
        if let Some(x) = (*s).sound_snapshots.as_ref() {
            if x.prepare.is_none() || x.ring_out.is_none() != x.choke_retiring.is_none() {
                return Err("Invalid sound-snapshot interface".into());
            }
        }
        (*s).voice_budget = ext(s, ids::OSCAIRN_EXT_VOICE_BUDGET);
        if let Some(x) = (*s).voice_budget.as_ref() {
            if x.honors_volume.is_none() || x.set_limits.is_none() || x.active_voices.is_none() || x.notes.is_none() {
                return Err("Invalid voice-budget interface".into());
            }
        }
        (*s).honors_volume = (*s).voice_budget.as_ref().is_some_and(|x| x.honors_volume.unwrap()(p));
        if let Some(x) = (*s).state.as_ref() {
            if x.save.is_none() || x.load.is_none() {
                return Err("Incomplete plugin state interface".into());
            }
        }
        (*s).asset_dependencies = ext(s, ids::OSCAIRN_EXT_ASSET_DEPENDENCIES);
        (*s).asset_optional = !(*s).asset_dependencies.is_null();
        if (*s).asset_dependencies.is_null() {
            (*s).asset_dependencies = ext(s, ids::OSCAIRN_EXT_ASSET_DEPENDENCIES_DRAFT_1);
            // its first two members only
        }
        if let Some(x) = (*s).asset_dependencies.as_ref() {
            if x.count.is_none() || x.get.is_none() {
                return Err("Incomplete asset-dependency interface".into());
            }
        }
        (*s).asset_optional = (*s).asset_optional && (*(*s).asset_dependencies).optional.is_some(); // draft 2 without optional: every asset required
        (*s).plugin_latency = ext(s, CLAP_EXT_LATENCY);
        (*s).timer_support = ext(s, CLAP_EXT_TIMER_SUPPORT);
        (*s).fd_support = ext(s, CLAP_EXT_POSIX_FD_SUPPORT);
        (*s).memory = ext(s, ids::OSCAIRN_EXT_MEMORY);
        if let Some(x) = (*s).memory.as_ref() {
            if x.estimate.is_none() || x.current.is_none() {
                return Err("Incomplete memory interface".into());
            }
        }
        (*s).ports_activation = ext(s, CLAP_EXT_AUDIO_PORTS_ACTIVATION);
        if (*s).ports_activation.is_null() {
            (*s).ports_activation = ext(s, CLAP_EXT_AUDIO_PORTS_ACTIVATION_COMPAT);
        }
        if (*s).ports_activation.as_ref().is_some_and(|x| x.set_active.is_none()) {
            (*s).ports_activation = null(); // incomplete: told nothing
        }
        (*s).activate_while_processing = (*s).ports_activation.as_ref().is_some_and(|x| x.can_activate_while_processing.is_some_and(|f| f(p)));
        (*s).applied_told = false;
        find_gui(s);
        // Initialized and not yet processing, on the main thread, as clap.render asks.
        let render: *const clap_plugin_render = ext(s, CLAP_EXT_RENDER);
        if let Some(r) = render.as_ref() {
            if let Some(set) = r.set {
                let hard = r.has_hard_realtime_requirement.is_some_and(|f| f(p));
                set(p, if (*s).offline && !hard { CLAP_RENDER_OFFLINE } else { CLAP_RENDER_REALTIME });
            }
        }
        (*s).sample_rate = rate;
        (*s).max_frames = frames;
        activate(s)?;
        (*s).loaded.store(true, SeqCst);
        (*s).dirty.store(false, SeqCst);
        Ok(())
    }
}

/// Embedded where the host has windows of its own (macOS, Cocoa), else floating where the plugin floats one.
///
/// # Safety
/// As main, a live plugin.
unsafe fn find_gui(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        (*s).gui = ext(s, CLAP_EXT_GUI);
        (*s).gui_api = null();
        (*s).gui_floating = false;
        let Some(g) = (*s).gui.as_ref() else { return };
        if g.is_api_supported.is_none() || g.create.is_none() || g.destroy.is_none() || g.show.is_none() || g.hide.is_none() {
            (*s).gui = null();
            return;
        }
        let p = (*s).plugin;
        let api = bridge::kick_clap_window_api();
        let window: Option<&CStr> = (!api.is_null()).then(|| CStr::from_ptr(api));
        let platform = if cfg!(target_os = "macos") {
            CLAP_WINDOW_API_COCOA
        } else if cfg!(windows) {
            CLAP_WINDOW_API_WIN32
        } else {
            CLAP_WINDOW_API_X11
        };
        if let Some(api) = window.filter(|api| g.get_size.is_some() && g.set_parent.is_some() && g.is_api_supported.unwrap()(p, api.as_ptr(), false)) {
            (*s).gui_api = api.as_ptr();
        } else if g.is_api_supported.unwrap()(p, platform.as_ptr(), true) {
            (*s).gui_api = platform.as_ptr();
            (*s).gui_floating = true;
        }
    }
}
/// Host.cpp's showGui: the editor made (embedded in a window of the host's own, or floating), titled and shown.
///
/// # Safety
/// As main; a plugin with a window here.
pub unsafe fn show_gui(s: *mut Host, title: &CStr) -> R<()> {
    unsafe extern "C" fn closing(c: *mut c_void) {
        // SAFETY: the window's host, alive while its window is.
        unsafe { (*c.cast::<Host>()).gui_gone.store(true, SeqCst) }
    }
    unsafe extern "C" fn resized(c: *mut c_void, w: *mut u32, h: *mut u32) {
        // SAFETY: as closing; the window's own sizes.
        unsafe {
            let s = c.cast::<Host>();
            let g = &*(*s).gui;
            if let Some(adjust) = g.adjust_size {
                adjust((*s).plugin, w, h);
            }
            if let Some(set) = g.set_size {
                set((*s).plugin, *w, *h);
            }
        }
    }
    // SAFETY: the caller's.
    unsafe {
        let g = &*(*s).gui;
        let p = (*s).plugin;
        if !(*s).gui_created.load(SeqCst) {
            bridge::kick_clap_start_windows();
            if !g.create.unwrap()(p, (*s).gui_api, (*s).gui_floating) {
                return Err("Plugin could not open its window".into());
            }
            (*s).gui_created.store(true, SeqCst);
            if !(*s).gui_floating {
                let (mut width, mut height) = (0, 0);
                if !g.get_size.unwrap()(p, &mut width, &mut height) || width == 0 || height == 0 {
                    (width, height) = (640, 480);
                }
                let resizable = g.can_resize.is_some_and(|f| f(p));
                (*s).window = bridge::kick_clap_window_create(title.as_ptr(), width, height, resizable, s.cast(), Some(closing), Some(resized));
                let parent = clap_window { api: (*s).gui_api, specific: clap_window_handle { ptr: bridge::kick_clap_window_view((*s).window) } };
                if !g.set_parent.unwrap()(p, &parent) {
                    return Err("Plugin could not open its window in the host's".into());
                }
            }
        }
        if !(*s).window.is_null() {
            bridge::kick_clap_window_title((*s).window, title.as_ptr());
        } else if let Some(suggest) = g.suggest_title {
            suggest(p, title.as_ptr());
        }
        // An embedded editor shows with its window; a floating one must show itself.
        if !g.show.unwrap()(p) && (*s).gui_floating {
            return Err("Plugin could not show its window".into());
        }
        if !(*s).window.is_null() {
            bridge::kick_clap_window_show((*s).window, true);
        }
        Ok(())
    }
}
/// # Safety
/// As main.
pub unsafe fn destroy_gui(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        if (*s).gui_created.swap(false, SeqCst) {
            ((*(*s).gui).destroy.unwrap())((*s).plugin);
        }
        if !(*s).window.is_null() {
            bridge::kick_clap_window_free((*s).window);
            (*s).window = null_mut();
        }
        for f in [&(*s).gui_show, &(*s).gui_hide, &(*s).gui_gone, &(*s).gui_hints] {
            f.store(false, SeqCst);
        }
        (*s).gui_size.store(0, SeqCst);
    }
}
/// # Safety
/// As main.
unsafe fn service_gui(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        if !(*s).gui_created.load(SeqCst) {
            return;
        }
        if (*s).gui_gone.load(SeqCst) {
            destroy_gui(s);
            return;
        }
        let g = &*(*s).gui;
        let w = (*s).window;
        let size = (*s).gui_size.swap(0, SeqCst);
        if size != 0 && !w.is_null() {
            bridge::kick_clap_window_size(w, (size >> 32) as u32, size as u32);
        }
        if (*s).gui_hints.swap(false, SeqCst) && !w.is_null() {
            if let Some(f) = g.can_resize {
                bridge::kick_clap_window_resizable(w, f((*s).plugin));
            }
        }
        if (*s).gui_hide.swap(false, SeqCst) {
            g.hide.unwrap()((*s).plugin);
            if !w.is_null() {
                bridge::kick_clap_window_show(w, false);
            }
        }
        if (*s).gui_show.swap(false, SeqCst) {
            g.show.unwrap()((*s).plugin);
            if !w.is_null() {
                bridge::kick_clap_window_show(w, true);
            }
        }
    }
}

/// # Safety
/// As main.
unsafe fn read_parameters(s: *mut Host) -> R<()> {
    struct Item {
        info: Parameter,
        cookie: *mut c_void,
        value: f64,
    }
    // SAFETY: the caller's.
    unsafe {
        let mut items = Vec::new();
        if let Some(params) = (*s).params.as_ref() {
            let p = (*s).plugin;
            let count = params.count.unwrap()(p);
            if count > 32768 {
                return Err("Plugin advertises too many parameters".into());
            }
            for i in 0..count {
                let mut q: clap_param_info = std::mem::zeroed();
                if !params.get_info.unwrap()(p, i, &mut q) || q.id == CLAP_INVALID_ID {
                    return Err(format!("Invalid CLAP parameter metadata: parameter {i} has no info"));
                }
                if !q.min_value.is_finite() || !q.max_value.is_finite() || q.min_value > q.max_value {
                    return Err(format!(
                        "Invalid CLAP parameter metadata: '{}' ({}) minimum {}, maximum {}",
                        field(&q.name).to_string_lossy(),
                        q.id,
                        cpp_to_string(q.min_value),
                        cpp_to_string(q.max_value)
                    ));
                }
                // A default outside the range only misplaces a reset, so it is brought into the range rather than
                // refusing the plugin (Airwindows Consolidated reports garbage defaults for the controls an effect does not use).
                q.default_value = if q.default_value.is_finite() { q.default_value.clamp(q.min_value, q.max_value) } else { q.min_value };
                let mut value = q.default_value;
                params.get_value.unwrap()(p, q.id, &mut value);
                if !value.is_finite() {
                    value = q.default_value;
                }
                items.push(Item {
                    info: Parameter {
                        id: q.id,
                        name: field(&q.name[..CLAP_NAME_SIZE]),
                        module: field(&q.module[..CLAP_PATH_SIZE]),
                        minimum: q.min_value,
                        maximum: q.max_value,
                        default_value: q.default_value,
                        flags: q.flags,
                        widget: 0,
                        priority: 0,
                        range_high: CLAP_INVALID_ID,
                        group: 0,
                        role: 0,
                        view: 0,
                        field: 0,
                        index: 0,
                        algorithms: Vec::new(),
                        seconds: Vec::new(),
                    },
                    cookie: q.cookie,
                    value,
                });
            }
        }
        items.sort_by_key(|i| i.info.id); // stable, as std::sort's order between equal IDs does not matter: they fail below
        (*s).param_info.clear();
        (*s).cookies.clear();
        (*s).param_values = items.iter().map(|_| AtomicU64::new(0)).collect();
        for (i, item) in items.into_iter().enumerate() {
            if i > 0 && (*s).param_info[i - 1].id == item.info.id {
                return Err("Duplicate CLAP parameter ID".into());
            }
            (*s).param_info.push(item.info);
            (*s).cookies.push(item.cookie);
            f64_store(&(*s).param_values[i], item.value);
        }
        let mut list = std::mem::take(&mut (*s).param_info);
        apply_hints(s, &mut list);
        (*s).param_info = list;
        (*s).metadata_version += 1;
        Ok(())
    }
}

/// std::to_string(double): "%f".
pub fn cpp_to_string(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    format!("{v:.6}")
}

/// Hints from oscairn.ui, each kept only where it fits its parameter (Host.cpp's applyHints).
///
/// # Safety
/// As main.
unsafe fn apply_hints(s: *mut Host, list: &mut [Parameter]) {
    // SAFETY: the caller's.
    unsafe {
        let Some(ui) = (*s).ui.as_ref() else { return };
        let p = (*s).plugin;
        let ids: Vec<clap_id> = list.iter().map(|q| q.id).collect();
        for q in list.iter_mut() {
            let mut hint = oscairn_param_hint_t {
                widget: OSCAIRN_WIDGET_AUTO,
                priority: 0,
                range_high: CLAP_INVALID_ID,
                group: 0,
                role: 0,
                view: 0,
                field: OSCAIRN_FIELD_NONE,
                index: 0,
            };
            if !ui.param_hint.unwrap()(p, q.id, &mut hint) {
                continue;
            }
            let stepped = q.flags & CLAP_PARAM_IS_STEPPED != 0;
            let values: i64 = if stepped { (q.maximum - q.minimum).round() as i64 + 1 } else { 0 };
            let high = ids.iter().any(|&o| o == hint.range_high && o != q.id);
            let w = hint.widget;
            let fits = w == OSCAIRN_WIDGET_SLIDER
                || w == OSCAIRN_WIDGET_KNOB
                || ((w == OSCAIRN_WIDGET_BUTTONS || w == OSCAIRN_WIDGET_LIST) && stepped)
                || (w == OSCAIRN_WIDGET_SWITCH && values == 2)
                || (w == OSCAIRN_WIDGET_RANGE && high);
            q.widget = if fits { w } else { OSCAIRN_WIDGET_AUTO };
            q.range_high = if q.widget == OSCAIRN_WIDGET_RANGE { hint.range_high } else { CLAP_INVALID_ID };
            q.priority = hint.priority;
            q.group = hint.group;
            q.role = if hint.role <= OSCAIRN_ROLE_OUTPUT_BUS { hint.role } else { 0 }; // the last role this host knows
            if !(*s).ui_views {
                continue;
            }
            let f = hint.field;
            let field = (OSCAIRN_FIELD_DELAY..=OSCAIRN_FIELD_RELEASE).contains(&f)
                || (OSCAIRN_FIELD_LEVEL_0..=OSCAIRN_FIELD_LEVEL_8).contains(&f)
                || (OSCAIRN_FIELD_TIME_1..=OSCAIRN_FIELD_RATE_8).contains(&f)
                || (OSCAIRN_FIELD_KEY_LOW..=OSCAIRN_FIELD_VELOCITY_CURVE).contains(&f)
                || (OSCAIRN_FIELD_ALGORITHM..=OSCAIRN_FIELD_FIXED).contains(&f);
            let in_view = hint.view != 0 && field && hint.index <= 8 && (f != OSCAIRN_FIELD_ALGORITHM || (stepped && (1..=128).contains(&values)));
            q.view = if in_view { hint.view } else { 0 };
            q.field = if in_view { f } else { 0 };
            q.index = if in_view { hint.index } else { 0 };
            read_seconds(s, q, values);
            let Some(algorithm) = ui.algorithm.filter(|_| q.field == OSCAIRN_FIELD_ALGORITHM) else { continue };
            // Its routings, all or none; operators past the eighth are dropped.
            for v in 0..values {
                let mut a: oscairn_fm_algorithm_t = std::mem::zeroed();
                if !algorithm(p, q.id, (q.minimum.round() as i64 + v) as u32, &mut a) {
                    q.algorithms.clear();
                    break;
                }
                let mut r = [0u8; 11];
                r[..8].copy_from_slice(&a.inputs);
                r[8] = a.carriers;
                r[9] = if a.feedback_from <= 8 { a.feedback_from } else { 0 };
                r[10] = if a.feedback_to <= 8 && r[9] != 0 { a.feedback_to } else { 0 };
                q.algorithms.push(r);
            }
        }
    }
}
/// A time or rate field's seconds (Host.cpp's readSeconds): from seconds() (draft 5), else from value texts.
///
/// # Safety
/// As main.
unsafe fn read_seconds(s: *mut Host, q: &mut Parameter, values: i64) {
    let f = q.field;
    if !(((OSCAIRN_FIELD_DELAY..=OSCAIRN_FIELD_RELEASE).contains(&f) && f != OSCAIRN_FIELD_SUSTAIN)
        || (OSCAIRN_FIELD_TIME_1..=OSCAIRN_FIELD_RATE_8).contains(&f))
    {
        return;
    }
    // SAFETY: the caller's.
    unsafe {
        let p = (*s).plugin;
        let asked = if (*s).ui_seconds { (*(*s).ui).seconds } else { None };
        let to_text = (*s).params.as_ref().and_then(|x| x.value_to_text);
        if asked.is_none() && to_text.is_none() {
            return;
        }
        let n = if (2..=128).contains(&values) { values } else { 33 };
        for k in 0..n {
            let value = q.minimum + (q.maximum - q.minimum) * k as f64 / (n - 1) as f64;
            let mut seconds = -1.0;
            if let Some(f) = asked {
                if !f(p, q.id, value, &mut seconds) {
                    seconds = -1.0;
                }
            } else {
                let mut text = [0 as c_char; 256];
                if to_text.unwrap()(p, q.id, value, text.as_mut_ptr(), 255) {
                    seconds = parse_seconds(&text);
                }
            }
            if !seconds.is_finite() || seconds < 0.0 {
                q.seconds.clear();
                return;
            }
            q.seconds.push(seconds);
        }
    }
}
/// strtod, then " ms", " s" or " sec" (spaces skipped), as readSeconds reads a value text; -1 where it cannot.
fn parse_seconds(text: &[c_char]) -> f64 {
    let bytes: Vec<u8> = text.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    let mut end = 0;
    let (mut best, mut value) = (0, 0.0);
    // The longest prefix strtod takes (after leading spaces), as C reads a number.
    let start = bytes.iter().take_while(|b| b.is_ascii_whitespace()).count();
    while end < bytes.len() {
        end += 1;
        if let Ok(v) = std::str::from_utf8(&bytes[start..end]).unwrap_or("").parse::<f64>() {
            if !bytes[start..end].iter().any(|b| b.is_ascii_alphabetic())
                || bytes[start..end].eq_ignore_ascii_case(b"inf")
                || bytes[start..end].eq_ignore_ascii_case(b"nan")
            {
                best = end;
                value = v;
            }
        }
    }
    if best == 0 {
        return -1.0;
    }
    let mut rest = &bytes[best..];
    while rest.first() == Some(&b' ') {
        rest = &rest[1..];
    }
    match rest {
        b"ms" => value / 1000.0,
        b"s" | b"sec" => value,
        _ => -1.0,
    }
}

/// CLAP_PARAM_RESCAN_INFO taken in place (Host.cpp's readParameterInfo); false when more changed than it may change.
///
/// # Safety
/// As main.
unsafe fn read_parameter_info(s: *mut Host) -> bool {
    // SAFETY: the caller's.
    unsafe {
        let Some(params) = (*s).params.as_ref() else { return false };
        let p = (*s).plugin;
        if params.count.unwrap()(p) as usize != (*s).param_info.len() {
            return false;
        }
        let mut next = (*s).param_info.clone();
        let changeable = CLAP_PARAM_IS_HIDDEN | CLAP_PARAM_IS_PERIODIC;
        for i in 0..next.len() as u32 {
            let mut q: clap_param_info = std::mem::zeroed();
            if !params.get_info.unwrap()(p, i, &mut q) {
                return false;
            }
            let at = param_index(s, q.id);
            if at == next.len() {
                return false;
            }
            let info = &mut next[at];
            info.name = field(&q.name);
            info.module = field(&q.module);
            info.flags = (info.flags & !changeable) | (q.flags & changeable);
            info.widget = OSCAIRN_WIDGET_AUTO;
            info.priority = 0;
            info.range_high = CLAP_INVALID_ID;
            info.group = 0;
            info.role = 0;
            info.view = 0;
            info.field = 0;
            info.index = 0;
            info.algorithms.clear();
            info.seconds.clear();
        }
        apply_hints(s, &mut next);
        let _gate = Gate::new(&(*s).gate);
        (*s).param_info = next;
        (*s).metadata_version += 1;
        true
    }
}

/// # Safety
/// As main.
unsafe fn read_ports(s: *mut Host) -> R<()> {
    // SAFETY: the caller's.
    unsafe {
        let p = (*s).plugin;
        let (mut next_inputs, mut next_outputs) = (Vec::new(), Vec::new());
        // A plugin without the audio-ports extension has no audio ports: normal for note effects.
        let audio: *const clap_plugin_audio_ports = ext(s, CLAP_EXT_AUDIO_PORTS);
        if let Some(a) = audio.as_ref() {
            if a.count.is_none() || a.get.is_none() {
                return Err("Incomplete CLAP audio-ports interface".into());
            }
        }
        for input in [true, false] {
            let ports: &mut Vec<Port> = if input { &mut next_inputs } else { &mut next_outputs };
            let count = audio.as_ref().map_or(0, |a| a.count.unwrap()(p, input));
            if count > MAX_AUDIO_CHANNELS {
                return Err("Audio bus count exceeds the 64-channel budget".into());
            }
            let mut channels = 0;
            for i in 0..count {
                let mut q: clap_audio_port_info = std::mem::zeroed();
                if !(*audio).get.unwrap()(p, i, input, &mut q)
                    || q.channel_count == 0
                    || q.channel_count > MAX_AUDIO_CHANNELS
                    || channels + q.channel_count > MAX_AUDIO_CHANNELS
                {
                    return Err("Invalid or unsupported audio channel layout".into());
                }
                ports.push(Port { id: q.id, name: field(&q.name), channels: q.channel_count, flags: q.flags });
                channels += q.channel_count;
            }
        }
        let same = |a: &[Port], b: &[Port]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.id == y.id && x.channels == y.channels);
        if (*s).audio_layout_locked && (!same(&(*s).inputs, &next_inputs) || !same(&(*s).outputs, &next_outputs)) {
            return Err("CLAP audio port layout changed; reload this graph node before reconnecting audio".into());
        }
        (*s).inputs = next_inputs;
        (*s).outputs = next_outputs;
        (*s).note_inputs.clear();
        (*s).note_outputs.clear();
        let notes: *const clap_plugin_note_ports = ext(s, CLAP_EXT_NOTE_PORTS);
        if let Some(n) = notes.as_ref().filter(|n| n.count.is_some() && n.get.is_some()) {
            for input in [true, false] {
                let count = n.count.unwrap()(p, input);
                if count > 16 {
                    return Err("More than 16 note ports are unsupported".into());
                }
                for i in 0..count {
                    let mut q: clap_note_port_info = std::mem::zeroed();
                    if !n.get.unwrap()(p, i, input, &mut q) {
                        return Err("Invalid CLAP note port".into());
                    }
                    if input { &mut (*s).note_inputs } else { &mut (*s).note_outputs }.push(q);
                }
            }
        }
        // A modulator may send only on parameter-target lanes (RFC 0002): those are its outputs.
        let lanes: *const oscairn_plugin_param_target_lanes_t = ext(s, ids::OSCAIRN_EXT_PARAM_TARGET_LANES);
        (*s).info.lanes.clear();
        if let Some(l) = lanes.as_ref().filter(|l| l.count.is_some() && l.get.is_some()) {
            for i in 0..l.count.unwrap()(p).min(64) {
                let mut lane: oscairn_param_target_lane_info_t = std::mem::zeroed();
                if l.get.unwrap()(p, i, &mut lane) {
                    (*s).info.lanes.push((lane.lane, field(&lane.name[..CLAP_NAME_SIZE])));
                }
            }
        }
        if (*s).outputs.is_empty() && (*s).note_outputs.is_empty() && (*s).info.lanes.is_empty() {
            return Err("Plugin has neither audio nor note outputs".into());
        }
        // Read while the plugin is not processing; never freed here, so a pool the audio thread holds stays valid.
        if (*s).sysex_output.is_none() && midi_port(&(*s).note_outputs) {
            (*s).sysex_output = Some(vec![0u8; SYSEX_BLOCK as usize].into_boxed_slice());
        }
        let ni: usize = (*s).inputs.iter().map(|q| q.channels as usize).sum();
        let no: usize = (*s).outputs.iter().map(|q| q.channels as usize).sum();
        // The ring-out buffer is four more scratch channels after the ports', in no port.
        (*s).ring_out_channels = if (*s).sound_snapshots.as_ref().is_some_and(|x| x.ring_out.is_some()) { 4 } else { 0 };
        let ring = (*s).ring_out_channels as usize;
        (*s).input_pointers.resize(ni, null_mut());
        (*s).output_pointers.resize(no + ring, null_mut());
        (*s).input_buffers.resize((*s).inputs.len(), std::mem::zeroed());
        (*s).output_buffers.resize((*s).outputs.len(), std::mem::zeroed());
        (*s).scratch.clear();
        (*s).scratch.resize((ni + no + ring) * (*s).max_frames as usize, 0.0);
        (*s).ring_out_buffer = std::mem::zeroed();
        (*s).ring_out_buffer.channel_count = ring as u32;
        (*s).ring_out_buffer.data32 = (*s).output_pointers.as_mut_ptr().add(no);
        let mut offset = 0;
        for i in 0..(*s).inputs.len() {
            let b = &mut (*s).input_buffers[i];
            *b = std::mem::zeroed();
            b.channel_count = (*s).inputs[i].channels;
            b.data32 = (*s).input_pointers.as_mut_ptr().add(offset);
            offset += (*s).inputs[i].channels as usize;
        }
        offset = 0;
        for i in 0..(*s).outputs.len() {
            let b = &mut (*s).output_buffers[i];
            *b = std::mem::zeroed();
            b.channel_count = (*s).outputs[i].channels;
            b.data32 = (*s).output_pointers.as_mut_ptr().add(offset);
            offset += (*s).outputs[i].channels as usize;
        }
        Ok(())
    }
}
pub fn midi_port(ports: &[clap_note_port_info]) -> bool {
    ports.iter().any(|q| q.supported_dialects & CLAP_NOTE_DIALECT_MIDI != 0)
}

/// # Safety
/// As main.
unsafe fn apply_outputs(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        let Some(pa) = (*s).ports_activation.as_ref() else { return };
        if !(*s).outputs_told.load(Acquire) {
            return;
        }
        let want = (*s).wanted_outputs.load(Acquire);
        for i in 0..(*s).outputs.len().min(64) {
            let on = (want >> i) & 1 != 0;
            if !(*s).applied_told || (((*s).applied_outputs >> i) & 1 != 0) != on {
                pa.set_active.unwrap()((*s).plugin, false, i as u32, on, 32);
            }
        }
        (*s).applied_outputs = want;
        (*s).applied_told = true;
    }
}

/// # Safety
/// As main.
pub unsafe fn activate(s: *mut Host) -> R<()> {
    // SAFETY: the caller's.
    unsafe {
        let rate = (*s).sample_rate;
        if !rate.is_finite() || !(8000.0..=768000.0).contains(&rate) || (*s).max_frames == 0 || (*s).max_frames > 65536 {
            return Err("Invalid sample rate or maximum block size".into());
        }
        read_ports(s)?;
        read_parameters(s)?;
        // Asked after the state is loaded and before activating, so a plugin that would not fit is refused before its
        // allocation (RFC 0025).
        (*s).has_declared = false;
        if let Some(m) = (*s).memory.as_ref() {
            if m.estimate.unwrap()((*s).plugin, rate, (*s).max_frames, &mut (*s).declared) {
                (*s).has_declared = true;
                let d = (*s).declared;
                let mut bytes: u64 = 0;
                for part in [if (*s).context { 0 } else { d.shared_bytes }, d.context_bytes, d.asset_bytes] {
                    bytes = bytes.checked_add(part).ok_or("Plugin's memory declaration is not credible")?;
                    // a sum that wraps
                }
                if (*s).memory_room != 0 && bytes > (*s).memory_room {
                    return Err(refuse(s, bytes));
                }
            }
        }
        let refused = bridge::kick_clap_activating(if (*s).has_declared { &(*s).declared } else { null() });
        if !refused.is_null() {
            return Err(CStr::from_ptr(refused).to_string_lossy().into_owned());
        }
        (*s).hardware_epoch.fetch_add(1, SeqCst); // jobs from an earlier activation collect as stale
        (*s).applied_told = false;
        apply_outputs(s); // deactivated: told every port again
        if !((*(*s).plugin).activate.unwrap())((*s).plugin, rate, 1, (*s).max_frames) {
            return Err("Plugin activation failed".into());
        }
        (*s).active = true;
        (*s).transport.reset();
        (*s).have_harmony = false;
        let latency = (*s).plugin_latency.as_ref().and_then(|l| l.get).map_or(0, |f| f((*s).plugin));
        (*s).latency_frames.store(latency, SeqCst);
        Ok(())
    }
}
/// # Safety
/// s is a live host.
pub unsafe fn refuse(s: *const Host, bytes: u64) -> String {
    // SAFETY: the caller's.
    let room = unsafe { (*s).memory_room };
    format!("Plugin needs {} MiB of memory where {} MiB is left", (bytes + (1 << 20) - 1) >> 20, room >> 20)
}
/// # Safety
/// s is a live host.
pub unsafe fn fail(s: *mut Host, why: String) {
    // SAFETY: the caller's.
    unsafe {
        (*s).message = why;
        (*s).failed.store(true, SeqCst);
    }
}

// ---- Events in and out ----
unsafe extern "C" fn event_count(list: *const clap_input_events) -> u32 {
    // SAFETY: the host's own list.
    unsafe { (*(*list).ctx.cast::<Host>()).chunk_count }
}
unsafe extern "C" fn event_get(list: *const clap_input_events, i: u32) -> *const clap_event_header {
    // SAFETY: as event_count.
    unsafe {
        let s = (*list).ctx.cast::<Host>();
        if i < (*s).chunk_count {
            std::ptr::from_ref((*s).chunk_events[i as usize].header())
        } else {
            null()
        }
    }
}
unsafe extern "C" fn output_push(list: *const clap_output_events, e: *const clap_event_header) -> bool {
    // SAFETY: the host's own list; e the plugin's event, its header's size readable (CLAP).
    unsafe {
        let s = (*list).ctx.cast::<Host>();
        if !event::valid(e) || (*e).time >= (*s).current_process_frames {
            if !e.is_null() && (*e).space_id == CLAP_CORE_EVENT_SPACE_ID && (*e).type_ == CLAP_EVENT_MIDI_SYSEX {
                (*s).dropped.fetch_add(1, SeqCst); // over the limit: counted
            }
            return false;
        }
        if (*e).space_id == BEAT_SPACE || (*e).space_id == HARMONY_SPACE {
            return false; // the host's to send
        }
        let core = (*e).space_id == CLAP_CORE_EVENT_SPACE_ID;
        let mut copy = Event::copy(e);
        if core && (*e).type_ == CLAP_EVENT_PARAM_VALUE {
            let p: clap_event_param_value = copy.get();
            let i = param_index(s, p.param_id);
            if i < (*s).param_info.len() && p.value.is_finite() && f64::from_bits((*s).param_values[i].swap(p.value.to_bits(), SeqCst)) != p.value {
                (*s).dirty.store(true, SeqCst);
                (*s).parameter_reports.fetch_add(1, Release);
            }
        }
        copy.header_mut().time += (*s).output_frame_offset;
        if core && (*e).type_ == CLAP_EVENT_MIDI_SYSEX {
            // MIDI data bytes are 7-bit. The plugin's buffer is valid only during this call: copy it.
            let mut x: clap_event_midi_sysex = copy.get();
            let data = (1..x.size.saturating_sub(1)).all(|i| *x.buffer.add(i as usize) < 0x80);
            let used = (*s).sysex_used;
            let held = data && (*s).in_process && x.size <= SYSEX_BLOCK - used && (*s).sysex_output.is_some();
            if !held {
                (*s).dropped.fetch_add(1, SeqCst);
                return false;
            }
            let pool = (*s).sysex_output.as_mut().unwrap().as_mut_ptr().add(used as usize);
            std::ptr::copy_nonoverlapping(x.buffer, pool, x.size as usize);
            x.buffer = pool;
            (*s).sysex_used += x.size;
            copy = Event::from(&x);
        }
        // Parameter and gesture events go to the control thread; everything else (notes, MIDI, expressions) is kept for
        // the caller of process(), which routes it to the next plugin in the same block.
        let t = (*e).type_;
        let control =
            core && (t == CLAP_EVENT_PARAM_VALUE || t == CLAP_EVENT_PARAM_MOD || t == CLAP_EVENT_PARAM_GESTURE_BEGIN || t == CLAP_EVENT_PARAM_GESTURE_END);
        if !control {
            if (*s).in_process && ((*s).block_output_count as usize) < MAX_BLOCK_EVENTS {
                (*s).block_output[(*s).block_output_count as usize] = copy;
                (*s).block_output_count += 1;
                return true;
            }
            (*s).dropped.fetch_add(1, SeqCst);
            return false;
        }
        if (*s).emitted.push(copy) {
            return true;
        }
        (*s).output_drops.fetch_add(1, SeqCst);
        (*s).dropped.fetch_add(1, SeqCst);
        false
    }
}
/// # Safety
/// As main under the gate, or the audio thread in process.
unsafe fn flush_parameters(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        let Some(flush) = (*s).params.as_ref().and_then(|p| p.flush) else { return };
        let _scope = AudioScope::new(s);
        (*s).chunk_count = 0;
        (*s).output_frame_offset = 0;
        (*s).current_process_frames = 1;
        // Only parameter events belong to flush(); service() never drains the command queue.
        let input = clap_input_events { ctx: s.cast(), size: Some(event_count), get: Some(event_get) };
        let output = clap_output_events { ctx: s.cast(), try_push: Some(output_push) };
        flush((*s).plugin, &input, &output);
    }
}
/// # Safety
/// s is a live host (audio or main thread, under the gate).
pub unsafe fn translate(s: *const Host, event: Event) -> Event {
    // SAFETY: the caller's.
    unsafe {
        let h = event.header();
        let inputs = &(*s).note_inputs;
        if h.type_ == CLAP_EVENT_PARAM_VALUE {
            let mut p: clap_event_param_value = event.get();
            let i = param_index(s, p.param_id);
            if i < (*s).cookies.len() {
                p.cookie = (*s).cookies[i];
                return Event::from(&p);
            }
        } else if h.type_ == CLAP_EVENT_MIDI {
            let m: clap_event_midi = event.get();
            if (m.port_index as usize) < inputs.len() {
                let dialect = inputs[m.port_index as usize].supported_dialects;
                if dialect & CLAP_NOTE_DIALECT_CLAP != 0 && dialect & (CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE) == 0 {
                    let status = m.data[0] & 0xf0;
                    if status == 0x90 || status == 0x80 {
                        return Event::note(
                            status == 0x90 && m.data[2] != 0,
                            m.data[1].into(),
                            f64::from(m.data[2]) / 127.0,
                            h.time,
                            (m.data[0] & 15).into(),
                            m.port_index.into(),
                            -1,
                        );
                    }
                }
            }
        } else if h.type_ == CLAP_EVENT_NOTE_ON || h.type_ == CLAP_EVENT_NOTE_OFF {
            let n: clap_event_note = event.get();
            if n.port_index >= 0 && (n.port_index as usize) < inputs.len() && inputs[n.port_index as usize].supported_dialects & CLAP_NOTE_DIALECT_CLAP == 0 {
                return Event::midi(
                    (if h.type_ == CLAP_EVENT_NOTE_ON { 0x90 } else { 0x80 }) | n.channel as u8,
                    n.key as u8,
                    (n.velocity.clamp(0.0, 1.0) * 127.0) as u8,
                    h.time,
                    n.port_index as u16,
                );
            }
        }
        event
    }
}
/// # Safety
/// As translate.
unsafe fn accepts(s: *const Host, event: &Event) -> bool {
    // SAFETY: the caller's.
    unsafe {
        let h = event.header();
        let inputs = &(*s).note_inputs;
        match h.type_ {
            CLAP_EVENT_NOTE_END => false, // plugin-to-host only
            CLAP_EVENT_MIDI_SYSEX => {
                // On a note port that takes MIDI, as CLAP's dialects say.
                let port = event.get::<clap_event_midi_sysex>().port_index as usize;
                port < inputs.len() && inputs[port].supported_dialects & CLAP_NOTE_DIALECT_MIDI != 0
            }
            CLAP_EVENT_PARAM_VALUE | CLAP_EVENT_PARAM_MOD | CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => {
                let id = match h.type_ {
                    CLAP_EVENT_PARAM_VALUE => event.get::<clap_event_param_value>().param_id,
                    CLAP_EVENT_PARAM_MOD => event.get::<clap_event_param_mod>().param_id,
                    _ => event.get::<clap_event_param_gesture>().param_id,
                };
                let i = param_index(s, id);
                i < (*s).param_info.len() && (*s).param_info[i].flags & CLAP_PARAM_IS_READONLY == 0
            }
            CLAP_EVENT_MIDI => {
                let port = event.get::<clap_event_midi>().port_index as usize;
                port < inputs.len() && inputs[port].supported_dialects & (CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE) != 0
            }
            CLAP_EVENT_MIDI2 => false, // not advertised by this host
            _ => {
                let port = if h.type_ == CLAP_EVENT_NOTE_EXPRESSION {
                    event.get::<clap_event_note_expression>().port_index
                } else {
                    event.get::<clap_event_note>().port_index
                };
                if port == -1 {
                    return !inputs.is_empty();
                }
                port >= 0 && (port as usize) < inputs.len() && inputs[port as usize].supported_dialects & CLAP_NOTE_DIALECT_CLAP != 0
            }
        }
    }
}

// ---- The control thread's calls ----

/// Host::service.
///
/// # Safety
/// s is a live host; called on its main thread.
pub unsafe fn service(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        if !main(s) || (*s).plugin.is_null() {
            return;
        }
        if let Err(e) = service_inner(s) {
            fail(s, e);
        }
    }
}
unsafe fn service_inner(s: *mut Host) -> R<()> {
    // SAFETY: service's.
    unsafe {
        let p = (*s).plugin;
        if (*s).callback.swap(false, SeqCst) {
            ((*p).on_main_thread.unwrap())(p);
        }
        let now = Instant::now();
        // Callbacks may register and unregister timers and fds, so iterate copies.
        let timers = (*s).timers.clone();
        if let Some(on_timer) = (*s).timer_support.as_ref().and_then(|t| t.on_timer) {
            for timer in timers.iter().filter(|t| now >= t.next) {
                if let Some(t) = (*s).timers.iter_mut().find(|t| t.id == timer.id) {
                    t.next = now + Duration::from_millis(timer.period.into());
                    on_timer(p, timer.id);
                }
            }
        }
        let fds = (*s).fds.clone();
        if let Some(on_fd) = (*s).fd_support.as_ref().and_then(|f| f.on_fd) {
            for item in &fds {
                let mut fd = PollFd { fd: item.fd, events: 0, revents: 0 };
                if item.flags & CLAP_POSIX_FD_READ != 0 {
                    fd.events |= POLLIN;
                }
                if item.flags & CLAP_POSIX_FD_WRITE != 0 {
                    fd.events |= POLLOUT;
                }
                if poll(&mut fd, 1, 0) > 0 {
                    let mut flags = 0;
                    if fd.revents & POLLIN != 0 {
                        flags |= CLAP_POSIX_FD_READ;
                    }
                    if fd.revents & POLLOUT != 0 {
                        flags |= CLAP_POSIX_FD_WRITE;
                    }
                    if fd.revents & (POLLERR | POLLHUP | POLLNVAL) != 0 {
                        flags |= CLAP_POSIX_FD_ERROR;
                    }
                    if flags != 0 && (*s).fds.iter().any(|f| f.fd == item.fd) {
                        on_fd(p, item.fd, flags);
                    }
                }
            }
        }
        service_gui(s);
        let flags = (*s).rescan.swap(0, SeqCst);
        let restart = (*s).restart.swap(false, SeqCst);
        if restart || flags & CLAP_PARAM_RESCAN_ALL != 0 || (flags & CLAP_PARAM_RESCAN_INFO != 0 && !read_parameter_info(s)) {
            let _gate = Gate::new(&(*s).gate);
            deactivate(s);
            activate(s)?;
        } else if flags & CLAP_PARAM_RESCAN_VALUES != 0 {
            let get = (*(*s).params).get_value.unwrap();
            for i in 0..(*s).param_info.len() {
                let mut value = 0.0;
                if get(p, (*s).param_info[i].id, &mut value) && value.is_finite() {
                    f64_store(&(*s).param_values[i], value);
                }
            }
        }
        // While the plugin is processing, process() performs requested flushes on the audio thread. Otherwise flush here,
        // excluding a process() that starts.
        if !(*s).processing.load(SeqCst) && (*s).flush.load(SeqCst) {
            let _gate = Gate::new(&(*s).gate);
            if !(*s).processing.load(SeqCst) && (*s).flush.swap(false, SeqCst) {
                flush_parameters(s);
            }
        }
        if (*s).failed.load(SeqCst) {
            let _gate = Gate::new(&(*s).gate);
            stop(s);
        }
        Ok(())
    }
}
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
const POLLIN: i16 = 0x1;
const POLLOUT: i16 = 0x4;
const POLLERR: i16 = 0x8;
const POLLHUP: i16 = 0x10;
const POLLNVAL: i16 = 0x20;
extern "C" {
    fn poll(fds: *mut PollFd, n: u64, timeout: i32) -> i32;
}

/// Host::enqueue.
///
/// # Safety
/// s is a live host; any thread.
pub unsafe fn enqueue(s: *mut Host, e: &Event) -> bool {
    // SAFETY: the caller's; e is a whole event.
    unsafe {
        let h = e.header();
        if !event::valid(h) || h.space_id != CLAP_CORE_EVENT_SPACE_ID || h.type_ == CLAP_EVENT_MIDI_SYSEX {
            return false;
        }
        // Count before publishing, so process() never takes an event it has not counted.
        (*s).pending_commands.fetch_add(1, SeqCst);
        if (*s).commands.push(*e) {
            wake_up(s);
            return true;
        }
        (*s).pending_commands.fetch_sub(1, SeqCst);
        (*s).dropped.fetch_add(1, SeqCst);
        // Control callers can retain and retry refused parameter and gesture commands; resetting here would discard
        // already accepted commands and cut held notes.
        let t = h.type_;
        if t != CLAP_EVENT_PARAM_VALUE && t != CLAP_EVENT_PARAM_MOD && t != CLAP_EVENT_PARAM_GESTURE_BEGIN && t != CLAP_EVENT_PARAM_GESTURE_END {
            (*s).panic.store(true, SeqCst);
        }
        false
    }
}
/// # Safety
/// As enqueue.
pub unsafe fn set_parameter(s: *mut Host, id: clap_id, value: f64) -> bool {
    // SAFETY: the caller's.
    unsafe {
        let index = param_index(s, id);
        if index == (*s).param_info.len() || !value.is_finite() || (*s).param_info[index].flags & CLAP_PARAM_IS_READONLY != 0 {
            return false;
        }
        let info = &(*s).param_info[index];
        let mut value = value.clamp(info.minimum, info.maximum);
        if info.flags & CLAP_PARAM_IS_STEPPED != 0 {
            value = value.trunc();
        }
        // Zeroed first, padding too, as the C++ value-initializes it: the plugin is given every byte.
        let mut e: clap_event_param_value = std::mem::zeroed();
        e.header = clap_event_header {
            size: size_of::<clap_event_param_value>() as u32,
            time: 0,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_PARAM_VALUE,
            flags: CLAP_EVENT_IS_LIVE,
        };
        e.param_id = id;
        e.note_id = -1;
        e.port_index = -1;
        e.channel = -1;
        e.key = -1;
        e.value = value;
        if !enqueue(s, &Event::from(&e)) {
            return false;
        }
        f64_store(&(*s).param_values[index], value);
        (*s).dirty.store(true, SeqCst);
        true
    }
}
/// # Safety
/// As enqueue.
pub unsafe fn parameter_gesture(s: *mut Host, id: clap_id, begin: bool) -> bool {
    // SAFETY: plain old data, zeroed as set_parameter's.
    let mut e: clap_event_param_gesture = unsafe { std::mem::zeroed() };
    e.header = clap_event_header {
        size: size_of::<clap_event_param_gesture>() as u32,
        time: 0,
        space_id: CLAP_CORE_EVENT_SPACE_ID,
        type_: if begin { CLAP_EVENT_PARAM_GESTURE_BEGIN } else { CLAP_EVENT_PARAM_GESTURE_END },
        flags: CLAP_EVENT_IS_LIVE,
    };
    e.param_id = id;
    // SAFETY: the caller's.
    unsafe { enqueue(s, &Event::from(&e)) }
}
/// # Safety
/// The one consumer: the main thread.
pub unsafe fn pop_output(s: *mut Host) -> Option<Event> {
    // SAFETY: the caller's.
    unsafe { (*s).emitted.pop() }
}
/// # Safety
/// As main.
pub unsafe fn parameter_text(s: *mut Host, id: clap_id, value: f64) -> Vec<u8> {
    // SAFETY: the caller's.
    unsafe {
        if let Some(f) = (*s).params.as_ref().and_then(|p| p.value_to_text) {
            let mut text = [0 as c_char; 256];
            if f((*s).plugin, id, value, text.as_mut_ptr(), 256) {
                return text.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
            }
        }
        cpp_to_string(value).into_bytes()
    }
}
/// Host::panic.
///
/// # Safety
/// s is a live host.
pub unsafe fn panic(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        (*s).panic.store(true, SeqCst);
        wake_up(s);
    }
}
/// # Safety
/// As main.
pub unsafe fn reset(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        if !main(s) || (*s).plugin.is_null() {
            return;
        }
        let _gate = Gate::new(&(*s).gate);
        let _scope = AudioScope::new(s);
        ((*(*s).plugin).reset.unwrap())((*s).plugin);
        (*s).sleeping.store(false, SeqCst);
    }
}
/// # Safety
/// As main.
pub unsafe fn set_active_outputs(s: *mut Host, ports: u64) {
    // SAFETY: the caller's.
    unsafe {
        let told = (*s).outputs_told.load(Relaxed);
        if told && (*s).wanted_outputs.load(Relaxed) == ports {
            return;
        }
        (*s).wanted_outputs.store(ports, Release);
        (*s).outputs_told.store(true, Release);
        if !(*s).ports_activation.is_null() && !(*s).activate_while_processing && (*s).active {
            (*s).restart.store(true, SeqCst); // only while deactivated: service() activates it again
        }
    }
}
/// # Safety
/// Audio thread, between process() calls.
pub unsafe fn set_voice_limits(s: *mut Host, limit: u32, contexts: *const oscairn_voice_budget_context_t, count: u32) {
    // SAFETY: the caller's; contexts holds count.
    unsafe {
        if (*s).voice_budget.is_null() {
            return;
        }
        (*s).voice_limit = limit;
        (*s).voice_context_count = if contexts.is_null() { 0 } else { count.min(OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS) };
        for i in 0..(*s).voice_context_count as usize {
            (*s).voice_contexts[i] = *contexts.add(i);
        }
        (*s).limits_waiting = true;
    }
}
/// # Safety
/// Audio thread, after process().
pub unsafe fn voice_report(s: *const Host, cost: &mut f64, notes: &mut *const oscairn_voice_budget_note_t, count: &mut u32) -> u32 {
    // SAFETY: the caller's.
    unsafe {
        *cost = (*s).voice_cost;
        *notes = (*s).voice_notes.as_ptr();
        let budget = !(*s).voice_budget.is_null();
        *count = if budget { (*s).voice_note_count } else { 0 };
        if budget {
            (*s).active_voices
        } else {
            0
        }
    }
}
/// # Safety
/// s is a live host.
pub unsafe fn restful(s: *const Host) -> bool {
    // SAFETY: the caller's.
    unsafe {
        (*s).sleeping.load(SeqCst)
            && !(*s).wake.load(SeqCst)
            && !(*s).flush.load(SeqCst)
            && !(*s).panic.load(SeqCst)
            && (*s).carry_count == 0
            && !(*s).wants_beat.load(SeqCst)
            && !(*s).wants_harmony.load(SeqCst)
    }
}
/// # Safety
/// Audio thread.
pub unsafe fn rested(s: *mut Host, frames: u32, blocks: u32) {
    // SAFETY: the caller's.
    unsafe {
        (*s).steady_time += i64::from(frames);
        (*s).idle_blocks.fetch_add(blocks.into(), SeqCst);
    }
}
/// # Safety
/// s is a live host; mirror outlives its setting.
pub unsafe fn mirror_wake(s: *mut Host, mirror: *const AtomicU32) {
    // SAFETY: the caller's.
    unsafe {
        (*s).wake_mirror = mirror;
        if !mirror.is_null() {
            (*mirror).store(u32::from((*s).wake.load(SeqCst)), SeqCst);
        }
    }
}
/// Host::lockAudioLayout.
///
/// # Safety
/// As main.
pub unsafe fn lock_audio_layout(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        if main(s) && !(*s).plugin.is_null() {
            (*s).audio_layout_locked = true;
        }
    }
}
/// Host::reconfigure.
///
/// # Safety
/// As main.
pub unsafe fn reconfigure(s: *mut Host, rate: f64, frames: u32) -> bool {
    // SAFETY: the caller's.
    unsafe {
        if !main(s) || !(*s).loaded.load(SeqCst) {
            return false;
        }
        let _gate = Gate::new(&(*s).gate);
        deactivate(s);
        (*s).sample_rate = rate;
        (*s).max_frames = frames;
        match activate(s) {
            Ok(()) => {
                (*s).failed.store(false, SeqCst);
                true
            }
            Err(e) => {
                fail(s, e);
                false
            }
        }
    }
}

// ---- The audio thread's block ----

/// Host::process. Allocates nothing and takes no lock: the gate is tried, never waited for.
///
/// # Safety
/// s is a live host; its one audio thread; inputs, outputs and events as Host.h says.
#[allow(clippy::too_many_arguments)]
pub unsafe fn process(
    s: *mut Host,
    input: *const *const f32,
    ni: u32,
    output: *const *mut f32,
    no: u32,
    frames: u32,
    incoming: *const Event,
    count: u32,
    transport: *const clap_event_transport,
) -> bool {
    // SAFETY: the caller's.
    unsafe {
        let silence = || {
            if !output.is_null() {
                for c in 0..no as usize {
                    let o = *output.add(c);
                    if !o.is_null() {
                        std::ptr::write_bytes(o, 0, frames as usize);
                    }
                }
            }
        };
        (*s).published_output_count = 0;
        (*s).reset_in_process = false;
        if frames == 0 {
            return true;
        }
        // A note-only plugin (no audio outputs) may be processed with no output buffers.
        if (no != 0 && output.is_null()) || (count != 0 && incoming.is_null()) {
            silence();
            return false;
        }
        if (*s).gate.swap(true, Acquire) {
            (*s).control_silences.fetch_add(1, SeqCst);
            silence();
            return false;
        }
        let ok = block(s, input, ni, output, no, frames, incoming, count, transport, &silence);
        (*s).in_process = false;
        (*s).published_output_count = (*s).block_output_count;
        (*s).gate.store(false, Release);
        ok
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn block(
    s: *mut Host,
    input: *const *const f32,
    ni: u32,
    output: *const *mut f32,
    no: u32,
    frames: u32,
    incoming: *const Event,
    count: u32,
    transport: *const clap_event_transport,
    silence: &dyn Fn(),
) -> bool {
    // SAFETY: process's.
    unsafe {
        (*s).block_output_count = 0;
        (*s).sysex_used = 0;
        (*s).in_process = true;
        let _scope = AudioScope::new(s);
        let p = (*s).plugin;
        if p.is_null() || !(*s).active || (*s).failed.load(SeqCst) {
            silence();
            return false;
        }
        let start = Instant::now();
        // Saving must wait until the plugin processed them, not just until they left the queue.
        let mut acknowledged = 0u32;
        let ok = render(s, p, input, ni, output, no, frames, incoming, count, transport, silence, &mut acknowledged, start);
        if acknowledged != 0 {
            (*s).pending_commands.fetch_sub(acknowledged, Release);
        }
        ok
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn render(
    s: *mut Host,
    p: *const clap_plugin,
    input: *const *const f32,
    ni: u32,
    output: *const *mut f32,
    no: u32,
    frames: u32,
    incoming: *const Event,
    count: u32,
    transport: *const clap_event_transport,
    silence: &dyn Fn(),
    acknowledged: &mut u32,
    start: Instant,
) -> bool {
    // SAFETY: process's.
    unsafe {
        struct Collect {
            total: usize,
            harmony_came: bool, // a block without one: the plugin is in no part
        }
        unsafe fn deliver(s: *mut Host, c: &mut Collect, frames: u32, e: Event) {
            // SAFETY: render's.
            unsafe {
                let h = e.header();
                // A malformed or late event is dropped on its own; resetting the plugin for it would cut every sounding note.
                // Oscairn's host-to-plugin events go only to a plugin that asked for them, as they are.
                if h.space_id == BEAT_SPACE || h.space_id == HARMONY_SPACE {
                    if !event::valid(h) || h.time >= frames {
                        (*s).dropped.fetch_add(1, SeqCst);
                        return;
                    }
                    if h.space_id == BEAT_SPACE && !(*s).wants_beat.load(Relaxed) {
                        return;
                    }
                    if h.space_id == HARMONY_SPACE {
                        let x = std::ptr::read_unaligned(e.bytes.as_ptr().cast::<oscairn_event_harmony_t>()).harmony;
                        c.harmony_came = true;
                        if !(*s).wants_harmony.load(Relaxed) || ((*s).have_harmony && harmony_eq(&x, &(*s).last_harmony)) {
                            return;
                        }
                        (*s).last_harmony = x;
                        (*s).have_harmony = true;
                        publish_harmony(s, Some(&x));
                    }
                    if c.total == RENDER_EVENTS {
                        (*s).dropped.fetch_add(1, SeqCst);
                        return;
                    }
                    (*s).ordered[c.total] = Timed { event: e, order: c.total as u32 };
                    c.total += 1;
                    return;
                }
                // Events in other spaces (param-target) are for the caller, never for a plugin.
                if !event::valid(h) || h.space_id != CLAP_CORE_EVENT_SPACE_ID || h.time >= frames {
                    (*s).dropped.fetch_add(1, SeqCst);
                    return;
                }
                // Overflow can drop note-offs, so it still resets the plugin rather than risk stuck notes.
                if c.total == RENDER_EVENTS {
                    (*s).dropped.fetch_add(1, SeqCst);
                    (*s).panic.store(true, SeqCst);
                    return;
                }
                let translated = translate(s, e);
                if !accepts(s, &translated) {
                    (*s).dropped.fetch_add(1, SeqCst);
                    return;
                }
                (*s).ordered[c.total] = Timed { event: translated, order: c.total as u32 };
                c.total += 1;
            }
        }
        unsafe fn append(s: *mut Host, c: &mut Collect, frames: u32, e: Event) {
            // SAFETY: render's.
            unsafe {
                let h = e.header();
                if event::valid(h) && h.space_id == CLAP_CORE_EVENT_SPACE_ID && (h.type_ == CLAP_EVENT_NOTE_OFF || h.type_ == CLAP_EVENT_NOTE_CHOKE) {
                    let n: clap_event_note = e.get();
                    for port in 0..(*s).note_inputs.len() {
                        if n.port_index >= 0 && n.port_index as usize != port {
                            continue;
                        }
                        if (*s).note_inputs[port].supported_dialects & CLAP_NOTE_DIALECT_CLAP != 0 {
                            let mut copy = n;
                            copy.port_index = port as i16;
                            deliver(s, c, frames, Event::from(&copy));
                        } else {
                            for channel in 0..16u8 {
                                if n.channel >= 0 && n.channel as u8 != channel {
                                    continue;
                                }
                                if n.key < 0 {
                                    deliver(
                                        s,
                                        c,
                                        frames,
                                        Event::midi(0xb0 | channel, if h.type_ == CLAP_EVENT_NOTE_CHOKE { 120 } else { 123 }, 0, h.time, port as u16),
                                    );
                                } else {
                                    deliver(s, c, frames, Event::midi(0x80 | channel, n.key as u8, (n.velocity * 127.0) as u8, h.time, port as u16));
                                }
                            }
                        }
                    }
                } else {
                    deliver(s, c, frames, e);
                }
            }
        }
        let mut c = Collect { total: 0, harmony_came: false };
        for i in 0..(*s).carry_count as usize {
            append(s, &mut c, frames, (*s).carry_events[i]);
        }
        (*s).carry_count = 0;
        let direct = (count as usize).min(MAX_PROCESS_EVENTS);
        while (*acknowledged as usize) < MAX_BLOCK_EVENTS {
            let Some(e) = (*s).commands.pop() else { break };
            *acknowledged += 1;
            append(s, &mut c, frames, e);
        }
        for i in 0..direct {
            append(s, &mut c, frames, *incoming.add(i));
        }
        let mut total = c.total;
        if count as usize > MAX_PROCESS_EVENTS {
            (*s).dropped.fetch_add(u64::from(count) - MAX_PROCESS_EVENTS as u64, SeqCst);
            (*s).panic.store(true, SeqCst);
        }
        if !c.harmony_came && (*s).harmony_words[3].load(Relaxed) != 0 {
            (*s).have_harmony = false; // out of its part, or its part's harmony gone
            publish_harmony(s, None);
        }
        if (*s).panic.swap(false, SeqCst) {
            // Never leave a stuck gate after an event overflow. Reset is an audio-thread operation and covers every note
            // channel and port.
            ((*p).reset.unwrap())(p);
            total = 0;
            (*s).reset_in_process = true;
            (*s).sleeping.store(false, SeqCst);
            (*s).have_harmony = false; // the harmony comes again next block
        }
        (*s).ordered[..total].sort_unstable_by_key(|e| (e.event.header().time, e.order));
        let mut has_input = false;
        if !input.is_null() {
            for c in 0..ni as usize {
                let i = *input.add(c);
                if !i.is_null() && std::slice::from_raw_parts(i, frames as usize).iter().any(|&x| x != 0.0) {
                    has_input = true;
                    break;
                }
            }
        }
        let transport = transport.as_ref();
        let transport_wake = (*s).transport.changed(transport, frames, (*s).sample_rate);
        if (*s).sleeping.load(SeqCst) && total == 0 && !has_input && !transport_wake && !(*s).wake.swap(false, SeqCst) && !(*s).flush.load(SeqCst) {
            (*s).idle_blocks.fetch_add(1, SeqCst);
            (*s).steady_time += i64::from(frames);
            silence();
            return true;
        }
        (*s).wake.store(false, SeqCst);
        (*s).sleeping.store(false, SeqCst);
        if !(*s).wake_mirror.is_null() {
            (*(*s).wake_mirror).store(0, Release);
        }
        if !(*s).processing.load(SeqCst) {
            if !((*p).start_processing.unwrap())(p) {
                (*s).failed.store(true, SeqCst);
                (*s).errors.fetch_add(1, SeqCst);
                silence();
                return false;
            }
            (*s).processing.store(true, SeqCst);
        }
        if (*s).flush.swap(false, SeqCst) {
            flush_parameters(s);
        }
        if (*s).activate_while_processing
            && (*s).outputs_told.load(Relaxed)
            && (!(*s).applied_told || (*s).wanted_outputs.load(Acquire) != (*s).applied_outputs)
        {
            apply_outputs(s);
        }
        // oscairn.voice-budget: new limits before the block (an audio-thread call outside process()).
        if (*s).limits_waiting {
            if let Some(v) = (*s).voice_budget.as_ref() {
                v.set_limits.unwrap()(p, (*s).voice_limit, (*s).voice_contexts.as_ptr(), (*s).voice_context_count);
                (*s).limits_waiting = false;
            }
        }
        let max_frames = (*s).max_frames as usize;
        let ni_ports = (*s).input_pointers.len();
        let mut next = 0;
        let mut done = 0u32;
        while done < frames {
            let n = (*s).max_frames.min(frames - done);
            (*s).chunk_count = 0;
            (*s).output_frame_offset = done;
            (*s).current_process_frames = n;
            while next < total && (*s).ordered[next].event.header().time < done + n {
                let mut e = (*s).ordered[next].event;
                next += 1;
                e.header_mut().time -= done;
                (*s).chunk_events[(*s).chunk_count as usize] = e;
                (*s).chunk_count += 1;
            }
            let scratch = (*s).scratch.as_mut_ptr();
            for c in 0..ni_ports {
                let target = scratch.add(c * max_frames);
                let i = if !input.is_null() && c < ni as usize { *input.add(c) } else { null() };
                if i.is_null() {
                    std::ptr::write_bytes(target, 0, n as usize);
                } else {
                    std::ptr::copy_nonoverlapping(i.add(done as usize), target, n as usize);
                }
                (*s).input_pointers[c] = target;
            }
            for c in 0..(*s).output_pointers.len() {
                let target = scratch.add((ni_ports + c) * max_frames);
                std::ptr::write_bytes(target, 0, n as usize);
                (*s).output_pointers[c] = target;
            }
            let in_events = clap_input_events { ctx: s.cast(), size: Some(event_count), get: Some(event_get) };
            let out_events = clap_output_events { ctx: s.cast(), try_push: Some(output_push) };
            let mut position: clap_event_transport;
            let mut process = clap_process {
                steady_time: (*s).steady_time,
                frames_count: n,
                transport: null(),
                audio_inputs: (*s).input_buffers.as_ptr(),
                audio_outputs: (*s).output_buffers.as_mut_ptr(),
                audio_inputs_count: (*s).input_buffers.len() as u32,
                audio_outputs_count: (*s).output_buffers.len() as u32,
                in_events: &in_events,
                out_events: &out_events,
            };
            if let Some(t) = transport {
                position = *t;
                let playing = position.flags & CLAP_TRANSPORT_IS_PLAYING != 0;
                if playing && position.flags & CLAP_TRANSPORT_HAS_SECONDS_TIMELINE != 0 {
                    position.song_pos_seconds += ((f64::from(done) / (*s).sample_rate) * CLAP_SECTIME_FACTOR as f64) as i64;
                }
                if playing && position.flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0 && position.flags & CLAP_TRANSPORT_HAS_TEMPO != 0 {
                    position.song_pos_beats += ((f64::from(done) / (*s).sample_rate) * position.tempo / 60.0 * CLAP_BEATTIME_FACTOR as f64) as i64;
                }
                position.header.time = 0;
                process.transport = &position;
            }
            // The caller asks for the ring-out buffer by passing its channels after the ports'.
            if (*s).ring_out_channels != 0 {
                let ring = if no as usize >= (*s).output_pointers.len() { std::ptr::addr_of!((*s).ring_out_buffer) } else { null() };
                (*(*s).sound_snapshots).ring_out.unwrap()(p, ring);
            }
            let status = ((*p).process.unwrap())(p, &process);
            if !(CLAP_PROCESS_CONTINUE..=CLAP_PROCESS_SLEEP).contains(&status) {
                (*s).failed.store(true, SeqCst);
                (*s).errors.fetch_add(1, SeqCst);
                silence();
                return false;
            }
            for c in 0..no as usize {
                let o = *output.add(c);
                if o.is_null() {
                    continue;
                }
                let destination = o.add(done as usize);
                if c < (*s).output_pointers.len() {
                    // One pass without a branch a sample: copied, then a block with any sample not finite silenced.
                    let from = (*s).output_pointers[c];
                    let mut bad = 0u32;
                    for i in 0..n as usize {
                        let x = *from.add(i);
                        *destination.add(i) = x;
                        bad |= u32::from(!x.is_finite()); // the C++ `!(fabs(x) <= FLT_MAX)`: NaN or infinite
                    }
                    if bad != 0 {
                        (*s).failed.store(true, SeqCst);
                        (*s).errors.fetch_add(1, SeqCst);
                        silence();
                        return false;
                    }
                } else {
                    std::ptr::write_bytes(destination, 0, n as usize);
                }
            }
            (*s).sleeping.store(status == CLAP_PROCESS_SLEEP, SeqCst);
            (*s).steady_time += i64::from(n);
            done += n;
        }
        if let Some(v) = (*s).voice_budget.as_ref() {
            (*s).active_voices = v.active_voices.unwrap()(p, &mut (*s).voice_cost);
            let capacity = (*s).voice_notes.len() as u32;
            (*s).voice_note_count = v.notes.unwrap()(p, (*s).voice_notes.as_mut_ptr(), capacity).min(capacity);
        }
        (*s).blocks.fetch_add(1, SeqCst);
        let us = start.elapsed().as_secs_f64() * 1e6;
        f64_store(&(*s).last_us, us);
        if us > f64_load(&(*s).max_us) {
            f64_store(&(*s).max_us, us);
        }
        f64_store(&(*s).total_us, f64_load(&(*s).total_us) + us);
        if us > f64::from(frames) / (*s).sample_rate * 1e6 {
            (*s).deadline_misses.fetch_add(1, SeqCst);
        }
        true
    }
}
/// memcmp of two harmonies: they have no padding (six bytes, two bytes, two words).
fn harmony_eq(a: &oscairn_harmony_t, b: &oscairn_harmony_t) -> bool {
    a.key_root == b.key_root
        && a.chord_root == b.chord_root
        && a.chord_type == b.chord_type
        && a.chord_bass == b.chord_bass
        && a.scale == b.scale
        && a.reserved == b.reserved
        && a.held == b.held
}

/// # Safety
/// s is a live host.
pub unsafe fn supports_state(s: *const Host) -> bool {
    // SAFETY: the caller's.
    unsafe { (*s).state.as_ref().is_some_and(|x| x.save.is_some() && x.load.is_some()) }
}
/// # Safety
/// s is a live host.
pub unsafe fn exclusive_state(s: *const Host) -> bool {
    // SAFETY: the caller's.
    unsafe { (*s).exclusive_state }
}

// ---- For more.rs: the host's state as the rest of Host.cpp touches it ----
impl Host {
    pub fn dirty_clear(&self) {
        self.dirty.store(false, SeqCst);
    }
    pub fn dirty_set(&self) {
        self.dirty.store(true, SeqCst);
    }
    pub fn last_logged_error_clear(&mut self) {
        self.last_logged_error.clear();
    }
    pub fn last_logged_error(&self) -> String {
        self.last_logged_error.clone()
    }
    pub fn asset_dependencies(&self) -> *const oscairn_plugin_asset_dependencies_t {
        self.asset_dependencies
    }
    pub fn asset_optional(&self) -> bool {
        self.asset_optional
    }
    pub fn timing(&self) -> (f64, u32) {
        (self.sample_rate, self.max_frames)
    }
}
/// The plugin's state table: clap.state's, or its sound contexts' own.
///
/// # Safety
/// s is a live host.
pub unsafe fn state(s: *const Host) -> *const clap_plugin_state {
    // SAFETY: the caller's.
    unsafe { (*s).state }
}
/// # Safety
/// As main.
pub unsafe fn stop_processing(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe { stop(s) }
}
/// Every queued command dropped.
///
/// # Safety
/// As main, under the gate.
pub unsafe fn drain_commands(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        while (*s).commands.pop().is_some() {
            (*s).pending_commands.fetch_sub(1, SeqCst);
        }
    }
}
/// # Safety
/// As main, under the gate (the queue's one consumer).
pub unsafe fn pop_command(s: *mut Host) -> Option<Event> {
    // SAFETY: the caller's.
    unsafe { (*s).commands.pop() }
}
/// # Safety
/// As translate.
pub unsafe fn translate_event(s: *const Host, e: Event) -> Event {
    // SAFETY: the caller's.
    unsafe { translate(s, e) }
}
/// The plugin's parameter flush with these events (saveState's), as the audio thread.
///
/// # Safety
/// As main, under the gate, the plugin not processing.
pub unsafe fn flush_with(s: *mut Host, events: &[Event]) {
    // SAFETY: the caller's.
    unsafe {
        (*s).chunk_count = events.len() as u32;
        (*s).chunk_events[..events.len()].copy_from_slice(events);
        (*s).output_frame_offset = 0;
        (*s).current_process_frames = 1;
        let Some(flush) = (*s).params.as_ref().and_then(|p| p.flush) else { return };
        let _scope = AudioScope::new(s);
        let input = clap_input_events { ctx: s.cast(), size: Some(event_count), get: Some(event_get) };
        let output = clap_output_events { ctx: s.cast(), try_push: Some(output_push) };
        flush((*s).plugin, &input, &output);
    }
}
/// The parameters' values read back from the plugin (a rescan of values, a preset loaded).
///
/// # Safety
/// As main.
pub unsafe fn read_values(s: *mut Host) {
    // SAFETY: the caller's.
    unsafe {
        let Some(get) = (*s).params.as_ref().and_then(|p| p.get_value) else { return };
        for i in 0..(*s).param_info.len() {
            let mut value = 0.0;
            if get((*s).plugin, (*s).param_info[i].id, &mut value) && value.is_finite() {
                f64_store(&(*s).param_values[i], value);
            }
        }
    }
}
/// What an effect context takes from the instance it is made from.
///
/// # Safety
/// Both live hosts.
pub unsafe fn inherit(s: *mut Host, parent: *const Host) {
    // SAFETY: the caller's.
    unsafe {
        (*s).library = (*parent).library.clone();
        (*s).info = (*parent).info.clone();
        (*s).context = true;
        (*s).memory_room = (*parent).memory_room;
        (*s).offline = (*parent).offline;
        (*s).assets = (*parent).assets; // a copy of the table: the parent may go first
    }
}
/// # Safety
/// As initialize.
pub unsafe fn initialize_context(s: *mut Host, rate: f64, frames: u32) -> R<()> {
    // SAFETY: the caller's.
    unsafe { initialize(s, rate, frames) }
}
/// # Safety
/// s is a live host.
pub unsafe fn refusal(s: *const Host, bytes: u64) -> String {
    // SAFETY: the caller's.
    unsafe { refuse(s, bytes) }
}
