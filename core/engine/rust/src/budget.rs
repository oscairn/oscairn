//! The voice budget (RFC 0018 option B, per lane as RFC 0045; once VoiceBudget in core/engine/src/NoteRoute.h) and the node
//! costs it reads (Graph::Activity). Counts the C++ scans for are popcounts of the routes' maps.
//!
//! Which voices go is one order over every part (RFC 0046): a voice's tier (released, held by a pedal, key held, just
//! struck), its part's priority, then how much it is heard in the mix. And one rule says where they may come from: only
//! where taking them frees the time that is short. A lane over its own time (its thread's block) gives up voices on that
//! lane alone: its own engine's, and in a pool or a shared instance its neighbors'. Voices on other lanes go only when the
//! device is out of core time as a whole (the lanes' total), and then only as many as the excess.

use crate::route::{NoteRoute, FADE_STEPS};
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
    pub importance_mismatch: AtomicU64,
    pub held_off: AtomicU64,
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
    pub step: usize,
    pub channel: i16,
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
    /// Its sound context's importance report (draft 2), on the entry its context's report is kept on; None for none.
    pub heard: Option<Heard>,
}

/// A sound context's least important voices as its plugin last listed them (oscairn.voice-budget draft 2), each by
/// its tier, audibility before the part's gain and priority, and weight (its cost against the instance's other voices);
/// unit, the cost of a weight of 1 in the instance's voices (their count over their weights); how many were given up
/// since (by lowering its context's limit, which the plugin keeps to by the same order) and their weights, and the limit
/// that set, to check it was kept.
#[derive(Clone, Copy, Default)]
pub struct Heard {
    pub voices: u32,
    pub count: usize,
    pub least: [(u8, f32, f32); LEAST],
    pub unit: f32,
    pub taken: usize,
    pub freed: f32,
    pub admitted: u32, // voices of notes admitted since its report, which the plugin now sounds too
    pub checked: u32, // the cap to check at the report after next (0 none), and the blocks until then
    pub check_in: u8,
}
pub const LEAST: usize = 16;

/// RFC 0046's scale (voice-budget-plugin.hpp's): a voice struck this recently is not judged by its level; below
/// INAUDIBLE_DB a voice counts as released; each second of age counts as AGED_DB_PER_S quieter, up to AGED_MOST_S.
pub const STRUCK_MS: f32 = 50.0;
pub const INAUDIBLE_DB: f32 = -60.0;
const AGED_DB_PER_S: f32 = 1.0;
const AGED_MOST_S: f32 = 10.0;
/// A note whose plugin reports no level is heard at the middle of the range, by its velocity.
const UNKNOWN_DB: f32 = -24.0;
/// One key orders every voice: the tier above all, a part's highest and lowest held keys (the bass and the melody of a
/// chord) last within the held tier, then the part's priority, then a note just struck above the others of its tier and
/// priority, then audibility.
const TIER: f32 = 100_000.0;
const OUTER: f32 = 30_000.0;
const PRIORITY: f32 = 2000.0;
const STRUCK_ABOVE: f32 = 1000.0;
pub const RELEASED: u8 = 0;
pub const PEDALED: u8 = 1;
pub const KEY_HELD: u8 = 2;
/// A tier's flag: struck under STRUCK_MS ago.
pub const STRUCK: u8 = 0x80;

