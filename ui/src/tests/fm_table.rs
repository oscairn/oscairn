// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM view's operator table and the detail's named stages (ui/GRID.md "The operator table", "The detail"):
//! fixtures for the data kinds the layout extension can say (an engine-like operator with named stages as numbered
//! segments, a hold among them; plain named stages; numbered rates and levels; each with and without `seconds`), for mute
//! and solo as the data gives them, and the checks that need them.

use super::shots::*;
use super::views::viewed;
use super::*;
use serde_json::{json, Value};

/// What the fixture's envelopes are made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Env {
    /// A hold (a time, `time1`), then an attack, two decays and a release, each a rate (`rate2` to `rate5`) to its level
    /// (`level2` to `level5`; the release's is where a note starts and ends); `seconds` of a rate: the full range's.
    Engine { seconds: bool },
    /// Attack, hold, decay, sustain and release as named stages (times in seconds when `seconds`).
    Named { seconds: bool },
    /// Four rates and four levels (`rate1` to `rate4`, `level1` to `level4`).
    Numbered { seconds: bool },
}

/// What the plugin says of an operator's mute and solo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Mute {
    /// Neither field.
    None,
    /// A mute (the player's edit) and no solo.
    Only,
    /// A mute and a solo.
    Both,
    /// A mute marked transient: the host derives the solo.
    Transient,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub ops: usize,
    pub env: Env,
    pub mute: Mute,
    /// The operator's other parameters beyond the table's and the envelope's: a dozen more, as an engine has.
    pub rich: bool,
}

pub(super) const ENGINE: Spec = Spec { ops: 8, env: Env::Engine { seconds: true }, mute: Mute::Transient, rich: true };

/// Parameter ids: operator n (1 to 8) has a block of 100 from `1000 + 100 n`.
pub(super) fn id(n: usize, k: u32) -> u32 {
    1000 + 100 * n as u32 + k
}

pub(super) const LEVEL: u32 = 0;
pub(super) const COARSE: u32 = 1;
pub(super) const FINE: u32 = 2;
pub(super) const DETUNE: u32 = 3;
pub(super) const MODE: u32 = 4;
pub(super) const MUTE: u32 = 5;
pub(super) const SOLO: u32 = 6;
/// The envelope's parameters from here: times (or rates) at +0 to +7, levels at +10 to +18.
pub(super) const ENV: u32 = 10;
pub(super) const REST: u32 = 40;

fn geometric(from: f64, to: f64) -> Vec<f64> {
    (0..33).map(|k| from * (to / from).powf(f64::from(k) / 32.)).collect()
}

fn param(id: u32, name: &str, min: f64, max: f64, stepped: bool, field: &str, index: usize) -> Value {
    let mut p = viewed(id, name, max, stepped, 1, field, index as u32);
    p["minimum"] = json!(min);
    p["module"] = json!("Operators");
    p
}

/// The plugin: the algorithm and feedback, and for each operator its table's parameters, its envelope and, with
/// `rich`, more.
pub(super) fn list(spec: Spec) -> Value {
    let routing = |ops: usize| {
        let mut inputs = [0u8; 8];
        inputs[0] = 0b10;
        inputs[1] = 0b100;
        if ops > 3 {
            inputs[ops / 2] = 1 << (ops - 1);
        }
        let second = ops / 2;
        json!({"inputs": inputs, "carriers": 1 | (1u32 << second), "feedbackFrom": 2, "feedbackTo": 2})
    };
    let mut algorithm = param(1, "Algorithm", 0., 1., true, "algorithm", 0);
    algorithm["algorithms"] = json!([routing(spec.ops), routing(spec.ops)]);
    let mut p = vec![algorithm, param(2, "Feedback", 0., 7., true, "feedback", 0)];
    for n in 1..=spec.ops {
        p.push(param(id(n, LEVEL), &format!("Level {n}"), 0., 99., true, "operatorLevel", n));
        p.push(param(id(n, COARSE), &format!("Coarse {n}"), 0., 31., true, "ratio", n));
        p.push(param(id(n, FINE), &format!("Fine {n}"), 0., 127., true, "fine", n));
        p.push(param(id(n, DETUNE), &format!("Detune {n}"), -15., 15., true, "detune", n));
        p.push(param(id(n, MODE), &format!("Mode {n}"), 0., 1., true, "fixed", n));
        if matches!(spec.mute, Mute::Only | Mute::Both | Mute::Transient) {
            let mut m = param(id(n, MUTE), &format!("Mute {n}"), 0., 1., true, "operatorMute", n);
            m["transient"] = json!(spec.mute == Mute::Transient);
            p.push(m);
        }
        if spec.mute == Mute::Both {
            p.push(param(id(n, SOLO), &format!("Solo {n}"), 0., 1., true, "operatorSolo", n));
        }
        let rate_seconds = |full: bool| if full { json!(geometric(0.0005, 20.)) } else { Value::Null };
        let mut stage = |k: u32, name: &str, field: &str, max: f64, seconds: Value| {
            let mut q = param(id(n, ENV + k), &format!("{name} {n}"), 0., max, true, field, n);
            if !seconds.is_null() {
                q["seconds"] = seconds;
            }
            p.push(q);
        };
        match spec.env {
            Env::Engine { seconds } => {
                stage(0, "Hold", "time1", 99., if seconds { json!(geometric(0.001, 2.)) } else { Value::Null });
                for (k, name) in ["Attack", "Decay 1", "Decay 2", "Release"].iter().enumerate() {
                    stage(1 + k as u32, name, &format!("rate{}", k + 2), 99., rate_seconds(seconds));
                    stage(11 + k as u32, &format!("{name} level"), &format!("level{}", k + 2), 99., Value::Null);
                }
            }
            Env::Named { seconds } => {
                let secs = |lo: f64, hi: f64| if seconds { json!(geometric(lo, hi)) } else { Value::Null };
                stage(0, "Attack", "attack", 99., secs(0.001, 5.));
                stage(1, "Hold", "hold", 99., secs(0.001, 2.));
                stage(2, "Decay", "decay", 99., secs(0.005, 10.));
                stage(3, "Sustain", "sustain", 99., Value::Null);
                stage(4, "Release", "release", 99., secs(0.005, 10.));
            }
            Env::Numbered { seconds } => {
                for k in 1..=4u32 {
                    stage(k - 1, &format!("Rate {k}"), &format!("rate{k}"), 99., rate_seconds(seconds));
                    stage(10 + k - 1, &format!("Level {k}"), &format!("level{k}"), 99., Value::Null);
                }
            }
        }
        if spec.rich {
            for (k, (name, max, widget)) in [("Level vel", 14., "knob"), ("Key follow", 7., "knob"), ("Break point", 127., "slider"), ("Low depth", 99., "knob"), ("High depth", 99., "knob"), ("Low curve", 3., "buttons"), ("High curve", 3., "buttons"), ("Form", 6., "list"), ("Spread", 99., "knob"), ("Formant", 99., "knob"), ("Key reset", 1., "switch"), ("Fixed key", 1., "switch"), ("Pitch vel", 14., "knob"), ("Pitch start", 99., "knob"), ("Pitch attack", 99., "knob"), ("Pitch time", 99., "knob")].iter().enumerate() {
                let mut q = param(id(n, REST + k as u32), &format!("{name} {n}"), 0., *max, true, "", n);
                q["widget"] = json!(widget);
                p.push(q);
            }
        }
    }
    json!({ "parameters": p })
}

