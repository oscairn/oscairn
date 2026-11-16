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
