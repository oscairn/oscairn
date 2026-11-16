// SPDX-License-Identifier: AGPL-3.0-or-later
//! Splits and lanes on the chain screen (chain.rs; the marks are tests/chain.rs, the plain chain tests/pages.rs):
//! inserting a split with its Split and Join plugins, adding and removing lanes and splits, a lane's plugins
//! and settings, a band split's crossovers; each action's requests, the cursor and touch.

use super::chain::{info, message, open, pictures, rows};
use super::*;
use crate::chain::{Outcome, Request, Screen as Chain};
use crate::client::Error;

pub(super) fn sent(out: &mut Vec<crate::chain::Out>) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(out).into_iter().map(|(_, m, p)| (m, p)).collect()
}

pub(super) fn labels(ui: &Shell) -> Vec<String> {
    rows(ui).into_iter().map(|r| r.label.to_string()).collect()
}

fn value_of(ui: &Shell, label: &str) -> String {
    rows(ui).into_iter().find(|r| r.label == label).unwrap_or_else(|| panic!("{label} in {:?}", labels(ui))).value.to_string()
}

/// Presses row `target`: the cursor is put on it from where it is (or shown first).
fn press_at(c: &mut Chain, ui: &Shell, target: usize) -> Outcome {
    let now = match rows(ui).iter().position(|x| x.cursor) {
        Some(at) => at as i32,
        None => {
            c.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor
            0
        }
    };
    if target as i32 != now {
        c.act(Action::Move { pane: None, by: target as i32 - now });
    }
    c.act(Action::Confirm(None))
}

/// Presses the first row with that label.
pub(super) fn press_row(c: &mut Chain, ui: &Shell, label: &str) -> Outcome {
    let at = labels(ui).iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {:?}", labels(ui)));
    press_at(c, ui, at)
}

fn plugin(name: &str, id: &str, features: &[&str]) -> crate::scan::Plugin {
    crate::scan::Plugin { path: format!("/p/{name}.clap"), id: id.into(), name: name.into(), vendor: String::new(), features: features.iter().map(|f| f.to_string()).collect() }
}

fn lane(chain: serde_json::Value, keys: Option<(u32, u32)>, level: f64, pan: f64, mute: bool) -> serde_json::Value {
    let zones = keys.map(|(l, h)| json!([{"keys": [l, h], "velocities": [0, 1], "channels": 65535, "transpose": 0, "curve": 1, "fixedVelocity": 0}])).unwrap_or(json!([]));
    json!({"chain": chain, "zones": zones, "level": level, "pan": pan, "mute": mute, "solo": false})
}

/// A part with a Chorus, then a band split of three lanes (the Rhodes on keys 36 to 60 at 80%; a Pad and a
/// split of layers holding an Echo, muted, panned to the left; no plugin), then a Reverb. The band split's
/// Split plugin is node 7. `kind` and `lanes` change the band split.
pub(super) fn split_graph(kind: &str, lanes: usize) -> serde_json::Value {
    let node = |id: u32, name: &str, ins: bool, outs: bool, notes: bool| {
        json!({"id": id, "name": name, "plugin": {"name": name}, "audioInputs": if ins { json!([{"channels": 2}]) } else { json!([]) },
               "audioOutputs": if outs { json!([{"channels": 2}]) } else { json!([]) }, "noteInputs": u32::from(notes), "noteOutputs": 0, "enabled": true})
    };
    let all = [
        lane(json!([1]), Some((36, 60)), 0.8, 0.0, false),
        lane(json!([4, {"split": 6}]), None, 1.0, -0.25, true),
        lane(json!([]), None, 1.0, 0.0, false),
        lane(json!([]), None, 1.0, 0.0, false),
    ];
    json!({
        "nodes": [node(1, "Rhodes", false, true, true), node(2, "Chorus", true, true, false), node(3, "Reverb", true, true, false), node(4, "Pad", false, true, true),
                  node(7, "Split", true, true, false), node(9, "Echo", true, true, false)],
        "parts": [{"id": 10, "name": "Keys", "level": 0.5, "pan": 0.0, "mute": false, "solo": false, "chain": [2, {"split": 5}, 3], "bus": 0, "sends": [],
                   "splits": [
                       {"id": 5, "kind": kind, "splitter": 7, "joiner": 0, "lanes": all[..lanes]},
                       {"id": 6, "kind": "copy", "splitter": 0, "joiner": 0, "lanes": [lane(json!([9]), None, 1.0, 0.0, false)]}],
                   "from": {"sound": "s1", "name": "Warm Rhodes", "modified": false}, "input": {"kind": "notes", "port": 0, "devices": [], "zones": []}}],
        "wires": [], "scenes": [], "scene": 0,
    })
}

