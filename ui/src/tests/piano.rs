// SPDX-License-Identifier: AGPL-3.0-or-later
//! The on-screen playing keyboard's tests (src/piano.rs): what the JUCE KeyboardWindow offers (notes as
//! an input's, the computer's keys, octaves, Fixed and Sustain, bands, the input's held keys lit, closing
//! when the input goes), the knobs and cursor added for use without touch, and the shell opening it.

use super::*;
use crate::piano::{Outcome, Piano};
use crate::PianoState;

fn keys(ui: &Shell) -> Vec<crate::PianoKey> {
    ui.global::<PianoState>().get_keys().iter().collect()
}

/// What it sent as (method, on, key, velocity, input) after the last call.
fn sent(p: &mut Piano) -> Vec<(bool, u64, Option<f64>, Option<String>)> {
    std::mem::take(&mut p.out).into_iter().map(|(m, v)| {
        assert_eq!(m, "notes.send");
        (v["on"].as_bool().unwrap(), v["key"].as_u64().unwrap(), v["velocity"].as_f64(), v["input"].as_str().map(String::from))
    }).collect()
}

fn open(s: &Screen, input: &str) -> Piano {
    Piano::open(&s.ui, input, &json!({"midiInputs": [{"source": 3, "name": "Roland", "kind": "usb", "port": 0}], "heldNotes": []}), &super::part::part_graph())
}

#[test]
fn touch_plays_as_the_input_with_fixed_velocity_and_sustain() {
    let s = Screen::new();
    let mut p = open(&s, "Roland");
    assert_eq!(keys(&s.ui).len(), 25, "two octaves and the next C");
    assert_eq!(s.ui.global::<PianoState>().get_range().as_str(), "C3–C5");
    p.touch(0, true, 0.5);
    p.touch(0, false, 0.);
    assert_eq!(sent(&mut p), vec![(true, 48, Some(0.5), Some("Roland".into())), (false, 48, None, Some("Roland".into()))]);
    // Fixed: every note at 100/127, wherever the finger is.
    p.tap_control(2);
    p.touch(4, true, 0.2);
    assert_eq!(sent(&mut p)[0].2.map(|v| (v * 127.).round()), Some(100.));
    // Sustain: let go keys keep sounding until it is off.
    p.tap_control(3);
    p.touch(4, false, 0.);
    assert!(sent(&mut p).is_empty());
    assert!(keys(&s.ui)[4].held, "still shown sounding");
    p.tap_control(3);
    assert_eq!(sent(&mut p), vec![(false, 52, None, Some("Roland".into()))]);
    // The built-in keys send no input.
    let mut b = open(&s, "");
    b.touch(1, true, 1.);
    assert_eq!(sent(&mut b), vec![(true, 49, Some(1.0), None)]);
}

#[test]
fn computers_keys_and_octaves() {
    let s = Screen::new();
    let mut p = open(&s, "");
    assert!(p.typed("a", true) && p.typed("A", true), "its repeats pass: one note");
    assert!(p.typed(";", true));
    assert_eq!(sent(&mut p).iter().map(|n| (n.0, n.1, n.2)).collect::<Vec<_>>(), vec![(true, 60, Some(0.8)), (true, 76, Some(0.8))]);
    assert!(p.typed("a", false) && p.typed(";", false));
    assert_eq!(sent(&mut p).iter().map(|n| (n.0, n.1)).collect::<Vec<_>>(), vec![(false, 60), (false, 76)]);
    assert!(!p.typed("q", true), "not the keyboard's key");
    // Z and X: an octave down and up; a key held goes off where it began.
    p.typed("k", true);
    p.typed("x", true);
    assert_eq!(p.lowest(), 60);
    assert_eq!(sent(&mut p).iter().map(|n| (n.0, n.1)).collect::<Vec<_>>(), vec![(true, 72), (false, 72)]);
    for _ in 0..8 {
        p.typed("z", true);
    }
    assert_eq!(p.lowest(), 0, "four octaves down at most");
    p.typed("a", true);
    assert_eq!(sent(&mut p)[0].1, 12);
}

