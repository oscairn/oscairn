//! One compiled plan's note state (Graph::process's note work, core/engine/src/graph.cpp): each
//! step's events merged in time order and routed with the stolen notes' fades among them; after its
//! plugin rendered, its notes counted and costed and its NOTE_ENDs taken; per block, the budget's
//! lanes. Everything is sized when the plan is built: a block allocates nothing.

use crate::bindings::oscairn_voice_budget_importance_t as Listed;
use crate::budget::{self, Activity, Budget, Entry, Heard};
use crate::event::*;
use crate::route::{self, Context, Zones, UNTAGGED};
use crate::{Route, Routes, Shared};
use std::sync::atomic::Ordering::Relaxed;

const LOAD_MARGIN: f32 = 0.8; // the limit's share of a lane's time, for the wake-ups, copies and the machine's own

/// A note connection into a step.
pub struct Source {
    pub route: Route,
    pub zones: Zones,
    pub peers: Vec<Route>,
    pub channel: i16,
    pub draining: bool,
    pub from: i32, // the step its events come from, -1 the graph's input
}

pub struct SoundContext {
    pub channel: i16,
    pub activity: Shared<Activity>,
    pub id: u32, // its node
}

pub struct Step {
    pub activity: Option<Shared<Activity>>,
    pub sources: Vec<Source>,
    pub contexts: Vec<SoundContext>,
    pub lane: usize,
    pub in_process: bool,
    pub late_lane: bool,
    pub budgeted: bool,
    /// The later steps its audio goes to, each with the largest gain on the way; and those it comes from.
    pub next: Vec<(usize, f32)>,
    prev: Vec<(usize, f32)>,
    recent: [f32; 5], // its last blocks' times, for the median the lane's limit is held against
    recent_at: usize,
    recent_n: usize,
    limit_before: u32, // the voice limit in force a block earlier, which it has had a block and a fade to keep to
    live_before: u32,  // the notes live a block earlier: those of the block a late lane's plugin renders now
    cost: f32,         // its last block's usual time; an effect asleep, its cost awake
    awake: f32,        // an effect's usual time when it last ran awake
}

impl Step {
    pub fn new(
        activity: Option<Shared<Activity>>,
        sources: Vec<Source>,
        contexts: Vec<SoundContext>,
        lane: usize,
        in_process: bool,
        late_lane: bool,
        next: Vec<(usize, f32)>,
    ) -> Step {
        Step {
            activity,
            sources,
            contexts,
            lane,
            in_process,
            late_lane,
            budgeted: false,
            next,
            prev: Vec::new(),
            recent: [0.0; 5],
            recent_at: 0,
            recent_n: 0,
            limit_before: 0,
            live_before: 0,
            cost: 0.0,
            awake: 0.0,
        }
    }
    /// Its usual block, the median of its last five: a block held up from outside moves nothing, two in
    /// a row neither, while three of a real overload do.
    fn typical(&mut self, took: f32) -> f32 {
        self.recent[self.recent_at % 5] = took;
        self.recent_at = (self.recent_at + 1) % 5;
        self.recent_n = (self.recent_n + 1).min(5);
        let mut v = self.recent;
        let n = self.recent_n;
        v[..n].sort_unstable_by(f32::total_cmp);
        v[n / 2]
    }
}

/// A step's event and where it goes (oscairn_key).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Key {
    pub time: u32,
    pub order: u32,
    pub event: Shared<Event>,
    pub source: i16,
    pub tag: u8,
    pub after_notes: bool,
}

/// One block's timing (oscairn_block).
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Timing {
    pub budget: f32,
    pub period: f32,
    pub deadline: f32,
    pub allowance: f32,
    pub cores: f32,
    pub quiet_after: u64,
    pub low_slack: f32,
}

/// oscairn_voice_budget_note_t (oscairn-clap/voice-budget.h).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct VoiceNote {
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub voices: u32,
    pub loudness: f32,
}

