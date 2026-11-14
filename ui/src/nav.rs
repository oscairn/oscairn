// SPDX-License-Identifier: AGPL-3.0-or-later
//! Navigation: what the musician asks of the lists on a screen, the same whatever asked it, as
//! the JUCE app's ListCursor and ListAction have it (hosts/standalone/Theme.h, ListAction.h).
//! Three sources make these actions: touch (a tap does what confirming does), a computer keyboard
//! (`from_key`), and a device's panel: its map says which control does what in a list
//! (`navigate`, docs/panel-map.schema.json), the engine counts what they did, and the shell reads
//! the totals from `controllers` (`Totals`). The abilities are universal; the map is the device's.

use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// A list's cursor moves by steps: in pane N from the left (None: the pane last touched).
    /// Nothing opens until Confirm.
    Move { pane: Option<u8>, by: i32 },
    /// Cursor keys: across and down, a grid's columns and rows.
    Arrow { dx: i32, dy: i32 },
    /// A page towards the start (-1) or the end (1).
    Page(i32),
    /// The first (-1) or last (1) item.
    End(i32),
    /// What a tap on the cursor's item does.
    Confirm(Option<u8>),
    /// Drops the cursor; with none, goes back.
    Cancel(Option<u8>),
}

/// A computer keyboard's key (Slint's key text): arrows, Page Up and Down, Home and End, Return
/// and Escape, as the JUCE lists take them.
pub fn from_key(text: &str) -> Option<Action> {
    use slint::platform::Key;
    let is = |k: Key| text == slint::SharedString::from(k).as_str();
    Some(if is(Key::UpArrow) {
        Action::Arrow { dx: 0, dy: -1 }
    } else if is(Key::DownArrow) {
        Action::Arrow { dx: 0, dy: 1 }
    } else if is(Key::LeftArrow) {
        Action::Arrow { dx: -1, dy: 0 }
    } else if is(Key::RightArrow) {
        Action::Arrow { dx: 1, dy: 0 }
    } else if is(Key::PageUp) {
        Action::Page(-1)
    } else if is(Key::PageDown) {
        Action::Page(1)
    } else if is(Key::Home) {
        Action::End(-1)
    } else if is(Key::End) {
        Action::End(1)
    } else if is(Key::Return) || text == "\n" || text == "\r" {
        Action::Confirm(None)
    } else if is(Key::Escape) {
        Action::Cancel(None)
    } else {
        return None;
    })
}

/// The panel's navigation as the engine counts it (`controllers.get`'s `navigation`): totals since
/// the engine started, by key (`move`, `move:N`, `confirm[:N]`, `cancel[:N]`). The first read sets
/// the baseline; each later one gives the actions since the last.
#[derive(Default)]
pub struct Totals(Option<HashMap<String, i64>>);

impl Totals {
    pub fn read(&mut self, navigation: &Value) -> Vec<Action> {
        let Some(now) = navigation.as_object() else { return Vec::new() };
        let now: HashMap<String, i64> = now.iter().filter_map(|(k, v)| Some((k.clone(), v.as_i64()?))).collect();
        let Some(last) = self.0.replace(now.clone()) else { return Vec::new() };
        let mut actions = Vec::new();
        for (key, total) in now {
            let delta = total - last.get(&key).copied().unwrap_or(0);
            if delta == 0 {
                continue;
            }
            let (what, pane) = key.split_once(':').map_or((key.as_str(), None), |(w, n)| (w, n.parse::<u8>().ok()));
            match what {
                "move" => actions.push(Action::Move { pane, by: delta.clamp(-1000, 1000) as i32 }),
                // A count gone back (an engine started again) is nothing to press.
                "confirm" if delta > 0 => actions.extend((0..delta.min(4)).map(|_| Action::Confirm(pane))),
                "cancel" if delta > 0 => actions.extend((0..delta.min(4)).map(|_| Action::Cancel(pane))),
                _ => {}
            }
        }
        actions
    }

    /// A new connection's first read is a baseline again (the engine may have started again).
    pub fn forget(&mut self) {
        self.0 = None;
    }
}

