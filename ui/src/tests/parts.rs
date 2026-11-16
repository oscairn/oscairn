// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Parts list (B1 to B5, B8 to B10): read as the JUCE page 0 reads graph.get, the cursor along
//! the cells of a row and down the rows, each edit's request, touch, whole views that follow the
//! window, and on the shell (Play's Edit, Back from a part) through the fake engine.

use super::bar::{play_on_the_shell, request_in};
use super::*;
use crate::parts::{Outcome, Screen as Parts};
use crate::{PartState, PartsState, PlayState};

pub fn parts_graph() -> serde_json::Value {
    let node = |id: u64, name: &str, inputs: u64, outputs: u64, notes_in: u64, enabled: bool| {
        let ports = |n: u64| if n > 0 { json!([{"channels": n}]) } else { json!([]) };
        json!({"id": id, "name": name, "plugin": {"name": name}, "audioInputs": ports(inputs), "audioOutputs": ports(outputs), "noteInputs": notes_in, "noteOutputs": 0, "enabled": enabled})
    };
    let input = |devices: serde_json::Value, keys: [i64; 2], transpose: i64| json!({"kind": "notes", "port": 0, "devices": devices, "zones": [{"keys": keys, "velocities": [0, 1], "channels": 65535, "transpose": transpose}]});
    json!({
        "nodes": [node(1, "Rhodes", 0, 2, 1, true), node(2, "Chorus", 2, 2, 0, false), node(3, "Hall", 2, 2, 0, true), node(4, "Bass", 0, 1, 1, true),
                  node(5, "Pad", 0, 2, 1, true), node(6, "Echo", 2, 2, 0, true), node(8, "Master FX", 2, 2, 0, true)],
        "parts": [
            {"id": 10, "name": "Keys", "level": 0.5, "mute": false, "solo": false, "chain": [2, 1], "bus": 0, "sends": [{"bus": 12, "level": 0.5}], "input": input(json!([]), [36, 96], -12)},
            {"id": 11, "name": "Low", "level": 1.0, "mute": false, "solo": false, "chain": [{"split": 1}], "bus": 0, "sends": [],
             "splits": [{"id": 1, "kind": "leftRight", "splitter": 0, "joiner": 0, "lanes": [{"chain": [4]}, {"chain": []}]}],
             "input": input(json!([{"kind": "usbNamed", "pattern": "KeyLab", "match": "contains", "port": 0}]), [0, 59], 0)},
            {"id": 12, "name": "Reverb", "level": 0.8, "mute": false, "solo": false, "chain": [3], "bus": 0, "sends": [], "input": {"kind": "bus"}},
            {"id": 13, "name": "Delay", "level": 0.6, "mute": false, "solo": false, "chain": [6], "bus": 0, "sends": [], "input": {"kind": "bus"}},
            {"id": 14, "name": "Pad", "level": 0.8, "mute": true, "solo": false, "chain": [5], "bus": 12, "sends": [{"bus": 12, "level": 0.3}, {"bus": 13, "level": 0.7}], "input": input(json!([]), [0, 127], 0)},
            {"id": 15, "name": "Empty", "level": 1.0, "mute": false, "solo": false, "chain": [], "bus": 0, "sends": [], "input": {"kind": "none"}},
        ],
        "wires": [
            {"id": 1, "owner": "user", "kind": "audio", "from": 2, "to": 8},
            {"id": 2, "owner": "user", "kind": "audio", "from": 5, "to": 8},
            {"id": 3, "owner": "user", "kind": "audio", "from": 3, "to": 4294967295u64},
        ],
        "scenes": [{"id": 1, "name": "Verse"}], "scene": 1, "master": {"level": 1.0, "pan": 0.0},
    })
}

/// The Parts list on its own in a window of `w` x `h`, `graph` read and laid out.
pub fn parts_screen(s: &mut Screen, graph: &serde_json::Value) -> Parts {
    let mut parts = Parts::new(&s.ui);
    s.ui.set_screen(9);
    s.frame();
    parts.graph(graph);
    s.frame();
    parts.shown(); // as the shell does when the window's size is known
    parts
}

type Cells = (String, String, i32, bool);

/// Each row of the view: its name, its second line, the cell the cursor is on and whether it is being changed.
fn rows(ui: &Shell) -> Vec<Cells> {
    ui.global::<PartsState>().get_rows().iter().map(|r| (r.name.to_string(), r.sound.to_string(), r.cell, r.editing)).collect()
}

