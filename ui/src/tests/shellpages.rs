// SPDX-License-Identifier: AGPL-3.0-or-later
//! The chain, parameter and sound screens on the shell through the fake engine (their own tests are
//! in pages.rs): the Part screen's "Insert effects" opens the chain, a plugin's Parameters its pages
//! (params.changed is subscribed while they show and let go after), Cancel goes back one screen at a
//! time, and "Sound" opens the chooser, whose library.query is asked.

use super::*;

fn tiles(ui: &Shell) -> Vec<(String, String)> {
    ui.global::<crate::GridState>().get_tiles().iter().map(|t| (t.name.to_string(), t.value.to_string())).collect()
}

fn rows(ui: &Shell) -> Vec<(String, String)> {
    ui.global::<crate::PartState>().get_rows().iter().map(|r| (r.label.to_string(), r.value.to_string())).collect()
}

#[test]
fn part_to_chain_parameters_and_sounds_on_the_shell() {
    let mut f = Fixture::new("pages");
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(0); // Performance: Play
    let graph = super::part::part_graph();
    for _ in 0..12 {
        let request = f.engine.request();
        let result = match request["method"].as_str().unwrap() {
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => graph.clone(),
            "library.query" => json!({"items": [], "total": 0}),
            _ => json!({}),
        };
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        f.pump();
    }
    let ui = f.screen.ui.clone_strong();
    let next = |f: &mut Fixture, want: &str| loop {
        let r = f.engine.request();
        let method = r["method"].as_str().unwrap().to_string();
        let result = match method.as_str() {
            _ if method == want => return r,
            "parts.meters" => json!({"meters": []}),
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => graph.clone(),
            "session.status" => json!({"midiInputs": []}),
            "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
            "params.text" => json!({"text": "t"}),
            "controllers.view" => json!({"accepted": true}),
            "unsubscribe" | "subscribe" => json!({}),
            "edit.history" => json!({"undo": "", "redo": ""}),
            _ => return r,
        };
        f.reply(&r, json!({"result": result}));
    };
    ui.global::<crate::PlayState>().invoke_part_tapped(0);
    assert_eq!(ui.get_screen(), 4);
    // The last view of the Part list: Sound, Insert effects, Name, Remove part.
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -2 });
    assert_eq!(rows(&ui)[1].0, "Insert effects");
    shell::act(Action::Confirm(None));
    assert_eq!((ui.get_screen(), ui.global::<Bar>().get_where().as_str()), (5, "Insert effects"));
    assert_eq!(rows(&ui)[1].0, "1. Chorus");
    // Chorus, then its Parameters.
    shell::act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let r = next(&mut f, "window.info");
    f.reply(&r, json!({"result": {"available": false}}));
    assert_eq!(rows(&ui)[0].0, "Parameters");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    assert_eq!((ui.get_screen(), ui.global::<Bar>().get_where().as_str()), (7, "Parameters"));
    let r = next(&mut f, "params.list");
    assert_eq!(r["params"], json!({"node": 2}));
    f.reply(&r, json!({"result": super::pages::param_list()}));
    let r = next(&mut f, "params.get");
    f.reply(&r, json!({"result": {"values": [{"node": 2, "param": 1, "value": 0.25}]}}));
    // The filter by name (Cutoff, Resonance, Type) is one run, in the filter's order: Type first.
    assert_eq!(tiles(&ui).iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["Type", "Cutoff", "Resonance", "Bypass"]);
    // A change from the engine shows (the shell subscribed to params.changed on opening).
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "params.changed", "params": {"changes": [{"node": 2, "param": 1, "kind": "value", "value": 0.5}]}})));
    f.pump();
    assert_eq!(tiles(&ui)[2], ("Resonance".to_string(), "0.5".to_string()));
    // Cancel: back to the chain, and params.changed let go.
    shell::act(Action::Cancel(None)); // the cursor was never put on a tile: back at once
    assert_eq!(ui.get_screen(), 5);
    let r = next(&mut f, "unsubscribe");
    assert_eq!((r["method"].as_str(), r["params"]["topics"][0].as_str()), (Some("unsubscribe"), Some("params.changed")));
    f.reply(&r, json!({"result": {}}));
    // The chain (its cursor went with the screen): its actions close, then it goes back to the Part screen.
    shell::act(Action::Cancel(None));
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 4);
    // Sound: three rows up from the last, which turns the view back.
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -3 });
    assert!(rows(&ui).iter().any(|r| r.0 == "Sound"));
    shell::act(Action::Confirm(None));
    assert_eq!((ui.get_screen(), ui.global::<Bar>().get_where().as_str()), (6, "Sound"));
    let r = next(&mut f, "library.query");
    assert_eq!(r["params"], json!({"kind": "sound", "limit": 1000}));
    f.reply(&r, json!({"result": {"items": [{"id": "s1", "name": "Warm Rhodes"}], "total": 1}}));
    assert_eq!(rows(&ui).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Search", "Warm Rhodes"]);
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 4);
}
