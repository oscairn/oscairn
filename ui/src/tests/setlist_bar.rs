// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Setlist's own bar, the one row under the top bar (the owner's layout, 2026-11-21): the setlists' dropdown, the
//! banks' dropdown, then Previous and Next, which show the bank before or after and open nothing, as on the reference
//! keyboard (a song opens when it is chosen). No sidebar of setlists and no footer: the bank's slots have all the room
//! under the bar. Stepping to the previous or next song stays where it was: the page keys and Play's buttons.

use super::*;
use crate::{MenuState, Theme};
use std::time::Duration;

/// A setlist of `banks` banks of `per` slots each, as `setlists.get` answers it.
fn doc(id: &str, name: &str, mode: &str, banks: usize, per: usize) -> serde_json::Value {
    let pages: Vec<_> = (0..banks)
        .map(|b| {
            let slots: Vec<_> = (0..per).map(|i| json!({"id": format!("p{}", b * per + i), "name": format!("Song {}", b * per + i), "color": if i % 5 == 2 { "#3366cc" } else { "" }, "label": ""})).collect();
            json!({"name": if b == 1 { String::new() } else { format!("Set {}", b + 1) }, "slots": slots})
        })
        .collect();
    json!({"format": "oscairn.setlist", "version": 2, "info": {"id": id, "name": name}, "display": {"mode": mode, "columns": 4, "rows": 3}, "pages": pages})
}

/// The Setlist shown on a fake engine with `sets` setlists, the first one read (`banks` banks of `per` slots), the
/// connection quiet. The engine's answers are `library.query`'s and `setlists.get`'s (docs/control-protocol.md).
fn on_setlist(name: &str, sets: usize, mode: &str, banks: usize, per: usize) -> Fixture {
    let mut f = Fixture::new(name);
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(2); // Setlist
    let setlists: Vec<_> = (0..sets).map(|i| json!({"id": format!("{name}-{}", i + 1), "name": format!("Night {}", i + 1)})).collect();
    let performances: Vec<_> = (0..40).map(|i| json!({"id": format!("p{i}"), "name": format!("Song {i}"), "color": "", "path": format!("/lib/p{i}")})).collect();
    f.serve(13, &json!({"setlists": setlists, "performances": performances, "setlist": doc(&format!("{name}-1"), "Night 1", mode, banks, per)}));
    quiet(&mut f);
    f
}

/// Whatever the shell still asks answered `{}` until it asks no more.
fn quiet(f: &mut Fixture) {
    while let Some(r) = f.engine.try_request(Duration::from_millis(60)) {
        f.reply(&r, json!({"result": {}}));
    }
}

fn menu(f: &Fixture) -> MenuState<'_> {
    f.screen.ui.global::<MenuState>()
}

fn state(f: &Fixture) -> SetlistState<'_> {
    f.screen.ui.global::<SetlistState>()
}

fn stops(f: &Fixture) -> Vec<crate::BarStop> {
    f.screen.ui.global::<SetlistState>().get_bar().iter().collect()
}

fn slots(f: &Fixture) -> Vec<crate::SlotData> {
    f.screen.ui.global::<SetlistState>().get_slots().iter().collect()
}

fn playing(f: &Fixture) -> Vec<i32> {
    slots(f).iter().filter(|s| s.playing).map(|s| s.number).collect()
}

fn asks_nothing(f: &mut Fixture, what: &str) {
    assert!(f.engine.try_request(Duration::from_millis(80)).is_none(), "{what} asked the engine for something");
}

