// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Controls page (G1 to G4, G6) and the menu it opens: read as the JUCE controls page reads graph.get,
//! the cursor over the cells of the rows, each edit's request (and that none is committed, as the JUCE page
//! sends none), the menus of targets, curves, scenes and Now..., the table By scene, whole views that follow
//! the window, and on the shell (the Parts list's Controls button, the menu, the keyboard for a name).

use super::bar::{play_on_the_shell, request_in};
use super::parts::parts_graph;
use super::*;
use crate::controls::{Curve, Kind, Outcome, Request, Screen as Controls, Target};
use crate::menu::{Item, Kind as MenuKind, Menu, Outcome as MenuOutcome};
use crate::{ControlsState, MenuState, PartsState};

/// A target as graph.get gives it (with its `base`).
fn read_target(kind: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut t = json!({"kind": kind, "node": 0, "param": 0, "part": 0, "bus": 0, "control": 0, "from": 0, "to": 0, "start": 0.0, "end": 1.0, "curve": "linear", "relative": false, "base": 0.0, "offIn": []});
    for (k, v) in extra.as_object().unwrap() {
        t[k] = v.clone();
    }
    t
}

/// A target as `controls.targets` is sent it: the same without the `base` the engine works out.
fn target(kind: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut t = read_target(kind, extra);
    t.as_object_mut().unwrap().remove("base");
    t
}

/// The Parts list's Performance with two controls, two scenes and the nodes' parameter revisions.
fn graph() -> serde_json::Value {
    let mut g = parts_graph();
    for n in g["nodes"].as_array_mut().unwrap() {
        n["parameterRevision"] = json!(1);
    }
    g["scenes"] = json!([{"id": 1, "name": "Verse"}, {"id": 2, "name": "Chorus"}]);
    g["controls"] = json!([
        {"id": 1, "part": 0, "name": "Brightness", "value": 0.5, "source": null, "targets": [
            target("parameter", json!({"node": 1, "param": 7})),
            target("partLevel", json!({"part": 10, "start": 0.2, "end": 0.9, "curve": {"kind": "db", "min": -60.0, "max": 0.0}, "offIn": [2]})),
        ]},
        {"id": 2, "part": 0, "name": "Space", "value": 0.0, "source": null, "targets": [
            target("morph", json!({"from": 1, "to": 2})),
            target("control", json!({"control": 1, "relative": true})),
        ]},
    ]);
    g
}

fn controls_screen(s: &mut Screen, graph: &serde_json::Value) -> Controls {
    let mut c = Controls::new(&s.ui);
    s.ui.set_screen(11);
    s.frame();
    c.graph(graph);
    s.frame();
    c.shown();
    c
}

fn list_reply() -> serde_json::Value {
    json!({"parameters": [
        {"id": 7, "name": "Cutoff", "flags": 32 | 1024, "minimum": 0.0, "maximum": 10.0, "stepped": false},
        {"id": 8, "name": "Mode", "flags": 32 | 1, "minimum": 0.0, "maximum": 4.0, "stepped": true},
        {"id": 9, "name": "Meter", "flags": 8, "minimum": 0.0, "maximum": 1.0, "stepped": false},
    ]})
}

/// Answers the lists that were asked, as the engine would: the same parameters for every node.
fn answer(c: &mut Controls) {
    for (request, method, params) in std::mem::take(&mut c.out) {
        if let (Request::List(node), "params.list") = (&request, method) {
            let _ = params;
            c.reply(Request::List(*node), Ok(list_reply()));
        }
    }
}

type Row = (i32, String, i32, bool);

fn rows(ui: &Shell) -> Vec<Row> {
    ui.global::<ControlsState>().get_rows().iter().map(|r| (r.kind, r.name.to_string(), r.cell, r.editing)).collect()
}

fn tools(ui: &Shell) -> Vec<(String, bool, bool)> {
    ui.global::<ControlsState>().get_tools().iter().map(|t| (t.label.to_string(), t.on, t.cursor)).collect()
}

fn cursor(ui: &Shell) -> Option<(usize, i32)> {
    rows(ui).iter().position(|r| r.2 >= 0).map(|i| (i, rows(ui)[i].2))
}

fn edits(c: &mut Controls) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(&mut c.out).into_iter().filter(|o| !matches!(o.0, Request::List(_))).map(|(_, m, p)| (m, p)).collect()
}

fn mv(c: &mut Controls, pane: Option<u8>, by: i32) -> Outcome {
    c.act(Action::Move { pane, by })
}

/// Opens the menu that a press made ready and chooses the item at `path`'s labels.
fn menu_of(c: &mut Controls, s: &Screen) -> Menu {
    let (title, items) = c.take_menu().expect("a menu was made ready");
    Menu::open(&s.ui, &title, items)
}

fn choose(m: &mut Menu, c: &mut Controls, path: &[&str]) {
    for (i, label) in path.iter().enumerate() {
        let at = m.labels().iter().position(|l| l.0 == *label).unwrap_or_else(|| panic!("{label} in {:?}", m.labels()));
        // Move the cursor there, a row at a time as the dial does, and press.
        while m.labels().iter().position(|l| l.2) != Some(at) {
            let now = m.labels().iter().position(|l| l.2).unwrap();
            m.act(Action::Move { pane: None, by: if now < at { 1 } else { -1 } });
        }
        match m.act(Action::Confirm(None)) {
            MenuOutcome::Choose(id) => {
                assert_eq!(i, path.len() - 1, "a choice at the end of the path");
                c.chosen(id);
            }
            MenuOutcome::Stay => assert!(i < path.len() - 1, "a submenu opened"),
            MenuOutcome::Close => panic!("closed"),
        }
    }
}

