// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pictures of the Settings app, through the shell and the fake engine, in three windows (the keyboard's 1280x720 at 1.55, a
//! desktop's at 1, a small device's 800x480), three themes (dark, and the two odd ones: one with its tabs in rails, one in
//! strips) and the two kinds of profile (the keyboard's, which fixes its audio device so has no Audio page, and a
//! computer's), in target/screens as settings-PROFILE-WINDOW-THEME-WHAT.png. Each is also checked: the rows are as many as the
//! window holds, and the cursor stays in view.

use super::library::{labels, rows};
use super::settings::{folder, installed_theme};
use super::settings_fake::{dev, port, Settings as Fake};
use super::settings_shell::Shelf;
use super::*;
use crate::{MenuState, NoticeState};

/// An engine with a good deal to show: long names, many ports and keys, a repository of the user's.
fn busy_engine() -> Fake {
    let mut e = Fake::new();
    e.audio.outputs.push(dev("A Studio Interface With A Name That Is Far Too Long To Fit In The Row", &[44100, 48000, 88200, 96000, 176400, 192000], &[16, 32, 64, 128, 256, 512, 1024, 2048]));
    e.ports = (0..9).map(|i| port(&format!("Controller Number {i} With A Long Name"), &[("a", "First Profile"), ("b", "Second Profile")])).collect();
    e.ports[3].ignored = true;
    e.ports[4].profiles.clear();
    e.keys = vec![("Stage".into(), "30fa782829102c1859d24993dc0349bebd65c09993f440a937213c2c50e81ed2".into()), ("Home-Studio_2".into(), "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".into())];
    e.repos.push(super::settings_fake::Repo { address: "https://repository.example.org/with/a/long/path/to/its/index".into(), name: "A Friend's Instruments".into(), key: "30fa782829102c1859d24993dc0349bebd65c09993f440a937213c2c50e81ed2".into(), origin: "user".into(), enabled: false });
    let package = |id: &str, name: &str, version: &str, kind: &str, repository: &str, signers: &[&str]| json!({"id": id, "version": version, "kind": kind, "repository": repository, "sha256": "", "signers": signers, "otherBuild": false, "withdrawn": null, "dependency": false, "name": name});
    e.installed_packages = vec![
        package("org.example.reverb", "Example Reverb", "1.0.0", "plugin", "https://packages.oscairn.org", &["6a2dd1acf4d4e23c"]),
        package("org.example.a.WonderfullyLongPackageIdentifierForSomeSoundLibrary", "A Sound Library With A Name That Is Far Too Long To Fit In The Row", "0.9.0", "library", "https://friend.example.org", &["89074b634c0de731"]),
        package("org.example.strings", "example strings", "2.1.0", "asset", "", &[]),
        package("org.example.sample-types", "Sample types", "1.0.0", "library", "https://packages.oscairn.org", &["6a2dd1acf4d4e23c"]),
        package("lib-1", "Jacob's Performances", "", "library", "", &[]),
        package("lib-2", "Field Recordings", "", "library", "", &[]),
    ];
    e.libraries = vec![("lib-1".into(), "Jacob's Performances".into(), 87), ("lib-2".into(), "Field Recordings".into(), 34)];
    e.removal = vec![(
        "org.example.reverb".into(),
        vec![json!({"kind": "library", "id": "lib-1", "name": "Jacob's Performances", "version": "", "items": 87}), json!({"kind": "own", "id": "", "name": "", "version": "", "items": 3, "examples": ["Keys Split", "Pad", "Lead"]})],
        vec![json!({"id": "org.example.strings", "version": "2.1.0", "kind": "asset", "name": "example strings", "bytes": 1258291}), json!({"id": "org.example.sample-types", "version": "1.0.0", "kind": "library", "name": "Sample types", "bytes": 612})],
    )];
    e
}

/// A bar action by its label, through "More" where the bar folds its actions into one.
fn bar_do(s: &mut Shelf, label: &str) {
    s.f.screen.frame();
    let ui = s.ui();
    let labels: Vec<String> = ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect();
    if ui.global::<Bar>().get_more() {
        ui.global::<Bar>().invoke_more_tapped();
        s.f.screen.frame(); // the menu counts the rows its window holds once it is drawn
        let menu: Vec<String> = ui.global::<MenuState>().get_rows().iter().map(|r| r.label.to_string()).collect();
        let at = menu.iter().position(|l| l.starts_with(label)).unwrap_or_else(|| panic!("{label} in {menu:?}"));
        ui.global::<MenuState>().invoke_tapped(at as i32);
    } else {
        let at = labels.iter().position(|l| l.starts_with(label)).unwrap_or_else(|| panic!("{label} in {labels:?}"));
        ui.global::<Bar>().invoke_action(at as i32);
    }
}

