// SPDX-License-Identifier: AGPL-3.0-or-later
//! The grid framework: tiles laid out from the plugin's own layout data (`params.list` with its
//! `oscairn.ui` hints: widget, priority, group, range, rich views), as the JUCE ParameterGrid
//! builds them (hosts/standalone/ParameterGrid.cpp `build`); the notes and the checklist are
//! ui/GRID.md. This module is the data side: which control each parameter gets, where each tile,
//! card, heading and view panel goes, the cursor's moves over tiles in two dimensions, and an
//! envelope's curve. The widgets are slint/grid.slint; a screen on it is params.rs.

use crate::views::{self, View};
use serde_json::Value;

pub const FLAG_HIDDEN: u64 = 1 << 2;
pub const FLAG_READONLY: u64 = 1 << 3;
/// Up to this many values: buttons side by side (`auto`).
const MAX_CHOICES: usize = 4;
/// More values than this get a slider.
const MAX_LISTED: usize = 64;
/// Sizes in the shell's logical pixels, the JUCE grid's in touch mode (RFC 0019: a control 44 high).
/// The gap between sections when they are apart; a theme whose sections join has none (`theme::section_gap`).
pub const SECTION_GAP: f32 = 6.;
const TILE: f32 = 84.;
const KNOB_EXTRA: f32 = 36.;
const HEADING: f32 = 30.;
/// The views' heights in touch mode (ParameterGrid.cpp build): envelope, LFO, filter, key map. The FM operator
/// view has no fixed one: it is the top pane of a master and detail block that fills the area.
const ENVELOPE: f32 = 180.;
const LFO: f32 = 140.;
const FILTER: f32 = 160.;
const MAP: f32 = 180.;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Param {
    pub id: u32,
    pub name: String,
    pub module: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub stepped: bool,
    pub flags: u64,
    pub priority: u64,
    /// The role the plugin declares (`cutoff`, `attack`, `mix`...), empty when it declares none.
    pub role: String,
    /// The hint's widget as `params.list` names it ("" or "auto": the host decides).
    pub widget: String,
    pub group: u64,
    pub range_high: Option<u32>,
    pub view: u64,
    pub field: String,
    pub index: u64,
    /// A time or rate field's stage length at evenly spaced values.
    pub seconds: Vec<f64>,
    /// An algorithm's routing at each value from its minimum: eight operators' input masks, the
    /// carriers' mask, and the feedback's operator from and to (1 to 8; 0 none).
    pub algorithms: Vec<[u8; 11]>,
    /// PROVISIONAL (draft 6's `param_tab`, not yet in the engine's `params.list`; docs/control-protocol.md and
    /// ui/PERFORMANCE-APP.md "Items without a request"): the id of the page or tab the plugin puts this on.
    pub declared_tab: Option<u32>,
    /// PROVISIONAL (draft 6's `value_icon`): the `enum oscairn_icon` of each value from the minimum, 0 none.
    pub icons: Vec<u32>,
    /// PROVISIONAL (draft 7, notes/ui-fm-fields-proposal.md): a mute that is not the player's edit, not saved with the
    /// sound and not an undo step (`transient` in `params.list`); the host may then derive a solo from it.
    pub transient: bool,
}

impl Param {
    pub fn values(&self) -> usize {
        if self.stepped {
            (self.max - self.min).round().max(0.) as usize + 1
        } else {
            0
        }
    }
    pub fn readonly(&self) -> bool {
        self.flags & FLAG_READONLY != 0
    }
    pub fn norm(&self, v: f64) -> f64 {
        if self.max > self.min {
            ((v - self.min) / (self.max - self.min)).clamp(0., 1.)
        } else {
            0.
        }
    }
    pub fn denorm(&self, n: f64) -> f64 {
        let v = self.min + n.clamp(0., 1.) * (self.max - self.min);
        if self.stepped {
            v.round()
        } else {
            v
        }
    }
    /// Centered on zero (pan, detune): a symmetric range whose default is zero.
    pub fn bipolar(&self) -> bool {
        self.min < 0. && self.max > 0. && (self.min + self.max).abs() < 1e-9 * (self.max - self.min) && self.default == 0.
    }
}

pub fn parse(list: &Value) -> Vec<Param> {
    list["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| Param {
            id: p["id"].as_u64().unwrap_or(0) as u32,
            name: p["name"].as_str().unwrap_or_default().into(),
            module: p["module"].as_str().unwrap_or_default().into(),
            min: p["minimum"].as_f64().unwrap_or(0.),
            max: p["maximum"].as_f64().unwrap_or(1.),
            default: p["defaultValue"].as_f64().unwrap_or(0.),
            stepped: p["stepped"].as_bool().unwrap_or(false),
            flags: p["flags"].as_u64().unwrap_or(0),
            priority: p["priority"].as_u64().unwrap_or(0),
            role: p["role"].as_str().unwrap_or_default().into(),
            widget: p["widget"].as_str().unwrap_or_default().into(),
            group: p["group"].as_u64().unwrap_or(0),
            range_high: p["rangeHigh"].as_u64().map(|v| v as u32),
            view: p["view"].as_u64().unwrap_or(0),
            field: p["field"].as_str().unwrap_or_default().into(),
            index: p["index"].as_u64().unwrap_or(0),
            seconds: p["seconds"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect(),
            algorithms: p["algorithms"].as_array().into_iter().flatten().map(routing).collect(),
            declared_tab: p["uiTab"].as_u64().map(|v| v as u32),
            icons: p["valueIcons"].as_array().into_iter().flatten().map(|v| v.as_u64().unwrap_or(0) as u32).collect(),
            transient: p["transient"].as_bool().unwrap_or(false),
        })
        .collect()
}

/// One algorithm's routing as `params.list` writes it (`inputs`, `carriers`, `feedbackFrom`, `feedbackTo`).
fn routing(a: &Value) -> [u8; 11] {
    let mut r = [0u8; 11];
    for (k, v) in a["inputs"].as_array().into_iter().flatten().take(8).enumerate() {
        r[k] = v.as_u64().unwrap_or(0) as u8;
    }
    r[8] = a["carriers"].as_u64().unwrap_or(0) as u8;
    r[9] = a["feedbackFrom"].as_u64().unwrap_or(0) as u8;
    r[10] = a["feedbackTo"].as_u64().unwrap_or(0) as u8;
    r
}

/// The control a tile gets (slint/grid.slint's `kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Slider = 0,
    Knob = 1,
    Buttons = 2,
    Switch = 3,
    List = 4,
    Range = 5,
    /// Read only: the value's text alone.
    Value = 6,
    // shortcut: no params.list field selects these two yet (RFC 0042, draft 6); Play's slider row will.
    #[allow(dead_code)]
    VSlider = 7,
    #[allow(dead_code)]
    Drawbar = 8,
}

/// Off and On (or 0 and 1, a plugin without names for them): a switch, not two buttons.
fn on_off(a: &str, b: &str) -> bool {
    (a.eq_ignore_ascii_case("off") && b.eq_ignore_ascii_case("on")) || (a == "0" && b == "1")
}

/// The plugin's hint where it fits, else by its values: a few as buttons (Off and On a switch), up
/// to 64 as a list, the rest a slider. `labels`: the value texts of a stepped parameter's values,
/// once known. `params`: all of them, for a range's high end.
pub fn control(p: &Param, labels: &[String], params: &[Param]) -> Control {
    if p.readonly() {
        return Control::Value;
    }
    let count = p.values();
    let c = match p.widget.as_str() {
        "knob" => Control::Knob,
        "slider" => Control::Slider,
        "buttons" if count <= 8 => Control::Buttons,
        "buttons" | "list" if count <= MAX_LISTED => Control::List,
        "buttons" | "list" => Control::Slider,
        "switch" => Control::Switch,
        "range" => Control::Range,
        _ if (2..=MAX_CHOICES).contains(&count) && !(count == 2 && labels.len() == 2 && on_off(&labels[0], &labels[1])) && !(count == 2 && labels.len() < 2) => Control::Buttons,
        _ if count == 2 => Control::Switch,
        _ if count > 2 && count <= MAX_LISTED => Control::List,
        _ => Control::Slider,
    };
    if c == Control::Range && !p.range_high.is_some_and(|h| h != p.id && params.iter().any(|q| q.id == h)) {
        return Control::Slider;
    }
    c
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn union(&self, o: &Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect { x, y, w: self.right().max(o.right()) - x, h: (self.y + self.h).max(o.y + o.h) - y }
    }
    fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() + 0.01 && o.y + o.h <= self.y + self.h + 0.01
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub param: usize,
    pub rect: Rect,
    pub control: Control,
    /// In a linked run (drawn as one card with its neighbors).
    pub linked: bool,
    /// A compact control of a master's detail (`compact_height`): smaller than a tile, for a dense row.
    pub compact: bool,
    /// The envelope stage the control moves, which names it and says how its value reads, where the tile is one of a
    /// master's Time or Level rows.
    pub stage: Option<Stage>,
}

