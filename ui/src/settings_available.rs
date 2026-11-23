// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Available page (ui/SETTINGS-APP.md P1): what the enabled repositories offer, read once when the
//! page is shown and on the bar's Refresh, nothing at rest. Each enabled repository is read as settings_repos.rs reads
//! one (`packages.list` is a job in the engine, asked about every 100 ms until it ends), one after another, and the
//! listings are joined with what is installed: one alphabetical list by name (the id when two share one), each row its
//! name, the word for its kind as the Installed page writes it, the newest version offered, and a state word:
//! "Installed" when that version is installed, "Update from X" when an older version X is installed, nothing when it
//! is not installed. A repository that fails to read shows the app's usual failed row with the engine's words and the
//! rest still lists.

use crate::client::Error;
use crate::library::older;
use crate::settings_app::{failed, head, line, row, said, Ctx, Page, Step as Any};
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

/// A package's kind, from the offered version's `types`, as the manifest's own `kind` and the Installed page decide it:
/// a version holding plugin builds is a plugin; one holding an asset item, an asset; else a library.
fn kind(types: &Value) -> &'static str {
    let count = |k: &str| types.get(k).and_then(Value::as_u64).unwrap_or(0);
    if count("clap") > 0 || count("lv2") > 0 {
        "plugin"
    } else if count("oscairn.asset") > 0 {
        "asset"
    } else {
        "library"
    }
}

/// One row of the list.
#[derive(Clone, Debug, PartialEq)]
struct Offer {
    name: String,
    id: String,
    kind_word: &'static str,
    version: String,
    /// "Installed", "Update from X", or empty when it is not installed.
    state: String,
    /// The repository that offers it, for an install's `source` (I2).
    source: String,
}

#[derive(Default)]
pub struct Available {
    /// The enabled repositories still to read.
    queue: Vec<Repo>,
    /// Each enabled repository's listing, in the order read: address, packages.
    listings: Vec<(String, Vec<Value>)>,
    /// The newest version installed of each package, by id.
    installed: BTreeMap<String, String>,
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

    /// The rows: each package once (the first repository offering it wins), by name whatever the case, then by id.
    fn list(&self) -> Vec<Offer> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (source, packages) in &self.listings {
            for p in packages {
                let id = p["id"].as_str().unwrap_or_default().to_string();
                if !seen.insert(id.clone()) {
                    continue;
                }
                let versions = p["versions"].as_array().cloned().unwrap_or_default();
                // The newest version offered, whatever its channel.
                let newest = versions.iter().fold(None::<&Value>, |best, v| {
                    if best.is_none_or(|b| older(b["version"].as_str().unwrap_or_default(), v["version"].as_str().unwrap_or_default())) {
                        Some(v)
                    } else {
                        best
                    }
                });
                let Some(newest) = newest else { continue };
                let version = newest["version"].as_str().unwrap_or_default().to_string();
                let name = {
                    let n = p["name"].as_str().unwrap_or_default();
                    if n.is_empty() { id.clone() } else { n.to_string() }
                };
                let state = match self.installed.get(&id) {
                    Some(have) if !have.is_empty() && *have == version => "Installed".to_string(),
                    Some(have) if !have.is_empty() && older(have, &version) => format!("Update from {have}"),
                    _ => String::new(),
                };
                out.push(Offer { name, id, kind_word: kind_word(kind(&newest["types"])), version, state, source: source.clone() });
            }
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)).then_with(|| a.version.cmp(&b.version)));
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
                self.listings.push((repo.address, job["packages"].as_array().cloned().unwrap_or_default()));
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
            return vec![crate::settings_app::faint(row("Nothing available", ""))];
        }
        list.iter()
            .map(|o| {
                let v = match o.state.is_empty() {
                    true => format!("{} {}", o.kind_word, o.version),
                    false => format!("{} {}  ·  {}", o.kind_word, o.version, o.state),
                };
                row(&o.name, v)
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
            lines.push(head(&o.name));
            let mut v = format!("{} {}", o.kind_word, o.version);
            if !o.state.is_empty() {
                v += &format!("  ·  {}", o.state);
            }
            lines.extend(said(&v));
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
                let mut m: BTreeMap<String, String> = BTreeMap::new();
                for p in r["packages"].as_array().into_iter().flatten() {
                    let (id, version) = (p["id"].as_str().unwrap_or_default().to_string(), p["version"].as_str().unwrap_or_default().to_string());
                    match m.get(&id) {
                        Some(have) if !older(have, &version) => {}
                        _ => {
                            m.insert(id, version);
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
