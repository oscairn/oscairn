// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app (settings_app.rs, ui/SETTINGS-APP.md) against a fake engine that answers as the real handlers do
//! (settings_fake.rs): the frame and its tabs, Appearance, the Top bar, MIDI controllers and Audio, each by the cursor
//! and by touch, and what each asks the engine.

use super::library::{footer, info, labels, rows, value_of};
use super::settings_fake::{dev, port, Settings as Fake};
use super::*;
use crate::menu;
use crate::profile::Profile;
use crate::settings_app::{Call, Id, Outcome, Screen as Settings};
use crate::{PartState, SettingsState};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

/// A sentence on the panel's lines.
pub fn wrapped(text: &str) -> Vec<String> {
    crate::library::wrap(text, crate::library::INFO_WIDTH)
}

pub fn tabs(ui: &Shell) -> Vec<String> {
    ui.global::<SettingsState>().get_pages().iter().map(|p| p.to_string()).collect()
}

/// A folder of the test's own, empty.
pub fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oscairn-ui-settings-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub struct Rig {
    pub s: super::Screen,
    pub c: Settings,
    pub e: Fake,
    pub config: PathBuf,
    /// What was asked later than at once, with the delay.
    pub delayed: Vec<(String, Duration)>,
    waiting: Vec<crate::settings_app::Send>,
    /// The menu the screen has open, the question and the keyboard's prompt.
    pub menu: Option<(String, Vec<menu::Item>)>,
    pub notice: Option<(String, String, Vec<String>)>,
    pub prompt: Option<(String, String)>,
}

impl Rig {
    /// The Settings app on a profile (a built-in's id; none: a computer's) against `e`.
    pub fn new(name: &str, profile: Option<&str>, e: Fake) -> Rig {
        let s = super::Screen::new();
        tall(&s.ui);
        let profile = Profile::load(profile).unwrap();
        let mut c = Settings::new(&s.ui, &profile);
        let config = folder(name);
        c.set_folders(Some(config.clone()), crate::theme::Places { built_in: None, installed: Some(config.join("themes")) });
        c.open();
        c.shown();
        let mut r = Rig { s, c, e, config, delayed: vec![], waiting: vec![], menu: None, notice: None, prompt: None };
        r.settle();
        r
    }

    pub fn desktop(name: &str) -> Rig {
        Rig::new(name, None, Fake::new())
    }

    fn take(&mut self) {
        // What the shell does from its event loop: the theme chosen and the meters, shown.
        let (theme, meters) = self.c.take_look();
        if let Some(theme) = theme {
            crate::theme::apply(&self.s.ui, &theme);
        }
        if let Some(meters) = meters {
            crate::shell::show_settings(&self.s.ui, meters);
        }
        if let Some(m) = self.c.take_menu() {
            self.menu = Some(m);
        }
        if let Some(n) = self.c.take_notice() {
            self.notice = Some(n);
        }
    }

    fn answer(&mut self, send: crate::settings_app::Send) {
        let Call { step, method, params } = send.call;
        let result = self.e.handle(method, &params);
        self.c.reply(Call { step, method, params }, result);
    }

    /// Answers what the screen asked, and what that made it ask, until it asks no more (a request to be asked later is
    /// noted, not asked).
    pub fn settle(&mut self) {
        self.take();
        for _ in 0..80 {
            let outs = std::mem::take(&mut self.c.out);
            if outs.is_empty() {
                return;
            }
            for send in outs {
                if let Some(after) = send.after {
                    self.delayed.push((send.call.method.to_string(), after));
                    self.waiting.push(send);
                    continue;
                }
                self.answer(send);
            }
            self.take();
        }
        panic!("the screen never stopped asking");
    }

    /// Answers what was asked later, and what that makes it ask, until nothing is waiting.
    pub fn later_all(&mut self) {
        for _ in 0..80 {
            if self.waiting.is_empty() {
                return;
            }
            for send in std::mem::take(&mut self.waiting) {
                self.answer(send);
            }
            self.settle();
        }
        panic!("the screen kept asking later");
    }

    /// The status the engine pushes, handed to the page shown.
    pub fn status(&mut self) {
        let status = self.e.status();
        self.c.status(&status);
        self.settle();
    }

    pub fn ui(&self) -> &Shell {
        &self.s.ui
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let o = self.c.act(action);
        self.settle();
        o
    }

    /// The cursor onto row `label` (the first move shows it on the first row), and a press.
    pub fn press_row(&mut self, label: &str) -> Outcome {
        let target = labels(self.ui()).iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {:?}", labels(self.ui())));
        let now = match rows(self.ui()).iter().position(|x| x.cursor) {
            Some(at) => at as i32,
            None => {
                self.act(Action::Move { pane: None, by: 1 });
                0
            }
        };
        if target as i32 != now {
            self.act(Action::Move { pane: None, by: target as i32 - now });
        }
        self.act(Action::Confirm(None))
    }

