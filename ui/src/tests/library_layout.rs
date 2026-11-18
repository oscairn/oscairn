// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's layout (the owner's, 2026-11-17): under the top bar one row of the kinds as tabs and the search box, under
//! it the sidebar (Favorites, Recent, libraries, tags) at the left and the list at the right. Each is drawn at 1280x720
//! (the keyboard's, at 1.55) and checked: the sidebar's choices by the panel's moves alone, search, each kind, nothing at rest.

use super::library::{footer, rows};
use super::library_shell::Shelf;
use super::owner::{lib_png, scene};
use super::*;
use crate::{KeyboardState, LibraryState};

/// The line the sidebar's cursor is on.
pub(super) fn side_cursor(ui: &Shell) -> Option<String> {
    ui.global::<LibraryState>().get_side_rows().iter().find(|r| r.cursor).map(|r| r.label.to_string())
}

fn side_labels(ui: &Shell) -> Vec<String> {
    ui.global::<LibraryState>().get_side_rows().iter().map(|r| r.label.to_string()).collect()
}

/// The dial down the sidebar to the line `label`.
fn dial_to(s: &mut Shelf, label: &str) {
    for _ in 0..40 {
        if side_cursor(&s.ui()).as_deref() == Some(label) {
            return;
        }
        shell::act(Action::Move { pane: None, by: 1 });
    }
    panic!("{label} not reached: {:?} of {} lines", side_cursor(&s.ui()), s.ui().global::<LibraryState>().get_side_total());
}

// Without touch: the left arrow goes from the list to the sidebar, the dial walks its choices (the headings are stops: a press folds),
// a press chooses (a library, then a tag: each asks the engine at once), the right arrow goes back to the list, up from its
// first row to the tabs, along them to the search box; the one cursor is in one place at a time.
#[test]
fn the_sidebar_is_reached_and_chosen_by_the_panel_alone() {
    let mut s = scene("layout-nav", 10);
    let ui = s.ui();
    s.e.calls.clear();
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(rows(&ui).iter().any(|r| r.cursor), "the list has the cursor");
    shell::act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("Favorites"));
    assert!(rows(&ui).iter().all(|r| !r.cursor), "one cursor: the list gave it up");
    shell::act(Action::Move { pane: None, by: 4 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("My own"), "the heading is a stop: Favorites, Recent, Libraries, All, My own");
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["source"], "mine", "choosing applies at once");
    assert!(s.ui().global::<LibraryState>().get_side_rows().iter().any(|r| r.on && r.label == "My own"), "ticked");
    // A tag.
    dial_to(&mut s, "keys");
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["tags"], json!(["keys"]));
    assert_eq!(s.e.called("library.query").last().unwrap()["source"], "mine", "the library and the tag narrow together");
    s.f.screen.frame();
    s.f.screen.frame();
    lib_png(&s, "lib-B-tag-selected");
    // Back to the list: the cursor is on its first row.
    shell::act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(side_cursor(&ui), None);
    assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(0));
    // Up from the first row: the tabs, then along to the search box, then down to the list again.
    shell::act(Action::Move { pane: None, by: -1 });
    assert_eq!(ui.global::<LibraryState>().get_page_cursor(), 0, "the Performances tab");
    assert!(rows(&ui).iter().all(|r| !r.cursor));
    shell::act(Action::Move { pane: None, by: 3 });
    assert!(ui.global::<LibraryState>().get_search_cursor(), "the search box");
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    assert!(!ui.global::<LibraryState>().get_search_cursor());
    assert!(rows(&ui).iter().any(|r| r.cursor), "down from the row under the bar is the list");
}

