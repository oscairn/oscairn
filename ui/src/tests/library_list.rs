// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's list: search, the filters, the star, the empty states, the release's setup line and the panel
//! (ui/LIBRARY-APP.md rows A4 to A14), against the fake engine.

use super::library::*;
use super::library_shell::Shelf;
use super::*;
use crate::library::Outcome;
use crate::tests::library_fake::stored;
use crate::{LibraryState, PartState, Theme};

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

/// A point on the window that lies on row `row` of the list: the Library lays the shared list at the
/// area the tabs and the sidebar leave (`content-x`, `content-y`), the Part's own list at its margin.
fn on_row(s: &Screen, row: usize, inset: f32) -> (f32, f32) {
    let th = s.ui.global::<Theme>();
    let (margin, gap, touch, bar) = (th.get_margin(), th.get_gap(), th.get_touch(), th.get_bar_height());
    let cx = 2.0 * margin + (W as f32 - 3.0 * margin) * th.get_sidebar_share();
    let cy = bar + margin + touch + gap; // the screen sits under the top bar
    let pitch = touch + th.get_list_gap();
    (cx + margin + 40.0, cy + margin + row as f32 * pitch + inset)
}

fn press(s: &Screen, p: (f32, f32)) {
    s.ui.window().dispatch_event(slint::platform::WindowEvent::PointerPressed { position: slint::LogicalPosition::new(p.0, p.1), button: slint::platform::PointerEventButton::Left });
    s.ui.window().dispatch_event(slint::platform::WindowEvent::PointerMoved { position: slint::LogicalPosition::new(p.0, p.1) });
}
fn move_to(s: &Screen, p: (f32, f32)) {
    s.ui.window().dispatch_event(slint::platform::WindowEvent::PointerMoved { position: slint::LogicalPosition::new(p.0, p.1) });
}
fn release(s: &Screen, p: (f32, f32)) {
    s.ui.window().dispatch_event(slint::platform::WindowEvent::PointerReleased { position: slint::LogicalPosition::new(p.0, p.1), button: slint::platform::PointerEventButton::Left });
}

// A finger drag on the rows scrolls the list (it follows the finger row for row) and is not a tap: a
// press on a row, a drag three rows up, a release — the list moved three rows, no row was opened.
#[test]
fn a_drag_on_the_rows_scrolls_the_list_and_is_not_a_tap() {
    let mut s = Shelf::new("library-drag");
    s.e.items.extend((0..40).map(|i| stored("performance", &format!("p{i}"), &format!("Warm Bass {i}"))));
    let ui = s.ui();
    crate::shell::goto(crate::shell::Screen::Library);
    s.serve();
    s.f.screen.frame(); // the rows are placed in the window
    let (x, y) = on_row(&s.f.screen, 4, 22.0);
    assert_eq!(ui.global::<PartState>().get_first(), 0, "starts on the first view");
    let info_before = s.e.called("performance.info").len();
    press(&s.f.screen, (x, y));
    // Up by three rows (the touch + gap pitch each), in small steps so the drag crosses the theme's
    // threshold early and then scrolls row for row.
    for i in 1..=12 {
        move_to(&s.f.screen, (x, y - i as f32 * 12.0));
    }
    release(&s.f.screen, (x, y - 144.0));
    s.serve();
    assert_eq!(ui.global::<PartState>().get_first(), 3, "the list followed the drag up three rows");
    assert!(rows(&ui).iter().all(|rr| !rr.cursor), "no row was opened by the drag");
    assert_eq!(s.e.called("performance.info").len(), info_before, "the drag opened no row: {:?}", s.e.methods());
}

// A press and a release with no movement is still a tap: the view does not move, the row is opened.
#[test]
fn a_press_and_release_without_movement_taps_the_row() {
    let mut s = Shelf::new("library-tap");
    s.e.items.extend((0..40).map(|i| stored("performance", &format!("p{i}"), &format!("Warm Bass {i}"))));
    let ui = s.ui();
    crate::shell::goto(crate::shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    let (x, y) = on_row(&s.f.screen, 0, 22.0);
    let info_before = s.e.called("performance.info").len();
    press(&s.f.screen, (x, y));
    release(&s.f.screen, (x, y));
    s.serve();
    assert_eq!(ui.global::<PartState>().get_first(), 0, "a tap does not scroll");
    assert!(s.e.called("performance.info").len() > info_before, "the row's tap opened it: {:?}", s.e.methods());
}

// The right knob under the screen (pane 1) moves the list cursor and keeps it in view, like the jog wheel
// (the dial, pane None): turning it past the last visible row scrolls the list so the cursor's row is always
// among the rows shown. Before the fix, pane 1 scrolled the read-only panel and left the list cursor on no row
// at all; the view never followed.
#[test]
fn the_right_knob_keeps_the_cursor_in_view_in_every_list() {
    let mut r = Rig::new((0..40).map(|i| stored("performance", &format!("p{i}"), &format!("Warm Bass {i}"))).collect());
    r.ui().global::<crate::PartState>().set_rows_per_view(5);
    let per = r.ui().global::<crate::PartState>().get_rows_per_view().max(1) as usize;
    assert_eq!(per, 5, "a short view: five rows");
    // The first turn shows the cursor on the first row (as the dial's first move shows it).
    r.act(Action::Move { pane: Some(1), by: 1 });
    assert_eq!(rows(r.ui()).iter().position(|x| x.cursor), Some(0), "the first turn shows the cursor");
    // To the last row and back: the list scrolls by whole views, the cursor's row always shown.
    for _ in 0..(40 + 5) {
        r.act(Action::Move { pane: Some(1), by: 1 });
        assert!(rows(r.ui()).iter().any(|x| x.cursor), "the right knob keeps the cursor in the view");
    }
    let first = r.ui().global::<crate::PartState>().get_first() as usize;
    assert_eq!(first + per, 40, "at the end the list shows the last view");
    for _ in 0..(40 + 5) {
        r.act(Action::Move { pane: Some(1), by: -1 });
        assert!(rows(r.ui()).iter().any(|x| x.cursor), "turned back the cursor stays in the view");
    }
}
