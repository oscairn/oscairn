// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM operator view (src/fm.rs): the algorithm as a horizontal flow, the plot of every operator's
//! envelope, the strip of operator cells, the operator colors, the dots. Fixtures: a routing for every kind
//! the data can express, each on eight operators of a made-up plugin.

use super::shots::*;
use super::views::{fm8_plugin, fm8_text, fm8_values, fm_plugin, fm_text, fm_values, pictures, viewed};
use super::*;
use crate::fm::{algorithm_layout, Flow};

pub(super) type Routing = [u8; 11];

pub(super) fn routing(inputs: [u8; 8], carriers: u8, from: u8, to: u8) -> Routing {
    [inputs[0], inputs[1], inputs[2], inputs[3], inputs[4], inputs[5], inputs[6], inputs[7], carriers, from, to]
}

/// Eight operators in parallel, all carriers, the first feeding itself back.
pub(super) fn parallel() -> Routing {
    routing([0; 8], 0xFF, 1, 1)
}

/// One chain eight deep: 8 into 7 into ... into 1, feedback from the deepest back into the carrier.
pub(super) fn chain() -> Routing {
    routing([0b10, 0b100, 0b1000, 0b1_0000, 0b10_0000, 0b100_0000, 0b1000_0000, 0], 0b1, 8, 1)
}

/// Two stacks sharing a modulator: 5 into 2 into 1 and 5 into 4 into 3; carriers 1 and 3; feedback 2 into 2.
pub(super) fn shared() -> Routing {
    routing([0b10, 0b1_0000, 0b1000, 0b1_0000, 0, 0, 0, 0], 0b101, 2, 2)
}

/// A wire that skips a column: 3 into 2 into 1 and 3 into 1; 4 and 5 into 3; carriers 1 and 6 (6 alone); a
/// feedback from 1 into 4.
pub(super) fn skipping() -> Routing {
    routing([0b110, 0b100, 0b1_1000, 0, 0, 0, 0, 0], 0b10_0001, 1, 4)
}

/// An eight-operator plugin: the algorithm of each routing given, a level, ratio, fine and detune for every
/// operator, an ADSR envelope with times in seconds for the first four and a rate and level envelope for the others.
pub(super) fn fm_n(routings: &[Routing]) -> serde_json::Value {
    let mut algorithm = viewed(1, "Algorithm", routings.len() as f64 - 1., true, 1, "algorithm", 0);
    algorithm["algorithms"] = json!(routings.iter().map(|r| json!({"inputs": r[..8], "carriers": r[8], "feedbackFrom": r[9], "feedbackTo": r[10]})).collect::<Vec<_>>());
    let mut p = vec![algorithm, viewed(2, "Feedback", 7., true, 1, "feedback", 0)];
    for n in 1..=8u32 {
        p.push(viewed(100 + n, &format!("Level {n}"), 99., false, 1, "operatorLevel", n));
        p.push(viewed(200 + n, &format!("Ratio {n}"), 31., true, 1, "ratio", n));
        p.push(viewed(300 + n, &format!("Fine {n}"), 99., false, 1, "fine", n));
        p.push(viewed(350 + n, &format!("Detune {n}"), 14., true, 1, "detune", n));
        if n <= 4 {
            for (k, f) in ["attack", "decay", "sustain", "release"].iter().enumerate() {
                let mut q = viewed(500 + 4 * (n - 1) + k as u32, &format!("{f} {n}"), 1., false, 1, f, n);
                if *f != "sustain" {
                    q["seconds"] = json!([0.002, 0.2, 2.0]);
                }
                p.push(q);
            }
        } else {
            for k in 1..=4u32 {
                p.push(viewed(600 + 10 * n + k, &format!("Rate {k} {n}"), 99., false, 1, &format!("rate{k}"), n));
            }
            for k in 0..=4u32 {
                p.push(viewed(620 + 10 * n + k, &format!("Level {k} {n}"), 99., false, 1, &format!("level{k}"), n));
            }
        }
    }
    for q in &mut p {
        q["module"] = json!("FM");
    }
    json!({ "parameters": p })
}

