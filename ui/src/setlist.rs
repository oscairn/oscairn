// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Setlist screen, from the prototype (branch experiment/rust-setlist) onto the shell: two
//! panes, the setlists on the left and the open setlist's slots on the right, each with a cursor
//! of its own (the panel's pane 0 and pane 1; the VALUE dial moves the pane last touched). A
//! setlist as docs/setlist-format.md has it (version 2); Previous and Next as
//! hosts/standalone/Setlist.cpp does them: slots numbered across pages, empty slots passed over,
//! a placeholder or a Performance not in the library stopped at. A slot asked for while another
//! opens opens next, the last one chosen winning (as the JUCE Engine queues library loads).
//! Slots show a view at a time: the cursor leaving it turns a whole view, never scrolling.

use crate::nav::{Action, Cursor};
use crate::{ListRow, SetlistState, Shell, SlotData};
use serde_json::{json, Value};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub empty: bool,
    pub id: String, // empty: a placeholder
    pub name: String,
    pub color: String,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub name: String,
    pub slots: Vec<Slot>,
}

#[derive(Clone, Debug)]
pub struct Setlist {
    pub id: String,
    pub name: String,
    pub grid: bool, // display.mode grid; list and scroll show one column
    pub columns: u32,
    pub rows: u32,
    pub pages: Vec<Page>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

impl Setlist {
    /// The document `setlists.get` returns. The engine has validated it; this only refuses
    /// what the screen cannot show.
    pub fn parse(doc: &Value) -> Result<Setlist, String> {
        if doc["format"] != "oscairn.setlist" || doc["version"] != 2 {
            return Err("not a version 2 setlist".into());
        }
        let display = &doc["display"];
        let side = |k: &str| display[k].as_u64().unwrap_or(1).clamp(1, 8) as u32;
        let slot = |x: &Value| match x {
            Value::Null => Slot { empty: true, ..Default::default() },
            x => Slot { empty: false, id: text(x, "id"), name: text(x, "name"), color: text(x, "color"), label: text(x, "label") },
        };
        let pages: Vec<Page> = doc["pages"]
            .as_array()
            .ok_or("no pages")?
            .iter()
            .map(|p| Page { name: text(p, "name"), slots: p["slots"].as_array().map(|s| s.iter().map(slot).collect()).unwrap_or_default() })
            .collect();
        if pages.is_empty() {
            return Err("no pages".into());
        }
        Ok(Setlist { id: text(&doc["info"], "id"), name: text(&doc["info"], "name"), grid: display["mode"] == "grid", columns: side("columns"), rows: side("rows"), pages })
    }

    pub fn count(&self) -> usize {
        self.pages.iter().map(|p| p.slots.len()).sum()
    }
    /// The slot index of a page's first slot.
    pub fn first(&self, page: usize) -> usize {
        self.pages.iter().take(page).map(|p| p.slots.len()).sum()
    }
    /// The page a slot is on, and the slot.
    pub fn find(&self, mut index: usize) -> Option<(usize, &Slot)> {
        for (n, p) in self.pages.iter().enumerate() {
            if index < p.slots.len() {
                return Some((n, &p.slots[index]));
            }
            index -= p.slots.len();
        }
        None
    }
    pub fn slot(&self, index: usize) -> Option<&Slot> {
        self.find(index).map(|f| f.1)
    }
    /// Where Previous (-1) or Next (+1) goes from `current`: the next slot that is not empty.
    pub fn step(&self, current: Option<usize>, by: i64) -> Option<usize> {
        let n = self.count() as i64;
        let mut i = match current {
            Some(c) => c as i64 + by,
            None if by > 0 => 0,
            None => n - 1,
        };
        while i >= 0 && i < n {
            if !self.slot(i as usize)?.empty {
                return Some(i as usize);
            }
            i += by;
        }
        None
    }
}

/// What the screen asks the engine, so its answer comes back to it.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Setlists,
    Get(String),
    Performances,
    /// A slot opened, by its index in the setlist.
    Load(usize),
    Prepare,
}

/// A request to send: what it is for, its method and parameters.
pub type Out = (Request, &'static str, Value);

/// What the screen asks of the shell after an action.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Cancel with no cursor: back Home.
    Back,
}