fn shoot(s: &mut Shelf, name: &str) {
    s.serve();
    s.f.screen.frame();
    s.f.screen.frame();
    png(&s.f.screen, name);
}

#[test]
fn pictures_of_the_settings_app() {
    fit_to_window();
    for (kind, profile) in [("device", Some("fantom-07")), ("computer", None)] {
        for (window, (w, h, scale)) in [("keyboard", (1280, 720, 1.55f32)), ("desktop", (1280, 720, 1.)), ("small", (800, 480, 1.))] {
            for (theme_name, theme) in [("dark", theme::chosen(&theme::Places::default(), "oscairn-dark", "")), ("odd", odd_theme()), ("joined", joined_theme())] {
                let mut s = Shelf::on(&format!("sp-{kind}-{}-{theme_name}", &window[..3]), profile, busy_engine());
                let ui = s.ui();
                shell::settings_in_rust(true);
                shell::set_text_on_screen(false);
                s.f.screen.resize(w, h);
                s.f.screen.set_scale(scale);
                theme::apply(&ui, &theme);
                installed_theme(&s.config, "evening-glass", "Evening Glass With A Rather Long Name Indeed", |d| d["spacing"] = json!(1.3));
                let one = |what: &str| format!("settings-{kind}-{window}-{theme_name}-{what}");
                shell::goto(shell::Screen::Home);
                shell::goto(shell::Screen::Settings);
                s.serve();
                let pages = crate::tests::settings::tabs(&ui);
                for (i, page) in pages.iter().enumerate() {
                    ui.global::<crate::SettingsState>().invoke_show_page(i as i32);
                    s.serve();
                    shell::act(Action::Move { pane: None, by: 1 });
                    shell::act(Action::Move { pane: None, by: 2 });
                    shoot(&mut s, &one(&page.to_lowercase().replace(' ', "-")));
                    assert!(rows(&ui).iter().any(|r| r.cursor) || labels(&ui).len() < 3, "{page}: the cursor is in view");
                    let held = rows(&ui).len();
                    assert!(held >= 2 || labels(&ui).len() < 2, "{page}: {held} rows in a {w}x{h} window at {scale}");
                }
                // A choice open over the Top bar page, a question over the repositories, an error under the audio device.
                ui.global::<crate::SettingsState>().invoke_show_page(1);
                s.serve();
                ui.global::<crate::PartState>().invoke_tapped(0);
                s.f.screen.frame();
                shoot(&mut s, &one("menu"));
                assert!(ui.global::<MenuState>().get_open());
                shell::act(Action::Cancel(None));
                let repos = pages.iter().position(|p| p == "Package repositories").unwrap() as i32;
                ui.global::<crate::SettingsState>().invoke_show_page(repos);
                s.serve();
                shell::act(Action::Move { pane: None, by: 1 });
                shell::act(Action::Move { pane: None, by: 1 });
                bar_do(&mut s, "Remove");
                shoot(&mut s, &one("question"));
                assert!(ui.global::<NoticeState>().get_open());
                shell::act(Action::Cancel(None));
                if let Some(audio) = pages.iter().position(|p| p == "Audio") {
                    s.e.audio.cannot_open = vec![("Studio Interface".into(), "Studio Interface is in use by another program and cannot be opened".into())];
                    ui.global::<crate::SettingsState>().invoke_show_page(audio as i32);
                    s.serve();
                    shell::act(Action::Move { pane: None, by: 1 });
                    shell::act(Action::Confirm(None));
                    s.f.screen.frame();
                    ui.global::<MenuState>().invoke_tapped(1);
                    shoot(&mut s, &one("audio-refused"));
                }
            }
        }
    }
    let _ = folder("shots");
}

/// The WCAG relative luminance of a color, 0 to 1.
fn luminance(c: slint::Color) -> f64 {
    let f = |v: u8| {
        let v = f64::from(v) / 255.;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powi(12) }
    };
    0.2126 * f(c.red()) + 0.7152 * f(c.green()) + 0.0722 * f(c.blue())
}

