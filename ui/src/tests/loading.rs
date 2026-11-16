// SPDX-License-Identifier: AGPL-3.0-or-later
//! Waits the player expects: while a Performance opens the top bar shows "Loading NAME" (a chip with a bar that sweeps),
//! the slot being opened is marked, and nothing that reads as a fault is raised: no banner, no error in a screen's line
//! for the busy answers the engine gives meanwhile. The banner is for what nothing explains: no answer, a wait far past
//! its usual, a lost connection. At rest, and once the load has landed, nothing is drawn and nothing is asked.

use super::library::{footer, rows};
use super::library_shell::Shelf;
use super::*;
use crate::{NoticeState, PartState};
use std::time::Duration;

const NAMES: [&str; 12] = ["Harbor Lights", "Dusk Choir", "Paper Moons", "Glass Garden", "Slow Tide", "Northern Strings", "Amber Keys", "Copper Pad", "Quiet Brass", "Velvet Organ", "Salt Marsh", "Low Lantern"];

fn content() -> serde_json::Value {
    let performances: Vec<_> = NAMES.iter().enumerate().map(|(i, n)| json!({"id": format!("p{i}"), "name": n, "color": "", "path": format!("/lib/p{i}")})).collect();
    let slots: Vec<_> = NAMES.iter().enumerate().map(|(i, n)| json!({"id": format!("p{i}"), "name": n, "color": "", "label": ""})).collect();
    json!({
        "setlists": [{"id": "s1", "name": "Friday"}, {"id": "s2", "name": "Saturday"}],
        "performances": performances,
        "setlist": {"format": "oscairn.setlist", "version": 2, "info": {"id": "s1", "name": "Friday"}, "display": {"mode": "grid", "columns": 4, "rows": 3},
            "pages": [{"name": "Set 1", "slots": slots}]},
    })
}

/// The Setlist shown, its setlist read, the connection quiet.
fn on_setlist(name: &str) -> Fixture {
    let mut f = Fixture::new(name);
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(2); // Setlist
    f.serve(13, &content());
    f
}

fn loading(f: &Fixture) -> String {
    f.screen.ui.global::<Bar>().get_loading().to_string()
}

fn banner(f: &Fixture) -> String {
    f.screen.ui.get_engine().to_string()
}

fn opening(f: &Fixture) -> Vec<bool> {
    f.screen.ui.global::<SetlistState>().get_slots().iter().map(|s| s.opening).collect()
}

fn ok() -> serde_json::Value {
    json!({"result": {}})
}

/// Pictures of the screen as it is in the dark theme and in the light one, and of the part of the bar at `crop`, enlarged;
/// the theme the test had is put back.
fn shots(f: &mut Fixture, name: &str, crop: Rect) {
    for id in ["oscairn-dark", "oscairn-light"] {
        theme::apply(&f.screen.ui, &theme::chosen(&theme::Places::default(), id, ""));
        tick(f);
        png(&f.screen, &format!("{name}-{id}"));
        png_crop(&f.screen, &format!("{name}-chip-{id}"), crop, 3);
    }
    theme::apply(&f.screen.ui, &theme::chosen(&theme::Places::default(), "", ""));
    tick(f);
}

/// Frames while something animates; the animation is the window's to ask for, so a frame follows a tick.
fn tick(f: &mut Fixture) -> Option<Rect> {
    std::thread::sleep(Duration::from_millis(3)); // the animation clock is in milliseconds
    slint::platform::update_timers_and_animations();
    f.screen.frame()
}