// graph.get as the JUCE controls page reads it: each control with its targets, what a target drives named in
// the JUCE words (a parameter by the node's and the parameter's name once params.list has said it, else its
// number), the curves and ends, the scenes it is off in.
#[test]
fn the_page_reads_controls_and_names_what_targets_drive() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    let ui = &s.ui;
    assert_eq!(rows(ui).iter().map(|r| (r.0, r.1.as_str())).collect::<Vec<_>>(), [(0, "Brightness"), (1, "Rhodes · 7"), (1, "Keys level"), (0, "Space"), (1, "Morph Verse → Chorus"), (1, "→ Brightness")]);
    let asked: Vec<_> = c.out.iter().map(|o| (o.0.clone(), o.1, o.2.clone())).collect();
    assert_eq!(asked, [(Request::List(1), "params.list", json!({"node": 1}))], "the node a parameter target drives is asked once");
    answer(&mut c);
    assert_eq!(rows(ui)[1].1, "Rhodes · Cutoff");
    c.graph(&graph());
    assert!(c.out.is_empty(), "not asked again");
    let state = ui.global::<ControlsState>();
    let second = state.get_rows().row_data(2).unwrap();
    assert_eq!((second.curve.as_str(), second.scenes.as_str(), second.start, second.end, second.relative), ("Fader (dB)", "Scenes...", 0.2, 0.9, false));
    let first = state.get_rows().row_data(1).unwrap();
    assert_eq!((first.curve.as_str(), first.scenes.as_str(), first.now), ("Linear", "Scenes: all", true));
    assert!(!state.get_rows().row_data(4).unwrap().now, "a morph's ends are its scenes: no Now");
    assert!(state.get_rows().row_data(5).unwrap().relative);
    assert_eq!(state.get_rows().row_data(0).unwrap().value, 0.5);
    assert_eq!(tools(ui).iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["By scene", "+ Control"]);
    // A plugin whose parameters changed is read again.
    let mut changed = graph();
    changed["nodes"][0]["parameterRevision"] = json!(2);
    c.graph(&changed);
    assert_eq!(c.out.iter().map(|o| o.0.clone()).collect::<Vec<_>>(), [Request::List(1)]);
}

// The curves' shapes and a target's point in its range are the engine's (Graph::ControlCurve).
#[test]
fn curves_shape_as_the_engine_shapes_them() {
    let c = |kind, low, high, exponent| Curve { kind, low, high, exponent, points: vec![] };
    assert_eq!(c(0, -96., 0., 2.).shape(0.3), 0.3);
    assert!((c(1, -96., 0., 2.).shape(0.5) - (2f64.exp() - 1.) / (4f64.exp() - 1.)).abs() < 1e-12);
    assert!((c(2, -96., 0., 2.).shape(1.0) - 1.0).abs() < 1e-12);
    assert_eq!(c(3, -96., 0., 2.).shape(0.5), 0.5);
    assert_eq!((c(4, -60., 0., 2.).shape(0.0), (c(4, -60., 0., 2.).shape(1.0) - 1.0).abs() < 1e-12), (0.0, true));
    assert!((c(4, -60., 0., 2.).shape(0.5) - 10f64.powf(-30. / 20.)).abs() < 1e-12, "evenly in decibels");
    assert_eq!(c(5, -96., 0., 3.).shape(0.5), 0.125);
    let t = Target { kind: Kind::PartLevel(1), start: 0.2, end: 0.8, curve: c(0, -96., 0., 2.), relative: false, base: 0., off_in: vec![] };
    assert!((crate::controls::Target::read(&json!({"kind": "partLevel", "part": 1, "start": 0.2, "end": 0.8})).start - t.start).abs() < 1e-12);
    assert_eq!(t.json()["curve"], json!("linear"));
    assert_eq!(Target { curve: c(4, -60., -6., 2.), ..t.clone() }.json()["curve"], json!({"kind": "db", "min": -60.0, "max": -6.0}));
    assert_eq!(Target { curve: c(5, -96., 0., 0.5), ..t.clone() }.json()["curve"], json!({"kind": "power", "exponent": 0.5}));
    // A measured table: its points evenly over 0..1, linear between; read and written back whole.
    let table = json!({"kind": "table", "points": [0.0, 1.0, -0.5]});
    let read = crate::controls::Target::read(&json!({"kind": "partLevel", "part": 1, "curve": table}));
    assert_eq!((read.curve.shape(0.25), read.curve.shape(0.75), read.curve.shape(1.0)), (0.5, 0.25, -0.5));
    assert_eq!(Target { curve: read.curve, ..t }.json()["curve"], table);
}

// The cursor has two dimensions as the Parts list's: the dial cell by cell, row by row, then the buttons; the
// left knob along a row, the right knob down the rows. A control has a name, a fader, + Target and Remove; a
// target a start, an end, the curve, Relative, Now..., Scenes... and Remove: Now is left out for a morph.
#[test]
fn the_cursor_moves_along_cells_and_down_rows() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    let ui = &s.ui;
    assert_eq!(cursor(ui), None);
    mv(&mut c, Some(0), 1);
    assert_eq!(cursor(ui), Some((0, 0)), "the first move shows it on the first control's name");
    mv(&mut c, Some(0), 10);
    assert_eq!(cursor(ui), Some((0, 3)), "held at Remove: a control has four cells");
    mv(&mut c, Some(1), 1);
    assert_eq!(cursor(ui), Some((1, 3)), "down a row, the same cell: Relative");
    mv(&mut c, Some(0), 10);
    assert_eq!(cursor(ui), Some((1, 6)), "a target has seven");
    mv(&mut c, Some(1), 3);
    assert_eq!(cursor(ui), Some((4, 6)), "the morph's row");
    mv(&mut c, Some(0), -1);
    assert_eq!(cursor(ui), Some((4, 5)), "Scenes");
    mv(&mut c, Some(0), -1);
    assert_eq!(cursor(ui), Some((4, 3)), "Relative: a morph has no Now");
    mv(&mut c, Some(1), -4);
    assert_eq!(cursor(ui), Some((0, 3)), "up to the first control, its last cell");
    // The dial goes cell by cell, row by row, then the buttons.
    c.act(Action::End(-1));
    assert_eq!(cursor(ui), Some((0, 0)));
    assert!(c.at_top());
    for _ in 0..4 {
        mv(&mut c, None, 1);
    }
    assert_eq!(cursor(ui), Some((1, 0)), "four cells, then the next row");
    mv(&mut c, None, -1);
    assert_eq!(cursor(ui), Some((0, 3)));
    c.act(Action::End(1));
    assert_eq!(tools(ui).iter().position(|t| t.2), Some(1), "End: the last button");
    mv(&mut c, None, -1);
    assert_eq!(tools(ui).iter().position(|t| t.2), Some(0));
    mv(&mut c, None, -1);
    assert_eq!(cursor(ui), Some((5, 6)), "back from the buttons: the last row's last cell");
    assert!(edits(&mut c).is_empty(), "moving sends nothing");
    assert_eq!(c.act(Action::Cancel(None)), Outcome::Stay);
    assert_eq!(cursor(ui), None);
    assert_eq!(c.act(Action::Cancel(None)), Outcome::Back);
}

