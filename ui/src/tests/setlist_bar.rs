// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Setlist's own bar, the one row under the top bar (the owner's layout, 2026-11-21): the setlists' dropdown, the
//! banks' dropdown, then Previous and Next, which show the bank before or after and open nothing, as on the reference
//! keyboard (a song opens when it is chosen). No sidebar of setlists and no footer: the bank's slots have all the room
//! under the bar. Stepping to the previous or next song stays where it was: the page keys and Play's buttons.

use super::*;
use crate::{MenuState, Theme};
use std::time::Duration;

/// The slots' colors, by their place in a bank: a light one, a blue, a dark one, a red and a green among the plain.
const COLORS: [&str; 12] = ["", "#ffe082", "#3366cc", "", "", "#1a237e", "#e5484d", "", "", "", "#2e7d32", ""];

/// A song's name: short ones, and a long one fourth.
pub(super) fn song(i: usize) -> String {
    if i == 3 { "Somebody's Watching Me (Extended)".into() } else { format!("Song {i}") }
}

/// A setlist of `banks` banks of `per` slots each (the ninth of a bank a placeholder; `grid4`: four rows of four), as `setlists.get` answers it.
pub(super) fn doc(id: &str, name: &str, mode: &str, banks: usize, per: usize) -> serde_json::Value {
    let pages: Vec<_> = (0..banks)
        .map(|b| {
            let slots: Vec<_> = (0..per).map(|i| json!({"id": if i == 8 { String::new() } else { format!("p{}", b * per + i) }, "name": song(b * per + i), "color": COLORS[i % 12], "label": ""})).collect();
            json!({"name": if b == 1 { String::new() } else { format!("Set {}", b + 1) }, "slots": slots})
        })
        .collect();
    json!({"format": "oscairn.setlist", "version": 2, "info": {"id": id, "name": name}, "display": {"mode": mode.trim_end_matches('4'), "columns": 4, "rows": if mode == "grid4" { 4 } else { 3 }}, "pages": pages})
}

/// The Setlist shown on a fake engine with `sets` setlists, the first one read (`banks` banks of `per` slots), the
/// connection quiet. The engine's answers are `library.query`'s and `setlists.get`'s (docs/control-protocol.md).
pub(super) fn on_setlist(name: &str, sets: usize, mode: &str, banks: usize, per: usize) -> Fixture {
    on_setlist_doc(name, sets, doc(&format!("{name}-1"), "Night 1", mode, banks, per))
}

/// The same with the setlist read given.
pub(super) fn on_setlist_doc(name: &str, sets: usize, setlist: serde_json::Value) -> Fixture {
    let mut f = Fixture::new(name);
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(2); // Setlist
    let setlists: Vec<_> = (0..sets).map(|i| json!({"id": format!("{name}-{}", i + 1), "name": format!("Night {}", i + 1)})).collect();
    let performances: Vec<_> = (0..40).map(|i| json!({"id": format!("p{i}"), "name": song(i), "color": "", "path": format!("/lib/p{i}")})).collect();
    f.serve(13, &json!({"setlists": setlists, "performances": performances, "setlist": setlist}));
    quiet(&mut f);
    f
}

/// Whatever the shell still asks answered `{}` until it asks no more.
pub(super) fn quiet(f: &mut Fixture) {
    while let Some(r) = f.engine.try_request(Duration::from_millis(60)) {
        f.reply(&r, json!({"result": {}}));
    }
}

pub(super) fn menu(f: &Fixture) -> MenuState<'_> {
    f.screen.ui.global::<MenuState>()
}

pub(super) fn state(f: &Fixture) -> SetlistState<'_> {
    f.screen.ui.global::<SetlistState>()
}

pub(super) fn stops(f: &Fixture) -> Vec<crate::BarStop> {
    f.screen.ui.global::<SetlistState>().get_bar().iter().collect()
}

pub(super) fn slots(f: &Fixture) -> Vec<crate::SlotData> {
    f.screen.ui.global::<SetlistState>().get_slots().iter().collect()
}

fn playing(f: &Fixture) -> Vec<i32> {
    slots(f).iter().filter(|s| s.playing).map(|s| s.number).collect()
}

pub(super) fn asks_nothing(f: &mut Fixture, what: &str) {
    assert!(f.engine.try_request(Duration::from_millis(80)).is_none(), "{what} asked the engine for something");
}

/// The lamp changes asked for since the last call (the fake engine sets them apart from other requests).
fn lamps_asked(f: &mut Fixture, what: &str) -> Vec<serde_json::Value> {
    asks_nothing(f, what);
    std::mem::take(&mut f.engine.lamps)
}

