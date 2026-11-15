// SPDX-License-Identifier: AGPL-3.0-or-later
//! The parameter pages on the grid (params.rs, grid.rs, slint/grid.slint) against the engine's
//! words: the layout data read from `params.list`, each control kind's requests by knobs, dial and
//! buttons and by touch, the cursor in two dimensions and across pages, the page published for the
//! screen knobs, and pictures of every control kind, a real instrument's page (the Basic Synth's
//! hints, plugins/basic-synth/basic_synth.cpp) at several window sizes and scales.

use super::*;
use crate::client::Error;
use crate::params::{Outcome, Request};

fn sent(out: &mut Vec<crate::params::Out>) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(out).into_iter().map(|(_, m, p)| (m, p)).collect()
}

fn tiles(ui: &Shell) -> Vec<crate::GridTile> {
    ui.global::<crate::GridState>().get_tiles().iter().collect()
}

fn names(ui: &Shell) -> Vec<String> {
    tiles(ui).iter().map(|t| t.name.to_string()).collect()
}

fn cursor(ui: &Shell) -> Option<String> {
    tiles(ui).iter().find(|t| t.cursor).map(|t| t.name.to_string())
}

fn param(id: u32, name: &str, module: &str, max: f64, stepped: bool, widget: &str, priority: u32) -> serde_json::Value {
    json!({"id": id, "name": name, "module": module, "minimum": 0.0, "maximum": max, "defaultValue": 0.0, "stepped": stepped, "flags": 0, "widget": widget, "priority": priority, "group": 0})
}

/// A page with every control kind the data selects, and an envelope.
fn every_kind() -> serde_json::Value {
    let mut p = vec![
        param(0, "Cutoff", "Voice", 1., false, "knob", 1),
        param(1, "Resonance", "Voice", 1., false, "knob", 2),
        param(2, "Pan", "Voice", 1., false, "slider", 3),
        param(3, "Mode", "Voice", 2., true, "buttons", 4),
        param(4, "Sync", "Voice", 1., true, "switch", 5),
        param(5, "Wave", "Voice", 5., true, "list", 6),
        param(6, "Keys", "Voice", 127., true, "range", 7),
        param(7, "Key high", "Voice", 127., true, "auto", 0),
        param(8, "Voices", "Voice", 1., false, "auto", 8),
        param(9, "Attack", "Voice/Envelope", 1., false, "knob", 0),
        param(10, "Decay", "Voice/Envelope", 1., false, "knob", 0),
        param(11, "Sustain", "Voice/Envelope", 1., false, "knob", 0),
        param(12, "Release", "Voice/Envelope", 1., false, "knob", 0),
        param(13, "Gain", "Output", 1., false, "slider", 0),
    ];
    p[0]["group"] = json!(1);
    p[1]["group"] = json!(1);
    p[2]["minimum"] = json!(-1.0); // bipolar: symmetric, default 0
    p[6]["rangeHigh"] = json!(7);
    p[8]["flags"] = json!(8); // read only
    for (k, f) in ["attack", "decay", "sustain", "release"].iter().enumerate() {
        p[9 + k]["view"] = json!(1);
        p[9 + k]["field"] = json!(f);
        p[9 + k]["index"] = json!(1);
        p[9 + k]["group"] = json!(2);
    }
    json!({"parameters": p})
}

