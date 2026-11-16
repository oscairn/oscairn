// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tab family (slint/theme.slint TabBar, TabPages; src/tabs.rs the model; ui/TABS.md): the parameter pages
//! on it, the model from the module paths and from what a plugin declares, the cursor and a press, a strip that
//! goes on, and the selected tab told from the others in the rendered pixels of every theme.

use super::shots::open_with;
use super::*;
use crate::palette::{contrast, TAB_CONTRAST};
use crate::{GridState, TabStyle, Theme};

fn p(id: u32, name: &str, module: &str, widget: &str) -> serde_json::Value {
    json!({"id": id, "name": name, "module": module, "minimum": 0.0, "maximum": 1.0, "defaultValue": 0.0, "stepped": false, "flags": 0, "widget": widget, "priority": 0, "group": 0})
}

/// Five pages, the first of eight tabs (an operator each, four parameters on it): 32 parameters on the page,
/// which is more than the 24 a page may have before it gets tabs.
pub(super) fn five_pages() -> serde_json::Value {
    let mut list = Vec::new();
    for n in 1..=8u32 {
        for (k, name) in ["Level", "Ratio", "Fine", "Detune"].iter().enumerate() {
            list.push(p(n * 10 + k as u32, name, &format!("Operators/Operator {n}"), if k % 2 == 0 { "knob" } else { "slider" }));
        }
    }
    for (page, names) in [("Filter", vec!["Cutoff", "Resonance", "Drive", "Keytrack"]), ("Amp", vec!["Level", "Pan", "Velocity"]), ("Effects", vec!["Chorus", "Delay", "Reverb", "Mix", "Width"]), ("Global", vec!["Tune", "Glide", "Bend"])] {
        for (k, name) in names.iter().enumerate() {
            list.push(p(1000 + list.len() as u32 + k as u32, name, page, if k % 2 == 0 { "knob" } else { "slider" }));
        }
    }
    json!({"parameters": list})
}

pub(super) fn values_of(list: &serde_json::Value) -> Vec<(u32, f64)> {
    list["parameters"].as_array().unwrap().iter().enumerate().map(|(i, q)| (q["id"].as_u64().unwrap() as u32, 0.2 + 0.07 * (i % 9) as f64)).collect()
}

pub(super) fn percent(_: u32, v: f64) -> String {
    format!("{:.0} %", v * 100.)
}

/// A plugin's page in a window of this size and scale, in this theme and tab style.
fn shown(list: &serde_json::Value, (w, h): (usize, usize), scale: f32, theme: &theme::Loaded, style: TabStyle) -> (Screen, crate::params::Screen) {
    fit_to_window();
    let mut s = Screen::sized(w, h);
    s.set_scale(scale);
    theme::apply(&s.ui, theme);
    s.ui.global::<Theme>().set_tabs(style);
    let mut screen = open_with(&mut s, list.clone(), &values_of(list), &percent);
    s.frame();
    screen.shown();
    s.frame();
    (s, screen)
}

fn plugins() -> Vec<(&'static str, serde_json::Value)> {
    vec![("five-pages", five_pages()), ("two-pages", super::grid::every_kind()), ("fm", super::fm::fm_n(&[super::fm::parallel()]))]
}

// Pictures for a look by eye, target/theme-shots/tabs-PLUGIN-THEME-STYLE@WxHxSCALE.png: the three plugins in
// every theme, in both tab styles, on the keyboard's screen (1280x720 at 1.55), on a 1280x720 window at 1 and
// on 800x480.
#[test]
fn tab_pictures() {
    for (plugin, list) in plugins() {
        for (size, scale) in [((1280, 720), 1.55f32), ((1280, 720), 1.), ((800, 480), 1.)] {
            for t in &all_themes() {
                for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
                    let (s, _) = shown(&list, size, scale, t, style);
                    shot(&s, &format!("tabs-{plugin}-{}-{label}@{}x{}x{scale}", t.id, size.0, size.1));
                }
            }
        }
    }
}

