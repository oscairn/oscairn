// SPDX-License-Identifier: AGPL-3.0-or-later
//! The search on the parameter pages (ParameterGrid's search field: any page's parameters whose name or
//! module has the text, each module's under its whole path, no tabs, none of the pages the one shown;
//! shown for a plugin of more than 24 parameters), by touch, by the cursor and the on-screen keyboard.

use super::shots::*;
use super::*;
use crate::nav::Action;
use crate::params::Outcome;

/// Thirty parameters on three pages: Oscillator, Filter, and Amp (its Env module under it).
pub(super) fn big_plugin() -> serde_json::Value {
    let names = [
        ("Oscillator", ["Pitch", "Fine", "Wave", "Level", "Pan", "Sync", "Phase", "Shape", "Detune", "Unison"]),
        ("Filter", ["Cutoff", "Resonance", "Drive", "Type", "Keytrack", "Envelope amount", "Mix", "Slope", "Pan", "Level"]),
        ("Amp/Env", ["Attack", "Decay", "Sustain", "Release", "Level", "Velocity", "Pan", "Gain", "Hold", "Delay"]),
    ];
    let mut list = Vec::new();
    for (m, page) in names {
        for name in page {
            let mut p = plugin_param(list.len() as u32, name, 1., false, "slider", 0, 0);
            p["module"] = json!(m);
            list.push(p);
        }
    }
    json!({ "parameters": list })
}

fn names(ui: &Shell) -> Vec<String> {
    ui.global::<crate::GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect()
}

fn headings(ui: &Shell) -> Vec<String> {
    ui.global::<crate::GridState>().get_headings().iter().map(|h| h.text.to_string()).collect()
}

fn text(_: u32, v: f64) -> String {
    format!("{v:.2}")
}

// A plugin of more than 24 parameters has the field; a search lists the matches of every page under their
// module's path, by name or module, whatever the case, with no tabs and no page marked; choosing a page ends it.
#[test]
fn the_search_lists_matches_from_every_page() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let g = ui.global::<crate::GridState>();
    let mut p = open_with(&mut s, big_plugin(), &[], &text);
    assert!(g.get_search_shown());
    assert_eq!(g.get_pages().row_count(), 3);
    assert_eq!(names(ui).len(), 10, "the first page");
    sent(&mut p.out);
    // The text arrives from the keyboard: Pan is on three pages, and the values of those are asked for.
    p.entered("pan");
    assert_eq!(names(ui), ["Pan", "Pan", "Pan"]);
    assert_eq!(headings(ui), ["Oscillator", "Filter", "Amp \u{203a} Env"], "each under its module's whole path");
    assert_eq!((g.get_page(), g.get_tabs().row_count(), g.get_search().as_str()), (-1, 0, "pan"), "no page is the one shown, no tabs");
    let asked = sent(&mut p.out);
    let get = asked.iter().find(|a| a.0 == "params.get").expect("the matches' values asked for");
    let ids: Vec<u64> = get.1["values"].as_array().unwrap().iter().map(|v| v["param"].as_u64().unwrap()).collect();
    assert_eq!(ids, [4, 18, 26]);
    // A module's name matches too, and the case does not matter.
    p.entered("FILTER");
    assert_eq!(names(ui).len(), 10);
    assert_eq!(headings(ui), ["Filter"]);
    p.entered("  cut ");
    assert_eq!(names(ui), ["Cutoff"], "the text is trimmed");
    p.entered("zzz");
    assert!(names(ui).is_empty() && headings(ui).is_empty());
    // Choosing a page ends the search and shows the page.
    p.entered("level");
    assert_eq!(names(ui), ["Level", "Level", "Level"]);
    p.show_page(1);
    assert_eq!((g.get_search().as_str(), g.get_page()), ("", 1));
    assert_eq!(names(ui)[..3], ["Type", "Cutoff", "Resonance"], "the filter by name is one run, in the filter's order");
    // An empty text ends it too; a plugin of 24 or fewer has no field.
    p.entered("pan");
    p.entered("");
    assert_eq!((names(ui).len(), g.get_page()), (10, 1));
    let mut s = Screen::new();
    let mut list = big_plugin();
    list["parameters"].as_array_mut().unwrap().truncate(24);
    let _q = open_with(&mut s, list, &[], &text);
    assert!(!s.ui.global::<crate::GridState>().get_search_shown());
}

