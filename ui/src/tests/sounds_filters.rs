// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sound chooser's list as the library pane has it (sounds.rs, LibraryPane.cpp in Pick mode): the query
//! its filters make (search, Favorites, Recent, the library, tags), the tags and libraries listed, each
//! Sound's row (detail, lock, fit, star, color), the favorite action, by the cursor and by touch, drawn in the
//! themes at the sizes of the keyboard's screen and the smaller and larger ones.

use super::chain::marks_graph;
use super::*;
use crate::client::Error;
use crate::sounds::{Outcome, Request, Screen as Sounds};

fn rows(ui: &Shell) -> Vec<crate::SettingData> {
    ui.global::<crate::PartState>().get_rows().iter().collect()
}

fn labels(ui: &Shell) -> Vec<String> {
    rows(ui).into_iter().map(|r| r.label.to_string()).collect()
}

fn value_of(ui: &Shell, label: &str) -> String {
    rows(ui).into_iter().find(|r| r.label == label).unwrap_or_else(|| panic!("{label} in {:?}", labels(ui))).value.to_string()
}

fn sent(out: &mut Vec<crate::sounds::Out>) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(out).into_iter().map(|(_, m, p)| (m, p)).collect()
}

fn open(s: &super::Screen) -> Sounds {
    tall(&s.ui);
    let mut c = Sounds::new(&s.ui);
    c.graph(&marks_graph());
    c.open(10);
    c.shown();
    c
}

fn press_at(c: &mut Sounds, ui: &Shell, target: usize) -> Outcome {
    let now = match rows(ui).iter().position(|x| x.cursor) {
        Some(at) => at as i32,
        None => {
            c.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor
            0
        }
    };
    if target as i32 != now {
        c.act(Action::Move { pane: None, by: target as i32 - now });
    }
    c.act(Action::Confirm(None))
}

fn press_row(c: &mut Sounds, ui: &Shell, label: &str) -> Outcome {
    let at = labels(ui).iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {:?}", labels(ui)));
    press_at(c, ui, at)
}

fn sounds() -> serde_json::Value {
    json!({"items": [
        {"id": "s1", "name": "Warm Rhodes", "tags": ["keys", "warm"], "description": "Soft", "favorite": true, "color": "#3366cc"},
        {"id": "s2", "name": "", "tags": [], "library": "factory"},
        {"id": "s3", "name": "Huge Pad", "tags": ["pad"], "unfit": "needs 3 GB"},
        {"id": "s4", "name": "Fat Bass", "tags": ["bass", "keys"]},
    ], "total": 4, "tags": [{"tag": "keys", "count": 2}, {"tag": "bass", "count": 1}, {"tag": "pad", "count": 1}, {"tag": "warm", "count": 1}]})
}

// The row of a Sound says its name ("Untitled" without one), its tags and description, "Locked" for a library's,
// "Does not fit this device" when the engine says so, a star when it is a favorite, "Auditioning" while it is
// tried; its color goes where the theme puts it.
#[test]
fn a_sounds_row_is_the_library_panes() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s);
    c.reply(Request::Query, Ok(sounds()));
    assert_eq!(labels(ui), ["Search", "Filters", "Warm Rhodes", "Untitled", "Huge Pad", "Fat Bass"]);
    assert_eq!(value_of(ui, "Warm Rhodes"), "keys  ·  warm   Soft  ★");
    assert_eq!(value_of(ui, "Untitled"), "Locked");
    assert_eq!(value_of(ui, "Huge Pad"), "Does not fit this device  ·  pad");
    assert_eq!(value_of(ui, "Fat Bass"), "bass  ·  keys");
    let r = rows(ui);
    assert_eq!(r.iter().map(|r| r.tinted).collect::<Vec<_>>(), [false, false, true, false, false, false], "only the Sound with a color is tinted");
    // Trying a Sound: it says so first; stopping takes it back.
    press_row(&mut c, ui, "Fat Bass");
    press_row(&mut c, ui, "Audition in this part");
    assert_eq!(sent(&mut c.out).last().unwrap(), &("audition.start", json!({"id": "s4", "part": 10})));
    c.act(Action::Cancel(None));
    c.act(Action::Cancel(None));
    assert_eq!(value_of(ui, "Fat Bass"), "Auditioning  ·  bass  ·  keys");
    press_row(&mut c, ui, "Fat Bass");
    press_row(&mut c, ui, "Stop auditioning");
    assert_eq!(sent(&mut c.out).last().unwrap(), &("audition.stop", json!({})));
    c.act(Action::Cancel(None));
    c.act(Action::Cancel(None));
    assert_eq!(value_of(ui, "Fat Bass"), "bass  ·  keys");
}