/// The Basic Synth's parameters and hints as plugins/basic-synth/basic_synth.cpp gives them.
fn basic_synth() -> serde_json::Value {
    let mut p = vec![
        param(100, "Brightness", "", 1., false, "knob", 1),
        param(101, "Level", "", 1., false, "knob", 2),
        param(102, "Pressure", "", 1., false, "knob", 5),
        param(103, "Oscillator", "", 2., true, "buttons", 3),
        param(104, "Detune", "", 50., false, "knob", 4),
        param(105, "Pitch LFO rate", "", 40., false, "knob", 6),
        param(106, "Pitch LFO depth", "", 200., false, "knob", 7),
        param(107, "Output bus", "", 8., true, "list", 0),
        param(108, "Pitch bend range", "", 24., false, "auto", 0),
        param(109, "Mod wheel", "", 1., true, "buttons", 0),
        param(110, "Attack", "Envelope", 2., false, "knob", 0),
        param(111, "Decay", "Envelope", 2., false, "knob", 0),
        param(112, "Sustain", "Envelope", 1., false, "knob", 0),
        param(113, "Release", "Envelope", 2., false, "knob", 0),
    ];
    p[5]["group"] = json!(1);
    p[6]["group"] = json!(1);
    for (k, f) in ["attack", "decay", "sustain", "release"].iter().enumerate() {
        p[10 + k]["view"] = json!(1);
        p[10 + k]["field"] = json!(f);
        p[10 + k]["index"] = json!(0);
        p[10 + k]["group"] = json!(2);
    }
    json!({"parameters": p})
}

/// The plugin's text for a value, as a fake engine answers params.text.
fn text_of(param: u64, v: f64) -> String {
    match param {
        3 => ["Saw", "Square", "Sine"].get(v as usize).copied().unwrap_or("?").into(),
        4 | 9 => if v > 0. { "On" } else { "Off" }.into(),
        5 => ["Sine", "Triangle", "Saw", "Square", "Noise", "S&H"].get(v as usize).copied().unwrap_or("?").into(),
        6 | 7 => format!("{}{}", ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][v as usize % 12], v as i64 / 12 - 1),
        103 => ["Saw", "Square", "Triangle"].get(v as usize).copied().unwrap_or("?").into(),
        107 => if v == 0. { "Main".into() } else { format!("Bus {v}") },
        109 => if v > 0. { "On" } else { "Off" }.into(),
        104 => format!("{v:.1} cents"),
        _ => format!("{:.0} %", v * 100.),
    }
}

/// Answers every request but the values: the texts as the plugin writes them.
fn answer(p: &mut crate::params::Screen) -> Vec<(&'static str, serde_json::Value)> {
    let mut rest = Vec::new();
    for _ in 0..4 {
        for (r, m, a) in std::mem::take(&mut p.out) {
            if let Request::Text(id, bits) = r {
                p.reply(Request::Text(id, bits), Ok(json!({"text": text_of(id as u64, f64::from_bits(bits))})));
            } else {
                rest.push((m, a));
            }
        }
    }
    rest
}

fn open(s: &mut Screen, list: serde_json::Value, values: &[(u32, f64)]) -> crate::params::Screen {
    s.ui.set_screen(7);
    let mut p = crate::params::Screen::new(&s.ui);
    p.open(2, "Synth");
    p.shown();
    assert_eq!(sent(&mut p.out), [("params.list", json!({"node": 2}))]);
    p.reply(Request::List, Ok(list));
    let asked = answer(&mut p);
    assert!(asked.iter().any(|a| a.0 == "params.get"));
    let values: Vec<serde_json::Value> = values.iter().map(|(id, v)| json!({"node": 2, "param": id, "value": v})).collect();
    p.reply(Request::Values, Ok(json!({ "values": values })));
    answer(&mut p);
    // The grid's size settles with the first frame (the pages column takes its room): laid out again.
    s.frame();
    p.shown();
    p
}

fn every_kind_values() -> Vec<(u32, f64)> {
    vec![(0, 0.7), (1, 0.3), (2, -0.4), (3, 1.), (4, 1.), (5, 2.), (6, 36.), (7, 84.), (8, 0.5), (9, 0.1), (10, 0.4), (11, 0.6), (12, 0.5), (13, 0.8)]
}

