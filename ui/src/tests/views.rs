// SPDX-License-Identifier: AGPL-3.0-or-later
//! The grid's rich views (src/views.rs, `ViewPanel` in slint/grid.slint) against the engine's words: touch
//! on a handle sets the parameters it stands for inside their gestures and nothing else does, the node the
//! cursor's tile sets is marked.

use super::shots::*;
use super::*;

// A node taken by touch and dragged sets its time (across) and its level (up and down) in gestures of its
// own; a touch away from every node takes nothing (the page scrolls) and changes nothing; the node the
// cursor's tile sets is marked, and only that.
#[test]
fn an_envelope_node_is_dragged_by_touch() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    // The envelope of the every-kind page alone (its Cutoff and Resonance would make a filter view too).
    let mut list = super::grid::every_kind();
    list["parameters"].as_array_mut().unwrap().retain(|p| p["view"] == json!(1));
    let mut p = super::grid::open(&mut s, list, &[(9, 0.1), (10, 0.4), (11, 0.6), (12, 0.5)]);
    sent(&mut p.out);
    let before = panels(ui)[0].clone();
    assert_eq!(before.points.row_count(), 3, "attack, decay, release");
    // Where a node is now.
    let node = |k: usize| {
        let q = panels(ui)[0].points.row_data(k).unwrap();
        (q.x, q.y)
    };
    // Away from every node: nothing is taken.
    assert!(!p.view_down(0, node(0).0 + 100., node(0).1 + 60.));
    assert!(sent(&mut p.out).is_empty(), "no gesture, no change");
    // The decay node carries the decay time and the sustain level; dragging it past the plot's top and right.
    let (x, y) = node(1);
    assert!(p.view_down(0, x + 3., y - 2.));
    p.view_move(0, 1000., -100.);
    p.view_up(0, 0., 0.);
    let out = sent(&mut p.out);
    assert_eq!(out.first().unwrap(), &gesture(10, true));
    assert!(out.contains(&gesture(11, true)));
    assert!(out.contains(&set(10, 1.)), "the time at the end of its segment: {out:?}");
    assert!(out.contains(&set(11, 1.)), "the level at the top");
    assert!(out.contains(&gesture(10, false)) && out.contains(&gesture(11, false)), "the gestures end with the touch");
    assert_eq!(out.iter().filter(|o| o.0 == "params.set").count(), 2, "only what the node stands for");
    // And the other way: to the left and the bottom.
    let (x, y) = node(1);
    assert!(p.view_down(0, x, y));
    p.view_move(0, -1000., 1000.);
    p.view_up(0, 0., 0.);
    let out = sent(&mut p.out);
    assert!(out.contains(&set(10, 0.)) && out.contains(&set(11, 0.)), "{out:?}");
    // A node with only a time (the attack: its level is fixed at full) sets only the time.
    let (x, y) = node(0);
    assert!(p.view_down(0, x, y));
    p.view_move(0, -1000., 0.);
    p.view_up(0, 0., 0.);
    let out = sent(&mut p.out);
    assert_eq!(out.iter().filter(|o| o.0 == "params.set").collect::<Vec<_>>(), [&set(9, 0.)], "only the time: {out:?}");
    assert!(out.contains(&gesture(9, true)) && !out.iter().any(|o| o.1["param"] == 11), "no gesture on a parameter it does not stand for");
    // The cursor's tile marks its node: nothing is marked until a tile with a node has the cursor.
    assert!(panels(ui)[0].points.iter().all(|q| !q.lit));
    p.act(crate::nav::Action::Move { pane: None, by: 1 });
    let tile = ui.global::<crate::GridState>().get_tiles().iter().find(|t| t.cursor).map(|t| t.name.to_string());
    assert_eq!(tile.as_deref(), Some("Attack"));
    let lit: Vec<bool> = panels(ui)[0].points.iter().map(|q| q.lit).collect();
    assert_eq!(lit, [true, false, false], "the attack's node");
    p.act(crate::nav::Action::Move { pane: None, by: 2 });
    let lit: Vec<bool> = panels(ui)[0].points.iter().map(|q| q.lit).collect();
    assert_eq!(lit, [false, true, false], "the sustain tile sets the decay node's level");
    s.frame();
    png(&s, "grid-envelope-cursor");
}

// The LFO plugin of plugins/lfo/lfo.cpp, by its hints: an LFO view recognized by the names Shape and Rate
// (and Phase) among parameters in no view.
fn lfo_plugin() -> serde_json::Value {
    json!({"parameters": [
        plugin_param(1, "Rate", 1., false, "knob", 1, 1),
        plugin_param(2, "Sync", 1., true, "switch", 4, 1),
        plugin_param(3, "Synced rate", 7., true, "list", 5, 1),
        plugin_param(4, "Shape", 4., true, "buttons", 3, 0),
        plugin_param(5, "Depth", 1., false, "knob", 2, 0),
        plugin_param(6, "Phase", 1., false, "knob", 6, 0),
        plugin_param(7, "Retrigger", 1., true, "switch", 7, 0),
        plugin_param(8, "Polarity", 1., true, "buttons", 8, 0),
        plugin_param(9, "Mode", 1., true, "buttons", 9, 0),
    ]})
}

