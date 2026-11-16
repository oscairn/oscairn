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
    pub unfit: AtomicU64,
}

/// The blocks a plugin that gives up its own voices may sound more than its limit before its notes
/// are given up as any plugin's (Activity::overLimitBlocks).
const OVER_LIMIT_BLOCKS: u32 = 16; // a fade's time and more, against the lane's own limit
/// For lost blocks and the deadline, the blocks past the limit of a block before after which a plugin giving up
/// its own voices has its notes faded from outside: its fade may run into the next block.
const BACKSTOP_BLOCKS: u32 = 1;

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
/// part's reserve and priority. For a plugin giving up its own voices, after the guard lowered its limit (12k): cap,
/// the most voices it leaves the node (0 for none); ceiling, the limit in force when the trouble came (0 once
/// forgotten); calm, the healthy blocks since; probes, the probes above the ceiling that failed (each doubles the
/// wait for the next); cost_at, its cost per voice when the ceiling was set.
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
    pub cap: u32,
    pub ceiling: u32,
    pub calm: u32,
    pub probes: u8,
    pub cost_at: f32,
}

impl Entry {
    /// Trouble (a voice given up for lost blocks or the deadline), `limit` what its node sounded under: a plugin giving
    /// up its own voices, its limit in force; any other, the notes the host holds for the node (12m). The ceiling set
    /// there, the cap one under it, the way back started again; trouble at or past the ceiling is a failed probe.
    fn trouble(&mut self, limit: u32) {
        if limit == 0 {
            return;
        }
        // A probe fails where trouble comes with the cap raised to the ceiling or past it (not in the blocks a fall
        // takes, the cap one under the ceiling, its limit a block behind; nor where losses a probe's wait apart reset
        // the healthy count before they were answered).
        if self.ceiling != 0 && self.cap >= self.ceiling {
            self.probes = (self.probes + 1).min(MOST_PROBE_DOUBLINGS);
        }
        (self.ceiling, self.cost_at, self.calm) = (limit, self.cost.note_now(), 0);
        self.cap = (limit - 1).max(1);
    }
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
    pub hold: u8,   // blocks before the next note goes for its losses
    pub since: u32, // blocks since its last loss, plus one; 0 for none yet
    pub worst: f32, // its recent worst block's time: the largest, falling 1 % a block (12n)
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
}

const SOFT: f32 = 0.85; // stealing starts here, so fades have time
/// After notes fade for the total, the blocks it waits before judging it again.
const DEVICE_HOLD_BLOCKS: u32 = 8;
/// After a voice goes for a lane's lost blocks, the blocks before the next: its fade (5 ms).
const LOSS_HOLD_BLOCKS: u8 = 2;
/// A loss this close after the lane's last (2 s) counts as losses going on, however far apart: a chain near the
/// callback's deadline loses blocks one at a time.
const LOSS_MEMORY_BLOCKS: u32 = 750;
/// A lowered limit's way back (12k, 12n), in healthy blocks: below its ceiling less one, a voice back each 8
/// (21 ms); at or above it, a voice each 750 (2 s), doubled for each probe that lost a block, until the notes are let
/// go (held under 2^16 times); each only where the measurements say the voice fits; the ceiling forgotten after
/// 3,750 (10 s) or once the cost per voice moved by a quarter.
const RECOVER_BLOCKS: u32 = 8;
const PROBE_BLOCKS: u32 = 750;
const MOST_PROBE_DOUBLINGS: u8 = 16;
const FORGET_BLOCKS: u32 = 3750;
const COST_DRIFT: f32 = 0.25;

