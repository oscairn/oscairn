// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM view's dots measured in the rendered pixels (the host's side of the live positions; the plugin's
//! side is a proposal). Fixtures in tests/fm.rs.

use super::fm::*;
use super::shots::*;
use super::*;
use crate::fm::point_along;
use crate::views::Along;

/// How much of `color` over `ground` a pixel has (0 to 1), the pixels of another hue none.
fn cover(p: slint::Rgb8Pixel, color: [f64; 3], ground: [f64; 3]) -> f64 {
    let d = [color[0] - ground[0], color[1] - ground[1], color[2] - ground[2]];
    let v = [f64::from(p.r) - ground[0], f64::from(p.g) - ground[1], f64::from(p.b) - ground[2]];
    let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let share = (v[0] * d[0] + v[1] * d[1] + v[2] * d[2]) / len2;
    let off = (0..3).map(|i| (v[i] - share * d[i]).powi(2)).sum::<f64>().sqrt();
    if off > 0.07 * len2.sqrt() {
        0.
    } else {
        share.max(0.)
    }
}

// A dot's center is within 0.75 px of its line: the dot's own pixels are the operator's color the picture
// gains when it is drawn (what it covers of other lines and the nodes is not counted); the centroid of that
// gain is compared with the point along the line the position says. In the pixel theme: gray panels,
// every operator's color derived from a green accent.
#[test]
fn a_dots_centroid_is_on_its_line() {
    use crate::palette::operators;
    let theme = super::envelope::pixel_theme();
    let c = |n: &str| theme.data.ink(n);
    let colors = operators(c("accent"), c("surface"), c("background"), c("text"));
    let rgb = |n: usize| [f64::from(colors[n].fill.red()), f64::from(colors[n].fill.green()), f64::from(colors[n].fill.blue())];
    let ground = [f64::from(c("surface").red()), f64::from(c("surface").green()), f64::from(c("surface").blue())];
    let mut worst = 0f64;
    for (size, scale) in [((1280, 720), 1.), ((1920, 1080), 2.), ((1920, 1080), 1.)] {
        let (mut shot, mut p) = view_shot(&fm_n(&[shared()]), &fm_n_values(0., false), &fm_n_text, 0, size, scale);
        let line = |n: usize| -> Vec<(f32, f32)> {
            let s = shot.panel.shapes.iter().filter(|s| s.ink == 16 + n as i32 && !s.fill).last().expect("the line");
            super::envelope::vertices(&s.commands).into_iter().map(|(x, y)| (x as f32, y as f32)).collect()
        };
        let cases = [(1, Along::Segment(2, 0.5)), (2, Along::Path(0.7)), (3, Along::Segment(3, 0.4)), (4, Along::Path(0.45))];
        let before = shot.s.pixels.clone();
        // One at a time, so the dots' halos do not meet.
        for (n, along) in cases {
            let mut positions = vec![None; 8];
            positions[n] = Some(along);
            p.set_positions(positions);
            assert!(shot.s.frame().is_some());
            let expected = shot.screen({
                let at = point_along(&line(n), along).expect("a point");
                (f64::from(at.0), f64::from(at.1))
            });
            let k = shot.k;
            let (mut sum, mut cx, mut cy) = (0f64, 0f64, 0f64);
            for y in (expected.1 - 8. * k) as usize..(expected.1 + 8. * k) as usize {
                for x in (expected.0 - 8. * k) as usize..(expected.0 + 8. * k) as usize {
                    let gain = (cover(shot.s.pixels[y * shot.s.w + x], rgb(n), ground) - cover(before[y * shot.s.w + x], rgb(n), ground)).max(0.);
                    sum += gain;
                    cx += gain * (x as f64 + 0.5);
                    cy += gain * (y as f64 + 0.5);
                }
            }
            assert!(sum > 3. * k * k, "the dot of operator {} is in the picture ({sum})", n + 1);
            let off = ((cx / sum - expected.0).powi(2) + (cy / sum - expected.1).powi(2)).sqrt();
            println!("operator {} at {size:?}@{scale}: dot centroid {off:.2} px from its line's point", n + 1);
            println!("   centroid {:.2},{:.2} expected {:.2},{:.2}, mass {sum:.1}", cx / sum, cy / sum, expected.0, expected.1);
            worst = worst.max(off);
            assert!(off <= 0.75, "operator {}'s dot is {off:.2} px off its line at {size:?}@{scale}", n + 1);
        }
    }
    println!("worst dot offset {worst:.2} px");
}
