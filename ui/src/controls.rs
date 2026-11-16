// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Controls page: the controls page of the JUCE Edit view (hosts/standalone/PartsView.cpp,
//! `controlsShown`), on the shell. A control is a knob (0 to 1) that drives any number of targets: a
//! plugin's parameter, a part's level, mute or send, another control, or a morph between two scenes,
//! each with its own range (a start and an end), curve and Relative; a row per control (its name, a
//! fader, + Target, Remove) and under it a row per target (what it drives, the range's two sliders, the
//! curve, Relative, Now..., Scenes..., Remove). By scene shows the table of every mapping against
//! every scene, ticked where it works.
//!
//! Without touch the cursor has two dimensions as the Parts list's: the dial goes cell by cell, row by
//! row, then through the buttons under the list (By scene, + Control); the left knob moves along a
//! row, the right knob down the rows. A press on a name renames it (the text keyboard), on a fader or
//! a slider starts a change the dial then makes (a fortieth of the slider a click; a press or Cancel
//! ends it), on a button or Relative does it, on the curve, Now..., Scenes... or + Target opens a menu
//! (menu.rs) the dial and the knobs move through, and a press chooses. Cancel ends a change, drops the
//! cursor, then goes back to the Parts list. Touch: sliders drag, everything else taps.
//!
//! How many rows a view holds, and how many lines a target takes, is whatever fills the width and
//! height (the component says), so it follows the window, the spacing and the type scale.
//!
//! Requests (none new): `graph.get` (asked by the shell: controls, parts, scenes, nodes), `controls.set`
//! (a control's value, live), `controls.add`, `controls.name`, `controls.remove`, `controls.targets` (a
//! control's whole list again, as the JUCE `setTargets` sends it), `params.list` for the names of the
//! parameters targets drive and the menu of them, `params.get` for Now (the target's value now); the
//! JUCE page sends no `graph.commit` after any of these, and neither does this.

use crate::menu::Item;
use crate::nav::Action;
use crate::setlist::update;
use crate::{ControlRowData, ControlToolData, ControlsState, Shell, TableCellData, TableRowData};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Any edit: its refusal is said.
    Edit,
    /// params.list of a node.
    List(u64),
    /// params.get of the parameter a Now... works from: the control, the target and which end.
    Value(u32, usize, bool),
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Back to the Parts list.
    Back,
    /// The text keyboard for a control's name: its title and the name now.
    Prompt(String, String),
}

/// What a target drives, as `controls.targets` names it.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Parameter(u64, u64),
    PartLevel(u32),
    PartMute(u32),
    SendLevel(u32, u32),
    Control(u32),
    Morph(u32, u32),
}

impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Kind::Parameter(..) => "parameter",
            Kind::PartLevel(_) => "partLevel",
            Kind::PartMute(_) => "partMute",
            Kind::SendLevel(..) => "sendLevel",
            Kind::Control(_) => "control",
            Kind::Morph(..) => "morph",
        }
    }

    /// The kinds whose value the summing rule adds up (`MacroTarget::sums`).
    fn sums(&self) -> bool {
        !matches!(self, Kind::Control(_) | Kind::Morph(..))
    }
}

/// The names the JUCE combo gives the curves, in `MacroTarget::Curve`'s order.
pub const CURVES: [&str; 6] = ["Linear", "Exponential", "Logarithmic", "S-curve", "Fader (dB)", "Power"];
const CURVE_KINDS: [&str; 4] = ["linear", "exponential", "logarithmic", "s-curve"];

/// A target's curve: the kind (an index into CURVES) and the numbers the dB and power curves have, kept
/// whatever the kind so choosing a curve leaves them as they were (`MacroTarget`'s `low`, `high`, `exponent`).
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub kind: usize,
    pub low: f64,
    pub high: f64,
    pub exponent: f64,
}

impl Default for Curve {
    fn default() -> Curve {
        Curve { kind: 0, low: -96., high: 0., exponent: 2. }
    }
}

impl Curve {
    fn read(v: &Value) -> Curve {
        let mut c = Curve::default();
        match v {
            Value::String(s) => c.kind = CURVE_KINDS.iter().position(|k| k == s).unwrap_or(0),
            Value::Object(_) => match v["kind"].as_str() {
                Some("db") => {
                    c.kind = 4;
                    c.low = v["min"].as_f64().unwrap_or(c.low);
                    c.high = v["max"].as_f64().unwrap_or(c.high);
                }
                Some("power") => {
                    c.kind = 5;
                    c.exponent = v["exponent"].as_f64().unwrap_or(c.exponent);
                }
                _ => {}
            },
            _ => {}
        }
        c
    }

    /// The curve as `controls.targets` takes it: a name, or an object with its numbers.
    fn json(&self) -> Value {
        match self.kind {
            4 => json!({"kind": "db", "min": self.low, "max": self.high}),
            5 => json!({"kind": "power", "exponent": self.exponent}),
            k => json!(CURVE_KINDS[k.min(3)]),
        }
    }

