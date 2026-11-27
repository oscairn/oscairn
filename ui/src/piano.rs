// SPDX-License-Identifier: AGPL-3.0-or-later
//! The on-screen playing keyboard: the JUCE KeyboardWindow's job (hosts/standalone/KeyboardWindow.cpp),
//! the keys that play when no MIDI keyboard is connected, as a device's input (`notes.send` with
//! `input`, so the parts' device selectors and zones apply) or the built-in one. Kept from the JUCE
//! one: the computer's keys A W S E D F T G Y H U J K O L P and ; from middle C (Z and X an octave
//! down and up, at most four either way), Fixed velocity (every note at 100/127; off, the lower on a
//! key the louder; a typed key 0.8), Sustain (keys let go keep sounding until it is off), a band per
//! part the input plays above the keys, the keys the input holds lit (`heldNotes`), and the keyboard
//! closing when its input is unplugged, with its notes let go.
//!
//! Changed (the owner allows a better layout): two octaves and the next C, C3 to C5 at octave 0
//! (15 white keys, 85 pixels each on the FANTOM-07's screen, where the JUCE one is 56 pixels over
//! three octaves), the octave on the left knob, and a border cursor for use without touch: the
//! right knob or the dial steps it over the keys and then Octave down, Octave up, Fixed, Sustain and
//! Close; a press plays the key under it (a short note) or does the control; Cancel drops the cursor,
//! then closes.

use crate::nav::Action;
use crate::play::note_name;
use crate::setlist::update;
use crate::{PianoBand, PianoControl, PianoKey, PianoState, Shell};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

/// Keys shown: two octaves and the next C.
const KEYS: usize = 25;
const TYPING: &str = "awsedftgyhujkolp;";
const CONTROLS: [&str; 5] = ["−", "+", "Fixed", "Sustain", "Close"];
const FIXED: f64 = 100. / 127.;
/// A key pressed from the cursor sounds this long.
pub const TAP: std::time::Duration = std::time::Duration::from_millis(300);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Open,
    Close,
}

pub struct Piano {
    ui: slint::Weak<Shell>,
    model: Rc<VecModel<PianoKey>>,
    bands: Rc<VecModel<PianoBand>>,
    controls: Rc<VecModel<PianoControl>>,
    input: String,
    octave: i32,
    fixed: bool,
    sustain: bool,
    /// Pressed by touch or the cursor.
    held: BTreeSet<u8>,
    /// Pressed on the computer's keyboard, by the character.
    typed: HashMap<char, u8>,
    /// Let go with Sustain on: still sounding.
    sustained: BTreeSet<u8>,
    lit: Vec<u8>,
    /// The parts' zones: name, lowest and highest key.
    zones: Vec<(String, i64, i64)>,
    inputs: Vec<Value>,
    graph: Value,
    /// The item under the cursor: keys 0..KEYS, then the controls.
    cursor: Option<usize>,
    /// A note the cursor's press started, ended by `tap_end`.
    tap: Option<u8>,
    pub out: Vec<(&'static str, Value)>,
}

impl Piano {
    pub fn open(ui: &Shell, input: &str, status: &Value, graph: &Value) -> Piano {
        let state = ui.global::<PianoState>();
        let mut p = Piano {
            ui: ui.as_weak(),
            model: Rc::new(VecModel::default()),
            bands: Rc::new(VecModel::default()),
            controls: Rc::new(VecModel::default()),
            input: input.into(),
            octave: 0,
            fixed: false,
            sustain: false,
            held: BTreeSet::new(),
            typed: HashMap::new(),
            sustained: BTreeSet::new(),
            lit: vec![],
            zones: vec![],
            inputs: vec![],
            graph: Value::Null,
            cursor: None,
            tap: None,
            out: vec![],
        };
        state.set_keys(ModelRc::from(p.model.clone()));
        state.set_bands(ModelRc::from(p.bands.clone()));
        state.set_controls(ModelRc::from(p.controls.clone()));
        state.set_open(true);
        p.graph = graph.clone();
        p.status(status);
        p.show();
        p
    }

    pub fn close(&mut self) {
        self.release();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<PianoState>().set_open(false);
        }
    }

    pub fn lowest(&self) -> u8 {
        (48 + 12 * self.octave).clamp(0, 127 - 24) as u8
    }

