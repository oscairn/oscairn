// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's list: search, the filters, the star, the empty states, the release's setup line and the panel
//! (ui/LIBRARY-APP.md rows A4 to A14), against the fake engine.

use super::library::*;
use super::*;
use crate::library::Outcome;
use crate::LibraryState;

// Search is the box under the top bar: a tap or a press asks for text on the keyboard, and what is typed narrows the list
// by the engine's own match (name, description, tags); the box shows the text and the line under the list counts the matches.
#[test]
fn search_narrows_by_what_is_typed() {
    let mut r = Rig::new(library());
    assert!(r.c.bar_tools().iter().all(|t| !t.label.starts_with("Search")), "the box searches, not a bar action");
    r.e.calls.clear();
    assert_eq!(r.c.ask_search(), Outcome::Prompt("Search the Performances".into(), String::new()));
    r.c.entered("  warm ");
    r.settle();
    assert_eq!(r.e.called("library.query"), [json!({"kind": "performance", "limit": 1000, "text": "warm"})]);
    assert_eq!(labels(r.ui()), ["Brass Corner", "Evening Strings"]);
    assert_eq!(r.ui().global::<LibraryState>().get_search(), "warm");
    assert_eq!(footer(r.ui()).1, "Performances 1–2 of 2");
    assert_eq!(r.c.ask_search(), Outcome::Prompt("Search the Performances".into(), "warm".into()), "the text is there to change");
    r.c.entered("");
    r.settle();
    assert_eq!(labels(r.ui()).len(), 4);
    // Canceled: nothing changes.
    r.c.ask_search();
    r.e.calls.clear();
    r.c.not_entered();
    r.settle();
    assert!(r.e.calls.is_empty());
    // The cross takes the text away and asks again.
    r.search("warm");
    r.e.calls.clear();
    r.c.clear_search();
    r.settle();
    assert_eq!(r.e.called("library.query"), [json!({"kind": "performance", "limit": 1000})]);
    assert_eq!(r.ui().global::<LibraryState>().get_search(), "");
    assert_eq!(labels(r.ui()).len(), 4);
}

// The sidebar's choices make the query: Favorites and Recent switches that ask again, the library (All, my own or an
// installed one, with its count), the tags with their counts (the chosen first), all chosen must match, and a library gone
// while chosen is let go.
#[test]
fn the_sidebar_makes_the_query() {
    let mut r = Rig::new(library());
    assert_eq!(r.side_labels(), ["Favorites", "Recent", "Libraries", "All", "My own", "A Friend's", "Tags", "warm", "brass", "strings", "choir", "pad"]);
    assert_eq!(r.side().iter().filter(|x| x.on).map(|x| x.label.to_string()).collect::<Vec<_>>(), ["All"], "the library chosen is ticked");
    r.e.calls.clear();
    r.pick("Favorites");
    assert_eq!(r.e.called("library.query"), [json!({"kind": "performance", "limit": 1000, "favorites": true})]);
    assert!(r.side()[0].on);
    r.pick("Favorites");
    r.pick("Recent");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "recent": true}));
    r.pick("Recent");
    // The library.
    assert_eq!(r.side().iter().map(|x| x.value.to_string()).collect::<Vec<_>>()[3..6], ["✓", "", "2"], "an installed library's items counted");
    r.pick("A Friend's");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "source": "friend"}));
    assert!(r.e.methods().contains(&"library.libraries".to_string()), "which libraries are installed is asked again");
    assert_eq!(r.side().iter().filter(|x| x.on).map(|x| x.label.to_string()).collect::<Vec<_>>(), ["A Friend's"]);
    r.pick("All");
    // Tags: counted over the matches, the chosen first.
    assert_eq!(r.side().iter().find(|x| x.label == "warm").unwrap().value, "2");
    r.pick("strings");
    r.pick("warm");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "tags": ["strings", "warm"]}));
    assert_eq!(r.side_labels()[7..9], ["strings", "warm"]);
    assert_eq!(r.side().iter().find(|x| x.label == "warm").unwrap().value, "✓ 1");
    assert_eq!(labels(r.ui()), ["Evening Strings"]);
    r.pick("warm");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "tags": ["strings"]}), "a tag let go");
}

