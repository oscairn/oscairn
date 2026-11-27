// SPDX-License-Identifier: AGPL-3.0-or-later
//! Managing setlists in the Setlist app (issue #41; the Library's Setlists tab is gone): the top bar's Manage opens the menu of
//! Setlist (new, rename, tags, color, duplicate, delete), Bank (new, rename, earlier, later, delete) and Slots (add a Performance,
//! add an empty slot, up, down, remove). Each is reached with panel moves only (the bar's cursor by the third pane, the dial in the
//! menus, the keys of the typing keyboard) and asserted by the request the engine received.

use super::setlist_bar::*;
use super::*;
use crate::{Bar, KeyboardState, NoticeState};
use serde_json::Value;
use slint::platform::Key;
use std::time::Duration;

/// The engine answers a save with `id`, then gives back what the app asks to read again: the setlists (the one saved) and the
/// setlist. Returns the document the save carried.
fn saved(f: &mut Fixture, id: &str) -> Value {
    let save = f.engine.request();
    assert_eq!(save["method"], "setlists.save", "{save}");
    let sent = save["params"]["setlist"].clone();
    f.reply(&save, json!({"result": {"id": id}}));
    let mut kept = sent.clone();
    kept["info"]["id"] = json!(id);
    read_again(f, &[kept]);
    sent
}

/// Answers the reads that follow a change: the setlists listed are `docs`, `setlists.get` gives the one asked for.
fn read_again(f: &mut Fixture, docs: &[Value]) {
    while let Some(r) = f.engine.try_request(Duration::from_millis(60)) {
        let result = match r["method"].as_str() {
            Some("library.query") => json!({"items": docs.iter().map(|d| json!({"id": d["info"]["id"], "name": d["info"]["name"]})).collect::<Vec<_>>(), "total": docs.len()}),
            Some("setlists.get") => json!({"setlist": docs.iter().find(|d| d["info"]["id"] == r["params"]["id"]).expect("a setlist that was kept")}),
            _ => json!({}),
        };
        f.reply(&r, json!({"result": result}));
    }
}

/// The top bar's cursor onto the action `label`, by the third pane, and a press; then the cursor is left where it was.
fn bar_press(f: &mut Fixture, label: &str) {
    let at = |ui: &Shell| ui.global::<Bar>().get_actions().iter().find(|a| a.cursor).map(|a| a.label.to_string());
    let ui = &f.screen.ui;
    shell::act(Action::Move { pane: Some(2), by: 1 });
    let mut moves = 0;
    while at(ui).as_deref() != Some(label) {
        shell::act(Action::Move { pane: Some(2), by: 1 });
        moves += 1;
        assert!(moves < 20, "the bar's cursor never reached {label}");
    }
    shell::act(Action::Confirm(None));
    f.screen.frame(); // the menu is drawn: it holds the rows that fit
}

/// Past the bar's last stop is the screen's own cursor again.
fn leave_bar(f: &Fixture) {
    let mut moves = 0;
    while f.screen.ui.global::<Bar>().get_actions().iter().any(|a| a.cursor) || f.screen.ui.global::<Bar>().get_back_cursor() {
        shell::act(Action::Move { pane: Some(2), by: 1 });
        moves += 1;
        assert!(moves < 20, "the bar kept the cursor");
    }
}

/// The row `label` of the open menu chosen: the dial to it, a press. A disabled row cannot be reached, which the loop reports.
fn choose(f: &mut Fixture, label: &str) {
    for _ in 0..400 {
        let rows: Vec<_> = menu(f).get_rows().iter().collect();
        let (at, to) = (rows.iter().position(|r| r.cursor), rows.iter().position(|r| r.label == label));
        match (at, to) {
            (Some(a), Some(t)) if a == t => {
                shell::act(Action::Confirm(None));
                f.screen.frame();
                return;
            }
            (Some(a), Some(t)) => shell::act(Action::Move { pane: None, by: if t > a { 1 } else { -1 } }),
            _ => shell::act(Action::Move { pane: None, by: 1 }),
        }
    }
    panic!("{label} was not reached in the menu: {:?}", menu(f).get_rows().iter().map(|r| (r.label.to_string(), r.enabled)).collect::<Vec<_>>());
}

/// What the open menu offers: label and whether it can be chosen.
fn offered(f: &Fixture) -> Vec<(String, bool)> {
    menu(f).get_rows().iter().map(|r| (r.label.to_string(), r.enabled)).collect()
}

