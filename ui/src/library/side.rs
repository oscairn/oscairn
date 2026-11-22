// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library's sidebar (LibraryPane.cpp's left column, ui/LIBRARY-APP.md A5 to A8): Favorites and Recent, then the
//! libraries (All, My own, each one installed, with its count), then the tags with their counts. One line an entry,
//! or, as bubbles, entries that wrap along a line (the JUCE pane's pills); the ones chosen ticked or lit; a choice narrows
//! the list at once (the engine is asked again), several tags may be chosen and all must match. It has its own cursor and
//! view, and its own scroll bar for touch.

use super::*;
use crate::nav::Cursor;
use slint::Model;

/// What a line of the sidebar is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Side {
    /// A section's heading (0 the libraries, 1 the tags): a press folds the section.
    Heading(usize),
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
        !matches!(self, Side::Empty)
    }

    /// Alone on its line, however the entries are laid out.
    fn whole_line(self) -> bool {
        matches!(self, Side::Heading(_) | Side::Empty)
    }
}

/// An entry with its place: the line it is on and, along the line, where it starts and how wide it is, the line's top
/// in the list and the line's height (logical pixels; a heading line is compact, a bubble line a touch target).
#[derive(Clone)]
pub(super) struct Cell {
    pub side: Side,
    pub data: SettingData,
    pub line: usize,
    pub x: f32,
    pub w: f32,
    pub top: f32,
    pub h: f32,
}

/// How wide `text` is at `size`, estimated from the typeface's proportions (Inter): a narrow letter, a wide one, a capital, the rest.
/// Bubbles are laid out here, before the window draws them, with a little over what the text needs (a name that still does not
/// fit shrinks to 70% in its bubble, then is cut).
pub(super) fn text_width(text: &str, size: f32) -> f32 {
    let em: f32 = text
        .chars()
        .map(|c| match c {
            ' ' | 'i' | 'j' | 'l' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' => 0.28,
            'f' | 't' | 'r' | 'I' | '1' | '-' | '(' | ')' => 0.38,
            'm' | 'M' | 'W' | 'w' | '@' => 0.86,
            c if c.is_ascii_uppercase() => 0.68,
            c if c.is_ascii_digit() => 0.6,
            _ => 0.57,
        })
        .sum();
    em * size * 1.06
}

impl Screen {
    /// Whether the sidebar is at the left: with the kinds' list.
    pub(super) fn side_visible(&self) -> bool {
        self.mode == Mode::List && !self.info_on
    }

    /// The lines of the sidebar, each with what it is.
    pub(super) fn side_entries(&self) -> Vec<(Side, SettingData)> {
        let head = |n: usize, t: &str| (Side::Heading(n), SettingData { label: t.into(), kind: 4, fluid: true, value: if self.folded[n] { "closed" } else { "open" }.into(), ..Default::default() });
        // A line: `count` is what the list shows, `bubble` the count shown after a bubble's name. `value` keeps the check
        // the list draws on a chosen line; `bubble` is a library's items (with the check when it is chosen), empty for the
        // Favorites, Recent and the tags, whose bubbles show no count.
        let line = |side: Side, label: &str, count: String, on: bool, tint: Option<slint::Color>, bubble: String| {
            let value = if on { format!("✓ {count}").trim_end().to_string() } else { count };
            (side, SettingData { label: label.into(), value: value.into(), count: bubble.into(), kind: 2, on, fluid: true, tint: tint.unwrap_or_default(), tinted: tint.is_some(), ..Default::default() })
        };
        let mut out = vec![line(Side::Favorites, "Favorites", String::new(), self.filters.favorites, None, String::new()), line(Side::Recent, "Recent", String::new(), self.filters.recent, None, String::new()), head(0, "Libraries")];
        for (i, (id, name, items)) in self.filters.sources(&self.libraries).into_iter().enumerate().filter(|_| !self.folded[0]) {
            let name = if id.is_empty() { "All".to_string() } else { name };
            let n = items.map(|n| n.to_string()).unwrap_or_default();
            let bubble = if id == self.filters.source { format!("✓ {n}").trim_end().to_string() } else { n.clone() };
            out.push(line(Side::Source(i), &name, n, id == self.filters.source, None, bubble));
        }
        out.push(head(1, "Tags"));
        let tags = self.filters.tag_list(&self.tags);
        if self.folded[1] {
            return out;
        }
        if tags.is_empty() {
            out.push((Side::Empty, SettingData { label: "No tags".into(), kind: 4, ..Default::default() }));
        }
        for (i, (tag, count)) in tags.iter().enumerate() {
            out.push(line(Side::Tag(i), tag, count.to_string(), self.filters.chosen.contains(tag), self.tag_color(tag), String::new()));
        }
        out
    }

