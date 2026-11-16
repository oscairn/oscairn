// SPDX-License-Identifier: AGPL-3.0-or-later
//! A part's insert chain (the JUCE Part page's chain row, hosts/standalone/PartsView.cpp and
//! PartsModel.cpp's insertPlugin, removeBox): its plugins in order, with "+ Insert plugin here" before
//! each and after the last; a press on a plugin opens what can be done to it (its parameters, turn it off
//! or on, move it earlier or later, remove it, and where it has a window of its own a disabled entry
//! saying that view is not available in this shell yet); a press on an insert row lists the plugins the
//! isolated scanner found (scan.rs), and choosing one adds it there. Splits and lanes are listed
//! read only (ui/PERFORMANCE-APP.md, D7 to D9).
//!
//! Requests, each followed by `graph.commit` as the JUCE setters commit: inserting is `graph.add` (named
//! "Basic Synth", then "Basic Synth 2", as `addNamed`) and, when it answers, `parts.insert`, the node
//! taken out again with `graph.remove` if the insert is refused; `parts.move`, `graph.enable`, and
//! removing is `parts.removeNode` then `graph.remove` with `userWires: reconnect` (as `removeBox`).
//! `window.info` is asked for a plugin whose actions are shown. Without touch: the left knob or the dial
//! moves the cursor, a press chooses, Cancel goes back one layer (actions or plugins, then the chain,
//! then the Part screen); the right knob scrolls the read-only panel.

use crate::browser::{Browser, Nav};
use crate::menu::Item;
use crate::nav::Action;
use crate::parts::{role_color, role_of};
use crate::scan::Plugin;
use crate::{InfoLine, SettingData, Shell};
use serde_json::{json, Value};
use std::collections::HashMap;

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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    List,
    /// The actions on the plugin at this place in the chain.
    Actions(usize),
    /// The plugins to insert at this place.
    Plugins(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Insert(usize),
    Node(usize),
    Split(usize),
    Parameters,
    Power,
    Earlier,
    Later,
    Remove,
    Routing,
    SceneControls,
    View,
    Choose(usize),
    Nothing,
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Node(u64),
    Split(u64),
}

