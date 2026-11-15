// SPDX-License-Identifier: AGPL-3.0-or-later
//! The chain and sound-chooser screens' tests (chain.rs, sounds.rs; the parameter pages are tests/grid.rs),
//! on their own against the engine's words: each action's requests (and the commit after each edit),
//! the refusals, the cursor without touch, and the drawing in the themes.

use super::*;
use crate::client::Error;
use crate::scan::Plugin;

fn rows(ui: &Shell) -> Vec<(String, String, i32, bool, bool)> {
    ui.global::<crate::PartState>().get_rows().iter().map(|r| (r.label.to_string(), r.value.to_string(), r.kind, r.cursor, r.editing)).collect()
}

fn labels(ui: &Shell) -> Vec<String> {
    rows(ui).into_iter().map(|r| r.0).collect()
}

fn info(ui: &Shell) -> Vec<(bool, String)> {
    ui.global::<crate::PartState>().get_info().iter().map(|l| (l.head, l.text.to_string())).collect()
}

fn graph() -> serde_json::Value {
    json!({
        "nodes": [
            {"id": 1, "name": "Rhodes", "plugin": {"name": "Surge XT"}, "audioInputs": [], "audioOutputs": [{"channels": 2}], "noteInputs": 1, "noteOutputs": 0, "enabled": true},
            {"id": 2, "name": "Chorus", "plugin": {"name": "Chorus"}, "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0, "enabled": false},
        ],
        "parts": [{"id": 10, "name": "Keys", "level": 0.5, "pan": 0.0, "mute": false, "solo": false, "chain": [2, 1], "bus": 0, "sends": [], "from": {"sound": "s1", "name": "Warm Rhodes", "modified": false},
                   "input": {"kind": "notes", "port": 0, "devices": [], "zones": []}}],
        "wires": [], "scenes": [], "scene": 0,
    })
}

fn sent(out: &mut Vec<(impl Sized, &'static str, serde_json::Value)>) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(out).into_iter().map(|(_, m, p)| (m, p)).collect()
}

fn chain_screen(s: &Screen) -> crate::chain::Screen {
    tall(&s.ui);
    let mut c = crate::chain::Screen::new(&s.ui);
    c.graph(&graph());
    c.open(10);
    c.shown();
    c
}

fn press(c: &mut crate::chain::Screen, row: usize) -> crate::chain::Outcome {
    c.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor
    c.act(Action::Move { pane: None, by: row as i32 });
    c.act(Action::Confirm(None))
}

fn plugin(name: &str, id: &str, features: &[&str]) -> Plugin {
    Plugin { path: format!("/p/{name}.clap"), id: id.into(), name: name.into(), vendor: String::new(), features: features.iter().map(|f| f.to_string()).collect() }
}

// The chain lists its plugins with an insert row before each and after the last; a press on one opens
// its actions (window.info is asked, a plugin view shown disabled when it has one); turn off or on,
// move, remove, each its requests and a commit; the parameters open its pages; Cancel goes back a layer.
#[test]
fn chain_actions_and_requests() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = chain_screen(&s);
    assert_eq!(labels(ui), ["+ Insert plugin here", "1. Chorus", "+ Insert plugin here", "2. Rhodes", "+ Insert plugin here"]);
    assert_eq!(rows(ui)[1].1, "Effect (off) ›");
    assert_eq!(rows(ui)[3].1, "Instrument ›");
    assert_eq!(ui.global::<crate::PartState>().get_name().as_str(), "Keys");
    assert!(sent(&mut c.out).is_empty(), "looking sends nothing");
    // Chorus: its actions; window.info asked; no view row until it says there is one.
    assert_eq!(press(&mut c, 1), crate::chain::Outcome::Stay);
    assert_eq!(sent(&mut c.out), [("window.info", json!({"node": 2}))]);
    assert_eq!(labels(ui), ["Parameters", "Turn on", "Move later", "Remove"], "the first of the chain cannot move earlier");
    c.reply(crate::chain::Request::Window(2), Ok(json!({"available": true})));
    let r = rows(ui);
    assert_eq!((r[4].0.as_str(), r[4].1.as_str(), r[4].2), ("Plugin view", "not available in this shell yet", 4), "labeled, disabled");
    // Turn on: off turns it on, as the JUCE button.
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    assert_eq!(sent(&mut c.out), [("graph.enable", json!({"node": 2, "enabled": true})), ("graph.commit", json!({}))]);
    // Move later: parts.move and the commit; the actions follow the plugin to its new place.
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    assert_eq!(sent(&mut c.out), [("parts.move", json!({"part": 10, "from": 0, "to": 1})), ("graph.commit", json!({}))]);
    let mut moved = graph();
    moved["parts"][0]["chain"] = json!([1, 2]);
    c.graph(&moved); // as the engine's graph.get then says
    assert_eq!(labels(ui), ["Parameters", "Turn on", "Move earlier", "Remove", "Plugin view"], "now the last: it can move earlier");
    // The parameters open its pages.
    c.act(Action::Move { pane: None, by: -10 });
    assert_eq!(c.act(Action::Confirm(None)), crate::chain::Outcome::Params(2, "Chorus".into()));
    // Remove: the node leaves the part, then the graph with its wires reconnected, then a commit.
    c.act(Action::Move { pane: None, by: 3 });
    c.act(Action::Confirm(None));
    assert_eq!(
        sent(&mut c.out),
        [("parts.removeNode", json!({"part": 10, "node": 2})), ("graph.remove", json!({"node": 2, "userWires": "reconnect"})), ("graph.commit", json!({}))]
    );
    assert_eq!(labels(ui)[0], "+ Insert plugin here", "back at the chain");
    // Cancel with no cursor goes back to the Part screen.
    assert_eq!(c.act(Action::Cancel(None)), crate::chain::Outcome::Back);
    // By touch: a tap on an insert row is a press.
    let mut t = chain_screen(&s);
    t.tap(0);
    assert_eq!(labels(ui), ["Finding plugins..."], "the scanner has not answered");
}

// Inserting: the scanner's plugins listed (instruments and effects, by name); choosing one adds a node
// named uniquely, then inserts it at the place and commits; a refused insert removes the node again
// and says so; a refused add says so.
#[test]
fn chain_inserts_a_plugin() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = chain_screen(&s);
    c.plugins(vec![plugin("Reverb", "org.verb", &["audio-effect"]), plugin("Chorus", "org.chorus", &["audio-effect"]), plugin("Basic Synth", "org.synth", &["instrument"])]);
    assert_eq!(press(&mut c, 2), crate::chain::Outcome::Stay); // the insert row between the two
    assert_eq!(labels(ui), ["Basic Synth", "Chorus", "Reverb"]);
    assert_eq!(rows(ui)[0].1, "Instrument");
    // Chorus is taken: the new one is "Chorus 2".
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    let out = sent(&mut c.out);
    assert_eq!(out, [("graph.add", json!({"name": "Chorus 2", "path": "/p/Chorus.clap", "pluginId": "org.chorus"}))]);
    assert_eq!(labels(ui)[1], "1. Chorus", "back at the chain");
    c.reply(crate::chain::Request::Add(1), Ok(json!({"node": 9})));
    assert_eq!(sent(&mut c.out), [("parts.insert", json!({"part": 10, "index": 1, "node": 9}))]);
    c.reply(crate::chain::Request::Insert(9), Ok(json!({"accepted": true})));
    assert_eq!(sent(&mut c.out), [("graph.commit", json!({}))]);
    // Refused: the node comes out again.
    let refused = c.reply(crate::chain::Request::Insert(9), Err(Error { code: -32602, message: "no room".into() }));
    assert!(refused);
    assert_eq!(sent(&mut c.out), [("graph.remove", json!({"node": 9})), ("graph.commit", json!({}))]);
    assert_eq!(ui.global::<crate::PartState>().get_message().as_str(), "The engine refused: no room");
    assert!(c.reply(crate::chain::Request::Add(0), Err(Error { code: -32001, message: "cannot load".into() })));
    // No plugins found: said.
    let mut e = chain_screen(&s);
    e.plugins(vec![]);
    press(&mut e, 0);
    assert_eq!(labels(ui), ["No plugins found"]);
    // Cancel goes back a layer: the list of plugins to the chain, the chain to the Part screen.
    assert_eq!(e.act(Action::Cancel(None)), crate::chain::Outcome::Stay);
    assert_eq!(labels(ui)[0], "+ Insert plugin here");
    assert_eq!(e.act(Action::Cancel(None)), crate::chain::Outcome::Back);
}