// A slot chosen: the bar says "Loading NAME" from the moment it is asked until it has opened, the slot is marked, no
// banner. Another client's long request makes the engine refuse the load as busy: the load is asked again, nothing
// is raised, the chip stays. When it lands the chip goes, and the shell is as still as before: no frame, no request.
#[test]
fn a_switch_shows_the_chip_with_the_name_and_no_banner() {
    let mut f = on_setlist("loading-switch");
    assert_eq!((loading(&f), banner(&f)), (String::new(), String::new()));
    f.screen.frame();
    f.screen.frame();
    assert_eq!(f.screen.frame(), None, "before it: at rest");
    assert!(!f.screen.animating());
    f.screen.ui.global::<SetlistState>().invoke_tapped_slot(1);
    let load = f.engine.request();
    assert_eq!((load["method"].as_str(), load["params"]["id"].as_str()), (Some("library.load"), Some("p1")));
    assert_eq!((loading(&f), banner(&f)), ("Loading Dusk Choir".to_string(), String::new()), "asked: the chip names it, nothing is a fault");
    assert_eq!(opening(&f).iter().position(|o| *o), Some(1), "the slot being opened is marked");
    assert!(tick(&mut f).is_some(), "the bar sweeps while the wait lasts: a frame at every tick");
    assert!(f.screen.animating());
    shots(&mut f, "loading-setlist", (700, 0, 580, 56));
    // Busy, naming the long request (another client's load): asked again, quietly.
    f.reply(&load, json!({"error": {"code": -32009, "message": "The engine is busy (library.load); this did not run. Ask again when it is done.", "data": {"busy": "library.load", "ms": 900}}}));
    f.pump(); // the busy answer is heard before its reply: the shell has both now
    std::thread::sleep(crate::client::BUSY_RETRY + Duration::from_millis(50));
    slint::platform::update_timers_and_animations();
    let again = f.engine.request();
    assert_eq!((again["method"].as_str(), again["params"]["id"].as_str()), (Some("library.load"), Some("p1")));
    assert_eq!((loading(&f), banner(&f), f.screen.ui.global::<SetlistState>().get_message().to_string()), ("Loading Dusk Choir".to_string(), String::new(), "2. Dusk Choir".to_string()), "a busy answer raises nothing");
    f.reply(&again, ok());
    assert_eq!((loading(&f), banner(&f)), (String::new(), String::new()), "it opened: the chip is gone");
    assert!(opening(&f).iter().all(|o| !o));
    let prepare = f.engine.request();
    assert_eq!(prepare["method"], "performance.prepare", "the next one prepared");
    f.reply(&prepare, ok());
    // Nothing moves any more: no frame, no animation, no request, not even a tick of the watchdog.
    f.screen.frame();
    f.screen.frame();
    assert_eq!(tick(&mut f), None, "no redraw after the wait ends");
    assert!(!f.screen.animating());
    assert_eq!(tick(&mut f), None);
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(tick(&mut f), None);
    assert!(f.engine.try_request(Duration::from_millis(300)).is_none(), "nothing is asked at rest");
}

// Another client's load of a second: the Setlist's refused load is asked again at the client's own pace (100 ms), not at
// once, so the engine is asked about ten times, not hundreds; the chip and the slot's bar stay all along, nothing is said
// as a failure, and the load that lands ends it.
#[test]
fn a_load_refused_as_busy_is_asked_again_quietly_not_in_a_tight_loop() {
    let mut f = on_setlist("loading-retry");
    f.screen.ui.global::<SetlistState>().invoke_tapped_slot(1);
    let started = std::time::Instant::now();
    let mut asked = 0;
    let busy = json!({"error": {"code": -32009, "message": "busy", "data": {"busy": "library.load", "ms": 5}}});
    while started.elapsed() < Duration::from_millis(1000) {
        slint::platform::update_timers_and_animations();
        if let Some(r) = f.engine.try_request(Duration::from_millis(20)) {
            assert_eq!(r["method"], "library.load");
            asked += 1;
            f.reply(&r, busy.clone());
            f.pump(); // its busy event and then its reply: two events
        }
        assert_eq!((loading(&f).as_str(), banner(&f).as_str()), ("Loading Dusk Choir", ""));
        assert_eq!(opening(&f).iter().position(|o| *o), Some(1));
    }
    assert!((5..=14).contains(&asked), "{asked} loads asked in a second");
    assert!(!f.screen.ui.global::<SetlistState>().get_message().contains("Did not open"));
    std::thread::sleep(Duration::from_millis(150));
    slint::platform::update_timers_and_animations();
    let last = f.engine.request();
    f.reply(&last, ok());
    assert_eq!(loading(&f), "");
}

