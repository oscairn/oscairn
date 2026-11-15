// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Play screen (RFC 0011), the JUCE Performance app's first view (hosts/standalone/PlayView.cpp)
//! for the stage, on the shell: the scenes along the top (pane 0, the left knob), a strip per part
//! and bus below (pane 1, the right knob) with its name and sound, level fader, a meter per channel,
//! mute and solo, and the master pinned at the right. Everything is read from `graph.get` (asked by
//! the shell on each `graph.changed`), the levels and mutes that move live from `controls.changed`
//! (subscribed only while this screen is shown), and the meters from `parts.meters`, asked every
//! METER_INTERVAL while it is shown and never otherwise.
//!
//! Selecting a part (a press on it, a tap on its name) opens the Part screen (part.rs); mute and solo
//! stay on the strip's M and S and on that screen.
//!
//! Not here, handed to the JUCE app from the screen's "Edit" entry: the knob row and its banks,
//! Learn, Edit's other pages, adding a scene, the on-screen keyboard hint.

use crate::nav::{Action, Cursor};
use crate::setlist::update;
use crate::{PlayState, SceneData, Shell, StripData};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// How often the meters are asked while Play is shown (the JUCE view reads its engine's every 48 ms).
pub const METER_INTERVAL: Duration = Duration::from_millis(100);
const SCENES: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Meters,
    /// Any edit (a scene, a level, a mute, solo): its refusal is said.
    Edit,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Cancel with no cursor: back where Play was opened from.
    Back,
    /// Previous (-1) or Next (1) of the setlist.
    Step(i64),
    /// A part selected (a press on it): its Part screen.
    Open(u32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    pub id: u32,
    pub name: String,
    pub detail: String,
    pub level: f64,
    pub mute: bool,
    pub solo: bool,
    pub silenced: bool,
    /// Meter bars as shown: 0..1 along -60..+6 dB, in hundredths; the channels (0: none yet).
    pub peaks: (f32, f32),
    pub channels: i32,
}

const NOTES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

pub(crate) fn note_name(n: i64) -> String {
    if (0..=127).contains(&n) { format!("{}{}", NOTES[n as usize % 12], n / 12 - 1) } else { "–".into() }
}

/// The inputs a part takes notes from, as PartsModel.cpp's devicesName names them.
pub(crate) fn devices_name(rules: &Value) -> String {
    let Some(rules) = rules.as_array().filter(|r| !r.is_empty()) else { return "Any device".into() };
    let names: Vec<String> = rules
        .iter()
        .map(|r| {
            let pattern = r["pattern"].as_str().unwrap_or_default();
            let matched = |name: String| match r["match"].as_str() {
                Some("contains") => format!("“{pattern}”"),
                Some("regex") => format!("/{pattern}/"),
                _ => name,
            };
            match r["kind"].as_str() {
                Some("builtIn") => "Built in".into(),
                Some("external") => "All external".into(),
                Some("all") => "Any device".into(),
                Some("port") => format!("Port {}", r["port"].as_i64().unwrap_or(0)),
                Some("usb") => "All USB".into(),
                Some("usbNamed") if pattern.is_empty() => String::new(),
                Some("usbNamed") => matched(pattern.into()),
                Some("network") if pattern.is_empty() => "Network".into(),
                Some("network") => format!("Network {}", matched(pattern.into())),
                _ => String::new(),
            }
        })
        .collect();
    names.join(" + ")
}

/// What kind of box a node is, as PartsModel.cpp's roleOf: "Instrument" is a part's sound.
pub(crate) fn is_instrument(node: &Value) -> bool {
    let channels = |key: &str| node[key].as_array().map_or(0, |p| p.iter().map(|p| p["channels"].as_u64().unwrap_or(0)).sum::<u64>());
    let (i, o) = (channels("audioInputs"), channels("audioOutputs"));
    let ni = node["noteInputs"].as_u64().unwrap_or(0);
    o > 0 && i == 0 && ni > 0
}

