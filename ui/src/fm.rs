// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM operator view. Oscairn's own design (the owner's, 2026-11-15; ParameterGrid.cpp's `Operators` draws
//! the algorithm as a vertical stack with a column of controls per operator): three parts across the panel.
//!
//! - The algorithm as a horizontal signal flow, modulators at the left feeding rightward into the carriers,
//!   which meet the output line at the right edge. Each operator is a box in its own color with its number;
//!   each wire has the color of the operator it comes from; feedback is a loop that does not cross a box.
//! - Next to it, one plot of every operator's amplitude envelope, a line in the operator's color on common
//!   axes (stages left to right on one time scale, level up), scaled by the operator's output level so the
//!   picture shows how much each one contributes. The selected operator's line is thicker, on top, and the
//!   only one with handles.
//! - Under both, a cell per operator in its color: number, level bar, ratio, fine and detune.
//!
//! The layout is a function of the algorithm's data (`algorithm_layout`: layers, then the order in each
//! layer that keeps wires from crossing); the picture is one set of points (`geometry`) that draws, grabs
//! and drags. Dots that move along the lines come from `Ctx::positions` and are drawn only when given.

use crate::grid::{self, Param};
use crate::views::{height_of, inner_rect, poly, rect_path, width_of, Area, BoxDraw, Ctx, Drawn, Handle, Held, Ink, Label, Shape};

/// Where an operator's most recent voice is on its envelope line: a fraction of the line's length, or a
/// fraction of one of the line's segments (segment 0 runs from the start to the first node; a sustain's hold
/// is a segment of its own). Nothing builds one yet: the plugin's side is a proposal (see `Screen::set_positions`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Along {
    Path(f32),
    Segment(usize, f32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Operator {
    pub level: Option<usize>,
    pub ratio: Option<usize>,
    pub fixed: Option<usize>,
    /// The operator's envelope parameters (the grid's), in the plugin's order.
    pub envelope: Vec<usize>,
}

impl Operator {
    fn owns(&self, param: usize) -> bool {
        [self.level, self.ratio, self.fixed].contains(&Some(param)) || self.envelope.contains(&param)
    }
}

/// An FM operator view a plugin describes: the algorithm, feedback, and each operator's parameters
/// (operators 1 to 8, as the view's indexes). Indexes are the grid's parameters'.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FmShape {
    pub algorithm: Option<usize>,
    pub feedback: Option<usize>,
    /// The view's own parameters (index 0: the algorithm, the feedback, any other), in the plugin's order: named
    /// above the diagram, where the cursor and a touch reach them.
    pub own: Vec<usize>,
    /// Operator n at n - 1.
    pub operators: Vec<Operator>,
}

impl FmShape {
    /// The operator (0 to 7) a parameter belongs to.
    pub fn operator_of(&self, param: usize) -> Option<usize> {
        self.operators.iter().position(|o| o.owns(param))
    }
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
        if p.index == 0 {
            f.own.push(i);
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
            "fixed" => op.fixed = Some(i),
            field if grid::is_envelope(field) => op.envelope.push(i),
            _ => {}
        }
    }
    f
}

// ---- The algorithm as a flow ----

/// A place in the flow: an operator, or a point a long wire passes through in a column it skips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowNode {
    pub op: Option<usize>,
    /// 0 is the leftmost; the carriers are in the last.
    pub column: usize,
    /// In rows from the top, at least one apart within a column.
    pub y: f32,
}

/// One wire as the nodes it passes: from operator `source` to operator `target`, or to the output line.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub source: usize,
    pub target: Option<usize>,
    pub nodes: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flow {
    pub columns: usize,
    /// The operators first (operator n at n - 1), then the pass-through points.
    pub nodes: Vec<FlowNode>,
    pub chains: Vec<Chain>,
    /// Wires that run backward (modulation inside a cycle) and the routing's feedback: (from, to) operators,
    /// drawn as loops.
    pub loops: Vec<(usize, usize)>,
    /// The rows the flow spans.
    pub rows: f32,
}

/// The flow of the first `count` (at most 8) operators of `routing` (eight operators' input masks, the
/// carriers' mask, the feedback's operator from and to). An operator's column is its distance from the
/// output: the carriers are in the last, what modulates a carrier one column left, and so on (a cycle is cut
/// after eight passes and its backward wires are loops). A wire that skips a column passes through a point
/// in it, which keeps its row clear of boxes. Rows are found by repeated sweeps that put each node at the
/// mean row of what it is wired to, which keeps wires from crossing where the routing allows it.
pub fn algorithm_layout(routing: &[u8; 11], count: usize) -> Flow {
    let count = count.min(8);
    let modulates = |m: usize, t: usize| m != t && routing[t] & (1 << m) != 0;
    let mut layer = vec![-1i32; count];
    for (n, l) in layer.iter_mut().enumerate() {
        if routing[8] & (1 << n) != 0 {
            *l = 0;
        }
    }
    let spread = |layer: &mut Vec<i32>| {
        for _ in 0..8 {
            for t in 0..count {
                for m in 0..count {
                    if layer[t] >= 0 && modulates(m, t) && layer[m] < layer[t] + 1 {
                        layer[m] = (layer[t] + 1).min(7);
                    }
                }
            }
        }
    };
    spread(&mut layer);
    // What no path reaches the output from stands where a carrier does, with its own modulators before it.
    for l in &mut layer {
        *l = (*l).max(0);
    }
    spread(&mut layer);
    let mut distinct = layer.clone();
    distinct.sort_unstable();
    distinct.dedup();
    let columns = distinct.len();
    let column = |n: usize| columns - 1 - distinct.iter().position(|&l| l == layer[n]).unwrap_or(0);
    let mut flow = Flow { columns, ..Default::default() };
    for n in 0..count {
        flow.nodes.push(FlowNode { op: Some(n), column: column(n), y: 0. });
    }
    let pass = |flow: &mut Flow, from: usize, to_column: usize| -> Vec<usize> {
        let mut nodes = vec![from];
        for c in flow.nodes[from].column + 1..to_column {
            flow.nodes.push(FlowNode { op: None, column: c, y: 0. });
            nodes.push(flow.nodes.len() - 1);
        }
        nodes
    };
    for t in 0..count {
        for m in 0..count {
            if !modulates(m, t) {
                continue;
            }
            if layer[m] > layer[t] {
                let mut nodes = pass(&mut flow, m, column(t));
                nodes.push(t);
                flow.chains.push(Chain { source: m, target: Some(t), nodes });
            } else {
                flow.loops.push((m, t));
            }
        }
    }
    for n in 0..count {
        if routing[8] & (1 << n) != 0 {
            let nodes = pass(&mut flow, n, columns);
            flow.chains.push(Chain { source: n, target: None, nodes });
        }
    }
    let (from, to) = (usize::from(routing[9]), usize::from(routing[10]));
    if (1..=count).contains(&from) && (1..=count).contains(&to) {
        flow.loops.push((from - 1, to - 1));
    }
    sweep(&mut flow);
    flow
}

