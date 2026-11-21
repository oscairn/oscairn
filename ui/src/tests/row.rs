// SPDX-License-Identifier: AGPL-3.0-or-later
//! Play's knob row (A9 to A11, A16) on its own, against the fake engine's words: what each knob of the
//! FANTOM-07's screen group plays and shows, the Performance's controls or a part's own banks, the part and
//! page published to the engine, a value changed by touch and by the dial, and Learn. The row is drawn by the
//! grid's tiles; the pictures are in target/screens (play-knobs*) and target/theme-shots.

use super::*;
use crate::nav::Action;
use crate::play::Request as PlayRequest;
use crate::row::Request;

/// What the FANTOM-07 has under its screen (its profile's control groups).
fn groups() -> Vec<crate::knobs::Group> {
    crate::profile::Profile::load(Some("fantom-07")).unwrap().controls
}

/// Which of the profile's groups the row shows (the six screen encoders).
fn screen_group() -> usize {
    crate::knobs::knob_group(&groups()).unwrap()
}

fn graph() -> serde_json::Value {
    json!({
        "nodes": [
            {"id": 1, "name": "Rhodes", "audioInputs": [], "audioOutputs": [{"channels": 2}], "noteInputs": 1, "parameterRevision": 4},
            {"id": 2, "name": "Chorus", "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "parameterRevision": 7},
        ],
        "parts": [
            {"id": 10, "name": "Keys", "level": 0.5, "mute": false, "solo": false, "chain": [2, 1], "input": {"kind": "notes", "devices": [], "zones": []}},
            {"id": 11, "name": "Low", "level": 1.0, "mute": true, "solo": false, "chain": [], "input": {"kind": "none"}},
        ],
        "controls": [{"id": 20, "name": "Super", "part": 0, "value": 0.5}, {"id": 21, "name": "Swell", "part": 0, "value": 0.25}],
        "wires": [], "scenes": [{"id": 1, "name": "Verse"}, {"id": 2, "name": "Chorus"}], "scene": 1,
        "master": {"level": 1.0, "pan": 0.0},
    })
}

fn param(id: u32, name: &str, stepped: bool, max: f64) -> serde_json::Value {
    json!({"id": id, "name": name, "module": "", "minimum": 0.0, "maximum": max, "defaultValue": 0.0, "stepped": stepped, "flags": 0})
}

/// The synth's parameters (Gain, Tune) and the chorus's (a stepped Mode, then Depth).
fn list(node: u64) -> serde_json::Value {
    if node == 1 { json!({"parameters": [param(7, "Gain", false, 1.), param(99, "Tune", false, 100.)]}) } else { json!({"parameters": [param(1, "Mode", true, 3.), param(3, "Depth", false, 1.)]}) }
}

/// Answers what the row asked as an engine would: the parameters' lists, values and texts.
fn answer(play: &mut crate::play::Screen, values: &[(u64, u32, f64)]) {
    for _ in 0..4 {
        for (r, m, p) in std::mem::take(&mut play.out) {
            let PlayRequest::Knobs(k) = r else { continue };
            let result = match (m, &k) {
                ("params.list", Request::List(node)) => list(*node),
                ("assignments.get", _) => json!({"assignments": [], "unlinked": [], "physicalControls": {}}),
                ("params.get", _) => json!({"values": values.iter().map(|(n, id, v)| json!({"node": n, "param": id, "value": v})).collect::<Vec<_>>()}),
                ("params.text", Request::Text(..)) => json!({"text": format!("{:.2}", p["value"].as_f64().unwrap())}),
                _ => json!({}),
            };
            play.reply(PlayRequest::Knobs(k), Ok(result));
        }
    }
}

struct Rig {
    s: Screen,
    play: crate::play::Screen,
}

impl Rig {
    fn new() -> Rig {
        fit_to_window();
        let s = Screen::new();
        let mut play = crate::play::Screen::new(&s.ui);
        s.ui.set_screen(3);
        play.knobs(groups());
        play.shown();
        let mut rig = Rig { s, play };
        rig.frame();
        rig.play.graph(&graph());
        rig.frame();
        rig
    }

    fn frame(&mut self) {
        self.s.frame();
        self.play.shown();
        self.s.frame();
    }

    /// What Play asked of the engine, the row's requests as (kind, method, params).
    fn asked(&mut self) -> Vec<(Option<Request>, &'static str, serde_json::Value)> {
        std::mem::take(&mut self.play.out)
            .into_iter()
            .map(|(r, m, p)| (if let PlayRequest::Knobs(k) = r { Some(k) } else { None }, m, p))
            .collect()
    }

    /// Answers what the row asked as an engine would: the parameters' lists, values and texts.
    fn serve(&mut self, values: &[(u64, u32, f64)]) {
        answer(&mut self.play, values);
        self.frame();
    }

    fn tiles(&self) -> Vec<crate::GridTile> {
        self.s.ui.global::<crate::PlayState>().get_knobs().iter().collect()
    }

    fn buttons(&self) -> Vec<(String, bool)> {
        self.s.ui.global::<crate::PlayState>().get_knob_buttons().iter().map(|b| (b.text.to_string(), b.on)).collect()
    }
}

fn views(asked: &[(Option<Request>, &'static str, serde_json::Value)]) -> Vec<serde_json::Value> {
    asked.iter().filter(|a| a.1 == "controllers.view").map(|a| a.2.clone()).collect()
}

// The Performance's controls in the six knobs of the FANTOM's screen group: a knob for each control with its
// name, value and place, the rest saying they play nothing; the part (0) and the group's page published to the
// engine once, as PlayView does (the screen page stays empty); the bank named above.
#[test]
fn the_knobs_show_the_performances_controls_and_the_engine_is_told_what_they_follow() {
    let mut rig = Rig::new();
    let first = rig.asked();
    assert_eq!(views(&first), [json!({"part": 0, "positions": [{"group": screen_group(), "position": 0}]})]);
    assert!(first.iter().all(|a| a.1 != "assignments.get" && a.1 != "params.list"), "the screen group is never overridden, and the Performance's controls need no plugin's list: {first:?}");
    let tiles = rig.tiles();
    assert_eq!(tiles.len(), 6);
    let shown: Vec<(i32, String, String)> = tiles.iter().map(|t| (t.kind, t.name.to_string(), t.value.to_string())).collect();
    assert_eq!(shown[0], (1, "Super".into(), "50%".into()));
    assert_eq!(shown[1], (1, "Swell".into(), "25%".into()));
    assert_eq!(shown[2], (6, "–".into(), String::new()));
    assert!((tiles[0].norm - 0.5).abs() < 1e-6 && (tiles[1].norm - 0.25).abs() < 1e-6);
    assert_eq!(rig.s.ui.global::<crate::PlayState>().get_knob_bank().as_str(), "Controls 1\u{2013}6");
    assert!(rig.buttons().is_empty(), "one bank and no part selected: no buttons");
    // Nothing new is sent while nothing changes.
    rig.play.graph(&graph());
    assert!(views(&rig.asked()).is_empty());
    // A control moved live shows at once.
    rig.play.controls(&json!({"controls": [{"control": 20, "value": 0.8}]}));
    rig.frame();
    assert_eq!(rig.tiles()[0].value.as_str(), "80%");
}

// A part chosen has its own controls: the buttons choose between the Performance's and the part's, the part's
// banks page by the bank button (Assign from the sound's first parameters, then the effects), each change is
// published, and the parameters' values and the plugin's own text are what the tiles say.
#[test]
fn a_part_has_its_banks_and_the_bank_button_pages_them() {
    let mut rig = Rig::new();
    rig.asked();
    // Opening the part (a tap on its strip) selects it.
    let part = rig.play.tap_open(0);
    assert_eq!(part, Some(10));
    rig.frame();
    assert_eq!(rig.buttons(), [("Performance".into(), true), ("Keys".into(), false)]);
    rig.s.frame();
    rig.play.knob_button(1); // the part's own controls
    let asked = rig.asked();
    assert_eq!(views(&asked), [json!({"part": 10, "positions": [{"group": screen_group(), "position": 0}]})]);
    let lists: Vec<u64> = asked.iter().filter_map(|a| if let Some(Request::List(n)) = &a.0 { Some(*n) } else { None }).collect();
    assert_eq!(lists, [2, 1], "the plugins of the part, in its chain's order");
    // Put the asks back for the engine's answers.
    for (r, m, p) in asked {
        rig.play.out.push((PlayRequest::Knobs(r.unwrap()), m, p));
    }
    rig.serve(&[(1, 7, 0.25), (1, 99, 40.)]);
    let tiles = rig.tiles();
    assert_eq!((tiles[0].name.as_str(), tiles[0].value.as_str(), tiles[1].name.as_str(), tiles[1].value.as_str()), ("Gain", "0.25", "Tune", "40.00"));
    assert_eq!(rig.s.ui.global::<crate::PlayState>().get_knob_bank().as_str(), "Assign 1\u{2013}6");
    assert_eq!(rig.buttons(), [("Performance".into(), false), ("Keys".into(), true), ("Bank 1/2".into(), false)]);
    // The next bank: the chorus's main control, the first that is not the stepped Mode.
    rig.play.knob_button(2);
    let asked = rig.asked();
    assert_eq!(views(&asked), [json!({"part": 10, "positions": [{"group": screen_group(), "position": 1}]})]);
    rig.frame();
    assert_eq!((rig.tiles()[0].name.as_str(), rig.s.ui.global::<crate::PlayState>().get_knob_bank().as_str()), ("Depth", "Effects 1\u{2013}6"));
    assert_eq!(rig.buttons()[2].0, "Bank 2/2");
    // Back to the Performance's controls: part 0, page 0.
    rig.play.knob_button(0);
    assert_eq!(views(&rig.asked()), [json!({"part": 0, "positions": [{"group": screen_group(), "position": 0}]})]);
    rig.frame();
    assert_eq!(rig.tiles()[0].name.as_str(), "Super");
}

// What the knobs follow, the Performance's controls or the selected part's own, is a column of two tabs of the
// shell's tab family (the bank's button is not one: it cycles); the one followed has the selected fill, the stop
// the cursor is on the ring, a tap on the other follows it.
#[test]
fn what_the_knobs_follow_is_a_column_of_tabs() {
    let mut rig = Rig::new();
    rig.play.tap_open(0);
    rig.frame();
    let ui = rig.s.ui.clone_strong();
    let state = ui.global::<crate::PlayState>();
    let names = || state.get_column_names().iter().map(|n| n.to_string()).collect::<Vec<_>>();
    assert_eq!((names(), state.get_column_current(), state.get_column_cursor()), (vec!["Performance".to_string(), "Keys".to_string()], 0, -1));
    rig.play.knob_button(1);
    rig.frame();
    assert_eq!((state.get_column_current(), state.get_column_cursor()), (1, -1), "a tap follows the part's controls");
    rig.serve(&[(1, 7, 0.25), (1, 99, 40.)]);
    // The bank's button, a second stop here, is not in the column.
    assert_eq!(names().len(), 2);
    assert_eq!(rig.buttons().len(), 3);
    // The cursor on the first of the stops (the Performance's tab): a ring, and the selected fill is on the part's tab.
    rig.play.act(Action::Move { pane: Some(0), by: 1 });
    rig.play.act(Action::Move { pane: Some(0), by: 5 });
    rig.play.act(Action::Move { pane: Some(0), by: 1 });
    rig.frame();
    assert_eq!((state.get_column_current(), state.get_column_cursor()), (1, 0));
    // Painted: the selected tab's fill is in the column's place at the right of the knob row, and the other tab's.
    let theme = ui.global::<crate::Theme>();
    let (on, off) = (theme.get_tab_on(), theme.get_tab_off());
    let count = |c: slint::Color| rig.s.pixels.iter().filter(|p| (p.r, p.g, p.b) == (c.red(), c.green(), c.blue())).count();
    assert!(count(on) > 800 && count(off) > 800, "{} pixels of the selected fill, {} of the other's", count(on), count(off));
    // A press on the stop under the cursor follows it.
    rig.play.act(Action::Confirm(None));
    rig.frame();
    assert_eq!(state.get_column_current(), 0);
}

// Pictures for a look by eye: Play with a part selected, so the column of two tabs shows, in every theme, both tab
// styles, at 1 and at the keyboard's 1.55: target/theme-shots/play-tabs-THEME-STYLE@SCALE.png.
#[test]
fn the_column_of_tabs_draws_in_every_theme() {
    let mut rig = Rig::new();
    rig.play.tap_open(0);
    rig.play.knob_button(1);
    rig.serve(&[(1, 7, 0.25), (1, 99, 40.)]);
    rig.play.act(Action::Move { pane: Some(0), by: 1 });
    rig.play.act(Action::Move { pane: Some(0), by: 6 });
    for scale in [1., 1.55] {
        rig.s.set_scale(scale);
        for t in &all_themes() {
            for (style, label) in [(crate::TabStyle::Folder, "folder"), (crate::TabStyle::Segmented, "segmented")] {
                theme::apply(&rig.s.ui, t);
                rig.s.ui.global::<crate::Theme>().set_tabs(style);
                rig.frame();
                shot(&rig.s, &format!("play-tabs-{}-{label}@{scale}", t.id));
            }
        }
    }
}

// A value by touch: a finger on a knob begins a gesture, each drag sets the parameter across its range (a
// stepped one rounded), the release ends it; a Performance control and a part's level are set as the JUCE
// slider sets them.
#[test]
fn a_knob_is_turned_by_touch() {
    let mut rig = Rig::new();
    rig.play.tap_open(0);
    rig.play.knob_button(1);
    rig.serve(&[(1, 7, 0.25), (1, 99, 40.)]);
    rig.asked();
    rig.play.knob_grab(1); // Tune: 0 to 100
    rig.play.knob_drag(1, 0.5);
    rig.play.knob_drag(1, 0.75);
    rig.play.knob_release();
    let asked: Vec<(&str, serde_json::Value)> = rig.asked().into_iter().filter(|a| a.1 != "params.text").map(|a| (a.1, a.2)).collect();
    assert_eq!(
        asked,
        [
            ("params.gesture", json!({"node": 1, "param": 99, "begin": true})),
            ("params.set", json!({"values": [{"node": 1, "param": 99, "value": 50.0}]})),
            ("params.set", json!({"values": [{"node": 1, "param": 99, "value": 75.0}]})),
            ("params.gesture", json!({"node": 1, "param": 99, "begin": false})),
        ]
    );
    rig.frame();
    // A Performance control.
    rig.play.knob_button(0);
    rig.frame();
    rig.asked();
    rig.play.knob_grab(0);
    rig.play.knob_drag(0, 0.3);
    rig.play.knob_release();
    let asked: Vec<(&str, serde_json::Value)> = rig.asked().into_iter().map(|a| (a.1, a.2)).collect();
    assert_eq!(asked, [("controls.set", json!({"control": 20, "value": 0.3}))]);
}

// By the buttons and the dial: the cursor goes from the last scene into the row (buttons first, then the knobs
// that play something), a press takes a knob and the dial changes it, Cancel ends that, then the cursor, and the
// dial leaves the row downward to the parts and upward to the scenes.
#[test]
fn the_knobs_are_reached_and_changed_without_touch() {
    let mut rig = Rig::new();
    rig.play.tap_open(0);
    rig.play.knob_button(1);
    rig.serve(&[(1, 7, 0.25), (1, 99, 40.)]);
    rig.asked();
    // From the last scene (the + Scene tile is the third) the dial goes on into the row: the first stop is a button.
    rig.play.act(Action::Move { pane: Some(0), by: 1 });
    rig.play.act(Action::Move { pane: Some(0), by: 5 });
    rig.play.act(Action::Move { pane: Some(0), by: 1 });
    let cursors = |rig: &Rig| {
        let state = rig.s.ui.global::<crate::PlayState>();
        (state.get_knob_buttons().iter().position(|b| b.cursor), state.get_knobs().iter().position(|t| t.cursor), state.get_scenes().iter().position(|t| t.cursor))
    };
    rig.frame();
    assert_eq!(cursors(&rig), (Some(0), None, None), "the Performance button");
    rig.play.act(Action::Move { pane: None, by: 3 });
    rig.frame();
    assert_eq!(cursors(&rig), (None, Some(0), None), "past the three buttons, the first knob");
    // A press takes it; the dial moves Gain by a hundredth of its range a click.
    rig.play.act(Action::Confirm(None));
    rig.asked();
    rig.play.act(Action::Move { pane: None, by: 2 });
    let asked: Vec<(&str, serde_json::Value)> = rig.asked().into_iter().filter(|a| a.1 != "params.text").map(|a| (a.1, a.2)).collect();
    assert_eq!(asked[0], ("params.set", json!({"values": [{"node": 1, "param": 7, "value": 0.26}]})), "{asked:?}");
    assert_eq!(rig.play.act(Action::Cancel(None)), crate::play::Outcome::Stay);
    let ended = rig.asked();
    assert_eq!(ended.last().map(|a| (a.1, a.2.clone())), Some(("params.gesture", json!({"node": 1, "param": 7, "begin": false}))));
    rig.frame();
    assert_eq!(cursors(&rig).1, Some(0), "the cursor stays on it");
    // Down past the last knob goes to the first part, up past the first button back to the last scene.
    rig.play.act(Action::Move { pane: None, by: 1 });
    rig.play.act(Action::Move { pane: None, by: 1 });
    rig.frame();
    assert_eq!(rig.s.ui.global::<crate::PlayState>().get_strips().iter().position(|s| s.cursor), Some(0));
    rig.play.act(Action::Move { pane: None, by: -1 });
    rig.frame();
    assert_eq!(cursors(&rig).1, Some(1), "back up into the row at its last knob");
    rig.play.act(Action::Cancel(None));
    assert_eq!(cursors(&rig), (None, None, None), "Cancel drops the cursor");
}

// Learn (A11): the top bar's action turns it on; a control moved on the panel (the engine's `controllers`
// says which) is named; a part's level, its M, a scene or a knob touched is what that control plays from then
// on (`assignments.set`); Default removes the override; Done and Cancel give up.
#[test]
fn learn_names_what_a_moved_control_plays() {
    let mut rig = Rig::new();
    assert!(rig.play.can_learn(), "the FANTOM-07 has assignable knobs");
    rig.asked();
    rig.play.learn_toggle();
    let asked = rig.asked();
    assert_eq!(asked.iter().map(|a| a.1).collect::<Vec<_>>(), ["controllers.get"]);
    rig.play.reply(PlayRequest::Knobs(Request::Touches), Ok(json!({"touched": "knob:1", "touches": 5})));
    rig.frame();
    let ui = rig.s.ui.clone_strong();
    let state = ui.global::<crate::PlayState>();
    assert_eq!((state.get_learning(), state.get_learn_text().as_str()), (true, "Learn: move a knob on the panel"));
    // Moves before Learn do not count; the next does.
    rig.play.controllers(&json!({"touched": "knob:1", "touches": 5}));
    rig.play.controllers(&json!({"touched": "knob:3", "touches": 6}));
    rig.frame();
    assert_eq!(state.get_learn_text().as_str(), "Knob 3: touch what it should play: a strip, its M, a scene or a knob");
    assert!(state.get_learn_reset_shown());
    // A scene touched names it, and does not play it.
    rig.play.tap_scene(1);
    let asked = rig.asked();
    assert_eq!(asked.iter().map(|a| (a.1, a.2.clone())).collect::<Vec<_>>(), [("assignments.set", json!({"control": "knob:3", "kind": "scene", "id": 2}))]);
    assert!(!rig.play.learning());
    rig.play.reply(PlayRequest::Knobs(Request::Assign), Ok(json!({"accepted": true})));
    assert!(rig.asked().iter().any(|a| a.1 == "assignments.get"), "the overrides are read again");
    // A part's M: its mute, not flipped.
    rig.play.learn_toggle();
    rig.asked();
    rig.play.reply(PlayRequest::Knobs(Request::Touches), Ok(json!({"touched": "knob:3", "touches": 6})));
    rig.play.controllers(&json!({"touched": "fader:2", "touches": 7}));
    rig.play.tap_mute(1);
    let asked = rig.asked();
    assert_eq!(asked.iter().map(|a| (a.1, a.2.clone())).collect::<Vec<_>>(), [("assignments.set", json!({"control": "fader:2", "kind": "partMute", "id": 11}))]);
    // Default: the control back to what it plays by default.
    rig.play.learn_toggle();
    rig.asked();
    rig.play.reply(PlayRequest::Knobs(Request::Touches), Ok(json!({"touched": "fader:2", "touches": 7})));
    rig.play.controllers(&json!({"touched": "knob:2", "touches": 8}));
    rig.play.learn_button(crate::row::Stop::Default);
    assert_eq!(rig.asked().iter().map(|a| (a.1, a.2.clone())).collect::<Vec<_>>(), [("assignments.set", json!({"control": "knob:2", "kind": "none"}))]);
    // Done, and Cancel with no cursor, give up.
    rig.play.learn_toggle();
    rig.play.learn_button(crate::row::Stop::Done);
    assert!(!rig.play.learning());
    rig.play.learn_toggle();
    assert_eq!(rig.play.act(Action::Cancel(None)), crate::play::Outcome::Stay);
    assert!(!rig.play.learning(), "Cancel ended Learn before it leaves Play");
}

// A device with no knobs of its own to move has no Learn (the action is the device's).
#[test]
fn a_device_without_assignable_knobs_has_no_learn() {
    let s = Screen::new();
    let mut play = crate::play::Screen::new(&s.ui);
    play.knobs(vec![]);
    assert!(!play.can_learn(), "a computer's four knobs have nothing to move");
}

// The row on the shell's windows: laid out for any width and under every theme, inside its area, in the
// grid's own tile sizes; pictures at 1280x720 and the FANTOM's 1.55, and at an 800x480 device.
#[test]
fn the_row_reflows_and_is_drawn_in_every_theme() {
    for (size, scale) in [((1280, 720), 1.), ((1280, 720), 1.55), ((800, 480), 1.), ((1920, 1080), 2.)] {
        let mut s = Screen::sized(size.0, size.1);
        s.set_scale(scale);
        let mut play = crate::play::Screen::new(&s.ui);
        s.ui.set_screen(3);
        play.knobs(groups());
        s.frame();
        play.shown();
        play.graph(&graph());
        // A part selected: the three buttons.
        play.tap_open(0);
        play.knob_button(1);
        answer(&mut play, &[(1, 7, 0.25), (1, 99, 40.)]);
        s.frame();
        play.shown();
        s.frame();
        for t in &all_themes() {
            theme::apply(&s.ui, t);
            s.frame();
            play.shown();
            s.frame();
            let state = s.ui.global::<crate::PlayState>();
            let (w, h) = (state.get_knob_width(), state.get_knob_height());
            let tiles: Vec<crate::GridTile> = state.get_knobs().iter().collect();
            assert_eq!(tiles.len(), 6);
            for (i, t) in tiles.iter().enumerate() {
                assert!(t.x >= 0. && t.x + t.w <= w + 0.01 && t.y >= 0. && t.y + t.h <= h + 0.01, "tile {i} inside the row's area {w}x{h} at {size:?}@{scale}");
                if i > 0 {
                    assert!(tiles[i - 1].x + tiles[i - 1].w <= t.x + 0.01, "tiles {} and {i} apart", i - 1);
                }
            }
            assert!(state.get_knob_height() >= 80., "a knob has room to be turned: {h}");
            shot(&s, &format!("play-knobs-{}-{}x{}@{scale}", t.id, size.0, size.1));
        }
        theme::apply(&s.ui, &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
        s.frame();
        play.shown();
        s.frame();
        png(&s, &format!("play-knobs-{}x{}@{scale}", size.0, size.1));
    }
}

// Learn waiting for a target, the cursor on a knob and on Learn's buttons: pictures.
#[test]
fn learn_and_the_cursor_are_drawn() {
    let mut rig = Rig::new();
    rig.play.tap_open(0);
    rig.play.knob_button(1);
    rig.serve(&[(1, 7, 0.25), (1, 99, 40.)]);
    rig.play.learn_toggle();
    rig.asked();
    rig.play.reply(PlayRequest::Knobs(Request::Touches), Ok(json!({"touched": "knob:1", "touches": 1})));
    rig.play.controllers(&json!({"touched": "knob:3", "touches": 2}));
    rig.play.act(Action::Arrow { dx: 0, dy: 1 });
    rig.play.act(Action::Move { pane: None, by: 3 });
    rig.frame();
    png(&rig.s, "play-knobs-learn");
    rig.s.set_scale(1.55);
    rig.frame();
    png(&rig.s, "play-knobs-learn@1.55");
    for t in &all_themes() {
        theme::apply(&rig.s.ui, t);
        rig.frame();
        shot(&rig.s, &format!("play-knobs-learn-{}@1.55", t.id));
    }
}

// Play's areas at the keyboard's scale and the other sizes: the reference (PlayView.cpp, `tight`) gives a screen
// 826 across, less the top bar, 44 to the scenes, a quarter of its height to the knob row (the buttons in a column
// beside the tiles, not a row of their own) and the rest to the strips, with no line under them. Here nothing
// overlaps, each area keeps what it is given, the faders keep a travel, no scene name of eight characters is cut
// and the knobs that play nothing say a dash.

/// The least a part strip's fader travels, in logical pixels, with nothing but the strips below the knob row (the
/// roomy theme, whose paddings are 1.6 times larger, is the tightest case at the keyboard's size: 78).
const FADER_TRAVEL: f32 = 72.;
/// The same with one of the no-MIDI line, Learn's line and the line under the strips shown as well (two of them at
/// once take what is left: 15 in the roomy theme at the keyboard's size).
const FADER_TRAVEL_WITH_A_LINE: f32 = 36.;

fn big_graph(parts: usize) -> serde_json::Value {
    let mut g = graph();
    g["parts"] = (0..parts).map(|i| json!({"id": 10 + i, "name": format!("Part {}", i + 1), "level": 0.5, "mute": false, "solo": false, "chain": [], "input": {"kind": "none"}})).collect();
    g["scenes"] = ["Verse", "Chorus 2", "Bridge", "Outro", "Intro", "Solo", "Break", "Coda"].iter().enumerate().map(|(i, n)| json!({"id": i + 1, "name": n})).collect();
    g
}

#[test]
fn play_divides_the_keyboards_screen_and_the_others_without_overlap() {
    let sizes = [((1280, 720), 1.55), ((1280, 720), 1.), ((800, 480), 1.)];
    let (mut plain_least, mut line_least) = (f32::MAX, f32::MAX);
    for (size, scale) in sizes {
        for hint in [false, true] {
            for (state, parts, learn) in [("plain", 4, false), ("part", 5, false), ("paged", 14, false), ("learn", 5, true)] {
                fit_to_window();
                let mut s = Screen::sized(size.0, size.1);
                s.set_scale(scale);
                let mut play = crate::play::Screen::new(&s.ui);
                s.ui.set_screen(3);
                play.knobs(groups());
                play.offer_keyboard(true, false);
                if hint {
                    play.status(&json!({"midiInputs": []}));
                }
                s.frame();
                play.shown();
                play.graph(&big_graph(parts));
                if state != "plain" {
                    play.tap_open(0);
                }
                if state == "part" {
                    play.knob_button(1);
                }
                answer(&mut play, &[(1, 7, 0.25), (1, 99, 40.)]);
                if learn {
                    play.learn_toggle();
                }
                s.frame();
                play.shown();
                s.frame();
                for t in &all_themes() {
                    theme::apply(&s.ui, t);
                    s.frame();
                    play.shown();
                    s.frame();
                    if state == "paged" {
                        s.ui.global::<crate::PlayState>().set_message("The engine refused: a long reason that fills the line".into());
                        s.frame();
                    }
                    let what = format!("{state} hint {hint} at {size:?}@{scale} in {}", t.id);
                    let ui = &s.ui;
                    let l = ui.global::<crate::PlayState>().get_layout();
                    let margin = ui.global::<crate::Theme>().get_margin();
                    let line = |y: f32, h: f32| (y, y + h);
                    let areas = [
                        ("scenes", line(margin, l.scenes_height)),
                        ("hint", line(l.hint_y, l.hint_height)),
                        ("knobs", line(l.knobs_y, l.knob_height)),
                        ("banner", line(l.banner_y, l.banner_height)),
                        ("strips", line(l.strips_y, l.strips_height)),
                        ("footer", line(l.footer_y, l.footer_height)),
                    ];
                    let mut at = 0.;
                    for (name, (top, bottom)) in areas {
                        assert!(top >= at - 0.01 || bottom - top < 0.01, "{what}: the {name} starts at {top} over what ends at {at}");
                        assert!(bottom <= l.height + 0.01, "{what}: the {name} ends at {bottom} past the screen's {}", l.height);
                        if bottom - top > 0.01 {
                            at = bottom;
                        }
                    }
                    assert!(l.strips_height > 0., "{what}: strips");
                    // The scene tiles: eight across the width.
                    let scenes = ui.global::<crate::PlayState>().get_scenes().row_count();
                    assert_eq!(scenes, 8, "{what}");
                    assert!(8. * l.scene_width <= l.width - 2. * margin + 0.01, "{what}: scene tiles {} wide", l.scene_width);
                    // A scene's name is whole, or shrunk, never cut.
                    assert_eq!(play.scene_cuts().len(), 8, "{what}: every tile says whether its name fits");
                    assert!(!play.scene_cuts().iter().any(|c| *c), "{what}: a scene name is cut: {:?}", play.scene_cuts());
                    // The fader's travel.
                    let lines = usize::from(hint) + usize::from(learn) + usize::from(l.footer_height > 0.);
                    let least = if lines > 0 { FADER_TRAVEL_WITH_A_LINE } else { FADER_TRAVEL };
                    if lines < 2 {
                        let seen = if lines > 0 { &mut line_least } else { &mut plain_least };
                        *seen = seen.min(l.fader_travel);
                        assert!(l.fader_travel >= least, "{what}: the faders travel {} (at least {least})", l.fader_travel);
                    }
                    // The knobs that play nothing say a dash, not cut words.
                    if state == "plain" || state == "part" {
                        let tiles: Vec<crate::GridTile> = ui.global::<crate::PlayState>().get_knobs().iter().collect();
                        assert!(tiles.iter().all(|t| t.name.as_str() != "No control"), "{what}");
                    }
                    shot(&s, &format!("play-fit-{state}{}-{}-{}x{}@{scale}", if hint { "-hint" } else { "" }, t.id, size.0, size.1));
                }
            }
        }
    }
    eprintln!("fader travel, least seen: {plain_least} plain, {line_least} with a line");
}

// The test above can fail: a name too long for its tile, even at 70% of its size, is said to be cut.
#[test]
fn a_scene_name_too_long_is_said_to_be_cut() {
    let mut rig = Rig::new();
    let mut g = big_graph(2);
    g["scenes"][1]["name"] = json!("A scene named at great length");
    rig.play.graph(&g);
    rig.frame();
    let cuts = rig.play.scene_cuts();
    assert_eq!((cuts[0], cuts[1]), (false, true));
}
