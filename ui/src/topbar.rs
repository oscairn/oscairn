// SPDX-License-Identifier: AGPL-3.0-or-later
//! The top bar's figures (RFC 0030): where the musician is, the open Performance and its scene,
//! the tempo, and the resource section: how full the CPU is and how much memory is in use, each
//! as a bar, a percent, both or neither (the profile's `screens[].topBar`, the user's
//! `top-bar.json` over it). Kept as what is shown: whole percents, tempo text, so a change too
//! small to see changes nothing and draws nothing.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    None,
    Bar,
    Percent,
    Both,
}

impl Show {
    pub fn parse(text: &str) -> Option<Show> {
        Some(match text {
            "none" => Show::None,
            "bar" => Show::Bar,
            "percent" => Show::Percent,
            "both" => Show::Both,
            _ => return None,
        })
    }
    pub fn bar(self) -> bool {
        matches!(self, Show::Bar | Show::Both)
    }
    pub fn percent(self) -> bool {
        matches!(self, Show::Percent | Show::Both)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeterSettings {
    pub cpu: Show,
    pub ram: Show,
}

impl Default for MeterSettings {
    fn default() -> Self {
        MeterSettings { cpu: Show::Both, ram: Show::Both }
    }
}

/// Green, yellow or red, as the JUCE top bar colors the load (Launcher.cpp: 75% and 90%).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Normal,
    High,
    Full,
}

/// A meter as shown: a whole percent, 0 to 100, or None while not known.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meter(pub Option<u8>);

impl Meter {
    fn of(fraction: f64) -> Meter {
        Meter(fraction.is_finite().then(|| (fraction * 100.).round().clamp(0., 100.) as u8))
    }
    pub fn level(self) -> Level {
        match self.0 {
            Some(p) if p >= 90 => Level::Full,
            Some(p) if p >= 75 => Level::High,
            _ => Level::Normal,
        }
    }
}

/// How full the CPU is: session.status's `cpuLoad`, the audio device callback's share of each
/// block's time (JUCE's getCpuUsage, the figure the JUCE top bar shows). 100% is a callback that
/// takes the whole block: the next block is late and the audio glitches.
pub fn cpu(status: &Value) -> Meter {
    status["cpuLoad"].as_f64().map(Meter::of).unwrap_or_default()
}

/// How much memory is in use: the device's memory (the profile's compute.memory, else the
/// budget's `device`) less what the system has available (memory.state's `systemAvailable`,
/// Linux's MemAvailable), as a share of the device's. 100% is nothing left available. Not known
/// where the engine cannot tell what is available (macOS) or nothing says the device's memory.
pub fn ram(memory: &Value, device_bytes: u64) -> Meter {
    let total = if device_bytes > 0 { device_bytes } else { memory["device"].as_u64().unwrap_or(0) };
    match memory["systemAvailable"].as_u64() {
        Some(available) if available > 0 && total > 0 => Meter::of(1. - available as f64 / total as f64),
        _ => Meter(None),
    }
}

/// transport.get's tempo, as the JUCE top bar writes it.
pub fn tempo(transport: &Value) -> String {
    transport["tempo"].as_f64().filter(|t| t.is_finite()).map(|t| format!("{t:.1} BPM")).unwrap_or_default()
}

/// performance.info's name; an open Performance without one is Untitled, as in the JUCE app.
pub fn performance(info: &Value) -> String {
    match info["name"].as_str() {
        Some("") => "Untitled".into(),
        Some(name) => name.into(),
        None => String::new(),
    }
}

/// graph.get's scene playing, by its name.
pub fn scene(graph: &Value) -> String {
    let id = &graph["scene"];
    graph["scenes"].as_array().and_then(|s| s.iter().find(|s| &s["id"] == id)).and_then(|s| s["name"].as_str()).unwrap_or_default().into()
}

/// A take being recorded, as the JUCE record buttons (Recording.cpp, RecordButtons) show it: the
/// time the first take waiting for its start or recording has run, "m:ss", and whether its writer
/// is behind (`behind`: the disk is slow).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recording {
    pub time: String,
    pub behind: bool,
}

