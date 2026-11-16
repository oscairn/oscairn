// SPDX-License-Identifier: AGPL-3.0-or-later
//! The grid's rich views, as ParameterGrid.cpp draws them (its `Envelope`, `Filter`, `Lfo`, `KeyMap` and
//! `Operators`): each is a panel across the row above the tiles, drawn from the plugin's parameters, and
//! the ones that can be dragged set those parameters, which keep their tiles too (nothing here is the only
//! way to change a value: the cursor and the dial work on the tiles, and the node a tile sets is marked).
//!
//! Everything is data: `draw` gives the shapes (paths in the panel's own pixels, drawn unscaled), the
//! handles, labels and boxes of one panel from one set of points; `grab` and `drag` turn a touch on it into
//! the parameter values it sets. The widgets are `ViewPanel` in slint/grid.slint; params.rs joins them to the
//! engine. Sizes are the JUCE grid's in touch mode (the keyboard's); a handle is taken within `REACH`.

use crate::grid::{self, Envelope, Param};

/// How near a touch must be to take a handle (ParameterGrid.cpp `reach`, touch mode).
pub const REACH: f32 = 24.;

/// The theme color a shape, label or box is drawn in (slint/grid.slint maps each to the theme).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Accent = 0,
    Muted = 1,
    Border = 2,
    Text = 3,
    Raised = 4,
    /// The border faintly: a plot's frame and baselines.
    Frame = 5,
    Background = 6,
    /// The accent lightly: what a curve encloses.
    AccentWash = 7,
    /// A key map's white keys.
    Keys = 8,
    /// The zone a key map is for, and the others.
    Zone = 9,
    ZoneFaint = 10,
    /// Text on an accent fill (a carrier operator's number).
    OnAccent = 11,
}

/// A path in the panel's pixels: stroked (`width`: 0 a hairline, 1 thin, 2 the cursor's) or filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub ink: Ink,
    pub fill: bool,
    pub width: i32,
    pub commands: String,
}

/// A handle to take: its center, and the parameters (indexes into the grid's) dragging it sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    pub at: (f32, f32),
    pub params: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: String,
    pub ink: Ink,
    pub bold: bool,
    /// 0 from the left, 1 centered, 2 from the right.
    pub align: i32,
}

/// A rounded box with a text in it.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxDraw {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub fill: Ink,
    pub edge: Option<Ink>,
    pub text: String,
    pub text_ink: Ink,
    pub bold: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawn {
    pub title: String,
    pub shapes: Vec<Shape>,
    pub handles: Vec<Handle>,
    pub labels: Vec<Label>,
    pub boxes: Vec<BoxDraw>,
}

/// A view and the parameters it reads. Envelope, filter and LFO shapes index into the panel's `params`;
/// a map's and an operator view's into the grid's.
#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Envelope(Envelope),
    Filter(FilterShape),
    Lfo(LfoShape),
    /// A key or velocity map, and the index (zone, element) it is for.
    Map(MapShape, u64),
    Fm(FmShape),
}

/// What a view reads: the grid's parameters and their values, the panel's size, and the parameter the
/// cursor is on (its node is marked).
pub struct Ctx<'a> {
    pub params: &'a [Param],
    /// The panel's parameters (indexes into `params`).
    pub members: &'a [usize],
    /// A parameter's value from 0 to 1, and the plugin's text for it.
    pub norm: &'a dyn Fn(usize) -> f64,
    pub text: &'a dyn Fn(usize) -> String,
    pub focus: Option<usize>,
    pub w: f32,
    pub h: f32,
    /// Where the plot starts down the panel: under the title, with room for a handle's radius.
    pub top: f32,
}

/// A handle taken by touch, and what it set at the start.
#[derive(Clone, Debug, PartialEq)]
pub enum Held {
    /// A handle (an envelope's point, a map's edge).
    Node(usize),
    /// An FM view's algorithm: dragged across the routing, a step per 24 px; its value and where the touch began.
    Algorithm { param: usize, start: f64, from: (f32, f32) },
    /// An operator's level bar.
    Level { param: usize },
    /// An operator's ratio: dragged up or down, a step per 12 px.
    Ratio { param: usize, start: f64, from: (f32, f32) },
}

fn nearest(handles: &[Handle], at: (f32, f32)) -> Option<usize> {
    let mut best = REACH;
    let mut found = None;
    for (k, h) in handles.iter().enumerate() {
        let d = (h.at.0 - at.0).hypot(h.at.1 - at.1);
        if d < best {
            best = d;
            found = Some(k);
        }
    }
    found
}

pub fn draw(view: &View, c: &Ctx) -> Drawn {
    match view {
        View::Envelope(e) => draw_envelope(e, c),
        View::Filter(f) => draw_filter(f, c),
        View::Lfo(l) => draw_lfo(l, c),
        View::Map(m, index) => draw_map(m, *index, c),
        View::Fm(f) => draw_fm(f, c),
    }
}

/// The handle under a touch at `at` (the panel's pixels), if one is within reach: the views that are
/// previews (a filter's response, an LFO's wave) have none.
pub fn grab(view: &View, c: &Ctx, at: (f32, f32)) -> Option<Held> {
    match view {
        View::Envelope(_) | View::Map(..) => nearest(&draw(view, c).handles, at).map(Held::Node),
        View::Fm(f) => fm_grab(f, c, at),
        View::Filter(_) | View::Lfo(_) => None,
    }
}

/// The parameters (indexes into the grid's) a held handle sets: their gestures open and close with the touch.
pub fn touched(view: &View, c: &Ctx, held: &Held) -> Vec<usize> {
    match held {
        Held::Node(k) => draw(view, c).handles.get(*k).map(|h| h.params.clone()).unwrap_or_default(),
        Held::Algorithm { param, .. } | Held::Level { param } | Held::Ratio { param, .. } => vec![*param],
    }
}

/// What dragging a held handle to `at` sets: each parameter and its value.
pub fn drag(view: &View, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match (view, held) {
        (View::Envelope(e), Held::Node(k)) => envelope_drag(e, c, *k, at),
        (View::Map(m, index), Held::Node(k)) => map_drag(m, *index, c, *k, at),
        (View::Fm(f), held) => fm_drag(f, c, held, at),
        _ => vec![],
    }
}

