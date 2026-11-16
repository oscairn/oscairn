// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the views' tests share: opening a plugin's page with the texts it would write, and measuring a view
//! in the rendered pixels (a green accent line over gray panels on black): how far the drawn line lies from
//! the path it was drawn from, which proves a view's paths are drawn unscaled from one set of points.

use super::*;

pub(super) fn sent(out: &mut Vec<crate::params::Out>) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(out).into_iter().map(|(_, m, p)| (m, p)).collect()
}

pub(super) fn panels(ui: &Shell) -> Vec<crate::GridPanel> {
    ui.global::<crate::GridState>().get_panels().iter().collect()
}

pub(super) fn set(id: u32, value: f64) -> (&'static str, serde_json::Value) {
    ("params.set", json!({"values": [{"node": 2, "param": id, "value": value}]}))
}

pub(super) fn gesture(id: u32, begin: bool) -> (&'static str, serde_json::Value) {
    ("params.gesture", json!({"node": 2, "param": id, "begin": begin}))
}

pub(super) fn plugin_param(id: u32, name: &str, max: f64, stepped: bool, widget: &str, priority: u32, group: u32) -> serde_json::Value {
    json!({"id": id, "name": name, "module": "", "minimum": 0.0, "maximum": max, "defaultValue": 0.0, "stepped": stepped, "flags": 0, "widget": widget, "priority": priority, "group": group})
}

/// The page of a plugin: its parameters and values, with the texts as `text` writes them.
pub(super) fn open_with(s: &mut Screen, list: serde_json::Value, values: &[(u32, f64)], text: &dyn Fn(u32, f64) -> String) -> crate::params::Screen {
    use crate::params::Request;
    s.ui.set_screen(7);
    let mut p = crate::params::Screen::new(&s.ui);
    p.open(2, "Synth");
    p.shown();
    sent(&mut p.out);
    p.reply(Request::List, Ok(list));
    let answer = |p: &mut crate::params::Screen| {
        for _ in 0..4 {
            for (r, _, _) in std::mem::take(&mut p.out) {
                if let Request::Text(id, bits) = r {
                    p.reply(Request::Text(id, bits), Ok(json!({"text": text(id, f64::from_bits(bits))})));
                }
            }
        }
    };
    answer(&mut p);
    let values: Vec<serde_json::Value> = values.iter().map(|(id, v)| json!({"node": 2, "param": id, "value": v})).collect();
    p.reply(Request::Values, Ok(json!({ "values": values })));
    answer(&mut p);
    s.frame();
    p.shown();
    answer(&mut p);
    s.frame();
    p
}

/// A view measured in the picture: the window, the panel as the screen gave it, and where its top left is
/// in the window's pixels (`k` pixels to the panel's pixel).
pub(super) struct Shot {
    pub s: Screen,
    pub panel: crate::GridPanel,
    pub origin: (f64, f64),
    pub k: f64,
}

/// Opens the page at this window size and scale in the pixel theme (a green accent on gray panels over
/// black) and finds panel number `which`: the band of rows lit across most of the panel's width whose height
/// is the panel's.
pub(super) fn view_shot(list: &serde_json::Value, values: &[(u32, f64)], text: &dyn Fn(u32, f64) -> String, which: usize, size: (usize, usize), scale: f32) -> (Shot, crate::params::Screen) {
    fit_to_window();
    let (w, h) = size;
    let mut s = Screen::sized(w, h);
    s.set_scale(scale);
    s.frame();
    theme::apply(&s.ui, &super::envelope::pixel_theme());
    let p = open_with(&mut s, list.clone(), values, text);
    s.frame();
    let panel = panels(&s.ui)[which].clone();
    let k = f64::from(scale);
    let lit = |x: usize, y: usize| {
        let q = s.pixels[y * w + x];
        u32::from(q.r) + u32::from(q.g) + u32::from(q.b) > 20
    };
    let width = f64::from(panel.w) * k;
    let rows: Vec<bool> = (0..h).map(|y| (0..w).filter(|&x| lit(x, y)).count() as f64 > 0.8 * width).collect();
    let mut bands: Vec<(usize, usize)> = Vec::new();
    let mut y = 0;
    while y < h {
        if rows[y] {
            let start = y;
            while y < h && rows[y] {
                y += 1;
            }
            bands.push((start, y - start));
        } else {
            y += 1;
        }
    }
    let expected = (f64::from(panel.h) * k).round() as usize;
    let same: Vec<&(usize, usize)> = bands.iter().filter(|b| b.1.abs_diff(expected) <= 2).collect();
    let before = panels(&s.ui)[..which].iter().filter(|q| (q.h - panel.h).abs() < 0.5).count();
    let (y0, _) = **same.get(before).unwrap_or_else(|| panic!("{w}x{h}@{scale}: the panel's rows (wanted {expected} tall) in {bands:?}"));
    let x0 = (0..w).find(|&x| lit(x, y0 + expected / 2)).expect("the panel's left edge");
    (Shot { s, panel, origin: (x0 as f64, y0 as f64), k }, p)
}