// Previous and Next of the bar show the bank before or after: no request, and the song playing stays the one it was
// (opened here first, so there is one). At the first and last bank the button is off and a press does nothing: no wrap.
#[test]
fn bank_previous_and_next_turn_the_bank_and_open_nothing() {
    let mut f = on_setlist("setlist-bank-turn", 2, "grid", 3, 12);
    state(&f).invoke_tapped_slot(1); // a song chosen: the one playing
    let load = f.engine.request();
    assert_eq!((load["method"].as_str(), load["params"]["id"].as_str()), (Some("library.load"), Some("p1")));
    f.reply(&load, json!({"result": {}}));
    quiet(&mut f); // the next one prepared
    assert_eq!(playing(&f), [2]);
    let labels = |f: &Fixture| stops(f).iter().map(|s| (s.text.to_string(), s.enabled)).collect::<Vec<_>>();
    let s = |t: &str, on: bool| (t.to_string(), on);
    assert_eq!(labels(&f), [s("Night 1", true), s("Set 1  1/3", true), s("Previous", false), s("Next", true)]);
    // By the panel: the bar's cursor (pane 0) onto Next, a press.
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Move { pane: Some(0), by: 3 });
    assert!(stops(&f)[3].cursor && slots(&f)[0].number == 1, "moving the cursor turns nothing");
    shell::act(Action::Confirm(None));
    assert_eq!((slots(&f)[0].number, labels(&f)[1].clone(), playing(&f)), (13, s("Bank 2  2/3", true), vec![]), "the second bank, no slot of it playing");
    assert_eq!(f.screen.ui.get_screen(), 1);
    shell::act(Action::Confirm(None));
    assert_eq!((slots(&f)[0].number, labels(&f)[3].clone()), (25, s("Next", false)), "the last bank: Next is off");
    shell::act(Action::Confirm(None));
    assert_eq!(slots(&f)[0].number, 25, "no wrap");
    // By touch: Previous twice, and once more at the first bank.
    for want in [13, 1, 1] {
        state(&f).invoke_tapped_stop(2);
        assert_eq!(slots(&f)[0].number, want);
    }
    assert_eq!((labels(&f)[2].clone(), playing(&f)), (s("Previous", false), vec![2]), "back on the first bank the song playing is the one it was");
    asks_nothing(&mut f, "turning the banks");
    // The next song is still stepped to from where the song playing is, whatever bank shows (the page keys; Play's buttons call the same).
    state(&f).invoke_tapped_stop(3);
    shell::act(Action::Page(1));
    let next = f.engine.request();
    assert_eq!((next["method"].as_str(), next["params"]["id"].as_str()), (Some("library.load"), Some("p2")), "the song after the one playing");
    f.reply(&next, json!({"result": {}}));
    assert_eq!((slots(&f)[0].number, playing(&f)), (1, vec![3]), "its bank shows");
}

// A setlist chosen from the dropdown with panel moves only: the bar's cursor on the dropdown, a press opens its list
// (the shared menu, a border cursor, the one open ticked), the dial moves, nothing is read until the press; Cancel
// closes it with nothing changed.
#[test]
fn a_setlist_is_chosen_from_the_dropdown_by_panel_moves() {
    let mut f = on_setlist("setlist-drop-sets", 12, "list", 1, 5);
    f.navigation(json!({"move:0": 0, "move": 0, "confirm": 0, "cancel": 0}));
    f.navigation(json!({"move:0": 1, "move": 0, "confirm": 0, "cancel": 0})); // E1: the cursor shows on the dropdown
    assert!(stops(&f)[0].cursor && !menu(&f).get_open(), "the cursor opens nothing");
    f.navigation(json!({"move:0": 1, "move": 0, "confirm": 1, "cancel": 0})); // ENTER
    f.screen.frame(); // the list is drawn: it holds the rows that fit
    let rows: Vec<_> = menu(&f).get_rows().iter().collect();
    assert!(menu(&f).get_open() && menu(&f).get_title() == "Setlist");
    assert_eq!((rows[0].label.as_str(), rows[0].tick, rows[0].cursor, rows[1].tick), ("Night 1", 1, true, 0), "the one open is ticked, the cursor on the first");
    f.navigation(json!({"move:0": 1, "move": 2, "confirm": 1, "cancel": 0})); // the dial, two on
    assert!(menu(&f).get_rows().row_data(2).unwrap().cursor);
    f.navigation(json!({"move:0": 1, "move": 2, "confirm": 1, "cancel": 1})); // EXIT: closed, the Setlist still shown
    assert!(!menu(&f).get_open() && f.screen.ui.get_screen() == 1 && stops(&f)[0].text == "Night 1");
    asks_nothing(&mut f, "a dropdown opened, walked and closed");
    f.navigation(json!({"move:0": 1, "move": 2, "confirm": 2, "cancel": 1})); // opened again
    f.navigation(json!({"move:0": 1, "move": 4, "confirm": 2, "cancel": 1}));
    asks_nothing(&mut f, "moving in the dropdown");
    f.navigation(json!({"move:0": 1, "move": 4, "confirm": 3, "cancel": 1})); // the press
    let get = f.engine.request();
    assert_eq!((get["method"].as_str(), get["params"]["id"].as_str()), (Some("setlists.get"), Some("setlist-drop-sets-3")));
    f.reply(&get, json!({"result": {"setlist": doc("setlist-drop-sets-3", "Night 3", "list", 2, 3)}}));
    let texts: Vec<_> = stops(&f).iter().map(|s| s.text.to_string()).collect();
    assert_eq!((texts[0].as_str(), texts[1].as_str(), menu(&f).get_open(), slots(&f).len()), ("Night 3", "Set 1  1/2", false, 3));
    // The setlists past one view of the list are reached by the dial too: the list turns a view.
    shell::act(Action::Confirm(None));
    shell::act(Action::Move { pane: None, by: 11 });
    shell::act(Action::Confirm(None));
    assert_eq!(f.engine.request()["params"]["id"], "setlist-drop-sets-12", "the last of twelve");
}