// A wait far past what a load ever takes is the one case with a banner, in plain words, and the chip stays; when it
// ends both go.
#[test]
fn a_load_far_past_its_usual_says_so_in_plain_words() {
    let mut f = on_setlist("loading-long");
    shell::wait_limits(Duration::from_millis(300), Duration::from_millis(300));
    f.screen.ui.global::<SetlistState>().invoke_tapped_slot(1);
    let load = f.engine.request();
    std::thread::sleep(Duration::from_millis(100));
    slint::platform::update_timers_and_animations();
    assert_eq!(banner(&f), "", "not yet");
    std::thread::sleep(Duration::from_millis(500));
    slint::platform::update_timers_and_animations();
    let said = banner(&f);
    assert_eq!(said, "Opening Dusk Choir is taking much longer than usual. Wait for it, or restart the instrument if it never ends.");
    assert!(!said.contains("busy") && !said.to_lowercase().contains("waiting on engine"));
    assert_eq!(loading(&f), "", "one message at a time: the banner says it, the chip steps aside");
    assert!(tick(&mut f).is_some());
    shots(&mut f, "loading-long-wait", (0, 0, 1280, 160));
    f.reply(&load, ok());
    assert_eq!((loading(&f), banner(&f)), (String::new(), String::new()));
}

// A request nothing explains (reading the setlist: no load, no install) with no answer is the other banner.
#[test]
fn a_request_nothing_explains_gets_the_slow_to_answer_banner() {
    let mut f = on_setlist("loading-silent");
    shell::wait_limits(Duration::from_secs(30), Duration::from_millis(300));
    f.screen.ui.global::<SetlistState>().invoke_tapped_setlist(1);
    let get = f.engine.request();
    assert_eq!(get["method"], "setlists.get");
    assert_eq!((loading(&f), banner(&f)), (String::new(), String::new()), "a read is no wait the player expects");
    std::thread::sleep(Duration::from_millis(500));
    slint::platform::update_timers_and_animations();
    assert_eq!(banner(&f), "The sound engine is slow to answer. This screen catches up when it does; restart the instrument if it never does.");
    assert_eq!(loading(&f), "");
    let doc = content()["setlist"].clone();
    f.reply(&get, json!({"result": {"setlist": doc}}));
    assert_eq!(banner(&f), "");
}

// Another client's load, named by the engine's busy answers and ended by `engine.busy`: "Loading a Performance", whose
// name the engine does not tell; any other long request is "Working".
#[test]
fn another_clients_load_is_a_wait_too() {
    let mut f = on_setlist("loading-other");
    shell::dispatch(Event::Busy { method: "library.load".into() });
    assert_eq!((loading(&f), banner(&f)), ("Loading a Performance".to_string(), String::new()));
    shell::dispatch(Event::Notification { method: "engine.busy".into(), params: json!({"busy": false, "method": "library.load", "ms": 3000}) });
    assert_eq!(loading(&f), "");
    shell::dispatch(Event::Busy { method: "packages.install".into() });
    assert_eq!(loading(&f), "Working");
    shell::dispatch(Event::Notification { method: "engine.busy".into(), params: json!({"busy": false}) });
    assert_eq!((loading(&f), banner(&f)), (String::new(), String::new()));
    f.screen.frame();
    assert_eq!(tick(&mut f), None, "and the bar rests");
}

