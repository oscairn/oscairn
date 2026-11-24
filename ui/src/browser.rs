// SPDX-License-Identifier: AGPL-3.0-or-later
//! The list the Chain, Sounds and Parameters screens share: the Part screen's component (a list of
//! rows on the left with a border cursor, a read-only panel on the right) driven by `Browser`, which
//! keeps the cursor, the view and the models, and turns a navigation action into what the screen
//! decides on. Without touch: the left knob (pane 0) or the dial moves the cursor, a press on a row is
//! `Nav::Press`, a press on a row the screen marks for the dial starts changing it (`editing`) and the
//! dial then gives `Nav::Dial`, the right knob (pane 1) gives `Nav::Pane(by)` (the screen's second
//! pane: the panel), Cancel ends a change, then drops the cursor, then goes back.

use crate::nav::{Action, Cursor};
use crate::setlist::update;
use crate::{InfoLine, PartState, SettingData, Shell};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

#[derive(Debug, PartialEq)]
pub enum Nav {
    None,
    /// A press (or a tap) on row `at` of the whole list.
    Press(usize),
    /// The dial on row `at`, which is being changed.
    Dial(usize, i64),
    /// The right knob: the panel's.
    Pane(i64),
    /// Cancel with nothing to drop: back.
    Back,
}

pub struct Browser {
    ui: slint::Weak<Shell>,
    rows: Rc<VecModel<SettingData>>,
    info: Rc<VecModel<InfoLine>>,
    pub cursor: Cursor,
    pub info_cursor: Cursor,
    /// The dial changes the value under the cursor.
    pub editing: bool,
    count: usize,
    /// This screen is the one shown: only it writes the window's list (the Part screen shares it).
    active: bool,
}

/// The cursor fitted to `count` items, `per_page` a view, the view following it.
fn fit(c: Cursor, count: usize, per_page: usize) -> Cursor {
    if count == 0 {
        // Nothing to sit on: no selection, the first empty page.
        return Cursor::new(0, per_page);
    }
    let c = Cursor { count, per_page, at: c.at.map(|a| a.min(count - 1)), ..c };
    match c.at {
        Some(at) => c.to(at),
        None => Cursor { first: if c.first >= count { 0 } else { c.first }, ..c },
    }
}

impl Browser {
    pub fn new(ui: &Shell) -> Browser {
        Browser { ui: ui.as_weak(), rows: Rc::new(VecModel::default()), info: Rc::new(VecModel::default()), cursor: Cursor::new(0, 1), info_cursor: Cursor::new(0, 1), editing: false, count: 0, active: false }
    }