// The search box: the dial reaches it, a press asks for the text on the keyboard, the text shows in the box and narrows the
// list and the line under it, and the cross (the stop after the box, only with text) takes it away.
#[test]
fn search_is_reached_typed_and_cleared() {
    let mut s = scene("layout-search", 10);
    let ui = s.ui();
    shell::set_text_on_screen(false);
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Move { pane: None, by: -1 });
    shell::act(Action::Move { pane: None, by: 3 });
    assert!(ui.global::<LibraryState>().get_search_cursor());
    shell::act(Action::Confirm(None));
    assert!(ui.global::<KeyboardState>().get_open());
    assert_eq!(ui.global::<KeyboardState>().get_title().as_str(), "Search the Performances");
    for ch in "bright".chars() {
        assert!(shell::key(&ch.to_string()));
    }
    assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Return)));
    s.serve();
    assert_eq!(ui.global::<LibraryState>().get_search().as_str(), "bright");
    assert_eq!(s.e.called("library.query").last().unwrap()["text"], "bright");
    assert_eq!(footer(&ui).1, "Performances 1–6 of 9", "the one without it in its name is gone");
    s.f.screen.frame();
    s.f.screen.frame();
    lib_png(&s, "lib-B-search");
    // The cross is the next stop, now that there is text.
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(ui.global::<LibraryState>().get_clear_cursor());
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(ui.global::<LibraryState>().get_search().as_str(), "");
    assert_eq!(footer(&ui).1, "Performances 1–6 of 10");
    assert!(ui.global::<LibraryState>().get_search_cursor(), "the cursor stays by the box");
}

// Each kind has the sidebar: the same sections (Favorites, Recent, the libraries, the tags of what it lists), and the
// tabs are the first row under the top bar. Pictures of each kind in the dark theme.
#[test]
fn each_kind_has_the_sidebar() {
    let mut s = scene("layout-kinds", 10);
    let ui = s.ui();
    s.e.items.extend((0..8).map(|i| super::library_fake::stored("sound", &format!("s{i}"), &format!("Soft Felt {i}")).tags(&["keys", ["warm", "bright"][i % 2]])));
    s.e.items.extend((0..3).map(|i| super::library_fake::stored("setlist", &format!("l{i}"), &format!("Friday Set {i}")).tags(&["live"])));
    for (page, name) in [(0, "performances"), (1, "sounds"), (2, "setlists")] {
        ui.global::<LibraryState>().invoke_show_page(page);
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        let labels = side_labels(&ui);
        assert_eq!(labels[..4], ["Favorites", "Recent", "Libraries", "All"], "{name}");
        assert!(ui.global::<LibraryState>().get_side_total() > 7, "{name}: libraries and tags after the first view: {}", ui.global::<LibraryState>().get_side_total());
        assert_eq!(ui.global::<LibraryState>().get_pages().row_count(), 3);
        lib_png(&s, &format!("lib-B-{name}"));
    }
}

// The 30-character name is drawn whole with its two tags at 1280x720: the name comes before the tags and the description.
#[test]
fn a_thirty_character_name_shows_whole_beside_the_sidebar() {
    assert!(super::owner::last_character_shows_for("layout-30", "A Bright String With Length 30"), "the name's last character is cut off");
}

// The layout at the tight spacing (0.7) and in the light theme: the row under the bar, the sidebar and the list as in the
// dark one, the sidebar holding at least a view of lines (pictures: lib-B-spacing-0.7, lib-B-light).
#[test]
fn the_layout_at_tight_spacing_and_in_the_light_theme() {
    for (name, spacing, bright) in [("lib-B-spacing-0.7", 0.7f32, false), ("lib-B-light", 1.0, true)] {
        let mut s = scene("layout-themes", 10);
        let ui = s.ui();
        if bright {
            theme::apply(&ui, &theme::chosen(&theme::Places::default(), "oscairn-light", ""));
        }
        ui.global::<crate::Theme>().set_spacing(spacing);
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        let lib = ui.global::<LibraryState>();
        assert!(lib.get_side_per_view() >= 6, "{name}: the sidebar holds {} lines", lib.get_side_per_view());
        lib_png(&s, name);
    }
}
