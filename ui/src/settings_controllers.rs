// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's MIDI controllers page (ui/SETTINGS-APP.md C; MidiControllersPage.cpp is the reference, RFC
//! 0024): each MIDI input with the controller profile that plays it (Automatic, None, one that fits, or each of
//! those by MIDI channel where identical controllers fit) and an Ignore switch, all kept by the engine and applied at
//! once. A refusal is said under its port and the ports listed again; the page lists again when the status says
//! `midiPortList` changed. The reference asks nothing before a change (it is reversible on the same row, and the
//! shell's own panel is not a MIDI input), and neither does this.

use crate::client::Error;
use crate::menu;
use crate::settings_app::{choice, failed, faint, head, said, switch, Ctx, Page, Step as Any};
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// controllers.inputs.
    List,
    /// controllers.choose or controllers.ignore, of the port by name.
    Change(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Port {
    pub name: String,
    /// The controller profiles that fit it: id and name.
    pub profiles: Vec<(String, String)>,
    pub automatic: bool,
    pub chosen: Vec<String>,
    pub playing: Vec<String>,
    pub used: String,
    pub ignored: bool,
}

impl Port {
    /// The ports of `controllers.inputs`, `.choose` and `.ignore` (json_rpc.cpp:1910-1945).
    pub fn all(v: &Value) -> Vec<Port> {
        let ids = |x: &Value| x.as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>();
        v["inputs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| Port {
                name: p["name"].as_str().unwrap_or_default().into(),
                profiles: p["profiles"].as_array().into_iter().flatten().map(|f| (f["id"].as_str().unwrap_or_default().to_string(), f["name"].as_str().unwrap_or_default().to_string())).collect(),
                automatic: p["automatic"].as_bool().unwrap_or(true),
                chosen: ids(&p["chosen"]),
                playing: ids(&p["playing"]),
                used: p["use"].as_str().unwrap_or_default().into(),
                ignored: p["ignored"].as_bool().unwrap_or(false),
            })
            .collect()
    }

    /// A profile's name by its id, as the port lists it.
    fn named(&self, id: &str) -> String {
        self.profiles.iter().find(|p| p.0 == id).map_or_else(|| id.to_string(), |p| p.1.clone())
    }

    /// What the port is, in plain words (MidiControllersPage.cpp `said`).
    pub fn said(&self) -> String {
        if self.ignored {
            return "Ignored: everything it sends is left out.".into();
        }
        let by = self.playing.first().map(|p| self.named(p)).unwrap_or_default();
        match self.used.as_str() {
            "controls" => format!("Played by {by}{}: its controls join.", if self.playing.len() > 1 { " and the others chosen, by MIDI channel" } else { "" }),
            "mackie" => format!("Read as Mackie Control by {by}."),
            "duplicate" => format!("Left out: {by} sends the same on another port."),
            _ if self.profiles.is_empty() => "No controller profile fits it: it plays notes as any MIDI input does.".into(),
            _ => "No profile: it plays notes as any MIDI input does.".into(),
        }
    }

    /// The choice as the list names it: Automatic (the first that fits), None, a profile, or each of them.
    fn choice_text(&self) -> String {
        if self.automatic {
            return match self.profiles.first() {
                Some(p) => format!("Automatic ({})", p.1),
                None => "Automatic".into(),
            };
        }
        match self.chosen.len() {
            0 => "None".into(),
            1 => self.named(&self.chosen[0]),
            _ if self.profiles.len() > 1 => "Each of these, by MIDI channel".into(),
            _ => self.named(&self.chosen[0]),
        }
    }
}

/// The menu's items: Automatic, None, each profile that fits, then each of them together where several do.
const AUTOMATIC: usize = 1;
const NONE: usize = 2;
const FIRST: usize = 3;

#[derive(Default)]
pub struct Controllers {
    ports: Vec<Port>,
    busy: bool,
    list_error: String,
    /// The port a refusal is about, and what it said.
    refused: Option<(String, String)>,
    seen: Option<u64>,
    /// The port whose profile is being chosen.
    asking: Option<usize>,
    listed: bool,
}

impl Controllers {
    fn list(&mut self, c: &mut Ctx) {
        self.busy = true;
        c.ask(Any::Controllers(Step::List), "controllers.inputs", json!({}));
    }

    /// The ports and their rows: a port's profile, then its Ignore switch.
    fn at(&self, row: usize) -> Option<(usize, bool)> {
        (row / 2 < self.ports.len()).then_some((row / 2, row % 2 == 1))
    }

