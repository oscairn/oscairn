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
use crate::views::{height_of, inner_rect, poly, rect_path, width_of, Area, BoxDraw, Ctx, Drawn, Handle, Held, Ink, Label, Shape, StagesShape};

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
    /// The frequency ratio, or the coarse part of it where the operator has a `fine` of its own.
    pub ratio: Option<usize>,
    pub fixed: Option<usize>,
    pub fine: Option<usize>,
    pub detune: Option<usize>,
    /// PROVISIONAL (notes/ui-fm-fields-proposal.md): the operator's mute and solo, `operatorMute` and `operatorSolo`
    /// in `params.list`. The layout extension has no field for either yet.
    pub mute: Option<usize>,
    pub solo: Option<usize>,
    /// The operator's envelope parameters (the grid's), in the plugin's order.
    pub envelope: Vec<usize>,
}

impl Operator {
    fn owns(&self, param: usize) -> bool {
        [self.level, self.ratio, self.fixed, self.fine, self.detune, self.mute, self.solo].contains(&Some(param)) || self.envelope.contains(&param)
    }
}

/// A column of the operator table. A column is there when the plugin's data has the field for any operator (mute and
/// solo always: off, with the reason, where the plugin has none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Mute,
    Solo,
    Level,
    /// The ratio's coarse part (the `ratio` field, where there is a `fine` field too).
    Coarse,
    Fine,
    /// The ratio, or the fixed frequency: the `ratio` field's text (with its unit when the operator is fixed).
    Ratio,
    /// The mode, ratio or fixed (the `fixed` field), where coarse and fine are columns of their own.
    Mode,
    Detune,
}

/// Where a solo comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoloFrom {
    /// The plugin's own solo field.
    Field,
    /// The host's: the others muted, over a mute the plugin says is transient (not saved, not an undo step).
    Others,
    None,
}

impl Column {
    pub fn heading(self, f: &FmShape) -> &'static str {
        match self {
            Column::Mute => "M",
            Column::Solo => "S",
            Column::Level => "Level",
            Column::Coarse => "Coarse",
            Column::Fine => "Fine",
            Column::Ratio if f.operators.iter().any(|o| o.fixed.is_some()) => "Ratio/Hz",
            Column::Ratio => "Ratio",
            Column::Mode => "Mode",
            Column::Detune => "Detune",
        }
    }
    /// The parameter an operator's cell in this column edits.
    pub fn param(self, o: &Operator) -> Option<usize> {
        match self {
            Column::Mute => o.mute,
            Column::Solo => o.solo,
            Column::Level => o.level,
            Column::Coarse | Column::Ratio => o.ratio,
            Column::Fine => o.fine,
            Column::Mode => o.fixed,
            Column::Detune => o.detune,
        }
    }
    /// Pressed and not turned: the cell is switched, not dialed.
    pub fn toggles(self) -> bool {
        matches!(self, Column::Mute | Column::Solo | Column::Mode)
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

    /// The table's columns, left to right, for the fields the data has: mute and solo, level, then coarse and fine with the mode where the operator has a `fine` field of
    /// its own, else the ratio as one column, then detune.
    pub fn columns(&self) -> Vec<Column> {
        let any = |f: &dyn Fn(&Operator) -> bool| self.operators.iter().any(f);
        let mut v = vec![];
        // Mute and solo are always there; where the plugin has no field for one it is shown off, with the reason.
        if !self.operators.is_empty() {
            v.extend([Column::Mute, Column::Solo]);
        }
        if any(&|o| o.level.is_some()) {
            v.push(Column::Level);
        }
        if any(&|o| o.fine.is_some()) {
            if any(&|o| o.ratio.is_some()) {
                v.push(Column::Coarse);
            }
            v.push(Column::Fine);
            if any(&|o| o.fixed.is_some()) {
                v.push(Column::Mode);
            }
        } else if any(&|o| o.ratio.is_some() || o.fixed.is_some()) {
            v.push(Column::Ratio);
        }
        if any(&|o| o.detune.is_some()) {
            v.push(Column::Detune);
        }
        v
    }

    /// Where the operators' solo comes from: a solo field, else the host muting the others over a mute the plugin
    /// marks transient, else nowhere (the S column is then shown off, with the reason in the readout).
    pub fn solo_from(&self, params: &[Param]) -> SoloFrom {
        if self.operators.iter().any(|o| o.solo.is_some()) {
            SoloFrom::Field
        } else if self.operators.iter().all(|o| o.mute.is_some_and(|m| params[m].transient)) && self.operators.iter().any(|o| o.mute.is_some()) {
            SoloFrom::Others
        } else {
            SoloFrom::None
        }
    }

    /// Why the cell of operator `n` in `column` cannot be edited, if it cannot (said in the cursor's readout).
    pub fn disabled(&self, n: usize, column: Column, params: &[Param]) -> Option<&'static str> {
        let o = self.operators.get(n)?;
        match column {
            Column::Solo => match self.solo_from(params) {
                SoloFrom::Others => None,
                SoloFrom::Field if o.solo.is_some() => None,
                SoloFrom::Field => Some("This operator has no solo"),
                SoloFrom::None if self.operators.iter().any(|o| o.mute.is_some()) => Some("No solo: this plugin's mute is saved with the sound, so the host does not mute others"),
                SoloFrom::None => Some("This plugin has no solo for its operators"),
            },
            Column::Mute if o.mute.is_none() => Some("This plugin has no mute for its operators"),
            _ if column.param(o).is_none() => Some("This operator has no such field"),
            _ => None,
        }
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
            "fine" => op.fine = Some(i),
            "detune" => op.detune = Some(i),
            "operatorMute" => op.mute = Some(i),
            "operatorSolo" => op.solo = Some(i),
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

/// One line of the operator table: the operator's number chip, a cell in each of the table's columns
/// (`FmShape::columns`), and the whole line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub whole: Area,
    pub chip: Area,
    pub cells: Vec<Area>,
}

