// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Parts list: page 0 of the JUCE Edit view (hosts/standalone/PartsView.cpp, PartsModel.cpp's
//! readParts), on the shell. A row per part, then the buses ("Shared buses") and the plugins several
//! parts are wired into in Routing; each part and bus has its name and sound, where it plays from, its
//! chain, a level, a mute and (parts) a send per bus. Under the list: Mixer, Controls and Routing (the
//! pages of the JUCE Edit view), + Part and + Bus.
//!
//! Without touch the cursor has two dimensions, as the grid's: the dial moves along the cells of a
//! row (the part, its level, its mute, each send) and on to the next row, then the buttons under the
//! list; the left knob (pane 0) moves along the cells, the right knob (pane 1) along the rows. A press
//! on the part opens its Part screen, on a level or a send starts changing it (the dial then steps
//! it by a fortieth of the slider, a press or Cancel ends that), on mute toggles it, on a shared row
//! opens Routing, on a button does it. Cancel ends a change, drops the cursor, then goes back. Touch:
//! a tap on the part's words opens it, the sliders drag, the mute and the buttons tap.
//!
//! How many rows a view holds is whatever fills the height (the screen's component says how tall the
//! list is and what a row costs), so it follows the window, the spacing and the type scale: the view
//! is cut in whole views from the first row, the cursor's view shown.
//!
//! Requests: `graph.get` (asked by the shell), `parts.meters` every METER_INTERVAL while shown (the
//! notes the shared budget took, as the JUCE view polls them), `session.status` (devices, failures),
//! `parts.level` (a level or a mute) and `parts.send`, each followed by `graph.commit` as the JUCE
//! setters commit, and `parts.add` then `graph.commit` for a part named "Part N" or a bus "Bus A".

use crate::nav::Action;
use crate::play::{chain_nodes, devices_name, is_instrument, note_name};
use crate::setlist::update;
use crate::{ChipData, PartRowData, PartsState, SendData, Shell, ToolData};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::Duration;

/// How often the notes given up are asked while the list is shown (the JUCE view's timer, 200 ms).
pub const METER_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Meters,
    Status,
    /// Any edit: its refusal is said.
    Edit,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Back to Play.
    Back,
    /// A part's Part screen.
    Open(u32),
    /// Routing, at this node.
    Routing(u64),
    /// One of the pages the buttons under the list go to.
    Page(Page),
}

/// How a plugin of a chain is drawn: by its role, as the JUCE chip's colors say it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Role {
    #[default]
    Other,
    Instrument,
    Effect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chip {
    pub node: u64,
    pub name: String,
    pub role: Role,
    pub off: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub id: u32,
    pub bus: bool,
    pub name: String,
    pub sound: String,
    pub input: String,
    /// Whether it plays from notes, its devices (rules as the engine gives them) and its first zone's keys.
    pub notes: bool,
    pub devices: Value,
    pub keys: (i64, i64),
    pub level: f64,
    pub mute: bool,
    pub silenced: bool,
    pub draining: bool,
    /// The level of the send to each bus, in the order of the buses (None: no send).
    pub sends: Vec<Option<f64>>,
    pub chain: Vec<Chip>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    pub node: u64,
    pub name: String,
    pub fed_by: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Row(Row),
    Shared(Shared),
}

/// The page the buttons under the list go to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Page {
    Mixer,
    Controls,
    Routing,
}

impl Page {
    fn name(self) -> &'static str {
        match self {
            Page::Mixer => "Mixer",
            Page::Controls => "Controls",
            Page::Routing => "Routing",
        }
    }

}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    Page(Page),
    AddPart,
    AddBus,
}

const TOOLS: [Tool; 5] = [Tool::Page(Page::Mixer), Tool::Page(Page::Controls), Tool::Page(Page::Routing), Tool::AddPart, Tool::AddBus];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    /// A cell of an entry: 0 the part, 1 level, 2 mute, 3 and on the sends.
    Cell(usize, usize),
    Tool(usize),
}

/// The role of a node, as PartsModel.cpp's roleOf names it (what a chip is colored by).
pub(crate) fn role_of(node: &Value) -> &'static str {
    let channels = |key: &str| node[key].as_array().map_or(0, |p| p.iter().map(|p| p["channels"].as_u64().unwrap_or(0)).sum::<u64>());
    let (i, o) = (channels("audioInputs"), channels("audioOutputs"));
    let (ni, no) = (node["noteInputs"].as_u64().unwrap_or(0), node["noteOutputs"].as_u64().unwrap_or(0));
    if ni > 0 && no > 0 && o == 0 {
        "Notes"
    } else if no > 0 && o == 0 {
        if ni > 0 { "Notes" } else { "Sequencer" }
    } else if o > 0 && i == 0 {
        if ni > 0 { "Instrument" } else { "Generator" }
    } else if i > 0 && o > 0 {
        "Effect"
    } else if i > 0 {
        "Analyzer"
    } else {
        "Notes"
    }
}

/// The color class of a plugin's role in a list: 1 an instrument, 2 an effect, 3 the rest (notes, sequencers and the like).
pub(crate) fn role_color(role: &str) -> i32 {
    match role {
        "Instrument" => 1,
        "Effect" => 2,
        _ => 3,
    }
}

/// The name a lane gives a split's box: its instrument, else its last box, else its own name ("Dry" for a copy).
pub(crate) fn lane_name(kind: &str, k: usize, boxes: &[Chip]) -> String {
    if let Some(b) = boxes.iter().find(|b| b.role == Role::Instrument) {
        return b.name.clone();
    }
    if let Some(b) = boxes.last() {
        return b.name.clone();
    }
    match (kind, k) {
        ("leftRight", 0) => "Left".into(),
        ("leftRight", _) => "Right".into(),
        ("midSide", 0) => "Mid".into(),
        ("midSide", _) => "Side".into(),
        ("bands", k) => format!("Band {}", k + 1),
        _ => "Dry".into(),
    }
}