/// The texts of the fixture's values, as a plugin writes them.
pub(super) fn text(pid: u32, v: f64) -> String {
    let k = pid.saturating_sub(1000) % 100;
    match pid {
        1 => format!("{}", v as i64 + 1),
        2 => format!("{}", v as i64),
        _ => match k {
            LEVEL => format!("{v:.0}"),
            COARSE => format!("{v:.0}"),
            FINE => format!("{v:.0}"),
            DETUNE => format!("{v:+.0}"),
            MODE => if v > 0.5 { "Fixed" } else { "Ratio" }.into(),
            MUTE | SOLO => if v > 0.5 { "On" } else { "Off" }.into(),
            k if (ENV..ENV + 10).contains(&k) => format!("{v:.0}"),
            k if (ENV + 10..ENV + 20).contains(&k) => format!("{v:.0}"),
            REST..=59 => format!("{v:.0}"),
            _ => format!("{v:.0}"),
        },
    }
}

/// Levels from loud to quiet; operators 3 and 6 muted where there is a mute; operator 5 fixed; every envelope its own.
pub(super) fn values(spec: Spec) -> Vec<(u32, f64)> {
    let mut v = vec![(1, 0.), (2, 5.)];
    for n in 1..=spec.ops {
        let f = n as f64;
        v.push((id(n, LEVEL), 99. - 9. * (f - 1.)));
        v.push((id(n, COARSE), if n == 5 { 20. } else { f64::from(n as u32 % 4) + 1. }));
        v.push((id(n, FINE), 10. * f));
        v.push((id(n, DETUNE), f - 4.));
        v.push((id(n, MODE), if n == 5 { 1. } else { 0. }));
        if spec.mute != Mute::None {
            v.push((id(n, MUTE), if n == 3 || n == 6 { 1. } else { 0. }));
        }
        let (a, d, s, r) = (60. - 5. * f, 40. + 6. * f, 70. - 6. * f, 30. + 5. * f);
        match spec.env {
            Env::Engine { .. } => {
                v.push((id(n, ENV), 20. + 5. * f));
                for (k, rate) in [a + 20., d, 90. - 4. * f, r].iter().enumerate() {
                    v.push((id(n, ENV + 1 + k as u32), *rate));
                }
                for (k, level) in [99., s + 10., s, 0.].iter().enumerate() {
                    v.push((id(n, ENV + 11 + k as u32), *level));
                }
            }
            Env::Named { .. } => {
                for (k, x) in [a, 20. + f, d, s, r].iter().enumerate() {
                    v.push((id(n, ENV + k as u32), *x));
                }
            }
            Env::Numbered { .. } => {
                for (k, x) in [a + 20., d, 90. - 4. * f, r].iter().enumerate() {
                    v.push((id(n, ENV + k as u32), *x));
                }
                for (k, x) in [99., s + 10., s, 0.].iter().enumerate() {
                    v.push((id(n, ENV + 10 + k as u32), *x));
                }
            }
        }
        if spec.rich {
            for k in 0..16u32 {
                v.push((id(n, REST + k), f64::from(k + n as u32) % 4.));
            }
        }
    }
    v
}

/// The plugin with four more pages beside the operators' (so the keyboard's two levels of tabs are there).
pub(super) fn paged(mut l: Value) -> Value {
    let mut all = l["parameters"].as_array().unwrap().clone();
    for (page, n) in [("Filter", 3), ("Amp", 3), ("Voicing", 3), ("Global", 3)] {
        for k in 0..n {
            all.push(json!({"id": 9000 + all.len() as u32, "name": format!("{page} {k}"), "module": page, "minimum": 0.0, "maximum": 1.0, "defaultValue": 0.5, "stepped": false, "flags": 0, "widget": "knob", "priority": 0, "group": 0}));
        }
    }
    l["parameters"] = json!(all);
    l
}

pub(super) fn open_at(spec: Spec, size: (usize, usize), scale: f32, theme: &theme::Loaded, placement: crate::TabPlacement, selected: usize) -> (Screen, crate::params::Screen) {
    open_list(list(spec), spec, size, scale, theme, placement, selected)
}

pub(super) fn open_list(l: Value, spec: Spec, size: (usize, usize), scale: f32, theme: &theme::Loaded, placement: crate::TabPlacement, selected: usize) -> (Screen, crate::params::Screen) {
    fit_to_window();
    let mut s = Screen::sized(size.0, size.1);
    s.set_scale(scale);
    theme::apply(&s.ui, theme);
    s.ui.global::<crate::Theme>().set_tab_placement(placement);
    let mut p = open_with(&mut s, l, &values(spec), &text);
    s.frame();
    p.shown();
    s.frame();
    // The page's pages: with the plugin's own page only (one page), the placement does not change the area.
    if selected > 1 {
        for _ in 1..selected {
            p.act(crate::nav::Action::Page(1));
        }
        s.frame();
    }
    (s, p)
}

// Pictures for a look: target/theme-shots/table-ENV-OPS-SELECTED-PLACEMENT-THEME@SCALE-WxH.png. The engine-like data at
// eight operators in every theme at the keyboard's screen and the three sizes; every data kind and operator count in the
// dark theme.
#[test]
fn table_pictures() {
    use crate::TabPlacement::{Side, Top};
    let envs: [(&str, Env, Mute); 5] = [
        ("engine", Env::Engine { seconds: true }, Mute::Transient),
        ("named", Env::Named { seconds: true }, Mute::Both),
        ("named-plain", Env::Named { seconds: false }, Mute::None),
        ("rates", Env::Numbered { seconds: true }, Mute::Only),
        ("rates-plain", Env::Numbered { seconds: false }, Mute::Transient),
    ];
    let themes = all_themes();
    let by = |id: &str| themes.iter().find(|t| t.id == id).unwrap_or_else(|| panic!("theme {id}"));
    for (ename, env, mute) in envs {
        for ops in [8usize, 6, 4] {
            let spec = Spec { ops, env, mute, rich: ename == "engine" };
            let picks: &[usize] = match ops {
                8 => &[1, 5, 8],
                6 => &[1, 5, 6],
                _ => &[1, 3, 4],
            };
            type Case<'a> = ((usize, usize), f32, crate::TabPlacement, &'a str, &'a str);
            let mut cases: Vec<Case> = vec![((1280, 720), 1.55, Side, "side", "oscairn-dark")];
            if ename == "engine" {
                cases.extend([((1280, 720), 1.55, Top, "top", "oscairn-dark"), ((1280, 720), 1., Side, "side", "oscairn-dark"), ((800, 480), 1., Top, "top", "oscairn-dark")]);
                if ops == 8 {
                    cases.extend(["oscairn-light", "odd", "odd-joined", "oscairn-block"].map(|t| ((1280, 720), 1.55, Side, "side", t)));
                }
            }
            for (size, scale, placement, pname, theme) in cases {
                let (mut s, mut p) = open_list(paged(list(spec)), spec, size, scale, by(theme), placement, 1);
                for &n in picks {
                    select(&mut p, &s, n);
                    s.frame();
                    shot(&s, &format!("table-{ename}-{ops}-{n}-{pname}-{theme}@{scale}-{}x{}", size.0, size.1));
                }
            }
        }
    }
}