pub fn param_list() -> serde_json::Value {
    json!({"parameters": [
        {"id": 0, "name": "Cutoff", "module": "Filter", "minimum": 0.0, "maximum": 1.0, "stepped": false, "flags": 0, "priority": 2},
        {"id": 1, "name": "Resonance", "module": "Filter", "minimum": 0.0, "maximum": 1.0, "stepped": false, "flags": 0, "priority": 1},
        {"id": 2, "name": "Type", "module": "Filter", "minimum": 0.0, "maximum": 3.0, "stepped": true, "flags": 1, "priority": 0},
        {"id": 3, "name": "Bypass", "module": "Filter", "minimum": 0.0, "maximum": 1.0, "stepped": true, "flags": 1, "priority": 0},
        {"id": 4, "name": "Secret", "module": "Filter", "minimum": 0.0, "maximum": 1.0, "stepped": false, "flags": 4, "priority": 0},
        {"id": 5, "name": "Level", "module": "Output", "minimum": 0.0, "maximum": 1.0, "stepped": false, "flags": 8, "priority": 0},
    ]})
}

fn sounds_screen(s: &Screen) -> crate::sounds::Screen {
    tall(&s.ui);
    let mut c = crate::sounds::Screen::new(&s.ui);
    c.graph(&graph());
    c.open(10);
    c.shown();
    c
}