// The tab rows enlarged, for looking closely: the pages' strip and the tabs' strip at the keyboard's scale (the left
// half, twice), and the pages' column with the first of the tabs' strip at 1 (three times), with the cursor on the
// second page and the second tab; target/theme-shots/tabrows-*.png.
#[test]
fn tab_rows_enlarged() {
    use crate::nav::Action;
    let list = five_pages();
    for t in &all_themes() {
        for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
            let (mut s, mut p) = shown(&list, (1280, 720), 1.55, t, style);
            p.act(Action::Move { pane: None, by: 1 });
            p.act(Action::Move { pane: None, by: -1 });
            p.act(Action::Move { pane: None, by: 1 });
            s.frame();
            png_crop(&s, &format!("tabrows-{}-{label}-strips@1.55", t.id), (0, 150, 640, 160), 2);
            let (mut s, mut p) = shown(&list, (1280, 720), 1., t, style);
            p.act(Action::Move { pane: None, by: 1 });
            p.act(Action::Arrow { dx: -1, dy: 0 });
            p.act(Action::Arrow { dx: 0, dy: 1 });
            s.frame();
            png_crop(&s, &format!("tabrows-{}-{label}-column@1", t.id), (0, 96, 440, 240), 3);
        }
    }
}

/// What the cursor is on: the search, a page, a tab, a tile.
#[derive(Debug, PartialEq)]
enum On {
    Nothing,
    Search,
    Page(i32),
    Tab(i32),
    Tile(usize),
}

fn on(ui: &Shell) -> On {
    let g = ui.global::<GridState>();
    let tile = g.get_tiles().iter().position(|t| t.cursor);
    match (g.get_search_cursor(), g.get_page_cursor(), g.get_tab_cursor(), tile) {
        (false, -1, -1, None) => On::Nothing,
        (true, -1, -1, None) => On::Search,
        (false, p, -1, None) => On::Page(p),
        (false, -1, t, None) => On::Tab(t),
        (false, -1, -1, Some(t)) => On::Tile(t),
        other => panic!("more than one cursor: {other:?}"),
    }
}

fn requested(p: &mut crate::params::Screen) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(&mut p.out).into_iter().filter(|o| o.1 != "params.text").map(|(_, m, v)| (m, v)).collect()
}

