// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Setlist screen, from the prototype (branch experiment/rust-setlist) onto the shell: under
//! the top bar one row, the setlist bar (a dropdown of the setlists, a dropdown of the open
//! setlist's banks, its `pages`, then Previous and Next, which show the bank before or after and
//! open nothing), and under it the shown bank's slots over the whole width. One cursor: on the
//! bar's stops (the panel's pane 0) or on the slots (pane 1); the VALUE dial and the arrows walk
//! from one into the other. A setlist as docs/setlist-format.md has it (version 2); stepping to
//! the previous or next song (`step_by`: Play's buttons, the page keys) as
//! hosts/standalone/Setlist.cpp does it: slots numbered across pages, empty slots passed over,
//! a placeholder or a Performance not in the library stopped at. A slot asked for while another
//! opens opens next, the last one chosen winning (as the JUCE Engine queues library loads).
//! Slots show a view at a time: the cursor leaving it turns a whole view, never scrolling.

use crate::menu::Item;
use crate::nav::{Action, Cursor};
use crate::{BarStop, SetlistState, Shell, SlotData};
use serde_json::{json, Value};
use slint::{Color, ComponentHandle, Model, ModelRc, VecModel};
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

/// The setlist bar's stops, from the left: the setlists' dropdown, the banks' dropdown, Previous, Next.
const STOPS: usize = 4;