/// A chain's boxes in order; a split is one box named after its lanes, as readParts makes it.
pub(crate) fn boxes(chain: &Value, part: &Value, nodes: &HashMap<u64, &Value>, depth: usize) -> Vec<Chip> {
    let mut out = Vec::new();
    for entry in chain.as_array().into_iter().flatten() {
        if let Some(n) = entry.as_u64() {
            if let Some(node) = nodes.get(&n) {
                let role = match role_of(node) {
                    "Instrument" => Role::Instrument,
                    "Effect" => Role::Effect,
                    _ => Role::Other,
                };
                out.push(Chip { node: n, name: node["name"].as_str().unwrap_or_default().into(), role, off: node["enabled"] == false });
            }
        } else if let Some(split) = part["splits"].as_array().and_then(|s| s.iter().find(|s| s["id"] == entry["split"])).filter(|_| depth < 8) {
            let kind = split["kind"].as_str().unwrap_or("copy");
            let names: Vec<String> = split["lanes"].as_array().into_iter().flatten().enumerate().map(|(k, lane)| lane_name(kind, k, &boxes(&lane["chain"], part, nodes, depth + 1))).collect();
            out.push(Chip { node: 0, name: names.join(" | "), role: Role::Other, off: false });
        }
    }
    out
}

/// Whether a part's devices find an input that is connected, as `resolveDevices` does for a part
/// with devices (the built-in keys are always there). A regular expression cannot be tried here, so
/// a part named by one is taken as found.
fn devices_found(rules: &Value, inputs: &[Value]) -> bool {
    let Some(rules) = rules.as_array().filter(|r| !r.is_empty()) else { return true };
    let named = |rule: &Value, name: &str| {
        let (pattern, name) = (rule["pattern"].as_str().unwrap_or_default().to_lowercase(), name.to_lowercase());
        match rule["match"].as_str() {
            Some("contains") => name.contains(&pattern),
            Some("regex") => true,
            _ => name == pattern,
        }
    };
    rules.iter().any(|r| match r["kind"].as_str() {
        Some("all" | "builtIn") => true,
        Some("external") => inputs.iter().any(|e| e["kind"] != "builtIn"),
        Some("port") => inputs.iter().any(|e| e["kind"] == "port" && e["port"] == r["port"]),
        Some("usb") => inputs.iter().any(|e| e["kind"] == "usb"),
        Some("usbNamed") => inputs.iter().any(|e| e["kind"] == "usb" && named(r, e["name"].as_str().unwrap_or_default())),
        Some("network") => inputs.iter().any(|e| e["kind"] == "network" && (r["pattern"].as_str().unwrap_or_default().is_empty() || named(r, e["name"].as_str().unwrap_or_default()))),
        _ => false,
    })
}

