// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's second cut (ui/SETTINGS-APP.md R, K, B and A6): the package repositories, the signing keys, About and
//! installing a theme, against the fake engine of settings_fake.rs.

use super::library::{info, labels, rows, value_of};
use super::settings::{folder, installed_theme, wrapped, Rig};
use super::settings_fake::{Reads, Repo, Settings as Fake};
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

fn package(id: &str, name: &str, version: &str, kind: &str, repository: &str, signers: &[&str]) -> Value {
    json!({"id": id, "version": version, "kind": kind, "repository": repository, "sha256": "", "signers": signers, "otherBuild": false, "withdrawn": null, "dependency": false, "name": name})
}

/// A plugin package, an asset package, a library with no name of its own, a library both requests list and one only
/// `library.libraries` does. The reverb is used and has a dependency nothing else uses.
fn installed() -> Rig {
    let mut e = Fake::new();
    e.installed_packages = vec![
        package("org.example.reverb", "Example Reverb", "1.0.0", "plugin", "https://packages.oscairn.org", &["6a2dd1acf4d4e23c"]),
        package("lib-1", "Wicked Synths", "", "library", "", &[]),
        package("org.example.strings", "example strings", "2.1.0", "asset", "", &[]),
        package("org.example.a.WonderfullyLongPackageIdentifierForSomeSoundLibrary", "", "0.9.0", "library", "https://friend.example.org", &["89074b634c0de731"]),
    ];
    e.libraries = vec![("lib-1".into(), "Wicked Synths".into(), 12), ("lib-2".into(), "Field Recordings".into(), 34)];
    e.removal = vec![(
        "org.example.reverb".into(),
        vec![
            json!({"kind": "library", "id": "lib-1", "name": "Wicked Synths", "version": "", "items": 87}),
            json!({"kind": "own", "id": "", "name": "", "version": "", "items": 3, "examples": ["Keys Split", "Pad", "Lead"]}),
        ],
        vec![json!({"id": "org.example.strings", "version": "2.1.0", "kind": "asset", "name": "example strings", "bytes": 1258291})],
    )];
    let mut r = Rig::new("installed", None, e);
    r.page(Id::Installed);
    r
}

const LONG: &str = "org.example.a.WonderfullyLongPackageIdentifierForSomeSoundLibrary";
const ALL: [&str; 5] = ["Example Reverb", "example strings", "Field Recordings", LONG, "Wicked Synths"];

// One list of everything installed, by name whatever the case, no heading rows: each row its name, the word for its
// kind and its version; the count is of items; a long name is the row's to fit, whole on the panel, which also says
// the publisher (the repository it came from) (P1).
#[test]
fn the_installed_is_one_list_by_name() {
    let mut r = installed();
    assert_eq!(r.e.methods(), ["packages.installed", "library.libraries"]);
    assert_eq!(labels(r.ui()), ALL, "a library only library.libraries lists is a row too, and one with no name shows its id");
    let values: Vec<String> = ALL.iter().map(|l| value_of(r.ui(), l)).collect();
    assert_eq!(values, ["Plugin 1.0.0", "Samples 2.1.0", "Library", "Library 0.9.0", "Library"]);
    assert_eq!(r.ui().global::<crate::PartState>().get_total(), 5, "the count is of items");
    r.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor on the first row
    r.act(Action::Move { pane: None, by: 3 });
    let panel = info(r.ui());
    assert!(panel.join("").contains(LONG), "the whole name, wrapped: {panel:?}");
    assert!(panel.join(" ").contains("Publisher") && panel.join(" ").contains("https://friend.example.org"), "the publisher is the repository it came from");
    r.act(Action::Move { pane: None, by: 1 });
    let panel = info(r.ui());
    assert!(panel.contains(&"Wicked Synths".to_string()) && panel.contains(&"12".to_string()) && panel.contains(&"lib-1".to_string()), "a library package has its item count: {panel:?}");
    // Two of one name are told apart by id.
    r.e.installed_packages.push(package("a.first", "wicked synths", "", "library", "", &[]));
    r.page(Id::Appearance);
    r.page(Id::Installed);
    assert_eq!(labels(r.ui())[4..], ["wicked synths", "Wicked Synths"], "a.first before lib-1");
}

