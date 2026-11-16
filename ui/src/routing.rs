// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Routing page: the routing page of the JUCE Edit view (hosts/standalone/PartsView.cpp,
//! `routingShown`), on the shell. The real graph as boxes in columns by signal flow (wires.rs lays it
//! out, the same points are what is drawn), fitted to the space above a panel for what is chosen: a
//! plugin's or a part's words, what it connects to, each source's level into it. Parts are boxes with
//! their plugins as chips (collapsed) or frames holding them (expanded); wires are drawn in the theme's
//! wire colors (audio, notes, disabled, draining, unconnected), an audio wire as a line per channel, with
//! Meters on in the color of the level it carries; a plugin's lane to a control or a parameter is a line
//! in the control color.
//!
//! Without touch the cursor is a focus border: the left knob moves over the boxes (the frames, parts,
//! plugins and ends, then the controls, in columns from the left), the right knob over the panel's cells,
//! the dial over the buttons under the header, then the boxes, then the panel (or the pane last touched
//! by a knob); a press on a box chooses it (again: lets go of it), on a panel cell does what it says: a
//! button, a toggle, a slider or the wire channel, which the dial then changes (a press or Cancel ends
//! it). The connections are menus (menu.rs). Cancel ends a change, drops the cursor, then goes back.
//!
//! Requests: `graph.get` (asked by the shell), `wires.meters` every 50 ms with Meters on, `params.list`
//! (the parameters a lane can drive, and the names a lane row says), `window.info` (a plugin's own view
//! is not in this shell yet), and the edits, each followed by `graph.commit` as the JUCE setters commit:
//! `graph.connect`, `wires.disconnect`, `wires.mix`, `wires.enable`, `wires.zones`, `graph.enable`,
//! `graph.add`, `graph.remove`, `graph.map`'s undoing `graph.unmap`, `controls.add`, `controls.source`,
//! `controls.targets` and `controls.set` (a lane to a control or a parameter).

use crate::controls::{Kind as TargetKind, Target};
use crate::menu::Item;
use crate::nav::Action;
use crate::scan::Plugin;
use crate::setlist::update;
use crate::wires::{self, BoxKind, Graph, Layout, Metrics, Pt, WireState, GRAPH_INPUT, GRAPH_OUTPUT, NOTES_PORT};
use crate::{PanelCellData, PanelRowData, RoutingBox, RoutingLink, RoutingState, RoutingWire, Shell, Theme};
use serde_json::{json, Value};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

/// How often the wires' peaks are asked with Meters on (the JUCE view's timer, 50 ms).
pub const METER_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Any edit: its refusal is said.
    Edit,
    Meters,
    /// session.status, asked as the page opens: the plugins that failed.
    Status,
    /// params.list of a node.
    List(u64),
    /// window.info of a node.
    Window(u64),
    /// graph.add of a plugin outside the parts: its node comes back, and is committed.
    Add,
    /// controls.add for a lane that has no control yet: its number comes back.
    LaneAdd(u64),
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Back to the Parts list.
    Back,
    /// A part's Part screen.
    OpenPart(u32),
    /// The playing keyboard for this input (empty: the built-in keys).
    Keys(String),
    /// The Controls page (a control's pill was chosen).
    Controls,
}

/// What a button, toggle or slider of the panel does.
#[derive(Clone, Debug, PartialEq)]
enum Act {
    None,
    OpenPart(u64),
    ToggleExpand(u64),
    ConnectAudio,
    ConnectNotes,
    ConnectLane,
    Keys,
    Power(u64, bool),
    Remove(u64),
    DisconnectLane(u64, u64),
    WireOn(u64),
    WireLevel(u64),
    WireMute(u64),
    WireSolo(u64),
    WireChannel(u64),
    Disconnect(u64),
}

