// SPDX-License-Identifier: AGPL-3.0-or-later
//! A plugin's parameter pages, host-drawn on the grid (grid.rs, slint/grid.slint; the JUCE
//! ParameterGrid, RFC 0019's generated editor): this is how most plugins are edited. The parameters
//! (`params.list`, with the plugin's `oscairn.ui` layout data) without the hidden ones become pages,
//! one per top-level CLAP module path ("Main" for parameters with none; numbered pages of one kind
//! together); a plugin of more than 24 parameters without module paths gets modules from its names'
//! leading words; a page of more than 24 has a tab per second-level module. A page's tiles are laid
//! out by grid.rs: views first, linked runs kept together, by priority, each tile the control the
//! plugin asks for (or its values suggest) and its value as the plugin writes it (`params.text`).
//!
//! Without touch: the cursor moves over the tiles in reading order (the dial), by columns (pane 0,
//! the left knob) and rows (pane 1, the right knob), or by the arrows; past the last tile it turns to
//! the next page, before the first to the one before; Page turns pages. A press selects the tile: a
//! knob or slider then takes the dial (a gesture begins, each step is sent, a press or Cancel ends
//! it), a range takes its low end then on a second press its high end, split buttons step their
//! stops, a dropdown opens its list (dial, press chooses, Cancel closes), a switch flips. Cancel backs
//! out one layer, then goes back. Touch: drag knobs and sliders, tap switches, stops and dropdowns,
//! the pages and tabs; a drag on the background scrolls. The shown tiles are published by priority
//! (`controllers.view` `screen`), which the device's screen knobs play (engine side, RFC 0011).
//! Values follow the engine (`params.changed`) except the one being changed.

use crate::grid::{self, by_priority, Control, Laid, Param, FLAG_HIDDEN};
use crate::nav::Action;
use crate::{GridCard, GridHeading, GridPanel, GridPoint, GridState, GridTile, Shell};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::HashMap;
use std::rc::Rc;

/// A page with more parameters than this has tabs.
const TABBED_ABOVE: usize = 24;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    List,
    Values,
    /// A change; its refusal is said.
    Set,
    Gesture,
    /// params.text for this parameter and value.
    Text(u32, u64),
    /// controllers.view: the page shown, for the screen knobs.
    View,
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Back,
}

/// What the dial and a press do now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Edit {
    None,
    /// A tile takes the dial; for a range, its end (0 low, 1 high).
    Value(usize, usize),
    /// A tile's dropdown is open.
    List(usize),
}