/// Puts the nodes of a column at least a row apart, each as near the mean row of its neighbors (`want`)
/// as that allows, the column shifted to keep the mean of the wanted rows.
fn settle(nodes: &mut [FlowNode], column: &[usize], want: &dyn Fn(&[FlowNode], usize) -> Option<f32>) {
    if column.is_empty() {
        return;
    }
    let mut order: Vec<(f32, f32, usize)> = column.iter().map(|&i| (want(nodes, i).unwrap_or(nodes[i].y), nodes[i].y, i)).collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut placed = Vec::with_capacity(order.len());
    let mut previous = f32::MIN / 2.;
    for (wanted, _, _) in &order {
        let y = wanted.max(previous + 1.);
        placed.push(y);
        previous = y;
    }
    let shift = order.iter().zip(&placed).map(|((wanted, _, _), y)| wanted - y).sum::<f32>() / order.len() as f32;
    for ((_, _, i), y) in order.iter().zip(placed) {
        nodes[*i].y = y + shift;
    }
}

fn sweep(flow: &mut Flow) {
    let mut by_column: Vec<Vec<usize>> = vec![vec![]; flow.columns];
    for (i, n) in flow.nodes.iter().enumerate() {
        by_column[n.column].push(i);
    }
    for column in &by_column {
        for (k, &i) in column.iter().enumerate() {
            flow.nodes[i].y = k as f32;
        }
    }
    let (mut next, mut before) = (vec![vec![]; flow.nodes.len()], vec![vec![]; flow.nodes.len()]);
    for chain in &flow.chains {
        for pair in chain.nodes.windows(2) {
            next[pair[0]].push(pair[1]);
            before[pair[1]].push(pair[0]);
        }
    }
    let mean = |neighbors: &Vec<Vec<usize>>| {
        let neighbors = neighbors.clone();
        move |nodes: &[FlowNode], i: usize| {
            let list = &neighbors[i];
            (!list.is_empty()).then(|| list.iter().map(|&j| nodes[j].y).sum::<f32>() / list.len() as f32)
        }
    };
    let (toward_output, toward_input) = (mean(&next), mean(&before));
    for _ in 0..8 {
        for c in (0..flow.columns.saturating_sub(1)).rev() {
            settle(&mut flow.nodes, &by_column[c], &toward_output);
        }
        for column in by_column.iter().skip(1) {
            settle(&mut flow.nodes, column, &toward_input);
        }
    }
    let low = flow.nodes.iter().map(|n| n.y).fold(f32::MAX, f32::min);
    for n in &mut flow.nodes {
        n.y -= low;
    }
    flow.rows = flow.nodes.iter().map(|n| n.y).fold(0., f32::max) + 1.;
}

#[cfg(test)]
impl Flow {
    /// How many pairs of wire segments between neighboring columns cross (wires from one node that part
    /// do not count).
    pub fn crossings(&self) -> usize {
        let mut segments: Vec<(usize, usize, usize)> = vec![];
        for (k, chain) in self.chains.iter().enumerate() {
            for pair in chain.nodes.windows(2) {
                segments.push((k, pair[0], pair[1]));
            }
        }
        let mut count = 0;
        for (i, a) in segments.iter().enumerate() {
            for b in &segments[i + 1..] {
                let (na, nb) = ((&self.nodes[a.1], &self.nodes[a.2]), (&self.nodes[b.1], &self.nodes[b.2]));
                if na.0.column != nb.0.column || a.1 == b.1 || a.2 == b.2 {
                    continue;
                }
                if (na.0.y - nb.0.y) * (na.1.y - nb.1.y) < 0. {
                    count += 1;
                }
            }
        }
        count
    }
}

// ---- The picture ----

/// A wire in pixels: the operator it comes from, the points it runs through, whether an arrow ends it.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub source: usize,
    pub points: Vec<(f32, f32)>,
    pub arrow: bool,
}

/// The algorithm drawn in an area: the operators' boxes (by operator), the wires, feedback loops, and the
/// output line (x, top, bottom) the carriers' wires end on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagram {
    pub boxes: Vec<Area>,
    pub wires: Vec<Wire>,
    pub loops: Vec<Wire>,
    pub bus: Option<(f32, f32, f32)>,
    pub out_label: Area,
}

