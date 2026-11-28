// SPDX-License-Identifier: AGPL-3.0-or-later
//! Home (RFC 0030): a tile for each app the profile offers, in the JUCE Launcher's order, each
//! with its icon, name and a line about its state; tiles in reading order under a border cursor
//! that the navigation actions move, nothing opened until it is confirmed (or tapped).

use crate::nav::{Action, Cursor};

/// An app Home offers: its id (the profile's `apps`), its name, its icon (an index into
/// home.slint's icons) and its line when nothing better is known. As Main.cpp's launcherApps.
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: i32,
    pub line: &'static str,
}

pub const APPS: [App; 5] = [
    App { id: "oscairn.performance", name: "Performance", icon: 0, line: "Play or edit it" },
    App { id: "oscairn.library", name: "Library", icon: 1, line: "Performances and Sounds" },
    App { id: "oscairn.setlist", name: "Setlist", icon: 2, line: "Songs in order, for the stage" },
    App { id: "oscairn.recordings", name: "Recordings", icon: 3, line: "Takes and captures" },
    App { id: "oscairn.settings", name: "Settings", icon: 4, line: "Audio, controllers, keys" },
];

pub struct Home {
    /// The apps shown, as indexes into APPS.
    pub tiles: Vec<usize>,
    pub cursor: Cursor,
    pub columns: usize,
}

impl Home {
    /// The apps `offered` names (the profile's; none: a computer's, every one), `columns` across.
    pub fn new(offered: &[String], columns: usize) -> Home {
        let tiles: Vec<usize> = (0..APPS.len()).filter(|&i| offered.is_empty() || offered.iter().any(|o| o == APPS[i].id)).collect();
        Home { cursor: Cursor::new(tiles.len(), tiles.len().max(1)), tiles, columns: columns.max(1) }
    }