    /// The cursor onto row `index` and a press, for rows whose labels repeat.
    pub fn press_index(&mut self, index: usize) -> Outcome {
        let now = match rows(self.ui()).iter().position(|x| x.cursor) {
            Some(at) => at as i32,
            None => {
                self.act(Action::Move { pane: None, by: 1 });
                0
            }
        };
        if index as i32 != now {
            self.act(Action::Move { pane: None, by: index as i32 - now });
        }
        self.act(Action::Confirm(None))
    }

    /// A page shown by its tab.
    pub fn page(&mut self, id: Id) {
        let at = self.c.pages().iter().position(|p| *p == id).unwrap_or_else(|| panic!("{id:?} in {:?}", self.c.pages()));
        self.c.show_page(at);
        self.settle();
    }

    /// The menu open: its title and labels.
    pub fn menu_labels(&self) -> (String, Vec<String>) {
        let (title, items) = self.menu.as_ref().expect("a menu is open");
        (title.clone(), items.iter().map(|i| i.label.clone()).collect())
    }

    /// The menu's labels ticked.
    pub fn ticked(&self) -> Vec<String> {
        self.menu.as_ref().expect("a menu is open").1.iter().filter(|i| i.tick == Some(true)).map(|i| i.label.clone()).collect()
    }

    /// A choice of the menu open, by its label; the menu closes.
    pub fn choose(&mut self, label: &str) {
        let (_, items) = self.menu.take().expect("a menu is open");
        let item = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("{label} in {:?}", items.iter().map(|i| &i.label).collect::<Vec<_>>()));
        let menu::Kind::Choice(id) = item.kind else { panic!("{label} is not a choice") };
        self.c.chosen(id);
        self.settle();
    }

    /// The question's answer by its label.
    pub fn answer_notice(&mut self, label: &str) {
        let (_, _, choices) = self.notice.take().expect("a question is open");
        let at = choices.iter().position(|c| c == label).unwrap_or_else(|| panic!("{label} in {choices:?}"));
        self.c.noticed(Some(at));
        self.settle();
    }

    /// The keyboard's text for the prompt a press made.
    pub fn type_text(&mut self, text: &str) {
        self.c.entered(text);
        self.settle();
    }

    pub fn tool(&mut self, label: &str) -> Outcome {
        let at = self.c.bar_tools().iter().position(|t| t.label.starts_with(label)).unwrap_or_else(|| panic!("{label} in {:?}", self.c.bar_tools()));
        let o = self.c.tool(at);
        if let Outcome::Prompt(t, x) = &o {
            self.prompt = Some((t.clone(), x.clone()));
        }
        self.settle();
        o
    }

    /// What the footer says.
    pub fn said(&self) -> String {
        footer(self.ui()).0
    }

    pub fn danger_lines(&self) -> Vec<String> {
        self.ui().global::<PartState>().get_info().iter().filter(|l| l.failed).map(|l| l.text.to_string()).collect()
    }
}

// ---- S: the frame ----

// The pages the profile has, as tabs in the order Appearance, Top bar, MIDI controllers, Audio, Package repositories,
// Signing keys, About; the keyboard's profile fixes its audio device, so it has no Audio page (S2, S3).
#[test]
fn the_pages_are_the_profiles_tabs() {
    let r = Rig::desktop("tabs-desktop");
    assert_eq!(tabs(r.ui()), ["Appearance", "Top bar", "MIDI controllers", "Audio", "Package repositories", "Installed", "Signing keys", "About"]);
    assert_eq!(r.c.path(), ["Settings", "Appearance"]);
    let r = Rig::new("tabs-fantom", Some("fantom-07"), Fake::null());
    assert_eq!(tabs(r.ui()), ["Appearance", "Top bar", "MIDI controllers", "Package repositories", "Installed", "Signing keys", "About"], "no Audio where the profile fixes the device");
    for id in ["ignite", "imx8mm-evk", "kick", "modx-m", "qemu-aarch64"] {
        let r = Rig::new(&format!("tabs-{id}"), Some(id), Fake::new());
        assert!(tabs(r.ui()).contains(&"Audio".to_string()), "{id} leaves the device to the engine: it has the page");
    }
}

// A tab shows its page, by touch and by the dial: the dial or the left knob walks the tabs from the first row, a press
// shows one, past the last it leaves for the list; the path in the bar follows (S2, S7).
#[test]
fn a_tab_shows_its_page_by_touch_and_by_the_dial() {
    let mut r = Rig::desktop("tabs-walk");
    r.c.tap_page(1);
    r.settle();
    assert_eq!(r.c.path(), ["Settings", "Top bar"]);
    assert_eq!(r.ui().global::<SettingsState>().get_page(), 1);
    assert_eq!(labels(r.ui()), ["CPU", "RAM"]);
    // Up from the first row reaches the tabs, the dial walks them, a press shows one.
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: -1 });
    assert_eq!(r.ui().global::<SettingsState>().get_page_cursor(), 1, "the cursor is on the tab of the page shown");
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Confirm(None));
    assert_eq!(r.c.path(), ["Settings", "MIDI controllers"]);
    // Past the last tab the dial goes back to the list.
    r.act(Action::End(1));
    r.act(Action::Move { pane: None, by: 1 });
    assert_eq!(r.ui().global::<SettingsState>().get_page_cursor(), -1);
    // Cancel with nothing to drop leaves the app.
    r.act(Action::Cancel(None));
    assert_eq!(r.act(Action::Cancel(None)), Outcome::Back);
}

