// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shell: one connection to the engine, the top bar it feeds, and the screens. Runs on the
//! UI thread; the client's events reach it through the event loop.
//!
//! What it asks the engine and when: on connecting, `subscribe` to `status`,
//! `graph.changed`, `controllers` and `record`, `memory.shell` (its own memory; the rest of the session's setup is the
//! engine's, docs/rust-ui.md "Session setup"), `performance.reopen` (the Performance last open, as the JUCE app asks at its
//! start), `performance.info` and `graph.get`; the first `status` of a connection asks `record.takes`. A `status` (at most 4 a second) that says what the
//! last said (its load as a whole percent, the figure the bar shows, is compared, not the fraction) costs nothing:
//! no request, no screen asked, no frame. One that changed moves the meters at most twice a second and then asks
//! `memory.state` and `transport.get`; each `graph.changed` asks `performance.info` and `graph.get` again; a `record`
//! notification (a take's state changed, at most 10 a second, none at rest) is the top bar's recording mark. Nothing else wakes it at rest.
//! While Play is shown it also subscribes to `controls.changed` and asks `parts.meters` every
//! 100 ms (one at a time); leaving Play ends both.

use crate::client::{Client, Error, Event, BUSY};
use crate::home::{self, Home};
use crate::nav::{self, Action};
use crate::chain;
use crate::controls;
use crate::keyboard;
use crate::library;
use crate::settings_app;
use crate::menu;
use crate::notice;
use crate::mixer;
use crate::params;
use crate::part;
use crate::parts;
use crate::piano;
use crate::sounds;
use crate::play;
use crate::routing;
use crate::profile::Profile;
use crate::scene;
use crate::setlist;
use crate::topbar::{self, Level, Meter, MeterSettings};
use crate::{Bar, BarAction, GridState, HomeState, ControlsState, KeyboardState, MenuState, MixerState, RoutingState, PartState, PartsState, PianoState, PlayState, SetlistState, Shell, TileData};
use serde_json::{json, Value};
use slint::{ComponentHandle, Model, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod library_glue;
mod settings_glue;
mod wait_glue;

pub use settings_glue::theme_followed;
#[cfg(test)]
pub use settings_glue::{settings_folders, settings_in_rust};

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
    /// record.get: whether a take is recording and for how long, and how many seconds Capture keeps.
    Record,
    /// record.start or record.stop from the top bar's Record and Capture: the answer says whether it was done.
    RecordDo(&'static str),
    /// edit.history, edit.undo, edit.redo: what undo and redo would now do.
    History,
    /// library.save, from the top bar.
    Save,
    /// library.query's total of Performances (false) or Sounds (true), for Home's Library line.
    Library(bool),
    /// controllers.view: whether lists are on screen, so the panel's navigate controls navigate.
    View,
    /// session.status, asked as the playing keyboard opens.
    Keys,
    /// notes.send from the playing keyboard: nothing to read in the answer.
    Notes,
    Setlist(setlist::Request),
    Play(play::Request),
    Part(part::Request),
    Parts(parts::Request),
    Mixer(mixer::Request),
    Controls(controls::Request),
    Routing(routing::Request),
    Chain(chain::Request),
    Sounds(sounds::Request),
    Params(params::Request),
    Scene(scene::Request),
    LibraryApp(library::Request),
    SettingsApp(settings_app::Call),
}

impl Request {
    /// The engine taking long over it is shown: the musician waits on it.
    fn waited_on(&self) -> bool {
        match self {
            Request::Reopen => true,
            Request::Setlist(r) => setlist::Screen::waits(r),
            Request::LibraryApp(r) => library::Screen::waits(r),
            Request::SettingsApp(c) => settings_app::Screen::waits(c),
            _ => false,
        }
    }
}

const METER_INTERVAL: Duration = Duration::from_millis(500);

/// How long the screens' meters keep being asked after the last signal: a tail dies away in less (they hold and fall),
/// and after it the polling stops until something that may make a sound wakes it (`wake_meters`).
const QUIET_AFTER: Duration = Duration::from_secs(2);

/// What the top bar shows now, so only a change reaches the window.
#[derive(Clone, Default, PartialEq)]
struct Shown {
    performance: String,
    scene: String,
    tempo: String,
    cpu: Meter,
    ram: Meter,
    recording: Option<topbar::Recording>,
    audio_device: String,
    performances: Option<u64>,
    sounds: Option<u64>,
    /// The plugin packages installed, which Home's Library line counts.
    plugins: usize,
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
    /// A part's insert chain (chain.rs), opened from the Part screen.
    Chain = 5,
    /// The sound chooser (sounds.rs), opened from the Part screen.
    Sounds = 6,
    /// A plugin's parameter pages (params.rs), opened from the chain.
    Params = 7,
    /// The playing scene's setup (scene.rs), opened from Play's bar.
    Scene = 8,
    /// The parts and buses of the Performance (parts.rs), opened from Play's Edit.
    Parts = 9,
    /// Every part and bus side by side (mixer.rs), opened from the Parts list.
    Mixer = 10,
    /// The controls and their targets (controls.rs), opened from the Parts list.
    Controls = 11,
    /// The real graph and its wires (routing.rs), opened from the Parts list.
    Routing = 12,
    /// The Library app (library/), opened from Home.
    Library = 13,
    /// The Settings app (settings_app.rs), opened from Home.
    Settings = 14,
}

pub struct App {
    ui: slint::Weak<Shell>,
    client: Arc<Client<Request>>,
    device_memory: u64,
    shown: Shown,
    last_meters: Option<Instant>,
    /// The last status that was acted on, by what the bar and the screens read of it: an identical one costs nothing.
    last_status: Option<Value>,
    /// A changed status whose meters wait for the next METER_INTERVAL.
    pending_status: Option<Value>,
    /// The last status without its load: what changed in it besides the figure the bar shows.
    last_other: Option<Value>,
    /// RAM and the tempo are asked again at the next meters: the status said something besides its load.
    state_stale: bool,
    /// Since when the meters of the screen have answered nothing but silence, and whether their polling is stopped.
    quiet_since: Option<Instant>,
    meters_asleep: bool,
    meter_every: Duration,
    /// How many seconds the retroactive buffer keeps (record.get's `retro`): Capture is off when none.
    retro: f64,
    /// The transport is playing (the `transport` topic): the meters are asked while it does.
    playing: bool,
    /// An ask for what undo names was dropped while one was out: asked again when it answers.
    history_again: bool,
    flush: slint::Timer,
    /// performance.info and graph.get refused while the engine was busy: asked again at the next meters.
    stale: bool,
    watchdog: slint::Timer,
    /// What the player waits on, and what is wrong (wait_glue.rs).
    wait: wait_glue::Wait,
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
    parts: parts::Screen,
    mixer: mixer::Screen,
    controls: controls::Screen,
    routing: routing::Screen,
    /// A menu over the screen that asked for it (menu.rs).
    menu: Option<(Screen, menu::Menu)>,
    chain: chain::Screen,
    sounds: sounds::Screen,
    params: params::Screen,
    scene: scene::Screen,
    library: library::Screen,
    settings: settings_app::Screen,
    /// A question over the screen that asked it (notice.rs).
    question: Option<(Screen, notice::Notice)>,
    /// The Library tile opens the Rust app (library_glue.rs: not yet, until every screen of it is built).
    library_in_rust: bool,
    /// Tests: the memory memory.shell says instead of this process's (a test binary holds every test's windows: far more than a shell).
    memory_override: Option<u64>,
    /// The Settings tile opens the Rust app (settings_glue.rs: not yet, until every page of it is built).
    settings_in_rust: bool,
    /// Whether the plugin scanner has been started (the first time the chain is opened).
    scan_started: bool,
    /// The on-screen keyboard, open for the screen that asked for text.
    keyboard: Option<(Screen, keyboard::Keyboard)>,
    /// The on-screen playing keyboard (piano.rs), over whatever screen opened it.
    piano: Option<piano::Piano>,
    /// Ends a note the playing keyboard's cursor started.
    piano_tap: slint::Timer,
    /// The graph as last read, for the playing keyboard's bands.
    last_graph: Value,
    /// The profile's textEntry is onScreen: the keyboard shows its keys.
    text_on_screen: bool,
    /// Repeats Del, Left and Right while one is held.
    repeat: slint::Timer,
    /// Where Play was opened from, which Cancel goes back to.
    play_from: Screen,
    /// Where the Part screen was opened from (Play or the Parts list), which Back returns to.
    part_from: Screen,
    /// Asks for the meters while Play is shown, and only then.
    meters: slint::Timer,
    /// The open Performance's library ID (performance.info), which the setlist's slot playing is compared with.
    performance_id: String,
    performance_name: String,
    /// What undo and redo would do (edit.history), named on the top bar's buttons.
    history: topbar::History,
    /// The top bar's actions as shown, and the stop the cursor is on (the Performance's name is
    /// stop 0 when shown, then each action): None while the screen has the cursor.
    bar_list: Vec<topbar::Act>,
    /// The title's options as last set on the bar.
    shown_path: Vec<String>,
    bar_at: Option<usize>,
    /// The bar is too narrow for its actions and shows one "More", a menu of them (`Bar.more`).
    bar_more: bool,
    /// The menu open is the bar's "More": a choice is an action of `bar_list`.
    bar_menu: bool,
    bar_rows: Rc<VecModel<BarAction>>,
    /// Clears what the last action said.
    notice: slint::Timer,
    /// The text keyboard is open for Save (true: Save As New).
    saving: Option<bool>,
}

thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            f(app)
        }
    })
}

/// The Performance app's screens, which carry the top bar's actions (undo, redo, save).
fn has_actions(screen: Screen) -> bool {
    matches!(screen, Screen::Play | Screen::Part | Screen::Parts | Screen::Mixer | Screen::Controls | Screen::Routing | Screen::Chain | Screen::Sounds | Screen::Params | Screen::Scene)
}

/// The screens that follow levels and mutes moved live (controls.changed).
fn live(screen: Screen) -> bool {
    matches!(screen, Screen::Play | Screen::Part | Screen::Parts | Screen::Mixer | Screen::Controls)
}

/// Whether a request is the meters of a screen (its answer says whether a signal is there).
fn is_meters(request: &Request) -> bool {
    matches!(request, Request::Play(play::Request::Meters) | Request::Parts(parts::Request::Meters) | Request::Mixer(mixer::Request::Meters) | Request::Routing(routing::Request::Meters))
}

