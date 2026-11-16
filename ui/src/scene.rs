// SPDX-License-Identifier: AGPL-3.0-or-later
//! The playing scene's setup (the JUCE Parts view's scene edit menu, hosts/standalone/PartsView.cpp
//! `sceneEditMenu`), as a list on the Part screen's component: Store (capture the whole state into the
//! scene again, `scenes.store`), Rename (`scenes.name`, the name asked on the keyboard), its scope, the
//! kinds of state selecting it applies, each switched on or off (`scenes.scope`), and its transition: the
//! glide time, the curve and when it launches while playing (`scenes.transition`). The JUCE menu has no
//! remove: neither does this (the engine has `scenes.remove`; nothing in the JUCE app calls it).
//! Without touch: the left knob or the dial moves the cursor, a press does a row or starts changing
//! its value, the dial then steps it, Cancel ends the change, drops the cursor, then goes back.
//! The edits are sent as the JUCE menu sends them, with no `graph.commit` (the engine does not stage them).

use crate::browser::{Browser, Nav};
use crate::nav::Action;
use crate::{InfoLine, SettingData, Shell};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Any edit: its refusal is said.
    Edit,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Back,
    /// The new name asked for on the keyboard: title and text; `entered` takes it.
    Prompt(String, String),
}

/// The scope's kinds in the engine's order (sceneKinds), as the JUCE menu words them.
pub const KINDS: [(&str, &str); 6] = [
    ("enabled", "Plugins and wires on or off"),
    ("mutes", "Mutes and solos"),
    ("mix", "Levels, pans and sends"),
    ("parameters", "Parameters and controls"),
    ("mappings", "Control mappings"),
    ("zones", "Part note zones"),
];
/// The glide times the menu offers, in milliseconds.
pub const GLIDES: [u64; 8] = [0, 100, 250, 500, 1000, 2000, 4000, 8000];
const CURVES: [(&str, &str); 3] = [("linear", "Linear"), ("ease", "Ease in and out"), ("equal-power", "Equal power (levels by power)")];
const LAUNCHES: [(&str, &str); 3] = [("now", "At once"), ("beat", "On the next beat"), ("bar", "On the next bar")];

