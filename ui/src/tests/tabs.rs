// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tab family (slint/theme.slint TabBar, TabPages; src/tabs.rs the model; ui/TABS.md): the parameter pages
//! on it, in the two placements (`top`: two strips; `side`: two rails the height of the screen), the cursor and a
//! press, a strip and a rail that go on, the width and height rules of the rails, the `auto` rule, and the
//! selected tab told from the others in the rendered pixels of every theme.

use super::shots::open_with;
use super::*;
use crate::palette::{contrast, TAB_CONTRAST};
use crate::TabPlacement::{Auto, Side, Top};
use crate::{GridState, TabPlacement, TabStyle, Theme};

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
    for (page, names) in [("Filter", vec!["Cutoff", "Resonance", "Drive", "Keytrack"]), ("Amp", vec!["Level", "Pan", "Velocity"]), ("Voicing", vec!["Glide", "Unison", "Spread", "Mix", "Width"]), ("Global", vec!["Tune", "Glide", "Bend"])] {
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

/// The window `w` x `h` at `scale` with the plugin's page in the placement chosen (whatever the theme says).
fn placed(list: &serde_json::Value, (w, h): (usize, usize), scale: f32, theme: &theme::Loaded, style: TabStyle, placement: TabPlacement) -> (Screen, crate::params::Screen) {
    fit_to_window();
    let mut s = Screen::sized(w, h);
    s.set_scale(scale);
    theme::apply(&s.ui, theme);
    s.ui.global::<Theme>().set_tabs(style);
    s.ui.global::<Theme>().set_tab_placement(placement);
    let mut screen = open_with(&mut s, list.clone(), &values_of(list), &percent);
    s.frame();
    screen.shown();
    s.frame();
    (s, screen)
}

/// Two strips, whatever the theme says: these tests are of the strips.
fn shown(list: &serde_json::Value, size: (usize, usize), scale: f32, theme: &theme::Loaded, style: TabStyle) -> (Screen, crate::params::Screen) {
    placed(list, size, scale, theme, style, Top)
}

fn plugins() -> Vec<(&'static str, serde_json::Value)> {
    vec![("five-pages", five_pages()), ("two-pages", super::grid::every_kind()), ("fm", super::fm::fm_n(&[super::fm::parallel()]))]
}

/// Pages and tabs with the names given: each page of three parameters, and where a page's name has a slash its tabs.
fn named(pages: &[&str]) -> serde_json::Value {
    let mut list = Vec::new();
    for (k, name) in pages.iter().enumerate() {
        for j in 0..3 {
            list.push(p(k as u32 * 10 + j, &format!("P{j}"), name, "knob"));
        }
    }
    json!({"parameters": list})
}

/// A plugin of two pages, the first of `n` tabs of three parameters each named `Operator N` (more than a rail of touch
/// targets holds at 465 above 9), the second with no tabs.
fn operators(n: u32) -> serde_json::Value {
    let mut list = Vec::new();
    for k in 1..=n {
        for (j, name) in ["Level", "Ratio", "Fine"].iter().enumerate() {
            list.push(p(k * 10 + j as u32, name, &format!("Operators/Operator {k}"), "knob"));
        }
    }
    for k in 0..3 {
        list.push(p(900 + k, &format!("Out {k}"), "Output", "knob"));
    }
    json!({"parameters": list})
}

/// Pages and tabs whose names share nothing and are long: the rail cannot show them horizontally at the small size.
const UNRELATED: [&str; 4] = ["Modulation Matrix", "Voice Shaping", "Macro Controls", "Global"];

/// A plugin of two pages: one with four tabs of long unrelated names (more than 24 parameters on it), one without.
fn long_names() -> serde_json::Value {
    let mut list = Vec::new();
    for (k, name) in UNRELATED.iter().enumerate() {
        for j in 0..7 {
            list.push(p(k as u32 * 10 + j, &format!("P{j}"), &format!("Sound/{name}"), "knob"));
        }
    }
    for j in 0..3 {
        list.push(p(900 + j, &format!("Out {j}"), "Output", "knob"));
    }
    json!({"parameters": list})
}

fn tile_names(ui: &Shell) -> Vec<String> {
    ui.global::<GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect()
}

fn area(s: &Screen) -> (f32, f32) {
    let g = s.ui.global::<GridState>();
    (g.get_view_width(), g.get_view_height())
}

/// How many columns of tiles the page has: the tiles in its first row.
fn columns(s: &Screen) -> usize {
    let tiles = s.ui.global::<GridState>().get_tiles();
    let first = tiles.iter().map(|t| t.y).fold(f32::MAX, f32::min);
    tiles.iter().filter(|t| t.y == first).count()
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

/// The two strips of the `top` placement in the window's pixels, as the shell lays them out (slint/theme.slint
/// TabPages, slint/grid.slint Grid): the pages' and the tabs'.
fn bars(s: &Screen, scale: f32) -> (Box4, Box4) {
    let t = s.ui.global::<Theme>();
    let (margin, gap, touch) = (t.get_margin(), t.get_gap(), t.get_touch());
    let to = |v: f32| (v * scale).round() as usize;
    // Under the shell's top bar, which carries Back and the title.
    let top = t.get_bar_height() + margin;
    let width = s.w as f32 / scale;
    ((to(margin), to(top), to(width - margin), to(top + touch)), (to(margin), to(top + touch + gap), to(width - margin), to(top + 2. * touch + gap)))
}

/// The two rails of the `side` placement in the window's pixels: from under the top bar to the bottom of the window.
fn rails(s: &Screen, scale: f32) -> (Box4, Box4) {
    let (g, t) = (s.ui.global::<GridState>(), s.ui.global::<Theme>());
    let to = |v: f32| (v * scale).round() as usize;
    let (rail, gap) = (g.get_rail(), t.get_gap());
    let top = to(t.get_bar_height());
    ((0, top, to(rail), s.h), (to(rail + gap), top, to(2. * rail + gap), s.h))
}

// Pictures for a look by eye, target/theme-shots/tabs-PLUGIN-PLACEMENT-THEME-STYLE@WxHxSCALE.png: the three plugins in
// both placements, every theme, both tab styles, on the keyboard's screen (1280x720 at 1.55), on a 1280x720 window at 1
// and on 800x480.
#[test]
fn tab_pictures() {
    for (plugin, list) in plugins() {
        for (size, scale) in [((1280, 720), 1.55f32), ((1280, 720), 1.), ((800, 480), 1.)] {
            for t in &all_themes() {
                for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
                    for (place, name) in [(Top, "top"), (Side, "side")] {
                        let (s, _) = placed(&list, size, scale, t, style, place);
                        shot(&s, &format!("tabs-{plugin}-{name}-{}-{label}@{}x{}x{scale}", t.id, size.0, size.1));
                    }
                }
            }
        }
    }
}

// Rails of other names, for a look by eye: fourteen tabs (the rail scrolls), long unrelated names, and the cursor on
// a tab, each in both tab styles, in the dark theme and the odd ones, at the keyboard's scale (the rails of the first page
// at the left); target/theme-shots/rail-*.png.
#[test]
fn rail_pictures() {
    use crate::nav::Action;
    for t in &all_themes() {
        for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
            for (name, list) in [("fourteen", operators(14)), ("long-names", long_names())] {
                let (mut s, mut p) = placed(&list, (1280, 720), 1.55, t, style, Side);
                p.act(Action::Move { pane: None, by: 1 });
                p.act(Action::Move { pane: None, by: -1 });
                p.act(Action::Move { pane: None, by: 2 });
                s.frame();
                shot(&s, &format!("rail-{name}-{}-{label}@1.55", t.id));
            }
        }
    }
}