fn lfo_text(id: u32, v: f64) -> String {
    match id {
        4 => ["Sine", "Triangle", "Saw", "Square", "Sample and hold"].get(v as usize).copied().unwrap_or("?").into(),
        3 => ["4 bars", "2 bars", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/32"].get(v as usize).copied().unwrap_or("?").into(),
        2 | 7 => if v > 0. { "On" } else { "Off" }.into(),
        8 => if v > 0. { "Unipolar" } else { "Bipolar" }.into(),
        9 => if v > 0. { "Set" } else { "Modulate" }.into(),
        _ => format!("{:.1} Hz", 0.1 + v * 20.),
    }
}

fn lfo_values(shape: f64) -> Vec<(u32, f64)> {
    vec![(1, 0.35), (2, 0.), (3, 4.), (4, shape), (5, 1.), (6, 0.), (7, 0.), (8, 0.), (9, 0.)]
}

// An LFO is drawn above the tiles, titled by its wave's text, from the Shape and Rate parameters; its
// tiles keep Rate with Sync (the plugin's group) and the Shape and Phase the view reads join as a run.
#[test]
fn an_lfo_is_recognized_by_name_and_drawn_above_its_tiles() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let mut p = open_with(&mut s, lfo_plugin(), &lfo_values(1.), &lfo_text);
    let panel = &panels(ui)[0];
    assert_eq!(panel.title.as_str(), "LFO  \u{b7}  Triangle");
    assert_eq!(panels(ui).len(), 1);
    assert_eq!(panel.h, 140., "the reference's height in touch mode");
    assert_eq!(panel.shapes.row_count(), 2, "the baseline and the wave");
    assert_eq!(panel.points.row_count(), 0, "a preview: nothing to take");
    assert!(!p.view_down(0, 100., 80.), "a touch on it takes nothing");
    let tiles: Vec<crate::GridTile> = ui.global::<crate::GridState>().get_tiles().iter().collect();
    let linked: Vec<(String, bool)> = tiles.iter().map(|t| (t.name.to_string(), t.linked)).collect();
    assert!(linked.contains(&("Shape".into(), true)) && linked.contains(&("Phase".into(), true)), "the wave and phase run: {linked:?}");
    assert!(linked.contains(&("Rate".into(), true)) && linked.contains(&("Sync".into(), true)), "the plugin's group: {linked:?}");
    assert!(linked.contains(&("Depth".into(), false)), "{linked:?}");
    // Another wave, another title and wave; a wave the reference does not know is said not drawn.
    let wave = |shape: f64, text: &dyn Fn(u32, f64) -> String| {
        let mut s = Screen::new();
        let ui = s.ui.clone_strong();
        let _p = open_with(&mut s, lfo_plugin(), &lfo_values(shape), text);
        let panel = panels(&ui)[0].clone();
        (panel.title.to_string(), panel.shapes.iter().map(|s| s.commands.to_string()).collect::<Vec<_>>(), panel.labels.iter().map(|l| l.text.to_string()).collect::<Vec<_>>())
    };
    let (sine, saw) = (wave(0., &lfo_text), wave(2., &lfo_text));
    assert_ne!(sine.1[1], saw.1[1], "each wave its own curve");
    assert!(sine.0.ends_with("Sine") && saw.0.ends_with("Saw") && sine.2.is_empty());
    let odd = wave(0., &|id, v| if id == 4 { "Wobble".into() } else { lfo_text(id, v) });
    assert_eq!(odd.1.len(), 1, "only the baseline");
    assert_eq!(odd.2, ["Drawn by the plugin"]);
}

// The wave's shapes, delay, fade and phase, from the reference's formulas.
#[test]
fn the_lfo_waves_follow_the_reference() {
    use crate::views::{lfo_at, wave_named, LfoRun, Wave};
    let none = |_: usize| 0.;
    let at = |kind, x, cycles, phase, delay, fade| lfo_at(kind, x, LfoRun { cycles, phase, delay, fade }, &none, 0);
    assert!((at(Wave::Sine, 0.25, 1., 0., 0., 0.) - 1.).abs() < 1e-9, "a sine peaks a quarter in");
    assert!((at(Wave::Triangle, 0.25, 1., 0., 0., 0.) - 1.).abs() < 1e-9 && (at(Wave::Triangle, 0.75, 1., 0., 0., 0.) + 1.).abs() < 1e-9);
    assert_eq!((at(Wave::Square, 0.2, 1., 0., 0., 0.), at(Wave::Square, 0.7, 1., 0., 0., 0.)), (1., -1.));
    assert_eq!((at(Wave::SawUp, 0., 1., 0., 0., 0.), at(Wave::SawDown, 0., 1., 0., 0., 0.)), (-1., 1.));
    assert_eq!(at(Wave::Sine, 0.1, 1., 0., 0.25, 0.), 0., "nothing before the delay");
    let fading = at(Wave::Square, 0.3, 1., 0., 0.25, 0.25);
    assert!((fading - 0.2).abs() < 1e-9, "faded in a fifth of the way: {fading}");
    assert_eq!(at(Wave::Square, 0.3, 1., 0.5, 0., 0.), -1., "a phase of a half starts in the other half");
    assert_eq!(at(Wave::Random, 0.3, 3., 0., 0., 0.), at(Wave::Random, 0.3, 3., 0., 0., 0.), "the same steps each time");
    let steps = |k: usize| [0., 1.][k];
    let one = LfoRun { cycles: 1., phase: 0., delay: 0., fade: 0. };
    assert_eq!((lfo_at(Wave::Steps, 0.1, one, &steps, 2), lfo_at(Wave::Steps, 0.9, one, &steps, 2)), (-1., 1.));
    for (text, wave) in [("Sine", Wave::Sine), ("Triangle", Wave::Triangle), ("Pulse width", Wave::Square), ("Saw down", Wave::SawDown), ("Sawtooth", Wave::SawUp), ("Sample and hold", Wave::Random), ("S&H", Wave::Random), ("User", Wave::Steps), ("Wobble", Wave::None)] {
        assert_eq!(wave_named(text), wave, "{text}");
    }
}

// A synth's filter section: Cutoff, Resonance and a Type, found by their names among parameters in no view.
fn filter_plugin() -> serde_json::Value {
    json!({"parameters": [
        plugin_param(1, "Cutoff", 1., false, "knob", 1, 0),
        plugin_param(2, "Resonance", 1., false, "knob", 2, 0),
        plugin_param(3, "Type", 5., true, "list", 3, 0),
        plugin_param(4, "Drive", 1., false, "knob", 4, 0),
    ]})
}

fn filter_text(id: u32, v: f64) -> String {
    match id {
        3 => ["LP 12 dB", "LP 24 dB", "HP 12 dB", "BPF", "Notch", "Mystery"].get(v as usize).copied().unwrap_or("?").into(),
        1 => format!("{:.0} Hz", 20. * 1000f64.powf(v)),
        _ => format!("{:.0} %", v * 100.),
    }
}

fn filter_values(kind: f64) -> Vec<(u32, f64)> {
    vec![(1, 0.55), (2, 0.4), (3, kind), (4, 0.)]
}

