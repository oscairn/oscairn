// SPDX-License-Identifier: AGPL-3.0-or-later
//! What is done with an item (LibraryPane.cpp `itemMenu`, `editItem`, `copyItem`, `deleteItem`, `tagMenu`): each
//! entry of its menu, the menu itself, a tag's, and the question before a delete.

use super::*;

/// The colors the menus give a Performance, a Sound or a tag (PerformanceColors.h `performanceColors`).
pub const COLORS: [(&str, &str); 9] = [
    ("Red", "#e5484d"),
    ("Orange", "#f76b15"),
    ("Yellow", "#ffc53d"),
    ("Green", "#46a758"),
    ("Teal", "#12a594"),
    ("Blue", "#3e63dd"),
    ("Purple", "#8e4ec6"),
    ("Pink", "#d6409f"),
    ("Gray", "#8b8d98"),
];

/// A menu entry that can be chosen or not, as its item says.
trait Enabled {
    fn enabled(self, on: bool) -> Self;
}

impl Enabled for menu::Item {
    fn enabled(self, on: bool) -> menu::Item {
        if on {
            self
        } else {
            self.off()
        }
    }
}

/// The numbers of the menu's entries.
mod entry {
    pub const OPEN: usize = 1;
    pub const SETLIST: usize = 2;
    pub const NEW_PART: usize = 3;
    pub const FAVORITE: usize = 4;
    pub const EDIT: usize = 5;
    pub const COPY: usize = 6;
    pub const DELETE: usize = 7;
    pub const EXPORT: usize = 8;
    pub const AUDITION: usize = 9;
    pub const IMPORT: usize = 10;
    /// A submenu of colors: None, then the nine.
    pub const COLOR: usize = 100;
    /// A tag's menu.
    pub const HIDE: usize = 1;
}

impl Screen {
    /// A press on an item's row. A Performance opens; while one is being tried, any row tried instead; anything else
    /// lists what can be done with it (the JUCE click does nothing for a Sound or a setlist: its Options does it).
    pub(super) fn item_pressed(&mut self, i: usize) -> Outcome {
        let Some(item) = self.items.get(i).cloned() else { return Outcome::Stay };
        self.selected = Some(item.id.clone());
        if !self.auditioning.id.is_empty() {
            if item.id != self.auditioning.id {
                self.start_audition(&item.id);
            }
            return Outcome::Stay;
        }
        if item.kind == "performance" {
            self.open_performance(&item.id);
        } else {
            self.item_menu(&item.id);
        }
        Outcome::Stay
    }

    /// Everything for one item, by its kind and by whether it is the user's own or a locked library's.
    pub(super) fn item_menu(&mut self, id: &str) {
        let Some(item) = self.items.iter().find(|i| i.id == id).cloned() else { return };
        let mut items = Vec::new();
        match item.kind.as_str() {
            "performance" => {
                items.push(menu::Item::choice("Open", entry::OPEN));
                items.push(menu::Item::choice("Add to setlist", entry::SETLIST));
            }
            "sound" => items.push(menu::Item::choice("New part", entry::NEW_PART)),
            _ => {}
        }
        if item.kind == "performance" || item.kind == "sound" {
            items.push(menu::Item::choice(if self.auditioning.id == item.id { "Stop audition" } else { "Audition" }, entry::AUDITION));
        }
        items.push(menu::Item::choice(if item.favorite { "Remove from favorites" } else { "Add to favorites" }, entry::FAVORITE));
        let writable = item.writable(); // RFC 0023 step 3: its type's package gone, or indexed in place
        items.push(menu::Item::choice("Edit name, description and tags...", entry::EDIT).enabled(writable));
        let mut colors = vec![menu::Item::ticked("None", entry::COLOR, item.color.is_empty())];
        colors.extend(COLORS.iter().enumerate().map(|(i, (name, hex))| menu::Item::ticked(*name, entry::COLOR + 1 + i, item.color == *hex)));
        items.push(menu::Item::sub("Color", colors).enabled(writable));
        items.push(menu::Item::choice(if item.own() { "Duplicate" } else { "Copy into my own, to edit" }, entry::COPY).enabled(!item.read_only));
        items.push(menu::Item::choice("Export...", entry::EXPORT).enabled(!item.in_place));
        if item.kind == "performance" {
            items.push(menu::Item::choice("Import into the open Performance...", entry::IMPORT));
        }
        items.push(menu::Item::choice("Delete...", entry::DELETE).enabled(!item.in_place));
        self.menu = Some((item.title().to_string(), items));
        self.menu_for = Some(MenuFor::Item(item.id));
    }