/// What a step's plugin made of its block (oscairn_finish), its arrays as slices.
pub struct Rendered<'a> {
    pub took: f32,
    pub processed: bool,
    pub reset: bool,
    pub missed: bool,
    pub own_voices: bool,
    pub honors_volume: bool,
    pub voices: u32,
    pub voice_cost: f64,
    pub notes: &'a [VoiceNote],
    pub sent: &'a [Event],
    pub importance: &'a [Listed],
    /// The engine's sound ID of each of the step's contexts, by index (an importance report names its lists by them).
    pub contexts: &'a [u32],
    /// Its worker thread's CPU time in the block and its wait to be woken for it, microseconds; 0 or below unknown.
    pub cpu: f32,
    pub wait: f32,
}

pub struct Notes {
    pub steps: Vec<Step>,
    pub budget: Budget,
    timing: Timing,
    budgeting: bool,
    total: f32,
    reach: Vec<f32>, // scratch, per step: the gain from it to a step whose block was lost (mark_lost)
    after: Vec<f32>, // scratch, per step: the costliest way from it to the output (begin)
    slack: u8,       // the last four blocks, a bit each set where the callback's slack ran low or a block was lost
    lost_now: bool,  // a step's block lost in this block
    worst_used: f32, // the callback's recent worst time to its deadline less slack: the largest, falling 1 % a block; -1 unmeasured
}

/// Where a step's merged events go, with the channel the current connection's events take.
struct Merged<'a> {
    out: &'a mut [Event],
    heard: Option<&'a mut [i16]>,
    n: usize,
    channel: i16,
}

impl Merged<'_> {
    fn push(&mut self, e: &Event) -> bool {
        let Some(slot) = self.out.get_mut(self.n) else { return false };
        *slot = e.on_channel(self.channel);
        if let Some(h) = self.heard.as_deref_mut().and_then(|h| h.get_mut(self.n)) {
            *h = if self.channel < 0 { -1 } else { e.channel() };
        }
        self.n += 1;
        true
    }
}

impl Notes {
    /// The plan's note state; the budget's entries: every note connection, grouped by part (Graph::compile):
    /// a MIDI-dialect plugin's notes too, which a note-off ends.
    pub fn new(mut steps: Vec<Step>, lanes: usize) -> Notes {
        let mut entries: Vec<Entry> = Vec::new();
        for (index, step) in steps.iter_mut().enumerate() {
            let Some(activity) = step.activity else { continue };
            for source in &step.sources {
                let zone = source.zones.first();
                let part = zone.map_or(0, |z| z.part);
                let group = entries.iter().find(|e| part != 0 && e.part == part && e.draining == source.draining).map_or(entries.len(), |e| e.group);
                let rest = !entries.iter().any(|e| std::ptr::eq(&*e.cost, &*activity));
                entries.push(Entry {
                    route: source.route,
                    step: index,
                    channel: source.channel,
                    cost: activity,
                    group,
                    rest,
                    draining: source.draining,
                    lane: step.lane,
                    part,
                    reserve: zone.map_or(0, |z| z.reserve),
                    priority: zone.map_or(0, |z| z.priority),
                    cap: 0,
                    ceiling: 0,
                    calm: 0,
                    probes: 0,
                    cost_at: 0.0,
                    heard: None,
                });
                step.budgeted = true;
            }
        }
        for s in 0..steps.len() {
            for (t, gain) in steps[s].next.clone() {
                if let Some(next) = steps.get_mut(t) {
                    next.prev.push((s, gain));
                }
            }
        }
        let n = steps.len();
        let mut budget = Budget::new(entries, lanes);
        budget.gains = vec![1.0; n];
        Notes {
            budget,
            steps,
            timing: Timing::default(),
            budgeting: false,
            total: 0.0,
            reach: vec![0.0; n],
            after: vec![0.0; n],
            slack: 0,
            lost_now: false,
            worst_used: -1.0,
        }
    }

