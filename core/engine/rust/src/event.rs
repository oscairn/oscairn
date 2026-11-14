//! One CLAP event in 64 bytes, as kick::clap::Event (oscairn_event), read and written by field
//! offset: the CLAP structs' layout on every target Oscairn builds for. Offsets are constants, so
//! the bounds checks fold away.

pub const CORE_SPACE: u16 = 0;
pub const NOTE_ON: u16 = 0;
pub const NOTE_OFF: u16 = 1;
pub const NOTE_CHOKE: u16 = 2;
pub const NOTE_END: u16 = 3;
pub const NOTE_EXPRESSION: u16 = 4;
pub const PARAM_VALUE: u16 = 5;
pub const PARAM_MOD: u16 = 6;
const GESTURE_BEGIN: u16 = 7;
const GESTURE_END: u16 = 8;
pub const MIDI: u16 = 10;
const MIDI2: u16 = 12;
pub const VOLUME: i32 = 0; // CLAP_NOTE_EXPRESSION_VOLUME
const PRESSURE: i32 = 6; // CLAP_NOTE_EXPRESSION_PRESSURE

pub const NOTE_SIZE: u32 = 40; // clap_event_note_t, and clap_event_note_expression_t
pub const MIDI_SIZE: u32 = 24;
pub const PARAM_SIZE: u32 = 56; // value and modulation alike

// Field offsets: a note's, an expression's and a parameter event's address (note_id, port, channel, key).
pub const NOTE_ID: usize = 16;
pub const NOTE_KEY: usize = 24;
pub const NOTE_VELOCITY: usize = 32;
pub const EXPR_ID: usize = 16;
pub const EXPR_NOTE: usize = 20;
pub const EXPR_VALUE: usize = 32;
pub const PARAM_NOTE: usize = 32;
const FLAGS: usize = 12;

#[derive(Clone, Copy, PartialEq)]
#[repr(C, align(8))]
pub struct Event(pub [u8; 64]);

impl Default for Event {
    fn default() -> Self {
        Event([0; 64])
    }
}

macro_rules! field {
    ($get:ident, $set:ident, $t:ty, $n:literal) => {
        #[inline(always)]
        pub fn $get(&self, at: usize) -> $t {
            let mut b = [0u8; $n];
            b.copy_from_slice(&self.0[at..at + $n]);
            <$t>::from_ne_bytes(b)
        }
        #[inline(always)]
        pub fn $set(&mut self, at: usize, v: $t) {
            self.0[at..at + $n].copy_from_slice(&v.to_ne_bytes());
        }
    };
}

impl Event {
    field!(u16_at, set_u16, u16, 2);
    field!(i16_at, set_i16, i16, 2);
    field!(u32_at, set_u32, u32, 4);
    field!(i32_at, set_i32, i32, 4);
    field!(f64_at, set_f64, f64, 8);