/// The nodes of a chain in order, a split's lanes in place of it.
pub(crate) fn chain_nodes(chain: &Value, splits: &Value, depth: usize, out: &mut Vec<u64>) {
    for entry in chain.as_array().into_iter().flatten() {
        match entry.as_u64() {
            Some(node) => out.push(node),
            None if depth < 8 => {
                let split = splits.as_array().and_then(|s| s.iter().find(|s| s["id"] == entry["split"]));
                for lane in split.and_then(|s| s["lanes"].as_array()).into_iter().flatten() {
                    chain_nodes(&lane["chain"], splits, depth + 1, out);
                }
            }
            None => {}
        }
    }
}

fn input_text(p: &Value) -> String {
    let input = &p["input"];
    match input["kind"].as_str() {
        Some("notes") => {
            let zone = input["zones"].as_array().and_then(|z| z.first());
            let key = |i: usize| zone.and_then(|z| z["keys"][i].as_i64());
            let (low, high) = (key(0).unwrap_or(0), key(1).unwrap_or(127));
            let mut text = format!("{} · {}", devices_name(&input["devices"]), if low == 0 && high == 127 { "all keys".into() } else { format!("{}–{}", note_name(low), note_name(high)) });
            match zone.and_then(|z| z["transpose"].as_i64()).unwrap_or(0) {
                0 => {}
                t => text += &format!(" · {}{}", if t > 0 { "+" } else { "−" }, t.abs()),
            }
            text
        }
        Some("audio") => format!("Audio in {}", input["port"].as_i64().unwrap_or(0) + 1),
        Some("bus") => String::new(),
        _ => "Its own notes".into(),
    }
}