    /// What the layout needs of the window: the width the entries share, the padding, the gap between bubbles, the type and a
    /// touch target (the window's own numbers; the usual ones where there is no window).
    fn side_metrics(&self) -> (f32, f32, f32, f32, f32, f32, f32) {
        let Some(ui) = self.ui.upgrade() else { return (280., 10., 4., 16., 13., 4., 44.) };
        let t = ui.global::<crate::Theme>();
        (ui.global::<LibraryState>().get_side_room(), t.get_pad(), t.get_gap(), t.get_body(), t.get_small(), t.get_pad_tight(), t.get_touch())
    }

    /// The entries with their places: a line each, or as bubbles that wrap (a heading, and the line of "No tags", alone on theirs).
    /// A heading's line is a compact label high, a bubble's line a touch target; both are placed top to bottom here, in the same
    /// order as the window draws them.
    pub(super) fn side_cells(&self) -> Vec<Cell> {
        let entries = self.side_entries();
        let (room, pad, gap, body, small, pad_tight, touch) = self.side_metrics();
        if !self.bubbles {
            return entries.into_iter().enumerate().map(|(i, (side, data))| Cell { side, data, line: i, x: 0., w: room, top: 0., h: touch }).collect();
        }
        let step = self.ui.upgrade().map_or(gap, |ui| ui.global::<crate::Theme>().invoke_dense(gap, self.compact));
        let head_h = small + 2. * pad_tight;
        let (mut line, mut x, mut top, mut line_h, mut open) = (0, 0f32, 0f32, 0f32, false);
        let mut out = Vec::with_capacity(entries.len());
        for (side, data) in entries {
            let h = if side.whole_line() { head_h } else { touch };
            if side.whole_line() {
                if open {
                    top += line_h + step;
                    line += 1;
                    open = false;
                }
                out.push(Cell { side, data, line, x: 0., w: room, top, h });
                top += h + step;
                line += 1;
                x = 0.;
                continue;
            }
            let dot = if data.tinted { small * 0.7 + gap } else { 0. };
            // A library's bubble also holds its count (the caption size), a gap after the name; the name is the part that
            // gives when the whole bubble does not fit, so the count keeps its room.
            let count = if data.count.is_empty() { 0. } else { gap + text_width(&data.count, small) };
            let w = (text_width(&data.label, body) + 2. * pad + dot + count).clamp(touch, room.max(touch));
            if open && x + w > room + 0.01 {
                top += line_h + step;
                line += 1;
                x = 0.;
            }
            if !open {
                line_h = h;
            }
            out.push(Cell { side, data, line, x, w, top, h });
            x += w + gap;
            open = true;
        }
        out
    }

