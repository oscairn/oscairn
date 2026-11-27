// SPDX-License-Identifier: AGPL-3.0-or-later
//! The on-screen text keyboard: a port of the JUCE app's TextKeyboard (hosts/standalone/TextKeyboard.cpp,
//! RFC 0019), the same keys, rows and behavior. Row 0: the text with its caret, then Left, Right,
//! Clear, Cancel and Done; row 1: q to p (1 to 0 on the symbols layer) and Del; row 2: a to l
//! (@ # & - _ / : ' "), centered; row 3: Shift and z to m and the period (( ) , ? ! % + .); row 4: the
//! layer key (123 or abc), Caps, space and , - / @. US QWERTY only. Shift is for the next character
//! only, Caps stays; neither works on the symbols layer. Left and Right move the caret, Clear empties
//! the text, Cancel closes it with nothing (the JUCE one puts back the text the editor had), Done is
//! Return: the text goes to the screen that asked. A tap types at once (on press, as the JUCE keys
//! are triggered on mouse down), and Del, Left and Right repeat while held (450 ms, then every 70 ms).
//!
//! Added for use without touch, where the JUCE one has none: a border cursor on a key. The dial or
//! the left knob steps it along the keys, the cursor keys move it across and down, Home and End find
//! the first key and Done, a press types the key, Cancel closes the keyboard; the right knob moves the
//! caret in the text. On a screen whose profile says `textEntry` is `keyboard` (a computer with a typing keyboard) the rows of keys are left out,
//! as the JUCE app shows none there, and the typed keys, Backspace, the arrows, Return and Escape do it.

use crate::nav::Action;
use crate::setlist::update;
use crate::{KeyData, KeyboardState, Shell};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

/// The longest name typed: what a part's or scene's name needs.
const MAX: usize = 48;
/// A row's width in key units, as the JUCE top row's letters and Del make it.
const ROW: f32 = 11.5;
/// The text's share of row 0; Left, Right, Clear, Cancel and Done take the rest.
const LINE: f32 = 6.4;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Char(char, char),
    Shift,
    Caps,
    Layer,
    Space,
    Back,
    Left,
    Right,
    Clear,
    Cancel,
    Done,
}

#[derive(Clone, Copy)]
struct Key {
    kind: Kind,
    x: f32,
    w: f32,
    total: f32,
    row: i32,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Typing,
    /// Done: the text.
    Accept(String),
    Cancel,
}

fn layout() -> Vec<Key> {
    let mut keys = Vec::new();
    // A row of keys from `x`, each its weight wide, in a row `total` wide.
    let mut row = |row: i32, x: f32, total: f32, list: Vec<(Kind, f32)>| {
        let mut at = x;
        for (kind, w) in list {
            keys.push(Key { kind, x: at, w, total, row });
            at += w;
        }
    };
    // Row 0's keys are the JUCE weights (1.2, 1.2, 1.6, 2, 2.5) in the share of the row they take.
    let unit = (ROW - LINE) / 8.5;
    row(0, LINE, ROW, vec![(Kind::Left, 1.2 * unit), (Kind::Right, 1.2 * unit), (Kind::Clear, 1.6 * unit), (Kind::Cancel, 2. * unit), (Kind::Done, 2.5 * unit)]);
    let top = "qwertyuiop".chars().zip("1234567890".chars()).map(|(l, s)| (Kind::Char(l, s), 1.)).chain([(Kind::Back, 1.5)]);
    row(1, 0., ROW, top.collect());
    row(2, 1.25, ROW, "asdfghjkl".chars().zip("@#&-_/:'\"".chars()).map(|(l, s)| (Kind::Char(l, s), 1.)).collect());
    let bottom = [(Kind::Shift, 1.5)].into_iter().chain("zxcvbnm.".chars().zip("(),?!%+.".chars()).map(|(l, s)| (Kind::Char(l, s), 1.)));
    row(3, 0., 9.5, bottom.collect());
    let last = [(Kind::Layer, 1.5), (Kind::Caps, 1.5), (Kind::Space, 4.)].into_iter().chain(",-/@".chars().map(|c| (Kind::Char(c, c), 1.)));
    row(4, 0., 11., last.collect());
    keys
}

