// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Available page (ui/SETTINGS-APP.md P4): what the enabled repositories offer, read once when the
//! page is shown and on the bar's Refresh, nothing at rest. Each enabled repository is read as settings_repos.rs reads
//! one (`packages.list` is a job in the engine, asked about every 100 ms until it ends), one after another, and the
//! listings are joined with what is installed as the Library's Catalog joins them (catalog.rs `offers`): one
//! alphabetical list by name (the id when two share one), each row its name, the word for its kind as the Installed
//! page writes it, the version the engine would install (the newest stable one this device can, not yanked or
//! withdrawn; else the newest, faint, the panel saying why), and a state word: "Installed" when it is installed,
//! "Update from X" when an older version X is installed, nothing when it is not installed. A repository that fails to
//! read shows the app's usual failed row with the engine's words and the rest still lists.

use crate::client::Error;
use crate::library::{offers, older, why_not, InstalledPackage, Listing, Package};
use crate::settings_app::{failed, faint, head, line, row, said, Ctx, Page, Step as Any};
use crate::settings_installed::kind_word;
use crate::topbar::Tool;
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// packages.repositories.
    Repos,
    /// packages.installed.
    Installed,
    /// packages.list of one repository: the job.
    Listed(Repo),
    /// packages.job of that listing.
    Poll(Repo, u64),
}

/// A repository to read: its address and this device's name for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub address: String,
    pub name: String,
}

/// One row of the list: the Catalog's offer, its kind word and its state word.
#[derive(Clone, Debug, PartialEq)]
struct Offer {
    offer: crate::library::Offer,
    kind_word: &'static str,
    /// "Installed", "Update from X", or empty when it is not installed.
    state: String,
}

impl Offer {
    fn new(offer: crate::library::Offer) -> Offer {
        // What is installed says its kind (`packages.installed` `kind`). A listing carries none, so it is read from what
        // the version holds: a plugin package holds only plugin builds, an asset package only asset files
        // (control-protocol.md, "Libraries and packages"); anything else installs as a library.
        let held: Vec<&str> = offer.version.types.iter().filter(|(_, n)| *n > 0).map(|(t, _)| t.as_str()).collect();
        let only = |kinds: &[&str]| !held.is_empty() && held.iter().all(|t| kinds.contains(t));
        let kind = match &offer.installed {
            Some(_) if !offer.record.kind.is_empty() => offer.record.kind.as_str(),
            _ if only(&["clap", "lv2"]) => "plugin",
            _ if only(&["oscairn.asset"]) => "asset",
            _ => "library",
        };
        let state = match &offer.installed {
            Some(have) if offer.update => format!("Update from {have}"),
            Some(_) => "Installed".into(),
            None => String::new(),
        };
        Offer { kind_word: kind_word(kind), state, offer }
    }

    fn name(&self) -> &str {
        self.offer.title()
    }

    fn value(&self) -> String {
        match self.state.is_empty() {
            true => format!("{} {}", self.kind_word, self.offer.version.version),
            false => format!("{} {}  ·  {}", self.kind_word, self.offer.version.version, self.state),
        }
    }

    /// Nothing here can install it and it is not installed.
    fn off(&self) -> bool {
        !self.offer.installable && self.offer.installed.is_none()
    }
}

#[derive(Default)]
pub struct Available {
    /// The enabled repositories still to read.
    queue: Vec<Repo>,
    /// Each enabled repository's listing, in the order read: its address, and this device's name for it and its packages.
    listings: Vec<(String, Listing)>,
    /// The newest version installed of each package, by id.
    installed: BTreeMap<String, InstalledPackage>,
    repos_error: String,
    installed_error: String,
    /// A repository that failed to read: "NAME (engine's words)".
    failed: Vec<String>,
    reading: bool,
    /// A listing job is out.
    active: bool,
    /// "Reading NAME...".
    progress: String,
}

impl Available {
    /// The page is read again: what it shows cleared.
    fn reset(&mut self) {
        self.queue.clear();
        self.listings.clear();
        self.failed.clear();
        self.repos_error.clear();
        self.installed_error.clear();
        self.installed.clear();
        self.reading = true;
        self.active = false;
        self.progress.clear();
    }

    /// The rows: the Catalog's offers, each package once (one this device can install over one it cannot, then the newer
    /// version, then the first repository read), by name whatever the case, then by id.
    fn list(&self) -> Vec<Offer> {
        let mut out: Vec<Offer> = Vec::new();
        // shortcut: no plugin scan here, so a plugin that comes with the app is not "Built in" as in the Catalog.
        for o in offers(&self.listings, &self.installed, &BTreeMap::new()) {
            match out.iter_mut().find(|x| x.offer.package.id == o.package.id) {
                Some(x) if (!x.offer.installable && o.installable) || (x.offer.installable == o.installable && older(&x.offer.version.version, &o.version.version)) => *x = Offer::new(o),
                Some(_) => {}
                None => out.push(Offer::new(o)),
            }
        }
        out.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()).then_with(|| a.offer.package.id.cmp(&b.offer.package.id)));
        out
    }

    /// The next enabled repository, else the end of the reading.
    fn read_next(&mut self, c: &mut Ctx) {
        if self.queue.is_empty() {
            self.active = false;
            self.reading = false;
            self.progress.clear();
            return;
        }
        let repo = self.queue.remove(0);
        self.active = true;
        self.progress = format!("Reading {}...", repo.name);
        c.ask(Any::Available(Step::Listed(repo.clone())), "packages.list", json!({"source": repo.address}));
    }

    /// The end of a listing job: still running, or what it came to.
    fn polled(&mut self, repo: Repo, job: Value, c: &mut Ctx) {
        match job["state"].as_str().unwrap_or_default() {
            "running" => c.ask_after(Duration::from_millis(100), Any::Available(Step::Poll(repo, job["job"].as_u64().unwrap_or(0))), "packages.job", json!({"job": job["job"]})),
            "done" => {
                let packages = job["packages"].as_array().into_iter().flatten().map(Package::from_json).collect();
                self.listings.push((repo.address, Listing { name: repo.name, packages }));
                self.active = false;
                self.read_next(c);
            }
            _ => {
                self.failed.push(format!("{} ({})", repo.name, job["error"].as_str().unwrap_or_default()));
                self.active = false;
                self.read_next(c);
            }
        }
    }
}