// The layout data decides the tiles: by priority, each its control, a range's high end without a tile
// of its own, the envelope's view; the shown tiles are published for the screen knobs.
#[test]
fn the_page_is_laid_out_from_the_plugins_data() {
    let s = Screen::new();
    let ui = &s.ui;
    s.ui.set_screen(7);
    let mut p = crate::params::Screen::new(ui);
    p.open(2, "Synth");
    p.shown();
    sent(&mut p.out);
    p.reply(Request::List, Ok(every_kind()));
    let out: Vec<(&'static str, serde_json::Value)> = p.out.iter().map(|o| (o.1, o.2.clone())).collect();
    answer(&mut p);
    let view = out.iter().find(|o| o.0 == "controllers.view").expect("the page published");
    let ids: Vec<u64> = view.1["screen"]["params"].as_array().unwrap().iter().map(|a| a["param"].as_u64().unwrap()).collect();
    assert_eq!(ids, [0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12], "the shown tiles by priority, numbered first");
    assert_eq!(view.1["screen"]["name"], "Voice");
    assert_eq!(view.1["screen"]["params"][0], json!({"kind": "parameter", "node": 2, "param": 0}));
    // Every value of a stepped parameter's text is asked for (the list's, the buttons' names).
    assert!((0..6).all(|v| out.iter().any(|o| o.0 == "params.text" && o.1["param"] == 5 && o.1["value"] == v as f64)));
    p.reply(Request::Values, Ok(json!({"values": every_kind_values().iter().map(|(id, v)| json!({"node": 2, "param": id, "value": v})).collect::<Vec<_>>()})));
    answer(&mut p);
    let t = tiles(ui);
    assert_eq!(names(ui), ["Cutoff", "Resonance", "Pan", "Mode", "Sync", "Wave", "Keys", "Voices", "Attack", "Decay", "Sustain", "Release"]);
    assert_eq!(t.iter().map(|t| t.kind).collect::<Vec<_>>(), [1, 1, 0, 2, 3, 4, 5, 6, 1, 1, 1, 1]);
    assert!(t[0].linked && t[1].linked && !t[2].linked, "a hint's group is one card");
    assert!(t[2].bipolar && (t[2].norm - 0.3).abs() < 1e-6);
    assert_eq!(t[3].choices.iter().map(|c| c.to_string()).collect::<Vec<_>>(), ["Saw", "Square", "Sine"]);
    assert_eq!(t[3].chosen, 1);
    assert_eq!((t[4].on, t[4].value.as_str()), (true, "On"));
    assert_eq!(t[5].value.as_str(), "Saw", "the list shows its value's text");
    assert_eq!(t[6].value.as_str(), "C2 – C6", "a range shows both ends");
    assert!((t[6].high - 84. / 127.).abs() < 1e-6);
    assert_eq!(t[7].value.as_str(), "50 %", "read only: the plugin's text alone");
    let g = ui.global::<crate::GridState>();
    assert_eq!(g.get_panels().row_count(), 1, "the envelope");
    assert_eq!(g.get_panels().row_data(0).unwrap().points.row_count(), 3);
    assert_eq!(g.get_headings().iter().map(|h| h.text.to_string()).collect::<Vec<_>>(), ["Envelope"]);
    assert_eq!(g.get_pages().iter().map(|p| p.to_string()).collect::<Vec<_>>(), ["Voice", "Output"]);
    // Hidden: the screen knobs let go of the page.
    p.hidden();
    assert_eq!(sent(&mut p.out).last().unwrap(), &("controllers.view", json!({"screen": {"name": "", "params": []}})));
}

// Without touch: the dial in reading order, the left knob across columns, the right knob down rows;
// a press selects a tile and the dial changes it (in a gesture), a dropdown opens its list, split
// buttons step their stops, a range takes its low end then its high end, a switch flips; Cancel backs
// out a layer at a time, then goes back; past the last tile the page turns.
#[test]
fn every_control_by_knobs_dial_and_buttons() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let mut p = open(&mut s, every_kind(), &every_kind_values());
    assert_eq!(cursor(ui), None, "no cursor until the first move");
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(cursor(ui).as_deref(), Some("Cutoff"));
    p.act(Action::Move { pane: None, by: 2 });
    assert_eq!(cursor(ui).as_deref(), Some("Pan"));
    p.act(Action::Move { pane: Some(0), by: -1 });
    assert_eq!(cursor(ui).as_deref(), Some("Resonance"), "the left knob: columns");
    let first_row = tiles(ui)[1].y;
    p.act(Action::Move { pane: Some(1), by: 1 });
    let below = tiles(ui).into_iter().find(|t| t.cursor).unwrap();
    assert!(below.y > first_row, "the right knob: rows");
    p.act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(cursor(ui).as_deref(), Some("Resonance"));
    // A knob: press, the dial steps a hundredth, press ends.
    p.act(Action::Move { pane: Some(0), by: -1 });
    p.act(Action::Confirm(None));
    assert!(tiles(ui)[0].editing);
    p.act(Action::Move { pane: None, by: 3 });
    let out = sent(&mut p.out);
    assert_eq!(out[0], ("params.gesture", json!({"node": 2, "param": 0, "begin": true})));
    assert_eq!(out[1].0, "params.set");
    assert!((out[1].1["values"][0]["value"].as_f64().unwrap() - 0.73).abs() < 1e-9);
    // The engine's own change of it waits; another's shows.
    p.changed(&json!({"changes": [{"node": 2, "param": 0, "kind": "value", "value": 0.1}, {"node": 2, "param": 1, "kind": "value", "value": 0.9}]}));
    assert!((tiles(ui)[0].norm - 0.73).abs() < 1e-6 && (tiles(ui)[1].norm - 0.9).abs() < 1e-6);
    p.act(Action::Confirm(None));
    assert_eq!(sent(&mut p.out).iter().filter(|o| o.0 == "params.gesture").map(|o| o.1["begin"].clone()).collect::<Vec<_>>(), [json!(false)]);
    assert!(!tiles(ui)[0].editing);
    // Split buttons: the dial steps the stops, no further than the last.
    p.act(Action::Move { pane: None, by: 3 });
    assert_eq!(cursor(ui).as_deref(), Some("Mode"));
    p.act(Action::Confirm(None));
    p.act(Action::Move { pane: None, by: 5 });
    let sets: Vec<_> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.set").collect();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].1["values"][0]["value"], json!(2.0));
    assert_eq!(tiles(ui)[3].chosen, 2);
    p.act(Action::Cancel(None)); // ends the change
    assert!(!tiles(ui)[3].editing && cursor(ui).is_some(), "Cancel backs out one layer: the cursor stays");
    sent(&mut p.out);
    // A switch flips on a press, in a gesture of its own.
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(out.iter().map(|o| o.0).filter(|m| *m != "params.text").collect::<Vec<_>>(), ["params.gesture", "params.set", "params.gesture"]);
    assert!(!tiles(ui)[4].on);
    // A dropdown opens its list at its value; the dial moves, a press chooses and closes.
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Confirm(None));
    let g = ui.global::<crate::GridState>();
    assert!(g.get_list_open());
    assert_eq!(g.get_list().iter().map(|v| v.to_string()).collect::<Vec<_>>(), ["Sine", "Triangle", "Saw", "Square", "Noise", "S&H"]);
    assert_eq!(g.get_list_cursor(), 2);
    p.act(Action::Move { pane: None, by: 2 });
    p.act(Action::Confirm(None));
    assert!(!g.get_list_open());
    let set = sent(&mut p.out).into_iter().find(|o| o.0 == "params.set").unwrap();
    assert_eq!(set.1["values"][0]["value"], json!(4.0));
    // Cancel closes a list without a change.
    p.act(Action::Confirm(None));
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Cancel(None));
    assert!(!g.get_list_open());
    assert!(!sent(&mut p.out).iter().any(|o| o.0 == "params.set"));
    // A range: the low end first, a second press its high end; the ends do not cross.
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(cursor(ui).as_deref(), Some("Keys"));
    p.act(Action::Confirm(None));
    p.act(Action::Move { pane: None, by: 100 });
    let set = sent(&mut p.out).into_iter().find(|o| o.0 == "params.set").unwrap();
    assert_eq!(set.1["values"][0], json!({"node": 2, "param": 6, "value": 84.0}), "no higher than the high end");
    p.act(Action::Confirm(None));
    assert_eq!(tiles(ui)[6].part, 1);
    p.act(Action::Move { pane: None, by: 2 });
    let out = sent(&mut p.out);
    assert!(out.contains(&("params.gesture", json!({"node": 2, "param": 7, "begin": true}))));
    assert!(out.contains(&("params.set", json!({"values": [{"node": 2, "param": 7, "value": 86.0}]}))));
    p.act(Action::Confirm(None));
    let ended: Vec<_> = sent(&mut p.out).into_iter().filter(|o| o.0 == "params.gesture").map(|o| o.1["param"].clone()).collect();
    assert_eq!(ended, [json!(6), json!(7)], "both ends' gestures end");
    // Read only: a press does nothing.
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Confirm(None));
    assert!(sent(&mut p.out).is_empty() && !tiles(ui)[7].editing);
    // Past the last tile: the next page, its values asked.
    p.act(Action::End(1));
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(names(ui), ["Gain"]);
    assert_eq!(cursor(ui).as_deref(), Some("Gain"));
    assert!(sent(&mut p.out).iter().any(|o| o.0 == "params.get"));
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!(cursor(ui).as_deref(), Some("Release"), "back: the page before, at its last tile");
    p.act(Action::Page(1));
    assert_eq!(names(ui), ["Gain"]);
    // A refusal is said.
    assert!(p.reply(Request::Set, Err(Error { code: -32602, message: "no such parameter".into() })));
    assert_eq!(ui.global::<crate::GridState>().get_message().as_str(), "The engine refused: no such parameter");
    // Cancel: the cursor goes, then back.
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Back);
}