pub struct Keyboard {
    keys: Vec<Key>,
    text: Vec<char>,
    /// The caret: a position between characters.
    caret: usize,
    shift: bool,
    caps: bool,
    symbols: bool,
    /// The rows of keys are shown (the profile's textEntry is onScreen).
    on_screen: bool,
    at: usize,
    model: Rc<VecModel<KeyData>>,
    ui: slint::Weak<Shell>,
}

impl Keyboard {
    /// Opens over the shell with `text` already in the field (a name being changed), the caret at its
    /// end and the cursor on the first letter.
    pub fn open(ui: &Shell, title: &str, text: &str, on_screen: bool) -> Keyboard {
        let model = Rc::new(VecModel::default());
        let state = ui.global::<KeyboardState>();
        state.set_keys(ModelRc::from(model.clone()));
        state.set_title(title.into());
        state.set_keys_shown(on_screen);
        state.set_open(true);
        let text: Vec<char> = text.chars().take(MAX).collect();
        let k = Keyboard { keys: layout(), caret: text.len(), text, shift: false, caps: false, symbols: false, on_screen, at: 5, model, ui: ui.as_weak() };
        k.show();
        k
    }

    pub fn close(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<KeyboardState>().set_open(false);
        }
    }

    fn upper(&self) -> bool {
        self.shift || self.caps
    }

    fn label(&self, k: &Key) -> String {
        match k.kind {
            Kind::Char(_, s) if self.symbols => s.to_string(),
            Kind::Char(l, _) if self.upper() => l.to_uppercase().to_string(),
            Kind::Char(l, _) => l.to_string(),
            Kind::Shift => "Shift".into(),
            Kind::Caps => "Caps".into(),
            Kind::Layer => if self.symbols { "abc" } else { "123" }.into(),
            Kind::Space => "space".into(),
            Kind::Back => "Del".into(),
            Kind::Left => "<".into(),
            Kind::Right => ">".into(),
            Kind::Clear => "Clear".into(),
            Kind::Cancel => "Cancel".into(),
            Kind::Done => "Done".into(),
        }
    }

    /// The text typed so far.
    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    fn show(&self) {
        let rows: Vec<KeyData> = self
            .keys
            .iter()
            .enumerate()
            .filter(|(_, k)| self.on_screen || k.row == 0)
            .map(|(i, k)| KeyData {
                label: self.label(k).into(),
                x: k.x,
                w: k.w,
                total: k.total,
                row: k.row,
                index: i as i32,
                cursor: i == self.at,
                on: (k.kind == Kind::Shift && self.shift) || (k.kind == Kind::Caps && self.caps),
                special: !matches!(k.kind, Kind::Char(..)),
                disabled: self.symbols && matches!(k.kind, Kind::Shift | Kind::Caps),
                done: k.kind == Kind::Done,
            })
            .collect();
        update(&self.model, rows);
        if let Some(ui) = self.ui.upgrade() {
            let state = ui.global::<KeyboardState>();
            state.set_before(self.text[..self.caret].iter().collect::<String>().into());
            state.set_after(self.text[self.caret..].iter().collect::<String>().into());
            state.set_line(LINE / ROW);
        }
    }

    /// Characters typed at the caret, as a field takes real typing (its length limit applies).
    fn insert(&mut self, c: char) {
        if self.text.len() < MAX {
            self.text.insert(self.caret, c);
            self.caret += 1;
        }
    }

    /// The key a move across or down reaches: the one in the next row under the same place, or beside the cursor.
    fn arrow(&mut self, dx: i32, dy: i32) {
        let shown: Vec<usize> = (0..self.keys.len()).filter(|i| self.on_screen || self.keys[*i].row == 0).collect();
        let pos = shown.iter().position(|i| *i == self.at).unwrap_or(0);
        if dx != 0 {
            self.at = shown[(pos as i32 + dx).clamp(0, shown.len() as i32 - 1) as usize];
        } else {
            let k = self.keys[self.at];
            let row = (k.row + dy).clamp(0, if self.on_screen { 4 } else { 0 });
            let centre = |k: &Key| (k.x + k.w / 2.) / k.total;
            let here = centre(&k);
            let best = shown.iter().copied().filter(|i| self.keys[*i].row == row).min_by(|a, b| (centre(&self.keys[*a]) - here).abs().total_cmp(&(centre(&self.keys[*b]) - here).abs()));
            if let Some(i) = best {
                self.at = i;
            }
        }
    }

    fn press(&mut self) -> Outcome {
        let kind = self.keys[self.at].kind;
        match kind {
            Kind::Char(l, s) => {
                self.insert(if self.symbols { s } else if self.upper() { l.to_uppercase().next().unwrap_or(l) } else { l });
                self.shift = false; // one-shot
            }
            Kind::Space => {
                self.insert(' ');
                self.shift = false;
            }
            Kind::Shift => self.shift = !self.shift && !self.symbols,
            Kind::Caps => self.caps = !self.caps && !self.symbols,
            Kind::Layer => {
                self.symbols = !self.symbols;
                self.shift = false;
            }
            Kind::Back => self.backspace(),
            Kind::Left => self.caret = self.caret.saturating_sub(1),
            Kind::Right => self.caret = (self.caret + 1).min(self.text.len()),
            Kind::Clear => {
                self.text.clear();
                self.caret = 0;
            }
            Kind::Cancel => return Outcome::Cancel,
            Kind::Done => return Outcome::Accept(self.text()),
        }
        Outcome::Typing
    }

    fn backspace(&mut self) {
        if self.caret > 0 {
            self.text.remove(self.caret - 1);
            self.caret -= 1;
        }
    }

    /// A key typed on a computer's keyboard (the profile's textEntry is `keyboard`): a printable
    /// character, or Backspace, Left, Right, Return (Done) and Escape (Cancel) by their Slint key text.
    pub fn typed(&mut self, text: &str) -> Outcome {
        use slint::platform::Key;
        let is = |k: Key| text == slint::SharedString::from(k).as_str();
        let outcome = if is(Key::Backspace) {
            self.backspace();
            Outcome::Typing
        } else if is(Key::LeftArrow) {
            self.caret = self.caret.saturating_sub(1);
            Outcome::Typing
        } else if is(Key::RightArrow) {
            self.caret = (self.caret + 1).min(self.text.len());
            Outcome::Typing
        } else if is(Key::Return) || text == "\n" || text == "\r" {
            Outcome::Accept(self.text())
        } else if is(Key::Escape) {
            Outcome::Cancel
        } else {
            let mut chars = text.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                if !c.is_control() {
                    self.insert(c);
                }
            }
            Outcome::Typing
        };
        self.show();
        outcome
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let last = self.keys.len() - 1;
        let outcome = match action {
            // The right knob (pane 1) moves the caret in the text; the left knob and the dial, the cursor over the keys.
            Action::Move { pane: Some(1), by } => {
                self.caret = (self.caret as i64 + by as i64).clamp(0, self.text.len() as i64) as usize;
                Outcome::Typing
            }
            Action::Move { by, .. } => {
                let shown: Vec<usize> = (0..=last).filter(|i| self.on_screen || self.keys[*i].row == 0).collect();
                let pos = shown.iter().position(|i| *i == self.at).unwrap_or(0) as i64;
                self.at = shown[(pos + by as i64).clamp(0, shown.len() as i64 - 1) as usize];
                Outcome::Typing
            }
            Action::Arrow { dx, dy } => {
                self.arrow(dx, dy);
                Outcome::Typing
            }
            Action::Page(by) => {
                self.arrow(0, by);
                Outcome::Typing
            }
            // The first key, and Done, which is the last of row 0.
            Action::End(by) => {
                self.at = if by < 0 { 0 } else { 4 };
                Outcome::Typing
            }
            Action::Confirm(_) => self.press(),
            Action::Cancel(_) => Outcome::Cancel,
        };
        self.show();
        outcome
    }

    /// A key pressed by touch, by its index in the layout, which types at once.
    pub fn tap(&mut self, i: usize) -> Outcome {
        if i < self.keys.len() {
            self.at = i;
        }
        let outcome = self.press();
        self.show();
        outcome
    }

    /// Whether the key `i` repeats while held.
    pub fn repeats(&self, i: usize) -> bool {
        self.keys.get(i).is_some_and(|k| matches!(k.kind, Kind::Back | Kind::Left | Kind::Right))
    }

    /// One repeat of the key held: pressed again.
    pub fn repeat(&mut self) {
        self.press();
        self.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keyboard in a plain state, with no window: what a test of the pure logic needs.
    fn kb(text: &str, on_screen: bool) -> Keyboard {
        let text: Vec<char> = text.chars().collect();
        Keyboard {
            keys: layout(),
            caret: text.len(),
            text,
            shift: false,
            caps: false,
            symbols: false,
            on_screen,
            at: 5,
            model: Rc::new(VecModel::default()),
            ui: slint::Weak::default(),
        }
    }

    #[test]
    fn the_layout_holds_every_key_inside_its_row() {
        let keys = layout();
        assert_eq!(keys.len(), 41, "the rows as the JUCE TextKeyboard has them");
        assert_eq!(keys[0].kind, Kind::Left, "row 0 starts with Left");
        assert_eq!(keys[4].kind, Kind::Done, "…and ends with Done");
        assert_eq!(keys[40].kind, Kind::Char('@', '@'), "the very last key is @");
        assert_eq!(keys[15].kind, Kind::Back, "Del is the last of row 1");
        assert_eq!((keys[25].kind, keys[33].kind), (Kind::Shift, Kind::Char('.', '.')), "row 3: Shift first, period last");
        for k in &keys {
            assert!(k.w > 0., "every key has a width");
            assert!(k.x >= 0., "a key starts inside its row");
            assert!(k.x + k.w <= k.total + 1e-4, "a key ends at or before its row's edge");
        }
        // The rows are laid out in order: a row number never goes backward.
        assert!(keys.windows(2).all(|w| w[1].row >= w[0].row), "rows come in order");
    }

    #[test]
    fn labels_follow_the_layer_and_the_caps() {
        let keys = layout();
        let mut k = kb("", true);
        assert_eq!(k.label(&keys[5]), "q", "the letter at hand under a free shift");
        k.caps = true;
        assert_eq!(k.label(&keys[5]), "Q", "Caps shows the capital");
        k.symbols = true;
        assert_eq!(k.label(&keys[5]), "1", "on the symbols layer a number, and caps does nothing");
        assert_eq!(k.label(&keys[34]), "abc", "the layer key says abc on it");
        k.symbols = false;
        assert_eq!(k.label(&keys[34]), "123", "and 123 off it");
        assert_eq!((k.label(&keys[25]).as_str(), k.label(&keys[35]).as_str(), k.label(&keys[36]).as_str(), k.label(&keys[15]).as_str()),
            ("Shift", "Caps", "space", "Del"));
        assert_eq!(k.label(&keys[0]), "<", "Left is an arrow");
        assert_eq!(k.label(&keys[1]), ">", "Right is an arrow");
    }

    #[test]
    fn insert_puts_the_character_at_the_caret_and_holds_the_limit() {
        let mut k = kb("ab", true);
        k.caret = 1;
        k.insert('X');
        assert_eq!((k.text().as_str(), k.caret), ("aXb", 2), "X goes between a and b, at the caret");
        k.caret = 3; // to the end
        k.insert('é'); // one character, though it takes two bytes
        assert_eq!((k.text().as_str(), k.caret), ("aXbé", 4));
        assert_eq!(k.text().chars().count(), 4, "the count is in characters");
    }

    #[test]
    fn insert_ignores_a_character_past_the_field_limit() {
        let mut k = kb(&"x".repeat(MAX), true);
        k.insert('y');
        assert_eq!(k.text().chars().count(), MAX, "the field holds its limit");
    }

    #[test]
    fn backspace_removes_the_character_before_the_caret_and_holds_at_zero() {
        let mut k = kb("aé", true);
        k.backspace();
        assert_eq!((k.text().as_str(), k.caret), ("a", 1), "the é goes whole");
        k.backspace();
        assert_eq!((k.text().as_str(), k.caret), ("", 0));
        k.backspace();
        assert_eq!((k.text().as_str(), k.caret), ("", 0), "nothing before the start");
    }

    #[test]
    fn typed_takes_one_printing_key_or_a_special() {
        use slint::platform::Key;
        let key = |k: Key| slint::SharedString::from(k).to_string();
        let mut k = kb("ab", true);
        assert_eq!(k.typed("xy"), Outcome::Typing, "two characters are not one key");
        assert_eq!(k.typed("\t"), Outcome::Typing, "a control character is skipped");
        assert_eq!(k.typed(""), Outcome::Typing, "an empty key is a dead press");
        assert_eq!(k.text(), "ab");
        k.caret = 1;
        k.typed("é");
        assert_eq!((k.text().as_str(), k.caret), ("aéb", 2), "a character types at the caret");
        assert_eq!(k.typed(&key(Key::Backspace)), Outcome::Typing);
        assert_eq!((k.text().as_str(), k.caret), ("ab", 1), "backspace takes the é whole");
        k.caret = 0;
        assert_eq!(k.typed(&key(Key::Backspace)), Outcome::Typing);
        assert_eq!(k.text(), "ab", "nothing before the start");
        assert_eq!(k.typed(&key(Key::LeftArrow)), Outcome::Typing);
        assert_eq!(k.caret, 0, "the caret holds at the start");
        k.caret = k.text().chars().count();
        assert_eq!(k.typed(&key(Key::RightArrow)), Outcome::Typing);
        assert_eq!(k.caret, 2, "the caret holds at the end");
        assert_eq!(k.typed(&key(Key::Return)), Outcome::Accept("ab".into()));
        assert_eq!(k.typed(&key(Key::Escape)), Outcome::Cancel);
        assert_eq!(kb("n", true).typed("\n"), Outcome::Accept("n".into()), "a raw newline is Return");
        assert_eq!(kb("r", true).typed("\r"), Outcome::Accept("r".into()));
    }

    #[test]
    fn a_press_types_the_letter_under_the_state() {
        let mut k = kb("", true);
        k.at = 5; // q
        assert_eq!(k.press(), Outcome::Typing);
        assert_eq!(k.text(), "q");
        k.shift = true;
        assert_eq!(k.press(), Outcome::Typing);
        assert_eq!(k.text(), "qQ");
        assert!(!k.shift, "Shift was for one character");
        k.symbols = true;
        assert_eq!(k.press(), Outcome::Typing);
        assert_eq!(k.text(), "qQ1", "the symbols layer types the number");
        k.at = 36;
        assert_eq!(k.press(), Outcome::Typing);
        assert_eq!(k.text(), "qQ1 ");
    }

    #[test]
    fn shift_and_caps_toggle_but_stay_off_on_the_symbols_layer() {
        let mut k = kb("", true);
        k.at = 25;
        k.press();
        assert!(k.shift, "Shift toggles on");
        k.press();
        assert!(!k.shift, "and off");
        k.symbols = true;
        k.press();
        assert!(!k.shift, "Shift stays off there");
        k.symbols = false;
        k.at = 35;
        k.press();
        assert!(k.caps, "Caps stays on");
        k.press();
        assert!(!k.caps, "Caps toggles off");
        k.symbols = true;
        k.press();
        assert!(!k.caps, "Caps is off there too");
    }

    #[test]
    fn the_layer_key_opens_and_closes_the_symbols() {
        let mut k = kb("", true);
        k.at = 34;
        k.shift = true;
        assert_eq!(k.press(), Outcome::Typing);
        assert!(k.symbols, "the layer key opens the symbols");
        assert!(!k.shift, "and drops the shift");
        assert_eq!(k.press(), Outcome::Typing);
        assert!(!k.symbols, "and closes them again");
    }

    #[test]
    fn the_caret_edges_and_clear() {
        let mut k = kb("ab", true);
        k.caret = 0;
        k.at = 0;
        k.press(); // Left
        assert_eq!(k.caret, 0, "the caret does not go before the start");
        k.caret = 2;
        k.at = 1;
        k.press(); // Right, at the end
        assert_eq!(k.caret, 2, "and not past it");
        k.at = 2;
        k.caret = 0;
        k.press(); // Clear
        assert_eq!((k.text().as_str(), k.caret), ("", 0), "Clear empties the field");
    }

    #[test]
    fn done_and_cancel_are_return_and_escape() {
        let mut k = kb("ok", true);
        k.at = 4;
        assert_eq!(k.press(), Outcome::Accept("ok".into()));
        k.at = 3;
        assert_eq!(k.press(), Outcome::Cancel);
    }

    #[test]
    fn the_knob_moves_the_caret_within_the_text() {
        let mut k = kb("ab", true);
        assert_eq!(k.act(Action::Move { pane: Some(1), by: -9 }), Outcome::Typing);
        assert_eq!(k.caret, 0, "the knob clamps to the start");
        assert_eq!(k.act(Action::Move { pane: Some(1), by: 9 }), Outcome::Typing);
        assert_eq!(k.caret, 2, "and to the end");
    }

    #[test]
    fn the_dial_steps_the_cursor_and_holds_it_in_the_rows() {
        let mut k = kb("", true);
        assert_eq!(k.act(Action::Move { pane: None, by: 100 }), Outcome::Typing);
        assert_eq!(k.at, 40, "the cursor stops at @, the last key");
        assert_eq!(k.act(Action::Move { pane: None, by: -100 }), Outcome::Typing);
        assert_eq!(k.at, 0, "and at Left, the first");
        k.act(Action::End(-1));
        assert_eq!(k.at, 0, "Home finds the first key");
        k.act(Action::End(1));
        assert_eq!(k.at, 4, "End finds Done");
    }

    #[test]
    fn the_cursor_keys_stay_within_the_rows() {
        let keys = layout();
        // Off a keyboard screen only the top row shows: the first arrow must still land on one of its keys.
        let mut k = kb("", false);
        k.act(Action::Arrow { dx: 1, dy: 0 });
        assert_eq!(k.at, 1, "Right, beside Left, even when it starts on an unseen key");
        k.act(Action::Arrow { dx: -100, dy: 0 });
        assert_eq!(k.at, 0, "left of the first key holds");
        k.act(Action::Arrow { dx: 0, dy: 1 });
        assert_eq!(keys[k.at].row, 0, "no key rows off a keyboard screen");
        k.act(Action::Arrow { dx: 0, dy: -1 });
        assert_eq!(keys[k.at].row, 0);
        // On it, the cursor walks the rows and stops at the ends.
        let mut k = kb("", true);
        k.at = 0;
        k.act(Action::Arrow { dx: -1, dy: 0 });
        assert_eq!(k.at, 0, "left of the first key stays there");
        k.act(Action::Arrow { dx: 100, dy: 0 });
        assert_eq!(k.at, 40, "right past the last stops at @");
        k.at = 0;
        k.act(Action::Arrow { dx: 0, dy: 1 });
        assert_eq!(keys[k.at].row, 1, "down leaves the top row");
        for _ in 0..5 {
            k.act(Action::Arrow { dx: 0, dy: 1 });
        }
        assert_eq!(keys[k.at].row, 4, "and settles in the bottom row");
        for _ in 0..6 {
            k.act(Action::Arrow { dx: 0, dy: -1 });
        }
        assert_eq!(keys[k.at].row, 0, "and back up to the top");
    }

    #[test]
    fn a_tap_out_of_range_keeps_the_cursor_where_it_is() {
        let mut k = kb("", true);
        k.at = 5; // q
        assert_eq!(k.tap(1000), Outcome::Typing);
        assert_eq!(k.at, 5, "an index past the last key is ignored");
    }

    #[test]
    fn repeats_only_del_left_and_right_inside_the_layout() {
        let k = kb("", true);
        assert!(k.repeats(0) && k.repeats(1) && k.repeats(15), "Left, Right and Del repeat");
        assert!(!k.repeats(4), "Done does not");
        assert!(!k.repeats(5), "a letter does not");
        assert!(!k.repeats(41) && !k.repeats(usize::MAX), "out of range repeats nothing");
    }

    #[test]
    fn text_collects_the_characters() {
        let k = kb("Prés", true);
        assert_eq!(k.text(), "Prés", "unicode comes back whole");
    }
}
