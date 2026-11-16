// SPDX-License-Identifier: AGPL-3.0-or-later
//! A list screen lays out again when its room changes, not only when it is shown: Play, the Part
//! screen, the chain, the sound chooser and the Parts list, on the shell, switched to another theme
//! (spacing and type) while shown. Before, the old row count stayed until the screen was shown again.
//! And only one cursor shows: the screen's own is not drawn while the top bar has it.

use super::bar::{play_on_the_shell, request_in};
use super::*;
use crate::{PartState, PartsState, PlayState};

fn strips(ui: &Shell) -> (usize, usize) {
    (ui.global::<PlayState>().get_strips().row_count(), ui.global::<PlayState>().get_strips_per_view() as usize)
}

fn list(ui: &Shell) -> (usize, usize) {
    (ui.global::<PartState>().get_rows().row_count(), ui.global::<PartState>().get_rows_per_view() as usize)
}

fn parts(ui: &Shell) -> (usize, i32) {
    (ui.global::<PartsState>().get_rows().row_count(), ui.global::<PartsState>().get_row_pitch() as i32)
}

/// A Performance of many parts, the first with a long chain, so every list is longer than a view.
fn big_graph() -> serde_json::Value {
    let mut g = super::part::part_graph();
    let mut chain = vec![json!(1)];
    for i in 0..14 {
        g["nodes"].as_array_mut().unwrap().push(json!({"id": 100 + i, "name": format!("Fx {i}"), "plugin": {"name": "Fx"}, "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0, "enabled": true}));
        chain.push(json!(100 + i));
    }
    g["parts"][0]["chain"] = json!(chain);
    for i in 0..14 {
        let part = json!({"id": 30 + i, "name": format!("Part {i}"), "level": 0.5, "pan": 0.0, "mute": false, "solo": false, "chain": [], "bus": 0, "sends": [], "input": {"kind": "none"}});
        g["parts"].as_array_mut().unwrap().push(part);
    }
    g
}

/// How many pixels of the window, from row `from` to row `to`, are exactly the odd theme's cursor color.
fn focus_pixels(s: &Screen, from: usize, to: usize) -> usize {
    let focus = slint::Rgb8Pixel { r: 0, g: 0, b: 255 };
    s.pixels[from * s.w..to * s.w].iter().filter(|p| **p == focus).count()
}

// Only one cursor shows: with the cursor in the top bar (Tab, the third pane, the dial up out of the list) the
// screen's own is not drawn, and it is back when the cursor is given back. Counted in pixels of the odd theme,
// whose cursor color nothing else on the screens has, on every screen that has the bar's actions.
#[test]
fn only_one_cursor_shows_the_bars_or_the_screens() {
    let mut f = play_on_the_shell("one-cursor", json!({"id": "p1", "name": "Grand", "unsaved": false}), big_graph());
    let ui = f.screen.ui.clone_strong();
    theme::apply(&ui, &odd_theme());
    f.screen.frame();
    let bar = ui.global::<crate::Bar>().get_actions().row_count();
    assert!(bar > 0);
    // Play: a strip has the cursor.
    shell::act(Action::Move { pane: Some(1), by: 1 });
    f.screen.frame();
    let (top, below) = (focus_pixels(&f.screen, 0, 48), focus_pixels(&f.screen, 48, f.screen.h));
    assert!(below > 0 && top == 0, "the strip's cursor shows alone: {top} in the bar, {below} below");
    f.screen.save("one-cursor-screen");
    assert!(shell::key("\t"), "Tab: the cursor goes to the bar");
    f.screen.frame();
    let (top, below) = (focus_pixels(&f.screen, 0, 48), focus_pixels(&f.screen, 48, f.screen.h));
    assert!(top > 0 && below == 0, "the bar's cursor shows alone: {top} in the bar, {below} below");
    f.screen.save("one-cursor-bar");
    png(&f.screen, "one-cursor-bar");
    assert!(shell::key("\t"), "Tab again: given back");
    f.screen.frame();
    let (top, below) = (focus_pixels(&f.screen, 0, 48), focus_pixels(&f.screen, 48, f.screen.h));
    assert!(below > 0 && top == 0, "the strip's again: {top} in the bar, {below} below");
    // The Parts list, a cell with the cursor, then the bar's.
    // The roomy theme's type folds the actions into "More" at this width: Edit is the last row of its menu.
    if ui.global::<crate::Bar>().get_more() {
        ui.global::<crate::Bar>().invoke_more_tapped();
        ui.global::<crate::MenuState>().invoke_tapped(bar as i32 - 1);
    } else {
        ui.global::<crate::Bar>().invoke_action(bar as i32 - 1);
    }
    assert_eq!(ui.get_screen(), 9);
    shell::act(Action::Move { pane: Some(0), by: 1 });
    f.screen.frame();
    let (top, below) = (focus_pixels(&f.screen, 0, 48), focus_pixels(&f.screen, 48, f.screen.h));
    assert!(below > 0 && top == 0, "the list's cursor shows alone: {top} in the bar, {below} below");
    shell::act(Action::Move { pane: Some(2), by: 1 });
    f.screen.frame();
    let (top, below) = (focus_pixels(&f.screen, 0, 48), focus_pixels(&f.screen, 48, f.screen.h));
    assert!(top > 0 && below == 0, "the bar's alone: {top} in the bar, {below} below");
    // The dial up out of the list's first cell is the bar's too, and Cancel gives it back.
    shell::act(Action::Cancel(None));
    shell::act(Action::Move { pane: None, by: -1 });
    f.screen.frame();
    assert!(focus_pixels(&f.screen, 0, 48) > 0 && focus_pixels(&f.screen, 48, f.screen.h) == 0);
    shell::act(Action::Cancel(None));
    f.screen.frame();
    assert!(focus_pixels(&f.screen, 48, f.screen.h) > 0 && focus_pixels(&f.screen, 0, 48) == 0, "given back");
}

// Play, the Part screen, the chain, the sound chooser and the Parts list count the rows their room holds again
// when the theme changes under them (spacing and type), with no help from the test: the window's own
// change of size reaches them.
#[test]
fn list_screens_lay_out_again_when_the_theme_changes() {
    let mut f = play_on_the_shell("layout", json!({"id": "p1", "name": "Grand", "unsaved": false}), big_graph());
    let ui = f.screen.ui.clone_strong();
    let none = json!({"undo": "", "redo": ""});
    let graph = big_graph();
    f.screen.frame();
    // Each screen: the rows now, another theme, the rows then. The tight, joined theme has no gap between sections.
    let mut seen = vec![];
    let mut check = |f: &mut Fixture, name: &str, what: &dyn Fn(&Shell) -> (usize, usize)| {
        f.screen.frame();
        let before = what(&ui);
        assert_eq!(before.0, before.1, "{name}: a longer list shows what the room holds");
        theme::apply(&ui, &joined_theme());
        f.screen.frame();
        let tight = what(&ui);
        theme::apply(&ui, &odd_theme());
        f.screen.frame();
        let roomy = what(&ui);
        theme::apply(&ui, &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
        f.screen.frame();
        let back = what(&ui);
        assert_eq!(tight.0, tight.1, "{name}: the rows shown are the room's count at once");
        assert_eq!(roomy.0, roomy.1, "{name}: and again");
        assert_eq!(back, before, "{name}: back where it was");
        assert_ne!(tight, roomy, "{name}: another theme, another count: {tight:?} against {roomy:?}");
        seen.push((name.to_string(), before, tight, roomy));
    };
    check(&mut f, "play", &|ui| strips(ui));
    // The Part screen, from a strip.
    ui.global::<PlayState>().invoke_part_tapped(0);
    assert_eq!(ui.get_screen(), 4);
    check(&mut f, "part", &|ui| list(ui));
    // The chain: Insert effects is three rows up from the end of the Part screen's list.
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -2 });
    shell::act(Action::Confirm(None));
    assert_eq!(ui.get_screen(), 5);
    check(&mut f, "chain", &|ui| list(ui));
    shell::act(Action::Cancel(None));
    assert_eq!(ui.get_screen(), 4);
    // The sound chooser, once the library has answered.
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -3 });
    shell::act(Action::Confirm(None));
    assert_eq!(ui.get_screen(), 6);
    let r = loop {
        let r = request_in(&mut f, "library.query", &none, &graph);
        if r["method"] == "library.query" {
            break r;
        }
        f.reply(&r, json!({"result": {"midiInputs": [], "failures": []}})); // session.status, asked by the Part screen
    };
    let sounds: Vec<_> = (0..40).map(|i| json!({"id": format!("s{i}"), "name": format!("Sound {i}")})).collect();
    f.reply(&r, json!({"result": {"items": sounds, "total": 40}}));
    check(&mut f, "sounds", &|ui| list(ui));
    // The Parts list: its rows by the pitch of each theme.
    ui.global::<crate::Bar>().invoke_home();
    ui.global::<crate::HomeState>().invoke_tapped(0);
    assert_eq!(ui.get_screen(), 3);
    ui.global::<crate::Bar>().invoke_action(5);
    assert_eq!(ui.get_screen(), 9);
    f.screen.frame();
    let before = parts(&ui);
    theme::apply(&ui, &joined_theme());
    f.screen.frame();
    let tight = parts(&ui);
    theme::apply(&ui, &odd_theme());
    f.screen.frame();
    let roomy = parts(&ui);
    assert!(tight.0 > before.0 && before.0 > roomy.0, "the list holds more rows in a tight theme and fewer in a roomy one: {tight:?} {before:?} {roomy:?}");
    assert!(seen.iter().all(|s| s.2 != s.3), "{seen:?}");
}
