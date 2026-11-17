//! The engine thread's core in Rust: note routing, the voice budget, the budget's per-block
//! bookkeeping and the plan's audio (buffers, mixing, the output guard, meters), behind the C interface in core/engine/src/engine_core.h (names rust_core_*). The
//! differential tests (core/engine/tests: core_diff.cpp, audio_diff.cpp, run_diff.cpp) hold it to the outputs its C++
//! reference gave, recorded in core/engine/tests/vectors. All unsafe code is in this file, at that boundary.

mod audio;
#[rustfmt::skip]
mod bindings;
mod budget;
mod event;
mod notes;
mod route;
mod run;

use audio::{Audio, Bus};
use bindings::*;
use event::Event;
use notes::{Key, Notes, Rendered, SoundContext, Source, Step, Timing, VoiceNote};
use route::{NoteRoute, Zones};
use run::{Block, Control, HarmonySource, Run, Tap};
use std::ffi::c_void;
use std::ops::{Deref, Index, IndexMut};
use std::ptr::NonNull;
use std::sync::atomic::AtomicU64;
// The CLAP host (core/clap-host/rust): its rust_clap_* functions go out in this crate's static library, the one the engine links.
use oscairn_clap_host as _;

/// An object the C++ owns and keeps alive, unmoved, for as long as the plan or call naming it
/// (engine_core.h's contract); shared with other threads only through atomics.
#[repr(transparent)]
pub struct Shared<T>(NonNull<T>);
impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Shared<T> {}
impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: alive and unmoved while named (the contract above); only read here, or through atomics.
        unsafe { self.0.as_ref() }
    }
}

/// A route the C++ owns (engine_core.h's contract: alive while a plan or call names it).
#[derive(Clone, Copy, PartialEq)]
pub struct Route(NonNull<NoteRoute>);

/// Access to the routes during one call into the core: the C++ makes one call at a time on the audio
/// thread and touches no route meanwhile, and `&mut Routes` lends out one route at a time.
pub struct Routes(());
impl Index<Route> for Routes {
    type Output = NoteRoute;
    fn index(&self, r: Route) -> &NoteRoute {
        // SAFETY: a live route (the contract above), not changed while this shared borrow of `self` lasts.
        unsafe { r.0.as_ref() }
    }
}
impl IndexMut<Route> for Routes {
    fn index_mut(&mut self, r: Route) -> &mut NoteRoute {
        // SAFETY: a live route nothing else touches during the call; `&mut self` lends out one at a time.
        unsafe { &mut *r.0.as_ptr() }
    }
}

/// A plan's audio buffers (audio.rs): one allocation, buffer k at k * stride. The C++ plugin hosts
/// write them through pointers between calls into the core (engine_core.h: one call at a time on
/// the audio thread), so they are only ever reached through the raw pointer, never a lasting borrow.
pub struct Buffers {
    base: NonNull<f32>,
    count: usize,
    stride: usize,
}
impl Buffers {
    pub fn new(count: usize, stride: usize) -> Buffers {
        let all = Box::into_raw(vec![0f32; count * stride].into_boxed_slice());
        Buffers { base: NonNull::new(all.cast()).expect("a Box is never null"), count, stride }
    }
    pub fn pointer(&self, k: usize) -> *mut f32 {
        self.base.as_ptr().wrapping_add(k * self.stride)
    }
    /// Buffer k's first n samples.
    pub fn get(&self, k: usize, n: usize) -> &[f32] {
        assert!(k < self.count && n <= self.stride);
        // SAFETY: inside the allocation (asserted); nothing writes it while `self` is borrowed (the contract above).
        unsafe { std::slice::from_raw_parts(self.pointer(k), n) }
    }
    pub fn get_mut(&mut self, k: usize, n: usize) -> &mut [f32] {
        assert!(k < self.count && n <= self.stride);
        // SAFETY: as `get`, and `&mut self` lends out one at a time.
        unsafe { std::slice::from_raw_parts_mut(self.pointer(k), n) }
    }
    /// Buffer `from` to read and buffer `to` to write, two different ones.
    pub fn pair(&mut self, from: usize, to: usize, n: usize) -> (&[f32], &mut [f32]) {
        assert!(from != to && from.max(to) < self.count && n <= self.stride);
        // SAFETY: two different buffers inside the allocation (asserted) do not overlap; as `get_mut`.
        unsafe { (std::slice::from_raw_parts(self.pointer(from), n), std::slice::from_raw_parts_mut(self.pointer(to), n)) }
    }
}
impl Drop for Buffers {
    fn drop(&mut self) {
        // SAFETY: the allocation `new` made, freed once.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.base.as_ptr(), self.count * self.stride)) });
    }
}