/// Manage, then the submenu and the entry: `Manage` > `group` > `entry`.
fn manage(f: &mut Fixture, group: &str, entry: &str) {
    bar_press(f, "Manage");
    assert!(menu(f).get_open(), "Manage opens its menu");
    choose(f, group);
    choose(f, entry);
}

/// The text typed on the keyboard that is open (the keys of a typing keyboard), replacing what it started with, and Return.
fn type_text(f: &Fixture, text: &str) {
    let ui = &f.screen.ui;
    assert!(ui.global::<KeyboardState>().get_open(), "the keyboard is asked for");
    for _ in 0..ui.global::<KeyboardState>().get_before().chars().count() {
        assert!(shell::key(&slint::SharedString::from(Key::Backspace)));
    }
    for ch in text.chars() {
        assert!(shell::key(&ch.to_string()));
    }
    assert!(shell::key(&slint::SharedString::from(Key::Return)));
}

/// The slot titled `title` has the cursor, by the dial over the slots (the second pane): the first move shows the cursor, then
/// the distance to the slot.
fn cursor_on(f: &Fixture, title: &str) {
    leave_bar(f);
    if !slots(f).iter().any(|s| s.cursor) {
        shell::act(Action::Move { pane: Some(1), by: 1 });
    }
    let at = slots(f).iter().position(|s| s.cursor).expect("the cursor is on a slot");
    let to = slots(f).iter().position(|s| s.title == title).unwrap_or_else(|| panic!("{title} is not in the view: {:?}", titles(f)));
    shell::act(Action::Move { pane: Some(1), by: to as i32 - at as i32 });
    assert!(slots(f)[to].cursor, "the cursor is on {title}");
}

fn titles(f: &Fixture) -> Vec<String> {
    slots(f).iter().map(|s| s.title.to_string()).collect()
}

fn names_of(setlist: &Value, bank: usize) -> Vec<String> {
    setlist["pages"][bank]["slots"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap_or("(empty)").to_string()).collect()
}

fn fixture(name: &str, banks: usize) -> Fixture {
    let f = on_setlist(name, 2, "list", banks, 4);
    shell::set_text_on_screen(false);
    f
}

// Manage is a second action of the top bar after Play, and its menu is the three groups; nothing is asked of the engine by
// opening it, and Cancel closes it.
#[test]
fn manage_is_a_bar_action_with_three_groups() {
    let mut f = fixture("manage-menu", 2);
    let labels: Vec<String> = f.screen.ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect();
    assert!(labels.iter().position(|l| l == "Play") < labels.iter().position(|l| l == "Manage"), "{labels:?}");
    bar_press(&mut f, "Manage");
    assert_eq!(offered(&f), [("Setlist".to_string(), true), ("Bank".to_string(), true), ("Slots".to_string(), true)]);
    shell::act(Action::Cancel(None));
    assert!(!menu(&f).get_open() && f.screen.ui.get_screen() == 1, "Cancel closes the menu and stays");
    asks_nothing(&mut f, "opening and closing Manage");
}

// A new setlist is named on the keyboard and saved as a document of one empty bank; it is the one shown after.
#[test]
fn a_new_setlist_is_named_and_saved() {
    let mut f = fixture("manage-new", 2);
    manage(&mut f, "Setlist", "New setlist...");
    assert_eq!(f.screen.ui.global::<KeyboardState>().get_title(), "New setlist");
    type_text(&f, "Opening Night");
    let sent = saved(&mut f, "made-1");
    assert_eq!((sent["info"]["id"].as_str(), sent["info"]["name"].as_str()), (Some(""), Some("Opening Night")), "no id: a new setlist");
    assert_eq!(sent["pages"].as_array().map(Vec::len), Some(1));
    assert_eq!(sent["pages"][0]["slots"].as_array().map(Vec::len), Some(0), "an empty bank");
    assert_eq!(stops(&f)[0].text, "Opening Night", "the new setlist is shown");
    assert!(slots(&f).is_empty());
    // Nothing typed: nothing made, and said.
    manage(&mut f, "Setlist", "New setlist...");
    type_text(&f, "");
    asks_nothing(&mut f, "a new setlist with no name");
    // The keyboard cancelled: nothing made.
    manage(&mut f, "Setlist", "New setlist...");
    shell::act(Action::Cancel(None));
    asks_nothing(&mut f, "a new setlist cancelled");
}