    /// A block starts: each lane's limit, the smaller of the budget and LOAD_MARGIN of the time its
    /// blocks have before they are missed (a period for a lane whose plugins all render a block late,
    /// the callback's for the engine's process, else a worker's allowance); an instrument's also leaves
    /// room before the callback's deadline for the costliest way after it to the output (its inserts,
    /// the buses it sends to and theirs, at their last blocks' times), though never less than a quarter.
    pub fn begin(&mut self, t: Timing) {
        self.timing = t;
        self.budgeting = t.budget != 0.0;
        if !self.budgeting {
            return;
        }
        self.total = (if t.budget > 0.0 { t.budget } else { LOAD_MARGIN * t.period }) * t.cores;
        for lane in &mut self.budget.lanes {
            (lane.limit, lane.miss, lane.took, lane.others_took) = (if t.budget > 0.0 { t.budget } else { f32::INFINITY }, f32::INFINITY, 0.0, 0.0);
            (lane.run, lane.cpu, lane.timed, lane.wait, lane.lost_own) = (0.0, 0.0, false, 0.0, false);
        }
        for s in (0..self.steps.len()).rev() {
            let Some(step) = self.steps.get(s) else { continue };
            let way = step.next.iter().map(|&(n, _)| self.steps.get(n).map_or(0.0, |x| x.cost) + self.after.get(n).copied().unwrap_or(0.0)).fold(0.0, f32::max);
            if let Some(after) = self.after.get_mut(s) {
                *after = way;
            }
        }
        for (step, after) in self.steps.iter().zip(&self.after) {
            let Some(lane) = self.budget.lanes.get_mut(step.lane) else { continue };
            let time = if step.late_lane {
                t.period
            } else if step.in_process {
                t.deadline
            } else {
                t.allowance
            };
            let own = LOAD_MARGIN * time;
            let chained = if step.budgeted && !step.late_lane { (LOAD_MARGIN * (t.deadline - after)).max(own / 4.0) } else { own };
            lane.miss = lane.miss.min(time);
            lane.limit = lane.limit.min(own.min(chained));
        }
    }

    /// One step's events, sorted and routed into `out` (see notes_route in engine_core.h).
    #[allow(clippy::too_many_arguments)]
    pub fn route(
        &mut self,
        rs: &mut Routes,
        s: usize,
        keys: &mut [Key],
        frames: u32,
        next_id: &mut u32,
        out: &mut [Event],
        heard: Option<&mut [i16]>,
        mut overflow: bool,
    ) -> (usize, bool) {
        let Notes { steps, budget, timing, .. } = self;
        if frames != 0 && timing.period > 0.0 {
            budget.frame_us = timing.period / frames as f32;
        }
        let Some(step) = steps.get(s) else { return (0, overflow) };
        let mut m = Merged { out, heard, n: 0, channel: -1 };
        keys.sort_unstable_by_key(|k| (k.time, k.after_notes, k.order));
        // Stolen notes' fade steps go in among the events, at their times.
        let fades = |rs: &mut Routes, m: &mut Merged, until: u32| {
            let mut ok = true;
            for source in &step.sources {
                m.channel = source.channel;
                ok = rs[source.route].fade(until, &mut |e| m.push(e)) && ok;
            }
            m.channel = -1;
            ok
        };
        if !overflow {
            for k in keys.iter() {
                if !fades(rs, &mut m, k.time.min(frames)) {
                    overflow = true;
                    break;
                }
                let source = usize::try_from(k.source).ok().and_then(|i| step.sources.get(i));
                m.channel = source.filter(|_| !k.after_notes).map_or(-1, |s| s.channel);
                let sent = match source {
                    Some(source) if k.time < frames => {
                        let mut cx = if k.after_notes {
                            Context { zones: None, source: UNTAGGED, draining: false, peers: &[], budget: None }
                        } else {
                            Context { zones: Some(&source.zones), source: k.tag, draining: source.draining, peers: &source.peers, budget: Some(&mut *budget) }
                        };
                        route::route(rs, source.route, &k.event, next_id, &mut |e| m.push(e), &mut cx)
                    }
                    _ => m.push(&k.event),
                };
                m.channel = -1;
                if !sent {
                    overflow = true;
                    break;
                }
            }
        }
        if !overflow && !fades(rs, &mut m, frames) {
            overflow = true;
        }
        for source in &step.sources {
            rs[source.route].advance(frames);
        }
        if overflow {
            for source in &step.sources {
                rs[source.route].clear(); // a lost release: the host resets the plugin in its own dialect
            }
            return (0, true);
        }
        (m.n, false)
    }