// DEC and INC's lamps follow the banks: lit where a bank lies that way. Only the change is asked for, and nothing
// when the shown bank turns to one with the same neighbors. (That the engine sends nothing for a name the panel map
// gives no lamp is core/control/tests/json_rpc.cpp's: it answers it in `unknown`.)
#[test]
fn dec_and_inc_lamps_follow_the_banks() {
    let mut f = on_setlist("setlist-lamps", 2, "grid", 3, 12);
    let first = std::mem::take(&mut f.engine.lamps);
    assert_eq!(first.last(), Some(&json!([{"control": "inc", "on": true}])), "the first bank: INC only (before the setlist is read, neither)");
    state(&f).invoke_tapped_stop(3); // the second bank: a bank on both sides
    assert_eq!(lamps_asked(&mut f, "turning the bank"), [json!([{"control": "dec", "on": true}])]);
    state(&f).invoke_tapped_stop(3); // the last: none after it
    assert_eq!(lamps_asked(&mut f, "turning the bank"), [json!([{"control": "inc", "on": false}])]);
    state(&f).invoke_tapped_stop(2);
    state(&f).invoke_tapped_stop(2); // the first again
    assert_eq!(lamps_asked(&mut f, "turning the bank"), [json!([{"control": "inc", "on": true}]), json!([{"control": "dec", "on": false}])]);
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
    // Opened again, the cursor starts on the bank shown (the ticked row), not the first; and the setlists' on the one open.
    shell::act(Action::Confirm(None));
    f.screen.frame();
    let at = |f: &Fixture| menu(f).get_rows().iter().map(|r| (r.cursor, r.tick == 1)).collect::<Vec<_>>();
    assert_eq!(at(&f), [(false, false), (false, false), (true, true)], "the banks' list opens on the bank shown");
    shell::act(Action::Cancel(None));
    shell::act(Action::Move { pane: Some(0), by: -1 });
    shell::act(Action::Confirm(None));
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let get = f.engine.request();
    assert_eq!(get["params"]["id"], "setlist-drop-banks-2");
    f.reply(&get, json!({"result": {"setlist": doc("setlist-drop-banks-2", "Night 2", "list", 1, 3)}}));
    shell::act(Action::Confirm(None));
    f.screen.frame();
    assert_eq!(at(&f), [(false, false), (true, true)], "the setlists' list opens on the one open");
}

fn rgb(c: slint::Color) -> (u8, u8, u8) {
    (c.red(), c.green(), c.blue())
}

fn pixel(f: &Fixture, x: f32, y: f32) -> (u8, u8, u8) {
    let p = f.screen.pixels[y as usize * f.screen.w + x as usize];
    (p.r, p.g, p.b)
}

/// The runs of pixels that are not the background's along a line from (x, y) by (dx, dy): each one's start and length.
fn runs(f: &Fixture, x: usize, y: usize, dx: usize, dy: usize) -> Vec<(usize, usize)> {
    let back = rgb(f.screen.ui.global::<Theme>().get_background());
    let (mut out, mut start) = (vec![], None);
    let n = if dx == 1 { f.screen.w - x } else { f.screen.h - y };
    for i in 0..=n {
        let on = i < n && pixel(f, (x + i * dx) as f32, (y + i * dy) as f32) != back;
        match (on, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s + if dx == 1 { x } else { y }, i - s));
                start = None;
            }
            _ => {}
        }
    }
    out
}

// The bar as drawn: a quarter taller than a touch target (the Library's row is one touch target), by the theme's own
// factor; Previous and Next the theme's small button each, Next's right edge at the right margin; the two dropdowns
// equal halves of everything left of them. At 1280 and at 800 across.
#[test]
fn the_bar_is_taller_and_its_buttons_are_at_the_right() {
    let mut f = on_setlist("setlist-bar-shape", 2, "grid", 2, 12);
    for (w, h) in [(1280, 720), (800, 480)] {
        f.screen.resize(w, h);
        for _ in 0..3 {
            f.screen.frame();
        }
        let t = f.screen.ui.global::<Theme>();
        let (margin, touch, tall, button) = (t.get_margin(), t.get_touch(), t.get_tall_row(), t.get_button_width());
        assert_eq!((t.get_tall_row_scale(), tall), (1.25, touch * 1.25));
        let down = runs(&f, margin as usize + 8, 48, 0, 1);
        assert_eq!(down[0], (48 + margin as usize, tall as usize), "{w}: the bar starts at the margin and is the tall row high (a touch target is {touch})");
        let across = runs(&f, 0, 48 + (margin + tall / 2.) as usize, 1, 0);
        assert_eq!(across.len(), 4, "{w}: two dropdowns and two buttons: {across:?}");
        assert_eq!(across[3].0 + across[3].1, w - margin as usize, "{w}: Next ends at the right margin");
        assert_eq!((across[2].1, across[3].1), (button as usize, button as usize), "{w}: Previous and Next are the theme's small button");
        assert_eq!(across[0].0, margin as usize, "{w}: the first dropdown starts at the left margin");
        assert!(across[0].1.abs_diff(across[1].1) <= 1 && across[0].1 > 2 * button as usize, "{w}: the dropdowns are equal and have the rest: {across:?}");
        assert_eq!(across[2].0 - (across[1].0 + across[1].1), margin as usize, "{w}: nothing between the dropdowns and the buttons but the gap");
    }
}