/// An envelope control of a master's detail: its name ("Hold", "Decay 1") and what it moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub name: String,
    pub kind: StageKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    /// A time: how long the stage takes.
    Time,
    /// A rate: how fast the stage runs, higher is faster; the time it takes depends on the levels it runs between.
    Rate,
    /// A level the envelope reaches.
    Level,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub rect: Rect,
    pub view: View,
    /// The view's parameters (indexes into the parameters), which an envelope's, filter's and LFO's shapes refer to by place.
    pub params: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Laid {
    pub tiles: Vec<Tile>,
    pub headings: Vec<(f32, String)>,
    pub cards: Vec<Rect>,
    pub panels: Vec<Panel>,
    pub height: f32,
    /// The page's master and detail, when a view on it has an overview (ui/GRID.md "Master and detail").
    pub detail: Option<Detail>,
}

/// A master and detail block of a page: the view's overview on top (panel 0), under it the heading of the one
/// selected (panel 1) and that one's parameters as the page's tiles (the first `tiles` of `Laid::tiles`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    pub view: u64,
    /// The selected one's index in the view (1 to 8).
    pub index: u64,
    /// How many there are to select (the overview's lines).
    pub count: usize,
    /// The view's own parameters (index 0), which the overview carries.
    pub own: Vec<usize>,
    /// Every parameter of the selected one in the plugin's priority order (the knobs play these first).
    pub params: Vec<usize>,
    pub columns: usize,
    /// Rows of tiles it has in all, the first shown, and how many are.
    pub rows: usize,
    pub row: usize,
    pub per: usize,
    /// The tiles of the detail in view: the first of the page's.
    pub tiles: usize,
    /// How many of them come first and are not paged: the Time and Level rows of the envelope's stages.
    pub fixed: usize,
}

/// What a page with a master and detail is laid out for.
#[derive(Clone, Copy)]
pub struct Master<'a> {
    /// The area's height: the block fills it (and goes past it only when the overview's least does not fit).
    pub height: f32,
    /// The one selected (0 for the first) and the first row of its tiles shown.
    pub selected: usize,
    pub row: usize,
    /// The theme's type scale and spacing, which the overview's sizes follow.
    pub scale: f32,
    pub spacing: f32,
    /// What each index is called ("Operator 3"), where the plugin names them (its tabs, before they were folded in).
    pub names: &'a [(u64, String)],
}

impl Default for Master<'_> {
    fn default() -> Self {
        Master { height: 405., selected: 0, row: 0, scale: 1., spacing: 1., names: &[] }
    }
}

/// The detail heading's height.
pub const DETAIL_HEAD: f32 = views::DETAIL_HEAD;

/// The view on a page whose indexed siblings it shows as master and detail: the first with an overview (an FM
/// operator view: the one with an algorithm). A key map's zones or a sampler's elements are views that could have
/// one; only this has an overview built.
pub fn master_view(params: &[Param], members: &[usize]) -> Option<u64> {
    members.iter().map(|&i| &params[i]).find(|p| p.view != 0 && p.field == "algorithm").map(|p| p.view)
}

/// How many columns a width holds: two to six, a tile about 210 across.
pub fn columns(width: f32) -> usize {
    ((width / 210.) as usize).clamp(2, 6)
}

/// A parameter whose tile draws another's too: a range's high end has none of its own.
fn drawn_by_another(params: &[Param], i: usize) -> bool {
    params.iter().any(|q| q.widget == "range" && q.range_high == Some(params[i].id) && q.id != params[i].id)
}

/// The plugin's priority order: numbered first, then the rest in its order.
pub fn by_priority(params: &[Param], indices: &mut [usize]) {
    indices.sort_by_key(|&i| if params[i].priority == 0 { u64::MAX } else { params[i].priority });
}

/// The tiles, headings, cards and view panels of `groups` (each a heading and its parameters) at
/// `width`, as `ParameterGrid::build`: views first across the row, then runs of linked tiles (a
/// hint's group, an envelope's stages) kept together, the rest one tile each, by priority.
#[cfg(test)]
pub fn layout(params: &[Param], groups: &[(String, Vec<usize>)], width: f32, gap: f32, control_of: &dyn Fn(usize) -> Control) -> Laid {
    layout_with(params, groups, width, gap, control_of, &Master::default())
}

/// `layout` with what a master and detail block needs: the area's height, the one selected and the row shown.
pub fn layout_with(params: &[Param], groups: &[(String, Vec<usize>)], width: f32, gap: f32, control_of: &dyn Fn(usize) -> Control, master: &Master) -> Laid {
    let mut out = Laid::default();
    let cols = columns(width);
    let tile_w = (width - gap * (cols - 1) as f32) / cols as f32;
    let groups: Vec<(String, Vec<usize>)> = groups
        .iter()
        .map(|(h, m)| {
            let mut m: Vec<usize> = m.iter().copied().filter(|&i| !drawn_by_another(params, i)).collect();
            by_priority(params, &mut m);
            (h.clone(), m)
        })
        .filter(|(_, m)| !m.is_empty())
        .collect();
    let knobs = groups.iter().any(|(_, m)| m.iter().any(|&i| control_of(i) == Control::Knob));
    let height = if knobs { TILE + KNOB_EXTRA } else { TILE };
    let mut y = 0.;
    for (heading, members) in &groups {
        if !heading.is_empty() {
            out.headings.push((y, heading.replace('/', " › ")));
            y += HEADING;
        }
        // A view with an overview: its siblings are a master and a detail, not tiles (once on a page).
        let rest: Vec<usize>;
        let mut members = members;
        let mut block = false;
        if let (None, Some(view)) = (&out.detail, master_view(params, members)) {
            y += master_block(&mut out, params, members, view, width, gap, control_of, master, y);
            rest = members.iter().copied().filter(|&i| !(params[i].view == view && params[i].index <= 8)).collect();
            members = &rest;
            block = true;
        }
        if block && !members.is_empty() {
            y += gap + 6.;
        }
        let members = members;
        // The envelopes the plugin describes, each a panel across the row, then one recognized by name.
        let mut described: Vec<Vec<usize>> = Vec::new();
        let mut keys: Vec<(u64, u64)> = Vec::new();
        let mut maps: Vec<(u64, u64)> = Vec::new(); // views and indexes of key and velocity maps
        for &i in members {
            let p = &params[i];
            if p.view == 0 {
                continue;
            }
            let list = if is_envelope(&p.field) { &mut keys } else { &mut maps };
            if (is_envelope(&p.field) || views::is_map_field(&p.field)) && !list.contains(&(p.view, p.index)) {
                list.push((p.view, p.index));
            }
        }
        let panel = |out: &mut Laid, y: &mut f32, view: View, high: f32, list: Vec<usize>| {
            out.panels.push(Panel { rect: Rect { x: 0., y: *y, w: width, h: high }, view, params: list });
            *y += high + gap;
        };
        // The FM operator view where its algorithm is, then an envelope per view and index.
        // (A second operator view on a page is not a master: it keeps no panel, its parameters stay tiles.)
        for (view, index) in keys {
            let list: Vec<usize> = members.iter().copied().filter(|&i| params[i].view == view && params[i].index == index && is_envelope(&params[i].field)).collect();
            let fields: Vec<&str> = list.iter().map(|&i| params[i].field.as_str()).collect();
            let shape = envelope_of_fields(&fields);
            if shape.points.is_empty() {
                continue;
            }
            described.push(linked_order(&shape).into_iter().map(|k| list[k]).collect());
            panel(&mut out, &mut y, View::Envelope(shape), ENVELOPE, list);
        }
        for (view, index) in maps {
            let shape = views::map_of(params, view, index);
            if !shape.empty() {
                panel(&mut out, &mut y, View::Map(shape, index), MAP, vec![]);
            }
        }
        // Among the parameters in no view, by name: a filter, an LFO, then an envelope (their tiles' runs
        // go envelope, LFO, filter).
        let plain: Vec<usize> = members.iter().copied().filter(|&i| params[i].view == 0).collect();
        let names: Vec<&str> = plain.iter().map(|&i| params[i].name.as_str()).collect();
        let (filter, lfo) = (views::filter_of(&names), views::lfo_of(&names));
        if !filter.empty() {
            panel(&mut out, &mut y, View::Filter(filter.clone()), FILTER, plain.clone());
        }
        if !lfo.empty() {
            panel(&mut out, &mut y, View::Lfo(lfo.clone()), LFO, plain.clone());
        }
        let named = envelope_of_names(&names);
        if !named.points.is_empty() {
            described.push(linked_order(&named).into_iter().map(|k| plain[k]).collect());
            panel(&mut out, &mut y, View::Envelope(named), ENVELOPE, plain.clone());
        }
        let mut recognized: Vec<Vec<usize>> = Vec::new();
        if !lfo.empty() {
            recognized.push(lfo.linked_order().into_iter().map(|k| plain[k]).collect());
        }
        if !filter.empty() {
            recognized.push(filter.linked_order().into_iter().map(|k| plain[k]).collect());
        }
        // Runs: the plugin's groups, then each envelope's stages not already in one; others alone.
        let mut linked: Vec<Vec<usize>> = Vec::new();
        for &i in members {
            let g = params[i].group;
            if g == 0 {
                continue;
            }
            match linked.iter_mut().find(|r| params[r[0]].group == g) {
                Some(r) => r.push(i),
                None => linked.push(vec![i]),
            }
        }
        for order in described.iter().chain(&recognized) {
            let next: Vec<usize> = order.iter().copied().filter(|&i| params[i].group == 0 && members.contains(&i)).collect();
            if next.len() > 1 {
                linked.push(next);
            }
        }
        let mut done = vec![false; params.len()];
        let mut runs: Vec<Vec<usize>> = Vec::new();
        for &i in members {
            if done[i] {
                continue;
            }
            let run = linked.iter().find(|r| r.contains(&i)).cloned().unwrap_or_else(|| vec![i]);
            let next: Vec<usize> = run.into_iter().filter(|&j| !std::mem::replace(&mut done[j], true)).collect();
            if !next.is_empty() {
                runs.push(next);
            }
        }
        let mut column = 0;
        let start = out.tiles.len();
        for run in &runs {
            let group = run.len() > 1;
            // A group that does not fit what is left of the row starts the next one; the tile before
            // it widens to the row's end, so the row has no hole.
            if column > 0 && (column == cols || (group && column + run.len() > cols)) {
                if column < cols {
                    if let Some(last) = out.tiles.last_mut().filter(|t| !t.linked) {
                        last.rect.w = width - last.rect.x;
                    }
                }
                column = 0;
                y += height + gap;
            }
            let mut card: Option<Rect> = None;
            for (k, &param) in run.iter().enumerate() {
                // A group wider than the screen goes on in the next row, its rows touching.
                if column == cols {
                    column = 0;
                    y += height + if group { 0. } else { gap };
                }
                let mut cell = Rect { x: column as f32 * (tile_w + gap), y, w: tile_w, h: height };
                if group {
                    if k + 1 < run.len() && column + 1 < cols {
                        cell.w = tile_w + gap; // touches the next
                    }
                    if k + 1 == run.len() && run.len() > cols {
                        cell.w = width - cell.x; // a clean rectangle; nothing after it in the row
                        column = cols - 1;
                    }
                    card = Some(card.map_or(cell, |c| c.union(&cell)));
                }
                out.tiles.push(Tile { param, rect: cell, control: control_of(param), linked: group, compact: false, stage: None });
                column += 1;
            }
            if let Some(c) = card {
                out.cards.push(c);
            }
        }
        if out.tiles.len() > start {
            y += height + gap + 6.;
        }
    }
    // A tile is linked when a card holds it (a single tile widened stays alone).
    for t in &mut out.tiles {
        t.linked = out.cards.iter().any(|c| c.contains(&t.rect));
    }
    out.height = y.max(1.);
    out
}