// Each page reads when it is shown and not otherwise; nothing at rest asks, no timer is waiting (S5).
#[test]
fn a_page_asks_when_shown_and_nothing_at_rest() {
    let mut r = Rig::desktop("rest");
    assert!(r.e.calls.is_empty(), "Appearance reads files, not the engine: {:?}", r.e.methods());
    r.page(Id::Audio);
    assert_eq!(r.e.methods(), ["audio.devices"]);
    r.page(Id::Controllers);
    r.page(Id::Repositories);
    r.page(Id::Keys);
    r.page(Id::About);
    assert_eq!(r.e.methods(), ["audio.devices", "controllers.inputs", "packages.repositories", "keys.list", "packages.preinstalled"]);
    assert!(r.delayed.is_empty(), "nothing is asked again: {:?}", r.delayed);
    r.e.calls.clear();
    r.page(Id::Audio);
    // A status that says nothing changed asks nothing.
    r.status();
    r.status();
    assert_eq!(r.e.methods(), ["audio.devices"], "only the page's own opening");
}

// An engine that is busy with a long request refuses what is asked; the screen asks again in a moment, and answers the
// dial meanwhile (S4).
#[test]
fn a_busy_engine_is_asked_again() {
    let mut r = Rig::desktop("busy");
    r.e.busy = 2;
    r.page(Id::Controllers);
    assert_eq!(r.e.methods(), ["controllers.inputs"], "refused as busy");
    assert_eq!(r.delayed, [("controllers.inputs".to_string(), Duration::from_millis(300))]);
    r.act(Action::Move { pane: None, by: 1 }); // the screen is alive
    assert!(rows(r.ui()).iter().any(|x| x.cursor));
    r.later_all();
    assert_eq!(r.e.methods(), ["controllers.inputs", "controllers.inputs", "controllers.inputs"], "refused twice, then answered");
    assert_eq!(labels(r.ui()), ["MIDI inputs"]);
    assert_eq!(value_of(r.ui(), "MIDI inputs"), "No MIDI input is connected");
}

// ---- A: Appearance ----

fn font_of(id: &str, into: &std::path::Path) {
    let from = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hosts/standalone/themes/oscairn-dark");
    let _ = id;
    std::fs::create_dir_all(into.join("fonts")).unwrap();
    for f in ["theme.json", "fonts/Inter-Regular.ttf", "fonts/Inter-Bold.ttf", "fonts/OFL.txt"] {
        std::fs::copy(from.join(f), into.join(f)).unwrap();
    }
}

/// A theme of the player's own, installed under `config`/themes: a copy of the dark one under another id and name.
pub fn installed_theme(config: &std::path::Path, id: &str, name: &str, change: impl FnOnce(&mut Value)) {
    let into = config.join("themes").join(id);
    font_of(id, &into);
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(into.join("theme.json")).unwrap()).unwrap();
    doc["id"] = json!(id);
    doc["name"] = json!(name);
    change(&mut doc);
    std::fs::write(into.join("theme.json"), doc.to_string()).unwrap();
}

// The themes are listed by name, the built-in ones and the installed ones, the one in use marked; a folder not named
// after its id, one that does not parse and an installed one with a built-in's id are left out; the panel says what the
// theme under the cursor sets (A1, A3, A4).
#[test]
fn the_installed_themes_are_listed_with_what_each_sets() {
    let config = folder("themes-list");
    installed_theme(&config, "evening-glass", "Evening Glass", |d| {
        d["spacing"] = json!(1.3);
        d["fontScale"] = json!(1.2);
        d["style"]["tabPlacement"] = json!("side");
    });
    installed_theme(&config, "oscairn-dark", "A Fake Dark", |_| {});
    std::fs::create_dir_all(config.join("themes/broken")).unwrap();
    std::fs::write(config.join("themes/broken/theme.json"), "{").unwrap();
    installed_theme(&config, "misnamed", "Misnamed", |_| {});
    std::fs::rename(config.join("themes/misnamed"), config.join("themes/other-folder")).unwrap();
    let s = super::Screen::new();
    tall(&s.ui);
    let mut c = Settings::new(&s.ui, &Profile::load(None).unwrap());
    c.set_folders(Some(config.clone()), crate::theme::Places { built_in: None, installed: Some(config.join("themes")) });
    c.open();
    c.shown();
    let names = labels(&s.ui);
    assert_eq!(names, ["Oscairn Block", "Oscairn Classic", "Oscairn Dark", "Oscairn Light", "Evening Glass"], "{names:?}");
    let marked: Vec<(String, String)> = rows(&s.ui).into_iter().map(|r| (r.label.to_string(), r.value.to_string())).filter(|r| !r.1.is_empty()).collect();
    assert_eq!(marked, [("Oscairn Dark".to_string(), "In use".to_string())], "none chosen: the built-in default is in use");
    c.act(Action::Move { pane: None, by: 1 });
    c.act(Action::Move { pane: None, by: 4 });
    let said = info(&s.ui);
    assert!(said.contains(&"Evening Glass".to_string()) && said.iter().any(|l| l.contains("Spacing 1.30, font scale 1.20")) && said.iter().any(|l| l.contains("placed side")), "{said:?}");
}

