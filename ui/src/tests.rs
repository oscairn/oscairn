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
    /// The window's size in physical pixels.
    pub w: usize,
    pub h: usize,
}

impl Screen {
    pub fn new() -> Screen {
        Screen::sized(W, H)
    }

    /// A window of `w` x `h` physical pixels (a desktop window's, say).
    pub fn sized(w: usize, h: usize) -> Screen {
        // The data and configuration folders of the tests' own, never the user's: the shell
        // keeps the Setlist's state and reads the user's choices there. Set once, before any
        // test reads them.
        static FOLDERS: std::sync::Once = std::sync::Once::new();
        FOLDERS.call_once(|| {
            // Folders already given (ui/target/test.sh gives the worktree's own) are kept.
            if std::env::var_os("OSCAIRN_DATA_HOME").is_none() || std::env::var_os("OSCAIRN_CONFIG_HOME").is_none() {
                let root = std::env::temp_dir().join(format!("oscairn-ui-tests-{}", std::process::id()));
                std::env::set_var("OSCAIRN_DATA_HOME", root.join("data"));
                std::env::set_var("OSCAIRN_CONFIG_HOME", root.join("config"));
            }
        });
        let _ = slint::platform::set_platform(Box::new(Headless)); // once per test thread
        let ui = Shell::new().unwrap();
        theme::attach(&ui);
        let window = WINDOW.with(|w| w.borrow_mut().take()).unwrap();
        window.set_size(PhysicalSize::new(w as u32, h as u32));
        ui.show().unwrap();
        Screen { ui, window, pixels: vec![Rgb8Pixel::default(); w * h], w, h }
    }

    /// Renders what changed since the last frame: the bounding box of what was repainted, None for nothing.
    pub fn frame(&mut self) -> Option<Rect> {
        slint::platform::update_timers_and_animations();
        let mut repainted = None;
        self.window.draw_if_needed(|renderer| {
            let region = renderer.render(&mut self.pixels, self.w);
            let (o, s) = (region.bounding_box_origin(), region.bounding_box_size());
            repainted = (s.width > 0 && s.height > 0).then_some((o.x, o.y, s.width as i32, s.height as i32));
        });
        repainted
    }

    /// The window as a PPM picture in target/screens/NAME.ppm.
    pub fn save(&self, name: &str) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screens");
        std::fs::create_dir_all(&dir).unwrap();
        let mut ppm = format!("P6 {} {} 255\n", self.w, self.h).into_bytes();
        ppm.extend(self.pixels.iter().flat_map(|p| [p.r, p.g, p.b]));
        std::fs::write(dir.join(format!("{name}.ppm")), ppm).unwrap();
    }

    /// The resource section's rectangle in the window (the top bar is at the window's top left).
    pub fn resources(&self) -> Rect {
        let ui = &self.ui;
        (ui.get_resources_x() as i32, 0, ui.get_resources_width().ceil() as i32, 48)
    }
}

