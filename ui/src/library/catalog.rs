// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Catalog (ui/LIBRARY-APP.md D4 to D7; hosts/standalone/Catalog.cpp, RFC 0031): the packages the enabled repositories
//! offer, as the Library's Install opens them. This is the model, a port of Catalog.cpp's `offers`, `describe`,
//! `facts`, `replaceQuestion`, `codeQuestion` and `downloads`; the screen that shows it and the requests that fill it
//! are catalog_flow.rs.

use super::install::{fingerprint, Signer};
use super::*;
use std::collections::BTreeMap;

/// A package version's order, as `assets::older` (core/plugin-host/src/assets.cpp) has it: split at '.', '-' and '+',
/// numbers by value and before words.
pub fn older(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut part = String::new();
        for c in v.chars().chain(std::iter::once('.')) {
            if matches!(c, '.' | '-' | '+') {
                out.push(std::mem::take(&mut part));
            } else {
                part.push(c);
            }
        }
        out
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).map_or("0", String::as_str), y.get(i).map_or("0", String::as_str));
        let (pn, qn) = (!p.is_empty() && p.chars().all(|c| c.is_ascii_digit()), !q.is_empty() && q.chars().all(|c| c.is_ascii_digit()));
        if pn && qn {
            let (pv, qv) = (p.trim_start_matches('0'), q.trim_start_matches('0'));
            if pv.len() != qv.len() {
                return pv.len() < qv.len();
            }
            if pv != qv {
                return pv < qv;
            }
        } else if pn != qn {
            return pn; // a number before a word
        } else if p != q {
            return p < q;
        }
    }
    false
}

/// A revocation list's withdrawal of a version (`withdrawn`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Withdrawn {
    pub repository: String,
    pub reason: String,
    pub date: String,
}

impl Withdrawn {
    fn from_json(v: &Value) -> Option<Withdrawn> {
        v.is_object().then(|| Withdrawn { repository: v["repository"].as_str().unwrap_or_default().into(), reason: v["reason"].as_str().unwrap_or_default().into(), date: v["date"].as_str().unwrap_or_default().into() })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Version {
    pub version: String,
    pub channel: String,
    pub yanked: String,
    pub incompatible: String,
    pub sha256: String,
    pub size: u64,
    pub installed: u64,
    /// How many items of each type and plugins of each format it holds.
    pub types: Vec<(String, u64)>,
    pub dependencies: Vec<(String, String)>,
    pub withdrawn: Option<Withdrawn>,
    /// Ready-made archives: platform ("" for all), address.
    pub exports: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub license: String,
    pub publisher: String,
    pub publisher_key: String,
    pub tags: Vec<String>,
    pub versions: Vec<Version>,
}

impl Package {
    /// A package of a `packages.job` listing.
    pub fn from_json(x: &Value) -> Package {
        let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
        Package {
            id: text(x, "id"),
            name: text(x, "name"),
            summary: text(x, "summary"),
            license: text(x, "license"),
            publisher: text(x, "publisher"),
            publisher_key: text(x, "publisherKey"),
            tags: x["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect(),
            versions: x["versions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| Version {
                    version: text(v, "version"),
                    channel: text(v, "channel"),
                    yanked: text(v, "yanked"),
                    incompatible: text(v, "incompatible"),
                    sha256: text(v, "sha256"),
                    size: v["size"].as_u64().unwrap_or(0),
                    installed: v["installed"].as_u64().unwrap_or(0),
                    types: v["types"].as_object().into_iter().flatten().map(|(k, n)| (k.clone(), n.as_u64().unwrap_or(0))).collect(),
                    dependencies: v["dependencies"].as_array().into_iter().flatten().map(|d| (text(d, "id"), text(d, "version"))).collect(),
                    withdrawn: Withdrawn::from_json(&v["withdrawn"]),
                    exports: v["exports"].as_array().into_iter().flatten().map(|e| (text(e, "platform"), text(e, "address"))).collect(),
                })
                .collect(),
        }
    }
}

/// A package installed here (`packages.installed`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Installed {
    pub id: String,
    pub version: String,
    pub kind: String,
    pub repository: String,
    pub sha256: String,
    pub signers: Vec<String>,
    pub other_build: bool,
    pub withdrawn: Option<Withdrawn>,
    pub dependency: bool,
}

impl Installed {
    pub fn from_json(x: &Value) -> Installed {
        let text = |k: &str| x[k].as_str().unwrap_or_default().to_string();
        Installed {
            id: text("id"),
            version: text("version"),
            kind: text("kind"),
            repository: text("repository"),
            sha256: text("sha256"),
            signers: x["signers"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect(),
            other_build: x["otherBuild"] == true,
            withdrawn: Withdrawn::from_json(&x["withdrawn"]),
            dependency: x["dependency"] == true,
        }
    }
}

/// One repository read: this device's name for it, and its packages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub name: String,
    pub packages: Vec<Package>,
}

/// One package a repository offers, as the Catalog lists it (Catalog.h `Offer`): where from, and the version this device
/// would install (the newest stable one it can, not yanked), else the newest, with why; and the version installed
/// here, offered as an update when `version` is newer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Offer {
    /// The repository's address and this device's name for it.
    pub source: String,
    pub repository: String,
    pub package: Package,
    pub version: Version,
    pub installable: bool,
    pub installed: Option<String>,
    pub update: bool,
    /// Installed at `version`, but not this repository's build.
    pub other_build: bool,
    /// Not installed, but its plugins come with the app.
    pub built_in: bool,
    pub record: Installed,
    /// An asset library installed only as a dependency: the installed package whose listed versions need it.
    pub with: String,
}

