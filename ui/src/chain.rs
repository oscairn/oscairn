// SPDX-License-Identifier: AGPL-3.0-or-later
//! A part's insert chain (the JUCE Part page's chain row, hosts/standalone/PartsView.cpp and
//! PartsModel.cpp's insertPlugin, insertSplit, removeBox, removeSplitKeepingFirst, removeLaneAndBoxes, editLane):
//! its plugins in order, with "+ Insert plugin here" before each and after the last; a press on a plugin opens
//! what can be done to it (its parameters, turn it off or on, move it earlier or later, remove it, and where it
//! has a window of its own a disabled entry saying that view is not available in this shell yet); a press on an
//! insert row lists what can go there: the four kinds of split (on the part's own chain only, as `insertMenu`)
//! and the plugins the isolated scanner found (scan.rs, without Split and Join, which belong to splits), and
//! choosing one adds it there.
//!
//! A split is a row of the chain; a press on it lists its lanes (a lane is named Left and Right, Mid and Side,
//! Band n, or Lane n for the layers of a copy, with its keys, level and mute or solo as the reference writes
//! them at the lane's right), a band split's crossovers (the Split plugin's parameters 3 to 5, in Hz, a dial step a
//! sixth of an octave), Add lane (Add band), which a copy and a band split of fewer than four have, and Remove split.
//! A press on a lane lists its own plugins as the chain lists the part's (with the same actions and "+ Insert
//! plugin here", moving within the lane), then its settings: the lowest and highest key, level, pan, mute, solo,
//! and Remove lane (a copy, or a band split of more than two lanes). A split inside a lane is listed and not
//! opened, as the reference draws it as a gap.
//!
//! Requests, each followed by `graph.commit` as the JUCE setters commit: inserting is `graph.add` (named
//! "Basic Synth", then "Basic Synth 2", as `addNamed`) and, when it answers, `parts.insert` (with `split` and
//! `lane` inside a lane), the node taken out again with `graph.remove` if the insert is refused; a split other
//! than a copy is `graph.add` of the Split plugin (and of the Join plugin for left / right and mid / side), then
//! `parts.addSplit` with their nodes, which are taken out again if it is refused; `parts.move`, `graph.enable`;
//! removing is `parts.removeNode` then `graph.remove` with `userWires: reconnect` (as `removeBox`); removing a
//! split is `parts.removeSplit` then `graph.remove` of the nodes of every lane but the first, and removing a lane
//! `parts.removeLane` then `graph.remove` of its nodes (all with the wires reconnected, as the reference does);
//! `parts.addLane`; a lane's settings are `parts.lane` with the whole lane (zones, level, pan, mute, solo);
//! a crossover is `params.set` (it commits nothing, as `setParam`). None of the removals asks first, as the
//! reference's do not: the top bar's Undo takes them back. `window.info` is asked for a plugin whose actions are
//! shown. Without touch: the left knob or the dial moves the cursor, a press chooses (on a value it lets the dial
//! change it, a second press or Cancel lets go), Cancel goes back one layer (actions or plugins, a lane, a split,
//! then the chain, then the Part screen); the right knob scrolls the read-only panel.

use slint::ComponentHandle;
use crate::browser::{Browser, Nav};
use crate::menu::Item;
use crate::nav::Action;
use crate::parts::{role_color, role_of};
use crate::part::{edit_zones, pan_text};
use crate::play::note_name;
use crate::scan::Plugin;
use crate::{InfoLine, SettingData, Shell};
use serde_json::{json, Value};
use std::collections::HashMap;

const SPLIT_PLUGIN: &str = "org.oscairn.split";
const JOIN_PLUGIN: &str = "org.oscairn.join";

/// The frequency range of a crossover (the reference's slider) and the frequency in the middle of its throw.
const CROSSOVER: (f64, f64, f64) = (20., 20000., 1000.);

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Any edit: its refusal is said.
    Edit,
    /// graph.add for a plugin to be inserted at `index`: its node comes back.
    Add(usize),
    /// parts.insert of that node: refused, it is removed again.
    Insert(u64),
    /// window.info for a node.
    Window(u64),
    /// params.list of a node, for the scene controls menu.
    List(u64),
    /// graph.add of the Split plugin for a new split.
    Splitter,
    /// graph.add of the Join plugin for a new split.
    Joiner,
    /// parts.addSplit: refused, the nodes added for it are removed again.
    NewSplit,
    /// params.get of a band split's crossovers.
    Crossovers,
    /// presets.list of a node.
    Presets(u64),
    /// graph.replace of a node with one of its presets.
    Replace,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Back to the Part screen.
    Back,
    /// A plugin's parameter pages: its node and name.
    Params(u64, String),
    /// The plugin in Routing.
    Routing(u64),
}

/// Where a chain is: the part's own, or a lane of a split.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Part,
    Lane(u64, usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// The part's chain.
    List,
    /// A split's lanes, crossovers and actions.
    Split(u64),
    /// A lane's plugins and settings.
    Lane(u64, usize),
    /// The actions on the plugin at this place in a chain.
    Actions(Place, usize),
    /// What to insert at this place in a chain.
    Plugins(Place, usize),
    /// The presets of the plugin at this place in a chain.
    Presets(Place, usize),
}

impl Mode {
    /// The chain this mode shows or acts on.
    fn place(self) -> Place {
        match self {
            Mode::Lane(s, l) => Place::Lane(s, l),
            Mode::Actions(p, _) | Mode::Plugins(p, _) | Mode::Presets(p, _) => p,
            _ => Place::Part,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Insert(usize),
    Node(usize),
    Split(usize),
    /// A split inside a lane: listed, not opened.
    Nested,
    Lane(usize),
    Crossover(usize),
    AddLane,
    RemoveSplit,
    LowKey,
    HighKey,
    Level,
    Pan,
    Mute,
    Solo,
    RemoveLane,
    Parameters,
    Power,
    Presets,
    Preset(usize),
    AskAgain,
    Earlier,
    Later,
    Remove,
    Routing,
    SceneControls,
    View,
    NewSplit(&'static str),
    Choose(usize),
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Node(u64),
    Split(u64),
}

/// A split being made: the nodes it needs are added one at a time, then the split itself.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    index: usize,
    kind: &'static str,
    splitter: u64,
    joiner: u64,
}

/// What a split of that kind is called.
fn split_kind(kind: &str) -> &'static str {
    match kind {
        "leftRight" => "left / right",
        "midSide" => "mid / side",
        "bands" => "frequency bands",
        "ports" => "output ports",
        _ => "layers",
    }
}

/// The four kinds of split an insert row offers, in the reference's order and words.
const NEW_SPLITS: [(&str, &str); 4] = [
    ("copy", "Split here: layers"),
    ("leftRight", "Split here: left / right"),
    ("midSide", "Split here: mid / side"),
    ("bands", "Split here: frequency bands"),
];

/// A crossover's frequency stepped `by` sixths of an octave (at least a hertz): the reference's slider has no
/// step, it is dragged along a scale that puts 1000 Hz in the middle of its throw, so equal steps there are
/// equal ratios.
fn stepped_crossover(was: f64, by: i64) -> f64 {
    let (min, max, _) = CROSSOVER;
    let now = (was * 2f64.powf(by.clamp(-200, 200) as f64 / 6.)).clamp(min, max).round();
    if now == was && by != 0 { (was + by.signum() as f64).clamp(min, max) } else { now }
}

/// A level or a pan stepped, to the hundredth.
fn stepped(was: f64, by: i64, step: f64, least: f64) -> f64 {
    ((was + step * by as f64).clamp(least, 1.) * 100.).round() / 100.
}

pub struct Screen {
    b: Browser,
    graph: Value,
    id: u64,
    mode: Mode,
    /// The plugin the actions or presets shown are for: they follow it when the chain changes
    /// under them (an undo, another editor), as their place alone would name another plugin.
    acting: Option<u64>,
    plugins: Vec<Plugin>,
    scanning: bool,
    /// Which nodes have a window of their own, as window.info said.
    windows: HashMap<u64, bool>,
    /// The scene controls menu of a node, ready for the shell to open, and what its choices are.
    ready_menu: Option<(String, Vec<Item>)>,
    picks: Vec<(u64, u64)>,
    /// The engine's refusal of the last edit, said until the next.
    error: String,
    /// The plugins that failed and are silent (node, name, reason), from `session.status`.
    failures: Vec<(u64, String, String)>,
    /// Where the plugin being added goes.
    placing: Place,
    /// The split being made.
    pending: Option<Pending>,
    /// A band split's crossovers as the engine said them, by node and parameter.
    crossovers: HashMap<(u64, u64), f64>,
    /// The presets of the plugin whose presets show (uri and name), or why they could not be listed.
    presets: Vec<(String, String)>,
    presets_error: String,
    /// A finger on the lane's key strip, and the window's state it draws on.
    drag: Option<crate::keystrip::Drag>,
    ui: slint::Weak<Shell>,
    /// The preset a plugin is being reloaded with.
    reloading: String,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen {
            b: Browser::new(ui),
            graph: Value::Null,
            id: 0,
            mode: Mode::List,
            acting: None,
            plugins: vec![],
            scanning: true,
            windows: HashMap::new(),
            ready_menu: None,
            picks: vec![],
            error: String::new(),
            failures: vec![],
            placing: Place::Part,
            pending: None,
            crossovers: HashMap::new(),
            presets: vec![],
            presets_error: String::new(),
            drag: None,
            ui: ui.as_weak(),
            reloading: String::new(),
            out: vec![],
        }
    }

