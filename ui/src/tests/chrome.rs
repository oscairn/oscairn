// SPDX-License-Identifier: AGPL-3.0-or-later
//! The top bar carries the chrome: Back, the title of what is open, the screen's own actions. No screen draws a
//! Back button or a title row; Back works from the bar by touch and by the dial, and by a device's back button,
//! on every screen; the title is the path of what is open and shortens from the left at 826 and 640 across; and
//! every screen has the old header row's height for its content.

use super::*;
use crate::shell::Screen as S;
use crate::topbar::path_options;

/// A shell on a fake engine that has said hello, with `screen` shown at once.
fn on(name: &str, screen: S) -> Fixture {
    let mut f = Fixture::new(name);
    f.engine.answer(); // session.hello
    f.pump();
    shell::goto(screen);
    f
}

fn s(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

// A structural test over the screens, as the one that finds a literal color: no screen's own slint draws a Back
// button, has a Back callback or a header row, or a title in the header's place (a bold title-size text at the
// margin). The overlays (a menu's Back, the keyboards' Cancel) are not screens.
#[test]
fn no_screen_draws_its_own_back_or_title_row() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("slint");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let screen = ["home", "setlist", "play", "part", "parts", "mixer", "controls", "routing", "grid"].iter().any(|s| name == format!("{s}.slint"));
        if !screen {
            continue;
        }
        checked += 1;
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            assert!(!code.contains("\"Back\""), "{name}:{}: a Back button: {line}", n + 1);
            assert!(!code.contains("callback back"), "{name}:{}: a Back callback: {line}", n + 1);
            assert!(!code.contains("property <length> header"), "{name}:{}: a header row: {line}", n + 1);
            if code.contains("font-size: Theme.title") {
                // The Text this belongs to: from its opening line, is it placed at the top margin?
                let start = (0..=n).rev().find(|i| lines[*i].trim_end().ends_with("Text {")).unwrap_or(n);
                let block = lines[start..=n].join("\n");
                assert!(!block.contains("y: Theme.margin;"), "{name}:{}: a title row at the top: {line}", n + 1);
            }
        }
    }
    assert_eq!(checked, 9, "the screens were read");
}

// The path the bar's title shows: each option drops one more leading part and says so; the last always has the last part.
#[test]
fn the_path_options_drop_parts_from_the_left() {
    assert_eq!(path_options(&s(&["Keys"])), ["Keys"]);
    assert_eq!(path_options(&s(&["Keys", "Insert effects"])), ["Keys › Insert effects", "… › Insert effects"]);
    assert_eq!(path_options(&s(&["Keys", "Insert effects", "Chorus"])), ["Keys › Insert effects › Chorus", "… › Insert effects › Chorus", "… › Chorus"]);
    assert_eq!(path_options(&s(&["A", "B", "C", "D", "E"])), ["A › B › C › D › E", "… › B › C › D › E", "… › C › D › E", "… › E"]);
    assert_eq!(path_options(&s(&["", "Parts", ""])), ["Parts"], "an empty part is not a part");
    assert_eq!(path_options(&[]), [""]);
}

