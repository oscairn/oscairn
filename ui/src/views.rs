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

/// The theme color a shape, label or box is drawn in (slint/grid.slint maps each to the theme): a number the
/// widget decodes, 0 to 12 the theme's colors by role, 16 to 39 the FM view's operator colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink(pub i32);

#[allow(non_upper_case_globals)]
impl Ink {
    pub const Accent: Ink = Ink(0);
    pub const Muted: Ink = Ink(1);
    pub const Border: Ink = Ink(2);
    pub const Text: Ink = Ink(3);
    pub const Raised: Ink = Ink(4);
    /// The border faintly: a plot's frame and baselines.
    pub const Frame: Ink = Ink(5);
    pub const Background: Ink = Ink(6);
    /// The accent lightly: what a curve encloses.
    pub const AccentWash: Ink = Ink(7);
    /// A key map's white keys.
    pub const Keys: Ink = Ink(8);
    /// The zone a key map is for, and the others.
    pub const Zone: Ink = Ink(9);
    pub const ZoneFaint: Ink = Ink(10);
    /// The cursor's color (the theme's `focus`).
    pub const Focus: Ink = Ink(12);
    /// Nothing: a box with no fill.
    pub const Clear: Ink = Ink(13);
    /// A mute's color when on, and a mark that is off (mute and solo, as Play's and the mixer's are drawn).
    pub const Danger: Ink = Ink(14);
    pub const MarkOff: Ink = Ink(15);

    /// The color of operator `n` (0 to 7: src/palette.rs), the text that reads on it, and the color dimmed.
    pub fn operator(n: usize) -> Ink {
        Ink(16 + n.min(7) as i32)
    }
    pub fn on_operator(n: usize) -> Ink {
        Ink(24 + n.min(7) as i32)
    }
    pub fn dim_operator(n: usize) -> Ink {
        Ink(32 + n.min(7) as i32)
    }
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
    /// Drawn with a mark's corners (the theme's mute and solo mark: square or round).
    pub mark: bool,
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
    /// The heading of a master and detail page's detail pane (ui/GRID.md "Master and detail").
    Detail(DetailHead),
    /// The selected operator's envelope in the detail, larger, over the rows of its stages' controls (the panel
    /// holds the graph and the rows' labels; the controls are tiles over it).
    Stages(StagesShape),
}

/// The detail's envelope panel: the FM view whose selected operator it shows, and the rows of controls under the
/// graph (their height, `grid::compact_height`, and the gap between them), where the graph is `graph` high (0: none).
#[derive(Clone, Debug, PartialEq)]
pub struct StagesShape {
    pub fm: FmShape,
    pub graph: f32,
    pub row: f32,
    pub gap: f32,
    /// The left edge of the rows' controls and of the graph's plot: the labels "Time" and "Level" are before it.
    pub label: f32,
    /// Whether the Time row and the Level row are there (the envelope has stages).
    pub rows: bool,
}

/// The detail pane's heading: a bar in the selected one's color with its name; when the detail has more rows of
/// tiles than it shows, which rows these are, and two arrows that page them one row at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct DetailHead {
    /// The color (0 to 7) of the one selected, as the overview draws it.
    pub color: usize,
    pub title: String,
    /// How many there are to select, which of them (0 to count - 1) is: the heading's selector steps the choice.
    pub count: usize,
    pub index: usize,
    /// Rows of tiles in all, the first one shown, and how many are.
    pub rows: usize,
    pub row: usize,
    pub per: usize,
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
    /// The theme's type scale and spacing (the FM view sizes its boxes and wires from them).
    pub scale: f32,
    pub spacing: f32,
    /// The FM view's selected operator (0 to 7): the one the cursor is on, else the last touched.
    pub selected: usize,
    /// The table cell the cursor is on, as (operator 0 to 7, column among `FmShape::columns`): `over` is then its row.
    pub cell: Option<(usize, usize)>,
    /// The dial has the parameter `focus` is, or the cell `cell` is on (a press, then the dial): its ring is the accent's.
    pub editing: bool,
    /// Which operators are soloed by the host (a solo the plugin has no field for: `fm::SoloFrom::Others`).
    pub soloed: &'a [bool],
    /// Where each operator's most recent voice is on its envelope, when the plugin says (`None`: no dot).
    pub positions: &'a [Option<Along>],
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
    /// A tap on an operator (its box, cell or line): selects it.
    Select(usize),
    /// An operator's ratio: dragged up or down, a step per 12 px.
    Ratio { param: usize, start: f64, from: (f32, f32) },
    /// One of the view's own parameters named at the top (the algorithm, the feedback): a tap on its left half steps it
    /// back, on its right half on (`mid` is the cell's middle).
    Nudge { param: usize, start: f64, from: (f32, f32), mid: f32 },
    /// The detail's rows paged by this many (a tap on its heading's arrows).
    Rows(i32),
    /// The selected operator stepped by this many (a tap on the heading's selector).
    Operator(i32),
    /// A tap on a table cell of operator `row` (0 to 7) in the column `col` of the table's: selects the operator.
    Cell { row: usize, col: usize },
    /// The same on a switched cell (mute, solo, mode): letting go on it switches it.
    Mark { row: usize, col: usize },
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
        View::Fm(f) => crate::fm::draw(f, c),
        View::Detail(h) => draw_detail(h, c),
        View::Stages(s) => crate::fm::draw_stages(s, c),
    }
}

