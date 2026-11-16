// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sound chooser for a part (the JUCE app's library Sounds, RFC 0021): the library's Sounds
//! (`library.query` of kind `sound`, narrowed by a search text typed on the on-screen keyboard), and for
//! the one pressed: load it into the part (`parts.loadSound`: the chain is replaced, the seat kept),
//! make a new part from it (`parts.fromSound`), try it (`audition.start` for this part, `audition.stop`).
//! Saving the part as a Sound (`parts.saveSound`) is here too: as the JUCE page saves, over the Sound it
//! came from, or as a new one named on the keyboard. Each of these takes long and is refused while graph
//! edits are staged; the refusal is said. Without touch: the left knob or the dial moves the cursor, a
//! press chooses, Cancel goes back one layer.

use crate::browser::{Browser, Nav};
use crate::nav::Action;
use crate::{InfoLine, SettingData, Shell};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Query,
    /// Load, new part, audition, save: the answer says what happened.
    Do(&'static str),
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Back,
    /// The search or a new Sound's name asked for on the keyboard: title and text; `entered` takes it.
    Prompt(String, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Search,
    Sound(usize),
    Load,
    NewPart,
    Audition,
    Stop,
    Save,
    SaveNew,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    List,
    Actions(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Asking {
    Search,
    SaveNew,
}

pub struct Screen {
    b: Browser,
    graph: Value,
    id: u64,
    mode: Mode,
    items: Vec<Value>,
    total: u64,
    search: String,
    asking: Option<Asking>,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen { b: Browser::new(ui), graph: Value::Null, id: 0, mode: Mode::List, items: vec![], total: 0, search: String::new(), asking: None, out: vec![] }
    }

    pub fn at_top(&self) -> bool {
        self.b.at_top()
    }

    pub fn open(&mut self, id: u32) {
        self.id = id as u64;
        self.mode = Mode::List;
        self.search.clear();
        self.b.reset();
        self.b.say("");
        self.query();
        self.show();
    }

    fn query(&mut self) {
        let mut params = json!({"kind": "sound", "limit": 1000});
        if !self.search.is_empty() {
            params["text"] = json!(self.search);
        }
        self.out.push((Request::Query, "library.query", params));
    }

    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
        self.show();
    }

    fn part(&self) -> Option<&Value> {
        self.graph["parts"].as_array().and_then(|p| p.iter().find(|p| p["id"].as_u64() == Some(self.id)))
    }

    fn rows(&self) -> Vec<(Row, SettingData)> {
        let row = |label: String, value: String, kind: i32| SettingData { label: label.into(), value: value.into(), kind, ..Default::default() };
        match self.mode {
            Mode::List => {
                let mut rows = vec![(Row::Search, row("Search".into(), format!("{} ›", if self.search.is_empty() { "all" } else { &self.search }), 2))];
                rows.extend(self.items.iter().enumerate().map(|(i, s)| (Row::Sound(i), row(s["name"].as_str().unwrap_or_default().into(), s["tags"].as_array().and_then(|t| t.first()).and_then(Value::as_str).unwrap_or_default().into(), 2))));
                rows
            }
            Mode::Actions(_) => {
                let from = self.part().is_some_and(|p| p["from"].is_object());
                vec![
                    (Row::Load, row("Load into this part".into(), String::new(), 2)),
                    (Row::NewPart, row("New part from it".into(), String::new(), 2)),
                    (Row::Audition, row("Audition in this part".into(), String::new(), 2)),
                    (Row::Stop, row("Stop auditioning".into(), String::new(), 2)),
                    (Row::Save, row(if from { "Save this part over its Sound" } else { "Save this part as a Sound" }.into(), String::new(), 2)),
                    (Row::SaveNew, row("Save this part as a new Sound".into(), "name ›".into(), 2)),
                ]
            }
        }
    }

    fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head("Sound of this part")];
        match self.part().and_then(|p| p["from"]["name"].as_str()) {
            Some(n) => lines.push(line(format!("From “{n}”{}", if self.part().is_some_and(|p| p["from"]["modified"] == true) { ", edited" } else { "" }))),
            None => lines.push(line("Not from a library Sound".into())),
        }
        lines.push(head("Library"));
        lines.push(line(format!("{} of {} Sounds", self.items.len(), self.total)));
        if let Mode::Actions(i) = self.mode {
            if let Some(s) = self.items.get(i) {
                lines.extend([head("Chosen"), line(s["name"].as_str().unwrap_or_default().into())]);
                if let Some(d) = s["description"].as_str().filter(|d| !d.is_empty()) {
                    lines.push(line(d.into()));
                }
            }
        }
        lines
    }

    fn show(&mut self) {
        let name = self.part().and_then(|p| p["name"].as_str()).unwrap_or_default().to_string();
        let title = match self.mode {
            Mode::List => "Sound".to_string(),
            Mode::Actions(i) => self.items.get(i).and_then(|s| s["name"].as_str()).unwrap_or_default().to_string(),
        };
        let rows = self.rows().into_iter().map(|r| r.1).collect();
        let info = self.info();
        self.b.show(&name, &title, if self.mode == Mode::List { "Sounds" } else { "Actions" }, rows, info);
    }

    /// Whether an edit was refused (said).
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Query, Ok(r)) => {
                self.items = r["items"].as_array().cloned().unwrap_or_default();
                self.total = r["total"].as_u64().unwrap_or(self.items.len() as u64);
            }
            (Request::Do(what), Ok(r)) => {
                let flagged = r["flagged"].as_array().map_or(0, Vec::len);
                self.b.say(&match (what, flagged) {
                    ("load", 0) => "Loaded".to_string(),
                    ("load", n) => format!("Loaded; {n} wires or targets were removed"),
                    ("fromSound", _) => "A new part was made".into(),
                    ("saveSound", _) => "Saved".into(),
                    _ => String::new(),
                });
            }
            (Request::Query | Request::Do(_), Err(e)) if e.code != 0 => {
                self.b.say(&format!("The engine refused: {}", e.message));
                self.show();
                return true;
            }
            _ => {}
        }
        self.show();
        false
    }

    fn press(&mut self, at: usize) -> Outcome {
        let Some((row, _)) = self.rows().into_iter().nth(at) else { return Outcome::Stay };
        let id = |i: usize| self.items.get(i).and_then(|s| s["id"].as_str()).map(String::from);
        match (row, self.mode) {
            (Row::Search, _) => {
                self.asking = Some(Asking::Search);
                return Outcome::Prompt("Search the Sounds".into(), self.search.clone());
            }
            (Row::Sound(i), _) => {
                self.mode = Mode::Actions(i);
                self.b.reset_cursor();
            }
            (Row::Load, Mode::Actions(i)) => {
                if let Some(id) = id(i) {
                    self.out.push((Request::Do("load"), "parts.loadSound", json!({"part": self.id, "id": id})));
                }
            }
            (Row::NewPart, Mode::Actions(i)) => {
                if let Some(id) = id(i) {
                    self.out.push((Request::Do("fromSound"), "parts.fromSound", json!({"id": id})));
                }
            }
            (Row::Audition, Mode::Actions(i)) => {
                if let Some(id) = id(i) {
                    self.out.push((Request::Do("audition"), "audition.start", json!({"id": id, "part": self.id})));
                }
            }
            (Row::Stop, _) => self.out.push((Request::Do("audition"), "audition.stop", json!({}))),
            (Row::Save, _) => {
                // Over the Sound it came from: with no name and not as new, the next revision.
                self.out.push((Request::Do("saveSound"), "parts.saveSound", json!({"part": self.id, "new": false})));
            }
            (Row::SaveNew, _) => {
                self.asking = Some(Asking::SaveNew);
                return Outcome::Prompt("Name of the new Sound".into(), self.part().and_then(|p| p["name"].as_str()).unwrap_or_default().into());
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// The keyboard's text, after Done.
    pub fn entered(&mut self, text: &str) {
        match self.asking.take() {
            Some(Asking::Search) => {
                self.search = text.trim().to_string();
                self.b.reset_cursor();
                self.query();
            }
            Some(Asking::SaveNew) if !text.trim().is_empty() => {
                self.out.push((Request::Do("saveSound"), "parts.saveSound", json!({"part": self.id, "name": text.trim(), "new": true})));
            }
            _ => {}
        }
        self.show();
    }

    pub fn not_entered(&mut self) {
        self.asking = None;
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
                Mode::Actions(_) => {
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
