// SPDX-License-Identifier: AGPL-3.0-or-later
//! The envelope's nodes sit on the vertices of its line, measured in the rendered pixels: a green line,
//! white nodes ringed in green, on a gray panel. Each node's centroid is compared with where the two fitted
//! segments of the line meet (an end node: its distance from its one segment, and the last one from the
//! fill's bottom edge, it being at the baseline), at two window sizes and two scales.

use super::*;

type P = (f64, f64);

fn pixel_theme() -> theme::Loaded {
    theme_of(json!({"version": 7, "id": "pixels", "name": "Pixels", "fontScale": 1, "radius": 6,
        "font": {"regular": "fonts/Inter-Regular.ttf", "license": "fonts/OFL.txt"},
        "colors": {"background": "#000000", "surface": "#202020", "raised": "#303030", "border": "#808080", "text": "#FFFFFF", "muted": "#A0A0A0", "accent": "#00FF00",
                   "positive": "#5AD9A4", "danger": "#FF8D87"},
        "style": {"buttons": "rounded", "edges": "none", "depth": "flat", "sliders": "line", "tabs": "segmented", "marks": "square", "sections": "separate", "rows": "left"},
        "wires": {"audio": "#0000FF", "notes": "#00FF00", "disabled": "#808080", "draining": "#FF8800", "unconnected": "#FF0000", "control": "#00FFFF"}}))
}

/// The vertices of a path "M x y L x y ...".
fn vertices(path: &str) -> Vec<P> {
    let n: Vec<f64> = path.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    n.chunks(2).map(|c| (c[0], c[1])).collect()
}

/// A line through weighted points: a point and a unit direction (total least squares).
fn fit(pts: &[(P, f64)]) -> Option<(P, P)> {
    let w: f64 = pts.iter().map(|p| p.1).sum();
    if pts.len() < 8 || w <= 0. {
        return None;
    }
    let (mx, my) = (pts.iter().map(|p| p.0 .0 * p.1).sum::<f64>() / w, pts.iter().map(|p| p.0 .1 * p.1).sum::<f64>() / w);
    let (mut sxx, mut sxy, mut syy) = (0., 0., 0.);
    for ((x, y), k) in pts {
        let (dx, dy) = (x - mx, y - my);
        sxx += k * dx * dx;
        sxy += k * dx * dy;
        syy += k * dy * dy;
    }
    let angle = 0.5 * (2. * sxy).atan2(sxx - syy);
    Some(((mx, my), (angle.cos(), angle.sin())))
}

fn meet(a: (P, P), b: (P, P)) -> Option<P> {
    let cross = a.1 .0 * b.1 .1 - a.1 .1 * b.1 .0;
    if cross.abs() < 0.5 {
        return None;
    }
    let t = ((b.0 .0 - a.0 .0) * b.1 .1 - (b.0 .1 - a.0 .1) * b.1 .0) / cross;
    Some((a.0 .0 + a.1 .0 * t, a.0 .1 + a.1 .1 * t))
}

fn distance(l: (P, P), p: P) -> f64 {
    ((p.0 - l.0 .0) * l.1 .1 - (p.1 - l.0 .1) * l.1 .0).abs()
}