    pub fn size(&self) -> u32 {
        self.u32_at(0)
    }
    pub fn time(&self) -> u32 {
        self.u32_at(4)
    }
    pub fn space(&self) -> u16 {
        self.u16_at(8)
    }
    pub fn kind(&self) -> u16 {
        self.u16_at(10)
    }
    pub fn flags(&self) -> u32 {
        self.u32_at(FLAGS)
    }
    pub fn set_kind(&mut self, kind: u16) {
        self.set_u16(10, kind)
    }
    pub fn set_flags(&mut self, flags: u32) {
        self.set_u32(FLAGS, flags)
    }
    pub fn header(size: u32, time: u32, space: u16, kind: u16, flags: u32) -> Event {
        let mut e = Event::default();
        e.set_u32(0, size);
        e.set_u32(4, time);
        e.set_u16(8, space);
        e.set_u16(10, kind);
        e.set_u32(FLAGS, flags);
        e
    }
    fn is(&self, kind: u16, size: u32) -> bool {
        self.space() == CORE_SPACE && self.kind() == kind && self.size() == size
    }
    /// A note address at `at` (note_id, port, channel, key), as every CLAP note-addressed event has.
    pub fn address(&self, at: usize) -> (i32, i16, i16, i16) {
        (self.i32_at(at), self.i16_at(at + 4), self.i16_at(at + 6), self.i16_at(at + 8))
    }
    pub fn set_address(&mut self, at: usize, id: i32, port: i16, channel: i16, key: i16) {
        self.set_i32(at, id);
        self.set_i16(at + 4, port);
        self.set_i16(at + 6, channel);
        self.set_i16(at + 8, key);
    }
    /// Event::note: a CLAP note on or off.
    pub fn note(on: bool, key: i16, velocity: f64, time: u32, channel: i16, port: i16) -> Event {
        let mut e = Event::header(NOTE_SIZE, time, CORE_SPACE, if on { NOTE_ON } else { NOTE_OFF }, 0);
        e.set_address(NOTE_ID, -1, port, channel, key);
        e.set_f64(NOTE_VELOCITY, velocity);
        e
    }
    /// Event::midi: a MIDI 1.0 message at sample 0.
    pub fn midi(status: u8, a: u8, b: u8, port: u16) -> Event {
        let mut e = Event::header(MIDI_SIZE, 0, CORE_SPACE, MIDI, 0);
        e.set_u16(16, port);
        e.0[18..21].copy_from_slice(&[status, a, b]);
        e
    }
    /// midiToClap's aftertouch (midi_input.cpp): pressure on `key` (-1: every note of the channel).
    pub fn pressure(time: u32, port: i16, channel: i16, key: i16, value: u8) -> Event {
        let mut e = Event::header(NOTE_SIZE, time, CORE_SPACE, NOTE_EXPRESSION, 0);
        e.set_i32(EXPR_ID, PRESSURE);
        e.set_address(EXPR_NOTE, -1, port, channel, key);
        e.set_f64(EXPR_VALUE, f64::from(value) / 127.0);
        e
    }
    pub fn midi_bytes(&self) -> (u16, [u8; 3]) {
        (self.u16_at(16), [self.0[18], self.0[19], self.0[20]])
    }
    /// Where its channel is, for the events that address one: notes, expressions, parameters.
    fn channel_at(&self) -> Option<usize> {
        match self.kind() {
            NOTE_ON | NOTE_OFF | NOTE_CHOKE if self.is(self.kind(), NOTE_SIZE) => Some(NOTE_ID + 6),
            NOTE_EXPRESSION if self.is(NOTE_EXPRESSION, NOTE_SIZE) => Some(EXPR_NOTE + 6),
            PARAM_VALUE | PARAM_MOD if self.is(self.kind(), PARAM_SIZE) => Some(PARAM_NOTE + 6),
            _ => None,
        }
    }
    /// The MIDI channel it addresses, or -1 for none (graph.cpp's channelOf).
    pub fn channel(&self) -> i16 {
        if self.space() != CORE_SPACE {
            return -1;
        }
        if let Some(at) = self.channel_at() {
            return self.i16_at(at);
        }
        let status = self.0[18];
        if self.is(MIDI, MIDI_SIZE) && (0x80..0xf0).contains(&status) {
            i16::from(status & 15)
        } else {
            -1
        }
    }
    /// For a shared engine's context: the event on `channel` (-1: as it is; graph.cpp's onChannel).
    pub fn on_channel(mut self, channel: i16) -> Event {
        if channel < 0 || self.space() != CORE_SPACE {
            return self;
        }
        if let Some(at) = self.channel_at() {
            self.set_i16(at, channel);
        } else if self.is(MIDI, MIDI_SIZE) && (0x80..0xf0).contains(&self.0[18]) {
            self.0[18] = (self.0[18] & 0xf0) | channel as u8;
        }
        self
    }
    /// The instance's NOTE_END for a context's connection on `channel`: only its channel's, then
    /// matched as its own notes were sent, on no channel (graph.cpp's endedOn).
    pub fn ended_on(mut self, channel: i16) -> Option<Event> {
        if channel < 0 || !self.is(NOTE_END, NOTE_SIZE) {
            return Some(self);
        }
        let at = NOTE_ID + 6;
        if self.i16_at(at) >= 0 && self.i16_at(at) != channel {
            return None;
        }
        self.set_i16(at, -1);
        Some(self)
    }
    /// Event::valid for the core event space (Host.cpp's validEvent), but for SysEx, which routing
    /// passes on either way.
    pub fn valid(&self) -> bool {
        if self.space() != CORE_SPACE || self.size() > 64 {
            return false;
        }
        let kind = self.kind();
        let size = match kind {
            NOTE_ON | NOTE_OFF | NOTE_CHOKE | NOTE_END | NOTE_EXPRESSION => NOTE_SIZE,
            PARAM_VALUE | PARAM_MOD => PARAM_SIZE,
            GESTURE_BEGIN | GESTURE_END => 24,
            MIDI => MIDI_SIZE,
            MIDI2 => 32,
            _ => return false,
        };
        if self.size() != size {
            return false;
        }
        let address = |at| {
            let (id, port, channel, key) = self.address(at);
            id >= -1 && port >= -1 && (-1..=15).contains(&channel) && (-1..=127).contains(&key)
        };
        match kind {
            NOTE_ON | NOTE_OFF | NOTE_CHOKE | NOTE_END => {
                let v = self.f64_at(NOTE_VELOCITY);
                let (_, port, channel, key) = self.address(NOTE_ID);
                address(NOTE_ID) && v.is_finite() && (0.0..=1.0).contains(&v) && (kind != NOTE_ON || (channel >= 0 && key >= 0 && port >= 0))
            }
            PARAM_VALUE | PARAM_MOD => address(PARAM_NOTE) && self.f64_at(48).is_finite(),
            NOTE_EXPRESSION => address(EXPR_NOTE) && self.f64_at(EXPR_VALUE).is_finite(),
            MIDI => {
                let (_, d) = self.midi_bytes();
                d[0] >= 0x80 && d[0] != 0xf0 && d[0] != 0xf7 && d[1] <= 127 && d[2] <= 127
            }
            _ => true,
        }
    }
}
