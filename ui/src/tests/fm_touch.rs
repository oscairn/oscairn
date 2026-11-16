// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM view's operators chosen and edited: by touch (a box, a cell, a line), by the cursor on the tiles,
//! and the knobs under the screen following the choice; the envelope of the chosen operator edited by its
//! nodes. Fixtures in tests/fm.rs.

use super::fm::*;
use super::shots::*;
use super::views::{fm_plugin, fm_text, fm_values};
use super::*;
use crate::nav::Action;
use crate::views::Along;

fn nodes(panel: &crate::GridPanel) -> Vec<(f32, f32)> {
    panel.points.iter().map(|p| (p.x, p.y)).collect()
}

/// The diagram's box of operator `n` (1 to 8): its middle.
fn box_of(panel: &crate::GridPanel, n: usize) -> (f32, f32) {
    let b = panel.boxes.iter().find(|b| b.edge == 2 && b.text == n.to_string()).expect("the operator's box");
    (b.x + b.w / 2., b.y + b.h / 2.)
}

/// The strip's level bars (their tracks), by operator.
fn tracks(panel: &crate::GridPanel) -> Vec<crate::GridBox> {
    panel.boxes.iter().filter(|b| b.fill == 4 && b.edge == 5 && b.text.is_empty()).collect()
}

fn param_ids(sent: &[(&str, serde_json::Value)]) -> Vec<u64> {
    sent.iter().rfind(|o| o.0 == "controllers.view").map_or(vec![], |o| o.1["screen"]["params"].as_array().unwrap().iter().map(|p| p["param"].as_u64().unwrap()).collect())
}

fn set_ids(sent: &[(&str, serde_json::Value)]) -> Vec<u64> {
    sent.iter().filter(|o| o.0 == "params.set").map(|o| o.1["values"][0]["param"].as_u64().unwrap()).collect()
}

#[test]
fn a_tap_on_a_box_a_cell_or_a_line_chooses_the_operator_and_sets_nothing() {
    fit_to_window();
    let mut s = Screen::sized(1280, 720);
    theme::apply(&s.ui, &all_themes()[0]);
    let mut p = open_with(&mut s, fm_plugin(), &fm_values(1.), &fm_text);
    sent(&mut p.out);
    let first = nodes(&panels(&s.ui)[0]);
    assert!(!first.is_empty(), "the first operator's envelope has handles");
    // A tap on operator 3's box in the diagram.
    let at = box_of(&panels(&s.ui)[0], 3);
    assert!(p.view_down(0, at.0, at.1));
    p.view_up(0, at.0, at.1);
    let out = sent(&mut p.out);
    assert!(set_ids(&out).is_empty(), "a tap chooses and does not edit: {out:?}");
    let third = nodes(&panels(&s.ui)[0]);
    assert_ne!(first, third, "the handles are operator 3's now");
    assert_eq!(first.len(), third.len());
    // The knobs under the screen play operator 3's parameters first: its level, ratio and envelope.
    assert_eq!(&param_ids(&out)[..5], [13, 23, 46, 47, 48]);
    // A tap on operator 2's cell in the strip (the number's chip) chooses it.
    let chip = panels(&s.ui)[0].boxes.iter().filter(|b| b.edge == -1 && b.text == "2").last().expect("operator 2's chip");
    let at = (chip.x + chip.w / 2., chip.y + chip.h / 2.);
    assert!(p.view_down(0, at.0, at.1));
    p.view_up(0, at.0, at.1);
    let out = sent(&mut p.out);
    assert!(set_ids(&out).is_empty());
    assert_eq!(&param_ids(&out)[..2], [12, 22]);
    let second = nodes(&panels(&s.ui)[0]);
    assert!(second != first && second != third);
    // A tap on another operator's line in the graph chooses that one: operator 1's, at its first node's neighbor.
    let line = panels(&s.ui)[0].shapes.iter().filter(|s| s.ink == 16 && !s.fill).last().expect("operator 1's line").commands.to_string();
    let v = super::envelope::vertices(&line);
    let mid = ((v[1].0 + v[2].0) / 2., (v[1].1 + v[2].1) / 2.);
    assert!(p.view_down(0, mid.0 as f32, mid.1 as f32));
    p.view_up(0, mid.0 as f32, mid.1 as f32);
    assert_eq!(nodes(&panels(&s.ui)[0]), first, "operator 1 again");
    // A tap away from everything but the routing steps the algorithm (the old behavior, kept): left half, back.
    sent(&mut p.out);
    assert!(p.view_down(0, 14., 36.));
    p.view_up(0, 14., 36.);
    assert_eq!(set_ids(&sent(&mut p.out)), [1], "the algorithm");
}