/// Chooses operator `n` (1 to 8) by a tap on its chip in the table.
pub(super) fn select(p: &mut crate::params::Screen, s: &Screen, n: usize) {
    let panel = &panels(&s.ui)[0];
    let chip = panel.boxes.iter().filter(|b| b.edge == -1 && b.text == n.to_string() && b.fill >= 16).last().expect("the operator's chip");
    let at = (chip.x + chip.w / 2., chip.y + chip.h / 2.);
    assert!(p.view_down(0, at.0, at.1));
    p.view_up(0, at.0, at.1);
}

// ---- The checks ----

use crate::fm::{self, Column};
use crate::nav::Action;
use crate::GridState;
use crate::TabPlacement::{Side, Top};

pub(super) fn dark() -> theme::Loaded {
    all_themes().into_iter().find(|t| t.id == "oscairn-dark").expect("the dark theme")
}

fn tiles(s: &Screen) -> Vec<crate::GridTile> {
    s.ui.global::<GridState>().get_tiles().iter().collect()
}

fn shape_of(l: &Value) -> fm::FmShape {
    fm::fm_of(&crate::grid::parse(l), 1)
}

fn stage_tiles(s: &Screen) -> (Vec<crate::GridTile>, Vec<crate::GridTile>) {
    let compact: Vec<crate::GridTile> = tiles(s).into_iter().filter(|t| t.compact).collect();
    let top = compact.iter().map(|t| t.y).fold(f32::MAX, f32::min);
    let time: Vec<crate::GridTile> = compact.iter().filter(|t| (t.y - top).abs() < 0.5).cloned().collect();
    let level_y = compact.iter().map(|t| t.y).filter(|&y| y > top + 0.5).fold(f32::MAX, f32::min);
    let level: Vec<crate::GridTile> = compact.iter().filter(|t| (t.y - level_y).abs() < 0.5).cloned().collect();
    (time, level)
}

fn names(v: &[crate::GridTile]) -> Vec<String> {
    v.iter().map(|t| t.name.to_string()).collect()
}

fn param_ids(sent: &[(&str, Value)]) -> Vec<u64> {
    sent.iter().rfind(|o| o.0 == "controllers.view").map_or(vec![], |o| o.1["screen"]["params"].as_array().unwrap().iter().map(|p| p["param"].as_u64().unwrap()).collect())
}

fn sets(sent: &[(&str, Value)]) -> Vec<(u64, f64)> {
    sent.iter().filter(|o| o.0 == "params.set").flat_map(|o| o.1["values"].as_array().unwrap().iter().map(|v| (v["param"].as_u64().unwrap(), v["value"].as_f64().unwrap()))).collect()
}

/// The cursor on the table's cell (row 1 to 8, column among the table's) by the keys: up from the first tile to the table, then along.
fn to_cell(p: &mut crate::params::Screen, s: &Screen, row: usize, col: usize) {
    select(p, s, row);
    p.act(Action::Arrow { dx: 0, dy: 1 });
    p.act(Action::Arrow { dx: 0, dy: -1 });
    p.act(Action::Move { pane: Some(0), by: -8 });
    p.act(Action::Move { pane: Some(0), by: col as i32 });
}

fn labels_of(s: &Screen, panel: usize) -> Vec<String> {
    panels(&s.ui)[panel].labels.iter().map(|l| l.text.to_string()).collect()
}

// The table has a column for each field the data provides, left to right, and its headings once.
#[test]
fn the_tables_columns_follow_the_data() {
    use Column::*;
    assert_eq!(shape_of(&list(ENGINE)).columns(), [Mute, Solo, Level, Coarse, Fine, Mode, Detune]);
    let without = |fields: &[&str]| {
        let mut l = list(ENGINE);
        let kept: Vec<Value> = l["parameters"].as_array().unwrap().iter().filter(|p| !fields.contains(&p["field"].as_str().unwrap())).cloned().collect();
        l["parameters"] = json!(kept);
        l
    };
    assert_eq!(shape_of(&without(&["detune"])).columns(), [Mute, Solo, Level, Coarse, Fine, Mode]);
    assert_eq!(shape_of(&without(&["fine", "fixed"])).columns(), [Mute, Solo, Level, Ratio, Detune], "no fine of its own: the ratio is one column");
    assert_eq!(shape_of(&without(&["operatorLevel", "detune", "fixed"])).columns(), [Mute, Solo, Coarse, Fine]);
    // The headings are drawn once, in the overview.
    for (l, absent) in [(list(ENGINE), None), (without(&["detune"]), Some("Detune"))] {
        fit_to_window();
        let mut s = Screen::sized(1280, 720);
        theme::apply(&s.ui, &dark());
        let _p = open_with(&mut s, l, &values(ENGINE), &text);
        s.frame();
        let labels = labels_of(&s, 0);
        for h in ["Level", "Coarse", "Fine", "Mode", "Detune"] {
            let n = labels.iter().filter(|t| *t == h).count();
            assert_eq!(n, usize::from(absent != Some(h)), "{h} heading drawn once, or not at all where the data has no such field");
        }
    }
}

