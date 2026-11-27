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

/// The chain's rows as their labels, from the model it holds.
pub(super) fn chain_labels(ui: &Shell) -> Vec<String> {
    ui.global::<crate::PartState>().get_rows().iter().map(|r| r.label.to_string()).collect()
}

/// Where the one cursor is in the rows the model holds.
pub(super) fn chain_cursor(ui: &Shell) -> Option<usize> {
    ui.global::<crate::PartState>().get_rows().iter().position(|r| r.cursor)
}

/// Whether the chain is showing its top List (a plugin, or a "+ Insert plugin here", first).
pub(super) fn chain_list(ui: &Shell) -> bool {
    let m = ui.global::<crate::PartState>().get_rows();
    m.row_count() > 0 && m.row_data(0).is_some_and(|r| r.label.starts_with("+ Insert"))
}

/// Cancel back until the chain's top List shows again, then stop exactly there.
fn chain_to_list(ui: &Shell, mover: &mut impl FnMut(&str, i64), guard: usize) {
    for _ in 0..guard {
        if chain_list(ui) {
            return;
        }
        mover("cancel", 1);
    }
    assert!(chain_list(ui), "Cancel did not return the chain to its own List of rows");
}

/// The dial onto the `target`th row of the list before it, one step at a time, as a device's dial.
fn chain_mv(ui: &Shell, target: usize, mover: &mut impl FnMut(&str, i64), guard: usize) {
    for _ in 0..guard {
        match chain_cursor(ui) {
            Some(at) if at == target => return,
            Some(at) => mover("move", if at < target { 1 } else { -1 }),
            None => mover("move", 1),
        }
    }
    panic!("the dial could not stop on the {target}th row");
}