/// Whether a touch sets its parameter where it lands, before it moves: an operator's level bar does.
pub fn drags_on_grab(held: &Held) -> bool {
    matches!(held, Held::Level { .. })
}

/// What letting go at `at` sets: a tap on the routing steps the algorithm, back on its left half, on on its right.
pub fn release(view: &View, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match (view, held) {
        (View::Fm(f), Held::Algorithm { param, start, from }) if (at.0 - from.0).hypot(at.1 - from.1) < 4. => {
            let g = fm_geometry(f, c);
            let p = &c.params[*param];
            let step = if at.0 < (g.routing.0 + g.routing.2) / 2. { -1. } else { 1. };
            vec![(*param, (start + step).clamp(p.min, p.max))]
        }
        _ => vec![],
    }
}

/// The plot's area in a panel: inset 14 across and 10 below, from `top` down.
fn plot(c: &Ctx) -> (f32, f32, f32, f32) {
    (14., c.top, c.w - 14., c.h - 10.)
}

fn centered(text: &str, (l, t, r, b): (f32, f32, f32, f32)) -> Label {
    Label { x: l, y: t, w: r - l, h: b - t, text: text.into(), ink: Ink::Muted, bold: false, align: 1 }
}

/// A one-pixel row or column of a frame sits on a pixel: `v` to the pixel's middle.
fn crisp(v: f32) -> f32 {
    v.floor() + 0.5
}

// The key and velocity map (ParameterGrid::KeyMap): the zones as rectangles over the keys (across) and the
// velocities (up), the one this index owns in the accent; its level scaling across the keys from the
// breakpoint; and velocity's effect on level in a box at the right. Drag a zone's edge, the breakpoint, a
// scaling end or the velocity line to set its parameter.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Zone {
    pub index: u64,
    pub key_low: Option<usize>,
    pub key_high: Option<usize>,
    pub velocity_low: Option<usize>,
    pub velocity_high: Option<usize>,
}

/// A key or velocity map a plugin describes: its zones' key and velocity ranges (one per index), and one
/// index's level scaling across the keys and by velocity. Indexes are the grid's parameters'.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapShape {
    pub zones: Vec<Zone>,
    pub breakpoint: Option<usize>,
    pub left_depth: Option<usize>,
    pub right_depth: Option<usize>,
    pub left_curve: Option<usize>,
    pub right_curve: Option<usize>,
    pub velocity_depth: Option<usize>,
    pub velocity_curve: Option<usize>,
}

impl MapShape {
    pub fn empty(&self) -> bool {
        self.zones.is_empty() && self.breakpoint.is_none() && self.velocity_depth.is_none()
    }
}

pub fn is_map_field(field: &str) -> bool {
    matches!(field, "keyLow" | "keyHigh" | "velocityLow" | "velocityHigh" | "breakpoint" | "leftDepth" | "rightDepth" | "leftCurve" | "rightCurve" | "velocityDepth" | "velocityCurve")
}

/// The map of `view` as `index` sees it: every index's zone, and that index's scaling.
pub fn map_of(params: &[Param], view: u64, index: u64) -> MapShape {
    let mut m = MapShape::default();
    if view == 0 {
        return m;
    }
    for (i, p) in params.iter().enumerate().filter(|(_, p)| p.view == view) {
        if matches!(p.field.as_str(), "keyLow" | "keyHigh" | "velocityLow" | "velocityHigh") {
            let at = match m.zones.iter().position(|z| z.index == p.index) {
                Some(at) => at,
                None => {
                    m.zones.push(Zone { index: p.index, ..Default::default() });
                    m.zones.len() - 1
                }
            };
            let z = &mut m.zones[at];
            match p.field.as_str() {
                "keyLow" => z.key_low = Some(i),
                "keyHigh" => z.key_high = Some(i),
                "velocityLow" => z.velocity_low = Some(i),
                _ => z.velocity_high = Some(i),
            }
        }
        if p.index != index {
            continue;
        }
        match p.field.as_str() {
            "breakpoint" => m.breakpoint = Some(i),
            "leftDepth" => m.left_depth = Some(i),
            "rightDepth" => m.right_depth = Some(i),
            "leftCurve" => m.left_curve = Some(i),
            "rightCurve" => m.right_curve = Some(i),
            "velocityDepth" => m.velocity_depth = Some(i),
            "velocityCurve" => m.velocity_curve = Some(i),
            _ => {}
        }
    }
    m.zones.sort_by_key(|z| z.index);
    m
}

/// What dragging a map's handle sets: a key, a velocity, a scaling depth, velocity's depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pull {
    Across,
    Up,
    Depth,
    Velocity,
}

type Area = (f32, f32, f32, f32);

struct MapGeometry {
    plane: Area,
    keys: Area,
    velocity: Option<Area>,
    /// The handles: where, the parameter (the grid's), what dragging sets.
    handles: Vec<((f32, f32), usize, Pull)>,
}

fn width_of(a: Area) -> f32 {
    a.2 - a.0
}

fn height_of(a: Area) -> f32 {
    a.3 - a.1
}

/// A map's parameter as 0 to 1, or `otherwise` without one.
fn map_value(c: &Ctx, p: Option<usize>, otherwise: f64) -> f64 {
    p.map_or(otherwise, |p| (c.norm)(p))
}

fn map_at(c: &Ctx, plane: Area, p: Option<usize>) -> f32 {
    plane.0 + map_value(c, p, 0.) as f32 * width_of(plane)
}

/// A zone's rectangle: left, top, right, bottom.
fn zone_rect(c: &Ctx, plane: Area, z: &Zone) -> Area {
    let left = plane.0 + map_value(c, z.key_low, 0.) as f32 * width_of(plane);
    let right = plane.0 + map_value(c, z.key_high, 1.) as f32 * width_of(plane);
    let top = plane.3 - map_value(c, z.velocity_high, 1.) as f32 * height_of(plane);
    let bottom = plane.3 - map_value(c, z.velocity_low, 0.) as f32 * height_of(plane);
    (left.min(right), top.min(bottom), left.max(right), top.max(bottom))
}