// The cursor moves by row and column, the dial in reading order; a press on a number's cell starts its editing, the
// dial then changes it, a press or Cancel ends it; nothing is sent before the press.
#[test]
fn the_cursor_walks_cells_and_a_press_starts_editing_one() {
    let (s, mut p) = open_at(ENGINE, (1280, 720), 1.55, &dark(), Side, 1);
    sent(&mut p.out);
    let rings = |s: &Screen| -> Vec<(i32, bool)> { panels(&s.ui)[0].shapes.iter().filter(|x| (x.ink == 12 || x.ink == 0) && x.width == 2 && !x.fill).map(|x| (x.ink, true)).collect() };
    // From the tiles, up: the table, on the selected operator's row in the first column (the mute mark).
    p.act(Action::Arrow { dx: 0, dy: 1 });
    p.act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!(rings(&s), [(12, true)], "one ring: the cursor's");
    assert_eq!(sent(&mut p.out).iter().filter(|o| o.0 == "params.set").count(), 0, "looking is not an edit");
    // The dial walks the cells in reading order: along the row, then the next row's first.
    p.act(Action::Move { pane: None, by: 2 });
    let ring_at = |s: &Screen| {
        let r = panels(&s.ui)[0].shapes.iter().filter(|x| x.ink == 12 && x.width == 2).last().unwrap().commands.to_string();
        let v = super::envelope::vertices(&r);
        (v.iter().map(|q| q.0).fold(f64::MAX, f64::min), v.iter().map(|q| q.1).fold(f64::MAX, f64::min))
    };
    let level_of_row_1 = panels(&s.ui)[0].boxes.iter().filter(|b| b.fill >= 16 && b.edge == -1 && b.text.is_empty()).map(|b| (f64::from(b.x), f64::from(b.y))).next().unwrap();
    let (x2, y2) = ring_at(&s);
    assert!(x2 >= level_of_row_1.0 - 3. && (y2 - level_of_row_1.1).abs() < 8., "two on from the mute mark is the level cell of the same row ({x2},{y2}) vs {level_of_row_1:?}");
    p.act(Action::Move { pane: None, by: 5 });
    let (_, y_next) = ring_at(&s);
    assert!(y_next > y2 + 5., "past the row's last cell, the next row ({y2} to {y_next})");
    // The pane keys: pane 1 down the rows in the same column.
    let (xa, _) = ring_at(&s);
    p.act(Action::Move { pane: Some(1), by: 1 });
    let (xb, yb) = ring_at(&s);
    assert!((xa - xb).abs() < 1. && yb > y_next + 5., "a row down, the same column");
    // A press on the level's cell: editing (the accent's ring), nothing yet sent; the dial then changes the value.
    p.act(Action::Move { pane: Some(0), by: -8 });
    p.act(Action::Move { pane: Some(0), by: 2 });
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    assert!(panels(&s.ui)[0].shapes.iter().any(|x| x.ink == 0 && x.width == 2 && !x.fill), "the ring is the accent's while editing");
    let out = sent(&mut p.out);
    assert_eq!(sets(&out), vec![], "a press sets nothing");
    p.act(Action::Move { pane: None, by: -1 });
    let out = sent(&mut p.out);
    let row = shape_row(&s);
    assert_eq!(sets(&out), vec![(u64::from(id(row, LEVEL)), values(ENGINE).iter().find(|v| v.0 == id(row, LEVEL)).unwrap().1 - 1.)], "the dial changes the level by a step");
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(out.iter().filter(|o| o.0 == "params.gesture").count(), 1, "the press ends the gesture: {out:?}");
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(sets(&sent(&mut p.out)), vec![], "no longer editing: the dial moves the cursor, it sets nothing");
}

/// The selected operator (1 to 8): the number in the detail's heading.
fn shape_row(s: &Screen) -> usize {
    labels_of(s, 1).iter().find_map(|t| t.strip_prefix("Operator ").and_then(|n| n.parse().ok())).expect("the heading names the operator")
}

// M switches the operator's mute field with one edit; S by the data: none, mute only, mute and solo, a transient mute.
#[test]
fn mute_and_solo_by_what_the_data_offers() {
    // A transient mute: M is one `params.set` with no gesture; S mutes the others and gives them back.
    let (s, mut p) = open_at(ENGINE, (1280, 720), 1.55, &dark(), Side, 1);
    to_cell(&mut p, &s, 2, 0);
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(sets(&out), vec![(u64::from(id(2, MUTE)), 1.)], "M sends exactly one edit");
    assert!(out.iter().all(|o| o.0 == "params.set"), "no gesture, no commit (no undo step, nothing saved): {out:?}");
    let mute_box = |s: &Screen, n: usize| panels(&s.ui)[0].boxes.iter().filter(|b| b.mark && b.text == "M").nth(n - 1).map(|b| (b.fill, b.edge)).unwrap();
    assert_eq!(mute_box(&s, 2), (14, -1), "operator 2's mark is on, in the mute color");
    p.act(Action::Confirm(None));
    assert_eq!(sets(&sent(&mut p.out)), vec![(u64::from(id(2, MUTE)), 0.)], "and off again");
    // Solo of operator 2 (mutes of 3 and 6 are there): the others are muted, the soloed is not.
    p.act(Action::Move { pane: Some(0), by: 1 });
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    let mut muted: Vec<u64> = sets(&out).iter().map(|x| x.0).collect();
    muted.sort_unstable();
    assert_eq!(muted, [1, 4, 5, 7, 8].map(|n| u64::from(id(n, MUTE))), "every operator that was not muted is: 1, 4, 5, 7 and 8 (3 and 6 were)");
    assert!(sets(&out).iter().all(|x| x.1 == 1.), "all muted");
    assert!(out.iter().all(|o| o.0 == "params.set"), "a host solo is no gesture and no commit: no undo step, no saved change: {out:?}");
    assert!(labels_of(&s, 0).iter().any(|t| t == "solo"), "a mark says a solo is on");
    let on = panels(&s.ui)[0].boxes.iter().filter(|b| b.mark && b.text == "S" && b.fill == 0).count();
    assert_eq!(on, 1, "one S lit, in the accent");
    // A second solo: operator 5 too; then the un-soloed stay muted, 2 and 5 are heard.
    to_cell(&mut p, &s, 5, 1);
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(sets(&out), vec![(u64::from(id(5, MUTE)), 0.)], "soloing a second operator un-mutes it: {out:?}");
    // Releasing both gives the mutes back as they were: 3 and 6 muted, the rest not.
    p.act(Action::Confirm(None));
    sent(&mut p.out);
    to_cell(&mut p, &s, 2, 1);
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    let after: Vec<(u64, f64)> = {
        let mut v = sets(&out);
        v.sort_by_key(|x| x.0);
        v
    };
    assert!(after.iter().all(|x| x.1 == if x.0 == u64::from(id(3, MUTE)) || x.0 == u64::from(id(6, MUTE)) { 1. } else { 0. }), "the mutes are what they were: {after:?}");
    assert!(!labels_of(&s, 0).iter().any(|t| t == "solo"), "no solo, no mark");
    assert!(!out.iter().any(|o| o.0 == "params.gesture" || o.0 == "graph.commit"));

    // A mute that is the player's edit (not transient): M is one edit with its gesture; S is disabled, the readout says why.
    let spec = Spec { mute: Mute::Only, ..ENGINE };
    let (s, mut p) = open_at(spec, (1280, 720), 1.55, &dark(), Side, 1);
    to_cell(&mut p, &s, 2, 0);
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(sets(&out), vec![(u64::from(id(2, MUTE)), 1.)]);
    assert_eq!(out.iter().filter(|o| o.0 == "params.gesture").count(), 2, "an edit as any other: begin and end");
    p.act(Action::Move { pane: Some(0), by: 1 });
    assert!(labels_of(&s, 0).iter().any(|t| t.starts_with("No solo")), "the readout says why S does nothing: {:?}", labels_of(&s, 0));
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    assert!(sent(&mut p.out).is_empty(), "S sends nothing");

    // A mute and a solo field: S is one edit of the solo field.
    let spec = Spec { mute: Mute::Both, ..ENGINE };
    let (s, mut p) = open_at(spec, (1280, 720), 1.55, &dark(), Side, 1);
    to_cell(&mut p, &s, 4, 1);
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(sets(&out), vec![(u64::from(id(4, SOLO)), 1.)]);
    assert!(panels(&s.ui)[0].boxes.iter().any(|b| b.mark && b.text == "S" && b.fill == 0), "the soloed mark is lit");

    // Neither field: M and S are drawn off, and say why.
    let spec = Spec { mute: Mute::None, ..ENGINE };
    let (s, mut p) = open_at(spec, (1280, 720), 1.55, &dark(), Side, 1);
    let marks: Vec<_> = panels(&s.ui)[0].boxes.iter().filter(|b| b.mark).map(|b| (b.fill, b.edge)).collect();
    assert_eq!(marks.len(), 16);
    assert!(marks.iter().all(|m| *m == (13, 5)), "every mark drawn disabled: {marks:?}");
    to_cell(&mut p, &s, 3, 0);
    assert!(labels_of(&s, 0).iter().any(|t| t.starts_with("This plugin has no mute")));
    sent(&mut p.out);
    p.act(Action::Confirm(None));
    assert!(sent(&mut p.out).is_empty());
}

