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
            "subscribe" | "unsubscribe" | "controllers.view" | "record.get" | "memory.state" | "transport.get" => json!({}),
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
    assert_eq!(labels(&f), [("Undo Volume of Keys".into(), true, false), ("Redo".into(), false, false), ("Save".into(), true, false), ("Scene".into(), true, false), ("Learn".into(), true, false), ("Edit".into(), true, false), ("Record".into(), true, false), ("Capture".into(), false, false)]);
    f.screen.frame();
    f.screen.save("bar-actions");
    png(&f.screen, "bar-actions");
    // Touch: Undo, whose answer says what undo and redo are now.
    ui.global::<Bar>().invoke_action(0);
    let r = request(&mut f, "edit.undo", &none);
    f.reply(&r, json!({"result": {"undo": "", "redo": "Volume of Keys"}}));
    assert_eq!(labels(&f), [("Undo".into(), false, false), ("Redo Volume of Keys".into(), true, false), ("Save".into(), true, false), ("Scene".into(), true, false), ("Learn".into(), true, false), ("Edit".into(), true, false), ("Record".into(), true, false), ("Capture".into(), false, false)]);
    // A disabled one does nothing.
    ui.global::<Bar>().invoke_action(0);
    // The third pane enters the bar at Back; the cursor then goes to the name (Save As New) and along the actions.
    shell::act(Action::Move { pane: Some(2), by: 1 });
    assert!(ui.global::<Bar>().get_back_cursor() && !ui.global::<Bar>().get_pill_cursor());
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(ui.global::<Bar>().get_pill_cursor() && !ui.global::<Bar>().get_back_cursor());
    shell::act(Action::Move { pane: None, by: 1 });
    assert_eq!(labels(&f).iter().map(|l| l.2).collect::<Vec<_>>(), [true, false, false, false, false, false, false, false]);
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
    // Past the last stop the cursor goes back to the screen.
    shell::act(Action::Move { pane: None, by: 1 }); // Scene
    shell::act(Action::Move { pane: None, by: 1 }); // Learn
    shell::act(Action::Move { pane: None, by: 1 }); // Edit
    shell::act(Action::Move { pane: None, by: 1 }); // Record
    shell::act(Action::Move { pane: None, by: 1 }); // Capture
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(labels(&f).iter().all(|l| !l.2));
    // The dial moving up out of the first scene reaches the last stop; Tab reaches the first and gives it back.
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Move { pane: None, by: -1 });
    assert_eq!(labels(&f).iter().map(|l| l.2).collect::<Vec<_>>(), [false, false, false, false, false, false, false, true]);
    assert!(shell::key("\t") && labels(&f).iter().all(|l| !l.2));
    assert!(shell::key("\t") && ui.global::<Bar>().get_back_cursor(), "Tab enters at the first stop, Back");
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(ui.global::<Bar>().get_pill_cursor());
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
    // Cancel is Back in one step, with the cursor on the bar as anywhere: Play goes back to Home.
    shell::act(Action::Move { pane: None, by: -1 }); // from the name to Back
    assert!(ui.global::<Bar>().get_back_cursor());
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 0, "Cancel with the cursor on the bar left Play");
    assert!(!ui.global::<Bar>().get_back_cursor() && !ui.global::<crate::Theme>().get_bar_cursor());
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
        let action = |label: &str, enabled: bool, cursor: bool| BarAction { label: label.into(), enabled, cursor, icon: 0, on: false };
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
        bar.set_path(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::from("Grand Piano with a rather long name  ·  Chorus")])));
        bar.set_title_field(true);
        bar.set_back(true);
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

