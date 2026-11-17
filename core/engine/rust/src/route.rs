//! NoteRoute (core/engine/src/NoteRoute.h): one connection's ledger of the notes it passed on, with
//! the same semantics. Occupancy and state are 512-bit maps, so counts are popcounts and scans visit
//! only the slots in use, in ascending order as the C++ loops do. Routes are reached through
//! `Routes` by handle, one at a time, so a note limit's peers and the budget hold no references.

use crate::budget::Budget;
use crate::event::*;
use crate::{Route, Routes};

pub const SLOTS: usize = 512; // Host::maxBlockEvents
const WORDS: usize = SLOTS / 64;
pub const FADE_STEPS: u8 = 8;
pub const UNTAGGED: u8 = 0xff;
const MAX_SOURCES: u8 = 64;

/// A set of slots, one bit each, iterated in ascending order.
#[derive(Clone, Copy, Default)]
pub struct Bits([u64; WORDS]);

impl Bits {
    pub fn has(&self, i: usize) -> bool {
        self.0[i / 64 % WORDS] >> (i % 64) & 1 != 0
    }
    fn set(&mut self, i: usize) {
        self.0[i / 64 % WORDS] |= 1 << (i % 64)
    }
    fn unset(&mut self, i: usize) {
        self.0[i / 64 % WORDS] &= !(1 << (i % 64))
    }
    pub fn count(&self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }
    pub fn any(&self) -> bool {
        self.0.iter().any(|&w| w != 0)
    }
    fn full(&self) -> bool {
        self.0.iter().all(|&w| w == !0)
    }
    fn first_free(&self) -> Option<usize> {
        let (i, w) = self.0.iter().enumerate().find(|(_, &w)| w != !0)?;
        Some(i * 64 + w.trailing_ones() as usize)
    }
    pub fn and(self, o: Bits) -> Bits {
        Bits(std::array::from_fn(|i| self.0[i] & o.0[i]))
    }
    pub fn and_not(self, o: Bits) -> Bits {
        Bits(std::array::from_fn(|i| self.0[i] & !o.0[i]))
    }
    pub fn iter(self) -> impl Iterator<Item = usize> {
        self.0.into_iter().enumerate().flat_map(|(i, mut w)| {
            std::iter::from_fn(move || {
                (w != 0).then(|| {
                    let b = w.trailing_zeros() as usize;
                    w &= w - 1;
                    i * 64 + b
                })
            })
        })
    }
}

/// A note passed on: its address as its source sent it (which releases and expressions match), the
/// key it plays here, its zone (-1 for none), where it entered the graph, its fade, and its loudness
/// for the budget (its velocity, or the plugin's report).
#[derive(Clone, Copy, Default)]
pub struct Slot {
    pub id: i32,
    note_id: i32,
    flags: u32, // the note-on's header flags, which its release keeps
    pub fade_at: i32,
    pub velocity: f32,
    pub key: i16,
    address_key: i16,
    channel: i16,
    port: u8,
    zone: i8,
    source: u8,
    pub faded: u8,
    midi: bool,
    voice_ended: bool,
    born: u64, // the route's frames at its note-on, for its age
}

/// NoteZone (note_zone.h).
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Zone {
    low_key: u8,
    high_key: u8,
    low_velocity: f64,
    high_velocity: f64,
    channels: u16,
    transpose: i8,
    curve: f64,
    fixed_velocity: f64,
    sources: u64,
    pub max_notes: u16,
    pub part: u32,
    pub reserve: u16,
    pub priority: u8,
    pedals: u8, // whose pedals the part takes: two bits per class, PedalReceive (All 0, None 1, Own 2)
}

impl Zone {
    fn hears(&self, source: u8) -> bool {
        source >= MAX_SOURCES || self.sources >> source & 1 != 0
    }
    fn accepts(&self, key: i16, velocity: f64, channel: i16, source: u8) -> bool {
        key >= i16::from(self.low_key)
            && key <= i16::from(self.high_key)
            && velocity >= self.low_velocity
            && velocity <= self.high_velocity
            && (!(0..=15).contains(&channel) || u32::from(self.channels) & (1 << channel) != 0)
            && self.hears(source)
    }
    fn shape(&self, velocity: f64) -> f64 {
        if self.fixed_velocity > 0.0 {
            self.fixed_velocity
        } else {
            velocity.powf(self.curve)
        }
    }
}

