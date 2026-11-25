// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's row of tabs (Performances, Sounds, Setlists) beside its search box, measured in the drawn pixels, in every
//! theme at the keyboard's scale and at 1: each tab as tall as the search box, its top and bottom on the box's, the selected
//! one no taller than the others, the row's left edge on the sidebar's. Pictures in target/screens/library-tabs-THEME-SCALE.png.

use super::library_fake::stored;
use super::library_shell::Shelf;
use super::*;

/// The drawn boxes of the row under the top bar, left to right, as (left, right, top, bottom) in physical pixels (bottom and
/// right inclusive): runs of columns whose pixels other than the background span at least half the search box, the search box
/// last; and the sidebar's left edge.
fn row_boxes(s: &Screen) -> (Vec<(usize, usize, usize, usize)>, usize) {
    let theme = s.ui.global::<crate::Theme>();
    let back = theme.get_background();
    let k = s.scale;
    let bg = |x: usize, y: usize| {
        let p = s.pixels[y * s.w + x];
        (p.r, p.g, p.b) == (back.red(), back.green(), back.blue())
    };
    let from = (theme.get_bar_height() * k) as usize + 1;
    let xs = s.w * 6 / 10; // inside the search box in every theme and window
    let top = (from..s.h).find(|&y| !bg(xs, y)).unwrap();
    let span = (theme.get_touch() * k).round() as usize;
    let (y0, y1) = (from.max(top.saturating_sub((theme.get_margin() * k) as usize)), top + span + 1);
    let extent = |x: usize| {
        let ys: Vec<usize> = (y0..y1).filter(|&y| !bg(x, y)).collect();
        ys.first().map(|&t| (t, *ys.last().unwrap()))
    };
    let mut boxes: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut open = false;
    for x in 0..=xs {
        match extent(x).filter(|(t, b)| b - t >= span / 2) {
            Some((t, b)) if open => {
                let last = boxes.last_mut().unwrap();
                *last = (last.0, x, last.2.min(t), last.3.max(b));
            }
            Some((t, b)) => {
                boxes.push((x, x, t, b));
                open = true;
            }
            None => open = false,
        }
    }
    let probe = boxes.last().unwrap().3 + span / 2;
    let side = (0..xs).find(|&x| !bg(x, probe)).unwrap();
    (boxes, side)
}

#[test]
fn the_librarys_tabs_are_the_search_boxs_height_and_line_up_with_it_and_the_sidebar() {
    fit_to_window();
    let mut wrong = Vec::new();
    for theme in all_themes() {
        for scale in [1.55f32, 1.] {
            let mut s = Shelf::new(&format!("lt-{}-{scale}", theme.id));
            let ui = s.ui();
            shell::library_in_rust(true);
            s.f.screen.resize(1280, 720);
            s.f.screen.set_scale(scale);
            theme::apply(&ui, &theme);
            s.e.items = (0..8).map(|i| stored("performance", &format!("p{i}"), &format!("Warm Strings {i:03}"))).collect();
            shell::goto(shell::Screen::Home);
            shell::goto(shell::Screen::Library);
            s.serve();
            s.f.screen.frame();
            s.f.screen.frame();
            png(&s.f.screen, &format!("library-tabs-{}-{scale}", theme.id));
            let (boxes, side) = row_boxes(&s.f.screen);
            let (search, tabs) = boxes.split_last().unwrap();
            let what = format!("{} at {scale}: tabs {tabs:?}, search {search:?}, sidebar at {side}", theme.id);
            let folder = ui.global::<crate::Theme>().get_tabs() == crate::TabStyle::Folder;
            // Segmented: the track is the row, the selected tab lifted out of it inside; folder: a box for each tab.
            if tabs.len() != if folder { 3 } else { 1 } {
                wrong.push(format!("{what}: {} boxes", tabs.len()));
            }
            if tabs.iter().any(|t| t.2.abs_diff(search.2) > 1 || t.3.abs_diff(search.3) > 1) {
                wrong.push(format!("{what}: a tab's top or bottom is not the search box's"));
            }
            if tabs[0].0.abs_diff(side) > 1 {
                wrong.push(format!("{what}: the row's left edge is not the sidebar's"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