thread_local! { static FIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// For a test that draws pictures: the lists keep the rows and panel lines the window really holds,
/// as the shell shows them, instead of the whole list (what `tall` gives the logic tests).
pub fn fit_to_window() {
    FIT.with(|f| f.set(true));
}

/// A list screen's logic tests see every row at once; a picture test (`fit_to_window`) does not.
pub fn tall(ui: &Shell) {
    if !FIT.with(|f| f.get()) {
        ui.global::<crate::PartState>().set_rows_per_view(30);
        ui.global::<crate::PartState>().set_info_per_view(30);
    }
}

/// The picture again at the FANTOM-07's scale (1.55, which the shell sets from the profile): the same
/// window of 1280x720 pixels holds fewer, larger rows. Saved as NAME@1.55.png; the scale goes back.
/// `refresh` shows the screen again, as the shell does when the window's size changes.
pub fn at_fantom_scale(s: &mut Screen, name: &str, refresh: impl FnOnce()) {
    s.set_scale(1.55);
    let mut drawn = s.frame().is_some(); // the screen's own count of rows that fit follows the scale
    refresh();
    drawn |= s.frame().is_some();
    assert!(drawn, "{name}: drawn at 1.55");
    png(s, &format!("{name}@1.55"));
    s.set_scale(1.0);
    s.frame();
}

impl Screen {
    pub fn set_scale(&mut self, scale: f32) {
        self.ui.window().dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: scale });
        self.window.set_size(PhysicalSize::new(self.w as u32, self.h as u32));
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

// The same with a theme of extreme colors, the largest font scale and the roomiest spacing.
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
            let right = 10. * s.ui.global::<crate::Theme>().get_spacing(); // the bar's right padding
            assert!((seen[0].0 + seen[0].1 + right - W as f32).abs() < 0.01, "{cpu:?}/{ram:?}: at the right edge");
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
        let mut request = self.engine.request();
        // What the shell tells the engine of what the screen shows (lists, the knob row's part and page) is not what a test waits for.
        while request["method"] == "controllers.view" && method != "controllers.view" {
            self.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": {"accepted": true}})));
            self.pump();
            request = self.engine.request();
        }
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
    f.answer("record.takes", json!({"takes": []}));
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

// A take recording is said in the top bar (every screen, once: Play adds no mark of its own), with the time it has run,
// as the JUCE record buttons say it: learned by reading record.takes with the status notifications
// at most every 250 ms (the JUCE poll's period), and gone when the take ends or the engine is lost.
#[test]
fn a_take_recording_is_shown_in_the_top_bar_once() {
    let mut f = Fixture::new("record");
    f.engine.answer(); // session.hello
    f.pump();
    f.serve(10, &json!({}));
    let status = |f: &mut Fixture| {
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "status", "params": {"cpuLoad": 0.1}})));
        f.pump();
    };
    let state = |f: &Fixture| {
        let bar = f.screen.ui.global::<Bar>();
        (bar.get_recording(), bar.get_recording_time().to_string(), bar.get_recording_behind())
    };
    assert_eq!(state(&f), (false, String::new(), false), "nothing recording until the engine says so");
    status(&mut f);
    f.answer("record.takes", json!({"takes": [{"take": 1, "state": "done", "frames": 96000, "sampleRate": 48000}, {"take": 2, "state": "recording", "frames": 48000 * 67, "sampleRate": 48000, "behind": false}]}));
    f.answer("memory.state", json!({"systemAvailable": 496u64 << 20, "device": 0}));
    f.answer("transport.get", json!({"tempo": 120, "playing": false}));
    assert_eq!(state(&f), (true, "1:07".into(), false));
    f.screen.frame();
    f.screen.save("topbar-recording");
    png(&f.screen, "topbar-recording");
    // Within 250 ms of the last read nothing is asked; after it the take's time moves.
    status(&mut f);
    std::thread::sleep(std::time::Duration::from_millis(260));
    status(&mut f);
    f.answer("record.takes", json!({"takes": [{"take": 2, "state": "recording", "frames": 48000 * 68, "sampleRate": 48000, "behind": true}]}));
    assert_eq!(state(&f), (true, "1:08".into(), true));
    // On Play the top bar says it and the footer does not: the bottom of Play looks the same without a take.
    f.screen.ui.global::<HomeState>().invoke_tapped(0);
    assert_eq!(f.screen.ui.get_screen(), 3);
    f.screen.frame();
    png(&f.screen, "play-recording");
    shot(&f.screen, "play-recording");
    let footer = |s: &Screen| s.pixels[(H - 60) * W..].to_vec();
    let with_take = footer(&f.screen);
    // The take ends: the mark goes.
    std::thread::sleep(std::time::Duration::from_millis(260));
    status(&mut f);
    loop {
        let r = f.engine.request();
        let take = r["method"] == "record.takes";
        let result = if take { json!({"takes": [{"take": 2, "state": "done", "frames": 48000 * 70, "sampleRate": 48000}]}) } else { json!({}) };
        f.reply(&r, json!({"result": result}));
        if take {
            break;
        }
    }
    assert_eq!(state(&f), (false, String::new(), false));
    f.screen.frame();
    assert!(footer(&f.screen) == with_take, "Play's footer carries no recording mark");
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