#[test]
fn knobs_and_cursor_without_touch() {
    let s = Screen::new();
    let mut p = open(&s, "");
    // The left knob: the octave.
    assert_eq!(p.act(Action::Move { pane: Some(0), by: 3 }), Outcome::Open);
    assert_eq!(s.ui.global::<PianoState>().get_range().as_str(), "C4–C6");
    p.act(Action::Move { pane: Some(0), by: -1 });
    // The right knob and the dial: the cursor, first on middle C (note 60 of C3..C5).
    p.act(Action::Move { pane: Some(1), by: 1 });
    assert_eq!(keys(&s.ui).iter().position(|k| k.cursor), Some(12));
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(keys(&s.ui).iter().position(|k| k.cursor), Some(13));
    p.act(Action::Confirm(None));
    assert_eq!(sent(&mut p).iter().map(|n| (n.0, n.1)).collect::<Vec<_>>(), vec![(true, 61)]);
    assert!(p.tapping() && keys(&s.ui)[13].held);
    p.tap_end();
    assert_eq!(sent(&mut p).iter().map(|n| (n.0, n.1)).collect::<Vec<_>>(), vec![(false, 61)]);
    // Past the last key: Octave down, Octave up, Fixed, Sustain, Close.
    p.act(Action::End(1));
    let controls = || s.ui.global::<PianoState>().get_controls().iter().map(|c| (c.label.to_string(), c.on, c.cursor)).collect::<Vec<_>>();
    assert_eq!(controls().last().unwrap(), &("Close".to_string(), false, true));
    p.act(Action::Move { pane: None, by: -1 });
    p.act(Action::Confirm(None));
    assert_eq!(controls()[3], ("Sustain".to_string(), true, true));
    // Cancel drops the cursor (sustain is let go with the keyboard), then closes.
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Open);
    assert!(controls().iter().all(|c| !c.2));
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Close);
    p.act(Action::End(1));
    assert_eq!(p.act(Action::Confirm(None)), Outcome::Close);
}

#[test]
fn shows_what_the_input_holds_the_parts_it_plays_and_goes_with_its_input() {
    let s = Screen::new();
    let mut p = open(&s, "Roland");
    let status = json!({"midiInputs": [{"source": 3, "name": "Roland", "kind": "usb", "port": 0}], "heldNotes": [{"source": 3, "keys": [50, 52]}, {"source": 0, "keys": [60]}]});
    assert!(p.status(&status));
    let lit: Vec<usize> = keys(&s.ui).iter().enumerate().filter(|k| k.1.lit).map(|k| k.0).collect();
    assert_eq!(lit, vec![2, 4], "the input's keys, not the built-in's");
    // Both parts of the graph play from any device; the Keys part's zone is 36 to 96, Low's the whole range.
    assert_eq!(p.zone_names().iter().map(|z| (z.0.as_str(), z.1, z.2)).collect::<Vec<_>>(), vec![("Keys", 36, 96), ("Low", 0, 127)]);
    assert_eq!(s.ui.global::<PianoState>().get_bands().row_count(), 2);
    // Unplugged: it says so, and its held keys are let go.
    p.touch(0, true, 1.);
    p.out.clear();
    assert!(!p.status(&json!({"midiInputs": [], "heldNotes": []})));
    p.close();
    assert_eq!(sent(&mut p), vec![(false, 48, None, Some("Roland".into()))]);
    assert!(!s.ui.global::<PianoState>().get_open());
    // The built-in keys stay with no inputs at all.
    let mut b = open(&s, "");
    assert!(b.status(&json!({"midiInputs": [], "heldNotes": [{"source": 0, "keys": [48]}]})));
    assert!(keys(&s.ui)[0].lit);
}

#[test]
fn only_a_named_device_plays_its_own_parts() {
    let s = Screen::new();
    let mut graph = super::part::part_graph();
    graph["parts"][0]["input"]["devices"] = json!([{"kind": "usbNamed", "port": 0, "pattern": "Roland", "match": "contains"}]);
    graph["parts"][1]["input"]["devices"] = json!([{"kind": "builtIn", "port": 0, "pattern": "", "match": "contains"}]);
    let status = json!({"midiInputs": [{"source": 3, "name": "Roland A-88", "kind": "usb", "port": 0}], "heldNotes": []});
    let names = |input: &str| Piano::open(&s.ui, input, &status, &graph).zone_names().into_iter().map(|z| z.0).collect::<Vec<_>>();
    assert_eq!(names("Roland A-88"), vec!["Keys"]);
    assert_eq!(names(""), vec!["Low"]);
}