impl Offer {
    /// Whether it holds `kind`: "performance", "sound", "plugin" or "asset".
    pub fn holds(&self, kind: &str) -> bool {
        self.version.types.iter().any(|(t, n)| *n > 0 && if kind == "plugin" { t == "clap" || t == "lv2" } else { *t == format!("oscairn.{kind}") })
    }

    /// Install, Update, or the repository's build.
    #[cfg(test)]
    pub fn offered(&self) -> bool {
        self.installable && (self.installed.is_none() || self.update || self.other_build)
    }

    pub fn title(&self) -> &str {
        if self.package.name.is_empty() {
            &self.package.id
        } else {
            &self.package.name
        }
    }
}

/// The offers of these listings, each once per repository (`installed`: the newest version installed of each package, by
/// ID). A plugin package not installed whose plugins all come with the app (`bundled`: their versions, by plugin ID) is
/// built in, at the oldest of their versions: shown as Built in, or offered as Update to a newer version the repository
/// has, which installs beside the app's (RFC 0031).
pub fn offers(listings: &[(String, Listing)], installed: &BTreeMap<String, Installed>, bundled: &BTreeMap<String, String>) -> Vec<Offer> {
    // The index lists no plugin IDs, so a package of several is named here (Catalog.cpp:45-46).
    let several: BTreeMap<&str, Vec<&str>> = BTreeMap::from([("org.oscairn.splitters", vec!["org.oscairn.split", "org.oscairn.join"])]);
    let mut out = Vec::new();
    for (source, listing) in listings {
        for p in &listing.packages {
            if p.versions.is_empty() {
                continue;
            }
            // The version the engine would install; else the newest, to say why not.
            let (mut pick, mut newest): (Option<&Version>, Option<&Version>) = (None, None);
            for v in &p.versions {
                if newest.is_none_or(|n| older(&n.version, &v.version)) {
                    newest = Some(v);
                }
                if v.incompatible.is_empty() && v.yanked.is_empty() && v.withdrawn.is_none() && (v.channel.is_empty() || v.channel == "stable") && pick.is_none_or(|b| older(&b.version, &v.version)) {
                    pick = Some(v);
                }
            }
            let mut o = Offer { source: source.clone(), repository: listing.name.clone(), package: p.clone(), version: pick.or(newest).cloned().unwrap_or_default(), installable: pick.is_some(), ..Default::default() };
            if let Some(r) = installed.get(&p.id) {
                o.installed = Some(r.version.clone());
                o.record = r.clone();
                // Installed only because a package needed it: the installed one whose listed versions name it.
                if r.dependency {
                    'find: for (_, l) in listings {
                        for q in &l.packages {
                            if installed.contains_key(&q.id) && q.versions.iter().any(|v| v.dependencies.iter().any(|d| d.0 == p.id)) {
                                o.with = if q.name.is_empty() { q.id.clone() } else { q.name.clone() };
                                break 'find;
                            }
                        }
                    }
                }
                o.update = pick.is_some_and(|k| !r.version.is_empty() && older(&r.version, &k.version));
                // "Installed" only as this repository builds it (decided 2026-11-11).
                for v in &p.versions {
                    if !o.update && !r.version.is_empty() && v.version == r.version && (r.other_build || (!r.sha256.is_empty() && !v.sha256.is_empty() && r.sha256 != v.sha256)) {
                        o.other_build = true;
                        o.version = v.clone();
                        o.installable = v.incompatible.is_empty() && v.withdrawn.is_none();
                    }
                }
            } else if o.holds("plugin") {
                let ids: Vec<&str> = several.get(p.id.as_str()).cloned().unwrap_or_else(|| vec![p.id.as_str()]);
                let mut oldest: Option<&String> = None;
                let mut all = true;
                for id in ids {
                    match bundled.get(id) {
                        None => all = false,
                        Some(b) => {
                            if oldest.is_none_or(|o| older(b, o)) {
                                oldest = Some(b);
                            }
                        }
                    }
                }
                if all {
                    o.built_in = true;
                    o.installed = oldest.cloned();
                    o.update = pick.is_some_and(|k| oldest.is_some_and(|b| older(b, &k.version)));
                }
            }
            out.push(o);
        }
    }
    out.sort_by_key(|o| o.package.name.to_lowercase());
    out
}

