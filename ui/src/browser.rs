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
    let c = Cursor { count, per_page, at: c.at.map(|a| a.min(count.saturating_sub(1))), ..c };
    match c.at {
        Some(at) => c.to(at),
        None => Cursor { first: if c.first >= count { 0 } else { c.first / per_page * per_page }, ..c },
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

    /// The scroll bar asked for the view starting at row `first` (a touch only: it selects nothing, so the cursor drops).
    pub fn scroll_to(&mut self, first: usize) {
        let per = self.cursor.per_page.max(1);
        let last = self.count.saturating_sub(1) / per * per;
        self.cursor = Cursor { at: None, first: (first / per * per).min(last), ..self.cursor };
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
