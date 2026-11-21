//! A plan's block (engine_core.h's oscairn_run; Graph::process and beginBlock): the steps wave by
//! wave, each step's events merged and routed through the notes, its plugin called (through
//! oscairn_plugins), what it sent passed on as parameter-target lanes, MIDI taps and to the listener,
//! the parts' harmonies. Events stay the plugins' 64-byte CLAP events from end to end: what a plugin
//! sent is read where it lies, keys point at events, the merged block is what the plugin is handed.

use crate::audio::Audio;
use crate::bindings::{
    oscairn_curve as Curve, oscairn_lane_route as LaneRoute, oscairn_midi as Midi, oscairn_param as Param, oscairn_voice_budget_context_t as VoiceContext,
};
use crate::budget::F32;
use crate::event::*;
use crate::notes::{Key, Notes, Rendered, Timing};
use crate::route::{self, Zones, UNTAGGED};
use crate::{oscairn_plugins as Plugins, Channels, Mut, Route, Routes, Shared};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

const MAX_BLOCK_EVENTS: usize = 512; // Host::maxBlockEvents
const MAX_PROCESS_EVENTS: usize = 2 * MAX_BLOCK_EVENTS + 1 + 32; // Host::maxProcessEvents
const MAX_MIDI_OUT: u32 = 1024; // Graph::maxMidiOut
const PARAM_TARGET_SPACE: u16 = 1000; // Host::paramTargetSpace; OSCAIRN_EVENT_PARAM_TARGET is 0
const HARMONY_SPACE: u16 = 1002; // Host::harmonySpace; OSCAIRN_EVENT_HARMONY is 0
const MAX_CONTEXTS: usize = 16; // OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS
const SELECTED: u32 = u32::MAX; // OSCAIRN_VOICE_BUDGET_SELECTED

/// oscairn_harmony_t (oscairn-clap/harmony.h).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Harmony {
    key_root: u8,
    chord_root: u8,
    chord_type: u8,
    chord_bass: u8,
    scale: u16,
    reserved: u16,
    held: [u64; 2],
}
const NO_LIMITS: VoiceContext = VoiceContext { context: 0, limit: 0, reserve: 0, priority: 0, flags: 0 };
const NO_MIDI: Midi = Midi { time: 0, ports: 0, size: 0, bytes: [0; 3] };

/// One block (oscairn_run_block), its arrays as slices.
pub struct Block<'a> {
    pub inputs: Channels<'a>,
    pub outputs: Channels<'a>,
    pub frames: u32,
    pub fade: u32,
    pub events: &'a [Event],
    pub tags: Option<&'a [u8]>,
    pub params: &'a [Param],
    pub beat: &'a Event,
    pub panic_input: bool,
    pub panic_notes: bool,
    pub listening: bool,
    pub parallel: bool,
    pub deadlines: bool,
    pub transport_wake: bool,
    pub timing: Timing,
    pub deadline: i64,
    pub worth: f32,
    pub live_from: &'a [f32],
    pub live_to: &'a [f32],
    pub next_id: &'a mut u32,
    pub drops: &'a AtomicU64,
    pub midi: &'a mut [Midi],
    pub midi_count: &'a mut u32,
}

pub struct HarmonySource {
    pub route: Route,
    pub zones: Zones,
    pub input: bool,
    pub root: u8,
    pub scale: u16,
    pub last: Mut<Harmony>,
}
struct Part {
    s: HarmonySource,
    events: Vec<Event>,  // this block's, at most 32
    keys: Vec<[u64; 2]>, // the keys held after each input event
}
pub struct Tap {
    pub step: i32,
    pub route: Route,
    pub zones: Zones,
    pub ports: u64,
    pub channel: u8,
}
pub struct Control {
    pub path: Vec<Curve>,
    pub origin: f64,
    pub modulate: bool,
}
pub struct StepIn {
    pub host: usize,
    pub id: u32,
    pub harmony: i32,
    pub routes: Vec<LaneRoute>,
    pub lanes_in: bool,
    pub budgeted: bool,
    pub may_rest: bool,
}

pub struct Step {
    host: usize,
    id: u32,
    harmony: i32,
    budgeted: bool,
    may_rest: bool, // the rest policy lets its plugin rest at all (the plan's, once): else no rest check
    routes: Vec<LaneRoute>,
    releases: Vec<Event>, // at most MAX_BLOCK_EVENTS
    release_overflow: bool,
    lanes: Vec<(Event, i16)>, // lane events for it this block, each with the note connection it addresses
    lanes_in: bool,
    contexts_in: Vec<Event>, // its contexts' parameters this block, on their channels
    merged: Vec<Event>,
    merged_count: usize,
    heard: Vec<i16>,           // a shared engine's: each merged event's channel as it came
    pub processed: AtomicBool, // written on the audio helpers too
    pub cost: F32,             // the microseconds its last rendering took here, or its worker's round trip
    started: bool,
    handed: i64,
    missed: u64,
    voice_limits: [VoiceContext; MAX_CONTEXTS],
    voice_limit: u32,
    voice_contexts: usize,
}
impl Step {
    pub fn merged(&self) -> &[Event] {
        &self.merged[..self.merged_count]
    }
}

pub struct Run {
    pub notes: NonNull<Notes>,
    pub audio: NonNull<Audio>,
    pub steps: Vec<Step>,
    parts: Vec<Part>,
    taps: Vec<Tap>,
    controls: Vec<Control>,
    waves: Vec<u32>,
    pub local: Vec<u32>,
    after: Vec<f32>,
    keys: Vec<Key>,
}

