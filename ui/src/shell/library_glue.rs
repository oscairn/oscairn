// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shell's glue for the Library app (library/): the requests it asks sent (a step the engine refused as busy
//! is sent again after its delay), the menu and the question it has ready opened over it, and the question's answer.

use super::*;

/// Whether Home's Library tile opens the Rust app: it does (Library is ported whole). `OSCAIRN_UI_LIBRARY=0` hands the tile over to the JUCE
/// app again (a fault found on the device); `OSCAIRN_UI_LIBRARY=1` opens the Rust app whatever `PORTED` says.
const PORTED: bool = true;

pub(super) fn in_rust() -> bool {
    match std::env::var_os("OSCAIRN_UI_LIBRARY") {
        Some(v) if v == "0" => false,
        Some(v) if !v.is_empty() => true,
        _ => PORTED,
    }
}

impl App {
    /// Something for the Library, then what it asks the engine sent, and the menu or question it has ready opened.
    pub(super) fn on_library<R>(&mut self, f: impl FnOnce(&mut library::Screen) -> R) -> R {
        self.library.part = self.part.id();
        let r = f(&mut self.library);
        for send in std::mem::take(&mut self.library.out) {
            let request = Request::LibraryApp(send.request);
            match send.after {
                None => self.send(request, send.method, send.params),
                Some(after) => {
                    let (method, params) = (send.method, send.params);
                    slint::Timer::single_shot(after, move || with_app(|a| a.send(request, method, params)));
                }
            }
        }
        if let Some((title, items)) = self.library.take_menu() {
            self.open_menu(Screen::Library, &title, items);
        }
        if let Some((title, message, choices)) = self.library.take_notice() {
            self.open_notice(Screen::Library, &title, &message, &choices.iter().map(String::as_str).collect::<Vec<_>>());
        }
        if self.screen == Screen::Library {
            self.show_bar();
        }
        self.refresh_wait();
        if self.library.take_scan() {
            self.library_scan();
        }
        r
    }

    /// The plugins that come with the app, by ID and version, for the Catalog's "Built in": the plugin scanner run as the
    /// chain's is (scan.rs), off the UI thread; with no scanner beside the program none are known.
    fn library_scan(&mut self) {
        let (dir, helper) = (crate::scan::plugin_dir(super::PLUGIN_DIR.with(|d| d.borrow().clone()).as_deref()), crate::scan::scanner());
        if !helper.is_file() {
            self.library.scanned(Default::default());
            return;
        }
        let weak = self.ui.clone();
        std::thread::spawn(move || {
            let found: std::collections::BTreeMap<String, String> = crate::scan::scan_versions(&dir, &helper, crate::scan::TIMEOUT).into_iter().collect();
            let _ = weak.upgrade_in_event_loop(move |_| with_app(|a| a.on_library(|l| l.scanned(found))));
        });
    }

    /// A navigation action or tap for the Library, and where it takes the shell.
    pub(super) fn library_act(&mut self, f: impl FnOnce(&mut library::Screen) -> library::Outcome) {
        match self.on_library(f) {
            library::Outcome::Back => self.go(Screen::Home),
            library::Outcome::Prompt(title, text) => self.open_keyboard(Screen::Library, &title, &text),
            library::Outcome::Stay => {}
        }
    }

    /// A question over `screen`, which gets the answer.
    pub(super) fn open_notice(&mut self, screen: Screen, title: &str, message: &str, choices: &[&str]) {
        if let Some(ui) = self.ui() {
            self.question = Some((screen, notice::Notice::open(&ui, title, message, choices)));
        }
    }

    pub(super) fn close_notice(&mut self) {
        if let Some((_, mut n)) = self.question.take() {
            n.close();
        }
    }

    pub(super) fn notice_act(&mut self, action: Action) {
        let outcome = self.question.as_mut().map(|(_, n)| n.act(action));
        self.notice_outcome(outcome);
    }

    pub(super) fn notice_tap(&mut self, i: usize) {
        let outcome = self.question.as_mut().map(|(_, n)| n.tap(i));
        self.notice_outcome(outcome);
    }

    /// What a question came to: a choice goes to the screen that asked, a close is no choice.
    fn notice_outcome(&mut self, outcome: Option<notice::Outcome>) {
        let choice = match outcome {
            Some(notice::Outcome::Choose(i)) => Some(i),
            Some(notice::Outcome::Close) => None,
            _ => return,
        };
        if let Some((screen, _)) = self.question.take() {
            if screen == Screen::Library {
                self.library_act(|l| l.noticed(choice));
            }
            if screen == Screen::Settings {
                self.on_settings(|s| s.noticed(choice));
            }
        }
    }
}
