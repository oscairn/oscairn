// SPDX-License-Identifier: AGPL-3.0-or-later
//! One plugin instance: kick::clap::Host (core/clap-host/src/Host.cpp), its behavior call for call.
//!
//! Threads as Host.h says: control calls on one main thread, process on one audio thread, enqueue and the counters on
//! any. The plugin calls the host back from inside any of them. Every thread reaches the host through an H, which gives
//! the shared Host only; each field is an atomic, a queue, or an Owned that one thread at a time reads and writes, as
//! the groups on Host say (the C++ Impl's main-thread, gated, audio-thread and atomic state), and no reference into a
//! field lives across a call into the plugin.
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
use std::cell::{Cell, UnsafeCell};
use std::ffi::{c_char, c_void, CStr, CString};
use std::ops::Deref;
use std::ptr::{null, null_mut, NonNull};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
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

// ---- The handle and the threads' split of the Host ----

/// The Host as every thread reaches it: the C caller (lib.rs), the plugin's callbacks, the host's window. A pointer,
/// Copy; it gives the shared Host only, so no thread ever holds the whole Host exclusively while another touches it.
#[derive(Clone, Copy)]
pub struct H(NonNull<Host>);
impl H {
    /// # Safety
    /// host is a live Host (Host::create's, not yet freed), and this thread keeps to the groups on Host for as long as it
    /// uses this H and its copies: clap_host.h names the thread of each C call, CLAP the thread of each callback.
    pub unsafe fn new(host: *mut Host) -> H {
        // SAFETY: the caller's: live, so not null.
        H(unsafe { NonNull::new_unchecked(host) })
    }
    pub fn ptr(self) -> *mut Host {
        self.0.as_ptr()
    }
}
impl Deref for H {
    type Target = Host;
    fn deref(&self) -> &Host {
        // SAFETY: H::new's contract: the Host is alive (freed only by rust_clap_free or a failed load, after its last use).
        // A shared reference to it is sound on every thread at once because nothing in Host is written through one but
        // atomics, the queues (synchronized inside) and Owned fields, and each Owned field is touched by one thread at a
        // time, as the groups on Host divide them: the main thread's; the gated ones, written by the main thread holding
        // the gate and read by the gate's holder (the main thread, or the audio thread inside process()) and by the main
        // thread; the audio thread's, which the main thread touches only holding the gate while the plugin is not
        // processing. An Owned lends its field only to a closure that does not call into the plugin, the one way this
        // thread can reach the same field again. The one read outside these groups is Owned::racy, saying why.
        unsafe { self.0.as_ref() }
    }
}

/// A Host field one thread at a time reads and writes (the groups on Host): copied in and out, or lent to a closure for
/// the length of the call. Through it the shared Host never claims a field another thread may write.
#[repr(transparent)]
pub struct Owned<T>(UnsafeCell<T>);
impl<T> Owned<T> {
    pub const fn new(value: T) -> Owned<T> {
        Owned(UnsafeCell::new(value))
    }
    /// The field lent to f, which must not call into the plugin (or anything else that can reach this Host).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        // SAFETY: the groups on Host give this field to this thread now (H's invariant), and f is its only borrow: f
        // does not call into the plugin, whose callbacks are this thread's one other way to it.
        f(unsafe { &mut *self.0.get() })
    }
    pub fn replace(&self, value: T) -> T {
        self.with(|x| std::mem::replace(x, value))
    }
    /// The old value is dropped after the field is written (a library's deinit runs outside with).
    pub fn set(&self, value: T) {
        drop(self.replace(value));
    }
    /// Where the plugin is given the field to write (an out parameter, a buffer).
    pub fn as_ptr(&self) -> *mut T {
        self.0.get()
    }
}
impl<T: Copy> Owned<T> {
    pub fn get(&self) -> T {
        self.with(|x| *x)
    }
    /// The value read on a thread its group does not give it to.
    ///
    /// # Safety
    /// No write races with this read; each use says why, or says that it is not so.
    pub unsafe fn racy(&self) -> T {
        // SAFETY: the caller's.
        unsafe { *self.0.get() }
    }
}

// ---- The plugin's calls ----

/// The plugin a Host holds: made only from the Host's plugin field (instance, live) or a table it gave, used only while
/// the Host holds it (close() destroys it and clears the field, with no Plugin of it kept past that).
#[derive(Clone, Copy)]
pub struct Plugin(NonNull<clap_plugin>);
impl Plugin {
    fn table(&self) -> &clap_plugin {
        // SAFETY: Plugin's invariant: the live plugin's own clap_plugin.
        unsafe { self.0.as_ref() }
    }
    pub fn raw(self) -> *const clap_plugin {
        self.0.as_ptr()
    }
    /// A member of one of the plugin's tables called on it: its clap_plugin's (below), or one of its extension tables'.
    pub fn call<F: Member>(self, f: F, args: F::Args) -> F::Out {
        // SAFETY: the plugin is live (Plugin's invariant) and f is one of its own members (a None aborts, where Host.cpp's
        // call through null would crash); each call is made on the thread and in the state CLAP names for it, as Host.cpp
        // makes it, and its pointer arguments are references or the host's own buffers, valid for the call and sized as
        // passed.
        unsafe { f.invoke(self.raw(), args) }
    }
    pub fn init(self) -> bool {
        self.call(self.table().init, ())
    }
    pub fn destroy(self) {
        self.call(self.table().destroy, ())
    }
    pub fn activate(self, rate: f64, min: u32, max: u32) -> bool {
        self.call(self.table().activate, (rate, min, max))
    }
    pub fn deactivate(self) {
        self.call(self.table().deactivate, ())
    }
    pub fn start_processing(self) -> bool {
        self.call(self.table().start_processing, ())
    }
    pub fn stop_processing(self) {
        self.call(self.table().stop_processing, ())
    }
    pub fn reset(self) {
        self.call(self.table().reset, ())
    }
    pub fn process(self, block: &clap_process) -> clap_process_status {
        self.call(self.table().process, (block,))
    }
    pub fn on_main_thread(self) {
        self.call(self.table().on_main_thread, ())
    }
    /// The plugin's extension table of type T for this ID, or null.
    pub fn ext_ptr<T>(self, id: &CStr) -> *const T {
        self.call(self.table().get_extension, (id.as_ptr(),)).cast()
    }
    pub fn ext<T>(&self, id: &CStr) -> Option<&T> {
        // SAFETY: null, or the plugin's table of type T for this ID (CLAP), alive as long as the plugin.
        unsafe { self.ext_ptr::<T>(id).as_ref() }
    }
}
/// A function of one of the plugin's tables, its first argument the plugin; Plugin::call is its one caller.
pub trait Member {
    type Args;
    type Out;
    /// # Safety
    /// Plugin::call's.
    unsafe fn invoke(self, plugin: *const clap_plugin, args: Self::Args) -> Self::Out;
}
macro_rules! member { ($($a:ident)*) => {
    impl<R, $($a),*> Member for unsafe extern "C" fn(*const clap_plugin $(, $a)*) -> R {
        type Args = ($($a,)*);
        type Out = R;
        #[allow(non_snake_case)]
        unsafe fn invoke(self, plugin: *const clap_plugin, ($($a,)*): Self::Args) -> R {
            // SAFETY: the caller's.
            unsafe { self(plugin $(, $a)*) }
        }
    }
    impl<R, $($a),*> Member for Option<unsafe extern "C" fn(*const clap_plugin $(, $a)*) -> R> {
        type Args = ($($a,)*);
        type Out = R;
        unsafe fn invoke(self, plugin: *const clap_plugin, args: Self::Args) -> R {
            // SAFETY: the caller's.
            unsafe { self.unwrap().invoke(plugin, args) }
        }
    }
}}
member!();
member!(A);
member!(A B);
member!(A B C);
member!(A B C D);
member!(A B C D E);
member!(A B C D E F);
member!(A B C D E F G);
/// One of the plugin's extension tables, as get_extension gave it: null, or the live plugin's own (cleared in close()
/// with the plugin), in the group of the Host field it is.
#[repr(transparent)]
pub struct Ext<T>(Owned<*const T>);
impl<T> Ext<T> {
    pub fn get(&self) -> Option<&T> {
        // SAFETY: null, or the live plugin's table of type T, alive as long as the plugin (CLAP) and so past this borrow
        // of the Host: no borrow of a table outlives the call into the host it is made in.
        unsafe { self.0.get().as_ref() }
    }
    pub fn set(&self, table: *const T) {
        self.0.set(table)
    }
    pub fn is_null(&self) -> bool {
        self.0.get().is_null()
    }
}

pub struct Host {
    // ---- Fixed: written before the plugin is made, then only read, by any thread ----
    pub host: clap_host,
    main_thread: Owned<Option<std::thread::ThreadId>>, // the first control call's (main()), made before the plugin

    // ---- Any thread: atomics, and the queues ----
    hardware_epoch: AtomicU32,
    wanted_outputs: AtomicU64,
    outputs_told: AtomicBool,
    pub sleeping: AtomicBool,
    wants_beat: AtomicBool,
    wants_harmony: AtomicBool,
    harmony_sequence: AtomicU32,
    harmony_words: [AtomicU64; 4],
    processing: AtomicBool,
    pub pending_commands: AtomicU32,
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
    pub output_drops: AtomicU64,
    gui_created: AtomicBool,
    gui_show: AtomicBool,
    gui_hide: AtomicBool,
    gui_gone: AtomicBool,
    gui_hints: AtomicBool,
    gui_size: AtomicU64,
    commands: Queue<Event>, // producers: any thread; the consumer: the gate's holder
    emitted: Queue<Event>,  // the producer: the gate's holder; the consumer: the main thread

    // ---- The main thread's ----
    pub library: Owned<Option<Arc<Library>>>,
    ui: Ext<oscairn_plugin_ui_t>,
    ui_views: Owned<bool>,
    ui_seconds: Owned<bool>,
    state: Ext<clap_plugin_state>,
    exclusive_state: Owned<bool>,
    pub sounds: Ext<oscairn_plugin_sound_contexts_t>,
    pub honors_volume: Owned<bool>,
    asset_dependencies: Ext<oscairn_plugin_asset_dependencies_t>,
    asset_optional: Owned<bool>,
    pub assets: Owned<Option<crate::bindings::oscairn_clap_assets>>,
    window: Owned<*mut crate::bindings::kick_clap_window>,
    plugin_latency: Ext<clap_plugin_latency>,
    timer_support: Ext<clap_plugin_timer_support>,
    fd_support: Ext<clap_plugin_posix_fd_support>,
    memory: Ext<oscairn_plugin_memory_t>,
    pub has_declared: Owned<bool>,
    context: Owned<bool>,
    pub declared: Owned<oscairn_memory_t>,
    pub memory_room: Owned<u64>,
    pub offline: Owned<bool>,
    pub info: Owned<Descriptor>,
    pub message: Owned<String>,
    last_logged_error: Owned<String>,
    pub audio_layout_locked: Owned<bool>,
    pub metadata_version: Owned<u64>,
    timers: Owned<Vec<Timer>>,
    next_timer: Owned<clap_id>,
    fds: Owned<Vec<Fd>>,
    gui: Ext<clap_plugin_gui>,
    pub gui_api: Owned<*const c_char>,
    gui_floating: Owned<bool>,

    // ---- Gated: written by the main thread holding the gate; read by the gate's holder, and by the main thread ----
    pub plugin: Owned<*const clap_plugin>,
    params: Ext<clap_plugin_params>,
    pub sound_snapshots: Ext<oscairn_plugin_sound_snapshots_t>,
    // Also read by the audio thread between blocks (set_voice_limits, voice_report): it is written only as the plugin
    // initializes, before any audio thread has the host.
    pub voice_budget: Ext<oscairn_plugin_voice_budget_t>,
    ports_activation: Ext<clap_plugin_audio_ports_activation>,
    activate_while_processing: Owned<bool>,
    active: Owned<bool>,
    pub param_info: Owned<Vec<Parameter>>,
    cookies: Owned<Vec<*mut c_void>>,
    pub param_values: Owned<Box<[AtomicU64]>>, // the box gated, the values any thread's
    pub inputs: Owned<Vec<Port>>,
    pub outputs: Owned<Vec<Port>>,
    pub note_inputs: Owned<Vec<clap_note_port_info>>,
    pub note_outputs: Owned<Vec<clap_note_port_info>>,
    input_buffers: Owned<Vec<clap_audio_buffer>>,
    output_buffers: Owned<Vec<clap_audio_buffer>>,
    pub ring_out_channels: Owned<u32>,
    ring_out_buffer: Owned<clap_audio_buffer>,
    sample_rate: Owned<f64>,
    max_frames: Owned<u32>,
    sysex_output: Owned<Option<Box<[u8]>>>, // made by the main thread; its bytes the gate holder's