/// The scaled level at `x`, from the middle: up or down by each side's depth, straight or curved as its
/// curve's value text says ("-LIN", "+EXP"; a minus lowers it).
fn map_scaled(m: &MapShape, c: &Ctx, plane: Area, x: f32) -> f32 {
    let b = map_at(c, plane, m.breakpoint);
    let left = x < b;
    let (depth, curve) = if left { (m.left_depth, m.left_curve) } else { (m.right_depth, m.right_curve) };
    let span = if left { b - plane.0 } else { plane.2 - b };
    let mut t = if span > 0. { (x - b).abs() / span } else { 0. };
    let shape = curve.map(|k| (c.text)(k).to_uppercase()).unwrap_or_default();
    if shape.contains("EXP") {
        t *= t;
    }
    let sign = if shape.starts_with('-') { -1. } else { 1. };
    (plane.1 + plane.3) / 2. - sign * t * map_value(c, depth, 0.) as f32 * height_of(plane) / 2.
}

/// Where the velocity line starts: the level at the softest velocity.
fn map_quiet(m: &MapShape, c: &Ctx, velocity: Area) -> f32 {
    velocity.1 + map_value(c, m.velocity_depth, 0.) as f32 * height_of(velocity)
}

fn map_geometry(m: &MapShape, index: u64, c: &Ctx) -> MapGeometry {
    let (left, top, right, bottom) = plot(c);
    let mut keys = (left, bottom - 14., right, bottom);
    let mut plane = (left, top, right, bottom - 14.);
    let velocity = m.velocity_depth.map(|_| {
        let width = 90f32.min(width_of(plane) / 4.);
        let v = (plane.2 - width + 12., plane.1, plane.2, plane.3);
        plane.2 -= width;
        v
    });
    if let Some(v) = velocity {
        keys.2 = v.0 - 12.;
    }
    let mut handles = Vec::new();
    for z in m.zones.iter().filter(|z| z.index == index) {
        let r = zone_rect(c, plane, z);
        let (mid_x, mid_y) = ((r.0 + r.2) / 2., (r.1 + r.3) / 2.);
        if let Some(p) = z.key_low {
            handles.push(((r.0, mid_y), p, Pull::Across));
        }
        if let Some(p) = z.key_high {
            handles.push(((r.2, mid_y), p, Pull::Across));
        }
        if let Some(p) = z.velocity_high {
            handles.push(((mid_x, r.1), p, Pull::Up));
        }
        if let Some(p) = z.velocity_low {
            handles.push(((mid_x, r.3), p, Pull::Up));
        }
    }
    if let Some(b) = m.breakpoint {
        handles.push(((map_at(c, plane, Some(b)), (plane.1 + plane.3) / 2.), b, Pull::Across));
        if let Some(p) = m.left_depth {
            handles.push(((plane.0, map_scaled(m, c, plane, plane.0)), p, Pull::Depth));
        }
        if let Some(p) = m.right_depth {
            handles.push(((plane.2, map_scaled(m, c, plane, plane.2)), p, Pull::Depth));
        }
    }
    if let (Some(v), Some(p)) = (velocity, m.velocity_depth) {
        handles.push(((v.0, map_quiet(m, c, v)), p, Pull::Velocity));
    }
    MapGeometry { plane, keys, velocity, handles }
}

/// A rectangle drawn inside its edge, `stroke` wide.
fn inner_rect(a: Area, stroke: f32) -> String {
    rect_path(a.0 + stroke / 2., a.1 + stroke / 2., width_of(a) - stroke, height_of(a) - stroke)
}

fn small_label(x: f32, y: f32, w: f32, text: String, align: i32) -> Label {
    Label { x, y, w, h: 14., text, ink: Ink::Muted, bold: false, align }
}

fn draw_map(m: &MapShape, index: u64, c: &Ctx) -> Drawn {
    let g = map_geometry(m, index, c);
    let mut d = Drawn { title: "Key map".into(), ..Default::default() };
    // The keys under the plane, an octave's black keys where a keyboard has them.
    d.shapes.push(Shape { ink: Ink::Keys, fill: true, width: 2, commands: rect_path(g.keys.0, g.keys.1, width_of(g.keys), height_of(g.keys)) });
    let mut black = String::new();
    for note in 0..128 {
        if matches!(note % 12, 1 | 3 | 6 | 8 | 10) {
            let w = width_of(g.keys) / 128.;
            black += &(rect_path(g.keys.0 + width_of(g.keys) * note as f32 / 128., g.keys.1, w, height_of(g.keys) * 0.6) + " ");
        }
    }
    d.shapes.push(Shape { ink: Ink::Background, fill: true, width: 2, commands: black });
    d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: inner_rect(g.plane, 1.) });
    for z in &m.zones {
        let r = zone_rect(c, g.plane, z);
        let mine = z.index == index;
        d.shapes.push(Shape { ink: if mine { Ink::Zone } else { Ink::ZoneFaint }, fill: true, width: 2, commands: rect_path(r.0, r.1, width_of(r), height_of(r)) });
        d.shapes.push(Shape { ink: if mine { Ink::Accent } else { Ink::Muted }, fill: false, width: if mine { 1 } else { 0 }, commands: inner_rect(r, if mine { 1.5 } else { 1. }) });
        if m.zones.len() > 1 {
            d.labels.push(small_label(r.0 + 4., r.1 + 2., (width_of(r) - 8.).max(0.), z.index.to_string(), 0));
        }
    }
    if let Some(b) = m.breakpoint {
        let x = map_at(c, g.plane, Some(b)).floor() + 0.5;
        d.shapes.push(Shape { ink: Ink::Border, fill: false, width: 0, commands: format!("M {x:.2} {:.2} L {x:.2} {:.2}", g.plane.1, g.plane.3) });
        let mut points = Vec::new();
        let mut x = g.plane.0;
        while x <= g.plane.2 {
            points.push((x, map_scaled(m, c, g.plane, x)));
            x += 2.;
        }
        d.shapes.push(Shape { ink: Ink::Accent, fill: false, width: 2, commands: poly(&points) });
        d.labels.push(small_label(map_at(c, g.plane, Some(b)) + 4., g.plane.1 + 2., 80., (c.text)(b), 0));
    }
    if let (Some(v), Some(_)) = (g.velocity, m.velocity_depth) {
        d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: inner_rect(v, 1.) });
        d.shapes.push(Shape { ink: Ink::Accent, fill: false, width: 2, commands: poly(&[(v.0, map_quiet(m, c, v)), (v.2, v.1)]) });
        d.labels.push(Label { x: v.0, y: g.keys.1, w: width_of(v), h: height_of(g.keys), text: "Velocity".into(), ink: Ink::Muted, bold: false, align: 1 });
    }
    d.handles = g.handles.iter().map(|(at, p, _)| Handle { at: *at, params: vec![*p] }).collect();
    d
}