/// An object the C++ owns that the core writes on the audio thread (a harmony's last value), one call at a time.
pub struct Mut<T>(NonNull<T>);
impl<T: Copy> Mut<T> {
    pub fn get(&self) -> T {
        // SAFETY: alive and unmoved while named (as Shared); touched by nothing else during the call.
        unsafe { *self.0.as_ptr() }
    }
    pub fn set(&mut self, v: T) {
        // SAFETY: as `get`.
        unsafe { *self.0.as_ptr() = v }
    }
}

/// The caller's channels for one call (the graph's outputs, or the external input), any of them null.
pub struct Channels<'a>(&'a [*mut f32]);
impl Channels<'_> {
    pub fn none() -> Channels<'static> {
        Channels(&[])
    }
    pub fn count(&self) -> usize {
        self.0.len()
    }
    pub fn get(&self, c: usize, frames: usize) -> Option<&[f32]> {
        // SAFETY: null, or `frames` samples the caller lends for the call (engine_core.h).
        self.0.get(c).filter(|p| !p.is_null()).map(|&p| unsafe { std::slice::from_raw_parts(p, frames) })
    }
    pub fn get_mut(&mut self, c: usize, frames: usize) -> Option<&mut [f32]> {
        // SAFETY: as `get`, writable and aliased by nothing else; `&mut self` lends out one at a time.
        self.0.get(c).filter(|p| !p.is_null()).map(|&p| unsafe { std::slice::from_raw_parts_mut(p, frames) })
    }
}

impl oscairn_audio_step {
    pub fn routes(&self) -> &[oscairn_audio_route] {
        // SAFETY: the caller's array, readable during audio_new (engine_core.h).
        unsafe { slice(self.routes, self.route_count) }
    }
    pub fn buses(&self) -> &[Bus] {
        // SAFETY: as `routes`.
        unsafe { slice(self.buses, self.bus_count) }
    }
}

type Emit = oscairn_emit;

/// The caller's emit callback as a closure.
fn emitter(emit: Emit, context: *mut c_void) -> impl FnMut(&Event) -> bool {
    move |e| match emit {
        // SAFETY: the caller's callback and its context, valid for the call that passed them.
        Some(f) => unsafe { f(context, e) },
        None => true,
    }
}

/// A C array as a slice; empty for a null pointer.
/// # Safety
/// `p` is null or points to `n` valid `T`s that outlive `'a`.
unsafe fn slice<'a, T>(p: *const T, n: u32) -> &'a [T] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract.
        unsafe { std::slice::from_raw_parts(p, n as usize) }
    }
}
/// As `slice`, writable.
/// # Safety
/// `p` is null or points to `n` writable `T`s that outlive `'a`, aliased by nothing else.
unsafe fn slice_mut<'a, T>(p: *mut T, n: u32) -> &'a mut [T] {
    if p.is_null() || n == 0 {
        &mut []
    } else {
        // SAFETY: the caller's contract.
        unsafe { std::slice::from_raw_parts_mut(p, n as usize) }
    }
}

#[no_mangle]
pub extern "C" fn rust_core_route_new(ports: u32, clap_ports: u16, fade_frames: u32) -> *mut NoteRoute {
    Box::into_raw(Box::new(NoteRoute::new(ports, clap_ports, fade_frames)))
}

/// # Safety
/// `route` came from rust_core_route_new, or is null, and is not used again.
#[no_mangle]
pub unsafe extern "C" fn rust_core_route_free(route: *mut NoteRoute) {
    if !route.is_null() {
        // SAFETY: the box rust_core_route_new made, handed back once.
        drop(unsafe { Box::from_raw(route) });
    }
}

/// # Safety
/// `route` is a live route.
#[no_mangle]
pub unsafe extern "C" fn rust_core_route_live(route: *const NoteRoute) -> u32 {
    // SAFETY: a live route (engine_core.h's contract).
    unsafe { &*route }.live()
}

/// # Safety
/// `route` is a live route; `pedals` holds 256 bytes.
#[no_mangle]
pub unsafe extern "C" fn rust_core_route_pedals(route: *const NoteRoute, pedals: *mut [[u8; 16]; 16]) {
    // SAFETY: the caller's contract: a live route and 256 writable bytes.
    unsafe { *pedals = (*route).pedal };
}

/// # Safety
/// `route` is a live route; `keys` holds two words.
#[no_mangle]
pub unsafe extern "C" fn rust_core_route_held_keys(route: *const NoteRoute, keys: *mut [u64; 2]) {
    // SAFETY: the caller's contract: a live route and two writable words.
    unsafe { *keys = (*route).held_keys };
}

