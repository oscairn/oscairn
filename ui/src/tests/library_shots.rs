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
pub(super) fn bar_do(s: &mut Shelf, label: &str) {
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
            // (A name the search left on the screen still finds.)
            s.e.items[1].name = "Velvet Aaa 000".into();
            s.e.items[1].unfit = "needs 3 GB of memory".into();
            shell::goto(shell::Screen::Library);
            s.serve();
            press(&mut s, "Velvet Aaa 000");
            shoot(&mut s, "refused");
            assert_eq!(super::library::footer(&ui).0, "Not opened: Velvet Aaa 000 does not fit this device: needs 3 GB of memory", "the refusal is said under the list");
            shell::goto(shell::Screen::Home);
        }
    }
    let _ = (labels, Fake::new(vec![]));
}

/// A row of the list pressed as the dial does: the cursor moved onto it, then Confirm; the shell answered.
fn press(s: &mut Shelf, label: &str) {
    let ui = s.ui();
    let names: Vec<String> = super::library::labels(&ui);
    let target = names.iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {names:?}"));
    let now = super::library::rows(&ui).iter().position(|r| r.cursor);
    match now {
        None => {
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Move { pane: None, by: target as i32 });
        }
        Some(at) => shell::act(Action::Move { pane: None, by: target as i32 - at as i32 }),
    }
    shell::act(Action::Confirm(None));
    s.serve();
}

/// A menu's choice by its label, tapped, the menu's view turned to it when it is not in the first.
fn tap_menu(s: &mut Shelf, label: &str) {
    s.f.screen.frame();
    let ui = s.ui();
    for _ in 0..6 {
        let menu: Vec<String> = ui.global::<crate::MenuState>().get_rows().iter().map(|r| r.label.to_string()).collect();
        if let Some(at) = menu.iter().position(|l| l == label) {
            ui.global::<crate::MenuState>().invoke_tapped(at as i32);
            s.serve();
            return;
        }
        ui.global::<crate::MenuState>().invoke_turn(1);
    }
    panic!("{label} not in the menu");
}