// D7: an insert row of the part's chain offers the four kinds of split before the plugins (and a lane's does
// not); a copy is one request; the others add the Split plugin (and the Join plugin for left / right and
// mid / side) first and then the split with their nodes; refused, those nodes come out again and it is said;
// without the plugins the three kinds are listed and cannot be pressed; Split and Join are not plugins to pick.
#[test]
fn a_split_is_inserted_with_its_split_and_join_plugins() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &super::chain::marks_graph());
    c.plugins(vec![plugin("Reverb", "org.verb", &["audio-effect"]), plugin("Split", "org.oscairn.split", &["audio-effect"]), plugin("Join", "org.oscairn.join", &["audio-effect"])]);
    sent(&mut c.out);
    // A layer split needs no plugin: one request, and the commit when it answers.
    press_at(&mut c, ui, 0);
    assert_eq!(labels(ui), ["Split here: layers", "Split here: left / right", "Split here: mid / side", "Split here: frequency bands", "Reverb"], "Split and Join are not boxes of their own");
    assert!(rows(ui)[..4].iter().all(|r| r.kind == 2));
    press_at(&mut c, ui, 0);
    assert_eq!(sent(&mut c.out), [("parts.addSplit", json!({"part": 10, "index": 0, "kind": "copy"}))]);
    assert!(!c.reply(Request::NewSplit, Ok(json!({"split": 1}))));
    assert_eq!(sent(&mut c.out), [("graph.commit", json!({}))]);
    // Left / right: the Split plugin, the Join plugin, then the split with both, at the third place.
    press_at(&mut c, ui, 4);
    assert!(sent(&mut c.out).is_empty());
    press_row(&mut c, ui, "Split here: left / right");
    assert_eq!(sent(&mut c.out), [("graph.add", json!({"name": "Split", "path": "/p/Split.clap", "pluginId": "org.oscairn.split"}))]);
    assert_eq!(labels(ui)[0], "+ Insert plugin here", "back at the chain");
    c.reply(Request::Splitter, Ok(json!({"node": 20})));
    assert_eq!(sent(&mut c.out), [("graph.add", json!({"name": "Join", "path": "/p/Join.clap", "pluginId": "org.oscairn.join"}))]);
    c.reply(Request::Joiner, Ok(json!({"node": 21})));
    assert_eq!(sent(&mut c.out), [("parts.addSplit", json!({"part": 10, "index": 2, "kind": "leftRight", "splitter": 20, "joiner": 21}))]);
    // Refused: both nodes come out again, the commit follows, and it is said.
    assert!(c.reply(Request::NewSplit, Err(Error { code: -32602, message: "no room".into() })));
    assert_eq!(sent(&mut c.out), [("graph.remove", json!({"node": 20})), ("graph.remove", json!({"node": 21})), ("graph.commit", json!({}))]);
    assert_eq!(message(ui), "The engine refused: no room");
    // Frequency bands have no Join node; mid / side have one, and a refused add of it takes the Split node out.
    press_at(&mut c, ui, 0);
    press_row(&mut c, ui, "Split here: frequency bands");
    sent(&mut c.out);
    c.reply(Request::Splitter, Ok(json!({"node": 30})));
    assert_eq!(sent(&mut c.out), [("parts.addSplit", json!({"part": 10, "index": 0, "kind": "bands", "splitter": 30}))]);
    assert!(!c.reply(Request::NewSplit, Ok(json!({"split": 2}))));
    sent(&mut c.out);
    press_at(&mut c, ui, 0);
    press_row(&mut c, ui, "Split here: mid / side");
    sent(&mut c.out);
    c.reply(Request::Splitter, Ok(json!({"node": 31})));
    sent(&mut c.out);
    assert!(c.reply(Request::Joiner, Err(Error { code: -32001, message: "cannot load".into() })));
    assert_eq!(sent(&mut c.out), [("graph.remove", json!({"node": 31})), ("graph.commit", json!({}))]);
    // Without the plugins the other three kinds are listed and cannot be pressed.
    let mut bare = open(&s, &super::chain::marks_graph());
    bare.plugins(vec![plugin("Reverb", "org.verb", &["audio-effect"])]);
    press_at(&mut bare, ui, 0);
    let r = rows(ui);
    assert_eq!(r.iter().map(|r| r.kind).take(4).collect::<Vec<_>>(), [2, 4, 4, 4]);
    assert_eq!(r[1].value.as_str(), "needs the Split and Join plugins");
    bare.act(Action::Move { pane: None, by: 1 });
    bare.act(Action::Move { pane: None, by: 1 }); // left / right
    bare.act(Action::Confirm(None));
    assert!(sent(&mut bare.out).is_empty(), "a disabled kind sends nothing");
}