// A filter is drawn above the tiles, titled with its type's text; Type, Cutoff and Resonance are one
// run; a type the reference cannot read as a response is said not drawn.
#[test]
fn a_filter_is_recognized_by_name_and_drawn_from_its_type() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let _p = open_with(&mut s, filter_plugin(), &filter_values(0.), &filter_text);
    let panel = &panels(ui)[0];
    assert_eq!(panel.title.as_str(), "Filter  \u{b7}  LP 12 dB");
    assert_eq!(panel.h, 160.);
    assert_eq!(panel.shapes.row_count(), 3, "the frame, the fill and the response");
    let tiles: Vec<crate::GridTile> = ui.global::<crate::GridState>().get_tiles().iter().collect();
    let order: Vec<(String, bool)> = tiles.iter().map(|t| (t.name.to_string(), t.linked)).collect();
    assert_eq!(order[..3], [("Type".to_string(), true), ("Cutoff".to_string(), true), ("Resonance".to_string(), true)], "the run, in the filter's order: {order:?}");
    // The curve and its fill are one set of points: the fill is the line closed along the bottom.
    let (line, fill) = (super::envelope::vertices(&panel.shapes.row_data(2).unwrap().commands), super::envelope::vertices(&panel.shapes.row_data(1).unwrap().commands));
    assert_eq!(fill.len(), line.len() + 2);
    assert_eq!(&fill[1..=line.len()], &line[..]);
    assert_eq!((fill[0].1, fill[0].0), (fill[line.len() + 1].1, line[0].0), "closed along the bottom, from under its first point");
    assert!(line[0].1 < fill[0].1 && line.iter().all(|p| p.1 >= 10. && p.1 <= fill[0].1), "inside the plot");
    // A type that is no response: said so, and nothing is drawn but the frame.
    let mut s = Screen::new();
    let _q = open_with(&mut s, filter_plugin(), &filter_values(5.), &filter_text);
    let odd = &panels(&s.ui)[0];
    assert_eq!(odd.shapes.row_count(), 1);
    assert_eq!(odd.labels.row_data(0).unwrap().text.as_str(), "Drawn by the plugin");
    // With no Type parameter the response is a low-pass.
    let mut list = filter_plugin();
    list["parameters"].as_array_mut().unwrap().remove(2);
    let mut s = Screen::new();
    let _r = open_with(&mut s, list, &filter_values(0.), &filter_text);
    assert_eq!(panels(&s.ui)[0].title.as_str(), "Filter");
    assert_eq!(panels(&s.ui)[0].shapes.row_count(), 3);
}

// Type names as responses, and the response itself: the low-pass is down 3 dB at its cutoff at the
// plain quality, the band-pass is flat there, the notch is deep, each pole adds a slope.
#[test]
fn the_filter_response_follows_the_reference() {
    use crate::views::{filter_db, filter_named, Response as R};
    for (text, kind, poles) in [("LP 24 dB", R::LowPass, 4), ("LPF24D", R::LowPass, 4), ("HP 12 dB", R::HighPass, 2), ("LP 18", R::LowPass, 3), ("BPF", R::BandPass, 2), ("LP+HP", R::BandPass, 2), ("BEF", R::Notch, 2), ("Notch", R::Notch, 2), ("Off", R::None, 2), ("Thru", R::None, 2), ("Comb", R::Unknown, 2), ("LP 6 dB", R::LowPass, 1)] {
        assert_eq!(filter_named(text), (kind, poles), "{text}");
    }
    let q = 0.707;
    assert!((filter_db(R::LowPass, 2, 1., q) + 3.0).abs() < 0.05, "-3 dB at the cutoff");
    assert!(filter_db(R::LowPass, 2, 0.01, q).abs() < 0.01, "flat below");
    assert!((filter_db(R::LowPass, 2, 10., q) + 40.).abs() < 0.5, "12 dB per octave: about -40 a decade up");
    assert_eq!(filter_db(R::LowPass, 4, 10., q), -48., "four poles: past the floor");
    assert!(filter_db(R::HighPass, 2, 0.1, q) < -35. && filter_db(R::HighPass, 2, 100., q).abs() < 0.01);
    assert!(filter_db(R::BandPass, 2, 1., q).abs() < 1e-9, "flat at the center");
    assert_eq!(filter_db(R::Notch, 2, 1., q), -48., "the notch is deep");
    assert_eq!(filter_db(R::None, 2, 1., q), 0.);
    assert_eq!(filter_db(R::LowPass, 2, 1., 80.), 24., "clamped at +24 dB");
}

/// A parameter that is part of a view: its number, its field's name and the zone or element it is of.
#[allow(clippy::too_many_arguments)]
fn viewed(id: u32, name: &str, max: f64, stepped: bool, view: u32, field: &str, index: u32) -> serde_json::Value {
    let mut p = plugin_param(id, name, max, stepped, "auto", 0, 0);
    p["view"] = json!(view);
    p["field"] = json!(field);
    p["index"] = json!(index);
    p
}

// A sampler-like key map: two zones (the first is the one shown, kept in the top left away from the
// scaling line), the first's level scaling about a breakpoint with a depth and a curve a side, and
// velocity's depth.
fn map_plugin() -> serde_json::Value {
    json!({"parameters": [
        viewed(1, "Low key 1", 127., true, 1, "keyLow", 1),
        viewed(2, "High key 1", 127., true, 1, "keyHigh", 1),
        viewed(3, "Low velocity 1", 127., true, 1, "velocityLow", 1),
        viewed(4, "High velocity 1", 127., true, 1, "velocityHigh", 1),
        viewed(5, "Low key 2", 127., true, 1, "keyLow", 2),
        viewed(6, "High key 2", 127., true, 1, "keyHigh", 2),
        viewed(7, "Breakpoint", 127., true, 1, "breakpoint", 1),
        viewed(8, "Left depth", 99., false, 1, "leftDepth", 1),
        viewed(9, "Right depth", 99., false, 1, "rightDepth", 1),
        viewed(10, "Left curve", 3., true, 1, "leftCurve", 1),
        viewed(11, "Right curve", 3., true, 1, "rightCurve", 1),
        viewed(12, "Velocity depth", 7., false, 1, "velocityDepth", 1),
    ]})
}

fn map_text(id: u32, v: f64) -> String {
    match id {
        10 | 11 => ["-LIN", "-EXP", "+EXP", "+LIN"].get(v as usize).copied().unwrap_or("?").into(),
        7 => format!("C{}", (v as i64) / 12 - 1),
        _ => format!("{v:.0}"),
    }
}

fn map_values() -> Vec<(u32, f64)> {
    vec![(1, 10.), (2, 40.), (3, 90.), (4, 127.), (5, 50.), (6, 100.), (7, 64.), (8, 39.6), (9, 69.3), (10, 1.), (11, 3.), (12, 3.5)]
}

/// The plane of a key map as it was drawn: from its frame (the third shape, a rectangle drawn inside its edge).
fn plane_of(panel: &crate::GridPanel) -> (f32, f32, f32, f32) {
    let v = super::envelope::vertices(&panel.shapes.row_data(2).unwrap().commands);
    ((v[0].0 - 0.5) as f32, (v[0].1 - 0.5) as f32, (v[2].0 + 0.5) as f32, (v[2].1 + 0.5) as f32)
}