    fn velocity(&self, y: Option<f64>) -> f64 {
        if self.fixed {
            FIXED
        } else {
            y.map_or(0.8, |y| y.clamp(0.1, 1.))
        }
    }

    fn send(&mut self, on: bool, key: u8, velocity: f64) {
        let mut params = json!({"on": on, "key": key});
        if on {
            params["velocity"] = json!((velocity * 1000.).round() / 1000.);
        }
        if !self.input.is_empty() {
            params["input"] = json!(self.input);
        }
        self.out.push(("notes.send", params));
    }

    /// A key pressed (with where on it, 0..1 down) or let go; the note is the key's number of the range.
    fn press(&mut self, note: u8, on: bool, y: Option<f64>) {
        if on {
            if self.held.insert(note) {
                self.sustained.remove(&note);
                let v = self.velocity(y);
                self.send(true, note, v);
            }
        } else if self.held.remove(&note) && !self.sustained_contains_typed(note) {
            self.let_go(note);
        }
    }

    fn sustained_contains_typed(&self, note: u8) -> bool {
        self.typed.values().any(|&n| n == note)
    }

    fn let_go(&mut self, note: u8) {
        if self.sustain {
            self.sustained.insert(note);
        } else {
            self.send(false, note, 0.);
        }
    }

    /// Every note let go at once (the keyboard closes, goes behind, or the octave moves).
    pub fn release(&mut self) {
        let all: BTreeSet<u8> = self.held.iter().chain(self.sustained.iter()).copied().chain(self.typed.values().copied()).collect();
        self.held.clear();
        self.sustained.clear();
        self.typed.clear();
        for n in all {
            self.send(false, n, 0.);
        }
        self.sustain = false;
        self.show();
    }

    /// A key of the layout touched: index `i` of the range, `y` how far down.
    pub fn touch(&mut self, i: usize, down: bool, y: f64) {
        let note = self.lowest() + i as u8;
        self.press(note, down, Some(y));
        self.show();
    }

    /// A character typed on the computer's keyboard (`down`), or let go: a note of the typing row, or
    /// Z and X for the octave. True when the character is the keyboard's.
    pub fn typed(&mut self, text: &str, down: bool) -> bool {
        let Some(c) = text.chars().next().filter(|_| text.chars().count() == 1).map(|c| c.to_ascii_lowercase()) else { return false };
        if c == 'z' || c == 'x' {
            if down {
                self.shift(if c == 'x' { 1 } else { -1 });
            }
            return true;
        }
        let Some(at) = TYPING.find(c) else { return false };
        if down {
            if !self.typed.contains_key(&c) {
                let note = (60 + 12 * self.octave + at as i32).clamp(0, 127) as u8;
                self.typed.insert(c, note);
                let v = self.velocity(None);
                if self.held.insert(note) {
                    self.send(true, note, v);
                }
            }
        } else if let Some(note) = self.typed.remove(&c) {
            if self.held.remove(&note) {
                self.let_go(note);
            }
        }
        self.show();
        true
    }

    fn shift(&mut self, by: i32) {
        // Played an octave over, released in it.
        let all: Vec<u8> = self.held.iter().copied().collect();
        self.held.clear();
        self.typed.clear();
        for n in all {
            self.send(false, n, 0.);
        }
        self.octave = (self.octave + by).clamp(-4, 4);
        self.show();
    }

    fn control(&mut self, n: usize) -> Outcome {
        match n {
            0 => self.shift(-1),
            1 => self.shift(1),
            2 => self.fixed = !self.fixed,
            3 => {
                self.sustain = !self.sustain;
                if !self.sustain {
                    for n in std::mem::take(&mut self.sustained) {
                        if !self.held.contains(&n) {
                            self.send(false, n, 0.);
                        }
                    }
                }
            }
            _ => return Outcome::Close,
        }
        self.show();
        Outcome::Open
    }

    pub fn tap_control(&mut self, n: usize) -> Outcome {
        self.control(n)
    }