// A press on a fader or a slider starts a change the dial makes, a fortieth of the slider a click, each step a
// request with no commit after (the JUCE page sends none): `controls.set` for a control's value, the control's
// whole list of targets again (`controls.targets`) for a target's end. A press or Cancel ends the change.
#[test]
fn a_press_changes_a_fader_and_the_ends_of_a_range() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    let ui = &s.ui;
    c.act(Action::End(-1));
    mv(&mut c, Some(0), 1);
    assert_eq!(c.act(Action::Confirm(None)), Outcome::Stay);
    assert!(rows(ui)[0].3, "being changed");
    mv(&mut c, None, 1);
    assert_eq!(edits(&mut c), [("controls.set", json!({"control": 1, "value": 0.525}))]);
    mv(&mut c, None, -2);
    assert_eq!(edits(&mut c).last().unwrap().1["value"], json!(0.475));
    mv(&mut c, Some(1), 1);
    assert_eq!(cursor(ui), Some((0, 1)), "the right knob does not leave a value being changed");
    c.act(Action::Cancel(None));
    assert!(!rows(ui)[0].3 && cursor(ui) == Some((0, 1)), "Cancel ends the change, the cursor stays");
    // The second target's start, then its end.
    mv(&mut c, Some(0), -1);
    mv(&mut c, Some(1), 2);
    assert_eq!(cursor(ui), Some((2, 0)));
    c.act(Action::Confirm(None));
    mv(&mut c, None, 2);
    let sent = edits(&mut c);
    assert_eq!(sent.last().unwrap().0, "controls.targets");
    let list = sent.last().unwrap().1["targets"].as_array().unwrap().clone();
    assert_eq!(sent.last().unwrap().1["control"], json!(1));
    assert_eq!(list.len(), 2, "the whole list again");
    assert_eq!(list[1]["start"], json!(0.25), "0.2 and two clicks");
    assert_eq!(list[1]["curve"], json!({"kind": "db", "min": -60.0, "max": 0.0}), "the rest as it was");
    assert_eq!(list[1]["offIn"], json!([2]));
    assert_eq!(list[1]["part"], json!(10));
    assert_eq!(list[0], target("parameter", json!({"node": 1, "param": 7})), "the other target as it was");
    c.act(Action::Confirm(None));
    mv(&mut c, Some(0), 1);
    c.act(Action::Confirm(None));
    mv(&mut c, None, 100);
    assert_eq!(edits(&mut c).last().unwrap().1["targets"][1]["end"], json!(1.0), "held at the top");
    mv(&mut c, None, 1);
    assert!(edits(&mut c).is_empty(), "nothing at the end");
}

// + Control is named as the JUCE page names it, Remove takes a control or a target away, a press on a name asks
// for the new one on the text keyboard (a name that did not change sends nothing), Relative toggles.
#[test]
fn controls_are_added_renamed_and_removed_and_targets_removed_and_made_relative() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    let ui = &s.ui;
    c.act(Action::End(1));
    c.act(Action::Confirm(None));
    assert_eq!(edits(&mut c), [("controls.add", json!({"name": "Control 3"}))], "two controls: the third");
    assert_eq!(c.tap_tool(1), Outcome::Stay);
    assert_eq!(edits(&mut c)[0].1["name"], json!("Control 3"));
    c.act(Action::End(-1));
    assert_eq!(c.act(Action::Confirm(None)), Outcome::Prompt("Rename Brightness".into(), "Brightness".into()));
    c.entered("Brightness");
    assert!(edits(&mut c).is_empty(), "the same name: nothing");
    c.act(Action::Confirm(None));
    c.entered("Tone");
    assert_eq!(edits(&mut c), [("controls.name", json!({"control": 1, "name": "Tone"}))]);
    c.act(Action::Confirm(None));
    c.not_entered();
    c.entered("Lost");
    assert!(edits(&mut c).is_empty(), "a name given up changes nothing");
    // Relative on the first target.
    mv(&mut c, Some(1), 1);
    mv(&mut c, Some(0), 3);
    assert_eq!(cursor(ui), Some((1, 3)));
    c.act(Action::Confirm(None));
    let sent = edits(&mut c);
    assert_eq!((sent[0].0, sent[0].1["targets"][0]["relative"].clone(), sent[0].1["targets"][1]["relative"].clone()), ("controls.targets", json!(true), json!(false)));
    // Remove the second target: the list again without it; then the first control.
    mv(&mut c, Some(1), 1);
    mv(&mut c, Some(0), 3);
    assert_eq!(cursor(ui), Some((2, 6)));
    c.act(Action::Confirm(None));
    let sent = edits(&mut c);
    assert_eq!(sent[0].1["targets"].as_array().unwrap().len(), 1);
    assert_eq!(cursor(ui), None, "the row is gone with the cursor");
    c.act(Action::End(-1));
    mv(&mut c, Some(0), 3);
    c.act(Action::Confirm(None));
    assert_eq!(edits(&mut c), [("controls.remove", json!({"control": 1}))]);
    assert!(c.targets().len() == 2, "the screen waits for the engine's graph");
}

// No controls.* request commits, so the real engine sends no graph.changed after one (found against it,
// ui/REAL-ENGINE.md: a control added or renamed never showed): an accepted edit has the graph read
// again; a value moved (controls.set, live) does not.
#[test]
fn an_accepted_edit_has_the_graph_read_again_a_value_does_not() {
    let mut s = Screen::new();
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    c.act(Action::End(1));
    c.act(Action::Confirm(None)); // + Control
    let (request, method, _) = std::mem::take(&mut c.out).into_iter().find(|o| o.1 == "controls.add").expect("controls.add");
    assert_eq!(method, "controls.add");
    assert!(c.reply(request, Ok(json!({"control": 3}))), "read the graph again: the new control shows from it");
    c.level(0, 0.9); // Brightness, at 0.5
    let (request, method, _) = std::mem::take(&mut c.out).into_iter().next().expect("the fader's request");
    assert_eq!(method, "controls.set");
    assert!(!c.reply(request, Ok(json!({"accepted": true}))), "a value moved live: nothing to read again");
}

