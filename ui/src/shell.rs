// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shell: one connection to the engine, the top bar it feeds, and the screens. Runs on the
//! UI thread; the client's events reach it through the event loop.
//!
//! What it asks the engine and when: on connecting, `subscribe` to `status` and
//! `graph.changed`, `memory.shell` (its own memory; the rest of the session's setup is the
//! engine's, docs/rust-ui.md "Session setup"), `performance.reopen` (the Performance last open, as the JUCE app asks at its
//! start), `performance.info` and `graph.get`. Each `status` (at most 4 a second) moves the
//! meters at most twice a second and then asks `memory.state` and `transport.get`; each
//! `graph.changed` asks `performance.info` and `graph.get` again. Nothing else wakes it at rest.
//! While Play is shown it also subscribes to `controls.changed` and asks `parts.meters` every
//! 100 ms (one at a time); leaving Play ends both.

use crate::client::{Client, Error, Event, BUSY};
use crate::home::{self, Home};
use crate::nav::{self, Action};
use crate::part;
use crate::play;
use crate::profile::Profile;
use crate::setlist;
use crate::topbar::{self, Level, Meter, MeterSettings};
use crate::{Bar, HomeState, PartState, PlayState, SetlistState, Shell, TileData};
use serde_json::{json, Value};
use slint::{ComponentHandle, Model, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What each request is for, so its answer goes where it belongs.
pub enum Request {
    Subscribe,
    /// memory.shell: this UI's own memory, which the engine reserves for it.
    Shell,
    Reopen,
    Info,
    Graph,
    Transport,
    Memory,
    /// library.query's total of Performances (false) or Sounds (true), for Home's Library line.
    Library(bool),
    /// controllers.view: whether lists are on screen, so the panel's navigate controls navigate.
    View,
    Setlist(setlist::Request),
    Play(play::Request),
    Part(part::Request),
}

impl Request {
    /// The engine taking long over it is shown: the musician waits on it.
    fn waited_on(&self) -> bool {
        match self {
            Request::Reopen => true,
            Request::Setlist(r) => setlist::Screen::waits(r),
            _ => false,
        }
    }
}

const METER_INTERVAL: Duration = Duration::from_millis(500);

/// What the top bar shows now, so only a change reaches the window.
#[derive(Clone, Default, PartialEq)]
struct Shown {
    performance: String,
    scene: String,
    tempo: String,
    cpu: Meter,
    ram: Meter,
    audio_device: String,
    performances: Option<u64>,
    sounds: Option<u64>,
}

/// The screen shown under the top bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Home = 0,
    Setlist = 1,
    /// A JUCE screen asked for: the display is being handed over (docs/rust-ui.md).
    Opening = 2,
    /// The Performance's scenes, parts and meters (play.rs).
    Play = 3,
    /// One part's settings (part.rs), opened from Play.
    Part = 4,
}

pub struct App {
    ui: slint::Weak<Shell>,
    client: Arc<Client<Request>>,
    device_memory: u64,
    shown: Shown,
    last_meters: Option<Instant>,
    /// performance.info and graph.get refused while the engine was busy: asked again at the next meters.
    stale: bool,
    watchdog: slint::Timer,
    screen: Screen,
    home: Home,
    tiles: Rc<VecModel<TileData>>,
    /// The JUCE app's name for the screen asked for (its --app), once one is.
    handover: Option<&'static str>,
    /// Whether this shell can hand the display over (it was started with a handover file); a
    /// computer's cannot, and says where that screen is instead.
    hands_over: bool,
    leave: slint::Timer,
    /// The panel's navigation totals as last read.
    totals: nav::Totals,
    setlist: setlist::Screen,
    play: play::Screen,
    part: part::Screen,
    /// Where Play was opened from, which Cancel goes back to.
    play_from: Screen,
    /// Asks for the meters while Play is shown, and only then.
    meters: slint::Timer,
    /// The open Performance's library ID (performance.info), which the setlist's slot playing is compared with.
    performance_id: String,
}

thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            f(app)
        }
    })
}

/// The screens that follow levels and mutes moved live (controls.changed).
fn live(screen: Screen) -> bool {
    matches!(screen, Screen::Play | Screen::Part)
}

