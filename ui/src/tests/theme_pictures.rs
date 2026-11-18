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

fn blue() -> theme::Loaded {
    blue_square_theme()
}

fn orange() -> theme::Loaded {
    theme::chosen(&theme::Places::default(), "oscairn-dark", "")
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