/// # Safety
/// `route` is a live route; `remaining` holds 256 bytes; `emit` is a valid callback for `context`.
#[no_mangle]
pub unsafe extern "C" fn rust_core_route_retire(route: *mut NoteRoute, remaining: *const [[u8; 16]; 16], emit: Emit, context: *mut c_void) -> bool {
    // SAFETY: the caller's contract: a live route used by nothing else, 256 readable bytes.
    let (route, remaining) = unsafe { (&mut *route, &*remaining) };
    route.retire(remaining, &mut emitter(emit, context))
}

/// # Safety
/// `route` is a live route; `emit` is a valid callback for `context`.
#[no_mangle]
pub unsafe extern "C" fn rust_core_route_let_go(route: *mut NoteRoute, emit: Emit, context: *mut c_void) {
    // SAFETY: a live route used by nothing else during the call (engine_core.h's contract).
    unsafe { &mut *route }.let_go(&mut emitter(emit, context));
}

/// # Safety
/// `route` is a live route; `zones` a NoteZones; `events` holds `count` events, `tags` (or null)
/// `count` bytes and `held` (or null) 2 * `count` words; `next_id` is writable.
#[no_mangle]
pub unsafe extern "C" fn rust_core_route_events(
    route: *mut NoteRoute,
    zones: *const Zones,
    events: *const Event,
    count: u32,
    tags: *const u8,
    skip_ends: bool,
    next_id: *mut u32,
    emit: Emit,
    context: *mut c_void,
    held: *mut [u64; 2],
) {
    // SAFETY: the caller's contract, for each pointer as documented above.
    let (zones, events, tags, held, next_id) = unsafe { (&*zones, slice(events, count), slice(tags, count), slice_mut(held, count), &mut *next_id) };
    let Some(route) = NonNull::new(route).map(Route) else { return };
    route::route_events(&mut Routes(()), route, zones, events, tags, skip_ends, next_id, &mut emitter(emit, context), held);
}

/// # Safety
/// `steps` holds `count` steps as engine_core.h describes, every pointer in them live for the
/// notes' life; `budgeted` holds `count` bools.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_new(steps: *const oscairn_step, count: u32, lanes: u32, budgeted: *mut bool) -> *mut Notes {
    let handle = |p: *mut NoteRoute| NonNull::new(p).map(Route);
    let mut steps_out = Vec::new();
    // SAFETY: the caller's contract: `count` steps, each array in them holding its count, every
    // NoteZones readable for this call (copied here).
    unsafe {
        for s in slice(steps, count) {
            let mut sources = Vec::new();
            for c in slice(s.sources, s.source_count) {
                let (Some(route), Some(&zones)) = (handle(c.route), c.zones.as_ref()) else { continue };
                let peers = slice(c.peers, c.peer_count).iter().filter_map(|&p| handle(p)).collect();
                sources.push(Source { route, zones, peers, channel: c.channel, draining: c.draining, from: c.from });
            }
            let contexts = slice(s.contexts, s.context_count)
                .iter()
                .filter_map(|c| Some(SoundContext { channel: c.channel, activity: Shared(NonNull::new(c.activity)?), id: c.id }));
            let next = slice(s.next, s.next_count).iter().map(|l| (l.step as usize, l.gain)).collect();
            steps_out.push(Step::new(NonNull::new(s.activity).map(Shared), sources, contexts.collect(), s.lane as usize, s.in_process, s.late_lane, next));
        }
    }
    let notes = Notes::new(steps_out, lanes as usize);
    // SAFETY: `count` writable bools (the caller's contract).
    for (out, step) in unsafe { slice_mut(budgeted, count) }.iter_mut().zip(&notes.steps) {
        *out = step.budgeted;
    }
    Box::into_raw(Box::new(notes))
}

/// # Safety
/// `notes` came from rust_core_notes_new, or is null, and is not used again.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_free(notes: *mut Notes) {
    if !notes.is_null() {
        // SAFETY: the box rust_core_notes_new made, handed back once.
        drop(unsafe { Box::from_raw(notes) });
    }
}

/// # Safety
/// `notes` is live and used by no other call; `block` is readable.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_begin(notes: *mut Notes, block: *const Timing) {
    // SAFETY: the caller's contract.
    unsafe { (*notes).begin(*block) };
}

/// # Safety
/// `notes` is live and used by no other call; `keys` holds `count` keys, each event live; `merged`
/// holds `capacity` events and `heard` (or null) `capacity` channels; `next_id` and `overflow` writable.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_route(
    notes: *mut Notes,
    step: u32,
    keys: *mut Key,
    count: u32,
    frames: u32,
    next_id: *mut u32,
    merged: *mut Event,
    capacity: u32,
    heard: *mut i16,
    overflow: *mut bool,
) -> u32 {
    // SAFETY: the caller's contract, for each pointer as documented above.
    let (notes, keys, merged, next_id, overflow, heard) = unsafe {
        (
            &mut *notes,
            slice_mut(keys, count),
            slice_mut(merged, capacity),
            &mut *next_id,
            &mut *overflow,
            heard.as_mut().map(|h| std::slice::from_raw_parts_mut(h, capacity as usize)),
        )
    };
    let (n, lost) = notes.route(&mut Routes(()), step as usize, keys, frames, next_id, merged, heard, *overflow);
    *overflow = lost;
    n as u32
}

