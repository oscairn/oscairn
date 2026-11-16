// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's Audio page (ui/SETTINGS-APP.md D; AudioPage.cpp is the reference, RFC 0024's user choice): the
//! output device, the input device where the engine opens inputs, the sample rate and the buffer size, as
//! `audio.devices` offers them, each changed with `audio.set`. Where the device came from and anything passed over are
//! said beside them; a device the profile fixes is shown so and cannot be changed. Listed again when the status says
//! the devices, the device or its rate or size changed. A change refused says why, and the last device plays on: the
//! engine opens that one again if a new one does not open, which is all the protection the reference has (no question,
//! no timed revert), and so all there is here. Only on a profile that does not fix the device (`Id::applies`).

use crate::client::Error;
use crate::menu;
use crate::settings_app::{choice, failed, faint, head, said, Ctx, Page, Step as Any};
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// audio.devices.
    List,
    /// audio.set, of one choice.
    Set(Which),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Output,
    Input,
    Rate,
    Size,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub name: String,
    pub rates: Vec<f64>,
    pub sizes: Vec<u32>,
}

/// What `audio.devices` and `audio.set` answer (json_rpc.cpp:1307-1334).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Devices {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub output: String,
    pub input: String,
    pub rate: f64,
    pub size: u32,
    pub source: String,
    pub notice: String,
    pub fixed: bool,
}

impl Devices {
    pub fn from_json(v: &Value) -> Devices {
        let list = |k: &str| -> Vec<Device> {
            v[k].as_array()
                .into_iter()
                .flatten()
                .map(|d| Device {
                    name: d["name"].as_str().unwrap_or_default().into(),
                    rates: d["sampleRates"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect(),
                    sizes: d["bufferSizes"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| n as u32).collect(),
                })
                .collect()
        };
        let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        Devices {
            outputs: list("outputs"),
            inputs: list("inputs"),
            output: text("output"),
            input: text("input"),
            rate: v["sampleRate"].as_f64().unwrap_or(0.),
            size: v["bufferSize"].as_u64().unwrap_or(0) as u32,
            source: text("source"),
            notice: text("notice"),
            fixed: v["fixed"].as_bool().unwrap_or(false),
        }
    }

    /// The output device that is open.
    fn open(&self) -> Option<&Device> {
        self.outputs.iter().find(|o| o.name == self.output)
    }
}

/// What the status says of the devices, which makes the page list them again when it changes.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    list: u64,
    device: String,
    rate: f64,
    size: u64,
}

#[derive(Default)]
pub struct Audio {
    devices: Option<Devices>,
    busy: bool,
    /// What is said while a change is out ("Opening it...").
    saying: String,
    /// Where the last refusal is said, until a change goes through.
    refusal: Option<(Which, String)>,
    list_error: String,
    seen: Option<Seen>,
    asking: Option<Which>,
}

/// "48000 Hz".
pub fn hertz(rate: f64) -> String {
    format!("{} Hz", rate.round() as i64)
}

/// "128 samples (2.7 ms)".
pub fn samples(frames: u32, rate: f64) -> String {
    format!("{frames} samples{}", if rate > 0. { format!(" ({:.1} ms)", f64::from(frames) * 1000. / rate) } else { String::new() })
}

impl Audio {
    fn list(&mut self, c: &mut Ctx) {
        self.busy = true;
        c.ask(Any::Audio(Step::List), "audio.devices", json!({}));
    }

    /// The rows that show, in order: the input's only where the engine lists inputs.
    fn shown(&self) -> Vec<Which> {
        let mut shown = vec![Which::Output];
        if self.devices.as_ref().is_some_and(|d| !d.inputs.is_empty()) {
            shown.push(Which::Input);
        }
        shown.extend([Which::Rate, Which::Size]);
        shown
    }

    /// The choices a row offers, by their labels.
    fn options(&self, which: Which) -> Vec<String> {
        let Some(d) = &self.devices else { return vec![] };
        match which {
            Which::Output => d.outputs.iter().map(|o| o.name.clone()).collect(),
            Which::Input => d.inputs.iter().map(|o| o.name.clone()).collect(),
            Which::Rate => d.open().map(|o| o.rates.iter().map(|r| hertz(*r)).collect()).unwrap_or_default(),
            Which::Size => d.open().map(|o| o.sizes.iter().map(|s| samples(*s, d.rate)).collect()).unwrap_or_default(),
        }
    }

    fn current(&self, which: Which) -> String {
        let Some(d) = &self.devices else { return "Reading...".into() };
        let text = match which {
            Which::Output => d.output.clone(),
            Which::Input => d.input.clone(),
            Which::Rate => hertz(d.rate),
            Which::Size => samples(d.size, d.rate),
        };
        if self.options(which).contains(&text) {
            text
        } else {
            "–".into()
        }
    }

    /// Whether the row can be changed now: not while a request is out, not a device the profile fixes, and only where
    /// there is something to choose from (AudioPage.cpp `show`).
    fn enabled(&self, which: Which) -> bool {
        let fixed = self.devices.as_ref().is_some_and(|d| d.fixed);
        !self.busy && self.options(which).len() > 1 && !(which == Which::Output && fixed)
    }

    fn apply(&mut self, which: Which, i: usize, c: &mut Ctx) {
        if self.busy {
            return;
        }
        let Some(d) = &self.devices else { return };
        let params = match which {
            Which::Output => d.outputs.get(i).map(|o| json!({"output": o.name})),
            Which::Input => d.inputs.get(i).map(|o| json!({"input": o.name})),
            // The handler reads the rate as an integer (json_rpc.cpp:1316).
            Which::Rate => d.open().and_then(|o| o.rates.get(i)).map(|r| json!({"sampleRate": r.round() as i64})),
            Which::Size => d.open().and_then(|o| o.sizes.get(i)).map(|s| json!({"bufferSize": s})),
        };
        let Some(params) = params else { return };
        self.busy = true;
        self.refusal = None;
        self.saying = if matches!(which, Which::Output | Which::Input) { "Opening it..." } else { "Restarting the audio device..." }.into();
        c.ask(Any::Audio(Step::Set(which)), "audio.set", params);
    }