fn tools(ui: &Shell) -> Vec<(String, bool, bool)> {
    ui.global::<PartsState>().get_tools().iter().map(|t| (t.label.to_string(), t.enabled, t.cursor)).collect()
}

fn cursor(ui: &Shell) -> Option<(usize, i32)> {
    rows(ui).iter().position(|r| r.2 >= 0).map(|i| (i, rows(ui)[i].2))
}

fn edits(p: &mut Parts) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(&mut p.out).into_iter().map(|(_, m, p)| (m, p)).filter(|(m, _)| *m != "graph.commit").collect()
}

fn requests(p: &mut Parts) -> Vec<&'static str> {
    std::mem::take(&mut p.out).into_iter().map(|(_, m, _)| m).collect()
}

fn mv(p: &mut Parts, pane: Option<u8>, by: i32) -> Outcome {
    p.act(Action::Move { pane, by })
}

// graph.get as PartsModel.cpp's readParts reads it: parts first, then buses, then the nodes several parts
// are wired into; each part's sound, input (with "No device:" when its devices are not connected), chain
// boxes (a split named after its lanes), sends in the order of the buses, and what a bus is fed by.
#[test]
fn the_list_reads_parts_buses_and_shared_nodes_as_the_juce_page_does() {
    let (parts, buses, shared) = crate::parts::read(&parts_graph(), &[]);
    let names = |rows: &[crate::parts::Row]| rows.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
    assert_eq!((names(&parts), names(&buses)), (["Keys", "Low", "Pad", "Empty"].map(String::from).to_vec(), ["Reverb", "Delay"].map(String::from).to_vec()));
    let sound: Vec<_> = parts.iter().map(|r| r.sound.as_str()).collect();
    assert_eq!(sound, ["Rhodes", "Bass", "Pad", ""]);
    let input: Vec<_> = parts.iter().map(|r| r.input.as_str()).collect();
    assert_eq!(input, ["Any device · C2–C7 · −12", "No device: “KeyLab” · C-1–B3", "Any device · all keys", "Its own notes"]);
    let usb = [json!({"kind": "usb", "name": "KeyLab 61", "port": 0}), json!({"kind": "builtIn", "name": "Built in", "port": 0})];
    assert_eq!(crate::parts::read(&parts_graph(), &usb).0[1].input, "“KeyLab” · C-1–B3", "with the device connected");
    let chain: Vec<Vec<_>> = parts.iter().map(|r| r.chain.iter().map(|c| (c.name.as_str(), c.off)).collect()).collect();
    assert_eq!(chain, [vec![("Chorus", true), ("Rhodes", false)], vec![("Bass | Right", false)], vec![("Pad", false)], vec![]], "off marks; a split is named after its lanes");
    // Sends in the order of the buses; a bus says who feeds it (a part's output or a send).
    assert_eq!(parts.iter().map(|r| r.sends.clone()).collect::<Vec<_>>(), [vec![Some(0.5), None], vec![None, None], vec![Some(0.3), Some(0.7)], vec![None, None]]);
    assert_eq!(buses.iter().map(|r| r.input.as_str()).collect::<Vec<_>>(), ["From Keys, Pad, Pad", "From Pad"]);
    // The node outside every part that two parts are wired into; the one wired to the output is no one's.
    assert_eq!(shared.len(), 1);
    assert_eq!((shared[0].node, shared[0].name.as_str(), shared[0].fed_by.clone()), (8, "Master FX", vec!["Keys".to_string(), "Pad".to_string()]));
}