fn level(m: Meter) -> i32 {
    match m.level() {
        Level::Normal => 0,
        Level::High => 1,
        Level::Full => 2,
    }
}

/// The meters as the settings say, before anything is known.
pub fn show_settings(ui: &Shell, settings: MeterSettings) {
    let bar = ui.global::<Bar>();
    bar.set_cpu_bar(settings.cpu.bar());
    bar.set_cpu_percent(settings.cpu.percent());
    bar.set_ram_bar(settings.ram.bar());
    bar.set_ram_percent(settings.ram.percent());
}

/// Starts the shell on `ui`, a client of the engine's socket at `socket`. `post` takes each of the
/// client's events, on its reader thread, to the UI thread, which hands it to `dispatch`.
pub fn start(ui: &Shell, socket: PathBuf, profile: &Profile, hands_over: bool, post: impl Fn(Event<Request>) + Send + 'static) -> std::io::Result<()> {
    let client = Client::start(socket, post)?;
    ui.set_engine("connecting".into());
    let home = Home::new(&profile.apps, ui.get_home_columns().max(1) as usize);
    let tiles = Rc::new(VecModel::<TileData>::default());
    ui.global::<HomeState>().set_tiles(tiles.clone().into());
    ui.global::<HomeState>().on_tapped(|i| with_app(|a| a.tap(i)));
    ui.global::<Bar>().on_home(|| with_app(|a| a.go(Screen::Home)));
    ui.on_key(|text| nav::from_key(&text).map(act).is_some());
    let lists = ui.global::<SetlistState>();
    lists.on_tapped_slot(|i| with_app(|a| a.on_setlist(|s| s.tap_slot(i.max(0) as usize))));
    lists.on_tapped_setlist(|i| with_app(|a| a.on_setlist(|s| s.tap_setlist(i.max(0) as usize))));
    lists.on_step(|by| with_app(|a| a.on_setlist(|s| s.step_by(by as i64))));
    lists.on_play(|| with_app(|a| a.go(Screen::Play)));
    let play = ui.global::<PlayState>();
    play.on_scene_tapped(|i| with_app(|a| a.on_play(|p| p.tap_scene(i))));
    play.on_mute_tapped(|i| with_app(|a| a.on_play(|p| p.tap_mute(i))));
    play.on_solo_tapped(|i| with_app(|a| a.on_play(|p| p.tap_solo(i))));
    play.on_level(|i, f| with_app(|a| a.on_play(|p| p.level(i, f))));
    play.on_held(|i, down| with_app(|a| a.on_play(|p| p.hold(i, down))));
    play.on_step(|by| with_app(|a| a.on_setlist(|s| s.step_by(by as i64))));
    play.on_edit(|| with_app(|a| a.hand_over(&home::APPS[0])));
    play.on_part_tapped(|i| {
        with_app(|a| {
            let mut id = None;
            a.on_play(|p| id = p.tap_open(i));
            if let Some(id) = id {
                a.open_part(id);
            }
        })
    });
    let part = ui.global::<PartState>();
    part.on_tapped(|i| with_app(|a| a.part_act(|p| p.tap(i))));
    part.on_step(|i, by| with_app(|a| a.on_part(|p| p.step_row(i, by))));
    part.on_back(|| with_app(|a| a.go(Screen::Play)));
    // The JUCE Setlist app's own state file, so either opens where the other left off.
    let state = crate::settings::data_dir().map(|d| d.join("AppData/oscairn.setlist/state.json"));
    APP.with(|a| {
        let mut app = App {
            ui: ui.as_weak(),
            client,
            device_memory: profile.memory,
            shown: Shown::default(),
            last_meters: None,
            stale: false,
            watchdog: slint::Timer::default(),
            screen: Screen::Home,
            home,
            tiles,
            handover: None,
            hands_over,
            leave: slint::Timer::default(),
            totals: nav::Totals::default(),
            setlist: setlist::Screen::new(ui, state),
            play: play::Screen::new(ui),
            part: part::Screen::new(ui),
            play_from: Screen::Home,
            meters: slint::Timer::default(),
            performance_id: String::new(),
        };
        app.show_home();
        *a.borrow_mut() = Some(app);
    });
    Ok(())
}

/// One of the client's events, on the UI thread.
pub fn dispatch(event: Event<Request>) {
    with_app(|a| a.event(event))
}

