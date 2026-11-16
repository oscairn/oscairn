// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Mixer (H1, H2): read as the JUCE mixer page reads graph.get, the cursor over the controls of the
//! strips, each edit's request with its commit, touch, the strips and sends a view holds following the
//! window, and on the shell (the Parts list's Mixer button, the bar, Back) through the fake engine.

use super::bar::{play_on_the_shell, request_in};
use super::parts::parts_graph;
use super::*;
use crate::mixer::{Outcome, Screen as Mixer};
use crate::{MixerState, PartsState};

/// The Parts list's Performance with a pan, a solo and a master pan set: four parts, two buses.
fn graph() -> serde_json::Value {
    let mut g = parts_graph();
    g["parts"][0]["pan"] = json!(0.25);
    g["parts"][1]["solo"] = json!(true);
    g["master"] = json!({"level": 1.0, "pan": -0.5});
    g
}

fn mixer_screen(s: &mut Screen, graph: &serde_json::Value) -> Mixer {
    let mut m = Mixer::new(&s.ui);
    s.ui.set_screen(10);
    s.frame();
    m.graph(graph);
    s.frame();
    m.shown();
    m
}

/// Each strip of the view: its name and the control the cursor is on (-1 none).
fn strips(ui: &Shell) -> Vec<(String, i32, bool)> {
    ui.global::<MixerState>().get_strips().iter().map(|r| (r.name.to_string(), r.cursor, r.editing)).collect()
}

fn master(ui: &Shell) -> (i32, bool) {
    let m = ui.global::<MixerState>().get_master();
    (m.cursor, m.editing)
}

/// Where the cursor is: the strip's name and its control (the master is "Master").
fn at(ui: &Shell) -> Option<(String, i32)> {
    strips(ui).into_iter().find(|r| r.1 >= 0).map(|r| (r.0, r.1)).or_else(|| (master(ui).0 >= 0).then(|| ("Master".to_string(), master(ui).0)))
}

fn edits(m: &mut Mixer) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(&mut m.out).into_iter().map(|(_, method, p)| (method, p)).filter(|(method, _)| *method != "graph.commit").collect()
}

fn mv(m: &mut Mixer, pane: Option<u8>, by: i32) -> Outcome {
    m.act(Action::Move { pane, by })
}

// graph.get as the JUCE mixer page reads it: the parts, then the buses, each with its level, pan, mute,
// solo and the send to each bus in the order of the buses, and the master's level and pan.
#[test]
fn the_mixer_reads_parts_buses_and_the_master_as_the_juce_page_does() {
    let (strips, buses, master) = crate::mixer::read(&graph());
    assert_eq!(strips.iter().map(|s| (s.name.as_str(), s.bus)).collect::<Vec<_>>(), [("Keys", false), ("Low", false), ("Pad", false), ("Empty", false), ("Reverb", true), ("Delay", true)]);
    assert_eq!(buses, [(12, "Reverb".to_string()), (13, "Delay".to_string())]);
    assert_eq!(strips.iter().map(|s| s.pan).collect::<Vec<_>>(), [0.25, 0., 0., 0., 0., 0.]);
    assert_eq!(strips.iter().map(|s| s.solo).collect::<Vec<_>>(), [false, true, false, false, false, false]);
    assert_eq!(strips.iter().map(|s| (s.level, s.mute)).collect::<Vec<_>>(), [(0.5, false), (1.0, false), (0.8, true), (1.0, false), (0.8, false), (0.6, false)]);
    assert_eq!(strips[0].sends, [Some(0.5), None]);
    assert_eq!(strips[2].sends, [Some(0.3), Some(0.7)]);
    assert_eq!(master, (1.0, -0.5));
}