// Pictures of the second cut's screens: the file chooser, the install choices with the warnings, the export choices, the
// Libraries menu, the Catalog (reading, listed, a package) and the import choices, in the keyboard's window and, in the
// dark theme, the others.
#[test]
fn pictures_of_the_second_cut() {
    fit_to_window();
    let dir = std::env::temp_dir().join(format!("oscairn-ui-shots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Songs and Sounds")).unwrap();
    std::fs::write(dir.join("Aaa Friend Pack.olib"), b"x").unwrap();
    std::fs::write(dir.join("A Performance With A Very Long File Name To See How It Is Cut Off.olib"), vec![0u8; 9000]).unwrap();
    for i in 0..30 {
        std::fs::write(dir.join(format!("Archive {i:02}.olib")), b"x").unwrap();
    }
    let code = json!([
        {"kind": "clap", "name": "Fuzz Box", "version": "1.2", "platform": "linux-arm64", "status": "unknown", "key": "aaaabbbbccccdddd", "keyName": "", "publicKey": "pub"},
        {"kind": "clap", "name": "Hiss", "version": "0.1", "platform": "macos-arm64", "status": "unsigned", "key": "", "keyName": "", "publicKey": ""},
        {"kind": "clap", "name": "Old Organ", "version": "3.0", "platform": "linux-arm64", "status": "revoked", "key": "1111222233334444", "keyName": "Pebble", "publicKey": "p2"}]);
    let archive = json!({"id": "friend-pack", "name": "Friend Pack", "kind": "library", "items": [{"kind": "performance", "id": "x1", "name": "Dawn Pad"}, {"kind": "sound", "id": "x2", "name": "Warm Bass"}, {"kind": "performance", "id": "x3", "name": "Dusk Lead With A Name Long Enough To Be Cut"}],
        "code": code, "signer": {"kind": "archive", "name": "", "version": "", "platform": "", "status": "unknown", "key": "0123456789abcdef", "keyName": "Quill", "publicKey": ""}, "license": "CC0-1.0", "source": {"url": "https://example.org/friend-pack", "revision": "v1.2.0"}});
    let pkg = |id: &str, name: &str, types: serde_json::Value| json!({"id": id, "name": name, "summary": "An invented package with a summary long enough that it has to be cut", "license": "CC0", "publisher": "Quill Audio Works", "publisherKey": "0123456789abcdef", "tags": ["demo"],
        "versions": [{"version": "1.2.0", "channel": "stable", "yanked": "", "incompatible": "", "sha256": "aabbccddeeff00112233", "size": 5242880, "installed": 12582912, "types": types, "dependencies": [{"id": "org.example.base", "version": "1.0"}], "withdrawn": null,
            "exports": [{"platform": "linux-arm64", "address": "https://repo.example/exports/a/1.2.0/a-1.2.0-linux-arm64.olib"}]}]});
    for (window, (w, h, scale), themes) in [("keyboard", (1280, 720, 1.55f32), vec!["dark", "odd", "joined"]), ("desktop", (1280, 720, 1.), vec!["dark"]), ("small", (800, 480, 1.), vec!["dark"])] {
        for theme_name in themes {
            let theme = match theme_name {
                "dark" => theme::chosen(&theme::Places::default(), "oscairn-dark", ""),
                "odd" => odd_theme(),
                _ => joined_theme(),
            };
            let mut s = Shelf::new(&format!("l2-{}-{theme_name}", &window[..3]));
            let ui = s.ui();
            shell::library_in_rust(true);
            shell::set_text_on_screen(false);
            s.f.screen.resize(w, h);
            s.f.screen.set_scale(scale);
            theme::apply(&ui, &theme);
            s.e.items = big(40);
            s.e.libraries = vec![("friend".into(), "A Friend's".into())];
            s.e.archives = vec![(dir.join("Aaa Friend Pack.olib").display().to_string(), archive.clone())];
            s.e.keys = vec![("stage-key".into(), "0011223344556677".into())];
            s.e.folders = vec!["/music/Samples".into()];
            s.e.plugins = vec![json!({"format": "clap", "id": "org.example.fuzz", "version": "1.2", "name": "Fuzz Box", "platforms": ["linux-arm64", "macos-arm64"], "bytes": 3_000_000, "usedBy": ["Warm Strings 000"], "open": false, "license": ""})];
            s.e.repos = vec![("https://repo.example".into(), "Example Repository".into(), true)];
            s.e.repo_packages = vec![("https://repo.example".into(), Ok((0..14).map(|i| pkg(&format!("org.example.p{i}"), &format!("Invented Package {i:02}"), if i % 3 == 0 { json!({"clap": 1}) } else { json!({"oscairn.performance": 2}) })).collect()))];
            s.e.job_polls = 0;
            let shoot = |s: &mut Shelf, what: &str| {
                s.f.screen.frame();
                s.f.screen.frame();
                png(&s.f.screen, &format!("library2-{window}-{theme_name}-{what}"));
            };
            shell::goto(shell::Screen::Library);
            s.serve();
            // The file chooser, from Install... then File.
            s.e.items[0].favorite = true;
            shell::library_files_dir(dir.clone());
            super::library_shots::bar_do(&mut s, "Install");
            tap_menu(&mut s, "File");
            shoot(&mut s, "files");
            // The install choices, with the warnings.
            press(&mut s, "Aaa Friend Pack.olib");
            shoot(&mut s, "install");
            shell::act(Action::Cancel(None)); // Back: out of the choices
            s.serve();
            // The export choices.
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Move { pane: None, by: 2 });
            super::library_shots::bar_do(&mut s, "Options");
            tap_menu(&mut s, "Export...");
            press(&mut s, "Save here");
            shoot(&mut s, "export");
            shell::act(Action::Cancel(None));
            s.serve();
            // The Libraries menu.
            super::library_shots::bar_do(&mut s, "Libraries");
            s.serve();
            shoot(&mut s, "libraries");
            shell::act(Action::Cancel(None));
            // The Catalog, being read and listed, a package, and the questions.
            super::library_shots::bar_do(&mut s, "Install");
            tap_menu(&mut s, "Catalog");
            shoot(&mut s, "catalog");
            ui.global::<crate::LibraryState>().invoke_show_page(2);
            shoot(&mut s, "catalog-plugins");
            press(&mut s, "Invented Package 00  1.2.0");
            shoot(&mut s, "package");
            shell::act(Action::Cancel(None));
            shell::act(Action::Cancel(None));
            s.serve();
            // The import choices.
            shell::act(Action::Move { pane: None, by: 1 });
            shell::act(Action::Move { pane: None, by: 2 });
            super::library_shots::bar_do(&mut s, "Options");
            tap_menu(&mut s, "Import into the open Performance...");
            shoot(&mut s, "import");
            shell::goto(shell::Screen::Home);
        }
    }
}

// A rail label too wide for the rail even at 70% of the type goes across two lines before it is cut: the Library's selected
// tab reads "Performances" in two lines (the pixels of its fill read for lines of ink), and a short name stays in one.
#[test]
fn a_rail_label_that_does_not_fit_at_seventy_percent_wraps_to_two_lines() {
    fit_to_window();
    let mut s = Shelf::new("rail-wrap");
    let ui = s.ui();
    shell::library_in_rust(true);
    s.f.screen.resize(1280, 720);
    s.f.screen.set_scale(1.55);
    theme::apply(&ui, &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
    s.e.items = big(10);
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    serve_and_draw(&mut s, "library-rail-wrap");
    let screen = &s.f.screen;
    let theme = ui.global::<crate::Theme>();
    let (on, ink) = (theme.get_tab_on(), theme.get_tab_on_ink());
    let same = |p: slint::Rgb8Pixel, c: slint::Color| (p.r, p.g, p.b) == (c.red(), c.green(), c.blue());
    let rail = (theme.get_rail_width() * 1.55) as usize;
    // The selected tab's rows: where the fill is at the rail's left edge.
    let rows: Vec<usize> = (0..screen.h).filter(|&y| same(screen.pixels[y * screen.w + 6], on)).collect();
    assert!(rows.len() > 40, "the selected tab is in the rail: {} rows", rows.len());
    let mut bands = 0;
    let mut in_band = false;
    for &y in &rows {
        let has_ink = (0..rail).any(|x| same(screen.pixels[y * screen.w + x], ink));
        if has_ink && !in_band {
            bands += 1;
        }
        in_band = has_ink;
    }
    assert_eq!(bands, 2, "the selected tab's label is in two lines");
}