/// The path below a module path's first n levels.
fn after(module: &str, n: usize) -> String {
    module.split('/').skip(n).collect::<Vec<_>>().join("/")
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    node: u64,
    name: String,
    params: Vec<Param>,
    pages: Vec<(String, Vec<usize>)>,
    /// What the pages step through: a page, or a tab of one.
    entries: Vec<(usize, Option<String>)>,
    entry: usize,
    values: HashMap<u32, f64>,
    /// The plugin's text for a parameter's value, by value's bits.
    texts: HashMap<(u32, u64), String>,
    laid: Laid,
    width: f32,
    cursor: Option<usize>,
    edit: Edit,
    scroll: f32,
    list_cursor: usize,
    list_first: usize,
    /// The parameters whose gestures are open.
    gestures: Vec<u32>,
    message: String,
    /// The page last published to the screen knobs.
    published: Value,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen { ui: ui.as_weak(), ..Screen::new_detached() }
    }

    pub fn open(&mut self, node: u64, name: &str) {
        let ui = self.ui.clone();
        *self = Screen { ui, node, name: name.into(), ..Screen::new_detached() };
        self.out.push((Request::List, "params.list", json!({"node": node})));
        self.show();
    }

    fn new_detached() -> Screen {
        Screen {
            ui: slint::Weak::default(),
            node: 0,
            name: String::new(),
            params: vec![],
            pages: vec![],
            entries: vec![],
            entry: 0,
            values: HashMap::new(),
            texts: HashMap::new(),
            laid: Laid::default(),
            width: 0.,
            cursor: None,
            edit: Edit::None,
            scroll: 0.,
            list_cursor: 0,
            list_first: 0,
            gestures: vec![],
            message: String::new(),
            published: Value::Null,
            out: vec![],
        }
    }

    /// The headings and parameters of the page (or tab) shown.
    fn groups(&self) -> Vec<(String, Vec<usize>)> {
        let Some((page, tab)) = self.entries.get(self.entry) else { return vec![] };
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for &i in &self.pages[*page].1 {
            let m = &self.params[i].module;
            let heading = match tab {
                None => after(m, 1),
                Some(t) if level(m, 1) == t => after(m, 2),
                Some(_) => continue,
            };
            match groups.iter_mut().find(|g| g.0 == heading) {
                Some(g) => g.1.push(i),
                None => groups.push((heading, vec![i])),
            }
        }
        groups
    }

    /// The value texts of a stepped parameter's values: the plugin's where known, else the numbers;
    /// and whether all are the plugin's.
    fn labels(&self, p: &Param) -> (Vec<String>, bool) {
        let mut all = true;
        let labels = (0..p.values().min(64))
            .map(|k| {
                let v = p.min + k as f64;
                self.texts.get(&(p.id, v.to_bits())).cloned().unwrap_or_else(|| {
                    all = false;
                    tidy(v)
                })
            })
            .collect();
        (labels, all)
    }

    fn control(&self, i: usize) -> Control {
        let p = &self.params[i];
        let (labels, known) = self.labels(p);
        grid::control(p, if known { &labels } else { &[] }, &self.params)
    }

    fn relayout(&mut self) {
        let groups = self.groups();
        // Sections join in some themes: no gap between tiles then.
        let gap = self.ui.upgrade().map_or(grid::SECTION_GAP, |ui| crate::theme::section_gap(&ui, grid::SECTION_GAP));
        self.laid = grid::layout(&self.params, &groups, self.width, gap, &|i| self.control(i));
        if self.cursor.is_some_and(|c| c >= self.laid.tiles.len()) {
            self.cursor = None;
        }
        self.publish();
    }

    /// The tiles shown, by priority, for the device's screen knobs (as ParameterGrid::publishScreen).
    fn publish(&mut self) {
        if self.params.is_empty() {
            return;
        }
        let mut shown: Vec<usize> = self.laid.tiles.iter().map(|t| t.param).collect();
        by_priority(&self.params, &mut shown);
        let name = self.entries.get(self.entry).map(|(p, _)| self.pages[*p].0.clone()).unwrap_or_default();
        let params: Vec<Value> = shown.iter().take(64).map(|&i| json!({"kind": "parameter", "node": self.node, "param": self.params[i].id})).collect();
        let screen = json!({"name": name, "params": params});
        if screen != self.published {
            self.published = screen.clone();
            self.out.push((Request::View, "controllers.view", json!({"screen": screen})));
        }
    }

    /// The parameters of the page shown and a range's high ends: their values asked for, and the
    /// texts of every value of a stepped one of up to 64.
    fn ask_values(&mut self) {
        let mut ids: Vec<u32> = Vec::new();
        for (_, members) in self.groups() {
            for i in members {
                let p = self.params[i].clone();
                ids.push(p.id);
                if p.stepped && p.values() <= 64 && !p.readonly() {
                    for k in 0..p.values() {
                        self.ask_text(p.id, p.min + k as f64);
                    }
                }
            }
        }
        if !ids.is_empty() {
            let values: Vec<Value> = ids.iter().map(|id| json!({"node": self.node, "param": id})).collect();
            self.out.push((Request::Values, "params.get", json!({"values": values})));
        }
    }

    /// params.list's answer: the pages, and the values of the first.
    fn listed(&mut self, list: &Value) {
        self.params = editor_parameters(grid::parse(list));
        self.pages = editor_pages(&self.params);
        self.entries = self
            .pages
            .iter()
            .enumerate()
            .flat_map(|(i, (_, members))| {
                let tabs = tabs_for(&self.params, members);
                if tabs.is_empty() {
                    vec![(i, None)]
                } else {
                    tabs.into_iter().map(|t| (i, Some(t))).collect()
                }
            })
            .collect();
        self.entry = 0;
        self.relayout();
        self.ask_values();
    }

    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::List, Ok(r)) => self.listed(&r),
            (Request::Values, Ok(r)) => {
                for v in r["values"].as_array().into_iter().flatten() {
                    if let (Some(p), Some(x)) = (v["param"].as_u64(), v["value"].as_f64()) {
                        self.value_arrived(p as u32, x);
                    }
                }
            }
            (Request::Text(p, bits), Ok(r)) => {
                if let Some(t) = r["text"].as_str() {
                    self.texts.insert((p, bits), t.to_string());
                    // Off and On told apart from two buttons only once the texts are in.
                    if self.params.iter().any(|q| q.id == p && q.values() == 2) {
                        self.relayout();
                    }
                }
            }
            (Request::Set | Request::List, Err(e)) if e.code != 0 => {
                self.message = format!("The engine refused: {}", e.message);
                self.show();
                return true;
            }
            _ => {}
        }
        self.show();
        false
    }

    /// A value read or pushed: kept, its text asked for when not known, but not over one being changed.
    fn value_arrived(&mut self, param: u32, value: f64) {
        if self.gestures.contains(&param) {
            return;
        }
        self.values.insert(param, value);
        self.ask_text(param, value);
    }

    fn ask_text(&mut self, param: u32, value: f64) {
        let key = (param, value.to_bits());
        if !self.texts.contains_key(&key) && !self.out.iter().any(|o| o.0 == Request::Text(param, value.to_bits())) {
            self.out.push((Request::Text(param, value.to_bits()), "params.text", json!({"node": self.node, "param": param, "value": value})));
        }
    }

    /// params.changed: the values that moved, for this plugin.
    pub fn changed(&mut self, params: &Value) {
        for c in params["changes"].as_array().into_iter().flatten() {
            if c["node"].as_u64() == Some(self.node) && c["kind"] == "value" {
                if let (Some(p), Some(v)) = (c["param"].as_u64(), c["value"].as_f64()) {
                    self.value_arrived(p as u32, v);
                }
            }
        }
        self.show();
    }

    fn value(&self, p: &Param) -> f64 {
        self.values.get(&p.id).copied().unwrap_or(p.default.clamp(p.min.min(p.max), p.max.max(p.min)))
    }

    fn value_text(&self, p: &Param) -> String {
        match self.values.get(&p.id) {
            Some(v) => self.texts.get(&(p.id, v.to_bits())).cloned().unwrap_or_else(|| tidy(*v)),
            None => String::new(),
        }
    }

    fn high(&self, p: &Param) -> Option<&Param> {
        p.range_high.and_then(|h| self.params.iter().find(|q| q.id == h))
    }

    /// The parameter a tile's part sets: a range's high end for part 1.
    fn param_of(&self, tile: usize, part: usize) -> Option<Param> {
        let t = self.laid.tiles.get(tile)?;
        let p = &self.params[t.param];
        Some(if part == 1 && t.control == Control::Range { self.high(p)?.clone() } else { p.clone() })
    }

    fn tile_data(&self, k: usize) -> GridTile {
        let t = &self.laid.tiles[k];
        let p = &self.params[t.param];
        let v = self.value(p);
        let norm = p.norm(v);
        let mut value = self.value_text(p);
        let mut high = 0.;
        if t.control == Control::Range {
            if let Some(h) = self.high(p) {
                high = h.norm(self.value(h)) as f32;
                value = format!("{} – {}", value, self.value_text(h));
            }
        }
        if t.control == Control::Switch {
            value = match value.as_str() {
                "1" => "On".into(),
                "0" | "" => "Off".into(),
                _ => value,
            };
        }
        let choices: Vec<slint::SharedString> = if t.control == Control::Buttons { self.labels(p).0.into_iter().map(Into::into).collect() } else { vec![] };
        let (editing, part) = match self.edit {
            Edit::Value(e, part) if e == k => (true, part as i32),
            Edit::List(e) if e == k => (true, 0),
            _ => (false, 0),
        };
        let from = if p.bipolar() { 0.5 } else { 0. };
        GridTile {
            x: t.rect.x,
            y: t.rect.y,
            w: t.rect.w,
            h: t.rect.h,
            kind: t.control as i32,
            name: p.name.as_str().into(),
            value: value.into(),
            norm: norm as f32,
            high,
            bipolar: p.bipolar(),
            arc: grid::arc(from, norm).into(),
            choices: ModelRc::new(VecModel::from(choices)),
            chosen: (v - p.min).round() as i32,
            on: v > p.min,
            cursor: self.cursor == Some(k),
            editing,
            part,
            linked: t.linked,
        }
    }

    fn panel_data(&self, panel: &grid::Panel) -> GridPanel {
        let refs: Vec<&Param> = panel.params.iter().map(|&i| &self.params[i]).collect();
        let norm = |k: usize| refs[k].norm(self.value(refs[k]));
        let (line, fill, points) = grid::envelope_curve(&panel.envelope, &refs, &norm, panel.rect.w, panel.rect.h);
        let points: Vec<GridPoint> = points.into_iter().map(|(x, y)| GridPoint { x, y }).collect();
        GridPanel { x: panel.rect.x, y: panel.rect.y, w: panel.rect.w, h: panel.rect.h, title: panel.title.as_str().into(), line: line.into(), fill: fill.into(), points: ModelRc::new(VecModel::from(points)) }
    }

    /// Keeps the cursor's tile in view.
    fn follow(&mut self, view: f32) {
        if let Some(t) = self.cursor.and_then(|c| self.laid.tiles.get(c)) {
            if t.rect.y < self.scroll {
                self.scroll = t.rect.y;
            } else if t.rect.y + t.rect.h > self.scroll + view {
                self.scroll = t.rect.y + t.rect.h - view;
            }
        }
        self.scroll = self.scroll.clamp(0., (self.laid.height - view).max(0.));
    }

    fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let g = ui.global::<GridState>();
        let width = g.get_view_width();
        if (width - self.width).abs() > 0.5 {
            self.width = width;
            self.relayout();
        }
        self.follow(g.get_view_height());
        g.set_name(self.name.as_str().into());
        let page = self.entries.get(self.entry).map_or(0, |e| e.0);
        let pages: Vec<slint::SharedString> = self.pages.iter().map(|p| p.0.as_str().into()).collect();
        let tabs: Vec<slint::SharedString> = self.entries.iter().filter(|e| e.0 == page).filter_map(|e| e.1.as_deref().map(Into::into)).collect();
        let tab = self.entries.iter().filter(|e| e.0 == page).position(|e| *e == self.entries[self.entry]).unwrap_or(0);
        g.set_pages(ModelRc::new(VecModel::from(pages)));
        g.set_page(page as i32);
        g.set_tabs(ModelRc::new(VecModel::from(tabs)));
        g.set_tab(tab as i32);
        let tiles: Vec<GridTile> = (0..self.laid.tiles.len()).map(|k| self.tile_data(k)).collect();
        g.set_tiles(Rc::new(VecModel::from(tiles)).into());
        let cards: Vec<GridCard> = self.laid.cards.iter().map(|c| GridCard { x: c.x, y: c.y, w: c.w, h: c.h }).collect();
        g.set_cards(Rc::new(VecModel::from(cards)).into());
        let headings: Vec<GridHeading> = self.laid.headings.iter().map(|(y, t)| GridHeading { y: *y, text: t.as_str().into() }).collect();
        g.set_headings(Rc::new(VecModel::from(headings)).into());
        let panels: Vec<GridPanel> = self.laid.panels.iter().map(|p| self.panel_data(p)).collect();
        g.set_panels(Rc::new(VecModel::from(panels)).into());
        g.set_scroll(self.scroll);
        let message = if self.params.is_empty() && self.entries.is_empty() && self.message.is_empty() { "Reading the parameters...".to_string() } else { self.message.clone() };
        g.set_message(message.into());
        match self.edit {
            Edit::List(k) => {
                let p = &self.params[self.laid.tiles[k].param];
                let per = (g.get_list_per_view().max(1)) as usize;
                if self.list_cursor < self.list_first {
                    self.list_first = self.list_cursor;
                } else if self.list_cursor >= self.list_first + per {
                    self.list_first = self.list_cursor + 1 - per;
                }
                let list: Vec<slint::SharedString> = self.labels(p).0.into_iter().map(Into::into).collect();
                g.set_list(ModelRc::new(VecModel::from(list)));
                g.set_list_title(p.name.as_str().into());
                g.set_list_cursor(self.list_cursor as i32);
                g.set_list_first(self.list_first as i32);
                g.set_list_open(true);
            }
            _ => g.set_list_open(false),
        }
    }

    fn begin(&mut self, id: u32) {
        if !self.gestures.contains(&id) {
            self.gestures.push(id);
            self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": id, "begin": true})));
        }
    }

    fn end_gestures(&mut self) {
        for id in std::mem::take(&mut self.gestures) {
            self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": id, "begin": false})));
        }
    }

    fn set(&mut self, p: &Param, to: f64) {
        self.values.insert(p.id, to);
        self.message.clear();
        self.out.push((Request::Set, "params.set", json!({"values": [{"node": self.node, "param": p.id, "value": to}]})));
        self.ask_text(p.id, to);
    }

    /// A change in a gesture of its own (a tap, a flip, a choice).
    fn set_once(&mut self, p: &Param, to: f64) {
        self.begin(p.id);
        self.set(p, to);
        self.end_gestures();
    }

    /// The value of a tile's part stepped `by`: a stepped value's step, else a hundredth of the range.
    fn dial(&mut self, tile: usize, part: usize, by: i32) {
        let Some(p) = self.param_of(tile, part) else { return };
        let now = self.value(&p);
        let step = if p.stepped { 1. } else { (p.max - p.min) / 100. };
        let mut to = (now + step * by as f64).clamp(p.min.min(p.max), p.max.max(p.min));
        if p.stepped {
            to = to.round();
        }
        // A range's ends do not cross.
        if self.laid.tiles[tile].control == Control::Range {
            let low = self.params[self.laid.tiles[tile].param].clone();
            if let Some(h) = self.high(&low).cloned() {
                to = if part == 0 { to.min(self.value(&h)) } else { to.max(self.value(&low)) };
            }
        }
        if to != now {
            self.set(&p, to);
        }
    }

    /// A press on the cursor's tile.
    fn press(&mut self, k: usize) {
        let Some(t) = self.laid.tiles.get(k).cloned() else { return };
        let p = self.params[t.param].clone();
        match t.control {
            Control::Value => {}
            Control::Switch => {
                let to = if self.value(&p) > p.min { p.min } else { p.max };
                self.set_once(&p, to);
            }
            Control::List => {
                self.edit = Edit::List(k);
                self.list_cursor = (self.value(&p) - p.min).round().max(0.) as usize;
                self.list_first = self.list_cursor;
            }
            _ => {
                self.edit = Edit::Value(k, 0);
                self.begin(p.id);
            }
        }
    }

    fn turn_page(&mut self, by: i32) -> bool {
        let to = (self.entry as i32 + by).clamp(0, self.entries.len() as i32 - 1) as usize;
        if self.entries.is_empty() || to == self.entry {
            return false;
        }
        self.end_gestures();
        self.edit = Edit::None;
        self.entry = to;
        self.scroll = 0.;
        self.cursor = None;
        self.relayout();
        self.ask_values();
        true
    }

    /// The cursor moved by `by` in reading order; past either end the page turns.
    fn step(&mut self, by: i32) {
        let n = self.laid.tiles.len() as i32;
        let Some(c) = self.cursor else {
            if n > 0 {
                self.cursor = Some(if by > 0 { 0 } else { n as usize - 1 });
            }
            return;
        };
        let to = c as i32 + by;
        if to >= n {
            if self.turn_page(1) {
                self.cursor = (!self.laid.tiles.is_empty()).then_some(0);
            }
        } else if to < 0 {
            if self.turn_page(-1) {
                self.cursor = self.laid.tiles.len().checked_sub(1);
            }
        } else {
            self.cursor = Some(to as usize);
        }
    }

    /// The cursor across (dx) or up and down (dy); past the last row down the page turns, past the first up.
    fn arrow(&mut self, dx: i32, dy: i32) {
        let Some(c) = self.cursor else {
            self.cursor = (!self.laid.tiles.is_empty()).then_some(0);
            return;
        };
        let mut c = c;
        for _ in 0..(dx.abs() + dy.abs()) {
            match grid::arrow(&self.laid.tiles, c, dx.signum(), dy.signum()) {
                Some(to) => c = to,
                None if dy != 0 => {
                    if self.turn_page(dy.signum()) {
                        self.cursor = (!self.laid.tiles.is_empty()).then_some(if dy > 0 { 0 } else { self.laid.tiles.len() - 1 });
                    }
                    return;
                }
                None => break,
            }
        }
        self.cursor = Some(c);
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let outcome = self.handle(action);
        self.show();
        outcome
    }

    fn handle(&mut self, action: Action) -> Outcome {
        match (self.edit, action) {
            (Edit::List(k), Action::Move { by, .. } | Action::Arrow { dy: by, dx: 0 }) => {
                let n = self.params[self.laid.tiles[k].param].values().min(64) as i32;
                self.list_cursor = (self.list_cursor as i32 + by).clamp(0, (n - 1).max(0)) as usize;
            }
            (Edit::List(k), Action::Confirm(_)) => {
                let p = self.params[self.laid.tiles[k].param].clone();
                self.set_once(&p, p.min + self.list_cursor as f64);
                self.edit = Edit::None;
            }
            (Edit::List(_), Action::Cancel(_)) => self.edit = Edit::None,
            (Edit::Value(k, part), Action::Move { by, .. }) => self.dial(k, part, by),
            (Edit::Value(k, part), Action::Arrow { dx, dy }) => self.dial(k, part, dx - dy),
            (Edit::Value(k, 0), Action::Confirm(_)) if self.laid.tiles[k].control == Control::Range => {
                if let Some(h) = self.param_of(k, 1) {
                    self.begin(h.id);
                }
                self.edit = Edit::Value(k, 1);
            }
            (Edit::Value(..), Action::Confirm(_) | Action::Cancel(_)) => {
                self.end_gestures();
                self.edit = Edit::None;
            }
            (Edit::None, Action::Move { pane: Some(0), by }) => self.arrow(by, 0),
            (Edit::None, Action::Move { pane: Some(1), by }) => self.arrow(0, by),
            (Edit::None, Action::Move { by, .. }) => self.step(by),
            (Edit::None, Action::Arrow { dx, dy }) => self.arrow(dx, dy),
            (_, Action::Page(by)) => {
                self.turn_page(by);
            }
            (Edit::None, Action::End(by)) => self.cursor = self.laid.tiles.len().checked_sub(1).map(|last| if by < 0 { 0 } else { last }),
            (Edit::None, Action::Confirm(_)) => match self.cursor {
                Some(c) => self.press(c),
                None => self.cursor = (!self.laid.tiles.is_empty()).then_some(0),
            },
            (Edit::None, Action::Cancel(_)) if self.cursor.take().is_none() => return Outcome::Back,
            _ => {}
        }
        Outcome::Stay
    }

    // Touch.

    /// A drag begins on a tile's part.
    pub fn grab(&mut self, tile: i32, part: i32) {
        let Some(p) = self.param_of(tile as usize, part as usize) else { return };
        self.end_gestures();
        self.edit = Edit::None;
        self.cursor = Some(tile as usize);
        self.begin(p.id);
        self.show();
    }

    pub fn drag(&mut self, tile: i32, part: i32, norm: f32) {
        let Some(p) = self.param_of(tile as usize, part as usize) else { return };
        let to = p.denorm(norm as f64);
        if to != self.value(&p) {
            self.set(&p, to);
            self.show();
        }
    }

    pub fn release(&mut self, _tile: i32) {
        self.end_gestures();
        self.show();
    }

    /// A tap on a switch or a dropdown (`stop` -1), or on split buttons' stop.
    pub fn tap(&mut self, tile: i32, stop: i32) {
        let k = tile as usize;
        let Some(t) = self.laid.tiles.get(k).cloned() else { return };
        self.end_gestures();
        self.edit = Edit::None;
        self.cursor = Some(k);
        let p = self.params[t.param].clone();
        if t.control == Control::Buttons && stop >= 0 {
            self.set_once(&p, p.min + stop as f64);
        } else {
            self.press(k);
        }
        self.show();
    }

    /// A dropdown's value tapped, or -1: closed.
    pub fn choose(&mut self, k: i32) {
        if let (Edit::List(tile), true) = (self.edit, k >= 0) {
            let p = self.params[self.laid.tiles[tile].param].clone();
            self.set_once(&p, p.min + k as f64);
        }
        self.edit = Edit::None;
        self.show();
    }

    pub fn show_page(&mut self, page: i32) {
        if let Some(e) = self.entries.iter().position(|e| e.0 == page as usize) {
            self.turn_page(e as i32 - self.entry as i32);
        }
        self.show();
    }

    pub fn show_tab(&mut self, tab: i32) {
        let page = self.entries.get(self.entry).map_or(0, |e| e.0);
        if let Some(e) = self.entries.iter().enumerate().filter(|e| e.1 .0 == page).nth(tab as usize).map(|e| e.0) {
            self.turn_page(e as i32 - self.entry as i32);
        }
        self.show();
    }

    pub fn scrolled(&mut self, by: f32) {
        self.scroll += by;
        let view = self.ui.upgrade().map_or(400., |ui| ui.global::<GridState>().get_view_height());
        self.scroll = self.scroll.clamp(0., (self.laid.height - view).max(0.));
        self.show();
    }

    pub fn hidden(&mut self) {
        self.end_gestures();
        self.edit = Edit::None;
        // The screen knobs go back to what they play without a page.
        self.published = Value::Null;
        self.out.push((Request::View, "controllers.view", json!({"screen": {"name": "", "params": []}})));
    }

    pub fn shown(&mut self) {
        if !self.laid.tiles.is_empty() {
            self.publish();
        }
        self.show();
    }
}

