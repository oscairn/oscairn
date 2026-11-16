// SPDX-License-Identifier: AGPL-3.0-or-later
//! The chain screen's splits, lanes and marks (chain.rs; the plain chain's tests are in pages.rs): the
//! engine's words for a part with a split, each action's requests, the cursor without touch, and the
//! drawing in the themes at the sizes of the keyboard's screen and the smaller and larger ones.

use super::*;
use crate::chain::{Request, Screen as Chain};
use crate::client::Error;

/// A part whose chain is a Chorus (off), then a Rhodes (an instrument), then a level meter that only
/// takes sound in (an analyzer): the three role colors.
pub(super) fn marks_graph() -> serde_json::Value {
    json!({
        "nodes": [
            {"id": 1, "name": "Rhodes", "plugin": {"name": "Surge XT"}, "audioInputs": [], "audioOutputs": [{"channels": 2}], "noteInputs": 1, "noteOutputs": 0, "enabled": true},
            {"id": 2, "name": "Chorus", "plugin": {"name": "Chorus"}, "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0, "enabled": false},
            {"id": 3, "name": "Scope", "plugin": {"name": "Scope"}, "audioInputs": [{"channels": 2}], "audioOutputs": [], "noteInputs": 0, "noteOutputs": 0, "enabled": true},
        ],
        "parts": [{"id": 10, "name": "Keys", "level": 0.5, "pan": 0.0, "mute": false, "solo": false, "chain": [2, 1, 3], "bus": 0, "sends": [], "splits": [],
                   "from": {"sound": "s1", "name": "Warm Rhodes", "modified": false}, "input": {"kind": "notes", "port": 0, "devices": [], "zones": []}}],
        "wires": [], "scenes": [], "scene": 0,
    })
}

pub(super) fn rows(ui: &Shell) -> Vec<crate::SettingData> {
    ui.global::<crate::PartState>().get_rows().iter().collect()
}

pub(super) fn info(ui: &Shell) -> Vec<crate::InfoLine> {
    ui.global::<crate::PartState>().get_info().iter().collect()
}

pub(super) fn open(s: &super::Screen, graph: &serde_json::Value) -> Chain {
    tall(&s.ui);
    let mut c = Chain::new(&s.ui);
    c.graph(graph);
    c.open(10);
    c.shown();
    c
}

pub(super) fn message(ui: &Shell) -> String {
    ui.global::<crate::PartState>().get_message().to_string()
}

// A plugin is marked by its role (the instrument, the effect, and the rest each their own color class),
// faint when off, and outlined and named when it failed and is silent; an edit the engine refused is said
// before the failure, and the failure is said again once the refusal goes. The panel draws the path In to
// Out with the same marks.
#[test]
fn plugins_are_marked_by_role_off_and_failure() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s, &marks_graph());
    let r = rows(ui);
    let plugins: Vec<_> = r.iter().filter(|r| r.kind == 2 && r.role > 0).map(|r| (r.label.to_string(), r.value.to_string(), r.role, r.off, r.failed)).collect();
    assert_eq!(
        plugins,
        [
            ("1. Chorus".to_string(), "Effect (off) ›".to_string(), 2, true, false),
            ("2. Rhodes".to_string(), "Instrument ›".to_string(), 1, false, false),
            ("3. Scope".to_string(), "Analyzer ›".to_string(), 3, false, false),
        ],
        "the role's word and color class, off, failed"
    );
    assert!(r.iter().filter(|r| r.label.starts_with('+')).all(|r| r.role == 0 && !r.off && !r.failed), "the insert rows are not plugins");
    let lines: Vec<_> = info(ui).iter().map(|l| (l.text.to_string(), l.role, l.off, l.failed)).collect();
    assert_eq!(
        lines,
        [
            ("Chain".to_string(), 0, false, false),
            ("In".to_string(), 0, false, false),
            ("1. Chorus".to_string(), 2, true, false),
            ("2. Rhodes".to_string(), 1, false, false),
            ("3. Scope".to_string(), 3, false, false),
            ("Out".to_string(), 0, false, false),
        ]
    );
    assert_eq!(message(ui), "");
    // The engine says the Rhodes failed: its row and its line are outlined, the message names it and why.
    c.status(&json!({"failures": [{"node": 1, "name": "Rhodes", "reason": "it crashed"}]}));
    assert_eq!(rows(ui).iter().filter(|r| r.failed).map(|r| r.label.to_string()).collect::<Vec<_>>(), ["2. Rhodes"]);
    assert_eq!(info(ui).iter().filter(|l| l.failed).map(|l| l.text.to_string()).collect::<Vec<_>>(), ["2. Rhodes"]);
    assert_eq!(message(ui), "Rhodes is silent: it crashed");
    // A refused edit is said first; the next edit that is not refused clears it and the failure shows again.
    assert!(c.reply(Request::Edit, Err(Error { code: -32602, message: "no room".into() })));
    assert_eq!(message(ui), "The engine refused: no room");
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None)); // the Chorus's actions
    assert_eq!(message(ui), "The engine refused: no room", "looking does not clear it");
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Confirm(None)); // Turn on
    assert_eq!(message(ui), "Rhodes is silent: it crashed");
    // It recovers: the marks and the message go.
    c.status(&json!({"failures": []}));
    assert!(rows(ui).iter().all(|r| !r.failed) && info(ui).iter().all(|l| !l.failed));
    assert_eq!(message(ui), "");
    // A status without a list changes nothing.
    c.status(&json!({"failures": [{"node": 3, "name": "Scope", "reason": "x"}]}));
    c.status(&json!({"midiInputs": []}));
    assert_eq!(message(ui), "Scope is silent: x");
}

/// The screen in the dark theme and the odd roomy and tight ones at 1280x720 scale 1, at 1.55 and at 800x480,
/// pictures for a look (target/screens/NAME-THEME-SIZE.png).
pub(super) fn pictures(name: &str, graph: &serde_json::Value, setup: &dyn Fn(&mut Chain)) {
    fit_to_window();
    let dark = theme::chosen(&theme::Places::default(), "oscairn-dark", "");
    for ((w, h), scale, size) in [((1280, 720), 1.0, "1280x720"), ((1280, 720), 1.55, "1280x720@1.55"), ((800, 480), 1.0, "800x480")] {
        let mut s = Screen::sized(w, h);
        s.set_scale(scale);
        crate::keystrip::install(&s.ui);
        s.ui.set_screen(5);
        for (t, id) in [(&dark, "dark"), (&odd_theme(), "odd"), (&joined_theme(), "joined")] {
            theme::apply(&s.ui, t);
            s.frame();
            let mut c = open(&s, graph);
            c.status(&json!({"failures": [{"node": 1, "name": "Rhodes", "reason": "it crashed"}]}));
            setup(&mut c);
            assert!(s.frame().is_some(), "{name} {id} {size}: drawn");
            png(&s, &format!("{name}-{id}-{size}"));
        }
    }
}

#[test]
fn the_marks_are_drawn() {
    pictures("chain-marks", &marks_graph(), &|_| {});
}