// A map is drawn for each zone's index that the page carries (its own zone and scaling in the accent), at
// the reference's height, with a handle for each edge, the breakpoint, the depths and the velocity line.
#[test]
fn a_key_map_is_described_by_the_plugin_and_drawn_per_index() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let _p = open_with(&mut s, map_plugin(), &map_values(), &map_text);
    let all = panels(ui);
    assert_eq!(all.len(), 2, "one map per index");
    assert!(all.iter().all(|p| p.title.as_str() == "Key map" && p.h == 180.));
    assert_eq!((all[0].points.row_count(), all[1].points.row_count()), (8, 2), "4 zone edges, breakpoint, 2 depths, velocity; 2 key edges of the other zone");
    let labels: Vec<String> = all[0].labels.iter().map(|l| l.text.to_string()).collect();
    assert_eq!(labels, ["1", "2", "C4", "Velocity"], "the zones numbered, the breakpoint's text, the velocity box");
    // The scaling at the plane's ends: the left depth is down (a minus curve) by its depth at the plane's
    // end, the right is up by its depth (a plus curve); the breakpoint handle is on the middle line.
    let (l, t, r, b) = plane_of(&all[0]);
    let mid = (t + b) / 2.;
    let handle = |k: usize| {
        let q = all[0].points.row_data(k).unwrap();
        (q.x, q.y)
    };
    let (bp, left, right) = (handle(4), handle(5), handle(6));
    assert!((bp.1 - mid).abs() < 0.01 && (bp.0 - (l + 64. / 127. * (r - l))).abs() < 0.01);
    assert!((left.0 - l).abs() < 0.01 && (left.1 - (mid + 0.4 * (b - t) / 2.)).abs() < 0.01, "-EXP: lowered by .4 of the half height at the end: {left:?}");
    assert!((right.0 - r).abs() < 0.01 && (right.1 - (mid - 0.7 * (b - t) / 2.)).abs() < 0.01, "+LIN: raised by .7: {right:?}");
}

// Touch on a map's handle sets its parameter inside a gesture: a zone's edge across or up, the breakpoint
// across, a depth by its distance from the middle line, the velocity's depth down the box; at the edges of
// the plane the values stop; a touch away from every handle takes nothing.
#[test]
fn a_key_map_is_dragged_by_touch() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let mut p = open_with(&mut s, map_plugin(), &map_values(), &map_text);
    sent(&mut p.out);
    let at = |k: usize| {
        let q = panels(ui)[0].points.row_data(k).unwrap();
        (q.x, q.y)
    };
    let (l, t, r, b) = plane_of(&panels(ui)[0]);
    let mid = (t + b) / 2.;
    assert!(!p.view_down(0, l + 3., t - 12.), "away from every handle");
    // The first zone's low key (handle 0): across, to the plane's far left and right.
    let (x, y) = at(0);
    assert!(p.view_down(0, x + 2., y + 1.));
    p.view_move(0, l - 500., y);
    p.view_move(0, r + 500., y);
    p.view_up(0, 0., 0.);
    let out = sent(&mut p.out);
    assert_eq!(out.iter().filter(|o| o.0 == "params.set").map(|o| o.1.clone()).collect::<Vec<_>>(), [set(1, 0.).1, set(1, 127.).1]);
    assert_eq!((out.first().unwrap(), out.last().unwrap()), (&gesture(1, true), &gesture(1, false)));
    // Its high velocity (handle 2): up, to the top and the bottom of the plane.
    let (x, y) = at(2);
    assert!(p.view_down(0, x, y));
    p.view_move(0, x, b + 500.);
    p.view_move(0, x, t - 500.);
    p.view_up(0, 0., 0.);
    let sets: Vec<serde_json::Value> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1).collect();
    assert_eq!(sets, [set(4, 0.).1, set(4, 127.).1]);
    // The left depth (handle 5): the middle line is none, the top or bottom of the plane the most.
    let (x, y) = at(5);
    assert!(p.view_down(0, x + 1., y));
    p.view_move(0, x, mid);
    p.view_move(0, x, t);
    p.view_up(0, 0., 0.);
    let sets: Vec<f64> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1["values"][0]["value"].as_f64().unwrap()).collect();
    assert!(sets.len() == 2 && sets[0] == 0. && (sets[1] - 99.).abs() < 1e-3, "{sets:?}");
    // The velocity line's soft end (handle 7): down the box.
    let (x, y) = at(7);
    assert!(p.view_down(0, x + 1., y));
    p.view_move(0, x, t - 500.);
    p.view_move(0, x, b + 500.);
    p.view_up(0, 0., 0.);
    let sets: Vec<serde_json::Value> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1).collect();
    assert_eq!(sets, [set(12, 0.).1, set(12, 7.).1]);
    // The breakpoint marks its tile when the cursor is on it.
    assert!(panels(ui)[0].points.iter().all(|q| !q.lit));
}

// The scaling line of a key map is drawn from its own points, unscaled, as the others are.
#[test]
fn the_key_map_scaling_line_is_drawn_where_its_path_is() {
    let mut list = map_plugin();
    // The shown zone in the top left, the other muted: away from the scaling line, whose ends sit about the middle.
    list["parameters"].as_array_mut().unwrap().retain(|p| p["id"].as_u64().unwrap() != 5 && p["id"].as_u64().unwrap() != 6);
    let mut values = map_values();
    values.retain(|v| v.0 != 5 && v.0 != 6);
    values[1].1 = 30.; // the high key
    values[2].1 = 100.; // the low velocity
    // Right depth and curves: a straight right and a curved left.
    // Shapes: the keys, the black keys, the frame, the zone (fill and edge), the breakpoint, the scaling line.
    let report = check_line("scaling", &list, &values, &map_text, 0, 6);
    println!("{report}");
}

// A four-operator FM synth: an algorithm of four routings (a chain, two stacks, all carriers, three into
// one), the feedback, a level and a ratio and an envelope per operator.
fn fm_plugin() -> serde_json::Value {
    let routing = |inputs: [u8; 4], carriers: u8, from: u8, to: u8| json!({"inputs": [inputs[0], inputs[1], inputs[2], inputs[3], 0, 0, 0, 0], "carriers": carriers, "feedbackFrom": from, "feedbackTo": to});
    let mut algorithm = viewed(1, "Algorithm", 3., true, 1, "algorithm", 0);
    algorithm["algorithms"] = json!([
        routing([0b10, 0b100, 0b1000, 0], 0b1, 4, 4),
        routing([0b10, 0, 0b1000, 0], 0b101, 2, 2),
        routing([0, 0, 0, 0], 0b1111, 0, 0),
        routing([0b1110, 0, 0, 0], 0b1, 0, 0),
    ]);
    let mut p = vec![algorithm, viewed(2, "Feedback", 7., true, 1, "feedback", 0)];
    for n in 1..=4u32 {
        p.push(viewed(10 + n, &format!("Level {n}"), 99., false, 1, "operatorLevel", n));
        p.push(viewed(20 + n, &format!("Ratio {n}"), 31., true, 1, "ratio", n));
    }
    for n in 1..=4u32 {
        for (k, f) in ["attack", "decay", "release"].iter().enumerate() {
            p.push(viewed(40 + 3 * (n - 1) + k as u32, &format!("{f} {n}"), 1., false, 1, f, n));
        }
    }
    json!({ "parameters": p })
}