fn map_drag(m: &MapShape, index: u64, c: &Ctx, k: usize, at: (f32, f32)) -> Vec<(usize, f64)> {
    let g = map_geometry(m, index, c);
    let Some(&(_, param, pull)) = g.handles.get(k) else { return vec![] };
    let across = ((at.0 - g.plane.0) / width_of(g.plane)).clamp(0., 1.);
    let up = ((g.plane.3 - at.1) / height_of(g.plane)).clamp(0., 1.);
    let v = match pull {
        Pull::Across => across,
        Pull::Up => up,
        Pull::Depth => ((at.1 - (g.plane.1 + g.plane.3) / 2.).abs() / (height_of(g.plane) / 2.)).clamp(0., 1.),
        Pull::Velocity => g.velocity.map_or(0., |v| ((at.1 - v.1) / height_of(v)).clamp(0., 1.)),
    };
    vec![(param, c.params[param].denorm(f64::from(v)))]
}

// The FM operator view (ParameterGrid::Operators): its algorithm's routing on the left (carriers on the
// bottom row, filled; each modulator above what it modulates; feedback as a loop), and a column per
// operator with its level as a bar, its ratio and its envelope's shape. Drag across the routing (or tap
// either half) to choose the algorithm, a level bar to set the level, a ratio up or down to step it.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Operator {
    pub level: Option<usize>,
    pub ratio: Option<usize>,
    pub fine: Option<usize>,
    pub detune: Option<usize>,
    pub fixed: Option<usize>,
    /// The operator's envelope parameters (the grid's), in the plugin's order.
    pub envelope: Vec<usize>,
}

/// An FM operator view a plugin describes: the algorithm, feedback, and each operator's parameters
/// (operators 1 to 8, as the view's indexes). Indexes are the grid's parameters'.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FmShape {
    pub algorithm: Option<usize>,
    pub feedback: Option<usize>,
    /// Operator n at n - 1.
    pub operators: Vec<Operator>,
}

pub fn fm_of(params: &[Param], view: u64) -> FmShape {
    let mut f = FmShape::default();
    if view == 0 {
        return f;
    }
    for (i, p) in params.iter().enumerate().filter(|(_, p)| p.view == view) {
        match p.field.as_str() {
            "algorithm" => f.algorithm = Some(i),
            "feedback" => f.feedback = Some(i),
            _ => {}
        }
        // Operators 1 to 8 (the reference grows the list to any index; the diagram holds eight).
        if p.index == 0 || p.index > 8 {
            continue;
        }
        if f.operators.len() < p.index as usize {
            f.operators.resize(p.index as usize, Operator::default());
        }
        let op = &mut f.operators[p.index as usize - 1];
        match p.field.as_str() {
            "operatorLevel" => op.level = Some(i),
            "ratio" => op.ratio = Some(i),
            "fine" => op.fine = Some(i),
            "detune" => op.detune = Some(i),
            "fixed" => op.fixed = Some(i),
            field if grid::is_envelope(field) => op.envelope.push(i),
            _ => {}
        }
    }
    f
}

/// Where each operator of a routing sits in its diagram: its row (0 the carriers', at the bottom) and across,
/// in operators' widths from the diagram's middle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    pub row: i32,
    pub x: f32,
}

/// The places of the first `count` (at most 8) operators of `routing` (eight operators' input masks, the
/// carriers' mask, the feedback's operator from and to): a carrier's row is 0, a modulator's one above the
/// highest of the operators it modulates (a loop is cut after eight passes), each over the middle of what
/// it modulates (a carrier, or an operator nothing reaches, in number order), then pushed right until none
/// overlap. Feedback is not an input: drawn apart.
pub fn algorithm_layout(routing: &[u8; 11], count: usize) -> Vec<Place> {
    let count = count.min(8);
    let mut row = vec![-1i32; count];
    for (n, r) in row.iter_mut().enumerate() {
        if routing[8] & (1 << n) != 0 {
            *r = 0;
        }
    }
    for _ in 0..8 {
        for target in 0..count {
            for m in 0..count {
                if row[target] >= 0 && routing[target] & (1 << m) != 0 && m != target && row[m] < row[target] + 1 {
                    row[m] = (row[target] + 1).min(7);
                }
            }
        }
    }
    let mut places = vec![Place::default(); count];
    let mut rows = 0;
    for n in 0..count {
        places[n].row = row[n].max(0);
        rows = rows.max(places[n].row + 1);
    }
    let mut next = 0f32;
    for r in 0..rows {
        let mut in_row: Vec<usize> = Vec::new();
        for n in 0..count {
            if places[n].row != r {
                continue;
            }
            let (mut sum, mut targets) = (0f32, 0);
            for t in 0..count {
                if t != n && routing[t] & (1 << n) != 0 && places[t].row < r {
                    sum += places[t].x;
                    targets += 1;
                }
            }
            places[n].x = if targets > 0 {
                sum / targets as f32
            } else {
                next += 1.;
                next - 1.
            };
            in_row.push(n);
        }
        in_row.sort_by(|&a, &b| places[a].x.total_cmp(&places[b].x));
        for k in 1..in_row.len() {
            places[in_row[k]].x = places[in_row[k]].x.max(places[in_row[k - 1]].x + 1.);
        }
    }
    let (mut low, mut high) = (0f32, 0f32);
    for p in &places {
        low = low.min(p.x);
        high = high.max(p.x);
    }
    for p in &mut places {
        p.x -= (low + high) / 2.;
    }
    places
}