// Every row is reached by the cursor and the panel; nothing asks, and nothing is asked again, at rest (P1).
#[test]
fn every_installed_row_is_reached_and_nothing_at_rest() {
    let mut r = installed();
    assert!(r.delayed.is_empty(), "nothing is asked again: {:?}", r.delayed);
    for _ in 0..5 {
        r.act(Action::Move { pane: None, by: 1 });
    }
    assert!(info(r.ui()).contains(&"Wicked Synths".to_string()), "{:?}", info(r.ui()));
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
    assert!(labels(r.ui()).contains(&"Field Recordings".to_string()), "the libraries were read");
    r.e.fail = vec![("library.libraries".into(), "lost the connection to the engine".into())];
    r.page(Id::Appearance);
    r.page(Id::Installed);
    assert!(info(r.ui()).contains(&"The libraries could not be read".to_string()), "{:?}", info(r.ui()));
    assert!(r.danger_lines().contains(&"lost the connection to the engine".to_string()), "{:?}", r.danger_lines());
    assert!(labels(r.ui()).contains(&"Example Reverb".to_string()), "what read before stays");
}

// Remove works on every row (P2): `packages.removal` is asked first; nothing uses this one and it has no
// dependencies, so the plain question, then one `packages.uninstall` and the lists read again; every row is faint
// while it runs.
#[test]
fn a_package_nothing_uses_is_removed_after_the_question() {
    let mut r = installed();
    for at in 0..5 {
        r.act(Action::Move { pane: None, by: 1 });
        assert!(r.c.bar_tools().into_iter().find(|t| t.label.starts_with("Remove")).unwrap().enabled, "row {at} is offered Remove");
    }
    r.e.calls.clear(); // the cursor is on Wicked Synths
    r.tool("Remove");
    assert_eq!(r.e.called("packages.removal"), [json!({"id": "lib-1"})]);
    let (title, message, choices) = r.notice.clone().unwrap();
    assert_eq!((title.as_str(), choices.as_slice()), ("Remove Wicked Synths?", ["Remove".to_string(), "Cancel".to_string()].as_slice()));
    assert_eq!(message, "It is removed from this device.");
    // Confirmed before the engine answers: the row says so, every row faint.
    r.notice = None;
    r.c.noticed(Some(0));
    assert_eq!(r.said(), "Removing Wicked Synths...");
    assert!(rows(r.ui()).iter().all(|x| x.off), "every row is faint while it runs");
    r.settle();
    assert!(r.notice.is_none(), "no dependencies, no second question");
    assert_eq!(r.e.called("packages.uninstall"), [json!({"id": "lib-1"})]);
    assert_eq!(r.e.methods(), ["packages.removal", "packages.uninstall", "packages.installed", "library.libraries"], "one uninstall, then the lists read again");
    assert_eq!(labels(r.ui()), ALL[..4], "the list no longer has it");
    assert!(rows(r.ui()).iter().all(|x| !x.off), "the rows are back");
    assert!(r.delayed.is_empty(), "a package that is gone is not read for again: {:?}", r.delayed);
}

// What uses a package is named in the question, with Cancel and Remove anyway; then its dependencies nothing else
// uses are listed with their sizes, to keep or remove too; then one `packages.uninstall` says all of it (P2).
#[test]
fn a_used_package_names_what_uses_it_and_offers_its_unused_dependencies() {
    let mut r = installed();
    r.act(Action::Move { pane: None, by: 1 }); // Example Reverb, a plugin package
    r.e.calls.clear();
    r.tool("Remove");
    assert_eq!(r.e.called("packages.removal"), [json!({"id": "org.example.reverb", "version": "1.0.0"})], "a plugin package by its version");
    let (title, message, choices) = r.notice.clone().unwrap();
    assert_eq!((title.as_str(), choices.as_slice()), ("Remove Example Reverb?", ["Cancel".to_string(), "Remove anyway".to_string()].as_slice()));
    assert_eq!(message, "Used by: Wicked Synths (87 items), 3 of your own. What uses it will not work until it is installed again.");
    r.answer_notice("Remove anyway");
    assert_eq!(r.e.methods(), ["packages.removal"], "nothing is removed before the second answer");
    let (title, message, choices) = r.notice.clone().unwrap();
    assert_eq!(title, "Also remove what nothing else uses?");
    assert_eq!(message, "example strings (1.2 MB)");
    assert_eq!(choices, ["Keep them", "Remove them too", "Cancel"]);
    r.answer_notice("Remove them too");
    assert_eq!(r.e.called("packages.uninstall"), [json!({"id": "org.example.reverb", "version": "1.0.0", "evenIfUsed": true, "withUnused": ["org.example.strings"]})]);
    assert_eq!(labels(r.ui()), ["Field Recordings", LONG, "Wicked Synths"], "both are gone");
    // Keep them: the package alone.
    let mut r = installed();
    r.act(Action::Move { pane: None, by: 1 });
    r.tool("Remove");
    r.answer_notice("Remove anyway");
    r.answer_notice("Keep them");
    assert_eq!(r.e.called("packages.uninstall"), [json!({"id": "org.example.reverb", "version": "1.0.0", "evenIfUsed": true})]);
    assert_eq!(labels(r.ui()), ALL[1..], "its dependency stays");
}

