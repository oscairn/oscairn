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

#[cfg(test)]
mod tests {
    use super::*;

    fn port(used: &str) -> Port {
        Port {
            name: "Pad Controller".into(),
            profiles: vec![("padbox".into(), "Pad Box".into()), ("padbox2".into(), "Pad Box Mk 2".into())],
            automatic: true,
            chosen: vec![],
            playing: vec![],
            used: used.into(),
            ignored: false,
        }
    }

    #[test]
    fn all_rejects_a_missing_inputs_field() {
        assert!(Port::all(&json!({})).is_empty());
        assert!(Port::all(&json!({"inputs": "not a list"})).is_empty());
        assert!(Port::all(&json!({"inputs": []})).is_empty());
    }

    #[test]
    fn all_parses_every_field() {
        let p = &Port::all(&json!({"inputs": [{"name": "Keys", "profiles": [{"id": "a", "name": "Alpha"}], "automatic": false, "chosen": ["a"], "playing": ["a"], "use": "controls", "ignored": true}]}))[0];
        assert_eq!(p.name, "Keys");
        assert_eq!(p.profiles, vec![("a".to_string(), "Alpha".to_string())]);
        assert!(!p.automatic);
        assert_eq!(p.chosen, vec!["a".to_string()]);
        assert_eq!(p.playing, vec!["a".to_string()]);
        assert_eq!(p.used, "controls");
        assert!(p.ignored);
    }

    #[test]
    fn all_defaults_missing_and_wrong_typed_fields() {
        let p = &Port::all(&json!({"inputs": [{}]}))[0];
        // No name, no choice made, no ignored: the parser's defaults.
        assert_eq!(p.name, "");
        assert_eq!(p.profiles, Vec::<(String, String)>::new());
        assert!(p.automatic);
        assert_eq!(p.chosen, Vec::<String>::new());
        assert_eq!(p.playing, Vec::<String>::new());
        assert_eq!(p.used, "");
        assert!(!p.ignored);
        // A list that is the wrong type is left out; a profile with an id only is kept with an empty name.
        let q = &Port::all(&json!({"inputs": [{"profiles": [{"id": "x"}], "chosen": 5, "playing": "a"}]}))[0];
        assert_eq!(q.profiles, vec![("x".to_string(), "".to_string())]);
        assert_eq!(q.chosen, Vec::<String>::new());
        assert_eq!(q.playing, Vec::<String>::new());
    }

    #[test]
    fn all_keeps_a_profile_whose_id_only_is_known() {
        // A renamed port: the chosen id no longer fits, kept as it is by id (RFC 0024).
        let p = &Port::all(&json!({"inputs": [{"name": "Keys", "profiles": [{"id": "new", "name": "New"}], "automatic": false, "chosen": ["old"], "use": ""}]}))[0];
        assert_eq!(p.chosen, vec!["old".to_string()]);
    }

    #[test]
    fn named_returns_the_profile_name_or_the_id() {
        let p = port("");
        assert_eq!(p.named("padbox"), "Pad Box");
        assert_eq!(p.named("not-a-profile"), "not-a-profile");
        assert_eq!(port("").with_no_profiles().named("padbox"), "padbox");
    }

    #[test]
    fn said_names_each_use() {
        let played = Port { playing: vec!["padbox".into()], ..port("controls") };
        assert_eq!(played.said(), "Played by Pad Box: its controls join.");
        let both = Port { playing: vec!["padbox".into(), "padbox2".into()], used: "controls".into(), ..port("controls") };
        assert_eq!(both.said(), "Played by Pad Box and the others chosen, by MIDI channel: its controls join.");
        let mackie = Port { playing: vec!["padbox".into()], used: "mackie".into(), ..port("mackie") };
        assert_eq!(mackie.said(), "Read as Mackie Control by Pad Box.");
        let dup = Port { playing: vec!["padbox".into()], used: "duplicate".into(), ..port("duplicate") };
        assert_eq!(dup.said(), "Left out: Pad Box sends the same on another port.");
        assert_eq!(port("").with_no_profiles().said(), "No controller profile fits it: it plays notes as any MIDI input does.");
        assert_eq!(port("").said(), "No profile: it plays notes as any MIDI input does.");
    }

    #[test]
    fn said_leads_with_ignored() {
        let p = Port { ignored: true, used: "controls".into(), ..port("controls") };
        assert_eq!(p.said(), "Ignored: everything it sends is left out.");
        let p = Port { ignored: true, playing: vec!["padbox".into()], used: "mackie".into(), ..port("mackie") };
        assert_eq!(p.said(), "Ignored: everything it sends is left out.");
    }

    #[test]
    fn choice_text_names_a_chosen_profile() {
        assert_eq!(port("").choice_text(), "Automatic (Pad Box)");
        let auto_empty = Port { profiles: vec![], ..port("") };
        assert_eq!(auto_empty.choice_text(), "Automatic");
        let none = Port { automatic: false, ..port("") };
        assert_eq!(none.choice_text(), "None");
        let one = Port { automatic: false, chosen: vec!["padbox".into()], ..port("") };
        assert_eq!(one.choice_text(), "Pad Box");
        let renamed = Port { automatic: false, chosen: vec!["old".into()], ..port("") };
        assert_eq!(renamed.choice_text(), "old");
    }

    #[test]
    fn choice_text_says_each_when_several_are_chosen() {
        let several = Port { automatic: false, chosen: vec!["padbox".into(), "padbox2".into()], ..port("") };
        assert_eq!(several.choice_text(), "Each of these, by MIDI channel");
        // One fitting profile but several chosen ids: the first chosen is named (RFC 0024's renamed port).
        let only_one = Port { profiles: vec![("padbox".into(), "Pad Box".into())], automatic: false, chosen: vec!["padbox".into(), "old".into()], ..port("") };
        assert_eq!(only_one.choice_text(), "Pad Box");
    }

    #[test]
    fn at_maps_every_other_row_to_a_switch() {
        let c = Controllers { ports: vec![port("")], ..Default::default() };
        assert_eq!(c.at(0), Some((0, false)));
        assert_eq!(c.at(1), Some((0, true)));
        assert_eq!(c.at(2), None);
        assert_eq!(c.at(usize::MAX), None);
        let two = Controllers { ports: vec![port(""), port("mackie")], ..Default::default() };
        assert_eq!(two.at(3), Some((1, true)));
        assert_eq!(two.at(4), None);
        assert_eq!(Controllers::default().at(0), None);
    }

    impl Port {
        fn with_no_profiles(self) -> Port {
            Port { profiles: vec![], playing: vec![], ..self }
        }
    }
}