/// NoteZones (note_zone.h): at most eight; none, every note passes unchanged.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Zones {
    zones: [Zone; 8],
    count: usize,
}

impl Zones {
    pub fn all(&self) -> &[Zone] {
        &self.zones[..self.count.min(8)]
    }
    pub fn first(&self) -> Option<&Zone> {
        self.all().first()
    }
    /// NoteZones::takesPedal: the first zone's receive for pedal class `class` (sustain, sostenuto, soft).
    fn takes_pedal(&self, class: usize, source: u8) -> bool {
        match self.first().map_or(0, |z| z.pedals >> (2 * class) & 3) {
            1 => false,
            2 => self.all().iter().any(|z| z.hears(source)),
            _ => true,
        }
    }
    /// Whether the receive for pedal class `class` takes each source's (its table slot, the last for untagged).
    fn pedal_from(&self, class: usize, slot: usize) -> bool {
        self.takes_pedal(class, u8::try_from(slot).unwrap_or(UNTAGGED))
    }
    fn takes_channel(&self, channel: u8) -> bool {
        self.all().iter().any(|z| u32::from(z.channels) & (1 << channel) != 0)
    }
    fn passes(&self, channel: u8, source: u8) -> bool {
        self.all().iter().any(|z| u32::from(z.channels) & (1 << channel) != 0 && z.hears(source))
    }
}

pub struct NoteRoute {
    slots: [Slot; SLOTS],
    volume: [f64; SLOTS], // each note's last volume expression, read only by a fade
    pub used: Bits,
    pub held: Bits,
    pub stolen: Bits,
    pub fading: Bits, // used, with a fade step or choke due (fade_at >= 0)
    pub pedal: [[u8; 16]; 16],
    clap: [bool; 16],
    ports: u32,
    fade_frames: u32,
    pub held_keys: [u64; 2],
    held_count: [u16; 128],
    pedals_by: [[[u8; MAX_SOURCES as usize + 1]; 16]; 3], // each pedal's last value from each source by channel (untagged last)
    pedals_sent: [[u8; 16]; 3], // each pedal's value last sent on by channel
    frames: u64,                // the frames it has passed, for its notes' ages
}

/// What a routed event may touch besides its route: its zones, where it entered the graph, whether
/// the connection drains, the note limit's peers (this route among them; none: it alone) and the
/// plan's shared budget.
pub struct Context<'a> {
    pub zones: Option<&'a Zones>,
    pub source: u8,
    pub draining: bool,
    pub peers: &'a [Route],
    pub budget: Option<&'a mut Budget>,
}

impl NoteRoute {
    pub fn new(ports: u32, clap_ports: u16, fade_frames: u32) -> NoteRoute {
        NoteRoute {
            slots: [Slot::default(); SLOTS],
            volume: [1.0; SLOTS],
            used: Bits::default(),
            held: Bits::default(),
            stolen: Bits::default(),
            fading: Bits::default(),
            pedal: [[0; 16]; 16],
            clap: std::array::from_fn(|p| clap_ports >> p & 1 != 0),
            ports,
            fade_frames,
            held_keys: [0; 2],
            held_count: [0; 128],
            pedals_by: [[[0; MAX_SOURCES as usize + 1]; 16]; 3],
            pedals_sent: [[0; 16]; 3],
            frames: 0,
        }
    }
    pub fn slot(&self, i: usize) -> &Slot {
        &self.slots[i % SLOTS]
    }
    fn pedal_of(&self, s: &Slot) -> u8 {
        self.pedal[usize::from(s.port & 15)][(s.channel & 15) as usize]
    }
    pub fn takes_clap(&self, s: &Slot) -> bool {
        self.clap[usize::from(s.port & 15)]
    }