impl Budget {
    pub fn new(entries: Vec<Entry>, lanes: usize) -> Budget {
        Budget { groups: vec![NO_GROUP; entries.len()], entries, lanes: vec![Lane::default(); lanes.max(1)], device_over: false, device_hold: 0 }
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
    /// frees time), else any (time is running out, whatever the measurements say). owned: a plugin keeping
    /// within its limit too, whose limit `give_up` then lowers; and none from a part down to its last voice (the
    /// floor the loss and deadline rules hold: its limit at 1, or one note left).
    fn choose(&mut self, rs: &Routes, lane: Option<usize>, reserved: bool, costed: bool, owned: bool) -> Victim {
        self.choose_on(rs, lane, reserved, costed, owned, false, None)
    }
    /// choose for the soft limit's fades (a note-on, a block past the limit, the lanes' total): a lane's estimate may
    /// pass its soft limit whatever the part does (the time of the plugins beside it in the lane, not its notes), and
    /// giving every note up would not bring it under, so a connection's last note is never chosen, as the loss rules
    /// leave it (only `choke`, for a block that would be missed, takes it).
    fn choose_soft(&mut self, rs: &Routes, lane: Option<usize>) -> Victim {
        self.choose_on(rs, lane, false, true, false, true, None)
    }
    /// choose, from `node`'s connections alone when given; `last`: each connection keeps its last note.
    #[allow(clippy::too_many_arguments)]
    fn choose_on(&mut self, rs: &Routes, lane: Option<usize>, reserved: bool, costed: bool, owned: bool, last: bool, node: Option<*const Activity>) -> Victim {
        self.count_held(rs, None);
        let mut best: Victim = None;
        let (mut best_rank, mut best_priority, mut best_honors, mut best_velocity, mut best_id) = (0, 0u8, false, 0f32, 0i32);
        for (k, e) in self.entries.iter().enumerate() {
            let a = &e.cost;
            let within = a.voice_limit.load(Relaxed) != 0 && a.over_limit.load(Relaxed) <= OVER_LIMIT_BLOCKS;
            if lane.is_some_and(|l| l != e.lane) || node.is_some_and(|n| !std::ptr::eq(&**a, n)) || (costed && a.note_now() <= 0.0) || (!owned && a.own_voices.load(Relaxed) && within) {
                continue;
            }
            let (r, g) = (&rs[e.route], self.groups.get(e.group).copied().unwrap_or(NO_GROUP));
            if (owned || last) && (if owned && a.own_voices.load(Relaxed) && a.voice_limit.load(Relaxed) != 0 { a.voice_limit.load(Relaxed) <= 1 } else { r.used.and_not(r.stolen).count() <= 1 }) {
                continue;
            }
            let honors = a.honors_volume.load(Relaxed);
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
                v = self.choose(rs, Some(lane), false, true, false);
            }
            if v.is_none() && at_note_on && self.reserves(rs, lane) * scale + extra > limit {
                v = self.choose(rs, Some(lane), true, true, false); // reserves alone pass the hard limit
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
        let Some(k) = self.entries.iter().position(|e| e.route == me) else { return };
        self.capped(rs, k, time);
        let Some(mine) = self.entries.get(k) else { return };
        let lane = mine.lane;
        let limit = self.lane(lane).limit;
        if limit <= 0.0 {
            return;
        }
        // A plugin that gives up its own voices keeps the new note within its limit itself.
        let cost = if mine.cost.own_voices.load(Relaxed) { 0.0 } else { mine.cost.note_now() };
        while self.estimate(rs, lane, Counting::Kept) + cost > SOFT * limit {
            let Some(v) = self.choose_soft(rs, Some(lane)) else { break };
            let at = if self.entries.get(v.0).is_some_and(|e| e.route == me) { time } else { 0 };
            if let Some(a) = self.steal(rs, v, at) {
                a.stolen.fetch_add(1, Relaxed);
            }
        }
        self.choke(rs, lane, 1.0, cost, Some(me), time, true);
    }
    /// The notes the host holds for entry `k`'s node (unreleased or released, not given up), across its connections.
    fn node_notes(&self, rs: &Routes, k: usize) -> u32 {
        let Some(node) = self.entries.get(k).map(|e| &*e.cost) else { return 0 };
        self.entries.iter().filter(|e| std::ptr::eq(&*e.cost, node)).map(|e| rs[e.route].used.and_not(rs[e.route].stolen).count()).sum()
    }
    /// 12m: before a note into entry `k` passes on, a plugin that does not give up its own voices, its node capped after
    /// lost blocks or the deadline, keeps under its cap: its node's quietest released note fades first, so a note
    /// struck again does not bring back what the losses took.
    fn capped(&mut self, rs: &mut Routes, k: usize, time: u32) {
        let Some(node) = self.entries.get(k).map(|e| &*e.cost as *const Activity) else { return };
        // SAFETY: the node's activity lives as long as the plan's entries, which this borrows.
        let own = unsafe { &*node }.own_voices.load(Relaxed);
        let cap = self.entries.iter().filter(|e| e.cap != 0 && std::ptr::eq(&*e.cost, node)).map(|e| e.cap).min().unwrap_or(0);
        if own || cap == 0 {
            return;
        }
        while self.node_notes(rs, k) >= cap {
            let Some(v) = self.choose_on(rs, None, false, false, false, false, Some(node)) else { break };
            let at = if self.entries.get(v.0).is_some_and(|e| e.route == self.entries[k].route) { time } else { 0 };
            if let Some(a) = self.steal(rs, v, at) {
                a.stolen.fetch_add(1, Relaxed);
            }
        }
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
            let Some(v) = self.choose_soft(rs, Some(lane)) else { break };
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
                if let Some(v) = self.choose_soft(rs, Some(i)) {
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
    /// A voice goes: from a plugin giving up its own voices and keeping within a limit above one, off its limit
    /// (it chooses the voice: their levels, layers and streams are its to know); else the note fades.
    /// Its node's activity, for the count.
    fn give_up(&mut self, rs: &mut Routes, (k, i): (usize, usize)) -> Option<&Activity> {
        let own = self.entries.get(k)?.cost.own_voices.load(Relaxed);
        let held = if own { 0 } else { self.node_notes(rs, k) };
        let e = self.entries.get_mut(k)?;
        let backstop = !own || e.cost.voice_limit.load(Relaxed) <= 1 || e.cost.over_limit.load(Relaxed) > BACKSTOP_BLOCKS;
        e.trouble(if own { e.cost.voice_limit.load(Relaxed) } else { held });
        if backstop {
            rs[e.route].steal(i, 0);
        }
        Some(&e.cost)
    }
    /// Blocks are being lost (on a lane this block, and two in its last eight or its last loss within
    /// LOSS_MEMORY_BLOCKS): on each such lane a voice goes, quietest released first, then the fade's time
    /// (LOSS_HOLD_BLOCKS) before the next, so the lanes losing blocks all give way at once and keep giving way
    /// while losses go on. Never choked: losses say nothing of how much is over. Down to every part's last
    /// voice, the loss is counted (`unfit`, on the lane's nodes holding notes): the chain after it cannot fit.
    pub fn lost(&mut self, rs: &mut Routes) {
        for i in 0..self.lanes.len() {
            let Some(l) = self.lanes.get_mut(i) else { continue };
            let now = l.losses & 1 != 0;
            let recent = l.since != 0 && l.since <= LOSS_MEMORY_BLOCKS;
            l.since = if now { 1 } else if l.since != 0 { l.since.saturating_add(1) } else { 0 };
            if l.hold != 0 {
                l.hold -= 1;
                continue;
            }
            if !now || (l.losses.count_ones() < 2 && !recent) {
                continue;
            }
            l.hold = LOSS_HOLD_BLOCKS;
            match self.choose(rs, Some(i), false, false, true) {
                Some(v) => {
                    if let Some(a) = self.give_up(rs, v) {
                        a.faded_for_loss.fetch_add(1, Relaxed);
                    }
                }
                None => {
                    for e in self.entries.iter().filter(|e| e.lane == i && rs[e.route].used.any()) {
                        e.cost.unfit.fetch_add(1, Relaxed);
                    }
                }
            }
        }
    }
    /// The engine's own time is running out (the callback's slack, or blocks lost): a voice goes across every
    /// lane, quietest released first.
    pub fn deadline(&mut self, rs: &mut Routes) {
        if let Some(a) = self.choose(rs, None, false, false, true).and_then(|v| self.give_up(rs, v)) {
            a.faded_for_deadline.fetch_add(1, Relaxed);
        }
    }
    /// After each block, the lowered limits' way back (12k): all let go once the plugin holds no note; else on a
    /// healthy block (no loss on its lane in its last eight, the lane within its soft limit, the callback's slack
    /// fine) a voice back each RECOVER_BLOCKS below the ceiling less one, each PROBE_BLOCKS (doubled per failed
    /// probe) above it, the ceiling forgotten after FORGET_BLOCKS or once the cost per voice moved by COST_DRIFT.
    /// `slack_low`: the callback's slack ran low this block, or a block was lost. 12n: a voice comes back only where
    /// it fits by the measurements: the lane's recent worst block and the node's cost per note within the lane's soft
    /// limit, and `room` (the callback's recent worst slack; None unmeasured) less that cost still at least the soft
    /// limit's margin of `deadline`. Without the measurements (no slack, no cost, no lane limit), only after
    /// FORGET_BLOCKS healthy, at the probe's pace.
    pub fn recover(&mut self, rs: &Routes, slack_low: bool, room: Option<f32>, deadline: f32) {
        for k in 0..self.entries.len() {
            let held = self.node_notes(rs, k);
            let Some(e) = self.entries.get_mut(k) else { continue };
            if e.cap == 0 {
                continue;
            }
            if !rs[e.route].used.any() {
                (e.cap, e.ceiling, e.calm, e.probes) = (0, 0, 0, 0);
                continue;
            }
            let l = self.lanes.get(e.lane).copied().unwrap_or_default();
            let limit = if e.cost.own_voices.load(Relaxed) { e.cost.voice_limit.load(Relaxed) } else { held };
            if slack_low || l.losses != 0 || (l.limit > 0.0 && l.took > SOFT * l.limit) {
                e.calm = 0;
                continue;
            }
            e.calm += 1;
            let cost = e.cost.note_now();
            if e.ceiling != 0 && (e.calm >= FORGET_BLOCKS || (cost - e.cost_at).abs() > COST_DRIFT * e.cost_at) {
                (e.ceiling, e.probes) = (0, 0);
            }
            let measured = cost > 0.0 && l.limit > 0.0 && room.is_some();
            let fits = l.worst + cost <= SOFT * l.limit && room.is_some_and(|r| r - cost >= (1.0 - SOFT) * deadline);
            let probe = PROBE_BLOCKS.saturating_mul(1 << e.probes.min(MOST_PROBE_DOUBLINGS));
            let every = if !measured { probe } else if e.ceiling == 0 || e.cap + 1 < e.ceiling { RECOVER_BLOCKS } else { probe };
            if e.calm % every == 0 && (if measured { fits } else { e.calm >= FORGET_BLOCKS }) {
                e.cap += 1;
            }
            if e.ceiling == 0 && e.cap > limit + 1 {
                e.cap = 0; // no longer what holds it
            }
        }
    }
    /// oscairn.voice-budget: the voices a node on `lane` that gives up its own may keep, `admitted` the
    /// voices of the notes the host holds for it (not given up) and `voices` those it sounds now: those
    /// admitted and the room the soft limit leaves at its cost per voice (the estimate counts the admitted
    /// already), never fewer than those admitted unless the lane is measured over its limit, and then at
    /// most a quarter fewer than it sounds a block; 0 for no limit (no budget, or no cost measured).
    pub fn voice_limit(&self, rs: &Routes, lane: usize, node: &Activity, voices: u32, admitted: f32) -> u32 {
        let cap = self.entries.iter().filter(|e| e.cap != 0 && std::ptr::eq(&*e.cost, node)).map(|e| e.cap).min().unwrap_or(u32::MAX);
        self.limit_for(rs, lane, node, voices, admitted).map_or(0, |l| l.min(cap))
    }
    fn limit_for(&self, rs: &Routes, lane: usize, node: &Activity, voices: u32, admitted: f32) -> Option<u32> {
        let each = node.note_now() / node.per_note_scale.get();
        let l = self.lane(lane);
        if l.limit <= 0.0 || each <= 0.0 {
            return None;
        }
        let room = ((SOFT * l.limit - self.estimate(rs, lane, Counting::Kept)) / each).floor();
        let least = if l.over { (voices as f32 * 0.75).floor() } else { admitted };
        // Held below 2^16 voices, as a cost near 0 makes the room huge.
        Some(65536f32.min(1f32.max(least).max(admitted + room)) as u32)
    }
}
