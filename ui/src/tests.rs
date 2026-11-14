// SPDX-License-Identifier: AGPL-3.0-or-later
//! Scene tests: the shell's window rendered headless by Slint's software renderer into a buffer
//! (no window is shown), the area each change repaints taken from the renderer, and the shell
//! driven through a socket by a fake engine. Pictures for a look by eye go to target/screens/.

use crate::client::tests::{FakeEngine, WAIT};
use crate::client::Event;
use crate::shell::{self, Request};
use crate::theme;
use crate::topbar::{MeterSettings, Show};
use crate::nav::{Action, Roles};
use crate::{Bar, HomeState, Shell};
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
        shell::start(&screen.ui, path, &crate::profile::Profile::load(Some("fantom-07")).unwrap(), move |e| {
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
    assert_eq!(f.engine.request()["params"]["topics"], json!(["status", "graph.changed"]));
    f.answer("performance.reopen", json!({"notice": ""}));
    f.answer("performance.info", json!({"name": "Grand Piano", "unsaved": false}));
    f.answer("graph.get", json!({"scene": 1, "scenes": [{"id": 1, "name": "Verse"}]}));
    f.answer("library.query", json!({"items": [], "total": 24}));
    f.answer("library.query", json!({"items": [], "total": 1}));
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
    f.pump(); // the subscribe still out, failed
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
    let key = |s: &Screen, k: slint::platform::Key| s.ui.invoke_key(slint::SharedString::from(k), false);
    assert!(key(s, slint::platform::Key::RightArrow));
    let repainted = s.frame().unwrap();
    let (a, b) = (tile(s, 0), tile(s, 1));
    assert!(within(repainted, (a.0, a.1, b.0 + b.2 - a.0, a.3)), "a move repainted {repainted:?}: tiles 0 and 1 are {a:?} {b:?}");
    let cursor = |s: &Screen| s.ui.global::<HomeState>().get_tiles().iter().position(|t| t.cursor);
    assert_eq!((cursor(s), s.ui.get_screen()), (Some(1), 0), "moved, nothing opened");
    // The panel: the data dial two steps on, as a FANTOM-07's map would have it.
    let roles = Roles::parse(&json!({"navigation": {"cursor": ["data"], "confirm": ["enter"]}}));
    shell::act(roles.action("data", 2).unwrap());
    assert_eq!(cursor(s), Some(3));
    shell::act(Action::Move { pane: 0, by: -1 });
    shell::act(roles.action("enter", 1).unwrap());
    assert_eq!((s.ui.get_screen(), s.ui.global::<Bar>().get_where().as_str(), s.ui.global::<Bar>().get_home_button()), (1, "Setlist", true));
    assert!(key(s, slint::platform::Key::Escape), "Cancel goes Home");
    assert_eq!(s.ui.get_screen(), 0);
    s.ui.global::<HomeState>().invoke_tapped(4); // Settings: a JUCE screen
    assert_eq!((s.ui.get_screen(), s.ui.get_opening().as_str(), shell::handover()), (2, "Settings", Some("settings")));
    s.frame();
    s.save("home-opening-settings");
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