#[test]
fn the_chosen_operators_nodes_are_dragged_and_no_other_operators_envelope_changes() {
    fit_to_window();
    let mut s = Screen::sized(1280, 720);
    theme::apply(&s.ui, &all_themes()[0]);
    let mut p = open_with(&mut s, fm_n(&[shared()]), &fm_n_values(0., false), &fm_n_text);
    sent(&mut p.out);
    // Choose operator 2 (ADSR: attack 504, decay 505, sustain 506, release 507).
    let at = box_of(&panels(&s.ui)[0], 2);
    p.view_down(0, at.0, at.1);
    p.view_up(0, at.0, at.1);
    sent(&mut p.out);
    let panel = panels(&s.ui)[0].clone();
    let n = nodes(&panel);
    assert_eq!(n.len(), 3, "attack, decay and release: the handles of operator 2 only");
    // The decay's node (the second) up and across: its time and the sustain level it reaches.
    let (x, y) = n[1];
    assert!(p.view_down(0, x, y));
    p.view_move(0, x + 12., y - 8.);
    p.view_up(0, x + 12., y - 8.);
    let out = sent(&mut p.out);
    let mut ids = set_ids(&out);
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids, [505, 506], "the decay time and the sustain level of operator 2, nothing else: {out:?}");
    assert!(out.iter().any(|o| o.0 == "params.gesture" && o.1["begin"] == true) && out.iter().any(|o| o.0 == "params.gesture" && o.1["begin"] == false));
    // Operator 6 has rates and levels: its envelope is edited the same way once chosen.
    let at = box_of(&panels(&s.ui)[0], 6);
    p.view_down(0, at.0, at.1);
    p.view_up(0, at.0, at.1);
    sent(&mut p.out);
    let n = nodes(&panels(&s.ui)[0]);
    assert_eq!(n.len(), 4, "four rate and level stages");
    let (x, y) = n[1];
    p.view_down(0, x, y);
    p.view_move(0, x - 6., y + 10.);
    p.view_up(0, x - 6., y + 10.);
    let mut ids = set_ids(&sent(&mut p.out));
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids, [662, 682], "rate 2 and level 2 of operator 6 (ids 600 + 10 n + k, 620 + 10 n + k)");
}

#[test]
fn the_cursor_on_an_operators_tile_chooses_it_and_rings_the_field_in_its_cell() {
    fit_to_window();
    let mut s = Screen::sized(1280, 720);
    theme::apply(&s.ui, &all_themes()[0]);
    let mut p = open_with(&mut s, fm_plugin(), &fm_values(0.), &fm_text);
    sent(&mut p.out);
    let rings = |s: &Screen| panels(&s.ui)[0].shapes.iter().filter(|x| x.ink == 12).count();
    assert_eq!(rings(&s), 0, "no cursor, no ring");
    let first = nodes(&panels(&s.ui)[0]);
    let names: Vec<String> = s.ui.global::<crate::GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect();
    let tile = names.iter().position(|n| n == "Ratio 3").expect("operator 3's ratio tile");
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Move { pane: None, by: tile as i32 });
    s.frame();
    assert_eq!(rings(&s), 1, "the field the cursor is on");
    assert_ne!(nodes(&panels(&s.ui)[0]), first, "operator 3 is the one chosen");
    let out = sent(&mut p.out);
    assert_eq!(&param_ids(&out)[..2], [13, 23], "the knobs play it");
    // Back to a tile of operator 1: its handles again.
    let level1 = names.iter().position(|n| n == "Level 1").unwrap();
    p.act(Action::Move { pane: None, by: level1 as i32 - tile as i32 });
    assert_eq!(nodes(&panels(&s.ui)[0]), first);
}