// A slot's color is its whole box, in the grid and in the list, in a dark theme and a light one: the box's pixels are
// the color itself, and the ink on it (black or white) is at least 4.5 to 1 against it, for a dark color and a light
// one; a slot without a color keeps the theme's surface. The slot playing and the cursor are told apart on a colored
// box without its background: the cursor is the focus border with a line of the box's ink inside it, the slot
// playing has its left edge in the box's ink (and its number on a chip of it); a slot can be both.
#[test]
fn a_slots_color_is_its_whole_box() {
    for mode in ["grid", "list"] {
        let mut f = on_setlist(&format!("setlist-colors-{mode}"), 2, mode, 1, 12);
        state(&f).invoke_tapped_slot(1); // the light one plays
        let load = f.engine.request();
        f.reply(&load, json!({"result": {}}));
        quiet(&mut f);
        shell::act(Action::Move { pane: Some(1), by: 1 });
        shell::act(Action::Move { pane: Some(1), by: 2 }); // the cursor on the blue one
        for theme in ["oscairn-dark", "oscairn-light"] {
            theme::apply(&f.screen.ui, &theme::chosen(&theme::Places::default(), theme, ""));
            for _ in 0..3 {
                f.screen.frame();
            }
            let t = f.screen.ui.global::<Theme>();
            let (margin, gap, tall, focus) = (t.get_margin(), t.get_gap(), t.get_tall_row(), rgb(t.get_focus()));
            let (across, down) = if mode == "grid" { (4., 3.) } else { (1., 1.) };
            let top = 48. + margin + tall + gap;
            let cw = (f.screen.w as f32 - 2. * margin - (across - 1.) * gap) / across;
            let ch = if mode == "grid" { (f.screen.h as f32 - top - margin - (down - 1.) * gap) / down } else { t.get_touch() + 2. * margin };
            // A slot's box: its left, top, width and height.
            let cell = |i: usize| (margin + (i % across as usize) as f32 * (cw + gap), top + (i / across as usize) as f32 * (ch + gap), cw, ch);
            let all = slots(&f);
            for (i, light) in [(1, Some(true)), (2, Some(false)), (5, Some(false)), (6, Some(false))] {
                let (s, (x, y, w, h)) = (&all[i], cell(i));
                let (tint, ink) = (rgb(s.tint), rgb(s.ink));
                assert!(s.tinted && light.is_none_or(|l| ink == if l { (0, 0, 0) } else { (255, 255, 255) }), "{mode} {theme} slot {i}: its ink {ink:?}");
                assert!(crate::palette::contrast(s.tint, s.ink) >= 3., "{mode} {theme} slot {i}: {}", crate::palette::contrast(s.tint, s.ink));
                // The box is the color: near each corner's inside, and under the name.
                for (px, py) in [(x + w * 0.75, y + h - 6.), (x + w - 8., y + 6.), (x + w * 0.5, y + h - 4.)] {
                    assert_eq!(pixel(&f, px, py), tint, "{mode} {theme} slot {i}: the box at {px}, {py}");
                }
                // And something is written on it in its ink.
                let written = (y as usize..(y + h) as usize).any(|py| (x as usize..(x + w) as usize).any(|px| pixel(&f, px as f32, py as f32) == ink));
                assert!(written, "{mode} {theme} slot {i}: its name in {ink:?}");
                // The slot playing (1): its left edge in its ink; the others: the color. The cursor (2): the focus border, the ink's line inside it.
                let (edge, border, inner) = (pixel(&f, x + 4., y + h / 2.), pixel(&f, x + w / 2., y + 1.), pixel(&f, x + w / 2., y + 2.));
                assert_eq!(edge == ink, s.playing, "{mode} {theme} slot {i}: the playing edge {edge:?}");
                assert_eq!((border == focus, inner == ink), (s.cursor, s.cursor), "{mode} {theme} slot {i}: the cursor {border:?} {inner:?}");
                assert_eq!((s.playing, s.cursor), (i == 1, i == 2));
            }
            // No color: the theme's surface, as before.
            assert!(!all[0].tinted && pixel(&f, cell(0).0 + cell(0).2 * 0.75, cell(0).1 + cell(0).3 - 6.) == rgb(t.get_surface()), "{mode} {theme}: a slot without a color");
        }
    }
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
    for gone in ["property <length> footer", "list-width", "list-rows", "setlists", "tapped-setlist", "\"Play\"", "play()", "step(", "can-next", "can-previous", "slots-x", "message", "RowMark"] {
        assert!(!code.contains(gone), "setlist.slint still has {gone}");
    }
    // No literal size either: every length is the theme's.
    for (n, line) in code.lines().enumerate() {
        for word in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
            let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            assert!(digits.is_empty() || digits == "0" || digits.parse::<f32>().is_err() || !["px", "ms", "s"].contains(&&word[digits.len()..]), "setlist.slint:{}: a literal size or time {word}", n + 1);
        }
    }
    let mut f = on_setlist("setlist-room", 2, "list", 1, 30);
    // The first row is drawn from margin to margin: the surface color at both ends of its middle line, under the bar.
    f.screen.set_scale(1.0);
    for _ in 0..3 {
        f.screen.frame();
    }
    let t = f.screen.ui.global::<Theme>();
    let (margin, touch, gap) = (t.get_margin(), t.get_touch(), t.get_gap());
    let y = (48. + margin + t.get_tall_row() + gap + touch / 2.) as usize;
    let px = |x: usize| f.screen.pixels[y * f.screen.w + x];
    let (left, right, outside) = (px(margin as usize + 30), px(f.screen.w - margin as usize - 30), px(1));
    assert_eq!((left.r, left.g, left.b), (right.r, right.g, right.b), "the row's fill at both ends");
    assert_ne!((left.r, left.g, left.b), (outside.r, outside.g, outside.b), "and it is the row's, not the background");
}