#[derive(Clone, Debug, PartialEq)]
struct PCell {
    act: Act,
    kind: i32,
    label: String,
    on: bool,
    value: f32,
    width: f32,
    enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct PRow {
    kind: i32,
    text: String,
    cells: Vec<PCell>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pick {
    /// A box of the cursor's order: frame, part or plugin, or a control.
    Frame(usize),
    Box(usize),
    Control(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    /// The nth of the order.
    Canvas(usize),
    /// A cell of a row of the whole panel.
    Panel(usize, usize),
}

/// What a choice of the menu that is open means.
#[derive(Clone, Debug, PartialEq)]
enum Choice {
    Connect(u64, u64, bool),
    Plugin(usize),
    LaneControl(u64, u64, u64),
    LaneParam(u64, u64, u64, u64),
}

/// A lane's wiring waiting for the control it made.
#[derive(Clone, Debug, PartialEq)]
enum LaneOp {
    Control { node: u64, lane: u64 },
    Param { node: u64, lane: u64, to: u64, param: u64 },
}

/// One parameter of a plugin as `params.list` gives it: what a lane's menu and rows say.
#[derive(Clone, Debug, PartialEq)]
struct Param {
    id: u64,
    name: String,
    flags: u64,
}

const READONLY: u64 = 1 << 3;
const HIDDEN: u64 = 1 << 2;

/// The shown form of a box, in plain types, so a row of the window is set only when it changed.
#[derive(Clone, Debug, PartialEq, Default)]
struct PBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    name: String,
    detail: String,
    chips: Vec<String>,
    bright: bool,
    chosen: bool,
    off: bool,
    failed: bool,
    badge: bool,
    tint: i32,
    cursor: bool,
    part: bool,
    chevron: i32,
}

impl PBox {
    fn slint(&self) -> RoutingBox {
        RoutingBox {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            name: self.name.as_str().into(),
            detail: self.detail.as_str().into(),
            chips: ModelRc::new(VecModel::from(self.chips.iter().map(|c| SharedString::from(c.as_str())).collect::<Vec<_>>())),
            bright: self.bright,
            chosen: self.chosen,
            off: self.off,
            failed: self.failed,
            badge: self.badge,
            tint: self.tint,
            cursor: self.cursor,
            part: self.part,
            chevron: self.chevron,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PWire {
    commands: String,
    color: Color,
    width: f32,
    alpha: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct PLink {
    commands: String,
    width: f32,
    alpha: f32,
    dot: (f32, f32),
}

/// Changes `model` to `now`, a row set only where it differs from `last`.
fn sync<P: PartialEq + Clone, S: Clone + 'static>(model: &VecModel<S>, last: &mut Vec<P>, now: Vec<P>, make: impl Fn(&P) -> S) {
    for (i, p) in now.iter().enumerate() {
        if i >= model.row_count() {
            model.push(make(p));
        } else if last.get(i) != Some(p) {
            model.set_row_data(i, make(p));
        }
    }
    while model.row_count() > now.len() {
        model.remove(model.row_count() - 1);
    }
    *last = now;
}

/// Where a view `room` long in a canvas scrolled to `pan` has to move to hold the stretch `at` to `at + len` with
/// `margin` to spare: where it is when it holds it, else the least move (to the stretch's start when it is longer).
fn follow(pan: f64, at: f64, len: f64, room: f64, margin: f64) -> f64 {
    if len + 2. * margin >= room || at - margin < pan {
        at - margin
    } else if at + len + margin > pan + room {
        at + len + margin - room
    } else {
        pan
    }
}

/// A wire's id, where it leaves and where it enters in the canvas's pixels, and how many lines it is.
#[cfg(test)]
pub type WireEnds = (u64, (f64, f64), (f64, f64), usize);

fn two(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn num(x: f64, y: f64) -> String {
    format!("{} {}", two(x), two(y))
}

/// A wire's lines as path commands in the canvas's pixels: one cubic per pair of points with its control
/// points at their middle in x (the JUCE drawing), once for a single line; for a bus a line per channel
/// side by side, each the same curve moved along its normals, and dashed when `dash` says so.
fn wire_commands(points: &[(f64, f64)], lines: usize, pitch: f64, dash: Option<(f64, f64)>) -> String {
    if points.len() < 2 {
        return String::new();
    }
    if lines <= 1 && dash.is_none() {
        let mut s = format!("M {}", num(points[0].0, points[0].1));
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let mid = (a.0 + b.0) / 2.;
            s += &format!(" C {} {} {}", num(mid, a.1), num(mid, b.1), num(b.0, b.1));
        }
        return s;
    }
    // Sampled: the same curve, a point every 1/24th of each segment.
    let mut curve: Vec<(f64, f64)> = vec![];
    for (i, w) in points.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let mid = (a.0 + b.0) / 2.;
        for s in if i == 0 { 0 } else { 1 }..=24 {
            let (t, u) = (f64::from(s) / 24., 1. - f64::from(s) / 24.);
            curve.push((u * u * u * a.0 + 3. * u * u * t * mid + 3. * u * t * t * mid + t * t * t * b.0, u * u * u * a.1 + 3. * u * u * t * a.1 + 3. * u * t * t * b.1 + t * t * t * b.1));
        }
    }
    let mut out = String::new();
    for k in 0..lines.max(1) {
        let offset = (k as f64 - (lines.max(1) as f64 - 1.) / 2.) * pitch;
        let moved: Vec<(f64, f64)> = (0..curve.len())
            .map(|i| {
                let (p, q) = (curve[i.saturating_sub(1)], curve[(i + 1).min(curve.len() - 1)]);
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                let len = dx.hypot(dy).max(1e-9);
                (curve[i].0 - dy / len * offset, curve[i].1 + dx / len * offset)
            })
            .collect();
        match dash {
            None => {
                out += &format!("M {}", num(moved[0].0, moved[0].1));
                for p in &moved[1..] {
                    out += &format!(" L {}", num(p.0, p.1));
                }
            }
            Some((on, off)) => {
                // Along the curve: `on` drawn, `off` skipped, by length.
                let (mut drawing, mut left) = (true, on);
                out += &format!("M {}", num(moved[0].0, moved[0].1));
                for w in moved.windows(2) {
                    let (mut p, q) = (w[0], w[1]);
                    let mut length = (q.0 - p.0).hypot(q.1 - p.1);
                    while length > 0. {
                        let step = left.min(length);
                        let at = if length > 0. { step / length } else { 1. };
                        let next = (p.0 + (q.0 - p.0) * at, p.1 + (q.1 - p.1) * at);
                        if drawing {
                            out += &format!(" L {}", num(next.0, next.1));
                        }
                        length -= step;
                        left -= step;
                        p = next;
                        if left <= 1e-9 {
                            drawing = !drawing;
                            left = if drawing { on } else { off };
                            if drawing {
                                out += &format!(" M {}", num(p.0, p.1));
                            }
                        }
                        if length <= 1e-9 {
                            break;
                        }
                    }
                }
            }
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

/// A control line's corners rounded by `radius`: straight runs, each turn a quadratic round the corner.
fn rounded(points: &[(f64, f64)], radius: f64) -> String {
    let Some(first) = points.first() else { return String::new() };
    let mut s = format!("M {}", num(first.0, first.1));
    for i in 1..points.len() {
        let c = points[i];
        if i + 1 == points.len() {
            s += &format!(" L {}", num(c.0, c.1));
            break;
        }
        let (p, n) = (points[i - 1], points[i + 1]);
        let (d1, d2) = ((p.0 - c.0).hypot(p.1 - c.1), (n.0 - c.0).hypot(n.1 - c.1));
        let (r1, r2) = (radius.min(d1 / 2.), radius.min(d2 / 2.));
        let a = (c.0 + (p.0 - c.0) / d1.max(1e-9) * r1, c.1 + (p.1 - c.1) / d1.max(1e-9) * r1);
        let b = (c.0 + (n.0 - c.0) / d2.max(1e-9) * r2, c.1 + (n.1 - c.1) / d2.max(1e-9) * r2);
        s += &format!(" L {} Q {} {}", num(a.0, a.1), num(c.0, c.1), num(b.0, b.1));
    }
    s
}

/// The MIDI channel the zones take: 1 to 16, 0 any, -1 more than one (the JUCE `zoneChannel`).
pub fn zone_channel(zones: &[Value]) -> i64 {
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

/// Zones with every channel mask set to `channel` (0 any), identical ones one, and a zone left passing
/// everything gone (the JUCE `zoneChannels`).
pub fn set_zone_channel(zones: &[Value], channel: i64) -> Vec<Value> {
    let mask = if channel == 0 { 65535 } else { 1u64 << (channel - 1) };
    let default = json!({"keys": [0, 127], "velocities": [0, 1], "channels": 65535, "transpose": 0, "curve": 1, "fixedVelocity": 0});
    let mut zones = zones.to_vec();
    if zones.is_empty() && channel != 0 {
        zones.push(default.clone());
    }
    for z in &mut zones {
        z["channels"] = json!(mask);
    }
    let mut kept: Vec<Value> = vec![];
    for z in zones {
        if !kept.contains(&z) {
            kept.push(z);
        }
    }
    if channel == 0 && kept.len() == 1 && kept[0] == default {
        kept.clear();
    }
    kept
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    json: Value,
    g: Graph,
    /// Parts shown as boxes (the user's choice, kept in the JUCE app's routing-view.txt), except those in `expanded`.
    collapsed: bool,
    expanded: BTreeSet<u64>,
    meters_on: bool,
    peaks: HashMap<u64, f64>,
    /// What is chosen: a plugin or an end (its node and port), or a part.
    routed: Option<(u64, u64)>,
    routed_part: u64,
    layout: Layout,
    /// The layout as it was drawn: its scale and where it starts in the canvas (centered when it is smaller).
    scale: f64,
    origin: (f64, f64),
    /// The place of the order the view last followed: the view moves to a place the cursor reaches, once.
    followed: Option<usize>,
    /// Where the cursor can go on the canvas, in order, and what each place is.
    order: Vec<Pick>,
    stop: Option<Stop>,
    /// The pane the dial moves: 0 the canvas, 1 the panel.
    touched: u8,
    editing: bool,
    panel_first: usize,
    said: String,
    failures: Vec<(u64, String, String)>,
    windows: HashMap<u64, bool>,
    params: HashMap<u64, Vec<Param>>,
    asked: Vec<u64>,
    plugins: Vec<Plugin>,
    scanning: bool,
    keys_offered: bool,
    choices: Vec<Choice>,
    ready_menu: Option<(String, Vec<Item>)>,
    /// The lane menu waits for the parameters of every plugin it lists.
    waiting_lane: Option<u64>,
    /// A finger on the slider of this wire's level.
    held: Option<u64>,
    frames: Rc<VecModel<RoutingBox>>,
    boxes: Rc<VecModel<RoutingBox>>,
    controls: Rc<VecModel<RoutingBox>>,
    wires: Rc<VecModel<RoutingWire>>,
    links: Rc<VecModel<RoutingLink>>,
    panel: Rc<VecModel<PanelRowData>>,
    last_frames: Vec<PBox>,
    last_boxes: Vec<PBox>,
    last_controls: Vec<PBox>,
    last_wires: Vec<PWire>,
    last_links: Vec<PLink>,
    last_panel: Vec<(PRow, Option<usize>)>,
    last_editing: bool,
    /// The layout is laid out again: the graph, or what is expanded, changed.
    dirty: bool,
    frame_ids: Vec<usize>,
    box_ids: Vec<usize>,
    /// Lane wirings waiting for the control that was made for them.
    pending: HashMap<u64, LaneOp>,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(window: &Shell) -> Screen {
        let s = Screen {
            ui: window.as_weak(),
            json: Value::Null,
            g: Graph::default(),
            collapsed: true,
            expanded: BTreeSet::new(),
            meters_on: false,
            peaks: HashMap::new(),
            routed: None,
            routed_part: 0,
            layout: Layout::default(),
            scale: 1.,
            origin: (0., 0.),
            followed: None,
            order: vec![],
            stop: None,
            touched: 0,
            editing: false,
            panel_first: 0,
            said: String::new(),
            failures: vec![],
            windows: HashMap::new(),
            params: HashMap::new(),
            asked: vec![],
            plugins: vec![],
            scanning: true,
            keys_offered: false,
            choices: vec![],
            ready_menu: None,
            waiting_lane: None,
            held: None,
            frames: Rc::new(VecModel::default()),
            boxes: Rc::new(VecModel::default()),
            controls: Rc::new(VecModel::default()),
            wires: Rc::new(VecModel::default()),
            links: Rc::new(VecModel::default()),
            panel: Rc::new(VecModel::default()),
            last_frames: vec![],
            last_boxes: vec![],
            last_controls: vec![],
            last_wires: vec![],
            last_links: vec![],
            last_panel: vec![],
            last_editing: false,
            dirty: true,
            frame_ids: vec![],
            box_ids: vec![],
            pending: HashMap::new(),
            out: vec![],
        };
        let state = window.global::<RoutingState>();
        state.set_frames(ModelRc::from(s.frames.clone()));
        state.set_boxes(ModelRc::from(s.boxes.clone()));
        state.set_controls(ModelRc::from(s.controls.clone()));
        state.set_wires(ModelRc::from(s.wires.clone()));
        state.set_links(ModelRc::from(s.links.clone()));
        state.set_panel(ModelRc::from(s.panel.clone()));
        s
    }

    /// Whether parts are shown collapsed: the user's choice, read when the shell starts.
    pub fn set_collapsed(&mut self, collapsed: bool) {
        self.collapsed = collapsed;
        self.expanded.clear();
        self.dirty = true;
    }

    /// The profile offers the on-screen keyboard: the keys and MIDI's panel has a button for it.
    pub fn offer_keyboard(&mut self, offered: bool) {
        self.keys_offered = offered;
    }

    /// The plugins there are to add outside the parts: the scanner's answer.
    pub fn plugins(&mut self, found: Vec<Plugin>) {
        self.plugins = crate::scan::choices(found);
        self.scanning = false;
    }

    /// Whether Meters is on, for the shell's timer.
    pub fn metering(&self) -> bool {
        self.meters_on
    }

    /// A shared node (the Parts list's row) opens here with that plugin chosen.
    pub fn choose_node(&mut self, node: u64) {
        self.routed = Some((node, 0));
        self.routed_part = 0;
        self.stop = None;
    }

    /// Opened from the Parts list's button: nothing chosen.
    pub fn opened(&mut self) {
        self.routed = None;
        self.routed_part = 0;
        self.stop = None;
        self.editing = false;
        self.panel_first = 0;
    }

    /// graph.get: the graph again, what is chosen kept while it is still there.
    pub fn graph(&mut self, graph: &Value) {
        self.json = graph.clone();
        self.g = Graph::read(graph);
        self.dirty = true;
        if let Some((node, port)) = self.routed {
            let there = if node == GRAPH_INPUT || node == GRAPH_OUTPUT { wires::read_routing(&self.g).find(node, port).is_some() } else { self.g.node(node).is_some() };
            if !there {
                self.routed = None;
            }
        }
        if self.routed_part != 0 && !self.g.parts.iter().any(|p| p.id == self.routed_part) {
            self.routed_part = 0;
        }
        self.expanded.retain(|p| self.g.parts.iter().any(|x| x.id == *p));
        let ids: Vec<u64> = self.g.nodes.iter().map(|n| n.id).collect();
        self.params.retain(|n, _| ids.contains(n));
        self.ask_lane_names();
        self.show();
    }

    /// session.status: the plugin that failed, outlined in the danger color.
    pub fn status(&mut self, status: &Value) {
        if let Some(list) = status["failures"].as_array() {
            let failures: Vec<(u64, String, String)> = list.iter().map(|f| (f["node"].as_u64().unwrap_or(0), f["name"].as_str().unwrap_or_default().into(), f["reason"].as_str().unwrap_or_default().into())).collect();
            if failures != self.failures {
                self.failures = failures;
                self.show();
            }
        }
    }

    /// wires.meters: each audio wire's recent peak.
    pub fn meters(&mut self, result: &Value) {
        self.peaks.clear();
        for m in result["meters"].as_array().into_iter().flatten() {
            if let (Some(w), Some(p)) = (m["wire"].as_u64(), m["peak"].as_f64()) {
                self.peaks.insert(w, p);
            }
        }
        self.show();
    }

    /// The nodes whose parameter names a lane's row says, asked once each.
    fn ask_lane_names(&mut self) {
        let Some((node, _)) = self.routed.filter(|(n, _)| *n != GRAPH_INPUT && *n != GRAPH_OUTPUT) else { return };
        let targets: Vec<u64> = self.g.mappings.iter().filter(|m| m.source == node).map(|m| m.target).collect();
        for t in targets {
            self.ask(t);
        }
        if self.g.node(node).is_some_and(|n| !self.windows.contains_key(&n.id)) {
            self.windows.insert(node, false);
            self.out.push((Request::Window(node), "window.info", json!({"node": node})));
        }
    }

    fn ask(&mut self, node: u64) {
        if self.params.contains_key(&node) || self.asked.contains(&node) || self.g.node(node).is_none() {
            return;
        }
        self.asked.push(node);
        self.out.push((Request::List(node), "params.list", json!({"node": node})));
    }

    /// Whether an edit was refused: the shell then asks the graph again.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Meters, Ok(r)) => self.meters(&r),
            (Request::Status, Ok(r)) => self.status(&r),
            (Request::List(node), r) => {
                self.asked.retain(|n| *n != node);
                let list = r.ok().map(|r| r["parameters"].as_array().into_iter().flatten().map(|p| Param { id: p["id"].as_u64().unwrap_or(0), name: p["name"].as_str().unwrap_or_default().into(), flags: p["flags"].as_u64().unwrap_or(0) }).collect()).unwrap_or_default();
                self.params.insert(node, list);
                if let Some(lane) = self.waiting_lane.filter(|_| self.asked.is_empty()) {
                    self.waiting_lane = None;
                    self.said.clear();
                    self.ready_menu = self.lane_menu(lane);
                }
                self.show();
            }
            (Request::Window(node), Ok(r)) => {
                self.windows.insert(node, r["available"].as_bool().unwrap_or(false));
                self.show();
            }
            (Request::Add, Ok(r)) if r["node"].as_u64().is_some() => self.out.push((Request::Edit, "graph.commit", json!({}))),
            (Request::LaneAdd(op), Ok(r)) => {
                if let Some(control) = r["control"].as_u64() {
                    self.lane_made(op, control);
                }
            }
            (Request::Edit | Request::Add | Request::LaneAdd(_), Err(e)) if e.code != 0 => {
                self.said = format!("The engine refused: {}", e.message);
                self.show();
                return true;
            }
            _ => {}
        }
        false
    }

    /// An edit, committed as the JUCE setters commit.
    fn edit(&mut self, method: &'static str, params: Value) {
        self.said.clear();
        self.out.push((Request::Edit, method, params));
        self.out.push((Request::Edit, "graph.commit", json!({})));
    }

    /// A menu ready for the shell to open.
    pub fn take_menu(&mut self) -> Option<(String, Vec<Item>)> {
        self.ready_menu.take()
    }

    // The drawing.

    fn routing_nodes(&self) -> wires::Routing {
        wires::read_routing(&self.g)
    }

    fn colors(&self) -> [Color; 9] {
        let Some(ui) = self.ui.upgrade() else { return [Color::default(); 9] };
        let t = ui.global::<Theme>();
        [t.get_wire_audio(), t.get_wire_notes(), t.get_wire_disabled(), t.get_wire_draining(), t.get_wire_unconnected(), t.get_wire_control(), t.get_positive(), t.get_warning(), t.get_danger()]
    }

    fn part_of_node(&self, node: u64) -> u64 {
        self.g.owner(node).map_or(0, |p| p.id)
    }

    /// Whether wire `w` touches what is chosen.
    fn touches(&self, w: &wires::Wire) -> bool {
        if self.routed_part != 0 {
            return self.part_of_node(w.from) == self.routed_part || self.part_of_node(w.to) == self.routed_part;
        }
        match self.routed {
            Some((GRAPH_INPUT, port)) => w.from == GRAPH_INPUT && if w.notes { port == NOTES_PORT } else { w.from_port == port },
            Some((GRAPH_OUTPUT, port)) => w.to == GRAPH_OUTPUT && w.to_port == port,
            Some((node, _)) => w.from == node || w.to == node,
            None => true,
        }
    }

    fn focus(&self) -> bool {
        self.routed.is_some() || self.routed_part != 0
    }

    fn canvas(&self) -> (f64, f64) {
        self.ui.upgrade().map_or((0., 0.), |ui| {
            let s = ui.global::<RoutingState>();
            (f64::from(s.get_canvas_width()), f64::from(s.get_canvas_height()))
        })
    }

    /// The cursor's places on the canvas: the frames, parts, plugins and ends, then the controls, in columns
    /// from the left, top to bottom.
    fn make_order(&self) -> Vec<Pick> {
        let mut places: Vec<((i32, i32, i32), Pick)> = vec![];
        for (i, b) in self.layout.boxes.iter().enumerate() {
            places.push(((b.bounds.x, b.bounds.y, i32::from(b.kind != BoxKind::Frame)), if b.kind == BoxKind::Frame { Pick::Frame(i) } else { Pick::Box(i) }));
        }
        for (i, c) in self.layout.controls.iter().enumerate() {
            places.push(((c.bounds.x, c.bounds.y, 2), Pick::Control(i)));
        }
        places.sort_by_key(|p| p.0);
        places.into_iter().map(|p| p.1).collect()
    }

    fn px(&self, p: Pt) -> (f64, f64) {
        (self.origin.0 + p.x * self.scale, self.origin.1 + p.y * self.scale)
    }

    fn rect(&self, r: wires::Rect) -> (f32, f32, f32, f32) {
        let (x, y) = self.px(Pt { x: f64::from(r.x), y: f64::from(r.y) });
        (x as f32, y as f32, (f64::from(r.w) * self.scale) as f32, (f64::from(r.h) * self.scale) as f32)
    }

    /// The window's view: the layout fitted to the canvas, the boxes, wires and lines, the panel and the
    /// buttons, each row set only when it changed.
    pub fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let (cw, ch) = self.canvas();
        if self.dirty {
            self.layout = wires::layout_routing(&self.g, self.collapsed, &self.expanded, &Metrics::default());
            self.dirty = false;
        }
        // The drawing is not shrunk to fit: its type is the theme's small size at the least (the reference scales it
        // down with no floor, to a few pixels). What does not fit the canvas is scrolled: a drag, or the cursor
        // reaching a place that is out of view.
        let (lw, lh) = (f64::from(self.layout.width.max(1)), f64::from(self.layout.height.max(1)));
        self.scale = f64::from(ui.global::<Theme>().get_font_scale()).max(1.);
        self.origin = (((cw - lw * self.scale) / 2.).max(0.), ((ch - lh * self.scale) / 2.).max(0.));
        let (content_w, content_h) = (cw.max(lw * self.scale), ch.max(lh * self.scale));
        self.order = self.make_order();
        if matches!(self.stop, Some(Stop::Canvas(i)) if i >= self.order.len()) {
            self.stop = None;
            self.editing = false;
        }
        let view = ui.global::<RoutingState>();
        let mut pan = (f64::from(view.get_pan_x()).clamp(0., content_w - cw), f64::from(view.get_pan_y()).clamp(0., content_h - ch));
        match self.stop {
            Some(Stop::Canvas(i)) if self.followed != Some(i) => {
                self.followed = Some(i);
                if let Some(at) = self.order.get(i).copied() {
                    let bounds = match at {
                        Pick::Frame(b) | Pick::Box(b) => self.layout.boxes[b].bounds,
                        Pick::Control(c) => self.layout.controls[c].bounds,
                    };
                    let (x, y, w, h) = self.rect(bounds);
                    let room = 8. * self.scale;
                    pan = (follow(pan.0, f64::from(x), f64::from(w), cw, room), follow(pan.1, f64::from(y), f64::from(h), ch, room));
                }
            }
            Some(Stop::Canvas(_)) => {}
            _ => self.followed = None,
        }
        let pan = (pan.0.clamp(0., content_w - cw), pan.1.clamp(0., content_h - ch));
        view.set_content_width(content_w as f32);
        view.set_content_height(content_h as f32);
        if (f64::from(view.get_pan_x()) - pan.0).abs() > 0.01 {
            view.set_pan_x(pan.0 as f32);
        }
        if (f64::from(view.get_pan_y()) - pan.1).abs() > 0.01 {
            view.set_pan_y(pan.1 as f32);
        }
        let colors = self.colors();
        let routing = self.routing_nodes();
        // What stays bright: what is chosen and what its wires reach.
        let focus = self.focus();
        let mut reached: HashSet<u64> = HashSet::new();
        let mut reached_parts: HashSet<u64> = HashSet::new();
        for p in &self.layout.paths {
            let w = &self.g.wires[p.wire];
            if self.touches(w) {
                for n in [w.from, w.to] {
                    reached.insert(n);
                    let part = self.part_of_node(n);
                    if part != 0 {
                        reached_parts.insert(part);
                    }
                }
            }
        }
        let cursor = match self.stop {
            Some(Stop::Canvas(i)) => self.order.get(i).copied(),
            _ => None,
        };
        let failed = |node: u64| self.failures.iter().any(|f| f.0 == node && node != 0);
        let (mut frames, mut boxes, mut controls) = (vec![], vec![], vec![]);
        self.frame_ids.clear();
        self.box_ids.clear();
        for (i, b) in self.layout.boxes.iter().enumerate() {
            let (x, y, w, h) = self.rect(b.bounds);
            let bright = !focus
                || if b.kind == BoxKind::Node {
                    if b.node == GRAPH_INPUT || b.node == GRAPH_OUTPUT { reached.contains(&b.node) } else { reached.contains(&b.node) || (self.routed_part != 0 && b.part == self.routed_part) }
                } else {
                    b.part == self.routed_part || reached_parts.contains(&b.part)
                };
            let mut p = PBox { x, y, w, h, name: b.name.clone(), detail: b.detail.clone(), chips: b.chips.clone(), bright, ..Default::default() };
            match b.kind {
                BoxKind::Frame => {
                    p.chosen = b.part == self.routed_part;
                    p.chevron = if self.collapsed { 2 } else { 0 };
                    p.cursor = cursor == Some(Pick::Frame(i));
                    self.frame_ids.push(i);
                    frames.push(p);
                }
                BoxKind::Part => {
                    p.part = true;
                    p.chosen = b.part == self.routed_part;
                    p.chevron = 1;
                    p.tint = match self.g.parts.iter().find(|x| x.id == b.part) {
                        Some(x) if x.draining => 1,
                        Some(x) if x.mute => 2,
                        _ => 0,
                    };
                    p.cursor = cursor == Some(Pick::Box(i));
                    self.box_ids.push(i);
                    boxes.push(p);
                }
                BoxKind::Node => {
                    p.chosen = self.routed.is_some_and(|(n, port)| n == b.node && ((b.node != GRAPH_INPUT && b.node != GRAPH_OUTPUT) || port == b.port));
                    p.off = self.g.disabled(b.node);
                    p.failed = failed(b.node);
                    p.badge = routing.find(b.node, b.port).is_some_and(|n| n.user_wires && n.part != 0);
                    p.cursor = cursor == Some(Pick::Box(i));
                    self.box_ids.push(i);
                    boxes.push(p);
                }
            }
        }
        for (i, c) in self.layout.controls.iter().enumerate() {
            let (x, y, w, h) = self.rect(c.bounds);
            controls.push(PBox { x, y, w, h, name: c.name.clone(), bright: true, cursor: cursor == Some(Pick::Control(i)), ..Default::default() });
        }
        // Wires, in the theme's colors: a line per channel of a bus, dashed when disabled or draining.
        let [audio, notes, disabled, draining, unconnected, _control, positive, warning, danger] = colors;
        let mut wires_now = vec![];
        for p in &self.layout.paths {
            if p.points.len() < 2 {
                continue;
            }
            let w = &self.g.wires[p.wire];
            let screen: Vec<(f64, f64)> = p.points.iter().map(|q| self.px(*q)).collect();
            let peak = self.peaks.get(&w.id).copied();
            let sounding = self.meters_on && !w.notes && w.connected && p.state == WireState::Enabled && peak.is_some_and(|v| v >= 0.001); // -60 dB
            let color = if !w.connected {
                unconnected
            } else if p.state == WireState::Draining {
                draining
            } else if p.state == WireState::Disabled {
                disabled
            } else if sounding {
                match peak.unwrap_or(0.) {
                    v if v >= 0.708 => danger,
                    v if v >= 0.251 => warning,
                    _ => positive,
                }
            } else if w.notes {
                notes
            } else {
                audio
            };
            let user = w.part.is_none();
            let alpha = if focus && !self.touches(w) { 0.15 } else if user || sounding { 1. } else { 0.75 };
            let width = (if user || p.state == WireState::Draining { 2.5 } else { 1.75 }) * self.scale.max(0.6);
            let lines = wires::wire_channels(w, &self.g).min(8) as usize;
            let line = if lines > 1 { width * 0.7 } else { width };
            let gap = width * 0.5;
            let dash = (w.connected && p.state != WireState::Enabled).then_some((6., 4.));
            wires_now.push(PWire { commands: wire_commands(&screen, lines, line + gap, dash), color, width: line as f32, alpha });
        }
        // Control lines.
        let mut links_now = vec![];
        for l in &self.layout.links {
            let points: Vec<(f64, f64)> = if l.points.is_empty() { vec![self.px(l.from), self.px(l.to)] } else { l.points.iter().map(|q| self.px(*q)).collect() };
            let lit = !focus || self.routed.is_some_and(|(n, _)| l.source == n || l.target == n);
            let to = self.px(l.to);
            links_now.push(PLink { commands: rounded(&points, 8. * self.scale), width: (2. * self.scale.max(0.6)) as f32, alpha: if lit { 1. } else { 0.15 }, dot: (to.0 as f32, to.1 as f32) });
        }
        sync(&self.frames, &mut self.last_frames, frames, PBox::slint);
        sync(&self.boxes, &mut self.last_boxes, boxes, PBox::slint);
        sync(&self.controls, &mut self.last_controls, controls, PBox::slint);
        sync(&self.wires, &mut self.last_wires, wires_now, |w| RoutingWire { commands: w.commands.as_str().into(), color: w.color, width: w.width, alpha: w.alpha });
        sync(&self.links, &mut self.last_links, links_now, |l| RoutingLink { commands: l.commands.as_str().into(), width: l.width, alpha: l.alpha, dot_x: l.dot.0, dot_y: l.dot.1 });
        // The panel.
        let units = ui.global::<RoutingState>().get_panel_units().max(1) as f32;
        let per = ui.global::<RoutingState>().get_panel_rows().max(1) as usize;
        let rows = self.packed(units);
        if matches!(self.stop, Some(Stop::Panel(r, _)) if r >= rows.len()) {
            self.stop = None;
            self.editing = false;
        }
        if let Some(Stop::Panel(r, _)) = self.stop {
            self.panel_first = r / per * per;
        }
        self.panel_first = if self.panel_first >= rows.len() { 0 } else { self.panel_first / per * per };
        let end = rows.len().min(self.panel_first + per);
        let at = match self.stop {
            Some(Stop::Panel(r, c)) => Some((r, c)),
            _ => None,
        };
        let editing = self.editing;
        let first = self.panel_first;
        // The rows of the view, and with them where the cursor is, so a row is set only when it changed.
        let compare: Vec<(PRow, Option<usize>)> = rows[first..end].iter().enumerate().map(|(i, r)| (r.clone(), at.filter(|a| a.0 == first + i).map(|a| a.1))).collect();
        if self.last_panel != compare || self.last_editing != editing {
            let now: Vec<PanelRowData> = compare
                .iter()
                .map(|(r, cursor)| PanelRowData {
                    kind: r.kind,
                    text: r.text.as_str().into(),
                    cells: ModelRc::new(VecModel::from(
                        r.cells.iter().enumerate().map(|(c, x)| PanelCellData { kind: x.kind, label: x.label.as_str().into(), on: x.on, value: x.value, width: x.width, enabled: x.enabled, cursor: *cursor == Some(c), editing: editing && *cursor == Some(c) }).collect::<Vec<_>>(),
                    )),
                })
                .collect();
            update(&self.panel, now);
            self.last_panel = compare;
            self.last_editing = editing;
        }
        let state = ui.global::<RoutingState>();
        state.set_scale(self.scale as f32);
        state.set_panel_need(rows.len() as i32);
        state.set_more_panel(rows.len() > per);
        state.set_message(self.said.as_str().into());
        let failure = self.failures.first().map(|f| format!("{} is silent: {}", f.1, f.2)).unwrap_or_default();
        if self.said.is_empty() && !failure.is_empty() {
            state.set_message(failure.into());
        }
        state.set_view(if rows.len() > per { format!("Panel {}–{} of {}", self.panel_first + 1, end, rows.len()) } else { String::new() }.into());
    }

    /// The screen's own actions, which the top bar carries: add a plugin, the parts collapsed, the meters.
    pub fn bar_tools(&self) -> Vec<crate::topbar::Tool> {
        vec![crate::topbar::Tool::new("+ Plugin"), crate::topbar::Tool { on: self.collapsed, ..crate::topbar::Tool::new("Parts collapsed") }, crate::topbar::Tool { on: self.meters_on, ..crate::topbar::Tool::new("Meters") }]
    }

    // The panel.

    fn cell(act: Act, label: &str, width: f32) -> PCell {
        PCell { act, kind: 0, label: label.into(), on: false, value: 0., width, enabled: true }
    }

    /// The panel for what is chosen, a row of words and cells each (`refresh`'s panel).
    fn panel_rows(&self) -> Vec<PRow> {
        let mut rows = vec![];
        if self.routed_part != 0 {
            let part = self.g.parts.iter().find(|p| p.id == self.routed_part);
            let mut cells = vec![Self::cell(Act::OpenPart(self.routed_part), "Open part", 0.8)];
            if self.collapsed {
                cells.push(Self::cell(Act::ToggleExpand(self.routed_part), if self.expanded.contains(&self.routed_part) { "Collapse" } else { "Expand" }, 0.8));
            }
            rows.push(PRow { kind: 3, text: part.map_or(String::new(), |p| p.name.clone()), cells });
            return rows;
        }
        let Some((node, port)) = self.routed else {
            rows.push(PRow { kind: 0, text: "Select a plugin to wire it, or to set the level of each source into it.".into(), cells: vec![] });
            return rows;
        };
        let routing = self.routing_nodes();
        let Some(chosen) = routing.find(node, port) else { return rows };
        let info = self.g.node(node);
        let sends_audio = if node == GRAPH_INPUT { port != NOTES_PORT } else { info.is_some_and(|n| !n.outputs.is_empty()) };
        let sends_notes = if node == GRAPH_INPUT { port == NOTES_PORT } else { info.is_some_and(|n| n.note_outputs > 0) };
        let mut cells = vec![];
        if sends_audio {
            cells.push(Self::cell(Act::ConnectAudio, "Connect audio to...", 1.0));
        }
        if sends_notes {
            cells.push(Self::cell(Act::ConnectNotes, "Connect notes to...", 1.0));
        }
        if sends_notes && node == GRAPH_INPUT && self.keys_offered {
            cells.push(Self::cell(Act::Keys, "Keyboard", 0.8));
        }
        let lanes: &[(u64, String)] = info.map_or(&[], |n| &n.lanes);
        if !lanes.is_empty() {
            cells.push(Self::cell(Act::ConnectLane, "Connect lane to...", 1.0));
        }
        if node != GRAPH_INPUT && node != GRAPH_OUTPUT {
            let off = self.g.disabled(node);
            cells.push(Self::cell(Act::Power(node, off), if off { "Turn on" } else { "Turn off" }, 0.7));
            if chosen.part != 0 {
                cells.push(Self::cell(Act::OpenPart(chosen.part), "Open part", 0.8));
            } else {
                cells.push(Self::cell(Act::Remove(node), "Remove", 0.7));
            }
            if self.windows.get(&node) == Some(&true) {
                cells.push(PCell { enabled: false, ..Self::cell(Act::None, "Plugin view: not available in this shell yet", 2.2) });
            }
        }
        rows.push(PRow { kind: 3, text: format!("{}{}", chosen.name, if chosen.part != 0 { format!("  ·  {}", chosen.part_name) } else { String::new() }), cells });
        // Its lanes' control lines: a mapping to a parameter, or a control it drives, each with Disconnect.
        for (lane, name) in lanes {
            let mut reached: Vec<String> = vec![];
            for m in self.g.mappings.iter().filter(|m| m.source == node && m.lane == *lane) {
                let param = self.params.get(&m.target).and_then(|l| l.iter().find(|p| p.id == m.param)).map_or(m.param.to_string(), |p| p.name.clone());
                reached.push(format!("{} · {}", self.g.node(m.target).map_or(String::new(), |n| n.name.clone()), param));
            }
            for m in self.g.macros.iter().filter(|m| m.source == Some((node, *lane))) {
                reached.push(format!("control {} ({} {})", m.name, m.targets, if m.targets == 1 { "target" } else { "targets" }));
            }
            if reached.is_empty() {
                continue;
            }
            rows.push(PRow { kind: 2, text: format!("{name} → {}", reached.join(", ")), cells: vec![Self::cell(Act::DisconnectLane(node, *lane), "Disconnect", 0.8)] });
        }
        // Each input with sources: a Mix row per source.
        for input in wires::inputs_of(&self.g, node) {
            if node == GRAPH_OUTPUT && input.port != port {
                continue;
            }
            rows.push(PRow { kind: 1, text: format!("{}{}", input.name, if input.rows.len() > 1 { format!(" · mixed from {} sources", input.rows.len()) } else { String::new() }), cells: vec![] });
            for row in &input.rows {
                let wire = self.g.wires.iter().find(|w| w.id == row.wire);
                let enabled = wire.is_none_or(|w| w.enabled);
                let mut cells = vec![PCell { kind: 1, on: enabled, ..Self::cell(Act::WireOn(row.wire), "On", 0.45) }];
                if input.port == NOTES_PORT {
                    // A note wire from the keys and MIDI: which channel it takes.
                    if row.keys && row.user {
                        let channel = zone_channel(&wire.map_or(vec![], |w| w.zones.clone()));
                        let label = match channel {
                            -1 => "Several channels".to_string(),
                            0 => "Wire channel: any".to_string(),
                            n => format!("Wire channel: {n}"),
                        };
                        cells.push(PCell { kind: 3, ..Self::cell(Act::WireChannel(row.wire), &label, 1.2) });
                    }
                    if row.user {
                        cells.push(Self::cell(Act::Disconnect(row.wire), "Disconnect", 0.8));
                    }
                } else {
                    cells.push(PCell { kind: 2, value: row.level.clamp(0., 1.) as f32, enabled: row.editable, ..Self::cell(Act::WireLevel(row.wire), "", 1.4) });
                    cells.push(PCell { kind: 1, on: row.mute, ..Self::cell(Act::WireMute(row.wire), "M", 0.3) });
                    cells.push(PCell { kind: 1, on: row.solo, ..Self::cell(Act::WireSolo(row.wire), "S", 0.3) });
                    if row.user {
                        cells.push(Self::cell(Act::Disconnect(row.wire), "Disconnect", 0.8));
                    }
                }
                rows.push(PRow { kind: 3, text: format!("{}{}", row.source, if row.user { "" } else { " · part" }), cells });
            }
        }
        rows
    }

    /// The panel's rows with a row's cells wrapped onto more rows where `units` button widths do not hold them.
    fn packed(&self, units: f32) -> Vec<PRow> {
        let mut out = vec![];
        for r in self.panel_rows() {
            if r.cells.is_empty() {
                out.push(r);
                continue;
            }
            let mut line = PRow { cells: vec![], ..r.clone() };
            let mut used = 0.;
            for c in r.cells {
                if !line.cells.is_empty() && used + c.width > units {
                    out.push(line);
                    line = PRow { kind: 3, text: String::new(), cells: vec![] };
                    used = 0.;
                }
                used += c.width;
                line.cells.push(c);
            }
            out.push(line);
        }
        out
    }

    fn panel_units(&self) -> f32 {
        self.ui.upgrade().map_or(1., |ui| ui.global::<RoutingState>().get_panel_units().max(1) as f32)
    }

    /// Where the cursor can go in the panel: each cell that does something.
    fn panel_stops(&self) -> Vec<(usize, usize)> {
        let rows = self.packed(self.panel_units());
        let mut v = vec![];
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.enabled && cell.act != Act::None {
                    v.push((r, c));
                }
            }
        }
        v
    }

    // The menus.

    /// Destinations for a wire from the chosen node: every node that takes that kind, and the outputs
    /// (`connectMenu`).
    fn connect_menu(&mut self, notes: bool) -> Option<(String, Vec<Item>)> {
        let (from, _) = self.routed?;
        let routing = self.routing_nodes();
        self.choices.clear();
        let mut items = vec![];
        let mut seen_output = false;
        for n in &routing.nodes {
            if n.node == from || n.node == GRAPH_INPUT {
                continue;
            }
            if n.node == GRAPH_OUTPUT {
                if notes || seen_output {
                    continue;
                }
                seen_output = true;
            } else {
                let Some(info) = self.g.node(n.node) else { continue };
                if if notes { info.note_inputs == 0 } else { info.inputs.is_empty() } {
                    continue;
                }
            }
            self.choices.push(Choice::Connect(from, n.node, notes));
            items.push(Item::choice(format!("{}{}", if n.node == GRAPH_OUTPUT { "Main output" } else { n.name.as_str() }, if n.part != 0 { format!(" · {}", n.part_name) } else { String::new() }), self.choices.len() - 1));
        }
        if items.is_empty() {
            items.push(Item::choice("Nothing takes this", 0).off());
        }
        Some((if notes { "Connect notes to" } else { "Connect audio to" }.into(), items))
    }

    /// The plugins there are to add outside the parts (`addLooseMenu`).
    fn plugin_menu(&mut self) -> Option<(String, Vec<Item>)> {
        self.choices.clear();
        let mut items = vec![];
        if self.plugins.is_empty() {
            items.push(Item::choice(if self.scanning { "Finding plugins..." } else { "No plugins found" }, 0).off());
        }
        // Split and Join belong to splits: they are not plugins of their own.
        for (i, p) in self.plugins.iter().enumerate().filter(|(_, p)| p.id != "org.oscairn.split" && p.id != "org.oscairn.join") {
            self.choices.push(Choice::Plugin(i));
            items.push(Item::choice(p.name.clone(), self.choices.len() - 1));
        }
        Some(("Add a plugin outside the parts".into(), items))
    }

    /// A lane of the chosen plugin to a control (one already there, or a new one) or to another plugin's
    /// parameter, which becomes a relative target of the lane's control: CLAP modulation (`laneMenu`).
    /// The parameters of every plugin it lists are read first.
    fn lane_menu_start(&mut self) -> Option<(String, Vec<Item>)> {
        let (from, _) = self.routed?;
        let missing: Vec<u64> = self.g.nodes.iter().map(|n| n.id).filter(|n| *n != from && !self.params.contains_key(n)).collect();
        if missing.is_empty() {
            return self.lane_menu(0);
        }
        self.waiting_lane = Some(0);
        self.said = "Reading the plugins' parameters...".into();
        for n in missing {
            self.ask(n);
        }
        None
    }

    fn lane_menu(&mut self, _wait: u64) -> Option<(String, Vec<Item>)> {
        let (from, _) = self.routed?;
        let info = self.g.node(from)?.clone();
        let routing = self.routing_nodes();
        self.choices.clear();
        let mut items = vec![];
        for (lane, lane_name) in &info.lanes {
            let several = info.lanes.len() > 1;
            let mut own = vec![];
            let mut controls = vec![];
            for m in self.g.macros.iter().filter(|m| m.source != Some((from, *lane))) {
                self.choices.push(Choice::LaneControl(from, *lane, m.id));
                controls.push(Item::choice(m.name.clone(), self.choices.len() - 1));
            }
            self.choices.push(Choice::LaneControl(from, *lane, 0));
            controls.push(Item::choice("New control", self.choices.len() - 1));
            own.push(Item::sub("Control", controls));
            for n in &routing.nodes {
                if n.node == from || n.node == GRAPH_INPUT || n.node == GRAPH_OUTPUT {
                    continue;
                }
                let mut params = vec![];
                for p in self.params.get(&n.node).into_iter().flatten().filter(|p| p.flags & READONLY == 0 && p.flags & HIDDEN == 0) {
                    self.choices.push(Choice::LaneParam(from, *lane, n.node, p.id));
                    params.push(Item::choice(p.name.clone(), self.choices.len() - 1));
                }
                if !params.is_empty() {
                    own.push(Item::sub(format!("{}{}", n.name, if n.part != 0 { format!(" · {}", n.part_name) } else { String::new() }), params));
                }
            }
            if several {
                items.push(Item::sub(lane_name.clone(), own));
            } else {
                items.extend(own);
            }
        }
        Some(("Connect lane to".into(), items))
    }

    /// A choice of the menu that was open.
    pub fn chosen(&mut self, id: usize) {
        let Some(choice) = self.choices.get(id).cloned() else { return };
        match choice {
            Choice::Connect(from, to, notes) => self.edit("graph.connect", json!({"kind": if notes { "notes" } else { "audio" }, "from": from, "to": to})),
            Choice::Plugin(i) => {
                if let Some(p) = self.plugins.get(i).cloned() {
                    let names: Vec<&str> = self.g.nodes.iter().map(|n| n.name.as_str()).collect();
                    let name = (1..).map(|n| if n > 1 { format!("{} {n}", p.name) } else { p.name.clone() }).find(|n| !names.contains(&n.as_str())).unwrap_or_default();
                    self.said.clear();
                    self.out.push((Request::Add, "graph.add", json!({"name": name, "path": p.path, "pluginId": p.id})));
                }
            }
            Choice::LaneControl(node, lane, control) => {
                if control != 0 {
                    self.lane_source(node, lane, control);
                    self.out.push((Request::Edit, "graph.commit", json!({})));
                } else {
                    self.lane_add(LaneOp::Control { node, lane });
                }
            }
            Choice::LaneParam(node, lane, to, param) => {
                // The lane's control, which is made when it has none: a relative target moving around where the parameter is.
                let existing = self.g.macros.iter().find(|m| m.source == Some((node, lane))).map(|m| m.id);
                match existing {
                    Some(control) => self.lane_target(control, to, param),
                    None => self.lane_add(LaneOp::Param { node, lane, to, param }),
                }
            }
        }
        self.show();
    }

    fn lane_add(&mut self, op: LaneOp) {
        let node = match &op {
            LaneOp::Control { node, .. } | LaneOp::Param { node, .. } => *node,
        };
        let name = self.g.node(node).map_or("Lane".to_string(), |n| n.name.clone());
        let key = match &op {
            LaneOp::Control { node, lane } => node * 1000 + lane,
            LaneOp::Param { node, lane, .. } => node * 1000 + lane,
        };
        self.pending.insert(key, op);
        self.said.clear();
        self.out.push((Request::LaneAdd(key), "controls.add", json!({"name": name})));
    }

    fn lane_source(&mut self, node: u64, lane: u64, control: u64) {
        self.out.push((Request::Edit, "controls.source", json!({"control": control, "node": node, "lane": lane})));
    }

    /// A relative target on `to`'s `param` added to the control the lane drives, committed.
    fn lane_target(&mut self, control: u64, to: u64, param: u64) {
        let mut targets: Vec<Value> = self.json["controls"].as_array().into_iter().flatten().find(|m| m["id"].as_u64() == Some(control)).and_then(|m| m["targets"].as_array().cloned()).unwrap_or_default().iter().map(|t| Target::read(t).json()).collect();
        let mut t = Target::new(TargetKind::Parameter(to, param));
        t.relative = true;
        targets.push(t.json());
        self.out.push((Request::Edit, "controls.targets", json!({"control": control, "targets": targets})));
        self.out.push((Request::Edit, "graph.commit", json!({})));
    }

    /// The control a lane's wiring waited for has a number: the rest of it.
    fn lane_made(&mut self, key: u64, control: u64) {
        let Some(op) = self.pending.remove(&key) else { return };
        match op {
            LaneOp::Control { node, lane } => {
                self.lane_source(node, lane, control);
                self.out.push((Request::Edit, "graph.commit", json!({})));
                self.out.push((Request::Edit, "controls.set", json!({"control": control, "value": 0.5})));
            }
            LaneOp::Param { node, lane, to, param } => {
                self.lane_source(node, lane, control);
                self.lane_target(control, to, param);
                self.out.push((Request::Edit, "controls.set", json!({"control": control, "value": 0.5})));
            }
        }
    }

    // Wires.

    fn mix(&mut self, wire: u64, change: impl FnOnce(&mut wires::Wire)) {
        let Some(w) = self.g.wires.iter_mut().find(|w| w.id == wire) else { return };
        change(w);
        self.dirty = true;
        let (level, pan, mute, solo) = (w.level, w.pan, w.mute, w.solo);
        self.edit("wires.mix", json!({"wire": wire, "level": level, "pan": pan, "mute": mute, "solo": solo}));
    }

    fn set_channel(&mut self, wire: u64, channel: i64) {
        self.dirty = true;
        let Some(w) = self.g.wires.iter_mut().find(|w| w.id == wire && w.notes) else { return };
        w.zones = set_zone_channel(&w.zones, channel);
        let zones = w.zones.clone();
        self.edit("wires.zones", json!({"wire": wire, "zones": zones}));
    }

    fn set_wire_on(&mut self, wire: u64, on: bool) {
        if let Some(w) = self.g.wires.iter_mut().find(|w| w.id == wire) {
            w.enabled = on;
        }
        self.dirty = true;
        self.edit("wires.enable", json!({"wire": wire, "enabled": on}));
    }

    fn disconnect_lane(&mut self, node: u64, lane: u64) {
        let mapped = self.g.mappings.iter().filter(|m| m.source == node && m.lane == lane).count();
        for _ in 0..mapped {
            self.said.clear();
            self.out.push((Request::Edit, "graph.unmap", json!({"from": node, "lane": lane})));
        }
        let macros: Vec<u64> = self.g.macros.iter().filter(|m| m.source == Some((node, lane))).map(|m| m.id).collect();
        for m in macros {
            self.out.push((Request::Edit, "controls.source", json!({"control": m})));
        }
        self.out.push((Request::Edit, "graph.commit", json!({})));
    }

    // Actions.

    fn wire_of(&self, row: u64) -> Option<&wires::Wire> {
        self.g.wires.iter().find(|w| w.id == row)
    }

    /// What a press on a panel cell does.
    fn press_cell(&mut self, act: Act) -> Outcome {
        match act {
            Act::None => {}
            Act::OpenPart(id) => return Outcome::OpenPart(id as u32),
            Act::ToggleExpand(id) => self.toggle_expand(id),
            Act::ConnectAudio => self.ready_menu = self.connect_menu(false),
            Act::ConnectNotes => self.ready_menu = self.connect_menu(true),
            Act::ConnectLane => self.ready_menu = self.lane_menu_start(),
            Act::Keys => return Outcome::Keys(String::new()),
            Act::Power(node, off) => self.edit("graph.enable", json!({"node": node, "enabled": off})),
            Act::Remove(node) => {
                self.routed = None;
                self.stop = None;
                self.edit("graph.remove", json!({"node": node, "userWires": "reconnect"}));
            }
            Act::DisconnectLane(node, lane) => self.disconnect_lane(node, lane),
            Act::WireOn(w) => {
                let on = self.wire_of(w).is_some_and(|w| w.enabled);
                self.set_wire_on(w, !on);
            }
            Act::WireLevel(_) | Act::WireChannel(_) => self.editing = !self.editing,
            Act::WireMute(w) => self.mix(w, |w| w.mute = !w.mute),
            Act::WireSolo(w) => self.mix(w, |w| w.solo = !w.solo),
            Act::Disconnect(w) => {
                self.stop = None;
                self.edit("wires.disconnect", json!({"wire": w}));
            }
        }
        Outcome::Stay
    }

    /// The cell at `(row, cell)` of the whole panel.
    fn cell_at(&self, row: usize, cell: usize) -> Option<PCell> {
        self.packed(self.panel_units()).get(row)?.cells.get(cell).cloned()
    }

    /// The dial on the value being changed: a wire's level (a fortieth a click) or its channel.
    fn step_value(&mut self, by: i64) {
        let Some(Stop::Panel(r, c)) = self.stop else { return };
        let Some(cell) = self.cell_at(r, c) else { return };
        match cell.act {
            Act::WireLevel(w) => {
                let was = self.wire_of(w).map_or(1., |w| w.level);
                let level = (((was + 0.025 * by as f64).clamp(0., 1.)) * 1000.).round() / 1000.;
                if (level - was).abs() > f64::EPSILON {
                    self.mix(w, |w| w.level = level);
                }
            }
            Act::WireChannel(w) => {
                let was = zone_channel(&self.wire_of(w).map_or(vec![], |w| w.zones.clone())).max(0);
                let to = (was + by).clamp(0, 16);
                if to != was {
                    self.set_channel(w, to);
                }
            }
            _ => {}
        }
    }

    /// What a press on a place of the canvas does: choose a plugin, end or part (again: let go of it).
    fn press_pick(&mut self, pick: Pick) -> Outcome {
        match pick {
            Pick::Control(_) => return Outcome::Controls,
            Pick::Frame(i) | Pick::Box(i) => {
                let Some(b) = self.layout.boxes.get(i).cloned() else { return Outcome::Stay };
                if b.kind == BoxKind::Node {
                    let again = self.routed == Some((b.node, b.port));
                    self.routed = (!again).then_some((b.node, b.port));
                    self.routed_part = 0;
                    self.panel_first = 0;
                    if !again {
                        self.ask_lane_names();
                    }
                } else {
                    self.routed_part = if self.routed_part == b.part { 0 } else { b.part };
                    self.routed = None;
                    self.panel_first = 0;
                }
            }
        }
        Outcome::Stay
    }

    /// A tap on the chevron of a part's box or frame: it expands or collapses.
    fn toggle_expand(&mut self, part: u64) {
        if self.collapsed && !self.expanded.remove(&part) {
            self.expanded.insert(part);
        }
        self.dirty = true;
    }

    fn press_tool(&mut self, t: usize) {
        match t {
            0 => self.ready_menu = self.plugin_menu(),
            1 => {
                self.collapsed = !self.collapsed;
                self.expanded.clear();
                self.dirty = true;
                crate::settings::keep_routing_collapsed(self.collapsed);
            }
            _ => {
                self.meters_on = !self.meters_on;
                self.peaks.clear();
            }
        }
    }

    /// What a press on `stop` does.
    fn press(&mut self, stop: Stop) -> Outcome {
        match stop {
            Stop::Canvas(i) => {
                if let Some(p) = self.order.get(i).copied() {
                    return self.press_pick(p);
                }
            }
            Stop::Panel(r, c) => {
                if let Some(cell) = self.cell_at(r, c) {
                    return self.press_cell(cell.act);
                }
            }
        }
        Outcome::Stay
    }

    // The cursor.

    fn stops(&self) -> Vec<Stop> {
        let mut v: Vec<Stop> = (0..self.order.len()).map(Stop::Canvas).collect();
        v.extend(self.panel_stops().into_iter().map(|(r, c)| Stop::Panel(r, c)));
        v
    }

    /// The cursor `by` places along the dial, over the buttons, the boxes and the panel in one list.
    fn dial(&mut self, by: i64) {
        let stops = self.stops();
        let at = match self.stop.and_then(|s| stops.iter().position(|x| *x == s)) {
            Some(at) => (at as i64 + by).clamp(0, stops.len() as i64 - 1) as usize,
            None => stops.iter().position(|s| matches!(s, Stop::Canvas(_))).unwrap_or(0), // the first move shows it on the first box
        };
        self.stop = stops.get(at).copied();
    }

    /// The left knob: over the canvas's places, as far as the first and the last.
    fn along_canvas(&mut self, by: i64) {
        if self.order.is_empty() {
            return;
        }
        self.stop = Some(Stop::Canvas(match self.stop {
            Some(Stop::Canvas(i)) => (i as i64 + by).clamp(0, self.order.len() as i64 - 1) as usize,
            _ => 0,
        }));
    }

    /// The right knob: over the panel's cells.
    fn along_panel(&mut self, by: i64) {
        let stops = self.panel_stops();
        if stops.is_empty() {
            return;
        }
        let at = match self.stop {
            Some(Stop::Panel(r, c)) => stops.iter().position(|s| *s == (r, c)).map_or(0, |a| (a as i64 + by).clamp(0, stops.len() as i64 - 1) as usize),
            _ => 0,
        };
        self.stop = Some(Stop::Panel(stops[at].0, stops[at].1));
    }

    /// An arrow across the canvas: the nearest place in the column to the left (-1) or right (1).
    fn across(&mut self, dx: i64) {
        let Some(Stop::Canvas(i)) = self.stop else { return self.along_canvas(0) };
        let centre = |p: &Pick| match p {
            Pick::Frame(i) | Pick::Box(i) => self.layout.boxes.get(*i).map(|b| (f64::from(b.bounds.x) + f64::from(b.bounds.w) / 2., f64::from(b.bounds.y) + f64::from(b.bounds.h) / 2.)),
            Pick::Control(i) => self.layout.controls.get(*i).map(|c| (f64::from(c.bounds.x) + f64::from(c.bounds.w) / 2., f64::from(c.bounds.y) + f64::from(c.bounds.h) / 2.)),
        };
        let Some(here) = self.order.get(i).and_then(centre) else { return };
        let mut best: Option<(f64, usize)> = None;
        for (k, p) in self.order.iter().enumerate() {
            let Some(c) = centre(p) else { continue };
            if (dx > 0 && c.0 <= here.0 + 1.) || (dx < 0 && c.0 >= here.0 - 1.) {
                continue;
            }
            let score = (c.0 - here.0).abs() * 4. + (c.1 - here.1).abs();
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, k));
            }
        }
        if let Some((_, k)) = best {
            self.stop = Some(Stop::Canvas(k));
        }
    }

