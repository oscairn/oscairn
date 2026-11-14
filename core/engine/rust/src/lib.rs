//! The engine thread's core in Rust: note routing, the voice budget and the budget's per-block
//! bookkeeping, behind the C interface in core/engine/src/engine_core.h (names rust_core_*). The
//! C++ implementation of the same interface (core_cpp.cpp) is the reference the differential test
//! holds this to. All unsafe code is in this file, at that boundary.

mod budget;
mod event;
mod notes;
mod route;

use budget::Activity;
use event::{Event, CORE_SPACE, NOTE_END};
use notes::{Key, Notes, Rendered, SoundContext, Source, Step, Timing, VoiceNote};
use route::{Context, NoteRoute, Zones, UNTAGGED};
use std::ffi::c_void;
use std::ops::{Deref, Index, IndexMut};
use std::ptr::NonNull;

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

type Emit = Option<unsafe extern "C" fn(*mut c_void, *const Event) -> bool>;

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
    // SAFETY: the caller's contract.
    if p.is_null() || n == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(p, n as usize) }
    }
}
/// As `slice`, writable.
/// # Safety
/// `p` is null or points to `n` writable `T`s that outlive `'a`, aliased by nothing else.
unsafe fn slice_mut<'a, T>(p: *mut T, n: u32) -> &'a mut [T] {
    // SAFETY: the caller's contract.
    if p.is_null() || n == 0 {
        &mut []
    } else {
        unsafe { std::slice::from_raw_parts_mut(p, n as usize) }
    }
}

#[repr(C)]
pub struct CSource {
    route: *mut NoteRoute,
    zones: *const Zones,
    peers: *const *mut NoteRoute,
    peer_count: u32,
    channel: i16,
    draining: bool,
}
#[repr(C)]
pub struct CContext {
    channel: i16,
    activity: *mut Activity,
}
#[repr(C)]
pub struct CStep {
    activity: *mut Activity,
    sources: *const CSource,
    source_count: u32,
    contexts: *const CContext,
    context_count: u32,
    lane: u32,
    in_process: bool,
    late_lane: bool,
}
#[repr(C)]
pub struct CFinish {
    took: f32,
    processed: bool,
    reset: bool,
    own_voices: bool,
    honors_volume: bool,
    voices: u32,
    voice_cost: f64,
    notes: *const VoiceNote,
    note_count: u32,
    sent: *const Event,
    sent_count: u32,
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
pub unsafe extern "C" fn rust_core_route_clap(route: *const NoteRoute) -> bool {
    // SAFETY: a live route (engine_core.h's contract).
    unsafe { &*route }.any_clap()
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
    let rs = &mut Routes(());
    let mut emit = emitter(emit, context);
    for (i, e) in events.iter().enumerate() {
        if e.space() != CORE_SPACE || (skip_ends && e.kind() == NOTE_END) {
            continue;
        }
        let mut cx = Context { zones: Some(zones), source: tags.get(i).copied().unwrap_or(UNTAGGED), draining: false, peers: &[], budget: None };
        route::route(rs, route, e, next_id, &mut emit, &mut cx);
        if let Some(h) = held.get_mut(i) {
            *h = rs[route].held_keys;
        }
    }
}

/// # Safety
/// `steps` holds `count` steps as engine_core.h describes, every pointer in them live for the
/// notes' life; `budgeted` holds `count` bools.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_new(steps: *const CStep, count: u32, lanes: u32, budgeted: *mut bool) -> *mut Notes {
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
                sources.push(Source { route, zones, peers, channel: c.channel, draining: c.draining });
            }
            let contexts = slice(s.contexts, s.context_count)
                .iter()
                .filter_map(|c| Some(SoundContext { channel: c.channel, activity: Shared(NonNull::new(c.activity)?) }));
            steps_out.push(Step::new(NonNull::new(s.activity).map(Shared), sources, contexts.collect(), s.lane as usize, s.in_process, s.late_lane));
        }
    }
    let notes = Notes::new(&Routes(()), steps_out, lanes as usize);
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
pub unsafe extern "C" fn rust_core_notes_finish(notes: *mut Notes, step: u32, finish: *const CFinish) {
    // SAFETY: the caller's contract: live notes used by nothing else, a readable finish and arrays.
    let (notes, f, voice_notes, sent) =
        unsafe { (&mut *notes, &*finish, slice((*finish).notes, (*finish).note_count), slice((*finish).sent, (*finish).sent_count)) };
    let rendered = Rendered {
        took: f.took,
        processed: f.processed,
        reset: f.reset,
        own_voices: f.own_voices,
        honors_volume: f.honors_volume,
        voices: f.voices,
        voice_cost: f.voice_cost,
        notes: voice_notes,
        sent,
    };
    notes.finish(&mut Routes(()), step as usize, &rendered);
}