/// # Safety
/// `notes` is live and used by no other call; `finish` and its arrays are readable.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_finish(notes: *mut Notes, step: u32, finish: *const oscairn_finish) {
    // SAFETY: the caller's contract: live notes used by nothing else, a readable finish and arrays.
    let (notes, f, voice_notes, sent, importance) = unsafe {
        (
            &mut *notes,
            &*finish,
            slice((*finish).notes, (*finish).note_count),
            slice((*finish).sent, (*finish).sent_count),
            slice((*finish).importance, (*finish).importance_count),
        )
    };
    let rendered = Rendered {
        took: f.took,
        processed: f.processed,
        reset: f.reset,
        missed: f.missed,
        own_voices: f.own_voices,
        honors_volume: f.honors_volume,
        voices: f.voices,
        voice_cost: f.voice_cost,
        notes: voice_notes,
        sent,
        importance,
    };
    notes.finish(&mut Routes(()), step as usize, &rendered);
}

/// # Safety
/// `notes` is live.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_voice_limit(notes: *const Notes, step: u32, voices: u32, contexts: *mut u32, count: u32) -> u32 {
    // SAFETY: live notes and `count` writable limits (the caller's contract).
    unsafe { (&*notes).voice_limit(&Routes(()), step as usize, voices, slice_mut(contexts, count)) }
}

/// # Safety
/// `notes` is live and used by no other call.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_end(notes: *mut Notes, slack: f32) {
    // SAFETY: live notes used by nothing else (the caller's contract).
    unsafe { &mut *notes }.end(&mut Routes(()), slack);
}

/// # Safety
/// `steps` and `outputs` are readable arrays of `count` and `output_count`, their arrays readable too.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_new(
    steps: *const oscairn_audio_step,
    count: u32,
    outputs: *const oscairn_audio_route,
    output_count: u32,
    inputs: u32,
    channels: u32,
    max_frames: u32,
) -> *mut Audio {
    // SAFETY: the caller's arrays (above).
    let (steps, outputs) = unsafe { (slice(steps, count), slice(outputs, output_count)) };
    Box::into_raw(Box::new(Audio::new(steps, outputs, inputs, channels, max_frames)))
}

/// # Safety
/// `audio` came from rust_core_audio_new and is freed once.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_free(audio: *mut Audio) {
    // SAFETY: the caller's contract.
    drop(unsafe { Box::from_raw(audio) });
}

/// # Safety
/// `audio` is live; `step` is -1 or one of its steps.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_buffers(audio: *mut Audio, step: i32, outputs: bool) -> *const *mut f32 {
    // SAFETY: live audio (the caller's contract).
    unsafe { &*audio }.pointers(usize::try_from(step).ok(), outputs).as_ptr()
}

/// # Safety
/// `next` and `current` are live, different, and used by no other call.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_carry(next: *mut Audio, current: *const Audio, ramp: u32) {
    // SAFETY: the caller's contract.
    unsafe { (*next).carry(&*current, ramp) }
}

/// # Safety
/// `audio` is live and used by no other call; `inputs` and `outputs` hold `ni` and `no` channels (each
/// null or `frames` samples, outputs writable), and `frames` is at most the plan's max_frames.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_begin(audio: *mut Audio, inputs: *const *const f32, ni: u32, outputs: *const *mut f32, no: u32, frames: u32) {
    // SAFETY: the caller's contract.
    let (audio, inputs, mut outputs) = unsafe { (&mut *audio, Channels(slice(inputs.cast(), ni)), Channels(slice(outputs, no))) };
    audio.begin(&inputs, &mut outputs, frames as usize);
}

/// # Safety
/// As rust_core_audio_begin, for the graph's outputs (step == its count) at least as many as it has; live_from
/// and live_to hold Graph::liveGains gains each.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_mix(audio: *mut Audio, step: u32, outputs: *const *mut f32, frames: u32, live_from: *const f32, live_to: *const f32) {
    // SAFETY: the caller's contract.
    let (audio, live_from, live_to) = unsafe { (&mut *audio, slice(live_from, 256), slice(live_to, 256)) };
    // SAFETY: as many outputs as the graph has, for the outputs' mix alone.
    let mut outputs = Channels(unsafe { slice(outputs, if outputs.is_null() { 0 } else { audio.output_channels() }) });
    audio.mix(step as usize, &mut outputs, frames as usize, live_from, live_to);
}

