//! VoiceBudget (core/engine/src/NoteRoute.h; RFC 0018 option B, per lane as RFC 0045) and the node
//! costs it reads (Graph::Activity). Counts the C++ scans for are popcounts of the routes' maps.

use crate::route::FADE_STEPS;
use crate::{Route, Routes, Shared};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

/// A float another thread may read: an f32's bits in an AtomicU32 (relaxed, as the C++ atomics).
#[derive(Default)]
#[repr(transparent)]
pub struct F32(AtomicU32);
impl F32 {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed)
    }
}

/// Graph::Activity (graph.h), field for field (oscairn_activity): every field an atomic, as other
/// threads read them and the C++ writes some on this thread too.
#[derive(Default)]
#[repr(C)]
pub struct Activity {
    pub sounding: AtomicU32,
    pub quiet: AtomicU64,
    pub age: AtomicU64,
    pub asleep: AtomicBool,
    pub rest: F32,
    pub per_note: F32,
    pub seed_rest: F32,
    pub seed_note: F32,
    pub stolen: AtomicU64,
    pub hard_chokes: AtomicU64,
    pub stolen_for_device: AtomicU64,
    pub lane_limit: F32,
    pub lane_took: F32,
    pub bad_output: AtomicU64,
    pub bad_run: AtomicU32,
    pub stranded: AtomicBool,
    pub own_voices: AtomicBool,
    pub honors_volume: AtomicBool,
    pub per_note_scale: F32,
    pub voice_limit: AtomicU32,
    pub over_limit: AtomicU32,
    pub faded_for_loss: AtomicU64,
    pub lost: AtomicU64,
    pub faded_for_deadline: AtomicU64,
}

/// The blocks a plugin that gives up its own voices may sound more than its limit before its notes
/// are given up as any plugin's (Activity::overLimitBlocks).
const OVER_LIMIT_BLOCKS: u32 = 16;

impl Activity {
    pub fn rest_now(&self) -> f32 {
        let r = self.rest.get();
        if r >= 0.0 {
            r
        } else {
            0f32.max(self.seed_rest.get())
        }
    }
    pub fn note_now(&self) -> f32 {
        let n = self.per_note.get();
        self.per_note_scale.get() * if n >= 0.0 { n } else { 0f32.max(self.seed_note.get()) }
    }
    /// A block took `micros` with `notes` live: with none, its cost at rest; with some, the time above
    /// rest shared among them. A sample counts at most four times the estimate, and an estimate falls
    /// faster than it rises, so a block held up from outside soon passes.
    pub fn observe(&self, micros: f32, notes: u32) {
        let follow = |value: &F32, mut sample: f32, rise: f32, fall: f32| {
            let v = value.get();
            if v > 0.0 {
                sample = sample.min(4.0 * v);
            }
            value.set(if v < 0.0 { sample } else { v + (if sample > v { rise } else { fall }) * (sample - v) });
        };
        if notes == 0 {
            follow(&self.rest, micros, 0.05, 0.5);
        } else {
            follow(&self.per_note, 0f32.max(micros - self.rest_now()) / notes as f32, 0.1, 0.3);
        }
    }
}

/// A note connection into a plugin taking CLAP notes: its route, its node's cost, its part's group
/// (the first entry of its part, else itself), whether it counts its node at rest, its lane, and its
/// part's reserve and priority.
pub struct Entry {
    pub route: Route,
    pub cost: Shared<Activity>,
    pub group: usize,
    pub rest: bool,
    pub draining: bool,
    pub lane: usize,
    pub part: u32,
    pub reserve: u16,
    pub priority: u8,
}

#[derive(Clone, Copy)]
struct Group {
    held: u32,
    low: i16,
    high: i16,
    cost: f32, // of its held notes
}
const NO_GROUP: Group = Group { held: 0, low: i16::MAX, high: -1, cost: 0.0 };

