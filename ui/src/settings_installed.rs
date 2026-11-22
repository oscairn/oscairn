// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Installed page (ui/SETTINGS-APP.md P, the reference's `notBuilt` "Installed content" page, RFC
//! 0006): what is installed on this device, read once when the page is shown and never on a timer — the packages
//! `packages.installed` reports (each signed and from a repository where the handler says so) and the libraries
//! `library.libraries` (by name and their item count), in two sections under shared heading rows. A long name is cut
//! with an ellipsis in its row, whole on the panel. A refusal or a lost connection is the app's usual failed row.

use crate::client::Error;
use crate::settings_app::{failed, faint, head, line, row, said, Ctx, Page, Step as Any};
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

    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, _c: &mut Ctx) {
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
                self.libraries_error.clear();
                self.libraries = r["libraries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|x| Library { id: x["id"].as_str().unwrap_or_default().to_string(), name: x["name"].as_str().unwrap_or_default().to_string(), items: x["items"].as_u64().unwrap_or(0) })
                    .collect();
            }
            (Step::Libraries, Err(e)) => {
                self.libraries_error = e.message;
            }
        }
    }
}

impl Page for Installed {
    fn open(&mut self, c: &mut Ctx) {
        self.read(c);
    }

    fn noun(&self) -> &'static str {
        "Installed"
    }

    fn rows(&self) -> Vec<SettingData> {
        self.rows_rows()
            .into_iter()
            .map(|r| match r {
                Row::PackagesHead | Row::LibrariesHead => faint(row(heading(r), "")),
                Row::NonePackages => row("No packages installed", ""),
                Row::NoneLibraries => row("No libraries", ""),
                Row::Package(i) => row(cut(&self.packages[i].id), self.packages[i].version.as_str()),
                Row::Library(i) => row(cut(&self.libraries[i].name), format!("{} {}", self.libraries[i].items, if self.libraries[i].items == 1 { "item" } else { "items" })),
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

    fn press(&mut self, _at: usize, _c: &mut Ctx) {}
}

fn heading(r: Row) -> String {
    match r {
        Row::PackagesHead => "Packages".to_string(),
        Row::LibrariesHead => "Libraries".to_string(),
        _ => String::new(),
    }
}