// Filters: a row summing up what is on; Favorites and Recent switch on and off and ask again; the library is
// chosen from everything, my own and the installed ones (asked for when the list opens); tags from the matches'
// own with their counts, the chosen first, all of them must match; a library gone while chosen is let go.
#[test]
fn the_filters_make_the_query() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s);
    assert_eq!(sent(&mut c.out), [("library.query", json!({"kind": "sound", "limit": 1000})), ("library.libraries", json!({}))]);
    c.reply(Request::Query, Ok(sounds()));
    c.reply(Request::Libraries, Ok(json!({"libraries": [{"id": "factory", "name": "Factory", "items": 40}, {"id": "friend", "name": "A Friend's", "items": 3}]})));
    assert_eq!(value_of(ui, "Filters"), "none ›");
    press_row(&mut c, ui, "Filters");
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Sound: filters");
    assert_eq!(labels(ui), ["Favorites", "Recent", "Library", "Tags", "One line a Sound"]);
    assert_eq!(rows(ui).iter().map(|r| (r.kind, r.on)).collect::<Vec<_>>()[..2], [(1, false), (1, false)]);
    assert_eq!((value_of(ui, "Library").as_str(), value_of(ui, "Tags").as_str()), ("Everything ›", "none ›"));
    // Favorites, Recent: each a switch that asks again.
    press_row(&mut c, ui, "Favorites");
    assert_eq!(sent(&mut c.out), [("library.query", json!({"kind": "sound", "limit": 1000, "favorites": true}))]);
    assert!(rows(ui)[0].on && value_of(ui, "Favorites") == "On");
    press_row(&mut c, ui, "Recent");
    assert_eq!(sent(&mut c.out), [("library.query", json!({"kind": "sound", "limit": 1000, "favorites": true, "recent": true}))]);
    // The library: everything, my own, then each installed one with its count; the one chosen says so.
    press_row(&mut c, ui, "Library");
    assert_eq!(sent(&mut c.out), [("library.libraries", json!({}))]);
    assert_eq!(labels(ui), ["Everything", "My own", "Factory", "A Friend's"]);
    assert_eq!((value_of(ui, "Everything").as_str(), value_of(ui, "Factory").as_str(), value_of(ui, "A Friend's").as_str()), ("Chosen", "40", "3"));
    press_row(&mut c, ui, "Factory");
    assert_eq!(sent(&mut c.out), [("library.query", json!({"kind": "sound", "limit": 1000, "favorites": true, "recent": true, "source": "factory"}))]);
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Sound: filters", "back at the filters");
    assert_eq!(value_of(ui, "Library"), "Factory ›");
    press_row(&mut c, ui, "Library");
    press_row(&mut c, ui, "My own");
    assert_eq!(sent(&mut c.out).pop().unwrap().1["source"], "mine");
    // Tags: the chosen first, then the most used; each press chooses or lets go, and asks again.
    press_row(&mut c, ui, "Tags");
    assert_eq!(labels(ui), ["keys", "bass", "pad", "warm", "Hide a tag or give it a color"]);
    assert_eq!(value_of(ui, "keys"), "2");
    press_row(&mut c, ui, "pad");
    assert_eq!(sent(&mut c.out).pop().unwrap().1["tags"], json!(["pad"]));
    assert_eq!(labels(ui), ["pad", "keys", "bass", "warm", "Hide a tag or give it a color"], "the chosen one first");
    assert_eq!((value_of(ui, "pad").as_str(), rows(ui)[0].on), ("✓ 1", true));
    press_row(&mut c, ui, "keys");
    assert_eq!(sent(&mut c.out).pop().unwrap().1["tags"], json!(["pad", "keys"]), "all must match");
    assert_eq!(value_of(ui, "pad"), "✓ 1");
    c.act(Action::Cancel(None)); // the cursor
    c.act(Action::Cancel(None)); // the tags: back at the filters
    assert_eq!(ui.global::<crate::PartState>().get_title().as_str(), "Sound: filters");
    assert_eq!(value_of(ui, "Tags"), "pad, keys ›");
    c.act(Action::Cancel(None));
    c.act(Action::Cancel(None));
    assert_eq!(value_of(ui, "Filters"), "Favorites, Recent, My own, pad, keys ›");
    // Let go: turning Favorites off and the tag off asks again without them.
    press_row(&mut c, ui, "Filters");
    press_row(&mut c, ui, "Favorites");
    assert_eq!(sent(&mut c.out).pop().unwrap().1.get("favorites"), None);
    // A library uninstalled while chosen: everything shows.
    press_row(&mut c, ui, "Library");
    press_row(&mut c, ui, "A Friend's");
    sent(&mut c.out);
    c.reply(Request::Libraries, Ok(json!({"libraries": [{"id": "factory", "name": "Factory", "items": 40}]})));
    assert_eq!(sent(&mut c.out).pop().unwrap().1.get("source"), None, "the library is gone: everything");
    // No tags in the matches: said.
    c.reply(Request::Query, Ok(json!({"items": [], "total": 0, "tags": []})));
    press_row(&mut c, ui, "Tags");
    assert_eq!(labels(ui), ["No tags"]);
    assert_eq!(rows(ui)[0].kind, 4);
}

