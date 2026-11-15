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
//! Not here, handed to the JUCE app (the "Rename, sound, effects, sends" row): the name, choosing
//! the sound, the chain's plugins and splits, sends and where it plays, MIDI out, the input
//! channel, velocities, notes at once, reserve and priority.

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
    /// Any edit: its refusal is said.
    Edit,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Cancel with nothing to drop: back to Play.
    Back,
    /// The JUCE app's Performance, for what is not here.
    Handover,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Level,
    Pan,
    Mute,
    Solo,
    Devices,
    LowKey,
    HighKey,
    Transpose,
    Pedal(usize),
    Handover,
}

const PEDALS: [&str; 3] = ["sustain", "sostenuto", "soft"];
const PEDAL_NAMES: [&str; 3] = ["Sustain pedal", "Sostenuto pedal", "Soft pedal"];
/// A pedal's receive as the file says it and the JUCE page words it.
const RECEIVE: [(&str, &str); 3] = [("all", "Every pedal"), ("none", "None"), ("own", "Its devices' pedals")];
const TRANSPOSE: i64 = 24;

/// A level in dB, as the JUCE page's label has it.
fn db(level: f64) -> String {
    if level > 0. { format!("{:.1} dB", 20. * level.log10()).replace('-', "−") } else { "−∞ dB".into() }
}