    /// The sidebar's view written to the window, each line only when it changed.
    pub(super) fn show_side(&mut self) {
        let Some(ui) = self.ui.upgrade().filter(|_| self.active) else { return };
        let lib = ui.global::<LibraryState>();
        lib.set_bubbles(self.bubbles);
        lib.set_compact(self.compact);
        ui.global::<crate::PartState>().set_compact(self.compact);
        lib.set_side_rows(ModelRc::from(self.side_rows.clone()));
        lib.set_side_geo(ModelRc::from(self.side_geo.clone()));
        let cells = if self.side_visible() { self.side_cells() } else { vec![] };
        let n = cells.len();
        let lines = cells.last().map_or(0, |c| c.line + 1);
        let at = self.side_cursor.at.map(|a| a.min(n.saturating_sub(1))).filter(|_| n > 0);
        let last = lines.saturating_sub(1);
        // The first line of the view: the one the cursor is on is in it, else where the view was.
        let first = match at {
            Some(a) if self.side_focus => cells[a].line / self.side_cursor.per_page.max(1) * self.side_cursor.per_page.max(1),
            _ => self.side_cursor.first.min(last),
        };
        // How many lines fit: the sidebar's lines vary (a heading is compact, a bubble a touch target), so a view fits the
        // lines from its own first, not the list's first: a later view, all touch-tall bubbles, holds fewer lines than the
        // first with its compact headings. The Rust layout is the source of the count (read back here), so nothing a view
        // shows is drawn past the view's bottom at any scroll position.
        let side = lib.get_side_height();
        let (per, view_h) = if self.bubbles {
            let (mut fit, mut last_line, mut base, mut bottom) = (0usize, usize::MAX, 0f32, 0f32);
            for c in &cells {
                if c.line < first {
                    continue;
                }
                if c.line == last_line {
                    continue;
                }
                if fit == 0 {
                    base = c.top;
                }
                if c.top - base + c.h > side + 0.01 {
                    break;
                }
                fit += 1;
                last_line = c.line;
                bottom = c.top - base + c.h;
            }
            (fit.max(1), bottom)
        } else {
            // List view: every line a touch target, one line an entry, as before.
            let t = ui.global::<crate::Theme>();
            let step = t.invoke_dense(t.invoke_gap_between(t.get_gap()), self.compact);
            let per = std::cmp::max(1, ((side + step) / (t.get_touch() + step)) as usize);
            (per, per as f32 * (t.get_touch() + step) - step)
        };
        lib.set_side_per_view(per as i32);
        // The scroll bar's track is as tall as the lines the view shows, not the room they have.
        lib.set_side_view_h(view_h);
        self.side_cursor = Cursor { at, count: n, per_page: per, first };
        self.side_page = cells.iter().position(|c| c.line >= first).unwrap_or(n);
        let (focus, at) = (self.side_focus, at);
        let page: Vec<(usize, &Cell)> = cells.iter().enumerate().skip(self.side_page).take_while(|(_, c)| c.line < first + per).collect();
        let shown: Vec<SettingData> = page.iter().map(|(i, c)| SettingData { cursor: focus && at == Some(*i), ..c.data.clone() }).collect();
        let top0 = page.first().map_or(0., |(_, c)| c.top);
        let geo: Vec<SideGeo> = page.iter().map(|(_, c)| SideGeo { line: (c.line - first) as i32, x: c.x, w: c.w, y: c.top - top0, h: c.h }).collect();
        crate::setlist::update(&self.side_rows, shown);
        if self.side_geo.iter().ne(geo.iter().cloned()) {
            self.side_geo.set_vec(geo);
        }
        lib.set_side_total(lines as i32);
        lib.set_side_first(first as i32);
    }

    /// The cursor to the sidebar (the one cursor leaves the list or the row of tabs), on the line it was on, else the first.
    pub(super) fn enter_side(&mut self) {
        let cells = self.side_cells();
        self.tab_at = None;
        self.b.cursor = self.b.cursor.clear();
        self.side_focus = true;
        let on = self.side_cursor.at.filter(|a| cells.get(*a).is_some_and(|e| e.side.pickable()));
        self.side_cursor.at = on.or_else(|| cells.iter().position(|e| e.side.pickable()));
    }

    /// The cursor back to the list, on the first line of its view.
    pub(super) fn leave_side(&mut self) {
        self.side_focus = false;
        self.b.cursor = self.b.cursor.moved(0);
    }

    /// The cursor to the row of tabs, from the first entry.
    fn side_to_tabs(&mut self) {
        self.side_focus = false;
        self.tab_at = Some(self.tab_current());
    }