    /// The curve's shape, 0 to 1 to 0 to 1 (`Graph::ControlCurve::shape`).
    pub fn shape(&self, value: f64) -> f64 {
        let x = value.clamp(0., 1.);
        match self.kind {
            1 => ((4. * x).exp() - 1.) / (4f64.exp() - 1.),
            2 => ((4f64.exp() - 1.) * x).ln_1p() / 4.,
            3 => x * x * (3. - 2. * x),
            4 => {
                if x > 0. {
                    ((self.high - self.low).mul_add(x, self.low) / 20. * std::f64::consts::LN_10).exp()
                } else {
                    0.
                }
            }
            5 => x.powf(self.exponent),
            _ => x,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub kind: Kind,
    pub start: f64,
    pub end: f64,
    pub curve: Curve,
    pub relative: bool,
    pub base: f64,
    pub off_in: Vec<u32>,
}

impl Target {
    fn new(kind: Kind) -> Target {
        Target { kind, start: 0., end: 1., curve: Curve::default(), relative: false, base: 0., off_in: vec![] }
    }

    pub(crate) fn read(t: &Value) -> Target {
        let id = |k: &str| t[k].as_u64().unwrap_or(0);
        let kind = match t["kind"].as_str() {
            Some("partLevel") => Kind::PartLevel(id("part") as u32),
            Some("partMute") => Kind::PartMute(id("part") as u32),
            Some("sendLevel") => Kind::SendLevel(id("part") as u32, id("bus") as u32),
            Some("morph") => Kind::Morph(id("from") as u32, id("to") as u32),
            Some("control") => Kind::Control(id("control") as u32),
            _ => Kind::Parameter(id("node"), id("param")),
        };
        Target {
            kind,
            start: t["start"].as_f64().unwrap_or(0.),
            end: t["end"].as_f64().unwrap_or(1.),
            curve: Curve::read(&t["curve"]),
            relative: t["relative"].as_bool().unwrap_or(false),
            base: t["base"].as_f64().unwrap_or(0.),
            off_in: t["offIn"].as_array().map(|a| a.iter().filter_map(|s| s.as_u64().map(|s| s as u32)).collect()).unwrap_or_default(),
        }
    }

    /// The target as `RemoteControl::setMacroTargets` sends it: every field, the ones its kind does not use zero.
    pub(crate) fn json(&self) -> Value {
        let (mut node, mut param, mut part, mut bus, mut control, mut from, mut to) = (0, 0, 0, 0, 0, 0, 0);
        match self.kind {
            Kind::Parameter(n, p) => (node, param) = (n, p),
            Kind::PartLevel(p) | Kind::PartMute(p) => part = u64::from(p),
            Kind::SendLevel(p, b) => (part, bus) = (u64::from(p), u64::from(b)),
            Kind::Control(c) => control = u64::from(c),
            Kind::Morph(a, b) => (from, to) = (u64::from(a), u64::from(b)),
        }
        json!({"kind": self.kind.name(), "node": node, "param": param, "part": part, "bus": bus, "control": control, "from": from, "to": to,
               "start": self.start, "end": self.end, "curve": self.curve.json(), "relative": self.relative, "offIn": self.off_in})
    }

    /// Its point in the range, start at 0 and end at 1 (`Graph::ControlCurve::point`).
    fn point(&self, value: f64) -> f64 {
        (self.end - self.start).mul_add(self.curve.shape(value), self.start)
    }

    fn same_destination(&self, o: &Target) -> bool {
        self.kind.sums() && self.kind == o.kind
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub id: u32,
    pub name: String,
    pub value: f64,
    pub targets: Vec<Target>,
}

/// A part or bus as the target descriptions and menus need it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartInfo {
    pub id: u32,
    pub name: String,
    pub bus: bool,
    pub level: f64,
    pub mute: bool,
    /// The sends: bus and level, in the part's order.
    pub sends: Vec<(u32, f64)>,
}

/// One parameter of a plugin as `params.list` gives it, what the names, the menu and Now... need.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Param {
    pub id: u64,
    pub name: String,
    pub flags: u64,
    pub min: f64,
    pub max: f64,
    pub stepped: bool,
}

const READONLY: u64 = 1 << 3;
const MODULATABLE: u64 = 1 << 10;

/// graph.get's controls, parts and buses, scenes and nodes, as the JUCE page reads them.
pub fn read(graph: &Value) -> (Vec<Control>, Vec<PartInfo>) {
    let controls = graph["controls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| Control {
            id: m["id"].as_u64().unwrap_or(0) as u32,
            name: m["name"].as_str().unwrap_or_default().into(),
            value: m["value"].as_f64().unwrap_or(0.),
            targets: m["targets"].as_array().into_iter().flatten().map(Target::read).collect(),
        })
        .collect();
    let parts = graph["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| PartInfo {
            id: p["id"].as_u64().unwrap_or(0) as u32,
            name: p["name"].as_str().unwrap_or_default().into(),
            bus: p["input"]["kind"] == "bus",
            level: p["level"].as_f64().unwrap_or(1.),
            mute: p["mute"].as_bool().unwrap_or(false),
            sends: p["sends"].as_array().into_iter().flatten().filter_map(|s| Some((s["bus"].as_u64()? as u32, s["level"].as_f64().unwrap_or(0.)))).collect(),
        })
        .collect();
    (controls, parts)
}

/// A level by `by` clicks of the dial, a fortieth of the slider each, to a thousandth, held at the ends.
fn stepped(was: f64, by: i64) -> f64 {
    ((was + 0.025 * by as f64).clamp(0., 1.) * 1000.).round() / 1000.
}

