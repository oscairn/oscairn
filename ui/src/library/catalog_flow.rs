// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Catalog on the Library's screen (ui/LIBRARY-APP.md D4 to D7; Catalog.cpp `read`, `readNext`, `emptySaying`, `open`;
//! LibraryPane.cpp `showCatalog`, `installPackage`, `awaitPackageJob`): the enabled repositories read one after another (each
//! `packages.list` is a job in the engine, asked about every 100 ms until it ends), the packages in four tabs, a package's
//! details, and installing one: a job asked about the same way, with the questions for code this device does not trust
//! and for a different build already here.

use super::catalog::{self, code_question, offers, CodeQuestion, Installed as InstalledPackage, Listing, Offer, Package};
use super::install::Signer;
use super::*;
use std::collections::BTreeMap;

/// The Catalog's tabs: the kind each holds, and its name (Catalog.cpp `tabs`).
pub const TABS: [(&str, &str); 4] = [("performance", "Performances"), ("sound", "Sounds"), ("plugin", "Plugins"), ("asset", "Content")];

fn noun(kind: &str) -> &'static str {
    match kind {
        "performance" => "Performance",
        "sound" => "Sound",
        "plugin" => "plugin",
        _ => "content",
    }
}

/// A repository to read: its address and this device's name for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub address: String,
    pub name: String,
}

/// An install on its way: what it is, so a question's answer can go on with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ctx {
    pub source: String,
    pub id: String,
    pub version: String,
    pub publisher: String,
    pub publisher_key: String,
    pub replace: bool,
    /// Whether this attempt says yes to code from an unknown source.
    pub accept: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Req {
    /// packages.installed, for the Catalog's marks.
    Installed(u64),
    /// packages.repositories.
    Repos(u64),
    /// packages.list of one repository: the job.
    Listed(u64, Repo),
    /// packages.job of that listing.
    Poll(u64, Repo, u64),
    /// packages.install: the job.
    Install(Ctx),
    /// packages.job of the install.
    InstallPoll(Ctx, u64),
    /// trust.list, before keys are trusted: what to ask about and the keys.
    TrustList(Ctx, Vec<(String, String)>),
    /// trust.set of one key.
    TrustSet(Ctx),
}

/// The Catalog being read and shown (Catalog.h `Catalog`).
#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    pub kind: String,
    pub search: String,
    pub all: Vec<Offer>,
    pub status: String,
    pub reading: bool,
    pub none_on: bool,
    generation: u64,
    queue: Vec<Repo>,
    read: Vec<String>,
    failed: Vec<String>,
    listings: Vec<(String, Listing)>,
    installed: BTreeMap<String, InstalledPackage>,
    installed_known: bool,
    /// Which repositories are on is known.
    repos_known: bool,
    /// A repository's job is out.
    active: bool,
}

impl Catalog {
    pub fn new(kind: &str, generation: u64) -> Catalog {
        Catalog { kind: kind.into(), search: String::new(), all: vec![], status: "Reading...".into(), reading: true, none_on: false, generation, queue: vec![], read: vec![], failed: vec![], listings: vec![], installed: BTreeMap::new(), installed_known: false, repos_known: false, active: false }
    }

    /// The offers of the tab shown that the search finds: every word of it in the name, ID, summary, publisher or a tag.
    pub fn shown(&self) -> Vec<&Offer> {
        let words: Vec<String> = self.search.split_whitespace().map(str::to_lowercase).collect();
        self.all
            .iter()
            .filter(|o| o.holds(&self.kind))
            .filter(|o| {
                let haystack = format!("{} {} {} {} {}", o.package.name, o.package.id, o.package.summary, o.package.publisher, o.package.tags.join(" ")).to_lowercase();
                words.iter().all(|w| haystack.contains(w))
            })
            .collect()
    }