struct Column {
    name: Area,
    level: Area,
    level_text: Area,
    ratio: Area,
    envelope: Area,
}

struct FmGeometry {
    routing: Area,
    /// Where the carriers' outputs join, down the routing.
    output: f32,
    boxes: Vec<Area>,
    columns: Vec<Column>,
    route: Option<[u8; 11]>,
}

/// The algorithm's routing at its value, when the plugin gives one.
fn fm_route(f: &FmShape, c: &Ctx) -> Option<[u8; 11]> {
    let a = &c.params[f.algorithm?];
    let at = ((c.norm)(f.algorithm?) * (a.max - a.min)).round() as usize;
    a.algorithms.get(at).copied()
}

fn contains(a: Area, at: (f32, f32), grow: f32) -> bool {
    at.0 >= a.0 - grow && at.0 <= a.2 + grow && at.1 >= a.1 - grow && at.1 <= a.3 + grow
}

fn fm_geometry(f: &FmShape, c: &Ctx) -> FmGeometry {
    let (left, top, right, bottom) = plot(c);
    let routing = (left, top, left + (0.36 * (right - left)).min(360.), bottom);
    let area = (routing.2 + 14., top, right, bottom);
    let count = f.operators.len();
    let route = fm_route(f, c);
    // The routing's operators: rows from the bottom, each row centered; no routing: one row.
    let mut places: Vec<Place> = (0..count).map(|n| Place { row: 0, x: n as f32 - (count as f32 - 1.) / 2. }).collect();
    if let Some(r) = &route {
        places = algorithm_layout(r, count);
    }
    let (mut rows, mut span) = (1, 1f32);
    for p in &places {
        rows = rows.max(p.row + 1);
        span = span.max(p.x.abs() * 2. + 1.);
    }
    let output = routing.3 - 10.;
    let row_height = 46f32.min((height_of(routing) - 24.) / rows as f32);
    let side = (row_height - 12.).min(30.);
    let column_width = 48f32.min((width_of(routing) - 24.) / span);
    let centre = (routing.0 + routing.2) / 2.;
    let boxes = places
        .iter()
        .map(|p| {
            let (x, y) = (centre + p.x * column_width, output - 12. - row_height * p.row as f32 - side / 2.);
            (x - side / 2., y - side / 2., x + side / 2., y + side / 2.)
        })
        .collect();
    let width = if count > 0 { width_of(area) / count as f32 } else { 0. };
    let columns = (0..count)
        .map(|n| {
            let (x, r) = (area.0 + width * n as f32 + 4., area.0 + width * (n + 1) as f32 - 4.);
            let name = (x, area.1, r, area.1 + 18.);
            let rest_top = name.3;
            let envelope = (x, area.3 - (area.3 - rest_top) * 0.26, r, area.3);
            let mut below = envelope.1 - 6.; // what is left above the ratio
            let ratio = (x, below - 30., r, below);
            below = ratio.1 - 6.;
            let level_text = (x, below - 16., r, below);
            let rest = (x, rest_top, r, level_text.1);
            let bar_width = width_of(rest).min(36.);
            let level = ((rest.0 + rest.2 - bar_width) / 2., rest.1, (rest.0 + rest.2 + bar_width) / 2., rest.3);
            Column { name, level, level_text, ratio, envelope }
        })
        .collect();
    FmGeometry { routing, output, boxes, columns, route }
}

fn fm_text(f: &FmShape, c: &Ctx) -> String {
    let mut title = format!("Algorithm {}", f.algorithm.map(|a| (c.text)(a)).unwrap_or_default());
    if let Some(k) = f.feedback {
        title += &format!("  \u{b7}  Feedback {}", (c.text)(k));
    }
    title
}

fn line(a: (f32, f32), b: (f32, f32)) -> String {
    poly(&[a, b])
}