// The eight knobs play the selected row's columns, left to right.
#[test]
fn the_knobs_play_the_selected_rows_columns() {
    let (s, mut p) = open_at(ENGINE, (1280, 720), 1.55, &dark(), Side, 1);
    for n in [5, 2, 8] {
        sent(&mut p.out);
        select(&mut p, &s, n);
        let out = sent(&mut p.out);
        let ids = param_ids(&out);
        assert_eq!(ids[..4], [id(n, LEVEL), id(n, COARSE), id(n, FINE), id(n, DETUNE)].map(u64::from), "operator {n}'s level, coarse, fine and detune first");
        assert!(!ids[..4].contains(&u64::from(id(n, MUTE))) && !ids[..4].contains(&u64::from(id(n, MODE))), "switched cells are not what the row's knobs play first");
        assert!(sets(&out).is_empty(), "choosing is not an edit");
    }
    // The cursor walking the table's rows moves them too.
    to_cell(&mut p, &s, 1, 2);
    sent(&mut p.out);
    p.act(Action::Move { pane: Some(1), by: 2 });
    assert_eq!(shape_row(&s), 3);
    let ids = param_ids(&sent(&mut p.out));
    assert_eq!(ids[0], u64::from(id(3, LEVEL)));
}

// Stages by name, in both kinds of data: a segment with a time and no level is a Hold, then the Attack, the Decays, the
// Release; the named form by its fields.
#[test]
fn stages_are_named_for_every_kind_of_data() {
    let open = |env: Env| open_at(Spec { env, ..ENGINE }, (1280, 720), 1.55, &dark(), Side, 1);
    let (s, _p) = open(Env::Engine { seconds: true });
    let (time, level) = stage_tiles(&s);
    assert_eq!(names(&time), ["Hold", "Attack", "Decay 1", "Decay 2", "Release"]);
    assert_eq!(names(&level), ["Attack", "Decay 1", "Decay 2", "Release"], "the release level is the one the note starts and ends at: shown once");
    let (s, _p) = open(Env::Numbered { seconds: true });
    let (time, level) = stage_tiles(&s);
    assert_eq!(names(&time), ["Attack", "Decay 1", "Decay 2", "Release"], "rate 1 and level 1 are the attack, 2 the first decay, 3 the second (the sustain), 4 the release");
    assert_eq!(names(&level), ["Attack", "Decay 1", "Decay 2", "Release"]);
    let (s, _p) = open(Env::Named { seconds: true });
    let (time, level) = stage_tiles(&s);
    assert_eq!(names(&time), ["Attack", "Hold", "Decay", "Release"], "the named stages by their fields");
    assert_eq!(names(&level), ["Sustain"]);
    // The stage helper alone, on the three shapes.
    let st = |env: Env| {
        let l = list(Spec { env, ops: 1, ..ENGINE });
        let params = crate::grid::parse(&l);
        let fields: Vec<&str> = params.iter().filter(|p| p.index == 1 && crate::grid::is_envelope(&p.field)).map(|p| p.field.as_str()).collect();
        let e = crate::grid::envelope_of_fields(&fields);
        crate::grid::stages(&e, &fields).cells.iter().map(|c| (c.name.clone(), c.time.map(|t| t.1), c.level.as_ref().map(|l| l.1.clone()))).collect::<Vec<_>>()
    };
    let n = |name: &str, rate: Option<bool>, level: Option<&str>| (name.to_string(), rate, level.map(String::from));
    assert_eq!(st(Env::Engine { seconds: true }), [n("Hold", Some(false), None), n("Attack", Some(true), Some("Attack")), n("Decay 1", Some(true), Some("Decay 1")), n("Decay 2", Some(true), Some("Decay 2")), n("Release", Some(true), Some("Release"))], "a hold is a time, the others rates");
    assert_eq!(st(Env::Numbered { seconds: false }).len(), 4);
}

// A time shows in ms or s from the plugin's seconds when it gives them; a rate's seconds are marked as the full range's,
// never the stage's length; without seconds a rate is the raw value and "rate", a time the plugin's own text.
#[test]
fn times_show_only_where_the_plugin_gives_them() {
    let values = |spec: Spec| {
        let (s, _p) = open_at(spec, (1280, 720), 1.55, &dark(), Side, 1);
        let (time, level) = stage_tiles(&s);
        (time.iter().map(|t| (t.name.to_string(), t.value.to_string())).collect::<Vec<_>>(), level.iter().map(|t| t.value.to_string()).collect::<Vec<_>>())
    };
    let is_time = |t: &str| {
        let mut parts = t.split(' ');
        parts.next().is_some_and(|n| n.parse::<f64>().is_ok()) && matches!(parts.next(), Some("ms" | "s")) && parts.next().is_none()
    };
    // Engine-like: Operator 1 is selected. The hold is a duration; the four rates are marked full range.
    let (time, level) = values(Spec { env: Env::Engine { seconds: true }, ..ENGINE });
    assert!(is_time(&time[0].1), "the hold's real time: {:?}", time[0]);
    for (name, v) in &time[1..] {
        assert!(v.ends_with(" /range") && is_time(v.trim_end_matches(" /range")), "{name}: the full-range time, marked: {v}");
    }
    assert!(level.iter().all(|v| v.parse::<f64>().is_ok()), "levels are plain values: {level:?}");
    // Without seconds: the hold's own text, the rates raw with their unit word, no made-up time.
    let (time, _) = values(Spec { env: Env::Engine { seconds: false }, ..ENGINE });
    assert!(time[0].1.parse::<f64>().is_ok(), "the plugin's text: {:?}", time[0]);
    for (name, v) in &time[1..] {
        assert!(v.ends_with(" rate") && v.trim_end_matches(" rate").parse::<f64>().is_ok(), "{name}: the raw rate: {v}");
    }
    // Numbered rates and levels, with and without.
    let (time, _) = values(Spec { env: Env::Numbered { seconds: true }, ..ENGINE });
    assert!(time.iter().all(|(_, v)| v.ends_with(" /range") && is_time(v.trim_end_matches(" /range"))), "{time:?}");
    let (time, _) = values(Spec { env: Env::Numbered { seconds: false }, ..ENGINE });
    assert!(time.iter().all(|(_, v)| v.ends_with(" rate")), "{time:?}");
    // Named times are durations: seconds, no mark; without, the plugin's text.
    let (time, _) = values(Spec { env: Env::Named { seconds: true }, ..ENGINE });
    assert!(time.iter().all(|(_, v)| is_time(v)), "{time:?}");
    let (time, _) = values(Spec { env: Env::Named { seconds: false }, ..ENGINE });
    assert!(time.iter().all(|(_, v)| v.parse::<f64>().is_ok()), "{time:?}");
    assert_eq!(crate::grid::seconds_text(0.0123), "12 ms");
    assert_eq!(crate::grid::seconds_text(0.0095), "9.5 ms");
    assert_eq!(crate::grid::seconds_text(1.26), "1.3 s");
    assert_eq!(crate::grid::seconds_text(14.2), "14 s");
}

