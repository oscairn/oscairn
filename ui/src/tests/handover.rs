// SPDX-License-Identifier: AGPL-3.0-or-later
//! Nothing inside the Performance app hands the display to the JUCE program (ui/PERFORMANCE-APP.md, "The switch"): from
//! Home's Performance tile the walk presses every stop the cursor reaches on every screen (the dial, the left and right
//! knobs, the bar's actions), goes into what opens and does the same there, on the shell through the fake engine, started
//! as the device starts it (with a handover file, so a handover would end the program). After every press the screen is
//! none of the "Opening ..." screen and the shell has asked for no app.

use super::*;
use std::collections::HashSet;
use std::time::Duration;

/// What a quiet engine answers; every request the shell has out is answered, until it has none for a moment.
fn settle(f: &mut Fixture) {
    let mut graph = super::chain_splits::split_graph("bands", 3);
    graph["scenes"] = json!([{"id": 1, "name": "Verse"}]);
    graph["scene"] = json!(1);
    while let Some(r) = f.engine.try_request(Duration::from_millis(12)) {
        let result = match r["method"].as_str().unwrap_or_default() {
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => graph.clone(),
            "library.query" => json!({"items": [{"id": "s1", "name": "Warm Rhodes", "tags": ["keys"]}], "total": 1, "tags": [{"tag": "keys", "count": 1}], "tagColors": [], "hiddenTags": []}),
            "library.libraries" => json!({"libraries": []}),
            "params.list" => super::pages::param_list(),
            "params.get" => json!({"values": [{"node": 2, "param": 1, "value": 0.25}]}),
            "params.text" => json!({"text": "t"}),
            "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
            "session.status" => json!({"midiInputs": [], "midiOutputs": [], "failures": []}),
            "edit.history" | "edit.undo" | "edit.redo" => json!({"undo": "Level", "redo": ""}),
            "record.get" => json!({"recording": 0, "retro": {"seconds": 30}, "takes": []}),
            "parts.meters" | "wires.meters" => json!({"meters": []}),
            "window.info" => json!({"available": false}),
            "controls.get" | "assignments.get" => json!({"assignments": []}),
            "presets.list" => json!({"presets": []}),
            _ => json!({}),
        };
        f.reply(&r, json!({"result": result}));
    }
}

/// Where the screen is: its number and the bar's title, which says what is open in it.
fn here(ui: &Shell) -> String {
    format!("{}:{}", ui.get_screen(), title(ui))
}

/// The shell did not hand over, and shows no screen that says it is not in this UI.
fn no_handover(ui: &Shell, what: &str) {
    assert_ne!(ui.get_screen(), 2, "{what}: the Opening screen, which the display goes to the JUCE app from");
    assert_eq!(shell::handover(), None, "{what}: the shell asked for a JUCE app");
}

/// Closes what a press opened over the screen (a keyboard, a menu), as Cancel does.
fn close_overlays(f: &mut Fixture) {
    let ui = f.screen.ui.clone_strong();
    for _ in 0..3 {
        if ui.global::<crate::KeyboardState>().get_open() || ui.global::<crate::MenuState>().get_open() || ui.global::<crate::PianoState>().get_open() {
            shell::act(Action::Cancel(None));
            settle(f);
        }
    }
}

/// One step from the app's first screen to another: the stop `k` of the screen pressed, or the bar's action `i`.
#[derive(Clone, Copy, Debug)]
enum Step {
    Stop(i32),
    Bar(usize),
    /// A row of the bar's More menu.
    Menu(usize),
    /// A page of the app's rail (the Library's kinds, the Settings' pages).
    Page(usize),
}

/// The stop `k` of the screen by the dial, pressed.
fn press_stop(f: &mut Fixture, k: i32) {
    shell::act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor
    shell::act(Action::End(-1)); // the first stop
    if k > 0 {
        shell::act(Action::Move { pane: None, by: k });
    }
    settle(f);
    shell::act(Action::Confirm(None));
    settle(f);
}

/// To the place `path` names from wherever the walk is: back to Home, the app's tile (`tile`), then each step done.
fn enter(f: &mut Fixture, tile: i32, path: &[Step]) {
    let ui = f.screen.ui.clone_strong();
    for _ in 0..8 {
        close_overlays(f);
        if ui.get_screen() == 0 {
            break;
        }
        shell::act(Action::Cancel(None));
        settle(f);
    }
    ui.global::<HomeState>().invoke_tapped(tile);
    settle(f);
    for step in path {
        match *step {
            Step::Stop(k) => press_stop(f, k),
            Step::Bar(i) => {
                ui.global::<Bar>().invoke_action(i as i32);
                settle(f);
            }
            Step::Page(i) => {
                if tile == 1 {
                    ui.global::<crate::LibraryState>().invoke_show_page(i as i32);
                } else {
                    ui.global::<crate::SettingsState>().invoke_show_page(i as i32);
                }
                settle(f);
            }
            Step::Menu(i) => {
                ui.global::<Bar>().invoke_more_tapped();
                f.screen.frame(); // the menu counts the rows its window holds once it is drawn
                ui.global::<crate::MenuState>().invoke_tapped(i as i32);
                settle(f);
            }
        }
        close_overlays(f);
    }
}