// A bank chosen from its dropdown with panel moves only: no request, the song playing untouched. With one bank (the
// test after) the dropdown, Previous and Next are shown off and a press does nothing; with one setlist its dropdown
// lists the one.
#[test]
fn a_bank_is_chosen_from_the_dropdown_by_panel_moves() {
    let mut f = on_setlist("setlist-drop-banks", 2, "list", 3, 4);
    let before = playing(&f);
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Move { pane: Some(0), by: 1 }); // the banks' dropdown
    assert!(stops(&f)[1].cursor && !menu(&f).get_open());
    shell::act(Action::Confirm(None));
    f.screen.frame();
    let rows: Vec<_> = menu(&f).get_rows().iter().map(|r| (r.label.to_string(), r.tick)).collect();
    assert_eq!(rows, [("Set 1".to_string(), 1), ("Bank 2".to_string(), 0), ("Set 3".to_string(), 0)], "a bank's name, or its number");
    shell::act(Action::Move { pane: None, by: 2 });
    assert_eq!(slots(&f)[0].number, 1, "nothing shows until the press");
    shell::act(Action::Cancel(None));
    assert_eq!((menu(&f).get_open(), slots(&f)[0].number, f.screen.ui.get_screen()), (false, 1, 1), "Cancel closes it unchanged");
    shell::act(Action::Confirm(None));
    shell::act(Action::Move { pane: None, by: 2 });
    shell::act(Action::Confirm(None));
    assert_eq!((slots(&f)[0].number, stops(&f)[1].text.as_str(), playing(&f) == before), (9, "Set 3  3/3", true));
    asks_nothing(&mut f, "choosing a bank");
}

#[test]
fn one_bank_and_one_setlist() {
    let mut f = on_setlist("setlist-one-bank", 1, "list", 1, 4);
    assert_eq!(stops(&f).iter().map(|s| s.enabled).collect::<Vec<_>>(), [true, false, false, false], "one bank: its dropdown, Previous and Next are off");
    for stop in 1..4 {
        f.screen.ui.global::<SetlistState>().invoke_tapped_stop(stop);
    }
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Confirm(None));
    assert!(!menu(&f).get_open() && slots(&f)[0].number == 1);
    shell::act(Action::Move { pane: Some(0), by: -1 });
    shell::act(Action::Confirm(None));
    assert_eq!(menu(&f).get_rows().row_count(), 1, "one setlist: the dropdown lists it");
    asks_nothing(&mut f, "one bank");
}

// The dial alone reaches everything: up out of the first bank's first slot is the bar's last stop, on along the bar,
// up out of its first stop is the top bar (Play, the last of the screen's own actions before the record ones), and
// down past the bar's last stop is the first slot again. One cursor shows at a time.
#[test]
fn the_dial_walks_from_the_slots_through_the_bar_to_the_top_bar() {
    let mut f = on_setlist("setlist-dial", 2, "list", 2, 4);
    let at = |f: &Fixture| (stops(f).iter().position(|s| s.cursor), slots(f).iter().position(|s| s.cursor), f.screen.ui.global::<Theme>().get_bar_cursor());
    shell::act(Action::Move { pane: None, by: 1 });
    assert_eq!(at(&f), (None, Some(0), false), "the first move shows the cursor on the first slot");
    shell::act(Action::Move { pane: None, by: -1 });
    assert_eq!(at(&f), (Some(3), None, false), "up out of the slots: the bar's last stop");
    shell::act(Action::Move { pane: None, by: 1 });
    assert_eq!(at(&f), (None, Some(0), false), "and down again");
    shell::act(Action::Arrow { dx: 0, dy: -1 });
    shell::act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(at(&f), (Some(2), None, false), "the arrows too");
    shell::act(Action::Move { pane: None, by: -2 });
    assert_eq!(at(&f), (Some(0), None, false));
    shell::act(Action::Move { pane: None, by: -1 });
    assert!(at(&f).2, "up out of the bar's first stop: the top bar's cursor");
    assert_eq!(f.screen.ui.get_screen(), 1, "nothing switched on the way");
    asks_nothing(&mut f, "the cursor's walk");
}

