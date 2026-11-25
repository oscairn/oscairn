// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Installed page (ui/SETTINGS-APP.md P, the reference's `notBuilt` "Installed content" page, RFC
//! 0006): what is installed on this device, read once when the page is shown and never on a timer. One list, by name
//! whatever the case (the id where two share one): every package `packages.installed` reports, and any library only
//! `library.libraries` lists. Each row is one package: its name, the word for its kind and its version; what an
//! archive held is not listed apart. A long name is the row's to fit, the panel wraps the whole one. A refusal or a
//! lost connection is the app's usual failed row.
//! Confirming a row opens its detail in the panel (id, kind, version, the publisher and the key fingerprint of what
//! signed it; Back closes it on the same row; no trust flag, which the handler does not report). The bar's Remove...
//! works on every row: its question opens at once ("Checking what uses it..."), `packages.removal` is asked, and its
//! answer words the questions: what uses the package
//! (Cancel / Remove anyway), else the plain Remove?; then, when it has dependencies nothing else uses, whether those
//! go too, each with its size. One `packages.uninstall` follows, and the lists are read until the package is gone.

use crate::client::Error;
use crate::library::files::size_text;
use crate::library::fingerprint;
use crate::settings_app::{failed, head, line, row, said, Ctx, Page, Step as Any};
use crate::topbar::Tool;
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};
use std::time::Duration;

/// How often, and how soon after, the lists are read while an accepted removal still shows (5 s at most).
const FOLLOW: u32 = 25;
const FOLLOW_AFTER: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// packages.installed.
    Installed,
    /// library.libraries.
    Libraries,
    /// packages.removal, of the item asked about.
    Removal(Item),
    /// packages.uninstall, of the item.
    Uninstall(Item),
}

/// One row: a package, or a library no package is recorded for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: String,
    pub version: String,
    /// plugin, asset or library.
    pub kind: String,
    /// What to show; the id where the engine has no name.
    pub name: String,
    /// The repository it came from, empty for one installed from a file.
    pub repository: String,
    /// The IDs of the keys that signed it, empty when unknown.
    pub signers: Vec<String>,
    /// A library's item count.
    pub items: Option<u64>,
    /// `packages.installed` lists it, so where it came from is known.
    pub package: bool,
}

impl Item {
    /// The same package still listed: its id, and for a plugin package, whose versions install side by side, its version.
    fn is(&self, other: &Item) -> bool {
        self.id == other.id && (self.kind != "plugin" || self.version == other.version)
    }

    /// What the two removal requests name.
    fn params(&self) -> Value {
        match self.kind == "plugin" && !self.version.is_empty() {
            true => json!({"id": self.id, "version": self.version}),
            false => json!({"id": self.id}),
        }
    }
}

/// The removal being asked about: which question is open, and what the answers so far make of the request.
#[derive(Clone, Debug, PartialEq)]
struct Asking {
    item: Item,
    /// Something uses it: the first question offers Remove anyway.
    used: bool,
    /// Its dependencies nothing else uses: id and "Name (size)".
    unused: Vec<(String, String)>,
    /// The second question is the one open.
    second: bool,
}

#[derive(Default)]
pub struct Installed {
    packages: Vec<Item>,
    libraries: Vec<Item>,
    package_error: String,
    libraries_error: String,
    asking: Option<Asking>,
    /// The item `packages.removal` is out for: its question is open, saying it is checking, until the answer fills it in.
    checking: Option<Item>,
    /// While a removal is asked about or runs, or the lists are read back afterwards: every row faint.
    busy: bool,
    /// The item `packages.uninstall` removed, and the reads of the list made since: the lists are read again until it no longer shows.
    removing: Option<(Item, u32)>,
    /// "Removing NAME..." while it runs.
    progress: String,
    /// A removal refused: the item's id and what the engine said.
    refused: (String, String),
    /// The item whose detail the panel shows, opened by a press on its row; None: the panel follows the cursor.
    detail: Option<Item>,
}