/// The harmony the held keys read as (oscairn::recognizeChord): the bass's reading first, else the
/// first shape in the table's order on any root.
fn recognize(h: &mut Harmony) {
    const SHAPES: [(&[u8], u8); 22] = [
        (&[0, 4, 7], 1),
        (&[0, 3, 7], 2),
        (&[0, 3, 6], 3),
        (&[0, 4, 8], 4),
        (&[0, 2, 7], 5),
        (&[0, 5, 7], 6),
        (&[0, 7], 7),
        (&[0, 4, 7, 9], 8),
        (&[0, 3, 7, 9], 9),
        (&[0, 4, 7, 10], 10),
        (&[0, 4, 7, 11], 11),
        (&[0, 3, 7, 10], 12),
        (&[0, 3, 7, 11], 13),
        (&[0, 3, 6, 10], 14),
        (&[0, 3, 6, 9], 15),
        (&[0, 4, 8, 10], 16),
        (&[0, 5, 7, 10], 17),
        (&[0, 2, 4, 7], 18),
        (&[0, 2, 3, 7], 19),
        (&[0, 2, 4, 7, 10], 20),
        (&[0, 2, 4, 7, 11], 21),
        (&[0, 2, 3, 7, 10], 22),
    ];
    let mask = |s: &[u8]| s.iter().fold(0u16, |m, &i| m | 1 << i);
    (h.chord_root, h.chord_type, h.chord_bass) = (0, 0, 0);
    let (mut pitches, mut lowest) = (0u16, None);
    for k in (0..128).filter(|k| h.held[k / 64] >> (k % 64) & 1 != 0) {
        pitches |= 1 << (k % 12);
        lowest.get_or_insert(k);
    }
    let Some(lowest) = lowest else { return };
    h.chord_bass = (lowest % 12) as u8;
    let above = |root: u16| ((pitches >> root) | (pitches << (12 - root))) & 0xfff;
    if let Some(&(_, t)) = SHAPES.iter().find(|s| mask(s.0) == above(u16::from(h.chord_bass))) {
        (h.chord_root, h.chord_type) = (h.chord_bass, t);
        return;
    }
    for &(s, t) in &SHAPES {
        if let Some(root) = (0..12).find(|&r| pitches >> r & 1 != 0 && above(r) == mask(s)) {
            (h.chord_root, h.chord_type) = (root as u8, t);
            return;
        }
    }
}

/// A routed event as a MIDI 1.0 channel message on `channel` (1..16; 0: its own): notes, pressure,
/// and MIDI events' own bytes (graph.cpp's toMidi).
fn to_midi(e: &Event, channel: u8) -> Option<Midi> {
    if e.space() != CORE_SPACE {
        return None;
    }
    let on = |own: i32| if channel != 0 { channel - 1 } else { own.clamp(0, 15) as u8 };
    let seven = |v: f64| (v.clamp(0.0, 1.0) * 127.0).round() as u8;
    let m = |bytes: [u8; 3], size| Some(Midi { size, bytes, ..NO_MIDI });
    match e.kind() {
        NOTE_ON | NOTE_OFF | NOTE_CHOKE if e.size() == NOTE_SIZE => {
            let (_, _, ch, key) = e.address(NOTE_ID);
            let (down, v) = (e.kind() == NOTE_ON, seven(e.f64_at(NOTE_VELOCITY)));
            if !(0..=127).contains(&key) {
                return None;
            }
            m([(if down { 0x90 } else { 0x80 }) | on(ch.into()), key as u8, if down { v.max(1) } else { v }], 3)
        }
        NOTE_EXPRESSION if e.size() == NOTE_SIZE => {
            let (_, _, ch, key) = e.address(EXPR_NOTE);
            if e.i32_at(EXPR_ID) != 6 || key > 127 {
                return None; // CLAP_NOTE_EXPRESSION_PRESSURE only
            }
            let v = seven(e.f64_at(EXPR_VALUE));
            if key < 0 {
                m([0xd0 | on(ch.into()), v, 0], 2)
            } else {
                m([0xa0 | on(ch.into()), key as u8, v], 3)
            }
        }
        MIDI if e.size() == MIDI_SIZE => {
            let (_, d) = e.midi_bytes();
            let kind = d[0] & 0xf0;
            if !(0x80..0xf0).contains(&kind) || d[1] > 127 || d[2] > 127 {
                return None;
            }
            m([kind | on((d[0] & 15).into()), d[1], d[2]], if kind == 0xc0 || kind == 0xd0 { 2 } else { 3 })
        }
        _ => None,
    }
}

/// `m` into the graph's MIDI out, or counted as dropped when it is full.
fn midi_out(midi: &mut [Midi], count: &mut u32, drops: &AtomicU64, m: Midi) {
    if *count == MAX_MIDI_OUT {
        drops.fetch_add(1, Relaxed);
    } else {
        midi[*count as usize] = m;
        *count += 1;
    }
}

impl Curve {
    /// Graph::ControlCurve::shape, point and next; mul_add where the C++ says std::fma (graph.h point,
    /// graph.cpp Db; the C++ reference's minimum + x * range), so the values are the same at every optimization level.
    fn shape(&self, value: f64) -> f64 {
        let x = value.clamp(0.0, 1.0);
        match self.kind {
            1 => ((4.0 * x).exp() - 1.0) / (4f64.exp() - 1.0),
            2 => ((4f64.exp() - 1.0) * x).ln_1p() / 4.0,
            3 => x * x * (3.0 - 2.0 * x), // 2x is exact: fused or not, the same
            4 if x > 0.0 => ((self.high - self.low).mul_add(x, self.low) / 20.0 * std::f64::consts::LN_10).exp(),
            4 => 0.0,
            5 => x.powf(self.exponent),
            _ => x,
        }
    }
    fn point(&self, x: f64) -> f64 {
        (self.end - self.start).mul_add(self.shape(x), self.start)
    }
}