/// Lays `flow` out in `area` with boxes of at most `box_w` by `box_h` (theme sizes); `labeled`: room for the
/// output's label under the diagram.
pub fn diagram(flow: &Flow, count: usize, area: Area, small: f32, routed: bool, centered: bool) -> Diagram {
    let mut d = Diagram { boxes: vec![(0., 0., 0., 0.); count], ..Default::default() };
    if count == 0 {
        return d;
    }
    let columns = flow.columns.max(1);
    let lane = (small * 0.35).max(3.);
    let gutter = (small * 0.8).max(8.);
    let half = (small * 1.4).max(16.); // half the output label's width: the line is that far from the area's edge
    let final_room = if routed { half + 12. } else { 0. };
    // The lanes: in the gutter right of column g, one for each node a wire leaves toward another row (and
    // one for each wire that joins another at its target).
    let rows_of = |i: usize| flow.nodes[i].y;
    let mut inputs = vec![0usize; flow.nodes.len()];
    for chain in flow.chains.iter().filter(|c| c.target.is_some()) {
        if let Some(&last) = chain.nodes.last() {
            inputs[last] += 1;
        }
    }
    let mut lanes: Vec<Vec<usize>> = vec![vec![]; columns];
    for chain in &flow.chains {
        for pair in chain.nodes.windows(2) {
            let moves = (rows_of(pair[0]) - rows_of(pair[1])).abs() > 0.01 || (flow.nodes[pair[1]].op.is_some() && inputs[pair[1]] > 1);
            let g = flow.nodes[pair[0]].column;
            if moves && !lanes[g].contains(&pair[0]) {
                lanes[g].push(pair[0]);
            }
        }
    }
    for l in &mut lanes {
        l.sort_by(|&a, &b| rows_of(a).total_cmp(&rows_of(b)));
    }
    let loops_up = flow.loops.iter().filter(|(a, b)| a != b).count();
    let mut widths: Vec<f32> = (0..columns.saturating_sub(1)).map(|g| gutter + lanes[g].len() as f32 * lane).collect();
    let margin = 6.;
    let preferred = (small * 2.2).max(24.);
    let room = width_of(area) - margin - final_room;
    let fixed: f32 = widths.iter().sum();
    let mut bw = ((room - fixed) / columns as f32).min(preferred);
    if bw < 16. {
        // Too many columns for the lanes: the gutters give way, to a third at the least.
        bw = 16.;
        let shrink = ((room - 16. * columns as f32) / fixed.max(1.)).clamp(0.34, 1.);
        for w in &mut widths {
            *w *= shrink;
        }
    } else if bw >= preferred {
        // Room left over goes to the gutters, up to twice a box.
        let slack = room - columns as f32 * bw - fixed;
        if slack > 0. && columns > 1 {
            let add = (slack / (columns - 1) as f32).min(2. * bw);
            for w in &mut widths {
                *w += add;
            }
        }
    }
    let mut left = vec![0f32; columns];
    let mut right = vec![0f32; columns];
    let mut x = area.0 + margin;
    for c in 0..columns {
        left[c] = x;
        right[c] = x + bw;
        x = right[c] + widths.get(c).copied().unwrap_or(0.);
    }
    if centered {
        // Alone in the panel's width (no graph beside it): in the middle of it.
        let shift = ((area.2 - area.0) - (right[columns - 1] + final_room - area.0)).max(0.) / 2.;
        for c in 0..columns {
            left[c] += shift;
            right[c] += shift;
        }
    }
    let label_h = if routed { small * 1.2 } else { 0. };
    let top_margin = 4. + loops_up as f32 * lane;
    let (content_top, content_bottom) = (area.1 + top_margin, area.3 - label_h);
    let rows = flow.rows.max(1.);
    let full = (content_bottom - content_top) / rows;
    let preferred_h = (small * 1.9).max(22.);
    let pitch = full.min(preferred_h + 16.);
    let box_h = (pitch - 6.).clamp(12., preferred_h).min(pitch - 1.);
    let gap = pitch - box_h;
    let offset = content_top + (full * rows - pitch * rows) / 2.;
    let y_of = |i: usize| offset + (flow.nodes[i].y + 0.5) * pitch;
    for n in 0..count {
        let node = &flow.nodes[n];
        d.boxes[n] = (left[node.column], y_of(n) - box_h / 2., right[node.column], y_of(n) + box_h / 2.);
    }
    let bus_x = if centered { right[columns - 1] + final_room - half } else { area.2 - half };
    // Where each wire arrives at its box: spread along the box's left edge in the order of where they come from.
    let mut entry: Vec<Option<f32>> = vec![None; flow.chains.len()];
    for n in 0..count {
        let mut arriving: Vec<(f32, usize)> = flow.chains.iter().enumerate().filter(|(_, c)| c.target == Some(n)).map(|(k, c)| (rows_of(c.nodes[c.nodes.len() - 2]), k)).collect();
        arriving.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let b = d.boxes[n];
        for (i, (_, k)) in arriving.iter().enumerate() {
            entry[*k] = Some(b.1 + height_of(b) * (i + 1) as f32 / (arriving.len() + 1) as f32);
        }
    }
    let lane_x = |g: usize, a: usize| right[g] + gutter / 2. + lane * lanes[g].iter().position(|&x| x == a).unwrap_or(0) as f32 + lane / 2.;
    let mut stubs = Vec::new();
    for (k, chain) in flow.chains.iter().enumerate() {
        let mut points: Vec<(f32, f32)> = vec![];
        let count_nodes = chain.nodes.len();
        for (s, pair) in chain.nodes.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let (xa, ya) = (right[flow.nodes[a].column], y_of(a));
            let (xb, mut yb) = (left[flow.nodes[b].column], y_of(b));
            if s + 2 == count_nodes {
                yb = entry[k].unwrap_or(yb);
            }
            points.push((xa, ya));
            if (ya - yb).abs() > 0.01 {
                let xl = lane_x(flow.nodes[a].column, a);
                points.extend([(xl, ya), (xl, yb)]);
            }
            points.push((xb, yb));
        }
        if chain.target.is_none() {
            let last = *chain.nodes.last().unwrap_or(&0);
            let (x, y) = (right[flow.nodes[last].column], y_of(last));
            points.extend([(x, y), (bus_x, y)]);
            stubs.push(y);
        }
        points.dedup_by(|a, b| (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01);
        if routed {
            d.wires.push(Wire { source: chain.source, points, arrow: chain.target.is_some() });
        }
    }
    if routed && !stubs.is_empty() {
        let low = stubs.iter().copied().fold(f32::MAX, f32::min) - box_h / 2.;
        let high = stubs.iter().copied().fold(f32::MIN, f32::max) + box_h / 2.;
        d.bus = Some((bus_x, low, high));
        d.out_label = (bus_x - half, high + 1., bus_x + half, high + 1. + label_h);
    }
    // Loops go out of a box's top into the gap above its row, along the gap to the box's side, up the side to
    // a lane above every row, across, and down the side of the box they enter: nothing in it but wires.
    let mut up = 0;
    for &(from, to) in &flow.loops {
        let (fb, tb) = (d.boxes[from], d.boxes[to]);
        let (fx, tx) = ((fb.0 + fb.2) / 2., (tb.0 + tb.2) / 2.);
        let points = if from == to {
            let y = fb.1 - gap * 0.6;
            vec![(fx - width_of(fb) / 4., fb.1), (fx - width_of(fb) / 4., y), (fx + width_of(fb) / 4., y), (fx + width_of(fb) / 4., fb.1)]
        } else {
            let (fc, tc) = (flow.nodes[from].column, flow.nodes[to].column);
            let (yf, yt) = (fb.1 - gap / 2., tb.1 - gap / 2.);
            let x1 = if tc >= fc { fb.2 + 3. } else { fb.0 - 3. };
            if fc == tc {
                vec![(fx, fb.1), (fx, yf), (x1, yf), (x1, yt), (tx, yt), (tx, tb.1)]
            } else {
                let x2 = if tc > fc { tb.0 - 3. } else { tb.2 + 3. };
                let lane_y = area.1 + 2. + lane * up as f32;
                up += 1;
                vec![(fx, fb.1), (fx, yf), (x1, yf), (x1, lane_y), (x2, lane_y), (x2, yt), (tx, yt), (tx, tb.1)]
            }
        };
        d.loops.push(Wire { source: from, points, arrow: true });
    }
    d
}

