// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's sidebar as a list or as bubbles (the owner's, 2026-11-17; the JUCE pane's "Show as a list" and "Show as pills"):
//! the button in the row under the top bar, the layout, the cursor among bubbles, the choices as in the list, and what is kept
//! (library-view.txt). Pictures: polish-3-list, polish-3-bubbles (and -blue).

use super::library_fake::stored;
use super::library_layout::side_cursor;
use super::library_shell::Shelf;
use super::owner::{lib_png, scene};
use super::theme_pictures::{blue, orange};
use super::*;
use crate::LibraryState;

const TAGS: [&str; 14] = ["keys", "brass", "pad", "studio", "live", "warm", "bright", "strings", "lead", "bass", "choir", "organ", "noisy", "soft"];

/// The Library with the tags the owner's library has: ten Performances, three of the fourteen each.
pub(super) fn tagged(name: &str) -> Shelf {
    let mut s = scene(name, 10);
    s.e.items = (0..10).map(|i| stored("performance", &format!("p{i}"), &format!("Bright Strings {:03}", i + 2)).tags(&[TAGS[i], TAGS[(i + 4) % 14], TAGS[(i + 9) % 14]])).collect();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    s
}

/// How many of the tags the view shows.
pub(super) fn tags_in_view(ui: &Shell) -> usize {
    ui.global::<LibraryState>().get_side_rows().iter().filter(|r| TAGS.contains(&r.label.as_str())).count()
}

/// The panel alone: up to the row under the bar, along to the button `back` stops before the last (the panel's): 2 the sidebar's view,
/// 1 compact; a press.
pub(super) fn press_tool(s: &mut Shelf, back: i32) {
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Move { pane: None, by: -1 });
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -back });
    shell::act(Action::Confirm(None));
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
}

/// The sidebar's view button, pressed by the panel alone.
fn press_view_button(s: &mut Shelf) {
    press_tool(s, 2);
}

// The button is a stop of the row under the bar (before the panel's), a press shows the sidebar as bubbles and another as lines.
// More of the tags are above the fold as bubbles; each bubble is on a line with its place along it, none past the sidebar's width,
// none over another.
#[test]
fn bubbles_show_many_entries_to_a_line() {
    let mut s = tagged("bubbles");
    let ui = s.ui();
    let lib = ui.global::<LibraryState>();
    assert!(!lib.get_bubbles());
    let (list_tags, list_lines) = (tags_in_view(&ui), lib.get_side_total());
    s.e.calls.clear();
    press_view_button(&mut s);
    assert!(lib.get_view_cursor(), "the view's button has the cursor");
    assert!(lib.get_bubbles(), "a press on the button");
    assert!(s.e.calls.is_empty(), "a view is not a question for the engine: {:?}", s.e.methods());
    let (bubble_tags, bubble_lines) = (tags_in_view(&ui), lib.get_side_total());
    eprintln!("POLISH3 tags in the first view: {list_tags} as lines, {bubble_tags} as bubbles; lines in all {list_lines} and {bubble_lines}");
    assert!(bubble_tags > list_tags, "more tags above the fold: {list_tags} then {bubble_tags}");
    assert!(bubble_lines < list_lines, "fewer lines in all");
    let (geo, room) = (lib.get_side_geo(), lib.get_side_room());
    let first = geo.row_data(0).unwrap();
    let second = geo.row_data(1).unwrap();
    assert_eq!((first.line, second.line), (0, 0), "Favorites and Recent share the first line");
    for i in 0..geo.row_count() {
        let g = geo.row_data(i).unwrap();
        assert!(g.x >= 0. && g.x + g.w <= room + 0.01, "inside the sidebar: {} + {} of {room}", g.x, g.w);
        if i > 0 {
            let p = geo.row_data(i - 1).unwrap();
            assert!(p.line < g.line || p.x + p.w <= g.x + 0.01, "no two overlap");
        }
    }
    assert!(lib.get_side_per_view() >= 6);
    // The same press back: lines again, as at first.
    press_view_button(&mut s);
    assert!(!lib.get_bubbles());
    assert_eq!(lib.get_side_total(), list_lines);
    // Nothing at rest: no request, no timer.
    s.e.calls.clear();
    s.f.screen.frame();
    assert!(s.e.calls.is_empty());
}