/// A lane's limit for this block in microseconds (0 for none); miss, the time its blocks have before
/// they are missed; others, the last block's cost of its plugins taking no notes; over, the last block
/// measured past the limit; took and others_took, this block's so far; losses, its last eight blocks,
/// a bit each set where a block on it or after it was lost, the latest lowest, and weight, the largest
/// gain on the way from it to such a block in them.
#[derive(Clone, Copy, Default)]
pub struct Lane {
    pub limit: f32,
    pub miss: f32,
    pub others: f32,
    pub over: bool,
    pub took: f32,
    pub others_took: f32,
    pub losses: u8,
    pub weight: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Counting {
    Kept,   // not given up
    Fading, // and those fading out
    Ending, // and those choked, until they end
}

type Victim = Option<(usize, usize)>; // entry, slot

pub struct Budget {
    pub entries: Vec<Entry>,
    groups: Vec<Group>, // scratch, one per entry
    pub lanes: Vec<Lane>,
    device_over: bool,
    device_hold: u32,
    loss_hold: u32,
}

const SOFT: f32 = 0.85; // stealing starts here, so fades have time
/// After notes fade for the total, the blocks it waits before judging it again.
const DEVICE_HOLD_BLOCKS: u32 = 8;
/// After a note fades for lost blocks, the blocks before the next: its fade (5 ms) and a block to see it.
const LOSS_HOLD_BLOCKS: u32 = 3;

impl Budget {
    pub fn new(entries: Vec<Entry>, lanes: usize) -> Budget {
        Budget { groups: vec![NO_GROUP; entries.len()], entries, lanes: vec![Lane::default(); lanes.max(1)], device_over: false, device_hold: 0, loss_hold: 0 }
    }
    fn lane(&self, lane: usize) -> Lane {
        self.lanes.get(lane).copied().unwrap_or_default()
    }

