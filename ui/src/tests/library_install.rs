// SPDX-License-Identifier: AGPL-3.0-or-later
//! Installing an archive and exporting an item (ui/LIBRARY-APP.md D1 to D3, D12, C8) against the fake engine: the file
//! chooser over a real folder, the install choices and the warnings for code from an unknown source, the export's
//! choices and signature, a file written over only once the new one is written.

use super::library::*;
use super::library_items::options_for;
use super::*;
use crate::library::Outcome;
use crate::menu;

/// The id of the choice at `path` in a menu's items, through its submenus.
pub fn pick(items: &[menu::Item], path: &[&str]) -> usize {
    let found = items.iter().find(|i| i.label == path[0] || i.label.starts_with(&format!("{} ", path[0])) && path.len() == 1).unwrap_or_else(|| panic!("{} in {:?}", path[0], items.iter().map(|i| &i.label).collect::<Vec<_>>()));
    match &found.kind {
        menu::Kind::Choice(id) => *id,
        menu::Kind::Sub(inner) => pick(inner, &path[1..]),
        menu::Kind::Head => panic!("{} is a heading", path[0]),
    }
}

pub fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("oscairn-ui-lib-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn archive(kind: &str, items: &[(&str, &str, &str)], code: serde_json::Value) -> serde_json::Value {
    json!({"id": "friend-pack", "name": "Friend Pack", "kind": kind,
        "items": items.iter().map(|(k, id, name)| json!({"kind": k, "id": id, "name": name})).collect::<Vec<_>>(),
        "code": code, "signer": {"kind": "archive", "name": "", "version": "", "platform": "", "status": "trusted", "key": "0123456789abcdef", "keyName": "Ada", "publicKey": ""},
        "license": "CC0", "source": {"url": "https://example.org/pack", "revision": "v1"}})
}

/// A panel's lines as one text per line.
fn rows_text(lines: &[String]) -> Vec<String> {
    lines.to_vec()
}

/// The menu entry that exports an item (actions.rs).
const EXPORT: usize = 8;

/// The chooser opened on `dir` by Install... then File.
fn open_chooser(r: &mut Rig, dir: &std::path::Path) {
    r.c.set_files_dir(dir.to_path_buf());
    r.tool("Install");
    let (_, items) = r.c.take_menu().expect("the Install menu");
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Catalog", "File"], "exactly two ways");
    r.c.chosen(2);
}

// Install... is Catalog and File; File is the chooser over the folder, which lists the folders and the .olib files; a file
// chosen has its contents read (`library.contents`) and the choices are asked.
#[test]
fn a_file_is_chosen_and_its_contents_asked_about() {
    let dir = folder("install");
    std::fs::write(dir.join("pack.olib"), b"x").unwrap();
    std::fs::write(dir.join("notes.txt"), b"x").unwrap();
    std::fs::create_dir(dir.join("Songs")).unwrap();
    let mut r = Rig::new(library());
    r.e.archives = vec![(dir.join("pack.olib").display().to_string(), archive("library", &[("performance", "x1", "Dawn Pad"), ("sound", "x2", "Warm Bass"), ("performance", "x3", "Dusk Lead")], json!([])))];
    open_chooser(&mut r, &dir);
    assert_eq!(labels(r.ui()), ["‹ Up", "Songs", "pack.olib"]);
    assert_eq!(r.c.path()[2], "Install a library, an exported Performance or Sound, or a plugin package");
    r.e.calls.clear();
    r.press_row("pack.olib");
    assert_eq!(r.e.methods(), ["library.contents"]);
    assert_eq!(r.e.called("library.contents"), [json!({"file": dir.join("pack.olib").display().to_string()})]);
    // A library archive is installed locked by default; Performances first, then Sounds.
    assert_eq!(labels(r.ui()), ["Into my own, to edit", "As a locked library", "Dawn Pad", "Dusk Lead", "Warm Bass", "Install", "Cancel"]);
    let on: Vec<bool> = rows(r.ui()).iter().map(|x| x.on).collect();
    assert_eq!(on, [false, true, true, true, true, false, false]);
    assert_eq!(rows(r.ui())[2].value.to_string(), "Performance");
    let lines = info(r.ui());
    assert_eq!(lines[0], "Install Friend Pack");
    let text = lines.join(" ");
    assert!(text.contains("Signed by Ada (0123 4567 89ab cdef), a key you trust"), "{text}");
    assert!(text.contains("CC0") && text.contains("Source: https://example.org/pack at v1"), "{text}");
    // Everything ticked is all of it: an empty list.
    r.e.calls.clear();
    r.press_row("Install");
    assert_eq!(r.e.called("library.install"), [json!({"file": dir.join("pack.olib").display().to_string(), "asLibrary": true, "items": [], "acceptUnsignedCode": false})]);
    assert_eq!(footer(r.ui()).0, "Installed 3.");
    assert_eq!(r.c.path(), ["Library", "Performances"], "back on the list");
    assert!(labels(r.ui()).contains(&"Dawn Pad".to_string()) && value_of(r.ui(), "Dawn Pad") == "Locked");
    assert!(r.e.methods().contains(&"library.libraries".to_string()), "the libraries read again: {:?}", r.e.methods());
}