/// A voice's place in the order: lower goes first.
pub fn key(tier: u8, priority: u8, audibility: f32) -> f32 {
    let struck = if tier & STRUCK != 0 { STRUCK_ABOVE } else { 0.0 };
    TIER * f32::from((tier & !STRUCK).min(KEY_HELD)) + PRIORITY * f32::from(priority.min(10)) + struck + (100.0 + audibility).clamp(0.0, 999.0)
}
/// A voice of the last Performance ringing out (a draining connection, RFC 0021): below every live voice of its tier, above
/// the tier under it (a tier's keys span 0 to 52,000 above its base; this takes 60,000 off).
const RETIRING: f32 = 60_000.0;
/// Entry `e`'s voice by the key, a retiring one taken below the live voices of its tier.
fn entry_key(e: &Entry, tier: u8, audibility: f32) -> f32 {
    retiring(key(tier, e.priority, audibility), e.draining)
}
fn retiring(key: f32, draining: bool) -> f32 {
    if draining {
        key - RETIRING
    } else {
        key
    }
}
/// Its level in the mix less its age's share.
pub fn audibility(level_db: f32, age_ms: f32) -> f32 {
    level_db - (age_ms.max(0.0) / 1000.0 * AGED_DB_PER_S).min(AGED_MOST_S * AGED_DB_PER_S)
}
pub fn db(gain: f32) -> f32 {
    if gain > 1e-6 {
        (20.0 * gain.log10()).clamp(-120.0, 24.0)
    } else {
        -120.0
    }
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
    /// The voices its context sounds, with those of notes admitted since its report, less those given up since; 0
    /// without a report.
    fn listed_left(&self) -> u32 {
        self.heard.map_or(0, |h| (h.voices + h.admitted).saturating_sub(h.taken as u32))
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
    pub settle: u8, // blocks before it is judged against its limit again, after listed voices on it went
    /// This block's raw time (each step's last block, not their medians), its threads' CPU time in it (a step's time
    /// where unknown; timed, some step's known), and its thread's wait to be woken, the least of its plugins' (0 unknown;
    /// RFC 0046 step 4).
    pub run: f32,
    pub cpu: f32,
    pub timed: bool,
    pub wait: f32,
    pub lost_own: bool, // a block of its own lost this block (not one after it)
    /// Over its time or losing a block of its own only by waiting for a core: its threads' CPU time within its limit,
    /// with its time well past that CPU time (preempted) or a wait to be woken past WAIT_SHARE of its time. Its own
    /// voices would not help; the device's time is what is short.
    pub held: bool,
}

impl Lane {
    /// RFC 0046 step 4: over its time, or losing a block of its own, while its threads' own work (their CPU time) fits its
    /// limit and they were held off a core (their time well past their CPU time, or a long wait to be woken): the
    /// device's time is short, not its thread's. A lane over by its own work is its own to answer, however long its next
    /// wake waits behind it; and a lane marked for a loss after it, within its own time, is not held: its voices free
    /// the time of the block that was lost.
    pub fn judge_held(&mut self) {
        let trouble = self.took > self.limit || self.lost_own;
        let held_off = self.run - self.cpu > (1.0 - SOFT) * self.limit || self.wait > WAIT_SHARE * self.miss;
        self.held = trouble && self.timed && self.limit.is_finite() && self.cpu <= self.limit && held_off;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Counting {
    Kept,   // not given up
    Fading, // and those fading out
    Ending, // and those choked, until they end
}

type Victim = Option<(usize, usize)>; // entry, slot; LISTED for the next voice its context's report lists
/// A Victim's slot for a voice of a reporting context: its context gives up the voice it listed next.
pub const LISTED: usize = usize::MAX;

pub struct Budget {
    pub entries: Vec<Entry>,
    groups: Vec<Group>, // scratch, one per entry
    pub lanes: Vec<Lane>,
    device_over: bool,
    device_hold: u32,
    /// Each step's largest gain to the graph's outputs at the live gains (a part's level and the mix), linear.
    pub gains: Vec<f32>,
    /// Microseconds a frame, for the notes' ages, and a block's, for their slack.
    pub frame_us: f32,
    pub block_us: f32,
    /// The lanes a deadline's give-up may take from (MARKED): the one furthest over its time and those feeding it.
    pub marked: Vec<bool>,
}
/// A thread woken this share of its time late was held off its core: the device's time is short, not its own.
const WAIT_SHARE: f32 = 0.25;
/// choose's `lane` for the lanes `marked` names.
pub const MARKED: usize = usize::MAX;

const SOFT: f32 = 0.85; // stealing starts here, so fades have time
/// After notes fade for the total, the blocks it waits before judging it again.
const DEVICE_HOLD_BLOCKS: u32 = 8;
/// After a voice goes for a lane's lost blocks, the blocks before the next: its fade (5 ms).
const LOSS_HOLD_BLOCKS: u8 = 2;
/// After listed voices go on a lane, the blocks before it is judged against its limit again: the plugin gives them up at
/// its next block (a block on where they went at a later step's note-on) and its report drops them at once, while the
/// lane's time, the median of five blocks, follows in three.
const SETTLE_BLOCKS: u8 = 3;
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
        let steps = entries.iter().map(|e| e.step + 1).max().unwrap_or(0);
        let lanes = lanes.max(1);
        Budget {
            groups: vec![NO_GROUP; entries.len()],
            entries,
            lanes: vec![Lane::default(); lanes],
            device_over: false,
            device_hold: 0,
            gains: vec![1.0; steps],
            frame_us: 0.0,
            block_us: 0.0,
            marked: vec![false; lanes],
        }
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
            if counting != Counting::Ending {
                // Listed voices given up: no longer counted, their fades a block away.
                total -= e.heard.map_or(0.0, |h| h.freed * h.unit) * self.voice_cost(e);
            }
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
        self.choose_on(rs, lane, reserved, costed, owned, false, None, owned)
    }
    /// choose for the soft limit's fades (a note-on, a block past the limit, the lanes' total): a lane's estimate may
    /// pass its soft limit whatever the part does (the time of the plugins beside it in the lane, not its notes), and
    /// giving every note up would not bring it under, so a connection's last note is never chosen, as the loss rules
    /// leave it (only `choke`, for a block that would be missed, takes it).
    fn choose_soft(&mut self, rs: &Routes, lane: Option<usize>) -> Victim {
        self.choose_on(rs, lane, false, true, false, true, None, true)
    }
    /// choose, from `node`'s connections alone when given; `last`: each connection keeps its last note. The least
    /// important voice by RFC 0046's key: of the notes the host holds, and with `listed`, of the voices a plugin listed
    /// (draft 2) while it keeps to its limits; a plugin giving up its own voices without listing them is left to its
    /// limit unless `owned`. Ties: a plugin that honors the volume expression, then the oldest note.
    #[allow(clippy::too_many_arguments)]
    fn choose_on(&mut self, rs: &Routes, lane: Option<usize>, reserved: bool, costed: bool, owned: bool, last: bool, node: Option<*const Activity>, listed: bool) -> Victim {
        self.count_held(rs, None);
        let mut best: Victim = None;
        let (mut best_key, mut best_honors, mut best_id) = (0f32, false, 0i32);
        // A context whose whole list went with voices left unlisted: those are at least as important as its last listed,
        // and may be less than any other; nothing above that goes until its next report lists them.
        let mut unlisted = f32::INFINITY;
        for (k, e) in self.entries.iter().enumerate() {
            let a = &e.cost;
            let elsewhere = lane.is_some_and(|l| if l == MARKED { !self.marked.get(e.lane).copied().unwrap_or(false) } else { l != e.lane });
            if elsewhere || node.is_some_and(|n| !std::ptr::eq(&**a, n)) || (costed && a.note_now() <= 0.0) {
                continue;
            }
            let keeping = a.over_limit.load(Relaxed) <= OVER_LIMIT_BLOCKS;
            if e.heard.is_some() && a.own_voices.load(Relaxed) && keeping {
                // Its context's next listed voice, never its last.
                if let Some(h) = e.heard.filter(|h| listed && h.count == LEAST && h.taken >= h.count && e.listed_left() > 1) {
                    let (tier, heard, _) = h.least[LEAST - 1];
                    unlisted = unlisted.min(entry_key(e, tier, heard + self.gain_db(e)));
                }
                if let Some(x) = listed.then(|| self.listed_key(e, true)).flatten() {
                    if best.is_none() || x < best_key {
                        best = Some((k, LISTED));
                        (best_key, best_honors, best_id) = (x, true, i32::MIN);
                    }
                }
                continue;
            }
            let within = a.voice_limit.load(Relaxed) != 0 && keeping;
            if !owned && a.own_voices.load(Relaxed) && within {
                continue;
            }
            let (r, g) = (&rs[e.route], self.groups.get(e.group).copied().unwrap_or(NO_GROUP));
            if (owned || last) && (if owned && a.own_voices.load(Relaxed) && a.voice_limit.load(Relaxed) != 0 { a.voice_limit.load(Relaxed) <= 1 } else { r.used.and_not(r.stolen).count() <= 1 }) {
                continue;
            }
            let honors = a.honors_volume.load(Relaxed);
            for i in r.used.and_not(r.stolen).iter() {
                if r.held.has(i) && !reserved && g.held <= u32::from(e.reserve) {
                    continue;
                }
                let (x, id) = (self.note_key(r, e, &g, i), r.slot(i).id);
                let better = best.is_none() || if x != best_key { x < best_key } else if honors != best_honors { honors } else { id < best_id };
                if better {
                    best = Some((k, i));
                    (best_key, best_honors, best_id) = (x, honors, id);
                }
            }
        }
        best.filter(|_| best_key <= unlisted)
    }
    /// Entry `e`'s part gain to the outputs, in dB.
    fn gain_db(&self, e: &Entry) -> f32 {
        db(self.gains.get(e.step).copied().unwrap_or(1.0))
    }
    /// A note the host holds, by RFC 0046's key: its tier from what the host knows (the key, the pedal, its age), its level
    /// the plugin's report where it gives one (else the middle of the range, by its velocity), in the mix.
    fn note_key(&self, r: &NoteRoute, e: &Entry, g: &Group, i: usize) -> f32 {
        let s = r.slot(i);
        let age = r.age(i) as f32 * self.frame_us / 1000.0;
        let reported = e.cost.own_voices.load(Relaxed);
        let level = if reported { db(s.velocity) } else { UNKNOWN_DB + db(s.velocity) };
        let held = r.held.has(i);
        // Its age counts from its block's start, up to a block more than it has sounded: protected a block longer, never less.
        let struck = self.frame_us > 0.0 && age < STRUCK_MS + self.block_us / 1000.0;
        let tier = if !struck && reported && level < INAUDIBLE_DB {
            RELEASED
        } else if held {
            KEY_HELD
        } else if r.pedaled(i) {
            PEDALED
        } else {
            RELEASED
        };
        let outer = tier == KEY_HELD && g.held > 2 && (s.key == g.low || s.key == g.high);
        let tier = if struck { tier | STRUCK } else { tier };
        entry_key(e, tier, audibility(level + self.gain_db(e), age)) + if outer { OUTER } else { 0.0 }
    }
    /// The next voice entry `e`'s context listed, by the key: None when none is left to give (or, with `last`, its last).
    fn listed_key(&self, e: &Entry, last: bool) -> Option<f32> {
        let h = e.heard?;
        if h.taken >= h.count || (last && e.listed_left() <= 1) {
            return None;
        }
        let (tier, heard, _) = h.least[h.taken];
        Some(entry_key(e, tier, heard + self.gain_db(e)))
    }
    /// A voice's cost on entry `e`'s node.
    fn voice_cost(&self, e: &Entry) -> f32 {
        let scale = e.cost.per_note_scale.get();
        e.cost.note_now() / if scale > 0.0 { scale } else { 1.0 }
    }
    /// What giving victim `v` up frees, by the estimates.
    fn victim_cost(&self, (k, i): (usize, usize)) -> f32 {
        self.entries.get(k).map_or(0.0, |e| match e.heard.filter(|h| i == LISTED && h.taken < h.count) {
            Some(h) => h.least[h.taken].2 * h.unit * self.voice_cost(e),
            None if i == LISTED => self.voice_cost(e),
            None => e.cost.note_now(),
        })
    }
    /// A listed voice of entry `k`'s context goes: its context's cap a voice lower, which the plugin keeps to by giving
    /// up the voice it listed next; for lost blocks or the deadline (`trouble`) as trouble, so its way back probes.
    fn take_listed(&mut self, k: usize, trouble: bool) {
        let Some(e) = self.entries.get_mut(k) else { return };
        if let Some(l) = self.lanes.get_mut(e.lane) {
            l.settle = SETTLE_BLOCKS;
        }
        let before = e.listed_left();
        let Some(h) = e.heard.as_mut() else { return };
        h.freed += h.least.get(h.taken).map_or(1.0, |x| x.2);
        h.taken += 1;
        if trouble {
            e.trouble(before);
        } else {
            let cap = before.saturating_sub(1).max(1);
            e.cap = if e.cap == 0 { cap } else { e.cap.min(cap) };
            e.calm = 0;
        }
        if let Some(h) = e.heard.as_mut() {
            (h.checked, h.check_in) = (e.cap, 2);
        }
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
    fn steal(&mut self, rs: &mut Routes, (k, i): (usize, usize), at: u32) -> Option<&Activity> {
        if i == LISTED {
            self.take_listed(k, false);
        } else {
            rs[self.entries.get(k)?.route].steal(i, at);
        }
        Some(&self.entries.get(k)?.cost)
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
        // A plugin that gives up its own voices keeps the new note within its limit itself; one listing its voices is
        // limited only where the budget took them (its limit is at least what it sounds), so its new note is made room for:
        // one of its voices (what its limit counts: a sampler's note, a synth's voice) at the instance's cost per voice.
        let (step, channel) = (mine.step, mine.channel);
        let listing = self.entries.iter().position(|e| e.step == step && e.channel == channel && e.heard.is_some());
        let cost = match listing {
            Some(_) => self.voice_cost(mine),
            None if mine.cost.own_voices.load(Relaxed) => 0.0,
            None => mine.cost.note_now(),
        };
        // That voice counts with its context's listed ones until the next report has it, so a cap set meanwhile (below
        // too) leaves it the note just admitted.
        if let Some(h) = listing.and_then(|l| self.entries.get_mut(l)).and_then(|e| e.heard.as_mut()) {
            h.admitted += 1;
        }
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
            let Some(v) = self.choose_on(rs, None, false, false, false, false, Some(node), false) else { break };
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
        if l.settle != 0 {
            l.settle -= 1;
            return;
        }
        // Over only by waiting for a core: the device rule's to answer, from any lane (RFC 0046 step 4).
        if limit <= 0.0 || total <= limit || !again || l.held {
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
    /// After a block whose lanes together took more than `total` microseconds, or with a lane over its time only by
    /// waiting for a core (held), in it and the one before: the device is out of core time as a whole, so any lane's
    /// voice frees what is short. The least important voices go, wherever they are, until the excess (the total's, or
    /// the held lanes' time past their limits; less the notes already fading) is covered.
    pub fn device(&mut self, rs: &mut Routes, total: f32) {
        let took: f32 = self.lanes.iter().map(|l| l.took).sum();
        let held: f32 = self.lanes.iter().filter(|l| l.held).map(|l| (l.took - l.limit).max(0.0)).sum();
        let short = (took - total).max(held);
        let again = std::mem::replace(&mut self.device_over, short > 0.0);
        if self.device_hold != 0 {
            self.device_hold -= 1;
            return;
        }
        if short <= 0.0 || !again {
            return;
        }
        let mut excess = short;
        for i in 0..self.lanes.len() {
            excess -= self.estimate(rs, i, Counting::Ending) - self.estimate(rs, i, Counting::Kept);
            // given up already, until they end
        }
        while excess > 0.0 {
            let Some(v) = self.choose_soft(rs, None) else { return };
            let each = self.victim_cost(v);
            let Some(a) = self.steal(rs, v, 0) else { return };
            a.stolen_for_device.fetch_add(1, Relaxed);
            excess -= each;
            self.device_hold = DEVICE_HOLD_BLOCKS;
        }
    }
    /// A voice goes: from a plugin giving up its own voices and keeping within a limit above one, off its limit
    /// (it chooses the voice: their levels, layers and streams are its to know); else the note fades.
    /// Its node's activity, for the count.
    fn give_up(&mut self, rs: &mut Routes, (k, i): (usize, usize)) -> Option<&Activity> {
        if i == LISTED {
            self.take_listed(k, true);
            return Some(&self.entries.get(k)?.cost);
        }
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
            // A lane held off its core loses blocks for the device's time: any lane's voice frees it.
            let from = if l.held { None } else { Some(i) };
            match self.choose(rs, from, false, false, true) {
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
    /// The engine's own time is running out (the callback's slack, or blocks lost): the least important voice goes
    /// where it frees what is short: with the device out of core time, from any lane; else from the lanes `marked` (the
    /// one furthest over its time and those feeding it).
    pub fn deadline(&mut self, rs: &mut Routes) {
        let lane = if self.device_over { None } else { Some(MARKED) };
        if let Some(a) = self.choose(rs, lane, false, false, true).and_then(|v| self.give_up(rs, v)) {
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
            let limit = match e.heard {
                Some(h) => h.voices,
                None if e.cost.own_voices.load(Relaxed) => e.cost.voice_limit.load(Relaxed),
                None => held,
            };
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
    /// A node whose contexts list their voices (draft 2) is never cut by its lane's room: the budget takes its voices by
    /// lowering each context's own limit (`context_cap`), so its limit is at least the voices it sounds.
    pub fn voice_limit(&self, rs: &Routes, lane: usize, node: &Activity, voices: u32, admitted: f32) -> u32 {
        let mine = || self.entries.iter().filter(|e| std::ptr::eq(&*e.cost, node));
        let listed = mine().any(|e| e.heard.is_some());
        let cap = if listed { u32::MAX } else { mine().filter(|e| e.cap != 0).map(|e| e.cap).min().unwrap_or(u32::MAX) };
        self.limit_for(rs, lane, node, voices, if listed { admitted.max(voices as f32) } else { admitted }, listed).map_or(0, |l| l.min(cap))
    }
    /// The cap on the context a step's `channel` plays, where its plugin lists its voices (0: none).
    pub fn context_cap(&self, step: usize, channel: i16) -> Option<u32> {
        self.entries.iter().find(|e| e.step == step && e.channel == channel && e.heard.is_some()).map(|e| e.cap)
    }
    /// The cap on a step that one part plays alone, where its plugin lists its voices.
    pub fn step_cap(&self, step: usize) -> Option<u32> {
        self.entries.iter().find(|e| e.step == step && e.heard.is_some()).map(|e| e.cap)
    }
    fn limit_for(&self, rs: &Routes, lane: usize, node: &Activity, voices: u32, admitted: f32, listed: bool) -> Option<u32> {
        let each = node.note_now() / node.per_note_scale.get();
        let l = self.lane(lane);
        if l.limit <= 0.0 || each <= 0.0 {
            return None;
        }
        let room = ((SOFT * l.limit - self.estimate(rs, lane, Counting::Kept)) / each).floor();
        let least = if l.over && !listed { (voices as f32 * 0.75).floor() } else { admitted };
        // Held below 2^16 voices, as a cost near 0 makes the room huge.
        Some(65536f32.min(1f32.max(least).max(admitted + room)) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node whose voices cost next to nothing leaves room for billions: the limit stays a count a u32 holds (found
    /// by a sanitizer in the C++ reference, which converted the float out of range).
    #[test]
    fn voice_limit_held_at_2_16() {
        let mut budget = Budget::new(Vec::new(), 1);
        budget.lanes[0].limit = 2000.0;
        let cheap = Activity::default();
        cheap.per_note.set(1e-7);
        cheap.per_note_scale.set(1.0);
        assert_eq!(budget.voice_limit(&crate::Routes(()), 0, &cheap, 10, 10.0), 65536);
    }

    /// RFC 0046 step 4: a lane is held off its core (the device's time short, not its thread's) only when it is over
    /// its time or lost a block of its own while its own work fits its limit and it was kept from running.
    #[test]
    fn held_only_by_waiting_for_a_core() {
        let lane = |took: f32, run: f32, cpu: f32, wait: f32, lost_own: bool| {
            let mut l = Lane { limit: 800.0, miss: 1000.0, took, run, cpu, timed: cpu > 0.0, wait, lost_own, ..Lane::default() };
            l.judge_held();
            l.held
        };
        assert!(!lane(900.0, 900.0, 890.0, 20.0, false), "over by its own work: its own to answer");
        assert!(!lane(900.0, 900.0, 890.0, 600.0, false), "over by its own work, however long its next wake waited behind it");
        assert!(lane(900.0, 900.0, 600.0, 20.0, false), "preempted: its time well past its CPU time");
        assert!(lane(700.0, 700.0, 650.0, 400.0, true), "a block of its own lost after a long wait to be woken");
        assert!(!lane(700.0, 700.0, 650.0, 20.0, true), "a block lost with no sign of waiting");
        assert!(!lane(700.0, 700.0, 300.0, 400.0, false), "within its time: nothing to answer");
        assert!(!lane(900.0, 900.0, 0.0, 600.0, false), "its CPU time unknown (the engine's process)");
    }

    /// RFC 0046's key: the tier above all (a quiet held key above a loud pedaled note), then the part's priority, then
    /// how much it is heard; a note just struck above the others of its tier and priority.
    #[test]
    fn importance_key_orders_tier_priority_audibility() {
        assert!(key(PEDALED, 10, 0.0) < key(KEY_HELD, 0, -50.0));
        assert!(key(KEY_HELD, 5, 0.0) < key(KEY_HELD | STRUCK, 5, -90.0));
        assert!(key(KEY_HELD | STRUCK, 4, 0.0) < key(KEY_HELD, 5, -90.0));
        assert!(key(RELEASED | STRUCK, 10, 0.0) < key(PEDALED, 0, -90.0));
        assert!(key(KEY_HELD, 10, 0.0) + OUTER > key(KEY_HELD | STRUCK, 10, 0.0));
        assert!(key(PEDALED, 4, 0.0) < key(PEDALED, 5, -80.0));
        assert!(key(RELEASED, 5, -30.0) < key(RELEASED, 5, -20.0));
        assert!(audibility(-20.0, 5000.0) < audibility(-20.0, 1000.0));
        assert_eq!(audibility(-20.0, 60_000.0), audibility(-20.0, 10_000.0));
    }
    /// A retiring tail's voice (the last Performance's, ringing out) goes before every live voice of its tier, however
    /// loud and whatever its part's priority, and after every voice of the tier under it.
    #[test]
    fn retiring_voices_rank_below_live_ones_of_their_tier() {
        for tier in [RELEASED, PEDALED, KEY_HELD] {
            let top = retiring(key(tier | STRUCK, 10, 899.0) + if tier == KEY_HELD { OUTER } else { 0.0 }, true);
            assert!(top < key(tier, 0, -100.0), "tier {tier}");
            if tier > RELEASED {
                assert!(retiring(key(tier, 0, -100.0), true) > key((tier - 1) | STRUCK, 10, 899.0), "tier {tier}");
            }
        }
    }
}