// The Library: a press asks the engine through its chain; the chip names the row from the first answer to the last, the
// line under the list keeps its own words, and the busy answers are asked again. The question about unsaved changes is no
// wait: nothing has been asked of the engine until it is answered.
#[test]
fn the_library_shows_the_chip_while_a_performance_opens() {
    let mut s = Shelf::new("loading-library");
    let ui = s.ui();
    shell::goto(shell::Screen::Library);
    s.serve();
    ui.global::<PartState>().invoke_tapped(2); // Brass Corner
    let step = s.f.engine.request();
    assert_eq!(step["method"], "performance.info");
    s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": step["id"], "error": {"code": -32009, "message": "busy", "data": {"busy": "library.load", "ms": 800}}})));
    s.f.pump();
    s.f.pump();
    assert_eq!((ui.global::<Bar>().get_loading().as_str(), ui.get_engine().as_str()), ("Loading Brass Corner", ""));
    assert_eq!(footer(&ui).0, "Opening Brass Corner...", "the line under the list is as it was");
    assert_eq!(rows(&ui).iter().filter(|r| r.busy).map(|r| r.label.to_string()).collect::<Vec<_>>(), ["Brass Corner"], "the row being opened has the bar");
    assert!(tick_shelf(&mut s).is_some());
    assert!(s.f.screen.animating());
    shots(&mut s.f, "loading-library", (300, 0, 700, 56));
    std::thread::sleep(Duration::from_millis(150));
    slint::platform::update_timers_and_animations();
    s.serve();
    assert_eq!((ui.global::<Bar>().get_loading().as_str(), ui.get_engine().as_str()), ("", ""), "opened");
    assert!(rows(&ui).iter().all(|r| !r.busy), "and the row's bar is gone");
    s.f.screen.frame();
    s.f.screen.frame();
    assert_eq!(tick_shelf(&mut s), None, "no redraw after it ends");
    assert!(!s.f.screen.animating());
    // Unsaved changes: the question first, and no chip while it is open.
    s.e.open = ("p1".into(), "Evening Strings".into(), true);
    ui.global::<PartState>().invoke_tapped(3);
    s.serve();
    assert!(ui.global::<NoticeState>().get_open());
    assert_eq!(ui.global::<Bar>().get_loading().as_str(), "", "nothing switches until it is confirmed");
    crate::shell::act(Action::Cancel(None));
    s.serve();
    assert_eq!(ui.global::<Bar>().get_loading().as_str(), "");
}

fn tick_shelf(s: &mut Shelf) -> Option<Rect> {
    std::thread::sleep(Duration::from_millis(3));
    slint::platform::update_timers_and_animations();
    s.f.screen.frame()
}

// Play shows the same chip beside the Performance that stays shown; the Performance's name is not given up.
#[test]
fn play_keeps_the_open_performance_and_shows_the_chip() {
    let mut f = Fixture::new("loading-play");
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(2); // Setlist, to choose from
    f.serve(13, &content());
    shell::goto(shell::Screen::Play);
    // The connection's requests, answered as an engine does, until it is quiet.
    while let Some(request) = f.engine.try_request(Duration::from_millis(150)) {
        let result = match request["method"].as_str().unwrap() {
            "performance.info" => json!({"id": "p0", "name": "Harbor Lights", "unsaved": false}),
            "graph.get" => play_graph(),
            "edit.history" => json!({"undo": "", "redo": ""}),
            "library.query" => json!({"items": [], "total": 0}),
            _ => json!({}),
        };
        f.reply(&request, json!({"result": result}));
    }
    f.screen.ui.global::<SetlistState>().invoke_tapped_slot(1);
    let load = f.engine.request();
    assert_eq!(load["method"], "library.load");
    assert_eq!((loading(&f), banner(&f)), ("Loading Dusk Choir".to_string(), String::new()));
    assert_eq!(f.screen.ui.global::<Bar>().get_performance().as_str(), "Harbor Lights", "the open Performance stays on the bar");
    assert!(tick(&mut f).is_some());
    shots(&mut f, "loading-play", (100, 0, 600, 56));
    f.reply(&load, ok());
    assert_eq!(loading(&f), "");
}