// The curve menu has the six curves, the one the target has ticked; choosing one sends the list again with
// it, the dB and power curves keeping their numbers; Scenes... lists the scenes ticked where the mapping works
// and choosing one switches it there; Now... offers Start and End.
#[test]
fn the_curve_scenes_and_now_menus() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    c.act(Action::End(-1));
    mv(&mut c, Some(1), 2);
    mv(&mut c, Some(0), 2);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    assert_eq!(m.labels().iter().map(|l| (l.0.as_str(), l.1)).collect::<Vec<_>>(), [("Linear", Some(false)), ("Exponential", Some(false)), ("Logarithmic", Some(false)), ("S-curve", Some(false)), ("Fader (dB)", Some(true)), ("Power", Some(false))]);
    choose(&mut m, &mut c, &["Power"]);
    let sent = edits(&mut c);
    assert_eq!(sent[0].1["targets"][1]["curve"], json!({"kind": "power", "exponent": 2.0}), "the JUCE default exponent");
    assert_eq!(sent[0].1["targets"][0]["curve"], json!("linear"));
    let mut m = {
        mv(&mut c, Some(0), 0);
        c.act(Action::Confirm(None));
        menu_of(&mut c, &s)
    };
    choose(&mut m, &mut c, &["S-curve"]);
    assert_eq!(edits(&mut c)[0].1["targets"][1]["curve"], json!("s-curve"));
    // The scenes: this mapping is off in the Chorus.
    mv(&mut c, Some(0), 3);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    assert_eq!(m.labels().iter().map(|l| (l.0.as_str(), l.1)).collect::<Vec<_>>(), [("Verse", Some(true)), ("Chorus", Some(false))]);
    choose(&mut m, &mut c, &["Chorus"]);
    assert_eq!(edits(&mut c)[0].1["targets"][1]["offIn"], json!([]), "switched on there");
    mv(&mut c, Some(0), 0);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["Verse"]);
    assert_eq!(edits(&mut c)[0].1["targets"][1]["offIn"], json!([1]), "switched off in the Verse");
    // Now...
    mv(&mut c, Some(0), -1);
    c.act(Action::Confirm(None));
    let m = menu_of(&mut c, &s);
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Start = now", "End = now"]);
}

// Now... sets either end of the range to where the destination is now, as setRangeToPresent works it out
// (RFC 0011's summing rule): the part's level for a part's level; for a parameter its value asked of the engine
// (params.get), as a fraction of its range, a stepped one rounded to a step; a relative one leaves the middle.
#[test]
fn now_sets_an_end_to_where_the_destination_is() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    // The part's level: one absolute target at the destination, so the point is the level less the others (none).
    c.act(Action::End(-1));
    mv(&mut c, Some(1), 2);
    mv(&mut c, Some(0), 4);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["End = now"]);
    let sent = edits(&mut c);
    assert_eq!(sent[0].1["targets"][1]["end"], json!(0.5), "Keys is at 0.5");
    assert_eq!(sent[0].1["targets"][1]["start"], json!(0.2));
    // A parameter: params.get asked, then the value as a fraction of the range, 2.5 of 0 to 10.
    mv(&mut c, Some(1), -1);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["Start = now"]);
    let asked: Vec<_> = std::mem::take(&mut c.out);
    assert_eq!((asked[0].1, asked[0].2.clone()), ("params.get", json!({"values": [{"node": 1, "param": 7}]})));
    c.reply(asked[0].0.clone(), Ok(json!({"values": [{"node": 1, "param": 7, "value": 2.5}]})));
    let sent = edits(&mut c);
    assert_eq!(sent[0].1["targets"][0]["start"], json!(0.25));
    // A parameter that takes modulation and a relative target: the middle, the value left alone.
    let mut g = graph();
    g["controls"][0]["targets"][0]["relative"] = json!(true);
    c.graph(&g);
    c.out.clear();
    c.act(Action::Cancel(None));
    c.act(Action::End(-1));
    mv(&mut c, Some(1), 1);
    mv(&mut c, Some(0), 4);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["End = now"]);
    let asked: Vec<_> = std::mem::take(&mut c.out);
    c.reply(asked[0].0.clone(), Ok(json!({"values": [{"node": 1, "param": 7, "value": 9.0}]})));
    assert_eq!(edits(&mut c)[0].1["targets"][0]["end"], json!(0.5), "Cutoff takes modulation (a flag of the list): the point is the middle");
    // A stepped parameter is rounded to its step: 4 steps over 0 to 4, 2.6 is 3 of 4.
    let mut g = graph();
    g["controls"][0]["targets"][0]["param"] = json!(8);
    c.graph(&g);
    answer(&mut c);
    c.act(Action::Cancel(None));
    c.act(Action::End(-1));
    mv(&mut c, Some(1), 1);
    mv(&mut c, Some(0), 4);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["Start = now"]);
    let asked: Vec<_> = std::mem::take(&mut c.out);
    c.reply(asked[0].0.clone(), Ok(json!({"values": [{"node": 1, "param": 8, "value": 2.6}]})));
    assert_eq!(edits(&mut c)[0].1["targets"][0]["start"], json!(0.75));
    // Another control: its value. The second control's second target drives control 1, at 0.5, relative.
    mv(&mut c, Some(1), 4);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["Start = now"]);
    let sent = edits(&mut c);
    assert_eq!(sent[0].1["control"], json!(2));
    assert_eq!(sent[0].1["targets"][1]["start"], json!(1.0), "a relative target that does not sum: now less its base, and a half: 0.5 + 0.5");
}