// The cursor has two dimensions: the left knob along the cells of a row (the part, its level, its mute, each
// send), the right knob down the rows, the dial through all of them in reading order and on to the buttons.
// The first move shows it; nothing opens or changes until a press.
#[test]
fn the_cursor_moves_along_cells_and_down_rows() {
    let mut s = Screen::new();
    let mut p = parts_screen(&mut s, &parts_graph());
    let ui = &s.ui;
    assert_eq!(rows(ui).len(), 7, "four parts, two buses, one shared node");
    assert_eq!(cursor(ui), None);
    mv(&mut p, Some(0), 1);
    assert_eq!(cursor(ui), Some((0, 0)), "the first move shows the cursor on the first part");
    mv(&mut p, Some(0), 1);
    mv(&mut p, Some(0), 1);
    assert_eq!(cursor(ui), Some((0, 2)), "level, then mute");
    mv(&mut p, Some(0), 10);
    assert_eq!(cursor(ui), Some((0, 4)), "held at the last send (a part has the part, level, mute and two sends)");
    mv(&mut p, Some(1), 1);
    assert_eq!(cursor(ui), Some((1, 4)), "down a row, the same cell");
    mv(&mut p, Some(1), 1);
    mv(&mut p, Some(1), 1);
    mv(&mut p, Some(1), 1); // the buses have no sends: the cell is the last they have
    assert_eq!(cursor(ui), Some((4, 2)));
    mv(&mut p, Some(1), 1);
    mv(&mut p, Some(1), 1);
    assert_eq!(cursor(ui), Some((6, 0)), "the shared node has the one cell");
    // The dial goes cell by cell, row by row, then the buttons.
    p.act(Action::End(-1));
    assert_eq!(cursor(ui), Some((0, 0)));
    for _ in 0..4 {
        mv(&mut p, None, 1);
    }
    assert_eq!(cursor(ui), Some((0, 4)));
    mv(&mut p, None, 1);
    assert_eq!(cursor(ui), Some((1, 0)), "on to the next row");
    mv(&mut p, None, -1);
    assert_eq!(cursor(ui), Some((0, 4)), "and back");
    p.act(Action::End(1));
    assert_eq!(tools(ui).iter().position(|t| t.2), Some(4), "End: the last button");
    mv(&mut p, None, -1);
    mv(&mut p, None, -1);
    mv(&mut p, None, -1);
    mv(&mut p, None, -1);
    mv(&mut p, None, -1);
    assert_eq!(cursor(ui), Some((6, 0)), "up past the first button: the last row");
    assert!(tools(ui).iter().all(|t| !t.2));
    // Arrow keys: left and right along the row, up and down the rows.
    p.act(Action::Arrow { dx: 0, dy: -1 });
    p.act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(cursor(ui), Some((5, 1)));
    p.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(cursor(ui), Some((5, 0)));
    assert!(edits(&mut p).is_empty(), "moving sends nothing");
    // Cancel drops the cursor, then goes back.
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(cursor(ui), None);
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Back);
}

// A press on the part opens its screen; on a level or a send it starts a change the dial then makes, a fortieth
// of the slider a click, each step `parts.level` or `parts.send` with a commit after; a press or Cancel ends
// it; a send taken to nothing is removed; on mute it toggles.
#[test]
fn a_press_opens_a_part_changes_a_level_or_a_send_and_toggles_mute() {
    let mut s = Screen::new();
    let mut p = parts_screen(&mut s, &parts_graph());
    let ui = &s.ui;
    mv(&mut p, Some(0), 1);
    assert_eq!(p.act(Action::Confirm(None)), Outcome::Open(10));
    // Level: a press, then the dial; the move out of the row waits for the end of the change.
    mv(&mut p, Some(0), 1);
    assert_eq!(p.act(Action::Confirm(None)), Outcome::Stay);
    assert_eq!(cursor(ui), Some((0, 1)));
    assert!(rows(ui)[0].3, "being changed");
    mv(&mut p, None, 1);
    let sent = std::mem::take(&mut p.out);
    assert_eq!(sent.iter().map(|o| o.1).collect::<Vec<_>>(), ["parts.level", "graph.commit"]);
    assert_eq!(sent[0].2, json!({"part": 10, "level": 0.525, "mute": false}));
    mv(&mut p, None, -1);
    assert_eq!(edits(&mut p)[0].1["level"], json!(0.5), "a click back is where it was");
    mv(&mut p, Some(1), 1);
    assert_eq!(cursor(ui), Some((0, 1)), "the right knob does not leave a value being changed");
    p.act(Action::Cancel(None));
    assert!(!rows(ui)[0].3 && cursor(ui) == Some((0, 1)), "Cancel ends the change, the cursor stays");
    // Level to the ends is held there, and nothing is sent for a value that did not move.
    p.act(Action::Confirm(None));
    mv(&mut p, None, 100);
    assert_eq!(edits(&mut p).last().unwrap().1["level"], json!(1.0));
    mv(&mut p, None, 1);
    assert!(edits(&mut p).is_empty(), "nothing at the end");
    p.act(Action::Confirm(None)); // a press ends it as well
    assert!(!rows(ui)[0].3);
    // Mute toggles on a press, keeping the level.
    mv(&mut p, Some(0), 1);
    p.act(Action::Confirm(None));
    assert_eq!(edits(&mut p), [("parts.level", json!({"part": 10, "level": 1.0, "mute": true}))]);
    p.act(Action::Confirm(None));
    assert_eq!(edits(&mut p)[0].1["mute"], json!(false), "a second press undoes it");
    // A send: the first send's cell, a press, the dial.
    mv(&mut p, Some(0), 1);
    p.act(Action::Confirm(None));
    mv(&mut p, None, 1);
    assert_eq!(edits(&mut p), [("parts.send", json!({"part": 10, "bus": 12, "level": 0.525}))]);
    // Down to nothing the send goes, as the JUCE setSendLevel removes it: no level in the request.
    mv(&mut p, None, -100);
    let e = edits(&mut p);
    assert_eq!(e.last().unwrap(), &("parts.send", json!({"part": 10, "bus": 12})));
    p.act(Action::Cancel(None));
    // The second send of a part that had none: a send is made by raising it.
    mv(&mut p, Some(0), 1);
    p.act(Action::Confirm(None));
    mv(&mut p, None, 1);
    assert_eq!(edits(&mut p), [("parts.send", json!({"part": 10, "bus": 13, "level": 0.025}))]);
    assert_eq!(requests(&mut p), Vec::<&str>::new());
}