    /// The cursor's press: a note for `TAP`, or a control.
    fn confirm(&mut self) -> Outcome {
        match self.cursor {
            Some(i) if i < KEYS => {
                self.tap_end();
                let note = self.lowest() + i as u8;
                self.tap = Some(note);
                self.press(note, true, None);
                self.show();
                Outcome::Open
            }
            Some(i) => self.control(i - KEYS),
            None => Outcome::Open,
        }
    }

    /// The note the cursor's press started ends.
    pub fn tap_end(&mut self) {
        if let Some(n) = self.tap.take() {
            self.press(n, false, None);
            self.show();
        }
    }

    pub fn tapping(&self) -> bool {
        self.tap.is_some()
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let items = KEYS + CONTROLS.len();
        // Where the cursor goes first: middle C.
        let step = |c: Option<usize>, by: i64| Some(c.map_or(12, |c| (c as i64 + by).clamp(0, items as i64 - 1) as usize));
        match action {
            Action::Move { pane: Some(0), by } => self.shift(by.signum()),
            Action::Move { by, .. } => self.cursor = step(self.cursor, by as i64),
            Action::Arrow { dx: 0, dy } => self.shift(-dy.signum()),
            Action::Arrow { dx, .. } => self.cursor = step(self.cursor, dx as i64),
            Action::Page(by) => self.shift(by),
            Action::End(by) => self.cursor = Some(if by < 0 { 0 } else { items - 1 }),
            Action::Confirm(_) => return self.confirm(),
            Action::Cancel(_) => {
                if self.cursor.take().is_none() {
                    return Outcome::Close;
                }
            }
        }
        self.show();
        Outcome::Open
    }

