// SPDX-License-Identifier: AGPL-3.0-or-later
//! The key strip, the reference's ZoneEditor in its keys-only form (hosts/standalone/ZoneEditor.cpp): a key range over the
//! whole MIDI keyboard, 75 white keys across. A finger on an end of the range (within eight pixels of it) moves that
//! end; on its middle, the whole range; anywhere else, the nearer end goes there. The range is reported when the finger
//! lifts, so a drag is one edit; a double tap is the whole keyboard (0 to 127).

use crate::{StripKey, StripState};
use slint::ComponentHandle;
use slint::{ModelRc, VecModel};

/// The white keys of the whole keyboard (MIDI notes 0 to 127).
pub const WHITES: usize = 75;

/// How near an end a finger must be to take it, in pixels (the reference's 8).
pub const GRAB: f64 = 8.;

pub fn black(key: i64) -> bool {
    matches!(key.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// How many white keys lie below `key`.
fn whites_before(key: i64) -> i64 {
    (0..key.clamp(0, 128)).filter(|k| !black(*k)).count() as i64
}

/// The key under a finger at `share` (0 to 1) of the strip's width: the white key there.
pub fn key_at(share: f64) -> i64 {
    let white = ((share * WHITES as f64).floor() as i64).clamp(0, WHITES as i64 - 1);
    (0..128).filter(|k| !black(*k)).nth(white as usize).unwrap_or(127)
}

/// The keys as the strip draws them: a white key a unit wide, a black one 0.6 of it, centered on the line between its
/// neighbors (the reference's keyBounds).
pub fn keys() -> Vec<StripKey> {
    (0..128)
        .map(|k| {
            if black(k) {
                StripKey { key: k as i32, x: whites_before(k + 1) as f32 - 0.3, w: 0.6, black: true }
            } else {
                StripKey { key: k as i32, x: whites_before(k) as f32, w: 1., black: false }
            }
        })
        .collect()
}

/// The left edge of `key` as a share of the strip's width, and the right edge.
fn left(key: i64) -> f64 {
    if black(key) { whites_before(key + 1) as f64 - 0.3 } else { whites_before(key) as f64 }
}

fn right(key: i64) -> f64 {
    if black(key) { whites_before(key + 1) as f64 + 0.3 } else { whites_before(key) as f64 + 1. }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Low,
    High,
    Both,
}

/// A drag in progress: what it holds, where it began and the range then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    grab: Grab,
    grabbed: i64,
    low: i64,
    high: i64,
    /// The range as the drag has it now.
    pub now: (i64, i64),
}

impl Drag {
    /// A finger down at `share` of the width `width` (pixels) on the range `low` to `high`.
    pub fn begin(low: i64, high: i64, share: f64, width: f64) -> Drag {
        let x = share * width;
        let (a, b) = (left(low) / WHITES as f64 * width, right(high) / WHITES as f64 * width);
        let grab = if (x - a).abs() <= GRAB && (x - a).abs() <= (x - b).abs() {
            Grab::Low
        } else if (x - b).abs() <= GRAB {
            Grab::High
        } else if x > a && x < b {
            Grab::Both
        } else if x < a {
            Grab::Low
        } else {
            Grab::High
        };
        let mut drag = Drag { grab, grabbed: key_at(share), low, high, now: (low, high) };
        drag.to(share);
        drag
    }

    /// The finger at `share`: the range it makes.
    pub fn to(&mut self, share: f64) -> (i64, i64) {
        let at = key_at(share);
        let (mut low, mut high) = (self.low, self.high);
        match self.grab {
            Grab::Low => low = at.min(high),
            Grab::High => high = at.max(low),
            Grab::Both => {
                let by = (at - self.grabbed).clamp(-self.low, 127 - self.high);
                low += by;
                high += by;
            }
        }
        self.now = (low, high);
        self.now
    }
}

/// "All keys" or "C-1 - B3": the range in words over the keys.
pub fn range_text(low: i64, high: i64) -> String {
    if low == 0 && high == 127 {
        "All keys".into()
    } else {
        format!("{} – {}", crate::play::note_name(low), crate::play::note_name(high))
    }
}

/// Sets the strip's keys once (they never change) on the window's state.
pub fn install(ui: &crate::Shell) {
    ui.global::<StripState>().set_keys(ModelRc::new(VecModel::from(keys())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_are_seventy_five_whites_and_fifty_three_blacks() {
        let k = keys();
        assert_eq!((k.iter().filter(|k| !k.black).count(), k.iter().filter(|k| k.black).count()), (WHITES, 53));
        assert_eq!(whites_before(128), WHITES as i64);
        // C-1 is the first white key, G9 the last.
        assert_eq!((key_at(0.), key_at(1.)), (0, 127));
        assert_eq!(key_at(1. / WHITES as f64 * 1.5), 2); // the second white key is D
    }

    #[test]
    fn an_end_moves_the_middle_shifts_and_elsewhere_the_nearer_end_goes_there() {
        let width = 750.; // ten pixels a white key
        let share = |x: f64| x / width;
        let (low, high) = (60, 72); // C4 to C5: whites 35 to 42
        let (a, b) = (left(low) * 10., right(high) * 10.);
        // On the low end: it moves, the high end stays.
        let mut d = Drag::begin(low, high, share(a + 2.), width);
        assert_eq!(d.to(share(a - 100.)).1, 72);
        assert!(d.now.0 < 60);
        // Past the high end, the low end stops at it.
        assert_eq!(d.to(share(b + 200.)), (72, 72));
        // In the middle: the whole range moves by as many keys as the finger.
        let mut d = Drag::begin(low, high, share((a + b) / 2.), width);
        let (l, h) = d.to(share((a + b) / 2. + 30.));
        assert_eq!(h - l, 12);
        assert!(l > 60);
        // The ends of the keyboard hold it.
        assert_eq!(d.to(share(width * 2.)).1, 127);
        // Elsewhere: left of the range the low end, right of it the high end.
        assert_eq!(Drag::begin(low, high, share(a - 100.), width).grab, Grab::Low);
        assert_eq!(Drag::begin(low, high, share(b + 100.), width).grab, Grab::High);
    }

    #[test]
    fn the_range_in_words() {
        assert_eq!(range_text(0, 127), "All keys");
        assert_eq!(range_text(60, 72), "C4 – C5");
    }

    #[test]
    fn a_share_outside_the_strip_clamps_to_the_nearest_end() {
        assert_eq!(key_at(-0.5), 0); // before the first key is C-1
        assert_eq!(key_at(0.), 0);
        assert_eq!(key_at(2.), 127); // past the last key is G9
        assert_eq!(key_at(1.), 127);
    }

    #[test]
    fn a_black_key_is_detected_by_pitch_class() {
        assert!(!black(0) && !black(12) && !black(24)); // C
        assert!(black(1) && black(13)); // C-sharp in any octave
        assert!(!black(-1)); // one below C-1, outside the keyboard
        assert!(!black(127)); // G9 is a white key
    }

    #[test]
    fn the_end_keys_draw_flat() {
        let k = keys();
        assert_eq!((k[0].key, k[0].x, k[0].w, k[0].black), (0, 0., 1., false));
        assert_eq!((k[127].key, k[127].x, k[127].w, k[127].black), (127, 74., 1., false));
        // A black key sits 0.6 wide, centered on the line between its neighbors.
        assert_eq!((k[1].key, k[1].x, k[1].w, k[1].black), (1, 0.7, 0.6, true));
    }

    #[test]
    fn dragging_the_middle_to_the_low_end_stops_at_the_keyboard() {
        let width = 750.; // ten pixels a white key
        let (low, high) = (60, 72);
        let a = left(low) * 10.;
        let b = right(high) * 10.;
        let mut d = Drag::begin(low, high, (a + b) / 2. / width, width);
        let (l, h) = d.to(0.); // the finger goes to the far left
        assert_eq!((l, h), (0, high - low)); // the range keeps its width, held at 0
    }
}
