// SPDX-License-Identifier: AGPL-3.0-or-later
//! Navigation: what the musician asks of a screen, the same whatever asked it. Three sources make
//! these actions: touch (a tap moves the cursor there and confirms), a computer keyboard
//! (`from_key`), and a device's panel controls through the roles its panel map names
//! (`Roles`, the map's `navigation`). Every action is reachable without touch. The abilities
//! are universal; which control plays which role is the device's map.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// A cursor moves by steps: pane 0 is the focused list, 1 and 2 a two-pane screen's left
    /// and right (an encoder's turn). Nothing opens until Confirm.
    Move { pane: u8, by: i32 },
    /// Cursor keys: across and down, a grid's columns and rows, a list's rows.
    Arrow { dx: i32, dy: i32 },
    /// A page back or forward.
    Page(i32),
    /// Previous or Next, where the screen has them (Setlist: open the slot before or after).
    Step(i32),
    Confirm,
    Cancel,
    Home,
}

/// A computer keyboard's key (Slint's key text): arrows move the cursor, Enter confirms, Escape
/// cancels, Tab pages, Page Up and Page Down step (page turners and foot switches send them).
pub fn from_key(text: &str, shift: bool) -> Option<Action> {
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
    } else if is(Key::Return) || text == "\n" || text == "\r" {
        Action::Confirm
    } else if is(Key::Escape) {
        Action::Cancel
    } else if is(Key::Home) {
        Action::Home
    } else if is(Key::Backtab) || (is(Key::Tab) && shift) {
        Action::Page(-1)
    } else if is(Key::Tab) {
        Action::Page(1)
    } else if is(Key::PageUp) {
        Action::Step(-1)
    } else if is(Key::PageDown) {
        Action::Step(1)
    } else {
        return None;
    })
}

/// The panel map's `navigation` (docs/panel-map.schema.json): each role and the controls (by
/// their identity in the map, `screen:1`, `data`, `enter`) that play it.
#[derive(Default, Debug)]
pub struct Roles(Vec<(String, String)>);

impl Roles {
    pub fn parse(map: &Value) -> Roles {
        let mut roles = Vec::new();
        if let Some(nav) = map["navigation"].as_object() {
            for (role, controls) in nav {
                for c in controls.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    roles.push((c.to_string(), role.clone()));
                }
            }
        }
        Roles(roles)
    }

    /// A control moved: an encoder by `steps`, a switch pressed (`steps` 1; a release, 0, does
    /// nothing). None for a control that plays no role.
    pub fn action(&self, control: &str, steps: i32) -> Option<Action> {
        let role = &self.0.iter().find(|(c, _)| c == control)?.1;
        if steps == 0 {
            return None;
        }
        Some(match role.as_str() {
            "cursor" => Action::Move { pane: 0, by: steps },
            "pane1" => Action::Move { pane: 1, by: steps },
            "pane2" => Action::Move { pane: 2, by: steps },
            "up" => Action::Arrow { dx: 0, dy: -1 },
            "down" => Action::Arrow { dx: 0, dy: 1 },
            "left" => Action::Arrow { dx: -1, dy: 0 },
            "right" => Action::Arrow { dx: 1, dy: 0 },
            "pageBack" => Action::Page(-1),
            "pageForward" => Action::Page(1),
            "previous" => Action::Step(-1),
            "next" => Action::Step(1),
            "confirm" => Action::Confirm,
            "cancel" => Action::Cancel,
            "home" => Action::Home,
            _ => return None,
        })
    }
}

/// A cursor over `count` items shown `per_page` at a time: it moves and wraps nowhere (it stops
/// at either end), and the page shown follows it by whole pages, never scrolling, so a move
/// redraws the two items it leaves and reaches, or one page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub at: usize,
    pub count: usize,
    pub per_page: usize,
}

impl Cursor {
    pub fn new(count: usize, per_page: usize) -> Cursor {
        Cursor { at: 0, count, per_page: per_page.max(1) }
    }
    pub fn moved(self, by: i64) -> Cursor {
        if self.count == 0 {
            return self;
        }
        Cursor { at: (self.at as i64 + by).clamp(0, self.count as i64 - 1) as usize, ..self }
    }
    pub fn to(self, at: usize) -> Cursor {
        if self.count == 0 {
            return self;
        }
        Cursor { at: at.min(self.count - 1), ..self }
    }
    /// The first item of the page the cursor is on.
    pub fn first(self) -> usize {
        self.at / self.per_page * self.per_page
    }
    pub fn pages(self) -> usize {
        self.count.div_ceil(self.per_page).max(1)
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
        assert_eq!(from_key(&k(Key::DownArrow), false), Some(Action::Arrow { dx: 0, dy: 1 }));
        assert_eq!(from_key(&k(Key::Return), false), Some(Action::Confirm));
        assert_eq!(from_key(&k(Key::Tab), true), Some(Action::Page(-1)));
        assert_eq!(from_key(&k(Key::PageDown), false), Some(Action::Step(1)));
        assert_eq!(from_key("a", false), None);
    }

    #[test]
    fn panel_roles_from_the_map() {
        let map = json!({"navigation": {"cursor": ["data"], "pane1": ["screen:1", "screen:2"], "pane2": ["screen:6"], "confirm": ["enter"], "odd": ["x"], "cancel": "exit"}});
        let r = Roles::parse(&map);
        assert_eq!(r.action("data", -3), Some(Action::Move { pane: 0, by: -3 }));
        assert_eq!(r.action("screen:2", 1), Some(Action::Move { pane: 1, by: 1 }));
        assert_eq!(r.action("screen:6", 2), Some(Action::Move { pane: 2, by: 2 }));
        assert_eq!((r.action("enter", 1), r.action("enter", 0)), (Some(Action::Confirm), None)); // pressed, released
        assert_eq!((r.action("x", 1), r.action("exit", 1), r.action("knob:1", 5)), (None, None, None)); // an unknown role, not a list, no role
        assert!(Roles::parse(&json!({})).action("data", 1).is_none());
    }

    #[test]
    fn cursor_pages_and_stops_at_the_ends() {
        let c = Cursor::new(25, 10);
        assert_eq!((c.moved(-1).at, c.moved(9).first(), c.moved(10).first(), c.moved(99).at, c.pages()), (0, 0, 10, 24, 3));
        assert_eq!(c.to(99).at, 24);
        let empty = Cursor::new(0, 10);
        assert_eq!((empty.moved(3).at, empty.to(5).at, empty.pages()), (0, 0, 1));
    }
}