// The tab rows enlarged, for looking closely: at the keyboard's scale the strips (the left half, twice) and the two
// rails (the left 240 physical pixels, three times), with the cursor on the second page and the second tab; at 1
// the rails again; target/theme-shots/tabrows-*.png.
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
            png_crop(&s, &format!("tabrows-{}-{label}-strips@1.55", t.id), (0, 74, 640, 160), 2);
            for (scale, name) in [(1.55f32, "1.55"), (1., "1")] {
                let (mut s, mut p) = placed(&list, (1280, 720), scale, t, style, Side);
                p.act(Action::Move { pane: None, by: 1 });
                p.act(Action::Move { pane: None, by: -1 });
                p.act(Action::Move { pane: None, by: -1 });
                p.act(Action::Arrow { dx: 0, dy: 1 });
                s.frame();
                let top = (48. * scale) as i32;
                png_crop(&s, &format!("tabrows-{}-{label}-rails@{name}", t.id), (0, top, (240. * scale / 1.55) as i32, (720 - top as usize).min(300) as i32), 3);
            }
        }
    }
}

/// What the cursor is on: the search, a page, a tab, a tile.
#[derive(Debug, PartialEq)]
enum On {
    Nothing,
    Page(i32),
    Tab(i32),
    Tile(usize),
}

fn on(ui: &Shell) -> On {
    let g = ui.global::<GridState>();
    let tile = g.get_tiles().iter().position(|t| t.cursor);
    match (g.get_page_cursor(), g.get_tab_cursor(), tile) {
        (-1, -1, None) => On::Nothing,
        (p, -1, None) => On::Page(p),
        (-1, t, None) => On::Tab(t),
        (-1, -1, Some(t)) => On::Tile(t),
        other => panic!("more than one cursor: {other:?}"),
    }
}

