// SPDX-License-Identifier: AGPL-3.0-or-later
//! What is done with an item: its menu and the rules for which entries are on, editing, color, duplicate, delete
//! (asked first), a tag's menu (ui/LIBRARY-APP.md rows C1 to C7), against the fake engine.

use super::library::*;
use super::*;
use crate::library::Outcome;

/// The menu Options opens for the row `label`: the cursor goes there first.
pub fn options_for(r: &mut Rig, label: &str) -> Vec<(String, bool)> {
    let target = labels(r.ui()).iter().position(|l| l == label).unwrap();
    let now = rows(r.ui()).iter().position(|x| x.cursor);
    match now {
        None => {
            r.act(Action::Move { pane: None, by: 1 });
            r.act(Action::Move { pane: None, by: target as i32 });
        }
        Some(at) => {
            r.act(Action::Move { pane: None, by: target as i32 - at as i32 });
        }
    }
    r.tool("Options");
    let (_, items) = r.c.take_menu().expect("a menu");
    items.iter().map(|i| (i.label.clone(), i.enabled)).collect()
}

pub fn on(menu: &[(String, bool)]) -> Vec<&str> {
    menu.iter().filter(|m| m.1).map(|m| m.0.as_str()).collect()
}

// The menu by kind and by whether the item is the user's own or a locked library's: a Performance opens, goes to a
// setlist, is tried; a Sound is made a part; the user's own is edited, colored, duplicated, exported and deleted; a
// locked one is copied into the user's own, exported and deleted, and neither edited nor colored.
#[test]
fn the_menu_is_by_kind_and_ownership() {
    let mut r = Rig::new(library());
    let own = options_for(&mut r, "Brass Corner");
    assert_eq!(own.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["Open", "Add to setlist", "Audition", "Add to favorites", "Edit name, description and tags...", "Color", "Duplicate", "Export...", "Import into the open Performance...", "Delete..."]);
    assert_eq!(on(&own), ["Open", "Add to setlist", "Audition", "Add to favorites", "Edit name, description and tags...", "Color", "Duplicate", "Export...", "Import into the open Performance...", "Delete..."]);
    let locked = options_for(&mut r, "Untitled");
    assert_eq!(locked[6].0, "Copy into my own, to edit");
    assert!(!locked[4].1 && !locked[5].1, "a locked library's item cannot be edited or colored");
    assert!(locked[6].1 && locked[9].1);
    let starred = options_for(&mut r, "Evening Strings");
    assert_eq!(starred[3].0, "Remove from favorites");
    r.c.show_page(1);
    r.settle();
    let sound = options_for(&mut r, "Soft Felt");
    assert_eq!(sound.iter().map(|m| m.0.as_str()).collect::<Vec<_>>()[..3], ["New part", "Audition", "Add to favorites"]);
    r.c.show_page(2);
    r.settle();
    let setlist = options_for(&mut r, "Friday Set");
    assert_eq!(setlist.iter().map(|m| m.0.as_str()).collect::<Vec<_>>()[..2], ["Add to favorites", "Edit name, description and tags..."], "a setlist is neither opened, tried nor made a part");
    // An item indexed in place or whose type's package is gone: read only, and not exported or deleted in place.
    let mut items = library();
    items[3].read_only = true;
    items[3].in_place = true;
    let mut r = Rig::new(items);
    let held = options_for(&mut r, "Brass Corner");
    assert!(!held[4].1 && !held[5].1 && !held[6].1 && !held[7].1 && !held[9].1, "read only and in place: no edit, color, copy or delete: {held:?}");
}

// Edit name, description and tags: three texts on the keyboard, tags split on commas, trimmed and each once; Save sends
// them with the color kept; "NAME saved."; Cancel sends nothing.
#[test]
fn an_item_is_edited_in_three_fields() {
    let mut r = Rig::new(library());
    options_for(&mut r, "Brass Corner");
    r.c.chosen(5);
    assert_eq!(labels(r.ui()), ["Name", "Description", "Tags", "Save", "Cancel"]);
    assert_eq!(r.c.path(), ["Library", "Performances", "Edit Brass Corner"]);
    assert_eq!(value_of(r.ui(), "Tags"), "brass, warm ›");
    assert_eq!(r.press_row("Name"), Outcome::Prompt("Name".into(), "Brass Corner".into()));
    r.c.entered(" Brass Hall ");
    assert_eq!(r.press_row("Description"), Outcome::Prompt("Description".into(), String::new()));
    r.c.entered("Big and bright");
    assert_eq!(r.press_row("Tags"), Outcome::Prompt("Tags, separated by commas".into(), "brass, warm".into()));
    r.c.entered("brass, hall , brass,,");
    assert_eq!(value_of(r.ui(), "Name"), "Brass Hall ›");
    r.e.calls.clear();
    r.press_row("Save");
    r.settle();
    assert_eq!(r.e.called("library.edit"), [json!({"id": "p4", "name": "Brass Hall", "description": "Big and bright", "tags": ["brass", "hall"], "color": ""})]);
    assert_eq!(footer(r.ui()).0, "Brass Hall saved.");
    assert_eq!(labels(r.ui())[0], "Untitled", "back on the list");
    assert!(labels(r.ui()).contains(&"Brass Hall".to_string()));
    // Cancel: nothing is sent.
    options_for(&mut r, "Brass Hall");
    r.c.chosen(5);
    r.e.calls.clear();
    r.press_row("Cancel");
    r.settle();
    assert!(r.e.called("library.edit").is_empty());
    assert_eq!(labels(r.ui())[0], "Untitled");
}