fn draw_fm(f: &FmShape, c: &Ctx) -> Drawn {
    let g = fm_geometry(f, c);
    let mut d = Drawn { title: fm_text(f, c), ..Default::default() };
    d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: inner_rect(g.routing, 1.) });
    // The routing: lines first, then the operators over them.
    if let Some(r) = &g.route {
        let mut wires = String::new();
        for t in 0..g.boxes.len() {
            for m in 0..g.boxes.len() {
                if r[t] & (1 << m) != 0 && m != t {
                    wires += &(line(((g.boxes[m].0 + g.boxes[m].2) / 2., g.boxes[m].3), ((g.boxes[t].0 + g.boxes[t].2) / 2., g.boxes[t].1)) + " ");
                }
            }
            if r[8] & (1 << t) != 0 {
                let x = (g.boxes[t].0 + g.boxes[t].2) / 2.;
                wires += &(line((x, g.boxes[t].3), (x, g.output)) + " ");
            }
        }
        d.shapes.push(Shape { ink: Ink::Muted, fill: false, width: 1, commands: wires });
        // Feedback: out of its operator's right side, up and over into the operator it feeds.
        let n = g.boxes.len();
        if r[9] != 0 && usize::from(r[9]) <= n && r[10] != 0 && usize::from(r[10]) <= n {
            let (from, to) = (g.boxes[usize::from(r[9]) - 1], g.boxes[usize::from(r[10]) - 1]);
            let side = from.2.max(to.2) + 6.;
            let up = from.1.min(to.1) - 6.;
            let to_x = (to.0 + to.2) / 2.;
            let path = poly(&[(from.2, (from.1 + from.3) / 2.), (side, (from.1 + from.3) / 2.), (side, up), (to_x, up), (to_x, to.1)]);
            d.shapes.push(Shape { ink: Ink::Accent, fill: false, width: 1, commands: path });
        }
        let y = crisp(g.output);
        d.shapes.push(Shape { ink: Ink::Muted, fill: false, width: 0, commands: line((g.routing.0 + 12., y), (g.routing.2 - 12., y)) });
    }
    let carrier = |n: usize| g.route.is_some_and(|r| r[8] & (1 << n) != 0);
    for (n, b) in g.boxes.iter().enumerate() {
        let (fill, ink) = if carrier(n) { (Ink::Accent, Ink::OnAccent) } else { (Ink::Raised, Ink::Text) };
        d.boxes.push(BoxDraw { x: b.0, y: b.1, w: width_of(*b), h: height_of(*b), fill, edge: Some(Ink::Border), text: (n + 1).to_string(), text_ink: ink, bold: true });
    }
    // A column per operator: its number, its level as a bar and text, its ratio, and its envelope.
    for (n, (col, op)) in g.columns.iter().zip(&f.operators).enumerate() {
        d.labels.push(Label { x: col.name.0, y: col.name.1, w: width_of(col.name), h: height_of(col.name), text: format!("Op {}", n + 1), ink: if carrier(n) { Ink::Accent } else { Ink::Text }, bold: true, align: 1 });
        d.boxes.push(BoxDraw { x: col.level.0, y: col.level.1, w: width_of(col.level), h: height_of(col.level), fill: Ink::Raised, edge: None, text: String::new(), text_ink: Ink::Text, bold: false });
        if let Some(l) = op.level {
            let v = (c.norm)(l) as f32 * height_of(col.level);
            d.boxes.push(BoxDraw { x: col.level.0, y: col.level.3 - v, w: width_of(col.level), h: v, fill: Ink::Accent, edge: None, text: String::new(), text_ink: Ink::Text, bold: false });
            d.labels.push(Label { x: col.level_text.0, y: col.level_text.1, w: width_of(col.level_text), h: height_of(col.level_text), text: (c.text)(l), ink: Ink::Text, bold: false, align: 1 });
        }
        let mut ratio = op.ratio.map(|r| (c.text)(r)).unwrap_or_default();
        if let Some(fine) = op.fine {
            ratio += &format!(" {}", (c.text)(fine));
        }
        if let Some(fixed) = op.fixed.filter(|&x| (c.norm)(x) > 0.5) {
            ratio = format!("{} {ratio}", (c.text)(fixed));
        }
        d.boxes.push(BoxDraw { x: col.ratio.0, y: col.ratio.1, w: width_of(col.ratio), h: height_of(col.ratio), fill: Ink::Raised, edge: None, text: ratio, text_ink: Ink::Text, bold: false });
        if !op.envelope.is_empty() {
            let fields: Vec<&str> = op.envelope.iter().map(|&i| c.params[i].field.as_str()).collect();
            let e = grid::envelope_of_fields(&fields);
            let refs: Vec<&Param> = op.envelope.iter().map(|&i| &c.params[i]).collect();
            let times: Vec<&Param> = e.points.iter().filter_map(|p| p.time).map(|t| refs[t]).collect();
            let longest = grid::longest_stage(&times);
            let env = col.envelope;
            let level = |k: Option<usize>, fixed: f64| env.3 - k.map_or(fixed, |k| (c.norm)(op.envelope[k])) as f32 * height_of(env);
            let segment = width_of(env) * 0.8 / e.points.len().max(1) as f32;
            let hold = width_of(env) * 0.2;
            let mut x = env.0;
            let mut points = vec![(x, level(e.start, 0.))];
            for p in &e.points {
                if let Some(t) = p.time {
                    x += grid::stage_across(refs[t], (c.norm)(op.envelope[t]), p.rate, longest) as f32 * segment;
                }
                points.push((x, level(p.level, p.fixed)));
                if p.sustain {
                    x += hold;
                    points.push((x, level(p.level, p.fixed)));
                }
            }
            d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: inner_rect(env, 1.) });
            d.shapes.push(Shape { ink: Ink::Accent, fill: false, width: 1, commands: poly(&points) });
        }
    }
    d
}

fn fm_grab(f: &FmShape, c: &Ctx, at: (f32, f32)) -> Option<Held> {
    let g = fm_geometry(f, c);
    let raw = |p: usize| c.params[p].min + (c.norm)(p) * (c.params[p].max - c.params[p].min);
    let mut held = None;
    if let (true, Some(a)) = (contains(g.routing, at, 0.), f.algorithm) {
        held = Some(Held::Algorithm { param: a, start: raw(a), from: at });
    }
    for (col, op) in g.columns.iter().zip(&f.operators) {
        if let (true, Some(l)) = (contains(col.level, at, 4.), op.level) {
            held = Some(Held::Level { param: l });
        }
        if let (true, Some(r)) = (contains(col.ratio, at, 4.), op.ratio) {
            held = Some(Held::Ratio { param: r, start: raw(r), from: at });
        }
    }
    held
}

fn fm_drag(f: &FmShape, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match held {
        Held::Level { param } => {
            let g = fm_geometry(f, c);
            let Some(col) = f.operators.iter().position(|o| o.level == Some(*param)).and_then(|n| g.columns.get(n)) else { return vec![] };
            vec![(*param, c.params[*param].denorm(f64::from(((col.level.3 - at.1) / height_of(col.level)).clamp(0., 1.))))]
        }
        Held::Algorithm { param, start, from } | Held::Ratio { param, start, from } => {
            // A step per 24 pixels across the routing, or up a ratio; a continuous ratio by its range.
            let steps = if matches!(held, Held::Algorithm { .. }) { f64::from(at.0 - from.0) / 24. } else { f64::from(from.1 - at.1) / 12. };
            let p = &c.params[*param];
            let v = if p.stepped { start + steps.trunc() } else { start + steps / 100. * (p.max - p.min) };
            vec![(*param, v.clamp(p.min, p.max))]
        }
        Held::Node(_) => vec![],
    }
}

// The filter (ParameterGrid::Filter): its response across the audio range, from its cutoff (across) and
// resonance (the peak) for the type its value text names. A preview: its tiles edit it.

/// A filter recognized from its parameters' names: Cutoff and Resonance, and its Type when it has one,
/// whose value text gives the response. Indexes are places in the names given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterShape {
    pub cutoff: Option<usize>,
    pub resonance: Option<usize>,
    pub kind: Option<usize>,
}

impl FilterShape {
    pub fn empty(&self) -> bool {
        self.cutoff.is_none() || self.resonance.is_none()
    }
    /// The parameters in the order their tiles go when linked.
    pub fn linked_order(&self) -> Vec<usize> {
        [self.kind, self.cutoff, self.resonance].into_iter().flatten().collect()
    }
}

