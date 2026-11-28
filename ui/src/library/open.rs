// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opening a Performance and trying one (LibraryPane.cpp `choose`, `startAudition`, `stopAudition`;
//! SoundPickerModel.cpp `applySoundAction`).
//!
//! Opening is a chain the JUCE app runs on a connection of its own: what would opening cost and would it fit beside
//! the open one (`budget.estimate`), prepared while that one plays when it would (`performance.prepare`), then
//! opened (`library.load`). Unsaved changes to the open Performance are asked about first. Each step can take
//! seconds, and the engine answers every other request as busy meanwhile (-32009: it did not run), so a step
//! refused is asked again in a moment; the screen says what is opening, the UI is never blocked, and a second
//! Performance chosen meanwhile waits and replaces any waiting (the last chosen is the one opened).

use super::*;
use std::time::Instant;

/// Where an opening is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// performance.info: is anything unsaved.
    Info,
    /// library.save, before opening.
    Save,
    /// budget.estimate: would it fit beside the open one.
    Estimate,
    /// performance.prepare: loaded beside the open one, which keeps playing.
    Prepare,
    /// library.load.
    Load,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Aud {
    Start,
    /// audition.stop, kept or put back.
    Stop(bool),
    Get,
}

/// A Performance being opened.
#[derive(Clone, Debug)]
pub struct Open {
    pub id: String,
    pub name: String,
    pub path: String,
    step: Step,
    /// The open Performance's name and whether it was ever saved, from `performance.info`.
    open_name: String,
    open_saved: bool,
    /// What the Performance being left is saved under, when it was never saved.
    save_as: Option<String>,
    /// Whether it opens beside the one playing (else it is a cut).
    seamless: bool,
    /// The open Performance itself, pressed within FRESH_WITHIN of this screen opening it (issue 333, the owner 2026-11-27): it opens
    /// fresh, keeping nothing. Told by `performance.info`'s id.
    fresh: bool,
    since: Instant,
}

/// How soon after it opened a press on the same Performance asks for a fresh load (issue 333: "within a couple of seconds").
pub const FRESH_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

impl Open {
    fn new(item: &Item) -> Open {
        Open { id: item.id.clone(), name: item.title().to_string(), path: item.path.clone(), step: Step::Info, open_name: String::new(), open_saved: false, save_as: None, seamless: true, fresh: false, since: Instant::now() }
    }

    /// What the line under the list says while it opens: what, and how long it has been.
    pub fn saying(&self) -> String {
        let secs = self.since.elapsed().as_secs();
        format!("Opening {}...{}", self.name, if secs >= 1 { format!(" {secs} s") } else { String::new() })
    }
}

/// A fresh load said (issue 333).
const FRESH: &str = "Performance opened fresh: every plugin loaded again from its saved files.";

fn load_params(id: &str, fresh: bool) -> Value {
    if fresh { json!({"id": id, "fresh": true}) } else { json!({"id": id}) }
}

/// The reference's words for what a Performance that would not fit beside the one ringing out does.
const CUT: &str = "Opened at once: it and the last Performance ringing out would not fit this device together.";

impl Screen {
    /// A press on a Performance: it opens in place of the open one (after the question about unsaved changes).
    pub(super) fn open_performance(&mut self, id: &str) {
        let Some(item) = self.items.iter().find(|i| i.id == id).cloned() else { return };
        let open = Open::new(&item);
        if self.job.is_some() {
            // The last chosen is the one that opens next (Engine.cpp `soundAction`).
            self.say(format!("{} opens next", open.name));
            self.waiting = Some(open);
            return;
        }
        self.begin(open);
    }

    fn begin(&mut self, open: Open) {
        self.say(open.saying());
        self.job = Some(open);
        self.ask_step();
    }

    /// The request for the step the opening is at.
    fn ask_step(&mut self) {
        let Some(job) = &self.job else { return };
        let (step, id, path) = (job.step, job.id.clone(), job.path.clone());
        let name = job.save_as.clone();
        let (method, params) = match step {
            Step::Info => ("performance.info", json!({})),
            Step::Save => ("library.save", name.map_or(json!({}), |n| json!({"name": n}))),
            Step::Estimate => ("budget.estimate", json!({"directory": path})),
            Step::Prepare => ("performance.prepare", json!({"directory": path})),
            Step::Load => ("library.load", load_params(&id, job.fresh)),
        };
        self.push(Request::Open(step), method, params);
    }

    fn go(&mut self, step: Step) {
        if let Some(job) = &mut self.job {
            job.step = step;
        }
        self.ask_step();
    }

    /// The opening ended, well or not: what was chosen meanwhile opens next.
    fn finished(&mut self) {
        self.job = None;
        if let Some(next) = self.waiting.take() {
            self.begin(next);
        }
    }

