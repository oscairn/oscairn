// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's list: search, the filters, the star, the empty states, the release's setup line and the panel
//! (ui/LIBRARY-APP.md rows A4 to A14), against the fake engine.

use super::library::*;
use super::*;
use crate::library::Outcome;

// Search is the top bar's action: it asks for text on the keyboard, and what is typed narrows the list by the engine's
// own match (name, description, tags); the Filters row says what is on.
#[test]
fn search_narrows_by_what_is_typed() {
    let mut r = Rig::new(library());
    assert_eq!(r.c.bar_tools()[0].label, "Search");
    r.e.calls.clear();
    assert_eq!(r.tool("Search"), Outcome::Prompt("Search the Performances".into(), String::new()));
    r.c.entered("  warm ");
    r.settle();
    assert_eq!(r.e.called("library.query"), [json!({"kind": "performance", "limit": 1000, "text": "warm"})]);
    assert_eq!(labels(r.ui()), ["Filters", "Brass Corner", "Evening Strings"]);
    assert_eq!(value_of(r.ui(), "Filters"), "“warm” ›");
    assert_eq!(r.c.bar_tools()[0].label, "Search: warm");
    assert_eq!(r.tool("Search"), Outcome::Prompt("Search the Performances".into(), "warm".into()), "the text is there to change");
    r.c.entered("");
    r.settle();
    assert_eq!(labels(r.ui()).len(), 5);
    // Canceled: nothing changes.
    r.tool("Search");
    r.e.calls.clear();
    r.c.not_entered();
    r.settle();
    assert!(r.e.calls.is_empty());
}

// The filters are the sound chooser's: Favorites and Recent switches that ask again, the library everything, my own
// or an installed one (with its count), the tags with their counts (the chosen first), all chosen must match, and a
// library gone while chosen is let go.
#[test]
fn the_filters_make_the_query() {
    let mut r = Rig::new(library());
    r.e.calls.clear();
    r.press_row("Filters");
    assert_eq!(labels(r.ui()), ["Favorites", "Recent", "Library", "Tags"]);
    assert_eq!(r.c.path(), ["Library", "Performances", "Filters"]);
    r.press_row("Favorites");
    assert_eq!(r.e.called("library.query"), [json!({"kind": "performance", "limit": 1000, "favorites": true})]);
    assert_eq!(value_of(r.ui(), "Favorites"), "On");
    r.press_row("Favorites");
    r.press_row("Recent");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "recent": true}));
    r.press_row("Recent");
    // The library.
    r.press_row("Library");
    assert_eq!(labels(r.ui()), ["Everything", "My own", "A Friend's"]);
    assert_eq!(rows(r.ui()).iter().map(|x| x.value.to_string()).collect::<Vec<_>>(), ["Chosen", "", "2"], "an installed library's items counted");
    r.press_row("A Friend's");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "source": "friend"}));
    assert_eq!(value_of(r.ui(), "Library"), "A Friend's ›");
    // Tags: counted over the matches, the chosen first.
    r.press_row("Library");
    r.press_row("Everything");
    r.press_row("Tags");
    assert_eq!(labels(r.ui()), ["warm", "brass", "strings", "choir", "pad"]);
    assert_eq!(value_of(r.ui(), "warm"), "2");
    r.press_row("strings");
    r.press_row("warm");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000, "tags": ["strings", "warm"]}));
    assert_eq!(labels(r.ui())[..2], ["strings", "warm"]);
    assert_eq!(value_of(r.ui(), "warm"), "✓ 1");
    r.c.back();
    assert_eq!(value_of(r.ui(), "Tags"), "strings, warm ›");
    r.c.back();
    assert_eq!(labels(r.ui()), ["Filters", "Evening Strings"]);
    assert_eq!(value_of(r.ui(), "Filters"), "strings, warm ›");
    assert_eq!(r.c.path(), ["Library", "Performances"]);
}

// A library uninstalled while it was chosen: everything shows (LibraryPane.cpp timerCallback).
#[test]
fn a_library_gone_while_chosen_is_let_go() {
    let mut r = Rig::new(library());
    r.press_row("Filters");
    r.press_row("Library");
    r.press_row("A Friend's");
    assert_eq!(value_of(r.ui(), "Library"), "A Friend's ›");
    r.e.libraries.clear();
    r.e.calls.clear();
    r.press_row("Library"); // asks which libraries are installed
    assert_eq!(r.e.methods()[0], "library.libraries");
    assert_eq!(r.e.called("library.query").last().unwrap(), &json!({"kind": "performance", "limit": 1000}));
    assert_eq!(labels(r.ui()), ["Everything", "My own"]);
}

// The star toggles from the item's menu, which Options on the top bar opens for the item under the cursor (the touch
// route to it too), the list asked again so it shows.
#[test]
fn a_favorite_is_toggled_from_the_menu() {
    let mut r = Rig::new(library());
    r.e.calls.clear();
    r.act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor on Filters
    r.act(Action::Move { pane: None, by: 2 }); // Brass Corner
    assert_eq!(rows(r.ui()).iter().position(|x| x.cursor), Some(2));
    r.tool("Options");
    let (title, items) = r.c.take_menu().expect("the item's menu");
    assert_eq!(title, "Brass Corner");
    assert_eq!(items.iter().map(|i| (i.label.as_str(), i.enabled)).collect::<Vec<_>>()[..4], [("Open", true), ("Add to setlist", true), ("Audition", true), ("Add to favorites", true)]);
    r.c.chosen(4);
    r.settle();
    assert_eq!(r.e.called("library.favorite"), [json!({"id": "p4", "favorite": true})]);
    assert!(value_of(r.ui(), "Brass Corner").ends_with('★'));
    assert_eq!(r.e.methods(), ["library.favorite", "library.query"], "asked again to show it");
    assert!(r.e.called("library.query")[0]["kind"] == "performance");
}

// Nothing of the kind and nothing narrowing it says so, with what is installed; a search with no match says so too.
#[test]
fn an_empty_library_says_what_is_installed() {
    let r = Rig::new(vec![]);
    assert_eq!(labels(r.ui()), ["Filters"]);
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
    r.tool("Search");
    r.c.entered("zzz");
    r.settle();
    assert_eq!(labels(r.ui()), ["Filters"]);
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
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: 1 }); // Untitled
    let lines = info(r.ui());
    assert_eq!(lines[..3], ["Performance", "Untitled", "Locked: A Friend's"]);
    r.act(Action::Move { pane: None, by: 3 }); // Glass Choir
    let lines = info(r.ui());
    assert_eq!(lines[..2], ["Performance", "Glass Choir"]);
    assert!(lines.contains(&"choir, pad".to_string()) && lines.contains(&"My own".to_string()));
    assert!(lines.contains(&"Does not fit this device".to_string()) && lines.contains(&"needs 3 GB of memory".to_string()), "{lines:?}");
}
