// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pages and tabs model (src/tabs.rs): filled from the module paths it is today's pages and tabs, filled from
//! what a plugin declares (PROVISIONAL fields of `params.list`: `uiTabs`, `uiTab`, `valueIcons`; a fake engine
//! gives them here) it is what the plugin said, and the widgets read the same thing either way; the declared icons
//! of a stepped parameter's values.

use super::shots::open_with;
use super::*;
use crate::grid::{self, Param};
use crate::params::{after, editor_pages, editor_parameters, level, tabs_for};
use crate::tabs::{declared_in, Model};
use crate::GridState;
use slint::Model as _;

/// The parameters an editor shows, and the pages and tabs as they were before the model: a page per top-level
/// module, a tab per second-level module on a page of more than 24; the headings of a page's (or tab's) groups.
/// A page, its tab, and the headings with their parameters.
type Before = (String, Option<String>, Vec<(String, Vec<usize>)>);

fn today(params: &[Param]) -> Vec<Before> {
    let mut out = Vec::new();
    for (name, members) in editor_pages(params) {
        let tabs = tabs_for(params, &members);
        let shown: Vec<Option<String>> = if tabs.is_empty() { vec![None] } else { tabs.into_iter().map(Some).collect() };
        for tab in shown {
            let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
            for &i in &members {
                let m = &params[i].module;
                let heading = match &tab {
                    None => after(m, 1),
                    Some(t) if level(m, 1) == t => after(m, 2),
                    Some(_) => continue,
                };
                match groups.iter_mut().find(|g| g.0 == heading) {
                    Some(g) => g.1.push(i),
                    None => groups.push((heading, vec![i])),
                }
            }
            out.push((name.clone(), tab, groups));
        }
    }
    out
}

fn fixtures() -> Vec<(&'static str, serde_json::Value)> {
    // A plugin of more than 24 parameters with no module paths: modules from the leading words of its names.
    let words: Vec<serde_json::Value> = ["Osc A", "Osc B", "Filter", "Amp"]
        .iter()
        .flat_map(|w| (1..=7).map(move |k| (w, k)))
        .enumerate()
        .map(|(i, (w, k))| json!({"id": i, "name": format!("{w} Control {k}"), "module": "", "minimum": 0.0, "maximum": 1.0, "defaultValue": 0.0, "stepped": false, "flags": 0, "widget": "knob", "priority": 0, "group": 0}))
        .collect();
    // Numbered pages ("Part 10" after "Part 2"), the second level only on the page that is long enough.
    let mut numbered = Vec::new();
    for page in [10, 2, 1] {
        for k in 0..3 {
            numbered.push(json!({"id": numbered.len(), "name": format!("P{k}"), "module": format!("Part {page}/Group {k}"), "minimum": 0.0, "maximum": 1.0, "defaultValue": 0.0, "stepped": false, "flags": 0, "widget": "knob", "priority": 0, "group": 0}));
        }
    }
    vec![
        ("five pages", super::tabs::five_pages()),
        ("two pages", super::grid::every_kind()),
        ("search's three pages", super::search::big_plugin()),
        ("the basic synth", super::grid::basic_synth()),
        ("the fm plugin", super::fm::fm_n(&[super::fm::parallel()])),
        ("no modules", json!({ "parameters": words })),
        ("numbered pages", json!({ "parameters": numbered })),
    ]
}