/// A theme read from a document, as a file would be: refused (a panic here) if the reference would refuse it.
fn theme_of(doc: serde_json::Value) -> theme::Loaded {
    let data = theme::ThemeData::parse(&doc.to_string()).expect("a theme the reference accepts");
    theme::Loaded { id: data.id.clone(), data, folder: None, regular: None, bold: None, refused: None }
}

/// A deliberately odd theme that is still a valid one: clashing colors, pill buttons, the largest
/// radius and nearly the largest type, outlined edges, soft shadows, cap sliders, folder tabs,
/// round marks, a filled row color, and the roomiest spacing (`joined_theme` is the other extreme:
/// sections with no gap, the tightest spacing).
pub fn odd_theme() -> theme::Loaded {
    theme_of(json!({"version": 7, "id": "odd", "name": "Odd", "fontScale": 1.39, "spacing": 1.6, "radius": 16,
        "font": {"regular": "fonts/Inter-Regular.ttf", "license": "fonts/OFL.txt"},
        "colors": {"background": "#000000", "surface": "#7A0070", "raised": "#006B6B", "border": "#FF8800", "text": "#FFFFFF", "muted": "#00FFFF", "accent": "#FFFF00",
                   "positive": "#FF00FF", "danger": "#00FF00", "warning": "#FF0000", "focus": "#0000FF"},
        "style": {"buttons": "pill", "edges": "outlined", "depth": "soft", "sliders": "cap", "tabs": "folder", "marks": "round", "sections": "separate", "rows": "fill"},
        "wires": {"audio": "#FF0000", "notes": "#00FF00", "disabled": "#FFFFFF", "draining": "#0000FF", "unconnected": "#FFFF00", "control": "#FF00FF"}}))
}

/// The other extreme: square buttons, no radius, no edge, flat, filled sliders, segmented tabs,
/// sections joined edge to edge, the row color as a dot, a light theme with the smallest accepted type.
pub fn joined_theme() -> theme::Loaded {
    theme_of(json!({"version": 7, "id": "odd-joined", "name": "Odd joined", "fontScale": 0.9, "spacing": 0.6, "radius": 0,
        "font": {"regular": "fonts/Inter-Regular.ttf", "license": "fonts/OFL.txt"},
        "colors": {"background": "#F0E8D0", "surface": "#FFF8E0", "raised": "#E0D0A0", "border": "#806000", "text": "#201000", "muted": "#504020", "accent": "#0040C0",
                   "positive": "#006000", "danger": "#C00000"},
        "style": {"buttons": "rounded", "edges": "none", "depth": "flat", "sliders": "filled", "tabs": "segmented", "marks": "square", "sections": "joined", "rows": "dot"},
        "wires": {"audio": "#0000FF", "notes": "#00FF00", "disabled": "#808080", "draining": "#FF8800", "unconnected": "#FF0000", "control": "#00FFFF"}}))
}

