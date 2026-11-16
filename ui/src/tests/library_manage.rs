// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Libraries menu (ui/LIBRARY-APP.md C7, D8 to D11): the library exported, the folders indexed in place, a hidden tag
//! shown again, the plugins in the store (what uses each, a build for another platform, removal) and a library
//! uninstalled, each asked first where the reference asks, against the fake engine.

use super::library::*;
use super::library_install::{folder, pick};
use super::*;
use crate::library::Outcome;

fn plugin(id: &str, version: &str, name: &str, used_by: &[&str]) -> serde_json::Value {
    json!({"format": "clap", "id": id, "version": version, "name": name, "platforms": ["linux-arm64", "macos-arm64"], "bytes": 3_000_000, "usedBy": used_by, "open": false, "license": ""})
}

/// The Libraries menu, once the engine has answered what it waits for.
fn libraries_menu(r: &mut Rig) -> Vec<crate::menu::Item> {
    r.tool("Libraries");
    let (title, items) = r.c.take_menu().expect("the Libraries menu");
    assert_eq!(title, "Libraries");
    items
}

fn rig() -> Rig {
    let mut r = Rig::new(library());
    r.e.folders = vec!["/music/Samples".into()];
    r.e.plugins = vec![plugin("org.example.fuzz", "1.2", "Fuzz", &["Glass Choir"]), plugin("org.example.hiss", "0.1", "Hiss", &[])];
    r.e.hidden = vec!["secret".into()];
    r.c.open(); // the list asked again, with what is hidden
    r.settle();
    r
}