/// # Safety
/// `audio` is live; `step` is one of its steps; `frames` as in rust_core_audio_begin.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_quiet(audio: *const Audio, step: u32, frames: u32) -> bool {
    // SAFETY: the caller's contract.
    unsafe { &*audio }.quiet(step as usize, frames as usize)
}

/// # Safety
/// `audio` is live and used by no other call; `step` is one of its steps; `frames` as in rust_core_audio_begin.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_silence(audio: *mut Audio, step: u32, frames: u32) {
    // SAFETY: the caller's contract.
    unsafe { &mut *audio }.silence(step as usize, frames as usize);
}

/// # Safety
/// `audio` is live and used by no other call; `step` is one of its steps; `frames` as in rust_core_audio_begin.
#[no_mangle]
pub unsafe extern "C" fn rust_core_audio_finish(audio: *mut Audio, step: u32, frames: u32, processed: bool, asleep: bool, fade: u32) {
    // SAFETY: the caller's contract.
    unsafe { &mut *audio }.finish(step as usize, frames as usize, processed, asleep, fade);
}

// oscairn_plugins: the C++ plugin hosts' calls for the run (engine_core.h), every one set. What a call
// returns lives until that step renders again.
macro_rules! calls {
    ($($name:ident($($a:ident: $t:ty),*) -> $r:ty;)*) => {
        impl oscairn_plugins {
            $(pub fn $name(&self, s: usize $(, $a: $t)*) -> $r {
                // SAFETY: the C++'s callback (engine_core.h), called with its context and a step of the plan.
                unsafe { (self.$name.expect(stringify!($name)))(self.context, s as u32 $(, $a)*) }
            })*
        }
    };
}
calls! {
    finish() -> bool; late() -> bool; panic() -> (); kept() -> bool; was_reset() -> bool; asleep() -> bool;
    own_voices() -> bool; honors_volume() -> bool; missed() -> u64; took() -> f64; sound(c: u32) -> u32;
    set_limits(limit: u32, parts: *const oscairn_voice_budget_context_t, n: u32) -> ();
    start(e: *const Event, n: u32) -> bool; render(e: *const Event, n: u32) -> bool; rest(frames: u32) -> bool;
}
impl oscairn_plugins {
    pub fn sent(&self, s: usize) -> &[Event] {
        let mut n = 0;
        // SAFETY: as calls!; the host's events, `n` of them, alive until the step renders again.
        unsafe { slice((self.sent.expect("sent"))(self.context, s as u32, &mut n), n) }
    }
    /// Its voices sounding, its cost per voice and its notes (oscairn.voice-budget's report).
    pub fn voices(&self, s: usize) -> (u32, f64, &[VoiceNote]) {
        let (mut cost, mut notes, mut n) = (0.0, std::ptr::null(), 0);
        // SAFETY: as `sent`.
        unsafe { ((self.voices.expect("voices"))(self.context, s as u32, &mut cost, &mut notes, &mut n), cost, slice(notes, n)) }
    }
    /// Draft 2's importance report: a list per context (RFC 0046).
    pub fn importance(&self, s: usize) -> &[oscairn_voice_budget_importance_t] {
        let mut lists = std::ptr::null();
        // SAFETY: as `sent`.
        unsafe { slice(lists, (self.importance.expect("importance"))(self.context, s as u32, &mut lists)) }
    }
    /// A shared engine's context `c`'s parameters set since its last block, each to `emit`.
    pub fn parameters(&self, s: usize, c: usize, mut emit: &mut dyn FnMut(&Event) -> bool) {
        unsafe extern "C" fn call(context: *mut c_void, e: *const Event) -> bool {
            // SAFETY: the closure passed below, for this call; an event the host lends for the call.
            unsafe { (*context.cast::<&mut dyn FnMut(&Event) -> bool>())(&*e) }
        }
        // SAFETY: as calls!; `emit` outlives the call.
        unsafe { (self.parameters.expect("parameters"))(self.context, s as u32, c as u32, Some(call), std::ptr::from_mut(&mut emit).cast()) }
    }
    pub fn listen(&self, node: u32, e: &[Event], sent: bool) {
        // SAFETY: as calls!.
        unsafe { (self.listen.expect("listen"))(self.context, node, e.as_ptr(), e.len() as u32, sent) }
    }
    pub fn now(&self) -> i64 {
        // SAFETY: as calls!.
        unsafe { (self.now.expect("now"))(self.context) }
    }
    pub fn deadline(&self, at: i64, only: bool) {
        // SAFETY: as calls!.
        unsafe { (self.deadline.expect("deadline"))(self.context, at, only) }
    }
    /// The wave's `count` local steps rendered on the audio helpers and here (Run::render_local).
    pub fn helpers(&self, run: &Run, count: usize) {
        struct Job<'a>(&'a Run, &'a oscairn_plugins);
        unsafe extern "C" fn job(context: *mut c_void, k: u32) {
            // SAFETY: the Job below, alive and only read (atomics aside) while the helpers run.
            let Job(run, plugins) = unsafe { &*context.cast::<Job>() };
            run.render_local(plugins, k as usize);
        }
        let j = Job(run, self);
        // SAFETY: as calls!; the job and its context outlive the call.
        unsafe { (self.helpers.expect("helpers"))(self.context, count as u32, Some(job), std::ptr::from_ref(&j).cast_mut().cast()) }
    }
}

