// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the Library lists: an item of `library.query` (hosts/standalone/LibraryPane.cpp's `Control::LibraryItem`),
//! the kind of items a tab shows, and the words the reference uses for them.

use serde_json::Value;

/// The kinds the Library shows, one tab each (LibraryPane.cpp `kind_`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Performance,
    Sound,
    Setlist,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Performance, Kind::Sound, Kind::Setlist];

    /// The item type's kind, as `library.query`'s `kind` takes it.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Performance => "performance",
            Kind::Sound => "sound",
            Kind::Setlist => "setlist",
        }
    }

    pub fn plural(self) -> &'static str {
        match self {
            Kind::Performance => "Performances",
            Kind::Sound => "Sounds",
            Kind::Setlist => "Setlists",
        }
    }

    pub fn singular(self) -> &'static str {
        match self {
            Kind::Performance => "Performance",
            Kind::Sound => "Sound",
            Kind::Setlist => "Setlist",
        }
    }

    pub fn noun(self, n: u64) -> &'static str {
        if n == 1 {
            self.singular()
        } else {
            self.plural()
        }
    }
}

/// An item of the library, as `library.query` lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    /// The item's folder, which `budget.estimate` and `performance.prepare` take.
    pub path: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub color: String,
    pub revision: String,
    /// The installed library that holds it (locked), empty for the user's own.
    pub library: String,
    /// Why it would not fit this device, empty when it does.
    pub unfit: String,
    pub read_only: bool,
    pub in_place: bool,
}

impl Item {
    pub fn from_json(v: &Value) -> Option<Item> {
        let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        Some(Item {
            id: v["id"].as_str()?.to_string(),
            kind: text("kind"),
            name: text("name"),
            description: text("description"),
            path: text("path"),
            tags: v["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect(),
            favorite: v["favorite"] == true,
            color: text("color"),
            revision: text("revision"),
            library: text("library"),
            unfit: text("unfit"),
            read_only: v["readOnly"] == true,
            in_place: v["inPlace"] == true,
        })
    }

    /// Its name, "Untitled" when it has none.
    pub fn title(&self) -> &str {
        if self.name.is_empty() {
            "Untitled"
        } else {
            &self.name
        }
    }

    /// The user's own, not a locked library's.
    pub fn own(&self) -> bool {
        self.library.is_empty()
    }

    /// Its name, description, tags and color may be edited: the user's own, and its type's package not gone nor
    /// indexed in place (RFC 0023 step 3).
    pub fn writable(&self) -> bool {
        self.own() && !self.read_only
    }
}

/// "3 Performances", "1 Sound".
pub fn count(n: u64, kind: Kind) -> String {
    format!("{n} {}", kind.noun(n))
}

/// "5 instruments and effects", "1 instrument or effect" (LibraryPane.cpp `instrumentsAndEffects`).
pub fn instruments_and_effects(n: usize) -> String {
    format!("{n} {}", if n == 1 { "instrument or effect" } else { "instruments and effects" })
}

/// Home's Library tile (LibraryPane.cpp `libraryLine`): "5 instruments and effects · 0 Performances · 3 Sounds", the
/// first part only when plugin packages are installed, the last only when there are Sounds.
pub fn library_line(plugins: usize, performances: u64, sounds: u64) -> String {
    let dot = " · ";
    let mut line = String::new();
    if plugins > 0 {
        line += &format!("{}{dot}", instruments_and_effects(plugins));
    }
    line += &count(performances, Kind::Performance);
    if sounds > 0 {
        line += &format!("{dot}{}", count(sounds, Kind::Sound));
    }
    line
}

/// What an empty list of a kind says, with nothing narrowing it (LibraryPane.cpp `libraryEmpty`): an instrument's
/// presets are not Sounds in the library, and an empty list must not say it has none.
pub fn empty_saying(kind: Kind, plugins: usize) -> String {
    match kind {
        Kind::Setlist => "No setlists yet. The Setlist app makes them.".into(),
        Kind::Sound if plugins > 0 => format!("No Sounds yet. The presets of the {} installed are in the sound picker, not here.", instruments_and_effects(plugins)),
        Kind::Sound => "No Sounds yet. An instrument's own presets are in the sound picker, not here.".into(),
        Kind::Performance if plugins > 0 => format!("No Performances yet. {} {} installed, to start one with.", instruments_and_effects(plugins), if plugins == 1 { "is" } else { "are" }),
        Kind::Performance => "No Performances yet.".into(),
    }
}

/// A sentence on lines of at most `width` characters, broken at spaces (a word longer than a line is kept whole).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_words() {
        assert_eq!(library_line(0, 1, 0), "1 Performance");
        assert_eq!(library_line(5, 0, 3), "5 instruments and effects · 0 Performances · 3 Sounds");
        assert_eq!(library_line(1, 24, 1), "1 instrument or effect · 24 Performances · 1 Sound");
        assert_eq!(empty_saying(Kind::Performance, 1), "No Performances yet. 1 instrument or effect is installed, to start one with.");
        assert_eq!(empty_saying(Kind::Performance, 5), "No Performances yet. 5 instruments and effects are installed, to start one with.");
        assert_eq!(empty_saying(Kind::Sound, 5), "No Sounds yet. The presets of the 5 instruments and effects installed are in the sound picker, not here.");
        assert_eq!(empty_saying(Kind::Sound, 0), "No Sounds yet. An instrument's own presets are in the sound picker, not here.");
        assert_eq!(empty_saying(Kind::Setlist, 0), "No setlists yet. The Setlist app makes them.");
    }

    #[test]
    fn wrapping_keeps_words_whole() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("extraordinarily long", 5), ["extraordinarily", "long"]);
        assert!(wrap("", 5).is_empty());
    }
}
