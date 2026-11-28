// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app on the shell, through a socket to the fake engine of settings_fake.rs: the tile, the bar, a menu, the
//! on-screen keyboard and the questions over the screen, the status that makes a page list again, and that nothing at rest
//! costs a request or a frame.

use super::library::{info, labels, rows, value_of};
use super::settings::{folder, installed_theme, tabs};
use super::settings_fake::Settings as Fake;
use super::*;
use crate::{KeyboardState, MenuState, NoticeState, PartState};
use serde_json::{json, Value};
use slint::Model;
use std::time::Duration;

fn inst_pkg(id: &str, name: &str, version: &str, kind: &str, repository: &str, signers: &[&str]) -> Value {
    json!({"id": id, "version": version, "kind": kind, "repository": repository, "sha256": "", "signers": signers, "otherBuild": false, "withdrawn": null, "dependency": false, "name": name})
}

fn offered_pkg(id: &str, name: &str, version: &str, types: Value) -> Value {
    json!({"id": id, "name": name, "summary": "", "license": "", "publisher": "", "publisherKey": "", "tags": [],
           "versions": [{"version": version, "channel": "stable", "yanked": "", "incompatible": "", "sha256": "", "size": 1234, "installed": 0, "types": types, "dependencies": [], "withdrawn": null, "exports": []}]})
}

pub(super) struct Shelf {
    pub(super) f: Fixture,
    pub(super) e: Fake,
    pub(super) config: std::path::PathBuf,
}

impl Shelf {
    pub(super) fn new(name: &str) -> Shelf {
        Shelf::with(name, Fake::new())
    }

    pub(super) fn with(name: &str, e: Fake) -> Shelf {
        Shelf::on(name, Some("fantom-07"), e)
    }

    /// On a profile (a built-in's id; none: a computer's).
    pub(super) fn on(name: &str, profile: Option<&str>, e: Fake) -> Shelf {
        let mut f = Fixture::on(name, profile, true);
        f.engine.answer(); // session.hello
        f.pump();
        let config = folder(name);
        shell::settings_folders(Some(config.clone()), crate::theme::Places { built_in: None, installed: Some(config.join("themes")) });
        let mut s = Shelf { f, e, config };
        s.serve();
        s
    }

    /// Answers every request the shell makes until it is quiet for a moment, as a real engine would.
    pub(super) fn serve(&mut self) {
        let mut quiet = 0;
        while quiet < 3 {
            slint::platform::update_timers_and_animations(); // a step asked again later is a timer
            let Some(request) = self.f.engine.try_request(Duration::from_millis(60)) else {
                quiet += 1;
                continue;
            };
            quiet = 0;
            let method = request["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "graph.get" => json!({"scene": 0, "scenes": [], "parts": [], "nodes": []}),
                "edit.history" => json!({"undo": "", "redo": ""}),
                "subscribe" | "unsubscribe" | "controllers.view" | "memory.shell" => json!({"accepted": true}),
                "performance.reopen" => json!({"notice": ""}),
                "library.query" => json!({"items": [], "total": 0}),
                "packages.installed" => json!({"packages": self.e.installed_packages}),
                "record.takes" | "memory.state" | "transport.get" => json!({}), // the shell's own polls after a status
                _ => match self.e.handle(&method, &request["params"]) {
                    Ok(r) => r,
                    Err(e) => {
                        self.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "error": {"code": e.code, "message": e.message}})));
                        self.f.pump();
                        continue;
                    }
                },
            };
            self.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
            self.f.pump();
            for (method, params) in std::mem::take(&mut self.e.notifications) {
                self.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": method, "params": params})));
                self.f.pump();
            }
        }
    }

    pub(super) fn ui(&self) -> Shell {
        self.f.screen.ui.clone_strong()
    }

    /// The engine pushes its status.
    pub(super) fn push_status(&mut self) {
        self.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": self.e.status()})));
        self.f.pump();
        self.serve();
    }
}

// Home's tile hands over to the JUCE app until the whole app is built; with the switch on it is the Rust one, with the
// bar's title, Back and the tabs (S1).
#[test]
fn the_tile_opens_the_rust_app_only_when_it_is_switched_on() {
    let mut s = Shelf::new("settings-tile");
    let ui = s.ui();
    shell::settings_in_rust(false);
    ui.global::<crate::HomeState>().invoke_tapped(4);
    assert_eq!((ui.get_screen(), ui.get_opening().as_str(), shell::handover()), (2, "Settings", Some("settings")), "the JUCE app, as before");
    shell::goto(shell::Screen::Home);
    s.serve();
    shell::settings_in_rust(true);
    ui.global::<crate::HomeState>().invoke_tapped(4);
    s.serve();
    assert_eq!(ui.get_screen(), 14);
    assert_eq!(title(&ui), "Settings › Appearance");
    assert!(ui.global::<Bar>().get_back() && ui.global::<Bar>().get_home_button());
    assert_eq!(tabs(&ui), ["Appearance", "Top bar", "MIDI controllers", "Package repositories", "Installed", "Available", "Signing keys", "Start", "About"], "the keyboard's profile fixes its audio device");
    let actions: Vec<String> = ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect();
    assert_eq!(actions, ["Install...", "Record", "Capture"], "the page's own action and the two every screen has; Undo, Redo and Save are the Performance app's");
    // Back goes Home.
    assert!(ui.invoke_key(slint::SharedString::from(slint::platform::Key::Escape)));
    assert_eq!(ui.get_screen(), 0);
}

// A choice is a list over the screen: the dial and a press choose, the bar and the file follow (T1, T4).
#[test]
fn a_choice_is_a_menu_over_the_screen() {
    let mut s = Shelf::new("settings-menu");
    let ui = s.ui();
    shell::goto(shell::Screen::Settings);
    s.serve();
    ui.global::<crate::SettingsState>().invoke_show_page(1);
    s.serve();
    assert_eq!(title(&ui), "Settings › Top bar");
    ui.global::<MenuState>().set_rows_per_view(20);
    ui.global::<PartState>().invoke_tapped(0);
    assert!(ui.global::<MenuState>().get_open());
    let items: Vec<String> = ui.global::<MenuState>().get_rows().iter().map(|r| r.label.to_string()).collect();
    assert_eq!(items, ["Bar and percent", "Bar", "Percent", "Hidden"]);
    ui.global::<MenuState>().invoke_tapped(1);
    s.serve();
    assert!(!ui.global::<MenuState>().get_open());
    assert_eq!(std::fs::read_to_string(s.config.join("top-bar.json")).unwrap(), "{\"cpu\":\"bar\"}\n");
    assert!(ui.global::<Bar>().get_cpu_bar() && !ui.global::<Bar>().get_cpu_percent());
    assert_eq!(value_of(&ui, "CPU"), "Bar ›");
    // By the dial: the cursor, a press, the menu's cursor, a press.
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    assert!(ui.global::<MenuState>().get_open());
    shell::act(Action::Cancel(None));
    assert!(!ui.global::<MenuState>().get_open() && ui.get_screen() == 14, "Cancel closed the menu, not the screen");
}

