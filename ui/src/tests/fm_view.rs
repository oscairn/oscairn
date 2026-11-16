// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM view as the window draws it (src/fm.rs, fixtures in tests/fm.rs): boxes and wires measured in the
//! panel's data, the operator colors, the graph's lines in the rendered pixels, the dots, selection and touch.

use super::fm::*;
use super::shots::*;
use super::*;

type Rect = (f32, f32, f32, f32);

/// The values of a fixture with its algorithm set.
fn with_algorithm(values: &[(u32, f64)], algorithm: f64) -> Vec<(u32, f64)> {
    values.iter().map(|&(id, v)| if id == 1 { (1, algorithm) } else { (id, v) }).collect()
}

/// Every routing and algorithm the fixtures hold, as (name, plugin, values, text).
type Case = (String, serde_json::Value, Vec<(u32, f64)>, fn(u32, f64) -> String);

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = vec![];
    for (name, list, values, text) in routings() {
        let algorithms = match name {
            "fm-four" => 4,
            "fm-eight" => 2,
            _ => 1,
        };
        for a in 0..algorithms {
            out.push((format!("{name}-{a}"), list.clone(), with_algorithm(&values, f64::from(a)), text));
        }
    }
    out
}

fn is_operator_ink(i: i32) -> bool {
    (16..24).contains(&i) || (32..40).contains(&i)
}

fn subpaths(commands: &str) -> Vec<Vec<(f32, f32)>> {
    commands
        .split('M')
        .filter(|t| !t.trim().is_empty())
        .map(|t| {
            let n: Vec<f32> = t.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            n.chunks(2).map(|c| (c[0], c[1])).collect()
        })
        .collect()
}

fn on_edge(r: Rect, p: (f32, f32)) -> bool {
    let near = |a: f32, b: f32| (a - b).abs() < 0.6;
    let across = p.0 >= r.0 - 0.6 && p.0 <= r.2 + 0.6;
    let down = p.1 >= r.1 - 0.6 && p.1 <= r.3 + 0.6;
    ((near(p.0, r.0) || near(p.0, r.2)) && down) || ((near(p.1, r.1) || near(p.1, r.3)) && across)
}

/// The diagram's boxes (by number), wires and loops (the paths of shapes before the graph's frame) and the output line.
struct Drawn {
    boxes: Vec<(usize, Rect)>,
    wires: Vec<Vec<(f32, f32)>>,
    bus: Option<(f32, f32, f32)>,
}

fn drawn(panel: &crate::GridPanel) -> Drawn {
    let boxes = panel
        .boxes
        .iter()
        .filter(|b| b.edge == 2)
        .map(|b| (b.text.parse::<usize>().expect("a numbered box"), (b.x, b.y, b.x + b.w, b.y + b.h)))
        .collect();
    let frame = panel.shapes.iter().position(|s| s.ink == 5).unwrap_or(panel.shapes.row_count());
    let mut wires = vec![];
    let mut bus = None;
    for s in panel.shapes.iter().take(frame) {
        if is_operator_ink(s.ink) && !s.fill {
            wires.extend(subpaths(&s.commands));
        } else if s.ink == 1 && !s.fill {
            let v = subpaths(&s.commands);
            bus = Some((v[0][0].0, v[0][0].1.min(v[0][1].1), v[0][0].1.max(v[0][1].1)));
        }
    }
    Drawn { boxes, wires, bus }
}

/// The estimated width of a text at the theme's small type: 0.6 em a character, a little more in bold.
fn text_width(text: &str, bold: bool, small: f32) -> f32 {
    text.chars().count() as f32 * 0.6 * small * if bold { 1.06 } else { 1. }
}