/// Whether sending this may make a sound or move a meter: the meters are asked again for a while after it.
fn wakes_meters(method: &str) -> bool {
    method != "parts.meters" && ["notes.", "scenes.", "controls.", "parts.", "master.", "params.set", "edit.", "library.load", "performance.", "audition.", "record.start"].iter().any(|p| method.starts_with(p))
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
    let bar_rows = Rc::new(VecModel::<BarAction>::default());
    ui.global::<Bar>().set_actions(bar_rows.clone().into());
    ui.global::<Bar>().on_action(|i| with_app(|a| a.bar_tap(i.max(0) as usize)));
    ui.global::<Bar>().on_more_tapped(|| with_app(|a| a.bar_tap(0)));
    ui.global::<Bar>().on_more_changed(|| with_app(|a| a.show_bar()));
    ui.global::<Bar>().on_pill_tapped(|| with_app(|a| a.bar_pill()));
    ui.global::<Bar>().on_back_tapped(|| with_app(|a| a.bar_back()));
    ui.on_key(|text| key(&text));
    ui.on_key_released(|text| {
        with_app(|a| {
            let _ = a.on_piano(|p| p.typed(&text, false));
        })
    });
    let keys = ui.global::<PianoState>();
    keys.on_key_down(|i, y| {
        with_app(|a| {
            let _ = a.on_piano(|p| p.touch(i.max(0) as usize, true, y as f64));
        })
    });
    keys.on_key_up(|i| {
        with_app(|a| {
            let _ = a.on_piano(|p| p.touch(i.max(0) as usize, false, 0.));
        })
    });
    keys.on_control(|i| with_app(|a| a.piano_control(i.max(0) as usize)));
    let lists = ui.global::<SetlistState>();
    lists.on_tapped_slot(|i| with_app(|a| a.on_setlist(|s| s.tap_slot(i.max(0) as usize))));
    lists.on_tapped_stop(|i| with_app(|a| a.on_setlist(|s| s.tap_stop(i.max(0) as usize))));
    lists.on_resized(|| with_app(|a| a.on_setlist(setlist::Screen::shown)));
    let play = ui.global::<PlayState>();
    play.on_scene_tapped(|i| with_app(|a| a.on_play(|p| p.tap_scene(i))));
    play.on_mute_tapped(|i| with_app(|a| a.on_play(|p| p.tap_mute(i))));
    play.on_solo_tapped(|i| with_app(|a| a.on_play(|p| p.tap_solo(i))));
    play.on_level(|i, f| with_app(|a| a.on_play(|p| p.level(i, f))));
    play.on_held(|i, down| with_app(|a| a.on_play(|p| p.hold(i, down))));
    play.on_step(|by| with_app(|a| a.on_setlist(|s| s.step_by(by as i64))));
    play.on_resized(|| with_app(|a| a.on_play(play::Screen::shown)));
    play.on_hint_keys(|| with_app(|a| a.hint_tap(0)));
    play.on_hint_dismiss(|| with_app(|a| a.hint_tap(1)));
    play.on_knob_grab(|t, _| with_app(|a| a.on_play(|p| p.knob_grab(t))));
    play.on_knob_drag(|t, _, n| with_app(|a| a.on_play(|p| p.knob_drag(t, n))));
    play.on_knob_release(|_| with_app(|a| a.on_play(|p| p.knob_release())));
    play.on_knob_tap(|t, _| with_app(|a| a.on_play(|p| p.knob_tap(t))));
    play.on_knob_resized(|| with_app(|a| a.on_play(play::Screen::shown)));
    play.on_knob_button(|i| with_app(|a| a.on_play(|p| p.knob_button(i))));
    play.on_learn_reset(|| with_app(|a| a.on_play(|p| p.learn_button(crate::row::Stop::Default))));
    play.on_learn_done(|| with_app(|a| a.on_play(|p| p.learn_button(crate::row::Stop::Done))));
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
    part.on_tapped(|i| with_app(|a| a.list_tap(i)));
    part.on_scrolled(|first| with_app(|a| a.list_scroll_to(first)));
    part.on_step(|i, by| with_app(|a| a.list_step(i, by)));
    part.on_info_scroll(|by| with_app(|a| a.list_scroll(by)));
    part.on_resized(|| with_app(|a| a.refit()));
    let list = ui.global::<PartsState>();
    list.on_open(|i| with_app(|a| a.parts_act(|p| p.tap_open(i))));
    list.on_mute(|i| with_app(|a| a.on_parts(|p| p.tap_mute(i))));
    list.on_level(|i, f| with_app(|a| a.on_parts(|p| p.level(i, f))));
    list.on_send(|i, k, f| with_app(|a| a.on_parts(|p| p.send(i, k, f))));
    list.on_held(|i, cell, down| with_app(|a| a.on_parts(|p| p.hold(i, cell, down))));
    list.on_tool(|i| with_app(|a| a.parts_act(|p| p.tap_tool(i))));
    list.on_resized(|| with_app(|a| a.on_parts(parts::Screen::shown)));
    let mix = ui.global::<MixerState>();
    mix.on_level(|i, f| with_app(|a| a.on_mixer(|m| m.level(i, f))));
    mix.on_pan(|i, f| with_app(|a| a.on_mixer(|m| m.pan(i, f))));
    mix.on_send(|i, j, f| with_app(|a| a.on_mixer(|m| m.send(i, j, f))));
    mix.on_mute(|i| with_app(|a| a.on_mixer(|m| m.tap_mute(i))));
    mix.on_solo(|i| with_app(|a| a.on_mixer(|m| m.tap_solo(i))));
    mix.on_held(|i, control, down| with_app(|a| a.on_mixer(|m| m.hold(i, control, down))));
    mix.on_resized(|| with_app(|a| a.on_mixer(mixer::Screen::shown)));
    let ctl = ui.global::<ControlsState>();
    ctl.on_level(|i, f| with_app(|a| a.on_controls(|c| c.level(i, f))));
    ctl.on_range(|i, end, f| with_app(|a| a.on_controls(|c| c.range(i, end, f))));
    ctl.on_held(|i, cell, down| with_app(|a| a.on_controls(|c| c.hold(i, cell, down))));
    ctl.on_press(|i, cell| with_app(|a| a.controls_act(|c| c.tap(i, cell))));
    ctl.on_table_press(|i, col| with_app(|a| a.on_controls(|c| c.tap_table(i, col))));
    ctl.on_tool(|i| with_app(|a| a.controls_act(|c| c.tap_tool(i))));
    ctl.on_turn_columns(|by| with_app(|a| a.on_controls(|c| c.turn_columns(by))));
    ctl.on_resized(|| with_app(|a| a.on_controls(controls::Screen::shown)));
    let rt = ui.global::<RoutingState>();
    rt.on_tapped(|kind, i| with_app(|a| a.routing_act(|r| r.tap(kind, i))));
    rt.on_chevron(|kind, i| with_app(|a| a.on_routing(|r| r.tap_chevron(kind, i))));
    rt.on_panel_press(|row, cell| with_app(|a| a.routing_act(|r| r.tap_cell(row, cell))));
    rt.on_panel_moved(|row, cell, f| with_app(|a| a.on_routing(|r| r.level(row, cell, f))));
    rt.on_panel_held(|row, cell, down| with_app(|a| a.on_routing(|r| r.hold(row, cell, down))));
    rt.on_turn_panel(|by| with_app(|a| a.on_routing(|r| r.turn_panel(by))));
    rt.on_resized(|| with_app(|a| a.on_routing(routing::Screen::shown)));
    let menu = ui.global::<MenuState>();
    menu.on_tapped(|i| with_app(|a| a.menu_tap(i)));
    menu.on_back(|| with_app(|a| a.menu_back()));
    menu.on_close(|| with_app(|a| a.close_menu()));
    menu.on_turn(|by| with_app(|a| a.menu_turn(by)));
    menu.on_resized(|| with_app(|a| a.menu_shown()));
    ui.global::<crate::LibraryState>().on_show_page(|i| with_app(|a| a.on_library(|l| l.tap_page(i.max(0) as usize))));
    ui.global::<crate::LibraryState>().on_search_tapped(|| with_app(|a| a.library_act(|l| l.ask_search())));
    ui.global::<crate::LibraryState>().on_info_toggled(|| with_app(|a| a.on_library(|l| l.toggle_info())));
    ui.global::<crate::LibraryState>().on_view_toggled(|| with_app(|a| a.on_library(|l| l.toggle_bubbles())));
    ui.global::<crate::LibraryState>().on_compact_toggled(|| with_app(|a| a.on_library(|l| l.toggle_compact())));
    ui.global::<crate::LibraryState>().on_search_cleared(|| with_app(|a| a.on_library(|l| l.clear_search())));
    ui.global::<crate::LibraryState>().on_side_tapped(|i| with_app(|a| a.on_library(|l| l.side_tap(i))));
    ui.global::<crate::LibraryState>().on_side_scrolled(|first| with_app(|a| a.on_library(|l| l.side_scroll_to(first))));
    ui.global::<crate::LibraryState>().on_side_resized(|| with_app(|a| a.on_library(|l| l.show())));
    ui.global::<crate::SettingsState>().on_show_page(|i| with_app(|a| a.on_settings(|s| s.tap_page(i.max(0) as usize))));
    let notice = ui.global::<crate::NoticeState>();
    notice.on_tapped(|i| with_app(|a| a.notice_tap(i.max(0) as usize)));
    notice.on_close(|| with_app(|a| a.notice_act(Action::Cancel(None))));
    let grid = ui.global::<GridState>();
    grid.on_grab(|t, part| with_app(|a| a.on_params(|p| p.grab(t, part))));
    grid.on_drag(|t, part, n| with_app(|a| a.on_params(|p| p.drag(t, part, n))));
    grid.on_release(|t| with_app(|a| a.on_params(|p| p.release(t))));
    grid.on_tap(|t, stop| with_app(|a| a.on_params(|p| p.tap(t, stop))));
    grid.on_choose(|k| with_app(|a| a.on_params(|p| p.choose(k))));
    grid.on_show_page(|k| with_app(|a| a.on_params(|p| p.show_page(k))));
    grid.on_show_tab(|k| with_app(|a| a.on_params(|p| p.show_tab(k))));
    grid.on_scrolled(|by| with_app(|a| a.on_params(|p| p.scrolled(by))));
    grid.on_view_down(|v, x, y| {
        let mut taken = false;
        with_app(|a| taken = a.on_params(|p| p.view_down(v, x, y)));
        taken
    });
    grid.on_view_move(|v, x, y| with_app(|a| a.on_params(|p| p.view_move(v, x, y))));
    grid.on_view_up(|v, x, y| with_app(|a| a.on_params(|p| p.view_up(v, x, y))));
        grid.on_resized(|| with_app(|a| a.on_params(crate::params::Screen::shown)));
    let keys = ui.global::<KeyboardState>();
    keys.on_pressed(|i| with_app(|a| a.keyboard_press(i)));
    crate::keystrip::install(ui);
    let strip = ui.global::<crate::StripState>();
    strip.on_down(|share, width| with_app(|a| a.on_chain(|c| c.strip_down(share as f64, width as f64))));
    strip.on_moved(|share| with_app(|a| a.on_chain(|c| c.strip_move(share as f64))));
    strip.on_up(|| with_app(|a| a.on_chain(|c| c.strip_up())));
    strip.on_reset(|| with_app(|a| a.on_chain(|c| c.strip_reset())));
    keys.on_released(|| with_app(|a| a.repeat.stop()));
    keys.on_cancel(|| with_app(|a| a.keyboard_act(Action::Cancel(None))));
    // The JUCE Setlist app's own state file, so either opens where the other left off.
    let state = crate::settings::data_dir().map(|d| d.join("AppData/oscairn.setlist/state.json"));
    APP.with(|a| {
        let mut app = App {
            ui: ui.as_weak(),
            client,
            device_memory: profile.memory,
            shown: Shown::default(),
            last_meters: None,
            last_status: None,
            pending_status: None,
            last_other: None,
            state_stale: false,
            quiet_since: None,
            meters_asleep: false,
            meter_every: Duration::from_millis(100),
            playing: false,
            history_again: false,
            retro: 0.,
            flush: slint::Timer::default(),
            stale: false,
            watchdog: slint::Timer::default(),
            wait: wait_glue::Wait::default(),
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
            parts: parts::Screen::new(ui),
            mixer: mixer::Screen::new(ui),
            controls: controls::Screen::new(ui),
            routing: routing::Screen::new(ui),
            menu: None,
            chain: chain::Screen::new(ui),
            sounds: sounds::Screen::new(ui),
            params: params::Screen::new(ui),
            scene: scene::Screen::new(ui),
            library: library::Screen::new(ui),
            settings: settings_app::Screen::new(ui, profile),
            question: None,
            library_in_rust: library_glue::in_rust(),
            memory_override: None,
            settings_in_rust: settings_glue::in_rust(),
            scan_started: false,
            keyboard: None,
            piano: None,
            piano_tap: slint::Timer::default(),
            last_graph: Value::Null,
            text_on_screen: profile.text_on_screen,
            repeat: slint::Timer::default(),
            play_from: Screen::Home,
            part_from: Screen::Play,
            meters: slint::Timer::default(),
            performance_id: String::new(),
            performance_name: String::new(),
            history: topbar::History::default(),
            bar_list: vec![],
            shown_path: vec![],
            bar_at: None,
            bar_more: false,
            bar_menu: false,
            bar_rows,
            notice: slint::Timer::default(),
            saving: None,
        };
        app.part.keys = profile.on_screen_keyboard;
        app.routing.offer_keyboard(profile.on_screen_keyboard);
        app.routing.set_collapsed(crate::settings::routing_collapsed());
        app.play.offer_keyboard(profile.on_screen_keyboard, crate::settings::keyboard_hint_dismissed());
        app.play.knobs(profile.controls.clone());
        app.show_home();
        *a.borrow_mut() = Some(app);
    });
    Ok(())
}