/// The nth level of a module path, or "".
fn level(module: &str, n: usize) -> &str {
    module.split('/').nth(n).filter(|_| !module.is_empty()).unwrap_or("")
}

/// A name's leading word with the number after it when it has one ("Step 6"); empty when nothing would be left.
fn lead(name: &str) -> String {
    let Some(end) = name.find(' ') else { return String::new() };
    if let Some(next) = name[end + 1..].find(' ').map(|n| n + end + 1) {
        if name[end + 1..next].chars().all(|c| c.is_ascii_digit()) && next > end + 1 {
            return name[..next].to_string();
        }
    }
    name[..end].to_string()
}

/// The parameters an editor shows: hidden ones left out; and for a plugin of more than 24 that gives no
/// module paths, modules from its names' leading words, in module order, as `editorParameters`.
pub fn editor_parameters(mut params: Vec<Param>) -> Vec<Param> {
    params.retain(|p| p.flags & FLAG_HIDDEN == 0);
    if params.len() <= TABBED_ABOVE || params.iter().any(|p| !p.module.is_empty()) {
        return params;
    }
    for depth in 0..2 {
        let mut count: HashMap<String, usize> = HashMap::new();
        let mut page: HashMap<String, usize> = HashMap::new();
        for p in &params {
            *page.entry(level(&p.module, 0).to_string()).or_default() += 1;
            let l = lead(&p.name);
            if !l.is_empty() {
                *count.entry(format!("{}/{}", p.module, l)).or_default() += 1;
            }
        }
        for p in &mut params {
            let l = lead(&p.name);
            if l.is_empty() || count[&format!("{}/{}", p.module, l)] < 2 || (depth == 1 && (p.module.is_empty() || page[level(&p.module, 0)] <= TABBED_ABOVE)) {
                continue;
            }
            p.module = if p.module.is_empty() { l.clone() } else { format!("{}/{}", p.module, l) };
            p.name = p.name[l.len() + 1..].to_string();
        }
    }
    params.sort_by_key(|p| natural(&p.module));
    params
}

