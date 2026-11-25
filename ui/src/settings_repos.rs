// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Package repositories page (ui/SETTINGS-APP.md R; RepositoriesPage.cpp is the reference, RFC
//! 0031): each repository by name with its address and the fingerprint of its root key, a switch to turn it on or
//! off, Remove for those the user added (asked first), and Add by an `https://` address alone. A repository is read
//! first (`packages.list` is a job in the engine, asked about every 100 ms until it ends), so only one that verifies
//! is added; a root key not trusted yet is offered by its fingerprint and trusted only when the player says so, and a
//! repository that still does not verify then is taken out again, key and all.

use crate::client::Error;
use crate::library::fingerprint;
use crate::settings_app::{choice, failed, head, line, said, switch, Ctx, Page, Step as Any};
use crate::topbar::Tool;
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// packages.repositories.
    List,
    /// packages.enableRepository, of the address.
    Enable(String),
    /// packages.removeRepository, of the address.
    Remove(String),
    /// packages.list of the address (the key trusted already or not): the job.
    Read(String, bool),
    /// packages.job of that listing.
    Poll(String, bool),
    /// packages.addRepository of what the job read.
    Add(String),
    /// packages.addRepository to trust the key: the address.
    Trust(String),
    /// packages.removeRepository of one that does not verify even with its key trusted; the reason it gives.
    Drop(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Repository {
    pub address: String,
    pub name: String,
    pub key: String,
    pub id: String,
    pub origin: String,
    pub enabled: bool,
}

impl Repository {
    /// What it is or why it is off (RepositoriesPage.cpp `under`, after its address and root key's fingerprint).
    fn what(&self) -> Option<&'static str> {
        match self.origin.as_str() {
            "official" => Some("Oscairn's own: it can be turned off, never removed"),
            "device" => Some("This device's: it can be turned off, never removed"),
            _ if !self.enabled => Some("turned off: the Catalog reads nothing from it"),
            _ => None,
        }
    }
}

/// What a question asked is about.
#[derive(Clone, Debug, PartialEq)]
enum Asking {
    Address,
    Remove(String),
    /// A root key to trust: the address, the name its root claims and its key.
    Trust(String, String, String),
}

#[derive(Default)]
pub struct Repositories {
    repos: Vec<Repository>,
    busy: bool,
    listed: bool,
    list_error: String,
    /// The repository a refusal is about (its address; empty: the page's) and what it said.
    refused: (String, String),
    /// What an add is doing: "Reading ADDRESS...".
    progress: String,
    asking: Option<Asking>,
    /// The repository last pressed, which Remove is about when the cursor is elsewhere.
    selected: Option<String>,
}

/// The first 16 hex digits of the SHA-256 of a key's 32 raw bytes: the key's ID (`signing::keyId`), as the engine has it.
pub fn key_id(public_key: &str) -> String {
    let bytes: Option<Vec<u8>> = (public_key.len() == 64).then(|| (0..32).map(|i| u8::from_str_radix(public_key.get(2 * i..2 * i + 2)?, 16).ok()).collect::<Option<Vec<u8>>>()).flatten();
    match bytes {
        Some(b) => sha256(&b).iter().take(8).map(|x| format!("{x:02x}")).collect(),
        None => String::new(),
    }
}

/// SHA-256 (FIPS 180-4), for `key_id`: the shell has no other use of a hash.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

impl Repositories {
    fn list(&mut self, c: &mut Ctx) {
        self.busy = true;
        c.ask(Any::Repos(Step::List), "packages.repositories", json!({}));
    }

    /// The repository a row is, and whether it is the Add row.
    fn at(&self, row: usize) -> Option<&Repository> {
        self.repos.get(row)
    }

    fn target(&self, at: Option<usize>) -> Option<&Repository> {
        at.and_then(|a| self.at(a)).or_else(|| self.selected.as_ref().and_then(|s| self.repos.iter().find(|r| r.address == *s)))
    }

    /// An add that did not come to anything: said, the repositories listed again.
    fn not_read(&mut self, why: &str, c: &mut Ctx) {
        self.progress.clear();
        self.refused = (String::new(), format!("Not read: {why}"));
        self.list(c);
    }