// A narrow bar gives up, in order: the actions' words (undo and redo to icons, then all the actions into one "More")
// while the name keeps its comfortable room, the name's room down to its least (its text shrinks to 70%, then is cut),
// the meters' names, then their bars' length. The recording mark and the tempo stay, and nothing runs off the bar:
// at the keyboard's 826 across, 800 and 640, with a take recording (slow, or not), the longest actions and a long name.
#[test]
fn a_narrow_bar_folds_its_actions_before_it_cuts_anything() {
    use crate::BarAction;
    for (w, h, scale) in [(1280, 720, 1.55), (800, 480, 1.0), (640, 480, 1.0), (1280, 720, 1.0)] {
        for behind in [false, true] {
            let mut s = Screen::sized(w, h);
            s.set_scale(scale);
            let logical = w as f32 / scale;
            let ui = s.ui.clone_strong();
            let bar = ui.global::<Bar>();
            let action = |label: &str, enabled: bool, icon: i32| BarAction { label: label.into(), enabled, cursor: false, icon, on: false };
            let list = [action("Undo Volume of Keys", true, 1), action("Redo", false, 2), action("Save", true, 0), action("Scene", true, 0), action("Learn", true, 0), action("Edit", true, 0)];
            bar.set_actions(slint::ModelRc::new(slint::VecModel::from(list.to_vec())));
            bar.set_performance("Grand Piano with a rather long name".into());
            bar.set_scene("Chorus".into());
            bar.set_tempo("120.0 BPM".into());
            bar.set_cpu(42);
            bar.set_cpu_known(true);
            bar.set_ram(60);
            bar.set_ram_known(true);
            bar.set_recording(true);
            bar.set_recording_time("12:07".into());
            bar.set_recording_behind(behind);
            bar.set_path(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::from("Grand Piano with a rather long name  ·  Chorus")])));
        bar.set_title_field(true);
        bar.set_back(true);
            bar.set_home_button(true);
            s.ui.set_screen(3);
            let mut modes = vec![];
            let mut unfit = vec![];
            for t in all_themes() {
                theme::apply(&s.ui, &t);
                s.frame();
                s.frame();
                let what = format!("{w}x{h}@{scale} behind {behind} in {}", t.id);
                let small = s.ui.global::<crate::Theme>().get_small();
                let pad = s.ui.global::<crate::Theme>().get_pad();
                let (end, pill_end) = (s.ui.get_resources_x() + s.ui.get_resources_width(), s.ui.get_pill_x() + s.ui.get_pill_width());
                let mark = s.ui.get_rec_width();
                let fits = end <= logical - pad + 0.5 && s.ui.get_pill_width() >= (small * 9.).round() - 0.5 && mark > 0. && s.ui.get_tempo_x() - mark >= pill_end - 0.5;
                eprintln!("BAR {what}: modes {:?}, name {:.0}, meters end {end:.0} of {logical:.0}, fits {fits}", (s.ui.get_actions_mode(), s.ui.get_meters_mode()), s.ui.get_pill_width());
                if !fits {
                    unfit.push(what.clone());
                }
                modes.push((s.ui.get_actions_mode(), s.ui.get_meters_mode()));
                shot(&s, &format!("bar-narrow-{}-{w}x{h}@{scale}{}", t.id, if behind { "-slow" } else { "" }));
            }
            // In the dark theme: what the width gives.
            let (actions, meters) = modes[0];
            match logical {
                // Back takes its place on the bar: with a slow disk's words up as well, Undo and Redo are icons at 1280.
                x if x >= 1200. => assert!(meters == 0 && actions <= if behind { 1 } else { 0 }, "{w}x{h}@{scale}: room for everything, or icons for Undo and Redo with a slow disk's words up: {actions} {meters}"),
                x if x < 700. => assert_eq!(actions, 2, "{w}x{h}@{scale}: folded into More"),
                _ => assert!(actions >= 1, "{w}x{h}@{scale}: icons or More"),
            }
            assert!(meters == 0 || actions == 2, "the meters give up nothing while the actions can still fold");
            // Nothing runs off the bar or leaves the name under its least, in any theme (the roomy odd one included: type 1.39,
            // spacing 1.6), but for that theme at 640 across, which the fixed parts alone (Home, Back, More, the mark, the
            // tempo, the percents) are wider than.
            let unfit: Vec<_> = unfit.into_iter().filter(|w| !(w.ends_with("in odd") && logical < 700.)).collect();
            assert!(unfit.is_empty(), "{unfit:?}");
        }
    }
}