// The chain lists a split as a row (its kind and lanes) that opens its lanes; the panel draws its lanes under
// it; a lane says its name, its keys when not all, its level, muted or solo, and "Dry" when a copy's lane has no
// plugin; a lane nobody can hear (muted, or another lane solo) is faint; Cancel goes back a layer at a time.
#[test]
fn a_split_lists_its_lanes_and_a_lane_its_plugins_and_settings() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &split_graph("bands", 3));
    assert_eq!(labels(ui), ["+ Insert plugin here", "1. Chorus", "+ Insert plugin here", "2. Split", "+ Insert plugin here", "3. Reverb", "+ Insert plugin here"]);
    assert_eq!(value_of(ui, "2. Split"), "frequency bands · 3 lanes ›");
    let lines: Vec<String> = info(ui).iter().map(|l| l.text.to_string()).collect();
    assert_eq!(
        lines,
        ["Chain", "In", "1. Chorus", "2. Split: frequency bands", "    Band 1: Rhodes", "    Band 2: Pad", "        Split", "    Band 3: Dry", "3. Reverb", "Out"],
        "the lanes under the split, a lane without plugins Dry, a split inside a lane listed"
    );
    // The split: its lanes first, the crossovers (asked of the engine), Add band, Remove split.
    press_row(&mut c, ui, "2. Split");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Split: frequency bands");
    assert_eq!(labels(ui), ["Band 1", "Band 2", "Band 3", "Crossover 1", "Crossover 2", "Add band", "Remove split"]);
    assert_eq!(sent(&mut c.out), [("params.get", json!({"values": [{"node": 7, "param": 3}, {"node": 7, "param": 4}]}))], "the crossovers are asked of the engine");
    assert_eq!(value_of(ui, "Band 1"), "C2–C4 · 80% ›");
    assert_eq!(value_of(ui, "Band 2"), "100% · muted ›");
    assert_eq!(value_of(ui, "Band 3"), "100% ›");
    assert_eq!(rows(ui).iter().map(|r| r.off).collect::<Vec<_>>()[..3], [false, true, false], "the muted lane is faint");
    // A lane: its plugins as the chain's, then its settings.
    press_row(&mut c, ui, "Band 2");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Band 2");
    assert_eq!(
        labels(ui),
        ["+ Insert plugin here", "1. Pad", "+ Insert plugin here", "2. Split", "+ Insert plugin here", "Lowest key", "Highest key", "Lane level", "Lane pan", "Mute", "Solo", "Remove lane"]
    );
    assert_eq!((value_of(ui, "2. Split").as_str(), rows(ui)[3].kind), ("inside a lane: not editable here", 4));
    assert_eq!((value_of(ui, "Lowest key").as_str(), value_of(ui, "Highest key").as_str(), value_of(ui, "Lane level").as_str(), value_of(ui, "Lane pan").as_str()), ("C-1", "G9", "100%", "L 25"));
    assert_eq!((value_of(ui, "Mute").as_str(), value_of(ui, "Solo").as_str()), ("On", "Off"));
    // Cancel: the cursor drops, then the lane, the split and the chain close, then the Part screen is back.
    c.act(Action::Move { pane: None, by: 1 });
    assert_eq!(c.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Band 2", "the cursor first");
    c.act(Action::Cancel(None));
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Split: frequency bands");
    c.act(Action::Cancel(None));
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Insert effects");
    assert_eq!(c.act(Action::Cancel(None)), Outcome::Back);
    // A copy's lanes are Lane n, "Dry" when empty; the other kinds name theirs.
    let mut copy = open(&s, &split_graph("copy", 3));
    press_row(&mut copy, ui, "2. Split");
    assert_eq!(rows(ui).iter().take(3).map(|r| r.value.to_string()).collect::<Vec<_>>(), ["C2–C4 · 80% ›", "100% · muted ›", "Dry · 100% ›"]);
    assert_eq!(labels(ui)[..3], ["Lane 1", "Lane 2", "Lane 3"]);
    let mut lr = open(&s, &split_graph("leftRight", 2));
    press_row(&mut lr, ui, "2. Split");
    assert_eq!(labels(ui)[..2], ["Left", "Right"]);
    assert_eq!(labels(ui), ["Left", "Right", "Remove split"], "no crossovers, nothing to add");
}

