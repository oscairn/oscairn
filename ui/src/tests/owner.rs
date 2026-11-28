// SPDX-License-Identifier: AGPL-3.0-or-later
//! The owner's first fixes (2026-11-17): the top bar's More, the Library row's name, the rail. Each test reads the
//! Library as the owner's screenshot shows it (Performances, 1280x720 at 1.55, dark); with OWNER_SHOTS=dir the pictures
//! are written to dir as owner-N-before/after.png.

use super::library::rows;
use super::library_bubbles::press_tool;
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

/// A picture named as is (lib-B-search), when OWNER_SHOTS names a folder.
pub(super) fn lib_png(s: &Shelf, name: &str) {
    if let Ok(dir) = std::env::var("OWNER_SHOTS") {
        png_of(&dir, &s.f.screen.pixels, s.f.screen.w, s.f.screen.h, name);
    }
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
pub(super) fn last_character_shows_for(name: &str, full: &str) -> bool {
    let draw = |last: char| {
        let mut s = scene(name, 10);
        s.e.items[0].name = format!("{}{last}", &full[..full.len() - 1]);
        shell::goto(shell::Screen::Home);
        shell::goto(shell::Screen::Library);
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        let (w, ui) = (s.f.screen.w, s.ui());
        // The first row of the list, 1280x720 at 1.55, as wide as the name may be.
        let t = ui.global::<crate::Theme>();
        let (x0, y0) = (((ui.get_list_x() + t.get_margin()) * 1.55) as usize, ((ui.get_list_y() + t.get_margin()) * 1.55) as usize);
        let band: Vec<(u8, u8, u8)> = (y0..y0 + 66).flat_map(|y| (x0..x0 + 800).map(move |x| (x, y))).map(|(x, y)| s.f.screen.pixels[y * w + x]).map(|p| (p.r, p.g, p.b)).collect();
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
    assert!(last_character_shows_for("owner-row", LONG), "the name's last character is cut off the row");
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
    let x = ui.get_list_x() + ui.get_lane_x() + t.get_scroll_lane() / 2.;
    let (top, track) = (ui.get_list_y() + t.get_margin(), ui.get_track_h());
    let at = |f: f32| slint::LogicalPosition::new(x, top + f * track);
    ui.window().dispatch_event(WindowEvent::PointerPressed { position: at(from), button: PointerEventButton::Left });
    for i in 1..=steps {
        ui.window().dispatch_event(WindowEvent::PointerMoved { position: at(from + (to - from) * i as f32 / steps as f32) });
    }
    ui.window().dispatch_event(WindowEvent::PointerReleased { position: at(to), button: PointerEventButton::Left });
    s.serve();
    s.f.screen.frame();
}

// The bar's thumb shows the view's place and share of the list: a view of 6 in 10 rows
// is more than half of the track; in 300 rows it is the least length, a touch target, and it moves down the track with the
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
        let want = (track * per / n as f32).max(touch).min(track);
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

// The main list's scroll track is as tall as the rows its view shows (per-view touch rows and the gaps between them), measured
// against the rows' own places, not against the formula that sizes the track: the room leftover below the last row is not track.
// Normal and Compact.
#[test]
fn the_main_list_s_track_ends_at_the_last_row_of_its_view() {
    for compact in [false, true] {
        let mut s = scene("owner-track", 300);
        let ui = s.ui();
        if ui.global::<crate::LibraryState>().get_compact() != compact {
            press_tool(&mut s, 1);
        }
        let (t, st) = (ui.global::<crate::Theme>(), ui.global::<crate::PartState>());
        assert_eq!(ui.global::<crate::LibraryState>().get_compact(), compact);
        assert!(st.get_rows_per_view() > 1, "a list long enough for a track");
        let gap = t.invoke_dense(t.get_list_gap(), compact);
        let per = st.get_rows_per_view() as f32;
        // The view's rows: each a touch target with the gap between them, the last row's bottom is where the track ends.
        let drawn = per * (t.get_touch() + gap) - gap;
        let track = ui.get_track_h();
        eprintln!("compact {compact}: rows {per} end at {drawn}, track {track}");
        assert!((track - drawn).abs() < 0.5, "compact {compact}: the track is {track}, the rows the view shows end at {drawn}");
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

// The scroll bar can be hit with a finger: its lane is the theme's touch target and the visible bar fills it, a side margin
// from each side (issue #269: the bar was a third of the lane, with the lane's empty space beside it). lib-A-1 is the narrower
// lane tried, three quarters of a touch target; lib-A-2 the default.
#[test]
fn the_scroll_bar_is_wide_enough_to_tap() {
    for (n, lane_of_touch) in [(1, 0.75f32), (2, 1.0)] {
        let mut s = scene("owner-wide", 300);
        let ui = s.ui();
        let t = ui.global::<crate::Theme>();
        if n == 1 {
            t.set_scroll_lane(t.get_touch() * lane_of_touch);
        }
        shell::act(Action::Move { pane: None, by: 1 });
        shell::act(Action::Move { pane: None, by: 150 });
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        lib_png(&s, &format!("lib-A-{n}"));
        if n == 2 {
            assert!(t.get_scroll_lane() >= t.get_touch() - 0.01, "the lane is a touch target: {} of {}", t.get_scroll_lane(), t.get_touch());
        }
        // The bar fills the lane it is given: its thumb is the lane, less the side margins, wider than the old third.
        let pad = t.get_scroll_pad();
        let want = t.get_scroll_lane() - 2. * pad;
        assert!((ui.get_thumb_w() - want).abs() <= 1.5, "the bar fills the lane minus its side margins: {} against {} - 2*{}", ui.get_thumb_w(), t.get_scroll_lane(), pad);
        assert!(ui.get_thumb_w() >= t.get_scroll_lane() / 3. - 0.01, "the bar is wider than a third of the lane: {}", ui.get_thumb_w());
    }
}

// The Settings app's rail (a lone rail, as the Library's was) is wide enough for its longest label on one line, as far as 16%
// of the width allows, and shows its labels at the full small type where that is enough.
#[test]
fn the_settings_rail_holds_its_longest_label_on_one_line() {
    fit_to_window();
    for spacing in [1.0f32, 0.7] {
        let mut s = super::settings_shell::Shelf::new("owner-settings-rail");
        let ui = s.f.screen.ui.clone_strong();
        s.f.screen.resize(1280, 720);
        s.f.screen.set_scale(1.55);
        theme::apply(&ui, &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
        ui.global::<crate::Theme>().set_spacing(spacing);
        shell::goto(shell::Screen::Settings);
        s.serve();
        s.f.screen.frame();
        s.f.screen.frame();
        let t = ui.global::<crate::Theme>();
        let (rail, need, fit) = (ui.get_settings_rail(), ui.get_settings_rail_need(), ui.get_settings_rail_fit());
        let across = 2. * (t.get_tab_pad() + t.get_pad_tight());
        let cap = 1280. / 1.55 * t.get_rail_label_share();
        eprintln!("OWNER3 settings spacing {spacing}: rail {rail:.1}, need {need:.1} + {across:.1}, fit {fit}");
        assert!(need > 0., "the labels were measured");
        assert!(rail + 0.01 >= (need + across).min(cap), "the rail {rail} holds the label {need} and the padding {across} (cap {cap})");
        if need + across <= cap {
            assert_eq!(fit, 1.0, "the labels at the full small type");
        }
    }
}

// The scroll bar fills its lane: the bar is a small side margin (`scroll-pad`) from the lane's left (the list's side) and the
// same from the lane's right, so there is no band of empty lane beside it (issue #269), while the lane, the touch target, stays
// a full touch target wide and begins no further left than the rows' right padding, so a tap on the star or the warning icon is
// the row's, never the lane's. A non-flush bar (Part's list) keeps both margins. Pictures: polish-2 (and -blue).
#[test]
fn the_scroll_bar_sits_close_to_the_list() {
    for (name, theme) in [("polish-2", None), ("polish-2-blue", Some(super::theme_pictures::blue_square_theme()))] {
        let mut s = scene(name, 10);
        let ui = s.ui();
        if let Some(t) = theme {
            theme::apply(&ui, &t);
            s.f.screen.frame();
            s.f.screen.frame();
        }
        let t = ui.global::<crate::Theme>();
        let k = 1.55;
        let lane_left = ui.get_list_x() + ui.get_lane_x();
        assert!(t.get_scroll_lane() >= t.get_touch() - 0.01, "{name}: the lane is a touch target");
        // The bar fills the lane: its width is the lane less a side margin from each side (Part's list is not flush).
        let pad = t.get_scroll_pad();
        let lane = t.get_scroll_lane();
        let want = lane - 2. * pad;
        assert!((ui.get_thumb_w() - want).abs() <= 1.5, "{name}: the bar fills the lane minus its side margins: {} against {} - 2*{}", ui.get_thumb_w(), lane, pad);
        // As drawn: the first pixel of the bar's thumb (the muted color) in the first row's height is the lane's side margin
        // from the lane's left, in the lane (a bar at the far side would start `lane - 2*pad` away).
        let y = ((ui.get_list_y() + t.get_margin() + t.get_touch() / 2.) * k) as usize;
        let m = t.get_muted();
        let want = (m.red(), m.green(), m.blue());
        let w = s.f.screen.w;
        let from = (lane_left * k) as usize;
        let to = ((lane_left + lane) * k) as usize;
        let at = (from..to).find(|&x| {
            let p = s.f.screen.pixels[y * w + x];
            (p.r, p.g, p.b) == want
        });
        let left = at.expect("the thumb is drawn") as f32 / k - lane_left;
        eprintln!("{name}: the bar starts {left:.1} px into the lane (the side margin {})", t.get_scroll_pad());
        assert!((left - pad).abs() <= 2., "{name}: the bar starts its side margin {left} into the lane (the margin {})", t.get_scroll_pad());
        assert!(left + ui.get_thumb_w() <= lane, "{name}: the bar's right leaves its side margin too");
        lib_png(&s, name);
    }
}

/// A finger on the Library's list as the device delivers it: the shell turns a touch into a press, moves and a release
/// followed by the pointer leaving (Slint's touch state, `process_ended`); the same events here, `steps` moves from `from` to `to`.
fn finger(s: &Shelf, from: (f32, f32), to: (f32, f32), steps: usize) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let ui = s.ui();
    let w = ui.window();
    let at = |f: f32| slint::LogicalPosition::new(from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f);
    w.dispatch_event(WindowEvent::PointerPressed { position: at(0.), button: PointerEventButton::Left });
    for i in 1..=steps {
        w.dispatch_event(WindowEvent::PointerMoved { position: at(i as f32 / steps as f32) });
    }
    w.dispatch_event(WindowEvent::PointerReleased { position: at(1.), button: PointerEventButton::Left });
    w.dispatch_event(WindowEvent::PointerExited);
}

/// The Library with 300 Performances, the first six with a star or the warning.
fn icons_scene(name: &str) -> Shelf {
    let mut s = scene(name, 300);
    for (i, it) in s.e.items.iter_mut().enumerate().take(6) {
        *it = if i % 2 == 0 { it.clone().starred() } else { it.clone().unfit("needs 3 GB of memory") };
    }
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    s
}

/// Where row `row` of the list is, `from_right` logical pixels in from the rows' right end (the icons are there).
fn row_point(s: &Shelf, row: usize, from_right: f32) -> (f32, f32) {
    let ui = s.ui();
    let t = ui.global::<crate::Theme>();
    let x = ui.get_list_x() + ui.get_lane_x() - from_right;
    (x, ui.get_list_y() + t.get_margin() + row as f32 * (t.get_touch() + t.get_list_gap()) + t.get_touch() / 2.)
}

// A finger drag on rows that carry a star or the warning scrolls the list row for row (starting on the icon too), opens no row,
// asks the engine for nothing while it moves (the whole list is held), and a flick of two big moves does not open one at its end;
// a press and release in place still opens the row.
#[test]
fn a_finger_drag_scrolls_from_a_row_with_an_icon() {
    let mut s = icons_scene("drag-icons");
    let ui = s.ui();
    let t = ui.global::<crate::Theme>();
    let pitch = t.get_touch() + t.get_list_gap();
    let first = || ui.global::<crate::PartState>().get_first();
    for (row, from_right) in [(2, 20.), (1, 20.), (0, 20.), (3, 300.)] {
        let before = (first(), s.e.calls.len());
        let p = row_point(&s, row, from_right);
        finger(&s, p, (p.0, p.1 - 3. * pitch), 30);
        s.serve();
        assert_eq!(first(), before.0 + 3, "row {row} at {from_right}: the list followed three rows");
        assert!(rows(&ui).iter().all(|r| !r.cursor), "row {row}: a drag opens nothing");
        assert_eq!(s.e.calls.len(), before.1, "row {row}: the drag asked the engine for {:?}", &s.e.calls[before.1.min(s.e.calls.len())..]);
        let back = (first(), s.e.calls.len());
        finger(&s, p, (p.0, p.1 + 3. * pitch), 30);
        s.serve();
        assert_eq!(first(), back.0.saturating_sub(3), "dragging down goes back");
        assert_eq!(s.e.calls.len(), back.1);
    }
    // A flick: two big moves, then the release.
    let before = (first(), s.e.calls.len());
    let p = row_point(&s, 4, 20.);
    finger(&s, p, (p.0, p.1 - 4. * pitch), 2);
    s.serve();
    assert!(first() > before.0, "a flick scrolls");
    assert!(rows(&ui).iter().all(|r| !r.cursor) && s.e.calls.len() == before.1, "a flick's end opens nothing: {:?}", s.e.calls);
    // A tap in place on a starred row.
    let p = row_point(&s, 0, 20.);
    let n = s.e.called("performance.info").len();
    finger(&s, p, p, 0);
    s.serve();
    assert!(s.e.called("performance.info").len() > n || rows(&ui).iter().any(|r| r.cursor), "a tap opens the row");
}

// A drag that starts on the scroll bar's lane drags the bar (and nothing else: no row opens, no request).
#[test]
fn a_drag_from_the_scroll_bar_lane_still_drags_the_bar() {
    let mut s = icons_scene("drag-bar");
    let ui = s.ui();
    let calls = s.e.calls.len();
    drag_bar(&mut s, 0.0, 0.5, 20);
    let first = ui.global::<crate::PartState>().get_first();
    assert!(first > 100, "the bar moved the list to {first}");
    assert!(rows(&ui).iter().all(|r| !r.cursor) && s.e.calls.len() == calls, "{:?}", &s.e.calls[calls.min(s.e.calls.len())..]);
}

