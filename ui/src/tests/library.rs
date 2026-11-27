// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library app's first cut (library/, ui/LIBRARY-APP.md rows A, B and C) against a fake engine that answers
//! as the real handlers do (library_fake.rs): the list, the kinds, the filters, opening a Performance and trying
//! one, the item menu and what it does, each by the cursor and by touch.

use super::library_fake::{stored, Library as Fake, Stored};
use super::*;
use crate::library::{Outcome, Screen as Library};
use crate::{LibraryState, PartState, SettingData};
use std::time::Duration;

pub fn rows(ui: &Shell) -> Vec<SettingData> {
    ui.global::<PartState>().get_rows().iter().collect()
}

pub fn labels(ui: &Shell) -> Vec<String> {
    rows(ui).into_iter().map(|r| r.label.to_string()).collect()
}

pub fn value_of(ui: &Shell, label: &str) -> String {
    rows(ui).into_iter().find(|r| r.label == label).unwrap_or_else(|| panic!("{label} in {:?}", labels(ui))).value.to_string()
}

pub fn info(ui: &Shell) -> Vec<String> {
    ui.global::<PartState>().get_info().iter().map(|l| l.text.to_string()).collect()
}

pub fn footer(ui: &Shell) -> (String, String) {
    let state = ui.global::<PartState>();
    (state.get_message().to_string(), state.get_view().to_string())
}

pub fn tabs(ui: &Shell) -> Vec<String> {
    ui.global::<LibraryState>().get_pages().iter().map(|p| p.to_string()).collect()
}

/// The fixture: a few Performances, Sounds and a setlist, under invented names.
pub fn library() -> Vec<Stored> {
    vec![
        stored("performance", "p1", "Evening Strings").tags(&["strings", "warm"]).described("Slow and wide").starred(),
        stored("performance", "p2", "").tags(&[]).locked("friend"),
        stored("performance", "p3", "Glass Choir").tags(&["choir", "pad"]).unfit("needs 3 GB of memory").colored("#3e63dd"),
        stored("performance", "p4", "Brass Corner").tags(&["brass", "warm"]),
        stored("sound", "s1", "Soft Felt").tags(&["keys", "warm"]).described("Muted piano"),
        stored("sound", "s2", "Hollow Reed").tags(&["woodwind"]).locked("friend"),
        stored("setlist", "l1", "Friday Set"),
    ]
}

pub struct Rig {
    pub s: super::Screen,
    pub c: Library,
    pub e: Fake,
    /// What was asked later than at once, with the delay.
    pub delayed: Vec<(String, Duration)>,
    waiting: Vec<crate::library::Send>,
    /// Whether any answer said the Performance changed under the screen.
    pub again: bool,
}

impl Rig {
    pub fn new(items: Vec<Stored>) -> Rig {
        let s = super::Screen::new();
        tall(&s.ui);
        let mut c = Library::new(&s.ui);
        c.part = 10;
        c.open();
        c.shown();
        let mut e = Fake::new(items);
        e.libraries = vec![("friend".into(), "A Friend's".into())];
        let mut r = Rig { s, c, e, delayed: vec![], waiting: vec![], again: false };
        r.settle();
        r
    }

    /// Answers what the screen asked, and what that made it ask, until it asks no more (a request to be asked later
    /// is noted, not asked).
    pub fn settle(&mut self) {
        for _ in 0..50 {
            let outs = std::mem::take(&mut self.c.out);
            if outs.is_empty() {
                return;
            }
            for send in outs {
                if let Some(after) = send.after {
                    self.delayed.push((send.method.to_string(), after));
                    self.waiting.push(send);
                    continue;
                }
                let result = self.e.handle(send.method, &send.params);
                self.again |= self.c.reply(send.request, result);
            }
        }
        panic!("the screen never stopped asking");
    }

    /// Answers one request that was asked later.
    pub fn later(&mut self) {
        let outs = std::mem::take(&mut self.waiting);
        for send in outs {
            let result = self.e.handle(send.method, &send.params);
            self.again |= self.c.reply(send.request, result);
        }
    }

