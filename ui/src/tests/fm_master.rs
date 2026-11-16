// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM operators view as a master and a detail on one page (ui/GRID.md "Master and detail", ui/TABS.md "Tabs that
//! are indexes"): the overview of every operator on top, the selected one's parameters as tiles under it; the page of
//! tabs that were the operators' indexes folded into it; selection by the dial and by touch reaching the diagram,
//! the graph, the list and the detail; the knobs under the screen; rows paged; the split at the screens' sizes.

use super::fm::*;
use super::fm_view::check_panel;
use super::shots::*;
use super::*;
use crate::nav::Action;
use crate::tabs::Model;
use crate::TabPlacement::{Side, Top};
use crate::{GridState, TabPlacement, Theme};
use serde_json::{json, Value};
use slint::Model as _;

/// A routing for `ops` operators: 2 into 1, 3 into 2, and a second stack for the upper half (the last into it).
fn routing_for(ops: usize) -> Routing {
    let mut inputs = [0u8; 8];
    inputs[0] = 0b10;
    inputs[1] = 0b100;
    let second = ops / 2;
    inputs[second] = 1 << (ops - 1);
    routing(inputs, 1 | (1 << second), 2, 2)
}

/// An FM plugin of `ops` operators on one page "Operators": the algorithm and feedback on the page itself, each
/// operator's parameters under a module "Operators/Operator N" (the page of tabs a plugin makes by module paths);
/// with `pages` four more pages, so the keyboard's two levels of tabs both show.
pub(super) fn plugin(ops: usize, pages: bool) -> Value {
    let list = fm_n(&[routing_for(ops), routing([0; 8], ((1u16 << ops) - 1) as u8, 0, 0)]);
    let all = list["parameters"].as_array().unwrap().clone();
    let mut out: Vec<Value> = all.into_iter().filter(|p| p["index"].as_u64().unwrap() as usize <= ops).collect();
    for p in &mut out {
        let n = p["index"].as_u64().unwrap();
        p["module"] = json!(if n == 0 { "Operators".to_string() } else { format!("Operators/Operator {n}") });
        if matches!(p["field"].as_str(), Some("operatorLevel" | "fine" | "detune")) {
            p["widget"] = json!("knob");
        }
    }
    if pages {
        for (page, names) in [("Filter", ["Cutoff", "Resonance", "Drive"]), ("Amp", ["Level", "Pan", "Velocity"]), ("Voicing", ["Glide", "Unison", "Spread"]), ("Global", ["Tune", "Bend", "Mix"])] {
            for (k, name) in names.iter().enumerate() {
                out.push(json!({"id": 2000 + out.len() as u32, "name": name, "module": page, "minimum": 0.0, "maximum": 1.0, "defaultValue": 0.5, "stepped": false, "flags": 0, "widget": if k == 0 { "knob" } else { "slider" }, "priority": 0, "group": 0}));
            }
        }
    }
    json!({ "parameters": out })
}

fn model_of(list: &Value) -> Model {
    Model::build(&crate::params::editor_parameters(crate::grid::parse(list)), None)
}

fn open_at(list: &Value, size: (usize, usize), scale: f32, theme: &theme::Loaded, placement: TabPlacement) -> (Screen, crate::params::Screen) {
    fit_to_window();
    let mut s = Screen::sized(size.0, size.1);
    s.set_scale(scale);
    theme::apply(&s.ui, theme);
    s.ui.global::<Theme>().set_tab_placement(placement);
    let mut p = open_with(&mut s, list.clone(), &fm_n_values(0., false), &fm_n_text);
    s.frame();
    p.shown();
    s.frame();
    (s, p)
}

fn tile_names(s: &Screen) -> Vec<String> {
    s.ui.global::<GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect()
}

fn param_ids(sent: &[(&str, Value)]) -> Vec<u64> {
    sent.iter().rfind(|o| o.0 == "controllers.view").map_or(vec![], |o| o.1["screen"]["params"].as_array().unwrap().iter().map(|p| p["param"].as_u64().unwrap()).collect())
}