/// One of the client's events, on the UI thread.
pub fn dispatch(event: Event<Request>) {
    with_app(|a| a.event(event))
}

/// A key typed on the computer's keyboard: text for the on-screen keyboard's field when it is open
/// on a screen with a typing keyboard, else a navigation action. True when it was taken.
pub fn key(text: &str) -> bool {
    APP.with(|a| a.borrow_mut().as_mut().is_some_and(|app| app.key(text)))
}

/// Shows `screen` at once, with whatever state it has: for the tests of what every screen has in common.
#[cfg(test)]
pub fn goto(screen: Screen) {
    with_app(|a| a.go(screen))
}

/// Tests: whether Home's Library tile opens the Rust app (it does not, until the whole app is built).
#[cfg(test)]
pub fn library_in_rust(on: bool) {
    with_app(|a| a.library_in_rust = on)
}

/// Tests: how long a wait lasts before the banner says it is far past its usual, and how long a request nothing explains
/// may be out before it says the engine is slow to answer (30 s and 2 s).
#[cfg(test)]
pub fn wait_limits(long: Duration, no_answer: Duration) {
    with_app(|a| a.wait.limit(long, no_answer))
}

/// Tests: the memory the shell tells the engine it holds.
#[cfg(test)]
pub fn memory_override(bytes: Option<u64>) {
    with_app(|a| a.memory_override = bytes)
}

/// Tests: the folder the Library's file chooser starts in.
#[cfg(test)]
pub fn library_files_dir(dir: PathBuf) {
    with_app(|a| a.library.set_files_dir(dir))
}

