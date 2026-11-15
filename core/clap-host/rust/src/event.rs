// SPDX-License-Identifier: AGPL-3.0-or-later
//! kick::clap::Event: an owning 64-byte CLAP event, and the host's checks of one (Host.cpp's validEvent).
use clap_sys::events::*;
use oscairn_clap_sys::oscairn_chord_type::OSCAIRN_CHORD_MINOR9;
use oscairn_clap_sys::oscairn_param_target_mode::{OSCAIRN_PARAM_TARGET_MODULATE, OSCAIRN_PARAM_TARGET_SET};
use oscairn_clap_sys::{oscairn_event_beat_t, oscairn_event_harmony_t, oscairn_event_param_target_t};
use oscairn_clap_sys::{OSCAIRN_EVENT_BEAT, OSCAIRN_EVENT_HARMONY, OSCAIRN_EVENT_PARAM_TARGET};
use std::mem::size_of;

/// The event spaces this host assigns (Host.h): oscairn.param-target, oscairn.beat, oscairn.harmony.
pub const PARAM_TARGET_SPACE: u16 = 1000;
pub const BEAT_SPACE: u16 = 1001;
pub const HARMONY_SPACE: u16 = 1002;
/// SysexPool::maxMessage.
pub const MAX_SYSEX: u32 = 64 * 1024;

#[repr(C, align(8))]
#[derive(Clone, Copy, PartialEq)]
pub struct Event {
    pub bytes: [u8; 64],
}

impl Default for Event {
    fn default() -> Self {
        Self { bytes: [0; 64] }
    }
}

impl Event {
    pub fn header(&self) -> &clap_event_header {
        // SAFETY: 64 bytes, 8-aligned, hold any header; every bit pattern is a valid header.
        unsafe { &*self.bytes.as_ptr().cast() }
    }
    pub fn header_mut(&mut self) -> &mut clap_event_header {
        // SAFETY: as header.
        unsafe { &mut *self.bytes.as_mut_ptr().cast() }
    }
    /// The event as T, which must fit (every T used is plain old data).
    pub fn get<T: Copy>(&self) -> T {
        const { assert!(size_of::<T>() <= 64) };
        // SAFETY: T fits, and is read unaligned.
        unsafe { self.bytes.as_ptr().cast::<T>().read_unaligned() }
    }
    pub fn from<T: Copy>(source: &T) -> Event {
        const { assert!(size_of::<T>() <= 64) };
        let mut e = Event::default();
        // SAFETY: T fits.
        unsafe { e.bytes.as_mut_ptr().cast::<T>().write_unaligned(*source) };
        e
    }
    /// Copies an event a plugin gave (its header's size, at most 64).
    ///
    /// # Safety
    /// h points at a header followed by its size's bytes, at most 64.
    pub unsafe fn copy(h: *const clap_event_header) -> Event {
        let mut e = Event::default();
        // SAFETY: the caller's.
        unsafe { std::ptr::copy_nonoverlapping(h.cast::<u8>(), e.bytes.as_mut_ptr(), (*h).size as usize) };
        e
    }
    pub fn midi(status: u8, a: u8, b: u8, frame: u32, port: u16) -> Event {
        // SAFETY: plain old data, all zeros valid.
        let mut e: clap_event_midi = unsafe { std::mem::zeroed() };
        e.header =
            clap_event_header { size: size_of::<clap_event_midi>() as u32, time: frame, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: CLAP_EVENT_MIDI, flags: 0 };
        e.port_index = port;
        e.data = [status, a, b];
        Event::from(&e)
    }
    pub fn note(on: bool, key: i32, velocity: f64, frame: u32, channel: i32, port: i32, note_id: i32) -> Event {
        // SAFETY: as midi.
        let mut e: clap_event_note = unsafe { std::mem::zeroed() };
        let type_ = if on { CLAP_EVENT_NOTE_ON } else { CLAP_EVENT_NOTE_OFF };
        e.header = clap_event_header { size: size_of::<clap_event_note>() as u32, time: frame, space_id: CLAP_CORE_EVENT_SPACE_ID, type_, flags: 0 };
        e.note_id = note_id;
        e.port_index = port as i16;
        e.channel = channel as i16;
        e.key = key as i16;
        e.velocity = velocity;
        Event::from(&e)
    }
}