/// Chooses operator `n` (1 to 8) by a tap on its line's number in the overview.
fn tap_operator(p: &mut crate::params::Screen, s: &Screen, n: usize) {
    let panel = &panels(&s.ui)[0];
    let chip = panel.boxes.iter().filter(|b| b.edge == -1 && b.text == n.to_string()).last().expect("the operator's chip");
    let at = (chip.x + chip.w / 2., chip.y + chip.h / 2.);
    assert!(p.view_down(0, at.0, at.1));
    p.view_up(0, at.0, at.1);
}

fn heading(s: &Screen) -> String {
    panels(&s.ui)[1].labels.iter().next().expect("the detail's heading").text.to_string()
}

fn cursor_tile(s: &Screen) -> Option<usize> {
    s.ui.global::<GridState>().get_tiles().iter().position(|t| t.cursor)
}

// The rule: a page whose tabs are exactly the indexes of one view that has an overview is not a page of tabs.
#[test]
fn tabs_that_are_the_indexes_of_one_view_fold_into_it_and_others_stay_tabs() {
    let list = plugin(8, false);
    let m = model_of(&list);
    assert_eq!(m.pages.len(), 1);
    let page = &m.pages[0];
    // Without the rule the page has the plugin's own tab (the algorithm and feedback) and one per operator.
    assert!(page.tabs.is_empty(), "no tab per operator: {:?}", page.tabs);
    assert_eq!(page.indexed.as_ref().map(|i| i.view), Some(1));
    assert_eq!(page.indexed.as_ref().unwrap().names, (1..=8u64).map(|n| (n, format!("Operator {n}"))).collect::<Vec<_>>());
    assert!(page.params.iter().all(|x| x.tab.is_none()), "every parameter is on the page");
    assert_eq!(m.entries().len(), 1, "one entry for the page keys");
    // With other pages beside it, the operators' page is still one, the others untouched.
    let m = model_of(&plugin(8, true));
    assert_eq!(m.pages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Operators", "Filter", "Amp", "Voicing", "Global"]);
    assert!(m.pages[0].tabs.is_empty() && m.pages[0].indexed.is_some());

    let tabs_after = |edit: &dyn Fn(&mut Vec<Value>)| {
        let mut list = plugin(8, false);
        let mut ps = list["parameters"].as_array().unwrap().clone();
        edit(&mut ps);
        list["parameters"] = json!(ps);
        let m = model_of(&list);
        (m.pages[0].tabs.len(), m.pages[0].indexed.is_some())
    };
    // The tab of operator 3 holds a parameter that is operator 4's: not the indexes.
    let (tabs, folded) = tabs_after(&|ps| {
        let q = ps.iter_mut().find(|p| p["module"] == "Operators/Operator 3").unwrap();
        q["index"] = json!(4);
    });
    assert!(!folded && tabs == 9, "a tab whose parameter has another index stays a tab ({tabs})");
    // Tab names without numbers.
    let (tabs, folded) = tabs_after(&|ps| {
        for p in ps.iter_mut() {
            if let Some(n) = p["module"].as_str().and_then(|m| m.strip_prefix("Operators/Operator ")).map(str::to_string) {
                p["module"] = json!(format!("Operators/Voice {}", ["A", "B", "C", "D", "E", "F", "G", "H"][n.parse::<usize>().unwrap() - 1]));
            }
        }
    });
    assert!(!folded && tabs == 9, "tabs that are not numbered stay tabs ({tabs})");
    // Parameters with no view at all (tabs of a plugin that does not describe its operators).
    let (_, folded) = tabs_after(&|ps| {
        for p in ps.iter_mut() {
            p["view"] = json!(0);
        }
    });
    assert!(!folded, "tabs of parameters that are in no view stay tabs");
    // A view with no overview (no algorithm): its indexes have no master to be the detail of.
    let (tabs, folded) = tabs_after(&|ps| ps.retain(|p| p["field"] != "algorithm"));
    assert!(!folded && tabs >= 8, "a view without an algorithm has no overview ({tabs})");
    // A tab of another view among them.
    let (_, folded) = tabs_after(&|ps| {
        for p in ps.iter_mut().filter(|p| p["module"] == "Operators/Operator 2") {
            p["view"] = json!(2);
        }
    });
    assert!(!folded, "operators of two views are not the indexes of one");
    // An unrelated tab beside the operators' (a plugin that has more on its page).
    let (_, folded) = tabs_after(&|ps| {
        let mut q = ps[0].clone();
        q["id"] = json!(3000);
        q["view"] = json!(0);
        q["field"] = json!("");
        q["index"] = json!(0);
        q["module"] = json!("Operators/Routing");
        ps.push(q);
    });
    assert!(!folded, "a tab with a name that is not an index stays, so the page stays tabs");
    // Two numbered tabs of an unrelated word stem are not siblings.
    let (_, folded) = tabs_after(&|ps| {
        for p in ps.iter_mut().filter(|p| p["module"] == "Operators/Operator 2") {
            p["module"] = json!("Operators/Layer 2");
        }
    });
    assert!(!folded, "tabs of different words before their numbers stay tabs");
    // The five-pages plugin of the tab tests (Operator tabs, no views): still tabs.
    let m = model_of(&super::tabs::five_pages());
    assert_eq!(m.pages[0].tabs.len(), 8);
    assert!(m.pages[0].indexed.is_none());
}

// Selecting an operator, by a tap on its line, reaches the diagram, the graph, the list and the detail at once.
#[test]
fn selecting_an_operator_reaches_the_diagram_the_graph_the_list_and_the_detail() {
    let list = plugin(8, false);
    let (s, mut p) = open_at(&list, (1280, 720), 1., &all_themes()[0], Top);
    let ring_around = |s: &Screen, n: usize| -> bool {
        let panel = &panels(&s.ui)[0];
        let b = panel.boxes.iter().find(|b| b.edge == 2 && b.text == n.to_string()).unwrap();
        panel.shapes.iter().filter(|x| x.ink == 3 && !x.fill && x.width == 1).any(|x| {
            let v = super::envelope::vertices(&x.commands);
            let (x0, x1) = (v.iter().map(|q| q.0).fold(f64::MAX, f64::min), v.iter().map(|q| q.0).fold(f64::MIN, f64::max));
            let (y0, y1) = (v.iter().map(|q| q.1).fold(f64::MAX, f64::min), v.iter().map(|q| q.1).fold(f64::MIN, f64::max));
            x0 < f64::from(b.x) && x1 > f64::from(b.x + b.w) && y0 < f64::from(b.y) && y1 > f64::from(b.y + b.h)
        })
    };
    let facts = |s: &Screen, n: usize| {
        let panel = &panels(&s.ui)[0];
        let ink = 16 + n as i32 - 1;
        let thick = panel.shapes.iter().any(|x| x.ink == ink && !x.fill && x.width == 2);
        let thin_others = (0..8).filter(|&m| m != n - 1).all(|m| !panel.shapes.iter().any(|x| (x.ink == 16 + m as i32 || x.ink == 32 + m as i32) && !x.fill && x.width == 2));
        let chosen_cells = panel.boxes.iter().filter(|b| b.fill == 4 && b.edge == 3 && b.text.is_empty()).count();
        let cell = panel.boxes.iter().find(|b| b.fill == 4 && b.edge == 3 && b.text.is_empty()).map(|b| (b.y, b.h));
        let chip = panel.boxes.iter().filter(|b| b.edge == -1 && b.text == n.to_string()).last().map(|b| b.y + b.h / 2.);
        let in_cell = matches!((cell, chip), (Some((y, h)), Some(c)) if c > y && c < y + h);
        let detail = &panels(&s.ui)[1];
        let tinted = detail.shapes.iter().next().is_some_and(|x| x.fill && x.ink == ink);
        (ring_around(s, n), thick && thin_others, chosen_cells == 1 && in_cell, tinted && heading(s) == format!("Operator {n}"))
    };
    assert_eq!(facts(&s, 1), (true, true, true, true), "operator 1 at the start");
    for n in [5, 8, 3] {
        tap_operator(&mut p, &s, n);
        let names = tile_names(&s);
        assert!(!names.is_empty() && names.iter().all(|t| t.ends_with(&format!(" {n}"))), "the detail holds operator {n}'s parameters only: {names:?}");
        assert_eq!(facts(&s, n), (true, true, true, true), "diagram ring, thick line, list cell and detail heading of operator {n}");
    }
    assert!(s.ui.global::<GridState>().get_tabs().row_count() == 0, "no tab for an operator");
}

// The detail has every parameter of the operator and only its, in the plugin's priority order (the numbered first, then
// the plugin's own), at the size the grid's tiles are elsewhere; the knobs under the screen play them in that order.
#[test]
fn the_detail_lists_the_selected_operators_parameters_in_priority_order_and_the_knobs_play_them() {
    let mut list = plugin(8, false);
    // Operator 5's ratio first, its release level second, its level third.
    for p in list["parameters"].as_array_mut().unwrap() {
        let id = p["id"].as_u64().unwrap();
        p["priority"] = json!(match id {
            205 => 1,
            644 => 2, // rate 4 of operator 5 (600 + 10 n + k)
            105 => 3,
            _ => 0,
        });
    }
    let all: Vec<Value> = list["parameters"].as_array().unwrap().iter().filter(|p| p["index"] == 5).cloned().collect();
    let mut expected: Vec<(u64, String)> = all.iter().map(|p| (p["id"].as_u64().unwrap(), p["name"].as_str().unwrap().to_string())).collect();
    expected.sort_by_key(|(id, _)| match id {
        205 => 1,
        644 => 2,
        105 => 3,
        _ => u64::MAX,
    });
    let (s, mut p) = open_at(&list, (1280, 720), 1., &all_themes()[0], Top);
    sent(&mut p.out);
    tap_operator(&mut p, &s, 5);
    let out = sent(&mut p.out);
    // 13 parameters at 6 columns: all in view, three rows.
    assert_eq!(tile_names(&s), expected.iter().map(|e| e.1.clone()).collect::<Vec<_>>(), "exactly operator 5's, in priority order");
    let tiles: Vec<crate::GridTile> = s.ui.global::<GridState>().get_tiles().iter().collect();
    assert!(tiles.iter().all(|t| (t.h == 84. || t.h == 120.) && t.w >= 150.), "tiles are the grid's size");
    assert_eq!(param_ids(&out)[..13], expected.iter().map(|e| e.0).collect::<Vec<_>>()[..], "the knobs play the row as the detail has it");
    // Another operator: the knobs follow.
    tap_operator(&mut p, &s, 2);
    let out = sent(&mut p.out);
    assert_eq!(&param_ids(&out)[..4], [102, 202, 302, 352], "operator 2's level, ratio, fine and detune first");
    // Selection costs no parameter change.
    assert!(out.iter().all(|o| o.0 != "params.set"));
}

// The rows of a detail that has more than it shows: the dial pages by row past the last or first tile in view, the
// arrows of the heading by touch, the page keys step the operator and keep the row.
#[test]
fn rows_page_by_the_dial_by_touch_and_the_page_keys_step_operators() {
    let list = plugin(8, false);
    // 826 x 465: three columns, one row in view of a 13-parameter operator (5 rows).
    let (s, mut p) = open_at(&list, (1280, 720), 1.55, &all_themes()[0], Side);
    tap_operator(&mut p, &s, 5);
    assert_eq!(tile_names(&s).len(), 3, "one row of three tiles in view");
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 1 of 5"), "the heading says which row");
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(cursor_tile(&s), Some(0));
    p.act(Action::Move { pane: None, by: 2 });
    assert_eq!(cursor_tile(&s), Some(2));
    let first_row = tile_names(&s);
    p.act(Action::Move { pane: None, by: 1 });
    assert_ne!(tile_names(&s), first_row, "the next row of tiles");
    assert_eq!(cursor_tile(&s), Some(0), "the cursor at the start of the new row");
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 2 of 5"));
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!((tile_names(&s), cursor_tile(&s)), (first_row.clone(), Some(2)), "and back a row, at the end of it");
    // Down by the arrows, in the column the cursor is in.
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(cursor_tile(&s), Some(2));
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 2 of 5"));
    // By touch: the heading's down arrow.
    let head = panels(&s.ui)[1].clone();
    let (x, y) = (head.w - head.h / 2., head.h / 2.);
    assert!(p.view_down(1, x, y));
    p.view_up(1, x, y);
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 3 of 5"), "a tap on the down arrow pages a row");
    // The page keys step operators and keep the row.
    p.act(Action::Page(1));
    assert_eq!(heading(&s), "Operator 6");
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 3 of 5"), "the same row of the next operator");
    p.act(Action::Page(-1));
    p.act(Action::Page(-1));
    assert_eq!(heading(&s), "Operator 4");
    for _ in 0..3 {
        p.act(Action::Page(-1));
    }
    assert_eq!(heading(&s), "Operator 1");
}

// The dial walks the overview (the view's own parameters, then the operators) between the page rail and the tiles; looking at
// an operator is not an edit, so the detail follows the cursor; the algorithm needs a press and a press to leave.
#[test]
fn the_dial_walks_the_overview_and_the_detail_follows_without_an_edit() {
    let list = plugin(8, true);
    let (s, mut p) = open_at(&list, (1280, 720), 1.55, &all_themes()[0], Side);
    sent(&mut p.out);
    let rings = |s: &Screen| panels(&s.ui)[0].shapes.iter().filter(|x| x.ink == 12).count();
    assert_eq!(rings(&s), 0);
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(cursor_tile(&s), Some(0), "the first move lands on the first tile");
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!((cursor_tile(&s), rings(&s)), (None, 1), "one step back is the overview, on the operator in view");
    assert_eq!(heading(&s), "Operator 1");
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(heading(&s), "Operator 2", "one step on is the next operator, selected as the cursor reaches it");
    assert_eq!(rings(&s), 1, "one cursor at a time");
    let out = sent(&mut p.out);
    assert!(out.iter().all(|o| o.0 != "params.set" && o.0 != "params.gesture"), "walking the overview sends no edit: {out:?}");
    assert_eq!(&param_ids(&out)[..2], [102, 202], "the knobs follow the operator in view");
    // A press on an operator takes the cursor into its tiles.
    p.act(Action::Confirm(None));
    assert_eq!(cursor_tile(&s), Some(0));
    // Up to the overview and down again lands on the same tile; a step on in between is the next operator.
    p.act(Action::Move { pane: None, by: 2 });
    assert_eq!(cursor_tile(&s), Some(2));
    p.act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!((cursor_tile(&s), heading(&s)), (None, "Operator 2".to_string()));
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(heading(&s), "Operator 3");
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(cursor_tile(&s), Some(2), "the tile it left");
    assert!(tile_names(&s).iter().all(|n| n.ends_with(" 3")));
    // The algorithm: moved to, it changes nothing; a press gives it the dial and a gesture; a press ends it.
    p.act(Action::Arrow { dx: 0, dy: -1 });
    p.act(Action::Move { pane: None, by: -3 });
    assert_eq!(heading(&s), "Operator 3", "the view's own parameters do not select an operator");
    assert_eq!(rings(&s), 1);
    sent(&mut p.out);
    p.act(Action::Move { pane: None, by: -1 });
    let out = sent(&mut p.out);
    assert!(out.iter().all(|o| o.0 != "params.set"), "a move onto the algorithm changes nothing");
    p.act(Action::Confirm(None));
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(out.iter().filter(|o| o.0 == "params.set").count(), 1);
    assert_eq!((out.first().unwrap(), out.last().unwrap()), (&gesture(1, true), &gesture(1, false)));
}

fn detail_area(s: &Screen) -> (f32, f32) {
    let g = s.ui.global::<GridState>();
    (g.get_view_width(), g.get_view_height())
}

// The split at the three screens, 8, 6 and 4 operators, with the pages at the side, over the tiles, and no pages at all:
// the overview as high as it needs, a row of normal tiles under it, a line for each operator and none for the missing.
#[test]
fn the_overview_and_the_detail_share_the_area_at_the_three_screens() {
    let mut report = String::new();
    for ops in [8usize, 6, 4] {
        for (size, scale) in [((1280, 720), 1.55f32), ((1280, 720), 1.), ((800, 480), 1.)] {
            for (name, pages, placement) in [("no pages", false, Top), ("pages at the side", true, Side), ("pages over", true, Top)] {
                let list = plugin(ops, pages);
                let t = &all_themes()[0];
                let (s, _p) = open_at(&list, size, scale, t, placement);
                let at = format!("{ops} operators, {size:?}@{scale}, {name}");
                let (w, h) = detail_area(&s);
                let all = panels(&s.ui);
                assert_eq!(all.len(), 2, "the overview and the detail's heading {at}");
                let (top, head) = (&all[0], &all[1]);
                check_panel(top, ops, 13. * t.data.font_scale, &at);
                let tiles: Vec<crate::GridTile> = s.ui.global::<GridState>().get_tiles().iter().collect();
                assert!(!tiles.is_empty(), "a row of tiles {at}");
                assert!(tiles.iter().all(|t| (t.h == 84. || t.h == 120.) && t.w >= 200.), "tiles of the grid's size {at}: {:?}", tiles.iter().map(|t| (t.w, t.h)).collect::<Vec<_>>());
                let lines = top.boxes.iter().filter(|b| b.edge == -1 && !b.text.is_empty() && (16..24).contains(&b.fill) || b.edge == -1 && !b.text.is_empty() && b.fill >= 32).count();
                assert_eq!(lines, ops, "a line for each operator and none more {at}");
                assert!(s.ui.global::<GridState>().get_tabs().row_count() == 0, "no tab {at}");
                let bottom = tiles.iter().map(|t| t.y + t.h).fold(0., f32::max);
                let fits = bottom <= h + 0.5;
                assert!(fits || (ops == 8 && placement == Top && pages), "the block fits the area {at}: ends at {bottom} of {h}");
                report += &format!("{at}: area {w:.0}x{h:.0}, overview {:.0}, heading {:.0}, tiles {:.0}x{:.0} x{} in view, block ends {bottom:.0}{}\n", top.h, head.h, tiles[0].w, tiles[0].h, tiles.len(), if fits { "" } else { " (past the area)" });
            }
        }
    }
    println!("{report}");
}

// Pictures for a look by eye: target/theme-shots/master-OPS-SELECTED-PLACEMENT-THEME@SCALE.png.
#[test]
fn master_detail_pictures() {
    for ops in [8usize, 6, 4] {
        let picks: &[usize] = match ops {
            8 => &[1, 5, 8],
            6 => &[1, 5, 6],
            _ => &[1, 3, 4],
        };
        let list = plugin(ops, true);
        for (placement, pname) in [(Side, "side"), (Top, "top")] {
            for (size, scale) in [((1280, 720), 1.55f32), ((1280, 720), 1.), ((800, 480), 1.)] {
                let themes = all_themes();
                for (ti, t) in themes.iter().enumerate() {
                    // Every theme at the keyboard's screen; the dark one at the other two.
                    if scale != 1.55 && ti != 0 {
                        continue;
                    }
                    let (mut s, mut p) = open_at(&list, size, scale, t, placement);
                    for &n in picks {
                        tap_operator(&mut p, &s, n);
                        s.frame();
                        shot(&s, &format!("master-{ops}-{n}-{pname}-{}@{scale}-{}x{}", t.id, size.0, size.1));
                    }
                }
            }
        }
    }
}