// + Part and + Bus are named as the JUCE view names them: "Part N" (N the parts after it), "Bus A" and on; the
// input a part or a bus starts with; Controls and Routing say they are not in this shell yet, Mixer opens.
#[test]
fn the_buttons_add_parts_and_buses_and_say_which_pages_are_not_built() {
    let mut s = Screen::new();
    let mut p = parts_screen(&mut s, &parts_graph());
    let ui = &s.ui;
    assert_eq!(tools(ui).iter().map(|t| (t.0.as_str(), t.1)).collect::<Vec<_>>(), [("Mixer", true), ("Controls", false), ("Routing", false), ("+ Part", true), ("+ Bus", true)]);
    p.act(Action::End(1));
    p.act(Action::Confirm(None)); // + Bus, the last
    let sent = std::mem::take(&mut p.out);
    assert_eq!(sent.iter().map(|o| o.1).collect::<Vec<_>>(), ["parts.add", "graph.commit"]);
    assert_eq!(sent[0].2["name"], json!("Bus C"), "two buses: the third");
    assert_eq!(sent[0].2["input"], json!({"kind": "bus", "port": 0, "zones": [], "devices": [], "maxNotes": 0, "reserve": 0, "priority": 0, "receive": {}}));
    mv(&mut p, Some(0), -1);
    p.act(Action::Confirm(None));
    assert_eq!(edits(&mut p)[0].1["name"], json!("Part 5"));
    // A tap does the same.
    p.tap_tool(4);
    assert_eq!(edits(&mut p)[0].1["name"], json!("Bus C"));
    // A page not built says so, and sends nothing; one that is built is gone to.
    assert_eq!(p.tap_tool(1), Outcome::Stay);
    assert_eq!(ui.global::<PartsState>().get_message().as_str(), "Controls is not in this shell yet.");
    assert!(edits(&mut p).is_empty());
    assert_eq!(p.tap_tool(0), Outcome::Page(crate::parts::Page::Mixer));
    p.act(Action::End(1));
    mv(&mut p, Some(0), -1);
    assert_eq!(ui.global::<PartsState>().get_message().as_str(), "", "what was said goes with the next move");
}