#[test]
fn a_level_bar_and_a_ratio_are_edited_in_the_strip_by_touch_as_before() {
    fit_to_window();
    let mut s = Screen::sized(1280, 720);
    theme::apply(&s.ui, &all_themes()[0]);
    let mut p = open_with(&mut s, fm_plugin(), &fm_values(1.), &fm_text);
    sent(&mut p.out);
    let panel = panels(&s.ui)[0].clone();
    let bars = tracks(&panel);
    assert_eq!(bars.len(), 4);
    let bar = &bars[1];
    assert!(p.view_down(0, bar.x + bar.w * 0.25, bar.y + bar.h / 2.));
    let out = sent(&mut p.out);
    let sets: Vec<f64> = out.iter().filter(|o| o.0 == "params.set").map(|o| o.1["values"][0]["value"].as_f64().unwrap()).collect();
    assert!(sets.len() == 1 && (sets[0] - 24.75).abs() < 0.01, "a quarter of the way across operator 2's bar: {sets:?}");
    assert_eq!(&param_ids(&out)[..2], [12, 22], "and operator 2 is the one in view");
    p.view_move(0, bar.x + bar.w + 500., bar.y);
    p.view_up(0, bar.x + bar.w + 500., bar.y);
    let out = sent(&mut p.out);
    assert!(out.contains(&set(12, 99.)) && out.contains(&gesture(12, false)), "dragged past the end: {out:?}");
    // A ratio up two steps (25 px).
    let ratio = panel.boxes.iter().filter(|b| b.fill == 4 && b.text.starts_with('x')).nth(3).expect("operator 4's ratio").clone();
    assert!(p.view_down(0, ratio.x + 2., ratio.y + ratio.h / 2.));
    p.view_move(0, ratio.x + 2., ratio.y + ratio.h / 2. - 25.);
    p.view_up(0, ratio.x + 2., ratio.y + ratio.h / 2. - 25.);
    let sets: Vec<serde_json::Value> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1).collect();
    assert_eq!(sets, [set(24, 3.).1], "25 px up is two steps: 1 + 2");
}

// The dots (the host's side: the graph takes positions along each operator's line and draws them; the
// plugin's side is a proposal, notes/ui-live-positions-proposal.md).
#[test]
fn dots_are_drawn_only_while_positions_are_given_and_cost_nothing_at_rest() {
    let (mut shot, mut p) = view_shot(&fm_n(&[shared()]), &fm_n_values(0., false), &fm_n_text, 0, (1280, 720), 1.);
    let before = shot.s.pixels.clone();
    let dots = |s: &Screen| panels(&s.ui)[0].shapes.iter().filter(|x| x.fill && x.commands.contains(" A ")).count();
    assert_eq!(dots(&shot.s), 0, "no positions, no dots");
    assert_eq!(shot.s.frame(), None, "and nothing is drawn at rest");
    let positions = vec![Some(Along::Path(0.3)), Some(Along::Segment(1, 0.5)), None, Some(Along::Path(1.)), None, None, None, None];
    p.set_positions(positions.clone());
    assert!(shot.s.frame().is_some(), "the dots are drawn");
    assert_eq!(dots(&shot.s), 6, "a halo and a dot for each of the three given");
    assert_ne!(before, shot.s.pixels);
    p.set_positions(positions);
    assert_eq!(shot.s.frame(), None, "the same positions again draw nothing");
    p.set_positions(vec![None; 8]);
    assert!(shot.s.frame().is_some(), "the dots go");
    assert_eq!(shot.s.pixels, before, "and the picture is what it was");
    assert_eq!(dots(&shot.s), 0);
    assert_eq!(shot.s.frame(), None);
}