// The refused queries are said; a Sound's actions have the favorite (added or removed, by the Sound's state) and
// ask again; the panel counts the matches and says when the library has no Sounds at all.
#[test]
fn favorites_are_toggled_and_the_empty_library_is_said() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s);
    sent(&mut c.out);
    c.reply(Request::Query, Ok(sounds()));
    press_row(&mut c, ui, "Warm Rhodes");
    assert_eq!(labels(ui)[4], "Remove from favorites");
    press_row(&mut c, ui, "Remove from favorites");
    assert_eq!(sent(&mut c.out), [("library.favorite", json!({"id": "s1", "favorite": false}))]);
    assert!(!c.reply(Request::Do("favorite"), Ok(json!({"accepted": true}))));
    assert_eq!(sent(&mut c.out), [("library.query", json!({"kind": "sound", "limit": 1000}))], "asked again, so the star goes");
    c.act(Action::Cancel(None));
    c.act(Action::Cancel(None));
    press_row(&mut c, ui, "Fat Bass");
    assert_eq!(labels(ui)[4], "Add to favorites");
    press_row(&mut c, ui, "Add to favorites");
    assert_eq!(sent(&mut c.out), [("library.favorite", json!({"id": "s4", "favorite": true}))]);
    assert!(c.reply(Request::Do("favorite"), Err(Error { code: -32001, message: "no such item".into() })));
    assert_eq!(ui.global::<crate::PartState>().get_message().as_str(), "The engine refused: no such item");
    // The panel's lines: the count, and the empty library's words (not when a filter is on).
    let lines = |ui: &Shell| ui.global::<crate::PartState>().get_info().iter().map(|l| l.text.to_string()).collect::<Vec<_>>();
    let mut empty = open(&s);
    empty.reply(Request::Query, Ok(json!({"items": [], "total": 0, "tags": []})));
    assert!(lines(ui).contains(&"0 of 0 Sounds".to_string()));
    assert!(lines(ui).iter().any(|l| l.starts_with("No Sounds yet.")));
    empty.entered("");
    press_at(&mut empty, ui, 0);
    empty.entered("organ");
    empty.reply(Request::Query, Ok(json!({"items": [], "total": 0, "tags": []})));
    assert!(!lines(ui).iter().any(|l| l.starts_with("No Sounds yet.")), "a search that finds nothing is not an empty library");
}

/// The chooser in the dark theme and the odd ones at the sizes, as pictures (target/screens).
fn pictures(name: &str, setup: &dyn Fn(&mut Sounds, &Shell)) {
    fit_to_window();
    let dark = theme::chosen(&theme::Places::default(), "oscairn-dark", "");
    for ((w, h), scale, size) in [((1280, 720), 1.0, "1280x720"), ((1280, 720), 1.55, "1280x720@1.55"), ((800, 480), 1.0, "800x480")] {
        let mut s = Screen::sized(w, h);
        s.set_scale(scale);
        s.ui.set_screen(6);
        for (t, id) in [(&dark, "dark"), (&odd_theme(), "odd"), (&joined_theme(), "joined")] {
            theme::apply(&s.ui, t);
            s.frame();
            let mut c = open(&s);
            c.reply(Request::Query, Ok(sounds()));
            c.reply(Request::Libraries, Ok(json!({"libraries": [{"id": "factory", "name": "Factory", "items": 40}]})));
            setup(&mut c, &s.ui);
            assert!(s.frame().is_some(), "{name} {id} {size}: drawn");
            png(&s, &format!("{name}-{id}-{size}"));
        }
    }
}

