// SPDX-License-Identifier: AGPL-3.0-or-later
//! The chain, parameter and sound screens on the shell through the fake engine (their own tests are
//! in pages.rs): the Part screen's "Insert effects" opens the chain, a plugin's Parameters its pages
//! (params.changed is subscribed while they show and let go after), Cancel goes back one screen at a
//! time, and "Sound" opens the chooser, whose library.query is asked.

use super::*;

use std::time::Duration;

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
    // The last view of the Part list: Name, Remove part (twelve rows a view; Sound and Insert effects end the one before).
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -2 });
    assert_eq!(rows(&ui)[11].0, "Insert effects");
    shell::act(Action::Confirm(None));
    assert_eq!((ui.get_screen(), title(&ui)), (5, "Keys › Insert effects".to_string()));
    assert_eq!(rows(&ui)[1].0, "1. Chorus");
    // Chorus, then its Parameters.
    shell::act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None)); // Chorus, then its Parameters.
    let r = next(&mut f, "window.info");
    f.reply(&r, json!({"result": {"available": false}}));
    assert_eq!(rows(&ui)[0].0, "Parameters");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    assert_eq!((ui.get_screen(), title(&ui)), (7, "Keys › Insert effects › Chorus".to_string()));
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
    assert_eq!((ui.get_screen(), title(&ui)), (6, "Keys › Sound".to_string()));
    let r = next(&mut f, "library.query");
    assert_eq!(r["params"], json!({"kind": "sound", "limit": 1000}));
    f.reply(&r, json!({"result": {"items": [{"id": "s1", "name": "Warm Rhodes"}], "total": 1}}));
    assert_eq!(rows(&ui).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Search", "Filters", "Warm Rhodes"]);
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 4);
}

// A `session.resync` (the engine's parameter-change history wrapped, or a pending command dropped:
// docs/control-protocol.md) means the page's values may be stale: it reads them again, as an Undo does,
// while the page is shown; with no such screen shown it asks nothing.
#[test]
fn a_session_resync_reads_the_page_values_again() {
    let mut f = Fixture::new("resync");
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(0); // Performance: Play
    let graph = super::part::part_graph();
    let ui = f.screen.ui.clone_strong();
    // What comes, comes in an order that drifts with how the events are pumped: answer whatever is
    // asked until the engine goes quiet, then act, so no ordering is relied on.
    let settle = |f: &mut Fixture| {
        let mut quiet = 0;
        while quiet < 2 {
            let Some(r) = f.engine.try_request(std::time::Duration::from_millis(400)) else { quiet += 1; continue };
            quiet = 0;
            let method = r["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "graph.get" => graph.clone(),
                "session.status" => json!({"midiInputs": []}),
                "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
                "window.info" => json!({"available": false}),
                "params.list" => super::pages::param_list(),
                "params.get" => json!({"values": [{"node": 2, "param": 1, "value": 0.25}]}),
                "params.text" => json!({"text": format!("{}", r["params"]["value"].as_f64().unwrap_or_default())}),
                _ => json!({}),
            };
            f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": r["id"], "result": result})));
            // The answer's event and everything it wakes, without a blocking wait: nothing may be pending.
            for _ in 0..25 {
                std::thread::sleep(std::time::Duration::from_millis(2));
                f.drain();
            }
        }
    };
    settle(&mut f); // Play's data: its parts, so the tap finds one
    ui.global::<crate::PlayState>().invoke_part_tapped(0); // Performance: Play's Part
    settle(&mut f);
    assert_eq!(ui.get_screen(), 4);
    // The last view of the Part list: Name, Remove part (twelve rows a view; Sound and Insert effects end the one before).
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -2 });
    assert_eq!(rows(&ui)[11].0, "Insert effects");
    shell::act(Action::Confirm(None)); // the chain opens
    settle(&mut f);
    assert_eq!((ui.get_screen(), title(&ui)), (5, "Keys › Insert effects".to_string()));
    assert_eq!(rows(&ui)[1].0, "1. Chorus");
    // Chorus, then its Parameters.
    shell::act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor, on Chorus
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None)); // the plugin's actions
    settle(&mut f);
    assert_eq!((ui.get_screen(), title(&ui)), (5, "Keys › Insert effects › Chorus".to_string()));
    assert_eq!(rows(&ui)[0].0, "Parameters", "the plugin's actions");
    // Parameters.
    shell::act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor, on Parameters
    shell::act(Action::Confirm(None));
    settle(&mut f);
    assert_eq!((ui.get_screen(), title(&ui)), (7, "Keys › Insert effects › Chorus".to_string()));
    assert_eq!(tiles(&ui)[2], ("Resonance".to_string(), "0.25".to_string()));
    // A `session.resync` (the engine's parameter-change history wrapped, or a pending command dropped:
    // docs/control-protocol.md) means the page's values may be stale: it reads them again, as an Undo
    // does, in requests of at most 128, and the page shows what came back.
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "session.resync", "params": {"topic": "params.changed"}})));
    for _ in 0..25 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        f.drain();
    }