/// # Safety
/// As engine_core.h's run_new: `notes` and `audio` live and outliving the run, every array readable
/// with its count, every route and zones live for the run's life (zones are copied).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn rust_core_run_new(
    notes: *mut Notes,
    audio: *mut Audio,
    steps: *const oscairn_step,
    budgeted: *const bool,
    count: u32,
    waves: *const u32,
    wave_count: u32,
    harmonies: *const oscairn_harmony_source,
    harmony_count: u32,
    taps: *const oscairn_tap,
    tap_count: u32,
    controls: *const oscairn_control,
    control_count: u32,
) -> *mut Run {
    // SAFETY: the caller's contract, for each array and pointer.
    unsafe {
        let route = |p: *mut NoteRoute| Route(NonNull::new(p).expect("a route"));
        let budgeted = slice(budgeted, count);
        let steps_in = slice(steps, count)
            .iter()
            .zip(budgeted)
            .map(|(s, &b)| run::StepIn {
                host: s.host as usize,
                id: s.id,
                harmony: s.harmony,
                routes: slice(s.lane_routes, s.lane_route_count).to_vec(),
                lanes_in: s.lanes_in,
                budgeted: b,
                may_rest: s.may_rest,
            })
            .collect();
        let parts = slice(harmonies, harmony_count)
            .iter()
            .map(|h| HarmonySource {
                route: route(h.route),
                zones: *h.zones,
                input: h.input,
                root: h.root,
                scale: h.scale,
                last: Mut(NonNull::new(h.last).expect("a harmony")),
            })
            .collect();
        let taps =
            slice(taps, tap_count).iter().map(|t| Tap { step: t.step, route: route(t.route), zones: *t.zones, ports: t.ports, channel: t.channel }).collect();
        let controls =
            slice(controls, control_count).iter().map(|c| Control { path: slice(c.path, c.count).to_vec(), origin: c.origin, modulate: c.modulate }).collect();
        let (notes, audio) = (NonNull::new(notes).expect("notes"), NonNull::new(audio).expect("audio"));
        let contexts = |s: usize| slice(steps, count)[s].context_count as usize;
        Box::into_raw(Box::new(Run::new(notes, audio, steps_in, slice(waves, wave_count), parts, taps, controls, contexts)))
    }
}

/// # Safety
/// `run` came from rust_core_run_new and is freed once.
#[no_mangle]
pub unsafe extern "C" fn rust_core_run_free(run: *mut Run) {
    // SAFETY: the caller's contract.
    drop(unsafe { Box::from_raw(run) });
}

/// # Safety
/// Two live, different runs, on the audio thread, nothing else using them or their notes and audio;
/// `midi` holds Graph::maxMidiOut messages; `drops` is an atomic.
#[no_mangle]
pub unsafe extern "C" fn rust_core_run_adopt(
    next: *mut Run,
    current: *mut Run,
    ramp: bool,
    ramp_frames: u32,
    midi: *mut oscairn_midi,
    midi_count: *mut u32,
    drops: *mut u64,
) {
    // SAFETY: the caller's contract.
    unsafe {
        let (next, current) = (&mut *next, &*current);
        next.adopt(current, &mut Routes(()), next.notes.as_ref(), current.notes.as_ref(), slice_mut(midi, 1024), &mut *midi_count, AtomicU64::from_ptr(drops));
        if ramp {
            (*next.audio.as_ptr()).carry(current.audio.as_ref(), ramp_frames);
        }
    }
}

