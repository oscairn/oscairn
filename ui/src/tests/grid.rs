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
pub(super) fn every_kind() -> serde_json::Value {
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
pub(super) fn basic_synth() -> serde_json::Value {
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

pub(super) fn open(s: &mut Screen, list: serde_json::Value, values: &[(u32, f64)]) -> crate::params::Screen {
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
    assert_eq!(g.get_panels().row_count(), 2, "the filter (Cutoff and Resonance by name), then the envelope");
    assert_eq!(g.get_panels().row_data(1).unwrap().points.row_count(), 3);
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
    // Past the last tile the cursor stops: a page opens only by a press on its tab or a page key, its values asked.
    p.act(Action::End(1));
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(cursor(ui).as_deref(), Some("Release"));
    assert_ne!(names(ui), ["Gain"]);
    p.act(Action::Page(1));
    assert_eq!(names(ui), ["Gain"]);
    assert!(sent(&mut p.out).iter().any(|o| o.0 == "params.get"));
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

// The dropdown over a plugin's page (the sound block's parameters) scrolls the same way the shared
// lists do (issue #269): when it holds more values than a view a scroll bar shows (the list is longer
// than its view), a drag or the bar turns the view to the value asked, and the cursor stays on a
// shown row when the dial moves it.
#[test]
fn the_dropdown_list_scrolls_when_longer_than_a_view() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let mut p = open(&mut s, every_kind(), &every_kind_values());
    // A small view, so the list holds more than it: the scroll bar shows.
    ui.global::<crate::GridState>().set_list_per_view(2);
    p.shown();
    p.tap(5, -1); // the Wave list opens, six values, more than two fit
    let g = ui.global::<crate::GridState>();
    assert!(g.get_list_open(), "the list is open");
    let total = g.get_list().row_count();
    assert!(total > (g.get_list_per_view() as usize), "a longer list shows its scroll bar: {total} values, {} in view", g.get_list_per_view());
    // The bar or a drag asked for a deep value: the view turns, page-aligned, the cursor on a shown row.
    p.list_scroll_to(total as i32);
    let per = (g.get_list_per_view().max(1)) as usize;
    assert!((g.get_list_first() as usize) >= total - per, "the view turned near the end: first {} of {total}", g.get_list_first());
    assert!((g.get_list_cursor() as usize) < (g.get_list_first() as usize) + per, "the cursor is on a shown row after the scroll");
    // A dial move past the last shown value keeps the cursor on a shown row.
    p.act(Action::Move { pane: None, by: 100 });
    assert!((g.get_list_cursor() as usize) >= (g.get_list_first() as usize) && (g.get_list_cursor() as usize) < (g.get_list_first() as usize) + per, "a move keeps the cursor on a shown row: cursor {}, first {}", g.get_list_cursor(), g.get_list_first());
    // Cancel still closes it.
    p.act(Action::Cancel(None));
    assert!(!g.get_list_open());
}

// The columns follow the width the grid is given: two to six.
#[test]
fn the_grid_reflows_for_the_width() {
    for (w, columns) in [(600usize, 2usize), (700, 3), (1100, 5), (1600, 6)] {
        let mut s = Screen::sized(w, 700);
        // The width the tiles are given, not the rails': two strips.
        s.ui.global::<crate::Theme>().set_tab_placement(crate::TabPlacement::Top);
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
        s.frame(); // the area's width changes with the paddings: the grid is told on this frame
        p.shown();
        let tiles = tiles(&s.ui);
        let gap = tiles[3].x - (tiles[2].x + tiles[2].w);
        let joined = t.data.style.sections == crate::SectionStyle::Joined;
        assert!((gap - if joined { 0. } else { crate::grid::SECTION_GAP * t.data.spacing }).abs() < 0.01, "{}: the gap between neighbors", t.id);
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

// Small screens (the audit's smoke): a grid of long, realistic names (each 40+ characters) and a long
// module path (its rest is a heading) lays out with every tile inside the view the screen gives it and
// inside the window, a tile keeping half its width for a touch control, and no two tiles overlapping,
// on the small device's screen (800x480) and at the keyboard's scale (1280x720 at 1.55). A name too
// long for its box is cut with an ellipsis, never a bare clip: the tile's name, its value and the
// headings draw with `overflow: elide`.
#[test]
fn long_names_leave_nothing_cut_or_overlapping_on_small_screens() {
    let names = [
        "Filter resonance in the warm vocal section",
        "Oscillator coarse detune stretched to full",
        "Envelope attack time with the smoothing ramp",
        "LFO modulation rate keyboard tracking curve",
        "The split crossover eighth octave band limit",
        "Output bus routing to the left main channel",
    ];
    assert!(names.iter().all(|n| n.len() >= 40), "the audit's names must be 40+ characters");
    let list = json!({"parameters": (0..6u32).map(|i| param(i, names[i as usize], "Voice/The Upper Performance Section Arrangement", 1.0, true, "slider", 0)).collect::<Vec<_>>()});
    let values: Vec<(u32, f64)> = (0u32..6).map(|id| (id, 0.4 + f64::from(id) * 0.05)).collect();
    // The tile's name, its value and a heading are each cut with an ellipsis, never a bare clip.
    let slint = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("slint/grid.slint")).unwrap();
    assert!(slint.matches("overflow: elide").count() >= 4, "grid.slint: the name, the value and the headings must elide their text");
    for (w, h, scale, what) in [(800usize, 480usize, 1.0f32, "800x480"), (1280, 720, 1.55, "1280x720@1.55")] {
        let mut s = Screen::sized(w, h);
        s.set_scale(scale);
        s.ui.global::<crate::Theme>().set_tab_placement(crate::TabPlacement::Top);
        let mut p = open(&mut s, list.clone(), &values);
        p.shown();
        assert!(s.frame().is_some(), "{what}: drawn");
        let t = tiles(&s.ui);
        assert_eq!(t.len(), 6, "{what}: every parameter is a tile");
        let g = s.ui.global::<crate::GridState>();
        let (vw, vh) = (g.get_view_width(), g.get_view_height());
        let (win_w, win_h) = (w as f32 / scale, h as f32 / scale);
        for (i, tile) in t.iter().enumerate() {
            assert!(tile.x >= -0.01 && tile.y >= -0.01, "{what} t{i} ({}): starts inside the view", tile.name);
            assert!(tile.x + tile.w <= vw + 0.51 && tile.y + tile.h <= vh + 0.51, "{what} t{i} ({}): fits the view it was given", tile.name);
            assert!(tile.x + tile.w <= win_w + 0.51 && tile.y + tile.h <= win_h + 0.51, "{what} t{i} ({}): nothing leaves the window", tile.name);
            assert!(tile.w >= 2. * 44., "{what} t{i} ({}): a tile keeps half its width for a touch control", tile.name);
            for (j, other) in t.iter().enumerate() {
                if i != j {
                    let overlaps = tile.x < other.x + other.w - 0.01 && other.x < tile.x + tile.w - 0.01
                        && tile.y < other.y + other.h - 0.01 && other.y < tile.y + tile.h - 0.01;
                    assert!(!overlaps, "{what}: t{i} ({}) overlaps t{j} ({})", tile.name, other.name);
                }
            }
        }
        shot(&s, &format!("grid-long-{}", what.replace('@', "at").replace('.', "p")));
    }
}

// The light theme (the audit's smoke): the parameter pages render in the light theme with the theme's
// own background where nothing is drawn, no dark-only ink (a literal or a token without a light value)
// covering the screen — the same bounds the stage screens' test holds — body and muted text at 4.5:1 on
// the surfaces they draw on, and the cursor's focus border drawn (the pixels in the focus color grow
// when the cursor is shown).
#[test]
fn the_parameter_pages_draw_light_and_the_cursor_shows() {
    fn contrast(x: slint::Color, y: slint::Color) -> f64 {
        fn lum(c: slint::Color) -> f64 {
            let lin = |v: u8| {
                let v = f64::from(v) / 255.;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * lin(c.red()) + 0.7152 * lin(c.green()) + 0.0722 * lin(c.blue())
        }
        let (a, b) = (lum(x), lum(y));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    let light = theme::chosen(&theme::Places::default(), "oscairn-light", "");
    let bg = light.data.ink("background");
    let surface = light.data.ink("surface");
    let text = light.data.ink("text");
    let muted = light.data.ink("muted");
    assert!(contrast(text, bg) >= 4.5 && contrast(muted, bg) >= 4.5, "body and muted text read on the background");
    assert!(contrast(text, surface) >= 4.5 && contrast(muted, surface) >= 4.5, "a tile's name and value read on its surface");
    let mut s = Screen::new();
    theme::apply(&s.ui, &light);
    let mut p = open(&mut s, every_kind(), &every_kind_values());
    p.shown();
    let focus = s.ui.global::<crate::Theme>().get_focus();
    let (fr, fg, fb) = (focus.red(), focus.green(), focus.blue());
    let focus_pixels = |s: &Screen| s.pixels.iter().filter(|p| p.r.abs_diff(fr) <= 6 && p.g.abs_diff(fg) <= 6 && p.b.abs_diff(fb) <= 6).count();
    let base = focus_pixels(&s);
    p.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor on the first tile
    assert_eq!(cursor(&s.ui).as_deref(), Some("Cutoff"), "a tile is under the cursor");
    s.frame();
    assert_eq!(s.ui.global::<crate::Theme>().get_id(), "oscairn-light", "the theme applied");
    assert!(!s.ui.global::<crate::Theme>().get_dark(), "a light background");
    let near = |want: [u8; 3], tol: u8| {
        let mut count = 0u64;
        for p in &s.pixels {
            if p.r.abs_diff(want[0]) <= tol && p.g.abs_diff(want[1]) <= tol && p.b.abs_diff(want[2]) <= tol { count += 1; }
        }
        count as f64 / s.pixels.len() as f64
    };
    let corner: [u8; 3] = [s.pixels[0].r, s.pixels[0].g, s.pixels[0].b];
    assert_eq!(corner, [bg.red(), bg.green(), bg.blue()], "the background where nothing is drawn");
    let dark_ink = near([0xF6, 0xF8, 0xFA], 2);
    assert!(dark_ink < 0.000_28, "{:.2}% of the screen in a dark theme's text color", dark_ink * 100.0);
    // The grid is dense, so a tile's text alone is a few percent of the window (measured 2.4%); the
    // dark theme's surfaces (#111317, #1D2127) would cover most of a tile. A share under an eighth.
    let dark = s.pixels.iter().filter(|p| 0.2126 * f64::from(p.r) + 0.7152 * f64::from(p.g) + 0.0722 * f64::from(p.b) < 80.0).count();
    assert!(dark * 8 < s.pixels.len(), "{:.2}% of the screen is dark, not a light scene", 100.0 * dark as f64 / s.pixels.len() as f64);
    let shown = focus_pixels(&s);
    assert!(shown > base + 100, "the cursor's focus border is drawn on the light background ({} to {} focus pixels)", base, shown);
    shot(&s, "grid-light");
}

// Spacing alone grows the gap between tiles, and the grid is laid out again for it: the same theme at
// 0.6, 1 and 1.6 differs in nothing else.
#[test]
fn a_larger_spacing_grows_the_gaps_between_tiles() {
    fit_to_window();
    let mut s = Screen::new();
    let mut p = open(&mut s, every_kind(), &every_kind_values());
    let base = all_themes().into_iter().find(|t| t.data.style.sections == crate::SectionStyle::Separate).unwrap();
    let mut seen = vec![];
    for spacing in [1., 0.6, 1.6, 1.] {
        let mut t = base.clone();
        t.data.spacing = spacing;
        theme::apply(&s.ui, &t);
        s.frame();
        p.shown();
        let tiles = tiles(&s.ui);
        seen.push((tiles[3].x - (tiles[2].x + tiles[2].w), tiles.iter().map(|t| t.y).fold(0., f32::max)));
    }
    for (got, spacing) in [(seen[0].0, 1.), (seen[1].0, 0.6), (seen[2].0, 1.6)] {
        assert!((got - crate::grid::SECTION_GAP * spacing).abs() < 0.01, "at {spacing} the gap is {got}");
    }
    assert!(seen[1].1 < seen[0].1 && seen[0].1 < seen[2].1, "rows are farther apart as the spacing grows: {seen:?}");
    assert_eq!(seen[3], seen[0], "and back");
}

// Tile density at the devices' screens against the reference's own rules (ParameterGrid::build in touch
// mode: columns width / 210 from 2 to 6, a tile 84 high or 120 with a knob, a gap of 6), and against the
// reference's room: in the JUCE app the grid is the Part page's panel, 16 in from each side, under the
// header (56), the chain strip (96 at least), a plugin's action row (44) and gaps (12 + 12 + 8) and above
// a 28 footer, under the 48 high top bar; its page list is 200 wide and 8 apart, its scroll bar 8. The Rust
// grid is a screen of its own, so it has more room (PartsView.cpp:750-788 for the sums).
#[test]
fn tile_density_follows_the_references_rules_and_has_more_room() {
    fit_to_window();
    // The devices' screens (hosts/standalone/profiles) at the scale Screen::scale gives them.
    for (name, w, h, scale) in [("FANTOM-07", 1280usize, 720usize, 1.55f32), ("MODX-M", 800, 480, 1.), ("Kick", 1024, 600, 1.)] {
        for (what, widget, pages, knobs) in [("knobs, two pages", "knob", true, true), ("knobs, one page", "knob", false, true), ("sliders, one page", "slider", false, false)] {
            let mut list = Vec::new();
            for i in 0..16 {
                let mut p = param(i, &format!("P{i}"), if pages && i >= 8 { "B" } else { "A" }, 1., false, widget, 0);
                p["group"] = json!(0);
                list.push(p);
            }
            let mut s = Screen::sized(w, h);
            s.set_scale(scale);
            s.frame();
            let _p = open(&mut s, json!({ "parameters": list }), &[]);
            s.frame();
            let g = s.ui.global::<crate::GridState>();
            let tiles: Vec<crate::GridTile> = g.get_tiles().iter().collect();
            let (vw, vh) = (g.get_view_width(), g.get_view_height());
            let columns = tiles.iter().filter(|t| t.y == 0.).count();
            let in_view = tiles.iter().filter(|t| t.y + t.h <= vh).count();
            assert_eq!(columns, ((vw / 210.) as usize).clamp(2, 6), "{name} {what}: the reference's columns for the width");
            assert_eq!(tiles[0].h, if knobs { 120. } else { 84. }, "{name} {what}: the reference's tile height");
            assert!((tiles[1].x - (tiles[0].x + tiles[0].w) - 6.).abs() < 0.01, "{name} {what}: the gap");
            // The reference's room at this screen, in the JUCE app's logical pixels.
            let (jw, jh) = (w as f32 / scale - 32. - if pages { 208. } else { 0. } - 8., h as f32 / scale - 48. - 32. - (56. + 12. + 96. + 12.) - 28. - (44. + 8.));
            let reference_columns = ((jw / 210.) as usize).clamp(2, 6);
            let reference_rows = ((jh + 6.) / ((if knobs { 120. } else { 84. }) + 6.)).floor().max(0.) as usize;
            let reference_in_view = reference_columns * reference_rows;
            println!("{name} {what}: Rust {vw:.0}x{vh:.0} px, {columns} columns, {in_view} tiles in view; the reference {jw:.0}x{jh:.0}, {reference_columns} columns, {reference_in_view} tiles in view");
            assert!(columns >= reference_columns && in_view >= reference_in_view, "{name} {what}: not fewer than the reference");
            if name == "FANTOM-07" {
                let numbers = (vw.round() as i32, vh.round() as i32, columns, in_view, reference_columns, reference_in_view);
                let expected = match what {
                    // Two pages: `auto` puts the pages in a rail at the left (one rail, 71 wide), which leaves more area.
                    "knobs, two pages" => (743, 405, 3, 8, 2, 2),
                    "knobs, one page" => (814, 405, 3, 9, 3, 3),
                    _ => (814, 405, 3, 12, 3, 3),
                };
                assert_eq!(numbers, expected, "FANTOM-07 {what}");
            }
        }
    }
}

// The parameter page is reachable without touch: from Home every control of the screen — the pages'
// rail, the tabs' rail under it, every tile (knob, switch, dropdown, value), the search in the top
// bar — is reached and activated with panel moves only (the dial `move`, `move:0` and `move:1` for
// the rails' and tiles' columns and rows, confirm, cancel/EXIT), no touch. The walk records what it
// visited and holds it against what the screen declares: the pages' and tabs' names and the tiles it
// draws per entry, and the banks published for the device's screen knobs (`controllers.view`
// `screen`), which follow the page and tiles shown.

/// The walk's panel actions as the device's panel reaches the shell: the `controllers` totals, then
/// answered and drawn.
fn walk_act(f: &mut Fixture, total: &mut std::collections::BTreeMap<String, i64>, key: &str, by: i64, list: &serde_json::Value, published: &mut Vec<(String, Vec<u64>)>) {
    *total.entry(key.into()).or_default() += by;
    f.navigation(json!(total.clone()));
    params_settle(f, list, published);
    f.screen.frame();
}

/// A quiet engine: every request out is answered until none comes for a moment. `params.list` is
/// answered with `list`, values and texts as the plugin writes them; the screens are left at rest.
/// What the page publishes for the device's screen knobs is recorded in `published`.
fn params_settle(f: &mut Fixture, list: &serde_json::Value, published: &mut Vec<(String, Vec<u64>)>) {
    let name_of = param_names(list);
    let value_of = |id: u64| match id {
        23 => 2.0,
        24 => 1.0,
        _ => 0.5,
    };
    while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(12)) {
        let method = r["method"].as_str().unwrap_or_default().to_string();
        let result = match method.as_str() {
            "params.list" => list.clone(),
            "params.get" => {
                let node = r["params"]["values"][0]["node"].clone();
                let values: Vec<serde_json::Value> = r["params"]["values"].as_array().unwrap().iter().map(|v| json!({"node": node, "param": v["param"], "value": value_of(v["param"].as_u64().unwrap_or(0))})).collect();
                json!({"values": values})
            }
            "params.text" => json!({"text": "v"}),
            "graph.get" => play_graph(),
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "edit.history" | "edit.undo" | "edit.redo" => json!({"undo": "", "redo": ""}),
            "record.get" => json!({"recording": 0, "retro": {"seconds": 30}, "takes": []}),
            "parts.meters" => json!({"meters": []}),
            "session.status" => json!({"midiInputs": [], "midiOutputs": []}),
            "window.info" => json!({"available": false}),
            "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
            "presets.list" => json!({"presets": []}),
            "controllers.view" if r["params"]["screen"].is_object() => {
                let screen = &r["params"]["screen"];
                let ids: Vec<u64> = screen["params"].as_array().unwrap().iter().map(|a| a["param"].as_u64().unwrap()).collect();
                published.push((screen["name"].as_str().unwrap_or_default().to_string(), ids));
                json!({})
            }
            _ => json!({}),
        };
        f.reply(&r, json!({"result": result}));
    }
    let _ = name_of;
}

/// The parameters' names by id, from the list's JSON.
fn param_names(list: &serde_json::Value) -> std::collections::HashMap<u64, String> {
    list["parameters"].as_array().unwrap().iter().map(|p| (p["id"].as_u64().unwrap(), p["name"].as_str().unwrap().to_string())).collect()
}

/// The parameters' priorities by id.
fn param_priorities(list: &serde_json::Value) -> std::collections::HashMap<u64, u64> {
    list["parameters"].as_array().unwrap().iter().map(|p| (p["id"].as_u64().unwrap(), p["priority"].as_u64().unwrap())).collect()
}

/// The walk over one entry's tiles (a page's, or a page and tab's): the dial from the first tile to
/// the last, each pressed (a knob's change taken and laid down, a switch flipped and back, a dropdown
/// opened, all its values reached and EXIT closed, a value seen), and what it visited held against
/// what the screen declares for this entry.
fn walk_tiles(f: &mut Fixture, total: &mut std::collections::BTreeMap<String, i64>, ui: &Shell, entry: (String, String), declared: &[String], list: &serde_json::Value, published: &mut Vec<(String, Vec<u64>)>) {
    let _ = param_names(list);
    let _tiles = |ui: &Shell| -> Vec<String> { ui.global::<crate::GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect() };
    let list_at = |ui: &Shell| -> (bool, i32, usize) {
        let g = ui.global::<crate::GridState>();
        (g.get_list_open(), g.get_list_cursor(), g.get_list().row_count())
    };
    let mut visited: Vec<String> = Vec::new();
    let mut last = String::new();
    let mut first = true;
    for _ in 0..2 * declared.len() + 8 {
        let g = ui.global::<crate::GridState>();
        let at = g.get_tiles().iter().position(|t| t.cursor);
        let Some(t) = at else {
            if first {
                walk_act(f, total, "move:0", 1, list, published); // across from the rail onto the tiles
                first = false;
                continue;
            }
            break; // the cursor came off the tiles (the walk moved up past the first)
        };
        let name = g.get_tiles().row_data(t).unwrap().name.to_string();
        if name == last {
            break; // past the last tile
        }
        if !visited.contains(&name) {
            visited.push(name.clone());
            let kind = g.get_tiles().row_data(t).unwrap().kind;
            walk_act(f, total, "confirm", 1, list, published);
            match kind {
                // A knob or slider: the change, another press lays it down.
                0 | 1 => {
                    let g = ui.global::<crate::GridState>();
                    let t = g.get_tiles().iter().position(|t| t.cursor).expect("the cursor stays while a value is changed");
                    assert!(g.get_tiles().row_data(t).unwrap().editing, "{entry:?} {name}: the press starts the change");
                    walk_act(f, total, "move", 1, list, published);
                    walk_act(f, total, "confirm", 1, list, published);
                    let g = ui.global::<crate::GridState>();
                    let t = g.get_tiles().iter().position(|t| t.cursor).expect("the cursor stays when laid down");
                    assert!(!g.get_tiles().row_data(t).unwrap().editing, "{entry:?} {name}: the second press ends it");
                }
                // A switch: the press flips it; the second flips it back.
                3 => {
                    let g = ui.global::<crate::GridState>();
                    let t = g.get_tiles().iter().position(|t| t.cursor).expect("the cursor stays on the switch");
                    let was = g.get_tiles().row_data(t).unwrap().on;
                    walk_act(f, total, "confirm", 1, list, published);
                    let g = ui.global::<crate::GridState>();
                    let t = g.get_tiles().iter().position(|t| t.cursor).expect("the cursor stays after the flip");
                    assert!(g.get_tiles().row_data(t).unwrap().on != was, "{entry:?} {name}: the second press flips it back");
                }
                // A dropdown: the list opens; every value reached by the dial; EXIT closes it.
                4 => {
                    let (open, _at, n) = list_at(ui);
                    assert!(open, "{entry:?} {name}: the press opens the dropdown");
                    while list_at(ui).1 > 0 {
                        walk_act(f, total, "move", -1, list, published);
                    }
                    for _ in 0..n - 1 {
                        walk_act(f, total, "move", 1, list, published);
                    }
                    assert_eq!(list_at(ui).1, (n - 1) as i32, "{entry:?} {name}: the dial reached the last value");
                    walk_act(f, total, "cancel", 1, list, published);
                    assert!(!list_at(ui).0, "{entry:?} {name}: EXIT closes the dropdown");
                    // EXIT is one step from anywhere: it dropped the tile cursor with the list. The dial
                    // onto the tiles again, and the walk goes on from the first.
                    walk_act(f, total, "move:0", 1, list, published);
                    assert!(ui.global::<crate::GridState>().get_tiles().iter().any(|t| t.cursor), "{entry:?} {name}: the dial shows the cursor again");
                }
                // A value: shown, not offered (read only).
                6 => {}
                other => panic!("{entry:?} {name}: the walk pressed kind {other}, which it knows no effect of"),
            }
        }
        last = name.clone();
        walk_act(f, total, "move", 1, list, published);
    }
    let mut seen: Vec<String> = visited.to_vec();
    seen.sort();
    let mut want: Vec<String> = declared.to_vec();
    want.sort();
    assert_eq!(seen, want, "{entry:?}: the walk visited the tiles, the screen declares {want:?}");
}

#[test]
fn params_reachable_from_home_by_panel_moves() {
    // A plugin whose page has tabs: 26 parameters on the page "Voice" (more than 24, so it has a tabs'
    // rail), in the second-level modules "Tone" and "Amp"; the page "Output" without tabs. 27 in all,
    // so the search is the page's action in the top bar.
    let mut list = Vec::new();
    for i in 0..13u32 {
        list.push(param(i, &format!("Tone {i}"), "Voice/Tone", 1., false, "knob", i + 1));
    }
    for i in 13..23u32 {
        list.push(param(i, &format!("Amp {}", i - 13), "Voice/Amp", 1., false, "knob", i + 1));
    }
    list.push(param(23, "Wave", "Voice/Amp", 5., true, "list", 24));
    list.push(param(24, "Sync", "Voice/Amp", 1., true, "switch", 25));
    list.push(param(25, "Voices", "Voice/Amp", 1., false, "auto", 26));
    list[25]["flags"] = json!(8); // read only: the value alone
    list.push(param(26, "Gain", "Output", 1., false, "knob", 27));
    let list = json!({"parameters": list});
    let name_of = param_names(&list);
    let priority_of = param_priorities(&list);

    let mut f = bar::play_on_the_shell("params-reach", json!({"id": "p1", "name": "Grand", "unsaved": false}), play_graph());
    let ui = f.screen.ui.clone_strong();
    let mut published: Vec<(String, Vec<u64>)> = Vec::new();
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    params_settle(&mut f, &list, &mut published);
    walk_act(&mut f, &mut total, "move", 0, &list, &mut published); // the first read is only a baseline
    // Home → the Performance tile → Play (panel moves only, as on the device).
    assert_eq!(ui.get_screen(), 3, "the shell opens on Play");
    walk_act(&mut f, &mut total, "move:1", 1, &list, &mut published);
    walk_act(&mut f, &mut total, "confirm:1", 1, &list, &mut published);
    assert_eq!(ui.get_screen(), 4, "the press on a part opens the Part screen");
    // The Part screen: the dial down to "Insert effects", pressed, opens the chain.
    let part_cursor = |ui: &Shell| -> Option<String> {
        ui.global::<crate::PartState>().get_rows().iter().find(|r| r.cursor).map(|r| r.label.to_string())
    };
    let mut moves = 0;
    while part_cursor(&ui).as_deref() != Some("Insert effects") {
        walk_act(&mut f, &mut total, "move", 1, &list, &mut published);
        moves += 1;
        let rows: Vec<String> = ui.global::<crate::PartState>().get_rows().iter().map(|r| r.label.to_string()).collect();
        assert!(moves < 60, "the dial never reached Insert effects: {:?} among {rows:?}", part_cursor(&ui));
    }
    walk_act(&mut f, &mut total, "confirm", 1, &list, &mut published);
    assert_eq!(ui.get_screen(), 5, "Insert effects opens the chain");
    // The chain: the dial onto the plugin, pressed: its actions; on to Parameters.
    let mut moves = 0;
    while part_cursor(&ui).as_deref() != Some("1. Rhodes") {
        walk_act(&mut f, &mut total, "move", 1, &list, &mut published);
        moves += 1;
        assert!(moves < 20, "the dial never reached the plugin: {:?}", part_cursor(&ui));
    }
    walk_act(&mut f, &mut total, "confirm", 1, &list, &mut published);
    let mut moves = 0;
    while part_cursor(&ui).as_deref() != Some("Parameters") {
        walk_act(&mut f, &mut total, "move", 1, &list, &mut published);
        moves += 1;
        assert!(moves < 20, "the actions never reached Parameters: {:?}", part_cursor(&ui));
    }
    walk_act(&mut f, &mut total, "confirm", 1, &list, &mut published);
    assert_eq!(ui.get_screen(), 7, "Parameters opens the page");
    // The page: the pages' rail, entry by entry; under it the tabs', and under that the tiles; the
    // walk reads what the screen declares as it shows each entry, and what it visited is held against it.
    let pages = |ui: &Shell| -> Vec<String> { ui.global::<crate::GridState>().get_pages().iter().map(|p| p.to_string()).collect() };
    let tabs = |ui: &Shell| -> Vec<String> { ui.global::<crate::GridState>().get_tabs().iter().map(|t| t.to_string()).collect() };
    let tiles = |ui: &Shell| -> Vec<String> { ui.global::<crate::GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect() };
    let page_cursor = |ui: &Shell| -> i32 { ui.global::<crate::GridState>().get_page_cursor() };
    let tab_cursor = |ui: &Shell| -> i32 { ui.global::<crate::GridState>().get_tab_cursor() };
    let page = |ui: &Shell| -> i32 { ui.global::<crate::GridState>().get_page() };
    let declared_pages = pages(&ui);
    let mut declared_entries: Vec<(String, String, Vec<String>)> = Vec::new(); // (page, tab or "", tiles)
    let mut visited_pages: Vec<String> = Vec::new();
    let mut visited_entry_count = 0;
    for p in 0..declared_pages.len() {
        while page_cursor(&ui) < 0 {
            walk_act(&mut f, &mut total, "move", -1, &list, &mut published); // the dial back out of the tiles (and the tabs' rail)
        }
        while page_cursor(&ui) != p as i32 {
            let by = if page_cursor(&ui) < p as i32 { 1 } else { -1 };
            walk_act(&mut f, &mut total, "move:1", by, &list, &mut published); // along the pages' rail (the rails stand at the side: pane 1 runs along one)
        }
        if !visited_pages.contains(&pages(&ui)[p]) {
            visited_pages.push(pages(&ui)[p].clone());
        }
        walk_act(&mut f, &mut total, "confirm", 1, &list, &mut published); // the page opens, the cursor stays on the rail
        let page_name = pages(&ui)[p].clone();
        let tab_names = tabs(&ui);
        if tab_names.len() > 1 {
            walk_act(&mut f, &mut total, "move:0", 1, &list, &mut published); // across to the tabs' rail, on the page's open tab
            assert!(tab_cursor(&ui) >= 0, "{page_name}: across from the pages' rail is the tabs'");
            let mut visited_tabs: Vec<String> = Vec::new();
            for t in 0..tab_names.len() {
                while tab_cursor(&ui) != t as i32 {
                    let by = if tab_cursor(&ui) < t as i32 { 1 } else { -1 };
                    walk_act(&mut f, &mut total, "move:1", by, &list, &mut published); // along the tabs' rail
                }
                walk_act(&mut f, &mut total, "confirm", 1, &list, &mut published); // the tab opens
                visited_tabs.push(tabs(&ui)[t].clone());
                let declared = tiles(&ui);
                declared_entries.push((page_name.clone(), tabs(&ui)[t].clone(), declared.clone()));
                walk_tiles(&mut f, &mut total, &ui, (page_name.clone(), tabs(&ui)[t].clone()), &declared, &list, &mut published);
                visited_entry_count += 1;
                while tab_cursor(&ui) < 0 {
                    walk_act(&mut f, &mut total, "move", -1, &list, &mut published); // back up to the tabs' rail
                }
            }
            // The tabs the walk visited against what the page declares.
            let mut seen: Vec<String> = visited_tabs.to_vec();
            seen.sort();
            let mut want: Vec<String> = tab_names.to_vec();
            want.sort();
            assert_eq!(seen, want, "{page_name}: the walk visited the tabs, the page declares {want:?}");
        } else {
            let declared = tiles(&ui);
            declared_entries.push((page_name.clone(), String::new(), declared.clone()));
            walk_tiles(&mut f, &mut total, &ui, (page_name.clone(), String::new()), &declared, &list, &mut published); // across to the tiles (walk_tiles descends)
            visited_entry_count += 1;
        }
        while page_cursor(&ui) < 0 {
            walk_act(&mut f, &mut total, "move", -1, &list, &mut published); // back up to the pages' rail
        }
    }
    // The pages the walk visited against what the screen declares, and every entry once.
    let mut seen: Vec<String> = visited_pages.to_vec();
    seen.sort();
    let mut want: Vec<String> = declared_pages.to_vec();
    want.sort();
    assert_eq!(seen, want, "the walk visited the pages, the screen declares {want:?}");
    let mut entries: Vec<(String, String)> = declared_entries.iter().map(|e| (e.0.clone(), e.1.clone())).collect();
    entries.sort();
    entries.dedup();
    assert_eq!(entries.len(), declared_entries.len(), "an entry visited twice: {declared_entries:?}");
    assert_eq!(visited_entry_count, entries.len(), "the walk visited an entry twice: {visited_entry_count} of {entries:?}");
    // The top bar's action of the page: the search (a plugin of more than 24 parameters), by the third
    // pane, the dial along the bar, the press; EXIT closes the keyboard over the screen.
    let bar_labels = |ui: &Shell| -> Vec<String> { ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect() };
    assert!(bar_labels(&ui).iter().any(|l| l.starts_with("Search")), "the page declares a search among its bar actions: {:?}", bar_labels(&ui));
    walk_act(&mut f, &mut total, "move:2", 1, &list, &mut published);
    assert!(ui.global::<Bar>().get_back_cursor(), "the third pane enters the bar at Back");
    let mut moves = 0;
    while !ui.global::<Bar>().get_actions().iter().any(|a| a.cursor && a.label.to_string().starts_with("Search")) && !ui.global::<Bar>().get_more_cursor() {
        walk_act(&mut f, &mut total, "move", 1, &list, &mut published);
        moves += 1;
        assert!(moves < 12, "the bar's cursor never reached the search: {:?}", bar_labels(&ui));
    }
    walk_act(&mut f, &mut total, "confirm", 1, &list, &mut published);
    if ui.global::<crate::MenuState>().get_open() {
        // The bar folded its actions into More: the search is a row of the menu.
        let mut moves = 0;
        while !ui.global::<crate::MenuState>().get_rows().iter().any(|r| r.cursor && r.label.to_string().starts_with("Search")) {
            walk_act(&mut f, &mut total, "move", 1, &list, &mut published);
            moves += 1;
            assert!(moves < 12, "the menu's cursor never reached the search");
        }
        walk_act(&mut f, &mut total, "confirm", 1, &list, &mut published);
    }
    assert!(ui.global::<crate::KeyboardState>().get_open(), "the search opens the text keyboard");
    walk_act(&mut f, &mut total, "cancel", 1, &list, &mut published);
    assert!(!ui.global::<crate::KeyboardState>().get_open() && ui.get_screen() == 7, "EXIT closes it, the page kept");
    // The banks: what was last published for the device's screen knobs is the page shown and its tiles
    // by priority (the knobs play the page's banks).
    let shown_page = pages(&ui)[page(&ui).max(0) as usize].clone();
    let mut order: Vec<(u64, u64)> = tiles(&ui).iter().map(|n| {
        let id = *name_of.iter().find(|(_, m)| *m == n).expect("a tile of the plugin's parameter").0;
        (id, priority_of[&id])
    }).collect();
    order.sort_by_key(|(_, pr)| *pr);
    let ids: Vec<u64> = order.iter().map(|(id, _)| *id).collect();
    let last = published.last().unwrap_or_else(|| panic!("the page published nothing for the screen knobs"));
    assert_eq!(last, &(shown_page, ids), "the banks follow the page shown and its tiles by priority; published so far: {published:?}");
    eprintln!("visited {} pages and {visited_entry_count} entries; the last published page has {} banks", visited_pages.len(), published.last().map(|(_, p)| p.len()).unwrap_or(0));
}
