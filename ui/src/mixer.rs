// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Mixer: the mixer page of the JUCE Edit view (hosts/standalone/PartsView.cpp, `mixerShown`), on
//! the shell. The reference is strips, not tiles: a strip for every part and bus side by side (name,
//! a fader beside its meters, the level in dB, mute and solo, pan, a send to each bus) and the master
//! after them, so strips are what is built here, with the fader, marks and meters Play's strips use.
//!
//! Without touch the cursor sits on one control of one strip: the dial goes strip by strip, control
//! by control (fader, M, S, pan, each send), then on to the next strip and the master; the left knob
//! (pane 0) moves along the strips keeping the control, the right knob (pane 1) up and down the
//! strip. A press on a fader, the pan or a send starts a change, which the dial then makes (a fortieth
//! of the control's travel a click; a press or Cancel ends it), on M or S toggles it. Cancel ends a
//! change, drops the cursor, then goes back to the Parts list. Touch: the faders and sliders drag, a
//! double tap on a pan or send puts it back (the JUCE pan's double click), M and S tap.
//!
//! How many strips a view holds and how many sends a strip shows is whatever fills the width and
//! height (the screen's component says), so it follows the window, the spacing and the type scale.
//! The view the cursor is in is shown; a strip never scrolls, a view is a whole set of them.
//!
//! Requests: `graph.get` (asked by the shell), `parts.meters` every METER_INTERVAL while shown (the
//! JUCE view polls at 50 ms), `parts.level` (a level or a mute), `parts.pan`, `parts.solo`,
//! `parts.send`, `master.set`, each followed by `graph.commit` as the JUCE setters commit.

use crate::nav::Action;
use crate::play::{fraction, level_at};
use crate::setlist::update;
use crate::{MixStrip, MixerState, Shell};
use serde_json::{json, Value};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// How often the meters are asked while the Mixer is shown (the JUCE view's timer, 50 ms).
pub const METER_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Meters,
    /// Any edit: its refusal is said.
    Edit,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Back to the Parts list.
    Back,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Strip {
    pub id: u32,
    pub bus: bool,
    pub name: String,
    pub level: f64,
    pub mute: bool,
    pub solo: bool,
    pub silenced: bool,
    pub pan: f64,
    /// The level of the send to each bus, in the order of the buses (None: no send).
    pub sends: Vec<Option<f64>>,
}

/// A bus: its id and name.
pub type Bus = (u64, String);

/// The controls of a strip, in the order the cursor goes down it; a send is `SEND` and its place among the buses.
const FADER: usize = 0;
const MUTE: usize = 1;
const SOLO: usize = 2;
const PAN: usize = 3;
const SEND: usize = 4;

/// The cursor: a strip (the master is the one after the last) and one of its controls.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stop {
    strip: usize,
    control: usize,
}

/// graph.get's parts then buses and the master, as the JUCE mixer page reads them (PartsModel.cpp's
/// readParts for the rows, `graph_.masterLevel` and `masterPan` for the master).
pub fn read(graph: &Value) -> (Vec<Strip>, Vec<Bus>, (f64, f64)) {
    let wires = graph["wires"].as_array().cloned().unwrap_or_default();
    let all = graph["parts"].as_array().cloned().unwrap_or_default();
    let is_bus = |p: &Value| p["input"]["kind"] == "bus";
    let buses: Vec<Bus> = all.iter().filter(|p| is_bus(p)).filter_map(|p| Some((p["id"].as_u64()?, p["name"].as_str().unwrap_or_default().to_string()))).collect();
    let strip = |p: &Value| {
        let id = p["id"].as_u64().unwrap_or(0);
        let mute = p["mute"].as_bool().unwrap_or(false);
        // Another part's solo: the wires where it plays are muted though it is not.
        let silenced = !mute && wires.iter().any(|w| w["owner"] == "part" && w["part"].as_u64() == Some(id) && w["fader"] == true && w["mute"] == true);
        let sends = buses.iter().map(|b| p["sends"].as_array().and_then(|s| s.iter().find(|s| s["bus"].as_u64() == Some(b.0))).and_then(|s| s["level"].as_f64())).collect();
        Strip { id: id as u32, bus: is_bus(p), name: p["name"].as_str().unwrap_or_default().into(), level: p["level"].as_f64().unwrap_or(1.), mute, solo: p["solo"].as_bool().unwrap_or(false), silenced, pan: p["pan"].as_f64().unwrap_or(0.), sends }
    };
    let strips = all.iter().filter(|p| !is_bus(p)).chain(all.iter().filter(|p| is_bus(p))).map(strip).collect();
    (strips, buses, (graph["master"]["level"].as_f64().unwrap_or(1.), graph["master"]["pan"].as_f64().unwrap_or(0.)))
}

