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
        [("Level", "−6.0 dB"), ("Pan", "R 25"), ("Mute", "Off"), ("Solo", "Off"), ("Plays from", "Any device ›"), ("Lowest key", "C2"), ("Highest key", "C7"), ("Transpose", "−12"), ("Sustain pedal", "None"), ("Sostenuto pedal", "Every pedal"), ("Soft pedal", "Every pedal"), ("Input channel", "Any channel"), ("Lowest velocity", "0"), ("Highest velocity", "127"), ("Notes at once", "Any"), ("Reserved notes", "None"), ("Priority", "0 (gives way first)"), ("Plays to", "Main"), ("Key", "Performance's"), ("MIDI out", "Off ›"), ("MIDI out channel", "Own channels"), ("Send to Reverb", "−6.0 dB"), ("Keyboard", "plays this part ›"), ("Sound", "Rhodes ›"), ("Insert effects", "›"), ("Name", "Keys ›"), ("Remove part", "Press twice")]
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
    assert_eq!(cursor_at(ui), Some(26), "End: the last");
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
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Plays from");
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
    assert_eq!(rows(ui)[26].1, "Press again to remove");
    part.act(Action::Move { pane: None, by: -1 });
    part.act(Action::Move { pane: None, by: 1 });
    assert_eq!(rows(ui)[26].1, "Press twice", "moving away undoes the first press");
    assert!(requests(&mut part).is_empty());
    // A bus has the fader's settings and where it plays to; an audio part no keys.
    let _bus = part_screen(&s, &part_graph(), 12);
    assert_eq!(rows(ui).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Level", "Pan", "Mute", "Solo", "Plays to", "Key", "Sound", "Insert effects", "Name", "Remove part"]);
    assert_eq!(info_lines(ui), ["Sound", "No instrument", "Plays to", "Main", "Sends", "None", "Insert effects", "Hall"]);
    let mut audio = part_graph();
    audio["parts"][1]["input"] = json!({"kind": "audio", "port": 0});
    let _audio = part_screen(&s, &audio, 11);
    assert_eq!(rows(ui).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["Level", "Pan", "Mute", "Solo", "Plays to", "Key", "MIDI out", "MIDI out channel", "Send to Reverb", "Keyboard", "Sound", "Insert effects", "Name", "Remove part"]);
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
    for _ in 0..12 {
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
            "harmony.get" => json!({"key": {"root": 2, "scale": [0, 2, 3, 5, 7, 8, 10], "name": "Minor"}, "parts": [], "scales": ["Major", "Minor"]}),
            "unsubscribe" | "subscribe" | "controllers.view" => json!({}),
            "edit.history" => json!({"undo": "", "redo": ""}),
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
    assert_eq!((ui.get_screen(), title(&ui), part.get_name().as_str()), (4, "Keys".to_string(), "Keys"), "a press on the part opens it");
    // Moves on either pane move the cursor; a press on Pan, the dial; a tap on Mute.
    nav(&mut f, "move:0", 1);
    nav(&mut f, "move", 1);
    assert_eq!(cursor_at(&ui), Some(1));
    nav(&mut f, "confirm", 1);
    nav(&mut f, "move", -1);
    let r = next(&mut f, "parts.pan");
    assert_eq!((r["method"].as_str(), r["params"].clone()), (Some("parts.pan"), json!({"part": 10, "pan": 0.2})));
    f.reply(&r, json!({"result": {"accepted": true}}));
    nav(&mut f, "confirm", 1); // a press ends the change (Cancel would leave the screen)
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
    ui.global::<Bar>().invoke_back_tapped();
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
    // The list pages (twelve rows fit at 720 px, the header row's room being the bar's) and the handover row gives the display away.
    ui.global::<HomeState>().invoke_tapped(0);
    ui.global::<crate::PlayState>().invoke_part_tapped(0);
    assert_eq!(ui.get_screen(), 4);
    assert_eq!(rows(&ui).len(), 12);
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    assert_eq!(rows(&ui).iter().map(|r| (r.0.as_str(), r.2)).collect::<Vec<_>>(), [("Name", false), ("Remove part", true)], "the cursor turned the view");
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

// A performance instrument: with the Part screen open and nothing happening, it asks nothing and draws
// nothing (docs/rust-ui.md, idle); a status that says what the last said costs neither.
#[test]
fn at_rest_it_costs_no_request_and_no_frame() {
    let mut f = Fixture::new("part-idle");
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(0); // Performance: Play
    for _ in 0..12 {
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
    f.screen.ui.global::<crate::PlayState>().invoke_part_tapped(1);
    assert_eq!(f.screen.ui.get_screen(), 4, "the tap opened the part");
    let status = json!({"cpuLoad": 0.42, "audioDevice": "null", "sampleRate": 48000});
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": status}))); // the first: the takes and the meters follow
    f.pump();
    // Everything the shell and the screen ask until they are quiet, as a real engine would.
    let mut quiet = 0;
    while quiet < 3 {
        slint::platform::update_timers_and_animations();
        let Some(request) = f.engine.try_request(std::time::Duration::from_millis(60)) else {
            quiet += 1;
            continue;
        };
        quiet = 0;
        let result = match request["method"].as_str().unwrap() {
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => part_graph(),
            "record.get" => json!({"takes": []}),
            "memory.state" => json!({"systemAvailable": 496u64 << 20, "device": 0}),
            "transport.get" => json!({"tempo": 120.0, "playing": false}),
            "session.status" => json!({"midiInputs": []}),
            "harmony.get" => json!({"key": {"root": 2, "scale": [0, 2, 3, 5, 7, 8, 10], "name": "Minor"}, "parts": [], "scales": ["Major", "Minor"]}),
            "edit.history" => json!({"undo": "", "redo": ""}),
            _ => json!({}),
        };
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        f.pump();
    }
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
    let loaded = all_themes();
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        part.shown(); // the rows a view holds depend on the paddings
        s.frame();
        s.save(&format!("part-{}", t.id));
        png(&s, &format!("part-{}", t.id));
        shot(&s, &format!("part-{}", t.id));
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

// The Part screen's list scrolls the way the shared lists do (issue #269): when the settings hold
// more rows than a view, moving the knob past the last visible row keeps the cursor row in view
// and a scroll bar shows (PartState.total > the view, and its first follows the cursor's page); a
// finger drag or the bar turns the view without moving the cursor.
#[test]
fn the_part_list_keeps_the_cursor_in_view_and_shows_a_scroll_bar() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut part = part_screen(&s, &part_graph(), 10); // a notes part: its settings fill two pages
    ui.global::<crate::PartState>().set_rows_per_view(4);
    part.shown(); // fit to the small view
    let p = ui.global::<crate::PartState>();
    let count = p.get_total();
    assert!(count > p.get_rows_per_view(), "the settings hold more than a view: {count}");
    // The knob or the dial moves the cursor past the last visible row; it stays in view.
    mv(&mut part, 1); // the first move shows the cursor on the first row
    part.act(Action::End(1));
    let shown = rows(ui);
    let at = shown.iter().position(|r| r.2).expect("the cursor is on a shown row") as i32;
    assert_eq!(shown[at as usize].0, "Remove part", "the last setting is in view");
    // A scroll bar shows and follows the cursor's view.
    assert!(p.get_total() > p.get_rows_per_view(), "a scroll bar shows when the list is longer than the view");
    assert_eq!(p.get_first() + at, count - 1, "the bar's thumb sits at the cursor's page");
    // A finger drag turns the view: the first page, the cursor dropped (a scroll selects nothing).
    part.scroll_to(0);
    assert_eq!(p.get_first(), 0);
    assert_eq!(cursor_at(ui), None);
    // A knob move after the scroll shows the cursor again, on the first row shown of the new page.
    mv(&mut part, 1);
    assert_eq!(p.get_first(), 0, "the view does not jump on a move from the top page");
    assert!(cursor_at(ui).is_some(), "the cursor is on a shown row");
}

// The Devices list over the settings is a dialog with rows: when more inputs than a view holds,
// moving the knob to one off the visible rows keeps it on a shown row and a scroll bar shows; a
// finger drag turns the dialog. Issue #269's "a dialog with more rows than fit".
#[test]
fn the_devices_dialog_keeps_the_cursor_in_view_and_shows_a_scroll_bar() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut part = part_screen(&s, &part_graph(), 10);
    let inputs: Vec<serde_json::Value> = (0..20).map(|i| json!({"source": i, "name": format!("Input {i}"), "kind": "usb", "port": i})).collect();
    part.endpoints(&json!({"midiInputs": inputs}));
    ui.global::<crate::PartState>().set_rows_per_view(4);
    part.shown();
    // Open the devices dialog.
    mv(&mut part, 5);
    mv(&mut part, 4); // "Plays from"
    press(&mut part);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Plays from");
    let p = ui.global::<crate::PartState>();
    let count = p.get_total();
    assert!(count > p.get_rows_per_view(), "the devices dialog holds more than a view: {count}");
    // The knob to an input off the visible rows keeps it on a shown row.
    mv(&mut part, 1); // the first move shows the cursor
    part.act(Action::End(1));
    let shown = rows(ui);
    let at = shown.iter().position(|r| r.2).expect("the cursor is on a shown row") as i32;
    assert_eq!(shown[at as usize].0, "Input 19", "the last connected input is in view");
    assert_eq!(p.get_first() + at, count - 1, "the scroll bar follows the cursor's view");
    // A finger drag turns the dialog, the cursor dropped.
    part.scroll_to(0);
    assert_eq!(p.get_first(), 0);
    assert_eq!(cursor_at(ui), None);
    // Cancel still closes the dialog.
    assert_eq!(part.act(Action::Cancel(None)), Stay);
    assert!(rows(ui).iter().any(|r| r.0 == "Plays from"));
}

// Issue #26 item 8: the Part page shows the sostenuto and soft pedals' receive beside the
// sustain's (the engine's per-part pedal receive, `input.receive`), and the Key and Scale rows.
// None of the three pedals may be silently dropped from the list, and the Scale row must appear
// with a part that owns a key. (Reachability without touch and the requests they send are the
// round trips and the reachability walk above.)
#[test]
fn part_shows_each_pedal_receive_and_the_key_and_scale_rows() {
    let s = Screen::new();
    let ui = &s.ui;
    part_screen(&s, &part_graph(), 10);
    // The three pedals' receive rows sit beside each other, in the file's order.
    let labels: Vec<String> = rows(ui).iter().map(|r| r.0.clone()).collect();
    let sustain = labels.iter().position(|l| l == "Sustain pedal").unwrap();
    assert_eq!(&labels[sustain..sustain + 3], ["Sustain pedal", "Sostenuto pedal", "Soft pedal"]);
    // The Key row is always there; the Scale row appears once the part owns a key.
    assert!(rows(ui).iter().any(|r| r.0 == "Key"));
    assert!(!rows(ui).iter().any(|r| r.0 == "Scale"), "no Scale row while it follows the Performance");
    let mut graph = part_graph();
    graph["parts"][0]["key"] = json!({"root": 2, "scale": [0, 2, 3, 5, 7, 8, 10]});
    let mut own = part_screen(&s, &graph, 10);
    harmony(&mut own, minor(), &["Major", "Minor"]);
    let labels: Vec<String> = rows(ui).iter().map(|r| r.0.clone()).collect();
    let key = labels.iter().position(|l| l == "Key").unwrap();
    let scale = labels.iter().position(|l| l == "Scale").unwrap();
    assert!(scale > key, "the Scale row sits after the Key row");
}

// The Key and Scale rows drawn, the Key being changed: a long value ("Performance's (D Minor)") fits or elides.
#[test]
fn part_key_rows_are_drawn() {
    fit_to_window();
    let mut s = Screen::new();
    let mut part = part_screen(&s, &part_graph(), 10);
    harmony(&mut part, minor(), &["Major", "Minor"]);
    mv(&mut part, 1);
    mv(&mut part, 18);
    assert_eq!(rows(&s.ui).iter().find(|r| r.2).map(|r| r.0.as_str()), Some("Key"));
    press(&mut part);
    s.ui.set_screen(4);
    assert!(s.frame().is_some());
    png(&s, "part-key-follows");
    mv(&mut part, 1);
    assert!(s.frame().is_some());
    png(&s, "part-key-own");
    let mut other = Screen::sized(800, 480);
    let mut narrow = part_screen(&other, &part_graph(), 10);
    harmony(&mut narrow, minor(), &["Major", "Minor"]);
    mv(&mut narrow, 1);
    mv(&mut narrow, 18);
    other.ui.set_screen(4);
    assert!(other.frame().is_some());
    png(&other, "part-key-800");
}

fn harmony(part: &mut crate::part::Screen, key: serde_json::Value, scales: &[&str]) {
    let refused = part.reply(crate::part::Request::Harmony, Ok(json!({"key": key, "parts": [], "scales": scales})));
    assert!(!refused);
}

// Reachability: every actionable control of the Part screen reached and activated from Home with
// panel moves only (move, confirm, cancel/EXIT), no touch. The walk turns the dial (Move) over
// the whole list, pages included, records which rows it visited, and presses (Confirm) each: a
// value takes the change (a second press lays it down), a toggle sends its request and a second
// press undoes it, a list of devices opens and EXIT closes it, an action (Sound, Insert effects,
// Keyboard, Name) opens and EXIT comes back from it, and Remove part pressed twice removes the
// part. The walk ends when a move leaves the cursor on the row it came from, past the last, and
// what it visited is held against what the screen declares (its view's "… of N").
#[test]
fn part_reachable_from_home_by_panel_moves() {
    let mut f = Fixture::new("part-reach");
    f.engine.answer(); // session.hello
    f.pump();
    let graph = std::cell::RefCell::new(part_graph());
    let ui = f.screen.ui.clone_strong();
    // A quiet engine: every request out is answered until none comes for a moment.
    let settle = |f: &mut Fixture| {
        while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(12)) {
            let method = r["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "graph.get" => graph.borrow().clone(),
                "library.query" => json!({"items": [], "total": 0}),
                "library.libraries" => json!({"libraries": []}),
                "session.status" => json!({"midiInputs": [], "midiOutputs": []}),
                "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
                "edit.history" | "edit.undo" | "edit.redo" => json!({"undo": "", "redo": ""}),
                "window.info" => json!({"available": false}),
                "presets.list" => json!({"presets": []}),
                "record.get" => json!({"recording": 0, "retro": {"seconds": 30}, "takes": []}),
                "parts.meters" => json!({"meters": []}),
                _ => json!({}),
            };
            f.reply(&r, json!({"result": result}));
        }
    };
    settle(&mut f);
    // Home → the Performance tile → Play.
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |f: &mut Fixture, key: &str, by: i64| {
        *total.entry(key.into()).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move", 0); // the first read is only a baseline
    nav(&mut f, "move", 1); // the cursor onto the Performance tile
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert_eq!(ui.get_screen(), 3, "the Performance tile opens Play");
    // Play: the dial down to the first part, pressed, opens the Part screen.
    nav(&mut f, "move:1", 1);
    nav(&mut f, "confirm:1", 1);
    settle(&mut f);
    assert_eq!((ui.get_screen(), title(&ui)), (4, "Keys".to_string()), "the press on a part opens the Part screen");
    // The whole list the screen declares: the count its view's line ends with ("Settings 1–12 of N"),
    // read while the screen is up, before the walk starts.
    let declared = |ui: &Shell| -> usize {
        let view = ui.global::<crate::PartState>().get_view().to_string();
        view.rsplit(" of ").next().unwrap_or("0").parse().unwrap_or(0)
    };
    let n = declared(&ui);
    // A value row: the press starts the change, the second lays it down. The rows the walk presses
    // and what each press does; anything else it meets is a failure of the walk's own map.
    let edit_rows = ["Level", "Pan", "Plays to", "Key", "Input channel", "Lowest key", "Highest key", "Transpose", "Lowest velocity", "Highest velocity", "Notes at once", "Reserved notes", "Priority", "MIDI out channel", "Send to Reverb", "Sustain pedal", "Sostenuto pedal", "Soft pedal"];
    let part_state = ui.global::<crate::PartState>();
    let (mut visited, mut previous, mut removed) = (Vec::new(), String::new(), false);
    for _ in 0..200 {
        let Some(at) = cursor_at(&ui) else {
            // The cursor dropped (the screen was left and came back): the dial shows it again.
            nav(&mut f, "move", 1);
            settle(&mut f);
            continue;
        };
        let label = rows(&ui)[at].0.clone();
        if label == previous {
            break; // the move left the cursor where it was: past the last row
        }
        if !visited.contains(&label) {
            visited.push(label.clone());
            nav(&mut f, "confirm", 1);
            settle(&mut f);
            if edit_rows.contains(&label.as_str()) {
                assert!(rows(&ui)[at].3, "{label}: the press starts the change");
                nav(&mut f, "confirm", 1); // the change laid down
                settle(&mut f);
                assert!(!rows(&ui)[at].3, "{label}: the second press ends it");
            } else {
                match label.as_str() {
                    "Mute" | "Solo" => {
                        nav(&mut f, "confirm", 1); // pressed again, back where it was
                        settle(&mut f);
                    }
                    "Plays from" | "MIDI out" => {
                        assert_eq!(part_state.get_title().as_str(), label.as_str(), "{label}: the press opens the list of devices");
                        assert_eq!(rows(&ui)[0].0, if label == "Plays from" { "Any device" } else { "Off" });
                        nav(&mut f, "cancel", 1); // EXIT closes the list
                        settle(&mut f);
                        assert_eq!(ui.get_screen(), 4, "{label}: closed, the walk still on the Part screen");
                    }
                    "Keyboard" => {
                        assert!(ui.global::<crate::PianoState>().get_open(), "Keyboard: the press opens the playing keyboard");
                        nav(&mut f, "cancel", 1);
                        settle(&mut f);
                        assert_eq!(ui.get_screen(), 4, "Keyboard: closed, the walk still on the Part screen");
                    }
                    "Name" => {
                        assert!(ui.global::<crate::KeyboardState>().get_open(), "Name: the press opens the text keyboard");
                        nav(&mut f, "cancel", 1);
                        settle(&mut f);
                        assert_eq!(ui.get_screen(), 4, "Name: closed, the walk still on the Part screen");
                    }
                    "Sound" | "Insert effects" => {
                        assert_ne!(ui.get_screen(), 4, "{label}: the press opens its screen");
                        nav(&mut f, "cancel", 1);
                        settle(&mut f);
                        assert_eq!(ui.get_screen(), 4, "{label}: EXIT comes back to the Part screen");
                    }
                    "Remove part" => {
                        nav(&mut f, "confirm", 1); // pressed twice removes it
                        settle(&mut f);
                        removed = true;
                    }
                    _ => panic!("the walk pressed “{label}”, which it knows no effect of"),
                }
            }
        }
        if removed {
            break;
        }
        previous = label;
        nav(&mut f, "move", 1); // the dial on
        settle(&mut f);
    }
    assert!(removed, "the walk never removed the part, which its last row does");
    // What the walk visited against what the screen declares: every one of its N rows, no row twice.
    eprintln!("visited {} of {n}: {visited:?}", visited.len());
    assert!(n > 0, "the screen declares no count");
    assert_eq!(visited.len(), n, "the walk visited {visited:?}, the screen declares {n}");
    assert_eq!(visited.iter().collect::<std::collections::HashSet<_>>().len(), visited.len(), "a row visited twice: {visited:?}");
    assert_eq!(ui.get_screen(), 3, "the part removed, the shell back on Play");
}

fn minor() -> serde_json::Value {
    json!({"root": 2, "scale": [0, 2, 3, 5, 7, 8, 10], "name": "Minor"})
}

// The part's key (C20: RFC 0033, `harmony.set`; the JUCE app has no control for it): it follows the Performance's
// (harmony.get names it) until the dial gives it a root of its own, in the scale the Performance has; the scale row
// then appears and steps through the scales' names; the dial back to the start follows the Performance again.
// Each step is `harmony.set` with the part, then `graph.commit`.
#[test]
fn part_key_follows_the_performance_or_has_its_own_root_and_scale() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut part = part_screen(&s, &part_graph(), 10);
    let key_row = |ui: &Shell| rows(ui).into_iter().find(|r| r.0 == "Key").map(|r| r.1);
    assert_eq!(key_row(ui).as_deref(), Some("Performance's"), "until harmony.get answers");
    harmony(&mut part, minor(), &["Major", "Minor", "Dorian"]);
    assert_eq!(key_row(ui).as_deref(), Some("Performance's (D Minor)"));
    assert!(!rows(ui).iter().any(|r| r.0 == "Scale"), "no scale of its own while it follows");
    let at = rows(ui).iter().position(|r| r.0 == "Key").unwrap();
    mv(&mut part, 1);
    mv(&mut part, at as i32);
    assert_eq!(cursor_at(ui), Some(at));
    press(&mut part);
    mv(&mut part, 1);
    let sent = std::mem::take(&mut part.out);
    assert_eq!(sent.iter().map(|o| o.1).collect::<Vec<_>>(), ["harmony.set", "graph.commit"]);
    assert_eq!(sent[0].2, json!({"part": 10, "key": {"root": 0, "scale": "Minor"}}), "the first root, in the Performance's scale");
    assert_eq!(key_row(ui).as_deref(), Some("C"));
    assert_eq!(rows(ui).iter().find(|r| r.0 == "Scale").map(|r| r.1.clone()).as_deref(), Some("Minor"), "its scale shows now");
    mv(&mut part, 3);
    assert_eq!(edits(&mut part).last().unwrap().1["key"], json!({"root": 3, "scale": "Minor"}));
    assert_eq!(key_row(ui).as_deref(), Some("D♯"));
    part.act(Action::Cancel(None));
    mv(&mut part, 1); // on to Scale
    press(&mut part);
    mv(&mut part, 1);
    assert_eq!(edits(&mut part), [("harmony.set", json!({"part": 10, "key": {"root": 3, "scale": "Dorian"}}))]);
    mv(&mut part, 5);
    assert!(edits(&mut part).is_empty(), "no further than the last scale");
    // The graph says its own key with no name for the scale: it shows as custom, the first step names one.
    let mut graph = part_graph();
    graph["parts"][0]["key"] = json!({"root": 7, "scale": [0, 2, 7], "name": null});
    let mut other = part_screen(&s, &graph, 10);
    harmony(&mut other, json!({"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}), &["Major", "Minor"]);
    assert_eq!(rows(ui).iter().find(|r| r.0 == "Scale").map(|r| r.1.clone()).as_deref(), Some("Custom (0 2 7)"));
    let at = rows(ui).iter().position(|r| r.0 == "Scale").unwrap();
    mv(&mut other, 1);
    mv(&mut other, at as i32);
    press(&mut other);
    mv(&mut other, 1);
    assert_eq!(edits(&mut other), [("harmony.set", json!({"part": 10, "key": {"root": 7, "scale": "Major"}}))]);
    // Back to following: the dial down from the first root.
    let mut back = part_screen(&s, &part_graph(), 10);
    harmony(&mut back, minor(), &["Major", "Minor"]);
    let at = rows(ui).iter().position(|r| r.0 == "Key").unwrap();
    mv(&mut back, 1);
    mv(&mut back, at as i32);
    press(&mut back);
    mv(&mut back, 1);
    edits(&mut back);
    mv(&mut back, -1);
    assert_eq!(edits(&mut back), [("harmony.set", json!({"part": 10, "key": null}))]);
    assert_eq!(key_row(ui).as_deref(), Some("Performance's (D Minor)"));
    assert!(!rows(ui).iter().any(|r| r.0 == "Scale"));
}