// Which items go and how: own or locked, a choice of some; none ticked is said; the ones already here are counted.
#[test]
fn the_choices_make_the_request() {
    let dir = folder("choices");
    std::fs::write(dir.join("pack.olib"), b"x").unwrap();
    let mut r = Rig::new(library());
    r.e.archives = vec![(dir.join("pack.olib").display().to_string(), archive("performance", &[("performance", "x1", "Dawn Pad"), ("performance", "p4", "Brass Corner"), ("sound", "x2", "Warm Bass")], json!([])))];
    open_chooser(&mut r, &dir);
    r.press_row("pack.olib");
    // An exported item is installed as the user's own by default.
    assert_eq!(rows(r.ui()).iter().map(|x| x.on).collect::<Vec<_>>()[..2], [true, false]);
    r.press_row("As a locked library");
    assert_eq!(rows(r.ui()).iter().map(|x| x.on).collect::<Vec<_>>()[..2], [false, true]);
    r.press_row("Into my own, to edit");
    // Untick two: one is asked for.
    r.press_row("Brass Corner");
    r.press_row("Warm Bass");
    r.e.calls.clear();
    r.press_row("Install");
    assert_eq!(r.e.called("library.install")[0]["items"], json!(["x1"]));
    assert_eq!(r.e.called("library.install")[0]["asLibrary"], false);
    // None ticked.
    open_chooser(&mut r, &dir);
    r.press_row("pack.olib");
    for name in ["Dawn Pad", "Brass Corner", "Warm Bass"] {
        r.press_row(name);
    }
    r.e.calls.clear();
    r.press_row("Install");
    assert!(r.e.calls.is_empty());
    assert_eq!(footer(r.ui()).0, "Choose at least one item.");
    r.press_row("Cancel");
    assert_eq!(r.c.path(), ["Library", "Performances"], "Cancel closes the choices, as the JUCE dialog's does");
    // The ones already here are counted.
    open_chooser(&mut r, &dir);
    r.press_row("pack.olib");
    r.press_row("Install");
    assert!(footer(r.ui()).0.starts_with("Installed 1, 2 already here."), "{}", footer(r.ui()).0);
}

