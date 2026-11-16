// SPDX-License-Identifier: AGPL-3.0-or-later
//! A question over the screen (slint/notice.slint), what the JUCE app asks in an alert window: a title, a message
//! and the choices as buttons, a border cursor on one. Without touch: the dial, the knobs and the arrows move the
//! cursor (none until the first move, as in a list), a press chooses, Cancel closes with nothing chosen. A tap
//! outside the panel closes it too.

use crate::nav::Action;
use crate::{NoticeState, Shell};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// A choice was pressed: its number in the order given; the notice is closed.
    Choose(usize),
    /// Closed with nothing chosen.
    Close,
}

pub struct Notice {
    ui: slint::Weak<Shell>,
    choices: usize,
    at: Option<usize>,
}

impl Notice {
    pub fn open(ui: &Shell, title: &str, message: &str, choices: &[&str]) -> Notice {
        let state = ui.global::<NoticeState>();
        state.set_title(title.into());
        state.set_message(message.into());
        state.set_choices(ModelRc::new(VecModel::from(choices.iter().map(|c| SharedString::from(*c)).collect::<Vec<_>>())));
        state.set_cursor(-1);
        state.set_open(true);
        Notice { ui: ui.as_weak(), choices: choices.len(), at: None }
    }

    fn show(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<NoticeState>().set_cursor(self.at.map_or(-1, |a| a as i32));
        }
    }

    pub fn close(&mut self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<NoticeState>().set_open(false);
        }
    }

    fn step(&mut self, by: i64) {
        if self.choices == 0 {
            return;
        }
        let at = match self.at {
            None => 0,
            Some(at) => (at as i64 + by).clamp(0, self.choices as i64 - 1) as usize,
        };
        self.at = Some(at);
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        match action {
            Action::Move { by, .. } => self.step(i64::from(by)),
            Action::Arrow { dx, dy } => self.step(i64::from(if dy != 0 { dy } else { dx })),
            Action::Page(by) | Action::End(by) => self.step(i64::from(by) * 100_000),
            Action::Confirm(_) => {
                if let Some(at) = self.at {
                    self.close();
                    return Outcome::Choose(at);
                }
            }
            Action::Cancel(_) => {
                self.close();
                return Outcome::Close;
            }
        }
        self.show();
        Outcome::Stay
    }

    /// A tap on choice `i`.
    pub fn tap(&mut self, i: usize) -> Outcome {
        if i < self.choices {
            self.close();
            return Outcome::Choose(i);
        }
        Outcome::Stay
    }
}
