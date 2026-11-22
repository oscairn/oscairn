// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's second cut (ui/SETTINGS-APP.md R, K, B and A6): the package repositories, the signing keys, About and
//! installing a theme, against the fake engine of settings_fake.rs.

use super::library::{info, labels, rows, value_of};
use super::settings::{folder, installed_theme, wrapped, Rig};
use super::settings_fake::{Reads, Settings as Fake};
use super::*;
use crate::settings_app::Id;
use serde_json::Value;
use crate::settings_repos::{key_id, sha256};

const FRIEND: &str = "30fa782829102c1859d24993dc0349bebd65c09993f440a937213c2c50e81ed2";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// The key's ID is the first 16 hex digits of the SHA-256 of its 32 raw bytes: SHA-256 against its published vectors, and the
// ID against two keys the real engine named (an official repository's root and one `keys.make` made) (R6).
#[test]
fn a_keys_id_is_what_the_engine_says() {
    assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(hex(&sha256(&[b'a'; 1000])), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    assert_eq!(key_id("6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841"), "6a2dd1acf4d4e23c");
    assert_eq!(key_id(FRIEND), "89074b634c0de731");
    assert_eq!(key_id("not a key"), "");
}

fn repositories() -> Rig {
    let mut e = Fake::new();
    e.repos.push(super::settings_fake::Repo { address: "https://friend.example.org".into(), name: "A Friend's".into(), key: FRIEND.into(), origin: "user".into(), enabled: false });
    let mut r = Rig::new("repos", None, e);
    r.page(Id::Repositories);
    r
}

// Each repository is a row with its On switch, the address and the fingerprint of its root key on the panel with what it
// is; the Add row ends the list (R1).
#[test]
fn each_repository_is_a_row_with_its_fingerprint() {
    let mut r = repositories();
    assert_eq!(r.e.methods(), ["packages.repositories"]);
    assert_eq!(labels(r.ui()), ["Oscairn", "A Friend's", "Add a repository"]);
    assert_eq!((value_of(r.ui(), "Oscairn"), value_of(r.ui(), "A Friend's")), ("On".to_string(), "Off".to_string()));
    r.act(Action::Move { pane: None, by: 1 });
    assert_eq!(info(r.ui()).join(" "), "Oscairn https://packages.oscairn.org Root key 6a2d d1ac f4d4 e23c Oscairn's own: it can be turned off, never removed");
    r.act(Action::Move { pane: None, by: 1 });
    assert_eq!(info(r.ui()).join(" "), "A Friend's https://friend.example.org Root key 8907 4b63 4c0d e731 turned off: the Catalog reads nothing from it");
}

// A press turns a repository on or off; it is listed again as the engine kept it; a refusal is said under it (R2).
#[test]
fn a_repository_is_turned_on_and_off() {
    let mut r = repositories();
    r.e.calls.clear();
    r.press_row("A Friend's");
    assert_eq!(r.e.called("packages.enableRepository"), [json!({"address": "https://friend.example.org", "enabled": true})]);
    assert_eq!(r.e.methods(), ["packages.enableRepository", "packages.repositories"], "listed again");
    assert_eq!(value_of(r.ui(), "A Friend's"), "On");
    r.press_row("Oscairn");
    assert_eq!(r.e.called("packages.enableRepository").last().unwrap(), &json!({"address": "https://packages.oscairn.org", "enabled": false}), "the official one can be turned off");
    assert_eq!(value_of(r.ui(), "Oscairn"), "Off");
    r.e.fail = vec![("packages.enableRepository".into(), "the repository list is read-only".into())];
    r.press_row("A Friend's");
    assert_eq!(value_of(r.ui(), "A Friend's"), "On", "as the engine kept it");
    assert_eq!(r.danger_lines(), wrapped("Not changed: the repository list is read-only"));
}

// Remove is for a repository the user added, asked first in the reference's words; the official one is never offered (R3).
#[test]
fn a_repository_is_removed_after_the_question() {
    let mut r = repositories();
    r.act(Action::Move { pane: None, by: 1 });
    assert!(!r.c.bar_tools().iter().find(|t| t.label.starts_with("Remove")).unwrap().enabled, "never the official one");
    r.act(Action::Move { pane: None, by: 1 });
    assert!(r.c.bar_tools().iter().find(|t| t.label.starts_with("Remove")).unwrap().enabled);
    r.e.calls.clear();
    r.tool("Remove");
    let (title, message, choices) = r.notice.clone().unwrap();
    assert_eq!((title.as_str(), choices.as_slice()), ("Remove A Friend's?", ["Remove".to_string(), "Cancel".to_string()].as_slice()));
    assert_eq!(message, "What you installed from it stays. Its root key is no longer trusted for repositories.");
    r.c.noticed(Some(1));
    r.settle();
    assert!(r.e.calls.is_empty(), "Cancel removes nothing");
    r.tool("Remove");
    r.answer_notice("Remove");
    assert_eq!(r.e.called("packages.removeRepository"), [json!({"address": "https://friend.example.org"})]);
    assert_eq!(labels(r.ui()), ["Oscairn", "Add a repository"]);
    // A refusal is said.
    r.e.repos.push(super::settings_fake::Repo { address: "https://other.example.org".into(), name: "Other".into(), key: FRIEND.into(), origin: "user".into(), enabled: true });
    r.page(Id::Appearance);
    r.page(Id::Repositories);
    r.e.fail = vec![("packages.removeRepository".into(), "it is in use".into())];
    r.press_row("Other");
    r.e.fail.clear();
    r.act(Action::Move { pane: None, by: 0 });
    r.e.fail = vec![("packages.removeRepository".into(), "it is in use".into())];
    r.tool("Remove");
    r.answer_notice("Remove");
    assert_eq!(r.danger_lines(), wrapped("Not removed: it is in use"));
}

// Add takes an https address alone, in the reference's words; nothing is asked of the engine for another (R4).
#[test]
fn only_an_https_address_is_added() {
    let mut r = repositories();
    r.e.calls.clear();
    let o = r.tool("Add");
    assert_eq!(o, crate::settings_app::Outcome::Prompt("The repository's https:// address".into(), String::new()));
    r.type_text("http://plain.example.org");
    assert!(r.e.calls.is_empty());
    assert_eq!(r.danger_lines().join(" "), "Not an https address: a repository is added by its https:// address, such as https://packages.example.org. To install from a file, use the Library's Install... > File.");
    r.tool("Add");
    r.type_text("/home/me/a-folder");
    assert!(r.e.calls.is_empty());
    // Cancel on the keyboard asks nothing and says nothing.
    r.tool("Add");
    r.c.not_entered();
    r.settle();
    assert!(r.e.calls.is_empty() && r.danger_lines().is_empty() && r.said().is_empty(), "{:?}", r.said());
}

// A repository that reads is read first, by a job asked about until it ends, then added under the name its root claims
// (R5).
#[test]
fn a_repository_is_read_before_it_is_added() {
    let mut r = repositories();
    r.e.sources = vec![("https://new.example.org".into(), Reads::Done("New Repository".into(), "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".into()))];
    r.e.job_polls = 2;
    r.e.calls.clear();
    r.tool("Add");
    r.type_text("https://new.example.org");
    assert_eq!(r.e.methods(), ["packages.list", "packages.job"], "the job is asked about at once, then again later");
    assert!(rows(r.ui()).iter().all(|x| x.off), "every row is faint while a job is out");
    r.prompt = None;
    r.press_row("Add a repository");
    assert!(r.prompt.is_none() && r.e.methods().len() == 2, "and a press does nothing");
    assert_eq!(r.e.called("packages.list"), [json!({"source": "https://new.example.org"})]);
    assert_eq!(r.said(), "Reading https://new.example.org...");
    r.later_all();
    assert_eq!(r.e.methods(), ["packages.list", "packages.job", "packages.job", "packages.job", "packages.addRepository", "packages.repositories"]);
    assert_eq!(r.delayed.len(), 2, "asked about again every 100 ms: {:?}", r.delayed);
    assert!(r.delayed.iter().all(|d| d.0 == "packages.job" && d.1 == std::time::Duration::from_millis(100)));
    assert_eq!(r.e.called("packages.addRepository"), [json!({"address": "https://new.example.org", "key": "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841", "name": "New Repository"})]);
    assert!(labels(r.ui()).contains(&"New Repository".to_string()));
}

// A root key not trusted yet is offered by its fingerprint; Trust trusts it, the repository is read again and kept (R6, R7).
#[test]
fn a_root_key_is_trusted_only_when_the_player_says_so() {
    let mut r = repositories();
    r.e.sources = vec![("https://friendly.example.org".into(), Reads::Untrusted("Friendly".into(), FRIEND.into()))];
    r.e.job_polls = 0;
    r.e.trusted.retain(|k| k != FRIEND);
    r.tool("Add");
    r.type_text("https://friendly.example.org");
    r.later_all();
    let (title, message, choices) = r.notice.clone().unwrap();
    assert_eq!(title, "Trust this repository?");
    assert_eq!(message, "It calls itself \"Friendly\", signed by the root key whose fingerprint is 8907 4b63 4c0d e731. Trust it only if its publisher gave you that fingerprint.");
    assert_eq!(choices, ["Trust", "Cancel"]);
    assert!(!r.e.trusted.contains(&FRIEND.to_string()), "nothing is trusted before the answer");
    r.e.calls.clear();
    r.answer_notice("Cancel");
    assert!(r.e.called("packages.addRepository").is_empty() && !r.e.trusted.contains(&FRIEND.to_string()));
    assert_eq!(r.danger_lines(), wrapped("Not added: its root key was not trusted."));
    // Again, and Trust.
    r.tool("Add");
    r.type_text("https://friendly.example.org");
    r.later_all();
    r.e.calls.clear();
    r.answer_notice("Trust");
    r.later_all();
    assert_eq!(r.e.methods(), ["packages.addRepository", "packages.list", "packages.job", "packages.addRepository", "packages.repositories"], "trusted with the repository, read again, kept under its name");
    assert!(r.e.trusted.contains(&FRIEND.to_string()));
    assert_eq!(labels(r.ui()), ["Oscairn", "A Friend's", "Friendly", "Add a repository"]);
}

// A repository that does not verify even with its key trusted is taken out again, key and all; any other failure is
// said as it is (R7, R8).
#[test]
fn a_repository_that_does_not_read_is_not_kept() {
    let mut r = repositories();
    r.e.job_polls = 0;
    r.e.sources = vec![("https://broken.example.org".into(), Reads::Fails("its index key is not signed by its root key".into()))];
    r.tool("Add");
    r.type_text("https://broken.example.org");
    r.later_all();
    assert_eq!(r.danger_lines(), wrapped("Not read: its index key is not signed by its root key"));
    assert!(r.e.called("packages.addRepository").is_empty());
    // Unreachable: the engine's own words.
    r.tool("Add");
    r.type_text("https://nowhere.example.org");
    r.later_all();
    assert!(r.danger_lines().join(" ").starts_with("Not read: not a usable repository: cannot fetch repository.json"));
    // Another listing is running: the engine says so, and it is said.
    r.e.fail.clear();
    r.tool("Add");
    r.type_text("https://third.example.org");
    r.later_all();
    // The key trusted, and it still does not verify: removed.
    let mut r = repositories();
    r.e.job_polls = 0;
    r.e.trusted.retain(|k| k != FRIEND);
    r.e.sources = vec![("https://half.example.org".into(), Reads::Untrusted("Half".into(), FRIEND.into()))];
    r.tool("Add");
    r.type_text("https://half.example.org");
    r.later_all();
    r.e.sources = vec![("https://half.example.org".into(), Reads::Fails("its root's signature does not verify (abcd)".into()))];
    r.answer_notice("Trust");
    r.later_all();
    assert_eq!(r.e.called("packages.removeRepository"), [json!({"address": "https://half.example.org"})]);
    assert!(!labels(r.ui()).contains(&"Half".to_string()));
    assert_eq!(r.danger_lines(), wrapped("Not read: its root's signature does not verify (abcd)"));
}

// ---- K: Signing keys ----

fn keys() -> Rig {
    let mut r = Rig::desktop("keys");
    r.e.keys = vec![("Stage".into(), FRIEND.into())];
    r.page(Id::Keys);
    r
}

// ---- P: Installed ----

fn installed() -> Rig {
    let mut e = Fake::new();
    e.installed_packages = vec![
        json!({"id": "org.example.reverb", "version": "1.0.0", "kind": "plugin", "repository": "https://packages.oscairn.org", "sha256": "", "signers": ["6a2dd1acf4d4e23c"], "otherBuild": false, "withdrawn": null, "dependency": false}),
        json!({"id": "org.example.strings", "version": "2.1.0", "kind": "asset", "repository": "", "sha256": "", "signers": [], "otherBuild": false, "withdrawn": null, "dependency": false}),
        json!({"id": "org.example.a.WonderfullyLongPackageIdentifierForSomeSoundLibrary", "version": "0.9.0", "kind": "library", "repository": "https://friend.example.org", "sha256": "", "signers": ["89074b634c0de731"], "otherBuild": false, "withdrawn": null, "dependency": true}),
    ];
    e.libraries = vec![("lib-1".into(), "Wicked Synths".into(), 12), ("lib-2".into(), "Field Recordings".into(), 34)];
    let mut r = Rig::new("installed", None, e);
    r.page(Id::Installed);
    r
}

// The packages and the libraries, each a row under its section's heading, by name and version (the packages) or name
// and item count (the libraries); a long name is the row's to fit, whole on the panel, which also
// says the publisher (the repository it came from) (P1).
#[test]
fn the_installed_lists_packages_and_libraries() {
    let mut r = installed();
    assert_eq!(r.e.methods(), ["packages.installed", "library.libraries"]);
    let all = labels(r.ui());
    assert_eq!(all, ["Packages", "org.example.reverb", "org.example.strings", "org.example.a.WonderfullyLongPackageIdentifierForSomeSoundLibrary", "Libraries", "Wicked Synths", "Field Recordings"]);
    assert_eq!((value_of(r.ui(), "org.example.reverb"), value_of(r.ui(), "Wicked Synths"), value_of(r.ui(), "Field Recordings")), ("1.0.0".to_string(), "12 items".to_string(), "34 items".to_string()));
    r.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor on the packages' heading
    r.act(Action::Move { pane: None, by: 3 });
    let panel = info(r.ui());
    assert!(panel.contains(&"org.example.a.WonderfullyLongPackageIdentifierForSomeSoundLibrary".to_string()), "{panel:?}");
    assert!(panel.join(" ").contains("Publisher") && panel.join(" ").contains("https://friend.example.org"), "the publisher is the repository it came from");
    r.act(Action::Move { pane: None, by: 3 });
    let panel = info(r.ui());
    assert!(panel.contains(&"Field Recordings".to_string()) && panel.join(" ").contains("34"), "{panel:?}");
}

// Every row is reached by the cursor and the panel; nothing asks, and nothing is asked again, at rest (P1).
#[test]
fn every_installed_row_is_reached_and_nothing_at_rest() {
    let mut r = installed();
    assert!(r.delayed.is_empty(), "nothing is asked again: {:?}", r.delayed);
    for _ in 0..7 {
        r.act(Action::Move { pane: None, by: 1 });
    }
    assert!(info(r.ui()).contains(&"Field Recordings".to_string()), "{:?}", info(r.ui()));
    r.e.calls.clear();
    for _ in 0..4 {
        r.act(Action::Move { pane: None, by: -1 });
    }
    assert!(r.e.calls.is_empty(), "moving the cursor asks nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
}

// A request refused, or a lost connection, is the app's usual failed row; what was read of the other side stays (P1).
#[test]
fn a_refused_installed_request_is_a_failed_row() {
    let mut r = installed();
    r.e.fail = vec![("packages.installed".into(), "the package store is not readable".into())];
    r.page(Id::Appearance);
    r.page(Id::Installed);
    assert!(info(r.ui()).contains(&"The packages could not be read".to_string()), "{:?}", info(r.ui()));
    assert!(r.danger_lines().contains(&"the package store is not readable".to_string()), "{:?}", r.danger_lines());
    assert!(labels(r.ui()).contains(&"Wicked Synths".to_string()), "the libraries were read");
    r.e.fail = vec![("library.libraries".into(), "lost the connection to the engine".into())];
    r.page(Id::Appearance);
    r.page(Id::Installed);
    assert!(info(r.ui()).contains(&"The libraries could not be read".to_string()), "{:?}", info(r.ui()));
    assert!(r.danger_lines().contains(&"lost the connection to the engine".to_string()), "{:?}", r.danger_lines());
    assert!(labels(r.ui()).contains(&"org.example.reverb".to_string()), "what read before stays");
}

// Remove is for a library, asked first as for a repository (P2): the question, one `library.uninstall` with the
// library's id, the list read again; a package row is never offered, and every row is faint while it runs.
#[test]
fn a_library_is_removed_after_the_question() {
    let mut r = installed();
    r.act(Action::Move { pane: None, by: 1 }); // the packages' heading
    r.act(Action::Move { pane: None, by: 1 }); // org.example.reverb, a package
    let remove = r.c.bar_tools().into_iter().find(|t| t.label.starts_with("Remove")).unwrap();
    assert!(!remove.enabled, "a package row is not offered Remove");
    r.act(Action::Move { pane: None, by: 4 }); // Wicked Synths
    let remove = r.c.bar_tools().into_iter().find(|t| t.label.starts_with("Remove")).unwrap();
    assert!(remove.enabled);
    r.e.calls.clear();
    r.tool("Remove");
    let (title, message, choices) = r.notice.clone().unwrap();
    assert_eq!((title.as_str(), choices.as_slice()), ("Remove Wicked Synths?", ["Remove".to_string(), "Cancel".to_string()].as_slice()));
    assert_eq!(message, "Its items are removed from this device with it.");
    // Confirmed before the engine answers: the row says so, every row faint.
    r.c.noticed(Some(0));
    assert_eq!(r.said(), "Removing Wicked Synths...");
    assert!(rows(r.ui()).iter().all(|x| x.off), "every row is faint while it runs");
    r.settle();
    assert_eq!(r.e.called("library.uninstall"), [json!({"library": "lib-1"})]);
    assert_eq!(r.e.methods(), ["library.uninstall", "packages.installed", "library.libraries"], "one uninstall, then the lists read again");
    assert!(!labels(r.ui()).contains(&"Wicked Synths".to_string()), "the list no longer has it");
    assert!(rows(r.ui()).iter().all(|x| !x.off), "the rows are back");
    assert!(r.delayed.is_empty(), "a library that is gone is not read for again: {:?}", r.delayed);
}

// The handler answers `accepted` and the list changes a moment later: the list is read again, soon, until the library
// is gone, every row faint meanwhile, and not at all after (P2).
#[test]
fn an_accepted_removal_is_followed_until_the_library_is_gone() {
    let mut r = installed();
    r.e.uninstall_lag = 3;
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: 5 }); // Wicked Synths
    r.e.calls.clear();
    r.tool("Remove");
    r.answer_notice("Remove");
    assert_eq!(r.e.methods(), ["library.uninstall", "packages.installed", "library.libraries"]);
    assert_eq!(r.delayed, [("library.libraries".to_string(), std::time::Duration::from_millis(200))], "still listed: read again later, not at once");
    assert_eq!(value_of(r.ui(), "Wicked Synths"), "12 items", "accepted is not gone");
    assert!(rows(r.ui()).iter().all(|x| x.off), "every row is faint until it is gone");
    r.later_all();
    assert_eq!(r.e.called("library.libraries").len(), 4, "three stale reads, then the one without it");
    assert!(!labels(r.ui()).contains(&"Wicked Synths".to_string()));
    assert!(rows(r.ui()).iter().all(|x| !x.off));
    assert_eq!(r.delayed.len(), 3, "one later read for each stale answer and none after: {:?}", r.delayed);
}

