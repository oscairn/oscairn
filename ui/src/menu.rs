// SPDX-License-Identifier: AGPL-3.0-or-later
//! A menu over the screen: what the JUCE Edit view's pages open as popup menus (a control's targets,
//! its curve, the scenes a mapping works in, Routing's connections), as a list with a border cursor
//! (slint/menu.slint). A row is a choice (ticked, unticked or neither), a submenu (a press opens the
//! next list, Back or Cancel goes up) or a heading. The dial, the knobs and the arrows move the cursor,
//! which starts on the first row that can be chosen; nothing is chosen until a press; Cancel goes up a
//! level and at the first closes. A row cannot be chosen when it is disabled. How many rows show is
//! what fills the height (the component says), so it follows the window, the spacing and the type;
//! the view is cut from the first row and follows the cursor, Page turns it.

use crate::nav::Action;
use crate::{MenuRow, MenuState, Shell};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A choice, which the screen that opened the menu knows by this number.
    Choice(usize),
    /// A list of its own.
    Sub(Vec<Item>),
    /// A heading over the rows after it.
    Head,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    /// Ticked or not, or no tick column's mark for this row.
    pub tick: Option<bool>,
    pub enabled: bool,
    pub kind: Kind,
}

impl Item {
    pub fn choice(label: impl Into<String>, id: usize) -> Item {
        Item { label: label.into(), tick: None, enabled: true, kind: Kind::Choice(id) }
    }
    pub fn ticked(label: impl Into<String>, id: usize, on: bool) -> Item {
        Item { tick: Some(on), ..Item::choice(label, id) }
    }
    pub fn sub(label: impl Into<String>, items: Vec<Item>) -> Item {
        Item { label: label.into(), tick: None, enabled: !items.is_empty(), kind: Kind::Sub(items) }
    }
    #[cfg(test)]
    pub fn head(label: impl Into<String>) -> Item {
        Item { label: label.into(), tick: None, enabled: false, kind: Kind::Head }
    }
    pub fn off(mut self) -> Item {
        self.enabled = false;
        self
    }
    fn selectable(&self) -> bool {
        self.enabled && self.kind != Kind::Head
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// A choice was pressed: its number; the menu is closed.
    Choose(usize),
    /// Closed with nothing chosen.
    Close,
}

/// One list of the path to the one shown: its title, its rows and the cursor in it.
struct Level {
    title: String,
    items: Vec<Item>,
    at: usize,
    first: usize,
}

pub struct Menu {
    ui: slint::Weak<Shell>,
    levels: Vec<Level>,
    rows: Rc<VecModel<MenuRow>>,
}

impl Menu {
    /// A menu showing `items` under `title`, the cursor on the first row that can be chosen.
    pub fn open(ui: &Shell, title: &str, items: Vec<Item>) -> Menu {
        let rows = Rc::new(VecModel::default());
        let state = ui.global::<MenuState>();
        state.set_rows(ModelRc::from(rows.clone()));
        state.set_open(true);
        let mut m = Menu { ui: ui.as_weak(), levels: vec![], rows };
        m.push(title.to_string(), items);
        m
    }

    /// The cursor on row `at` of the list shown, if it can be chosen: a dropdown opens on the choice it has now.
    pub fn start(&mut self, at: usize) {
        if let Some(level) = self.levels.last_mut().filter(|l| l.items.get(at).is_some_and(Item::selectable)) {
            level.at = at;
        }
        self.show();
    }

    fn push(&mut self, title: String, items: Vec<Item>) {
        let at = items.iter().position(Item::selectable).unwrap_or(0);
        self.levels.push(Level { title, items, at, first: 0 });
        self.show();
    }

    fn per_view(&self) -> usize {
        self.ui.upgrade().map_or(1, |ui| ui.global::<MenuState>().get_rows_per_view().max(1) as usize)
    }

    /// The path to the list shown: the titles of the levels, joined.
    fn path(&self) -> String {
        self.levels.iter().map(|l| l.title.as_str()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" › ")
    }