/// One line of the overview's list: an operator's number, its level (a bar under its text) and its ratio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub whole: Area,
    pub chip: Area,
    pub level: Area,
    pub ratio: Area,
}

/// An operator's envelope line in the plot.
struct Line {
    envelope: grid::Envelope,
    layout: grid::EnvLayout,
    gain: f32,
}

/// One of the view's own parameters, named above the diagram: its place and its text.
struct Head {
    area: Area,
    param: usize,
    text: String,
}

struct Geometry {
    diagram: Area,
    graph: Option<Area>,
    plot: Area,
    picture: Diagram,
    cells: Vec<Cell>,
    heads: Vec<Head>,
    lines: Vec<Option<Line>>,
    selected: usize,
    tag: (f32, f32),
    small: f32,
}

fn contains(a: Area, at: (f32, f32), grow: f32) -> bool {
    at.0 >= a.0 - grow && at.0 <= a.2 + grow && at.1 >= a.1 - grow && at.1 <= a.3 + grow
}

/// The measures of the overview that its height and the detail's are worked out from, all from the theme's type
/// size and spacing: where the body starts under the heads, the least a list line is as high as, and the gap
/// between lines.
struct Metrics {
    small: f32,
    sp: f32,
    body_top: f32,
    row_min: f32,
    gap: f32,
    foot: f32,
}

fn metrics(scale: f32, spacing: f32) -> Metrics {
    let small = 13. * scale;
    let sp = spacing;
    Metrics { small, sp, body_top: 6. * sp + small * 1.3 + 2. * sp, row_min: small * 1.1, gap: 3. * sp, foot: 8. * sp }
}

/// The least the overview is high and still draws `count` operators as lines of the list each its least high.
pub fn least_height(count: usize, scale: f32, spacing: f32) -> f32 {
    let m = metrics(scale, spacing);
    m.body_top + count as f32 * m.row_min + count.saturating_sub(1) as f32 * m.gap + m.foot
}

/// What the overview is given when the detail can spare it: lines of 1.6 lines of type, and at least 14 lines of type
/// in all, which keeps the diagram's boxes and the graph's tags legible.
pub fn preferred_height(count: usize, scale: f32, spacing: f32) -> f32 {
    let m = metrics(scale, spacing);
    (m.body_top + count as f32 * m.small * 1.6 + count.saturating_sub(1) as f32 * m.gap + m.foot).max(m.small * 14.)
}

/// The algorithm's routing at its value, when the plugin gives one.
fn route_of(f: &FmShape, c: &Ctx) -> Option<[u8; 11]> {
    let a = &c.params[f.algorithm?];
    let at = ((c.norm)(f.algorithm?) * (a.max - a.min)).round() as usize;
    a.algorithms.get(at).copied()
}