    fn change(&mut self, port: usize, method: &'static str, params: Value, c: &mut Ctx) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.refused = None;
        c.ask(Any::Controllers(Step::Change(self.ports[port].name.clone())), method, params);
    }

    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, c: &mut Ctx) {
        match (step, result) {
            (Step::List, Ok(r)) => {
                self.busy = false;
                self.listed = true;
                self.list_error.clear();
                self.ports = Port::all(&r);
            }
            (Step::List, Err(e)) => {
                self.busy = false;
                self.listed = true;
                self.list_error = e.message;
            }
            (Step::Change(_), Ok(r)) => {
                self.busy = false;
                self.ports = Port::all(&r);
            }
            // Refused: said under its port, and listed again as it is.
            (Step::Change(name), Err(e)) => {
                self.refused = Some((name, e.message));
                self.list(c);
            }
        }
    }
}

impl Page for Controllers {
    fn open(&mut self, c: &mut Ctx) {
        self.seen = None;
        self.refused = None;
        self.list(c);
    }

    fn noun(&self) -> &'static str {
        "Settings"
    }

    fn rows(&self) -> Vec<SettingData> {
        if self.ports.is_empty() {
            return vec![SettingData { label: "MIDI inputs".into(), value: if self.listed { "No MIDI input is connected" } else { "Reading..." }.into(), kind: 4, ..Default::default() }];
        }
        self.ports
            .iter()
            .flat_map(|p| {
                let profile = choice(p.name.as_str(), p.choice_text());
                let profile = if self.busy || p.ignored { faint(profile) } else { profile };
                let ignore = switch("Ignore this port", p.ignored);
                [profile, if self.busy { faint(ignore) } else { ignore }]
            })
            .collect()
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let mut lines = vec![];
        if !self.list_error.is_empty() {
            lines.push(head("The inputs could not be read"));
            lines.extend(failed(&self.list_error));
        }
        let port = at.and_then(|a| self.at(a)).map(|(p, _)| &self.ports[p]).or(if at.is_none() { self.ports.first() } else { None });
        if let Some(p) = port {
            lines.push(head(&p.name));
            lines.extend(said(&p.said()));
            if let Some((name, why)) = &self.refused {
                if *name == p.name {
                    lines.push(head("Refused"));
                    lines.extend(failed(why));
                }
            }
        } else if self.ports.is_empty() && self.listed && self.list_error.is_empty() {
            lines.extend(said("No MIDI input is connected. A controller plugged in shows here."));
        }
        lines
    }

    fn press(&mut self, at: usize, c: &mut Ctx) {
        let Some((i, ignore)) = self.at(at) else { return };
        if self.busy {
            return;
        }
        let port = &self.ports[i];
        if ignore {
            let name = port.name.clone();
            let on = !port.ignored;
            return self.change(i, "controllers.ignore", json!({"input": name, "ignored": on}), c);
        }
        if port.ignored {
            return; // the profile's choice is disabled while ignored
        }
        let selected = if port.automatic {
            AUTOMATIC
        } else if port.chosen.is_empty() {
            NONE
        } else if port.chosen.len() > 1 && port.profiles.len() > 1 {
            FIRST + port.profiles.len()
        } else {
            port.profiles.iter().position(|p| Some(&p.0) == port.chosen.first()).map_or(AUTOMATIC, |k| FIRST + k)
        };
        let mut items = vec![menu::Item::ticked(port.profiles.first().map_or("Automatic".to_string(), |p| format!("Automatic ({})", p.1)), AUTOMATIC, selected == AUTOMATIC), menu::Item::ticked("None", NONE, selected == NONE)];
        items.extend(port.profiles.iter().enumerate().map(|(k, p)| menu::Item::ticked(p.1.as_str(), FIRST + k, selected == FIRST + k)));
        if port.profiles.len() > 1 {
            items.push(menu::Item::ticked("Each of these, by MIDI channel", FIRST + port.profiles.len(), selected == FIRST + port.profiles.len())); // identical controllers, a profile each
        }
        self.asking = Some(i);
        c.menu = Some((format!("Profile for {}", port.name), items));
    }

    fn chosen(&mut self, id: usize, c: &mut Ctx) {
        let Some(i) = self.asking.take().filter(|i| *i < self.ports.len()) else { return };
        let port = &self.ports[i];
        let each = FIRST + port.profiles.len();
        let name = port.name.clone();
        if id == AUTOMATIC {
            self.change(i, "controllers.choose", json!({"input": name, "automatic": true}), c);
        } else {
            let chosen: Vec<&str> = port.profiles.iter().enumerate().filter(|(k, _)| id == each || id == FIRST + k).map(|(_, p)| p.0.as_str()).collect();
            self.change(i, "controllers.choose", json!({"input": name, "profiles": chosen}), c); // None: empty
        }
    }

    fn status(&mut self, s: &Value, c: &mut Ctx) {
        // The inputs came or went, or a choice changed (here or elsewhere): read again.
        let now = s["midiPortList"].as_u64().unwrap_or(0);
        match self.seen {
            None => self.seen = Some(now),
            Some(was) if was != now && !self.busy => {
                self.seen = Some(now);
                self.list(c);
            }
            _ => {}
        }
    }
}