// What `packages.uninstall` removed may show a moment longer: the lists are read again, soon, until it is gone, every
// row faint meanwhile, and not at all after (P2).
#[test]
fn a_removal_is_followed_until_the_package_is_gone() {
    let mut r = installed();
    r.e.uninstall_lag = 3;
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: 4 }); // Wicked Synths
    r.e.calls.clear();
    r.tool("Remove");
    r.answer_notice("Remove");
    assert_eq!(r.e.methods(), ["packages.removal", "packages.uninstall", "packages.installed", "library.libraries"]);
    let later = std::time::Duration::from_millis(200);
    assert_eq!(r.delayed, [("packages.installed".to_string(), later), ("library.libraries".to_string(), later)], "still listed: read again later, not at once");
    assert!(labels(r.ui()).contains(&"Wicked Synths".to_string()), "removed is not gone");
    assert!(rows(r.ui()).iter().all(|x| x.off), "every row is faint until it is gone");
    r.later_all();
    assert_eq!(r.e.called("packages.installed").len(), 4, "three stale reads, then the one without it");
    assert!(!labels(r.ui()).contains(&"Wicked Synths".to_string()));
    assert!(rows(r.ui()).iter().all(|x| !x.off));
    assert_eq!(r.delayed.len(), 6, "a later read of both lists for each stale answer and none after: {:?}", r.delayed);
}

// Cancel at either question leaves everything: nothing is asked but what removing it means, the rows stay.
#[test]
fn cancelling_a_remove_removes_nothing() {
    let mut r = installed();
    r.act(Action::Move { pane: None, by: 1 }); // Example Reverb
    r.e.calls.clear();
    r.tool("Remove");
    r.answer_notice("Cancel");
    assert!(r.notice.is_none(), "no second question after Cancel");
    r.tool("Remove");
    r.answer_notice("Remove anyway");
    r.answer_notice("Cancel");
    assert_eq!(r.e.methods(), ["packages.removal", "packages.removal"], "Cancel removes nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
    assert_eq!(labels(r.ui()), ALL);
    assert!(rows(r.ui()).iter().all(|x| !x.off), "the rows are not left faint");
}

// A removal the engine refuses says the reason in its words and the row stays; a refusal is not a change, so the list
// is not read again (P2). So does a `packages.removal` it cannot answer, with no question asked.
#[test]
fn a_refused_removal_says_why_and_the_row_stays() {
    let mut r = installed();
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: 4 }); // Wicked Synths
    r.e.fail = vec![("packages.uninstall".into(), "the open Performance uses Wicked Synths; remove it from the Performance or open another first".into())];
    r.e.calls.clear();
    r.tool("Remove");
    r.answer_notice("Remove");
    assert_eq!(r.e.methods(), ["packages.removal", "packages.uninstall"], "a refusal is not followed by a re-read");
    assert!(r.danger_lines().join(" ").contains("the open Performance uses Wicked Synths"), "{:?}", r.danger_lines());
    assert_eq!(labels(r.ui()), ALL, "the row stays");
    assert!(rows(r.ui()).iter().all(|x| !x.off));
    // The library only library.libraries lists is no package to the engine: its words, and no question.
    r.act(Action::Move { pane: None, by: -2 }); // Field Recordings
    r.e.fail.clear();
    r.tool("Remove");
    assert!(r.notice.is_none());
    assert!(r.danger_lines().join(" ").contains("no package lib-2 is installed"), "{:?}", r.danger_lines());
}

