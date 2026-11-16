// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rest of Play (A6, A12, A13) on the shell through the fake engine: the master reached by the cursor
//! and changed by the dial, the no-MIDI hint and its two links, and the empty state.

use super::bar::{play_on_the_shell, request_in};
use super::*;

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
    // Cancel ends the change and keeps the cursor; the next drops it, the next goes back.
    shell::act(Action::Cancel(None));
    assert_eq!(master(), (true, false));
    shell::act(Action::Cancel(None));
    assert_eq!(master(), (false, false));
    assert_eq!(ui.get_screen(), 3);
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
