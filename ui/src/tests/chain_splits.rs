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
        ["Chain", "IN", "Any device · all keys", "1. Chorus", "2. Split: frequency bands", "    Band 1: Rhodes", "    Band 2: Pad", "        Split", "    Band 3: Dry", "3. Reverb", "OUT", "Main"],
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

// At rest, the chain and its splits and lanes ask the engine nothing and draw nothing over many
// ticks (docs/rust-ui.md, "Nothing is drawn at rest"), as the Part and Settings idle tests hold:
// a band split asks its crossovers once when it opens, and nothing follows while nothing changes.
#[test]
fn splits_and_lanes_at_rest_cost_no_request_and_no_frame() {
    let mut s = Screen::new();
    let mut c = open(&s, &split_graph("bands", 3));
    s.ui.set_screen(5); // the chain shown, so the frames below are its own
    // The chain itself: nothing asked, nothing drawn.
    assert!(sent(&mut c.out).is_empty(), "the chain at rest asked for something");
    s.frame();
    assert_eq!(s.frame(), None, "the chain at rest drew a frame");
    // The split asks its crossovers once when it opens, then nothing at rest.
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Move { pane: None, by: 3 }); // 2. Split
    c.act(Action::Confirm(None));
    assert_eq!(sent(&mut c.out).first().map(|r| r.0), Some("params.get"), "the crossovers are asked when the split opens");
    c.reply(Request::Crossovers, Ok(json!({"values": [{"node": 7, "param": 3, "value": 500.0}, {"node": 7, "param": 4, "value": 4000.0}]})));
    s.frame();
    for _ in 0..40 {
        assert!(sent(&mut c.out).is_empty(), "the split at rest asked for something");
        assert_eq!(s.frame(), None, "the split at rest drew a frame");
    }
    // A lane: no crossovers, nothing at rest either.
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None)); // Band 1
    assert!(s.frame().is_some(), "the lane is drawn once when it opens");
    for _ in 0..40 {
        assert!(sent(&mut c.out).is_empty(), "the lane at rest asked for something");
        assert_eq!(s.frame(), None, "the lane at rest drew a frame");
    }
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

