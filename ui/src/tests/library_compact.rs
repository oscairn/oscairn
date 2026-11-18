// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's compact mode and the sidebar's folding sections (the owner's, 2026-11-17): a button in the row under the top bar,
//! the spacing scale's 0.7 step applied to the gaps of the rows and entries and the footer line cut to the type, the rows still a
//! touch target tall; a heading folds its section. Pictures: polish-4-compact, polish-4-compact-bubbles (and -blue).

use super::library_bubbles::{press_tool, tagged, tags_in_view};
use super::library_layout::side_cursor;
use super::library_shell::Shelf;
use super::owner::lib_png;
use super::theme_pictures::{blue, orange};
use super::*;
use crate::{LibraryState, PartState};

/// What the window shows: the list's rows and the sidebar's entries in the view, and how many each view holds.
fn counts(ui: &Shell) -> (usize, usize, usize) {
    let (rows, side) = (ui.global::<PartState>().get_rows_per_view() as usize, ui.global::<LibraryState>().get_side_per_view() as usize);
    (rows, side, ui.global::<LibraryState>().get_side_rows().row_count())
}

/// The sidebar's cursor to the line `label` by the dial, then a press.
fn press_side(s: &mut Shelf, label: &str) {
    let ui = s.ui();
    if side_cursor(&ui).is_none() {
        let l = ui.global::<LibraryState>();
        if l.get_page_cursor() >= 0 || l.get_search_cursor() || l.get_view_cursor() || l.get_compact_cursor() || l.get_info_cursor() {
            shell::act(Action::Arrow { dx: 0, dy: 1 }); // down from the row under the bar to the list
        }
        shell::act(Action::Arrow { dx: -1, dy: 0 });
    }
    for _ in 0..60 {
        if side_cursor(&ui).as_deref() == Some(label) {
            break;
        }
        shell::act(Action::Move { pane: None, by: 1 });
    }
    assert_eq!(side_cursor(&ui).as_deref(), Some(label), "reached by the dial");
    shell::act(Action::Confirm(None));
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
}

// Compact is a stop of the row under the bar (between the sidebar's view and the panel's button): a press closes up the rows and
// the entries (the gap at the spacing scale's 0.7 step, the footer line the height of the type), and shows more of both at
// 1280x720; every row and entry is still the theme's touch target tall (44); it is kept, as the JUCE pane keeps it, and the next
// Library starts that way; a press again opens it out.
#[test]
fn compact_shows_more_rows_and_entries_and_is_kept() {
    let mut s = tagged("compact");
    let ui = s.ui();
    let lib = ui.global::<LibraryState>();
    let t = ui.global::<crate::Theme>();
    let normal = counts(&ui);
    assert!(!lib.get_compact());
    let touch = t.get_touch();
    assert!(touch >= 44., "the least touch target is {touch}");
    s.e.calls.clear();
    press_tool(&mut s, 1);
    assert!(lib.get_compact_cursor(), "the button has the cursor");
    assert!(lib.get_compact());
    assert!(s.e.calls.is_empty(), "a view is not a question for the engine");
    let compact = counts(&ui);
    eprintln!("POLISH4 at 1280x720: rows in the list {} then {}, entries in the sidebar {} then {} (lines in a view), the touch target stays {touch}", normal.0, compact.0, normal.1, compact.1);
    assert!(compact.0 > normal.0 && compact.1 > normal.1, "more of both: {normal:?} then {compact:?}");
    // The gap is the spacing scale at 0.7: a theme with spacing 0.7 has that gap.
    let (gap, dense) = (t.get_gap(), t.invoke_dense(t.get_gap(), true));
    assert!((dense - gap * 0.7).abs() < 0.001, "{dense} of {gap}");
    assert_eq!(t.invoke_dense(gap, false), gap);
    assert!(t.invoke_footer(true) < touch && t.invoke_footer(true) >= t.get_body());
    // Where the scale is already at its least, compact goes no lower.
    t.set_spacing(0.6);
    assert!((t.invoke_dense(t.get_gap(), true) - t.get_gap()).abs() < 0.001, "no lower than the scale's least");
    t.set_spacing(1.0);
    // Kept; opened out again; and the next Library starts as it was left.
    assert_eq!(crate::settings::library_view(), (true, false));
    press_tool(&mut s, 1);
    assert!(!lib.get_compact());
    assert_eq!(counts(&ui), normal);
    assert_eq!(crate::settings::library_view(), (false, false));
    press_tool(&mut s, 1);
    assert_eq!(crate::settings::library_view_text(true, true), "compact pills");
    let s2 = tagged("compact-again");
    assert!(s2.ui().global::<LibraryState>().get_compact(), "the next time it starts compact");
}