#[test]
fn the_part_screen_offers_it_where_the_profile_does() {
    let s = Screen::new();
    let mut part = super::part::part_screen(&s, &super::part::part_graph(), 10);
    let labels = |ui: &Shell| ui.global::<crate::PartState>().get_rows().iter().map(|r| r.label.to_string()).collect::<Vec<_>>();
    assert!(labels(&s.ui).contains(&"Keyboard".to_string()));
    part.keys = false;
    part.graph(&super::part::part_graph());
    assert!(!labels(&s.ui).contains(&"Keyboard".to_string()), "hidden when onScreenKeyboard is false");
    assert!(!crate::profile::Profile::load(Some("fantom-07")).unwrap().on_screen_keyboard);
    assert!(crate::profile::Profile::load(None).unwrap().on_screen_keyboard);
    assert!(!crate::profile::Profile::parse(r#"{"format":"oscairn.profile","controls":[{"kind":"keys","role":"notes"}]}"#).unwrap().on_screen_keyboard);
    assert!(crate::profile::Profile::parse(r#"{"format":"oscairn.profile","onScreenKeyboard":true,"controls":[{"kind":"keys","role":"notes"}]}"#).unwrap().on_screen_keyboard);
}

#[test]
fn the_shell_sends_its_notes_and_closes_it() {
    let mut f = Fixture::new("piano");
    f.engine.answer(); // session.hello
    f.pump();
    shell::open_piano("");
    let ui = f.screen.ui.clone_strong();
    assert!(ui.global::<PianoState>().get_open());
    let note = |f: &mut Fixture| loop {
        let r = f.engine.request();
        if r["method"] == "notes.send" {
            return r["params"].clone();
        }
    };
    shell::act(Action::Move { pane: Some(1), by: 1 });
    shell::act(Action::Confirm(None));
    assert_eq!(note(&mut f), json!({"on": true, "key": 60, "velocity": 0.8}));
    // The note ends after its tap.
    std::thread::sleep(crate::piano::TAP + std::time::Duration::from_millis(100));
    slint::platform::update_timers_and_animations();
    assert_eq!(note(&mut f), json!({"on": false, "key": 60}));
    // Z on the computer's keyboard moves the octave; Cancel twice closes it.
    assert!(shell::key("z"));
    assert_eq!(ui.global::<PianoState>().get_range().as_str(), "C2–C4");
    shell::act(Action::Cancel(None));
    shell::act(Action::Cancel(None));
    assert!(!ui.global::<PianoState>().get_open());
}

/// The note number of the keyboard's first key, from the range it shows ("C3–C5": 48).
fn range_low(ui: &Shell) -> i64 {
    let range = ui.global::<PianoState>().get_range().to_string();
    let first = range.split('–').next().unwrap_or("");
    let octave: i64 = first.chars().skip(1).take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(3);
    (octave + 1) * 12
}

fn cursor_key(ui: &Shell) -> Option<usize> {
    ui.global::<PianoState>().get_keys().iter().position(|k| k.cursor)
}

fn cursor_control(ui: &Shell) -> Option<usize> {
    ui.global::<PianoState>().get_controls().iter().position(|c| c.cursor)
}

// The no-touch walk: from Home by panel moves only, through the Performance tile, Play and the part to
// the playing keyboard, then every stop its models declare is a dial stop and pressing it has its real
// effect — a key sounds with the part's input (notes.send, the note the range and the position give),
// Octave down/up and the octave knob move the range, Fixed and Sustain light their controls, Close is
// back on the Part screen. The cursor first lands on middle C, so the walk sweeps back to the first
// key and then forward through every key and the header buttons.
#[test]
fn piano_walked_from_home_by_panel_moves_only() {
    let graph = super::part::part_graph();
    let mut f = Fixture::on("piano-walk", None, false); // the computer's profile offers the playing keyboard
    f.engine.answer(); // session.hello
    f.pump();
    let ui = f.screen.ui.clone_strong();
    let settle = |f: &mut Fixture| {
        while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(12)) {
            let method = r["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "graph.get" => graph.clone(),
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "library.query" => json!({"items": [], "total": 0}),
                "library.libraries" => json!({"libraries": []}),
                "session.status" => json!({"midiInputs": [], "midiOutputs": [], "failures": []}),
                "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
                "edit.history" | "edit.undo" | "edit.redo" => json!({"undo": "", "redo": ""}),
                "window.info" => json!({"available": false}),
                "presets.list" => json!({"presets": []}),
                "record.get" => json!({"recording": 0, "retro": {"seconds": 30}, "takes": []}),
                "parts.meters" => json!({"meters": []}),
                _ => json!({}),
            };
            f.reply(&r, json!({"result": result}));
        }
    };
    settle(&mut f);
    // Panel moves only, from Home: the cursor onto the Performance tile, confirmed, opens Play.
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |f: &mut Fixture, key: &str, by: i64| {
        *total.entry(key.into()).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move", 0); // the first read is only a baseline
    settle(&mut f);
    nav(&mut f, "move", 1); // the cursor onto the Performance tile
    settle(&mut f);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert_eq!(ui.get_screen(), 3, "the Performance tile opens Play");
    // Play → the first part, pressed, opens the Part screen.
    nav(&mut f, "move:1", 1);
    nav(&mut f, "confirm:1", 1);
    settle(&mut f);
    assert_eq!(ui.get_screen(), 4, "the press on a part opens the Part screen");
    // The Part screen: the dial down the rows, page to page, until the Keyboard row is under the cursor, pressed.
    let keyboard_row = |ui: &Shell| ui.global::<crate::PartState>().get_rows().iter().position(|r| r.label == "Keyboard");
    let row_cursor = |ui: &Shell| ui.global::<crate::PartState>().get_rows().iter().position(|r| r.cursor);
    let mut moves = 0;
    nav(&mut f, "move", 1); // show the cursor on the first row
    settle(&mut f);
    while row_cursor(&ui) != keyboard_row(&ui) {
        nav(&mut f, "move", 1);
        settle(&mut f);
        moves += 1;
        assert!(moves < 400, "the dial never reached the Keyboard row");
    }
    nav(&mut f, "confirm", 1);
    assert!(ui.global::<PianoState>().get_open(), "the Keyboard row opens the playing keyboard");
    // What the screen offers, from its own models: the keys and the header buttons.
    let (keys_n, controls_n) = {
        let s = ui.global::<PianoState>();
        (s.get_keys().iter().count(), s.get_controls().iter().count())
    };
    assert!(keys_n > 0 && controls_n > 0, "the keyboard declares keys and controls");
    let declared = keys_n + controls_n;
    // A note as the engine reads it: (on, key, velocity), velocity only on a note on; the built-in keys add no input.
    let notes = |f: &mut Fixture| -> Vec<serde_json::Value> {
        let mut out = vec![];
        while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(12)) {
            if r["method"] == "notes.send" {
                out.push(r["params"].clone());
            } else {
                f.reply(&r, json!({"result": {}}));
            }
        }
        out
    };
    // The cursor first lands on a key of its own (middle C); sweep back to the first key, pressing each.
    nav(&mut f, "move:1", 1);
    settle(&mut f);
    let mut stop = Vec::new(); // the stops pressed: keys 0..keys_n, then the controls
    let expected = range_low(&ui);
    let mut previous: Option<i64> = None;
    let start = cursor_key(&ui).expect("the first move puts the cursor on a key");
    for k in (0..start).rev() {
        nav(&mut f, "move:1", -1);
        assert_eq!(cursor_key(&ui), Some(k), "the dial reached key {k}");
        nav(&mut f, "confirm", 1);
        let note = notes(&mut f);
        let n = note.last().expect("a key press notes with the key's input");
        assert_eq!(n["on"], json!(true), "key {k}: the press plays a note");
        assert_eq!(n["key"].as_i64(), Some(expected + k as i64), "key {k}: the note is the range's");
        assert_eq!(n["velocity"].as_f64(), Some(0.8), "key {k}: the built-in key, not Fixed, plays at 0.8");
        assert!(n["input"].is_null(), "key {k}: the built-in keys send no input");
        if let Some(p) = previous {
            assert_eq!(note[0]["on"], json!(false), "key {k}: the press lets the previous note {p} go");
            assert_eq!(note[0]["key"].as_i64(), Some(p), "key {k}: the released note is the previous one");
        }
        assert!(keys(&ui)[k].held, "key {k}: the key is shown held");
        previous = Some(expected + k as i64);
        stop.push(("key", k));
    }
    // Forward through every key from the first, pressing each.
    for k in 1..keys_n {
        nav(&mut f, "move:1", 1);
        assert_eq!(cursor_key(&ui), Some(k), "the dial reached key {k}");
        nav(&mut f, "confirm", 1);
        let note = notes(&mut f);
        let n = note.last().expect("a key press notes with the key's input");
        assert_eq!(n["on"], json!(true), "key {k}: the press plays a note");
        assert_eq!(n["key"].as_i64(), Some(expected + k as i64), "key {k}: the note is the range's");
        assert_eq!(n["velocity"].as_f64(), Some(0.8), "key {k}: the built-in key plays at 0.8");
        assert!(n["input"].is_null(), "key {k}: the built-in keys send no input");
        assert_eq!(note[0]["on"], json!(false), "key {k}: the press lets the previous note go");
        assert_eq!(note[0]["key"].as_i64(), Some(expected + (k - 1) as i64), "key {k}: the released note is the previous one");
        assert!(keys(&ui)[k].held, "key {k}: the key is shown held");
        stop.push(("key", k));
    }
    // The octave knob (the left knob, pane 0): a move shifts the range; the cursor stays on the keys.
    nav(&mut f, "move:0", 1);
    assert_eq!(range_low(&ui), expected + 12, "the octave knob moves an octave up");
    nav(&mut f, "move:0", -1);
    assert_eq!(range_low(&ui), expected, "the octave knob moves back down");
    // On to the header buttons and pressing each: Octave down, Octave up, Fixed, Sustain, Close.
    for c in 0..controls_n {
        nav(&mut f, "move:1", 1);
        assert_eq!(cursor_control(&ui), Some(c), "the dial reached the {c}. control");
        let (label, was_on) = {
            let s = ui.global::<PianoState>();
            let ctl = s.get_controls().iter().nth(c).expect("the control is declared");
            (ctl.label.to_string(), ctl.on)
        };
        nav(&mut f, "confirm", 1);
        let _ = notes(&mut f);
        match label.as_str() {
            "−" => assert_eq!(range_low(&ui), expected - 12, "Octave down lowers the range"),
            "+" => assert_eq!(range_low(&ui), expected, "Octave up restores it"),
            "Fixed" | "Sustain" => {
                let on = ui.global::<PianoState>().get_controls().iter().nth(c).unwrap().on;
                assert!(on && !was_on, "{label}: the control turns on");
            }
            _ => {
                assert!(!ui.global::<PianoState>().get_open(), "Close ends the keyboard");
                assert_eq!(ui.get_screen(), 4, "Close is back on the Part screen");
            }
        }
        stop.push(("control", c));
        if label == "Close" {
            break;
        }
    }
    // Every stop the screen declared was reached: the keys then the controls, nothing hard-coded.
    let distinct: std::collections::HashSet<_> = stop.iter().collect();
    assert_eq!(distinct.len(), declared, "the walk reached {stop:?}: the screen declares {declared} stops");
}

#[test]
fn piano_draws_in_every_theme() {
    let mut s = Screen::new();
    let mut p = open(&s, "Roland");
    p.touch(5, true, 0.5);
    p.act(Action::Move { pane: Some(1), by: 1 });
    let loaded = all_themes();
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        png(&s, &format!("piano-{}", t.id));
        shot(&s, &format!("piano-{}", t.id));
    }
    at_fantom_scale(&mut s, "piano", || ());
}