// + Target: a menu of what a control can drive (a plugin's parameter by node, a part's level, mute or send,
// another control, a morph between two scenes), after the parameters of every node are read; the choice is
// added with the full range, the linear curve and every scene.
#[test]
fn the_target_menu_lists_what_a_control_can_drive_and_adds_it() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    c.act(Action::End(-1));
    mv(&mut c, Some(0), 2);
    c.act(Action::Confirm(None));
    assert!(c.take_menu().is_none(), "the other nodes' parameters are asked first");
    assert_eq!(s.ui.global::<ControlsState>().get_message().as_str(), "Reading the plugins' parameters...");
    let asked: Vec<_> = c.out.iter().map(|o| o.0.clone()).collect();
    assert_eq!(asked, [2, 3, 4, 5, 6, 8].map(Request::List), "every node but the one already read");
    answer(&mut c);
    assert_eq!(s.ui.global::<ControlsState>().get_message().as_str(), "");
    let mut m = menu_of(&mut c, &s);
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Plugin parameter", "Part level", "Part mute", "Part send", "Another control", "Morph between scenes"]);
    choose(&mut m, &mut c, &["Plugin parameter", "Rhodes", "Mode"]);
    let sent = edits(&mut c);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1["control"], json!(1));
    assert_eq!(sent[0].1["targets"].as_array().unwrap().len(), 3);
    assert_eq!(sent[0].1["targets"][2], target("parameter", json!({"node": 1, "param": 8})), "a new target: the whole range, linear, in every scene");
    c.graph(&graph()); // the engine's graph after the edit, as graph.changed brings it
    // The read-only parameter is not offered; the others are.
    mv(&mut c, Some(0), 0);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    m.act(Action::Confirm(None));
    m.act(Action::Confirm(None));
    assert_eq!(m.labels().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["Cutoff", "Mode"], "Meter is read only");
    m.act(Action::Cancel(None));
    m.act(Action::Cancel(None));
    choose(&mut m, &mut c, &["Part level", "Pad"]);
    assert_eq!(edits(&mut c)[0].1["targets"][2], target("partLevel", json!({"part": 14})));
    c.graph(&graph());
    mv(&mut c, Some(0), 0);
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["Part mute", "Low"]);
    assert_eq!(edits(&mut c)[0].1["targets"][2], target("partMute", json!({"part": 11})));
    c.graph(&graph());
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["Part send", "Pad to Delay"]);
    assert_eq!(edits(&mut c)[0].1["targets"][2], target("sendLevel", json!({"part": 14, "bus": 13})));
    c.graph(&graph());
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    assert_eq!(m.labels()[4].0, "Another control");
    choose(&mut m, &mut c, &["Another control", "Space"]);
    assert_eq!(edits(&mut c)[0].1["targets"][2], target("control", json!({"control": 2})));
    c.graph(&graph());
    c.act(Action::Confirm(None));
    let mut m = menu_of(&mut c, &s);
    choose(&mut m, &mut c, &["Morph between scenes", "Chorus →", "Verse"]);
    assert_eq!(edits(&mut c)[0].1["targets"][2], target("morph", json!({"from": 2, "to": 1})));
}

// By scene: the table of every mapping against every scene, ticked where it works; a tick toggles it; the
// cursor goes over the ticks and the toggle is the first button.
#[test]
fn by_scene_shows_the_table_and_toggles_a_mapping_in_a_scene() {
    let mut s = Screen::sized(1280, 900);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    let ui = &s.ui;
    c.act(Action::End(1));
    mv(&mut c, None, -1);
    assert_eq!(tools(ui), [("By scene".into(), false, true), ("+ Control".into(), false, false)]);
    c.act(Action::Confirm(None));
    let state = ui.global::<ControlsState>();
    assert!(state.get_table());
    assert_eq!(state.get_scene_names().iter().map(|n| n.to_string()).collect::<Vec<_>>(), ["Verse", "Chorus"]);
    assert_eq!(state.get_table_rows().iter().map(|r| r.label.to_string()).collect::<Vec<_>>(), ["Brightness  ·  Rhodes · Cutoff", "Brightness  ·  Keys level", "Space  ·  Morph Verse → Chorus", "Space  ·  → Brightness"]);
    let ticks = |ui: &Shell| ui.global::<ControlsState>().get_table_cells().iter().map(|c| (c.row, c.col, c.on)).collect::<Vec<_>>();
    assert_eq!(ticks(ui), [(0, 0, true), (0, 1, true), (1, 0, true), (1, 1, false), (2, 0, true), (2, 1, true), (3, 0, true), (3, 1, true)], "off in the Chorus: the second mapping");
    assert!(tools(ui)[0].1, "the toggle is on");
    // The cursor over the ticks: the dial in reading order, the knobs by column and row.
    mv(&mut c, None, 1);
    let at = |ui: &Shell| ui.global::<ControlsState>().get_table_cells().iter().find(|c| c.cursor).map(|c| (c.row, c.col));
    assert_eq!(at(ui), Some((0, 0)));
    mv(&mut c, None, 1);
    assert_eq!(at(ui), Some((0, 1)));
    mv(&mut c, None, 1);
    assert_eq!(at(ui), Some((1, 0)), "on to the next mapping");
    mv(&mut c, Some(0), 1);
    mv(&mut c, Some(1), 1);
    assert_eq!(at(ui), Some((2, 1)));
    mv(&mut c, Some(1), -1);
    c.act(Action::Confirm(None));
    let sent = edits(&mut c);
    assert_eq!(sent[0].1["control"], json!(1));
    assert_eq!(sent[0].1["targets"][1]["offIn"], json!([]), "on again in the Chorus");
    c.tap_table(0, 0);
    assert_eq!(edits(&mut c)[0].1["targets"][0]["offIn"], json!([1]), "off in the Verse by a tap");
    assert_eq!(at(ui), None, "a tap drops the cursor");
    // Leaving the table: the toggle again.
    c.act(Action::End(1));
    mv(&mut c, None, -1);
    c.act(Action::Confirm(None));
    assert!(!ui.global::<ControlsState>().get_table());
    assert_eq!(cursor(ui), None);
}

