// SPDX-License-Identifier: AGPL-3.0-or-later
//! The newer screens — the Library (as lines and as bubbles), the Setlist (a grid and a list), and the Settings
//! Installed page with a question over it — in the light theme, the check tests.rs's
//! `light_theme_renders_the_stage_screens` adds for the stage ones: at 1280x720 the theme is applied and light, the
//! corner where nothing is drawn is the light background, no element is left in the dark theme's surface color (a
//! hard-coded dark surface would stay), the body text is drawn in the theme's text ink (which reads 4.5 to 1 on the
//! light background), a Setlist's color slot draws its ink at 3 to 1 on its box (the large names), the cursor's focus
//! border appears where it is shown, and the screen is mostly the light theme. Pictures:
//! target/theme-shots/light-{screen}.png.

use super::*;
use crate::{NoticeState, SettingsState, SetlistState, SlotData};

/// The Setlist shown on a fake engine, `mode` grid or list, with a light, a blue, a navy, a red and a green slot
/// among plain ones (setlist_bar's COLORS) in the first bank, the connection quiet.
fn setlist(mode: &str, name: &str) -> Fixture {
    const COLORS: [&str; 12] = ["", "#ffe082", "#3366cc", "", "", "#1a237e", "#e5484d", "", "", "", "#2e7d32", ""];
    let mut f = Fixture::new(name);
    f.engine.answer(); // session.hello
    f.pump();
    f.screen.ui.global::<HomeState>().invoke_tapped(2); // Setlist
    let setlists: Vec<_> = (0..2).map(|i| json!({"id": format!("{name}-{}", i + 1), "name": format!("Night {}", i + 1)})).collect();
    let performances: Vec<_> = (0..40).map(|i| json!({"id": format!("p{i}"), "name": format!("Song {i}"), "color": "", "path": format!("/lib/p{i}")})).collect();
    let slots: Vec<_> = (0..12)
        .map(|i| json!({"id": if i == 8 { String::new() } else { format!("p{i}") }, "name": format!("Song {i}"), "color": COLORS[i % 12], "label": ""}))
        .collect();
    let setlist = json!({"format": "oscairn.setlist", "version": 2, "info": {"id": format!("{name}-1"), "name": "Night 1"}, "display": {"mode": mode, "columns": 4, "rows": 3}, "pages": [{"name": "Set 1", "slots": slots}]});
    f.serve(13, &json!({"setlists": setlists, "performances": performances, "setlist": setlist}));
    f
}

/// A bar action by its label, the settings page's action not folded ("More") at 1280 wide.
fn settings_bar_do(s: &mut super::settings_shell::Shelf, label: &str) {
    let ui = s.ui();
    let labels: Vec<String> = ui.global::<crate::Bar>().get_actions().iter().map(|a| a.label.to_string()).collect();
    let at = labels.iter().position(|l| l.starts_with(label)).unwrap_or_else(|| panic!("{label} in {labels:?}"));
    ui.global::<crate::Bar>().invoke_action(at as i32);
}

