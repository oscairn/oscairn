// SPDX-License-Identifier: AGPL-3.0-or-later
//! The owner's first fixes (2026-11-17): the top bar's More, the Library row's name, the rail. Each test reads the
//! Library as the owner's screenshot shows it (Performances, 1280x720 at 1.55, dark); with OWNER_SHOTS=dir the pictures
//! are written to dir as owner-N-before/after.png.

use super::library::rows;
use super::library_fake::stored;
use super::library_shell::Shelf;
use super::*;

pub(super) const LONG: &str = "A Very Long Performance Name For Testing 1";

/// The Library's Performances as the screenshot has them: the long name first, two tags each.
pub(super) fn scene(name: &str, n: usize) -> Shelf {
    fit_to_window();
    let mut s = Shelf::new(name);
    let ui = s.ui();
    shell::library_in_rust(true);
    s.f.screen.resize(1280, 720);
    s.f.screen.set_scale(1.55);
    theme::apply(&ui, &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
    s.e.items = (0..n)
        .map(|i| {
            let nm = if i == 0 { LONG.to_string() } else { format!("Bright Strings {:03}", i + 2) };
            let it = stored("performance", &format!("p{i}"), &nm).tags(&[["keys", "brass", "pad"][i % 3], ["studio", "live"][i % 2]]);
            if i == 1 { it.described("A long description that does not fit on one line of the list however wide it is, to see it cut") } else { it }
        })
        .collect();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    s
}

/// The picture, when OWNER_SHOTS names a folder.
pub(super) fn owner_png(s: &Shelf, name: &str) {
    if let Ok(dir) = std::env::var("OWNER_SHOTS") {
        let tag = std::env::var("OWNER_TAG").unwrap_or_else(|_| "after".into());
        png_of(&dir, &s.f.screen.pixels, s.f.screen.w, s.f.screen.h, &format!("{name}-{tag}"));
    }
}

// More is an icon at the right end of the action area, directly left of the meters, and gives its width to the title.
#[test]
fn more_is_an_icon_left_of_the_meters() {
    let mut s = scene("owner-more", 10);
    let ui = s.ui();
    // The chip's room: how much of the name shows.
    ui.global::<Bar>().set_loading(format!("Loading {LONG}").into());
    s.f.screen.frame();
    s.f.screen.frame();
    owner_png(&s, "owner-1");
    let pw = ui.global::<crate::Theme>().get_pad_wide();
    let (chip, full) = (ui.get_loading_width(), ui.get_loading_full());
    let shown = (format!("Loading {LONG}").chars().count() as f32 * (chip - 2. * pw) / (full - 2. * pw)).floor();
    eprintln!("OWNER1 chip {chip} of {full}, about {shown} characters");
    assert!(ui.global::<Bar>().get_more(), "folded at this width");
    let (mx, mw) = (ui.get_more_x(), ui.get_more_width());
    assert!((mw - 36.).abs() < 0.5, "an icon button's width, not the word's: {mw}");
    assert!(mx >= ui.get_tempo_x() + ui.get_tempo_w() - 0.5, "right of the tempo: {mx}");
    assert!((ui.get_resources_x() - (mx + mw)).abs() < 12., "directly left of the meters: {} {}", mx + mw, ui.get_resources_x());
    assert!(chip <= ui.get_pill_width() / 2. + 0.5);
    // Half of the title area, which is wider by what More gave up (a word and its padding, 62, for an icon, 36): 193 before.
    assert!(chip > 198., "the chip: {chip}");
}

/// The same scene with the last character of the long name changed: whether the row is drawn differently, that is,
/// whether that character is drawn at all (when it is cut off, the two pictures are alike).
fn last_character_shows(name: &str) -> bool {
    let draw = |last: char| {
        let mut s = scene(name, 10);
        s.e.items[0].name = format!("{}{last}", &LONG[..LONG.len() - 1]);
        shell::goto(shell::Screen::Home);
        shell::goto(shell::Screen::Library);
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        let w = s.f.screen.w;
        // The second row of the list (the first is Filters): 1280x720 at 1.55, left of the panel.
        let band: Vec<(u8, u8, u8)> = (160..226).flat_map(|y| (120..640).map(move |x| (x, y))).map(|(x, y)| s.f.screen.pixels[y * w + x]).map(|p| (p.r, p.g, p.b)).collect();
        (band, s)
    };
    let (a, s) = draw('1');
    let (b, _) = draw('7');
    owner_png(&s, "owner-2");
    a != b
}

// The row's name takes the room the row has: with two tags the whole 42-character name is drawn, in the Performances list
// at 1280x720; tags give way to the name, and the description to the tags.
#[test]
fn a_row_s_name_is_drawn_whole_when_the_row_has_the_room() {
    assert!(last_character_shows("owner-row"), "the name's last character is cut off the row");
}

// The rail is wide enough for its longest label on one line: "Performances" whole, at the default spacing and at 0.7.
#[test]
fn the_rail_holds_its_longest_label_on_one_line() {
    for spacing in [1.0f32, 0.7] {
        let mut s = scene("owner-rail", 10);
        let ui = s.ui();
        ui.global::<crate::Theme>().set_spacing(spacing);
        s.f.screen.frame();
        s.f.screen.frame();
        owner_png(&s, &format!("owner-3-{spacing}"));
        let (rail, need, fit) = (ui.get_library_rail(), ui.get_library_rail_need(), ui.get_library_rail_fit());
        let across = 2. * (ui.global::<crate::Theme>().get_tab_pad() + ui.global::<crate::Theme>().get_pad_tight());
        eprintln!("OWNER3 spacing {spacing}: rail {rail:.1}, need {need:.1} + {across:.1}, fit {fit}");
        assert!(need > 0. && rail + 0.01 >= need + across, "the rail {rail} holds the label {need} and the padding {across}");
        assert_eq!(fit, 1.0, "the label at the full small type");
    }
}

// A long list drawn scrolled to the middle (the picture alone).
#[test]
fn a_picture_of_a_long_list() {
    let mut s = scene("owner-long", 300);
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Move { pane: None, by: 150 });
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    owner_png(&s, "owner-4");
}

