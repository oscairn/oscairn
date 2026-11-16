// SPDX-License-Identifier: AGPL-3.0-or-later
//! Scenes on Play and the scene's setup (A2 to A4): the "+ Scene" tile after the last scene adds one, the
//! Scene screen from the top bar stores, renames, sets the scope and the transition, each by the cursor and
//! the dial and by touch, as the JUCE menu's requests (PartsView.cpp sceneEditMenu).

use super::bar::{play_on_the_shell, request_in};
use super::*;

fn graph() -> serde_json::Value {
    let mut g = play_graph();
    g["scenes"] = json!([
        {"id": 1, "name": "Verse", "scope": ["enabled", "mutes", "mix", "parameters", "mappings"], "glide": 0, "curve": "linear", "launch": "now", "parts": [{}, {}], "values": []},
        {"id": 2, "name": "Chorus", "scope": ["mutes", "mix"], "glide": 0, "curve": "linear", "launch": "now", "parts": [{}, {}, {}], "values": [{}, {}]},
    ]);
    g
}

fn rows(f: &Fixture) -> Vec<(String, String, bool)> {
    f.screen.ui.global::<crate::PartState>().get_rows().iter().map(|r| (r.label.to_string(), r.value.to_string(), r.cursor)).collect()
}

fn done() -> serde_json::Value {
    json!({"result": {"accepted": true}})
}

#[test]
fn a_scene_is_added_from_the_tile_after_the_last() {
    let mut f = play_on_the_shell("scene-add", json!({"id": "p1", "name": "Grand", "unsaved": false}), graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    let tiles = ui.global::<crate::PlayState>().get_scenes();
    let add: Vec<bool> = tiles.iter().map(|t| t.add).collect();
    assert_eq!(add, [false, false, true, false, false, false, false, false], "one + Scene tile, after the last");
    // The left knob reaches it (the first move shows the cursor on the first tile); a press adds.
    for _ in 0..3 {
        shell::act(Action::Move { pane: Some(0), by: 1 });
    }
    assert!(ui.global::<crate::PlayState>().get_scenes().row_data(2).unwrap().cursor);
    shell::act(Action::Confirm(Some(0)));
    let r = request_in(&mut f, "scenes.add", &none, &graph());
    assert_eq!(r["params"], json!({"name": "Scene 3"}));
    f.reply(&r, json!({"result": {"scene": 3}}));
    // A tap does the same.
    ui.global::<crate::PlayState>().invoke_scene_tapped(2);
    let r = request_in(&mut f, "scenes.add", &none, &graph());
    assert_eq!(r["params"], json!({"name": "Scene 3"}));
    f.reply(&r, json!({"result": {"scene": 4}}));
    // With eight scenes there is no tile to add one.
    let mut full = graph();
    full["scenes"] = json!((1..=8).map(|i| json!({"id": i, "name": format!("S{i}")})).collect::<Vec<_>>());
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "graph.changed", "params": {}})));
    f.pump();
    let r = request_in(&mut f, "graph.get", &none, &full);
    f.reply(&r, json!({"result": full}));
    let add: Vec<bool> = ui.global::<crate::PlayState>().get_scenes().iter().map(|t| t.add).collect();
    assert!(add.iter().all(|a| !a));
    f.screen.frame();
    f.screen.save("play-scenes");
    png(&f.screen, "play-scenes");
}

