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

#[test]
fn piano_draws_in_every_theme() {
    let mut s = Screen::new();
    let mut p = open(&s, "Roland");
    p.touch(5, true, 0.5);
    p.act(Action::Move { pane: Some(1), by: 1 });
    let places = theme::Places::default();
    let mut loaded: Vec<theme::Loaded> = ["oscairn-dark", "oscairn-light"].iter().map(|id| theme::chosen(&places, id, "")).collect();
    loaded.push(odd_theme());
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        png(&s, &format!("piano-{}", t.id));
    }
}