// The notes the shared budget took show on the row (parts.meters), a plugin that failed marks its chip and the
// message line says so (session.status); a refusal is said in the same place.
#[test]
fn given_up_notes_and_failed_plugins_are_shown() {
    let mut s = Screen::new();
    let mut p = parts_screen(&mut s, &parts_graph());
    let ui = &s.ui;
    let sound = |ui: &Shell| ui.global::<PartsState>().get_rows().iter().map(|r| format!("{}{}", r.sound, r.given)).collect::<Vec<_>>();
    assert_eq!(sound(ui)[0], "Rhodes");
    p.meters(&json!({"meters": [{"part": 10, "stolen": 3, "hardChokes": 0}, {"part": 11, "stolen": 7, "hardChokes": 2}, {"part": 14, "stolen": 0, "hardChokes": 0}]}));
    assert_eq!(&sound(ui)[..3], ["Rhodes · 3 stolen", "Bass · 7 stolen, 2 cut", "Pad"]);
    p.meters(&json!({"meters": []}));
    assert_eq!(sound(ui)[0], "Rhodes", "gone when the meters say nothing was given up");
    let chips = |ui: &Shell| ui.global::<PartsState>().get_chips().iter().map(|c| (c.row, c.name.to_string(), c.failed, c.off, c.role)).collect::<Vec<_>>();
    assert!(chips(ui).iter().all(|c| !c.2));
    p.status(&json!({"failures": [{"node": 5, "name": "Pad", "reason": "it crashed"}]}));
    assert!(chips(ui).iter().any(|c| c.1 == "Pad" && c.2), "the plugin that failed");
    assert_eq!(ui.global::<PartsState>().get_message().as_str(), "Pad is silent: it crashed");
    // An edit's refusal takes the line; the failure is back with the next action.
    p.act(Action::Move { pane: Some(0), by: 1 });
    p.act(Action::Move { pane: Some(0), by: 1 });
    p.act(Action::Confirm(None));
    p.act(Action::Move { pane: None, by: 1 });
    assert!(p.reply(crate::parts::Request::Edit, Err(crate::client::Error { code: -32009, message: "busy".into() })));
    assert_eq!(ui.global::<PartsState>().get_message().as_str(), "The engine refused: busy");
    p.act(Action::Cancel(None));
    assert_eq!(ui.global::<PartsState>().get_message().as_str(), "Pad is silent: it crashed");
    // Chips by role: an effect, an instrument, a split.
    let roles: Vec<_> = chips(ui).iter().filter(|c| c.0 == 0).map(|c| (c.1.clone(), c.4)).collect();
    assert_eq!(roles, [("Chorus".to_string(), 2), ("Rhodes".to_string(), 1)]);
}

// Touch: a tap on the part's words opens it, the sliders move a level and a send (a tap and a drag), a finger on
// one keeps the engine's own value for it away, the mute and the buttons tap.
#[test]
fn touch_opens_a_part_drags_the_sliders_and_taps_mute() {
    let mut s = Screen::new();
    let mut p = parts_screen(&mut s, &parts_graph());
    let ui = &s.ui;
    let state = ui.global::<PartsState>();
    mv(&mut p, Some(0), 1);
    assert_eq!(p.tap_open(2), Outcome::Open(14));
    assert_eq!(cursor(ui), None, "a tap takes the cursor away");
    p.level(0, 0.75);
    assert_eq!(edits(&mut p), [("parts.level", json!({"part": 10, "level": 0.75, "mute": false}))]);
    p.level(0, 0.75);
    assert!(edits(&mut p).is_empty(), "the same place is not sent again");
    p.send(2, 1, 0.4);
    assert_eq!(edits(&mut p), [("parts.send", json!({"part": 14, "bus": 13, "level": 0.4}))]);
    p.tap_mute(1);
    assert_eq!(edits(&mut p), [("parts.level", json!({"part": 11, "level": 1.0, "mute": true}))]);
    p.tap_mute(1);
    assert_eq!(edits(&mut p)[0].1["mute"], json!(false));
    // A finger down on a level: the graph that comes meanwhile does not move it.
    p.hold(0, 1, true);
    p.level(0, 0.9);
    p.graph(&parts_graph());
    assert_eq!(state.get_rows().row_data(0).unwrap().level, 0.9);
    p.controls(&json!({"parts": [{"part": 10, "level": 0.2, "mute": false}]}));
    assert_eq!(state.get_rows().row_data(0).unwrap().level, 0.9);
    p.hold(0, 1, false);
    p.controls(&json!({"parts": [{"part": 10, "level": 0.2, "mute": true}]}));
    assert_eq!((state.get_rows().row_data(0).unwrap().level, state.get_rows().row_data(0).unwrap().mute), (0.2, true), "followed again, with the mute that came");
    // The Parts' own headings and the sends' columns are the buses'.
    assert_eq!(state.get_bus_names().iter().map(|n| n.to_string()).collect::<Vec<_>>(), ["Reverb", "Delay"]);
    assert_eq!(state.get_rows().iter().map(|r| r.heading).collect::<Vec<_>>(), [false, false, false, false, true, false, false], "Shared buses above the first bus");
}

