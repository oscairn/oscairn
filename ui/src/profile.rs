// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the UI takes from the device profile (docs/profile.schema.json, RFC 0024). The JUCE app
//! validates profiles in full (hosts/standalone/Profile.cpp); this reads the few fields the UI
//! needs and takes anything missing or odd as its default.

use crate::topbar::MeterSettings;
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
    /// Pixels per design unit: the JUCE app's screen scale (hosts/standalone/Profile.h, RFC 0019), so
    /// the Rust UI's sizes match; 0 where the profile does not say the screen's size.
    pub scale: f64,
    /// screens[0].topBar: what the top bar's resource section shows.
    pub top_bar: MeterSettings,
    /// compute.memory in bytes; 0 when the profile does not say.
    pub memory: u64,
    /// The apps Home offers; none: every one (a computer).
    pub apps: Vec<String>,
    /// screens[0].textEntry is `onScreen`: no typing keyboard, so text is typed on the on-screen
    /// keyboard's keys (the JUCE app's TextKeyboard); otherwise (the default) a typing keyboard does it.
    pub text_on_screen: bool,
    /// The on-screen playing keyboard is offered: `onScreenKeyboard`, else unless the device has keys of its own for notes (Profile.cpp).
    pub on_screen_keyboard: bool,
    /// panel.map: the panel map's file name (beside the program, in share/oscairn/panels) or path.
    pub panel_map: String,
    /// The device's control groups (`controls`): what the knob row follows (knobs.rs).
    pub controls: Vec<crate::knobs::Group>,
    /// `audio.device`: the profile fixes the audio device and its channels, so Settings has no Audio page to offer.
    pub audio_fixed: bool,
    /// `rolandNextBoot`: the device can start Roland's own system at the next power-on (the FANTOM's), so About asks
    /// the engine about it. False elsewhere (every other profile, the desktop).
    pub roland_next_boot: bool,
}

/// Profile.h's Screen::scale(): a touch target of 44 units at least 6.5 mm across, never below 1.
fn screen_scale(screen: &Value) -> f64 {
    let n = |k: &str| screen[k].as_f64().filter(|v| *v > 0.).unwrap_or(0.);
    let diagonal = n("diagonal");
    if diagonal == 0. {
        return 0.;
    }
    (6.5 * n("width").hypot(n("height")) / diagonal / 44.).max(1.)
}