/// A navigation action from any source, on the UI thread.
pub fn act(action: Action) {
    with_app(|a| a.act(action))
}

/// The JUCE screen asked for, if one is: the program ends to hand the display over.
pub fn handover() -> Option<&'static str> {
    APP.with(|a| a.borrow().as_ref().and_then(|a| a.handover))
}

/// This process's resident memory, as the JUCE app reads its own for memory.shell; None where it
/// cannot be read.
pub fn resident_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let pages: u64 = std::fs::read_to_string("/proc/self/statm").ok()?.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: sysconf reads a constant of the system.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        (pages > 0 && page > 0).then_some(pages * page as u64)
    }
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        // SAFETY: proc_pidinfo writes at most `size` bytes into `info`, which is that size.
        let n = unsafe { libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, info.as_mut_ptr().cast(), size) };
        // SAFETY: filled in whole when it returns its size; zeroed (a valid proc_taskinfo) otherwise.
        (n == size).then(|| unsafe { info.assume_init() }.pti_resident_size).filter(|&b| b > 0)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// The handover file: the app's name as a whole line, newline and all, as the start script reads it.
pub fn write_handover(file: &std::path::Path, app: &str) -> std::io::Result<()> {
    std::fs::write(file, format!("{app}\n"))
}

impl App {
    fn ui(&self) -> Option<Shell> {
        self.ui.upgrade()
    }

    fn send(&mut self, request: Request, method: &str, params: Value) {
        let waited = request.waited_on();
        if self.client.send(request, method, params) && waited && !self.watchdog.running() {
            // Runs only while something is out, so the shell does not wake at rest.
            self.watchdog.start(slint::TimerMode::Repeated, Duration::from_millis(250), || with_app(|a| a.watch()));
        }
    }

    /// Asks unless the same is already out: an answer slower than the meters never piles up.
    fn ask(&mut self, request: Request, method: &str) {
        let same = std::mem::discriminant(&request);
        if self.client.waiting(|r| std::mem::discriminant(r) == same).is_none() {
            self.send(request, method, json!({}));
        }
    }

    fn watch(&mut self) {
        let Some(ui) = self.ui() else { return };
        match self.client.waiting(Request::waited_on) {
            Some(out) if out >= Duration::from_millis(400) => ui.set_engine(format!("busy {:.1} s", out.as_secs_f32()).into()),
            Some(_) => {}
            None => {
                self.watchdog.stop();
                if self.client.connected() {
                    ui.set_engine(SharedString::new());
                }
            }
        }
    }

    fn event(&mut self, event: Event<Request>) {
        match event {
            Event::Connected => {
                if let Some(ui) = self.ui() {
                    ui.set_engine(SharedString::new());
                }
                self.last_meters = None;
                self.totals.forget();
                self.send(Request::Subscribe, "subscribe", json!({"topics": ["status", "graph.changed", "controllers"]}));
                self.send_view();
                // The session's setup is the engine's, from the profile (docs/rust-ui.md, "Session
                // setup"); only this UI's own memory is the UI's to send.
                if let Some(bytes) = resident_bytes() {
                    self.send(Request::Shell, "memory.shell", json!({ "bytes": bytes }));
                }
                self.send(Request::Reopen, "performance.reopen", json!({}));
                self.ask_performance();
                self.send(Request::Library(false), "library.query", json!({"kind": "performance", "limit": 1}));
                self.send(Request::Library(true), "library.query", json!({"kind": "sound", "limit": 1}));
                self.on_setlist(setlist::Screen::connected);
                if live(self.screen) {
                    self.send(Request::Subscribe, "subscribe", json!({"topics": ["controls.changed"]}));
                }
            }
            Event::Lost(why) => {
                if let Some(ui) = self.ui() {
                    ui.set_engine(format!("lost: {why}").into());
                }
            }
            Event::Notification { method, params } => match method.as_str() {
                "status" => self.status(&params),
                "graph.changed" => self.ask_performance(),
                "controls.changed" => {
                    self.on_play(|p| p.controls(&params));
                    self.on_part(|p| p.controls(&params));
                }
                // Read always, so the totals stay current; acted on only by the screen shown, and
                // by none while the display goes to the JUCE app (act drops them then).
                "controllers" => {
                    for action in self.totals.read(&params["navigation"]) {
                        self.act(action);
                    }
                }
                _ => {}
            },
            Event::Reply { request, result } => self.reply(request, result),
        }
    }

