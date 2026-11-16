// SPDX-License-Identifier: AGPL-3.0-or-later
//! Each operator's line in the FM view's graph is drawn where its path is, measured in the rendered pixels as
//! the other views' lines are (tests/shots.rs `line_offsets`): the pixel theme's gray panels and an operator's
//! own color (the palette from that theme's green accent, dimmed for an operator at level 0), at two window
//! sizes and scales. The window's panel is found to a whole pixel, so the fractional 1.55 is not measured here.

use super::fm::*;
use super::shots::*;
use super::views::{fm8_plugin, fm8_text, fm8_values};
use super::*;

#[test]
fn every_operators_line_is_drawn_where_its_path_is() {
    use crate::palette::operators;
    let theme = super::envelope::pixel_theme();
    let c = |n: &str| theme.data.ink(n);
    let colors = operators(c("accent"), c("surface"), c("background"), c("text"));
    let ground = [f64::from(c("surface").red()), f64::from(c("surface").green()), f64::from(c("surface").blue())];
    let rgb = |n: usize| [f64::from(colors[n].fill.red()), f64::from(colors[n].fill.green()), f64::from(colors[n].fill.blue())];
    let pages: [Page; 3] = [
        ("eight operators", fm8_plugin(), fm8_values(0.), fm8_text),
        ("parallel, the last off", fm_n(&[parallel()]), fm_n_values(0., true), fm_n_text),
        ("chain", fm_n(&[chain()]), fm_n_values(0., false), fm_n_text),
    ];
    let mut report = String::new();
    let mut checked = 0;
    for (name, list, values, text) in &pages {
        for (size, scale) in [((1280, 720), 1.), ((1920, 1080), 2.), ((1920, 1080), 1.)] {
            let (mut shot, _p) = view_shot(list, values, text, 0, size, scale);
            // The graph's lines: operator-colored paths after the graph's frame.
            let frame = shot.panel.shapes.iter().position(|s| s.ink == 5).expect("the frame");
            let lines: Vec<(usize, String)> = shot.panel.shapes.iter().skip(frame).filter(|s| (16..24).contains(&s.ink) || (32..40).contains(&s.ink)).filter(|s| !s.fill).map(|s| ((s.ink as usize - 16) % 16, s.commands.to_string())).collect();
            assert_eq!(lines.len(), 8, "a line for each operator in {name}");
            let mut measured = 0;
            for (n, path) in &lines {
                let dim = shot.panel.shapes.iter().skip(frame).any(|s| s.ink == 32 + *n as i32 && !s.fill);
                let (color, ground) = (rgb(*n), ground);
                let shown = if dim { [0, 1, 2].map(|i| ground[i] + 0.45 * (color[i] - ground[i])) } else { color };
                shot.tint = Some((shown, ground));
                // Where another line runs within 9 px the measure is not asked (as under a handle): a crossing or a
                // near neighbor repaints the line's edge pixels. Those places are given as handles.
                let mut near: Vec<crate::GridPoint> = shot.panel.points.iter().collect();
                for (_, other) in lines.iter().filter(|(m, _)| m != n) {
                    let v = super::envelope::vertices(other);
                    for pair in v.windows(2) {
                        let len = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
                        let steps = (len / 3.).ceil().max(1.) as usize;
                        for k in 0..=steps {
                            let t = k as f64 / steps as f64;
                            near.push(crate::GridPoint { x: (pair[0].0 + (pair[1].0 - pair[0].0) * t) as f32, y: (pair[0].1 + (pair[1].1 - pair[0].1) * t) as f32, lit: false, held: false });
                        }
                    }
                }
                // The tags' text is drawn over the lines: not measured under it either.
                for l in shot.panel.labels.iter().filter(|l| (16..40).contains(&l.ink)) {
                    let (mut y, mut x) = (l.y, l.x);
                    while y <= l.y + l.h {
                        x = l.x;
                        while x <= l.x + l.w {
                            near.push(crate::GridPoint { x, y, lit: false, held: false });
                            x += 3.;
                        }
                        y += 3.;
                    }
                    let _ = x;
                }
                let kept = shot.panel.clone();
                shot.panel.points = slint::ModelRc::new(slint::VecModel::from(near));
                let (count, off, at) = line_offsets(&shot, path, 2.);
                shot.panel = kept;
                report += &format!("{name} {size:?}@{scale}: operator {} {count} places, worst {off:.2} px (at {at:.1?})\n", n + 1);
                if count > 0 {
                    checked += count;
                    measured += 1;
                    if off > 0.6 {
                        super::png(&shot.s, "failed-fm-line");
                    }
                    assert!(off <= 0.6, "operator {}'s line is off its path by up to {off:.2} px:\n{report}", n + 1);
                }
            }
            assert!(measured >= 4, "at least four lines were measured in {name} at {size:?}@{scale}:\n{report}");
        }
    }
    // A path a pixel lower is seen as off, so the measure can fail.
    let (mut shot, _p) = view_shot(&pages[0].1, &pages[0].2, &pages[0].3, 0, (1280, 720), 1.);
    let frame = shot.panel.shapes.iter().position(|s| s.ink == 5).unwrap();
    let path = shot.panel.shapes.iter().skip(frame).find(|s| s.ink == 17).unwrap().commands.to_string();
    shot.tint = Some((rgb(1), ground));
    let lower: Vec<(f32, f32)> = super::envelope::vertices(&path).into_iter().map(|(x, y)| (x as f32, y as f32 + 1.)).collect();
    let (count, wrong, _) = line_offsets(&shot, &crate::views::poly(&lower), 2.);
    assert!(count > 0 && wrong > 0.7, "a path a pixel off is seen as {wrong:.2} px off");
    println!("{report}{checked} places measured");
}
