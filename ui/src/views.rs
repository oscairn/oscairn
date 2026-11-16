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
    // 2 to 4: border, text, raised; 6: background (slint/grid.slint `ViewPanel.ink`).
    /// The border faintly: a plot's frame and baselines.
    Frame = 5,
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
    Filter(FilterShape),
    Lfo(LfoShape),
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
        View::Filter(f) => draw_filter(f, c),
        View::Lfo(l) => draw_lfo(l, c),
    }
}

/// The handle under a touch at `at` (the panel's pixels), if one is within reach: the views that are
/// previews (a filter's response, an LFO's wave) have none.
pub fn grab(view: &View, c: &Ctx, at: (f32, f32)) -> Option<Held> {
    match view {
        View::Envelope(_) => nearest(&draw(view, c).handles, at).map(Held::Node),
        View::Filter(_) | View::Lfo(_) => None,
    }
}

/// The parameters (indexes into the grid's) a held handle sets: their gestures open and close with the touch.
pub fn touched(view: &View, c: &Ctx, held: &Held) -> Vec<usize> {
    match held {
        Held::Node(k) => draw(view, c).handles.get(*k).map(|h| h.params.clone()).unwrap_or_default(),
    }
}

/// What dragging a held handle to `at` sets: each parameter and its value.
pub fn drag(view: &View, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match (view, held) {
        (View::Envelope(e), Held::Node(k)) => envelope_drag(e, c, *k, at),
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