// The model from the module paths is what the screen had before it: the same pages in the same order, the same
// tabs, and on each page and tab the same headings with the same parameters, for every fixture the tests have.
#[test]
fn the_model_inferred_from_module_paths_is_the_pages_and_tabs_of_today() {
    for (name, list) in fixtures() {
        let params = editor_parameters(grid::parse(&list));
        let model = Model::inferred(&params);
        let entries = model.entries();
        let before = today(&params);
        assert_eq!(entries.len(), before.len(), "{name}: the entries");
        for (e, (page, tab, groups)) in entries.iter().zip(&before) {
            assert_eq!(&model.pages[e.page].name, page, "{name}: the page");
            assert_eq!(e.tab.map(|t| &model.pages[e.page].tabs[t]), tab.as_ref(), "{name}: the tab");
            assert_eq!(&model.groups(e.page, e.tab), groups, "{name}: the headings and parameters of {page} / {tab:?}");
        }
        // Every parameter is on a page, once; none is above tabs (inference puts each on a tab).
        let placed: usize = model.pages.iter().map(|p| p.params.len()).sum();
        assert_eq!(placed, params.len(), "{name}");
        assert!(model.pages.iter().all(|p| p.params.iter().all(|x| x.tab.is_some() != p.tabs.is_empty())), "{name}");
    }
    // The cases the fixtures stand for, spelled out: eight operators are eight tabs of the first page, and a page
    // of 24 or fewer has none.
    let params = editor_parameters(grid::parse(&super::tabs::five_pages()));
    let model = Model::inferred(&params);
    assert_eq!(model.pages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Operators", "Filter", "Amp", "Effects", "Global"]);
    assert_eq!(model.pages[0].tabs, (1..=8).map(|n| format!("Operator {n}")).collect::<Vec<_>>());
    assert!(model.pages[1..].iter().all(|p| p.tabs.is_empty()));
    assert_eq!(model.entries().len(), 8 + 4);
}

/// A made-up plugin that declares its pages and tabs (PROVISIONAL fields, see src/tabs.rs `Declared`): the
/// declaration says what the module paths do not (the order, the names), a parameter on a page itself, ones it
/// leaves out, and entries a host must drop.
fn declared_plugin() -> serde_json::Value {
    let param = |id: u32, name: &str, module: &str, tab: Option<u32>| {
        let mut p = json!({"id": id, "name": name, "module": module, "minimum": 0.0, "maximum": 1.0, "defaultValue": 0.0, "stepped": false, "flags": 0, "widget": "knob", "priority": 0, "group": 0});
        if let Some(t) = tab {
            p["uiTab"] = json!(t);
        }
        p
    };
    json!({
        "uiTabs": [
            {"id": 1, "parent": 0, "name": "Sound"},
            {"id": 2, "parent": 0, "name": "Mod"},
            {"id": 3, "parent": 1, "name": "Osc"},
            {"id": 4, "parent": 1, "name": "Filter"},
            {"id": 5, "parent": 2, "name": "LFO"},
            {"id": 3, "parent": 2, "name": "an id used twice"},
            {"id": 6, "parent": 3, "name": "a tab under a tab"},
            {"id": 7, "parent": 1, "name": ""},
            {"id": 8, "parent": 99, "name": "a tab under nothing"},
        ],
        "parameters": [
            param(10, "Wave", "Voice/Osc", Some(3)),
            param(11, "Tune", "Voice/Osc", Some(3)),
            param(12, "Cutoff", "Voice/Filter", Some(4)),
            param(13, "Master", "Voice", Some(1)),
            param(14, "Rate", "LFO", Some(5)),
            param(15, "Depth", "Mod", None),
            param(16, "Odd", "Extra", None),
            param(17, "Odder", "", Some(77)),
        ],
    })
}

// The model from a declaration: the pages and tabs as the plugin ordered and named them, a parameter on a page
// above its tabs whichever is open, the entries ui.h says a host drops dropped, a page of one tab drawn without
// it, and what the plugin did not place put where its module path puts it.
#[test]
fn the_model_from_declared_data_is_what_the_plugin_said() {
    let list = declared_plugin();
    let params = grid::parse(&list);
    let declared = declared_in(&list).expect("declared");
    assert_eq!(declared.entries.len(), 9);
    let model = Model::build(&params, Some(&declared));
    let names = |ids: &[usize]| ids.iter().map(|&i| params[i].name.as_str()).collect::<Vec<_>>();
    let page_names: Vec<&str> = model.pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(page_names, ["Sound", "Mod", "Extra", "Main"], "the declared pages in order, then a page for what has none: the module path's, and Main for no path");
    assert_eq!(model.pages[0].tabs, ["Osc", "Filter"], "the duplicate id, the tab under a tab, the empty name and the orphan are dropped");
    assert!(model.pages[1].tabs.is_empty(), "Mod has one tab: drawn without it");
    // Sound: Master is on the page, above both tabs; the others on their tabs.
    let sound = |tab| model.groups(0, tab).into_iter().flat_map(|g| g.1).collect::<Vec<_>>();
    assert_eq!(names(&sound(Some(0))), ["Master", "Wave", "Tune"]);
    assert_eq!(names(&sound(Some(1))), ["Master", "Cutoff"]);
    // Mod: the one tab's parameter (Rate) and the one with a path "Mod" and no declaration (Depth), together.
    assert_eq!(names(&model.groups(1, None).into_iter().flat_map(|g| g.1).collect::<Vec<_>>()), ["Rate", "Depth"]);
    assert_eq!(names(&model.groups(2, None).into_iter().flat_map(|g| g.1).collect::<Vec<_>>()), ["Odd"]);
    // A parameter naming an entry that does not exist goes where its (empty) path puts it.
    assert_eq!(names(&model.groups(3, None).into_iter().flat_map(|g| g.1).collect::<Vec<_>>()), ["Odder"]);
    // The screen steps through the same: a page without tabs once, a page with two tabs twice.
    assert_eq!(model.entries().iter().map(|e| (e.page, e.tab)).collect::<Vec<_>>(), [(0, Some(0)), (0, Some(1)), (1, None), (2, None), (3, None)]);
    // With no declaration the same parameters get the module paths' pages.
    assert_eq!(Model::build(&params, None), Model::inferred(&params));
    assert_eq!(declared_in(&json!({"parameters": []})), None);
    // At most 16 pages, and 16 tabs under a page.
    let many: Vec<serde_json::Value> = (1..=20).map(|i| json!({"id": i, "parent": 0, "name": format!("Page {i}")})).chain((21..=40).map(|i| json!({"id": i, "parent": 1, "name": format!("Tab {i}")}))).collect();
    let params: Vec<Param> = (1..=40).map(|i| Param { id: i, name: format!("P{i}"), declared_tab: Some(i), ..Default::default() }).collect();
    let big = Model::declared(&params, &declared_in(&json!({"uiTabs": many})).unwrap());
    assert_eq!(big.pages.iter().filter(|p| p.name.starts_with("Page ")).count(), 16, "the declared pages past the sixteenth are skipped");
    assert_eq!(big.pages[0].tabs.len(), 16, "and the tabs past the sixteenth under a page");
}

// The screen reads the same model whatever fills it: the pages and tabs the plugin declared reach the widgets, a
// page's own parameters show on every tab, and a tab opens the parameters placed on it.
#[test]
fn the_screen_shows_declared_pages_and_tabs() {
    let list = declared_plugin();
    let mut s = Screen::new();
    let ui = s.ui.clone_strong();
    let g = ui.global::<GridState>();
    let mut p = open_with(&mut s, list, &[], &|_, _| "t".to_string());
    let names = |ui: &Shell| ui.global::<GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect::<Vec<_>>();
    let strings = |m: slint::ModelRc<slint::SharedString>| m.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(strings(g.get_pages()), ["Sound", "Mod", "Extra", "Main"]);
    assert_eq!(strings(g.get_tabs()), ["Osc", "Filter"]);
    assert_eq!(names(&ui), ["Master", "Wave", "Tune"]);
    p.show_tab(1);
    assert_eq!((g.get_tab(), names(&ui)), (1, vec!["Master".to_string(), "Cutoff".to_string()]));
    p.show_page(1);
    assert_eq!((strings(g.get_tabs()), names(&ui)), (vec![], vec!["Rate".to_string(), "Depth".to_string()]));
}

// A value's icon is the plugin's where it declared one (PROVISIONAL `valueIcons`: enum oscairn_icon from the
// value's minimum), else the name rule; where any value has a declared icon, no icon is guessed for the others.
#[test]
fn a_declared_icon_comes_before_the_name_rule() {
    use crate::grid::{icon, icons_for};
    // The lookup: declared first (ui.h's numbers: 1 sine, 2 triangle, 3 saw, 4 saw down, 5 square, 6 pulse,
    // 7 noise, 8 sample and hold), the widget's own numbers out; none declared, the name.
    assert_eq!([1, 2, 3, 4, 5, 6, 7, 8].map(|d| icon(Some(d), "")), [3, 4, 1, 7, 2, 6, 5, 8]);
    assert_eq!(icon(Some(5), "Sine"), 2, "the declaration wins over the name");
    assert_eq!(icon(None, "Sine"), 3, "no declaration: the name rule");
    assert_eq!(icon(Some(0), "Saw"), 1, "an icon of none, or one the host does not know, falls back to the name");
    assert_eq!(icon(Some(99), "Square"), 2);
    assert_eq!(icon(None, "Wave 7"), 0);
    let labels: Vec<String> = ["Saw", "Sine", "Wave 3", "Square"].iter().map(|s| s.to_string()).collect();
    let mut p = Param { id: 1, stepped: true, max: 3., ..Default::default() };
    assert_eq!(icons_for(&p, &labels), [1, 3, 0, 2], "none declared: every value's name");
    p.icons = vec![0, 0, 3, 0];
    assert_eq!(icons_for(&p, &labels), [0, 0, 1, 0], "one declared: the host guesses none for the others");
    p.icons = vec![5, 1, 2, 8];
    assert_eq!(icons_for(&p, &labels), [2, 3, 4, 8]);
    // Through the engine's words and the tile: a split button whose texts name no waveform draws the declared icons.
    let list = json!({"parameters": [{"id": 1, "name": "Wave", "module": "", "minimum": 0.0, "maximum": 3.0, "defaultValue": 0.0, "stepped": true, "flags": 0, "widget": "buttons", "priority": 1, "group": 0, "valueIcons": [1, 2, 3, 5]}]});
    let mut s = Screen::new();
    let ui = s.ui.clone_strong();
    let _p = open_with(&mut s, list, &[(1, 0.)], &|_, v| format!("Mode {v}"));
    let tile = ui.global::<GridState>().get_tiles().row_data(0).unwrap();
    assert_eq!(tile.icons.iter().collect::<Vec<_>>(), [3, 4, 1, 2]);
}