/// "Plugin", "Samples" or "Library": the word a row and the panel say for a package's kind.
pub(crate) fn kind_word(kind: &str) -> &'static str {
    match kind {
        "plugin" => "Plugin",
        "asset" => "Samples",
        _ => "Library",
    }
}

fn items(r: &Value, key: &str) -> Vec<Value> {
    r[key].as_array().cloned().unwrap_or_default()
}

impl Installed {
    /// Everything installed, in the order shown: the packages, each library package with its item count, and the
    /// libraries only `library.libraries` lists; by name whatever the case, then by id.
    fn list(&self) -> Vec<Item> {
        let mut all = self.packages.clone();
        for l in &self.libraries {
            match all.iter_mut().find(|p| p.kind == "library" && p.id == l.id) {
                Some(p) => {
                    p.items = l.items;
                    if p.name == p.id {
                        p.name = l.name.clone();
                    }
                }
                None => all.push(l.clone()),
            }
        }
        all.sort_by_cached_key(|i| (i.name.to_lowercase(), i.id.clone(), i.version.clone()));
        all
    }

    fn read(&mut self, c: &mut Ctx) {
        self.package_error.clear();
        self.libraries_error.clear();
        c.ask(Any::Installed(Step::Installed), "packages.installed", json!({}));
        c.ask(Any::Installed(Step::Libraries), "library.libraries", json!({}));
    }

    fn rest(&mut self) {
        self.busy = false;
        self.removing = None;
        self.progress.clear();
    }