    /// The cursor is on the first stop: moving up leaves for the top bar's actions.
    pub fn at_top(&self) -> bool {
        self.stop == self.stops().first().copied() && !self.editing
    }

    /// A navigation action.
    pub fn act(&mut self, action: Action) -> Outcome {
        self.said.clear();
        let pane = |p: Option<u8>, touched: u8| p.map_or(touched, |p| p.min(1));
        match action {
            Action::Move { pane: p, by } => {
                if self.editing {
                    self.step_value(i64::from(by));
                } else {
                    let to = pane(p, self.touched);
                    if p.is_some() {
                        self.touched = to;
                        if to == 0 { self.along_canvas(i64::from(by)) } else { self.along_panel(i64::from(by)) }
                    } else {
                        self.dial(i64::from(by));
                    }
                }
            }
            Action::Arrow { dx, dy } => {
                if self.editing {
                    self.step_value(i64::from(if dx != 0 { dx } else { dy }));
                } else if dx != 0 && matches!(self.stop, Some(Stop::Canvas(_))) {
                    self.across(i64::from(dx));
                } else {
                    self.dial(i64::from(if dy != 0 { dy } else { dx }));
                }
            }
            Action::Page(by) => {
                if !self.editing {
                    self.turn_panel(by);
                }
            }
            Action::End(by) => {
                if !self.editing {
                    let stops = self.stops();
                    self.stop = if by < 0 { stops.first().copied() } else { stops.last().copied() };
                }
            }
            Action::Confirm(_) => {
                if let Some(stop) = self.stop {
                    let outcome = self.press(stop);
                    self.show();
                    return outcome;
                }
            }
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

    /// A view of the panel towards its start (-1) or end (1).
    pub fn turn_panel(&mut self, by: i32) {
        let per = self.ui.upgrade().map_or(1, |ui| ui.global::<RoutingState>().get_panel_rows().max(1)) as usize;
        let rows = self.packed(self.panel_units()).len();
        let views = rows.div_ceil(per).max(1);
        let to = ((self.panel_first / per) as i64 + i64::from(by)).clamp(0, views as i64 - 1) as usize;
        self.panel_first = to * per;
        if let Some(Stop::Panel(..)) = self.stop {
            let stops = self.panel_stops();
            if let Some(s) = stops.iter().find(|s| s.0 >= self.panel_first) {
                self.stop = Some(Stop::Panel(s.0, s.1));
            }
        }
        self.show();
    }

    // Touch.

    fn touched(&mut self) {
        self.stop = None;
        self.editing = false;
    }

    /// A tap on a box (`kind` 0 a frame, 1 a plugin, end or part, 2 a control) of that list.
    pub fn tap(&mut self, kind: i32, index: i32) -> Outcome {
        self.touched();
        let at = index.max(0) as usize;
        let pick = match kind {
            0 => self.frame_ids.get(at).map(|i| Pick::Frame(*i)),
            1 => self.box_ids.get(at).map(|i| Pick::Box(*i)),
            _ => (at < self.layout.controls.len()).then_some(Pick::Control(at)),
        };
        let outcome = pick.map_or(Outcome::Stay, |p| self.press_pick(p));
        self.show();
        outcome
    }

    /// A tap on the chevron of a frame (kind 0) or a part's box (kind 1).
    pub fn tap_chevron(&mut self, kind: i32, index: i32) {
        self.touched();
        let at = index.max(0) as usize;
        let id = if kind == 0 { self.frame_ids.get(at) } else { self.box_ids.get(at) };
        if let Some(part) = id.and_then(|i| self.layout.boxes.get(*i)).map(|b| b.part).filter(|p| *p != 0) {
            self.toggle_expand(part);
        }
        self.show();
    }

    pub fn tap_tool(&mut self, i: i32) {
        self.touched();
        self.press_tool(i.max(0) as usize);
        self.show();
    }

    fn panel_row_of(&self, row: i32) -> usize {
        self.panel_first + row.max(0) as usize
    }

    /// A tap on cell `cell` of the view's row `row`.
    pub fn tap_cell(&mut self, row: i32, cell: i32) -> Outcome {
        self.touched();
        let outcome = match self.cell_at(self.panel_row_of(row), cell.max(0) as usize) {
            Some(PCell { act: Act::WireChannel(w), .. }) => {
                // A tap on the channel steps it round: any, 1 to 16, any.
                let was = zone_channel(&self.wire_of(w).map_or(vec![], |w| w.zones.clone())).max(0);
                self.set_channel(w, (was + 1) % 17);
                Outcome::Stay
            }
            Some(PCell { act: Act::WireLevel(_), .. }) => Outcome::Stay,
            Some(c) => self.press_cell(c.act),
            None => Outcome::Stay,
        };
        self.show();
        outcome
    }

    /// The level slider of a source moved to `place` of its length.
    pub fn level(&mut self, row: i32, cell: i32, place: f32) {
        self.touched();
        if let Some(PCell { act: Act::WireLevel(w), .. }) = self.cell_at(self.panel_row_of(row), cell.max(0) as usize) {
            let level = (f64::from(place) * 1000.).round() / 1000.;
            if self.wire_of(w).is_some_and(|x| (x.level - level).abs() > f64::EPSILON) {
                self.mix(w, |w| w.level = level);
            }
        }
        self.show();
    }

    pub fn hold(&mut self, row: i32, cell: i32, down: bool) {
        self.held = if down {
            match self.cell_at(self.panel_row_of(row), cell.max(0) as usize) {
                Some(PCell { act: Act::WireLevel(w), .. }) => Some(w),
                _ => None,
            }
        } else {
            None
        };
    }

    /// Where each drawn audio wire leaves and enters, in the pixels of the canvas, with its id, for tests.
    #[cfg(test)]
    pub fn wire_ends(&self) -> Vec<WireEnds> {
        self.layout.paths.iter().filter(|p| p.points.len() >= 2 && !self.g.wires[p.wire].notes).map(|p| (self.g.wires[p.wire].id, self.px(p.points[0]), self.px(*p.points.last().unwrap()), wires::wire_channels(&self.g.wires[p.wire], &self.g) as usize)).collect()
    }

    /// The cursor drops when the screen goes.
    pub fn hidden(&mut self) {
        self.stop = None;
        self.editing = false;
        self.held = None;
        self.said.clear();
        self.show();
    }

    /// Shown: the sizes may have changed while it was not.
    pub fn shown(&mut self) {
        self.show();
    }
}