// A theme chosen on the page is the whole shell's at once: every screen draws with it, and the watcher of the file does
// not apply it a second time (A2).
#[test]
fn a_chosen_theme_is_the_shells() {
    let mut s = Shelf::new("settings-theme");
    installed_theme(&s.config, "evening-glass", "Evening Glass", |d| {
        d["radius"] = json!(14);
        d["spacing"] = json!(1.3);
    });
    let ui = s.ui();
    shell::goto(shell::Screen::Settings);
    s.serve();
    s.e.calls.clear();
    ui.global::<PartState>().invoke_tapped(4);
    s.serve();
    let theme = ui.global::<crate::Theme>();
    assert_eq!((theme.get_id().as_str(), theme.get_radius(), theme.get_spacing()), ("evening-glass", 14., 1.3));
    assert_eq!(std::fs::read_to_string(s.config.join("appearance.txt")).unwrap(), "evening-glass");
    assert!(s.e.calls.is_empty(), "the theme is not the engine's: {:?}", s.e.methods());
    assert_eq!(value_of(&ui, "Evening Glass"), "In use");
}

// A key's name is typed on the on-screen keyboard; the engine's refusal comes back as a question with one answer (K2, K3 in
// the second cut; the keyboard and the question are shared with every page).
#[test]
fn a_name_is_typed_and_a_refusal_is_a_question() {
    let mut s = Shelf::new("settings-keyboard");
    let ui = s.ui();
    shell::set_text_on_screen(false);
    shell::goto(shell::Screen::Settings);
    s.serve();
    ui.global::<crate::SettingsState>().invoke_show_page(6);
    s.serve();
    assert_eq!(title(&ui), "Settings › Signing keys");
    ui.global::<PartState>().invoke_tapped(1);
    assert!(ui.global::<KeyboardState>().get_open());
    assert_eq!(ui.global::<KeyboardState>().get_title().as_str(), "Name of the new signing key");
    for ch in "bad name".chars() {
        assert!(shell::key(&ch.to_string()));
    }
    assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Return)));
    s.serve();
    assert_eq!(s.e.called("keys.make"), [json!({"name": "bad name"})]);
    assert!(ui.global::<NoticeState>().get_open());
    assert_eq!(ui.global::<NoticeState>().get_title().as_str(), "No key made");
    assert!(ui.global::<NoticeState>().get_message().contains("letters, digits"));
    shell::act(Action::Cancel(None));
    assert!(!ui.global::<NoticeState>().get_open());
    assert_eq!(ui.get_screen(), 14);
}

// The status the engine pushes reaches the page shown: it lists again when its list changed, and a status that says the
// same asks nothing.
#[test]
fn the_status_reaches_the_page_shown() {
    let mut s = Shelf::new("settings-status");
    let ui = s.ui();
    shell::goto(shell::Screen::Settings);
    s.serve();
    ui.global::<crate::SettingsState>().invoke_show_page(2);
    s.serve();
    s.e.calls.clear();
    s.push_status(); // what the page has (the first status of a connection also asks for the takes, once)
    s.push_status();
    assert!(s.e.methods().iter().all(|m| m == "record.get"), "{:?}", s.e.methods());
    s.e.calls.clear();
    s.e.ports.push(super::settings_fake::port("Late Surface", &[]));
    s.e.midi_list += 1;
    s.push_status();
    assert_eq!(s.e.methods(), ["controllers.inputs"]);
    assert!(labels(&ui).contains(&"Late Surface".to_string()), "{:?}", labels(&ui));
}

// A performance instrument: with Settings open and nothing happening, it asks nothing and draws nothing.
#[test]
fn at_rest_it_costs_no_request_and_no_frame() {
    let mut s = Shelf::new("settings-idle");
    shell::goto(shell::Screen::Settings);
    s.serve();
    for page in 0..6 {
        s.ui().global::<crate::SettingsState>().invoke_show_page(page);
        s.serve();
    }
    s.f.screen.frame();
    s.f.screen.frame();
    assert_eq!(s.f.screen.frame(), None, "nothing is drawn at rest");
    std::thread::sleep(Duration::from_millis(1200));
    slint::platform::update_timers_and_animations();
    assert!(s.f.engine.try_request(Duration::from_millis(500)).is_none(), "nothing is asked at rest");
    assert!(!rows(&s.ui()).is_empty());
}

// Every Settings page is reached from Home by the panel's move controls alone: the dial (pane none) walks the
// tiles, a press opens the app, up from the list's first row reaches the page rail (not the top bar), the dial
// walks it, a press shows the page, down past the last tab returns to the list, and EXIT (cancel) returns.
#[test]
fn every_page_is_reached_by_moves_alone() {
    let mut s = Shelf::new("settings-pages-moves");
    let ui = s.ui();
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |s: &mut Shelf, key: &str, by: i64| {
        *total.entry(key.to_string()).or_default() += by;
        let n = json!(total.clone());
        s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": n}})));
        s.f.pump();
        s.serve();
    };
    let open_settings = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64)| {
        let ui = s.ui();
        // The first read is a baseline; five moves put the cursor on the Settings tile.
        nav(s, "move", 0);
        for _ in 0..5 {
            nav(s, "move", 1);
        }
        nav(s, "confirm", 1);
        s.serve();
        assert_eq!(ui.get_screen(), 14, "the Settings tile opens the Rust app");
        assert_eq!(title(&ui), "Settings › Appearance");
    };
    // To the page rail from the list the app opens on: up shows the cursor on the list's first row, a
    // second up reaches the rail, not the top bar.
    let to_rail = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64)| {
        let ui = s.ui();
        nav(s, "move", -1);
        assert!(ui.global::<crate::SettingsState>().get_page_cursor() < 0, "the first up shows the list's cursor");
        nav(s, "move", -1);
        assert!(ui.global::<crate::SettingsState>().get_page_cursor() >= 0, "the second up reaches the page rail, not the top bar");
        assert!(!ui.global::<crate::Theme>().get_bar_cursor(), "the top bar did not take the move");
    };
    open_settings(&mut s, &mut nav);
    let pages = ["Appearance", "Top bar", "MIDI controllers", "Package repositories", "Installed", "Available", "Signing keys", "Start", "About"];
    let last = pages.len() as i32 - 1;
    for (want, name) in pages.iter().enumerate() {
        let want = want as i32;
        to_rail(&mut s, &mut nav);
        // Walk the rail to this page's tab, then press.
        let mut tries = 0;
        while ui.global::<crate::SettingsState>().get_page_cursor() != want {
            nav(&mut s, "move", 1);
            tries += 1;
            assert!(tries <= pages.len(), "the rail reaches {name}");
        }
        nav(&mut s, "confirm", 1);
        s.serve();
        assert_eq!(ui.global::<crate::SettingsState>().get_page(), want, "a press on the tab shows {name}");
        assert_eq!(title(&ui), format!("Settings › {name}"));
        // Down into the page's list: past the last tab, the list's first row has the cursor.
        let mut tries = 0;
        while ui.global::<crate::SettingsState>().get_page_cursor() < last {
            nav(&mut s, "move", 1);
            tries += 1;
            assert!(tries <= pages.len(), "the rail walks to its end from {name}");
        }
        nav(&mut s, "move", 1);
        assert!(ui.global::<crate::SettingsState>().get_page_cursor() < 0, "{name}: the list has the cursor again");
        // EXIT leaves the app; the tile opens it again.
        nav(&mut s, "cancel", 1);
        s.serve();
        assert_eq!(ui.get_screen(), 0, "{name}: EXIT returns Home");
        open_settings(&mut s, &mut nav);
    }
}