// Touch: a drag on a knob or slider sets it between a gesture's begin and end; a tap on a stop sets it;
// a tap on a dropdown opens it and a tap on a value chooses; a tap on a page shows it.
#[test]
fn every_control_by_touch() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let mut p = open(&mut s, every_kind(), &every_kind_values());
    p.grab(2, 0);
    p.drag(2, 0, 0.75);
    p.release(2);
    let out = sent(&mut p.out);
    assert_eq!(out[0], ("params.gesture", json!({"node": 2, "param": 2, "begin": true})));
    assert_eq!(out[1], ("params.set", json!({"values": [{"node": 2, "param": 2, "value": 0.5}]})));
    assert_eq!(out.last().unwrap(), &("params.gesture", json!({"node": 2, "param": 2, "begin": false})));
    // A range's high end, dragged.
    p.grab(6, 1);
    p.drag(6, 1, 1.0);
    p.release(6);
    assert!(sent(&mut p.out).contains(&("params.set", json!({"values": [{"node": 2, "param": 7, "value": 127.0}]}))));
    p.tap(3, 0);
    let out = sent(&mut p.out);
    assert!(out.contains(&("params.set", json!({"values": [{"node": 2, "param": 3, "value": 0.0}]}))));
    p.tap(5, -1);
    assert!(ui.global::<crate::GridState>().get_list_open());
    p.choose(5);
    assert!(!ui.global::<crate::GridState>().get_list_open());
    assert!(sent(&mut p.out).contains(&("params.set", json!({"values": [{"node": 2, "param": 5, "value": 5.0}]}))));
    p.show_page(1);
    assert_eq!(names(ui), ["Gain"]);
}