// Cancel leaves the library: nothing is asked, nothing read again, the row stays.
#[test]
fn cancelling_a_libraries_remove_asks_nothing() {
    let mut r = installed();
    r.act(Action::Move { pane: None, by: 1 }); // the packages' heading
    r.act(Action::Move { pane: None, by: 5 }); // Wicked Synths
    r.e.calls.clear();
    r.tool("Remove");
    r.answer_notice("Cancel");
    assert!(r.e.calls.is_empty(), "Cancel sends nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
    assert_eq!(labels(r.ui()), ["Packages", "org.example.reverb", "org.example.strings", "org.example.a.WonderfullyLongPackageIdentifierForSomeSoundLibrary", "Libraries", "Wicked Synths", "Field Recordings"]);
    assert_eq!(value_of(r.ui(), "Wicked Synths"), "12 items", "the row stays");
}

// A removal the engine refuses says the reason and the row stays; a refusal is not a change, so the list is not read
// again (P2).
#[test]
fn a_refused_library_uninstall_says_why_and_the_row_stays() {
    let mut r = installed();
    r.act(Action::Move { pane: None, by: 1 }); // the packages' heading
    r.act(Action::Move { pane: None, by: 5 }); // Wicked Synths
    r.e.fail = vec![("library.uninstall".into(), "it is in use".into())];
    r.e.calls.clear();
    r.tool("Remove");
    r.answer_notice("Remove");
    assert_eq!(r.e.methods(), ["library.uninstall"], "a refusal is not followed by a re-read");
    assert!(r.danger_lines().contains(&"it is in use".to_string()), "{:?}", r.danger_lines());
    assert_eq!(value_of(r.ui(), "Wicked Synths"), "12 items", "the row stays");
}

// A package row confirmed opens its detail: id, version, the publisher (the repository it came from), the key
// fingerprint of what signed it, as the repositories page shows a root key's; the panel is
// the detail until Back, reaching a package by the cursor and a press, and nothing is asked (P3).
#[test]
fn a_packages_detail_shows_what_the_engine_reports() {
    let mut r = installed();
    let base = r.e.calls.len();
    r.press_index(1); // org.example.reverb
    assert_eq!(r.e.calls.len(), base, "a press asks nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
    assert_eq!(r.c.path(), ["Settings", "Installed", "org.example.reverb"], "the bar names the package");
    let panel = info(r.ui());
    assert!(panel.contains(&"org.example.reverb".to_string()), "{panel:?}");
    assert!(panel.contains(&"1.0.0".to_string()), "{panel:?}");
    assert!(panel.contains(&"https://packages.oscairn.org".to_string()), "the publisher is the repository it came from: {panel:?}");
    assert!(panel.contains(&"Key fingerprint".to_string()) && panel.contains(&"6a2d d1ac f4d4 e23c".to_string()), "the key that signed it, in the reference's grouping: {panel:?}");
    assert!(!panel.join(" ").contains("Trusted"), "the engine reports no trust flag: {panel:?}");
    // A file's package, whose source is unknown: no fingerprint.
    r.press_index(2); // org.example.strings
    assert_eq!(r.c.path(), ["Settings", "Installed", "org.example.strings"], "the detail is the package confirmed");
    let panel = info(r.ui()).join(" ");
    assert!(panel.contains("org.example.strings") && panel.contains("installed from a file"), "{panel}");
    assert!(!panel.contains("Key fingerprint"), "{panel}");
    assert_eq!(r.e.calls.len(), base, "another press asks nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
}

// Back closes a package's detail and the list shows again with the cursor on the same row; nothing is asked (P3).
#[test]
fn back_closes_a_packages_detail_on_the_same_row() {
    let mut r = installed();
    r.press_index(1); // org.example.reverb
    assert_eq!(r.c.path(), ["Settings", "Installed", "org.example.reverb"]);
    let base = r.e.calls.len();
    let o = r.c.back(); // the hardware Back, as the shell's go_back reaches the screen
    assert_eq!(o, crate::settings_app::Outcome::Stay, "Back closes the detail, not the page: {o:?}");
    r.settle();
    assert_eq!(r.c.path(), ["Settings", "Installed"]);
    assert_eq!(rows(r.ui())[1].label, "org.example.reverb", "the list shows again");
    assert!(rows(r.ui())[1].cursor, "the cursor is back on the same row");
    assert!(!info(r.ui()).join(" ").contains("Key fingerprint"), "the detail is closed: {:?}", info(r.ui()));
    assert_eq!(r.e.calls.len(), base, "back asks nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
}

// The keys are listed by name with their fingerprint; the row to make one ends the list; with none the page says so (K1).
#[test]
fn the_keys_are_listed_by_fingerprint() {
    let mut r = keys();
    assert_eq!(labels(r.ui()), ["Stage", "Make a signing key"]);
    assert_eq!(value_of(r.ui(), "Stage"), "8907 4b63 4c0d e731");
    let mut e = Rig::desktop("keys-none");
    e.page(Id::Keys);
    assert_eq!(labels(e.ui()), ["Signing keys", "Make a signing key"]);
    assert_eq!(value_of(e.ui(), "Signing keys"), "No keys yet");
    r.e.fail = vec![("keys.list".into(), "no key store".into())];
    r.page(Id::About);
    r.page(Id::Keys);
    assert_eq!(r.danger_lines(), wrapped("no key store"));
    assert!(info(r.ui()).contains(&"The keys could not be read".to_string()));
}

// Make asks for a name on the keyboard and makes the key; a name the engine refuses comes back as a question named "No
// key made" (K2, K3).
#[test]
fn a_key_is_made_from_a_name() {
    let mut r = keys();
    r.e.calls.clear();
    let o = r.press_row("Make a signing key");
    assert_eq!(o, crate::settings_app::Outcome::Prompt("Name of the new signing key".into(), String::new()));
    r.type_text("  Home-Studio_2  ");
    assert_eq!(r.e.methods(), ["keys.make", "keys.list"]);
    assert_eq!(r.e.called("keys.make"), [json!({"name": "Home-Studio_2"})]);
    assert_eq!(labels(r.ui()), ["Stage", "Home-Studio_2", "Make a signing key"]);
    assert_eq!(value_of(r.ui(), "Home-Studio_2").len(), 19, "16 digits in four groups");
    r.press_row("Make a signing key");
    r.type_text("bad name");
    let (title, message, choices) = r.notice.take().unwrap();
    assert_eq!((title.as_str(), choices.as_slice()), ("No key made", ["OK".to_string()].as_slice()));
    assert_eq!(message, "a key's name is letters, digits, '.', '-' and '_', at most 64, not starting with '.'");
    r.press_row("Make a signing key");
    r.type_text("Stage");
    assert_eq!(r.notice.take().unwrap().1, "there is a key named Stage already");
    // Cancelling the keyboard asks nothing.
    let n = r.e.calls.len();
    r.press_row("Make a signing key");
    r.c.not_entered();
    r.settle();
    assert_eq!(r.e.calls.len(), n);
}

// ---- B: About ----

// About says Oscairn's version, its source, the engine's version once read, and the license, in the reference's words (B1 to B4).
#[test]
fn about_says_what_this_is() {
    let mut r = Rig::desktop("about");
    r.page(Id::About);
    assert_eq!(r.e.methods(), ["packages.preinstalled"]);
    assert_eq!(labels(r.ui()), ["Oscairn", "Source", "Engine", "License"]);
    // (CMake gives the build the release it is made for: ctest's run is built with it; a plain cargo build has none.)
    assert_eq!(value_of(r.ui(), "Oscairn"), option_env!("OSCAIRN_RELEASE").filter(|v| !v.is_empty()).unwrap_or(env!("CARGO_PKG_VERSION")), "the release the build was given, else its crate's");
    if option_env!("OSCAIRN_SOURCE_COMMIT").is_none_or(str::is_empty) {
        assert_eq!(value_of(r.ui(), "Source"), "Development build");
    }
    assert_eq!(value_of(r.ui(), "Engine"), "0.1.0-alpha.1");
    assert_eq!(value_of(r.ui(), "License"), "AGPL-3.0-or-later");
    let text = info(r.ui()).join(" ");
    assert!(text.contains("Built from a working copy, not a release's commit.") && text.contains("The audio engine this app is connected to.") && text.contains("GNU Affero General Public License") && text.contains("LICENSE and CREDITS.md in the source"), "{text}");
    // A release's: its commit, twelve characters, and the address of the tree at it.
    let about = crate::settings_keys::About::with("0.2.0", "0123456789abcdef0123");
    use crate::settings_app::Page;
    assert_eq!(about.rows()[1].value, "0123456789ab");
    assert!(about.info(Some(1)).iter().any(|l| l.text == "https://github.com/oscairn/oscairn/tree/0123456789abcdef0123"));
    // The engine cannot be asked.
    r.e.fail = vec![("packages.preinstalled".into(), "the packages are not readable".into())];
    r.page(Id::Appearance);
    r.page(Id::About);
    assert_eq!(value_of(r.ui(), "Engine"), "Unknown");
    assert!(r.danger_lines().join(" ").contains("The engine's version could not be read: the packages"), "{:?}", r.danger_lines());
    assert!(!rows(r.ui()).is_empty());
}

// ---- A6: installing a theme ----

fn source_of(name: &str, id: &str, change: impl FnOnce(&mut Value)) -> std::path::PathBuf {
    let dir = folder(name);
    installed_theme(&dir, id, "Mine", change);
    dir.join("themes").join(id)
}

// A theme is installed from the folder of its theme.json: read and checked, its files copied under the installed themes,
// then chosen and shown (A6).
#[test]
fn a_theme_is_installed_from_its_folder() {
    let mut r = Rig::desktop("install");
    let source = source_of("install-source", "evening-glass", |d| d["radius"] = json!(12));
    r.c.set_files_dir(source.clone());
    let o = r.tool("Install");
    assert_eq!(o, crate::settings_app::Outcome::Stay, "a chooser, not the keyboard");
    assert_eq!(r.c.path(), ["Settings", "Appearance", "Install a theme"]);
    assert_eq!(labels(r.ui()), ["‹ Up", "fonts", "theme.json"]);
    r.press_row("theme.json");
    assert_eq!(r.c.path(), ["Settings", "Appearance"], "the chooser is closed");
    let kept = r.config.join("themes/evening-glass");
    assert!(kept.join("theme.json").is_file() && kept.join("fonts/Inter-Regular.ttf").is_file() && kept.join("fonts/OFL.txt").is_file());
    assert_eq!(std::fs::read_to_string(r.config.join("appearance.txt")).unwrap(), "evening-glass", "chosen as it is installed");
    assert_eq!(r.ui().global::<crate::Theme>().get_id().as_str(), "evening-glass");
    assert_eq!(value_of(r.ui(), "Mine"), "In use");
    assert!(std::fs::read_dir(r.config.join("themes")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')), "no staging folder is left");
    // Installed again, identical: no change and no refusal; different under the same id: refused.
    r.c.choose_file(&source.join("theme.json"));
    r.settle();
    assert!(r.danger_lines().is_empty(), "{:?}", r.danger_lines());
    let other = source_of("install-other", "evening-glass", |d| d["radius"] = json!(3));
    r.c.choose_file(&other.join("theme.json"));
    r.settle();
    assert_eq!(r.danger_lines(), wrapped("That theme ID is already installed; use a new ID for a different theme"));
}

// What cannot be installed is refused with the reference's words: a built-in's id, a theme that does not load, a file that is
// not a theme.json (A6).
#[test]
fn a_theme_that_cannot_be_installed_is_refused() {
    let mut r = Rig::desktop("install-refused");
    let built_in = source_of("install-reserved", "oscairn-dark", |_| {});
    r.c.choose_file(&built_in.join("theme.json"));
    r.settle();
    assert_eq!(r.danger_lines(), wrapped("Built-in theme IDs are reserved"));
    let broken = source_of("install-broken", "broken-one", |_| {});
    std::fs::remove_file(broken.join("fonts/OFL.txt")).unwrap();
    r.c.choose_file(&broken.join("theme.json"));
    r.settle();
    assert_eq!(r.danger_lines(), wrapped("Theme file is missing"));
    assert!(!r.config.join("themes/broken-one").exists());
    r.c.choose_file(&broken.join("fonts/Inter-Regular.ttf"));
    r.settle();
    assert_eq!(r.danger_lines(), wrapped("Choose a theme folder or its theme.json"));
}