// Each control of the rows is under the stage it moves: its point lies in its control's span (the stage's slot) within a
// node's radius, wherever the values put it, when the width allows (the slots are as wide as a compact control at least).
#[test]
fn each_stage_control_is_under_its_point() {
    for env in [Env::Engine { seconds: true }, Env::Numbered { seconds: true }, Env::Named { seconds: false }] {
        for (size, scale) in [((1280, 720), 1.55f32), ((1280, 720), 1.), ((800, 480), 1.)] {
            let spec = Spec { ops: 4, env, ..ENGINE };
            let (s, _p) = open_at(spec, size, scale, &dark(), Side, 1);
            let panel = &panels(&s.ui)[2];
            let points: Vec<(f32, f32)> = panel.points.iter().map(|p| (p.x, p.y)).collect();
            assert!(!points.is_empty(), "the detail's graph has its handles at {env:?} {size:?}@{scale}");
            let (time, level) = stage_tiles(&s);
            // Time cell k is under point k; a level cell under the point of its stage (the ones with a level, in order).
            let tol = crate::grid::NODE_RADIUS;
            let ends: Vec<f32> = points.iter().map(|p| p.0).collect();
            // The handles are one per point, in the envelope's order; the time cells are the points that have a time.
            let with_time: Vec<usize> = match env {
                Env::Engine { .. } => vec![0, 1, 2, 3, 4],
                Env::Numbered { .. } => vec![0, 1, 2, 3],
                Env::Named { .. } => vec![0, 1, 2, 3],
            };
            assert_eq!(time.len(), with_time.len());
            for (t, &k) in time.iter().zip(&with_time) {
                assert!(ends[k] >= t.x - tol && ends[k] <= t.x + t.w + tol, "{env:?} {size:?}@{scale}: {} cell {}..{} and its point at {}", t.name, t.x, t.x + t.w, ends[k]);
            }
            let with_level: Vec<usize> = match env {
                Env::Engine { .. } => vec![1, 2, 3, 4],
                Env::Numbered { .. } => vec![0, 1, 2, 3],
                Env::Named { .. } => vec![2],
            };
            assert_eq!(level.len(), with_level.len());
            for (t, &k) in level.iter().zip(&with_level) {
                assert!(ends[k] >= t.x - tol && ends[k] <= t.x + t.w + tol, "{env:?} {size:?}@{scale}: level {} cell {}..{} and its point at {}", t.name, t.x, t.x + t.w, ends[k]);
            }
            // The graph's own geometry: the cell's left edge is the slot's, so editing a value cannot move a control.
        }
    }
}

// The compact control's size is defined once, from the touch size, the spacing and the type scale: the Rust layout and the
// theme's sizes agree in every theme, never below the touch height, and the controls are that size.
#[test]
fn the_compact_control_is_defined_once_and_is_never_below_a_touch_target() {
    for t in all_themes() {
        let (s, _p) = open_at(Spec { ops: 4, ..ENGINE }, (1280, 720), 1.55, &t, Side, 1);
        let g = s.ui.global::<crate::Theme>();
        let (h, w) = (g.get_compact_height(), g.get_compact_width());
        assert!((h - crate::grid::compact_height(g.get_font_scale(), g.get_spacing())).abs() < 0.01, "{}: theme {h}, layout {}", t.id, crate::grid::compact_height(g.get_font_scale(), g.get_spacing()));
        assert!(h >= g.get_touch() && w >= 1.9 * g.get_touch() - 0.01, "{}: {w} x {h}", t.id);
        let compact: Vec<crate::GridTile> = tiles(&s).into_iter().filter(|t| t.compact).collect();
        assert!(!compact.is_empty());
        assert!(compact.iter().all(|c| (c.h - h).abs() < 0.01 && c.h >= g.get_touch()), "{}: every compact control is {h} high", t.id);
        assert!(compact.iter().all(|c| c.w >= w - 0.5 || c.w >= g.get_touch()), "{}: widths {:?}", t.id, compact.iter().map(|c| c.w).collect::<Vec<_>>());
    }
}

// The layout's numbers at the three screens: with 8 operators the table shows every row without scrolling at a readable
// height, the detail has the two rows and a row of eight compact controls; with fewer operators what is freed goes to the
// detail's graph. Printed with --nocapture (ui/GRID.md "The operator table").
#[test]
fn the_screens_hold_all_rows_the_stage_rows_and_a_row_of_controls() {
    let mut report = String::new();
    for ops in [8usize, 6, 4] {
        for (size, scale) in [((1280, 720), 1.55f32), ((1280, 720), 1.), ((800, 480), 1.)] {
            for (name, placement) in [("no pages", Top), ("rail at the side", Side), ("strips over", Top)] {
                let t = dark();
                // The engine's own page plus four more, so two levels of tabs are there (none for "no pages").
                let one = Spec { ops, ..ENGINE };
                let l = if name == "no pages" { list(one) } else { paged(list(one)) };
                fit_to_window();
                let mut s = Screen::sized(size.0, size.1);
                s.set_scale(scale);
                theme::apply(&s.ui, &t);
                s.ui.global::<crate::Theme>().set_tab_placement(placement);
                let mut p = open_with(&mut s, l, &values(Spec { ops, ..ENGINE }), &text);
                s.frame();
                p.shown();
                s.frame();
                let g = s.ui.global::<GridState>();
                let (w, h) = (g.get_view_width(), g.get_view_height());
                let at = format!("{ops} operators, {size:?}@{scale}, {name}");
                let all = panels(&s.ui);
                let top = &all[0];
                let small = 13. * t.data.font_scale;
                // Every row of the table, at a readable height, inside the overview.
                let chips: Vec<crate::GridBox> = top.boxes.iter().filter(|b| b.fill >= 16 && b.edge == -1 && b.text.parse::<usize>().is_ok()).collect();
                assert_eq!(chips.len(), ops, "a row for every operator {at}");
                assert!(chips.iter().all(|c| c.h >= small * 1.3 - 0.01 && c.y >= 0. && c.y + c.h <= top.h), "rows at a readable height ({:.1} of {:.1} low) inside the overview {at}", chips[0].h, small * 1.3);
                super::fm_view::check_panel(top, ops, small, &at);
                let tl = tiles(&s);
                let bottom = tl.iter().map(|t| t.y + t.h).fold(0., f32::max);
                assert!(bottom <= h + 0.5, "the block fits the area {at}: ends at {bottom} of {h}");
                let compact = tl.iter().filter(|t| t.compact).count();
                let (time, level) = stage_tiles(&s);
                assert_eq!((time.len(), level.len()), (5, 4), "the Time and Level rows {at}");
                let graph = all.get(2).map_or(0., |p| if p.points.row_count() == 0 { 0. } else { p.h });
                // The selected operator's envelope is drawn above its Time and Level rows at eight operators too, wherever the
                // two levels of tabs do not take the height (the strips over the page at the keyboard's size and under).
                if name != "strips over" {
                    let first_row = time.iter().map(|t| t.y).fold(f32::MAX, f32::min);
                    let room = first_row - all[2].y;
                    assert!(all[2].points.row_count() > 0 && room >= 3. * small - 0.5, "the detail's graph above the Time row at {at}: {room}");
                    assert!(all[2].points.iter().all(|q| all[2].y + q.y < first_row), "its handles are above the Time row at {at}");
                }
                report += &format!("{at}: area {w:.0}x{h:.0}; overview {:.0} (rows {:.1}), detail graph {}, rows 2, controls {compact} compact of {:.0}x{:.0}; block ends {bottom:.0}\n", top.h, chips[0].h, if graph > 0. { format!("{:.0}", graph) } else { "none".into() }, tl.iter().find(|t| t.compact).map_or(0., |t| t.w), tl.iter().find(|t| t.compact).map_or(0., |t| t.h));
            }
        }
    }
    println!("{report}");
}

