// SPDX-License-Identifier: AGPL-3.0-or-later
//! The user's choices the shell reads, in the JUCE app's configuration folder
//! (hosts/standalone/Theme.cpp): `appearance.txt`, the theme chosen, and `top-bar.json`, what the
//! top bar's resource section shows, which the JUCE Settings app's Top bar page writes. Read at
//! start: the shell starts again after every JUCE screen (docs/rust-ui.md), Settings included.

use crate::topbar::{MeterSettings, Show};
use serde_json::Value;
use std::path::PathBuf;

/// OSCAIRN_CONFIG_HOME, else the platform's place for Oscairn's settings, as Theme.cpp has it.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OSCAIRN_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Some(dir.into());
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        return Some(home.join("Library/Application Support/Oscairn"));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or(home.join(".config"));
    Some(base.join("Oscairn"))
}

/// OSCAIRN_DATA_HOME, else the platform's place for Oscairn's data, as the engine has it
/// (core/control/src/library.cpp): where the apps keep their state (AppData/<app id>).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OSCAIRN_DATA_HOME").filter(|d| !d.is_empty()) {
        return Some(dir.into());
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        return Some(home.join("Library/Application Support/Oscairn"));
    }
    let base = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or(home.join(".local/share"));
    Some(base.join("oscairn"))
}

fn read(name: &str) -> Option<String> {
    std::fs::read_to_string(config_dir()?.join(name)).ok()
}

/// The theme the user chose, by its id; empty: none chosen.
pub fn chosen_theme() -> String {
    read("appearance.txt").unwrap_or_default().trim().to_string()
}

/// Whether the Play screen's "No MIDI device" hint was dismissed: the line "hint dismissed" of
/// keyboard.txt, which the JUCE app's on-screen keyboards keep (KeyboardWindow.cpp, Keyboards).
pub fn keyboard_hint_dismissed() -> bool {
    read("keyboard.txt").is_some_and(|t| t.lines().any(|l| l.trim() == "hint dismissed"))
}

/// Keeps the hint dismissed: the line goes first, as the JUCE app writes it, and the windows' places
/// after it stay.
pub fn dismiss_keyboard_hint() -> std::io::Result<()> {
    let Some(dir) = config_dir() else { return Ok(()) };
    let rest = read("keyboard.txt").unwrap_or_default();
    if rest.lines().any(|l| l.trim() == "hint dismissed") {
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("keyboard.txt"), format!("hint dismissed\n{rest}"))
}

/// How the sound chooser lists: library-view.txt, which the JUCE app's library pane keeps beside the theme choice
/// (LibraryPane.cpp, `viewPreference`): the words "compact" or "comfortable" and "pills" or "list". The second word is
/// kept as it is read.
pub fn library_view() -> (bool, bool) {
    let text = read("library-view.txt").unwrap_or_default();
    let words: Vec<&str> = text.split_whitespace().collect();
    (words.contains(&"compact"), words.contains(&"pills"))
}

/// Keeps the compact choice, with the pills word as it was.
pub fn keep_library_view(compact: bool) -> std::io::Result<()> {
    let Some(dir) = config_dir() else { return Ok(()) };
    let (_, pills) = library_view();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("library-view.txt"), format!("{}{}", if compact { "compact" } else { "comfortable" }, if pills { " pills" } else { " list" }))
}

/// Whether Routing shows parts collapsed: the line of routing-view.txt, which the JUCE app's Routing
/// page keeps beside the theme choice (PartsView.cpp, `routingPreference`); collapsed unless it says "expanded".
pub fn routing_collapsed() -> bool {
    read("routing-view.txt").is_none_or(|t| t.trim() != "expanded")
}

/// Keeps the choice of Routing's "Parts collapsed" as the JUCE app writes it.
pub fn keep_routing_collapsed(collapsed: bool) {
    let Some(dir) = config_dir() else { return };
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("routing-view.txt"), if collapsed { "collapsed\n" } else { "expanded\n" });
    }
}

/// The profile's meter settings with the user's over them, each of `cpu` and `ram` on its own.
pub fn top_bar(profile: MeterSettings) -> MeterSettings {
    let Some(doc) = read("top-bar.json").and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return profile };
    over(profile, &doc)
}

pub fn over(base: MeterSettings, doc: &Value) -> MeterSettings {
    let pick = |key: &str, was: Show| doc[key].as_str().and_then(Show::parse).unwrap_or(was);
    MeterSettings { cpu: pick("cpu", base.cpu), ram: pick("ram", base.ram) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_users_choice_goes_over_the_profiles_one_meter_at_a_time() {
        let profile = MeterSettings { cpu: Show::Both, ram: Show::Bar };
        let doc: Value = serde_json::from_str(r#"{"cpu":"percent","ram":"sideways"}"#).unwrap();
        assert_eq!(over(profile, &doc), MeterSettings { cpu: Show::Percent, ram: Show::Bar }); // an odd value leaves the profile's
        assert_eq!(over(profile, &Value::Null), profile);
    }
}
