// SPDX-License-Identifier: AGPL-3.0-or-later
//! The two overlays shared by every screen that can pop one up — the menu (menu.slint) and the
//! notice (notice.slint) — audited against the owner's rules: at rest nothing is drawn and nothing
//! asks, every control fits the small window with a long name (nothing leaves the window, and the
//! rows page), and in the light theme the text reads on its fill and the cursor border shows. The
//! loading chip shares the same rules and is tested in loading.rs.

use super::*;
use crate::menu::{Item, Menu};
use crate::notice::Notice;
use crate::{MenuState, NoticeState, Theme as ThemeGlobal};

const LONG: &str = "A very long title for this overlay that really must be elided by the window fourty plus";

/// A menu that both pages (more rows than one view holds at 800x480) and carries long names.
fn long_menu() -> Vec<Item> {
    vec![
        Item::head("Heading over the rows that itself carries a very long name"),
        Item::choice("A first choice with an extremely long label that the menu must elide at tiny sizes", 1),
        Item::ticked("Second choice with a long label and a tick shown on the right", 2, true),
        Item::choice("Third", 3),
        Item::sub("A submenu whose label also has quite a long name indeed", vec![Item::choice("Inner one", 4), Item::choice("Inner two", 5)]),
        Item::choice("Sixth row that continues well past a single view", 6),
        Item::choice("Seventh row with another long label to make the list turn", 7),
        Item::choice("Eighth row, yet another one to page past", 8),
        Item::choice("Ninth and final row of the long list, off the first view", 9),
    ]
}

fn luminance(c: slint::Color) -> f64 {
    let lin = |v: u8| {
        let v = f64::from(v) / 255.;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.red()) + 0.7152 * lin(c.green()) + 0.0722 * lin(c.blue())
}

