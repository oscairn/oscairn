// SPDX-License-Identifier: AGPL-3.0-or-later
//! The same screens in two themes side by side (the owner's, 2026-11-17): the current one (dark, orange accent, rounded) and a
//! light one with a blue accent, square edges and the list's items joined with thin dividers and no gaps. The second is a
//! theme as a file would define it (`theme_of`), selectable in Settings > Appearance once it is a folder with a theme.json
//! (the built-in ones live in hosts/standalone/themes, outside this program). Pictures: theme-orange-library, theme-blue-library,
//! theme-orange-play, theme-blue-play.

use super::bar::play_on_the_shell;
use super::library_shell::Shelf;
use super::owner::{lib_png, scene};
use super::*;

/// Light, a blue accent, square: radius 0, square buttons, flat, sections (and so a list's items) joined edge to edge.
pub fn blue_square_theme() -> theme::Loaded {
    theme_of(json!({"version": 7, "id": "blue-square", "name": "Blue square", "radius": 0,
        "font": {"regular": "fonts/Inter-Regular.ttf", "license": "fonts/OFL.txt"},
        "colors": {"background": "#F3F5F9", "surface": "#FFFFFF", "raised": "#E2E7EF", "border": "#AEB8C6", "text": "#101722", "muted": "#4A5565", "accent": "#1F5FD1",
                   "positive": "#0A7A55", "danger": "#C42D2D", "warning": "#9A5B00"},
        "style": {"buttons": "rounded", "edges": "hairline", "depth": "flat", "sliders": "line", "tabs": "segmented", "marks": "square", "sections": "joined", "rows": "left"},
        "wires": {"audio": "#2F6FD0", "notes": "#7B4BD6", "disabled": "#9AA3AD", "draining": "#B85A0A", "unconnected": "#C42D2D", "control": "#00805A"}}))
}

pub(super) fn blue() -> theme::Loaded {
    blue_square_theme()
}

pub(super) fn orange() -> theme::Loaded {
    theme::chosen(&theme::Places::default(), "oscairn-dark", "")
}

/// Rounded corners with rectangular buttons and the same radius as the default: the other button style, for the scaled radius.
fn rounded_theme() -> theme::Loaded {
    theme_of(json!({"version": 7, "id": "rounded-short", "name": "Rounded", "radius": 6,
        "font": {"regular": "fonts/Inter-Regular.ttf", "license": "fonts/OFL.txt"},
        "colors": {"background": "#111317", "surface": "#1D2127", "raised": "#2D333B", "border": "#4A535E", "text": "#F6F8FA", "muted": "#B4BDC8",
                   "accent": "#FF9A45", "positive": "#5AD9A4", "danger": "#FF6B63", "warning": "#e8c547"},
        "style": {"buttons": "rounded", "edges": "hairline", "depth": "soft", "sliders": "line", "tabs": "segmented", "marks": "square", "sections": "separate", "rows": "left"},
        "wires": {"audio": "#8CC2FF", "notes": "#C29CFF", "disabled": "#6B7480", "draining": "#FF9A45", "unconnected": "#FF6B63", "control": "#6FE0A8"}}))
}

// The list's style tokens follow the theme's sections until a theme says otherwise, so the default look is as it was: separate
// cards (a gap, corners, no line) in the current theme; no gap, square corners and a line in the blue one, in which a row's
// corner pixel is drawn (not the background showing through) where in the current one it is the background's.
#[test]
fn the_list_tokens_follow_the_theme_and_the_default_is_unchanged() {
    let mut s = scene("list-tokens", 10);
    let ui = s.ui();
    let t = ui.global::<crate::Theme>();
    assert_eq!((t.get_list_gap(), t.get_list_radius(), t.get_list_divider()), (t.get_gap(), t.get_section_radius(), false));
    let corner = |s: &Shelf| {
        let ui = s.ui();
        let t = ui.global::<crate::Theme>();
        let (x, y) = (((ui.get_list_x() + t.get_margin()) * 1.55) as usize, ((ui.get_list_y() + t.get_margin() + t.get_touch() + t.get_list_gap()) * 1.55) as usize);
        let p = s.f.screen.pixels[y * s.f.screen.w + x];
        ((i32::from(p.r), i32::from(p.g), i32::from(p.b)), t.get_background())
    };
    s.f.screen.frame();
    let ((p, background), _) = (corner(&s), 0);
    let bg = (i32::from(background.red()), i32::from(background.green()), i32::from(background.blue()));
    assert_eq!(p, bg, "rounded: the background shows at a row's corner");
    theme::apply(&ui, &blue());
    s.f.screen.frame();
    s.f.screen.frame();
    assert_eq!((t.get_list_gap(), t.get_list_radius(), t.get_list_divider()), (0., 0., true));
    let (p, background) = corner(&s);
    let bg = (i32::from(background.red()), i32::from(background.green()), i32::from(background.blue()));
    assert_ne!(p, bg, "square: the row's own corner is drawn");
}