// Without touch: back from the first tile the cursor is on the row of pages and goes no further (the search is
// a button of the top bar, which the dial reaches from the first stop: params.rs `at_top`); the bar's Search asks
// for the text and says what was searched for; a page key ends the search.
#[test]
fn the_search_by_the_cursor() {
    let mut s = Screen::new();
    // The pages a strip over the tiles (tabs.rs has the rails').
    s.ui.global::<crate::Theme>().set_tab_placement(crate::TabPlacement::Top);
    let ui = &s.ui.clone_strong();
    let g = ui.global::<crate::GridState>();
    let mut p = open_with(&mut s, big_plugin(), &[], &text);
    let on_tile = || ui.global::<crate::GridState>().get_tiles().iter().position(|t| t.cursor);
    let label = |p: &crate::params::Screen| p.bar_tools().into_iter().map(|t| t.label).collect::<Vec<_>>();
    assert_eq!(label(&p), ["Search"]);
    assert!(!p.at_top(), "no cursor yet");
    p.act(Action::Move { pane: None, by: 1 });
    assert_eq!(on_tile(), Some(0));
    assert!(!p.at_top(), "a tile under the pages is not the first stop");
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!((on_tile(), g.get_page_cursor()), (None, 0), "before the first tile: the pages, on the one shown");
    assert!(p.at_top(), "the pages are the first stop: moving up leaves for the bar");
    p.act(Action::Move { pane: None, by: -1 });
    assert_eq!((on_tile(), g.get_page_cursor()), (None, 0), "nothing before the pages");
    // The bar's button asks for the text.
    assert_eq!(p.search_tap(), Outcome::Prompt("Search parameters".into(), String::new()));
    p.entered("res");
    assert_eq!(names(ui), ["Resonance"]);
    assert_eq!(label(&p), ["Search: res"]);
    // Back leaves the screen from anywhere; a dropdown that is open closes first.
    assert_eq!(p.back(), Outcome::Back);
    // Turning the page ends a search, too.
    p.entered("pan");
    p.act(Action::Page(1));
    assert_eq!((g.get_search().as_str(), g.get_page()), ("", 1));
    assert_eq!(label(&p), ["Search"]);
}

// Pictures for a look by eye: the search button in the header, with a search and the cursor on it, at 1 and at the
// FANTOM's 1.55, and under the odd theme.
#[test]
fn search_pictures() {
    fit_to_window();
    let mut s = Screen::new();
    let mut p = open_with(&mut s, big_plugin(), &[], &text);
    s.frame();
    png(&s, "grid-search-empty");
    p.act(Action::Move { pane: None, by: 1 });
    p.act(Action::Arrow { dx: 0, dy: -1 });
    p.act(Action::Arrow { dx: 0, dy: -1 });
    p.entered("pan");
    s.frame();
    png(&s, "grid-search");
    at_fantom_scale(&mut s, "grid-search", || p.shown());
    theme::apply(&s.ui, &odd_theme());
    s.frame();
    p.shown();
    assert!(s.frame().is_some());
    shot(&s, "grid-search-odd");
}

// By touch: a tap on the field asks for the text; a touch on a tile lets the cursor go from it.
#[test]
fn the_search_by_touch() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let g = ui.global::<crate::GridState>();
    let mut p = open_with(&mut s, big_plugin(), &[], &text);
    assert_eq!(p.search_tap(), Outcome::Prompt("Search parameters".into(), String::new()));
    p.entered("level");
    p.tap(1, -1);
    assert_eq!(g.get_search().as_str(), "level", "a tile touched leaves the search");
    assert_eq!(p.search_tap(), Outcome::Prompt("Search parameters".into(), "level".into()));
}