/// Presses every stop of the screen the path reaches and every action of its bar, checks each, and walks into what opened.
fn walk(f: &mut Fixture, tile: i32, depth: usize, seen: &mut HashSet<String>, pressed: &mut usize, path: &mut Vec<Step>) {
    let ui = f.screen.ui.clone_strong();
    enter(f, tile, path);
    let at = here(&ui);
    if depth == 0 || !seen.insert(at.clone()) {
        return;
    }
    let cursor = |ui: &Shell| ui.global::<crate::PartState>().get_rows().iter().find(|r| r.cursor).map(|r| r.label.to_string());
    let mut last = None;
    for k in 0..30 {
        if here(&ui) != at {
            enter(f, tile, path);
            if here(&ui) != at {
                return; // the path no longer leads here
            }
        }
        press_stop(f, k);
        let now = (here(&ui), cursor(&ui));
        if now.1.is_some() && last.as_ref() == Some(&now) {
            break; // the dial is past the last stop: the same row again
        }
        last = Some(now);
        *pressed += 1;
        no_handover(&ui, &format!("{at} stop {k}"));
        close_overlays(f);
        let mut opened = here(&ui) != at;
        if !opened {
            // A value taken for the dial, or a toggle: a second press lets go of it.
            shell::act(Action::Confirm(None));
            settle(f);
            no_handover(&ui, &format!("{at} stop {k} again"));
            close_overlays(f);
            opened = here(&ui) != at;
        }
        if opened && here(&ui).split(':').next() != Some("0") {
            eprintln!("stop {k} of {at} opened {} via {path:?}", here(&ui));
            path.push(Step::Stop(k));
            walk(f, tile, depth - 1, seen, pressed, path);
            path.pop();
        }
    }
    // The bar's actions, each done.
    enter(f, tile, path);
    let n = ui.global::<Bar>().get_actions().row_count();
    let more = ui.global::<Bar>().get_more();
    if more && tile != 0 {
        // The actions are folded into More: each row of its menu is pressed (the Library and Settings walks).
        ui.global::<Bar>().invoke_more_tapped();
        f.screen.frame();
        let rows = ui.global::<crate::MenuState>().get_rows().row_count();
        close_overlays(f);
        for i in 0..rows {
            enter(f, tile, path);
            ui.global::<Bar>().invoke_more_tapped();
            f.screen.frame();
            ui.global::<crate::MenuState>().invoke_tapped(i as i32);
            settle(f);
            *pressed += 1;
            no_handover(&ui, &format!("{at} menu row {i}"));
            close_overlays(f);
            if here(&ui) != at && ui.get_screen() != 0 {
                path.push(Step::Menu(i));
                walk(f, tile, depth - 1, seen, pressed, path);
                path.pop();
            }
        }
    }
    for i in 0..n {
        if more {
            break; // the same actions are in More's menu
        }
        enter(f, tile, path);
        ui.global::<Bar>().invoke_action(i as i32);
        settle(f);
        *pressed += 1;
        no_handover(&ui, &format!("{at} bar action {i}"));
        close_overlays(f);
        if here(&ui) != at && ui.get_screen() != 0 {
            path.push(Step::Bar(i));
            walk(f, tile, depth - 1, seen, pressed, path);
            path.pop();
        }
    }
}

#[test]
fn nothing_in_the_performance_app_hands_the_display_over() {
    let mut f = Fixture::new("handover");
    f.engine.answer(); // session.hello
    f.pump();
    let ui = f.screen.ui.clone_strong();
    ui.global::<HomeState>().invoke_tapped(0); // the Performance tile
    settle(&mut f);
    assert_eq!(ui.get_screen(), 3, "the Performance tile opens Play in this shell");
    no_handover(&ui, "the Performance tile");
    let (mut seen, mut pressed) = (HashSet::new(), 0);
    walk(&mut f, 0, 7, &mut seen, &mut pressed, &mut vec![]);
    let mut screens: Vec<&String> = seen.iter().collect();
    screens.sort();
    eprintln!("walked {} places with {pressed} presses: {screens:?}", seen.len());
    // Every kind of screen of the app was walked (Play 3, Part 4, Chain 5, Sounds 6, Params 7, Scene 8, Parts 9, Mixer 10,
    // Controls 11, Routing 12).
    for screen in [3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
        assert!(seen.iter().any(|s| s.starts_with(&format!("{screen}:"))), "the walk never reached screen {screen}: {screens:?}");
    }
    assert!(pressed > 300, "the walk pressed {pressed}");
}