    pub fn clear(&mut self) {
        self.used = Bits::default();
        self.held = Bits::default();
        self.stolen = Bits::default();
        self.fading = Bits::default();
        self.pedal = [[0; 16]; 16];
        self.held_keys = [0; 2];
        self.held_count = [0; 128];
        self.pedals_by = [[[0; MAX_SOURCES as usize + 1]; 16]; 3];
        self.pedals_sent = [[0; 16]; 3];
    }
    fn hold(&mut self, i: usize) {
        self.held.set(i);
        if let Ok(k @ 0..128) = usize::try_from(self.slot(i).key) {
            if self.held_count[k] == 0 {
                self.held_keys[k / 64] |= 1 << (k % 64);
            }
            self.held_count[k] += 1;
        }
    }
    fn lift(&mut self, i: usize) {
        if let (true, Ok(k @ 0..128)) = (self.used.has(i) && self.held.has(i), usize::try_from(self.slot(i).key)) {
            self.held_count[k] -= 1;
            if self.held_count[k] == 0 {
                self.held_keys[k / 64] &= !(1 << (k % 64));
            }
        }
        self.held.unset(i);
    }
    fn drop_slot(&mut self, i: usize) {
        self.lift(i);
        self.used.unset(i);
        self.stolen.unset(i);
        self.fading.unset(i);
    }
    fn set_fade(&mut self, i: usize, at: i32) {
        self.slots[i % SLOTS].fade_at = at;
        if at >= 0 {
            self.fading.set(i)
        } else {
            self.fading.unset(i)
        }
    }
    fn from(s: &Slot, source: u8) -> bool {
        source == UNTAGGED || s.source == source
    }
    fn matches(&self, i: usize, (id, port, channel, key): (i32, i16, i16, i16)) -> bool {
        let s = self.slot(i);
        self.used.has(i)
            && (id < 0 || id == s.note_id)
            && (port < 0 || port == i16::from(s.port))
            && (channel < 0 || channel == s.channel)
            && (key < 0 || key == s.address_key)
    }
    fn release(&self, i: usize, time: u32, choke: bool) -> Event {
        let s = self.slot(i);
        let mut e = Event::header(NOTE_SIZE, time, CORE_SPACE, if choke { NOTE_CHOKE } else { NOTE_OFF }, s.flags);
        e.set_address(NOTE_ID, if self.takes_clap(s) { s.id } else { -1 }, i16::from(s.port), s.channel, s.key);
        e
    }
    /// Notes still sounding by the player's hand: keys down, or released under the sustain pedal.
    pub fn sounding(&self) -> u32 {
        self.held.count() + self.used.and_not(self.held).iter().filter(|&i| self.pedal_of(self.slot(i)) >= 64).count() as u32
    }
    /// Released and held only by the sustain pedal.
    pub fn pedaled(&self, i: usize) -> bool {
        !self.held.has(i) && self.pedal_of(self.slot(i)) >= 64
    }
    /// Note `i`'s age in frames.
    pub fn age(&self, i: usize) -> u64 {
        self.frames.saturating_sub(self.slot(i).born)
    }
    /// The notes it holds until their plugin ends them, released and fading ones included.
    pub fn live(&self) -> u32 {
        self.used.count()
    }