/// The estimated width of a bold text at the small type (0.6 em a character).
fn bold_width(text: &str, small: f32) -> f32 {
    text.chars().count() as f32 * 0.6 * small * 1.06
}

fn geometry(f: &FmShape, c: &Ctx) -> Geometry {
    let m = metrics(c.scale, c.spacing);
    let (small, sp) = (m.small, m.sp);
    let pad = 12. * sp;
    let count = f.operators.len();
    let has_graph = f.operators.iter().any(|o| !grid::envelope_of_fields(&o.envelope.iter().map(|&i| c.params[i].field.as_str()).collect::<Vec<_>>()).points.is_empty());
    let (top, bottom) = (m.body_top, c.h - m.foot);
    let (left, right) = (pad, c.w - pad);
    // The list of operators at the right: a line each, as high as the room gives (at most 2.3 lines of type for few).
    let row_max = small * if count <= 6 { 2.3 } else { 1.9 };
    let row = if count > 0 { ((bottom - top - m.gap * (count - 1) as f32) / count as f32).clamp(m.row_min, row_max) } else { m.row_min };
    let list_w = (((right - left) * 0.22).max(small * 11.5)).min(small * 15.).min(right - left);
    let list_left = right - list_w;
    let across = list_left - 12. * sp - left;
    let diagram_right = if has_graph { left + (across - 12. * sp) * 0.46 } else { left + across };
    let diagram_area = (left, top, diagram_right, bottom);
    let graph = has_graph.then_some((diagram_right + 12. * sp, top, left + across, bottom));
    let tag = (small * 1.3, small * 1.2);
    let plot = graph.map_or((0., 0., 0., 0.), |g| (g.0 + 10. * sp, g.1 + grid::NODE_RADIUS + 4., g.2 - tag.0 - grid::NODE_RADIUS - 6., g.3 - 8.));
    let route = route_of(f, c);
    let flow = (count > 0).then(|| match &route {
        Some(r) => algorithm_layout(r, count),
        None => {
            // No routing in the data: the operators in a column, nothing to say how they connect.
            let mut flow = Flow { columns: 1, rows: count as f32, ..Default::default() };
            flow.nodes = (0..count).map(|n| FlowNode { op: Some(n), column: 0, y: n as f32 }).collect();
            flow
        }
    });
    let picture = flow.as_ref().map(|fl| diagram(fl, count, diagram_area, small, route.is_some(), graph.is_none())).unwrap_or_default();
    // The heads: the view's own parameters named in a line over the diagram, each as wide as its text.
    let mut heads = Vec::new();
    let mut x = left;
    for &p in &f.own {
        let text = format!("{} {}", c.params[p].name, (c.text)(p));
        let w = bold_width(&text, small).min((list_left - 12. * sp - x).max(0.));
        heads.push(Head { area: (x, 6. * sp, x + w, 6. * sp + small * 1.3), param: p, text });
        x += w + 12. * sp;
    }
    // The cells: chip, level (text over a bar) and ratio on one line.
    let cell_gap = 3. * sp;
    let ratio_w = ((list_w - row) * 0.42).max(small * 3.);
    let level_w = (list_w - row - ratio_w - 2. * cell_gap).max(0.);
    let cells = (0..count)
        .map(|n| {
            let y = top + (row + m.gap) * n as f32;
            let chip = (list_left, y, list_left + row, y + row);
            let level = (chip.2 + cell_gap, y, chip.2 + cell_gap + level_w, y + row);
            let ratio = (level.2 + cell_gap, y, right, y + row);
            Cell { whole: (list_left - 1., y - 1., right + 1., y + row + 1.), chip, level, ratio }
        })
        .collect();
    // The lines: every operator's envelope in the plot, on one time scale, scaled by its level.
    let mut lines: Vec<Option<Line>> = (0..count).map(|_| None).collect();
    if has_graph {
        let shapes: Vec<(grid::Envelope, Vec<&Param>)> = f
            .operators
            .iter()
            .map(|o| {
                let fields: Vec<&str> = o.envelope.iter().map(|&i| c.params[i].field.as_str()).collect();
                (grid::envelope_of_fields(&fields), o.envelope.iter().map(|&i| &c.params[i]).collect())
            })
            .collect();
        let longest = shapes
            .iter()
            .map(|(e, refs)| grid::longest_stage(&e.points.iter().filter_map(|p| p.time).map(|t| refs[t]).collect::<Vec<_>>()))
            .fold(0., f64::max);
        for (n, ((e, refs), o)) in shapes.into_iter().zip(&f.operators).enumerate() {
            if e.points.is_empty() {
                continue;
            }
            let norm = |k: usize| (c.norm)(o.envelope[k]);
            let gain = o.level.map_or(1., |l| (c.norm)(l) as f32);
            let layout = grid::envelope_layout_in(&e, &refs, &norm, plot, gain, longest);
            lines[n] = Some(Line { envelope: e, layout, gain });
        }
    }
    Geometry { diagram: diagram_area, graph, plot, picture, cells, heads, lines, selected: c.selected.min(count.saturating_sub(1)), tag, small }
}

fn dimmed(f: &FmShape, c: &Ctx, n: usize) -> bool {
    f.operators[n].level.is_some_and(|l| (c.norm)(l) <= 0.)
}