    /// Why nothing is listed, once read: no repository on, none read, a search, or a tab with nothing, pointing to the tabs
    /// that have something; empty while something is listed.
    pub fn empty_saying(&self) -> String {
        if self.reading || !self.shown().is_empty() {
            return String::new();
        }
        let where_ = "Repositories are turned on and added in the Settings app, on its Package repositories page.";
        if self.none_on {
            return format!("No package repository is on. {where_}");
        }
        if self.all.is_empty() {
            return format!("{}{where_}", if self.read.is_empty() { "No repository could be read: check the connection. " } else { "The repositories you have on offer no packages. " });
        }
        if !self.search.is_empty() {
            return "Nothing here matches the search.".into();
        }
        let elsewhere: Vec<&str> = TABS.iter().filter(|(k, _)| *k != self.kind && self.all.iter().any(|o| o.holds(k))).map(|(_, l)| *l).collect();
        let list = match elsewhere.split_last() {
            None => String::new(),
            Some((last, [])) => format!(" Look under {last}."),
            Some((last, rest)) => format!(" Look under {} or {last}.", rest.join(", ")),
        };
        format!("No {} packages in the repositories you have on.{list}", noun(&self.kind))
    }

    pub fn rows(&self) -> Vec<SettingData> {
        self.shown()
            .into_iter()
            .map(|o| SettingData { label: format!("{}  {}", o.title(), o.installed.as_deref().filter(|_| o.installed.is_some()).unwrap_or(&o.version.version)).into(), value: catalog::describe(o).into(), columns: ModelRc::new(VecModel::from(catalog::columns(o).into_iter().map(slint::SharedString::from).collect::<Vec<_>>())), kind: 2, off: !(o.installable || o.installed.is_some()), ..Default::default() })
            .collect()
    }

    pub fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head("Catalog")];
        lines.extend(wrap(&self.status, INFO_WIDTH).into_iter().map(line));
        lines.extend(wrap(&self.empty_saying(), INFO_WIDTH).into_iter().map(line));
        lines
    }
}

impl Screen {
    pub(super) fn catalog_open(&mut self, kind: &str) {
        self.catalog_generation += 1;
        let c = Catalog::new(kind, self.catalog_generation);
        let generation = c.generation;
        self.enter(Mode::Catalog(Box::new(c)));
        self.scanned = false;
        self.want_scan = true;
        self.push(Request::Pkg(Req::Installed(generation)), "packages.installed", json!({}));
        self.push(Request::Pkg(Req::Repos(generation)), "packages.repositories", json!({}));
    }

    /// The Catalog opened for the kind the Library shows ("plugin" for plugins).
    pub(super) fn catalog_for_kind(&mut self) {
        let kind = self.kind.key();
        self.catalog_open(kind);
    }

    fn catalog_mut(&mut self, generation: u64) -> Option<&mut Catalog> {
        match &mut self.mode {
            Mode::Catalog(c) if c.generation == generation => Some(c),
            _ => None,
        }
    }

    /// The plugins that come with the app, by ID: their versions (the scanner's answer).
    pub fn scanned(&mut self, bundled: BTreeMap<String, String>) {
        self.bundled = bundled;
        self.scanned = true;
        self.try_finish();
    }

    /// Whether the plugin scanner should run: asked once each time the Catalog opens.
    pub fn take_scan(&mut self) -> bool {
        std::mem::take(&mut self.want_scan)
    }