// D8: Add lane (a copy) and Add band (fewer than four) are one request and a commit; Remove lane (a copy, or a
// band split of more than two lanes) removes the lane's nodes, those in a split inside it too, with the wires
// reconnected; Remove split keeps the first lane's plugins and removes those of every other lane; neither asks
// first (the reference does not: Undo takes it back). A lane's plugins insert, move and go as the chain's.
#[test]
fn lanes_and_splits_are_added_and_removed() {
    let s = Screen::new();
    let ui = &s.ui;
    // Add band: a band split of three lanes; at four it is not offered; left / right and mid / side never.
    let mut c = open(&s, &split_graph("bands", 3));
    press_row(&mut c, ui, "2. Split");
    sent(&mut c.out);
    press_row(&mut c, ui, "Add band");
    assert_eq!(sent(&mut c.out), [("parts.addLane", json!({"part": 10, "split": 5})), ("graph.commit", json!({}))]);
    let four = open(&s, &split_graph("bands", 4));
    let mut four = four;
    press_row(&mut four, ui, "2. Split");
    assert!(!labels(ui).iter().any(|l| l.starts_with("Add")), "four bands are the most");
    assert_eq!(labels(ui).iter().filter(|l| l.starts_with("Crossover")).count(), 3);
    let mut copy = open(&s, &split_graph("copy", 3));
    press_row(&mut copy, ui, "2. Split");
    assert!(labels(ui).contains(&"Add lane".to_string()), "a copy takes any number of layers");
    // Remove lane: the Pad and the Echo in the split inside it leave the graph, wires reconnected.
    let mut c = open(&s, &split_graph("bands", 3));
    press_row(&mut c, ui, "2. Split");
    press_row(&mut c, ui, "Band 2");
    sent(&mut c.out);
    press_row(&mut c, ui, "Remove lane");
    assert_eq!(
        sent(&mut c.out),
        [
            ("parts.removeLane", json!({"part": 10, "split": 5, "lane": 1})),
            ("graph.remove", json!({"node": 4, "userWires": "reconnect"})),
            ("graph.remove", json!({"node": 9, "userWires": "reconnect"})),
            ("graph.commit", json!({})),
        ]
    );
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Split: frequency bands", "back at the split");
    // Two bands are the least: no Remove lane. Left / right neither.
    let mut two = open(&s, &split_graph("bands", 2));
    press_row(&mut two, ui, "2. Split");
    press_row(&mut two, ui, "Band 1");
    assert!(!labels(ui).contains(&"Remove lane".to_string()));
    // Remove split keeps the first lane's plugins and removes the others'.
    let mut c = open(&s, &split_graph("bands", 3));
    press_row(&mut c, ui, "2. Split");
    sent(&mut c.out);
    press_row(&mut c, ui, "Remove split");
    assert_eq!(
        sent(&mut c.out),
        [
            ("parts.removeSplit", json!({"part": 10, "split": 5})),
            ("graph.remove", json!({"node": 4, "userWires": "reconnect"})),
            ("graph.remove", json!({"node": 9, "userWires": "reconnect"})),
            ("graph.commit", json!({})),
        ]
    );
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Insert effects", "back at the chain");
    // A lane's plugins: inserting names the lane, moving stays in the lane, removing is a plain remove.
    let mut c = open(&s, &split_graph("bands", 3));
    c.plugins(vec![plugin("Reverb", "org.verb", &["audio-effect"])]);
    press_row(&mut c, ui, "2. Split");
    press_row(&mut c, ui, "Band 2");
    sent(&mut c.out);
    press_at(&mut c, ui, 2); // between the Pad and the split inside the lane
    assert_eq!(labels(ui), ["Reverb"], "a lane offers no split of its own");
    press_at(&mut c, ui, 0);
    assert_eq!(sent(&mut c.out), [("graph.add", json!({"name": "Reverb 2", "path": "/p/Reverb.clap", "pluginId": "org.verb"}))]);
    c.reply(Request::Add(1), Ok(json!({"node": 40})));
    assert_eq!(sent(&mut c.out), [("parts.insert", json!({"part": 10, "index": 1, "node": 40, "split": 5, "lane": 1}))]);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Band 2", "back in the lane");
    // The Pad's actions: it is the first of two, so it can move later, not earlier.
    press_row(&mut c, ui, "1. Pad");
    assert_eq!(sent(&mut c.out), [("window.info", json!({"node": 4}))]);
    assert_eq!(labels(ui), ["Parameters", "Turn off", "Presets...", "Move later", "Remove", "Routing...", "Scene controls..."]);
    press_row(&mut c, ui, "Move later");
    assert_eq!(sent(&mut c.out), [("parts.move", json!({"part": 10, "from": 0, "to": 1, "split": 5, "lane": 1})), ("graph.commit", json!({}))]);
    // The engine's graph now has it moved; the Pad is the second.
    let mut moved = split_graph("bands", 3);
    moved["parts"][0]["splits"][0]["lanes"][1]["chain"] = json!([{"split": 6}, 4]);
    c.graph(&moved);
    assert_eq!(labels(ui), ["Parameters", "Turn off", "Presets...", "Move earlier", "Remove", "Routing...", "Scene controls..."], "the last of the lane cannot move later");
    press_row(&mut c, ui, "Remove");
    assert_eq!(
        sent(&mut c.out),
        [("parts.removeNode", json!({"part": 10, "node": 4})), ("graph.remove", json!({"node": 4, "userWires": "reconnect"})), ("graph.commit", json!({}))]
    );
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Band 2", "back in the lane");
    // A refusal is said.
    assert!(c.reply(Request::Edit, Err(Error { code: -32602, message: "unknown lane".into() })));
    assert_eq!(message(ui), "The engine refused: unknown lane");
}