// Every screen but Home has Back on the bar, and it works: by a tap, by the dial (the bar's first stop, then a press),
// by Escape and by the device's back button (`cancel` in the panel map, which the engine counts and the shell reads), from
// wherever the screen's own cursor is, and the screen goes to the one it came from.
#[test]
fn back_works_on_every_screen_by_touch_by_the_dial_and_by_the_devices_button() {
    let cases = [
        (S::Setlist, S::Home),
        (S::Play, S::Home),
        (S::Part, S::Play),
        (S::Parts, S::Play),
        (S::Mixer, S::Parts),
        (S::Controls, S::Parts),
        (S::Routing, S::Parts),
        (S::Chain, S::Part),
        (S::Sounds, S::Part),
        (S::Scene, S::Play),
        (S::Params, S::Chain),
    ];
    for (screen, parent) in cases {
        for how in ["touch", "dial", "escape", "device"] {
            let mut f = on(&format!("back-{how}-{}", screen as i32), screen);
            let ui = f.screen.ui.clone_strong();
            let at = format!("{screen:?} by {how}");
            assert_eq!(ui.get_screen(), screen as i32, "{at}");
            assert!(ui.global::<Bar>().get_back(), "{at}: the bar has Back");
            // The screen's own cursor is somewhere, as it may be when the player wants out.
            shell::act(Action::Move { pane: None, by: 1 });
            match how {
                "touch" => ui.global::<Bar>().invoke_back_tapped(),
                "dial" => {
                    shell::act(Action::Move { pane: Some(2), by: 1 });
                    assert!(ui.global::<Bar>().get_back_cursor() && ui.global::<crate::Theme>().get_bar_cursor(), "{at}: the dial reaches Back, the one cursor");
                    shell::act(Action::Confirm(None));
                }
                "escape" => assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Escape)), "{at}"),
                _ => {
                    f.navigation(json!({"cancel": 0})); // the first read is a baseline
                    f.navigation(json!({"cancel": 1}));
                }
            }
            // Chain, Sounds and Params go up a level of their own first, when they have one open; none has here.
            assert_eq!(ui.get_screen(), parent as i32, "{at}: back goes to {parent:?}");
        }
    }
    // Home has nowhere to go back to.
    let f = on("back-home", S::Home);
    assert!(!f.screen.ui.global::<Bar>().get_back() && !f.screen.ui.global::<Bar>().get_home_button());
}

// The path title on the screens that need no data to know it, and what Back and Home show.
#[test]
fn the_title_is_the_path_of_what_is_open() {
    for (screen, want) in [(S::Home, "Home"), (S::Setlist, "Setlist"), (S::Parts, "Parts"), (S::Mixer, "Parts › Mixer"), (S::Controls, "Parts › Controls"), (S::Routing, "Parts › Routing")] {
        let f = on(&format!("title-{}", screen as i32), screen);
        assert_eq!(title(&f.screen.ui), want, "{screen:?}");
        let bar = f.screen.ui.global::<Bar>();
        assert_eq!((bar.get_home_button(), bar.get_back(), bar.get_title_field()), (screen != S::Home, screen != S::Home, false), "{screen:?}");
    }
    // Play: the Performance's name in a field.
    let f = on("title-play", S::Play);
    assert!(f.screen.ui.global::<Bar>().get_title_field(), "Play's title is the Performance's name, a field");
}

/// Sets a title and gives back what the bar made of it at `logical` across.
fn shown(f: &mut Fixture, parts: &[&str], logical: f32) -> (String, bool, bool) {
    f.screen.set_scale(1280. / logical);
    let bar = f.screen.ui.global::<Bar>();
    let options: Vec<slint::SharedString> = path_options(&s(parts)).into_iter().map(Into::into).collect();
    bar.set_path(slint::ModelRc::new(slint::VecModel::from(options)));
    f.screen.frame();
    f.screen.frame();
    (f.screen.ui.get_title_shown().to_string(), f.screen.ui.get_title_shrunk(), f.screen.ui.get_title_cut())
}

// A path shortens from the left when it does not fit, then its last part shrinks to 70% of the type, and only then is cut.
// At 826 (the keyboard's screen at 1.55) and 640 across.
#[test]
fn a_long_path_is_shortened_from_the_left_shrunk_to_seventy_percent_and_then_cut() {
    let mut f = on("title-fit", S::Parts);
    let deep = ["Rhodes Stage Piano", "Insert effects", "Split: frequency bands", "Band 2"];
    let whole = deep.join(" › ");
    let (at_1280, shrunk, cut) = shown(&mut f, &deep, 1280.);
    assert_eq!((at_1280.as_str(), shrunk, cut), (whole.as_str(), false, false), "room enough: the whole path");
    let (at_826, shrunk, cut) = shown(&mut f, &deep, 826.);
    assert!(at_826.starts_with("… › ") && at_826.ends_with("Band 2") && !shrunk && !cut, "826: shortened from the left, whole type: {at_826:?}");
    let (at_640, shrunk, cut) = shown(&mut f, &deep, 640.);
    assert!(at_640.starts_with("… › ") && at_640.ends_with("Band 2") && at_640.len() <= at_826.len() && !cut, "640: shorter still: {at_640:?} shrunk {shrunk}");
    // A last part longer than the room at 640: 70% of the type first, the cut after.
    let long = ["Parts", "A very long name of a part, longer than any bar has room for at all"];
    let (text, shrunk, cut) = shown(&mut f, &long, 640.);
    assert!(text.starts_with("… › ") && shrunk && cut, "{text:?}: shortened, at 70% and then cut");
    let medium = ["Parts", "Resonant Filter Number Two"];
    let (text, shrunk, cut) = shown(&mut f, &medium, 640.);
    assert!(text.starts_with("… › ") && shrunk && !cut, "{text:?}: 70% was enough: shrunk {shrunk}, cut {cut}");
}