    /// A lane's keys set on the graph as read, and sent: the whole lane, as `editLane` sends it.
    fn set_lane_keys(&mut self, s: u64, k: usize, l: i64, h: i64) {
        if let Some(lane) = self.lane_mut(s, k) {
            let zones = lane["zones"].as_array().cloned().unwrap_or_default();
            lane["zones"] = Value::Array(edit_zones(zones, |z| z["keys"] = json!([l, h])));
        }
        self.send_lane(s, k);
    }

    /// The key strip of the lane open, over the panel (the reference's ZoneEditor): the range as the lane has it, or as
    /// a finger drags it, and the end the cursor's row changes.
    fn show_strip(&self) {
        let Some(ui) = self.ui.upgrade().filter(|_| self.b.active()) else { return };
        let state = ui.global::<crate::StripState>();
        let Mode::Lane(s, k) = self.mode else {
            if state.get_shown() {
                state.set_shown(false);
            }
            return;
        };
        let (low, high) = self.drag.map_or_else(|| self.lane_keys(s, k), |d| d.now);
        let end = match self.b.cursor.at.and_then(|at| self.rows().get(at).map(|r| r.0)) {
            Some(Row::LowKey) => 1,
            Some(Row::HighKey) => 2,
            _ => 0,
        };
        state.set_low(low as i32);
        state.set_high(high as i32);
        state.set_end(end);
        state.set_range(crate::keystrip::range_text(low, high).into());
        if !state.get_shown() {
            state.set_shown(true);
        }
    }

    /// A finger down on the strip at `share` of its `width` pixels.
    pub fn strip_down(&mut self, share: f64, width: f64) {
        if let Mode::Lane(s, k) = self.mode {
            let (low, high) = self.lane_keys(s, k);
            self.drag = Some(crate::keystrip::Drag::begin(low, high, share, width));
            self.show_strip();
        }
    }

    pub fn strip_move(&mut self, share: f64) {
        if let Some(d) = self.drag.as_mut() {
            d.to(share);
            self.show_strip();
        }
    }

    /// The finger lifts: one edit for the whole drag.
    pub fn strip_up(&mut self) {
        let Some(d) = self.drag.take() else { return };
        if let Mode::Lane(s, k) = self.mode {
            if d.now != self.lane_keys(s, k) {
                self.set_lane_keys(s, k, d.now.0, d.now.1);
            }
        }
        self.show();
    }

    /// A double tap: the whole keyboard.
    pub fn strip_reset(&mut self) {
        self.drag = None;
        if let Mode::Lane(s, k) = self.mode {
            if self.lane_keys(s, k) != (0, 127) {
                self.set_lane_keys(s, k, 0, 127);
            }
        }
        self.show();
    }

    pub fn at_top(&self) -> bool {
        self.b.at_top()
    }

