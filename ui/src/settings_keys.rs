// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Signing keys and About pages (ui/SETTINGS-APP.md K and B; SigningKeysPage.cpp, SigningKeys.h and
//! AboutPage.cpp are the reference). Signing keys lists the keys exports are signed with, by name and fingerprint, and
//! makes one (its name typed on the keyboard; the private half never leaves the device). About says which Oscairn this
//! is, which engine it is connected to and under what license.

use crate::client::Error;
use crate::library::fingerprint;
use crate::settings_app::{choice, failed, head, row, said, Ctx, Page, Step as Any};
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// keys.list.
    List,
    /// keys.make.
    Make,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub name: String,
    pub id: String,
}

#[derive(Default)]
pub struct Keys {
    keys: Vec<Key>,
    busy: bool,
    listed: bool,
    list_error: String,
    asking: bool,
}

impl Keys {
    fn list(&mut self, c: &mut Ctx) {
        self.busy = true;
        c.ask(Any::Keys(Step::List), "keys.list", json!({}));
    }

    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, c: &mut Ctx) {
        match (step, result) {
            (Step::List, Ok(r)) => {
                self.busy = false;
                self.listed = true;
                self.list_error.clear();
                self.keys = r["keys"].as_array().into_iter().flatten().map(|k| Key { name: k["name"].as_str().unwrap_or_default().into(), id: k["id"].as_str().unwrap_or_default().into() }).collect();
            }
            (Step::List, Err(e)) => {
                self.busy = false;
                self.listed = true;
                self.list_error = e.message;
            }
            (Step::Make, Ok(_)) => self.list(c),
            // SigningKeys.h `askToMakeKey`: the error is shown instead.
            (Step::Make, Err(e)) => {
                c.notice = Some(("No key made".into(), e.message, vec!["OK".into()]));
                self.list(c);
            }
        }
    }
}

impl Page for Keys {
    fn open(&mut self, c: &mut Ctx) {
        self.asking = false;
        self.list(c);
    }

    fn noun(&self) -> &'static str {
        "Keys"
    }

    fn rows(&self) -> Vec<SettingData> {
        let mut rows: Vec<SettingData> = self.keys.iter().map(|k| row(k.name.as_str(), fingerprint(&k.id))).collect();
        if self.keys.is_empty() {
            rows.push(row("Signing keys", if self.listed { "No keys yet" } else { "Reading..." }));
        }
        let make = choice("Make a signing key", "name");
        rows.push(if self.busy { crate::settings_app::faint(make) } else { make });
        rows
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let mut lines = vec![];
        if !self.list_error.is_empty() {
            lines.push(head("The keys could not be read"));
            lines.extend(failed(&self.list_error));
        }
        match at.and_then(|a| self.keys.get(a)) {
            Some(k) => {
                lines.push(head(&k.name));
                lines.extend(said(&format!("Fingerprint {}", fingerprint(&k.id))));
            }
            None => {
                lines.push(head("Make a signing key"));
                lines.extend(said("Exports signed with one of these show others they came from you. Each key is shown by its fingerprint."));
                lines.extend(said("A new key's name, which only you see: letters, digits, '.', '-' and '_'. The private key stays on this device."));
            }
        }
        lines
    }

    fn press(&mut self, at: usize, c: &mut Ctx) {
        let make = if self.keys.is_empty() { 1 } else { self.keys.len() };
        if at == make && !self.busy {
            self.asking = true;
            c.prompt = Some(("Name of the new signing key".into(), String::new()));
        }
    }

    fn entered(&mut self, text: &str, c: &mut Ctx) {
        if !std::mem::take(&mut self.asking) {
            return;
        }
        self.busy = true;
        c.ask(Any::Keys(Step::Make), "keys.make", json!({"name": text.trim()}));
    }

    fn not_entered(&mut self, _c: &mut Ctx) {
        self.asking = false;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AboutStep {
    /// packages.preinstalled, for the release the engine is.
    Engine,
}

pub struct About {
    release: String,
    commit: String,
    /// The engine's release; none until it is read.
    engine: Option<Result<String, String>>,
}

impl About {
    pub fn new() -> About {
        About::with(option_env!("OSCAIRN_RELEASE").unwrap_or(env!("CARGO_PKG_VERSION")), option_env!("OSCAIRN_SOURCE_COMMIT").unwrap_or(""))
    }

    pub fn with(release: &str, commit: &str) -> About {
        About { release: release.into(), commit: commit.into(), engine: None }
    }

    pub fn reply(&mut self, _step: AboutStep, result: Result<Value, Error>, _c: &mut Ctx) {
        self.engine = Some(result.map(|r| r["release"].as_str().unwrap_or_default().to_string()).map_err(|e| e.message));
    }

    fn source(&self) -> String {
        if self.commit.is_empty() {
            "Development build".into()
        } else {
            self.commit.chars().take(12).collect()
        }
    }
}

impl Default for About {
    fn default() -> About {
        About::new()
    }
}

impl Page for About {
    fn open(&mut self, c: &mut Ctx) {
        self.engine = None;
        c.ask(Any::About(AboutStep::Engine), "packages.preinstalled", json!({}));
    }

    fn rows(&self) -> Vec<SettingData> {
        let engine = match &self.engine {
            None => "Reading...".to_string(),
            Some(Ok(release)) => release.clone(),
            Some(Err(_)) => "Unknown".into(),
        };
        vec![row("Oscairn", self.release.as_str()), row("Source", self.source()), row("Engine", engine), row("License", "AGPL-3.0-or-later")]
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let mut lines = vec![];
        let section = |name: &str, text: String, lines: &mut Vec<InfoLine>| {
            lines.push(head(name));
            lines.extend(said(&text));
        };
        let all = at.is_none();
        if all || at == Some(0) {
            section("Oscairn", format!("Version {}", self.release), &mut lines);
        }
        if all || at == Some(1) {
            let text = if self.commit.is_empty() { "Built from a working copy, not a release's commit.".to_string() } else { format!("https://github.com/oscairn/oscairn/tree/{}", self.commit) };
            section("Source", text, &mut lines);
        }
        if all || at == Some(2) {
            section("Engine", "The audio engine this app is connected to.".into(), &mut lines);
            if let Some(Err(why)) = &self.engine {
                lines.extend(failed(&format!("The engine's version could not be read: {why}")));
            }
        }
        if all || at == Some(3) {
            let more = if self.commit.is_empty() { "; in a development build, LICENSE and CREDITS.md in the source." } else { "." };
            section(
                "License",
                format!("Free software under the GNU Affero General Public License, version 3 or later, with no warranty. Its text and the notices of what is built into Oscairn are in its Licenses folder (on macOS, Oscairn.app/Contents/Resources/Licenses, and on the disk image){more}"),
                &mut lines,
            );
        }
        lines
    }

    fn press(&mut self, _at: usize, _c: &mut Ctx) {}
}
