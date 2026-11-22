// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Installed page (ui/SETTINGS-APP.md P, the reference's `notBuilt` "Installed content" page, RFC
//! 0006): what is installed on this device, read once when the page is shown and never on a timer — the packages
//! `packages.installed` reports (each signed and from a repository where the handler says so) and the libraries
//! `library.libraries` (by name and their item count), in two sections under shared heading rows. A long name is cut
//! with an ellipsis in its row, whole on the panel. A refusal or a lost connection is the app's usual failed row.
//! Confirming a package row opens its detail in the panel (id, version, the publisher and the key fingerprint of what
//! signed it; Back closes it on the same row), and a library row is removed with the bar's Remove..., asked first as
//! the repositories page asks before Remove; the protocol reports no library that cannot be removed, so every one is
//! offered, and no license, platforms, size or dependency list for a package.

use crate::client::Error;
use crate::library::fingerprint;
use crate::settings_app::{failed, head, line, row, said, Ctx, Page, Step as Any};
use crate::topbar::Tool;
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};

/// A name too wide for a row is cut with an ellipsis; the panel keeps the whole one.
fn cut(name: &str) -> String {
    const WIDTH: usize = 34;
    if name.chars().count() <= WIDTH {
        name.to_string()
    } else {
        let mut s: String = name.chars().take(WIDTH - 1).collect();
        s.push('…');
        s
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// packages.installed.
    Installed,
    /// library.libraries.
    Libraries,
    /// library.uninstall, of the library's id.
    Uninstall(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Package {
    pub id: String,
    pub version: String,
    /// The repository it came from, empty for one installed from a file.
    pub repository: String,
    /// The IDs of the keys that signed it, empty when unknown.
    pub signers: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub items: u64,
}

#[derive(Default)]
pub struct Installed {
    packages: Vec<Package>,
    libraries: Vec<Library>,
    package_error: String,
    libraries_error: String,
    /// The library whose Remove is being asked about.
    asking: Option<String>,
    /// While `library.uninstall` runs, or the list is read back afterwards: every row faint.
    busy: bool,
    /// "Removing NAME..." while it runs.
    progress: String,
    /// A removal refused: the library and what the engine said.
    refused: (String, String),
    /// The package whose detail the panel shows, opened by a press on its row; None: the panel follows the cursor.
    detail: Option<usize>,
}

/// What the row at `index` of the list is: a section's heading, a package, a library, or the page's plain row.
#[derive(Clone, Debug, Copy, PartialEq)]
enum Row {
    PackagesHead,
    Package(usize),
    NonePackages,
    LibrariesHead,
    Library(usize),
    NoneLibraries,
}

impl Installed {
    /// The library a row is, if it is one.
    fn at(&self, at: usize) -> Option<&Library> {
        match self.rows_rows().get(at) {
            Some(Row::Library(i)) => self.libraries.get(*i),
            _ => None,
        }
    }

    /// The rows, in the order they show: the packages under their heading, the libraries under theirs.
    fn rows_rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::PackagesHead];
        if self.packages.is_empty() {
            rows.push(Row::NonePackages);
        } else {
            rows.extend(self.packages.iter().enumerate().map(|(i, _)| Row::Package(i)));
        }
        rows.push(Row::LibrariesHead);
        if self.libraries.is_empty() {
            rows.push(Row::NoneLibraries);
        } else {
            rows.extend(self.libraries.iter().enumerate().map(|(i, _)| Row::Library(i)));
        }
        rows
    }

    fn read(&mut self, c: &mut Ctx) {
        self.package_error.clear();
        self.libraries_error.clear();
        c.ask(Any::Installed(Step::Installed), "packages.installed", json!({}));
        c.ask(Any::Installed(Step::Libraries), "library.libraries", json!({}));
    }

    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, c: &mut Ctx) {
        match (step, result) {
            (Step::Installed, Ok(r)) => {
                self.package_error.clear();
                self.packages = r["packages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|x| {
                        let text = |k: &str| x[k].as_str().unwrap_or_default().to_string();
                        Package { id: text("id"), version: text("version"), repository: text("repository"), signers: x["signers"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect() }
                    })
                    .collect();
            }
            (Step::Installed, Err(e)) => {
                self.package_error = e.message;
            }
            (Step::Libraries, Ok(r)) => {
                self.busy = false;
                self.libraries_error.clear();
                self.libraries = r["libraries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|x| Library { id: x["id"].as_str().unwrap_or_default().to_string(), name: x["name"].as_str().unwrap_or_default().to_string(), items: x["items"].as_u64().unwrap_or(0) })
                    .collect();
            }
            (Step::Libraries, Err(e)) => {
                self.busy = false;
                self.libraries_error = e.message;
            }
            (Step::Uninstall(id), result) => {
                self.progress.clear();
                match result {
                    // Removed: the list is read again, as the engine kept it.
                    Ok(_) => c.ask(Any::Installed(Step::Libraries), "library.libraries", json!({})),
                    Err(e) => {
                        self.busy = false;
                        self.refused = (id, e.message);
                    }
                }
            }
        }
    }
}

impl Page for Installed {
    fn open(&mut self, c: &mut Ctx) {
        self.asking = None;
        self.progress.clear();
        self.refused = (String::new(), String::new());
        self.detail = None;
        self.read(c);
    }

    fn noun(&self) -> &'static str {
        "Installed"
    }

    fn rows(&self) -> Vec<SettingData> {
        let faint = |r: SettingData| if self.busy { crate::settings_app::faint(r) } else { r };
        self.rows_rows()
            .into_iter()
            .map(|r| match r {
                Row::PackagesHead | Row::LibrariesHead => faint(row(heading(r), "")),
                Row::NonePackages => faint(row("No packages installed", "")),
                Row::NoneLibraries => faint(row("No libraries", "")),
                Row::Package(i) => faint(row(cut(&self.packages[i].id), self.packages[i].version.as_str())),
                Row::Library(i) => faint(row(cut(&self.libraries[i].name), format!("{} {}", self.libraries[i].items, if self.libraries[i].items == 1 { "item" } else { "items" }))),
            })
            .collect()
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
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
        if !why.is_empty() && (whose.is_empty() || at.and_then(|a| self.at(a)).is_some_and(|l| l.id == *whose)) {
            lines.push(head("Not done"));
            lines.extend(failed(why));
        }
        if let Some(i) = self.detail {
            // A package's detail, opened by a press on its row and shown until Back: id, version, the publisher (the
            // repository it came from), the key fingerprint of what signed it and whether it is trusted. `packages.installed`
            // reports no license, platforms, size or dependency list, so the protocol shows no more.
            if let Some(p) = self.packages.get(i) {
                lines.push(head(&p.id));
                lines.push(head("Version"));
                lines.push(line(if p.version.is_empty() { "—" } else { &p.version }));
                lines.push(head("Publisher"));
                lines.extend(crate::library::files::wrap_path(if p.repository.is_empty() { "installed from a file" } else { &p.repository }, crate::library::INFO_WIDTH).iter().map(|l| line(l)));
                lines.push(head("Trusted"));
                // The handler reports the keys that signed it and the repository it came from, not a trust flag: a
                // package is trusted when it was verified at install — signed, or from a repository whose root key is
                // trusted (the two cases that let it install).
                lines.push(line(if p.signers.is_empty() && p.repository.is_empty() { "Not verified" } else { "Yes" }));
                if !p.signers.is_empty() {
                    lines.push(head("Key fingerprint"));
                    lines.extend(p.signers.iter().map(|id| line(&fingerprint(id))));
                }
                return lines;
            }
        }
        let Some(at) = at else { return lines };
        match self.rows_rows().get(at) {
            Some(Row::PackagesHead) => {
                lines.push(head("Packages"));
                lines.extend(said("The plugin and asset packages installed on this device, and the packages installed as locked libraries, as `packages.installed` reports them."));
            }
            Some(Row::LibrariesHead) => {
                lines.push(head("Libraries"));
                lines.extend(said("The libraries installed, as `library.libraries` reports them, and their item count."));
            }
            Some(Row::Package(i)) => {
                let p = &self.packages[*i];
                lines.push(head(&p.id));
                lines.push(head("Version"));
                lines.push(line(if p.version.is_empty() { "—" } else { &p.version }));
                lines.push(head("Publisher"));
                lines.extend(crate::library::files::wrap_path(if p.repository.is_empty() { "installed from a file" } else { &p.repository }, crate::library::INFO_WIDTH).iter().map(|l| line(l)));
                lines.push(head("Trusted"));
                // The handler reports the keys that signed it and the repository it came from, not a trust flag: a
                // package is trusted when it was verified at install — signed, or from a repository whose root key is
                // trusted (the two cases that let it install).
                lines.push(line(if p.signers.is_empty() && p.repository.is_empty() { "Not verified" } else { "Yes" }));
            }
            Some(Row::Library(i)) => {
                let l = &self.libraries[*i];
                lines.push(head(&l.name));
                lines.push(head("Items"));
                lines.push(line(&l.items.to_string()));
            }
            Some(Row::NonePackages) if self.package_error.is_empty() => {
                lines.push(head("No packages installed"));
            }
            Some(Row::NoneLibraries) if self.libraries_error.is_empty() => {
                lines.push(head("No libraries installed"));
            }
            _ => {}
        }
        lines
    }

    fn press(&mut self, at: usize, _c: &mut Ctx) {
        // A package row confirmed opens its detail, which Back closes; a library row is changed by the bar's Remove.
        if let Some(Row::Package(i)) = self.rows_rows().get(at) {
            self.detail = Some(*i);
        }
    }

    fn path(&self) -> Vec<String> {
        match &self.detail {
            Some(i) => vec![self.packages.get(*i).map_or_else(String::new, |p| p.id.clone())],
            None => vec![],
        }
    }

    fn back(&mut self, _c: &mut Ctx) -> bool {
        self.detail.take().is_some()
    }

    fn tools(&self, at: Option<usize>) -> Vec<Tool> {
        vec![Tool { label: "Remove...".into(), enabled: at.and_then(|a| self.at(a)).is_some() && !self.busy, on: false }]
    }

    fn tool(&mut self, i: usize, at: Option<usize>, c: &mut Ctx) {
        if i != 0 || self.busy {
            return;
        }
        let Some(lib) = at.and_then(|a| self.at(a)).cloned() else { return };
        c.notice = Some((format!("Remove {}?", lib.name), "Its items are removed from this device with it.".into(), vec!["Remove".into(), "Cancel".into()]));
        self.asking = Some(lib.id);
    }

    fn noticed(&mut self, choice: Option<usize>, c: &mut Ctx) {
        let Some(id) = self.asking.take() else { return };
        let Some(lib) = self.libraries.iter().find(|l| l.id == id).cloned() else { return };
        if choice != Some(0) {
            // Cancel: nothing happens; the question just closes.
            return;
        }
        self.busy = true;
        self.progress = format!("Removing {}...", lib.name);
        self.refused = (id.clone(), String::new());
        c.ask(Any::Installed(Step::Uninstall(id.clone())), "library.uninstall", json!({"library": id}));
    }

    fn message(&self) -> String {
        if self.busy && !self.progress.is_empty() {
            self.progress.clone()
        } else {
            String::new()
        }
    }
}

fn heading(r: Row) -> String {
    match r {
        Row::PackagesHead => "Packages".to_string(),
        Row::LibrariesHead => "Libraries".to_string(),
        _ => String::new(),
    }
}