/// A press, a drag in `steps` moves and a release, on the Library's scroll bar from `from` to `to` (fractions of its track).
fn drag_bar(s: &mut Shelf, from: f32, to: f32, steps: usize) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let ui = s.ui();
    let t = ui.global::<crate::Theme>();
    // The list sits at the rail's right edge (the content's x less a margin), just under the bar.
    let x = ui.get_library_rail() + ui.get_lane_x() + t.get_scroll_lane() / 2.;
    let (top, track) = (t.get_bar_height() + t.get_margin(), ui.get_track_h());
    let at = |f: f32| slint::LogicalPosition::new(x, top + f * track);
    ui.window().dispatch_event(WindowEvent::PointerPressed { position: at(from), button: PointerEventButton::Left });
    for i in 1..=steps {
        ui.window().dispatch_event(WindowEvent::PointerMoved { position: at(from + (to - from) * i as f32 / steps as f32) });
    }
    ui.window().dispatch_event(WindowEvent::PointerReleased { position: at(to), button: PointerEventButton::Left });
    s.serve();
    s.f.screen.frame();
}

// The bar's thumb shows the view's place and share of the list: a view of 6 in 11 rows (the Filters row and ten Performances)
// is more than half of the track; in 301 rows it is the least length, a touch target, and it moves down the track with the
// view. The rows keep clear of the bar's lane.
#[test]
fn the_scroll_bar_shows_where_the_view_is_in_the_list() {
    for (n, long) in [(10usize, true), (300, true)] {
        let mut s = scene("owner-thumb", n);
        let ui = s.ui();
        let (touch, track) = (ui.global::<crate::Theme>().get_touch(), ui.get_track_h());
        let (y, h) = (ui.get_thumb_y(), ui.get_thumb_h());
        let per = ui.global::<crate::PartState>().get_rows_per_view() as f32;
        assert!(long && h >= touch - 0.01 && y.abs() < 0.01, "{n}: thumb {y} {h}");
        let want = (track * per / (n as f32 + 1.)).max(touch).min(track);
        assert!((h - want).abs() < 0.01, "{n}: the thumb is {h}, the view's share {want} of {track}");
        if n == 10 {
            assert!(h > track / 2., "6 of 11 is over half");
        } else {
            assert_eq!(h, touch, "300 rows: the least length");
        }
        // The last view: the thumb at the foot of the track.
        shell::act(Action::Move { pane: None, by: 1 });
        shell::act(Action::Move { pane: None, by: 100_000 });
        s.f.screen.frame();
        assert!((ui.get_thumb_y() + ui.get_thumb_h() - track).abs() < 0.01, "{n}: at the foot {} {}", ui.get_thumb_y(), ui.get_thumb_h());
    }
}

// A tap on the track jumps there; a drag across 300 rows follows the finger, the footer ("Performances 151-156 of 300")
// follows the view, and the drag asks the engine for nothing (the list is held whole: 0 requests, at most 0 allowed).
#[test]
fn the_scroll_bar_is_tapped_and_dragged() {
    let mut s = scene("owner-drag", 300);
    let ui = s.ui();
    let first_shown = |s: &Shelf| -> usize {
        let text = super::library::footer(&s.ui()).1;
        text.split_whitespace().nth(1).and_then(|r| r.split('–').next()).and_then(|n| n.parse().ok()).unwrap_or_else(|| panic!("a footer with a range: {text:?}"))
    };
    assert_eq!(first_shown(&s), 1);
    s.e.calls.clear();
    // A tap at the middle of the track.
    drag_bar(&mut s, 0.5, 0.5, 0);
    let mid = first_shown(&s);
    assert!((100..=200).contains(&mid), "a tap in the middle of 300 rows jumps to about row 150, not a page: {mid}");
    assert!(rows(&ui).iter().all(|r| !r.cursor), "scrolling selects nothing");
    // A drag from the top to the foot, in a hundred moves.
    drag_bar(&mut s, 0.02, 0.98, 100);
    let last = first_shown(&s);
    assert!(last > 280, "the foot of the list: {last}");
    assert!((ui.get_thumb_y() + ui.get_thumb_h() - ui.get_track_h()).abs() < 1., "the thumb is at the foot");
    assert!(s.e.calls.is_empty(), "a drag asks the engine for nothing: {:?}", s.e.methods());
    // And back to the top, the footer in step.
    drag_bar(&mut s, 0.98, 0.0, 20);
    assert_eq!(first_shown(&s), 1);
}