/// Every node's offset in pixels at this window size and scale.
fn measure(w: usize, h: usize, scale: f32) -> Vec<(String, f64)> {
    fit_to_window();
    let mut s = Screen::sized(w, h);
    let mut list = super::grid::every_kind();
    let only: Vec<_> = list["parameters"].as_array().unwrap().iter().filter(|p| p["view"] == json!(1)).cloned().collect();
    list["parameters"] = json!(only);
    s.set_scale(scale);
    s.frame();
    theme::apply(&s.ui, &pixel_theme());
    let mut p = super::grid::open(&mut s, list, &[(9, 0.1), (10, 0.4), (11, 0.6), (12, 0.5)]);
    s.frame();
    p.shown();
    s.frame();
    let panel = s.ui.global::<crate::GridState>().get_panels().iter().next().expect("an envelope panel");
    let nodes: Vec<P> = panel.points.iter().map(|q| (q.x as f64, q.y as f64)).collect();
    let verts = vertices(panel.line.as_str());
    let k = f64::from(scale);
    let green = |x: usize, y: usize| {
        let p = s.pixels[y * w + x];
        (f64::from(p.g) - f64::from(p.r)) / 255.
    };
    let white = |x: usize, y: usize| {
        let p = s.pixels[y * w + x];
        p.r >= 200 && p.g >= 200 && p.b >= 200
    };
    let lit = |x: usize, y: usize| {
        let p = s.pixels[y * w + x];
        u32::from(p.r) + u32::from(p.g) + u32::from(p.b) > 20
    };
    // The panel's band: the first run of rows lit more than 80% of the panel's width (the tiles under it follow).
    let width = f64::from(panel.w) * k;
    let rows: Vec<bool> = (0..h).map(|y| (0..w).filter(|&x| lit(x, y)).count() as f64 > 0.8 * width).collect();
    let tall = 40 * scale as usize; // not the top bar's rule
    let y0 = (0..h).find(|&y| rows[y..(y + tall).min(h)].iter().all(|&r| r)).expect("the panel's rows");
    let y1 = y0 + rows[y0..].iter().position(|&r| !r).unwrap_or(h - y0);
    let x0 = (0..w).find(|&x| lit(x, (y0 + y1) / 2)).unwrap();
    let x1 = (0..w).rev().find(|&x| lit(x, (y0 + y1) / 2)).unwrap() + 1;
    // The white components in the panel are the nodes.
    let mut seen = vec![false; w * h];
    let mut found: Vec<P> = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            if seen[y * w + x] || !white(x, y) {
                continue;
            }
            let (mut stack, mut n, mut sx, mut sy) = (vec![(x, y)], 0., 0., 0.);
            seen[y * w + x] = true;
            while let Some((cx, cy)) = stack.pop() {
                n += 1.;
                sx += cx as f64 + 0.5;
                sy += cy as f64 + 0.5;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                    let (nx, ny) = ((cx as i32 + dx) as usize, (cy as i32 + dy) as usize);
                    if nx >= x0 && nx < x1 && ny >= y0 && ny < y1 && !seen[ny * w + nx] && white(nx, ny) {
                        seen[ny * w + nx] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            if n >= 10. {
                found.push((sx / n, sy / n));
            }
        }
    }
    found.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    assert_eq!(found.len(), nodes.len(), "{w}x{h}@{scale}: a white disc per node");
    // Roughly where the panel is (the nodes only help pick the line's pixels; the offsets come from the pixels).
    let n = nodes.len() as f64;
    let origin = (found.iter().zip(&nodes).map(|(f, p)| f.0 - k * p.0).sum::<f64>() / n, found.iter().zip(&nodes).map(|(f, p)| f.1 - k * p.1).sum::<f64>() / n);
    let screen = |v: P| (origin.0 + k * v.0, origin.1 + k * v.1);
    // The line's pixels along each segment, away from the nodes and the corners.
    let mut lines: Vec<Option<(P, P)>> = Vec::new();
    for pair in verts.windows(2) {
        let (a, b) = (screen(pair[0]), screen(pair[1]));
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        let mut pts = Vec::new();
        if len > 30. * k {
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f64 + 0.5 - a.0, y as f64 + 0.5 - a.1);
                    let t = (px * dx + py * dy) / (len * len);
                    let off = (px * dy - py * dx).abs() / len;
                    let weight = green(x, y);
                    if (0.3..=0.7).contains(&t) && off < 4. * k && weight > 0.4 {
                        pts.push(((x as f64 + 0.5, y as f64 + 0.5), weight));
                    }
                }
            }
        }
        lines.push(fit(&pts));
    }
    // Vertex v of the path is between segments v-1 and v; a node is the vertex it has the coordinates of.
    let mut out = Vec::new();
    for (j, node) in nodes.iter().enumerate() {
        let v = verts.iter().position(|q| (q.0 - node.0).abs() < 0.11 && (q.1 - node.1).abs() < 0.11).expect("a node is a vertex of the line");
        let c = found[j];
        let (before, after) = (if v > 0 { lines[v - 1] } else { None }, lines.get(v).copied().flatten());
        let name = format!("{w}x{h}@{scale} node {j}");
        match (before, after) {
            (Some(a), Some(b)) => match meet(a, b) {
                Some(m) => out.push((format!("{name}, to the vertex (dx {:+.2}, dy {:+.2})", c.0 - m.0, c.1 - m.1), (c.0 - m.0).hypot(c.1 - m.1))),
                // Segments nearly parallel meet far along themselves: the node's distance from each line instead.
                None => out.push((format!("{name}, to its two segments"), distance(a, c).max(distance(b, c)))),
            },
            (Some(l), None) | (None, Some(l)) => out.push((format!("{name}, to its segment"), distance(l, c))),
            _ => {}
        }
    }
    // The last node is at the baseline: its centre is on the fill's bottom edge, found in a column before it.
    let last = found[found.len() - 1];
    let column = (last.0 - 14. * k) as usize;
    for y in (y0..y1).rev() {
        let a = green(column, y);
        if (0.02..0.3).contains(&a) {
            let edge = y as f64 + (a / 0.17).min(1.);
            out.push((format!("{w}x{h}@{scale} last node, to the fill's bottom edge (dy {:+.2})", last.1 - edge), (last.1 - edge).abs()));
            break;
        }
    }
    out
}

#[test]
fn nodes_are_centered_on_the_lines_vertices_in_the_pixels() {
    let mut worst = 0f64;
    let mut report = String::new();
    for (w, h, scale) in [(1280, 720, 1.), (1280, 720, 2.), (1920, 1080, 1.), (1920, 1080, 2.)] {
        for (what, d) in measure(w, h, scale) {
            report += &format!("{what}: {d:.2} px\n");
            worst = worst.max(d);
        }
    }
    println!("{report}");
    assert!(worst <= 0.75, "a node is off its vertex by up to {worst:.2} px:\n{report}");
}