    /// The window's view of the list: the view the cursor is in, each row set only when it changed.
    pub fn show(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let per = self.per_view();
        let path = self.path();
        let Some(level) = self.levels.last_mut() else { return };
        level.at = level.at.min(level.items.len().saturating_sub(1));
        level.first = level.at / per * per;
        let end = level.items.len().min(level.first + per);
        let rows: Vec<MenuRow> = level.items[level.first..end]
            .iter()
            .enumerate()
            .map(|(i, it)| MenuRow {
                label: it.label.as_str().into(),
                tick: it.tick.map_or(-1, i32::from),
                enabled: it.enabled,
                sub: matches!(it.kind, Kind::Sub(_)),
                head: it.kind == Kind::Head,
                cursor: level.first + i == level.at && it.selectable(),
            })
            .collect();
        crate::setlist::update(&self.rows, rows);
        let state = ui.global::<MenuState>();
        state.set_title(path.into());
        state.set_up(self.levels.len() > 1);
        let level = self.levels.last().unwrap();
        // The list's size and the view's first row, for its scroll bar: a drag or a tap on the bar asks for a view.
        state.set_total(level.items.len() as i32);
        state.set_first(level.first as i32);
        state.set_more(level.items.len() > per);
        state.set_view(if level.items.is_empty() { "Nothing here".to_string() } else { format!("{}–{} of {}", level.first + 1, end, level.items.len()) }.into());
    }

    /// The cursor `by` rows that can be chosen along, held at the ends.
    fn step(&mut self, by: i64) {
        let Some(level) = self.levels.last_mut() else { return };
        let ok: Vec<usize> = (0..level.items.len()).filter(|i| level.items[*i].selectable()).collect();
        if ok.is_empty() {
            return;
        }
        let at = ok.iter().position(|i| *i >= level.at).unwrap_or(ok.len() - 1) as i64;
        let to = (at + by).clamp(0, ok.len() as i64 - 1) as usize;
        level.at = ok[to];
    }

    /// A press on the row `at` of the list shown.
    fn press(&mut self, at: usize) -> Outcome {
        let Some(level) = self.levels.last() else { return Outcome::Close };
        let Some(item) = level.items.get(at).filter(|i| i.selectable()).cloned() else { return Outcome::Stay };
        match item.kind {
            Kind::Choice(id) => {
                self.close();
                return Outcome::Choose(id);
            }
            Kind::Sub(items) => self.push(item.label, items),
            Kind::Head => {}
        }
        Outcome::Stay
    }

    /// Up a level, or closed from the first.
    pub fn back(&mut self) -> Outcome {
        if self.levels.len() > 1 {
            self.levels.pop();
            self.show();
            return Outcome::Stay;
        }
        self.close();
        Outcome::Close
    }

    pub fn close(&mut self) {
        self.levels.clear();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<MenuState>().set_open(false);
        }
    }

    /// A view of rows towards the start (-1) or the end (1), the cursor on its first row that can be chosen.
    pub fn turn(&mut self, by: i32) {
        let per = self.per_view();
        if let Some(level) = self.levels.last_mut() {
            let views = level.items.len().div_ceil(per).max(1);
            let to = ((level.first / per) as i64 + i64::from(by)).clamp(0, views as i64 - 1) as usize;
            level.at = (to * per..level.items.len().min((to + 1) * per)).find(|i| level.items[*i].selectable()).unwrap_or(to * per);
        }
        self.show();
    }

    /// The scroll bar's or a finger drag's view starts at row `first` (a touch only): the view turns to that page
    /// and the cursor sits on a row of it that can be chosen (a scroll is not a choice). A whole view at a time.
    pub fn scroll_to(&mut self, first: i32) {
        let per = self.per_view();
        if let Some(level) = self.levels.last_mut() {
            let page = (first.max(0) as usize / per) * per;
            let last = level.items.len().saturating_sub(1);
            level.first = page.min(last);
            let want = (page..level.items.len().min(page + per)).find(|i| level.items[*i].selectable());
            level.at = want.unwrap_or(level.first);
        }
        self.show();
    }

    /// A navigation action.
    pub fn act(&mut self, action: Action) -> Outcome {
        let outcome = match action {
            Action::Move { by, .. } => {
                self.step(i64::from(by));
                Outcome::Stay
            }
            Action::Arrow { dx, dy } => {
                self.step(i64::from(if dy != 0 { dy } else { dx }));
                Outcome::Stay
            }
            Action::Page(by) => {
                self.turn(by);
                Outcome::Stay
            }
            Action::End(by) => {
                self.step(i64::from(by) * 100_000);
                Outcome::Stay
            }
            Action::Confirm(_) => match self.levels.last().map(|l| l.at) {
                Some(at) => self.press(at),
                None => Outcome::Stay,
            },
            Action::Cancel(_) => self.back(),
        };
        if outcome == Outcome::Stay {
            self.show();
        }
        outcome
    }