    /// After a step's plugin rendered (Graph::process's finish): routes emptied if its notes are gone,
    /// the notes sounding through it counted, its cost measured, a lost block marked on its lane and
    /// those before it, its NOTE_ENDs taken, and released notes its plugin never ended let go once it
    /// has been silent a while.
    pub fn finish(&mut self, rs: &mut Routes, s: usize, r: &Rendered) {
        if r.missed && self.budgeting {
            self.mark_lost(s);
        }
        let Notes { steps, budget, timing, budgeting, .. } = self;
        let Some(step) = steps.get_mut(s) else { return };
        if r.reset {
            for source in &step.sources {
                rs[source.route].clear();
            }
        }
        let sounding =
            |channel: Option<i16>| step.sources.iter().filter(|s| channel.is_none_or(|c| c == s.channel)).map(|s| rs[s.route].sounding()).sum::<u32>();
        for c in &step.contexts {
            c.activity.sounding.store(sounding(Some(c.channel)), Relaxed);
        }
        if let Some(a) = &step.activity {
            a.sounding.store(sounding(None), Relaxed);
        }
        if *budgeting {
            let usual = step.typical(r.took);
            // An effect asleep on silence counts at its cost awake (one never awake at its learned cost, its seed),
            // so the first note into its chain is costed before it wakes.
            step.cost = usual;
            if let (false, Some(a)) = (step.budgeted, &step.activity) {
                if !a.asleep.load(Relaxed) {
                    step.awake = usual;
                }
                step.cost = usual.max(if step.awake > 0.0 { step.awake } else { a.rest_now() });
            }
            if let Some(lane) = budget.lanes.get_mut(step.lane) {
                lane.took += usual;
                if !step.budgeted {
                    lane.others_took += step.cost;
                }
                lane.run += r.took;
                lane.cpu += if r.cpu > 0.0 { r.cpu } else { r.took };
                lane.timed |= r.cpu > 0.0;
                // The thread's wait to be woken: its first block's (in a pool or a chain each later plugin's wait
                // includes the blocks rendered before it on the same thread).
                if r.wait > 0.0 {
                    lane.wait = if lane.wait > 0.0 { lane.wait.min(r.wait) } else { r.wait };
                }
            }
            if let (true, Some(a)) = (step.budgeted, &step.activity) {
                let live: u32 = step.sources.iter().map(|s| rs[s.route].live()).sum();
                // RFC 0045: a plugin a block late took this block's time on the block handed a call earlier, so its cost
                // per note is measured against the notes live then; counted with this block's, a first note costs nothing.
                let rendered = if step.late_lane { std::mem::replace(&mut step.live_before, live) } else { live };
                // A plugin that reports no voices while notes live, or whose block was not rendered, is counted by
                // its notes; one giving up its own voices is judged against its limit so even when its blocks are lost.
                let voices = if r.voices == 0 || !r.processed { live } else { r.voices };
                let limit = std::mem::replace(&mut step.limit_before, a.voice_limit.load(Relaxed));
                a.over_limit.store(if limit != 0 && voices > limit { a.over_limit.load(Relaxed) + 1 } else { 0 }, Relaxed);
                if r.processed {
                    a.own_voices.store(r.own_voices, Relaxed);
                    a.honors_volume.store(r.honors_volume, Relaxed);
                    // A block past twice both the usual and what its notes are estimated to cost now is held
                    // up from outside (new notes raise the estimate with them): the larger is taken.
                    let expected = usual.max(a.rest_now() + a.note_now() / a.per_note_scale.get() * (if r.own_voices { voices } else { rendered }) as f32);
                    let took = if a.per_note.get() >= 0.0 && r.took > 2.0 * expected { expected } else { r.took };
                    if !r.own_voices {
                        a.observe(took, rendered);
                    } else {
                        // Its voices, each costed alike, a note at its share of them, and each note's loudness
                        // in place of its velocity.
                        if r.voice_cost > 0.0 && a.per_note.get() < 0.0 {
                            a.seed_note.set(r.voice_cost as f32);
                        }
                        a.observe(took, voices);
                        a.per_note_scale.set(if live != 0 { 1f32.max(voices as f32 / live as f32) } else { 1.0 });
                        for n in r.notes {
                            for source in &step.sources {
                                rs[source.route].set_loudness(n.note_id, n.channel, n.key, n.loudness, source.channel);
                            }
                        }
                        hear(budget, s, step, r);
                    }
                }
            }
        }
        for e in r.sent {
            for source in &step.sources {
                if let Some(e) = e.ended_on(source.channel) {
                    rs[source.route].ended(&e);
                }
            }
        }
        // Silent for noteEndQuiet (a shared engine: its context's bus), its released notes no plugin ended go.
        for source in &step.sources {
            let a = step.activity.as_deref().or_else(|| step.contexts.iter().find(|c| c.channel == source.channel).map(|c| &*c.activity));
            if rs[source.route].used.any() && a.is_some_and(|a| a.quiet.load(Relaxed) >= timing.quiet_after) {
                rs[source.route].expire();
            }
        }
    }