/// graph.get's parts and buses, as PartsModel.cpp's readParts gives each row: parts first, then buses.
pub fn parse_parts(graph: &Value) -> Vec<Part> {
    let nodes: HashMap<u64, &Value> = graph["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_u64()?, n))).collect();
    let name = |id: u64| nodes.get(&id).and_then(|n| n["name"].as_str()).unwrap_or_default().to_string();
    let wires = graph["wires"].as_array().cloned().unwrap_or_default();
    let mut parts = Vec::new();
    let mut buses = Vec::new();
    for p in graph["parts"].as_array().into_iter().flatten() {
        let id = p["id"].as_u64().unwrap_or(0) as u32;
        let bus = p["input"]["kind"] == "bus";
        let mut flat = Vec::new();
        chain_nodes(&p["chain"], &p["splits"], 0, &mut flat);
        let sound = flat.iter().find(|n| nodes.get(n).is_some_and(|n| is_instrument(n))).map(|n| name(*n)).unwrap_or_default();
        let input = input_text(p);
        let detail = if bus {
            "Bus".to_string() + &flat.first().map(|n| format!(" · {}", name(*n))).unwrap_or_default()
        } else {
            (if sound.is_empty() { "No instrument".to_string() } else { sound }) + &if input.is_empty() { String::new() } else { format!(" · {input}") }
        };
        let mute = p["mute"].as_bool().unwrap_or(false);
        // Another part's solo: the wires where it plays are muted though it is not.
        let silenced = !mute && wires.iter().any(|w| w["owner"] == "part" && w["part"].as_u64() == Some(id as u64) && w["fader"] == true && w["mute"] == true);
        let row = Part { id, name: p["name"].as_str().unwrap_or_default().into(), detail, level: p["level"].as_f64().unwrap_or(1.), mute, solo: p["solo"].as_bool().unwrap_or(false), silenced, ..Default::default() };
        if bus { buses.push(row) } else { parts.push(row) }
    }
    parts.extend(buses);
    parts
}

/// The fader's place for a level, through the JUCE faders' skew (a part's range is 0..1, the
/// master's 0..2, each with its midpoint a quarter of the way up: -12 dB halfway up the part's).
pub fn fraction(level: f64, max: f64) -> f32 {
    (level / max).clamp(0., 1.).sqrt() as f32
}

pub(crate) fn level_at(fraction: f32, max: f64) -> f64 {
    ((max * (fraction as f64).powi(2)) * 1000.).round() / 1000.
}

/// A peak (linear) along the meter's -60..+6 dB, in hundredths.
fn meter(peak: f64) -> f32 {
    let db = if peak > 0. { 20. * peak.log10() } else { -100. };
    (((db + 60.) / 66.).clamp(0., 1.) * 100.).round() as f32 / 100.
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    scenes: Vec<(u64, String)>,
    scene: u64,
    parts: Vec<Part>,
    master: Option<(f64, f64)>, // level, pan
    scene_cursor: Cursor,       // pane 0
    part_cursor: Cursor,        // pane 1, a view of the parts at a time
    /// The pane the dial moves: the one last touched.
    touched: u8,
    /// The part (-1 the master) whose fader a finger is on: the engine's own values for it wait.
    held: Option<i64>,
    follow: bool,
    tiles: Rc<VecModel<SceneData>>,
    rows: Rc<VecModel<StripData>>,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(window: &Shell) -> Screen {
        let tiles = Rc::new(VecModel::default());
        let rows = Rc::new(VecModel::default());
        let state = window.global::<PlayState>();
        state.set_scenes(ModelRc::from(tiles.clone()));
        state.set_strips(ModelRc::from(rows.clone()));
        Screen {
            ui: window.as_weak(),
            scenes: vec![],
            scene: 0,
            parts: vec![],
            master: None,
            scene_cursor: Cursor::new(0, SCENES),
            part_cursor: Cursor::new(0, 1),
            touched: 1,
            held: None,
            follow: false,
            tiles,
            rows,
            out: vec![],
        }
    }

    /// graph.get: the scenes, the scene playing, the parts and the master. The meters shown stay.
    pub fn graph(&mut self, graph: &Value) {
        self.scenes = graph["scenes"].as_array().into_iter().flatten().map(|s| (s["id"].as_u64().unwrap_or(0), s["name"].as_str().unwrap_or_default().to_string())).collect();
        self.scene = graph["scene"].as_u64().unwrap_or(0);
        let old: HashMap<u32, Part> = self.parts.drain(..).map(|p| (p.id, p)).collect();
        let held = self.held;
        self.parts = parse_parts(graph)
            .into_iter()
            .map(|mut p| {
                if let Some(was) = old.get(&p.id) {
                    p.peaks = was.peaks;
                    p.channels = was.channels;
                    if held == Some(p.id as i64) {
                        p.level = was.level;
                    }
                }
                p
            })
            .collect();
        if held != Some(-1) {
            self.master = graph["master"]["level"].as_f64().map(|l| (l, graph["master"]["pan"].as_f64().unwrap_or(0.)));
        }
        self.show();
    }

    /// controls.changed: levels and mutes moved live, and the master's.
    pub fn controls(&mut self, params: &Value) {
        for c in params["parts"].as_array().into_iter().flatten() {
            let Some(id) = c["part"].as_u64() else { continue };
            if let Some(p) = self.parts.iter_mut().find(|p| p.id as u64 == id) {
                if self.held != Some(id as i64) {
                    p.level = c["level"].as_f64().unwrap_or(p.level);
                }
                p.mute = c["mute"].as_bool().unwrap_or(p.mute);
            }
        }
        if self.held != Some(-1) {
            if let Some(level) = params["master"]["level"].as_f64() {
                self.master = Some((level, params["master"]["pan"].as_f64().unwrap_or(self.master.map_or(0., |m| m.1))));
            }
        }
        self.show();
    }

    /// parts.meters: each part's channels' recent peaks, the first bar and the loudest of the rest.
    pub fn meters(&mut self, result: &Value) {
        for m in result["meters"].as_array().into_iter().flatten() {
            let Some(p) = self.parts.iter_mut().find(|p| Some(p.id as u64) == m["part"].as_u64()) else { continue };
            let channels: Vec<f64> = m["channels"].as_array().map(|c| c.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let (a, b) = match channels.as_slice() {
                [] => (0., 0.),
                [a] => (*a, 0.),
                [a, rest @ ..] => (*a, rest.iter().copied().fold(0., f64::max)),
            };
            p.channels = channels.len().clamp(1, 2) as i32;
            p.peaks = (meter(a), meter(b));
        }
        self.show();
    }

    /// Whether the open Performance is the setlist's slot playing: Previous and Next show.
    pub fn follow(&mut self, follow: bool) {
        if self.follow != follow {
            self.follow = follow;
            self.show();
        }
    }

    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) {
        match (request, result) {
            (Request::Meters, Ok(r)) => self.meters(&r),
            (Request::Edit, Err(e)) if e.code != 0 => {
                self.say(format!("The engine refused: {}", e.message));
                self.show();
            }
            _ => {}
        }
    }

    fn say(&self, text: String) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<PlayState>().set_message(text.into());
        }
    }

    /// Cursors and the view's size, then the window's view of it, each row set only when it changed.
    fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let state = ui.global::<PlayState>();
        let per_view = state.get_strips_per_view().max(1) as usize;
        let scenes = self.scenes.len().min(SCENES);
        self.scene_cursor = Cursor { count: scenes, at: self.scene_cursor.at.map(|a| a.min(scenes.saturating_sub(1))), ..self.scene_cursor };
        self.part_cursor = Cursor { count: self.parts.len(), per_page: per_view, at: self.part_cursor.at.map(|a| a.min(self.parts.len().saturating_sub(1))), ..self.part_cursor };
        self.part_cursor = match self.part_cursor.at {
            Some(at) => self.part_cursor.to(at), // the view follows the cursor
            None => Cursor { first: if self.part_cursor.first >= self.parts.len() { 0 } else { self.part_cursor.first / per_view * per_view }, ..self.part_cursor },
        };
        let tiles: Vec<SceneData> = (0..SCENES)
            .map(|i| match self.scenes.get(i) {
                Some((id, name)) => SceneData { number: i as i32 + 1, name: name.as_str().into(), filled: true, on: *id == self.scene, cursor: self.scene_cursor.at == Some(i) },
                None => SceneData { number: i as i32 + 1, ..Default::default() },
            })
            .collect();
        update(&self.tiles, tiles);
        let first = self.part_cursor.first;
        let end = self.parts.len().min(first + per_view);
        let rows: Vec<StripData> = (first..end)
            .map(|i| {
                let p = &self.parts[i];
                StripData {
                    name: p.name.as_str().into(),
                    detail: p.detail.as_str().into(),
                    fraction: fraction(p.level, 1.),
                    mute: p.mute,
                    solo: p.solo,
                    dim: p.mute || p.silenced,
                    cursor: self.part_cursor.at == Some(i),
                    channels: p.channels,
                    peak_a: p.peaks.0,
                    peak_b: p.peaks.1,
                }
            })
            .collect();
        update(&self.rows, rows);
        let master = self.master.filter(|_| !self.parts.is_empty());
        state.set_has_master(master.is_some());
        if let Some((level, _)) = master {
            state.set_master(StripData { name: "Master".into(), detail: "Performance".into(), fraction: fraction(level, 2.), ..Default::default() });
        }
        state.set_can_step(self.follow);
        let view: SharedString = if self.parts.is_empty() { SharedString::new() } else { format!("Parts {}–{} of {}", first + 1, end, self.parts.len()).into() };
        state.set_view(view);
    }

    fn edit(&mut self, method: &'static str, params: Value) {
        self.out.push((Request::Edit, method, params));
    }

    /// An edit the JUCE strip commits (`c.commit()` after setPartLevel, setPartSolo and setMaster);
    /// the engine compiles nothing after a live one, but what is staged goes with it. scenes.select commits itself.
    fn edit_committed(&mut self, method: &'static str, params: Value) {
        self.edit(method, params);
        self.edit("graph.commit", json!({}));
    }

    fn select_scene(&mut self, i: usize) {
        if let Some((id, _)) = self.scenes.get(i).filter(|_| i < SCENES) {
            self.scene = *id; // shown now; graph.get confirms
            let id = *id;
            self.edit("scenes.select", json!({"scene": id}));
        }
    }

    fn toggle_mute(&mut self, i: usize) {
        if let Some(p) = self.parts.get_mut(i) {
            p.mute = !p.mute;
            let (id, level, mute) = (p.id, p.level, p.mute);
            self.edit_committed("parts.level", json!({"part": id, "level": level, "mute": mute}));
        }
    }

    fn toggle_solo(&mut self, i: usize) {
        if let Some(p) = self.parts.get_mut(i) {
            p.solo = !p.solo;
            let (id, solo) = (p.id, p.solo);
            self.edit_committed("parts.solo", json!({"part": id, "solo": solo}));
        }
    }

    /// The pane's cursor moved by `by` steps, shown on the first item by the first move.
    fn move_in(&mut self, pane: u8, by: i64) {
        self.touched = pane;
        if pane == 0 {
            self.scene_cursor = self.scene_cursor.moved(by);
        } else {
            self.part_cursor = self.part_cursor.moved(by);
        }
    }

    /// A navigation action. Pane 0 is the scenes, pane 1 the parts; the arrow keys: left and right
    /// move along the pane last touched, up the scenes, down the parts; Page Up and Down are the
    /// setlist's Previous and Next (page turners send them).
    pub fn act(&mut self, action: Action) -> Outcome {
        let pane = |p: Option<u8>, touched: u8| p.map_or(touched, |p| p.min(1));
        match action {
            Action::Move { pane: p, by } => self.move_in(pane(p, self.touched), by as i64),
            Action::Arrow { dx: 0, dy } => self.move_in(if dy < 0 { 0 } else { 1 }, 0),
            Action::Arrow { dx, .. } => self.move_in(self.touched, dx as i64),
            Action::Page(by) => {
                return if self.follow { Outcome::Step(by as i64) } else { Outcome::Stay };
            }
            Action::End(by) => self.move_in(self.touched, by as i64 * 10_000),
            Action::Confirm(p) => match pane(p, self.touched) {
                0 => {
                    if let Some(at) = self.scene_cursor.at {
                        self.select_scene(at);
                    }
                }
                _ => {
                    if let Some(p) = self.part_cursor.at.and_then(|at| self.parts.get(at)) {
                        return Outcome::Open(p.id);
                    }
                }
            },
            Action::Cancel(_) => {
                if self.scene_cursor.at.is_none() && self.part_cursor.at.is_none() {
                    return Outcome::Back;
                }
                self.scene_cursor = self.scene_cursor.clear();
                self.part_cursor = self.part_cursor.clear();
            }
        }
        self.show();
        Outcome::Stay
    }

    /// The row of the view `row` as an index into the parts.
    fn at(&self, row: i32) -> usize {
        self.part_cursor.first + row.max(0) as usize
    }

    pub fn tap_scene(&mut self, i: i32) {
        self.scene_cursor = self.scene_cursor.clear();
        self.select_scene(i.max(0) as usize);
        self.show();
    }

    /// A tap on a strip's name: that part selected, its Part screen.
    pub fn tap_open(&mut self, row: i32) -> Option<u32> {
        self.parts.get(self.at(row)).map(|p| p.id)
    }

    pub fn tap_mute(&mut self, row: i32) {
        self.part_cursor = self.part_cursor.clear();
        self.toggle_mute(self.at(row));
        self.show();
    }

    pub fn tap_solo(&mut self, row: i32) {
        self.part_cursor = self.part_cursor.clear();
        self.toggle_solo(self.at(row));
        self.show();
    }

    /// A fader moved (row -1: the master's) to `place` of its travel; sent when it moved a
    /// hundredth or more, so a drag is some tens of small requests, as the JUCE slider's.
    pub fn level(&mut self, row: i32, place: f32) {
        if row < 0 {
            if let Some((level, pan)) = self.master {
                let want = level_at(place, 2.);
                if (want - level).abs() >= 0.01 {
                    self.master = Some((want, pan));
                    self.edit_committed("master.set", json!({"level": want, "pan": pan}));
                }
            }
        } else if let Some(p) = self.parts.get_mut(self.part_cursor.first + row as usize) {
            let want = level_at(place, 1.);
            if (want - p.level).abs() >= 0.01 {
                p.level = want;
                let (id, mute) = (p.id, p.mute);
                self.edit_committed("parts.level", json!({"part": id, "level": want, "mute": mute}));
            }
        }
        self.show();
    }

    pub fn hold(&mut self, row: i32, down: bool) {
        self.held = down.then(|| if row < 0 { -1 } else { self.parts.get(self.at(row)).map_or(-2, |p| p.id as i64) });
    }

    /// The cursors drop when the screen goes, as on focus loss in the JUCE lists.
    pub fn hidden(&mut self) {
        self.scene_cursor = self.scene_cursor.clear();
        self.part_cursor = self.part_cursor.clear();
        self.held = None;
        self.show();
    }

    /// Shown: the views' sizes may have changed while it was not.
    pub fn shown(&mut self) {
        self.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> Value {
        json!({
            "nodes": [
                {"id": 1, "name": "Rhodes", "audioInputs": [], "audioOutputs": [{"channels": 2}], "noteInputs": 1, "noteOutputs": 0},
                {"id": 2, "name": "Chorus", "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0},
                {"id": 3, "name": "Hall", "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0},
                {"id": 4, "name": "Bass", "audioInputs": [], "audioOutputs": [{"channels": 1}], "noteInputs": 1, "noteOutputs": 0},
            ],
            "parts": [
                {"id": 10, "name": "Keys", "level": 0.5, "mute": false, "solo": false, "chain": [2, 1],
                 "input": {"kind": "notes", "devices": [], "zones": [{"keys": [36, 96], "transpose": -12}]}},
                {"id": 11, "name": "Low", "level": 1.0, "mute": false, "solo": false, "chain": [{"split": 1}],
                 "splits": [{"id": 1, "lanes": [{"chain": [4]}]}],
                 "input": {"kind": "notes", "devices": [{"kind": "external", "pattern": "", "match": "equals"}], "zones": [{"keys": [0, 127], "transpose": 0}]}},
                {"id": 12, "name": "Reverb", "level": 0.8, "mute": false, "solo": false, "chain": [3], "input": {"kind": "bus"}},
                {"id": 13, "name": "Empty", "level": 1.0, "mute": true, "solo": false, "chain": [], "input": {"kind": "none"}},
            ],
            "wires": [{"owner": "part", "part": 10, "fader": true, "mute": true}, {"owner": "part", "part": 13, "fader": true, "mute": true}],
            "scenes": [{"id": 1, "name": "Verse"}, {"id": 2, "name": "Chorus"}], "scene": 2,
            "master": {"level": 1.0, "pan": 0.0},
        })
    }

    #[test]
    fn parts_read_as_the_parts_view_reads_them() {
        let parts = parse_parts(&graph());
        let detail: Vec<_> = parts.iter().map(|p| (p.name.as_str(), p.detail.as_str())).collect();
        assert_eq!(detail, [("Keys", "Rhodes · Any device · C2–C7 · −12"), ("Low", "Bass · All external · all keys"), ("Empty", "No instrument · Its own notes"), ("Reverb", "Bus · Hall")]);
        // A bus after the parts; the part another's solo silences, but not the one muted itself.
        assert_eq!(parts.iter().map(|p| (p.silenced, p.mute)).collect::<Vec<_>>(), [(true, false), (false, false), (false, true), (false, false)]);
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(61), "C♯4");
    }

    #[test]
    fn faders_and_meters_use_the_juce_scales() {
        assert_eq!(fraction(0.25, 1.), 0.5, "-12 dB halfway up");
        assert_eq!((fraction(0., 1.), fraction(1., 1.), fraction(2., 2.), fraction(9., 1.)), (0., 1., 1., 1.));
        assert_eq!(level_at(0.5, 1.), 0.25);
        assert_eq!(level_at(1., 2.), 2.);
        assert_eq!((meter(0.), meter(1.), meter(10.)), (0., 0.91, 1.), "0 dB on the -60..+6 dB scale");
        assert_eq!((meter(0.001), meter(0.01)), (0., 0.3), "-60 dB is the bottom, -40 dB 0.3");
    }
}