// Each page of the app, reached the way the panel reaches it: a move shows the cursor on the list's first row, an arrow
// up (the panel's up key) reaches the tabs on the page shown, arrows along them, a press shows the page; from the last
// tab an arrow down leaves for the list. With nothing happening on a page, it asks nothing and draws nothing.
#[test]
fn every_page_by_panel_moves_is_idle_at_rest() {
    let mut s = Shelf::new("settings-idle-pages");
    shell::goto(shell::Screen::Settings);
    s.serve();
    let idle = |s: &mut Shelf, what: &str| {
        s.f.screen.frame();
        s.f.screen.frame();
        assert_eq!(s.f.screen.frame(), None, "{what}: nothing is drawn at rest");
        std::thread::sleep(Duration::from_millis(1200));
        slint::platform::update_timers_and_animations();
        assert!(s.f.engine.try_request(Duration::from_millis(500)).is_none(), "{what}: nothing is asked at rest");
    };
    let walk = |s: &mut Shelf, page: usize| {
        if s.ui().global::<crate::PartState>().get_rows().iter().all(|r| !r.cursor) {
            shell::act(Action::Move { pane: Some(0), by: 1 }); // the cursor on the list's first row
        }
        shell::act(Action::Arrow { dx: 0, dy: -1 }); // up to the tabs, on the page shown
        if page > 0 {
            shell::act(Action::Arrow { dx: 1, dy: 0 }); // along to the next tab
        }
        shell::act(Action::Confirm(None)); // it shows
        s.serve();
        assert_eq!(s.ui().global::<crate::SettingsState>().get_page(), page as i32, "the panel reached the page");
    };
    walk(&mut s, 0); // Appearance, where the app opened
    s.serve();
    let what = title(&s.ui()).to_string();
    idle(&mut s, &what);
    for page in 1..=tabs(&s.ui()).len() - 1 {
        walk(&mut s, page);
        let what = title(&s.ui()).to_string();
        idle(&mut s, &what);
        // From the tabs, arrow down walks them and past the last tab leaves for the list: down until the list has the
        // cursor back.
        loop {
            shell::act(Action::Arrow { dx: 0, dy: 1 });
            s.serve();
            if s.ui().global::<crate::SettingsState>().get_page_cursor() == -1 {
                break;
            }
        }
    }
}