// A theme is chosen with a press: it is shown at once on every screen and kept in appearance.txt, the file the JUCE
// app reads, replaced whole; the choice is the one in use afterwards (A2, A4, A7).
#[test]
fn choosing_a_theme_shows_it_and_keeps_it() {
    let mut r = Rig::desktop("themes-pick");
    installed_theme(&r.config, "evening-glass", "Evening Glass", |d| d["radius"] = json!(14));
    r.c.open();
    r.settle();
    assert_eq!(r.ui().global::<crate::Theme>().get_id().as_str(), "");
    r.press_row("Evening Glass");
    assert_eq!(std::fs::read_to_string(r.config.join("appearance.txt")).unwrap(), "evening-glass");
    assert_eq!(r.ui().global::<crate::Theme>().get_id().as_str(), "evening-glass", "shown at once");
    assert_eq!(r.ui().global::<crate::Theme>().get_radius(), 14.);
    assert_eq!(value_of(r.ui(), "Evening Glass"), "In use");
    assert_eq!(value_of(r.ui(), "Oscairn Dark"), "");
    assert_eq!(r.said(), "Evening Glass is shown.");
    // Another program chose a theme: the page marks it when the shell has shown it.
    std::fs::write(r.config.join("appearance.txt"), "oscairn-light").unwrap();
    r.c.open();
    assert_eq!(value_of(r.ui(), "Oscairn Light"), "In use");
    assert!(!r.config.join("appearance.txt.tmp").exists(), "written through a temporary file that is gone");
    assert!(r.e.calls.is_empty(), "the theme is not the engine's");
}

// Another program chose a theme (the JUCE app, another window) and the shell showed it: the page marks it, at the time the
// shell tells it, with nothing read again (A7).
#[test]
fn a_theme_chosen_elsewhere_is_marked_when_the_shell_shows_it() {
    let mut r = Rig::desktop("themes-follow");
    assert_eq!(value_of(r.ui(), "Oscairn Dark"), "In use");
    let places = crate::theme::Places::default();
    crate::theme::apply(r.ui(), &crate::theme::chosen(&places, "oscairn-light", ""));
    assert_eq!(value_of(r.ui(), "Oscairn Dark"), "In use", "not before it is told");
    r.c.theme_shown();
    assert_eq!((value_of(r.ui(), "Oscairn Light"), value_of(r.ui(), "Oscairn Dark")), ("In use".to_string(), "".to_string()));
}

// A theme that cannot be used is refused with the reason and nothing is written; a choice the program cannot keep is
// said; a chosen theme that fails to load shows the default and the page says why (A2, A5).
#[test]
fn a_theme_that_cannot_be_used_is_said() {
    let mut r = Rig::desktop("themes-refused");
    installed_theme(&r.config, "no-license", "No License", |_| {});
    std::fs::remove_file(r.config.join("themes/no-license/fonts/OFL.txt")).unwrap();
    r.c.open();
    r.settle();
    r.press_row("No License");
    assert_eq!(r.danger_lines(), ["Theme file is missing"]);
    assert!(!r.config.join("appearance.txt").exists(), "nothing is written for a theme that cannot be read");
    assert_eq!(value_of(r.ui(), "Oscairn Dark"), "In use");
    // Chosen elsewhere and broken since: the default is shown, and the page says why.
    std::fs::write(r.config.join("appearance.txt"), "no-license").unwrap();
    r.c.open();
    r.settle();
    assert_eq!(value_of(r.ui(), "Oscairn Dark"), "In use");
    assert!(r.danger_lines().iter().any(|l| l.contains("Theme file is missing")), "{:?}", r.danger_lines());
    // The configuration folder cannot be written.
    let blocked = folder("themes-blocked").join("file");
    std::fs::write(&blocked, "x").unwrap();
    r.c.set_folders(Some(blocked.join("under-a-file")), crate::theme::Places::default());
    r.c.open();
    r.settle();
    r.press_row("Oscairn Light");
    assert_eq!(r.danger_lines(), ["Cannot save appearance file"]);
}

// ---- T: the Top bar ----