// W06: the chain's splits and lanes walked from Home with panel moves only (no touch): the dial and the
// left/right knobs, Confirm, cancel/EXIT — no tap. Home's Performance tile, a part, then Insert effects
// open the chain; the walk then presses every row the chain itself declares ("Chain 1–N of N"), each to
// a real effect (a plugin's actions, an insert row's split choices, a split's lanes) and EXIT back, and
// dives into the band split: its lanes, a lane's settings value by value (each `parts.lane` with the whole
// lane), Add band (`parts.addLane`), a crossover (`params.set`), Remove lane (`parts.removeLane` +
// `graph.remove`) and Remove split (`parts.removeSplit` + `graph.remove`), each asserted as the request the
// fake engine received or the view changed. Every count comes from the screen's own models, never the test's;
// nothing switches until Confirm.
#[test]
fn chain_splits_walked_from_home_with_panel_moves() {
    let mut f = Fixture::new("chain-splits-walk");
    f.engine.answer(); // session.hello
    f.pump();
    let graph = std::cell::RefCell::new(split_graph("bands", 3));
    let ui = f.screen.ui.clone_strong();
    let settle = |f: &mut Fixture| {
        while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(12)) {
            let method = r["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "graph.get" => graph.borrow().clone(),
                _ => json!({}),
            };
            f.reply(&r, json!({"result": result}));
        }
    };
    settle(&mut f);
    // The screen's own title (the shown screen's `show`, not the bar's breadcrumb).
    let scrn_title = |ui: &Shell| ui.global::<crate::PartState>().get_title().to_string();
    // Home → the Performance tile → Play, by panel moves only (nothing tapped).
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |f: &mut Fixture, key: &str, by: i64| {
        *total.entry(key.into()).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move", 0); // the first read is only a baseline
    nav(&mut f, "move", 1); // the cursor onto the Performance tile
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert_eq!(ui.get_screen(), 3, "Home's Performance tile opens Play");
    // Play → a part → the Part screen.
    nav(&mut f, "move:1", 1);
    nav(&mut f, "confirm:1", 1);
    settle(&mut f);
    assert_eq!((ui.get_screen(), title(&ui)), (4, "Keys".to_string()), "a press on a part opens the Part screen");
    // The Part screen → the dial down to Insert effects → the chain.
    nav(&mut f, "move", 1);
    settle(&mut f);
    let mut on_chain = false;
    for _ in 0..50 {
        let at = rows(&ui).iter().position(|r| r.cursor).expect("the cursor is on a row");
        let label = rows(&ui)[at].label.clone();
        if label == "Insert effects" {
            nav(&mut f, "confirm", 1);
            settle(&mut f);
            on_chain = ui.get_screen() == 5;
            break;
        }
        nav(&mut f, "move", 1);
        settle(&mut f);
    }
    assert!(on_chain, "Insert effects opens the chain screen");
    assert_eq!(scrn_title(&ui), "Insert effects");
    // The chain's own count, from its view line ("Chain 1–N of N"), read while the screen is up.
    let declared = |ui: &Shell| -> usize {
        let view = ui.global::<crate::PartState>().get_view().to_string();
        view.rsplit(" of ").next().unwrap_or("0").trim().parse().unwrap_or(0)
    };
    let n = declared(&ui);
    assert!(n >= 7, "the chain declares {n} rows (Chorus, a split, a Reverb and their insert rows)");
    // The next engine request, captured for the effect (before settle drains it). A meter poll that
    // fired in the same tick would otherwise shadow the effect request, so skip (and answer) any
    // `*.meters` poll instead of taking it as the effect.
    let next = |f: &mut Fixture, what: &str| -> serde_json::Value {
        loop {
            let r = f.engine.try_request(std::time::Duration::from_secs(5)).unwrap_or_else(|| panic!("no request after {what}"));
            if r["method"].as_str().unwrap_or_default().ends_with(".meters") {
                f.reply(&r, json!({"result": {}}));
                continue;
            }
            return r;
        }
    };
    // The split dive: the lanes and a lane's settings, then the split's own actions; Remove split returns
    // the chain list.
    let split_dived = std::cell::Cell::new(false);
    let dive_split = |f: &mut Fixture, nav: &mut dyn FnMut(&mut Fixture, &str, i64), settle: &dyn Fn(&mut Fixture)| {
        split_dived.set(true);
        assert_eq!(scrn_title(&ui), "Split: frequency bands", "the split opens on Confirm");
        assert!(declared(&ui) >= 6, "the split declares its rows");
        // Confirm on a lane opens it; the lane's rows, counted from its own view.
        let lane_of = |ui: &Shell| rows(ui).iter().position(|r| r.cursor).map(|i| rows(ui)[i].label.to_string());
        nav(f, "move", 1);
        settle(f);
        let mut lane_ok = false;
        for _ in 0..6 {
            let at = rows(&ui).iter().position(|r| r.cursor).expect("the cursor is on a row");
            if rows(&ui)[at].label == "Band 1" {
                nav(f, "confirm", 1);
                settle(f);
                lane_ok = scrn_title(&ui) == "Band 1";
                break;
            }
            nav(f, "move", 1);
            settle(f);
        }
        assert!(lane_ok, "a lane opens by Confirm");
        assert!(declared(&ui) >= 5, "a lane declares its settings");
        // The lane's settings, each to its `parts.lane`: a value starts with Confirm, the dial lays it down
        // and Confirm ends it; Mute sends the lane at once; Remove lane removes it and leaves the split.
        let (mut lane_visited, mut previous, mut removed_lane, mut lane_sent) = (Vec::new(), String::new(), false, false);
        nav(f, "move", 1);
        settle(f);
        for _ in 0..20 {
            let Some(at) = lane_of(&ui) else { nav(f, "move", 1); settle(f); continue };
            if at == previous {
                break;
            }
            if !lane_visited.contains(&at) {
                lane_visited.push(at.clone());
                match at.as_str() {
                    "Lane level" | "Lane pan" | "Lowest key" | "Highest key" => {
                        nav(f, "confirm", 1); // starts the change
                        settle(f);
                        nav(f, "move", 1); // the dial makes it: `parts.lane` with the whole lane
                        let r = next(f, &format!("{at} changed"));
                        assert_eq!((r["method"].as_str(), r["params"]["split"].as_u64(), r["params"]["lane"].as_u64()),
                            (Some("parts.lane"), Some(5), Some(0)), "{at}: the lane sent whole");
                        f.reply(&r, json!({"result": {}}));
                        settle(f);
                        lane_sent = true;
                        nav(f, "confirm", 1); // lays it down
                        settle(f);
                    }
                    "Mute" | "Solo" => {
                        nav(f, "confirm", 1); // toggled and sent at once
                        let r = next(f, &format!("{at} toggled"));
                        assert_eq!(r["method"].as_str(), Some("parts.lane"), "{at}: the lane sent whole");
                        f.reply(&r, json!({"result": {}}));
                        settle(f);
                        lane_sent = true;
                    }
                    "Remove lane" => {
                        nav(f, "confirm", 1);
                        let r = next(f, "Remove lane");
                        assert_eq!((r["method"].as_str(), r["params"]["split"].as_u64(), r["params"]["lane"].as_u64()),
                            (Some("parts.removeLane"), Some(5), Some(0)), "Remove lane removes the lane");
                        f.reply(&r, json!({"result": {}}));
                        settle(f);
                        removed_lane = true;
                        nav(f, "move", 1);
                        settle(f);
                        break;
                    }
                    _ => {} // the lane's "+ Insert plugin here" (it has none)
                }
            }
            previous = at;
            nav(f, "move", 1);
            settle(f);
        }
        assert!(removed_lane, "the walk removed a lane, which the lane's last row does");
        assert!(lane_sent, "a lane setting sent `parts.lane`");
        assert!(lane_visited.len() >= declared(&ui) - 1, "the lane's rows reached {lane_visited:?}");
        assert_eq!(scrn_title(&ui), "Split: frequency bands", "Remove lane leaves the split");
        // The split's own actions: Add band, a crossover, Remove split. Each press leaves its first request
        // queued for the caller to assert, then settle answers it.
        let find_press = |f: &mut Fixture, nav: &mut dyn FnMut(&mut Fixture, &str, i64), target: &str| -> bool {
            nav(f, "move", 1);
            settle(f);
            for _ in 0..12 {
                let at = rows(&ui).iter().position(|r| r.cursor).expect("the cursor is on a row");
                let lab = rows(&ui)[at].label.clone();
                if lab == target {
                    nav(f, "confirm", 1);
                    return true; // its request stays queued for the caller to assert
                }
                nav(f, "move", 1);
                settle(f);
            }
            false
        };
        assert!(find_press(f, nav, "Add band"), "Add band is reached and pressed");
        let r = next(f, "Add band");
        assert_eq!(r["method"].as_str(), Some("parts.addLane"), "Add band adds a lane");
        f.reply(&r, json!({"result": {}}));
        settle(f);
        // Add band keeps the cursor where it was; return it to the top of the split, one step at a
        // time until the move no longer moves the cursor up (the dial clamps at the first row).
        nav(f, "move", -1);
        settle(f);
        loop {
            let before = rows(&ui).iter().position(|r| r.cursor);
            nav(f, "move", -1);
            settle(f);
            let after = rows(&ui).iter().position(|r| r.cursor);
            match (before, after) {
                (Some(b), Some(a)) if a < b => continue,
                _ => break,
            }
        }
        assert!(find_press(f, nav, "Crossover 1"), "a crossover is reached and pressed");
        nav(f, "move", 1); // a step of the change
        let r = next(f, "a crossover step");
        assert_eq!(r["method"].as_str(), Some("params.set"), "a crossover is set in hertz");
        f.reply(&r, json!({"result": {}}));
        settle(f);
        nav(f, "confirm", 1); // lays the change down
        settle(f);
        assert!(find_press(f, nav, "Remove split"), "Remove split is reached and pressed");
        let r = next(f, "Remove split");
        assert_eq!(r["method"].as_str(), Some("parts.removeSplit"), "Remove split names the split");
        f.reply(&r, json!({"result": {}}));
        settle(f);
        assert_eq!(scrn_title(&ui), "Insert effects", "Remove split returns the chain");
    };
    // The chain's list, walked by the dial: every row it declares, pressed and EXIT back, the split dived
    // into. Nothing switches before Confirm. The count is the screen's own `rows()` (its cursor stops, one
    // per row), not the test's.
    let n = rows(&ui).len();
    assert_eq!(n, declared(&ui), "the screen's list and its view agree on the count");
    assert!(n >= 7, "the chain declares {n} rows (Chorus, a split, a Reverb and their insert rows)");
    let mut done = vec![false; n];
    let mut visited = Vec::<String>::new();
    nav(&mut f, "move", 1);
    settle(&mut f);
    for _ in 0..400 {
        let Some(i) = rows(&ui).iter().position(|r| r.cursor) else {
            nav(&mut f, "move", 1);
            settle(&mut f);
            continue;
        };
        if i >= n {
            break;
        }
        if done[i] {
            let before = i;
            nav(&mut f, "move", 1);
            settle(&mut f);
            if rows(&ui).iter().position(|r| r.cursor) == Some(before) {
                break; // the move left the cursor where it was: past the last row
            }
            continue;
        }
        done[i] = true;
        let at = rows(&ui)[i].label.to_string();
        visited.push(at.clone());
        if at.starts_with('+') {
            // An insert row: the plugins and the four split kinds, counted from the screen's own list.
            nav(&mut f, "confirm", 1);
            settle(&mut f);
            assert_eq!(scrn_title(&ui), "Insert: choose a plugin");
            let splits = labels(&ui).iter().filter(|l| l.starts_with("Split here:")).count();
            assert!(splits >= 4, "an insert row offers the four kinds of split, got {splits}");
            nav(&mut f, "cancel", 1);
            settle(&mut f);
            assert_eq!(ui.get_screen(), 5, "EXIT from the insert list back to the chain");
        } else if at == "2. Split" {
            nav(&mut f, "confirm", 1);
            settle(&mut f);
            dive_split(&mut f, &mut nav, &settle);
        } else {
            // A plugin: its actions.
            nav(&mut f, "confirm", 1);
            settle(&mut f);
            assert_ne!(scrn_title(&ui), "Insert effects", "{at} opens its actions");
            assert!(labels(&ui).iter().any(|l| l == "Parameters" || l == "Turn off" || l == "Remove"), "{at}'s actions: {:?}", labels(&ui));
            nav(&mut f, "cancel", 1);
            settle(&mut f);
            assert_eq!(ui.get_screen(), 5, "EXIT from {at}'s actions back to the chain");
        }
    }
    assert_eq!(visited.len(), n, "every row pressed once, got {visited:?}");
    assert!(split_dived.get(), "the split was dived into");
}