fn fm_text(id: u32, v: f64) -> String {
    match id {
        1 => format!("{}", v as i64 + 1),
        2 => format!("{}", v as i64),
        11..=14 => format!("{v:.0}"),
        21..=24 => format!("x{}", v as i64 / 2 + 1),
        _ => format!("{:.0} %", v * 100.),
    }
}

fn fm_values(algorithm: f64) -> Vec<(u32, f64)> {
    let mut v = vec![(1, algorithm), (2, 5.), (11, 99.), (12, 70.), (13, 40.), (14, 10.), (21, 2.), (22, 4.), (23, 8.), (24, 1.)];
    v.extend([(40, 0.05), (41, 0.4), (42, 0.3), (43, 0.1), (44, 0.5), (45, 0.4), (46, 0.3), (47, 0.3), (48, 0.2), (49, 0.2), (50, 0.4), (51, 0.2)]);
    v
}

// The algorithm layouts of the reference: each carrier on the bottom row, each modulator above what it
// modulates, centered, pushed right until none overlap.
#[test]
fn algorithms_are_laid_out_as_the_reference_does() {
    use crate::views::algorithm_layout;
    let pos = |r: [u8; 11], n| algorithm_layout(&r, n).iter().map(|p| (p.row, p.x)).collect::<Vec<_>>();
    // 4 -> 3 -> 2 -> 1, feedback aside: a column of four.
    assert_eq!(pos([0b10, 0b100, 0b1000, 0, 0, 0, 0, 0, 0b1, 4, 4], 4), [(0, 0.), (1, 0.), (2, 0.), (3, 0.)]);
    // 2 -> 1 and 4 -> 3: two stacks, centered about the middle.
    assert_eq!(pos([0b10, 0, 0b1000, 0, 0, 0, 0, 0, 0b101, 0, 0], 4), [(0, -0.5), (1, -0.5), (0, 0.5), (1, 0.5)]);
    // All carriers: one row.
    assert_eq!(pos([0; 11], 4), [(0, 0.); 4].iter().enumerate().map(|(i, _)| (0, i as f32 - 1.5)).collect::<Vec<_>>());
    let mut all = [0u8; 11];
    all[8] = 0b1111;
    assert_eq!(pos(all, 4), [(0, -1.5), (0, -0.5), (0, 0.5), (0, 1.5)]);
    // 2, 3 and 4 into 1: the three modulators side by side over it.
    assert_eq!(pos([0b1110, 0, 0, 0, 0, 0, 0, 0, 0b1, 0, 0], 4), [(0, -1.), (1, -1.), (1, 0.), (1, 1.)]);
    // More than eight are not drawn; a loop (1 and 2 into each other) is cut and every operator is placed.
    assert_eq!(algorithm_layout(&[0; 11], 12).len(), 8);
    assert_eq!(algorithm_layout(&[0b10, 0b01, 0, 0, 0, 0, 0, 0, 0b1, 0, 0], 2).len(), 2);
}

// The FM view is described by the plugin's data: found beside the algorithm, titled with its algorithm's
// and feedback's text, with the routing drawn, a column per operator, and the operators' envelopes drawn
// as views of their own too (as the reference does). An operator the plugin gives no envelope draws none.
#[test]
fn an_fm_view_is_described_by_the_plugin_and_drawn() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let _p = open_with(&mut s, fm_plugin(), &fm_values(0.), &fm_text);
    let all = panels(ui);
    assert_eq!(all.len(), 5, "the operators, then each described envelope (one per operator)");
    assert_eq!((all[0].title.as_str(), all[0].h), ("Algorithm 1  \u{b7}  Feedback 5", 300.));
    assert_eq!(all[1].title.as_str(), "Envelope");
    let boxes: Vec<crate::GridBox> = all[0].boxes.iter().collect();
    let numbered: Vec<(&str, i32)> = boxes.iter().take(4).map(|b| (b.text.as_str(), b.fill)).collect();
    assert_eq!(numbered, [("1", 0), ("2", 4), ("3", 4), ("4", 4)], "operator 1 is the carrier of the chain, filled in the accent");
    // Operator 4's ratio shows the plugin's text; a column per operator named Op n.
    let ratios: Vec<String> = boxes.iter().filter(|b| b.fill == 4 && b.text.starts_with('x')).map(|b| b.text.to_string()).collect();
    assert_eq!(ratios, ["x2", "x3", "x5", "x1"]);
    let names: Vec<String> = all[0].labels.iter().filter(|l| l.text.starts_with("Op ")).map(|l| l.text.to_string()).collect();
    assert_eq!(names, ["Op 1", "Op 2", "Op 3", "Op 4"]);
    // The routing: wires (the chain's three), the feedback loop, the output line; every column carries an envelope.
    let ink = |k: usize| all[0].shapes.row_data(k).map(|s| (s.ink, s.width));
    assert_eq!((ink(1), ink(2)), (Some((1, 1)), Some((0, 1))), "the wires in the muted ink, then the feedback in the accent");
    assert_eq!(all[0].shapes.row_count(), 4 + 2 * 4, "frame, wires, feedback, output line, and a frame and line for four envelopes");
    // Another algorithm: a different routing (four carriers, no wires or feedback).
    let mut s = Screen::new();
    let _q = open_with(&mut s, fm_plugin(), &fm_values(2.), &fm_text);
    let all = panels(&s.ui);
    assert_eq!(all[0].title.as_str(), "Algorithm 3  \u{b7}  Feedback 5");
    assert!(all[0].boxes.iter().take(4).all(|b| b.fill == 0), "four carriers");
    // A plugin that gives no routings: the operators in a row, nothing but the frame to say how they connect.
    let mut list = fm_plugin();
    list["parameters"][0]["algorithms"] = json!([]);
    let mut s = Screen::new();
    let _r = open_with(&mut s, list, &fm_values(0.), &fm_text);
    let all = panels(&s.ui);
    assert!(all[0].boxes.iter().take(4).all(|b| b.fill == 4));
    let xs: Vec<f32> = all[0].boxes.iter().take(4).map(|b| b.x).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]) && (xs[1] - xs[0] - (xs[2] - xs[1])).abs() < 0.01, "in a row, evenly: {xs:?}");
}