// Pictures (target/screens/setlist-*.png): the list and the grid with the dropdowns closed, and the banks' dropdown
// open, at 1280 x 720 at scale 1 and at the keyboard's 1.55 (NAME-155); the list again at 800 x 480. The second slot
// (a light color) plays and the cursor is on the third (blue).
#[test]
fn pictures_of_the_setlist() {
    for (scale, tag) in [(1.0, ""), (1.55, "-155")] {
        for mode in ["list", "grid", "grid4"] {
            let mut f = on_setlist(&format!("setlist-pic-{mode}{tag}"), 3, mode, 3, if mode == "grid" { 12 } else if mode == "grid4" { 16 } else { 30 });
            f.screen.set_scale(scale);
            let draw = |f: &mut Fixture, name: &str| {
                for _ in 0..3 {
                    f.screen.frame();
                }
                png(&f.screen, &format!("{name}{tag}"));
            };
            f.screen.ui.global::<SetlistState>().invoke_tapped_slot(1);
            let load = f.engine.request();
            f.reply(&load, json!({"result": {}}));
            quiet(&mut f);
            shell::act(Action::Move { pane: Some(1), by: 1 });
            shell::act(Action::Move { pane: Some(1), by: 2 });
            draw(&mut f, &format!("setlist-{mode}"));
            if mode == "grid" {
                shell::act(Action::Move { pane: Some(0), by: 1 });
                shell::act(Action::Move { pane: Some(0), by: 1 });
                shell::act(Action::Confirm(None));
                draw(&mut f, "setlist-bank-open");
                shell::act(Action::Cancel(None));
            } else if mode == "list" && tag.is_empty() {
                f.screen.resize(800, 480);
                draw(&mut f, "setlist-list-800");
            }
        }
    }
}

/// Slot `i`'s box in the window's pixels at `scale`: a tile of a grid of four across and `rows` down, or a list row (`rows` 0).
fn slot_box(f: &Fixture, scale: f32, rows: usize, i: usize) -> (f32, f32, f32, f32) {
    let t = f.screen.ui.global::<Theme>();
    let (margin, gap) = (t.get_margin(), t.get_gap());
    let top = 48. + margin + t.get_tall_row() + gap;
    let (across, cw, ch) = if rows == 0 {
        (1, f.screen.w as f32 / scale - 2. * margin, t.get_touch() + 2. * margin)
    } else {
        (4, (f.screen.w as f32 / scale - 2. * margin - 3. * gap) / 4., (f.screen.h as f32 / scale - top - margin - (rows - 1) as f32 * gap) / rows as f32)
    };
    ((margin + (i % across) as f32 * (cw + gap)) * scale, (top + (i / across) as f32 * (ch + gap)) * scale, cw * scale, ch * scale)
}

/// The bands of rows (`down`) or columns of the rectangle that have something drawn on its fill (the color at its
/// lower right inside corner): each one's start within it and its length.
fn bands(f: &Fixture, (x, y, w, h): (f32, f32, f32, f32), down: bool) -> Vec<(usize, usize)> {
    let fill = pixel(f, x + w - 1., y + h - 1.);
    let (n, m) = if down { (h as usize, w as usize) } else { (w as usize, h as usize) };
    let (mut out, mut start) = (vec![], None);
    for a in 0..=n {
        let on = a < n && (0..m).any(|b| if down { pixel(f, x + b as f32, y + a as f32) } else { pixel(f, x + a as f32, y + b as f32) } != fill);
        match (on, start) {
            (true, None) => start = Some(a),
            (false, Some(s)) => {
                out.push((s, a - s));
                start = None;
            }
            _ => {}
        }
    }
    out
}