    /// The second question, when the package has dependencies nothing else uses; else the removal itself.
    fn go_on(&mut self, mut a: Asking, c: &mut Ctx) {
        if !a.unused.is_empty() && !a.second {
            let list = a.unused.iter().map(|u| u.1.as_str()).collect::<Vec<_>>().join(", ");
            c.notice = Some(("Also remove what nothing else uses?".into(), list, vec!["Keep them".into(), "Remove them too".into(), "Cancel".into()]));
            a.second = true;
            self.asking = Some(a);
            return;
        }
        let mut params = a.item.params();
        if a.used {
            params["evenIfUsed"] = json!(true);
        }
        if !a.unused.is_empty() {
            params["withUnused"] = json!(a.unused.iter().map(|u| u.0.as_str()).collect::<Vec<_>>());
        }
        self.busy = true;
        self.progress = format!("Removing {}...", a.item.name);
        c.ask(Any::Installed(Step::Uninstall(a.item)), "packages.uninstall", params);
    }

    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, c: &mut Ctx) {
        match (step, result) {
            (Step::Installed, Ok(r)) => {
                self.package_error.clear();
                self.packages = items(&r, "packages")
                    .iter()
                    .map(|x| {
                        let text = |k: &str| x[k].as_str().unwrap_or_default().to_string();
                        let name = if text("name").is_empty() { text("id") } else { text("name") };
                        Item { id: text("id"), version: text("version"), kind: text("kind"), name, repository: text("repository"), signers: x["signers"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect(), items: None, package: true }
                    })
                    .collect();
                // Removed is not always gone at once: the lists are read again until the package no longer shows.
                match self.removing.take() {
                    Some((item, n)) if self.packages.iter().any(|p| p.is(&item)) => {
                        if n < FOLLOW {
                            self.removing = Some((item, n + 1));
                            c.ask_after(FOLLOW_AFTER, Any::Installed(Step::Installed), "packages.installed", json!({}));
                            c.ask_after(FOLLOW_AFTER, Any::Installed(Step::Libraries), "library.libraries", json!({}));
                        } else {
                            self.rest();
                            self.refused = (item.id, "The engine removed it but still lists it".into());
                        }
                    }
                    Some(_) => self.rest(),
                    None => {}
                }
            }
            (Step::Installed, Err(e)) => {
                self.rest();
                self.package_error = e.message;
            }
            (Step::Libraries, Ok(r)) => {
                self.libraries_error.clear();
                self.libraries = items(&r, "libraries")
                    .iter()
                    .map(|x| {
                        let text = |k: &str| x[k].as_str().unwrap_or_default().to_string();
                        Item { id: text("id"), kind: "library".into(), name: if text("name").is_empty() { text("id") } else { text("name") }, items: Some(x["items"].as_u64().unwrap_or(0)), ..Default::default() }
                    })
                    .collect();
            }
            (Step::Libraries, Err(e)) => {
                self.libraries_error = e.message;
            }
            // Cancelled while checking, or the page left: the answer asks nothing.
            (Step::Removal(item), _) if self.checking.as_ref() != Some(&item) => self.busy = false,
            (Step::Removal(item), Ok(r)) => {
                self.busy = false;
                self.checking = None;
                let dependents: Vec<String> = items(&r, "dependents")
                    .iter()
                    .map(|d| {
                        let n = d["items"].as_u64().unwrap_or(0);
                        match (d["kind"].as_str().unwrap_or_default(), d["name"].as_str().unwrap_or_default()) {
                            ("own", _) => format!("{n} of your own"),
                            (_, name) if n > 0 => format!("{name} ({n} {})", if n == 1 { "item" } else { "items" }),
                            (_, name) => name.to_string(),
                        }
                    })
                    .collect();
                let unused = items(&r, "unused")
                    .iter()
                    .map(|u| (u["id"].as_str().unwrap_or_default().to_string(), format!("{} ({})", u["name"].as_str().unwrap_or_default(), size_text(u["bytes"].as_u64().unwrap_or(0)))))
                    .collect();
                let title = format!("Remove {}?", item.name);
                c.notice = Some(match dependents.is_empty() {
                    true => (title, "It is removed from this device.".into(), vec!["Remove".into(), "Cancel".into()]),
                    false => (title, format!("Used by: {}. What uses it will not work until it is installed again.", dependents.join(", ")), vec!["Cancel".into(), "Remove anyway".into()]),
                });
                self.asking = Some(Asking { item, used: !dependents.is_empty(), unused, second: false });
            }
            (Step::Removal(item), Err(e)) => {
                self.rest();
                self.checking = None;
                c.notice = Some((format!("Remove {}?", item.name), format!("What uses it could not be read: {}", e.message), vec!["OK".into()]));
                self.refused = (item.id, e.message);
            }
            (Step::Uninstall(item), Err(e)) => {
                self.rest();
                self.refused = (item.id, e.message);
            }
            (Step::Uninstall(item), Ok(_)) => {
                self.removing = Some((item, 0));
                self.read(c);
            }
        }
    }
}

impl Page for Installed {
    fn open(&mut self, c: &mut Ctx) {
        self.asking = None;
        self.checking = None;
        self.rest();
        self.refused = (String::new(), String::new());
        self.detail = None;
        self.read(c);
    }

