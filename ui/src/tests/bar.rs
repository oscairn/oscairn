// SPDX-License-Identifier: AGPL-3.0-or-later
//! The top bar's actions on the Performance app's screens (J1 undo and redo named by the edit, A15 save)
//! on the shell through the fake engine: by touch, by the cursor (the third pane, Tab, the dial moving
//! up out of the list), and what the engine answers.

use super::*;

/// Play on the shell, the connection's requests answered; `info` is the Performance's.
pub fn play_on_the_shell(name: &str, info: serde_json::Value, graph: serde_json::Value) -> Fixture {
    let mut f = Fixture::new(name);
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(0); // Performance: Play
    for _ in 0..14 {
        let request = f.engine.request();
        let result = match request["method"].as_str().unwrap() {
            "performance.info" => info.clone(),
            "graph.get" => graph.clone(),
            "library.query" => json!({"items": [], "total": 0}),
            "edit.history" => json!({"undo": "Volume of Keys", "redo": ""}),
            _ => json!({}),
        };
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        f.pump();
    }
    f
}

/// The next request the test is waiting for; the shell's own polls are answered as an engine would.
pub fn request_in(f: &mut Fixture, want: &str, history: &serde_json::Value, graph: &serde_json::Value) -> serde_json::Value {
    loop {
        let r = f.engine.request();
        let result = match r["method"].as_str().unwrap() {
            m if m == want => return r,
            "parts.meters" => json!({"meters": []}),
            "edit.history" => history.clone(),
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => graph.clone(),
            "subscribe" | "unsubscribe" | "controllers.view" | "record.takes" | "memory.state" | "transport.get" => json!({}),
            _ => return r,
        };
        f.reply(&r, json!({"result": result}));
    }
}

pub fn request(f: &mut Fixture, want: &str, history: &serde_json::Value) -> serde_json::Value {
    request_in(f, want, history, &play_graph())
}

fn labels(f: &Fixture) -> Vec<(String, bool, bool)> {
    f.screen.ui.global::<Bar>().get_actions().iter().map(|a| (a.label.to_string(), a.enabled, a.cursor)).collect()
}

// Undo and Redo carry the edit's name and are off while there is nothing to do; Save saves a Performance
// that is in the library at once and asks the name of one that is not; the Performance's name on the bar is
// Save As New. The cursor reaches them without touch and leaves past the last.
#[test]
fn undo_redo_and_save_on_the_top_bar() {
    let mut f = play_on_the_shell("bar", json!({"id": "p1", "name": "Grand", "unsaved": true}), play_graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    assert_eq!(labels(&f), [("Undo Volume of Keys".into(), true, false), ("Redo".into(), false, false), ("Save".into(), true, false), ("Scene".into(), true, false), ("Learn".into(), true, false)]);
    f.screen.frame();
    f.screen.save("bar-actions");
    png(&f.screen, "bar-actions");
    // Touch: Undo, whose answer says what undo and redo are now.
    ui.global::<Bar>().invoke_action(0);
    let r = request(&mut f, "edit.undo", &none);
    f.reply(&r, json!({"result": {"undo": "", "redo": "Volume of Keys"}}));
    assert_eq!(labels(&f), [("Undo".into(), false, false), ("Redo Volume of Keys".into(), true, false), ("Save".into(), true, false), ("Scene".into(), true, false), ("Learn".into(), true, false)]);
    // A disabled one does nothing.
    ui.global::<Bar>().invoke_action(0);
    // The third pane enters the bar at the name (Save As New); the cursor then goes along the actions.
    shell::act(Action::Move { pane: Some(2), by: 1 });
    assert!(ui.global::<Bar>().get_pill_cursor());
    shell::act(Action::Move { pane: None, by: 1 });
    assert_eq!(labels(&f).iter().map(|l| l.2).collect::<Vec<_>>(), [true, false, false, false, false]);
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None)); // Redo
    let r = request(&mut f, "edit.redo", &none);
    assert_eq!(r["params"], json!({}), "nothing else is sent with it");
    f.reply(&r, json!({"result": {"undo": "Volume of Keys", "redo": ""}}));
    assert!(!ui.global::<Bar>().get_pill_cursor());
    // Save: the Performance is in the library (id p1): saved over at once.
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let r = request(&mut f, "library.save", &none);
    assert_eq!(r["params"], json!({}));
    f.reply(&r, json!({"result": {"id": "p1"}}));
    assert_eq!(ui.global::<Bar>().get_notice().as_str(), "Performance saved.");
    // Past the last stop the cursor goes back to the screen; Cancel gives it back at once.
    shell::act(Action::Move { pane: None, by: 1 }); // Scene
    shell::act(Action::Move { pane: None, by: 1 }); // Learn
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(labels(&f).iter().all(|l| !l.2));
    shell::act(Action::Move { pane: Some(2), by: 0 });
    shell::act(Action::Cancel(None));
    assert!(labels(&f).iter().all(|l| !l.2) && !ui.global::<Bar>().get_pill_cursor());
    assert_eq!(ui.get_screen(), 3, "Cancel gave the cursor back, it did not leave Play");
    // The dial moving up out of the first scene reaches the last stop; Tab reaches the first and gives it back.
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Move { pane: None, by: -1 });
    assert_eq!(labels(&f).iter().map(|l| l.2).collect::<Vec<_>>(), [false, false, false, false, true]);
    assert!(shell::key("\t") && labels(&f).iter().all(|l| !l.2));
    assert!(shell::key("\t") && ui.global::<Bar>().get_pill_cursor());
    // The name: Save As New asks for the name on the keyboard, which starts as the Performance's.
    shell::act(Action::Confirm(None));
    assert!(ui.global::<crate::KeyboardState>().get_open());
    shell::act(Action::End(1));
    shell::act(Action::Confirm(None)); // Done
    let r = request(&mut f, "library.save", &none);
    assert_eq!(r["params"], json!({"new": true, "name": "Grand"}));
    f.reply(&r, json!({"error": {"code": -32009, "message": "The engine is busy"}}));
    assert_eq!(ui.global::<Bar>().get_notice().as_str(), "Not saved: The engine is busy");
    f.screen.frame();
    f.screen.save("bar-notice");
    png(&f.screen, "bar-notice");
}

