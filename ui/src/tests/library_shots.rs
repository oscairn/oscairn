// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pictures of the Library, through the shell and the fake engine, in three windows (the keyboard's 1280x720 at 1.55,
//! a desktop's at 1, a small device's 800x480) and three themes (dark, and the two odd ones: one with its tabs in
//! rails, one in strips), in target/screens as library-WINDOW-THEME-WHAT.png. Each is also checked: nothing is cut off the
//! window, and the rows are as many as the window holds.

use super::library::{labels, rows};
use super::library_fake::{stored, Library as Fake, Stored};
use super::library_shell::Shelf;
use super::*;
use crate::{NoticeState, PartState};

/// A library of `n` Performances under invented names, tagged and described, a few starred, locked or colored.
fn big(n: usize) -> Vec<Stored> {
    let first = ["Warm", "Glass", "Slow", "Bright", "Hollow", "Velvet", "Copper", "Misty", "Paper", "Quiet"];
    let second = ["Strings", "Choir", "Organ", "Keys", "Brass", "Pad", "Bells", "Reeds", "Harp", "Lead"];
    (0..n)
        .map(|i| {
            let mut s = stored("performance", &format!("p{i}"), &format!("{} {} {:03}", first[i % 10], second[(i / 10) % 10], i)).tags(&[["strings", "keys", "pad", "brass", "warm"][i % 5], ["live", "studio", "demo"][i % 3]]);
            if i % 7 == 0 {
                s = s.starred();
            }
            if i % 11 == 0 {
                s = s.locked("friend");
            }
            if i % 13 == 0 {
                s = s.colored("#3e63dd");
            }
            if i % 17 == 3 {
                s = s.described("A long description that does not fit on one line of the list however wide it is, to see it cut");
            }
            s
        })
        .collect()
}

fn sounds(shelf: &mut Shelf) {
    shelf.e.items.extend((0..12).map(|i| stored("sound", &format!("s{i}"), &format!("Soft Felt {i}")).tags(&["keys"])));
}

/// A bar action by its label, through "More" where the bar folds its actions into one.
fn bar_do(s: &mut Shelf, label: &str) {
    s.f.screen.frame();
    let ui = s.ui();
    let labels: Vec<String> = ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect();
    if ui.global::<Bar>().get_more() {
        ui.global::<Bar>().invoke_more_tapped();
        s.f.screen.frame(); // the menu counts the rows its window holds once it is drawn
        let menu: Vec<String> = ui.global::<crate::MenuState>().get_rows().iter().map(|r| r.label.to_string()).collect();
        let at = menu.iter().position(|l| l.starts_with(label)).unwrap_or_else(|| panic!("{label} in {menu:?}"));
        ui.global::<crate::MenuState>().invoke_tapped(at as i32);
    } else {
        let at = labels.iter().position(|l| l.starts_with(label)).unwrap_or_else(|| panic!("{label} in {labels:?}"));
        ui.global::<Bar>().invoke_action(at as i32);
    }
}

fn serve_and_draw(s: &mut Shelf, name: &str) {
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    png(&s.f.screen, name);
}