    /// session.status: the keys the input holds, and the inputs the parts' devices resolve against.
    /// False when the input is gone (unplugged).
    pub fn status(&mut self, status: &Value) -> bool {
        if let Some(inputs) = status["midiInputs"].as_array() {
            self.inputs = inputs.clone();
        }
        let source = self.source();
        if source.is_none() && status["midiInputs"].is_array() {
            return false;
        }
        self.lit = status["heldNotes"].as_array().into_iter().flatten().filter(|h| h["source"].as_i64() == source).flat_map(|h| h["keys"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|k| k as u8)).collect();
        self.zones();
        self.show();
        true
    }

    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
        self.zones();
        self.show();
    }

    /// The input's number in the status (0 the built-in keys).
    fn source(&self) -> Option<i64> {
        if self.input.is_empty() {
            return Some(0);
        }
        self.inputs.iter().find(|e| e["name"].as_str() == Some(self.input.as_str())).and_then(|e| e["source"].as_i64())
    }

    /// Whether a part's device rules take notes from this input.
    fn plays(&self, rules: &Value) -> bool {
        let Some(rules) = rules.as_array().filter(|r| !r.is_empty()) else { return true };
        let this = self.inputs.iter().find(|e| e["name"].as_str() == Some(self.input.as_str()));
        rules.iter().any(|r| {
            let pattern = r["pattern"].as_str().unwrap_or_default();
            // The engine's DeviceRule::named (parts.cpp): equals is the whole name, any case;
            // contains (and the engine's default, and regex, which the UI has no crate to evaluate
            // exactly) is a case-insensitive substring.
            let named = |name: &str| {
                let n = name.to_lowercase();
                if r["match"].as_str() == Some("equals") { n == pattern.to_lowercase() }
                else { n.contains(&pattern.to_lowercase()) }
            };
            match (r["kind"].as_str(), this) {
                (Some("all"), _) => true,
                (Some("builtIn"), None) => true,
                (Some("builtIn"), Some(_)) => false,
                (_, None) => false,
                (Some("external"), Some(e)) => e["kind"] != "builtIn",
                (Some("port"), Some(e)) => e["kind"] == "port" && e["port"] == r["port"],
                (Some("usb"), Some(e)) => e["kind"] == "usb",
                (Some("usbNamed"), Some(e)) => e["kind"] == "usb" && named(e["name"].as_str().unwrap_or_default()),
                (Some("network"), Some(e)) => e["kind"] == "network" && (pattern.is_empty() || named(e["name"].as_str().unwrap_or_default())),
                _ => false,
            }
        })
    }

    fn zones(&mut self) {
        let parts: Vec<&Value> = self.graph["parts"].as_array().into_iter().flatten().filter(|p| p["input"]["kind"] == "notes" && self.plays(&p["input"]["devices"])).collect();
        self.zones = parts
            .into_iter()
            .map(|p| {
                let keys = &p["input"]["zones"][0]["keys"];
                (p["name"].as_str().unwrap_or_default().to_string(), keys[0].as_i64().unwrap_or(0), keys[1].as_i64().unwrap_or(127))
            })
            .collect();
    }

    /// The title: the input's name.
    fn title(&self) -> String {
        if self.input.is_empty() { "On-screen keyboard".into() } else { self.input.clone() }
    }

    fn show(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let low = self.lowest();
        let mut keys = vec![];
        let mut white = 0.;
        for i in 0..KEYS {
            let note = low + i as u8;
            let black = matches!(note % 12, 1 | 3 | 6 | 8 | 10);
            let (x, w) = if black { (white - 0.3, 0.6) } else { (white, 1.) };
            if !black {
                white += 1.;
            }
            keys.push(PianoKey { x, w, black, lit: self.lit.contains(&note), held: self.held.contains(&note) || self.sustained.contains(&note), cursor: self.cursor == Some(i), label: if note.is_multiple_of(12) { note_name(note as i64).into() } else { Default::default() } });
        }
        update(&self.model, keys);
        let bands: Vec<PianoBand> = self
            .zones
            .iter()
            .take(6)
            .enumerate()
            .filter_map(|(row, (name, lo, hi))| {
                let (lo, hi) = ((*lo).max(low as i64), (*hi).min((low + KEYS as u8 - 1) as i64));
                (lo <= hi).then(|| {
                    let at = |n: i64| (low as i64..n).filter(|k| !matches!(k % 12, 1 | 3 | 6 | 8 | 10)).count() as f32;
                    PianoBand { x: at(lo), w: at(hi) + 1. - at(lo), row: row as i32, name: name.as_str().into() }
                })
            })
            .collect();
        update(&self.bands, bands);
        let on = [false, false, self.fixed, self.sustain, false];
        let controls: Vec<PianoControl> = CONTROLS.iter().enumerate().map(|(i, l)| PianoControl { label: (*l).into(), on: on[i], cursor: self.cursor == Some(KEYS + i) }).collect();
        update(&self.controls, controls);
        let state = ui.global::<PianoState>();
        state.set_whites(15.);
        state.set_title(self.title().into());
        state.set_range(format!("{}–{}", note_name(low as i64), note_name((low + KEYS as u8 - 1) as i64)).into());
    }

    #[cfg(test)]
    pub fn zone_names(&self) -> Vec<(String, i64, i64)> {
        self.zones.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A keyboard with no window: `show` sees the weak `ui` never upgrade and stops early, so the
    /// pure parts (how a note, a velocity or a zone is worked out) are put under test alone.
    fn piano(input: &str) -> Piano {
        Piano {
            ui: slint::Weak::default(),
            model: Rc::new(VecModel::default()),
            bands: Rc::new(VecModel::default()),
            controls: Rc::new(VecModel::default()),
            input: input.into(),
            octave: 0,
            fixed: false,
            sustain: false,
            held: BTreeSet::new(),
            typed: HashMap::new(),
            sustained: BTreeSet::new(),
            lit: vec![],
            zones: vec![],
            inputs: vec![],
            graph: Value::Null,
            cursor: None,
            tap: None,
            out: vec![],
        }
    }

    /// The key the last `notes.send` for a `true` note put down at.
    fn sent_note(p: &Piano) -> u64 {
        p.out.last().unwrap().1["key"].as_u64().unwrap()
    }

    #[test]
    fn lowest_keeps_the_octave_inside_the_playable_range() {
        let mut p = piano("");
        assert_eq!(p.lowest(), 48, "octave 0 starts on C3");
        p.octave = 4;
        assert_eq!(p.lowest(), 96, "four octaves up");
        p.octave = -4;
        assert_eq!(p.lowest(), 0, "four octaves down");
        p.octave = 8;
        assert_eq!(p.lowest(), 103, "more than four octaves up clamps so the last key is under 128");
        assert_eq!((p.lowest() as i64 + KEYS as i64 - 1), 127, "the top key is the 128th, note 127");
        p.octave = -8;
        assert_eq!(p.lowest(), 0, "more than four octaves down clamps to 0");
    }

    #[test]
    fn velocity_is_fixed_or_drops_soft_and_loud_touches() {
        let mut p = piano("");
        assert_eq!(p.velocity(Some(0.5)), 0.5, "a plain touch plays where it is");
        assert_eq!(p.velocity(Some(0.)), 0.1, "a touch at the very top is not silent");
        assert_eq!(p.velocity(Some(2.)), 1., "a touch past the bottom is capped at full");
        assert_eq!(p.velocity(Some(-1.)), 0.1, "a touch past the top drops to the quietest");
        assert_eq!(p.velocity(None), 0.8, "a typed key and the cursor are 0.8");
        p.fixed = true;
        assert_eq!(p.velocity(Some(0.)), FIXED, "Fixed ignores where it is touched");
        assert_eq!(p.velocity(None), FIXED, "Fixed ignores a typed key too");
        assert_eq!(FIXED, 100. / 127., "Fixed is 100 of 127");
    }

    #[test]
    fn typed_accepts_only_one_known_key_and_folds_case() {
        let mut p = piano("");
        assert!(p.typed("a", true), "the a key is the keyboard's");
        assert!(p.typed("A", true), "a capital plays the same key");
        assert!(p.typed("p", true), "p is a key");
        assert!(p.typed(";", true), "the semicolon is a key");
        assert!(!p.typed("1", true), "a digit is not a key");
        assert!(!p.typed("ab", true), "two characters are not one key");
        assert!(!p.typed("", true), "empty is no key");
        assert!(!p.typed("é", true), "a unicode letter is not a key");
    }

    #[test]
    fn typed_key_notes_follow_the_octave_and_clamp_to_the_ends() {
        let at = |p: &mut Piano, key: &str, octave: i32| {
            p.out.clear();
            p.typed.clear();
            p.held.clear();
            p.octave = octave;
            assert!(p.typed(key, true), "{key} is a key at octave {octave}");
            sent_note(p)
        };
        let mut p = piano("");
        assert_eq!(at(&mut p, "a", 0), 60, "a at octave 0 is middle C");
        assert_eq!(at(&mut p, "a", 4), 108, "a four octaves up");
        assert_eq!(at(&mut p, "a", 8), 127, "past the top clamps to the last key");
        assert_eq!(at(&mut p, "a", -8), 0, "past the bottom clamps to the first key");
        assert_eq!(at(&mut p, ";", 4), 108 + 16, "the last typing key stays inside the octave");
    }

    #[test]
    fn source_is_built_in_for_an_empty_input_and_finds_a_named_device() {
        let mut p = piano("");
        assert_eq!(p.source(), Some(0), "the built-in keys are input 0");
        p.input = "Roland".into();
        p.inputs = vec![json!({"name": "Other", "source": 7}), json!({"name": "Roland", "source": 3})];
        assert_eq!(p.source(), Some(3), "a named input finds its source");
        p.input = "Missing".into();
        assert_eq!(p.source(), None, "a named input that is gone has no source");
    }

    #[test]
    fn plays_keeps_or_skips_a_part_by_its_device_rules() {
        let mut p = piano("");
        p.inputs = vec![
            json!({"name": "Roland", "source": 3, "kind": "usb", "port": 2}),
            json!({"name": "Midi", "source": 5, "kind": "port", "port": 1}),
            json!({"name": "net1", "source": 9, "kind": "network", "port": 0}),
        ];
        assert!(p.plays(&json!([])), "no rules plays everything");
        assert!(p.plays(&json!([{"kind": "all"}])), "all plays anything");
        assert!(p.plays(&json!([{"kind": "builtIn"}])), "builtIn plays the built-in keys");
        p.input = "Roland".into();
        assert!(!p.plays(&json!([{"kind": "builtIn"}])), "builtIn does not play a plugged device");
        assert!(p.plays(&json!([{"kind": "external"}])), "external plays any plugged device");
        assert!(p.plays(&json!([{"kind": "usb"}])), "usb plays the usb device");
        assert!(!p.plays(&json!([{"kind": "port", "port": 2}])), "a port rule does not play a usb device");
        p.input = "Midi".into();
        assert!(p.plays(&json!([{"kind": "port", "port": 1}])), "the device on the matching port");
        assert!(!p.plays(&json!([{"kind": "port", "port": 2}])), "another port does not");
        p.input = "Roland".into();
        assert!(p.plays(&json!([{"kind": "usbNamed", "pattern": "roland", "match": "contains"}])), "contains matches by name, any case");
        assert!(p.plays(&json!([{"kind": "usbNamed", "pattern": "Roland"}])), "the exact name matches");
        assert!(!p.plays(&json!([{"kind": "nonsense"}])), "an unknown kind plays nothing");
        p.input = "net1".into();
        assert!(p.plays(&json!([{"kind": "network", "pattern": ""}])), "an unnamed network rule plays any network");
        assert!(p.plays(&json!([{"kind": "network", "pattern": "net1"}])), "the network name");
        assert!(!p.plays(&json!([{"kind": "usbNamed", "pattern": "net1"}])), "usbNamed skips a network");
        p.input = "Missing".into();
        assert!(!p.plays(&json!([{"kind": "external"}])), "a plugged device that is gone plays no external rule");
    }

    #[test]
    fn plays_matches_the_engine_named_rule_table() {
        // The engine's own DeviceRule table from core/control/tests/parts.cpp `devices`,
        // through the UI's plays(): both sides agree.
        let mut p = piano("");
        p.inputs = vec![
            json!({"name": "Arturia KeyLab 61", "source": 1, "kind": "usb", "port": 0}),
            json!({"name": "nanoKONTROL2", "source": 2, "kind": "usb", "port": 0}),
            json!({"name": "MIDI 2", "source": 3, "kind": "port", "port": 2}),
            json!({"name": "Session 1", "source": 4, "kind": "network", "port": 0}),
        ];
        p.input = "Arturia KeyLab 61".into();
        assert!(p.plays(&json!([{"kind": "usbNamed", "pattern": "arturia keylab 61", "match": "equals"}])), "equals is the whole name, any case");
        assert!(!p.plays(&json!([{"kind": "usbNamed", "pattern": "arturia", "match": "equals"}])), "equals is the whole name, not a part");
        p.input = "MIDI 2".into();
        assert!(!p.plays(&json!([{"kind": "usbNamed", "pattern": "midi"}])), "usbNamed skips a port, even one it is named");
        assert!(!p.plays(&json!([{"kind": "usbNamed", "pattern": "midi", "match": "equals"}])), "usbNamed skips a port for equals too");
        assert!(p.plays(&json!([{"kind": "port", "port": 2}])), "the port rule hears the port");
        p.input = "nanoKONTROL2".into();
        // The UI has no regex crate and no engine report here, so regex falls back to a
        // case-insensitive contains (documented in the note, not exact).
        assert!(p.plays(&json!([{"kind": "usbNamed", "pattern": "kontrol", "match": "regex"}])), "regex falls back to case-insensitive contains");
        assert!(!p.plays(&json!([{"kind": "usbNamed", "pattern": "^nano", "match": "regex"}])), "regex anchors are not exact without a crate");
    }

    #[test]
    fn zones_keep_the_notes_parts_the_input_plays() {
        let mut p = piano("");
        p.graph = json!({
            "parts": [
                {"name": "Keys", "input": {"kind": "notes", "zones": [{"keys": [60, 72]}], "devices": []}},
                {"name": "Drum", "input": {"kind": "midi", "zones": [{"keys": [0, 127]}]}},
                {"name": "Only 3", "input": {"kind": "notes", "zones": [{"keys": [3, 4]}], "devices": [{"kind": "builtIn"}]}}
            ]
        });
        p.zones();
        assert!(p.zone_names().iter().all(|(n, _, _)| n != "Drum"), "a midi part has no note zone");
        assert_eq!(p.zone_names(), [("Keys".to_string(), 60, 72), ("Only 3".to_string(), 3, 4)], "the notes parts the input plays");
    }

    #[test]
    fn zones_default_a_part_without_a_zone_to_the_whole_range() {
        let mut p = piano("");
        p.graph = json!({"parts": [{"name": "No zones", "input": {"kind": "notes"}}]});
        p.zones();
        assert_eq!(p.zone_names(), [("No zones".to_string(), 0, 127)], "a missing zone reads as the whole keyboard");
    }
}