    /// A tag's menu: hide it from the tag list, or its color.
    pub(super) fn tag_menu(&mut self, tag: &str) {
        let current = self.tag_colors.iter().find(|(t, _)| t == tag).map(|(_, c)| c.clone()).unwrap_or_default();
        let mut colors = vec![menu::Item::ticked("None", entry::COLOR, current.is_empty())];
        colors.extend(COLORS.iter().enumerate().map(|(i, (name, hex))| menu::Item::ticked(*name, entry::COLOR + 1 + i, current == *hex)));
        self.menu = Some((tag.to_string(), vec![menu::Item::choice("Hide from the tag list", entry::HIDE), menu::Item::sub("Color", colors)]));
        self.menu_for = Some(MenuFor::Tag(tag.to_string()));
    }

    /// A choice in the menu the screen opened.
    pub fn chosen(&mut self, choice: usize) -> Outcome {
        let Some(what) = self.menu_for.take() else { return Outcome::Stay };
        let color_of = |choice: usize| match choice.checked_sub(entry::COLOR) {
            Some(0) => Some(String::new()),
            Some(n) => COLORS.get(n - 1).map(|c| c.1.to_string()),
            None => None,
        };
        let outcome = match what {
            MenuFor::Tag(tag) => {
                let current = self.tag_colors.iter().find(|(t, _)| *t == tag).map(|(_, c)| c.clone()).unwrap_or_default();
                if choice == entry::HIDE {
                    // A hidden tag chosen as a filter stops filtering.
                    self.filters.chosen.retain(|t| *t != tag);
                    self.push(Request::Do(Do::Tag), "library.tag", json!({"tag": tag, "hidden": true, "color": current}));
                } else if let Some(color) = color_of(choice) {
                    self.push(Request::Do(Do::Tag), "library.tag", json!({"tag": tag, "hidden": false, "color": color}));
                }
                self.query();
                Outcome::Stay
            }
            MenuFor::Library(menus) => self.library_chosen(menus, choice),
            MenuFor::Item(id) => {
                let Some(item) = self.items.iter().find(|i| i.id == id).cloned() else { return Outcome::Stay };
                match choice {
                    entry::OPEN => self.open_performance(&id),
                    entry::SETLIST => self.add_to_setlist(&id),
                    entry::NEW_PART => {
                        self.push(Request::Do(Do::NewPart(item.title().into())), "parts.fromSound", json!({"id": id}));
                        self.say(format!("Adding {} as a new part", item.title()));
                    }
                    entry::FAVORITE => self.push(Request::Do(Do::Favorite), "library.favorite", json!({"id": id, "favorite": !item.favorite})),
                    entry::EDIT => {
                        self.mode = Mode::Edit(Draft { id, name: item.name.clone(), description: item.description.clone(), tags: item.tags.join(", "), color: item.color.clone() });
                        self.b.reset_cursor();
                    }
                    entry::COPY => self.push(Request::Do(Do::Copy(item.title().into(), item.own())), "library.copy", json!({"id": id})),
                    entry::IMPORT => self.import_start(&id),
                    entry::EXPORT => {
                        let name = item.title().to_string();
                        self.files_open(super::files::Want::Save, "olib", &name, super::files::After::ExportItem(id), &format!("Export {name}"), false);
                    }
                    entry::DELETE => {
                        let message = if item.own() { "It is removed from your library. Performances that used it keep their parts." } else { "It is removed. Installing its library again brings it back." };
                        self.notice = Some((format!("Delete {}?", item.title()), message.into(), vec!["Delete".into(), "Cancel".into()]));
                        self.notice_for = Some(NoticeFor::Delete(id));
                    }
                    entry::AUDITION => {
                        if self.auditioning.id == id {
                            self.stop_audition(false);
                        } else {
                            self.start_audition(&id);
                        }
                    }
                    n if n >= entry::COLOR && item.writable() => {
                        if let Some(color) = color_of(n) {
                            self.push(
                                Request::Do(Do::Edit(item.title().into())),
                                "library.edit",
                                json!({"id": id, "name": item.name, "description": item.description, "tags": item.tags, "color": color}),
                            );
                        }
                    }
                    _ => {}
                }
                Outcome::Stay
            }
        };
        self.show();
        outcome
    }