/// # Safety
/// A live run, its notes and audio used by nothing else; `block` and `plugins` as engine_core.h says.
#[no_mangle]
pub unsafe extern "C" fn rust_core_run_block(run: *mut Run, block: *const oscairn_run_block, plugins: *const oscairn_plugins) -> bool {
    // SAFETY: the caller's contract, for each pointer and array.
    unsafe {
        let (run, b, plugins) = (&mut *run, &*block, &*plugins);
        let (notes, audio) = (&mut *run.notes.as_ptr(), &mut *run.audio.as_ptr());
        let mut block = Block {
            inputs: Channels(slice(b.inputs.cast(), b.ni)),
            outputs: Channels(slice(b.outputs, b.no)),
            frames: b.frames,
            fade: b.fade,
            events: slice(b.events, b.count),
            tags: (!b.tags.is_null()).then(|| slice(b.tags, b.count)),
            params: slice(b.params, b.param_count),
            beat: &*b.beat,
            panic_input: b.panic_input,
            panic_notes: b.panic_notes,
            listening: b.listening,
            parallel: b.parallel,
            deadlines: b.deadlines,
            transport_wake: b.transport_wake,
            timing: b.timing,
            deadline: b.deadline,
            worth: b.worth,
            live_from: slice(b.live_from, 256),
            live_to: slice(b.live_to, 256),
            next_id: &mut *b.next_id,
            drops: AtomicU64::from_ptr(b.drops),
            midi: slice_mut(b.midi, 1024),
            midi_count: &mut *b.midi_count,
        };
        run.block(&mut Routes(()), notes, audio, &mut block, plugins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use budget::Activity;
    use route::UNTAGGED;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::mem::{offset_of, size_of};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    // The layouts the C++ shares (engine_core.h; graph.cpp asserts the C++ side).
    #[test]
    fn layouts() {
        assert_eq!(size_of::<Event>(), 64);
        assert_eq!(size_of::<Activity>(), 144);
        assert_eq!((offset_of!(Activity, rest), offset_of!(Activity, stolen), offset_of!(Activity, lane_limit)), (28, 48, 72));
        assert_eq!((offset_of!(Activity, stranded), offset_of!(Activity, per_note_scale), offset_of!(Activity, over_limit)), (92, 96, 104));
        assert_eq!(size_of::<Zones>(), 8 * 72 + 8);
        assert_eq!(size_of::<VoiceNote>(), 20);
        assert_eq!(size_of::<Key>(), 24);
    }

    // bindings.rs was generated from these headers as they are (the engine core's and the engine-worker shared block's) (core/engine/rust/bindgen regenerates it).
    #[test]
    fn engine_core_h_unchanged() {
        let fnv = |b: &[u8]| b.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
        let now = [
            fnv(include_bytes!("../../src/engine_core.h")),
            fnv(include_bytes!("../../../clap-ext/include/oscairn-clap/voice-budget.h")),
            fnv(include_bytes!("../../../plugin-host/src/worker_shared.h")),
        ];
        assert_eq!(now, bindings::MADE_FROM, "engine_core.h, voice-budget.h or worker_shared.h changed: regenerate src/bindings.rs with core/engine/rust/bindgen");
    }

    /// Counts the allocations this thread makes while `COUNTING` is set.
    struct Counting;
    pub(crate) static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    thread_local!(pub(crate) static COUNTING: Cell<bool> = const { Cell::new(false) });
    // SAFETY: System's allocator, unchanged; only counted.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if COUNTING.with(Cell::get) {
                ALLOCATIONS.fetch_add(1, Relaxed);
            }
            // SAFETY: the caller's layout, passed on.
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
            // SAFETY: memory System.alloc gave, with its layout.
            unsafe { System.dealloc(p, layout) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    // A block allocates nothing: routing a flood of notes past a tight budget (steals, chokes, fades),
    // NOTE_ENDs, the watchdog, the lanes judged, lost blocks, the callback's slack running low.
    #[test]
    fn blocks_allocate_nothing() {
        let activity = Activity::default();
        activity.rest.set(-1.0);
        activity.per_note.set(-1.0);
        activity.per_note_scale.set(1.0);
        activity.seed_note.set(40.0);
        let zones = Zones::default();
        let routes = [rust_core_route_new(1, 1, 64), rust_core_route_new(1, 1, 0)];
        let sources =
            routes.map(|route| oscairn_source { route, zones: &zones, peers: std::ptr::null(), peer_count: 0, channel: -1, draining: false, from: -1 });
        let step = oscairn_step {
            activity: std::ptr::from_ref(&activity).cast_mut(),
            sources: sources.as_ptr(),
            source_count: 2,
            contexts: std::ptr::null(),
            context_count: 0,
            lane: 0,
            in_process: false,
            late_lane: false,
            next: std::ptr::null(),
            next_count: 0,
            host: std::ptr::null(),
            id: 0,
            harmony: -1,
            lane_routes: std::ptr::null(),
            lane_route_count: 0,
            lanes_in: false,
            may_rest: false,
        };
        let mut budgeted = false;
        // SAFETY: one step whose arrays and pointers outlive the notes.
        let notes = unsafe { rust_core_notes_new(&step, 1, 1, &mut budgeted) };
        let timing = Timing { budget: 300.0, period: 2666.0, deadline: 2266.0, allowance: 1333.0, cores: 1.0, quiet_after: 256, low_slack: 0.0 };
        let (mut merged, mut next_id) = ([Event::default(); 1057], 0u32);
        let mut events = [Event::default(); 64];
        let mut end = Event::note(false, -1, 0.0, 0, -1, -1);
        end.set_kind(event::NOTE_END);
        COUNTING.with(|c| c.set(true));
        for b in 0..2000u32 {
            for (i, e) in events.iter_mut().enumerate() {
                *e = Event::note(i % 3 != 0, 36 + (b as i16 + i as i16) % 40, 0.5, i as u32, 0, 0);
            }
            let mut keys: [Key; 64] = std::array::from_fn(|i| Key {
                time: i as u32,
                order: i as u32,
                event: Shared(NonNull::from(&events[i])),
                source: (b % 2) as i16,
                tag: UNTAGGED,
                after_notes: false,
            });
            let mut overflow = false;
            let finish = oscairn_finish {
                took: 500.0,
                processed: true,
                reset: b % 97 == 0,
                missed: b % 3 == 0,
                own_voices: false,
                honors_volume: false,
                voices: 0,
                voice_cost: 0.0,
                notes: std::ptr::null(),
                note_count: 0,
                sent: &end,
                sent_count: u32::from(b % 5 == 0),
                importance: std::ptr::null(),
                importance_count: 0,
            };
            // SAFETY: live notes and buffers of the sizes given, for these calls.
            unsafe {
                rust_core_notes_begin(notes, &timing);
                rust_core_notes_route(notes, 0, keys.as_mut_ptr(), 64, 128, &mut next_id, merged.as_mut_ptr(), 1057, std::ptr::null_mut(), &mut overflow);
                rust_core_notes_finish(notes, 0, &finish);
                rust_core_notes_end(notes, if b % 2 == 0 { -10.0 } else { f32::NAN });
            }
        }
        COUNTING.with(|c| c.set(false));
        assert!(budgeted && activity.stolen.load(Relaxed) + activity.hard_chokes.load(Relaxed) > 0, "notes were given up");
        assert!(activity.faded_for_loss.load(Relaxed) > 0 && activity.faded_for_deadline.load(Relaxed) > 0, "for lost blocks and the deadline too");
        // A plan's audio too: mixes with live gains and a played change's ramp, a bad block faded, meters, activity.
        let connections = [
            oscairn_audio_route { source: 0, from: 0, to: 0, gain: 0.5, live: [1, 2, 0, 0] },
            oscairn_audio_route { source: 0, from: 1, to: 0, gain: 1.0, live: [0; 4] },
        ];
        let mut peaks = [0u32; 16];
        let steps = [0u32, 1].map(|s| oscairn_audio_step {
            host: std::ptr::null::<u8>().wrapping_add(s as usize + 1).cast(),
            inputs: 2 * s,
            outputs: 2,
            metered: 2,
            peaks: std::ptr::from_mut(&mut peaks).cast(),
            activity: std::ptr::from_ref(&activity).cast_mut(),
            routes: if s == 1 { connections.as_ptr() } else { std::ptr::null() },
            route_count: 2 * s,
            buses: std::ptr::null(),
            bus_count: 0,
        });
        // SAFETY: arrays that outlive the calls; the audio freed once.
        unsafe {
            let (was, audio) = (
                rust_core_audio_new(steps.as_ptr(), 2, connections.as_ptr(), 2, 0, 2, 128),
                rust_core_audio_new(steps.as_ptr(), 2, connections.as_ptr(), 2, 0, 2, 128),
            );
            let (mut out, live_from, live_to) = ([0f32; 256], [0.5f32; 256], [1f32; 256]);
            let outs = [out.as_mut_ptr(), out.as_mut_ptr().add(128)];
            COUNTING.with(|c| c.set(true));
            rust_core_audio_carry(audio, was, 64);
            for b in 0..200u32 {
                rust_core_audio_begin(audio, std::ptr::null(), 0, outs.as_ptr(), 2, 128);
                for s in 0..2 {
                    rust_core_audio_mix(audio, s, std::ptr::null(), 128, live_from.as_ptr(), live_to.as_ptr());
                    *(*rust_core_audio_buffers(audio, s as i32, true)).add(5) = if b % 7 == 0 { f32::NAN } else { 0.25 };
                    rust_core_audio_finish(audio, s, 128, true, false, 48);
                }
                rust_core_audio_mix(audio, 2, outs.as_ptr(), 128, live_from.as_ptr(), live_to.as_ptr());
            }
            COUNTING.with(|c| c.set(false));
            assert!(activity.bad_output.load(Relaxed) > 0 && peaks[0] == 0.25f32.to_bits() && out[5] != 0.0);
            rust_core_audio_free(was);
            rust_core_audio_free(audio);
        }
        assert_eq!(ALLOCATIONS.load(Relaxed), 0);
        // SAFETY: made above, freed once.
        unsafe {
            rust_core_notes_free(notes);
            for r in routes {
                rust_core_route_free(r);
            }
        }
    }
}