// A library uninstalled while it was chosen: everything shows (LibraryPane.cpp timerCallback).
#[test]
fn a_library_gone_while_chosen_is_let_go() {
    let mut r = Rig::new(library());
    r.pick("A Friend's");
    assert!(r.side()[5].on);
    r.e.libraries.clear();
    r.e.calls.clear();
    r.c.open(); // which libraries are installed, asked again
    r.settle();
    assert_eq!(r.e.methods()[1], "library.libraries");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000}));
    assert_eq!(r.side_labels()[3..], ["All", "My own", "Tags", "warm", "brass", "strings", "choir", "pad"]);
}

// The star toggles from the item's menu, which Options on the top bar opens for the item under the cursor (the touch
// route to it too), the list asked again so it shows.
#[test]
fn a_favorite_is_toggled_from_the_menu() {
    let mut r = Rig::new(library());
    r.e.calls.clear();
    r.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor on the first row
    r.act(Action::Move { pane: None, by: 1 }); // Brass Corner
    assert_eq!(rows(r.ui()).iter().position(|x| x.cursor), Some(1));
    r.tool("Options");
    let (title, items) = r.c.take_menu().expect("the item's menu");
    assert_eq!(title, "Brass Corner");
    assert_eq!(items.iter().map(|i| (i.label.as_str(), i.enabled)).collect::<Vec<_>>()[..4], [("Open", true), ("Add to setlist", true), ("Audition", true), ("Add to favorites", true)]);
    r.c.chosen(4);
    r.settle();
    assert_eq!(r.e.called("library.favorite"), [json!({"id": "p4", "favorite": true})]);
    assert!(rows(r.ui()).iter().any(|x| x.label == "Brass Corner" && x.star));
    assert_eq!(r.e.methods(), ["library.favorite", "library.query"], "asked again to show it");
    assert!(r.e.called("library.query")[0]["kind"] == "performance");
}

// Nothing of the kind and nothing narrowing it says so, with what is installed; a search with no match says so too.
#[test]
fn an_empty_library_says_what_is_installed() {
    let r = Rig::new(vec![]);
    assert_eq!(labels(r.ui()), ["Browse the Catalog"], "an empty list has a way to the Catalog");
    assert!(info(r.ui()).join(" ").contains("0 of 0 Performances No Performances yet."), "{:?}", info(r.ui()));
    let mut r = Rig::new(vec![]);
    r.e.plugin_packages = 5;
    r.c.open();
    r.settle();
    assert!(info(r.ui()).join(" ").contains("No Performances yet. 5 instruments and effects are installed, to start one with."), "{:?}", info(r.ui()));
    r.c.show_page(1);
    r.settle();
    assert!(info(r.ui()).join(" ").contains("No Sounds yet. The presets of the 5 instruments and effects installed are in the sound picker, not here."), "{:?}", info(r.ui()));
    r.c.show_page(2);
    r.settle();
    assert!(info(r.ui()).join(" ").contains("No setlists yet. The Setlist app makes them."));
    let mut r = Rig::new(library());
    r.search("zzz");
    assert!(labels(r.ui()).is_empty());
    assert!(info(r.ui()).contains(&"Nothing matches".to_string()));
}

// While the packages that come with the release install, one line says so and the screen asks again each second, and
// only while it runs; when it ends the list is asked again.
#[test]
fn the_setup_of_the_release_is_said_while_it_runs() {
    let mut r = Rig::new(library());
    r.e.preinstall = "running".into();
    r.e.calls.clear();
    r.c.open();
    r.settle();
    assert_eq!(footer(r.ui()).0, "Setting up the instruments that come with Oscairn…");
    assert_eq!(r.delayed.last(), Some(&("packages.preinstalled".to_string(), std::time::Duration::from_secs(1))));
    r.e.preinstall = "done".into();
    r.e.calls.clear();
    r.later();
    r.settle();
    assert_eq!(r.e.methods(), ["packages.preinstalled", "library.query"]);
    assert_eq!(footer(r.ui()).0, "");
}

// The panel says what the item under the cursor is.
#[test]
fn the_panel_describes_the_item_under_the_cursor() {
    let mut r = Rig::new(library());
    r.act(Action::Move { pane: None, by: 1 }); // Untitled
    let lines = info(r.ui());
    assert_eq!(lines[..3], ["Performance", "Untitled", "Locked: A Friend's"]);
    r.act(Action::Move { pane: None, by: 3 }); // Glass Choir
    let lines = info(r.ui());
    assert_eq!(lines[..2], ["Performance", "Glass Choir"]);
    assert!(lines.contains(&"choir, pad".to_string()) && lines.contains(&"My own".to_string()));
    assert!(lines.contains(&"Does not fit this device".to_string()) && lines.contains(&"needs 3 GB of memory".to_string()), "{lines:?}");
}