// By touch: a tap on a switched cell (mute) switches it with one edit and selects its row; a tap on a number's cell selects
// its row and changes nothing; a tap on a cell that cannot be edited selects the row and the readout says why.
#[test]
fn touch_switches_a_mark_selects_a_row_and_explains_a_disabled_cell() {
    let at = |s: &Screen, letter: &str, n: usize| {
        let b = panels(&s.ui)[0].boxes.iter().filter(|b| b.mark && b.text == letter).nth(n - 1).expect("the mark");
        (b.x + b.w / 2., b.y + b.h / 2.)
    };
    let (s, mut p) = open_at(ENGINE, (1280, 720), 1.55, &dark(), Side, 1);
    sent(&mut p.out);
    let (x, y) = at(&s, "M", 4);
    assert!(p.view_down(0, x, y));
    assert!(sets(&sent(&mut p.out)).is_empty(), "nothing is set until the finger lifts");
    p.view_up(0, x, y);
    let out = sent(&mut p.out);
    assert_eq!(sets(&out), vec![(u64::from(id(4, MUTE)), 1.)], "one edit");
    assert!(!out.iter().any(|o| o.0 == "params.gesture" || o.0 == "graph.commit"), "a transient mute: no gesture, no commit");
    assert_eq!(shape_row(&s), 4, "the row is the one selected");
    // A finger that slides off before it lifts switches nothing.
    let (x, y) = at(&s, "M", 5);
    assert!(p.view_down(0, x, y));
    p.view_up(0, x + 200., y + 100.);
    assert!(sets(&sent(&mut p.out)).is_empty());
    // A number's cell: select only.
    let coarse = panels(&s.ui)[0].boxes.iter().filter(|b| b.text == "20").nth(1).expect("operator 5's coarse cell");
    let (x, y) = (coarse.x + coarse.w / 2., coarse.y + coarse.h / 2.);
    assert!(p.view_down(0, x, y));
    p.view_up(0, x, y);
    assert!(sets(&sent(&mut p.out)).is_empty(), "a tap on a number's cell changes nothing");
    assert_eq!(shape_row(&s), 5);
    // Neither field: a tap on a mark selects the row and says why.
    let (s, mut p) = open_at(Spec { mute: Mute::None, ..ENGINE }, (1280, 720), 1.55, &dark(), Side, 1);
    sent(&mut p.out);
    let (x, y) = at(&s, "S", 6);
    assert!(p.view_down(0, x, y));
    p.view_up(0, x, y);
    assert!(sent(&mut p.out).iter().all(|o| o.0 != "params.set"));
    assert_eq!(shape_row(&s), 6);
    assert!(labels_of(&s, 0).iter().any(|t| t.starts_with("This plugin has no solo")), "the readout: {:?}", labels_of(&s, 0));
}

// The detail's graph: a handle is dragged by touch (its stage's time or rate across its slot, its level up), for the
// selected operator only, inside one gesture.
#[test]
fn the_detail_graphs_handles_are_dragged_by_touch() {
    let (s, mut p) = open_at(Spec { ops: 4, ..ENGINE }, (1280, 720), 1.55, &dark(), Side, 1);
    let panel = panels(&s.ui)[2].clone();
    let points: Vec<(f32, f32)> = panel.points.iter().map(|q| (q.x, q.y)).collect();
    assert_eq!(points.len(), 5, "hold, attack, two decays, release");
    // The attack's handle (the second): across and down.
    let (x, y) = points[1];
    sent(&mut p.out);
    assert!(p.view_down(2, x, y));
    p.view_move(2, x + 14., y + 9.);
    p.view_up(2, x + 14., y + 9.);
    let out = sent(&mut p.out);
    let mut ids: Vec<u64> = sets(&out).iter().map(|q| q.0).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids, [u64::from(id(1, ENV + 1)), u64::from(id(1, ENV + 11))], "the attack's rate and level of operator 1, nothing else: {out:?}");
    assert!(out.iter().any(|o| o.0 == "params.gesture" && o.1["begin"] == true) && out.iter().any(|o| o.0 == "params.gesture" && o.1["begin"] == false));
    // The hold's handle has a time and no level.
    let (x, y) = points[0];
    assert!(p.view_down(2, x, y));
    p.view_move(2, x + 10., y + 30.);
    p.view_up(2, x + 10., y + 30.);
    let ids: Vec<u64> = sets(&sent(&mut p.out)).iter().map(|q| q.0).collect();
    assert!(!ids.is_empty() && ids.iter().all(|&i| i == u64::from(id(1, ENV))), "the hold's time only: {ids:?}");
}

// The overview's graph has at least a third of the overview's width and at least what the diagram has; the diagram shrinks
// first, the table's columns are tightened before the graph gives up width (the keyboard's size and the two others).
#[test]
fn the_graph_has_a_third_of_the_overview_and_at_least_the_diagrams_width() {
    let t = dark();
    for ops in [8usize, 6, 4] {
        for (size, scale, placement) in [((1280, 720), 1.55f32, Side), ((1280, 720), 1.55, Top), ((1280, 720), 1., Side), ((800, 480), 1., Top)] {
            let (s, _p) = open_at(Spec { ops, ..ENGINE }, size, scale, &t, placement, 1);
            let panel = panels(&s.ui)[0].clone();
            let sp = s.ui.global::<crate::Theme>().get_spacing();
            let at = format!("{ops} operators {size:?}@{scale}");
            let (pad, gap) = (12. * sp, 12. * sp);
            let table_x = panel.labels.iter().find(|l| l.text == "Op").expect("the table's heading").x;
            let diagram_x = panel.labels.iter().find(|l| l.text.starts_with("Algorithm")).expect("the algorithm's head").x;
            let (graph_w, diagram_w, avail) = (table_x - gap - pad, panel.w - pad - diagram_x, panel.w - 2. * pad);
            assert!(graph_w >= avail / 3. - 1., "the graph has a third of the overview ({graph_w:.0} of {avail:.0}) {at}");
            assert!(graph_w >= diagram_w - 1., "the graph is at least as wide as the diagram ({graph_w:.0} against {diagram_w:.0}) {at}");
        }
    }
}