/// From the chain's List, the dial onto `plugin_row`, a press into its actions, the dial onto the
/// action `label`, a press. Every step is a panel move; no tap anywhere.
fn chain_press(ui: &Shell, plugin_row: usize, label: &str, mover: &mut impl FnMut(&str, i64)) {
    chain_to_list(ui, mover, 8);
    chain_mv(ui, plugin_row, mover, 40);
    mover("confirm", 1);
    let labels = chain_labels(ui);
    let target = labels.iter().position(|l| l == label).unwrap_or_else(|| panic!("the plugin does not offer {label}: {}", labels.join(" | ")));
    chain_mv(ui, target, mover, 60);
    mover("confirm", 1);
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
            ("IN".to_string(), 0, false, false),
            ("Any device · all keys".to_string(), 0, false, false),
            ("1. Chorus".to_string(), 2, true, false),
            ("2. Rhodes".to_string(), 1, false, false),
            ("3. Scope".to_string(), 3, false, false),
            ("OUT".to_string(), 0, false, false),
            ("Main".to_string(), 0, false, false),
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

// D6: the chain's In and Out ends say what the reference's say: the part's devices and keys (what feeds a bus), Main or the bus
// it plays through, a line for each send, an arrow for each plugin outside the parts its plugins are wired to; each end is
// one box (the lines marked first and last).
#[test]
fn the_ends_say_the_devices_and_the_sends() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut g = marks_graph();
    g["nodes"].as_array_mut().unwrap().push(json!({"id": 20, "name": "Hall", "plugin": {"name": "Hall"}, "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0, "enabled": true}));
    g["parts"][0]["input"] = json!({"kind": "notes", "port": 0, "devices": [{"kind": "external"}], "zones": [{"keys": [36, 72], "transpose": -12}]});
    g["parts"][0]["sends"] = json!([{"bus": 11, "level": 0.5}]);
    g["parts"].as_array_mut().unwrap().push(json!({"id": 11, "name": "Reverb bus", "level": 1.0, "pan": 0.0, "mute": false, "solo": false, "chain": [], "bus": 0, "sends": [], "splits": [], "input": {"kind": "bus", "port": 0}}));
    g["wires"] = json!([{"id": 1, "owner": "user", "kind": "audio", "from": 3, "to": 20, "enabled": true}]);
    let c = open(&s, &g);
    let lines: Vec<(String, bool, bool, bool)> = info(ui).iter().map(|l| (l.text.to_string(), l.boxed, l.first, l.last)).collect();
    assert_eq!(lines.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Chain", "IN", "All external · C2–C5 · −12", "1. Chorus", "2. Rhodes", "3. Scope", "OUT", "Main", "Send Reverb bus 50%", "→ Hall"]);
    assert_eq!(lines.iter().filter(|l| l.1).count(), 2 + 3 + 1, "the lines of the two ends are boxed");
    assert_eq!(lines.iter().filter(|l| l.2).map(|l| l.0.as_str()).collect::<Vec<_>>(), ["IN", "OUT"]);
    assert_eq!(lines.iter().filter(|l| l.3).map(|l| l.0.as_str()).collect::<Vec<_>>(), ["All external · C2–C5 · −12", "→ Hall"]);
    drop(c);
    // A bus: what feeds it, and where it plays through.
    g["parts"][1]["chain"] = json!([]);
    let mut c = Chain::new(ui);
    c.graph(&g);
    c.open(11);
    c.shown();
    let text: Vec<String> = info(ui).iter().map(|l| l.text.to_string()).collect();
    assert_eq!(text, ["Chain", "IN", "From Keys", "Empty: the sound goes straight through", "OUT", "Main"]);
    pictures("chain-ends", &g, &|_| {});
}

#[test]
fn the_marks_are_drawn() {
    pictures("chain-marks", &marks_graph(), &|_| {});
}

// W05: the chain view's no-touch walk. From Home by panel moves only — the Performance tile, Play, a
// part, the Part list's "Insert effects" — into the chain, where the dial stops on every row the
// screen's own model declares (never a literal count), and every action a plugin's press offers is
// pressed to the request the fake engine received or the screen that opened. No taps: the panel's
// navigation totals drive the shell as a device's would.
#[test]
fn the_chain_walked_from_home_by_panel_moves_only() {
    let mut f = Fixture::new("chain-walk");
    f.engine.answer(); // session.hello
    f.pump();
    let graph = std::cell::RefCell::new(part::part_graph());
    let ui = f.screen.ui.clone_strong();
    // A quiet engine: every request out is answered until none comes for a moment, kept so the walk
    // can hold each action to the request it made.
    let saw: std::rc::Rc<std::cell::RefCell<Vec<(String, serde_json::Value)>>> = Default::default();
    let settle = |f: &mut Fixture, saw: &std::rc::Rc<std::cell::RefCell<Vec<(String, serde_json::Value)>>>| {
        while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(12)) {
            let method = r["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "graph.get" => graph.borrow().clone(),
                "graph.add" => json!({"node": 99}),
                "graph.commit" | "graph.remove" | "parts.insert" | "graph.replace" => json!({}),
                "window.info" => json!({"available": r["params"]["node"].as_u64() == Some(2)}),
                "presets.list" => json!({"presets": []}),
                "params.list" => json!({"parameters": []}),
                "params.get" => json!({"values": []}),
                "library.query" => json!({"items": [], "total": 0}),
                "library.libraries" => json!({"libraries": []}),
                "session.status" => json!({"midiInputs": [], "midiOutputs": []}),
                "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
                "edit.history" | "edit.undo" | "edit.redo" => json!({"undo": "", "redo": ""}),
                "record.get" => json!({"recording": 0, "retro": {"seconds": 30}, "takes": []}),
                "parts.meters" => json!({"meters": []}),
                _ => json!({}),
            };
            saw.borrow_mut().push((method.clone(), r["params"].clone()));
            f.reply(&r, json!({"result": result}));
        }
    };
    settle(&mut f, &saw);
    // The panel: the shell reads the differences of its navigation totals, as a device counts them.
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |f: &mut Fixture, key: &str, by: i64| {
        *total.entry(key.into()).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move", 0); // the first read is only a baseline
    nav(&mut f, "move", 1); // the Performance tile
    nav(&mut f, "confirm", 1);
    settle(&mut f, &saw);
    assert_eq!(ui.get_screen(), 3, "Home's Performance tile opens Play");
    // Play: pane 1 down to the first part, pressed.
    nav(&mut f, "move:1", 1);
    nav(&mut f, "confirm:1", 1);
    settle(&mut f, &saw);
    assert_eq!(ui.get_screen(), 4, "the press on a part opens the Part screen");
    // Part: the dial down until the cursor reaches "Insert effects", then pressed → the chain.
    let cursor_at = |ui: &Shell| -> Option<usize> {
        ui.global::<crate::PartState>().get_rows().iter().position(|r| r.cursor)
    };
    let mut down = 0;
    loop {
        let Some(at) = cursor_at(&ui) else {
            nav(&mut f, "move", 1);
            settle(&mut f, &saw);
            continue;
        };
        if at < ui.global::<crate::PartState>().get_rows().row_count() && ui.global::<crate::PartState>().get_rows().row_data(at).is_some_and(|r| r.label.as_str() == "Insert effects") {
            break;
        }
        down += 1;
        assert!(down < 40, "the Part list has no “Insert effects” the dial can stop on");
        nav(&mut f, "move", 1);
        settle(&mut f, &saw);
    }
    nav(&mut f, "confirm", 1);
    settle(&mut f, &saw);
    assert_eq!((ui.get_screen(), title(&ui)), (5, "Keys › Insert effects".to_string()), "Insert effects opens the chain");
    // Every row the chain's own model declares is a stop of the dial (no touch): the count is the
    // screen's — never a literal — and the dial reaches them all without a tap.
    let declared = ui.global::<crate::PartState>().get_rows().row_count();
    assert!(declared > 0, "the chain declares no rows");
    let mut visited: Vec<usize> = Vec::new();
    nav(&mut f, "move", 1); // the first move shows the cursor
    settle(&mut f, &saw);
    for _ in 0..=declared {
        let Some(at) = cursor_at(&ui) else { break };
        if visited.contains(&at) {
            break; // the dial held where it was: past the last row
        }
        visited.push(at);
        nav(&mut f, "move", 1);
        settle(&mut f, &saw);
    }
    assert_eq!(visited.len(), declared, "the dial stopped on {visited:?}, the chain declares {declared} rows");
    // A panel's moves, as a device counts them: one dial or key turn and its Quit, in one step.
    let mut mover = |key: &str, by: i64| {
        nav(&mut f, key, by);
        settle(&mut f, &saw);
    };
    // Before any action, press the plugin to learn exactly what its action menu offers: the walk below
    // must press every row of it, so an action added later, or one left unhandled, fails. window.info
    // answers available for this node, so "Plugin view" is offered too.
    let offered = {
        chain_to_list(&ui, &mut mover, 8);
        chain_mv(&ui, 1, &mut mover, 40);
        mover("confirm", 1);
        chain_labels(&ui)
    };
    assert!(offered.iter().any(|l| l == "Plugin view"), "window.info available should offer Plugin view: {offered:?}");
    let mut handled: std::collections::BTreeSet<String> = Default::default();
    // "Turn on": Chorus is off, so the power control turns it on and commits.
    chain_press(&ui, 1, "Turn on", &mut mover);
    assert!(saw.borrow().iter().any(|(m, p)| m == "graph.enable" && p["node"] == 2 && p["enabled"] == true), "Turn on sent graph.enable to enable node 2: {saw:?}");
    handled.insert("Turn on".into());
    // "Move later": the first of the chain can move later; the commit follows.
    chain_press(&ui, 1, "Move later", &mut mover);
    assert!(saw.borrow().iter().any(|(m, p)| m == "parts.move" && p["part"] == 10 && p["from"] == 0 && p["to"] == 1), "Move later sent parts.move 0 to 1: {saw:?}");
    handled.insert("Move later".into());
    // Rhodes, the third row, can move earlier (from 1 to 0).
    chain_press(&ui, 3, "Move earlier", &mut mover);
    assert!(saw.borrow().iter().any(|(m, p)| m == "parts.move" && p["part"] == 10 && p["from"] == 1 && p["to"] == 0), "Move earlier sent parts.move 1 to 0: {saw:?}");
    // "Presets...": presets.list is asked for the node, the presets view replaces the actions.
    chain_press(&ui, 1, "Presets...", &mut mover);
    assert!(saw.borrow().iter().any(|(m, p)| m == "presets.list" && p["node"] == 2), "Presets... asked presets.list for node 2: {saw:?}");
    assert_eq!(ui.get_screen(), 5, "presets stay on the chain (their own view)");
    handled.insert("Presets...".into());
    // "Parameters": the grid of knobs opens for the node (the view changed); its Cancel returns.
    chain_press(&ui, 1, "Parameters", &mut mover);
    assert_eq!(ui.get_screen(), 7, "Parameters opens the parameter page");
    mover("cancel", 1);
    assert_eq!(ui.get_screen(), 5, "the parameter page's Cancel returns to the chain");
    handled.insert("Parameters".into());
    // "Routing": the wiring page opens for the node (the view changed), and back from it is Parts:
    // the shell's Back from Routing is uniformly the Parts list (go_back: Mixer|Controls|Routing),
    // and Routing remembers no view it was opened from.
    chain_press(&ui, 1, "Routing...", &mut mover);
    assert_eq!(ui.get_screen(), 12, "Routing opens the wiring page for the node");
    mover("cancel", 1);
    assert_eq!(ui.get_screen(), 9, "the wiring page's Cancel returns to the Parts list");
    handled.insert("Routing...".into());
    // Back into the chain: Parts' first part, then Insert effects again — all panel moves.
    let mut back_in = 0;
    while ui.get_screen() != 5 {
        back_in += 1;
        assert!(back_in < 8, "could not return to the chain from Parts");
        mover("move:0", 1); // Parts' first part
        mover("confirm", 1);
        let mut down = 0;
        loop {
            let Some(at) = chain_cursor(&ui) else { mover("move", 1); continue; };
            if chain_labels(&ui).get(at).map(|l| l.as_str()) == Some("Insert effects") {
                break;
            }
            down += 1;
            assert!(down < 40, "the Part list has no “Insert effects” the dial can stop on");
            mover("move", 1);
        }
        mover("confirm", 1);
    }
    // "Scene controls...": the node's control map is asked (params.list) and a menu opens; Cancel
    // closes the menu, then back to the List.
    chain_press(&ui, 1, "Scene controls...", &mut mover);
    assert!(saw.borrow().iter().any(|(m, p)| m == "params.list" && p["node"] == 2), "Scene controls asked params.list for node 2: {saw:?}");
    mover("cancel", 1); // the scene-controls menu closes
    chain_to_list(&ui, &mut mover, 8);
    handled.insert("Scene controls...".into());
    // "Plugin view": window.info said this node has a window, so the menu offers the row; in this
    // shell the window has not been implemented yet, so a press is a no-op (it stays on the chain).
    chain_press(&ui, 1, "Plugin view", &mut mover);
    assert_eq!(ui.get_screen(), 5, "Plugin view stays on the chain (no window in this shell yet)");
    handled.insert("Plugin view".into());
    // The inserts: an insert row opens the choices — the four sorts of split first, then the plugins
    // the scanner found — and choosing one adds the node (graph.add) and inserts it (parts.insert).
    shell::plugins_found(vec![crate::scan::Plugin { id: "org.chorus".into(), name: "Chorus".into(), path: "/p/Chorus.clap".into(), vendor: String::new(), features: vec!["audio-effect".into()] }]);
    chain_to_list(&ui, &mut mover, 8);
    chain_mv(&ui, 0, &mut mover, 40);
    mover("confirm", 1); // the first "+ Insert plugin here"
    let picking = chain_labels(&ui);
    let splits = ["Split here: layers", "Split here: left / right", "Split here: mid / side", "Split here: frequency bands"];
    assert!(picking.windows(4).any(|w| w == splits), "an insert row lists the four sorts of split before the plugins: {picking:?}");
    let choose = picking.iter().position(|l| l == "Chorus").expect("the scanned plugin is offered");
    chain_mv(&ui, choose, &mut mover, 80);
    mover("confirm", 1);
    assert!(saw.borrow().iter().any(|(m, p)| m == "graph.add" && p["name"] == "Chorus 2" && p["pluginId"] == "org.chorus"), "choosing a plugin adds the node: {saw:?}");
    assert!(saw.borrow().iter().any(|(m, p)| m == "parts.insert" && p["node"] == 99 && p["part"] == 10), "the add is followed by parts.insert: {saw:?}");
    assert!(saw.borrow().iter().any(|(m, _)| m == "graph.commit"), "inserting commits");
    // "Remove": the plugin's node leaves the part (parts.removeNode), the graph's wires reconnected,
    // then the commit.
    chain_press(&ui, 1, "Remove", &mut mover);
    assert!(saw.borrow().iter().any(|(m, p)| m == "parts.removeNode" && p["part"] == 10 && p["node"] == 2), "Remove sent parts.removeNode for node 2: {saw:?}");
    assert!(saw.borrow().iter().any(|(m, p)| m == "graph.remove" && p["userWires"] == "reconnect"), "Remove reconnects the wires the node leaves: {saw:?}");
    assert!(saw.borrow().iter().any(|(m, _)| m == "graph.commit"), "Remove commits");
    handled.insert("Remove".into());
    // Every action the plugin's own menu offered was pressed: an action added later, or one the
    // walk left out, fails here rather than slipping past.
    for label in &offered {
        assert!(handled.contains(label), "the walk did not press the offered action {label:?}");
    }
}