// The screens in the light theme at 1280x720: each shares tests.rs's stage screen check, and the new screens have
// their own: a Setlist's color slot's ink reads 3 to 1 on its box (the large names), and the cursor's focus border
// appears where it is shown (the stage test did not cover the later screens' cursors).
#[test]
fn light_theme_renders_the_later_screens() {
    // The light theme and the dark one it is checked against.
    let light = theme::chosen(&theme::Places::default(), "oscairn-light", "");
    let dark = theme::chosen(&theme::Places::default(), "oscairn-dark", "");
    let bg = light.data.ink("background");
    let text = light.data.ink("text");
    let accent = light.data.ink("accent"); // the light theme has no focus: the cursor is the accent
    let dark_surface = dark.data.ink("surface");
    assert!(crate::palette::contrast(text, bg) >= 4.5, "the light theme's body text reads 4.5 to 1 on its background");
    assert!(crate::palette::contrast(text, light.data.ink("surface")) >= 4.5, "the light theme's body text reads 4.5 to 1 on its surface");
    // The checks every screen shares: the theme applied and light, the corner the light background, nothing left in
    // the dark theme's surface, the theme's text ink drawn (a screen that drew its words in another ink would have
    // none), and mostly the light theme. `dark_share` is the most of the screen allowed below a low luminance (the
    // Setlist's colored slots are more than the stage screens').
    let check = |s: &Screen, name: &str, dark_share: f64| {
        let g = s.ui.global::<crate::Theme>();
        assert_eq!(g.get_id(), "oscairn-light", "{name}: the theme applied");
        assert!(!g.get_dark(), "{name}: a light background");
        let corner = [s.pixels[0].r, s.pixels[0].g, s.pixels[0].b];
        assert_eq!(corner, [bg.red(), bg.green(), bg.blue()], "{name}: the background where nothing is drawn");
        let left = s.pixels.iter().filter(|p| p.r.abs_diff(dark_surface.red()) <= 2 && p.g.abs_diff(dark_surface.green()) <= 2 && p.b.abs_diff(dark_surface.blue()) <= 2).count() as f64 / s.pixels.len() as f64;
        // A broad fill left in the dark surface (a whole row or a panel: hundreds of pixels) is the fault; a few
        // pixels at that value are text antialiasing (an ink coverage of about 93% passes through it) and belong.
        assert!(left < 1e-3, "{name}: {:.2}% of the screen is the dark theme's surface", left * 100.);
        let ink = s.pixels.iter().filter(|p| p.r.abs_diff(text.red()) <= 2 && p.g.abs_diff(text.green()) <= 2 && p.b.abs_diff(text.blue()) <= 2).count() as f64 / s.pixels.len() as f64;
        assert!(ink > 2e-5, "{name}: the theme's text ink is drawn ({:.3}%)", ink * 100.);
        let dark = s.pixels.iter().filter(|p| 0.2126 * f64::from(p.r) + 0.7152 * f64::from(p.g) + 0.0722 * f64::from(p.b) < 80.0).count() as f64 / s.pixels.len() as f64;
        assert!(dark < dark_share, "{name}: {:.1}% of the screen is dark (bound {:.1}%)", dark * 100., dark_share * 100.);
        shot(s, name);
    };
    // The cursor's focus border appears where it is shown: the border the first move draws is the accent.
    let focus = |s: &Screen| s.pixels.iter().filter(|p| p.r.abs_diff(accent.red()) <= 2 && p.g.abs_diff(accent.green()) <= 2 && p.b.abs_diff(accent.blue()) <= 2).count();

    // The Setlist, a grid and a list: one light slot (its ink black), the others' ink 3 to 1 on their boxes, the
    // cursor on a slot.
    for mode in ["grid", "list"] {
        let mut f = setlist(mode, &format!("light-setlist-{mode}"));
        let before = focus(&f.screen);
        shell::act(Action::Move { pane: Some(1), by: 1 }); // the cursor on the first slot
        f.screen.frame();
        let slots: Vec<SlotData> = f.screen.ui.global::<SetlistState>().get_slots().iter().collect();
        for (i, slot) in slots.iter().enumerate() {
            if slot.tinted {
                let ratio = crate::palette::contrast(slot.tint, slot.ink);
                assert!(ratio >= 3., "{mode} slot {i}: its ink reads {ratio:.1} to 1 on {} (the large names)", slot.tint);
                assert_eq!(slot.ink == slint::Color::from_rgb_u8(0, 0, 0), slots[1].tinted && i == 1, "{mode} slot {i}: the light slot's ink is black");
            }
        }
        theme::apply(&f.screen.ui, &light);
        f.screen.frame();
        check(&f.screen, &format!("light-setlist-{mode}"), 0.10);
        assert!(focus(&f.screen) > before + 800, "{mode}: the cursor's focus border appears on the slot ({} px, before {before})", focus(&f.screen));
    }

    // The Library, as lines and as bubbles: the cursor on a row.
    shell::library_in_rust(true);
    shell::set_text_on_screen(false);
    let mut s = super::library_shell::Shelf::new("light-library");
    let ui = s.ui();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    s.serve();
    theme::apply(&ui, &light);
    let before = focus(&s.f.screen);
    shell::act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor on the first row
    s.f.screen.frame();
    check(&s.f.screen, "light-library-list", 0.05);
    assert!(focus(&s.f.screen) > before + 800, "the list cursor's border appears ({} px, before {before})", focus(&s.f.screen));
    super::library_bubbles::press_tool(&mut s, 2); // the row under the bar's Bubbles button: the sidebar as pills
    s.f.screen.frame();
    check(&s.f.screen, "light-library-bubbles", 0.05);

    // Settings' Installed page, and a question over it: the cursor on a row, then Remove's question.
    let mut e = super::settings_fake::Settings::new();
    let package = |id: &str, name: &str, version: &str, kind: &str| json!({"id": id, "version": version, "kind": kind, "repository": "https://packages.oscairn.org", "sha256": "", "signers": ["6a2dd1acf4d4e23c"], "otherBuild": false, "withdrawn": null, "dependency": false, "name": name});
    e.installed_packages = vec![
        package("org.example.reverb", "Example Reverb", "1.0.0", "plugin"),
        package("org.example.strings", "example strings", "2.1.0", "asset"),
    ];
    let mut s = super::settings_shell::Shelf::with("light-settings", e);
    shell::settings_in_rust(true);
    let ui = s.ui();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Settings);
    s.serve();
    theme::apply(&ui, &light);
    let installed = super::settings::tabs(&ui).iter().position(|t| t == "Installed").expect("the Installed page") as i32;
    ui.global::<SettingsState>().invoke_show_page(installed);
    s.serve();
    let before = focus(&s.f.screen);
    shell::act(Action::Move { pane: None, by: 1 }); // the first move shows the cursor on the first row
    s.f.screen.frame();
    check(&s.f.screen, "light-settings-installed", 0.05);
    assert!(focus(&s.f.screen) > before + 800, "the Installed cursor's border appears ({} px, before {before})", focus(&s.f.screen));
    settings_bar_do(&mut s, "Remove");
    s.serve();
    assert!(ui.global::<NoticeState>().get_open(), "Remove's question is over the screen");
    s.f.screen.frame();
    check(&s.f.screen, "light-settings-question", 0.05);
}