    /// Read first, so only a repository that verifies is added (RepositoriesPage.cpp `listThenAdd`).
    fn read(&mut self, address: &str, trusted: bool, c: &mut Ctx) {
        self.busy = true;
        self.refused = (String::new(), String::new());
        self.progress = format!("Reading {address}...");
        c.ask(Any::Repos(Step::Read(address.to_string(), trusted)), "packages.list", json!({"source": address}));
    }

    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, c: &mut Ctx) {
        match (step, result) {
            (Step::List, Ok(r)) => {
                self.busy = false;
                self.listed = true;
                self.list_error.clear();
                self.repos = r["repositories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|x| {
                        let text = |k: &str| x[k].as_str().unwrap_or_default().to_string();
                        Repository { address: text("address"), name: text("name"), key: text("key"), id: text("id"), origin: text("origin"), enabled: x["enabled"].as_bool().unwrap_or(false) }
                    })
                    .collect();
            }
            (Step::List, Err(e)) => {
                self.busy = false;
                self.listed = true;
                self.list_error = e.message;
            }
            (Step::Enable(address), result) => {
                if let Err(e) = result {
                    self.refused = (address, format!("Not changed: {}", e.message));
                }
                self.list(c); // as the engine kept it, refusal or not
            }
            (Step::Remove(address), result) => {
                if let Err(e) = result {
                    self.refused = (address, format!("Not removed: {}", e.message));
                }
                self.list(c);
            }
            (Step::Read(address, trusted), Ok(r)) => match r["job"].as_u64() {
                Some(job) => c.ask(Any::Repos(Step::Poll(address, trusted)), "packages.job", json!({"job": job})),
                None => self.not_read("the engine gave no job", c),
            },
            (Step::Read(_, _), Err(e)) => self.not_read(&e.message, c),
            (Step::Poll(address, trusted), Ok(job)) => self.polled(address, trusted, &job, c),
            (Step::Poll(_, _), Err(e)) => self.not_read(&e.message, c),
            (Step::Add(_), result) => {
                self.progress.clear();
                self.refused = match result {
                    Ok(_) => (String::new(), String::new()),
                    Err(e) => (String::new(), format!("Not added: {}", e.message)),
                };
                self.list(c);
            }
            (Step::Trust(address), result) => match result {
                // The key is trusted now: read it for its name, then keep it.
                Ok(_) => self.read(&address, true, c),
                Err(e) => {
                    self.progress.clear();
                    self.refused = (String::new(), format!("Not added: {}", e.message));
                    self.list(c);
                }
            },
            (Step::Drop(why), _) => self.not_read(&why, c),
        }
    }

    /// An answer of `packages.job` for a listing: still running, or what it came to (RepositoriesPage.cpp).
    fn polled(&mut self, address: String, trusted: bool, job: &Value, c: &mut Ctx) {
        let text = |k: &str| job[k].as_str().unwrap_or_default().to_string();
        match text("state").as_str() {
            "running" => c.ask_after(Duration::from_millis(100), Any::Repos(Step::Poll(address, trusted)), "packages.job", json!({"job": job["job"]})),
            "done" => {
                // Kept under the name its root claims, which the device tells apart from any other by its key.
                self.progress = format!("Adding {address}...");
                c.ask(Any::Repos(Step::Add(address.clone())), "packages.addRepository", json!({"address": address, "key": text("rootKey"), "name": text("claimedName")}));
            }
            _ => {
                let error = text("error");
                if trusted {
                    // It does not verify even with its key just trusted: not kept, nor that key.
                    c.ask(Any::Repos(Step::Drop(error)), "packages.removeRepository", json!({"address": address}));
                } else if !text("rootKey").is_empty() && error.contains("not a repository this device trusts") {
                    let (name, key) = (text("claimedName"), text("rootKey"));
                    self.progress = "Asking whether to trust its root key...".into();
                    c.notice = Some((
                        "Trust this repository?".into(),
                        format!("It calls itself \"{name}\", signed by the root key whose fingerprint is {}. Trust it only if its publisher gave you that fingerprint.", fingerprint(&key_id(&key))),
                        vec!["Trust".into(), "Cancel".into()],
                    ));
                    self.asking = Some(Asking::Trust(address, name, key));
                } else {
                    self.not_read(&error, c);
                }
            }
        }
    }
}

