// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's sidebar (LibraryPane.cpp's left column, ui/LIBRARY-APP.md A5 to A8): Favorites and Recent, then the
//! libraries (All, My own, each one installed, with its count), then the tags with their counts. One line an entry,
//! the ones chosen ticked; a choice narrows the list at once (the engine is asked again), several tags may be chosen
//! and all must match. It has its own cursor and view, and its own scroll bar for touch.

use super::*;
use crate::nav::Cursor;

/// What a line of the sidebar is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Side {
    Heading,
    Favorites,
    Recent,
    /// Library `n` of `Filters::sources`.
    Source(usize),
    /// Tag `n` of `Filters::tag_list`.
    Tag(usize),
    /// Said where there is nothing to choose.
    Empty,
}

impl Side {
    fn pickable(self) -> bool {
        !matches!(self, Side::Heading | Side::Empty)
    }
}

impl Screen {
    /// Whether the sidebar is at the left: with the kinds' list.
    pub(super) fn side_visible(&self) -> bool {
        self.mode == Mode::List
    }

    /// The lines of the sidebar, each with what it is.
    pub(super) fn side_entries(&self) -> Vec<(Side, SettingData)> {
        let head = |t: &str| (Side::Heading, SettingData { label: t.into(), kind: 4, ..Default::default() });
        let line = |side: Side, label: &str, count: String, on: bool, tint: Option<slint::Color>| {
            let value = if on { format!("✓ {count}").trim_end().to_string() } else { count };
            (side, SettingData { label: label.into(), value: value.into(), kind: 2, on, fluid: true, tint: tint.unwrap_or_default(), tinted: tint.is_some(), ..Default::default() })
        };
        let mut out = vec![line(Side::Favorites, "Favorites", String::new(), self.filters.favorites, None), line(Side::Recent, "Recent", String::new(), self.filters.recent, None), head("Libraries")];
        for (i, (id, name, items)) in self.filters.sources(&self.libraries).into_iter().enumerate() {
            let name = if id.is_empty() { "All".to_string() } else { name };
            out.push(line(Side::Source(i), &name, items.map(|n| n.to_string()).unwrap_or_default(), id == self.filters.source, None));
        }
        out.push(head("Tags"));
        let tags = self.filters.tag_list(&self.tags);
        if tags.is_empty() {
            out.push((Side::Empty, SettingData { label: "No tags".into(), kind: 4, ..Default::default() }));
        }
        for (i, (tag, count)) in tags.iter().enumerate() {
            out.push(line(Side::Tag(i), tag, count.to_string(), self.filters.chosen.contains(tag), self.tag_color(tag)));
        }
        out
    }

    /// The sidebar's view written to the window, each line only when it changed.
    pub(super) fn show_side(&mut self) {
        let Some(ui) = self.ui.upgrade().filter(|_| self.active) else { return };
        let lib = ui.global::<LibraryState>();
        lib.set_side_rows(ModelRc::from(self.side_rows.clone()));
        let entries = if self.side_visible() { self.side_entries() } else { vec![] };
        let (per, n) = (lib.get_side_per_view().max(1) as usize, entries.len());
        let at = self.side_cursor.at.map(|a| a.min(n.saturating_sub(1))).filter(|_| n > 0);
        let last = n.saturating_sub(1) / per * per;
        let first = match at {
            Some(a) if self.side_focus => a / per * per,
            _ => (self.side_cursor.first / per * per).min(last),
        };
        self.side_cursor = Cursor { at, count: n, per_page: per, first };
        let (focus, at) = (self.side_focus, at);
        let shown: Vec<SettingData> = entries.into_iter().enumerate().skip(first).take(per).map(|(i, (_, d))| SettingData { cursor: focus && at == Some(i), ..d }).collect();
        crate::setlist::update(&self.side_rows, shown);
        lib.set_side_total(n as i32);
        lib.set_side_first(first as i32);
    }

    /// The cursor to the sidebar (the one cursor leaves the list or the row of tabs), on the line it was on, else the first.
    pub(super) fn enter_side(&mut self) {
        let entries = self.side_entries();
        self.tab_at = None;
        self.b.cursor = self.b.cursor.clear();
        self.side_focus = true;
        let on = self.side_cursor.at.filter(|a| entries.get(*a).is_some_and(|e| e.0.pickable()));
        self.side_cursor.at = on.or_else(|| entries.iter().position(|e| e.0.pickable()));
    }