// A heading folds its section: reached by the dial and pressed (or touched), the libraries' entries go and the tags come up the
// view; nothing is asked of the engine; pressed again they are back. With bubbles too. The fold is the session's (not kept).
#[test]
fn a_heading_folds_its_section() {
    for bubbles in [false, true] {
        let mut s = tagged("fold");
        let ui = s.ui();
        let lib = ui.global::<LibraryState>();
        if bubbles {
            press_tool(&mut s, 2);
        }
        let before = (tags_in_view(&ui), lib.get_side_total());
        s.e.calls.clear();
        press_side(&mut s, "Libraries");
        assert!(s.e.calls.is_empty(), "a fold is not a question for the engine: {:?}", s.e.methods());
        let labels: Vec<String> = lib.get_side_rows().iter().map(|r| r.label.to_string()).collect();
        assert!(!labels.iter().any(|l| l == "All" || l == "My own"), "the libraries are folded away: {labels:?}");
        assert!(lib.get_side_rows().iter().any(|r| r.label == "Libraries" && r.value == "closed"), "the heading says it is closed");
        let after = (tags_in_view(&ui), lib.get_side_total());
        eprintln!("POLISH4 bubbles {bubbles}: tags in the first view {} then {}, lines in all {} then {}", before.0, after.0, before.1, after.1);
        assert!(after.0 > before.0 && after.1 < before.1, "the tags come up: {before:?} then {after:?}");
        // The cursor was on the heading; a press opens it.
        assert_eq!(side_cursor(&ui).as_deref(), Some("Libraries"));
        shell::act(Action::Confirm(None));
        s.serve();
        assert!(lib.get_side_rows().iter().any(|r| r.label == "All"));
        assert_eq!((tags_in_view(&ui), lib.get_side_total()), before);
        // By touch: the Libraries heading's row folds them again, then the Tags heading's row, now in view.
        let row = |l: &LibraryState<'_>, name: &str| l.get_side_rows().iter().position(|r| r.label == name).unwrap_or_else(|| panic!("{name} in view"));
        lib.invoke_side_tapped(row(&lib, "Libraries") as i32);
        s.serve();
        lib.invoke_side_tapped(row(&lib, "Tags") as i32);
        s.serve();
        assert!(lib.get_side_rows().iter().any(|r| r.label == "Tags" && r.value == "closed"));
        assert!(!lib.get_side_rows().iter().any(|r| TAGS_SOME.contains(&r.label.as_str())), "no tag under a closed Tags");
    }
}

const TAGS_SOME: [&str; 4] = ["keys", "brass", "pad", "live"];

// Pictures: compact, with the libraries folded, as lines and as bubbles; the default theme and the blue square one.
#[test]
fn polish_4_pictures() {
    for (suffix, theme) in [("", orange()), ("-blue", blue())] {
        for bubbles in [false, true] {
            let _ = crate::settings::keep_library_view(false, false);
            let mut s = tagged("polish-4");
            let ui = s.ui();
            theme::apply(&ui, &theme);
            s.f.screen.frame();
            s.f.screen.frame();
            if bubbles {
                press_tool(&mut s, 2);
            }
            press_tool(&mut s, 1);
            press_side(&mut s, "Libraries");
            // The cursor past the Tags heading on the first tag, and a tag chosen.
            shell::act(Action::Move { pane: None, by: 2 });
            shell::act(Action::Confirm(None));
            s.serve();
            s.f.screen.frame();
            s.f.screen.frame();
            lib_png(&s, &format!("polish-4-compact{}{suffix}", if bubbles { "-bubbles" } else { "" }));
        }
    }
}