/// # Safety
/// `notes` is live.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_voice_limit(notes: *const Notes, step: u32, voices: u32) -> u32 {
    // SAFETY: live notes (the caller's contract).
    unsafe { &*notes }.voice_limit(&Routes(()), step as usize, voices)
}

/// # Safety
/// `notes` is live and used by no other call.
#[no_mangle]
pub unsafe extern "C" fn rust_core_notes_end(notes: *mut Notes) {
    // SAFETY: live notes used by nothing else (the caller's contract).
    unsafe { &mut *notes }.end(&mut Routes(()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::mem::{offset_of, size_of};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    // The layouts the C++ shares (engine_core.h; core_cpp.cpp asserts the C++ side).
    #[test]
    fn layouts() {
        assert_eq!(size_of::<Event>(), 64);
        assert_eq!(size_of::<Activity>(), 112);
        assert_eq!((offset_of!(Activity, rest), offset_of!(Activity, stolen), offset_of!(Activity, lane_limit)), (28, 48, 72));
        assert_eq!((offset_of!(Activity, stranded), offset_of!(Activity, per_note_scale), offset_of!(Activity, over_limit)), (92, 96, 104));
        assert_eq!(size_of::<Zones>(), 8 * 72 + 8);
        assert_eq!(size_of::<VoiceNote>(), 20);
        assert_eq!(size_of::<Key>(), 24);
    }

    /// Counts the allocations this thread makes while `COUNTING` is set.
    struct Counting;
    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    thread_local!(static COUNTING: Cell<bool> = const { Cell::new(false) });
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
    // NOTE_ENDs, the watchdog, the lanes judged.
    #[test]
    fn blocks_allocate_nothing() {
        let activity = Activity::default();
        activity.rest.set(-1.0);
        activity.per_note.set(-1.0);
        activity.per_note_scale.set(1.0);
        activity.seed_note.set(40.0);
        let zones = Zones::default();
        let routes = [rust_core_route_new(1, 1, 64), rust_core_route_new(1, 1, 0)];
        let sources = routes.map(|route| CSource { route, zones: &zones, peers: std::ptr::null(), peer_count: 0, channel: -1, draining: false });
        let step = CStep {
            activity: std::ptr::from_ref(&activity).cast_mut(),
            sources: sources.as_ptr(),
            source_count: 2,
            contexts: std::ptr::null(),
            context_count: 0,
            lane: 0,
            in_process: false,
            late_lane: false,
        };
        let mut budgeted = false;
        // SAFETY: one step whose arrays and pointers outlive the notes.
        let notes = unsafe { rust_core_notes_new(&step, 1, 1, &mut budgeted) };
        let timing = Timing { budget: 300.0, period: 2666.0, deadline: 2266.0, allowance: 1333.0, cores: 1.0, quiet_after: 256 };
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
            let finish = CFinish {
                took: 500.0,
                processed: true,
                reset: b % 97 == 0,
                own_voices: false,
                honors_volume: false,
                voices: 0,
                voice_cost: 0.0,
                notes: std::ptr::null(),
                note_count: 0,
                sent: &end,
                sent_count: u32::from(b % 5 == 0),
            };
            // SAFETY: live notes and buffers of the sizes given, for these calls.
            unsafe {
                rust_core_notes_begin(notes, &timing);
                rust_core_notes_route(notes, 0, keys.as_mut_ptr(), 64, 128, &mut next_id, merged.as_mut_ptr(), 1057, std::ptr::null_mut(), &mut overflow);
                rust_core_notes_finish(notes, 0, &finish);
                rust_core_notes_end(notes);
            }
        }
        COUNTING.with(|c| c.set(false));
        assert!(budgeted && activity.stolen.load(Relaxed) + activity.hard_chokes.load(Relaxed) > 0, "notes were given up");
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