// CPU and RAM each offer Bar and percent (the default), Bar, Percent and Hidden, the one kept ticked; a choice
// is stored in top-bar.json as the JUCE page stores it, at once, and the bar shows it at once (T1, T2, T4).
#[test]
fn the_top_bar_choices_are_stored_and_shown() {
    let mut r = Rig::desktop("topbar");
    r.page(Id::TopBar);
    assert_eq!(labels(r.ui()), ["CPU", "RAM"]);
    assert_eq!(value_of(r.ui(), "CPU"), "Bar and percent ›");
    r.press_row("CPU");
    assert_eq!(r.menu_labels(), ("CPU".to_string(), ["Bar and percent", "Bar", "Percent", "Hidden"].map(String::from).to_vec()));
    assert_eq!(r.ticked(), ["Bar and percent"]);
    r.choose("Percent");
    assert_eq!(std::fs::read_to_string(r.config.join("top-bar.json")).unwrap(), "{\"cpu\":\"percent\"}\n");
    let bar = r.ui().global::<Bar>();
    assert!(!bar.get_cpu_bar() && bar.get_cpu_percent(), "the bar shows percent now");
    assert!(bar.get_ram_bar() && bar.get_ram_percent(), "RAM unchosen: both");
    r.press_row("RAM");
    r.choose("Hidden");
    assert_eq!(std::fs::read_to_string(r.config.join("top-bar.json")).unwrap(), "{\"cpu\":\"percent\",\"ram\":\"none\"}\n");
    assert!(!r.ui().global::<Bar>().get_ram_bar() && !r.ui().global::<Bar>().get_ram_percent());
    // Back to Bar and percent leaves the key out, as the JUCE page does.
    r.press_row("RAM");
    r.choose("Bar and percent");
    assert_eq!(std::fs::read_to_string(r.config.join("top-bar.json")).unwrap(), "{\"cpu\":\"percent\"}\n");
    // The page shown again reads what is kept.
    r.page(Id::Appearance);
    r.page(Id::TopBar);
    assert_eq!((value_of(r.ui(), "CPU"), value_of(r.ui(), "RAM")), ("Percent ›".to_string(), "Bar and percent ›".to_string()));
    assert!(r.e.calls.is_empty());
}

// A file an earlier version wrote, with "device" or no value, reads as Bar and percent and still loads.
#[test]
fn an_old_device_value_reads_as_bar_and_percent() {
    let mut r = Rig::desktop("topbar-old");
    std::fs::write(r.config.join("top-bar.json"), "{\"cpu\":\"device\",\"ram\":\"bar\"}\n").unwrap();
    r.page(Id::TopBar);
    assert_eq!((value_of(r.ui(), "CPU"), value_of(r.ui(), "RAM")), ("Bar and percent ›".to_string(), "Bar ›".to_string()));
    r.press_row("CPU");
    assert_eq!(r.ticked(), ["Bar and percent"]);
}

// The explanations are the reference's; a file that cannot be written is said in the danger color (T3).
#[test]
fn the_top_bar_says_why_it_was_not_saved() {
    let mut r = Rig::desktop("topbar-fail");
    r.page(Id::TopBar);
    assert!(info(r.ui()).iter().any(|l| l.starts_with("How full the CPU is")), "{:?}", info(r.ui()));
    let file = folder("topbar-blocked").join("file");
    std::fs::write(&file, "x").unwrap();
    r.c.set_folders(Some(file.join("under")), crate::theme::Places::default());
    r.press_row("CPU");
    r.choose("Bar");
    let danger = r.danger_lines();
    assert!(danger.join(" ").starts_with("Not saved: ") && danger.join(" ").contains("cannot be written"), "{danger:?}");
}

// ---- C: MIDI controllers ----

fn controllers() -> Fake {
    let mut e = Fake::new();
    e.ports = vec![port("Pad Controller", &[("pads-a", "Pad Box"), ("pads-b", "Pad Box Mk 2")]), port("Wooden Keys", &[]), {
        let mut p = port("Mixer Surface", &[("mixer", "Mixer Surface")]);
        p.kind = "mackie".into();
        p
    }];
    e
}

// Each MIDI input is a row with its profile and an Ignore switch under it; what each is said on the panel in the
// reference's words (C1, C2, C4).
#[test]
fn each_port_has_its_profile_and_an_ignore_switch() {
    let mut r = Rig::new("ctl", None, controllers());
    r.page(Id::Controllers);
    assert_eq!(labels(r.ui()), ["Pad Controller", "Ignore this port", "Wooden Keys", "Ignore this port", "Mixer Surface", "Ignore this port"]);
    assert_eq!(value_of(r.ui(), "Pad Controller"), "Automatic (Pad Box) ›");
    assert_eq!(value_of(r.ui(), "Wooden Keys"), "Automatic ›");
    assert_eq!(rows(r.ui())[3].value, "Off");
    r.act(Action::Move { pane: None, by: 1 });
    assert_eq!(info(r.ui()), [vec!["Pad Controller".to_string()], wrapped("Played by Pad Box: its controls join.")].concat());
    r.act(Action::Move { pane: None, by: 2 });
    assert_eq!(info(r.ui()), [vec!["Wooden Keys".to_string()], wrapped("No controller profile fits it: it plays notes as any MIDI input does.")].concat());
    r.act(Action::Move { pane: None, by: 2 });
    assert_eq!(info(r.ui()), [vec!["Mixer Surface".to_string()], wrapped("Read as Mackie Control by Mixer Surface.")].concat());
    assert_eq!(r.e.methods(), ["controllers.inputs"]);
}