// Code from an unknown source is said and the button says "Install anyway"; unsigned code too; a revoked key's is
// listed as not installed; a package of plugins offers no choice of how.
#[test]
fn code_from_an_unknown_source_is_warned_about() {
    let dir = folder("code");
    std::fs::write(dir.join("synth.olib"), b"x").unwrap();
    let code = json!([
        {"kind": "clap", "name": "Fuzz", "version": "1.2", "platform": "linux-arm64", "status": "unknown", "key": "aaaabbbbccccdddd", "keyName": "", "publicKey": "pub"},
        {"kind": "clap", "name": "Hiss", "version": "0.1", "platform": "macos-arm64", "status": "unsigned", "key": "", "keyName": "", "publicKey": ""},
        {"kind": "clap", "name": "Old", "version": "3", "platform": "linux-arm64", "status": "revoked", "key": "1111222233334444", "keyName": "Bob", "publicKey": "pub2"},
        {"kind": "clap", "name": "Fine", "version": "1", "platform": "linux-arm64", "status": "trusted", "key": "9999888877776666", "keyName": "Ada", "publicKey": "pub3"}]);
    let mut r = Rig::new(library());
    r.e.archives = vec![(dir.join("synth.olib").display().to_string(), archive("plugin", &[], code))];
    open_chooser(&mut r, &dir);
    r.press_row("synth.olib");
    assert_eq!(labels(r.ui()), ["Install anyway", "Cancel"], "a plugin package has no choice of how and no items");
    let lines = rows_text(&info(r.ui()));
    let text = lines.join(" ");
    assert!(text.contains("These plugins run code from an unknown source:"), "{text}");
    assert!(text.contains("• Fuzz 1.2 (linux-arm64), signed by unknown key aaaabbbbccccdddd"), "{text}");
    assert!(text.contains("• Hiss 0.1 (macos-arm64), unsigned"), "{text}");
    assert!(text.contains("Signed by a revoked key, so not installed: • Old 3 (linux-arm64)"), "{text}");
    assert!(text.contains("Old 3 (linux-arm64): Signed by Bob (1111 2222 3333 4444), which is revoked, not installed"), "{text}");
    assert!(!text.contains("• Fine"), "a trusted build is not warned about");
    assert_eq!(rows(r.ui()).len(), 2);
    r.e.calls.clear();
    r.press_row("Install anyway");
    assert_eq!(r.e.called("library.install"), [json!({"file": dir.join("synth.olib").display().to_string(), "asLibrary": false, "items": [], "acceptUnsignedCode": true})]);
}

// A refusal of the engine, and an archive that cannot be read, are said in a question with one answer.
#[test]
fn an_archive_that_cannot_be_read_is_said() {
    let dir = folder("unreadable");
    std::fs::write(dir.join("bad.olib"), b"x").unwrap();
    let mut r = Rig::new(library());
    open_chooser(&mut r, &dir);
    r.press_row("bad.olib");
    let (title, message, choices) = r.c.take_notice().expect("an alert");
    assert_eq!((title.as_str(), choices.as_slice()), ("Not installed", ["OK".to_string()].as_slice()));
    assert!(message.contains("no such archive"), "{message}");
    assert_eq!(r.c.noticed(Some(0)), Outcome::Stay);
    assert_eq!(r.c.path()[0], "Library");
}

// Export... of an item: the chooser asks for a name and a place, then the choices; the archive goes where it was said, a
// Performance with its edited parts' Sounds when asked, signed only when a key is chosen.
#[test]
fn an_item_is_exported_with_its_choices() {
    let dir = folder("export");
    let mut r = Rig::new(library());
    r.e.keys = vec![("mine".into(), "0011223344556677".into())];
    r.c.set_files_dir(dir.clone());
    options_for(&mut r, "Brass Corner");
    r.c.chosen(EXPORT);
    assert_eq!(labels(r.ui())[..3], ["‹ Up", "Name", "Save here"]);
    assert_eq!(value_of(r.ui(), "Name"), "Brass Corner ›");
    assert_eq!(r.press_row("Name"), Outcome::Prompt("The file's name".into(), "Brass Corner".into()));
    r.c.entered("Brass Export");
    r.e.calls.clear();
    r.press_row("Save here");
    assert_eq!(r.e.methods(), ["keys.list"], "the keys are listed for the signature");
    assert_eq!(labels(r.ui()), ["With edited parts' Sounds", "Signature", "Export", "Cancel"]);
    assert_eq!(value_of(r.ui(), "Signature"), "Not signed ›");
    assert!(info(r.ui()).join(" ").contains("Samples and the Sounds of unedited parts always go."));
    r.press_row("With edited parts' Sounds");
    assert_eq!(value_of(r.ui(), "With edited parts' Sounds"), "On");
    // Signed with the key.
    r.press_row("Signature");
    let (_, items) = r.c.take_menu().unwrap();
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Not signed", "Sign with mine", "Make a key..."]);
    r.c.chosen(pick(&items, &["Sign with mine"]));
    assert_eq!(value_of(r.ui(), "Signature"), "Sign with mine ›");
    r.e.calls.clear();
    r.press_row("Export");
    let file = dir.join("Brass Export.olib");
    assert_eq!(r.e.called("library.export"), [json!({"id": "p4", "file": file.display().to_string(), "editedSounds": true, "sign": {"key": "mine", "items": true, "archive": true}})]);
    assert_eq!(footer(r.ui()).0, "Exported. Signed with mine.");
    assert!(file.exists());
    assert_eq!(r.c.path(), ["Library", "Performances"]);
}

