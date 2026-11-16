// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shell's glue for the Library app (library/): the requests it asks sent (a step the engine refused as busy
//! is sent again after its delay), the menu and the question it has ready opened over it, and the question's answer.

use super::*;

/// Whether Home's Library tile opens the Rust app. Not until every screen of ui/LIBRARY-APP.md is built: a half app
/// that hands the display back is worse than the JUCE app, so the tile hands over as before. Opened for development
/// with `OSCAIRN_UI_LIBRARY=1`; the tests enter it with `shell::goto`.
const PORTED: bool = false;

pub(super) fn in_rust() -> bool {
    PORTED || std::env::var_os("OSCAIRN_UI_LIBRARY").is_some_and(|v| v != "0" && !v.is_empty())
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
        r
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
        }
    }
}