// D9: a lane's keys, level, pan, mute and solo are `parts.lane` with the whole lane, one request and a commit
// each; the dial changes a value after a press, - and + by touch; the highest key never goes under the lowest;
// a zone that passes every note is no zone; the level and pan stay in range; a band split's crossovers are read
// with `params.get`, shown in Hz, and set with `params.set` a sixth of an octave a step (finer in hertz at the low
// end), with no commit.
#[test]
fn a_lanes_settings_are_sent_whole() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &split_graph("bands", 3));
    press_row(&mut c, ui, "2. Split");
    press_row(&mut c, ui, "Band 1");
    sent(&mut c.out);
    let whole = |keys: serde_json::Value, level: f64, pan: f64, mute: bool, solo: bool| {
        let zones = if keys.is_null() { json!([]) } else { json!([{"keys": keys, "velocities": [0, 1], "channels": 65535, "transpose": 0, "curve": 1, "fixedVelocity": 0}]) };
        [("parts.lane", json!({"part": 10, "split": 5, "lane": 0, "zones": zones, "level": level, "pan": pan, "mute": mute, "solo": solo})), ("graph.commit", json!({}))]
    };
    assert_eq!((value_of(ui, "Lowest key").as_str(), value_of(ui, "Highest key").as_str()), ("C2", "C4"));
    // Lowest key: a press lets the dial change it; the highest follows it up when it is passed.
    press_row(&mut c, ui, "Lowest key");
    assert!(rows(ui).iter().any(|r| r.editing && r.label == "Lowest key"));
    c.act(Action::Move { pane: None, by: 2 });
    assert_eq!(sent(&mut c.out), whole(json!([38, 60]), 0.8, 0., false, false));
    c.act(Action::Move { pane: None, by: 30 });
    assert_eq!(sent(&mut c.out), whole(json!([68, 68]), 0.8, 0., false, false), "the highest key follows");
    assert_eq!((value_of(ui, "Lowest key").as_str(), value_of(ui, "Highest key").as_str()), ("G♯4", "G♯4"));
    // Highest key: never under the lowest; a step that changes nothing sends nothing.
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Highest key");
    c.act(Action::Move { pane: None, by: -100 });
    assert!(sent(&mut c.out).is_empty());
    c.act(Action::Move { pane: None, by: 5 });
    assert_eq!(sent(&mut c.out), whole(json!([68, 73]), 0.8, 0., false, false));
    // A range of every key is no zone at all.
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Lowest key");
    c.act(Action::Move { pane: None, by: -200 });
    assert_eq!(sent(&mut c.out), whole(json!([0, 73]), 0.8, 0., false, false));
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Highest key");
    c.act(Action::Move { pane: None, by: 100 });
    assert_eq!(sent(&mut c.out), whole(json!(null), 0.8, 0., false, false), "keys 0 to 127 clear the zones, as setLaneKeys does");
    // Level and pan by the dial, kept in range.
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Lane level");
    c.act(Action::Move { pane: None, by: 1 });
    assert_eq!(sent(&mut c.out), whole(json!(null), 0.82, 0., false, false));
    c.act(Action::Move { pane: None, by: 50 });
    assert_eq!(sent(&mut c.out), whole(json!(null), 1.0, 0., false, false));
    assert_eq!(value_of(ui, "Lane level"), "100%");
    c.act(Action::Move { pane: None, by: 1 });
    assert!(sent(&mut c.out).is_empty(), "no step past the end");
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Lane pan");
    c.act(Action::Move { pane: None, by: -1 });
    assert_eq!(sent(&mut c.out), whole(json!(null), 1.0, -0.05, false, false));
    assert_eq!(value_of(ui, "Lane pan"), "L 5");
    c.act(Action::Move { pane: None, by: -100 });
    assert_eq!(sent(&mut c.out), whole(json!(null), 1.0, -1.0, false, false));
    // Mute and solo are pressed, not changed by the dial.
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Mute");
    assert_eq!(sent(&mut c.out), whole(json!(null), 1.0, -1.0, true, false));
    assert!(rows(ui).iter().all(|r| !r.editing));
    press_row(&mut c, ui, "Solo");
    assert_eq!(sent(&mut c.out), whole(json!(null), 1.0, -1.0, true, true));
    assert_eq!((value_of(ui, "Mute").as_str(), value_of(ui, "Solo").as_str()), ("On", "On"));
    // By touch: a tap on a switch is a press; - and + step a value.
    let row_of = |label: &str| labels(ui).iter().position(|l| l == label).unwrap() as i32;
    c.tap(row_of("Mute"));
    assert_eq!(sent(&mut c.out), whole(json!(null), 1.0, -1.0, false, true));
    c.step_row(row_of("Lane pan"), 1);
    assert_eq!(sent(&mut c.out), whole(json!(null), 1.0, -0.95, false, true));
    c.step_row(row_of("Lowest key"), 1);
    assert_eq!(sent(&mut c.out), whole(json!([1, 127]), 1.0, -0.95, false, true));
    // A lane another lane's solo silences is faint in the split.
    c.act(Action::Cancel(None));
    c.act(Action::Cancel(None));
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Split: frequency bands");
    c.graph(&{
        let mut g = split_graph("bands", 3);
        g["parts"][0]["splits"][0]["lanes"][0]["solo"] = json!(true);
        g
    });
    assert_eq!(rows(ui).iter().map(|r| r.off).collect::<Vec<_>>()[..3], [false, true, true], "one lane solo silences the rest");
    assert_eq!(value_of(ui, "Band 1"), "C2–C4 · 80% · solo ›");
}