// W16: every action of the MIDI controllers page reached from Home by the panel's moves alone (no touch): the dial
// walks to the Settings tile, a press opens the app, up to the page rail and along it to the MIDI controllers tab, a
// press shows the page, then the dial walks every row the page shows (a port's choice of profile, its Ignore switch)
// and each is pressed, held against the request the fake engine got and the view that changed. The walk visits as
// many rows as the page shows, no row twice.
#[test]
fn midi_controllers_reachable_from_home_by_panel_moves() {
    let mut e = Fake::new();
    e.ports = vec![
        super::settings_fake::port("Pad Controller", &[("pads-a", "Pad Box"), ("pads-b", "Pad Box Mk 2")]),
        super::settings_fake::port("Wooden Keys", &[]),
        super::settings_fake::port("Mixer Surface", &[("mixer", "Mixer Surface")]),
    ];
    let mut s = Shelf::with("w16-midi-walk", e);
    let ui = s.ui();
    tall(&ui);
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |s: &mut Shelf, key: &str, by: i64| {
        *total.entry(key.to_string()).or_default() += by;
        s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": json!(total.clone())}})));
        s.f.pump();
        s.serve();
    };
    // Home → the Settings tile → the Rust app; up to the rail, along it to the MIDI controllers tab, a press shows it.
    nav(&mut s, "move", 0); // the first read is only a baseline
    for _ in 0..5 {
        nav(&mut s, "move", 1); // onto the Settings tile
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(ui.get_screen(), 14, "the Settings tile opens the Rust app");
    nav(&mut s, "move", -1); // the list's cursor
    nav(&mut s, "move", -1); // the page rail, not the top bar
    let pages: Vec<String> = ui.global::<crate::SettingsState>().get_pages().iter().map(|p| p.to_string()).collect();
    let tab = pages.iter().position(|p| p == "MIDI controllers").expect("a MIDI controllers tab") as i32;
    let mut tries = 0;
    while ui.global::<crate::SettingsState>().get_page_cursor() != tab {
        nav(&mut s, "move", 1); // along the rail to the MIDI controllers tab
        tries += 1;
        assert!(tries <= pages.len(), "the rail reaches the MIDI controllers tab");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(ui.global::<crate::SettingsState>().get_page(), tab, "a press shows the MIDI controllers page");
    assert_eq!(title(&ui), "Settings › MIDI controllers");
    // Down past the rail's last tab to the list: the cursor on the first row.
    while ui.global::<crate::SettingsState>().get_page_cursor() >= 0 {
        nav(&mut s, "move", 1);
    }
    // The rows the page shows, then a press on each: a port's profile (a list opens, a chosen item is one request)
    // and its Ignore switch (a press turns it over). Every row the screen shows is visited, none twice.
    let n = rows(&ui).len();
    assert!(n >= 4, "the page shows a row per port's profile and its Ignore switch: {n}");
    for step in 0..n {
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(step), "the dial reached row {step}");
        let name = s.e.ports[step / 2].name.clone();
        if rows(&ui)[step].switch {
            nav(&mut s, "confirm", 1);
            s.serve();
            assert_eq!(s.e.called("controllers.ignore").last().unwrap(), &json!({"input": name, "ignored": true}), "the Ignore switch turns {name} off");
            assert_eq!(rows(&ui)[step].value, "On", "the switch is now On");
        } else {
            nav(&mut s, "confirm", 1);
            s.serve();
            assert!(ui.global::<MenuState>().get_open(), "{name}: the press opens the profile choice");
            nav(&mut s, "move", 1); // to "None" (Automatic is first)
            nav(&mut s, "confirm", 1);
            s.serve();
            assert_eq!(s.e.called("controllers.choose").last().unwrap(), &json!({"input": name, "profiles": []}), "{name}: the choice laid down");
            assert!(!ui.global::<MenuState>().get_open(), "the choice closed the menu");
        }
        if step + 1 < n {
            nav(&mut s, "move", 1);
            s.serve();
        }
    }
}

// W20: the Settings Installed and Available pages are reached from Home by the panel's moves alone, and every action
// they offer is pressed to its real effect. The count of rows is the screen's own (the rows it shows), never hard-coded.
// Installed: a press on each row opens its detail in the panel (the bar's path gains the row) and Back closes it on
// the same row; the bar's Remove... on a row asks `packages.removal` and, answered, one `packages.uninstall` takes it
// out of the list. Available: the bar's Refresh reads the repositories and what is installed again; a row offered
// Install is `packages.install` after its question, the job polled until the offer shows installed, and a row offered
// Update is the same for the newer version. Nothing is reached by touch.
#[test]
fn installed_and_available_reachable_from_home_by_panel_moves() {
    let official = "https://packages.oscairn.org".to_string();
    let official_key = "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".to_string();
    let mut e = Fake::new();
    e.installed_packages = vec![
        inst_pkg("org.example.reverb", "Example Reverb", "1.0.0", "plugin", &official, &["6a2dd1acf4d4e23c"]),
        inst_pkg("lib-1", "Wicked Synths", "", "library", "", &[]),
        inst_pkg("org.beta", "Beta Pack", "1.0.0", "asset", &official, &[]),
    ];
    e.libraries = vec![("lib-sec".into(), "Field Recordings".into(), 34)];
    e.sources = vec![(official.clone(), super::settings_fake::Reads::Done("Oscairn".into(), official_key.clone()))];
    e.offers = vec![(official.clone(), vec![
        offered_pkg("org.delta", "Delta Pack", "3.0.0", json!({"oscairn.asset": 1})),
        offered_pkg("org.beta", "Beta Pack", "2.5.0", json!({"oscairn.asset": 1})),
    ])];
    let done = json!({"added": 1, "skipped": 0, "notes": [], "code": [], "dependencies": []});
    e.installs = vec![
        (official.clone(), "org.delta".into(), "3.0.0".into(), done.clone(), inst_pkg("org.delta", "Delta Pack", "3.0.0", "asset", &official, &[])),
        (official.clone(), "org.beta".into(), "2.5.0".into(), done, inst_pkg("org.beta", "Beta Pack", "2.5.0", "asset", &official, &[])),
    ];
    let mut s = Shelf::with("w20-installed-available", e);
    let ui = s.ui();
    tall(&ui);
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |s: &mut Shelf, key: &str, by: i64| {
        *total.entry(key.to_string()).or_default() += by;
        s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": json!(total.clone())}})));
        s.f.pump();
        s.serve();
    };
    // The bar's path has "Settings" and the page: two parts without a detail, three with it.
    let path_rows = |ui: &Shell| ui.global::<crate::Bar>().get_path().row_count();
    // Up out of the whole list to the page rail (the dial walks the rows, then one stop up reaches the tabs), then along
    // it to the page `want` (found in the screen's own get_pages by name) and a press shows it; past the last tab the
    // list has the cursor again.
    let to_page = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64), want: &str| {
        while ui.global::<crate::SettingsState>().get_page_cursor() < 0 {
            nav(s, "move", -1);
        }
        let pages: Vec<String> = ui.global::<crate::SettingsState>().get_pages().iter().map(|p| p.to_string()).collect();
        let want = pages.iter().position(|p| p == want).expect("the rail names the page") as i32;
        let mut tries = 0;
        while ui.global::<crate::SettingsState>().get_page_cursor() != want {
            // The rail enters on the page shown; a page before it needs a step back, then forward onto it.
            let step = if ui.global::<crate::SettingsState>().get_page_cursor() > want { -1 } else { 1 };
            nav(s, "move", step);
            tries += 1;
            assert!(tries <= 20, "the rail reaches the page");
        }
        nav(s, "confirm", 1);
        s.serve();
        assert_eq!(ui.global::<crate::SettingsState>().get_page(), want, "a press shows the page");
        while ui.global::<crate::SettingsState>().get_page_cursor() >= 0 {
            nav(s, "move", 1);
        }
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(0), "the list's first row has the cursor");
    };
    // A bar tool stop: enter the top bar's actions from the list (pane 2), walk to the action `label` of the page by
    // name in the screen's own get_actions, and press.
    let bar_tool = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64), label: &str| {
        let at = ui.global::<crate::Bar>().get_actions().iter().position(|a| a.label == label).expect("the bar action the test wants") as i32;
        nav(s, "move:2", 1); // the bar, at its first stop (Back)
        for _ in 0..=at {
            nav(s, "move", 1);
        }
        nav(s, "confirm", 1);
        s.serve();
    };
    // A move past the bar's last stop puts the cursor back on the screen's list (the bar had it).
    let drop_bar = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64)| {
        nav(s, "move", 30);
        s.serve();
    };
    // Home → the Settings tile → the Rust app, then the rail along to the Installed page.
    nav(&mut s, "move", 0); // the first read is only a baseline
    for _ in 0..5 {
        nav(&mut s, "move", 1); // onto the Settings tile
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(ui.get_screen(), 14, "the Settings tile opens the Rust app");
    to_page(&mut s, &mut nav, "Installed");
    assert_eq!(title(&ui), "Settings › Installed");
    assert_eq!(path_rows(&ui), 2, "the bar names Settings and the page");
    // Installed: every row the screen shows is reached by the dial, and a press on each opens its detail (the bar's
    // path gains the row) which Back closes on the same row — nothing is asked.
    let n = rows(&ui).len();
    let mut visited = Vec::new();
    for step in 0..n {
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(step), "the dial reached row {step}");
        let label = rows(&ui)[step].label.clone();
        visited.push(label.to_string());
        nav(&mut s, "confirm", 1);
        s.serve();
        assert_eq!(path_rows(&ui), 3, "{label}: the press opened its detail in the panel");
        assert!(info(&ui).contains(&label.to_string()), "{label}: its detail is the one shown: {:?}", info(&ui));
        nav(&mut s, "cancel", 1);
        s.serve();
        assert_eq!(path_rows(&ui), 2, "{label}: Back closed the detail, still on the page");
        if step + 1 < n {
            nav(&mut s, "move", 1);
            s.serve();
        }
    }
    assert_eq!(visited, labels(&ui), "every row once, none twice, in the order shown");
    // The cursor is on the last row (Wicked Synths), after the press that opened its detail and the Back that closed it:
    // the bar's Remove... asks what uses it, and one uninstall takes it out of the list — the list it shows counts one
    // fewer, of the screen's own rows.
    assert_eq!(rows(&ui)[n - 1].label, "Wicked Synths");
    let base = s.e.called("packages.removal").len();
    bar_tool(&mut s, &mut nav, "Remove..."); // Remove... on the row under the cursor
    assert_eq!(s.e.called("packages.removal").last().unwrap(), &json!({"id": "lib-1"}), "what uses it is asked first");
    assert!(ui.global::<NoticeState>().get_open(), "the question is open");
    nav(&mut s, "move", 1); // the question's cursor onto Remove
    nav(&mut s, "confirm", 1);
    s.serve();
    drop_bar(&mut s, &mut nav);
    assert_eq!(s.e.called("packages.removal").len(), base + 1);
    assert_eq!(s.e.called("packages.uninstall"), [json!({"id": "lib-1"})], "one uninstall, with the row's id");
    assert!(!labels(&ui).contains(&"Wicked Synths".to_string()), "the list no longer shows it: {:?}", labels(&ui));
    assert_eq!(rows(&ui).len(), n - 1, "the count is of the items left");
    // The rail to the Available page.
    to_page(&mut s, &mut nav, "Available");
    assert_eq!(title(&ui), "Settings › Available");
    // Available: every offer the screen shows is reached by the dial.
    let m = rows(&ui).len();
    for step in 0..m {
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(step), "the dial reached the offer {step}");
        // A press on an offer row opens nothing and asks nothing: the install/update is the bar's action alone, after
        // its question. The list is unchanged and no request went to the engine.
        nav(&mut s, "confirm", 1);
        s.serve();
        assert!(!ui.global::<NoticeState>().get_open(), "a press on an offer asks nothing");
        assert!(s.e.called("packages.install").is_empty(), "a press on an offer installs nothing");
        assert_eq!(path_rows(&ui), 2, "a press on an offer opens no detail");
        if step + 1 < m {
            nav(&mut s, "move", 1);
            s.serve();
        }
    }
    // Refresh reads the repositories and what is installed again; the same offers remain.
    let before = s.e.called("packages.repositories").len();
    bar_tool(&mut s, &mut nav, "Refresh");
    drop_bar(&mut s, &mut nav);
    assert_eq!(s.e.called("packages.repositories").len(), before + 1, "refresh reads the repositories again");
    assert_eq!(labels(&ui).len(), m, "the same offers after a refresh");
    // The first offer (Beta Pack, an older version installed) is Updated after its question. A refresh leaves the
    // cursor on the first offer (Beta Pack).
    assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(0), "the cursor on the first offer after the refresh");
    assert_eq!(rows(&ui)[0].label, "Beta Pack", "the list is alphabetical");
    bar_tool(&mut s, &mut nav, "Update"); // the Update action of the row under the cursor
    assert!(ui.global::<NoticeState>().get_open(), "an update asks first");
    nav(&mut s, "move", 1);
    nav(&mut s, "confirm", 1);
    s.serve();
    drop_bar(&mut s, &mut nav);
    assert_eq!(s.e.called("packages.install").last().unwrap(), &json!({"source": official.clone(), "id": "org.beta", "version": "2.5.0", "acceptUnsignedCode": false, "replace": false}));
    assert_eq!(value_of(&ui, "Beta Pack"), "Samples 2.5.0  ·  Installed", "the new version is the installed one");
    // The second offer (Delta Pack, not installed) is Installed the same way.
    nav(&mut s, "move", 1);
    s.serve();
    assert_eq!(rows(&ui)[1].label, "Delta Pack");
    bar_tool(&mut s, &mut nav, "Install"); // Install, the row's action
    assert!(ui.global::<NoticeState>().get_open(), "an install asks first");
    nav(&mut s, "move", 1);
    nav(&mut s, "confirm", 1);
    s.serve();
    drop_bar(&mut s, &mut nav);
    assert_eq!(s.e.called("packages.install").last().unwrap(), &json!({"source": official, "id": "org.delta", "version": "3.0.0", "acceptUnsignedCode": false, "replace": false}));
    assert_eq!(value_of(&ui, "Delta Pack"), "Samples 3.0.0  ·  Installed");
}