/// A bank's name as its dropdown says it: the page's own, or its number.
fn bank_name(s: &Setlist, page: usize) -> String {
    let name = &s.pages[page].name;
    if name.is_empty() { format!("Bank {}", page + 1) } else { name.clone() }
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
    bar: Cursor,                      // pane 0: the setlist bar's stops (`STOPS`)
    setlist: Option<Setlist>,
    performances: HashMap<String, Performance>,
    whole: bool, // every Performance listed: one not among them is missing
    page: usize,
    slots: Cursor, // pane 1, over the page's slots, a view at a time
    /// The pane the dial moves: the one the cursor is in (0 the setlist bar, 1 the slots).
    touched: u8,
    /// A dropdown's list for the shell to open as the menu, and whether the one asked for is the banks'.
    menu: Option<(String, Vec<Item>, usize)>, // its title, its rows, the row the cursor starts on (the one chosen now)
    banks_menu: bool,
    /// The slot stepped from (Play's Previous and Next): the one chosen here, or the first holding the engine's open
    /// Performance. Every slot holding the same Performance is marked as playing.
    current: Option<usize>,
    /// The engine's open Performance (`performance.info`'s library `id`; empty: none from the library), once reported.
    open: Option<String>,
    opening: bool,
    /// The slot the engine is opening (`current` may already be a later one waiting).
    opening_index: Option<usize>,
    queued: Option<usize>,
    /// A load the engine refused as busy is to be asked again after `BUSY_RETRY` (the shell sets the timer).
    retry_wanted: bool,
    stops: Rc<VecModel<BarStop>>,
    rows: Rc<VecModel<SlotData>>,
    state_file: Option<PathBuf>,
    restore: Option<(String, Option<usize>)>,
    /// Requests for the shell to send.
    pub out: Vec<Out>,
    /// What it has to say (why a slot did not open, a refusal), for the shell to show.
    pub said: Option<String>,
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

/// `a` and `b` mixed `f` toward `b` (as Slint's `Color.mix`): `(1-f)*a + f*b`.
fn mix(a: Color, b: Color, f: f32) -> Color {
    let l = |c: u8| f32::from(c);
    Color::from_rgb_u8(
        (l(a.red()) * (1. - f) + l(b.red()) * f).round() as u8,
        (l(a.green()) * (1. - f) + l(b.green()) * f).round() as u8,
        (l(a.blue()) * (1. - f) + l(b.blue()) * f).round() as u8,
    )
}

/// A placeholder name's ink: the theme's secondary (muted) wherever it reads on the box (3 to 1, large text), else the
/// ink that does (white or black on the box), so a placeholder never relies on its color to be read.
pub(crate) fn placeholder_ink(fill: Color, muted: Color) -> Color {
    if crate::palette::contrast(muted, fill) >= 3. {
        muted
    } else {
        crate::theme::white_first(fill)
    }
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
        let stops = Rc::new(VecModel::default());
        let rows = Rc::new(VecModel::default());
        let state = window.global::<SetlistState>();
        state.set_bar(ModelRc::from(stops.clone()));
        state.set_slots(ModelRc::from(rows.clone()));
        let restore = state_file.as_deref().and_then(read_state);
        Screen {
            ui: window.as_weak(),
            setlists: vec![],
            bar: Cursor::new(STOPS, STOPS),
            setlist: None,
            performances: HashMap::new(),
            whole: false,
            page: 0,
            slots: Cursor::new(0, 1),
            touched: 1,
            menu: None,
            banks_menu: false,
            current: None,
            open: None,
            opening: false,
            opening_index: None,
            queued: None,
            retry_wanted: false,
            stops,
            rows,
            state_file,
            restore,
            out: vec![],
            said: None,
        }
    }

    /// Something to tell the musician: the shell says it where it says such things (the top bar, for a while).
    fn say(&mut self, text: impl Into<String>) {
        self.said = Some(text.into());
    }

    /// Connected: the setlists and the Performances read again.
    pub fn connected(&mut self) {
        self.opening = false;
        self.queued = None;
        self.retry_wanted = false;
        self.out.push((Request::Setlists, "library.query", json!({"type": "oscairn.setlist", "limit": 1000})));
        self.out.push((Request::Performances, "library.query", json!({"kind": "performance", "limit": 1000})));
    }

    /// Shown: on the bank of the slot playing, if one is; the views' sizes may have changed while it was not.
    pub fn shown(&mut self) {
        if let Some(page) = self.current_page() {
            self.show_bank(page);
        }
        self.show();
    }

    fn current_page(&self) -> Option<usize> {
        self.setlist.as_ref()?.find(self.current?).map(|f| f.0)
    }

    /// The engine's open Performance (`performance.info`'s `id`), reported whenever it may have changed: the marks follow it.
    pub fn open_performance(&mut self, id: &str) {
        self.open = Some(id.to_string());
        self.follow();
        self.show();
    }

    /// `current` onto the engine's open Performance: kept while its slot holds it, else the first slot that does, else
    /// none. Not while a slot opens here (the engine still has the one before).
    fn follow(&mut self) {
        let (Some(open), Some(s)) = (self.open.as_deref(), self.setlist.as_ref()) else { return };
        if self.opening || self.queued.is_some() {
            return;
        }
        let holds = |i: usize| s.slot(i).is_some_and(|x| !x.id.is_empty() && x.id == open);
        if self.current.is_some_and(holds) {
            return;
        }
        let now = (0..s.count()).find(|i| holds(*i));
        if now != self.current {
            self.current = now;
            self.keep();
        }
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
                        self.follow(); // the engine's Performance is still the one before
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
                self.open_setlist(at); // with none, the dropdown says "No setlist"
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
                        self.slots = Cursor::new(0, 1);
                    }
                    self.page = self.page.min(s.pages.len() - 1);
                    self.setlist = Some(s);
                    self.follow();
                    if other {
                        self.page = self.current_page().unwrap_or(0);
                    }
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
        let placeholder = !slot.empty && slot.id.is_empty();
        // A placeholder's name: the theme's secondary ink where that reads on its box, else the ink that does. The box and
        // the ink drawn are derived in setlist.slint so they follow a theme switch; this is the same reckoning.
        let ink = if placeholder {
            let (surface, muted, disabled) = self.ui.upgrade().map_or(
                (Color::default(), Color::default(), 1.),
                |u| {
                    let t = u.global::<crate::Theme>();
                    (t.get_surface(), t.get_muted(), t.get_disabled())
                }
            );
            let fill = mix(tint.unwrap_or(surface), surface, 1. - disabled);
            placeholder_ink(fill, muted)
        } else {
            tint.map(crate::theme::white_first).unwrap_or_default()
        };
        SlotData {
            number: index as i32 + 1,
            title: self.title(index).into(),
            note: note.into(),
            tint: tint.unwrap_or_default(),
            tinted: tint.is_some(),
            ink,
            placeholder,
            empty: slot.empty,
            opens: self.opens(index),
            // Every slot holding the Performance playing (a song twice in the setlist is marked twice).
            playing: self.current == Some(index) || (!slot.id.is_empty() && self.playing() == Some(slot.id.as_str())),
            opening: self.opening_at() == Some(index),
            cursor,
        }
    }

    /// The window's view of it, each row set only when it changed.
    fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let state = ui.global::<SetlistState>();
        // The setlist bar: the two dropdowns say what is open, Previous and Next are off at the first and last bank.
        let pages = self.setlist.as_ref().map_or(0, |s| s.pages.len());
        let open = self.setlist.as_ref().map(|s| if s.name.is_empty() { "Untitled" } else { s.name.as_str() });
        let bank = self.setlist.as_ref().map_or(String::new(), |s| format!("{}  {}/{pages}", bank_name(s, self.page), self.page + 1));
        let stop = |i: usize, text: &str, drop: bool, enabled: bool| BarStop { text: text.into(), drop, enabled, cursor: self.bar.at == Some(i) };
        let stops = vec![
            stop(0, open.unwrap_or("No setlist"), true, !self.setlists.is_empty()),
            stop(1, &bank, true, pages > 1),
            stop(2, "Previous", false, self.page > 0),
            stop(3, "Next", false, self.page + 1 < pages),
        ];
        update(&self.stops, stops);
        let Some(s) = self.setlist.as_ref() else { return };
        state.set_name(if s.name.is_empty() { "Untitled".into() } else { s.name.as_str().into() });
        let page = &s.pages[self.page];
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
        state.set_count(page.slots.len() as i32);
        state.set_view(format!("{}–{} of {}", self.slots.first + 1, self.slots.first + (end - start), page.slots.len()).into());
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

    /// The slots' cursor by `by`, onto the setlist's next or last page past either end of this one; `joined` (the
    /// dial, the arrows): up out of the first bank's first slot onto the setlist bar's last stop. The bar's drops: one cursor.
    fn move_slots(&mut self, by: i64, joined: bool) {
        let Some(s) = self.setlist.as_ref() else { return };
        self.touched = 1;
        self.bar = self.bar.clear();
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
            Some(a) if a + by < 0 && joined => {
                self.slots = self.slots.clear();
                self.touched = 0;
                self.bar = self.bar.to(STOPS - 1);
            }
            _ => self.slots = Cursor { count: count as usize, ..self.slots }.moved(by),
        }
    }

    /// The setlist bar's cursor by `by` (the slots' drops: one cursor); `joined`: down past its last stop into the slots.
    fn move_bar(&mut self, by: i64, joined: bool) {
        if joined && by > 0 && self.bar.at == Some(STOPS - 1) && self.setlist.is_some() {
            return self.move_slots(by, false);
        }
        self.touched = 0;
        self.slots = self.slots.clear();
        self.bar = self.bar.moved(by);
    }

    /// The bank shown: nothing opens and the slot playing stays (a slot opens when it is chosen).
    fn show_bank(&mut self, page: usize) {
        if page != self.page && self.setlist.as_ref().is_some_and(|s| page < s.pages.len()) {
            self.page = page;
            self.slots = Cursor::new(0, 1);
        }
    }

    /// DEC (-) and INC (+): the bank `by` before or after, as the bar's Previous and Next; stops at the ends, no wrap.
    pub fn turn_bank(&mut self, by: i32) {
        let last = self.setlist.as_ref().map_or(0, |s| s.pages.len() - 1) as i64;
        self.show_bank((self.page as i64 + i64::from(by)).clamp(0, last) as usize);
        self.show();
    }

    /// A press on a stop of the setlist bar: a dropdown's list is made for the shell to open, Previous and Next
    /// show the bank before or after. A stop that is off does nothing.
    fn press(&mut self, stop: usize) {
        let pages = self.setlist.as_ref().map_or(0, |s| s.pages.len());
        match (stop, self.setlist.as_ref()) {
            (0, open) if !self.setlists.is_empty() => {
                let name = |n: &str| if n.is_empty() { "Untitled".to_string() } else { n.to_string() };
                self.banks_menu = false;
                let at = self.setlists.iter().position(|s| open.is_some_and(|o| o.id == s.0)).unwrap_or(0);
                self.menu = Some(("Setlist".into(), self.setlists.iter().enumerate().map(|(i, s)| Item::ticked(name(&s.1), i, i == at && open.is_some())).collect(), at));
            }
            (1, Some(s)) if pages > 1 => {
                self.banks_menu = true;
                self.menu = Some(("Bank".into(), (0..pages).map(|i| Item::ticked(bank_name(s, i), i, i == self.page)).collect(), self.page));
            }
            (2, _) if self.page > 0 => self.show_bank(self.page - 1),
            (3, _) => self.show_bank(self.page + 1),
            _ => {}
        }
    }

    /// A dropdown's list to open over the screen (taken: the shell opens it as the menu).
    pub fn take_menu(&mut self) -> Option<(String, Vec<Item>, usize)> {
        self.menu.take()
    }

    /// What was pressed in the dropdown: that setlist is read, or that bank is shown.
    pub fn chosen(&mut self, id: usize) {
        if self.banks_menu {
            self.show_bank(id);
        } else {
            self.open_setlist(id);
        }
        self.show();
    }

    /// Whether a bank lies before the one shown, and one after: the DEC and INC lamps.
    pub fn banks_around(&self) -> (bool, bool) {
        let pages = self.setlist.as_ref().map_or(0, |s| s.pages.len());
        (self.page > 0, self.page + 1 < pages)
    }

    /// The cursor is on the setlist bar's first stop: the dial moving up leaves for the top bar.
    pub fn at_top(&self) -> bool {
        self.bar.at == Some(0)
    }

    /// A navigation action; Back for Cancel with no cursor.
    pub fn act(&mut self, action: Action) -> Outcome {
        let pane = |p: Option<u8>, touched: u8| p.map_or(touched, |p| p.min(1));
        match action {
            Action::Move { pane: p, by } => match pane(p, self.touched) {
                0 => self.move_bar(by as i64, p.is_none()),
                _ => self.move_slots(by as i64, p.is_none()),
            },
            // As the JUCE Setlist: up and down move the cursor (along the setlist bar's stops, then the slots); left,
            // right and the page keys are the previous and next song (page turners and foot switches send them).
            Action::Arrow { dy, dx: 0 } if self.bar.at.is_some() => self.move_bar(dy as i64, true),
            Action::Arrow { dy, dx: 0 } => {
                let across = if self.setlist.as_ref().is_some_and(|s| s.grid) { self.setlist.as_ref().map_or(1, |s| s.columns as i64) } else { 1 };
                self.move_slots(dy as i64 * across, true);
            }
            Action::Arrow { dx, .. } | Action::Page(dx) => self.step(dx as i64),
            Action::End(by) => self.move_slots(by as i64 * 10_000, false),
            Action::Confirm(p) => match pane(p, self.touched) {
                0 => {
                    if let Some(at) = self.bar.at {
                        self.press(at);
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
                if self.bar.at.is_none() && self.slots.at.is_none() {
                    return Outcome::Back;
                }
                self.bar = self.bar.clear();
                self.slots = self.slots.clear();
            }
        }
        self.show();
        Outcome::Stay
    }

    /// A tap on a slot of the view, or on a stop of the setlist bar: what Confirm does, the cursor gone.
    pub fn tap_slot(&mut self, row: usize) {
        self.bar = self.bar.clear();
        self.slots = self.slots.clear();
        let Some(s) = self.setlist.as_ref() else { return };
        let index = s.first(self.page) + self.slots.first + row;
        self.go(index);
    }

    pub fn tap_stop(&mut self, stop: usize) {
        self.bar = self.bar.clear();
        self.slots = self.slots.clear();
        self.press(stop);
        self.show();
    }

    /// The previous (-1) or next (1) song of the setlist: Play's Previous and Next and its page keys.
    pub fn step_by(&mut self, by: i64) {
        self.step(by);
    }

    /// The screen's own action the top bar carries: Play, which opens Play.
    pub fn bar_tools(&self) -> Vec<crate::topbar::Tool> {
        vec![crate::topbar::Tool::new("Play")]
    }

    /// The Performance's library ID in the slot playing, as the JUCE Setlist sends it to Play.
    pub fn playing(&self) -> Option<&str> {
        self.setlist.as_ref()?.slot(self.current?).map(|s| s.id.as_str()).filter(|id| !id.is_empty())
    }

    /// The cursors drop when the screen goes, as on focus loss in the JUCE lists.
    pub fn hidden(&mut self) {
        self.bar = self.bar.clear();
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
