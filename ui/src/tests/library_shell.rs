// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library on the shell, through a socket to the fake engine of library_fake.rs: the tile, the bar, the
//! keyboard and the questions over the screen, a load that takes seconds while the screen stays alive, and that
//! nothing at rest costs a request or a frame.

use super::library::{footer, labels, library, rows, tabs, value_of};
use super::library_fake::Library as Fake;
use super::*;
use crate::{KeyboardState, NoticeState, PartState};
use std::time::Duration;

pub(super) struct Shelf {
    pub(super) f: Fixture,
    pub(super) e: Fake,
}

impl Shelf {
    pub(super) fn new(name: &str) -> Shelf {
        let mut f = Fixture::new(name);
        f.engine.answer(); // session.hello
        f.pump();
        let mut e = Fake::new(library());
        e.libraries = vec![("friend".into(), "A Friend's".into())];
        let mut s = Shelf { f, e };
        s.serve();
        s
    }

    /// Answers every request the shell makes until it is quiet for a moment, as a real engine would.
    pub(super) fn serve(&mut self) {
        let mut quiet = 0;
        while quiet < 5 {
            slint::platform::update_timers_and_animations(); // a step asked again later is a timer
            let Some(request) = self.f.engine.try_request(Duration::from_millis(120)) else {
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
}

// Home's tile hands over to the JUCE app until the whole app is built; with the switch on it is the Rust one, with
// the bar's title, Back and the actions of the screen.
#[test]
fn the_tile_opens_the_rust_app_only_when_it_is_switched_on() {
    let mut s = Shelf::new("library-tile");
    let ui = s.ui();
    shell::library_in_rust(false);
    ui.global::<crate::HomeState>().invoke_tapped(1);
    assert_eq!((ui.get_screen(), ui.get_opening().as_str(), shell::handover()), (2, "Library", Some("library")), "the JUCE app, as before");
    shell::goto(shell::Screen::Home);
    s.serve();
    shell::library_in_rust(true);
    ui.global::<crate::HomeState>().invoke_tapped(1);
    s.serve();
    assert_eq!(ui.get_screen(), 13);
    assert_eq!(title(&ui), "Library › Performances");
    assert!(ui.global::<Bar>().get_back() && ui.global::<Bar>().get_home_button());
    let actions: Vec<String> = ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect();
    assert_eq!(actions, ["Search", "Audition", "Options"], "Undo, Redo and Save are the Performance app's");
    assert_eq!(tabs(&ui), ["Performances", "Sounds", "Setlists"]);
    assert_eq!(labels(&ui)[0], "Filters");
    // Back goes Home; the screen's state is dropped with it.
    assert!(ui.invoke_key(slint::SharedString::from(slint::platform::Key::Escape)));
    assert_eq!(ui.get_screen(), 0);
}

// A press opens a Performance while the engine takes its time: every other request is refused as busy and asked
// again, the bar says the engine is busy and the line under the list what is opening, and the screen answers a
// cursor move meanwhile; when it is open the line says so.
#[test]
fn a_load_that_takes_seconds_leaves_the_screen_alive() {
    let mut s = Shelf::new("library-load");
    let ui = s.ui();
    shell::goto(shell::Screen::Library);
    s.serve();
    s.e.calls.clear();
    s.e.busy = 3;
    // The press by touch: row 2, Brass Corner.
    ui.global::<PartState>().invoke_tapped(2);
    // The shell sent the first step; the engine answered busy; the screen is not blocked.
    let step = s.f.engine.request();
    assert_eq!(step["method"], "performance.info");
    s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": step["id"], "error": {"code": -32009, "message": "busy", "data": {"busy": "library.load", "ms": 800}}})));
    s.f.pump();
    assert_eq!(footer(&ui).0, "Opening Brass Corner...");
    assert_eq!(value_of(&ui, "Brass Corner"), "Opening  ·  brass  ·  warm");
    shell::act(Action::Move { pane: None, by: 1 });
    assert!(rows(&ui).iter().any(|r| r.cursor), "a cursor move is answered while it loads");
    // The engine is free again: the step is asked again by the shell's own timer, and the chain runs.
    s.e.calls.clear();
    std::thread::sleep(Duration::from_millis(150));
    slint::platform::update_timers_and_animations();
    s.serve();
    assert!(s.e.methods().contains(&"library.load".to_string()), "{:?}", s.e.methods());
    assert_eq!(footer(&ui).0, "Performance opened. Press Play or play a note.");
    assert_eq!(ui.global::<Bar>().get_performance().as_str(), "Brass Corner", "the bar follows: the Performance was read again after the load");
}

// The question about unsaved changes is over the screen: a press by the dial chooses, Cancel is no choice.
#[test]
fn the_question_over_the_screen_is_answered_by_the_dial() {
    let mut s = Shelf::new("library-question");
    let ui = s.ui();
    shell::goto(shell::Screen::Library);
    s.serve();
    s.e.open = ("p1".into(), "Evening Strings".into(), true);
    ui.global::<PartState>().invoke_tapped(2);
    s.serve();
    assert!(ui.global::<NoticeState>().get_open());
    assert_eq!(ui.global::<NoticeState>().get_title().as_str(), "Save changes to Evening Strings before opening Brass Corner?");
    assert_eq!(ui.global::<NoticeState>().get_choices().iter().map(|c| c.to_string()).collect::<Vec<_>>(), ["Save", "Don't Save", "Cancel"]);
    assert_eq!(ui.global::<NoticeState>().get_cursor(), -1, "no cursor until the first move");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Move { pane: None, by: 1 });
    assert_eq!(ui.global::<NoticeState>().get_cursor(), 1);
    s.e.calls.clear();
    shell::act(Action::Confirm(None));
    s.serve();
    assert!(!ui.global::<NoticeState>().get_open());
    assert!(!s.e.methods().contains(&"library.save".to_string()), "Don't Save");
    assert_eq!(s.e.called("library.load"), [json!({"id": "p4"})]);
    // Cancel closes it with nothing chosen: nothing opens.
    s.e.open = ("p1".into(), "Evening Strings".into(), true);
    s.e.calls.clear();
    ui.global::<PartState>().invoke_tapped(3);
    s.serve();
    assert!(ui.global::<NoticeState>().get_open());
    shell::act(Action::Cancel(None));
    s.serve();
    assert!(!ui.global::<NoticeState>().get_open());
    assert_eq!(ui.get_screen(), 13, "Cancel closed the question, not the screen");
    assert!(s.e.called("library.load").is_empty());
}

// Search is a bar action: the keyboard opens for it and what is typed narrows the list.
#[test]
fn search_is_typed_on_the_keyboard() {
    let mut s = Shelf::new("library-search");
    let ui = s.ui();
    shell::set_text_on_screen(false);
    shell::goto(shell::Screen::Library);
    s.serve();
    ui.global::<Bar>().invoke_action(0);
    assert!(ui.global::<KeyboardState>().get_open());
    assert_eq!(ui.global::<KeyboardState>().get_title().as_str(), "Search the Performances");
    for ch in "warm".chars() {
        assert!(shell::key(&ch.to_string()));
    }
    assert!(shell::key(&slint::SharedString::from(slint::platform::Key::Return)));
    s.serve();
    assert_eq!(s.e.called("library.query").last().unwrap()["text"], "warm");
    assert_eq!(labels(&ui), ["Filters", "Brass Corner", "Evening Strings"]);
    assert_eq!(ui.global::<Bar>().get_actions().row_data(0).unwrap().label.as_str(), "Search: warm");
}

// A performance instrument: with the Library open and nothing happening, it asks nothing and draws nothing.
#[test]
fn at_rest_it_costs_no_request_and_no_frame() {
    let mut s = Shelf::new("library-idle");
    shell::goto(shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    assert_eq!(s.f.screen.frame(), None, "nothing is drawn at rest");
    std::thread::sleep(Duration::from_millis(1200));
    slint::platform::update_timers_and_animations();
    assert!(s.f.engine.try_request(Duration::from_millis(500)).is_none(), "nothing is asked at rest");
}

// Home's tile says what is installed as the JUCE launcher does: the plugin packages first, only when there are any, the
// Sounds last, only when there are some.
#[test]
fn home_says_what_is_installed() {
    let line = |s: &Shelf| s.f.screen.ui.global::<crate::HomeState>().get_tiles().iter().nth(1).unwrap().line.to_string();
    let mut f = Fixture::new("library-home");
    f.engine.answer(); // session.hello
    f.pump();
    let mut e = Fake::new(library());
    e.plugin_packages = 5;
    let mut s = Shelf { f, e };
    s.serve();
    assert_eq!(line(&s), "5 instruments and effects · 4 Performances · 2 Sounds");
    let mut f = Fixture::new("library-home2");
    f.engine.answer();
    f.pump();
    let mut s = Shelf { f, e: Fake::new(vec![]) };
    s.serve();
    assert_eq!(line(&s), "0 Performances");
}
