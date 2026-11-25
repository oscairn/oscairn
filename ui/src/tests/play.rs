// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rest of Play (A6, A12, A13) on the shell through the fake engine: the master reached by the cursor
//! and changed by the dial, the no-MIDI hint and its two links, and the empty state.

use super::bar::{play_on_the_shell, request_in};
use super::*;
use std::time::Duration;

fn status(f: &mut Fixture, inputs: serde_json::Value) {
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": {"cpuLoad": 0.1, "midiInputs": inputs}})));
    f.pump();
}

#[test]
fn the_master_is_the_last_stop_of_the_right_pane_and_the_dial_moves_its_fader() {
    let mut f = play_on_the_shell("play-master", json!({"id": "p1", "name": "Grand", "unsaved": false}), play_graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    let state = || ui.global::<crate::PlayState>();
    let master = || (state().get_master().cursor, state().get_master().editing);
    for _ in 0..3 {
        shell::act(Action::Move { pane: Some(1), by: 1 }); // the first shows the first part, then the next two
    }
    assert_eq!((state().get_strips().row_data(2).unwrap().cursor, master()), (true, (false, false)));
    shell::act(Action::Move { pane: Some(1), by: 1 }); // past the last part: the master
    assert_eq!((state().get_strips().row_data(2).unwrap().cursor, master()), (false, (true, false)));
    shell::act(Action::Move { pane: Some(1), by: 1 }); // held at the end
    assert_eq!(master(), (true, false));
    shell::act(Action::Move { pane: Some(1), by: -1 });
    assert_eq!((state().get_strips().row_data(2).unwrap().cursor, master()), (true, (false, false)), "back onto the last part");
    shell::act(Action::Move { pane: Some(1), by: 1 });
    // A press, then the dial: one click is 1/40 of the fader's travel, sent as a fader's moves are, committed.
    shell::act(Action::Confirm(Some(1)));
    assert_eq!(master(), (true, true));
    f.screen.frame();
    f.screen.save("play-master");
    png(&f.screen, "play-master");
    shell::act(Action::Move { pane: None, by: 1 });
    let r = request_in(&mut f, "master.set", &none, &play_graph());
    assert_eq!((r["params"]["level"].as_f64().unwrap(), r["params"]["pan"].as_f64().unwrap()), (1.072, 0.0));
    f.reply(&r, json!({"result": {"accepted": true}}));
    let r = request_in(&mut f, "graph.commit", &none, &play_graph());
    f.reply(&r, json!({"result": {"revision": 1}}));
    shell::act(Action::Move { pane: None, by: -1 });
    let r = request_in(&mut f, "master.set", &none, &play_graph());
    assert_eq!(r["params"]["level"].as_f64().unwrap(), 1.0, "a click back is where it was");
    f.reply(&r, json!({"result": {"accepted": true}}));
    let r = request_in(&mut f, "graph.commit", &none, &play_graph());
    f.reply(&r, json!({"result": {"revision": 1}}));
    // Cancel is Back in one step, with the master's fader being changed: Play goes back to where it was opened from.
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 0);
}

// One test, as it writes the user's keyboard.txt (the tests' own folder) and two at once would meet there.
#[test]
fn the_no_midi_hint_offers_the_keyboard_and_can_be_dismissed_for_good() {
    let file = crate::settings::config_dir().unwrap().join("keyboard.txt");
    let _ = std::fs::remove_file(&file);
    let mut f = play_on_the_shell("play-hint", json!({"id": "p1", "name": "Grand", "unsaved": false}), play_graph());
    let ui = f.screen.ui.clone_strong();
    let state = || (ui.global::<crate::PlayState>().get_hint(), ui.global::<crate::PlayState>().get_hint_cursor());
    assert_eq!(state(), (false, -1), "the FANTOM-07 has keys of its own: no hint, whatever the inputs");
    status(&mut f, json!([{"kind": "builtIn"}]));
    assert_eq!(state(), (false, -1));
    shell::offer_keyboard(true, crate::settings::keyboard_hint_dismissed()); // a profile without keys
    assert_eq!(state(), (true, -1));
    shell::offer_keyboard(true, false);
    status(&mut f, json!([]));
    shell::offer_keyboard(true, false);
    status(&mut f, json!([{"kind": "builtIn", "name": "Built in"}]));
    assert_eq!(state(), (true, -1), "only the built-in input: no MIDI device");
    f.screen.frame();
    f.screen.save("play-hint");
    png(&f.screen, "play-hint");
    // A device connects and the line goes; unplugged, it is back.
    status(&mut f, json!([{"kind": "builtIn"}, {"kind": "usb", "name": "Keystation"}]));
    assert!(!state().0);
    status(&mut f, json!([{"kind": "builtIn"}]));
    assert!(state().0);
    // The cursor: the first move of the right pane shows it on the first link, a press opens the keyboard.
    shell::act(Action::Move { pane: Some(1), by: 1 });
    assert_eq!(state(), (true, 0));
    shell::act(Action::Confirm(Some(1)));
    assert!(ui.global::<crate::PianoState>().get_open(), "the on-screen keyboard");
    shell::act(Action::Cancel(None));
    assert!(!ui.global::<crate::PianoState>().get_open());
    // Dismiss: gone, and kept for the next start, in the JUCE app's file.
    shell::act(Action::Move { pane: Some(1), by: 1 });
    assert_eq!(state(), (true, 1));
    shell::act(Action::Move { pane: Some(1), by: 1 }); // on to the first part
    assert_eq!(state().1, -1);
    shell::act(Action::Move { pane: Some(1), by: -1 });
    assert_eq!(state().1, 1, "and back up onto the links");
    shell::act(Action::Confirm(Some(1)));
    assert!(!state().0);
    assert_eq!(std::fs::read_to_string(&file).unwrap().lines().next(), Some("hint dismissed"));
    status(&mut f, json!([{"kind": "builtIn"}]));
    assert!(!state().0, "it stays dismissed");
    // The windows' places the JUCE app keeps in the file stay, and the line is written once.
    std::fs::write(&file, "10 20 640 200 Keystation\n").unwrap();
    crate::settings::dismiss_keyboard_hint().unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hint dismissed\n10 20 640 200 Keystation\n");
    crate::settings::dismiss_keyboard_hint().unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap().matches("hint dismissed").count(), 1);
    let _ = std::fs::remove_file(&file);
}

