// SPDX-License-Identifier: AGPL-3.0-or-later
//! Managing setlists in the Setlist app (issue #41; it was the Library's Setlists tab): the top bar's Manage opens one menu of
//! three submenus, Setlist (new, rename, tags, description, color, duplicate, delete), Bank (new, rename, earlier, later, delete) and Slots
//! (add a Performance, add an empty slot, up, down, remove, move to the end of another bank). An edit changes the document the engine gave (`Setlist::doc`) and
//! saves it whole with `setlists.save`, then the setlists are read again and the one saved shown, the cursor where the edit
//! left it. A duplicate is `library.copy`, a delete `library.delete`, as the Library's were. Names and tags are asked on the
//! keyboard, a delete is confirmed.

use super::*;
use crate::library::actions::{tags_from, COLORS};
use crate::library::setlist::{new_document, MAX_SLOTS};

/// What the menu on screen is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuFor {
    Setlists,
    Banks,
    Manage,
    /// The Performances offered to add.
    Offered,
}

/// What the keyboard is asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ask {
    NewSetlist,
    RenameSetlist,
    Tags,
    Description,
    RenameBank,
}

/// What a question is about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sure {
    DeleteSetlist,
    DeleteBank,
}

/// The management menu's entries.
mod entry {
    pub const NEW: usize = 1;
    pub const RENAME: usize = 2;
    pub const TAGS: usize = 3;
    pub const DESCRIPTION: usize = 6;
    pub const DUPLICATE: usize = 4;
    pub const DELETE: usize = 5;
    pub const BANK_NEW: usize = 10;
    pub const BANK_RENAME: usize = 11;
    pub const BANK_EARLIER: usize = 12;
    pub const BANK_LATER: usize = 13;
    pub const BANK_DELETE: usize = 14;
    pub const ADD: usize = 20;
    pub const GAP: usize = 21;
    pub const UP: usize = 22;
    pub const DOWN: usize = 23;
    pub const REMOVE: usize = 24;
    /// A submenu of the setlist's other banks: `MOVE_TO + j` is the `j`-th bank that is not the slot's.
    pub const MOVE_TO: usize = 25;
    /// A submenu of colors: None, then the nine (kept past the Move-to-bank ids, which can grow with the banks).
    pub const COLOR: usize = 200;
}

/// The most banks a setlist holds (docs/setlist-format.md).
const MAX_BANKS: usize = 100;

fn pages(doc: &mut Value) -> &mut Vec<Value> {
    doc["pages"].as_array_mut().expect("a setlist the screen shows has pages")
}

fn slots(doc: &mut Value, page: usize) -> &mut Vec<Value> {
    pages(doc)[page]["slots"].as_array_mut().expect("a bank has slots")
}

impl Screen {
    /// Manage: the menu of what can be done, from what is open and where the cursor is.
    pub fn manage_menu(&mut self) {
        let open = self.setlist.is_some();
        let (banks, count) = self.setlist.as_ref().map_or((0, 0), |s| (s.pages.len(), s.count()));
        let at = self.target().map_or(0, |t| t.1);
        let on = |item: Item, yes: bool| if yes { item } else { item.off() };
        let target = self.target();
        let current = self.setlist.as_ref().map_or("", |s| s.doc["info"]["color"].as_str().unwrap_or_default());
        let mut colors = vec![Item::ticked("None", entry::COLOR, current.is_empty())];
        colors.extend(COLORS.iter().enumerate().map(|(i, (name, hex))| Item::ticked(*name, entry::COLOR + 1 + i, current == *hex)));
        let setlist = vec![
            Item::choice("New setlist...", entry::NEW),
            on(Item::choice("Rename...", entry::RENAME), open),
            on(Item::choice("Tags...", entry::TAGS), open),
            on(Item::choice("Description...", entry::DESCRIPTION), open),
            on(Item::sub("Color", colors), open),
            on(Item::choice("Duplicate", entry::DUPLICATE), open),
            on(Item::choice("Delete...", entry::DELETE), open),
        ];
        let bank = vec![
            on(Item::choice("New bank", entry::BANK_NEW), open && banks < MAX_BANKS),
            on(Item::choice("Rename...", entry::BANK_RENAME), open),
            on(Item::choice("Move earlier", entry::BANK_EARLIER), self.page > 0),
            on(Item::choice("Move later", entry::BANK_LATER), self.page + 1 < banks),
            on(Item::choice("Delete...", entry::BANK_DELETE), banks > 1),
        ];
        let mut slot = vec![
            on(Item::choice("Add a Performance...", entry::ADD), open && count < MAX_SLOTS && !self.performances.is_empty()),
            on(Item::choice("Add an empty slot", entry::GAP), open && count < MAX_SLOTS),
            on(Item::choice("Move up", entry::UP), target.is_some() && at > 0),
            on(Item::choice("Move down", entry::DOWN), target.is_some() && self.target().is_some_and(|t| t.1 + 1 < self.setlist.as_ref().map_or(0, |s| s.pages[t.0].slots.len()))),
            on(Item::choice("Remove", entry::REMOVE), target.is_some()),
        ];
        if let Some((p, _)) = target {
            if banks > 1 {
                // The other banks a slot can move to, by their name (the slot's own bank left out).
                let s = self.setlist.as_ref().expect("a targeted slot is in a setlist");
                let moves: Vec<Item> = (0..banks).filter(|n| *n != p).enumerate().map(|(j, n)| Item::choice(bank_name(s, n), entry::MOVE_TO + j)).collect();
                slot.push(Item::sub("Move to bank...", moves));
            }
        }
        self.menu_for = MenuFor::Manage;
        self.menu = Some(("Manage".into(), vec![Item::sub("Setlist", setlist), Item::sub("Bank", bank), Item::sub("Slots", slot)], 0));
    }