/// What a screen has for its content at the keyboard's 826 x 465: the figures the top bar's row gave it.
fn room(screen: S) -> Vec<(&'static str, f32)> {
    let mut f = on(&format!("room-{}", screen as i32), screen);
    f.screen.set_scale(1.55);
    for _ in 0..3 {
        f.screen.frame();
    }
    let ui = f.screen.ui.clone_strong();
    match screen {
        S::Setlist => vec![("slots a view", ui.global::<SetlistState>().get_slots_per_view() as f32)],
        S::Part | S::Chain | S::Sounds | S::Scene => vec![("rows a view", ui.global::<crate::PartState>().get_rows_per_view() as f32), ("panel lines", ui.global::<crate::PartState>().get_info_per_view() as f32)],
        S::Parts => vec![("list height", ui.global::<crate::PartsState>().get_area_height())],
        S::Mixer => vec![("strips height", ui.global::<crate::MixerState>().get_strips_height())],
        S::Controls => vec![("list height", ui.global::<crate::ControlsState>().get_area_height())],
        S::Routing => vec![("canvas height", ui.global::<crate::RoutingState>().get_canvas_height())],
        S::Params => vec![("content height", ui.global::<crate::GridState>().get_view_height())],
        _ => vec![],
    }
}

// Every screen has the old header row's room for its content: a touch target and a gap, 48 logical pixels at the
// theme's own spacing. The figures before are from the same measure run on the tree before the change (bc6e42d6).
#[test]
fn every_screen_gained_the_header_rows_height() {
    let header = 44. + 4.; // Theme.touch + Theme.gap, spacing 1
    let before = BEFORE;
    for (screen, was) in before {
        let now = room(screen);
        for ((name, then), (now_name, now)) in was.iter().zip(&now) {
            assert_eq!(name, now_name);
            if name.contains("height") {
                assert!((now - then - header).abs() < 1., "{screen:?} {name}: {then} before, {now} now, and {header} more were the header's");
            } else {
                assert!(now >= then, "{screen:?} {name}: {then} before, {now} now");
            }
        }
        eprintln!("ROOM {screen:?}: before {was:?}, now {now:?}");
    }
}

/// The figures of `room` before the top bar took the headers over, measured with this test's `room` on bc6e42d6.
const BEFORE: [(S, &[(&str, f32)]); 10] = [
    (S::Part, &[("rows a view", 6.), ("panel lines", 13.)]),
    (S::Chain, &[("rows a view", 6.), ("panel lines", 13.)]),
    (S::Sounds, &[("rows a view", 6.), ("panel lines", 13.)]),
    (S::Scene, &[("rows a view", 6.), ("panel lines", 13.)]),
    (S::Parts, &[("list height", 258.516)]),
    (S::Mixer, &[("strips height", 331.516)]),
    (S::Controls, &[("list height", 283.516)]),
    (S::Routing, &[("canvas height", 259.516)]),
    (S::Params, &[("content height", 312.516)]),
    (S::Setlist, &[("slots a view", 4.)]),
];

/// The shell's answers to what it asks, until it asks no more: enough for every screen to show a Performance.
fn quiet(f: &mut Fixture, graph: &serde_json::Value) {
    let sounds: Vec<_> = (0..12).map(|i| json!({"id": format!("s{i}"), "name": format!("Sound {i}")})).collect();
    while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(50)) {
        let result = match r["method"].as_str().unwrap_or_default() {
            "performance.info" => json!({"id": "p1", "name": "Grand Piano", "unsaved": false}),
            "graph.get" => graph.clone(),
            "edit.history" => json!({"undo": "Volume of Keys", "redo": ""}),
            "params.list" => super::pages::param_list(),
            "params.get" => json!({"values": []}),
            "params.text" => json!({"text": "t"}),
            "window.info" => json!({"available": false}),
            "library.query" => json!({"items": sounds, "total": 12}),
            "session.status" => json!({"midiInputs": [], "failures": []}),
            "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
            "parts.meters" | "wires.meters" => json!({"meters": []}),
            "memory.state" => json!({"systemAvailable": 496u64 << 20, "device": 0}),
            "transport.get" => json!({"tempo": 120, "playing": false}),
            "record.get" => json!({"takes": []}),
            _ => json!({}),
        };
        f.reply(&r, json!({"result": result}));
    }
}