    /// A NOTE_END: a voice ended. A held key keeps its slot, so its release still goes out.
    pub fn ended(&mut self, e: &Event) {
        if e.space() != CORE_SPACE || e.kind() != NOTE_END || e.size() != NOTE_SIZE {
            return;
        }
        let (id, port, channel, key) = e.address(NOTE_ID);
        for i in self.used.iter() {
            let s = self.slot(i);
            if (id < 0 || s.id == id) && (port < 0 || i16::from(s.port) == port) && (channel < 0 || s.channel == channel) && (key < 0 || s.key == key) {
                if self.held.has(i) {
                    self.slots[i % SLOTS].voice_ended = true
                } else {
                    self.drop_slot(i)
                }
            }
        }
    }
    /// A stolen note's fade (RFC 0018): volume steps over fade_frames, then a choke; the steps due
    /// before `until`, in time order across notes.
    pub fn fade(&mut self, until: u32, emit: &mut impl FnMut(&Event) -> bool) -> bool {
        let mut ok = true;
        // The earliest due, the lowest slot on a tie.
        while let Some(i) = self.fading.iter().min_by_key(|&i| self.slot(i).fade_at) {
            let s = *self.slot(i);
            let at = s.fade_at as u32;
            if at >= until {
                break;
            }
            if s.faded == FADE_STEPS {
                self.set_fade(i, -1);
                ok = emit(&self.release(i, at, true)) && ok;
                // A MIDI note's choke goes out as its note-off, its end here unless the pedal holds it.
                if !self.takes_clap(&s) && self.pedal_of(&s) < 64 {
                    self.drop_slot(i);
                }
                continue;
            }
            let mut v = Event::header(NOTE_SIZE, at, CORE_SPACE, NOTE_EXPRESSION, 0);
            v.set_i32(EXPR_ID, VOLUME);
            v.set_address(EXPR_NOTE, s.id, i16::from(s.port), s.channel, s.key);
            v.set_f64(EXPR_VALUE, self.volume[i % SLOTS] * f64::from(FADE_STEPS - 1 - s.faded) / f64::from(FADE_STEPS));
            let slot = &mut self.slots[i % SLOTS];
            slot.faded += 1;
            slot.fade_at += (self.fade_frames / u32::from(FADE_STEPS)) as i32;
            ok = emit(&v) && ok;
        }
        ok
    }
    /// The note-end watchdog: released notes no plugin ended, once it has been silent a while; a
    /// key down, a pedal holding it or a fade under way keeps its slot.
    pub fn expire(&mut self) {
        for i in self.used.and_not(self.held).and_not(self.fading).iter() {
            let s = self.slot(i);
            if self.takes_clap(s) && self.pedal_of(s) < 64 {
                self.drop_slot(i);
            }
        }
    }
    /// Chokes a note at `at` without (more of) a fade, at the budget's hard limit.
    pub fn choke(&mut self, i: usize, at: u32) {
        if self.stolen.has(i) && self.fading.has(i) {
            let f = self.slot(i).fade_at.min(at as i32);
            self.set_fade(i, f);
        } else {
            self.stolen.set(i);
            self.lift(i);
            self.set_fade(i, at as i32);
        }
        self.slots[i % SLOTS].faded = FADE_STEPS;
    }
    /// Gives a note up from elsewhere (a peer's note limit, the budget): it fades from `at`, or is choked
    /// there; a MIDI note, which no volume reaches, is released there.
    pub fn steal(&mut self, i: usize, at: u32) {
        self.stolen.set(i);
        self.lift(i);
        self.set_fade(i, at as i32);
        self.slots[i % SLOTS].faded = if self.fade_frames != 0 && self.takes_clap(self.slot(i)) { 0 } else { FADE_STEPS };
    }
    /// A block of `frames` done: fades carry on from the next block's start.
    pub fn advance(&mut self, frames: u32) {
        self.frames += u64::from(frames);
        for i in self.fading.iter() {
            let s = &mut self.slots[i % SLOTS];
            s.fade_at = (s.fade_at - frames as i32).max(0);
        }
    }
    /// The notes it can give up: not given up yet, into a port taking CLAP notes (which a choke reaches).
    pub fn playing(&self) -> impl Iterator<Item = usize> + '_ {
        self.used.and_not(self.stolen).iter().filter(|&i| self.takes_clap(self.slot(i)))
    }
    /// oscairn.voice-budget: a note's loudness as its plugin reports it, by its ID, else by its channel
    /// (`channel`, a shared engine context's, else the note's own) and key.
    pub fn set_loudness(&mut self, note_id: i32, channel: i16, key: i16, loudness: f32, context: i16) {
        for i in self.used.iter() {
            let s = &mut self.slots[i % SLOTS];
            let on = if context < 0 { s.channel } else { context };
            if if note_id >= 0 { s.id == note_id } else { on == channel && s.key == key } {
                s.velocity = loudness;
            }
        }
    }
    /// The connection leaves the plan: CLAP notes choked; MIDI cannot choke one pedal-held voice, so
    /// its channel is silenced rather than strand it; each pedal set to `remaining`'s; then empty.
    pub fn retire(&mut self, remaining: &[[u8; 16]; 16], emit: &mut impl FnMut(&Event) -> bool) -> bool {
        let mut ok = true;
        let mut panic = [[false; 16]; 16];
        for i in self.used.iter() {
            let s = *self.slot(i);
            if self.takes_clap(&s) {
                ok = emit(&self.release(i, 0, true)) && ok;
            } else {
                panic[usize::from(s.port & 15)][(s.channel & 15) as usize] = true;
            }
        }
        for (p, (panic, pedal)) in panic.iter().zip(&self.pedal).enumerate() {
            for (c, (&panic, &pedal)) in panic.iter().zip(pedal).enumerate() {
                if panic {
                    ok = emit(&Event::midi(0xb0 | c as u8, 120, 0, p as u16)) && ok;
                }
                if pedal != 0 {
                    ok = emit(&Event::midi(0xb0 | c as u8, 64, remaining[p][c], p as u16)) && ok;
                }
            }
        }
        self.clear();
        ok
    }
    /// A MIDI tap gone: a release for each note it holds, its pedals lifted, then empty.
    pub fn let_go(&mut self, emit: &mut impl FnMut(&Event) -> bool) {
        for i in self.used.iter() {
            emit(&self.release(i, 0, false));
        }
        for p in self.pedal {
            for (c, &value) in p.iter().enumerate() {
                if value >= 64 {
                    emit(&Event::midi(0xb0 | c as u8, 64, 0, 0));
                }
            }
        }
        self.clear();
    }
}

