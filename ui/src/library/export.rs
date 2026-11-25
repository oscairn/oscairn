// SPDX-License-Identifier: AGPL-3.0-or-later
//! Exporting (ui/LIBRARY-APP.md C8 and D8; LibraryPane.cpp `exportItem`, `exportLibrary`, `ExportChoices`, SigningKeys.h
//! `askToMakeKey`): an item to an archive file with the choices the JUCE dialog offers, a Performance's edited parts'
//! Sounds too and a signature (off unless a key is chosen), and the user's whole library with the setlists chosen. The
//! engine refuses to write over a file, so an existing one is replaced only once the new archive is written
//! (Engine.cpp:436-445, :471-476): the archive goes to a folder beside it first.

use super::*;
use std::path::{Path, PathBuf};

/// A signing key (`keys.list`): its name, ID and public key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Key {
    pub name: String,
    pub id: String,
}

impl Key {
    pub fn from_json(v: &Value) -> Option<Key> {
        Some(Key { name: v["name"].as_str()?.to_string(), id: v["id"].as_str().unwrap_or_default().to_string() })
    }
}

/// The choices of one item's export.
#[derive(Clone, Debug, PartialEq)]
pub struct Export {
    pub id: String,
    pub name: String,
    /// Whether it is a Performance, which offers its edited parts' Sounds.
    pub performance: bool,
    pub file: PathBuf,
    pub edited: bool,
    /// The key to sign with: its name; none for not signed.
    pub key: Option<String>,
    pub keys: Vec<Key>,
}

impl Export {
    pub fn rows(&self) -> Vec<SettingData> {
        let mut rows = Vec::new();
        if self.performance {
            rows.push(SettingData { label: "With edited parts' Sounds".into(), value: if self.edited { "On" } else { "Off" }.into(), kind: 1, on: self.edited, switch: true, ..Default::default() });
        }
        let signature = self.key.as_ref().map_or("Not signed".to_string(), |k| format!("Sign with {k}"));
        rows.push(SettingData { label: "Signature".into(), value: format!("{signature} ›").into(), kind: 2, ..Default::default() });
        let row = |label: &str| SettingData { label: label.into(), value: String::new().into(), kind: 2, ..Default::default() };
        rows.push(row("Export"));
        rows.push(row("Cancel"));
        rows
    }

    pub fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head(&format!("Export {}", if self.name.is_empty() { "Untitled" } else { &self.name })), line(self.file.display().to_string())];
        if self.performance {
            lines.extend(wrap("Samples and the Sounds of unedited parts always go.", INFO_WIDTH).into_iter().map(line));
        }
        lines.push(head("Signature"));
        lines.extend(wrap("Signing shows others the export came from you; it is off unless you choose a key", INFO_WIDTH).into_iter().map(line));
        lines
    }

    /// `library.export`'s parameters, the archive going to `file`.
    pub fn request(&self, file: &Path) -> Value {
        let mut params = json!({"id": self.id, "file": file.display().to_string(), "editedSounds": self.performance && self.edited});
        if let Some(key) = &self.key {
            params["sign"] = json!({"key": key, "items": true, "archive": true});
        }
        params
    }
}

/// The user's library exported: a name and the setlists it takes with the Performances they name.
#[derive(Clone, Debug, PartialEq)]
pub struct Whole {
    pub name: String,
    /// Each setlist: ID, name, whether it goes.
    pub setlists: Vec<(String, String, bool)>,
}

impl Whole {
    pub fn rows(&self) -> Vec<SettingData> {
        let mut rows = vec![SettingData { label: "Name".into(), value: format!("{} ›", self.name).into(), kind: 2, ..Default::default() }];
        rows.extend(self.setlists.iter().map(|(_, name, on)| SettingData { label: if name.is_empty() { "Untitled".into() } else { name.as_str().into() }, value: "Setlist".into(), kind: 1, on: *on, switch: true, ..Default::default() }));
        let row = |label: &str| SettingData { label: label.into(), value: String::new().into(), kind: 2, ..Default::default() };
        rows.push(row("Export..."));
        rows.push(row("Cancel"));
        rows
    }

    pub fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head("Export library")];
        let said = if self.setlists.is_empty() { "Your own Performances and Sounds, with every sample and plugin they use." } else { "Your own Performances and Sounds, and the setlists chosen here with the Performances they name:" };
        lines.extend(wrap(said, INFO_WIDTH).into_iter().map(line));
        lines
    }

    pub fn request(&self, file: &Path) -> Value {
        json!({"file": file.display().to_string(), "name": self.name, "setlists": self.setlists.iter().filter(|s| s.2).map(|s| s.0.as_str()).collect::<Vec<_>>()})
    }
}