/// Host.cpp's validEvent, check for check.
///
/// # Safety
/// h is null or points at a header followed by at least the bytes the type it names needs (a SysEx event's buffer
/// readable for its size).
pub unsafe fn valid(h: *const clap_event_header) -> bool {
    if h.is_null() {
        return false;
    }
    // SAFETY: the caller's.
    let hd = unsafe { &*h };
    if hd.space_id == PARAM_TARGET_SPACE {
        // SAFETY: the caller's: the bytes are there (read before the size is checked, as Host.cpp does).
        let t = unsafe { h.cast::<oscairn_event_param_target_t>().read_unaligned() };
        return hd.type_ == OSCAIRN_EVENT_PARAM_TARGET as u16
            && hd.size as usize == size_of::<oscairn_event_param_target_t>()
            && (t.mode == OSCAIRN_PARAM_TARGET_SET as _ || t.mode == OSCAIRN_PARAM_TARGET_MODULATE as _)
            && t.value.is_finite();
    }
    if hd.space_id == BEAT_SPACE {
        // SAFETY: as above.
        let b = unsafe { h.cast::<oscairn_event_beat_t>().read_unaligned() };
        return hd.type_ == OSCAIRN_EVENT_BEAT as u16 && hd.size as usize == size_of::<oscairn_event_beat_t>() && b.tempo.is_finite() && b.tempo > 0.0;
    }
    if hd.space_id == HARMONY_SPACE {
        // SAFETY: as above.
        let x = unsafe { h.cast::<oscairn_event_harmony_t>().read_unaligned() }.harmony;
        return hd.type_ == OSCAIRN_EVENT_HARMONY as u16
            && hd.size as usize == size_of::<oscairn_event_harmony_t>()
            && x.key_root < 12
            && x.chord_root < 12
            && x.chord_bass < 12
            && x.chord_type as u32 <= OSCAIRN_CHORD_MINOR9
            && x.scale < 4096
            && (x.scale & 1) != 0
            && x.reserved == 0;
    }
    if hd.space_id != CLAP_CORE_EVENT_SPACE_ID || hd.size > 64 {
        return false;
    }
    let size = match hd.type_ {
        CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE | CLAP_EVENT_NOTE_END => size_of::<clap_event_note>(),
        CLAP_EVENT_NOTE_EXPRESSION => size_of::<clap_event_note_expression>(),
        CLAP_EVENT_PARAM_VALUE => size_of::<clap_event_param_value>(),
        CLAP_EVENT_PARAM_MOD => size_of::<clap_event_param_mod>(),
        CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => size_of::<clap_event_param_gesture>(),
        CLAP_EVENT_MIDI => size_of::<clap_event_midi>(),
        CLAP_EVENT_MIDI2 => size_of::<clap_event_midi2>(),
        CLAP_EVENT_MIDI_SYSEX => size_of::<clap_event_midi_sysex>(),
        _ => return false,
    };
    if hd.size as usize != size {
        return false;
    }
    // SAFETY: the header's size is its type's, so the struct is all there.
    let e = unsafe { Event::copy(h) };
    let address = |note_id: i32, port: i16, channel: i16, key: i16| note_id >= -1 && port >= -1 && (-1..=15).contains(&channel) && (-1..=127).contains(&key);
    match hd.type_ {
        CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE | CLAP_EVENT_NOTE_END => {
            let n: clap_event_note = e.get();
            if !n.velocity.is_finite()
                || n.velocity < 0.0
                || n.velocity > 1.0
                || n.note_id < -1
                || n.channel < -1
                || n.channel > 15
                || n.key < -1
                || n.key > 127
                || n.port_index < -1
            {
                return false;
            }
            if hd.type_ == CLAP_EVENT_NOTE_ON && (n.channel < 0 || n.key < 0 || n.port_index < 0) {
                return false;
            }
        }
        CLAP_EVENT_PARAM_VALUE => {
            let p: clap_event_param_value = e.get();
            if !address(p.note_id, p.port_index, p.channel, p.key) || !p.value.is_finite() {
                return false;
            }
        }
        CLAP_EVENT_PARAM_MOD => {
            let p: clap_event_param_mod = e.get();
            if !address(p.note_id, p.port_index, p.channel, p.key) || !p.amount.is_finite() {
                return false;
            }
        }
        CLAP_EVENT_NOTE_EXPRESSION => {
            let p: clap_event_note_expression = e.get();
            if !address(p.note_id, p.port_index, p.channel, p.key) || !p.value.is_finite() {
                return false;
            }
        }
        CLAP_EVENT_MIDI => {
            let m: clap_event_midi = e.get();
            if m.data[0] < 0x80 || m.data[0] == 0xf0 || m.data[0] == 0xf7 || m.data[1] > 127 || m.data[2] > 127 {
                return false;
            }
        }
        CLAP_EVENT_MIDI_SYSEX => {
            // One whole message, F0 to F7; its data bytes are checked where a plugin sends it.
            let x: clap_event_midi_sysex = e.get();
            if x.buffer.is_null() || x.size < 2 || x.size > MAX_SYSEX {
                return false;
            }
            // SAFETY: the caller's: the buffer is readable for its size.
            let (first, last) = unsafe { (*x.buffer, *x.buffer.add(x.size as usize - 1)) };
            if first != 0xf0 || last != 0xf7 {
                return false;
            }
        }
        _ => {}
    }
    true
}