// Touch: dragging across the routing steps the algorithm (24 px a step, whole steps), a tap on its left or
// right half steps it back or on; a level bar sets its level where it is touched and as it is dragged; a
// ratio steps by 12 px up or down; each inside gestures; values stop at the ends.
#[test]
fn an_fm_view_is_edited_by_touch() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let mut p = open_with(&mut s, fm_plugin(), &fm_values(1.), &fm_text);
    sent(&mut p.out);
    let routing = {
        let v = super::envelope::vertices(&panels(ui)[0].shapes.row_data(0).unwrap().commands);
        ((v[0].0 - 0.5) as f32, (v[0].1 - 0.5) as f32, (v[2].0 + 0.5) as f32, (v[2].1 + 0.5) as f32)
    };
    let mid = ((routing.0 + routing.2) / 2., (routing.1 + routing.3) / 2.);
    // Across the routing: 30 px right is one step on, 60 px two, back past the start one step off.
    assert!(p.view_down(0, mid.0, mid.1));
    p.view_move(0, mid.0 + 30., mid.1);
    p.view_move(0, mid.0 + 60., mid.1);
    p.view_move(0, mid.0 - 30., mid.1);
    p.view_move(0, mid.0 + 1000., mid.1);
    p.view_up(0, mid.0 + 1000., mid.1);
    let out = sent(&mut p.out);
    let sets: Vec<serde_json::Value> = out.iter().filter(|o| o.0 == "params.set").map(|o| o.1.clone()).collect();
    assert_eq!(sets, [set(1, 2.).1, set(1, 3.).1, set(1, 0.).1, set(1, 3.).1], "1 + 1, 1 + 2, 1 - 1, and the end");
    assert_eq!((out.first().unwrap(), out.last().unwrap()), (&gesture(1, true), &gesture(1, false)));
    // A tap on the left half steps back, on the right half on.
    p.view_down(0, routing.0 + 20., mid.1);
    p.view_up(0, routing.0 + 20., mid.1);
    let sets: Vec<serde_json::Value> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1).collect();
    assert_eq!(sets, [set(1, 2.).1], "from 3 back to 2");
    p.view_down(0, routing.2 - 20., mid.1);
    p.view_up(0, routing.2 - 20., mid.1);
    let sets: Vec<serde_json::Value> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1).collect();
    assert_eq!(sets, [set(1, 3.).1], "from 2 on to 3");
    // The level bars and ratios, found as the view drew them.
    let boxes: Vec<crate::GridBox> = panels(ui)[0].boxes.iter().collect();
    let bar = boxes.iter().filter(|b| b.fill == 4 && b.text.is_empty()).nth(1).expect("operator 2's level bar").clone();
    assert!(p.view_down(0, bar.x + bar.w / 2., bar.y + bar.h * 0.25));
    let sets: Vec<f64> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1["values"][0]["value"].as_f64().unwrap()).collect();
    assert!(sets.len() == 1 && (sets[0] - 74.25).abs() < 0.01, "touched a quarter of the way down: {sets:?}");
    p.view_move(0, bar.x, bar.y - 500.);
    p.view_up(0, bar.x, bar.y - 500.);
    let out = sent(&mut p.out);
    assert!(out.contains(&set(12, 99.)) && out.contains(&gesture(12, false)), "dragged up past the top: {out:?}");
    let ratio = boxes.iter().filter(|b| b.fill == 4 && b.text.starts_with('x')).nth(3).expect("operator 4's ratio").clone();
    assert!(p.view_down(0, ratio.x + 2., ratio.y + ratio.h / 2.));
    p.view_move(0, ratio.x + 2., ratio.y + ratio.h / 2. - 25.);
    p.view_up(0, ratio.x + 2., ratio.y + ratio.h / 2. - 25.);
    let sets: Vec<serde_json::Value> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").map(|o| o.1).collect();
    assert_eq!(sets, [set(24, 3.).1], "25 px up is two steps: 1 + 2");
    assert!(!p.view_down(0, routing.0 + 5., routing.1 - 30.), "away from the routing and every bar takes nothing");
}

// The FM view's lines in the accent (the feedback loop, each operator's envelope) are drawn where their
// paths are, unscaled, at two sizes and two scales; they are short, so measured every 2 px.
#[test]
fn the_fm_lines_are_drawn_where_their_paths_are() {
    // Shapes: the routing's frame, the wires, the feedback loop, the output line, then a frame and a line
    // for each operator's envelope.
    // The panel is 300 high: windows that hold it (1280x720 at 2 shows 360).
    let m = Measure { step: 2., least: 6, sizes: &[((1280, 720), 1.), ((1920, 1080), 1.), ((1920, 1080), 2.)] };
    let mut report = String::new();
    for (name, shape) in [("feedback loop", 2), ("operator 1's envelope", 5), ("operator 2's envelope", 7)] {
        report += &check_line_with(name, &fm_plugin(), &fm_values(0.), &fm_text, 0, shape, &m);
    }
    println!("{report}");
}

