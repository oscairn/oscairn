// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's layout (the owner's, 2026-11-17): under the top bar one row of the kinds as tabs and the search box, under
//! it the sidebar (Favorites, Recent, libraries, tags) at the left and the list at the right. Each is drawn at 1280x720
//! (the keyboard's, at 1.55) and checked: the sidebar's choices by the panel's moves alone, search, each kind, nothing at rest.

use super::library::{footer, rows};
use super::library_bubbles::tagged;
use super::library_fake::stored;
use super::library_shell::Shelf;
use super::owner::{lib_png, scene};
use super::*;
use crate::{KeyboardState, LibraryState, PartState};

/// The line the sidebar's cursor is on.
pub(super) fn side_cursor(ui: &Shell) -> Option<String> {
    ui.global::<LibraryState>().get_side_rows().iter().find(|r| r.cursor).map(|r| r.label.to_string())
}

fn side_labels(ui: &Shell) -> Vec<String> {
    ui.global::<LibraryState>().get_side_rows().iter().map(|r| r.label.to_string()).collect()
}

/// The window at `w` x `h` (logical) and `scale`, showing the Library's Performances, normal or compact.
fn band_rig(name: &str, w: usize, h: usize, scale: f32, compact: bool) -> Shelf {
    fit_to_window();
    let mut s = Shelf::new(name);
    shell::library_in_rust(true);
    s.f.screen.resize(w, h);
    s.f.screen.set_scale(scale);
    theme::apply(&s.ui(), &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
    s.e.items = (0..10).map(|i| stored("performance", &format!("p{i}"), &format!("Bright Strings {:03}", i + 2))).collect();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    // Compact is kept between Libraries, so it may already be on: press the Compact button (as the owner does) only to change it.
    if s.ui().global::<PartState>().get_compact() != compact {
        super::library_bubbles::press_tool(&mut s, 1);
    }
    assert_eq!(s.ui().global::<PartState>().get_compact(), compact, "the list is {}", if compact { "Compact" } else { "Normal" });
    s
}

// The empty band under the Library's main list: the line that counts the rows is pinned to the window's bottom, so the
// spare room the rows' fit leaves sits empty between the last row and it (at 1280x720 and at 800x480). The count line now
// sits directly under the last row (one theme gap below it), so the band is no taller than a row gap, in Normal and Compact
// alike. The last row's bottom and the count line's top are read where the layout draws them (rows-per-view at the theme's
// touch and gap from the list's top, and the count line's own box), at the two windows the device and a small screen show.
#[test]
fn no_dead_band_between_the_last_row_and_the_count_line() {
    use crate::Theme;
    for (w, h, scale, mode) in [(1280, 720, 1.55, "the keyboard"), (800, 480, 1.0, "a small window")] {
        for compact in [false, true] {
            let name = format!("f1-band-{w}x{h}-{}", if compact { "compact" } else { "normal" });
            let s = band_rig(&name, w, h, scale, compact);
            let ui = s.ui();
            let t = ui.global::<Theme>();
            let per = ui.global::<crate::PartState>().get_rows_per_view() as f32;
            let (m, touch) = (t.get_margin(), t.get_touch());
            let gap = t.invoke_dense(t.get_list_gap(), compact); // the rows' gap, closed up in Compact
            // The last visible row's bottom and the count line's top, in the window's logical pixels.
            let last_bottom = ui.get_list_y() + m + (per - 1.0) * (touch + gap) + touch;
            let count_top = ui.get_list_y() + ui.get_count_y();
            let band = count_top - last_bottom;
            eprintln!("F1 {mode} {} {per} rows: the count line is {band} px below the last row (the gap is {gap})", if compact { "Compact" } else { "Normal" });
            assert!(band <= gap + 1.0, "{mode}, {}: {per} rows then a band of {band} px to the count line, taller than the {gap} px row gap", if compact { "Compact" } else { "Normal" });
            lib_png(&s, &name);
        }
    }
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
    shell::act(Action::Move { pane: None, by: ui.global::<LibraryState>().get_pages().row_count() as i32 });
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
    shell::act(Action::Move { pane: None, by: ui.global::<LibraryState>().get_pages().row_count() as i32 });
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
    for (page, name) in [(0, "performances"), (1, "sounds")] {
        ui.global::<LibraryState>().invoke_show_page(page);
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        let labels = side_labels(&ui);
        assert_eq!(labels[..4], ["Favorites", "Recent", "Libraries", "All"], "{name}");
        assert!(ui.global::<LibraryState>().get_side_total() > 7, "{name}: libraries and tags after the first view: {}", ui.global::<LibraryState>().get_side_total());
        assert_eq!(ui.global::<LibraryState>().get_pages().row_count(), 2);
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

// The whole Library screen, walked by the panel alone (the dial, the arrows and Confirm): the tabs' three kinds, the
// search box typed through the text keyboard and cleared by the cross, the row's three buttons (Bubbles, Compact and
// the Details info toggle), the sidebar (a switch, a library, a tag, a heading folded and unfolded), a row opened and
// a row starred. Every reached action is asserted by what it sent the fake engine or changed on the screen; the
// actions are counted from the screen's own models (the three pages, the rows, the sidebar's lines), and each is
// reached. Nothing the screen offers needs touch.
#[test]
fn the_whole_library_screen_by_panel_moves() {
    let mut s = tagged("walk");
    let ui = s.ui();
    let lib = ui.global::<LibraryState>();
    assert_eq!(lib.get_pages().row_count(), 2, "the kinds are two tabs");
    eprintln!("WALK: {} rows a view, {} sidebar lines in all", ui.global::<PartState>().get_rows_per_view(), lib.get_side_total());
    // The first move shows the cursor on the first row of the list.
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(rows(&ui).iter().any(|r| r.cursor), "the first move shows the list cursor");
    // Up out of the first row: the tabs. Walk each kind, each its own list asked again.
    shell::act(Action::Move { pane: None, by: -1 });
    assert_eq!(lib.get_page_cursor(), 0, "up out of the first row: the Performances tab");
    for (page, kind) in [(1i32, "sound"), (0, "performance")] {
        // The cursor stays on the tabs after a press: step along them.
        for _ in 0..2 {
            if lib.get_page_cursor() == page {
                break;
            }
            shell::act(Action::Move { pane: None, by: if lib.get_page_cursor() < page { 1 } else { -1 } });
        }
        assert_eq!(lib.get_page_cursor(), page, "the {kind} tab is a stop of the row");
        s.e.calls.clear();
        shell::act(Action::Confirm(None));
        s.serve();
        assert_eq!(lib.get_page(), page, "a press on the {kind} tab shows it");
        assert_eq!(s.e.called("library.query").last().unwrap()["kind"], kind, "the {kind} list is asked");
    }
    // On along the row under the bar to the search box: a press asks for the text on the keyboard, what is typed
    // narrows the list, and the cross (the stop after the box, once there is text) takes it away.
    shell::set_text_on_screen(false);
    shell::act(Action::Move { pane: None, by: lib.get_pages().row_count() as i32 });
    assert!(lib.get_search_cursor(), "the search box is the next stop along the row");
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    assert!(ui.global::<KeyboardState>().get_open(), "a press on the box opens the text keyboard");
    for ch in "warm".chars() {
        assert!(shell::key(&ch.to_string()));
    }
    assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Return)));
    s.serve();
    assert_eq!(lib.get_search().as_str(), "warm", "the typed text is in the box");
    assert_eq!(s.e.called("library.query").last().unwrap()["text"], "warm", "the narrowed list is asked");
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(lib.get_clear_cursor(), "the cross is the next stop with text");
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(lib.get_search().as_str(), "", "the cross clears the box");
    // The row's three buttons, each pressed and pressed again: Bubbles, Compact, and Details (the info toggle).
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(lib.get_view_cursor(), "Bubbles is the next stop");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(lib.get_bubbles(), "a press shows the sidebar as bubbles");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(!lib.get_bubbles(), "the same press makes them lines again");
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(lib.get_compact_cursor(), "Compact follows");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(lib.get_compact(), "a press closes the rows and the entries up");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(!lib.get_compact(), "and the same press opens them out");
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(lib.get_info_cursor(), "Details is the last stop of the row");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(lib.get_info_on(), "a press shows the panel, the info toggle");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(!lib.get_info_on(), "and the same press hides it");
    // The sidebar: down from the row to the list, left to the sidebar, then down its lines with the dial.
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    shell::act(Action::Arrow { dx: 1, dy: 0 });
    shell::act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("Favorites"), "left from the list enters the sidebar");
    // A switch, Favorites: a press asks with favorites, the same press again without.
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["favorites"], true, "a press on the Favorites switch");
    dial_to(&mut s, "Favorites");
    shell::act(Action::Confirm(None));
    s.serve();
    assert_ne!(s.e.called("library.query").last().unwrap().get("favorites").and_then(serde_json::Value::as_bool), Some(true), "the same press turns the switch off");
    // A switch, Recent: the dial down to it, a press asks with recent, the same press again without.
    dial_to(&mut s, "Recent");
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["recent"], true, "a press on the Recent switch");
    dial_to(&mut s, "Recent");
    shell::act(Action::Confirm(None));
    s.serve();
    assert_ne!(s.e.called("library.query").last().unwrap().get("recent").and_then(serde_json::Value::as_bool), Some(true), "the same press turns the switch off");
    // A heading folded and unfolded: a press on the Libraries heading sends nothing and hides its entries, the same again
    // brings them back.
    dial_to(&mut s, "Libraries");
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(s.e.calls.is_empty(), "a fold is not a question for the engine: {:?}", s.e.methods());
    assert!(lib.get_side_rows().iter().any(|r| r.value == "closed" && r.label == "Libraries"), "the heading says it is closed");
    assert!(side_labels(&ui).iter().all(|l| l != "All" && l != "My own"), "the libraries are folded away");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(side_labels(&ui).iter().any(|l| l == "All" || l == "My own"), "the same press unfolds them");
    // A library: the dial down to My own, a press asks with its source.
    dial_to(&mut s, "My own");
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["source"], "mine", "a press on an installed library");
    // A tag: the dial down past the Tags heading to the first tag, a press asks with it.
    dial_to(&mut s, "keys");
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["tags"], json!(["keys"]), "a press on a tag narrows to it");
    // Right: back to the list. A row opened: Confirm on the first row runs the open chain to a load.
    shell::act(Action::Arrow { dx: 1, dy: 0 });
    assert!(rows(&ui).iter().any(|r| r.cursor), "right from the sidebar is the list");
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    let loaded = s.e.called("library.load");
    let id = loaded.last().unwrap()["id"].as_str().unwrap_or_default().to_string();
    assert_eq!(footer(&ui).0, "Performance opened. Press Play or play a note.", "the row opened");
    assert!(id.starts_with('p'), "the opening named the row's own item: {id}");
    // A row starred: the bar folds its actions into More at this window, so Back then More then Options, the dial to
    // Add to favorites, a press. The star is for the row under the cursor.
    shell::act(Action::Move { pane: Some(2), by: 1 }); // the third pane enters the bar at Back
    assert!(ui.global::<crate::Bar>().get_back_cursor());
    shell::act(Action::Move { pane: None, by: 1 }); // More, the bar's one action stop folded
    assert!(ui.global::<crate::Bar>().get_more_cursor(), "the bar folded its actions into More at this window");
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(ui.global::<crate::MenuState>().get_open(), "More opened the actions' menu");
    for _ in 0..8 {
        if ui.global::<crate::MenuState>().get_rows().iter().any(|r| r.cursor && r.label == "Options") {
            break;
        }
        shell::act(Action::Move { pane: None, by: 1 });
    }
    assert!(ui.global::<crate::MenuState>().get_rows().iter().any(|r| r.cursor && r.label == "Options"), "the dial chose Options");
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(ui.global::<crate::MenuState>().get_open(), "Options opened the item's menu");
    for _ in 0..6 {
        if ui.global::<crate::MenuState>().get_rows().iter().any(|r| r.cursor && r.label == "Add to favorites") {
            break;
        }
        shell::act(Action::Move { pane: None, by: 1 });
    }
    assert!(ui.global::<crate::MenuState>().get_rows().iter().any(|r| r.cursor && r.label == "Add to favorites"), "the dial reached Add to favorites");
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.favorite"), [json!({"id": id, "favorite": true})], "the star is sent for the row under the cursor");
    assert!(!ui.global::<crate::MenuState>().get_open());
    // Nothing at rest after the walk: the screen asks nothing and draws nothing.
    s.e.calls.clear();
    s.f.screen.frame();
    assert!(s.e.calls.is_empty());
}
