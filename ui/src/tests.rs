// SPDX-License-Identifier: AGPL-3.0-or-later
//! Scene tests: the shell's window rendered headless by Slint's software renderer into a buffer
//! (no window is shown), the area each change repaints taken from the renderer, and the shell
//! driven through a socket by a fake engine. Pictures for a look by eye go to target/screens/.

use crate::client::tests::{FakeEngine, WAIT};
use crate::client::Event;
use crate::shell::{self, Request};
use crate::theme;
use crate::topbar::{MeterSettings, Show};
use crate::nav::Action;
use crate::{Bar, HomeState, SetlistState, Shell};
use serde_json::json;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, Model, PhysicalSize, Rgb8Pixel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

pub const W: usize = 1280;
pub const H: usize = 720;

/// A platform with no display: each window is a buffer the test renders into.
struct Headless;

thread_local! { static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) }; }

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
        Ok(window)
    }
}

/// A rectangle in the window's pixels: x, y, width, height.
pub type Rect = (i32, i32, i32, i32);

pub struct Screen {
    pub ui: Shell,
    window: Rc<MinimalSoftwareWindow>,
    pub pixels: Vec<Rgb8Pixel>,
}

impl Screen {
    pub fn new() -> Screen {
        // The data and configuration folders of the tests' own, never the user's: the shell
        // keeps the Setlist's state and reads the user's choices there. Set once, before any
        // test reads them.
        static FOLDERS: std::sync::Once = std::sync::Once::new();
        FOLDERS.call_once(|| {
            let root = std::env::temp_dir().join(format!("oscairn-ui-tests-{}", std::process::id()));
            std::env::set_var("OSCAIRN_DATA_HOME", root.join("data"));
            std::env::set_var("OSCAIRN_CONFIG_HOME", root.join("config"));
        });
        let _ = slint::platform::set_platform(Box::new(Headless)); // once per test thread
        let ui = Shell::new().unwrap();
        let window = WINDOW.with(|w| w.borrow_mut().take()).unwrap();
        window.set_size(PhysicalSize::new(W as u32, H as u32));
        ui.show().unwrap();
        Screen { ui, window, pixels: vec![Rgb8Pixel::default(); W * H] }
    }

    /// Renders what changed since the last frame: the bounding box of what was repainted, None for nothing.
    pub fn frame(&mut self) -> Option<Rect> {
        slint::platform::update_timers_and_animations();
        let mut repainted = None;
        self.window.draw_if_needed(|renderer| {
            let region = renderer.render(&mut self.pixels, W);
            let (o, s) = (region.bounding_box_origin(), region.bounding_box_size());
            repainted = (s.width > 0 && s.height > 0).then_some((o.x, o.y, s.width as i32, s.height as i32));
        });
        repainted
    }

    /// The window as a PPM picture in target/screens/NAME.ppm.
    pub fn save(&self, name: &str) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screens");
        std::fs::create_dir_all(&dir).unwrap();
        let mut ppm = format!("P6 {W} {H} 255\n").into_bytes();
        ppm.extend(self.pixels.iter().flat_map(|p| [p.r, p.g, p.b]));
        std::fs::write(dir.join(format!("{name}.ppm")), ppm).unwrap();
    }

    /// The resource section's rectangle in the window (the top bar is at the window's top left).
    pub fn resources(&self) -> Rect {
        let ui = &self.ui;
        (ui.get_resources_x() as i32, 0, ui.get_resources_width().ceil() as i32, 48)
    }
}

/// True when `inner` lies within `outer`.
pub fn within(inner: Rect, outer: Rect) -> bool {
    inner.0 >= outer.0 && inner.1 >= outer.1 && inner.0 + inner.2 <= outer.0 + outer.2 && inner.1 + inner.3 <= outer.1 + outer.3
}

/// The pixels outside `r` that differ between two frames.
fn changed_outside(a: &[Rgb8Pixel], b: &[Rgb8Pixel], r: Rect) -> usize {
    (0..W * H).filter(|&i| !within(((i % W) as i32, (i / W) as i32, 1, 1), r) && a[i] != b[i]).count()
}

