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
    let mut p = super::grid::open(&mut s, super::grid::every_kind(), &[(9, 0.1), (10, 0.4), (11, 0.6), (12, 0.5)]);
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
    p.act(crate::nav::Action::Move { pane: None, by: 8 });
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

// Pictures for a look by eye (target/screens, target/theme-shots): the LFO page under the default theme at
// 1 and at the FANTOM's 1.55, and under the odd theme.
#[test]
fn view_pictures() {
    fit_to_window();
    let mut s = Screen::new();
    let mut p = open_with(&mut s, lfo_plugin(), &lfo_values(0.), &lfo_text);
    s.frame();
    png(&s, "grid-lfo");
    let ui = s.ui.clone_strong();
    at_fantom_scale(&mut s, "grid-lfo", || p.shown());
    let _ = ui;
    theme::apply(&s.ui, &odd_theme());
    s.frame();
    p.shown();
    assert!(s.frame().is_some());
    shot(&s, "grid-lfo-odd");
}

// The wave's line is drawn from its own points, unscaled: the pixels of the line lie on the path at two
// window sizes and two scales, for a curve (the sine), a polyline (the triangle) and a stepped wave.
#[test]
fn the_lfo_line_is_drawn_where_its_path_is() {
    let mut report = String::new();
    let mut worst = 0f64;
    for shape in [0., 1., 4.] {
        for (size, scale) in [((1280, 720), 1.), ((1280, 720), 2.), ((1920, 1080), 1.), ((1920, 1080), 2.)] {
            let (shot, _p) = view_shot(&lfo_plugin(), &lfo_values(shape), &lfo_text, 0, size, scale);
            let path = shot.panel.shapes.iter().nth(1).expect("the wave").commands.to_string();
            let (count, off) = line_offsets(&shot, &path);
            report += &format!("{size:?}@{scale} shape {shape}: {count} places, worst {off:.2} px\n");
            assert!(count >= 20, "the line was measured at enough places:\n{report}");
            worst = worst.max(off);
            // The measure sees an offset: the same path a pixel lower is found that far off.
            let lower: Vec<(f32, f32)> = super::envelope::vertices(&path).into_iter().map(|(x, y)| (x as f32, y as f32 + 1.)).collect();
            let (_, wrong) = line_offsets(&shot, &crate::views::poly(&lower));
            assert!(wrong > 0.7 * f64::from(scale), "a path a pixel off is seen as {wrong:.2} px off");
        }
    }
    println!("{report}");
    assert!(worst <= 0.6, "the line is off its path by up to {worst:.2} px:\n{report}");
}