/// By name only, as the reference does (Cutoff or Frequency, Resonance or Q, Type or Filter Type).
pub fn filter_of(names: &[&str]) -> FilterShape {
    let find = |options: &[&str]| options.iter().find_map(|o| names.iter().position(|n| n.eq_ignore_ascii_case(o)));
    FilterShape { cutoff: find(&["Cutoff", "Frequency"]), resonance: find(&["Resonance", "Q"]), kind: find(&["Type", "Filter Type"]) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    None,
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Unknown,
}

/// A filter type's name as a response: "LP 24 dB" or "LPF24D" a four-pole low-pass; "Off" or "Thru" none.
pub fn filter_named(text: &str) -> (Response, u32) {
    let t: String = text.to_uppercase().chars().filter(|c| *c != ' ').collect();
    if t == "OFF" || t == "THRU" {
        return (Response::None, 2);
    }
    let (lp, hp, bp) = (t.contains("LP"), t.contains("HP"), t.contains("BP"));
    let notch = t.contains("BEF") || t.contains("NOTCH") || t.starts_with('N');
    let kind = if bp || (lp && hp) {
        Response::BandPass
    } else if notch {
        Response::Notch
    } else if hp {
        Response::HighPass
    } else if lp {
        Response::LowPass
    } else {
        Response::Unknown
    };
    let poles = if t.contains("24") {
        4
    } else if t.contains("18") {
        3
    } else if t.contains("12") {
        2
    } else if t.contains('6') {
        1
    } else {
        2
    };
    (kind, poles)
}

/// The response in dB (-48 to 24) at `x` times the cutoff for a resonant two-pole section of quality `q`
/// raised to the type's order.
pub fn filter_db(kind: Response, poles: u32, x: f64, q: f64) -> f64 {
    let d = ((1. - x * x).powi(2) + (x / q).powi(2)).sqrt();
    let gain = match kind {
        Response::LowPass => 1. / d,
        Response::HighPass => x * x / d,
        Response::BandPass => (x / q) / d,
        Response::Notch => (1. - x * x).abs() / d,
        _ => 1.,
    };
    (20. * gain.powf(f64::from(poles) / 2.).max(1e-6).log10()).clamp(-48., 24.)
}

fn draw_filter(f: &FilterShape, c: &Ctx) -> Drawn {
    let (Some(cutoff), Some(resonance)) = (f.cutoff, f.resonance) else { return Drawn::default() };
    let mut title = "Filter".to_string();
    let mut response = (Response::LowPass, 2);
    if let Some(k) = f.kind {
        let text = (c.text)(c.members[k]);
        title += &format!("  \u{b7}  {text}");
        response = filter_named(&text);
    }
    let area = plot(c);
    let (left, top, right, bottom) = area;
    let mut d = Drawn { title, ..Default::default() };
    d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: rect_path(left + 0.5, top + 0.5, right - left - 1., bottom - top - 1.) });
    if response.0 == Response::Unknown {
        d.labels.push(centered("Drawn by the plugin", area));
        return d;
    }
    // Across 20 Hz to 20 kHz on a log scale, the cutoff at its parameter's place, the top at +24 dB.
    let hz = 1000f64.powf((c.norm)(c.members[cutoff])) * 20.;
    let q = 0.707 + 8. * (c.norm)(c.members[resonance]);
    let n = (right - left) as usize;
    let points: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let f = 1000f64.powf(i as f64 / n as f64) * 20.;
            let db = filter_db(response.0, response.1, f / hz, q);
            (left + i as f32, top + ((24. - db) / 72.) as f32 * (bottom - top))
        })
        .collect();
    // The fill is the same points, closed down to the bottom under the first and the right edge.
    let mut fill = vec![(points[0].0, bottom)];
    fill.extend(&points);
    fill.push((right, bottom));
    d.shapes.push(Shape { ink: Ink::AccentWash, fill: true, width: 2, commands: poly(&fill) + " Z" });
    d.shapes.push(Shape { ink: Ink::Accent, fill: false, width: 2, commands: poly(&points) });
    d
}

/// A rectangle as path commands.
pub fn rect_path(x: f32, y: f32, w: f32, h: f32) -> String {
    poly(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)]) + " Z"
}

// The LFO (ParameterGrid::Lfo): its waveform over a few cycles (more when faster), after its delay and
// fading in, as its parameters set it. A preview: its tiles edit it.

/// An LFO recognized from its parameters' names: its wave and its speed, and a delay, fade-in, phase and
/// user steps when it has them. Indexes are places in the names given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LfoShape {
    pub wave: Option<usize>,
    pub speed: Option<usize>,
    pub delay: Option<usize>,
    pub fade: Option<usize>,
    pub phase: Option<usize>,
    pub steps: Vec<usize>,
}

impl LfoShape {
    pub fn empty(&self) -> bool {
        self.wave.is_none() || self.speed.is_none()
    }
    /// The parameters in the order their tiles go when linked.
    pub fn linked_order(&self) -> Vec<usize> {
        [self.wave, self.speed, self.delay, self.fade, self.phase].into_iter().flatten().collect()
    }
}

/// By name only (Wave, Type or Shape with Speed, Rate or Frequency), as the reference does.
pub fn lfo_of(names: &[&str]) -> LfoShape {
    let either = |options: &[&str]| options.iter().find_map(|o| names.iter().position(|n| n.eq_ignore_ascii_case(o)));
    let mut l = LfoShape {
        wave: either(&["Wave", "Waveform", "Type", "Shape"]),
        speed: either(&["Speed", "Rate", "Frequency"]),
        delay: either(&["Delay"]),
        fade: either(&["Fade In", "Attack"]),
        phase: either(&["Phase"]),
        steps: vec![],
    };
    for n in 1..=64 {
        match either(&[&format!("User Step {n}")]) {
            Some(i) => l.steps.push(i),
            None => break,
        }
    }
    l
}

/// The waveform a wave's value text names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    None,
    Sine,
    Triangle,
    Square,
    SawUp,
    SawDown,
    Random,
    Steps,
}