/// The point `along` a line's vertices.
pub fn point_along(points: &[(f32, f32)], along: Along) -> Option<(f32, f32)> {
    let lengths: Vec<f32> = points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).collect();
    let (segment, fraction) = match along {
        Along::Segment(k, t) => (k.min(lengths.len().checked_sub(1)?), t.clamp(0., 1.)),
        Along::Path(t) => {
            let total: f32 = lengths.iter().sum();
            let mut left = t.clamp(0., 1.) * total;
            let mut found = (lengths.len().checked_sub(1)?, 1.);
            for (k, l) in lengths.iter().enumerate() {
                if left <= *l || k + 1 == lengths.len() {
                    found = (k, if *l > 0. { (left / l).min(1.) } else { 0. });
                    break;
                }
                left -= l;
            }
            found
        }
    };
    let (a, b) = (points[segment], points[segment + 1]);
    Some((a.0 + (b.0 - a.0) * fraction, a.1 + (b.1 - a.1) * fraction))
}

fn circle(x: f32, y: f32, r: f32) -> String {
    format!("M {:.2} {y:.2} A {r:.2} {r:.2} 0 1 0 {:.2} {y:.2} A {r:.2} {r:.2} 0 1 0 {:.2} {y:.2} Z", x - r, x + r, x - r)
}

/// The ratio box's text: the plugin's, with the fixed-frequency switch's when it is on and the box is wide
/// enough for both (at 70% type, an estimate of 0.6 em a character); else the ratio's alone and the box is
/// edged in the accent, which says the same.
fn ratio_text(o: &Operator, c: &Ctx, room: f32) -> (String, bool) {
    let ratio = o.ratio.map(|r| (c.text)(r)).unwrap_or_default();
    match o.fixed.filter(|&x| (c.norm)(x) > 0.5) {
        Some(fixed) => {
            let both = format!("{} {ratio}", (c.text)(fixed));
            if both.chars().count() as f32 * 0.6 * 13. * c.scale * 0.7 <= room - 2. {
                (both, true)
            } else {
                (ratio, true)
            }
        }
        None => (ratio, false),
    }
}

fn arrow(tip: (f32, f32), toward: (f32, f32), size: f32) -> String {
    let (dx, dy) = (tip.0 - toward.0, tip.1 - toward.1);
    let len = dx.hypot(dy).max(0.001);
    let (ux, uy) = (dx / len, dy / len);
    let base = (tip.0 - ux * size * 1.6, tip.1 - uy * size * 1.6);
    poly(&[tip, (base.0 - uy * size, base.1 + ux * size), (base.0 + uy * size, base.1 - ux * size)]) + " Z"
}

fn ring(a: Area, grow: f32) -> String {
    rect_path(a.0 - grow, a.1 - grow, width_of(a) + 2. * grow, height_of(a) + 2. * grow)
}