// A no-touch walk of the Signing keys page: from Home by the panel's moves alone, then every action the page
// offers is walked by the dial and pressed, the count taken from the page's own rows (never a hard-coded one); a
// press on a key asks nothing but the panel follows the cursor with that key's fingerprint, and the Make row's
// press opens the keyboard, asks keys.make with the name typed, and lists the new key.
#[test]
fn signing_keys_page_is_a_no_touch_walk() {
    let mut e = Fake::new();
    e.keys.push(("Stage".into(), "11".repeat(32)));
    e.keys.push(("Pad".into(), "22".repeat(32)));
    let mut s = Shelf::with("settings-keys-walk", e);
    shell::set_text_on_screen(false); // the name is typed on the on-screen keyboard's field
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |s: &mut Shelf, key: &str, by: i64| {
        *total.entry(key.to_string()).or_default() += by;
        let n = json!(total.clone());
        s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": n}})));
        s.f.pump();
        s.serve();
    };
    // From Home the dial alone: the first read is a baseline, then the dial walks the tiles until the
    // Settings tile (the one named so) has the cursor, and a press opens the Rust app.
    nav(&mut s, "move", 0);
    let mut tries = 0;
    while !s.ui().global::<crate::HomeState>().get_tiles().iter().any(|t| t.cursor && t.name == "Settings") {
        nav(&mut s, "move", 1);
        tries += 1;
        assert!(tries < 8, "the dial reaches the Settings tile");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(s.ui().get_screen(), 14, "the Settings tile opens the Rust app");
    // The pages the screen shows provide the walk's count: where the Signing keys tab sits, the last tab.
    let (keys_tab, last) = {
        let ui = s.ui();
        let pages: Vec<String> = ui.global::<crate::SettingsState>().get_pages().iter().map(|p| p.to_string()).collect();
        (pages.iter().position(|p| p == "Signing keys").unwrap() as i32, pages.len() as i32 - 1)
    };
    // Up to the page rail (not the top bar), along to the Signing keys tab, a press shows it.
    nav(&mut s, "move", -1);
    nav(&mut s, "move", -1);
    assert!(s.ui().global::<crate::SettingsState>().get_page_cursor() >= 0, "the second up reaches the page rail, not the top bar");
    let mut tries = 0;
    while s.ui().global::<crate::SettingsState>().get_page_cursor() != keys_tab {
        nav(&mut s, "move", 1);
        tries += 1;
        assert!(tries <= keys_tab as usize + 1, "the rail reaches the Signing keys tab");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(s.ui().global::<crate::SettingsState>().get_page(), keys_tab, "a press shows the Signing keys page");
    assert_eq!(title(&s.ui()), "Settings › Signing keys");
    // Down past the last tab leaves the rail for the list, the cursor on its first row.
    while s.ui().global::<crate::SettingsState>().get_page_cursor() < last {
        nav(&mut s, "move", 1);
    }
    nav(&mut s, "move", 1);
    assert!(s.ui().global::<crate::SettingsState>().get_page_cursor() < 0, "down past the last tab leaves for the list");
    // The page's own rows, the walk counted from them: each key's row (the panel follows with its fingerprint,
    // a press asks nothing) and the Make row's press.
    let make = { let ui = s.ui(); rows(&ui).iter().position(|r| r.label == "Make a signing key").expect("a Make row") };
    assert!(make >= 2, "the two keys sit above the Make row: {make} rows");
    assert!(rows(&s.ui())[0].cursor, "the list's first row has the cursor");
    for i in 0..=make {
        if i > 0 {
            nav(&mut s, "move", 1);
        }
        let ui = s.ui();
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(i), "the walk sits on row {i}");
        let name = rows(&ui)[i].label.to_string();
        assert!(info(&ui).iter().any(|l| l == name.as_str()), "the panel follows the cursor with {name}: {:?}", info(&ui));
        nav(&mut s, "confirm", 1);
        s.serve();
        if name == "Make a signing key" {
            assert!(s.ui().global::<crate::KeyboardState>().get_open(), "the Make row's press opens the keyboard");
            break;
        }
        assert!(!s.ui().global::<crate::KeyboardState>().get_open(), "a key's press opens no keyboard");
        assert!(s.e.called("keys.make").is_empty(), "a key's press asks nothing");
    }
    // The name is typed on the keyboard; Return asks keys.make with it, and the new key is listed.
    let before = rows(&s.ui()).len();
    for ch in "stage".chars() {
        assert!(shell::key(&ch.to_string()));
    }
    assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Return)));
    s.serve();
    assert_eq!(s.e.called("keys.make"), [json!({"name": "stage"})]);
    let ui = s.ui();
    assert!(rows(&ui).iter().any(|r| r.label == "stage"), "the new key is listed: {:?}", labels(&ui));
    assert_eq!(before + 1, rows(&ui).len(), "the page's own count grew by the one key made");
}