/// NoteRoute::route for route `me`: zones decide which notes pass and how they change; each zone that
/// accepts a note plays its own copy.
/// Events through a route's zones alone, no peers, no budget (engine_core.h's route_events): those in
/// the core space, NOTE_END too unless skip_ends; `held`, where long enough, gets the keys held after each.
#[allow(clippy::too_many_arguments)]
pub fn route_events(
    rs: &mut Routes,
    me: Route,
    zones: &Zones,
    events: &[Event],
    tags: &[u8],
    skip_ends: bool,
    next_id: &mut u32,
    emit: &mut impl FnMut(&Event) -> bool,
    held: &mut [[u64; 2]],
) {
    for (i, e) in events.iter().enumerate() {
        if e.space() != CORE_SPACE || (skip_ends && e.kind() == NOTE_END) {
            continue;
        }
        let mut cx = Context { zones: Some(zones), source: tags.get(i).copied().unwrap_or(UNTAGGED), draining: false, peers: &[], budget: None };
        route(rs, me, e, next_id, emit, &mut cx);
        if let Some(h) = held.get_mut(i) {
            *h = rs[me].held_keys;
        }
    }
}

pub fn route(rs: &mut Routes, me: Route, e: &Event, next_id: &mut u32, emit: &mut impl FnMut(&Event) -> bool, cx: &mut Context) -> bool {
    if !e.valid() || e.space() != CORE_SPACE {
        return emit(e);
    }
    let mut e = *e;
    let mut midi = false;
    if e.kind() == MIDI && e.size() == MIDI_SIZE {
        let (port, d) = e.midi_bytes();
        let r = &mut rs[me];
        if u32::from(port) >= r.ports || port >= 16 {
            return emit(&e);
        }
        let (kind, channel, time) = (d[0] & 0xf0, d[0] & 15, e.time());
        // Releases, chokes and a pedal-up pass any zone: they end only notes this route played.
        let release = kind == 0x80 || (kind == 0x90 && d[2] == 0) || (kind == 0xb0 && (d[1] == 120 || d[1] == 123 || (d[1] == 64 && d[2] < 64)));
        let pedal = match (kind, d[1]) {
            (0xb0, 64) => Some(0),
            (0xb0, 66) => Some(1),
            (0xb0, 67) => Some(2),
            _ => None,
        };
        let mut value = d[2];
        match (cx.zones.filter(|z| z.count != 0), pedal) {
            // A pedal reaches a part by its receive setting, whichever device its notes come from (a pedal-down
            // only on its zones' channels); it goes on as the largest of the sources taken, when that differs from
            // what went on last. A pedal-up from any source counts: a receive edited while a pedal is down lets go of
            // it with that pedal's release.
            (Some(z), Some(p)) => {
                if d[2] >= 64 && (!z.takes_pedal(p, cx.source) || !z.takes_channel(channel)) {
                    return true;
                }
                let by = &mut r.pedals_by[p][usize::from(channel)];
                by[usize::from(cx.source.min(MAX_SOURCES))] = d[2];
                value = by.iter().enumerate().filter(|&(s, _)| z.pedal_from(p, s)).map(|(_, &v)| v).max().unwrap_or(0);
                let sent = &mut r.pedals_sent[p][usize::from(channel)];
                if value == *sent {
                    return true;
                }
                *sent = value;
                e.0[20] = value;
            }
            (Some(z), None) if (0x80..0xf0).contains(&kind) && !release && !z.passes(channel, cx.source) => return true, // another zone's channel or device
            // No zones: it goes on as it came, kept for zones set later.
            (None, Some(p)) => {
                r.pedals_by[p][usize::from(channel)][usize::from(cx.source.min(MAX_SOURCES))] = d[2];
                r.pedals_sent[p][usize::from(channel)] = d[2];
            }
            _ => {}
        }
        let (port16, channel16) = (port as i16, i16::from(channel));
        if kind == 0x90 || kind == 0x80 {
            e = Event::note(kind == 0x90 && d[2] != 0, i16::from(d[1]), f64::from(d[2]) / 127.0, time, channel16, port16);
            midi = true;
        } else if kind == 0xb0 && d[1] == 64 {
            r.pedal[usize::from(port)][usize::from(channel)] = value;
            if !r.clap[usize::from(port)] && value < 64 {
                for i in r.used.and_not(r.held).iter() {
                    let s = r.slot(i);
                    if u16::from(s.port) == port && s.channel == channel16 {
                        r.drop_slot(i);
                    }
                }
            }
            return emit(&e);
        } else if kind == 0xb0 && (d[1] == 120 || d[1] == 123) {
            let mut n = Event::note(false, -1, 0.0, time, channel16, port16);
            if d[1] == 120 {
                n.set_kind(NOTE_CHOKE);
            }
            return route(rs, me, &n, next_id, emit, cx);
        } else if kind == 0xa0 || kind == 0xd0 {
            let (key, value) = if kind == 0xd0 { (-1, d[1]) } else { (i16::from(d[1]), d[2]) };
            return route(rs, me, &Event::pressure(time, port16, channel16, key, value), next_id, emit, cx);
        } else {
            return emit(&e);
        }
    }
    let kind = e.kind();
    match kind {
        NOTE_ON if e.size() == NOTE_SIZE => note_on(rs, me, &e, midi, next_id, emit, cx),
        NOTE_OFF | NOTE_CHOKE if e.size() == NOTE_SIZE => note_off(&mut rs[me], &e, midi, kind == NOTE_CHOKE, emit, cx.source),
        NOTE_EXPRESSION if e.size() == NOTE_SIZE => {
            let r = &mut rs[me];
            let address = e.address(EXPR_NOTE);
            let volume = e.i32_at(EXPR_ID) == VOLUME;
            let mut ok = true;
            for i in r.used.iter() {
                if !NoteRoute::from(r.slot(i), cx.source) || !r.matches(i, address) || (volume && r.stolen.has(i)) {
                    continue; // a stolen note's fade owns its volume
                }
                if volume {
                    r.volume[i % SLOTS] = e.f64_at(EXPR_VALUE);
                }
                ok = emit(&renote(r, i, &e, EXPR_NOTE)) && ok;
            }
            ok
        }
        PARAM_VALUE | PARAM_MOD if e.size() == PARAM_SIZE => {
            let r = &rs[me];
            let address = e.address(PARAM_NOTE);
            if address.0 < 0 && address.3 < 0 {
                return emit(&e);
            }
            let mut ok = true;
            for i in r.used.iter().filter(|&i| r.matches(i, address)) {
                ok = emit(&renote(r, i, &e, PARAM_NOTE)) && ok;
            }
            ok
        }
        NOTE_END => true, // consumed by the owning destination, never forwarded
        _ => emit(&e),
    }
}