/// Where an archive is written when a file is there already: a folder beside it (".oscairn-export", or the first of
/// ".oscairn-export_2" and on that is not there), the archive in it under the same name; None when the file is new.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplaceInfo {
    pub folder: PathBuf,
    pub written: PathBuf,
    pub target: PathBuf,
}

/// The file the engine is asked to write, and what replaces the old one once it is written.
pub fn plan(target: &Path) -> (PathBuf, Option<ReplaceInfo>) {
    if !target.is_file() {
        return (target.to_path_buf(), None);
    }
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut folder = parent.join(".oscairn-export");
    let mut n = 1;
    while folder.exists() {
        n += 1;
        folder = parent.join(format!(".oscairn-export_{n}"));
    }
    if std::fs::create_dir(&folder).is_err() {
        return (target.to_path_buf(), None); // the engine will say it exists
    }
    let written = folder.join(target.file_name().unwrap_or_default());
    (written.clone(), Some(ReplaceInfo { folder, written, target: target.to_path_buf() }))
}

impl ReplaceInfo {
    /// The archive written: it goes over the old file. An error is said as the JUCE app says it.
    pub fn finish(self, written_ok: bool) -> Result<(), String> {
        let result = if written_ok { std::fs::rename(&self.written, &self.target).map_err(|e| format!("Cannot replace {}: {e}", self.target.display())) } else { Ok(()) };
        let _ = std::fs::remove_dir_all(&self.folder);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oscairn-ui-export-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_new_file_is_written_in_place() {
        let d = dir("new");
        let (file, replace) = plan(&d.join("a.olib"));
        assert_eq!(file, d.join("a.olib"));
        assert!(replace.is_none());
    }

    #[test]
    fn an_old_file_is_replaced_only_once_the_new_one_is_written() {
        let d = dir("old");
        std::fs::write(d.join("a.olib"), "last week's").unwrap();
        let (file, replace) = plan(&d.join("a.olib"));
        assert_eq!(file, d.join(".oscairn-export").join("a.olib"));
        // A failed export keeps the old archive and takes the folder away.
        replace.unwrap().finish(false).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.olib")).unwrap(), "last week's");
        assert!(!d.join(".oscairn-export").exists());
        // A written one goes over it.
        let (file, replace) = plan(&d.join("a.olib"));
        std::fs::write(&file, "this week's").unwrap();
        replace.unwrap().finish(true).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.olib")).unwrap(), "this week's");
        assert!(!d.join(".oscairn-export").exists());
    }

    #[test]
    fn a_folder_already_there_is_not_used() {
        let d = dir("busy");
        std::fs::write(d.join("a.olib"), "x").unwrap();
        std::fs::create_dir(d.join(".oscairn-export")).unwrap();
        let (file, replace) = plan(&d.join("a.olib"));
        assert_eq!(file, d.join(".oscairn-export_2").join("a.olib"));
        replace.unwrap().finish(false).unwrap();
        assert!(d.join(".oscairn-export").exists(), "another one's folder is left alone");
    }

    #[test]
    fn the_request_signs_only_when_a_key_is_chosen() {
        let mut e = Export { id: "p1".into(), name: "A".into(), performance: true, file: PathBuf::from("/x/a.olib"), edited: true, key: None, keys: vec![] };
        assert_eq!(e.request(&e.file), json!({"id": "p1", "file": "/x/a.olib", "editedSounds": true}));
        e.key = Some("mine".into());
        e.performance = false;
        assert_eq!(e.request(&e.file), json!({"id": "p1", "file": "/x/a.olib", "editedSounds": false, "sign": {"key": "mine", "items": true, "archive": true}}));
    }

    #[test]
    fn a_keys_reply_missing_the_name_or_the_id() {
        assert_eq!(Key::from_json(&json!({"name": "mine", "id": "abc"})), Some(Key { name: "mine".into(), id: "abc".into() }));
        assert_eq!(Key::from_json(&json!({"name": "mine"})), Some(Key { name: "mine".into(), id: String::new() }), "no id: the key still lists, unsigned");
        assert_eq!(Key::from_json(&json!({})), None, "no name: it is not a key");
        assert_eq!(Key::from_json(&json!({"name": 3, "id": "abc"})), None, "a name that is not text: not a key");
    }

    #[test]
    fn edited_sounds_follow_the_performance_and_its_edits() {
        let e = Export { id: "p1".into(), name: "A".into(), performance: true, file: PathBuf::from("/x/a.olib"), edited: false, key: None, keys: vec![] };
        assert_eq!(e.request(&e.file), json!({"id": "p1", "file": "/x/a.olib", "editedSounds": false}), "nothing edited: no edited sounds");
        let mut e = e;
        e.key = Some("mine".into());
        assert_eq!(e.request(&e.file), json!({"id": "p1", "file": "/x/a.olib", "editedSounds": false, "sign": {"key": "mine", "items": true, "archive": true}}), "a signature with the sounds still off");
    }

    #[test]
    fn an_items_rows_say_signed_or_not_and_offer_the_sounds_only_for_a_performance() {
        let mut e = Export { id: "p1".into(), name: "A".into(), performance: true, file: PathBuf::from("/x/a.olib"), edited: true, key: None, keys: vec![] };
        let rows = e.rows();
        assert_eq!(rows[0].label.to_string(), "With edited parts' Sounds");
        assert!(rows[0].switch, "the edited sounds choice is a switch");
        assert_eq!(rows[1].label.to_string(), "Signature");
        assert_eq!(rows[1].value.to_string(), "Not signed ›");
        assert_eq!(rows.last().unwrap().label.to_string(), "Cancel");
        e.performance = false;
        let rows = e.rows();
        assert_eq!(rows.iter().filter(|r| r.label == "With edited parts' Sounds").count(), 0, "no sounds row for a Sound");
        e.key = Some("mine".into());
        assert_eq!(e.rows()[0].label.to_string(), "Signature");
        assert_eq!(e.rows()[0].value.to_string(), "Sign with mine ›");
    }

    #[test]
    fn an_untitled_export_and_a_performances_info() {
        let e = Export { id: "p1".into(), name: String::new(), performance: true, file: PathBuf::from("/x/a.olib"), edited: true, key: None, keys: vec![] };
        assert_eq!(e.info()[0].text.to_string(), "Export Untitled");
        let said = |lines: &[InfoLine]| lines.iter().filter(|l| !l.head).map(|l| l.text.to_string()).collect::<Vec<_>>().join(" ");
        assert_eq!(said(&e.info()), "/x/a.olib Samples and the Sounds of unedited parts always go. Signing shows others the export came from you; it is off unless you choose a key", "a Performance: the samples note before the signature's");
        let e = Export { id: "p1".into(), name: "A".into(), performance: false, file: PathBuf::from("/x/a.olib"), edited: true, key: None, keys: vec![] };
        assert_eq!(said(&e.info()), "/x/a.olib Signing shows others the export came from you; it is off unless you choose a key", "a Sound: no samples note");
    }

    #[test]
    fn a_wholes_request_keeps_the_setlists_chosen_in_order() {
        let w = Whole { name: "My library".into(), setlists: vec![("s1".into(), "One".into(), true), ("s2".into(), "Two".into(), false), ("s3".into(), "Three".into(), true)] };
        assert_eq!(w.request(Path::new("/x/all.olib")), json!({"file": "/x/all.olib", "name": "My library", "setlists": ["s1", "s3"]}));
        let w = Whole { name: "My library".into(), setlists: vec![] };
        assert_eq!(w.request(Path::new("/x/all.olib")), json!({"file": "/x/all.olib", "name": "My library", "setlists": []}));
    }

    #[test]
    fn a_wholes_rows_show_its_name_and_nothing_when_there_are_no_setlists() {
        let w = Whole { name: "My library".into(), setlists: vec![("s1".into(), String::new(), true), ("s2".into(), "Two".into(), false)] };
        let rows = w.rows();
        assert_eq!((rows[0].label.to_string(), rows[0].value.to_string()), ("Name".to_string(), "My library ›".to_string()));
        assert_eq!(rows[1].label.to_string(), "Untitled", "a setlist with no name");
        assert!(rows[1].on);
        assert_eq!(rows[2].label.to_string(), "Two");
        assert!(!rows[2].on);
        assert_eq!(rows.last().unwrap().label.to_string(), "Cancel");
        let w = Whole { name: String::new(), setlists: vec![] };
        let rows = w.rows();
        assert_eq!(rows.iter().map(|r| r.label.to_string()).collect::<Vec<_>>(), ["Name", "Export...", "Cancel"], "no setlist rows when none are chosen");
    }

    #[test]
    fn a_wholes_info_says_the_setlists_when_any_are_chosen() {
        let w = Whole { name: "My library".into(), setlists: vec![] };
        assert_eq!(w.info()[0].text.to_string(), "Export library");
        let said = |lines: &[InfoLine]| lines.iter().filter(|l| !l.head).map(|l| l.text.to_string()).collect::<Vec<_>>().join(" ");
        assert_eq!(said(&w.info()), "Your own Performances and Sounds, with every sample and plugin they use.", "with no setlists: the plain note");
        let w = Whole { name: "My library".into(), setlists: vec![("s1".into(), "One".into(), true)] };
        assert!(said(&w.info()).contains("and the setlists chosen here with the Performances they name:"), "the phrase once a setlist is chosen");
    }
}