/// A compact control's widget for a parameter whose plain control is `control`: split buttons need more width than a
/// compact control has, so a few values are a list there; a range and the upright sliders are a plain slider.
fn compact_control(control: Control) -> Control {
    match control {
        Control::Buttons => Control::List,
        Control::Range | Control::VSlider | Control::Drawbar => Control::Slider,
        c => c,
    }
}

/// The master and detail block of a page, ui/GRID.md "Master and detail": the overview (panel 0: the algorithm, a
/// graph of every operator, the operator table), under it the heading of the selected operator (panel 1), then its
/// envelope as a graph (panel 2) with its stages' controls over it: a Time row and a Level row, each control in the
/// slot of the stage it moves; then its other parameters as compact controls, as many to a row as fit up to eight.
/// Appends the panels and tiles to `out` and returns the block's height. The block fills the area; what the overview
/// keeps, in order: its table's rows at the least height, the detail's controls, the graph's least, then it grows.
#[allow(clippy::too_many_arguments)]
fn master_block(out: &mut Laid, params: &[Param], members: &[usize], view: u64, width: f32, gap: f32, control_of: &dyn Fn(usize) -> Control, master: &Master, y: f32) -> f32 {
    // Inside the detail the gaps are a third less than the sections' (a dense panel); a joined theme's stay none.
    let cg = gap * 2. / 3.;
    let fm = views::fm_of(params, view);
    let count = fm.operators.len();
    let selected = master.selected.min(count.saturating_sub(1));
    let index = selected as u64 + 1;
    let mut own: Vec<usize> = members.iter().copied().filter(|&i| params[i].view == view && params[i].index == 0).collect();
    by_priority(params, &mut own);
    let of = |n: u64| -> Vec<usize> {
        let mut v: Vec<usize> = members.iter().copied().filter(|&i| params[i].view == view && params[i].index == n && !drawn_by_another(params, i)).collect();
        by_priority(params, &mut v);
        v
    };
    let list = of(index);
    let (scale, spacing) = (master.scale, master.spacing);
    let small = 13. * scale;
    let sc = compact_height(scale, spacing);
    let cols = compact_columns(width, cg);
    // The selected operator's envelope, by name: its stages' time and level controls.
    let fields: Vec<&str> = fm.operators.get(selected).map_or(vec![], |o| o.envelope.iter().map(|&i| params[i].field.as_str()).collect());
    let env = envelope_of_fields(&fields);
    let named = stages(&env, &fields);
    let global = |local: usize| fm.operators[selected].envelope[local];
    let label = 3.6 * small;
    let slots = stage_slots(&env, label, width - NODE_RADIUS);
    let has_stages = !named.cells.is_empty();
    let mut level_free: Vec<bool> = vec![true; named.cells.len()];
    for (k, cell) in named.cells.iter().enumerate() {
        if cell.level.is_some() {
            level_free[k] = false;
        }
    }
    let slot_rect = |k: usize, row_y: f32| -> Rect {
        let (a, b) = slots[k];
        let x = a + cg / 2.;
        let right = if k + 1 == slots.len() { width } else { b - cg / 2. };
        Rect { x, y: row_y, w: (right - x).max(0.), h: sc }
    };
    // The rows' places come later; the stage tiles are made with a row of 0 and moved.
    let mut stage_tiles: Vec<(usize, usize, Stage, bool)> = Vec::new(); // (param, slot, stage, in the level row)
    for (k, cell) in named.cells.iter().enumerate() {
        if let Some((t, rate)) = cell.time {
            stage_tiles.push((global(t), k, Stage { name: cell.name.clone(), kind: if rate { StageKind::Rate } else { StageKind::Time } }, false));
        }
    }
    for (k, cell) in named.cells.iter().enumerate() {
        if let Some((l, name)) = &cell.level {
            stage_tiles.push((global(*l), k, Stage { name: name.clone(), kind: StageKind::Level }, true));
        }
    }
    // The start level of an envelope that has one of its own goes under the first stage when that has no level.
    if let (Some(s), Some(k)) = (named.start, level_free.iter().position(|&f| f)) {
        if k == 0 {
            stage_tiles.push((global(s), 0, Stage { name: "Start".into(), kind: StageKind::Level }, true));
        }
    }
    let in_stages: Vec<usize> = stage_tiles.iter().map(|t| t.0).collect();
    // The mute and solo fields are the table's marks, not controls of the row.
    let rest: Vec<usize> = list.iter().copied().filter(|i| !in_stages.contains(i) && !matches!(params[*i].field.as_str(), "operatorMute" | "operatorSolo")).collect();
    let has_levels = stage_tiles.iter().any(|t| t.3);
    let env_rows = if !has_stages { 0 } else if has_levels { 2 } else { 1 };
    let rows_of = |n: usize| n.div_ceil(cols);
    let rest_rows = rows_of(rest.len());
    let stages_h = |graph: f32| -> f32 {
        if env_rows == 0 {
            0.
        } else {
            (if graph > 0. { graph + cg } else { 0. }) + env_rows as f32 * sc + (env_rows - 1) as f32 * cg
        }
    };
    let params_h = |r: usize| if r == 0 { 0. } else { r as f32 * sc + (r - 1) as f32 * cg };
    // The detail under the overview: the heading, the stages (their graph over the two rows), the other controls.
    let detail_h = |graph: f32, r: usize| DETAIL_HEAD + if env_rows > 0 { cg + stages_h(graph) } else { 0. } + if r > 0 { cg + params_h(r) } else { 0. };
    let area = (master.height - y).max(0.);
    let (least, pref, greatest) = (crate::fm::least_height(count, scale, spacing), crate::fm::preferred_height(count, scale, spacing), crate::fm::greatest_height(count, scale, spacing));
    let (gmin, gmax) = (4. * small, 9. * small);
    // Rows of the other controls: one, and more while the overview keeps what it prefers and the graph its least.
    let mut per = rest_rows.min(1);
    for r in 2..=rest_rows {
        if area - gap - detail_h(if has_stages { gmin } else { 0. }, r) >= pref {
            per = r;
        }
    }
    let room = area - gap - detail_h(0., per);
    let (top, graph) = if env_rows > 0 && room - cg - least >= gmin {
        let mut top = (room - cg - gmin).clamp(least, pref);
        let mut graph = room - cg - top;
        if graph > gmax {
            top = (room - cg - gmax).min(greatest).max(top);
            graph = room - cg - top;
        }
        (top, graph)
    } else {
        (room.max(least), 0.)
    };
    let row = master.row.min(rest_rows.saturating_sub(per));
    let title = master.names.iter().find(|n| n.0 == index).map_or_else(|| format!("Operator {index}"), |n| n.1.clone());
    out.panels.push(Panel { rect: Rect { x: 0., y, w: width, h: top }, view: View::Fm(fm.clone()), params: vec![] });
    let head = Rect { x: 0., y: y + top + gap, w: width, h: DETAIL_HEAD };
    let rows = rest_rows;
    out.panels.push(Panel { rect: head, view: View::Detail(views::DetailHead { color: selected, title, count, index: selected, rows, row, per }), params: vec![] });
    let mut next = head.y + DETAIL_HEAD + cg;
    let before = out.tiles.len();
    if env_rows > 0 {
        let rect = Rect { x: 0., y: next, w: width, h: stages_h(graph) };
        out.panels.push(Panel { rect, view: View::Stages(views::StagesShape { fm: fm.clone(), graph, row: sc, gap: cg, label, rows: true }), params: vec![] });
        let time_y = next + if graph > 0. { graph + cg } else { 0. };
        for (param, k, stage, level) in stage_tiles {
            let rect = slot_rect(k, if level { time_y + sc + cg } else { time_y });
            let control = if params[param].readonly() { Control::Value } else { Control::Knob };
            out.tiles.push(Tile { param, rect, control, linked: false, compact: true, stage: Some(stage) });
        }
        next += stages_h(graph) + cg;
    }
    let fixed = out.tiles.len() - before;
    let tile_w = (width - cg * (cols - 1) as f32) / cols as f32;
    let shown: Vec<usize> = rest.iter().copied().skip(row * cols).take(per * cols).collect();
    for (k, &param) in shown.iter().enumerate() {
        let rect = Rect { x: (k % cols) as f32 * (tile_w + cg), y: next + (k / cols) as f32 * (sc + cg), w: tile_w, h: sc };
        out.tiles.push(Tile { param, rect, control: compact_control(control_of(param)), linked: false, compact: true, stage: None });
    }
    out.detail = Some(Detail { view, index, count, own, params: list, columns: cols, rows, row, per, tiles: out.tiles.len() - before, fixed });
    top + gap + detail_h(graph, per)
}

