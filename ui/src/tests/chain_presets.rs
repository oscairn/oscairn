// SPDX-License-Identifier: AGPL-3.0-or-later
//! A plugin's presets on the chain screen (chain.rs; PluginBrowser.cpp's "Reload with preset"): the plugin's
//! actions list them (`presets.list`), choosing one reloads the plugin with it (`graph.replace` with the
//! plugin's own file and ID and the preset's URI, no commit: the request commits itself), in the part's chain
//! and in a lane; the engine's "ask again" and refusals are said.

use super::chain::{info, message, open, pictures, rows};
use super::chain_splits::{labels, press_row, sent, split_graph};
use super::*;
use crate::chain::{Outcome, Request};
use crate::client::Error;

/// The graph with the Rhodes' plugin as the engine words it: its name, ID and file.
fn with_plugin(mut graph: serde_json::Value) -> serde_json::Value {
    graph["nodes"][0]["plugin"] = json!({"name": "Surge XT", "id": "org.surge", "path": "/plugins/Surge.clap"});
    graph
}

fn presets() -> serde_json::Value {
    json!({"presets": [{"uri": "clap-preset:warm", "name": "Warm"}, {"uri": "clap-preset:bright", "name": "Bright"}]})
}

// The plugin's actions have Presets...; it asks the engine and lists them by name; the hint of the reference is
// in the panel; a press reloads the plugin with that one and the actions are back; the answers are said.
#[test]
fn a_plugin_is_reloaded_with_a_preset() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &with_plugin(super::chain::marks_graph()));
    // The Rhodes (the second box of the chain).
    press_row(&mut c, ui, "2. Rhodes");
    sent(&mut c.out);
    assert!(labels(ui).contains(&"Presets...".to_string()));
    press_row(&mut c, ui, "Presets...");
    assert_eq!(sent(&mut c.out), [("presets.list", json!({"node": 1}))]);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Rhodes: presets");
    assert_eq!(labels(ui), ["No presets listed"], "until the engine answers");
    assert!(!c.reply(Request::Presets(1), Ok(presets())));
    assert_eq!(labels(ui), ["Warm", "Bright"]);
    assert!(info(ui).iter().any(|l| l.text.starts_with("Replacing stops its notes and tails")), "the reference's hint, in short lines");
    // Bright: graph.replace with the plugin's own file and ID and the preset's URI, and nothing after it.
    press_row(&mut c, ui, "Bright");
    assert_eq!(sent(&mut c.out), [("graph.replace", json!({"node": 1, "path": "/plugins/Surge.clap", "pluginId": "org.surge", "preset": "clap-preset:bright"}))]);
    assert_eq!(message(ui), "Reloading with “Bright”...");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Rhodes", "the actions are back");
    assert!(!c.reply(Request::Replace, Ok(json!({"revision": 4}))));
    assert_eq!(message(ui), "Reloaded with “Bright”");
    // A refusal (graph edits staged, a preset the plugin cannot load) is said and the graph asked again.
    press_row(&mut c, ui, "Presets...");
    sent(&mut c.out);
    c.reply(Request::Presets(1), Ok(presets()));
    press_row(&mut c, ui, "Warm");
    sent(&mut c.out);
    assert!(c.reply(Request::Replace, Err(Error { code: -32005, message: "Preset not prepared: no such preset".into() })));
    assert_eq!(message(ui), "The engine refused: Preset not prepared: no such preset");
    // Cancel from the presets goes back to the actions, then the chain.
    press_row(&mut c, ui, "Presets...");
    assert_eq!(c.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Rhodes");
    assert_eq!(c.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Insert effects");
}

// The engine never waits for the scan helper: presets not ready are "ask again", said where the list would be,
// with a row that asks again; a plugin without presets says so; an answer for another plugin is not shown.
#[test]
fn presets_not_ready_are_asked_for_again() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &with_plugin(super::chain::marks_graph()));
    press_row(&mut c, ui, "2. Rhodes");
    press_row(&mut c, ui, "Presets...");
    sent(&mut c.out);
    assert!(!c.reply(Request::Presets(1), Err(Error { code: -32004, message: "Presets are being listed; ask again".into() })));
    assert_eq!(labels(ui), ["Presets are being listed; ask again", "Ask again"]);
    assert_eq!(rows(ui).iter().map(|r| r.kind).collect::<Vec<_>>(), [4, 2]);
    press_row(&mut c, ui, "Ask again");
    assert_eq!(sent(&mut c.out), [("presets.list", json!({"node": 1}))]);
    c.reply(Request::Presets(3), Ok(presets()));
    assert_eq!(labels(ui), ["No presets listed"], "an answer for another plugin is not this one's");
    c.reply(Request::Presets(1), Ok(json!({"presets": []})));
    assert_eq!(labels(ui), ["No presets listed"]);
    // By touch: a tap on Presets... and on a preset are presses.
    let mut t = open(&s, &with_plugin(super::chain::marks_graph()));
    let row_of = |name: &str| labels(ui).iter().position(|l| l == name).unwrap() as i32;
    t.tap(row_of("2. Rhodes"));
    t.tap(row_of("Presets..."));
    assert_eq!(sent(&mut t.out).last().unwrap(), &("presets.list", json!({"node": 1})));
    t.reply(Request::Presets(1), Ok(presets()));
    t.tap(0);
    assert_eq!(sent(&mut t.out)[0].0, "graph.replace");
}

// A plugin in a lane has its presets too: the same request for its own node.
#[test]
fn a_plugin_in_a_lane_is_reloaded_with_a_preset() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &with_plugin(split_graph("bands", 3)));
    press_row(&mut c, ui, "2. Split");
    press_row(&mut c, ui, "Band 1");
    press_row(&mut c, ui, "1. Rhodes");
    sent(&mut c.out);
    press_row(&mut c, ui, "Presets...");
    assert_eq!(sent(&mut c.out), [("presets.list", json!({"node": 1}))]);
    c.reply(Request::Presets(1), Ok(presets()));
    press_row(&mut c, ui, "Warm");
    assert_eq!(sent(&mut c.out)[0].1["preset"], "clap-preset:warm");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Rhodes", "its actions, in its lane");
    c.act(Action::Cancel(None));
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Band 1", "then the lane");
}

#[test]
fn the_presets_are_drawn() {
    pictures("chain-presets", &with_plugin(super::chain::marks_graph()), &|c| {
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 3 });
        c.act(Action::Confirm(None));
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 2 });
        c.act(Action::Confirm(None));
        c.reply(Request::Presets(1), Ok(presets()));
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 1 });
    });
}