    /// Makes these the models the window shows: the screen that is shown does it.
    pub fn install(&mut self) {
        self.active = true;
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<PartState>();
            state.set_rows(ModelRc::from(self.rows.clone()));
            state.set_info(ModelRc::from(self.info.clone()));
        }
    }

    /// Starts over on a new list (the cursor on none, the first view).
    /// Whether this screen is the one shown (its models are the window's).
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn reset(&mut self) {
        self.cursor = Cursor::new(0, 1);
        self.info_cursor = Cursor::new(0, 1);
        self.editing = false;
    }

    /// The cursor to none and the first view, the panel kept.
    pub fn reset_cursor(&mut self) {
        self.cursor = Cursor::new(0, 1);
        self.editing = false;
    }

    /// The cursor is on the first row and no value is being changed: moving up leaves the list
    /// for the top bar's actions.
    pub fn at_top(&self) -> bool {
        self.cursor.at == Some(0) && !self.editing
    }

    /// Row `row` of the view as an index of the whole list.
    pub fn index(&self, row: i32) -> usize {
        self.cursor.first + row.max(0) as usize
    }

    /// Shows `rows` and the panel `info`: the cursor and view fitted, each row set only when it changed.
    pub fn show(&mut self, name: &str, title: &str, noun: &str, rows: Vec<SettingData>, info: Vec<InfoLine>) {
        let Some(ui) = self.ui.upgrade().filter(|_| self.active) else { return };
        let state = ui.global::<PartState>();
        let per_view = state.get_rows_per_view().max(1) as usize;
        self.count = rows.len();
        self.cursor = fit(self.cursor, rows.len(), per_view);
        let first = self.cursor.first;
        let end = rows.len().min(first + per_view);
        let at = self.cursor.at;
        let editing = self.editing;
        let shown: Vec<SettingData> = rows.into_iter().enumerate().skip(first).take(per_view).map(|(i, r)| SettingData { cursor: at == Some(i), editing: editing && at == Some(i), ..r }).collect();
        update(&self.rows, shown);
        let per = state.get_info_per_view().max(1) as usize;
        self.info_cursor = fit(self.info_cursor, info.len(), per);
        state.set_info_more(info.len() > per);
        update(&self.info, info.into_iter().skip(self.info_cursor.first).take(per).collect());
        state.set_total(self.count as i32);
        state.set_first(first as i32);
        state.set_name(name.into());
        state.set_title(title.into());
        state.set_view(if self.count == 0 { String::new() } else { format!("{noun} {}–{} of {}", first + 1, end, self.count) }.into());
    }

    pub fn say(&self, text: &str) {
        if let Some(ui) = self.ui.upgrade().filter(|_| self.active) {
            ui.global::<PartState>().set_message(text.into());
        }
    }

    /// The scroll bar or a finger drag asked for the view starting at row `first` (a touch only: it selects nothing,
    /// so the cursor drops). A scroll keeps the exact row asked (the list may hold a partial page).
    pub fn scroll_to(&mut self, first: usize) {
        let last = self.count.saturating_sub(1);
        self.cursor = Cursor { at: None, first: first.min(last), ..self.cursor };
        self.editing = false;
    }

    /// The panel's own cursor moved: its view turns.
    pub fn scroll_info(&mut self, by: i64) {
        self.info_cursor = self.info_cursor.moved(by * self.info_cursor.per_page as i64);
    }

    fn shift(&mut self, by: i64) {
        if !self.editing {
            self.cursor = self.cursor.moved(by);
        }
    }

    /// The cursor, or the value being changed, by `by`.
    fn step(&mut self, by: i64) -> Nav {
        match self.cursor.at.filter(|_| self.editing) {
            Some(at) => Nav::Dial(at, by),
            None => {
                self.shift(by);
                Nav::None
            }
        }
    }

    pub fn nav(&mut self, action: Action) -> Nav {
        match action {
            Action::Move { pane: Some(1), by } => Nav::Pane(by as i64),
            Action::Move { by, .. } => self.step(by as i64),
            Action::Arrow { dx: 0, dy } => self.step(dy as i64),
            Action::Arrow { dx, .. } => self.step(dx as i64),
            Action::Page(by) => {
                self.shift(by as i64 * self.cursor.per_page as i64);
                Nav::None
            }
            Action::End(by) => {
                self.shift(by as i64 * 10_000);
                Nav::None
            }
            Action::Confirm(_) => self.cursor.at.map_or(Nav::None, Nav::Press),
            Action::Cancel(_) => {
                if self.editing {
                    self.editing = false;
                    Nav::None
                } else if self.cursor.at.is_some() {
                    self.cursor = self.cursor.clear();
                    Nav::None
                } else {
                    Nav::Back
                }
            }
        }
    }

    /// A tap on row `row` of the view: the cursor there, then what a press does.
    pub fn tap(&mut self, row: i32) -> Nav {
        let at = self.index(row);
        self.cursor = self.cursor.to(at);
        self.editing = false;
        self.cursor.at.map_or(Nav::None, Nav::Press)
    }

    /// The cursor drops when the screen goes.
    pub fn hidden(&mut self) {
        self.active = false;
        self.cursor = self.cursor.clear();
        self.editing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Browser` whose window is never shown: the methods under test never upgrade the weak.
    fn browser() -> Browser {
        Browser {
            ui: slint::Weak::default(),
            rows: Rc::new(VecModel::default()),
            info: Rc::new(VecModel::default()),
            cursor: Cursor::new(0, 10),
            info_cursor: Cursor::new(0, 10),
            editing: false,
            count: 0,
            active: false,
        }
    }

    #[test]
    fn fit_keeps_a_first_cursor_on_a_list() {
        let c = fit(Cursor::new(0, 1), 5, 10);
        assert_eq!(c.at, None, "no selection until the first move");
        assert_eq!((c.first, c.count), (0, 5));
    }

    #[test]
    fn fit_puts_the_selected_items_page_under_it() {
        let c = fit(Cursor::new(30, 10).to(15), 30, 10);
        assert_eq!((c.at, c.first), (Some(15), 10), "a view of ten turns at fifteen");
    }

    #[test]
    fn fit_clamps_a_selection_past_the_end_of_a_shrunk_list() {
        let c = fit(Cursor::new(30, 10).to(29), 10, 10);
        assert_eq!((c.at, c.first), (Some(9), 0), "row 9 of ten, its page");
    }

    #[test]
    fn fit_empty_list_shows_no_cursor() {
        let c = fit(Cursor::new(0, 1), 0, 10);
        assert_eq!((c.at, c.first, c.count), (None, 0, 0));
    }

    #[test]
    fn fit_shrunk_to_nothing_drops_an_old_selection_and_page() {
        let c = fit(Cursor::new(50, 10).to(39), 0, 10);
        assert_eq!((c.at, c.first, c.count), (None, 0, 0), "an empty list has nothing to sit on");
    }

    #[test]
    fn index_never_goes_before_the_page() {
        let mut b = browser();
        b.cursor = Cursor::new(30, 10).to(17); // first is 10
        assert_eq!(b.index(0), 10);
        assert_eq!(b.index(7), 17);
        assert_eq!(b.index(-5), 10, "a drag above the view clamps to the first shown");
    }

    #[test]
    fn at_top_needs_the_first_row_and_no_change() {
        let mut b = browser();
        b.cursor = Cursor::new(5, 10);
        assert!(!b.at_top(), "no selection is not on the top row");
        b.cursor = Cursor::new(5, 10).to(0);
        assert!(b.at_top());
        b.cursor = Cursor::new(5, 10).to(1);
        assert!(!b.at_top(), "the second row leaves the list");
        b.cursor = Cursor::new(5, 10).to(0);
        b.editing = true;
        assert!(!b.at_top(), "a value being changed still holds the list");
    }

    #[test]
    fn nav_moves_the_cursor_and_answers_none_until_confirmed() {
        let mut b = browser();
        b.cursor = Cursor::new(5, 10);
        assert_eq!(b.nav(Action::Move { pane: None, by: 1 }), Nav::None);
        assert_eq!(b.cursor.at, Some(0), "the first move shows it on the first shown");
        assert_eq!(b.nav(Action::Arrow { dx: 0, dy: 3 }), Nav::None);
        assert_eq!(b.cursor.at, Some(3));
        assert_eq!(b.nav(Action::Confirm(Some(0))), Nav::Press(3));
    }

    #[test]
    fn nav_arrow_across_steps_like_the_dial() {
        let mut b = browser();
        b.cursor = Cursor::new(5, 10);
        assert_eq!(b.nav(Action::Arrow { dx: 1, dy: 0 }), Nav::None);
        assert_eq!(b.cursor.at, Some(0));
    }

    #[test]
    fn nav_right_knob_steers_the_panel_apart_from_the_cursor() {
        let mut b = browser();
        b.cursor = Cursor::new(5, 10).to(3);
        b.editing = true;
        assert_eq!(b.nav(Action::Move { pane: Some(1), by: 2 }), Nav::Pane(2));
        assert_eq!((b.cursor.at, b.editing), (Some(3), true), "the panel leaves the row's change untouched");
    }

    #[test]
    fn nav_a_page_and_an_end_shift_without_opening() {
        let mut b = browser();
        b.cursor = Cursor::new(25, 10).to(5);
        assert_eq!(b.nav(Action::Page(1)), Nav::None);
        assert_eq!((b.cursor.at, b.cursor.first), (Some(15), 10), "a whole page on");
        assert_eq!(b.nav(Action::End(1)), Nav::None);
        assert_eq!((b.cursor.at, b.cursor.first), (Some(24), 20), "the last row, its page");
    }

    #[test]
    fn nav_end_back_to_the_first() {
        let mut b = browser();
        b.cursor = Cursor::new(25, 10).to(17);
        assert_eq!(b.nav(Action::End(-1)), Nav::None);
        assert_eq!((b.cursor.at, b.cursor.first), (Some(0), 0));
    }

    #[test]
    fn nav_cancel_first_drops_the_change_then_the_cursor_then_back() {
        let mut b = browser();
        assert_eq!(b.nav(Action::Cancel(None)), Nav::Back, "nothing to drop: back");
        b.cursor = Cursor::new(5, 10).to(2);
        assert_eq!(b.nav(Action::Cancel(None)), Nav::None, "a selection drops it");
        assert_eq!(b.cursor.at, None);
        b.cursor = Cursor::new(5, 10).to(2);
        b.editing = true;
        assert_eq!(b.nav(Action::Cancel(None)), Nav::None, "a change drops it first, keeping the row");
        assert!(!b.editing);
        assert_eq!(b.cursor.at, Some(2), "the row stays until a second cancel");
    }

    #[test]
    fn nav_confirm_with_no_selection_answers_none() {
        let mut b = browser();
        assert_eq!(b.nav(Action::Confirm(None)), Nav::None);
    }

    #[test]
    fn nav_dial_changes_the_value_under_the_cursor_after_a_press() {
        let mut b = browser();
        b.cursor = Cursor::new(5, 10).to(2);
        b.editing = true;
        assert_eq!(b.nav(Action::Move { pane: None, by: 4 }), Nav::Dial(2, 4));
        assert_eq!(b.cursor.at, Some(2), "the value changes, not the row");
    }

    #[test]
    fn tap_selects_and_presses_the_row() {
        let mut b = browser();
        b.cursor = Cursor::new(20, 10);
        assert_eq!(b.tap(3), Nav::Press(3));
        assert_eq!(b.cursor.at, Some(3));
        assert_eq!(b.tap(-1), Nav::Press(0), "a finger above the list presses its first row");
    }

    #[test]
    fn tap_on_an_empty_list_answers_none() {
        let mut b = browser();
        assert_eq!(b.tap(0), Nav::None);
        assert_eq!(b.cursor.at, None);
    }

    #[test]
    fn scroll_to_holds_the_first_and_drops_the_selection() {
        let mut b = browser();
        b.count = 10;
        b.cursor = Cursor::new(10, 10).to(5);
        b.editing = true;
        b.scroll_to(3);
        assert_eq!((b.cursor.at, b.cursor.first, b.editing), (None, 3, false));
        b.scroll_to(99);
        assert_eq!(b.cursor.first, 9, "past the end clamps to the last row");
        b.scroll_to(0);
        assert_eq!(b.cursor.first, 0);
    }

    #[test]
    fn scroll_info_turns_the_panel_a_whole_view_at_a_time() {
        let mut b = browser();
        b.info_cursor = Cursor::new(20, 5);
        b.scroll_info(1);
        assert_eq!((b.info_cursor.at, b.info_cursor.first), (Some(0), 0));
        b.scroll_info(1);
        assert_eq!((b.info_cursor.at, b.info_cursor.first), (Some(5), 5));
        b.scroll_info(-1);
        assert_eq!((b.info_cursor.at, b.info_cursor.first), (Some(0), 0));
        b.scroll_info(99);
        assert_eq!((b.info_cursor.at, b.info_cursor.first), (Some(19), 15), "stops at the last row's page");
    }

    #[test]
    fn reset_and_hidden_drop_the_cursor() {
        let mut b = browser();
        b.cursor = Cursor::new(5, 10).to(3);
        b.editing = true;
        b.info_cursor = Cursor::new(5, 10).to(2);
        b.hidden();
        assert_eq!((b.cursor.at, b.editing, b.active()), (None, false, false));
        b.cursor = Cursor::new(5, 10).to(3);
        b.editing = true;
        b.reset_cursor();
        assert_eq!((b.cursor.at, b.editing), (None, false), "the panel keeps its row");
        assert_eq!(b.info_cursor.at, Some(2));
    }
}
