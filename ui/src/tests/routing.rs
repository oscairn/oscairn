// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Routing page (I1 to I6, G5): the graph drawn from one set of points (the wires meet their ports at every
//! window size and scale, measured in the rendered pixels), what is chosen and its panel, the connection menus,
//! each source's mix, loose plugins, parts collapsed and expanded, the wires' meters, a lane's control, and on the
//! shell (the Parts list's Routing button and its shared rows, Open part and back) through the fake engine.

use super::bar::{play_on_the_shell, request_in};
use super::wires::graph as wire_graph;
use super::*;
use crate::routing::{Outcome, Request, Screen as Routing};
use crate::scan::Plugin;
use crate::{MenuState, PartsState, RoutingState};

const INPUT: u64 = 0xFFFF_FFFE;

/// The graph of the layout tests, with a plugin outside the parts that has a lane (an LFO), a user wire of notes
/// from the keys to the Bass synth, and a control the LFO's lane drives.
fn graph() -> serde_json::Value {
    let mut g = wire_graph();
    let mut lfo = json!({"id": 9, "name": "LFO", "plugin": {"id": "mod.lfo", "name": "LFO", "lanes": [{"lane": 0, "name": "Wave"}, {"lane": 1, "name": "Step"}]},
                         "audioInputs": [], "audioOutputs": [], "noteInputs": 0, "noteOutputs": 0, "enabled": true, "ringing": false});
    lfo["parameterRevision"] = json!(1);
    g["nodes"].as_array_mut().unwrap().push(lfo);
    let mut w = json!({"id": 30, "kind": "notes", "owner": "user", "from": INPUT, "to": 3, "fromPort": 0, "toPort": 0, "channels": [], "level": 1.0, "pan": 0.0, "mute": false, "solo": false,
                       "zones": [], "send": 0, "split": 0, "lane": 0, "fader": false, "connected": true, "enabled": true});
    w["zones"] = json!([]);
    g["wires"].as_array_mut().unwrap().push(w);
    g["controls"] = json!([{"id": 5, "part": 0, "name": "Wobble", "value": 0.5, "source": {"node": 9, "lane": 0}, "targets": [{"kind": "parameter", "node": 8, "param": 1, "start": 0.0, "end": 1.0, "curve": "linear", "relative": true, "base": 0.0, "offIn": []}]}]);
    g["mappings"] = json!([{"from": 9, "lane": 1, "to": 4, "param": 2}]);
    g["scenes"] = json!([]);
    g
}

fn routing_screen(s: &mut Screen, graph: &serde_json::Value) -> Routing {
    let mut r = Routing::new(&s.ui);
    s.ui.set_screen(12);
    s.frame();
    r.graph(graph);
    s.frame();
    r.shown();
    s.frame();
    r.shown(); // the canvas's size is known now
    r
}

fn names(model: slint::ModelRc<crate::RoutingBox>) -> Vec<String> {
    model.iter().map(|b| b.name.to_string()).collect()
}

fn boxes(ui: &Shell) -> Vec<String> {
    names(ui.global::<RoutingState>().get_boxes())
}

fn at(ui: &Shell, name: &str) -> i32 {
    boxes(ui).iter().position(|n| n == name).unwrap_or_else(|| panic!("{name} in {:?}", boxes(ui))) as i32
}

/// The panel's rows: the words and each cell's label.
fn panel(ui: &Shell) -> Vec<(String, Vec<String>)> {
    ui.global::<RoutingState>().get_panel().iter().map(|r| (r.text.to_string(), r.cells.iter().map(|c| c.label.to_string()).collect())).collect()
}

fn cursor(ui: &Shell) -> Option<String> {
    let s = ui.global::<RoutingState>();
    if let Some(b) = s.get_boxes().iter().find(|b| b.cursor) {
        return Some(b.name.to_string());
    }
    if let Some(b) = s.get_frames().iter().find(|b| b.cursor) {
        return Some(format!("frame {}", b.name));
    }
    if let Some(b) = s.get_controls().iter().find(|b| b.cursor) {
        return Some(format!("control {}", b.name));
    }
    for (r, row) in s.get_panel().iter().enumerate() {
        if let Some(c) = row.cells.iter().position(|c| c.cursor) {
            return Some(format!("panel {r}.{c}"));
        }
    }
    None
}

fn edits(r: &mut Routing) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(&mut r.out).into_iter().filter(|o| !matches!(o.0, Request::List(_) | Request::Window(_))).map(|(_, m, p)| (m, p)).filter(|(m, _)| *m != "graph.commit").collect()
}

fn mv(r: &mut Routing, pane: Option<u8>, by: i32) -> Outcome {
    r.act(Action::Move { pane, by })
}

fn press_cell(r: &mut Routing, ui: &Shell, row_text: &str, label: &str) -> Outcome {
    let rows = ui.global::<RoutingState>().get_panel();
    let (i, j) = rows
        .iter()
        .enumerate()
        .find_map(|(i, row)| (row.text.as_str() == row_text).then(|| row.cells.iter().position(|c| c.label.as_str() == label).map(|j| (i, j))).flatten())
        .unwrap_or_else(|| panic!("{label} on {row_text} in {:?}", panel(ui)));
    r.tap_cell(i as i32, j as i32)
}

fn choose(m: &mut crate::menu::Menu, r: &mut Routing, path: &[&str]) {
    for (i, label) in path.iter().enumerate() {
        let at = m.labels().iter().position(|l| l.0 == *label).unwrap_or_else(|| panic!("{label} in {:?}", m.labels()));
        while m.labels().iter().position(|l| l.2) != Some(at) {
            let now = m.labels().iter().position(|l| l.2).unwrap();
            m.act(Action::Move { pane: None, by: if now < at { 1 } else { -1 } });
        }
        match m.act(Action::Confirm(None)) {
            crate::menu::Outcome::Choose(id) => {
                assert_eq!(i, path.len() - 1);
                r.chosen(id);
            }
            crate::menu::Outcome::Stay => assert!(i < path.len() - 1),
            crate::menu::Outcome::Close => panic!("closed"),
        }
    }
}

fn menu_of(r: &mut Routing, s: &Screen) -> crate::menu::Menu {
    let (title, items) = r.take_menu().expect("a menu was ready");
    crate::menu::Menu::open(&s.ui, &title, items)
}