// No parts: the strips and the master give way to a line saying so (A13); the scenes stay.
#[test]
fn an_empty_performance_says_so() {
    let mut empty = play_graph();
    empty["parts"] = json!([]);
    let mut f = play_on_the_shell("play-empty", json!({"id": "p1", "name": "Grand", "unsaved": false}), empty);
    let play = f.screen.ui.global::<crate::PlayState>();
    assert!(!play.get_has_master() && play.get_strips().row_count() == 0);
    assert_eq!(play.get_scenes().iter().filter(|s| s.filled).count(), 2);
    f.screen.frame();
    f.screen.save("play-empty");
    png(&f.screen, "play-empty");
}

// The walk: every actionable control of the Play screen reached and activated from Home with panel
// moves only (move, confirm, cancel/EXIT), no touch. The dial (pane 0) turns over the scene tiles and
// the "+ Scene" tile, confirm selects each (scenes.select, with the scene) and adds one (scenes.add,
// named as the JUCE view names it); the right knob (pane 1) walks the strips, confirm opens each part
// (the Part screen: the view changes) and EXIT comes back; past the last strip the master is the
// pane's last stop, confirm then the dial move its fader (master.set). What the walk visited is held
// against what the screen shows (the scenes and strips of its own models): every one, in order, none twice.
#[test]
fn play_reachable_from_home_by_panel_moves() {
    let mut f = Fixture::new("play-walk");
    f.engine.answer(); // session.hello
    f.pump();
    let ui = f.screen.ui.clone_strong();
    let shared = play_graph();
    let none = json!({"undo": "", "redo": ""});
    // What the scene presses asked of the engine, in turn: (scenes.select, the scene) or (scenes.add, the name).
    let sent = std::cell::RefCell::new(Vec::<(String, serde_json::Value)>::new());
    // A quiet engine: every request out is answered until none comes for a moment.
    let settle = |f: &mut Fixture| {
        while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(15)) {
            let method = r["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "graph.get" => shared.clone(),
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "library.query" => json!({"items": [], "total": 0}),
                "library.libraries" => json!({"libraries": []}),
                "params.list" => json!({"parameters": []}),
                "session.status" => json!({"midiInputs": [], "midiOutputs": []}),
                "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
                "window.info" => json!({"available": false}),
                "presets.list" => json!({"presets": []}),
                "edit.history" | "edit.undo" | "edit.redo" => json!({"undo": "", "redo": ""}),
                "record.get" => json!({"recording": 0, "retro": {"seconds": 30}, "takes": []}),
                "parts.meters" => json!({"meters": []}),
                _ => json!({}),
            };
            match method.as_str() {
                "scenes.select" | "scenes.add" => sent.borrow_mut().push((method.clone(), r["params"].clone())),
                _ => {}
            }
            f.reply(&r, json!({"result": result}));
        }
    };
    settle(&mut f);
    // Home → Play: the dial onto the Performance tile, confirm opens it.
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |f: &mut Fixture, key: &str, by: i64| {
        *total.entry(key.into()).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move", 0); // the first read is only a baseline
    nav(&mut f, "move", 1); // the cursor onto the Performance tile
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert_eq!(ui.get_screen(), 3, "the Performance tile opens Play");
    // What the walk must visit, from the screen's own models: the scenes it shows (the filled tiles)
    // and the "+ Scene" tile after them, the strips, and the master as the strips' pane's last stop.
    let state = ui.global::<crate::PlayState>();
    let tiles: Vec<crate::SceneData> = state.get_scenes().iter().collect();
    let scenes = tiles.iter().filter(|t| t.filled).count();
    assert!(tiles.iter().any(|t| t.add), "a tile after the last scene adds one: the “+ Scene” stop");
    let stops = scenes + 1;
    let n = state.get_strips().row_count();
    assert!(state.get_strips_per_view() as usize >= n, "the strips fit one view, for the walk to count them");
    assert!(n > 0, "the screen shows no strips");
    let names: Vec<String> = state.get_strips().iter().map(|s| s.name.to_string()).collect();
    // The scenes: pane 0 over the tiles, confirm selects each (and adds one past the last), its effect the request.
    nav(&mut f, "move:0", 1); // the first move shows the cursor on the first tile
    settle(&mut f);
    for i in 0..stops {
        assert_eq!(state.get_scenes().iter().position(|t| t.cursor), Some(i), "the dial is on scene {i}");
        sent.borrow_mut().clear();
        nav(&mut f, "confirm:0", 1);
        settle(&mut f);
        // Its effect is the request the engine received: a scene selects (with the scene), the tile past the last adds one.
        let asked = sent.borrow().first().unwrap_or_else(|| panic!("the press on scene {i} asked something")).clone();
        assert_eq!(sent.borrow().len(), 1, "the press on scene {i} asked exactly once: {asked:?}");
        if i < scenes {
            let id = shared["scenes"][i]["id"].clone();
            assert_eq!(asked, ("scenes.select".to_string(), json!({"scene": id})), "scene {i} selects the scene it shows");
        } else {
            assert_eq!(asked, ("scenes.add".to_string(), json!({"name": format!("Scene {}", scenes + 1)})), "the tile after the last scene adds one, as the JUCE view names it");
        }
        nav(&mut f, "move:0", 1); // on to the next tile (past the last, into the knob row, when the stops are done)
        settle(&mut f);
    }
    // The strips: pane 1 over them, confirm opens each part, EXIT comes back.
    for (i, name) in names.iter().enumerate() {
        nav(&mut f, "move:1", 1); // the first move shows the cursor on the first strip
        settle(&mut f);
        for _ in 0..i {
            nav(&mut f, "move:1", 1);
            settle(&mut f);
        }
        assert_eq!(state.get_strips().iter().position(|s| s.cursor), Some(i), "the dial is on strip {i}");
        nav(&mut f, "confirm:1", 1);
        settle(&mut f);
        assert_eq!(ui.get_screen(), 4, "a press on strip {i} opens its Part screen");
        assert_eq!(title(&ui), *name, "the Part screen is “{name}”");
        nav(&mut f, "cancel", 1); // EXIT
        settle(&mut f);
        assert_eq!(ui.get_screen(), 3, "EXIT comes back to Play");
    }
    // The master, the strips' pane's last stop: confirm, then the dial move its fader (master.set).
    nav(&mut f, "move:1", 1); // the cursor on the first strip again
    settle(&mut f);
    for _ in 0..n {
        nav(&mut f, "move:1", 1); // past the last strip: the master
        settle(&mut f);
    }
    assert!(state.get_has_master(), "the master is a stop of the strips' pane");
    assert_eq!((state.get_master().cursor, state.get_master().editing), (true, false));
    nav(&mut f, "confirm:1", 1);
    settle(&mut f);
    assert!(state.get_master().editing, "a press on the master starts its fader");
    nav(&mut f, "move", 1); // the dial: one click is 1/40 of the fader's travel
    let r = request_in(&mut f, "master.set", &none, &shared);
    assert!(r["params"]["level"].as_f64().unwrap() > 1., "the dial click raised the master from unity");
    assert_eq!(r["params"]["pan"].as_f64(), Some(0.), "the pan is left where it was");
    f.reply(&r, json!({"result": {"accepted": true}}));
    let r = request_in(&mut f, "graph.commit", &none, &shared);
    f.reply(&r, json!({"result": {"revision": 1}}));
}

// A performance instrument: with Play shown and nothing happening, it asks nothing and draws nothing. The meters
// played while Play is shown (parts.meters, 100 ms apart) are answered with silence until the asking stops.
#[test]
fn at_rest_it_costs_no_request_and_no_frame() {
    let mut f = play_on_the_shell("play-idle", json!({"id": "p1", "name": "Grand", "unsaved": false}), play_graph());
    // The meters stop after two seconds of quiet: answer them with silence and let that quiet pass.
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        slint::platform::update_timers_and_animations();
        while let Some(request) = f.engine.try_request(Duration::from_millis(20)) {
            f.reply(&request, json!({"result": {"meters": []}}));
        }
    }
    f.screen.frame();
    f.screen.frame();
    assert_eq!(f.screen.frame(), None, "nothing is drawn at rest");
    std::thread::sleep(Duration::from_millis(300));
    slint::platform::update_timers_and_animations();
    assert!(f.engine.try_request(Duration::from_millis(500)).is_none(), "nothing is asked at rest");
}
