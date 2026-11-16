// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shell's waits: what the player sees while the engine does something that takes a moment, and when something is
//! wrong. Opening a Performance takes a fraction of a second to a few seconds (4 to 20 s for a big one on the FANTOM:
//! docs/control-protocol.md, "Long requests") and is normal: the top bar shows a calm loading chip, "Loading NAME", from
//! the moment the player asks until it has opened, and nothing else changes. The banner under the bar is only for what is
//! unexplained: the engine not answering, a wait far past what it ever takes, the connection gone.
//!
//! What the UI knows of a load: it asked for it and for which name (the Setlist's slot, the Library's row), the engine
//! answered another request as busy and said what it is busy with (`Event::Busy`, then `engine.busy` when it is over: a
//! load another client asked for, whose name is not told), or a `performance.reopen` is out. The engine reports no name
//! and no progress of its own, so the chip's bar sweeps.

use super::*;

/// A wait is far past its usual after this long: 20 s is the slowest measured load (the FANTOM, docs/control-protocol.md),
/// and half as long again is something the player should be told is not usual.
pub(super) const LONG_WAIT: Duration = Duration::from_secs(30);

/// A request nothing explains (it is no load, no install) with no answer after this long: the engine answers in 100 ms, a
/// busy one within milliseconds.
pub(super) const NO_ANSWER: Duration = Duration::from_secs(2);

/// A busy answer's `engine.busy` that never came is forgotten after this long, so the chip cannot stay for ever.
const BUSY_STALE: Duration = Duration::from_secs(120);

#[derive(Default)]
pub(super) struct Wait {
    /// When the wait the chip names began.
    since: Option<Instant>,
    /// What another client's long request is, as the last busy answer named it, and when.
    busy: Option<(String, Instant)>,
    /// Why the connection is lost, until it is made again.
    lost: Option<String>,
    /// Tests: the two limits, shortened so a test need not wait 30 s.
    limits: Option<(Duration, Duration)>,
}

impl Request {
    /// A wait on this request is expected: the chip names it. Reading a setlist is not one.
    pub(super) fn expected(&self) -> bool {
        self.waited_on() && !matches!(self, Request::Setlist(setlist::Request::Get(_)))
    }
}

impl Wait {
    fn long(&self) -> Duration {
        self.limits.map_or(LONG_WAIT, |l| l.0)
    }

    fn no_answer(&self) -> Duration {
        self.limits.map_or(NO_ANSWER, |l| l.1)
    }

    pub(super) fn forget_busy(&mut self) {
        self.busy = None;
    }

    #[cfg(test)]
    pub(super) fn limit(&mut self, long: Duration, no_answer: Duration) {
        self.limits = Some((long, no_answer));
    }
}

impl App {
    pub(super) fn busy_with(&mut self, method: String) {
        self.wait.busy = Some((method, Instant::now()));
        self.refresh_wait();
    }

    pub(super) fn busy_over(&mut self) {
        self.wait.busy = None;
        self.refresh_wait();
    }

    pub(super) fn connection(&mut self, lost: Option<String>) {
        self.wait.busy = None;
        self.wait.lost = lost;
        self.refresh_wait();
    }

    /// What the player waits on for what was expected: the chip's words, and the banner's subject if it lasts too long.
    fn expected_wait(&self) -> Option<(String, String)> {
        let named = |n: String| Some((format!("Loading {n}"), format!("Opening {n}")));
        if let Some(n) = self.setlist.opening_name() {
            return named(n);
        }
        if let Some(n) = self.library.opening_name() {
            return named(n);
        }
        if self.client.waiting(|r| matches!(r, Request::Reopen)).is_some() {
            return Some(("Loading the last Performance".into(), "Opening the last Performance".into()));
        }
        if let Some((method, _)) = self.wait.busy.as_ref().filter(|(_, at)| at.elapsed() < BUSY_STALE) {
            return Some(if crate::client::is_load(method) { ("Loading a Performance".into(), "Opening a Performance".into()) } else { ("Working".into(), "What the engine is doing".into()) });
        }
        self.client.waiting(Request::expected).map(|_| ("Working".into(), "What the engine is doing".into()))
    }

    /// The banner: the connection lost, a wait far past its usual, or a request no answer comes to.
    fn banner(&self, expected: &Option<(String, String)>) -> (String, bool) {
        if let Some(why) = &self.wait.lost {
            return (format!("Lost contact with the sound engine ({why}). Trying again; if this stays, restart the instrument."), true);
        }
        if let (Some((_, subject)), Some(since)) = (expected, self.wait.since) {
            if since.elapsed() >= self.wait.long() {
                return (format!("{subject} is taking much longer than usual. Wait for it, or restart the instrument if it never ends."), false);
            }
        }
        if self.client.waiting(|r| r.waited_on() && !r.expected()).is_some_and(|out| out >= self.wait.no_answer()) {
            return ("The sound engine is slow to answer. This screen catches up when it does; restart the instrument if it never does.".into(), false);
        }
        (String::new(), false)
    }

    /// Works out what the chip and the banner say from what is known now and sets what changed. Called whenever
    /// something that may change it happens, and every 250 ms by the watchdog, which runs only while a wait does.
    pub(super) fn refresh_wait(&mut self) {
        let expected = self.expected_wait();
        match (&expected, self.wait.since) {
            (Some(_), None) => self.wait.since = Some(Instant::now()),
            (None, Some(_)) => self.wait.since = None,
            _ => {}
        }
        let chip = expected.as_ref().map(|e| e.0.clone()).unwrap_or_default();
        let banner = self.banner(&expected);
        // The window's own properties are what is compared, so a change reaches it once and nothing is set at rest.
        if let Some(ui) = self.ui() {
            if ui.global::<Bar>().get_loading() != chip.as_str() {
                ui.global::<Bar>().set_loading(chip.as_str().into());
            }
            if ui.get_engine() != banner.0.as_str() {
                ui.set_engine(banner.0.as_str().into());
            }
            if ui.get_engine_lost() != banner.1 {
                ui.set_engine_lost(banner.1);
            }
        }
        // The watchdog ticks only while something is out or being waited on, so the shell does not wake at rest.
        let needed = expected.is_some() || self.client.waiting(Request::waited_on).is_some();
        if needed && !self.watchdog.running() {
            self.watchdog.start(slint::TimerMode::Repeated, Duration::from_millis(250), || with_app(|a| a.watch()));
        } else if !needed && self.watchdog.running() {
            self.watchdog.stop();
        }
    }
}