// The window shows them as the JUCE strips do: the fader through the skew (a quarter of the way up the
// level's range is halfway up the travel), the level in dB with one decimal and minus infinity at nothing,
// the pan across, only a part with sends, a faint strip when muted, and the master's own range of 0 to 2.
#[test]
fn strips_show_fader_db_pan_sends_and_the_master() {
    let mut s = Screen::sized(1280, 900);
    let _m = mixer_screen(&mut s, &graph());
    let state = s.ui.global::<MixerState>();
    let rows: Vec<_> = state.get_strips().iter().collect();
    assert_eq!(rows.len(), 6);
    assert_eq!((rows[0].db.as_str(), rows[1].db.as_str()), ("-6.0 dB", "0.0 dB"));
    assert!((rows[0].fader - 0.5f32.sqrt()).abs() < 1e-6, "the fader's place is the square root of the level");
    assert_eq!((rows[0].pan, rows[3].pan), (0.625, 0.5));
    assert_eq!(rows[0].sends.iter().collect::<Vec<_>>(), [0.5, 0.0], "a send each bus; none is zero");
    assert_eq!(rows[4].sends.row_count(), 0, "a bus has none");
    assert_eq!((rows[2].mute, rows[2].dim, rows[1].solo, rows[0].dim), (true, true, true, false));
    assert_eq!(state.get_send_names().iter().map(|n| n.to_string()).collect::<Vec<_>>(), ["Reverb", "Delay"]);
    let master = state.get_master();
    assert_eq!((master.name.as_str(), master.db.as_str(), master.pan, master.master), ("Master", "0.0 dB", 0.25, true));
    assert!((master.fader - 0.5f32.sqrt()).abs() < 1e-6, "level 1 of 2 is a half of the range: the square root of a half up");
    drop(state);
    let mut quiet = graph();
    quiet["parts"][0]["level"] = json!(0.0);
    quiet["master"]["level"] = json!(2.0);
    let _m = mixer_screen(&mut s, &quiet);
    let state = s.ui.global::<MixerState>();
    assert_eq!(state.get_strips().row_data(0).unwrap().db.as_str(), "−∞ dB");
    assert_eq!((state.get_master().fader, state.get_master().db.as_str()), (1.0, "6.0 dB"));
}