// A file that is there is asked about, and written over only once the new archive is written: a refused export keeps it.
#[test]
fn a_file_written_over_is_kept_until_the_new_one_is_written() {
    let dir = folder("overwrite");
    std::fs::write(dir.join("Brass Corner.olib"), "last week's").unwrap();
    let mut r = Rig::new(library());
    r.c.set_files_dir(dir.clone());
    options_for(&mut r, "Brass Corner");
    r.c.chosen(EXPORT);
    r.press_row("Save here");
    let (title, _, choices) = r.c.take_notice().expect("Replace?");
    assert_eq!((title.as_str(), choices), ("Replace Brass Corner.olib?", vec!["Replace".to_string(), "Cancel".to_string()]));
    r.c.noticed(None);
    assert_eq!(labels(r.ui())[0], "‹ Up", "Cancel leaves the chooser as it was");
    r.press_row("Save here");
    r.c.take_notice().unwrap();
    r.c.noticed(Some(0));
    r.e.calls.clear();
    r.press_row("Export");
    let written = r.e.called("library.export")[0]["file"].as_str().unwrap().to_string();
    assert!(written.contains(".oscairn-export"), "written beside the old one first: {written}");
    assert_eq!(std::fs::read_to_string(dir.join("Brass Corner.olib")).unwrap(), "an archive", "now it is over the old one");
    assert!(!dir.join(".oscairn-export").exists());
    // A refusal keeps the old archive.
    std::fs::write(dir.join("Brass Corner.olib"), "last week's").unwrap();
    r.e.fail.push(("library.export".into(), "No such signing key".into()));
    options_for(&mut r, "Brass Corner");
    r.c.chosen(EXPORT);
    r.press_row("Save here");
    r.c.take_notice().unwrap();
    r.c.noticed(Some(0));
    r.press_row("Export");
    assert_eq!(footer(r.ui()).0, "No such signing key");
    assert_eq!(std::fs::read_to_string(dir.join("Brass Corner.olib")).unwrap(), "last week's");
    assert!(!dir.join(".oscairn-export").exists());
}

// A key is made from the signature menu: its name asked for, `keys.make`, listed again and chosen; a name the engine
// refuses is said.
#[test]
fn a_signing_key_is_made_and_chosen() {
    let dir = folder("key");
    let mut r = Rig::new(library());
    r.c.set_files_dir(dir);
    options_for(&mut r, "Brass Corner");
    r.c.chosen(EXPORT);
    r.press_row("Save here");
    r.press_row("Signature");
    let (_, items) = r.c.take_menu().unwrap();
    let outcome = r.c.chosen(pick(&items, &["Make a key..."]));
    assert_eq!(outcome, Outcome::Prompt("Make a signing key: its name".into(), String::new()));
    r.e.calls.clear();
    r.c.entered("my key!");
    r.settle();
    assert_eq!(r.e.methods(), ["keys.make"]);
    let (title, message, _) = r.c.take_notice().expect("No key made");
    assert_eq!(title, "No key made");
    assert!(message.contains("letters, digits"), "{message}");
    r.c.noticed(Some(0));
    r.press_row("Signature");
    let (_, items) = r.c.take_menu().unwrap();
    r.c.chosen(pick(&items, &["Make a key..."]));
    r.e.calls.clear();
    r.c.entered("stage-key");
    r.settle();
    assert_eq!(r.e.methods(), ["keys.make", "keys.list"]);
    assert_eq!(value_of(r.ui(), "Signature"), "Sign with stage-key ›", "the new key is chosen");
}
