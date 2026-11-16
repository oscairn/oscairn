// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Catalog (ui/LIBRARY-APP.md D4 to D7) against the fake engine: the repositories read one after another by jobs asked
//! about until they end, the packages in four tabs, a package's details, installing one, and the questions for code this
//! device does not trust and for a different build already here.

use super::library::*;
use super::*;
use crate::library::Outcome;
use std::collections::BTreeMap;

fn version(v: &str, types: serde_json::Value) -> serde_json::Value {
    json!({"version": v, "channel": "stable", "yanked": "", "incompatible": "", "sha256": format!("sha-{v}-0123456789abcdef"), "size": 5_242_880, "installed": 12_582_912,
        "types": types, "dependencies": [], "withdrawn": null, "exports": [{"platform": "", "address": format!("https://repo.example/exports/{v}.olib")}]})
}

fn package(id: &str, name: &str, versions: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"id": id, "name": name, "summary": "Invented", "license": "CC0", "publisher": "Quill", "publisherKey": "0123456789abcdef", "tags": ["demo"], "versions": versions})
}

/// A library whose engine has one repository with a Performance package, a plugin and a Sounds package.
fn rig() -> Rig {
    let mut r = Rig::new(library());
    r.e.repos = vec![("https://repo.example".into(), "Example".into(), true), ("https://off.example".into(), "Off".into(), false)];
    r.e.repo_packages = vec![(
        "https://repo.example".into(),
        Ok(vec![
            package("org.example.pads", "Dawn Pads", vec![version("1.0.0", json!({"oscairn.performance": 3})), version("1.1.0", json!({"oscairn.performance": 3}))]),
            package("org.example.fuzz", "Fuzz Box", vec![version("2.0.0", json!({"clap": 1}))]),
            package("org.example.keys", "Felt Keys", vec![version("1.0.0", json!({"oscairn.sound": 5}))]),
        ]),
    )];
    r
}

/// Opens the Catalog from Install..., answers what it asks (the plugin scan with `bundled`) and the jobs until it is read.
fn catalog(r: &mut Rig, bundled: &[(&str, &str)]) {
    r.tool("Install");
    r.c.take_menu().unwrap();
    r.c.chosen(1);
    r.settle();
    assert!(r.c.take_scan(), "the app's plugins are scanned each time the Catalog opens");
    r.c.scanned(bundled.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeMap<_, _>>());
    r.later_all();
}

// The Catalog asks what is installed and which repositories are on, reads each one that is on by a job asked about every
// 100 ms, and shows what they offer in four tabs: Performances first, with the state of each package in its second line.
#[test]
fn the_repositories_are_read_by_jobs_and_listed() {
    let mut r = rig();
    r.e.calls.clear();
    r.tool("Install");
    r.c.take_menu().unwrap();
    r.c.chosen(1);
    r.settle();
    assert_eq!(r.e.methods(), ["packages.installed", "packages.repositories", "packages.list"], "only the repository that is on is read");
    assert_eq!(r.e.called("packages.list"), [json!({"source": "https://repo.example"})]);
    assert_eq!(r.c.path(), ["Library", "Catalog"]);
    assert_eq!(tabs(r.ui()), ["Performances", "Sounds", "Plugins", "Content"]);
    assert!(info(r.ui()).join(" ").contains("Reading Example..."), "{:?}", info(r.ui()));
    assert!(r.delayed.iter().any(|d| d.0 == "packages.job" && d.1 == std::time::Duration::from_millis(100)), "asked about each 100 ms: {:?}", r.delayed);
    r.c.scanned(BTreeMap::new());
    r.later_all();
    assert_eq!(labels(r.ui()), ["Dawn Pads  1.1.0"]);
    assert_eq!(value_of(r.ui(), "Dawn Pads  1.1.0"), "Quill  ·  Invented");
    assert!(info(r.ui()).join(" ").contains("From Example."), "{:?}", info(r.ui()));
    r.c.show_page(1);
    assert_eq!(labels(r.ui()), ["Felt Keys  1.0.0"]);
    r.c.show_page(2);
    assert_eq!(labels(r.ui()), ["Fuzz Box  2.0.0"]);
    r.c.show_page(3);
    assert!(labels(r.ui()).is_empty());
    assert!(info(r.ui()).join(" ").contains("No content packages in the repositories you have on. Look under Performances, Sounds or Plugins."), "{:?}", info(r.ui()));
    // Packages are asked about nothing more at rest.
    r.e.calls.clear();
    r.settle();
    assert!(r.e.calls.is_empty());
}

