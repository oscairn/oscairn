// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Part screen: the settings of one part, as the JUCE Part page (hosts/standalone/PartsView.cpp,
//! PartsModel.cpp's partFields) has them for the stage, on the shell. Opened from Play by selecting
//! a part. A list of settings (level, pan, mute, solo, which devices play it, its key range and
//! transpose, and the sustain, sostenuto and soft pedals each: every pedal, none or its devices')
//! with a border cursor, and what the part is made of beside it, read only: its sound and engine,
//! where it plays and its sends, its insert effects.
//!
//! Without touch: the left knob (pane 0) or the dial moves the cursor; a press on a value starts changing it,
//! after which the dial changes it (each step is sent, as the JUCE sliders send each move) and a
//! press or Cancel ends that; a press on mute or solo toggles it; on "Plays from" opens the list of
//! devices, where a press ticks one; Cancel closes the list, then drops the cursor, then goes back.
//! The right knob (pane 1) scrolls the read-only panel when it holds more than fits, a view at a
//! time, with the same cursor. Touch: a tap does what a press does, - and + step a value, and the
//! panel's arrows scroll it.
//!
//! Everything is read from `graph.get` (asked by the shell on each `graph.changed`), the levels and
//! mutes that move live from `controls.changed` and the inputs from `session.status`. Edits are
//! `parts.level`, `parts.pan`, `parts.solo` and `parts.input` (the part's own input as `graph.get`
//! gives it with the one setting changed, as the JUCE page's editInput does).
//!
//! Also here, as the JUCE page's partFields list them: the name (a press opens the on-screen
//! keyboard), where the part plays to (Main or a bus), the input channel, the velocities, notes at
//! once, reserved notes, priority, MIDI out (a list of the outputs, and its channel), a send per
//! bus, and Remove part (a second press confirms). Every edit that the engine stages (the input, MIDI
//! out, output, sends, name, removal) is followed by `graph.commit`, as the JUCE page's setters commit.
//!
//! The part's key (RFC 0033; the JUCE app has no control for it): "Key" follows the Performance's (named from
//! `harmony.get`) or is a root of its own, then "Scale" names its scale; `harmony.set` with the part.
//!
//! Not here yet: choosing the sound, the chain's plugins and splits (ui/PERFORMANCE-APP.md, C15 to D12).

use crate::nav::{Action, Cursor};
use crate::play::{chain_nodes, devices_name, fraction, is_instrument, level_at, note_name};
use crate::setlist::update;
use crate::{InfoLine, PartState, SettingData, Shell};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// session.status: the MIDI inputs the devices list offers.
    Status,
    /// harmony.get: the Performance's key and the names of the scales.
    Harmony,
    /// Any edit: its refusal is said.
    Edit,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Cancel with nothing to drop: back to Play.
    Back,
    /// The insert chain, and the sound chooser.
    Chain,
    Sound,
    /// The on-screen playing keyboard for this input (empty: the built-in keys).
    Keys(String),
    /// A press on the name: the on-screen keyboard, with this title and text; `entered` takes the result.
    Prompt(String, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Name,
    Sound,
    Effects,
    Level,
    Pan,
    Mute,
    Solo,
    Output,
    Key,
    Scale,
    Devices,
    Channel,
    LowKey,
    HighKey,
    LowVelocity,
    HighVelocity,
    Transpose,
    MaxNotes,
    Reserve,
    Priority,
    Keyboard,
    Pedal(usize),
    MidiOut,
    MidiChannel,
    Send(u64),
    Remove,
}

/// The two lists of devices: where the part plays from, and the outputs its MIDI goes to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Menu {
    Devices,
    MidiOut,
}

/// Notes at once and reserved notes, as the JUCE menus offer them (0: any, none).
const NOTE_CHOICES: [i64; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 24, 32, 64];

const PEDALS: [&str; 3] = ["sustain", "sostenuto", "soft"];
const PEDAL_NAMES: [&str; 3] = ["Sustain pedal", "Sostenuto pedal", "Soft pedal"];
/// A pedal's receive as the file says it and the JUCE page words it.
const RECEIVE: [(&str, &str); 3] = [("all", "Every pedal"), ("none", "None"), ("own", "Its devices' pedals")];
const TRANSPOSE: i64 = 24;
/// The twelve roots, as the pad editor names them.
const ROOTS: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
/// A key with no scale name (the Performance's before harmony.get answers): major.
const MAJOR: [i64; 7] = [0, 2, 4, 5, 7, 9, 11];

/// A key as the pad editor names it: "D Minor", else "D, custom" for a scale with no name.
fn key_text(key: &Value) -> String {
    let root = ROOTS[key["root"].as_u64().unwrap_or(0) as usize % 12];
    match key["name"].as_str() {
        Some(name) => format!("{root} {name}"),
        None => format!("{root}, custom"),
    }
}

/// A level in dB, as the JUCE page's label has it.
fn db(level: f64) -> String {
    if level > 0. { format!("{:.1} dB", 20. * level.log10()).replace('-', "−") } else { "−∞ dB".into() }
}

pub(crate) fn pan_text(pan: f64) -> String {
    let n = (pan.abs() * 100.).round() as i64;
    match n {
        0 => "Center".into(),
        _ => format!("{} {n}", if pan < 0. { "L" } else { "R" }),
    }
}

fn signed(n: i64) -> String {
    match n {
        0 => "0".into(),
        n if n > 0 => format!("+{n}"),
        n => format!("−{}", -n),
    }
}

