// SPDX-License-Identifier: AGPL-3.0-or-later
//! Importing another Performance's parts and scenes into the open one (ui/LIBRARY-APP.md D14; LibraryPane.cpp
//! `ImportChoices`, Main.cpp `importPerformance`): a Performance of the library is chosen, its parts and scenes are listed
//! all ticked, and the chosen ones come, parts with what they need. Everything ticked is all of it, loose plugins
//! included.

use super::jobs::Kind as JobKind;
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    pub name: String,
    pub path: String,
    /// Each part: its ID, name, whether it is a bus, whether it comes.
    pub parts: Vec<(u64, String, bool, bool)>,
    pub scenes: Vec<(u64, String, bool)>,
}

impl Import {
    pub fn from_json(name: &str, path: &str, v: &Value) -> Import {
        Import {
            name: name.into(),
            path: path.into(),
            parts: v["parts"].as_array().into_iter().flatten().map(|p| (p["id"].as_u64().unwrap_or(0), p["name"].as_str().unwrap_or_default().to_string(), p["bus"] == true, true)).collect(),
            scenes: v["scenes"].as_array().into_iter().flatten().map(|p| (p["id"].as_u64().unwrap_or(0), p["name"].as_str().unwrap_or_default().to_string(), true)).collect(),
        }
    }

    pub fn rows(&self) -> Vec<SettingData> {
        let mut rows: Vec<SettingData> = self.parts.iter().map(|(_, n, bus, on)| SettingData { label: format!("{n}{}", if *bus { " (bus)" } else { "" }).into(), value: "Part".into(), kind: 1, on: *on, switch: true, ..Default::default() }).collect();
        rows.extend(self.scenes.iter().map(|(_, n, on)| SettingData { label: n.as_str().into(), value: "Scene".into(), kind: 1, on: *on, switch: true, ..Default::default() }));
        let row = |label: &str| SettingData { label: label.into(), value: String::new().into(), kind: 2, ..Default::default() };
        rows.push(row("Import"));
        rows.push(row("Cancel"));
        rows
    }

    pub fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head(&format!("Import from {}", if self.name.is_empty() { "Untitled" } else { &self.name }))];
        lines.extend(wrap("Its parts and scenes come into the open Performance, parts with what they need.", INFO_WIDTH).into_iter().map(line));
        lines
    }

    /// `performance.import`'s parameters, or why not: all of it is two empty lists; some needs a part and, where it has
    /// scenes, a scene, for the scene sets how the parts come.
    pub fn request(&self) -> Result<Value, &'static str> {
        let parts: Vec<u64> = self.parts.iter().filter(|p| p.3).map(|p| p.0).collect();
        let scenes: Vec<u64> = self.scenes.iter().filter(|s| s.2).map(|s| s.0).collect();
        let all = parts.len() == self.parts.len() && scenes.len() == self.scenes.len();
        if !all && (parts.is_empty() || (scenes.is_empty() && !self.scenes.is_empty())) {
            return Err("Choose at least one part, and one scene: the scene sets how the parts come.");
        }
        Ok(if all { json!({"directory": self.path, "parts": [], "scenes": []}) } else { json!({"directory": self.path, "parts": parts, "scenes": scenes}) })
    }
}

impl Screen {
    pub(super) fn import_start(&mut self, id: &str) {
        let Some(item) = self.items.iter().find(|i| i.id == id).cloned() else { return };
        self.say("Reading its parts and scenes...");
        self.importing = Some((item.title().to_string(), item.path.clone()));
        self.push(Request::Lib(libraries::Req::Held), "performance.contents", json!({"directory": item.path}));
    }

    pub(super) fn held_reply(&mut self, result: Result<Value, Error>) {
        let Some((name, path)) = self.importing.take() else { return };
        match result {
            Ok(r) => {
                self.say("");
                self.enter(Mode::Import(Import::from_json(&name, &path, &r)));
            }
            Err(e) => self.say(if e.code == BUSY { "The engine is busy; try again in a moment.".to_string() } else { e.message }),
        }
    }

    pub(super) fn import_press(&mut self, at: usize) {
        let Mode::Import(i) = &mut self.mode else { return };
        let (parts, scenes) = (i.parts.len(), i.scenes.len());
        if at < parts {
            i.parts[at].3 = !i.parts[at].3;
        } else if at < parts + scenes {
            i.scenes[at - parts].2 = !i.scenes[at - parts].2;
        } else if at == parts + scenes {
            match i.request() {
                Ok(params) => {
                    self.start_job(JobKind::Import, "performance.import", params, "Importing...");
                    self.leave();
                }
                Err(why) => self.say(why),
            }
        } else {
            self.leave();
        }
    }
}