const SEPARATOR: &str = "  ·  ";

/// Why a version cannot be installed here: this device, or a revocation list withdrawing it.
fn why_not(v: &Version) -> String {
    if !v.incompatible.is_empty() {
        return v.incompatible.clone();
    }
    if let Some(w) = v.withdrawn.as_ref().filter(|w| !w.reason.is_empty()) {
        return format!("withdrawn by {}: {}", w.repository, w.reason);
    }
    "no version it can install".into()
}

fn with_what(o: &Offer) -> String {
    format!("with {}", if o.with.is_empty() { "a package that needs it" } else { &o.with })
}

/// A row's second line (Catalog.cpp `describe`): its state, then its publisher and summary; for what is installed and
/// withdrawn since, who withdrew it and why, and Update to when there is a newer version.
pub fn describe(o: &Offer) -> String {
    let about = format!("{}{}", o.package.publisher, if o.package.summary.is_empty() { String::new() } else { format!("{SEPARATOR}{}", o.package.summary) });
    if let (Some(_), Some(w)) = (&o.installed, o.record.withdrawn.as_ref().filter(|w| !w.reason.is_empty())) {
        return format!("Withdrawn by {}: {}{}", w.repository, w.reason, if o.update { format!("{SEPARATOR}Update to {}", o.version.version) } else { String::new() });
    }
    if o.update {
        format!("Update to {}{SEPARATOR}{about}", o.version.version)
    } else if o.built_in {
        format!("Built in{SEPARATOR}{about}")
    } else if o.other_build {
        format!("Installed (a different build){SEPARATOR}{about}")
    } else if o.installed.is_some() {
        format!("{}{SEPARATOR}{about}", if o.record.dependency { format!("Installed {}", with_what(o)) } else { "Installed".into() })
    } else if o.installable {
        about
    } else {
        format!("Cannot be installed here: {}", why_not(&o.version))
    }
}

/// "NAME VERSION was withdrawn by REPOSITORY: REASON" for what is installed and withdrawn since, else empty.
pub fn withdrawn_saying(o: &Offer) -> String {
    match o.record.withdrawn.as_ref().filter(|w| !w.reason.is_empty()) {
        Some(w) => format!("{} {} was withdrawn by {}: {}", o.title(), o.record.version, w.repository, w.reason),
        None => String::new(),
    }
}