// A no-touch walk of the Audio page: from Home by the panel's moves alone, up to the page rail and along it to
// the Audio tab, a press shows it, then every row the page shows (Output, Input, Sample rate, Buffer size) is
// walked by the dial and pressed, each choice laid down and held against the `audio.set` the fake engine got with
// its parameters, the count the page's own rows give (never a hard-coded one) and no row twice. Only a profile
// that does not fix its audio device has the Audio page, so the walk runs on a computer.
#[test]
fn audio_page_is_a_no_touch_walk() {
    let mut e = Fake::new();
    // The two outputs offer the same rates and sizes, so the menus do not shift when the open device changes.
    e.audio.outputs = vec![
        super::settings_fake::dev("Room", &[44100, 48000, 96000], &[64, 128, 256, 512]),
        super::settings_fake::dev("Stage", &[44100, 48000, 96000], &[64, 128, 256, 512]),
    ];
    e.audio.output = "Room".into();
    e.audio.inputs = vec![
        super::settings_fake::dev("Mic In", &[48000], &[128]),
        super::settings_fake::dev("Line In", &[48000], &[128]),
    ];
    e.audio.input = "Mic In".into();
    e.audio.rate = 48000;
    e.audio.size = 128;
    let mut s = Shelf::on("w15-audio-walk", None, e);
    let ui = s.ui();
    tall(&ui);
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |s: &mut Shelf, key: &str, by: i64| {
        *total.entry(key.to_string()).or_default() += by;
        let n = json!(total.clone());
        s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": n}})));
        s.f.pump();
        s.serve();
    };
    // From Home the dial alone: the first read is a baseline, then the dial walks the tiles until the
    // Settings tile (always the last one the screen offers) has the cursor, and a press opens the Rust app.
    nav(&mut s, "move", 0);
    let mut tries = 0;
    while !ui.global::<crate::HomeState>().get_tiles().iter().any(|t| t.cursor && t.name == "Settings") {
        nav(&mut s, "move", 1);
        tries += 1;
        assert!(tries < 8, "the dial reaches the Settings tile");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(ui.get_screen(), 14, "the Settings tile opens the Rust app");
    // The Audio page is a tab of this computer's app: where it sits among the pages the screen shows.
    let audio_tab = {
        let pages: Vec<String> = ui.global::<crate::SettingsState>().get_pages().iter().map(|p| p.to_string()).collect();
        pages.iter().position(|p| p == "Audio").expect("a computer's app has the Audio page") as i32
    };
    // Up to the page rail (not the top bar), along to the Audio tab, a press shows it.
    nav(&mut s, "move", -1);
    nav(&mut s, "move", -1);
    assert!(ui.global::<crate::SettingsState>().get_page_cursor() >= 0, "the second up reaches the page rail, not the top bar");
    let mut tries = 0;
    while ui.global::<crate::SettingsState>().get_page_cursor() != audio_tab {
        nav(&mut s, "move", 1);
        tries += 1;
        assert!(tries <= audio_tab as usize + 1, "the rail reaches the Audio tab");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(ui.global::<crate::SettingsState>().get_page(), audio_tab, "a press shows the Audio page");
    assert_eq!(title(&ui), "Settings › Audio");
    // Down past the rail's last tab to the list: the cursor on its first row (Output).
    while ui.global::<crate::SettingsState>().get_page_cursor() >= 0 {
        nav(&mut s, "move", 1);
    }
    // Every row the page shows, walked and pressed: each choice laid down is one `audio.set` of that choice alone,
    // its parameters held. The walk visits as many rows as the page shows, none twice.
    let n = rows(&ui).len();
    assert!(n >= 4, "Output, Input, Sample rate and Buffer size all show (the fake has inputs): {n} rows");
    for step in 0..n {
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(step), "the dial reached row {step}");
        let label = rows(&ui)[step].label.to_string();
        nav(&mut s, "confirm", 1);
        s.serve();
        assert!(ui.global::<crate::MenuState>().get_open(), "{label}: the press opens its choice");
        // The current choice is ticked; the dial walks one choice on and turns the menu a page at a time
        // until a different (unticked) one has the cursor — every choice row offers more than the current,
        // so one always differs — and a press lays that one down as one `audio.set`.
        nav(&mut s, "move", 1);
        s.serve();
        let mut guards = 0;
        while ui.global::<crate::MenuState>().get_rows().iter().any(|r| r.cursor && r.tick == 1) {
            nav(&mut s, "move", 1);
            s.serve();
            guards += 1;
            assert!(guards < 8, "{label}: the dial reaches a different choice");
        }
        let chose = ui.global::<crate::MenuState>().get_rows().iter().find(|r| r.cursor).map(|r| r.label.to_string()).expect("{label}: the menu has the cursor");
        nav(&mut s, "confirm", 1);
        s.serve();
        assert!(!ui.global::<crate::MenuState>().get_open(), "{label}: the choice closed the menu");
        let called = s.e.called("audio.set").last().cloned().expect("{label}: a laid-down choice is one audio.set");
        match label.as_str() {
            "Output" => assert_eq!(called, json!({"output": chose}), "{label}: the choice sets that output"),
            "Input" => assert_eq!(called, json!({"input": chose}), "{label}: the choice sets that input"),
            "Sample rate" => {
                let rate: i64 = chose.split_whitespace().next().unwrap().parse().unwrap();
                assert_eq!(called, json!({"sampleRate": rate}), "{label}: the choice sets that rate");
            }
            "Buffer size" => {
                let size: u32 = chose.split_whitespace().next().unwrap().parse().unwrap();
                assert_eq!(called, json!({"bufferSize": size}), "{label}: the choice sets that size");
            }
            other => panic!("a row the walk did not expect: {other}"),
        }
        if step + 1 < n {
            nav(&mut s, "move", 1);
            s.serve();
        }
    }
}


// A no-touch walk of the Package repositories page: from Home by the panel's moves alone, then every row the
// page's own model shows (never a hard-coded count) is walked by the dial and pressed to its real effect — a
// repository's row turns its switch, the engine's `packages.enableRepository` naming that address, and the Add
// row's press opens the keyboard, the address typed, the listing read and kept: `packages.list`, the job polled,
// `packages.addRepository`, and the new repository listed.
#[test]
fn package_repositories_page_is_a_no_touch_walk() {
    let oscairn = "https://packages.oscairn.org";
    let friend = "https://packages.friend.org";
    let more = "https://packages.more.org";
    let key = "33".repeat(32);
    let mut e = Fake::new();
    e.repos = vec![
        super::settings_fake::Repo {
            address: oscairn.into(),
            name: "Oscairn".into(),
            key: "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".into(),
            origin: "official".into(),
            enabled: true,
        },
        super::settings_fake::Repo { address: friend.into(), name: "Friend Sounds".into(), key: key.clone(), origin: "user".into(), enabled: true },
    ];
    e.sources = vec![(more.into(), super::settings_fake::Reads::Done("More Sounds".into(), key.clone()))];
    e.trusted.push(key.clone());
    let mut s = Shelf::with("w17-repos-walk", e);
    shell::set_text_on_screen(false);
    let ui = s.ui();
    tall(&ui);
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |s: &mut Shelf, key: &str, by: i64| {
        *total.entry(key.to_string()).or_default() += by;
        let n = json!(total.clone());
        s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": n}})));
        s.f.pump();
        s.serve();
    };
    // From Home the dial alone: the first read is a baseline, then the dial walks the tiles until the
    // Settings tile (the one named so) has the cursor, and a press opens the Rust app.
    nav(&mut s, "move", 0);
    let mut tries = 0;
    while !s.ui().global::<crate::HomeState>().get_tiles().iter().any(|t| t.cursor && t.name == "Settings") {
        nav(&mut s, "move", 1);
        tries += 1;
        assert!(tries < 8, "the dial reaches the Settings tile");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(s.ui().get_screen(), 14, "the Settings tile opens the Rust app");
    // Up to the page rail (not the top bar), along to the Package repositories tab, a press shows it.
    nav(&mut s, "move", -1);
    nav(&mut s, "move", -1);
    assert!(s.ui().global::<crate::SettingsState>().get_page_cursor() >= 0, "the second up reaches the page rail, not the top bar");
    let (tab, last) = {
        let ui = s.ui();
        let pages: Vec<String> = ui.global::<crate::SettingsState>().get_pages().iter().map(|p| p.to_string()).collect();
        (pages.iter().position(|p| p == "Package repositories").expect("a Package repositories tab") as i32, pages.len() as i32 - 1)
    };
    let mut tries = 0;
    while s.ui().global::<crate::SettingsState>().get_page_cursor() != tab {
        nav(&mut s, "move", 1);
        tries += 1;
        assert!(tries <= tab as usize + 1, "the rail reaches the Package repositories tab");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(s.ui().global::<crate::SettingsState>().get_page(), tab, "a press shows the Package repositories page");
    assert_eq!(title(&s.ui()), "Settings › Package repositories");
    // Down past the last tab leaves the rail for the list, the cursor on its first row.
    while s.ui().global::<crate::SettingsState>().get_page_cursor() < last {
        nav(&mut s, "move", 1);
    }
    nav(&mut s, "move", 1);
    assert!(s.ui().global::<crate::SettingsState>().get_page_cursor() < 0, "down past the last tab leaves for the list");
    // The page's own rows, the walk counted from them: each repository's switch pressed, then the Add row's press.
    let n = rows(&s.ui()).len();
    assert!(n >= 3, "the page shows a row per repository and the Add row: {n}");
    for step in 0..n {
        if step > 0 {
            nav(&mut s, "move", 1);
        }
        let ui = s.ui();
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(step), "the dial reached row {step}");
        if rows(&ui)[step].switch {
            let want = s.e.repos[step].address.clone();
            nav(&mut s, "confirm", 1);
            s.serve();
            assert_eq!(s.e.called("packages.enableRepository").last(), Some(&json!({"address": want, "enabled": false})), "a press turns repository {step} off");
            assert_eq!(rows(&s.ui())[step].value, "Off", "the switch is now Off");
        } else {
            assert_eq!(rows(&ui)[step].label, "Add a repository", "the last row adds one");
            nav(&mut s, "confirm", 1);
            s.serve();
            assert!(s.ui().global::<crate::KeyboardState>().get_open(), "the Add row's press opens the keyboard");
        }
    }
    // The address is typed on the keyboard; Return asks packages.list, the listing read and polled, the
    // repository added and listed.
    let before = rows(&s.ui()).len();
    for ch in more.chars() {
        assert!(shell::key(&ch.to_string()));
    }
    assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Return)));
    s.serve();
    assert_eq!(s.e.called("packages.list"), [json!({"source": more})]);
    assert_eq!(s.e.called("packages.addRepository").last(), Some(&json!({"address": more, "key": key, "name": "More Sounds"})), "the repository the listing kept is added");
    let ui = s.ui();
    assert!(rows(&ui).iter().any(|r| r.label == "More Sounds"), "the new repository is listed: {:?}", labels(&ui));
    assert_eq!(before + 1, rows(&ui).len(), "the page's own count grew by the one repository added");
    assert_eq!(value_of(&ui, "More Sounds"), "On", "the added repository is on");
}

