// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sound chooser for a part (the JUCE app's library Sounds, RFC 0021, hosts/standalone/LibraryPane.cpp in
//! its Pick mode): the library's Sounds (`library.query` of kind `sound`) narrowed as the reference's pane
//! narrows them: a search text typed on the on-screen keyboard (name, description or tags), Favorites (only the
//! starred), Recent (only the ones used lately, the latest first), the library they come from (everything, my
//! own, or an installed library, from `library.libraries`) and tags (all of the chosen must match; the tags of
//! the matches listed with their counts, the chosen first, then the most used). Each Sound's row says its name
//! ("Untitled" without one), its tags and description, "Locked" when a library holds it, "Does not fit this
//! device" when the engine says so, "Auditioning" while it is tried, a star when it is a favorite, and its
//! color as the theme puts a row's color. A press on a Sound lists what can be done: load it into the part
//! (`parts.loadSound`: the chain is replaced, the seat kept), make a new part from it (`parts.fromSound`), try it
//! (`audition.start` for this part, `audition.stop`), add it to the favorites or take it out (`library.favorite`).
//! Saving the part as a Sound (`parts.saveSound`) is here too: as the JUCE page saves, over the Sound it came
//! from, or as a new one named on the keyboard. Each of these takes long and is refused while graph edits are
//! staged; the refusal is said. Without touch: the left knob or the dial moves the cursor, a press chooses (or
//! turns a filter on or off), Cancel goes back one layer.

use crate::browser::{Browser, Nav};
use crate::nav::Action;
use crate::setlist::color;
use crate::{InfoLine, SettingData, Shell};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Query,
    /// library.libraries: the installed libraries, for the Library filter.
    Libraries,
    /// Load, new part, audition, save, favorite: the answer says what happened.
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
    Filters,
    Sound(usize),
    Favorites,
    Recent,
    Library,
    Tags,
    Source(usize),
    Tag(usize),
    Load,
    NewPart,
    Audition,
    Stop,
    Favorite,
    Save,
    SaveNew,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    List,
    Actions(usize),
    /// Favorites, Recent, the library and the tags.
    Filters,
    Sources,
    Tags,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Asking {
    Search,
    SaveNew,
}

/// The reference's separator between the parts of a Sound's detail.
const SEPARATOR: &str = "  ·  ";