impl Run {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notes: NonNull<Notes>,
        audio: NonNull<Audio>,
        steps: Vec<StepIn>,
        waves: &[u32],
        parts: Vec<HarmonySource>,
        taps: Vec<Tap>,
        controls: Vec<Control>,
        contexts: impl Fn(usize) -> usize,
    ) -> Run {
        let steps: Vec<Step> = steps
            .into_iter()
            .enumerate()
            .map(|(i, s)| Step {
                host: s.host,
                id: s.id,
                harmony: s.harmony,
                budgeted: s.budgeted,
                may_rest: s.may_rest,
                routes: s.routes,
                releases: Vec::with_capacity(MAX_BLOCK_EVENTS),
                release_overflow: false,
                lanes: Vec::with_capacity(if s.lanes_in { MAX_BLOCK_EVENTS } else { 0 }),
                lanes_in: s.lanes_in,
                contexts_in: Vec::with_capacity(if contexts(i) > 0 { MAX_BLOCK_EVENTS } else { 0 }),
                merged: vec![Event::default(); MAX_PROCESS_EVENTS],
                merged_count: 0,
                heard: vec![0; if contexts(i) > 0 { MAX_PROCESS_EVENTS } else { 0 }],
                processed: AtomicBool::new(false),
                cost: F32::default(),
                started: false,
                handed: 0,
                missed: 0,
                voice_limits: [NO_LIMITS; MAX_CONTEXTS],
                voice_limit: 0,
                voice_contexts: 0,
            })
            .collect();
        let parts = parts.into_iter().map(|s| Part { s, events: Vec::with_capacity(32), keys: vec![[0; 2]; MAX_BLOCK_EVENTS] }).collect();
        let n = steps.len();
        Run {
            notes,
            audio,
            steps,
            parts,
            taps,
            controls,
            waves: waves.to_vec(),
            local: vec![0; n],
            after: vec![0.0; waves.len()],
            keys: Vec::with_capacity(MAX_PROCESS_EVENTS),
        }
    }

    /// `self` replaces `was` (Graph::beginBlock): connections leaving the plan retired into the steps'
    /// releases, plugins leaving it holding notes stranded and those coming back choked, taps gone let go.
    #[allow(clippy::too_many_arguments)]
    pub fn adopt(&mut self, was: &Run, rs: &mut Routes, notes: &Notes, old: &Notes, midi: &mut [Midi], count: &mut u32, drops: &AtomicU64) {
        for (o, ostep) in was.steps.iter().enumerate() {
            for (s, step) in self.steps.iter_mut().enumerate().filter(|(_, s)| s.host == ostep.host) {
                let staying = &notes.steps[s].sources;
                for gone in old.steps[o].sources.iter().filter(|x| !staying.iter().any(|y| y.route == x.route)) {
                    // Its pedals go to what the routes staying hold.
                    let mut remaining = [[0u8; 16]; 16];
                    for r in staying {
                        for (m, p) in remaining.as_flattened_mut().iter_mut().zip(rs[r.route].pedal.as_flattened()) {
                            *m = (*m).max(*p);
                        }
                    }
                    let releases = &mut step.releases;
                    let ok = rs[gone.route].retire(&remaining, &mut |e| {
                        releases.len() < MAX_BLOCK_EVENTS && {
                            releases.push(e.on_channel(gone.channel));
                            true
                        }
                    });
                    step.release_overflow |= !ok;
                }
            }
        }
        // A plugin the new plan no longer runs is sent no releases for the notes it holds: its voices
        // stop where they are. One an undo brings back chokes them first, in its first block.
        let stranded = |n: &Notes, s: usize, set: bool| match n.steps[s].contexts.is_empty() {
            true => n.steps[s].activity.is_some_and(|a| a.stranded.swap(set, Relaxed)),
            false => n.steps[s].contexts.iter().fold(false, |was, c| c.activity.stranded.swap(set, Relaxed) | was),
        };
        for (o, ostep) in was.steps.iter().enumerate() {
            if !self.steps.iter().any(|s| s.host == ostep.host) && old.steps[o].sources.iter().any(|x| rs[x.route].live() != 0) {
                stranded(old, o, true);
            }
        }
        for (s, step) in self.steps.iter_mut().enumerate() {
            if stranded(notes, s, false) {
                if step.releases.len() == MAX_BLOCK_EVENTS {
                    step.release_overflow = true;
                    continue;
                }
                let mut choke = Event::note(false, -1, 0.0, 0, -1, -1);
                choke.set_kind(NOTE_CHOKE);
                step.releases.push(choke);
            }
        }
        // A tap gone: note offs for the notes it holds, and its pedals released.
        for t in was.taps.iter().filter(|t| !self.taps.iter().any(|n| n.route == t.route)) {
            rs[t.route].let_go(&mut |e| {
                if let Some(m) = to_midi(e, t.channel) {
                    midi_out(midi, count, drops, Midi { ports: t.ports, ..m });
                }
                true
            });
        }
    }

    /// The block (Graph::process from the beat on); false where a plugin did not render.
    pub fn block(&mut self, rs: &mut Routes, notes: &mut Notes, audio: &mut Audio, b: &mut Block, p: &Plugins) -> bool {
        let frames = b.frames;
        audio.begin(&b.inputs, &mut b.outputs, frames as usize);
        // RFC 0033: each part's harmony at sample 0, then each change its input's keys make.
        for part in &mut self.parts {
            let s = &mut part.s;
            let mut add = |events: &mut Vec<Event>, time: u32, keys: [u64; 2]| {
                let mut x = Harmony { key_root: s.root, scale: s.scale, held: keys, ..Harmony::default() };
                recognize(&mut x);
                if !events.is_empty() && x == s.last.get() {
                    return;
                }
                s.last.set(x);
                let mut e = Event::header(40, time, HARMONY_SPACE, 0, 0); // oscairn_event_harmony_t
                e.0[16..20].copy_from_slice(&[x.key_root, x.chord_root, x.chord_type, x.chord_bass]);
                e.set_u16(20, x.scale);
                e.0[24..32].copy_from_slice(&x.held[0].to_ne_bytes());
                e.0[32..40].copy_from_slice(&x.held[1].to_ne_bytes());
                // shortcut: past 32 changes in one block the last slot takes each later one, at its time; raise the capacity if played input ever needs more.
                if events.len() == 32 {
                    events.pop();
                }
                events.push(e);
            };
            part.events.clear();
            add(&mut part.events, 0, rs[s.route].held_keys);
            if !s.input {
                continue;
            }
            route::route_events(rs, s.route, &s.zones, b.events, b.tags.unwrap_or(&[]), false, b.next_id, &mut |_| true, &mut part.keys);
            for (i, e) in b.events.iter().enumerate().filter(|(_, e)| e.space() == CORE_SPACE) {
                add(&mut part.events, e.time().min(frames - 1), part.keys[i]);
            }
        }
        let mut ok = true;
        let budgeting = b.timing.budget != 0.0;
        notes.begin(b.timing);
        // RFC 0045: no wait on a worker past the callback's deadline (realtime.h); with step 6, each wave's
        // workers wait to it less what the waves after theirs took in the last block, the costliest of each.
        let ends = b.deadline;
        let deadlines = ends != 0 && b.deadlines;
        p.deadline(ends, deadlines);
        // The reserves also bound the wait for a block a call late (RFC 0045's extra block), always. At most half a
        // period: one long block in a later wave (a preemption) must not take every earlier wave's wait away.
        if ends != 0 {
            let mut later = 0f32;
            for w in (0..self.waves.len()).rev() {
                self.after[w] = later.min(b.timing.period / 2.0);
                let first = if w > 0 { self.waves[w - 1] } else { 0 };
                later += (first..self.waves[w]).fold(0f32, |m, i| m.max(self.steps[i as usize].cost.get()));
            }
        }
        let less = |after: f32| ends - after as i64 * 1000;
        let micros = |ns: i64| ns as f32 / 1000.0; // duration<float, std::micro> of steady_clock's nanoseconds
        let waves = std::mem::take(&mut self.waves);
        if !b.parallel {
            for s in 0..self.steps.len() {
                self.prepare(s, rs, notes, audio, b, p);
                self.limit_voices(s, rs, notes, p);
                if !Self::rest(&self.steps[s], s, audio, b, p) {
                    let m = self.steps[s].merged();
                    let processed = p.render(s, m.as_ptr(), m.len() as u32);
                    self.steps[s].processed.store(processed, Relaxed);
                }
                ok &= self.finish(s, rs, notes, audio, b, p, budgeting);
            }
        } else {
            let mut first = 0;
            for (wave, &end) in waves.iter().enumerate() {
                // A wave: each plugin in a worker starts as soon as its inputs are ready; the others
                // render meanwhile, here and on the audio helpers; then every one finishes, in plan order.
                let (first_, end_) = (first as usize, end as usize);
                if deadlines {
                    p.deadline(less(self.after[wave]), true);
                }
                let mut locals = 0;
                for s in first_..end_ {
                    self.prepare(s, rs, notes, audio, b, p);
                    self.limit_voices(s, rs, notes, p);
                    if Self::rest(&self.steps[s], s, audio, b, p) {
                        self.steps[s].started = false;
                        continue;
                    }
                    self.steps[s].handed = p.now();
                    let m = self.steps[s].merged();
                    self.steps[s].started = p.start(s, m.as_ptr(), m.len() as u32);
                    if !self.steps[s].started {
                        self.local[locals] = s as u32;
                        locals += 1;
                    }
                }
                // The helpers only when, by the last block, the wave leaves enough besides its largest
                // plugin to pay for waking them; otherwise all here.
                let (total, largest) =
                    self.local[..locals].iter().map(|&s| self.steps[s as usize].cost.get()).fold((0f32, 0f32), |(t, l), c| (t + c, l.max(c)));
                if total - largest >= b.worth {
                    p.helpers(self, locals);
                } else {
                    (0..locals).for_each(|k| self.render_local(p, k));
                }
                // RFC 0045: plugins a block late last, so waiting for their blocks in flight overlaps the wave's others, and
                // only to the callback's deadline less what the waves after this one took in the last block.
                for late in [false, true] {
                    for s in first_..end_ {
                        if !self.steps[s].started || p.late(s) != late {
                            continue;
                        }
                        if late && ends != 0 {
                            p.deadline(less(self.after[wave]), deadlines);
                        }
                        let processed = p.finish(s);
                        let step = &self.steps[s];
                        step.processed.store(processed, Relaxed);
                        // A block given up on took the time it was allowed: the last kept. One a call late costs the waves before it nothing.
                        if processed {
                            step.cost.set(if late { 0.0 } else { micros(p.now() - step.handed) });
                        }
                    }
                }
                if ends != 0 {
                    p.deadline(if deadlines { less(self.after[wave]) } else { ends }, deadlines);
                }
                for s in first_..end_ {
                    ok &= self.finish(s, rs, notes, audio, b, p, budgeting);
                }
                first = end;
            }
        }
        self.waves = waves;
        p.deadline(0, false);
        // MIDI for external outputs (setMidiTaps), from the input or a node's note outputs.
        for t in &self.taps {
            let (list, tags) = if t.step < 0 { (b.events, b.tags.unwrap_or(&[])) } else { (p.sent(t.step as usize), &[][..]) };
            let (midi, count, drops) = (&mut *b.midi, &mut *b.midi_count, b.drops);
            route::route_events(
                rs,
                t.route,
                &t.zones,
                list,
                tags,
                true,
                b.next_id,
                &mut |e| {
                    if let Some(m) = to_midi(e, t.channel) {
                        midi_out(midi, count, drops, Midi { time: e.time().min(frames - 1), ports: t.ports, ..m });
                    }
                    true
                },
                &mut [],
            );
        }
        audio.mix(self.steps.len(), &mut b.outputs, frames as usize, b.live_from, b.live_to);
        // Each step's gain to the outputs, by which the budget hears a part's voices in the mix (RFC 0046).
        if notes.budgeting() {
            audio.reach(b.live_to, &mut notes.budget.gains);
        }
        // The worker stamps only where a lane's time is short (time_lanes); the lanes judged, the callback's time left once the
        // block is collected (RFC 0045's guard), the timing report kept.
        notes.time_lanes(|s| p.waited(s));
        notes.end(rs, if ends != 0 { micros(ends - p.now()) } else { f32::NAN });
        ok
    }

    /// RFC 0045, sleeping plugins: a block its plugin would sleep through, not called (no core events for it, its
    /// input quiet, the transport as it went, its host saying nothing else is pending): silence, rendered.
    fn rest(step: &Step, s: usize, audio: &mut Audio, b: &Block, p: &Plugins) -> bool {
        if !step.may_rest {
            return false;
        }
        let frames = b.frames as usize;
        if b.transport_wake || step.merged().iter().any(|e| e.space() == CORE_SPACE) || !audio.quiet(s, frames) || !p.rest(s, b.frames) {
            return false;
        }
        audio.silence(s, frames);
        step.processed.store(true, Relaxed);
        true
    }

    /// A step rendered here or on a helper (`k`: its place among the wave's local steps).
    pub fn render_local(&self, p: &Plugins, k: usize) {
        let s = self.local[k] as usize;
        let step = &self.steps[s];
        let began = p.now();
        step.processed.store(p.render(s, step.merged().as_ptr(), step.merged_count as u32), Relaxed);
        step.cost.set(micros_since(p, began));
    }

    /// A step's inputs: its audio mixed, every source's events this block merged in time order and routed.
    fn prepare(&mut self, s: usize, rs: &mut Routes, notes: &mut Notes, audio: &mut Audio, b: &mut Block, p: &Plugins) {
        let sources = &notes.steps[s].sources;
        let input_recovery = (b.panic_notes && !sources.is_empty()) || (b.panic_input && sources.iter().any(|x| x.from < 0));
        audio.mix(s, &mut Channels::none(), b.frames as usize, b.live_from, b.live_to);
        let Run { steps, parts, keys, .. } = self;
        let step = &mut steps[s];
        keys.clear();
        let capacity = step.merged.len();
        let key = |keys: &mut Vec<Key>, e: &Event, time: u32, source: i16, tag: u8, after_notes: bool| {
            let order = keys.len() as u32;
            keys.push(Key { time, order, event: Shared(NonNull::from(e)), source, tag, after_notes });
        };
        key(keys, b.beat, 0, -1, UNTAGGED, false); // oscairn.beat, first at sample 0
        if step.harmony >= 0 {
            for e in &parts[step.harmony as usize].events {
                key(keys, e, e.time(), -1, UNTAGGED, false); // the part's harmony, each before the notes at its sample
            }
        }
        let mut total = (step.lanes.len() + step.releases.len()) as u64;
        let mut overflow = step.release_overflow || input_recovery;
        // The beat and the part's harmony ride with the notes in their own room (maxProcessEvents).
        for e in &step.releases {
            key(keys, e, 0, -1, UNTAGGED, false);
        }
        let contexts = &notes.steps[s].contexts;
        if contexts.is_empty() {
            for q in b.params.iter().filter(|q| q.node == step.id) {
                total += 1;
                if keys.len() < capacity {
                    key(keys, &q.event, 0, -1, UNTAGGED, false);
                } else {
                    overflow = true;
                }
            }
        }
        // A shared engine's contexts: each one's parameters, queued or set, on its own channel.
        step.contexts_in.clear();
        for (c, context) in contexts.iter().enumerate() {
            let mut add = |e: &Event| {
                total += 1;
                if step.contexts_in.len() < MAX_BLOCK_EVENTS && keys.len() < capacity {
                    step.contexts_in.push(e.on_channel(context.channel));
                    key(keys, step.contexts_in.last().expect("pushed"), 0, -1, UNTAGGED, false);
                } else {
                    overflow = true;
                }
                true
            };
            for q in b.params.iter().filter(|q| q.node == context.id) {
                add(&q.event);
            }
            p.parameters(s, c, &mut add);
        }
        for (e, notes_of) in &step.lanes {
            if keys.len() < capacity {
                key(keys, e, e.time(), *notes_of, UNTAGGED, *notes_of >= 0);
            } else {
                overflow = true;
            }
        }
        for (c, source) in sources.iter().enumerate() {
            let list = if source.from < 0 { b.events } else { p.sent(source.from as usize) };
            for (i, e) in list.iter().enumerate().filter(|(_, e)| e.space() == CORE_SPACE && e.kind() != NOTE_END) {
                total += 1; // note ends belong to their destination
                if keys.len() < capacity {
                    key(keys, e, e.time(), c as i16, if source.from < 0 { b.tags.map_or(UNTAGGED, |t| t[i]) } else { UNTAGGED }, false);
                } else {
                    overflow = true;
                }
            }
        }
        // Routed through its connections, with stolen notes' fade steps among them; a lost release must reset the
        // destination in its own dialect (the core empties its ledgers).
        let heard = if step.heard.is_empty() { None } else { Some(&mut step.heard[..]) };
        let (n, lost) = notes.route(rs, s, keys, b.frames, b.next_id, &mut step.merged, heard, overflow);
        step.merged_count = n;
        if lost {
            b.drops.fetch_add(total, Relaxed);
            p.panic(s);
        }
    }

    /// oscairn.voice-budget (RFC 0018 step 4): a plugin that gives up its own voices gets the voices the
    /// budget leaves it, with its part's reserve and priority for the contexts it plays, when any changed.
    fn limit_voices(&mut self, s: usize, rs: &Routes, notes: &Notes, p: &Plugins) {
        let (step, n) = (&mut self.steps[s], &notes.steps[s]);
        let Some(activity) = n.activity.filter(|a| step.budgeted && a.own_voices.load(Relaxed)) else { return };
        let (voices, _, _) = p.voices(s);
        // One entry for the selected contexts, or each of a shared engine's contexts' own, under the sound its channel selects.
        let mut parts = [NO_LIMITS; MAX_CONTEXTS];
        let part = |channel: i16, context: u32| {
            let zone = n.sources.iter().filter(|x| x.channel == channel && !x.draining).find_map(|x| x.zones.first());
            VoiceContext { context, reserve: zone.map_or(0, |z| u32::from(z.reserve)), priority: zone.map_or(0, |z| z.priority), ..NO_LIMITS }
        };
        let entries = if n.contexts.is_empty() {
            parts[0] = part(-1, SELECTED);
            1
        } else {
            for (c, context) in n.contexts.iter().enumerate().take(MAX_CONTEXTS) {
                parts[c] = part(context.channel, p.sound(s, c as u32));
            }
            n.contexts.len().min(MAX_CONTEXTS)
        };
        // Each sound context's own limit too: one sound's tails not given up for another's notes while the instance keeps within its own.
        let mut own = [0u32; MAX_CONTEXTS];
        let shared = if n.contexts.is_empty() { 0 } else { entries };
        let limit = notes.voice_limit(rs, s, voices, &mut own[..shared]);
        for (q, o) in parts.iter_mut().zip(&own).take(shared) {
            q.limit = *o;
        }
        if limit == step.voice_limit && entries == step.voice_contexts && parts[..entries] == step.voice_limits[..entries] {
            return;
        }
        (step.voice_limit, step.voice_limits, step.voice_contexts) = (limit, parts, entries);
        activity.voice_limit.store(limit, Relaxed);
        p.set_limits(s, limit, parts.as_ptr(), entries as u32);
    }

    /// After a step's plugin rendered: the listener, its audio's guard and meters, its notes' bookkeeping, its lanes passed on.
    #[allow(clippy::too_many_arguments)]
    fn finish(&mut self, s: usize, rs: &mut Routes, notes: &mut Notes, audio: &mut Audio, b: &Block, p: &Plugins, budgeting: bool) -> bool {
        let contexts = &notes.steps[s].contexts;
        let step = &self.steps[s];
        if b.listening && contexts.is_empty() {
            p.listen(step.id, step.merged(), false);
        } else if b.listening {
            // A shared engine's: each node what came on its channel, as it came.
            for c in contexts {
                for (i, e) in step.merged().iter().enumerate().filter(|(_, e)| e.channel() < 0 || e.channel() == c.channel) {
                    let heard = step.heard[i];
                    p.listen(c.id, std::slice::from_ref(&if heard < 0 { *e } else { e.on_channel(heard) }), false);
                }
            }
        }
        let processed = step.processed.load(Relaxed);
        // The output guard, its meters, and its and its contexts' quiet, asleep and age.
        audio.finish(s, b.frames as usize, processed, processed && p.asleep(s), b.fade);
        // A worker's late or busy block lost nothing (kept), so its notes go on (RFC 0045); a host that reset
        // the plugin on its own ended every note it was sent, with no NOTE_END.
        let lost = !processed && !p.kept(s);
        if lost {
            p.panic(s);
        }
        let reset = lost || p.was_reset(s);
        if b.listening && processed {
            p.listen(step.id, p.sent(s), true);
        }
        let took = if budgeting { p.took(s) as f32 } else { 0.0 };
        let missed = p.missed(s);
        let mut ids = [0u32; MAX_CONTEXTS]; // its contexts' sound IDs, which its importance report names
        let step = &mut self.steps[s];
        let mut r = Rendered {
            took,
            processed,
            reset,
            missed: missed > step.missed,
            own_voices: false,
            honors_volume: false,
            voices: 0,
            voice_cost: 0.0,
            notes: &[],
            sent: &[],
            importance: &[],
            contexts: &[],
            cpu: 0.0,
            wait: 0.0,
        };
        step.missed = missed;
        if budgeting && step.budgeted && processed {
            (r.voices, r.voice_cost, r.notes) = p.voices(s);
            r.importance = p.importance(s);
            if !r.importance.is_empty() {
                let n = notes.contexts(s).min(MAX_CONTEXTS);
                for (c, id) in ids.iter_mut().enumerate().take(n) {
                    *id = p.sound(s, c as u32);
                }
                r.contexts = &ids[..n];
            }
            (r.own_voices, r.honors_volume) = (p.own_voices(s), p.honors_volume(s));
        }
        r.sent = p.sent(s);
        notes.finish(rs, s, &r);
        step.lanes.clear();
        step.releases.clear();
        step.release_overflow = false;
        // This step's param-target lane events become parameter events for their targets, later in this block.
        if !self.steps[s].routes.is_empty() {
            for e in p.sent(s).iter().filter(|e| e.space() == PARAM_TARGET_SPACE && e.kind() == 0) {
                self.lane(s, e, b.drops);
            }
        }
        processed
    }

    /// A lane event `e` from step `s` to each parameter its lane reaches, directly or through a control.
    fn lane(&mut self, s: usize, e: &Event, drops: &AtomicU64) {
        let (lane, set, note_id, value) = (e.u32_at(16), e.u32_at(20) == 0, e.i32_at(24), e.f64_at(40));
        let (port, channel, key) = (e.i16_at(28), e.i16_at(30), e.i16_at(32));
        for k in 0..self.steps[s].routes.len() {
            let r = self.steps[s].routes[k];
            if r.lane != lane {
                continue;
            }
            // Per-note lane addresses belong to the source's corresponding note route.
            if note_id >= 0 && r.notes < 0 {
                drops.fetch_add(1, Relaxed);
                continue;
            }
            let notes_of = if note_id >= 0 { r.notes } else { -1 };
            let range = r.maximum - r.minimum;
            // Through a control: the path from the control's value to the parameter's fraction.
            let control = usize::try_from(r.control).ok().map(|c| &self.controls[c]);
            let follow = |c: &Control, mut x: f64| {
                for (k, curve) in c.path.iter().enumerate() {
                    x = if c.modulate && k + 1 == c.path.len() { curve.shift + curve.point(x) } else { (curve.shift + curve.point(x)).clamp(0.0, 1.0) };
                }
                x
            };
            // The control's value from the lane: a SET event's, or a MODULATE event's amount from where it is.
            let x = control.map_or(0.0, |c| (if set { value } else { c.origin + value.clamp(-1.0, 1.0) }).clamp(0.0, 1.0));
            let absolute = control.map_or(set, |c| !c.modulate);
            let mut out = Event::header(PARAM_SIZE, e.time(), CORE_SPACE, if absolute { PARAM_VALUE } else { PARAM_MOD }, 0);
            out.set_u32(16, r.param);
            out.set_address(PARAM_NOTE, note_id, port, if r.channel >= 0 { r.channel } else { channel }, key);
            out.set_f64(
                48,
                match (absolute, control) {
                    (true, c) => {
                        let v = c.map_or(value.clamp(0.0, 1.0), |c| follow(c, x)).mul_add(range, r.minimum);
                        if r.stepped {
                            v.round()
                        } else {
                            v
                        }
                    }
                    (false, c) => c.map_or(value.clamp(-1.0, 1.0), |c| follow(c, x)) * range,
                },
            );
            // A target's block full of lane points: the newest replaces the last one waiting for the same
            // parameter, so its stream thins but ends at its latest value; each point lost is counted.
            let target = &mut self.steps[r.target as usize];
            if target.lanes.len() < if target.lanes_in { MAX_BLOCK_EVENTS } else { 0 } {
                target.lanes.push((out, notes_of));
                continue;
            }
            let same = |a: &Event| a.kind() == out.kind() && a.u32_at(16) == out.u32_at(16) && a.address(PARAM_NOTE) == out.address(PARAM_NOTE);
            if let Some(slot) = target.lanes.iter_mut().rev().find(|(a, _)| same(a)) {
                *slot = (out, notes_of);
            }
            drops.fetch_add(1, Relaxed);
        }
    }
}