// The Library in both: the row under the bar, the sidebar, the list with a few colored rows and a warning, a star.
#[test]
fn the_library_in_two_themes() {
    for (name, theme) in [("theme-orange-library", orange()), ("theme-blue-library", blue())] {
        let mut s = scene(name, 10);
        let ui = s.ui();
        for (i, hex) in [(1, "#3e63dd"), (2, "#46a758"), (3, "#e5484d")] {
            s.e.items[i].color = hex.into();
        }
        s.e.items[2].unfit = "needs 3 GB of memory".into();
        s.e.items[2].favorite = true;
        theme::apply(&ui, &theme);
        shell::goto(shell::Screen::Home);
        shell::goto(shell::Screen::Library);
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        lib_png(&s, name);
    }
}

// Play in both.
#[test]
fn play_in_two_themes() {
    for (name, theme) in [("theme-orange-play", orange()), ("theme-blue-play", blue())] {
        fit_to_window();
        let mut f = play_on_the_shell(name, json!({"id": "p1", "name": "Grand Piano", "unsaved": false}), play_graph());
        f.screen.resize(1280, 720);
        f.screen.set_scale(1.55);
        theme::apply(&f.screen.ui, &theme);
        f.screen.frame();
        f.screen.frame();
        if let Ok(dir) = std::env::var("OWNER_SHOTS") {
            png_of(&dir, &f.screen.pixels, f.screen.w, f.screen.h, name);
        }
    }
}

/// How far in, in pixels, the top row of the box that holds the pixel column `x` first drawn under `from_y` starts: the box's
/// corner as drawn (0 for a square one). The box's left edge is found a few rows down, on its straight side.
fn corner_run(s: &Shelf, x: usize, from_y: usize) -> usize {
    let (w, px) = (s.f.screen.w, &s.f.screen.pixels);
    let at = |x: usize, y: usize| {
        let p = px[y * w + x];
        (p.r, p.g, p.b)
    };
    let bg = at(3, s.f.screen.h - 3);
    let y0 = (from_y..s.f.screen.h).find(|&y| at(x, y) != bg).expect("a box");
    let y1 = y0 + 24;
    let x0 = (0..=x).rev().find(|&xx| at(xx, y1) == bg).map_or(0, |xx| xx + 1);
    (x0..w).take_while(|&xx| at(xx, y0) == bg).count()
}