/// An event re-addressed to the note in slot `i` as this route passed it on.
fn renote(r: &NoteRoute, i: usize, e: &Event, at: usize) -> Event {
    let s = r.slot(i);
    let mut copy = *e;
    copy.set_address(at, if r.takes_clap(s) { s.id } else { -1 }, i16::from(s.port), s.channel, s.key);
    copy
}

fn note_on(rs: &mut Routes, me: Route, e: &Event, midi: bool, next_id: &mut u32, emit: &mut impl FnMut(&Event) -> bool, cx: &mut Context) -> bool {
    if cx.draining {
        return true;
    }
    let (_, port, channel, key) = e.address(NOTE_ID);
    if port >= 16 || port as u32 >= rs[me].ports {
        return emit(e);
    }
    let zones = cx.zones.filter(|z| z.count != 0);
    let limit = match zones.and_then(Zones::first) {
        Some(z) if rs[me].clap[port as usize % 16] => z.max_notes,
        _ => 0,
    };
    let velocity = e.f64_at(NOTE_VELOCITY);
    let Some(zones) = zones else {
        return play(rs, me, e, midi, -1, key, velocity, limit, next_id, emit, cx);
    };
    let mut ok = true;
    for (z, zone) in zones.all().iter().enumerate() {
        let k = key + i16::from(zone.transpose);
        if zone.accepts(key, velocity, channel, cx.source) && (0..=127).contains(&k) {
            ok = play(rs, me, e, midi, z as i8, k, zone.shape(velocity), limit, next_id, emit, cx) && ok;
        }
    }
    ok // no zone took it: dropped, and so is its release
}