/// The WCAG contrast ratio of `a` over `b`, 1 to 21.
fn contrast(a: slint::Color, b: slint::Color) -> f64 {
    let (l1, l2) = (luminance(a), luminance(b));
    let (hi, lo) = (l1.max(l2), l1.min(l2));
    (hi + 0.05) / (lo + 0.05)
}

// The Audio and MIDI controllers pages in the light theme: the shots use the dark and the two odd themes only. Body
// text on the rows' surface and the faint rows' ink read 4.5:1 against the surface (the theme loader holds every
// theme to it, theme/tests.rs "Text must read on the three backgrounds"), no dark theme's surface color is left
// (a literal in the shared row would draw it), and the cursor's focus border draws and shows.
#[test]
// target/screens/settings-light-{audio,controllers}.png
fn the_audio_and_controllers_pages_draw_light() {
    fit_to_window();
    let mut s = Shelf::on("light-settings", None, Fake::new()); // a computer: it has the Audio page
    let ui = s.ui();
    shell::settings_in_rust(true);
    shell::set_text_on_screen(false);
    s.f.screen.resize(1280, 720);
    s.f.screen.set_scale(1.);
    let light = theme::chosen(&theme::Places::default(), "oscairn-light", "");
    theme::apply(&ui, &light);
    let d = &light.data;
    assert_eq!(ui.global::<crate::Theme>().get_id().as_str(), "oscairn-light");
    assert!(!ui.global::<crate::Theme>().get_dark(), "a light background");
    let row = d.ink("surface");
    let (text, muted) = (d.ink("text"), d.ink("muted"));
    assert!(contrast(text, row) >= 4.5, "text {text:#?} on {row:#?}: {:.2}:1", contrast(text, row));
    assert!(contrast(muted, row) >= 4.5, "muted {muted:#?} on {row:#?}: {:.2}:1", contrast(muted, row));
    // The built-in dark theme's "surface", the color a leftover dark literal would draw.
    let dark_surface = slint::Color::from_rgb_u8(0x20, 0x25, 0x2d);
    shell::goto(shell::Screen::Settings);
    s.serve();
    let pages = crate::tests::settings::tabs(&ui);
    for (i, name) in pages.iter().enumerate() {
        if name != "MIDI controllers" && name != "Audio" {
            continue;
        }
        ui.global::<crate::SettingsState>().invoke_show_page(i as i32);
        s.serve();
        s.f.screen.frame(); // the page, drawn
        let n = s.f.screen.pixels.len();
        let dark = s.f.screen.pixels.iter().filter(|p| p.r == dark_surface.red() && p.g == dark_surface.green() && p.b == dark_surface.blue()).count();
        assert!(dark * 2000 < n, "{name}: {dark} px of the dark theme's surface left (a literal), {n} px total");
        // The cursor: a move draws the focus border on a row, and its color is in the frame.
        shell::act(Action::Move { pane: None, by: 1 });
        assert!(s.f.screen.frame().is_some(), "{name}: the move drew the cursor");
        let focus = ui.global::<crate::Theme>().get_focus();
        let lit = s.f.screen.pixels.iter().filter(|p| p.r.abs_diff(focus.red()) <= 3 && p.g.abs_diff(focus.green()) <= 3 && p.b.abs_diff(focus.blue()) <= 3).count();
        assert!(lit > 0, "{name}: no {focus:#?} cursor pixels drawn");
        png(&s.f.screen, &format!("settings-light-{}", name.to_lowercase().replace(' ', "-")));
    }
}