// The cursor goes over the controls of the strips: the dial strip by strip, control by control (fader, M, S,
// pan, then the sends), the left knob along the strips keeping the control, the right knob up and down the
// strip. A bus has no sends and the master only a fader and a pan, so the control moves to the nearest.
#[test]
fn the_cursor_moves_over_controls_and_strips() {
    let mut s = Screen::sized(1280, 900);
    let mut m = mixer_screen(&mut s, &graph());
    let ui = &s.ui;
    assert_eq!(at(ui), None);
    mv(&mut m, None, 1);
    assert_eq!(at(ui), Some(("Keys".into(), 0)), "the first move shows it on the first fader");
    for _ in 0..3 {
        mv(&mut m, None, 1);
    }
    assert_eq!(at(ui), Some(("Keys".into(), 3)), "fader, M, S, pan");
    mv(&mut m, None, 2);
    assert_eq!(at(ui), Some(("Keys".into(), 5)), "the two sends");
    mv(&mut m, None, 1);
    assert_eq!(at(ui), Some(("Low".into(), 0)), "on to the next strip");
    mv(&mut m, None, -1);
    assert_eq!(at(ui), Some(("Keys".into(), 5)));
    // The right knob down the strip, held at the ends; the left knob along the strips.
    mv(&mut m, Some(1), 1);
    assert_eq!(at(ui), Some(("Keys".into(), 5)));
    mv(&mut m, Some(1), -2);
    assert_eq!(at(ui), Some(("Keys".into(), 3)));
    mv(&mut m, Some(0), 1);
    assert_eq!(at(ui), Some(("Low".into(), 3)), "the same control, the next strip");
    mv(&mut m, Some(1), 2);
    mv(&mut m, Some(0), 3);
    assert_eq!(at(ui), Some(("Reverb".into(), 3)), "a bus has no sends: the pan is the last it has");
    mv(&mut m, Some(0), 1);
    mv(&mut m, Some(0), 1);
    assert_eq!(at(ui), Some(("Master".into(), 3)), "past the last bus: the master, held there");
    mv(&mut m, Some(1), 1);
    assert_eq!(at(ui), Some(("Master".into(), 3)));
    mv(&mut m, Some(1), -1);
    assert_eq!(at(ui), Some(("Master".into(), 0)), "the master has a fader and a pan only");
    mv(&mut m, None, 1);
    assert_eq!(at(ui), Some(("Master".into(), 3)), "the dial stops at the last control");
    mv(&mut m, None, -1);
    mv(&mut m, None, -1);
    assert_eq!(at(ui), Some(("Delay".into(), 3)), "back across the strips");
    // Arrows: left and right along the strips, up and down the strip.
    m.act(Action::Arrow { dx: 0, dy: -1 });
    m.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(at(ui), Some(("Reverb".into(), 2)));
    m.act(Action::End(-1));
    assert_eq!(at(ui), Some(("Keys".into(), 0)));
    assert!(m.at_top());
    m.act(Action::End(1));
    assert_eq!(at(ui), Some(("Master".into(), 0)));
    assert!(!m.at_top());
    assert!(edits(&mut m).is_empty(), "moving sends nothing");
    // Cancel drops the cursor, then goes back.
    assert_eq!(m.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(at(ui), None);
    assert_eq!(m.act(Action::Cancel(None)), Outcome::Back);
}

// A press on a fader, the pan or a send starts a change the dial then makes, a fortieth of the travel a
// click (the fader's through its skew), each step one request with a commit after it; M and S toggle on a
// press; the master's fader and pan are `master.set` with the other kept. A press or Cancel ends a change.
#[test]
fn a_press_changes_a_fader_a_pan_a_send_and_toggles_mute_and_solo() {
    let mut s = Screen::sized(1280, 900);
    let mut m = mixer_screen(&mut s, &graph());
    let ui = &s.ui;
    mv(&mut m, None, 1);
    // The fader.
    assert_eq!(m.act(Action::Confirm(None)), Outcome::Stay);
    assert!(strips(ui)[0].2, "being changed");
    mv(&mut m, None, 1);
    let sent = std::mem::take(&mut m.out);
    assert_eq!(sent.iter().map(|o| o.1).collect::<Vec<_>>(), ["parts.level", "graph.commit"]);
    let want = crate::play::level_at(crate::play::fraction(0.5, 1.) + 0.025, 1.);
    assert_eq!(sent[0].2, json!({"part": 10, "level": want, "mute": false}));
    assert!(want > 0.5 && want < 0.55);
    mv(&mut m, None, -1);
    assert_eq!(edits(&mut m)[0].1["level"], json!(0.5), "a click back is where it was");
    mv(&mut m, Some(1), 1);
    assert_eq!(at(ui), Some(("Keys".into(), 0)), "the right knob does not leave a value being changed");
    m.act(Action::Cancel(None));
    assert!(!strips(ui)[0].2 && at(ui) == Some(("Keys".into(), 0)), "Cancel ends the change, the cursor stays");
    m.act(Action::Confirm(None));
    mv(&mut m, None, 100);
    assert_eq!(edits(&mut m).last().unwrap().1["level"], json!(1.0), "held at the top");
    mv(&mut m, None, 1);
    assert!(edits(&mut m).is_empty(), "nothing at the end");
    m.act(Action::Confirm(None));
    assert!(!strips(ui)[0].2, "a press ends it as well");
    // Mute and solo toggle, keeping the level; the mute goes with `parts.level`, the solo is `parts.solo`.
    mv(&mut m, Some(1), 1);
    m.act(Action::Confirm(None));
    assert_eq!(edits(&mut m), [("parts.level", json!({"part": 10, "level": 1.0, "mute": true}))]);
    m.act(Action::Confirm(None));
    assert_eq!(edits(&mut m)[0].1["mute"], json!(false), "a second press undoes it");
    mv(&mut m, Some(1), 1);
    m.act(Action::Confirm(None));
    assert_eq!(edits(&mut m), [("parts.solo", json!({"part": 10, "solo": true}))]);
    // The pan: a click is a fortieth of the travel, 0.05 of the whole -1 to 1.
    mv(&mut m, Some(1), 1);
    m.act(Action::Confirm(None));
    mv(&mut m, None, 1);
    assert_eq!(edits(&mut m), [("parts.pan", json!({"part": 10, "pan": 0.3}))]);
    mv(&mut m, None, -6);
    assert_eq!(edits(&mut m).last().unwrap().1["pan"], json!(0.0), "through the middle");
    m.act(Action::Cancel(None));
    // A send: a press, then the dial; taken to nothing it is removed.
    mv(&mut m, Some(1), 1);
    m.act(Action::Confirm(None));
    mv(&mut m, None, 1);
    assert_eq!(edits(&mut m), [("parts.send", json!({"part": 10, "bus": 12, "level": 0.525}))]);
    mv(&mut m, None, -100);
    let sent = edits(&mut m);
    assert_eq!(sent.last().unwrap(), &("parts.send", json!({"part": 10, "bus": 12})), "no level: the send is removed");
    m.act(Action::Cancel(None));
    // The second send adds one that was not there.
    mv(&mut m, Some(1), 1);
    m.act(Action::Confirm(None));
    mv(&mut m, None, 2);
    assert_eq!(edits(&mut m).last().unwrap(), &("parts.send", json!({"part": 10, "bus": 13, "level": 0.05})));
    m.act(Action::Cancel(None));
    // The master's level and pan, each keeping the other.
    m.act(Action::End(1));
    m.act(Action::Confirm(None));
    mv(&mut m, None, 1);
    let sent = edits(&mut m);
    assert_eq!(sent[0].0, "master.set");
    assert_eq!(sent[0].1["pan"], json!(-0.5));
    assert!(sent[0].1["level"].as_f64().unwrap() > 1.0, "the master goes up to 2");
    m.act(Action::Cancel(None));
    mv(&mut m, Some(1), 1);
    m.act(Action::Confirm(None));
    mv(&mut m, None, 1);
    assert_eq!(edits(&mut m)[0].1["pan"], json!(-0.45));
    assert_eq!(m.act(Action::Cancel(None)), Outcome::Stay);
}

// Touch: the faders and sliders drag and tap (a fader moves a hundredth or more to send), a double tap puts a
// pan or a send back, M and S tap; a finger on a control keeps the engine's own values for it, and touch
// drops the cursor.
#[test]
fn touch_drags_taps_and_a_held_control_keeps_its_value() {
    let mut s = Screen::sized(1280, 900);
    let mut m = mixer_screen(&mut s, &graph());
    let state = s.ui.global::<MixerState>();
    mv(&mut m, None, 1);
    m.level(1, 0.5);
    assert_eq!(at(&s.ui), None, "a touch drops the cursor");
    assert_eq!(edits(&mut m), [("parts.level", json!({"part": 11, "level": 0.25, "mute": false}))]);
    m.level(1, 0.501);
    assert!(edits(&mut m).is_empty(), "less than a hundredth sends nothing");
    m.pan(0, 0.75);
    assert_eq!(edits(&mut m), [("parts.pan", json!({"part": 10, "pan": 0.5}))]);
    m.pan(0, 0.5);
    assert_eq!(edits(&mut m)[0].1["pan"], json!(0.0), "a double tap: the middle");
    m.send(0, 1, 0.4);
    assert_eq!(edits(&mut m), [("parts.send", json!({"part": 10, "bus": 13, "level": 0.4}))]);
    m.send(0, 0, 0.0);
    assert_eq!(edits(&mut m), [("parts.send", json!({"part": 10, "bus": 12}))]);
    m.tap_mute(3);
    assert_eq!(edits(&mut m), [("parts.level", json!({"part": 15, "level": 1.0, "mute": true}))]);
    m.tap_solo(3);
    assert_eq!(edits(&mut m), [("parts.solo", json!({"part": 15, "solo": true}))]);
    m.level(-1, 0.5);
    assert_eq!(edits(&mut m), [("master.set", json!({"level": 0.5, "pan": -0.5}))]);
    m.pan(-1, 1.0);
    assert_eq!(edits(&mut m), [("master.set", json!({"level": 0.5, "pan": 1.0}))]);
    // A finger on a fader: the graph and the live values wait.
    m.hold(0, 0, true);
    m.level(0, 0.9);
    m.graph(&graph());
    assert_eq!(state.get_strips().row_data(0).unwrap().fader, 0.9);
    m.controls(&json!({"parts": [{"part": 10, "level": 0.2, "mute": false, "pan": -1.0}]}));
    assert_eq!(state.get_strips().row_data(0).unwrap().fader, 0.9, "the fader's own value");
    assert_eq!(state.get_strips().row_data(0).unwrap().pan, 0.0, "the pan followed: no finger on it");
    m.hold(0, 0, false);
    m.controls(&json!({"parts": [{"part": 10, "level": 0.2, "mute": true, "pan": -1.0}], "master": {"level": 2.0, "pan": 0.5}}));
    let first = state.get_strips().row_data(0).unwrap();
    assert_eq!((first.db.as_str(), first.mute, first.dim), ("-14.0 dB", true, true), "followed again, with the mute that came");
    assert_eq!((state.get_master().db.as_str(), state.get_master().pan), ("6.0 dB", 0.75), "and the master's");
}

// How many strips a view holds and how many sends a strip shows is whatever fills the window: a narrow one
// shows a few strips a view (the master always there), a short one a send at a time; the cursor going to a
// strip or a send out of view turns it, the paging buttons and keys turn it with no cursor.
#[test]
fn the_strips_and_sends_a_view_holds_follow_the_window() {
    let mut s = Screen::sized(500, 420);
    let mut m = mixer_screen(&mut s, &graph());
    let ui = &s.ui;
    let state = ui.global::<MixerState>();
    let names = |ui: &Shell| strips(ui).into_iter().map(|r| r.0).collect::<Vec<_>>();
    let view = |ui: &Shell| ui.global::<MixerState>().get_view().to_string();
    assert_eq!((names(ui), view(ui).as_str()), (vec!["Keys".to_string(), "Low".to_string(), "Pad".to_string()], "Strips 1–3 of 6 · Sends 1–1 of 2"));
    assert_eq!(m.bar_tools().iter().map(|t| (t.label.as_str(), t.enabled)).collect::<Vec<_>>(), [("‹ Strips", false), ("Strips ›", true), ("‹ Sends", false), ("Sends ›", true)], "the pagers are the top bar's");
    assert_eq!(state.get_strips().row_data(0).unwrap().sends.row_count(), 1, "a send shown in a window this short");
    assert_eq!(state.get_send_names().iter().map(|n| n.to_string()).collect::<Vec<_>>(), ["Reverb"]);
    m.act(Action::Page(1));
    assert_eq!(names(ui), ["Empty", "Reverb", "Delay"]);
    m.act(Action::Page(1));
    assert_eq!(names(ui), ["Empty", "Reverb", "Delay"], "held at the last view");
    m.act(Action::Page(-1));
    assert_eq!(names(ui)[0], "Keys");
    // The cursor on the second send turns the sends' window; the first strip is kept.
    mv(&mut m, None, 1);
    mv(&mut m, Some(1), 10);
    assert_eq!(at(ui), Some(("Keys".into(), 4)), "the one send shown");
    mv(&mut m, Some(1), 1);
    assert_eq!(at(ui), Some(("Keys".into(), 4)), "the second send comes into view as the window turns: the one place of the window");
    assert_eq!(state.get_send_names().iter().map(|n| n.to_string()).collect::<Vec<_>>(), ["Delay"]);
}

// Sends come into view as the cursor reaches them, and the buttons turn the window.
#[test]
fn the_sends_window_follows_the_cursor_and_the_buttons() {
    let mut s = Screen::sized(1280, 420);
    let mut m = mixer_screen(&mut s, &graph());
    let ui = &s.ui;
    let names = |ui: &Shell| ui.global::<MixerState>().get_send_names().iter().map(|n| n.to_string()).collect::<Vec<_>>();
    assert_eq!(names(ui), ["Reverb"]);
    mv(&mut m, None, 1);
    mv(&mut m, Some(1), 5);
    assert_eq!(at(ui), Some(("Keys".into(), 4)), "the cursor reached the second send: the first place of the window");
    assert_eq!(names(ui), ["Delay"], "the window turned to it");
    m.act(Action::Confirm(None));
    mv(&mut m, None, 1);
    assert_eq!(edits(&mut m)[0].1["bus"], json!(13), "the control under the cursor is the send shown");
    m.act(Action::Cancel(None));
    m.turn_sends(-1);
    assert_eq!(names(ui), ["Reverb"]);
    assert_eq!(at(ui), Some(("Keys".into(), 4)), "the cursor is on the send that is shown, now the first");
    m.send(0, 0, 0.9);
    assert_eq!(edits(&mut m)[0].1, json!({"part": 10, "bus": 12, "level": 0.9}), "a tap on the send shown acts on its bus");
}

// No parts: the master alone, as the JUCE page shows it.
#[test]
fn a_performance_with_no_parts_shows_the_master() {
    let mut s = Screen::new();
    let mut g = graph();
    g["parts"] = json!([]);
    let mut m = mixer_screen(&mut s, &g);
    assert_eq!(strips(&s.ui).len(), 0);
    mv(&mut m, None, 1);
    assert_eq!(at(&s.ui), Some(("Master".into(), 0)));
    assert_eq!(s.ui.global::<MixerState>().get_view().as_str(), "Master");
    assert_eq!(m.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(m.act(Action::Cancel(None)), Outcome::Back);
}

// The meters: a bar per channel of the part's sound along -60..+6 dB, one dark bar before any is read; the
// engine's refusal is said and the strips asked again.
#[test]
fn meters_are_a_bar_per_channel_and_a_refusal_is_said() {
    let mut s = Screen::sized(1280, 900);
    let mut m = mixer_screen(&mut s, &graph());
    let peaks = |ui: &Shell, i: usize| ui.global::<MixerState>().get_strips().row_data(i).unwrap().peaks.iter().collect::<Vec<_>>();
    assert_eq!(peaks(&s.ui, 0), [0.0]);
    m.meters(&json!({"meters": [{"part": 10, "peak": 1.0, "channels": [1.0, 0.0316]}, {"part": 11, "channels": [0.1]}, {"part": 12, "channels": [0.001, 0.5, 1.0]}]}));
    assert_eq!(peaks(&s.ui, 0), [0.91, 0.45], "0 dB and -30 dB");
    assert_eq!(peaks(&s.ui, 1), [0.61], "-20 dB");
    assert_eq!(peaks(&s.ui, 4).len(), 3, "a surround bus has three bars");
    assert_eq!(peaks(&s.ui, 2), [0.0], "no meter for it now");
    m.meters(&json!({"meters": []}));
    assert_eq!(peaks(&s.ui, 0), [0.0]);
    assert!(m.reply(crate::mixer::Request::Edit, Err(crate::client::Error { code: -32000, message: "no".into() })));
    assert_eq!(s.ui.global::<MixerState>().get_message().as_str(), "The engine refused: no");
    m.status(&json!({"failures": [{"node": 1, "name": "Rhodes", "reason": "gone"}]}));
    assert_eq!(s.ui.global::<MixerState>().get_message().as_str(), "The engine refused: no", "the refusal first");
    m.act(Action::Move { pane: None, by: 1 });
    assert_eq!(s.ui.global::<MixerState>().get_message().as_str(), "Rhodes is silent: gone");
}

// On the shell through the fake engine: the Parts list's Mixer button opens the page, which asks the meters on
// a short timer, follows controls.changed and the bar's Play; Back and Cancel return to the list.
#[test]
fn the_mixer_opens_from_the_parts_list_and_goes_back_to_it() {
    let mut f = play_on_the_shell("mixer-shell", json!({"id": "p1", "name": "Grand", "unsaved": false}), graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    let g = graph();
    let labels = |ui: &Shell| ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect::<Vec<_>>();
    ui.global::<Bar>().invoke_action(5);
    assert_eq!(ui.get_screen(), 9);
    let tools = ui.global::<PartsState>().get_tools();
    assert_eq!((tools.row_data(0).unwrap().label.as_str(), tools.row_data(0).unwrap().enabled), ("Mixer", true), "the button is on");
    ui.global::<PartsState>().invoke_tool(0);
    assert_eq!((ui.get_screen(), title(&ui)), (10, "Parts › Mixer".to_string()));
    assert_eq!(labels(&ui), ["Undo Volume of Keys", "Redo", "Save", "Play", "Record", "Capture"]);
    request_in(&mut f, "graph.get", &none, &g);
    // The meters are asked every 50 ms while shown.
    f.screen.frame();
    std::thread::sleep(std::time::Duration::from_millis(120));
    f.screen.frame();
    // (Answers to polls the Play and Parts screens left behind come first; the Mixer's is the one that takes.)
    for _ in 0..4 {
        let r = request_in(&mut f, "parts.meters", &none, &g);
        f.reply(&r, json!({"result": {"meters": [{"part": 10, "peak": 0.5, "channels": [0.5, 0.25]}]}}));
        if ui.global::<MixerState>().get_strips().row_data(0).unwrap().peaks.row_count() == 2 {
            break;
        }
    }
    assert_eq!(ui.global::<MixerState>().get_strips().row_data(0).unwrap().peaks.row_count(), 2);
    // A pan moved live by another client.
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controls.changed", "params": {"parts": [{"part": 10, "level": 0.9, "mute": true, "pan": -1.0}], "master": {"level": 1.0, "pan": 0.0}}})));
    f.pump();
    let first = ui.global::<MixerState>().get_strips().row_data(0).unwrap();
    assert_eq!((first.pan, first.mute), (0.0, true));
    // A press on the master's fader sends `master.set` and a commit.
    shell::act(Action::End(1));
    shell::act(Action::Confirm(None));
    shell::act(Action::Move { pane: None, by: 1 });
    let r = request_in(&mut f, "master.set", &none, &g);
    assert_eq!(r["params"]["pan"], json!(0.0));
    let r = request_in(&mut f, "graph.commit", &none, &g);
    f.reply(&r, json!({"result": {"accepted": true}}));
    // Cancel is Back in one step, from a fader being changed too: to the list.
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 9);
    // Back, and the bar's Play.
    ui.global::<PartsState>().invoke_tool(0);
    assert_eq!(ui.get_screen(), 10);
    ui.global::<Bar>().invoke_back_tapped();
    assert_eq!(ui.get_screen(), 9);
    ui.global::<PartsState>().invoke_tool(0);
    ui.global::<Bar>().invoke_action(3);
    assert_eq!(ui.get_screen(), 3, "Play");
    f.screen.frame();
}

// The Mixer in every built-in theme and the two odd ones, at the keyboard's 1280x720, a small device's 800x480,
// the FANTOM-07's scale and an 800-wide window: pictures in target/screens and target/theme-shots for a look
// by eye. A cursor and a fader being changed are drawn in each.
#[test]
fn the_mixer_draws_in_every_theme_at_several_sizes() {
    let peaks = json!({"meters": [{"part": 10, "channels": [0.7, 0.4]}, {"part": 11, "channels": [0.1]}, {"part": 14, "channels": [0.95, 0.9]}, {"part": 12, "channels": [0.3, 0.3]}]});
    for (w, h) in [(1280, 720), (800, 480), (800, 700)] {
        let mut s = Screen::sized(w, h);
        let mut m = mixer_screen(&mut s, &graph());
        s.ui.global::<Bar>().set_performance("Grand Piano".into());
        set_title(&s.ui, &["Parts", "Mixer"]);
        m.meters(&peaks);
        mv(&mut m, Some(1), 1);
        mv(&mut m, Some(1), 1);
        m.act(Action::Confirm(None)); // the first strip's mute? first move put it on the fader, down once: M
        mv(&mut m, Some(1), 2); // the pan
        m.act(Action::Confirm(None)); // being changed
        for t in &all_themes() {
            theme::apply(&s.ui, t);
            assert!(s.frame().is_some(), "{}: drawn", t.id);
            m.shown(); // the strips and sends a view holds depend on the paddings and the type
            s.frame();
            shot(&s, &format!("mixer-{}-{w}x{h}", t.id));
            if t.id == "oscairn-dark" {
                png(&s, &format!("mixer-{w}x{h}"));
            }
        }
    }
    let mut s = Screen::new();
    let mut m = mixer_screen(&mut s, &graph());
    m.meters(&peaks);
    mv(&mut m, None, 1);
    at_fantom_scale(&mut s, "mixer", || m.shown());
}
