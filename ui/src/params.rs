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

use crate::fm;
use crate::grid::{self, by_priority, Control, Laid, Param, FLAG_HIDDEN};
use crate::nav::Action;
use crate::tabs::{self, Entry, Model};
use crate::views;
use crate::{GridBox, GridCard, GridHeading, GridLabel, GridPanel, GridPoint, GridShape, GridState, GridTile, Shell};
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
    /// The on-screen keyboard for the search: its title and the text now.
    Prompt(String, String),
}

/// A row of tabs the cursor can be on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Strip {
    Pages,
    Tabs,
    /// The master's overview: the view's own parameters, then its operators, in the order they are drawn.
    Overview,
}

/// A stop of the cursor above the tiles: a row of tabs (the search is the top bar's).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    Row(Strip),
}

/// Where the cursor is: on a stop (and the entry in it), or on a tile.
#[derive(Clone, Copy, Debug, PartialEq)]
enum At {
    Bar(Stop, usize),
    Tile(usize),
}

/// What an entry of the overview's row is: one of the view's own parameters, or a cell of the operator table (the
/// operator 0 to 7, the column among the table's).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Over {
    Param(usize),
    Cell(usize, usize),
}

/// What the dial and a press do now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Edit {
    None,
    /// A tile takes the dial; for a range, its end (0 low, 1 high).
    Value(usize, usize),
    /// A tile's dropdown is open.
    List(usize),
    /// One of the overview's own parameters (the algorithm) takes the dial.
    Param(usize),
}

/// The labels of a row or rail of tabs: each name's short form on one and on two lines, and the heading the names share
/// (src/tabs.rs `abbreviate`); the names themselves stay what the path, the cursor and a screen reader use.
fn labels(names: &[String]) -> (Vec<crate::TabLabel>, String) {
    let tabs::Abbreviated { heading, shorts } = tabs::abbreviate(names);
    let labels = shorts
        .into_iter()
        .map(|short| {
            let (top, bottom) = tabs::two_lines(&short);
            crate::TabLabel { short: short.into(), top: top.into(), bottom: bottom.into() }
        })
        .collect();
    (labels, heading)
}

/// The path below a module path's first n levels.
pub fn after(module: &str, n: usize) -> String {
    module.split('/').skip(n).collect::<Vec<_>>().join("/")
}