// The rows a view holds fill the height, whatever it is: a short window shows two at a time (the heading above
// the first bus costs a row's room, so it takes none from a view that does not hold it), the cursor going past
// a view turns it, Page Up and Down turn it with no cursor, and a taller window shows more at once.
#[test]
fn the_rows_a_view_holds_fill_the_height_and_the_cursor_turns_the_view() {
    let mut s = Screen::sized(1280, 360);
    let mut p = parts_screen(&mut s, &parts_graph());
    let ui = &s.ui;
    let names = |ui: &Shell| rows(ui).into_iter().map(|r| r.0).collect::<Vec<_>>();
    let view = |ui: &Shell| ui.global::<PartsState>().get_view().to_string();
    assert_eq!((names(ui), view(ui).as_str()), (vec!["Keys".to_string(), "Low".to_string()], "Parts 1–2 of 7"));
    p.act(Action::Page(1));
    assert_eq!((names(ui), view(ui).as_str()), (vec!["Pad".to_string(), "Empty".to_string()], "Parts 3–4 of 7"));
    p.act(Action::Page(1));
    assert_eq!(names(ui), ["Reverb", "Delay"]);
    assert_eq!(ui.global::<PartsState>().get_rows().iter().map(|r| r.heading).collect::<Vec<_>>(), [true, false], "the heading above the first bus");
    p.act(Action::Page(1));
    assert_eq!(names(ui), ["Shared · Master FX"]);
    p.act(Action::Page(1));
    assert_eq!(names(ui), ["Shared · Master FX"], "held at the last view");
    p.act(Action::Page(-3));
    assert_eq!(names(ui)[0], "Keys");
    // The cursor down the rows: past the second row it turns the view, and the cell is kept.
    p.act(Action::Move { pane: Some(0), by: 1 });
    mv(&mut p, Some(0), 2);
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(cursor(ui), Some((1, 2)));
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!((names(ui), cursor(ui)), (vec!["Pad".to_string(), "Empty".to_string()], Some((0, 2))), "the next view, the cursor on its first row");
    p.act(Action::End(-1));
    assert_eq!((names(ui)[0].as_str(), cursor(ui)), ("Keys", Some((0, 0))));
    // A view with the cursor in it stays when the window changes size: the rows it holds follow the height.
    let mut tall = Screen::sized(1280, 1000);
    let _ = parts_screen(&mut tall, &parts_graph());
    assert_eq!(rows(&tall.ui).len(), 7, "all of them in a tall window");
}

// No parts: the words the JUCE page has, the buttons stay.
#[test]
fn an_empty_performance_says_so_and_still_adds() {
    let mut s = Screen::new();
    let mut graph = parts_graph();
    graph["parts"] = json!([]);
    let mut p = parts_screen(&mut s, &graph);
    assert!(s.ui.global::<PartsState>().get_empty());
    assert_eq!(tools(&s.ui).len(), 5);
    s.frame();
    png(&s, "parts-empty");
    p.act(Action::End(1));
    p.act(Action::Confirm(None));
    assert_eq!(edits(&mut p)[0].1["name"], json!("Bus A"));
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(p.act(Action::Cancel(None)), Outcome::Back);
}