/// What a row of knob tiles is as high as: a tile (84) and the knob's extra (36), as the pages' tiles with a knob.
pub const KNOB_ROW: f32 = TILE + KNOB_EXTRA;

/// `count` tiles side by side in an area `width` across with `gap` between them, each `height` high, the same
/// size: a row of the grid's tiles for a screen that shows a few controls (Play's knob row). They are placed
/// in the area's own pixels; a `Tile`'s insides (the name line, the control under it, a knob at most 76
/// across) follow from its size.
pub fn row_layout(count: usize, width: f32, height: f32, gap: f32) -> Vec<Rect> {
    if count == 0 {
        return vec![];
    }
    let w = ((width - gap * (count - 1) as f32) / count as f32).max(0.);
    (0..count).map(|i| Rect { x: i as f32 * (w + gap), y: 0., w, h: height }).collect()
}

/// What a tile of a row shows: the control, its name and value as text, where it stands from 0 to 1.
pub struct Face<'a> {
    pub control: Control,
    pub name: &'a str,
    pub value: &'a str,
    pub norm: f64,
    /// A switch's state.
    pub on: bool,
    pub cursor: bool,
    /// The dial has it (a press, then the dial), or a finger.
    pub editing: bool,
}

/// A tile of a row as the widgets take it (`GridTile`), placed at `rect`.
pub fn row_tile(rect: Rect, face: &Face) -> crate::GridTile {
    crate::GridTile {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        kind: face.control as i32,
        name: face.name.into(),
        value: face.value.into(),
        norm: face.norm as f32,
        high: 0.,
        bipolar: false,
        arc: arc(0., face.norm).into(),
        choices: Default::default(),
        icons: Default::default(),
        icon: 0,
        chosen: 0,
        on: face.on,
        cursor: face.cursor,
        editing: face.editing,
        part: 0,
        linked: false,
        compact: false,
    }
}

/// The tile the cursor goes to from `at` by `dx` columns or `dy` rows: in the same row the next
/// tile across; a row up or down, the tile there nearest the cursor's middle. None at an edge.
pub fn arrow(tiles: &[Tile], at: usize, dx: i32, dy: i32) -> Option<usize> {
    let from = tiles.get(at)?.rect;
    let mid = from.x + from.w / 2.;
    if dx != 0 {
        return tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| (t.rect.y - from.y).abs() < 0.5 && (t.rect.x - from.x) * dx as f32 > 0.5)
            .min_by(|a, b| (a.1.rect.x - from.x).abs().total_cmp(&(b.1.rect.x - from.x).abs()))
            .map(|(i, _)| i);
    }
    if dy == 0 {
        return Some(at);
    }
    let row = tiles.iter().map(|t| t.rect.y).filter(|&y| (y - from.y) * dy as f32 > 0.5).min_by(|a, b| (a - from.y).abs().total_cmp(&(b - from.y).abs()))?;
    tiles
        .iter()
        .enumerate()
        .filter(|(_, t)| (t.rect.y - row).abs() < 0.5)
        .min_by(|a, b| (a.1.rect.x + a.1.rect.w / 2. - mid).abs().total_cmp(&(b.1.rect.x + b.1.rect.w / 2. - mid).abs()))
        .map(|(i, _)| i)
}

/// A knob's arc as path commands in a 100 x 100 box: from 135 degrees left of the top to `to`
/// (0 to 1 across 270 degrees), from `from` (the bipolar middle, else 0).
pub fn arc(from: f64, to: f64) -> String {
    let (a, b) = (from.min(to), from.max(to));
    if b - a < 1e-4 {
        return String::new();
    }
    let angle = |n: f64| (-225. + 270. * n).to_radians();
    let at = |n: f64| (50. + 40. * angle(n).cos(), 50. + 40. * angle(n).sin());
    let (x0, y0) = at(a);
    let (x1, y1) = at(b);
    let large = if (b - a) * 270. > 180. { 1 } else { 0 };
    format!("M {x0:.2} {y0:.2} A 40 40 0 {large} 1 {x1:.2} {y1:.2}")
}

// The envelope (ParameterGrid.cpp envelopeOf, layout).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Point {
    /// Places in the envelope's parameter list.
    pub time: Option<usize>,
    pub level: Option<usize>,
    /// The level where no parameter gives one.
    pub fixed: f64,
    pub sustain: bool,
    pub rate: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Envelope {
    pub start: Option<usize>,
    pub points: Vec<Point>,
}

pub fn is_envelope(field: &str) -> bool {
    matches!(field, "delay" | "attack" | "hold" | "decay" | "sustain" | "release") || ["level", "time", "rate"].iter().any(|k| field.strip_prefix(k).is_some_and(|n| n.parse::<u8>().is_ok()))
}

fn named(attack: Option<usize>, decay: Option<usize>, sustain: Option<usize>, release: Option<usize>, delay: Option<usize>, hold: Option<usize>) -> Envelope {
    let mut e = Envelope::default();
    let (Some(_), Some(_)) = (attack, release) else { return e };
    if delay.is_some() {
        e.points.push(Point { time: delay, ..Default::default() });
    }
    e.points.push(Point { time: attack, level: if decay.is_none() { sustain } else { None }, fixed: 1., ..Default::default() });
    if hold.is_some() {
        let level = e.points.last().and_then(|p| p.level);
        e.points.push(Point { time: hold, level, fixed: 1., ..Default::default() });
    }
    if decay.is_some() {
        e.points.push(Point { time: decay, level: sustain, fixed: if sustain.is_some() { 0. } else { 1. }, ..Default::default() });
    }
    if let Some(p) = e.points.last_mut() {
        p.sustain = true;
    }
    e.points.push(Point { time: release, ..Default::default() });
    e
}