/// graph.get's parts, buses and shared nodes, as PartsModel.cpp's readParts gives them: the parts,
/// then the buses, then the nodes outside every part that several parts are wired into.
pub fn read(graph: &Value, inputs: &[Value]) -> (Vec<Row>, Vec<Row>, Vec<Shared>) {
    let nodes: HashMap<u64, &Value> = graph["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_u64()?, n))).collect();
    let name = |id: u64| nodes.get(&id).and_then(|n| n["name"].as_str()).unwrap_or("a plugin").to_string();
    let wires = graph["wires"].as_array().cloned().unwrap_or_default();
    let all = graph["parts"].as_array().cloned().unwrap_or_default();
    let is_bus = |p: &Value| p["input"]["kind"] == "bus";
    let buses: Vec<u64> = all.iter().filter(|p| is_bus(p)).filter_map(|p| p["id"].as_u64()).collect();
    // The parts sending to each bus, for what a bus says it is fed by.
    let mut feeders: HashMap<u64, Vec<String>> = HashMap::new();
    for p in &all {
        let pname = p["name"].as_str().unwrap_or_default().to_string();
        if let Some(b) = p["bus"].as_u64().filter(|b| *b != 0) {
            feeders.entry(b).or_default().push(pname.clone());
        }
        for s in p["sends"].as_array().into_iter().flatten() {
            if let Some(b) = s["bus"].as_u64() {
                feeders.entry(b).or_default().push(pname.clone());
            }
        }
    }
    let (mut parts, mut bus_rows) = (Vec::new(), Vec::new());
    for p in &all {
        let id = p["id"].as_u64().unwrap_or(0);
        let bus = is_bus(p);
        let mut flat = Vec::new();
        chain_nodes(&p["chain"], &p["splits"], 0, &mut flat);
        let sound = flat.iter().find(|n| nodes.get(n).is_some_and(|n| is_instrument(n))).map(|n| name(*n)).unwrap_or_default();
        let mute = p["mute"].as_bool().unwrap_or(false);
        let draining = wires.iter().any(|w| w["owner"] == "part" && w["part"].as_u64() == Some(id) && [&w["from"], &w["to"]].iter().any(|n| n.as_u64().and_then(|n| nodes.get(&n)).is_some_and(|n| n["ringing"] == true)));
        let silenced = !mute && wires.iter().any(|w| w["owner"] == "part" && w["part"].as_u64() == Some(id) && w["fader"] == true && w["mute"] == true);
        let notes = p["input"]["kind"] == "notes";
        let zone = p["input"]["zones"].as_array().and_then(|z| z.first());
        let key = |i: usize, default: i64| zone.and_then(|z| z["keys"][i].as_i64()).unwrap_or(default);
        let (low, high) = (key(0, 0), key(1, 127));
        let transpose = zone.and_then(|z| z["transpose"].as_i64()).unwrap_or(0);
        let mut input = match p["input"]["kind"].as_str() {
            Some("notes") => {
                let mut text = format!("{} · {}", devices_name(&p["input"]["devices"]), if low == 0 && high == 127 { "all keys".to_string() } else { format!("{}–{}", note_name(low), note_name(high)) });
                if transpose != 0 {
                    text += &format!(" · {}{}", if transpose > 0 { "+" } else { "−" }, transpose.abs());
                }
                text
            }
            Some("audio") => format!("Audio in {}", p["input"]["port"].as_i64().unwrap_or(0) + 1),
            Some("bus") => match feeders.get(&id) {
                Some(from) if !from.is_empty() => format!("From {}", from.join(", ")),
                _ => "Nothing sends to it yet".into(),
            },
            _ => "Its own notes".into(),
        };
        if notes && p["input"]["devices"].as_array().is_some_and(|d| !d.is_empty()) && !devices_found(&p["input"]["devices"], inputs) {
            input = format!("No device: {input}");
        }
        let sends = buses.iter().map(|b| p["sends"].as_array().and_then(|s| s.iter().find(|s| s["bus"].as_u64() == Some(*b))).and_then(|s| s["level"].as_f64())).collect();
        let row = Row {
            id: id as u32,
            bus,
            name: p["name"].as_str().unwrap_or_default().into(),
            sound,
            input,
            notes,
            devices: p["input"]["devices"].clone(),
            keys: (low, high),
            level: p["level"].as_f64().unwrap_or(1.),
            mute,
            silenced,
            draining,
            sends,
            chain: boxes(&p["chain"], p, &nodes, 0),
        };
        if bus { bus_rows.push(row) } else { parts.push(row) }
    }
    // The nodes outside the parts that parts are wired into in Routing; several make a shared row.
    let mut owner: HashMap<u64, u64> = HashMap::new();
    for p in &all {
        let id = p["id"].as_u64().unwrap_or(0);
        let mut flat = Vec::new();
        chain_nodes(&p["chain"], &p["splits"], 0, &mut flat);
        for n in flat {
            owner.insert(n, id);
        }
        for s in p["splits"].as_array().into_iter().flatten() {
            for key in ["splitter", "joiner"] {
                if let Some(n) = s[key].as_u64().filter(|n| *n != 0) {
                    owner.insert(n, id);
                }
            }
        }
    }
    let mut fed: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for w in &wires {
        let (Some(from), Some(to)) = (w["from"].as_u64(), w["to"].as_u64()) else { continue };
        if w["owner"] == "part" || w["kind"] != "audio" || owner.contains_key(&to) || to == u64::from(u32::MAX) {
            continue;
        }
        let Some(part) = owner.get(&from) else { continue };
        let list = fed.entry(to).or_default();
        if !list.contains(part) {
            list.push(*part);
        }
    }
    let shared = fed
        .into_iter()
        .filter(|(_, from)| from.len() > 1)
        .map(|(node, from)| Shared { node, name: name(node), fed_by: from.iter().flat_map(|id| all.iter().filter(move |p| p["id"].as_u64() == Some(*id)).map(|p| p["name"].as_str().unwrap_or_default().to_string())).collect() })
        .collect();
    (parts, bus_rows, shared)
}

/// A level by `by` clicks of the dial, a fortieth of the slider each, to a thousandth, held at the ends.
fn stepped(was: f64, by: i64) -> f64 {
    ((was + 0.025 * by as f64).clamp(0., 1.) * 1000.).round() / 1000.
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    graph: Value,
    entries: Vec<Entry>,
    /// The buses, which each part's sends are in the order of: id and name.
    buses: Vec<(u64, String)>,
    title: String,
    inputs: Vec<Value>,
    failures: Vec<(u64, String, String)>,
    /// What the shared budget took from each part: notes stolen, notes cut.
    given: HashMap<u32, (u64, u64)>,
    stop: Option<Stop>,
    /// The dial changes the level or send under the cursor.
    editing: bool,
    /// The first entry of the view shown.
    first: usize,
    /// The refusal of the last edit, or what a button that goes nowhere yet says.
    said: String,
    /// The slider a finger is on: the part and the cell. The engine's own values for it wait.
    held: Option<(u32, usize)>,
    rows: Rc<VecModel<PartRowData>>,
    chips: Rc<VecModel<ChipData>>,
    sends: Rc<VecModel<SendData>>,
    tools: Rc<VecModel<ToolData>>,
    bus_names: Rc<VecModel<SharedString>>,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(window: &Shell) -> Screen {
        let s = Screen {
            ui: window.as_weak(),
            graph: Value::Null,
            entries: vec![],
            buses: vec![],
            title: String::new(),
            inputs: vec![],
            failures: vec![],
            given: HashMap::new(),
            stop: None,
            editing: false,
            first: 0,
            said: String::new(),
            held: None,
            rows: Rc::new(VecModel::default()),
            chips: Rc::new(VecModel::default()),
            sends: Rc::new(VecModel::default()),
            tools: Rc::new(VecModel::default()),
            bus_names: Rc::new(VecModel::default()),
            out: vec![],
        };
        let state = window.global::<PartsState>();
        state.set_rows(ModelRc::from(s.rows.clone()));
        state.set_chips(ModelRc::from(s.chips.clone()));
        state.set_sends(ModelRc::from(s.sends.clone()));
        state.set_tools(ModelRc::from(s.tools.clone()));
        state.set_bus_names(ModelRc::from(s.bus_names.clone()));
        s
    }

    /// The open Performance's name, the list's title ("Parts" when it has none).
    pub fn performance(&mut self, name: &str) {
        self.title = name.to_string();
        self.show();
    }

    /// graph.get: the rows again, the cursor kept where it is.
    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
        self.load();
        self.show();
    }

    fn load(&mut self) {
        let (parts, buses, shared) = read(&self.graph, &self.inputs);
        self.buses = buses.iter().map(|b| (u64::from(b.id), b.name.clone())).collect();
        let held = self.held;
        let old: HashMap<u32, Row> = self.entries.drain(..).filter_map(|e| if let Entry::Row(r) = e { Some((r.id, r)) } else { None }).collect();
        self.entries = parts
            .into_iter()
            .chain(buses)
            .map(|mut r| {
                // A slider being dragged keeps its own value until the finger is up.
                if let (Some((id, cell)), Some(was)) = (held, old.get(&r.id)) {
                    if id == r.id {
                        match cell {
                            1 => r.level = was.level,
                            c if c >= 3 => {
                                if let (Some(slot), Some(v)) = (r.sends.get_mut(c - 3), was.sends.get(c - 3)) {
                                    *slot = *v;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Entry::Row(r)
            })
            .chain(shared.into_iter().map(Entry::Shared))
            .collect();
    }

    /// controls.changed: levels and mutes moved live (a slider with a finger on it keeps its own).
    pub fn controls(&mut self, params: &Value) {
        for c in params["parts"].as_array().into_iter().flatten() {
            let Some(id) = c["part"].as_u64() else { continue };
            for e in &mut self.entries {
                if let Entry::Row(r) = e {
                    if u64::from(r.id) == id {
                        if self.held != Some((r.id, 1)) {
                            r.level = c["level"].as_f64().unwrap_or(r.level);
                        }
                        r.mute = c["mute"].as_bool().unwrap_or(r.mute);
                    }
                }
            }
        }
        self.show();
    }

    /// session.status (a reply or the notification): the MIDI inputs, which say whether a part's
    /// devices are connected, and the plugins that failed.
    pub fn status(&mut self, status: &Value) {
        let mut changed = false;
        if let Some(inputs) = status["midiInputs"].as_array().filter(|i| **i != self.inputs) {
            self.inputs = inputs.clone();
            changed = true;
        }
        if let Some(list) = status["failures"].as_array() {
            let failures: Vec<(u64, String, String)> = list.iter().map(|f| (f["node"].as_u64().unwrap_or(0), f["name"].as_str().unwrap_or_default().into(), f["reason"].as_str().unwrap_or_default().into())).collect();
            if failures != self.failures {
                self.failures = failures;
                changed = true;
            }
        }
        if changed {
            self.load();
            self.show();
        }
    }

    /// parts.meters: the notes given up to the shared budget, by part (the JUCE view's `givenUp_`).
    pub fn meters(&mut self, result: &Value) {
        let given: HashMap<u32, (u64, u64)> = result["meters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let (stolen, cut) = (m["stolen"].as_u64().unwrap_or(0), m["hardChokes"].as_u64().unwrap_or(0));
                (stolen > 0 || cut > 0).then(|| Some((m["part"].as_u64()? as u32, (stolen, cut))))?
            })
            .collect();
        if given != self.given {
            self.given = given;
            self.show();
        }
    }

    /// Whether an edit was refused: the shell then asks the graph again, which puts the list right.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Meters, Ok(r)) => self.meters(&r),
            (Request::Status, Ok(r)) => self.status(&r),
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

    fn row(&self, at: usize) -> Option<&Row> {
        match self.entries.get(at) {
            Some(Entry::Row(r)) => Some(r),
            _ => None,
        }
    }

    fn row_mut(&mut self, at: usize) -> Option<&mut Row> {
        match self.entries.get_mut(at) {
            Some(Entry::Row(r)) => Some(r),
            _ => None,
        }
    }

    /// How many cells entry `at` has: the part, and for a part or bus its level and mute, and for a part a send per bus.
    fn cells(&self, at: usize) -> usize {
        match self.entries.get(at) {
            Some(Entry::Row(r)) if r.bus => 3,
            Some(Entry::Row(_)) => 3 + self.buses.len(),
            _ => 1,
        }
    }

    /// The failure that is shown, as the JUCE view's message line says it.
    fn message(&self) -> String {
        if !self.said.is_empty() {
            return self.said.clone();
        }
        self.failures.first().map(|(_, name, reason)| format!("{name} is silent: {reason}")).unwrap_or_default()
    }

    /// The starts of the views the entries are cut into, filling `area` with rows `pitch` apart (the
    /// first bus has its heading above it), at least one to a view.
    fn views(&self, area: f32, pitch: f32, heading: f32, slack: f32) -> Vec<usize> {
        let first_bus = self.entries.iter().position(|e| matches!(e, Entry::Row(r) if r.bus));
        let mut starts = vec![0];
        let mut used = 0.;
        for i in 0..self.entries.len() {
            let cost = pitch + if Some(i) == first_bus { heading } else { 0. };
            if used > 0. && used + cost > area + slack {
                starts.push(i);
                used = 0.;
            }
            used += cost;
        }
        starts
    }

    fn geometry(&self) -> (f32, f32, f32, f32) {
        let Some(ui) = self.ui.upgrade() else { return (0., 1., 0., 0.) };
        let s = ui.global::<PartsState>();
        (s.get_area_height(), s.get_row_pitch().max(1.), s.get_heading_pitch(), s.get_slack())
    }

    /// The window's view of the list: the view the cursor is in (else the one that was shown), each
    /// row set only when it changed.
    pub fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let (area, pitch, heading, slack) = self.geometry();
        let views = self.views(area, pitch, heading, slack);
        let view_of = |at: usize| views.iter().rposition(|s| *s <= at).unwrap_or(0);
        if self.stop.is_some_and(|s| matches!(s, Stop::Cell(r, _) if r >= self.entries.len())) {
            self.stop = None;
            self.editing = false;
        }
        let view = match self.stop {
            Some(Stop::Cell(row, _)) => view_of(row),
            _ => view_of(self.first),
        };
        self.first = views[view];
        let end = views.get(view + 1).copied().unwrap_or(self.entries.len());
        let first_bus = self.entries.iter().position(|e| matches!(e, Entry::Row(r) if r.bus));
        let mut rows = Vec::new();
        let mut chips = Vec::new();
        let mut sends = Vec::new();
        for i in self.first..end {
            let local = (i - self.first) as i32;
            let cell = match self.stop {
                Some(Stop::Cell(row, c)) if row == i => c as i32,
                _ => -1,
            };
            let editing = self.editing && cell >= 0;
            match &self.entries[i] {
                Entry::Row(r) => {
                    let given = self.given.get(&r.id).map(|(stolen, cut)| format!(" · {stolen} stolen{}", if *cut > 0 { format!(", {cut} cut") } else { String::new() })).unwrap_or_default();
                    let chain_text = if r.chain.is_empty() { "Empty: open it to add plugins".to_string() } else { r.chain.iter().map(|c| if c.off { format!("{} (off)", c.name) } else { c.name.clone() }).collect::<Vec<_>>().join(" › ") };
                    let failed = |c: &Chip| self.failures.iter().any(|f| f.0 == c.node && c.node != 0);
                    for (slot, c) in r.chain.iter().enumerate() {
                        chips.push(ChipData {
                            row: local,
                            slot: slot as i32,
                            count: r.chain.len() as i32,
                            name: c.name.as_str().into(),
                            role: match c.role {
                                Role::Other => 0,
                                Role::Instrument => 1,
                                Role::Effect => 2,
                            },
                            off: c.off,
                            failed: failed(c),
                        });
                    }
                    if !r.bus {
                        for (slot, level) in r.sends.iter().enumerate() {
                            sends.push(SendData { row: local, slot: slot as i32, level: level.unwrap_or(0.) as f32, cursor: cell == slot as i32 + 3, editing: editing && cell == slot as i32 + 3 });
                        }
                    }
                    rows.push(PartRowData {
                        kind: i32::from(r.bus),
                        heading: Some(i) == first_bus,
                        number: if r.bus { SharedString::new() } else { (i + 1).to_string().into() },
                        name: r.name.as_str().into(),
                        sound: if r.bus { "Shared bus" } else if r.sound.is_empty() { "No instrument yet" } else { r.sound.as_str() }.into(),
                        given: given.into(),
                        chain_text: chain_text.into(),
                        chain_empty: r.chain.is_empty(),
                        input: r.input.as_str().into(),
                        keys: r.notes,
                        key_low: r.keys.0 as f32 / 128.,
                        key_high: (r.keys.1 + 1) as f32 / 128.,
                        level: r.level.clamp(0., 1.) as f32,
                        mute: r.mute,
                        dim: r.mute || r.silenced,
                        draining: r.draining,
                        cell,
                        editing,
                        ..Default::default()
                    });
                }
                Entry::Shared(s) => rows.push(PartRowData {
                    kind: 2,
                    name: format!("Shared · {}", s.name).into(),
                    fed: format!("Fed by {} in Routing · open it", s.fed_by.join(", ")).into(),
                    cell,
                    ..Default::default()
                }),
            }
        }
        let state = ui.global::<PartsState>();
        update(&self.rows, rows);
        update(&self.chips, chips);
        update(&self.sends, sends);
        update(&self.bus_names, self.buses.iter().map(|b| SharedString::from(b.1.as_str())).collect());
        let tool_at = match self.stop {
            Some(Stop::Tool(t)) => Some(t),
            _ => None,
        };
        update(
            &self.tools,
            TOOLS
                .iter()
                .enumerate()
                .map(|(i, t)| ToolData {
                    label: match t {
                        Tool::Page(p) => p.name().into(),
                        Tool::AddPart => "+ Part".into(),
                        Tool::AddBus => "+ Bus".into(),
                    },
                    enabled: true,
                    cursor: tool_at == Some(i),
                })
                .collect(),
        );
        state.set_title(if self.title.is_empty() { "Parts".into() } else { self.title.as_str().into() });
        state.set_empty(self.entries.is_empty());
        state.set_message(self.message().into());
        state.set_view(if self.entries.is_empty() { String::new() } else { format!("Parts {}–{} of {}", self.first + 1, end, self.entries.len()) }.into());
    }

    /// The cursor's next place along the dial: cell by cell, row by row, then the buttons.
    fn dial(&self, by: i64) -> Option<Stop> {
        let mut stops: Vec<Stop> = Vec::new();
        for i in 0..self.entries.len() {
            stops.extend((0..self.cells(i)).map(|c| Stop::Cell(i, c)));
        }
        stops.extend((0..TOOLS.len()).map(Stop::Tool));
        let at = match self.stop {
            Some(s) => stops.iter().position(|x| *x == s)? as i64,
            None => return stops.get(self.first_row_stop(&stops)).copied(),
        };
        stops.get((at + by).clamp(0, stops.len() as i64 - 1) as usize).copied()
    }

    /// The first stop of the view shown, where the first move puts the cursor.
    fn first_row_stop(&self, stops: &[Stop]) -> usize {
        stops.iter().position(|s| matches!(s, Stop::Cell(r, 0) if *r >= self.first)).unwrap_or(0)
    }

    /// Along the cells of the cursor's row (or the buttons) by `by`.
    fn along(&mut self, by: i64) {
        match self.stop {
            None => self.stop = self.dial(0),
            Some(Stop::Cell(r, c)) => self.stop = Some(Stop::Cell(r, (c as i64 + by).clamp(0, self.cells(r) as i64 - 1) as usize)),
            Some(Stop::Tool(t)) => self.stop = Some(Stop::Tool((t as i64 + by).clamp(0, TOOLS.len() as i64 - 1) as usize)),
        }
    }

    /// Down the rows (the buttons after the last) by `by`, the cell kept where the row has it.
    fn down(&mut self, by: i64) {
        let rows = self.entries.len() as i64;
        match self.stop {
            None => self.stop = self.dial(0),
            Some(Stop::Cell(r, c)) => {
                let to = r as i64 + by;
                self.stop = if to >= rows {
                    (by > 0).then_some(Stop::Tool(0)).or(self.stop)
                } else {
                    let to = to.max(0) as usize;
                    Some(Stop::Cell(to, c.min(self.cells(to) - 1)))
                };
            }
            Some(Stop::Tool(t)) => {
                if by < 0 && rows > 0 {
                    self.stop = Some(Stop::Cell(rows as usize - 1, 0));
                } else {
                    self.stop = Some(Stop::Tool((t as i64 + by).clamp(0, TOOLS.len() as i64 - 1) as usize));
                }
            }
        }
    }

    /// The first cell of the first (-1) or last (1) row, or the last button.
    fn end(&mut self, by: i64) {
        self.stop = if by < 0 { (!self.entries.is_empty()).then_some(Stop::Cell(0, 0)) } else { Some(Stop::Tool(TOOLS.len() - 1)) };
    }

    /// The cursor is on the first cell of the first row: moving up leaves for the top bar's actions.
    pub fn at_top(&self) -> bool {
        self.stop == Some(Stop::Cell(0, 0)) && !self.editing
    }

    fn set_level(&mut self, at: usize, level: f64) {
        let Some(r) = self.row_mut(at) else { return };
        let level = (level.clamp(0., 1.) * 1000.).round() / 1000.;
        if (level - r.level).abs() < f64::EPSILON {
            return;
        }
        r.level = level;
        let (id, mute) = (r.id, r.mute);
        self.edit("parts.level", json!({"part": id, "level": level, "mute": mute}));
    }

    fn toggle_mute(&mut self, at: usize) {
        let Some(r) = self.row_mut(at) else { return };
        r.mute = !r.mute;
        let (id, level, mute) = (r.id, r.level, r.mute);
        self.edit("parts.level", json!({"part": id, "level": level, "mute": mute}));
    }

    /// A send's new level; none left (0) takes the send away, as the JUCE setSendLevel.
    fn set_send(&mut self, at: usize, slot: usize, level: f64) {
        let Some(bus) = self.buses.get(slot).map(|b| b.0) else { return };
        let Some(r) = self.row_mut(at) else { return };
        let level = (level.clamp(0., 1.) * 1000.).round() / 1000.;
        let was = r.sends.get(slot).copied().flatten().unwrap_or(0.);
        if (level - was).abs() < f64::EPSILON {
            return;
        }
        if let Some(s) = r.sends.get_mut(slot) {
            *s = (level > 0.).then_some(level);
        }
        let id = r.id;
        let params = if level > 0. { json!({"part": id, "bus": bus, "level": level}) } else { json!({"part": id, "bus": bus}) };
        self.edit("parts.send", params);
    }

    fn add(&mut self, bus: bool) {
        let parts = self.entries.iter().filter(|e| matches!(e, Entry::Row(r) if !r.bus)).count();
        let buses = self.buses.len();
        let name = if bus { format!("Bus {}", char::from(b'A' + (buses % 26) as u8)) } else { format!("Part {}", parts + 1) };
        let input = json!({"kind": if bus { "bus" } else { "notes" }, "port": 0, "zones": [], "devices": [], "maxNotes": 0, "reserve": 0, "priority": 0, "receive": {}});
        self.edit("parts.add", json!({"name": name, "input": input}));
    }

    fn press_tool(&mut self, t: usize) -> Outcome {
        match TOOLS.get(t) {
            Some(Tool::AddPart) => self.add(false),
            Some(Tool::AddBus) => self.add(true),
            Some(Tool::Page(p)) => return Outcome::Page(*p),
            _ => {}
        }
        Outcome::Stay
    }

    /// What a press on `stop` does.
    fn press(&mut self, stop: Stop) -> Outcome {
        match stop {
            Stop::Tool(t) => return self.press_tool(t),
            Stop::Cell(at, cell) => match (&self.entries[at], cell) {
                (Entry::Shared(s), _) => return Outcome::Routing(s.node),
                (Entry::Row(r), 0) => return Outcome::Open(r.id),
                (Entry::Row(_), 2) => self.toggle_mute(at),
                _ => self.editing = !self.editing, // a press, then the dial
            },
        }
        Outcome::Stay
    }

    /// The dial on a level or a send being changed.
    fn step_value(&mut self, by: i64) {
        let Some(Stop::Cell(at, cell)) = self.stop else { return };
        match cell {
            1 => {
                let was = self.row(at).map_or(0., |r| r.level);
                self.set_level(at, stepped(was, by));
            }
            c if c >= 3 => {
                let was = self.row(at).and_then(|r| r.sends.get(c - 3).copied().flatten()).unwrap_or(0.);
                self.set_send(at, c - 3, stepped(was, by));
            }
            _ => {}
        }
    }

    /// A navigation action. The dial and the keys' up and down move through the list, the left knob
    /// and the keys' left and right along a row, the right knob down the rows.
    pub fn act(&mut self, action: Action) -> Outcome {
        self.said.clear();
        match action {
            Action::Move { pane: None, by } => {
                if self.editing {
                    self.step_value(by as i64);
                } else {
                    self.stop = self.dial(by as i64).or(self.stop);
                }
            }
            Action::Move { pane: Some(0), by } => {
                if self.editing {
                    self.step_value(by as i64);
                } else {
                    self.along(by as i64);
                }
            }
            Action::Move { pane: Some(_), by } => {
                if !self.editing {
                    self.down(by as i64);
                }
            }
            Action::Arrow { dx: 0, dy } => {
                if !self.editing {
                    self.down(dy as i64);
                }
            }
            Action::Arrow { dx, .. } => {
                if self.editing {
                    self.step_value(dx as i64);
                } else {
                    self.along(dx as i64);
                }
            }
            Action::Page(by) => {
                if !self.editing {
                    self.turn(by);
                }
            }
            Action::End(by) => {
                if !self.editing {
                    self.end(by as i64);
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

    /// A view towards the start (-1) or the end (1): the cursor, if there is one, on its first row.
    fn turn(&mut self, by: i32) {
        let (area, pitch, heading, slack) = self.geometry();
        let views = self.views(area, pitch, heading, slack);
        let now = views.iter().rposition(|s| *s <= self.first).unwrap_or(0) as i64;
        let to = (now + i64::from(by)).clamp(0, views.len() as i64 - 1) as usize;
        self.first = views[to];
        if let Some(Stop::Cell(_, c)) = self.stop {
            if self.first < self.entries.len() {
                self.stop = Some(Stop::Cell(self.first, c.min(self.cells(self.first) - 1)));
            }
        }
    }

    /// The entry of the view's row `row`.
    fn at(&self, row: i32) -> usize {
        self.first + row.max(0) as usize
    }

    /// A tap on the part's words: that part's screen; on a shared row, Routing.
    pub fn tap_open(&mut self, row: i32) -> Outcome {
        self.stop = None;
        self.editing = false;
        let outcome = match self.entries.get(self.at(row)) {
            Some(Entry::Row(r)) => Outcome::Open(r.id),
            Some(Entry::Shared(s)) => Outcome::Routing(s.node),
            None => Outcome::Stay,
        };
        self.show();
        outcome
    }

    pub fn tap_mute(&mut self, row: i32) {
        self.stop = None;
        self.editing = false;
        self.toggle_mute(self.at(row));
        self.show();
    }

    /// A tap on one of the buttons under the list.
    pub fn tap_tool(&mut self, i: i32) -> Outcome {
        self.stop = None;
        self.editing = false;
        self.said.clear();
        let outcome = self.press_tool(i.max(0) as usize);
        self.show();
        outcome
    }

    /// The level slider of the view's row `row` moved to `place` of its length.
    pub fn level(&mut self, row: i32, place: f32) {
        self.stop = None;
        self.editing = false;
        self.set_level(self.at(row), f64::from(place));
        self.show();
    }

    /// The slider of send `slot` of the view's row `row` moved.
    pub fn send(&mut self, row: i32, slot: i32, place: f32) {
        self.stop = None;
        self.editing = false;
        self.set_send(self.at(row), slot.max(0) as usize, f64::from(place));
        self.show();
    }

    /// A finger on a slider (`cell`: 1 the level, 3 and on the sends) or off it.
    pub fn hold(&mut self, row: i32, cell: i32, down: bool) {
        self.held = if down { self.row(self.at(row)).map(|r| (r.id, cell.max(0) as usize)) } else { None };
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
    use serde_json::json;

    /// The role a node's pins give it, as PartsModel.cpp's roleOf names it: notes and
    /// sequencers from note pins, instruments and generators from audio out, effects from
    /// audio in and out, analyzers from audio in alone. A node neither names is "Notes".
    #[test]
    fn role_of_names_a_node_by_its_pins() {
        let instrument = json!({"audioInputs": [], "audioOutputs": [{"channels": 2}], "noteInputs": 1, "noteOutputs": 0});
        assert_eq!(role_of(&instrument), "Instrument");
        let generator = json!({"audioOutputs": [{"channels": 2}]});
        assert_eq!(role_of(&generator), "Generator");
        let effect = json!({"audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}]});
        assert_eq!(role_of(&effect), "Effect");
        let analyzer = json!({"audioInputs": [{"channels": 2}]});
        assert_eq!(role_of(&analyzer), "Analyzer");
        let sequencer = json!({"noteOutputs": 1});
        assert_eq!(role_of(&sequencer), "Sequencer");
        let notes = json!({"noteInputs": 1, "noteOutputs": 1});
        assert_eq!(role_of(&notes), "Notes");
    }

    /// A node missing its ports, or a present array whose element carries no channels, reads as one
    /// without audio in or out, and so as "Notes": a muted silence never draws as an effect.
    #[test]
    fn role_of_defaults_a_node_without_ports_to_notes() {
        assert_eq!(role_of(&json!({})), "Notes");
        let portless = json!({"audioInputs": [{}], "audioOutputs": [{}]});
        assert_eq!(role_of(&portless), "Notes");
    }

    /// A node whose note output sits beside an audio output is a generator, not a sequencer: the
    /// audio-out branch is earlier than the note-out one, so the pin's audio is what it is drawn as.
    #[test]
    fn role_of_an_audio_out_beats_a_note_out() {
        let node = json!({"noteOutputs": 1, "audioOutputs": [{"channels": 2}]});
        assert_eq!(role_of(&node), "Generator");
    }

    /// A split's lane is named by its instrument when it has one, else its last box, else by the
    /// split's shape: Left/Right, Mid/Side, "Band N", and "Dry" for a plain copy.
    #[test]
    fn lane_name_prefers_an_instrument_then_a_last_box_then_the_shape() {
        let named = Chip { node: 1, name: "Rhodes".into(), role: Role::Instrument, off: false };
        assert_eq!(lane_name("leftRight", 0, std::slice::from_ref(&named)), "Rhodes", "an instrument names the split over the shape");
        let last = Chip { node: 2, name: "Chorus".into(), role: Role::Effect, off: false };
        assert_eq!(lane_name("bands", 3, &[last]), "Chorus", "the last box names it when no instrument is in it");
    }

    /// With no box to take a name from, a lane keeps the split's own words.
    #[test]
    fn lane_name_falls_back_to_the_shape_and_dry() {
        assert_eq!(lane_name("leftRight", 0, &[]), "Left");
        assert_eq!(lane_name("leftRight", 1, &[]), "Right");
        assert_eq!(lane_name("leftRight", 9, &[]), "Right");
        assert_eq!(lane_name("midSide", 0, &[]), "Mid");
        assert_eq!(lane_name("midSide", 1, &[]), "Side");
        assert_eq!(lane_name("bands", 0, &[]), "Band 1");
        assert_eq!(lane_name("bands", 4, &[]), "Band 5");
        assert_eq!(lane_name("copy", 0, &[]), "Dry");
        assert_eq!(lane_name("", 0, &[]), "Dry");
    }

    /// A lane's instrument name is kept whole, whatever it is written in.
    #[test]
    fn lane_name_keeps_an_instruments_own_name() {
        let named = Chip { node: 1, name: "カシオ · Rhodes".into(), role: Role::Instrument, off: false };
        assert_eq!(lane_name("bands", 0, &[named]), "カシオ · Rhodes");
    }

    /// boxes maps each chain entry to its node's chip: an effect keeps its enabled-off mark and its
    /// role's class; a node the graph does not name is left out.
    #[test]
    fn boxes_maps_chips_from_the_nodes_and_skips_a_plain_id() {
        let effect = json!({"id": 5, "name": "Chorus", "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "enabled": false});
        let nodes: HashMap<u64, &Value> = [(5u64, &effect)].into_iter().collect();
        let chips = boxes(&json!([5, 99]), &json!({"splits": []}), &nodes, 0);
        assert_eq!(chips.len(), 1);
        assert_eq!((chips[0].node, chips[0].name.as_str()), (5, "Chorus"));
        assert_eq!(chips[0].role, Role::Effect);
        assert!(chips[0].off, "a disabled node is drawn off");
    }

    #[test]
    fn boxes_skips_entries_that_are_neither_a_node_nor_a_split() {
        assert!(boxes(&json!([42]), &json!({"splits": []}), &HashMap::new(), 0).is_empty());
        assert!(boxes(&json!([{"split": 7}]), &json!({"splits": []}), &HashMap::new(), 0).is_empty(), "a split the part has not is left out");
        assert!(boxes(&json!(["name"]), &json!({"splits": []}), &HashMap::new(), 0).is_empty());
    }

    /// A split is one chip named after its lanes, in order; the whole leftRight fixture is covered by
    /// the screen test, this one holds the empty lane's word.
    #[test]
    fn boxes_names_a_split_after_its_lanes() {
        let bass = json!({"id": 1, "name": "Bass", "audioInputs": [], "audioOutputs": [{"channels": 1}], "noteInputs": 1, "noteOutputs": 0});
        let nodes: HashMap<u64, &Value> = [(1u64, &bass)].into_iter().collect();
        let part = json!({"splits": [{"id": 9, "kind": "leftRight", "lanes": [{"chain": [1]}, {"chain": []}]}]});
        let chips = boxes(&json!([{"split": 9}]), &part, &nodes, 0);
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].name, "Bass | Right");
        assert_eq!(chips[0].role, Role::Other);
    }

    /// A part whose devices rules find nothing connected keeps its built-in keys: an empty or absent
    /// rule list is "found" before any rule is read.
    #[test]
    fn devices_found_with_no_rules_is_true() {
        assert!(devices_found(&json!([]), &[]));
        assert!(devices_found(&Value::Null, &[]), "an absent rule list is as found as an empty one");
    }

    #[test]
    fn devices_found_reads_each_rule_kind() {
        let usb = [json!({"kind": "usb", "name": "KeyLab 61", "port": 0}), json!({"kind": "builtIn", "name": "Keys", "port": 0})];
        let rule = |k: serde_json::Value| json!([k]);
        assert!(devices_found(&rule(json!({"kind": "all"})), &usb));
        assert!(devices_found(&rule(json!({"kind": "builtIn"})), &usb));
        assert!(devices_found(&rule(json!({"kind": "usb"})), &usb), "a usb rule finds a usb input");
        assert!(!devices_found(&rule(json!({"kind": "usb"})), &[json!({"kind": "port", "port": 0})]), "a usb rule alone does not find a port");
        assert!(devices_found(&rule(json!({"kind": "usbNamed", "pattern": "keylab 61"})), &usb), "a usbNamed rule matches by name, heedless of case");
        assert!(!devices_found(&rule(json!({"kind": "usbNamed", "pattern": "other"})), &usb));
        assert!(devices_found(&rule(json!({"kind": "port", "port": 0})), &[json!({"kind": "port", "port": 0})]));
        assert!(!devices_found(&rule(json!({"kind": "port", "port": 1})), &[json!({"kind": "port", "port": 0})]));
        assert!(devices_found(&rule(json!({"kind": "external"})), &[json!({"kind": "usb"})]));
        assert!(!devices_found(&rule(json!({"kind": "external"})), &[json!({"kind": "builtIn"})]));
        assert!(devices_found(&rule(json!({"kind": "network"})), &[json!({"kind": "network", "name": "LAN"})]), "a network rule with no pattern is found");
        assert!(devices_found(&rule(json!({"kind": "network", "pattern": "lan"})), &[json!({"kind": "network", "name": "LAN"})]));
        assert!(!devices_found(&rule(json!({"kind": "network", "pattern": "x"})), &[json!({"kind": "network", "name": "LAN"})]));
        assert!(!devices_found(&rule(json!({"kind": "unknown"})), &usb));
    }

    #[test]
    fn devices_found_one_rule_suffices_and_two_fail_leave_it_unfound() {
        let usb = [json!({"kind": "usb", "name": "KeyLab", "port": 0})];
        assert!(devices_found(&json!([{"kind": "usb"}, {"kind": "port", "port": 2}]), &usb));
        assert!(!devices_found(&json!([{"kind": "port", "port": 1}]), &usb));
    }

    /// A level is stepped by a fortieth of the slider a click, and held at the ends.
    #[test]
    fn stepped_moves_in_fortieths_and_is_held_at_the_ends() {
        assert_eq!(stepped(0.5, 2), 0.55);
        assert_eq!(stepped(0.5, -2), 0.45);
        assert_eq!(stepped(0., 0), 0.);
        assert_eq!(stepped(0., -1000), 0., "held at the bottom");
        assert_eq!(stepped(1., 1000), 1., "held at the top");
        assert_eq!(stepped(0.9, 1), 0.925, "a click off the top holds inside the slider");
    }

    #[test]
    fn stepped_rounds_to_a_thousandth() {
        assert_eq!(stepped(0.123, 1), 0.148, "0.123 + one click");
    }
}