/// A peak (linear) along the meter's -60..+6 dB, in hundredths: the JUCE meters' scale (Icons.h).
fn meter(peak: f64) -> f32 {
    let db = if peak > 0. { 20. * peak.log10() } else { -100. };
    (((db + 60.) / 66.).clamp(0., 1.) * 100.).round() as f32 / 100.
}

/// A fader's level in dB as the JUCE strip says it: one decimal, "−∞ dB" at nothing.
fn db(level: f64) -> String {
    if level > 0. { format!("{:.1} dB", 20. * level.log10()) } else { "−∞ dB".into() }
}

fn thousandths(v: f64) -> f64 {
    (v * 1000.).round() / 1000.
}

/// A send's level by `by` clicks of the dial, a fortieth of the slider each, held at the ends.
fn stepped(was: f64, by: i64) -> f64 {
    thousandths((was + 0.025 * by as f64).clamp(0., 1.))
}

/// What a strip shows, in plain types, so a row of the window is set only when it changed.
#[derive(Clone, Debug, Default, PartialEq)]
struct Shown {
    name: String,
    fader: f32,
    db: String,
    mute: bool,
    solo: bool,
    dim: bool,
    bus: bool,
    master: bool,
    pan: f32,
    sends: Vec<f32>,
    peaks: Vec<f32>,
    cursor: i32,
    editing: bool,
}

impl Shown {
    fn mixer(&self) -> MixStrip {
        MixStrip {
            name: self.name.as_str().into(),
            fader: self.fader,
            db: self.db.as_str().into(),
            mute: self.mute,
            solo: self.solo,
            dim: self.dim,
            bus: self.bus,
            master: self.master,
            pan: self.pan,
            sends: ModelRc::new(VecModel::from(self.sends.clone())),
            peaks: ModelRc::new(VecModel::from(self.peaks.clone())),
            cursor: self.cursor,
            editing: self.editing,
        }
    }
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    strips: Vec<Strip>,
    buses: Vec<Bus>,
    /// The master's level and pan.
    master: (f64, f64),
    /// Each strip's recent peaks per channel, by part, as the last `parts.meters` gave them.
    peaks: HashMap<u32, Vec<f32>>,
    failure: String,
    stop: Option<Stop>,
    /// The dial changes the control under the cursor.
    editing: bool,
    /// The first strip of the view shown, and the first send of the window shown.
    first: usize,
    send_first: usize,
    /// The refusal of the last edit.
    said: String,
    /// The control a finger is on: the strip's part (None the master) and the control. The engine's own value for it waits.
    held: Option<(Option<u32>, usize)>,
    rows: Rc<VecModel<MixStrip>>,
    last: Vec<Shown>,
    last_master: Option<Shown>,
    names: Rc<VecModel<SharedString>>,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(window: &Shell) -> Screen {
        let s = Screen {
            ui: window.as_weak(),
            strips: vec![],
            buses: vec![],
            master: (1., 0.),
            peaks: HashMap::new(),
            failure: String::new(),
            stop: None,
            editing: false,
            first: 0,
            send_first: 0,
            said: String::new(),
            held: None,
            rows: Rc::new(VecModel::default()),
            last: vec![],
            last_master: None,
            names: Rc::new(VecModel::default()),
            out: vec![],
        };
        let state = window.global::<MixerState>();
        state.set_strips(ModelRc::from(s.rows.clone()));
        state.set_send_names(ModelRc::from(s.names.clone()));
        s
    }

