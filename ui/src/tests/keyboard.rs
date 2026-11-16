// SPDX-License-Identifier: AGPL-3.0-or-later
//! The on-screen text keyboard's tests (src/keyboard.rs): what the JUCE TextKeyboard offers (the
//! layers, Shift for one character and Caps, the caret, Clear, Cancel, Done), the cursor and knobs
//! added for use without touch, a typing keyboard's keys, and the drawing in themes.

use super::*;
use crate::keyboard::{Keyboard, Outcome};

fn label_at(ui: &Shell, index: i32) -> String {
    ui.global::<crate::KeyboardState>().get_keys().iter().find(|k| k.index == index).map(|k| k.label.to_string()).unwrap_or_default()
}

fn cursor(ui: &Shell) -> Option<i32> {
    ui.global::<crate::KeyboardState>().get_keys().iter().find(|k| k.cursor).map(|k| k.index)
}

// Indices in the layout: 0 Left, 1 Right, 2 Clear, 3 Cancel, 4 Done; row 1 q..p 5..14, Del 15;
// row 2 a..l 16..24; row 3 Shift 25, z..m 26..32, period 33; row 4 123 34, Caps 35, space 36, then , - / @.
#[test]
fn keyboard_types_like_the_juce_one() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut k = Keyboard::open(ui, "Name", "Ab", true);
    let state = ui.global::<crate::KeyboardState>();
    assert!(state.get_open());
    assert_eq!((state.get_before().as_str(), state.get_after().as_str()), ("Ab", ""), "the caret at the end");
    assert_eq!(k.tap(5), Outcome::Typing); // q
    assert_eq!(k.text(), "Abq");
    // Shift is for the next character only; Caps stays.
    k.tap(25);
    assert_eq!(label_at(ui, 5), "Q");
    k.tap(5);
    assert_eq!((k.text().as_str(), label_at(ui, 5).as_str()), ("AbqQ", "q"));
    k.tap(35);
    k.tap(5);
    k.tap(5);
    assert_eq!(k.text(), "AbqQQQ", "Caps stays on");
    k.tap(35);
    // The symbols layer: digits on the top row, and Shift and Caps off.
    k.tap(34);
    assert_eq!((label_at(ui, 5).as_str(), label_at(ui, 34).as_str(), label_at(ui, 26).as_str()), ("1", "abc", "("));
    assert!(state.get_keys().iter().any(|x| x.index == 25 && x.disabled), "Shift does not work on it");
    k.tap(25);
    k.tap(5);
    assert_eq!(k.text(), "AbqQQQ1");
    k.tap(34);
    // Space, the caret: Left twice, then a character goes between.
    k.tap(36);
    k.tap(0);
    k.tap(0);
    k.tap(16); // a
    assert_eq!((state.get_before().as_str(), state.get_after().as_str()), ("AbqQQQa", "1 "));
    k.tap(15); // Del
    k.tap(1);
    assert_eq!((state.get_before().as_str(), state.get_after().as_str()), ("AbqQQQ1", " "));
    // Held, Del, Left and Right repeat; a letter does not.
    assert!(k.repeats(15) && k.repeats(0) && k.repeats(1) && !k.repeats(5) && !k.repeats(4));
    k.tap(15);
    k.repeat();
    assert_eq!(k.text(), "AbqQQ ", "Del, and the held key pressed again");
    k.tap(2); // Clear
    assert_eq!(k.text(), "");
    // The length limit holds as a field's does.
    for _ in 0..60 {
        k.tap(5);
    }
    assert_eq!(k.text().chars().count(), 48);
    // Done is Return, Cancel leaves with nothing.
    assert_eq!(k.tap(4), Outcome::Accept("q".repeat(48)));
    assert_eq!(k.tap(3), Outcome::Cancel);
    k.close();
    assert!(!state.get_open());
}