// One radius, in both themes: a button, the multi-way (segmented) strip, a list item, the search box and a sidebar entry all
// take the theme's corner (radius + 2, 0 for radius 0), as drawn, within a pixel of each other. The lifted tab in its strip is
// the corner less the strip's padding.
#[test]
fn radii_are_one_family() {
    for (name, theme, square) in [("polish-1-orange", orange(), false), ("polish-1-blue", blue(), true)] {
        let mut s = scene(name, 10);
        let ui = s.ui();
        theme::apply(&ui, &theme);
        s.f.screen.frame();
        s.f.screen.frame();
        let t = ui.global::<crate::Theme>();
        let (r, c) = (t.get_radius(), t.get_corner());
        assert_eq!(c, if r == 0. { 0. } else { r + 2. }, "{name}");
        for h in [30., 44., 56.] {
            assert_eq!((t.invoke_button_radius(h), t.invoke_field_radius(h)), (c, c), "{name}: a button and a field at {h}");
        }
        assert_eq!(t.get_section_radius(), if square { 0. } else { c }, "{name}");
        assert_eq!(t.get_list_radius(), t.get_section_radius(), "{name}");
        assert_eq!(t.invoke_inner(t.get_tab_pad()), (c - t.get_tab_pad()).max(0.), "{name}");
        let k = 1.55;
        // The tab strip, the search box, the info button, a sidebar entry, a list item: where each is under the bar.
        let spots = [("tabs", 40.), ("search", 800. / k), ("info", 1255. / k), ("sidebar", 100.), ("item", 800. / k)];
        let runs: Vec<(&str, usize)> = spots
            .iter()
            .map(|&(n, x)| (n, corner_run(&s, (x * k) as usize, if matches!(n, "sidebar" | "item") { 153 } else { 80 })))
            .collect();
        eprintln!("{name}: corner runs in pixels {runs:?}");
        let (lo, hi) = (runs.iter().map(|r| r.1).min().unwrap(), runs.iter().map(|r| r.1).max().unwrap());
        if r == 0. {
            assert_eq!(hi, 0, "{name}: square everywhere: {runs:?}");
        } else {
            assert!(lo > 2 && hi - lo <= 2, "{name}: one family of corners: {runs:?}");
        }
        lib_png(&s, name);
    }
}

// The scaled radius (D1): a short element's corner is the family's, scaled with its height — full at the touch height and above,
// proportionally smaller below it, never more than half its height, the same in both button styles as buttonRadius treats them,
// and square where the radius is 0. A full-height element keeps exactly what buttonRadius draws.
#[test]
fn scaled_radius_scales_short_elements_and_keeps_full_height() {
    let mut s = scene("scaled-radius", 10);
    let ui = s.ui();
    let t = ui.global::<crate::Theme>();
    for (name, theme) in [("pill", orange()), ("rounded", rounded_theme())] {
        theme::apply(&ui, &theme);
        s.f.screen.frame();
        s.f.screen.frame();
        let (touch, corner) = (t.get_touch(), t.get_corner());
        assert_eq!(t.invoke_scaled_radius(touch), corner, "{name}: full corner at the touch height");
        assert_eq!(t.invoke_scaled_radius(touch * 2.), corner, "{name}: full corner above it");
        assert_eq!(t.invoke_scaled_radius(touch), t.invoke_button_radius(touch), "{name}: a full-height element is unchanged");
        // Three heights under the touch target: the corner scaled with the height (twice the height, twice the corner), never above
        // half the height, never the full corner, the same in both button styles.
        let (h1, h2, h3) = (12., 24., 32.);
        let (r1, r2, r3) = (t.invoke_scaled_radius(h1), t.invoke_scaled_radius(h2), t.invoke_scaled_radius(h3));
        assert_eq!(r1, corner * (h1 / touch), "{name}: proportional at {h1}");
        assert_eq!(r2, corner * (h2 / touch), "{name}: proportional at {h2}");
        assert_eq!(r3, corner * (h3 / touch), "{name}: proportional at {h3}");
        assert!(r3 >= r2 && r2 >= r1, "{name}: more height, more corner");
        assert!(r3 < corner && r3 < touch / 2., "{name}: below the full corner and half the height");
    }
    // A square theme (radius 0): square everywhere, whatever the height.
    theme::apply(&ui, &blue());
    s.f.screen.frame();
    assert_eq!((t.invoke_scaled_radius(12.), t.invoke_scaled_radius(44.), t.invoke_scaled_radius(88.)), (0., 0., 0.), "square stays square");
    lib_png(&s, "polish-1-scaled-radius");
}

// Play in both themes, for the radii (the picture alone).
#[test]
fn polish_1_play_pictures() {
    for (name, theme) in [("polish-1-orange-play", orange()), ("polish-1-blue-play", blue())] {
        fit_to_window();
        let mut f = play_on_the_shell(name, json!({"id": "p1", "name": "Grand Piano", "unsaved": false}), play_graph());
        f.screen.resize(1280, 720);
        f.screen.set_scale(1.55);
        theme::apply(&f.screen.ui, &theme);
        f.screen.frame();
        f.screen.frame();
        if let Ok(dir) = std::env::var("OWNER_SHOTS") {
            png_of(&dir, &f.screen.pixels, f.screen.w, f.screen.h, name);
        }
    }
}