// A stage screen read from playing distance: at the keyboard's scale (1.55) a tile's name is drawn in the theme's
// display size (32) and a list row's in its headline size (26), where they were the title (20) and the body (16). The
// measure is the drawn text: from the top of "S" to the foot of "g" in "Song 0" (about nine tenths of the size), which
// is at least 0.85 of the new size and at least 1.4 times the whole of the old one. A long name is two lines in a
// 4 x 3 tile and never three (cut), and two in a 4 x 4 tile as well; the number's band, the name's lines and the box's edge do
// not meet (they would be one band); a list row's number ends clear of where its name starts. And the rows a list
// view holds: 9 at scale 1, 5 at 1.55 (a bank of 30, with the line under them that says which show).
// A 4 x 4 tile is no taller than two lines of the smallest name size, so the old rule ("fit in what the tile's floor
// says is the lines it has") drew a long name on one line, cut. The rule is "two lines are allowed": a size is a step
// smaller only when the name does not fit on two lines there, and the drawn text shows two wrapped lines, cut after.
// After the fix the long name is two lines in the 4 x 4 tile at the keyboard's scale too, and more of it shows than
// the single cut line did (the ink below the number's chip grows: before 1908 px, after two lines); a short name keeps
// the display size and one line, and no name's lines reach the number's chip (the per-tile loop of the test below).
#[test]
fn a_long_name_is_two_lines_in_a_4x4_tile() {
    let scale = 1.55;
    let inset = |(x, y, w, h): (f32, f32, f32, f32)| (x + 8., y + 5., w - 16., h - 10.);
    for (mode, rows, long_lines) in [("grid", 3, 2), ("grid4", 4, 2)] {
        let mut f = on_setlist(&format!("setlist-long-{mode}"), 2, mode, 1, 4 * rows);
        f.screen.set_scale(scale);
        for _ in 0..3 {
            f.screen.frame();
        }
        f.screen.save(&format!("h3-long-{mode}"));
        // The long slot (fourth) is drawn on `long_lines` lines, never more (cut after); the short one is still one.
        let long = bands(&f, inset(slot_box(&f, scale, rows, 3)), true);
        assert_eq!(long.len(), 1 + long_lines, "{mode}: the long name on {long_lines} lines under the number, cut after: {long:?}");
        assert_eq!(bands(&f, inset(slot_box(&f, scale, rows, 0)), true).len(), 2, "{mode}: the short name one line, unchanged");
        // More of the name shows than the single cut line did: the ink below the number's chip (the drawn characters).
        // Before the fix the 4 x 4 tile's one cut line drew 1908 px; two lines draw roughly twice that.
        if mode == "grid4" {
            let (r0, r1, r2, r3) = inset(slot_box(&f, scale, rows, 3));
            let fill = pixel(&f, r0 + r2 - 1., r1 + r3 - 1.);
            let back = rgb(f.screen.ui.global::<Theme>().get_background());
            let top = 5 + (long[0].0 + long[0].1);
            let ink = (0..r2 as usize).map(|lx| (0..r3 as usize).filter(|&ly| {
                let (px, py) = (r0 + lx as f32, r1 + ly as f32);
                py >= r1 + top as f32 && { let p = pixel(&f, px, py); p != fill && p != back }
            }).count()).sum::<usize>();
            assert!(ink > 2400, "grid4: the characters shown ({ink} px below the chip) rose from the one cut line (1908 px before); now two lines");
        }
    }
}

#[test]
fn names_are_large_and_nothing_overlaps() {
    let scale = 1.55;
    let inset = |(x, y, w, h): (f32, f32, f32, f32)| (x + 8., y + 5., w - 16., h - 10.);
    for (mode, rows, long_lines) in [("grid", 3, 2), ("grid4", 4, 2)] {
        let mut f = on_setlist(&format!("setlist-large-{mode}"), 2, mode, 1, 4 * rows);
        f.screen.set_scale(scale);
        for _ in 0..3 {
            f.screen.frame();
        }
        let display = f.screen.ui.global::<Theme>().get_display() * scale;
        let short = bands(&f, inset(slot_box(&f, scale, rows, 0)), true);
        assert_eq!(short.len(), 2, "{mode}: the number's band, then the name's one line, apart: {short:?}");
        let drawn = short[1].1 as f32;
        assert!(drawn >= 0.85 * display && drawn >= 1.4 * 20. * scale, "{mode}: \"Song 0\" is drawn {drawn} px high (the display size is {display} px, the old title {} px)", 20. * scale);
        assert!(short[1].1 > 2 * short[0].1, "{mode}: the number is clearly smaller than the name: {short:?}");
        let long = bands(&f, inset(slot_box(&f, scale, rows, 3)), true);
        assert_eq!(long.len(), 1 + long_lines, "{mode}: the long name on {long_lines} line(s) under the number, cut after: {long:?}");
        // Every tile of the view: what is drawn in it starts under the number's band and ends inside the box.
        for i in 0..4 * rows {
            let b = bands(&f, inset(slot_box(&f, scale, rows, i)), true);
            assert!(b.len() >= 2 && b.len() <= 4 && b[1].0 > b[0].0 + b[0].1, "{mode} tile {i}: {b:?}");
        }
    }
    let mut f = on_setlist("setlist-large-list", 2, "list", 1, 30);
    for (scale, per_view) in [(1.0, 9), (1.55, 5)] {
        f.screen.set_scale(scale);
        for _ in 0..3 {
            f.screen.frame();
        }
        assert_eq!(state(&f).get_slots_per_view(), per_view, "the rows a list view holds at {scale}");
        let t = f.screen.ui.global::<Theme>();
        let (headline, column) = (t.get_headline() * scale, (t.get_touch() + 2. * t.get_pad()) * scale);
        for i in [0, 3] {
            let (x, y, w, h) = slot_box(&f, scale, 0, i);
            let name = bands(&f, (x + column, y + 4., w - column - 8., h - 8.), true);
            assert_eq!(name.len(), 1, "{scale} row {i}: one line: {name:?}");
            let drawn = name[0].1 as f32;
            assert!(drawn >= 0.85 * headline && drawn >= 1.4 * 16. * scale, "{scale} row {i}: its name is drawn {drawn} px high (the headline size is {headline} px, the old body {} px)", 16. * scale);
            let number = bands(&f, (x + 8., y + 4., column - 8., h - 8.), false);
            let end = number.last().map(|n| n.0 + n.1).unwrap();
            assert!((end as f32) < column - 8. - 8. * scale, "{scale} row {i}: the number ends clear of the name: {number:?}");
            let high = bands(&f, (x + 8., y + 4., column - 8., h - 8.), true);
            assert!(high.len() == 1 && 3 * high[0].1 < 2 * name[0].1, "{scale} row {i}: the number is smaller than the name: {high:?} {name:?}");
        }
    }
}

