// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shell: one connection to the engine, the top bar it feeds, and the screens. Runs on the
//! UI thread; the client's events reach it through the event loop.
//!
//! What it asks the engine and when: on connecting, `subscribe` to `status` and
//! `graph.changed`, `performance.reopen` (the Performance last open, as the JUCE app asks at its
//! start), `performance.info` and `graph.get`. Each `status` (at most 4 a second) moves the
//! meters at most twice a second and then asks `memory.state` and `transport.get`; each
//! `graph.changed` asks `performance.info` and `graph.get` again. Nothing else wakes it at rest.

use crate::client::{Client, Error, Event, BUSY};
use crate::topbar::{self, Level, Meter, MeterSettings};
use crate::{Bar, Shell};
use serde_json::{json, Value};
use slint::{ComponentHandle, SharedString};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What each request is for, so its answer goes where it belongs.
pub enum Request {
    Subscribe,
    Reopen,
    Info,
    Graph,
    Transport,
    Memory,
}

impl Request {
    /// The engine taking long over it is shown: the musician waits on it.
    fn waited_on(&self) -> bool {
        matches!(self, Request::Reopen)
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
}

thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            f(app)
        }
    })
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
pub fn start(ui: &Shell, socket: PathBuf, device_memory: u64, post: impl Fn(Event<Request>) + Send + 'static) -> std::io::Result<()> {
    let client = Client::start(socket, post)?;
    ui.set_engine("connecting".into());
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            ui: ui.as_weak(),
            client,
            device_memory,
            shown: Shown::default(),
            last_meters: None,
            stale: false,
            watchdog: slint::Timer::default(),
        })
    });
    Ok(())
}

/// One of the client's events, on the UI thread.
pub fn dispatch(event: Event<Request>) {
    with_app(|a| a.event(event))
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
                self.send(Request::Subscribe, "subscribe", json!({"topics": ["status", "graph.changed"]}));
                self.send(Request::Reopen, "performance.reopen", json!({}));
                self.ask_performance();
            }
            Event::Lost(why) => {
                if let Some(ui) = self.ui() {
                    ui.set_engine(format!("lost: {why}").into());
                }
            }
            Event::Notification { method, params } => match method.as_str() {
                "status" => self.status(&params),
                "graph.changed" => self.ask_performance(),
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
        let now = Instant::now();
        if self.last_meters.is_some_and(|t| now.duration_since(t) < METER_INTERVAL) {
            return;
        }
        self.last_meters = Some(now);
        self.show(|s| s.cpu = topbar::cpu(status));
        self.ask(Request::Memory, "memory.state");
        self.ask(Request::Transport, "transport.get");
        if self.stale {
            self.ask_performance();
        }
    }

    fn reply(&mut self, request: Request, result: Result<Value, Error>) {
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                if e.code == BUSY && matches!(request, Request::Info | Request::Graph) {
                    self.stale = true;
                }
                if matches!(request, Request::Reopen) && e.code != 0 {
                    eprintln!("oscairn-ui: performance.reopen: {}", e.message);
                }
                return;
            }
        };
        match request {
            Request::Info => self.show(|s| s.performance = topbar::performance(&r)),
            Request::Graph => self.show(|s| s.scene = topbar::scene(&r)),
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
            Request::Subscribe => {}
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
        self.shown = next;
    }
}
