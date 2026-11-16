// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FM operators view as a master and a detail on one page (ui/GRID.md "Master and detail", ui/TABS.md "Tabs that
//! are indexes"): the overview of every operator on top, the selected one's parameters as compact controls under it; the
//! page of tabs that were the operators' indexes folded into it; selection by the dial and by touch reaching the diagram,
//! the graph, the table and the detail; the knobs under the screen; rows paged. The table, the stages, mute and solo and
//! the split at the screens' sizes are in tests/fm_table.rs.

use super::fm::*;
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

// Selecting an operator, by a tap on its table row, reaches the diagram, the graph, the table and the detail at once.
#[test]
fn selecting_an_operator_reaches_the_diagram_the_graph_the_table_and_the_detail() {
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
        let chosen_rows = panel.boxes.iter().filter(|b| b.fill == 4 && b.edge == 3 && b.text.is_empty()).count();
        let row = panel.boxes.iter().find(|b| b.fill == 4 && b.edge == 3 && b.text.is_empty()).map(|b| (b.y, b.h));
        let chip = panel.boxes.iter().filter(|b| b.edge == -1 && b.text == n.to_string() && b.fill >= 16).last().map(|b| b.y + b.h / 2.);
        let in_row = matches!((row, chip), (Some((y, h)), Some(c)) if c > y && c < y + h);
        let detail = &panels(&s.ui)[1];
        let tinted = detail.shapes.iter().next().is_some_and(|x| x.fill && x.ink == ink);
        (ring_around(s, n), thick && thin_others, chosen_rows == 1 && in_row, tinted && heading(s) == format!("Operator {n}"))
    };
    assert_eq!(facts(&s, 1), (true, true, true, true), "operator 1 at the start");
    for n in [5, 8, 3] {
        tap_operator(&mut p, &s, n);
        let names = tile_names(&s);
        // The heading says which operator; the controls under it do not say the number again.
        assert!(names.contains(&"Ratio".to_string()) && (1..=8).all(|m| !names.iter().skip(8).any(|t| t.ends_with(&format!(" {m}")))), "the detail's controls carry no operator number ({n}): {names:?}");
        assert_eq!(facts(&s, n), (true, true, true, true), "diagram ring, thick line, table row and detail heading of operator {n}");
    }
    assert!(s.ui.global::<GridState>().get_tabs().row_count() == 0, "no tab for an operator");
}

// The detail has the selected operator's parameters and only its: its envelope's stages first as the Time and Level rows
// (named, not numbered), then the others in the plugin's priority order; the knobs under the screen play the table's row first.
#[test]
fn the_detail_lists_the_selected_operators_parameters_and_the_knobs_play_its_row() {
    let mut list = plugin(8, false);
    // Operator 5's ratio first, its rate 4 second, its level third.
    for p in list["parameters"].as_array_mut().unwrap() {
        let id = p["id"].as_u64().unwrap();
        p["priority"] = json!(match id {
            205 => 1,
            654 => 2, // rate 4 of operator 5 (600 + 10 n + k)
            105 => 3,
            _ => 0,
        });
    }
    let (s, mut p) = open_at(&list, (1280, 720), 1., &all_themes()[0], Top);
    sent(&mut p.out);
    tap_operator(&mut p, &s, 5);
    let out = sent(&mut p.out);
    // 13 parameters: the four rates and four levels (their stages, in two rows), then the others by priority.
    assert_eq!(tile_names(&s), ["Attack", "Decay 1", "Decay 2", "Release", "Attack", "Decay 1", "Decay 2", "Release", "Ratio", "Level", "Fine", "Detune", "Level 0"], "operator 5's, stages by name");
    let tiles: Vec<crate::GridTile> = s.ui.global::<GridState>().get_tiles().iter().collect();
    assert!(tiles.iter().all(|t| t.compact && t.h >= 44.), "compact controls, never below a touch target");
    assert_eq!(param_ids(&out)[..5], [105, 205, 305, 355, 654], "the table's row (level, ratio, fine, detune), then the rest by priority");
    // Another operator: the knobs follow.
    tap_operator(&mut p, &s, 2);
    let out = sent(&mut p.out);
    assert_eq!(&param_ids(&out)[..4], [102, 202, 302, 352], "operator 2's level, ratio, fine and detune first");
    // Selection costs no parameter change.
    assert!(out.iter().all(|o| o.0 != "params.set"));
}