pub fn draw(f: &FmShape, c: &Ctx) -> Drawn {
    let g = geometry(f, c);
    let mut d = Drawn::default();
    let n_ops = f.operators.len();
    if n_ops == 0 {
        return d;
    }
    let ink_of = |n: usize| if dimmed(f, c, n) { Ink::dim_operator(n) } else { Ink::operator(n) };
    // The diagram: wires by operator, the selected one's last and thicker; loops; the output line.
    let order: Vec<usize> = (0..n_ops).filter(|&n| n != g.selected).chain([g.selected]).collect();
    let arrow_size = (g.small * 0.22).max(2.5);
    for &n in &order {
        let mut wires = String::new();
        let mut heads = String::new();
        for w in g.picture.wires.iter().chain(&g.picture.loops).filter(|w| w.source == n) {
            wires += &(poly(&w.points) + " ");
            if w.arrow && w.points.len() >= 2 {
                heads += &(arrow(w.points[w.points.len() - 1], w.points[w.points.len() - 2], arrow_size) + " ");
            }
        }
        if !wires.is_empty() {
            d.shapes.push(Shape { ink: ink_of(n), fill: false, width: if n == g.selected { 2 } else { 1 }, commands: wires });
            d.shapes.push(Shape { ink: ink_of(n), fill: true, width: 0, commands: heads });
        }
    }
    if let Some((x, low, high)) = g.picture.bus {
        d.shapes.push(Shape { ink: Ink::Muted, fill: false, width: 1, commands: poly(&[(crisp(x), low), (crisp(x), high)]) });
        let o = g.picture.out_label;
        d.labels.push(Label { x: o.0, y: o.1, w: width_of(o), h: height_of(o), text: "Out".into(), ink: Ink::Muted, bold: false, align: 1 });
    }
    // The selected operator's ring, around its box in the diagram and its chip in the strip.
    if g.selected < g.picture.boxes.len() {
        d.shapes.push(Shape { ink: Ink::Text, fill: false, width: 1, commands: ring(g.picture.boxes[g.selected], 2.5) });
    }
    for (n, b) in g.picture.boxes.iter().enumerate() {
        let (fill, text) = if dimmed(f, c, n) { (Ink::dim_operator(n), Ink::Text) } else { (Ink::operator(n), Ink::on_operator(n)) };
        d.boxes.push(BoxDraw { x: b.0, y: b.1, w: width_of(*b), h: height_of(*b), fill, edge: Some(Ink::Border), text: (n + 1).to_string(), text_ink: text, bold: true });
    }
    // The graph: a frame, every operator's line (the dimmed first, the selected last), a tag at each end.
    if let Some(frame) = g.graph {
        d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: inner_rect(frame, 1.) });
        let mut tags: Vec<(usize, f32, f32)> = vec![];
        for &n in dim_first(f, c, &order).iter() {
            let Some(line) = &g.lines[n] else { continue };
            d.shapes.push(Shape { ink: ink_of(n), fill: false, width: if n == g.selected { 2 } else { 1 }, commands: line.layout.line.clone() });
            if let Some(&(x, y)) = line.layout.points.last() {
                tags.push((n, x, y));
            }
        }
        // A tag above and right of its line's end (a line along the plot's foot, an operator at level 0, then passes under none), moved up past any already there.
        tags.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut placed: Vec<Area> = vec![];
        for (n, x, y) in tags {
            let (w, h) = g.tag;
            let mut top = y - h - 1.;
            while placed.iter().any(|p| x + 3. + grid::NODE_RADIUS < p.2 && x + 3. + grid::NODE_RADIUS + w > p.0 && top < p.3 && top + h > p.1) && top - h >= g.plot.1 - h {
                top -= h;
            }
            placed.push((x + 3. + grid::NODE_RADIUS, top, x + 3. + grid::NODE_RADIUS + w, top + h));
            d.labels.push(Label { x: x + 3. + grid::NODE_RADIUS, y: top, w, h, text: (n + 1).to_string(), ink: ink_of(n), bold: true, align: 0 });
        }
        // Dots: where each operator's most recent voice is on its line, a ring in the background under a dot in the operator's color.
        let radius = (g.small * 0.28).max(3.);
        for n in 0..n_ops {
            let (Some(line), Some(along)) = (&g.lines[n], c.positions.get(n).copied().flatten()) else { continue };
            if let Some((x, y)) = point_along(&line.layout.points, along) {
                d.shapes.push(Shape { ink: Ink::Background, fill: true, width: 0, commands: circle(x, y, radius + 1.5) });
                d.shapes.push(Shape { ink: Ink::operator(n), fill: true, width: 0, commands: circle(x, y, radius) });
            }
        }
        // The selected operator's envelope nodes are the handles.
        if let (Some(line), Some(o)) = (&g.lines[g.selected], f.operators.get(g.selected)) {
            for (h, p) in line.layout.handles.iter().zip(&line.envelope.points) {
                d.handles.push(Handle { at: h.at, params: [p.time, p.level].into_iter().flatten().map(|k| o.envelope[k]).collect() });
            }
        }
    }
    // The heads: the view's own parameters, the cursor's ring round the one it is on (the accent's while the dial has it).
    for h in &g.heads {
        d.labels.push(Label { x: h.area.0, y: h.area.1, w: width_of(h.area), h: height_of(h.area), text: h.text.clone(), ink: Ink::Muted, bold: true, align: 0 });
        if c.focus == Some(h.param) {
            let ink = if c.editing { Ink::Accent } else { Ink::Focus };
            d.shapes.push(Shape { ink, fill: false, width: 2, commands: inner_rect((h.area.0 - 3., h.area.1, h.area.2 + 3., h.area.3), 2.) });
        }
    }
    // The list: a line for each operator.
    let bar_h = (3. * c.spacing).max(2.);
    for (n, (cell, o)) in g.cells.iter().zip(&f.operators).enumerate() {
        let chosen = n == g.selected;
        d.boxes.push(BoxDraw { x: cell.whole.0, y: cell.whole.1, w: width_of(cell.whole), h: height_of(cell.whole), fill: if chosen { Ink::Raised } else { Ink::Clear }, edge: Some(if chosen { Ink::Text } else { Ink::Frame }), text: String::new(), text_ink: Ink::Text, bold: false });
        let (fill, text) = if dimmed(f, c, n) { (Ink::dim_operator(n), Ink::Text) } else { (Ink::operator(n), Ink::on_operator(n)) };
        d.boxes.push(BoxDraw { x: cell.chip.0, y: cell.chip.1, w: width_of(cell.chip), h: height_of(cell.chip), fill, edge: None, text: (n + 1).to_string(), text_ink: text, bold: true });
        if let Some(l) = o.level {
            let lv = cell.level;
            d.boxes.push(BoxDraw { x: lv.0, y: lv.1, w: width_of(lv), h: height_of(lv) - bar_h, fill: Ink::Clear, edge: None, text: (c.text)(l), text_ink: Ink::Text, bold: true });
            let bar = (lv.0, lv.3 - bar_h, lv.2, lv.3);
            d.boxes.push(BoxDraw { x: bar.0, y: bar.1, w: width_of(bar), h: bar_h, fill: Ink::Raised, edge: Some(Ink::Frame), text: String::new(), text_ink: Ink::Text, bold: false });
            let v = (c.norm)(l) as f32 * width_of(bar);
            if v > 0. {
                d.boxes.push(BoxDraw { x: bar.0, y: bar.1, w: v, h: bar_h, fill: Ink::operator(n), edge: None, text: String::new(), text_ink: Ink::Text, bold: false });
            }
        }
        let (ratio, fixed) = ratio_text(o, c, width_of(cell.ratio));
        d.boxes.push(BoxDraw { x: cell.ratio.0, y: cell.ratio.1, w: width_of(cell.ratio), h: height_of(cell.ratio), fill: Ink::Raised, edge: fixed.then_some(Ink::Accent), text: ratio, text_ink: Ink::Text, bold: false });
        // The cursor on the operator's line of the overview.
        if c.over == Some(n) {
            d.shapes.push(Shape { ink: Ink::Focus, fill: false, width: 2, commands: inner_rect((cell.whole.0 - 1., cell.whole.1 - 1., cell.whole.2 + 1., cell.whole.3 + 1.), 2.) });
        }
    }
    d
}

/// The operators in drawing order for the graph: dimmed ones first, then the rest, the selected last.
fn dim_first(f: &FmShape, c: &Ctx, order: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = order.to_vec();
    v.sort_by_key(|&n| (n == order[order.len() - 1], !dimmed(f, c, n)));
    v
}

pub fn crisp(v: f32) -> f32 {
    v.floor() + 0.5
}