    /// Moves or drops the cursor; the app to open on a confirm.
    pub fn act(&mut self, action: Action) -> Option<&'static App> {
        let c = self.cursor;
        self.cursor = match action {
            Action::Move { by, .. } => c.moved(by as i64),
            Action::Arrow { dx, dy } => c.moved(dx as i64 + dy as i64 * self.columns as i64),
            Action::Page(by) | Action::End(by) => c.moved(by as i64 * c.count as i64),
            Action::Cancel(_) => c.clear(),
            Action::Confirm(_) => return c.at.and_then(|at| self.tiles.get(at)).map(|&i| &APPS[i]),
        };
        None
    }

    /// A tap opens that app; the cursor goes, as a tap drops it in the JUCE lists.
    pub fn tap(&mut self, tile: usize) -> Option<&'static App> {
        self.cursor = self.cursor.clear();
        self.tiles.get(tile).map(|&i| &APPS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profiles_apps_in_the_launchers_order() {
        let h = Home::new(&["oscairn.settings".into(), "oscairn.setlist".into(), "oscairn.unknown".into()], 6);
        assert_eq!(h.tiles.iter().map(|&i| APPS[i].name).collect::<Vec<_>>(), ["Setlist", "Settings"]);
        assert_eq!(Home::new(&[], 6).tiles.len(), 5);
    }

    #[test]
    fn the_cursor_moves_and_only_confirm_opens() {
        let mut h = Home::new(&[], 3);
        assert!(h.act(Action::Confirm(None)).is_none(), "no cursor: nothing to confirm");
        assert!(h.act(Action::Move { pane: None, by: 1 }).is_none()); // shown on the first tile
        assert!(h.act(Action::Arrow { dx: 1, dy: 1 }).is_none()); // across and a row down: 0 + 1 + 3
        assert_eq!(h.cursor.at, Some(4));
        assert!(h.act(Action::Arrow { dx: 0, dy: 1 }).is_none()); // past the end: stays at the last
        assert_eq!(h.act(Action::Confirm(None)).unwrap().name, "Settings");
        assert!(h.act(Action::Cancel(None)).is_none() && h.cursor.at.is_none());
        assert_eq!(h.tap(2).unwrap().name, "Setlist");
        assert!(h.tap(9).is_none());
    }

    #[test]
    fn no_known_apps_offers_no_tiles() {
        let mut h = Home::new(&["oscairn.unknown".into(), "com.nobody.nothing".into()], 3);
        assert!(h.tiles.is_empty(), "a profile offering nothing known shows no tiles");
        assert!(h.cursor.at.is_none(), "no cursor on an empty Home");
        assert!(h.act(Action::Confirm(None)).is_none());
        assert!(h.act(Action::Move { pane: None, by: 1 }).is_none() && h.cursor.at.is_none());
        assert!(h.act(Action::Arrow { dx: 1, dy: 1 }).is_none() && h.cursor.at.is_none());
        assert!(h.tap(0).is_none(), "a tap on nothing opens nothing");
    }

    #[test]
    fn one_offered_app_is_the_only_tile() {
        let mut h = Home::new(&["oscairn.performance".into()], 3);
        assert_eq!(h.tiles.len(), 1);
        assert!(h.act(Action::Confirm(None)).is_none(), "no cursor to confirm yet");
        assert!(h.act(Action::Move { pane: None, by: 1 }).is_none(), "the first move shows the one tile");
        assert_eq!(h.cursor.at, Some(0));
        assert_eq!(h.act(Action::Confirm(None)).unwrap().id, "oscairn.performance");
    }

    #[test]
    fn zero_columns_is_one_column() {
        assert_eq!(Home::new(&[], 0).columns, 1, "columns are clamped to at least one");
        let mut h = Home::new(&[], 3);
        assert_eq!(h.cursor.at, None);
        assert!(h.act(Action::Arrow { dx: 1, dy: 0 }).is_none(), "the first move shows the first tile");
        assert_eq!(h.cursor.at, Some(0));
    }

    #[test]
    fn arrows_stop_at_either_end() {
        let mut h = Home::new(&[], 3);
        assert!(h.act(Action::Arrow { dx: 0, dy: 1 }).is_none()); // shows the first tile
        assert_eq!(h.cursor.at, Some(0));
        assert!(h.act(Action::Arrow { dx: -4, dy: -9 }).is_none()); // far past the start
        assert_eq!(h.cursor.at, Some(0), "an arrow never goes before the first tile");
        assert!(h.act(Action::Arrow { dx: 30, dy: 30 }).is_none()); // far past the end
        assert_eq!(h.cursor.at, Some(h.tiles.len() - 1), "an arrow never goes past the last tile");
    }

    #[test]
    fn page_and_end_reach_the_ends() {
        let mut h = Home::new(&[], 3);
        assert!(h.act(Action::End(1)).is_none(), "the first move shows the first tile");
        assert_eq!(h.cursor.at, Some(0));
        assert!(h.act(Action::End(1)).is_none());
        assert_eq!(h.cursor.at, Some(h.tiles.len() - 1), "End reaches the last app");
        assert_eq!(h.act(Action::Confirm(None)).unwrap().name, "Settings");
        assert!(h.act(Action::End(-1)).is_none());
        assert_eq!(h.cursor.at, Some(0), "End back reaches the first app");
        assert!(h.act(Action::End(-1)).is_none());
        assert_eq!(h.cursor.at, Some(0), "End back on the first stays there");
        assert!(h.act(Action::Page(1)).is_none());
        assert_eq!(h.cursor.at, Some(h.tiles.len() - 1), "Page Down reaches the last app");
    }

    #[test]
    fn a_tap_drops_a_live_cursor() {
        let mut h = Home::new(&[], 3);
        assert!(h.act(Action::Move { pane: None, by: 1 }).is_none());
        assert!(h.act(Action::Arrow { dx: 2, dy: 0 }).is_none());
        assert_eq!(h.cursor.at, Some(2));
        assert_eq!(h.tap(2).unwrap().name, "Setlist");
        assert!(h.cursor.at.is_none(), "a tap drops the cursor, as it does in the JUCE lists");
    }

    #[test]
    fn taps_on_the_first_and_last_open_them() {
        let mut h = Home::new(&[], 3);
        assert_eq!(h.tap(0).unwrap().id, "oscairn.performance", "the first tile");
        assert_eq!(h.tap(h.tiles.len() - 1).unwrap().id, "oscairn.settings", "the last tile");
    }
}
