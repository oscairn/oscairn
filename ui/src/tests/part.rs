// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Part screen's tests: on its own against graph.get's words, and on the shell through the
//! fake engine, as Play's are (the parent module has the fixture and the pictures).

use super::*;
use crate::part::Outcome::{Back, Stay};

pub fn part_graph() -> serde_json::Value {
    json!({
        "nodes": [
            {"id": 1, "name": "Rhodes", "plugin": {"name": "Surge XT"}, "audioInputs": [], "audioOutputs": [{"channels": 2}], "noteInputs": 1, "noteOutputs": 0, "enabled": true},
            {"id": 2, "name": "Chorus", "plugin": {"name": "Chorus"}, "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0, "enabled": false},
            {"id": 3, "name": "Hall", "plugin": {"name": "Verb"}, "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0, "enabled": true},
            {"id": 4, "name": "Bass", "plugin": {"name": "Bassline"}, "audioInputs": [], "audioOutputs": [{"channels": 1}], "noteInputs": 1, "noteOutputs": 0, "enabled": true},
        ],
        "parts": [
            {"id": 10, "name": "Keys", "level": 0.5, "pan": 0.25, "mute": false, "solo": false, "chain": [2, 1], "bus": 0, "sends": [{"bus": 12, "level": 0.5}],
             "from": {"sound": "s1", "name": "Warm Rhodes", "modified": true},
             "input": {"kind": "notes", "port": 0, "devices": [], "maxNotes": 0, "reserve": 0, "priority": 0, "receive": {"sustain": "none"},
                       "zones": [{"keys": [36, 96], "velocities": [0, 1], "channels": 65535, "transpose": -12, "curve": 1, "fixedVelocity": 0}]}},
            {"id": 11, "name": "Low", "level": 1.0, "pan": 0.0, "mute": false, "solo": false, "chain": [4], "bus": 12, "sends": [], "from": null,
             "input": {"kind": "notes", "port": 0, "devices": [], "maxNotes": 0, "reserve": 0, "priority": 0, "receive": {}, "zones": []}},
            {"id": 12, "name": "Reverb", "level": 0.8, "pan": 0.0, "mute": false, "solo": false, "chain": [3], "bus": 0, "sends": [], "input": {"kind": "bus"}},
        ],
        "wires": [], "scenes": [{"id": 1, "name": "Verse"}], "scene": 1, "master": {"level": 1.0, "pan": 0.0},
    })
}

/// Each row as (label, value, cursor, being changed).
fn rows(ui: &Shell) -> Vec<(String, String, bool, bool)> {
    ui.global::<crate::PartState>().get_rows().iter().map(|r| (r.label.to_string(), r.value.to_string(), r.cursor, r.editing)).collect()
}

fn cursor_at(ui: &Shell) -> Option<usize> {
    rows(ui).iter().position(|r| r.2)
}

fn info_lines(ui: &Shell) -> Vec<String> {
    ui.global::<crate::PartState>().get_info().iter().map(|l| l.text.to_string()).collect()
}

/// The part screen on its own, with `graph`, the whole list as one view.
pub fn part_screen(s: &Screen, graph: &serde_json::Value, id: u32) -> crate::part::Screen {
    let mut part = crate::part::Screen::new(&s.ui);
    tall(&s.ui);
    part.graph(graph);
    part.open(id);
    part
}

/// What was asked of the engine, the `graph.commit` after a staged edit left out (`commits` counts those).
fn edits(part: &mut crate::part::Screen) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(&mut part.out).into_iter().map(|(_, m, p)| (m, p)).filter(|(m, _)| *m != "graph.commit").collect()
}

/// Every request, `graph.commit` included.
fn requests(part: &mut crate::part::Screen) -> Vec<&'static str> {
    std::mem::take(&mut part.out).into_iter().map(|(_, m, _)| m).collect()
}

fn mv(part: &mut crate::part::Screen, by: i32) {
    part.act(Action::Move { pane: None, by });
}

fn press(part: &mut crate::part::Screen) {
    part.act(Action::Confirm(None));
}