    fn failed(&mut self, what: &str, why: &str) {
        self.say(format!("{what}: {why}"));
        self.finished();
    }

    /// An answer to a step. True when the Performance changed under the screen.
    pub(super) fn opened(&mut self, step: Step, result: Result<Value, Error>) -> bool {
        let Some(job) = self.job.as_ref().filter(|j| j.step == step) else { return false };
        let name = job.name.clone();
        // Refused as busy: it did not run, so it is asked again in a moment (the JUCE job waits its turn the same way).
        if let Err(e) = &result {
            if e.code == BUSY {
                let (method, params) = self.step_request();
                self.push_after(crate::client::BUSY_RETRY, Request::Open(step), method, params);
                return false;
            }
        }
        match (step, result) {
            (Step::Info, Ok(r)) => {
                let unsaved = r["unsaved"] == true;
                let recent = self.last_opened.clone();
                if let Some(job) = &mut self.job {
                    job.open_name = r["name"].as_str().unwrap_or_default().to_string();
                    job.open_saved = r["id"].as_str().is_some_and(|i| !i.is_empty());
                    job.fresh = r["id"].as_str() == Some(job.id.as_str()) && recent.is_some_and(|(id, at)| id == job.id && at.elapsed() < FRESH_WITHIN);
                }
                if unsaved {
                    self.ask_to_save(&name);
                } else {
                    self.go(Step::Estimate);
                }
            }
            // The engine did not answer: nothing is asked (LibraryPane.cpp `askToSave`).
            (Step::Info, Err(_)) => self.go(Step::Estimate),
            (Step::Save, Ok(_)) => self.go(Step::Estimate),
            (Step::Save, Err(e)) => self.failed("Not saved", &e.message),
            (Step::Estimate, Ok(r)) => {
                let seamless = r["seamless"] == true;
                if let Some(job) = &mut self.job {
                    job.seamless = seamless;
                }
                // A fresh load reads its own: what is prepared ahead is not published (control-protocol.md).
                let fresh = self.job.as_ref().is_some_and(|j| j.fresh);
                self.go(if seamless && !fresh { Step::Prepare } else { Step::Load });
            }
            (Step::Prepare, Ok(_)) => self.go(Step::Load),
            (Step::Load, Ok(_)) => {
                let cut = self.job.as_ref().is_some_and(|j| !j.seamless);
                let opened = if self.job.as_ref().is_some_and(|j| j.fresh) { FRESH } else { "Performance opened. Press Play or play a note." };
                self.last_opened = self.job.as_ref().map(|j| (j.id.clone(), Instant::now()));
                self.say(if cut { format!("{opened} {CUT}") } else { opened.to_string() });
                self.finished();
                self.query(); // it is now the most recent
                return true;
            }
            (_, Err(e)) => self.failed("Not opened", &e.message),
        }
        false
    }

    fn step_request(&self) -> (&'static str, Value) {
        let Some(job) = &self.job else { return ("performance.info", json!({})) };
        match job.step {
            Step::Info => ("performance.info", json!({})),
            Step::Save => ("library.save", job.save_as.clone().map_or(json!({}), |n| json!({"name": n}))),
            Step::Estimate => ("budget.estimate", json!({"directory": job.path})),
            Step::Prepare => ("performance.prepare", json!({"directory": job.path})),
            Step::Load => ("library.load", load_params(&job.id, job.fresh)),
        }
    }

    /// "Save changes to X before opening Y?" with Save, Don't Save and Cancel.
    fn ask_to_save(&mut self, opening: &str) {
        let name = self.job.as_ref().map(|j| j.open_name.clone()).unwrap_or_default();
        let name = if name.is_empty() { "Untitled".to_string() } else { name };
        self.notice = Some((format!("Save changes to {name} before opening {opening}?"), "Your changes will be lost if you don't save them.".into(), vec!["Save".into(), "Don't Save".into(), "Cancel".into()]));
        self.notice_for = Some(NoticeFor::SaveFirst);
    }

    /// The answer to the question about unsaved changes.
    pub(super) fn save_first(&mut self, choice: Option<usize>) -> Outcome {
        match choice {
            Some(0) => {
                // Over the Performance if it was ever saved, else its name is asked for (LibraryPane.cpp `savePerformance`).
                let (saved, name) = self.job.as_ref().map(|j| (j.open_saved, j.open_name.clone())).unwrap_or_default();
                if saved {
                    self.go(Step::Save);
                } else {
                    self.asking = Some(Asking::SaveName);
                    return Outcome::Prompt("Save: the Performance's name".into(), name);
                }
            }
            Some(1) => self.go(Step::Estimate),
            _ => {
                self.say("");
                self.finished();
            }
        }
        Outcome::Stay
    }