    /// What the lane's plugins are estimated to cost now: at rest, and their notes at each node's cost per note.
    fn estimate(&self, rs: &Routes, lane: usize, counting: Counting) -> f32 {
        let mut total = self.lane(lane).others;
        for e in self.entries.iter().filter(|e| e.lane == lane) {
            let r = &rs[e.route];
            if e.rest {
                total += e.cost.rest_now();
            }
            let n = match counting {
                Counting::Ending => r.used.count(),
                Counting::Kept => r.used.and_not(r.stolen).count(),
                Counting::Fading => r.used.and_not(r.stolen).count() + r.stolen.and(r.fading).iter().filter(|&i| r.slot(i).faded < FADE_STEPS).count() as u32,
            };
            total += n as f32 * e.cost.note_now();
        }
        total
    }
    /// Each group's held notes (every lane's), or one lane's with their cost.
    fn count_held(&mut self, rs: &Routes, lane: Option<usize>) {
        self.groups.fill(NO_GROUP);
        for e in self.entries.iter().filter(|e| lane.is_none_or(|l| l == e.lane)) {
            let r = &rs[e.route];
            let Some(g) = self.groups.get_mut(e.group) else { continue };
            for i in r.used.and(r.held).and_not(r.stolen).iter() {
                g.held += 1;
                g.low = g.low.min(r.slot(i).key);
                g.high = g.high.max(r.slot(i).key);
                if lane.is_some() {
                    g.cost += e.cost.note_now();
                }
            }
        }
    }
    /// The next note to give up, in RFC 0018's order: released notes, then held notes beyond their
    /// part's reserve, its highest and lowest held keys last; within each, the part of lower priority
    /// first, a plugin's that honors the volume expression before one's that does not, then the
    /// quietest, then the oldest. reserved: held notes within a reserve may go too. A plugin that gives
    /// up its own voices is left to the voice limit it was given while it keeps within it. Notes of nodes
    /// whose notes cost: a MIDI-dialect plugin's too, ended by a note-off.
    /// lane: every lane's notes for None. costed: only nodes whose notes are measured to cost (giving one up
    /// frees time), else any (time is running out, whatever the measurements say).
    fn choose(&mut self, rs: &Routes, lane: Option<usize>, reserved: bool, costed: bool) -> Victim {
        self.count_held(rs, None);
        let mut best: Victim = None;
        let (mut best_rank, mut best_priority, mut best_honors, mut best_velocity, mut best_id) = (0, 0u8, false, 0f32, 0i32);
        for (k, e) in self.entries.iter().enumerate() {
            let a = &e.cost;
            let within = a.voice_limit.load(Relaxed) != 0 && a.over_limit.load(Relaxed) <= OVER_LIMIT_BLOCKS;
            if lane.is_some_and(|l| l != e.lane) || (costed && a.note_now() <= 0.0) || (a.own_voices.load(Relaxed) && within) {
                continue;
            }
            let honors = a.honors_volume.load(Relaxed);
            let (r, g) = (&rs[e.route], self.groups.get(e.group).copied().unwrap_or(NO_GROUP));
            for i in r.used.and_not(r.stolen).iter() {
                let s = r.slot(i);
                let rank = if r.held.has(i) {
                    if !reserved && g.held <= u32::from(e.reserve) {
                        continue;
                    }
                    if g.held > 2 && (s.key == g.low || s.key == g.high) {
                        2
                    } else {
                        1
                    }
                } else {
                    0
                };
                let better = best.is_none()
                    || if rank != best_rank {
                        rank < best_rank
                    } else if e.priority != best_priority {
                        e.priority < best_priority
                    } else if rank != 0 && honors != best_honors {
                        honors
                    } else if s.velocity != best_velocity {
                        s.velocity < best_velocity
                    } else {
                        s.id < best_id
                    };
                if better {
                    best = Some((k, i));
                    (best_rank, best_priority, best_honors, best_velocity, best_id) = (rank, e.priority, honors, s.velocity, s.id);
                }
            }
        }
        best
    }
    /// The plugins at rest and each part's reserved held notes alone.
    fn reserves(&mut self, rs: &Routes, lane: usize) -> f32 {
        self.count_held(rs, Some(lane));
        let mut total = self.lane(lane).others;
        for e in self.entries.iter().filter(|e| e.lane == lane && e.rest) {
            total += e.cost.rest_now();
        }
        for (g, e) in self.groups.iter().zip(&self.entries).filter(|(g, _)| g.held != 0) {
            total += g.cost * g.held.min(u32::from(e.reserve)) as f32 / g.held as f32;
        }
        total
    }
    /// The oldest note still fading out, which a choke finishes.
    fn fading_note(&self, rs: &Routes, lane: usize) -> Victim {
        let mut best: Option<(usize, usize, i32)> = None;
        for (k, e) in self.entries.iter().enumerate().filter(|(_, e)| e.lane == lane && e.cost.note_now() > 0.0) {
            let r = &rs[e.route];
            for i in r.stolen.and(r.fading).iter() {
                let s = r.slot(i);
                if s.faded < FADE_STEPS && best.is_none_or(|b| s.id < b.2) {
                    best = Some((k, i, s.id));
                }
            }
        }
        best.map(|(k, i, _)| (k, i))
    }
    /// Past the time the lane's blocks have before they are missed: notes choked without a fade (a
    /// click, the last resort), fading ones first. At note-on a reserve goes only when reserves alone
    /// pass it; after a block, never a reserve.
    #[allow(clippy::too_many_arguments)]
    fn choke(&mut self, rs: &mut Routes, lane: usize, scale: f32, extra: f32, me: Option<Route>, time: u32, at_note_on: bool) {
        let limit = self.lane(lane).miss;
        if limit <= 0.0 {
            return;
        }
        while self.estimate(rs, lane, Counting::Fading) * scale + extra > limit {
            let mut v = self.fading_note(rs, lane);
            if v.is_none() {
                v = self.choose(rs, Some(lane), false, true);
            }
            if v.is_none() && at_note_on && self.reserves(rs, lane) * scale + extra > limit {
                v = self.choose(rs, Some(lane), true, true); // reserves alone pass the hard limit
            }
            let Some((e, i)) = v.and_then(|(k, i)| Some((self.entries.get(k)?, i))) else { return };
            if !rs[e.route].stolen.has(i) {
                e.cost.hard_chokes.fetch_add(1, Relaxed); // a fading note was counted when stolen
            }
            rs[e.route].choke(i, if Some(e.route) == me { time } else { 0 });
        }
    }
    /// Gives the victim up, fading from `at`; its node's activity, for the count.
    fn steal(&self, rs: &mut Routes, (k, i): (usize, usize), at: u32) -> Option<&Activity> {
        let e = self.entries.get(k)?;
        rs[e.route].steal(i, at);
        Some(&e.cost)
    }
    /// Before a note into route `me` at `time` passes on: under its lane's soft limit, notes fade to
    /// make room for it; only what would miss the lane's block is choked.
    pub fn admit(&mut self, rs: &mut Routes, me: Route, time: u32) {
        let Some(mine) = self.entries.iter().find(|e| e.route == me) else { return };
        let lane = mine.lane;
        let limit = self.lane(lane).limit;
        if limit <= 0.0 {
            return;
        }
        // A plugin that gives up its own voices keeps the new note within its limit itself.
        let cost = if mine.cost.own_voices.load(Relaxed) { 0.0 } else { mine.cost.note_now() };
        while self.estimate(rs, lane, Counting::Kept) + cost > SOFT * limit {
            let Some(v) = self.choose(rs, Some(lane), false, true) else { break };
            let at = if self.entries.get(v.0).is_some_and(|e| e.route == me) { time } else { 0 };
            if let Some(a) = self.steal(rs, v, at) {
                a.stolen.fetch_add(1, Relaxed);
            }
        }
        self.choke(rs, lane, 1.0, cost, Some(me), time, true);
    }
    /// After a block whose plugins took `total` microseconds on `lane`: past the limit in this block
    /// and the one before, notes fade until the estimate, scaled to the measurement, fits the soft
    /// limit; only what would still miss the lane's block is choked.
    pub fn measured(&mut self, rs: &mut Routes, lane: usize, total: f32) {
        let Some(l) = self.lanes.get_mut(lane) else { return };
        // Twice running: one block over may be the thread held up from outside, or a burst admitted.
        let again = std::mem::replace(&mut l.over, total > l.limit);
        let limit = l.limit;
        if limit <= 0.0 || total <= limit || !again {
            return;
        }
        // At most a quarter over the estimate a block, and never a reserve.
        let e = self.estimate(rs, lane, Counting::Ending);
        if e <= 0.0 {
            return;
        }
        let scale = (total / e).min(1.25);
        while self.estimate(rs, lane, Counting::Kept) * scale > SOFT * limit {
            let Some(v) = self.choose(rs, Some(lane), false, true) else { break };
            if let Some(a) = self.steal(rs, v, 0) {
                a.stolen.fetch_add(1, Relaxed);
            }
        }
        self.choke(rs, lane, scale, 0.0, None, 0, false);
    }
    /// After a block whose lanes together took more than `total` microseconds, in it and the one
    /// before: notes fade, each from the lane where giving one up frees the most time, until the
    /// excess (less the notes already fading) is covered.
    pub fn device(&mut self, rs: &mut Routes, total: f32) {
        let took: f32 = self.lanes.iter().map(|l| l.took).sum();
        let again = std::mem::replace(&mut self.device_over, took > total);
        if self.device_hold != 0 {
            self.device_hold -= 1;
            return;
        }
        if took <= total || !again {
            return;
        }
        let mut excess = took - total;
        for i in 0..self.lanes.len() {
            excess -= self.estimate(rs, i, Counting::Ending) - self.estimate(rs, i, Counting::Kept);
            // given up already, until they end
        }
        while excess > 0.0 {
            let mut best: Victim = None;
            let mut most = 0.0;
            for i in 0..self.lanes.len() {
                if let Some(v) = self.choose(rs, Some(i), false, true) {
                    let each = self.entries.get(v.0).map_or(0.0, |e| e.cost.note_now());
                    if each > most {
                        (best, most) = (Some(v), each);
                    }
                }
            }
            let Some(a) = best.and_then(|v| self.steal(rs, v, 0)) else { return };
            a.stolen_for_device.fetch_add(1, Relaxed);
            excess -= most;
            self.device_hold = DEVICE_HOLD_BLOCKS;
        }
    }
    /// Blocks are being lost (two in the last eight on a lane, one of them this block): one note fades, from
    /// the marked lane where it frees the most time, weighted by the gain on its way to the lost block;
    /// quietest released first within it. Then the fade's time (LOSS_HOLD_BLOCKS) before the next, so one
    /// excess does not take more notes than it needs. Never choked: losses say nothing of how much is over.
    pub fn lost(&mut self, rs: &mut Routes) {
        if self.loss_hold != 0 {
            self.loss_hold -= 1;
            return;
        }
        let mut best: Victim = None;
        let mut most = 0.0;
        for i in 0..self.lanes.len() {
            let l = self.lane(i);
            if l.losses.count_ones() < 2 || l.losses & 1 == 0 {
                continue;
            }
            if let Some(v) = self.choose(rs, Some(i), false, false) {
                let each = self.entries.get(v.0).map_or(0.0, |e| e.cost.note_now()) * l.weight;
                if best.is_none() || each > most {
                    (best, most) = (Some(v), each);
                }
            }
        }
        if let Some(a) = best.and_then(|v| self.steal(rs, v, 0)) {
            a.faded_for_loss.fetch_add(1, Relaxed);
            self.loss_hold = LOSS_HOLD_BLOCKS;
        }
    }
    /// The engine's own time is running out (the callback's slack): one note fades across every lane,
    /// quietest released first.
    pub fn deadline(&mut self, rs: &mut Routes) {
        if let Some(a) = self.choose(rs, None, false, false).and_then(|v| self.steal(rs, v, 0)) {
            a.faded_for_deadline.fetch_add(1, Relaxed);
        }
    }
    /// oscairn.voice-budget: the voices a node on `lane` that gives up its own may keep from the next
    /// block, `voices` sounding now: those and what the soft limit leaves at its cost per voice, at most
    /// a quarter fewer; 0 for no limit (no budget, or no cost measured).
    pub fn voice_limit(&self, rs: &Routes, lane: usize, node: &Activity, voices: u32) -> u32 {
        let each = node.note_now() / node.per_note_scale.get();
        let limit = self.lane(lane).limit;
        if limit <= 0.0 || each <= 0.0 {
            return 0;
        }
        let room = (SOFT * limit - self.estimate(rs, lane, Counting::Kept)) / each;
        // At most a quarter fewer a block; held below 2^16 voices, as a cost near 0 makes the room huge.
        let v = voices as f32;
        65536f32.min(1f32.max((v * 0.75).floor()).max(v + room.floor())) as u32
    }
}