pub(super) fn fm_n_text(id: u32, v: f64) -> String {
    match id {
        1 => format!("{}", v as i64 + 1),
        2 => format!("{}", v as i64),
        101..=108 => format!("{v:.0}"),
        201..=208 => format!("x{}", v as i64 / 2 + 1),
        301..=308 => format!("{:+.0}", v - 50.),
        351..=358 => format!("{:+.0}", v - 7.),
        _ => format!("{:.0} %", v * 100.),
    }
}

/// Levels from loud to quiet; operator 8 off (level 0) when `off`.
pub(super) fn fm_n_values(algorithm: f64, off: bool) -> Vec<(u32, f64)> {
    let mut v = vec![(1, algorithm), (2, 5.)];
    for n in 1..=8u32 {
        v.push((100 + n, if off && n == 8 { 0. } else { 99. - 9. * f64::from(n - 1) }));
        v.push((200 + n, f64::from(2 * n)));
        v.push((300 + n, 50. + f64::from(n) * 3.));
        v.push((350 + n, 7. + f64::from(n) - 4.));
        if n <= 4 {
            let (a, d, s, r) = (0.05 + 0.06 * f64::from(n), 0.3 + 0.08 * f64::from(n), 0.6 - 0.08 * f64::from(n), 0.2 + 0.1 * f64::from(n));
            v.extend([(500 + 4 * (n - 1), a), (501 + 4 * (n - 1), d), (502 + 4 * (n - 1), s), (503 + 4 * (n - 1), r)]);
        } else {
            for k in 1..=4u32 {
                v.push((600 + 10 * n + k, 90. - 12. * f64::from(k) - 4. * f64::from(n)));
            }
            for (k, l) in [0., 95., 70., 60., 0.].iter().enumerate() {
                v.push((620 + 10 * n + k as u32, *l));
            }
        }
    }
    v
}