fn settings(ui: &Shell, cpu: Show, ram: Show) {
    shell::show_settings(ui, MeterSettings { cpu, ram });
}

fn meters(ui: &Shell, cpu: i32, ram: i32) {
    let bar = ui.global::<Bar>();
    bar.set_cpu(cpu);
    bar.set_cpu_known(true);
    bar.set_ram(ram);
    bar.set_ram_known(true);
}

// The resource section's width depends on what it shows, never on the figures: with "1%" and
// "100%", tempos from 5 to 999.9 and a very long Performance name, neither it nor the tempo
// beside it moves, for every one of the sixteen settings.
#[test]
fn resource_section_has_a_fixed_width() {
    fixed_width(Screen::new(), "topbar");
}

// The same with a theme of extreme colors and the largest font scale.
#[test]
fn resource_section_has_a_fixed_width_with_a_larger_font() {
    let s = Screen::new();
    theme::apply(&s.ui, &odd_theme());
    fixed_width(s, "topbar-odd");
}

fn fixed_width(mut s: Screen, pictures: &str) {
    let all = [Show::None, Show::Bar, Show::Percent, Show::Both];
    for cpu in all {
        for ram in all {
            settings(&s.ui, cpu, ram);
            let mut seen = Vec::new();
            for (c, r, tempo, name) in [(1, 100, "5.0 BPM", "A"), (100, 1, "999.9 BPM", &*"Very long Performance name ".repeat(12)), (42, 7, "120.0 BPM", "")] {
                meters(&s.ui, c, r);
                let bar = s.ui.global::<Bar>();
                bar.set_tempo(tempo.into());
                bar.set_performance(name.into());
                bar.set_scene("Chorus with a long name".into());
                s.frame();
                seen.push((s.ui.get_resources_x(), s.ui.get_resources_width(), s.ui.get_tempo_x()));
            }
            assert!(seen.windows(2).all(|w| w[0] == w[1]), "{cpu:?}/{ram:?}: {seen:?}");
            assert_eq!(seen[0].1 == 0., cpu == Show::None && ram == Show::None, "{cpu:?}/{ram:?}: no room only when it shows nothing");
            assert_eq!(seen[0].0 + seen[0].1 + 10., W as f32, "{cpu:?}/{ram:?}: at the right edge");
            s.save(&format!("{pictures}-{cpu:?}-{ram:?}").to_lowercase());
        }
    }
}

// A meter update repaints its own digits and bar, inside the resource section, and no pixel
// outside it changes. At rest, and for a value set again unchanged, nothing is drawn.
#[test]
fn a_meter_update_repaints_only_the_resource_section() {
    let mut s = Screen::new();
    settings(&s.ui, Show::Both, Show::Both);
    s.ui.global::<Bar>().set_performance("Strings".into());
    s.ui.global::<Bar>().set_tempo("120.0 BPM".into());
    meters(&s.ui, 1, 30);
    s.frame();
    assert_eq!(s.frame(), None, "at rest");
    let section = s.resources();
    for (cpu, ram) in [(100, 30), (100, 31), (7, 31), (88, 95)] {
        let before = s.pixels.clone();
        meters(&s.ui, cpu, ram);
        let repainted = s.frame().expect("a change draws");
        assert!(within(repainted, section), "{cpu}/{ram}: repainted {repainted:?}, the section is {section:?}");
        assert_eq!(changed_outside(&before, &s.pixels, section), 0);
        meters(&s.ui, cpu, ram);
        assert_eq!(s.frame(), None, "{cpu}/{ram} set again");
    }
    s.save("topbar-updated");
}

