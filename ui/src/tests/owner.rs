// SPDX-License-Identifier: AGPL-3.0-or-later
//! The owner's first fixes (2026-11-17): the top bar's More, the Library row's name, the rail. Each test reads the
//! Library as the owner's screenshot shows it (Performances, 1280x720 at 1.55, dark); with OWNER_SHOTS=dir the pictures
//! are written to dir as owner-N-before/after.png.

use super::library_fake::stored;
use super::library_shell::Shelf;
use super::*;

pub(super) const LONG: &str = "A Very Long Performance Name For Testing 1";

/// The Library's Performances as the screenshot has them: the long name first, two tags each.
pub(super) fn scene(name: &str, n: usize) -> Shelf {
    fit_to_window();
    let mut s = Shelf::new(name);
    let ui = s.ui();
    shell::library_in_rust(true);
    s.f.screen.resize(1280, 720);
    s.f.screen.set_scale(1.55);
    theme::apply(&ui, &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
    s.e.items = (0..n)
        .map(|i| {
            let nm = if i == 0 { LONG.to_string() } else { format!("Bright Strings {:03}", i + 2) };
            let it = stored("performance", &format!("p{i}"), &nm).tags(&[["keys", "brass", "pad"][i % 3], ["studio", "live"][i % 2]]);
            if i == 1 { it.described("A long description that does not fit on one line of the list however wide it is, to see it cut") } else { it }
        })
        .collect();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    s
}

/// The picture, when OWNER_SHOTS names a folder.
pub(super) fn owner_png(s: &Shelf, name: &str) {
    if let Ok(dir) = std::env::var("OWNER_SHOTS") {
        let tag = std::env::var("OWNER_TAG").unwrap_or_else(|_| "after".into());
        png_of(&dir, &s.f.screen.pixels, s.f.screen.w, s.f.screen.h, &format!("{name}-{tag}"));
    }
}

// More is an icon at the right end of the action area, directly left of the meters, and gives its width to the title.
#[test]
fn more_is_an_icon_left_of_the_meters() {
    let mut s = scene("owner-more", 10);
    let ui = s.ui();
    // The chip's room: how much of the name shows.
    ui.global::<Bar>().set_loading(format!("Loading {LONG}").into());
    s.f.screen.frame();
    s.f.screen.frame();
    owner_png(&s, "owner-1");
    let pw = ui.global::<crate::Theme>().get_pad_wide();
    let (chip, full) = (ui.get_loading_width(), ui.get_loading_full());
    let shown = (format!("Loading {LONG}").chars().count() as f32 * (chip - 2. * pw) / (full - 2. * pw)).floor();
    eprintln!("OWNER1 chip {chip} of {full}, about {shown} characters");
    assert!(ui.global::<Bar>().get_more(), "folded at this width");
    let (mx, mw) = (ui.get_more_x(), ui.get_more_width());
    assert!((mw - 36.).abs() < 0.5, "an icon button's width, not the word's: {mw}");
    assert!(mx >= ui.get_tempo_x() + ui.get_tempo_w() - 0.5, "right of the tempo: {mx}");
    assert!((ui.get_resources_x() - (mx + mw)).abs() < 12., "directly left of the meters: {} {}", mx + mw, ui.get_resources_x());
    assert!(chip <= ui.get_pill_width() / 2. + 0.5);
    // Half of the title area, which is wider by what More gave up (a word and its padding, 62, for an icon, 36): 193 before.
    assert!(chip > 198., "the chip: {chip}");
}