/// A natural order: runs of digits compare as numbers.
fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut digits = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push((std::mem::take(&mut word), digits.parse().unwrap_or(0)));
                digits.clear();
            }
            word.push(c);
        }
    }
    out.push((word, digits.parse().unwrap_or(0)));
    out
}

/// "Operator 12" is ("Operator", 12); a name without a trailing number has none.
fn numbered(name: &str) -> (String, i64) {
    let end = name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if end == name.len() || end == 0 || !name[..end].ends_with(' ') {
        return (name.to_string(), -1);
    }
    (name[..end - 1].to_string(), name[end..].parse().unwrap_or(-1))
}

/// A page per top-level module in the plugin's order, numbered pages of one kind together in number order.
pub fn editor_pages(params: &[Param]) -> Vec<(String, Vec<usize>)> {
    let mut pages: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, p) in params.iter().enumerate() {
        let top = if p.module.is_empty() { "Main".to_string() } else { level(&p.module, 0).to_string() };
        match pages.iter_mut().find(|(n, _)| *n == top) {
            Some(page) => page.1.push(i),
            None => pages.push((top, vec![i])),
        }
    }
    let mut family: HashMap<String, usize> = HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        family.entry(numbered(&p.0).0).or_insert(i);
    }
    pages.sort_by_key(|(n, _)| {
        let (kind, number) = numbered(n);
        (family[&kind], number)
    });
    pages
}