// The columns follow the width the grid is given: two to six.
#[test]
fn the_grid_reflows_for_the_width() {
    for (w, columns) in [(700usize, 2usize), (1100, 4), (1600, 6)] {
        let mut s = Screen::sized(w, 700);
        let p = open(&mut s, every_kind(), &[]);
        drop(p);
        let t = tiles(&s.ui);
        let first_row = t.iter().filter(|x| x.y == t[0].y).count();
        assert_eq!(first_row, columns, "{w} wide");
        assert!(t.iter().all(|x| x.w >= 2. * 44.), "a tile keeps room for a touch control");
    }
}

fn add_upright_widgets(ui: &Shell) {
    // shortcut: no params.list field selects these two yet (RFC 0042); drawn here so every widget is seen.
    let g = ui.global::<crate::GridState>();
    let mut t: Vec<crate::GridTile> = g.get_tiles().iter().collect();
    let last = t.iter().map(|x| x.y + x.h).fold(0., f32::max);
    let mut a = t[2].clone();
    (a.x, a.y, a.w, a.h, a.kind, a.name, a.value, a.norm, a.bipolar, a.linked, a.cursor) = (0., last + 6., 120., 200., 7, "Fader".into(), "-6 dB".into(), 0.7, false, false, false);
    let mut b = a.clone();
    (b.x, b.kind, b.name, b.value, b.norm) = (126., 8, "16'".into(), "6".into(), 0.75);
    t.push(a);
    t.push(b);
    g.set_tiles(std::rc::Rc::new(slint::VecModel::from(t)).into());
}