    pub fn reply(&mut self, step: Step, result: Result<Value, Error>, c: &mut Ctx) {
        match (step, result) {
            (Step::List, Ok(r)) => {
                self.busy = false;
                self.list_error.clear();
                self.devices = Some(Devices::from_json(&r));
            }
            (Step::List, Err(e)) => {
                self.busy = false;
                self.list_error = e.message;
            }
            (Step::Set(_), Ok(r)) => {
                self.busy = false;
                self.saying.clear();
                self.devices = Some(Devices::from_json(&r));
            }
            // Refused, or the device did not open (the last one plays again): said by what was changed, and listed again as it is.
            (Step::Set(which), Err(e)) => {
                self.saying.clear();
                self.refusal = Some((which, e.message));
                self.list(c);
            }
        }
    }

    /// The device's own line: why it is the one open, and anything passed over on the way (AudioPage.cpp `show`).
    fn device_lines(&self) -> Vec<InfoLine> {
        let mut lines = vec![];
        if !self.list_error.is_empty() {
            lines.extend(failed(&format!("The devices could not be read: {}", self.list_error)));
            return lines;
        }
        let Some(d) = &self.devices else { return said("Reading...") };
        if !d.notice.is_empty() {
            lines.extend(failed(&d.notice));
        }
        lines.extend(said(if d.source == "null" {
            "No audio device: started with --audio null."
        } else if d.fixed {
            "Fixed by the device profile: its channels are wired to this device."
        } else if d.source == "user" {
            "Chosen here; it opens again at the next start."
        } else if d.source == "profile" {
            "Set when the engine started, by this device's profile or its options."
        } else {
            "The system's default device."
        }));
        lines
    }
}

impl Page for Audio {
    fn open(&mut self, c: &mut Ctx) {
        self.seen = None;
        self.refusal = None;
        self.saying.clear();
        self.list(c);
    }

    fn noun(&self) -> &'static str {
        "Settings"
    }

    fn rows(&self) -> Vec<SettingData> {
        self.shown()
            .into_iter()
            .map(|w| {
                let label = match w {
                    Which::Output => "Output",
                    Which::Input => "Input",
                    Which::Rate => "Sample rate",
                    Which::Size => "Buffer size",
                };
                let r = choice(label, self.current(w));
                if self.enabled(w) {
                    r
                } else {
                    faint(r)
                }
            })
            .collect()
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let shown = self.shown();
        let which = at.and_then(|a| shown.get(a)).copied().unwrap_or(Which::Output);
        let mut lines = vec![head(match which {
            Which::Output => "Output",
            Which::Input => "Input",
            Which::Rate => "Sample rate",
            Which::Size => "Buffer size",
        })];
        lines.extend(match which {
            Which::Output => self.device_lines(),
            Which::Input => said("The audio inputs' device."),
            Which::Rate => {
                let one = self.devices.as_ref().is_some_and(|d| d.fixed && d.open().is_some_and(|o| o.rates.len() == 1));
                said(if one { "Fixed by the device profile." } else { "Samples a second. A new rate restarts every plugin at it, keeping its sound." })
            }
            Which::Size => said("Smaller answers your playing sooner; larger is safer from dropouts."),
        });
        if let Some((w, why)) = &self.refusal {
            if *w == which {
                lines.push(head("Refused"));
                lines.extend(failed(why));
            }
        }
        if which != Which::Output && self.refusal.as_ref().is_some_and(|(w, _)| *w == Which::Output) {
            lines.push(head("Output refused"));
            lines.extend(failed(&self.refusal.as_ref().map(|r| r.1.clone()).unwrap_or_default()));
        }
        lines
    }

    fn message(&self) -> String {
        self.saying.clone()
    }

    fn press(&mut self, at: usize, c: &mut Ctx) {
        let Some(which) = self.shown().get(at).copied() else { return };
        if !self.enabled(which) {
            return;
        }
        let current = self.current(which);
        self.asking = Some(which);
        let title = match which {
            Which::Output => "Output",
            Which::Input => "Input",
            Which::Rate => "Sample rate",
            Which::Size => "Buffer size",
        };
        c.menu = Some((title.into(), self.options(which).into_iter().enumerate().map(|(i, o)| menu::Item::ticked(o.as_str(), i, o == current)).collect()));
    }

    fn chosen(&mut self, id: usize, c: &mut Ctx) {
        if let Some(which) = self.asking.take() {
            // The one already open is not a change.
            if self.options(which).get(id).is_some_and(|o| *o == self.current(which)) {
                return;
            }
            self.apply(which, id, c);
        }
    }

    fn status(&mut self, s: &Value, c: &mut Ctx) {
        let now = Seen { list: s["audioDeviceList"].as_u64().unwrap_or(0), device: s["audioDevice"].as_str().unwrap_or_default().into(), rate: s["sampleRate"].as_f64().unwrap_or(0.), size: s["bufferSize"].as_u64().unwrap_or(0) };
        // The status changed (devices came or went, or the device, its rate or size changed, here or elsewhere): read again.
        match &self.seen {
            None => self.seen = Some(now),
            Some(was) if *was != now && !self.busy => {
                self.seen = Some(now);
                self.list(c);
            }
            _ => {}
        }
    }
}