impl Page for Available {
    fn open(&mut self, c: &mut Ctx) {
        self.reset();
        c.ask(Any::Available(Step::Repos), "packages.repositories", json!({}));
        c.ask(Any::Available(Step::Installed), "packages.installed", json!({}));
    }

    fn noun(&self) -> &'static str {
        "Available"
    }

    fn rows(&self) -> Vec<SettingData> {
        let list = self.list();
        if list.is_empty() {
            // A placeholder row, so the list is never empty and the rail past it is still reached (as Installed's is).
            return vec![faint(row("Nothing available", ""))];
        }
        list.iter()
            .map(|o| {
                let r = row(o.name(), o.value());
                if o.off() { faint(r) } else { r }
            })
            .collect()
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let mut lines = vec![];
        if !self.repos_error.is_empty() {
            lines.push(head("The repositories could not be read"));
            lines.extend(failed(&self.repos_error));
        }
        if !self.installed_error.is_empty() {
            lines.push(head("The installed packages could not be read"));
            lines.extend(failed(&self.installed_error));
        }
        for f in &self.failed {
            lines.push(head("Not read"));
            lines.extend(failed(f));
        }
        if !self.progress.is_empty() {
            lines.push(head("Now"));
            lines.extend(said(&self.progress));
        }
        let list = self.list();
        if let Some(o) = at.and_then(|a| list.get(a)) {
            lines.push(head(o.name()));
            lines.extend(said(&o.value()));
            if !o.offer.repository.is_empty() {
                lines.extend(said(&format!("From {}", o.offer.repository)));
            }
            if o.off() {
                lines.extend(said(&format!("Cannot be installed here: {}", why_not(&o.offer.version))));
            }
        }
        if lines.is_empty() {
            lines.push(line(""));
        }
        lines
    }

    fn message(&self) -> String {
        if (self.reading || self.active) && !self.progress.is_empty() {
            self.progress.clone()
        } else {
            String::new()
        }
    }

    fn tools(&self, _at: Option<usize>) -> Vec<Tool> {
        vec![Tool { label: "Refresh".into(), enabled: !self.reading && !self.active, on: false }]
    }

    fn tool(&mut self, i: usize, _at: Option<usize>, c: &mut Ctx) {
        if i != 0 {
            return;
        }
        self.reset();
        c.ask(Any::Available(Step::Repos), "packages.repositories", json!({}));
        c.ask(Any::Available(Step::Installed), "packages.installed", json!({}));
    }

    fn press(&mut self, _at: usize, _c: &mut Ctx) {}
}

impl Available {
    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, c: &mut Ctx) {
        match (step, result) {
            (Step::Repos, Ok(r)) => {
                self.queue = r["repositories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|x| x["enabled"] == true)
                    .map(|x| Repo { address: x["address"].as_str().unwrap_or_default().to_string(), name: x["name"].as_str().unwrap_or_default().to_string() })
                    .collect();
                self.read_next(c);
            }
            (Step::Repos, Err(e)) => {
                self.repos_error = e.message;
                self.reading = false;
            }
            (Step::Installed, Ok(r)) => {
                self.installed_error.clear();
                // The newest version installed of each package, as the Catalog keeps it.
                let mut m: BTreeMap<String, InstalledPackage> = BTreeMap::new();
                for p in r["packages"].as_array().into_iter().flatten().map(InstalledPackage::from_json) {
                    match m.get(&p.id) {
                        Some(have) if !older(&have.version, &p.version) => {}
                        _ => {
                            m.insert(p.id.clone(), p);
                        }
                    }
                }
                self.installed = m;
            }
            (Step::Installed, Err(e)) => {
                self.installed_error = e.message;
            }
            (Step::Listed(repo), Ok(r)) => match r["job"].as_u64() {
                Some(job) => c.ask_after(Duration::from_millis(100), Any::Available(Step::Poll(repo, job)), "packages.job", json!({"job": job})),
                None => {
                    self.failed.push(format!("{} (the engine gave no job)", repo.name));
                    self.active = false;
                    self.read_next(c);
                }
            },
            (Step::Listed(repo), Err(e)) => {
                self.failed.push(format!("{} ({})", repo.name, e.message));
                self.active = false;
                self.read_next(c);
            }
            (Step::Poll(repo, job_id), result) => {
                let state = match &result {
                    Ok(r) => r.clone(),
                    Err(e) => json!({"state": "failed", "error": e.message}),
                };
                if state["state"] == "running" {
                    c.ask_after(Duration::from_millis(100), Any::Available(Step::Poll(repo.clone(), job_id)), "packages.job", json!({"job": job_id}));
                } else {
                    self.polled(repo, state, c);
                }
            }
        }
    }
}