// The apps on Home that are not ported still hand over: the Groovebox (and Recordings, where a profile offers it); the
// Performance, the Library, the Setlist and Settings are in this shell.
#[test]
fn the_home_tiles_that_still_hand_over_are_the_unported_apps() {
    let names: Vec<String> = {
        let mut f = Fixture::new("handover-names");
        f.engine.answer(); // session.hello
        f.pump();
        f.screen.ui.global::<HomeState>().get_tiles().iter().map(|t| t.name.to_string()).collect()
    };
    let mut handing: Vec<String> = vec![];
    for (i, name) in names.iter().enumerate() {
        // A fresh shell for each: a handover ends the program.
        let mut f = Fixture::new(&format!("handover-home-{i}"));
        f.engine.answer(); // session.hello
        f.pump();
        f.screen.ui.global::<HomeState>().invoke_tapped(i as i32);
        settle(&mut f);
        if f.screen.ui.get_screen() == 2 {
            handing.push(name.clone());
        }
    }
    eprintln!("tiles that hand over: {handing:?} of {names:?}");
    assert!(!handing.iter().any(|n| ["Performance", "Library", "Setlist", "Settings"].contains(&n.as_str())), "{handing:?}");
    assert_eq!(handing, ["Groovebox"], "the one app not ported is still the JUCE app's");
}

// The same walk from Home's Library tile: every stop and every bar action of the Library app, the catalog and the file chooser
// and the questions it opens included, asks for no handover.
#[test]
fn nothing_in_the_library_app_hands_the_display_over() {
    let mut f = Fixture::new("handover-library");
    f.engine.answer(); // session.hello
    f.pump();
    let ui = f.screen.ui.clone_strong();
    shell::library_in_rust(true);
    ui.global::<HomeState>().invoke_tapped(1); // the Library tile
    settle(&mut f);
    assert_eq!(ui.get_screen(), 13, "the Library tile opens the Rust Library");
    no_handover(&ui, "the Library tile");
    let (mut seen, mut pressed) = (HashSet::new(), 0);
    for page in 0..3 {
        walk(&mut f, 1, 7, &mut seen, &mut pressed, &mut vec![Step::Page(page)]);
    }
    // The sidebar's lines and the search box and its cross, each touched on every kind.
    for page in 0..3 {
        enter(&mut f, 1, &[Step::Page(page)]);
        let lib = ui.global::<crate::LibraryState>();
        for i in 0..lib.get_side_rows().row_count() {
            lib.invoke_side_tapped(i as i32);
            settle(&mut f);
            pressed += 1;
            no_handover(&ui, &format!("sidebar line {i} of kind {page}"));
        }
        lib.invoke_search_tapped();
        settle(&mut f);
        close_overlays(&mut f);
        lib.invoke_search_cleared();
        settle(&mut f);
        pressed += 2;
        no_handover(&ui, &format!("search of kind {page}"));
    }
    let mut screens: Vec<&String> = seen.iter().collect();
    screens.sort();
    eprintln!("walked {} places with {pressed} presses: {screens:?}", seen.len());
    assert!(seen.iter().any(|s| s.starts_with("13:Library")), "the Library's list was walked: {screens:?}");
    assert!(seen.len() >= 3, "the walk went into what opens (filters, menus, the catalog): {screens:?}");
    assert!(pressed > 40, "the walk pressed {pressed}");
}

// The same walk from Home's Settings tile: every page of its rail, every stop and bar action, the menus and questions.
#[test]
fn nothing_in_the_settings_app_hands_the_display_over() {
    let mut f = Fixture::new("handover-settings");
    f.engine.answer(); // session.hello
    f.pump();
    let ui = f.screen.ui.clone_strong();
    shell::settings_in_rust(true);
    ui.global::<HomeState>().invoke_tapped(4); // the Settings tile
    settle(&mut f);
    assert_eq!(ui.get_screen(), 14, "the Settings tile opens the Rust Settings");
    no_handover(&ui, "the Settings tile");
    let (mut seen, mut pressed) = (HashSet::new(), 0);
    for page in 0..6 {
        walk(&mut f, 4, 7, &mut seen, &mut pressed, &mut vec![Step::Page(page)]);
    }
    let mut screens: Vec<&String> = seen.iter().collect();
    screens.sort();
    eprintln!("walked {} places with {pressed} presses: {screens:?}", seen.len());
    assert!(seen.iter().any(|s| s.starts_with("14:Settings")), "the Settings' pages were walked: {screens:?}");
    assert!(seen.len() >= 6, "every page was walked: {screens:?}");
    assert!(pressed > 30, "the walk pressed {pressed}");
}