// Everything the FM view draws lies inside its panel (the routing, the columns, and each operator's
// envelope: its frame, its line and its stroke), at 1280x720 and the FANTOM's 1.55, under every theme;
// the four operators each have an envelope, below the ratio and above the panel's edge.
#[test]
fn the_fm_view_lies_inside_its_panel() {
    fit_to_window();
    for (size, scale) in [((1280, 720), 1.), ((1280, 720), 1.55)] {
        let mut s = Screen::sized(size.0, size.1);
        s.set_scale(scale);
        s.frame();
        let mut p = open_with(&mut s, fm_plugin(), &fm_values(0.), &fm_text);
        for t in &all_themes() {
            theme::apply(&s.ui, t);
            s.frame();
            p.shown();
            s.frame();
            let panel = &panels(&s.ui)[0];
            let (w, h) = (panel.w, panel.h);
            let at = format!("in {} at {scale}", t.id);
            let inside = |what: &str, (l, t, r, b): (f32, f32, f32, f32)| {
                assert!(l >= 0. && t >= 0. && r <= w && b <= h, "{what} ({l:.1}, {t:.1}, {r:.1}, {b:.1}) lies outside the {w}x{h} panel {at}");
            };
            let mut frames = Vec::new();
            for (k, shape) in panel.shapes.iter().enumerate() {
                let (xs, ys): (Vec<f64>, Vec<f64>) = super::envelope::vertices(&shape.commands).into_iter().unzip();
                let reach = f64::from(shape.width) / 2. + 0.5; // a line's stroke
                let bounds = (
                    (xs.iter().copied().fold(f64::MAX, f64::min) - reach) as f32,
                    (ys.iter().copied().fold(f64::MAX, f64::min) - reach) as f32,
                    (xs.iter().copied().fold(f64::MIN, f64::max) + reach) as f32,
                    (ys.iter().copied().fold(f64::MIN, f64::max) + reach) as f32,
                );
                inside(&format!("shape {k}"), bounds);
                if k >= 4 && k % 2 == 0 {
                    frames.push(bounds); // an envelope's frame; its line follows
                }
            }
            for b in panel.boxes.iter() {
                inside(&format!("box {:?}", b.text), (b.x, b.y, b.x + b.w, b.y + b.h));
            }
            for l in panel.labels.iter() {
                inside(&format!("label {:?}", l.text), (l.x, l.y, l.x + l.w, l.y + l.h));
            }
            assert_eq!(frames.len(), 4, "an envelope for each operator {at}");
            let ratio = panel.boxes.iter().filter(|b| b.text.starts_with('x')).map(|b| b.y + b.h).fold(0., f32::max);
            for (n, f) in frames.iter().enumerate() {
                assert!(f.1 > ratio && f.3 < h && f.3 - f.1 > 40., "operator {}'s envelope ({f:?}) is below the ratios ({ratio}) and whole {at}", n + 1);
            }
        }
    }
}

// An FM-X-like synth: eight operators, each with a level, a ratio, a fine ratio and an envelope (the last
// three also a fixed-frequency switch), and an algorithm of two routings with three carriers (1, 5 and 8): a
// stack four deep (4 into 3 into 2 into 1), operator 5 modulated by 6 and 7, operator 8 alone. The first
// routing feeds back from operator 7 into operator 6 (a different one), the second from operator 4 into itself.
// Every parameter has a module path: a plugin of more than 24 without one is split by its names' first words.
fn fm8_plugin() -> serde_json::Value {
    let routing = |inputs: [u8; 8], from: u8, to: u8| json!({"inputs": inputs, "carriers": 0b1001_0001, "feedbackFrom": from, "feedbackTo": to});
    let inputs = [0b10, 0b100, 0b1000, 0, 0b110_0000, 0, 0, 0];
    let mut algorithm = viewed(1, "Algorithm", 1., true, 1, "algorithm", 0);
    algorithm["algorithms"] = json!([routing(inputs, 7, 6), routing(inputs, 4, 4)]);
    let mut p = vec![algorithm, viewed(2, "Feedback", 7., true, 1, "feedback", 0)];
    for n in 1..=8u32 {
        p.push(viewed(100 + n, &format!("Level {n}"), 99., false, 1, "operatorLevel", n));
        p.push(viewed(200 + n, &format!("Ratio {n}"), 31., true, 1, "ratio", n));
        p.push(viewed(300 + n, &format!("Fine {n}"), 99., false, 1, "fine", n));
        if n >= 6 {
            p.push(viewed(400 + n, &format!("Fixed {n}"), 1., true, 1, "fixed", n));
        }
        for (k, f) in ["attack", "decay", "release"].iter().enumerate() {
            p.push(viewed(500 + 3 * (n - 1) + k as u32, &format!("{f} {n}"), 1., false, 1, f, n));
        }
    }
    for q in &mut p {
        q["module"] = json!("FM");
    }
    json!({ "parameters": p })
}

fn fm8_text(id: u32, v: f64) -> String {
    match id {
        1 => format!("{}", v as i64 + 1),
        2 => format!("{}", v as i64),
        101..=108 => format!("{v:.0}"),
        201..=208 => format!("x{}", v as i64 / 2 + 1),
        301..=308 => format!("{:+.0}", v - 50.),
        406..=408 => if v > 0.5 { "Fixed" } else { "Ratio" }.into(),
        _ => format!("{:.0} %", v * 100.),
    }
}

fn fm8_values(algorithm: f64) -> Vec<(u32, f64)> {
    let mut v = vec![(1, algorithm), (2, 5.), (406, 0.), (407, 1.), (408, 1.)];
    for n in 1..=8u32 {
        v.push((100 + n, 99. - 11. * f64::from(n - 1)));
        v.push((200 + n, f64::from(2 * n)));
        v.push((300 + n, 50. + f64::from(n) * 3.));
        for k in 0..3 {
            v.push((500 + 3 * (n - 1) + k, [0.05 + 0.05 * f64::from(n), 0.5 - 0.04 * f64::from(n), 0.3][k as usize]));
        }
    }
    v
}