// The ink on a slot's color: white wherever white is 3 to 1 against it (large text), black only on a light color.
#[test]
fn white_on_a_color_unless_it_is_light() {
    let (white, black) = (slint::Color::from_rgb_u8(255, 255, 255), slint::Color::from_rgb_u8(0, 0, 0));
    for (hex, want) in [("#e5484d", white), ("#3366cc", white), ("#1a237e", white), ("#2e7d32", white), ("#ffe082", black), ("#b2ebf2", black)] {
        let c = crate::setlist::color(hex).unwrap();
        let ink = crate::theme::white_first(c);
        assert_eq!(rgb(ink), rgb(want), "{hex}");
        assert!(crate::palette::contrast(c, ink) >= 3., "{hex}: {}", crate::palette::contrast(c, ink));
    }
}

/// A bank of four slots: an uncolored playable, a red playable, a red placeholder and a light placeholder, so a
/// placeholder's box is told apart from the same color's playable slot and from an uncolored slot, in the grid and
/// the list, with the cursor still shown on it and its name reading on the box (3 to 1).
fn on_placeholders(name: &str, mode: &str) -> Fixture {
    let mut f = Fixture::new(name);
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(2); // Setlist
    let setlists = json!([{"id": format!("{name}-1"), "name": "Night 1"}]);
    let performances = json!([{"id": "p0", "name": "Song", "color": "", "path": "/lib/p0"}, {"id": "p1", "name": "Red", "color": "", "path": "/lib/p1"}]);
    let doc = json!({"format": "oscairn.setlist", "version": 2, "info": {"id": format!("{name}-1"), "name": "Night 1"},
        "display": {"mode": mode, "columns": 4, "rows": 3},
        "pages": [{"name": "Set 1", "slots": [
            {"id": "p0", "name": "Song", "color": "", "label": ""},
            {"id": "p1", "name": "Red", "color": "#e5484d", "label": ""},
            {"id": "", "name": "Ghost", "color": "#e5484d", "label": ""},
            {"id": "", "name": "Phantom", "color": "#ffe082", "label": ""}]}]});
    f.serve(13, &json!({"setlists": setlists, "performances": performances, "setlist": doc}));
    quiet(&mut f);
    f
}