fn pan_text(pan: f64) -> String {
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
    /// session.status's midiInputs.
    inputs: Vec<Value>,
    cursor: Cursor,
    /// The dial changes the value under the cursor.
    editing: bool,
    /// The devices list, open: its own cursor.
    menu: Option<Cursor>,
    /// The read-only panel's own cursor: its view turns a page at a time.
    info_cursor: Cursor,
    rows: Rc<VecModel<SettingData>>,
    info: Rc<VecModel<InfoLine>>,
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
        Screen { ui: window.as_weak(), graph: Value::Null, id: 0, part: Value::Null, inputs: vec![], cursor: Cursor::new(0, 1), editing: false, menu: None, info_cursor: Cursor::new(0, 1), rows, info, out: vec![] }
    }

    /// The part to show, from the graph already read.
    pub fn open(&mut self, id: u32) {
        self.id = id as u64;
        self.cursor = Cursor::new(0, 1);
        self.editing = false;
        self.menu = None;
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
            if self.menu.is_some() {
                self.show();
            }
        }
    }

    /// Whether an edit was refused: the shell then asks the graph again, which puts the page right.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Status, Ok(r)) => self.endpoints(&r),
            (Request::Edit, Err(e)) if e.code != 0 => {
                self.say(format!("The engine refused: {}", e.message));
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
        if self.part["input"]["kind"] == "notes" {
            items.extend([Item::Devices, Item::LowKey, Item::HighKey, Item::Transpose, Item::Pedal(0), Item::Pedal(1), Item::Pedal(2)]);
        }
        items.push(Item::Handover);
        items
    }

    /// The devices list's choices, as the JUCE page's menu: any, the kinds, then each input; a rule is None for "any".
    fn options(&self) -> Vec<(String, Option<Value>)> {
        let rule = |kind: &str, port: i64, pattern: &str, matches: &str| json!({"kind": kind, "port": port, "pattern": pattern, "match": matches});
        let mut options: Vec<(String, Option<Value>)> = vec![("Any device".into(), None)];
        for kind in ["builtIn", "external", "usb", "network"] {
            let r = rule(kind, 0, "", "contains");
            options.push((devices_name(&json!([r.clone()])), Some(r)));
        }
        for e in &self.inputs {
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
        options
    }

    fn zone(&self) -> (i64, i64, i64) {
        let z = &self.part["input"]["zones"][0];
        (z["keys"][0].as_i64().unwrap_or(0), z["keys"][1].as_i64().unwrap_or(127), z["transpose"].as_i64().unwrap_or(0))
    }

    fn pedal(&self, i: usize) -> usize {
        RECEIVE.iter().position(|r| self.part["input"]["receive"][PEDALS[i]] == r.0).unwrap_or(0)
    }

    fn setting(&self, item: Item) -> SettingData {
        let level = self.part["level"].as_f64().unwrap_or(1.);
        let (low, high, transpose) = self.zone();
        let flag = |key: &str| self.part[key].as_bool().unwrap_or(false);
        let (label, value, kind, on): (String, String, i32, bool) = match item {
            Item::Level => ("Level".into(), db(level), 0, false),
            Item::Pan => ("Pan".into(), pan_text(self.part["pan"].as_f64().unwrap_or(0.)), 0, false),
            Item::Mute => ("Mute".into(), if flag("mute") { "On" } else { "Off" }.into(), 1, flag("mute")),
            Item::Solo => ("Solo".into(), if flag("solo") { "On" } else { "Off" }.into(), 1, flag("solo")),
            Item::Devices => ("Plays from".into(), devices_name(&self.part["input"]["devices"]) + " ›", 3, false),
            Item::LowKey => ("Lowest key".into(), note_name(low), 0, false),
            Item::HighKey => ("Highest key".into(), note_name(high), 0, false),
            Item::Transpose => ("Transpose".into(), signed(transpose), 0, false),
            Item::Pedal(i) => (PEDAL_NAMES[i].into(), RECEIVE[self.pedal(i)].1.into(), 0, false),
            Item::Handover => ("Rename, sound, effects, sends".into(), "Oscairn app ›".into(), 2, false),
        };
        SettingData { label: label.into(), value: value.into(), kind, on, cursor: false, editing: false }
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
        let head = |t: &str| InfoLine { head: true, text: t.into() };
        let line = |t: String| InfoLine { head: false, text: t.into() };
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
        let Some(ui) = self.ui.upgrade() else { return };
        let state = ui.global::<PartState>();
        let per_view = state.get_rows_per_view().max(1) as usize;
        let items = self.items();
        self.cursor = fit(self.cursor, items.len(), per_view);
        let options = if self.menu.is_some() { self.options() } else { vec![] };
        if let Some(m) = self.menu {
            self.menu = Some(fit(m, options.len(), per_view));
        }
        let (rows, cursor): (Vec<SettingData>, Cursor) = match self.menu {
            Some(m) => {
                let ticked = |o: &Option<Value>| match o {
                    None => self.part["input"]["devices"].as_array().is_none_or(|d| d.is_empty()),
                    Some(r) => self.part["input"]["devices"].as_array().is_some_and(|d| d.contains(r)),
                };
                (options.iter().map(|(label, o)| SettingData { label: label.as_str().into(), value: if ticked(o) { "✓" } else { "" }.into(), kind: 1, on: ticked(o), ..Default::default() }).collect(), m)
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
        state.set_title(if self.menu.is_some() { "Plays from: press to tick or untick" } else { "" }.into());
        state.set_view(if cursor.count == 0 { String::new() } else { format!("{} {}–{} of {}", if self.menu.is_some() { "Devices" } else { "Settings" }, cursor.first + 1, end, cursor.count) }.into());
    }

    fn edit(&mut self, method: &'static str, params: Value) {
        self.say(String::new());
        self.out.push((Request::Edit, method, params));
    }

    fn send_input(&mut self) {
        let params = json!({"part": self.id, "input": self.part["input"].clone()});
        self.edit("parts.input", params);
    }

    fn send_level(&mut self) {
        let params = json!({"part": self.id, "level": self.part["level"].as_f64().unwrap_or(1.), "mute": self.part["mute"].as_bool().unwrap_or(false)});
        self.edit("parts.level", params);
    }

    /// The first zone's keys and transpose set, as the JUCE page's setZone does: a zone that passes
    /// everything is no zone, and the highest key is never under the lowest.
    fn set_zone(&mut self, low: i64, high: i64, transpose: i64) {
        let mut list = self.part["input"]["zones"].as_array().cloned().unwrap_or_default();
        let mut zone = list.first().cloned().unwrap_or_else(|| json!({"keys": [0, 127], "velocities": [0, 1], "channels": 65535, "transpose": 0, "curve": 1, "fixedVelocity": 0}));
        zone["keys"] = json!([low, high]);
        zone["transpose"] = json!(transpose);
        let n = |v: &Value, d: f64| v.as_f64().unwrap_or(d);
        let everything = (low, high, transpose) == (0, 127, 0)
            && (n(&zone["velocities"][0], 0.), n(&zone["velocities"][1], 1.), n(&zone["channels"], 65535.), n(&zone["curve"], 1.), n(&zone["fixedVelocity"], 0.)) == (0., 1., 65535., 1., 0.);
        if list.is_empty() {
            if !everything {
                list.push(zone);
            }
        } else if everything && list.len() == 1 {
            list.clear();
        } else {
            list[0] = zone;
        }
        self.part["input"]["zones"] = Value::Array(list);
    }

    /// The value under the cursor stepped `by`, sent when it changed.
    fn dial(&mut self, item: Item, by: i64) {
        let (low, high, transpose) = self.zone();
        match item {
            Item::Level => {
                let was = self.part["level"].as_f64().unwrap_or(1.);
                let mut f = (fraction(was, 1.) + 0.02 * by as f32).clamp(0., 1.);
                let mut level = level_at(f, 1.);
                // Near silence a step is less than a thousandth: go on until it is heard.
                while level == was && f > 0. && f < 1. {
                    f = (f + 0.02 * by.signum() as f32).clamp(0., 1.);
                    level = level_at(f, 1.);
                }
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
            Item::Mute | Item::Solo | Item::Devices | Item::Handover => {}
        }
    }

    fn toggle(&mut self, option: Option<Value>) {
        let mut rules = self.part["input"]["devices"].as_array().cloned().unwrap_or_default();
        match option {
            None => rules.clear(),
            Some(r) => match rules.iter().position(|x| x == &r) {
                Some(p) => {
                    rules.remove(p);
                }
                None => rules.push(r),
            },
        }
        self.part["input"]["devices"] = Value::Array(rules);
        self.send_input();
    }

    /// A press on the item under the cursor.
    fn confirm(&mut self) -> Outcome {
        if let Some(m) = self.menu {
            if let Some(o) = m.at.and_then(|at| self.options().into_iter().nth(at)) {
                self.toggle(o.1);
            }
            return Outcome::Stay;
        }
        let Some(item) = self.cursor.at.and_then(|at| self.items().get(at).copied()) else { return Outcome::Stay };
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
            Item::Devices => self.menu = Some(Cursor::new(0, 1)),
            Item::Handover => return Outcome::Handover,
            _ => self.editing = !self.editing,
        }
        Outcome::Stay
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
            Some(m) => *m = m.moved(by),
            None if !self.editing => self.cursor = self.cursor.moved(by),
            None => {}
        }
    }

    /// The item of the row `row` of the view, as an index into the list shown.
    fn at(&self, row: i32) -> usize {
        self.menu.unwrap_or(self.cursor).first + row.max(0) as usize
    }

    /// A tap: the cursor to the row and what a press does; on a value, the dial then changes it.
    pub fn tap(&mut self, row: i32) -> Outcome {
        let at = self.at(row);
        match &mut self.menu {
            Some(m) => *m = m.to(at),
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
        self.show();
    }

    /// Shown: the view's size may have changed while it was not.
    pub fn shown(&mut self) {
        self.show();
    }
}