    fn ask_performance(&mut self) {
        self.stale = false;
        self.ask(Request::Info, "performance.info");
        self.ask(Request::Graph, "graph.get");
    }

    /// The meters, at most every METER_INTERVAL: the CPU from this status, RAM and the tempo asked.
    fn status(&mut self, status: &Value) {
        self.on_part(|p| p.endpoints(status)); // the inputs, whenever they change
        let now = Instant::now();
        if self.last_meters.is_some_and(|t| now.duration_since(t) < METER_INTERVAL) {
            return;
        }
        self.last_meters = Some(now);
        let device = status["audioDevice"].as_str().unwrap_or_default().to_string();
        self.show(|s| {
            s.cpu = topbar::cpu(status);
            s.audio_device = device;
        });
        self.ask(Request::Memory, "memory.state");
        self.ask(Request::Transport, "transport.get");
        if self.stale {
            self.ask_performance();
        }
    }

    fn reply(&mut self, request: Request, result: Result<Value, Error>) {
        let request = match request {
            Request::Setlist(r) => return self.on_setlist(|s| s.reply(r, result)),
            Request::Play(r) => return self.on_play(|p| p.reply(r, result)),
            Request::Part(r) => {
                if self.on_part(|p| p.reply(r, result)) {
                    self.ask_performance();
                }
                return;
            }
            other => other,
        };
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                if e.code == BUSY && matches!(request, Request::Info | Request::Graph) {
                    self.stale = true;
                }
                match request {
                    Request::Reopen if e.code != 0 => eprintln!("oscairn-ui: performance.reopen: {}", e.message),
                    Request::Shell if e.code != 0 => eprintln!("oscairn-ui: memory.shell: {}", e.message),
                    _ => {}
                }
                return;
            }
        };
        match request {
            Request::Info => {
                self.performance_id = r["id"].as_str().unwrap_or_default().to_string();
                self.follow();
                self.show(|s| s.performance = topbar::performance(&r))
            }
            Request::Graph => {
                self.on_play(|p| p.graph(&r));
                self.on_part(|p| p.graph(&r));
                self.show(|s| s.scene = topbar::scene(&r))
            }
            Request::Transport => self.show(|s| s.tempo = topbar::tempo(&r)),
            Request::Memory => {
                let ram = topbar::ram(&r, self.device_memory);
                self.show(|s| s.ram = ram)
            }
            Request::Reopen => {
                if let Some(notice) = r["notice"].as_str().filter(|n| !n.is_empty()) {
                    eprintln!("oscairn-ui: the last Performance did not open: {notice}");
                }
            }
            Request::Library(sounds) => {
                let total = r["total"].as_u64();
                self.show(|s| *if sounds { &mut s.sounds } else { &mut s.performances } = total)
            }
            Request::Subscribe | Request::Shell | Request::View | Request::Setlist(_) | Request::Play(_) | Request::Part(_) => {}
        }
    }

    /// Changes what the top bar shows and sets only what changed in the window.
    fn show(&mut self, change: impl FnOnce(&mut Shown)) {
        let mut next = self.shown.clone();
        change(&mut next);
        let Some(ui) = self.ui() else { return };
        let bar = ui.global::<Bar>();
        let was = &self.shown;
        if next.performance != was.performance {
            bar.set_performance(next.performance.as_str().into());
        }
        if next.scene != was.scene {
            bar.set_scene(next.scene.as_str().into());
        }
        if next.tempo != was.tempo {
            bar.set_tempo(next.tempo.as_str().into());
        }
        if next.cpu != was.cpu {
            bar.set_cpu(next.cpu.0.unwrap_or(0).into());
            bar.set_cpu_known(next.cpu.0.is_some());
            bar.set_cpu_level(level(next.cpu));
        }
        if next.ram != was.ram {
            bar.set_ram(next.ram.0.unwrap_or(0).into());
            bar.set_ram_known(next.ram.0.is_some());
            bar.set_ram_level(level(next.ram));
        }
        let lines_changed = (&next.performance, &next.audio_device, next.performances, next.sounds) != (&was.performance, &was.audio_device, was.performances, was.sounds);
        self.shown = next;
        if lines_changed {
            self.show_home();
        }
    }

    /// A tile's line: what the JUCE Launcher says of each app, from what the shell knows.
    fn line(&self, app: &home::App) -> String {
        let s = &self.shown;
        let plural = |n: u64, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
        match app.id {
            "oscairn.performance" if !s.performance.is_empty() => format!("{}: play or edit it", s.performance),
            "oscairn.library" => match (s.performances, s.sounds) {
                (Some(p), Some(n)) => format!("{}, {}", plural(p, "Performance"), plural(n, "Sound")),
                _ => app.line.into(),
            },
            "oscairn.settings" if s.audio_device == "null" => "No audio device (--audio null)".into(),
            "oscairn.settings" if !s.audio_device.is_empty() => s.audio_device.clone(),
            _ => app.line.into(),
        }
    }

    /// Home's tiles, each set only when it changed: a cursor move redraws the two tiles.
    fn show_home(&mut self) {
        for (n, &i) in self.home.tiles.iter().enumerate() {
            let app = &home::APPS[i];
            let tile = TileData { name: app.name.into(), line: self.line(app).into(), icon: app.icon, cursor: self.home.cursor.at == Some(n) };
            match self.tiles.row_data(n) {
                Some(was) if was == tile => {}
                Some(_) => self.tiles.set_row_data(n, tile),
                None => self.tiles.push(tile),
            }
        }
    }

    fn act(&mut self, action: Action) {
        match (self.screen, action) {
            (Screen::Opening, Action::Cancel(_)) if !self.hands_over => self.go(Screen::Home),
            (Screen::Opening, _) => {} // the display is going to the JUCE app
            (Screen::Home, _) => {
                if let Some(ui) = self.ui() {
                    self.home.columns = ui.get_home_columns().max(1) as usize;
                }
                let open = self.home.act(action);
                self.show_home();
                if let Some(app) = open {
                    self.open(app);
                }
            }
            (Screen::Play, _) => {
                let mut outcome = play::Outcome::Stay;
                self.on_play(|p| outcome = p.act(action));
                match outcome {
                    play::Outcome::Back => {
                        let from = self.play_from;
                        self.go(from);
                    }
                    play::Outcome::Step(by) => self.on_setlist(|s| s.step_by(by)),
                    play::Outcome::Open(id) => self.open_part(id),
                    play::Outcome::Stay => {}
                }
            }
            (Screen::Part, _) => self.part_act(|p| p.act(action)),
            (Screen::Setlist, _) => {
                let mut back = false;
                self.on_setlist(|s| back = s.act(action) == setlist::Outcome::Back);
                if back {
                    self.go(Screen::Home);
                }
            }
        }
    }

    fn tap(&mut self, tile: i32) {
        if self.screen != Screen::Home {
            return;
        }
        let open = self.home.tap(tile.max(0) as usize);
        self.show_home();
        if let Some(app) = open {
            self.open(app);
        }
    }

    /// Tells the engine whether the screen shows lists: Home's tiles and the Setlist are; a JUCE
    /// screen opening says its own once it runs.
    fn send_view(&mut self) {
        let lists = self.screen != Screen::Opening;
        self.send(Request::View, "controllers.view", json!({"lists": lists}));
    }

    /// Something for the Setlist screen, then what it asks the engine sent.
    fn on_setlist(&mut self, f: impl FnOnce(&mut setlist::Screen)) {
        f(&mut self.setlist);
        for (request, method, params) in std::mem::take(&mut self.setlist.out) {
            self.send(Request::Setlist(request), method, params);
        }
        self.follow();
    }

    /// Something for Play, then what it asks the engine sent.
    fn on_play(&mut self, f: impl FnOnce(&mut play::Screen)) {
        f(&mut self.play);
        for (request, method, params) in std::mem::take(&mut self.play.out) {
            self.send(Request::Play(request), method, params);
        }
    }

    /// Something for the Part screen, then what it asks the engine sent.
    fn on_part<R>(&mut self, f: impl FnOnce(&mut part::Screen) -> R) -> R {
        let r = f(&mut self.part);
        for (request, method, params) in std::mem::take(&mut self.part.out) {
            self.send(Request::Part(request), method, params);
        }
        r
    }

    /// A navigation action or tap for the Part screen, and where it takes the shell.
    fn part_act(&mut self, f: impl FnOnce(&mut part::Screen) -> part::Outcome) {
        match self.on_part(f) {
            part::Outcome::Back => self.go(Screen::Play),
            part::Outcome::Handover => self.hand_over(&home::APPS[0]),
            part::Outcome::Stay => {}
        }
    }

    /// A part selected in Play: its settings.
    fn open_part(&mut self, id: u32) {
        self.part.open(id);
        self.go(Screen::Part);
    }

    /// Previous and Next show on Play while the Performance open is the setlist's slot playing.
    fn follow(&mut self) {
        let on = !self.performance_id.is_empty() && self.setlist.playing() == Some(self.performance_id.as_str());
        self.play.follow(on);
    }

    /// The meters, asked unless the last answer is still out: a slow engine never piles them up.
    fn poll_meters(&mut self) {
        if self.client.waiting(|r| matches!(r, Request::Play(play::Request::Meters))).is_none() {
            self.send(Request::Play(play::Request::Meters), "parts.meters", json!({}));
        }
    }

    fn go(&mut self, screen: Screen) {
        if self.screen != screen {
            // A cursor drops when its screen goes, as on focus loss in the JUCE lists.
            self.home.cursor = self.home.cursor.clear();
            self.show_home();
            if self.screen == Screen::Setlist {
                self.on_setlist(setlist::Screen::hidden);
            }
            if self.screen == Screen::Play {
                self.play.hidden();
                self.meters.stop();
            }
            if self.screen == Screen::Part {
                self.on_part(part::Screen::hidden);
            }
            if live(self.screen) && !live(screen) {
                self.send(Request::Subscribe, "unsubscribe", json!({"topics": ["controls.changed"]}));
            }
            if screen == Screen::Play && self.screen != Screen::Part {
                self.play_from = self.screen;
            }
        }
        let was = self.screen;
        self.screen = screen;
        if (was == Screen::Opening) != (screen == Screen::Opening) {
            self.send_view();
        }
        if let Some(ui) = self.ui() {
            ui.set_screen(screen as i32);
            let bar = ui.global::<Bar>();
            bar.set_where(match screen {
                Screen::Home => "Home".into(),
                Screen::Setlist => "Setlist".into(),
                Screen::Play => "Play".into(),
                Screen::Part => "Part".into(),
                Screen::Opening => ui.get_opening(),
            });
            bar.set_home_button(screen != Screen::Home);
        }
        if screen == Screen::Setlist {
            self.on_setlist(setlist::Screen::shown);
        }
        if !live(was) && live(screen) {
            // The first controls.changed after subscribing carries every level and mute.
            self.send(Request::Subscribe, "subscribe", json!({"topics": ["controls.changed"]}));
        }
        if screen == Screen::Part {
            self.ask_performance();
            self.send(Request::Part(part::Request::Status), "session.status", json!({}));
            self.on_part(part::Screen::shown);
        }
        if was != Screen::Play && screen == Screen::Play {
            self.ask_performance();
            self.follow();
            self.play.shown();
            self.meters.start(slint::TimerMode::Repeated, play::METER_INTERVAL, || with_app(|a| a.poll_meters()));
        }
    }

    /// An app's tile confirmed: a Rust screen shows; a JUCE one gets the display (docs/rust-ui.md).
    fn open(&mut self, app: &'static home::App) {
        match app.id {
            "oscairn.setlist" => self.go(Screen::Setlist),
            "oscairn.performance" => self.go(Screen::Play),
            _ => self.hand_over(app),
        }
    }

    /// A JUCE screen asked for: it shows "Opening X...", and the display goes to the JUCE app.
    fn hand_over(&mut self, app: &'static home::App) {
        if let Some(ui) = self.ui() {
            ui.set_opening(app.name.into());
            ui.set_hands_over(self.hands_over);
        }
        self.go(Screen::Opening);
        if self.hands_over {
            self.handover = app.id.strip_prefix("oscairn.");
            // After the frame that says so: the program ends and the start script gives the
            // display to X and the JUCE app (docs/rust-ui.md).
            self.leave.start(slint::TimerMode::SingleShot, Duration::from_millis(100), || {
                let _ = slint::quit_event_loop();
            });
        }
    }
}