impl Profile {
    /// A built-in profile by its id, or a profile file by its path; none: this computer's.
    pub fn load(given: Option<&str>) -> Result<Profile, String> {
        let Some(given) = given.filter(|g| !g.is_empty()) else { return Ok(Profile { on_screen_keyboard: true, ..Profile::default() }) };
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
            scale: screen_scale(screen),
            top_bar: crate::settings::over(MeterSettings::default(), &screen["topBar"]),
            memory: doc["compute"]["memory"].as_u64().unwrap_or(0).min(1 << 24) << 20,
            apps: doc["apps"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect(),
            text_on_screen: screen["textEntry"] == "onScreen",
            on_screen_keyboard: doc["onScreenKeyboard"].as_bool().unwrap_or_else(|| !doc["controls"].as_array().is_some_and(|c| c.iter().any(|c| c["kind"] == "keys" && c["role"] == "notes"))),
            panel_map: doc["panel"]["map"].as_str().unwrap_or_default().into(),
            controls: doc["controls"].as_array().into_iter().flatten().map(crate::knobs::Group::from_profile).collect(),
            audio_fixed: doc["audio"]["device"].is_object(),
            roland_next_boot: doc["rolandNextBoot"].as_bool().unwrap_or(false),
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
    use crate::topbar::Show;
    use serde_json::json;

    #[test]
    fn built_in_profiles_read() {
        let p = Profile::load(Some("fantom-07")).unwrap();
        assert_eq!((p.id.as_str(), p.on_device, p.theme.as_str(), p.frame_rate()), ("fantom-07", true, "oscairn-block", 30));
        assert_eq!((p.memory, p.top_bar), (992 << 20, MeterSettings { cpu: Show::Both, ram: Show::Both }));
        assert_eq!((p.roland_next_boot, Profile::load(None).unwrap().roland_next_boot), (true, false), "only the FANTOM-07's profile offers Roland's system at the next boot");
        assert_eq!((p.apps.len(), p.apps[3].as_str(), p.panel_map.as_str()), (5, "oscairn.setlist", "fantom-07.json"));
        assert_eq!(Profile::load(None).unwrap().frame_rate(), 0); // a computer: no cap
        assert!(Profile::load(Some("no-such-profile")).is_err());
    }

    #[test]
    fn scale_is_the_juce_apps() {
        let p = Profile::parse(r#"{"format":"oscairn.profile","screens":[{"width":1280,"height":720,"diagonal":140}]}"#).unwrap();
        assert!((p.scale - 1.5496).abs() < 1e-3, "{}", p.scale); // the FANTOM-07's
        assert_eq!(Profile::parse(r#"{"format":"oscairn.profile","screens":[{"width":1280,"height":720}]}"#).unwrap().scale, 0.);
    }

    #[test]
    fn frame_rate_from_the_screen() {
        let p = Profile::parse(r#"{"format":"oscairn.profile","platform":{"runtime":"x"},"screens":[{"frameRate":0}]}"#).unwrap();
        assert_eq!(p.frame_rate(), 0); // a device that asks for no cap
        let p = Profile::parse(r#"{"format":"oscairn.profile","screens":[{"frameRate":"fast"}]}"#).unwrap();
        assert_eq!(p.frame_rate(), 0); // odd: the default
        assert!(Profile::parse("[]").is_err() && Profile::parse("{").is_err());
        let p = Profile::parse(r#"{"format":"oscairn.profile","screens":[{"topBar":{"cpu":"bar","ram":"none"}}]}"#).unwrap();
        assert_eq!(p.top_bar, MeterSettings { cpu: Show::Bar, ram: Show::None });
    }

    #[test]
    fn scale_never_below_one() {
        // A screen whose pixels are already at least 6.5 mm across scales by 1, however many pixels it has.
        assert_eq!(screen_scale(&json![{"width":1280, "height":720, "diagonal":144000}]), 1.0);
        // The pixel size unknown still forces the same minimum once the diagonal is.
        assert_eq!(screen_scale(&json![{"diagonal":140}]), 1.0);
        // No or an odd diagonal: unknown, 0.
        assert_eq!(screen_scale(&json![{"width":1280, "height":720, "diagonal":0}]), 0.);
        assert_eq!(screen_scale(&json![{"width":1280, "height":720, "diagonal":-140}]), 0.);
        assert_eq!(screen_scale(&json![{"width":1280, "height":720}]), 0.);
        assert_eq!(screen_scale(&Value::Null), 0.);
    }

    #[test]
    fn parse_defaults_when_fields_missing() {
        // A profile with only the format reads as an empty this-computer profile.
        let p = Profile::parse(r#"{"format":"oscairn.profile"}"#).unwrap();
        assert_eq!((p.id.as_str(), p.theme.as_str(), p.panel_map.as_str()), ("", "", ""));
        assert!(!p.on_device && !p.text_on_screen && !p.audio_fixed);
        assert!(p.on_screen_keyboard, "no keys or notes group: the on-screen keyboard is offered");
        assert_eq!((p.memory, p.frame_rate()), (0, 0));
        assert!(p.apps.is_empty() && p.controls.is_empty());
    }

    #[test]
    fn parse_reads_string_fields() {
        let p = Profile::parse(r#"{"format":"oscairn.profile","id":"mine","platform":{"runtime":"linux-arch"},"screens":[{"theme":"light"}],"panel":{"map":"pan.json"}}"#).unwrap();
        assert_eq!((p.id.as_str(), p.theme.as_str(), p.panel_map.as_str()), ("mine", "light", "pan.json"));
        assert!(p.on_device, "a platform.runtime makes it a device's own profile");
    }

    #[test]
    fn memory_in_megabytes_to_bytes() {
        let p = Profile::parse(r#"{"format":"oscairn.profile","compute":{"memory":1048576},"screens":[{}]}"#).unwrap();
        assert_eq!(p.memory, 1048576 << 20); // 1 TiB
        // Anything over the cap reads as 2^44 bytes (16 TiB).
        let p = Profile::parse(r#"{"format":"oscairn.profile","compute":{"memory":100000000000},"screens":[{}]}"#).unwrap();
        assert_eq!(p.memory, 1u64 << 44);
    }

    #[test]
    fn apps_keeps_only_strings() {
        let p = Profile::parse(r#"{"format":"oscairn.profile","apps":["a","b",42,null,true],"screens":[{}]}"#).unwrap();
        assert_eq!(p.apps, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn text_entry_on_screen_only() {
        assert!(Profile::parse(r#"{"format":"oscairn.profile","screens":[{"textEntry":"onScreen"}]}"#).unwrap().text_on_screen);
        assert!(!Profile::parse(r#"{"format":"oscairn.profile","screens":[{"textEntry":"external"}]}"#).unwrap().text_on_screen);
        assert!(!Profile::parse(r#"{"format":"oscairn.profile","screens":[{"textEntry":1}]}"#).unwrap().text_on_screen);
    }

    #[test]
    fn on_screen_keyboard_off_for_a_device_with_keys() {
        // A group of the device's own playing keys replaces the on-screen keyboard.
        assert!(!Profile::parse(r#"{"format":"oscairn.profile","controls":[{"kind":"keys","role":"notes","count":6}]}"#).unwrap().on_screen_keyboard);
        // A knob group does not.
        assert!(Profile::parse(r#"{"format":"oscairn.profile","controls":[{"kind":"knobs","role":"assign","count":4}]}"#).unwrap().on_screen_keyboard);
        // And an explicit choice wins over the controls.
        assert!(Profile::parse(r#"{"format":"oscairn.profile","onScreenKeyboard":true,"controls":[{"kind":"keys","role":"notes"}]}"#).unwrap().on_screen_keyboard);
    }

    #[test]
    fn audio_fixed_from_audio_device() {
        assert!(Profile::parse(r#"{"format":"oscairn.profile","audio":{"device":{"name":"hw:0"}}}"#).unwrap().audio_fixed);
        assert!(!Profile::parse(r#"{"format":"oscairn.profile","audio":{"device":"none"}}"#).unwrap().audio_fixed);
        assert!(!Profile::parse(r#"{"format":"oscairn.profile"}"#).unwrap().audio_fixed);
    }

    #[test]
    fn controls_read_from_the_profile() {
        let p = Profile::parse(r#"{"format":"oscairn.profile","controls":[{"kind":"knobs","role":"screen","count":4},{"kind":"encoders","paging":"button","count":3}]}"#).unwrap();
        assert_eq!(p.controls.len(), 2);
        assert_eq!((p.controls[0].count, p.controls[0].paged), (4, false));
        assert_eq!((p.controls[1].count, p.controls[1].paged), (3, true));
    }

    #[test]
    fn device_rate_and_cap() {
        // A device with no rate gets the 30-frame default, and a screen's rate is capped at 1000.
        let p = Profile::parse(r#"{"format":"oscairn.profile","platform":{"runtime":"x"},"screens":[{}]}"#).unwrap();
        assert_eq!(p.frame_rate(), 30);
        let p = Profile::parse(r#"{"format":"oscairn.profile","platform":{"runtime":"x"},"screens":[{"frameRate":5000}]}"#).unwrap();
        assert_eq!(p.frame_rate(), 1000);
    }
}
