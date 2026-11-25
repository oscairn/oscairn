// SPDX-License-Identifier: AGPL-3.0-or-later
//! The panel's button lamps a screen keeps lit (`panel.lamps`, docs/control-protocol.md): the screen
//! says which lamps it wants lit or off, by the panel map's control names, and only what changed is
//! asked for. Lamps it no longer names go off, so wanting none (the screen left) turns off all it lit.

use serde_json::{json, Value};

#[derive(Default)]
pub struct Lamps {
    known: Vec<(&'static str, bool)>, // as last asked for; a lamp not here may be in any state
    unsupported: bool,                // the engine has no panel lamps: nothing more is asked
}

impl Lamps {
    /// `panel.lamps`' params for what changed, or None: each lamp named as `want` says, each other one lit before off.
    pub fn want(&mut self, want: &[(&'static str, bool)]) -> Option<Value> {
        if self.unsupported {
            return None;
        }
        let mut set = vec![];
        for &(name, on) in want {
            if !self.known.contains(&(name, on)) {
                set.push(json!({"control": name, "on": on}));
            }
        }
        for &(name, on) in &self.known {
            if on && !want.iter().any(|w| w.0 == name) {
                set.push(json!({"control": name, "on": false}));
            }
        }
        self.known = want.to_vec();
        (!set.is_empty()).then(|| json!({ "set": set }))
    }

    /// The engine's answer. Without panel lamps nothing more is asked; refused (too many changes too soon), what
    /// was asked is forgotten, so the next change asks for every lamp again.
    pub fn answered(&mut self, result: Result<&Value, ()>) {
        match result {
            Ok(r) => self.unsupported = r["supported"] == false,
            Err(()) => self.known.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changes_are_asked_for() {
        let mut l = Lamps::default();
        assert_eq!(l.want(&[("dec", false), ("inc", true)]), Some(json!({"set": [{"control": "dec", "on": false}, {"control": "inc", "on": true}]})));
        assert_eq!(l.want(&[("dec", false), ("inc", true)]), None, "nothing at rest");
        assert_eq!(l.want(&[("dec", true), ("inc", true)]), Some(json!({"set": [{"control": "dec", "on": true}]})));
        assert_eq!(l.want(&[]), Some(json!({"set": [{"control": "dec", "on": false}, {"control": "inc", "on": false}]})), "the screen left: off");
        assert_eq!(l.want(&[]), None);
        l.want(&[("inc", true)]);
        l.answered(Err(()));
        assert_eq!(l.want(&[("inc", true)]), Some(json!({"set": [{"control": "inc", "on": true}]})), "refused: asked again");
        l.answered(Ok(&json!({"supported": false, "sent": 0, "unknown": []})));
        assert_eq!(l.want(&[("dec", true)]), None, "no lamps: nothing more asked");
    }
}