/// A page to open: its name, the plugin, its values and how its parameters read.
pub(super) type Page = (&'static str, serde_json::Value, Vec<(u32, f64)>, fn(u32, f64) -> String);

/// The routings the view is tested on, with the plugin that has it as its algorithm 1, and its name.
pub(super) fn routings() -> Vec<Page> {
    vec![
        ("fm-four", fm_plugin(), fm_values(0.), fm_text),
        ("fm-eight", fm8_plugin(), fm8_values(0.), fm8_text),
        ("fm-parallel", fm_n(&[parallel()]), fm_n_values(0., true), fm_n_text),
        ("fm-chain", fm_n(&[chain()]), fm_n_values(0., false), fm_n_text),
        ("fm-shared", fm_n(&[shared()]), fm_n_values(0., false), fm_n_text),
        ("fm-skipping", fm_n(&[skipping()]), fm_n_values(0., false), fm_n_text),
    ]
}

#[test]
fn fm_pictures() {
    for (name, list, values, text) in routings() {
        pictures(&format!("grid-{name}"), list, &values, &text);
    }
}

fn flow_shape(f: &Flow) -> (usize, f32, usize) {
    (f.columns, f.rows, f.crossings())
}

#[test]
fn flows_have_the_carriers_in_the_last_column_and_no_crossings_where_none_are_needed() {
    let f = algorithm_layout(&parallel(), 8);
    assert_eq!(flow_shape(&f), (1, 8., 0), "eight in parallel: one column of eight rows");
    let f = algorithm_layout(&chain(), 8);
    assert_eq!(flow_shape(&f), (8, 1., 0), "a chain eight deep: eight columns, one row");
    let columns: Vec<usize> = f.nodes.iter().take(8).map(|n| n.column).collect();
    assert_eq!(columns, [7, 6, 5, 4, 3, 2, 1, 0], "operator 1, the carrier, is the rightmost; each modulator is one column left");
    let f = algorithm_layout(&shared(), 8);
    assert_eq!(f.crossings(), 0, "two stacks sharing a modulator");
    assert_eq!((f.nodes[4].column, f.nodes[1].column, f.nodes[0].column), (0, 1, 2), "5 into 2 into 1");
    let y = |n: usize| f.nodes[n].y;
    assert!((y(4) - (y(1) + y(3)) / 2.).abs() < 0.01, "the shared modulator sits between the two stacks it feeds");
    assert!(y(0) < y(2) && y(1) < y(3), "the stacks keep their order: 1 over 3, 2 over 4");
    let f = algorithm_layout(&skipping(), 8);
    assert_eq!(f.crossings(), 0);
    // 3 into 1 skips the column of 2: it passes through a point of its own there, a row away from 2.
    let through: Vec<_> = f.chains.iter().filter(|c| c.source == 2 && c.target == Some(0)).collect();
    assert_eq!(through.len(), 1);
    assert_eq!(through[0].nodes.len(), 3, "3, the point it passes in the column of 2, and 1");
    let point = f.nodes[through[0].nodes[1]];
    assert!(point.op.is_none() && point.column == f.nodes[1].column && (point.y - f.nodes[1].y).abs() >= 1., "its point and operator 2 are different rows of one column");
    // The eight-operator fixture's routing: a stack four deep, an operator fed by two, feedback to another.
    let fm8 = algorithm_layout(&[0b10, 0b100, 0b1000, 0, 0b110_0000, 0, 0, 0, 0b1001_0001, 7, 6], 8);
    assert_eq!(fm8.columns, 4);
    assert_eq!(fm8.crossings(), 0);
    assert_eq!(fm8.loops, [(6, 5)], "feedback from 7 into 6 is a loop");
    // Never two nodes of one column closer than a row.
    for f in [&f, &fm8, &algorithm_layout(&parallel(), 8), &algorithm_layout(&chain(), 8)] {
        for a in &f.nodes {
            for b in &f.nodes {
                if !std::ptr::eq(a, b) && a.column == b.column {
                    assert!((a.y - b.y).abs() >= 0.999, "{a:?} and {b:?} share a row");
                }
            }
        }
    }
}

#[test]
fn a_routing_that_cycles_is_cut_and_its_backward_wires_are_loops() {
    // 1 and 2 modulate each other, 1 is the carrier: every operator is placed, both wires of the cycle are loops.
    let f = algorithm_layout(&[0b10, 0b01, 0, 0, 0, 0, 0, 0, 0b1, 0, 0], 2);
    assert_eq!(f.nodes.iter().filter(|n| n.op.is_some()).count(), 2);
    assert_eq!(f.loops.len(), 2);
    // Operators past eight are not drawn; two that nothing hears are placed with the modulator first.
    assert_eq!(algorithm_layout(&[0; 11], 12).nodes.len(), 8);
    let f = algorithm_layout(&[0, 0b1, 0, 0, 0, 0, 0, 0, 0b100, 0, 0], 3);
    assert!(f.nodes[0].column < f.nodes[1].column, "operator 1 modulates 2, which nothing hears: 1 is left of 2");
}

// Every routing in every theme (the built-in ones and the two odd ones) at the keyboard's 1.55 and at 1280x720:
// pictures in target/theme-shots, fm-NAME-THEME@SCALE.png, for a look by eye.
#[test]
fn the_fm_view_draws_in_every_theme_at_both_scales() {
    fit_to_window();
    for (name, list, values, text) in routings() {
        let mut s = Screen::new();
        let mut p = open_with(&mut s, list, &values, &text);
        for scale in [1., 1.55] {
            s.set_scale(scale);
            for t in &all_themes() {
                theme::apply(&s.ui, t);
                s.frame();
                p.shown();
                assert!(s.frame().is_some() || scale == 1., "{name} in {}: drawn", t.id);
                shot(&s, &format!("fm-{name}-{}@{scale}", t.id));
            }
        }
    }
}