/// The shell against a fake engine on a socket: the client's events come back to this thread
/// through a channel, as the event loop brings them in the program.
struct Fixture {
    screen: Screen,
    engine: FakeEngine,
    events: mpsc::Receiver<Event<Request>>,
    _listener: std::os::unix::net::UnixListener,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let screen = Screen::new();
        let path = std::env::temp_dir().join(format!("oscairn-ui-shell-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (tx, events) = mpsc::channel();
        shell::start(&screen.ui, path, &crate::profile::Profile::load(Some("fantom-07")).unwrap(), true, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let engine = FakeEngine::new(listener.accept().unwrap().0);
        Fixture { screen, engine, events, _listener: listener }
    }

    /// Hands the shell the client's next event.
    fn pump(&mut self) {
        shell::dispatch(self.events.recv_timeout(WAIT).unwrap());
    }

    /// The next request, answered with `result`.
    fn answer(&mut self, method: &str, result: serde_json::Value) {
        let request = self.engine.request();
        assert_eq!(request["method"], method);
        self.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        self.pump();
    }
}

// What the shell asks the engine and how the answers reach the top bar; a status sooner than
// half a second after the last moves nothing; a lost engine is said.
#[test]
fn the_top_bar_follows_the_engine() {
    let mut f = Fixture::new("topbar");
    f.engine.answer(); // session.hello
    f.pump();
    assert_eq!(f.engine.request()["params"]["topics"], json!(["status", "graph.changed", "controllers"]));
    assert_eq!(f.engine.request()["params"], json!({"lists": true}));
    let shell = f.engine.request();
    assert!(shell["method"] == "memory.shell" && shell["params"]["bytes"].as_u64().unwrap() > 1 << 20, "its own memory: {shell}");
    f.answer("performance.reopen", json!({"notice": ""}));
    f.answer("performance.info", json!({"name": "Grand Piano", "unsaved": false}));
    f.answer("graph.get", json!({"scene": 1, "scenes": [{"id": 1, "name": "Verse"}]}));
    f.answer("library.query", json!({"items": [], "total": 24}));
    f.answer("library.query", json!({"items": [], "total": 1}));
    f.answer("library.query", json!({"items": [], "total": 0})); // the Setlist's setlists
    f.answer("library.query", json!({"items": [], "total": 0})); // and Performances
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": {"cpuLoad": 0.42, "audioDevice": "null"}})));
    f.pump();
    f.answer("memory.state", json!({"systemAvailable": 496u64 << 20, "device": 0}));
    f.answer("transport.get", json!({"tempo": 96.5, "playing": false}));
    let bar = f.screen.ui.global::<Bar>();
    assert_eq!((bar.get_performance().as_str(), bar.get_scene().as_str(), bar.get_tempo().as_str()), ("Grand Piano", "Verse", "96.5 BPM"));
    assert_eq!((bar.get_cpu(), bar.get_cpu_known(), bar.get_ram(), bar.get_ram_known(), bar.get_ram_level()), (42, true, 50, true, 0));
    f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": {"cpuLoad": 0.95}})));
    f.pump();
    assert_eq!(f.screen.ui.global::<Bar>().get_cpu(), 42, "within half a second of the last: not shown");
    let lines: Vec<String> = f.screen.ui.global::<HomeState>().get_tiles().iter().map(|t| t.line.to_string()).collect();
    assert_eq!(lines, ["Grand Piano: play or edit it", "24 Performances, 1 Sound", "Songs in order, for the stage", "Patterns and steps", "No audio device (--audio null)"]);
    f.screen.frame();
    f.screen.save("topbar-engine");
    f.engine.writer.shutdown(std::net::Shutdown::Both).unwrap();
    f.pump(); // the subscribe, controllers.view and memory.shell still out, failed
    f.pump();
    f.pump();
    f.pump();
    assert!(f.screen.ui.get_engine().starts_with("lost"));
}

/// Home's tile `n` in the window, as home.slint lays it out (margin 6, gap 4, 120 tall).
fn tile(s: &Screen, n: i32) -> Rect {
    let columns = s.ui.get_home_columns();
    let width = (W as i32 - 12 - (columns - 1) * 4) / columns;
    (6 + (n % columns) * (width + 4), 48 + 6 + (n / columns) * 124, width + 1, 120)
}

// Home: the profile's apps; keys, the panel's roles and taps move the cursor, a move repaints
// the two tiles it leaves and reaches, nothing opens until confirmed; Setlist is a Rust screen,
// Settings a JUCE one the display is handed to.
#[test]
fn home_moves_by_keys_panel_and_touch() {
    let mut f = Fixture::new("home");
    f.engine.answer(); // session.hello
    f.pump();
    let s = &mut f.screen;
    let names: Vec<String> = s.ui.global::<HomeState>().get_tiles().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, ["Performance", "Library", "Setlist", "Groovebox", "Settings"]);
    s.frame();
    s.save("home");
    assert_eq!(s.frame(), None, "at rest");
    let key = |s: &Screen, k: slint::platform::Key| s.ui.invoke_key(slint::SharedString::from(k));
    let cursor = |s: &Screen| s.ui.global::<HomeState>().get_tiles().iter().position(|t| t.cursor);
    assert!(key(s, slint::platform::Key::RightArrow));
    assert_eq!(cursor(s), Some(0), "the first move shows the cursor on the first tile");
    s.frame();
    assert!(key(s, slint::platform::Key::RightArrow));
    let repainted = s.frame().unwrap();
    let (a, b) = (tile(s, 0), tile(s, 1));
    assert!(within(repainted, (a.0, a.1, b.0 + b.2 - a.0, a.3)), "a move repainted {repainted:?}: tiles 0 and 1 are {a:?} {b:?}");
    assert_eq!((cursor(s), s.ui.get_screen()), (Some(1), 0), "moved, nothing opened");
    // The panel, through the engine's navigation totals: the first a baseline, then the VALUE
    // dial two steps on (the pane last touched), then one back and ENTER.
    let s = &mut f.screen;
    let _ = s;
    let navigation = |f: &mut Fixture, n: serde_json::Value| {
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": n}})));
        f.pump();
    };
    navigation(&mut f, json!({"move": 10, "confirm": 5}));
    navigation(&mut f, json!({"move": 12, "confirm": 5}));
    assert_eq!(cursor(&f.screen), Some(3));
    navigation(&mut f, json!({"move": 11, "confirm": 6}));
    let s = &mut f.screen;
    assert_eq!((s.ui.get_screen(), s.ui.global::<Bar>().get_where().as_str(), s.ui.global::<Bar>().get_home_button()), (1, "Setlist", true));
    assert!(key(s, slint::platform::Key::Escape), "Cancel goes Home");
    assert_eq!((s.ui.get_screen(), cursor(s)), (0, None), "the cursor dropped with its screen");
    s.ui.global::<HomeState>().invoke_tapped(4); // Settings: a JUCE screen
    assert_eq!((s.ui.get_screen(), s.ui.get_opening().as_str(), shell::handover()), (2, "Settings", Some("settings")));
    let file = std::env::temp_dir().join(format!("oscairn-ui-handover-{}", std::process::id()));
    shell::write_handover(&file, shell::handover().unwrap()).unwrap();
    let written = std::fs::read_to_string(&file).unwrap();
    let _ = std::fs::remove_file(&file);
    assert_eq!(written, "settings\n", "a whole line, as the start script reads it");
    s.frame();
    s.save("home-opening-settings");
    shell::act(Action::Move { pane: None, by: 1 });
    assert_eq!(f.screen.ui.get_screen(), 2, "while the display goes to the JUCE app, nothing is taken");
}

/// A theme of extreme colors at the largest font scale, with keys missing (the default's then).
fn odd_theme() -> theme::Loaded {
    let doc = json!({"version": 7, "id": "odd", "fontScale": 1.4, "radius": 16,
        "colors": {"background": "#000000", "surface": "#FF00FF", "text": "#FFFFFF", "muted": "#00FFFF", "accent": "#FFFF00", "border": "nonsense"}});
    let places = theme::Places::default();
    let base = theme::chosen(&places, "", "");
    theme::Loaded { id: "odd".into(), doc: theme::merged(&base.doc, &doc), folder: None }
}

// Every built-in theme and the odd one show the top bar and Home; a switch repaints once, then
// nothing; a key a theme lacks is the default's.
#[test]
fn themes_switch_live_and_repaint_once() {
    let mut f = Fixture::new("themes");
    f.engine.answer(); // session.hello
    f.pump();
    f.answer("subscribe", json!({"accepted": true}));
    f.answer("controllers.view", json!({"accepted": true}));
    f.answer("memory.shell", json!({"accepted": true}));
    f.answer("performance.reopen", json!({"notice": ""})); // no busy line counting up meanwhile
    let s = &mut f.screen;
    meters(&s.ui, 42, 77);
    s.frame();
    let mut loaded: Vec<theme::Loaded> = crate::profile::THEMES.iter().map(|(id, _)| theme::chosen(&theme::Places::default(), id, "")).collect();
    loaded.push(odd_theme());
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: repainted", t.id);
        assert_eq!(s.frame(), None, "{}: once", t.id);
        s.save(&format!("theme-{}", t.id));
    }
    let g = s.ui.global::<crate::Theme>();
    assert_eq!((g.get_font_scale(), g.get_radius()), (1.4, 16.));
    assert_eq!(g.get_border(), slint::Color::from_rgb_u8(0x3a, 0x41, 0x4b), "an unusable color: the default theme's");
    assert_eq!(g.get_focus(), slint::Color::from_rgb_u8(0xff, 0xff, 0x00), "focus: the accent when the theme has none");
}