/// An operator's envelope line in a plot.
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
    columns: Vec<Column>,
    /// The line over the table where the columns' headings are (or the readout of the cell the cursor is on).
    heading: Area,
    rows: Vec<Row>,
    heads: Vec<Head>,
    lines: Vec<Option<Line>>,
    selected: usize,
    tag: (f32, f32),
    small: f32,
    /// Where the graph's time scale goes: under its plot.
    scale: Area,
}

fn contains(a: Area, at: (f32, f32), grow: f32) -> bool {
    at.0 >= a.0 - grow && at.0 <= a.2 + grow && at.1 >= a.1 - grow && at.1 <= a.3 + grow
}

/// The measures of the overview that its height and the detail's are worked out from, all from the theme's type
/// size and spacing: where the body starts under the heads, the least a table row is as high as and what it is given
/// when there is room, the gap between rows, and the foot.
struct Metrics {
    small: f32,
    sp: f32,
    body_top: f32,
    row_min: f32,
    row_pref: f32,
    row_max: f32,
    gap: f32,
    foot: f32,
}

fn metrics(scale: f32, spacing: f32) -> Metrics {
    let small = 13. * scale;
    let sp = spacing;
    Metrics { small, sp, body_top: 6. * sp + small * 1.3 + 2. * sp, row_min: small * 1.3, row_pref: small * 1.7, row_max: small * 2., gap: 2. * sp, foot: 6. * sp }
}

/// The least the overview is high and still draws `count` operators as table rows each its least high.
pub fn least_height(count: usize, scale: f32, spacing: f32) -> f32 {
    let m = metrics(scale, spacing);
    m.body_top + count as f32 * m.row_min + count.saturating_sub(1) as f32 * m.gap + m.foot
}

/// What the overview is given when the detail can spare it: rows of 1.7 lines of type, and at least 11 lines of type
/// in all, which keeps the diagram's boxes and the graph's tags legible.
pub fn preferred_height(count: usize, scale: f32, spacing: f32) -> f32 {
    let m = metrics(scale, spacing);
    (m.body_top + count as f32 * m.row_pref + count.saturating_sub(1) as f32 * m.gap + m.foot).max(m.small * 11.)
}