// W19: the Settings Appearance and Top bar pages, reached from Home by the panel's moves alone (no touch), every
// action each offers pressed to its real effect, counted from the screen's own rows (never hard-coded). Appearance:
// a press on each theme row writes appearance.txt with its id and shows the theme at once; the bar's Install...
// opens the file chooser, where choosing a theme.json installs it (a new row) and shows it. Top bar: a press on
// the CPU and RAM rows opens their choice, choosing one writes top-bar.json and moves the shell's meters at once.
// Nothing is reached by touch.
#[test]
fn appearance_and_top_bar_reachable_from_home_by_panel_moves() {
    // A theme of the player's own, listed on the Appearance page, and another whose folder is the Install chooser's
    // source (kept out of the installed list until it is installed; a folder named after its id).
    let mut s = Shelf::with("w19-appearance-topbar", Fake::new());
    installed_theme(&s.config, "evening-glass", "Evening Glass", |d| d["spacing"] = json!(1.3));
    installed_theme(&s.config, "amber", "Amber", |_| {});
    std::fs::rename(s.config.join("themes/amber"), s.config.join("amber")).unwrap();
    shell::settings_files_dir(s.config.join("amber"));
    let ui = s.ui();
    tall(&ui);
    let id_of = |name: &str| -> &'static str {
        match name {
            "Oscairn Block" => "oscairn-block",
            "Oscairn Classic" => "oscairn-classic",
            "Oscairn Dark" => "oscairn-dark",
            "Oscairn Light" => "oscairn-light",
            "Evening Glass" => "evening-glass",
            _ => panic!("a theme the test does not know: {name}"),
        }
    };
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |s: &mut Shelf, key: &str, by: i64| {
        *total.entry(key.to_string()).or_default() += by;
        s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": json!(total.clone())}})));
        s.f.pump();
        s.serve();
    };
    // Up out of the whole list to the page rail, then along it to the page `want` (found in the screen's own get_pages
    // by name) and a press shows it; past the last tab the list has the cursor again. Entered on the list.
    let to_page = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64), want: &str| {
        while ui.global::<crate::SettingsState>().get_page_cursor() < 0 {
            nav(s, "move", -1);
        }
        let pages: Vec<String> = ui.global::<crate::SettingsState>().get_pages().iter().map(|p| p.to_string()).collect();
        let want = pages.iter().position(|p| p == want).expect("the rail names the page") as i32;
        let mut tries = 0;
        while ui.global::<crate::SettingsState>().get_page_cursor() != want {
            // The rail enters on the page shown; a page before it needs a step back, then forward onto it.
            let step = if ui.global::<crate::SettingsState>().get_page_cursor() > want { -1 } else { 1 };
            nav(s, "move", step);
            tries += 1;
            assert!(tries <= 20, "the rail reaches the page");
        }
        nav(s, "confirm", 1);
        s.serve();
        assert_eq!(ui.global::<crate::SettingsState>().get_page(), want, "a press shows the page");
        while ui.global::<crate::SettingsState>().get_page_cursor() >= 0 {
            nav(s, "move", 1);
        }
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(0), "the list's first row has the cursor");
    };
    // A bar tool stop: enter the top bar's actions from the list (pane 2), walk to the action `label` of the page by
    // name in the screen's own get_actions, and press.
    let bar_tool = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64), label: &str| {
        let at = ui.global::<crate::Bar>().get_actions().iter().position(|a| a.label == label).expect("the bar action the test wants") as i32;
        nav(s, "move:2", 1); // the bar, at its first stop (Back)
        for _ in 0..=at {
            nav(s, "move", 1);
        }
        nav(s, "confirm", 1);
        s.serve();
    };
    // A move past the bar's last stop puts the cursor back on the screen's list (the bar had it).
    let drop_bar = |s: &mut Shelf, nav: &mut dyn FnMut(&mut Shelf, &str, i64)| {
        nav(s, "move", 30);
        s.serve();
    };
    // Home → the Settings tile → the Rust app (the page it opens on is Appearance); the first move shows the list's
    // first row.
    nav(&mut s, "move", 0); // the first read is only a baseline
    let mut tries = 0;
    while !ui.global::<crate::HomeState>().get_tiles().iter().any(|t| t.cursor && t.name == "Settings") {
        nav(&mut s, "move", 1); // onto the Settings tile
        tries += 1;
        assert!(tries < 8, "the dial reaches the Settings tile");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(ui.get_screen(), 14, "the Settings tile opens the Rust app");
    assert_eq!(title(&ui), "Settings › Appearance");
    nav(&mut s, "move", 1);
    assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(0), "the list's first row has the cursor");
    // Appearance: every theme the page's own rows show is reached by the dial, and a press on each writes
    // appearance.txt with its id and shows the theme at once. The count is the screen's own.
    let n = rows(&ui).len();
    assert!(n >= 5, "the built-in themes and the player's own are listed: {n}");
    for i in 0..n {
        assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(i), "the dial reached the theme row {i}");
        let theme = rows(&ui)[i].label.to_string();
        nav(&mut s, "confirm", 1);
        s.serve();
        assert_eq!(std::fs::read_to_string(s.config.join("appearance.txt")).unwrap(), id_of(&theme), "choosing {theme} wrote appearance.txt");
        assert_eq!(ui.global::<crate::Theme>().get_id(), id_of(&theme), "{theme} is shown at once");
        assert_eq!(value_of(&ui, &theme), "In use", "{theme} is the row's in-use one");
        if i + 1 < n {
            nav(&mut s, "move", 1);
            s.serve();
        }
    }
    // The rail to the Top bar page.
    to_page(&mut s, &mut nav, "Top bar");
    assert_eq!(title(&ui), "Settings › Top bar");
    assert_eq!(labels(&ui), ["CPU", "RAM"]);
    assert!(ui.global::<crate::Bar>().get_cpu_bar() && ui.global::<crate::Bar>().get_cpu_percent(), "CPU starts Bar and percent");
    // The CPU row: a press opens its choice, choosing Bar writes top-bar.json and the shell shows Bar at once.
    assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(0), "the dial reached CPU");
    nav(&mut s, "confirm", 1);
    s.serve();
    assert!(ui.global::<MenuState>().get_open(), "a press on CPU opens its choice");
    nav(&mut s, "move", 1); // to Bar
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(std::fs::read_to_string(s.config.join("top-bar.json")).unwrap(), "{\"cpu\":\"bar\"}\n", "choosing Bar wrote top-bar.json");
    assert!(ui.global::<crate::Bar>().get_cpu_bar() && !ui.global::<crate::Bar>().get_cpu_percent(), "the bar shows Bar now");
    assert_eq!(value_of(&ui, "CPU"), "Bar ›");
    // The RAM row: the dial reaches it and a press opens its choice; choosing Hidden writes it and the shell hides the
    // RAM meter at once.
    nav(&mut s, "move", 1);
    s.serve();
    assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(1), "the dial reached RAM");
    nav(&mut s, "confirm", 1);
    s.serve();
    assert!(ui.global::<MenuState>().get_open(), "a press on RAM opens its choice");
    let mut tries = 0;
    while !ui.global::<MenuState>().get_rows().iter().any(|r| r.cursor && r.label == "Hidden") {
        nav(&mut s, "move", 1); // along the choice to Hidden
        tries += 1;
        assert!(tries < 8, "the dial reaches Hidden");
    }
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(std::fs::read_to_string(s.config.join("top-bar.json")).unwrap(), "{\"cpu\":\"bar\",\"ram\":\"none\"}\n", "choosing Hidden wrote it");
    assert!(!ui.global::<crate::Bar>().get_ram_bar() && !ui.global::<crate::Bar>().get_ram_percent(), "RAM Hidden now");
    assert_eq!(value_of(&ui, "RAM"), "Hidden ›");
    // The rail to the Appearance page, for the Install... tool.
    to_page(&mut s, &mut nav, "Appearance");
    assert_eq!(title(&ui), "Settings › Appearance");
    // The bar's Install... opens the file chooser (the path gains a level); the chooser's own theme.json, when pressed,
    // installs the source theme as a new row and shows it.
    let was = rows(&ui).len();
    bar_tool(&mut s, &mut nav, "Install...");
    assert_eq!(title(&ui), "Settings › Appearance › Install a theme", "the chooser is over the page");
    assert_eq!(rows(&ui)[0].label, "‹ Up", "the chooser lists the files");
    drop_bar(&mut s, &mut nav);
    let target = labels(&ui).iter().position(|l| l == "theme.json").expect("the chooser lists theme.json");
    assert!(target >= 1, "Up, then the source's theme.json: {:?}", labels(&ui));
    for _ in 0..target {
        nav(&mut s, "move", 1);
        s.serve();
    }
    assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(target), "the dial reaches theme.json");
    nav(&mut s, "confirm", 1);
    s.serve();
    assert_eq!(title(&ui), "Settings › Appearance", "the chooser closed");
    assert_eq!(std::fs::read_to_string(s.config.join("appearance.txt")).unwrap(), "amber", "the installed theme is shown");
    assert_eq!(value_of(&ui, "Amber"), "In use");
    assert_eq!(rows(&ui).len(), was + 1, "the pages own count grew by the one theme installed: {:?}", labels(&ui));
}