// On the shell: at 640 across the actions are one "More" stop; it opens a menu of them (a disabled one cannot be chosen)
// by the dial and by touch, and a choice does what the action does.
#[test]
fn more_opens_a_menu_of_the_actions_by_the_dial_and_by_touch() {
    let mut f = play_on_the_shell("bar-more", json!({"id": "p1", "name": "Grand", "unsaved": false}), play_graph());
    f.screen.set_scale(2.0); // 640 across
    f.screen.frame();
    f.screen.frame();
    let ui = f.screen.ui.clone_strong();
    assert!(ui.global::<Bar>().get_more(), "folded at 640");
    f.screen.frame();
    // The stops are Back, the name and More: the third pane enters at Back, the dial goes on to More.
    shell::act(Action::Move { pane: Some(2), by: 1 });
    assert!(ui.global::<Bar>().get_back_cursor());
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(ui.global::<Bar>().get_pill_cursor());
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(ui.global::<Bar>().get_more_cursor(), "the cursor is on More");
    shell::act(Action::Confirm(None));
    assert!(ui.global::<crate::MenuState>().get_open(), "More opened its menu");
    f.screen.frame();
    f.screen.frame();
    let rows = || -> Vec<(String, bool, bool)> { ui.global::<crate::MenuState>().get_rows().iter().map(|r| (r.label.to_string(), r.enabled, r.cursor)).collect() };
    assert_eq!((rows()[0].0.as_str(), rows()[1].0.as_str(), rows()[1].1), ("Undo Volume of Keys", "Redo", false), "{:?}", rows());
    // Redo is off: touching it does nothing. The dial goes down to Edit (the last) and a press does it: the Parts list.
    ui.global::<crate::MenuState>().invoke_tapped(1);
    assert_eq!(ui.get_screen(), 3, "a disabled action does nothing");
    for _ in 0..10 {
        if rows().iter().any(|r| r.2 && r.0 == "Edit") {
            break;
        }
        shell::act(Action::Move { pane: None, by: 1 });
    }
    assert!(rows().iter().any(|r| r.2 && r.0 == "Edit"), "the dial reached Edit: {:?}", rows());
    shell::act(Action::Confirm(None));
    assert_eq!(ui.get_screen(), 9);
    assert!(!ui.global::<crate::MenuState>().get_open());
    // And by touch on More itself, closed by Cancel.
    f.screen.frame();
    ui.global::<Bar>().invoke_more_tapped();
    assert!(ui.global::<crate::MenuState>().get_open());
    shell::act(Action::Cancel(None));
    assert!(!ui.global::<crate::MenuState>().get_open());
}