impl Fixture {
    /// Answers the next `n` requests as a quiet engine would: setlists, Performances and a setlist
    /// from `content`, anything else `{}`.
    fn serve(&mut self, n: usize, content: &serde_json::Value) {
        for _ in 0..n {
            let request = self.engine.request();
            let method = request["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "library.query" if request["params"]["type"] == "oscairn.setlist" => json!({"items": content["setlists"], "total": 2}),
                "library.query" if request["params"]["limit"] == 1000 => json!({"items": content["performances"], "total": 40}),
                "library.query" => json!({"items": [], "total": 0}),
                "setlists.get" => json!({"setlist": content["setlist"]}),
                _ => json!({}),
            };
            self.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
            self.pump();
        }
    }

    fn reply(&mut self, request: &serde_json::Value, body: serde_json::Value) {
        let mut doc = json!({"jsonrpc": "2.0", "id": request["id"]});
        doc.as_object_mut().unwrap().extend(body.as_object().unwrap().clone());
        self.engine.send(&format!("{doc}\n"));
        self.pump();
    }

    fn navigation(&mut self, n: serde_json::Value) {
        self.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": n}})));
        self.pump();
    }
}

// The Setlist on the shell: two panes, E1 (pane 0) through the setlists and E6 (pane 1) or the
// dial through the slots; a view of slots at a time, the cursor past it turning a whole view; a
// slot opens only on Confirm or a tap; a slot chosen while one opens waits, the last chosen winning;
// a busy answer is tried again as the next; Cancel drops the cursor, then goes back.
#[test]
fn setlist_by_panes_pages_and_the_last_one_chosen() {
    let performances: Vec<_> = (0..40).map(|i| json!({"id": format!("p{i}"), "name": format!("Song {i}"), "color": "", "path": format!("/lib/p{i}")})).collect();
    let slots: Vec<_> = (0..30).map(|i| json!({"id": format!("p{i}"), "name": format!("Song {i}"), "color": "", "label": ""})).collect();
    let content = json!({
        "setlists": [{"id": "s1", "name": "Friday"}, {"id": "s2", "name": "Saturday"}],
        "performances": performances,
        "setlist": {"format": "oscairn.setlist", "version": 2, "info": {"id": "s1", "name": "Friday"}, "display": {"mode": "list"},
            "pages": [{"name": "Set 1", "slots": slots}, {"name": "Encore", "slots": [null, {"id": "p39", "name": "Song 39", "color": "#3366cc", "label": ""}]}]},
    });
    let mut f = Fixture::new("setlist");
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(2); // Setlist
    // Everything asked on connecting answered: the shell's eight, the Setlist's two, then the setlist.
    f.serve(11, &content);
    let rows = |f: &Fixture| f.screen.ui.global::<SetlistState>().get_slots().iter().collect::<Vec<_>>();
    let per_view = f.screen.ui.global::<SetlistState>().get_slots_per_view() as usize;
    assert!(per_view >= 5, "a list view holds {per_view}");
    assert_eq!((rows(&f).len(), rows(&f)[0].title.as_str(), f.screen.ui.global::<SetlistState>().get_name().as_str()), (per_view, "Song 0", "Friday"));
    f.screen.frame();
    f.screen.save("setlist");
    assert_eq!(f.screen.frame(), None, "at rest");
    // The panel: a baseline, then E6 (pane 1) twice: the cursor shown on the first slot, then on.
    let nav = |m0: i64, m1: i64, m: i64, c1: i64, cancel: i64| json!({"move:0": m0, "move:1": m1, "move": m, "confirm:1": c1, "cancel": cancel});
    f.navigation(nav(0, 0, 0, 0, 0));
    f.navigation(nav(0, 1, 0, 0, 0));
    f.screen.frame();
    f.navigation(nav(0, 2, 0, 0, 0));
    let cursor = |f: &Fixture| rows(f).iter().position(|r| r.cursor);
    assert_eq!(cursor(&f), Some(1));
    let repainted = f.screen.frame().unwrap();
    assert!(repainted.3 <= 2 * 56 + 4 + 2, "a move repaints the two rows: {repainted:?}");
    // The dial, on the pane last touched: past the view, a whole view on, never scrolled.
    let pv = per_view as i64;
    f.navigation(nav(0, 2, pv, 0, 0));
    assert_eq!((cursor(&f), rows(&f)[0].number as usize), (Some(1), per_view + 1), "the next view, the cursor on its second row");
    assert!(f.screen.ui.get_screen() == 1 && rows(&f).iter().all(|r| !r.playing), "nothing opened by moving");
    // Confirm on pane 1: that slot opens.
    f.navigation(nav(0, 2, pv, 1, 0));
    let first = f.engine.request();
    assert_eq!((first["method"].as_str(), first["params"]["id"].as_str()), (Some("library.load"), Some(&*format!("p{}", per_view + 1))));
    // While it opens: two more chosen by tap; only the last opens next.
    f.screen.ui.global::<SetlistState>().invoke_tapped_slot(3);
    f.screen.ui.global::<SetlistState>().invoke_tapped_slot(4);
    f.reply(&first, json!({"result": {}}));
    let next = f.engine.request();
    assert_eq!(next["params"]["id"].as_str(), Some(&*format!("p{}", per_view + 4)), "the last one chosen");
    // Busy (another client's long request): tried again, and it opens.
    f.reply(&next, json!({"error": {"code": -32009, "message": "busy"}}));
    let again = f.engine.request();
    assert_eq!(again["params"]["id"], next["params"]["id"]);
    f.reply(&again, json!({"result": {}}));
    assert_eq!(f.engine.request()["method"], "performance.prepare", "the next one prepared");
    assert!(rows(&f)[4].playing);
    // E1 (pane 0): the setlists' cursor.
    f.navigation(nav(2, 2, pv, 1, 0));
    let names = f.screen.ui.global::<SetlistState>().get_setlists().iter().collect::<Vec<_>>();
    assert_eq!((names[0].cursor, names[0].shown), (true, true), "shown on the first setlist by the first move");
    // Cancel: the cursors go; Cancel again: Home.
    f.navigation(nav(2, 2, pv, 1, 1));
    assert_eq!((cursor(&f), f.screen.ui.get_screen()), (None, 1));
    f.navigation(nav(2, 2, pv, 1, 2));
    assert_eq!(f.screen.ui.get_screen(), 0);
}