impl Page for Repositories {
    fn open(&mut self, c: &mut Ctx) {
        self.refused = (String::new(), String::new());
        self.progress.clear();
        self.asking = None;
        self.list(c);
    }

    fn noun(&self) -> &'static str {
        "Repositories"
    }

    fn rows(&self) -> Vec<SettingData> {
        let mut rows: Vec<SettingData> = self.repos.iter().map(|r| if self.busy { crate::settings_app::faint(switch(r.name.as_str(), r.enabled)) } else { switch(r.name.as_str(), r.enabled) }).collect();
        let add = choice("Add a repository", "address");
        rows.push(if self.busy { crate::settings_app::faint(add) } else { add });
        rows
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let mut lines = vec![];
        if !self.list_error.is_empty() {
            lines.push(head("The repositories could not be read"));
            lines.extend(failed(&self.list_error));
        }
        match at.and_then(|a| self.at(a)) {
            Some(r) => {
                lines.push(head(&r.name));
                lines.extend(crate::library::files::wrap_path(&r.address, crate::library::INFO_WIDTH).iter().map(|l| line(l)));
                lines.push(head("Root key"));
                lines.push(line(&fingerprint(&r.id)));
                lines.extend(r.what().map(said).unwrap_or_default());
            }
            None if at.is_some() => {
                lines.push(head("Add a repository"));
                lines.extend(said("A new repository is added by its https:// address alone; its root key is trusted only once you have checked its fingerprint."));
                lines.extend(said("To install from a file, use the Library's Install... > File."));
            }
            None => lines.push(line("")),
        }
        if !self.progress.is_empty() {
            lines.push(head("Now"));
            lines.extend(said(&self.progress));
        }
        let (whose, why) = &self.refused;
        if !why.is_empty() && (whose.is_empty() || at.and_then(|a| self.at(a)).is_some_and(|r| r.address == *whose)) {
            lines.push(head("Not done"));
            lines.extend(failed(why));
        }
        lines
    }

    fn message(&self) -> String {
        if self.busy && !self.progress.is_empty() {
            self.progress.clone()
        } else {
            self.refused.1.clone()
        }
    }

    fn press(&mut self, at: usize, c: &mut Ctx) {
        if self.busy {
            return;
        }
        match self.at(at).cloned() {
            Some(r) => {
                self.selected = Some(r.address.clone());
                self.busy = true;
                self.refused = (r.address.clone(), String::new());
                c.ask(Any::Repos(Step::Enable(r.address.clone())), "packages.enableRepository", json!({"address": r.address, "enabled": !r.enabled}));
            }
            None => self.ask_address(c),
        }
    }

    fn tools(&self, at: Option<usize>) -> Vec<Tool> {
        let removable = self.target(at).is_some_and(|r| r.origin == "user") && !self.busy;
        vec![Tool { label: "Add...".into(), enabled: !self.busy, on: false }, Tool { label: "Remove...".into(), enabled: removable, on: false }]
    }

    fn tool(&mut self, i: usize, at: Option<usize>, c: &mut Ctx) {
        if i == 0 {
            if !self.busy {
                self.ask_address(c);
            }
        } else if let Some(r) = self.target(at).filter(|r| r.origin == "user").cloned() {
            if self.busy {
                return;
            }
            c.notice = Some((format!("Remove {}?", r.name), "What you installed from it stays. Its root key is no longer trusted for repositories.".into(), vec!["Remove".into(), "Cancel".into()]));
            self.asking = Some(Asking::Remove(r.address));
        }
    }

    fn noticed(&mut self, choice: Option<usize>, c: &mut Ctx) {
        match (self.asking.take(), choice) {
            (Some(Asking::Remove(address)), Some(0)) => {
                self.busy = true;
                self.refused = (address.clone(), String::new());
                c.ask(Any::Repos(Step::Remove(address.clone())), "packages.removeRepository", json!({"address": address}));
            }
            (Some(Asking::Trust(address, name, key)), Some(0)) => {
                self.progress = "Trusting its root key...".into();
                // Trusts the key for repositories, and lists it.
                c.ask(Any::Repos(Step::Trust(address.clone())), "packages.addRepository", json!({"address": address, "key": key, "name": name}));
            }
            (Some(Asking::Trust(..)), _) => {
                self.busy = false;
                self.progress.clear();
                self.refused = (String::new(), "Not added: its root key was not trusted.".into());
                self.list(c);
            }
            _ => {}
        }
    }

    /// The engine takes http:// too, for a local test server; the app asks for https:// alone, so no one on the way sees
    /// what is fetched (signatures already catch a changed file), and an app may name no folder on the engine's file
    /// system (RFC 0022): a file goes through the Library's Install... > File, which the shell runs.
    fn entered(&mut self, text: &str, c: &mut Ctx) {
        if self.asking.take() != Some(Asking::Address) {
            return;
        }
        let address = text.trim();
        if address.starts_with("https://") {
            self.read(address, false, c);
        } else {
            self.refused = (String::new(), "Not an https address: a repository is added by its https:// address, such as https://packages.example.org. To install from a file, use the Library's Install... > File.".into());
        }
    }

    fn not_entered(&mut self, _c: &mut Ctx) {
        if self.asking == Some(Asking::Address) {
            self.asking = None;
            self.refused = (String::new(), String::new());
        }
    }
}