// The cursor among bubbles: right and left along a line, down and up to the nearest across on the next line, right past the last of a
// line to the list, up from the first line to the tabs; the dial goes one bubble at a time over headings; a press chooses as in the
// list (a library alone, tags together).
#[test]
fn the_cursor_moves_between_bubbles_and_chooses() {
    let mut s = tagged("bubbles-nav");
    let ui = s.ui();
    let lib = ui.global::<LibraryState>();
    press_view_button(&mut s);
    // Down out of the bar's stop is the list; left from there is the sidebar's first bubble.
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    shell::act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("Favorites"));
    shell::act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("Recent"), "right: along the line");
    shell::act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("Favorites"), "and left");
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("Libraries"), "down: the heading is a line of its own");
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("All"), "and the library nearest across below it");
    shell::act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("My own"), "libraries share a line");
    // A library is chosen alone.
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["source"], "mine");
    assert!(lib.get_side_rows().iter().any(|r| r.on && r.label == "My own"));
    // Down to the tags, two chosen together.
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("A Friend's"), "the library that wraps");
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(side_cursor(&ui).as_deref(), Some("Tags"), "the next heading");
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    let first_tag = side_cursor(&ui).unwrap();
    assert!(TAGS.contains(&first_tag.as_str()), "a tag: {first_tag}");
    shell::act(Action::Confirm(None));
    s.serve();
    shell::act(Action::Arrow { dx: 1, dy: 0 });
    let second_tag = side_cursor(&ui).unwrap();
    shell::act(Action::Confirm(None));
    s.serve();
    let chosen = s.e.called("library.query").last().unwrap()["tags"].clone();
    assert_eq!(chosen, json!([first_tag, second_tag]), "several tags, all must match");
    assert_eq!(s.e.called("library.query").last().unwrap()["source"], "mine", "with the library");
    // Right at the end of a line leaves for the list; up from the first line reaches the tabs.
    for _ in 0..20 {
        if side_cursor(&ui).is_none() {
            break;
        }
        shell::act(Action::Arrow { dx: 1, dy: 0 });
    }
    assert_eq!(side_cursor(&ui), None, "right past the end of a line: the list");
    shell::act(Action::Arrow { dx: -1, dy: 0 });
    for _ in 0..30 {
        shell::act(Action::Arrow { dx: 0, dy: -1 });
        if side_cursor(&ui).is_none() {
            break;
        }
    }
    assert_eq!(lib.get_page_cursor(), 0, "up from the first line: the tabs");
    // The dial: one bubble a step, in order.
    shell::act(Action::Arrow { dx: 0, dy: 1 });
    shell::act(Action::Arrow { dx: -1, dy: 0 });
    let mut seen = vec![side_cursor(&ui).unwrap()];
    for _ in 0..4 {
        shell::act(Action::Move { pane: None, by: 1 });
        seen.push(side_cursor(&ui).unwrap());
    }
    assert_eq!(seen[..4], ["Favorites", "Recent", "Libraries", "All"], "{seen:?}");
}

// The choice is kept the way the JUCE pane keeps it, in library-view.txt beside the theme's choice ("comfortable" or "compact", "list"
// or "pills"), and the next Library starts as it was left; a choice made by the file is read.
#[test]
fn the_view_is_kept_in_library_view_txt() {
    // The tests' own folders are set when the first window is made: never the user's.
    drop(tagged("bubbles-kept-folders"));
    crate::settings::test_view::file(true);
    let path = crate::settings::config_dir().unwrap().join("library-view.txt");
    let _ = std::fs::remove_file(&path);
    let mut s = tagged("bubbles-kept");
    let ui = s.ui();
    assert!(!ui.global::<LibraryState>().get_bubbles());
    press_view_button(&mut s);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "comfortable pills");
    assert_eq!(crate::settings::library_view(), (false, true));
    let s2 = tagged("bubbles-kept-again");
    assert!(s2.ui().global::<LibraryState>().get_bubbles(), "the next time it starts as bubbles");
    drop(s2);
    // The JUCE pane's own file, compact and pills: read as it writes it.
    std::fs::write(&path, "compact list").unwrap();
    assert_eq!(crate::settings::library_view(), (true, false));
    let _ = std::fs::remove_file(&path);
    crate::settings::test_view::file(false);
}

// Pictures: the Library with the sidebar as lines and as bubbles, in the default theme and the blue square one.
#[test]
fn polish_3_pictures() {
    for (suffix, theme) in [("", orange()), ("-blue", blue())] {
        let _ = crate::settings::keep_library_view(false, false);
        let mut s = tagged(&format!("polish-3{suffix}"));
        let ui = s.ui();
        theme::apply(&ui, &theme);
        s.f.screen.frame();
        s.f.screen.frame();
        lib_png(&s, &format!("polish-3-list{suffix}"));
        press_view_button(&mut s);
        // The first tag chosen, and the cursor on the second, to see both.
        shell::act(Action::Arrow { dx: 0, dy: 1 });
        shell::act(Action::Arrow { dx: -1, dy: 0 });
        for _ in 0..5 {
            shell::act(Action::Move { pane: None, by: 1 });
        }
        s.f.screen.frame();
        s.f.screen.frame();
        lib_png(&s, &format!("polish-3-bubbles{suffix}"));
    }
}

// The top bar's meters on the Library as a user has it (rows, tags and a panel shown, not the empty screen): a changing
// figure repaints a rectangle inside the resource section and no pixel outside it changes (the idle cost on a unit is
// one full frame a second if anything else on the screen follows the meters).
#[test]
fn a_meter_update_on_the_filled_library_repaints_only_the_section() {
    let mut s = tagged("meters-filled");
    // The scene draws at a scale of 1.55: the region the renderer reports is in physical pixels.
    let (x, _, w, h) = s.f.screen.resources();
    let k = |v: i32| (v as f32 * 1.55).floor() as i32;
    let section = (k(x), 0, k(w) + 2, k(h) + 2);
    for (cpu, ram) in [(1, 30), (74, 31), (90, 31), (100, 95), (7, 5)] {
        let before = s.f.screen.pixels.clone();
        let bar = s.ui();
        let bar = bar.global::<crate::Bar>();
        bar.set_cpu(cpu);
        bar.set_cpu_known(true);
        bar.set_ram(ram);
        bar.set_ram_known(true);
        let repainted = s.f.screen.frame().expect("a change draws");
        assert!(within(repainted, section), "{cpu}/{ram}: repainted {repainted:?}, the section is {section:?}");
        assert_eq!(super::changed_outside(&before, &s.f.screen.pixels, section), 0);
        assert_eq!(s.f.screen.frame(), None, "nothing more is drawn");
    }
}