pub struct Screen {
    b: Browser,
    graph: Value,
    id: u64,
    mode: Mode,
    items: Vec<Value>,
    total: u64,
    /// The tags of the matches with their counts, as the engine gave them.
    tags: Vec<(String, u64)>,
    /// The installed libraries: id, name, items.
    libraries: Vec<(String, String, u64)>,
    search: String,
    favorites: bool,
    recent: bool,
    /// "" everything, "mine" the user's own, or an installed library's id.
    source: String,
    /// The tags chosen: all must match.
    chosen: Vec<String>,
    /// The Sound being tried, while it is.
    auditioning: String,
    asking: Option<Asking>,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen {
            b: Browser::new(ui),
            graph: Value::Null,
            id: 0,
            mode: Mode::List,
            items: vec![],
            total: 0,
            tags: vec![],
            libraries: vec![],
            search: String::new(),
            favorites: false,
            recent: false,
            source: String::new(),
            chosen: vec![],
            auditioning: String::new(),
            asking: None,
            out: vec![],
        }
    }

    pub fn at_top(&self) -> bool {
        self.b.at_top()
    }

    pub fn open(&mut self, id: u32) {
        self.id = id as u64;
        self.mode = Mode::List;
        self.search.clear();
        self.favorites = false;
        self.recent = false;
        self.source.clear();
        self.chosen.clear();
        self.b.reset();
        self.b.say("");
        self.query();
        self.out.push((Request::Libraries, "library.libraries", json!({})));
        self.show();
    }

    fn query(&mut self) {
        let mut params = json!({"kind": "sound", "limit": 1000});
        if !self.search.is_empty() {
            params["text"] = json!(self.search);
        }
        if !self.chosen.is_empty() {
            params["tags"] = json!(self.chosen);
        }
        if self.favorites {
            params["favorites"] = json!(true);
        }
        if self.recent {
            params["recent"] = json!(true);
        }
        if !self.source.is_empty() {
            params["source"] = json!(self.source);
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

    /// Whether anything narrows the list.
    fn narrowed(&self) -> bool {
        !self.search.is_empty() || !self.chosen.is_empty() || self.favorites || self.recent || !self.source.is_empty()
    }

    /// What the Filters row says: what is on, or none.
    fn filters_text(&self) -> String {
        let mut on = Vec::new();
        if self.favorites {
            on.push("Favorites".to_string());
        }
        if self.recent {
            on.push("Recent".into());
        }
        if !self.source.is_empty() {
            on.push(self.source_name(&self.source));
        }
        on.extend(self.chosen.iter().cloned());
        if on.is_empty() { "none".into() } else { on.join(", ") }
    }

    fn source_name(&self, id: &str) -> String {
        match id {
            "" => "Everything".into(),
            "mine" => "My own".into(),
            _ => self.libraries.iter().find(|l| l.0 == id).map_or(id.to_string(), |l| l.1.clone()),
        }
    }

    /// The libraries to choose from: everything, the user's own, then each installed one.
    fn sources(&self) -> Vec<(String, String, Option<u64>)> {
        let mut out = vec![(String::new(), "Everything".to_string(), None), ("mine".into(), "My own".into(), None)];
        out.extend(self.libraries.iter().map(|l| (l.0.clone(), l.1.clone(), Some(l.2))));
        out
    }

    /// The tags to choose from: the chosen first, then the most used (stable).
    fn tag_list(&self) -> Vec<(String, u64)> {
        let mut tags = self.tags.clone();
        tags.sort_by_key(|(t, count)| (!self.chosen.contains(t), std::cmp::Reverse(*count)));
        tags
    }

    /// A Sound's detail as the reference writes it: its tags and description, behind what is said first.
    fn detail(&self, s: &Value) -> String {
        let tags: Vec<&str> = s["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let description = s["description"].as_str().unwrap_or_default();
        let mut detail = tags.join(SEPARATOR);
        if !description.is_empty() {
            detail += &format!("{}{description}", if tags.is_empty() { "" } else { "   " });
        }
        let first = |what: &str, detail: String| if detail.is_empty() { what.to_string() } else { format!("{what}{SEPARATOR}{detail}") };
        if s["library"].as_str().is_some_and(|l| !l.is_empty()) {
            detail = first("Locked", detail);
        }
        if s["unfit"].as_str().is_some_and(|u| !u.is_empty()) {
            detail = first("Does not fit this device", detail);
        }
        if !self.auditioning.is_empty() && s["id"].as_str() == Some(&self.auditioning) {
            detail = first("Auditioning", detail);
        }
        if s["favorite"] == true {
            detail = if detail.is_empty() { "★".into() } else { format!("{detail}  ★") };
        }
        detail
    }

    fn rows(&self) -> Vec<(Row, SettingData)> {
        let row = |label: String, value: String, kind: i32| SettingData { label: label.into(), value: value.into(), kind, ..Default::default() };
        let toggle = |label: &str, on: bool| SettingData { label: label.into(), value: if on { "On" } else { "Off" }.into(), kind: 1, on, ..Default::default() };
        match self.mode {
            Mode::List => {
                let mut rows = vec![
                    (Row::Search, row("Search".into(), format!("{} ›", if self.search.is_empty() { "all" } else { &self.search }), 2)),
                    (Row::Filters, row("Filters".into(), format!("{} ›", self.filters_text()), 2)),
                ];
                rows.extend(self.items.iter().enumerate().map(|(i, s)| {
                    let name = s["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("Untitled");
                    let tint = s["color"].as_str().and_then(color);
                    (Row::Sound(i), SettingData { tint: tint.unwrap_or_default(), tinted: tint.is_some(), ..row(name.into(), self.detail(s), 2) })
                }));
                rows
            }
            Mode::Filters => vec![
                (Row::Favorites, toggle("Favorites", self.favorites)),
                (Row::Recent, toggle("Recent", self.recent)),
                (Row::Library, row("Library".into(), format!("{} ›", self.source_name(&self.source)), 2)),
                (Row::Tags, row("Tags".into(), format!("{} ›", if self.chosen.is_empty() { "none".to_string() } else { self.chosen.join(", ") }), 2)),
            ],
            Mode::Sources => self
                .sources()
                .into_iter()
                .enumerate()
                .map(|(i, (id, name, items))| {
                    let on = id == self.source;
                    let count = items.map(|n| n.to_string()).unwrap_or_default();
                    (Row::Source(i), SettingData { value: if on { "Chosen".into() } else { count.into() }, on, ..toggle(&name, on) })
                })
                .collect(),
            Mode::Tags => {
                let tags = self.tag_list();
                if tags.is_empty() {
                    return vec![(Row::Nothing, row("No tags".into(), String::new(), 4))];
                }
                tags.into_iter()
                    .enumerate()
                    .map(|(i, (tag, count))| {
                        let on = self.chosen.contains(&tag);
                        (Row::Tag(i), SettingData { value: if on { format!("✓ {count}") } else { count.to_string() }.into(), on, ..toggle(&tag, on) })
                    })
                    .collect()
            }
            Mode::Actions(i) => {
                let from = self.part().is_some_and(|p| p["from"].is_object());
                let favorite = self.items.get(i).is_some_and(|s| s["favorite"] == true);
                vec![
                    (Row::Load, row("Load into this part".into(), String::new(), 2)),
                    (Row::NewPart, row("New part from it".into(), String::new(), 2)),
                    (Row::Audition, row("Audition in this part".into(), String::new(), 2)),
                    (Row::Stop, row("Stop auditioning".into(), String::new(), 2)),
                    (Row::Favorite, row(if favorite { "Remove from favorites" } else { "Add to favorites" }.into(), String::new(), 2)),
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
        lines.push(line(format!("{} of {} Sound{}", self.items.len(), self.total, if self.total == 1 { "" } else { "s" })));
        if self.total == 0 && !self.narrowed() {
            // The reference's empty state; an instrument's own presets are on its plugin's actions here.
            lines.push(line("No Sounds yet. An instrument's own presets are on the plugin's Presets action in the chain, not here.".into()));
        }
        if !self.auditioning.is_empty() {
            lines.push(line("Auditioning".into()));
        }
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
            Mode::Filters => "Sound: filters".into(),
            Mode::Sources => "Sound: library".into(),
            Mode::Tags => "Sound: tags".into(),
        };
        let rows = self.rows().into_iter().map(|r| r.1).collect();
        let info = self.info();
        let noun = match self.mode {
            Mode::List => "Sounds",
            Mode::Actions(_) => "Actions",
            Mode::Filters => "Filters",
            Mode::Sources => "Libraries",
            Mode::Tags => "Tags",
        };
        self.b.show(&name, &title, noun, rows, info);
    }

    /// Whether an edit was refused (said).
    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::Query, Ok(r)) => {
                self.items = r["items"].as_array().cloned().unwrap_or_default();
                self.total = r["total"].as_u64().unwrap_or(self.items.len() as u64);
                self.tags = r["tags"].as_array().into_iter().flatten().filter_map(|t| Some((t["tag"].as_str()?.to_string(), t["count"].as_u64().unwrap_or(0)))).collect();
            }
            (Request::Libraries, Ok(r)) => {
                self.libraries = r["libraries"].as_array().into_iter().flatten().filter_map(|l| Some((l["id"].as_str()?.to_string(), l["name"].as_str().unwrap_or_default().to_string(), l["items"].as_u64().unwrap_or(0)))).collect();
                // A library uninstalled while it was chosen: everything shows (the reference's rule).
                if !self.source.is_empty() && self.source != "mine" && !self.libraries.iter().any(|l| l.0 == self.source) {
                    self.source.clear();
                    self.query();
                }
            }
            (Request::Do(what), Ok(r)) => {
                let flagged = r["flagged"].as_array().map_or(0, Vec::len);
                match (what, flagged) {
                    ("audition", _) => {}
                    ("favorite", _) => self.query(), // the star shows at once: looked at again, as the reference's refresh
                    _ => {}
                }
                self.b.say(&match (what, flagged) {
                    ("load", 0) => "Loaded".to_string(),
                    ("load", n) => format!("Loaded; {n} wires or targets were removed"),
                    ("fromSound", _) => "A new part was made".into(),
                    ("saveSound", _) => "Saved".into(),
                    _ => String::new(),
                });
            }
            (Request::Query | Request::Libraries | Request::Do(_), Err(e)) if e.code != 0 => {
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
            (Row::Filters, _) => {
                self.mode = Mode::Filters;
                self.b.reset_cursor();
            }
            (Row::Favorites, _) => {
                self.favorites = !self.favorites;
                self.query();
            }
            (Row::Recent, _) => {
                self.recent = !self.recent;
                self.query();
            }
            (Row::Library, _) => {
                self.mode = Mode::Sources;
                self.b.reset_cursor();
                self.out.push((Request::Libraries, "library.libraries", json!({})));
            }
            (Row::Tags, _) => {
                self.mode = Mode::Tags;
                self.b.reset_cursor();
            }
            (Row::Source(i), _) => {
                if let Some((id, _, _)) = self.sources().get(i) {
                    self.source = id.clone();
                    self.mode = Mode::Filters;
                    self.b.reset_cursor();
                    self.query();
                }
            }
            (Row::Tag(i), _) => {
                // The tag chosen or let go; the list is asked again.
                if let Some((tag, _)) = self.tag_list().get(i).cloned() {
                    match self.chosen.iter().position(|t| *t == tag) {
                        Some(at) => {
                            self.chosen.remove(at);
                        }
                        None => self.chosen.push(tag),
                    }
                    self.query();
                }
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
                    self.auditioning = id.clone();
                    self.out.push((Request::Do("audition"), "audition.start", json!({"id": id, "part": self.id})));
                }
            }
            (Row::Stop, _) => {
                self.auditioning.clear();
                self.out.push((Request::Do("audition"), "audition.stop", json!({})));
            }
            (Row::Favorite, Mode::Actions(i)) => {
                if let Some(id) = id(i) {
                    let on = self.items.get(i).is_none_or(|s| s["favorite"] != true);
                    self.out.push((Request::Do("favorite"), "library.favorite", json!({"id": id, "favorite": on})));
                }
            }
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
                _ => {
                    self.mode = match self.mode {
                        Mode::Sources | Mode::Tags => Mode::Filters,
                        _ => Mode::List,
                    };
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