    /// Answers what was asked later, and what that makes it ask, until nothing is waiting.
    pub fn later_all(&mut self) {
        for _ in 0..50 {
            if self.waiting.is_empty() {
                return;
            }
            self.later();
            self.settle();
        }
        panic!("the screen kept asking later");
    }

    pub fn ui(&self) -> &Shell {
        &self.s.ui
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let o = self.c.act(action);
        self.settle();
        o
    }

    /// The cursor onto row `label` (the first move shows it on the first row), and a press.
    pub fn press_row(&mut self, label: &str) -> Outcome {
        let target = labels(self.ui()).iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {:?}", labels(self.ui())));
        let now = match rows(self.ui()).iter().position(|x| x.cursor) {
            Some(at) => at as i32,
            None => {
                self.act(Action::Move { pane: None, by: 1 });
                0
            }
        };
        if target as i32 != now {
            self.act(Action::Move { pane: None, by: target as i32 - now });
        }
        self.act(Action::Confirm(None))
    }

    /// The sidebar's lines as the window has them.
    pub fn side(&self) -> Vec<SettingData> {
        self.ui().global::<LibraryState>().get_side_rows().iter().collect()
    }

    pub fn side_labels(&self) -> Vec<String> {
        self.side().into_iter().map(|r| r.label.to_string()).collect()
    }

    /// A touch on the sidebar's line `label`.
    pub fn pick(&mut self, label: &str) {
        let at = self.side_labels().iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {:?}", self.side_labels()));
        self.c.side_tap(at as i32);
        self.settle();
    }

    /// The search box asked and answered: the keyboard's title and what it started with.
    pub fn search(&mut self, text: &str) -> Outcome {
        let o = self.c.ask_search();
        self.c.entered(text);
        self.settle();
        o
    }

    pub fn tool(&mut self, label: &str) -> Outcome {
        let at = self.c.bar_tools().iter().position(|t| t.label.starts_with(label)).unwrap_or_else(|| panic!("{label} in {:?}", self.c.bar_tools()));
        let o = self.c.tool(at);
        self.settle();
        o
    }
}

// Opening the screen asks for the Performances (the most the engine gives a time), the libraries installed, the
// plugin packages installed and the packages that come with the release; nothing else, and nothing again at rest.
#[test]
fn opening_asks_for_what_it_shows() {
    let r = Rig::new(library());
    assert_eq!(r.e.methods(), ["library.query", "library.libraries", "packages.installed", "packages.preinstalled"]);
    assert_eq!(r.e.calls[0].1, json!({"kind": "performance", "limit": 1000}));
    assert_eq!(r.e.calls[1].1, json!({}));
    assert!(r.delayed.is_empty(), "nothing is asked again at rest: {:?}", r.delayed);
    assert_eq!(tabs(r.ui()), ["Performances", "Sounds"], "setlists are the Setlist app's");
}

// A row says the item's name ("Untitled" when it has none), its tags and description, "Locked" for an installed
// library's, "Does not fit this device", a star for a favorite, and its color goes where the theme puts it; the
// line under the list counts the items (not the Filters row).
#[test]
fn a_row_is_the_library_panes() {
    let r = Rig::new(library());
    let ui = r.ui();
    assert_eq!(labels(ui), ["Untitled", "Brass Corner", "Evening Strings", "Glass Choir"]);
    assert_eq!(value_of(ui, "Evening Strings"), "strings  ·  warm");
    assert!(rows(ui).iter().find(|r| r.label == "Evening Strings").is_some_and(|r| r.star && !r.warn), "a favorite has the star");
    assert_eq!(rows(ui).iter().find(|r| r.label == "Evening Strings").map(|r| (r.detail.to_string(), r.fluid)), Some(("Slow and wide".to_string(), true)), "the description is the row's detail, apart from its tags");
    assert_eq!(value_of(ui, "Untitled"), "Locked");
    assert_eq!(value_of(ui, "Glass Choir"), "choir  ·  pad");
    assert!(rows(ui).iter().find(|r| r.label == "Glass Choir").is_some_and(|r| r.warn && !r.star && r.reason == "needs 3 GB of memory"), "the warning icon, the reason with it");
    assert_eq!(value_of(ui, "Brass Corner"), "brass  ·  warm");
    assert_eq!(rows(ui).iter().map(|r| r.tinted).collect::<Vec<_>>(), [false, false, false, true], "only the one with a color is tinted");
    assert_eq!(footer(ui).1, "Performances 1–4 of 4");
}