// A band split's crossovers: read with params.get and shown in Hz, set with params.set a sixth of an octave a step
// (a step is finer at the low end), with no commit after it, by the dial after a press and by - and +.
// D9: a lane's key range as the reference's strip over the whole keyboard: an end dragged, the middle moving the whole range,
// anywhere else bringing the nearer end there; one edit when the finger lifts (the whole lane, as the dial sends it), a
// double tap the whole keyboard; the strip follows the dial's rows and shows only on a lane.
#[test]
fn a_lanes_keys_are_dragged_on_the_strip() {
    use crate::StripState;
    let s = Screen::new();
    let ui = &s.ui;
    let strip = ui.global::<StripState>();
    crate::keystrip::install(ui);
    let mut c = open(&s, &split_graph("bands", 3));
    assert!(!strip.get_shown(), "not on the chain's list");
    press_row(&mut c, ui, "2. Split");
    assert!(!strip.get_shown(), "not on a split");
    press_row(&mut c, ui, "Band 1");
    assert!(strip.get_shown());
    assert_eq!((strip.get_low(), strip.get_high(), strip.get_range().as_str()), (36, 60, "C2 – C4"));
    sent(&mut c.out);
    let width = 750.; // ten pixels a white key
    let whole = |keys: serde_json::Value| {
        let zones = if keys.is_null() { json!([]) } else { json!([{"keys": keys, "velocities": [0, 1], "channels": 65535, "transpose": 0, "curve": 1, "fixedVelocity": 0}]) };
        [("parts.lane", json!({"part": 10, "split": 5, "lane": 0, "zones": zones, "level": 0.8, "pan": 0.0, "mute": false, "solo": false})), ("graph.commit", json!({}))]
    };
    // A white key's left edge, as a share of the width: the reference's keyBounds.
    let white = |n: f64| n * 10. / width;
    // The low end (C2, the 15th white key) taken and dragged to the 10th white key: nothing sent until the finger lifts.
    c.strip_down(white(15.) + 0.003, width);
    c.strip_move(white(12.) + 0.003);
    c.strip_move(white(10.) + 0.003);
    assert!(sent(&mut c.out).is_empty(), "a drag is one edit, sent at the end");
    assert_eq!(strip.get_range().as_str(), "F0 – C4", "the strip follows the finger");
    c.strip_up();
    let got = sent(&mut c.out);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].1["zones"][0]["keys"], json!([17, 60]), "{got:?}");
    // The middle moves the whole range as far as the finger.
    let (low, high) = (17, 60);
    let mid = |c: &Chain| {
        let _ = c;
        (white(9.) + white(35.)) / 2.
    };
    c.strip_down(mid(&c), width);
    c.strip_move(mid(&c) + white(2.));
    c.strip_up();
    let got = sent(&mut c.out);
    assert_eq!(got[0].1["zones"][0]["keys"].as_array().map(|k| k[1].as_i64().unwrap() - k[0].as_i64().unwrap()), Some(high - low), "the width is kept: {got:?}");
    // Past the high end: the high end goes there. A finger that does not move changes nothing.
    c.strip_down(white(70.), width);
    c.strip_up();
    let got = sent(&mut c.out);
    assert!(got.is_empty() || got[0].1["zones"][0]["keys"][1].as_i64().unwrap() > 100, "{got:?}");
    sent(&mut c.out);
    // A double tap: the whole keyboard, which is no zone at all.
    c.strip_reset();
    assert_eq!(sent(&mut c.out), whole(json!(null)));
    // The dial's row says which end the dial changes.
    c.act(Action::End(-1));
    press_row(&mut c, ui, "Lowest key");
    assert_eq!(strip.get_end(), 1);
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Highest key");
    assert_eq!(strip.get_end(), 2);
    // Back to the split: the strip goes.
    c.back();
    c.back();
    assert!(!strip.get_shown());
}