fn thousandths(v: f64) -> f64 {
    (v * 1000.).round() / 1000.
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Control(usize),
    Target(usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    /// A cell of an entry (in the table: a column of a mapping's row).
    Cell(usize, usize),
    Tool(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    ByScene,
    AddControl,
}

/// What a choice of the menu that is open means.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    Add(u32, Kind),
    Curve(u32, usize, usize),
    Scene(u32, usize, u32),
    Present(u32, usize, bool),
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    controls: Vec<Control>,
    parts: Vec<PartInfo>,
    scenes: Vec<(u32, String)>,
    /// Node names in graph order, and what each node's parameter list was when it was read.
    nodes: Vec<(u64, String)>,
    revisions: HashMap<u64, u64>,
    /// params.list of each node read so far, and the nodes asked for and not yet answered.
    params: HashMap<u64, Vec<Param>>,
    asked: Vec<u64>,
    by_scene: bool,
    stop: Option<Stop>,
    /// The dial changes the slider under the cursor.
    editing: bool,
    /// The first entry of the view shown; the first scene column.
    first: usize,
    col_first: usize,
    said: String,
    /// The slider a finger is on: the control, its target (None the fader) and the cell. The engine's own values for it wait.
    held: Option<(u32, Option<usize>, usize)>,
    /// The control being renamed.
    renaming: Option<u32>,
    picks: Vec<Pick>,
    /// A menu for the shell to open once the parameters it lists are read: its control.
    waiting_menu: Option<u32>,
    ready_menu: Option<(String, Vec<Item>)>,
    /// A Now... waiting for the parameter's list or value: the control, the target, which end.
    waiting_now: Option<(u32, usize, bool)>,
    rows: Rc<VecModel<ControlRowData>>,
    table_rows: Rc<VecModel<TableRowData>>,
    table_cells: Rc<VecModel<TableCellData>>,
    scene_names: Rc<VecModel<SharedString>>,
    tools: Rc<VecModel<ControlToolData>>,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(window: &Shell) -> Screen {
        let s = Screen {
            ui: window.as_weak(),
            controls: vec![],
            parts: vec![],
            scenes: vec![],
            nodes: vec![],
            revisions: HashMap::new(),
            params: HashMap::new(),
            asked: vec![],
            by_scene: false,
            stop: None,
            editing: false,
            first: 0,
            col_first: 0,
            said: String::new(),
            held: None,
            renaming: None,
            picks: vec![],
            waiting_menu: None,
            ready_menu: None,
            waiting_now: None,
            rows: Rc::new(VecModel::default()),
            table_rows: Rc::new(VecModel::default()),
            table_cells: Rc::new(VecModel::default()),
            scene_names: Rc::new(VecModel::default()),
            tools: Rc::new(VecModel::default()),
            out: vec![],
        };
        let state = window.global::<ControlsState>();
        state.set_rows(ModelRc::from(s.rows.clone()));
        state.set_table_rows(ModelRc::from(s.table_rows.clone()));
        state.set_table_cells(ModelRc::from(s.table_cells.clone()));
        state.set_scene_names(ModelRc::from(s.scene_names.clone()));
        state.set_tools(ModelRc::from(s.tools.clone()));
        s
    }

    /// graph.get: the controls again, the cursor kept where it is (a slider with a finger on it keeps its own value).
    pub fn graph(&mut self, graph: &Value) {
        let (mut controls, parts) = read(graph);
        if let Some((id, target, cell)) = self.held {
            if let (Some(now), Some(was)) = (controls.iter_mut().find(|c| c.id == id), self.controls.iter().find(|c| c.id == id)) {
                match target {
                    None => now.value = was.value,
                    Some(t) => {
                        if let (Some(n), Some(w)) = (now.targets.get_mut(t), was.targets.get(t)) {
                            *(if cell == 1 { &mut n.end } else { &mut n.start }) = if cell == 1 { w.end } else { w.start };
                        }
                    }
                }
            }
        }
        self.controls = controls;
        self.parts = parts;
        self.scenes = graph["scenes"].as_array().into_iter().flatten().map(|s| (s["id"].as_u64().unwrap_or(0) as u32, s["name"].as_str().unwrap_or_default().to_string())).collect();
        self.nodes = graph["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_u64()?, n["name"].as_str().unwrap_or_default().to_string()))).collect();
        // A plugin whose parameters changed is read again.
        for n in graph["nodes"].as_array().into_iter().flatten() {
            let (Some(id), Some(rev)) = (n["id"].as_u64(), n["parameterRevision"].as_u64()) else { continue };
            if self.revisions.get(&id).is_some_and(|r| *r != rev) {
                self.params.remove(&id);
            }
            self.revisions.insert(id, rev);
        }
        self.params.retain(|n, _| self.nodes.iter().any(|x| x.0 == *n));
        self.want_names();
        self.show();
    }

    /// controls.changed: the controls' values and the parts' levels and mutes moved live.
    pub fn changed(&mut self, params: &Value) {
        for c in params["controls"].as_array().into_iter().flatten() {
            let Some(id) = c["control"].as_u64() else { continue };
            if let Some(m) = self.controls.iter_mut().find(|m| u64::from(m.id) == id) {
                if self.held != Some((m.id, None, 1)) {
                    m.value = c["value"].as_f64().unwrap_or(m.value);
                }
            }
        }
        for c in params["parts"].as_array().into_iter().flatten() {
            let Some(id) = c["part"].as_u64() else { continue };
            if let Some(p) = self.parts.iter_mut().find(|p| u64::from(p.id) == id) {
                p.level = c["level"].as_f64().unwrap_or(p.level);
                p.mute = c["mute"].as_bool().unwrap_or(p.mute);
            }
        }
        self.show();
    }

    /// The nodes the targets' descriptions need names of parameters for, asked once each.
    fn want_names(&mut self) {
        let mut nodes: Vec<u64> = vec![];
        for c in &self.controls {
            for t in &c.targets {
                if let Kind::Parameter(n, _) = t.kind {
                    if !nodes.contains(&n) {
                        nodes.push(n);
                    }
                }
            }
        }
        for n in nodes {
            self.ask(n);
        }
    }

    /// params.list of `node`, unless it is read or being read.
    fn ask(&mut self, node: u64) {
        if self.params.contains_key(&node) || self.asked.contains(&node) || !self.nodes.iter().any(|n| n.0 == node) {
            return;
        }
        self.asked.push(node);
        self.out.push((Request::List(node), "params.list", json!({"node": node})));
    }

    /// Whether an edit was refused: the shell then asks the graph again, which puts the page right.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::List(node), Ok(r)) => {
                self.asked.retain(|n| *n != node);
                let list = r["parameters"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| Param {
                        id: p["id"].as_u64().unwrap_or(0),
                        name: p["name"].as_str().unwrap_or_default().into(),
                        flags: p["flags"].as_u64().unwrap_or(0),
                        min: p["minimum"].as_f64().unwrap_or(0.),
                        max: p["maximum"].as_f64().unwrap_or(1.),
                        stepped: p["stepped"].as_bool().unwrap_or(false),
                    })
                    .collect();
                self.params.insert(node, list);
                if let Some(c) = self.waiting_menu.filter(|_| self.asked.is_empty()) {
                    self.waiting_menu = None;
                    self.said.clear();
                    self.ready_menu = Some(self.target_menu(c));
                }
                if let Some((c, t, end)) = self.waiting_now.filter(|_| self.asked.is_empty()) {
                    self.waiting_now = None;
                    self.present(c, t, end);
                }
                self.show();
            }
            (Request::List(node), Err(_)) => {
                // Refused (no such node now): treated as having no parameters.
                self.asked.retain(|n| *n != node);
                self.params.insert(node, vec![]);
                if self.asked.is_empty() {
                    if let Some(c) = self.waiting_menu.take() {
                        self.ready_menu = Some(self.target_menu(c));
                    }
                }
                self.show();
            }
            (Request::Value(c, t, end), Ok(r)) => {
                let value = r["values"].as_array().and_then(|v| v.first()).and_then(|v| v["value"].as_f64());
                if let Some(value) = value {
                    self.set_present(c, t, end, Some(value));
                }
            }
            (Request::Edit | Request::Value(..), Err(e)) if e.code != 0 => {
                self.said = format!("The engine refused: {}", e.message);
                self.show();
                return true;
            }
            _ => {}
        }
        false
    }

    /// A menu that was waiting for parameters, now ready: the shell opens it.
    pub fn take_menu(&mut self) -> Option<(String, Vec<Item>)> {
        self.ready_menu.take()
    }

    /// Edits are sent as the JUCE page sends them: no commit after.
    fn edit(&mut self, method: &'static str, params: Value) {
        self.said.clear();
        self.out.push((Request::Edit, method, params));
    }

    fn control(&self, at: usize) -> Option<&Control> {
        self.controls.get(at)
    }

    fn part_name(&self, id: u32) -> String {
        self.parts.iter().find(|p| p.id == id).map_or("a part".to_string(), |p| p.name.clone())
    }

    fn scene_name(&self, id: u32) -> String {
        self.scenes.iter().find(|s| s.0 == id).map_or("a scene".to_string(), |s| s.1.clone())
    }

    /// What a target drives, in the JUCE words (`PartsView::describe`).
    pub fn describe(&self, t: &Target) -> String {
        match &t.kind {
            Kind::PartLevel(p) | Kind::PartMute(p) => match self.parts.iter().find(|x| x.id == *p) {
                Some(x) => format!("{} {}", x.name, if matches!(t.kind, Kind::PartLevel(_)) { "level" } else { "mute" }),
                None => "a part".into(),
            },
            Kind::SendLevel(p, b) => format!("{} send to {}", self.part_name(*p), self.part_name(*b)),
            Kind::Control(c) => match self.controls.iter().find(|x| x.id == *c) {
                Some(x) => format!("→ {}", x.name),
                None => "a control".into(),
            },
            Kind::Parameter(n, p) => {
                let node = self.nodes.iter().find(|x| x.0 == *n).map_or("a node".to_string(), |x| x.1.clone());
                let param = self.params.get(n).and_then(|l| l.iter().find(|x| x.id == *p)).map_or(p.to_string(), |x| x.name.clone());
                format!("{node} · {param}")
            }
            Kind::Morph(a, b) => format!("Morph {} → {}", self.scene_name(*a), self.scene_name(*b)),
        }
    }

    fn entries(&self) -> Vec<Entry> {
        let mut v = vec![];
        for (i, c) in self.controls.iter().enumerate() {
            v.push(Entry::Control(i));
            v.extend((0..c.targets.len()).map(|t| Entry::Target(i, t)));
        }
        v
    }

    /// The rows of By scene: a row per mapping.
    fn mappings(&self) -> Vec<(usize, usize)> {
        self.controls.iter().enumerate().flat_map(|(i, c)| (0..c.targets.len()).map(move |t| (i, t))).collect()
    }

    fn tool_list(&self) -> Vec<Tool> {
        let mut v = vec![];
        if !self.scenes.is_empty() {
            v.push(Tool::ByScene);
        }
        v.push(Tool::AddControl);
        v
    }

    /// The table is shown: By scene is on, and there are scenes and mappings to show.
    fn table_shown(&self) -> bool {
        self.by_scene && !self.scenes.is_empty() && !self.mappings().is_empty()
    }

    /// The cells of row `at` the cursor can be on: a control's four, a target's seven less Scenes
    /// when there are no scenes and Now for a morph; in the table a column for each scene.
    fn cells(&self, at: usize) -> Vec<usize> {
        if self.table_shown() {
            return (0..self.scenes.len()).collect();
        }
        match self.entries().get(at) {
            Some(Entry::Control(_)) => vec![0, 1, 2, 3],
            Some(Entry::Target(c, t)) => {
                let morph = matches!(self.controls[*c].targets[*t].kind, Kind::Morph(..));
                (0..7).filter(|x| (*x != 5 || !self.scenes.is_empty()) && (*x != 4 || !morph)).collect()
            }
            None => vec![],
        }
    }

    fn rows_count(&self) -> usize {
        if self.table_shown() { self.mappings().len() } else { self.entries().len() }
    }

    fn geometry(&self) -> (f32, f32, f32, f32, f32, usize) {
        let Some(ui) = self.ui.upgrade() else { return (0., 1., 1., 1., 0., 1) };
        let s = ui.global::<ControlsState>();
        (s.get_area_height(), s.get_control_pitch().max(1.), s.get_target_pitch().max(1.), s.get_table_pitch().max(1.), s.get_slack(), s.get_columns().max(1) as usize)
    }

    /// The starts of the views the rows are cut into, filling the height (a control's row and a target's
    /// cost what the component says), at least one row to a view. The table's first row is its heading.
    fn views(&self) -> Vec<usize> {
        let (area, control, target, table, slack, _) = self.geometry();
        let costs: Vec<f32> = if self.table_shown() {
            vec![table; self.mappings().len()]
        } else {
            self.entries().iter().map(|e| if matches!(e, Entry::Control(_)) { control } else { target }).collect()
        };
        let head = if self.table_shown() { table } else { 0. };
        let mut starts = vec![0];
        let mut used = head;
        for (i, cost) in costs.iter().enumerate() {
            if used > head && used + cost > area + slack {
                starts.push(i);
                used = head;
            }
            used += cost;
        }
        starts
    }

    /// The window's view of the page: the view the cursor is in, each row set only when it changed.
    pub fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let count = self.rows_count();
        let views = self.views();
        let view_of = |at: usize| views.iter().rposition(|s| *s <= at).unwrap_or(0);
        if self.stop.is_some_and(|s| matches!(s, Stop::Cell(r, _) if r >= count)) {
            self.stop = None;
            self.editing = false;
        }
        let view = match self.stop {
            Some(Stop::Cell(row, _)) => view_of(row),
            _ => view_of(self.first),
        };
        self.first = views[view];
        let end = views.get(view + 1).copied().unwrap_or(count);
        let tool_at = match self.stop {
            Some(Stop::Tool(t)) => Some(t),
            _ => None,
        };
        let state = ui.global::<ControlsState>();
        state.set_has_scenes(!self.scenes.is_empty());
        state.set_empty(self.controls.is_empty());
        let table = self.table_shown();
        state.set_table(table);
        if table {
            let (.., columns) = self.geometry();
            let at_col = match self.stop {
                Some(Stop::Cell(_, c)) => Some(c),
                _ => None,
            };
            if let Some(c) = at_col {
                if c < self.col_first {
                    self.col_first = c;
                } else if c >= self.col_first + columns {
                    self.col_first = c + 1 - columns;
                }
            }
            self.col_first = self.col_first.min(self.scenes.len().saturating_sub(columns));
            let cols_end = self.scenes.len().min(self.col_first + columns);
            let mappings = self.mappings();
            let mut rows = vec![];
            let mut cells = vec![];
            for (r, (ci, ti)) in mappings[self.first..end].iter().enumerate() {
                let at = self.first + r;
                let c = &self.controls[*ci];
                let t = &c.targets[*ti];
                let on_row = matches!(self.stop, Some(Stop::Cell(row, _)) if row == at);
                rows.push(TableRowData { label: format!("{}  ·  {}", c.name, self.describe(t)).into(), cursor: on_row });
                for (k, (id, _)) in self.scenes.iter().enumerate().take(cols_end).skip(self.col_first) {
                    cells.push(TableCellData { row: r as i32, col: (k - self.col_first) as i32, on: !t.off_in.contains(id), cursor: self.stop == Some(Stop::Cell(at, k)) });
                }
            }
            update(&self.table_rows, rows);
            update(&self.table_cells, cells);
            update(&self.scene_names, self.scenes[self.col_first..cols_end].iter().map(|s| SharedString::from(s.1.as_str())).collect());
            update(&self.rows, vec![]);
        } else {
            let entries = self.entries();
            let rows: Vec<ControlRowData> = entries[self.first..end.min(entries.len())]
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let at = self.first + i;
                    let cell = match self.stop {
                        Some(Stop::Cell(row, c)) if row == at => c as i32,
                        _ => -1,
                    };
                    let editing = self.editing && cell >= 0;
                    match e {
                        Entry::Control(ci) => {
                            let c = &self.controls[*ci];
                            ControlRowData { kind: 0, name: c.name.as_str().into(), value: c.value.clamp(0., 1.) as f32, cell, editing, ..Default::default() }
                        }
                        Entry::Target(ci, ti) => {
                            let t = &self.controls[*ci].targets[*ti];
                            ControlRowData {
                                kind: 1,
                                name: self.describe(t).into(),
                                start: t.start.clamp(0., 1.) as f32,
                                end: t.end.clamp(0., 1.) as f32,
                                curve: CURVES[t.curve.kind.min(5)].into(),
                                relative: t.relative,
                                scenes: if t.off_in.is_empty() { "Scenes: all" } else { "Scenes..." }.into(),
                                now: !matches!(t.kind, Kind::Morph(..)),
                                cell,
                                editing,
                                ..Default::default()
                            }
                        }
                    }
                })
                .collect();
            update(&self.rows, rows);
            update(&self.table_rows, vec![]);
            update(&self.table_cells, vec![]);
            update(&self.scene_names, vec![]);
        }
        state.set_more_columns(table && self.scenes.len() > self.geometry().5);
        update(
            &self.tools,
            self.tool_list()
                .iter()
                .enumerate()
                .map(|(i, t)| ControlToolData {
                    label: match t {
                        Tool::ByScene => "By scene".into(),
                        Tool::AddControl => "+ Control".into(),
                    },
                    on: *t == Tool::ByScene && self.by_scene,
                    cursor: tool_at == Some(i),
                })
                .collect(),
        );
        state.set_message(self.said.as_str().into());
        state.set_view(if count == 0 { String::new() } else { format!("{} {}–{} of {}", if table { "Mappings" } else { "Rows" }, self.first + 1, end, count) }.into());
    }

    /// The cursor's next place along the dial: cell by cell, row by row, then the buttons.
    fn dial(&self, by: i64) -> Option<Stop> {
        let mut stops: Vec<Stop> = Vec::new();
        for i in 0..self.rows_count() {
            stops.extend(self.cells(i).into_iter().map(|c| Stop::Cell(i, c)));
        }
        stops.extend((0..self.tool_list().len()).map(Stop::Tool));
        let at = match self.stop {
            Some(s) => stops.iter().position(|x| *x == s)? as i64,
            None => return stops.iter().find(|s| matches!(s, Stop::Cell(r, _) if *r >= self.first)).or(stops.first()).copied(),
        };
        stops.get((at + by).clamp(0, stops.len() as i64 - 1) as usize).copied()
    }

    /// Along the cells of the cursor's row (or the buttons) by `by`.
    fn along(&mut self, by: i64) {
        let tools = self.tool_list().len();
        match self.stop {
            None => self.stop = self.dial(0),
            Some(Stop::Cell(r, c)) => {
                let cells = self.cells(r);
                let at = cells.iter().position(|x| *x == c).unwrap_or(0) as i64;
                self.stop = cells.get((at + by).clamp(0, cells.len() as i64 - 1) as usize).map(|c| Stop::Cell(r, *c)).or(self.stop);
            }
            Some(Stop::Tool(t)) => self.stop = Some(Stop::Tool((t as i64 + by).clamp(0, tools as i64 - 1) as usize)),
        }
    }

    /// The cell of row `r` nearest `cell` (the one before it when it has none).
    fn near(&self, r: usize, cell: usize) -> usize {
        let cells = self.cells(r);
        cells.iter().rev().find(|c| **c <= cell).or(cells.first()).copied().unwrap_or(0)
    }

    /// Down the rows (the buttons after the last) by `by`, the cell kept where the row has it.
    fn down(&mut self, by: i64) {
        let rows = self.rows_count() as i64;
        let tools = self.tool_list().len() as i64;
        match self.stop {
            None => self.stop = self.dial(0),
            Some(Stop::Cell(r, c)) => {
                let to = r as i64 + by;
                self.stop = if to >= rows {
                    (by > 0).then_some(Stop::Tool(0)).or(self.stop)
                } else {
                    let to = to.max(0) as usize;
                    Some(Stop::Cell(to, self.near(to, c)))
                };
            }
            Some(Stop::Tool(t)) => {
                if by < 0 && rows > 0 {
                    let last = rows as usize - 1;
                    self.stop = Some(Stop::Cell(last, self.cells(last).first().copied().unwrap_or(0)));
                } else {
                    self.stop = Some(Stop::Tool((t as i64 + by).clamp(0, tools - 1) as usize));
                }
            }
        }
    }

    /// The first cell of the first (-1) or last (1) row, or the last button.
    fn end(&mut self, by: i64) {
        let last = self.tool_list().len().saturating_sub(1);
        self.stop = if by < 0 { (self.rows_count() > 0).then(|| Stop::Cell(0, self.cells(0).first().copied().unwrap_or(0))) } else { Some(Stop::Tool(last)) };
    }

    /// The cursor is on the first cell of the first row: moving up leaves for the top bar's actions.
    pub fn at_top(&self) -> bool {
        self.rows_count() > 0 && self.stop == Some(Stop::Cell(0, self.cells(0).first().copied().unwrap_or(0))) && !self.editing
    }

    fn set_value(&mut self, ci: usize, value: f64) {
        let value = thousandths(value.clamp(0., 1.));
        let Some(c) = self.controls.get_mut(ci) else { return };
        if (value - c.value).abs() < f64::EPSILON {
            return;
        }
        c.value = value;
        let id = c.id;
        self.edit("controls.set", json!({"control": id, "value": value}));
    }

    /// Sends control `ci`'s targets again, changed by `change` (the JUCE `setTargets`).
    fn set_targets(&mut self, ci: usize, change: impl FnOnce(&mut Vec<Target>)) {
        let Some(c) = self.controls.get_mut(ci) else { return };
        change(&mut c.targets);
        let (id, list) = (c.id, c.targets.iter().map(Target::json).collect::<Vec<_>>());
        self.edit("controls.targets", json!({"control": id, "targets": list}));
    }

    fn set_range(&mut self, ci: usize, ti: usize, end: bool, value: f64) {
        let value = thousandths(value.clamp(0., 1.));
        if self.controls.get(ci).and_then(|c| c.targets.get(ti)).is_none_or(|t| (if end { t.end } else { t.start }) == value) {
            return;
        }
        self.set_targets(ci, |ts| {
            if let Some(t) = ts.get_mut(ti) {
                *(if end { &mut t.end } else { &mut t.start }) = value;
            }
        });
    }

    fn add_control(&mut self) {
        let n = self.controls.len() + 1;
        self.edit("controls.add", json!({"name": format!("Control {n}")}));
    }

    /// A Now...: where the destination is now, as `setRangeToPresent` works it out. A parameter's
    /// value and information are asked first.
    fn present(&mut self, control: u32, ti: usize, end: bool) {
        let Some(c) = self.controls.iter().find(|c| c.id == control) else { return };
        let Some(t) = c.targets.get(ti) else { return };
        match t.kind {
            Kind::Parameter(node, param) => {
                if !self.params.contains_key(&node) {
                    self.waiting_now = Some((control, ti, end));
                    self.ask(node);
                    return;
                }
                self.out.push((Request::Value(control, ti, end), "params.get", json!({"values": [{"node": node, "param": param}]})));
            }
            Kind::Morph(..) => self.said = "A morph's ends are its scenes.".into(),
            _ => self.set_present(control, ti, end, None),
        }
    }

    /// The target's range end set to where its destination is now (`setRangeToPresent`): the parameter's
    /// value read, the others from what the page knows. The point that puts the destination where it is
    /// now, every other target where it is (RFC 0011's summing rule).
    fn set_present(&mut self, control: u32, ti: usize, end: bool, value: Option<f64>) {
        let Some(ci) = self.controls.iter().position(|c| c.id == control) else { return };
        let Some(t) = self.controls[ci].targets.get(ti).cloned() else { return };
        let param = match t.kind {
            Kind::Parameter(n, p) => self.params.get(&n).and_then(|l| l.iter().find(|x| x.id == p)).cloned(),
            _ => None,
        };
        let now = match &t.kind {
            Kind::Parameter(..) => match (&param, value) {
                (Some(i), Some(v)) if i.max > i.min => (v - i.min) / (i.max - i.min),
                _ => 0.,
            },
            Kind::PartLevel(p) => self.parts.iter().find(|x| x.id == *p).map_or(0., |x| x.level),
            Kind::PartMute(p) => self.parts.iter().find(|x| x.id == *p).map_or(0., |x| f64::from(u8::from(x.mute))),
            Kind::SendLevel(p, b) => self.parts.iter().find(|x| x.id == *p).and_then(|x| x.sends.iter().find(|s| s.0 == *b)).map_or(0., |s| s.1),
            Kind::Control(m) => self.controls.iter().find(|x| x.id == *m).map_or(0., |x| x.value),
            Kind::Morph(..) => return,
        }
        .clamp(0., 1.);
        // A relative target on a parameter that takes modulation leaves the value alone.
        let modulates = |o: &Target| o.relative && matches!(o.kind, Kind::Parameter(..)) && param.as_ref().is_some_and(|i| i.flags & MODULATABLE != 0);
        let own = modulates(&t);
        let (mut others, mut absolute) = (0., 0);
        if t.kind.sums() {
            for x in &self.controls {
                for (i, o) in x.targets.iter().enumerate() {
                    if o.same_destination(&t) && modulates(o) == own {
                        absolute += i32::from(!o.relative);
                        if !(x.id == control && i == ti) {
                            others += if o.relative { o.point(x.value) - 0.5 } else { o.point(x.value) };
                        }
                    }
                }
            }
        }
        let value_now = self.controls[ci].value;
        let mut v = if own {
            0.5 - others
        } else if !t.kind.sums() {
            if t.relative { now - t.base + 0.5 } else { now }
        } else if absolute == 1 {
            now - others + if t.relative { 0.5 } else { 0. }
        } else {
            t.point(value_now)
        };
        if let Some(i) = param.as_ref().filter(|i| i.stepped && i.max > i.min) {
            v = ((i.min + v * (i.max - i.min)).round() - i.min) / (i.max - i.min);
        }
        let v = v.clamp(0., 1.);
        self.set_targets(ci, |ts| {
            if let Some(t) = ts.get_mut(ti) {
                *(if end { &mut t.end } else { &mut t.start }) = v;
            }
        });
        self.show();
    }

    /// The menu of what a control can drive: a part's level, mute or send, another control, or any
    /// node's parameter, a morph between scenes (`PartsView::targetMenu`).
    fn target_menu(&mut self, control: u32) -> (String, Vec<Item>) {
        self.picks.clear();
        fn pick(p: Pick, picks: &mut Vec<Pick>) -> usize {
            picks.push(p);
            picks.len() - 1
        }
        let mut picks = std::mem::take(&mut self.picks);
        let mut parameters = vec![];
        for (n, name) in &self.nodes {
            let items: Vec<Item> = self
                .params
                .get(n)
                .into_iter()
                .flatten()
                .filter(|p| p.flags & READONLY == 0)
                .map(|p| Item::choice(p.name.clone(), pick(Pick::Add(control, Kind::Parameter(*n, p.id)), &mut picks)))
                .collect();
            parameters.push(Item::sub(if name.is_empty() { n.to_string() } else { name.clone() }, items));
        }
        let (mut levels, mut mutes, mut sends) = (vec![], vec![], vec![]);
        for p in &self.parts {
            levels.push(Item::choice(p.name.clone(), pick(Pick::Add(control, Kind::PartLevel(p.id)), &mut picks)));
            mutes.push(Item::choice(p.name.clone(), pick(Pick::Add(control, Kind::PartMute(p.id)), &mut picks)));
            if !p.bus {
                for (bus, _) in &p.sends {
                    let to = self.parts.iter().find(|b| b.id == *bus && b.bus).map_or("a bus".to_string(), |b| b.name.clone());
                    sends.push(Item::choice(format!("{} to {to}", p.name), pick(Pick::Add(control, Kind::SendLevel(p.id, *bus)), &mut picks)));
                }
            }
        }
        let others: Vec<Item> = self.controls.iter().filter(|m| m.id != control).map(|m| Item::choice(m.name.clone(), pick(Pick::Add(control, Kind::Control(m.id)), &mut picks))).collect();
        let morphs: Vec<Item> = self
            .scenes
            .iter()
            .map(|(a, name)| {
                let to: Vec<Item> = self.scenes.iter().filter(|(b, _)| b != a).map(|(b, n)| Item::choice(n.clone(), pick(Pick::Add(control, Kind::Morph(*a, *b)), &mut picks))).collect();
                Item::sub(format!("{name} →"), to)
            })
            .collect();
        self.picks = picks;
        let items = vec![
            Item::sub("Plugin parameter", parameters),
            Item::sub("Part level", levels),
            Item::sub("Part mute", mutes),
            Item::sub("Part send", sends),
            Item::sub("Another control", others),
            Item::sub("Morph between scenes", if self.scenes.len() >= 2 { morphs } else { vec![] }),
        ];
        let name = self.controls.iter().find(|c| c.id == control).map_or(String::new(), |c| c.name.clone());
        (format!("Target of {name}"), items)
    }

    /// + Target: the menu once every node's parameters are read.
    fn open_target_menu(&mut self, ci: usize) -> Option<(String, Vec<Item>)> {
        let id = self.control(ci)?.id;
        let missing: Vec<u64> = self.nodes.iter().map(|n| n.0).filter(|n| !self.params.contains_key(n)).collect();
        if missing.is_empty() {
            return Some(self.target_menu(id));
        }
        self.waiting_menu = Some(id);
        self.said = "Reading the plugins' parameters...".into();
        for n in missing {
            self.ask(n);
        }
        None
    }

    /// The menu of a target's curves, the one it has ticked.
    fn curve_menu(&mut self, ci: usize, ti: usize) -> Option<(String, Vec<Item>)> {
        let c = self.control(ci)?;
        let (id, t) = (c.id, c.targets.get(ti)?.clone());
        self.picks.clear();
        let items = CURVES
            .iter()
            .enumerate()
            .map(|(k, name)| {
                self.picks.push(Pick::Curve(id, ti, k));
                Item::ticked(*name, self.picks.len() - 1, t.curve.kind == k)
            })
            .collect();
        Some(("Curve".into(), items))
    }

    /// The scenes a mapping works in, each ticked; choosing one switches it there.
    fn scenes_menu(&mut self, ci: usize, ti: usize) -> Option<(String, Vec<Item>)> {
        let c = self.control(ci)?;
        let (id, t) = (c.id, c.targets.get(ti)?.clone());
        self.picks.clear();
        let items = self
            .scenes
            .clone()
            .iter()
            .map(|(scene, name)| {
                self.picks.push(Pick::Scene(id, ti, *scene));
                Item::ticked(name.clone(), self.picks.len() - 1, !t.off_in.contains(scene))
            })
            .collect();
        Some(("Scenes this mapping works in".into(), items))
    }

    /// Either end of a target's range to where its destination is now.
    fn present_menu(&mut self, ci: usize, ti: usize) -> Option<(String, Vec<Item>)> {
        let id = self.control(ci)?.id;
        self.picks.clear();
        self.picks.push(Pick::Present(id, ti, false));
        self.picks.push(Pick::Present(id, ti, true));
        Some(("Now".into(), vec![Item::choice("Start = now", 0), Item::choice("End = now", 1)]))
    }

    /// A choice of the menu that was open.
    pub fn chosen(&mut self, id: usize) {
        let Some(pick) = self.picks.get(id).cloned() else { return };
        match pick {
            Pick::Add(control, kind) => {
                if let Some(ci) = self.controls.iter().position(|c| c.id == control) {
                    self.set_targets(ci, |ts| ts.push(Target::new(kind)));
                }
            }
            Pick::Curve(control, ti, k) => {
                if let Some(ci) = self.controls.iter().position(|c| c.id == control) {
                    self.set_targets(ci, |ts| {
                        if let Some(t) = ts.get_mut(ti) {
                            t.curve.kind = k;
                        }
                    });
                }
            }
            Pick::Scene(control, ti, scene) => {
                if let Some(ci) = self.controls.iter().position(|c| c.id == control) {
                    self.set_targets(ci, |ts| {
                        if let Some(t) = ts.get_mut(ti) {
                            if let Some(at) = t.off_in.iter().position(|s| *s == scene) {
                                t.off_in.remove(at);
                            } else {
                                t.off_in.push(scene);
                            }
                        }
                    });
                }
            }
            Pick::Present(control, ti, end) => self.present(control, ti, end),
        }
        self.show();
    }

    /// The keyboard's name for the control being renamed.
    pub fn entered(&mut self, text: &str) {
        let Some(id) = self.renaming.take() else { return };
        if self.controls.iter().find(|c| c.id == id).is_some_and(|c| c.name != text) {
            self.edit("controls.name", json!({"control": id, "name": text}));
        }
    }

    pub fn not_entered(&mut self) {
        self.renaming = None;
    }

    /// What a press on `stop` does.
    fn press(&mut self, stop: Stop) -> Outcome {
        match stop {
            Stop::Tool(t) => match self.tool_list().get(t) {
                Some(Tool::ByScene) => {
                    self.by_scene = !self.by_scene;
                    self.stop = None;
                    self.editing = false;
                    self.first = 0;
                }
                Some(Tool::AddControl) => self.add_control(),
                None => {}
            },
            Stop::Cell(at, cell) if self.table_shown() => {
                self.toggle_in_scene(at, cell);
            }
            Stop::Cell(at, cell) => match self.entries().get(at).copied() {
                Some(Entry::Control(ci)) => match cell {
                    0 => {
                        let c = &self.controls[ci];
                        self.renaming = Some(c.id);
                        return Outcome::Prompt(format!("Rename {}", c.name), c.name.clone());
                    }
                    1 => self.editing = !self.editing,
                    2 => self.ready_menu = self.open_target_menu(ci),
                    _ => {
                        let id = self.controls[ci].id;
                        self.stop = None;
                        self.edit("controls.remove", json!({"control": id}));
                    }
                },
                Some(Entry::Target(ci, ti)) => match cell {
                    0 | 1 => self.editing = !self.editing,
                    2 => self.ready_menu = self.curve_menu(ci, ti),
                    3 => self.set_targets(ci, |ts| {
                        if let Some(t) = ts.get_mut(ti) {
                            t.relative = !t.relative;
                        }
                    }),
                    4 => self.ready_menu = self.present_menu(ci, ti),
                    5 => self.ready_menu = self.scenes_menu(ci, ti),
                    _ => {
                        self.set_targets(ci, |ts| {
                            ts.remove(ti);
                        });
                        self.stop = None;
                    }
                },
                None => {}
            },
        }
        Outcome::Stay
    }

    /// A tick of the table: a mapping on or off in a scene.
    fn toggle_in_scene(&mut self, row: usize, col: usize) {
        let Some((ci, ti)) = self.mappings().get(row).copied() else { return };
        let Some(scene) = self.scenes.get(col).map(|s| s.0) else { return };
        self.set_targets(ci, |ts| {
            if let Some(t) = ts.get_mut(ti) {
                if let Some(at) = t.off_in.iter().position(|s| *s == scene) {
                    t.off_in.remove(at);
                } else {
                    t.off_in.push(scene);
                }
            }
        });
    }

    /// The dial on the slider being changed.
    fn step_value(&mut self, by: i64) {
        let Some(Stop::Cell(at, cell)) = self.stop else { return };
        match self.entries().get(at).copied() {
            Some(Entry::Control(ci)) if cell == 1 => {
                let was = self.controls[ci].value;
                self.set_value(ci, stepped(was, by));
            }
            Some(Entry::Target(ci, ti)) if cell <= 1 => {
                let t = &self.controls[ci].targets[ti];
                let was = if cell == 1 { t.end } else { t.start };
                self.set_range(ci, ti, cell == 1, stepped(was, by));
            }
            _ => {}
        }
    }

    /// A navigation action. The dial and the keys' up and down go through the page, the left knob
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
        let views = self.views();
        let now = views.iter().rposition(|s| *s <= self.first).unwrap_or(0) as i64;
        let to = (now + i64::from(by)).clamp(0, views.len() as i64 - 1) as usize;
        self.first = views[to];
        if let Some(Stop::Cell(_, c)) = self.stop {
            if self.first < self.rows_count() {
                self.stop = Some(Stop::Cell(self.first, self.near(self.first, c)));
            }
        }
    }

    /// The entry of the view's row `row`.
    fn at(&self, row: i32) -> usize {
        self.first + row.max(0) as usize
    }

    fn touched(&mut self) {
        self.stop = None;
        self.editing = false;
    }

    /// A tap on cell `cell` of the view's row `row`: what a press there does.
    pub fn tap(&mut self, row: i32, cell: i32) -> Outcome {
        self.touched();
        let outcome = self.press(Stop::Cell(self.at(row), cell.max(0) as usize));
        self.show();
        outcome
    }

    /// A tap on a tick of the table: the view's row and the column shown.
    pub fn tap_table(&mut self, row: i32, col: i32) {
        self.touched();
        self.toggle_in_scene(self.at(row), self.col_first + col.max(0) as usize);
        self.show();
    }

    /// A tap on one of the buttons under the page.
    pub fn tap_tool(&mut self, i: i32) -> Outcome {
        self.touched();
        let outcome = self.press(Stop::Tool(i.max(0) as usize));
        self.show();
        outcome
    }

    /// A control's fader moved to `place` of its length.
    pub fn level(&mut self, row: i32, place: f32) {
        self.touched();
        if let Some(Entry::Control(ci)) = self.entries().get(self.at(row)).copied() {
            self.set_value(ci, f64::from(place));
        }
        self.show();
    }

    /// A target's slider moved: its end (true) or its start.
    pub fn range(&mut self, row: i32, end: bool, place: f32) {
        self.touched();
        if let Some(Entry::Target(ci, ti)) = self.entries().get(self.at(row)).copied() {
            self.set_range(ci, ti, end, f64::from(place));
        }
        self.show();
    }

    /// A finger on a slider (`cell`: 1 a control's fader, 0 and 1 a target's ends) or off it.
    pub fn hold(&mut self, row: i32, cell: i32, down: bool) {
        self.held = if down {
            match self.entries().get(self.at(row)) {
                Some(Entry::Control(ci)) => Some((self.controls[*ci].id, None, 1)),
                Some(Entry::Target(ci, ti)) => Some((self.controls[*ci].id, Some(*ti), cell.max(0) as usize)),
                None => None,
            }
        } else {
            None
        };
    }

    /// Scene columns the table shows towards the first (-1) or the last (1).
    pub fn turn_columns(&mut self, by: i32) {
        let (.., columns) = self.geometry();
        self.col_first = (self.col_first as i64 + i64::from(by) * columns as i64).clamp(0, self.scenes.len().saturating_sub(columns) as i64) as usize;
        self.show();
    }

    /// The cursor drops when the screen goes, as on focus loss in the JUCE lists.
    pub fn hidden(&mut self) {
        self.stop = None;
        self.editing = false;
        self.held = None;
        self.said.clear();
        self.show();
    }

    /// Shown: the views' sizes may have changed while it was not, and the names of the parameters are asked.
    pub fn shown(&mut self) {
        self.want_names();
        self.show();
    }

    /// Every control's targets by control id, for tests.
    #[cfg(test)]
    pub fn targets(&self) -> std::collections::BTreeMap<u32, Vec<Target>> {
        self.controls.iter().map(|c| (c.id, c.targets.clone())).collect()
    }
}