/// The envelope the fields describe (`params.list`'s `field`): named stages as an ADSR, or points.
pub fn envelope_of_fields(fields: &[&str]) -> Envelope {
    let find = |f: &str| fields.iter().position(|x| *x == f);
    let e = named(find("attack"), find("decay"), find("sustain"), find("release"), find("delay"), find("hold"));
    if !e.points.is_empty() {
        return e;
    }
    let segments = (1..=8).filter(|k| find(&format!("time{k}")).is_some() || find(&format!("rate{k}")).is_some()).max().unwrap_or(0);
    let mut e = Envelope { start: find("level0"), points: vec![] };
    for k in 1..=segments {
        let (time, rate) = (find(&format!("time{k}")), find(&format!("rate{k}")));
        e.points.push(Point { time: time.or(rate), level: find(&format!("level{k}")), rate: time.is_none() && rate.is_some(), ..Default::default() });
    }
    if e.points.is_empty() {
        return e;
    }
    if e.start.is_none() {
        e.start = e.points.last().and_then(|p| p.level);
    }
    for k in 0..e.points.len() - 1 {
        if e.points[k].level.is_none() {
            e.points[k].level = if k > 0 { e.points[k - 1].level } else { e.start };
            e.points[k].fixed = if k > 0 { e.points[k - 1].fixed } else { 0. };
        }
    }
    let n = e.points.len();
    if n > 1 {
        e.points[n - 2].sustain = true;
    }
    e
}

/// An envelope recognized by its parameters' names within one module (ParameterGrid.cpp `envelopeOf`, any
/// case): a DX7's four rates and four levels (it starts from level 4 and holds at level 3); an FM-X's time and
/// level for each stage; or an ADSR (Attack and Release with a Decay or a Sustain, optionally Delay and Hold).
pub fn envelope_of_names(names: &[&str]) -> Envelope {
    let find = |n: &str| names.iter().position(|x| x.eq_ignore_ascii_case(n));
    let either = |options: &[&str]| options.iter().find_map(|o| find(o));
    let mut e = Envelope::default();
    let (r, l) = ([find("Rate 1"), find("Rate 2"), find("Rate 3"), find("Rate 4")], [find("Level 1"), find("Level 2"), find("Level 3"), find("Level 4")]);
    if r.iter().chain(&l).all(Option::is_some) {
        e.start = l[3];
        e.points = (0..4).map(|k| Point { time: r[k], level: l[k], fixed: 0., sustain: k == 2, rate: true }).collect();
        return e;
    }
    if let (Some(attack_time), Some(release_time)) = (find("Attack Time"), find("Release Time")) {
        e.start = either(&["Release (Hold) Level", "Hold Level", "Initial Level"]);
        if let Some(hold) = find("Hold Time") {
            e.points.push(Point { time: Some(hold), level: e.start, ..Default::default() });
        }
        e.points.push(Point { time: Some(attack_time), level: find("Attack Level"), fixed: 1., ..Default::default() });
        for (t, lv) in [("Decay 1 Time", "Decay 1 Level"), ("Decay 2 Time", "Decay 2 Level"), ("Decay Time", "Decay Level")] {
            if let Some(time) = find(t) {
                let fixed = e.points.last().map_or(0., |p| p.fixed);
                e.points.push(Point { time: Some(time), level: find(lv), fixed, ..Default::default() });
            }
        }
        if let Some(p) = e.points.last_mut() {
            p.sustain = true;
        }
        let level = either(&["Release Level", "Release (Hold) Level"]).or(e.start);
        e.points.push(Point { time: Some(release_time), level, ..Default::default() });
        return e;
    }
    let (attack, decay, sustain, release) = (find("Attack"), find("Decay"), find("Sustain"), find("Release"));
    if attack.is_some() && release.is_some() && (decay.is_some() || sustain.is_some()) {
        if let Some(delay) = find("Delay") {
            e.points.push(Point { time: Some(delay), ..Default::default() });
        }
        e.points.push(Point { time: attack, fixed: 1., ..Default::default() });
        if let Some(hold) = find("Hold") {
            e.points.push(Point { time: Some(hold), fixed: 1., ..Default::default() });
        }
        if decay.is_some() {
            e.points.push(Point { time: decay, level: sustain, fixed: if sustain.is_some() { 0. } else { 1. }, ..Default::default() });
        }
        if decay.is_none() && sustain.is_some() {
            if let Some(p) = e.points.last_mut() {
                p.level = sustain;
            }
        }
        if let Some(p) = e.points.last_mut() {
            p.sustain = true;
        }
        e.points.push(Point { time: release, ..Default::default() });
    }
    e
}

/// The envelope's parameters in the order its tiles are linked: each point's time then level.
pub fn linked_order(e: &Envelope) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::new();
    let mut add = |i: Option<usize>| {
        if let Some(i) = i.filter(|i| !order.contains(i)) {
            order.push(i);
        }
    };
    let rates = e.points.first().is_some_and(|p| p.rate);
    if rates {
        e.points.iter().for_each(|p| add(p.time));
        e.points.iter().for_each(|p| add(p.level));
        add(e.start); // level 4 last, under rate 4
    } else {
        add(e.start);
        for p in &e.points {
            add(p.time);
            add(p.level);
        }
    }
    order
}

/// An envelope time's seconds at `n` (0 to 1) between the plugin's evenly spaced values: geometric
/// where both are above zero.
fn seconds_at(s: &[f64], n: f64) -> f64 {
    let at = n.clamp(0., 1.) * (s.len() - 1) as f64;
    let k = (at as usize).min(s.len() - 2);
    let (f, a, b) = (at - k as f64, s[k], s[k + 1]);
    if a > 0. && b > 0. {
        a * (b / a).powf(f)
    } else {
        a + (b - a) * f
    }
}

/// Where an envelope's plot starts in its panel: under the title (6 from the top, a line of the small type at
/// `scale`) with room for a node's radius above the highest level and 4 more; the paddings times `spacing`.
pub fn envelope_top(scale: f32, spacing: f32) -> f32 {
    6. * spacing + 13. * scale * 1.3 + NODE_RADIUS + 4. * spacing
}

/// An envelope node's radius (the widget draws 12 across).
pub const NODE_RADIUS: f32 = 6.;

/// The widget's icon number for the icon a plugin declares for a value (draft 6's `enum oscairn_icon`, ui.h):
/// 1 saw, 2 square, 3 sine, 4 triangle, 5 noise, 6 pulse, 7 saw down, 8 sample and hold; 0 for none or an
/// icon this host does not know.
pub fn declared_icon(icon: u32) -> i32 {
    match icon {
        1 => 3,
        2 => 4,
        3 => 1,
        4 => 7,
        5 => 2,
        6 => 6,
        7 => 5,
        8 => 8,
        _ => 0,
    }
}

/// A value's icon: the one the plugin declares for it where it declares one, else the name rule (`wave_icon`).
pub fn icon(declared: Option<u32>, name: &str) -> i32 {
    match declared.map(declared_icon) {
        Some(i) if i > 0 => i,
        _ => wave_icon(name),
    }
}

/// The icons of a stepped parameter's values, `labels` being their texts from the minimum. Where the plugin
/// declares an icon for any value, the host guesses none for the others (ui.h `value_icon`); else every value
/// has its name's.
pub fn icons_for(p: &Param, labels: &[String]) -> Vec<i32> {
    let declared = |k: usize| p.icons.get(k).copied().filter(|&i| declared_icon(i) > 0);
    if (0..labels.len()).any(|k| declared(k).is_some()) {
        (0..labels.len()).map(|k| icon(declared(k), "")).collect()
    } else {
        labels.iter().map(|l| icon(None, l)).collect()
    }
}

/// A waveform icon for a value's label, by the well-known names (any case): 1 saw, 2 square, 3 sine,
/// 4 triangle, 5 noise, 6 pulse; 0 for any other name. The fallback where a plugin declares no icon.
pub fn wave_icon(name: &str) -> i32 {
    match name.trim().to_lowercase().as_str() {
        "saw" | "sawtooth" => 1,
        "square" | "sqr" | "sq" => 2,
        "sine" | "sin" => 3,
        "triangle" | "tri" => 4,
        "noise" => 5,
        "pulse" => 6,
        _ => 0,
    }
}

/// The curve in a panel of `w` x `h` (its own pixels), its plot from `top` down: the line's path
/// commands, the filled area's (the same points, closed down to the baseline under the first and last),
/// and each point's place (the node's center, the same point). The widget draws all three in the panel's
/// own pixels. `params`: the envelope's parameters; `norm`: a parameter's value from 0 to 1.
#[cfg(test)]
pub fn envelope_curve(e: &Envelope, params: &[&Param], norm: &dyn Fn(usize) -> f64, w: f32, h: f32, top: f32) -> (String, String, Vec<(f32, f32)>) {
    let l = envelope_layout(e, params, norm, w, h, top);
    let points = l.handles.iter().map(|h| h.at).collect();
    (l.line, l.fill, points)
}

/// The longest any of the time or rate parameters' stages can take, in seconds; 0 when one gives no seconds
/// (the stages are then drawn by the parameters' ranges).
pub fn longest_stage(params: &[&Param]) -> f64 {
    if params.iter().any(|p| p.seconds.len() < 2) {
        return 0.;
    }
    params.iter().flat_map(|p| p.seconds.iter().copied()).fold(0., f64::max)
}

