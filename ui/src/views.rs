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
    // 1 to 6: muted, border, text, raised, the border faintly, background (slint/grid.slint `ViewPanel.ink`).
    /// The accent lightly: what a curve encloses.
    AccentWash = 7,
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
}

/// What a view reads: the grid's parameters and their values, the panel's size, and the parameter the
/// cursor is on (its node is marked).
pub struct Ctx<'a> {
    pub params: &'a [Param],
    /// The panel's parameters (indexes into `params`).
    pub members: &'a [usize],
    /// A parameter's value from 0 to 1.
    pub norm: &'a dyn Fn(usize) -> f64,
    pub focus: Option<usize>,
    pub w: f32,
    pub h: f32,
    /// Where the plot starts down the panel: under the title, with room for a handle's radius.
    pub top: f32,
}

/// A handle taken by touch, and what it set at the start.
#[derive(Clone, Debug, PartialEq)]
pub enum Held {
    /// An envelope's point.
    Node(usize),
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
    }
}

/// The handle under a touch at `at` (the panel's pixels), if one is within reach.
pub fn grab(view: &View, c: &Ctx, at: (f32, f32)) -> Option<Held> {
    match view {
        View::Envelope(e) => {
            let d = draw_envelope(e, c);
            nearest(&d.handles, at).map(Held::Node)
        }
    }
}

/// The parameters (indexes into the grid's) a held handle sets: their gestures open and close with the touch.
pub fn touched(view: &View, c: &Ctx, held: &Held) -> Vec<usize> {
    match (view, held) {
        (View::Envelope(e), Held::Node(k)) => draw_envelope(e, c).handles.get(*k).map(|h| h.params.clone()).unwrap_or_default(),
    }
}

/// What dragging a held handle to `at` sets: each parameter and its value.
pub fn drag(view: &View, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match (view, held) {
        (View::Envelope(e), Held::Node(k)) => envelope_drag(e, c, *k, at),
    }
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