// The Installed page on the keyboard's window, dark: the one list, a package's detail, and Remove's two questions
// for a package something uses that has dependencies nothing else does
// (target/screens/settings-installed-{list,detail,used,unused}.png).
#[test]
fn pictures_of_the_installed_page() {
    fit_to_window();
    let mut s = Shelf::on("sp-installed", Some("fantom-07"), busy_engine());
    let ui = s.ui();
    shell::settings_in_rust(true);
    shell::set_text_on_screen(false);
    s.f.screen.resize(1280, 720);
    s.f.screen.set_scale(1.55);
    theme::apply(&ui, &theme::chosen(&theme::Places::default(), "oscairn-dark", ""));
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Settings);
    s.serve();
    let at = crate::tests::settings::tabs(&ui).iter().position(|p| p == "Installed").unwrap() as i32;
    ui.global::<crate::SettingsState>().invoke_show_page(at);
    s.serve();
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Move { pane: None, by: 1 }); // Example Reverb
    shoot(&mut s, "settings-installed-list");
    shell::act(Action::Confirm(None));
    shoot(&mut s, "settings-installed-detail");
    shell::act(Action::Cancel(None));
    bar_do(&mut s, "Remove");
    s.serve();
    shoot(&mut s, "settings-installed-used");
    assert!(ui.global::<NoticeState>().get_open());
    ui.global::<NoticeState>().invoke_tapped(1); // Remove anyway
    s.serve();
    shoot(&mut s, "settings-installed-unused");
    assert!(ui.global::<NoticeState>().get_open(), "the second question");
}

// The Package repositories and Signing keys pages in the light theme at 1280x720: pictures in
// target/theme-shots, and measurements that the page draws in the light theme (check d). The window's
// corner is the theme's background where nothing is drawn; a dark theme's surface (#1d2127) or
// background (#111317) is gone; and the cursor's focus border is drawn where it sits. The rows draw
// in `Theme.surface` (part.slint:88), which the light theme makes a paper, so a dark surface literal
// left there covers the rows and fails the surface bound below.
#[test]
fn the_repositories_and_keys_pages_in_the_light_theme() {
    fit_to_window();
    let light = theme::chosen(&theme::Places::default(), "oscairn-light", "");
    let bg = light.data.ink("background");
    let mut s = Shelf::on("sp-light", Some("fantom-07"), busy_engine());
    let ui = s.ui();
    shell::settings_in_rust(true);
    s.f.screen.resize(1280, 720);
    s.f.screen.set_scale(1.);
    theme::apply(&ui, &light);
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Settings);
    s.serve();
    let pages = crate::tests::settings::tabs(&ui);
    let show = |s: &mut Shelf, name: &str| {
        s.f.screen.frame();
        s.f.screen.frame();
        let g = ui.global::<crate::Theme>();
        assert_eq!(g.get_id(), "oscairn-light", "{name}: the theme applied");
        assert!(!g.get_dark(), "{name}: a light background");
        let near = |want: [u8; 3], tol: u8| {
            let mut count = 0u64;
            for p in &s.f.screen.pixels {
                if p.r.abs_diff(want[0]) <= tol && p.g.abs_diff(want[1]) <= tol && p.b.abs_diff(want[2]) <= tol {
                    count += 1;
                }
            }
            count as f64 / s.f.screen.pixels.len() as f64
        };
        let corner: [u8; 3] = [s.f.screen.pixels[0].r, s.f.screen.pixels[0].g, s.f.screen.pixels[0].b];
        assert_eq!(corner, [bg.red(), bg.green(), bg.blue()], "{name}: the background where nothing is drawn");
        let surface = near([0x1d, 0x21, 0x27], 2);
        assert!(surface < 0.005, "{name}: a dark theme surface literal covers {:.2}%", surface * 100.0);
        let background = near([0x11, 0x13, 0x17], 2);
        assert!(background < 0.002, "{name}: a dark theme background literal covers {:.2}%", background * 100.0);
        let focus = g.get_focus();
        let ring = s.f.screen.pixels.iter().filter(|p| p.r.abs_diff(focus.red()) <= 12 && p.g.abs_diff(focus.green()) <= 12 && p.b.abs_diff(focus.blue()) <= 12).count();
        assert!(ring > 0, "{name}: the cursor's focus border is drawn {ring}");
        shot(&s.f.screen, name);
    };
    let repos = pages.iter().position(|p| p == "Package repositories").unwrap();
    ui.global::<crate::SettingsState>().invoke_show_page(repos as i32);
    s.serve();
    shell::act(Action::Move { pane: Some(0), by: 1 }); // the cursor on the first row
    show(&mut s, "settings-repos-light");
    let keys = pages.iter().position(|p| p == "Signing keys").unwrap();
    ui.global::<crate::SettingsState>().invoke_show_page(keys as i32);
    s.serve();
    shell::act(Action::Move { pane: Some(0), by: 1 });
    show(&mut s, "settings-keys-light");
}