    // ---- The audio thread's (the main thread's holding the gate, the plugin not processing) ----
    limits_waiting: Owned<bool>,
    voice_limit: Owned<u32>,
    voice_context_count: Owned<u32>,
    active_voices: Owned<u32>,
    voice_note_count: Owned<u32>,
    voice_contexts: Owned<[oscairn_voice_budget_context_t; OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS as usize]>,
    voice_cost: Owned<f64>,
    voice_notes: Owned<Box<[oscairn_voice_budget_note_t; MAX_BLOCK_EVENTS]>>,
    applied_outputs: Owned<u64>,
    applied_told: Owned<bool>,
    input_pointers: Owned<Vec<*mut f32>>,
    output_pointers: Owned<Vec<*mut f32>>,
    scratch: Owned<Vec<f32>>,
    steady_time: Owned<i64>, // also close()'s, which runs only where no audio thread has the host (a load, a free)
    last_harmony: Owned<oscairn_harmony_t>,
    have_harmony: Owned<bool>,
    transport: Owned<TransportWatch>,
    // Set by the worker's control thread (mirror_wake) and read by any thread that wakes the plugin.
    wake_mirror: AtomicPtr<AtomicU32>,
    ordered: Owned<Box<[Timed; RENDER_EVENTS]>>,
    chunk_events: Owned<Box<[Event; RENDER_EVENTS]>>,
    pub carry_events: Owned<Box<[Event; MAX_BLOCK_EVENTS]>>,
    pub carry_count: AtomicU32, // written under the gate, read by restful() without: see there
    pub block_output: Owned<Box<[Event; MAX_BLOCK_EVENTS]>>,
    block_output_count: Owned<u32>,
    pub published_output_count: Owned<u32>,
    pub reset_in_process: Owned<bool>,
    sysex_used: Owned<u32>,
    in_process: Owned<bool>,
    chunk_count: Owned<u32>,
    output_frame_offset: Owned<u32>,
    current_process_frames: Owned<u32>,
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
pub struct Gate<'a>(&'a AtomicBool);
impl Gate<'_> {
    pub fn new(flag: &AtomicBool) -> Gate<'_> {
        while flag.swap(true, Acquire) {
            std::thread::yield_now();
        }
        Gate(flag)
    }
}
impl Drop for Gate<'_> {
    fn drop(&mut self) {
        self.0.store(false, Release);
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
/// An audio buffer with nothing in it (the C++ value-initializes them).
fn no_buffer() -> clap_audio_buffer {
    clap_audio_buffer { data32: null_mut(), data64: null_mut(), channel_count: 0, latency: 0, constant_mask: 0 }
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
    /// A new Host, its handle's until it is freed (Box::from_raw of its pointer).
    pub fn create() -> H {
        static NAME: &CStr = c"Oscairn";
        static URL: &CStr = c"https://github.com/oscairn/oscairn";
        // OSCAIRN_RELEASE, as the C++ host is built with it (CMake gives cargo the same).
        static VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
        let version = VERSION.get_or_init(|| CString::new(option_env!("OSCAIRN_RELEASE").unwrap_or("0.0.0")).unwrap_or_default());
        // Every field not written below is a pointer, number, flag, atomic or plain C struct (an Owned of one is the same
        // bytes), whose zero is its start (as the C++ value-initializes Impl); the owning fields and the few nonzero ones
        // are written.
        let mut z = Box::<Host>::new_zeroed();
        let p = z.as_mut_ptr();
        macro_rules! set { ($($f:ident: $v:expr),* $(,)?) => { $(
            // SAFETY: a field of the allocation, written once before it is read.
            unsafe { std::ptr::addr_of_mut!((*p).$f).write($v) };
        )* } }
        set! {
            host: clap_host {
                clap_version: CLAP_VERSION,
                host_data: null_mut(), // below, once the host has its place
                name: NAME.as_ptr(),
                vendor: NAME.as_ptr(),
                url: URL.as_ptr(),
                version: version.as_ptr(),
                get_extension: Some(extension),
                request_restart: Some(request_restart),
                request_process: Some(request_process),
                request_callback: Some(request_callback),
            },
            main_thread: Owned::new(None), library: Owned::new(None), assets: Owned::new(None), transport: Owned::new(TransportWatch::default()),
            info: Owned::new(Descriptor::default()), message: Owned::new(String::new()), last_logged_error: Owned::new(String::new()),
            param_info: Owned::new(Vec::new()), cookies: Owned::new(Vec::new()), param_values: Owned::new(Box::new([])),
            inputs: Owned::new(Vec::new()), outputs: Owned::new(Vec::new()), note_inputs: Owned::new(Vec::new()), note_outputs: Owned::new(Vec::new()),
            input_buffers: Owned::new(Vec::new()), output_buffers: Owned::new(Vec::new()), input_pointers: Owned::new(Vec::new()),
            output_pointers: Owned::new(Vec::new()), scratch: Owned::new(Vec::new()), timers: Owned::new(Vec::new()), fds: Owned::new(Vec::new()),
            sysex_output: Owned::new(None), commands: Queue::new(MAX_EVENTS), emitted: Queue::new(MAX_EVENTS),
            ordered: Owned::new(Box::new([Timed::default(); RENDER_EVENTS])), chunk_events: Owned::new(Box::new([Event::default(); RENDER_EVENTS])),
            carry_events: Owned::new(Box::new([Event::default(); MAX_BLOCK_EVENTS])), block_output: Owned::new(Box::new([Event::default(); MAX_BLOCK_EVENTS])),
            voice_notes: Owned::new(Box::new(std::mem::zeroed())), // plain old data
            sample_rate: Owned::new(48000.0), max_frames: Owned::new(512), current_process_frames: Owned::new(1), next_timer: Owned::new(1),
        }
        // SAFETY: every field is set (above, or valid as zeros).
        let host = Box::into_raw(unsafe { z.assume_init() });
        // SAFETY: the new host, reached by nothing else yet; the plugin is given this pointer back as host_data.
        unsafe {
            (*host).host.host_data = host.cast();
            H::new(host)
        }
    }
}

/// The host from the plugin's clap_host.
///
/// # Safety
/// h is a Host's own clap_host, as its plugin was given it, in a call CLAP lets the plugin make on this thread.
unsafe fn me(h: *const clap_host) -> H {
    // SAFETY: the caller's.
    unsafe { H::new((*h).host_data.cast()) }
}

/// Host::Impl::main: whether this is the host's main thread (the first to make a control call).
pub fn main(s: &Host) -> bool {
    let me = std::thread::current().id();
    if s.main_thread.get().is_none() {
        s.main_thread.set(Some(me));
    }
    if s.main_thread.get() == Some(me) {
        return true;
    }
    s.errors.fetch_add(1, SeqCst); // the text is not written here: this is not the main thread, and message is its own
    false
}
fn is_main_of(s: &Host) -> bool {
    s.main_thread.get() == Some(std::thread::current().id())
}

// ---- The host's callbacks and extensions ----
unsafe extern "C" fn is_main(h: *const clap_host) -> bool {
    // SAFETY: the plugin's own host (and so in every callback below).
    let s = unsafe { me(h) };
    is_main_of(&s)
}
unsafe extern "C" fn is_audio(h: *const clap_host) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    AUDIO_SCOPE.with(|a| a.get() == s.ptr().cast_const())
}
unsafe extern "C" fn request_restart(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { me(h) }.restart.store(true, SeqCst)
}
unsafe extern "C" fn request_callback(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { me(h) }.callback.store(true, SeqCst)
}
unsafe extern "C" fn request_process(h: *const clap_host) {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    wake_up(&s)
}
unsafe extern "C" fn request_flush(h: *const clap_host) {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    s.flush.store(true, SeqCst);
    wake_up(&s);
}
pub fn wake_up(s: &Host) {
    s.wake.store(true, SeqCst);
    let mirror = s.wake_mirror.load(SeqCst);
    if !mirror.is_null() {
        // SAFETY: the mirror outlives its setting (Host.h).
        unsafe { (*mirror).store(1, Release) };
    }
}
unsafe extern "C" fn rescan_params(h: *const clap_host, flags: clap_param_rescan_flags) {
    // SAFETY: as is_main.
    unsafe { me(h) }.rescan.fetch_or(flags, SeqCst);
}
unsafe extern "C" fn clear_params(_: *const clap_host, _: clap_id, _: clap_param_clear_flags) {}
unsafe extern "C" fn mark_dirty(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { me(h) }.dirty.store(true, SeqCst)
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
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if severity == CLAP_LOG_FATAL || severity == CLAP_LOG_PLUGIN_MISBEHAVING {
        s.errors.fetch_add(1, SeqCst);
    }
    if severity == CLAP_LOG_FATAL {
        s.failed.store(true, SeqCst);
    }
    if severity == CLAP_LOG_ERROR && !text.is_null() && is_main_of(&s) {
        // SAFETY: the plugin's NUL-terminated text, read as strnlen(text, 512).
        let bytes = unsafe { std::slice::from_raw_parts(text.cast::<u8>(), (0..512).take_while(|&i| *text.add(i) != 0).count()) };
        s.last_logged_error.set(String::from_utf8_lossy(bytes).into_owned());
    }
}
unsafe extern "C" fn query_event_space(h: *const clap_host, name: *const c_char, space: *mut u16) -> bool {
    if name.is_null() || space.is_null() {
        return false;
    }
    // SAFETY: as is_main; name the plugin's NUL-terminated string.
    let (s, name) = unsafe { (me(h), CStr::from_ptr(name)) };
    let given = if name == ids::OSCAIRN_EVENT_SPACE_BEAT {
        s.wants_beat.store(true, SeqCst);
        BEAT_SPACE
    } else if name == ids::OSCAIRN_EVENT_SPACE_HARMONY {
        s.wants_harmony.store(true, SeqCst);
        HARMONY_SPACE
    } else if name == ids::OSCAIRN_EVENT_SPACE_PARAM_TARGET {
        PARAM_TARGET_SPACE
    } else {
        return false;
    };
    // SAFETY: the plugin's pointer, checked.
    unsafe { *space = given };
    true
}
unsafe extern "C" fn register_timer(h: *const clap_host, ms: u32, id: *mut clap_id) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) || id.is_null() || s.timers.with(|t| t.len()) >= 128 {
        return false;
    }
    let given = s.next_timer.get();
    s.next_timer.set(given + 1);
    // SAFETY: the plugin's pointer, checked.
    unsafe { *id = given };
    let period = ms.max(1);
    s.timers.with(|t| t.push(Timer { id: given, period, next: Instant::now() + Duration::from_millis(period.into()) }));
    true
}
unsafe extern "C" fn unregister_timer(h: *const clap_host, id: clap_id) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) {
        return false;
    }
    s.timers.with(|t| {
        let before = t.len();
        t.retain(|t| t.id != id);
        before != t.len()
    })
}
unsafe extern "C" fn register_fd(h: *const clap_host, fd: i32, flags: clap_posix_fd_flags) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) || fd < 0 || s.fds.with(|f| f.len()) >= 128 || flags & !(CLAP_POSIX_FD_READ | CLAP_POSIX_FD_WRITE | CLAP_POSIX_FD_ERROR) != 0 {
        return false;
    }
    s.fds.with(|fds| {
        if fds.iter().any(|f| f.fd == fd) {
            return false;
        }
        fds.push(Fd { fd, flags });
        true
    })
}
unsafe extern "C" fn modify_fd(h: *const clap_host, fd: i32, flags: clap_posix_fd_flags) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) {
        return false;
    }
    s.fds.with(|fds| match fds.iter_mut().find(|f| f.fd == fd) {
        Some(item) => {
            item.flags = flags;
            true
        }
        None => false,
    })
}
unsafe extern "C" fn unregister_fd(h: *const clap_host, fd: i32) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) {
        return false;
    }
    s.fds.with(|fds| {
        let before = fds.len();
        fds.retain(|f| f.fd != fd);
        before != fds.len()
    })
}
/// oscairn.assets' failures, written into the plugin's buffer as snprintf would.
///
/// # Safety
/// message is null or the plugin's buffer of capacity bytes.
unsafe fn report(message: *mut c_char, capacity: u32, text: &str) -> bool {
    if !message.is_null() && capacity > 0 {
        let n = text.len().min(capacity as usize - 1);
        // SAFETY: the caller's.
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
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) {
        // SAFETY: the plugin's buffers, of the sizes it gives (and so below); the resolver's table is complete (clap_host.h).
        return unsafe { report(message, size, "assets resolve on the main thread only") };
    }
    if r.is_null() || path.is_null() || capacity == 0 {
        // SAFETY: as above.
        return unsafe { report(message, size, "invalid asset request") };
    }
    match s.assets.get() {
        // SAFETY: as above.
        Some(a) => unsafe { a.resolve.unwrap()(a.context, r, path, capacity, message, size) },
        // SAFETY: as above.
        None => unsafe { report(message, size, "this host has no sound asset store") },
    }
}
unsafe extern "C" fn create_asset(h: *const clap_host, file: *const c_char, r: *mut oscairn_asset_ref_t, message: *mut c_char, size: u32) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) {
        // SAFETY: as resolve_asset.
        return unsafe { report(message, size, "assets are created on the main thread only") };
    }
    if file.is_null() || r.is_null() {
        // SAFETY: as resolve_asset.
        return unsafe { report(message, size, "invalid asset request") };
    }
    match s.assets.get() {
        // SAFETY: as resolve_asset.
        Some(a) => unsafe { a.create.unwrap()(a.context, file, r, message, size) },
        // SAFETY: as resolve_asset.
        None => unsafe { report(message, size, "this host has no sound asset store") },
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
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !is_main_of(&s) {
        // SAFETY: as resolve_asset.
        return unsafe { report(message, size, "samples resolve on the main thread only") };
    }
    if r.is_null() || path.is_null() || capacity == 0 || location.is_null() {
        // SAFETY: as resolve_asset.
        return unsafe { report(message, size, "invalid sample request") };
    }
    match s.assets.get() {
        // SAFETY: as resolve_asset.
        Some(a) => unsafe { a.resolve_sample.unwrap()(a.context, r, path, capacity, location, message, size) },
        // SAFETY: as resolve_asset.
        None => unsafe { report(message, size, "this host has no sample store") },
    }
}
unsafe extern "C" fn library_root(owner: *const c_void, library: *const c_char, min: *const c_char, path: *mut c_char, capacity: u32) -> bool {
    let h = owner.cast::<clap_host>();
    // SAFETY: the owner is the host the plugin was given (oscairn.assets draft 2).
    if h.is_null() || !unsafe { is_main(h) } || library.is_null() || path.is_null() || capacity == 0 {
        return false;
    }
    // SAFETY: as above; the plugin's strings and buffer, the resolver's table.
    unsafe { me(h) }.assets.get().is_some_and(|a| unsafe { a.library_root.unwrap()(a.context, library, min, path, capacity) })
}
unsafe extern "C" fn preset_error(h: *const clap_host, _: u32, _: *const c_char, _: *const c_char, _: i32, message: *const c_char) {
    // SAFETY: as is_main; the message the plugin's NUL-terminated string, or null.
    let (s, why) = unsafe { (me(h), (!message.is_null()).then(|| CStr::from_ptr(message))) };
    s.message.set(match why {
        None => "Preset not loaded".into(),
        Some(why) => format!("Preset not loaded: {}", why.to_string_lossy()),
    });
}
unsafe extern "C" fn preset_loaded(h: *const clap_host, _: u32, _: *const c_char, _: *const c_char) {
    // SAFETY: as is_main.
    unsafe { me(h) }.message.with(String::clear)
}
unsafe extern "C" fn gui_hints_changed(h: *const clap_host) {
    // SAFETY: as is_main.
    unsafe { me(h) }.gui_hints.store(true, SeqCst)
}
unsafe extern "C" fn gui_request_resize(h: *const clap_host, width: u32, height: u32) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    // SAFETY: gui_floating, the main thread's, read here on any thread: it is written only as the plugin initializes,
    // before show_gui sets gui_created (SeqCst), which this read follows.
    if width == 0 || height == 0 || !s.gui_created.load(SeqCst) || unsafe { s.gui_floating.racy() } {
        return false;
    }
    if !is_main_of(&s) {
        s.gui_size.store((u64::from(width) << 32) | u64::from(height), SeqCst);
        return true;
    }
    let window = s.window.get();
    if window.is_null() {
        return false;
    }
    // SAFETY: the host's own window.
    unsafe { bridge::kick_clap_window_size(window, width, height) };
    true
}
unsafe extern "C" fn gui_request_show(h: *const clap_host) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !s.gui_created.load(SeqCst) {
        return false;
    }
    s.gui_show.store(true, SeqCst);
    true
}
unsafe extern "C" fn gui_request_hide(h: *const clap_host) -> bool {
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    if !s.gui_created.load(SeqCst) {
        return false;
    }
    s.gui_hide.store(true, SeqCst);
    true
}
unsafe extern "C" fn gui_closed(h: *const clap_host, destroyed: bool) {
    if destroyed {
        // SAFETY: as is_main.
        unsafe { me(h) }.gui_gone.store(true, SeqCst)
    }
}
unsafe extern "C" fn read_harmony(h: *const clap_host, out: *mut oscairn_harmony_t) -> bool {
    if out.is_null() {
        return false;
    }
    // SAFETY: as is_main.
    let s = unsafe { me(h) };
    let mut words = [0u64; 4];
    loop {
        let before = s.harmony_sequence.load(Acquire);
        for (w, a) in words.iter_mut().zip(&s.harmony_words) {
            *w = a.load(Relaxed);
        }
        std::sync::atomic::fence(Acquire);
        let after = s.harmony_sequence.load(Relaxed);
        if before & 1 == 0 && before == after {
            break;
        }
    }
    if words[3] == 0 {
        return false;
    }
    // SAFETY: the plugin's harmony, three words.
    unsafe { std::ptr::copy_nonoverlapping(words.as_ptr().cast::<u8>(), out.cast::<u8>(), size_of::<oscairn_harmony_t>()) };
    true
}
use std::mem::size_of;
/// None: in no part.
fn publish_harmony(s: &Host, h: Option<&oscairn_harmony_t>) {
    let mut words = [0u64; 4];
    if let Some(h) = h {
        // SAFETY: a harmony is three words.
        unsafe { std::ptr::copy_nonoverlapping(std::ptr::from_ref(h).cast::<u8>(), words.as_mut_ptr().cast::<u8>(), size_of::<oscairn_harmony_t>()) };
        words[3] = 1;
    }
    let n = s.harmony_sequence.load(Relaxed);
    s.harmony_sequence.store(n + 1, Relaxed);
    std::sync::atomic::fence(Release);
    for (a, w) in s.harmony_words.iter().zip(words) {
        a.store(w, Relaxed);
    }
    s.harmony_sequence.store(n + 2, Release);
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
    // SAFETY: as hardware_get; the plugin's own host.
    unsafe { bridge::kick_clap_hardware_reserve(me(h).ptr().cast(), index, request, lease) }
}
unsafe extern "C" fn hardware_release(h: *const clap_host, lease: u32) {
    // SAFETY: as hardware_reserve.
    unsafe { bridge::kick_clap_hardware_release(me(h).ptr().cast(), lease) }
}
unsafe extern "C" fn hardware_submit(h: *const clap_host, lease: u32, job: *const oscairn_hardware_job_t) -> i32 {
    // SAFETY: as hardware_reserve.
    let s = unsafe { me(h) };
    // SAFETY: as hardware_get.
    unsafe { bridge::kick_clap_hardware_submit(s.ptr().cast(), s.hardware_epoch.load(Relaxed), lease, job) }
}
unsafe extern "C" fn hardware_collect(h: *const clap_host, lease: u32, result: *mut oscairn_hardware_result_t) -> i32 {
    // SAFETY: as hardware_reserve.
    let s = unsafe { me(h) };
    // SAFETY: as hardware_get.
    unsafe { bridge::kick_clap_hardware_collect(s.ptr().cast(), s.hardware_epoch.load(Relaxed), lease, result) }
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

impl Host {
    /// The plugin, where there is one.
    pub fn instance(&self) -> Option<Plugin> {
        NonNull::new(self.plugin.get().cast_mut()).map(Plugin)
    }
    /// The plugin, which the caller knows there is (as Host.cpp's caller does, which would crash on none).
    pub fn live(&self) -> Plugin {
        self.instance().expect("the host's plugin")
    }
}
/// The plugin's extension table of type T, or null.
fn ext<T>(s: &Host, id: &CStr) -> *const T {
    s.live().ext_ptr(id)
}

// ---- Loading, activation, the description ----

pub fn param_index(s: &Host, id: clap_id) -> usize {
    s.param_info.with(|info| info.binary_search_by(|p| p.id.cmp(&id)).unwrap_or(info.len()))
}

/// The main thread's; also run by process (the audio scope).
pub fn stop(s: H) {
    let _scope = AudioScope::new(s.ptr());
    if s.processing.load(SeqCst) {
        s.live().stop_processing();
    }
    s.processing.store(false, SeqCst);
    s.sleeping.store(false, SeqCst);
}
pub fn deactivate(s: H) {
    stop(s);
    if s.active.get() {
        s.live().deactivate();
    }
    s.active.set(false);
}
/// The main thread's, under the gate.
pub fn close(s: H) {
    s.audio_layout_locked.set(false);
    s.loaded.store(false, SeqCst);
    if !s.plugin.get().is_null() {
        destroy_gui(s);
    }
    s.gui.set(null());
    s.gui_api.set(null());
    if let Some(p) = s.instance() {
        deactivate(s);
        if let Some(destroy) = p.table().destroy {
            p.call(destroy, ()); // destroyed once: its field cleared below
        }
    }
    // SAFETY: this host as the owner of its leases.
    unsafe { bridge::kick_clap_hardware_release_all(s.ptr().cast()) }; // what the plugin did not release
    s.plugin.set(null());
    s.params.set(null());
    s.state.set(null());
    s.sounds.set(null());
    s.asset_dependencies.set(null());
    s.plugin_latency.set(null());
    s.timer_support.set(null());
    s.fd_support.set(null());
    s.memory.set(null());
    s.has_declared.set(false);
    s.ports_activation.set(null());
    s.timers.with(Vec::clear);
    s.fds.with(Vec::clear);
    s.param_info.with(Vec::clear);
    s.param_values.set(Box::new([]));
    s.cookies.with(Vec::clear);
    s.inputs.with(Vec::clear);
    s.outputs.with(Vec::clear);
    s.note_inputs.with(Vec::clear);
    s.note_outputs.with(Vec::clear);
    s.input_buffers.with(Vec::clear);
    s.output_buffers.with(Vec::clear);
    s.input_pointers.with(Vec::clear);
    s.output_pointers.with(Vec::clear);
    s.scratch.with(Vec::clear);
    s.library.set(None);
    while s.commands.pop().is_some() {
        s.pending_commands.fetch_sub(1, SeqCst);
    }
    while s.emitted.pop().is_some() {}
    for f in [&s.failed, &s.dirty, &s.restart, &s.callback, &s.wake, &s.flush, &s.panic] {
        f.store(false, SeqCst);
    }
    s.rescan.store(0, SeqCst);
    s.latency_frames.store(0, SeqCst);
    s.steady_time.set(0);
    s.transport.with(TransportWatch::reset);
    s.carry_count.store(0, Relaxed);
    s.metadata_version.set(s.metadata_version.get() + 1);
}

/// Host::load after its gate: the library opened, the plugin made and initialized. The main thread's, under the gate,
/// the host closed.
pub fn load(s: H, path: &str, id: &CStr, rate: f64, frames: u32) -> R<()> {
    let library = library::open(path)?;
    s.library.set(Some(library.clone()));
    let factory = library.factory().ok_or("No CLAP plugin factory")?;
    let mut chosen = null();
    // SAFETY: the library's factory, checked complete, and the descriptors it gives (each checked before it is read).
    unsafe {
        for i in 0..factory.get_plugin_count.unwrap()(factory).min(4096) {
            let d = factory.get_plugin_descriptor.unwrap()(factory, i);
            if !d.is_null() && !(*d).id.is_null() && !(*d).name.is_null() && same_id((*d).id, id) && clap_version_is_compatible((*d).clap_version) {
                chosen = d;
                break;
            }
        }
    }
    if chosen.is_null() {
        return Err("Saved plugin ID is absent from this binary".into());
    }
    // SAFETY: as above.
    let chosen = unsafe { &*chosen };
    s.info.set(describe(&library.path, chosen));
    // SAFETY: as above; the host's own clap_host, which outlives the plugin.
    s.plugin.set(unsafe { factory.create_plugin.unwrap()(factory, &s.host, chosen.id) });
    initialize(s, rate, frames)
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

/// As load.
pub fn initialize(s: H, rate: f64, frames: u32) -> R<()> {
    let Some(p) = s.instance().filter(|p| p.table().init.is_some() && p.table().destroy.is_some()) else {
        return Err("Incomplete CLAP plugin interface".into());
    };
    if !p.init() {
        return Err("Plugin initialization failed".into());
    }
    // The rest only once initialized: plugins built on clap-helpers fill them in during init.
    let q = p.table();
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
    s.params.set(ext(&s, CLAP_EXT_PARAMS));
    s.ui.set(ext(&s, ids::OSCAIRN_EXT_UI));
    if s.ui.is_null() {
        s.ui.set(ext(&s, ids::OSCAIRN_EXT_UI_DRAFT_5)); // its prefix: no tabs or icons (not read yet)
    }
    s.ui_seconds.set(!s.ui.is_null());
    if s.ui.is_null() {
        s.ui.set(ext(&s, ids::OSCAIRN_EXT_UI_DRAFT_4)); // its prefix: no seconds()
    }
    s.ui_views.set(!s.ui.is_null());
    if s.ui.is_null() {
        s.ui.set(ext(&s, ids::OSCAIRN_EXT_UI_DRAFT_3)); // its prefix: no views
    }
    if s.ui.get().is_some_and(|ui| ui.param_hint.is_none()) {
        s.ui.set(null());
    }
    if let Some(params) = s.params.get() {
        if params.count.is_none() || params.get_info.is_none() || params.get_value.is_none() || params.flush.is_none() {
            return Err("Incomplete CLAP parameter interface".into());
        }
    }
    s.sounds.set(ext(&s, ids::OSCAIRN_EXT_SOUND_CONTEXTS));
    if let Some(x) = s.sounds.get() {
        if x.capacity.is_none() || x.get.is_none() || x.prepare.is_none() || x.select.is_none() || x.discard.is_none() || p.call(x.capacity, ()) > 256 {
            return Err("Invalid sound-context interface".into());
        }
    }
    s.state.set(ext(&s, CLAP_EXT_STATE));
    let sounds = s.sounds.get();
    s.exclusive_state.set(sounds.is_some_and(|x| !x.state.is_null()));
    if s.exclusive_state.get() {
        s.state.set(sounds.map_or(null(), |x| x.state));
        s.sound_snapshots.set(ext(&s, ids::OSCAIRN_EXT_SOUND_SNAPSHOTS));
    }
    if let Some(x) = s.sound_snapshots.get() {
        if x.prepare.is_none() || x.ring_out.is_none() != x.choke_retiring.is_none() {
            return Err("Invalid sound-snapshot interface".into());
        }
    }
    s.voice_budget.set(ext(&s, ids::OSCAIRN_EXT_VOICE_BUDGET));
    let budget = s.voice_budget.get();
    if let Some(x) = budget {
        if x.honors_volume.is_none() || x.set_limits.is_none() || x.active_voices.is_none() || x.notes.is_none() {
            return Err("Invalid voice-budget interface".into());
        }
    }
    s.honors_volume.set(budget.is_some_and(|x| p.call(x.honors_volume, ())));
    if let Some(x) = s.state.get() {
        if x.save.is_none() || x.load.is_none() {
            return Err("Incomplete plugin state interface".into());
        }
    }
    s.asset_dependencies.set(ext(&s, ids::OSCAIRN_EXT_ASSET_DEPENDENCIES));
    s.asset_optional.set(!s.asset_dependencies.is_null());
    if s.asset_dependencies.is_null() {
        s.asset_dependencies.set(ext(&s, ids::OSCAIRN_EXT_ASSET_DEPENDENCIES_DRAFT_1));
        // its first two members only
    }
    let dependencies = s.asset_dependencies.get();
    if let Some(x) = dependencies {
        if x.count.is_none() || x.get.is_none() {
            return Err("Incomplete asset-dependency interface".into());
        }
    }
    // draft 2 without optional: every asset required (Host.cpp reads the table only when asset_optional is set)
    s.asset_optional.set(s.asset_optional.get() && dependencies.is_some_and(|x| x.optional.is_some()));
    s.plugin_latency.set(ext(&s, CLAP_EXT_LATENCY));
    s.timer_support.set(ext(&s, CLAP_EXT_TIMER_SUPPORT));
    s.fd_support.set(ext(&s, CLAP_EXT_POSIX_FD_SUPPORT));
    s.memory.set(ext(&s, ids::OSCAIRN_EXT_MEMORY));
    if let Some(x) = s.memory.get() {
        if x.estimate.is_none() || x.current.is_none() {
            return Err("Incomplete memory interface".into());
        }
    }
    s.ports_activation.set(ext(&s, CLAP_EXT_AUDIO_PORTS_ACTIVATION));
    if s.ports_activation.is_null() {
        s.ports_activation.set(ext(&s, CLAP_EXT_AUDIO_PORTS_ACTIVATION_COMPAT));
    }
    if s.ports_activation.get().is_some_and(|x| x.set_active.is_none()) {
        s.ports_activation.set(null()); // incomplete: told nothing
    }
    let while_processing = s.ports_activation.get().is_some_and(|x| x.can_activate_while_processing.is_some_and(|f| p.call(f, ())));
    s.activate_while_processing.set(while_processing);
    s.applied_told.set(false);
    find_gui(s);
    // Initialized and not yet processing, on the main thread, as clap.render asks.
    if let Some(r) = p.ext::<clap_plugin_render>(CLAP_EXT_RENDER) {
        if let Some(set) = r.set {
            let hard = r.has_hard_realtime_requirement.is_some_and(|f| p.call(f, ()));
            p.call(set, (if s.offline.get() && !hard { CLAP_RENDER_OFFLINE } else { CLAP_RENDER_REALTIME },));
        }
    }
    s.sample_rate.set(rate);
    s.max_frames.set(frames);
    activate(s)?;
    s.loaded.store(true, SeqCst);
    s.dirty.store(false, SeqCst);
    Ok(())
}

/// Embedded where the host has windows of its own (macOS, Cocoa), else floating where the plugin floats one. The main
/// thread's, a live plugin.
fn find_gui(s: H) {
    s.gui.set(ext(&s, CLAP_EXT_GUI));
    s.gui_api.set(null());
    s.gui_floating.set(false);
    let Some(g) = s.gui.get() else { return };
    if g.is_api_supported.is_none() || g.create.is_none() || g.destroy.is_none() || g.show.is_none() || g.hide.is_none() {
        s.gui.set(null());
        return;
    }
    let p = s.live();
    // SAFETY: a plain call; the API name is static, NUL-terminated.
    let api = unsafe { bridge::kick_clap_window_api() };
    // SAFETY: as above.
    let window: Option<&CStr> = (!api.is_null()).then(|| unsafe { CStr::from_ptr(api) });
    let platform = if cfg!(target_os = "macos") {
        CLAP_WINDOW_API_COCOA
    } else if cfg!(windows) {
        CLAP_WINDOW_API_WIN32
    } else {
        CLAP_WINDOW_API_X11
    };
    if let Some(api) = window.filter(|api| g.get_size.is_some() && g.set_parent.is_some() && p.call(g.is_api_supported, (api.as_ptr(), false))) {
        s.gui_api.set(api.as_ptr());
    } else if p.call(g.is_api_supported, (platform.as_ptr(), true)) {
        s.gui_api.set(platform.as_ptr());
        s.gui_floating.set(true);
    }
}
/// Host.cpp's showGui: the editor made (embedded in a window of the host's own, or floating), titled and shown. The
/// main thread's; a plugin with a window here.
pub fn show_gui(s: H, title: &CStr) -> R<()> {
    unsafe extern "C" fn closing(c: *mut c_void) {
        // SAFETY: the window's host, alive while its window is.
        unsafe { H::new(c.cast()) }.gui_gone.store(true, SeqCst)
    }
    unsafe extern "C" fn resized(c: *mut c_void, w: *mut u32, h: *mut u32) {
        // SAFETY: as closing, on the main thread.
        let s = unsafe { H::new(c.cast()) };
        let (p, g) = (s.live(), s.gui.get().expect("the plugin's gui"));
        if let Some(adjust) = g.adjust_size {
            p.call(adjust, (w, h)); // the window's own sizes
        }
        if let Some(set) = g.set_size {
            // SAFETY: the window's own sizes, for the call.
            let (width, height) = unsafe { (*w, *h) };
            p.call(set, (width, height));
        }
    }
    let (p, g) = (s.live(), s.gui.get().expect("the plugin's gui"));
    if !s.gui_created.load(SeqCst) {
        // SAFETY: a plain call.
        unsafe { bridge::kick_clap_start_windows() };
        if !p.call(g.create, (s.gui_api.get(), s.gui_floating.get())) {
            return Err("Plugin could not open its window".into());
        }
        s.gui_created.store(true, SeqCst);
        if !s.gui_floating.get() {
            let (mut width, mut height) = (0, 0);
            if !p.call(g.get_size, (&mut width, &mut height)) || width == 0 || height == 0 {
                (width, height) = (640, 480);
            }
            let resizable = g.can_resize.is_some_and(|f| p.call(f, ()));
            // SAFETY: the title NUL-terminated; the window calls back with this host, which outlives it (destroy_gui frees it).
            let window = unsafe { bridge::kick_clap_window_create(title.as_ptr(), width, height, resizable, s.ptr().cast(), Some(closing), Some(resized)) };
            s.window.set(window);
            // SAFETY: the host's own window (each window call below says the same).
            let parent = clap_window { api: s.gui_api.get(), specific: clap_window_handle { ptr: unsafe { bridge::kick_clap_window_view(window) } } };
            if !p.call(g.set_parent, (&parent,)) {
                return Err("Plugin could not open its window in the host's".into());
            }
        }
    }
    let window = s.window.get();
    if !window.is_null() {
        // SAFETY: as above.
        unsafe { bridge::kick_clap_window_title(window, title.as_ptr()) };
    } else if let Some(suggest) = g.suggest_title {
        p.call(suggest, (title.as_ptr(),));
    }
    // An embedded editor shows with its window; a floating one must show itself.
    if !p.call(g.show, ()) && s.gui_floating.get() {
        return Err("Plugin could not show its window".into());
    }
    if !window.is_null() {
        // SAFETY: as above.
        unsafe { bridge::kick_clap_window_show(window, true) };
    }
    Ok(())
}
/// The main thread's.
pub fn destroy_gui(s: H) {
    if s.gui_created.swap(false, SeqCst) {
        s.live().call(s.gui.get().expect("the plugin's gui").destroy, ());
    }
    let window = s.window.get();
    if !window.is_null() {
        // SAFETY: the host's own window, freed once.
        unsafe { bridge::kick_clap_window_free(window) };
        s.window.set(null_mut());
    }
    for f in [&s.gui_show, &s.gui_hide, &s.gui_gone, &s.gui_hints] {
        f.store(false, SeqCst);
    }
    s.gui_size.store(0, SeqCst);
}
/// The main thread's.
fn service_gui(s: H) {
    if !s.gui_created.load(SeqCst) {
        return;
    }
    if s.gui_gone.load(SeqCst) {
        destroy_gui(s);
        return;
    }
    let (p, g) = (s.live(), s.gui.get().expect("the plugin's gui"));
    let w = s.window.get();
    let size = s.gui_size.swap(0, SeqCst);
    if size != 0 && !w.is_null() {
        // SAFETY: the host's own window (and so below).
        unsafe { bridge::kick_clap_window_size(w, (size >> 32) as u32, size as u32) };
    }
    if s.gui_hints.swap(false, SeqCst) && !w.is_null() {
        if let Some(f) = g.can_resize {
            let resizable = p.call(f, ());
            // SAFETY: as above.
            unsafe { bridge::kick_clap_window_resizable(w, resizable) };
        }
    }
    if s.gui_hide.swap(false, SeqCst) {
        p.call(g.hide, ());
        if !w.is_null() {
            // SAFETY: as above.
            unsafe { bridge::kick_clap_window_show(w, false) };
        }
    }
    if s.gui_show.swap(false, SeqCst) {
        p.call(g.show, ());
        if !w.is_null() {
            // SAFETY: as above.
            unsafe { bridge::kick_clap_window_show(w, true) };
        }
    }
}

/// The main thread's.
fn read_parameters(s: H) -> R<()> {
    struct Item {
        info: Parameter,
        cookie: *mut c_void,
        value: f64,
    }
    let mut items = Vec::new();
    if let Some(params) = s.params.get() {
        let p = s.live();
        let count = p.call(params.count, ());
        if count > 32768 {
            return Err("Plugin advertises too many parameters".into());
        }
        for i in 0..count {
            // SAFETY: plain old data, all zeros valid; then as above.
            let mut q: clap_param_info = unsafe { std::mem::zeroed() };
            if !p.call(params.get_info, (i, &mut q)) || q.id == CLAP_INVALID_ID {
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
            p.call(params.get_value, (q.id, &mut value));
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
    s.param_info.with(Vec::clear);
    s.cookies.with(Vec::clear);
    s.param_values.set(items.iter().map(|_| AtomicU64::new(0)).collect());
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 && s.param_info.with(|info| info[i - 1].id == item.info.id) {
            return Err("Duplicate CLAP parameter ID".into());
        }
        s.param_info.with(|info| info.push(item.info));
        s.cookies.with(|c| c.push(item.cookie));
        s.param_values.with(|v| f64_store(&v[i], item.value));
    }
    let mut list = s.param_info.replace(Vec::new());
    apply_hints(s, &mut list);
    s.param_info.set(list);
    s.metadata_version.set(s.metadata_version.get() + 1);
    Ok(())
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

/// Hints from oscairn.ui, each kept only where it fits its parameter (Host.cpp's applyHints). The main thread's.
fn apply_hints(s: H, list: &mut [Parameter]) {
    // algorithm() is read only from draft 4 (ui_views).
    let Some(ui) = s.ui.get() else { return };
    let p = s.live();
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
        if !p.call(ui.param_hint, (q.id, &mut hint)) {
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
        if !s.ui_views.get() {
            continue;
        }
        let f = hint.field;
        let field = (OSCAIRN_FIELD_DELAY..=OSCAIRN_FIELD_RELEASE).contains(&f)
            || (OSCAIRN_FIELD_LEVEL_0..=OSCAIRN_FIELD_LEVEL_8).contains(&f)
            || (OSCAIRN_FIELD_TIME_1..=OSCAIRN_FIELD_RATE_8).contains(&f)
            || (OSCAIRN_FIELD_KEY_LOW..=OSCAIRN_FIELD_VELOCITY_CURVE).contains(&f)
            || (OSCAIRN_FIELD_ALGORITHM..=OSCAIRN_FIELD_FIXED).contains(&f);
        let last = if f >= OSCAIRN_FIELD_ALGORITHM { 8 } else { 32 }; // an operator's index is 1 to 8, an element's or a zone's to 32 (draft 6)
        let in_view = hint.view != 0 && field && hint.index <= last && (f != OSCAIRN_FIELD_ALGORITHM || (stepped && (1..=128).contains(&values)));
        q.view = if in_view { hint.view } else { 0 };
        q.field = if in_view { f } else { 0 };
        q.index = if in_view { hint.index } else { 0 };
        read_seconds(s, q, values);
        let Some(algorithm) = ui.algorithm.filter(|_| q.field == OSCAIRN_FIELD_ALGORITHM) else { continue };
        // Its routings, all or none; operators past the eighth are dropped.
        for v in 0..values {
            // SAFETY: plain old data, all zeros valid; then as above.
            let mut a: oscairn_fm_algorithm_t = unsafe { std::mem::zeroed() };
            if !p.call(algorithm, (q.id, (q.minimum.round() as i64 + v) as u32, &mut a)) {
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
/// A time or rate field's seconds (Host.cpp's readSeconds): from seconds() (draft 5), else from value texts. The main
/// thread's.
fn read_seconds(s: H, q: &mut Parameter, values: i64) {
    let f = q.field;
    if !(((OSCAIRN_FIELD_DELAY..=OSCAIRN_FIELD_RELEASE).contains(&f) && f != OSCAIRN_FIELD_SUSTAIN)
        || (OSCAIRN_FIELD_TIME_1..=OSCAIRN_FIELD_RATE_8).contains(&f))
    {
        return;
    }
    let p = s.live();
    // seconds() is read only from draft 5.
    let asked = if s.ui_seconds.get() { s.ui.get().expect("the plugin's ui").seconds } else { None };
    let to_text = s.params.get().and_then(|x| x.value_to_text);
    if asked.is_none() && to_text.is_none() {
        return;
    }
    let n = if (2..=128).contains(&values) { values } else { 33 };
    for k in 0..n {
        let value = q.minimum + (q.maximum - q.minimum) * k as f64 / (n - 1) as f64;
        let mut seconds = -1.0;
        if let Some(f) = asked {
            if !p.call(f, (q.id, value, &mut seconds)) {
                seconds = -1.0;
            }
        } else {
            let mut text = [0 as c_char; 256];
            // the buffer holds 255 and a NUL
            if p.call(to_text, (q.id, value, text.as_mut_ptr(), 255)) {
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
/// The main thread's.
fn read_parameter_info(s: H) -> bool {
    let Some(params) = s.params.get() else { return false };
    let p = s.live();
    if p.call(params.count, ()) as usize != s.param_info.with(|v| v.len()) {
        return false;
    }
    let mut next = s.param_info.with(|v| v.clone());
    let changeable = CLAP_PARAM_IS_HIDDEN | CLAP_PARAM_IS_PERIODIC;
    for i in 0..next.len() as u32 {
        // SAFETY: plain old data, all zeros valid; then as above.
        let mut q: clap_param_info = unsafe { std::mem::zeroed() };
        if !p.call(params.get_info, (i, &mut q)) {
            return false;
        }
        let at = param_index(&s, q.id);
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
    let _gate = Gate::new(&s.gate);
    s.param_info.set(next);
    s.metadata_version.set(s.metadata_version.get() + 1);
    true
}

/// The main thread's.
fn read_ports(s: H) -> R<()> {
    let p = s.live();
    let (mut next_inputs, mut next_outputs) = (Vec::new(), Vec::new());
    // A plugin without the audio-ports extension has no audio ports: normal for note effects.
    let audio = p.ext::<clap_plugin_audio_ports>(CLAP_EXT_AUDIO_PORTS);
    if let Some(a) = audio {
        if a.count.is_none() || a.get.is_none() {
            return Err("Incomplete CLAP audio-ports interface".into());
        }
    }
    for input in [true, false] {
        let ports: &mut Vec<Port> = if input { &mut next_inputs } else { &mut next_outputs };
        let count = audio.map_or(0, |a| p.call(a.count, (input,)));
        if count > MAX_AUDIO_CHANNELS {
            return Err("Audio bus count exceeds the 64-channel budget".into());
        }
        let mut channels = 0;
        for i in 0..count {
            // SAFETY: plain old data, all zeros valid.
            let mut q: clap_audio_port_info = unsafe { std::mem::zeroed() };
            if !p.call(audio.expect("count is 0 without the table").get, (i, input, &mut q))
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
    if s.audio_layout_locked.get() && (!s.inputs.with(|v| same(v, &next_inputs)) || !s.outputs.with(|v| same(v, &next_outputs))) {
        return Err("CLAP audio port layout changed; reload this graph node before reconnecting audio".into());
    }
    s.inputs.set(next_inputs);
    s.outputs.set(next_outputs);
    s.note_inputs.with(Vec::clear);
    s.note_outputs.with(Vec::clear);
    if let Some(n) = p.ext::<clap_plugin_note_ports>(CLAP_EXT_NOTE_PORTS).filter(|n| n.count.is_some() && n.get.is_some()) {
        for input in [true, false] {
            let count = p.call(n.count, (input,));
            if count > 16 {
                return Err("More than 16 note ports are unsupported".into());
            }
            for i in 0..count {
                // SAFETY: plain old data, all zeros valid.
                let mut q: clap_note_port_info = unsafe { std::mem::zeroed() };
                if !p.call(n.get, (i, input, &mut q)) {
                    return Err("Invalid CLAP note port".into());
                }
                if input { &s.note_inputs } else { &s.note_outputs }.with(|v| v.push(q));
            }
        }
    }
    // A modulator may send only on parameter-target lanes (RFC 0002): those are its outputs.
    let lanes = p.ext::<oscairn_plugin_param_target_lanes_t>(ids::OSCAIRN_EXT_PARAM_TARGET_LANES);
    s.info.with(|i| i.lanes.clear());
    if let Some(l) = lanes.filter(|l| l.count.is_some() && l.get.is_some()) {
        for i in 0..p.call(l.count, ()).min(64) {
            // SAFETY: plain old data, all zeros valid.
            let mut lane: oscairn_param_target_lane_info_t = unsafe { std::mem::zeroed() };
            if p.call(l.get, (i, &mut lane)) {
                s.info.with(|info| info.lanes.push((lane.lane, field(&lane.name[..CLAP_NAME_SIZE]))));
            }
        }
    }
    if s.outputs.with(|v| v.is_empty()) && s.note_outputs.with(|v| v.is_empty()) && s.info.with(|i| i.lanes.is_empty()) {
        return Err("Plugin has neither audio nor note outputs".into());
    }
    // Read while the plugin is not processing; never freed here, so a pool the audio thread holds stays valid.
    if s.sysex_output.with(|x| x.is_none()) && s.note_outputs.with(|v| midi_port(v)) {
        s.sysex_output.set(Some(vec![0u8; SYSEX_BLOCK as usize].into_boxed_slice()));
    }
    let ni: usize = s.inputs.with(|v| v.iter().map(|q| q.channels as usize).sum());
    let no: usize = s.outputs.with(|v| v.iter().map(|q| q.channels as usize).sum());
    // The ring-out buffer is four more scratch channels after the ports', in no port.
    s.ring_out_channels.set(if s.sound_snapshots.get().is_some_and(|x| x.ring_out.is_some()) { 4 } else { 0 });
    let ring = s.ring_out_channels.get() as usize;
    s.input_pointers.with(|v| v.resize(ni, null_mut()));
    s.output_pointers.with(|v| v.resize(no + ring, null_mut()));
    let (in_ports, out_ports) = (s.inputs.with(|v| v.len()), s.outputs.with(|v| v.len()));
    s.input_buffers.with(|v| v.resize(in_ports, no_buffer()));
    s.output_buffers.with(|v| v.resize(out_ports, no_buffer()));
    let max_frames = s.max_frames.get() as usize;
    s.scratch.with(|v| {
        v.clear();
        v.resize((ni + no + ring) * max_frames, 0.0);
    });
    let (in_pointers, out_pointers) = (s.input_pointers.with(|v| v.as_mut_ptr()), s.output_pointers.with(|v| v.as_mut_ptr()));
    s.ring_out_buffer.set(clap_audio_buffer { channel_count: ring as u32, data32: out_pointers.wrapping_add(no), ..no_buffer() });
    for (ports, buffers, pointers) in [(&s.inputs, &s.input_buffers, in_pointers), (&s.outputs, &s.output_buffers, out_pointers)] {
        ports.with(|ports| {
            buffers.with(|buffers| {
                let mut offset = 0;
                for (b, port) in buffers.iter_mut().zip(ports.iter()) {
                    *b = clap_audio_buffer { channel_count: port.channels, data32: pointers.wrapping_add(offset), ..no_buffer() };
                    offset += port.channels as usize;
                }
            })
        });
    }
    Ok(())
}
pub fn midi_port(ports: &[clap_note_port_info]) -> bool {
    ports.iter().any(|q| q.supported_dialects & CLAP_NOTE_DIALECT_MIDI != 0)
}

/// The main thread's under the gate, or the audio thread's in process.
fn apply_outputs(s: H) {
    let Some(pa) = s.ports_activation.get() else { return };
    if !s.outputs_told.load(Acquire) {
        return;
    }
    let want = s.wanted_outputs.load(Acquire);
    let p = s.live();
    for i in 0..s.outputs.with(|v| v.len()).min(64) {
        let on = (want >> i) & 1 != 0;
        if !s.applied_told.get() || ((s.applied_outputs.get() >> i) & 1 != 0) != on {
            p.call(pa.set_active, (false, i as u32, on, 32));
        }
    }
    s.applied_outputs.set(want);
    s.applied_told.set(true);
}

/// The main thread's.
pub fn activate(s: H) -> R<()> {
    let rate = s.sample_rate.get();
    if !rate.is_finite() || !(8000.0..=768000.0).contains(&rate) || s.max_frames.get() == 0 || s.max_frames.get() > 65536 {
        return Err("Invalid sample rate or maximum block size".into());
    }
    read_ports(s)?;
    read_parameters(s)?;
    // Asked after the state is loaded and before activating, so a plugin that would not fit is refused before its
    // allocation (RFC 0025).
    s.has_declared.set(false);
    let p = s.live();
    if let Some(m) = s.memory.get() {
        if p.call(m.estimate, (rate, s.max_frames.get(), s.declared.as_ptr())) {
            s.has_declared.set(true);
            let d = s.declared.get();
            let mut bytes: u64 = 0;
            for part in [if s.context.get() { 0 } else { d.shared_bytes }, d.context_bytes, d.asset_bytes] {
                bytes = bytes.checked_add(part).ok_or("Plugin's memory declaration is not credible")?;
                // a sum that wraps
            }
            if s.memory_room.get() != 0 && bytes > s.memory_room.get() {
                return Err(refuse(&s, bytes));
            }
        }
    }
    // SAFETY: the declaration the host's own, read during the call; the refusal kick_clap's NUL-terminated text.
    let refused = unsafe { bridge::kick_clap_activating(if s.has_declared.get() { s.declared.as_ptr() } else { null() }) };
    if !refused.is_null() {
        // SAFETY: as above.
        return Err(unsafe { CStr::from_ptr(refused) }.to_string_lossy().into_owned());
    }
    s.hardware_epoch.fetch_add(1, SeqCst); // jobs from an earlier activation collect as stale
    s.applied_told.set(false);
    apply_outputs(s); // deactivated: told every port again
    if !p.activate(rate, 1, s.max_frames.get()) {
        return Err("Plugin activation failed".into());
    }
    s.active.set(true);
    s.transport.with(TransportWatch::reset);
    s.have_harmony.set(false);
    let latency = s.plugin_latency.get().and_then(|l| l.get).map_or(0, |f| p.call(f, ()));
    s.latency_frames.store(latency, SeqCst);
    Ok(())
}
pub fn refuse(s: &Host, bytes: u64) -> String {
    format!("Plugin needs {} MiB of memory where {} MiB is left", (bytes + (1 << 20) - 1) >> 20, s.memory_room.get() >> 20)
}
pub fn fail(s: &Host, why: String) {
    s.message.set(why);
    s.failed.store(true, SeqCst);
}

// ---- Events in and out ----
unsafe extern "C" fn event_count(list: *const clap_input_events) -> u32 {
    // SAFETY: the host's own list, given to the plugin by the gate's holder for the length of a process or flush.
    unsafe { H::new((*list).ctx.cast()) }.chunk_count.get()
}
unsafe extern "C" fn event_get(list: *const clap_input_events, i: u32) -> *const clap_event_header {
    // SAFETY: as event_count.
    let s = unsafe { H::new((*list).ctx.cast()) };
    if i < s.chunk_count.get() {
        s.chunk_events.with(|e| std::ptr::from_ref(e[i as usize].header()))
    } else {
        null()
    }
}
unsafe extern "C" fn output_push(list: *const clap_output_events, e: *const clap_event_header) -> bool {
    // SAFETY: as event_count.
    let s = unsafe { H::new((*list).ctx.cast()) };
    // SAFETY: e is null or the plugin's event, its header's size readable (CLAP), and a SysEx event's buffer its size.
    let valid = unsafe { event::valid(e) };
    // SAFETY: as above.
    let h = unsafe { e.as_ref() };
    let Some(h) = h.filter(|h| valid && h.time < s.current_process_frames.get()) else {
        if h.is_some_and(|h| h.space_id == CLAP_CORE_EVENT_SPACE_ID && h.type_ == CLAP_EVENT_MIDI_SYSEX) {
            s.dropped.fetch_add(1, SeqCst); // over the limit: counted
        }
        return false;
    };
    if h.space_id == BEAT_SPACE || h.space_id == HARMONY_SPACE {
        return false; // the host's to send
    }
    let core = h.space_id == CLAP_CORE_EVENT_SPACE_ID;
    // SAFETY: as above: a valid event, at most 64 bytes.
    let mut copy = unsafe { Event::copy(e) };
    if core && h.type_ == CLAP_EVENT_PARAM_VALUE {
        let p: clap_event_param_value = copy.get();
        let i = param_index(&s, p.param_id);
        if i < s.param_info.with(|v| v.len()) && p.value.is_finite() && f64::from_bits(s.param_values.with(|v| v[i].swap(p.value.to_bits(), SeqCst))) != p.value
        {
            s.dirty.store(true, SeqCst);
            s.parameter_reports.fetch_add(1, Release);
        }
    }
    copy.header_mut().time += s.output_frame_offset.get();
    if core && h.type_ == CLAP_EVENT_MIDI_SYSEX {
        // MIDI data bytes are 7-bit. The plugin's buffer is valid only during this call: copy it.
        let mut x: clap_event_midi_sysex = copy.get();
        // SAFETY: as above: the buffer is readable for its size.
        let data = (1..x.size.saturating_sub(1)).all(|i| unsafe { *x.buffer.add(i as usize) } < 0x80);
        let used = s.sysex_used.get();
        let held = data && s.in_process.get() && x.size <= SYSEX_BLOCK - used && s.sysex_output.with(|x| x.is_some());
        if !held {
            s.dropped.fetch_add(1, SeqCst);
            return false;
        }
        let pool = s.sysex_output.with(|o| o.as_mut().map_or(null_mut(), |o| o.as_mut_ptr())).wrapping_add(used as usize);
        // SAFETY: as above; the pool holds SYSEX_BLOCK bytes, used + size of them at most.
        unsafe { std::ptr::copy_nonoverlapping(x.buffer, pool, x.size as usize) };
        x.buffer = pool;
        s.sysex_used.set(s.sysex_used.get() + x.size);
        copy = Event::from(&x);
    }
    // Parameter and gesture events go to the control thread; everything else (notes, MIDI, expressions) is kept for
    // the caller of process(), which routes it to the next plugin in the same block.
    let t = h.type_;
    let control =
        core && (t == CLAP_EVENT_PARAM_VALUE || t == CLAP_EVENT_PARAM_MOD || t == CLAP_EVENT_PARAM_GESTURE_BEGIN || t == CLAP_EVENT_PARAM_GESTURE_END);
    if !control {
        let n = s.block_output_count.get();
        if s.in_process.get() && (n as usize) < MAX_BLOCK_EVENTS {
            s.block_output.with(|o| o[n as usize] = copy);
            s.block_output_count.set(n + 1);
            return true;
        }
        s.dropped.fetch_add(1, SeqCst);
        return false;
    }
    if s.emitted.push(copy) {
        return true;
    }
    s.output_drops.fetch_add(1, SeqCst);
    s.dropped.fetch_add(1, SeqCst);
    false
}
/// The main thread's under the gate, or the audio thread's in process.
fn flush_parameters(s: H) {
    let Some(flush) = s.params.get().and_then(|p| p.flush) else { return };
    let _scope = AudioScope::new(s.ptr());
    s.chunk_count.set(0);
    s.output_frame_offset.set(0);
    s.current_process_frames.set(1);
    // Only parameter events belong to flush(); service() never drains the command queue.
    let input = clap_input_events { ctx: s.ptr().cast(), size: Some(event_count), get: Some(event_get) };
    let output = clap_output_events { ctx: s.ptr().cast(), try_push: Some(output_push) };
    s.live().call(flush, (&input, &output));
}
/// Audio thread, or main under the gate.
pub fn translate(s: &Host, event: Event) -> Event {
    let h = event.header();
    if h.type_ == CLAP_EVENT_PARAM_VALUE {
        let mut p: clap_event_param_value = event.get();
        let i = param_index(s, p.param_id);
        if let Some(cookie) = s.cookies.with(|c| c.get(i).copied()) {
            p.cookie = cookie;
            return Event::from(&p);
        }
    } else if h.type_ == CLAP_EVENT_MIDI {
        let m: clap_event_midi = event.get();
        if let Some(dialect) = s.note_inputs.with(|v| v.get(m.port_index as usize).map(|q| q.supported_dialects)) {
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
        let dialect = if n.port_index >= 0 { s.note_inputs.with(|v| v.get(n.port_index as usize).map(|q| q.supported_dialects)) } else { None };
        if dialect.is_some_and(|d| d & CLAP_NOTE_DIALECT_CLAP == 0) {
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
/// As translate.
fn accepts(s: &Host, event: &Event) -> bool {
    let h = event.header();
    let dialect = |port: usize| s.note_inputs.with(|v| v.get(port).map_or(0, |q| q.supported_dialects));
    let ports = s.note_inputs.with(|v| v.len());
    match h.type_ {
        CLAP_EVENT_NOTE_END => false, // plugin-to-host only
        CLAP_EVENT_MIDI_SYSEX => {
            // On a note port that takes MIDI, as CLAP's dialects say.
            let port = event.get::<clap_event_midi_sysex>().port_index as usize;
            port < ports && dialect(port) & CLAP_NOTE_DIALECT_MIDI != 0
        }
        CLAP_EVENT_PARAM_VALUE | CLAP_EVENT_PARAM_MOD | CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => {
            let id = match h.type_ {
                CLAP_EVENT_PARAM_VALUE => event.get::<clap_event_param_value>().param_id,
                CLAP_EVENT_PARAM_MOD => event.get::<clap_event_param_mod>().param_id,
                _ => event.get::<clap_event_param_gesture>().param_id,
            };
            let i = param_index(s, id);
            s.param_info.with(|info| i < info.len() && info[i].flags & CLAP_PARAM_IS_READONLY == 0)
        }
        CLAP_EVENT_MIDI => {
            let port = event.get::<clap_event_midi>().port_index as usize;
            port < ports && dialect(port) & (CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE) != 0
        }
        CLAP_EVENT_MIDI2 => false, // not advertised by this host
        _ => {
            let port = if h.type_ == CLAP_EVENT_NOTE_EXPRESSION {
                event.get::<clap_event_note_expression>().port_index
            } else {
                event.get::<clap_event_note>().port_index
            };
            if port == -1 {
                return ports != 0;
            }
            port >= 0 && (port as usize) < ports && dialect(port as usize) & CLAP_NOTE_DIALECT_CLAP != 0
        }
    }
}

// ---- The control thread's calls ----

/// Host::service. The main thread's.
pub fn service(s: H) {
    if !main(&s) || s.plugin.get().is_null() {
        return;
    }
    if let Err(e) = service_inner(s) {
        fail(&s, e);
    }
}
fn service_inner(s: H) -> R<()> {
    let p = s.live();
    if s.callback.swap(false, SeqCst) {
        p.on_main_thread();
    }
    let now = Instant::now();
    // Callbacks may register and unregister timers and fds, so iterate copies.
    let timers = s.timers.with(|t| t.clone());
    if let Some(on_timer) = s.timer_support.get().and_then(|t| t.on_timer) {
        for timer in timers.iter().filter(|t| now >= t.next) {
            let due = s.timers.with(|ts| ts.iter_mut().find(|t| t.id == timer.id).map(|t| t.next = now + Duration::from_millis(timer.period.into())));
            if due.is_some() {
                p.call(on_timer, (timer.id,));
            }
        }
    }
    let fds = s.fds.with(|f| f.clone());
    if let Some(on_fd) = s.fd_support.get().and_then(|f| f.on_fd) {
        for item in &fds {
            let mut fd = PollFd { fd: item.fd, events: 0, revents: 0 };
            if item.flags & CLAP_POSIX_FD_READ != 0 {
                fd.events |= POLLIN;
            }
            if item.flags & CLAP_POSIX_FD_WRITE != 0 {
                fd.events |= POLLOUT;
            }
            // SAFETY: one pollfd, the host's own.
            if unsafe { poll(&mut fd, 1, 0) } > 0 {
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
                if flags != 0 && s.fds.with(|f| f.iter().any(|f| f.fd == item.fd)) {
                    p.call(on_fd, (item.fd, flags));
                }
            }
        }
    }
    service_gui(s);
    let flags = s.rescan.swap(0, SeqCst);
    let restart = s.restart.swap(false, SeqCst);
    if restart || flags & CLAP_PARAM_RESCAN_ALL != 0 || (flags & CLAP_PARAM_RESCAN_INFO != 0 && !read_parameter_info(s)) {
        let _gate = Gate::new(&s.gate);
        deactivate(s);
        activate(s)?;
    } else if flags & CLAP_PARAM_RESCAN_VALUES != 0 {
        let get = s.params.get().expect("a plugin asking for a rescan of values has the params table").get_value;
        for i in 0..s.param_info.with(|v| v.len()) {
            let mut value = 0.0;
            if p.call(get, (s.param_info.with(|v| v[i].id), &mut value)) && value.is_finite() {
                s.param_values.with(|v| f64_store(&v[i], value));
            }
        }
    }
    // While the plugin is processing, process() performs requested flushes on the audio thread. Otherwise flush here,
    // excluding a process() that starts.
    if !s.processing.load(SeqCst) && s.flush.load(SeqCst) {
        let _gate = Gate::new(&s.gate);
        if !s.processing.load(SeqCst) && s.flush.swap(false, SeqCst) {
            flush_parameters(s);
        }
    }
    if s.failed.load(SeqCst) {
        let _gate = Gate::new(&s.gate);
        stop(s);
    }
    Ok(())
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

/// Host::enqueue. Any thread.
pub fn enqueue(s: &Host, e: &Event) -> bool {
    let h = e.header();
    // SAFETY: a whole event, 64 bytes (a SysEx event is refused after valid() reads its buffer, as Host.cpp does: the
    // caller's buffer).
    if !unsafe { event::valid(h) } || h.space_id != CLAP_CORE_EVENT_SPACE_ID || h.type_ == CLAP_EVENT_MIDI_SYSEX {
        return false;
    }
    // Count before publishing, so process() never takes an event it has not counted.
    s.pending_commands.fetch_add(1, SeqCst);
    if s.commands.push(*e) {
        wake_up(s);
        return true;
    }
    s.pending_commands.fetch_sub(1, SeqCst);
    s.dropped.fetch_add(1, SeqCst);
    // Control callers can retain and retry refused parameter and gesture commands; resetting here would discard
    // already accepted commands and cut held notes.
    let t = h.type_;
    if t != CLAP_EVENT_PARAM_VALUE && t != CLAP_EVENT_PARAM_MOD && t != CLAP_EVENT_PARAM_GESTURE_BEGIN && t != CLAP_EVENT_PARAM_GESTURE_END {
        s.panic.store(true, SeqCst);
    }
    false
}
/// The main thread's.
pub fn set_parameter(s: &Host, id: clap_id, value: f64) -> bool {
    let index = param_index(s, id);
    let Some((minimum, maximum, flags)) = s.param_info.with(|v| v.get(index).map(|p| (p.minimum, p.maximum, p.flags))) else { return false };
    if !value.is_finite() || flags & CLAP_PARAM_IS_READONLY != 0 {
        return false;
    }
    let mut value = value.clamp(minimum, maximum);
    if flags & CLAP_PARAM_IS_STEPPED != 0 {
        value = value.trunc();
    }
    // Zeroed first, padding too, as the C++ value-initializes it: the plugin is given every byte.
    // SAFETY: plain old data, all zeros valid.
    let mut e: clap_event_param_value = unsafe { std::mem::zeroed() };
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
    s.param_values.with(|v| f64_store(&v[index], value));
    s.dirty.store(true, SeqCst);
    true
}
/// The main thread's.
pub fn parameter_gesture(s: &Host, id: clap_id, begin: bool) -> bool {
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
    enqueue(s, &Event::from(&e))
}
/// The one consumer: the main thread.
pub fn pop_output(s: &Host) -> Option<Event> {
    s.emitted.pop()
}
/// The main thread's.
pub fn parameter_text(s: &Host, id: clap_id, value: f64) -> Vec<u8> {
    if let Some(f) = s.params.get().and_then(|p| p.value_to_text) {
        let mut text = [0 as c_char; 256];
        if s.live().call(f, (id, value, text.as_mut_ptr(), 256)) {
            return text.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        }
    }
    cpp_to_string(value).into_bytes()
}
/// Host::panic. Any thread.
pub fn panic(s: &Host) {
    s.panic.store(true, SeqCst);
    wake_up(s);
}
/// The main thread's.
pub fn reset(s: H) {
    if !main(&s) || s.plugin.get().is_null() {
        return;
    }
    let _gate = Gate::new(&s.gate);
    let _scope = AudioScope::new(s.ptr());
    s.live().reset();
    s.sleeping.store(false, SeqCst);
}
/// The main thread's.
pub fn set_active_outputs(s: &Host, ports: u64) {
    let told = s.outputs_told.load(Relaxed);
    if told && s.wanted_outputs.load(Relaxed) == ports {
        return;
    }
    s.wanted_outputs.store(ports, Release);
    s.outputs_told.store(true, Release);
    if !s.ports_activation.is_null() && !s.activate_while_processing.get() && s.active.get() {
        s.restart.store(true, SeqCst); // only while deactivated: service() activates it again
    }
}
/// The audio thread's, between process() calls.
///
/// # Safety
/// contexts holds count, or is null.
pub unsafe fn set_voice_limits(s: &Host, limit: u32, contexts: *const oscairn_voice_budget_context_t, count: u32) {
    if s.voice_budget.is_null() {
        return;
    }
    s.voice_limit.set(limit);
    let n = if contexts.is_null() { 0 } else { count.min(OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS) };
    s.voice_context_count.set(n);
    for i in 0..n as usize {
        // SAFETY: the caller's.
        let context = unsafe { *contexts.add(i) };
        s.voice_contexts.with(|c| c[i] = context);
    }
    s.limits_waiting.set(true);
}
/// The audio thread's, after process().
pub fn voice_report(s: &Host, cost: &mut f64, notes: &mut *const oscairn_voice_budget_note_t, count: &mut u32) -> u32 {
    *cost = s.voice_cost.get();
    *notes = s.voice_notes.with(|n| n.as_ptr());
    let budget = !s.voice_budget.is_null();
    *count = if budget { s.voice_note_count.get() } else { 0 };
    if budget {
        s.active_voices.get()
    } else {
        0
    }
}
/// The audio thread's, between blocks.
pub fn restful(s: &Host) -> bool {
    // carry_count is written under the gate and read here without. A stale read is harmless: 0 where events were just carried
    // rests the worker for one more block (they wait in carry_events for the block that runs), and more than 0 where they
    // were just cleared keeps it awake one more block. Never dropped.
    s.sleeping.load(SeqCst) && !s.wake.load(SeqCst) && !s.flush.load(SeqCst) && !s.panic.load(SeqCst) && s.carry_count.load(Relaxed) == 0
}
/// The audio thread's, between blocks.
pub fn rested(s: &Host, frames: u32, blocks: u32) {
    s.steady_time.set(s.steady_time.get() + i64::from(frames));
    s.idle_blocks.fetch_add(blocks.into(), SeqCst);
}
/// # Safety
/// mirror outlives its setting, or is null.
pub unsafe fn mirror_wake(s: &Host, mirror: *const AtomicU32) {
    s.wake_mirror.store(mirror.cast_mut(), SeqCst);
    if !mirror.is_null() {
        // SAFETY: the caller's.
        unsafe { (*mirror).store(u32::from(s.wake.load(SeqCst)), SeqCst) };
    }
}
/// Host::lockAudioLayout. The main thread's.
pub fn lock_audio_layout(s: &Host) {
    if main(s) && !s.plugin.get().is_null() {
        s.audio_layout_locked.set(true);
    }
}
/// Host::reconfigure. The main thread's.
pub fn reconfigure(s: H, rate: f64, frames: u32) -> bool {
    if !main(&s) || !s.loaded.load(SeqCst) {
        return false;
    }
    let _gate = Gate::new(&s.gate);
    deactivate(s);
    s.sample_rate.set(rate);
    s.max_frames.set(frames);
    match activate(s) {
        Ok(()) => {
            s.failed.store(false, SeqCst);
            true
        }
        Err(e) => {
            fail(&s, e);
            false
        }
    }
}

// ---- The audio thread's block ----

/// Host::process. Allocates nothing and takes no lock: the gate is tried, never waited for.
///
/// # Safety
/// Its one audio thread; inputs, outputs and events as Host.h says.
#[allow(clippy::too_many_arguments)]
pub unsafe fn process(
    s: H,
    input: *const *const f32,
    ni: u32,
    output: *const *mut f32,
    no: u32,
    frames: u32,
    incoming: *const Event,
    count: u32,
    transport: *const clap_event_transport,
) -> bool {
    let silence = || {
        if !output.is_null() {
            for c in 0..no as usize {
                // SAFETY: the caller's: no output channels, each null or of frames samples.
                unsafe {
                    let o = *output.add(c);
                    if !o.is_null() {
                        std::ptr::write_bytes(o, 0, frames as usize);
                    }
                }
            }
        }
    };
    s.published_output_count.set(0);
    s.reset_in_process.set(false);
    if frames == 0 {
        return true;
    }
    // A note-only plugin (no audio outputs) may be processed with no output buffers.
    if (no != 0 && output.is_null()) || (count != 0 && incoming.is_null()) {
        silence();
        return false;
    }
    if s.gate.swap(true, Acquire) {
        s.control_silences.fetch_add(1, SeqCst);
        silence();
        return false;
    }
    // SAFETY: the caller's.
    let ok = unsafe { block(s, input, ni, output, no, frames, incoming, count, transport, &silence) };
    s.in_process.set(false);
    s.published_output_count.set(s.block_output_count.get());
    s.gate.store(false, Release);
    ok
}

/// # Safety
/// As process, the gate held.
#[allow(clippy::too_many_arguments)]
unsafe fn block(
    s: H,
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
    s.block_output_count.set(0);
    s.sysex_used.set(0);
    s.in_process.set(true);
    let _scope = AudioScope::new(s.ptr());
    let Some(p) = s.instance().filter(|_| s.active.get() && !s.failed.load(SeqCst)) else {
        silence();
        return false;
    };
    let start = Instant::now();
    // Saving must wait until the plugin processed them, not just until they left the queue.
    let mut acknowledged = 0u32;
    // SAFETY: the caller's; p active.
    let ok = unsafe { render(s, p, input, ni, output, no, frames, incoming, count, transport, silence, &mut acknowledged, start) };
    if acknowledged != 0 {
        s.pending_commands.fetch_sub(acknowledged, Release);
    }
    ok
}

/// # Safety
/// As block; p the host's plugin, active.
#[allow(clippy::too_many_arguments)]
unsafe fn render(
    s: H,
    p: Plugin,
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
    struct Collect {
        total: usize,
        harmony_came: bool, // a block without one: the plugin is in no part
        timing: usize,      // of total, the beat and harmony events, which alone wake no plugin asleep
        harmony_new: bool,
    }
    fn deliver(s: H, c: &mut Collect, frames: u32, e: Event) {
        let h = e.header();
        // A malformed or late event is dropped on its own; resetting the plugin for it would cut every sounding note.
        // Oscairn's host-to-plugin events go only to a plugin that asked for them, as they are.
        if h.space_id == BEAT_SPACE || h.space_id == HARMONY_SPACE {
            // SAFETY: a whole event, 64 bytes.
            if !unsafe { event::valid(h) } || h.time >= frames {
                s.dropped.fetch_add(1, SeqCst);
                return;
            }
            if h.space_id == BEAT_SPACE && !s.wants_beat.load(Relaxed) {
                return;
            }
            if h.space_id == HARMONY_SPACE {
                // SAFETY: a harmony event fits the 64 bytes, read unaligned.
                let x = unsafe { std::ptr::read_unaligned(e.bytes.as_ptr().cast::<oscairn_event_harmony_t>()) }.harmony;
                c.harmony_came = true;
                if !s.wants_harmony.load(Relaxed) || (s.have_harmony.get() && harmony_eq(&x, &s.last_harmony.get())) {
                    return;
                }
                s.last_harmony.set(x);
                s.have_harmony.set(true);
                c.harmony_new = true;
                publish_harmony(&s, Some(&x));
            }
            if c.total == RENDER_EVENTS {
                s.dropped.fetch_add(1, SeqCst);
                return;
            }
            s.ordered.with(|o| o[c.total] = Timed { event: e, order: c.total as u32 });
            c.total += 1;
            c.timing += 1;
            return;
        }
        // Events in other spaces (param-target) are for the caller, never for a plugin.
        // SAFETY: as above.
        if !unsafe { event::valid(h) } || h.space_id != CLAP_CORE_EVENT_SPACE_ID || h.time >= frames {
            s.dropped.fetch_add(1, SeqCst);
            return;
        }
        // Overflow can drop note-offs, so it still resets the plugin rather than risk stuck notes.
        if c.total == RENDER_EVENTS {
            s.dropped.fetch_add(1, SeqCst);
            s.panic.store(true, SeqCst);
            return;
        }
        let translated = translate(&s, e);
        if !accepts(&s, &translated) {
            s.dropped.fetch_add(1, SeqCst);
            return;
        }
        s.ordered.with(|o| o[c.total] = Timed { event: translated, order: c.total as u32 });
        c.total += 1;
    }
    fn append(s: H, c: &mut Collect, frames: u32, e: Event) {
        let h = e.header();
        // SAFETY: a whole event, 64 bytes.
        if unsafe { event::valid(h) } && h.space_id == CLAP_CORE_EVENT_SPACE_ID && (h.type_ == CLAP_EVENT_NOTE_OFF || h.type_ == CLAP_EVENT_NOTE_CHOKE) {
            let n: clap_event_note = e.get();
            for port in 0..s.note_inputs.with(|v| v.len()) {
                if n.port_index >= 0 && n.port_index as usize != port {
                    continue;
                }
                if s.note_inputs.with(|v| v[port].supported_dialects) & CLAP_NOTE_DIALECT_CLAP != 0 {
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
    let mut c = Collect { total: 0, harmony_came: false, timing: 0, harmony_new: false };
    for i in 0..s.carry_count.load(Relaxed) as usize {
        append(s, &mut c, frames, s.carry_events.with(|e| e[i]));
    }
    s.carry_count.store(0, Relaxed);
    let direct = (count as usize).min(MAX_PROCESS_EVENTS);
    while (*acknowledged as usize) < MAX_BLOCK_EVENTS {
        let Some(e) = s.commands.pop() else { break };
        *acknowledged += 1;
        append(s, &mut c, frames, e);
    }
    for i in 0..direct {
        // SAFETY: the caller's: count events.
        append(s, &mut c, frames, unsafe { *incoming.add(i) });
    }
    let mut total = c.total;
    if count as usize > MAX_PROCESS_EVENTS {
        s.dropped.fetch_add(u64::from(count) - MAX_PROCESS_EVENTS as u64, SeqCst);
        s.panic.store(true, SeqCst);
    }
    if !c.harmony_came && s.harmony_words[3].load(Relaxed) != 0 {
        s.have_harmony.set(false); // out of its part, or its part's harmony gone
        publish_harmony(&s, None);
    }
    if s.panic.swap(false, SeqCst) {
        // Never leave a stuck gate after an event overflow. Reset is an audio-thread operation and covers every note
        // channel and port.
        p.reset();
        // The block's parameter changes outlive the reset, which ends notes, not values: one dropped here stays
        // lost, the caller holding it as set (a restarted worker's restored value, its first block late).
        let mut kept = 0;
        s.ordered.with(|o| {
            for i in 0..total {
                let h = o[i].event.header();
                if h.space_id == CLAP_CORE_EVENT_SPACE_ID
                    && (h.type_ == CLAP_EVENT_PARAM_VALUE
                        || h.type_ == CLAP_EVENT_PARAM_MOD
                        || h.type_ == CLAP_EVENT_PARAM_GESTURE_BEGIN
                        || h.type_ == CLAP_EVENT_PARAM_GESTURE_END)
                {
                    o[kept] = o[i];
                    kept += 1;
                }
            }
        });
        total = kept;
        c.timing = 0;
        s.reset_in_process.set(true);
        s.sleeping.store(false, SeqCst);
        s.have_harmony.set(false); // the harmony comes again next block
    }
    s.ordered.with(|o| o[..total].sort_unstable_by_key(|e| (e.event.header().time, e.order)));
    let mut has_input = false;
    if !input.is_null() {
        for c in 0..ni as usize {
            // SAFETY: the caller's: ni input channels, each null or of frames samples.
            let i = unsafe { *input.add(c) };
            // SAFETY: as above.
            if !i.is_null() && unsafe { std::slice::from_raw_parts(i, frames as usize) }.iter().any(|&x| x != 0.0) {
                has_input = true;
                break;
            }
        }
    }
    // SAFETY: the caller's: null or a transport.
    let transport = unsafe { transport.as_ref() };
    let rate = s.sample_rate.get();
    let transport_wake = s.transport.with(|t| t.changed(transport, frames, rate));
    // The beat and a harmony change alone wake no plugin asleep: they come with the block that does (the beat's tempo
    // changes only with the transport, which wakes it); a harmony held back here is sent again then.
    if s.sleeping.load(SeqCst) && total == c.timing && !has_input && !transport_wake && !s.wake.swap(false, SeqCst) && !s.flush.load(SeqCst) {
        if c.harmony_new {
            s.have_harmony.set(false);
        }
        s.idle_blocks.fetch_add(1, SeqCst);
        s.steady_time.set(s.steady_time.get() + i64::from(frames));
        silence();
        return true;
    }
    s.wake.store(false, SeqCst);
    s.sleeping.store(false, SeqCst);
    let mirror = s.wake_mirror.load(SeqCst);
    if !mirror.is_null() {
        // SAFETY: the mirror outlives its setting (Host.h).
        unsafe { (*mirror).store(0, Release) };
    }
    if !s.processing.load(SeqCst) {
        if !p.start_processing() {
            s.failed.store(true, SeqCst);
            s.errors.fetch_add(1, SeqCst);
            silence();
            return false;
        }
        s.processing.store(true, SeqCst);
    }
    if s.flush.swap(false, SeqCst) {
        flush_parameters(s);
    }
    if s.activate_while_processing.get() && s.outputs_told.load(Relaxed) && (!s.applied_told.get() || s.wanted_outputs.load(Acquire) != s.applied_outputs.get())
    {
        apply_outputs(s);
    }
    // oscairn.voice-budget: new limits before the block (an audio-thread call outside process()).
    if s.limits_waiting.get() {
        if let Some(v) = s.voice_budget.get() {
            let contexts = s.voice_contexts.with(|c| c.as_ptr());
            p.call(v.set_limits, (s.voice_limit.get(), contexts, s.voice_context_count.get()));
            s.limits_waiting.set(false);
        }
    }
    let max_frames = s.max_frames.get() as usize;
    let ni_ports = s.input_pointers.with(|v| v.len());
    let mut next = 0;
    let mut done = 0u32;
    while done < frames {
        let n = s.max_frames.get().min(frames - done);
        s.chunk_count.set(0);
        s.output_frame_offset.set(done);
        s.current_process_frames.set(n);
        let mut chunk = 0u32;
        s.ordered.with(|o| {
            s.chunk_events.with(|events| {
                while next < total && o[next].event.header().time < done + n {
                    let mut e = o[next].event;
                    next += 1;
                    e.header_mut().time -= done;
                    events[chunk as usize] = e;
                    chunk += 1;
                }
            })
        });
        s.chunk_count.set(chunk);
        let scratch = s.scratch.with(|v| v.as_mut_ptr());
        s.input_pointers.with(|pointers| {
            for (c, pointer) in pointers.iter_mut().enumerate() {
                let target = scratch.wrapping_add(c * max_frames);
                // SAFETY: the caller's input channels (as above); the scratch holds every port's max_frames.
                unsafe {
                    let i = if !input.is_null() && c < ni as usize { *input.add(c) } else { null() };
                    if i.is_null() {
                        std::ptr::write_bytes(target, 0, n as usize);
                    } else {
                        std::ptr::copy_nonoverlapping(i.add(done as usize), target, n as usize);
                    }
                }
                *pointer = target;
            }
        });
        s.output_pointers.with(|pointers| {
            for (c, pointer) in pointers.iter_mut().enumerate() {
                let target = scratch.wrapping_add((ni_ports + c) * max_frames);
                // SAFETY: as above.
                unsafe { std::ptr::write_bytes(target, 0, n as usize) };
                *pointer = target;
            }
        });
        let in_events = clap_input_events { ctx: s.ptr().cast(), size: Some(event_count), get: Some(event_get) };
        let out_events = clap_output_events { ctx: s.ptr().cast(), try_push: Some(output_push) };
        let mut position: clap_event_transport;
        let mut process = clap_process {
            steady_time: s.steady_time.get(),
            frames_count: n,
            transport: null(),
            audio_inputs: s.input_buffers.with(|b| b.as_ptr()),
            audio_outputs: s.output_buffers.with(|b| b.as_mut_ptr()),
            audio_inputs_count: s.input_buffers.with(|b| b.len()) as u32,
            audio_outputs_count: s.output_buffers.with(|b| b.len()) as u32,
            in_events: &in_events,
            out_events: &out_events,
        };
        if let Some(t) = transport {
            position = *t;
            let playing = position.flags & CLAP_TRANSPORT_IS_PLAYING != 0;
            if playing && position.flags & CLAP_TRANSPORT_HAS_SECONDS_TIMELINE != 0 {
                position.song_pos_seconds += ((f64::from(done) / s.sample_rate.get()) * CLAP_SECTIME_FACTOR as f64) as i64;
            }
            if playing && position.flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0 && position.flags & CLAP_TRANSPORT_HAS_TEMPO != 0 {
                position.song_pos_beats += ((f64::from(done) / s.sample_rate.get()) * position.tempo / 60.0 * CLAP_BEATTIME_FACTOR as f64) as i64;
            }
            position.header.time = 0;
            process.transport = &position;
        }
        // The caller asks for the ring-out buffer by passing its channels after the ports'.
        if s.ring_out_channels.get() != 0 {
            let ring = if no as usize >= s.output_pointers.with(|v| v.len()) { s.ring_out_buffer.as_ptr().cast_const() } else { null() };
            // ring_out present: ring_out_channels says so
            p.call(s.sound_snapshots.get().expect("the plugin's sound snapshots").ring_out, (ring,));
        }
        let status = p.process(&process);
        if !(CLAP_PROCESS_CONTINUE..=CLAP_PROCESS_SLEEP).contains(&status) {
            s.failed.store(true, SeqCst);
            s.errors.fetch_add(1, SeqCst);
            silence();
            return false;
        }
        for c in 0..no as usize {
            // SAFETY: the caller's output channels (as silence).
            let o = unsafe { *output.add(c) };
            if o.is_null() {
                continue;
            }
            let destination = o.wrapping_add(done as usize);
            if let Some(from) = s.output_pointers.with(|v| v.get(c).copied()) {
                // One pass without a branch a sample: copied, then a block with any sample not finite silenced.
                let mut bad = 0u32;
                // SAFETY: the plugin's output channel (scratch, n samples) and the caller's (as above).
                unsafe {
                    for i in 0..n as usize {
                        let x = *from.add(i);
                        *destination.add(i) = x;
                        bad |= u32::from(!x.is_finite()); // the C++ `!(fabs(x) <= FLT_MAX)`: NaN or infinite
                    }
                }
                if bad != 0 {
                    s.failed.store(true, SeqCst);
                    s.errors.fetch_add(1, SeqCst);
                    silence();
                    return false;
                }
            } else {
                // SAFETY: as above.
                unsafe { std::ptr::write_bytes(destination, 0, n as usize) };
            }
        }
        s.sleeping.store(status == CLAP_PROCESS_SLEEP, SeqCst);
        s.steady_time.set(s.steady_time.get() + i64::from(n));
        done += n;
    }
    if let Some(v) = s.voice_budget.get() {
        s.active_voices.set(p.call(v.active_voices, (s.voice_cost.as_ptr(),)));
        let (notes, capacity) = s.voice_notes.with(|n| (n.as_mut_ptr(), n.len() as u32));
        s.voice_note_count.set(p.call(v.notes, (notes, capacity)).min(capacity));
    }
    s.blocks.fetch_add(1, SeqCst);
    let us = start.elapsed().as_secs_f64() * 1e6;
    f64_store(&s.last_us, us);
    if us > f64_load(&s.max_us) {
        f64_store(&s.max_us, us);
    }
    f64_store(&s.total_us, f64_load(&s.total_us) + us);
    if us > f64::from(frames) / s.sample_rate.get() * 1e6 {
        s.deadline_misses.fetch_add(1, SeqCst);
    }
    true
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

pub fn supports_state(s: &Host) -> bool {
    s.state.get().is_some_and(|x| x.save.is_some() && x.load.is_some())
}
pub fn exclusive_state(s: &Host) -> bool {
    s.exclusive_state.get()
}

// ---- For more.rs: the host's state as the rest of Host.cpp touches it ----
impl Host {
    pub fn dirty_clear(&self) {
        self.dirty.store(false, SeqCst);
    }
    pub fn dirty_set(&self) {
        self.dirty.store(true, SeqCst);
    }
    pub fn last_logged_error_clear(&self) {
        self.last_logged_error.with(String::clear);
    }
    pub fn last_logged_error(&self) -> String {
        self.last_logged_error.with(|e| e.clone())
    }
    pub fn asset_dependencies(&self) -> Option<&oscairn_plugin_asset_dependencies_t> {
        self.asset_dependencies.get()
    }
    pub fn asset_optional(&self) -> bool {
        self.asset_optional.get()
    }
    pub fn timing(&self) -> (f64, u32) {
        (self.sample_rate.get(), self.max_frames.get())
    }
}
/// The plugin's state table: clap.state's, or its sound contexts' own.
pub fn state(s: &Host) -> Option<&clap_plugin_state> {
    s.state.get()
}
/// The main thread's.
pub fn stop_processing(s: H) {
    stop(s)
}
/// Every queued command dropped. The main thread's, under the gate.
pub fn drain_commands(s: &Host) {
    while s.commands.pop().is_some() {
        s.pending_commands.fetch_sub(1, SeqCst);
    }
}
/// The main thread's, under the gate (the queue's one consumer).
pub fn pop_command(s: &Host) -> Option<Event> {
    s.commands.pop()
}
/// As translate.
pub fn translate_event(s: &Host, e: Event) -> Event {
    translate(s, e)
}
/// The plugin's parameter flush with these events (saveState's), as the audio thread. The main thread's, under the
/// gate, the plugin not processing.
pub fn flush_with(s: H, events: &[Event]) {
    s.chunk_count.set(events.len() as u32);
    s.chunk_events.with(|e| e[..events.len()].copy_from_slice(events));
    s.output_frame_offset.set(0);
    s.current_process_frames.set(1);
    let Some(flush) = s.params.get().and_then(|p| p.flush) else { return };
    let _scope = AudioScope::new(s.ptr());
    let input = clap_input_events { ctx: s.ptr().cast(), size: Some(event_count), get: Some(event_get) };
    let output = clap_output_events { ctx: s.ptr().cast(), try_push: Some(output_push) };
    s.live().call(flush, (&input, &output));
}
/// The parameters' values read back from the plugin (a rescan of values, a preset loaded). The main thread's.
pub fn read_values(s: &Host) {
    let Some(get) = s.params.get().and_then(|p| p.get_value) else { return };
    for i in 0..s.param_info.with(|v| v.len()) {
        let mut value = 0.0;
        if s.live().call(get, (s.param_info.with(|v| v[i].id), &mut value)) && value.is_finite() {
            s.param_values.with(|v| f64_store(&v[i], value));
        }
    }
}
/// What an effect context takes from the instance it is made from.
pub fn inherit(s: &Host, parent: &Host) {
    s.library.set(parent.library.with(|l| l.clone()));
    s.info.set(parent.info.with(|i| i.clone()));
    s.context.set(true);
    s.memory_room.set(parent.memory_room.get());
    s.offline.set(parent.offline.get());
    s.assets.set(parent.assets.get()); // a copy of the table: the parent may go first
}
/// As initialize.
pub fn initialize_context(s: H, rate: f64, frames: u32) -> R<()> {
    initialize(s, rate, frames)
}
pub fn refusal(s: &Host, bytes: u64) -> String {
    refuse(s, bytes)
}
