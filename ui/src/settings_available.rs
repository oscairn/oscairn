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
use crate::library::files::size_text;
use crate::library::{install_ended, install_params, offers, older, trust_params, why_not, CodeQuestion, Ended, InstallCtx, InstalledPackage, Listing, Package};
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
    /// packages.install of the offer under the cursor.
    Install(InstallCtx),
    /// packages.job of that install.
    InstallPoll(InstallCtx, u64),
    /// trust.list, before the keys the code question named are trusted.
    TrustList(InstallCtx, Vec<(String, String)>),
    /// trust.set of one of them.
    TrustSet(InstallCtx),
}

/// The question out: an install or update, or the code this device does not trust.
#[derive(Clone, Debug, PartialEq)]
enum Ask {
    Install(Box<Offer>),
    Code(InstallCtx, Box<CodeQuestion>),
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
    /// The question out, which its answer goes on with.
    asking: Option<Ask>,
    /// The name of what is being installed, for "Installing NAME VERSION...".
    naming: String,
    /// trust.set answers still to come before the install is tried again.
    trusting_left: usize,
    /// An install job is out: the rows are faint and a second cannot start.
    active_install: bool,
    /// What a done install said: "NAME VERSION installed." and the job's notes.
    said: String,
    /// A failed install, or one a job already running refused: the engine's words.
    failed_install: String,
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
        self.asking = None;
        self.active_install = false;
        self.said.clear();
        self.failed_install.clear();
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