/// A list's cursor, apart from any selection: none until the first move, which shows it on the
/// first item shown; it stops at either end; the page shown follows it by whole pages, never
/// scrolling, so a move redraws the two items it leaves and reaches, or one page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub at: Option<usize>,
    pub count: usize,
    pub per_page: usize,
    /// The first item of the page shown.
    pub first: usize,
}

impl Cursor {
    pub fn new(count: usize, per_page: usize) -> Cursor {
        Cursor { at: None, count, per_page: per_page.max(1), first: 0 }
    }
    pub fn moved(self, by: i64) -> Cursor {
        if self.count == 0 {
            return self;
        }
        let at = match self.at {
            None => self.first.min(self.count - 1), // the first move shows it
            Some(at) => (at as i64 + by).clamp(0, self.count as i64 - 1) as usize,
        };
        self.to(at)
    }
    /// The cursor on `at` (a tap's item), the page following it.
    pub fn to(self, at: usize) -> Cursor {
        if self.count == 0 {
            return self;
        }
        let at = at.min(self.count - 1);
        Cursor { at: Some(at), first: at / self.per_page * self.per_page, ..self }
    }
    pub fn clear(self) -> Cursor {
        Cursor { at: None, ..self }
    }
    /// A page back or on, with the cursor (if any) on the same place in it.
    pub fn paged(self, by: i64) -> Cursor {
        let pages = self.count.div_ceil(self.per_page).max(1) as i64;
        let page = (self.first / self.per_page) as i64;
        let first = ((page + by).clamp(0, pages - 1) as usize) * self.per_page;
        match self.at {
            Some(at) => self.to(first + at % self.per_page),
            None => Cursor { first, ..self },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys() {
        use slint::platform::Key;
        let k = |key: Key| slint::SharedString::from(key).to_string();
        assert_eq!(from_key(&k(Key::DownArrow)), Some(Action::Arrow { dx: 0, dy: 1 }));
        assert_eq!(from_key(&k(Key::Return)), Some(Action::Confirm(None)));
        assert_eq!(from_key(&k(Key::PageUp)), Some(Action::Page(-1)));
        assert_eq!(from_key(&k(Key::End)), Some(Action::End(1)));
        assert_eq!(from_key("a"), None);
    }

    #[test]
    fn panel_totals_become_actions_once() {
        let mut t = Totals::default();
        assert!(t.read(&json!({"move": 40, "confirm": 3})).is_empty(), "the first read: a baseline");
        let mut a = t.read(&json!({"move": 42, "move:1": -3, "confirm": 4, "cancel:0": 1}));
        a.sort_by_key(|x| format!("{x:?}"));
        assert_eq!(a, [Action::Cancel(Some(0)), Action::Confirm(None), Action::Move { pane: None, by: 2 }, Action::Move { pane: Some(1), by: -3 }]);
        assert!(t.read(&json!({"move": 42, "move:1": -3, "confirm": 4, "cancel:0": 1})).is_empty(), "nothing new");
        assert!(t.read(&json!({"move": 1, "confirm": 0, "odd": "x"})).iter().all(|x| matches!(x, Action::Move { .. })), "an engine started again: no presses");
        t.forget();
        assert!(t.read(&json!({"move": 99})).is_empty());
        assert!(t.read(&json!(null)).is_empty());
    }

    #[test]
    fn cursor_shows_on_the_first_move_pages_and_stops_at_the_ends() {
        let c = Cursor::new(25, 10);
        assert_eq!(c.moved(5).at, Some(0), "the first move shows it where the page starts");
        let c = c.moved(1).moved(9);
        assert_eq!((c.at, c.first), (Some(9), 0));
        let c = c.moved(1);
        assert_eq!((c.at, c.first), (Some(10), 10), "past the page: the next page");
        assert_eq!(c.moved(99).at, Some(24));
        assert_eq!(c.paged(1).at, Some(20));
        assert_eq!(c.paged(9).first, 20);
        assert_eq!(Cursor::new(25, 10).paged(1), Cursor { at: None, count: 25, per_page: 10, first: 10 });
        assert_eq!(c.clear().at, None);
        let empty = Cursor::new(0, 10);
        assert_eq!((empty.moved(3).at, empty.to(5).at), (None, None));
    }
}