    /// The slot the menu acts on (its bank and place in it): the one under the cursor, else the last one the cursor or a tap was on.
    fn target(&self) -> Option<(usize, usize)> {
        let s = self.setlist.as_ref()?;
        let index = self.slots.at.map(|a| s.first(self.page) + a).or(self.picked).filter(|i| *i < s.count())?;
        let (page, _) = s.find(index)?;
        Some((page, index - s.first(page)))
    }

    /// The edit made to a copy of the document and saved; `place` is where the cursor goes when it is shown again.
    fn save(&mut self, said: impl Into<String>, edit: impl FnOnce(&mut Value) -> Option<(usize, usize)>) {
        let Some(s) = self.setlist.as_ref() else { return };
        let mut doc = s.doc.clone();
        let place = edit(&mut doc);
        self.out.push((Request::Save(said.into(), place), "setlists.save", json!({"setlist": doc})));
    }

    /// The cursor where the edit saved left it, once the setlist is read again.
    pub(super) fn place(&mut self) {
        let Some((page, at)) = self.pending.take() else { return };
        let Some(s) = self.setlist.as_ref() else { return };
        self.page = page.min(s.pages.len() - 1);
        let count = s.pages[self.page].slots.len();
        self.slots = Cursor::new(count, self.slots.per_page);
        if count > 0 {
            self.slots = self.slots.to(at.min(count - 1));
        }
        self.touched = 1;
        self.bar = self.bar.clear();
    }

    /// A choice in the management menu.
    pub(super) fn manage(&mut self, choice: usize) {
        let Some(s) = self.setlist.clone() else {
            if choice == entry::NEW {
                self.ask(Ask::NewSetlist, "New setlist", "");
            }
            return;
        };
        let page = self.page;
        let bank = s.pages[page].name.clone();
        let target = self.target();
        match choice {
            entry::NEW => self.ask(Ask::NewSetlist, "New setlist", ""),
            entry::RENAME => {
                let name = s.name.clone();
                self.ask(Ask::RenameSetlist, "Setlist name", &name);
            }
            entry::TAGS => {
                let tags = s.doc["info"]["tags"].as_array().map(|t| t.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                self.ask(Ask::Tags, "Tags, separated by commas", &tags);
            }
            entry::DESCRIPTION => {
                let description = s.doc["info"]["description"].as_str().unwrap_or_default().to_string();
                self.ask(Ask::Description, "Setlist description", &description);
            }
            entry::DUPLICATE => {
                let id = s.id.clone();
                self.out.push((Request::Copy, "library.copy", json!({"id": id})));
            }
            entry::DELETE => {
                let name = if s.name.is_empty() { "Untitled".to_string() } else { s.name.clone() };
                self.notice = Some((format!("Delete {name}?"), "The setlist is removed. Its Performances stay in the library.".into(), vec!["Delete".into(), "Cancel".into()]));
                self.notice_for = Some(Sure::DeleteSetlist);
            }
            c if (entry::MOVE_TO..entry::COLOR).contains(&c) => {
                let Some((p, i)) = target else { return };
                let to = (0..s.pages.len()).filter(|n| *n != p).nth(c - entry::MOVE_TO);
                if let Some(to) = to {
                    self.save("Slot moved.", |d| {
                        let slot = slots(d, p).remove(i);
                        let at = slots(d, to).len();
                        slots(d, to).push(slot);
                        Some((to, at))
                    });
                }
            }
            c if c >= entry::COLOR => {
                if let Some(color) = c.checked_sub(entry::COLOR + 1).map_or(Some(""), |i| COLORS.get(i).map(|c| c.1)) {
                    self.save("Color saved.", |d| {
                        d["info"]["color"] = json!(color);
                        None
                    });
                }
            }
            entry::BANK_NEW if s.pages.len() < MAX_BANKS => {
                let at = s.pages.len();
                self.save("Bank added.", |d| {
                    pages(d).push(json!({"name": "", "slots": []}));
                    Some((at, 0))
                });
            }
            entry::BANK_RENAME => self.ask(Ask::RenameBank, "Bank name", &bank),
            entry::BANK_EARLIER | entry::BANK_LATER => {
                let to = if choice == entry::BANK_EARLIER { page.checked_sub(1) } else { Some(page + 1).filter(|n| *n < s.pages.len()) };
                if let Some(to) = to {
                    self.save("Bank moved.", |d| {
                        pages(d).swap(page, to);
                        Some((to, 0))
                    });
                }
            }
            entry::BANK_DELETE if s.pages.len() > 1 => {
                if s.pages[page].slots.is_empty() {
                    self.delete_bank();
                } else {
                    let name = bank_name(&s, page);
                    let n = s.pages[page].slots.len();
                    self.notice = Some((format!("Delete {name}?"), format!("Its {n} slot{} leave the setlist. The Performances stay in the library.", if n == 1 { "" } else { "s" }), vec!["Delete".into(), "Cancel".into()]));
                    self.notice_for = Some(Sure::DeleteBank);
                }
            }
            entry::ADD => {
                let mut all: Vec<(&String, &Performance)> = self.performances.iter().collect();
                all.sort_by(|a, b| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()).then(a.0.cmp(b.0)));
                self.offered = all.iter().map(|p| p.0.clone()).collect();
                let items = all.iter().enumerate().map(|(i, p)| Item::choice(if p.1.name.is_empty() { "Untitled" } else { p.1.name.as_str() }, i)).collect();
                self.menu_for = MenuFor::Offered;
                self.menu = Some(("Add a Performance".into(), items, 0));
            }
            entry::GAP => {
                let at = s.pages[page].slots.len();
                self.save("Empty slot added.", |d| {
                    slots(d, page).push(Value::Null);
                    Some((page, at))
                });
            }
            entry::UP | entry::DOWN => {
                let Some((p, i)) = target else { return };
                let to = if choice == entry::UP { i.checked_sub(1) } else { Some(i + 1).filter(|n| *n < s.pages[p].slots.len()) };
                if let Some(to) = to {
                    self.save("Slot moved.", |d| {
                        slots(d, p).swap(i, to);
                        Some((p, to))
                    });
                }
            }
            entry::REMOVE => {
                let Some((p, i)) = target else { return };
                self.save("Slot removed.", |d| {
                    slots(d, p).remove(i);
                    Some((p, i))
                });
            }
            _ => {}
        }
    }