#[test]
fn a_placeholder_is_a_tint_not_a_solid_and_reads_on_it() {
    for mode in ["grid", "list"] {
        let mut f = on_placeholders(&format!("setlist-ph-{mode}"), mode);
        for scale in [1.0, 1.55] {
            f.screen.set_scale(scale);
            for _ in 0..3 {
                f.screen.frame();
            }
            let t = f.screen.ui.global::<Theme>();
            let rows = if mode == "grid" { 3 } else { 0 };
            let fill = |i: usize| pixel(&f, slot_box(&f, scale, rows, i).0 + slot_box(&f, scale, rows, i).2 * 0.75, slot_box(&f, scale, rows, i).1 + slot_box(&f, scale, rows, i).3 - 6.);
            let all = slots(&f);
            assert_eq!(all[2].note, "(placeholder)", "{mode} {scale}: slot 2 a placeholder");
            assert_eq!(all[3].note, "(placeholder)", "{mode} {scale}: slot 3 a placeholder");
            assert!(!all[2].empty && !all[3].empty && all[2].tinted && all[3].tinted, "{mode} {scale}: the placeholders keep their colors");
            // A placeholder's box: the tint of its color, not the solid, and not an uncolored slot's surface.
            let (red_ph, red_play, plain) = (fill(2), fill(1), fill(0));
            assert_ne!(red_ph, red_play, "{mode} {scale}: the placeholder is a tint, not the solid {red_ph:?}");
            assert_ne!(red_ph, plain, "{mode} {scale}: the placeholder is not an uncolored slot's {plain:?}");
            assert_ne!(red_ph, rgb(t.get_background()), "{mode} {scale}: and it is a slot, not the background");
            // Its name reads on the box: the ink (the theme's secondary where that reads, else white or black) gives at
            // least 3 to 1 on the box, and is drawn there.
            for i in [2, 3] {
                let (x, y, w, h) = slot_box(&f, scale, rows, i);
                let bg = fill(i);
                let ink = rgb(slots(&f)[i].ink);
                let c = slint::Color::from_rgb_u8(ink.0, ink.1, ink.2);
                let bgc = slint::Color::from_rgb_u8(bg.0, bg.1, bg.2);
                assert!(crate::palette::contrast(bgc, c) >= 3., "{mode} {scale} placeholder {i}: {}", crate::palette::contrast(bgc, c));
                let written = (y as usize..(y + h) as usize).any(|py| (x as usize..(x + w) as usize).any(|px| pixel(&f, px as f32, py as f32) == ink));
                assert!(written, "{mode} {scale} placeholder {i}: its name drawn in {ink:?}");
            }
            // The cursor border still shows on a placeholder, with the ink's line inside it on a colored box.
            let focus = rgb(t.get_focus());
            shell::act(Action::Move { pane: Some(1), by: 1 }); // the first move shows it on the first slot
            shell::act(Action::Move { pane: Some(1), by: 2 });
            f.screen.frame(); // the move is drawn
            let all = slots(&f);
            let j = all.iter().position(|s| s.cursor).expect("the cursor shows");
            assert_eq!(all[j].note, "(placeholder)", "{mode} {scale}: the cursor is on a placeholder (row {j})");
            let ink = rgb(all[j].ink);
            let (x, y, w, _) = slot_box(&f, scale, rows, j);
            let top = (y as usize)..(y as usize + 6);
            let xs = (x as usize)..(x as usize + w as usize);
            let at = |color: (u8, u8, u8)| top.clone().any(|py| xs.clone().any(|px| pixel(&f, px as f32, py as f32) == color));
            assert!(at(focus), "{mode} {scale}: the cursor's focus border on the placeholder");
            assert!(at(ink), "{mode} {scale}: its ink's line inside it");
        }
    }
}

/// Three banks of twelve with Performance p1 in three slots: the second and the sixth of the first bank and the second
/// of the second (slots 2, 6 and 14).
fn on_twice(name: &str) -> Fixture {
    let mut d = doc(&format!("{name}-1"), "Night 1", "grid", 3, 12);
    d["pages"][0]["slots"][5]["id"] = json!("p1");
    d["pages"][1]["slots"][1]["id"] = json!("p1");
    on_setlist_doc(name, 2, d)
}

/// The engine reports its open Performance as it does after any change (a Performance opened from the Library or
/// Home, a program change, an edit): `graph.changed`, and `performance.info` answered with `info`.
fn report(f: &mut Fixture, info: serde_json::Value) {
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "graph.changed", "params": {}})));
    f.pump();
    while let Some(r) = f.engine.try_request(Duration::from_millis(60)) {
        assert_ne!(r["method"], "library.load", "a report opened something");
        let result = if r["method"] == "performance.info" { info.clone() } else { json!({}) };
        f.reply(&r, json!({"result": result}));
    }
}

fn open(id: &str) -> serde_json::Value {
    json!({"id": id, "name": id, "unsaved": false})
}

// The slots playing follow the engine's open Performance, whoever opened it: every slot holding it is marked (a song
// twice in the setlist, twice), none when it is not in the setlist; an unsaved edit keeps it marked. Nothing is
// opened by it. The next song steps from the first slot marked, or from the start with none.
#[test]
fn the_marks_follow_the_engines_open_performance() {
    let mut f = on_twice("setlist-follow-marks");
    assert_eq!(playing(&f), Vec::<i32>::new(), "the engine reported no Performance of the setlist");
    report(&mut f, open("p1"));
    assert_eq!(playing(&f), [2, 6], "both slots holding it in the bank shown");
    state(&f).invoke_tapped_stop(3);
    assert_eq!(playing(&f), [14], "and the one in the next bank");
    report(&mut f, json!({"id": "p1", "name": "p1", "unsaved": true}));
    assert_eq!(playing(&f), [14], "an edit is still that Performance");
    report(&mut f, open("p17"));
    assert_eq!(playing(&f), [18], "changed outside the setlist: the marks move");
    state(&f).invoke_tapped_stop(2);
    assert_eq!(playing(&f), Vec::<i32>::new());
    report(&mut f, open("elsewhere"));
    state(&f).invoke_tapped_stop(3);
    assert_eq!(playing(&f), Vec::<i32>::new(), "not in the setlist: none marked");
    report(&mut f, open(""));
    assert_eq!(playing(&f), Vec::<i32>::new(), "none from the library: none marked");
    // Stepping: from the start with none marked, from the first marked otherwise.
    shell::act(Action::Page(1));
    let load = f.engine.request();
    assert_eq!(load["params"]["id"], "p0", "none marked: the first song");
    f.reply(&load, json!({"result": {}}));
    quiet(&mut f);
    report(&mut f, open("p1"));
    assert_eq!(playing(&f), [2, 6]);
    shell::act(Action::Page(1));
    let load = f.engine.request();
    assert_eq!(load["params"]["id"], "p2", "the song after the first slot marked");
}