struct Performance {
    name: String,
    color: String,
    path: String,
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    setlists: Vec<(String, String)>, // id, name
    lists: Cursor,                    // pane 0
    setlist: Option<Setlist>,
    performances: HashMap<String, Performance>,
    whole: bool, // every Performance listed: one not among them is missing
    page: usize,
    slots: Cursor, // pane 1, over the page's slots, a view at a time
    /// The pane the dial moves: the one last touched (0 the setlists, 1 the slots).
    touched: u8,
    current: Option<usize>,
    opening: bool,
    /// The slot the engine is opening (`current` may already be a later one waiting).
    opening_index: Option<usize>,
    queued: Option<usize>,
    /// A load the engine refused as busy is to be asked again after `BUSY_RETRY` (the shell sets the timer).
    retry_wanted: bool,
    names: Rc<VecModel<ListRow>>,
    rows: Rc<VecModel<SlotData>>,
    state_file: Option<PathBuf>,
    restore: Option<(String, Option<usize>)>,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
}

/// The state the JUCE Setlist app keeps (its AppData/oscairn.setlist/state.json), shared with it:
/// the setlist open and the slot playing.
fn read_state(file: &std::path::Path) -> Option<(String, Option<usize>)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    Some((v["setlist"].as_str()?.to_string(), v["current"].as_u64().map(|c| c as usize)))
}

pub(crate) fn color(hex: &str) -> Option<Color> {
    let v = u32::from_str_radix(hex.strip_prefix('#').filter(|h| h.len() == 6)?, 16).ok()?;
    Some(Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Sets a model's rows to `want`, each row only if it changed: a cursor move sets two.
pub fn update<T: Clone + PartialEq + 'static>(model: &VecModel<T>, want: Vec<T>) {
    if model.row_count() != want.len() {
        model.set_vec(want);
        return;
    }
    for (i, row) in want.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&row) {
            model.set_row_data(i, row);
        }
    }
}

impl Screen {
    pub fn new(window: &Shell, state_file: Option<PathBuf>) -> Screen {
        let names = Rc::new(VecModel::default());
        let rows = Rc::new(VecModel::default());
        let state = window.global::<SetlistState>();
        state.set_setlists(ModelRc::from(names.clone()));
        state.set_slots(ModelRc::from(rows.clone()));
        let restore = state_file.as_deref().and_then(read_state);
        Screen {
            ui: window.as_weak(),
            setlists: vec![],
            lists: Cursor::new(0, 1),
            setlist: None,
            performances: HashMap::new(),
            whole: false,
            page: 0,
            slots: Cursor::new(0, 1),
            touched: 1,
            current: None,
            opening: false,
            opening_index: None,
            queued: None,
            retry_wanted: false,
            names,
            rows,
            state_file,
            restore,
            out: vec![],
        }
    }

    fn view(&self, f: impl FnOnce(&SetlistState)) {
        if let Some(ui) = self.ui.upgrade() {
            f(&ui.global::<SetlistState>())
        }
    }

    fn say(&self, text: impl Into<SharedString>) {
        let text = text.into();
        self.view(|s| s.set_message(text));
    }

    /// Connected: the setlists and the Performances read again.
    pub fn connected(&mut self) {
        self.opening = false;
        self.queued = None;
        self.retry_wanted = false;
        self.out.push((Request::Setlists, "library.query", json!({"type": "oscairn.setlist", "limit": 1000})));
        self.out.push((Request::Performances, "library.query", json!({"kind": "performance", "limit": 1000})));
    }