fn contrast(a: slint::Color, b: slint::Color) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The window's pixels at or below `top` (the physical pixel the overlay's area starts at, the top
/// bar's foot) that hold a color near `c`, as (minx, miny, maxx, maxy) or None. Restricting the scan
/// to below the bar keeps the bar's own text and fills out of the measurements of the overlay.
fn bounds_near(s: &Screen, c: slint::Color, top: usize) -> Option<(usize, usize, usize, usize)> {
    let rgb = (i32::from(c.red()), i32::from(c.green()), i32::from(c.blue()));
    let near = |p: &Rgb8Pixel| (i32::from(p.r) - rgb.0).pow(2) + (i32::from(p.g) - rgb.1).pow(2) + (i32::from(p.b) - rgb.2).pow(2) <= 64;
    let (w, h) = (s.w, s.h);
    let top = top.min(h);
    let mut min_x = w;
    let mut min_y = h;
    let (mut max_x, mut max_y) = (0usize, 0usize);
    let mut any = false;
    for y in top..h {
        for x in 0..w {
            if near(&s.pixels[y * w + x]) {
                any = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    any.then_some((min_x, min_y, max_x, max_y))
}

/// The overlay's area starts at the bar's foot (`content-top`, the shell's), under the scale the test
/// set: scans below it leave the bar's own text and fills out of the measurements of the overlay.
fn overlay_top(s: &Screen) -> usize {
    let logical = s.ui.get_content_top();
    (logical as f64 * f64::from(s.scale)) as usize
}

/// The overlay fits the window at this scale: its own text, below the bar, stays inside the window
/// (nothing leaves it, so a long name is cut within, never spilled), and the cursor's focus border
/// is drawn in the theme.
fn holds_overlay(s: &Screen, which: &str) {
    let g = s.ui.global::<ThemeGlobal>();
    let top = overlay_top(s);
    for (color, name) in [(g.get_text(), "the text"), (g.get_muted(), "a muted line"), (g.get_accent(), "a tick or mark")] {
        // Ink in the window's last column or row is text the window cuts.
        if let Some((_minx, _miny, maxx, maxy)) = bounds_near(s, color, top) {
            assert!(maxx + 1 < s.w, "{which}: {name} reaches the window's right edge ({maxx} of {})", s.w);
            assert!(maxy + 1 < s.h, "{which}: {name} reaches the window's bottom ({maxy} of {})", s.h);
        }
    }
    assert!(bounds_near(s, g.get_focus(), top).is_some(), "{which}: the cursor's focus border is not drawn in the theme");
}

fn light(s: &mut Screen) {
    theme::apply(&s.ui, &theme::chosen(&theme::Places::default(), "oscairn-light", ""));
    s.frame();
}

// A notice is answered without touch, as the dial and the knobs reach it: the cursor moves onto a
// choice, a press chooses the number it is on (the notice is closed), Confirm with none open pressed
// leaves it alone, and Cancel closes it with nothing chosen. Every effect is the real one.
#[test]
fn a_notice_is_reached_without_touch() {
    let s = Screen::new();
    let ui = s.ui.clone_strong();
    let mut n = Notice::open(&ui, LONG, "Save changes before opening this Performance?", &["Save", "Discard", "Cancel"]);
    assert_eq!(ui.global::<NoticeState>().get_cursor(), -1, "none until the first move");
    assert_eq!(n.act(crate::nav::Action::Confirm(None)), crate::notice::Outcome::Stay, "a press with none chosen opens nothing");
    assert_eq!(n.act(crate::nav::Action::Move { pane: None, by: 1 }), crate::notice::Outcome::Stay);
    assert_eq!(ui.global::<NoticeState>().get_cursor(), 0, "the dial puts the cursor on the first choice");
    assert_eq!(n.act(crate::nav::Action::Move { pane: None, by: 1 }), crate::notice::Outcome::Stay);
    assert_eq!(ui.global::<NoticeState>().get_cursor(), 1, "the cursor moves along the choices");
    assert_eq!(ui.global::<NoticeState>().get_cursor(), 1);
    assert_eq!(n.act(crate::nav::Action::Confirm(None)), crate::notice::Outcome::Choose(1), "a press chooses the cursor's choice");
    assert!(!ui.global::<NoticeState>().get_open(), "a choice closes the notice");
    let mut n = Notice::open(&ui, LONG, "Delete this Performance?", &["Delete", "Cancel"]);
    assert_eq!(n.act(crate::nav::Action::Cancel(None)), crate::notice::Outcome::Close, "Cancel closes it with nothing chosen");
    assert!(!ui.global::<NoticeState>().get_open());
}

// A menu and a notice shown over an at-rest screen cost nothing once they settle: with nothing
// changing, a tick draws no frame and the window is not animating (the sweep that moves the loading
// chip is not here).
#[test]
fn an_open_menu_or_notice_at_rest_draws_no_frame() {
    let mut s = Screen::new();
    let ui = s.ui.clone_strong();
    let mut m = Menu::open(&ui, LONG, long_menu());
    m.start(1);
    s.frame();
    s.frame();
    assert!(!s.animating(), "the menu neither sweeps nor animates");
    assert_eq!(s.frame(), None, "a tick with nothing changing draws nothing over the menu");
    ui.global::<MenuState>().set_open(false);
    let mut n = Notice::open(&ui, LONG, "A long message to keep the choice from fitting in one line, wrapping as notices do to stay a normal question.", &["Save", "Discard", "Cancel"]);
    n.act(crate::nav::Action::Move { pane: None, by: 1 });
    s.frame();
    s.frame();
    assert!(!s.animating(), "the notice neither sweeps nor animates");
    assert_eq!(s.frame(), None, "a tick with nothing changing draws nothing over the notice");
}

// The menu (a dialog) scrolls the same way the shared lists do (issue #269): when it holds more rows
// than a view a scroll bar shows (total > the view), a drag or the bar turns the view to the row asked
// (page-aligned), and the cursor sits on a shown row.
#[test]
fn the_menu_scrolls_a_list_longer_than_its_view() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut m = Menu::open(ui, "M", long_menu());
    ui.global::<MenuState>().set_rows_per_view(3);
    m.show();
    let ms = ui.global::<MenuState>();
    assert!(ms.get_total() > ms.get_rows_per_view(), "a longer list shows the scroll bar: {} rows, {} in a view", ms.get_total(), ms.get_rows_per_view());
    m.scroll_to(8);
    assert_eq!(ms.get_first(), 6, "a drag or the bar turns to the last view (page-aligned)");
    let shown: Vec<(String, bool)> = ms.get_rows().iter().map(|r| (r.label.to_string(), r.cursor)).collect();
    assert!(shown.iter().any(|r| r.1), "the cursor is on a shown row after the scroll");
    m.scroll_to(0);
    assert_eq!(ms.get_first(), 0, "back to the first view");
    assert!(ms.get_rows().iter().any(|r| r.cursor), "a cursor is still on a shown row");
    assert_eq!((ms.get_total(), ms.get_first()), (9, 0), "the list and view stay consistent");
}

// A long menu fits the small window in the light theme: at 800x480 and at the FANTOM-07's 1280x720 at
// scale 1.55 the rows' text and the page marks stay inside the window, nothing reaches an edge, and
// the cursor's focus border is drawn in the light theme.
#[test]
fn the_menu_fits_the_small_window_in_the_light_theme() {
    let mut s = Screen::sized(800, 480);
    light(&mut s);
    let ui = s.ui.clone_strong();
    let mut m = Menu::open(&ui, LONG, long_menu());
    m.start(1);
    s.frame();
    s.frame();
    assert!(!s.animating());
    holds_overlay(&s, "menu-800x480");
    png(&s, "menu-overlay-800x480");
    theme::apply(&ui, &theme::chosen(&theme::Places::default(), "", ""));

    let mut s = Screen::sized(1280, 720);
    light(&mut s);
    let ui = s.ui.clone_strong();
    let mut m = Menu::open(&ui, LONG, long_menu());
    m.start(1);
    s.set_scale(1.55);
    s.frame(); // the window lays out at the new scale and the menu pages to the rows that fit
    m.show();
    s.frame();
    holds_overlay(&s, "menu-1280x720@1.55");
    png(&s, "menu-overlay@1.55");
    s.set_scale(1.0);
}

// The same for a notice: a long title and a wrapping message fit the small window and the FANTOM's,
// nothing reaches an edge, and the cursor border is drawn in the light theme.
#[test]
fn the_notice_fits_the_small_window_in_the_light_theme() {
    let mut s = Screen::sized(800, 480);
    light(&mut s);
    let ui = s.ui.clone_strong();
    let mut n = Notice::open(&ui, LONG, "A long message to keep the choice from fitting in one line, wrapping as notices do to stay a normal question.", &["Save", "Discard", "Cancel"]);
    n.act(crate::nav::Action::Move { pane: None, by: 1 });
    s.frame();
    s.frame();
    assert!(!s.animating());
    holds_overlay(&s, "notice-800x480");
    png(&s, "notice-overlay-800x480");
    theme::apply(&ui, &theme::chosen(&theme::Places::default(), "", ""));

    let mut s = Screen::sized(1280, 720);
    light(&mut s);
    let ui = s.ui.clone_strong();
    let mut n = Notice::open(&ui, LONG, "A long message to keep the choice from fitting in one line, wrapping as notices do to stay a normal question.", &["Save", "Discard", "Cancel"]);
    n.act(crate::nav::Action::Move { pane: None, by: 1 });
    s.set_scale(1.55);
    s.frame();
    let _ = n.act(crate::nav::Action::Move { pane: None, by: 0 });
    s.frame();
    holds_overlay(&s, "notice-1280x720@1.55");
    png(&s, "notice-overlay@1.55");
    s.set_scale(1.0);
}

// The menu and the notice read in the light theme: the body text of a row at least 4.5 to 1 against
// the surface it sits on, a title at least 3 to 1 against the panel, a small muted line at least 4.5
// to 1, measured from the theme's own tokens.
#[test]
fn the_menu_and_notice_read_in_the_light_theme() {
    let mut s = Screen::new();
    light(&mut s);
    let g = s.ui.global::<ThemeGlobal>();
    let row = contrast(g.get_text(), g.get_surface());
    assert!(row >= 4.5, "the rows' text on the surface is {row:.1} to 1");
    let title = contrast(g.get_text(), g.get_raised());
    assert!(title >= 3.0, "a title on the panel is {title:.1} to 1");
    let muted = contrast(g.get_muted(), g.get_raised());
    assert!(muted >= 4.5, "a small muted line on the panel is {muted:.1} to 1");
    theme::apply(&s.ui, &theme::chosen(&theme::Places::default(), "", ""));
}

// The overlays are the theme's: no length, color or time of their own in the .slint that draws them,
// as the loading chip's own test holds for loading.slint.
#[test]
fn the_menu_and_notice_have_no_size_color_or_time_literals() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("slint");
    for file in ["menu.slint", "notice.slint"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            assert!(!code.contains('#'), "{file}:{}: a literal color: {line}", n + 1);
            for word in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
                let digits: String = word.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                let unit = &word[digits.len()..];
                assert!(digits.is_empty() || digits == "0" || !["px", "ms", "s"].contains(&unit), "{file}:{}: a literal size or time {word}: {line}", n + 1);
            }
        }
    }
}
