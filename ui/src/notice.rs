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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::Action;

    /// A notice with no window: `show` and `close` upgrade an unset weak and are no-ops, so the
    /// cursor and the decisions are what is tested.
    fn notice(choices: usize) -> Notice {
        Notice { ui: slint::Weak::default(), choices, at: None }
    }

    #[test]
    fn the_first_move_lands_on_the_first_choice_even_backwards() {
        let mut n = notice(3);
        assert_eq!(n.act(Action::Move { pane: None, by: -3 }), Outcome::Stay);
        assert_eq!(n.at, Some(0));
    }

    #[test]
    fn a_move_holds_within_the_choices() {
        let mut n = notice(3);
        n.act(Action::Move { pane: None, by: 1 });
        assert_eq!(n.at, Some(0)); // the first move lands on the first choice
        n.act(Action::Move { pane: None, by: 1 });
        assert_eq!(n.at, Some(1));
        n.act(Action::Move { pane: None, by: 1 });
        assert_eq!(n.at, Some(2)); // the last choice
        n.act(Action::Move { pane: None, by: 1 });
        assert_eq!(n.at, Some(2)); // stays on the last choice
    }

    #[test]
    fn moving_above_the_top_clamps_at_zero() {
        let mut n = notice(3);
        n.act(Action::Move { pane: None, by: -1 });
        assert_eq!(n.at, Some(0));
        n.act(Action::Move { pane: None, by: -1 });
        assert_eq!(n.at, Some(0));
    }

    #[test]
    fn a_page_and_an_end_reach_the_ends_of_the_choices() {
        let mut n = notice(3);
        n.act(Action::Page(1)); // the first move lands on the first choice
        assert_eq!(n.at, Some(0));
        n.act(Action::Page(1)); // then a whole page reaches the last
        assert_eq!(n.at, Some(2));
        n.act(Action::End(-1)); // then back to the first
        assert_eq!(n.at, Some(0));
    }

    #[test]
    fn an_arrow_steps_in_the_row_axes() {
        let mut n = notice(3);
        n.act(Action::Arrow { dx: 0, dy: 1 });
        assert_eq!(n.at, Some(0));
        n.act(Action::Arrow { dx: 1, dy: 0 }); // a sideways arrow moves too
        assert_eq!(n.at, Some(1));
        n.act(Action::Arrow { dx: 9, dy: 0 });
        assert_eq!(n.at, Some(2));
        n.act(Action::Arrow { dx: 0, dy: -9 });
        assert_eq!(n.at, Some(0));
    }

    #[test]
    fn no_choices_moves_and_confirms_nothing() {
        let mut n = notice(0);
        assert_eq!(n.act(Action::Move { pane: None, by: 1 }), Outcome::Stay);
        assert_eq!(n.at, None);
        assert_eq!(n.act(Action::Confirm(None)), Outcome::Stay);
        assert_eq!(n.tap(0), Outcome::Stay);
    }

    #[test]
    fn confirm_chooses_what_the_cursor_sits_on() {
        let mut n = notice(3);
        assert_eq!(n.act(Action::Confirm(None)), Outcome::Stay); // no cursor yet
        n.act(Action::Move { pane: None, by: 1 });
        assert_eq!(n.act(Action::Confirm(None)), Outcome::Choose(0)); // the first move sits on the first choice
        let mut n = notice(3);
        n.act(Action::Move { pane: None, by: 1 });
        n.act(Action::Move { pane: None, by: 1 });
        assert_eq!(n.act(Action::Confirm(None)), Outcome::Choose(1));
        n.act(Action::End(1));
        n.act(Action::End(1));
        assert_eq!(n.act(Action::Confirm(None)), Outcome::Choose(2)); // the last choice
    }

    #[test]
    fn cancel_closes_without_choosing() {
        let mut n = notice(3);
        assert_eq!(n.act(Action::Cancel(None)), Outcome::Close);
    }

    #[test]
    fn a_tap_chooses_only_a_choice_in_range() {
        let mut n = notice(3);
        assert_eq!(n.tap(0), Outcome::Choose(0)); // the first choice
        let mut n = notice(3);
        assert_eq!(n.tap(2), Outcome::Choose(2)); // the last choice
        assert_eq!(n.tap(3), Outcome::Stay); // out of range
        assert_eq!(n.tap(usize::MAX), Outcome::Stay);
    }
}