    /// Whether the budget runs this block.
    pub fn budgeting(&self) -> bool {
        self.budgeting
    }
    /// How many sound contexts step `s` plays (a shared instance's parts).
    pub fn contexts(&self, s: usize) -> usize {
        self.steps.get(s).map_or(0, |x| x.contexts.len())
    }

    /// See notes_voice_limit (engine_core.h): the instance's limit and each sound context's, a context's the
    /// voices of its admitted notes and its share of the instance's room (or of its excess, over it), 0 for a
    /// context holding none (no limit of its own).
    pub fn voice_limit(&self, rs: &Routes, s: usize, voices: u32, contexts: &mut [u32]) -> u32 {
        let Some((step, a)) = self.steps.get(s).and_then(|st| Some((st, st.activity.as_deref()?))) else { return 0 };
        let scale = a.per_note_scale.get();
        let admitted = |channel: Option<i16>| {
            let notes: u32 =
                step.sources.iter().filter(|x| channel.is_none_or(|c| c == x.channel)).map(|x| rs[x.route].used.and_not(rs[x.route].stolen).count()).sum();
            (notes as f32 * scale).ceil()
        };
        let all = admitted(None);
        let mut limit = self.budget.voice_limit(rs, step.lane, a, voices, all);
        // An instance one part plays alone that lists its voices: the cap the budget set on its part, on the instance.
        if let Some(cap) = step.contexts.is_empty().then(|| self.budget.step_cap(s)).flatten().filter(|&c| c != 0) {
            limit = if limit == 0 { cap } else { limit.min(cap) };
        }
        for (out, c) in contexts.iter_mut().zip(&step.contexts) {
            // A context listing its voices keeps to the cap the budget set on it, by importance (RFC 0046); one that does
            // not, its share of the instance's limit by its notes.
            if let Some(cap) = self.budget.context_cap(s, c.channel) {
                *out = cap;
                continue;
            }
            let own = admitted(Some(c.channel));
            *out = if limit == 0 || own == 0.0 { 0 } else { 1f32.max(own + ((limit as f32 - all) * own / all).floor()) as u32 };
        }
        limit
    }