pub struct Screen {
    shared: bool,
    ui: slint::Weak<Shell>,
    node: u64,
    name: String,
    params: Vec<Param>,
    /// The pages and their tabs, from the module paths or the plugin's declaration (src/tabs.rs).
    model: Model,
    /// What the page keys step through: a page, or a tab of one.
    entries: Vec<Entry>,
    entry: usize,
    /// The cursor on the pages' or the tabs' row (one cursor at a time with the tiles' and the search's).
    strip: Option<(Strip, usize)>,
    /// The pages and the tabs are rails at the left (the side placement), not strips over the tiles.
    side: bool,
    values: HashMap<u32, f64>,
    /// The plugin's text for a parameter's value, by value's bits.
    texts: HashMap<(u32, u64), String>,
    laid: Laid,
    width: f32,
    /// The gap between sections the layout was made with (the theme's: none where sections join).
    gap: f32,
    cursor: Option<usize>,
    edit: Edit,
    scroll: f32,
    list_cursor: usize,
    list_first: usize,
    /// The search text (a name or module on any page); its button is on the top bar.
    search: String,
    /// The parameters whose gestures are open.
    gestures: Vec<u32>,
    /// A view's handle held by touch: the panel and the handle.
    view_held: Option<(usize, views::Held)>,
    message: String,
    /// The page last published to the screen knobs.
    published: Value,
    /// The master's selected one (0 to 7): the detail shows it, the knobs play it, the overview marks it.
    operator: usize,
    /// The table's column the cursor was last in (kept from row to row, and when it comes back to the table).
    cell_col: usize,
    /// The operators the host has soloed, and the mutes they had before (the host's solo, where the plugin's mute is
    /// transient: `fm::SoloFrom::Others`). Nothing is saved: it is the page's, and goes with it.
    soloed: Vec<bool>,
    baseline: Vec<f64>,
    /// The first row of the detail's tiles in view, and the area's height it was laid out for.
    detail_row: usize,
    height: f32,
    /// What the master's indexes are called (the folded tabs' names).
    names: Vec<(u64, String)>,
    /// The detail tile the cursor left upward, which a step down from the overview comes back to.
    memory: Option<usize>,
    /// Where each FM operator's most recent voice is on its envelope (the dots), when the plugin says.
    positions: Vec<Option<views::Along>>,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen { ui: ui.as_weak(), ..Screen::new_detached() }
    }

    pub fn open(&mut self, node: u64, name: &str) {
        let ui = self.ui.clone();
        *self = Screen { ui, node, name: name.into(), shared: false, ..Screen::new_detached() };
        self.out.push((Request::List, "params.list", json!({"node": node})));
        self.show();
    }

    fn new_detached() -> Screen {
        Screen {
            ui: slint::Weak::default(),
            shared: false,
            node: 0,
            name: String::new(),
            params: vec![],
            model: Model::default(),
            entries: vec![],
            entry: 0,
            strip: None,
            side: false,
            values: HashMap::new(),
            texts: HashMap::new(),
            laid: Laid::default(),
            width: 0.,
            gap: grid::SECTION_GAP,
            cursor: None,
            edit: Edit::None,
            scroll: 0.,
            list_cursor: 0,
            list_first: 0,
            search: String::new(),
            gestures: vec![],
            view_held: None,
            message: String::new(),
            published: Value::Null,
            operator: 0,
            cell_col: 0,
            soloed: vec![],
            baseline: vec![],
            detail_row: 0,
            height: 0.,
            names: vec![],
            memory: None,
            positions: vec![],
            out: vec![],
        }
    }

    /// The headings and parameters of the page (or tab) shown; while searching, the parameters of every
    /// page whose name or module has the text, each module's under its whole path.
    fn groups(&self) -> Vec<(String, Vec<usize>)> {
        if !self.search.is_empty() {
            let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
            let query = self.search.to_lowercase();
            for (i, p) in self.params.iter().enumerate() {
                if p.name.to_lowercase().contains(&query) || p.module.to_lowercase().contains(&query) {
                    let heading = if p.module.is_empty() { "Main".to_string() } else { p.module.clone() };
                    match groups.iter_mut().find(|g| g.0 == heading) {
                        Some(g) => g.1.push(i),
                        None => groups.push((heading, vec![i])),
                    }
                }
            }
            return groups;
        }
        match self.entries.get(self.entry) {
            Some(e) => self.model.groups(e.page, e.tab),
            None => vec![],
        }
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
        let (scale, spacing) = self.ui.upgrade().map_or((1., 1.), |ui| {
            let theme = ui.global::<crate::Theme>();
            (theme.get_font_scale(), theme.get_spacing())
        });
        self.names = self.entries.get(self.entry).and_then(|e| self.model.pages[e.page].indexed.as_ref()).map(|i| i.names.clone()).unwrap_or_default();
        let master = grid::Master { height: if self.height > 0. { self.height } else { 405. }, selected: self.operator, row: self.detail_row, scale, spacing, names: &self.names };
        self.laid = grid::layout_with(&self.params, &groups, self.width, self.gap, &|i| self.control(i), &master);
        if let Some(d) = &self.laid.detail {
            self.detail_row = d.row;
        }
        if self.cursor.is_some_and(|c| c >= self.laid.tiles.len()) {
            self.cursor = None;
        }
        self.publish();
    }

    /// The FM operator view of the page shown, if it has one.
    fn fm(&self) -> Option<&views::FmShape> {
        self.laid.panels.iter().find_map(|p| match &p.view {
            views::View::Fm(f) => Some(f),
            _ => None,
        })
    }

    /// The master's selected operator (0 to 7).
    fn selected_operator(&self) -> usize {
        self.operator
    }

    /// Selects an operator: the detail shows its parameters (the row kept, as the sweep of one parameter across
    /// operators wants), the knobs play them. Selecting is not an edit: nothing is sent but the knobs' page.
    fn select(&mut self, n: usize) {
        let count = self.laid.detail.as_ref().map_or(0, |d| d.count);
        let n = n.min(count.saturating_sub(1));
        if n != self.operator {
            self.operator = n;
            self.relayout();
        }
    }

    /// Where each FM operator's most recent voice is on its envelope line, from the plugin (`None`: no dot for
    /// it). The only call that draws dots: nothing runs while it is not called, and a repaint costs one
    /// page's drawing, so a source that has nothing to say simply stops calling (after a last call with every
    /// operator `None`, which takes the dots away). Whether a page wants them: `wants_positions`.
    #[allow(dead_code)] // the engine's side is a proposal; the tests call it
    pub fn set_positions(&mut self, positions: Vec<Option<views::Along>>) {
        if positions == self.positions {
            return;
        }
        self.positions = positions;
        self.show();
    }

    /// Whether the page shown has an FM view, which draws dots when `set_positions` gives it positions.
    #[allow(dead_code)]
    pub fn wants_positions(&self) -> bool {
        self.fm().is_some()
    }

    /// The tiles shown, by priority, for the device's screen knobs (as ParameterGrid::publishScreen). On a page
    /// with an FM view the selected operator's parameters come first, so the knobs play the operator in view.
    fn publish(&mut self) {
        if self.params.is_empty() {
            return;
        }
        let detail = self.laid.detail.as_ref().map_or(0, |d| d.tiles);
        let mut shown: Vec<usize> = self.laid.tiles.iter().skip(detail).map(|t| t.param).collect();
        by_priority(&self.params, &mut shown);
        // The selected one's parameters, all of them in the plugin's priority order, then the view's own, then the rest.
        if let Some(d) = &self.laid.detail {
            // The table's row of the selected operator first, its columns left to right (level, coarse, fine, ratio,
            // detune: not the switched ones), so the knobs play what the row shows; then its other parameters.
            let row: Vec<usize> = self.fm().and_then(|f| f.operators.get(self.operator).map(|o| f.columns().into_iter().filter(|c| !c.toggles()).filter_map(|c| c.param(o)).collect())).unwrap_or_default();
            let mut first = row.clone();
            first.extend(d.params.iter().copied().filter(|i| !row.contains(i)));
            first.extend(d.own.iter().copied());
            first.extend(shown);
            let mut seen: Vec<usize> = Vec::new();
            first.retain(|i| !seen.contains(i) && {
                seen.push(*i);
                true
            });
            shown = first;
        }
        let name = self.entries.get(self.entry).map(|e| self.model.pages[e.page].name.clone()).unwrap_or_default();
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

    /// Every value read again: after an Undo or a Redo, which the engine does not always say with a `params.changed` (the undo of
    /// a request that set two values said nothing against the real engine).
    pub fn values_again(&mut self) {
        // Every parameter of the plugin, not only those with a tile: a view (an envelope, an operator table) holds the rest.
        // (The engine takes 128 values to a request: `limits.parameterChanges`.)
        let values: Vec<Value> = self.params.iter().map(|p| json!({"node": self.node, "param": p.id})).collect();
        for chunk in values.chunks(128) {
            self.out.push((Request::Values, "params.get", json!({"values": chunk})));
        }
    }

    /// The plugin plays one sound of an instance other nodes share (`graph.get`'s `shared`): the host sets its Output bus, and
    /// the engine puts back a value a client sets, so the parameter is shown and not offered.
    pub fn set_shared(&mut self, shared: bool) {
        self.shared = shared;
    }

    /// params.list's answer: the pages, and the values of the first.
    fn listed(&mut self, list: &Value) {
        self.params = editor_parameters(grid::parse(list));
        if self.shared {
            for p in self.params.iter_mut().filter(|p| p.role == "outputBus") {
                p.flags |= grid::FLAG_READONLY;
            }
        }
        self.model = Model::build(&self.params, tabs::declared_in(list).as_ref());
        self.entries = self.model.entries();
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
                self.message = crate::client::refusal(&e);
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
        let mut moved = false;
        for c in params["changes"].as_array().into_iter().flatten() {
            if c["node"].as_u64() == Some(self.node) && c["kind"] == "value" {
                if let (Some(p), Some(v)) = (c["param"].as_u64(), c["value"].as_f64()) {
                    // A value the page already shows (the plugin rewriting it, another client setting the same)
                    // costs nothing: no text asked again, no relayout, no frame.
                    if self.values.get(&(p as u32)) != Some(&v) {
                        self.value_arrived(p as u32, v);
                        moved = true;
                    }
                }
            }
        }
        if moved {
            self.show();
        }
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
        let (scale, spacing) = self.ui.upgrade().map_or((1., 1.), |ui| {
            let theme = ui.global::<crate::Theme>();
            (theme.get_font_scale(), theme.get_spacing())
        });
        let mut value = self.value_text(p);
        let mut name = p.name.clone();
        // Under the heading "Operator 5" a control's own name does not say the number again; the cursor's readout and the
        // knobs keep the full name.
        if let (true, Some(d)) = (t.compact, &self.laid.detail) {
            name = grid::without_index(&name, d.index);
        }
        if let Some(stage) = &t.stage {
            name = stage.name.clone();
            value = match stage.kind {
                grid::StageKind::Level => value,
                // A time from the plugin's seconds where it gives them, else its own text; a rate's seconds are what a stage
                // across the envelope's whole range takes, not the stage's own length, and are marked so; without them the
                // raw value says "rate" (the stage's length depends on the levels it runs between: only the plugin knows it).
                grid::StageKind::Time => grid::stage_seconds(p, norm).map_or(value, grid::seconds_text),
                grid::StageKind::Rate => grid::stage_seconds(p, norm).map_or_else(|| format!("{} rate", tidy(v)), |s| format!("{} /range", grid::seconds_text(s))),
            };
        }
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
        let labels: Vec<String> = if t.control == Control::Buttons { self.labels(p).0 } else { vec![] };
        let icons: Vec<i32> = grid::icons_for(p, &labels);
        let icon = if t.control == Control::List {
            let all = grid::icons_for(p, &self.labels(p).0);
            all.get((v - p.min).round().max(0.) as usize).copied().unwrap_or_else(|| grid::icon(None, &value))
        } else {
            0
        };
        let choices: Vec<slint::SharedString> = labels.into_iter().map(Into::into).collect();
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
            name: name.as_str().into(),
            value: value.into(),
            norm: norm as f32,
            high,
            bipolar: p.bipolar(),
            arc: grid::arc(from, norm).into(),
            choices: ModelRc::new(VecModel::from(choices)),
            icons: ModelRc::new(VecModel::from(icons)),
            icon,
            chosen: (v - p.min).round() as i32,
            on: v > p.min,
            cursor: self.cursor == Some(k),
            editing,
            part,
            linked: t.linked,
            cell: t.control == Control::Knob && t.compact && grid::knob_is_cell(t.rect.w, t.rect.h, scale, spacing),
            compact: t.compact,
        }
    }

    /// A panel's view drawn from the values now, and what `f` makes of it with what it reads.
    fn with_view<R>(&self, panel: &grid::Panel, f: impl FnOnce(&views::Ctx) -> R) -> R {
        let (scale, spacing) = self.ui.upgrade().map_or((1., 1.), |ui| {
            let theme = ui.global::<crate::Theme>();
            (theme.get_font_scale(), theme.get_spacing())
        });
        let norm = |i: usize| self.params[i].norm(self.value(&self.params[i]));
        let text = |i: usize| self.value_text(&self.params[i]);
        let ctx = views::Ctx {
            params: &self.params,
            members: &panel.params,
            norm: &norm,
            text: &text,
            focus: self.cursor.and_then(|c| self.laid.tiles.get(c)).map(|t| t.param).or_else(|| match self.over() {
                Some(Over::Param(p)) => Some(p),
                _ => None,
            }),
            w: panel.rect.w,
            h: panel.rect.h,
            top: grid::envelope_top(scale, spacing),
            scale,
            spacing,
            selected: self.selected_operator(),
            cell: match self.over() {
                Some(Over::Cell(n, k)) => Some((n, k)),
                _ => None,
            },
            editing: matches!(self.edit, Edit::Param(_)),
            soloed: &self.soloed,
            positions: &self.positions,
        };
        f(&ctx)
    }

    fn panel_data(&self, k: usize) -> GridPanel {
        let panel = &self.laid.panels[k];
        let held = self.view_held.as_ref().filter(|h| h.0 == k).map(|h| &h.1);
        let drawn = self.with_view(panel, |c| (views::draw(&panel.view, c), held.map(|h| views::touched(&panel.view, c, h)).unwrap_or_default(), c.focus));
        let (d, taken, focus) = drawn;
        let shapes: Vec<GridShape> = d.shapes.into_iter().map(|s| GridShape { ink: s.ink.0, fill: s.fill, width: s.width, commands: s.commands.into() }).collect();
        let points: Vec<GridPoint> = d.handles.iter().map(|h| GridPoint { x: h.at.0, y: h.at.1, lit: focus.is_some_and(|f| h.params.contains(&f)), held: !taken.is_empty() && h.params == taken }).collect();
        let labels: Vec<GridLabel> = d.labels.into_iter().map(|l| GridLabel { x: l.x, y: l.y, w: l.w, h: l.h, text: l.text.into(), ink: l.ink.0, bold: l.bold, align: l.align }).collect();
        let boxes: Vec<GridBox> = d.boxes.into_iter().map(|b| GridBox { x: b.x, y: b.y, w: b.w, h: b.h, fill: b.fill.0, edge: b.edge.map_or(-1, |e| e.0), text: b.text.into(), text_ink: b.text_ink.0, bold: b.bold, mark: b.mark }).collect();
        GridPanel {
            x: panel.rect.x,
            y: panel.rect.y,
            w: panel.rect.w,
            h: panel.rect.h,
            title: d.title.into(),
            shapes: ModelRc::new(VecModel::from(shapes)),
            points: ModelRc::new(VecModel::from(points)),
            labels: ModelRc::new(VecModel::from(labels)),
            boxes: ModelRc::new(VecModel::from(boxes)),
        }
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
        // Laid out again when the area's width changed, or the theme's gap between sections (a theme
        // whose sections join has none).
        let gap = crate::theme::section_gap(&ui, grid::SECTION_GAP);
        let height = g.get_view_height();
        if (width - self.width).abs() > 0.5 || (height - self.height).abs() > 0.5 || gap != self.gap {
            self.width = width;
            self.height = height;
            self.gap = gap;
            self.relayout();
        }
        self.follow(g.get_view_height());
        g.set_name(self.name.as_str().into());
        self.side = g.get_layout() == 1;
        let page = self.entries.get(self.entry).map_or(0, |e| e.page);
        let searching = !self.search.is_empty();
        let pages: Vec<slint::SharedString> = self.model.pages.iter().map(|p| p.name.as_str().into()).collect();
        // A search has no tabs, and no page is the one shown.
        let tabs: Vec<slint::SharedString> = if searching { vec![] } else { self.model.pages.get(page).map_or(vec![], |p| p.tabs.iter().map(|t| t.as_str().into()).collect()) };
        let tab = self.entries.get(self.entry).and_then(|e| e.tab).unwrap_or(0);
        g.set_page_cursor(self.strip.filter(|s| s.0 == Strip::Pages).map_or(-1, |s| s.1 as i32));
        g.set_tab_cursor(self.strip.filter(|s| s.0 == Strip::Tabs).map_or(-1, |s| s.1 as i32));
        let (page_names, tab_names): (Vec<String>, Vec<String>) = (self.model.pages.iter().map(|p| p.name.clone()).collect(), tabs.iter().map(|t| t.to_string()).collect());
        let (page_labels, page_heading) = labels(&page_names);
        let (tab_labels, tab_heading) = labels(&tab_names);
        g.set_page_labels(ModelRc::new(VecModel::from(page_labels)));
        g.set_page_heading(page_heading.into());
        g.set_tab_labels(ModelRc::new(VecModel::from(tab_labels)));
        g.set_tab_heading(tab_heading.into());
        g.set_pages(ModelRc::new(VecModel::from(pages)));
        g.set_page(if searching { -1 } else { page as i32 });
        g.set_search_shown(self.search_shown());
        g.set_search(self.search.as_str().into());
        g.set_tabs(ModelRc::new(VecModel::from(tabs)));
        g.set_tab(tab as i32);
        let tiles: Vec<GridTile> = (0..self.laid.tiles.len()).map(|k| self.tile_data(k)).collect();
        g.set_tiles(Rc::new(VecModel::from(tiles)).into());
        let cards: Vec<GridCard> = self.laid.cards.iter().map(|c| GridCard { x: c.x, y: c.y, w: c.w, h: c.h }).collect();
        g.set_cards(Rc::new(VecModel::from(cards)).into());
        let headings: Vec<GridHeading> = self.laid.headings.iter().map(|(y, t)| GridHeading { y: *y, text: t.as_str().into() }).collect();
        g.set_headings(Rc::new(VecModel::from(headings)).into());
        let panels: Vec<GridPanel> = (0..self.laid.panels.len()).map(|k| self.panel_data(k)).collect();
        g.set_panels(Rc::new(VecModel::from(panels)).into());
        g.set_scroll(self.scroll);
        g.set_content(self.laid.height);
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
                let labels = self.labels(p).0;
                g.set_list_icons(ModelRc::new(VecModel::from(grid::icons_for(p, &labels))));
                let list: Vec<slint::SharedString> = labels.into_iter().map(Into::into).collect();
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

    /// A parameter of the overview's stepped `by`, as a tile's value is.
    fn dial_param(&mut self, i: usize, by: i32) {
        let p = self.params[i].clone();
        let now = self.value(&p);
        let step = if p.stepped { 1. } else { (p.max - p.min) / 100. };
        let mut to = (now + step * by as f64).clamp(p.min.min(p.max), p.max.max(p.min));
        if p.stepped {
            to = to.round();
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

    /// The page keys' step: to the entry `by` away, a page or a tab, at once (a key pressed for it, not a
    /// cursor moved). A page chosen while searching ends the search, as the reference's showPage does.
    fn turn_page(&mut self, by: i32) -> bool {
        let to = (self.entry as i32 + by).clamp(0, self.entries.len() as i32 - 1) as usize;
        let searching = !std::mem::take(&mut self.search).is_empty();
        if self.entries.is_empty() || (to == self.entry && !searching) {
            return false;
        }
        self.end_gestures();
        self.edit = Edit::None;
        self.entry = to;
        self.scroll = 0.;
        self.cursor = None;
        self.strip = None;
        self.relayout();
        self.ask_values();
        true
    }

    /// The rows above the tiles, top to bottom, as they are drawn: the search, the pages, the tabs.
    fn bars(&self) -> Vec<Stop> {
        let mut out = Vec::new();
        if self.model.pages.len() > 1 {
            out.push(Stop::Row(Strip::Pages));
        }
        if self.tab_names().len() > 1 {
            out.push(Stop::Row(Strip::Tabs));
        }
        if self.laid.detail.is_some() {
            out.push(Stop::Row(Strip::Overview));
        }
        out
    }

    fn tab_names(&self) -> &[String] {
        match (self.entries.get(self.entry), self.search.is_empty()) {
            (Some(e), true) => &self.model.pages[e.page].tabs,
            _ => &[],
        }
    }

    /// The page shown (None while searching: no page is the one) and the tab of it.
    fn shown_page(&self) -> Option<usize> {
        self.entries.get(self.entry).map(|e| e.page).filter(|_| self.search.is_empty())
    }

    fn shown_tab(&self) -> usize {
        self.entries.get(self.entry).and_then(|e| e.tab).unwrap_or(0)
    }

    /// Where the cursor is, among the stops above the tiles and the tiles.
    fn at(&self) -> Option<At> {
        if let Some((row, i)) = self.strip {
            Some(At::Bar(Stop::Row(row), i))
        } else {
            self.cursor.map(At::Tile)
        }
    }

    /// Puts the cursor on a stop (one cursor at a time).
    fn go(&mut self, to: At) {
        let left = self.cursor.filter(|&c| self.laid.detail.as_ref().is_some_and(|d| c < d.tiles));
        self.strip = None;
        self.cursor = None;
        match to {
            At::Bar(Stop::Row(row), i) => {
                self.strip = Some((row, i));
                if left.is_some() {
                    self.memory = left;
                }
                // Looking at an operator is not an edit: the detail follows the cursor along the overview.
                if let Some(Over::Cell(n, k)) = self.over() {
                    self.cell_col = k;
                    self.select(n);
                }
            }
            At::Tile(t) => self.cursor = Some(t),
        }
    }

    /// What the cursor is on in the overview's row, if it is on it.
    fn over(&self) -> Option<Over> {
        let (Strip::Overview, i) = self.strip? else { return None };
        let own = &self.laid.detail.as_ref()?.own;
        let stops = self.stops();
        Some(if i < own.len() { Over::Param(own[i]) } else { Over::Cell((i - own.len()) / stops, (i - own.len()) % stops) })
    }

    /// How many stops a row of the operator table has: a cell for each column (one at least).
    fn stops(&self) -> usize {
        self.fm().map_or(1, |f| f.columns().len().max(1))
    }

    /// The overview's stop of the cell (row, column).
    fn cell_stop(&self, row: usize, col: usize) -> usize {
        self.laid.detail.as_ref().map_or(0, |d| d.own.len()) + row * self.stops() + col.min(self.stops() - 1)
    }

    /// Whether the cursor's tile `t` is on the detail's last (`by` 1) or first (`by` -1) row shown and rows go on that way.
    fn can_page(&self, t: usize, by: i32) -> bool {
        let Some(d) = &self.laid.detail else { return false };
        // The stages' rows come first and do not page; the other controls' rows do.
        t >= d.fixed && t < d.tiles && if by > 0 { (t - d.fixed) / d.columns == (d.tiles - d.fixed - 1) / d.columns && d.row + d.per < d.rows } else { t - d.fixed < d.columns && d.row > 0 }
    }

    /// The detail's rows turned by `by`, one row at a time: whether they moved.
    fn page_rows(&mut self, by: i32) -> bool {
        let Some(d) = &self.laid.detail else { return false };
        let to = (d.row as i32 + by).clamp(0, d.rows.saturating_sub(d.per) as i32) as usize;
        if to == d.row {
            return false;
        }
        self.detail_row = to;
        self.relayout();
        true
    }

    /// The page keys' step on a page with a master: the selected one `by` on, the cursor staying on its place in the
    /// detail (so one parameter is swept across the operators). False at either end: the keys then turn the page.
    fn step_operator(&mut self, by: i32) -> bool {
        let Some(d) = &self.laid.detail else { return false };
        let to = self.operator as i32 + by;
        if to < 0 || to >= d.count as i32 {
            return false;
        }
        self.end_gestures();
        self.edit = Edit::None;
        self.select(to as usize);
        if let Some((Strip::Overview, _)) = self.strip {
            self.strip = Some((Strip::Overview, self.cell_stop(self.operator, self.cell_col)));
        }
        true
    }

    /// The row's stop on its selected entry.
    fn selected(&self, stop: Stop) -> At {
        match stop {
            Stop::Row(Strip::Pages) => At::Bar(stop, self.shown_page().unwrap_or(0)),
            Stop::Row(Strip::Tabs) => At::Bar(stop, self.shown_tab()),
            Stop::Row(Strip::Overview) => At::Bar(stop, self.cell_stop(self.operator, self.cell_col)),
        }
    }

    /// How many entries a row has.
    fn row_len(&self, row: Strip) -> usize {
        match row {
            Strip::Pages => self.model.pages.len(),
            Strip::Tabs => self.tab_names().len(),
            Strip::Overview => self.laid.detail.as_ref().map_or(0, |d| d.own.len() + d.count * self.stops()),
        }
    }

    /// The cursor moved by `by` in reading order: the search, the pages, the tabs, then the tiles. Nothing is
    /// chosen by it: a page or tab opens on a press, and the tiles stop at either end. Moving past a row's end
    /// goes to the next row's selected entry.
    fn step(&mut self, by: i32) {
        let n = self.laid.tiles.len();
        let Some(mut at) = self.at() else {
            if n > 0 {
                self.cursor = Some(if by > 0 { 0 } else { n - 1 });
            }
            return;
        };
        let bars = self.bars();
        for _ in 0..by.abs() {
            let forward = by > 0;
            at = match at {
                At::Tile(t) => {
                    let (columns, per, fixed) = self.laid.detail.as_ref().map_or((1, 1, 0), |d| (d.columns, d.per, d.fixed));
                    if forward && self.can_page(t, 1) && t + 1 == self.laid.detail.as_ref().map_or(0, |d| d.tiles) {
                        // Past the last tile of the rows in view the rows go on: a row on, the cursor at its start.
                        self.page_rows(1);
                        At::Tile((fixed + (per - 1) * columns).min(self.laid.tiles.len().saturating_sub(1)))
                    } else if forward {
                        At::Tile((t + 1).min(self.laid.tiles.len().saturating_sub(1)))
                    } else if t == fixed && self.can_page(t, -1) {
                        self.page_rows(-1);
                        At::Tile((fixed + columns - 1).min(self.laid.tiles.len().saturating_sub(1)))
                    } else if t > 0 {
                        At::Tile(t - 1)
                    } else {
                        match bars.last() {
                            Some(&stop) => self.selected(stop),
                            None => At::Tile(0),
                        }
                    }
                }
                At::Bar(stop, i) => {
                    let len = match stop {
                        Stop::Row(row) => self.row_len(row),
                    };
                    let place = bars.iter().position(|s| *s == stop).unwrap_or(0);
                    if forward && i + 1 < len {
                        At::Bar(stop, i + 1)
                    } else if !forward && i > 0 {
                        At::Bar(stop, i - 1)
                    } else if forward {
                        match bars.get(place + 1) {
                            Some(&next) => self.selected(next),
                            None if n > 0 => At::Tile(0),
                            None => at,
                        }
                    } else if place > 0 {
                        self.selected(bars[place - 1])
                    } else {
                        at
                    }
                }
            };
        }
        self.go(at);
    }

    /// The cursor across (dx) or up and down (dy): over the tiles by rows and columns, from the first row up
    /// to the tabs, the pages and the search, along a row of tabs, and in a column of pages up and down.
    fn arrow(&mut self, dx: i32, dy: i32) {
        let Some(mut at) = self.at() else {
            self.cursor = (!self.laid.tiles.is_empty()).then_some(0);
            return;
        };
        let bars = self.bars();
        let pages = Stop::Row(Strip::Pages);
        let tabs = Stop::Row(Strip::Tabs);
        let side = self.side;
        // The row above `from` (above the tiles when None) that is over the content: the rails are beside the
        // tiles, not over them.
        let above = |from: Option<Stop>| -> Option<Stop> {
            let place = from.and_then(|f| bars.iter().position(|s| *s == f)).unwrap_or(bars.len());
            bars[..place].iter().rev().copied().find(|s| !side || *s != pages && *s != tabs)
        };
        let no_tiles = self.laid.tiles.is_empty();
        let first_tile = |me: At| if no_tiles { me } else { At::Tile(0) };
        let (mut dx, mut dy) = (dx, dy);
        while dx != 0 || dy != 0 {
            let (sx, sy) = if dx != 0 { (dx.signum(), 0) } else { (0, dy.signum()) };
            dx -= sx;
            dy -= sy;
            at = match at {
                At::Tile(t) if sy != 0 && self.can_page(t, sy) => {
                    // Down from the last row in view or up from the first, where rows go on: a row on, the same column.
                    let (columns, per, fixed) = self.laid.detail.as_ref().map_or((1, 1, 0), |d| (d.columns, d.per, d.fixed));
                    self.page_rows(sy);
                    let row = if sy > 0 { per - 1 } else { 0 };
                    At::Tile((fixed + row * columns + (t - fixed) % columns).min(self.laid.tiles.len().saturating_sub(1)))
                }
                At::Tile(t) => match grid::arrow(&self.laid.tiles, t, sx, sy) {
                    Some(to) => At::Tile(to),
                    None if sy < 0 => above(None).map_or(at, |stop| self.selected(stop)),
                    None if sx < 0 && side && bars.contains(&tabs) => self.selected(tabs),
                    None if sx < 0 && side && bars.contains(&pages) => self.selected(pages),
                    None => at,
                },
                // The pages' rail beside the tabs' and the tiles: up and down along it, right to the tabs (or the tiles).
                At::Bar(stop @ Stop::Row(Strip::Pages), i) if side => {
                    let len = self.row_len(Strip::Pages);
                    if sy != 0 {
                        let to = i as i32 + sy;
                        if to < 0 {
                            at
                        } else {
                            At::Bar(stop, (to as usize).min(len - 1))
                        }
                    } else if sx > 0 {
                        if bars.contains(&tabs) {
                            self.selected(tabs)
                        } else {
                            first_tile(at)
                        }
                    } else {
                        at
                    }
                }
                // The tabs' rail beside the pages': up and down along it, left to the pages, right to the tiles.
                At::Bar(stop @ Stop::Row(Strip::Tabs), i) if side => {
                    let len = self.row_len(Strip::Tabs);
                    if sy != 0 {
                        At::Bar(stop, (i as i32 + sy).clamp(0, len as i32 - 1) as usize)
                    } else if sx > 0 {
                        first_tile(at)
                    } else if bars.contains(&pages) {
                        self.selected(pages)
                    } else {
                        at
                    }
                }
                // The operator table: across a row's columns, down and up its rows (up from the first to the view's own
                // parameters, then to the rows above), down from the last to the tiles; the dial goes by all in turn.
                At::Bar(stop @ Stop::Row(Strip::Overview), i) => {
                    let own = self.laid.detail.as_ref().map_or(0, |d| d.own.len());
                    let (stops, count) = (self.stops(), self.laid.detail.as_ref().map_or(0, |d| d.count));
                    if i < own {
                        if sx != 0 {
                            At::Bar(stop, (i as i32 + sx).clamp(0, own as i32 - 1) as usize)
                        } else if sy < 0 {
                            above(Some(stop)).map_or(at, |up| self.selected(up))
                        } else {
                            At::Bar(stop, self.cell_stop(self.operator, self.cell_col))
                        }
                    } else {
                        let (row, col) = ((i - own) / stops, (i - own) % stops);
                        if sx != 0 {
                            At::Bar(stop, self.cell_stop(row, (col as i32 + sx).clamp(0, stops as i32 - 1) as usize))
                        } else if sy < 0 && row > 0 {
                            At::Bar(stop, self.cell_stop(row - 1, col))
                        } else if sy < 0 && own > 0 {
                            At::Bar(stop, 0)
                        } else if sy < 0 {
                            above(Some(stop)).map_or(at, |up| self.selected(up))
                        } else if row + 1 < count {
                            At::Bar(stop, self.cell_stop(row + 1, col))
                        } else {
                            let place = bars.iter().position(|s| *s == stop).unwrap_or(0);
                            match bars.get(place + 1) {
                                Some(&next) => self.selected(next),
                                None => match self.memory.filter(|&m| m < self.laid.tiles.len()) {
                                    Some(m) => At::Tile(m),
                                    None => first_tile(at),
                                },
                            }
                        }
                    }
                }
                At::Bar(stop @ Stop::Row(row), i) => {
                    let len = self.row_len(row);
                    if sx != 0 {
                        let to = i as i32 + sx;
                        At::Bar(stop, to.clamp(0, len as i32 - 1) as usize)
                    } else if sy < 0 {
                        above(Some(stop)).map_or(at, |up| self.selected(up))
                    } else {
                        // Down: the next row, then the tiles (from the overview, the tile it left).
                        let place = bars.iter().position(|s| *s == stop).unwrap_or(0);
                        match bars.get(place + 1) {
                            Some(&next) => self.selected(next),
                            None => match self.memory.filter(|&m| row == Strip::Overview && m < self.laid.tiles.len()) {
                                Some(m) => At::Tile(m),
                                None => first_tile(at),
                            },
                        }
                    }
                }
            };
        }
        self.go(at);
    }

    /// A press on the row's entry the cursor is on: the page or tab opens (the cursor stays on it).
    fn open_entry(&mut self, row: Strip, i: usize) {
        match row {
            Strip::Pages => self.go_page(i),
            Strip::Tabs => self.go_tab(i),
            // A press on one of the view's own parameters gives it the dial; on a cell of the table it switches a mark
            // (mute, solo, mode) or gives the cell's value the dial, which the press or Cancel ends.
            Strip::Overview => {
                match self.over() {
                    Some(Over::Param(p)) => {
                        self.edit = Edit::Param(p);
                        self.begin(self.params[p].id);
                    }
                    Some(Over::Cell(n, k)) => self.confirm_cell(n, k),
                    None => {}
                }
                return;
            }
        }
        self.go(At::Bar(Stop::Row(row), i));
    }

    /// A press on the table's cell (operator `n`, column `k`): a switched cell (mute, solo, mode) switches; another gives
    /// its value the dial; one that cannot be edited does nothing (the readout says why).
    fn confirm_cell(&mut self, n: usize, k: usize) {
        let Some(fm) = self.fm().cloned() else { return };
        let (Some(&column), Some(o)) = (fm.columns().get(k), fm.operators.get(n)) else { return };
        if fm.disabled(n, column, &self.params).is_some() {
            return;
        }
        if column.toggles() {
            self.toggle_cell(n, column);
        } else if let Some(p) = column.param(o) {
            self.edit = Edit::Param(p);
            self.begin(self.params[p].id);
        }
    }

    /// Switches mute, solo or the mode of operator `n`: one edit of the plugin's field; the host's solo, where the plugin
    /// has none, mutes the others over a mute the plugin says is transient (and gives them back when the solo ends).
    fn toggle_cell(&mut self, n: usize, column: fm::Column) {
        let Some(fm) = self.fm().cloned() else { return };
        let Some(o) = fm.operators.get(n) else { return };
        let flip = |s: &Self, p: usize| {
            let q = &s.params[p];
            if s.value(q) > q.min {
                q.min
            } else {
                q.max
            }
        };
        match column {
            fm::Column::Mute => {
                if let Some(m) = o.mute {
                    let to = flip(self, m);
                    self.edit_mute(m, to);
                    if self.soloed.iter().any(|&s| s) {
                        self.baseline[n] = to;
                    }
                }
            }
            fm::Column::Solo => match fm.solo_from(&self.params) {
                fm::SoloFrom::Field => {
                    if let Some(s) = o.solo {
                        let (p, to) = (self.params[s].clone(), flip(self, s));
                        self.set_once(&p, to);
                    }
                }
                fm::SoloFrom::Others => self.solo_others(&fm, n),
                fm::SoloFrom::None => {}
            },
            fm::Column::Mode => {
                if let Some(x) = o.fixed {
                    let (p, to) = (self.params[x].clone(), flip(self, x));
                    self.set_once(&p, to);
                }
            }
            _ => {}
        }
    }

    /// An edit of a mute: a transient one is set alone (no gesture, so no undo step, and nothing the plugin saves); another
    /// is the player's edit as any parameter's.
    fn edit_mute(&mut self, m: usize, to: f64) {
        let p = self.params[m].clone();
        if p.transient {
            self.set(&p, to);
        } else {
            self.set_once(&p, to);
        }
    }

    /// The host's solo of operator `n`: it is soloed or not, and every operator is muted unless it is soloed; with none soloed
    /// the mutes are what they were before the first solo.
    fn solo_others(&mut self, fm: &fm::FmShape, n: usize) {
        let count = fm.operators.len();
        if self.soloed.len() != count {
            self.soloed = vec![false; count];
            self.baseline = vec![0.; count];
        }
        if !self.soloed.iter().any(|&s| s) {
            self.baseline = fm.operators.iter().map(|o| o.mute.map_or(0., |m| self.value(&self.params[m]))).collect();
        }
        self.soloed[n] = !self.soloed[n];
        let any = self.soloed.iter().any(|&s| s);
        for (m, o) in fm.operators.iter().enumerate() {
            let Some(mute) = o.mute else { continue };
            let q = self.params[mute].clone();
            let to = if !any {
                self.baseline[m]
            } else if self.soloed[m] {
                q.min
            } else {
                q.max
            };
            if to != self.value(&q) {
                self.edit_mute(mute, to);
            }
        }
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let outcome = self.handle(action);
        self.publish();
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
            (Edit::Param(p), Action::Move { by, .. }) => self.dial_param(p, by),
            (Edit::Param(p), Action::Arrow { dx, dy }) => self.dial_param(p, dx - dy),
            (Edit::Param(_), Action::Confirm(_) | Action::Cancel(_)) => {
                self.end_gestures();
                self.edit = Edit::None;
            }
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
            // A press on a page or a tab opens that one, and the cursor stays on it.
            (Edit::None, Action::Confirm(_)) if self.strip.is_some() => {
                if let Some((row, i)) = self.strip {
                    self.open_entry(row, i);
                }
            }
            (Edit::None, Action::Move { pane: Some(0), by }) => self.arrow(by, 0),
            (Edit::None, Action::Move { pane: Some(1), by }) => self.arrow(0, by),
            (Edit::None, Action::Move { by, .. }) => self.step(by),
            (Edit::None, Action::Arrow { dx, dy }) => self.arrow(dx, dy),
            (_, Action::Page(by)) => {
                let _ = self.step_operator(by) || self.turn_page(by);
            }
            (Edit::None, Action::End(by)) => {
                let last = self.laid.tiles.len().checked_sub(1);
                self.go(At::Tile(0));
                self.cursor = last.map(|last| if by < 0 { 0 } else { last });
            }
            (Edit::None, Action::Confirm(_)) => match self.cursor {
                Some(c) => self.press(c),
                None => self.cursor = (!self.laid.tiles.is_empty()).then_some(0),
            },
            // Cancel backs out a layer: the cursor on the search, a row of tabs or a tile, the search (the
            // reference's Escape), then the screen.
            (Edit::None, Action::Cancel(_)) => {
                if self.at().is_some() {
                    self.go(At::Tile(0));
                    self.cursor = None;
                } else {
                    if self.search.is_empty() {
                        return Outcome::Back;
                    }
                    self.set_search(String::new());
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// Whether the page has a search field: a plugin of more than 24 parameters, as the reference's.
    fn search_shown(&self) -> bool {
        self.params.len() > TABBED_ABOVE
    }

    /// The search text changed: its matches from every page are laid out from the top.
    fn set_search(&mut self, text: String) {
        self.search = text.trim().to_string();
        self.end_gestures();
        self.edit = Edit::None;
        self.scroll = 0.;
        self.cursor = None;
        self.relayout();
        self.ask_values();
    }

    /// The keyboard's text for the search (Done): an empty one ends the search.
    pub fn entered(&mut self, text: &str) {
        self.set_search(text.to_string());
        self.show();
    }

    /// The keyboard was closed with nothing (Cancel).
    pub fn not_entered(&mut self) {}

    // Touch.

    /// The plugin's name, for the bar's title.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The screen's own action, which the top bar carries: the search, for a plugin of more than 24 parameters; the
    /// text searched for while one is on.
    pub fn bar_tools(&self) -> Vec<crate::topbar::Tool> {
        if !self.search_shown() {
            return vec![];
        }
        vec![crate::topbar::Tool::new(if self.search.is_empty() { "Search".to_string() } else { format!("Search: {}", self.search) })]
    }

    /// The cursor is on the first stop, whichever it is: moving up leaves for the top bar.
    pub fn at_top(&self) -> bool {
        match self.at() {
            Some(At::Bar(Stop::Row(row), _)) => self.bars().first() == Some(&Stop::Row(row)) && self.edit == Edit::None,
            Some(At::Tile(0)) => self.bars().is_empty() && self.edit == Edit::None,
            _ => false,
        }
    }

    /// Back from anywhere on the screen: a dropdown that is open closes, else the screen goes; a value being
    /// changed is let go (its gesture ends).
    pub fn back(&mut self) -> Outcome {
        self.end_gestures();
        let open = matches!(self.edit, Edit::List(_));
        self.edit = Edit::None;
        self.cursor = None;
        self.strip = None;
        self.show();
        if open { Outcome::Stay } else { Outcome::Back }
    }

    /// A tap on the search button of the bar: the keyboard for its text.
    pub fn search_tap(&mut self) -> Outcome {
        self.end_gestures();
        self.edit = Edit::None;
        self.cursor = None;
        self.strip = None;
        self.show();
        Outcome::Prompt("Search parameters".into(), self.search.clone())
    }

    /// A drag begins on a tile's part.
    pub fn grab(&mut self, tile: i32, part: i32) {
        let Some(p) = self.param_of(tile as usize, part as usize) else { return };
        self.end_gestures();
        self.edit = Edit::None;
        self.strip = None;
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

    /// A touch on a view's panel at (x, y) in the panel's pixels: whether it took a handle (its parameters'
    /// gestures begin; the cursor and a value being changed with the dial let go).
    pub fn view_down(&mut self, panel: i32, x: f32, y: f32) -> bool {
        let k = panel as usize;
        let Some(p) = self.laid.panels.get(k) else { return false };
        let Some((held, ids, operator)) = self.with_view(p, |c| views::grab(&p.view, c, (x, y)).map(|h| (h.clone(), views::touched(&p.view, c, &h), views::operator_held(&p.view, c, &h)))) else { return false };
        self.end_gestures();
        self.edit = Edit::None;
        self.cursor = None;
        self.strip = None;
        if let Some(n) = operator {
            self.select(n);
        }
        if let views::Held::Rows(by) = held {
            self.page_rows(by);
            self.show();
            return true;
        }
        if let views::Held::Operator(by) = held {
            // The heading's selector: a step to the previous or next operator, the row of controls kept.
            let to = (self.operator as i32 + by).clamp(0, self.laid.detail.as_ref().map_or(0, |d| d.count) as i32 - 1);
            self.select(to as usize);
            self.show();
            return true;
        }
        // A tap on a cell that cannot be edited puts the cursor there, so the readout says why.
        if let views::Held::Cell { row, col } = held {
            self.strip = Some((Strip::Overview, self.cell_stop(row, col)));
            self.cell_col = col;
        }
        for i in ids {
            self.begin(self.params[i].id);
        }
        let on_grab = views::drags_on_grab(&held);
        self.view_held = Some((k, held));
        if on_grab {
            self.view_move(panel, x, y);
        }
        self.show();
        true
    }

    /// Sets what a view says (each parameter, its value), but not what already is.
    /// All of it is one `params.set`: the engine makes one undo step of the values of one request, so a handle that moves
    /// two parameters (a stage's rate and its level) is one step, not two.
    fn view_sets(&mut self, sets: Vec<(usize, f64)>) {
        let changed: Vec<(Param, f64)> = sets.into_iter().map(|(i, to)| (self.params[i].clone(), to)).filter(|(p, to)| *to != self.value(p)).collect();
        if changed.is_empty() {
            return;
        }
        self.message.clear();
        let values: Vec<Value> = changed.iter().map(|(p, to)| json!({"node": self.node, "param": p.id, "value": to})).collect();
        for (p, to) in &changed {
            self.values.insert(p.id, *to);
        }
        self.out.push((Request::Set, "params.set", json!({ "values": values })));
        for (p, to) in &changed {
            self.ask_text(p.id, *to);
        }
    }

    pub fn view_move(&mut self, panel: i32, x: f32, y: f32) {
        let k = panel as usize;
        let (Some(p), Some((held_panel, held))) = (self.laid.panels.get(k), self.view_held.clone()) else { return };
        if held_panel != k {
            return;
        }
        let sets = self.with_view(p, |c| views::drag(&p.view, c, &held, (x, y)));
        self.view_sets(sets);
        self.show();
    }

    /// The touch ends at (x, y): what letting go sets (a tap on the routing), then the gestures end.
    pub fn view_up(&mut self, panel: i32, x: f32, y: f32) {
        let k = panel as usize;
        if let (Some(p), Some((held_panel, held))) = (self.laid.panels.get(k).cloned(), self.view_held.clone()) {
            let p = &p;
            if held_panel == k {
                let sets = self.with_view(p, |c| views::release(&p.view, c, &held, (x, y)));
                self.view_sets(sets);
                // Letting go on the switched cell it began on (mute, solo, mode) switches it: a tap.
                if let views::Held::Mark { row, col } = held {
                    let again = self.with_view(p, |c| views::grab(&p.view, c, (x, y)));
                    if again == Some(views::Held::Mark { row, col }) {
                        if let Some(&column) = self.fm().and_then(|f| f.columns().get(col).copied()).as_ref() {
                            self.toggle_cell(row, column);
                        }
                    }
                }
            }
        }
        self.view_held = None;
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
        self.strip = None;
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

    /// A tap on a page.
    pub fn show_page(&mut self, page: i32) {
        self.go_page(page as usize);
        self.show();
    }

    /// A tap on a tab of the page shown.
    pub fn show_tab(&mut self, tab: i32) {
        self.go_tab(tab as usize);
        self.show();
    }

    /// The first entry of a page (or of a search's end: the page chosen ends the search).
    fn go_page(&mut self, page: usize) {
        if let Some(e) = self.entries.iter().position(|e| e.page == page) {
            self.turn_page(e as i32 - self.entry as i32);
        }
    }

    fn go_tab(&mut self, tab: usize) {
        let page = self.entries.get(self.entry).map_or(0, |e| e.page);
        if let Some(e) = self.entries.iter().position(|e| e.page == page && e.tab == Some(tab)) {
            self.turn_page(e as i32 - self.entry as i32);
        }
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
pub fn level(module: &str, n: usize) -> &str {
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
pub fn numbered(name: &str) -> (String, i64) {
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
pub fn tabs_for(params: &[Param], page: &[usize]) -> Vec<String> {
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