// Rename, tags and color change the setlist's info and keep the rest of the document as it was.
#[test]
fn a_setlist_is_renamed_tagged_and_colored() {
    let mut f = fixture("manage-info", 2);
    let was = doc("manage-info-1", "Night 1", "list", 2, 4);
    manage(&mut f, "Setlist", "Rename...");
    assert_eq!(f.screen.ui.global::<KeyboardState>().get_before(), "Night 1", "it starts with the name");
    type_text(&f, "Friday");
    let sent = saved(&mut f, "manage-info-1");
    assert_eq!(sent["info"]["name"], "Friday");
    assert_eq!(sent["info"]["id"], "manage-info-1", "saved over the same setlist");
    assert_eq!(sent["pages"], was["pages"], "the banks are as they were");
    assert_eq!(stops(&f)[0].text, "Friday");
    manage(&mut f, "Setlist", "Tags...");
    type_text(&f, "live, club, live");
    let sent = saved(&mut f, "manage-info-1");
    assert_eq!(sent["info"]["tags"], json!(["live", "club"]), "each once, trimmed");
    assert_eq!(sent["info"]["name"], "Friday", "the name kept by the first save");
    manage(&mut f, "Setlist", "Color");
    choose(&mut f, "Blue");
    let sent = saved(&mut f, "manage-info-1");
    assert_eq!(sent["info"]["color"], "#3e63dd");
    assert_eq!(sent["info"]["tags"], json!(["live", "club"]), "the tags kept");
}

// Duplicate is the engine's library.copy of the setlist open, and the copy is shown after; Delete asks first, and only the
// first choice sends library.delete.
#[test]
fn a_setlist_is_duplicated_and_deleted() {
    let mut f = fixture("manage-copy", 2);
    manage(&mut f, "Setlist", "Duplicate");
    let copy = f.engine.request();
    assert_eq!((copy["method"].as_str(), copy["params"].clone()), (Some("library.copy"), json!({"id": "manage-copy-1"})));
    f.reply(&copy, json!({"result": {"id": "manage-copy-9"}}));
    let mut second = doc("manage-copy-9", "Night 1 copy", "list", 2, 4);
    second["info"]["id"] = json!("manage-copy-9");
    read_again(&mut f, &[doc("manage-copy-1", "Night 1", "list", 2, 4), second]);
    assert_eq!(stops(&f)[0].text, "Night 1 copy", "the copy is shown");
    // Delete, then Cancel: no request.
    manage(&mut f, "Setlist", "Delete...");
    assert!(f.screen.ui.global::<NoticeState>().get_open());
    assert!(f.screen.ui.global::<NoticeState>().get_title().contains("Night 1 copy"), "{}", f.screen.ui.global::<NoticeState>().get_title());
    shell::act(Action::Cancel(None));
    asks_nothing(&mut f, "a delete cancelled");
    // Delete, then the first choice.
    manage(&mut f, "Setlist", "Delete...");
    let choices: Vec<_> = f.screen.ui.global::<NoticeState>().get_choices().iter().map(|c| c.to_string()).collect();
    assert_eq!(choices[0], "Delete");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let delete = f.engine.request();
    assert_eq!((delete["method"].as_str(), delete["params"].clone()), (Some("library.delete"), json!({"id": "manage-copy-9"})));
    f.reply(&delete, json!({"result": {"accepted": true}}));
    read_again(&mut f, &[doc("manage-copy-1", "Night 1", "list", 2, 4)]);
    assert_eq!(stops(&f)[0].text, "Night 1", "the one left is shown");
    // The last one deleted: nothing to show, and the rest of Manage is off.
    manage(&mut f, "Setlist", "Delete...");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let delete = f.engine.request();
    assert_eq!(delete["params"], json!({"id": "manage-copy-1"}));
    f.reply(&delete, json!({"result": {"accepted": true}}));
    read_again(&mut f, &[]);
    assert_eq!((stops(&f)[0].text.as_str(), slots(&f).len()), ("No setlist", 0));
    bar_press(&mut f, "Manage");
    choose(&mut f, "Setlist");
    assert_eq!(offered(&f).iter().filter(|r| r.1).map(|r| r.0.as_str()).collect::<Vec<_>>(), ["New setlist..."], "only a new one can be made");
}

