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
//! The knob row under the scenes (what the knobs under the device's screen play, its banks, and Learn) is
//! row.rs, drawn by the grid's tiles; this screen places it, moves the cursor into and out of it and names
//! Learn's targets. Edit is the top bar's last action, which opens the Parts list (parts.rs); the scene's
//! setup is scene.rs, also from the top bar (the bar's actions are shell.rs).

use crate::knobs::{self, Target};
use crate::nav::{Action, Cursor};
use crate::row::{self, Leave, Row, Stop};
use crate::setlist::update;
use crate::{KnobButton, PlayState, SceneData, Shell, StripData};
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
    /// Something of the knob row's (row.rs).
    Knobs(row::Request),
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
    /// The hint's "Open the on-screen keyboard".
    Keys,
    /// The hint's "Dismiss": kept (the shell writes it down).
    Dismiss,
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
    /// Pane 1's cursor is on the master, after the last part; once pressed the dial changes its level.
    master_cursor: bool,
    master_editing: bool,
    /// The no-MIDI hint: the profile offers the on-screen keyboard, session.status was read and
    /// says no device but the built-in is connected, and it was not dismissed (kept in the
    /// JUCE app's keyboard.txt). Its two links are the first stops of pane 1: 0 Open, 1 Dismiss.
    keys_offered: bool,
    midi_known: bool,
    midi_connected: bool,
    hint_dismissed: bool,
    hint_at: Option<u8>,
    /// The pane the dial moves: the one last touched (0 the scenes, 1 the parts, 2 the knob row).
    touched: u8,
    row: Row,
    /// The row is shown (the shell told it the device's controls).
    row_on: bool,
    /// Learn waits for a target and the cursor is on a part's M, not its level.
    learn_mute: bool,
    row_tiles: Rc<VecModel<crate::GridTile>>,
    row_buttons: Rc<VecModel<KnobButton>>,
    /// The part (-1 the master) whose fader a finger is on: the engine's own values for it wait.
    held: Option<i64>,
    follow: bool,
    tiles: Rc<VecModel<SceneData>>,
    rows: Rc<VecModel<StripData>>,
    /// Which scene tiles show a name cut after shrinking (the tiles say so as they are laid out).
    #[cfg(test)]
    cuts: Rc<std::cell::RefCell<Vec<bool>>>,
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
        let row_tiles = Rc::new(VecModel::default());
        let row_buttons = Rc::new(VecModel::default());
        state.set_knobs(ModelRc::from(row_tiles.clone()));
        state.set_knob_buttons(ModelRc::from(row_buttons.clone()));
        state.set_knob_tile(crate::grid::KNOB_ROW);
        #[cfg(test)]
        let cuts = Rc::new(std::cell::RefCell::new(vec![]));
        #[cfg(test)]
        state.on_scene_fit({
            let cuts = cuts.clone();
            move |i, cut| {
                let mut cuts = cuts.borrow_mut();
                if cuts.len() <= i as usize {
                    cuts.resize(i as usize + 1, false);
                }
                cuts[i as usize] = cut;
            }
        });
        #[cfg(not(test))]
        state.on_scene_fit(|_, _| {});
        Screen {
            ui: window.as_weak(),
            scenes: vec![],
            scene: 0,
            parts: vec![],
            master: None,
            scene_cursor: Cursor::new(0, SCENES),
            part_cursor: Cursor::new(0, 1),
            master_cursor: false,
            master_editing: false,
            keys_offered: false,
            midi_known: false,
            midi_connected: false,
            hint_dismissed: false,
            hint_at: None,
            touched: 1,
            row: Row::new(vec![]),
            row_on: false,
            learn_mute: false,
            row_tiles,
            row_buttons,
            held: None,
            follow: false,
            tiles,
            rows,
            #[cfg(test)]
            cuts,
            out: vec![],
        }
    }

    /// Whether each scene tile's name is cut (after shrinking to 70%).
    #[cfg(test)]
    pub fn scene_cuts(&self) -> Vec<bool> {
        self.cuts.borrow().clone()
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
        self.row.graph(graph);
        self.flush();
        self.show();
    }

    /// What the knob row asked for, with the other requests.
    fn flush(&mut self) {
        for (request, method, params) in std::mem::take(&mut self.row.out) {
            self.out.push((Request::Knobs(request), method, params));
        }
    }

    /// The device's control groups (its profile's): the knob row follows the knobs among them; none, a computer's four.
    /// Without this call there is no knob row.
    pub fn knobs(&mut self, groups: Vec<knobs::Group>) {
        self.row = Row::new(groups);
        self.row_on = true;
        self.show();
    }

    /// params.changed: the values that moved, for the knob row.
    pub fn params_changed(&mut self, params: &Value) {
        self.row.changed(params);
        self.flush();
        self.show();
    }

    /// controllers: the control moved last, for Learn.
    pub fn controllers(&mut self, params: &Value) {
        self.row.controllers(params);
        self.show();
    }

    /// controls.changed: levels and mutes moved live, and the master's.
    pub fn controls(&mut self, params: &Value) {
        self.row.controls(params);
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

    /// The profile offers the on-screen keyboard (`onScreenKeyboard`), and the hint was dismissed before.
    pub fn offer_keyboard(&mut self, offered: bool, dismissed: bool) {
        self.keys_offered = offered;
        self.hint_dismissed = dismissed;
        self.show();
    }

    /// session.status (its notification): whether a MIDI input besides the built-in one is connected
    /// (PlayView::showHint).
    pub fn status(&mut self, status: &Value) {
        if let Some(inputs) = status["midiInputs"].as_array() {
            self.midi_known = true;
            self.midi_connected = inputs.iter().any(|m| m["kind"] != "builtIn");
            self.show();
        }
    }

    fn hint_shown(&self) -> bool {
        self.keys_offered && self.midi_known && !self.midi_connected && !self.hint_dismissed
    }

    /// A tap on one of the hint's links (0 the keyboard, 1 Dismiss).
    pub fn tap_hint(&mut self, link: i32) -> Outcome {
        self.hint_at = None;
        self.hint_press(link.clamp(0, 1) as u8)
    }

    fn hint_press(&mut self, link: u8) -> Outcome {
        if link == 0 {
            return Outcome::Keys;
        }
        self.hint_dismissed = true;
        self.show();
        Outcome::Dismiss
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
            (Request::Knobs(r), result) => {
                self.row.reply(r, result);
                if !self.row.message.is_empty() {
                    let said = std::mem::take(&mut self.row.message);
                    self.say(said);
                }
                self.flush();
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
        // The tile after the last scene adds one, while there is room (the JUCE view shows eight tiles).
        let scenes = (self.scenes.len() + 1).min(SCENES);
        self.scene_cursor = Cursor { count: scenes, at: self.scene_cursor.at.map(|a| a.min(scenes.saturating_sub(1))), ..self.scene_cursor };
        self.part_cursor = Cursor { count: self.parts.len(), per_page: per_view, at: self.part_cursor.at.map(|a| a.min(self.parts.len().saturating_sub(1))), ..self.part_cursor };
        self.part_cursor = match self.part_cursor.at {
            Some(at) => self.part_cursor.to(at), // the view follows the cursor
            None => Cursor { first: if self.part_cursor.first >= self.parts.len() { 0 } else { self.part_cursor.first / per_view * per_view }, ..self.part_cursor },
        };
        let tiles: Vec<SceneData> = (0..SCENES)
            .map(|i| match self.scenes.get(i) {
                Some((id, name)) => SceneData { number: i as i32 + 1, name: name.as_str().into(), filled: true, add: false, on: *id == self.scene, cursor: self.scene_cursor.at == Some(i) },
                None => SceneData { number: i as i32 + 1, add: i == self.scenes.len(), cursor: self.scene_cursor.at == Some(i), ..Default::default() },
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
                    editing: false,
                    mark_cursor: self.part_cursor.at == Some(i) && self.learn_mute && self.row.learning_from(),
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
            state.set_master(StripData { name: "Master".into(), detail: "Performance".into(), fraction: fraction(level, 2.), cursor: self.master_cursor, editing: self.master_editing, ..Default::default() });
        }
        state.set_can_step(self.follow);
        // Said only when some parts are out of view: the strips already show how many there are.
        let view: SharedString = if self.parts.is_empty() || (first == 0 && end == self.parts.len()) { SharedString::new() } else { format!("Parts {}–{} of {}", first + 1, end, self.parts.len()).into() };
        state.set_view(view);
        let hint = self.hint_shown();
        if !hint {
            self.hint_at = None;
        }
        state.set_hint(hint);
        state.set_hint_cursor(self.hint_at.map_or(-1, i32::from));
        self.show_row(&ui);
    }

    /// The knob row: its tiles laid out in the area the window gave it (`grid::row_layout`), the bank's name, the
    /// buttons and Learn's line.
    fn show_row(&mut self, ui: &Shell) {
        let state = ui.global::<PlayState>();
        state.set_has_knobs(self.row_on);
        if !self.row_on {
            return;
        }
        let gap = crate::theme::section_gap(ui, crate::grid::SECTION_GAP);
        let want = self.row.tiles(state.get_knob_width(), state.get_knob_height(), gap, &self.parts, self.scene);
        update(&self.row_tiles, want);
        let cursor = self.row.cursor;
        let shown = self.row.buttons(&self.parts);
        // What the knobs follow is a column of tabs (they come before the bank's button, whose place is the last).
        let column: Vec<&(Stop, String, bool)> = shown.iter().filter(|b| b.0 != Stop::Page).collect();
        state.set_column_names(ModelRc::new(VecModel::from(column.iter().map(|b| b.1.as_str().into()).collect::<Vec<slint::SharedString>>())));
        state.set_column_current(column.iter().position(|b| b.2).map_or(-1, |i| i as i32));
        state.set_column_cursor(column.iter().position(|b| cursor == Some(b.0)).map_or(-1, |i| i as i32));
        let buttons: Vec<KnobButton> = shown.iter().map(|(stop, text, on)| KnobButton { text: text.as_str().into(), on: *on, cursor: cursor == Some(*stop), bank: *stop == Stop::Page }).collect();
        update(&self.row_buttons, buttons);
        state.set_knob_bank(self.row.bank().into());
        state.set_learning(self.row.learning());
        let learn = if self.row.learning_from() {
            let control = self.row.learn_from().replace(':', " ");
            let mut chars = control.chars();
            let name: String = chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
            format!("{name}: touch what it should play: a strip, its M, a scene or a knob")
        } else {
            "Learn: move a knob on the panel".to_string()
        };
        state.set_learn_text(learn.into());
        state.set_learn_reset_shown(self.row.learning_from());
        state.set_learn_reset_cursor(cursor == Some(Stop::Default));
        state.set_learn_done_cursor(cursor == Some(Stop::Done));
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

    /// A scene tile: the scene selected; the "+ Scene" tile after the last adds one, named as the
    /// JUCE view names it ("Scene N", N the count it makes), captured from all of the state as it is now.
    fn select_scene(&mut self, i: usize) {
        if let Some((id, _)) = self.scenes.get(i).filter(|_| i < SCENES) {
            self.scene = *id; // shown now; graph.get confirms
            let id = *id;
            self.edit("scenes.select", json!({"scene": id}));
        } else if i == self.scenes.len() && i < SCENES {
            let n = self.scenes.len() + 1;
            self.edit("scenes.add", json!({"name": format!("Scene {n}")}));
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

    /// The pane's cursor moved by `by` steps, shown on the first item by the first move. The master,
    /// pinned at the right, is the pane's last stop after the parts; once pressed, the dial moves its fader.
    fn move_in(&mut self, pane: u8, by: i64) {
        self.touched = pane;
        if pane == 0 {
            // Past the last scene the dial goes on into the knob row.
            let last = self.scene_cursor.count.saturating_sub(1);
            if by > 0 && self.row_on && self.scene_cursor.at == Some(last) && self.row.enter(true, &self.parts) {
                self.scene_cursor = self.scene_cursor.clear();
                self.touched = 2;
            } else {
                self.scene_cursor = self.scene_cursor.moved(by);
            }
        } else if pane == 2 {
            match self.row.move_by(by, &self.parts) {
                Leave::Stay => {}
                Leave::Up => {
                    self.row.cursor = None;
                    self.touched = 0;
                    self.scene_cursor = self.scene_cursor.to(self.scene_cursor.count.saturating_sub(1));
                }
                Leave::Down => {
                    self.row.cursor = None;
                    self.touched = 1;
                    self.enter_parts();
                }
            }
            self.flush();
        } else if self.master_editing {
            self.step_master(by);
        } else if let Some(link) = self.hint_at {
            match (link, by.signum()) {
                (0, -1) if self.row_on && self.row.enter(false, &self.parts) => {
                    self.hint_at = None; // up out of the links into the knob row
                    self.touched = 2;
                }
                (0, 1) => self.hint_at = Some(1),
                (1, 1) => {
                    self.hint_at = None; // on to the first part (or the master)
                    if self.parts.is_empty() {
                        self.master_cursor = self.has_master();
                    } else {
                        self.part_cursor = self.part_cursor.to(0);
                    }
                }
                (1, -1) => self.hint_at = Some(0),
                _ => {}
            }
        } else if self.hint_shown() && by < 0 && self.part_cursor.at == Some(0) {
            self.hint_at = Some(1); // up out of the first part onto the links
            self.part_cursor = self.part_cursor.clear();
        } else if self.hint_shown() && self.part_cursor.at.is_none() && !self.master_cursor {
            self.hint_at = Some(0); // the first move shows the cursor on the first stop
        } else if self.master_cursor {
            if by < 0 {
                self.master_cursor = false;
                self.part_cursor = self.part_cursor.to(self.parts.len().saturating_sub(1));
            }
        } else if by > 0 && self.has_master() && self.part_cursor.at.is_some_and(|a| a + 1 >= self.parts.len()) {
            self.master_cursor = true; // past the last part
            self.part_cursor = self.part_cursor.clear();
        } else if by < 0 && self.row_on && self.part_cursor.at == Some(0) && !self.learn_mute && self.row.enter(false, &self.parts) {
            self.part_cursor = self.part_cursor.clear(); // up out of the first part into the knob row
            self.touched = 2;
        } else if self.row.learning_from() && self.part_cursor.at.is_some() {
            // Learn: each part has two stops, its level and its M.
            for _ in 0..by.unsigned_abs().min(64) {
                let before = (self.part_cursor.at, self.learn_mute);
                if by > 0 {
                    if self.learn_mute {
                        self.part_cursor = self.part_cursor.moved(1);
                        self.learn_mute = self.part_cursor.at == before.0;
                    } else {
                        self.learn_mute = true;
                    }
                } else if self.learn_mute {
                    self.learn_mute = false;
                } else {
                    self.part_cursor = self.part_cursor.moved(-1);
                    self.learn_mute = self.part_cursor.at != before.0;
                }
            }
        } else {
            self.part_cursor = self.part_cursor.moved(by);
        }
    }

    /// The cursor onto the first stop of the parts pane: the hint's first link, the first part, or the master.
    fn enter_parts(&mut self) {
        if self.hint_shown() {
            self.hint_at = Some(0);
        } else if self.parts.is_empty() {
            self.master_cursor = self.has_master();
        } else {
            self.part_cursor = self.part_cursor.to(0);
        }
    }

    fn has_master(&self) -> bool {
        self.master.is_some() && !self.parts.is_empty()
    }

    /// The master's fader by `by` clicks of the dial, 1/40 of its travel each, held at the ends. Sent
    /// as the fader sends, committed.
    fn step_master(&mut self, by: i64) {
        let Some((level, pan)) = self.master else { return };
        let place = (fraction(level, 2.) as f64 + by.signum() as f64 * 0.025).clamp(0., 1.) as f32;
        let want = level_at(place, 2.);
        if want != level {
            self.master = Some((want, pan));
            self.edit_committed("master.set", json!({"level": want, "pan": pan}));
        }
    }

    /// A navigation action. Pane 0 is the scenes, pane 1 the parts; the arrow keys: left and right
    /// move along the pane last touched, up the scenes, down the parts; Page Up and Down are the
    /// setlist's Previous and Next (page turners send them).
    pub fn act(&mut self, action: Action) -> Outcome {
        let pane = |p: Option<u8>, touched: u8| p.map_or(touched, |p| p.min(1));
        match action {
            Action::Move { pane: p, by } => self.move_in(pane(p, self.touched), by as i64),
            // Up and down go from the scenes by the knob row (when there is one) to the parts.
            Action::Arrow { dx: 0, dy } => {
                let zones: &[u8] = if self.row_on { &[0, 2, 1] } else { &[0, 1] };
                let at = zones.iter().position(|z| *z == self.touched).unwrap_or(zones.len() - 1);
                let to = (at as i64 + i64::from(dy.signum())).clamp(0, zones.len() as i64 - 1) as usize;
                self.move_in(zones[to], 0);
            }
            Action::Arrow { dx, .. } if self.touched == 2 => {
                self.row.arrow(dx, &self.parts);
                self.flush();
            }
            Action::Arrow { dx, .. } => self.move_in(self.touched, dx as i64),
            Action::Page(by) => {
                return if self.follow { Outcome::Step(by as i64) } else { Outcome::Stay };
            }
            Action::End(by) if self.touched == 2 => {
                self.row.enter(by < 0, &self.parts);
            }
            Action::End(by) => self.move_in(self.touched, by as i64 * 10_000),
            Action::Confirm(p) => match pane(p, self.touched) {
                0 => {
                    if let Some(at) = self.scene_cursor.at {
                        // Learn waiting for a target: the scene is it, and does not play.
                        match self.scenes.get(at).map(|s| s.0 as u32) {
                            Some(id) if self.row.learning_from() => {
                                self.row.learn_target(Target::Scene(id));
                                self.flush();
                            }
                            _ => self.select_scene(at),
                        }
                    }
                }
                2 => {
                    self.row.confirm(&self.parts, self.scene);
                    self.flush();
                }
                _ if self.hint_at.is_some() => return self.hint_press(self.hint_at.unwrap_or(0)),
                _ if self.master_cursor => self.master_editing = !self.master_editing, // a press, then the dial
                _ => {
                    if let Some(id) = self.part_cursor.at.and_then(|at| self.parts.get(at)).map(|p| p.id) {
                        if self.row.learning_from() {
                            // Learn: the part's level, or its M.
                            self.row.learn_target(if self.learn_mute { Target::PartMute(id) } else { Target::PartLevel(id) });
                            self.learn_mute = false;
                            self.flush();
                        } else {
                            self.row.select(id);
                            self.flush();
                            return Outcome::Open(id);
                        }
                    }
                }
            },
            Action::Cancel(_) => {
                if self.master_editing {
                    self.master_editing = false; // ends the change, the cursor stays
                } else if self.row.has_cursor() && self.row.cancel() {
                    self.flush(); // a value being changed, or the cursor on the knob row
                } else {
                    let idle = self.scene_cursor.at.is_none() && self.part_cursor.at.is_none() && !self.master_cursor && self.hint_at.is_none();
                    if idle && self.row.cancel() {
                        self.learn_mute = false; // Learn given up first
                        self.flush();
                    } else if idle {
                        return Outcome::Back;
                    } else {
                        self.scene_cursor = self.scene_cursor.clear();
                        self.part_cursor = self.part_cursor.clear();
                        self.master_cursor = false;
                        self.hint_at = None;
                    }
                }
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
        // Learn waiting for a target: the scene is it, and does not play.
        match self.scenes.get(i.max(0) as usize).map(|s| s.0 as u32) {
            Some(id) if self.row.learning_from() => {
                self.row.learn_target(Target::Scene(id));
                self.flush();
            }
            _ => self.select_scene(i.max(0) as usize),
        }
        self.show();
    }

    /// A tap on a strip's name: that part selected, its Part screen; with Learn waiting for a target, its level is it.
    pub fn tap_open(&mut self, row: i32) -> Option<u32> {
        let id = self.parts.get(self.at(row)).map(|p| p.id)?;
        if self.row.learning_from() {
            self.row.learn_target(Target::PartLevel(id));
            self.flush();
            self.show();
            return None;
        }
        self.row.select(id);
        self.flush();
        Some(id)
    }

    pub fn tap_mute(&mut self, row: i32) {
        self.part_cursor = self.part_cursor.clear();
        if let (true, Some(id)) = (self.row.learning_from(), self.parts.get(self.at(row)).map(|p| p.id)) {
            // Learn: the mute is the target, and does not flip.
            self.row.learn_target(Target::PartMute(id));
            self.flush();
        } else {
            self.toggle_mute(self.at(row));
        }
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
        if down {
            // A finger on a fader takes over from the cursor, as a tap drops the cursors.
            self.master_cursor = false;
            self.master_editing = false;
        }
    }

    /// The device has knobs to Learn: a top bar action.
    pub fn can_learn(&self) -> bool {
        self.row_on && self.row.can_learn()
    }

    pub fn learning(&self) -> bool {
        self.row.learning()
    }

    /// The Learn action of the top bar: on, or off.
    pub fn learn_toggle(&mut self) {
        let on = !self.row.learning();
        self.row.learn(on);
        self.learn_mute = false;
        self.flush();
        self.show();
    }

    /// Whether the device's knobs play the row (the engine is told the screen shows no lists then).
    pub fn plays_row(&self) -> bool {
        self.row_on
    }

    /// A finger on a knob's tile: a drag begins, or Learn names it.
    pub fn knob_grab(&mut self, tile: i32) {
        self.row.cursor = None;
        self.row.grab(tile.max(0) as usize);
        self.flush();
        self.show();
    }

    pub fn knob_drag(&mut self, tile: i32, norm: f32) {
        self.row.drag(tile.max(0) as usize, norm, &self.parts);
        self.flush();
        self.show();
    }

    pub fn knob_release(&mut self) {
        self.row.release();
        self.flush();
        self.show();
    }

    pub fn knob_tap(&mut self, tile: i32) {
        self.row.cursor = None;
        self.row.tap(tile.max(0) as usize, &self.parts);
        self.flush();
        self.show();
    }

    /// A tap on one of the row's buttons (its place among those shown).
    pub fn knob_button(&mut self, i: i32) {
        self.row.cursor = None;
        if let Some((stop, _, _)) = self.row.buttons(&self.parts).get(i.max(0) as usize).cloned() {
            self.row.press_stop(stop, &self.parts, self.scene);
        }
        self.flush();
        self.show();
    }

    /// A tap on Learn's Default or Done.
    pub fn learn_button(&mut self, stop: Stop) {
        self.row.cursor = None;
        self.row.press_stop(stop, &self.parts, self.scene);
        self.flush();
        self.show();
    }

    /// The scene cursor is on the first tile: moving up or left leaves for the top bar's actions.
    pub fn at_top(&self) -> bool {
        self.touched == 0 && self.scene_cursor.at == Some(0)
    }

    /// The cursors drop when the screen goes, as on focus loss in the JUCE lists.
    pub fn hidden(&mut self) {
        self.row.hidden();
        self.row.learn(false);
        self.learn_mute = false;
        self.flush();
        self.scene_cursor = self.scene_cursor.clear();
        self.part_cursor = self.part_cursor.clear();
        self.master_cursor = false;
        self.master_editing = false;
        self.hint_at = None;
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