// Dragging across the diagram steps the algorithm (24 px a step, whole steps, kept inside its range), inside
// one gesture; the same as the vertical view's routing.
#[test]
fn dragging_across_the_diagram_steps_the_algorithm() {
    fit_to_window();
    let mut s = Screen::sized(1280, 720);
    theme::apply(&s.ui, &all_themes()[0]);
    let mut p = open_with(&mut s, fm_plugin(), &fm_values(1.), &fm_text);
    sent(&mut p.out);
    assert!(p.view_down(0, 14., 36.));
    p.view_move(0, 44., 36.);
    p.view_move(0, 74., 36.);
    p.view_move(0, -16., 36.);
    p.view_move(0, 1014., 36.);
    p.view_up(0, 1014., 36.);
    let out = sent(&mut p.out);
    let sets: Vec<serde_json::Value> = out.iter().filter(|o| o.0 == "params.set").map(|o| o.1.clone()).collect();
    assert_eq!(sets, [set(1, 2.).1, set(1, 3.).1, set(1, 0.).1, set(1, 3.).1], "1 + 1, 1 + 2, 1 - 1, and the end");
    assert_eq!((out.first().unwrap(), out.last().unwrap()), (&gesture(1, true), &gesture(1, false)));
}

// An operator at level 0 is drawn dimmed (its box, its cell's chip, its line), every other in its own color;
// without routings in the data the operators stand in one column with no wires; a plugin whose operators have no
// envelope gets no graph and the diagram the whole width.
#[test]
fn an_operator_at_level_zero_is_dimmed_and_missing_data_leaves_the_rest_whole() {
    fit_to_window();
    let mut s = Screen::sized(1280, 720);
    theme::apply(&s.ui, &all_themes()[0]);
    let _p = open_with(&mut s, fm_n(&[parallel()]), &fm_n_values(0., true), &fm_n_text);
    let panel = &panels(&s.ui)[0];
    let ink_of = |n: usize| panel.boxes.iter().find(|b| b.edge == 2 && b.text == n.to_string()).unwrap().fill;
    assert_eq!((ink_of(1), ink_of(7), ink_of(8)), (16, 22, 39), "operator 8, at level 0, is the dimmed color");
    assert!(panel.shapes.iter().any(|x| x.ink == 39 && !x.fill), "and so is its line");
    let mut list = fm_n(&[parallel()]);
    list["parameters"][0]["algorithms"] = json!([]);
    let mut s = Screen::sized(1280, 720);
    theme::apply(&s.ui, &all_themes()[0]);
    let _q = open_with(&mut s, list, &fm_n_values(0., false), &fm_n_text);
    let panel = &panels(&s.ui)[0];
    let xs: Vec<f32> = panel.boxes.iter().filter(|b| b.edge == 2).map(|b| b.x).collect();
    assert_eq!(xs.len(), 8);
    assert!(xs.iter().all(|x| (x - xs[0]).abs() < 0.01), "one column");
    assert!(!panel.shapes.iter().any(|x| x.ink == 1), "no output line without routings");
    // No envelope fields at all: no graph (no frame), the boxes may use the width.
    let mut list = fm_n(&[shared()]);
    let all: Vec<serde_json::Value> = list["parameters"].as_array().unwrap().iter().filter(|p| !matches!(p["field"].as_str(), Some("attack" | "decay" | "sustain" | "release") | Some("rate1" | "rate2" | "rate3" | "rate4" | "level0" | "level1" | "level2" | "level3" | "level4"))).cloned().collect();
    list["parameters"] = json!(all);
    let wide = {
        let mut s = Screen::sized(1280, 720);
        theme::apply(&s.ui, &all_themes()[0]);
        let _r = open_with(&mut s, list, &fm_n_values(0., false), &fm_n_text);
        let panel = panels(&s.ui)[0].clone();
        assert!(!panel.shapes.iter().any(|x| x.ink == 5), "no graph without an envelope");
        panel.boxes.iter().filter(|b| b.edge == 2).map(|b| b.x + b.w).fold(0., f32::max)
    };
    let narrow = {
        let mut s = Screen::sized(1280, 720);
        theme::apply(&s.ui, &all_themes()[0]);
        let _r = open_with(&mut s, fm_n(&[shared()]), &fm_n_values(0., false), &fm_n_text);
        panels(&s.ui)[0].boxes.iter().filter(|b| b.edge == 2).map(|b| b.x + b.w).fold(0., f32::max)
    };
    assert!(wide > narrow, "the diagram is wider ({wide}) than beside a graph ({narrow})");
}