/// How far across its room (0 to 1) the stage of a time or rate parameter at `v` (0 to 1) reaches: by its
/// seconds on the log axis up to `longest`, or by its range when that is 0 (a rate reversed).
pub fn stage_across(p: &Param, v: f64, rate: bool, longest: f64) -> f64 {
    if longest > 0. {
        time_across(seconds_at(&p.seconds, v), longest)
    } else if rate {
        1. - v
    } else {
        v
    }
}

/// An envelope's seconds-axis position (0 to 1) of a time of `seconds` when the longest stage is `longest`.
pub fn stage_axis(seconds: f64, longest: f64) -> f64 {
    time_across(seconds, longest)
}

fn time_across(seconds: f64, longest: f64) -> f64 {
    (seconds / SHORT_STAGE).ln_1p() / (longest / SHORT_STAGE).ln_1p()
}

/// About even below 50 ms and logarithmic past it, so a 30 ms release, a 250 ms decay and a 5 s pad all read.
const SHORT_STAGE: f64 = 0.05;

/// The normal value (0 to 1) of a time field whose stage is `seconds` long, inside the first interval of the
/// plugin's evenly spaced `s` that holds it (times rise with the value, rates fall), else at the nearer end.
fn normal_at(s: &[f64], seconds: f64) -> f64 {
    for k in 0..s.len().saturating_sub(1) {
        let (a, b) = (s[k], s[k + 1]);
        if (seconds - a) * (seconds - b) > 0. {
            continue;
        }
        let f = if a == b {
            0.
        } else if a > 0. && b > 0. && seconds > 0. {
            (seconds / a).ln() / (b / a).ln()
        } else {
            (seconds - a) / (b - a)
        };
        return (k as f64 + f) / (s.len() - 1) as f64;
    }
    match (s.first(), s.last()) {
        (Some(a), Some(b)) if (seconds - a).abs() <= (seconds - b).abs() => 0.,
        _ => 1.,
    }
}

/// What dragging a stage's node to `across` (0 to 1 of its segment) sets its time or rate parameter to, as a
/// value from 0 to 1 (`Param::denorm` makes the value): the inverse of how the stage is drawn.
pub fn stage_normal(p: &Param, rate: bool, longest: f64, across: f64) -> f64 {
    let across = across.clamp(0., 1.);
    if longest > 0. && p.seconds.len() >= 2 {
        normal_at(&p.seconds, SHORT_STAGE * (across * (longest / SHORT_STAGE).ln_1p()).exp_m1())
    } else if rate {
        1. - across
    } else {
        across
    }
}

/// An envelope laid out in a panel: its plot's edges, the node of each point with where its segment starts, the
/// segment's width and least width, and the paths drawn from those points.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvLayout {
    pub bottom: f32,
    pub height: f32,
    pub segment: f32,
    pub least: f32,
    pub longest: f64,
    pub handles: Vec<EnvHandle>,
    pub line: String,
    pub fill: String,
    /// The line's vertices.
    pub points: Vec<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvHandle {
    pub at: (f32, f32),
    /// Where its segment starts across.
    pub from: f32,
}

pub fn envelope_layout(e: &Envelope, params: &[&Param], norm: &dyn Fn(usize) -> f64, w: f32, h: f32, top: f32) -> EnvLayout {
    envelope_layout_in(e, params, norm, (14., top, w - 14., h - 10.), 1., 0.)
}

/// An envelope laid out in the area (left, top, right, bottom): levels are scaled by `gain` (0 to 1: an
/// operator's output level in a shared plot) and stage times by `longest` seconds when it is more than the
/// envelope's own longest stage (so several envelopes share one time scale), else by its own.
pub fn envelope_layout_in(e: &Envelope, params: &[&Param], norm: &dyn Fn(usize) -> f64, area: (f32, f32, f32, f32), gain: f32, longest_of_all: f64) -> EnvLayout {
    let (left, top, right, bottom) = area;
    let (aw, ah) = (right - left, (bottom - top) * gain);
    let hold = aw * 0.14;
    let segment = (aw - hold) / e.points.len().max(1) as f32;
    let least = (20f32).min(segment / 4.);
    let times: Vec<&Param> = e.points.iter().filter_map(|p| p.time).map(|t| params[t]).collect();
    let own = longest_stage(&times);
    let longest = if own > 0. { own.max(longest_of_all) } else { 0. };
    let across = |t: usize, rate: bool| stage_across(params[t], norm(t), rate, longest);
    let level = |p: Option<usize>, fixed: f64| bottom - norm_or(p, fixed, norm) as f32 * ah;
    let mut x = left;
    let start = level(e.start, 0.);
    let mut points = vec![(left, start)];
    let mut handles = Vec::new();
    for p in &e.points {
        let from = x;
        if let Some(t) = p.time {
            x += least + across(t, p.rate) as f32 * (segment - least);
        }
        let y = level(p.level, p.fixed);
        points.push((x, y));
        handles.push(EnvHandle { at: (x, y), from });
        if p.sustain {
            x += hold;
            points.push((x, y));
        }
    }
    let line = points.iter().enumerate().map(|(k, (x, y))| format!("{} {x:.1} {y:.1}", if k == 0 { "M" } else { " L" })).collect::<String>();
    let fill = format!("{line} L {x:.1} {bottom:.1} L {left:.1} {bottom:.1} Z");
    EnvLayout { bottom, height: ah, segment, least, longest, handles, line, fill, points }
}

fn norm_or(p: Option<usize>, fixed: f64, norm: &dyn Fn(usize) -> f64) -> f64 {
    p.map_or(fixed, norm)
}

/// The slots of an envelope's stages in a plot from `left` to `right`: stage k's slot, `envelope_layout_slots` and the
/// controls under the plot go by it.
pub fn stage_slots(e: &Envelope, left: f32, right: f32) -> Vec<(f32, f32)> {
    let aw = right - left;
    let hold = if e.points.iter().any(|p| p.sustain) { aw * 0.14 } else { 0. };
    let segment = (aw - hold) / e.points.len().max(1) as f32;
    let mut slot = left;
    e.points
        .iter()
        .map(|p| {
            let at = (slot, slot + segment);
            slot += segment + if p.sustain { hold } else { 0. };
            at
        })
        .collect()
}

/// An envelope laid out in slots, as the master's detail draws it: stage k has the slot k of the plot, every slot as
/// wide, and its point lies inside it at its stage's place on the slot's own scale (a time by its seconds on the log
/// axis up to `longest_of_all`, else by the parameter's range; a rate reversed). The time axis is therefore one slot
/// wide and read stage by stage, and a control placed under the slot is under its point whatever the values are. The
/// hold after a sustain is a gap between two slots, as in `envelope_layout_in`. Levels are as given (no gain).
pub fn envelope_layout_slots(e: &Envelope, params: &[&Param], norm: &dyn Fn(usize) -> f64, area: (f32, f32, f32, f32), longest_of_all: f64) -> EnvLayout {
    let (left, top, right, bottom) = area;
    let (aw, ah) = (right - left, bottom - top);
    let sustains = e.points.iter().any(|p| p.sustain);
    let hold = if sustains { aw * 0.14 } else { 0. };
    let segment = (aw - hold) / e.points.len().max(1) as f32;
    let least = (20f32).min(segment / 4.);
    let times: Vec<&Param> = e.points.iter().filter_map(|p| p.time).map(|t| params[t]).collect();
    let own = longest_stage(&times);
    let longest = if own > 0. { own.max(longest_of_all) } else { 0. };
    let level = |p: Option<usize>, fixed: f64| bottom - norm_or(p, fixed, norm) as f32 * ah;
    let mut slot = left;
    let mut points = vec![(left, level(e.start, 0.))];
    let mut handles = Vec::new();
    for p in &e.points {
        let from = slot;
        let x = match p.time {
            Some(t) => from + least + stage_across(params[t], norm(t), p.rate, longest) as f32 * (segment - least),
            None => from,
        };
        let y = level(p.level, p.fixed);
        points.push((x, y));
        handles.push(EnvHandle { at: (x, y), from });
        slot += segment;
        if p.sustain {
            slot += hold;
            points.push((slot, y));
        }
    }
    let line = points.iter().enumerate().map(|(k, (x, y))| format!("{} {x:.1} {y:.1}", if k == 0 { "M" } else { " L" })).collect::<String>();
    let end = points.last().map_or(left, |p| p.0);
    let fill = format!("{line} L {end:.1} {bottom:.1} L {left:.1} {bottom:.1} Z");
    EnvLayout { bottom, height: ah, segment, least, longest, handles, line, fill, points }
}

// ---- A master's detail: compact controls, and an envelope's stages by name ----

/// The touch target's least height (`Theme.touch`, RFC 0019).
pub const TOUCH: f32 = 44.;

/// A compact control's height, defined once (slint/theme.slint `Theme.compact-height` is this formula and a test holds
/// the two equal): the touch target, or what its name line, a control 0.45 of a touch target high and the tight paddings
/// need at the theme's type scale and spacing, whichever is more. Never less than the touch target.
pub fn compact_height(scale: f32, spacing: f32) -> f32 {
    (2. * 4. * spacing + 1.15 * 13. * scale + 0.45 * TOUCH).max(TOUCH)
}