/// A note limit: before a note past it, the oldest released note goes, else the oldest held one,
/// sparing the highest and lowest held keys while another can go; counted across the peers.
fn make_room(rs: &mut Routes, me: Route, time: u32, limit: u16, emit: &mut impl FnMut(&Event) -> bool, peers: &[Route]) -> bool {
    if limit == 0 {
        return true;
    }
    let own = [me];
    let routes = if peers.is_empty() { &own[..] } else { peers };
    loop {
        let (mut held, mut low, mut high) = (0usize, i16::MAX, -1i16);
        for &r in routes {
            let r = &rs[r];
            for i in r.playing().filter(|&i| r.held.has(i)) {
                held += 1;
                low = low.min(r.slot(i).key);
                high = high.max(r.slot(i).key);
            }
        }
        let spared = |held_now: bool, key: i16| held_now && held > 2 && (key == low || key == high);
        let mut victim: Option<(Route, usize, bool, i32, bool)> = None; // route, slot, held, id, spared
        let mut playing = 0usize;
        for &ri in routes {
            let r = &rs[ri];
            for i in r.playing() {
                playing += 1;
                let (h, s) = (r.held.has(i), r.slot(i));
                let sp = spared(h, s.key);
                let better = match victim {
                    None => true,
                    Some((_, _, vh, vid, vsp)) => (vsp && !sp) || (vsp == sp && ((vh && !h) || (vh == h && s.id < vid))),
                };
                if better {
                    victim = Some((ri, i, h, s.id, sp));
                }
            }
        }
        let Some((owner, i, ..)) = victim else { return true };
        if playing < usize::from(limit) {
            return true;
        }
        if owner != me {
            rs[owner].steal(i, 0); // its step sends the fade, from its block's start
            continue;
        }
        let r = &mut rs[me];
        r.stolen.set(i);
        r.lift(i);
        if r.fade_frames != 0 {
            r.set_fade(i, time as i32);
        } else if !emit(&r.release(i, time, true)) {
            return false;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    rs: &mut Routes,
    me: Route,
    e: &Event,
    midi: bool,
    zone: i8,
    key: i16,
    velocity: f64,
    limit: u16,
    next_id: &mut u32,
    emit: &mut impl FnMut(&Event) -> bool,
    cx: &mut Context,
) -> bool {
    let time = e.time();
    if !make_room(rs, me, time, limit, emit, cx.peers) {
        return false;
    }
    if let Some(budget) = cx.budget.as_deref_mut() {
        budget.admit(rs, me, time);
    }
    let r = &mut rs[me];
    // Full: a held note whose voice already ended gives its slot up, its key's release first.
    if r.used.full() {
        if let Some(i) = r.used.iter().find(|&i| r.slot(i).voice_ended) {
            if emit(&r.release(i, time, false)) {
                r.drop_slot(i);
            }
        }
    }
    // Still full: the oldest released note gives its slot up (only its NOTE_END is missing).
    if r.used.full() {
        if let Some(i) = r.used.and_not(r.held).iter().min_by_key(|&i| r.slot(i).id) {
            r.drop_slot(i);
        }
    }
    let Some(i) = r.used.first_free() else { return false };
    if *next_id > i32::MAX as u32 {
        return false;
    }
    let (note_id, port, channel, address_key) = e.address(NOTE_ID);
    let id = *next_id as i32;
    *next_id += 1;
    r.slots[i % SLOTS] = Slot {
        id,
        note_id,
        flags: e.flags(),
        fade_at: -1,
        velocity: velocity as f32,
        key,
        address_key,
        channel,
        port: port as u8,
        zone,
        source: cx.source,
        faded: 0,
        midi,
        voice_ended: false,
        born: r.frames,
    };
    r.volume[i % SLOTS] = 1.0;
    r.used.set(i);
    r.hold(i);
    let mut copy = *e;
    copy.set_i16(NOTE_KEY, key);
    copy.set_f64(NOTE_VELOCITY, velocity);
    copy.set_i32(NOTE_ID, if r.clap[port as usize % 16] { id } else { -1 });
    emit(&copy)
}

fn note_off(r: &mut NoteRoute, e: &Event, midi: bool, choke: bool, emit: &mut impl FnMut(&Event) -> bool, source: u8) -> bool {
    let (id, port, channel, key) = e.address(NOTE_ID);
    // A repeated MIDI key releases its oldest note, in each zone that played it.
    let mut oldest = [None::<usize>; 9];
    if midi {
        for i in r.used.and(r.held).iter() {
            let s = r.slot(i);
            if s.midi && NoteRoute::from(s, source) && r.matches(i, (-1, port, channel, key)) {
                let o = &mut oldest[(s.zone + 1) as usize % 9];
                if o.is_none_or(|o| s.id < r.slot(o).id) {
                    *o = Some(i);
                }
            }
        }
    }
    let (mut ok, mut sent) = (true, false);
    for i in r.used.and_not(r.stolen).iter() {
        let s = *r.slot(i);
        if !NoteRoute::from(&s, source) || !r.matches(i, (id, port, channel, key)) || (midi && oldest[(s.zone + 1) as usize % 9] != Some(i)) {
            continue;
        }
        // A note whose voice already ended needs only the key's release, once.
        if s.voice_ended && sent {
            r.drop_slot(i);
            continue;
        }
        sent = true;
        r.lift(i);
        let sustained = r.pedal_of(&s) >= 64;
        let mut out = r.release(i, e.time(), choke);
        out.set_f64(NOTE_VELOCITY, e.f64_at(NOTE_VELOCITY));
        out.set_flags(e.flags());
        ok = emit(&out) && ok;
        if s.voice_ended || choke || (!r.takes_clap(&s) && !sustained) {
            r.drop_slot(i);
        }
    }
    ok
}
