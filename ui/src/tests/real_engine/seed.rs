// SPDX-License-Identifier: AGPL-3.0-or-later
//! The seed data of the real-engine harnesses, made through the engine's own requests in the engine the harness started (so
//! in the run's root, from nothing, with invented names and only the repo's plugins from `OSCAIRN_REAL_PLUGINS`): the
//! Performance "Real Engine Check" for `real_engine` and a small library for `real_library`.

use super::Direct;
use serde_json::{json, Value};

/// What a harness starts with.
#[derive(Clone, Copy, PartialEq)]
pub enum Seed {
    /// "Real Engine Check", open.
    Check,
    /// Three Performances, a Sound and a setlist.
    Library,
}

fn plugin(dir: &str, name: &str) -> String {
    let path = std::path::Path::new(dir).join(format!("oscairn-{name}.clap"));
    assert!(path.is_file(), "{} is not there: OSCAIRN_REAL_PLUGINS must be a folder with the built plugins (basic-synth, delay, transpose)", path.display());
    path.to_string_lossy().into_owned()
}

fn id(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or_else(|| panic!("seed: no {key} in {v}"))
}

/// Seeds the engine for `seed`; `plugins` is the folder of built plugins.
pub fn make(e: &mut Direct, seed: Seed, plugins: Option<&str>) {
    match seed {
        Seed::Check => check(e, plugins.expect("OSCAIRN_REAL_PLUGINS (the folder of built plugins) is needed to make the Performance")),
        Seed::Library => library(e),
    }
}

/// Lead: Basic Synth + Delay and a send to Bus A; Groovebox: the step sequencer (the engine's own first Performance has both
/// parts); Split: layers, lane 1 Low Synth on C-1 to B3, lane 2 Transpose + High Synth on C4 and up at 80%; Empty; Bus A with
/// a Delay; scenes Verse and Chorus (Split at 50%); a control with two targets.
fn check(e: &mut Direct, plugins: &str) {
    let g = e.graph();
    let (lead, synth) = (id(&g["parts"][0], "id"), g["nodes"].as_array().unwrap().iter().find(|n| n["name"] == "synth").map(|n| id(n, "id")).expect("the first Performance has a synth"));
    e.call("parts.name", json!({"part": lead, "name": "Lead"}));
    let split = id(&e.call("parts.add", json!({"name": "Split"})), "part");
    let low = id(&e.call("graph.add", json!({"path": plugin(plugins, "basic-synth"), "name": "Low Synth"})), "node");
    let high = id(&e.call("graph.add", json!({"path": plugin(plugins, "basic-synth"), "name": "High Synth"})), "node");
    let transpose = id(&e.call("graph.add", json!({"path": plugin(plugins, "transpose"), "name": "Transpose"})), "node");
    let at = id(&e.call("parts.addSplit", json!({"part": split, "index": 0})), "split");
    e.call("parts.insert", json!({"part": split, "index": 0, "node": low, "split": at, "lane": 0}));
    e.call("parts.insert", json!({"part": split, "index": 0, "node": transpose, "split": at, "lane": 1}));
    e.call("parts.insert", json!({"part": split, "index": 1, "node": high, "split": at, "lane": 1}));
    e.call("parts.lane", json!({"part": split, "split": at, "lane": 0, "zones": [{"keys": [0, 59]}]}));
    e.call("parts.lane", json!({"part": split, "split": at, "lane": 1, "zones": [{"keys": [60, 127]}], "level": 0.8}));
    e.call("parts.add", json!({"name": "Empty"}));
    let bus_input = json!({"kind": "bus", "port": 0, "zones": [], "devices": [], "maxNotes": 0, "reserve": 0, "priority": 0, "receive": {}});
    let bus = id(&e.call("parts.add", json!({"name": "Bus A", "input": bus_input})), "part");
    let echo = id(&e.call("graph.add", json!({"path": plugin(plugins, "delay"), "name": "Bus Echo"})), "node");
    e.call("parts.insert", json!({"part": bus, "index": 0, "node": echo}));
    e.call("parts.send", json!({"part": lead, "bus": bus, "level": 0.4}));
    e.call("graph.commit", json!({}));
    let verse = id(&e.call("scenes.add", json!({"name": "Verse"})), "scene");
    e.call("parts.level", json!({"part": split, "level": 0.5, "mute": false}));
    e.call("scenes.add", json!({"name": "Chorus"}));
    e.call("scenes.select", json!({"scene": verse}));
    let control = id(&e.call("controls.add", json!({"name": "Knob"})), "control");
    e.call("controls.targets", json!({"control": control, "targets": [{"kind": "parameter", "node": synth, "param": 1}, {"kind": "partLevel", "part": lead}]}));
    e.call("graph.commit", json!({}));
    e.call("library.save", json!({"name": "Real Engine Check", "new": true}));
}

/// Brass Hall, Glass Choir and Night Strings (the last two tagged warm), the Sound Soft Felt (from the first part) and the
/// setlist Friday Set holding Glass Choir (Brass Hall is deleted and installed again by the harness).
fn library(e: &mut Direct) {
    let lead = id(&e.graph()["parts"][0], "id");
    e.call("library.save", json!({"name": "Brass Hall"}));
    let mut choir = String::new();
    for name in ["Glass Choir", "Night Strings"] {
        let made = e.call("library.save", json!({"name": name, "new": true}))["id"].as_str().unwrap().to_string();
        e.call("library.edit", json!({"id": made, "name": name, "description": "invented", "tags": ["warm", "pad"]}));
        if choir.is_empty() {
            choir = made;
        }
    }
    e.call("parts.saveSound", json!({"part": lead, "name": "Soft Felt", "new": true}));
    let slot = json!({"id": choir, "name": "Glass Choir", "color": "", "label": ""});
    let setlist = json!({"format": "oscairn.setlist", "version": 2,
        "info": {"id": "", "name": "Friday Set", "description": "", "tags": [], "color": "", "revision": "", "parent": "", "hash": "", "origin": null},
        "display": {"mode": "grid", "columns": 4, "rows": 3}, "pages": [{"name": "Set 1", "slots": [slot]}]});
    e.call("setlists.save", json!({"setlist": setlist}));
}