// The rows a view holds fill the height, a target taking a line, two or three by the width, whatever the
// window: a short one shows a few rows, the cursor going past a view turns it, Page Up and Down turn it with
// no cursor, a narrow one gives the buttons their own lines.
#[test]
fn the_rows_a_view_holds_follow_the_window() {
    let mut s = Screen::sized(1280, 300);
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    let ui = &s.ui;
    let names = |ui: &Shell| rows(ui).into_iter().map(|r| r.1).collect::<Vec<_>>();
    let view = |ui: &Shell| ui.global::<ControlsState>().get_view().to_string();
    let first = names(ui);
    assert!(first.len() < 6 && first.len() >= 2, "a short window holds a few rows: {first:?}");
    assert!(view(ui).starts_with(&format!("Rows 1–{} of 6", first.len())));
    c.act(Action::Page(1));
    assert_ne!(names(ui), first, "the next view");
    c.act(Action::Page(-5));
    assert_eq!(names(ui), first);
    c.act(Action::End(-1));
    for _ in 0..first.len() {
        c.act(Action::Arrow { dx: 0, dy: 1 });
    }
    assert!(cursor(ui).is_some() && names(ui) != first, "the cursor turned the view and is in it");
    let mut tall = Screen::sized(1280, 1000);
    let _ = controls_screen(&mut tall, &graph());
    assert_eq!(rows(&tall.ui).len(), 6, "all of them in a tall window");
}

// No controls: the words the JUCE page has; + Control stays.
#[test]
fn no_controls_says_so_and_still_adds() {
    let mut s = Screen::new();
    let mut g = graph();
    g["controls"] = json!([]);
    let mut c = controls_screen(&mut s, &g);
    assert!(s.ui.global::<ControlsState>().get_empty());
    assert_eq!(tools(&s.ui).iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["By scene", "+ Control"]);
    s.frame();
    png(&s, "controls-empty");
    c.act(Action::End(1));
    c.act(Action::Confirm(None));
    assert_eq!(edits(&mut c)[0].1["name"], json!("Control 1"));
    g["scenes"] = json!([]);
    c.graph(&g);
    assert_eq!(tools(&s.ui).iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["+ Control"], "no scenes, no By scene");
}

fn items(labels: &[&str]) -> Vec<Item> {
    labels.iter().enumerate().map(|(i, l)| Item::choice(*l, i)).collect()
}

// The menu: the dial and the knobs move the cursor over what can be chosen (a heading or a disabled row is
// passed), a press chooses or opens a submenu, Cancel goes up a level and closes at the first, nothing is
// chosen until a press; Page turns a long list's view; a tap chooses.
#[test]
fn a_menu_moves_chooses_and_goes_up() {
    let mut s = Screen::sized(1280, 720);
    let ui = s.ui.clone_strong();
    let ui = &ui;
    let mut m = Menu::open(
        ui,
        "Test",
        vec![Item::head("Kinds"), Item::sub("Sub", vec![Item::choice("A", 10), Item::ticked("B", 11, true).off(), Item::choice("C", 12)]), Item::choice("Off", 1).off(), Item::ticked("Tick", 2, false), Item::sub("Empty", vec![])],
    );
    s.frame();
    assert!(ui.global::<MenuState>().get_open());
    let cursor = |m: &Menu| m.labels().iter().position(|l| l.2);
    assert_eq!(cursor(&m), Some(1), "the first row that can be chosen");
    assert_eq!(m.act(Action::Move { pane: None, by: 1 }), MenuOutcome::Stay);
    assert_eq!(cursor(&m), Some(3), "the disabled row is passed");
    m.act(Action::Move { pane: Some(0), by: 5 });
    assert_eq!(cursor(&m), Some(3), "held at the last row that can be chosen: an empty submenu cannot");
    m.act(Action::Move { pane: None, by: -1 });
    assert_eq!(m.act(Action::Confirm(None)), MenuOutcome::Stay);
    assert_eq!(ui.global::<MenuState>().get_title().as_str(), "Test › Sub");
    assert!(ui.global::<MenuState>().get_up());
    assert_eq!(cursor(&m), Some(0));
    m.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(cursor(&m), Some(2), "B is disabled");
    assert_eq!(m.act(Action::Cancel(None)), MenuOutcome::Stay, "up a level");
    assert_eq!(ui.global::<MenuState>().get_title().as_str(), "Test");
    assert_eq!(m.tap(1), MenuOutcome::Stay, "a tap on a submenu opens it");
    assert_eq!(m.tap(2), MenuOutcome::Choose(12));
    assert!(!ui.global::<MenuState>().get_open(), "a choice closes it");
    let mut m = Menu::open(ui, "Again", items(&["One", "Two"]));
    assert_eq!(m.act(Action::Cancel(None)), MenuOutcome::Close);
    assert!(!ui.global::<MenuState>().get_open());
    // A long list: the view turns with the cursor and Page.
    let long: Vec<String> = (0..40).map(|i| format!("Row {i}")).collect();
    let mut m = Menu::open(ui, "Long", items(&long.iter().map(String::as_str).collect::<Vec<_>>()));
    s.frame();
    let per = ui.global::<MenuState>().get_rows_per_view() as usize;
    assert!((3..40).contains(&per));
    assert_eq!(m.shown_rows(), per);
    m.act(Action::Page(1));
    assert_eq!(cursor(&m), Some(per));
    assert_eq!(ui.global::<MenuState>().get_rows().row_data(0).unwrap().label.as_str(), format!("Row {per}"));
    m.act(Action::End(1));
    assert_eq!(cursor(&m), Some(39));
    assert_eq!(MenuKind::Choice(0), MenuKind::Choice(0));
}