/// The built-in themes and the two odd ones: what every screen is drawn in.
pub fn all_themes() -> Vec<theme::Loaded> {
    let mut loaded: Vec<theme::Loaded> = crate::profile::THEMES.iter().map(|(id, _)| theme::chosen(&theme::Places::default(), id, "")).collect();
    loaded.push(odd_theme());
    loaded.push(joined_theme());
    loaded
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
    for t in &all_themes() {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: repainted", t.id);
        assert_eq!(s.frame(), None, "{}: once", t.id);
        s.save(&format!("theme-{}", t.id));
        shot(s, &format!("home-{}", t.id));
    }
    let g = s.ui.global::<crate::Theme>();
    assert_eq!((g.get_font_scale(), g.get_spacing(), g.get_radius()), (0.9, 0.6, 0.), "the last theme applied");
    assert_eq!(g.get_focus(), slint::Color::from_rgb_u8(0x00, 0x40, 0xc0), "focus: the accent when the theme has none");
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
    // Everything asked on connecting answered: the shell's eight, the Setlist's two, then the setlist (and the part the knob row follows).
    f.serve(12, &content); // and the knob row's view
    let rows = |f: &Fixture| f.screen.ui.global::<SetlistState>().get_slots().iter().collect::<Vec<_>>();
    let per_view = f.screen.ui.global::<SetlistState>().get_slots_per_view() as usize;
    assert!(per_view >= 5, "a list view holds {per_view}");
    assert_eq!((rows(&f).len(), rows(&f)[0].title.as_str(), f.screen.ui.global::<SetlistState>().get_name().as_str()), (per_view, "Song 0", "Friday"));
    f.screen.frame();
    f.screen.save("setlist");
    assert_eq!(f.screen.frame(), None, "at rest");
    for t in &all_themes() {
        theme::apply(&f.screen.ui, t);
        assert!(f.screen.frame().is_some(), "{}: drawn", t.id);
        shot(&f.screen, &format!("setlist-{}", t.id));
    }
    theme::apply(&f.screen.ui, &theme::chosen(&theme::Places::default(), "", ""));
    f.screen.frame();
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

fn play_graph() -> serde_json::Value {
    json!({
        "nodes": [
            {"id": 1, "name": "Rhodes", "audioInputs": [], "audioOutputs": [{"channels": 2}], "noteInputs": 1, "noteOutputs": 0},
            {"id": 3, "name": "Hall", "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}], "noteInputs": 0, "noteOutputs": 0},
            {"id": 4, "name": "Bass", "audioInputs": [], "audioOutputs": [{"channels": 1}], "noteInputs": 1, "noteOutputs": 0},
        ],
        "parts": [
            {"id": 10, "name": "Keys", "level": 0.5, "mute": false, "solo": false, "chain": [1],
             "input": {"kind": "notes", "devices": [], "zones": [{"keys": [36, 96], "transpose": 0}]}},
            {"id": 11, "name": "Bass", "level": 1.0, "mute": false, "solo": false, "chain": [4],
             "input": {"kind": "notes", "devices": [], "zones": [{"keys": [0, 59], "transpose": 0}]}},
            {"id": 12, "name": "Reverb", "level": 0.8, "mute": false, "solo": false, "chain": [3], "input": {"kind": "bus"}},
        ],
        "wires": [],
        "scenes": [{"id": 1, "name": "Verse"}, {"id": 2, "name": "Chorus"}], "scene": 2,
        "master": {"level": 1.0, "pan": 0.0},
    })
}

/// A viewable copy of the picture in target/screens/NAME.png: a PNG made by hand (stored zlib
/// blocks, no image crate).
fn png(s: &Screen, name: &str) {
    png_in("target/screens", s, name);
}

/// The same for a screen in a theme: target/theme-shots/NAME.png, which the theme work looks at.
fn shot(s: &Screen, name: &str) {
    png_in("target/theme-shots", s, name);
}

fn png_in(folder: &str, s: &Screen, name: &str) {
    png_of(folder, &s.pixels, s.w, s.h, name);
}

/// A part of the picture, each of its pixels `zoom` across, in target/theme-shots/NAME.png: for looking closely.
fn png_crop(s: &Screen, name: &str, r: Rect, zoom: usize) {
    let (x0, y0, w, h) = (r.0 as usize, r.1 as usize, r.2 as usize, r.3 as usize);
    let mut out = Vec::with_capacity(w * h * zoom * zoom);
    for y in 0..h * zoom {
        for x in 0..w * zoom {
            out.push(s.pixels[(y0 + y / zoom) * s.w + x0 + x / zoom]);
        }
    }
    png_of("target/theme-shots", &out, w * zoom, h * zoom, name);
}

fn png_of(folder: &str, pixels: &[Rgb8Pixel], w: usize, h: usize, name: &str) {
    use std::io::Write;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0u8);
        raw.extend(pixels[y * w..(y + 1) * w].iter().flat_map(|p| [p.r, p.g, p.b]));
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend((c.len() as u16).to_le_bytes());
        z.extend((!(c.len() as u16)).to_le_bytes());
        z.extend(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let crc = |data: &[u8]| {
        let mut crc = !0u32;
        for &x in data {
            crc ^= x as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    };
    let mut head = Vec::new();
    head.extend((w as u32).to_be_bytes());
    head.extend((h as u32).to_be_bytes());
    head.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &head);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::File::create(dir.join(format!("{name}.png"))).unwrap().write_all(&out).unwrap();
}

/// The next request that is not a meter poll (the timer may fire between a test's steps), polls answered.
fn next_edit(f: &mut Fixture) -> serde_json::Value {
    loop {
        let r = f.engine.request();
        match r["method"].as_str() {
            Some("parts.meters") => f.reply(&r, json!({"result": {"meters": []}})),
            Some("edit.history") => f.reply(&r, json!({"result": {"undo": "", "redo": ""}})),
            Some("controllers.view") => f.reply(&r, json!({"result": {"accepted": true}})), // what the knob row tells the engine
            // The JUCE strip commits after a level, solo or master edit; counted, answered.
            Some("graph.commit") => {
                COMMITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                f.reply(&r, json!({"result": {"revision": 1}}));
            }
            _ => return r,
        }
    }
}

static COMMITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

// Play on the shell: the Performance's scenes, parts and master from graph.get; the left knob
// (pane 0) through the scenes and the right (pane 1) through the parts, a press selecting a scene
// or a part (Part, below); touch on the scene tiles, M, S and the faders; levels moved live by
// controls.changed; the meters asked only while Play is shown; Cancel drops the cursors, then goes
// back; Edit opens the Parts list.
#[test]
fn play_by_panes_touch_and_meters() {
    let mut f = Fixture::new("play");
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(0); // Performance: Play, no handover
    assert_eq!(f.screen.ui.get_screen(), 3);
    // The shell's ten on connecting, then Play's controls.changed subscription.
    let mut methods = vec![];
    for _ in 0..12 {
        let request = f.engine.request();
        let method = request["method"].as_str().unwrap().to_string();
        let result = match method.as_str() {
            "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
            "graph.get" => play_graph(),
            "library.query" => json!({"items": [], "total": 0}),
            _ => json!({}),
        };
        methods.push(method);
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": request["id"], "result": result})));
        f.pump();
    }
    assert_eq!(methods.iter().filter(|m| *m == "graph.get").count(), 1, "graph.get asked once, not again by Play: {methods:?}");
    assert_eq!(methods.iter().filter(|m| *m == "subscribe").count(), 2, "{methods:?}");
    let ui = f.screen.ui.clone_strong();
    let play = ui.global::<crate::PlayState>();
    let strips = |f: &Fixture| f.screen.ui.global::<crate::PlayState>().get_strips().iter().collect::<Vec<_>>();
    let scenes = |f: &Fixture| f.screen.ui.global::<crate::PlayState>().get_scenes().iter().collect::<Vec<_>>();
    assert_eq!(f.screen.ui.global::<Bar>().get_where().as_str(), "Play");
    let names: Vec<_> = strips(&f).iter().map(|s| (s.name.to_string(), s.detail.to_string())).collect();
    assert_eq!(names, [("Keys".into(), "Rhodes · Any device · C2–C7".into()), ("Bass".into(), "Bass · Any device · C-1–B3".into()), ("Reverb".into(), "Bus · Hall".into())]);
    assert!(play.get_has_master() && (play.get_master().fraction - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3, "the master's level 1 of 0..2");
    assert!((strips(&f)[0].fraction - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    let scene_state: Vec<_> = scenes(&f).iter().map(|s| (s.filled, s.on)).collect();
    assert_eq!(scene_state, [(true, false), (true, true), (false, false), (false, false), (false, false), (false, false), (false, false), (false, false)]);
    assert!(!play.get_can_step(), "not the setlist's Performance");
    f.screen.frame();
    f.screen.save("play");
    png(&f.screen, "play");
    assert_eq!(f.screen.frame(), None, "at rest");
    // The panel: a baseline, then E1 (pane 0) once: the cursor on the first scene; press: it plays.
    let mut total = std::collections::BTreeMap::<&str, i64>::new();
    let mut nav = |f: &mut Fixture, key: &'static str, by: i64| {
        *total.entry(key).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move:0", 0);
    nav(&mut f, "move:0", 1);
    assert_eq!(scenes(&f).iter().position(|s| s.cursor), Some(0));
    let repainted = f.screen.frame().unwrap();
    assert!(repainted.1 >= 48 && repainted.1 + repainted.3 <= 48 + 6 + 44 + 2, "a scene cursor repaints the scene row only: {repainted:?}");
    nav(&mut f, "confirm:0", 1);
    let select = next_edit(&mut f);
    assert_eq!((select["method"].as_str(), select["params"]["scene"].as_u64()), (Some("scenes.select"), Some(1)));
    assert_eq!(scenes(&f).iter().position(|s| s.on), Some(0), "shown at once");
    f.reply(&select, json!({"result": {"accepted": true}}));
    // Accepted, the graph is read again: scenes.select commits nothing, so the real engine sends no
    // graph.changed, and the top bar's scene would stay the one before.
    let mut selected = play_graph();
    selected["scene"] = json!(1);
    for (method, result) in [("performance.info", json!({"id": "p1", "name": "Grand", "unsaved": false})), ("graph.get", selected)] {
        let r = next_edit(&mut f);
        assert_eq!(r["method"].as_str(), Some(method));
        f.reply(&r, json!({ "result": result }));
    }
    assert_eq!(f.screen.ui.global::<Bar>().get_scene().as_str(), "Verse");
    // E6 (pane 1) twice: the cursor on the first part, then the second; M on it mutes.
    nav(&mut f, "move:1", 1);
    nav(&mut f, "move:1", 1);
    assert_eq!(strips(&f).iter().position(|s| s.cursor), Some(1));
    play.invoke_mute_tapped(1); // a press here selects the part (Part screen, below); M mutes
    let mute = next_edit(&mut f);
    assert_eq!((mute["method"].as_str(), mute["params"].clone()), (Some("parts.level"), json!({"part": 11, "level": 1.0, "mute": true})));
    f.reply(&mute, json!({"result": {"accepted": true}}));
    assert!(strips(&f)[1].mute && strips(&f)[1].dim);
    // Touch: M on the first part, S on the second, a fader to its middle (a quarter of the level).
    play.invoke_mute_tapped(0);
    let r = next_edit(&mut f);
    assert_eq!((r["method"].as_str(), r["params"]["part"].as_u64(), r["params"]["mute"].as_bool()), (Some("parts.level"), Some(10), Some(true)));
    f.reply(&r, json!({"result": {}}));
    play.invoke_solo_tapped(1);
    let r = next_edit(&mut f);
    assert_eq!((r["method"].as_str(), r["params"].clone()), (Some("parts.solo"), json!({"part": 11, "solo": true})));
    f.reply(&r, json!({"result": {}}));
    play.invoke_level(2, 0.5);
    let r = next_edit(&mut f);
    assert_eq!((r["method"].as_str(), r["params"]["part"].as_u64(), r["params"]["level"].as_f64()), (Some("parts.level"), Some(12), Some(0.25)));
    f.reply(&r, json!({"result": {}}));
    play.invoke_level(-1, 0.5);
    let r = next_edit(&mut f);
    assert_eq!((r["method"].as_str(), r["params"]["level"].as_f64()), (Some("master.set"), Some(0.5)));
    f.reply(&r, json!({"result": {}}));
    // A level moved live; a finger on a fader keeps the engine's value from it. (The fake engine
    // does not push controls.changed by itself: the notification is sent by hand.)
    let live = |f: &mut Fixture, level: f64| {
        f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controls.changed", "params": {"parts": [{"part": 10, "level": level, "mute": false}], "master": {"level": 0.5, "pan": 0.0}}})));
        f.pump();
    };
    live(&mut f, 0.81);
    assert!((strips(&f)[0].fraction - 0.9).abs() < 1e-6, "the level moved live");
    play.invoke_held(0, true);
    live(&mut f, 0.25);
    assert!((strips(&f)[0].fraction - 0.9).abs() < 1e-6, "held: the engine's value waits");
    play.invoke_held(0, false);
    live(&mut f, 0.25);
    assert!((strips(&f)[0].fraction - 0.5).abs() < 1e-6);
    // The meters: asked on a timer while Play is shown, one at a time; only the strip's own area repaints.
    f.screen.frame();
    std::thread::sleep(std::time::Duration::from_millis(150));
    f.screen.frame();
    let mut history = 0;
    let r = loop {
        let r = f.engine.request();
        if r["method"] == "edit.history" {
            // A level moved live is an undoable edit with no graph.changed: what undo names is asked.
            history += 1;
            f.reply(&r, json!({"result": {"undo": "Keys level", "redo": ""}}));
            continue;
        }
        if r["method"] != "graph.commit" {
            break r;
        }
        COMMITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        f.reply(&r, json!({"result": {"revision": 1}}));
    };
    assert_eq!(r["method"], "parts.meters");
    assert!(history >= 1, "controls.changed with a part's level asks edit.history");
    f.screen.frame(); // the bar's Undo, named now
    // The scene select sent no commit (the engine does it); each mute, the solo, the level and the master did.
    assert_eq!(COMMITS.load(std::sync::atomic::Ordering::SeqCst), 5);
    f.reply(&r, json!({"result": {"meters": [{"part": 10, "peak": 0.5, "channels": [0.5, 1.0], "stolen": 0, "hardChokes": 0}]}}));
    assert_eq!((strips(&f)[0].channels, strips(&f)[0].peak_a, strips(&f)[0].peak_b), (2, 0.82, 0.91));
    let repainted = f.screen.frame().unwrap();
    assert!(repainted.2 < 200, "a meter repaints its strip, not the screen: {repainted:?}");
    f.screen.save("play-meters");
    png(&f.screen, "play-meters");
    // Cancel: the cursors go; Cancel again: back to Home, the subscription ended, no more meters.
    nav(&mut f, "cancel", 1);
    assert_eq!((strips(&f).iter().any(|s| s.cursor), f.screen.ui.get_screen()), (false, 3));
    nav(&mut f, "cancel", 1);
    assert_eq!(f.screen.ui.get_screen(), 0);
    let r = next_edit(&mut f);
    assert_eq!((r["method"].as_str(), r["params"]["topics"][0].as_str()), (Some("unsubscribe"), Some("controls.changed")));
    f.reply(&r, json!({"result": {}}));
    // Open Play again, and Edit (the top bar's last action): the Parts list, in this UI, no handover.
    f.screen.ui.global::<HomeState>().invoke_tapped(0);
    assert_eq!(f.screen.ui.get_screen(), 3);
    f.screen.ui.global::<Bar>().invoke_action(5);
    assert_eq!((f.screen.ui.get_screen(), shell::handover()), (9, None));
}

// Previous and Next on Play, the page keys, only while the Performance is the setlist's.
#[test]
fn play_steps_through_the_setlist_only_when_it_follows_it() {
    let mut s = Screen::new();
    let mut play = crate::play::Screen::new(&s.ui);
    assert_eq!(play.act(Action::Page(1)), crate::play::Outcome::Stay);
    play.follow(true);
    assert!(s.ui.global::<crate::PlayState>().get_can_step());
    assert_eq!(play.act(Action::Page(-1)), crate::play::Outcome::Step(-1));
    play.follow(false);
    assert!(!s.ui.global::<crate::PlayState>().get_can_step());
    assert_eq!(play.act(Action::Cancel(None)), crate::play::Outcome::Back);
    s.frame();
}

// Play draws in the built-in themes and an odd one (large type, extreme colors).
#[test]
fn play_draws_in_every_theme() {
    let mut s = Screen::new();
    let mut play = crate::play::Screen::new(&s.ui);
    play.graph(&play_graph());
    s.ui.set_screen(3);
    play.meters(&json!({"meters": [{"part": 10, "peak": 0.5, "channels": [0.5, 1.0]}, {"part": 11, "peak": 2.0, "channels": [2.0]}]}));
    // A take recording, slow, so every theme draws the top bar's mark and Play's.
    let bar = s.ui.global::<Bar>();
    bar.set_recording(true);
    bar.set_recording_time("12:07".into());
    bar.set_recording_behind(true);
    bar.set_performance("Grand Piano".into());
    bar.set_scene("Chorus".into());
    let loaded = all_themes();
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        play.shown(); // the strips a view holds depend on the paddings
        s.frame();
        s.save(&format!("play-{}", t.id));
        png(&s, &format!("play-{}", t.id));
        shot(&s, &format!("play-{}", t.id));
    }
    at_fantom_scale(&mut s, "play", || play.graph(&play_graph()));
}

#[cfg(test)]
mod bar;
mod play;
mod row;
mod scene;
mod part;
mod keyboard;
mod pages;
mod chain;
mod chain_splits;
mod chain_presets;
mod sounds_filters;
mod piano;
mod shellpages;
mod envelope;
mod fm;
mod fm_view;
mod palette;
mod fm_touch;
mod fm_dots;
mod fm_lines;
mod grid;
mod tabs;
mod tabs_model;
mod search;
mod shots;
mod views;
mod parts;
mod mixer;
mod wires;
mod routing;
mod controls;
mod layout;

// `apply` puts every property of a theme in the window's `Theme` global: the nine colors and the
// optional two, the metrics, the eight style choices, the wires, the splash's colors, and what is
// derived (a dark theme, the text on the accent, the piano keys' inks); a second window is its own.
#[test]
fn applying_a_theme_sets_every_property() {
    let s = Screen::new();
    let other = Screen::new();
    for t in &all_themes() {
        theme::apply(&s.ui, t);
        let (g, d) = (s.ui.global::<crate::Theme>(), &t.data);
        let colors = [g.get_background(), g.get_surface(), g.get_raised(), g.get_border(), g.get_text(), g.get_muted(), g.get_accent(), g.get_positive(), g.get_danger()];
        assert_eq!(colors, d.colors, "{}", t.id);
        assert_eq!((g.get_warning(), g.get_focus()), (d.warning.unwrap_or(slint::Color::from_rgb_u8(0xe8, 0xc5, 0x47)), d.focus.unwrap_or(d.colors[6])), "{}", t.id);
        assert_eq!((g.get_radius(), g.get_font_scale(), g.get_spacing()), (d.radius, d.font_scale, d.spacing), "{}", t.id);
        assert_eq!((g.get_gap(), g.get_margin(), g.get_pad(), g.get_pad_wide(), g.get_pad_narrow(), g.get_pad_tight()), (4. * d.spacing, 6. * d.spacing, 10. * d.spacing, 12. * d.spacing, 8. * d.spacing, 4. * d.spacing), "{}: paddings and gaps scale", t.id);
        let st = d.style;
        assert_eq!((g.get_buttons(), g.get_edges(), g.get_depth(), g.get_sliders()), (st.buttons, st.edges, st.depth, st.sliders), "{}", t.id);
        assert_eq!((g.get_tabs(), g.get_marks(), g.get_sections(), g.get_rows()), (st.tabs, st.marks, st.sections, st.rows), "{}", t.id);
        let w = d.wires;
        assert_eq!([g.get_wire_audio(), g.get_wire_notes(), g.get_wire_disabled(), g.get_wire_draining(), g.get_wire_unconnected(), g.get_wire_control()], [w.audio, w.notes, w.disabled, w.draining, w.unconnected, w.control], "{}", t.id);
        assert_eq!((g.get_splash_background(), g.get_splash_ink()), (d.splash.background.unwrap_or(d.colors[0]), d.splash.ink.unwrap_or(d.colors[4])), "{}", t.id);
        assert_eq!(g.get_dark(), t.id != "oscairn-light" && t.id != "odd-joined", "{}: a dark background", t.id);
        let light_accent = t.id == "oscairn-dark" || t.id == "oscairn-classic" || t.id == "oscairn-block" || t.id == "odd";
        assert_eq!(g.get_on_accent(), if light_accent { slint::Color::from_rgb_u8(0, 0, 0) } else { slint::Color::from_rgb_u8(255, 255, 255) }, "{}: black on a bright accent", t.id);
        assert_eq!(theme::section_gap(&s.ui, 6.), if t.data.style.sections == crate::SectionStyle::Joined { 0. } else { 6. * d.spacing }, "{}", t.id);
    }
    // Held per window: the other one still has what it had.
    assert_eq!(other.ui.global::<crate::Theme>().get_radius(), 6., "a theme applied to one window is not another's");
}