    /// For the deadline: the lane furthest over its time this block (its time against its limit), and every lane
    /// whose audio reaches it, whose voices free its time too (RFC 0046: only where taking them frees what is short).
    fn mark_slowest(&mut self) {
        self.budget.marked.fill(false);
        let lanes = &self.budget.lanes;
        let over = |l: &crate::budget::Lane| if l.limit > 0.0 && l.limit.is_finite() { l.took / l.limit } else { 0.0 };
        let Some(slowest) = (0..lanes.len()).max_by(|&a, &b| over(&lanes[a]).total_cmp(&over(&lanes[b]))) else { return };
        self.reach.fill(0.0);
        for i in (0..self.steps.len()).rev() {
            let Some(step) = self.steps.get(i) else { continue };
            let reaches = step.lane == slowest || step.next.iter().any(|&(n, _)| self.reach.get(n).is_some_and(|&r| r > 0.0));
            if reaches {
                if let Some(r) = self.reach.get_mut(i) {
                    *r = 1.0;
                }
                if let Some(m) = self.budget.marked.get_mut(step.lane) {
                    *m = true;
                }
            }
        }
    }

    /// A lost block on step `s`: its lane is marked, and every lane before it on the way, by the largest
    /// gain from there (an insert's instrument, a bus's senders at their send levels).
    fn mark_lost(&mut self, s: usize) {
        self.lost_now = true;
        if let Some(a) = self.steps.get(s).and_then(|x| x.activity) {
            a.lost.fetch_add(1, Relaxed);
        }
        if let Some(lane) = self.steps.get(s).and_then(|x| self.budget.lanes.get_mut(x.lane)) {
            lane.lost_own = true;
        }
        self.reach.fill(0.0);
        for i in (0..=s).rev() {
            let Some(step) = self.steps.get(i) else { continue };
            let gain = step.next.iter().map(|&(n, g)| self.reach.get(n).map_or(0.0, |r| r * g.abs())).fold(if i == s { 1.0 } else { 0.0 }, f32::max);
            if let Some(r) = self.reach.get_mut(i) {
                *r = gain;
            }
            if let (true, Some(lane)) = (gain > 0.0, self.budget.lanes.get_mut(step.lane)) {
                lane.losses |= 1;
                lane.weight = lane.weight.max(gain);
            }
        }
    }

    /// The block is done: each lane judged against its limit, the lanes together against the total; then
    /// lost blocks, and the callback's slack (microseconds left before its deadline once the block was
    /// collected; NaN unknown) at or under the timing's low_slack in two of the last four blocks, this one
    /// among them, each give a note up; each node's lane's limit and time kept for the timing report.
    /// low_slack is 0 unless a profile says otherwise: the callback waits for its workers up to the deadline,
    /// so a heavy but healthy block leaves little slack, and none is left only where a wait was given up.
    pub fn end(&mut self, rs: &mut Routes, slack: f32) {
        if !self.budgeting {
            return;
        }
        for i in 0..self.budget.lanes.len() {
            let Some(lane) = self.budget.lanes.get_mut(i) else { continue };
            lane.others = lane.others_took;
            let took = lane.took;
            lane.worst = took.max(lane.worst * 0.99);
            lane.judge_held();
            if lane.held {
                for a in self.steps.iter().filter(|s| s.lane == i && s.budgeted).filter_map(|s| s.activity) {
                    a.held_off.fetch_add(1, Relaxed);
                }
            }
            self.budget.measured(rs, i, took);
        }
        self.budget.device(rs, self.total);
        self.budget.lost(rs);
        for lane in &mut self.budget.lanes {
            lane.losses <<= 1;
            if lane.losses == 0 {
                lane.weight = 0.0;
            }
        }
        // A block lost anywhere counts as the callback's time run out: with waits cut wave by wave the
        // callback ends before its deadline, its slack never at 0.
        if slack.is_finite() || self.lost_now {
            self.slack = (self.slack << 1 | u8::from(self.lost_now || slack <= self.timing.low_slack)) & 0xf;
            if self.slack.count_ones() >= 2 && self.slack & 1 != 0 {
                self.mark_slowest();
                self.budget.deadline(rs);
            }
        }
        let low_now = self.lost_now || slack <= self.timing.low_slack;
        self.lost_now = false;
        if slack.is_finite() {
            self.worst_used = (self.timing.deadline - slack).max(self.worst_used * 0.99);
        }
        let room = (self.worst_used >= 0.0).then(|| self.timing.deadline - self.worst_used);
        self.budget.recover(rs, low_now, room, self.timing.deadline);
        for step in &self.steps {
            if let (Some(a), Some(lane)) = (&step.activity, self.budget.lanes.get(step.lane)) {
                a.lane_limit.set(lane.limit);
                a.lane_took.set(lane.took);
            }
        }
    }
}