/// A compact control's least width (`Theme.compact-width`): 1.9 touch targets, which puts eight across the narrowest of the
/// keyboard's screens with both rails (743 logical px).
pub const COMPACT_WIDTH: f32 = 1.9 * TOUCH;

/// How many compact controls a row of `width` holds (a row of up to eight: the eight knobs under the screen), two at least.
pub fn compact_columns(width: f32, gap: f32) -> usize {
    (((width + gap) / (COMPACT_WIDTH + gap)) as usize).clamp(2, 8)
}

/// One stage of an envelope as a master's detail names it, and the controls that move its point.
#[derive(Clone, Debug, PartialEq)]
pub struct StageCells {
    pub name: String,
    /// The time or rate parameter (a place in the envelope's list), and whether it is a rate.
    pub time: Option<(usize, bool)>,
    /// The level the stage reaches, when the stage has a parameter of its own for it, and its name.
    pub level: Option<(usize, String)>,
}

/// An envelope's stages by name, one per point, and the start level when it has a parameter of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stages {
    pub cells: Vec<StageCells>,
    pub start: Option<usize>,
}

/// Names the points of an envelope from its fields: the named stages by their fields (Delay, Attack, Hold, Decay,
/// Release; the sustain's level is "Sustain"), the numbered ones by their place: a segment with a time or rate and no
/// level of its own is a Hold, the others in turn the Attack, Decay (Decay 1, Decay 2... where there are more than one)
/// and, the last, the Release. A level is shown once, under the stage whose own field it is (numbered `levelK` under
/// segment K), not again where the envelope reuses it (the start level of a hold).
pub fn stages(e: &Envelope, fields: &[&str]) -> Stages {
    let field = |i: Option<usize>| i.and_then(|i| fields.get(i).copied()).unwrap_or("");
    let numbered = e.points.iter().any(|p| matches!(field(p.time).strip_prefix("time").or_else(|| field(p.time).strip_prefix("rate")), Some(n) if n.parse::<u8>().is_ok()));
    let own_level = |k: usize, p: &Point| p.level.filter(|&l| if numbered { field(Some(l)) == format!("level{}", k + 1) } else { field(Some(l)) == "sustain" });
    let moving: Vec<usize> = (0..e.points.len()).filter(|&k| numbered && own_level(k, &e.points[k]).is_some()).collect();
    let mut cells = Vec::new();
    for (k, p) in e.points.iter().enumerate() {
        let name = if numbered {
            match moving.iter().position(|&m| m == k) {
                None => "Hold".to_string(),
                Some(j) => match (j, moving.len()) {
                    (_, 1) => "Stage".to_string(),
                    (0, _) => "Attack".to_string(),
                    (j, m) if j + 1 == m => "Release".to_string(),
                    (_, 3) => "Decay".to_string(),
                    (j, _) => format!("Decay {j}"),
                },
            }
        } else {
            match field(p.time) {
                "delay" => "Delay",
                "attack" => "Attack",
                "hold" => "Hold",
                "decay" => "Decay",
                "release" => "Release",
                _ => "Stage",
            }
            .to_string()
        };
        let level = own_level(k, p).map(|l| (l, if !numbered { "Sustain".to_string() } else { name.clone() }));
        cells.push(StageCells { name, time: p.time.map(|t| (t, p.rate)), level });
    }
    Stages { cells, start: e.start.filter(|&s| field(Some(s)) == "level0") }
}

/// A stage's length in seconds at `norm` (0 to 1 of its range), when the plugin gives them: its `seconds` at evenly
/// spaced values, as the graph reads them.
pub fn stage_seconds(p: &Param, norm: f64) -> Option<f64> {
    (p.seconds.len() >= 2).then(|| seconds_at(&p.seconds, norm))
}

