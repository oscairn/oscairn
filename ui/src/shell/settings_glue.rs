// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shell's glue for the Settings app (settings_app.rs): the requests its pages ask sent (a step the engine refused as
//! busy is sent again after its delay), the menu and the question a page has ready opened over it, and the question's
//! answer.

use super::*;

/// Whether Home's Settings tile opens the Rust app: it does (Settings is ported whole). `OSCAIRN_UI_SETTINGS=0` hands the tile over to the JUCE
/// app again (a fault found on the device); `OSCAIRN_UI_SETTINGS=1` opens the Rust app whatever `PORTED` says.
const PORTED: bool = true;

pub(super) fn in_rust() -> bool {
    match std::env::var_os("OSCAIRN_UI_SETTINGS") {
        Some(v) if v == "0" => false,
        Some(v) if !v.is_empty() => true,
        _ => PORTED,
    }
}

/// The theme the user's choice names was shown by the watcher of `appearance.txt` (another program chose it): the
/// Appearance page marks it.
pub fn theme_followed() {
    with_app(|a| {
        if a.screen == Screen::Settings {
            a.on_settings(|s| s.theme_shown());
        }
    })
}

impl App {
    /// Something for the Settings app, then what it asks the engine sent, and the menu or question it has ready opened.
    pub(super) fn on_settings<R>(&mut self, f: impl FnOnce(&mut settings_app::Screen) -> R) -> R {
        let r = f(&mut self.settings);
        for send in std::mem::take(&mut self.settings.out) {
            let call = send.call;
            let (method, params) = (call.method, call.params.clone());
            let request = Request::SettingsApp(call);
            match send.after {
                None => self.send(request, method, params),
                Some(after) => {
                    slint::Timer::single_shot(after, move || with_app(|a| a.send(request, method, params)));
                }
            }
        }
        // A theme chosen or the bar's meters changed are shown from the event loop: putting a theme in the window runs the
        // handlers of the screens, which this cannot be inside of.
        let (theme, meters) = self.settings.take_look();
        if theme.is_some() || meters.is_some() {
            let weak = self.ui.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                let Some(ui) = weak.upgrade() else { return };
                if let Some(theme) = &theme {
                    crate::theme::apply(&ui, theme);
                }
                if let Some(meters) = meters {
                    show_settings(&ui, meters);
                }
            });
        }
        if let Some((title, items)) = self.settings.take_menu() {
            self.open_menu(Screen::Settings, &title, items);
        }
        if let Some((title, message, choices)) = self.settings.take_notice() {
            self.open_notice(Screen::Settings, &title, &message, &choices.iter().map(String::as_str).collect::<Vec<_>>());
        }
        if self.screen == Screen::Settings {
            self.show_bar();
        }
        r
    }

    /// A navigation action or tap for the Settings app, and where it takes the shell.
    pub(super) fn settings_act(&mut self, f: impl FnOnce(&mut settings_app::Screen) -> settings_app::Outcome) {
        match self.on_settings(f) {
            settings_app::Outcome::Back => self.go(Screen::Home),
            settings_app::Outcome::Prompt(title, text) => self.open_keyboard(Screen::Settings, &title, &text),
            settings_app::Outcome::Stay => {}
        }
    }
}

/// Tests: whether Home's Settings tile opens the Rust app (it does not, until the whole app is built).
#[cfg(test)]
pub fn settings_in_rust(on: bool) {
    with_app(|a| a.settings_in_rust = on)
}

/// Tests: the folders the Settings app keeps the theme and the top bar's choice in, and the themes it lists.
#[cfg(test)]
pub fn settings_folders(config: Option<PathBuf>, places: crate::theme::Places) {
    with_app(|a| a.settings.set_folders(config, places))
}

/// Tests: the folder the Appearance page's theme chooser starts in.
#[cfg(test)]
pub fn settings_files_dir(dir: PathBuf) {
    with_app(|a| a.settings.set_files_dir(dir))
}