// The rows of controls a detail has more of than fits: the dial pages by row past the last or first control in view, the
// arrows of the heading by touch, the page keys step the operator and keep the row.
#[test]
fn rows_page_by_the_dial_by_touch_and_the_page_keys_step_operators() {
    use super::fm_table::{open_at as table_open, ENGINE};
    // 826 x 465: eight to a row, one row in view of the 22 other parameters of an engine's operator: three rows.
    let (s, mut p) = table_open(ENGINE, (1280, 720), 1.55, &super::fm_table::dark(), Side, 1);
    let in_view = |s: &Screen| tile_names(s).len();
    assert_eq!(in_view(&s), 9 + 8, "the nine stage controls and one row of eight");
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 1 of 3"), "the heading says which row");
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(cursor_tile(&s), Some(0));
    p.act(Action::Move { pane: None, by: 16 });
    assert_eq!(cursor_tile(&s), Some(16), "the last control of the row in view");
    let first_row = tile_names(&s);
    p.act(Action::Move { pane: None, by: 1 });
    assert_ne!(tile_names(&s), first_row, "the next row of controls");
    assert_eq!(cursor_tile(&s), Some(9), "the cursor at the start of the new row");
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 2 of 3"));
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!((tile_names(&s), cursor_tile(&s)), (first_row.clone(), Some(16)), "and back a row, at the end of it");
    // Down by the arrows, in the column the cursor is in.
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(cursor_tile(&s), Some(16));
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 2 of 3"));
    // By touch: the heading's down arrow.
    let head = panels(&s.ui)[1].clone();
    let (x, y) = (head.w - 22., head.h / 2.);
    assert!(p.view_down(1, x, y));
    p.view_up(1, x, y);
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 3 of 3"), "a tap on the down arrow pages a row");
    // The page keys step operators and keep the row.
    p.act(Action::Page(1));
    assert_eq!(heading(&s), "Operator 2");
    assert!(panels(&s.ui)[1].labels.iter().any(|l| l.text == "Row 3 of 3"), "the same row of the next operator");
    // The heading's selector steps the operator by touch, too, and costs no edit.
    sent(&mut p.out);
    let head = panels(&s.ui)[1].clone();
    assert!(p.view_down(1, 22. + 44., head.h / 2.));
    p.view_up(1, 22. + 44., head.h / 2.);
    assert_eq!(heading(&s), "Operator 3");
    assert!(p.view_down(1, 22., head.h / 2.));
    p.view_up(1, 22., head.h / 2.);
    assert_eq!(heading(&s), "Operator 2");
    assert!(sent(&mut p.out).iter().all(|o| o.0 != "params.set"));
    p.act(Action::Page(-1));
    assert_eq!(heading(&s), "Operator 1");
    p.act(Action::Page(-1));
    assert_eq!(heading(&s), "Operator 1");
}

// The dial walks the overview (the view's own parameters, then the table's cells in reading order) between the page rail
// and the controls; looking at a row is not an edit, so the detail follows the cursor; a press on a cell edits it, on the
// algorithm gives it the dial, and a press ends it.
#[test]
fn the_dial_walks_the_overview_and_the_detail_follows_without_an_edit() {
    use super::fm_table::{open_at as table_open, ENGINE};
    let (s, mut p) = table_open(ENGINE, (1280, 720), 1.55, &super::fm_table::dark(), Side, 1);
    sent(&mut p.out);
    let rings = |s: &Screen| panels(&s.ui)[0].shapes.iter().filter(|x| x.ink == 12).count();
    assert_eq!(rings(&s), 0);
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(cursor_tile(&s), Some(0), "the first move lands on the first control");
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!((cursor_tile(&s), rings(&s)), (None, 1), "one step back is the table, on the operator in view");
    assert_eq!(heading(&s), "Operator 1");
    // Seven columns to a row: seven on is the next row's first cell, and the operator shown follows the cursor.
    p.act(Action::Move { pane: None, by: 7 });
    assert_eq!(heading(&s), "Operator 2", "the next row, selected as the cursor reaches it");
    assert_eq!(rings(&s), 1, "one cursor at a time");
    let out = sent(&mut p.out);
    assert!(out.iter().all(|o| o.0 != "params.set" && o.0 != "params.gesture"), "walking the table sends no edit: {out:?}");
    assert_eq!(&param_ids(&out)[..2], [1200, 1201], "the knobs follow the operator in view");
    // Down by the arrows: the rows; down from the last row: the controls; up again: the table, on the same operator.
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(heading(&s), "Operator 3");
    for _ in 0..5 {
        p.act(Action::Arrow { dx: 0, dy: 1 });
    }
    assert_eq!(heading(&s), "Operator 8");
    p.act(Action::Arrow { dx: 0, dy: 1 });
    assert!(cursor_tile(&s).is_some(), "past the last row: the controls");
    p.act(Action::Arrow { dx: 0, dy: -1 });
    assert_eq!((cursor_tile(&s), heading(&s)), (None, "Operator 8".to_string()), "the table again, on the operator it left");
    // The algorithm: moved to, it changes nothing; a press gives it the dial and a gesture; a press ends it.
    for _ in 0..8 {
        p.act(Action::Arrow { dx: 0, dy: -1 });
    }
    assert_eq!(heading(&s), "Operator 1", "up from the first row is the view's own parameters, which select no operator");
    assert_eq!(rings(&s), 1);
    sent(&mut p.out);
    p.act(Action::Move { pane: None, by: 0 });
    assert!(sent(&mut p.out).iter().all(|o| o.0 != "params.set"), "a move onto the algorithm changes nothing");
    p.act(Action::Confirm(None));
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Confirm(None));
    let out = sent(&mut p.out);
    assert_eq!(out.iter().filter(|o| o.0 == "params.set").count(), 1);
    assert_eq!((out.first().unwrap(), out.last().unwrap()), (&gesture(1, true), &gesture(1, false)));
}
