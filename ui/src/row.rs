// SPDX-License-Identifier: AGPL-3.0-or-later
//! Play's knob row (hosts/standalone/PlayView.cpp): a tile for each knob under the device's screen showing
//! what that knob plays now, laid out and drawn by the grid (`grid::row_layout`, `TileRow`), banks paged by a
//! button, the Performance's controls or the selected part's own, and Learn (a control moved on the panel,
//! then the target touched). What each knob plays is resolved as the engine resolves it (knobs.rs); the part
//! and the group's page are published to the engine (`controllers.view` `part` and `positions`, as
//! `Engine::setKnobPart` and `setControlPosition` do), so the knobs play what the tiles show. The screen page
//! stays empty: with none, a device's screen knobs play Play's knob row (docs/control-protocol.md).

use crate::grid::{self, Control, Face, Param};
use crate::knobs::{self, Group, Layer, Override, Role, Slot, Source, Target};
use crate::play::Part;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// params.list of a node.
    List(u64),
    Assignments,
    Values,
    /// params.text of a node's parameter at a value (its bits).
    Text(u64, u32, u64),
    /// A change of what a knob plays; its refusal is said.
    Set,
    /// controllers.view: the part and page the knobs follow.
    View,
    /// controllers.get: where the controls' moves stand, the baseline for Learn.
    Touches,
    /// assignments.set: what Learn did.
    Assign,
}

pub type Out = (Request, &'static str, Value);

/// What the cursor can be on: the row's buttons, a knob's tile, Learn's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The knobs follow the Performance's controls.
    Performance,
    /// The knobs follow the selected part's own.
    PartControls,
    /// The next bank (or row).
    Page,
    Tile(usize),
    /// Learn: back to what the knob plays by default.
    Default,
    /// Learn: finished.
    Done,
}

/// Where the cursor went when a move left the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leave {
    Stay,
    Up,
    Down,
}

/// A tile narrower than this puts only the parameter's name, not its plugin's too (PlayView.cpp `Knob::narrow`).
const NARROW: f32 = 200.;

pub struct Row {
    /// The device's control groups (its profile's) and the one the row shows; with none, a computer's four knobs.
    groups: Vec<Group>,
    group: Option<usize>,
    fallback: Group,
    can_learn: bool,
    graph: Value,
    overrides: Vec<Override>,
    /// A node's parameters, and the revision they were read at; the revisions asked for.
    lists: HashMap<u64, (u64, Vec<Param>)>,
    asked: HashMap<u64, u64>,
    source: Source,
    selected: u32,
    part_controls: bool,
    page: usize,
    slots: Vec<Slot>,
    banks: usize,
    values: HashMap<(u64, u32), f64>,
    texts: HashMap<(u64, u32, u64), String>,
    controls: HashMap<u32, f64>,
    /// A value a finger or the dial is moving, shown over the engine's.
    local: Option<(Target, f64)>,
    pub cursor: Option<Stop>,
    /// The tile whose value the dial changes (after a press), or a finger holds.
    editing: Option<usize>,
    /// The parameter whose gesture is open.
    gesture: Option<(u64, u32)>,
    learning: bool,
    /// The control moved last, for Learn: its identity ("knob:3"), what the engine said last, and the baseline.
    learn_from: String,
    touched: String,
    touches: u64,
    seen: u64,
    baseline: bool,
    published: Option<(u32, Option<(usize, usize)>)>,
    /// The empty models every tile shares, so an unchanged tile compares equal and is not set again.
    no_choices: slint::ModelRc<slint::SharedString>,
    no_icons: slint::ModelRc<i32>,
    pub message: String,
    pub out: Vec<Out>,
}

fn tidy(v: f64) -> String {
    if v == v.trunc() { format!("{v:.0}") } else { format!("{v:.2}") }
}

fn db(level: f64) -> String {
    if level > 0. { format!("{:.1} dB", 20. * level.log10()) } else { "\u{2212}\u{221e} dB".into() }
}