// What it lists for a note part, a bus and an audio part; a knob of either pane or the dial moves
// the cursor, a press on a value starts changing it and the dial then changes it, each step sent
// as its request; Cancel ends a change, then drops the cursor, then goes back; mute and solo; the
// key range and transpose; a zone that passes everything goes; the devices list; the handover row.
#[test]
fn part_cursor_edits_and_requests() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut part = part_screen(&s, &part_graph(), 10);
    let list: Vec<_> = rows(ui).into_iter().map(|r| (r.0, r.1)).collect();
    let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert_eq!(
        list,
        [("Level", "−6.0 dB"), ("Pan", "R 25"), ("Mute", "Off"), ("Solo", "Off"), ("Plays from", "Any device ›"), ("Lowest key", "C2"), ("Highest key", "C7"), ("Transpose", "−12"), ("Sustain pedal", "None"), ("Sostenuto pedal", "Every pedal"), ("Soft pedal", "Every pedal"), ("Input channel", "Any channel"), ("Lowest velocity", "0"), ("Highest velocity", "127"), ("Notes at once", "Any"), ("Reserved notes", "None"), ("Priority", "0 (gives way first)"), ("Plays to", "Main"), ("MIDI out", "Off ›"), ("MIDI out channel", "Own channels"), ("Send to Reverb", "−6.0 dB"), ("Keyboard", "plays this part ›"), ("Sound", "Rhodes ›"), ("Insert effects", "›"), ("Name", "Keys ›"), ("Remove part", "Press twice")]
    );
    assert_eq!(
        info_lines(ui),
        ["Sound", "Rhodes", "From “Warm Rhodes”, edited", "Engine", "Surge XT", "Plays to", "Main", "Sends", "Reverb  −6.0 dB", "Insert effects", "Chorus (off)"],
        "the sound and engine, where it plays, the sends and the effects, read only"
    );
    assert_eq!(ui.global::<crate::PartState>().get_name().as_str(), "Keys");
    assert_eq!(cursor_at(ui), None, "no cursor until a move");
    // Either pane's knob and the dial move the cursor; the first move shows it.
    part.act(Action::Move { pane: Some(0), by: 1 });
    assert_eq!(cursor_at(ui), Some(0));
    part.act(Action::Move { pane: None, by: 1 });
    mv(&mut part, 1);
    assert_eq!(cursor_at(ui), Some(2));
    part.act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(cursor_at(ui), Some(1));
    part.act(Action::End(1));
    assert_eq!(cursor_at(ui), Some(25), "End: the last");
    part.act(Action::End(-1));
    assert_eq!(cursor_at(ui), Some(0));
    assert!(edits(&mut part).is_empty(), "moving the cursor sends nothing");
    // Level: a press starts changing it; the dial then steps it.
    assert_eq!(part.act(Action::Confirm(None)), Stay);
    assert!(rows(ui)[0].3);
    mv(&mut part, 1);
    let e = edits(&mut part);
    assert_eq!((e[0].0, &e[0].1["part"], &e[0].1["mute"]), ("parts.level", &json!(10), &json!(false)));
    assert_eq!(e[0].1["level"], json!(0.529));
    assert_eq!(rows(ui)[0].1, "−5.5 dB", "shown at once");
    part.act(Action::Cancel(None));
    assert!(!rows(ui)[0].3 && rows(ui)[0].2, "Cancel ends the change and keeps the cursor");
    mv(&mut part, 1);
    assert_eq!(cursor_at(ui), Some(1), "then the dial moves the cursor again");
    // Pan, from 25 right: two steps left, ten more, then to its end.
    press(&mut part);
    mv(&mut part, -2);
    assert_eq!(edits(&mut part), [("parts.pan", json!({"part": 10, "pan": 0.15}))]);
    mv(&mut part, -10);
    assert_eq!(edits(&mut part), [("parts.pan", json!({"part": 10, "pan": -0.35}))]);
    mv(&mut part, -100);
    assert_eq!(edits(&mut part)[0].1["pan"], json!(-1.0));
    mv(&mut part, -1);
    assert!(edits(&mut part).is_empty(), "no request for a value at its end");
    assert_eq!(rows(ui)[1].1, "L 100");
    part.act(Action::Cancel(None));
    // Mute and solo toggle on a press.
    mv(&mut part, 1);
    press(&mut part);
    assert_eq!(edits(&mut part), [("parts.level", json!({"part": 10, "level": 0.529, "mute": true}))]);
    mv(&mut part, 1);
    press(&mut part);
    assert_eq!(edits(&mut part), [("parts.solo", json!({"part": 10, "solo": true}))]);
    assert_eq!((rows(ui)[2].1.as_str(), rows(ui)[3].1.as_str()), ("On", "On"));
    press(&mut part);
    assert_eq!(edits(&mut part), [("parts.solo", json!({"part": 10, "solo": false}))], "a second press undoes it");
    // The key range and transpose: parts.input with the input as it was, one setting changed.
    mv(&mut part, 2); // Lowest key
    press(&mut part);
    mv(&mut part, 4);
    let e = edits(&mut part);
    assert_eq!(e[0].0, "parts.input");
    assert_eq!(e[0].1["part"], json!(10));
    assert_eq!(e[0].1["input"]["zones"], json!([{"keys": [40, 96], "velocities": [0, 1], "channels": 65535, "transpose": -12, "curve": 1, "fixedVelocity": 0}]));
    assert_eq!(e[0].1["input"]["receive"], json!({"sustain": "none"}), "the rest of the input is sent as it was");
    mv(&mut part, 60);
    assert_eq!(edits(&mut part)[0].1["input"]["zones"][0]["keys"], json!([100, 100]), "the highest key is never under the lowest: it comes up with it");
    part.act(Action::Cancel(None));
    mv(&mut part, 1); // Highest key
    press(&mut part);
    mv(&mut part, 100);
    assert_eq!(edits(&mut part)[0].1["input"]["zones"][0]["keys"], json!([100, 127]));
    mv(&mut part, -100);
    assert_eq!(edits(&mut part)[0].1["input"]["zones"][0]["keys"], json!([100, 100]), "and stops at the lowest");
    part.act(Action::Cancel(None));
    mv(&mut part, 1); // Transpose
    press(&mut part);
    mv(&mut part, 40);
    assert_eq!(edits(&mut part)[0].1["input"]["zones"][0]["transpose"], json!(24), "to the JUCE slider's +24");
    assert_eq!(rows(ui)[7].1, "+24");
    // A zone that passes everything is no zone, as the JUCE page's setZone drops it.
    let mut other = part_screen(&s, &part_graph(), 11);
    for _ in 0..8 {
        mv(&mut other, 1);
    }
    assert_eq!(rows(ui)[7].0, "Transpose");
    press(&mut other);
    mv(&mut other, 2);
    assert_eq!(edits(&mut other)[0].1["input"]["zones"], json!([{"keys": [0, 127], "velocities": [0, 1], "channels": 65535, "transpose": 2, "curve": 1, "fixedVelocity": 0}]));
    mv(&mut other, -2);
    assert_eq!(edits(&mut other)[0].1["input"]["zones"], json!([]), "back to everything: no zone");
    // A level at silence still rises.
    let mut quiet = part_graph();
    quiet["parts"][0]["level"] = json!(0.0);
    let mut q = part_screen(&s, &quiet, 10);
    mv(&mut q, 1);
    press(&mut q);
    mv(&mut q, 1);
    assert_eq!(edits(&mut q)[0].1["level"], json!(0.002));
    assert_eq!(rows(ui)[0].1, "−54.0 dB");
    // The devices list: a press opens it, any is ticked, a press ticks another, Cancel closes it.
    let mut part = part_screen(&s, &part_graph(), 10);
    part.endpoints(&json!({"midiInputs": [{"source": 1, "name": "KeyLab 61", "kind": "usb", "port": 0}, {"source": 2, "name": "DIN 1", "kind": "port", "port": 1}, {"source": 0, "name": "Built in", "kind": "builtIn", "port": 0}]}));
    mv(&mut part, 5);
    assert_eq!(cursor_at(ui), Some(0), "the first move shows the cursor");
    mv(&mut part, 4);
    assert_eq!(rows(ui)[4].0, "Plays from");
    press(&mut part);
    let list: Vec<_> = rows(ui).into_iter().map(|r| (r.0, r.1)).collect();
    assert_eq!(list.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Any device", "Built in", "All external", "All USB", "Network", "KeyLab 61", "Port 1 (DIN 1)"], "the built-in input is the Built in choice already");
    assert_eq!(list[0].1, "✓");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Plays from: press to tick or untick");
    mv(&mut part, 1); // the first move shows the cursor: on Any
    mv(&mut part, 5);
    press(&mut part);
    let e = edits(&mut part);
    assert_eq!(e[0].1["input"]["devices"], json!([{"kind": "usbNamed", "port": 0, "pattern": "KeyLab 61", "match": "equals"}]));
    assert_eq!(rows(ui)[0].1, "", "any is no longer ticked");
    mv(&mut part, 1);
    press(&mut part);
    let e = edits(&mut part);
    assert_eq!(e[0].1["input"]["devices"][1], json!({"kind": "port", "port": 1, "pattern": "", "match": "contains"}));
    mv(&mut part, -6);
    press(&mut part); // any clears the list
    assert_eq!(edits(&mut part)[0].1["input"]["devices"], json!([]));
    part.act(Action::Cancel(None));
    assert_eq!(rows(ui)[4].0, "Plays from", "closed: the settings again, the cursor on it");
    // Cancel: the cursor drops, then back.
    assert_eq!(part.act(Action::Cancel(None)), Stay);
    assert_eq!(cursor_at(ui), None);
    assert_eq!(part.act(Action::Cancel(None)), Back);
    // Name: a press asks for the keyboard; Remove part needs a second press.
    part.act(Action::End(1));
    part.act(Action::End(1)); // the first shows the cursor, the second goes to the last
    part.act(Action::Move { pane: None, by: -1 });
    assert_eq!(part.act(Action::Confirm(None)), crate::part::Outcome::Prompt("Name of the part".into(), "Keys".into()));
    part.not_entered();
    part.act(Action::Move { pane: None, by: 1 });
    assert_eq!(part.act(Action::Confirm(None)), Stay);
    assert_eq!(rows(ui)[25].1, "Press again to remove");
    part.act(Action::Move { pane: None, by: -1 });
    part.act(Action::Move { pane: None, by: 1 });
    assert_eq!(rows(ui)[25].1, "Press twice", "moving away undoes the first press");
    assert!(requests(&mut part).is_empty());
    // A bus has the fader's settings and where it plays to; an audio part no keys.
    let _bus = part_screen(&s, &part_graph(), 12);
    assert_eq!(rows(ui).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Level", "Pan", "Mute", "Solo", "Plays to", "Sound", "Insert effects", "Name", "Remove part"]);
    assert_eq!(info_lines(ui), ["Sound", "No instrument", "Plays to", "Main", "Sends", "None", "Insert effects", "Hall"]);
    let mut audio = part_graph();
    audio["parts"][1]["input"] = json!({"kind": "audio", "port": 0});
    let _audio = part_screen(&s, &audio, 11);
    assert_eq!(rows(ui).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Level", "Pan", "Mute", "Solo", "Plays to", "MIDI out", "MIDI out channel", "Send to Reverb", "Keyboard", "Sound", "Insert effects", "Name", "Remove part"]);
    assert_eq!(info_lines(ui)[5], "Reverb", "plays through a bus");
}

// The pedals: each of the three shows its own receive, the dial steps it between every pedal,
// none and the part's devices' own, only that class changing in what is sent; a class at every
// pedal is left out, as the file writes it; the engine's answer shows.
#[test]
fn part_pedal_receive_round_trip() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut part = part_screen(&s, &part_graph(), 10);
    mv(&mut part, 1);
    mv(&mut part, 8);
    assert_eq!((rows(ui)[8].0.as_str(), cursor_at(ui)), ("Sustain pedal", Some(8)));
    mv(&mut part, 1);
    assert_eq!(rows(ui)[9], ("Sostenuto pedal".into(), "Every pedal".into(), true, false));
    press(&mut part);
    mv(&mut part, 1);
    assert_eq!(edits(&mut part)[0].1["input"]["receive"], json!({"sustain": "none", "sostenuto": "none"}));
    mv(&mut part, 1);
    assert_eq!(edits(&mut part)[0].1["input"]["receive"], json!({"sustain": "none", "sostenuto": "own"}));
    assert_eq!(rows(ui)[9].1, "Its devices' pedals");
    mv(&mut part, 5);
    assert!(edits(&mut part).is_empty(), "no further than its devices'");
    mv(&mut part, -2);
    assert_eq!(edits(&mut part)[0].1["input"]["receive"], json!({"sustain": "none"}), "every pedal: left out");
    part.act(Action::Cancel(None));
    // Soft, by touch: + steps it.
    part.step_row(10, 1);
    assert_eq!(edits(&mut part)[0].1["input"]["receive"], json!({"sustain": "none", "soft": "none"}));
    // The engine's graph.get has it: every row shows what it says.
    let mut graph = part_graph();
    graph["parts"][0]["input"]["receive"] = json!({"sostenuto": "own", "soft": "none"});
    part.graph(&graph);
    let values: Vec<_> = rows(ui)[8..11].iter().map(|r| r.1.clone()).collect();
    assert_eq!(values, ["Every pedal", "Its devices' pedals", "None"]);
}

// Part on the shell through a fake engine: a press on a part in Play opens it (controls.changed
// stays subscribed), its edits reach the engine, the engine's receive comes back by graph.changed,
// a refusal is said, touch taps, Back and Cancel return to Play, a tap on a strip's name opens
// it, the list pages, and the handover row gives the display to the JUCE app.
#[test]
fn part_on_the_shell() {
    let mut f = Fixture::new("part");
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(0); // Performance: Play
    for _ in 0..11 {
        let request = f.engine.request();
        let result = match request["method"].as_str().unwrap() {
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => part_graph(),
            "library.query" => json!({"items": [], "total": 0}),
            _ => json!({}),
        };
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        f.pump();
    }
    let ui = f.screen.ui.clone_strong();
    let part = ui.global::<crate::PartState>();
    let graph = std::cell::RefCell::new(part_graph());
    // The next request that is `want`, or an edit; what the shell asks of its own is answered as an engine would.
    let next = |f: &mut Fixture, want: &str| loop {
        let r = f.engine.request();
        let method = r["method"].as_str().unwrap().to_string();
        let result = match method.as_str() {
            _ if method == want => return r,
            "parts.meters" => json!({"meters": []}),
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => graph.borrow().clone(),
            "session.status" => json!({"midiInputs": []}),
            "unsubscribe" | "subscribe" => json!({}),
            "graph.commit" => json!({"revision": 1}),
            _ => return r,
        };
        f.reply(&r, json!({"result": result}));
    };
    let mut total = std::collections::BTreeMap::<&str, i64>::new();
    let mut nav = |f: &mut Fixture, key: &'static str, by: i64| {
        *total.entry(key).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move:1", 0);
    nav(&mut f, "move:1", 1);
    nav(&mut f, "confirm:1", 1);
    assert_eq!((ui.get_screen(), ui.global::<Bar>().get_where().as_str(), part.get_name().as_str()), (4, "Part", "Keys"), "a press on the part opens it");
    // Moves on either pane move the cursor; a press on Pan, the dial; a tap on Mute.
    nav(&mut f, "move:0", 1);
    nav(&mut f, "move", 1);
    assert_eq!(cursor_at(&ui), Some(1));
    nav(&mut f, "confirm", 1);
    nav(&mut f, "move", -1);
    let r = next(&mut f, "parts.pan");
    assert_eq!((r["method"].as_str(), r["params"].clone()), (Some("parts.pan"), json!({"part": 10, "pan": 0.2})));
    f.reply(&r, json!({"result": {"accepted": true}}));
    nav(&mut f, "cancel", 1);
    part.invoke_tapped(2);
    let r = next(&mut f, "parts.level");
    assert_eq!((r["method"].as_str(), r["params"]["mute"].clone()), (Some("parts.level"), json!(true)));
    f.reply(&r, json!({"result": {"accepted": true}}));
    // A refusal is said, and the graph asked again to put the page right.
    part.invoke_tapped(3);
    let r = next(&mut f, "parts.solo");
    assert_eq!(r["method"], "parts.solo");
    f.reply(&r, json!({"error": {"code": -32602, "message": "no such part"}}));
    assert_eq!(part.get_message().as_str(), "The engine refused: no such part");
    // The pedal: sostenuto to none; the engine's graph.changed brings it back as it holds it.
    part.invoke_step(9, 1);
    let r = next(&mut f, "parts.input");
    assert_eq!((r["method"].as_str(), r["params"]["input"]["receive"].clone()), (Some("parts.input"), json!({"sustain": "none", "sostenuto": "none"})));
    f.reply(&r, json!({"result": {"accepted": true}}));
    let r = next(&mut f, "graph.commit");
    f.reply(&r, json!({"result": {"revision": 5}}));
    graph.borrow_mut()["parts"][0]["input"]["receive"] = json!({"sustain": "none", "sostenuto": "none"});
    graph.borrow_mut()["parts"][0]["mute"] = json!(true);
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "graph.changed", "params": {"revision": 5}})));
    f.pump();
    for _ in 0..2 {
        let r = f.engine.request();
        let result = if r["method"] == "graph.get" { graph.borrow().clone() } else { json!({"id": "p1", "name": "Grand", "unsaved": false}) };
        f.reply(&r, json!({"result": result}));
    }
    assert_eq!(rows(&ui)[9].1, "None", "the engine's receive shown");
    assert_eq!(rows(&ui)[2].1, "On", "and its mute");
    // controls.changed moves the level live while it is shown.
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controls.changed", "params": {"parts": [{"part": 10, "level": 0.25, "mute": true}], "master": {"level": 1.0, "pan": 0.0}}})));
    f.pump();
    assert_eq!(rows(&ui)[0].1, "−12.0 dB");
    f.screen.frame();
    f.screen.save("part");
    png(&f.screen, "part");
    // Back (touch) to Play; a tap on a strip's name opens that part; Cancel goes back, and Play's
    // Cancel goes to where Play was opened from.
    part.invoke_back();
    assert_eq!(ui.get_screen(), 3);
    ui.global::<crate::PlayState>().invoke_part_tapped(1);
    assert_eq!((ui.get_screen(), part.get_name().to_string()), (4, "Low".to_string()));
    assert_eq!(cursor_at(&ui), None, "a part opens with no cursor");
    nav(&mut f, "cancel", 1);
    assert_eq!(ui.get_screen(), 3);
    nav(&mut f, "cancel", 1);
    assert_eq!(ui.get_screen(), 0, "Play's Cancel goes Home, where it was opened from");
    let r = next(&mut f, "unsubscribe");
    assert_eq!((r["method"].as_str(), r["params"]["topics"][0].as_str()), (Some("unsubscribe"), Some("controls.changed")), "left only when neither Play nor Part is shown");
    // The list pages (eleven rows fit at 720 px) and the handover row gives the display away.
    ui.global::<HomeState>().invoke_tapped(0);
    ui.global::<crate::PlayState>().invoke_part_tapped(0);
    assert_eq!(ui.get_screen(), 4);
    assert_eq!(rows(&ui).len(), 11);
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    assert_eq!(rows(&ui).iter().map(|r| (r.0.as_str(), r.2)).collect::<Vec<_>>(), [("Insert effects", false), ("Name", false), ("Remove part", true)], "the cursor turned the view");
    // Rename from the shell: a press on Name opens the keyboard over the list; typing goes to it
    // alone (End finds Done), Done sends parts.name and the commit; Cancel closes it with nothing sent.
    shell::act(Action::Move { pane: None, by: -1 });
    shell::set_text_on_screen(true);
    shell::act(Action::Confirm(None));
    let keys = ui.global::<crate::KeyboardState>();
    assert!(keys.get_open());
    assert_eq!((keys.get_before().as_str(), keys.get_after().as_str()), ("Keys", ""), "the caret at the end");
    shell::act(Action::Cancel(None));
    assert!(!keys.get_open() && ui.get_screen() == 4, "Cancel closes the keyboard only");
    shell::act(Action::Confirm(None));
    shell::act(Action::End(-1));
    shell::act(Action::Move { pane: None, by: 2 }); // Clear
    shell::act(Action::Confirm(None));
    assert_eq!(keys.get_before().as_str(), "");
    keys.invoke_pressed(14); // by touch: p
    keys.invoke_pressed(16); // a
    keys.invoke_pressed(18); // d
    assert_eq!(keys.get_before().as_str(), "pad");
    shell::act(Action::End(1));
    shell::act(Action::Confirm(None)); // Done
    assert!(!keys.get_open());
    graph.borrow_mut()["parts"][0]["name"] = json!("pad"); // as the engine would hold it, for any graph.get meanwhile
    let r = next(&mut f, "parts.name");
    assert_eq!((r["method"].as_str(), r["params"].clone()), (Some("parts.name"), json!({"part": 10, "name": "pad"})));
    f.reply(&r, json!({"result": {"accepted": true}}));
    let r = next(&mut f, "graph.commit");
    assert_eq!(r["method"], "graph.commit", "a staged edit is committed, as the JUCE setters do");
    f.reply(&r, json!({"result": {"revision": 6}}));
    assert_eq!(part.get_name().as_str(), "pad");
    // Remove part: two presses, then parts.remove and the commit, and back to Play.
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    shell::act(Action::Confirm(None));
    let r = next(&mut f, "parts.remove");
    assert_eq!((r["method"].as_str(), r["params"].clone()), (Some("parts.remove"), json!({"part": 10})));
    f.reply(&r, json!({"result": {"accepted": true}}));
    assert_eq!(ui.get_screen(), 3, "back to Play");
}