// The compact controls say the operator once, in the heading: "Level" under "Operator 5", not "Level 5"; the knobs (and so the
// cursor's readout) keep the plugin's full name.
#[test]
fn the_detail_does_not_repeat_the_operators_number() {
    use crate::grid::without_index;
    assert_eq!(without_index("Level 5", 5), "Level");
    assert_eq!(without_index("Coarse 5", 5), "Coarse");
    assert_eq!(without_index("5 Fine", 5), "Fine");
    assert_eq!(without_index("Op 5 Detune", 5), "Detune");
    assert_eq!(without_index("Op5 Mode", 5), "Mode");
    assert_eq!(without_index("Level 6", 5), "Level 6", "another operator's number stays");
    assert_eq!(without_index("Level vel 5", 5), "Level vel");
    assert_eq!(without_index("5", 5), "5", "a name that is only the number stays");
    assert_eq!(without_index("Level 0", 5), "Level 0");
    let (s, mut p) = open_at(Spec { ops: 8, ..ENGINE }, (1280, 720), 1.55, &dark(), Side, 1);
    select(&mut p, &s, 5);
    let mut s = s;
    s.frame();
    let shown = names(&tiles(&s));
    assert!(shown.iter().skip(8).all(|n| !n.ends_with(" 5")), "no number under the heading: {shown:?}");
    assert!(shown.iter().skip(8).any(|n| n == "Level"), "{shown:?}");
}

// A compact knob under 28 px is a value cell with a thin bar and still the tile's whole touch target.
#[test]
fn a_compact_knob_too_small_to_be_one_is_a_value_cell() {
    use crate::grid::{compact_height, knob_is_cell, KNOB_LEAST};
    assert!(knob_is_cell(115., compact_height(1., 1.), 1., 1.), "at the touch target's height the knob would be 21 px");
    assert!(!knob_is_cell(115., 80., 1., 1.), "a tile high enough keeps its knob");
    assert!(KNOB_LEAST >= 28.);
    let (s, _p) = open_at(Spec { ops: 8, ..ENGINE }, (1280, 720), 1.55, &dark(), Side, 1);
    let all = tiles(&s);
    let knobs: Vec<&crate::GridTile> = all.iter().filter(|t| t.compact && t.kind == crate::grid::Control::Knob as i32).collect();
    assert!(!knobs.is_empty() && knobs.iter().all(|t| t.cell && t.h >= compact_height(1., 1.)), "every compact knob is a cell, a touch target high");
    assert!(all.iter().filter(|t| !t.compact).all(|t| !t.cell));
}

// The one-request rule: a handle that moves two parameters is one `params.set` carrying both, so the engine makes one undo
// step of it.
#[test]
fn a_handle_drag_is_one_request_with_both_values() {
    let (s, mut p) = open_at(Spec { ops: 4, ..ENGINE }, (1280, 720), 1.55, &dark(), Side, 1);
    let points: Vec<(f32, f32)> = panels(&s.ui)[2].points.iter().map(|q| (q.x, q.y)).collect();
    let (x, y) = points[1];
    sent(&mut p.out);
    assert!(p.view_down(2, x, y));
    p.view_move(2, x + 14., y + 9.);
    p.view_move(2, x + 20., y + 12.);
    p.view_up(2, x + 20., y + 12.);
    let out = sent(&mut p.out);
    let requests: Vec<&(&str, Value)> = out.iter().filter(|o| o.0 == "params.set").collect();
    assert_eq!(requests.len(), 2, "one request for each move of the handle, not one for each value: {out:?}");
    for r in requests {
        let ids: Vec<u64> = r.1["values"].as_array().unwrap().iter().map(|v| v["param"].as_u64().unwrap()).collect();
        assert_eq!(ids, [u64::from(id(1, ENV + 1)), u64::from(id(1, ENV + 11))], "the rate and the level together: {r:?}");
    }
    assert_eq!(out.iter().filter(|o| o.0 == "params.gesture" && o.1["begin"] == true).count(), 2, "inside the gesture of the two parameters");
}

// A page higher than its view (the odd theme's large type at the keyboard's size) is cut at the bottom and scrolls: a mark at the
// right edge says so, and where the view is; a page that fits is not marked.
#[test]
fn a_page_that_scrolls_has_a_mark_and_one_that_fits_has_none() {
    let themes = all_themes();
    let odd = themes.iter().find(|t| t.id == "odd").expect("the odd theme");
    let mark_in = |s: &Screen| -> bool {
        let accent = s.ui.global::<crate::Theme>().get_accent();
        let (r, g, b) = (accent.red(), accent.green(), accent.blue());
        // Some column in the last 40 pixels has a vertical run of 8 or more pixels of the accent.
        (s.w - 40..s.w).any(|x| {
            let mut run = 0;
            (0..s.h).any(|y| {
                let p = s.pixels[y * s.w + x];
                run = if (p.r, p.g, p.b) == (r, g, b) { run + 1 } else { 0 };
                run >= 8
            })
        })
    };
    let (mut s, mut p) = open_at(Spec { ops: 8, ..ENGINE }, (1280, 720), 1.55, odd, Side, 1);
    s.frame();
    let g = s.ui.global::<GridState>();
    assert!(g.get_content() > g.get_view_height() + 1., "the odd theme's page is higher than its view: {} of {}", g.get_content(), g.get_view_height());
    assert!(mark_in(&s), "a mark at the right edge says the page goes on");
    // Scrolled to the end, the mark has moved down with it.
    let before = s.pixels.clone();
    p.scrolled(10000.);
    p.shown();
    s.frame();
    assert_ne!(before, s.pixels, "the page moved");
    assert!(mark_in(&s), "and the mark is still there");
    let (s, _p) = open_at(Spec { ops: 4, ..ENGINE }, (1280, 720), 1., &dark(), Side, 1);
    let g = s.ui.global::<GridState>();
    assert!(g.get_content() <= g.get_view_height() + 1., "a page that fits: {} of {}", g.get_content(), g.get_view_height());
    // (The mark is drawn only while the page is higher than its view: the property says the two.)
}

// After an Undo or a Redo the page reads its values again: the engine's undo of a request that set two values sent no
// `params.changed` (the real-engine run), so the tiles kept the dragged values.
#[test]
fn the_page_reads_its_values_again_after_an_undo() {
    let (_s, mut p) = open_at(Spec { ops: 4, ..ENGINE }, (1280, 720), 1.55, &dark(), Side, 1);
    sent(&mut p.out);
    p.values_again();
    let out = sent(&mut p.out);
    // Every parameter, those of the views (the envelope's handles) too, not only those with a tile, in requests of at most 128.
    let asked: Vec<usize> = out.iter().filter(|o| o.0 == "params.get").map(|o| o.1["values"].as_array().unwrap().len()).collect();
    assert!(!asked.is_empty() && asked.iter().all(|&n| n <= 128), "{asked:?}");
    assert_eq!(asked.iter().sum::<usize>(), list(Spec { ops: 4, ..ENGINE })["parameters"].as_array().unwrap().len());
}