pub(super) fn check_panel(panel: &crate::GridPanel, ops: usize, small: f32, at: &str) {
    let (w, h) = (panel.w, panel.h);
    let d = drawn(panel);
    assert_eq!(d.boxes.len(), ops, "a box for each operator {at}");
    let mut numbers: Vec<usize> = d.boxes.iter().map(|b| b.0).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, (1..=ops).collect::<Vec<_>>(), "numbered 1 to {ops} {at}");
    for (n, r) in &d.boxes {
        assert!(r.0 >= 0. && r.1 >= 0. && r.2 <= w && r.3 <= h, "box {n} {r:?} lies outside the {w}x{h} panel {at}");
    }
    for (i, (a, ra)) in d.boxes.iter().enumerate() {
        for (b, rb) in &d.boxes[i + 1..] {
            assert!(ra.2 + 1. <= rb.0 || rb.2 + 1. <= ra.0 || ra.3 + 1. <= rb.1 || rb.3 + 1. <= ra.1, "boxes {a} and {b} overlap {at}: {ra:?} {rb:?}");
        }
    }
    // Every wire starts on a box edge and ends on a box edge or the output line; no segment passes through a box.
    for wire in &d.wires {
        let (first, last) = (wire[0], wire[wire.len() - 1]);
        assert!(d.boxes.iter().any(|(_, r)| on_edge(*r, first)), "a wire starts off every box edge at {first:?} {at}");
        let on_bus = d.bus.is_some_and(|(x, low, high)| (last.0 - x).abs() < 0.6 && last.1 >= low - 0.6 && last.1 <= high + 0.6);
        assert!(on_bus || d.boxes.iter().any(|(_, r)| on_edge(*r, last)), "a wire ends off every box edge and the output line at {last:?} {at}");
        for seg in wire.windows(2) {
            assert!(seg[0].0 == seg[1].0 || seg[0].1 == seg[1].1, "wires are orthogonal {at}: {seg:?}");
            let (lx, hx, ly, hy) = (seg[0].0.min(seg[1].0), seg[0].0.max(seg[1].0), seg[0].1.min(seg[1].1), seg[0].1.max(seg[1].1));
            for (n, r) in &d.boxes {
                assert!(!(hx > r.0 + 0.5 && lx < r.2 - 0.5 && hy > r.1 + 0.5 && ly < r.3 - 0.5), "a wire {seg:?} runs through box {n} {r:?} {at}");
            }
        }
    }
    for b in panel.boxes.iter() {
        assert!(b.x >= 0. && b.y >= 0. && b.x + b.w <= w && b.y + b.h <= h, "box {:?} outside the panel {at}", b.text);
        // A text shrinks to 70% before it is cut: it must fit then.
        assert!(b.text.is_empty() || text_width(&b.text, b.bold, small) * 0.7 <= b.w, "{:?} does not fit its {} px box even at 70% {at}", b.text, b.w);
    }
    for l in panel.labels.iter() {
        assert!(l.x >= 0. && l.y >= 0. && l.x + l.w <= w && l.y + l.h <= h, "label {:?} outside the panel {at}", l.text);
        assert!(text_width(&l.text, l.bold, small) <= l.w, "label {:?} is cut ({} px wide, needs {}) {at}", l.text, l.w, text_width(&l.text, l.bold, small));
    }
    // Every operator has its number at the end of its line in the graph.
    let tags = panel.labels.iter().filter(|l| is_operator_ink(l.ink)).count();
    // (A theme with big type at a small width has no room for the graph: it is left out, and its tags with it.)
    assert!(tags >= ops || !panel.shapes.iter().any(|s| s.ink == 5), "a numbered tag at each line end {at}: {tags}");
}

#[test]
fn the_diagram_has_boxes_apart_and_wires_that_meet_box_edges_and_miss_boxes() {
    fit_to_window();
    for (name, list, values, text) in cases() {
        let ops = if name.starts_with("fm-four") { 4 } else { 8 };
        for (size, scale) in [((1280, 720), 1.), ((1280, 720), 1.55)] {
            let mut s = Screen::sized(size.0, size.1);
            s.set_scale(scale);
            s.frame();
            let mut p = open_with(&mut s, list.clone(), &values, &text);
            for t in &all_themes() {
                theme::apply(&s.ui, t);
                s.frame();
                p.shown();
                s.frame();
                let panel = &panels(&s.ui)[0];
                check_panel(panel, ops, 13. * t.data.font_scale, &format!("in {name}, {}, {size:?}@{scale}", t.id));
            }
        }
    }
}