/// The tabs of a page of more than 24 parameters: its second-level modules, when there are two or more.
fn tabs_for(params: &[Param], page: &[usize]) -> Vec<String> {
    if page.len() <= TABBED_ABOVE {
        return vec![];
    }
    let mut names: Vec<String> = Vec::new();
    for &i in page {
        let n = level(&params[i].module, 1).to_string();
        if !names.contains(&n) {
            names.push(n);
        }
    }
    if names.len() < 2 {
        names.clear();
    }
    names
}

/// A value without trailing zeros ("0.0100000" is "0.01"), as the JUCE tidyNumber.
fn tidy(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: u32, name: &str, module: &str, priority: u64) -> Param {
        Param { id, name: name.into(), module: module.into(), max: 1., priority, ..Default::default() }
    }

    #[test]
    fn pages_follow_the_modules_and_numbered_ones_sit_together() {
        let params = vec![p(0, "Gain", "", 0), p(1, "Level", "Operator 2", 0), p(2, "Level", "Filter/Main", 0), p(3, "Level", "Operator 1", 0), p(4, "Mix", "Operator 1/Env", 0)];
        let pages: Vec<(String, Vec<usize>)> = editor_pages(&params);
        let expected: Vec<(String, Vec<usize>)> = vec![("Main".into(), vec![0]), ("Operator 1".into(), vec![3]), ("Operator 2".into(), vec![1]), ("Filter".into(), vec![2])];
        // "Operator 1/Env" is on the page of its top-level module, "Operator 1".
        assert_eq!(pages.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["Main", "Operator 1", "Operator 2", "Filter"]);
        assert_eq!(pages[1].1, vec![3, 4]);
        assert_eq!(pages[0], expected[0]);
    }

    #[test]
    fn priorities_first_then_the_plugins_order_and_hidden_ones_go() {
        let mut params = vec![p(0, "A", "", 0), p(1, "B", "", 2), p(2, "C", "", 1), p(3, "D", "", 0)];
        params[3].flags = FLAG_HIDDEN;
        let shown = editor_parameters(params);
        assert_eq!(shown.len(), 3);
        let mut order: Vec<usize> = vec![0, 1, 2];
        by_priority(&shown, &mut order);
        assert_eq!(order, [2, 1, 0]);
    }

    #[test]
    fn a_big_plugin_without_modules_is_grouped_by_the_leading_words_of_its_names() {
        let mut params: Vec<Param> = (0..13).map(|i| p(i, &format!("Osc{} Detune", i % 2), "", 0)).collect();
        params.extend((13..30).map(|i| p(i, &format!("Filter Cutoff {i}"), "", 0)));
        params.push(p(30, "Master", "", 0));
        let shown = editor_parameters(params);
        let modules: std::collections::BTreeSet<_> = shown.iter().map(|p| p.module.clone()).collect();
        assert!(modules.contains("Filter") && modules.contains("Osc0") && modules.contains("Osc1"), "{modules:?}");
        assert!(shown.iter().any(|p| p.module == "Osc0" && p.name == "Detune"), "the leading word moves into the module");
        assert!(shown.iter().any(|p| p.module.is_empty() && p.name == "Master"), "a word that leads one name only stays in it");
    }

    #[test]
    fn tabs_only_on_a_page_of_more_than_24() {
        let big: Vec<Param> = (0..30).map(|i| p(i, "P", if i < 15 { "Osc/A" } else { "Osc/B" }, 0)).collect();
        let page: Vec<usize> = (0..30).collect();
        assert_eq!(tabs_for(&big, &page), ["A", "B"]);
        assert!(tabs_for(&big, &page[..20]).is_empty());
        assert_eq!(numbered("Operator 12"), ("Operator".to_string(), 12));
        assert_eq!(numbered("Cutoff"), ("Cutoff".to_string(), -1));
        assert_eq!(tidy(0.0100000), "0.01");
    }
}
