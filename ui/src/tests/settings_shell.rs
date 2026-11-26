// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app on the shell, through a socket to the fake engine of settings_fake.rs: the tile, the bar, a menu, the
//! on-screen keyboard and the questions over the screen, the status that makes a page list again, and that nothing at rest
//! costs a request or a frame.

use super::library::{info, labels, rows, value_of};
use super::settings::{folder, installed_theme, tabs};
use super::settings_fake::Settings as Fake;
use super::*;
use crate::{KeyboardState, MenuState, NoticeState, PartState};
use std::time::Duration;

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
        let mut f = Fixture::on(name, profile);
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
    assert_eq!(tabs(&ui), ["Appearance", "Top bar", "MIDI controllers", "Package repositories", "Installed", "Available", "Signing keys", "About"], "the keyboard's profile fixes its audio device");
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
    let pages = ["Appearance", "Top bar", "MIDI controllers", "Package repositories", "Installed", "Available", "Signing keys", "About"];
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
    // From Home the dial alone: five moves out of the first read's baseline put the cursor on the Settings
    // tile, a press opens the Rust app.
    nav(&mut s, "move", 0); // the first read: a baseline
    for _ in 0..5 {
        nav(&mut s, "move", 1);
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