// A profile is chosen from Automatic, None, each that fits and Each of these where several do; each is one request, kept
// by the engine and shown as it answers (C2).
#[test]
fn a_ports_profile_is_chosen_from_a_list() {
    let mut r = Rig::new("ctl-choose", None, controllers());
    r.page(Id::Controllers);
    r.e.calls.clear();
    r.press_row("Pad Controller");
    assert_eq!(r.menu_labels(), ("Profile for Pad Controller".to_string(), ["Automatic (Pad Box)", "None", "Pad Box", "Pad Box Mk 2", "Each of these, by MIDI channel"].map(String::from).to_vec()));
    assert_eq!(r.ticked(), ["Automatic (Pad Box)"]);
    r.choose("Pad Box Mk 2");
    assert_eq!(r.e.called("controllers.choose"), [json!({"input": "Pad Controller", "profiles": ["pads-b"]})]);
    assert_eq!(value_of(r.ui(), "Pad Controller"), "Pad Box Mk 2 ›");
    r.press_row("Pad Controller");
    assert_eq!(r.ticked(), ["Pad Box Mk 2"]);
    r.choose("Each of these, by MIDI channel");
    assert_eq!(r.e.called("controllers.choose").last().unwrap(), &json!({"input": "Pad Controller", "profiles": ["pads-a", "pads-b"]}));
    assert_eq!(value_of(r.ui(), "Pad Controller"), "Each of these, by MIDI channel ›");
    assert_eq!(info(r.ui()).join(" "), "Pad Controller Played by Pad Box and the others chosen, by MIDI channel: its controls join.");
    r.press_row("Pad Controller");
    r.choose("None");
    assert_eq!(r.e.called("controllers.choose").last().unwrap(), &json!({"input": "Pad Controller", "profiles": []}));
    assert_eq!(value_of(r.ui(), "Pad Controller"), "None ›");
    r.press_row("Pad Controller");
    r.choose("Automatic (Pad Box)");
    assert_eq!(r.e.called("controllers.choose").last().unwrap(), &json!({"input": "Pad Controller", "automatic": true}));
    assert_eq!(value_of(r.ui(), "Pad Controller"), "Automatic (Pad Box) ›");
}

// Ignore turns a port off at once; its profile row is faint and does nothing while it is ignored (C3).
#[test]
fn ignoring_a_port_leaves_out_what_it_sends() {
    let mut r = Rig::new("ctl-ignore", None, controllers());
    r.page(Id::Controllers);
    r.e.calls.clear();
    r.press_index(3);
    assert_eq!(r.e.called("controllers.ignore"), [json!({"input": "Wooden Keys", "ignored": true})]);
    assert_eq!(rows(r.ui())[3].value, "On");
    assert!(rows(r.ui())[2].off, "the profile row is faint");
    r.act(Action::Move { pane: None, by: -1 });
    assert_eq!(info(r.ui())[1..].join(" "), "Ignored: everything it sends is left out.");
    r.press_row("Wooden Keys");
    assert!(r.menu.is_none(), "no list for an ignored port");
    r.press_index(3);
    assert_eq!(r.e.called("controllers.ignore").last().unwrap(), &json!({"input": "Wooden Keys", "ignored": false}));
    assert_eq!(rows(r.ui())[3].value, "Off");
}

// A refusal is said under its port, in the danger color, and the ports are listed again as they are (C5, C7).
#[test]
fn a_refused_choice_is_said_under_its_port() {
    let mut r = Rig::new("ctl-refused", None, controllers());
    r.page(Id::Controllers);
    r.e.fail = vec![("controllers.choose".into(), "The profile cannot play this port".into())];
    r.press_row("Pad Controller");
    r.choose("None");
    assert_eq!(r.e.methods().last().unwrap(), "controllers.inputs", "listed again");
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: -1 });
    assert_eq!(value_of(r.ui(), "Pad Controller"), "Automatic (Pad Box) ›", "as the engine has it");
    assert_eq!(r.danger_lines(), ["The profile cannot play this port"]);
    r.e.fail = vec![("controllers.inputs".into(), "no controller service".into())];
    r.page(Id::Appearance);
    r.page(Id::Controllers);
    assert!(r.danger_lines().contains(&"no controller service".to_string()), "{:?}", r.danger_lines());
    assert!(info(r.ui()).contains(&"The inputs could not be read".to_string()));
}

