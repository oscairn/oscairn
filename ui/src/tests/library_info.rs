// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's panel (what the item under the cursor is): hidden until the button at the right of the row under the top bar
//! shows it in the sidebar's place, and what only it said stays reachable with it hidden (the owner's, 2026-11-17).

use super::library::{footer, info, library, rows, Rig};
use super::library_shell::Shelf;
use super::owner::{lib_png, scene};
use super::*;
use crate::LibraryState;

/// The dial's moves along the row under the top bar: up from the first row, then to its last stop (the panel's button).
fn to_the_panel_button(ui: &Shell) {
    shell::act(Action::Move { pane: None, by: -1 });
    assert_eq!(ui.global::<LibraryState>().get_page_cursor(), 0, "the first tab");
    // Three tabs, the box, the button.
    shell::act(Action::Move { pane: None, by: 4 });
    assert!(ui.global::<LibraryState>().get_info_cursor(), "the panel's button has the cursor");
}

// The panel is hidden at first and the sidebar is there; the button (a stop of the row under the top bar, so by the panel's
// moves alone) shows it in the sidebar's place, with the cursor in the list as it was; the same button hides it again.
#[test]
fn the_button_shows_the_panel_in_the_sidebars_place() {
    let mut s = scene("info-nav", 10);
    let ui = s.ui();
    let lib = ui.global::<LibraryState>();
    assert!(!lib.get_info_on() && lib.get_side_total() > 0, "hidden, the sidebar at the left");
    shell::act(Action::Move { pane: None, by: 1 }); // the cursor on the first row
    to_the_panel_button(&ui);
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(lib.get_info_on());
    assert_eq!(lib.get_side_total(), 0, "the sidebar gives its place");
    let lines = info(&ui);
    assert_eq!(lines[..2], ["Performance", "A Very Long Performance Name For Testing 1"], "{lines:?}");
    // Left goes nowhere now: the list is the one place under the row; down reaches it.
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    assert!(rows(&ui).iter().any(|r| r.cursor));
    shell::act(Action::Arrow { dx: -1, dy: 0 });
    assert!(rows(&ui).iter().any(|r| r.cursor), "no sidebar to go to");
    s.f.screen.frame();
    s.f.screen.frame();
    lib_png(&s, "lib-C-info");
    // Hidden again: the sidebar is back.
    shell::act(Action::Move { pane: None, by: -1 });
    shell::act(Action::Move { pane: None, by: 4 });
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(!lib.get_info_on() && lib.get_side_total() > 0);
    // And by touch.
    lib.invoke_info_toggled();
    assert!(lib.get_info_on());
    lib.invoke_info_toggled();
    assert!(!lib.get_info_on());
}

// What only the panel said is in the line under the list while the cursor is on the row and the panel is hidden: why the
// Performance does not fit this device, which library it is locked in; with the panel shown the panel says it.
#[test]
fn what_only_the_panel_said_is_in_the_line_under_the_list() {
    let mut r = Rig::new(library());
    r.act(Action::Move { pane: None, by: 1 }); // Untitled: locked in a library
    assert_eq!(footer(r.ui()).1, "Performances 1–4 of 4   ·   Locked: A Friend's");
    r.act(Action::Move { pane: None, by: 3 }); // Glass Choir: does not fit
    assert_eq!(footer(r.ui()).1, "Performances 1–4 of 4   ·   Does not fit this device: needs 3 GB of memory");
    r.act(Action::Move { pane: None, by: -1 }); // Evening Strings: nothing to add
    assert_eq!(footer(r.ui()).1, "Performances 1–4 of 4");
    r.c.toggle_info();
    r.act(Action::Move { pane: None, by: 1 });
    assert_eq!(footer(r.ui()).1, "Performances 1–4 of 4", "the panel is shown: it says it");
    assert!(info(r.ui()).contains(&"needs 3 GB of memory".to_string()));
}

// Opening the screen with the panel hidden asks nothing more of the engine, and showing it asks nothing either.
#[test]
fn showing_the_panel_asks_the_engine_nothing() {
    let mut s = Shelf::new("info-idle");
    shell::goto(shell::Screen::Library);
    s.serve();
    s.e.calls.clear();
    let ui = s.ui();
    let lib = ui.global::<LibraryState>();
    lib.invoke_info_toggled();
    s.serve();
    lib.invoke_info_toggled();
    s.serve();
    assert!(s.e.calls.is_empty(), "{:?}", s.e.methods());
}