// The page draws what the layout has: a box per plugin and end, a part box with its chips (collapsed), the wires
// that cross parts, the control's pill and its lines; nothing is chosen, so everything is bright.
#[test]
fn the_page_draws_boxes_wires_chips_and_control_lines() {
    let mut s = Screen::sized(1280, 720);
    let r = routing_screen(&mut s, &graph());
    let state = s.ui.global::<RoutingState>();
    assert_eq!(boxes(&s.ui), ["Keys & MIDI", "Line in", "Keys", "Bass", "Reverb", "Master FX", "LFO", "Main out"]);
    let keys = state.get_boxes().iter().find(|b| b.name == "Keys").unwrap();
    assert!(keys.part && keys.chevron == 1 && keys.bright && !keys.chosen);
    assert_eq!(keys.chips.iter().map(|c| c.to_string()).collect::<Vec<_>>(), ["Chorus", "Rhodes"]);
    assert_eq!(state.get_boxes().iter().find(|b| b.name == "Reverb").unwrap().detail.as_str(), "bus");
    assert!(!state.get_boxes().iter().any(|b| b.name == "Master FX" && b.badge), "a loose plugin has no badge: only a part's plugin wired by hand");
    assert_eq!(state.get_frames().row_count(), 0, "collapsed: no frames");
    assert_eq!(names(state.get_controls()), ["Wobble"]);
    assert!(state.get_wires().row_count() >= 6);
    assert_eq!(state.get_links().row_count(), 3, "the LFO's lane to a parameter, to its control and the control to the plugin it drives");
    let w = state.get_wires().row_data(0).unwrap();
    assert!(w.commands.starts_with("M "));
    assert!(w.alpha > 0.5 && w.width > 0.);
    assert!(state.get_scale() > 0.3 && state.get_scale() <= 1.);
    assert_eq!(r.bar_tools().iter().map(|t| (t.label.to_string(), t.on)).collect::<Vec<_>>(), [("+ Plugin".into(), false), ("Parts collapsed".into(), true), ("Meters".into(), false)]);
    assert_eq!(panel(&s.ui), [("Select a plugin to wire it, or to set the level of each source into it.".to_string(), vec![])]);
}

// Choosing a plugin (a tap, or a press on the cursor) shows its panel and keeps what its wires reach bright, the
// rest faint; a second tap lets go; a part is chosen the same way and shows Open part and Expand.
#[test]
fn choosing_a_plugin_or_a_part_shows_its_panel_and_dims_the_rest() {
    let mut s = Screen::sized(1280, 720);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    let state = ui.global::<RoutingState>();
    r.tap(1, at(&ui, "Master FX"));
    let get = |n: &str| state.get_boxes().iter().find(|b| b.name == n).unwrap();
    assert!(get("Master FX").chosen && get("Master FX").bright);
    assert!(get("Keys").bright && get("Bass").bright && get("Main out").bright && get("Line in").bright, "what its wires reach");
    assert!(!get("Reverb").bright && !get("LFO").bright, "what they do not");
    assert_eq!(panel(&ui)[0], ("Master FX".to_string(), vec!["Connect audio to...".into(), "Turn off".into(), "Remove".into()]));
    let titles: Vec<_> = panel(&ui).into_iter().filter(|r| r.1.is_empty()).map(|r| r.0).collect();
    assert_eq!(titles, ["Main · mixed from 3 sources"]);
    // The wires that do not touch it are faint.
    let faint = state.get_wires().iter().filter(|w| w.alpha < 0.2).count();
    assert!(faint >= 2 && faint < state.get_wires().row_count());
    r.tap(1, at(&ui, "Master FX"));
    assert!(!get("Master FX").chosen && get("Reverb").bright, "a second tap lets go");
    // A part: Open part and Expand; the rest of the parts' wires faint.
    r.tap(1, at(&ui, "Keys"));
    assert!(get("Keys").chosen);
    assert_eq!(panel(&ui), [("Keys".to_string(), vec!["Open part".to_string(), "Expand".to_string()])]);
    assert_eq!(press_cell(&mut r, &ui, "Keys", "Open part"), Outcome::OpenPart(10));
    // The end: the keys and MIDI have notes only, an input of audio has audio.
    r.tap(1, at(&ui, "Keys & MIDI"));
    assert_eq!(panel(&ui)[0].1, ["Connect notes to...", "Keyboard"].map(String::from).to_vec().into_iter().filter(|c| c != "Keyboard").collect::<Vec<_>>());
    r.tap(1, at(&ui, "Line in"));
    assert_eq!(panel(&ui)[0].1, ["Connect audio to..."]);
    // The output lists what feeds it, a row of its own for each source.
    r.tap(1, at(&ui, "Main out"));
    assert_eq!(panel(&ui)[0], ("Main out".to_string(), vec![]));
}

