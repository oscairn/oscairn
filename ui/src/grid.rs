// SPDX-License-Identifier: AGPL-3.0-or-later
//! The grid framework: tiles laid out from the plugin's own layout data (`params.list` with its
//! `oscairn.ui` hints: widget, priority, group, range, rich views), as the JUCE ParameterGrid
//! builds them (hosts/standalone/ParameterGrid.cpp `build`); the notes and the checklist are
//! ui/GRID.md. This module is the data side: which control each parameter gets, where each tile,
//! card, heading and view panel goes, the cursor's moves over tiles in two dimensions, and an
//! envelope's curve. The widgets are slint/grid.slint; a screen on it is params.rs.

use serde_json::Value;

pub const FLAG_HIDDEN: u64 = 1 << 2;
pub const FLAG_READONLY: u64 = 1 << 3;
/// Up to this many values: buttons side by side (`auto`).
const MAX_CHOICES: usize = 4;
/// More values than this get a slider.
const MAX_LISTED: usize = 64;
/// Sizes in the shell's logical pixels, the JUCE grid's in touch mode (RFC 0019: a control 44 high).
const GAP: f32 = 6.;
const TILE: f32 = 84.;
const KNOB_EXTRA: f32 = 36.;
const HEADING: f32 = 30.;
const ENVELOPE: f32 = 180.;

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
    /// The hint's widget as `params.list` names it ("" or "auto": the host decides).
    pub widget: String,
    pub group: u64,
    pub range_high: Option<u32>,
    pub view: u64,
    pub field: String,
    pub index: u64,
    /// A time or rate field's stage length at evenly spaced values.
    pub seconds: Vec<f64>,
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
            widget: p["widget"].as_str().unwrap_or_default().into(),
            group: p["group"].as_u64().unwrap_or(0),
            range_high: p["rangeHigh"].as_u64().map(|v| v as u32),
            view: p["view"].as_u64().unwrap_or(0),
            field: p["field"].as_str().unwrap_or_default().into(),
            index: p["index"].as_u64().unwrap_or(0),
            seconds: p["seconds"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect(),
        })
        .collect()
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub rect: Rect,
    pub title: String,
    pub envelope: Envelope,
    /// The envelope's parameters (indexes into the parameters), which its points refer to by place.
    pub params: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Laid {
    pub tiles: Vec<Tile>,
    pub headings: Vec<(f32, String)>,
    pub cards: Vec<Rect>,
    pub panels: Vec<Panel>,
    pub height: f32,
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
pub fn layout(params: &[Param], groups: &[(String, Vec<usize>)], width: f32, control_of: &dyn Fn(usize) -> Control) -> Laid {
    let mut out = Laid::default();
    let cols = columns(width);
    let tile_w = (width - GAP * (cols - 1) as f32) / cols as f32;
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
        // The envelopes the plugin describes, each a panel across the row, then one recognized by name.
        let mut described: Vec<Vec<usize>> = Vec::new();
        let mut keys: Vec<(u64, u64)> = Vec::new();
        for &i in members {
            let p = &params[i];
            if p.view != 0 && is_envelope(&p.field) && !keys.contains(&(p.view, p.index)) {
                keys.push((p.view, p.index));
            }
        }
        let panel = |out: &mut Laid, y: &mut f32, envelope: Envelope, list: Vec<usize>| {
            out.panels.push(Panel { rect: Rect { x: 0., y: *y, w: width, h: ENVELOPE }, title: "Envelope".into(), envelope, params: list });
            *y += ENVELOPE + GAP;
        };
        for (view, index) in keys {
            let list: Vec<usize> = members.iter().copied().filter(|&i| params[i].view == view && params[i].index == index && is_envelope(&params[i].field)).collect();
            let fields: Vec<&str> = list.iter().map(|&i| params[i].field.as_str()).collect();
            let shape = envelope_of_fields(&fields);
            if shape.points.is_empty() {
                continue;
            }
            described.push(linked_order(&shape).into_iter().map(|k| list[k]).collect());
            panel(&mut out, &mut y, shape, list);
        }
        let plain: Vec<usize> = members.iter().copied().filter(|&i| params[i].view == 0).collect();
        let names: Vec<&str> = plain.iter().map(|&i| params[i].name.as_str()).collect();
        let named = envelope_of_names(&names);
        if !named.points.is_empty() {
            described.push(linked_order(&named).into_iter().map(|k| plain[k]).collect());
            panel(&mut out, &mut y, named, plain.clone());
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
        for order in &described {
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
                y += height + GAP;
            }
            let mut card: Option<Rect> = None;
            for (k, &param) in run.iter().enumerate() {
                // A group wider than the screen goes on in the next row, its rows touching.
                if column == cols {
                    column = 0;
                    y += height + if group { 0. } else { GAP };
                }
                let mut cell = Rect { x: column as f32 * (tile_w + GAP), y, w: tile_w, h: height };
                if group {
                    if k + 1 < run.len() && column + 1 < cols {
                        cell.w = tile_w + GAP; // touches the next
                    }
                    if k + 1 == run.len() && run.len() > cols {
                        cell.w = width - cell.x; // a clean rectangle; nothing after it in the row
                        column = cols - 1;
                    }
                    card = Some(card.map_or(cell, |c| c.union(&cell)));
                }
                out.tiles.push(Tile { param, rect: cell, control: control_of(param), linked: group });
                column += 1;
            }
            if let Some(c) = card {
                out.cards.push(c);
            }
        }
        if out.tiles.len() > start {
            y += height + GAP + 6.;
        }
    }
    // A tile is linked when a card holds it (a single tile widened stays alone).
    for t in &mut out.tiles {
        t.linked = out.cards.iter().any(|c| c.contains(&t.rect));
    }
    out.height = y.max(1.);
    out
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

fn is_envelope(field: &str) -> bool {
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

/// An ADSR recognized by its parameters' names within one module (Attack, Decay, Sustain, Release,
/// optionally Delay and Hold). shortcut: the JUCE app also recognizes DX7 and FM-X names; port them
/// when a plugin without views needs them.
pub fn envelope_of_names(names: &[&str]) -> Envelope {
    let find = |n: &str| names.iter().position(|x| x.eq_ignore_ascii_case(n));
    named(find("attack"), find("decay"), find("sustain"), find("release"), find("delay"), find("hold"))
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

/// The curve in a panel of `w` x `h` (its own pixels): the line's path commands, the filled area's,
/// and each point's place. `params`: the envelope's parameters; `norm`: a parameter's value from 0 to 1.
pub fn envelope_curve(e: &Envelope, params: &[&Param], norm: &dyn Fn(usize) -> f64, w: f32, h: f32) -> (String, String, Vec<(f32, f32)>) {
    let (left, top, right, bottom) = (14., 28., w - 14., h - 10.);
    let (aw, ah) = (right - left, bottom - top);
    let hold = aw * 0.14;
    let segment = (aw - hold) / e.points.len().max(1) as f32;
    let least = (20f32).min(segment / 4.);
    let times: Vec<usize> = e.points.iter().filter_map(|p| p.time).collect();
    let longest = if times.iter().all(|&t| params[t].seconds.len() >= 2) { times.iter().flat_map(|&t| params[t].seconds.iter().copied()).fold(0., f64::max) } else { 0. };
    let across = |t: usize, rate: bool| -> f64 {
        let v = norm(t);
        if longest > 0. {
            (seconds_at(&params[t].seconds, v) / 0.05).ln_1p() / (longest / 0.05).ln_1p()
        } else if rate {
            1. - v
        } else {
            v
        }
    };
    let level = |p: Option<usize>, fixed: f64| bottom - norm_or(p, fixed, norm) as f32 * ah;
    let mut x = left;
    let start = level(e.start, 0.);
    let mut line = format!("M {left:.1} {start:.1}");
    let mut points = Vec::new();
    for p in &e.points {
        if let Some(t) = p.time {
            x += least + across(t, p.rate) as f32 * (segment - least);
        }
        let y = level(p.level, p.fixed);
        line += &format!(" L {x:.1} {y:.1}");
        points.push((x, y));
        if p.sustain {
            x += hold;
            line += &format!(" L {x:.1} {y:.1}");
        }
    }
    let fill = format!("{line} L {x:.1} {bottom:.1} L {left:.1} {bottom:.1} Z");
    (line, fill, points)
}

fn norm_or(p: Option<usize>, fixed: f64, norm: &dyn Fn(usize) -> f64) -> f64 {
    p.map_or(fixed, norm)
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
        layout(params, &[(String::new(), all)], width, &|i| control(&params[i], &[], params))
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
        assert_eq!((r[3].x, r[3].y), (0., TILE + GAP), "the group starts the next row");
        assert_eq!(r[3].w, 205.5 + GAP, "its tiles touch");
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
        let laid = layout(&params, &groups, 900., &|i| control(&params[i], &[], &params));
        assert_eq!(laid.headings[0], (0., "Amp › Env".to_string()));
        assert_eq!(laid.panels.len(), 1, "an ADSR by name");
        assert_eq!(laid.panels[0].rect.y, HEADING);
        assert_eq!(laid.tiles[0].rect.y, HEADING + ENVELOPE + GAP, "the tiles under the view");
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
        let (line, fill, points) = envelope_curve(&named, &refs, &|i| [0., 0.5, 0.5, 1.][i], 400., 180.);
        assert!(line.starts_with("M 14.0 170.0") && fill.ends_with('Z'), "{line}");
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].1, 28., "the attack reaches the top");
        assert!(points[1].1 > 28. && points[1].1 < 170., "the sustain level halfway");
        assert_eq!(arc(0., 0.), "");
        assert!(arc(0., 1.).contains(" 1 1 "), "past 180 degrees: the large arc");
    }
}