// What the reference does at eight operators (ParameterGrid::Operators::layout): the same as at four: the routing
// keeps its share (36% of the width, at most 360) and the columns divide what is left equally, narrower, none
// wrapped or scrolled. So at eight everything must still lie inside its column and the panel, the operators of the
// diagram (eight, on rows by their routing) must not overlap, and the columns must not overlap each other.
#[test]
fn eight_operators_lie_inside_their_columns_and_the_panel() {
    fit_to_window();
    for (size, scale) in [((1280, 720), 1.), ((1280, 720), 1.55)] {
        for algorithm in [0., 1.] {
            let mut s = Screen::sized(size.0, size.1);
            s.set_scale(scale);
            s.frame();
            let mut p = open_with(&mut s, fm8_plugin(), &fm8_values(algorithm), &fm8_text);
            for t in &all_themes() {
                theme::apply(&s.ui, t);
                s.frame();
                p.shown();
                s.frame();
                let panel = &panels(&s.ui)[0];
                let (w, h) = (panel.w, panel.h);
                let at = format!("in {} at {scale}, algorithm {algorithm}", t.id);
                let boxes: Vec<crate::GridBox> = panel.boxes.iter().collect();
                for b in &boxes {
                    assert!(b.x >= 0. && b.y >= 0. && b.x + b.w <= w && b.y + b.h <= h, "box {:?} outside the panel {at}", b.text);
                }
                let names: Vec<crate::GridLabel> = panel.labels.iter().filter(|l| l.text.starts_with("Op ")).collect();
                assert_eq!(names.len(), 8, "a column for each operator {at}");
                for l in panel.labels.iter() {
                    assert!(l.x >= 0. && l.y >= 0. && l.x + l.w <= w && l.y + l.h <= h, "label {:?} outside the panel {at}", l.text);
                }
                // The routing's eight boxes: numbered, none over another.
                let diagram: Vec<&crate::GridBox> = boxes.iter().filter(|b| b.text.len() == 1 && b.text.chars().all(|c| c.is_ascii_digit())).collect();
                assert_eq!(diagram.len(), 8, "eight operators in the diagram {at}");
                let apart = |a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| a.2 <= b.0 || b.2 <= a.0 || a.3 <= b.1 || b.3 <= a.1;
                for (i, a) in diagram.iter().enumerate() {
                    for b in &diagram[i + 1..] {
                        assert!(apart((a.x, a.y, a.x + a.w, a.y + a.h), (b.x, b.y, b.x + b.w, b.y + b.h)), "operators {} and {} overlap {at}", a.text, b.text);
                    }
                }
                // The columns: each name, ratio and its envelope's frame inside its own column, which are in order and apart.
                let ratios: Vec<&crate::GridBox> = boxes.iter().filter(|b| b.text.starts_with('x') || b.text.starts_with("Fixed")).collect();
                assert_eq!(ratios.len(), 8, "a ratio for each operator {at}");
                let frames: Vec<(f64, f64)> = panel
                    .shapes
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| *k >= 4 && k % 2 == 0)
                    .map(|(_, s)| {
                        let (xs, _): (Vec<f64>, Vec<f64>) = super::envelope::vertices(&s.commands).into_iter().unzip();
                        (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max))
                    })
                    .collect();
                assert_eq!(frames.len(), 8, "an envelope for each operator {at}");
                for n in 0..8 {
                    let r = ratios[n];
                    assert!(f64::from(r.x) - 0.6 <= frames[n].0 && frames[n].1 <= f64::from(r.x + r.w) + 0.6, "operator {}'s envelope lies in its column {at}", n + 1);
                    assert!(names[n].x + names[n].w <= r.x + r.w + 0.6 && names[n].x >= r.x - 0.6, "operator {}'s name lies in its column {at}", n + 1);
                    if n > 0 {
                        assert!(ratios[n - 1].x + ratios[n - 1].w <= r.x, "columns {} and {} are apart {at}", n, n + 1);
                    }
                }
                // Nothing the reference would show is shortened: ratio texts fit their boxes.
                assert!(ratios.iter().all(|r| !r.text.is_empty()), "every ratio says something {at}");
            }
        }
    }
}

// The eight-operator view's lines (the feedback loop from one operator to another, and each envelope) are drawn
// where their paths are, at two sizes.
#[test]
fn the_eight_operator_lines_are_drawn_where_their_paths_are() {
    let m = Measure { step: 2., least: 4, sizes: &[((1280, 720), 1.), ((1920, 1080), 1.), ((1920, 1080), 2.)] };
    let mut report = String::new();
    for (name, shape) in [("feedback loop 7 into 6", 2), ("operator 1's envelope", 5), ("operator 8's envelope", 19)] {
        report += &check_line_with(name, &fm8_plugin(), &fm8_values(0.), &fm8_text, 0, shape, &m);
    }
    println!("{report}");
}

#[test]
fn eight_operator_pictures() {
    pictures("grid-fm8", fm8_plugin(), &fm8_values(0.), &fm8_text);
    pictures("grid-fm8-self", fm8_plugin(), &fm8_values(1.), &fm8_text);
}

/// A page's picture under the default theme at 1 and at the FANTOM's 1.55 (target/screens) and under the odd
/// theme (target/theme-shots), as NAME, NAME@1.55 and NAME-odd.
fn pictures(name: &str, list: serde_json::Value, values: &[(u32, f64)], text: &dyn Fn(u32, f64) -> String) {
    fit_to_window();
    let mut s = Screen::new();
    let mut p = open_with(&mut s, list, values, text);
    s.frame();
    png(&s, name);
    at_fantom_scale(&mut s, name, || p.shown());
    theme::apply(&s.ui, &odd_theme());
    s.frame();
    p.shown();
    assert!(s.frame().is_some());
    shot(&s, &format!("{name}-odd"));
}

// Every view's page in every theme (the built-in ones and the two odd ones): drawn, laid out again for the
// theme's spacing, pictures in target/theme-shots.
#[test]
fn the_views_draw_in_every_theme() {
    fit_to_window();
    type Page = (&'static str, serde_json::Value, Vec<(u32, f64)>, fn(u32, f64) -> String);
    let pages: [Page; 5] = [
        ("lfo", lfo_plugin(), lfo_values(0.), lfo_text),
        ("filter", filter_plugin(), filter_values(1.), filter_text),
        ("keymap", map_plugin(), map_values(), map_text),
        ("fm", fm_plugin(), fm_values(0.), fm_text),
        ("fm8", fm8_plugin(), fm8_values(0.), fm8_text),
    ];
    for (name, list, values, text) in pages {
        let mut s = Screen::new();
        let mut p = open_with(&mut s, list, &values, &text);
        for t in &all_themes() {
            theme::apply(&s.ui, t);
            s.frame();
            p.shown();
            assert!(s.frame().is_some(), "{name} in {}: drawn", t.id);
            assert!(!panels(&s.ui).is_empty());
            shot(&s, &format!("grid-{name}-{}", t.id));
        }
    }
}

// Pictures for a look by eye: each view's page.
#[test]
fn view_pictures() {
    pictures("grid-lfo", lfo_plugin(), &lfo_values(0.), &lfo_text);
    pictures("grid-filter", filter_plugin(), &filter_values(1.), &filter_text);
    pictures("grid-keymap", map_plugin(), &map_values(), &map_text);
    pictures("grid-fm", fm_plugin(), &fm_values(0.), &fm_text);
}

// The wave's line is drawn from its own points, unscaled: the pixels of the line lie on the path at two
// window sizes and two scales, for a curve (the sine), a polyline (the triangle) and a stepped wave.
#[test]
fn the_lfo_line_is_drawn_where_its_path_is() {
    let mut report = String::new();
    for (name, shape) in [("sine", 0.), ("triangle", 1.), ("sample and hold", 4.)] {
        report += &check_line(name, &lfo_plugin(), &lfo_values(shape), &lfo_text, 0, 1);
    }
    println!("{report}");
}

// The filter's response line, the same way: from the same points as its fill, drawn where the path is, at a
// low-pass, a high-pass and a band-pass (the notch dips past the plot's floor, which is flat there).
#[test]
fn the_filter_line_is_drawn_where_its_path_is() {
    let mut report = String::new();
    for (name, kind) in [("low-pass", 0.), ("high-pass", 2.), ("band-pass", 3.)] {
        report += &check_line(name, &filter_plugin(), &filter_values(kind), &filter_text, 0, 2);
    }
    println!("{report}");
}