// A color from the nine, or none: `library.edit` with the item's own texts and the color's hex; the list shows it.
#[test]
fn an_item_is_colored() {
    let mut r = Rig::new(library());
    let menu = options_for(&mut r, "Brass Corner");
    assert_eq!(menu[5].0, "Color");
    r.c.chosen(104); // Green: None is 100, Red 101, Orange 102, Yellow 103, Green 104
    r.settle();
    assert_eq!(r.e.called("library.edit"), [json!({"id": "p4", "name": "Brass Corner", "description": "", "tags": ["brass", "warm"], "color": "#46a758"})]);
    assert!(rows(r.ui())[1].tinted);
    // None.
    options_for(&mut r, "Brass Corner");
    r.c.chosen(100);
    r.settle();
    assert_eq!(r.e.called("library.edit").last().unwrap()["color"], "");
    assert!(!rows(r.ui())[1].tinted);
}

// Duplicate (the user's own: "NAME copy") or copy into my own (a library's, which keeps its name).
#[test]
fn an_item_is_duplicated_or_copied_into_my_own() {
    let mut r = Rig::new(library());
    options_for(&mut r, "Brass Corner");
    r.c.chosen(6);
    r.settle();
    assert_eq!(footer(r.ui()).0, "Brass Corner duplicated.");
    assert!(labels(r.ui()).contains(&"Brass Corner copy".to_string()));
    options_for(&mut r, "Untitled");
    r.c.chosen(6);
    r.settle();
    assert_eq!(footer(r.ui()).0, "Untitled copied into your own, to edit.");
    assert_eq!(r.e.called("library.copy"), [json!({"id": "p4"}), json!({"id": "p2"})]);
}

// Delete asks first, in words of its own for the user's own and a library's; Cancel or a close deletes nothing.
#[test]
fn a_delete_is_asked_first() {
    let mut r = Rig::new(library());
    options_for(&mut r, "Brass Corner");
    r.c.chosen(7);
    let (title, message, choices) = r.c.take_notice().unwrap();
    assert_eq!((title.as_str(), message.as_str()), ("Delete Brass Corner?", "It is removed from your library. Performances that used it keep their parts."));
    assert_eq!(choices, ["Delete", "Cancel"]);
    r.c.noticed(Some(1));
    r.c.noticed(None);
    r.settle();
    assert!(r.e.called("library.delete").is_empty());
    options_for(&mut r, "Brass Corner");
    r.c.chosen(7);
    r.c.take_notice().unwrap();
    r.c.noticed(Some(0));
    r.settle();
    assert_eq!(r.e.called("library.delete"), [json!({"id": "p4"})]);
    assert_eq!(footer(r.ui()).0, "Brass Corner deleted.");
    assert!(!labels(r.ui()).contains(&"Brass Corner".to_string()));
    options_for(&mut r, "Untitled");
    r.c.chosen(7);
    let (_, message, _) = r.c.take_notice().unwrap();
    assert_eq!(message, "It is removed. Installing its library again brings it back.");
}

// An engine refusal of an edit is said as the engine words it.
#[test]
fn a_refusal_is_said() {
    let mut r = Rig::new(library());
    r.e.fail.push(("library.copy".into(), "The disk is full".into()));
    options_for(&mut r, "Brass Corner");
    r.c.chosen(6);
    r.settle();
    assert_eq!(footer(r.ui()).0, "The disk is full");
}

// A tag's menu (Options with the sidebar's cursor on the tag): hide it from the tag list (a hidden tag chosen as a filter
// stops filtering) or give it a color, which shows beside it; hidden ones come back from the Libraries menu (second cut).
#[test]
fn a_tag_is_hidden_or_colored() {
    let mut r = Rig::new(library());
    // The left arrow reaches the sidebar; the dial walks its choices (headings are stops now) down to "warm".
    r.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(r.side().iter().position(|x| x.cursor), Some(0), "the sidebar has the cursor, on Favorites");
    r.act(Action::Move { pane: None, by: 7 });
    assert_eq!(r.side().iter().find(|x| x.cursor).map(|x| x.label.to_string()), Some("warm".into()));
    r.tool("Options");
    let (title, items) = r.c.take_menu().expect("the tag's menu");
    assert_eq!(title, "warm");
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Hide from the tag list", "Color"]);
    r.c.chosen(104); // Green
    r.settle();
    assert_eq!(r.e.called("library.tag"), [json!({"tag": "warm", "hidden": false, "color": "#46a758"})]);
    assert!(r.side().iter().find(|x| x.label == "warm").unwrap().tinted, "the tag's color shows");
    // Hidden: gone from the list, and no longer filtering.
    r.act(Action::Confirm(None));
    assert_eq!(r.e.called("library.query").last().unwrap()["tags"], json!(["warm"]));
    r.tool("Options");
    r.c.take_menu().unwrap();
    r.c.chosen(1);
    r.settle();
    assert_eq!(r.e.called("library.tag").last().unwrap(), &json!({"tag": "warm", "hidden": true, "color": "#46a758"}));
    assert!(!r.side_labels().contains(&"warm".to_string()));
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000}), "it no longer filters");
}