// On the shell through the fake engine: the Parts list's Controls button opens the page, + Target asks
// the nodes' parameters, opens the menu over the page (which takes the keys), a choice reaches the engine
// as the whole target list, Cancel closes the menu and then goes back; the bar's Play goes home.
#[test]
fn controls_open_from_the_parts_list_and_a_menu_choice_reaches_the_engine() {
    let mut f = play_on_the_shell("controls-shell", json!({"id": "p1", "name": "Grand", "unsaved": false}), graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    let g = graph();
    ui.global::<Bar>().invoke_action(5);
    assert_eq!(ui.get_screen(), 9);
    let tools_ = ui.global::<PartsState>().get_tools();
    assert_eq!((tools_.row_data(1).unwrap().label.as_str(), tools_.row_data(1).unwrap().enabled), ("Controls", true), "the button is on");
    ui.global::<PartsState>().invoke_tool(1);
    assert_eq!((ui.get_screen(), crate::tests::title(&ui)), (11, "Parts › Controls".to_string()));
    assert_eq!(ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect::<Vec<_>>(), ["Undo Volume of Keys", "Redo", "Save", "Play", "Record", "Capture"]);
    // The page asks the parameters of the node its first target drives.
    let r = request_in(&mut f, "params.list", &none, &g);
    assert_eq!(r["params"]["node"], json!(1));
    f.reply(&r, json!({"result": list_reply()}));
    assert_eq!(ui.global::<ControlsState>().get_rows().row_data(1).unwrap().name.as_str(), "Rhodes · Cutoff");
    // + Target on the first control: the other nodes are asked, then the menu opens.
    ui.global::<ControlsState>().invoke_press(0, 2);
    assert!(!ui.global::<MenuState>().get_open());
    let mut left = 6;
    while left > 0 {
        let r = request_in(&mut f, "params.list", &none, &g);
        if r["method"] == "params.list" {
            f.reply(&r, json!({"result": list_reply()}));
            left -= 1;
        } else {
            f.reply(&r, json!({"result": {}})); // the status the page asked as it opened
        }
    }
    assert!(ui.global::<MenuState>().get_open(), "the menu is over the page");
    f.screen.frame();
    assert_eq!(ui.global::<MenuState>().get_title().as_str(), "Target of Brightness");
    assert_eq!(ui.global::<MenuState>().get_rows().iter().map(|r| r.label.to_string()).collect::<Vec<_>>(), ["Plugin parameter", "Part level", "Part mute", "Part send", "Another control", "Morph between scenes"]);
    // The keys go to the menu: down to Part level, in, down to the second part, choose.
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    assert_eq!(ui.global::<MenuState>().get_title().as_str(), "Target of Brightness › Part level");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    assert!(!ui.global::<MenuState>().get_open(), "a choice closes it");
    let r = request_in(&mut f, "controls.targets", &none, &g);
    assert_eq!(r["params"]["targets"][2], target("partLevel", json!({"part": 11})));
    assert_eq!(r["params"]["control"], json!(1));
    // A menu closed by Cancel; then Cancel goes back to the list.
    ui.global::<ControlsState>().invoke_press(0, 2);
    assert!(ui.global::<MenuState>().get_open(), "the nodes' parameters are known now");
    shell::act(Action::Cancel(None));
    assert!(!ui.global::<MenuState>().get_open());
    assert_eq!(ui.get_screen(), 11, "Cancel closed the menu, not the page");
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 9);
    ui.global::<PartsState>().invoke_tool(1);
    ui.global::<Bar>().invoke_action(3);
    assert_eq!(ui.get_screen(), 3, "Play");
    // A menu is dropped with its screen.
    f.screen.frame();
}

// A performance instrument: with the Controls page open and nothing happening, it asks nothing and draws
// nothing (docs/rust-ui.md, idle); a status that says what the last said costs neither.
#[test]
fn at_rest_it_costs_no_request_and_no_frame() {
    let mut f = play_on_the_shell("controls-idle", json!({"id": "p1", "name": "Grand", "unsaved": false}), graph());
    let ui = f.screen.ui.clone_strong();
    let g = graph();
    ui.global::<Bar>().invoke_action(5); // Edit: the Parts list
    assert_eq!(ui.get_screen(), 9);
    ui.global::<PartsState>().invoke_tool(1); // Controls
    assert_eq!((ui.get_screen(), crate::tests::title(&ui)), (11, "Parts › Controls".to_string()));
    let status = json!({"cpuLoad": 0.42, "audioDevice": "null", "sampleRate": 48000});
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": status}))); // the first: the takes and the meters follow
    f.pump();
    // Everything the shell and the page ask until they are quiet, as a real engine would.
    let mut quiet = 0;
    while quiet < 3 {
        let Some(request) = f.engine.try_request(std::time::Duration::from_millis(60)) else {
            quiet += 1;
            continue;
        };
        quiet = 0;
        let result = match request["method"].as_str().unwrap() {
            "params.list" => list_reply(),
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => g.clone(),
            "record.get" => json!({"takes": []}),
            "memory.state" => json!({"systemAvailable": 496u64 << 20, "device": 0}),
            "transport.get" => json!({"tempo": 120.0, "playing": false}),
            "session.status" => json!({"midiInputs": []}),
            "edit.history" => json!({"undo": "Volume of Keys", "redo": ""}),
            _ => json!({}),
        };
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        f.pump();
    }
    assert_eq!(ui.global::<ControlsState>().get_rows().row_data(1).unwrap().name.as_str(), "Rhodes · Cutoff", "the page is whole: the targets' parameters are named");
    f.screen.frame();
    f.screen.frame();
    assert_eq!(f.screen.frame(), None, "nothing is drawn at rest");
    // The engine's status that says what the last said: no request, no frame.
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": status})));
    f.pump();
    assert!(f.engine.try_request(std::time::Duration::from_millis(700)).is_none(), "an unchanged status asked for something");
    std::thread::sleep(std::time::Duration::from_millis(1200));
    slint::platform::update_timers_and_animations();
    assert!(f.engine.try_request(std::time::Duration::from_millis(500)).is_none(), "nothing is asked at rest");
    assert_eq!(f.screen.frame(), None, "nothing is drawn at rest");
}