let mut asked: Vec<usize> = Vec::new();
    let mut quiet = 0;
    while quiet < 2 {
        let Some(r) = f.engine.try_request(std::time::Duration::from_millis(700)) else { quiet += 1; continue };
        quiet = 0;
        let method = r["method"].as_str().unwrap_or_default().to_string();
        if method != "params.get" {
            let result = match method.as_str() {
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "graph.get" => graph.clone(),
                "params.text" => json!({"text": "0.75"}),
                _ => json!({}),
            };
            f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": r["id"], "result": result})));
            for _ in 0..25 {
                std::thread::sleep(std::time::Duration::from_millis(2));
                f.drain();
            }
            continue;
        }
        let values = r["params"]["values"].as_array().unwrap().iter().map(|v| v["param"].as_u64().unwrap()).collect::<Vec<_>>();
        assert!(values.len() <= 128);
        asked.push(values.len());
        let all: Vec<_> = values.iter().map(|&p| json!({"node": 2, "param": p, "value": if p == 1 { 0.75 } else { 0.5 }})).collect();
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": r["id"], "result": {"values": all}})));
        for _ in 0..25 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            f.drain();
        }
    }
    assert!(!asked.is_empty(), "a session.resync with the page shown asked for its values again: nothing came");
    let shown: usize = super::pages::param_list()["parameters"].as_array().unwrap().iter().filter(|p| p["flags"].as_u64().unwrap_or(0) & 4 == 0).count(); // FLAG_HIDDEN
    assert_eq!(asked.iter().sum::<usize>(), shown, "every parameter of the page asked again");
    assert_eq!(tiles(&ui)[2], ("Resonance".to_string(), "0.75".to_string()), "the value read again is shown");
    // Back on the chain (the page gone, params.changed let go): a resync asks nothing.
    shell::act(Action::Cancel(None));
    settle(&mut f);
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "session.resync", "params": {"topic": "params.changed"}})));
    assert!(f.engine.try_request(std::time::Duration::from_millis(700)).is_none(), "a resync with no parameter page shown asked for something");
}

// The parameter pages on the shell cost nothing when nothing changes: no request, no frame at rest (docs/rust-ui.md,
// "Nothing is drawn at rest"), as the Part screen's idle test holds for the Part.
#[test]
fn the_parameter_pages_at_rest_cost_no_request_and_no_frame() {
    let mut f = Fixture::new("params-idle");
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
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -2 });
    assert_eq!(rows(&ui)[11].0, "Insert effects");
    shell::act(Action::Confirm(None));
    assert_eq!(ui.get_screen(), 5);
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
    let r = next(&mut f, "params.list");
    f.reply(&r, json!({"result": super::pages::param_list()}));
    let r = next(&mut f, "params.get");
    f.reply(&r, json!({"result": {"values": [{"node": 2, "param": 1, "value": 0.25}]}}));
    // A value that arrived with no text of its own asks the plugin for it once.
    let r = next(&mut f, "params.text");
    f.reply(&r, json!({"result": {"text": "0.25"}}));
    assert_eq!(ui.get_screen(), 7);
    f.screen.frame();
    f.screen.frame();
    assert_eq!(f.screen.frame(), None, "nothing is drawn at rest");
    std::thread::sleep(Duration::from_millis(1200));
    assert!(f.engine.try_request(Duration::from_millis(500)).is_none(), "nothing is asked at rest");
    assert_eq!(f.screen.frame(), None, "nothing is drawn at rest after the timers ran");
    // An unchanged params.changed (the engine's echo of a value the page already shows) costs nothing.
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "params.changed", "params": {"changes": [{"node": 2, "param": 1, "kind": "value", "value": 0.25}]}})));
    f.pump();
    assert!(f.engine.try_request(Duration::from_millis(500)).is_none(), "an unchanged params.changed asked for something");
    assert_eq!(f.screen.frame(), None, "an unchanged params.changed drew a frame");
}