impl Row {
    /// A row for a device's control groups (empty: a computer's, four knobs).
    pub fn new(groups: Vec<Group>) -> Row {
        let group = knobs::knob_group(&groups);
        // Learn is for a device with knobs to move: an assign group of them (a screen group's are never overridden).
        let can_learn = groups.iter().any(|g| g.role == Role::Assign && (g.kind == "knobs" || g.kind == "encoders"));
        Row {
            groups,
            group,
            fallback: Group { role: Role::Assign, kind: "knobs".into(), count: 4, rows: vec![], modes: vec![], functions: vec![], paged: false },
            can_learn,
            graph: Value::Null,
            overrides: vec![],
            lists: HashMap::new(),
            asked: HashMap::new(),
            source: Source::default(),
            selected: 0,
            part_controls: false,
            page: 0,
            slots: vec![],
            banks: 1,
            values: HashMap::new(),
            texts: HashMap::new(),
            controls: HashMap::new(),
            local: None,
            cursor: None,
            editing: None,
            gesture: None,
            learning: false,
            learn_from: String::new(),
            touched: String::new(),
            touches: 0,
            seen: 0,
            baseline: false,
            published: None,
            no_choices: Default::default(),
            no_icons: Default::default(),
            message: String::new(),
            out: vec![],
        }
    }

    /// The groups resolution reads and the one shown among them.
    fn shown(&self) -> (Vec<Group>, usize) {
        match self.group {
            Some(g) => (self.groups.clone(), g),
            None => (vec![self.fallback.clone()], 0),
        }
    }

    pub fn can_learn(&self) -> bool {
        self.can_learn
    }

    pub fn learning(&self) -> bool {
        self.learning
    }

    /// Learn has a control moved, and waits for what it should play.
    pub fn learning_from(&self) -> bool {
        self.learning && !self.learn_from.is_empty()
    }

    pub fn learn_from(&self) -> &str {
        &self.learn_from
    }

    /// A part chosen (opened): the knobs may follow its own controls.
    pub fn select(&mut self, part: u32) {
        if part != self.selected {
            self.selected = part;
            self.part_controls = false;
            self.page = 0;
            self.resolve();
        }
    }

    fn owner(&self) -> u32 {
        if self.part_controls && self.selected != 0 { self.selected } else { 0 }
    }

    fn rebuild(&mut self) {
        let lists = &self.lists;
        self.source = Source::from_graph(&self.graph, self.overrides.clone(), &|n| lists.get(&n).map(|(_, l)| knobs::assignable(l)));
    }