/// "1.2 MB" for a size in bytes (`juce::File::descriptionOfSizeInBytes`).
fn bytes(n: u64) -> String {
    super::files::size_text(n)
}

/// The details of an opened package, each named (Catalog.cpp `facts`).
pub fn facts(o: &Offer) -> Vec<(String, String)> {
    let v = &o.version;
    let needs: Vec<String> = v.dependencies.iter().map(|(id, least)| format!("{id} {least} or later")).collect();
    let mut out: Vec<(String, String)> = vec![
        ("Summary".into(), o.package.summary.clone()),
        ("Version".into(), format!("{}{}{}", v.version, if v.channel.is_empty() || v.channel == "stable" { String::new() } else { format!(" ({})", v.channel) }, if v.yanked.is_empty() { String::new() } else { format!(", yanked: {}", v.yanked) })),
        ("License".into(), if o.package.license.is_empty() { "Not stated".into() } else { o.package.license.clone() }),
        ("Publisher".into(), format!("{}, key {}", o.package.publisher, fingerprint(&o.package.publisher_key))),
        ("Repository".into(), o.repository.clone()),
        ("Size".into(), format!("{} to download, {} installed", bytes(v.size), bytes(v.installed))),
        ("Needs".into(), if needs.is_empty() { "Nothing else".into() } else { needs.join(", ") }),
    ];
    if let Some(installed) = &o.installed {
        let what = if installed.is_empty() {
            "Its version not known".to_string()
        } else {
            format!("{installed}{}", if o.built_in { ", built in: it comes with the app".to_string() } else if o.other_build { ", a different build: it does not match this repository's".into() } else if o.record.dependency { format!(", installed {}", with_what(o)) } else { String::new() })
        };
        out.insert(2, ("Installed".into(), what));
    }
    if !o.installable {
        out.push(("On this device".into(), why_not(v)));
    }
    if !downloads(o).is_empty() {
        out.push(("Download for another device".into(), String::new()));
    }
    // Said, and what the user can do about it; never removed or disabled for them.
    let said = withdrawn_saying(o);
    if !said.is_empty() {
        let list = match o.record.kind.as_str() {
            "plugin" => "plugins",
            "asset" => "asset libraries",
            _ => "libraries",
        };
        out.insert(0, ("Withdrawn".into(), format!("{said}{}. To remove it, use the Library's list of {list}.", if o.update { format!(". Update to {} replaces it", v.version) } else { String::new() })));
    }
    out
}

/// Each ready-made archive of the offered version, by its platform ("All platforms" for a package with no builds) and its
/// address; none for a withdrawn version.
pub fn downloads(o: &Offer) -> Vec<(String, String)> {
    if o.version.withdrawn.is_some() {
        return vec![];
    }
    o.version.exports.iter().map(|(p, a)| (if p.is_empty() { "All platforms".to_string() } else { p.clone() }, a.clone())).collect()
}

/// The question before installing the repository's build over a different one: both builds, each by its archive's
/// SHA-256 and signer, where known.
pub fn replace_question(o: &Offer) -> String {
    let archive = |sha: &str| format!("archive {}", &sha[..sha.len().min(16)]);
    let signers: Vec<String> = o.record.signers.iter().map(|k| fingerprint(k)).collect();
    let name = format!("{} {}", o.title(), o.version.version);
    let installed = if o.record.sha256.is_empty() { "a build installed some other way, its archive not known".to_string() } else { format!("{}{}", archive(&o.record.sha256), if signers.is_empty() { ", unsigned".to_string() } else { format!(", signed by key {}", signers.join(", ")) }) };
    format!(
        "{name} is installed here, but it is not the build {} has.\n\nInstalled: {installed}\n{}'s: {}, signed by {}, key {}\n\nInstalling the repository's build replaces the installed one.",
        o.repository,
        o.repository,
        archive(&o.version.sha256),
        o.package.publisher,
        fingerprint(&o.package.publisher_key)
    )
}