/// A length of time as a control writes it: "12 ms", "9.5 ms", "1.3 s", "12 s".
pub fn seconds_text(s: f64) -> String {
    let ms = s * 1000.;
    if ms < 9.95 {
        format!("{ms:.1} ms")
    } else if s < 1. {
        format!("{} ms", ms.round())
    } else if s < 9.95 {
        format!("{s:.1} s")
    } else {
        format!("{} s", s.round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: u32, name: &str, widget: &str, max: f64, stepped: bool) -> Param {
        Param { id, name: name.into(), widget: widget.into(), max, stepped, ..Default::default() }
    }

    #[test]
    fn controls_follow_the_hint_where_it_fits_else_the_values() {
        let all: Vec<Param> = vec![];
        let none: Vec<String> = vec![];
        assert_eq!(control(&p(0, "A", "knob", 1., false), &none, &all), Control::Knob);
        assert_eq!(control(&p(0, "A", "", 1., false), &none, &all), Control::Slider);
        assert_eq!(control(&p(0, "A", "", 2., true), &none, &all), Control::Buttons, "three values: buttons");
        let off_on = vec!["Off".to_string(), "On".to_string()];
        assert_eq!(control(&p(0, "A", "", 1., true), &off_on, &all), Control::Switch);
        assert_eq!(control(&p(0, "A", "", 1., true), &["Mono".into(), "Poly".into()], &all), Control::Buttons);
        assert_eq!(control(&p(0, "A", "", 1., true), &none, &all), Control::Switch, "a switch until the texts say otherwise");
        assert_eq!(control(&p(0, "A", "", 20., true), &none, &all), Control::List);
        assert_eq!(control(&p(0, "A", "", 99., true), &none, &all), Control::Slider);
        assert_eq!(control(&p(0, "A", "buttons", 9., true), &none, &all), Control::List, "ten values: too many for buttons");
        let mut r = p(0, "Low", "range", 1., false);
        assert_eq!(control(&r, &none, &all), Control::Slider, "a range without its high end is a slider");
        r.range_high = Some(1);
        assert_eq!(control(&r, &none, &[r.clone(), p(1, "High", "", 1., false)]), Control::Range);
        let mut ro = p(0, "A", "knob", 1., false);
        ro.flags = FLAG_READONLY;
        assert_eq!(control(&ro, &none, &all), Control::Value);
    }

    fn simple(params: &[Param], width: f32) -> Laid {
        let all: Vec<usize> = (0..params.len()).collect();
        layout(params, &[(String::new(), all)], width, SECTION_GAP, &|i| control(&params[i], &[], params))
    }

    #[test]
    fn tiles_fill_rows_and_a_group_that_does_not_fit_starts_the_next() {
        // 840 wide: four columns of 205.5.
        let mut params: Vec<Param> = (0..7).map(|i| p(i, &format!("P{i}"), "", 1., false)).collect();
        for q in &mut params[3..6] {
            q.group = 1;
        }
        let laid = simple(&params, 840.);
        assert_eq!(columns(840.), 4);
        let r: Vec<Rect> = laid.tiles.iter().map(|t| t.rect).collect();
        assert_eq!((r[0].x, r[1].x, r[2].x), (0., 211.5, 423.));
        assert_eq!(r[2].w, 840. - 423., "the tile before the group widens to the row's end");
        assert_eq!((r[3].x, r[3].y), (0., TILE + SECTION_GAP), "the group starts the next row");
        assert_eq!(r[3].w, 205.5 + SECTION_GAP, "its tiles touch");
        assert_eq!(laid.cards.len(), 1);
        assert!(laid.tiles[3].linked && laid.tiles[5].linked && !laid.tiles[6].linked && !laid.tiles[2].linked);
        assert_eq!(r[6].x, 3. * 211.5, "the next tile follows the group in its row");
    }

    #[test]
    fn a_group_wider_than_the_row_goes_on_in_touching_rows_and_knobs_make_tiles_taller() {
        let mut params: Vec<Param> = (0..5).map(|i| p(i, "P", "knob", 1., false)).collect();
        for q in &mut params {
            q.group = 2;
        }
        let laid = simple(&params, 500.); // two columns
        let r: Vec<Rect> = laid.tiles.iter().map(|t| t.rect).collect();
        let h = TILE + KNOB_EXTRA;
        assert_eq!((r[2].y, r[4].y), (h, 2. * h), "rows touch");
        assert_eq!((r[4].x, r[4].w), (0., 500.), "the last fills its row");
        assert_eq!(laid.cards.len(), 1);
    }

    #[test]
    fn headings_views_first_and_the_range_high_end_has_no_tile() {
        let mut params = vec![p(0, "Attack", "", 1., false), p(1, "Decay", "", 1., false), p(2, "Sustain", "", 1., false), p(3, "Release", "", 1., false), p(4, "Low", "range", 1., false), p(5, "High", "", 1., false)];
        params[4].range_high = Some(5);
        let groups = vec![("Amp/Env".to_string(), vec![0, 1, 2, 3]), ("Keys".to_string(), vec![4, 5])];
        let laid = layout(&params, &groups, 900., SECTION_GAP, &|i| control(&params[i], &[], &params));
        assert_eq!(laid.headings[0], (0., "Amp › Env".to_string()));
        assert_eq!(laid.panels.len(), 1, "an ADSR by name");
        assert_eq!(laid.panels[0].rect.y, HEADING);
        assert_eq!(laid.tiles[0].rect.y, HEADING + ENVELOPE + SECTION_GAP, "the tiles under the view");
        assert!(laid.tiles[..4].iter().all(|t| t.linked), "its stages are one run");
        assert_eq!(laid.tiles.iter().map(|t| t.param).collect::<Vec<_>>(), [0, 1, 2, 3, 4], "no tile for High");
        assert_eq!(laid.tiles[4].control, Control::Range);
    }

    #[test]
    fn the_cursor_moves_across_rows_and_columns() {
        let params: Vec<Param> = (0..7).map(|i| p(i, "P", "", 1., false)).collect();
        let laid = simple(&params, 840.); // 4 + 3
        let t = &laid.tiles;
        assert_eq!(arrow(t, 0, 1, 0), Some(1));
        assert_eq!(arrow(t, 3, 1, 0), None, "the row's end");
        assert_eq!(arrow(t, 1, 0, 1), Some(5));
        assert_eq!(arrow(t, 3, 0, 1), Some(6), "the nearest below");
        assert_eq!(arrow(t, 6, 0, -1), Some(2));
        assert_eq!(arrow(t, 6, 0, 1), None);
    }

    #[test]
    fn envelopes_from_fields_and_names() {
        let adsr = envelope_of_fields(&["attack", "decay", "sustain", "release"]);
        assert_eq!(adsr.points.len(), 3);
        assert!(adsr.points[1].sustain && adsr.points[1].level == Some(2));
        assert_eq!(linked_order(&adsr), [0, 1, 2, 3]);
        // DX7: rates 1-4 and levels 1-4, starting at level 4.
        let dx = envelope_of_fields(&["rate1", "rate2", "rate3", "rate4", "level1", "level2", "level3", "level4"]);
        assert_eq!(dx.start, Some(7));
        assert!(dx.points[0].rate && dx.points[2].sustain);
        assert_eq!(linked_order(&dx), [0, 1, 2, 3, 4, 5, 6, 7]);
        assert!(envelope_of_names(&["Attack", "Cutoff"]).points.is_empty(), "no release: no envelope");
        let named = envelope_of_names(&["attack", "Decay", "Sustain", "Release"]);
        let params: Vec<Param> = (0..4).map(|i| p(i, "P", "", 1., false)).collect();
        let refs: Vec<&Param> = params.iter().collect();
        let (line, fill, points) = envelope_curve(&named, &refs, &|i| [0., 0.5, 0.5, 1.][i], 400., 180., envelope_top(1., 1.));
        assert!(line.starts_with("M 14.0 170.0") && fill.ends_with('Z'), "{line}");
        assert_eq!(points.len(), 3);
        assert!((points[0].1 - envelope_top(1., 1.)).abs() < 0.01, "the attack reaches the top");
        assert!(points[1].1 > 28. && points[1].1 < 170., "the sustain level halfway");
        assert_eq!(arc(0., 0.), "");
        assert!(arc(0., 1.).contains(" 1 1 "), "past 180 degrees: the large arc");
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    // The envelopes a plugin without views is recognized to have, by its parameters' names.
    #[test]
    fn envelopes_by_name_as_the_reference_recognizes_them() {
        // A DX7's: rates and levels, from level 4, holding at level 3, every segment a rate.
        let dx = envelope_of_names(&["Level 4", "Rate 1", "Rate 2", "Rate 3", "Rate 4", "Level 1", "Level 2", "Level 3"]);
        assert_eq!(dx.start, Some(0));
        assert_eq!(dx.points.iter().map(|p| (p.time, p.level, p.sustain, p.rate)).collect::<Vec<_>>(), [(Some(1), Some(5), false, true), (Some(2), Some(6), false, true), (Some(3), Some(7), true, true), (Some(4), Some(0), false, true)]);
        assert!(envelope_of_names(&["Rate 1", "Rate 2", "Rate 3", "Rate 4", "Level 1", "Level 2", "Level 3"]).points.is_empty(), "without level 4 it is nothing");
        // An FM-X's: a time and a level a stage, from a hold level, the release back to it.
        let fm = envelope_of_names(&["Hold Level", "Attack Time", "Attack Level", "Decay 1 Time", "Decay 1 Level", "Decay 2 Time", "Decay 2 Level", "Release Time", "Release Level"]);
        assert_eq!(fm.start, Some(0));
        assert_eq!(fm.points.iter().map(|p| (p.time, p.level, p.fixed, p.sustain)).collect::<Vec<_>>(), [(Some(1), Some(2), 1., false), (Some(3), Some(4), 1., false), (Some(5), Some(6), 1., true), (Some(7), Some(8), 0., false)]);
        let held = envelope_of_names(&["Initial Level", "Hold Time", "Attack Time", "Release Time"]);
        assert_eq!(held.points.iter().map(|p| (p.time, p.level)).collect::<Vec<_>>(), [(Some(1), Some(0)), (Some(2), None), (Some(3), Some(0))], "the release without a level returns to the first");
        // An ADSR needs a Decay or a Sustain beside Attack and Release; Delay and Hold are optional.
        assert!(envelope_of_names(&["Attack", "Release"]).points.is_empty());
        let a = envelope_of_names(&["Delay", "Attack", "Hold", "Sustain", "Release"]);
        assert_eq!(a.points.iter().map(|p| (p.time, p.level, p.fixed, p.sustain)).collect::<Vec<_>>(), [(Some(0), None, 0., false), (Some(1), None, 1., false), (Some(2), Some(3), 1., true), (Some(4), None, 0., false)], "no decay: the sustain level is the last stage's");
        let adsr = envelope_of_names(&["attack", "DECAY", "release"]);
        assert_eq!(adsr.points.iter().map(|p| (p.level, p.fixed, p.sustain)).collect::<Vec<_>>(), [(None, 1., false), (None, 1., true), (None, 0., false)], "no sustain level: the decay stays at full");
    }
}

#[cfg(test)]
mod curve_and_icon_tests {
    use super::*;

    fn vertices(path: &str) -> Vec<(f32, f32)> {
        let t: Vec<&str> = path.split_whitespace().filter(|t| !matches!(*t, "M" | "L" | "Z")).collect();
        t.chunks(2).map(|c| (c[0].parse().unwrap(), c[1].parse().unwrap())).collect()
    }

    // One set of points drives the stroke, the fill and the nodes: the fill is the line closed down to the
    // baseline under its first and last points, every node is a vertex of the line, and the plot stays
    // under the title's room.
    #[test]
    fn the_envelope_line_fill_and_nodes_share_their_points() {
        let params: Vec<Param> = ["Attack", "Decay", "Sustain", "Release"].iter().enumerate().map(|(i, n)| Param { id: i as u32, name: (*n).into(), max: 1., ..Default::default() }).collect();
        let refs: Vec<&Param> = params.iter().collect();
        let shape = envelope_of_fields(&["attack", "decay", "sustain", "release"]);
        for (w, scale) in [(240., 1.), (900., 1.), (300., 1.4)] {
            let top = envelope_top(scale, 1.);
            let (line, fill, nodes) = envelope_curve(&shape, &refs, &|_| 0.5, w, 180., top);
            assert!(fill.starts_with(&line), "the fill begins with the line");
            let (l, f) = (vertices(&line), vertices(&fill));
            assert_eq!(&f[..l.len()], &l[..]);
            assert_eq!(f.len(), l.len() + 2, "and closes with two points");
            let (first, last) = (l[0], *l.last().unwrap());
            assert_eq!((f[l.len()], f[l.len() + 1]), ((last.0, 170.), (first.0, 170.)), "down to the baseline under the last and the first point");
            assert_eq!(first.1, 170., "the line starts on the baseline");
            for n in &nodes {
                assert!(l.iter().any(|v| (v.0 - n.0).abs() < 0.06 && (v.1 - n.1).abs() < 0.06), "node {n:?} is a vertex of the line");
                assert!(n.1 + 0.01 >= top, "a node stays below the title's room");
            }
            assert!(l.iter().all(|v| v.1 >= top), "the plot starts at {top}");
        }
    }

    #[test]
    fn waveform_names_map_to_icons() {
        let all = [("Saw", 1), ("sawtooth", 1), ("SQUARE", 2), ("Sqr", 2), ("Sine", 3), (" sin ", 3), ("Triangle", 4), ("Tri", 4), ("Noise", 5), ("Pulse", 6)];
        for (name, icon) in all {
            assert_eq!(wave_icon(name), icon, "{name}");
        }
        for name in ["", "Wave", "Sawyer", "Mono", "Off", "Square wave 2"] {
            assert_eq!(wave_icon(name), 0, "{name}: stays text");
        }
    }
}