/// The most the overview's rows can use: past it the table's rows stay as high as they are.
pub fn greatest_height(count: usize, scale: f32, spacing: f32) -> f32 {
    let m = metrics(scale, spacing);
    m.body_top + count as f32 * m.row_max + count.saturating_sub(1) as f32 * m.gap + m.foot
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

/// The widths of a table's columns at the type size `small` and a row `row` high: marks are square, the chip is a
/// little wider than high, the rest by what they hold ("Ratio/Hz" and "12.34 Hz" are the widest texts).
fn column_width(column: Column, small: f32, row: f32) -> f32 {
    match column {
        Column::Mute | Column::Solo => row,
        Column::Level => 6. * small,
        Column::Coarse | Column::Fine => 3.7 * small,
        Column::Ratio => 5. * small,
        Column::Mode => 3.6 * small,
        Column::Detune => 3.7 * small,
    }
}

/// The time scale's room under a graph (ticks and their labels), and how wide the graph's right margin for the
/// operators' tags is.
fn scale_height(small: f32) -> f32 {
    small * 1.4
}

/// An envelope plot's lines: every operator's in `plot`, on one time scale. `slotted`: a stage in a slot of its own
/// (the detail's graph: levels as given), else stage after stage with each line scaled by its operator's level.
fn lines_in(f: &FmShape, c: &Ctx, plot: Area, slotted: bool) -> Vec<Option<Line>> {
    let count = f.operators.len();
    let mut lines: Vec<Option<Line>> = (0..count).map(|_| None).collect();
    let shapes: Vec<(grid::Envelope, Vec<&Param>)> = f
        .operators
        .iter()
        .map(|o| {
            let fields: Vec<&str> = o.envelope.iter().map(|&i| c.params[i].field.as_str()).collect();
            (grid::envelope_of_fields(&fields), o.envelope.iter().map(|&i| &c.params[i]).collect())
        })
        .collect();
    let longest = shapes.iter().map(|(e, refs)| grid::longest_stage(&e.points.iter().filter_map(|p| p.time).map(|t| refs[t]).collect::<Vec<_>>())).fold(0., f64::max);
    for (n, ((e, refs), o)) in shapes.into_iter().zip(&f.operators).enumerate() {
        if e.points.is_empty() {
            continue;
        }
        let norm = |k: usize| (c.norm)(o.envelope[k]);
        let gain = if slotted { 1. } else { o.level.map_or(1., |l| (c.norm)(l) as f32) };
        let layout = if slotted { grid::envelope_layout_slots(&e, &refs, &norm, plot, longest) } else { grid::envelope_layout_in(&e, &refs, &norm, plot, gain, longest) };
        lines[n] = Some(Line { envelope: e, layout, gain });
    }
    lines
}

fn geometry(f: &FmShape, c: &Ctx) -> Geometry {
    let m = metrics(c.scale, c.spacing);
    let (small, sp) = (m.small, m.sp);
    let pad = 12. * sp;
    let count = f.operators.len();
    let columns = f.columns();
    let has_graph = f.operators.iter().any(|o| !grid::envelope_of_fields(&o.envelope.iter().map(|&i| c.params[i].field.as_str()).collect::<Vec<_>>()).points.is_empty());
    let (top, bottom) = (m.body_top, c.h - m.foot);
    let (left, right) = (pad, c.w - pad);
    let row = if count > 0 { ((bottom - top - m.gap * (count - 1) as f32) / count as f32).clamp(m.row_min, m.row_max) } else { m.row_min };
    // The table's width: its columns at the widths their texts need, scaled between 0.8 and 1 by what the panes beside
    // it leave (the marks and the chip stay the row's size); the graph at its left and the diagram at its right share
    // the rest. A graph that would not get nine lines of type across is left out: the diagram is what shrinks.
    let chip_w = row.max(small * 1.7);
    let cell_gap = 3. * sp;
    let pane_gap = 12. * sp;
    let fixed: f32 = chip_w + columns.iter().filter(|k| matches!(k, Column::Mute | Column::Solo)).map(|_| cell_gap + row).sum::<f32>();
    let flexible: f32 = columns.iter().filter(|k| !matches!(k, Column::Mute | Column::Solo)).map(|&k| cell_gap + column_width(k, small, row)).sum();
    let avail = right - left;
    // The least the diagram is given: nine lines of type across, and the room its heads (the view's own parameters named
    // over it) need side by side.
    let texts: Vec<(usize, String)> = f.own.iter().map(|&p| (p, format!("{} {}", c.params[p].name, (c.text)(p)))).collect();
    let head_widths: Vec<f32> = texts.iter().map(|(_, t)| bold_width(t, small) + 1.).collect();
    let min_diagram = (9. * small).max(head_widths.iter().sum::<f32>() + 12. * sp * head_widths.len().saturating_sub(1) as f32) + 2.;
    let min_pane = 9. * small;
    let fit = |panes: usize| -> f32 {
        let room = avail - fixed - panes as f32 * pane_gap - (panes - 1) as f32 * min_pane - min_diagram;
        (room / flexible.max(1.)).clamp(0.8, 1.)
    };
    let graph_shown = has_graph && avail - (fixed + flexible * 0.8) - 2. * pane_gap >= min_pane + min_diagram;
    let stretch = fit(if graph_shown { 2 } else { 1 });
    let table_w = fixed + flexible * stretch;
    let rest = (avail - table_w - if graph_shown { 2. } else { 1. } * pane_gap).max(0.);
    let diagram_w = if graph_shown { (rest * 0.55).max(min_diagram).min(rest) } else { rest };
    let graph_w = if graph_shown { rest - diagram_w } else { 0. };
    let table_x = left + if graph_shown { graph_w + pane_gap } else { 0. };
    let diagram_x = table_x + table_w + pane_gap;
    let diagram_area = (diagram_x.min(right), top, right, bottom);
    let tag = (small * 1.3, small * 1.2);
    let graph = graph_shown.then_some((left, top, left + graph_w, bottom));
    let scale_room = if graph_shown { scale_height(small) } else { 0. };
    let plot = graph.map_or((0., 0., 0., 0.), |g| (g.0 + 10. * sp, g.1 + grid::NODE_RADIUS + 4., g.2 - tag.0 - grid::NODE_RADIUS - 6., g.3 - 8. - scale_room));
    let scale = graph.map_or((0., 0., 0., 0.), |g| (plot.0, plot.3 + 4., plot.2 + tag.0, g.3 - 2.));
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
    let picture = flow.as_ref().map(|fl| diagram(fl, count, diagram_area, small, route.is_some(), true)).unwrap_or_default();
    // The heads: the view's own parameters named in a line over the diagram, each as wide as its text; the gaps between them
    // give way (to none) where the diagram is not wide enough for both, then the heads' widths share what there is.
    let widths = head_widths;
    let room = (right - diagram_x).max(0.);
    let gap = if texts.len() > 1 { ((room - widths.iter().sum::<f32>()) / (texts.len() - 1) as f32).clamp(0., 12. * sp) } else { 0. };
    let shrink = (room / (widths.iter().sum::<f32>() + gap * texts.len().saturating_sub(1) as f32).max(1.)).min(1.);
    let mut heads = Vec::new();
    let mut x = diagram_x;
    for ((p, text), w) in texts.into_iter().zip(widths) {
        let w = w * shrink;
        heads.push(Head { area: (x, 6. * sp, x + w, 6. * sp + small * 1.3), param: p, text });
        x += w + gap;
    }
    // The table: the chip, then a cell for each column, on one line for each operator.
    let widths: Vec<f32> = columns.iter().map(|&k| if matches!(k, Column::Mute | Column::Solo) { row } else { column_width(k, small, row) * stretch }).collect();
    let rows = (0..count)
        .map(|n| {
            let y = top + (row + m.gap) * n as f32;
            let chip = (table_x, y, table_x + chip_w, y + row);
            let mut x = chip.2 + cell_gap;
            let cells = widths
                .iter()
                .map(|w| {
                    let a = (x, y, x + w, y + row);
                    x += w + cell_gap;
                    a
                })
                .collect();
            Row { whole: (table_x - 1., y - 1., table_x + table_w + 1., y + row + 1.), chip, cells }
        })
        .collect();
    let heading = (table_x, 6. * sp, table_x + table_w, 6. * sp + small * 1.3);
    let lines = if has_graph && graph_shown { lines_in(f, c, plot, false) } else { (0..count).map(|_| None).collect() };
    Geometry { diagram: diagram_area, graph, plot, picture, columns, heading, rows, heads, lines, selected: c.selected.min(count.saturating_sub(1)), tag, small, scale }
}

/// Whether operator `n` is muted: its mute field is on.
fn muted(f: &FmShape, c: &Ctx, n: usize) -> bool {
    f.operators[n].mute.is_some_and(|m| (c.norm)(m) > 0.5)
}

/// Whether operator `n` is soloed: its solo field is on, or the host has soloed it.
fn soloed(f: &FmShape, c: &Ctx, n: usize) -> bool {
    f.operators[n].solo.is_some_and(|s| (c.norm)(s) > 0.5) || c.soloed.get(n).copied().unwrap_or(false)
}

/// Whether an operator is silent in the picture: its level is 0, it is muted, or another is soloed and it is not.
fn dimmed(f: &FmShape, c: &Ctx, n: usize) -> bool {
    let any_solo = (0..f.operators.len()).any(|m| soloed(f, c, m));
    f.operators[n].level.is_some_and(|l| (c.norm)(l) <= 0.) || muted(f, c, n) || (any_solo && !soloed(f, c, n))
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

/// The ratio column's text: the plugin's text for the ratio, and when the operator is fixed (its `fixed` field on: the
/// value is a frequency) the unit "Hz" where the plugin's own text has none. Whether it is fixed says it again in the
/// cell's edge.
fn ratio_text(o: &Operator, c: &Ctx) -> (String, bool) {
    let ratio = o.ratio.map(|r| (c.text)(r)).unwrap_or_default();
    match o.fixed.filter(|&x| (c.norm)(x) > 0.5) {
        Some(_) if ratio.chars().any(char::is_alphabetic) => (ratio, true),
        Some(_) => (format!("{ratio} Hz"), true),
        None => (ratio, false),
    }
}

/// A table cell's text, and whether its edge is the accent's (a fixed frequency).
fn cell_text(f: &FmShape, c: &Ctx, n: usize, column: Column) -> (String, bool) {
    let o = &f.operators[n];
    let of = |p: Option<usize>| p.map(|p| (c.text)(p)).unwrap_or_default();
    match column {
        Column::Mute => ("M".into(), false),
        Column::Solo => ("S".into(), false),
        Column::Level => (of(o.level), false),
        Column::Coarse => (of(o.ratio), false),
        Column::Fine => (of(o.fine), false),
        Column::Ratio => ratio_text(o, c),
        Column::Mode => (of(o.fixed), o.fixed.is_some_and(|x| (c.norm)(x) > 0.5)),
        Column::Detune => (of(o.detune), false),
    }
}

/// A level cell's bar and the text of its value beside it.
fn level_parts(cell: Area, row: f32) -> (Area, Area) {
    let w = width_of(cell);
    let text_w = (w * 0.38).max(0.);
    let bar_h = (row * 0.42).max(3.);
    let mid = (cell.1 + cell.3) / 2.;
    ((cell.0, mid - bar_h / 2., cell.2 - text_w, mid + bar_h / 2.), (cell.2 - text_w, cell.1, cell.2, cell.3))
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

/// The graph's time scale: where a stage's time falls along one stage of the plot (a stage's width is its least plus a
/// share of the rest by `grid::stage_across`: about even below 50 ms, logarithmic past it), as ticks under the plot with
/// the labels that fit, the longest first; and a note where the widths are not times. `layout` is a line's layout in the
/// plot (its stage width and its least), `at` the left edge of the stage the scale is drawn along.
fn scale_marks(d: &mut Drawn, layout: &grid::EnvLayout, at: f32, area: Area, small: f32, nominal: bool) {
    let (y, bottom) = (area.1 + 2., area.3);
    let note = |d: &mut Drawn, text: &str| {
        let w = bold_width(text, small);
        d.labels.push(Label { x: area.2 - w, y, w, h: bottom - y, text: text.into(), ink: Ink::Muted, bold: false, align: 2 });
    };
    if layout.longest <= 0. {
        note(d, "by value");
        return;
    }
    let (from, to) = (at + layout.least, at + layout.segment);
    d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 1, commands: poly(&[(crisp(from), y), (crisp(to), y)]) });
    let mut right_edge = f32::MAX;
    for (t, text) in [(10., "10 s"), (1., "1 s"), (0.1, "100 ms"), (0.01, "10 ms")] {
        if t > layout.longest * 1.001 {
            continue;
        }
        let x = from + grid::stage_axis(t, layout.longest) as f32 * (to - from);
        d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 1, commands: poly(&[(crisp(x), y), (crisp(x), y + 3.)]) });
        let w = bold_width(text, small);
        // Labels are drawn only where they do not run into the one to their right (the longer time).
        let mut left = (x - w / 2.).max(area.0);
        if left + w > right_edge {
            continue;
        }
        left = left.min(right_edge - w);
        right_edge = left - 2.;
        d.labels.push(Label { x: left, y: y + 3., w, h: (bottom - y - 3.).max(0.), text: text.into(), ink: Ink::Muted, bold: false, align: 1 });
    }
    if nominal && at + layout.segment + bold_width("nominal", small) < area.2 {
        note(d, "nominal");
    }
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
    // The selected operator's ring, around its box in the diagram.
    if g.selected < g.picture.boxes.len() {
        d.shapes.push(Shape { ink: Ink::Text, fill: false, width: 1, commands: ring(g.picture.boxes[g.selected], 2.5) });
    }
    for (n, b) in g.picture.boxes.iter().enumerate() {
        let (fill, text) = if dimmed(f, c, n) { (Ink::dim_operator(n), Ink::Text) } else { (Ink::operator(n), Ink::on_operator(n)) };
        d.boxes.push(BoxDraw { x: b.0, y: b.1, w: width_of(*b), h: height_of(*b), fill, edge: Some(Ink::Border), text: (n + 1).to_string(), text_ink: text, bold: true, mark: false });
    }
    // The graph: a frame, every operator's line (the dimmed first, the selected last), a tag at each end, the time scale.
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
        // The time scale, along the first stage of the selected operator's line (its stage width is the same for all).
        if let Some(line) = g.lines.get(g.selected).and_then(Option::as_ref).or_else(|| g.lines.iter().flatten().next()) {
            let nominal = g.lines.iter().flatten().any(|l| l.envelope.points.iter().any(|p| p.rate));
            scale_marks(&mut d, &line.layout, g.plot.0, g.scale, g.small, nominal);
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
    // The table. Over it, the columns' headings in the small dim type, once; or, while the cursor is on a cell that
    // cannot be edited, why (the cursor's readout).
    let why = c.cell.and_then(|(n, k)| g.columns.get(k).and_then(|&col| f.disabled(n, col, c.params)));
    let head = g.heading;
    match (why, g.rows.first()) {
        (Some(why), _) => d.labels.push(Label { x: head.0, y: head.1, w: width_of(head), h: height_of(head), text: why.into(), ink: Ink::Text, bold: false, align: 0 }),
        (None, Some(first)) => {
            let label = |d: &mut Drawn, a: Area, text: &str| d.labels.push(Label { x: a.0, y: head.1, w: width_of(a), h: height_of(head), text: text.into(), ink: Ink::Muted, bold: false, align: 1 });
            label(&mut d, first.chip, "Op");
            for (&col, &cell) in g.columns.iter().zip(&first.cells) {
                if !matches!(col, Column::Mute | Column::Solo) {
                    label(&mut d, cell, col.heading(f));
                }
            }
        }
        _ => {}
    }
    // A solo that is on is told over the mute and solo marks: a forgotten solo makes a sound seem broken.
    if let (true, Some(first)) = (c.soloed.iter().any(|&s| s), g.rows.first()) {
        if let Some(m) = g.columns.iter().position(|&k| k == Column::Mute) {
            let (x0, x1) = (first.cells[m].0, first.cells[m + 1].2);
            d.labels.push(Label { x: x0, y: head.1, w: x1 - x0, h: height_of(head), text: "solo".into(), ink: Ink::Accent, bold: true, align: 1 });
        }
    }
    let row_h = g.rows.first().map_or(0., |r| height_of(r.whole) - 2.);
    for (n, (row, o)) in g.rows.iter().zip(&f.operators).enumerate() {
        let chosen = n == g.selected;
        d.boxes.push(BoxDraw { x: row.whole.0, y: row.whole.1, w: width_of(row.whole), h: height_of(row.whole), fill: if chosen { Ink::Raised } else { Ink::Clear }, edge: Some(if chosen { Ink::Text } else { Ink::Frame }), text: String::new(), text_ink: Ink::Text, bold: false, mark: false });
        let (fill, text) = if dimmed(f, c, n) { (Ink::dim_operator(n), Ink::Text) } else { (Ink::operator(n), Ink::on_operator(n)) };
        d.boxes.push(BoxDraw { x: row.chip.0, y: row.chip.1, w: width_of(row.chip), h: height_of(row.chip), fill, edge: None, text: (n + 1).to_string(), text_ink: text, bold: true, mark: false });
        for (k, (&col, &cell)) in g.columns.iter().zip(&row.cells).enumerate() {
            let disabled = f.disabled(n, col, c.params).is_some();
            let (w, h) = (width_of(cell), height_of(cell));
            match col {
                Column::Mute | Column::Solo => {
                    let on = if col == Column::Mute { muted(f, c, n) } else { soloed(f, c, n) };
                    let (fill, edge, ink) = match (disabled, on) {
                        (true, _) => (Ink::Clear, Some(Ink::Frame), Ink::Frame),
                        (false, true) => (if col == Column::Mute { Ink::Danger } else { Ink::Accent }, None, Ink::Background),
                        (false, false) => (Ink::MarkOff, None, Ink::Muted),
                    };
                    d.boxes.push(BoxDraw { x: cell.0, y: cell.1, w, h, fill, edge, text: if col == Column::Mute { "M" } else { "S" }.into(), text_ink: ink, bold: true, mark: true });
                }
                Column::Level => {
                    if let Some(l) = o.level {
                        let (bar, value) = level_parts(cell, row_h);
                        d.boxes.push(BoxDraw { x: bar.0, y: bar.1, w: width_of(bar), h: height_of(bar), fill: Ink::Background, edge: Some(Ink::Frame), text: String::new(), text_ink: Ink::Text, bold: false, mark: false });
                        let v = (c.norm)(l) as f32 * width_of(bar);
                        if v > 0. {
                            d.boxes.push(BoxDraw { x: bar.0, y: bar.1, w: v, h: height_of(bar), fill: if dimmed(f, c, n) { Ink::dim_operator(n) } else { Ink::operator(n) }, edge: None, text: String::new(), text_ink: Ink::Text, bold: false, mark: false });
                        }
                        d.boxes.push(BoxDraw { x: value.0, y: value.1, w: width_of(value), h: height_of(value), fill: Ink::Clear, edge: None, text: (c.text)(l), text_ink: Ink::Text, bold: true, mark: false });
                    }
                }
                _ => {
                    if column_has(col, o) {
                        let (text, fixed) = cell_text(f, c, n, col);
                        d.boxes.push(BoxDraw { x: cell.0, y: cell.1, w, h, fill: if chosen { Ink::Background } else { Ink::Raised }, edge: Some(if fixed { Ink::Accent } else { Ink::Frame }), text, text_ink: Ink::Text, bold: false, mark: false });
                    }
                }
            }
            if c.cell == Some((n, k)) {
                let ink = if c.editing { Ink::Accent } else { Ink::Focus };
                d.shapes.push(Shape { ink, fill: false, width: 2, commands: inner_rect((cell.0 - 1., cell.1 - 1., cell.2 + 1., cell.3 + 1.), 2.) });
            }
        }
    }
    d
}