impl Shot {
    /// The green of a pixel (the accent over gray): 0 none, 1 full; a light tint is none.
    fn green(&self, x: i64, y: i64) -> f64 {
        if x < 0 || y < 0 || x >= self.s.w as i64 || y >= self.s.h as i64 {
            return 0.;
        }
        let p = self.s.pixels[y as usize * self.s.w + x as usize];
        // The fill under a curve is the accent lightly (about 0.17): not the line.
        (((f64::from(p.g) - f64::from(p.r)) / 255. - 0.3) / 0.7).max(0.)
    }
    /// The green at a place between pixel centers (bilinear).
    fn green_at(&self, x: f64, y: f64) -> f64 {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let g = |dx: i64, dy: i64| self.green(x0 as i64 + dx, y0 as i64 + dy);
        (g(0, 0) * (1. - tx) + g(1, 0) * tx) * (1. - ty) + (g(0, 1) * (1. - tx) + g(1, 1) * tx) * ty
    }
    /// A place in the panel's pixels, in the window's.
    pub fn screen(&self, v: (f64, f64)) -> (f64, f64) {
        (self.origin.0 + self.k * v.0, self.origin.1 + self.k * v.1)
    }
}

/// How far, in pixels, the drawn line lies from the path it was drawn from, at points along the path where it
/// runs straight (not at a corner or a step): the centroid of the green across the path's normal, measured
/// there. Returns how many places were measured and the worst offset.
pub(super) fn line_offsets(shot: &Shot, path: &str) -> (usize, f64) {
    let v: Vec<(f64, f64)> = super::envelope::vertices(path).into_iter().map(|q| shot.screen(q)).collect();
    // Resampled every 4 pixels along the path.
    let mut at: Vec<(f64, f64)> = Vec::new();
    let mut along = 0.;
    let mut next = 0.;
    for pair in v.windows(2) {
        let len = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
        while next <= along + len {
            let t = if len > 0. { (next - along) / len } else { 0. };
            at.push((pair[0].0 + (pair[1].0 - pair[0].0) * t, pair[0].1 + (pair[1].1 - pair[0].1) * t));
            next += 4.;
        }
        along += len;
    }
    let (mut count, mut worst) = (0, 0f64);
    for j in 2..at.len().saturating_sub(2) {
        let (a, b, c) = (at[j - 2], at[j], at[j + 2]);
        let (d1, d2) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
        let (l1, l2) = (d1.0.hypot(d1.1), d2.0.hypot(d2.1));
        if l1 < 1e-6 || l2 < 1e-6 || (d1.0 * d2.0 + d1.1 * d2.1) / (l1 * l2) < 0.98 {
            continue; // a corner or a step: the line is not straight here
        }
        let t = (c.0 - a.0, c.1 - a.1);
        let tl = t.0.hypot(t.1);
        let n = (-t.1 / tl, t.0 / tl);
        let (mut sum, mut weighted) = (0., 0.);
        let mut o = -4.;
        while o <= 4. {
            let g = shot.green_at(b.0 + n.0 * o, b.1 + n.1 * o);
            sum += g;
            weighted += g * o;
            o += 0.25;
        }
        if sum < 2. {
            continue; // not drawn here (under a handle, say)
        }
        count += 1;
        worst = worst.max((weighted / sum).abs());
    }
    (count, worst)
}

/// The four window sizes and scales the views are measured at.
pub(super) const SIZES: [((usize, usize), f32); 4] = [((1280, 720), 1.), ((1280, 720), 2.), ((1920, 1080), 1.), ((1920, 1080), 2.)];

/// Measures the path number `shape` of panel `which` of a page at every size and scale: it must have been
/// measured at enough places, the drawn line within 0.6 px of it, and the same path a pixel lower must be
/// found off (so the measure can fail). Returns a line of the report per size.
pub(super) fn check_line(name: &str, list: &serde_json::Value, values: &[(u32, f64)], text: &dyn Fn(u32, f64) -> String, which: usize, shape: usize) -> String {
    let mut report = String::new();
    for (size, scale) in SIZES {
        let (shot, _p) = view_shot(list, values, text, which, size, scale);
        let path = shot.panel.shapes.iter().nth(shape).expect("the path").commands.to_string();
        let (count, off) = line_offsets(&shot, &path);
        report += &format!("{name} {size:?}@{scale}: {count} places, worst {off:.2} px\n");
        assert!(count >= 20, "the line was measured at enough places:\n{report}");
        assert!(off <= 0.6, "the line is off its path by up to {off:.2} px:\n{report}");
        let lower: Vec<(f32, f32)> = super::envelope::vertices(&path).into_iter().map(|(x, y)| (x as f32, y as f32 + 1.)).collect();
        let (_, wrong) = line_offsets(&shot, &crate::views::poly(&lower));
        assert!(wrong > 0.7 * f64::from(scale), "a path a pixel off is seen as {wrong:.2} px off:\n{report}");
    }
    report
}