/// The cursor fitted to `count` items, `per_page` a view, the view following it.
fn fit(c: Cursor, count: usize, per_page: usize) -> Cursor {
    let c = Cursor { count, per_page, at: c.at.map(|a| a.min(count.saturating_sub(1))), ..c };
    match c.at {
        Some(at) => c.to(at),
        None => Cursor { first: if c.first >= count { 0 } else { c.first / per_page * per_page }, ..c },
    }
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    graph: Value,
    id: u64,
    /// The part as graph.get gives it, with the edits made since: successive dial steps build on each other.
    part: Value,
    /// session.status's midiInputs and midiOutputs.
    inputs: Vec<Value>,
    outputs: Vec<Value>,
    /// harmony.get: the Performance's key (`root`, `scale`, `name`; null until read) and the scales' names.
    perf_key: Value,
    scales: Vec<String>,
    cursor: Cursor,
    /// The dial changes the value under the cursor.
    editing: bool,
    /// A devices list, open: which, and its own cursor.
    menu: Option<(Menu, Cursor)>,
    /// Remove part was pressed once: the next press removes it.
    removing: bool,
    /// The name's keyboard is open.
    asking: bool,
    /// This is the screen shown: the chain, sounds and parameter screens share the window's list.
    active: bool,
    /// The read-only panel's own cursor: its view turns a page at a time.
    info_cursor: Cursor,
    rows: Rc<VecModel<SettingData>>,
    info: Rc<VecModel<InfoLine>>,
    /// The profile offers the on-screen playing keyboard (a "Keyboard" row).
    pub keys: bool,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(window: &Shell) -> Screen {
        let rows = Rc::new(VecModel::default());
        let info = Rc::new(VecModel::default());
        let state = window.global::<PartState>();
        state.set_rows(ModelRc::from(rows.clone()));
        state.set_info(ModelRc::from(info.clone()));
        Screen { ui: window.as_weak(), graph: Value::Null, id: 0, part: Value::Null, inputs: vec![], outputs: vec![], perf_key: Value::Null, scales: vec![], removing: false, asking: false, active: false, cursor: Cursor::new(0, 1), editing: false, menu: None, info_cursor: Cursor::new(0, 1), rows, info, keys: true, out: vec![] }
    }

    /// The input that plays this part, for its keyboard: the first device rule that names a connected
    /// input; else the built-in keys (empty).
    fn keys_input(&self) -> String {
        let named = self.part["input"]["devices"].as_array().into_iter().flatten().filter(|r| matches!(r["kind"].as_str(), Some("usbNamed" | "network"))).filter_map(|r| r["pattern"].as_str());
        for pattern in named {
            if let Some(e) = self.inputs.iter().find(|e| e["name"].as_str().is_some_and(|n| n.to_lowercase().contains(&pattern.to_lowercase()))) {
                return e["name"].as_str().unwrap_or_default().to_string();
            }
        }
        String::new()
    }

    /// The part to show, from the graph already read.
    /// The cursor is on the first row, at rest: moving up leaves for the top bar's actions.
    pub fn at_top(&self) -> bool {
        self.cursor.at == Some(0) && !self.editing && self.menu.is_none()
    }

    /// The part's name, for the bar's title.
    pub fn name(&self) -> String {
        self.part["name"].as_str().unwrap_or_default().to_string()
    }

    /// The list the part's settings give way to (devices, outputs), named for the bar's title.
    pub fn menu_name(&self) -> Option<&'static str> {
        self.menu.map(|(m, _)| if m == Menu::Devices { "Plays from" } else { "MIDI out" })
    }

    /// Back from anywhere on the screen: a list of devices closes, else the screen goes. What is being
    /// changed is applied already, so nothing is lost.
    pub fn back(&mut self) -> Outcome {
        self.removing = false;
        self.editing = false;
        self.cursor = self.cursor.clear();
        if self.menu.take().is_some() {
            self.show();
            return Outcome::Stay;
        }
        Outcome::Back
    }

    pub fn open(&mut self, id: u32) {
        self.id = id as u64;
        self.active = true;
        self.cursor = Cursor::new(0, 1);
        self.editing = false;
        self.menu = None;
        self.removing = false;
        self.asking = false;
        self.info_cursor = Cursor::new(0, 1);
        self.say(String::new());
        self.load();
        self.show();
    }

    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
        self.load();
        self.show();
    }

    fn load(&mut self) {
        self.part = self.graph["parts"].as_array().and_then(|p| p.iter().find(|p| p["id"].as_u64() == Some(self.id))).cloned().unwrap_or(Value::Null);
    }

    /// controls.changed: this part's level, pan and mute moved live (its level and pan not while the dial has them).
    pub fn controls(&mut self, params: &Value) {
        if !self.part.is_object() {
            return;
        }
        for c in params["parts"].as_array().into_iter().flatten().filter(|c| c["part"].as_u64() == Some(self.id)) {
            for key in ["level", "pan"] {
                if let (false, Some(v)) = (self.editing, c[key].as_f64()) {
                    self.part[key] = json!(v);
                }
            }
            if let Some(m) = c["mute"].as_bool() {
                self.part["mute"] = json!(m);
            }
        }
        self.show();
    }

    /// session.status, as a reply or a notification: the MIDI inputs, when it has them.
    pub fn endpoints(&mut self, status: &Value) {
        if let Some(inputs) = status["midiInputs"].as_array() {
            self.inputs = inputs.clone();
        }
        if let Some(outputs) = status["midiOutputs"].as_array() {
            self.outputs = outputs.clone();
        }
        if self.menu.is_some() {
            self.show();
        }
    }

    /// Whether an edit was refused: the shell then asks the graph again, which puts the page right.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Status, Ok(r)) => self.endpoints(&r),
            (Request::Harmony, Ok(r)) => {
                self.perf_key = r["key"].clone();
                self.scales = r["scales"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)).collect();
                self.show();
            }
            (Request::Edit, Err(e)) if e.code != 0 => {
                self.say(crate::client::refusal(&e));
                return true;
            }
            _ => {}
        }
        false
    }

    fn say(&self, text: String) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<PartState>().set_message(text.into());
        }
    }

    fn items(&self) -> Vec<Item> {
        if !self.part.is_object() {
            return vec![];
        }
        let mut items = vec![Item::Level, Item::Pan, Item::Mute, Item::Solo];
        let notes = self.part["input"]["kind"] == "notes";
        if notes {
            items.extend([
                Item::Devices,
                Item::LowKey,
                Item::HighKey,
                Item::Transpose,
                Item::Pedal(0),
                Item::Pedal(1),
                Item::Pedal(2),
                Item::Channel,
                Item::LowVelocity,
                Item::HighVelocity,
                Item::MaxNotes,
                Item::Reserve,
                Item::Priority,
            ]);
        }
        items.push(Item::Output);
        // Its own key and scale (RFC 0033), or the Performance's; the scale only with a key of its own.
        items.push(Item::Key);
        if self.part["key"].is_object() {
            items.push(Item::Scale);
        }
        if self.part["input"]["kind"] != "bus" {
            items.extend([Item::MidiOut, Item::MidiChannel]);
        }
        items.extend(self.buses().into_iter().map(|(id, _)| Item::Send(id)));
        if self.keys && self.part["input"]["kind"] != "bus" {
            items.push(Item::Keyboard);
        }
        items.extend([Item::Sound, Item::Effects, Item::Name, Item::Remove]);
        items
    }

    /// The buses a part can play to or send to: every bus but itself, as the JUCE page lists them.
    fn buses(&self) -> Vec<(u64, String)> {
        self.graph["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["input"]["kind"] == "bus" && p["id"].as_u64() != Some(self.id))
            .filter_map(|p| Some((p["id"].as_u64()?, p["name"].as_str().unwrap_or_default().to_string())))
            .collect()
    }

    /// A list's choices, as the JUCE page's menus: any, (for inputs) the kinds, then each endpoint; a rule is None for "any".
    fn options(&self, menu: Menu) -> Vec<(String, Option<Value>)> {
        let rule = |kind: &str, port: i64, pattern: &str, matches: &str| json!({"kind": kind, "port": port, "pattern": pattern, "match": matches});
        let mut options: Vec<(String, Option<Value>)> = vec![(if menu == Menu::Devices { "Any device" } else { "Off" }.into(), None)];
        if menu == Menu::Devices {
            for kind in ["builtIn", "external", "usb", "network"] {
                let r = rule(kind, 0, "", "contains");
                options.push((devices_name(&json!([r.clone()])), Some(r)));
            }
        }
        let endpoints = if menu == Menu::Devices { &self.inputs } else { &self.outputs };
        for e in endpoints {
            let name = e["name"].as_str().unwrap_or_default();
            let r = match e["kind"].as_str() {
                Some("builtIn") => rule("builtIn", 0, "", "contains"),
                Some("port") => rule("port", e["port"].as_i64().unwrap_or(0), "", "contains"),
                Some("network") => rule("network", 0, name, "equals"),
                _ => rule("usbNamed", 0, name, "equals"),
            };
            if options.iter().any(|(_, o)| o.as_ref() == Some(&r)) {
                continue;
            }
            let label = if e["kind"] == "port" { format!("{} ({name})", devices_name(&json!([r.clone()]))) } else { name.to_string() };
            options.push((label, Some(r)));
        }
        if menu == Menu::MidiOut {
            // Rules naming outputs that are not connected stay on the list, to untick.
            for r in self.part["midiOut"]["devices"].as_array().into_iter().flatten() {
                if !options.iter().any(|(_, o)| o.as_ref() == Some(r)) {
                    options.push((format!("{} (not connected)", devices_name(&json!([r.clone()]))), Some(r.clone())));
                }
            }
        }
        options
    }

    /// The rules the list ticks: the part's input devices, or its MIDI out's.
    fn rules(&self, menu: Menu) -> Vec<Value> {
        let key = if menu == Menu::Devices { &self.part["input"]["devices"] } else { &self.part["midiOut"]["devices"] };
        key.as_array().cloned().unwrap_or_default()
    }

    fn zone(&self) -> (i64, i64, i64) {
        let z = &self.part["input"]["zones"][0];
        (z["keys"][0].as_i64().unwrap_or(0), z["keys"][1].as_i64().unwrap_or(127), z["transpose"].as_i64().unwrap_or(0))
    }

    fn pedal(&self, i: usize) -> usize {
        RECEIVE.iter().position(|r| self.part["input"]["receive"][PEDALS[i]] == r.0).unwrap_or(0)
    }

    /// The first zone's velocities as MIDI values.
    fn velocities(&self) -> (i64, i64) {
        let z = &self.part["input"]["zones"][0];
        let v = |i: usize, d: f64| (z["velocities"][i].as_f64().unwrap_or(d) * 127.).round() as i64;
        (v(0, 0.), v(1, 1.))
    }

    /// The MIDI channel the zones take: 1 to 16, 0 any, -1 more than one, as the JUCE page's zoneChannel.
    fn channel(&self) -> i64 {
        let zones = self.part["input"]["zones"].as_array().cloned().unwrap_or_default();
        let mask = |z: &Value| z["channels"].as_u64().unwrap_or(65535);
        let first = zones.first().map_or(65535, mask);
        if zones.iter().any(|z| mask(z) != first) {
            -1
        } else if first == 65535 {
            0
        } else if first.is_power_of_two() && first < 65536 {
            first.trailing_zeros() as i64 + 1
        } else {
            -1
        }
    }

    fn input_number(&self, key: &str) -> i64 {
        self.part["input"][key].as_i64().unwrap_or(0)
    }

    /// The values a dial steps through for a choice, and the one now; None for an item that is no choice.
    fn choices(&self, item: Item) -> Option<(Vec<i64>, i64)> {
        let max = self.input_number("maxNotes");
        Some(match item {
            Item::Channel => ((0..=16).collect(), self.channel().max(0)),
            Item::MaxNotes => (std::iter::once(0).chain(NOTE_CHOICES).collect(), max),
            Item::Reserve => (std::iter::once(0).chain(NOTE_CHOICES.into_iter().filter(|n| max == 0 || *n <= max)).collect(), self.input_number("reserve")),
            Item::Priority => ((0..=10).collect(), self.input_number("priority")),
            Item::MidiChannel => ((0..=16).collect(), self.part["midiOut"]["channel"].as_i64().unwrap_or(0)),
            Item::Output => (std::iter::once(0).chain(self.buses().into_iter().map(|(id, _)| id as i64)).collect(), self.part["bus"].as_i64().unwrap_or(0)),
            // Follow the Performance's key (-1), or a root.
            Item::Key => (std::iter::once(-1).chain(0..12).collect(), self.part["key"]["root"].as_i64().unwrap_or(-1)),
            // The scales by name; -1 stands for the one it has now when that is none of them.
            Item::Scale => {
                let at = self.scales.iter().position(|n| self.part["key"]["name"].as_str() == Some(n));
                (if at.is_none() { std::iter::once(-1).chain(0..self.scales.len() as i64).collect() } else { (0..self.scales.len() as i64).collect() }, at.map_or(-1, |a| a as i64))
            }
            _ => return None,
        })
    }

    fn choice_text(&self, item: Item, value: i64) -> String {
        let channel = |zero: &str| if value == 0 { zero.to_string() } else { format!("Channel {value}") };
        match item {
            Item::Channel if self.channel() < 0 => "Several channels".into(),
            Item::Channel => channel("Any channel"),
            Item::MaxNotes if value == 0 => "Any".into(),
            Item::Reserve if value == 0 => "None".into(),
            Item::MaxNotes | Item::Reserve => value.to_string(),
            Item::Priority => match value {
                0 => "0 (gives way first)".into(),
                10 => "10 (gives way last)".into(),
                n => n.to_string(),
            },
            Item::MidiChannel => channel("Own channels"),
            Item::Key if value < 0 => match self.perf_key["root"].as_u64() {
                Some(_) => format!("Performance's ({})", key_text(&self.perf_key)),
                None => "Performance's".into(),
            },
            Item::Key => ROOTS[value as usize % 12].into(),
            Item::Scale if value < 0 => format!("Custom ({})", self.part["key"]["scale"].as_array().into_iter().flatten().filter_map(Value::as_i64).map(|n| n.to_string()).collect::<Vec<_>>().join(" ")),
            Item::Scale => self.scales.get(value as usize).cloned().unwrap_or_default(),
            Item::Output if value == 0 => "Main".into(),
            Item::Output => self.buses().into_iter().find(|(id, _)| *id as i64 == value).map_or_else(|| "a bus".into(), |b| b.1),
            _ => String::new(),
        }
    }

    fn send_level_of(&self, bus: u64) -> f64 {
        self.part["sends"].as_array().into_iter().flatten().find(|s| s["bus"].as_u64() == Some(bus)).and_then(|s| s["level"].as_f64()).unwrap_or(0.)
    }

    fn setting(&self, item: Item) -> SettingData {
        let level = self.part["level"].as_f64().unwrap_or(1.);
        let (low, high, transpose) = self.zone();
        let (low_velocity, high_velocity) = self.velocities();
        let flag = |key: &str| self.part[key].as_bool().unwrap_or(false);
        let choice = |item: Item| {
            let (_, now) = self.choices(item).unwrap_or_default();
            self.choice_text(item, now)
        };
        let (label, value, kind, on): (String, String, i32, bool) = match item {
            Item::Name => ("Name".into(), format!("{} ›", self.part["name"].as_str().unwrap_or_default()), 2, false),
            Item::Level => ("Level".into(), db(level), 0, false),
            Item::Pan => ("Pan".into(), pan_text(self.part["pan"].as_f64().unwrap_or(0.)), 0, false),
            Item::Mute => ("Mute".into(), if flag("mute") { "On" } else { "Off" }.into(), 1, flag("mute")),
            Item::Solo => ("Solo".into(), if flag("solo") { "On" } else { "Off" }.into(), 1, flag("solo")),
            Item::Output => ("Plays to".into(), choice(item), 0, false),
            Item::Key => ("Key".into(), choice(item), 0, false),
            Item::Scale => ("Scale".into(), choice(item), 0, false),
            Item::Devices => ("Plays from".into(), devices_name(&self.part["input"]["devices"]) + " ›", 3, false),
            Item::Channel => ("Input channel".into(), choice(item), 0, false),
            Item::LowKey => ("Lowest key".into(), note_name(low), 0, false),
            Item::HighKey => ("Highest key".into(), note_name(high), 0, false),
            Item::LowVelocity => ("Lowest velocity".into(), low_velocity.to_string(), 0, false),
            Item::HighVelocity => ("Highest velocity".into(), high_velocity.to_string(), 0, false),
            Item::Transpose => ("Transpose".into(), signed(transpose), 0, false),
            Item::MaxNotes => ("Notes at once".into(), choice(item), 0, false),
            Item::Reserve => ("Reserved notes".into(), choice(item), 0, false),
            Item::Priority => ("Priority".into(), choice(item), 0, false),
            Item::Pedal(i) => (PEDAL_NAMES[i].into(), RECEIVE[self.pedal(i)].1.into(), 0, false),
            Item::MidiOut => ("MIDI out".into(), self.midi_out_name() + " ›", 3, false),
            Item::MidiChannel => ("MIDI out channel".into(), choice(item), 0, false),
            Item::Send(bus) => {
                let name = self.buses().into_iter().find(|b| b.0 == bus).map(|b| b.1).unwrap_or_default();
                (format!("Send to {name}"), db(self.send_level_of(bus)), 0, false)
            }
            Item::Keyboard => ("Keyboard".into(), "plays this part ›".into(), 2, false),
            Item::Sound => ("Sound".into(), self.sound_name() + " ›", 2, false),
            Item::Effects => ("Insert effects".into(), "›".into(), 2, false),
            Item::Remove => ("Remove part".into(), if self.removing { "Press again to remove" } else { "Press twice" }.into(), 2, self.removing),
        };
        SettingData { label: label.into(), value: value.into(), kind, on, cursor: false, editing: false, switch: false, ..Default::default() }
    }

    /// The part's sound as the panel names it: the first instrument.
    fn sound_name(&self) -> String {
        let nodes: HashMap<u64, &Value> = self.graph["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_u64()?, n))).collect();
        let mut flat = Vec::new();
        chain_nodes(&self.part["chain"], &self.part["splits"], 0, &mut flat);
        flat.iter().find_map(|n| nodes.get(n).filter(|n| is_instrument(n)).and_then(|n| n["name"].as_str())).unwrap_or("No instrument").to_string()
    }

    pub fn id(&self) -> u32 {
        self.id as u32
    }

    /// "Off" for no outputs, else the outputs by name, "(not connected)" when none of the rules finds one.
    fn midi_out_name(&self) -> String {
        let rules = self.rules(Menu::MidiOut);
        if rules.is_empty() {
            return "Off".into();
        }
        let name = devices_name(&Value::Array(rules.clone()));
        let found = self.options(Menu::MidiOut).iter().any(|(label, o)| o.as_ref().is_some_and(|r| rules.contains(r)) && !label.ends_with("(not connected)"));
        if found { name } else { format!("{name} (not connected)") }
    }

    /// What the part is made of, read only: its sound and engine, where it plays, its sends and effects.
    fn info(&self) -> Vec<InfoLine> {
        if !self.part.is_object() {
            return vec![];
        }
        let nodes: HashMap<u64, &Value> = self.graph["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_u64()?, n))).collect();
        let mut flat = Vec::new();
        chain_nodes(&self.part["chain"], &self.part["splits"], 0, &mut flat);
        let instrument = flat.iter().copied().find(|n| nodes.get(n).is_some_and(|n| is_instrument(n)));
        let text = |n: Option<&&Value>, key: &str| n.and_then(|n| n[key].as_str()).unwrap_or_default().to_string();
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head("Sound")];
        lines.push(line(instrument.map(|i| text(nodes.get(&i), "name")).unwrap_or_else(|| "No instrument".into())));
        if let Some(from) = self.part["from"]["name"].as_str().filter(|n| !n.is_empty()) {
            lines.push(line(format!("From “{from}”{}", if self.part["from"]["modified"] == true { ", edited" } else { "" })));
        }
        if let Some(i) = instrument {
            lines.extend([head("Engine"), line(nodes.get(&i).map_or_else(String::new, |n| n["plugin"]["name"].as_str().unwrap_or_default().to_string()))]);
        }
        let bus_name = |id: u64| self.graph["parts"].as_array().and_then(|p| p.iter().find(|p| p["id"].as_u64() == Some(id))).and_then(|p| p["name"].as_str()).unwrap_or("a bus").to_string();
        let through = self.part["bus"].as_u64().unwrap_or(0);
        lines.extend([head("Plays to"), line(if through == 0 { "Main".into() } else { bus_name(through) })]);
        lines.push(head("Sends"));
        let sends: Vec<String> = self.part["sends"].as_array().into_iter().flatten().map(|s| format!("{}  {}", bus_name(s["bus"].as_u64().unwrap_or(0)), db(s["level"].as_f64().unwrap_or(0.)))).collect();
        lines.extend(if sends.is_empty() { vec![line("None".into())] } else { sends.into_iter().map(line).collect() });
        lines.push(head("Insert effects"));
        let effects: Vec<String> = flat
            .iter()
            .filter(|n| Some(**n) != instrument)
            .map(|n| {
                let node = nodes.get(n);
                let off = node.is_some_and(|n| n["enabled"] == false);
                format!("{}{}", text(node, "name"), if off { " (off)" } else { "" })
            })
            .collect();
        lines.extend(if effects.is_empty() { vec![line("None".into())] } else { effects.into_iter().map(line).collect() });
        lines
    }

    /// Cursors and the view's size, then the window's view of it, each row set only when it changed.
    fn show(&mut self) {
        let Some(ui) = self.ui.upgrade().filter(|_| self.active) else { return };
        let state = ui.global::<PartState>();
        let per_view = state.get_rows_per_view().max(1) as usize;
        let items = self.items();
        self.cursor = fit(self.cursor, items.len(), per_view);
        let options = self.menu.map_or_else(Vec::new, |(m, _)| self.options(m));
        if let Some((m, c)) = self.menu {
            self.menu = Some((m, fit(c, options.len(), per_view)));
        }
        let (rows, cursor): (Vec<SettingData>, Cursor) = match self.menu {
            Some((m, c)) => {
                let rules = self.rules(m);
                let ticked = |o: &Option<Value>| match o {
                    None => rules.is_empty(),
                    Some(r) => rules.contains(r),
                };
                (options.iter().map(|(label, o)| SettingData { label: label.as_str().into(), value: if ticked(o) { "✓" } else { "" }.into(), kind: 1, on: ticked(o), ..Default::default() }).collect(), c)
            }
            None => (items.iter().map(|i| self.setting(*i)).collect(), self.cursor),
        };
        let end = rows.len().min(cursor.first + per_view);
        let shown: Vec<SettingData> = rows
            .into_iter()
            .enumerate()
            .skip(cursor.first)
            .take(per_view)
            .map(|(i, r)| SettingData { cursor: cursor.at == Some(i), editing: self.editing && self.menu.is_none() && cursor.at == Some(i), ..r })
            .collect();
        update(&self.rows, shown);
        let lines = self.info();
        let per = state.get_info_per_view().max(1) as usize;
        self.info_cursor = fit(self.info_cursor, lines.len(), per);
        state.set_info_more(lines.len() > per);
        update(&self.info, lines.into_iter().skip(self.info_cursor.first).take(per).collect());
        state.set_name(self.part["name"].as_str().unwrap_or_default().into());
        state.set_title(self.menu_name().unwrap_or_default().into());
        let what = match self.menu {
            Some((Menu::Devices, _)) => "Devices",
            Some((Menu::MidiOut, _)) => "Outputs",
            None => "Settings",
        };
        state.set_view(if cursor.count == 0 { String::new() } else { format!("{what} {}–{} of {}", cursor.first + 1, end, cursor.count) }.into());
    }

    /// An edit, followed by `graph.commit` as every JUCE setter does (what the engine stages is
    /// compiled; after a live one, a fader or a pan, it compiles nothing).
    fn edit(&mut self, method: &'static str, params: Value) {
        self.say(String::new());
        self.out.push((Request::Edit, method, params));
        self.out.push((Request::Edit, "graph.commit", json!({})));
    }

    fn send_input(&mut self) {
        let params = json!({"part": self.id, "input": self.part["input"].clone()});
        self.edit("parts.input", params);
    }

    fn send_midi_out(&mut self) {
        let params = json!({"part": self.id, "midiOut": self.part["midiOut"].clone()});
        self.edit("parts.midiOut", params);
    }

    fn send_level(&mut self) {
        let params = json!({"part": self.id, "level": self.part["level"].as_f64().unwrap_or(1.), "mute": self.part["mute"].as_bool().unwrap_or(false)});
        self.edit("parts.level", params);
    }

    /// A change to the first zone; a zone that passes everything is no zone, as the JUCE page's editZone has it.
    fn edit_zone(&mut self, change: impl FnOnce(&mut Value)) {
        let list = self.part["input"]["zones"].as_array().cloned().unwrap_or_default();
        self.part["input"]["zones"] = Value::Array(edit_zones(list, change));
    }

    /// The first zone's keys and transpose set, the highest key never under the lowest.
    fn set_zone(&mut self, low: i64, high: i64, transpose: i64) {
        self.edit_zone(|z| {
            z["keys"] = json!([low, high]);
            z["transpose"] = json!(transpose);
        });
    }

    fn set_velocities(&mut self, low: i64, high: i64) {
        self.edit_zone(|z| z["velocities"] = json!([low as f64 / 127., high as f64 / 127.]));
    }

    /// The MIDI channel every zone takes (0 any), as the JUCE page's zoneChannels: zones left identical
    /// are one zone, and one that passes everything goes.
    fn set_channel(&mut self, channel: i64) {
        let mask = if channel == 0 { 65535 } else { 1u64 << (channel - 1) };
        let mut zones = self.part["input"]["zones"].as_array().cloned().unwrap_or_default();
        if zones.is_empty() && channel != 0 {
            zones.push(default_zone());
        }
        for z in &mut zones {
            z["channels"] = json!(mask);
        }
        let mut kept: Vec<Value> = Vec::new();
        for z in zones {
            if !kept.contains(&z) {
                kept.push(z);
            }
        }
        if channel == 0 && kept.len() == 1 && kept[0] == default_zone() {
            kept.clear();
        }
        self.part["input"]["zones"] = Value::Array(kept);
    }

    /// The value under the cursor stepped `by`, sent when it changed.
    fn dial(&mut self, item: Item, by: i64) {
        let (low, high, transpose) = self.zone();
        let (low_velocity, high_velocity) = self.velocities();
        match item {
            Item::Level => {
                let was = self.part["level"].as_f64().unwrap_or(1.);
                let level = stepped_level(was, by);
                if level != was {
                    self.part["level"] = json!(level);
                    self.send_level();
                }
            }
            Item::Pan => {
                let was = self.part["pan"].as_f64().unwrap_or(0.);
                let pan = ((was + 0.05 * by as f64).clamp(-1., 1.) * 100.).round() / 100.;
                if pan != was {
                    self.part["pan"] = json!(pan);
                    let params = json!({"part": self.id, "pan": pan});
                    self.edit("parts.pan", params);
                }
            }
            Item::LowKey | Item::HighKey | Item::Transpose => {
                let (l, h, t) = match item {
                    Item::LowKey => {
                        let l = (low + by).clamp(0, 127);
                        (l, high.max(l), transpose)
                    }
                    Item::HighKey => (low, (high + by).clamp(low, 127), transpose),
                    _ => (low, high, (transpose + by).clamp(-TRANSPOSE, TRANSPOSE)),
                };
                if (l, h, t) != (low, high, transpose) {
                    self.set_zone(l, h, t);
                    self.send_input();
                }
            }
            Item::LowVelocity | Item::HighVelocity => {
                let (l, h) = if item == Item::LowVelocity {
                    let l = (low_velocity + by).clamp(0, 127);
                    (l, high_velocity.max(l))
                } else {
                    (low_velocity, (high_velocity + by).clamp(low_velocity, 127))
                };
                if (l, h) != (low_velocity, high_velocity) {
                    self.set_velocities(l, h);
                    self.send_input();
                }
            }
            Item::Pedal(i) => {
                let to = (self.pedal(i) as i64 + by).clamp(0, 2) as usize;
                if to != self.pedal(i) {
                    if !self.part["input"]["receive"].is_object() {
                        self.part["input"]["receive"] = json!({});
                    }
                    let receive = &mut self.part["input"]["receive"];
                    if to == 0 {
                        receive.as_object_mut().map(|r| r.remove(PEDALS[i]));
                    } else {
                        receive[PEDALS[i]] = json!(RECEIVE[to].0);
                    }
                    self.send_input();
                }
            }
            Item::Channel | Item::MaxNotes | Item::Reserve | Item::Priority | Item::MidiChannel | Item::Output | Item::Key | Item::Scale => self.dial_choice(item, by),
            Item::Send(bus) => {
                let was = self.send_level_of(bus);
                let level = stepped_level(was, by);
                if level != was {
                    let mut sends = self.part["sends"].as_array().cloned().unwrap_or_default();
                    sends.retain(|s| s["bus"].as_u64() != Some(bus));
                    // Without a level the send is removed, as the JUCE page's setSendLevel does at 0.
                    let params = if level > 0. {
                        sends.push(json!({"bus": bus, "level": level}));
                        json!({"part": self.id, "bus": bus, "level": level})
                    } else {
                        json!({"part": self.id, "bus": bus})
                    };
                    self.part["sends"] = Value::Array(sends);
                    self.edit("parts.send", params);
                }
            }
            Item::Mute | Item::Solo | Item::Devices | Item::MidiOut | Item::Name | Item::Remove | Item::Sound | Item::Effects | Item::Keyboard => {}
        }
    }

    /// A choice stepped along its values.
    fn dial_choice(&mut self, item: Item, by: i64) {
        let Some((values, now)) = self.choices(item) else { return };
        let at = values.iter().position(|v| *v == now).map_or_else(|| values.iter().position(|v| *v >= now).unwrap_or(values.len() - 1) as i64, |p| p as i64);
        let to = values[(at + by).clamp(0, values.len() as i64 - 1) as usize];
        if to == now {
            return;
        }
        match item {
            Item::Channel => {
                self.set_channel(to);
                self.send_input();
            }
            Item::MaxNotes => {
                self.part["input"]["maxNotes"] = json!(to);
                if to > 0 && self.input_number("reserve") > to {
                    self.part["input"]["reserve"] = json!(to);
                }
                self.send_input();
            }
            Item::Reserve => {
                self.part["input"]["reserve"] = json!(to);
                self.send_input();
            }
            Item::Priority => {
                self.part["input"]["priority"] = json!(to);
                self.send_input();
            }
            // Disabled while no output is chosen, as the JUCE menu is.
            Item::MidiChannel if !self.rules(Menu::MidiOut).is_empty() => {
                self.part["midiOut"]["channel"] = json!(to);
                self.send_midi_out();
            }
            Item::Key if to < 0 => {
                self.part["key"] = Value::Null; // follows the Performance's again
                self.edit("harmony.set", json!({"part": self.id, "key": null}));
            }
            Item::Key => {
                // A root of its own, in the scale it had (else the Performance's, else major).
                let own = self.part["key"].is_object();
                let was = if own { self.part["key"].clone() } else { self.perf_key.clone() };
                let scale = match was["name"].as_str() {
                    Some(name) => json!(name),
                    None => was["scale"].as_array().filter(|a| !a.is_empty()).map_or_else(|| json!(MAJOR), |a| json!(a)),
                };
                let key = json!({"root": to, "scale": scale});
                self.part["key"] = json!({"root": to, "scale": was["scale"].clone(), "name": was["name"].clone()});
                self.edit("harmony.set", json!({"part": self.id, "key": key}));
            }
            Item::Scale => {
                let Some(name) = self.scales.get(to.max(0) as usize).cloned() else { return };
                let root = self.part["key"]["root"].as_i64().unwrap_or(0);
                self.part["key"] = json!({"root": root, "scale": [], "name": name});
                self.edit("harmony.set", json!({"part": self.id, "key": {"root": root, "scale": name}}));
            }
            Item::Output => {
                self.part["bus"] = json!(to);
                let params = if to == 0 { json!({"part": self.id}) } else { json!({"part": self.id, "bus": to}) };
                self.edit("parts.output", params);
            }
            _ => {}
        }
    }

    fn toggle(&mut self, menu: Menu, option: Option<Value>) {
        let mut rules = self.rules(menu);
        match option {
            None => rules.clear(),
            Some(r) => match rules.iter().position(|x| x == &r) {
                Some(p) => {
                    rules.remove(p);
                }
                None => rules.push(r),
            },
        }
        match menu {
            Menu::Devices => {
                self.part["input"]["devices"] = Value::Array(rules);
                self.send_input();
            }
            Menu::MidiOut => {
                if !self.part["midiOut"].is_object() {
                    self.part["midiOut"] = json!({"channel": 0});
                }
                self.part["midiOut"]["devices"] = Value::Array(rules);
                self.send_midi_out();
            }
        }
    }

    /// A press on the item under the cursor.
    fn confirm(&mut self) -> Outcome {
        if let Some((menu, c)) = self.menu {
            if let Some(o) = c.at.and_then(|at| self.options(menu).into_iter().nth(at)) {
                self.toggle(menu, o.1);
            }
            return Outcome::Stay;
        }
        let Some(item) = self.cursor.at.and_then(|at| self.items().get(at).copied()) else { return Outcome::Stay };
        if item != Item::Remove {
            self.removing = false;
        }
        match item {
            Item::Mute => {
                let on = !self.part["mute"].as_bool().unwrap_or(false);
                self.part["mute"] = json!(on);
                self.send_level();
            }
            Item::Solo => {
                let on = !self.part["solo"].as_bool().unwrap_or(false);
                self.part["solo"] = json!(on);
                let params = json!({"part": self.id, "solo": on});
                self.edit("parts.solo", params);
            }
            Item::Devices => self.menu = Some((Menu::Devices, Cursor::new(0, 1))),
            Item::MidiOut => self.menu = Some((Menu::MidiOut, Cursor::new(0, 1))),
            Item::Name => {
                self.asking = true;
                return Outcome::Prompt("Name of the part".into(), self.part["name"].as_str().unwrap_or_default().into());
            }
            Item::Sound => return Outcome::Sound,
            Item::Keyboard => return Outcome::Keys(self.keys_input()),
            Item::Effects => return Outcome::Chain,
            Item::Remove if !self.removing => self.removing = true,
            Item::Remove => {
                self.removing = false;
                let params = json!({"part": self.id});
                self.edit("parts.remove", params);
                return Outcome::Back;
            }
            _ => self.editing = !self.editing,
        }
        Outcome::Stay
    }

    /// The keyboard's text, after Done: the part's new name. Nothing typed keeps the name it has.
    pub fn entered(&mut self, text: &str) {
        self.asking = false;
        let text = text.trim();
        if text.is_empty() || self.part["name"].as_str() == Some(text) {
            return;
        }
        self.part["name"] = json!(text);
        let params = json!({"part": self.id, "name": text});
        self.edit("parts.name", params);
        self.show();
    }

    /// The keyboard closed without a name.
    pub fn not_entered(&mut self) {
        self.asking = false;
    }

    /// A navigation action. Any pane moves the one list: its cursor, or the value being changed.
    /// Keys: Up and Down move the cursor, Left and Right change a value, Home and End the first and
    /// last, Page Up and Down a view, Return confirms, Escape cancels.
    pub fn act(&mut self, action: Action) -> Outcome {
        let item = self.cursor.at.and_then(|at| self.items().get(at).copied());
        match action {
            // The right knob scrolls the read-only panel; the others, and the dial, the list.
            Action::Move { pane: Some(1), by } => self.info_cursor = self.info_cursor.moved(by as i64),
            Action::Move { by, .. } => self.step(by as i64, item),
            Action::Arrow { dx: 0, dy } => self.step(dy as i64, item),
            Action::Arrow { dx, .. } => match item.filter(|_| self.editing && self.menu.is_none()) {
                Some(item) => self.dial(item, dx as i64),
                None => self.step(dx as i64, item),
            },
            Action::Page(by) => self.shift(by as i64 * self.cursor.per_page as i64),
            Action::End(by) => self.shift(by as i64 * 10_000),
            Action::Confirm(_) => {
                let outcome = self.confirm();
                self.show();
                return outcome;
            }
            Action::Cancel(_) => {
                if self.menu.take().is_some() {
                    // the list closes
                } else if self.editing {
                    self.editing = false;
                } else if self.removing {
                    self.removing = false;
                } else if self.cursor.at.is_some() {
                    self.cursor = self.cursor.clear();
                } else {
                    return Outcome::Back;
                }
            }
        }
        self.show();
        Outcome::Stay
    }

    /// The dial or a knob by `by`: the value under the cursor when it is being changed, else the cursor.
    fn step(&mut self, by: i64, item: Option<Item>) {
        match item.filter(|_| self.editing && self.menu.is_none()) {
            Some(item) => self.dial(item, by),
            None => self.shift(by),
        }
    }

    fn shift(&mut self, by: i64) {
        match &mut self.menu {
            Some((_, c)) => *c = c.moved(by),
            None if !self.editing => {
                self.cursor = self.cursor.moved(by);
                self.removing = false;
            }
            None => {}
        }
    }

    /// The item of the row `row` of the view, as an index into the list shown.
    fn at(&self, row: i32) -> usize {
        self.menu.map_or(self.cursor, |m| m.1).first + row.max(0) as usize
    }

    /// A tap: the cursor to the row and what a press does; on a value, the dial then changes it.
    pub fn tap(&mut self, row: i32) -> Outcome {
        let at = self.at(row);
        match &mut self.menu {
            Some((_, c)) => *c = c.to(at),
            None => self.cursor = self.cursor.to(at),
        }
        self.editing = false;
        let outcome = self.confirm();
        self.show();
        outcome
    }

    /// The panel's up or down button: a view.
    pub fn scroll_info(&mut self, by: i32) {
        self.info_cursor = self.info_cursor.moved(by as i64 * self.info_cursor.per_page as i64);
        self.show();
    }

    /// - or + on a row: that value stepped.
    pub fn step_row(&mut self, row: i32, by: i32) {
        let at = self.at(row);
        if self.menu.is_none() {
            self.cursor = self.cursor.to(at);
            if let Some(item) = self.items().get(at).copied() {
                self.dial(item, by as i64);
            }
        }
        self.show();
    }

    /// The cursor drops when the screen goes, as on focus loss in the JUCE lists.
    pub fn hidden(&mut self) {
        self.cursor = self.cursor.clear();
        self.editing = false;
        self.menu = None;
        self.removing = false;
        self.active = false;
    }

    /// Shown: the view's size may have changed while it was not.
    pub fn shown(&mut self) {
        self.active = true;
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<PartState>();
            state.set_rows(ModelRc::from(self.rows.clone()));
            state.set_info(ModelRc::from(self.info.clone()));
        }
        self.show();
    }
}