    pub(super) fn pkg_reply(&mut self, request: Req, result: Result<Value, Error>) -> bool {
        match request {
            Req::Installed(g) => {
                if let Ok(r) = &result {
                    let map: BTreeMap<String, InstalledPackage> = {
                        let mut m: BTreeMap<String, InstalledPackage> = BTreeMap::new();
                        for p in r["packages"].as_array().into_iter().flatten().map(InstalledPackage::from_json) {
                            // The newest version installed of each package.
                            match m.get(&p.id) {
                                Some(have) if !catalog::older(&have.version, &p.version) => {}
                                _ => {
                                    m.insert(p.id.clone(), p);
                                }
                            }
                        }
                        m
                    };
                    if let Some(c) = self.catalog_mut(g) {
                        c.installed = map;
                        c.installed_known = true;
                    }
                } else if let (Err(e), Some(c)) = (&result, self.catalog_mut(g)) {
                    c.status = e.message.clone();
                    c.reading = false;
                }
                self.try_finish();
            }
            Req::Repos(g) => match result {
                Ok(r) => {
                    let on: Vec<Repo> = r["repositories"].as_array().into_iter().flatten().filter(|x| x["enabled"] == true).map(|x| Repo { address: x["address"].as_str().unwrap_or_default().into(), name: x["name"].as_str().unwrap_or_default().into() }).collect();
                    if let Some(c) = self.catalog_mut(g) {
                        c.repos_known = true;
                        c.none_on = on.is_empty();
                        if on.is_empty() {
                            c.status = "No repository is on. Settings lists them.".into();
                            c.reading = false;
                        } else {
                            c.queue = on;
                            self.read_next(g);
                        }
                    }
                }
                Err(e) => {
                    if let Some(c) = self.catalog_mut(g) {
                        c.status = e.message;
                        c.reading = false;
                    }
                }
            },
            Req::Listed(g, repo) => match result {
                Ok(r) => {
                    let job = r["job"].as_u64().unwrap_or(0);
                    self.push_after(Duration::from_millis(100), Request::Pkg(Req::Poll(g, repo, job)), "packages.job", json!({"job": job}));
                }
                Err(e) => {
                    if let Some(c) = self.catalog_mut(g) {
                        c.active = false;
                        c.failed.push(format!("{} ({})", repo.name, e.message));
                    }
                    self.read_next(g);
                }
            },
            Req::Poll(g, repo, job) => {
                let state = match &result {
                    Ok(r) => r.clone(),
                    Err(e) => json!({"state": "failed", "error": e.message}),
                };
                if state["state"] == "running" {
                    self.push_after(Duration::from_millis(100), Request::Pkg(Req::Poll(g, repo, job)), "packages.job", json!({"job": job}));
                } else {
                    if let Some(c) = self.catalog_mut(g) {
                        c.active = false;
                        if state["state"] == "done" {
                            // This device's name for it, never the one its root claims.
                            let packages = state["packages"].as_array().into_iter().flatten().map(Package::from_json).collect();
                            c.listings.push((repo.address.clone(), Listing { name: repo.name.clone(), packages }));
                            c.read.push(repo.name.clone());
                        } else {
                            c.failed.push(format!("{} ({})", repo.name, state["error"].as_str().unwrap_or_default()));
                        }
                    }
                    self.read_next(g);
                }
            }
            Req::Install(ctx) => match result {
                Ok(r) => {
                    let job = r["job"].as_u64().unwrap_or(0);
                    self.push_after(Duration::from_millis(100), Request::Pkg(Req::InstallPoll(ctx, job)), "packages.job", json!({"job": job}));
                }
                Err(e) => self.say(e.message),
            },
            Req::InstallPoll(ctx, job) => return self.install_poll(ctx, job, result),
            Req::TrustList(ctx, keys) => self.trust_list(ctx, keys, result),
            Req::TrustSet(ctx) => self.trust_set(ctx, result),
        }
        false
    }

    /// The next repository, else the end of the reading.
    fn read_next(&mut self, g: u64) {
        let Some(c) = self.catalog_mut(g) else { return };
        if c.queue.is_empty() {
            self.try_finish();
            return;
        }
        let repo = c.queue.remove(0);
        c.active = true;
        c.status = format!("Reading {}...", repo.name);
        let address = repo.address.clone();
        self.push(Request::Pkg(Req::Listed(g, repo)), "packages.list", json!({"source": address}));
    }

    /// Once every repository is read and the app's plugins are known: the offers.
    fn try_finish(&mut self) {
        let (scanned, bundled) = (self.scanned, self.bundled.clone());
        let Mode::Catalog(c) = &mut self.mode else { return };
        if !c.reading || c.active || !c.queue.is_empty() || !c.installed_known || !c.repos_known || !scanned || c.none_on {
            return;
        }
        c.reading = false;
        c.all = offers(&c.listings, &c.installed, &bundled);
        c.status = if c.read.is_empty() { String::new() } else { format!("From {}.", c.read.join(", ")) };
        if !c.failed.is_empty() {
            c.status += &format!("{}Not read: {}.", if c.status.is_empty() { "" } else { " " }, c.failed.join("; "));
        }
        // Opened on a tab with nothing to show, it moves to the first that has something.
        let has = |c: &Catalog, k: &str| c.all.iter().any(|o| o.holds(k));
        if c.search.is_empty() && !has(c, &c.kind) {
            if let Some((k, _)) = TABS.iter().find(|(k, _)| has(c, k)) {
                c.kind = (*k).to_string();
            }
        }
    }