/// Whether the operator has what its cell in `column` shows.
fn column_has(column: Column, o: &Operator) -> bool {
    column.param(o).is_some() || (column == Column::Ratio && o.fixed.is_some())
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
    for (n, (row, o)) in g.rows.iter().zip(&f.operators).enumerate() {
        if contains(row.chip, at, 1.) {
            return Some(Held::Select(n));
        }
        for (k, (&col, &cell)) in g.columns.iter().zip(&row.cells).enumerate() {
            if !contains(cell, at, 1.) {
                continue;
            }
            return Some(if f.disabled(n, col, c.params).is_some() {
                Held::Cell { row: n, col: k }
            } else if col.toggles() {
                Held::Mark { row: n, col: k }
            } else {
                match col.param(o) {
                    Some(l) if col == Column::Level => Held::Level { param: l },
                    Some(p) => Held::Ratio { param: p, start: raw(p), from: at },
                    None => Held::Cell { row: n, col: k },
                }
            });
        }
        if contains(row.whole, at, 0.) {
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
            let Some(n) = f.operators.iter().position(|o| o.level == Some(*param)) else { return vec![] };
            let Some(k) = g.columns.iter().position(|&col| col == Column::Level) else { return vec![] };
            let Some(row) = g.rows.get(n) else { return vec![] };
            let (bar, _) = level_parts(row.cells[k], height_of(row.whole) - 2.);
            vec![(*param, c.params[*param].denorm(f64::from(((at.0 - bar.0) / width_of(bar).max(1.)).clamp(0., 1.))))]
        }
        Held::Algorithm { param, start, from } | Held::Ratio { param, start, from } => {
            // A step per 24 pixels across the routing, or up a number; a continuous one by its range.
            let steps = if matches!(held, Held::Algorithm { .. }) { f64::from(at.0 - from.0) / 24. } else { f64::from(from.1 - at.1) / 12. };
            let p = &c.params[*param];
            let v = if p.stepped { start + steps.trunc() } else { start + steps / 100. * (p.max - p.min) };
            vec![(*param, v.clamp(p.min, p.max))]
        }
        Held::Node(k) => {
            let g = geometry(f, c);
            let (Some(Some(line)), Some(o)) = (g.lines.get(g.selected), f.operators.get(g.selected)) else { return vec![] };
            node_drag(line, o, c, *k, at, false)
        }
        Held::Select(_) | Held::Nudge { .. } | Held::Rows(_) | Held::Operator(_) | Held::Cell { .. } | Held::Mark { .. } => vec![],
    }
}