// The wait component takes a progress where a caller has one (an install, an export): the bar fills to it, and then it
// does not sweep.
#[test]
fn a_progress_fills_the_bar_and_stops_the_sweep() {
    let mut f = Fixture::new("loading-progress");
    f.engine.answer();
    f.pump();
    let ui = f.screen.ui.clone_strong();
    let bar = ui.global::<Bar>();
    bar.set_loading("Installing Dawn Pads".into());
    bar.set_loading_progress(0.4);
    f.screen.frame();
    f.screen.frame();
    assert_eq!(tick(&mut f), None, "a bar that reports progress is still until it moves");
    assert!(!f.screen.animating());
    bar.set_loading_progress(0.7);
    assert!(f.screen.frame().is_some());
    bar.set_loading_progress(-1.);
    assert!(tick(&mut f).is_some() && f.screen.animating(), "no progress to report: it sweeps");
    bar.set_loading(slint::SharedString::new());
    f.screen.frame();
    assert_eq!(tick(&mut f), None);
    assert!(!f.screen.animating());
}

// The wait component is the theme's: no length, color or time of its own in the .slint that draws it.
#[test]
fn the_wait_component_has_no_literals() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("slint");
    let text = std::fs::read_to_string(dir.join("loading.slint")).unwrap();
    for (n, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        assert!(!code.contains('#'), "loading.slint:{}: a literal color: {line}", n + 1);
        for word in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
            let digits: String = word.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            let unit = &word[digits.len()..];
            assert!(digits.is_empty() || digits == "0" || !["px", "ms", "s"].contains(&unit), "loading.slint:{}: a literal size or time {word}: {line}", n + 1);
        }
    }
}

fn luminance(c: slint::Color) -> f64 {
    let lin = |v: u8| {
        let v = f64::from(v) / 255.;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.red()) + 0.7152 * lin(c.green()) + 0.0722 * lin(c.blue())
}

fn contrast(a: slint::Color, b: slint::Color) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

// The chip is the theme's attention color with the text paired with it: in every built-in theme (dark, light, classic,
// block) and the two odd ones the text reads on it (4.5 to 1 at least), and the bar, drawn in the text's color, with it.
#[test]
fn the_chip_reads_in_every_theme() {
    let f = Fixture::new("loading-themes");
    for t in &all_themes() {
        theme::apply(&f.screen.ui, t);
        let g = f.screen.ui.global::<crate::Theme>();
        let ratio = contrast(g.get_loading_background(), g.get_loading_text());
        assert!(ratio >= 4.5, "{}: the chip's text on its fill is {ratio:.1} to 1", t.id);
    }
}

// A 40-character name in the chip: cut with an ellipsis (never wrapped, never more than half of the title's room), and
// nothing else on the bar moves: the title's field, the actions, the tempo and the meters are where they were.
#[test]
fn a_long_name_is_cut_and_moves_nothing() {
    let mut f = Fixture::new("loading-long-name");
    f.engine.answer();
    f.pump();
    f.reopened();
    shell::goto(shell::Screen::Play);
    let ui = f.screen.ui.clone_strong();
    let bar = ui.global::<Bar>();
    bar.set_performance("Harbor Lights".into());
    bar.set_tempo("120.0 BPM".into());
    f.screen.frame();
    f.screen.frame();
    let place = |ui: &crate::Shell| (ui.get_resources_x(), ui.get_tempo_x(), ui.get_pill_x(), ui.get_pill_width(), ui.get_actions_mode(), ui.get_meters_mode(), ui.get_rec_width());
    let before = place(&ui);
    bar.set_loading("Loading The Very Long Name Of A Performance X".into()); // 45 characters
    f.screen.frame();
    f.screen.frame();
    assert_eq!(place(&ui), before, "the bar is as it was");
    let (chip, pill) = (ui.get_loading_width(), ui.get_pill_width());
    assert!(chip > 0. && chip <= pill / 2. + 0.5, "the chip {chip} of the title's {pill}");
    shots(&mut f, "loading-long-name", (100, 0, 600, 56));
    // And with the keyboard's narrowest bar's neighbors: a short name takes only its own width.
    bar.set_loading("Loading Dusk".into());
    f.screen.frame();
    assert!(ui.get_loading_width() < chip);
    assert_eq!(place(&ui), before);
}