pub fn glide_text(ms: u64) -> String {
    match ms {
        0 => "At once".into(),
        ms if ms < 1000 => format!("{ms} ms"),
        ms => format!("{} s", ms / 1000),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Store,
    Name,
    Scope(usize),
    Glide,
    Curve,
    Launch,
}

const ROWS: [Row; 11] = [Row::Store, Row::Name, Row::Scope(0), Row::Scope(1), Row::Scope(2), Row::Scope(3), Row::Scope(4), Row::Scope(5), Row::Glide, Row::Curve, Row::Launch];

/// The scene's own words from graph.get.
#[derive(Clone, Debug, Default, PartialEq)]
struct Scene {
    id: u64,
    name: String,
    scope: Vec<String>,
    glide: u64,
    curve: usize,
    launch: usize,
    parts: usize,
    values: usize,
}

fn index(names: &[(&str, &str)], word: Option<&str>) -> usize {
    names.iter().position(|(n, _)| Some(*n) == word).unwrap_or(0)
}

fn read(s: &Value) -> Scene {
    Scene {
        id: s["id"].as_u64().unwrap_or(0),
        name: s["name"].as_str().unwrap_or_default().into(),
        scope: s["scope"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect(),
        glide: s["glide"].as_u64().unwrap_or(0),
        curve: index(&CURVES, s["curve"].as_str()),
        launch: index(&LAUNCHES, s["launch"].as_str()),
        parts: s["parts"].as_array().map_or(0, Vec::len),
        values: s["values"].as_array().map_or(0, Vec::len),
    }
}

pub struct Screen {
    b: Browser,
    id: u64,
    scene: Option<Scene>,
    asking: bool,
    /// Edits sent and not yet answered: a graph read meanwhile may be from before them, so the scene
    /// keeps what it shows (what was sent) until the read after the last answer.
    unanswered: usize,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen { b: Browser::new(ui), id: 0, scene: None, asking: false, unanswered: 0, out: vec![] }
    }

    pub fn at_top(&self) -> bool {
        self.b.at_top()
    }

    /// Opened for scene `id`.
    pub fn open(&mut self, id: u64) {
        self.id = id;
        self.scene = None;
        self.asking = false;
        self.unanswered = 0;
        self.b.reset();
        self.b.say("");
    }

    pub fn graph(&mut self, graph: &Value) {
        if self.unanswered > 0 && self.scene.is_some() {
            return;
        }
        self.scene = graph["scenes"].as_array().and_then(|s| s.iter().find(|s| s["id"].as_u64() == Some(self.id))).map(read);
        self.show();
    }

    fn rows(&self) -> Vec<SettingData> {
        let Some(s) = &self.scene else { return vec![] };
        let row = |label: &str, value: String, kind: i32, on: bool| SettingData { label: label.into(), value: value.into(), kind, on, ..Default::default() };
        let mut rows = vec![row("Store", "capture the state now".into(), 2, false), row("Rename", format!("{} ›", s.name), 2, false)];
        rows.extend(KINDS.iter().map(|(kind, label)| {
            let on = s.scope.iter().any(|k| k == kind);
            SettingData { switch: true, ..row(label, if on { "On" } else { "Off" }.into(), 1, on) }
        }));
        rows.push(row("Glide time", glide_text(s.glide), 0, false));
        rows.push(row("Curve", CURVES[s.curve].1.into(), 0, false));
        rows.push(row("While playing", LAUNCHES[s.launch].1.into(), 0, false));
        rows
    }

    fn row(&self, at: usize) -> Option<Row> {
        ROWS.get(at).copied()
    }

    fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let Some(s) = &self.scene else { return vec![line("The scene is gone".into())] };
        vec![
            head("Scene"),
            line(s.name.clone()),
            head("Stored"),
            line(format!("{} parts, {} parameter values", s.parts, s.values)),
            head("Selecting it applies"),
            line(KINDS.iter().filter(|(k, _)| s.scope.iter().any(|x| x == k)).map(|(_, l)| *l).collect::<Vec<_>>().join(", ")),
        ]
    }

    fn show(&mut self) {
        let name = self.scene.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let (rows, info) = (self.rows(), self.info());
        self.b.show(&name, "Scene setup", "Settings", rows, info);
    }

    /// Whether an edit was refused (said).
    /// What the engine answered; true when the graph should be read again: after a refusal, and after
    /// every accepted edit, as no scenes.* request commits and so no graph.changed follows one.
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        self.unanswered = self.unanswered.saturating_sub(1);
        match (request, result) {
            (Request::Edit, Err(e)) if e.code != 0 => {
                self.b.say(&crate::client::refusal(&e));
                self.show();
                true
            }
            (Request::Edit, Ok(_)) => true,
            _ => false,
        }
    }

    fn edit(&mut self, method: &'static str, params: Value) {
        self.unanswered += 1;
        self.out.push((Request::Edit, method, params));
    }

    fn transition(&mut self, glide: u64, curve: usize, launch: usize) {
        self.edit("scenes.transition", json!({"scene": self.id, "glide": glide, "curve": CURVES[curve].0, "launch": LAUNCHES[launch].0}));
        if let Some(s) = self.scene.as_mut() {
            (s.glide, s.curve, s.launch) = (glide, curve, launch); // shown now; graph.get confirms
        }
    }

    /// The dial (or − and +) on a value row: the glide through the menu's times, the curve and the launch through their choices.
    fn step(&mut self, row: Row, by: i64) {
        let Some(s) = self.scene.clone() else { return };
        let up = by > 0;
        match row {
            Row::Glide => {
                let at = if up { GLIDES.iter().position(|&g| g > s.glide).unwrap_or(GLIDES.len() - 1) } else { GLIDES.iter().rposition(|&g| g < s.glide).unwrap_or(0) };
                if GLIDES[at] != s.glide {
                    self.transition(GLIDES[at], s.curve, s.launch);
                }
            }
            Row::Curve if s.glide > 0 => {
                let to = (s.curve as i64 + by.signum()).clamp(0, CURVES.len() as i64 - 1) as usize;
                if to != s.curve {
                    self.transition(s.glide, to, s.launch);
                }
            }
            Row::Launch => {
                let to = (s.launch as i64 + by.signum()).clamp(0, LAUNCHES.len() as i64 - 1) as usize;
                if to != s.launch {
                    self.transition(s.glide, s.curve, to);
                }
            }
            _ => {}
        }
    }

    fn press(&mut self, at: usize) -> Outcome {
        let (Some(row), Some(s)) = (self.row(at), self.scene.clone()) else { return Outcome::Stay };
        match row {
            Row::Store => {
                self.edit("scenes.store", json!({"scene": self.id}));
                self.b.say("Stored");
            }
            Row::Name => {
                self.asking = true;
                return Outcome::Prompt(format!("Rename {}", s.name), s.name);
            }
            Row::Scope(k) => {
                let kind = KINDS[k].0;
                let mut scope: Vec<&str> = KINDS.iter().map(|(n, _)| *n).filter(|n| (s.scope.iter().any(|x| x == n)) != (*n == kind)).collect();
                scope.sort_by_key(|n| KINDS.iter().position(|(k, _)| k == n));
                self.edit("scenes.scope", json!({"scene": self.id, "scope": scope}));
                if let Some(s) = self.scene.as_mut() {
                    s.scope = scope.iter().map(|k| k.to_string()).collect(); // shown now; graph.get confirms
                }
            }
            Row::Curve if s.glide == 0 => self.b.say("The curve applies when there is a glide time"),
            Row::Glide | Row::Curve | Row::Launch => self.b.editing = true,
        }
        Outcome::Stay
    }

    /// The keyboard's text, after Done: a blank name changes nothing, as the JUCE rename.
    pub fn entered(&mut self, text: &str) {
        if std::mem::take(&mut self.asking) && !text.trim().is_empty() {
            self.edit("scenes.name", json!({"scene": self.id, "name": text.trim()}));
            if let Some(s) = self.scene.as_mut() {
                s.name = text.trim().to_string(); // shown now; graph.get confirms
            }
        }
        self.show();
    }

    pub fn not_entered(&mut self) {
        self.asking = false;
    }

    fn handle(&mut self, nav: Nav) -> Outcome {
        let outcome = match nav {
            Nav::Press(at) => {
                // A press on the row being changed ends the change.
                if self.b.editing {
                    self.b.editing = false;
                    Outcome::Stay
                } else {
                    self.press(at)
                }
            }
            Nav::Dial(at, by) => {
                if let Some(row) = self.row(at) {
                    self.step(row, by);
                }
                Outcome::Stay
            }
            Nav::Pane(by) => {
                self.b.scroll_info(by.signum());
                Outcome::Stay
            }
            Nav::Back => Outcome::Back,
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

    /// − or + on a value row.
    pub fn step_row(&mut self, row: i32, by: i32) {
        if let Some(r) = self.row(self.b.index(row)) {
            self.step(r, by as i64);
        }
        self.show();
    }

    pub fn scroll_info(&mut self, by: i32) {
        self.b.scroll_info(by as i64);
        self.show();
    }

    pub fn hidden(&mut self) {
        self.b.hidden();
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

    #[test]
    fn glide_times_read_as_the_menu_has_them() {
        let all: Vec<_> = GLIDES.iter().map(|&g| glide_text(g)).collect();
        assert_eq!(all, ["At once", "100 ms", "250 ms", "500 ms", "1 s", "2 s", "4 s", "8 s"]);
    }

    #[test]
    fn a_scene_reads_from_graph_get() {
        let s = read(&json!({"id": 3, "name": "Chorus", "scope": ["mutes", "mix"], "glide": 500, "curve": "ease", "launch": "bar", "parts": [{}, {}], "values": [{}]}));
        assert_eq!((s.id, s.name.as_str(), s.scope.len(), s.glide, s.curve, s.launch, s.parts, s.values), (3, "Chorus", 2, 500, 1, 2, 2, 1));
        assert_eq!(read(&json!({"id": 1})), Scene { id: 1, ..Default::default() }, "words the engine leaves out are the first choices");
    }
}