pub struct Screen {
    b: Browser,
    graph: Value,
    id: u64,
    mode: Mode,
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
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen { b: Browser::new(ui), graph: Value::Null, id: 0, mode: Mode::List, plugins: vec![], scanning: true, windows: HashMap::new(), ready_menu: None, picks: vec![], error: String::new(), failures: vec![], out: vec![] }
    }

    pub fn at_top(&self) -> bool {
        self.b.at_top()
    }

    pub fn open(&mut self, id: u32) {
        self.id = id as u64;
        self.mode = Mode::List;
        self.b.reset();
        self.say("");
        self.show();
    }

    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
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

    /// The entries of a split's lane.
    fn lane_entries(&self, split: u64, lane: usize) -> Vec<Entry> {
        self.split(split).map(|s| Self::entries_of(&s["lanes"][lane]["chain"])).unwrap_or_default()
    }

    fn name_of(&self, node: u64) -> String {
        self.node(node).and_then(|n| n["name"].as_str()).unwrap_or("a plugin").to_string()
    }

    fn rows(&self) -> Vec<(Row, SettingData)> {
        let row = |label: String, value: String, kind: i32| SettingData { label: label.into(), value: value.into(), kind, ..Default::default() };
        let boxed = |label: String, node: u64| {
            let n = self.node(node);
            let off = n.is_some_and(|n| n["enabled"] == false);
            let role = n.map_or("Notes", role_of);
            SettingData { label: label.into(), value: format!("{role}{} ›", if off { " (off)" } else { "" }).into(), kind: 2, role: role_color(role), off, failed: self.failed(node), ..Default::default() }
        };
        let entries = self.entries();
        match self.mode {
            Mode::List => {
                let mut rows = Vec::new();
                for i in 0..=entries.len() {
                    rows.push((Row::Insert(i), row("+ Insert plugin here".into(), String::new(), 2)));
                    match entries.get(i) {
                        Some(Entry::Node(n)) => {
                            rows.push((Row::Node(i), boxed(format!("{}. {}", i + 1, self.name_of(*n)), *n)));
                        }
                        Some(Entry::Split(s)) => {
                            let lanes = self.lane_count(*s);
                            rows.push((Row::Split(i), row(format!("{}. Split", i + 1), format!("{lanes} lanes (not editable here yet)"), 4)));
                        }
                        None => {}
                    }
                }
                rows
            }
            Mode::Actions(i) => {
                let Some(Entry::Node(n)) = entries.get(i).copied() else { return vec![] };
                let off = self.node(n).is_some_and(|n| n["enabled"] == false);
                let mut rows = vec![(Row::Parameters, row("Parameters".into(), "›".into(), 2)), (Row::Power, row(if off { "Turn on" } else { "Turn off" }.into(), String::new(), 2))];
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
            Mode::Plugins(_) => {
                if self.plugins.is_empty() {
                    return vec![(Row::Nothing, row(if self.scanning { "Finding plugins..." } else { "No plugins found" }.into(), String::new(), 4))];
                }
                self.plugins.iter().enumerate().map(|(i, p)| (Row::Choose(i), row(p.name.clone(), if p.is_instrument() { "Instrument" } else { "Effect" }.into(), 2))).collect()
            }
        }
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
        let mut lines = vec![head("Chain"), plain("In".into())];
        let entries = self.entries();
        if entries.is_empty() {
            lines.push(plain("Empty: the sound goes straight through".into()));
        }
        for (i, e) in entries.iter().enumerate() {
            match e {
                Entry::Node(n) => lines.push(boxed(format!("{}. {}", i + 1, self.name_of(*n)), *n)),
                Entry::Split(s) => {
                    lines.push(plain(format!("{}. Split: {}", i + 1, split_kind(self.split(*s).and_then(|x| x["kind"].as_str()).unwrap_or("copy")))));
                    for k in 0..self.lane_count(*s) {
                        let inside = self.lane_entries(*s, k);
                        if inside.is_empty() {
                            lines.push(plain(format!("    {}: Dry", self.lane_label(*s, k))));
                        }
                        for (j, x) in inside.iter().enumerate() {
                            let label = if j == 0 { format!("    {}: ", self.lane_label(*s, k)) } else { "        ".into() };
                            lines.push(match x {
                                Entry::Node(n) => boxed(format!("{label}{}", self.name_of(*n)), *n),
                                Entry::Split(_) => plain(format!("{label}Split")),
                            });
                        }
                    }
                }
            }
        }
        lines.push(plain("Out".into()));
        if let Mode::Actions(i) = self.mode {
            if let Some(Entry::Node(n)) = entries.get(i) {
                lines.extend([head("Plugin"), plain(self.node(*n).and_then(|x| x["plugin"]["name"].as_str()).unwrap_or_default().to_string())]);
            }
        }
        lines
    }

    /// What a lane is called: Left and Right, Mid and Side, Band n; "Lane n" for the layers of a copy.
    fn lane_label(&self, split: u64, lane: usize) -> String {
        match (self.split(split).and_then(|x| x["kind"].as_str()).unwrap_or("copy"), lane) {
            ("leftRight", 0) => "Left".into(),
            ("leftRight", _) => "Right".into(),
            ("midSide", 0) => "Mid".into(),
            ("midSide", _) => "Side".into(),
            ("bands", k) => format!("Band {}", k + 1),
            (_, k) => format!("Lane {}", k + 1),
        }
    }

    fn show(&mut self) {
        let name = self.part().and_then(|p| p["name"].as_str()).unwrap_or_default().to_string();
        let title = match self.mode {
            Mode::List => "Insert effects".to_string(),
            Mode::Actions(i) => match self.entries().get(i) {
                Some(Entry::Node(n)) => self.name_of(*n),
                _ => String::new(),
            },
            Mode::Plugins(_) => "Insert: choose a plugin".to_string(),
        };
        let rows = self.rows().into_iter().map(|r| r.1).collect();
        let info = self.info();
        self.b.say(&self.message()); // an edit the engine refused, else the plugin that failed (the reference's message line)
        let noun = match self.mode {
            Mode::List => "Chain",
            Mode::Actions(_) => "Actions",
            Mode::Plugins(_) => "Plugins",
        };
        self.b.show(&name, &title, noun, rows, info);
    }

    /// Whether an edit was refused: the shell then asks the graph again.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Add(index), Ok(r)) => {
                if let Some(node) = r["node"].as_u64() {
                    self.out.push((Request::Insert(node), "parts.insert", json!({"part": self.id, "index": index, "node": node})));
                }
            }
            (Request::Insert(_), Ok(_)) => self.out.push((Request::Edit, "graph.commit", json!({}))),
            (Request::Insert(node), Err(e)) => {
                // The JUCE insertPlugin takes the node out again when the insert is refused.
                self.out.push((Request::Edit, "graph.remove", json!({"node": node})));
                self.out.push((Request::Edit, "graph.commit", json!({})));
                self.say(&format!("The engine refused: {}", e.message));
                return true;
            }
            (Request::Add(_) | Request::Edit, Err(e)) if e.code != 0 => {
                self.say(&format!("The engine refused: {}", e.message));
                return true;
            }
            (Request::List(node), Ok(r)) => self.scene_controls(node, &r),
            (Request::Window(node), Ok(r)) => {
                self.windows.insert(node, r["available"].as_bool().unwrap_or(false));
                self.show();
            }
            _ => {}
        }
        false
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

    fn press(&mut self, at: usize) -> Outcome {
        let Some((row, _)) = self.rows().into_iter().nth(at) else { return Outcome::Stay };
        let entries = self.entries();
        match (row, self.mode) {
            (Row::Insert(i), _) => {
                self.mode = Mode::Plugins(i);
                self.b.reset_cursor();
            }
            (Row::Node(i), _) => {
                if let Some(Entry::Node(n)) = entries.get(i) {
                    self.out.push((Request::Window(*n), "window.info", json!({"node": n})));
                }
                self.mode = Mode::Actions(i);
                self.b.reset_cursor();
            }
            (Row::Parameters, Mode::Actions(i)) => {
                if let Some(Entry::Node(n)) = entries.get(i) {
                    return Outcome::Params(*n, self.name_of(*n));
                }
            }
            (Row::Power, Mode::Actions(i)) => {
                if let Some(Entry::Node(n)) = entries.get(i) {
                    let on = self.node(*n).is_some_and(|n| n["enabled"] == false); // the JUCE button: off turns it on
                    self.edit("graph.enable", json!({"node": n, "enabled": on}));
                }
            }
            (Row::Earlier | Row::Later, Mode::Actions(i)) => {
                let to = if row == Row::Earlier { i - 1 } else { i + 1 };
                self.edit("parts.move", json!({"part": self.id, "from": i, "to": to}));
                self.mode = Mode::Actions(to);
                self.b.reset_cursor();
            }
            (Row::Routing, Mode::Actions(i)) => {
                if let Some(Entry::Node(n)) = entries.get(i) {
                    return Outcome::Routing(*n);
                }
            }
            (Row::SceneControls, Mode::Actions(i)) => {
                if let Some(Entry::Node(n)) = entries.get(i) {
                    self.out.push((Request::List(*n), "params.list", json!({"node": n})));
                }
            }
            (Row::Remove, Mode::Actions(i)) => {
                if let Some(Entry::Node(n)) = entries.get(i) {
                    self.edit("parts.removeNode", json!({"part": self.id, "node": n}));
                    self.out.pop(); // the commit follows both
                    self.out.push((Request::Edit, "graph.remove", json!({"node": n, "userWires": "reconnect"})));
                    self.out.push((Request::Edit, "graph.commit", json!({})));
                    self.mode = Mode::List;
                    self.b.reset_cursor();
                }
            }
            (Row::Choose(p), Mode::Plugins(index)) => {
                if let Some(plugin) = self.plugins.get(p).cloned() {
                    let name = self.unique_name(&plugin.name);
                    self.say("");
                    self.out.push((Request::Add(index), "graph.add", json!({"name": name, "path": plugin.path, "pluginId": plugin.id})));
                    self.mode = Mode::List;
                    self.b.reset_cursor();
                }
            }
            _ => return Outcome::Stay,
        }
        Outcome::Stay
    }

    fn handle(&mut self, nav: Nav) -> Outcome {
        let outcome = match nav {
            Nav::Press(at) => self.press(at),
            Nav::Pane(by) => {
                self.b.scroll_info(by.signum());
                Outcome::Stay
            }
            Nav::Back => match self.mode {
                Mode::List => Outcome::Back,
                _ => {
                    self.mode = Mode::List;
                    self.b.reset_cursor();
                    Outcome::Stay
                }
            },
            Nav::Dial(..) | Nav::None => Outcome::Stay,
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

    pub fn scroll_info(&mut self, by: i32) {
        self.b.scroll_info(by as i64);
        self.show();
    }

    pub fn step_row(&mut self, _row: i32, _by: i32) {}

    pub fn hidden(&mut self) {
        self.b.hidden();
        self.show();
    }

    pub fn shown(&mut self) {
        self.b.install();
        self.show();
    }
}