// On the shell: the field opens the on-screen keyboard over the page, typing fills it, Return hands the text
// to the page, Cancel (Escape) closes it with nothing.
#[test]
fn the_search_on_the_shell_uses_the_keyboard() {
    let mut f = Fixture::new("search");
    shell::set_text_on_screen(false); // a typing keyboard: the keys typed go to the keyboard's field
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(0);
    let graph = super::part::part_graph();
    for _ in 0..11 {
        let request = f.engine.request();
        let result = match request["method"].as_str().unwrap() {
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => graph.clone(),
            "library.query" => json!({"items": [], "total": 0}),
            _ => json!({}),
        };
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        f.pump();
    }
    let ui = f.screen.ui.clone_strong();
    // The Part's chain, the plugin's Parameters: the screen and its list asked for, as the other shell test does.
    ui.global::<crate::PlayState>().invoke_part_tapped(0);
    shell::act(Action::End(1));
    shell::act(Action::End(1));
    shell::act(Action::Move { pane: None, by: -2 });
    shell::act(Action::Confirm(None));
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    let answer = |f: &mut Fixture, want: &str, result: serde_json::Value| loop {
        let r = f.engine.request();
        let method = r["method"].as_str().unwrap().to_string();
        let value = if method == want {
            result.clone()
        } else {
            match method.as_str() {
                "window.info" => json!({"available": false}),
                "params.text" => json!({"text": "t"}),
                "parts.meters" => json!({"meters": []}),
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "graph.get" => graph.clone(),
                "session.status" => json!({"midiInputs": []}),
                "controllers.view" => json!({"accepted": true}),
                _ => json!({}),
            }
        };
        f.reply(&r, json!({"result": value}));
        if method == want {
            return r;
        }
    };
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    assert_eq!(ui.get_screen(), 7);
    answer(&mut f, "params.list", big_plugin());
    assert!(ui.global::<crate::GridState>().get_search_shown());
    // The search is the top bar's first action: the dial up out of the first row reaches the bar, which ends
    // in the actions; a tap on the bar's Search opens the keyboard over the page.
    ui.global::<crate::Bar>().invoke_action(0);
    assert!(ui.global::<crate::KeyboardState>().get_open());
    assert_eq!(ui.global::<crate::KeyboardState>().get_title().as_str(), "Search parameters");
    for c in ["c", "u", "t"] {
        assert!(shell::key(c));
    }
    assert!(shell::key("\n"), "Return is Done");
    assert!(!ui.global::<crate::KeyboardState>().get_open());
    assert_eq!(ui.global::<crate::GridState>().get_search().as_str(), "cut");
    assert_eq!(ui.global::<crate::GridState>().get_tiles().iter().map(|t| t.name.to_string()).collect::<Vec<_>>(), ["Cutoff"]);
    // Again, and Escape closes it with nothing: the search is as it was.
    ui.global::<crate::Bar>().invoke_action(0);
    assert!(ui.global::<crate::KeyboardState>().get_open());
    assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Escape)));
    assert!(!ui.global::<crate::KeyboardState>().get_open());
    assert_eq!(ui.global::<crate::GridState>().get_search().as_str(), "cut");
}

fn view_of(out: &[(&'static str, serde_json::Value)]) -> Option<(String, Vec<u64>)> {
    out.iter().rfind(|o| o.0 == "controllers.view").map(|o| (o.1["screen"]["name"].as_str().unwrap().to_string(), o.1["screen"]["params"].as_array().unwrap().iter().map(|a| a["param"].as_u64().unwrap()).collect()))
}

// What the device's screen knobs play follows what the page shows (ParameterGrid::publishScreen, run at the end
// of every build): the page, another page, a search's matches; nothing is sent again while it is the same, and
// the knobs let go when the page hides and take it again when it shows.
#[test]
fn the_screen_knobs_follow_the_page_the_search_and_the_screen() {
    let mut s = Screen::new();
    let mut p = open_with(&mut s, big_plugin(), &[], &text);
    p.hidden();
    sent(&mut p.out);
    p.shown();
    assert_eq!(view_of(&sent(&mut p.out)), Some(("Oscillator".into(), (0..10).collect())), "shown again: the first page, in the plugin's order");
    p.show_page(1);
    let page = view_of(&sent(&mut p.out)).expect("another page is published");
    assert_eq!(page.0, "Filter");
    assert_eq!(page.1.len(), 10);
    p.shown();
    assert!(sent(&mut p.out).is_empty(), "the same page is not sent again");
    p.entered("pan");
    assert_eq!(view_of(&sent(&mut p.out)).map(|v| v.1), Some(vec![4, 18, 26]), "a search: its matches");
    p.hidden();
    assert_eq!(view_of(&sent(&mut p.out)), Some((String::new(), vec![])));
}