/// The detail heading's height (a heading of the grid, a little less: it is a bar).
pub const DETAIL_HEAD: f32 = 26.;

/// A heading's arrow is a touch target wide (the heading is not as high as one).
fn arrow_width(c: &Ctx) -> f32 {
    c.h.max(grid::TOUCH)
}

/// The heading's row arrows: the (up, down) boxes at its right, each a touch target wide.
fn detail_arrows(c: &Ctx) -> (Area, Area) {
    let side = arrow_width(c);
    ((c.w - 2. * side, 0., c.w - side, c.h), (c.w - side, 0., c.w, c.h))
}

/// The heading's selector: the (previous, next) boxes at its left.
fn detail_selector(c: &Ctx) -> (Area, Area) {
    let side = arrow_width(c);
    ((0., 0., side, c.h), (side, 0., 2. * side, c.h))
}

fn draw_detail(h: &DetailHead, c: &Ctx) -> Drawn {
    let mut d = Drawn::default();
    let (fill, text) = (Ink::operator(h.color), Ink::on_operator(h.color));
    // A shape, not a box: shapes are drawn before boxes and labels, and the arrows are shapes too.
    d.shapes.push(Shape { ink: fill, fill: true, width: 0, commands: rect_path(1., 1., c.w - 2., c.h - 2.) });
    let small = 13. * c.scale;
    let pad = 8. * c.spacing;
    let mid = |a: Area| ((a.0 + a.2) / 2., (a.1 + a.3) / 2.);
    let r = (c.h * 0.22).max(3.);
    // The selector: previous and next, which the operator's number between them says (an arrow that cannot go
    // further is a hairline outline).
    let selector = h.count > 1;
    let left = if selector { 2. * arrow_width(c) } else { 0. };
    if selector {
        let (prev, next) = detail_selector(c);
        for (a, dir, on) in [(prev, -1., h.index > 0), (next, 1., h.index + 1 < h.count)] {
            let (x, y) = mid(a);
            let tip = (x + dir * r * 0.8, y);
            let base = x - dir * r * 0.8;
            let tri = poly(&[tip, (base, y - r), (base, y + r)]) + " Z";
            d.shapes.push(Shape { ink: text, fill: on, width: 1, commands: tri });
        }
    }
    let paged = h.rows > h.per;
    let right = if paged { c.w - 2. * arrow_width(c) - pad } else { c.w - pad };
    d.labels.push(Label { x: left + pad, y: 1., w: (right - left - pad).max(0.), h: c.h - 2., text: h.title.clone(), ink: text, bold: true, align: 0 });
    if paged {
        let shown = if h.per == 1 { format!("Row {} of {}", h.row + 1, h.rows) } else { format!("Rows {} to {} of {}", h.row + 1, h.row + h.per, h.rows) };
        let room = 0.6 * small * 1.06 * shown.chars().count() as f32;
        let from = (right - room).max(left + c.w * 0.3);
        d.labels.push(Label { x: from, y: 1., w: (right - from).max(0.), h: c.h - 2., text: shown, ink: text, bold: false, align: 2 });
        let (up, down) = detail_arrows(c);
        for (a, dir, on) in [(up, -1., h.row > 0), (down, 1., h.row + h.per < h.rows)] {
            let (x, y) = mid(a);
            let tip = (x, y + dir * r * 0.8);
            let base = y - dir * r * 0.8;
            let tri = poly(&[tip, (x - r, base), (x + r, base)]) + " Z";
            d.shapes.push(Shape { ink: text, fill: on, width: 1, commands: tri });
        }
    }
    d
}