    /// The answer to a question the screen asked.
    pub fn noticed(&mut self, choice: Option<usize>) -> Outcome {
        let outcome = match self.notice_for.take() {
            Some(NoticeFor::SaveFirst) => self.save_first(choice),
            Some(NoticeFor::Alert) => Outcome::Stay,
            Some(NoticeFor::Replace(o)) => {
                self.replace_answer(*o, choice);
                Outcome::Stay
            }
            Some(NoticeFor::Code(ctx, q)) => {
                self.code_answer(ctx, *q, choice);
                Outcome::Stay
            }
            Some(other @ (NoticeFor::Overwrite(..) | NoticeFor::RemovePlugin(_) | NoticeFor::Uninstall(..))) => {
                self.library_noticed(other, choice);
                Outcome::Stay
            }
            Some(NoticeFor::Delete(id)) => {
                if choice == Some(0) {
                    if let Some(item) = self.items.iter().find(|i| i.id == id) {
                        let name = item.title().to_string();
                        self.push(Request::Do(Do::Delete(name)), "library.delete", json!({"id": id}));
                    }
                }
                Outcome::Stay
            }
            None => Outcome::Stay,
        };
        self.show();
        outcome
    }

    /// Save in the editor: the item's name, description and tags (separated by commas, trimmed, each once).
    pub(super) fn edit_save(&mut self) {
        let Mode::Edit(d) = self.mode.clone() else { return };
        let mut tags: Vec<String> = Vec::new();
        for t in d.tags.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            if !tags.iter().any(|x| x == t) {
                tags.push(t.to_string());
            }
        }
        let shown = if d.name.is_empty() { "Untitled".to_string() } else { d.name.clone() };
        self.push(Request::Do(Do::Edit(shown)), "library.edit", json!({"id": d.id, "name": d.name, "description": d.description, "tags": tags, "color": d.color}));
        self.mode = Mode::List;
        self.b.reset_cursor();
    }

    /// An answer to one of the actions above. True when the Performance changed under the screen.
    pub(super) fn done(&mut self, what: Do, result: Result<Value, Error>) -> bool {
        match result {
            Err(e) if e.code == BUSY => {
                self.say("The engine is busy; try again in a moment.");
                false
            }
            Err(e) => {
                self.say(e.message);
                if matches!(what, Do::Edit(_) | Do::Copy(..) | Do::Delete(_) | Do::Favorite | Do::Tag) {
                    self.query();
                }
                false
            }
            Ok(_) => match what {
                Do::Favorite | Do::Tag => {
                    self.query(); // the star shows at once: looked at again, as the reference's refresh
                    false
                }
                Do::NewPart(_) => {
                    self.say("Sound loaded.");
                    true
                }
                Do::Edit(name) => {
                    self.query();
                    self.say(format!("{name} saved."));
                    false
                }
                Do::Copy(name, own) => {
                    self.query();
                    self.say(if own { format!("{name} duplicated.") } else { format!("{name} copied into your own, to edit.") });
                    false
                }
                Do::Delete(name) => {
                    self.query();
                    self.say(format!("{name} deleted."));
                    false
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A library screen on a headless window (no engine), its list and mode as the test sets them.
    fn screen() -> (crate::tests::Screen, Screen) {
        let t = crate::tests::Screen::new();
        let c = Screen::new(&t.ui);
        (t, c)
    }

    /// `edit_save` sends the tags split on commas, trimmed, emptied, each once in order, with the color kept; a
    /// Performance without a name is saved under "Untitled" (LibraryPane.cpp `editItem`'s fallback).
    #[test]
    fn edit_save_normalizes_tags_and_names() {
        let (_t, mut c) = screen();
        let edited = |c: &mut Screen, tags: &str, name: &str| {
            c.mode = Mode::Edit(Draft { id: "p1".into(), name: name.into(), description: String::new(), tags: tags.into(), color: String::new() });
            c.out.clear();
            c.edit_save();
            c.out.last().expect("edit_save sends a save").params.clone()
        };
        assert_eq!(edited(&mut c, "", "A")["tags"], json!([]), "no tags");
        assert_eq!(edited(&mut c, "jazz", "A")["tags"], json!(["jazz"]));
        assert_eq!(edited(&mut c, " jazz  ", "A")["tags"], json!(["jazz"]), "a tag is trimmed");
        assert_eq!(edited(&mut c, "jazz,,", "A")["tags"], json!(["jazz"]), "empty tokens are dropped");
        assert_eq!(edited(&mut c, ", ,", "A")["tags"], json!([]), "only whitespace: none");
        assert_eq!(edited(&mut c, "a, b , a, b", "A")["tags"], json!(["a", "b"]), "each once, the first kept, in order");
        assert_eq!(edited(&mut c, "jazz, народ, folk", "A")["tags"], json!(["jazz", "народ", "folk"]), "Unicode kept");
        let long = format!("tag{}", "x".repeat(500));
        assert_eq!(edited(&mut c, &long, "A")["tags"], json!([long]), "a very long tag is kept whole");
        // A Performance with no name is saved under Untitled while the name is sent as typed.
        let p = edited(&mut c, "", "");
        assert_eq!(p["name"], "", "the empty name is sent as it was typed");
        assert_eq!(c.out.last().unwrap().request, Request::Do(Do::Edit("Untitled".into())), "saved under Untitled");
    }

    /// A color choice maps to its hex (None, the first entry, clears it), the first and last of the colors; a choice
    /// past them is ignored rather than reading past the list.
    #[test]
    fn a_color_choice_maps_to_its_hex_or_nothing() {
        let (_t, mut c) = screen();
        c.items = vec![Item { id: "p1".into(), kind: "performance".into(), name: "A".into(), ..Default::default() }];
        c.menu_for = Some(MenuFor::Item("p1".into()));
        let color_of = |c: &mut Screen, choice: usize| {
            // `chosen` takes `menu_for` for the call, so a fresh one each time.
            c.menu_for = Some(MenuFor::Item("p1".into()));
            c.out.clear();
            c.chosen(choice);
            c.out.last().map(|s| s.params["color"].as_str().unwrap_or_default().to_string()).unwrap_or_default()
        };
        assert_eq!(color_of(&mut c, entry::COLOR), "", "None clears the color");
        assert_eq!(color_of(&mut c, entry::COLOR + 1), COLORS[0].1, "the first, Red");
        assert_eq!(color_of(&mut c, entry::COLOR + COLORS.len()), COLORS[COLORS.len() - 1].1, "the last");
        c.menu_for = Some(MenuFor::Item("p1".into()));
        c.out.clear();
        c.chosen(entry::COLOR + COLORS.len() + 1);
        assert!(c.out.is_empty(), "a choice past the colors is ignored, not an index out of bounds");
    }
}