    /// The Catalog's tab `i`.
    pub(super) fn catalog_tab(&mut self, i: usize) {
        if let (Mode::Catalog(c), Some((k, _))) = (&mut self.mode, TABS.get(i)) {
            c.kind = (*k).to_string();
        }
        self.b.reset_cursor();
    }

    /// What an opened package says beside its details: its name, what it holds and its tags.
    pub(super) fn package_info(&self, o: &Offer) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head("Package"), line(format!("{} {}", o.title(), o.version.version))];
        lines.extend(wrap(&o.package.summary, INFO_WIDTH).into_iter().map(line));
        if !o.package.tags.is_empty() {
            lines.push(line(o.package.tags.join(", ")));
        }
        lines
    }

    /// The offer shown at row `at` of the list.
    fn offer_at(&self, at: usize) -> Option<Offer> {
        match &self.mode {
            Mode::Catalog(c) => c.shown().get(at).map(|o| (*o).clone()),
            _ => None,
        }
    }

    pub(super) fn catalog_press(&mut self, at: usize) {
        if let Some(offer) = self.offer_at(at) {
            self.enter(Mode::Package(Box::new(offer)));
        }
    }

    /// The details of an opened package: what can be done first (Install, Update to V, or the repository's build: the dial
    /// reaches it at once), then each fact a row, the download addresses after, then Back.
    pub(super) fn package_rows(o: &Offer) -> Vec<(SettingData, Option<PackageRow>)> {
        let fact = |label: &str, value: String| SettingData { label: label.into(), value: value.into(), kind: 4, ..Default::default() };
        let mut rows: Vec<(SettingData, Option<PackageRow>)> = Vec::new();
        if o.installed.is_none() || o.update || o.other_build {
            let label = if o.update { format!("Update to {}", o.version.version) } else if o.other_build { "Install the repository's build".to_string() } else { "Install".to_string() };
            rows.push((SettingData { label: label.into(), value: if o.installable { String::new() } else { "not here".into() }.into(), kind: 2, off: !o.installable, ..Default::default() }, Some(PackageRow::Install)));
        }
        rows.extend(catalog::facts(o).into_iter().filter(|(n, _)| n != "Download for another device").map(|(n, v)| (fact(&n, v), None)));
        let downloads = catalog::downloads(o);
        if !downloads.is_empty() {
            rows.push((fact("Download for another device", String::new()), None));
            rows.extend(downloads.into_iter().map(|(platform, address)| (SettingData { label: format!("  {platform}").into(), value: address.into(), kind: 4, ..Default::default() }, None)));
        }
        rows.push((SettingData { label: "Back".into(), value: String::new().into(), kind: 2, ..Default::default() }, Some(PackageRow::Back)));
        rows
    }

    pub(super) fn package_press(&mut self, at: usize) {
        let Mode::Package(o) = &self.mode else { return };
        let o = (**o).clone();
        match Screen::package_rows(&o).get(at).and_then(|r| r.1) {
            Some(PackageRow::Back) => self.leave(),
            Some(PackageRow::Install) if o.installable => {
                // The Catalog closes and the install goes on in the Library, as the JUCE dialog's does.
                self.leave();
                self.leave();
                if o.other_build && !o.update {
                    // Over a different build only when the user says so, having seen both.
                    self.notice = Some(("Install the repository's build?".into(), catalog::replace_question(&o), vec!["Install the repository's build".into(), "Cancel".into()]));
                    self.notice_for = Some(NoticeFor::Replace(Box::new(o)));
                } else {
                    let ctx = Ctx { source: o.source.clone(), id: o.package.id.clone(), version: o.version.version.clone(), publisher: o.package.publisher.clone(), publisher_key: o.package.publisher_key.clone(), replace: false, accept: false };
                    self.install_package(ctx);
                }
            }
            _ => {}
        }
    }

    /// The question about replacing a different build, answered.
    pub(super) fn replace_answer(&mut self, o: Offer, choice: Option<usize>) {
        if choice == Some(0) {
            let ctx = Ctx { source: o.source.clone(), id: o.package.id.clone(), version: o.version.version.clone(), publisher: o.package.publisher.clone(), publisher_key: o.package.publisher_key.clone(), replace: true, accept: false };
            self.install_package(ctx);
        }
    }

    /// `packages.install`: a job in the engine, asked about every 100 ms until it ends.
    pub(super) fn install_package(&mut self, ctx: Ctx) {
        self.say(format!("Installing {} {}...", ctx.id, ctx.version));
        let mut params = json!({"source": ctx.source, "id": ctx.id, "acceptUnsignedCode": ctx.accept, "replace": ctx.replace});
        if !ctx.version.is_empty() {
            params["version"] = json!(ctx.version);
        }
        self.push(Request::Pkg(Req::Install(ctx)), "packages.install", params);
    }

    fn install_poll(&mut self, ctx: Ctx, job: u64, result: Result<Value, Error>) -> bool {
        let state = match &result {
            Ok(r) => r.clone(),
            Err(e) => {
                self.say(e.message.clone());
                return false;
            }
        };
        if state["state"] == "running" {
            self.push_after(Duration::from_millis(100), Request::Pkg(Req::InstallPoll(ctx, job)), "packages.job", json!({"job": job}));
            return false;
        }
        let error = state["error"].as_str().unwrap_or_default().to_string();
        if state["state"] == "failed" && error.contains("unknown source") {
            // Code this device does not trust asks first, as installing an archive does.
            let code: Vec<Signer> = state["installed"]["code"].as_array().into_iter().flatten().map(Signer::from_json).collect();
            let q = code_question(&code, state["name"].as_str().unwrap_or_default(), &ctx.publisher, &ctx.publisher_key);
            self.say("");
            self.notice = Some((q.title.clone(), q.message.clone(), q.buttons.clone()));
            self.notice_for = Some(NoticeFor::Code(ctx, Box::new(q)));
            return false;
        }
        if state["state"] == "failed" {
            self.say("");
            self.alert("Not installed", &error);
            return false;
        }
        let mut said = format!("{} {} installed.", state["package"].as_str().unwrap_or_default(), state["version"].as_str().unwrap_or_default());
        for n in state["installed"]["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            said += &format!(" {n}.");
        }
        self.say(said);
        self.refresh_after_job(); // its items, listed again
        false
    }

    /// The answer to the question for code this device does not trust.
    pub(super) fn code_answer(&mut self, ctx: Ctx, q: CodeQuestion, choice: Option<usize>) {
        let Some(chosen) = choice else { return };
        if Some(chosen) == q.anyway {
            return self.install_package(Ctx { accept: true, ..ctx });
        }
        if Some(chosen) != q.trust {
            return;
        }
        // Trust the key for plugins, beside whatever else it is trusted for, then try again.
        self.push(Request::Pkg(Req::TrustList(ctx, q.keys.clone())), "trust.list", json!({}));
    }

    fn trust_list(&mut self, ctx: Ctx, keys: Vec<(String, String)>, result: Result<Value, Error>) {
        let had: Vec<Value> = result.as_ref().map(|r| r["keys"].as_array().cloned().unwrap_or_default()).unwrap_or_default();
        if let Err(e) = result {
            return self.say(e.message);
        }
        self.trusting_left = keys.len();
        for (public, id) in keys {
            // For plugins, beside whatever else the key is trusted for; by the publisher's name only where the repository vouches for it.
            let (mut name, mut scope) = (if id == ctx.publisher_key && !ctx.publisher.is_empty() { ctx.publisher.clone() } else { format!("Key {id}") }, vec!["plugins".to_string()]);
            if let Some(h) = had.iter().find(|h| h["key"] == public) {
                name = h["name"].as_str().unwrap_or_default().to_string();
                scope = h["scope"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).chain(std::iter::once("plugins".to_string())).collect();
            }
            self.push(Request::Pkg(Req::TrustSet(ctx.clone())), "trust.set", json!({"key": public, "name": name, "scope": scope, "revoked": false}));
        }
    }

    fn trust_set(&mut self, ctx: Ctx, result: Result<Value, Error>) {
        if let Err(e) = result {
            return self.say(e.message);
        }
        // The last key trusted: the install is tried again, not asking for code.
        self.trusting_left = self.trusting_left.saturating_sub(1);
        if self.trusting_left == 0 {
            self.install_package(Ctx { accept: false, ..ctx });
        }
    }
}

/// What a row of an opened package does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PackageRow {
    Install,
    Back,
}