// Without touch: the dial and the left knob step the cursor over the keys one after another, the
// cursor keys move across and down (under the same place), Home and End find the first key and
// Done, a press types, the right knob moves the caret, Cancel closes.
#[test]
fn keyboard_without_touch() {
    let s = Screen::new();
    let ui = &s.ui;
    let mut k = Keyboard::open(ui, "Name", "", true);
    assert_eq!(cursor(ui), Some(5), "on the first letter");
    k.act(Action::Move { pane: None, by: 2 });
    assert_eq!(cursor(ui), Some(7));
    k.act(Action::Move { pane: Some(0), by: -3 });
    assert_eq!(cursor(ui), Some(4), "back through the top row's keys to Done");
    k.act(Action::Arrow { dx: 0, dy: 1 });
    assert_eq!(cursor(ui).map(|i| label_at(ui, i)).as_deref(), Some("Del"), "down: the key under it, Del under Done");
    for _ in 0..3 {
        k.act(Action::Arrow { dx: 0, dy: 1 });
    }
    assert_eq!(cursor(ui).map(|i| label_at(ui, i)).as_deref(), Some("/"), "the bottom row, nearest the same place");
    k.act(Action::Arrow { dx: -1, dy: 0 });
    assert_eq!(cursor(ui).map(|i| label_at(ui, i)).as_deref(), Some("-"));
    k.act(Action::End(-1));
    assert_eq!(cursor(ui), Some(0));
    k.act(Action::End(1));
    assert_eq!(cursor(ui), Some(4), "End finds Done");
    // A press types the key under the cursor; the right knob moves the caret.
    k.act(Action::End(-1));
    k.act(Action::Move { pane: None, by: 5 }); // q
    k.act(Action::Confirm(None));
    k.act(Action::Move { pane: None, by: 1 }); // w
    k.act(Action::Confirm(None));
    assert_eq!(k.text(), "qw");
    k.act(Action::Move { pane: Some(1), by: -1 });
    k.act(Action::Confirm(None)); // w again, before the last w
    assert_eq!(ui.global::<crate::KeyboardState>().get_after().as_str(), "w");
    assert_eq!(k.text(), "qww");
    assert_eq!(k.act(Action::Cancel(None)), Outcome::Cancel);
    k.act(Action::End(1));
    assert_eq!(k.act(Action::Confirm(None)), Outcome::Accept("qww".into()));
}

// A screen with a typing keyboard (the profile's textEntry is `keyboard`): no rows of keys, only the
// top row; typed characters, Backspace, the arrows, Return and Escape do the typing.
#[test]
fn keyboard_with_a_typing_keyboard() {
    use slint::platform::Key;
    let s = Screen::new();
    let ui = &s.ui;
    let mut k = Keyboard::open(ui, "Name", "Pad", false);
    let state = ui.global::<crate::KeyboardState>();
    assert!(!state.get_keys_shown());
    assert_eq!(state.get_keys().iter().count(), 5, "Left, Right, Clear, Cancel and Done only");
    let key = |k: Key| slint::SharedString::from(k).to_string();
    k.typed("s");
    k.typed("É");
    k.typed(&key(Key::LeftArrow));
    k.typed(&key(Key::Backspace));
    assert_eq!((state.get_before().as_str(), state.get_after().as_str()), ("Pad", "É"));
    assert_eq!(k.typed(&key(Key::Return)), Outcome::Accept("PadÉ".into()));
    assert_eq!(k.typed(&key(Key::Escape)), Outcome::Cancel);
}

// The keyboard draws in the built-in themes and an odd one.
#[test]
fn keyboard_draws_in_every_theme() {
    let mut s = Screen::new();
    let _k = Keyboard::open(&s.ui, "Name of the part", "Keys", true);
    s.ui.set_screen(4);
    let loaded = all_themes();
    for t in &loaded {
        theme::apply(&s.ui, t);
        assert!(s.frame().is_some(), "{}: drawn", t.id);
        png(&s, &format!("keyboard-{}", t.id));
        shot(&s, &format!("keyboard-{}", t.id));
    }
    at_fantom_scale(&mut s, "keyboard", || ());
}

// On a screen with no typing keyboard the keyboard is an overlay on the lower part of the screen (the reference's): the
// screen's top stays in view above it, at the keyboard's 826 x 465 and at 800 x 480 and a large window.
#[test]
fn the_keyboard_is_an_overlay_on_the_lower_part_of_the_screen() {
    for (w, h, scale) in [(1280, 720, 1.55), (800, 480, 1.0), (1920, 1080, 1.0)] {
        let mut s = Screen::sized(w, h);
        s.set_scale(scale);
        let _k = Keyboard::open(&s.ui, "Name of the part", "Keys", true);
        s.ui.set_screen(4);
        s.frame();
        s.frame();
        let want = s.ui.global::<crate::KeyboardState>().get_overlay_height();
        let screen = h as f32 / scale;
        let bar = s.ui.global::<crate::Theme>().get_bar_height();
        eprintln!("keyboard overlay {want} of {screen} at {w}x{h}@{scale}");
        assert!(want <= screen - bar, "the overlay fits under the bar: {want} of {screen}");
        if screen >= 465. {
            assert!(want < screen * 0.62, "the screen's top stays in view: {want} of {screen}");
        }
        shot(&s, &format!("keyboard-overlay-{w}x{h}@{scale}"));
    }
}
