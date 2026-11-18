// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a Library row carries at its trailing end and its leading edge: the warning icon of a Performance that does not fit
//! this device, the star of a favorite, the item's own color (the owner's, 2026-11-17).

use super::library::rows;
use super::owner::{lib_png, scene};
use super::*;

/// The pixels within `near` (summed squared channel distance) of `color` in the band `y0..y1` between `x0..x1`.
fn count(s: &Shelf, color: slint::Color, (x0, x1, y0, y1): (usize, usize, usize, usize), near: i32) -> usize {
    let w = s.f.screen.w;
    let c = (i32::from(color.red()), i32::from(color.green()), i32::from(color.blue()));
    (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| {
        let p = s.f.screen.pixels[y * w + x];
        (i32::from(p.r) - c.0).pow(2) + (i32::from(p.g) - c.1).pow(2) + (i32::from(p.b) - c.2).pow(2) <= near
    }).count()
}

use super::library_shell::Shelf;

/// The band of row `i` of the list at 1280x720 and 1.55, in pixels: from its left edge to the window's right, its height.
fn row_band(s: &Shelf, i: usize) -> (usize, usize, usize, usize) {
    let ui = s.ui();
    let t = ui.global::<crate::Theme>();
    let (x, y) = ((ui.get_list_x() + t.get_margin()) * 1.55, (ui.get_list_y() + t.get_margin() + i as f32 * (t.get_touch() + t.get_gap())) * 1.55);
    (x as usize, 1250, y as usize, (y + t.get_touch() * 1.55) as usize)
}

// A Performance the engine says does not fit this device has a small warning icon in the row's trailing end, before the star;
// the reason is the row's (the panel, the line under the list and the row's accessible label say it), and the row's value no
// longer says it in words. A row that fits has none.
#[test]
fn a_row_that_does_not_fit_has_a_warning_icon_before_the_star() {
    let mut s = scene("rows-warn", 10);
    s.e.items[1].unfit = "needs 3 GB of memory".into();
    s.e.items[1].favorite = true;
    s.e.items[2].favorite = true;
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    let ui = s.ui();
    let rows = rows(&ui);
    assert!(rows[1].warn && rows[1].star && rows[1].reason == "needs 3 GB of memory", "{:?}", (rows[1].warn, rows[1].star));
    assert!(!rows[2].warn && rows[2].star && !rows[0].warn && !rows[0].star);
    assert!(!rows[1].value.contains("Does not fit"), "the icon says it, not the value: {}", rows[1].value);
    let warning = ui.global::<crate::Theme>().get_warning();
    let (with, without) = (count(&s, warning, row_band(&s, 1), 1800), count(&s, warning, row_band(&s, 2), 1800));
    assert!(with > 20 && without == 0, "the icon is drawn in the warning color on the unfit row only: {with} {without}");
    lib_png(&s, "lib-E-warning");
}