// The sound chooser: the library's Sounds (a search typed on the keyboard narrows them), the one pressed
// loaded into the part, made a new part, auditioned, and the part saved as a Sound; the refusals said.
#[test]
fn sound_chooser() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = sounds_screen(&s);
    assert_eq!(sent(&mut c.out), [("library.query", json!({"kind": "sound", "limit": 1000}))]);
    c.reply(
        crate::sounds::Request::Query,
        Ok(json!({"items": [{"id": "s1", "name": "Warm Rhodes", "tags": ["keys"], "description": "Soft"}, {"id": "s2", "name": "Fat Bass", "tags": []}], "total": 2})),
    );
    assert_eq!(labels(ui), ["Search", "Warm Rhodes", "Fat Bass"]);
    assert_eq!(rows(ui)[0].1, "all ›");
    assert!(info(ui).contains(&(false, "From “Warm Rhodes”".into())));
    // Search: the keyboard is asked for; the text narrows the query.
    c.act(Action::Move { pane: None, by: 1 });
    assert_eq!(c.act(Action::Confirm(None)), crate::sounds::Outcome::Prompt("Search the Sounds".into(), String::new()));
    c.entered("bass");
    assert_eq!(sent(&mut c.out), [("library.query", json!({"kind": "sound", "limit": 1000, "text": "bass"}))]);
    assert_eq!(rows(ui)[0].1, "bass ›");
    c.reply(crate::sounds::Request::Query, Ok(json!({"items": [{"id": "s2", "name": "Fat Bass", "tags": []}], "total": 2})));
    assert_eq!((labels(ui), info(ui)[3].1.clone()), (vec!["Search".to_string(), "Fat Bass".to_string()], "1 of 2 Sounds".to_string()));
    // Fat Bass: its actions, each a request.
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    assert_eq!(labels(ui)[0], "Load into this part");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Fat Bass", "the sound under the cursor when it was pressed");
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None));
    let out = sent(&mut c.out);
    assert_eq!(out[0], ("parts.loadSound", json!({"part": 10, "id": "s2"})));
    assert_eq!(out[1], ("parts.fromSound", json!({"id": "s2"})));
    assert_eq!(out[2], ("audition.start", json!({"id": "s2", "part": 10})));
    assert_eq!(out[3], ("audition.stop", json!({})));
    assert_eq!(out[4], ("parts.saveSound", json!({"part": 10, "new": false})));
    // Save as new: the keyboard, then the name.
    c.act(Action::Move { pane: None, by: 1 });
    assert_eq!(c.act(Action::Confirm(None)), crate::sounds::Outcome::Prompt("Name of the new Sound".into(), "Keys".into()));
    c.entered("My Keys");
    assert_eq!(sent(&mut c.out), [("parts.saveSound", json!({"part": 10, "name": "My Keys", "new": true}))]);
    // The answers and refusals.
    c.reply(crate::sounds::Request::Do("load"), Ok(json!({"accepted": true, "flagged": ["a wire"]})));
    assert_eq!(ui.global::<crate::PartState>().get_message().as_str(), "Loaded; 1 wires or targets were removed");
    assert!(c.reply(crate::sounds::Request::Do("load"), Err(Error { code: -32009, message: "graph edits are staged".into() })));
    assert_eq!(ui.global::<crate::PartState>().get_message().as_str(), "The engine refused: graph edits are staged");
    // Cancel: the cursor drops, then the actions close, then back.
    assert_eq!(c.act(Action::Cancel(None)), crate::sounds::Outcome::Stay);
    assert_eq!(c.act(Action::Cancel(None)), crate::sounds::Outcome::Stay);
    assert_eq!(labels(ui)[0], "Search");
    assert_eq!(c.act(Action::Cancel(None)), crate::sounds::Outcome::Back);
}

// The three screens draw in the built-in themes and an odd one.
#[test]
fn pages_draw_in_every_theme() {
    fit_to_window();
    let mut s = Screen::new();
    let mut c = chain_screen(&s);
    c.plugins(vec![plugin("Reverb", "org.verb", &["audio-effect"])]);
    c.act(Action::Move { pane: None, by: 1 });
    s.ui.set_screen(5);
    let loaded = all_themes();
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        png(&s, &format!("chain-{}", t.id));
    }
    at_fantom_scale(&mut s, "chain", || c.graph(&graph()));
}