fn micros_since(p: &Plugins, began: i64) -> f32 {
    (p.now() - began) as f32 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Activity;
    use crate::tests::{ALLOCATIONS, COUNTING};
    use crate::*;
    use std::cell::Cell;
    use std::ffi::c_void;

    struct World {
        sent: Vec<Event>,
        clock: Cell<i64>,
    }
    fn world<'a>(c: *mut c_void) -> &'a World {
        // SAFETY: the test's World, alive for every call it is passed to.
        unsafe { &*c.cast::<World>() }
    }
    extern "C" fn sent(c: *mut c_void, _: u32, n: *mut u32) -> *const Event {
        let w = world(c);
        // SAFETY: the run's count to write.
        unsafe { *n = w.sent.len() as u32 };
        w.sent.as_ptr()
    }
    extern "C" fn yes(_: *mut c_void, s: u32) -> bool {
        s.is_multiple_of(2)
    }
    extern "C" fn play(_: *mut c_void, _: u32, _: *const Event, _: u32) -> bool {
        true
    }
    extern "C" fn nothing(_: *mut c_void, _: u32) {}
    extern "C" fn zero(_: *mut c_void, _: u32) -> u64 {
        0
    }
    extern "C" fn took(_: *mut c_void, _: u32) -> f64 {
        30.0
    }
    extern "C" fn voices(_: *mut c_void, _: u32, cost: *mut f64, _: *mut *const VoiceNote, n: *mut u32) -> u32 {
        // SAFETY: the run's places to write.
        unsafe { (*cost, *n) = (10.0, 0) };
        3
    }
    extern "C" fn importance(_: *mut c_void, _: u32, _: *mut *const crate::bindings::oscairn_voice_budget_importance_t) -> u32 {
        0
    }
    extern "C" fn limits(_: *mut c_void, _: u32, _: u32, _: *const VoiceContext, _: u32) {}
    extern "C" fn sound(_: *mut c_void, _: u32, c: u32) -> u32 {
        c
    }
    extern "C" fn parameters(_: *mut c_void, _: u32, _: u32, emit: Emit, e: *mut c_void) {
        let p = Event::header(PARAM_SIZE, 0, CORE_SPACE, PARAM_VALUE, 0);
        // SAFETY: the run's callback and its context, for this call.
        unsafe { emit.expect("emit")(e, &p) };
    }
    extern "C" fn listen(_: *mut c_void, _: u32, _: *const Event, _: u32, _: bool) {}
    extern "C" fn now(c: *mut c_void) -> i64 {
        let w = world(c);
        w.clock.set(w.clock.get() + 20_000);
        w.clock.get()
    }
    extern "C" fn deadline(_: *mut c_void, _: i64, _: bool) {}
    extern "C" fn rest(_: *mut c_void, s: u32, _: u32) -> bool {
        s == 1
    }
    extern "C" fn helpers(_: *mut c_void, n: u32, job: oscairn_job, context: *mut c_void) {
        // SAFETY: the run's job and its context, for this call.
        (0..n).for_each(|i| unsafe { job.expect("job")(context, i) });
    }

    // A block allocates nothing: two waves (a worker's step and one rendered here, on the helpers), events
    // merged from the input and a step's notes, a shared engine's contexts, a harmony, a lane through a
    // control, a MIDI tap, the listener; then a plan replaced with its gains carried.
    #[test]
    fn run_blocks_allocate_nothing() {
        let mut note = Event::note(true, 60, 0.8, 4, 0, 0);
        note.set_i32(NOTE_ID, 7);
        let mut lane = Event::header(48, 10, PARAM_TARGET_SPACE, 0, 0);
        lane.set_f64(40, 0.4);
        let w = World { sent: vec![note, Event::note(false, 60, 0.0, 90, 0, 0), lane], clock: Cell::new(1_000_000) };
        let activity = [Activity::default(), Activity::default(), Activity::default()];
        let zones = Zones::default();
        let routes = [rust_core_route_new(1, 1, 64), rust_core_route_new(1, 0, 0), rust_core_route_new(16, 0, 0), rust_core_route_new(16, 0, 0)];
        let a = |i: usize| std::ptr::from_ref(&activity[i]).cast_mut();
        let sources = [
            oscairn_source { route: routes[0], zones: &zones, peers: std::ptr::null(), peer_count: 0, channel: -1, draining: false, from: -1 },
            oscairn_source { route: routes[1], zones: &zones, peers: std::ptr::null(), peer_count: 0, channel: 1, draining: false, from: 0 },
        ];
        let contexts = [oscairn_context { channel: 0, activity: a(1), id: 10 }, oscairn_context { channel: 1, activity: a(2), id: 11 }];
        let curve = Curve { kind: 3, start: 0.1, end: 0.9, shift: 0.0, low: -60.0, high: 0.0, exponent: 2.0 };
        let control = oscairn_control { path: &curve, count: 1, origin: 0.5, modulate: false };
        let lane_route = LaneRoute { lane: 0, target: 1, param: 3, minimum: 0.0, maximum: 10.0, stepped: false, notes: 0, channel: -1, control: 0 };
        let step = |s: usize| oscairn_step {
            activity: a(0),
            sources: &sources[s],
            source_count: 1,
            contexts: if s == 1 { contexts.as_ptr() } else { std::ptr::null() },
            context_count: if s == 1 { 2 } else { 0 },
            lane: s as u32,
            in_process: s == 1,
            late_lane: false,
            next: std::ptr::null(),
            next_count: 0,
            host: std::ptr::null::<u8>().wrapping_add(s + 1).cast(),
            id: s as u32,
            harmony: if s == 0 { 0 } else { -1 },
            lane_routes: if s == 0 { &lane_route } else { std::ptr::null() },
            lane_route_count: u32::from(s == 0),
            lanes_in: s == 1,
            may_rest: true,
        };
        let steps = [step(0), step(1)];
        let mut last = Harmony::default();
        let harmony = oscairn_harmony_source { route: routes[2], zones: &zones, input: true, root: 0, scale: 0xAB5, last: &mut last };
        let tap = oscairn_tap { step: 0, route: routes[3], zones: &zones, ports: 1, channel: 0 };
        let outs = [oscairn_audio_route { source: 1, from: 0, to: 0, gain: 0.5, live: [1, 0, 0, 0] }];
        let audio_steps = [0usize, 1].map(|s| oscairn_audio_step {
            host: std::ptr::null::<u8>().wrapping_add(s + 1).cast(),
            inputs: 0,
            outputs: 2,
            metered: 2,
            peaks: std::ptr::null_mut(),
            activity: std::ptr::null_mut(),
            routes: std::ptr::null(),
            route_count: 0,
            buses: std::ptr::null(),
            bus_count: 0,
        });
        let plugins = oscairn_plugins {
            context: std::ptr::from_ref(&w).cast_mut().cast(),
            sent: Some(sent),
            start: Some(play),
            render: Some(play),
            finish: Some(yes),
            late: Some(yes),
            panic: Some(nothing),
            kept: Some(yes),
            was_reset: Some(yes),
            asleep: Some(yes),
            own_voices: Some(yes),
            honors_volume: Some(yes),
            missed: Some(zero),
            took: Some(took),
            voices: Some(voices),
            importance: Some(importance),
            set_limits: Some(limits),
            sound: Some(sound),
            parameters: Some(parameters),
            listen: Some(listen),
            now: Some(now),
            deadline: Some(deadline),
            helpers: Some(helpers),
            rest: Some(rest),
            waited: None,
        };
        let (beat, live, mut out) = (Event::header(40, 0, 1001, 0, 0), [1f32; 256], [0f32; 256]);
        let (mut next_id, mut drops, mut midi) = (0u32, 0u64, [NO_MIDI; 1024]);
        let mut midi_count: u32;
        let input = [Event::note(true, 64, 0.5, 3, 0, 0), Event::midi(0xb0, 64, 127, 0), Event::note(false, 64, 0.0, 60, 0, 0)];
        let make = |budgeted: &mut [bool; 2]| {
            // SAFETY: arrays that outlive the run; its notes and audio freed after it.
            unsafe {
                let notes = rust_core_notes_new(steps.as_ptr(), 2, 2, budgeted.as_mut_ptr());
                let audio = rust_core_audio_new(audio_steps.as_ptr(), 2, outs.as_ptr(), 1, 0, 2, 128);
                let run = rust_core_run_new(notes, audio, steps.as_ptr(), budgeted.as_ptr(), 2, [1u32, 2].as_ptr(), 2, &harmony, 1, &tap, 1, &control, 1);
                (notes, audio, run)
            }
        };
        let (mut b1, mut b2) = ([false; 2], [false; 2]);
        let ((n1, a1, r1), (n2, a2, r2)) = (make(&mut b1), make(&mut b2));
        let o = out.as_mut_ptr();
        let outputs = [o, o.wrapping_add(128)];
        let mut taps = 0;
        COUNTING.with(|c| c.set(true));
        for b in 0..300u32 {
            midi_count = 0;
            let block = oscairn_run_block {
                inputs: std::ptr::null(),
                ni: 0,
                outputs: outputs.as_ptr(),
                no: 2,
                frames: 128,
                fade: 48,
                events: input.as_ptr(),
                count: if b % 3 == 0 { 3 } else { 0 },
                tags: std::ptr::null(),
                params: std::ptr::null(),
                param_count: 0,
                beat: &beat,
                panic_input: b % 50 == 7,
                panic_notes: false,
                listening: true,
                parallel: b % 2 == 0,
                deadlines: true,
                transport_wake: b % 10 == 0,
                timing: Timing { budget: 0.0, period: 2666.0, deadline: 2266.0, allowance: 1333.0, cores: 1.0, quiet_after: 256, low_slack: 0.0 },
                deadline: 50_000_000,
                worth: 0.0,
                live_from: live.as_ptr(),
                live_to: live.as_ptr(),
                next_id: &mut next_id,
                drops: &mut drops,
                midi: midi.as_mut_ptr(),
                midi_count: &mut midi_count,
            };
            // SAFETY: a live run and the arrays above, for the call.
            unsafe {
                if b == 150 {
                    rust_core_run_adopt(r2, r1, true, 240, midi.as_mut_ptr(), &mut midi_count, &mut drops);
                }
                rust_core_run_block(if b < 150 { r1 } else { r2 }, &block, &plugins);
            }
            taps += midi_count;
        }
        COUNTING.with(|c| c.set(false));
        assert!(taps > 0 && next_id > 0, "the tap and the notes ran");
        assert_eq!(ALLOCATIONS.load(Relaxed), 0);
        // SAFETY: made above, freed once, each run before its notes and audio.
        unsafe {
            for (n, a, r) in [(n1, a1, r1), (n2, a2, r2)] {
                rust_core_run_free(r);
                rust_core_audio_free(a);
                rust_core_notes_free(n);
            }
            for r in routes {
                rust_core_route_free(r);
            }
        }
    }
}