#[test]
fn pictures_of_the_library() {
    fit_to_window();
    for (window, (w, h, scale)) in [("keyboard", (1280, 720, 1.55f32)), ("desktop", (1280, 720, 1.)), ("small", (800, 480, 1.))] {
        for (theme_name, theme) in [("dark", theme::chosen(&theme::Places::default(), "oscairn-dark", "")), ("odd", odd_theme()), ("joined", joined_theme())] {
            let mut s = Shelf::new(&format!("lp-{}-{theme_name}", &window[..3]));
            let ui = s.ui();
            shell::library_in_rust(true);
            shell::set_text_on_screen(false); // a typing keyboard: the keys typed go to the keyboard's field
            s.f.screen.resize(w, h);
            s.f.screen.set_scale(scale);
            theme::apply(&ui, &theme);
            let shoot = |s: &mut Shelf, what: &str| serve_and_draw(s, &format!("library-{window}-{theme_name}-{what}"));
            // A list of ten, long names among them.
            s.e.items = big(10);
            s.e.items[2].name = "A Performance with a name that is far too long to fit in the row it is listed in".into();
            shell::goto(shell::Screen::Home);
            shell::goto(shell::Screen::Library);
            s.serve();
            shell::act(Action::Move { pane: None, by: 4 });
            shoot(&mut s, "list");
            // Five hundred: the cursor far down, then a search narrowing it.
            s.e.items = big(500);
            shell::goto(shell::Screen::Home);
            shell::goto(shell::Screen::Library);
            s.serve();
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Move { pane: None, by: 100 });
            shoot(&mut s, "500");
            assert!(rows(&ui).iter().any(|r| r.cursor), "the cursor stays in view");
            let held = rows(&ui).len();
            assert!((4..=40).contains(&held), "{held} rows in a {w}x{h} window at {scale}");
            bar_do(&mut s, "Search");
            for ch in "Velvet".chars() {
                shell::key(&ch.to_string());
            }
            shell::key(&slint::SharedString::from(slint::platform::Key::Return));
            shoot(&mut s, "500-search");
            // The filters, the tags.
            shell::act(Action::Move { pane: None, by: -1000 });
            assert_eq!(rows(&ui).iter().position(|r| r.cursor), Some(0), "the Filters row");
            shell::act(Action::Confirm(None));
            shoot(&mut s, "filters");
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Move { pane: None, by: 3 });
            shell::act(Action::Confirm(None));
            shoot(&mut s, "tags");
            shell::act(Action::Cancel(None));
            shell::act(Action::Cancel(None));
            // Sounds, then the Setlists tab, then an empty library.
            sounds(&mut s);
            ui.global::<crate::LibraryState>().invoke_show_page(1);
            shoot(&mut s, "sounds");
            ui.global::<crate::LibraryState>().invoke_show_page(2);
            s.e.items.retain(|i| i.kind != "setlist");
            shoot(&mut s, "setlists-empty");
            // An item's menu and a question over the screen.
            ui.global::<crate::LibraryState>().invoke_show_page(0);
            s.serve();
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Move { pane: None, by: 1 });
            bar_do(&mut s, "Options");
            shoot(&mut s, "menu");
            shell::act(Action::Cancel(None));
            s.e.open = ("p1".into(), "Warm Strings 000".into(), true);
            ui.global::<PartState>().invoke_tapped(3);
            s.serve();
            assert!(ui.global::<NoticeState>().get_open(), "screen {} menu {} {:?} {:?} {:?}", ui.get_screen(), ui.global::<crate::MenuState>().get_open(), labels(&ui), super::library::footer(&ui), s.e.methods());
            shoot(&mut s, "question");
            shell::act(Action::Cancel(None));
            // A load in progress: the engine busy, the row marked, the line says what is opening.
            s.e.open.2 = false;
            ui.global::<PartState>().invoke_tapped(4);
            let step = s.f.engine.request(); // performance.info: the engine is busy with another load
            s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "id": step["id"], "error": {"code": -32009, "message": "busy", "data": {"busy": "library.load", "ms": 2600}}})));
            s.f.pump();
            s.f.screen.frame();
            s.f.screen.frame();
            png(&s.f.screen, &format!("library-{window}-{theme_name}-loading"));
            shell::goto(shell::Screen::Home);
            s.serve();
            // Not opened: the engine's refusal, said.
            s.e.items[1].unfit = "needs 3 GB of memory".into();
            shell::goto(shell::Screen::Library);
            s.serve();
            ui.global::<PartState>().invoke_tapped(2);
            shoot(&mut s, "refused");
            shell::goto(shell::Screen::Home);
        }
    }
    let _ = (labels, Fake::new(vec![]));
}