    fn noun(&self) -> &'static str {
        "Installed"
    }

    fn rows(&self) -> Vec<SettingData> {
        let faint = |r: SettingData| if self.busy { crate::settings_app::faint(r) } else { r };
        let list = self.list();
        if list.is_empty() {
            return vec![faint(row("Nothing installed", ""))];
        }
        list.iter().map(|i| faint(row(&i.name, format!("{} {}", kind_word(&i.kind), i.version).trim_end()))).collect()
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let list = self.list();
        let under = at.and_then(|a| list.get(a));
        let mut lines = vec![];
        if !self.package_error.is_empty() {
            lines.push(head("The packages could not be read"));
            lines.extend(failed(&self.package_error));
        }
        if !self.libraries_error.is_empty() {
            lines.push(head("The libraries could not be read"));
            lines.extend(failed(&self.libraries_error));
        }
        if !self.progress.is_empty() {
            lines.push(head("Now"));
            lines.extend(said(&self.progress));
        }
        let (whose, why) = &self.refused;
        if !why.is_empty() && under.is_some_and(|i| i.id == *whose) {
            lines.push(head("Not done"));
            lines.extend(failed(why));
        }
        if let Some(i) = self.detail.as_ref().or(under) {
            lines.extend(item_lines(i, self.detail.is_some()));
        }
        lines
    }

    fn press(&mut self, at: usize, _c: &mut Ctx) {
        // A row confirmed opens its detail, which Back closes; it is removed by the bar's Remove.
        self.detail = self.list().get(at).cloned();
    }

    fn path(&self) -> Vec<String> {
        self.detail.iter().map(|i| i.name.clone()).collect()
    }

    fn back(&mut self, _c: &mut Ctx) -> bool {
        self.detail.take().is_some()
    }

    fn tools(&self, at: Option<usize>) -> Vec<Tool> {
        vec![Tool { label: "Remove...".into(), enabled: at.is_some_and(|a| a < self.list().len()) && !self.busy, on: false }]
    }

    fn tool(&mut self, i: usize, at: Option<usize>, c: &mut Ctx) {
        if i != 0 || self.busy {
            return;
        }
        let Some(item) = at.and_then(|a| self.list().get(a).cloned()) else { return };
        // What removing it means is asked first: the question opens at once and its answer words it.
        self.busy = true;
        self.refused = (String::new(), String::new());
        self.checking = Some(item.clone());
        c.notice = Some((format!("Remove {}?", item.name), "Checking what uses it...".into(), vec!["Cancel".into()]));
        c.ask(Any::Installed(Step::Removal(item.clone())), "packages.removal", item.params());
    }

    fn noticed(&mut self, choice: Option<usize>, c: &mut Ctx) {
        if self.checking.take().is_some() {
            self.busy = false; // Cancel before the answer
            return;
        }
        let Some(mut a) = self.asking.take() else { return };
        // Cancel, or the question closed: nothing happens.
        match (a.second, a.used, choice) {
            (false, true, Some(1)) | (false, false, Some(0)) | (true, _, Some(1)) => {}
            (true, _, Some(0)) => a.unused.clear(), // Keep them
            _ => return,
        }
        self.go_on(a, c);
    }

    fn message(&self) -> String {
        if self.busy && !self.progress.is_empty() {
            self.progress.clone()
        } else {
            String::new()
        }
    }
}