// The cursor goes over the boxes with the left knob and the dial, to the panel's cells with the right knob, and the
// dial goes on from the last box to the panel; a press chooses; arrows cross the columns; Cancel drops it.
#[test]
fn the_cursor_moves_over_boxes_and_the_panel() {
    let mut s = Screen::sized(1280, 720);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    assert_eq!(cursor(&ui), None);
    mv(&mut r, Some(0), 1);
    assert_eq!(cursor(&ui).as_deref(), Some("Keys & MIDI"), "the first box, in the first column");
    let mut seen = vec![];
    for _ in 0..8 {
        mv(&mut r, Some(0), 1);
        seen.push(cursor(&ui).unwrap());
    }
    assert!(seen.contains(&"Main out".to_string()) && seen.contains(&"control Wobble".to_string()), "all of them: {seen:?}");
    assert_eq!(seen.last().unwrap(), "Main out", "held at the last place: the rightmost column");
    mv(&mut r, Some(0), -20);
    assert_eq!(cursor(&ui).as_deref(), Some("Keys & MIDI"));
    // Right across the columns: the nearest box in the next one.
    r.act(Action::Arrow { dx: 1, dy: 0 });
    let next = cursor(&ui).unwrap();
    assert!(next != "Keys & MIDI" && next != "Line in", "a box of a column to the right: {next}");
    // A press chooses it, the panel shows it; the right knob goes to its cells.
    let master = at(&ui, "Master FX");
    r.tap(1, master);
    mv(&mut r, Some(1), 1);
    assert_eq!(cursor(&ui).as_deref(), Some("panel 0.0"));
    mv(&mut r, Some(1), 1);
    assert_eq!(cursor(&ui).as_deref(), Some("panel 0.1"));
    assert!(edits(&mut r).is_empty(), "moving sends nothing");
    // The dial from a box goes on through the rest of them into the panel.
    mv(&mut r, Some(0), 100);
    for _ in 0..30 {
        mv(&mut r, None, 1);
        if cursor(&ui).is_some_and(|c| c.starts_with("panel")) {
            break;
        }
    }
    assert!(cursor(&ui).is_some_and(|c| c.starts_with("panel")), "the dial reached the panel");
    assert_eq!(r.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(cursor(&ui), None);
    assert_eq!(r.act(Action::Cancel(None)), Outcome::Back);
    // Up from the first place: the top bar's (its buttons are the bar's, not stops of the page).
    r.act(Action::End(-1));
    assert_eq!(cursor(&ui).as_deref(), Some("Keys & MIDI"));
    assert!(r.at_top());
}

// Connect audio to... lists every node that takes audio, the output once (and not for notes), by part; choosing one is
// `graph.connect` and a commit; notes likewise for what takes notes; nothing takes it, a disabled line says so.
#[test]
fn the_connect_menus_list_what_takes_the_kind_and_connect() {
    let mut s = Screen::sized(1280, 720);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    r.tap(1, at(&ui, "Master FX"));
    assert_eq!(press_cell(&mut r, &ui, "Master FX", "Connect audio to..."), Outcome::Stay);
    let mut m = menu_of(&mut r, &s);
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Chorus · Keys", "Hall · Reverb", "Main output"], "what takes audio, the output once, not itself");
    let _ = std::mem::take(&mut r.out);
    choose(&mut m, &mut r, &["Hall · Reverb"]);
    let sent = std::mem::take(&mut r.out);
    assert_eq!(sent.iter().map(|o| o.1).collect::<Vec<_>>(), ["graph.connect", "graph.commit"]);
    assert_eq!(sent[0].2, json!({"kind": "audio", "from": 8, "to": 4}));
    // Notes from the keys: what has a note input.
    r.tap(1, at(&ui, "Keys & MIDI"));
    press_cell(&mut r, &ui, "Keys & MIDI", "Connect notes to...");
    let mut m = menu_of(&mut r, &s);
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Rhodes · Keys", "Bass · Bass"]);
    choose(&mut m, &mut r, &["Bass · Bass"]);
    assert_eq!(edits(&mut r), [("graph.connect", json!({"kind": "notes", "from": INPUT, "to": 3}))]);
    // Nothing takes notes from a plugin with no note input to send to... an output has no connect button at all.
    r.tap(1, at(&ui, "Main out"));
    assert!(panel(&ui)[0].1.is_empty());
}

// Each source into an input has a row: On, its level (a slider the dial changes a fortieth a click), M and S, and
// Disconnect where the wire was drawn by hand; a lane's or split's wire is set in its lane (the slider is off); a
// note wire from the keys has its channel. Every change commits.
#[test]
fn each_source_has_its_mix_a_channel_and_a_disconnect() {
    let mut s = Screen::sized(1280, 900);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    r.tap(1, at(&ui, "Master FX"));
    let rows = panel(&ui);
    assert!(rows.iter().any(|r| r.0 == "Chorus (Keys) · part" && r.1 == ["On", "", "M", "S"]), "{rows:?}");
    assert!(rows.iter().any(|r| r.0 == "Audio in" && r.1 == ["On", "", "M", "S", "Disconnect"]), "a wire drawn by hand can be taken away");
    // The slider of the hand-drawn wire: touch sets it, the dial steps it.
    let row_of = |ui: &Shell, text: &str| ui.global::<RoutingState>().get_panel().iter().position(|r| r.text.as_str() == text).unwrap() as i32;
    let audio = row_of(&ui, "Audio in");
    let _ = std::mem::take(&mut r.out);
    r.level(audio, 1, 0.5);
    let sent = std::mem::take(&mut r.out);
    assert_eq!(sent.iter().map(|o| o.1).collect::<Vec<_>>(), ["wires.mix", "graph.commit"]);
    assert_eq!(sent[0].2, json!({"wire": 9, "level": 0.5, "pan": 0.0, "mute": false, "solo": false}));
    let _ = std::mem::take(&mut r.out);
    r.act(Action::Move { pane: Some(1), by: 1 });
    let mut found = false;
    for _ in 0..12 {
        if cursor(&ui).is_some_and(|c| c.starts_with("panel")) && ui.global::<RoutingState>().get_panel().iter().any(|row| row.cells.iter().any(|c| c.kind == 2 && c.cursor)) {
            found = true;
            break;
        }
        r.act(Action::Move { pane: Some(1), by: 1 });
    }
    assert!(found, "the cursor reached a level slider");
    r.act(Action::Confirm(None));
    assert!(ui.global::<RoutingState>().get_panel().iter().any(|row| row.cells.iter().any(|c| c.editing)), "being changed");
    r.act(Action::Move { pane: None, by: -2 });
    let sent = edits(&mut r);
    assert_eq!(sent, [("wires.mix", json!({"wire": 3, "level": 0.95, "pan": 0.0, "mute": false, "solo": false}))], "two clicks of a fortieth down from full");
    r.act(Action::Cancel(None));
    // M, S and On on the first source (a part's wire: set in its lane, but the buttons are there).
    let chorus = row_of(&ui, "Chorus (Keys) · part");
    r.tap_cell(chorus, 2);
    assert_eq!(edits(&mut r), [("wires.mix", json!({"wire": 3, "level": 0.95, "pan": 0.0, "mute": true, "solo": false}))]);
    r.tap_cell(chorus, 3);
    assert_eq!(edits(&mut r)[0].1["solo"], json!(true));
    r.tap_cell(chorus, 0);
    assert_eq!(edits(&mut r), [("wires.enable", json!({"wire": 3, "enabled": false}))]);
    r.tap_cell(audio, 4);
    assert_eq!(edits(&mut r), [("wires.disconnect", json!({"wire": 9}))]);
    // A note wire from the keys: its channel, any, then 1 to 16 by the dial; a tap steps it round.
    r.tap_chevron(1, at(&ui, "Bass")); // the Bass part open: its synth is a box
    r.tap(1, at(&ui, "Bass"));
    let rows = panel(&ui);
    let notes = rows.iter().find(|r| r.0 == "Keys & MIDI").unwrap();
    assert_eq!(notes.1, ["On", "Wire channel: any", "Disconnect"], "{rows:?}");
    let note_row = row_of(&ui, "Keys & MIDI");
    r.tap_cell(note_row, 1);
    assert_eq!(edits(&mut r), [("wires.zones", json!({"wire": 30, "zones": [{"keys": [0, 127], "velocities": [0, 1], "channels": 1, "transpose": 0, "curve": 1, "fixedVelocity": 0}]}))]);
    assert_eq!(panel(&ui).iter().find(|r| r.0 == "Keys & MIDI").unwrap().1[1], "Wire channel: 1");
}

// A plugin outside the parts can be turned off, and removed (its wires reconnected); a part's plugin is opened in
// its part instead and has no Remove.
#[test]
fn a_plugin_is_turned_off_removed_or_opened_in_its_part() {
    let mut s = Screen::sized(1280, 720);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    r.tap(1, at(&ui, "Master FX"));
    press_cell(&mut r, &ui, "Master FX", "Turn off");
    assert_eq!(edits(&mut r), [("graph.enable", json!({"node": 8, "enabled": false}))]);
    let mut g = graph();
    g["nodes"][4]["enabled"] = json!(false);
    r.graph(&g);
    assert_eq!(panel(&ui)[0].1, ["Connect audio to...", "Turn on", "Remove"]);
    assert!(s.ui.global::<RoutingState>().get_boxes().iter().find(|b| b.name == "Master FX").unwrap().off);
    press_cell(&mut r, &ui, "Master FX", "Turn on");
    assert_eq!(edits(&mut r), [("graph.enable", json!({"node": 8, "enabled": true}))]);
    press_cell(&mut r, &ui, "Master FX", "Remove");
    let sent = std::mem::take(&mut r.out);
    assert_eq!(sent[0].1, "graph.remove");
    assert_eq!(sent[0].2, json!({"node": 8, "userWires": "reconnect"}));
    assert_eq!(sent[1].1, "graph.commit");
    assert_eq!(panel(&ui)[0].0, "Select a plugin to wire it, or to set the level of each source into it.", "what was removed is not chosen");
    // Expanded, a part's plugin: Open part, no Remove.
    r.tap_tool(1);
    r.graph(&g);
    r.tap(1, at(&ui, "Chorus"));
    assert_eq!(panel(&ui)[0], ("Chorus  ·  Keys".to_string(), vec!["Connect audio to...".to_string(), "Turn off".into(), "Open part".into()]));
    assert_eq!(press_cell(&mut r, &ui, "Chorus  ·  Keys", "Open part"), Outcome::OpenPart(10));
    r.tap_tool(1); // as it was
}

// + Plugin lists the plugins the scanner found (not Split and Join), "Finding plugins..." while it looks, and adds
// the choice as a node named uniquely outside the parts (committed when the engine says it made it).
#[test]
fn a_plugin_is_added_outside_the_parts() {
    let mut s = Screen::sized(1280, 720);
    let mut r = routing_screen(&mut s, &graph());
    r.tap_tool(0);
    let m = menu_of(&mut r, &s);
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Finding plugins..."]);
    let plugin = |name: &str, id: &str| Plugin { path: format!("/p/{name}.clap"), id: id.into(), name: name.into(), vendor: "Oscairn".into(), features: vec![] };
    r.plugins(vec![plugin("Reverb", "fx.reverb"), plugin("Hall", "fx.hall"), plugin("Split", "org.oscairn.split")]);
    r.tap_tool(0);
    let mut m = menu_of(&mut r, &s);
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Hall", "Reverb"], "sorted, without Split and Join");
    choose(&mut m, &mut r, &["Hall"]);
    let sent = std::mem::take(&mut r.out);
    assert_eq!((sent[0].1, sent[0].2.clone()), ("graph.add", json!({"name": "Hall 2", "path": "/p/Hall.clap", "pluginId": "fx.hall"})), "Hall is taken: Hall 2");
    assert!(!r.reply(Request::Add, Ok(json!({"node": 12}))));
    assert_eq!(std::mem::take(&mut r.out).iter().map(|o| o.1).collect::<Vec<_>>(), ["graph.commit"]);
    assert!(r.reply(Request::Add, Err(crate::client::Error { code: -32000, message: "no".into() })));
}

// Parts collapsed is kept beside the theme choice as the JUCE app keeps it; expanded, each part is a frame holding
// its plugins with a chevron to collapse it; collapsed, a part's box expands by its chevron or the panel's button.
#[test]
fn parts_collapse_and_expand_and_the_choice_is_kept() {
    let mut s = Screen::sized(1280, 900);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    let state = ui.global::<RoutingState>();
    // One part by its chevron.
    r.tap_chevron(1, at(&ui, "Bass"));
    assert_eq!(state.get_frames().row_count(), 1);
    assert!(boxes(&ui).contains(&"Bass".to_string()), "its plugin is a box of its own now");
    assert_eq!(state.get_frames().row_data(0).unwrap().chevron, 2, "the frame collapses it");
    r.tap_chevron(0, 0);
    assert_eq!(state.get_frames().row_count(), 0);
    // The panel's button.
    r.tap(1, at(&ui, "Keys"));
    press_cell(&mut r, &ui, "Keys", "Expand");
    assert_eq!(state.get_frames().row_count(), 1);
    assert_eq!(panel(&ui)[0].1, ["Open part", "Collapse"]);
    press_cell(&mut r, &ui, "Keys", "Collapse");
    assert_eq!(state.get_frames().row_count(), 0);
    // A frame chosen by its head shows the part, and again lets go of it.
    r.tap_chevron(1, at(&ui, "Keys"));
    r.tap(0, 0);
    assert_eq!(panel(&ui)[0].0, "Select a plugin to wire it, or to set the level of each source into it.", "Keys was chosen: this lets go");
    r.tap(0, 0);
    assert_eq!(panel(&ui)[0].0, "Keys");
    r.tap_chevron(0, 0);
    // The choice: off, every part a frame, kept as "expanded"; on again, "collapsed".
    let file = crate::settings::config_dir().unwrap().join("routing-view.txt");
    let _file = super::ROUTING_FILE.lock().unwrap_or_else(|e| e.into_inner()); // a shell starting meanwhile would read "expanded"
    r.tap_tool(1);
    assert_eq!(state.get_frames().row_count(), 3);
    assert!(state.get_boxes().iter().all(|b| !b.part));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "expanded\n");
    assert!(!crate::settings::routing_collapsed());
    assert!(!r.bar_tools()[1].on);
    r.tap_tool(1);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "collapsed\n");
    assert!(crate::settings::routing_collapsed());
}