fn distance_to_line(points: &[(f32, f32)], at: (f32, f32)) -> f32 {
    let mut best = f32::MAX;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0. { (((at.0 - a.0) * dx + (at.1 - a.1) * dy) / len2).clamp(0., 1.) } else { 0. };
        best = best.min((at.0 - (a.0 + t * dx)).hypot(at.1 - (a.1 + t * dy)));
    }
    best
}

pub fn grab(f: &FmShape, c: &Ctx, at: (f32, f32)) -> Option<Held> {
    let g = geometry(f, c);
    let raw = |p: usize| c.params[p].min + (c.norm)(p) * (c.params[p].max - c.params[p].min);
    for (n, (cell, o)) in g.cells.iter().zip(&f.operators).enumerate() {
        let near = |a: Area| contains(a, at, 2.);
        if let (true, Some(l)) = (near(cell.level), o.level) {
            return Some(Held::Level { param: l });
        }
        if let (true, Some(r)) = (near(cell.ratio), o.ratio) {
            return Some(Held::Ratio { param: r, start: raw(r), from: at });
        }
        if contains(cell.whole, at, 0.) {
            return Some(Held::Select(n));
        }
    }
    for h in &g.heads {
        if contains(h.area, at, 4.) {
            return Some(Held::Nudge { param: h.param, start: raw(h.param), from: at, mid: (h.area.0 + h.area.2) / 2. });
        }
    }
    if let Some(line) = &g.lines.get(g.selected).and_then(|l| l.as_ref()) {
        let _ = line;
        let d = draw(f, c);
        if let Some(k) = d.handles.iter().enumerate().filter(|(_, h)| (h.at.0 - at.0).hypot(h.at.1 - at.1) < crate::views::REACH).min_by(|a, b| (a.1.at.0 - at.0).hypot(a.1.at.1 - at.1).total_cmp(&(b.1.at.0 - at.0).hypot(b.1.at.1 - at.1))).map(|(k, _)| k) {
            return Some(Held::Node(k));
        }
    }
    for (n, b) in g.picture.boxes.iter().enumerate() {
        if contains(*b, at, 3.) {
            return Some(Held::Select(n));
        }
    }
    let mut nearest: Option<(f32, usize)> = None;
    for (n, line) in g.lines.iter().enumerate() {
        if let Some(l) = line {
            let d = distance_to_line(&l.layout.points, at);
            if d < 12. && nearest.is_none_or(|(best, _)| d < best) {
                nearest = Some((d, n));
            }
        }
    }
    if let Some((_, n)) = nearest {
        return Some(Held::Select(n));
    }
    if let (true, Some(a)) = (contains(g.diagram, at, 0.), f.algorithm) {
        return Some(Held::Algorithm { param: a, start: raw(a), from: at });
    }
    None
}

pub fn drag(f: &FmShape, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match held {
        Held::Level { param } => {
            let g = geometry(f, c);
            let Some(cell) = f.operators.iter().position(|o| o.level == Some(*param)).and_then(|n| g.cells.get(n)) else { return vec![] };
            vec![(*param, c.params[*param].denorm(f64::from(((at.0 - cell.level.0) / width_of(cell.level)).clamp(0., 1.))))]
        }
        Held::Algorithm { param, start, from } | Held::Ratio { param, start, from } => {
            // A step per 24 pixels across the routing, or up a ratio; a continuous one by its range.
            let steps = if matches!(held, Held::Algorithm { .. }) { f64::from(at.0 - from.0) / 24. } else { f64::from(from.1 - at.1) / 12. };
            let p = &c.params[*param];
            let v = if p.stepped { start + steps.trunc() } else { start + steps / 100. * (p.max - p.min) };
            vec![(*param, v.clamp(p.min, p.max))]
        }
        Held::Node(k) => {
            let g = geometry(f, c);
            let (Some(Some(line)), Some(o)) = (g.lines.get(g.selected), f.operators.get(g.selected)) else { return vec![] };
            let (Some(p), Some(h)) = (line.envelope.points.get(*k), line.layout.handles.get(*k)) else { return vec![] };
            let l = &line.layout;
            let mut out = Vec::new();
            if let Some(t) = p.time {
                let param = &c.params[o.envelope[t]];
                let across = ((at.0 - h.from - l.least) / (l.segment - l.least)).clamp(0., 1.);
                out.push((o.envelope[t], param.denorm(grid::stage_normal(param, p.rate, l.longest, f64::from(across)))));
            }
            if let (Some(v), true) = (p.level, line.gain > 0.) {
                let up = ((l.bottom - at.1) / l.height).clamp(0., 1.);
                out.push((o.envelope[v], c.params[o.envelope[v]].denorm(f64::from(up))));
            }
            out
        }
        Held::Select(_) | Held::Nudge { .. } | Held::Rows(_) => vec![],
    }
}

/// What letting go sets: a tap on the diagram steps the algorithm, back on its left half, on on its right.
pub fn release(f: &FmShape, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    match held {
        Held::Algorithm { param, start, from } if (at.0 - from.0).hypot(at.1 - from.1) < 4. => {
            let g = geometry(f, c);
            let p = &c.params[*param];
            let step = if at.0 < (g.diagram.0 + g.diagram.2) / 2. { -1. } else { 1. };
            vec![(*param, (start + step).clamp(p.min, p.max))]
        }
        Held::Nudge { param, start, from, mid } if (at.0 - from.0).hypot(at.1 - from.1) < 4. => {
            let p = &c.params[*param];
            let step = if at.0 < *mid { -1. } else { 1. };
            let by = if p.stepped { step } else { step * (p.max - p.min) / 100. };
            vec![(*param, (start + by).clamp(p.min, p.max))]
        }
        _ => vec![],
    }
}
