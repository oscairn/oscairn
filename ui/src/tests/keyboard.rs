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

// Reachability: every key of the text keyboard reached and activated from Home with panel moves only
// (move, confirm, cancel/EXIT), no touch. The walk turns the dial so the cursor stops on every key
// the keyboard's own model declares — never a hard-coded count — and presses (Confirm) each: a
// letter, digit, punctuation or space key types that character into the field, Del erases the last,
// the caret keys move the caret, Clear empties it, Shift and Caps switch to upper case (and Caps
// stays), 123 switches to the symbols layer and flips the labels, Cancel/EXIT closes the keyboard
// back onto the Part screen and Done accepts the name to the engine (a `parts.name` request with the
// new name). An action a key offers that the dial cannot reach would be a product gap.
#[test]
fn keyboard_reachable_from_home_by_panel_moves() {
    let mut f = Fixture::new("keyboard-reach");
    f.engine.answer(); // session.hello
    f.pump();
    let graph = super::part::part_graph();
    let ui = f.screen.ui.clone_strong();
    // A quiet engine: every request out is answered until none comes for a moment.
    let settle = |f: &mut Fixture| {
        while let Some(r) = f.engine.try_request(std::time::Duration::from_millis(12)) {
            let method = r["method"].as_str().unwrap_or_default().to_string();
            let result = match method.as_str() {
                "performance.info" => json!({"id": "p1", "name": "Grand", "unsaved": false}),
                "graph.get" => graph.clone(),
                "library.query" => json!({"items": [], "total": 0}),
                "library.libraries" => json!({"libraries": []}),
                "session.status" => json!({"midiInputs": [], "midiOutputs": []}),
                "harmony.get" => json!({"key": {"root": 0, "scale": [0, 2, 4, 5, 7, 9, 11], "name": "Major"}, "parts": [], "scales": ["Major", "Minor"]}),
                "edit.history" | "edit.undo" | "edit.redo" => json!({"undo": "", "redo": ""}),
                "window.info" => json!({"available": false}),
                "presets.list" => json!({"presets": []}),
                "record.get" => json!({"recording": 0, "retro": {"seconds": 30}, "takes": []}),
                "parts.meters" => json!({"meters": []}),
                _ => json!({}),
            };
            f.reply(&r, json!({"result": result}));
        }
    };
    settle(&mut f);
    // Home → the Performance tile → Play.
    let mut total = std::collections::BTreeMap::<String, i64>::new();
    let mut nav = |f: &mut Fixture, key: &str, by: i64| {
        *total.entry(key.into()).or_default() += by;
        f.navigation(json!(total.clone()));
    };
    nav(&mut f, "move", 0); // the first read is only a baseline
    nav(&mut f, "move", 1); // the cursor onto the Performance tile
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert_eq!(ui.get_screen(), 3, "the Performance tile opens Play");
    // Play: the dial down to the first part, pressed, opens the Part screen.
    nav(&mut f, "move:1", 1);
    nav(&mut f, "confirm:1", 1);
    settle(&mut f);
    assert_eq!((ui.get_screen(), title(&ui)), (4, "Keys".to_string()), "the press on a part opens the Part screen");
    // The Part list's cursor down to the name row, pressed: the text keyboard opens. The FANTOM-07
    // profile says textEntry is onScreen, so it shows its rows of keys.
    let part_rows = |ui: &Shell| ui.global::<crate::PartState>().get_rows().iter().collect::<Vec<_>>();
    for _ in 0..200 {
        let at = part_rows(&ui).iter().position(|r| r.cursor);
        if at.is_some_and(|a| part_rows(&ui)[a].label.as_str() == "Name") {
            break;
        }
        nav(&mut f, "move", 1);
        settle(&mut f);
    }
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    let state = ui.global::<crate::KeyboardState>();
    assert!(state.get_open() && state.get_keys_shown(), "the press on the name opens the on-screen keyboard");
    assert_eq!(state.get_title().as_str(), "Name of the part");
    // The whole keyboard the model declares: counted from KeyboardState.keys, never hard-coded.
    let n = state.get_keys().iter().count();
    assert!(n > 20, "the keyboard shows its rows of keys, {n}");
    // Reachability: the dial to the first key, then across every key; each is a cursor stop, and
    // nothing is pressed (a move sends nothing and nothing switches).
    let mut reached = Vec::new();
    while cursor(&ui) != Some(0) {
        nav(&mut f, "move", -1);
        settle(&mut f);
        reached.push(cursor(&ui).unwrap());
    }
    reached.push(0);
    for _ in 1..n as i64 {
        nav(&mut f, "move", 1);
        settle(&mut f);
        reached.push(cursor(&ui).unwrap());
    }
    reached.sort_unstable();
    reached.dedup();
    assert_eq!(reached.len(), n, "the walk reached {reached:?}, the keyboard declares {n}");
    assert!(reached.iter().enumerate().all(|(want, &at)| want == at as usize), "every key stop, none missed and none twice: {reached:?}");
    // The dial lands the cursor on a key; Confirm presses it. The field is what the keyboard types.
    let goto = |nav: &mut dyn FnMut(&mut Fixture, &str, i64), f: &mut Fixture, k: i64, settle: &dyn Fn(&mut Fixture)| {
        nav(f, "move", k - i64::from(cursor(&f.screen.ui).unwrap()));
        settle(f);
        assert_eq!(cursor(&f.screen.ui), Some(k as i32), "the cursor on key {k}");
    };
    let field = |ui: &Shell| {
        let s = ui.global::<crate::KeyboardState>();
        (s.get_before().to_string(), s.get_after().to_string())
    };
    let typed = |ui: &Shell| {
        let (b, a) = field(ui);
        b.len() + a.len()
    };
    // A key found by the label the keyboard shows (a letter in either case), never by its place.
    let index_of = |ui: &Shell, label: &str| -> i64 {
        let keys: Vec<_> = ui.global::<crate::KeyboardState>().get_keys().iter().collect();
        keys.iter().find(|k| k.label.as_str().eq_ignore_ascii_case(label)).unwrap_or_else(|| panic!("no key shows {label:?}")).index.into()
    };
    let on = |ui: &Shell, i: i64| ui.global::<crate::KeyboardState>().get_keys().iter().any(|k| i64::from(k.index) == i && k.on);
    let (shift, caps, layer, q) = (index_of(&ui, "Shift"), index_of(&ui, "Caps"), index_of(&ui, "123"), index_of(&ui, "q"));
    // The modals, on the letters' layer where they work: Shift for one character, Caps that stays,
    // and 123 that swaps every label to the symbols layer and back.
    goto(&mut nav, &mut f, shift, &settle);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert!(on(&ui, shift), "Shift engages");
    goto(&mut nav, &mut f, q, &settle); // q, now upper for one character
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert!(!on(&ui, shift), "Shift is for the next character only");
    goto(&mut nav, &mut f, caps, &settle);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert!(on(&ui, caps), "Caps engages");
    goto(&mut nav, &mut f, q, &settle);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert!(on(&ui, caps), "Caps stays on");
    goto(&mut nav, &mut f, layer, &settle);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert!(state.get_keys().iter().any(|k| i64::from(k.index) == q && k.label.as_str() == "1"), "123: the letters' keys show the symbols layer");
    goto(&mut nav, &mut f, layer, &settle);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    goto(&mut nav, &mut f, caps, &settle); // Caps off again, back on the letters
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    // Press every key but the two that leave (Cancel and Done, kept for last), each with its effect,
    // told by the label it shows as it is pressed: a character or space types, Del erases, the caret
    // keys move the caret, Clear empties the field.
    let label_at = |ui: &Shell, k: i64| ui.global::<crate::KeyboardState>().get_keys().iter().find(|x| i64::from(x.index) == k).unwrap().label.to_string();
    for k in 0..n as i64 {
        let label = label_at(&ui, k);
        if label == "Cancel" || label == "Done" {
            continue; // the walk's last two presses
        }
        goto(&mut nav, &mut f, k, &settle);
        let before = typed(&ui);
        nav(&mut f, "confirm", 1);
        settle(&mut f);
        match label.as_str() {
            "<" | ">" => assert_eq!(typed(&ui), before, "key {k}: the caret moves, nothing is typed"),
            "Clear" => assert_eq!(field(&ui), (String::new(), String::new()), "key {k} (Clear) empties the field"),
            "Del" => assert_eq!(typed(&ui) + 1, before, "key {k} (Del) erases the last character"),
            "Shift" | "Caps" | "123" | "abc" => assert_eq!(typed(&ui), before, "key {k} ({label}) switches a layer, nothing is typed"),
            _ => assert_eq!(typed(&ui), before + 1, "key {k} ({label}) types a character"),
        }
    }
    // The last two: Cancel closes the keyboard and EXIT comes back to the Part screen; Done accepts
    // the text to the engine — a `parts.name` request with the name, kept for the end.
    let cancel = index_of(&ui, "Cancel");
    goto(&mut nav, &mut f, cancel, &settle);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    assert!(!state.get_open(), "Cancel closes the keyboard");
    assert_eq!(ui.get_screen(), 4, "Cancel: EXIT comes back to the Part screen");
    nav(&mut f, "confirm", 1); // the name row again, a new keyboard
    settle(&mut f);
    assert!(state.get_open(), "the name opens the keyboard again");
    goto(&mut nav, &mut f, q, &settle);
    nav(&mut f, "confirm", 1);
    settle(&mut f);
    let done = index_of(&ui, "Done");
    goto(&mut nav, &mut f, done, &settle);
    nav(&mut f, "confirm", 1);
    let request = f.engine.try_request(std::time::Duration::from_secs(2)).expect("Done sends the part's new name");
    assert_eq!(request["method"], "parts.name");
    assert_eq!(request["params"]["part"], json!(10));
    assert_eq!(request["params"]["name"], "Keysq", "the name typed on the keyboard reaches the engine");
    settle(&mut f); // graph.commit and the rest
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

// A long name (40+ characters) in the caption and the field, at the keyboard's sizes: the overlay
// still fits under the bar and nothing leaves the window (the caption and the field cut a long text
// with an ellipsis, never wrapping it off).
#[test]
fn a_long_name_fits_the_keyboard_at_the_devices_sizes() {
    for (w, h, scale) in [(1280, 720, 1.55), (800, 480, 1.0)] {
        let mut s = Screen::sized(w, h);
        s.set_scale(scale);
        let name = "An unusually long part or scene name that keeps going past forty characters";
        assert!(name.chars().count() > 40, "a long name to test with");
        let _k = Keyboard::open(&s.ui, name, name, true);
        s.ui.set_screen(4);
        s.frame();
        let want = s.ui.global::<crate::KeyboardState>().get_overlay_height();
        let screen = h as f32 / scale;
        let bar = s.ui.global::<crate::Theme>().get_bar_height();
        assert!(want <= screen - bar, "a long name keeps the overlay under the bar: {want} of {screen}");
        shot(&s, &format!("keyboard-long-{w}x{h}@{scale}"));
    }
}

// The keyboard is an overlay whose own work is done when it is shown: with nothing changing it asks
// the engine nothing (it has no request of its own) and draws nothing over many ticks
// (docs/rust-ui.md, idle).
#[test]
fn at_rest_it_costs_no_request_and_no_frame() {
    let mut s = Screen::new();
    let ui = &s.ui;
    let _k = Keyboard::open(ui, "Name of the part", "Keys", true);
    s.ui.set_screen(4);
    assert!(s.frame().is_some(), "the keyboard draws once when shown");
    for _ in 0..20 {
        assert!(!s.animating(), "nothing on the keyboard animates");
        assert_eq!(s.frame(), None, "nothing is drawn at rest");
    }
}