/// A list of zones with a change to the first (a default zone when there is none); a zone that passes every
/// note is no zone, as `editZone` and `setLaneKeys` have it.
pub(crate) fn edit_zones(mut list: Vec<Value>, change: impl FnOnce(&mut Value)) -> Vec<Value> {
    let mut zone = list.first().cloned().unwrap_or_else(default_zone);
    change(&mut zone);
    let everything = zone == default_zone();
    if list.is_empty() {
        if !everything {
            list.push(zone);
        }
    } else if everything && list.len() == 1 {
        list.clear();
    } else {
        list[0] = zone;
    }
    list
}

/// A zone that passes every note, as `graph.get` words it.
pub(crate) fn default_zone() -> Value {
    json!({"keys": [0, 127], "velocities": [0, 1], "channels": 65535, "transpose": 0, "curve": 1, "fixedVelocity": 0})
}

/// A level stepped `by` fader steps, as the Play strip's fader moves.
fn stepped_level(was: f64, by: i64) -> f64 {
    let mut f = (fraction(was, 1.) + 0.02 * by as f32).clamp(0., 1.);
    let mut level = level_at(f, 1.);
    // Near silence a step is less than a thousandth: go on until it is heard.
    while level == was && f > 0. && f < 1. {
        f = (f + 0.02 * by.signum() as f32).clamp(0., 1.);
        level = level_at(f, 1.);
    }
    level
}
