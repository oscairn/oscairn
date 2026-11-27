// SPDX-License-Identifier: AGPL-3.0-or-later
//! A vertical tab row (VTabs, Play's knob column) and a rail: every tab is the whole height of its row, and a row of
//! folder tabs sits the theme's gap apart (a segmented one, none) — measured in the drawn pixels in every theme at 1.55
//! and 1. A strip of the screen between two tabs is the designed gap, so no assertion says the gap is absent.

use super::*;
use crate::tests::tabs::{five_pages, placed, rails};
use crate::{GridState, TabPlacement, TabStyle, Theme};

/// The runs of (is one of the screen's empty colors, surface or background) down the column `x` between rows `a` and
/// `b`; a folder gap is such a run, so is the screen around a column.
fn void_runs(s: &Screen, x: usize, a: usize, b: usize, empty: &[(u8, u8, u8); 2]) -> Vec<(bool, usize)> {
    fn void(p: &Rgb8Pixel, empty: &[(u8, u8, u8); 2]) -> bool {
        (p.r, p.g, p.b) == empty[0] || (p.r, p.g, p.b) == empty[1]
    }
    let mut out: Vec<(bool, usize)> = Vec::new();
    let mut i = a;
    while i < b {
        let surf = void(&s.pixels[i * s.w + x], empty);
        let mut j = i + 1;
        while j < b && void(&s.pixels[j * s.w + x], empty) == surf {
            j += 1;
        }
        out.push((surf, j - i));
        i = j;
    }
    out
}

/// A rail's drawn heights and the gaps between them in a theme and scale, from the side placement's pages rail (the
/// tab a full row tall, neighbor tabs `Theme.gap` apart in folder, contiguous in segmented) and Play's knob column, a
/// VTabs.
#[test]
fn a_vertical_tab_row_and_a_rail_are_tall_and_the_gap_between_them_is_the_themes() {
    fit_to_window();
    let mut wrong = Vec::new();
    for t in &all_themes() {
        for scale in [1.55f32, 1.] {
            // The pages rail of the side placement: five tabs to the screen's height, no heading.
            let (s, _p) = placed(&five_pages(), (1280, 720), scale, t, TabStyle::Folder, TabPlacement::Side);
            let th = s.ui.global::<Theme>();
            let segmented = th.get_tabs() == TabStyle::Segmented;
            let gap = if segmented { 0. } else { th.get_gap() };
            let surface = th.get_surface();
            let background = th.get_background();
            let empty = [(surface.red(), surface.green(), surface.blue()), (background.red(), background.green(), background.blue())];
            let (pages, _tabs) = rails(&s, scale);
            let n = s.ui.global::<GridState>().get_pages().row_count() as f32;
            // Just inside the fills' right edge, past the centered label's own text.
            let x = (pages.2 - 6).clamp(pages.0 + 1, pages.2.saturating_sub(1));
            let runs = void_runs(&s, x, pages.1, s.h, &empty);
            // The rail fills its height: the first and last rows are a tab (no void band at top or bottom).
            let gaps: Vec<usize> = runs.iter().filter(|(v, _)| *v).map(|(_, l)| *l).collect();
            let fills: Vec<usize> = runs.iter().filter(|(v, _)| !*v).map(|(_, l)| *l).collect();
            let room = (s.h - pages.1) as f32 / scale;
            let want_item = (room - (n - 1.) * gap) / n;
            let what = format!("{} rail at {scale}: runs {runs:?}, gaps {gaps:?}, fills {fills:?} (want item {want_item:.1} gap {gap:.1})", t.id);
            if segmented {
                // Tabs and their track are one continuous run; no void gap between them.
                if !gaps.is_empty() {
                    wrong.push(format!("{} rail segmented has {} gaps", t.id, gaps.len()));
                }
                if fills.len() != 1 {
                    wrong.push(format!("{} rail segmented: {} fill runs, want one track", t.id, fills.len()));
                }
            } else if gaps.len() != (n - 1.) as usize || fills.len() != n as usize {
                wrong.push(format!("{} rail folder: {} gaps {} fills for {n} tabs", t.id, gaps.len(), fills.len()));
            }
            // The void gaps between tabs are the theme's gap; each tab is its share of the room (a touch target at least).
            let mut ok = !segmented || (gaps.is_empty() && fills.len() == 1);
            if ok {
                for g in &gaps {
                    if ((*g as f32 / scale) - gap).abs() > 2.5 {
                        ok = false;
                        wrong.push(format!("{} rail: a gap of {} px is not the theme's {gap:.1}", t.id, g));
                    }
                }
                for f in &fills {
                    if ((*f as f32 / scale) - want_item).abs() > 3. {
                        ok = false;
                        wrong.push(format!("{} rail: a tab {} px tall, want {want_item:.1}", t.id, f));
                    }
                }
            }
            if !ok {
                // (the what line already carries the numbers)
                wrong.push(what);
            }

            // Play's knob column, a VTabs: two tabs (the Performance's controls and the selected part's).
            let mut s2 = Screen::sized(1280, 720);
            s2.set_scale(scale);
            theme::apply(&s2.ui, t);
            let ui = s2.ui.clone_strong();
            let mut play = crate::play::Screen::new(&ui);
            play.graph(&play_graph());
            ui.set_screen(3);
            play.knobs(vec![]); // the knob row, whatever the device's groups
            play.tap_open(0); // a part chosen: the column has the Performance's controls and the part's own
            play.shown();
            s2.frame();
            s2.frame();
            let th = s2.ui.global::<Theme>();
            let segmented = th.get_tabs() == TabStyle::Segmented;
            let gap = if segmented { 0. } else { th.get_gap() };
            let want_item = th.get_touch();
            let logical_w = s2.w as f32 / scale;
            // The knob column's place: a `button-width` column at the right, one margin from the edge. Its fills are
            // scanned just inside the right edge, past the centered label's own text.
            let col_left = ((logical_w - th.get_margin() - th.get_button_width()) as f64) as usize;
            let col_w = th.get_button_width();
            let x = (col_left as f32 + (col_w - 4.)) * scale;
            let runs = void_runs(&s2, x.round() as usize, 0, s2.h, &empty);
            // A pair of tabs: every void run, measured with the fills around it. The column's two tabs are a touch
            // target tall each and, in folder, sit the theme's gap apart (contiguous in a segmented style).
            let triples: Vec<(usize, usize, usize)> = runs.windows(3)
                .filter(|w| !w[0].0 && w[1].0 && !w[2].0 && pos(&w[0].1, want_item, scale) && pos(&w[2].1, want_item, scale))
                .map(|w| (w[0].1, w[1].1, w[2].1)).collect();
            let what = format!("{} knob column at {scale}: runs {runs:?} (want item {want_item:.1} gap {gap:.1})", t.id);
            let ok = if segmented {
                let contiguous = runs.windows(1).any(|w| !w[0].0 && pos(&w[0].1, want_item * 2., scale));
                contiguous && triples.is_empty() // two tabs touch, no gap between them
            } else {
                triples.len() == 1 && ((triples[0].1 as f32 / scale) - gap).abs() <= 3.
            };
            if !ok {
                wrong.push(what);
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// One run is a touch target tall in the scale's pixels (a knob-column tab), within 3 px.
fn pos(px: &usize, touch: f32, scale: f32) -> bool {
    (*px as f32).abs() >= (touch * scale - 3. * scale) && (*px as f32) <= (touch * scale + 3. * scale)
}