/// record.takes' answer: the first take `waiting` or `recording` (Engine::recordingTake), its time
/// from the frames written and the sample rate (a take with no rate is 0:00); None for no take.
pub fn recording(takes: &Value) -> Option<Recording> {
    let take = takes["takes"].as_array()?.iter().find(|t| matches!(t["state"].as_str(), Some("recording" | "waiting")))?;
    let seconds = match (take["frames"].as_f64(), take["sampleRate"].as_f64()) {
        (Some(frames), Some(rate)) if rate > 0. => (frames / rate).floor().max(0.) as u64,
        _ => 0,
    };
    Some(Recording { time: format!("{}:{:02}", seconds / 60, seconds % 60), behind: take["behind"].as_bool().unwrap_or(false) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cpu_is_the_callbacks_share_of_the_block() {
        assert_eq!(cpu(&json!({"cpuLoad": 0.234})), Meter(Some(23)));
        assert_eq!(cpu(&json!({"cpuLoad": 1.7})), Meter(Some(100))); // late callbacks: full, not past it
        assert_eq!(cpu(&json!({"cpuLoad": -1})), Meter(Some(0)));
        assert_eq!(cpu(&json!({})), Meter(None));
        assert_eq!((Meter(Some(74)).level(), Meter(Some(75)).level(), Meter(Some(90)).level()), (Level::Normal, Level::High, Level::Full));
    }

    #[test]
    fn ram_is_what_is_not_available_of_the_devices_memory() {
        const MIB: u64 = 1 << 20;
        let memory = json!({"systemAvailable": 248 * MIB, "device": 0});
        assert_eq!(ram(&memory, 992 * MIB), Meter(Some(75)));
        assert_eq!(ram(&json!({"systemAvailable": 100 * MIB, "device": 400 * MIB}), 0), Meter(Some(75))); // the budget's device
        assert_eq!(ram(&json!({"systemAvailable": 0}), 992 * MIB), Meter(None)); // macOS: not known
        assert_eq!(ram(&json!({"systemAvailable": 2000 * MIB}), 992 * MIB), Meter(Some(0)));
        assert_eq!(ram(&memory, 0), Meter(None)); // nothing says the device's memory
    }

    #[test]
    fn names_and_tempo() {
        assert_eq!(tempo(&json!({"tempo": 120})), "120.0 BPM");
        assert_eq!(tempo(&json!({})), "");
        assert_eq!((performance(&json!({"name": ""})), performance(&json!({"name": "Strings"}))), ("Untitled".into(), "Strings".into()));
        let graph = json!({"scene": 2, "scenes": [{"id": 1, "name": "Verse"}, {"id": 2, "name": "Chorus"}]});
        assert_eq!((scene(&graph), scene(&json!({"scene": 9}))), ("Chorus".into(), String::new()));
    }

    #[test]
    fn a_take_recording_is_its_time_and_whether_the_disk_is_slow() {
        let takes = json!({"takes": [
            {"take": 1, "state": "done", "frames": 480000, "sampleRate": 48000},
            {"take": 2, "state": "recording", "frames": 48000 * 75 + 100, "sampleRate": 48000, "behind": true},
        ]});
        assert_eq!(recording(&takes), Some(Recording { time: "1:15".into(), behind: true }));
        assert_eq!(recording(&json!({"takes": [{"take": 3, "state": "waiting"}]})), Some(Recording { time: "0:00".into(), behind: false }));
        assert_eq!(recording(&json!({"takes": [{"take": 1, "state": "finishing", "frames": 5}, {"take": 2, "state": "failed"}]})), None, "a take finishing is not recording");
        assert_eq!((recording(&json!({"takes": []})), recording(&json!({}))), (None, None));
    }

    #[test]
    fn show_parses_the_four_choices() {
        let all: Vec<_> = ["none", "bar", "percent", "both", "Both"].iter().map(|s| Show::parse(s)).collect();
        assert_eq!(all, [Some(Show::None), Some(Show::Bar), Some(Show::Percent), Some(Show::Both), None]);
        assert!(Show::Both.bar() && Show::Both.percent() && !Show::Bar.percent() && !Show::Percent.bar() && !Show::None.bar());
    }
}