    /// graph.get: the parts, controls and scenes; each plugin's parameters read when new or changed; the
    /// overrides read again (an edit may have moved one).
    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
        if self.selected != 0 && !graph["parts"].as_array().is_some_and(|p| p.iter().any(|p| p["id"].as_u64() == Some(u64::from(self.selected)))) {
            self.selected = 0;
            self.part_controls = false;
            self.page = 0;
        }
        for c in graph["controls"].as_array().into_iter().flatten() {
            if let (Some(id), Some(v)) = (c["id"].as_u64(), c["value"].as_f64()) {
                if !matches!(self.local, Some((Target::Control(l), _)) if u64::from(l) == id) {
                    self.controls.insert(id as u32, v);
                }
            }
        }
        if self.overridable() {
            self.out.push((Request::Assignments, "assignments.get", json!({})));
        }
        self.rebuild();
        self.resolve();
    }

    /// Whether overrides play on the row's knobs: not on a screen group's (never overridden).
    fn overridable(&self) -> bool {
        self.group.is_none_or(|g| self.groups[g].role != Role::Screen)
    }

    /// The parameters of the nodes resolution reads, asked for when new or changed: those of the part whose own
    /// controls the knobs follow, and those overrides name; the Performance's controls need none. The overrides
    /// are read where they play.
    fn ask_needed(&mut self) {
        let mut nodes: Vec<u64> = Vec::new();
        let owner = self.owner();
        for p in self.graph["parts"].as_array().into_iter().flatten() {
            if p["id"].as_u64() == Some(u64::from(owner)) && owner != 0 {
                crate::play::chain_nodes(&p["chain"], &p["splits"], 0, &mut nodes);
            }
        }
        for o in &self.overrides {
            if let Target::Parameter { node, .. } = o.target {
                nodes.push(node);
            }
        }
        for id in nodes {
            let revision = self.graph["nodes"].as_array().and_then(|n| n.iter().find(|n| n["id"].as_u64() == Some(id))).and_then(|n| n["parameterRevision"].as_u64()).unwrap_or(0);
            if self.lists.get(&id).map(|l| l.0) != Some(revision) && self.asked.get(&id) != Some(&revision) {
                self.asked.insert(id, revision);
                self.out.push((Request::List(id), "params.list", json!({"node": id})));
            }
        }
    }

    /// What each knob plays at the page and part now, published to the engine when the part or the page moved,
    /// and the values of what they play asked for.
    fn resolve(&mut self) {
        self.ask_needed();
        let owner = self.owner();
        let (groups, which) = self.shown();
        self.banks = self.source.positions(&groups[which], owner).max(1);
        self.page %= self.banks;
        self.slots = self.source.resolve(&groups, which, owner, self.page);
        // The engine plays the same: the part the knobs follow, and the group's page.
        let place = self.group.map(|g| (g, self.page));
        if self.published != Some((owner, place)) && !self.graph.is_null() {
            let mut view = json!({"part": owner});
            if let Some((g, p)) = place {
                view["positions"] = json!([{"group": g, "position": p}]);
            }
            self.published = Some((owner, place));
            self.out.push((Request::View, "controllers.view", view));
        }
        let mut wanted: Vec<Value> = Vec::new();
        for s in &self.slots {
            if let Target::Parameter { node, param } = s.target {
                wanted.push(json!({"node": node, "param": param}));
            }
        }
        if !wanted.is_empty() {
            self.out.push((Request::Values, "params.get", json!({"values": wanted})));
        }
    }

    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) {
        match (request, result) {
            (Request::List(node), Ok(r)) => {
                let revision = self.asked.get(&node).copied().unwrap_or(0);
                self.lists.insert(node, (revision, grid::parse(&r)));
                self.rebuild();
                self.resolve();
            }
            (Request::Assignments, Ok(r)) => {
                let overrides = Override::parse(&r);
                if overrides != self.overrides {
                    self.overrides = overrides;
                    self.rebuild();
                    self.resolve();
                }
            }
            (Request::Values, Ok(r)) => {
                for v in r["values"].as_array().into_iter().flatten() {
                    if let (Some(n), Some(p), Some(x)) = (v["node"].as_u64(), v["param"].as_u64(), v["value"].as_f64()) {
                        self.value_arrived(n, p as u32, x);
                    }
                }
            }
            (Request::Text(n, p, bits), Ok(r)) => {
                if let Some(t) = r["text"].as_str() {
                    self.texts.insert((n, p, bits), t.to_string());
                }
            }
            (Request::Touches, Ok(r)) => {
                self.touched = r["touched"].as_str().unwrap_or_default().into();
                self.touches = r["touches"].as_u64().unwrap_or(0);
                if self.baseline {
                    self.baseline = false;
                    self.seen = self.touches;
                }
            }
            (Request::Set | Request::Assign, Err(e)) if e.code != 0 => self.message = format!("The engine refused: {}", e.message),
            (Request::Assign, Ok(_)) => self.out.push((Request::Assignments, "assignments.get", json!({}))),
            _ => {}
        }
    }

    fn value_arrived(&mut self, node: u64, param: u32, value: f64) {
        if self.gesture == Some((node, param)) {
            return;
        }
        self.values.insert((node, param), value);
        self.ask_text(node, param, value);
    }

    fn ask_text(&mut self, node: u64, param: u32, value: f64) {
        let key = (node, param, value.to_bits());
        if !self.texts.contains_key(&key) && !self.out.iter().any(|o| o.0 == Request::Text(node, param, value.to_bits())) {
            self.out.push((Request::Text(node, param, value.to_bits()), "params.text", json!({"node": node, "param": param, "value": value})));
        }
    }

    /// params.changed: the values that moved.
    pub fn changed(&mut self, params: &Value) {
        for c in params["changes"].as_array().into_iter().flatten() {
            if c["kind"] != "value" {
                continue;
            }
            if let (Some(n), Some(p), Some(v)) = (c["node"].as_u64(), c["param"].as_u64(), c["value"].as_f64()) {
                if self.slots.iter().any(|s| s.target == (Target::Parameter { node: n, param: p as u32 })) {
                    self.value_arrived(n, p as u32, v);
                }
            }
        }
    }

    /// controls.changed: the Performance's and parts' controls that moved.
    pub fn controls(&mut self, params: &Value) {
        for c in params["controls"].as_array().into_iter().flatten() {
            if let (Some(id), Some(v)) = (c["control"].as_u64(), c["value"].as_f64()) {
                if !matches!(self.local, Some((Target::Control(l), _)) if u64::from(l) == id) {
                    self.controls.insert(id as u32, v);
                }
            }
        }
    }

    /// controllers: the control moved last and how many moves so far (what Learn waits for).
    pub fn controllers(&mut self, params: &Value) {
        let (Some(touches), Some(touched)) = (params["touches"].as_u64(), params["touched"].as_str()) else { return };
        self.touches = touches;
        self.touched = touched.into();
        if self.learning && !self.baseline && touches != self.seen {
            self.seen = touches;
            self.learn_from = touched.into();
        }
    }

    // What a knob shows.

    fn param(&self, node: u64, param: u32) -> Option<&Param> {
        self.lists.get(&node).and_then(|l| l.1.iter().find(|p| p.id == param))
    }

    /// Where a target stands, 0 to 1, and its text.
    fn standing(&self, t: Target, parts: &[Part], scene: u64) -> (f64, String) {
        if let Some((_, v)) = self.local.filter(|l| l.0 == t) {
            return self.described(t, v, parts, scene);
        }
        let value = match t {
            Target::Parameter { node, param } => self.values.get(&(node, param)).copied().or_else(|| self.param(node, param).map(|p| p.default)).unwrap_or(0.),
            Target::Control(id) => self.controls.get(&id).copied().unwrap_or(0.),
            Target::PartLevel(id) => parts.iter().find(|p| p.id == id).map_or(0., |p| p.level),
            Target::PartMute(id) => parts.iter().find(|p| p.id == id).map_or(0., |p| if p.mute { 1. } else { 0. }),
            Target::Scene(id) => (u64::from(id) == scene) as u8 as f64,
            Target::None | Target::Panic => 0.,
        };
        self.described(t, value, parts, scene)
    }

    fn described(&self, t: Target, v: f64, _parts: &[Part], scene: u64) -> (f64, String) {
        match t {
            Target::Parameter { node, param } => {
                let text = self.texts.get(&(node, param, v.to_bits())).cloned().unwrap_or_else(|| if self.values.contains_key(&(node, param)) || self.local.is_some() { tidy(v) } else { String::new() });
                (self.param(node, param).map_or(v, |p| p.norm(v)), text)
            }
            Target::Control(_) => (v, format!("{:.0}%", v * 100.)),
            Target::PartLevel(_) => (v, db(v)),
            Target::PartMute(_) => (v, if v >= 0.5 { "Muted" } else { "On" }.into()),
            Target::Scene(id) => (v, if u64::from(id) == scene { "Playing" } else { "Scene" }.into()),
            Target::Panic => (0., "All notes off".into()),
            Target::None => (0., String::new()),
        }
    }

    /// The control a knob's tile has: a knob, a switch for a mute, the value alone where nothing can be turned.
    fn control_of(t: Target) -> Control {
        match t {
            Target::Parameter { .. } | Target::Control(_) | Target::PartLevel(_) => Control::Knob,
            Target::PartMute(_) => Control::Switch,
            Target::Scene(_) | Target::Panic | Target::None => Control::Value,
        }
    }

    /// The tiles, placed by `grid::row_layout` in `width` x `height` with `gap`, with each knob's name, value
    /// and where it stands.
    pub fn tiles(&self, width: f32, height: f32, gap: f32, parts: &[Part], scene: u64) -> Vec<crate::GridTile> {
        let rects = grid::row_layout(self.slots.len(), width, height, gap);
        let narrow = rects.first().is_some_and(|r| r.w < NARROW);
        let learning = self.learning_from();
        self.slots
            .iter()
            .zip(&rects)
            .enumerate()
            .map(|(i, (slot, rect))| {
                let (norm, value) = self.standing(slot.target, parts, scene);
                let label = if narrow { slot.label.rsplit(": ").next().unwrap_or(&slot.label) } else { &slot.label };
                let name = if slot.layer == Layer::Override { format!("{label} \u{b7} Override") } else { label.to_string() };
                let (name, value) = if slot.target == Target::None { ("–".to_string(), String::new()) } else { (name, value) };
                let control = Row::control_of(slot.target);
                let mut tile = grid::row_tile(*rect, &Face { control, name: &name, value: &value, norm, on: norm >= 0.5, cursor: self.cursor == Some(Stop::Tile(i)), editing: self.editing == Some(i) || (learning && slot.target != Target::None) });
                tile.choices = self.no_choices.clone();
                tile.icons = self.no_icons.clone();
                tile
            })
            .collect()
    }

    /// The bank's name above the row.
    pub fn bank(&self) -> String {
        self.slots.iter().map(|s| s.bank.as_str()).find(|b| !b.is_empty()).map(String::from).unwrap_or_else(|| "Knobs".into())
    }

    /// The row's buttons, in order: the Performance's controls and the selected part's (while a part is
    /// selected), the bank (when there are several), as their stops and labels.
    pub fn buttons(&self, parts: &[Part]) -> Vec<(Stop, String, bool)> {
        let mut out = Vec::new();
        if self.learning {
            return out; // Learn's line takes the buttons' place
        }
        if self.selected != 0 {
            let name = parts.iter().find(|p| p.id == self.selected).map(|p| p.name.clone()).unwrap_or_default();
            out.push((Stop::Performance, "Performance".to_string(), self.owner() == 0));
            out.push((Stop::PartControls, name, self.owner() != 0));
        }
        if self.banks > 1 {
            let (groups, which) = self.shown();
            let g = &groups[which];
            let text = if g.rows.is_empty() { format!("Bank {}/{}", self.page + 1, self.banks) } else { g.rows[self.page % g.rows.len()].0.clone() };
            out.push((Stop::Page, text, false));
        }
        out
    }

    /// The stops the cursor visits, in order: the buttons, the knobs that play something, Learn's two.
    pub fn stops(&self, parts: &[Part]) -> Vec<Stop> {
        let mut out: Vec<Stop> = self.buttons(parts).into_iter().map(|b| b.0).collect();
        out.extend(self.slots.iter().enumerate().filter(|(_, s)| s.target != Target::None).map(|(i, _)| Stop::Tile(i)));
        if self.learning {
            if self.learning_from() {
                out.push(Stop::Default);
            }
            out.push(Stop::Done);
        }
        out
    }

    // Learn.

    pub fn learn(&mut self, on: bool) {
        self.learning = on;
        self.learn_from.clear();
        self.cursor = None;
        self.end_edit();
        if on {
            self.baseline = true;
            self.out.push((Request::Touches, "controllers.get", json!({})));
        }
    }

    /// What Learn was waiting for is named: the control moved last plays it from now on.
    pub fn learn_target(&mut self, t: Target) -> bool {
        if !self.learning_from() || t == Target::None {
            return false;
        }
        self.assign(t);
        true
    }

    fn assign(&mut self, t: Target) {
        let mut params = t.json();
        params["control"] = json!(self.learn_from);
        self.out.push((Request::Assign, "assignments.set", params));
        self.learn(false);
    }

    // The knobs' own changes.

    fn adjustable(t: Target) -> bool {
        matches!(Row::control_of(t), Control::Knob | Control::Switch)
    }

    fn begin(&mut self, t: Target) {
        if let Target::Parameter { node, param } = t {
            if self.gesture != Some((node, param)) {
                self.end_gesture();
                self.gesture = Some((node, param));
                self.out.push((Request::Set, "params.gesture", json!({"node": node, "param": param, "begin": true})));
            }
        }
    }

    fn end_gesture(&mut self) {
        if let Some((node, param)) = self.gesture.take() {
            self.out.push((Request::Set, "params.gesture", json!({"node": node, "param": param, "begin": false})));
        }
    }

    fn end_edit(&mut self) {
        self.end_gesture();
        self.editing = None;
        self.local = None;
    }

    /// A target set to `v` (its own units): what the engine is asked for it.
    fn set(&mut self, t: Target, v: f64, parts: &[Part]) {
        self.local = Some((t, v));
        self.message.clear();
        match t {
            Target::Parameter { node, param } => {
                self.values.insert((node, param), v);
                self.out.push((Request::Set, "params.set", json!({"values": [{"node": node, "param": param, "value": v}]})));
                self.ask_text(node, param, v);
            }
            Target::Control(id) => {
                self.controls.insert(id, v);
                self.out.push((Request::Set, "controls.set", json!({"control": id, "value": v})));
            }
            Target::PartLevel(id) => {
                let mute = parts.iter().find(|p| p.id == id).is_some_and(|p| p.mute);
                self.out.push((Request::Set, "parts.level", json!({"part": id, "level": v, "mute": mute})));
                self.out.push((Request::Set, "graph.commit", json!({})));
            }
            Target::PartMute(id) => {
                let level = parts.iter().find(|p| p.id == id).map_or(1., |p| p.level);
                self.out.push((Request::Set, "parts.level", json!({"part": id, "level": level, "mute": v >= 0.5})));
                self.out.push((Request::Set, "graph.commit", json!({})));
            }
            Target::Scene(_) | Target::Panic | Target::None => {}
        }
    }

    /// The value of a target `by` steps on: a stepped parameter's step, else a hundredth of its range.
    fn stepped(&self, t: Target, now: f64, by: i64) -> f64 {
        match t {
            Target::Parameter { node, param } => {
                let Some(p) = self.param(node, param) else { return now };
                let step = if p.stepped { 1. } else { (p.max - p.min) / 100. };
                let to = (now + step * by as f64).clamp(p.min.min(p.max), p.max.max(p.min));
                if p.stepped { to.round() } else { to }
            }
            Target::PartMute(_) => if now >= 0.5 { 0. } else { 1. },
            _ => (now + 0.01 * by as f64).clamp(0., 1.),
        }
    }

    fn raw(&self, t: Target, parts: &[Part]) -> f64 {
        match t {
            Target::Parameter { node, param } => self.values.get(&(node, param)).copied().or_else(|| self.param(node, param).map(|p| p.default)).unwrap_or(0.),
            Target::Control(id) => self.controls.get(&id).copied().unwrap_or(0.),
            Target::PartLevel(id) => parts.iter().find(|p| p.id == id).map_or(0., |p| p.level),
            Target::PartMute(id) => parts.iter().find(|p| p.id == id).map_or(0., |p| if p.mute { 1. } else { 0. }),
            _ => 0.,
        }
    }

    /// A finger on tile `i`: Learn names its target; else a gesture begins.
    pub fn grab(&mut self, i: usize) -> bool {
        let Some(t) = self.slots.get(i).map(|s| s.target) else { return false };
        if self.learning_from() {
            self.learn_target(t);
            return true;
        }
        if t == Target::None || !Row::adjustable(t) {
            return false;
        }
        self.cursor = None;
        self.editing = Some(i);
        self.begin(t);
        true
    }

    /// A finger dragged tile `i`'s knob to `norm` of its travel.
    pub fn drag(&mut self, i: usize, norm: f32, parts: &[Part]) {
        let Some(t) = self.slots.get(i).map(|s| s.target) else { return };
        if self.editing != Some(i) {
            return;
        }
        let to = match t {
            Target::Parameter { node, param } => match self.param(node, param) {
                Some(p) => {
                    let v = p.denorm(f64::from(norm));
                    if p.stepped { v.round() } else { v }
                }
                None => return,
            },
            Target::Control(_) | Target::PartLevel(_) => (f64::from(norm) * 100.).round() / 100.,
            _ => return,
        };
        if self.local.map(|l| l.1) != Some(to) {
            self.set(t, to, parts);
        }
    }

    pub fn release(&mut self) {
        self.end_edit();
    }

    /// A tap on a switch tile (a mute): Learn names it; else it flips.
    pub fn tap(&mut self, i: usize, parts: &[Part]) {
        let Some(t) = self.slots.get(i).map(|s| s.target) else { return };
        if self.learn_target(t) {
            return;
        }
        if matches!(t, Target::PartMute(_)) {
            let to = self.stepped(t, self.raw(t, parts), 1);
            self.set(t, to, parts);
            self.local = None;
        }
    }

    /// A tap on a button (by its place among `buttons`), or on Learn's.
    pub fn press_stop(&mut self, stop: Stop, parts: &[Part], scene: u64) {
        let _ = scene;
        match stop {
            Stop::Performance | Stop::PartControls => {
                self.part_controls = stop == Stop::PartControls;
                self.page = 0;
                self.resolve();
            }
            Stop::Page => {
                self.page += 1;
                self.resolve();
            }
            Stop::Tile(i) => {
                let Some(t) = self.slots.get(i).map(|s| s.target) else { return };
                if self.learn_target(t) {
                    return;
                }
                match Row::control_of(t) {
                    Control::Switch => self.tap(i, parts),
                    Control::Knob => {
                        if self.editing == Some(i) {
                            self.end_edit();
                        } else {
                            self.end_edit();
                            self.editing = Some(i);
                            self.begin(t);
                        }
                    }
                    _ => {}
                }
            }
            Stop::Default => {
                if self.learning_from() {
                    let mut params = Target::None.json();
                    params["control"] = json!(self.learn_from);
                    self.out.push((Request::Assign, "assignments.set", params));
                    self.learn(false);
                }
            }
            Stop::Done => self.learn(false),
        }
    }

    // The cursor.

    /// The dial: `by` steps along the stops, or on a value being changed.
    pub fn move_by(&mut self, by: i64, parts: &[Part]) -> Leave {
        if let (Some(i), Some(_)) = (self.editing, self.cursor) {
            if let Some(t) = self.slots.get(i).map(|s| s.target) {
                let now = self.local.filter(|l| l.0 == t).map_or_else(|| self.raw(t, parts), |l| l.1);
                let to = self.stepped(t, now, by.signum());
                if to != now {
                    self.set(t, to, parts);
                }
            }
            return Leave::Stay;
        }
        let stops = self.stops(parts);
        let at = self.cursor.and_then(|c| stops.iter().position(|s| *s == c));
        let next = match at {
            None => Some(if by < 0 { stops.len().saturating_sub(1) } else { 0 }),
            Some(a) => {
                let n = a as i64 + by;
                if n < 0 {
                    return Leave::Up;
                } else if n >= stops.len() as i64 {
                    return Leave::Down;
                }
                Some(n as usize)
            }
        };
        self.cursor = next.and_then(|n| stops.get(n).copied());
        Leave::Stay
    }

    /// The cursor onto the first stop (coming from above) or the last (from below).
    pub fn enter(&mut self, first: bool, parts: &[Part]) -> bool {
        let stops = self.stops(parts);
        self.cursor = if first { stops.first().copied() } else { stops.last().copied() };
        self.cursor.is_some()
    }

    /// Left and right along the stops.
    pub fn arrow(&mut self, dx: i32, parts: &[Part]) {
        self.move_by(i64::from(dx), parts);
    }

    /// A press on the cursor's stop.
    pub fn confirm(&mut self, parts: &[Part], scene: u64) {
        if let Some(c) = self.cursor {
            self.press_stop(c, parts, scene);
        }
    }

    /// Cancel: ends a value being changed, else Learn's wait for a target, else drops the cursor. False when
    /// there was nothing to give up.
    pub fn cancel(&mut self) -> bool {
        if self.editing.is_some() {
            self.end_edit();
            return true;
        }
        if self.cursor.take().is_some() {
            return true;
        }
        if self.learning {
            self.learn(false);
            return true;
        }
        false
    }

    pub fn has_cursor(&self) -> bool {
        self.cursor.is_some()
    }

    pub fn hidden(&mut self) {
        self.cursor = None;
        self.end_edit();
    }
}