    /// graph.get: the strips again, the cursor kept where it is (what a finger holds keeps its own value).
    pub fn graph(&mut self, graph: &Value) {
        let (mut strips, buses, mut master) = read(graph);
        if let Some((id, control)) = self.held {
            match id {
                None => {
                    if control == FADER {
                        master.0 = self.master.0;
                    } else if control == PAN {
                        master.1 = self.master.1;
                    }
                }
                Some(id) => {
                    if let (Some(now), Some(was)) = (strips.iter_mut().find(|s| s.id == id), self.strips.iter().find(|s| s.id == id)) {
                        match control {
                            FADER => now.level = was.level,
                            PAN => now.pan = was.pan,
                            c if c >= SEND => {
                                if let (Some(slot), Some(v)) = (now.sends.get_mut(c - SEND), was.sends.get(c - SEND)) {
                                    *slot = *v;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        self.strips = strips;
        self.buses = buses;
        self.master = master;
        self.show();
    }

    /// controls.changed: levels, mutes and pans moved live, and the master's (a control a finger is on keeps its own).
    pub fn controls(&mut self, params: &Value) {
        let held = self.held;
        for c in params["parts"].as_array().into_iter().flatten() {
            let Some(id) = c["part"].as_u64() else { continue };
            if let Some(s) = self.strips.iter_mut().find(|s| u64::from(s.id) == id) {
                if held != Some((Some(s.id), FADER)) {
                    s.level = c["level"].as_f64().unwrap_or(s.level);
                }
                if held != Some((Some(s.id), PAN)) {
                    s.pan = c["pan"].as_f64().unwrap_or(s.pan);
                }
                s.mute = c["mute"].as_bool().unwrap_or(s.mute);
            }
        }
        if held != Some((None, FADER)) {
            self.master.0 = params["master"]["level"].as_f64().unwrap_or(self.master.0);
        }
        if held != Some((None, PAN)) {
            self.master.1 = params["master"]["pan"].as_f64().unwrap_or(self.master.1);
        }
        self.show();
    }

    /// parts.meters: each part's and bus's channels' recent peaks, a bar per channel.
    pub fn meters(&mut self, result: &Value) {
        self.peaks.clear();
        for m in result["meters"].as_array().into_iter().flatten() {
            let Some(id) = m["part"].as_u64() else { continue };
            self.peaks.insert(id as u32, m["channels"].as_array().map(|c| c.iter().filter_map(Value::as_f64).map(meter).collect()).unwrap_or_default());
        }
        self.show();
    }

    /// session.status: the plugin that failed, said as the JUCE message line says it.
    pub fn status(&mut self, status: &Value) {
        if let Some(list) = status["failures"].as_array() {
            let failure = list.first().map(|f| format!("{} is silent: {}", f["name"].as_str().unwrap_or_default(), f["reason"].as_str().unwrap_or_default())).unwrap_or_default();
            if failure != self.failure {
                self.failure = failure;
                self.show();
            }
        }
    }

    /// Whether an edit was refused: the shell then asks the graph again, which puts the strips right.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Meters, Ok(r)) => self.meters(&r),
            (Request::Edit, Err(e)) if e.code != 0 => {
                self.said = crate::client::refusal(&e);
                self.show();
                return true;
            }
            _ => {}
        }
        false
    }

    fn edit(&mut self, method: &'static str, params: Value) {
        self.said.clear();
        self.out.push((Request::Edit, method, params));
        self.out.push((Request::Edit, "graph.commit", json!({})));
    }

    /// The master is the entry after the last strip.
    fn master(&self) -> usize {
        self.strips.len()
    }

    /// The controls strip `at` has: the faders and pans of all, mute and solo of a part and a bus,
    /// the sends of a part only.
    fn controls_of(&self, at: usize) -> Vec<usize> {
        match self.strips.get(at) {
            None => vec![FADER, PAN],
            Some(s) if s.bus => vec![FADER, MUTE, SOLO, PAN],
            Some(_) => (0..SEND + self.buses.len()).collect(),
        }
    }

    /// The control of strip `at` nearest to `control`, the one above it when it has none.
    fn near(&self, at: usize, control: usize) -> usize {
        let has = self.controls_of(at);
        has.iter().rev().find(|c| **c <= control).copied().unwrap_or(has[0])
    }

    fn stops(&self) -> Vec<Stop> {
        (0..=self.strips.len()).flat_map(|strip| self.controls_of(strip).into_iter().map(move |control| Stop { strip, control })).collect()
    }

    fn per_view(&self) -> usize {
        self.ui.upgrade().map_or(1, |ui| ui.global::<MixerState>().get_strips_per_view().max(1) as usize)
    }

    fn sends_shown(&self) -> usize {
        self.ui.upgrade().map_or(1, |ui| (ui.global::<MixerState>().get_sends_per_view().max(1) as usize).min(self.buses.len()))
    }

    /// The window's view: the strips from `first` and the sends from `send_first` that fit, the
    /// cursor's strip and send in them, each row set only when it changed.
    pub fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let per = self.per_view();
        let shown = self.sends_shown();
        if self.stop.is_some_and(|s| s.strip > self.strips.len()) {
            self.stop = None;
            self.editing = false;
        }
        if let Some(Stop { strip, control }) = self.stop {
            if strip < self.strips.len() {
                self.first = strip / per * per;
            }
            if control >= SEND {
                let k = control - SEND;
                if k < self.send_first {
                    self.send_first = k;
                } else if k >= self.send_first + shown {
                    self.send_first = k + 1 - shown;
                }
            }
        }
        self.first = if self.first >= self.strips.len() { 0 } else { self.first / per * per };
        self.send_first = self.send_first.min(self.buses.len().saturating_sub(shown));
        let end = self.strips.len().min(self.first + per);
        let (send_first, send_end) = (self.send_first, self.send_first + shown);
        let bars = |id: u32| match self.peaks.get(&id) {
            Some(p) if !p.is_empty() => p.clone(),
            _ => vec![0.],
        };
        // The send controls are numbered from SEND within the window shown.
        let cursor_of = |at: usize| match self.stop {
            Some(s) if s.strip == at => if s.control >= SEND { s.control.saturating_sub(send_first) as i32 } else { s.control as i32 },
            _ => -1,
        };
        let editing = |at: usize| self.editing && self.stop.is_some_and(|s| s.strip == at);
        let rows: Vec<Shown> = (self.first..end)
            .map(|i| {
                let s = &self.strips[i];
                Shown {
                    name: s.name.clone(),
                    fader: fraction(s.level, 1.),
                    db: db(s.level),
                    mute: s.mute,
                    solo: s.solo,
                    dim: s.mute || s.silenced,
                    bus: s.bus,
                    master: false,
                    pan: ((s.pan + 1.) / 2.).clamp(0., 1.) as f32,
                    sends: if s.bus { vec![] } else { (send_first..send_end).map(|k| s.sends.get(k).copied().flatten().unwrap_or(0.) as f32).collect() },
                    peaks: bars(s.id),
                    cursor: cursor_of(i),
                    editing: editing(i),
                }
            })
            .collect();
        for (i, row) in rows.iter().enumerate() {
            if self.rows.row_count() <= i {
                self.rows.push(row.mixer());
            } else if self.last.get(i) != Some(row) {
                self.rows.set_row_data(i, row.mixer());
            }
        }
        while self.rows.row_count() > rows.len() {
            self.rows.remove(self.rows.row_count() - 1);
        }
        self.last = rows;
        let master = self.master();
        let master_row = Shown {
            name: "Master".into(),
            fader: fraction(self.master.0, 2.),
            db: db(self.master.0),
            pan: ((self.master.1 + 1.) / 2.).clamp(0., 1.) as f32,
            cursor: cursor_of(master),
            editing: editing(master),
            master: true,
            ..Default::default()
        };
        if self.last_master.as_ref() != Some(&master_row) {
            ui.global::<MixerState>().set_master(master_row.mixer());
            self.last_master = Some(master_row);
        }
        update(&self.names, self.buses[send_first.min(self.buses.len())..send_end.min(self.buses.len())].iter().map(|b| SharedString::from(b.1.as_str())).collect());
        let state = ui.global::<MixerState>();
        state.set_buses(self.buses.len() as i32);
        let mut view = format!("Strips {}–{} of {}", self.first + 1, end, self.strips.len());
        if self.strips.is_empty() {
            view = "Master".into();
        }
        if self.buses.len() > shown {
            view += &format!(" · Sends {}–{} of {}", send_first + 1, send_end, self.buses.len());
        }
        state.set_view(view.into());
        let message = if self.said.is_empty() { self.failure.clone() } else { self.said.clone() };
        state.set_message(message.into());
    }

    fn set_level(&mut self, at: usize, level: f64) {
        if at == self.master() {
            let level = thousandths(level.clamp(0., 2.));
            if (level - self.master.0).abs() < f64::EPSILON {
                return;
            }
            self.master.0 = level;
            self.edit("master.set", json!({"level": level, "pan": self.master.1}));
        } else if let Some(s) = self.strips.get_mut(at) {
            let level = thousandths(level.clamp(0., 1.));
            if (level - s.level).abs() < f64::EPSILON {
                return;
            }
            s.level = level;
            let (id, mute) = (s.id, s.mute);
            self.edit("parts.level", json!({"part": id, "level": level, "mute": mute}));
        }
    }

    fn set_pan(&mut self, at: usize, pan: f64) {
        let pan = thousandths(pan.clamp(-1., 1.));
        if at == self.master() {
            if (pan - self.master.1).abs() < f64::EPSILON {
                return;
            }
            self.master.1 = pan;
            self.edit("master.set", json!({"level": self.master.0, "pan": pan}));
        } else if let Some(s) = self.strips.get_mut(at) {
            if (pan - s.pan).abs() < f64::EPSILON {
                return;
            }
            s.pan = pan;
            let id = s.id;
            self.edit("parts.pan", json!({"part": id, "pan": pan}));
        }
    }

    fn toggle_mute(&mut self, at: usize) {
        let Some(s) = self.strips.get_mut(at) else { return };
        s.mute = !s.mute;
        let (id, level, mute) = (s.id, s.level, s.mute);
        self.edit("parts.level", json!({"part": id, "level": level, "mute": mute}));
    }

    fn toggle_solo(&mut self, at: usize) {
        let Some(s) = self.strips.get_mut(at) else { return };
        s.solo = !s.solo;
        let (id, solo) = (s.id, s.solo);
        self.edit("parts.solo", json!({"part": id, "solo": solo}));
    }

    /// A send's new level (the bus `k` among the buses); none left (0) takes the send away, as the JUCE setSendLevel.
    fn set_send(&mut self, at: usize, k: usize, level: f64) {
        let Some(bus) = self.buses.get(k).map(|b| b.0) else { return };
        let Some(s) = self.strips.get_mut(at) else { return };
        let level = thousandths(level.clamp(0., 1.));
        let was = s.sends.get(k).copied().flatten().unwrap_or(0.);
        if (level - was).abs() < f64::EPSILON {
            return;
        }
        if let Some(slot) = s.sends.get_mut(k) {
            *slot = (level > 0.).then_some(level);
        }
        let id = s.id;
        let params = if level > 0. { json!({"part": id, "bus": bus, "level": level}) } else { json!({"part": id, "bus": bus}) };
        self.edit("parts.send", params);
    }

    /// A fader moved by `by` clicks of the dial, a fortieth of its travel each, held at the ends.
    fn step_fader(&mut self, at: usize, by: i64) {
        let max = if at == self.master() { 2. } else { 1. };
        let level = if at == self.master() { self.master.0 } else { self.strips.get(at).map_or(0., |s| s.level) };
        let place = (f64::from(fraction(level, max)) + by as f64 * 0.025).clamp(0., 1.);
        self.set_level(at, level_at(place as f32, max));
    }

    /// The dial on the control being changed.
    fn step_value(&mut self, by: i64) {
        let Some(Stop { strip, control }) = self.stop else { return };
        match control {
            FADER => self.step_fader(strip, by),
            PAN => {
                let was = if strip == self.master() { self.master.1 } else { self.strips.get(strip).map_or(0., |s| s.pan) };
                self.set_pan(strip, was + 0.05 * by as f64);
            }
            c if c >= SEND => {
                let was = self.strips.get(strip).and_then(|s| s.sends.get(c - SEND).copied().flatten()).unwrap_or(0.);
                self.set_send(strip, c - SEND, stepped(was, by));
            }
            _ => {}
        }
    }

    /// The cursor's next stop along the dial: strip by strip, control by control, held at either end.
    fn dial(&mut self, by: i64) {
        let stops = self.stops();
        let at = match self.stop.and_then(|s| stops.iter().position(|x| *x == s)) {
            Some(at) => (at as i64 + by).clamp(0, stops.len() as i64 - 1) as usize,
            None => stops.iter().position(|s| s.strip >= self.first).unwrap_or(0), // the first move shows it
        };
        self.stop = stops.get(at).copied();
    }

    /// Along the strips by `by`, the control kept where the strip has it.
    fn along(&mut self, by: i64) {
        let Some(Stop { strip, control }) = self.stop else { return self.dial(0) };
        let to = (strip as i64 + by).clamp(0, self.strips.len() as i64) as usize;
        self.stop = Some(Stop { strip: to, control: self.near(to, control) });
    }

    /// Up (-1) and down (1) the controls of the cursor's strip.
    fn down(&mut self, by: i64) {
        let Some(Stop { strip, control }) = self.stop else { return self.dial(0) };
        let has = self.controls_of(strip);
        let at = has.iter().position(|c| *c == control).unwrap_or(0) as i64;
        self.stop = Some(Stop { strip, control: has[(at + by).clamp(0, has.len() as i64 - 1) as usize] });
    }

    /// The cursor is on the first control of the first strip: moving up leaves for the top bar's actions.
    pub fn at_top(&self) -> bool {
        self.stop == Some(Stop { strip: 0, control: FADER }) && !self.editing
    }

    /// What a press on the cursor does.
    fn press(&mut self) {
        let Some(Stop { strip, control }) = self.stop else { return };
        match control {
            MUTE => self.toggle_mute(strip),
            SOLO => self.toggle_solo(strip),
            _ => self.editing = !self.editing, // a press, then the dial
        }
    }

    /// A navigation action. The dial and the keys' up and down go through the controls, the left
    /// knob and the keys' left and right along the strips, the right knob up and down the strip.
    pub fn act(&mut self, action: Action) -> Outcome {
        self.said.clear();
        match action {
            Action::Move { pane: None, by } => {
                if self.editing {
                    self.step_value(i64::from(by));
                } else {
                    self.dial(i64::from(by));
                }
            }
            Action::Move { pane: Some(0), by } => {
                if self.editing {
                    self.step_value(i64::from(by));
                } else {
                    self.along(i64::from(by));
                }
            }
            Action::Move { pane: Some(_), by } => {
                if !self.editing {
                    self.down(i64::from(by));
                }
            }
            Action::Arrow { dx: 0, dy } => {
                if !self.editing {
                    self.down(i64::from(dy));
                }
            }
            Action::Arrow { dx, .. } => {
                if self.editing {
                    self.step_value(i64::from(dx));
                } else {
                    self.along(i64::from(dx));
                }
            }
            Action::Page(by) => {
                if !self.editing {
                    self.turn(by);
                }
            }
            Action::End(by) => {
                if !self.editing {
                    self.stop = if by < 0 { Some(Stop { strip: 0, control: FADER }) } else { Some(Stop { strip: self.master(), control: FADER }) };
                }
            }
            Action::Confirm(_) => self.press(),
            Action::Cancel(_) => {
                if self.editing {
                    self.editing = false;
                } else if self.stop.is_some() {
                    self.stop = None;
                } else {
                    return Outcome::Back;
                }
            }
        }
        self.show();
        Outcome::Stay
    }

    /// The pagers, by what each does: (label, enabled, sends?, by). Only what is out of view has one.
    fn pagers(&self) -> Vec<(&'static str, bool, bool, i32)> {
        let mut v = vec![];
        let per = self.per_view();
        if self.strips.len() > per {
            v.push(("‹ Strips", self.first > 0, false, -1));
            v.push(("Strips ›", self.first + per < self.strips.len(), false, 1));
        }
        let shown = self.sends_shown().max(1);
        if self.buses.len() > shown {
            v.push(("‹ Sends", self.send_first > 0, true, -1));
            v.push(("Sends ›", self.send_first + shown < self.buses.len(), true, 1));
        }
        v
    }

    /// The screen's own actions, which the top bar carries: the strips' and the sends' pagers.
    pub fn bar_tools(&self) -> Vec<crate::topbar::Tool> {
        self.pagers().into_iter().map(|(label, enabled, ..)| crate::topbar::Tool { enabled, ..crate::topbar::Tool::new(label) }).collect()
    }

    /// The pager `i` of the bar.
    pub fn bar_tool(&mut self, i: usize) {
        if let Some((_, _, sends, by)) = self.pagers().get(i).copied().filter(|p| p.1) {
            if sends {
                self.turn_sends(by);
            } else {
                self.turn(by);
            }
        }
    }

    /// A view of strips towards the start (-1) or the end (1): the cursor, if there is one, on its first strip.
    pub fn turn(&mut self, by: i32) {
        let per = self.per_view();
        let views = self.strips.len().div_ceil(per).max(1);
        let to = ((self.first / per) as i64 + i64::from(by)).clamp(0, views as i64 - 1) as usize;
        self.first = to * per;
        if let Some(s) = self.stop.filter(|s| s.strip < self.strips.len()) {
            self.stop = Some(Stop { strip: self.first, control: self.near(self.first, s.control) });
        }
        self.show();
    }

    /// A window of sends towards the first bus (-1) or the last (1).
    pub fn turn_sends(&mut self, by: i32) {
        let shown = self.sends_shown().max(1);
        let to = (self.send_first as i64 + i64::from(by) * shown as i64).clamp(0, self.buses.len().saturating_sub(shown) as i64) as usize;
        self.send_first = to;
        if let Some(s) = self.stop.filter(|s| s.control >= SEND) {
            self.stop = Some(Stop { strip: s.strip, control: SEND + s.control.saturating_sub(SEND).clamp(to, to + shown - 1) });
        }
        self.show();
    }

    /// The strip of the view's row `row` (-1: the master), a touch's.
    fn at(&self, row: i32) -> usize {
        if row < 0 { self.master() } else { self.first + row as usize }
    }

    /// A touch takes over from the cursor, as a tap drops the cursors.
    fn touched(&mut self) {
        self.stop = None;
        self.editing = false;
    }

    /// A fader moved to `place` of its travel; sent when it moved a hundredth or more, so a drag is
    /// some tens of small requests, as the JUCE slider's.
    pub fn level(&mut self, row: i32, place: f32) {
        self.touched();
        let at = self.at(row);
        let (max, was) = if at == self.master() { (2., self.master.0) } else { (1., self.strips.get(at).map_or(0., |s| s.level)) };
        let want = level_at(place, max);
        if (want - was).abs() >= 0.01 {
            self.set_level(at, want);
        }
        self.show();
    }

    /// A pan slider moved to `place` (0..1) across; a double tap sends the middle.
    pub fn pan(&mut self, row: i32, place: f32) {
        self.touched();
        let at = self.at(row);
        self.set_pan(at, f64::from(place) * 2. - 1.);
        self.show();
    }

    /// The slider of the send shown `j`th moved.
    pub fn send(&mut self, row: i32, j: i32, place: f32) {
        self.touched();
        let at = self.at(row);
        self.set_send(at, self.send_first + j.max(0) as usize, f64::from(place));
        self.show();
    }

    pub fn tap_mute(&mut self, row: i32) {
        self.touched();
        self.toggle_mute(self.at(row));
        self.show();
    }

    pub fn tap_solo(&mut self, row: i32) {
        self.touched();
        self.toggle_solo(self.at(row));
        self.show();
    }

    /// A finger on a control (`control`: 0 the fader, 3 the pan, 4 and on the sends shown) or off it.
    pub fn hold(&mut self, row: i32, control: i32, down: bool) {
        self.held = down.then(|| {
            let at = self.at(row);
            let control = control.max(0) as usize;
            (self.strips.get(at).map(|s| s.id), if control >= SEND { control + self.send_first } else { control })
        });
    }

    /// The cursor drops when the screen goes, as on focus loss in the JUCE lists.
    pub fn hidden(&mut self) {
        self.stop = None;
        self.editing = false;
        self.held = None;
        self.said.clear();
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

    // The meters' scale is -60..+6 dB across the bar, in hundredths: silence (and anything quieter
    // than -60 dB) sits at 0, 0 dB is 91 hundredths up, loud tops out and clamps at 1.
    #[test]
    fn meter_is_the_juce_scale_in_hundredths() {
        assert_eq!(meter(1.), 0.91); // 0 dB, 60 of the 66 dB of travel
        assert_eq!(meter(0.1), 0.61); // -20 dB
        assert_eq!(meter(0.5), 0.82); // -6 dB
        assert_eq!(meter(0.), 0.);
        assert_eq!(meter(1e-6), 0.); // quieter than -60 dB
        assert_eq!(meter(-5.), 0.);
        assert_eq!(meter(2.), 1.); // +6 dB and louder
    }

    // A level's dB as the JUCE strip says it: one decimal, the word for minus infinity at nothing
    // (and at any non-positive).
    #[test]
    fn db_is_one_decimal_and_silent_below_something() {
        assert_eq!(db(1.), "0.0 dB");
        assert_eq!(db(0.01), "-40.0 dB");
        assert_eq!(db(100.), "40.0 dB");
        assert_eq!(db(0.), "−∞ dB");
        assert_eq!(db(-3.), "−∞ dB");
    }

    #[test]
    fn thousandths_rounds_to_three_places() {
        assert_eq!(thousandths(1.23456), 1.235);
        assert_eq!(thousandths(-0.23456), -0.235);
        assert_eq!(thousandths(0.5), 0.5);
        assert_eq!(thousandths(0.0004), 0.);
    }

    // A send moves by a fortieth of the slider a click, held at the ends, nothing past them.
    #[test]
    fn stepped_moves_a_fortieth_a_click_and_holds_at_the_ends() {
        assert_eq!(stepped(0., 1), 0.025);
        assert_eq!(stepped(0.5, 1), 0.525);
        assert_eq!(stepped(0.4, 0), 0.4);
        assert_eq!(stepped(1., 100), 1.);
        assert_eq!(stepped(0., -100), 0.);
    }

    // graph.get read as the JUCE mixer page reads it: the non-bus parts, then the buses, each with
    // its state and the send to each bus in the order of the buses (None where there is none), and
    // the master's own level and pan.
    #[test]
    fn read_parts_then_buses_with_sends_in_bus_order() {
        let g = json!({
            "parts": [
                {"id": 1, "name": "Lead", "input": {"kind": "wave"}, "level": 0.5, "mute": true, "solo": false, "pan": -1., "sends": [{"bus": 101, "level": 0.25}]},
                {"id": 101, "name": "FX1", "input": {"kind": "bus"}, "level": 0.7},
                {"id": 2, "name": "Drums", "input": {"kind": "sample"}, "level": 0.3, "sends": [{"bus": 999, "level": 0.9}, {"bus": 102, "level": 0.5}]}
            ],
            "master": {"level": 1.5, "pan": 0.5},
            "wires": []
        });
        let (strips, buses, master) = read(&g);
        assert_eq!(buses, vec![(101, "FX1".to_string())]);
        // The parts, then the bus: Lead, Drums, FX1.
        assert_eq!(strips.iter().map(|s| &s.name).collect::<Vec<_>>(), vec!["Lead", "Drums", "FX1"]);
        assert_eq!((strips[0].id, strips[0].bus, strips[0].mute, strips[0].pan), (1, false, true, -1.));
        assert_eq!(strips[0].sends, vec![Some(0.25)]); // the send to FX1, the only bus
        assert_eq!(strips[1].sends, vec![None]); // Drums' send is to an unknown bus
        assert!(strips[2].bus);
        assert_eq!(strips[2].sends, vec![None]);
        assert_eq!(master, (1.5, 0.5));
    }

    // A strip another part solos through a wire is silenced even though it is not muted; its own
    // solo and mute, the level and pan defaults, and the master's all fall back when missing.
    #[test]
    fn read_defaults_missing_fields_and_silences_by_anothers_solo() {
        let g = json!({
            "parts": [
                {"id": 1, "input": {}, "level": 0.4},
                {"id": 2, "input": {}, "level": 0.5}
            ],
            "wires": [{"owner": "part", "part": 1, "fader": true, "mute": true}],
            "master": {}
        });
        let (strips, _buses, master) = read(&g);
        assert!(strips[0].silenced); // the other part's solo mutes it
        assert!(!strips[1].silenced);
        assert_eq!(master, (1., 0.));
        // Nothing at all: the master alone, quiet and centered.
        let (strips, buses, master) = read(&json!({}));
        assert!(strips.is_empty());
        assert!(buses.is_empty());
        assert_eq!(master, (1., 0.));
    }
}