/// Step `s`'s importance report (oscairn.voice-budget draft 2) onto the budget's entries: each list on the first entry of
/// the context it names (an instance one part plays alone: every list, merged, on its first entry), each voice by its
/// tier and audibility before the part's gain; and the check that a context kept to the cap the budget set on it, two
/// reports on (`importance_mismatch`).
fn hear(budget: &mut Budget, s: usize, step: &Step, r: &Rendered) {
    let Some(a) = step.activity.as_deref() else { return };
    // A weight of 1 costs the instance's voices over their weights in its voices (one cost per voice measured for all).
    let weight = |w: f32, none: f32| if w.is_finite() && w > 0.0 { w.min(1e6) } else { none };
    let (voices, weights) = r.importance.iter().fold((0f32, 0f32), |(v, w), l| (v + f32::from(l.voices), w + weight(l.weight, f32::from(l.voices))));
    let unit = if weights > 0.0 { voices / weights } else { 1.0 };
    let mut seen = 0u32; // the channels (bit 16: none) whose first entry took its lists
    for e in budget.entries.iter_mut().filter(|e| e.step == s) {
        let (channel, bit) = (e.channel, 1u32 << (e.channel.clamp(-1, 15) + 1));
        let first = step.contexts.is_empty() && seen == 0 || !step.contexts.is_empty() && seen & bit == 0;
        seen |= if step.contexts.is_empty() { 1 } else { bit };
        let mine = |l: &&Listed| step.contexts.is_empty() || step.contexts.iter().zip(r.contexts).any(|(c, &id)| c.channel == channel && id == l.context);
        let mut lists = r.importance.iter().filter(mine).peekable();
        if !first || lists.peek().is_none() {
            e.heard = None;
            continue;
        }
        let mut h = Heard { unit, ..Heard::default() };
        for l in lists {
            h.voices += u32::from(l.voices);
            for v in l.least.iter().take(usize::from(l.count).min(budget::LEAST)) {
                let struck = if v.flags & 1 != 0 { budget::STRUCK } else { 0 }; // OSCAIRN_VOICE_BUDGET_STRUCK
                let level = if v.level_db.is_finite() { v.level_db.clamp(-120.0, 24.0) } else { -120.0 };
                let x = (v.tier.min(budget::KEY_HELD) | struck, budget::audibility(level, f32::from(v.age_ms)), weight(v.weight, 1.0));
                // Kept ascending: the least important LEAST of every list.
                // In the plugin's order: the tier, then one just struck after the others, then audibility.
                let rank = |t: u8| (t & !budget::STRUCK) << 1 | u8::from(t & budget::STRUCK != 0);
                let at = h.least[..h.count].partition_point(|y| rank(y.0) < rank(x.0) || rank(y.0) == rank(x.0) && y.1 <= x.1);
                if at < budget::LEAST {
                    h.count = (h.count + 1).min(budget::LEAST);
                    h.least.copy_within(at..h.count - 1, at + 1);
                    h.least[at] = x;
                }
            }
        }
        if let Some(was) = e.heard {
            (h.checked, h.check_in) = (was.checked, was.check_in.saturating_sub(1));
            if h.checked != 0 && h.check_in == 0 {
                if e.cap != 0 && h.voices > e.cap {
                    a.importance_mismatch.fetch_add(1, Relaxed);
                }
                h.checked = 0;
            }
        }
        e.heard = Some(h);
    }
}