// The Catalog opened on a tab with nothing moves to the first tab that has something; a repository that cannot be read is
// named with why, and the rest are listed.
#[test]
fn a_tab_with_nothing_moves_on_and_a_failed_repository_is_named() {
    let mut r = rig();
    r.e.repos.push(("https://broken.example".into(), "Broken".into(), true));
    r.e.repo_packages.push(("https://broken.example".into(), Err("its root key is not one this device trusts".into())));
    r.e.repo_packages[0].1 = Ok(vec![package("org.example.keys", "Felt Keys", vec![version("1.0.0", json!({"oscairn.sound": 5}))])]);
    catalog(&mut r, &[]);
    assert_eq!(r.c.path(), ["Library", "Catalog"]);
    assert_eq!(labels(r.ui()), ["Felt Keys  1.0.0"], "opened on Performances, which has none: Sounds is shown");
    assert_eq!(r.ui().global::<crate::LibraryState>().get_page(), 1);
    let text = info(r.ui()).join(" ");
    assert!(text.contains("From Example. Not read: Broken (its root key is not one this device trusts)."), "{text}");
}

// With no repository on, the Catalog says so and where they are turned on; one that offers nothing says so.
#[test]
fn nothing_to_list_is_said() {
    let mut r = rig();
    r.e.repos = vec![("https://off.example".into(), "Off".into(), false)];
    r.tool("Install");
    r.c.take_menu().unwrap();
    r.c.chosen(1);
    r.settle();
    r.c.scanned(BTreeMap::new());
    let text = info(r.ui()).join(" ");
    assert!(text.contains("No repository is on. Settings lists them."), "{text}");
    assert!(text.contains("No package repository is on. Repositories are turned on and added in the Settings app, on its Package repositories page."), "{text}");
    let mut r = rig();
    r.e.repo_packages = vec![("https://repo.example".into(), Ok(vec![]))];
    catalog(&mut r, &[]);
    assert!(info(r.ui()).join(" ").contains("The repositories you have on offer no packages."), "{:?}", info(r.ui()));
}

// The empty Library has a way to the Catalog on the kind it shows.
#[test]
fn an_empty_library_offers_the_catalog() {
    let mut r = Rig::new(vec![]);
    r.e.repos = vec![("https://repo.example".into(), "Example".into(), true)];
    r.e.repo_packages = vec![("https://repo.example".into(), Ok(vec![package("org.example.keys", "Felt Keys", vec![version("1.0.0", json!({"oscairn.sound": 5}))])]))];
    assert_eq!(labels(r.ui()), ["Filters", "Browse the Catalog"]);
    r.c.show_page(1);
    r.settle();
    r.press_row("Browse the Catalog");
    r.settle();
    r.c.scanned(BTreeMap::new());
    r.later_all();
    assert_eq!(labels(r.ui()), ["Felt Keys  1.0.0"], "the Catalog on the Sounds tab");
    // Back from the Catalog is the Library, not Home.
    assert_eq!(r.c.back(), Outcome::Stay);
    assert_eq!(r.c.path(), ["Library", "Sounds"]);
}