/// A navigation action from any source, on the UI thread.
#[cfg(test)]
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
        if wakes_meters(method) {
            self.wake_meters();
        }
        let waited = request.waited_on();
        if self.client.send(request, method, params) && waited {
            self.refresh_wait();
        }
    }

    /// Asks unless the same is already out: an answer slower than the meters never piles up.
    fn ask(&mut self, request: Request, method: &str) {
        let same = std::mem::discriminant(&request);
        if self.client.waiting(|r| std::mem::discriminant(r) == same).is_none() {
            self.send(request, method, json!({}));
        } else if matches!(request, Request::History) {
            // What undo names changed while the answer to an earlier ask was on its way: that answer may predate it, so
            // the answer asks again (an edit that is two steps, a drag, is the case).
            self.history_again = true;
        }
    }

    fn watch(&mut self) {
        self.on_library(library::Screen::tick);
        self.refresh_wait();
    }

    fn event(&mut self, event: Event<Request>) {
        match event {
            Event::Connected => {
                self.connection(None);
                self.last_meters = None;
                self.last_status = None;
                self.last_other = None;
                self.pending_status = None;
                self.totals.forget();
                self.send(Request::Subscribe, "subscribe", json!({"topics": ["status", "graph.changed", "controllers", "record", "transport"]}));
                self.send_view();
                // The session's setup is the engine's, from the profile (docs/rust-ui.md, "Session
                // setup"); only this UI's own memory is the UI's to send.
                if let Some(bytes) = self.memory_override.or_else(resident_bytes) {
                    self.send(Request::Shell, "memory.shell", json!({ "bytes": bytes }));
                }
                self.send(Request::Reopen, "performance.reopen", json!({}));
                self.ask_performance();
                self.send(Request::Library(false), "library.query", json!({"kind": "performance", "limit": 1}));
                self.send(Request::Library(true), "library.query", json!({"kind": "sound", "limit": 1}));
                self.send(Request::LibraryApp(library::Request::Plugins), "packages.installed", json!({}));
                self.on_setlist(setlist::Screen::connected);
                if live(self.screen) {
                    self.send(Request::Subscribe, "subscribe", json!({"topics": ["controls.changed"]}));
                }
                if has_actions(self.screen) {
                    self.ask(Request::History, "edit.history");
                }
            }
            Event::Lost(why) => {
                self.show(|s| s.recording = None); // what the engine is doing is not known now
                self.last_status = None;
                self.last_other = None;
                self.pending_status = None;
                self.connection(Some(why));
            }
            Event::Busy { method } => self.busy_with(method),
            Event::Notification { method, params } => match method.as_str() {
                "status" => self.status(&params),
                "engine.busy" => self.busy_over(),
                "record" => {
                    let takes = topbar::recording(&params);
                    self.show(|s| s.recording = takes)
                }
                "transport" => {
                    // The tempo as it is (nothing is asked after a status); a transport that plays keeps the meters going.
                    self.playing = params["playing"].as_bool().unwrap_or(false);
                    if self.playing {
                        self.wake_meters();
                    }
                    self.show(|s| s.tempo = topbar::tempo(&params))
                }
                "graph.changed" => {
                    self.wake_meters();
                    self.ask_performance();
                    self.on_library(library::Screen::changed);
                    if has_actions(self.screen) {
                        self.ask(Request::History, "edit.history"); // an edit: what undo names changed
                    }
                }
                "session.resync" if self.screen == Screen::Params => {
                    // The engine's parameter-change history wrapped or a pending command was dropped
                    // (docs/control-protocol.md, "session.resync"): the values a parameter page shows may
                    // be stale, so it reads them again, as an Undo does. No page shown, the arm does not
                    // match and nothing is asked.
                    self.on_params(|p| p.values_again());
                }
                "params.changed" => {
                    self.on_params(|p| p.changed(&params));
                    self.on_play(|p| p.params_changed(&params)); // the knob row's values
                    // A value set by a client is an edit the engine can undo, and no graph.changed
                    // follows it: what undo names is asked here.
                    let set = params["changes"].as_array().is_some_and(|c| c.iter().any(|c| c["source"] == "control" && c["kind"] == "value"));
                    if set && has_actions(self.screen) {
                        self.ask(Request::History, "edit.history");
                    }
                }
                "controls.changed" => {
                    self.wake_meters();
                    // A live level, pan or master (parts.level, parts.pan, master.set) is an edit
                    // with no graph.changed: what undo names is asked here.
                    let moved = ["parts", "master"].iter().any(|k| params[k].as_array().is_some_and(|a| !a.is_empty()) || params[k].is_object());
                    if moved && has_actions(self.screen) {
                        self.ask(Request::History, "edit.history");
                    }
                    self.on_play(|p| p.controls(&params));
                    self.on_part(|p| p.controls(&params));
                    self.on_parts(|p| p.controls(&params));
                    self.on_mixer(|m| m.controls(&params));
                    self.on_controls(|c| c.changed(&params));
                }
                // Read always, so the totals stay current; acted on only by the screen shown, and
                // by none while the display goes to the JUCE app (act drops them then).
                "controllers" => {
                    self.wake_meters();
                    self.on_play(|p| p.controllers(&params)); // the control moved last, for Learn
                    let actions = self.totals.read(&params["navigation"]);
                    // DEC and INC: on the Setlist (nothing over it) the bank before or after; elsewhere the move they always were.
                    match self.totals.take_steps() {
                        0 => {}
                        by if self.screen == Screen::Setlist && self.keyboard.is_none() && self.piano.is_none() && self.question.is_none() && self.menu.is_none() => {
                            self.on_setlist(|s| s.turn_bank(by))
                        }
                        by => self.act(Action::Move { pane: None, by }),
                    }
                    for action in actions {
                        self.act(action);
                    }
                }
                _ => {}
            },
            Event::Reply { request, result } => self.reply(request, result),
        }
    }

    /// The Performance read again; on a screen with the top bar's actions, what Undo and Redo name
    /// too, as an edit with no graph.changed (scenes.*, a live level) changed them as well.
    fn ask_performance(&mut self) {
        self.stale = false;
        self.ask(Request::Info, "performance.info");
        self.ask(Request::Graph, "graph.get");
        if has_actions(self.screen) {
            self.ask(Request::History, "edit.history");
        }
    }

    /// What a status says to the bar and the screens: what the shell reads of it (the inputs and outputs, the failures,
    /// the keys held, the audio device, the counter of the MIDI ports (the Settings page reads its list again when it moves), the memory the workers hold in whole MiB) and its load as the whole percent the
    /// bar shows. The engine's status carries figures that move every time (each worker's CPU and block times): two
    /// that agree here change nothing anyone can see.
    fn status_key(status: &Value) -> Value {
        let mut key = serde_json::Map::new();
        for field in ["audioDevice", "midiInputs", "midiOutputs", "midiPortList", "failures", "heldNotes"] {
            if let Some(v) = status.get(field) {
                key.insert(field.into(), v.clone());
            }
        }
        if let Some(workers) = status["workers"].as_array() {
            let bytes: u64 = workers.iter().filter_map(|w| w["memory"].as_u64()).sum();
            key.insert("workerMemory".into(), Value::from(bytes >> 20));
        }
        if status.get("cpuLoad").is_some() {
            key.insert("cpuLoad".into(), topbar::cpu(status).0.map_or(Value::Null, Value::from));
        }
        Value::Object(key)
    }

    /// A status. One that says what the last said costs nothing (nothing is asked, no screen is touched, no frame
    /// follows); one that changed goes to the screens, and its meters follow at most every METER_INTERVAL.
    fn status(&mut self, status: &Value) {
        let key = App::status_key(status);
        if self.last_status.as_ref() != Some(&key) {
            if self.last_status.is_none() {
                self.ask(Request::Record, "record.get"); // the first status of a connection: a take may be running already
            }
            let mut other = key.clone();
            if let Some(o) = other.as_object_mut() {
                o.remove("cpuLoad");
            }
            if self.last_other.as_ref() != Some(&other) {
                // Something besides the load: memory and the tempo are read again, and a sound may follow (keys held).
                self.last_other = Some(other);
                self.state_stale = true;
                self.wake_meters();
            }
            self.last_status = Some(key);
            self.on_part(|p| p.endpoints(status)); // the inputs, whenever they change
            self.on_play(|p| p.status(status)); // and whether a MIDI device is connected, for Play's hint
            self.on_parts(|p| p.status(status)); // and the devices and failures the Parts list says
            self.on_mixer(|m| m.status(status));
            self.on_chain(|c| c.status(status));
            self.on_routing(|r| r.status(status));
            if self.screen == Screen::Settings {
                self.on_settings(|s| s.status(status));
            }
            if self.on_piano(|p| !p.status(status)) == Some(true) {
                self.close_piano(); // its input was unplugged
            }
            self.pending_status = Some(status.clone());
        }
        self.flush_status();
    }

    /// The meters of the status that changed: the CPU from it, RAM and the tempo asked; a read the engine refused
    /// as busy asked again. Waits out METER_INTERVAL since the last, on a timer, as no later status may come.
    fn flush_status(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_meters.filter(|t| now.duration_since(*t) < METER_INTERVAL) {
            if self.pending_status.is_some() {
                self.flush.start(slint::TimerMode::SingleShot, METER_INTERVAL - now.duration_since(last), || with_app(|a| a.flush_status()));
            }
            return;
        }
        if self.stale {
            self.ask_performance();
        }
        let Some(status) = self.pending_status.take() else { return };
        self.last_meters = Some(now);
        let device = status["audioDevice"].as_str().unwrap_or_default().to_string();
        self.show(|s| {
            s.cpu = topbar::cpu(&status);
            s.audio_device = device;
        });
        if std::mem::take(&mut self.state_stale) {
            self.ask(Request::Memory, "memory.state");
            self.ask(Request::Transport, "transport.get");
        }
    }

    fn reply(&mut self, request: Request, result: Result<Value, Error>) {
        // A long request of this UI's own that the engine ran to its end: it was not busy with another client's then (the
        // `engine.busy` that says so may not have come yet, or at all).
        if result.is_ok() && request.expected() {
            self.wait.forget_busy();
        }
        self.answered(request, result);
        self.refresh_wait();
    }

    fn answered(&mut self, request: Request, result: Result<Value, Error>) {
        if let (true, Ok(answer)) = (is_meters(&request), &result) {
            let signal = answer["meters"].as_array().is_some_and(|m| m.iter().any(|x| x["peak"].as_f64().unwrap_or(0.) > 0.001));
            self.meters_answered(signal);
        }
        let request = match request {
            Request::Setlist(r) => return self.on_setlist(|s| s.reply(r, result)),
            Request::Play(r) => {
                let mut again = false;
                self.on_play(|p| again = p.reply(r, result));
                if again {
                    self.ask_performance();
                }
                return;
            }
            Request::Part(r) => {
                if self.on_part(|p| p.reply(r, result)) {
                    self.ask_performance();
                }
                return;
            }
            Request::Mixer(r) => {
                if self.on_mixer(|m| m.reply(r, result)) {
                    self.ask_performance();
                    self.stale = true;
                }
                return;
            }
            Request::Routing(r) => {
                if self.on_routing(|x| x.reply(r, result)) {
                    self.ask_performance();
                    self.stale = true;
                }
                return;
            }
            Request::Controls(r) => {
                if self.on_controls(|c| c.reply(r, result)) {
                    self.ask_performance();
                    self.stale = true;
                }
                return;
            }
            Request::Parts(r) => {
                if self.on_parts(|p| p.reply(r, result)) {
                    self.ask_performance();
                }
                return;
            }
            Request::Chain(r) => {
                if self.on_chain(|c| c.reply(r, result)) {
                    self.ask_performance();
                }
                return;
            }
            Request::Sounds(r) => {
                if self.on_sounds(|c| c.reply(r, result)) {
                    self.ask_performance();
                }
                return;
            }
            Request::Params(r) => {
                // A value this UI set is an edit the engine can undo, and the engine does not always say so with a
                // params.changed of its own: what undo names is asked when the set is answered.
                let set = matches!(r, params::Request::Set) && result.is_ok();
                if self.on_params(|c| c.reply(r, result)) {
                    self.ask_performance();
                }
                if set && has_actions(self.screen) {
                    self.ask(Request::History, "edit.history");
                }
                return;
            }
            Request::Scene(r) => {
                if self.on_scene(|c| c.reply(r, result)) {
                    self.ask_performance();
                }
                return;
            }
            Request::SettingsApp(call) => {
                self.on_settings(|s| s.reply(call, result));
                return;
            }
            Request::LibraryApp(r) => {
                let plugins = matches!(r, library::Request::Plugins);
                if self.on_library(|l| l.reply(r, result)) {
                    self.ask_performance();
                }
                if plugins {
                    let n = self.library.plugins();
                    self.show(|s| s.plugins = n); // Home's line counts them
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
                    Request::Save if e.code == BUSY => self.say(&e.message),
                    Request::Save if e.code != 0 => self.say(&format!("Not saved: {}", e.message)),
                    Request::RecordDo(what) if e.code != 0 => {
                        self.say(&format!("{}: {}", if what == "capture" { "Not captured" } else { "Not recorded" }, e.message));
                        self.ask(Request::Record, "record.get");
                    }
                    Request::History if e.code != 0 => self.say(&crate::client::refusal(&e)),
                    Request::Reopen if e.code != 0 => eprintln!("oscairn-ui: performance.reopen: {}", e.message),
                    Request::Shell if e.code != 0 => eprintln!("oscairn-ui: memory.shell: {}", e.message),
                    _ => {}
                }
                return;
            }
        };
        match request {
            Request::History => {
                self.history = topbar::History::read(&r);
                self.show_bar();
                if std::mem::take(&mut self.history_again) {
                    self.ask(Request::History, "edit.history");
                }
            }
            Request::Save => {
                self.say("Performance saved.");
                self.ask_performance();
            }
            Request::Info => {
                self.performance_id = r["id"].as_str().unwrap_or_default().to_string();
                self.performance_name = r["name"].as_str().unwrap_or_default().to_string();
                self.setlist.open_performance(&self.performance_id);
                let name = self.performance_name.clone();
                self.on_parts(|p| p.performance(&name));
                self.follow();
                self.show(|s| s.performance = topbar::performance(&r))
            }
            Request::Keys => {
                if self.on_piano(|p| !p.status(&r)) == Some(true) {
                    self.close_piano();
                }
            }
            Request::Graph => {
                self.last_graph = r.clone();
                self.library.graph(&r); // the plugins the Performance uses, for the Libraries menu
                self.on_piano(|p| p.graph(&r));
                self.on_play(|p| p.graph(&r));
                self.on_part(|p| p.graph(&r));
                self.on_parts(|p| p.graph(&r));
                self.on_mixer(|m| m.graph(&r));
                self.on_controls(|c| c.graph(&r));
                self.on_routing(|x| x.graph(&r));
                self.on_chain(|c| c.graph(&r));
                self.on_sounds(|c| c.graph(&r));
                self.on_scene(|c| c.graph(&r));
                self.show(|s| s.scene = topbar::scene(&r))
            }
            Request::Transport => self.show(|s| s.tempo = topbar::tempo(&r)),
            Request::Record => {
                self.retro = r["retro"]["seconds"].as_f64().unwrap_or(0.);
                self.show(|s| s.recording = topbar::recording(&r));
                self.show_bar();
            }
            Request::RecordDo(what) => {
                if what == "capture" {
                    self.say("Captured to Recordings");
                }
                self.ask(Request::Record, "record.get");
            }
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
            Request::Subscribe | Request::Shell | Request::View | Request::Notes | Request::Setlist(_) | Request::Play(_) | Request::Part(_) | Request::Parts(_) | Request::Mixer(_) | Request::Controls(_) | Request::Routing(_) | Request::Chain(_) | Request::Sounds(_) | Request::Params(_) | Request::Scene(_) | Request::LibraryApp(_) | Request::SettingsApp(_) => {}
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
        if next.recording != was.recording {
            let rec = next.recording.clone().unwrap_or_default();
            bar.set_recording(next.recording.is_some());
            bar.set_recording_time(rec.time.as_str().into());
            bar.set_recording_behind(rec.behind);
        }
        if next.ram != was.ram {
            bar.set_ram(next.ram.0.unwrap_or(0).into());
            bar.set_ram_known(next.ram.0.is_some());
            bar.set_ram_level(level(next.ram));
        }
        let lines_changed = (&next.performance, &next.audio_device, next.performances, next.sounds, next.plugins) != (&was.performance, &was.audio_device, was.performances, was.sounds, was.plugins);
        let named = (&next.performance, &next.scene) != (&was.performance, &was.scene); // Play's title, and the name is a stop of the bar's cursor
        self.shown = next;
        if named {
            self.show_bar();
        }
        if lines_changed {
            self.show_home();
        }
    }

    /// A tile's line: what the JUCE Launcher says of each app, from what the shell knows.
    fn line(&self, app: &home::App) -> String {
        let s = &self.shown;
        match app.id {
            "oscairn.performance" if !s.performance.is_empty() => format!("{}: play or edit it", s.performance),
            "oscairn.library" => match (s.performances, s.sounds) {
                (Some(p), Some(n)) => library::library_line(s.plugins, p, n),
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

    fn key(&mut self, text: &str) -> bool {
        if self.keyboard.is_none() && self.on_piano(|p| p.typed(text, true)) == Some(true) {
            return true;
        }
        if !self.text_on_screen && self.keyboard.is_some() {
            let outcome = self.keyboard.as_mut().map(|(_, k)| k.typed(text));
            self.keyboard_outcome(outcome);
            return true;
        }
        if text == "\t" && self.keyboard.is_none() && self.piano.is_none() {
            // Tab: the top bar's actions have the cursor, and Tab again gives it back.
            if self.bar_at.take().is_some() {
                self.show_bar();
            } else {
                self.bar_enter(Action::Move { pane: Some(2), by: 0 });
            }
            return true;
        }
        nav::from_key(text).map(|a| self.act(a)).is_some()
    }

    fn act(&mut self, action: Action) {
        if self.keyboard.is_some() {
            return self.keyboard_act(action);
        }
        if self.piano.is_some() {
            return self.piano_act(action);
        }
        if self.question.is_some() {
            return self.notice_act(action);
        }
        if self.menu.is_some() {
            return self.menu_act(action);
        }
        // Cancel is Back, in one step from anywhere on a screen (the bar's cursor, a list's cursor, a value being changed).
        if matches!(action, Action::Cancel(_)) && self.screen != Screen::Home {
            return self.bar_back();
        }
        if self.bar_at.is_some() {
            return self.bar_act(action);
        }
        if self.bar_enter(action) {
            return;
        }
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
                    play::Outcome::Keys => self.open_piano(""),
                    play::Outcome::Dismiss => self.keep_hint_dismissed(),
                    play::Outcome::Stay => {}
                }
            }
            (Screen::Part, _) => self.part_act(|p| p.act(action)),
            (Screen::Parts, _) => self.parts_act(|p| p.act(action)),
            (Screen::Controls, _) => self.controls_act(|c| c.act(action)),
            (Screen::Routing, _) => self.routing_act(|r| r.act(action)),
            (Screen::Mixer, _) => {
                if self.on_mixer(|m| m.act(action)) == mixer::Outcome::Back {
                    self.go(Screen::Parts);
                }
            }
            (Screen::Chain, _) => self.chain_act(|c| c.act(action)),
            (Screen::Sounds, _) => self.sounds_act(|c| c.act(action)),
            (Screen::Scene, _) => self.scene_act(|c| c.act(action)),
            (Screen::Library, _) => self.library_act(|c| c.act(action)),
            (Screen::Settings, _) => self.settings_act(|c| c.act(action)),
            (Screen::Params, _) => self.params_act(|c| c.act(action)),
            (Screen::Setlist, _) => {
                let mut back = false;
                self.on_setlist(|s| back = s.act(action) == setlist::Outcome::Back);
                if back {
                    self.go(Screen::Home);
                }
            }
        }
    }

    /// What an action of the top bar said, in the Performance's name's place for a few seconds.
    fn say(&mut self, text: &str) {
        if let Some(ui) = self.ui() {
            ui.global::<Bar>().set_notice(text.into());
        }
        self.notice.start(slint::TimerMode::SingleShot, Duration::from_secs(4), || {
            with_app(|a| {
                if let Some(ui) = a.ui() {
                    ui.global::<Bar>().set_notice(SharedString::new());
                }
            })
        });
    }

    /// Whether the title is the Performance's name in a field (Play's): a stop of the bar's cursor, Save As New.
    fn pill_shown(&self) -> bool {
        self.screen == Screen::Play && !self.shown.performance.is_empty()
    }

    /// Whether the bar has a Back button: every screen but Home has somewhere to go back to.
    fn back_shown(&self) -> bool {
        self.screen != Screen::Home && !(self.screen == Screen::Opening && self.hands_over)
    }

    /// The stops before the actions: Back, then the Performance's name.
    fn bar_lead(&self) -> usize {
        usize::from(self.back_shown()) + usize::from(self.pill_shown())
    }

    /// The stops the top bar's cursor visits: Back, the Performance's name (Save As New) when it is the
    /// title, then each action, or "More" alone when the bar folds them into it.
    fn bar_stops(&self) -> usize {
        self.bar_lead() + if self.bar_more { 1.min(self.bar_list.len()) } else { self.bar_list.len() }
    }

    /// The parts of the path of what is open, from the outermost: what the bar's title says. Play's is the
    /// Performance's name and scene.
    fn path(&self) -> Vec<String> {
        let part = |s: &mut Vec<String>, name: &str| s.push(if name.is_empty() { "Part".to_string() } else { name.to_string() });
        let ui = self.ui();
        let mut parts: Vec<String> = Vec::new();
        match self.screen {
            Screen::Home => parts.push("Home".into()),
            Screen::Setlist => {
                parts.push("Setlist".into());
                parts.extend(ui.as_ref().map(|u| u.global::<SetlistState>().get_name().to_string()));
            }
            Screen::Opening => parts.push(ui.as_ref().map(|u| u.get_opening().to_string()).unwrap_or_default()),
            Screen::Play => {
                let s = &self.shown;
                parts.push(if s.scene.is_empty() { s.performance.clone() } else { format!("{}  ·  {}", s.performance, s.scene) });
            }
            Screen::Part => {
                part(&mut parts, &self.part.name());
                parts.extend(self.part.menu_name().map(String::from));
            }
            Screen::Chain | Screen::Sounds => {
                // The part, what the screen is, then the level open in it (the list's own title: a plugin, a lane, filters).
                let what = if self.screen == Screen::Chain { "Insert effects" } else { "Sound" };
                if let Some(ui) = &ui {
                    let state = ui.global::<PartState>();
                    part(&mut parts, state.get_name().as_str());
                    parts.push(what.to_string());
                    let title = state.get_title();
                    let level = title.strip_prefix("Sound: ").unwrap_or(title.as_str());
                    if !level.is_empty() && level != what {
                        parts.push(level.to_string());
                    }
                }
            }
            Screen::Scene => {
                if let Some(ui) = &ui {
                    let state = ui.global::<PartState>();
                    parts.push(state.get_name().to_string());
                    parts.push("Scene setup".into());
                }
            }
            Screen::Params => {
                parts.push(self.chain.part_name());
                parts.push("Insert effects".into());
                parts.push(self.params.name().to_string());
            }
            Screen::Parts => parts.push("Parts".into()),
            Screen::Mixer => parts.extend(["Parts", "Mixer"].map(String::from)),
            Screen::Controls => parts.extend(["Parts", "Controls"].map(String::from)),
            Screen::Routing => parts.extend(["Parts", "Routing"].map(String::from)),
            Screen::Library => parts.extend(self.library.path()),
            Screen::Settings => parts.extend(self.settings.path()),
        }
        parts
    }

    /// What the screen shown offers the bar besides undo, redo and save.
    fn screen_tools(&self) -> Vec<topbar::Tool> {
        match self.screen {
            Screen::Setlist => self.setlist.bar_tools(),
            Screen::Routing => self.routing.bar_tools(),
            Screen::Mixer => self.mixer.bar_tools(),
            Screen::Params => self.params.bar_tools(),
            Screen::Library => self.library.bar_tools(),
            Screen::Settings => self.settings.bar_tools(),
            _ => vec![],
        }
    }

    /// The top bar: its title, Back, the actions for the screen shown and the cursor on them, each set only when it changed.
    fn show_bar(&mut self) {
        let learn = (self.screen == Screen::Play && self.play.can_learn()).then(|| self.play.learning());
        let mut list: Vec<topbar::Act> = self.screen_tools().into_iter().enumerate().map(|(i, t)| topbar::Act { what: topbar::Do::Tool(i), label: t.label, enabled: t.enabled, on: t.on }).collect();
        if has_actions(self.screen) {
            let then = topbar::then(self.screen == Screen::Play, matches!(self.screen, Screen::Parts | Screen::Mixer | Screen::Controls | Screen::Routing), learn);
            // The scene's setup needs a scene playing: none is (the engine says 0) after an undone `scenes.add` left none selected.
            let scene = self.last_graph["scenes"].as_array().is_some_and(|s| s.iter().any(|s| s["id"] == self.last_graph["scene"]));
            list.extend(topbar::actions(&self.history, then).into_iter().map(|(what, label, enabled)| topbar::Act { what, label, enabled: enabled && (what != topbar::Do::Scene || scene), on: false }));
        }
        // Record and Capture, on every screen as the JUCE bar has them over every app: icons, and in More as words.
        let recording = self.shown.recording.clone();
        list.push(topbar::Act { what: topbar::Do::Record, label: if recording.is_some() { "Stop recording" } else { "Record" }.into(), enabled: true, on: recording.is_some() });
        list.push(topbar::Act { what: topbar::Do::Capture, label: "Capture".into(), enabled: self.retro > 0., on: false });
        self.bar_list = list;
        let path = topbar::path_options(&self.path());
        let (back, field, home) = (self.back_shown(), self.screen == Screen::Play, self.screen != Screen::Home);
        let Some(ui) = self.ui() else { return };
        self.bar_more = ui.global::<Bar>().get_more() && !self.bar_list.is_empty();
        if self.bar_at.is_some_and(|a| a >= self.bar_stops()) {
            self.bar_at = None;
        }
        let lead = self.bar_lead();
        let more = self.bar_more;
        let rows: Vec<BarAction> = self
            .bar_list
            .iter()
            .enumerate()
            .map(|(i, a)| BarAction {
                label: a.label.as_str().into(),
                enabled: a.enabled,
                cursor: !more && self.bar_at == Some(i + lead),
                on: a.on,
                icon: match a.what {
                    topbar::Do::Undo => 1,
                    topbar::Do::Redo => 2,
                    topbar::Do::Record => 3,
                    topbar::Do::Capture => 4,
                    _ => 0,
                },
            })
            .collect();
        crate::setlist::update(&self.bar_rows, rows);
        ui.global::<crate::Theme>().set_bar_cursor(self.bar_at.is_some());
        let bar = ui.global::<Bar>();
        if self.shown_path != path {
            bar.set_path(slint::ModelRc::new(VecModel::from(path.iter().map(|p| SharedString::from(p.as_str())).collect::<Vec<_>>())));
            self.shown_path = path;
        }
        if bar.get_back() != back {
            bar.set_back(back);
        }
        if bar.get_title_field() != field {
            bar.set_title_field(field);
        }
        if bar.get_home_button() != home {
            bar.set_home_button(home);
        }
        bar.set_more_cursor(more && self.bar_at == Some(lead));
        let on_back = back && self.bar_at == Some(0);
        if bar.get_back_cursor() != on_back {
            bar.set_back_cursor(on_back);
        }
        let on_pill = self.pill_shown() && self.bar_at == Some(usize::from(back));
        if bar.get_pill_cursor() != on_pill {
            bar.set_pill_cursor(on_pill);
        }
    }

    /// Does what stop `stop` of the bar does.
    fn bar_do(&mut self, stop: usize) {
        let back = usize::from(self.back_shown());
        if stop < back {
            return self.go_back();
        }
        if self.pill_shown() && stop == back {
            return self.save(true);
        }
        let lead = self.bar_lead();
        if self.bar_more {
            // "More" is the one stop: a menu of the actions, those that cannot be done now dimmed.
            let items = self.bar_list.iter().enumerate().map(|(i, a)| menu::Item { enabled: a.enabled, ..menu::Item::choice(a.label.as_str(), i) }).collect();
            self.bar_menu = true;
            return self.open_menu(self.screen, "More", items);
        }
        let Some(act) = self.bar_list.get(stop - lead).cloned() else { return };
        if !act.enabled {
            return;
        }
        self.bar_run(act.what);
    }

    /// Does an action of the top bar.
    fn bar_run(&mut self, what: topbar::Do) {
        match what {
            topbar::Do::Undo | topbar::Do::Redo => {
                let method = if what == topbar::Do::Undo { "edit.undo" } else { "edit.redo" };
                self.send(Request::History, method, json!({}));
                // The values of a plugin page shown are read again behind it (in order on the same connection).
                if self.screen == Screen::Params {
                    self.on_params(|p| p.values_again());
                }
            }
            topbar::Do::Save => self.save(false),
            topbar::Do::Record => match self.shown.recording.as_ref().map(|r| r.take) {
                Some(take) => self.send(Request::RecordDo("stop"), "record.stop", json!({"take": take})),
                None => self.send(Request::RecordDo("start"), "record.start", json!({})),
            },
            topbar::Do::Capture => {
                // The last 30 seconds at most, or what the buffer keeps; named for the Performance, as the reference does.
                let name = if self.performance_name.is_empty() { "Untitled".to_string() } else { self.performance_name.clone() };
                let seconds = self.retro.min(30.);
                self.send(Request::RecordDo("capture"), "record.start", json!({"name": format!("{name} capture"), "from": {"secondsAgo": seconds}}));
            }
            topbar::Do::Learn => self.on_play(|p| p.learn_toggle()),
            topbar::Do::Edit => self.go(Screen::Parts),
            topbar::Do::Play => self.go(Screen::Play),
            topbar::Do::Tool(i) => self.run_tool(i),
            topbar::Do::Scene => {
                if let Some(id) = self.last_graph["scene"].as_u64() {
                    self.scene.open(id);
                    self.go(Screen::Scene);
                }
            }
        }
    }

    /// The screen's own action `i` of the bar.
    fn run_tool(&mut self, i: usize) {
        match self.screen {
            Screen::Setlist => self.go(Screen::Play),
            Screen::Routing => self.on_routing(|r| r.tap_tool(i as i32)),
            Screen::Mixer => self.on_mixer(|m| m.bar_tool(i)),
            Screen::Params => self.params_act(|p| p.search_tap()),
            Screen::Library => self.library_act(|l| l.tool(i)),
            Screen::Settings => self.settings_act(|s| s.tool(i)),
            _ => {}
        }
    }

    fn bar_tap(&mut self, i: usize) {
        self.bar_at = None;
        let lead = self.bar_lead();
        self.bar_do(i + lead);
        self.show_bar();
    }

    fn bar_pill(&mut self) {
        self.bar_at = None;
        if self.pill_shown() {
            self.save(true);
        }
        self.show_bar();
    }

    /// Back on the bar, by touch or the dial: what the device's back button does too.
    fn bar_back(&mut self) {
        self.bar_at = None;
        self.go_back();
        self.show_bar();
    }

    /// Save to the library, as the JUCE app does (LibraryPane.cpp, savePerformance): a Performance
    /// already in the library is saved over; else, or as new, the name is asked for on the keyboard.
    fn save(&mut self, as_new: bool) {
        if !as_new && !self.performance_id.is_empty() {
            return self.send(Request::Save, "library.save", json!({}));
        }
        self.saving = Some(as_new);
        let title = if as_new { "Save As New: the Performance's name" } else { "Save: the Performance's name" };
        let name = self.performance_name.clone();
        self.open_keyboard(self.screen, title, &name);
    }

    /// An action while the top bar has the cursor: it moves along the stops (leaving past the
    /// last, back to the screen), Confirm does what the stop does, Cancel gives the cursor back.
    fn bar_act(&mut self, action: Action) {
        let stops = self.bar_stops() as i64;
        let at = self.bar_at.unwrap_or(0) as i64;
        let to = |by: i64| {
            let next = at + by;
            (0..stops).contains(&next).then_some(next as usize)
        };
        match action {
            Action::Move { by, .. } => self.bar_at = if by < 0 { to(by.into()).or(Some(0)) } else { to(by.into()) },
            Action::Arrow { dx, dy } => {
                let by = i64::from((dx + dy).signum());
                self.bar_at = if by < 0 { to(by).or(Some(0)) } else { to(by) };
            }
            Action::End(by) => self.bar_at = Some(if by < 0 { 0 } else { stops as usize - 1 }),
            Action::Page(_) => {}
            Action::Confirm(_) => {
                if let Some(at) = self.bar_at {
                    self.bar_do(at);
                }
            }
            // Back in one step, as everywhere on a screen: the bar's Back and the device's back button do the same.
            Action::Cancel(_) => {
                self.bar_back();
                return;
            }
        }
        self.show_bar();
    }

    /// The cursor onto the top bar's actions, at the first stop or the last, when something asks
    /// for it: the third list pane (`move:2`), the Tab key, or the dial moving up out of the top
    /// of the screen's list. True when it did.
    fn bar_enter(&mut self, action: Action) -> bool {
        if self.bar_stops() == 0 {
            return false;
        }
        let first = match action {
            Action::Move { pane: Some(2..), by } => by >= 0,
            Action::Move { pane: None | Some(0), by } if by < 0 && self.at_top() => false,
            _ => return false,
        };
        self.bar_at = Some(if first { 0 } else { self.bar_stops() - 1 });
        self.show_bar();
        true
    }

    /// The screen's cursor is on its first row: moving up leaves for the bar.
    fn at_top(&self) -> bool {
        match self.screen {
            Screen::Play => self.play.at_top(),
            Screen::Part => self.part.at_top(),
            Screen::Parts => self.parts.at_top(),
            Screen::Mixer => self.mixer.at_top(),
            Screen::Controls => self.controls.at_top(),
            Screen::Routing => self.routing.at_top(),
            Screen::Chain => self.chain.at_top(),
            Screen::Sounds => self.sounds.at_top(),
            Screen::Scene => self.scene.at_top(),
            Screen::Params => self.params.at_top(),
            Screen::Library => self.library.at_top(),
            Screen::Settings => self.settings.at_top(),
            Screen::Setlist => self.setlist.at_top(),
            _ => false,
        }
    }

    /// Play's hint dismissed: kept in the JUCE app's keyboard.txt, so it stays so in either UI.
    fn keep_hint_dismissed(&mut self) {
        if let Err(e) = crate::settings::dismiss_keyboard_hint() {
            eprintln!("oscairn-ui: cannot keep the hint dismissed: {e}");
        }
    }

    /// A tap on one of Play's hint links.
    fn hint_tap(&mut self, link: i32) {
        let mut outcome = play::Outcome::Stay;
        self.on_play(|p| outcome = p.tap_hint(link));
        match outcome {
            play::Outcome::Keys => self.open_piano(""),
            play::Outcome::Dismiss => self.keep_hint_dismissed(),
            _ => {}
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

    /// Whether `screen` shows lists, so a device's navigate controls navigate: Home's tiles and the Setlist's
    /// are; not where the knobs play what is shown (Play's knob row, a parameter page: its tiles by priority),
    /// and a JUCE screen opening says its own once it runs.
    fn shows_lists(&self, screen: Screen) -> bool {
        screen != Screen::Opening && screen != Screen::Params && !(screen == Screen::Play && self.play.plays_row())
    }

    /// Tells the engine whether the screen shows lists.
    fn send_view(&mut self) {
        let lists = self.shows_lists(self.screen);
        self.send(Request::View, "controllers.view", json!({"lists": lists}));
    }

    /// Something for the Setlist screen, then what it asks the engine sent, and a dropdown's list it has ready opened.
    fn on_setlist(&mut self, f: impl FnOnce(&mut setlist::Screen)) {
        f(&mut self.setlist);
        for (request, method, params) in std::mem::take(&mut self.setlist.out) {
            self.send(Request::Setlist(request), method, params);
        }
        if let Some((title, items, at)) = self.setlist.take_menu() {
            self.open_menu(Screen::Setlist, &title, items);
            if let Some((_, m)) = self.menu.as_mut() {
                m.start(at);
            }
        }
        // What it says (why a slot did not open) goes where the bar's actions say theirs, on the screens that step it.
        if let Some(text) = self.setlist.said.take().filter(|_| matches!(self.screen, Screen::Setlist | Screen::Play)) {
            self.say(&text);
        }
        if self.setlist.take_retry() {
            slint::Timer::single_shot(crate::client::BUSY_RETRY, || with_app(|a| a.on_setlist(setlist::Screen::retry)));
        }
        self.follow();
        self.refresh_wait();
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
        if self.screen == Screen::Part {
            self.show_bar();
        }
        r
    }

    /// A navigation action or tap for the Part screen, and where it takes the shell.
    fn part_act(&mut self, f: impl FnOnce(&mut part::Screen) -> part::Outcome) {
        match self.on_part(f) {
            part::Outcome::Back => self.go(self.part_from),
            part::Outcome::Prompt(title, text) => self.open_keyboard(Screen::Part, &title, &text),
            part::Outcome::Chain => {
                self.chain.open(self.part.id());
                self.start_scan();
                self.go(Screen::Chain);
            }
            part::Outcome::Keys(input) => self.open_piano(&input),
            part::Outcome::Sound => {
                self.sounds.open(self.part.id());
                self.go(Screen::Sounds);
            }
            part::Outcome::Stay => {}
        }
    }

    /// Something for the Parts list, then what it asks the engine sent.
    fn on_parts<R>(&mut self, f: impl FnOnce(&mut parts::Screen) -> R) -> R {
        let r = f(&mut self.parts);
        for (request, method, params) in std::mem::take(&mut self.parts.out) {
            self.send(Request::Parts(request), method, params);
        }
        if self.screen == Screen::Parts {
            self.show_bar();
        }
        r
    }

    /// Something for the Mixer, then what it asks the engine sent.
    fn on_mixer<R>(&mut self, f: impl FnOnce(&mut mixer::Screen) -> R) -> R {
        let r = f(&mut self.mixer);
        for (request, method, params) in std::mem::take(&mut self.mixer.out) {
            self.send(Request::Mixer(request), method, params);
        }
        if self.screen == Screen::Mixer {
            self.show_bar();
        }
        r
    }

    /// Something for the Controls page, then what it asks the engine sent, and a menu it has ready opened.
    fn on_controls<R>(&mut self, f: impl FnOnce(&mut controls::Screen) -> R) -> R {
        let r = f(&mut self.controls);
        for (request, method, params) in std::mem::take(&mut self.controls.out) {
            self.send(Request::Controls(request), method, params);
        }
        if let Some((title, items)) = self.controls.take_menu() {
            self.open_menu(Screen::Controls, &title, items);
        }
        r
    }

    /// Something for Routing, then what it asks the engine sent, a menu it has ready opened, and the wires'
    /// meters' timer started or stopped as Meters is switched.
    fn on_routing<R>(&mut self, f: impl FnOnce(&mut routing::Screen) -> R) -> R {
        let was = self.routing.metering();
        let r = f(&mut self.routing);
        for (request, method, params) in std::mem::take(&mut self.routing.out) {
            self.send(Request::Routing(request), method, params);
        }
        if let Some((title, items)) = self.routing.take_menu() {
            self.open_menu(Screen::Routing, &title, items);
        }
        if self.screen == Screen::Routing {
            // Only the meters' toggle starts and stops the polling here: a start on every call would wake the
            // silent meters again at every changed status (the answers that say what the last said), and the shell
            // would never rest. Entering the screen starts them in `go`.
            if self.routing.metering() && !was && !self.meters.running() {
                self.start_meters(routing::METER_INTERVAL);
            } else if !self.routing.metering() && self.meters.running() {
                self.meters.stop();
            }
        }
        if self.screen == Screen::Routing {
            self.show_bar();
        }
        r
    }

    /// A navigation action or tap for Routing, and where it takes the shell.
    fn routing_act(&mut self, f: impl FnOnce(&mut routing::Screen) -> routing::Outcome) {
        match self.on_routing(f) {
            routing::Outcome::Back => self.go(Screen::Parts),
            routing::Outcome::OpenPart(id) => self.open_part(id),
            routing::Outcome::Keys(input) => self.open_piano(&input),
            routing::Outcome::Controls => self.go(Screen::Controls),
            routing::Outcome::Stay => {}
        }
    }

    /// A navigation action or tap for the Controls page, and where it takes the shell.
    fn controls_act(&mut self, f: impl FnOnce(&mut controls::Screen) -> controls::Outcome) {
        match self.on_controls(f) {
            controls::Outcome::Back => self.go(Screen::Parts),
            controls::Outcome::Prompt(title, text) => self.open_keyboard(Screen::Controls, &title, &text),
            controls::Outcome::Stay => {}
        }
    }

    /// A menu over the screen `screen`, which gets what is chosen in it.
    fn open_menu(&mut self, screen: Screen, title: &str, items: Vec<menu::Item>) {
        if let Some(ui) = self.ui() {
            self.menu = Some((screen, menu::Menu::open(&ui, title, items)));
        }
    }

    fn close_menu(&mut self) {
        self.bar_menu = false;
        if let Some((_, mut m)) = self.menu.take() {
            m.close();
        }
    }

    fn menu_act(&mut self, action: Action) {
        let outcome = self.menu.as_mut().map(|(_, m)| m.act(action));
        self.menu_outcome(outcome);
    }

    fn menu_tap(&mut self, row: i32) {
        let outcome = self.menu.as_mut().map(|(_, m)| m.tap(row));
        self.menu_outcome(outcome);
    }

    fn menu_back(&mut self) {
        let outcome = self.menu.as_mut().map(|(_, m)| m.back());
        self.menu_outcome(outcome);
    }

    fn menu_turn(&mut self, by: i32) {
        if let Some((_, m)) = self.menu.as_mut() {
            m.turn(by);
        }
    }

    fn menu_shown(&mut self) {
        if let Some((_, m)) = self.menu.as_mut() {
            m.show();
        }
    }

    /// What a menu came to: a choice goes to the screen that opened it, a close drops it.
    fn menu_outcome(&mut self, outcome: Option<menu::Outcome>) {
        match outcome {
            Some(menu::Outcome::Choose(id)) => {
                let screen = self.menu.take().map(|(s, _)| s);
                if std::mem::take(&mut self.bar_menu) {
                    if let Some(topbar::Act { what, enabled: true, .. }) = self.bar_list.get(id).cloned() {
                        self.bar_run(what);
                    }
                    self.show_bar();
                    return;
                }
                if screen == Some(Screen::Setlist) {
                    self.on_setlist(|s| s.chosen(id));
                }
                if screen == Some(Screen::Controls) {
                    self.on_controls(|c| c.chosen(id));
                }
                if screen == Some(Screen::Routing) {
                    self.on_routing(|r| r.chosen(id));
                }
                if screen == Some(Screen::Chain) {
                    self.on_chain(|c| c.chosen(id));
                }
                if screen == Some(Screen::Library) {
                    self.library_act(|l| l.chosen(id));
                }
                if screen == Some(Screen::Settings) {
                    self.settings_act(|s| {
                        s.chosen(id);
                        settings_app::Outcome::Stay
                    });
                }
            }
            Some(menu::Outcome::Close) => {
                self.menu = None;
                self.bar_menu = false;
            }
            _ => {}
        }
    }

    /// A navigation action or tap for the Parts list, and where it takes the shell.
    fn parts_act(&mut self, f: impl FnOnce(&mut parts::Screen) -> parts::Outcome) {
        match self.on_parts(f) {
            parts::Outcome::Back => self.go(Screen::Play),
            parts::Outcome::Open(id) => self.open_part(id),
            parts::Outcome::Routing(node) => {
                self.routing.opened();
                self.routing.choose_node(node);
                self.start_scan();
                self.go(Screen::Routing);
            }
            parts::Outcome::Page(parts::Page::Mixer) => self.go(Screen::Mixer),
            parts::Outcome::Page(parts::Page::Controls) => self.go(Screen::Controls),
            parts::Outcome::Page(parts::Page::Routing) => {
                self.routing.opened();
                self.start_scan();
                self.go(Screen::Routing);
            }
            parts::Outcome::Stay => {}
        }
    }

    /// Something for the playing keyboard if it is open, then the notes it played sent.
    fn on_piano<R>(&mut self, f: impl FnOnce(&mut piano::Piano) -> R) -> Option<R> {
        let r = f(self.piano.as_mut()?);
        let out = std::mem::take(&mut self.piano.as_mut()?.out);
        for (method, params) in out {
            self.send(Request::Notes, method, params);
        }
        if self.piano.as_ref().is_some_and(|p| p.tapping()) && !self.piano_tap.running() {
            self.piano_tap.start(slint::TimerMode::SingleShot, piano::TAP, || with_app(|a| a.piano_tap_end()));
        }
        Some(r)
    }

    fn piano_tap_end(&mut self) {
        self.on_piano(piano::Piano::tap_end);
    }

    /// The playing keyboard over the current screen, for `input` (empty: the built-in keys).
    fn open_piano(&mut self, input: &str) {
        let Some(ui) = self.ui() else { return };
        if let Some(mut old) = self.piano.take() {
            old.close();
        }
        let mut piano = piano::Piano::open(&ui, input, &Value::Null, &self.last_graph);
        let out = std::mem::take(&mut piano.out);
        self.piano = Some(piano);
        for (method, params) in out {
            self.send(Request::Notes, method, params);
        }
        self.send(Request::Keys, "session.status", json!({}));
    }

    fn close_piano(&mut self) {
        if self.piano.is_some() {
            self.on_piano(piano::Piano::release);
        }
        if let Some(mut p) = self.piano.take() {
            p.close();
        }
        self.piano_tap.stop();
    }

    fn piano_act(&mut self, action: Action) {
        if self.on_piano(|p| p.act(action)) == Some(piano::Outcome::Close) {
            self.close_piano();
        }
    }

    fn piano_control(&mut self, n: usize) {
        if self.on_piano(|p| p.tap_control(n)) == Some(piano::Outcome::Close) {
            self.close_piano();
        }
    }

    /// Something for the chain, then what it asks the engine sent.
    fn on_chain<R>(&mut self, f: impl FnOnce(&mut chain::Screen) -> R) -> R {
        let r = f(&mut self.chain);
        for (request, method, params) in std::mem::take(&mut self.chain.out) {
            self.send(Request::Chain(request), method, params);
        }
        if let Some((title, items)) = self.chain.take_menu() {
            self.open_menu(Screen::Chain, &title, items);
        }
        if self.screen == Screen::Chain {
            self.show_bar();
        }
        r
    }

    fn on_sounds<R>(&mut self, f: impl FnOnce(&mut sounds::Screen) -> R) -> R {
        let r = f(&mut self.sounds);
        for (request, method, params) in std::mem::take(&mut self.sounds.out) {
            self.send(Request::Sounds(request), method, params);
        }
        if self.screen == Screen::Sounds {
            self.show_bar();
        }
        r
    }

    fn on_params<R>(&mut self, f: impl FnOnce(&mut params::Screen) -> R) -> R {
        let r = f(&mut self.params);
        for (request, method, params) in std::mem::take(&mut self.params.out) {
            self.send(Request::Params(request), method, params);
        }
        if self.screen == Screen::Params {
            self.show_bar();
        }
        r
    }

    fn chain_act(&mut self, f: impl FnOnce(&mut chain::Screen) -> chain::Outcome) {
        match self.on_chain(f) {
            chain::Outcome::Back => self.go(Screen::Part),
            chain::Outcome::Params(node, name) => {
                self.params.open(node, &name);
                let shared = self.last_graph["nodes"].as_array().is_some_and(|n| n.iter().any(|n| n["id"].as_u64() == Some(node) && n["shared"].is_object()));
                self.params.set_shared(shared);
                self.go(Screen::Params);
            }
            chain::Outcome::Routing(node) => {
                self.routing.opened();
                self.routing.choose_node(node);
                self.start_scan();
                self.go(Screen::Routing);
            }
            chain::Outcome::Stay => {}
        }
    }

    fn sounds_act(&mut self, f: impl FnOnce(&mut sounds::Screen) -> sounds::Outcome) {
        match self.on_sounds(f) {
            sounds::Outcome::Back => self.go(Screen::Part),
            sounds::Outcome::Prompt(title, text) => self.open_keyboard(Screen::Sounds, &title, &text),
            sounds::Outcome::Stay => {}
        }
    }

    fn on_scene<R>(&mut self, f: impl FnOnce(&mut scene::Screen) -> R) -> R {
        let r = f(&mut self.scene);
        for (request, method, params) in std::mem::take(&mut self.scene.out) {
            self.send(Request::Scene(request), method, params);
        }
        if self.screen == Screen::Scene {
            self.show_bar();
        }
        r
    }

    fn scene_act(&mut self, f: impl FnOnce(&mut scene::Screen) -> scene::Outcome) {
        match self.on_scene(f) {
            scene::Outcome::Back => self.go(Screen::Play),
            scene::Outcome::Prompt(title, text) => self.open_keyboard(Screen::Scene, &title, &text),
            scene::Outcome::Stay => {}
        }
    }

    fn params_act(&mut self, f: impl FnOnce(&mut params::Screen) -> params::Outcome) {
        match self.on_params(f) {
            params::Outcome::Back => self.go(Screen::Chain),
            params::Outcome::Prompt(title, text) => self.open_keyboard(Screen::Params, &title, &text),
            params::Outcome::Stay => {}
        }
    }

    /// The list screen shown, laid out again for the rows its window now holds.
    fn refit(&mut self) {
        match self.screen {
            Screen::Part => self.on_part(part::Screen::shown),
            Screen::Chain => self.on_chain(chain::Screen::shown),
            Screen::Sounds => self.on_sounds(sounds::Screen::shown),
            Screen::Scene => self.on_scene(scene::Screen::shown),
            Screen::Mixer => self.on_mixer(mixer::Screen::shown),
            Screen::Controls => self.on_controls(controls::Screen::shown),
            Screen::Routing => self.on_routing(routing::Screen::shown),
            Screen::Library => self.on_library(library::Screen::show),
            Screen::Settings => self.on_settings(settings_app::Screen::show),
            _ => {}
        }
    }

    /// Back, from anywhere on a screen, whatever its cursor or a value being changed is doing: one step up the
    /// path the bar's title shows. The bar's Back button, the Escape key and a device's back button (the profile's
    /// panel map says which control is `cancel`) all come here. A screen's own list (the Part's devices, a
    /// dropdown, a chain's lane) closes or goes up first; a menu or a keyboard over the screen is closed by
    /// `act` before it gets here.
    fn go_back(&mut self) {
        match self.screen {
            Screen::Home => {}
            Screen::Opening => {
                if !self.hands_over {
                    self.go(Screen::Home);
                }
            }
            Screen::Setlist => self.go(Screen::Home),
            Screen::Play => self.go(self.play_from),
            Screen::Part => self.part_act(|p| p.back()),
            Screen::Parts => self.go(Screen::Play),
            Screen::Mixer | Screen::Controls | Screen::Routing => self.go(Screen::Parts),
            Screen::Chain => self.chain_act(|c| c.back()),
            Screen::Sounds => self.sounds_act(|c| c.back()),
            Screen::Scene => self.go(Screen::Play),
            Screen::Library => self.library_act(|l| l.back()),
            Screen::Settings => self.settings_act(|s| s.back()),
            Screen::Params => self.params_act(|p| p.back()),
        }
    }

    fn list_tap(&mut self, i: i32) {
        match self.screen {
            Screen::Part => self.part_act(|p| p.tap(i)),
            Screen::Chain => self.chain_act(|c| c.tap(i)),
            Screen::Sounds => self.sounds_act(|c| c.tap(i)),
            Screen::Scene => self.scene_act(|c| c.tap(i)),
            Screen::Library => self.library_act(|c| c.tap(i)),
            Screen::Settings => self.settings_act(|c| c.tap(i)),
            _ => {}
        }
    }

    fn list_step(&mut self, i: i32, by: i32) {
        match self.screen {
            Screen::Part => self.on_part(|p| p.step_row(i, by)),
            Screen::Chain => self.on_chain(|c| c.step_row(i, by)),
            Screen::Sounds => self.on_sounds(|c| c.step_row(i, by)),
            Screen::Scene => self.on_scene(|c| c.step_row(i, by)),
            Screen::Library => self.on_library(|c| c.step_row(i, by)),
            Screen::Settings => self.on_settings(|c| c.step_row(i, by)),
            _ => {}
        }
    }

    /// The scroll bar's request: the list's view starts at row `first`.
    fn list_scroll_to(&mut self, first: i32) {
        match self.screen {
            Screen::Chain => self.on_chain(|c| c.scroll_to(first)),
            Screen::Sounds => self.on_sounds(|c| c.scroll_to(first)),
            Screen::Scene => self.on_scene(|c| c.scroll_to(first)),
            Screen::Library => self.on_library(|c| c.scroll_to(first)),
            Screen::Settings => self.on_settings(|c| c.scroll_to(first)),
            _ => {}
        }
    }

    fn list_scroll(&mut self, by: i32) {
        match self.screen {
            Screen::Part => self.on_part(|p| p.scroll_info(by)),
            Screen::Chain => self.on_chain(|c| c.scroll_info(by)),
            Screen::Sounds => self.on_sounds(|c| c.scroll_info(by)),
            Screen::Scene => self.on_scene(|c| c.scroll_info(by)),
            Screen::Library => self.on_library(|c| c.scroll_info(by)),
            Screen::Settings => self.on_settings(|c| c.scroll_info(by)),
            _ => {}
        }
    }

    /// The plugin scanner, started the first time the chain is opened (it runs the helper on every
    /// plugin file, off the UI thread), its answer handed to the chain.
    fn start_scan(&mut self) {
        if self.scan_started {
            return;
        }
        self.scan_started = true;
        let weak = self.ui.clone();
        let (dir, helper) = (crate::scan::plugin_dir(PLUGIN_DIR.with(|d| d.borrow().clone()).as_deref()), crate::scan::scanner());
        std::thread::spawn(move || {
            let found = crate::scan::scan(&dir, &helper, crate::scan::TIMEOUT);
            let _ = weak.upgrade_in_event_loop(move |_| with_app(|a| { a.on_routing(|r| r.plugins(found.clone())); a.on_chain(|c| c.plugins(found)) }));
        });
    }

    /// The on-screen keyboard over `screen`, which asked for text.
    fn open_keyboard(&mut self, screen: Screen, title: &str, text: &str) {
        if let Some(ui) = self.ui() {
            self.keyboard = Some((screen, keyboard::Keyboard::open(&ui, title, text, self.text_on_screen)));
        }
    }

    /// An action for the keyboard while it is open: Done hands the text to the screen that asked, Cancel closes it.
    fn keyboard_act(&mut self, action: Action) {
        let outcome = self.keyboard.as_mut().map(|(_, k)| k.act(action));
        self.keyboard_outcome(outcome);
    }

    /// A key pressed by touch types at once; Del, Left and Right go on while it is held (450 ms, then every 70 ms).
    fn keyboard_press(&mut self, i: i32) {
        let i = i.max(0) as usize;
        let outcome = self.keyboard.as_mut().map(|(_, k)| k.tap(i));
        let repeats = self.keyboard.as_ref().is_some_and(|(_, k)| k.repeats(i));
        self.keyboard_outcome(outcome);
        if repeats {
            self.repeat.start(slint::TimerMode::SingleShot, Duration::from_millis(450), || with_app(|a| a.keyboard_repeat()));
        }
    }

    fn keyboard_repeat(&mut self) {
        if let Some((_, k)) = self.keyboard.as_mut() {
            k.repeat();
            self.repeat.start(slint::TimerMode::Repeated, Duration::from_millis(70), || with_app(|a| a.keyboard_repeat()));
        }
    }

    fn keyboard_outcome(&mut self, outcome: Option<keyboard::Outcome>) {
        let typed = match outcome {
            None | Some(keyboard::Outcome::Typing) => return,
            Some(keyboard::Outcome::Accept(text)) => Some(text),
            Some(keyboard::Outcome::Cancel) => None,
        };
        let Some((screen, k)) = self.keyboard.take() else { return };
        self.repeat.stop();
        k.close();
        if let Some(as_new) = self.saving.take() {
            // Save's name, the JUCE way: a blank name keeps the Performance's own.
            if let Some(name) = typed.map(|t| t.trim().to_string()) {
                let mut params = json!({"new": as_new});
                if !name.is_empty() {
                    params["name"] = json!(name);
                }
                self.send(Request::Save, "library.save", params);
            }
            return;
        }
        match (screen, typed) {
            (Screen::Part, Some(text)) => self.on_part(|p| p.entered(&text)),
            (Screen::Part, None) => self.on_part(part::Screen::not_entered),
            (Screen::Controls, Some(text)) => self.on_controls(|p| p.entered(&text)),
            (Screen::Controls, None) => self.on_controls(controls::Screen::not_entered),
            (Screen::Sounds, Some(text)) => self.on_sounds(|p| p.entered(&text)),
            (Screen::Sounds, None) => self.on_sounds(sounds::Screen::not_entered),
            (Screen::Scene, Some(text)) => self.on_scene(|p| p.entered(&text)),
            (Screen::Scene, None) => self.on_scene(scene::Screen::not_entered),
            (Screen::Library, Some(text)) => self.on_library(|p| p.entered(&text)),
            (Screen::Library, None) => self.on_library(library::Screen::not_entered),
            (Screen::Settings, Some(text)) => self.on_settings(|p| p.entered(&text)),
            (Screen::Settings, None) => self.on_settings(settings_app::Screen::not_entered),
            (Screen::Params, Some(text)) => self.on_params(|p| p.entered(&text)),
            (Screen::Params, None) => self.on_params(params::Screen::not_entered),
            _ => {}
        }
    }

    /// A part selected in Play: its settings.
    fn open_part(&mut self, id: u32) {
        self.part_from = if matches!(self.screen, Screen::Parts | Screen::Routing) { self.screen } else { Screen::Play };
        self.part.open(id);
        self.go(Screen::Part);
    }

    /// Previous and Next show on Play while the Performance open is the setlist's slot playing.
    fn follow(&mut self) {
        let on = !self.performance_id.is_empty() && self.setlist.playing() == Some(self.performance_id.as_str());
        self.play.follow(on);
    }

    /// Starts asking the meters of the screen shown every `every`, awake.
    fn start_meters(&mut self, every: Duration) {
        self.meter_every = every;
        self.meters_asleep = false;
        self.quiet_since = None;
        self.meters.start(slint::TimerMode::Repeated, every, || with_app(|a| a.poll_meters()));
    }

    /// Whether the screen shown draws meters.
    fn meters_shown(&self) -> bool {
        match self.screen {
            Screen::Play | Screen::Parts | Screen::Mixer => true,
            Screen::Routing => self.routing.metering(),
            _ => false,
        }
    }

    /// Something that may sound or move a meter happened (a key held, a change, a request that plays, the transport):
    /// the meters are asked again, from now, and while the answers carry a signal and for QUIET_AFTER after the last.
    fn wake_meters(&mut self) {
        self.quiet_since = None;
        if self.meters_asleep && self.meters_shown() {
            self.start_meters(self.meter_every);
        }
    }

    /// An answer of the meters: silence for QUIET_AFTER stops the asking (nothing at rest costs a request) until
    /// something wakes it. A transport that plays is never silent: a sequence may sound at any time.
    fn meters_answered(&mut self, signal: bool) {
        if signal || self.playing {
            self.quiet_since = None;
            return;
        }
        let since = *self.quiet_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= QUIET_AFTER && self.meters.running() {
            self.meters.stop();
            self.meters_asleep = true;
        }
    }

    /// The meters, asked unless the last answer is still out: a slow engine never piles them up.
    fn poll_meters(&mut self) {
        if self.screen == Screen::Routing {
            if self.client.waiting(|r| matches!(r, Request::Routing(routing::Request::Meters))).is_none() {
                self.send(Request::Routing(routing::Request::Meters), "wires.meters", json!({}));
            }
        } else if self.screen == Screen::Mixer {
            if self.client.waiting(|r| matches!(r, Request::Mixer(mixer::Request::Meters))).is_none() {
                self.send(Request::Mixer(mixer::Request::Meters), "parts.meters", json!({}));
            }
        } else if self.screen == Screen::Parts {
            if self.client.waiting(|r| matches!(r, Request::Parts(parts::Request::Meters))).is_none() {
                self.send(Request::Parts(parts::Request::Meters), "parts.meters", json!({}));
            }
        } else if self.client.waiting(|r| matches!(r, Request::Play(play::Request::Meters))).is_none() {
            self.send(Request::Play(play::Request::Meters), "parts.meters", json!({}));
        }
    }

    fn go(&mut self, screen: Screen) {
        self.close_piano();
        self.close_menu();
        self.close_notice();
        if let Some((_, k)) = self.keyboard.take() {
            k.close();
            self.saving = None;
            self.on_part(part::Screen::not_entered);
            self.on_sounds(sounds::Screen::not_entered);
            self.on_library(library::Screen::not_entered);
            self.on_settings(settings_app::Screen::not_entered);
        }
        if self.screen != screen {
            if self.screen == Screen::Settings {
                self.on_settings(settings_app::Screen::hidden);
            }
            if self.screen == Screen::Library {
                self.on_library(library::Screen::hidden);
            }
            // A cursor drops when its screen goes, as on focus loss in the JUCE lists.
            self.home.cursor = self.home.cursor.clear();
            self.show_home();
            if self.screen == Screen::Setlist {
                self.on_setlist(setlist::Screen::hidden);
            }
            if self.screen == Screen::Play {
                self.play.hidden();
            }
            if matches!(self.screen, Screen::Play | Screen::Parts | Screen::Mixer) {
                self.meters.stop();
            }
            if self.screen == Screen::Parts {
                self.on_parts(parts::Screen::hidden);
            }
            if self.screen == Screen::Mixer {
                self.on_mixer(mixer::Screen::hidden);
            }
            if self.screen == Screen::Controls {
                self.on_controls(controls::Screen::hidden);
            }
            if self.screen == Screen::Routing {
                self.on_routing(routing::Screen::hidden);
                self.meters.stop();
            }
            if self.screen == Screen::Part {
                self.on_part(part::Screen::hidden);
            }
            if self.screen == Screen::Chain {
                self.on_chain(chain::Screen::hidden);
            }
            if self.screen == Screen::Sounds {
                self.on_sounds(sounds::Screen::hidden);
            }
            if self.screen == Screen::Scene {
                self.on_scene(scene::Screen::hidden);
            }
            if self.screen == Screen::Params {
                self.on_params(params::Screen::hidden);
                self.send(Request::Subscribe, "unsubscribe", json!({"topics": ["params.changed"]}));
            }
            if live(self.screen) && !live(screen) {
                self.send(Request::Subscribe, "unsubscribe", json!({"topics": ["controls.changed"]}));
            }
            if screen == Screen::Play && matches!(self.screen, Screen::Home | Screen::Setlist) {
                self.play_from = self.screen;
            }
        }
        let was = self.screen;
        self.screen = screen;
        self.bar_at = None;
        if self.shows_lists(was) != self.shows_lists(screen) {
            self.send_view();
        }
        if let Some(ui) = self.ui() {
            ui.set_screen(screen as i32);
        }
        if screen == Screen::Setlist {
            self.on_setlist(setlist::Screen::shown);
        }
        if !live(was) && live(screen) {
            // The first controls.changed after subscribing carries every level and mute.
            self.send(Request::Subscribe, "subscribe", json!({"topics": ["controls.changed"]}));
        }
        if screen == Screen::Scene {
            self.on_scene(scene::Screen::shown);
        }
        if matches!(screen, Screen::Chain | Screen::Sounds | Screen::Params | Screen::Scene | Screen::Parts | Screen::Mixer | Screen::Controls | Screen::Routing) {
            self.ask_performance();
        }
        if screen == Screen::Parts {
            self.send(Request::Parts(parts::Request::Status), "session.status", json!({}));
            self.on_parts(parts::Screen::shown);
            self.start_meters(parts::METER_INTERVAL);
        }
        if screen == Screen::Mixer {
            self.on_mixer(mixer::Screen::shown);
            self.start_meters(mixer::METER_INTERVAL);
        }
        if screen == Screen::Controls {
            self.on_controls(controls::Screen::shown);
        }
        if screen == Screen::Routing {
            self.send(Request::Routing(routing::Request::Status), "session.status", json!({}));
            self.on_routing(routing::Screen::shown);
            if self.routing.metering() && !self.meters.running() {
                self.start_meters(routing::METER_INTERVAL);
            }
        }
        if screen == Screen::Chain {
            self.on_chain(chain::Screen::shown);
        }
        if screen == Screen::Library && was != Screen::Library {
            // The Setlist app's own state file, which says which setlist Add to setlist adds to.
            self.library.set_state_file(crate::settings::data_dir().map(|d| d.join("AppData/oscairn.setlist/state.json")));
            self.on_library(library::Screen::open);
            self.on_library(library::Screen::shown);
        }
        if screen == Screen::Settings && was != Screen::Settings {
            self.on_settings(settings_app::Screen::open);
            self.on_settings(settings_app::Screen::shown);
        }
        if screen == Screen::Sounds {
            self.on_sounds(sounds::Screen::shown);
        }
        if screen == Screen::Params {
            self.send(Request::Subscribe, "subscribe", json!({"topics": ["params.changed"]}));
            self.on_params(params::Screen::shown);
        }
        if screen == Screen::Part {
            self.ask_performance();
            self.send(Request::Part(part::Request::Status), "session.status", json!({}));
            self.send(Request::Part(part::Request::Harmony), "harmony.get", json!({}));
            self.on_part(part::Screen::shown);
        }
        if was != Screen::Play && screen == Screen::Play {
            self.ask_performance();
            self.follow();
            self.play.shown();
            self.start_meters(play::METER_INTERVAL);
        }
        self.show_bar();
        if has_actions(screen) {
            self.ask(Request::History, "edit.history");
        }
    }

    /// An app's tile confirmed: a Rust screen shows; a JUCE one gets the display (docs/rust-ui.md).
    fn open(&mut self, app: &'static home::App) {
        match app.id {
            "oscairn.setlist" => self.go(Screen::Setlist),
            "oscairn.performance" => self.go(Screen::Play),
            "oscairn.library" if self.library_in_rust => self.go(Screen::Library),
            "oscairn.settings" if self.settings_in_rust => self.go(Screen::Settings),
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

/// Tests: Play's hint as a profile with no keys of its own has it (the FANTOM-07's has none).
#[cfg(test)]
pub fn offer_keyboard(offered: bool, dismissed: bool) {
    with_app(|a| a.play.offer_keyboard(offered, dismissed))
}

/// Tests: the profile says text is typed on the on-screen keyboard's keys.
#[cfg(test)]
pub fn set_text_on_screen(on: bool) {
    with_app(|a| a.text_on_screen = on)
}

thread_local! {
    /// `--plugins DIR`, where the plugin scanner looks (else beside the program, as the JUCE app).
    static PLUGIN_DIR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Tests: the playing keyboard opened as the Part screen's row does.
#[cfg(test)]
pub fn open_piano(input: &str) {
    with_app(|a| a.open_piano(input))
}

/// Tests: the plugin scanner's answer handed to the screens, as start_scan's thread hands it through
/// the event loop, which the headless tests do not have.
#[cfg(test)]
pub fn plugins_found(found: Vec<crate::scan::Plugin>) {
    with_app(|a| {
        a.on_routing(|r| r.plugins(found.clone()));
        a.on_chain(|c| c.plugins(found))
    })
}

/// The plugin folder the command line gave.
pub fn set_plugin_dir(dir: Option<String>) {
    PLUGIN_DIR.with(|d| *d.borrow_mut() = dir);
}