// A Performance opened by a slot here marks every slot holding it at once, and a report of the same one keeps the
// slot chosen (the second time a song is in the set, the next song is the one after it).
#[test]
fn a_slot_chosen_here_stays_the_one_stepped_from() {
    let mut f = on_twice("setlist-follow-chosen");
    state(&f).invoke_tapped_slot(5); // slot 6, p1
    let load = f.engine.request();
    assert_eq!(load["params"]["id"], "p1");
    f.reply(&load, json!({"result": {}}));
    quiet(&mut f);
    assert_eq!(playing(&f), [2, 6]);
    report(&mut f, open("p1"));
    shell::act(Action::Page(1));
    assert_eq!(f.engine.request()["params"]["id"], "p6", "the song after the slot chosen, not after the first p1");
}

// The screen opens on the bank holding the Performance open (its first slot); with none of the setlist open, on the
// bank last shown.
#[test]
fn the_screen_opens_on_the_bank_playing() {
    let mut f = on_twice("setlist-follow-bank");
    let away_and_back = |f: &mut Fixture, info: serde_json::Value| {
        shell::act(Action::Cancel(None)); // Home
        assert_eq!(f.screen.ui.get_screen(), 0);
        report(f, info);
        f.screen.ui.global::<HomeState>().invoke_tapped(2);
        quiet(f);
        assert_eq!(f.screen.ui.get_screen(), 1);
    };
    away_and_back(&mut f, open("p30"));
    assert_eq!((slots(&f)[0].number, playing(&f)), (25, vec![31]), "the third bank");
    away_and_back(&mut f, open("p1"));
    assert_eq!((slots(&f)[0].number, playing(&f)), (1, vec![2, 6]), "the first holding it");
    state(&f).invoke_tapped_stop(3);
    away_and_back(&mut f, open("elsewhere"));
    assert_eq!((slots(&f)[0].number, playing(&f)), (13, vec![]), "none playing: the bank last shown");
}

// DEC and INC (the engine's `step` total) turn the bank on the Setlist as Previous and Next do: no request, no wrap, the
// slot cursor untouched otherwise; with the banks' dropdown open, and on Home, they move the cursor as they always did.
#[test]
fn dec_and_inc_turn_the_bank_and_move_elsewhere() {
    let mut f = on_twice("setlist-dec-inc");
    let n = |m: i64, s: i64| json!({"move": m, "step": s, "confirm": 0, "cancel": 0});
    f.navigation(n(0, 0)); // the baseline
    for (step, first) in [(1, 13), (2, 25), (3, 25), (2, 13), (-1, 1), (-3, 1)] {
        f.navigation(n(0, step));
        assert_eq!(slots(&f)[0].number, first, "step total {step}");
    }
    assert!(slots(&f).iter().all(|s| !s.cursor) && stops(&f).iter().all(|s| !s.cursor), "no cursor shown by a step");
    asks_nothing(&mut f, "DEC and INC");
    // The banks' dropdown open: a step moves its cursor, the bank shown stays.
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Confirm(None));
    f.screen.frame();
    f.navigation(n(0, -2));
    let at = menu(&f).get_rows().iter().position(|r| r.cursor);
    assert_eq!((menu(&f).get_open(), at, slots(&f)[0].number), (true, Some(1), 1), "INC in the dropdown: one down");
    shell::act(Action::Cancel(None));
    // Home: a step is the move it was.
    shell::act(Action::Cancel(None));
    assert_eq!(f.screen.ui.get_screen(), 0);
    let cursor = |f: &Fixture| f.screen.ui.global::<HomeState>().get_tiles().iter().position(|t| t.cursor);
    f.navigation(n(0, -1));
    f.navigation(n(0, 0));
    assert_eq!(cursor(&f), Some(1), "on Home DEC and INC move the cursor");
    asks_nothing(&mut f, "DEC and INC on Home");
}

// The picture the owner asked for: one Performance in two slots of a bank, both marked, at 1280 x 720 at the
// keyboard's scale (target/screens/setlist-follow.png).
#[test]
fn picture_of_a_song_twice() {
    let mut f = on_twice("setlist-follow-pic");
    f.screen.set_scale(1.55);
    report(&mut f, open("p1"));
    for _ in 0..3 {
        f.screen.frame();
    }
    png(&f.screen, "setlist-follow");
}