// Search narrows by every word in the name, ID, summary, publisher or a tag.
#[test]
fn the_catalog_is_searched() {
    let mut r = rig();
    r.e.repo_packages[0].1 = Ok(vec![
        package("org.example.pads", "Dawn Pads", vec![version("1.0.0", json!({"oscairn.performance": 3}))]),
        package("org.example.organ", "Pipe Organ", vec![version("1.0.0", json!({"oscairn.performance": 1}))]),
    ]);
    catalog(&mut r, &[]);
    assert_eq!(r.c.bar_tools().iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Search"]);
    assert_eq!(r.tool("Search"), Outcome::Prompt("Search the Catalog by name, tag or publisher".into(), String::new()));
    r.c.entered("pipe quill");
    assert_eq!(labels(r.ui()), ["Pipe Organ  1.0.0"]);
    r.tool("Search");
    r.c.entered("zzz");
    assert!(labels(r.ui()).is_empty());
    assert!(info(r.ui()).join(" ").contains("Nothing here matches the search."));
}

// What is installed says so with its version, an update is offered, a plugin that comes with the app says Built in, and a
// package that cannot be installed here is dimmed and says why.
#[test]
fn what_is_installed_is_marked() {
    let mut r = rig();
    r.e.installed_packages = vec![json!({"id": "org.example.pads", "version": "1.0.0", "kind": "library", "repository": "https://repo.example", "sha256": "sha-1.0.0-0123456789abcdef", "signers": [], "otherBuild": false, "withdrawn": null, "dependency": false})];
    let mut bad = version("3.0.0", json!({"oscairn.performance": 1}));
    bad["incompatible"] = json!("it needs a system update: Oscairn 9 or later");
    r.e.repo_packages[0].1.as_mut().unwrap().push(package("org.example.new", "Needs Update", vec![bad]));
    catalog(&mut r, &[("org.example.fuzz", "1.5.0")]);
    assert_eq!(labels(r.ui()), ["Dawn Pads  1.0.0", "Needs Update  3.0.0"]);
    assert_eq!(value_of(r.ui(), "Dawn Pads  1.0.0"), "Update to 1.1.0  ·  Quill  ·  Invented");
    assert_eq!(value_of(r.ui(), "Needs Update  3.0.0"), "Cannot be installed here: it needs a system update: Oscairn 9 or later");
    assert!(rows(r.ui())[1].off, "dimmed");
    r.c.show_page(2);
    assert_eq!(value_of(r.ui(), "Fuzz Box  1.5.0"), "Update to 2.0.0  ·  Quill  ·  Invented", "built in at 1.5.0: its row says the version it has");
}

// A package's details: each fact a row, the download addresses, then Install (or Update to V, or the repository's build) and
// Back; installing closes the Catalog and runs the job in the Library: asked about until it ends, then said.
#[test]
fn a_package_is_installed_from_its_details() {
    let mut r = rig();
    catalog(&mut r, &[]);
    r.press_row("Dawn Pads  1.1.0");
    assert_eq!(r.c.path(), ["Library", "Catalog", "Dawn Pads"]);
    let facts: Vec<(String, String)> = rows(r.ui()).iter().map(|x| (x.label.to_string(), x.value.to_string())).collect();
    assert_eq!(facts[..3], [("Install".to_string(), String::new()), ("Summary".to_string(), "Invented".to_string()), ("Version".to_string(), "1.1.0".to_string())], "what can be done comes first");
    assert!(facts.contains(&("License".to_string(), "CC0".to_string())) && facts.contains(&("Publisher".to_string(), "Quill, key 0123 4567 89ab cdef".to_string())) && facts.contains(&("Repository".to_string(), "Example".to_string())));
    assert!(facts.contains(&("Size".to_string(), "5.0 MB to download, 12.0 MB installed".to_string())) && facts.contains(&("Needs".to_string(), "Nothing else".to_string())));
    assert!(facts.contains(&("Download for another device".to_string(), String::new())) && facts.iter().any(|f| f.1 == "https://repo.example/exports/1.1.0.olib"));
    assert_eq!(facts.last().unwrap().0, "Back");
    r.e.calls.clear();
    r.press_row("Install");
    assert_eq!(r.c.path(), ["Library", "Performances"], "the Catalog is gone, as the JUCE dialog is");
    assert_eq!(footer(r.ui()).0, "Installing org.example.pads 1.1.0...");
    r.later_all();
    assert_eq!(r.e.called("packages.install"), [json!({"source": "https://repo.example", "id": "org.example.pads", "acceptUnsignedCode": false, "replace": false, "version": "1.1.0"})]);
    assert!(r.e.methods().iter().filter(|m| *m == "packages.job").count() >= 2, "asked about until it ended: {:?}", r.e.methods());
    assert_eq!(footer(r.ui()).0, "Dawn Pads 1.1.0 installed. Its builds are in the plugin store.");
    assert!(r.e.methods().contains(&"library.query".to_string()), "its items, listed again");
    // Back from the details.
    catalog(&mut r, &[]);
    r.press_row("Dawn Pads  1.1.0");
    assert!(labels(r.ui()).iter().all(|l| l != "Install"), "installed and current: nothing to install");
    r.press_row("Back");
    assert_eq!(r.c.path(), ["Library", "Catalog"]);
}

// Code this device does not trust asks first. Signed by the publisher's own key: trust it (for plugins) and install; a
// stranger's key: trust it, or install this once; unsigned: install this once or not.
#[test]
fn code_from_an_unknown_source_asks_first() {
    let code = |status: &str, key: &str| json!([{"kind": "clap", "name": "Fuzz", "version": "2.0.0", "platform": "linux-arm64", "status": status, "key": key, "keyName": "", "publicKey": "pub-key-bytes"}]);
    let mut r = rig();
    let mut fuzz = package("org.example.fuzz", "Fuzz Box", vec![version("2.0.0", json!({"clap": 1}))]);
    fuzz["_code"] = code("unknown", "0123456789abcdef");
    r.e.repo_packages[0].1 = Ok(vec![fuzz]);
    catalog(&mut r, &[]);
    r.press_row("Fuzz Box  2.0.0");
    r.e.calls.clear();
    r.press_row("Install");
    r.later_all();
    let (title, message, choices) = r.c.take_notice().expect("the question");
    assert_eq!(title, "Trust Quill to install plugins?");
    assert!(message.contains("Key fingerprint: 0123 4567 89ab cdef"), "{message}");
    assert_eq!(choices, ["Trust and install", "Cancel"]);
    assert_eq!(r.e.called("packages.install").len(), 1);
    // Trust: the key is trusted for plugins by the publisher's name, then it is installed, asking nothing.
    r.c.noticed(Some(0));
    r.settle();
    r.later_all();
    assert_eq!(r.e.called("trust.set"), [json!({"key": "pub-key-bytes", "name": "Quill", "scope": ["plugins"], "revoked": false})]);
    let installs = r.e.called("packages.install");
    assert_eq!(installs.len(), 2);
    assert_eq!(installs[1]["acceptUnsignedCode"], false);
    assert_eq!(footer(r.ui()).0, "Fuzz Box 2.0.0 installed. Its builds are in the plugin store.");
    // A stranger's key: Install anyway, this once, says yes to the code.
    let mut r = rig();
    let mut fuzz = package("org.example.fuzz", "Fuzz Box", vec![version("2.0.0", json!({"clap": 1}))]);
    fuzz["_code"] = code("unknown", "ffffeeeeddddcccc");
    r.e.repo_packages[0].1 = Ok(vec![fuzz.clone()]);
    catalog(&mut r, &[]);
    r.press_row("Fuzz Box  2.0.0");
    r.press_row("Install");
    r.later_all();
    let (title, _, choices) = r.c.take_notice().unwrap();
    assert_eq!((title.as_str(), choices), ("Install code from an unknown key?", vec!["Trust this key".to_string(), "Install anyway".to_string(), "Cancel".to_string()]));
    r.e.calls.clear();
    r.c.noticed(Some(1));
    r.settle();
    r.later_all();
    assert!(r.e.called("trust.set").is_empty(), "nothing is trusted unless the user says so");
    assert_eq!(r.e.called("packages.install")[0]["acceptUnsignedCode"], true);
    // The same, trusting the key: named for what it is, as nobody vouches for it.
    let mut r = rig();
    r.e.repo_packages[0].1 = Ok(vec![fuzz]);
    catalog(&mut r, &[]);
    r.press_row("Fuzz Box  2.0.0");
    r.press_row("Install");
    r.later_all();
    r.c.take_notice().unwrap();
    r.c.noticed(Some(0));
    r.settle();
    r.later_all();
    assert_eq!(r.e.called("trust.set"), [json!({"key": "pub-key-bytes", "name": "Key ffffeeeeddddcccc", "scope": ["plugins"], "revoked": false})]);
    // Cancel: nothing more is asked.
    let mut r = rig();
    let mut unsigned = package("org.example.fuzz", "Fuzz Box", vec![version("2.0.0", json!({"clap": 1}))]);
    unsigned["_code"] = code("unsigned", "");
    r.e.repo_packages[0].1 = Ok(vec![unsigned]);
    catalog(&mut r, &[]);
    r.press_row("Fuzz Box  2.0.0");
    r.press_row("Install");
    r.later_all();
    let (title, _, choices) = r.c.take_notice().unwrap();
    assert_eq!((title.as_str(), choices.len()), ("Install unsigned code?", 2));
    r.e.calls.clear();
    r.c.noticed(None);
    r.settle();
    r.later_all();
    assert!(r.e.calls.is_empty());
}

// A different build of the same version already here is installed over only when the user says so, having seen both.
#[test]
fn a_different_build_is_replaced_only_when_asked() {
    let mut r = rig();
    r.e.installed_packages = vec![json!({"id": "org.example.pads", "version": "1.1.0", "kind": "library", "repository": "https://repo.example", "sha256": "other-build-sha-0123456789", "signers": ["1111222233334444"], "otherBuild": false, "withdrawn": null, "dependency": false})];
    catalog(&mut r, &[]);
    assert_eq!(value_of(r.ui(), "Dawn Pads  1.1.0"), "Installed (a different build)  ·  Quill  ·  Invented");
    r.press_row("Dawn Pads  1.1.0");
    r.e.calls.clear();
    r.press_row("Install the repository's build");
    let (title, message, choices) = r.c.take_notice().expect("the question");
    assert_eq!((title.as_str(), choices), ("Install the repository's build?", vec!["Install the repository's build".to_string(), "Cancel".to_string()]));
    assert!(message.contains("Installed: archive other-build-sha-, signed by key 1111 2222 3333 4444") && message.contains("Example's: archive sha-1.1.0-012345,"), "{message}");
    assert!(r.e.called("packages.install").is_empty());
    r.c.noticed(Some(0));
    r.settle();
    r.later_all();
    assert_eq!(r.e.called("packages.install")[0]["replace"], true);
}

// A refusal of the engine (another job running) is said; an install that fails is told in a question with one answer.
#[test]
fn a_refused_install_is_said() {
    let mut r = rig();
    catalog(&mut r, &[]);
    r.press_row("Dawn Pads  1.1.0");
    r.e.fail.push(("packages.install".into(), "another job is running".into()));
    r.press_row("Install");
    r.later_all();
    assert_eq!(footer(r.ui()).0, "another job is running");
    r.e.fail.clear();
    r.e.repo_packages[0].1 = Ok(vec![]);
    catalog(&mut r, &[]);
    assert_eq!(r.c.path(), ["Library", "Catalog"]);
}