    pub fn open(&mut self, id: u32) {
        self.id = id as u64;
        self.mode = Mode::List;
        self.pending = None;
        self.b.reset();
        self.say("");
        self.show();
    }

    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
        self.ask_crossovers();
        self.show();
    }

    /// The scanner's answer: the plugins there are to insert.
    pub fn plugins(&mut self, found: Vec<Plugin>) {
        self.plugins = crate::scan::choices(found);
        self.scanning = false;
        self.show();
    }

    /// What the line under the list says: an edit that was refused, else a plugin that failed.
    fn say(&mut self, text: &str) {
        self.error = text.to_string();
        self.b.say(&self.message());
    }

    fn message(&self) -> String {
        match (self.error.is_empty(), self.failures.first()) {
            (false, _) => self.error.clone(),
            (true, Some((_, name, reason))) => format!("{name} is silent: {reason}"),
            _ => String::new(),
        }
    }

    /// `session.status`: the plugins that failed, outlined and named wherever they show.
    pub fn status(&mut self, status: &Value) {
        let Some(list) = status["failures"].as_array() else { return };
        let failures: Vec<(u64, String, String)> = list.iter().map(|f| (f["node"].as_u64().unwrap_or(0), f["name"].as_str().unwrap_or_default().into(), f["reason"].as_str().unwrap_or_default().into())).collect();
        if failures != self.failures {
            self.failures = failures;
            self.show();
        }
    }

    fn failed(&self, node: u64) -> bool {
        node != 0 && self.failures.iter().any(|f| f.0 == node)
    }

    /// The part's name, for the bar's title.
    pub fn part_name(&self) -> String {
        self.part().and_then(|p| p["name"].as_str()).unwrap_or_default().to_string()
    }

    /// Back from anywhere on the screen: up a level in the chain, else out of the screen.
    pub fn back(&mut self) -> Outcome {
        self.handle(Nav::Back)
    }

    fn part(&self) -> Option<&Value> {
        self.graph["parts"].as_array().and_then(|p| p.iter().find(|p| p["id"].as_u64() == Some(self.id)))
    }

    fn node(&self, id: u64) -> Option<&Value> {
        self.graph["nodes"].as_array().and_then(|n| n.iter().find(|n| n["id"].as_u64() == Some(id)))
    }

    /// The entries of a chain in order: a plugin, or a split.
    fn entries_of(chain: &Value) -> Vec<Entry> {
        chain.as_array().into_iter().flatten().filter_map(|e| e.as_u64().map(Entry::Node).or_else(|| e["split"].as_u64().map(Entry::Split))).collect()
    }

    /// The part's chain in order: a plugin, or a split (its lanes' plugins are in `lane_entries`).
    fn entries(&self) -> Vec<Entry> {
        self.part().map(|p| Self::entries_of(&p["chain"])).unwrap_or_default()
    }

    fn split(&self, id: u64) -> Option<&Value> {
        self.part().and_then(|p| p["splits"].as_array()).and_then(|l| l.iter().find(|x| x["id"].as_u64() == Some(id)))
    }

    fn lane_count(&self, split: u64) -> usize {
        self.split(split).and_then(|x| x["lanes"].as_array()).map_or(0, Vec::len)
    }

    fn lane(&self, split: u64, lane: usize) -> Option<&Value> {
        self.split(split).and_then(|s| s["lanes"].as_array()).and_then(|l| l.get(lane))
    }

    fn lane_mut(&mut self, split: u64, lane: usize) -> Option<&mut Value> {
        let id = self.id;
        let part = self.graph["parts"].as_array_mut()?.iter_mut().find(|p| p["id"].as_u64() == Some(id))?;
        let s = part["splits"].as_array_mut()?.iter_mut().find(|x| x["id"].as_u64() == Some(split))?;
        s["lanes"].as_array_mut()?.get_mut(lane)
    }

    fn kind_of(&self, split: u64) -> &str {
        self.split(split).and_then(|x| x["kind"].as_str()).unwrap_or("copy")
    }

    /// The entries of a split's lane.
    fn lane_entries(&self, split: u64, lane: usize) -> Vec<Entry> {
        self.lane(split, lane).map(|l| Self::entries_of(&l["chain"])).unwrap_or_default()
    }

    /// The entries of the chain at a place.
    fn entries_at(&self, place: Place) -> Vec<Entry> {
        match place {
            Place::Part => self.entries(),
            Place::Lane(s, l) => self.lane_entries(s, l),
        }
    }

    /// The nodes of a chain, those in its splits' lanes included.
    fn nodes_in(&self, chain: &Value, depth: usize, out: &mut Vec<u64>) {
        for e in Self::entries_of(chain) {
            match e {
                Entry::Node(n) => out.push(n),
                Entry::Split(s) if depth < 8 => {
                    for lane in self.split(s).and_then(|x| x["lanes"].as_array()).into_iter().flatten() {
                        self.nodes_in(&lane["chain"], depth + 1, out);
                    }
                }
                Entry::Split(_) => {}
            }
        }
    }

    fn name_of(&self, node: u64) -> String {
        self.node(node).and_then(|n| n["name"].as_str()).unwrap_or("a plugin").to_string()
    }

    /// What a lane is called: Left and Right, Mid and Side, Band n; "Lane n" for the layers of a copy.
    fn lane_label(&self, split: u64, lane: usize) -> String {
        match (self.kind_of(split), lane) {
            ("leftRight", 0) => "Left".into(),
            ("leftRight", _) => "Right".into(),
            ("midSide", 0) => "Mid".into(),
            ("midSide", _) => "Side".into(),
            ("bands", k) => format!("Band {}", k + 1),
            (_, k) => format!("Lane {}", k + 1),
        }
    }

    /// A lane's keys: lowest and highest.
    fn lane_keys(&self, split: u64, lane: usize) -> (i64, i64) {
        let z = self.lane(split, lane).map_or(&Value::Null, |l| &l["zones"][0]);
        (z["keys"][0].as_i64().unwrap_or(0), z["keys"][1].as_i64().unwrap_or(127))
    }

    fn lane_number(&self, split: u64, lane: usize, key: &str, or: f64) -> f64 {
        self.lane(split, lane).and_then(|l| l[key].as_f64()).unwrap_or(or)
    }

    fn lane_flag(&self, split: u64, lane: usize, key: &str) -> bool {
        self.lane(split, lane).is_some_and(|l| l[key].as_bool().unwrap_or(false))
    }

    /// Whether a lane cannot be heard: muted, or another lane of its split is solo.
    fn silent(&self, split: u64, lane: usize) -> bool {
        let solo = (0..self.lane_count(split)).any(|k| self.lane_flag(split, k, "solo"));
        self.lane_flag(split, lane, "mute") || (solo && !self.lane_flag(split, lane, "solo"))
    }

    /// What a lane says about itself at its right, as the reference writes it (its row here has the name):
    /// "Dry" for a copy's lane without plugins, its keys when not all, its level, muted or solo.
    fn lane_about(&self, split: u64, lane: usize) -> String {
        let mut about = if self.kind_of(split) == "copy" && self.lane_entries(split, lane).is_empty() { "Dry · ".to_string() } else { String::new() };
        let (low, high) = self.lane_keys(split, lane);
        if low != 0 || high != 127 {
            about += &format!("{}–{} · ", note_name(low), note_name(high));
        }
        about += &format!("{}%", (self.lane_number(split, lane, "level", 1.) * 100.).round());
        if self.lane_flag(split, lane, "mute") {
            about += " · muted";
        } else if self.lane_flag(split, lane, "solo") {
            about += " · solo";
        }
        about
    }

    /// Whether a lane can be removed: the layers of a copy, and the bands of more than two.
    fn lane_removable(&self, split: u64) -> bool {
        match self.kind_of(split) {
            "copy" => true,
            "bands" => self.lane_count(split) > 2,
            _ => false,
        }
    }

    /// The Split plugin's node of a band split and the parameter of crossover `k` (parameters 3 to 5).
    fn crossover_of(&self, split: u64, k: usize) -> Option<(u64, u64)> {
        let node = self.split(split).and_then(|s| s["splitter"].as_u64()).filter(|n| *n != 0)?;
        (self.kind_of(split) == "bands").then_some((node, 3 + k as u64))
    }

    /// The crossovers of the band split shown, asked of the engine.
    fn ask_crossovers(&mut self) {
        let Mode::Split(s) = self.mode else { return };
        let wanted: Vec<Value> = (0..self.lane_count(s).saturating_sub(1)).filter_map(|k| self.crossover_of(s, k)).map(|(node, param)| json!({"node": node, "param": param})).collect();
        if !wanted.is_empty() {
            self.out.push((Request::Crossovers, "params.get", json!({"values": wanted})));
        }
    }

    /// A plugin row: its role word, off and failed marks.
    fn boxed(&self, label: String, node: u64) -> SettingData {
        let n = self.node(node);
        let off = n.is_some_and(|n| n["enabled"] == false);
        let role = n.map_or("Notes", role_of);
        SettingData { label: label.into(), value: format!("{role}{} ›", if off { " (off)" } else { "" }).into(), kind: 2, role: role_color(role), off, failed: self.failed(node), ..Default::default() }
    }

    /// The rows of a chain: "+ Insert plugin here" before each entry and after the last.
    fn chain_rows(&self, entries: &[Entry], nested_ok: bool) -> Vec<(Row, SettingData)> {
        let row = |label: String, value: String, kind: i32| SettingData { label: label.into(), value: value.into(), kind, ..Default::default() };
        let mut rows = Vec::new();
        for i in 0..=entries.len() {
            rows.push((Row::Insert(i), row("+ Insert plugin here".into(), String::new(), 2)));
            match entries.get(i) {
                Some(Entry::Node(n)) => rows.push((Row::Node(i), self.boxed(format!("{}. {}", i + 1, self.name_of(*n)), *n))),
                Some(Entry::Split(s)) if nested_ok => {
                    let value = format!("{} · {} lanes ›", split_kind(self.kind_of(*s)), self.lane_count(*s));
                    rows.push((Row::Split(i), row(format!("{}. Split", i + 1), value, 2)));
                }
                Some(Entry::Split(_)) => rows.push((Row::Nested, row(format!("{}. Split", i + 1), "inside a lane: not editable here".into(), 4))),
                None => {}
            }
        }
        rows
    }

    fn rows(&self) -> Vec<(Row, SettingData)> {
        let row = |label: String, value: String, kind: i32| SettingData { label: label.into(), value: value.into(), kind, ..Default::default() };
        let part_entries = self.entries();
        match self.mode {
            Mode::List => self.chain_rows(&part_entries, true),
            Mode::Split(s) => {
                let mut rows = Vec::new();
                let lanes = self.lane_count(s);
                for k in 0..lanes {
                    let mut r = row(self.lane_label(s, k), format!("{} ›", self.lane_about(s, k)), 2);
                    r.off = self.silent(s, k);
                    rows.push((Row::Lane(k), r));
                }
                for k in 0..lanes.saturating_sub(1) {
                    if let Some(at) = self.crossover_of(s, k) {
                        let hz = self.crossovers.get(&at).copied().unwrap_or(CROSSOVER.2);
                        rows.push((Row::Crossover(k), row(format!("Crossover {}", k + 1), format!("{hz:.0} Hz"), 0)));
                    }
                }
                let bands = self.kind_of(s) == "bands";
                if self.kind_of(s) == "copy" || (bands && lanes < 4) {
                    rows.push((Row::AddLane, row(if bands { "Add band" } else { "Add lane" }.into(), String::new(), 2)));
                }
                rows.push((Row::RemoveSplit, row("Remove split".into(), "keeps the first lane's plugins".into(), 2)));
                rows
            }
            Mode::Lane(s, k) => {
                let mut rows = self.chain_rows(&self.lane_entries(s, k), false);
                let (low, high) = self.lane_keys(s, k);
                rows.push((Row::LowKey, row("Lowest key".into(), note_name(low), 0)));
                rows.push((Row::HighKey, row("Highest key".into(), note_name(high), 0)));
                rows.push((Row::Level, row("Lane level".into(), format!("{}%", (self.lane_number(s, k, "level", 1.) * 100.).round()), 0)));
                rows.push((Row::Pan, row("Lane pan".into(), pan_text(self.lane_number(s, k, "pan", 0.)), 0)));
                for (r, label, key) in [(Row::Mute, "Mute", "mute"), (Row::Solo, "Solo", "solo")] {
                    let on = self.lane_flag(s, k, key);
                    let mut data = row(label.into(), if on { "On" } else { "Off" }.into(), 1);
                    data.on = on;
                    rows.push((r, data));
                }
                if self.lane_removable(s) {
                    rows.push((Row::RemoveLane, row("Remove lane".into(), "and its plugins".into(), 2)));
                }
                rows
            }
            Mode::Actions(place, i) => {
                let entries = self.entries_at(place);
                let Some(Entry::Node(n)) = entries.get(i).copied() else { return vec![] };
                let off = self.node(n).is_some_and(|n| n["enabled"] == false);
                let mut rows = vec![(Row::Parameters, row("Parameters".into(), "›".into(), 2)), (Row::Power, row(if off { "Turn on" } else { "Turn off" }.into(), String::new(), 2)), (Row::Presets, row("Presets...".into(), "›".into(), 2))];
                if i > 0 {
                    rows.push((Row::Earlier, row("Move earlier".into(), String::new(), 2)));
                }
                if i + 1 < entries.len() {
                    rows.push((Row::Later, row("Move later".into(), String::new(), 2)));
                }
                rows.push((Row::Remove, row("Remove".into(), String::new(), 2)));
                rows.push((Row::Routing, row("Routing...".into(), "›".into(), 2)));
                rows.push((Row::SceneControls, row("Scene controls...".into(), "›".into(), 2)));
                if self.windows.get(&n).copied().unwrap_or(false) {
                    rows.push((Row::View, row("Plugin view".into(), "not available in this shell yet".into(), 4)));
                }
                rows
            }
            Mode::Presets(..) => {
                if !self.presets_error.is_empty() {
                    return vec![(Row::Nothing, row(self.presets_error.clone(), String::new(), 4)), (Row::AskAgain, row("Ask again".into(), String::new(), 2))];
                }
                if self.presets.is_empty() {
                    return vec![(Row::Nothing, row("No presets listed".into(), String::new(), 4))];
                }
                self.presets.iter().enumerate().map(|(i, (_, name))| (Row::Preset(i), row(name.clone(), String::new(), 2))).collect()
            }
            Mode::Plugins(place, _) => {
                let mut rows = Vec::new();
                if place == Place::Part {
                    // The kinds that divide the sound need the Split and Join plugins.
                    let splitters = self.plugins.iter().any(|p| p.id == SPLIT_PLUGIN);
                    for (kind, label) in NEW_SPLITS {
                        if kind == "copy" || splitters {
                            rows.push((Row::NewSplit(kind), row(label.into(), "›".into(), 2)));
                        } else {
                            rows.push((Row::Nothing, row(label.into(), "needs the Split and Join plugins".into(), 4)));
                        }
                    }
                }
                if self.plugins.is_empty() {
                    rows.push((Row::Nothing, row(if self.scanning { "Finding plugins..." } else { "No plugins found" }.into(), String::new(), 4)));
                }
                // Split and Join belong to splits; they are not boxes of their own.
                rows.extend(self.plugins.iter().enumerate().filter(|(_, p)| p.id != SPLIT_PLUGIN && p.id != JOIN_PLUGIN).map(|(i, p)| (Row::Choose(i), row(p.name.clone(), if p.is_instrument() { "Instrument" } else { "Effect" }.into(), 2))));
                rows
            }
        }
    }

    /// An end of the chain as the reference draws it: its name small over what it holds, on one box.
    fn end(name: &str, text: Vec<String>) -> Vec<InfoLine> {
        let n = text.len() + 1;
        std::iter::once((name.to_string(), true))
            .chain(text.into_iter().map(|t| (t, false)))
            .enumerate()
            .map(|(i, (t, head))| InfoLine { head, text: t.into(), boxed: true, first: i == 0, last: i + 1 == n, ..Default::default() })
            .collect()
    }

    /// What the part listens to and where it plays to, in words, as the reference's IN and OUT: the devices and the keys
    /// (or what feeds a bus), then Main or the bus it plays through, a line for each send, and an arrow for each plugin
    /// outside the parts its plugins are wired to in Routing.
    fn ends(&self) -> (Vec<String>, Vec<String>) {
        let Some(p) = self.part() else { return (vec![], vec![]) };
        let parts = self.graph["parts"].as_array().cloned().unwrap_or_default();
        let part_name = |id: u64| parts.iter().find(|q| q["id"].as_u64() == Some(id)).and_then(|q| q["name"].as_str()).map(String::from);
        let input = if p["input"]["kind"] == "bus" {
            let from: Vec<String> = parts
                .iter()
                .filter(|q| q["bus"].as_u64() == Some(self.id) || q["sends"].as_array().is_some_and(|s| s.iter().any(|s| s["bus"].as_u64() == Some(self.id))))
                .filter_map(|q| q["name"].as_str().map(String::from))
                .collect();
            if from.is_empty() { "Nothing sends to it yet".to_string() } else { format!("From {}", from.join(", ")) }
        } else {
            crate::play::input_text(p)
        };
        let mut output = vec![match p["bus"].as_u64().filter(|b| *b != 0) {
            Some(b) => part_name(b).unwrap_or_else(|| "a bus".into()),
            None => "Main".into(),
        }];
        for s in p["sends"].as_array().into_iter().flatten() {
            if let Some(name) = s["bus"].as_u64().and_then(part_name) {
                output.push(format!("Send {name} {}%", (s["level"].as_f64().unwrap_or(0.) * 100.).round() as i64));
            }
        }
        // The plugins outside every part that this part's plugins are wired into (Routing's user wires): shared when
        // another part feeds them as well.
        let mut owner: HashMap<u64, u64> = HashMap::new();
        for q in &parts {
            let qid = q["id"].as_u64().unwrap_or(0);
            let mut flat = Vec::new();
            crate::play::chain_nodes(&q["chain"], &q["splits"], 0, &mut flat);
            for n in flat {
                owner.insert(n, qid);
            }
            for s in q["splits"].as_array().into_iter().flatten() {
                for key in ["splitter", "joiner"] {
                    if let Some(n) = s[key].as_u64().filter(|n| *n != 0) {
                        owner.insert(n, qid);
                    }
                }
            }
        }
        let mut fed: std::collections::BTreeMap<u64, Vec<u64>> = std::collections::BTreeMap::new();
        for w in self.graph["wires"].as_array().into_iter().flatten() {
            let (Some(from), Some(to)) = (w["from"].as_u64(), w["to"].as_u64()) else { continue };
            if w["owner"] == "part" || w["kind"] != "audio" || owner.contains_key(&to) || to == u64::from(u32::MAX) {
                continue;
            }
            if let Some(q) = owner.get(&from) {
                let list = fed.entry(to).or_default();
                if !list.contains(q) {
                    list.push(*q);
                }
            }
        }
        for (node, from) in fed {
            if from.contains(&self.id) {
                output.push(format!("→ {}{}", self.name_of(node), if from.len() > 1 { " (shared)" } else { "" }));
            }
        }
        (vec![input], output)
    }

    /// The part's signal path in the read-only panel, as the reference draws it left to right: In, each plugin
    /// or split, Out; a split's lanes under it, a lane without plugins "Dry".
    fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let plain = |t: String| InfoLine { text: t.into(), ..Default::default() };
        let boxed = |text: String, node: u64| {
            let n = self.node(node);
            InfoLine { text: text.into(), role: role_color(n.map_or("Notes", role_of)), off: n.is_some_and(|n| n["enabled"] == false), failed: self.failed(node), ..Default::default() }
        };
        let mut lines = vec![head("Chain")];
        let (input, output) = self.ends();
        lines.extend(Self::end("IN", input));
        let entries = self.entries();
        if entries.is_empty() {
            lines.push(plain("Empty: the sound goes straight through".into()));
        }
        for (i, e) in entries.iter().enumerate() {
            match e {
                Entry::Node(n) => lines.push(boxed(format!("{}. {}", i + 1, self.name_of(*n)), *n)),
                Entry::Split(s) => {
                    lines.push(plain(format!("{}. Split: {}", i + 1, split_kind(self.kind_of(*s)))));
                    for k in 0..self.lane_count(*s) {
                        let inside = self.lane_entries(*s, k);
                        let name = if self.kind_of(*s) == "copy" { format!("Lane {}", k + 1) } else { self.lane_label(*s, k) };
                        if inside.is_empty() {
                            lines.push(plain(format!("    {name}: Dry")));
                        }
                        for (j, x) in inside.iter().enumerate() {
                            let label = if j == 0 { format!("    {name}: ") } else { "        ".into() };
                            lines.push(match x {
                                Entry::Node(n) => boxed(format!("{label}{}", self.name_of(*n)), *n),
                                Entry::Split(_) => plain(format!("{label}Split")),
                            });
                        }
                    }
                }
            }
        }
        lines.extend(Self::end("OUT", output));
        if let Mode::Actions(place, i) | Mode::Presets(place, i) = self.mode {
            if let Some(Entry::Node(n)) = self.entries_at(place).get(i) {
                lines.extend([head("Plugin"), plain(self.node(*n).and_then(|x| x["plugin"]["name"].as_str()).unwrap_or_default().to_string())]);
            }
        }
        if matches!(self.mode, Mode::Presets(..)) {
            // The reference's hint over its preset button.
            lines.push(head("Reload with a preset"));
            lines.extend(["Replacing stops its notes and tails,", "removes its prepared sounds;", "a load that fails keeps the plugin."].map(|t| plain(t.into())));
        }
        lines
    }

    /// A mode whose split, lane or plugin is gone (an undo, another editor) goes back to what is still there.
    fn fit(&mut self) {
        let gone = |s: &Screen, place: Place| match place {
            Place::Part => false,
            Place::Lane(split, lane) => s.lane(split, lane).is_none(),
        };
        self.mode = match self.mode {
            Mode::Split(s) if self.split(s).is_none() => Mode::List,
            Mode::Lane(s, _) if self.split(s).is_none() => Mode::List,
            Mode::Lane(s, k) if self.lane(s, k).is_none() => Mode::Split(s),
            Mode::Actions(p, _) | Mode::Plugins(p, _) | Mode::Presets(p, _) if gone(self, p) => Mode::List,
            m => m,
        };
        if let (Mode::Actions(p, i) | Mode::Presets(p, i), Some(n)) = (self.mode, self.acting) {
            match self.entries_at(p).iter().position(|e| *e == Entry::Node(n)) {
                Some(j) if j != i => self.mode = if matches!(self.mode, Mode::Actions(..)) { Mode::Actions(p, j) } else { Mode::Presets(p, j) },
                Some(_) => {}
                None => self.mode = Mode::List,
            }
        }
        self.acting = match self.mode {
            Mode::Actions(p, i) | Mode::Presets(p, i) => match self.entries_at(p).get(i) {
                Some(Entry::Node(n)) => Some(*n),
                _ => None,
            },
            _ => None,
        };
    }

    fn show(&mut self) {
        self.fit();
        let name = self.part().and_then(|p| p["name"].as_str()).unwrap_or_default().to_string();
        let title = match self.mode {
            Mode::List => "Insert effects".to_string(),
            Mode::Split(s) => format!("Split: {}", split_kind(self.kind_of(s))),
            Mode::Lane(s, k) => self.lane_label(s, k),
            Mode::Actions(place, i) => match self.entries_at(place).get(i) {
                Some(Entry::Node(n)) => self.name_of(*n),
                _ => String::new(),
            },
            Mode::Plugins(..) => "Insert: choose a plugin".to_string(),
            Mode::Presets(place, i) => match self.entries_at(place).get(i) {
                Some(Entry::Node(n)) => format!("{}: presets", self.name_of(*n)),
                _ => String::new(),
            },
        };
        let rows = self.rows().into_iter().map(|r| r.1).collect();
        let info = self.info();
        self.b.say(&self.message()); // an edit the engine refused, else the plugin that failed (the reference's message line)
        let noun = match self.mode {
            Mode::List => "Chain",
            Mode::Split(_) => "Split",
            Mode::Lane(..) => "Lane",
            Mode::Actions(..) => "Actions",
            Mode::Plugins(..) => "Plugins",
            Mode::Presets(..) => "Presets",
        };
        self.b.show(&name, &title, noun, rows, info);
        self.show_strip();
    }

    /// Whether an edit was refused: the shell then asks the graph again.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Add(index), Ok(r)) => {
                if let Some(node) = r["node"].as_u64() {
                    let mut params = json!({"part": self.id, "index": index, "node": node});
                    if let Place::Lane(split, lane) = self.placing {
                        params["split"] = json!(split);
                        params["lane"] = json!(lane);
                    }
                    self.out.push((Request::Insert(node), "parts.insert", params));
                }
            }
            (Request::Insert(_), Ok(_)) => self.out.push((Request::Edit, "graph.commit", json!({}))),
            (Request::Insert(node), Err(e)) => {
                // The JUCE insertPlugin takes the node out again when the insert is refused.
                self.out.push((Request::Edit, "graph.remove", json!({"node": node})));
                self.out.push((Request::Edit, "graph.commit", json!({})));
                self.say(&crate::client::refusal(&e));
                return true;
            }
            (Request::Splitter, Ok(r)) => {
                let Some(node) = r["node"].as_u64() else { return false };
                let Some(p) = self.pending.as_mut() else { return false };
                p.splitter = node;
                if matches!(p.kind, "leftRight" | "midSide") {
                    // Left / right and mid / side are joined again by a Join node.
                    match self.plugins.iter().find(|p| p.id == JOIN_PLUGIN).cloned() {
                        Some(join) => {
                            let name = self.unique_name(&join.name);
                            self.out.push((Request::Joiner, "graph.add", json!({"name": name, "path": join.path, "pluginId": join.id})));
                        }
                        None => return self.split_failed("the Split and Join plugins are not installed"),
                    }
                } else {
                    self.add_split();
                }
            }
            (Request::Joiner, Ok(r)) => {
                let Some(node) = r["node"].as_u64() else { return false };
                if let Some(p) = self.pending.as_mut() {
                    p.joiner = node;
                    self.add_split();
                }
            }
            (Request::NewSplit, Ok(_)) => {
                self.pending = None;
                self.out.push((Request::Edit, "graph.commit", json!({})));
            }
            (Request::Splitter | Request::Joiner | Request::NewSplit, Err(e)) => return self.split_failed(&e.message),
            (Request::Add(_) | Request::Edit, Err(e)) if e.code != 0 => {
                self.say(&crate::client::refusal(&e));
                return true;
            }
            (Request::List(node), Ok(r)) => self.scene_controls(node, &r),
            (Request::Window(node), Ok(r)) => {
                self.windows.insert(node, r["available"].as_bool().unwrap_or(false));
                self.show();
            }
            (Request::Presets(node), Ok(r)) if self.presets_node(node) => {
                self.presets = r["presets"].as_array().into_iter().flatten().filter_map(|p| Some((p["uri"].as_str()?.to_string(), p["name"].as_str().unwrap_or_default().to_string()))).collect();
                self.presets_error.clear();
                self.show();
            }
            // The engine never waits for the scan helper: a refusal is "ask again".
            (Request::Presets(node), Err(e)) if self.presets_node(node) => {
                self.presets.clear();
                self.presets_error = e.message;
                self.show();
            }
            (Request::Replace, Ok(_)) => {
                let name = std::mem::take(&mut self.reloading);
                self.say(&format!("Reloaded with “{name}”"));
            }
            (Request::Replace, Err(e)) => {
                self.reloading.clear();
                self.say(&crate::client::refusal(&e));
                return true;
            }
            (Request::Crossovers, Ok(r)) => {
                for v in r["values"].as_array().into_iter().flatten() {
                    if let (Some(node), Some(param), Some(value)) = (v["node"].as_u64(), v["param"].as_u64(), v["value"].as_f64()) {
                        self.crossovers.insert((node, param), value);
                    }
                }
                self.show();
            }
            _ => {}
        }
        false
    }

    /// Whether the presets shown are of this node.
    fn presets_node(&self, node: u64) -> bool {
        match self.mode {
            Mode::Presets(place, i) => self.entries_at(place).get(i) == Some(&Entry::Node(node)),
            _ => false,
        }
    }

    /// A split that could not be made: the nodes added for it come out again, and the refusal is said.
    fn split_failed(&mut self, why: &str) -> bool {
        if let Some(p) = self.pending.take() {
            for node in [p.splitter, p.joiner].into_iter().filter(|n| *n != 0) {
                self.out.push((Request::Edit, "graph.remove", json!({"node": node})));
            }
            self.out.push((Request::Edit, "graph.commit", json!({})));
        }
        self.say(&format!("The engine refused: {why}"));
        true
    }

    /// `parts.addSplit` with the nodes added for it.
    fn add_split(&mut self) {
        let Some(p) = self.pending.clone() else { return };
        let mut params = json!({"part": self.id, "index": p.index, "kind": p.kind});
        if p.splitter != 0 {
            params["splitter"] = json!(p.splitter);
        }
        if p.joiner != 0 {
            params["joiner"] = json!(p.joiner);
        }
        self.out.push((Request::NewSplit, "parts.addSplit", params));
    }

    /// A menu ready for the shell to open.
    pub fn take_menu(&mut self) -> Option<(String, Vec<Item>)> {
        self.ready_menu.take()
    }

    /// The plugin's parameters, ticked where scenes touch them (every automatable one but those set aside);
    /// choosing one sets it aside or takes it back (`sceneControlsMenu`).
    fn scene_controls(&mut self, node: u64, list: &Value) {
        let untouched: Vec<(u64, u64)> = self.graph["sceneUntouched"].as_array().into_iter().flatten().map(|u| (u["node"].as_u64().unwrap_or(0), u["param"].as_u64().unwrap_or(0))).collect();
        self.picks.clear();
        let mut items = vec![];
        for p in list["parameters"].as_array().into_iter().flatten() {
            let (id, flags) = (p["id"].as_u64().unwrap_or(0), p["flags"].as_u64().unwrap_or(0));
            let can = flags & (1 << 5) != 0 && flags & (1 << 3) == 0; // automatable, not read only
            self.picks.push((node, id));
            let mut item = Item::ticked(p["name"].as_str().unwrap_or_default(), self.picks.len() - 1, can && !untouched.contains(&(node, id)));
            item.enabled = can;
            items.push(item);
        }
        if items.is_empty() {
            items.push(Item::choice("No parameters", 0).off());
        }
        self.ready_menu = Some(("Scene controls".into(), items));
    }

    /// A choice of that menu: the parameter set aside from scenes, or taken back.
    pub fn chosen(&mut self, id: usize) {
        let Some(here) = self.picks.get(id).copied() else { return };
        let mut list: Vec<(u64, u64)> = self.graph["sceneUntouched"].as_array().into_iter().flatten().map(|u| (u["node"].as_u64().unwrap_or(0), u["param"].as_u64().unwrap_or(0))).collect();
        match list.iter().position(|x| *x == here) {
            Some(at) => {
                list.remove(at);
            }
            None => list.push(here),
        }
        let parameters: Vec<Value> = list.iter().map(|(node, param)| json!({"node": node, "param": param})).collect();
        self.say("");
        self.out.push((Request::Edit, "scenes.untouched", json!({"parameters": parameters})));
    }

    fn edit(&mut self, method: &'static str, params: Value) {
        self.say("");
        self.out.push((Request::Edit, method, params));
        self.out.push((Request::Edit, "graph.commit", json!({})));
    }

    /// A unique name for a node of `plugin`: "Basic Synth", then "Basic Synth 2" and so on.
    fn unique_name(&self, base: &str) -> String {
        let names: Vec<&str> = self.graph["nodes"].as_array().into_iter().flatten().filter_map(|n| n["name"].as_str()).collect();
        (1..)
            .map(|n| if n > 1 { format!("{base} {n}") } else { base.to_string() })
            .find(|name| !names.contains(&name.as_str()))
            .unwrap_or_default()
    }

    /// The `parts.lane` request for a lane as it is now: the whole lane, as `editLane` sends it.
    fn send_lane(&mut self, split: u64, lane: usize) {
        let Some(l) = self.lane(split, lane) else { return };
        let params = json!({
            "part": self.id, "split": split, "lane": lane,
            "zones": l["zones"].as_array().cloned().unwrap_or_default(),
            "level": l["level"].as_f64().unwrap_or(1.), "pan": l["pan"].as_f64().unwrap_or(0.),
            "mute": l["mute"].as_bool().unwrap_or(false), "solo": l["solo"].as_bool().unwrap_or(false),
        });
        self.edit("parts.lane", params);
    }

    /// The row under the cursor's value stepped `by` (the dial, or - and + by touch).
    fn dial(&mut self, row: Row, by: i64) {
        match (row, self.mode) {
            (Row::LowKey | Row::HighKey, Mode::Lane(s, k)) => {
                let (low, high) = self.lane_keys(s, k);
                let (l, h) = if row == Row::LowKey {
                    let l = (low + by).clamp(0, 127);
                    (l, high.max(l))
                } else {
                    (low, (high + by).clamp(low, 127))
                };
                if (l, h) != (low, high) {
                    self.set_lane_keys(s, k, l, h);
                }
            }
            (Row::Level | Row::Pan, Mode::Lane(s, k)) => {
                let (key, was) = if row == Row::Level { ("level", self.lane_number(s, k, "level", 1.)) } else { ("pan", self.lane_number(s, k, "pan", 0.)) };
                let now = if row == Row::Level { stepped(was, by, 0.02, 0.) } else { stepped(was, by, 0.05, -1.) };
                if now != was {
                    if let Some(lane) = self.lane_mut(s, k) {
                        lane[key] = json!(now);
                    }
                    self.send_lane(s, k);
                }
            }
            (Row::Crossover(k), Mode::Split(s)) => {
                let Some(at) = self.crossover_of(s, k) else { return };
                let was = self.crossovers.get(&at).copied().unwrap_or(CROSSOVER.2);
                let now = stepped_crossover(was, by);
                if now != was {
                    self.crossovers.insert(at, now);
                    self.say("");
                    self.out.push((Request::Edit, "params.set", json!({"values": [{"node": at.0, "param": at.1, "value": now}]})));
                }
            }
            _ => {}
        }
    }

    fn press(&mut self, at: usize) -> Outcome {
        let Some((row, _)) = self.rows().into_iter().nth(at) else { return Outcome::Stay };
        let place = self.mode.place();
        let entries = self.entries_at(place);
        match (row, self.mode) {
            (Row::Insert(i), _) => {
                self.mode = Mode::Plugins(place, i);
                self.b.reset_cursor();
            }
            (Row::Node(i), _) => {
                if let Some(Entry::Node(n)) = entries.get(i) {
                    self.out.push((Request::Window(*n), "window.info", json!({"node": n})));
                }
                self.mode = Mode::Actions(place, i);
                self.b.reset_cursor();
            }
            (Row::Split(i), _) => {
                if let Some(Entry::Split(s)) = entries.get(i) {
                    self.mode = Mode::Split(*s);
                    self.b.reset_cursor();
                    self.ask_crossovers();
                }
            }
            (Row::Lane(k), Mode::Split(s)) => {
                self.mode = Mode::Lane(s, k);
                self.b.reset_cursor();
            }
            (Row::LowKey | Row::HighKey | Row::Level | Row::Pan | Row::Crossover(_), _) => self.b.editing = !self.b.editing,
            (Row::Mute | Row::Solo, Mode::Lane(s, k)) => {
                let key = if row == Row::Mute { "mute" } else { "solo" };
                let on = !self.lane_flag(s, k, key);
                if let Some(lane) = self.lane_mut(s, k) {
                    lane[key] = json!(on);
                }
                self.send_lane(s, k);
            }
            (Row::AddLane, Mode::Split(s)) => self.edit("parts.addLane", json!({"part": self.id, "split": s})),
            (Row::RemoveSplit, Mode::Split(s)) => {
                // The first lane's plugins take the split's place; the other lanes' plugins leave the graph.
                let mut leaving = Vec::new();
                for k in 1..self.lane_count(s) {
                    if let Some(l) = self.lane(s, k) {
                        self.nodes_in(&l["chain"], 0, &mut leaving);
                    }
                }
                self.say("");
                self.out.push((Request::Edit, "parts.removeSplit", json!({"part": self.id, "split": s})));
                self.out_removed(&leaving);
                self.mode = Mode::List;
                self.b.reset_cursor();
            }
            (Row::RemoveLane, Mode::Lane(s, k)) => {
                let mut leaving = Vec::new();
                if let Some(l) = self.lane(s, k) {
                    self.nodes_in(&l["chain"], 0, &mut leaving);
                }
                self.say("");
                self.out.push((Request::Edit, "parts.removeLane", json!({"part": self.id, "split": s, "lane": k})));
                self.out_removed(&leaving);
                self.mode = Mode::Split(s);
                self.b.reset_cursor();
            }
            (Row::Parameters, Mode::Actions(p, i)) => {
                if let Some(Entry::Node(n)) = self.entries_at(p).get(i) {
                    return Outcome::Params(*n, self.name_of(*n));
                }
            }
            (Row::Power, Mode::Actions(p, i)) => {
                if let Some(Entry::Node(n)) = self.entries_at(p).get(i) {
                    let on = self.node(*n).is_some_and(|n| n["enabled"] == false); // the JUCE button: off turns it on
                    self.edit("graph.enable", json!({"node": n, "enabled": on}));
                }
            }
            (Row::Presets | Row::AskAgain, Mode::Actions(p, i) | Mode::Presets(p, i)) => {
                if let Some(Entry::Node(n)) = self.entries_at(p).get(i) {
                    self.presets.clear();
                    self.presets_error.clear();
                    self.mode = Mode::Presets(p, i);
                    self.b.reset_cursor();
                    self.out.push((Request::Presets(*n), "presets.list", json!({"node": n})));
                }
            }
            (Row::Preset(k), Mode::Presets(p, i)) => {
                // The plugin reloaded with the preset: its own file and ID, as `replacePlugin` is given them.
                if let (Some(Entry::Node(n)), Some((uri, name))) = (self.entries_at(p).get(i).copied(), self.presets.get(k).cloned()) {
                    if let Some(plugin) = self.node(n).map(|n| n["plugin"].clone()) {
                        self.reloading = name.clone();
                        self.say(&format!("Reloading with “{name}”..."));
                        self.out.push((Request::Replace, "graph.replace", json!({"node": n, "path": plugin["path"], "pluginId": plugin["id"], "preset": uri})));
                        self.mode = Mode::Actions(p, i);
                        self.b.reset_cursor();
                    }
                }
            }
            (Row::Earlier | Row::Later, Mode::Actions(p, i)) => {
                let to = if row == Row::Earlier { i - 1 } else { i + 1 };
                let mut params = json!({"part": self.id, "from": i, "to": to});
                if let Place::Lane(split, lane) = p {
                    params["split"] = json!(split);
                    params["lane"] = json!(lane);
                }
                self.edit("parts.move", params);
                self.mode = Mode::Actions(p, to);
                self.b.reset_cursor();
            }
            (Row::Routing, Mode::Actions(p, i)) => {
                if let Some(Entry::Node(n)) = self.entries_at(p).get(i) {
                    return Outcome::Routing(*n);
                }
            }
            (Row::SceneControls, Mode::Actions(p, i)) => {
                if let Some(Entry::Node(n)) = self.entries_at(p).get(i) {
                    self.out.push((Request::List(*n), "params.list", json!({"node": n})));
                }
            }
            (Row::Remove, Mode::Actions(p, i)) => {
                if let Some(Entry::Node(n)) = self.entries_at(p).get(i).copied() {
                    self.say("");
                    self.out.push((Request::Edit, "parts.removeNode", json!({"part": self.id, "node": n})));
                    self.out_removed(&[n]);
                    self.mode = match p {
                        Place::Part => Mode::List,
                        Place::Lane(s, k) => Mode::Lane(s, k),
                    };
                    self.b.reset_cursor();
                }
            }
            (Row::NewSplit(kind), Mode::Plugins(_, index)) => {
                self.say("");
                self.pending = Some(Pending { index, kind, splitter: 0, joiner: 0 });
                if kind == "copy" {
                    self.add_split();
                } else if let Some(split) = self.plugins.iter().find(|p| p.id == SPLIT_PLUGIN).cloned() {
                    let name = self.unique_name(&split.name);
                    self.out.push((Request::Splitter, "graph.add", json!({"name": name, "path": split.path, "pluginId": split.id})));
                }
                self.mode = Mode::List;
                self.b.reset_cursor();
            }
            (Row::Choose(p), Mode::Plugins(place, index)) => {
                if let Some(plugin) = self.plugins.get(p).cloned() {
                    let name = self.unique_name(&plugin.name);
                    self.say("");
                    self.placing = place;
                    self.out.push((Request::Add(index), "graph.add", json!({"name": name, "path": plugin.path, "pluginId": plugin.id})));
                    self.mode = match place {
                        Place::Part => Mode::List,
                        Place::Lane(s, k) => Mode::Lane(s, k),
                    };
                    self.b.reset_cursor();
                }
            }
            _ => return Outcome::Stay,
        }
        Outcome::Stay
    }

    /// The nodes that leave the graph with what was removed, their wires reconnected, then the commit that
    /// follows every edit.
    fn out_removed(&mut self, nodes: &[u64]) {
        for n in nodes {
            self.out.push((Request::Edit, "graph.remove", json!({"node": n, "userWires": "reconnect"})));
        }
        self.out.push((Request::Edit, "graph.commit", json!({})));
    }

    fn handle(&mut self, nav: Nav) -> Outcome {
        let outcome = match nav {
            Nav::Press(at) => self.press(at),
            Nav::Dial(at, by) => {
                if let Some((row, _)) = self.rows().into_iter().nth(at) {
                    self.dial(row, by);
                }
                Outcome::Stay
            }
            Nav::Pane(by) => {
                self.b.scroll_info(by.signum());
                Outcome::Stay
            }
            Nav::Back => match self.mode {
                Mode::List => Outcome::Back,
                _ => {
                    self.mode = match self.mode {
                        Mode::Lane(s, _) => Mode::Split(s),
                        Mode::Presets(p, i) => Mode::Actions(p, i),
                        Mode::Actions(Place::Lane(s, k), _) | Mode::Plugins(Place::Lane(s, k), _) => Mode::Lane(s, k),
                        _ => Mode::List,
                    };
                    self.b.reset_cursor();
                    self.ask_crossovers();
                    Outcome::Stay
                }
            },
            Nav::None => Outcome::Stay,
        };
        self.show();
        outcome
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let nav = self.b.nav(action);
        self.handle(nav)
    }

    pub fn tap(&mut self, row: i32) -> Outcome {
        let nav = self.b.tap(row);
        self.handle(nav)
    }

    pub fn scroll_to(&mut self, first: i32) {
        self.b.scroll_to(first.max(0) as usize);
        self.show();
    }

    pub fn scroll_info(&mut self, by: i32) {
        self.b.scroll_info(by as i64);
        self.show();
    }

    /// - or + on a row: that value stepped.
    pub fn step_row(&mut self, row: i32, by: i32) {
        let at = self.b.index(row);
        self.b.cursor = self.b.cursor.to(at);
        if let Some((r, _)) = self.rows().into_iter().nth(at) {
            self.dial(r, by as i64);
        }
        self.show();
    }

    pub fn hidden(&mut self) {
        self.b.hidden();
        self.drag = None;
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<crate::StripState>().set_shown(false);
        }
        self.show();
    }

    pub fn shown(&mut self) {
        self.b.install();
        self.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A step is a sixth of an octave, so it is finer in hertz where the frequencies are low, and a step up and a
    // step down meet again; the ends hold.
    #[test]
    fn a_crossover_steps_by_ratio() {
        let (min, max, _) = CROSSOVER;
        assert!(stepped_crossover(100., 1) - 100. < stepped_crossover(10000., 1) - 10000.);
        assert_eq!(stepped_crossover(1000., 6), 2000.);
        assert_eq!(stepped_crossover(1000., -6), 500.);
        assert_eq!(stepped_crossover(max, 1), max);
        assert_eq!(stepped_crossover(min, -1), min);
        assert_eq!(stepped_crossover(20., 1), 22., "from the least a step goes up");
        assert!((stepped_crossover(stepped_crossover(2000., 1), -1) - 2000.).abs() <= 1.);
        assert_eq!(stepped_crossover(20., -5000), min, "a long turn is held at the end");
    }

    // The chain is read as the engine sends it: a plugin is a number, a split an object; entries of another
    // shape (a missing field, the wrong type, a null) do not appear.
    #[test]
    fn a_chain_reads_its_plugins_and_splits() {
        use serde_json::json;
        assert!(Screen::entries_of(&Value::Null).is_empty(), "no chain has no rows");
        assert!(Screen::entries_of(&json!([])).is_empty(), "an empty chain has no rows");
        assert_eq!(Screen::entries_of(&json!([1, 2])), vec![Entry::Node(1), Entry::Node(2)]);
        assert_eq!(Screen::entries_of(&json!([{"split": 7}])), vec![Entry::Split(7)]);
        assert_eq!(Screen::entries_of(&json!(["a name", {}, 3, {"split": 8}, null])), vec![Entry::Node(3), Entry::Split(8)], "things that are not a plugin or a split are passed over");
    }

    // A value steps to the hundredth, held at the least and at 1, and a step down meets the step up again.
    #[test]
    fn a_value_steps_to_the_hundredth() {
        assert_eq!(stepped(0.5, 1, 0.02, 0.), 0.52);
        assert_eq!(stepped(0.5, -3, 0.02, 0.), 0.44);
        assert_eq!(stepped(0.52, -1, 0.02, 0.), 0.5, "a step down meets the step up");
        assert_eq!(stepped(0., -1, 0.05, 0.), 0., "a level holds at 0");
        assert_eq!(stepped(0.99, 5, 0.02, 0.), 1., "a level holds at 1");
        assert_eq!(stepped(-0.5, -2, 0.05, -1.), -0.6, "a pan steps below 0");
        assert_eq!(stepped(-1., -1, 0.05, -1.), -1., "a pan holds at -1");
        assert_eq!(stepped(0.123, 0, 0., 0.), 0.12, "rounded to the hundredth");
    }

    // Something the graph does not name is the layers of a copy.
    #[test]
    fn a_split_kind_has_words() {
        assert_eq!(split_kind("copy"), "layers");
        assert_eq!(split_kind(""), "layers");
        assert_eq!(split_kind("no_such_kind"), "layers");
    }
}