// Record and Capture, the JUCE record buttons, are the last two actions of every screen's bar (Home's included): Record starts
// a take (and, while one records, ends it by its number), Capture saves the last seconds the buffer keeps (off while it
// keeps none) as a take named for the Performance.
#[test]
fn record_and_capture_are_on_the_bar() {
    let mut f = play_on_the_shell("bar-record", json!({"id": "p1", "name": "Grand", "unsaved": false}), play_graph());
    let ui = f.screen.ui.clone_strong();
    let index = |f: &Fixture, label: &str| labels(f).iter().position(|l| l.0.starts_with(label)).unwrap_or_else(|| panic!("{label} in {:?}", labels(f)));
    let none = json!({"undo": "", "redo": ""});
    let last = labels(&f).len();
    assert_eq!((index(&f, "Record"), index(&f, "Capture")), (last - 2, last - 1));
    assert!(!labels(&f)[last - 1].1, "Capture is off while the buffer keeps nothing");
    ui.global::<Bar>().invoke_action(index(&f, "Capture") as i32); // does nothing while off
    ui.global::<Bar>().invoke_action(index(&f, "Record") as i32);
    let r = request(&mut f, "record.start", &none);
    assert_eq!(r["params"], json!({}));
    f.reply(&r, json!({"result": {"take": 4}}));
    let r = request(&mut f, "record.get", &none);
    f.reply(&r, json!({"result": {"recording": 4, "retro": {"seconds": 45}, "takes": [{"take": 4, "state": "recording", "frames": 0, "sampleRate": 48000}]}}));
    assert!(ui.global::<Bar>().get_recording());
    assert_eq!(labels(&f)[last - 2].0, "Stop recording");
    assert!(labels(&f)[last - 1].1, "Capture is on now");
    ui.global::<Bar>().invoke_action(index(&f, "Stop recording") as i32);
    let r = request(&mut f, "record.stop", &none);
    assert_eq!(r["params"], json!({"take": 4}));
    f.reply(&r, json!({"result": {"accepted": true}}));
    let r = request(&mut f, "record.get", &none);
    f.reply(&r, json!({"result": {"recording": 0, "retro": {"seconds": 45}, "takes": [{"take": 4, "state": "done"}]}}));
    assert!(!ui.global::<Bar>().get_recording());
    // Capture: the last 30 seconds at most, named for the Performance.
    ui.global::<Bar>().invoke_action(index(&f, "Capture") as i32);
    let r = request(&mut f, "record.start", &none);
    assert_eq!(r["params"], json!({"name": "Grand capture", "from": {"secondsAgo": 30.0}}));
    f.reply(&r, json!({"result": {"take": 5}}));
    assert_eq!(ui.global::<Bar>().get_notice().as_str(), "Captured to Recordings");
    let r = request(&mut f, "record.get", &none);
    f.reply(&r, json!({"result": {"recording": 0, "retro": {"seconds": 45}, "takes": []}}));
    // A refusal is said in the name's place.
    ui.global::<Bar>().invoke_action(index(&f, "Record") as i32);
    let r = request(&mut f, "record.start", &none);
    f.reply(&r, json!({"error": {"code": -32007, "message": "A take is recording"}}));
    assert_eq!(ui.global::<Bar>().get_notice().as_str(), "Not recorded: A take is recording");
    f.screen.frame();
    f.screen.frame();
    shot(&f.screen, "bar-record");
    png(&f.screen, "bar-record");
}

// A short title leaves its room to the actions: the bar folds them into More only when they do not fit with the title as
// it is written (up to the comfortable room), measured, not whenever the comfortable room would not be left.
#[test]
fn a_short_title_leaves_its_room_to_the_actions() {
    use crate::BarAction;
    let mode_with = |path: &str| {
        let mut s = Screen::sized(1280, 720);
        s.set_scale(1.55); // the keyboard's 826 across
        let ui = s.ui.clone_strong();
        let bar = ui.global::<Bar>();
        let action = |label: &str, icon: i32| BarAction { label: label.into(), enabled: true, cursor: false, icon, on: false };
        let list = [action("Strips", 0), action("Sends", 0), action("Undo", 1), action("Redo", 2), action("Save", 0)];
        bar.set_actions(slint::ModelRc::new(slint::VecModel::from(list.to_vec())));
        bar.set_tempo("120.0 BPM".into());
        bar.set_cpu(5);
        bar.set_cpu_known(true);
        bar.set_ram(20);
        bar.set_ram_known(true);
        bar.set_path(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::from(path)])));
        bar.set_title_field(false);
        bar.set_back(true);
        bar.set_home_button(true);
        s.ui.set_screen(10);
        s.frame();
        s.frame();
        shot(&s, &format!("bar-title-{}", path.len()));
        s.ui.get_actions_mode()
    };
    let short = mode_with("Mixer");
    let long = mode_with("Lead  ›  Insert effects  ›  Basic Synth  ›  Modulation Matrix");
    assert!(short < 2, "the actions fit beside a short title: {short}");
    assert!(short <= long, "a short title never folds more than a long one: {short} {long}");
}