// The page lists again when the status says the ports changed, and not for a status that does not (C6).
#[test]
fn the_ports_are_listed_again_when_the_status_says_so() {
    let mut r = Rig::new("ctl-status", None, controllers());
    r.page(Id::Controllers);
    r.status(); // the first status of the page: what it has
    r.e.calls.clear();
    r.status();
    assert!(r.e.calls.is_empty());
    r.e.ports.push(port("New Surface", &[]));
    r.e.midi_list += 1;
    r.status();
    assert_eq!(r.e.methods(), ["controllers.inputs"]);
    assert!(labels(r.ui()).contains(&"New Surface".to_string()));
    r.status();
    assert_eq!(r.e.methods(), ["controllers.inputs"], "once");
    // One of the page's own changes makes the engine say the list changed: listed once more, as the reference does.
    r.press_index(7);
    let after = r.e.methods().len();
    r.status();
    assert_eq!(r.e.methods().len(), after + 1);
}

// ---- D: Audio ----

// The devices are listed when the page opens: output, input where there are inputs, rate and buffer size of the open
// output ("128 samples (2.7 ms)"), and what the device is (D1, D6).
#[test]
fn the_audio_devices_are_listed() {
    let mut r = Rig::desktop("audio");
    r.page(Id::Audio);
    assert_eq!(r.e.methods(), ["audio.devices"]);
    assert_eq!(labels(r.ui()), ["Output", "Input", "Sample rate", "Buffer size"]);
    assert_eq!(value_of(r.ui(), "Output"), "Built-in Output ›");
    assert_eq!(value_of(r.ui(), "Input"), "Built-in Microphone ›");
    assert_eq!(value_of(r.ui(), "Sample rate"), "48000 Hz ›");
    assert_eq!(value_of(r.ui(), "Buffer size"), "128 samples (2.7 ms) ›");
    assert_eq!(info(r.ui()), [vec!["Output".to_string()], wrapped("The system's default device.")].concat());
    r.e.audio.source = "user".into();
    r.page(Id::Appearance);
    r.page(Id::Audio);
    assert_eq!(info(r.ui()), [vec!["Output".to_string()], wrapped("Chosen here; it opens again at the next start.")].concat());
    r.e.audio.source = "profile".into();
    r.e.audio.notice = "Studio Interface was not there: the default opened".into();
    r.page(Id::Appearance);
    r.page(Id::Audio);
    assert_eq!(r.danger_lines(), wrapped("Studio Interface was not there: the default opened"), "a choice passed over is said first, in the danger color");
    assert!(info(r.ui()).join(" ").contains("Set when the engine started"));
    // No input row where the engine lists no inputs.
    r.e.audio.inputs.clear();
    r.page(Id::Appearance);
    r.page(Id::Audio);
    assert_eq!(labels(r.ui()), ["Output", "Sample rate", "Buffer size"]);
}

// A choice is one `audio.set` of that choice alone: the output, the input, the rate (an integer) and the buffer size;
// the answer is what is shown, "Opening it..." while it is out (D2 to D5, D7).
#[test]
fn each_audio_choice_is_one_request() {
    let mut r = Rig::desktop("audio-set");
    r.page(Id::Audio);
    r.e.calls.clear();
    r.press_row("Buffer size");
    assert_eq!(r.menu_labels().1, ["64 samples (1.3 ms)", "128 samples (2.7 ms)", "256 samples (5.3 ms)", "512 samples (10.7 ms)"]);
    assert_eq!(r.ticked(), ["128 samples (2.7 ms)"]);
    r.choose("256 samples (5.3 ms)");
    assert_eq!(r.e.called("audio.set"), [json!({"bufferSize": 256})]);
    assert_eq!(value_of(r.ui(), "Buffer size"), "256 samples (5.3 ms) ›");
    r.press_row("Sample rate");
    assert_eq!(r.menu_labels().1, ["44100 Hz", "48000 Hz", "96000 Hz"]);
    r.choose("96000 Hz");
    assert_eq!(r.e.called("audio.set").last().unwrap(), &json!({"sampleRate": 96000}));
    r.press_row("Output");
    r.choose("Studio Interface");
    assert_eq!(r.e.called("audio.set").last().unwrap(), &json!({"output": "Studio Interface"}));
    assert_eq!(value_of(r.ui(), "Output"), "Studio Interface ›");
    // The rates and sizes are those of the output now open.
    r.press_row("Buffer size");
    assert_eq!(r.menu_labels().1, ["32 samples (0.7 ms)", "64 samples (1.3 ms)", "128 samples (2.7 ms)"].map(String::from).to_vec());
    r.menu = None;
    r.press_row("Input");
    r.choose("Studio Interface");
    assert_eq!(r.e.called("audio.set").last().unwrap(), &json!({"input": "Studio Interface"}));
    // The one already open is no change.
    let n = r.e.called("audio.set").len();
    r.press_row("Input");
    r.choose("Studio Interface");
    assert_eq!(r.e.called("audio.set").len(), n);
}