// Pictures of every screen, through the shell and a fake engine, in three windows (the keyboard's 1280x720 at 1.55, a
// desktop's at 1, a small device's 800x480) and three themes (dark, and the two odd ones), in target/screens as
// chrome-WINDOW-THEME-SCREEN.png, with the bar enlarged in target/theme-shots as chrome-bar-*. Each step that goes
// back goes by the bar's Back, so the pictures are of a shell that was driven.
#[test]
fn pictures_of_every_screen_with_the_chrome() {
    let graph = super::parts::parts_graph();
    let mut f = super::bar::play_on_the_shell("chrome-pics", json!({"id": "p1", "name": "Grand Piano", "unsaved": false}), graph.clone());
    let ui = f.screen.ui.clone_strong();
    for (window, (w, h, scale)) in [("keyboard", (1280, 720, 1.55f32)), ("desktop", (1280, 720, 1.)), ("small", (800, 480, 1.))] {
        for (theme_name, theme) in [("dark", theme::chosen(&theme::Places::default(), "oscairn-dark", "")), ("odd", odd_theme()), ("joined", joined_theme())] {
            f.screen.resize(w, h);
            f.screen.set_scale(scale);
            theme::apply(&ui, &theme);
            let shoot = |f: &mut Fixture, what: &str| {
                quiet(f, &graph);
                f.screen.frame();
                f.screen.frame();
                png(&f.screen, &format!("chrome-{window}-{theme_name}-{what}"));
                let bar_height = (48. * scale) as i32;
                png_crop(&f.screen, &format!("chrome-bar-{window}-{theme_name}-{what}"), (0, 0, w as i32, bar_height), 2);
            };
            let back = |f: &mut Fixture| {
                ui.global::<Bar>().invoke_back_tapped();
                quiet(f, &graph);
            };
            // Home's screens first, then Play and what opens from it.
            ui.global::<Bar>().invoke_home();
            shoot(&mut f, "home");
            ui.global::<HomeState>().invoke_tapped(2);
            shoot(&mut f, "setlist");
            back(&mut f);
            ui.global::<HomeState>().invoke_tapped(0);
            shoot(&mut f, "play");
            ui.global::<crate::PlayState>().invoke_part_tapped(0);
            shoot(&mut f, "part");
            shell::act(Action::End(1));
            shell::act(Action::End(1));
            shell::act(Action::Move { pane: None, by: -2 });
            shell::act(Action::Confirm(None));
            shoot(&mut f, "chain");
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Confirm(None));
            quiet(&mut f, &graph);
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Confirm(None));
            assert_eq!(ui.get_screen(), 7, "the parameter page");
            shoot(&mut f, "params");
            back(&mut f);
            back(&mut f);
            back(&mut f);
            assert_eq!(ui.get_screen(), 4);
            shell::act(Action::End(1));
            shell::act(Action::End(1));
            shell::act(Action::Move { pane: None, by: -3 });
            shell::act(Action::Confirm(None));
            shoot(&mut f, "sounds");
            back(&mut f);
            back(&mut f);
            assert_eq!(ui.get_screen(), 3);
            ui.global::<Bar>().invoke_action(3); // Scene
            shoot(&mut f, "scene");
            back(&mut f);
            ui.global::<Bar>().invoke_action(5); // Edit
            shoot(&mut f, "parts");
            for (tool, what) in [(0, "mixer"), (1, "controls"), (2, "routing")] {
                ui.global::<crate::PartsState>().invoke_tool(tool);
                shoot(&mut f, what);
                back(&mut f);
                assert_eq!(ui.get_screen(), 9);
            }
            back(&mut f);
            assert_eq!(ui.get_screen(), 3);
        }
    }
}
