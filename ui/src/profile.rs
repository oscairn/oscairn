// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the UI takes from the device profile (docs/profile.schema.json, RFC 0024). The JUCE app
//! validates profiles in full (hosts/standalone/Profile.cpp); this reads the few fields the UI
//! needs and takes anything missing or odd as its default.

use serde_json::Value;

mod built_in {
    include!(concat!(env!("OUT_DIR"), "/built_in.rs"));
}
pub use built_in::{DEFAULT_THEME, THEMES};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub id: String,
    /// A device's own image (it has a platform.runtime); the desktop app on a computer has none.
    pub on_device: bool,
    /// The screen's theme until the user chooses one.
    pub theme: String,
    /// screens[0].frameRate: the most frames a second the UI draws; None: the default.
    frame_rate: Option<u32>,
}

impl Profile {
    /// A built-in profile by its id, or a profile file by its path; none: this computer's.
    pub fn load(given: Option<&str>) -> Result<Profile, String> {
        let Some(given) = given.filter(|g| !g.is_empty()) else { return Ok(Profile::default()) };
        let text = match built_in::PROFILES.iter().find(|(id, _)| *id == given) {
            Some((_, text)) => text.to_string(),
            None => std::fs::read_to_string(given).map_err(|e| format!("no built-in profile {given}, and {given}: {e}"))?,
        };
        Profile::parse(&text).map_err(|e| format!("{given}: {e}"))
    }

    pub fn parse(text: &str) -> Result<Profile, String> {
        let doc: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if doc["format"] != "oscairn.profile" {
            return Err("not an Oscairn profile".into());
        }
        let screen = &doc["screens"][0];
        Ok(Profile {
            id: doc["id"].as_str().unwrap_or_default().into(),
            on_device: doc["platform"]["runtime"].as_str().is_some_and(|r| !r.is_empty()),
            theme: screen["theme"].as_str().unwrap_or_default().into(),
            frame_rate: screen["frameRate"].as_u64().map(|f| f.min(1000) as u32),
        })
    }

    /// Frames a second at most, 0 for no cap: the profile's, else 30 on a device (the audio shares
    /// its cores with the UI) and none on a computer.
    pub fn frame_rate(&self) -> u32 {
        self.frame_rate.unwrap_or(if self.on_device { 30 } else { 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_profiles_read() {
        let p = Profile::load(Some("fantom-07")).unwrap();
        assert_eq!((p.id.as_str(), p.on_device, p.theme.as_str(), p.frame_rate()), ("fantom-07", true, "oscairn-block", 30));
        assert_eq!(Profile::load(None).unwrap().frame_rate(), 0); // a computer: no cap
        assert!(Profile::load(Some("no-such-profile")).is_err());
    }

    #[test]
    fn frame_rate_from_the_screen() {
        let p = Profile::parse(r#"{"format":"oscairn.profile","platform":{"runtime":"x"},"screens":[{"frameRate":0}]}"#).unwrap();
        assert_eq!(p.frame_rate(), 0); // a device that asks for no cap
        let p = Profile::parse(r#"{"format":"oscairn.profile","screens":[{"frameRate":"fast"}]}"#).unwrap();
        assert_eq!(p.frame_rate(), 0); // odd: the default
        assert!(Profile::parse("[]").is_err() && Profile::parse("{").is_err());
    }
}