// A Performance not in the library has no id: Save asks the name first, a blank name keeps its own, Cancel saves nothing.
#[test]
fn save_asks_the_name_of_a_performance_not_in_the_library() {
    let mut f = play_on_the_shell("bar-new", json!({"id": "", "name": "Untitled", "unsaved": true}), play_graph());
    let none = json!({"undo": "", "redo": ""});
    let ui = f.screen.ui.clone_strong();
    ui.global::<Bar>().invoke_action(2); // Save
    assert!(ui.global::<crate::KeyboardState>().get_open());
    shell::act(Action::Cancel(None)); // closes it, nothing sent
    assert!(!ui.global::<crate::KeyboardState>().get_open());
    ui.global::<Bar>().invoke_action(2);
    shell::act(Action::End(1));
    shell::act(Action::Confirm(None));
    let r = request(&mut f, "library.save", &none);
    assert_eq!(r["params"], json!({"new": false, "name": "Untitled"}));
    f.reply(&r, json!({"result": {"id": "p9"}}));
}

// The bar with every part on it at once (actions with a cursor, a recording take, the tempo and the load), in
// every theme and at a narrow device's width: nothing overlaps, the name's pill gives way first.
#[test]
fn the_bar_draws_in_every_theme_and_at_a_narrow_width() {
    use crate::BarAction;
    for (w, h) in [(1280, 720), (800, 480)] {
        let mut s = Screen::sized(w, h);
        let bar = s.ui.global::<Bar>();
        let action = |label: &str, enabled: bool, cursor: bool| BarAction { label: label.into(), enabled, cursor };
        bar.set_actions(slint::ModelRc::new(slint::VecModel::from(vec![action("Undo Volume of Keys", true, false), action("Redo", false, false), action("Save", true, true)])));
        bar.set_performance("Grand Piano with a rather long name".into());
        bar.set_scene("Chorus".into());
        bar.set_tempo("120.0 BPM".into());
        bar.set_cpu(42);
        bar.set_cpu_known(true);
        bar.set_ram(60);
        bar.set_ram_known(true);
        bar.set_recording(true);
        bar.set_recording_time("12:07".into());
        bar.set_recording_behind(true);
        bar.set_pill_cursor(true);
        bar.set_where("Play".into());
        bar.set_home_button(true);
        s.ui.set_screen(3);
        for t in all_themes() {
            theme::apply(&s.ui, &t);
            assert!(s.frame().is_some(), "{}: drawn", t.id);
            shot(&s, &format!("bar-{}-{w}", t.id));
        }
    }
}

// The bar's actions are the Performance app's: none on Home or the Setlist.
#[test]
fn only_the_performance_apps_screens_have_actions() {
    let f = Fixture::new("bar-home");
    assert!(labels(&f).is_empty());
}