#[test]
fn crossovers_are_read_and_set_in_hertz() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &split_graph("bands", 3));
    press_row(&mut c, ui, "2. Split");
    sent(&mut c.out);
    assert_eq!(value_of(ui, "Crossover 1"), "1000 Hz", "the middle of the slider until the engine says");
    c.reply(Request::Crossovers, Ok(json!({"values": [{"node": 7, "param": 3, "value": 500.0}, {"node": 7, "param": 4, "value": 4000.0}]})));
    assert_eq!((value_of(ui, "Crossover 1").as_str(), value_of(ui, "Crossover 2").as_str()), ("500 Hz", "4000 Hz"));
    press_row(&mut c, ui, "Crossover 1");
    c.act(Action::Move { pane: None, by: 1 });
    let out = sent(&mut c.out);
    assert_eq!(out.len(), 1, "params.set alone: no commit");
    let up = out[0].1["values"][0]["value"].as_f64().unwrap();
    assert_eq!((out[0].0, out[0].1["values"][0]["node"].clone(), out[0].1["values"][0]["param"].clone()), ("params.set", json!(7), json!(3)));
    assert!(up > 500. && up < 700., "a step up: {up}");
    assert_eq!(value_of(ui, "Crossover 1"), format!("{up:.0} Hz"));
    c.act(Action::Move { pane: None, by: -1 });
    let down = sent(&mut c.out)[0].1["values"][0]["value"].as_f64().unwrap();
    assert!(down <= 500. && down > 300., "a step back: {down}");
    // The ends: a step past them sends nothing.
    c.act(Action::Move { pane: None, by: -1000 });
    let least = sent(&mut c.out).pop().unwrap().1["values"][0]["value"].as_f64().unwrap();
    assert_eq!(least, 20.);
    c.act(Action::Move { pane: None, by: -1 });
    assert!(sent(&mut c.out).is_empty());
    c.step_row(3, 1);
    assert!(sent(&mut c.out)[0].1["values"][0]["value"].as_f64().unwrap() > 20., "by touch: + from the least goes up");
    // A graph change asks again while the split shows.
    c.graph(&split_graph("bands", 3));
    assert_eq!(sent(&mut c.out)[0].0, "params.get");
}