// While a change is out every choice is faint and a second is ignored; the line says what is being done (D7).
#[test]
fn while_a_change_is_out_the_page_waits() {
    let mut r = Rig::desktop("audio-wait");
    r.page(Id::Audio);
    r.press_row("Output");
    let (_, items) = r.menu.take().unwrap();
    let menu::Kind::Choice(id) = items[1].kind else { panic!() };
    r.e.busy = 1; // the engine takes its time: asked again
    r.c.chosen(id);
    assert_eq!(r.said(), "Opening it...");
    assert!(rows(r.ui()).iter().all(|x| x.off), "every row is faint");
    r.press_row("Buffer size");
    assert!(r.menu.is_none(), "no list while a change is out");
    r.settle();
    r.later_all();
    assert_eq!(r.said(), "");
    assert_eq!(r.e.called("audio.set").len(), 2, "refused as busy, then asked again, and once");
    r.press_row("Sample rate");
    r.choose("96000 Hz");
    assert_eq!(r.e.called("audio.set").last().unwrap(), &json!({"sampleRate": 96000}));
}

// A rate the engine applies after its answer (the real engine's answer still has the old one) is corrected by the status that
// follows, which makes the page list again, once; the devices coming or going, and the device changing elsewhere, do the same
// (D4, D9).
#[test]
fn a_new_rate_is_shown_when_the_status_says_it_changed() {
    let mut r = Rig::desktop("audio-rate");
    r.page(Id::Audio);
    r.status();
    r.press_row("Sample rate");
    r.choose("96000 Hz");
    assert_eq!(value_of(r.ui(), "Sample rate"), "48000 Hz ›", "the answer has the old rate, as the real engine's does");
    r.e.calls.clear();
    r.status();
    assert_eq!(r.e.methods(), ["audio.devices"]);
    assert_eq!(value_of(r.ui(), "Sample rate"), "96000 Hz ›");
    r.status();
    assert_eq!(r.e.methods(), ["audio.devices"], "once");
    r.e.audio.rate = 44100; // the device went to another rate by itself
    r.status();
    assert_eq!(r.e.methods(), ["audio.devices", "audio.devices"]);
    assert_eq!(value_of(r.ui(), "Sample rate"), "44100 Hz ›");
    // The devices came or went elsewhere.
    r.e.audio.outputs.push(dev("Headphone Amp", &[48000], &[128]));
    r.e.list += 1;
    r.status();
    r.press_row("Output");
    assert!(r.menu_labels().1.contains(&"Headphone Amp".to_string()));
}

// A device that does not open is refused with the engine's reason under what was changed, and the devices are listed
// again as they are: the last device plays on (D8, D11).
#[test]
fn a_device_that_does_not_open_leaves_the_last_one() {
    let mut r = Rig::desktop("audio-refused");
    r.page(Id::Audio);
    r.e.audio.cannot_open = vec![("Studio Interface".into(), "Studio Interface is in use by another program".into())];
    r.press_row("Output");
    r.choose("Studio Interface");
    assert_eq!(r.e.methods().last().unwrap(), "audio.devices", "listed again");
    assert_eq!(value_of(r.ui(), "Output"), "Built-in Output ›", "the last one plays");
    assert_eq!(r.danger_lines(), wrapped("Studio Interface is in use by another program"));
    assert_eq!(r.said(), "");
    // Chosen again once it can be opened: the refusal is gone.
    r.e.audio.cannot_open.clear();
    r.press_row("Output");
    r.choose("Studio Interface");
    assert!(r.danger_lines().is_empty());
}

// A device the profile or the engine fixes cannot be changed, and says so; the rate and buffer size still can (D2, D6, D10, D11).
#[test]
fn a_fixed_device_is_shown_so_and_cannot_be_changed() {
    let mut r = Rig::new("audio-null", None, Fake::null());
    r.page(Id::Audio);
    assert!(rows(r.ui())[0].off, "the output is faint");
    r.press_row("Output");
    assert!(r.menu.is_none());
    assert_eq!(info(r.ui()), [vec!["Output".to_string()], wrapped("No audio device: started with --audio null.")].concat());
    r.press_row("Buffer size");
    assert_eq!(r.menu_labels().1.len(), 4);
    r.choose("256 samples (5.3 ms)");
    assert_eq!(r.e.called("audio.set"), [json!({"bufferSize": 256})]);
    r.e.audio.source = "profile".into();
    r.e.audio.fixed = true;
    r.page(Id::Appearance);
    r.page(Id::Audio);
    assert_eq!(info(r.ui()), [vec!["Output".to_string()], wrapped("Fixed by the device profile: its channels are wired to this device.")].concat());
    // The listing fails.
    r.e.fail = vec![("audio.devices".into(), "no audio engine".into())];
    r.page(Id::Appearance);
    r.page(Id::Audio);
    assert_eq!(r.danger_lines(), wrapped("The devices could not be read: no audio engine"));
    assert_eq!(value_of(r.ui(), "Output"), "null ›", "what was read before stays");
}
