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

fn read(name: &str) -> Option<String> {
    std::fs::read_to_string(config_dir()?.join(name)).ok()
}

/// The theme the user chose, by its id; empty: none chosen.
pub fn chosen_theme() -> String {
    read("appearance.txt").unwrap_or_default().trim().to_string()
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