// A row confirmed opens its detail: name, id, version, the publisher (the repository it came from), the key
// fingerprint of what signed it, as the repositories page shows a root key's; the panel is
// the detail until Back, reaching a package by the cursor and a press, and nothing is asked (P3).
#[test]
fn a_packages_detail_shows_what_the_engine_reports() {
    let mut r = installed();
    let base = r.e.calls.len();
    r.press_index(0); // Example Reverb
    assert_eq!(r.e.calls.len(), base, "a press asks nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
    assert_eq!(r.c.path(), ["Settings", "Installed", "Example Reverb"], "the bar names the package");
    let panel = info(r.ui());
    assert!(panel.contains(&"Plugin".to_string()) && panel.contains(&"org.example.reverb".to_string()), "{panel:?}");
    assert!(panel.contains(&"1.0.0".to_string()), "{panel:?}");
    assert!(panel.contains(&"https://packages.oscairn.org".to_string()), "the publisher is the repository it came from: {panel:?}");
    assert!(panel.contains(&"Key fingerprint".to_string()) && panel.contains(&"6a2d d1ac f4d4 e23c".to_string()), "the key that signed it, in the reference's grouping: {panel:?}");
    assert!(!panel.join(" ").contains("Trusted"), "the engine reports no trust flag: {panel:?}");
    // A file's package, whose source is unknown: no fingerprint.
    r.press_index(1); // example strings
    assert_eq!(r.c.path(), ["Settings", "Installed", "example strings"], "the detail is the package confirmed");
    let panel = info(r.ui()).join(" ");
    assert!(panel.contains("Samples") && panel.contains("org.example.strings") && panel.contains("installed from a file"), "{panel}");
    assert!(!panel.contains("Key fingerprint"), "{panel}");
    assert_eq!(r.e.calls.len(), base, "another press asks nothing: {:?}", r.e.methods());
    assert!(r.delayed.is_empty(), "{:?}", r.delayed);
}

// Back closes a package's detail and the list shows again with the cursor on the same row; nothing is asked (P3).
#[test]
fn back_closes_a_packages_detail_on_the_same_row() {
    let mut r = installed();
    r.press_index(1); // example strings
    assert_eq!(r.c.path(), ["Settings", "Installed", "example strings"]);
    let base = r.e.calls.len();
    let o = r.c.back(); // the hardware Back, as the shell's go_back reaches the screen
    assert_eq!(o, crate::settings_app::Outcome::Stay, "Back closes the detail, not the page: {o:?}");
    r.settle();
    assert_eq!(r.c.path(), ["Settings", "Installed"]);
    assert_eq!(rows(r.ui())[1].label, "example strings", "the list shows again");
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

// ---- P1: Available ----

// What one repository offers, as `packages.job` returns it (the handler's shape; `types` decide the kind).
fn offered(id: &str, name: &str, version: &str, types: Value) -> Value {
    json!({"id": id, "name": name, "summary": "", "license": "", "publisher": "", "publisherKey": "", "tags": [],
           "versions": [{"version": version, "channel": "stable", "yanked": "", "incompatible": "", "sha256": "", "size": 1234, "installed": 0, "types": types, "dependencies": [], "withdrawn": null, "exports": []}]})
}

/// Two enabled repositories, one a needed key trusted, offering five packages: one installed at the offered version,
/// one installed older, three not installed (P1). The list is read when the page is shown.
fn available() -> Rig {
    let mut e = Fake::new();
    e.trusted.push(FRIEND.into());
    e.repos.push(Repo { address: "https://friend.example.org".into(), name: "A Friend's".into(), key: FRIEND.into(), origin: "user".into(), enabled: true });
    e.sources = vec![
        ("https://packages.oscairn.org".into(), Reads::Done("Oscairn".into(), "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".into())),
        ("https://friend.example.org".into(), Reads::Done("A Friend's".into(), FRIEND.into())),
    ];
    e.offers = vec![
        ("https://packages.oscairn.org".into(), vec![
            offered("org.alpha", "Alpha Plugin", "1.2.0", json!({"clap": 1})),
            offered("org.delta", "Delta Pack", "3.0.0", json!({"oscairn.asset": 1})),
        ]),
        ("https://friend.example.org".into(), vec![
            offered("org.beta", "Beta Pack", "2.5.0", json!({"oscairn.asset": 1})),
            offered("org.epsilon", "Epsilon Pack", "1.0.0", json!({"oscairn.asset": 1})),
            offered("org.gamma", "Gamma Library", "0.9.0", json!({})),
        ]),
    ];
    e.installed_packages = vec![
        package("org.alpha", "Alpha Plugin", "1.2.0", "plugin", "", &[]),
        package("org.beta", "Beta Pack", "1.0.0", "asset", "", &[]),
    ];
    let mut r = Rig::new("available", None, e);
    r.e.calls.clear();
    r.page(Id::Available);
    r.later_all();
    r
}

// One alphabetical list by name, each row its name, the word for its kind as the Installed page writes it, the newest
// version offered, and a state word: "Installed" when that version is installed, "Update from X" when an older version
// X is installed, nothing when it is not installed (P1).
#[test]
fn the_available_is_one_list_by_name_with_words() {
    let mut r = available();
    assert!(r.e.methods().iter().any(|m| m == "packages.repositories"), "the enabled repositories are read");
    assert!(r.e.methods().iter().any(|m| m == "packages.list"), "each repository is listed");
    assert_eq!(labels(r.ui()), ["Alpha Plugin", "Beta Pack", "Delta Pack", "Epsilon Pack", "Gamma Library"], "one list, by name");
    assert_eq!(value_of(r.ui(), "Alpha Plugin"), "Plugin 1.2.0  ·  Installed", "its version is installed");
    assert_eq!(value_of(r.ui(), "Beta Pack"), "Samples 2.5.0  ·  Update from 1.0.0", "an older version is installed");
    assert_eq!(value_of(r.ui(), "Delta Pack"), "Samples 3.0.0", "an asset's word, not installed");
    assert_eq!(value_of(r.ui(), "Epsilon Pack"), "Samples 1.0.0");
    assert_eq!(value_of(r.ui(), "Gamma Library"), "Library 0.9.0", "a library's word, not installed");
    assert_eq!(r.ui().global::<crate::PartState>().get_total(), 5, "the count is of offers");
    r.e.calls.clear();
    for _ in 0..5 {
        r.act(Action::Move { pane: None, by: 1 });
    }
    assert!(info(r.ui()).contains(&"Gamma Library".to_string()), "{:?}", info(r.ui()));
    assert!(r.e.calls.is_empty(), "moving the cursor asks nothing: {:?}", r.e.methods());
}

// A failed repository shows the app's usual failed row with the engine's words, and the rest still lists (P1).
#[test]
fn a_failed_repository_is_said_and_the_rest_lists() {
    let mut e = Fake::new();
    e.trusted.push(FRIEND.into());
    e.repos.push(Repo { address: "https://friend.example.org".into(), name: "A Friend's".into(), key: FRIEND.into(), origin: "user".into(), enabled: true });
    e.sources = vec![
        ("https://packages.oscairn.org".into(), Reads::Fails("cannot fetch repository.json: Could not connect to the server.".into())),
        ("https://friend.example.org".into(), Reads::Done("A Friend's".into(), FRIEND.into())),
    ];
    e.offers = vec![("https://friend.example.org".into(), vec![offered("org.gamma", "Gamma Library", "0.9.0", json!({}))])];
    let mut r = Rig::new("available-fail", None, e);
    r.e.calls.clear();
    r.page(Id::Available);
    r.later_all();
    assert!(info(r.ui()).contains(&"Not read".to_string()), "{:?}", info(r.ui()));
    assert!(r.danger_lines().join(" ").contains("Could not connect to the server"), "the engine's words: {:?}", r.danger_lines());
    assert!(labels(r.ui()).contains(&"Gamma Library".to_string()), "the friend's repository still lists");
    r.e.calls.clear();
    assert!(r.e.calls.is_empty(), "at rest nothing more is asked: {:?}", r.e.methods());
}

// Every row is reached by the cursor, and Refresh reads the repositories and the listings again (P1).
#[test]
fn refresh_reads_the_lists_again() {
    let mut r = available();
    for at in 0..5 {
        r.e.calls.clear();
        r.act(Action::Move { pane: None, by: 1 });
        assert_eq!(rows(r.ui()).iter().position(|x| x.cursor), Some(at), "row {at} is reached by the cursor");
        assert!(r.e.calls.is_empty(), "moving the cursor asks nothing");
    }
    r.e.calls.clear();
    r.delayed.clear();
    r.tool("Refresh");
    assert_eq!(r.e.methods().iter().filter(|m| *m == "packages.repositories").count(), 1, "refresh asks the repositories again");
    assert_eq!(r.e.methods().iter().filter(|m| *m == "packages.installed").count(), 1, "refresh reads what is installed again");
    r.later_all();
    assert_eq!(r.e.methods().iter().filter(|m| *m == "packages.list").count(), 2, "both repositories are listed again");
    assert_eq!(labels(r.ui()), ["Alpha Plugin", "Beta Pack", "Delta Pack", "Epsilon Pack", "Gamma Library"], "the list is the same after a refresh");
}

// The version shown is the one the engine would install, as the Catalog picks it: the newest stable one this device
// can install, never a beta, a yanked, an incompatible or a withdrawn one; a package with none is faint and the panel
// says why. The kind is what is installed says, else what the version holds: a library holding plugins is a library.
#[test]
fn the_available_version_is_the_one_the_engine_would_install() {
    let version = |v: &str, channel: &str, yanked: &str, incompatible: &str, withdrawn: Value, types: Value| {
        json!({"version": v, "channel": channel, "yanked": yanked, "incompatible": incompatible, "sha256": "", "size": 10, "installed": 0, "types": types, "dependencies": [], "withdrawn": withdrawn, "exports": []})
    };
    let package = |id: &str, name: &str, versions: Vec<Value>| json!({"id": id, "name": name, "summary": "", "license": "", "publisher": "", "publisherKey": "", "tags": [], "versions": versions});
    let clap = json!({"clap": 1});
    let mut e = Fake::new();
    e.sources = vec![("https://packages.oscairn.org".into(), Reads::Done("Oscairn".into(), "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".into()))];
    e.offers = vec![(
        "https://packages.oscairn.org".into(),
        vec![
            package("org.many", "Many Versions", vec![
                version("2.0.0", "stable", "", "", Value::Null, clap.clone()),
                version("3.0.0", "beta", "", "", Value::Null, clap.clone()),
                version("2.5.0", "stable", "it crashes", "", Value::Null, clap.clone()),
                version("4.0.0", "stable", "", "it needs a system update: Oscairn 9 or later", Value::Null, clap.clone()),
                version("5.0.0", "stable", "", "", json!({"repository": "Oscairn", "reason": "a key was stolen", "date": "2026-11-01"}), clap.clone()),
            ]),
            package("org.none", "None Here", vec![version("1.0.0", "stable", "", "it has no build for this device (linux-arm64)", Value::Null, clap.clone())]),
            package("org.songs", "Songs With Synths", vec![version("1.0.0", "stable", "", "", Value::Null, json!({"oscairn.performance": 4, "clap": 1}))]),
            package("org.mine", "Mine Installed", vec![version("1.0.0", "stable", "", "", Value::Null, clap.clone())]),
        ],
    )];
    e.installed_packages = vec![package_installed("org.mine", "Mine Installed", "1.0.0", "library")];
    let mut r = Rig::new("available-versions", None, e);
    r.page(Id::Available);
    r.later_all();
    assert_eq!(value_of(r.ui(), "Many Versions"), "Plugin 2.0.0", "the newest stable version this device can install");
    assert_eq!(value_of(r.ui(), "Songs With Synths"), "Library 1.0.0", "a package holding more than plugins installs as a library");
    assert_eq!(value_of(r.ui(), "Mine Installed"), "Library 1.0.0  ·  Installed", "what is installed says its kind");
    let off: Vec<bool> = rows(r.ui()).iter().map(|x| x.off).collect();
    assert_eq!(off, [false, false, true, false], "only the package with no version for this device is faint: {:?}", labels(r.ui()));
    for _ in 0..3 {
        r.act(Action::Move { pane: None, by: 1 });
    }
    assert!(info(r.ui()).iter().any(|l| l.contains("no build for this device")), "the panel says why: {:?}", info(r.ui()));
}

fn package_installed(id: &str, name: &str, version: &str, kind: &str) -> Value {
    package(id, name, version, kind, "https://packages.oscairn.org", &[])
}