// A lane of the chosen plugin: Connect lane to... lists a submenu per lane, each with Control (the others and a new
// one) and the parameters of the plugins; a new control is added, made the lane's source, committed and set to the
// middle; a parameter becomes a relative target of the lane's control; Disconnect takes a lane's lines away.
#[test]
fn a_lane_is_connected_to_a_control_or_a_parameter_and_disconnected() {
    let mut s = Screen::sized(1280, 900);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    r.tap(1, at(&ui, "LFO"));
    let rows = panel(&ui);
    assert_eq!(rows[0].1, ["Connect lane to...", "Turn off", "Remove"]);
    assert!(rows.iter().any(|r| r.0 == "Wave → control Wobble (1 target)" && r.1 == ["Disconnect"]), "{rows:?}");
    assert!(rows.iter().any(|r| r.0 == "Step → Hall · 2" && r.1 == ["Disconnect"]), "the parameter's number until its name is read: {rows:?}");
    // The names are asked of the plugin the lane reaches.
    let asked: Vec<_> = r.out.iter().map(|o| o.0.clone()).collect();
    assert!(asked.contains(&Request::List(4)), "{asked:?}");
    r.reply(Request::List(4), Ok(json!({"parameters": [{"id": 2, "name": "Decay", "flags": 32}, {"id": 3, "name": "Size", "flags": 32 | 8}, {"id": 4, "name": "Hid", "flags": 4}]})));
    assert!(panel(&ui).iter().any(|r| r.0 == "Step → Hall · Decay"));
    let _ = std::mem::take(&mut r.out);
    // The menu waits for the rest of the plugins.
    press_cell(&mut r, &ui, "LFO", "Connect lane to...");
    assert!(r.take_menu().is_none());
    for n in [1, 2, 3, 8] {
        r.reply(Request::List(n), Ok(json!({"parameters": [{"id": 1, "name": "Level", "flags": 32}, {"id": 2, "name": "Meter", "flags": 8}]})));
    }
    let _ = std::mem::take(&mut r.out);
    let mut m = menu_of(&mut r, &s);
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Wave", "Step"], "a submenu per lane");
    m.act(Action::Confirm(None));
    let items: Vec<_> = m.labels().iter().map(|l| l.0.clone()).collect();
    assert_eq!(items.first().map(String::as_str), Some("Control"));
    assert!(items.contains(&"Hall · Reverb".to_string()) && items.contains(&"Rhodes · Keys".to_string()));
    m.act(Action::Cancel(None));
    // A new control for the Wave lane.
    choose(&mut m, &mut r, &["Wave", "Control", "New control"]);
    let sent = std::mem::take(&mut r.out);
    assert_eq!((sent[0].1, sent[0].2.clone()), ("controls.add", json!({"name": "LFO"})));
    let key = match &sent[0].0 {
        Request::LaneAdd(k) => *k,
        other => panic!("{other:?}"),
    };
    r.reply(Request::LaneAdd(key), Ok(json!({"control": 7})));
    let sent = std::mem::take(&mut r.out);
    assert_eq!(sent.iter().map(|o| (o.1, o.2.clone())).collect::<Vec<_>>(), [
        ("controls.source", json!({"control": 7, "node": 9, "lane": 0})), ("graph.commit", json!({})), ("controls.set", json!({"control": 7, "value": 0.5}))
    ]);
    // An existing control (the Step lane has none).
    press_cell(&mut r, &ui, "LFO", "Connect lane to...");
    let mut m = menu_of(&mut r, &s);
    choose(&mut m, &mut r, &["Step", "Control", "Wobble"]);
    assert_eq!(std::mem::take(&mut r.out).iter().map(|o| (o.1, o.2.clone())).collect::<Vec<_>>(), [
        ("controls.source", json!({"control": 5, "node": 9, "lane": 1})), ("graph.commit", json!({}))
    ]);
    // A parameter of Master FX (Level; Meter is read only) on the Wave lane, whose control Wobble has one target.
    press_cell(&mut r, &ui, "LFO", "Connect lane to...");
    let mut m = menu_of(&mut r, &s);
    choose(&mut m, &mut r, &["Wave", "Master FX", "Level"]);
    let sent = std::mem::take(&mut r.out);
    assert_eq!(sent.iter().map(|o| o.1).collect::<Vec<_>>(), ["controls.targets", "graph.commit"]);
    assert_eq!(sent[0].2["control"], json!(5), "the lane's own control");
    let targets = sent[0].2["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!((targets[1]["kind"].clone(), targets[1]["node"].clone(), targets[1]["param"].clone(), targets[1]["relative"].clone(), targets[1]["start"].clone(), targets[1]["end"].clone()), (json!("parameter"), json!(8), json!(1), json!(true), json!(0.0), json!(1.0)));
    // Disconnect lane Step: the mapping and the lane's control source, then a commit.
    press_cell(&mut r, &ui, "Step → Hall · Decay", "Disconnect");
    assert_eq!(std::mem::take(&mut r.out).iter().map(|o| (o.1, o.2.clone())).collect::<Vec<_>>(), [("graph.unmap", json!({"from": 9, "lane": 1})), ("graph.commit", json!({}))]);
    press_cell(&mut r, &ui, "Wave → control Wobble (1 target)", "Disconnect");
    assert_eq!(std::mem::take(&mut r.out).iter().map(|o| (o.1, o.2.clone())).collect::<Vec<_>>(), [("controls.source", json!({"control": 5})), ("graph.commit", json!({}))]);
    // The control's pill opens the Controls page.
    assert_eq!(r.tap(2, 0), Outcome::Controls);
}

// Meters on: an audio wire that carries sound takes the color of its level (green, yellow from -12 dB, red from -3);
// off or below -60 dB it keeps its own; the notes' wires never.
#[test]
fn wires_take_the_color_of_their_level_with_meters_on() {
    let mut s = Screen::sized(1280, 720);
    let mut r = routing_screen(&mut s, &graph());
    let state = s.ui.global::<RoutingState>();
    let before: Vec<_> = state.get_wires().iter().map(|w| (w.color, w.alpha)).collect();
    r.tap_tool(2);
    assert!(r.metering() && r.bar_tools()[2].on);
    r.meters(&json!({"meters": [{"wire": 5, "peak": 0.9, "channels": [0.9]}, {"wire": 8, "peak": 0.3}, {"wire": 6, "peak": 0.05}, {"wire": 1, "peak": 0.9}, {"wire": 3, "peak": 0.0004}]}));
    let after: Vec<_> = state.get_wires().iter().map(|w| (w.color, w.alpha)).collect();
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert!(changed >= 3, "the wires that sound changed color: {changed}");
    let theme_colors = (state.get_wires().iter().map(|w| w.color).collect::<Vec<_>>(), ());
    assert!(theme_colors.0.iter().any(|c| *c == s.ui.global::<crate::Theme>().get_danger()), "red at 0.9");
    assert!(theme_colors.0.iter().any(|c| *c == s.ui.global::<crate::Theme>().get_warning()), "yellow at 0.3");
    assert!(theme_colors.0.iter().any(|c| *c == s.ui.global::<crate::Theme>().get_positive()), "green at 0.05");
    r.tap_tool(2);
    assert!(!r.metering());
    assert!(state.get_wires().iter().all(|w| w.color != s.ui.global::<crate::Theme>().get_danger()), "off: the wires' own colors");
}

fn pixel_theme() -> theme::Loaded {
    theme_of(json!({"version": 7, "id": "pixels", "name": "Pixels", "fontScale": 1, "radius": 6,
        "font": {"regular": "fonts/Inter-Regular.ttf", "license": "fonts/OFL.txt"},
        "colors": {"background": "#000000", "surface": "#202020", "raised": "#303030", "border": "#808080", "text": "#FFFFFF", "muted": "#A0A0A0", "accent": "#00FF00",
                   "positive": "#5AD9A4", "danger": "#FF8D87"},
        "style": {"buttons": "rounded", "edges": "none", "depth": "flat", "sliders": "line", "tabs": "segmented", "marks": "square", "sections": "separate", "rows": "left"},
        "wires": {"audio": "#0000FF", "notes": "#00FF00", "disabled": "#808080", "draining": "#FF8800", "unconnected": "#FF0000", "control": "#00FFFF"}}))
}

/// Whether a pixel is the wire's blue (alpha over black).
fn blue(p: &slint::Rgb8Pixel) -> bool {
    p.b > 70 && p.r < 50 && p.g < 50
}

/// How far each audio wire's ends are from its ports in the rendered pixels at this window size and scale: a
/// wire's color is found within a line's width of where it leaves its source box's right edge and enters its
/// target's left edge, and the box is on the other side of that edge (not the wire's color). The drawing is
/// scrolled over the canvas in steps, so every end is measured while it is in view; an end never in view is a miss.
fn miss(w: usize, h: usize, scale: f32) -> Vec<String> {
    let mut s = Screen::sized(w, h);
    s.set_scale(scale);
    theme::apply(&s.ui, &pixel_theme());
    let mut r = routing_screen(&mut s, &graph());
    s.frame();
    r.shown();
    s.frame();
    let ui = s.ui.clone_strong();
    let state = ui.global::<RoutingState>();
    let (cx, cy) = (f64::from(state.get_canvas_x()), f64::from(state.get_canvas_y()) + f64::from(ui.global::<crate::Theme>().get_bar_height()));
    let (cw, ch) = (f64::from(state.get_canvas_width()), f64::from(state.get_canvas_height()));
    let (max_x, max_y) = ((f64::from(state.get_content_width()) - cw).max(0.), (f64::from(state.get_content_height()) - ch).max(0.));
    let sc = f64::from(scale);
    let mut bad = vec![];
    let at = |s: &Screen, x: f64, y: f64| -> slint::Rgb8Pixel { s.pixels[(y.round().clamp(0., s.h as f64 - 1.) as usize) * s.w + (x.round().clamp(0., s.w as f64 - 1.) as usize)] };
    let found = |s: &Screen, x: f64, y: f64, reach: f64| (-(reach as i32)..=reach as i32).any(|dy| (-1..=1).any(|dx| blue(&at(s, x + f64::from(dx), y + f64::from(dy)))));
    let ends = r.wire_ends();
    assert!(!ends.is_empty());
    let mut measured = std::collections::BTreeSet::new();
    let mut pan_y = 0.;
    for _ in 0..20 {
        let mut pan_x = 0.;
        for _ in 0..20 {
            state.set_pan_x(pan_x as f32);
            state.set_pan_y(pan_y as f32);
            s.frame();
            s.frame();
            for (id, from, to, lines) in &ends {
                let reach = (3. + *lines as f64 * 1.2 * 2.5 / 2.) * sc.max(1.);
                // An end is measured when the stretch of wire after it and the box before it are inside the canvas.
                let inside = |p: (f64, f64)| p.0 - pan_x > 8. + reach && p.0 - pan_x < cw - 8. - reach && p.1 - pan_y > reach && p.1 - pan_y < ch - reach;
                let (sx, sy) = ((cx + from.0 - pan_x) * sc, (cy + from.1 - pan_y) * sc);
                let (ex, ey) = ((cx + to.0 - pan_x) * sc, (cy + to.1 - pan_y) * sc);
                let step = 2. * sc;
                if inside(*from) {
                    measured.insert((*id, 0));
                    if !found(&s, sx + step, sy, reach) {
                        bad.push(format!("wire {id} not found after its source's edge at {sx:.1},{sy:.1} ({w}x{h}@{scale}, panned {pan_x},{pan_y})"));
                    }
                    if blue(&at(&s, sx - step, sy)) {
                        bad.push(format!("wire {id} drawn over its source box at {w}x{h}@{scale}"));
                    }
                }
                if inside(*to) {
                    measured.insert((*id, 1));
                    if !found(&s, ex - step, ey, reach) {
                        bad.push(format!("wire {id} not found before its target's edge at {ex:.1},{ey:.1} ({w}x{h}@{scale}, panned {pan_x},{pan_y})"));
                    }
                }
            }
            if pan_x >= max_x {
                break;
            }
            pan_x = (pan_x + cw / 2.).min(max_x);
        }
        if pan_y >= max_y {
            break;
        }
        pan_y = (pan_y + ch / 2.).min(max_y);
    }
    for (id, ..) in &ends {
        for end in 0..2 {
            if !measured.contains(&(*id, end)) {
                bad.push(format!("wire {id}'s end {end} was never in view ({w}x{h}@{scale}, content {:.0}x{:.0} in {cw:.0}x{ch:.0})", max_x + cw, max_y + ch));
            }
        }
    }
    if !bad.is_empty() {
        png(&s, &format!("routing-pixels-{w}x{h}"));
    }
    bad
}

// The wires meet their ports: with every point of the drawing laid out once and the wires' paths unscaled
// (`fit: preserve`), measured in the rendered pixels at the keyboard's 1280x720, a small device's 800x480, the
// FANTOM-07's scale 1.55 and a large device at scale 2.
#[test]
fn wires_meet_the_boxes_ports_at_every_window_size_and_scale() {
    let mut bad = vec![];
    for (w, h, scale) in [(1280, 720, 1.0), (800, 480, 1.0), (1280, 720, 1.55), (1920, 1080, 2.0), (700, 700, 1.0)] {
        bad.extend(miss(w, h, scale));
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

// The page in every built-in theme and the two odd ones, at the keyboard's 1280x720, a small device's 800x480, the
// FANTOM-07's scale and a tall narrow window: pictures in target/screens and target/theme-shots for a look by eye;
// a plugin chosen, a part chosen, a mix row being changed, and a menu over the page, expanded and collapsed.
#[test]
fn the_page_draws_in_every_theme_at_several_sizes() {
    for (w, h) in [(1280, 720), (800, 480), (700, 900)] {
        for collapsed in [true, false] {
            let mut s = Screen::sized(w, h);
            let mut r = routing_screen(&mut s, &graph());
            if !collapsed {
                r.set_collapsed(false);
                r.shown();
            }
            s.ui.global::<crate::Bar>().set_performance("Grand Piano".into());
            set_title(&s.ui, &["Parts", "Routing"]);
            let ui = s.ui.clone_strong();
            r.tap(1, at(&ui, "Master FX"));
            r.tap_tool(2);
            r.meters(&json!({"meters": [{"wire": 5, "peak": 0.9}, {"wire": 8, "peak": 0.3}, {"wire": 6, "peak": 0.05}]}));
            mv(&mut r, Some(0), 1);
            mv(&mut r, Some(0), 3);
            for t in &all_themes() {
                theme::apply(&s.ui, t);
                assert!(s.frame().is_some(), "{}: drawn", t.id);
                r.shown(); // the drawing is fitted to the space the theme leaves
                s.frame();
                let tag = if collapsed { "" } else { "-open" };
                shot(&s, &format!("routing{tag}-{}-{w}x{h}", t.id));
                if t.id == "oscairn-dark" {
                    png(&s, &format!("routing{tag}-{w}x{h}"));
                }
            }
            theme::apply(&s.ui, &all_themes()[0]);
            s.frame();
            r.shown();
            s.frame();
            r.tap(1, at(&ui, "LFO"));
            s.frame();
            png(&s, &format!("routing-lfo{}-{w}x{h}", if collapsed { "" } else { "-open" }));
            press_cell(&mut r, &ui, "LFO", "Connect lane to...");
            let _ = std::mem::take(&mut r.out);
            for n in [1, 2, 3, 4, 8] {
                r.reply(Request::List(n), Ok(json!({"parameters": [{"id": 1, "name": "Level", "flags": 32}, {"id": 2, "name": "Decay", "flags": 32}]})));
            }
            if let Some((title, items)) = r.take_menu() {
                let mut menu = crate::menu::Menu::open(&s.ui, &title, items);
                s.frame();
                menu.show();
                s.frame();
                png(&s, &format!("routing-menu{}-{w}x{h}", if collapsed { "" } else { "-open" }));
            }
        }
    }
    let mut s = Screen::new();
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    r.tap(1, at(&ui, "Master FX"));
    at_fantom_scale(&mut s, "routing", || r.shown());
}

/// The next request of this method, the others the page asked answered as an engine would.
fn want(f: &mut super::Fixture, method: &str, none: &serde_json::Value, graph: &serde_json::Value) -> serde_json::Value {
    loop {
        let r = request_in(f, method, none, graph);
        if r["method"] == method {
            return r;
        }
        f.reply(&r, json!({"result": {}}));
    }
}

// On the shell through the fake engine: the Parts list's Routing button opens the page and Back returns to the
// list; a shared row of the list opens it with that plugin chosen; Meters asks `wires.meters` on a short timer and
// only while on; Open part goes to the Part screen, which goes back to Routing; a control's pill opens Controls.
#[test]
fn routing_opens_from_the_parts_list_and_its_actions_reach_the_engine() {
    let mut shared = graph();
    for w in shared["wires"].as_array_mut().unwrap() {
        if matches!(w["id"].as_u64(), Some(3 | 5)) {
            w["owner"] = json!("user"); // wired by hand into Master FX: it is shared, a row of the list
        }
    }
    let mut f = play_on_the_shell("routing-shell", json!({"id": "p1", "name": "Grand", "unsaved": false}), shared.clone());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    ui.global::<crate::Bar>().invoke_action(5);
    assert_eq!(ui.get_screen(), 9);
    let tools = ui.global::<PartsState>().get_tools();
    assert_eq!((tools.row_data(2).unwrap().label.as_str(), tools.row_data(2).unwrap().enabled), ("Routing", true), "the button is on");
    ui.global::<PartsState>().invoke_tool(2);
    assert_eq!((ui.get_screen(), title(&ui)), (12, "Parts › Routing".to_string()));
    assert_eq!(ui.global::<crate::Bar>().get_actions().iter().map(|a| a.label.to_string()).collect::<Vec<_>>(), ["+ Plugin", "Parts collapsed", "Meters", "Undo Volume of Keys", "Redo", "Save", "Play", "Record", "Capture"], "the screen's own actions first");
    // The graph is asked and drawn; nothing is chosen.
    want(&mut f, "graph.get", &none, &shared);
    f.screen.frame();
    assert!(boxes(&ui).contains(&"Master FX".to_string()));
    assert_eq!(panel(&ui)[0].0, "Select a plugin to wire it, or to set the level of each source into it.");
    // A press with the cursor chooses a plugin and a connect reaches the engine (menus over the page take the keys).
    ui.global::<RoutingState>().invoke_tapped(1, at(&ui, "Master FX"));
    assert_eq!(panel(&ui)[0].0, "Master FX");
    ui.global::<RoutingState>().invoke_panel_press(0, 0);
    assert!(ui.global::<MenuState>().get_open(), "Connect audio to... is a menu over the page");
    crate::shell::act(Action::Move { pane: None, by: 1 });
    crate::shell::act(Action::Confirm(None));
    let r = want(&mut f, "graph.connect", &none, &shared);
    assert_eq!(r["params"], json!({"kind": "audio", "from": 8, "to": 4}));
    let r = want(&mut f, "graph.commit", &none, &shared);
    f.reply(&r, json!({"result": {"accepted": true}}));
    // Meters on: the wires' peaks are asked on a timer, off: no more.
    ui.global::<crate::Bar>().invoke_action(2); // Meters, the top bar's
    f.screen.frame();
    std::thread::sleep(std::time::Duration::from_millis(120));
    f.screen.frame();
    let r = want(&mut f, "wires.meters", &none, &shared);
    f.reply(&r, json!({"result": {"meters": [{"wire": 6, "peak": 0.9}]}}));
    let danger = ui.global::<crate::Theme>().get_danger();
    assert!(ui.global::<RoutingState>().get_wires().iter().any(|w| w.color == danger), "the wire that carries 0.9 is red");
    ui.global::<crate::Bar>().invoke_action(2); // Meters, the top bar's
    // Cancel with the cursor on nothing goes back to the list.
    crate::shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 9);
    // The shared row opens Routing with that plugin chosen.
    let row = ui.global::<PartsState>().get_rows().iter().position(|r| r.name.as_str().starts_with("Shared")).expect("a shared row") as i32;
    ui.global::<PartsState>().invoke_open(row);
    assert_eq!(ui.get_screen(), 12);
    f.screen.frame();
    assert_eq!(panel(&ui)[0].0, "Master FX");
    // Open part: a part's box chosen, Open part, the Part screen; Cancel there goes back here.
    ui.global::<RoutingState>().invoke_tapped(1, at(&ui, "Keys"));
    assert_eq!(panel(&ui)[0].1, ["Open part", "Expand"]);
    ui.global::<RoutingState>().invoke_panel_press(0, 0);
    assert_eq!((ui.get_screen(), title(&ui)), (4, "Keys".to_string()));
    crate::shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 12, "back to Routing, not the list");
    // A control's pill goes to the Controls page; its Back returns to the list.
    ui.global::<RoutingState>().invoke_tapped(2, 0);
    assert_eq!(ui.get_screen(), 11);
    // The bar's Back from Controls goes to the list; the bar's Play from Routing goes to Play.
    ui.global::<crate::Bar>().invoke_back_tapped();
    assert_eq!(ui.get_screen(), 9);
    ui.global::<PartsState>().invoke_tool(2);
    ui.global::<crate::Bar>().invoke_action(6); // Play, after the page's own three
    assert_eq!(ui.get_screen(), 3, "Play");
    f.screen.frame();
}

// The keys and MIDI have a button for the on-screen keyboard where the profile offers one; a plugin that has a
// window of its own shows a disabled entry saying that view is not in this shell yet (the one item allowed to stay
// unported), asked with window.info when the plugin is chosen; a plugin that failed is outlined and said.
#[test]
fn the_keys_open_the_keyboard_and_a_plugins_own_view_is_listed_disabled() {
    let mut s = Screen::sized(1280, 720);
    let mut r = routing_screen(&mut s, &graph());
    let ui = s.ui.clone_strong();
    r.offer_keyboard(true);
    r.tap(1, at(&ui, "Keys & MIDI"));
    assert_eq!(panel(&ui)[0].1, ["Connect notes to...", "Keyboard"]);
    assert_eq!(press_cell(&mut r, &ui, "Keys & MIDI", "Keyboard"), Outcome::Keys(String::new()));
    r.tap(1, at(&ui, "Master FX"));
    assert!(std::mem::take(&mut r.out).iter().any(|o| o.0 == Request::Window(8)), "window.info is asked of the plugin chosen");
    r.reply(Request::Window(8), Ok(json!({"available": true})));
    let cells = ui.global::<RoutingState>().get_panel().row_data(0).unwrap().cells;
    let view = cells.iter().find(|c| c.label.as_str().starts_with("Plugin view")).expect("the entry");
    assert_eq!((view.label.as_str(), view.enabled), ("Plugin view: not available in this shell yet", false));
    // A disabled cell is not a stop of the cursor.
    r.act(Action::End(1));
    for _ in 0..40 {
        r.act(Action::Move { pane: Some(1), by: -1 });
        let on_view = ui.global::<RoutingState>().get_panel().iter().any(|row| row.cells.iter().any(|c| c.cursor && c.label.as_str().starts_with("Plugin view")));
        assert!(!on_view, "the cursor skips a cell that does nothing");
    }
    assert!(cursor(&ui).is_some_and(|c| c.starts_with("panel")), "the right knob put it on the panel");
    // A failed plugin.
    r.status(&json!({"failures": [{"node": 8, "name": "Master FX", "reason": "it crashed"}]}));
    assert!(ui.global::<RoutingState>().get_boxes().iter().find(|b| b.name == "Master FX").unwrap().failed);
    assert_eq!(ui.global::<RoutingState>().get_message().as_str(), "Master FX is silent: it crashed");
}

// A drawing larger than the canvas at a legible size scrolls: the dial moves the cursor over its places and the view
// follows it (the place is whole in the canvas), a drag moves the view, and no text of a box is under the theme's
// small size. At the keyboard's size (826x465 logical) and a small device's.
#[test]
fn a_drawing_larger_than_the_canvas_scrolls_with_the_cursor_and_a_drag_and_its_type_stays_legible() {
    for (size, scale) in [((1280, 720), 1.55), ((800, 480), 1.)] {
        let mut s = Screen::sized(size.0, size.1);
        s.set_scale(scale);
        let mut r = routing_screen(&mut s, &graph());
        let ui = s.ui.clone_strong();
        let state = ui.global::<RoutingState>();
        let what = format!("{size:?}@{scale}");
        let (cw, ch) = (state.get_canvas_width(), state.get_canvas_height());
        assert!(state.get_content_width() > cw, "{what}: the drawing is wider than the canvas ({} in {cw})", state.get_content_width());
        // The drawing is laid out at the type's own size, not shrunk to the canvas.
        assert!(state.get_scale() >= 1.0, "{what}: scale {}", state.get_scale());
        assert!(state.get_base() * state.get_scale() >= ui.global::<crate::Theme>().get_small() - 0.01, "{what}: type under the small size");
        // The dial through every place: the one with the cursor is whole in the view.
        let mut moved = false;
        for _ in 0..12 {
            mv(&mut r, Some(0), 1);
            s.frame();
            r.shown();
            s.frame();
            let (px, py) = (state.get_pan_x(), state.get_pan_y());
            moved |= px > 0.5 || py > 0.5;
            let boxes: Vec<crate::RoutingBox> = state.get_boxes().iter().chain(state.get_controls().iter()).chain(state.get_frames().iter()).collect();
            let b = boxes.iter().find(|b| b.cursor).unwrap_or_else(|| panic!("{what}: a cursor"));
            let (x, y) = (b.x - px, b.y - py);
            assert!(x >= -0.5 && x + b.w <= cw + 0.5 && y >= -0.5 && y + b.h <= ch + 0.5, "{what}: {} at {x},{y} {}x{} is not whole in {cw}x{ch}", b.name, b.w, b.h);
        }
        assert!(moved, "{what}: the view followed the cursor");
        // A drag over the canvas moves the view the other way, as a finger does.
        mv(&mut r, Some(0), -20);
        r.act(Action::Cancel(None));
        state.set_pan_x(0.);
        state.set_pan_y(0.);
        s.frame();
        let (cx, cy) = (state.get_canvas_x(), state.get_canvas_y() + ui.global::<crate::Theme>().get_bar_height());
        let window = ui.window();
        let at = |dx: f32| slint::LogicalPosition::new(cx + cw - 4. + dx, cy + 4.);
        use slint::platform::{PointerEventButton, WindowEvent};
        window.dispatch_event(WindowEvent::PointerPressed { position: at(0.), button: PointerEventButton::Left });
        for step in 1..=10 {
            window.dispatch_event(WindowEvent::PointerMoved { position: at(-12. * step as f32) });
            s.frame();
        }
        window.dispatch_event(WindowEvent::PointerReleased { position: at(-120.), button: PointerEventButton::Left });
        s.frame();
        assert!(state.get_pan_x() > 40., "{what}: a drag of 120 moved the view {}", state.get_pan_x());
    }
}