#[test]
fn the_scene_screen_stores_renames_scopes_and_transitions() {
    let mut f = play_on_the_shell("scene-setup", json!({"id": "p1", "name": "Grand", "unsaved": false}), graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    let g = graph();
    let want = |f: &mut Fixture, method: &str| request_in(f, method, &none, &g);
    ui.global::<crate::Bar>().invoke_action(3); // Scene
    assert_eq!((ui.get_screen(), ui.global::<crate::Bar>().get_where().as_str()), (8, "Scene"));
    for _ in 0..4 {
        // The screen asks again for what it shows: the Performance, the graph, the history.
        let r = f.engine.request();
        let result = match r["method"].as_str().unwrap() {
            "graph.get" => g.clone(),
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            _ => none.clone(),
        };
        f.reply(&r, json!({"result": result}));
    }
    let labels: Vec<String> = rows(&f).iter().map(|r| r.0.clone()).collect();
    assert_eq!(labels, ["Store", "Rename", "Plugins and wires on or off", "Mutes and solos", "Levels, pans and sends", "Parameters and controls", "Control mappings", "Part note zones", "Glide time", "Curve", "While playing"]);
    let values: Vec<String> = rows(&f).iter().map(|r| r.1.clone()).collect();
    assert_eq!(values, ["capture the state now", "Chorus ›", "Off", "On", "On", "Off", "Off", "Off", "At once", "Linear", "At once"]);
    f.screen.frame();
    f.screen.save("scene-setup");
    png(&f.screen, "scene-setup");
    // Store, then Rename on the keyboard (the name starts as the scene's).
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let r = want(&mut f, "scenes.store");
    assert_eq!(r["params"], json!({"scene": 2}));
    f.reply(&r, done());
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    assert!(ui.global::<crate::KeyboardState>().get_open());
    shell::act(Action::End(1));
    shell::act(Action::Confirm(None)); // Done
    let r = want(&mut f, "scenes.name");
    assert_eq!(r["params"], json!({"scene": 2, "name": "Chorus"}));
    f.reply(&r, done());
    // The scope: a press on a kind flips it, in the engine's order.
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let r = want(&mut f, "scenes.scope");
    assert_eq!(r["params"], json!({"scene": 2, "scope": ["enabled", "mutes", "mix"]}));
    f.reply(&r, done());
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None)); // Mutes and solos: off
    let r = want(&mut f, "scenes.scope");
    assert_eq!(r["params"], json!({"scene": 2, "scope": ["mix"]}));
    f.reply(&r, done());
    // The curve needs a glide time: said, nothing to change.
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -1 });
    shell::act(Action::Confirm(None));
    assert!(ui.global::<crate::PartState>().get_message().contains("glide"));
    // The glide: a press, then the dial through the times; a press or Cancel ends it.
    shell::act(Action::Move { pane: None, by: -1 });
    shell::act(Action::Confirm(None));
    shell::act(Action::Move { pane: None, by: 1 });
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"], json!({"scene": 2, "glide": 100, "curve": "linear", "launch": "now"}));
    f.reply(&r, done());
    shell::act(Action::Move { pane: None, by: 1 });
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"]["glide"], 250);
    f.reply(&r, done());
    shell::act(Action::Move { pane: None, by: -3 });
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"]["glide"], 100, "steps of the menu's times, one a click");
    f.reply(&r, done());
    shell::act(Action::Cancel(None)); // ends the change, the cursor stays
    assert_eq!(ui.get_screen(), 8);
    // Now the curve and the launch change, by the dial (and by touch: + on the row).
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    shell::act(Action::Move { pane: None, by: 5 }); // however fast it turns: one choice a click
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"], json!({"scene": 2, "glide": 100, "curve": "ease", "launch": "now"}));
    f.reply(&r, done());
    shell::act(Action::Move { pane: None, by: 1 });
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"], json!({"scene": 2, "glide": 100, "curve": "equal-power", "launch": "now"}));
    f.reply(&r, done());
    shell::act(Action::Move { pane: None, by: 1 }); // the last choice: no more
    shell::act(Action::Move { pane: None, by: -1 });
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"]["curve"], "ease", "the end held, so this was the step back");
    f.reply(&r, done());
    shell::act(Action::Move { pane: None, by: 1 });
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"]["curve"], "equal-power");
    f.reply(&r, done());
    shell::act(Action::Cancel(None));
    ui.global::<crate::PartState>().invoke_step(10, 1); // the launch's +
    let r = want(&mut f, "scenes.transition");
    assert_eq!(r["params"], json!({"scene": 2, "glide": 100, "curve": "equal-power", "launch": "beat"}));
    f.reply(&r, done());
    assert_eq!(rows(&f)[10].1, "On the next beat");
    // A refusal is said; Cancel drops the cursor, then goes back to Play.
    shell::act(Action::Move { pane: None, by: 0 });
    ui.global::<crate::PartState>().invoke_tapped(0); // Store, by touch
    let r = want(&mut f, "scenes.store");
    f.reply(&r, json!({"error": {"code": -32602, "message": "no such scene"}}));
    assert!(ui.global::<crate::PartState>().get_message().contains("no such scene"));
    let _ = want(&mut f, "graph.get"); // the page set right
    shell::act(Action::Cancel(None));
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 3);
}