// No footer and no sidebar: the screen's own slint has neither's parts (a list of setlists, the footer's row, its
// Play and its song buttons), and the room is the slots': at the keyboard's size a list view holds more rows than the
// footer left it, each the whole width of the screen less the margins.
#[test]
fn the_setlist_has_no_footer_and_no_sidebar() {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("slint/setlist.slint")).unwrap();
    let code: String = text.lines().map(|l| l.split("//").next().unwrap_or_default()).collect::<Vec<_>>().join("\n");
    for gone in ["footer", "list-width", "list-rows", "setlists", "tapped-setlist", "\"Play\"", "play()", "step(", "can-next", "can-previous", "slots-x"] {
        assert!(!code.contains(gone), "setlist.slint still has {gone}");
    }
    // No literal size either: every length is the theme's.
    for (n, line) in code.lines().enumerate() {
        for word in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
            let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            assert!(digits.is_empty() || digits.parse::<f32>().is_err() || !["px", "ms", "s"].contains(&&word[digits.len()..]), "setlist.slint:{}: a literal size or time {word}", n + 1);
        }
    }
    let mut f = on_setlist("setlist-room", 2, "list", 1, 30);
    f.screen.set_scale(1.55);
    for _ in 0..3 {
        f.screen.frame();
    }
    let per_view = f.screen.ui.global::<SetlistState>().get_slots_per_view();
    assert!(per_view >= 6, "a list view at 826 x 465 holds {per_view} rows (5 with the footer)");
    // The first row is drawn from margin to margin: the surface color at both ends of its middle line, under the bar.
    f.screen.set_scale(1.0);
    for _ in 0..3 {
        f.screen.frame();
    }
    let t = f.screen.ui.global::<Theme>();
    let (margin, touch, gap) = (t.get_margin(), t.get_touch(), t.get_gap());
    let y = (48. + margin + touch + gap + touch / 2.) as usize;
    let px = |x: usize| f.screen.pixels[y * f.screen.w + x];
    let (left, right, outside) = (px(margin as usize + 30), px(f.screen.w - margin as usize - 30), px(1));
    assert_eq!((left.r, left.g, left.b), (right.r, right.g, right.b), "the row's fill at both ends");
    assert_ne!((left.r, left.g, left.b), (outside.r, outside.g, outside.b), "and it is the row's, not the background");
}

// Pictures (target/screens/setlist-*.png): the list and the grid with the dropdowns closed, and the banks' dropdown
// open, at 1280 x 720; the list again at 800 x 480.
#[test]
fn pictures_of_the_setlist() {
    for (name, mode) in [("setlist-list", "list"), ("setlist-grid", "grid")] {
        let mut f = on_setlist(name, 3, mode, 3, if mode == "grid" { 12 } else { 30 });
        f.screen.ui.global::<SetlistState>().invoke_tapped_slot(1);
        let load = f.engine.request();
        f.reply(&load, json!({"result": {}}));
        quiet(&mut f);
        shell::act(Action::Move { pane: Some(1), by: 1 });
        shell::act(Action::Move { pane: Some(1), by: 3 });
        f.screen.frame();
        f.screen.frame();
        png(&f.screen, name);
        if mode == "grid" {
            shell::act(Action::Move { pane: Some(0), by: 1 });
            shell::act(Action::Move { pane: Some(0), by: 1 });
            f.screen.frame();
            png(&f.screen, "setlist-bank-cursor");
            shell::act(Action::Confirm(None));
            shell::act(Action::Move { pane: None, by: 1 });
            f.screen.frame();
            f.screen.frame();
            png(&f.screen, "setlist-bank-open");
            shell::act(Action::Cancel(None));
        } else {
            f.screen.resize(800, 480);
            for _ in 0..3 {
                f.screen.frame();
            }
            png(&f.screen, "setlist-list-800");
        }
    }
}