    /// The cursor `by` entries on (headings are passed over); up from the first reaches the row of tabs.
    fn side_move(&mut self, by: i64) {
        let cells = self.side_cells();
        let pick: Vec<usize> = cells.iter().enumerate().filter(|(_, e)| e.side.pickable()).map(|(i, _)| i).collect();
        if pick.is_empty() {
            return;
        }
        let now = self.side_cursor.at.and_then(|a| pick.iter().position(|&p| p == a));
        let to = match now {
            None => 0,
            Some(0) if by < 0 => {
                self.side_to_tabs();
                return;
            }
            Some(k) => (k as i64 + by).clamp(0, pick.len() as i64 - 1) as usize,
        };
        self.side_cursor.at = Some(pick[to]);
    }

    /// An arrow among bubbles, to the neighbor where it points: along the line for left and right (right past the last of the
    /// line leaves for the list), to the bubble nearest across on the next line up or down (up from the first line reaches the tabs).
    fn side_arrow(&mut self, dx: i32, dy: i32) {
        let cells = self.side_cells();
        let Some(cur) = self.side_cursor.at.filter(|a| cells.get(*a).is_some_and(|c| c.side.pickable())) else {
            return self.side_move(1);
        };
        let c = &cells[cur];
        if dx != 0 {
            let next = if dx > 0 {
                (cur + 1..cells.len()).find(|&i| cells[i].line == c.line && cells[i].side.pickable())
            } else {
                (0..cur).rev().find(|&i| cells[i].line == c.line && cells[i].side.pickable())
            };
            match next {
                Some(i) => self.side_cursor.at = Some(i),
                None if dx > 0 => self.leave_side(),
                None => {}
            }
            return;
        }
        let line = if dy > 0 {
            (cur + 1..cells.len()).find(|&i| cells[i].line > c.line && cells[i].side.pickable()).map(|i| cells[i].line)
        } else {
            (0..cur).rev().find(|&i| cells[i].line < c.line && cells[i].side.pickable()).map(|i| cells[i].line)
        };
        let Some(line) = line else {
            if dy < 0 {
                self.side_to_tabs();
            }
            return;
        };
        // A heading is the whole line: from it the way down or up is to the nearest the line's start (the first bubble).
        let across = if c.side.whole_line() { c.x } else { c.x + c.w / 2. };
        let near = |i: &usize| ((cells[*i].x + cells[*i].w / 2. - across).abs() * 100.) as i64;
        self.side_cursor.at = (0..cells.len()).filter(|&i| cells[i].line == line && cells[i].side.pickable()).min_by_key(near);
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
            Action::Arrow { dx, dy } if self.bubbles && (dx != 0 || dy != 0) => self.side_arrow(dx.signum(), dy.signum()),
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
        let Some(cell) = self.side_cells().into_iter().nth(at) else { return };
        match cell.side {
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
            Side::Heading(n) => {
                // A section folds or opens: nothing to ask of the engine.
                self.folded[n] = !self.folded[n];
                return;
            }
            Side::Empty => return,
        }
        self.b.reset_cursor();
        self.query();
    }

    /// A touch on entry `row` of the view: the cursor drops, the entry is chosen.
    pub fn side_tap(&mut self, row: i32) {
        self.tab_at = None;
        self.side_focus = false;
        self.b.cursor = self.b.cursor.clear();
        self.side_press(self.side_page + row.max(0) as usize);
        self.show();
    }

    /// The scroll bar's or a finger drag's request: the view starts at line `first` (a touch only: the cursor drops).
    pub fn side_scroll_to(&mut self, first: i32) {
        self.side_focus = false;
        let lines = self.side_cells().last().map_or(0, |c| c.line + 1);
        let last = lines.saturating_sub(1);
        self.side_cursor.first = (first.max(0) as usize).min(last);
        self.show();
    }

    /// The tag under the sidebar's cursor: what Options is about there.
    pub(super) fn tag_under_cursor(&self) -> Option<String> {
        if !self.side_focus {
            return None;
        }
        match self.side_cells().get(self.side_cursor.at?)?.side {
            Side::Tag(i) => self.filters.tag_list(&self.tags).get(i).map(|t| t.0.clone()),
            _ => None,
        }
    }
}