    /// A Performance chosen in the list offered: the last slot of the bank shown.
    pub(super) fn add_performance(&mut self, row: usize) {
        let Some(id) = self.offered.get(row).cloned() else { return };
        let Some(name) = self.performances.get(&id).map(|p| p.name.clone()) else { return };
        let page = self.page;
        let shown = if name.is_empty() { "Untitled".to_string() } else { name.clone() };
        self.save(format!("{shown} added."), |d| {
            let list = slots(d, page);
            list.push(json!({"id": id, "name": name, "color": "", "label": ""}));
            Some((page, list.len() - 1))
        });
    }

    fn delete_bank(&mut self) {
        let page = self.page;
        self.save("Bank deleted.", |d| {
            pages(d).remove(page);
            Some((page, 0)) // shown clamped to the last bank
        });
    }

    fn ask(&mut self, what: Ask, title: &str, text: &str) {
        self.asked = Some(what);
        self.prompt = Some((title.into(), text.into()));
    }

    /// The text typed for what was asked.
    pub fn entered(&mut self, text: &str) {
        let text = text.trim();
        match self.asked.take() {
            Some(Ask::NewSetlist) if text.is_empty() => self.say("A setlist needs a name."),
            Some(Ask::NewSetlist) => {
                let doc = new_document(text);
                self.out.push((Request::Save(format!("{text} made."), None), "setlists.save", json!({"setlist": doc})));
            }
            Some(Ask::RenameSetlist) => self.save("Name saved.", |d| {
                d["info"]["name"] = json!(text);
                None
            }),
            Some(Ask::Tags) => {
                let tags = tags_from(text);
                self.save("Tags saved.", |d| {
                    d["info"]["tags"] = json!(tags);
                    None
                });
            }
            Some(Ask::Description) => self.save("Description saved.", |d| {
                d["info"]["description"] = json!(text);
                None
            }),
            Some(Ask::RenameBank) => {
                let page = self.page;
                self.save("Bank renamed.", |d| {
                    pages(d)[page]["name"] = json!(text);
                    Some((page, 0))
                });
            }
            None => {}
        }
    }

    /// The keyboard closed with nothing typed.
    pub fn not_entered(&mut self) {
        self.asked = None;
    }

    /// A question for the shell to open (taken).
    pub fn take_notice(&mut self) -> Option<(String, String, Vec<String>)> {
        self.notice.take()
    }

    /// The answer to a question: the first choice is the one asked about.
    pub fn noticed(&mut self, choice: Option<usize>) {
        let what = self.notice_for.take();
        if choice != Some(0) {
            return;
        }
        match (what, self.setlist.as_ref()) {
            (Some(Sure::DeleteSetlist), Some(s)) => {
                let (id, name) = (s.id.clone(), if s.name.is_empty() { "Untitled".to_string() } else { s.name.clone() });
                self.out.push((Request::Delete(name), "library.delete", json!({"id": id})));
            }
            (Some(Sure::DeleteBank), Some(_)) => self.delete_bank(),
            _ => {}
        }
    }
}