// Banks: new, renamed, moved earlier and later, deleted (asked first when it holds slots).
#[test]
fn banks_are_made_renamed_moved_and_deleted() {
    let mut f = fixture("manage-banks", 2);
    manage(&mut f, "Bank", "New bank");
    let sent = saved(&mut f, "manage-banks-1");
    assert_eq!(sent["pages"].as_array().map(Vec::len), Some(3));
    assert_eq!(sent["pages"][2]["slots"].as_array().map(Vec::len), Some(0));
    assert_eq!(stops(&f)[1].text, "Bank 3  3/3", "the new bank is the one shown");
    manage(&mut f, "Bank", "Rename...");
    type_text(&f, "Encore");
    let sent = saved(&mut f, "manage-banks-1");
    assert_eq!(sent["pages"][2]["name"], "Encore");
    assert_eq!(sent["pages"][0]["name"], "Set 1", "the others kept");
    assert_eq!(stops(&f)[1].text, "Encore  3/3");
    manage(&mut f, "Bank", "Move earlier");
    let sent = saved(&mut f, "manage-banks-1");
    assert_eq!([sent["pages"][1]["name"].as_str(), sent["pages"][2]["name"].as_str()], [Some("Encore"), Some("")], "the bank before it is now after it");
    assert_eq!(stops(&f)[1].text, "Encore  2/3", "the moved bank is still the one shown");
    manage(&mut f, "Bank", "Move later");
    let sent = saved(&mut f, "manage-banks-1");
    assert_eq!(sent["pages"][2]["name"], "Encore");
    // The last bank is empty: deleted at once, no question.
    manage(&mut f, "Bank", "Delete...");
    assert!(!f.screen.ui.global::<NoticeState>().get_open(), "an empty bank is deleted without asking");
    let sent = saved(&mut f, "manage-banks-1");
    assert_eq!(sent["pages"].as_array().map(Vec::len), Some(2));
    // The first bank has slots: asked first.
    leave_bar(&f);
    shell::act(Action::Move { pane: Some(0), by: 1 });
    shell::act(Action::Move { pane: Some(0), by: 2 });
    shell::act(Action::Confirm(None)); // Previous
    assert_eq!(stops(&f)[1].text, "Set 1  1/2");
    manage(&mut f, "Bank", "Delete...");
    assert!(f.screen.ui.global::<NoticeState>().get_open());
    shell::act(Action::Cancel(None));
    asks_nothing(&mut f, "a bank delete cancelled");
    manage(&mut f, "Bank", "Delete...");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let sent = saved(&mut f, "manage-banks-1");
    assert_eq!(sent["pages"].as_array().map(Vec::len), Some(1), "one bank left");
    assert_eq!(sent["pages"][0]["name"], "", "the second bank is the one that stays");
    // The last bank cannot be deleted, and Move earlier and later are off with one bank.
    bar_press(&mut f, "Manage");
    choose(&mut f, "Bank");
    assert!(offered(&f).iter().filter(|r| r.0 == "Delete..." || r.0.starts_with("Move")).all(|r| !r.1), "{:?}", offered(&f));
}