/// What dragging node `k` of an operator's line to `at` sets: its stage's time across (its own stage in the line, or its
/// slot) and its level up.
fn node_drag(line: &Line, o: &Operator, c: &Ctx, k: usize, at: (f32, f32), slotted: bool) -> Vec<(usize, f64)> {
    let (Some(p), Some(h)) = (line.envelope.points.get(k), line.layout.handles.get(k)) else { return vec![] };
    let l = &line.layout;
    let mut out = Vec::new();
    if let Some(t) = p.time {
        let param = &c.params[o.envelope[t]];
        let across = ((at.0 - h.from - l.least) / (l.segment - l.least)).clamp(0., 1.);
        out.push((o.envelope[t], param.denorm(grid::stage_normal(param, p.rate, l.longest, f64::from(across)))));
    }
    if let (Some(v), true) = (p.level, line.gain > 0. || slotted) {
        let up = ((l.bottom - at.1) / l.height).clamp(0., 1.);
        out.push((o.envelope[v], c.params[o.envelope[v]].denorm(f64::from(up))));
    }
    out
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

// ---- The detail's envelope ----

/// Where the stages' plot is in the detail's envelope panel: from the rows' left edge across, under the room a node's
/// radius needs, down to the time scale.
fn stages_plot(s: &StagesShape, c: &Ctx) -> Area {
    let small = 13. * c.scale;
    (s.label, grid::NODE_RADIUS + 4., c.w - grid::NODE_RADIUS, (s.graph - scale_height(small) - 4.).max(grid::NODE_RADIUS + 5.))
}

/// The detail's envelope panel: the selected operator's envelope large (its line bright over a wash with handles, the
/// others' dim behind it, a stage to a slot of the plot), the time scale under it, and the labels of the rows of
/// stage controls under that ("Time", "Level").
pub fn draw_stages(s: &StagesShape, c: &Ctx) -> Drawn {
    let f = &s.fm;
    let mut d = Drawn::default();
    let n_ops = f.operators.len();
    if n_ops == 0 {
        return d;
    }
    let small = 13. * c.scale;
    let selected = c.selected.min(n_ops - 1);
    if s.rows {
        let first = if s.graph > 0. { s.graph + s.gap } else { 0. };
        for (k, text) in ["Time", "Level"].into_iter().enumerate() {
            let y = first + k as f32 * (s.row + s.gap);
            d.labels.push(Label { x: 4. * c.spacing, y, w: (s.label - 4. * c.spacing).max(0.), h: s.row, text: text.into(), ink: Ink::Muted, bold: true, align: 0 });
        }
    }
    if s.graph <= 0. {
        return d;
    }
    let plot = stages_plot(s, c);
    let lines = lines_in(f, c, plot, true);
    let ink_of = |n: usize| if dimmed(f, c, n) { Ink::dim_operator(n) } else { Ink::operator(n) };
    d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: inner_rect((s.label, 0., c.w, s.graph), 1.) });
    // The slots' edges: a hairline where each stage's slot starts, so the controls under it are under its stage.
    if let Some(line) = lines.get(selected).and_then(Option::as_ref) {
        let mut rules = String::new();
        for h in line.layout.handles.iter().skip(1) {
            rules += &poly(&[(crisp(h.from), plot.1 - grid::NODE_RADIUS), (crisp(h.from), plot.3)]);
            rules += " ";
        }
        if !rules.is_empty() {
            d.shapes.push(Shape { ink: Ink::Frame, fill: false, width: 0, commands: rules });
        }
    }
    let others: Vec<usize> = (0..n_ops).filter(|&n| n != selected).collect();
    for n in others.into_iter().chain([selected]) {
        let Some(line) = &lines[n] else { continue };
        if n == selected {
            d.shapes.push(Shape { ink: Ink::dim_operator(n), fill: true, width: 0, commands: line.layout.fill.clone() });
        }
        d.shapes.push(Shape { ink: if n == selected { Ink::operator(n) } else { ink_of(n) }, fill: false, width: if n == selected { 2 } else { 1 }, commands: line.layout.line.clone() });
    }
    if let (Some(line), Some(o)) = (&lines[selected], f.operators.get(selected)) {
        for (h, p) in line.layout.handles.iter().zip(&line.envelope.points) {
            d.handles.push(Handle { at: h.at, params: [p.time, p.level].into_iter().flatten().map(|k| o.envelope[k]).collect() });
        }
        let nominal = lines.iter().flatten().any(|l| l.envelope.points.iter().any(|p| p.rate));
        let scale = (plot.0, plot.3 + 4., c.w - 2., s.graph - 2.);
        scale_marks(&mut d, &line.layout, plot.0, scale, small, nominal);
    }
    d
}

pub fn grab_stages(s: &StagesShape, c: &Ctx, at: (f32, f32)) -> Option<Held> {
    let d = draw_stages(s, c);
    d.handles.iter().enumerate().filter(|(_, h)| (h.at.0 - at.0).hypot(h.at.1 - at.1) < crate::views::REACH).min_by(|a, b| (a.1.at.0 - at.0).hypot(a.1.at.1 - at.1).total_cmp(&(b.1.at.0 - at.0).hypot(b.1.at.1 - at.1))).map(|(k, _)| Held::Node(k))
}

pub fn drag_stages(s: &StagesShape, c: &Ctx, held: &Held, at: (f32, f32)) -> Vec<(usize, f64)> {
    let Held::Node(k) = held else { return vec![] };
    let f = &s.fm;
    if f.operators.is_empty() || s.graph <= 0. {
        return vec![];
    }
    let selected = c.selected.min(f.operators.len() - 1);
    let lines = lines_in(f, c, stages_plot(s, c), true);
    let Some(line) = lines[selected].as_ref() else { return vec![] };
    node_drag(line, &f.operators[selected], c, *k, at, true)
}