    /// A row's action: "Install" when nothing is installed, "Update" when an older version is, nothing when the
    /// version offered is installed.
    fn install_label(o: &Offer) -> Option<&'static str> {
        if o.state.is_empty() {
            Some("Install")
        } else if o.state.starts_with("Update from") {
            Some("Update")
        } else {
            None
        }
    }

    /// No install job out any more; `error` the engine's words when it failed.
    fn install_over(&mut self, error: &str) {
        self.active_install = false;
        self.progress.clear();
        self.failed_install = error.to_string();
    }

    /// `packages.install` by the Library's own install (catalog_flow.rs `install_params`), its job asked about every
    /// 100 ms until it ends.
    fn install(&mut self, ctx: InstallCtx, c: &mut Ctx) {
        self.active_install = true;
        self.failed_install.clear();
        self.said.clear();
        self.progress = format!("Installing {} {}...", self.naming, ctx.version);
        let params = install_params(&ctx);
        c.ask(Any::Available(Step::Install(ctx)), "packages.install", params);
    }

    /// The install ended, or not (catalog_flow.rs `install_ended`, as the Library reads it): asked again, the question for
    /// code this device does not trust, the engine's words, or what it installed and both lists read again.
    fn install_poll(&mut self, ctx: InstallCtx, job: u64, state: &Value, c: &mut Ctx) {
        let said = match install_ended(state, &ctx) {
            Ended::Running => {
                c.ask_after(Duration::from_millis(100), Any::Available(Step::InstallPoll(ctx, job)), "packages.job", json!({"job": job}));
                return;
            }
            Ended::Code(q) => {
                self.install_over("");
                c.notice = Some((q.title.clone(), q.message.clone(), q.buttons.clone()));
                self.asking = Some(Ask::Code(ctx, q));
                return;
            }
            Ended::Failed(error) => return self.install_over(&error),
            Ended::Done(said) => said,
        };
        self.install_over("");
        self.said = said;
        // Both lists are read again, as Refresh reads them, so what installed shows and nothing more is asked.
        self.queue.clear();
        self.listings.clear();
        self.failed.clear();
        self.repos_error.clear();
        self.installed_error.clear();
        self.installed.clear();
        self.reading = true;
        self.active = false;
        c.ask(Any::Available(Step::Repos), "packages.repositories", json!({}));
        c.ask(Any::Available(Step::Installed), "packages.installed", json!({}));
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
                if o.off() || self.active_install { faint(r) } else { r }
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
        if !self.said.is_empty() {
            lines.push(head("Installed"));
            lines.extend(said(&self.said));
        }
        if !self.failed_install.is_empty() {
            lines.push(head("Not done"));
            lines.extend(failed(&self.failed_install));
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
        if (self.reading || self.active || self.active_install) && !self.progress.is_empty() {
            self.progress.clone()
        } else {
            self.said.clone()
        }
    }

    fn tools(&self, at: Option<usize>) -> Vec<Tool> {
        let busy = self.reading || self.active || self.active_install;
        let list = self.list();
        let action = at.and_then(|a| list.get(a)).and_then(Available::install_label);
        let mut v = vec![Tool { label: "Refresh".into(), enabled: !busy, on: false }];
        if let Some(label) = action {
            v.push(Tool { label: label.into(), enabled: !busy, on: false });
        }
        v
    }

    fn tool(&mut self, i: usize, at: Option<usize>, c: &mut Ctx) {
        let busy = self.reading || self.active || self.active_install;
        if busy {
            return;
        }
        if i == 0 {
            self.reset();
        } else {
            // The row under the cursor is installed or updated: asked first, and only when the answer confirms.
            let Some(o) = at.and_then(|a| self.list().get(a).cloned()) else { return };
            let Some(which) = Available::install_label(&o) else { return };
            let mut message = String::new();
            if !o.offer.repository.is_empty() {
                message += &format!("It is offered by {}. ", o.offer.repository);
            }
            if o.offer.version.size > 0 {
                message += &format!("It is {}. ", size_text(o.offer.version.size));
            }
            c.notice = Some((format!("{which} {} {}?", o.name(), o.offer.version.version), message, vec![which.into(), "Cancel".into()]));
            self.asking = Some(Ask::Install(Box::new(o)));
            return;
        }
        c.ask(Any::Available(Step::Repos), "packages.repositories", json!({}));
        c.ask(Any::Available(Step::Installed), "packages.installed", json!({}));
    }

    fn noticed(&mut self, choice: Option<usize>, c: &mut Ctx) {
        // Nothing until the answer confirms; Cancel, or the question closed, sends nothing.
        match (self.asking.take(), choice) {
            (Some(Ask::Install(o)), Some(0)) => {
                self.naming = o.name().to_string();
                let p = &o.offer.package;
                let ctx = InstallCtx { source: o.offer.source.clone(), id: p.id.clone(), version: o.offer.version.version.clone(), publisher: p.publisher.clone(), publisher_key: p.publisher_key.clone(), replace: false, accept: false };
                self.install(ctx, c);
            }
            // The code question, answered as the Library's is (actions.rs `code_answer`): install anyway, or trust the keys
            // for plugins and try again.
            (Some(Ask::Code(ctx, q)), Some(i)) if Some(i) == q.anyway => self.install(InstallCtx { accept: true, ..ctx }, c),
            (Some(Ask::Code(ctx, q)), Some(i)) if Some(i) == q.trust => {
                self.active_install = true;
                c.ask(Any::Available(Step::TrustList(ctx, q.keys.clone())), "trust.list", json!({}));
            }
            _ => {}
        }
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
            (Step::Install(ctx), Ok(r)) => {
                let job = r["job"].as_u64().unwrap_or(0);
                c.ask_after(Duration::from_millis(100), Any::Available(Step::InstallPoll(ctx, job)), "packages.job", json!({"job": job}));
            }
            // A job already running is refused (`-32007`, reason `busy`) and says so; any other at-once failure is said.
            (Step::Install(_), Err(e)) => self.install_over(&e.message),
            (Step::InstallPoll(ctx, job_id), result) => {
                let state = match &result {
                    Ok(r) => r.clone(),
                    Err(e) => json!({"state": "failed", "error": e.message}),
                };
                self.install_poll(ctx, job_id, &state, c);
            }
            (Step::TrustList(ctx, keys), Ok(r)) => {
                let had = r["keys"].as_array().cloned().unwrap_or_default();
                self.trusting_left = keys.len();
                for params in trust_params(&ctx, &keys, &had) {
                    c.ask(Any::Available(Step::TrustSet(ctx.clone())), "trust.set", params);
                }
            }
            (Step::TrustList(..), Err(e)) | (Step::TrustSet(_), Err(e)) => self.install_over(&e.message),
            (Step::TrustSet(ctx), Ok(_)) => {
                // The last key trusted: the install is tried again, not asking for code.
                self.trusting_left = self.trusting_left.saturating_sub(1);
                if self.trusting_left == 0 && self.active_install {
                    self.install(InstallCtx { accept: false, ..ctx }, c);
                }
            }
        }
    }
}