    /// Shown: the views' sizes may have changed while it was not.
    pub fn shown(&mut self) {
        self.show();
    }

    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) {
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                match request {
                    Request::Load(i) if e.code == crate::client::BUSY => {
                        // Another client's long request: this one waits its turn, asked again in a moment, as a newer
                        // press would (the last chosen wins). It stays opening: the chip and the slot's bar stay.
                        self.queued = self.queued.or(Some(i));
                        self.retry_wanted = true;
                    }
                    Request::Load(_) => {
                        self.opening = false;
                        self.show();
                        self.say(format!("Did not open: {}", e.message));
                        self.next_queued();
                    }
                    Request::Prepare => {}
                    _ => self.say(crate::client::refusal(&e)),
                }
                return;
            }
        };
        match request {
            Request::Setlists => {
                self.setlists = r["items"].as_array().map(|items| items.iter().map(|i| (text(i, "id"), text(i, "name"))).collect()).unwrap_or_default();
                let open = self.setlist.as_ref().map(|s| s.id.clone()).or(self.restore.as_ref().map(|r| r.0.clone()));
                let at = self.setlists.iter().position(|s| Some(&s.0) == open.as_ref()).unwrap_or(0);
                self.lists = Cursor { at: self.lists.at.map(|a| a.min(self.setlists.len().saturating_sub(1))), count: self.setlists.len(), ..self.lists };
                match self.setlists.get(at) {
                    Some((id, _)) => {
                        let id = id.clone();
                        self.out.push((Request::Get(id.clone()), "setlists.get", json!({"id": id})));
                    }
                    None => self.say("No setlists in the library."),
                }
                self.show();
            }
            Request::Performances => {
                let items = r["items"].as_array().cloned().unwrap_or_default();
                self.whole = r["total"].as_u64().unwrap_or(u64::MAX) <= items.len() as u64;
                self.performances = items.iter().map(|i| (text(i, "id"), Performance { name: text(i, "name"), color: text(i, "color"), path: text(i, "path") })).collect();
                self.show();
            }
            Request::Get(id) => match Setlist::parse(&r["setlist"]) {
                Ok(s) => {
                    let other = self.setlist.as_ref().is_none_or(|o| o.id != s.id);
                    // Started again (a crash, the display handed back): the slot playing, marked, not opened.
                    let restored = self.restore.take().filter(|r| r.0 == s.id).and_then(|r| r.1).filter(|c| *c < s.count());
                    if other {
                        self.current = restored;
                        self.page = restored.and_then(|c| s.find(c)).map_or(0, |f| f.0);
                        self.slots = Cursor::new(0, 1);
                        self.say(format!("{} slots", s.count()));
                    }
                    self.page = self.page.min(s.pages.len() - 1);
                    self.setlist = Some(s);
                    self.show();
                    self.keep();
                }
                Err(e) => self.say(format!("Cannot show setlist {id}: {e}")),
            },
            Request::Load(_) => {
                self.opening = false;
                self.show();
                if !self.next_queued() {
                    self.prepare_next();
                }
            }
            Request::Prepare => {}
        }
    }

    /// Whether a busy load is to be asked again in `BUSY_RETRY` (taken: the shell sets the timer).
    pub fn take_retry(&mut self) -> bool {
        std::mem::take(&mut self.retry_wanted)
    }

    /// The timer: the load the engine refused is asked again, or the slot chosen since.
    pub fn retry(&mut self) {
        self.opening = false;
        if !self.next_queued() {
            self.show();
        }
    }

    /// The slot waiting while another opened, now.
    fn next_queued(&mut self) -> bool {
        match self.queued.take() {
            Some(next) => {
                self.go(next);
                true
            }
            None => false,
        }
    }

    /// The slot the engine is opening now (asked for, not answered).
    fn opening_at(&self) -> Option<usize> {
        self.opening_index.filter(|_| self.opening)
    }

    /// What is opening, for the top bar's loading chip: the slot's title.
    pub fn opening_name(&self) -> Option<String> {
        self.opening_at().map(|i| self.title(i))
    }

    fn opens(&self, index: usize) -> bool {
        let Some(slot) = self.setlist.as_ref().and_then(|s| s.slot(index)) else { return false };
        !slot.empty && !slot.id.is_empty() && (!self.whole || self.performances.contains_key(&slot.id))
    }

    fn title(&self, index: usize) -> String {
        let Some(slot) = self.setlist.as_ref().and_then(|s| s.slot(index)) else { return String::new() };
        if !slot.label.is_empty() {
            return slot.label.clone();
        }
        let name = self.performances.get(&slot.id).map(|p| p.name.as_str()).filter(|n| !n.is_empty()).unwrap_or(&slot.name);
        if name.is_empty() { "Untitled".into() } else { name.into() }
    }

    fn row(&self, index: usize, cursor: bool) -> SlotData {
        let Some(slot) = self.setlist.as_ref().and_then(|s| s.slot(index)) else { return SlotData::default() };
        let own = if slot.color.is_empty() { self.performances.get(&slot.id).map_or("", |p| p.color.as_str()) } else { &slot.color };
        let tint = color(own);
        let note = if slot.empty || self.opens(index) { "" } else if slot.id.is_empty() { "(placeholder)" } else { "(not in the library)" };
        SlotData {
            number: index as i32 + 1,
            title: self.title(index).into(),
            note: note.into(),
            tint: tint.unwrap_or_default(),
            tinted: tint.is_some(),
            empty: slot.empty,
            opens: self.opens(index),
            playing: self.current == Some(index),
            opening: self.opening_at() == Some(index),
            cursor,
        }
    }

    /// The window's view of it, each row set only when it changed.
    fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let state = ui.global::<SetlistState>();
        // The views' sizes, which the window works out (rows of setlists; slots a view holds).
        let names: Vec<ListRow> = self.setlists.iter().enumerate().map(|(i, s)| ListRow { name: s.1.as_str().into(), cursor: self.lists.at == Some(i), shown: self.setlist.as_ref().is_some_and(|o| o.id == s.0) }).collect();
        self.lists = Cursor { per_page: state.get_list_rows().max(1) as usize, ..self.lists };
        let first_name = self.lists.first;
        update(&self.names, names.into_iter().skip(first_name).take(self.lists.per_page).collect());
        let Some(s) = self.setlist.as_ref() else { return };
        state.set_name(if s.name.is_empty() { "Untitled".into() } else { s.name.as_str().into() });
        let page = &s.pages[self.page];
        let name = if page.name.is_empty() { format!("Page {}", self.page + 1) } else { page.name.clone() };
        state.set_page(format!("{name}  ({}/{})", self.page + 1, s.pages.len()).into());
        state.set_grid(s.grid);
        state.set_columns(s.columns as i32);
        state.set_rows(s.rows as i32);
        let per_view = state.get_slots_per_view().max(1) as usize;
        self.slots = Cursor { count: page.slots.len(), per_page: per_view, ..self.slots };
        if let Some(at) = self.slots.at {
            self.slots = self.slots.to(at); // the view follows the cursor
        }
        let start = s.first(self.page) + self.slots.first;
        let end = s.first(self.page) + page.slots.len().min(self.slots.first + per_view);
        let rows: Vec<SlotData> = (start..end).map(|i| self.row(i, self.slots.at.map(|a| s.first(self.page) + a) == Some(i))).collect();
        update(&self.rows, rows);
        state.set_view(format!("{}–{} of {}", self.slots.first + 1, self.slots.first + (end - start), page.slots.len()).into());
        state.set_can_previous(self.current.is_some() && s.step(self.current, -1).is_some());
        state.set_can_next(s.step(self.current, 1).is_some());
    }

    fn keep(&self) {
        let (Some(file), Some(s)) = (self.state_file.as_ref(), self.setlist.as_ref()) else { return };
        let temporary = file.with_extension("json.new");
        let doc = json!({"setlist": s.id, "current": self.current.map_or(-1, |c| c as i64)}).to_string();
        let written = file.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(&temporary, doc).and_then(|_| std::fs::rename(&temporary, file)).is_ok();
        if !written {
            eprintln!("oscairn-ui: cannot keep the Setlist's state in {}", file.display());
        }
    }

    /// Opens the slot `index`: now, or once the one opening has (the last chosen wins).
    fn go(&mut self, index: usize) {
        let Some((page, slot)) = self.setlist.as_ref().and_then(|s| s.find(index)).map(|(p, s)| (p, s.clone())) else { return };
        if slot.empty {
            return;
        }
        if page != self.page {
            self.page = page;
            self.slots = Cursor::new(0, 1);
        }
        if !self.opens(index) {
            let why = if slot.id.is_empty() { "is a placeholder: it has no Performance" } else { "is not in the library" };
            self.say(format!("{} {why}.", self.title(index)));
            self.show();
            return;
        }
        self.current = Some(index);
        self.keep();
        if self.opening {
            self.queued = Some(index);
            self.say(format!("{}. {} opens once the last one has.", index + 1, self.title(index)));
        } else {
            self.queued = None;
            self.opening = true;
            self.opening_index = Some(index);
            self.out.push((Request::Load(index), "library.load", json!({"id": slot.id})));
            self.say(format!("{}. {}", index + 1, self.title(index)));
        }
        self.show();
    }

    /// The next one, loaded while this one plays, so going to it only switches (RFC 0021).
    fn prepare_next(&mut self) {
        let Some(s) = self.setlist.as_ref() else { return };
        let Some(next) = s.step(self.current, 1).filter(|n| self.opens(*n)) else { return };
        let Some(path) = s.slot(next).and_then(|x| self.performances.get(&x.id)).map(|p| p.path.clone()) else { return };
        self.out.push((Request::Prepare, "performance.prepare", json!({"directory": path})));
    }

    fn step(&mut self, by: i64) {
        if let Some(i) = self.setlist.as_ref().and_then(|s| s.step(self.current, by)) {
            self.go(i);
        }
    }

    fn open_setlist(&mut self, at: usize) {
        if let Some((id, _)) = self.setlists.get(at) {
            let id = id.clone();
            self.out.push((Request::Get(id.clone()), "setlists.get", json!({"id": id})));
        }
    }

    /// The slots' cursor by `by`, onto the setlist's next or last page past either end of this one.
    fn move_slots(&mut self, by: i64) {
        let Some(s) = self.setlist.as_ref() else { return };
        let count = s.pages[self.page].slots.len() as i64;
        let at = self.slots.at.map(|a| a as i64);
        match at {
            Some(a) if a + by >= count && self.page + 1 < s.pages.len() => {
                self.page += 1;
                self.slots = Cursor { count: s.pages[self.page].slots.len(), ..self.slots }.to(0);
            }
            Some(a) if a + by < 0 && self.page > 0 => {
                self.page -= 1;
                let count = s.pages[self.page].slots.len();
                self.slots = Cursor { count, ..self.slots }.to(count.saturating_sub(1));
            }
            _ => self.slots = Cursor { count: count as usize, ..self.slots }.moved(by),
        }
    }

    /// A navigation action; Back for Cancel with no cursor.
    pub fn act(&mut self, action: Action) -> Outcome {
        let pane = |p: Option<u8>, touched: u8| p.map_or(touched, |p| p.min(1));
        match action {
            Action::Move { pane: p, by } => {
                self.touched = pane(p, self.touched);
                if self.touched == 0 {
                    self.lists = self.lists.moved(by as i64);
                } else {
                    self.move_slots(by as i64);
                }
            }
            // As the JUCE Setlist: up and down move the slots' cursor; left, right and the page
            // keys are Previous and Next (page turners and foot switches send them).
            Action::Arrow { dy, dx: 0 } => {
                self.touched = 1;
                let across = if self.setlist.as_ref().is_some_and(|s| s.grid) { self.setlist.as_ref().map_or(1, |s| s.columns as i64) } else { 1 };
                self.move_slots(dy as i64 * across);
            }
            Action::Arrow { dx, .. } | Action::Page(dx) => self.step(dx as i64),
            Action::End(by) => {
                self.touched = 1;
                self.move_slots(by as i64 * 10_000);
            }
            Action::Confirm(p) => match pane(p, self.touched) {
                0 => {
                    if let Some(at) = self.lists.at {
                        self.lists = self.lists.clear();
                        self.open_setlist(at);
                    }
                }
                _ => {
                    if let (Some(at), Some(s)) = (self.slots.at, self.setlist.as_ref()) {
                        let index = s.first(self.page) + at;
                        self.go(index);
                    }
                }
            },
            Action::Cancel(_) => {
                if self.lists.at.is_none() && self.slots.at.is_none() {
                    return Outcome::Back;
                }
                self.lists = self.lists.clear();
                self.slots = self.slots.clear();
            }
        }
        self.show();
        Outcome::Stay
    }

    /// A tap on a slot of the view, or a setlist of the list shown: what Confirm does, the cursors gone.
    pub fn tap_slot(&mut self, row: usize) {
        self.slots = self.slots.clear();
        let Some(s) = self.setlist.as_ref() else { return };
        let index = s.first(self.page) + self.slots.first + row;
        self.go(index);
    }

    pub fn tap_setlist(&mut self, row: usize) {
        self.lists = self.lists.clear();
        self.open_setlist(self.lists.first + row);
        self.show();
    }

    pub fn step_by(&mut self, by: i64) {
        self.step(by);
    }

    /// The screen's own action the top bar carries: Play, which opens Play (the footer's button
    /// plays it by touch; the bar's cursor reaches it without).
    pub fn bar_tools(&self) -> Vec<crate::topbar::Tool> {
        vec![crate::topbar::Tool::new("Play")]
    }

    /// The Performance's library ID in the slot playing, as the JUCE Setlist sends it to Play.
    pub fn playing(&self) -> Option<&str> {
        self.setlist.as_ref()?.slot(self.current?).map(|s| s.id.as_str()).filter(|id| !id.is_empty())
    }

    /// The cursors drop when the screen goes, as on focus loss in the JUCE lists.
    pub fn hidden(&mut self) {
        self.lists = self.lists.clear();
        self.slots = self.slots.clear();
        self.show();
    }

    /// Requests for the engine still to answer that the musician waits on: a slot opening.
    pub fn waits(request: &Request) -> bool {
        matches!(request, Request::Load(_) | Request::Get(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Setlist {
        let doc: Value = serde_json::from_str(r##"{"format":"oscairn.setlist","version":2,"info":{"id":"s1","name":"Friday"},
            "display":{"mode":"grid","columns":4,"rows":3},
            "pages":[{"name":"Set 1","slots":[{"id":"a","name":"A","color":"#e5484d","label":""},null,{"id":"b","name":"B","color":"","label":"Opener"}]},
                     {"name":"Encore","slots":[null,{"id":"","name":"Piano","color":"","label":""}]}]}"##).unwrap();
        Setlist::parse(&doc).unwrap()
    }

    #[test]
    fn parses_pages_slots_empty_and_placeholders() {
        let s = sample();
        assert_eq!((s.name.as_str(), s.grid, s.columns, s.rows, s.pages.len(), s.count()), ("Friday", true, 4, 3, 2, 5));
        assert!(s.slot(1).unwrap().empty);
        assert_eq!(s.slot(2).unwrap().label, "Opener");
        assert!(s.slot(4).unwrap().id.is_empty() && !s.slot(4).unwrap().empty);
        assert_eq!((s.first(1), s.find(3).map(|f| f.0), s.find(5).map(|f| f.0)), (3, Some(1), None));
    }

    #[test]
    fn steps_skip_empty_slots_across_pages() {
        let s = sample();
        assert_eq!(s.step(None, 1), Some(0));
        assert_eq!(s.step(Some(0), 1), Some(2)); // over the empty slot
        assert_eq!(s.step(Some(2), 1), Some(4)); // across the page, over its empty first slot
        assert_eq!(s.step(Some(4), 1), None);
        assert_eq!(s.step(None, -1), Some(4));
        assert_eq!(s.step(Some(2), -1), Some(0));
        assert_eq!(s.step(Some(0), -1), None);
    }

    // What the engine should never send is shown as best it can be, never a panic.
    #[test]
    fn odd_documents_do_not_panic() {
        let odd: Value = serde_json::from_str(r#"{"format":"oscairn.setlist","version":2,"info":{},"display":{"columns":99},
            "pages":[{"slots":7},{"name":3,"slots":[null,null]},{"slots":[42,{"id":5}]}]}"#).unwrap();
        let s = Setlist::parse(&odd).unwrap();
        assert_eq!((s.columns, s.rows, s.count()), (8, 1, 4));
        assert_eq!(s.step(None, 1), Some(2)); // 42: a slot without an ID, a placeholder
        let empty: Value = serde_json::from_str(r#"{"format":"oscairn.setlist","version":2,"info":{},"display":{},"pages":[{"slots":[null]}]}"#).unwrap();
        let s = Setlist::parse(&empty).unwrap();
        assert_eq!((s.step(None, 1), s.step(None, -1), s.step(Some(0), 1)), (None, None, None));
        for bad in [r#"{"format":"oscairn.setlist","version":2,"info":{},"display":{},"pages":[]}"#, r#"null"#, r#"{"format":"oscairn.setlist","version":1}"#] {
            assert!(Setlist::parse(&serde_json::from_str(bad).unwrap()).is_err());
        }
    }
}