/// The handle under a touch at `at` (the panel's pixels), if one is within reach: the views that are
/// previews (a filter's response, an LFO's wave) have none.
pub fn grab(view: &View, c: &Ctx, at: (f32, f32)) -> Option<Held> {
    match view {
        View::Envelope(_) | View::Map(..) => nearest(&draw(view, c).handles, at).map(Held::Node),
        View::Fm(f) => crate::fm::grab(f, c, at),
        View::Detail(h) => {
            let inside = |a: Area| at.0 >= a.0 && at.0 <= a.2 && at.1 >= a.1 && at.1 <= a.3;
            if h.rows > h.per {
                let (up, down) = detail_arrows(c);
                if inside(up) {
                    return Some(Held::Rows(-1));
                } else if inside(down) {
                    return Some(Held::Rows(1));
                }
            }
            if h.count > 1 {
                let (prev, next) = detail_selector(c);
                if inside(prev) {
                    return Some(Held::Operator(-1));
                } else if inside(next) {
                    return Some(Held::Operator(1));
                }
            }
            None
        }
        View::Stages(s) => crate::fm::grab_stages(s, c, at),
        View::Filter(_) | View::Lfo(_) => None,
    }
}

/// The parameters (indexes into the grid's) a held handle sets: their gestures open and close with the touch.
pub fn touched(view: &View, c: &Ctx, held: &Held) -> Vec<usize> {
    match held {
        Held::Node(k) => draw(view, c).handles.get(*k).map(|h| h.params.clone()).unwrap_or_default(),
        Held::Select(_) | Held::Rows(_) | Held::Operator(_) | Held::Cell { .. } | Held::Mark { .. } => vec![],
        Held::Algorithm { param, .. } | Held::Level { param } | Held::Ratio { param, .. } | Held::Nudge { param, .. } => vec![*param],
    }
}

/// What dragging a held handle to `at` sets: each parameter and its value.
pub fn drag(view: &View, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match (view, held) {
        (View::Envelope(e), Held::Node(k)) => envelope_drag(e, c, *k, at),
        (View::Map(m, index), Held::Node(k)) => map_drag(m, *index, c, *k, at),
        (View::Fm(f), held) => crate::fm::drag(f, c, held, at),
        (View::Stages(s), held) => crate::fm::drag_stages(s, c, held, at),
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
        (View::Fm(f), held) => crate::fm::release(f, c, held, at),
        _ => vec![],
    }
}

/// The operator an FM view's held handle belongs to (0 to 7), so a touch on it selects that operator.
pub fn operator_held(view: &View, c: &Ctx, held: &Held) -> Option<usize> {
    let f = match view {
        View::Fm(f) => f,
        View::Stages(s) => &s.fm,
        _ => return None,
    };
    match held {
        Held::Select(n) => Some(*n),
        Held::Cell { row, .. } | Held::Mark { row, .. } => Some(*row),
        Held::Level { param } | Held::Ratio { param, .. } => f.operator_of(*param),
        Held::Node(k) => draw(view, c).handles.get(*k).and_then(|h| h.params.first()).and_then(|&p| f.operator_of(p)),
        Held::Algorithm { .. } | Held::Nudge { .. } | Held::Rows(_) | Held::Operator(_) => None,
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
pub fn crisp(v: f32) -> f32 {
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

pub type Area = (f32, f32, f32, f32);

struct MapGeometry {
    plane: Area,
    keys: Area,
    velocity: Option<Area>,
    /// The handles: where, the parameter (the grid's), what dragging sets.
    handles: Vec<((f32, f32), usize, Pull)>,
}

pub fn width_of(a: Area) -> f32 {
    a.2 - a.0
}

pub fn height_of(a: Area) -> f32 {
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
pub fn inner_rect(a: Area, stroke: f32) -> String {
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

pub use crate::fm::{fm_of, Along, FmShape};

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