pub fn wave_named(text: &str) -> Wave {
    let t = text.to_lowercase();
    if t.contains("sin") {
        Wave::Sine
    } else if t.contains("tri") {
        Wave::Triangle
    } else if t.contains("square") || t.contains("pulse") {
        Wave::Square
    } else if t.contains("saw") {
        if t.contains("down") {
            Wave::SawDown
        } else {
            Wave::SawUp
        }
    } else if t.contains("sample") || t.contains("s&h") || t.contains("noise") || t.contains("random") {
        Wave::Random
    } else if t.contains("user") || t.contains("step") {
        Wave::Steps
    } else {
        Wave::None
    }
}

/// How an LFO runs across its plot: after the `delay` (0 to 1 of the plot) `cycles` of it with the `phase`
/// added (0 to 1 of a cycle), faded in over `fade` (0 to 1 of the plot).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfoRun {
    pub cycles: f64,
    pub phase: f64,
    pub delay: f64,
    pub fade: f64,
}

/// The wave at `x` (0 to 1 across the plot) from -1 to 1; `step` gives a user step's level (0 to 1) of `steps`.
pub fn lfo_at(kind: Wave, x: f64, run: LfoRun, step: &dyn Fn(usize) -> f64, steps: usize) -> f64 {
    let LfoRun { cycles, phase, delay, fade } = run;
    if x < delay {
        return 0.;
    }
    let t = ((x - delay) / (1. - delay) * cycles + phase) % 1.;
    let mut y = match kind {
        Wave::Sine => (t * std::f64::consts::TAU).sin(),
        Wave::Triangle => {
            if t < 0.25 {
                t * 4.
            } else if t < 0.75 {
                2. - t * 4.
            } else {
                t * 4. - 4.
            }
        }
        Wave::Square => {
            if t < 0.5 {
                1.
            } else {
                -1.
            }
        }
        Wave::SawUp => t * 2. - 1.,
        Wave::SawDown => 1. - t * 2.,
        Wave::Random => {
            // The same steps each time.
            let n = ((x - delay) / (1. - delay) * cycles * 2.) as i64;
            let v = (n as f64 * 12.9898).sin() * 43758.5453;
            (v - v.floor()) * 2. - 1.
        }
        Wave::Steps => step(((t * steps as f64) as usize).min(steps.saturating_sub(1))) * 2. - 1.,
        Wave::None => 0.,
    };
    if fade > 0. {
        y *= ((x - delay) / fade).min(1.);
    }
    y
}

fn draw_lfo(l: &LfoShape, c: &Ctx) -> Drawn {
    let (Some(wave), Some(_)) = (l.wave, l.speed) else { return Drawn::default() };
    let name = (c.text)(c.members[wave]);
    let area = plot(c);
    let (left, top, right, bottom) = area;
    let mut d = Drawn { title: format!("LFO  \u{b7}  {name}"), ..Default::default() };
    let middle = crisp((top + bottom) / 2.);
    d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: format!("M {left:.2} {middle:.2} L {right:.2} {middle:.2}") });
    let kind = wave_named(&name);
    if kind == Wave::None || (kind == Wave::Steps && l.steps.is_empty()) {
        d.labels.push(centered("Drawn by the plugin", area));
        return d;
    }
    let value = |i: Option<usize>| i.map_or(0., |i| (c.norm)(c.members[i]));
    let run = LfoRun { cycles: 1. + 4. * value(l.speed), phase: value(l.phase), delay: 0.25 * value(l.delay), fade: 0.25 * value(l.fade) };
    let step = |k: usize| (c.norm)(c.members[l.steps[k]]);
    let n = (right - left) as usize;
    let centre = (top + bottom) / 2.;
    let points: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let y = lfo_at(kind, i as f64 / n as f64, run, &step, l.steps.len());
            (left + i as f32, centre - y as f32 * (bottom - top) / 2.)
        })
        .collect();
    d.shapes.push(Shape { ink: Ink::Accent, fill: false, width: 2, commands: poly(&points) });
    d
}

/// "M x y L x y ..." through the points.
pub fn poly(points: &[(f32, f32)]) -> String {
    let mut s = String::new();
    for (k, (x, y)) in points.iter().enumerate() {
        s += &format!("{}{x:.2} {y:.2}", if k == 0 { "M " } else { " L " });
    }
    s
}

// The envelope (ParameterGrid::Envelope): the curve, filled under, a handle per point; dragging a point
// sets its time across and its level up and down.

fn envelope_params<'a>(c: &'a Ctx) -> Vec<&'a Param> {
    c.members.iter().map(|&i| &c.params[i]).collect()
}

fn draw_envelope(e: &Envelope, c: &Ctx) -> Drawn {
    let refs = envelope_params(c);
    let norm = |k: usize| (c.norm)(c.members[k]);
    let l = grid::envelope_layout(e, &refs, &norm, c.w, c.h, c.top);
    let handles = l
        .handles
        .iter()
        .zip(&e.points)
        .map(|(h, p)| Handle { at: h.at, params: [p.time, p.level].into_iter().flatten().map(|k| c.members[k]).collect() })
        .collect();
    Drawn {
        title: "Envelope".into(),
        shapes: vec![Shape { ink: Ink::AccentWash, fill: true, width: 2, commands: l.fill }, Shape { ink: Ink::Accent, fill: false, width: 2, commands: l.line }],
        handles,
        ..Default::default()
    }
}

fn envelope_drag(e: &Envelope, c: &Ctx, k: usize, at: (f32, f32)) -> Vec<(usize, f64)> {
    let refs = envelope_params(c);
    let norm = |j: usize| (c.norm)(c.members[j]);
    let l = grid::envelope_layout(e, &refs, &norm, c.w, c.h, c.top);
    let (Some(p), Some(h)) = (e.points.get(k), l.handles.get(k)) else { return vec![] };
    let mut out = Vec::new();
    if let Some(t) = p.time {
        let across = ((at.0 - h.from - l.least) / (l.segment - l.least)).clamp(0., 1.);
        out.push((c.members[t], refs[t].denorm(grid::stage_normal(refs[t], p.rate, l.longest, f64::from(across)))));
    }
    if let Some(v) = p.level {
        let up = ((l.bottom - at.1) / l.height).clamp(0., 1.);
        out.push((c.members[v], refs[v].denorm(f64::from(up))));
    }
    out
}