// On the shell through the fake engine: Play's top bar has Edit, which opens the list in this UI (no handover);
// the list asks session.status and polls parts.meters while shown, follows controls.changed, opens a part whose
// Back returns to the list, and the bar's Play (or Cancel) goes back to Play.
#[test]
fn edit_opens_the_list_and_a_part_goes_back_to_it() {
    let mut f = play_on_the_shell("parts-shell", json!({"id": "p1", "name": "Grand", "unsaved": false}), parts_graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    let graph = parts_graph();
    let labels = |ui: &Shell| ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect::<Vec<_>>();
    assert_eq!(labels(&ui), ["Undo Volume of Keys", "Redo", "Save", "Scene", "Learn", "Edit"]);
    ui.global::<Bar>().invoke_action(5);
    assert_eq!((ui.get_screen(), shell::handover(), ui.global::<Bar>().get_where().as_str()), (9, None, "Edit"));
    assert_eq!(labels(&ui), ["Undo Volume of Keys", "Redo", "Save", "Play"], "the toggle back");
    let r = request_in(&mut f, "session.status", &none, &graph);
    f.reply(&r, json!({"result": {"midiInputs": [{"kind": "builtIn", "name": "Built in", "source": 0}], "failures": [{"node": 4, "name": "Bass", "reason": "gone"}]}}));
    assert_eq!(ui.global::<PartsState>().get_title().as_str(), "Grand");
    assert_eq!(rows(&ui).len(), 7);
    assert_eq!(ui.global::<PartsState>().get_message().as_str(), "Bass is silent: gone");
    // The notes given up come from parts.meters, asked on a timer while the list is shown.
    f.screen.frame();
    std::thread::sleep(std::time::Duration::from_millis(250));
    f.screen.frame();
    let r = request_in(&mut f, "parts.meters", &none, &graph);
    f.reply(&r, json!({"result": {"meters": [{"part": 10, "peak": 0.1, "channels": [0.1], "stolen": 4, "hardChokes": 0}]}}));
    assert_eq!(ui.global::<PartsState>().get_rows().row_data(0).unwrap().given.as_str(), " · 4 stolen");
    // A level moved live by another client.
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controls.changed", "params": {"parts": [{"part": 10, "level": 0.9, "mute": true}], "master": {"level": 1.0, "pan": 0.0}}})));
    f.pump();
    let first = ui.global::<PartsState>().get_rows().row_data(0).unwrap();
    assert_eq!((first.level, first.mute), (0.9, true));
    // A press on the first part opens its Part screen, and Back returns here, not to Play.
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Confirm(None));
    assert_eq!((ui.get_screen(), ui.global::<Bar>().get_where().as_str()), (4, "Part"));
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 9, "Cancel from the Part screen: the list it came from");
    assert!(rows(&ui).iter().all(|r| r.2 < 0), "the cursor went with the screen");
    // The Part screen opened from Play goes back to Play.
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 3);
    ui.global::<PlayState>().invoke_part_tapped(0);
    assert_eq!(ui.get_screen(), 4);
    ui.global::<PartState>().invoke_back();
    assert_eq!(ui.get_screen(), 3);
    // The list's own Play action and Back button go to Play; Play's Cancel then goes Home, not back to the list.
    ui.global::<Bar>().invoke_action(5);
    assert_eq!(ui.get_screen(), 9);
    ui.global::<Bar>().invoke_action(3);
    assert_eq!(ui.get_screen(), 3);
    ui.global::<Bar>().invoke_action(5);
    ui.global::<PartsState>().invoke_back();
    assert_eq!(ui.get_screen(), 3);
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 0, "Play went back to where it was opened from");
    f.screen.frame();
}

// The list in every built-in theme and the two odd ones, at the keyboard's 1280x720 and at a small device's
// 800x480, at the FANTOM-07's scale too: pictures in target/screens and target/theme-shots for a look by eye.
// The rows a view holds fill the height (each theme's spacing and type decide how many), and a cursor and
// a level being changed are drawn in each.
#[test]
fn the_list_draws_in_every_theme_at_two_sizes() {
    for (w, h) in [(1280, 720), (800, 480)] {
        let mut s = Screen::sized(w, h);
        let mut p = parts_screen(&mut s, &parts_graph());
        s.ui.global::<Bar>().set_performance("Grand Piano".into());
        s.ui.global::<Bar>().set_where("Edit".into());
        s.ui.global::<Bar>().set_home_button(true);
        p.meters(&json!({"meters": [{"part": 11, "stolen": 7, "hardChokes": 2}]}));
        p.status(&json!({"failures": [{"node": 5, "name": "Pad", "reason": "it crashed"}]}));
        mv(&mut p, Some(0), 1);
        mv(&mut p, Some(0), 1);
        p.act(Action::Confirm(None)); // the first part's level, being changed
        for t in &all_themes() {
            theme::apply(&s.ui, t);
            assert!(s.frame().is_some(), "{}: drawn", t.id);
            p.shown(); // the rows a view holds depend on the paddings and the type
            s.frame();
            shot(&s, &format!("parts-{}-{w}", t.id));
            if t.id == "oscairn-dark" {
                png(&s, &format!("parts-{w}"));
            }
        }
        theme::apply(&s.ui, &joined_theme());
        s.frame();
        p.shown();
        p.act(Action::Cancel(None));
        p.act(Action::End(1));
        s.frame();
        shot(&s, &format!("parts-tools-{w}"));
    }
    let mut s = Screen::new();
    let mut p = parts_screen(&mut s, &parts_graph());
    mv(&mut p, Some(1), 1);
    at_fantom_scale(&mut s, "parts", || p.shown());
}