// Slots: a Performance from the library added by its name to the bank shown, an empty slot, moved up and down, removed.
#[test]
fn slots_are_added_moved_and_removed() {
    let mut f = fixture("manage-slots", 2);
    manage(&mut f, "Slots", "Add a Performance...");
    assert_eq!(menu(&f).get_title(), "Add a Performance");
    choose(&mut f, &song(30));
    let sent = saved(&mut f, "manage-slots-1");
    let want = song(30);
    assert_eq!(names_of(&sent, 0).last(), Some(&want), "the last slot of the bank shown");
    assert_eq!(sent["pages"][0]["slots"].as_array().unwrap().last().unwrap()["id"], "p30");
    assert_eq!(names_of(&sent, 1), names_of(&doc("manage-slots-1", "Night 1", "list", 2, 4), 1), "the other bank is as it was");
    assert!(slots(&f).last().is_some_and(|s| s.cursor && s.title == want), "the cursor is on the new slot: {:?}", titles(&f));
    // An empty slot.
    manage(&mut f, "Slots", "Add an empty slot");
    let sent = saved(&mut f, "manage-slots-1");
    assert!(sent["pages"][0]["slots"].as_array().unwrap().last().unwrap().is_null());
    // The slot under the cursor moves up: before the one above it, the cursor going with it.
    let first = titles(&f);
    cursor_on(&f, &first[2]);
    manage(&mut f, "Slots", "Move up");
    let sent = saved(&mut f, "manage-slots-1");
    assert_eq!(names_of(&sent, 0)[..3], [first[0].clone(), first[2].clone(), first[1].clone()]);
    assert!(slots(&f)[1].cursor, "the cursor moved with it");
    manage(&mut f, "Slots", "Move down");
    let sent = saved(&mut f, "manage-slots-1");
    assert_eq!(names_of(&sent, 0)[..3], [first[0].clone(), first[1].clone(), first[2].clone()], "back where it was");
    // Move up is off at the first slot, Move down at the last.
    cursor_on(&f, &first[0]);
    bar_press(&mut f, "Manage");
    choose(&mut f, "Slots");
    assert!(offered(&f).iter().any(|r| r.0 == "Move up" && !r.1) && offered(&f).iter().any(|r| r.0 == "Move down" && r.1), "{:?}", offered(&f));
    shell::act(Action::Cancel(None)); // up to the groups
    shell::act(Action::Cancel(None)); // closed
    assert!(!menu(&f).get_open());
    leave_bar(&f);
    // Remove: the slot under the cursor goes and the cursor stays on the place.
    cursor_on(&f, &first[1]);
    manage(&mut f, "Slots", "Remove");
    let sent = saved(&mut f, "manage-slots-1");
    assert!(!names_of(&sent, 0).contains(&first[1]), "{:?}", names_of(&sent, 0));
    assert_eq!(names_of(&sent, 0)[..2], [first[0].clone(), first[2].clone()]);
    assert!(slots(&f)[1].cursor && slots(&f)[1].title == first[2], "the cursor is on what took its place");
}

// A refusal of the save is said in the top bar and nothing else changes.
#[test]
fn a_refused_save_is_said() {
    let mut f = fixture("manage-refused", 2);
    manage(&mut f, "Bank", "New bank");
    let save = f.engine.request();
    f.reply(&save, json!({"error": {"code": -32602, "message": "a setlist holds at most 1000 slots"}}));
    assert!(f.screen.ui.global::<Bar>().get_notice().contains("1000"), "{}", f.screen.ui.global::<Bar>().get_notice());
    asks_nothing(&mut f, "a refused save");
    assert_eq!(stops(&f)[1].text, "Set 1  1/2", "the setlist as it was");
}

// By touch: a tap on Manage in the bar, a tap on a group and on an entry (the menu's rows); a slot tapped (which opens it) is the
// one the slot entries act on.
#[test]
fn manage_by_touch() {
    let mut f = fixture("manage-touch", 2);
    let manage_at = f.screen.ui.global::<Bar>().get_actions().iter().position(|a| a.label == "Manage").expect("Manage is on the bar");
    f.screen.ui.global::<Bar>().invoke_action(manage_at as i32);
    f.screen.frame();
    let tap = |f: &mut Fixture, label: &str| {
        let row = menu(f).get_rows().iter().position(|r| r.label == label).unwrap_or_else(|| panic!("{label} is not a row: {:?}", offered(f)));
        menu(f).invoke_tapped(row as i32);
        f.screen.frame();
    };
    tap(&mut f, "Bank");
    tap(&mut f, "New bank");
    let sent = saved(&mut f, "manage-touch-1");
    assert_eq!(sent["pages"].as_array().map(Vec::len), Some(3));
    assert!(titles(&f).is_empty(), "the new bank is empty: {:?}", titles(&f));
    state(&f).invoke_tapped_stop(2);
    state(&f).invoke_tapped_stop(2);
    state(&f).invoke_tapped_stop(2); // back to the first bank
    let first = titles(&f);
    assert!(first.len() > 1, "{first:?}");
    state(&f).invoke_tapped_slot(1);
    let load = f.engine.request();
    assert_eq!(load["method"], "library.load");
    f.reply(&load, json!({"result": {}}));
    quiet(&mut f);
    f.screen.ui.global::<Bar>().invoke_action(manage_at as i32);
    f.screen.frame();
    tap(&mut f, "Slots");
    tap(&mut f, "Remove");
    let sent = saved(&mut f, "manage-touch-1");
    assert!(!names_of(&sent, 0).contains(&first[1]) && names_of(&sent, 0).len() == first.len() - 1, "the tapped slot is the one removed: {:?}", names_of(&sent, 0));
}