/// The question when an install is refused for plugin code this device does not trust (RFC 0006, RFC 0031), told apart by
/// each build's signer in the refusal: every one signed by `publisher_key` (vouched for by the repository): Trust and
/// install, or Cancel; one signed by a key no one vouches for: Trust this key, Install anyway (this once) or Cancel; one
/// unsigned: Install anyway or Cancel. `trust` and `anyway` are the numbers of the buttons that do them; `keys` the
/// public keys trusting trusts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeQuestion {
    pub title: String,
    pub message: String,
    pub buttons: Vec<String>,
    pub trust: Option<usize>,
    pub anyway: Option<usize>,
    pub keys: Vec<(String, String)>,
}

pub fn code_question(code: &[Signer], repository: &str, publisher: &str, publisher_key: &str) -> CodeQuestion {
    let (mut all, mut unsigned, mut strangers): (Vec<String>, Vec<String>, Vec<String>) = Default::default();
    let mut keys: Vec<(String, String)> = Vec::new(); // public key, its ID
    let add = |list: &mut Vec<String>, s: String| {
        if !list.contains(&s) {
            list.push(s);
        }
    };
    for c in code {
        if c.status != "unknown" && c.status != "unsigned" {
            continue; // trusted, or revoked (never installed)
        }
        add(&mut all, format!("{} {}", c.name, c.version));
        if c.status == "unsigned" {
            add(&mut unsigned, format!("{} {}", c.name, c.version));
        } else if publisher_key.is_empty() || c.key != publisher_key {
            add(&mut strangers, format!("key {}", fingerprint(&c.key)));
        }
        if c.status == "unknown" && !c.public_key.is_empty() && !keys.iter().any(|k| k.0 == c.public_key) {
            keys.push((c.public_key.clone(), c.key.clone()));
        }
    }
    let who = if publisher.is_empty() { "this publisher".to_string() } else { publisher.to_string() };
    let repository = if repository.is_empty() { "this repository".to_string() } else { repository.to_string() };
    let code_text = format!("The plugin code in this package ({})", all.join(", "));
    let risk = "\n\nA plugin runs as a program on this device, with your access to its files and network. Install it only if you know where it came from.";
    let mut q = CodeQuestion::default();
    if !keys.is_empty() && unsigned.is_empty() && strangers.is_empty() {
        q.title = format!("Trust {who} to install plugins?");
        q.message = format!("{code_text} is signed by {who}. The repository {repository} lists this key as {who}'s.\n\nKey fingerprint: {}\n\nOnce trusted, plugins signed by this key install without asking.", fingerprint(publisher_key));
        q.buttons = vec!["Trust and install".into(), "Cancel".into()];
        q.trust = Some(0);
        q.keys = keys;
    } else if unsigned.is_empty() {
        q.title = "Install code from an unknown key?".into();
        q.message = format!("{code_text} is signed by {}, which {repository} does not list as {who}'s and this device does not trust. Anyone could have made it.{risk}", strangers.join(", "));
        q.buttons = vec![if keys.len() > 1 { "Trust these keys" } else { "Trust this key" }.into(), "Install anyway".into(), "Cancel".into()];
        q.trust = Some(0);
        q.anyway = Some(1);
        q.keys = keys;
    } else {
        q.title = "Install unsigned code?".into();
        q.message = format!("{} in this package is not signed: nothing says who made it, or that it is unchanged.{}{risk}", unsigned.join(", "), if strangers.is_empty() { String::new() } else { format!(" The rest is signed by {}, which no one here vouches for.", strangers.join(", ")) });
        q.buttons = vec!["Install anyway".into(), "Cancel".into()];
        q.anyway = Some(0);
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn version(v: &str) -> Version {
        Version { version: v.into(), types: vec![("oscairn.performance".into(), 2)], ..Default::default() }
    }

    fn package(id: &str, name: &str, versions: Vec<Version>) -> Package {
        Package { id: id.into(), name: name.into(), publisher: "Ada".into(), publisher_key: "0123456789abcdef".into(), summary: "Pads".into(), versions, ..Default::default() }
    }

    fn listing(packages: Vec<Package>) -> Vec<(String, Listing)> {
        vec![("https://repo.example".into(), Listing { name: "Example".into(), packages })]
    }

    #[test]
    fn versions_are_ordered_as_the_engine_orders_them() {
        assert!(older("1.2", "1.10"));
        assert!(older("1.0", "1.0.1"));
        assert!(!older("1.0", "1.0.0"), "equal");
        assert!(!older("1.0-beta", "1.0") && older("1.0", "1.0-beta"), "a number before a word");
        assert!(older("0.9", "1.0"));
    }

    #[test]
    fn the_version_offered_is_the_newest_stable_one_this_device_can_install() {
        let mut beta = version("3.0");
        beta.channel = "beta".into();
        let mut yanked = version("2.5");
        yanked.yanked = "broken".into();
        let mut needs = version("2.2");
        needs.incompatible = "it needs a system update: Oscairn 9 or later".into();
        let l = listing(vec![package("a", "A", vec![version("1.0"), version("2.0"), beta, yanked, needs])]);
        let o = &offers(&l, &BTreeMap::new(), &BTreeMap::new())[0];
        assert_eq!((o.version.version.as_str(), o.installable), ("2.0", true));
        assert_eq!(describe(o), "Ada  ·  Pads");
        // Nothing installable: the newest, with why.
        let l = listing(vec![package("a", "A", vec![{
            let mut v = version("2.2");
            v.incompatible = "it has no build for this device (linux-arm64)".into();
            v
        }])]);
        let o = &offers(&l, &BTreeMap::new(), &BTreeMap::new())[0];
        assert_eq!((o.version.version.as_str(), o.installable), ("2.2", false));
        assert_eq!(describe(o), "Cannot be installed here: it has no build for this device (linux-arm64)");
    }

    #[test]
    fn an_installed_package_says_so_and_offers_an_update_or_the_repositorys_build() {
        let l = listing(vec![package("a", "A", vec![version("1.0"), version("2.0")]), package("b", "B", vec![{
            let mut v = version("1.0");
            v.sha256 = "aaaa".into();
            v
        }])]);
        let mut installed = BTreeMap::new();
        installed.insert("a".to_string(), Installed { id: "a".into(), version: "1.0".into(), ..Default::default() });
        installed.insert("b".to_string(), Installed { id: "b".into(), version: "1.0".into(), sha256: "bbbb".into(), ..Default::default() });
        let all = offers(&l, &installed, &BTreeMap::new());
        let (a, b) = (&all[0], &all[1]);
        assert!(a.update && a.offered());
        assert_eq!(describe(a), "Update to 2.0  ·  Ada  ·  Pads");
        assert!(b.other_build && b.offered());
        assert_eq!(describe(b), "Installed (a different build)  ·  Ada  ·  Pads");
        // Installed as this repository builds it: nothing to install.
        installed.insert("b".to_string(), Installed { id: "b".into(), version: "1.0".into(), sha256: "aaaa".into(), ..Default::default() });
        let all = offers(&l, &installed, &BTreeMap::new());
        assert!(!all[1].other_build && !all[1].offered());
        assert_eq!(describe(&all[1]), "Installed  ·  Ada  ·  Pads");
    }

    #[test]
    fn a_plugin_that_comes_with_the_app_is_built_in_and_an_update_installs_beside_it() {
        let mut v = version("2.0");
        v.types = vec![("clap".into(), 1)];
        let l = listing(vec![package("org.oscairn.delay", "Delay", vec![v])]);
        let bundled = BTreeMap::from([("org.oscairn.delay".to_string(), "1.0".to_string())]);
        let o = &offers(&l, &BTreeMap::new(), &bundled)[0];
        assert!(o.built_in && o.update && o.installable);
        assert_eq!(describe(o), "Update to 2.0  ·  Ada  ·  Pads");
        let bundled = BTreeMap::from([("org.oscairn.delay".to_string(), "2.0".to_string())]);
        let o = &offers(&l, &BTreeMap::new(), &bundled)[0];
        assert!(o.built_in && !o.update);
        assert_eq!(describe(o), "Built in  ·  Ada  ·  Pads");
        assert_eq!(facts(o).iter().find(|f| f.0 == "Installed").unwrap().1, "2.0, built in: it comes with the app");
        // A package of several plugins is built in only when all of them are.
        let mut v = version("1.0");
        v.types = vec![("clap".into(), 2)];
        let l = listing(vec![package("org.oscairn.splitters", "Splitters", vec![v])]);
        let one = BTreeMap::from([("org.oscairn.split".to_string(), "1.0".to_string())]);
        assert!(!offers(&l, &BTreeMap::new(), &one)[0].built_in);
        let both = BTreeMap::from([("org.oscairn.split".to_string(), "1.0".to_string()), ("org.oscairn.join".to_string(), "0.9".to_string())]);
        let o = &offers(&l, &BTreeMap::new(), &both)[0];
        assert!(o.built_in && o.installed.as_deref() == Some("0.9") && o.update, "the oldest of their versions");
    }

    #[test]
    fn what_is_installed_and_withdrawn_says_who_and_why() {
        let l = listing(vec![package("a", "A", vec![version("1.0"), version("2.0")])]);
        let installed = BTreeMap::from([("a".to_string(), Installed { id: "a".into(), version: "1.0".into(), kind: "plugin".into(), withdrawn: Some(Withdrawn { repository: "Example".into(), reason: "a key was stolen".into(), date: "2026-11-01".into() }), ..Default::default() })]);
        let o = &offers(&l, &installed, &BTreeMap::new())[0];
        assert_eq!(describe(o), "Withdrawn by Example: a key was stolen  ·  Update to 2.0");
        assert_eq!(withdrawn_saying(o), "A 1.0 was withdrawn by Example: a key was stolen");
        let f = facts(o);
        assert_eq!(f[0].0, "Withdrawn");
        assert_eq!(f[0].1, "A 1.0 was withdrawn by Example: a key was stolen. Update to 2.0 replaces it. To remove it, use the Library's list of plugins.");
    }

    #[test]
    fn an_asset_library_installed_for_another_package_says_which() {
        let mut needing = version("1.0");
        needing.dependencies = vec![("assets".into(), "1.0".into())];
        let l = listing(vec![package("plug", "Plug", vec![needing]), package("assets", "Assets", vec![version("1.0")])]);
        let installed = BTreeMap::from([
            ("plug".to_string(), Installed { id: "plug".into(), version: "1.0".into(), ..Default::default() }),
            ("assets".to_string(), Installed { id: "assets".into(), version: "1.0".into(), dependency: true, ..Default::default() }),
        ]);
        let all = offers(&l, &installed, &BTreeMap::new());
        let assets = all.iter().find(|o| o.package.id == "assets").unwrap();
        assert_eq!(assets.with, "Plug");
        assert_eq!(describe(assets), "Installed with Plug  ·  Ada  ·  Pads");
    }

    #[test]
    fn the_details_are_each_named() {
        let mut v = version("2.0");
        v.size = 5 << 20;
        v.installed = 12 << 20;
        v.dependencies = vec![("base".into(), "1.1".into())];
        v.exports = vec![("linux-arm64".into(), "https://repo.example/exports/a/2.0/a-2.0-linux-arm64.olib".into()), (String::new(), "https://repo.example/exports/a/2.0/a-2.0.olib".into())];
        let mut p = package("a", "A", vec![v]);
        p.license = "CC0".into();
        let o = &offers(&listing(vec![p]), &BTreeMap::new(), &BTreeMap::new())[0];
        let f = facts(o);
        assert_eq!(f.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Summary", "Version", "License", "Publisher", "Repository", "Size", "Needs", "Download for another device"]);
        assert_eq!(f[3].1, "Ada, key 0123 4567 89ab cdef");
        assert_eq!(f[5].1, "5.0 MB to download, 12.0 MB installed");
        assert_eq!(f[6].1, "base 1.1 or later");
        assert_eq!(downloads(o).iter().map(|d| d.0.as_str()).collect::<Vec<_>>(), ["linux-arm64", "All platforms"]);
        // A withdrawn version has none.
        let mut withdrawn = o.clone();
        withdrawn.version.withdrawn = Some(Withdrawn { repository: "Example".into(), reason: "bad".into(), date: String::new() });
        assert!(downloads(&withdrawn).is_empty());
    }

    #[test]
    fn replacing_a_build_says_both() {
        let mut o = Offer { repository: "Example".into(), package: package("a", "A", vec![]), version: Version { version: "1.0".into(), sha256: "0123456789abcdef0123".into(), ..Default::default() }, ..Default::default() };
        o.record = Installed { sha256: "fedcba9876543210ffff".into(), signers: vec!["1111222233334444".into()], ..Default::default() };
        assert_eq!(
            replace_question(&o),
            "A 1.0 is installed here, but it is not the build Example has.\n\nInstalled: archive fedcba9876543210, signed by key 1111 2222 3333 4444\nExample's: archive 0123456789abcdef, signed by Ada, key 0123 4567 89ab cdef\n\nInstalling the repository's build replaces the installed one."
        );
        o.record.sha256.clear();
        assert!(replace_question(&o).contains("Installed: a build installed some other way, its archive not known"));
    }

    fn signer(status: &str, key: &str, public: &str) -> Signer {
        Signer { kind: "clap".into(), name: "Fuzz".into(), version: "1.0".into(), platform: "linux-arm64".into(), status: status.into(), key: key.into(), public_key: public.into(), ..Default::default() }
    }

    #[test]
    fn the_question_for_code_is_told_by_who_signed_it() {
        // Signed by the key the repository lists for the publisher: trust it and install.
        let q = code_question(&[signer("unknown", "0123456789abcdef", "pub")], "Example", "Ada", "0123456789abcdef");
        assert_eq!(q.title, "Trust Ada to install plugins?");
        assert_eq!(q.buttons, ["Trust and install", "Cancel"]);
        assert_eq!((q.trust, q.anyway), (Some(0), None));
        assert_eq!(q.keys, [("pub".to_string(), "0123456789abcdef".to_string())]);
        assert!(q.message.contains("The repository Example lists this key as Ada's.\n\nKey fingerprint: 0123 4567 89ab cdef"));
        // A stranger's key: trust it, or install this once.
        let q = code_question(&[signer("unknown", "ffffeeeeddddcccc", "pub2")], "Example", "Ada", "0123456789abcdef");
        assert_eq!(q.title, "Install code from an unknown key?");
        assert_eq!(q.buttons, ["Trust this key", "Install anyway", "Cancel"]);
        assert_eq!((q.trust, q.anyway), (Some(0), Some(1)));
        assert!(q.message.contains("signed by key ffff eeee dddd cccc, which Example does not list as Ada's"));
        let q = code_question(&[signer("unknown", "ffffeeeeddddcccc", "pub2"), signer("unknown", "aaaabbbbccccdddd", "pub3")], "", "", "");
        assert_eq!(q.buttons[0], "Trust these keys");
        assert!(q.message.contains("which this repository does not list as this publisher's"));
        // Unsigned: install anyway or not.
        let q = code_question(&[signer("unsigned", "", "")], "Example", "Ada", "x");
        assert_eq!(q.title, "Install unsigned code?");
        assert_eq!((q.buttons.len(), q.trust, q.anyway), (2, None, Some(0)));
        assert!(q.message.starts_with("Fuzz 1.0 in this package is not signed"));
        // A trusted or revoked build is not asked about.
        let q = code_question(&[signer("trusted", "k", "p"), signer("revoked", "k2", "p2"), signer("unsigned", "", "")], "E", "A", "x");
        assert!(q.message.starts_with("Fuzz 1.0 in this package"), "{}", q.message);
        let _ = json!({});
    }
}
