// SPDX-License-Identifier: AGPL-3.0-or-later
//! Add to setlist (ui/LIBRARY-APP.md row B8): a Performance becomes the last slot of the page the playing slot is
//! on, in the setlist the Setlist app has open, by `setlists.get` and `setlists.save`.

use super::library::*;
use super::*;

fn state(name: &str, content: Option<serde_json::Value>) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("oscairn-ui-lib-state-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("state.json");
    if let Some(c) = content {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, c.to_string()).unwrap();
    }
    file
}

fn setlist_doc(id: &str, name: &str) -> serde_json::Value {
    json!({"format": "oscairn.setlist", "version": 2,
        "info": {"id": id, "name": name, "description": "", "tags": [], "color": "", "revision": "", "parent": "", "hash": "", "origin": null},
        "display": {"mode": "list"},
        "pages": [{"name": "Set 1", "slots": [{"id": "x1", "name": "One", "color": "", "label": ""}, null]}, {"name": "Encore", "slots": [{"id": "x2", "name": "Two", "color": "", "label": "Last"}]}]})
}

fn add_via_menu(r: &mut Rig, label: &str) {
    let target = labels(r.ui()).iter().position(|l| l == label).unwrap();
    match rows(r.ui()).iter().position(|x| x.cursor) {
        None => {
            r.act(Action::Move { pane: None, by: 1 });
            r.act(Action::Move { pane: None, by: target as i32 });
        }
        Some(at) => {
            r.act(Action::Move { pane: None, by: target as i32 - at as i32 });
        }
    }
    r.tool("Options");
    r.c.take_menu().unwrap();
    r.c.chosen(2);
    r.settle();
}

// With a setlist open and a slot playing, the slot goes to the end of that slot's page, and the document is saved
// whole with the one slot more: the other pages, the empty slot and the labels as they were.
#[test]
fn a_performance_goes_to_the_page_playing() {
    let mut r = Rig::new(library());
    r.e.docs = vec![setlist_doc("l1", "Friday Set")];
    // Slots count over the pages: 0 and 1 are on Set 1 (one empty), 2 is the Encore's.
    let file = state("page", Some(json!({"setlist": "l1", "current": 2})));
    r.c.set_state_file(Some(file));
    r.e.calls.clear();
    add_via_menu(&mut r, "Brass Corner");
    assert_eq!(r.e.methods(), ["setlists.get", "setlists.save"]);
    assert_eq!(r.e.called("setlists.get"), [json!({"id": "l1"})]);
    let saved = &r.e.called("setlists.save")[0]["setlist"];
    assert_eq!(saved["pages"][0], setlist_doc("l1", "Friday Set")["pages"][0], "the first page is as it was");
    assert_eq!(saved["pages"][1]["slots"], json!([{"id": "x2", "name": "Two", "color": "", "label": "Last"}, {"id": "p4", "name": "Brass Corner", "color": "", "label": ""}]));
    assert_eq!(footer(r.ui()).0, "Added to Friday Set");
    // Nothing playing: the first page.
    let file = state("first", Some(json!({"setlist": "l1"})));
    r.c.set_state_file(Some(file));
    add_via_menu(&mut r, "Evening Strings");
    let saved = &r.e.called("setlists.save")[1]["setlist"];
    assert_eq!(saved["pages"][0]["slots"][2], json!({"id": "p1", "name": "Evening Strings", "color": "", "label": ""}));
}

// No setlist open: the user's own "Setlist" is the one, made when there is none, and then it is the one open.
#[test]
fn with_no_setlist_open_the_one_called_setlist_is_used_or_made() {
    let mut r = Rig::new(library());
    let file = state("none", None);
    r.c.set_state_file(Some(file.clone()));
    r.e.calls.clear();
    add_via_menu(&mut r, "Brass Corner");
    assert_eq!(r.e.methods(), ["library.query", "setlists.save"]);
    assert_eq!(r.e.called("library.query")[0], json!({"kind": "setlist", "limit": 1000, "source": "mine"}));
    let saved = &r.e.called("setlists.save")[0]["setlist"];
    assert_eq!(saved["info"]["name"], "Setlist");
    assert_eq!(saved["pages"], json!([{"name": "Page 1", "slots": [{"id": "p4", "name": "Brass Corner", "color": "", "label": ""}]}]));
    assert_eq!(footer(r.ui()).0, "Added to Setlist");
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(kept["setlist"].as_str().unwrap().starts_with("setlist-"), "the new setlist is the one open now: {kept}");
    // One called "Setlist" there already is opened, not made again.
    let mut r = Rig::new(library());
    r.e.items.push(super::library_fake::stored("setlist", "own-1", "Setlist"));
    r.e.docs = vec![setlist_doc("own-1", "Setlist")];
    r.c.set_state_file(Some(state("named", None)));
    r.e.calls.clear();
    add_via_menu(&mut r, "Brass Corner");
    assert_eq!(r.e.methods(), ["library.query", "setlists.get", "setlists.save"]);
    assert_eq!(r.e.called("setlists.save")[0]["setlist"]["info"]["id"], "own-1");
}

// A setlist holds at most 1000 slots; the engine's refusal is said.
#[test]
fn a_full_setlist_and_a_refusal_are_said() {
    let mut r = Rig::new(library());
    let mut doc = setlist_doc("l1", "Friday Set");
    doc["pages"][0]["slots"] = json!((0..1000).map(|i| json!({"id": format!("x{i}"), "name": "S", "color": "", "label": ""})).collect::<Vec<_>>());
    r.e.docs = vec![doc];
    r.c.set_state_file(Some(state("full", Some(json!({"setlist": "l1"})))));
    r.e.calls.clear();
    add_via_menu(&mut r, "Brass Corner");
    assert_eq!(footer(r.ui()).0, "A setlist holds at most 1000 slots.");
    assert!(r.e.called("setlists.save").is_empty());
    r.e.docs = vec![setlist_doc("l1", "Friday Set")];
    r.e.fail.push(("setlists.save".into(), "The disk is full".into()));
    add_via_menu(&mut r, "Brass Corner");
    assert_eq!(footer(r.ui()).0, "The disk is full");
}