// A split, lane or plugin that is gone (an undo, another editor) sends the screen back to what is still there.
#[test]
fn a_split_that_goes_takes_the_screen_back_to_the_chain() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &split_graph("bands", 3));
    press_row(&mut c, ui, "2. Split");
    press_row(&mut c, ui, "Band 3");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Band 3");
    c.graph(&split_graph("bands", 2));
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Split: frequency bands", "the lane is gone: its split");
    let mut gone = split_graph("bands", 2);
    gone["parts"][0]["chain"] = json!([2, 3]);
    gone["parts"][0]["splits"] = json!([]);
    c.graph(&gone);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Insert effects", "the split is gone: the chain");
}

#[test]
fn splits_and_lanes_are_drawn() {
    pictures("chain-split", &split_graph("bands", 3), &|c| {
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 3 });
        c.act(Action::Confirm(None));
    });
    pictures("chain-lane", &split_graph("bands", 3), &|c| {
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 3 });
        c.act(Action::Confirm(None));
        c.reply(Request::Crossovers, Ok(json!({"values": [{"node": 7, "param": 3, "value": 500.0}, {"node": 7, "param": 4, "value": 4000.0}]})));
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Confirm(None));
    });
    // The same lane at its settings, a value being changed.
    pictures("chain-lane-settings", &split_graph("bands", 3), &|c| {
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 3 });
        c.act(Action::Confirm(None));
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Confirm(None));
        c.act(Action::End(1)); // the first move shows the cursor, the second goes to the end
        c.act(Action::End(1));
        c.act(Action::Move { pane: None, by: -3 }); // Lane pan
        c.act(Action::Confirm(None));
    });
}