/// The real engine (OSCAIRN_ENGINE, which ctest gives when the build has one) with no audio
/// device, in folders of its own (never the user's), ended when dropped. None without one.
struct RealEngine {
    process: std::process::Child,
    home: std::path::PathBuf,
    socket: std::path::PathBuf,
}

impl RealEngine {
    fn start(name: &str, options: &[&str]) -> Option<RealEngine> {
        let engine = std::env::var_os("OSCAIRN_ENGINE").filter(|e| !e.is_empty())?;
        let home = std::env::temp_dir().join(format!("oscairn-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("run")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(home.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap(); // the engine's rule for its socket's folder
        let socket = home.join("run/engine.sock");
        let log = std::fs::File::create(home.join("engine.log")).unwrap();
        let process = std::process::Command::new(engine)
            .args(["--socket", socket.to_str().unwrap(), "--audio", "null", "--buffer-frames", "128"])
            .args(options)
            .envs(Self::folders(&home))
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        Some(RealEngine { process, home, socket })
    }

    /// HOME and the data and configuration folders, the temporary folder's, for any process the test starts.
    fn folders(home: &std::path::Path) -> [(&'static str, std::path::PathBuf); 3] {
        [("HOME", home.to_path_buf()), ("OSCAIRN_DATA_HOME", home.join("data")), ("OSCAIRN_CONFIG_HOME", home.join("config"))]
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.home.join("engine.log")).unwrap_or_default()
    }

    /// One request on a connection of its own, waiting up to 30 s for the engine to listen; its result.
    fn call(&self, method: &str, params: serde_json::Value) -> serde_json::Value {
        use std::io::{BufRead, Write};
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let stream = loop {
            match std::os::unix::net::UnixStream::connect(&self.socket) {
                Ok(s) => break s,
                Err(e) if std::time::Instant::now() > deadline => panic!("{method}: {e}; the engine's log:\n{}", self.log()),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        };
        stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
        writeln!(&stream, "{}", json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})).unwrap();
        for line in std::io::BufReader::new(&stream).lines() {
            let doc: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            if doc["id"] == 1 {
                assert!(doc.get("error").is_none(), "{method}: {doc}");
                return doc["result"].clone();
            }
        }
        panic!("{method}: no answer; the engine's log:\n{}", self.log())
    }
}

impl Drop for RealEngine {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// Hands the shell its events until `done` or 30 s.
fn run_shell_until(events: &mpsc::Receiver<Event<Request>>, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !done() && std::time::Instant::now() < deadline {
        if let Ok(e) = events.recv_timeout(std::time::Duration::from_millis(200)) {
            shell::dispatch(e);
        }
    }
}

// The shell against the real engine: it answers what the shell asks, the top bar and Home fill in
// from it, and the engine's status reaches the meters. Skipped without one.
#[test]
fn against_the_real_engine() {
    let Some(engine) = RealEngine::start("engine", &[]) else {
        eprintln!("against_the_real_engine: skipped, OSCAIRN_ENGINE is not set");
        return;
    };
    let screen = Screen::new();
    let (tx, events) = mpsc::channel();
    shell::start(&screen.ui, engine.socket.clone(), &crate::profile::Profile::load(Some("fantom-07")).unwrap(), true, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let bar = || screen.ui.global::<Bar>();
    run_shell_until(&events, || bar().get_cpu_known() && !bar().get_tempo().is_empty() && !bar().get_performance().is_empty());
    let shown = (bar().get_cpu_known(), bar().get_tempo().to_string(), bar().get_performance().to_string(), screen.ui.get_engine().to_string());
    let lines: Vec<String> = screen.ui.global::<HomeState>().get_tiles().iter().map(|t| t.line.to_string()).collect();
    assert!(shown.0 && !shown.1.is_empty() && !shown.2.is_empty() && shown.3.is_empty(), "shown {shown:?}; the engine's log:\n{}", engine.log());
    assert!(lines.iter().any(|l| l == "No audio device (--audio null)"), "{lines:?}");
}

/// What the engine reports of the session's setup: the budget, memory classes, capture,
/// repositories and audio, the UI's own memory (`shell`) taken out of what it is part of.
fn session_setup(engine: &RealEngine) -> serde_json::Value {
    let mut memory = engine.call("memory.state", json!({}));
    let shell = memory["shell"].as_u64().unwrap();
    let budget = engine.call("budget.estimate", json!({}))["budget"].clone();
    let status = engine.call("session.status", json!({}));
    for (key, to) in [("reserved", -1i64), ("share", 1)] {
        memory[key] = json!(memory[key].as_i64().unwrap() + to * shell as i64);
    }
    for key in ["systemAvailable", "shell", "committed", "opportunistic"] {
        memory.as_object_mut().unwrap().remove(key); // the machine's and the moment's, not the setup's
    }
    json!({
        "budget": {"cpu": budget["cpu"], "memory": budget["memory"].as_u64().map(|m| m + shell)},
        "memory": memory,
        "capture": engine.call("record.get", json!({}))["retro"]["seconds"],
        "repositories": engine.call("packages.repositories", json!({}))["repositories"],
        "audio": [&status["audioDevice"], &status["sampleRate"], &status["bufferSize"], &status["refusedLoads"]],
    })
}

// The session's setup after a Rust-path start is the JUCE path's (docs/rust-ui.md, "Session
// setup"). Rust: the engine started with the profile, as the start script's engine-args gives it
// (--device-profile), then the shell's own start against it. JUCE: an engine with no profile, as
// the app starts its own, configured by the real JUCE app's start (--connect, offscreen through
// --snapshot), and the one request a snapshot run leaves out, record.retro with the length the app
// would read (none saved: the profile's 30 s). Both report the same budget, memory classes,
// capture and repositories, the FANTOM-07's measured on a unit; each UI's own memory differs and
// is compared apart. Needs OSCAIRN_ENGINE and OSCAIRN_APP (the JUCE app), which ctest gives.
#[test]
fn session_setup_matches_the_juce_app() {
    let Some(app) = std::env::var_os("OSCAIRN_APP").filter(|a| !a.is_empty()) else {
        eprintln!("session_setup_matches_the_juce_app: skipped, OSCAIRN_APP is not set");
        return;
    };
    let Some(rust) = RealEngine::start("setup-rust", &["--device-profile", "fantom-07"]) else {
        eprintln!("session_setup_matches_the_juce_app: skipped, OSCAIRN_ENGINE is not set");
        return;
    };
    let screen = Screen::new();
    let (tx, events) = mpsc::channel();
    shell::start(&screen.ui, rust.socket.clone(), &crate::profile::Profile::load(Some("fantom-07")).unwrap(), true, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let bar = || screen.ui.global::<Bar>();
    run_shell_until(&events, || !bar().get_performance().is_empty()); // connected, memory.shell and the reopen answered before
    let shell = rust.call("memory.state", json!({}))["shell"].as_u64().unwrap();
    assert!(shell > 1 << 20, "the shell sends its own memory: {shell}");

    let juce = RealEngine::start("setup-juce", &[]).unwrap();
    juce.call("session.status", json!({})); // listening
    let out = std::process::Command::new(app)
        .args(["--connect", juce.socket.to_str().unwrap(), "--profile", "fantom-07", "--snapshot", "juce.png", "--size", "1280x720"])
        .current_dir(&juce.home)
        .envs(RealEngine::folders(&juce.home))
        .output()
        .unwrap();
    assert!(out.status.success(), "the JUCE app: {}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
    juce.call("record.retro", json!({"seconds": 30}));

    let (r, j) = (session_setup(&rust), session_setup(&juce));
    assert_eq!(r, j, "the Rust path's setup (left) and the JUCE path's (right)");
    const MIB: u64 = 1 << 20;
    let m = &r["memory"];
    let always = json!([{"app": "oscairn.performance", "reserve": 150 * MIB}, {"app": "oscairn.library", "reserve": 120 * MIB}]);
    assert_eq!(
        (&r["budget"]["cpu"], &m["device"], &m["floor"], &m["margin"], &m["available"], &m["alwaysAvailable"], &r["capture"]),
        (&json!(0.75), &json!(992 * MIB), &json!(300 * MIB), &json!(992 * MIB / 20), &json!(200 * MIB), &always, &json!(30)),
        "the FANTOM-07's, as measured on a unit after a JUCE-path start"
    );
}