fn requested(p: &mut crate::params::Screen) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(&mut p.out).into_iter().filter(|o| o.1 != "params.text").map(|(_, m, v)| (m, v)).collect()
}

fn asked_ids(sent: &[(&'static str, serde_json::Value)]) -> Vec<u64> {
    sent.iter().filter(|o| o.0 == "params.get").flat_map(|o| o.1["values"].as_array().cloned().unwrap_or_default()).filter_map(|v| v["param"].as_u64()).collect()
}

// Without touch, pages as a strip over the tabs: the dial walks the search, the
// pages, the tabs and the tiles in that order, the arrows go along a row and between rows, and nothing opens
// until a press: the page shown and the requests are as they were. A press opens, the cursor stays on it; the
// page keys step through the pages and tabs at once; Cancel backs out of the cursor, then the screen; a touch
// opens by itself and lets the cursor go (one cursor at a time).
#[test]
fn the_cursor_moves_over_the_tab_rows_and_a_press_opens() {
    use crate::nav::Action;
    let list = five_pages();
    let (s, mut p) = shown(&list, (1280, 720), 1., &all_themes()[0], TabStyle::Segmented);
    let ui = s.ui.clone_strong();
    let g = ui.global::<GridState>();
    requested(&mut p);
    assert_eq!((g.get_layout(), g.get_page(), g.get_tab(), on(&ui)), (0, 0, 0, On::Nothing));
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
    // The arrows: along the row, clamped; up to the pages (on the one shown), the first stop; the search is the top bar's.
    at(&mut p, Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(on(&ui), On::Tab(1));
    at(&mut p, Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(on(&ui), On::Page(0));
    at(&mut p, Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(on(&ui), On::Page(0), "nothing above it");
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
    // Back, from wherever the cursor is, leaves the screen at once and drops the cursor.
    at(&mut p, Action::Move { pane: None, by: 1 });
    at(&mut p, Action::Move { pane: None, by: -1 });
    assert_eq!(on(&ui), On::Tab(3));
    assert_eq!(p.back(), crate::params::Outcome::Back);
    assert_eq!(on(&ui), On::Nothing);
    // The dial alone reaches everything: back from the first tile through the tabs and the pages.
    at(&mut p, Action::Move { pane: None, by: 1 });
    let mut seen = vec![on(&ui)];
    for _ in 0..40 {
        at(&mut p, Action::Move { pane: None, by: -1 });
        if seen.last() != Some(&on(&ui)) {
            seen.push(on(&ui));
        }
    }
    assert_eq!(seen.first(), Some(&On::Tile(0)));
    assert_eq!(seen.last(), Some(&On::Page(0)));
    assert!(p.at_top(), "and there moving up leaves for the top bar");
    assert!(seen.contains(&On::Tab(3)) && seen.contains(&On::Page(0)), "{seen:?}");
    // And forward again, through the pages and the tabs to the tiles.
    for _ in 0..40 {
        at(&mut p, Action::Move { pane: None, by: 1 });
    }
    assert!(matches!(on(&ui), On::Tile(_)));
}

// The same in the rails: up and down along each, right from the pages to the tabs (or the tiles), left from the tiles'
// first column to the tabs and from the tabs to the pages; the same press; the dial's order is the pages, the tabs and the
// tiles. The cursor's ring is drawn in the narrow tab.
#[test]
fn the_cursor_in_the_rails() {
    use crate::nav::Action;
    let t = &all_themes()[0];
    for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
        let (s, mut p) = placed(&five_pages(), (1280, 720), 1.55, t, style, Side);
        let ui = s.ui.clone_strong();
        let g = ui.global::<GridState>();
        assert_eq!(g.get_layout(), 1);
        p.act(Action::Move { pane: None, by: 1 });
        assert_eq!(on(&ui), On::Tile(0));
        p.act(Action::Arrow { dx: -1, dy: 0 });
        assert_eq!(on(&ui), On::Tab(0), "left from the first column: the tabs, on the one shown");
        p.act(Action::Arrow { dx: -1, dy: 0 });
        assert_eq!(on(&ui), On::Page(0), "left again: the pages");
        p.act(Action::Arrow { dx: 0, dy: 1 });
        p.act(Action::Arrow { dx: 0, dy: 1 });
        assert_eq!((on(&ui), g.get_page()), (On::Page(2), 0), "down the rail, nothing opened");
        p.act(Action::Arrow { dx: 1, dy: 0 });
        assert_eq!(on(&ui), On::Tab(0), "right: the tabs");
        p.act(Action::Arrow { dx: 0, dy: 1 });
        assert_eq!((on(&ui), g.get_tab()), (On::Tab(1), 0), "down the tabs, nothing opened");
        p.act(Action::Arrow { dx: -1, dy: 0 });
        assert_eq!(on(&ui), On::Page(0), "left from the tabs: the pages, on the one shown");
        p.act(Action::Arrow { dx: 0, dy: -1 });
        assert_eq!(on(&ui), On::Page(0), "up the rail: nothing above the first page");
        p.act(Action::Arrow { dx: 0, dy: 1 });
        // The ring is in the narrow tab: the focus color in quantity round the page's tab, and no other color's ring.
        let mut shot_screen = s;
        shot_screen.frame();
        let focus = shot_screen.ui.global::<Theme>().get_focus();
        let (pages, _) = rails(&shot_screen, 1.55);
        let rows = shot_screen.h - pages.1;
        // The second of five tabs: its ring is two logical pixels thick round the whole box.
        let tab_h = rows / 5;
        let box4 = (pages.0, pages.1 + tab_h, pages.2, pages.1 + 2 * tab_h);
        let ring = count(&shot_screen, box4, focus);
        println!("{label}: {ring} pixels of the focus color round the tab under the cursor");
        assert!(ring > 2 * (box4.2 - box4.0) * 2, "{label}: the ring round a narrow tab is visible: {ring}");
        p.act(Action::Confirm(None));
        assert_eq!((g.get_page(), on(&ui)), (1, On::Page(1)));
        p.act(Action::Arrow { dx: 1, dy: 0 });
        assert_eq!(on(&ui), On::Tile(0), "right with no tabs: the tiles");
        // A touch opens by itself and takes the cursor away.
        p.show_page(3);
        assert_eq!((g.get_page(), on(&ui)), (3, On::Nothing));
    }
}

// The selected tab is told by its fill: in every theme (the four built in and the two odd ones), in both tab
// styles, in the strips and in the rails, the fill of the selected tab and of an unselected one are both painted
// in quantity, and the contrast between them is at least 3 to 1 (WCAG 1.4.11's least for a graphic), the text
// on the selected one at least 4.5 to 1. The numbers are printed.
#[test]
fn the_selected_tab_is_told_by_its_fill_in_every_theme() {
    let list = five_pages();
    let mut least = f32::MAX;
    for t in &all_themes() {
        for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
            for (place, size, scale) in [(Top, (1280, 720), 1.55f32), (Top, (800, 480), 1.), (Side, (1280, 720), 1.55), (Side, (800, 480), 1.), (Side, (1280, 720), 1.)] {
                let (mut s, _p) = placed(&list, size, scale, t, style, place);
                let g = s.ui.global::<Theme>();
                let (on, off, ink) = (g.get_tab_on(), g.get_tab_off(), g.get_tab_on_ink());
                let ratio = contrast(on, off);
                least = least.min(ratio);
                assert!(ratio >= TAB_CONTRAST, "{} {label}: the selected fill against an unselected one is {ratio:.2} to 1", t.id);
                assert!(contrast(on, ink) >= 4.5, "{} {label}: the text on the selected tab", t.id);
                s.frame();
                let (first, second) = if place == Top { bars(&s, scale) } else { rails(&s, scale) };
                for (what, r) in [("pages", first), ("tabs", second)] {
                    // The window is read in the painted pixels: a selected tab's fill and the others' are both there.
                    let (n_on, n_off) = (count(&s, r, on), count(&s, r, off));
                    assert!(n_on > 400 && n_off > 400, "{} {label} {what} {place:?} {size:?}@{scale}: {n_on} pixels of the selected fill, {n_off} of the unselected", t.id);
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
            let (strip, _) = bars(&s, 1.55);
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

// A strip that goes on shows it (an arrow box at the cut end, the tab cut behind it) and scrolls to keep the
// cursor's tab, or the selected one, in view. Looked at in the pixels: the arrow boxes are the surface color. Twelve
// pages of names that share nothing do not fit an 800 px wide window.
#[test]
fn a_strip_that_overflows_scrolls_with_visible_continuation() {
    use crate::nav::Action;
    let t = &all_themes()[0];
    let surface = t.data.ink("surface");
    let end = |s: &Screen, r: Box4, last: bool| -> usize {
        // The arrow box is 0.6 of a touch target at the end, the hairline not counted.
        let more = (0.6 * s.ui.global::<Theme>().get_touch()) as usize;
        let (x0, y0, x1, y1) = r;
        let box4 = if last { (x1 - more + 2, y0, x1, y1) } else { (x0, y0, x0 + more - 2, y1) };
        count(s, box4, surface) * 100 / ((box4.2 - box4.0) * (box4.3 - box4.1)).max(1)
    };
    let list = named(&["Modulation Matrix", "Voice Shaping", "Macro Controls", "Global", "Oscillators", "Envelopes", "Filters", "Keyboard", "Mixer", "Performance", "Voicing", "Macros"]);
    let (mut s, mut p) = shown(&list, (800, 480), 1., t, TabStyle::Segmented);
    let (strip, _) = bars(&s, 1.);
    let (left, right) = (end(&s, strip, false), end(&s, strip, true));
    println!("strip at the start: {left} % of the left end and {right} % of the right end are an arrow box");
    assert!(left < 30 && right > 70, "{left} {right}");
    // Select the last page by the dial and a press: the strip scrolls so that it shows, the arrow box is at the left.
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Move { pane: None, by: -1 });
    p.act(Action::Move { pane: None, by: 11 });
    s.frame();
    let (left, right) = (end(&s, strip, false), end(&s, strip, true));
    println!("the cursor on the last page: {left} % left, {right} % right");
    assert!(left > 70 && right < 30, "the cursor's tab is scrolled into view: {left} {right}");
    p.act(Action::Confirm(None));
    s.frame();
    let on = s.ui.global::<Theme>().get_tab_on();
    let shown_fill = count(&s, strip, on);
    assert!(shown_fill > 3000, "the selected last tab is in view and whole: {shown_fill} pixels of its fill");
    // Back to the first: the strip scrolls back.
    p.show_page(0);
    s.frame();
    assert!(end(&s, strip, false) < 30 && end(&s, strip, true) > 70);
}

// Names that share their first word are shown once in a strip that would overflow: the eight operators' strip, which
// scrolled ("Operator 1" to "Operator 7" and an arrow), shows "Operator" at its start and 1 to 8 as the tabs, all in view.
// A strip that fits shows the whole names.
#[test]
fn a_strip_of_names_that_share_a_word_shows_it_once_and_fits() {
    let t = &all_themes()[0];
    let surface = t.data.ink("surface");
    for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
        for (size, scale) in [((1280, 720), 1.55f32), ((800, 480), 1.)] {
            let (mut s, _) = shown(&five_pages(), size, scale, t, style);
            let (_, strip) = bars(&s, scale);
            let more = (0.6 * s.ui.global::<Theme>().get_touch() * scale) as usize;
            for (what, b) in [("left", (strip.0, strip.1, strip.0 + more - 2, strip.3)), ("right", (strip.2 - more + 2, strip.1, strip.2, strip.3))] {
                let n = count(&s, b, surface) * 100 / ((b.2 - b.0) * (b.3 - b.1)).max(1);
                assert!(n < 30, "{label} {size:?}@{scale}: an arrow box at the {what} end ({n} %): the strip scrolls");
            }
            // All eight tabs are in view: the selected one's fill is there whole, and so is the last tab's box.
            assert!(count(&s, strip, s.ui.global::<Theme>().get_tab_on()) > 400);
            shot(&s, &format!("tabs-shared-word-{label}@{}x{}x{scale}", size.0, size.1));
            s.frame();
        }
    }
    // Whole names where they fit: two pages named alike do not lose the word while the strip has room (a heading is shown only on overflow).
    let (s, _) = shown(&named(&["Operator 1", "Operator 2"]), (1280, 720), 1.55, t, TabStyle::Segmented);
    let g = s.ui.global::<GridState>();
    assert_eq!(g.get_page_heading().as_str(), "Operator", "the heading is known; the strip uses it only when the names do not fit");
    assert_eq!(g.get_pages().iter().map(|n| n.to_string()).collect::<Vec<_>>(), ["Operator 1", "Operator 2"], "and the names stay whole");
}

// A rail that goes on shows it (an arrow box at the cut end), scrolls to keep the cursor's tab in view, and the dial
// walks the pages, then the tabs, then the tiles, and a press opens (never the move).
#[test]
fn a_rail_that_overflows_scrolls_and_the_dial_walks_pages_then_tabs_then_tiles() {
    use crate::nav::Action;
    let t = &all_themes()[0];
    let surface = t.data.ink("surface");
    // The arrow boxes are at the cut ends of the tabs, under the heading ("Operator", two lines of the small type tall).
    let more = |s: &Screen, r: Box4, last: bool| -> usize {
        let th = s.ui.global::<Theme>();
        let (m, head) = ((0.6 * th.get_touch()) as usize, (2. * th.get_small()) as usize);
        let b = if last { (r.0, r.3 - m + 2, r.2, r.3) } else { (r.0, r.1 + head, r.2, r.1 + head + m - 2) };
        count(s, b, surface) * 100 / ((b.2 - b.0) * (b.3 - b.1)).max(1)
    };
    // Fourteen operators: 14 touch targets are more than the rail holds at 800 x 480.
    let (mut s, mut p) = placed(&operators(14), (800, 480), 1., t, TabStyle::Segmented, Side);
    let (_, col) = rails(&s, 1.);
    assert_eq!(s.ui.global::<GridState>().get_tabs().row_count(), 14);
    let (top, bottom) = (more(&s, col, false), more(&s, col, true));
    println!("tabs rail at the start: {top} % top, {bottom} % bottom are an arrow box");
    assert!(top < 30 && bottom > 70, "the end that goes on says so: {top} {bottom}");
    // The dial: the first tile, back to the tabs (on the one shown), a press, and the cursor on the last tab shows it.
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(on(&s.ui), On::Tile(0));
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!(on(&s.ui), On::Tab(0), "back from the first tile: the tabs");
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!(on(&s.ui), On::Page(0), "back from the tabs: the pages");
    p.act(Action::Move { pane: None, by: 2 });
    p.act(Action::Move { pane: None, by: 13 });
    s.frame();
    assert_eq!(on(&s.ui), On::Tab(13));
    assert_eq!(s.ui.global::<GridState>().get_tab(), 0, "moving opened nothing");
    let (top, bottom) = (more(&s, col, false), more(&s, col, true));
    assert!(top > 70 && bottom < 30, "the cursor's tab is scrolled into view: {top} {bottom}");
    p.act(Action::Confirm(None));
    assert_eq!(s.ui.global::<GridState>().get_tab(), 13, "a press opens");
    // Arrows in the rails: left from the tiles to the tabs, left again to the pages, right back.
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(on(&s.ui), On::Tile(0));
    p.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(on(&s.ui), On::Tab(13), "left from the first column of tiles: the tabs, on the one shown");
    p.act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(on(&s.ui), On::Tab(12), "up and down along the tabs");
    p.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(on(&s.ui), On::Page(0), "left from the tabs: the pages");
    p.act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(on(&s.ui), On::Tab(13), "right from the pages: the tabs, on the one shown");
    p.act(Action::Arrow { dx: 1, dy: 0 });
    assert_eq!(on(&s.ui), On::Tile(0), "right from the tabs: the tiles");
}

// The tile area on the keyboard's screen (826 x 465 logical at 1.55) in the two placements, for a 5-page plugin whose first
// page has 8 tabs, a 2-page plugin with no tabs and the FM fixture: two strips (`top`) take the height of the rows, the two
// rails (`side`) take width and leave the tiles the whole height. Printed (ui/TABS.md has them). With both rails present the
// tiles keep three columns.
#[test]
fn the_placements_on_the_keyboards_screen_leave_the_tiles_their_room() {
    let t = &all_themes()[0];
    for (plugin, list) in plugins() {
        let mut heights = vec![];
        for (label, placement) in [("top", Top), ("side", Side)] {
            let (s, _) = placed(&list, (1280, 720), 1.55, t, TabStyle::Segmented, placement);
            let (w, h) = area(&s);
            let g = s.ui.global::<GridState>();
            println!("{plugin} {label}: tile area {w:.0} x {h:.0} = {:.0} (logical px), {} columns of tiles, rail {:.0}", w * h, columns(&s), g.get_rail());
            heights.push(h);
            assert_eq!(g.get_layout(), if placement == Top { 0 } else { 1 }, "{plugin} {label}");
        }
        // 465 less the bar's 48, the margin above and below.
        assert!((heights[1] - (465. - 48. - 12.)).abs() < 1., "{plugin}: the side placement leaves the tiles the full height: {}", heights[1]);
    }
    // The first plugin has both rails: three columns of tiles at 826 x 465 and at 800 x 480.
    for (size, scale) in [((1280, 720), 1.55f32), ((800, 480), 1.)] {
        let (s, _) = placed(&five_pages(), size, scale, t, TabStyle::Segmented, Side);
        let g = s.ui.global::<GridState>();
        assert!(g.get_tabs().row_count() > 1 && g.get_pages().row_count() > 1, "both rails are present");
        assert_eq!(columns(&s), 3, "{size:?}@{scale}: three columns of tiles with both rails: the area {:?}", area(&s));
    }
}

// A rail is from just under the top bar to the bottom of the window, as one continuous track (read in the pixels: the
// track's color at the rail's left edge on every row from the bar to the window's end), its tabs share that height
// equally (the selected tab's fill, tab by tab, is the same height, and the boxes follow each other), and a rail with a
// heading gives the heading's height first. Both styles, in the dark theme and the odd ones.
#[test]
fn a_rail_is_the_height_of_the_screen_under_the_bar_and_its_tabs_share_it_equally() {
    for t in &all_themes() {
        for (style, label) in [(TabStyle::Folder, "folder"), (TabStyle::Segmented, "segmented")] {
            for (size, scale) in [((1280, 720), 1.55f32), ((800, 480), 1.)] {
                let (mut s, mut p) = placed(&five_pages(), size, scale, t, style, Side);
                let th = s.ui.global::<Theme>();
                let (on, track) = (th.get_tab_on(), if style == TabStyle::Segmented { th.get_tab_off() } else { th.get_surface() });
                let (pages, tabs) = rails(&s, scale);
                // One continuous track: its color at the rail's left edge (x = 1) on every row under the bar.
                for (what, r) in [("pages", pages), ("tabs", tabs)] {
                    let x = r.0 + 1;
                    let rows: Vec<usize> = (r.1..s.h).filter(|&y| (s.pixels[y * s.w + x].r, s.pixels[y * s.w + x].g, s.pixels[y * s.w + x].b) != (track.red(), track.green(), track.blue())).collect();
                    let missing = rows.len();
                    assert_eq!(missing, 0, "rows {:?}..{:?} x {x} {:?} want {:?} rail {} gap {} ", rows.first(), rows.last(), s.pixels[rows.first().copied().unwrap_or(0) * s.w + x], track, s.ui.global::<GridState>().get_rail(), s.ui.global::<Theme>().get_gap());
                    assert_eq!(missing, 0,"{} {label} {what} {size:?}@{scale}: {missing} rows without the rail's track", t.id);
                }
                // Equal tabs: the page rail has five tabs; the fill of each, selected in turn, is as tall as the others'.
                let logical = (size.1 as f32 / scale) - 48.;
                let item = logical / 5.;
                let mut tops = vec![];
                for k in 0..5 {
                    p.show_page(k);
                    s.frame();
                    let xmid = (pages.0 + pages.2) / 2;
                    let ys: Vec<usize> = (pages.1..s.h).filter(|&y| (s.pixels[y * s.w + xmid].r, s.pixels[y * s.w + xmid].g, s.pixels[y * s.w + xmid].b) == (on.red(), on.green(), on.blue())).collect();
                    let (first, last) = (*ys.first().unwrap_or(&0), *ys.last().unwrap_or(&0));
                    tops.push((first, last - first + 1));
                }
                let heights: Vec<usize> = tops.iter().map(|t| t.1).collect();
                let spread = heights.iter().max().unwrap() - heights.iter().min().unwrap();
                assert!(spread <= 2, "{} {label} {size:?}@{scale}: the tabs' fills are {heights:?} tall", t.id);
                let step: Vec<f32> = tops.windows(2).map(|w| (w[1].0 as f32 - w[0].0 as f32) / scale).collect();
                for d in &step {
                    assert!((d - item).abs() <= 1.5, "{} {label} {size:?}@{scale}: the tabs are {item:.1} apart, not {step:?}", t.id);
                }
            }
        }
    }
}

// A rail's width is the theme's, not its longest name's: four of the small type and two narrow paddings (68 logical px at
// the touch scale), at most 12% of the screen's width; it grows with the font scale and the spacing; a page with no tabs has no
// tabs' rail, and a plugin with one page has no rails.
#[test]
fn a_rails_width_is_the_themes_and_at_most_twelve_percent_of_the_screen() {
    let t = &all_themes()[0];
    let (s, _) = placed(&five_pages(), (1280, 720), 1.55, t, TabStyle::Segmented, Side);
    let th = s.ui.global::<Theme>();
    let rail = s.ui.global::<GridState>().get_rail();
    assert!((rail - (5. * th.get_small() + 1.5 * th.get_pad_tight())).abs() < 0.01, "{rail}");
    assert!((56. ..=72.).contains(&rail), "a rail at the keyboard's scale is 56 to 72: {rail}");
    // The same width for a rail of short names and of long ones.
    let (s, _) = placed(&long_names(), (1280, 720), 1.55, t, TabStyle::Segmented, Side);
    assert_eq!(s.ui.global::<GridState>().get_rail(), rail, "not by the longest name");
    // Never more than 12% of the width: a narrow screen's rail is cut to its share.
    for (w, scale) in [(1280, 1.55f32), (800, 1.), (640, 1.), (400, 1.), (1280, 1.)] {
        let (s, _) = placed(&five_pages(), (w, 720), scale, t, TabStyle::Segmented, Side);
        let (g, th) = (s.ui.global::<GridState>(), s.ui.global::<Theme>());
        let logical = w as f32 / scale;
        let r = g.get_rail();
        println!("{logical:.0} across: rail {r:.1}, at most {:.1}", 0.12 * logical);
        assert!(r <= th.get_side_share() * logical + 0.01 && (r - th.get_rail_width().min(0.12 * logical)).abs() < 0.01, "{logical}: {r}");
    }
    // It scales with the spacing and the font scale.
    let (s, _) = placed(&five_pages(), (1280, 720), 1., t, TabStyle::Segmented, Side);
    let base = s.ui.global::<GridState>().get_rail();
    for (spacing, font) in [(1.6f32, 1.0f32), (1.0, 1.4), (0.6, 0.8)] {
        fit_to_window();
        let mut s = Screen::sized(1280, 720);
        theme::apply(&s.ui, t);
        s.ui.global::<Theme>().set_spacing(spacing);
        s.ui.global::<Theme>().set_font_scale(font);
        s.ui.global::<Theme>().set_tab_placement(Side);
        let list = five_pages();
        let mut screen = open_with(&mut s, list.clone(), &values_of(&list), &percent);
        s.frame();
        screen.shown();
        s.frame();
        let r = s.ui.global::<GridState>().get_rail();
        let want = 5. * 13. * font + 1.5 * 4. * spacing;
        println!("spacing {spacing}, font scale {font}: rail {r:.1} (the touch scale {base:.1})");
        assert!((r - want).abs() < 0.01, "{r} for {want}");
    }
    // Two pages and no tabs: one rail, the content right after it; one page: no rail.
    let (s, _) = placed(&super::grid::every_kind(), (1280, 720), 1.55, t, TabStyle::Segmented, Side);
    let g = s.ui.global::<GridState>();
    assert!(g.get_tabs().row_count() <= 1 && area(&s).0 > 826. - 68. - 3. * 6., "{:?}", area(&s));
    let (s, _) = placed(&super::fm::fm_n(&[super::fm::parallel()]), (1280, 720), 1.55, t, TabStyle::Segmented, Side);
    assert!((area(&s).0 - (826. - 12.)).abs() < 0.5, "no rail: the content has the width: {}", area(&s).0);
}

// The text of a rail is chosen per rail: the small type where every label fits the rail's width (on one line or two), else all
// of the rail's labels shrunk by the same least amount that fits them, at most to 70%, then cut. Rotated text is not drawn (the
// software renderer does not rotate it: ui/TABS.md). Read from the factor each rail reports; printed for the fixtures at 826 x 465.
#[test]
fn a_rails_text_is_the_small_type_where_it_fits_else_shrunk_alike_down_to_seventy_percent() {
    let t = &all_themes()[0];
    let fits = |list: &serde_json::Value| {
        let (s, _) = placed(list, (1280, 720), 1.55, t, TabStyle::Segmented, Side);
        let g = s.ui.global::<GridState>();
        (g.get_page_fit(), g.get_tab_fit())
    };
    let (pages, tabs) = fits(&five_pages());
    println!("five pages: the pages' rail {pages:.2}, the tabs' rail {tabs:.2}");
    assert_eq!(tabs, 1., "the digits of the operators");
    assert!(pages >= 0.85, "Operators, Filter, Amp, Voicing, Global: the small type, or all but: {pages}");
    let (pages, tabs) = fits(&long_names());
    println!("long names: the pages' rail {pages:.2}, the tabs' rail {tabs:.2}");
    assert!(pages >= 0.85, "Sound, Output: the small type, or all but, with the padding each side: {pages}");
    assert!((0.7..1.).contains(&tabs),"Modulation Matrix, Voice Shaping, Macro Controls, Global: shrunk alike, not below 70%: {tabs}");
    let (pages, _) = fits(&named(&["A page named for everything it holds, and then some", "Filter"]));
    assert_eq!(pages, 0.7, "a name that does not fit even then is shrunk to 70% and cut");
    let (pages, _) = fits(&super::grid::every_kind());
    println!("two pages: the pages' rail {pages:.2}");
    assert_eq!(pages, 1., "Voice, Output");
}

// `auto` is the placement that leaves the content the larger area at the size at hand: the same plugin is placed both ways
// and `auto` must be the one with the larger area (two strips where they are equal). Printed for the three fixtures at 826 x 465,
// at 1280 x 720 at scale 1 and at 800 x 480.
#[test]
fn auto_is_the_placement_with_the_larger_tile_area() {
    let t = &all_themes()[0];
    for (plugin, list) in plugins() {
        for (size, scale) in [((1280, 720), 1.55f32), ((1280, 720), 1.), ((800, 480), 1.)] {
            let area_of = |place| {
                let (s, _) = placed(&list, size, scale, t, TabStyle::Segmented, place);
                let (w, h) = area(&s);
                (w * h, s.ui.global::<GridState>().get_layout(), columns(&s))
            };
            let (top, side, auto) = (area_of(Top), area_of(Side), area_of(Auto));
            println!(
                "{plugin} {}x{}@{scale}: top {:.0} ({} columns), side {:.0} ({} columns), auto {} with {:.0}",
                size.0,
                size.1,
                top.0,
                top.2,
                side.0,
                side.2,
                if auto.1 == 0 { "top" } else { "side" },
                auto.0
            );
            let want = if side.0 > top.0 { 1 } else { 0 };
            assert_eq!(auto.1, want, "{plugin} {size:?}@{scale}: top {} side {}", top.0, side.0);
            assert!((auto.0 - top.0.max(side.0)).abs() < 1., "{plugin} {size:?}@{scale}");
        }
    }
}