    /// A tap on the row `row` of the view.
    pub fn tap(&mut self, row: i32) -> Outcome {
        let first = self.levels.last().map_or(0, |l| l.first);
        let outcome = self.press(first + row.max(0) as usize);
        if outcome == Outcome::Stay {
            self.show();
        }
        outcome
    }

    /// The rows of the list shown as a plain list, for tests: label, tick, cursor.
    #[cfg(test)]
    pub fn labels(&self) -> Vec<(String, Option<bool>, bool)> {
        let Some(level) = self.levels.last() else { return vec![] };
        level.items.iter().enumerate().map(|(i, it)| (it.label.clone(), it.tick, i == level.at)).collect()
    }

    #[cfg(test)]
    pub fn shown_rows(&self) -> usize {
        slint::Model::row_count(&*self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        Menu { ui: slint::Weak::default(), levels: vec![], rows: Rc::new(VecModel::default()) }
    }

    /// The index of the cursor's row, or None when the list is empty or the cursor nowhere.
    fn at(m: &Menu) -> Option<usize> {
        m.labels().iter().position(|(_, _, c)| *c)
    }

    fn sample() -> Vec<Item> {
        vec![
            Item::head("Curve"),
            Item::ticked("Curve A", 0, true),
            Item::choice("Curve B", 1),
            Item::choice("Curve C", 2).off(),
            Item::sub("More", vec![Item::choice("Inner", 9)]),
        ]
    }

    #[test]
    fn rows_carry_what_they_are() {
        let head = Item::head("H");
        let dis = Item::choice("X", 1).off();
        let sub = Item::sub("S", vec![]);
        assert!(!head.selectable(), "a heading is not chosen");
        assert_eq!(Item::head("H").tick, None);
        let ticked = Item::ticked("T", 2, false);
        assert_eq!((ticked.tick, ticked.enabled, ticked.kind), (Some(false), true, Kind::Choice(2)));
        assert!(!dis.selectable(), "an off row is not chosen");
        assert!(!sub.selectable(), "an empty submenu is not chosen");
        assert_eq!((sub.enabled, sub.kind), (false, Kind::Sub(vec![])));
        let on = Item::sub("S", vec![Item::choice("A", 0)]);
        assert!(on.selectable(), "a submenu with a list can be chosen");
    }

    #[test]
    fn opens_on_the_first_row_that_can_be_chosen() {
        let mut m = menu();
        m.push("t".into(), sample());
        assert_eq!(at(&m), Some(1), "a heading and an off row are skipped");
        let mut m = menu();
        m.push("t".into(), vec![Item::head("H"), Item::head("G")]);
        assert_eq!(at(&m), Some(0), "nothing to choose: the first row");
        let mut m = menu();
        m.push("t".into(), vec![]);
        assert_eq!(at(&m), None, "no rows");
        assert_eq!(m.press(0), Outcome::Stay, "a press on an empty list");
    }

    #[test]
    fn the_cursor_steps_along_the_rows_that_can_be_chosen() {
        let mut m = menu();
        m.push("t".into(), sample());
        m.step(1);
        assert_eq!(at(&m), Some(2), "over the off row to Curve B");
        m.step(1);
        assert_eq!(at(&m), Some(4), "to the submenu");
        m.step(1);
        assert_eq!(at(&m), Some(4), "held at the end");
        m.step(-1);
        assert_eq!(at(&m), Some(2));
        m.step(2);
        assert_eq!(at(&m), Some(4), "over the off row, held at the end");
        m.step(-99);
        assert_eq!(at(&m), Some(1), "held at the start");
    }

    #[test]
    fn step_stands_still_with_nothing_to_choose() {
        let mut m = menu();
        m.push("t".into(), vec![Item::head("H"), Item::choice("X", 1).off()]);
        assert_eq!(at(&m), Some(0));
        m.step(1);
        assert_eq!(at(&m), Some(0), "no row can be chosen");
        let mut m = menu();
        m.push("t".into(), vec![Item::choice("Only", 1)]);
        m.step(5);
        m.step(-5);
        assert_eq!(at(&m), Some(0), "one row: held");
    }

    #[test]
    fn a_press_chooses_opens_or_does_nothing() {
        let mut m = menu();
        m.push("t".into(), sample());
        assert_eq!(m.press(5), Outcome::Stay, "an out-of-range row");
        assert_eq!(m.press(3), Outcome::Stay, "an off row");
        assert_eq!(m.press(0), Outcome::Stay, "a heading");
        assert_eq!(at(&m), Some(1), "none of those moved the cursor");
        assert_eq!(m.press(1), Outcome::Choose(0), "a ticked choice chooses its number");
        assert!(m.levels.is_empty(), "choosing closed the menu");
    }

    #[test]
    fn a_submenu_opens_its_list_and_back_returns() {
        let mut m = menu();
        m.push("t".into(), vec![Item::sub("More", vec![Item::choice("Inner", 7)])]);
        assert_eq!(m.press(0), Outcome::Stay, "opening is not choosing");
        assert_eq!(at(&m), Some(0), "the inner row is chosen");
        assert_eq!(m.labels(), vec![("Inner".to_string(), None, true)]);
        assert_eq!(m.back(), Outcome::Stay, "back goes up a level");
        assert_eq!(m.labels(), vec![("More".to_string(), None, true)]);
        assert_eq!(m.back(), Outcome::Close, "back at the first closes");
        assert!(m.labels().is_empty());
    }

    #[test]
    fn tap_presses_the_view_row_and_closes_with_a_choice() {
        let mut m = menu();
        m.push("t".into(), vec![Item::choice("A", 5), Item::head("H"), Item::choice("B", 6)]);
        assert_eq!(m.tap(2), Outcome::Choose(6), "row 2 of the view");
        assert!(m.labels().is_empty(), "the choice closed it");
        let mut m = menu();
        m.push("t".into(), vec![Item::choice("A", 5), Item::head("H")]);
        assert_eq!(m.tap(1), Outcome::Stay, "a heading in the view");
    }

    #[test]
    fn confirm_chooses_the_cursor_and_cancel_goes_back() {
        let mut m = menu();
        m.push("t".into(), sample());
        assert_eq!(m.act(Action::Confirm(None)), Outcome::Choose(0), "confirm presses the cursor's row");
        assert!(m.labels().is_empty());
        let mut m = menu();
        m.push("t".into(), sample());
        assert_eq!(m.act(Action::Cancel(None)), Outcome::Close);
        assert!(m.labels().is_empty());
        let mut m = menu();
        assert_eq!(m.act(Action::Confirm(None)), Outcome::Stay, "nothing open");
    }

    #[test]
    fn the_arrows_move_and_end_jumps_to_an_end() {
        let mut m = menu();
        m.push("t".into(), sample());
        assert_eq!(m.act(Action::Arrow { dx: 1, dy: 0 }), Outcome::Stay, "an arrow step");
        assert_eq!(at(&m), Some(2));
        m.act(Action::End(1));
        assert_eq!(at(&m), Some(4), "End 1 to the last row that can be chosen");
        m.act(Action::End(-1));
        assert_eq!(at(&m), Some(1), "End -1 to the first");
        m.act(Action::End(1));
        assert_eq!(at(&m), Some(4));
        m.act(Action::End(1));
        assert_eq!(at(&m), Some(4), "End past the end is held");
    }

    #[test]
    fn a_page_turns_the_view_and_holds_at_the_ends() {
        let items: Vec<Item> = (0..5).map(|i| Item::choice(format!("R{i}"), i)).collect();
        let mut m = menu();
        m.push("t".into(), items);
        assert_eq!(at(&m), Some(0));
        m.turn(1);
        assert_eq!(at(&m), Some(1), "a page lands on the first row that can be chosen of that view");
        m.turn(99);
        assert_eq!(at(&m), Some(4), "past the last view: held at the end");
        m.turn(-99);
        assert_eq!(at(&m), Some(0), "past the first: held at the start");
    }

    #[test]
    fn start_moves_the_cursor_only_to_a_row_that_can_be_chosen() {
        let mut m = menu();
        m.push("t".into(), sample());
        m.start(3);
        assert_eq!(at(&m), Some(1), "an off row is not a start");
        m.start(2);
        assert_eq!(at(&m), Some(2));
        m.start(9);
        assert_eq!(at(&m), Some(2), "out of range: unchanged");
        m.start(0);
        assert_eq!(at(&m), Some(2), "a heading is not a start");
    }

    #[test]
    fn a_detached_menu_has_no_window_and_leaves_empty_titles_out_of_the_path() {
        let m = menu();
        assert_eq!(m.per_view(), 1, "no window: one row a view");
        let mut m = menu();
        m.push("".into(), sample());
        m.push("Second".into(), sample());
        assert_eq!(m.path(), "Second", "empty titles are left out, the rest joined");
        let mut m = menu();
        m.push("First".into(), sample());
        m.push("Second".into(), sample());
        assert_eq!(m.path(), "First › Second");
    }
}