fn asked_ids(sent: &[(&'static str, serde_json::Value)]) -> Vec<u64> {
    sent.iter().filter(|o| o.0 == "params.get").flat_map(|o| o.1["values"].as_array().cloned().unwrap_or_default()).filter_map(|v| v["param"].as_u64()).collect()
}

// Without touch, pages as a strip over the tabs (the keyboard's arrangement): the dial walks the search, the
// pages, the tabs and the tiles in that order, the arrows go along a row and between rows, and nothing opens
// until a press: the page shown and the requests are as they were. A press opens, the cursor stays on it; the
// page keys step through the pages and tabs at once; Cancel backs out of the cursor, then the screen; a touch
// opens by itself and lets the cursor go (one cursor at a time).
#[test]
fn the_cursor_moves_over_the_tab_rows_and_a_press_opens() {
    use crate::nav::Action;
    let mut s = Screen::new();
    s.ui.global::<Theme>().set_column_from(100000.);
    let ui = s.ui.clone_strong();
    let g = ui.global::<GridState>();
    let list = five_pages();
    let mut p = open_with(&mut s, list.clone(), &values_of(&list), &percent);
    requested(&mut p);
    assert_eq!((g.get_page(), g.get_tab(), on(&ui)), (0, 0, On::Nothing));
    let at = |p: &mut crate::params::Screen, a: Action| {
        p.act(a);
    };
    // The first move is on the first tile; back from it the tabs, on the one shown, then the pages, then the search.
    at(&mut p, Action::Move { pane: None, by: 1 });
    assert_eq!(on(&ui), On::Tile(0));
    at(&mut p, Action::Move { pane: None, by: -1 });
    assert_eq!(on(&ui), On::Tab(0));
    // Along the tabs: nothing opens.
    at(&mut p, Action::Move { pane: None, by: 1 });
    at(&mut p, Action::Move { pane: None, by: 1 });
    assert_eq!(on(&ui), On::Tab(2));
    assert_eq!((g.get_page(), g.get_tab()), (0, 0), "the page does not follow the cursor");
    assert_eq!(asked_ids(&requested(&mut p)), Vec::<u64>::new(), "and nothing was asked for another tab");
    // A press opens the tab under the cursor (operator 3's parameters are 30 to 33), which stays under it.
    at(&mut p, Action::Confirm(None));
    assert_eq!((g.get_tab(), on(&ui)), (2, On::Tab(2)));
    assert_eq!(asked_ids(&requested(&mut p)), [30, 31, 32, 33]);
    // The arrows: along the row, clamped; up to the pages (on the one shown) and the search; down again.
    at(&mut p, Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(on(&ui), On::Tab(1));
    at(&mut p, Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(on(&ui), On::Page(0));
    at(&mut p, Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(on(&ui), On::Search);
    at(&mut p, Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(on(&ui), On::Search, "nothing above it");
    at(&mut p, Action::Arrow { dx: 0, dy: 1 });
    at(&mut p, Action::Arrow { dx: 1, dy: 0 });
    assert_eq!((on(&ui), g.get_page()), (On::Page(1), 0), "along the pages, nothing opened");
    // A press on a page opens it: its first (here only) entry, a page without tabs, so no row of tabs.
    at(&mut p, Action::Confirm(None));
    assert_eq!((g.get_page(), g.get_tabs().row_count(), on(&ui)), (1, 0, On::Page(1)));
    assert_eq!(tile_names(&ui), ["Cutoff", "Resonance", "Drive", "Keytrack"]);
    // Down from the pages with no tabs: the first tile; the tiles stop at their end (nothing turns the page).
    at(&mut p, Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(on(&ui), On::Tile(0));
    at(&mut p, Action::End(1));
    at(&mut p, Action::Move { pane: None, by: 5 });
    assert_eq!((on(&ui), g.get_page()), (On::Tile(3), 1), "past the last tile it stops");
    // The page keys step through pages and tabs at once, from the first: tab by tab, then the next page.
    p.show_page(0);
    requested(&mut p);
    at(&mut p, Action::Page(1));
    assert_eq!((g.get_page(), g.get_tab(), on(&ui)), (0, 1, On::Nothing));
    assert_eq!(asked_ids(&requested(&mut p)), [20, 21, 22, 23]);
    p.show_tab(7);
    at(&mut p, Action::Page(1));
    assert_eq!((g.get_page(), g.get_tabs().row_count()), (1, 0), "past the last tab: the next page");
    at(&mut p, Action::Page(-1));
    assert_eq!((g.get_page(), g.get_tab()), (0, 7), "and back to the last tab of the page before");
    // A touch opens by itself and takes the cursor away.
    at(&mut p, Action::Move { pane: None, by: 1 });
    at(&mut p, Action::Move { pane: None, by: -1 });
    assert_eq!(on(&ui), On::Tab(7));
    p.show_tab(3);
    assert_eq!((g.get_tab(), on(&ui)), (3, On::Nothing));
    // Cancel: the cursor, then the screen.
    at(&mut p, Action::Move { pane: None, by: 1 });
    at(&mut p, Action::Move { pane: None, by: -1 });
    assert_eq!(on(&ui), On::Tab(3));
    assert_eq!(p.act(Action::Cancel(None)), crate::params::Outcome::Stay);
    assert_eq!(on(&ui), On::Nothing);
    assert_eq!(p.act(Action::Cancel(None)), crate::params::Outcome::Back);
    // The dial alone reaches everything: back from the first tile through the tabs, the pages and the search.
    at(&mut p, Action::Move { pane: None, by: 1 });
    let mut seen = vec![on(&ui)];
    for _ in 0..40 {
        at(&mut p, Action::Move { pane: None, by: -1 });
        if seen.last() != Some(&on(&ui)) {
            seen.push(on(&ui));
        }
    }
    assert_eq!(seen.first(), Some(&On::Tile(0)));
    assert_eq!(seen.last(), Some(&On::Search));
    assert!(seen.contains(&On::Tab(3)) && seen.contains(&On::Page(0)), "{seen:?}");
    // And forward again, through the search, the pages and the tabs to the tiles.
    for _ in 0..40 {
        at(&mut p, Action::Move { pane: None, by: 1 });
    }
    assert!(matches!(on(&ui), On::Tile(_)));
}

fn tile_names(ui: &Shell) -> Vec<String> {
    ui.global::<GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect()
}

// The pages as a column at the left (a window tall enough): up and down the column, right into the tabs, left
// from the tiles' first column or the first tab back to the pages; the same press.
#[test]
fn the_cursor_in_the_column_arrangement() {
    use crate::nav::Action;
    let mut s = Screen::new();
    s.ui.global::<Theme>().set_column_from(0.);
    let ui = s.ui.clone_strong();
    let g = ui.global::<GridState>();
    let list = five_pages();
    let mut p = open_with(&mut s, list.clone(), &values_of(&list), &percent);
    assert!(g.get_column());
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(on(&ui), On::Tile(0));
    p.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(on(&ui), On::Page(0), "left from the first column: the pages");
    p.act(Action::Arrow { dx: 0, dy: 1 });
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!((on(&ui), g.get_page()), (On::Page(2), 0), "down the column, nothing opened");
    p.act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(on(&ui), On::Tab(0), "right: the tabs");
    p.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(on(&ui), On::Page(0), "left from the first tab: the pages, on the one shown");
    p.act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(on(&ui), On::Search, "up the column: the search");
    p.act(Action::Arrow { dx: 0, dy: 1 });
    p.act(Action::Arrow { dx: 0, dy: 1 });
    p.act(Action::Confirm(None));
    assert_eq!((g.get_page(), on(&ui)), (1, On::Page(1)));
    p.act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(on(&ui), On::Tile(0), "right with no tabs: the tiles");
}

/// A rectangle of the window in physical pixels: x0, y0, x1, y1.
type Box4 = (usize, usize, usize, usize);

fn count(s: &Screen, r: Box4, c: slint::Color) -> usize {
    let mut n = 0;
    for y in r.1..r.3.min(s.h) {
        for x in r.0..r.2.min(s.w) {
            let px = s.pixels[y * s.w + x];
            n += usize::from((px.r, px.g, px.b) == (c.red(), c.green(), c.blue()));
        }
    }
    n
}

/// The tab bars of a window of `scale`, as the shell lays them out (src/theme.slint TabPages, slint/grid.slint
/// Grid): the pages' column when the arrangement is one, else the pages' strip, and the tabs' strip.
fn bars(s: &Screen, scale: f32, column: bool) -> (Box4, Box4) {
    let t = s.ui.global::<Theme>();
    let (margin, gap, touch, pages) = (t.get_margin(), t.get_gap(), t.get_touch(), t.get_pages_width());
    let to = |v: f32| (v * scale).round() as usize;
    // Under the shell's top bar, Back and the title.
    let top = t.get_bar_height() + margin + touch + gap;
    let width = s.w as f32 / scale;
    if column {
        let tabs_x = margin + pages + gap;
        ((to(margin), to(top), to(margin + pages), to(top + 5. * touch)), (to(tabs_x), to(top), to(width - margin), to(top + touch)))
    } else {
        ((to(margin), to(top), to(width - margin), to(top + touch)), (to(margin), to(top + touch + gap), to(width - margin), to(top + 2. * touch + gap)))
    }
}

// The selected tab is told by its fill: in every theme (the four built in and the two odd ones), in both tab
// styles, in a column and in a strip, the fill of the selected tab and of an unselected one are both painted
// in quantity, and the contrast between them is at least 3 to 1 (WCAG 1.4.11's least for a graphic), the text
// on the selected one at least 4.5 to 1. The numbers are printed.
#[test]
fn the_selected_tab_is_told_by_its_fill_in_every_theme() {
    let list = five_pages();
    let mut least = f32::MAX;
    for t in &all_themes() {
        for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
            for (column, size, scale) in [(true, (1280, 720), 1.), (false, (1280, 720), 1.55), (false, (800, 480), 1.)] {
                let (mut s, _p) = shown(&list, size, scale, t, style);
                if column != s.ui.global::<GridState>().get_column() {
                    continue;
                }
                let g = s.ui.global::<Theme>();
                let (on, off, ink) = (g.get_tab_on(), g.get_tab_off(), g.get_tab_on_ink());
                let ratio = contrast(on, off);
                least = least.min(ratio);
                assert!(ratio >= TAB_CONTRAST, "{} {label}: the selected fill against an unselected one is {ratio:.2} to 1", t.id);
                assert!(contrast(on, ink) >= 4.5, "{} {label}: the text on the selected tab", t.id);
                s.frame();
                let (first, second) = bars(&s, scale, column);
                for (what, r) in [("pages", first), ("tabs", second)] {
                    // The window is read in the painted pixels: a selected tab's fill and the others' are both there.
                    let (n_on, n_off) = (count(&s, r, on), count(&s, r, off));
                    assert!(n_on > 400 && n_off > 400, "{} {label} {what} {column}: {n_on} pixels of the selected fill, {n_off} of the unselected", t.id);
                }
            }
        }
    }
    println!("least contrast between a selected and an unselected tab's fill over all themes and styles: {least:.2} to 1");
}

// A long name does not stretch its tab past four and a half touch targets, shrinks to 70% of the type and is then
// cut; a short one is not narrower than a touch target. Measured on the selected tab's fill
// in the strip's middle row; pictures tabs-long-*.
#[test]
fn a_long_name_shrinks_to_seventy_percent_and_is_then_cut() {
    let mut list = Vec::new();
    for (page, name) in ["A", "A page named for everything it holds, and then some", "Filter", "Amp"].iter().enumerate() {
        for k in 0..3 {
            list.push(p(page as u32 * 10 + k, &format!("P{k}"), name, "knob"));
        }
    }
    let list = json!({"parameters": list});
    for t in &all_themes()[..2] {
        for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
            let (mut s, mut screen) = shown(&list, (1280, 720), 1.55, t, style);
            let on = s.ui.global::<Theme>().get_tab_on();
            let (strip, _) = bars(&s, 1.55, false);
            let touch = s.ui.global::<Theme>().get_touch() * 1.55;
            // The middle row of the strip: from the first to the last pixel of the selected fill, the tab's width
            // (the text in between is not the fill).
            let row = (strip.1 + strip.3) / 2;
            let width = |s: &Screen| {
                let xs: Vec<usize> = (0..s.w).filter(|&x| (s.pixels[row * s.w + x].r, s.pixels[row * s.w + x].g, s.pixels[row * s.w + x].b) == (on.red(), on.green(), on.blue())).collect();
                (xs.last().copied().unwrap_or(0) - xs.first().copied().unwrap_or(0) + 1) as f32
            };
            let short = width(&s);
            screen.show_page(1);
            s.frame();
            let long = width(&s);
            shot(&s, &format!("tabs-long-{}-{label}@1.55", t.id));
            println!("{} {label}: the selected tab of the short name {short:.0} px wide, of the long one {long:.0} (a touch target is {touch:.0}, four and a half {:.0})", t.id, 4.5 * touch);
            // The fill is the box less the cursor's ring and a hair on each side.
            let fill = 2. * 3. * 1.55;
            assert!(short >= touch - fill - 1., "a short name's tab is a touch target: {short}");
            assert!(long <= 4.5 * touch - fill + 1., "a long name's tab stops at four and a half touch targets: {long}");
            assert!(long > 3. * touch, "and is wide, not squeezed: {long}");
        }
    }
}

/// A plugin of twelve pages of three parameters (the pages overflow a column of a short window).
fn twelve_pages() -> serde_json::Value {
    let mut list = Vec::new();
    for page in 1..=12 {
        for k in 0..3 {
            list.push(p(page * 10 + k, &format!("P{k}"), &format!("Section {page}"), "knob"));
        }
    }
    json!({"parameters": list})
}

// A strip that goes on shows it (an arrow box at the cut end, the tab cut behind it) and scrolls to keep the
// cursor's tab, or the selected one, in view; a column does the same down. Looked at in the pixels: the arrow
// boxes are the surface color.
#[test]
fn a_strip_that_overflows_scrolls_with_visible_continuation() {
    use crate::nav::Action;
    let t = &all_themes()[0];
    let surface = t.data.ink("surface");
    let end = |s: &Screen, r: Box4, vertical: bool, last: bool| -> usize {
        // The arrow box is 0.6 of a touch target at the end, the hairline not counted.
        let more = (0.6 * s.ui.global::<Theme>().get_touch()) as usize;
        let (x0, y0, x1, y1) = r;
        let box4 = match (vertical, last) {
            (false, false) => (x0, y0, x0 + more - 2, y1),
            (false, true) => (x1 - more + 2, y0, x1, y1),
            (true, false) => (x0, y0, x1, y0 + more - 2),
            (true, true) => (x0, y1 - more + 2, x1, y1),
        };
        count(s, box4, surface) * 100 / ((box4.2 - box4.0) * (box4.3 - box4.1)).max(1)
    };
    // A strip of eight tabs on an 800 px wide window: the right end goes on; no tab is cut at the left.
    let list = five_pages();
    let (mut s, mut p) = shown(&list, (800, 480), 1., t, TabStyle::Segmented);
    assert!(!s.ui.global::<GridState>().get_column());
    let (_, strip) = bars(&s, 1., false);
    let (left, right) = (end(&s, strip, false, false), end(&s, strip, false, true));
    println!("strip at the start: {left} % of the left end and {right} % of the right end are an arrow box");
    assert!(left < 30 && right > 70, "{left} {right}");
    // Select the last tab by the dial and a press: the strip scrolls so that it shows, the arrow box is at the left.
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Move { pane: None, by: -1 });
    p.act(Action::Move { pane: None, by: 7 });
    s.frame();
    let (left, right) = (end(&s, strip, false, false), end(&s, strip, false, true));
    println!("the cursor on the last tab: {left} % left, {right} % right");
    assert!(left > 70 && right < 30, "the cursor's tab is scrolled into view: {left} {right}");
    p.act(Action::Confirm(None));
    s.frame();
    let on = s.ui.global::<Theme>().get_tab_on();
    let shown_fill = count(&s, strip, on);
    assert!(shown_fill > 3000, "the selected last tab is in view and whole: {shown_fill} pixels of its fill");
    // Back to the first: the strip scrolls back.
    p.show_tab(0);
    s.frame();
    assert!(end(&s, strip, false, false) < 30 && end(&s, strip, false, true) > 70);
    // A column of twelve pages on a short window, the same down.
    let list = twelve_pages();
    fit_to_window();
    let mut s = Screen::sized(800, 480);
    theme::apply(&s.ui, t);
    s.ui.global::<Theme>().set_column_from(0.);
    let mut p = open_with(&mut s, list.clone(), &values_of(&list), &percent);
    s.frame();
    p.shown();
    s.frame();
    let (col, _) = bars(&s, 1., true);
    let col = (col.0, col.1, col.2, 480 - 6);
    let (top, bottom) = (end(&s, col, true, false), end(&s, col, true, true));
    println!("column at the start: {top} % top, {bottom} % bottom");
    assert!(top < 30 && bottom > 70, "{top} {bottom}");
    p.show_page(11);
    s.frame();
    let (top, bottom) = (end(&s, col, true, false), end(&s, col, true, true));
    assert!(top > 70 && bottom < 30, "the last page is scrolled into view: {top} {bottom}");
    assert!(count(&s, col, s.ui.global::<Theme>().get_tab_on()) > 3000, "and whole");
}

fn area(s: &Screen) -> (f32, f32) {
    let g = s.ui.global::<GridState>();
    (g.get_view_width(), g.get_view_height())
}

// The two arrangements on the keyboard's screen (826 x 465 logical at 1.55), the pages as a column with the
// tabs a strip over the tiles, or two strips stacked: the area left for the tiles, in logical pixels.
#[test]
fn the_arrangements_on_the_keyboards_screen() {
    let t = &all_themes()[0];
    for (plugin, list) in plugins().into_iter().take(2) {
        for (label, from) in [("column", 0.), ("stacked", 100000.)] {
            fit_to_window();
            let mut s = Screen::sized(1280, 720);
            s.set_scale(1.55);
            theme::apply(&s.ui, t);
            s.ui.global::<Theme>().set_column_from(from);
            let mut screen = open_with(&mut s, list.clone(), &values_of(&list), &percent);
            s.frame();
            screen.shown();
            s.frame();
            let (w, h) = area(&s);
            println!("{plugin} {label}: tile area {w:.0} x {h:.0} = {:.0} (logical px)", w * h);
            shot(&s, &format!("tabs-arrangement-{plugin}-{label}@1.55"));
        }
    }
}