// The menu asks the engine for the folders and the plugins and opens when both are in: Export library, the folders, the
// hidden tags, the plugins, and an Uninstall for each library installed.
#[test]
fn the_menu_is_made_of_what_the_engine_holds() {
    let mut r = rig();
    r.e.calls.clear();
    let items = libraries_menu(&mut r);
    assert_eq!(r.e.methods(), ["library.folders", "plugins.list"]);
    assert_eq!(items.iter().map(|i| (i.label.as_str(), i.enabled)).collect::<Vec<_>>(), [("Export library...", true), ("Folders indexed in place", true), ("Show a hidden tag", true), ("Plugins", true), ("Uninstall A Friend's (2 items)", true)]);
    let crate::menu::Kind::Sub(folders) = &items[1].kind else { panic!() };
    assert_eq!(folders.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Stop indexing /music/Samples", "Index a folder in place..."]);
    let crate::menu::Kind::Sub(plugins) = &items[3].kind else { panic!() };
    assert_eq!(plugins.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Fuzz 1.2", "Hiss 0.1"]);
    let crate::menu::Kind::Sub(fuzz) = &plugins[0].kind else { panic!() };
    assert_eq!(fuzz.iter().map(|i| (i.label.as_str(), i.enabled)).collect::<Vec<_>>(), [("Used by Glass Choir", false), ("Builds: linux-arm64, macos-arm64  (2.9 MB)", false), ("Add a build for another platform...", true), ("Remove from this device...", true)]);
    let crate::menu::Kind::Sub(hiss) = &plugins[1].kind else { panic!() };
    assert_eq!(hiss[0].label, "Nothing in the library uses it");
    // With nothing hidden and no plugin their submenus cannot be opened.
    let mut r = Rig::new(library());
    let items = libraries_menu(&mut r);
    assert!(!items[2].enabled && !items[3].enabled, "{items:?}");
    // A plugin of the open Performance that the store does not hold, for another platform's build.
    r.c.graph(&json!({"nodes": [{"id": 2, "name": "synth", "plugin": {"id": "org.example.pad", "path": "/plugins/pad.clap", "name": "Pad Synth"}}, {"id": 3, "name": "x", "plugin": {"id": "org.example.lv", "path": "/plugins/x.lv2", "name": "Lv"}}]}));
    let items = libraries_menu(&mut r);
    let crate::menu::Kind::Sub(plugins) = &items[3].kind else { panic!() };
    assert_eq!(plugins.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["In this Performance", "Pad Synth "], "a CLAP plugin only, with no version the graph does not give");
}

// A hidden tag is shown again, its color kept.
#[test]
fn a_hidden_tag_is_shown_again() {
    let mut r = rig();
    r.e.tag_colors = vec![("secret".into(), "#46a758".into())];
    r.c.reply(crate::library::Request::Libraries, Ok(json!({"libraries": []})));
    r.e.calls.clear();
    r.c.open();
    r.settle();
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Show a hidden tag", "secret"]));
    r.settle();
    assert_eq!(r.e.called("library.tag"), [json!({"tag": "secret", "hidden": false, "color": "#46a758"})]);
    assert!(r.e.hidden.is_empty());
}

// A folder is indexed in place by the chooser's own row for the folder it shows, and taken out by its entry.
#[test]
fn a_folder_is_indexed_in_place_and_taken_out() {
    let dir = folder("index");
    std::fs::create_dir(dir.join("Piano Samples")).unwrap();
    let mut r = rig();
    r.c.set_files_dir(dir.clone());
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Folders indexed in place", "Index a folder in place..."]));
    assert_eq!(labels(r.ui()), ["‹ Up", "Choose this folder", "Piano Samples"]);
    r.press_row("Piano Samples");
    assert_eq!(labels(r.ui()), ["‹ Up", "Choose this folder"]);
    r.e.calls.clear();
    r.press_row("Choose this folder");
    let added = dir.join("Piano Samples").display().to_string();
    assert_eq!(r.e.called("library.addFolder"), [json!({"folder": added})]);
    assert_eq!(footer(r.ui()).0, "Piano Samples is indexed in place.");
    assert_eq!(r.c.path(), ["Library", "Performances"]);
    assert!(r.e.folders.contains(&added));
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Folders indexed in place", "Stop indexing /music/Samples"]));
    r.settle();
    assert_eq!(r.e.called("library.removeFolder"), [json!({"folder": "/music/Samples"})]);
    assert!(!r.e.folders.contains(&"/music/Samples".to_string()));
}

// A plugin is removed after the question that says what uses it; a build for another platform is copied in from a
// file, the platform asked from the list without this device's own.
#[test]
fn a_plugin_is_removed_or_given_a_build() {
    let dir = folder("plugin");
    std::fs::write(dir.join("Fuzz.clap"), b"x").unwrap();
    let mut r = rig();
    r.c.set_files_dir(dir.clone());
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Plugins", "Hiss 0.1", "Remove from this device..."]));
    let (title, message, choices) = r.c.take_notice().unwrap();
    assert_eq!((title.as_str(), message.as_str(), choices), ("Remove Hiss 0.1?", "Nothing in the library uses it. Every platform's build is removed.", vec!["Remove".to_string(), "Cancel".to_string()]));
    r.c.noticed(None);
    r.e.calls.clear();
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Plugins", "Fuzz 1.2", "Remove from this device..."]));
    let (_, message, _) = r.c.take_notice().unwrap();
    assert_eq!(message, "1 item uses it. It keeps its place and saved state and plays silence until it is installed again.");
    r.c.noticed(Some(0));
    r.settle();
    assert_eq!(r.e.called("plugins.remove"), [json!({"format": "clap", "id": "org.example.fuzz", "version": "1.2", "platform": ""})]);
    assert_eq!(footer(r.ui()).0, "Fuzz 1.2 removed.");
    // A build for another platform.
    r.e.plugins = vec![plugin("org.example.hiss", "0.1", "Hiss", &[])];
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Plugins", "Hiss 0.1", "Add a build for another platform..."]));
    assert_eq!(labels(r.ui()), ["‹ Up", "Fuzz.clap"], "a plugin build is a .clap file or bundle");
    r.press_row("Fuzz.clap");
    let (title, platforms) = {
        let (t, items) = r.c.take_menu().expect("the platforms");
        (t, items.iter().map(|i| i.label.clone()).collect::<Vec<_>>())
    };
    assert_eq!(title, "A build of Hiss 0.1");
    assert_eq!(platforms.len(), 4, "every platform but this device's");
    assert!(!platforms.contains(&format!("For {}", crate::library::platform())), "{platforms:?}");
    let first = ["linux-arm64", "linux-x86_64", "macos-arm64", "macos-x86_64", "windows-x86_64"].iter().find(|p| **p != crate::library::platform()).unwrap().to_string();
    r.c.chosen(1);
    assert_eq!(footer(r.ui()).0, format!("Adding the {first} build..."));
    r.e.calls.clear();
    r.settle();
    assert_eq!(r.e.called("plugins.addBuild"), [json!({"format": "clap", "id": "org.example.hiss", "version": "0.1", "platform": first, "path": dir.join("Fuzz.clap").display().to_string()})]);
    assert_eq!(footer(r.ui()).0, format!("Added the {first} build."));
}

// A library is uninstalled after the question that says what stays; its items go from the list.
#[test]
fn a_library_is_uninstalled_after_the_question() {
    let mut r = rig();
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Uninstall A Friend's (2 items)"]));
    let (title, message, _) = r.c.take_notice().unwrap();
    assert_eq!(title, "Uninstall A Friend's?");
    assert_eq!(message, "Its Sounds and Performances are removed. Your Performances keep their parts and samples, and your own copies stay.");
    r.c.noticed(Some(1));
    r.e.calls.clear();
    let items = libraries_menu(&mut r);
    r.c.chosen(pick(&items, &["Uninstall A Friend's (2 items)"]));
    r.c.take_notice().unwrap();
    r.c.noticed(Some(0));
    r.settle();
    assert_eq!(r.e.called("library.uninstall"), [json!({"library": "friend"})]);
    assert_eq!(footer(r.ui()).0, "A Friend's uninstalled.");
    assert!(!labels(r.ui()).contains(&"Untitled".to_string()), "its Performance is gone from the list");
}

// The user's library is exported: a name, the setlists chosen, a file; the engine is asked for the archive with no
// signature (the reference's dialog has none) and the archive is where it was said.
#[test]
fn the_library_is_exported_with_the_setlists_chosen() {
    let dir = folder("library-export");
    let mut r = rig();
    r.c.set_files_dir(dir.clone());
    let items = libraries_menu(&mut r);
    r.e.calls.clear();
    r.c.chosen(pick(&items, &["Export library..."]));
    r.settle();
    assert_eq!(r.e.called("library.query"), [json!({"limit": 1000, "type": ["oscairn.setlist"]})]);
    assert_eq!(labels(r.ui()), ["Name", "Friday Set", "Export...", "Cancel"]);
    assert_eq!(value_of(r.ui(), "Name"), "My library ›");
    assert!(info(r.ui()).join(" ").contains("and the setlists chosen here with the Performances they name:"));
    assert_eq!(r.press_row("Name"), Outcome::Prompt("The library's name".into(), "My library".into()));
    r.c.entered("Stage Pack");
    r.press_row("Friday Set");
    r.press_row("Export...");
    assert_eq!(labels(r.ui())[..3], ["‹ Up", "Name", "Save here"]);
    assert_eq!(value_of(r.ui(), "Name"), "Stage Pack ›");
    r.e.calls.clear();
    r.press_row("Save here");
    r.settle();
    let file = dir.join("Stage Pack.olib");
    assert_eq!(r.e.called("library.exportLibrary"), [json!({"file": file.display().to_string(), "name": "Stage Pack", "setlists": ["l1"]})]);
    assert_eq!(footer(r.ui()).0, "Library exported.");
    assert!(file.exists());
    assert_eq!(r.c.path(), ["Library", "Performances"]);
}