// Part draws in the built-in themes and an odd one (large type, extreme colors), with a value
// being changed, and the devices list.
#[test]
fn part_draws_in_every_theme() {
    fit_to_window();
    let mut s = Screen::new();
    let mut part = part_screen(&s, &part_graph(), 10);
    s.ui.set_screen(4);
    mv(&mut part, 1);
    press(&mut part);
    let places = theme::Places::default();
    let mut loaded: Vec<theme::Loaded> = ["oscairn-dark", "oscairn-light"].iter().map(|id| theme::chosen(&places, id, "")).collect();
    loaded.push(odd_theme());
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        s.save(&format!("part-{}", t.id));
        png(&s, &format!("part-{}", t.id));
    }
    at_fantom_scale(&mut s, "part", || part.graph(&part_graph()));
    part.act(Action::Cancel(None));
    mv(&mut part, 4);
    press(&mut part);
    mv(&mut part, 2);
    assert!(s.frame().is_some());
    png(&s, "part-devices");
}

// A long insert-effects list scrolls: the right knob (pane 1) turns the read-only panel a view at
// a time, the panel's arrows too; the left knob and the dial keep to the settings.
#[test]
fn part_info_scrolls_by_the_right_knob() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut graph = part_graph();
    let mut chain = vec![json!(1)];
    for i in 0..10 {
        graph["nodes"].as_array_mut().unwrap().push(json!({"id": 100 + i, "name": format!("Fx {i}"), "plugin": {"name": "Fx"}, "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0, "enabled": true}));
        chain.push(json!(100 + i));
    }
    graph["parts"][0]["chain"] = json!(chain);
    let mut part = crate::part::Screen::new(&s.ui);
    ui.global::<crate::PartState>().set_rows_per_view(20);
    ui.global::<crate::PartState>().set_info_per_view(6);
    part.graph(&graph);
    part.open(10);
    let p = ui.global::<crate::PartState>();
    assert_eq!((info_lines(ui).len(), p.get_info_more(), info_lines(ui)[0].as_str()), (6, true, "Sound"));
    part.act(Action::Move { pane: Some(1), by: 1 });
    assert_eq!(info_lines(ui)[0], "Sound", "the first move shows the cursor");
    part.act(Action::Move { pane: Some(1), by: 6 });
    assert_ne!(info_lines(ui)[0], "Sound", "past the view: the next view");
    assert_eq!(cursor_at(ui), None, "the settings' cursor did not move");
    part.scroll_info(-1);
    assert_eq!(info_lines(ui)[0], "Sound");
    part.scroll_info(1);
    assert_ne!(info_lines(ui)[0], "Sound");
    part.act(Action::Move { pane: Some(1), by: 1000 });
    assert_eq!(info_lines(ui).last().unwrap(), "Fx 9", "the end of the list");
}