impl Repositories {
    fn ask_address(&mut self, c: &mut Ctx) {
        self.asking = Some(Asking::Address);
        c.prompt = Some(("The repository's https:// address".into(), String::new()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(address: &str, origin: &str, enabled: bool) -> Repository {
        Repository { address: address.into(), name: String::new(), key: String::new(), id: String::new(), origin: origin.into(), enabled }
    }

    // What a row is comes from its origin first (official and this device's can be turned off, never removed), and a
    // user one is described only while it is turned off; an enabled user one has nothing to add (R1, `under`).
    #[test]
    fn what_says_the_origin_and_only_a_turned_off_user_repository() {
        assert_eq!(repo("https://a", "official", true).what(), Some("Oscairn's own: it can be turned off, never removed"));
        assert_eq!(repo("https://a", "official", false).what(), Some("Oscairn's own: it can be turned off, never removed"));
        assert_eq!(repo("https://a", "device", true).what(), Some("This device's: it can be turned off, never removed"));
        assert_eq!(repo("https://a", "device", false).what(), Some("This device's: it can be turned off, never removed"));
        assert_eq!(repo("https://a", "user", false).what(), Some("turned off: the Catalog reads nothing from it"));
        assert_eq!(repo("https://a", "user", true).what(), None);
        assert_eq!(repo("https://a", "", false).what(), Some("turned off: the Catalog reads nothing from it"));
        assert_eq!(repo("https://a", "", true).what(), None);
    }

    // A row past the list's end is nothing, however large the index is (the Add row and the panel answer that way).
    #[test]
    fn at_reads_a_row_and_nothing_past_the_end() {
        let mut r = Repositories::default();
        assert!(r.at(0).is_none());
        r.repos.push(repo("https://a", "user", true));
        assert_eq!(r.at(0).map(|x| x.address.as_str()), Some("https://a"));
        assert!(r.at(1).is_none());
        assert!(r.at(usize::MAX).is_none());
    }

    // The row the cursor is over answers first; when it is elsewhere (the cursor past the list, or Add), the selected
    // repository answers for Remove, and a selection the list has lost answers nothing.
    #[test]
    fn target_uses_the_row_first_then_the_selected_repository() {
        let mut r = Repositories::default();
        r.repos.push(repo("https://a", "user", true));
        r.repos.push(repo("https://b", "user", true));
        assert_eq!(r.target(Some(1)).map(|x| x.address.as_str()), Some("https://b"));
        assert!(r.target(Some(2)).is_none());
        r.selected = Some("https://a".into());
        assert_eq!(r.target(None).map(|x| x.address.as_str()), Some("https://a"));
        assert_eq!(r.target(Some(9)).map(|x| x.address.as_str()), Some("https://a"));
        r.repos.retain(|x| x.address != "https://a");
        assert!(r.target(None).is_none());
    }
}