// The page and the menu in every built-in theme and the two odd ones, at the keyboard's 1280x720, a small
// device's 800x480, 800 wide and the FANTOM-07's scale: pictures in target/screens and target/theme-shots.
#[test]
fn the_page_and_the_menu_draw_in_every_theme_at_several_sizes() {
    for (w, h) in [(1280, 720), (800, 480), (800, 700)] {
        let mut s = Screen::sized(w, h);
        let mut c = controls_screen(&mut s, &graph());
        answer(&mut c);
        s.ui.global::<Bar>().set_performance("Grand Piano".into());
        crate::tests::set_title(&s.ui, &["Parts", "Controls"]);
        mv(&mut c, Some(1), 1);
        mv(&mut c, Some(1), 1);
        mv(&mut c, Some(0), 2);
        c.act(Action::Confirm(None)); // the first target's end slider... being changed
        for t in &all_themes() {
            theme::apply(&s.ui, t);
            assert!(s.frame().is_some(), "{}: drawn", t.id);
            c.shown(); // the rows a view holds depend on the paddings and the type
            s.frame();
            shot(&s, &format!("controls-{}-{w}x{h}", t.id));
            if t.id == "oscairn-dark" {
                png(&s, &format!("controls-{w}x{h}"));
            }
        }
        // By scene, and a menu over the page.
        theme::apply(&s.ui, &all_themes()[0]);
        c.act(Action::Cancel(None));
        c.act(Action::End(1));
        mv(&mut c, None, -1);
        c.act(Action::Confirm(None));
        c.act(Action::Move { pane: None, by: 2 });
        s.frame();
        png(&s, &format!("controls-table-{w}x{h}"));
        c.act(Action::Confirm(None));
        c.act(Action::Cancel(None));
        c.act(Action::Confirm(None)); // By scene off again
        c.act(Action::End(-1));
        mv(&mut c, Some(0), 2);
        c.act(Action::Confirm(None));
        let _ = edits(&mut c);
        c.act(Action::Cancel(None));
        c.act(Action::End(-1));
        mv(&mut c, Some(0), 2);
        c.act(Action::Confirm(None));
        let (title, items) = c.take_menu().unwrap_or_else(|| ("Curve".to_string(), vec![]));
        let mut menu = Menu::open(&s.ui, &title, if items.is_empty() { (0..30).map(|i| Item::ticked(format!("Parameter number {i}"), i, i == 3)).collect() } else { items });
        s.frame();
        menu.show(); // as the shell shows it when the window says how many rows fit
        s.frame();
        png(&s, &format!("controls-menu-{w}x{h}"));
        for t in &all_themes() {
            theme::apply(&s.ui, t);
            s.frame();
            menu.show();
            s.frame();
            shot(&s, &format!("controls-menu-{}-{w}x{h}", t.id));
        }
    }
    let mut s = Screen::new();
    let mut c = controls_screen(&mut s, &graph());
    answer(&mut c);
    mv(&mut c, Some(1), 1);
    at_fantom_scale(&mut s, "controls", || c.shown());
}

// A control's name and a scene's name each 45+ characters: at a small device's 800x480 and the
// FANTOM-07's 1280x720 at 1.55 the rows that fit show whole and by scene, every name elided, nothing
// overlaps the tools' row or the footer or leaves the window; and the same page in the light theme
// is drawn in the theme's colors (the background where nothing is drawn is the theme's, and no
// dark-only ink covers the screen). The names and scenes come from the page's own models, and how
// many rows fit is the screen's own count for the window.
#[test]
fn long_names_fit_the_small_screens_and_draw_in_the_light_theme() {
    let long = "An Extremely Long Control Name That Goes Well Beyond Forty Characters";
    let scene = "And A Very Long Scene Name That Also Passes The Forty Character Mark";
    assert!(long.len() > 40 && scene.len() > 40);
    let mut g = graph();
    g["controls"][0]["name"] = json!(long);
    g["scenes"] = json!([{"id": 1, "name": scene}, {"id": 2, "name": "Chorus"}]);
    // Nothing leaves the window: every element of the page is inset by the theme's margin, so the
    // window's outer border is the background wherever nothing is drawn (a long name that wrapped or
    // ran past its section would cross it).
    let fits = |s: &Screen, scale: f32| {
        let (w, h) = (s.w, s.h);
        let bg = s.pixels[0];
        let mut bad = 0;
        // Below the top bar, the page's left and right edge are the background at every height, and
        // the bottom four rows under the footer: a long name that wrapped or ran past its section
        // would cross them (the bar's own meters at the right edge are the bar's concern).
        for y in (48.0 * scale).ceil() as usize..h { // below the top bar, 48 logical
            for &(x, n) in &[(0usize, 4usize), (w - 4, 4)] {
                for i in 0..n {
                    if x + i < w && s.pixels[y * w + x + i] != bg {
                        bad += 1;
                    }
                }
            }
        }
        for x in 0..w {
            for y in (h - 4)..h {
                if s.pixels[y * w + x] != bg {
                    bad += 1;
                }
            }
        }
        assert_eq!(bad, 0, "{bad} px of a control or its text reach the window's edge");
    };
    for (w, h) in [(800, 480), (800, 700), (1280, 720)] {
        let mut s = Screen::sized(w, h);
        let mut c = controls_screen(&mut s, &g);
        answer(&mut c);
        c.shown();
        assert!(s.frame().is_some(), "{w}x{h}: drawn");
        let shown = s.ui.global::<ControlsState>().get_rows().row_count();
        assert!(shown > 0, "{w}x{h}: the long-named rows are shown");
        fits(&s, 1.0);
        png(&s, &format!("controls-long-{w}x{h}"));
        // At 1.55, checked before the scale goes back (at_fantom_scale ends at 1.0).
        s.set_scale(1.55);
        s.frame();
        c.shown();
        s.frame();
        fits(&s, 1.55);
        assert!(s.ui.global::<ControlsState>().get_rows().row_count() > 0, "{w}x{h}: rows at 1.55");
        png(&s, &format!("controls-long-{w}x{h}@1.55"));
        s.set_scale(1.0);
    }
    // The light theme at the keyboard's 1280x720.
    let mut s = Screen::sized(1280, 720);
    let mut c = controls_screen(&mut s, &g);
    answer(&mut c);
    c.shown();
    let light = theme::chosen(&theme::Places::default(), "oscairn-light", "");
    let bg = light.data.ink("background");
    theme::apply(&s.ui, &light);
    s.frame();
    let t = s.ui.global::<crate::Theme>();
    assert_eq!(t.get_id(), "oscairn-light", "the theme applied");
    assert!(!t.get_dark(), "a light background");
    let corner: [u8; 3] = [s.pixels[0].r, s.pixels[0].g, s.pixels[0].b];
    assert_eq!(corner, [bg.red(), bg.green(), bg.blue()], "the background where nothing is drawn");
    // No dark theme's surface left: the page is drawn in the light theme's colors, so no more than a
    // small share (thin dark text) is darker than a mid grey. (The stage screens' near-white "#F6F8FA"
    // ink heuristic does not fit here: the light theme's own white surface renders through the software
    // renderer near that value.)
    let share = s.pixels.iter().filter(|p| 0.2126 * f64::from(p.r) + 0.7152 * f64::from(p.g) + 0.0722 * f64::from(p.b) < 80.0).count();
    assert!(share * 50 < s.pixels.len(), "the page is not mostly dark ({share} px)");
    shot(&s, "controls-long-light");
    at_fantom_scale(&mut s, "controls-long-light", || c.shown());
}