    /// The cursor back to the list, on the first line of its view.
    pub(super) fn leave_side(&mut self) {
        self.side_focus = false;
        self.b.cursor = self.b.cursor.moved(0);
    }

    /// The cursor `by` lines down the choices (headings are passed over); up from the first reaches the row of tabs.
    fn side_move(&mut self, by: i64) {
        let entries = self.side_entries();
        let pick: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e.0.pickable()).map(|(i, _)| i).collect();
        if pick.is_empty() {
            return;
        }
        let now = self.side_cursor.at.and_then(|a| pick.iter().position(|&p| p == a));
        let to = match now {
            None => 0,
            Some(0) if by < 0 => {
                self.side_focus = false;
                self.tab_at = Some(self.tab_current());
                return;
            }
            Some(k) => (k as i64 + by).clamp(0, pick.len() as i64 - 1) as usize,
        };
        self.side_cursor.at = Some(pick[to]);
    }

    /// A navigation action while the cursor is on the sidebar: the dial and the arrows walk it, right goes to the list.
    pub(super) fn act_side(&mut self, action: Action) -> Outcome {
        let per = self.side_cursor.per_page as i64;
        match action {
            Action::Move { pane: Some(1), .. } => {
                self.leave_side();
                let nav = self.b.nav(action);
                return self.handle(nav);
            }
            Action::Move { by, .. } => self.side_move(i64::from(by)),
            Action::Arrow { dx, dy } if dx > 0 && dy == 0 => self.leave_side(),
            Action::Arrow { dx: 0, dy } => self.side_move(i64::from(dy)),
            Action::Page(by) => self.side_move(i64::from(by) * per),
            Action::End(by) => self.side_move(i64::from(by) * 10_000),
            Action::Confirm(_) => {
                if let Some(at) = self.side_cursor.at {
                    self.side_press(at);
                }
            }
            _ => {}
        }
        self.show();
        Outcome::Stay
    }

    /// A line chosen: what it narrows to is asked of the engine at once (choosing applies; opening a Performance still waits for a press).
    pub(super) fn side_press(&mut self, at: usize) {
        let Some((side, _)) = self.side_entries().into_iter().nth(at) else { return };
        match side {
            Side::Favorites => self.filters.favorites = !self.filters.favorites,
            Side::Recent => self.filters.recent = !self.filters.recent,
            Side::Source(i) => {
                let Some((id, _, _)) = self.filters.sources(&self.libraries).get(i).cloned() else { return };
                self.filters.source = id;
                // Which libraries are installed, asked again: one gone while chosen is let go (LibraryPane.cpp `timerCallback`).
                self.push(Request::Libraries, "library.libraries", json!({}));
            }
            Side::Tag(i) => {
                let Some((tag, _)) = self.filters.tag_list(&self.tags).get(i).cloned() else { return };
                self.filters.toggle_tag(&tag);
            }
            Side::Heading | Side::Empty => return,
        }
        self.b.reset_cursor();
        self.query();
    }

    /// A touch on line `row` of the view: the cursor drops, the line is chosen.
    pub fn side_tap(&mut self, row: i32) {
        self.tab_at = None;
        self.side_focus = false;
        self.b.cursor = self.b.cursor.clear();
        self.side_press(self.side_cursor.first + row.max(0) as usize);
        self.show();
    }

    /// The scroll bar's request: the view starts at line `first` (a touch only: the cursor drops).
    pub fn side_scroll_to(&mut self, first: i32) {
        self.side_focus = false;
        let per = self.side_cursor.per_page.max(1);
        let last = self.side_cursor.count.saturating_sub(1) / per * per;
        self.side_cursor.first = (first.max(0) as usize / per * per).min(last);
        self.show();
    }

    /// The tag under the sidebar's cursor: what Options is about there.
    pub(super) fn tag_under_cursor(&self) -> Option<String> {
        if !self.side_focus {
            return None;
        }
        match self.side_entries().get(self.side_cursor.at?)?.0 {
            Side::Tag(i) => self.filters.tag_list(&self.tags).get(i).map(|t| t.0.clone()),
            _ => None,
        }
    }
}