// With no Sound in the library the list offers saving the part as the first one (the save is in a Sound's actions, and
// there is none to open); a search that finds nothing does not offer it.
#[test]
fn an_empty_library_offers_the_first_sound() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s);
    c.reply(Request::Query, Ok(json!({"items": [], "total": 0, "tags": []})));
    assert_eq!(labels(ui), ["Search", "Filters", "Save this part as the first Sound"]);
    let out = press_row(&mut c, ui, "Save this part as the first Sound");
    assert!(matches!(out, Outcome::Prompt(..)), "the name is asked on the keyboard");
    c.entered("Lead");
    assert_eq!(sent(&mut c.out).last().unwrap().0, "parts.saveSound");
    // Narrowed by a search, an empty list is only that.
    press_row(&mut c, ui, "Search");
    c.entered("zzz");
    c.reply(Request::Query, Ok(json!({"items": [], "total": 0, "tags": []})));
    assert_eq!(labels(ui), ["Search", "Filters"]);
}

// Tags: a color shows on the tag's row, a tag is hidden (and stops filtering) or shown again, and the Sounds go to
// one line each, kept in library-view.txt, all by the cursor and a press.
#[test]
fn tags_have_colors_and_can_be_hidden_and_sounds_one_line() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut c = open(&s);
    let mut list = sounds();
    list["tagColors"] = json!([{"tag": "keys", "color": "#46a758"}]);
    list["hiddenTags"] = json!(["old"]);
    c.reply(Request::Query, Ok(list));
    press_row(&mut c, ui, "Filters");
    press_row(&mut c, ui, "Tags");
    let r = rows(ui);
    assert_eq!((r[0].label.as_str(), r[0].tinted), ("keys", true), "the colored tag first, with its color");
    assert!(!r[1].tinted);
    assert_eq!(labels(ui).last().unwrap(), "Show a hidden tag");
    // Choose keys to color: Hide a tag or give it a color > keys > Color > Blue.
    press_row(&mut c, ui, "Hide a tag or give it a color");
    press_row(&mut c, ui, "keys");
    assert_eq!(labels(ui), ["Hide from the tag list", "Color"]);
    assert_eq!(value_of(ui, "Color"), "Green ›");
    press_row(&mut c, ui, "Color");
    assert_eq!(labels(ui)[..3], ["None", "Red", "Orange"]);
    press_row(&mut c, ui, "Blue");
    let sent_now = sent(&mut c.out);
    assert_eq!(sent_now.last().unwrap(), &("library.tag", json!({"tag": "keys", "hidden": false, "color": "#3e63dd"})));
    // Hide it: the choice of it as a filter ends, its color is kept.
    press_row(&mut c, ui, "Hide from the tag list");
    assert_eq!(sent(&mut c.out).last().unwrap(), &("library.tag", json!({"tag": "keys", "hidden": true, "color": "#46a758"})));
    // A hidden one comes back.
    press_row(&mut c, ui, "Show a hidden tag");
    assert_eq!(labels(ui), ["old"]);
    press_row(&mut c, ui, "old");
    assert_eq!(sent(&mut c.out).last().unwrap().1["hidden"], false);
    // EXIT goes up one level each time: tags, filters, the list.
    assert!(matches!(c.back(), Outcome::Stay));
    assert_eq!(labels(ui)[0], "Favorites");
    // One line a Sound, kept for the next time.
    press_row(&mut c, ui, "One line a Sound");
    assert!(crate::settings::library_view().0);
    c.back();
    assert_eq!(value_of(ui, "Warm Rhodes"), "");
    press_row(&mut c, ui, "Filters");
    press_row(&mut c, ui, "One line a Sound");
    assert!(!crate::settings::library_view().0);
}

#[test]
fn the_sound_chooser_is_drawn() {
    pictures("sounds-list", &|c, _| {
        c.act(Action::Move { pane: None, by: 1 });
        c.act(Action::Move { pane: None, by: 2 });
    });
    pictures("sounds-filters", &|c, ui| {
        press_row(c, ui, "Filters");
        press_row(c, ui, "Tags");
        press_row(c, ui, "keys");
    });
}
