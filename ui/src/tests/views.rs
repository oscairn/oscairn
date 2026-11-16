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
    p.view_up(0);
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
    p.view_up(0);
    let out = sent(&mut p.out);
    assert!(out.contains(&set(10, 0.)) && out.contains(&set(11, 0.)), "{out:?}");
    // A node with only a time (the attack: its level is fixed at full) sets only the time.
    let (x, y) = node(0);
    assert!(p.view_down(0, x, y));
    p.view_move(0, -1000., 0.);
    p.view_up(0);
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

// Pictures for a look by eye: each view's page.
#[test]
fn view_pictures() {
    pictures("grid-lfo", lfo_plugin(), &lfo_values(0.), &lfo_text);
    pictures("grid-filter", filter_plugin(), &filter_values(1.), &filter_text);
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