// Pictures for a look by eye: every control kind and a real instrument's pages, at the keyboard's size
// (1280x720 at 1 and at 1.55), in desktop windows of three widths and at scale 2, and on a small
// device (800x480) and a large one (1920x1080 at scale 2).
#[test]
fn grid_pictures() {
    fit_to_window();
    let mut s = Screen::new();
    let mut p = open(&mut s, every_kind(), &every_kind_values());
    p.act(Action::Move { pane: None, by: 2 });
    add_upright_widgets(&s.ui);
    s.frame();
    png(&s, "grid-every-kind");
    let ui = s.ui.clone_strong();
    at_fantom_scale(&mut s, "grid-every-kind", || {
        p.shown();
        add_upright_widgets(&ui);
    });
    // The dropdown open.
    p.act(Action::Move { pane: None, by: 3 });
    p.act(Action::Confirm(None));
    s.frame();
    png(&s, "grid-dropdown");
    let mut s = Screen::new();
    let mut b = open(&mut s, basic_synth(), &[(100, 0.8), (101, 0.7), (102, 0.5), (103, 0.), (104, 7.), (105, 4.), (106, 20.), (107, 0.), (108, 2.), (109, 0.), (110, 0.05), (111, 0.5), (112, 0.35), (113, 0.6)]);
    s.frame();
    png(&s, "grid-basic-synth");
    at_fantom_scale(&mut s, "grid-basic-synth", || b.shown());
    b.show_page(1);
    s.frame();
    png(&s, "grid-basic-synth-envelope");
    at_fantom_scale(&mut s, "grid-basic-synth-envelope", || b.shown());
    for (w, h, scale) in [(700usize, 700usize, 1f32), (1100, 700, 1.), (1600, 900, 1.), (2200, 1400, 2.), (800, 480, 1.), (3840, 2160, 2.)] {
        let mut s = Screen::sized(w, h);
        s.set_scale(scale);
        let mut p = open(&mut s, every_kind(), &every_kind_values());
        p.shown();
        assert!(s.frame().is_some());
        png(&s, &format!("grid-desktop-{}x{}@{}", (w as f32 / scale) as usize, (h as f32 / scale) as usize, scale));
    }
}

// Every kind of tile, the envelope and the dropdown in every theme (the built-in ones and the two odd
// ones): laid out again for the theme's gap between sections (none where they join), pictures in
// target/theme-shots.
#[test]
fn the_grid_draws_in_every_theme() {
    fit_to_window();
    let mut s = Screen::new();
    let mut p = open(&mut s, every_kind(), &every_kind_values());
    p.act(Action::Move { pane: None, by: 2 });
    for t in &all_themes() {
        theme::apply(&s.ui, t);
        p.shown();
        let tiles = tiles(&s.ui);
        let gap = tiles[3].x - (tiles[2].x + tiles[2].w);
        let joined = t.data.style.sections == crate::SectionStyle::Joined;
        assert_eq!(gap, if joined { 0. } else { crate::grid::SECTION_GAP }, "{}: the gap between neighbors", t.id);
        add_upright_widgets(&s.ui);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        shot(&s, &format!("grid-{}", t.id));
    }
    p.act(Action::Move { pane: None, by: 5 });
    p.act(Action::Confirm(None));
    for t in &all_themes() {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        shot(&s, &format!("grid-dropdown-{}", t.id));
    }
}