    /// The name a Performance never saved is saved under before another opens.
    pub(super) fn save_named(&mut self, name: &str) {
        if let Some(job) = &mut self.job {
            job.save_as = (!name.is_empty()).then(|| name.to_string());
        }
        self.go(Step::Save);
    }

    pub(super) fn save_cancelled(&mut self) {
        self.say("");
        self.finished();
    }

    // ---- trying an item ----

    pub(super) fn can_audition(&self) -> bool {
        self.target().and_then(|id| self.items.iter().find(|i| i.id == id)).is_some_and(|i| i.kind == "sound" || i.kind == "performance")
    }

    /// Audition on the item Options is about, or stop the one being tried.
    pub(super) fn audition_toggle(&mut self) {
        if !self.auditioning.id.is_empty() {
            return self.stop_audition(false);
        }
        if let Some(id) = self.target() {
            self.start_audition(&id);
        }
    }

    pub(super) fn start_audition(&mut self, id: &str) {
        let Some(item) = self.items.iter().find(|i| i.id == id).cloned() else { return };
        if item.kind != "sound" && item.kind != "performance" {
            return; // a Sound or a Performance is tried, nothing else
        }
        // One engine job at a time: what is asked while another loads would be lost.
        if self.job.is_some() {
            return self.say("Still loading; try again in a moment.");
        }
        let mut params = json!({"id": item.id});
        let part = if item.kind == "sound" { self.part } else { 0 };
        if item.kind == "sound" {
            if part == 0 {
                return self.say("Show a part in the Part view to audition a Sound in it.");
            }
            params["part"] = json!(part);
        }
        self.push(Request::Audition(Aud::Start), "audition.start", params);
        self.auditioning = Auditioning { id: item.id.clone(), kind: item.kind.clone(), part };
        self.selected = Some(item.id.clone());
        self.say(format!("Auditioning {}{}", item.title(), if item.kind == "sound" { " in the part shown." } else { " in place of the open Performance." }));
    }

    pub(super) fn stop_audition(&mut self, keep: bool) {
        if self.auditioning.id.is_empty() {
            return;
        }
        if self.job.is_some() {
            return self.say("Still loading; stop again in a moment.");
        }
        self.push(Request::Audition(Aud::Stop(keep)), "audition.stop", json!({"keep": keep}));
        self.auditioning = Auditioning::default();
        self.say(if keep { "Kept." } else { "Put back as it was." });
    }

    /// An answer about an audition. True when the Performance changed under the screen.
    pub(super) fn auditioned(&mut self, what: Aud, result: Result<Value, Error>) -> bool {
        match (what, result) {
            (Aud::Start, Ok(_)) => true,
            (Aud::Stop(_), Ok(_)) => true,
            (Aud::Get, Ok(r)) => {
                // Ended elsewhere (a Performance opened keeps it): the marks go.
                let id = r["id"].as_str().unwrap_or_default();
                if id != self.auditioning.id {
                    self.auditioning = if id.is_empty() { Auditioning::default() } else { Auditioning { id: id.to_string(), kind: r["kind"].as_str().unwrap_or_default().to_string(), part: r["part"].as_u64().unwrap_or(0) as u32 } };
                }
                false
            }
            (Aud::Start, Err(e)) => {
                self.auditioning = Auditioning::default();
                self.say(format!("Not auditioned: {}", e.message));
                false
            }
            (_, Err(e)) if e.code != 0 => {
                self.say(crate::client::refusal(&e));
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn open_named(name: &str, since: Instant) -> Open {
        Open { id: "id".into(), name: name.into(), path: "p".into(), step: Step::Info, open_name: String::new(), open_saved: false, save_as: None, seamless: true, fresh: false, since }
    }

    // The line under the list while it opens: what, and the whole seconds once it has been a second (docs/rust-ui.md
    // "the line under the list says 'Opening X... N s'"). Before a second it says the name without the time.
    #[test]
    fn saying_waits_a_second_before_the_time() {
        assert_eq!(open_named("X", Instant::now()).saying(), "Opening X...");
    }

    #[test]
    fn saying_counts_the_whole_seconds_after_one() {
        let past = Instant::now() - Duration::from_millis(3500);
        assert_eq!(open_named("X", past).saying(), "Opening X... 3 s");
    }

    // A name keeps its words and characters whole (nothing is cut or wrapped while it opens).
    #[test]
    fn saying_keeps_a_long_and_unicode_name_whole() {
        let past = Instant::now() - Duration::from_millis(2500);
        let name = "Shards, no longer yours — 𝔫𝔦𝔠𝔢";
        assert_eq!(open_named(name, past).saying(), format!("Opening {name}... 2 s"));
    }
}