/// An item's panel, with the signing keys' fingerprints in its detail: what the engine reports of it (no license, platforms or trust flag).
fn item_lines(i: &Item, detail: bool) -> Vec<InfoLine> {
    let wrapped = |text: &str| crate::library::files::wrap_path(text, crate::library::INFO_WIDTH).iter().map(|l| line(l)).collect::<Vec<_>>();
    let mut lines = vec![head(kind_word(&i.kind))];
    lines.extend(wrapped(&i.name));
    if i.name != i.id {
        lines.push(head("ID"));
        lines.extend(wrapped(&i.id));
    }
    lines.extend([head("Version"), line(if i.version.is_empty() { "—" } else { &i.version })]);
    if let Some(n) = i.items {
        lines.extend([head("Items"), line(&n.to_string())]);
    }
    if i.package {
        lines.push(head("Publisher"));
        lines.extend(wrapped(if i.repository.is_empty() { "installed from a file" } else { &i.repository }));
    }
    if detail && !i.signers.is_empty() {
        lines.push(head("Key fingerprint"));
        lines.extend(i.signers.iter().map(|id| line(&fingerprint(id))));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kind_word_picks_the_word_for_a_kind() {
        assert_eq!(kind_word("plugin"), "Plugin");
        assert_eq!(kind_word("asset"), "Samples");
        assert_eq!(kind_word("library"), "Library");
        assert_eq!(kind_word(""), "Library", "anything else is a library");
        assert_eq!(kind_word("sound"), "Library");
    }

    #[test]
    fn items_reads_an_array_and_defaults_on_missing_or_wrong() {
        let r = json!({"p": [1, 2], "s": "x", "n": null});
        assert_eq!(items(&r, "p"), vec![json!(1), json!(2)]);
        assert!(items(&r, "absent").is_empty());
        assert!(items(&r, "s").is_empty(), "a string is not an array");
        assert!(items(&r, "n").is_empty(), "null is not an array");
    }

    #[test]
    fn item_params_name_a_package_for_removal() {
        let plain = Item { id: "p".into(), kind: "asset".into(), version: "3".into(), ..Default::default() };
        assert_eq!(plain.params(), json!({"id": "p"}));
        let plugin = Item { id: "p".into(), kind: "plugin".into(), version: "1.0".into(), ..Default::default() };
        assert_eq!(plugin.params(), json!({"id": "p", "version": "1.0"}), "a plugin names its version");
        let bare = Item { id: "p".into(), kind: "plugin".into(), version: String::new(), ..Default::default() };
        assert_eq!(bare.params(), json!({"id": "p"}), "a plugin with no version names only the id");
    }

    #[test]
    fn item_is_the_same_package_still_listed() {
        let plain = |v: &str| Item { id: "p".into(), kind: "asset".into(), version: v.into(), ..Default::default() };
        assert!(plain("1").is(&plain("1")));
        assert!(plain("1").is(&plain("9")), "a non-plugin is the same whatever the version");
        let plug = |v: &str| Item { id: "p".into(), kind: "plugin".into(), version: v.into(), ..Default::default() };
        assert!(plug("1").is(&plug("1")));
        assert!(!plug("1").is(&plug("2")), "a plugin's versions install side by side");
    }

    #[test]
    fn list_merges_a_library_into_its_package_and_sorts_clearly() {
        let s = Installed {
            packages: vec![
                Item { id: "a".into(), kind: "library".into(), name: "Alpha".into(), version: "1".into(), ..Default::default() },
                Item { id: "b".into(), kind: "library".into(), name: "b".into(), version: "1".into(), ..Default::default() },
            ],
            libraries: vec![
                Item { id: "a".into(), kind: "library".into(), name: "Alpha".into(), items: Some(4), ..Default::default() },
                Item { id: "b".into(), kind: "library".into(), name: "Beta".into(), items: Some(7), ..Default::default() },
                Item { id: "c".into(), kind: "library".into(), name: "Gamma".into(), items: Some(2), ..Default::default() },
            ],
            ..Default::default()
        };
        let all = s.list();
        assert_eq!(all.len(), 3, "a library with no package is listed too");
        assert_eq!((all[0].id.as_str(), all[0].name.as_str(), all[0].items), ("a", "Alpha", Some(4)), "the package keeps its name and takes the item count");
        assert_eq!((all[1].id.as_str(), all[1].name.as_str(), all[1].items), ("b", "Beta", Some(7)), "a package named by its id shows the library's name");
        assert_eq!(all[2].id, "c");
    }

    #[test]
    fn list_is_empty_with_nothing_installed() {
        assert!(Installed::default().list().is_empty());
    }

    #[test]
    fn panel_shows_a_libraries_detail_in_full() {
        let i = Item { id: "p1".into(), version: "".into(), kind: "plugin".into(), name: "MyPack".into(), repository: String::new(), signers: vec!["abcdefgh".into(), "ijkl".into()], items: None, package: true };
        let lines = item_lines(&i, true);
        let line_text = |n: usize| lines[n].text.as_str();
        assert!(lines[0].head && line_text(0) == "Plugin");
        assert_eq!(line_text(1), "MyPack");
        assert!(lines[2].head && line_text(2) == "ID");
        assert_eq!(line_text(3), "p1");
        assert!(lines[4].head && line_text(4) == "Version");
        assert_eq!(line_text(5), "—", "a package with no version shows a dash");
        assert!(lines[6].head && line_text(6) == "Publisher");
        assert_eq!(line_text(7), "installed from a file");
        assert!(lines[8].head && line_text(8) == "Key fingerprint");
        assert_eq!(line_text(9), "abcd efgh");
        assert_eq!(line_text(10), "ijkl");
    }
}