// The kinds are the tabs: choosing Sounds asks for Sounds, with the tags chosen for the others cleared; the search
// stays.
#[test]
fn the_kinds_are_tabs_and_clear_the_tags() {
    let mut r = Rig::new(library());
    r.e.calls.clear();
    r.pick("warm");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "tags": ["warm"]}));
    r.e.calls.clear();
    r.c.show_page(1);
    r.settle();
    assert_eq!(r.e.called("library.query"), [json!({"kind": "sound", "limit": 1000})], "the tags went with the kind");
    assert_eq!(labels(r.ui()), ["Hollow Reed", "Soft Felt"]);
    assert_eq!(r.ui().global::<LibraryState>().get_page(), 1);
    assert_eq!(footer(r.ui()).1, "Sounds 1–2 of 2");
    r.c.show_page(2);
    r.settle();
    assert_eq!(labels(r.ui()), ["Hollow Reed", "Soft Felt"], "there is no third tab: the Sounds stay");
}

/// Every name the Library's fixtures use for an item, a library, a package, a publisher or a signer: invented, none a person's
/// or a company's (the repository is public). A new fixture name is added here on purpose.
pub const INVENTED: &[&str] = &[
    "An invented package with a summary long enough that it has to be cut",
    "Big and bright",
    "Brass Corner",
    "Brass Hall",
    "Bus A",
    "Chorus",
    "Dawn Pad",
    "Dawn Pads",
    "Dusk Lead With A Name Long Enough To Be Cut",
    "Encore",
    "Evening Strings",
    "Fine",
    "Friday Set",
    "Friend Pack",
    "Fuzz",
    "Fuzz Box",
    "Glass Choir",
    "Hiss",
    "Hollow Reed",
    "Invented",
    "Key ffffeeeeddddcccc",
    "Lead",
    "Lv",
    "Old",
    "Old Organ",
    "One",
    "Pad Synth",
    "Page 1",
    "Pebble",
    "Quill",
    "Quill Audio Works",
    "Renamed",
    "S",
    "Set 1",
    "Setlist",
    "Sketch One",
    "Soft Felt",
    "Stage Pack",
    "synth",
    "Two",
    "Verse",
    "Warm Bass",
    "x",
    "",
];

// The names the Library's fixtures put in an item, a publisher, a signer, a package or a summary come from the invented list:
// the sources of every Library test are read for them, so a real person's or company's name cannot come back without that
// list being edited on purpose.
#[test]
fn fixture_names_come_from_the_invented_list() {
    let sources = [
        include_str!("library.rs"),
        include_str!("library_catalog.rs"),
        include_str!("library_fake.rs"),
        include_str!("library_import.rs"),
        include_str!("library_install.rs"),
        include_str!("library_items.rs"),
        include_str!("library_list.rs"),
        include_str!("library_manage.rs"),
        include_str!("library_open.rs"),
        include_str!("library_setlist.rs"),
        include_str!("library_shell.rs"),
        include_str!("library_shots.rs"),
        include_str!("real_engine/library.rs"),
    ];
    let mut seen = 0;
    for source in sources {
        for key in ["\"name\": \"", "\"publisher\": \"", "\"keyName\": \"", "\"summary\": \"", "\"description\": \""] {
            for part in source.split(key).skip(1) {
                let value = part.split('"').next().unwrap_or("");
                seen += 1;
                assert!(INVENTED.contains(&value), "{key}{value}\" is not in the invented list (tests/library.rs INVENTED)");
            }
        }
        for part in source.split("stored(\"").skip(1) {
            // stored("performance", "p1", "Evening Strings"): the third string.
            let fields: Vec<&str> = part.splitn(7, '"').collect();
            if fields.len() >= 6 && fields[1] == ", " && fields[3] == ", " {
                seen += 1;
                assert!(INVENTED.contains(&fields[4]), "stored(.., \"{}\") is not in the invented list", fields[4]);
            }
        }
    }
    assert!(seen > 20, "the fixtures were read: {seen}");
}
