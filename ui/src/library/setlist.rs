// SPDX-License-Identifier: AGPL-3.0-or-later
//! Add to setlist (LibraryPane.cpp `send`, Setlist.cpp `SetlistPane::add`): a Performance becomes the last slot of
//! the page the setlist is on, in the setlist the Setlist app has open. The JUCE Library asks the Setlist app's
//! process to do it (`apps.request`); the Rust shell has no such process and its Setlist does not edit, so the slot
//! is added in the setlist document itself: `setlists.get`, then `setlists.save` with the one slot more.
//!
//! Which setlist and which page are the Setlist app's own state file (`AppData/oscairn.setlist/state.json`: the
//! setlist's ID and the slot playing), which the Rust Setlist keeps too. With no setlist open the user's own
//! "Setlist" is the one, made with a first page when there is none (Setlist.cpp `create`).

use super::*;
use std::path::PathBuf;

/// The most slots a setlist holds (docs/setlist-format.md).
const MAX_SLOTS: usize = 1000;

/// A Performance on its way into a setlist.
#[derive(Clone, Debug, PartialEq)]
pub struct Add {
    pub id: String,
    pub name: String,
    /// The slot playing, as the state file says, which decides the page.
    pub current: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// library.query: the user's own setlist called "Setlist", when none is open.
    Find(Add),
    /// setlists.get.
    Get(Add),
    /// setlists.save, with the setlist's name.
    Save(String),
}

/// The setlist the Setlist app has open and the slot playing: the state file's `setlist` and `current`.
pub fn read_state(file: &std::path::Path) -> Option<(String, Option<usize>)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    Some((v["setlist"].as_str().filter(|s| !s.is_empty())?.to_string(), v["current"].as_u64().map(|c| c as usize)))
}

/// A new setlist's document, as `setlists.save` takes one (docs/setlist-format.md).
fn new_document(name: &str) -> Value {
    json!({"format": "oscairn.setlist", "version": 2,
        "info": {"id": "", "name": name, "description": "", "tags": [], "color": "", "revision": "", "parent": "", "hash": "", "origin": null},
        "display": {"mode": "list", "columns": 4, "rows": 3}, "pages": [{"name": "Page 1", "slots": []}]})
}

impl Screen {
    /// The state file the Setlist app keeps (the shell says where).
    pub fn set_state_file(&mut self, file: Option<PathBuf>) {
        self.state_file = file;
    }

    pub(super) fn add_to_setlist(&mut self, id: &str) {
        let Some(item) = self.items.iter().find(|i| i.id == id).cloned() else { return };
        let state = self.state_file.as_deref().and_then(read_state);
        let add = Add { id: item.id.clone(), name: item.name.clone(), current: state.as_ref().and_then(|s| s.1) };
        match state {
            Some((setlist, _)) => self.push(Request::Setlist(Step::Get(add)), "setlists.get", json!({"id": setlist})),
            None => {
                let mut params = Filters::default().params(Kind::Setlist);
                params["source"] = json!("mine");
                self.push(Request::Setlist(Step::Find(add)), "library.query", params);
            }
        }
    }

    /// An answer to a step. True when it ended with a setlist saved.
    pub(super) fn setlisted(&mut self, step: Step, result: Result<Value, Error>) -> bool {
        match (step, result) {
            (Step::Find(add), Ok(r)) => {
                // The user's own "Setlist" is opened; else one is made (Setlist.cpp `create`).
                match r["items"].as_array().into_iter().flatten().find(|s| s["name"] == "Setlist" && s["library"].as_str().is_none_or(str::is_empty)).and_then(|s| s["id"].as_str()) {
                    Some(id) => self.push(Request::Setlist(Step::Get(Add { current: None, ..add })), "setlists.get", json!({"id": id})),
                    None => self.append(new_document("Setlist"), add),
                }
            }
            (Step::Get(add), Ok(r)) => self.append(r["setlist"].clone(), add),
            (Step::Save(name), Ok(r)) => {
                // A setlist made here is the one the Setlist app has open from now on.
                if let (Some(file), Some(id)) = (self.state_file.clone(), r["id"].as_str()) {
                    if read_state(&file).is_none() {
                        if let Some(dir) = file.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let _ = std::fs::write(&file, json!({"setlist": id}).to_string());
                    }
                }
                self.say(format!("Added to {name}"));
                return true;
            }
            (Step::Find(_), Err(e)) | (Step::Get(_), Err(e)) | (Step::Save(_), Err(e)) => {
                self.say(if e.code == BUSY { "The engine is busy; add it again in a moment.".to_string() } else { e.message });
            }
        }
        false
    }

    /// The Performance as the last slot of the page the playing slot is on, then the setlist saved.
    fn append(&mut self, mut doc: Value, add: Add) {
        let name = doc["info"]["name"].as_str().unwrap_or_default().to_string();
        let counts: Vec<usize> = doc["pages"].as_array().into_iter().flatten().map(|p| p["slots"].as_array().map_or(0, Vec::len)).collect();
        if counts.iter().sum::<usize>() >= MAX_SLOTS {
            return self.say("A setlist holds at most 1000 slots.");
        }
        if counts.is_empty() {
            doc["pages"] = json!([{"name": "Page 1", "slots": []}]);
        }
        // The page the slot playing is on; the first when none is.
        let page = add.current.and_then(|c| {
            let mut left = c;
            counts.iter().position(|n| {
                if left < *n {
                    true
                } else {
                    left -= n;
                    false
                }
            })
        });
        let page = page.unwrap_or(0);
        if let Some(slots) = doc["pages"][page]["slots"].as_array_mut() {
            slots.push(json!({"id": add.id, "name": add.name, "color": "", "label": ""}));
        }
        self.push(Request::Setlist(Step::Save(name)), "setlists.save", json!({"setlist": doc}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A state file in a scratch folder; `None` leaves the file absent.
    fn state_file(name: &str, content: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oscairn-ui-setlist-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        if let Some(c) = content {
            std::fs::write(&file, c).unwrap();
        }
        file
    }

    #[test]
    fn read_state_reads_the_setlist_and_the_slot() {
        let c = state_file("good", Some(r#"{"setlist": "l1", "current": 2}"#));
        assert_eq!(read_state(&c), Some(("l1".to_string(), Some(2))));
    }

    #[test]
    fn read_state_with_no_slot_playing() {
        let c = state_file("no_current", Some(r#"{"setlist": "l1"}"#));
        assert_eq!(read_state(&c), Some(("l1".to_string(), None)));
    }

    #[test]
    fn read_state_of_a_missing_or_unreadable_file() {
        let c = state_file("absent", None);
        assert_eq!(read_state(&c), None);
    }

    #[test]
    fn read_state_of_something_that_is_not_json() {
        let c = state_file("junk", Some("not json"));
        assert_eq!(read_state(&c), None);
    }

    #[test]
    fn read_state_without_or_with_a_blank_setlist() {
        let missing = state_file("no_setlist", Some(r#"{"current": 1}"#));
        assert_eq!(read_state(&missing), None, "no setlist field");
        let blank = state_file("blank_setlist", Some(r#"{"setlist": ""}"#));
        assert_eq!(read_state(&blank), None, "a blank setlist field");
    }

    #[test]
    fn read_state_of_a_wrong_typed_setlist_or_current() {
        let setlist_number = state_file("setlist_number", Some(r#"{"setlist": 5}"#));
        assert_eq!(read_state(&setlist_number), None, "setlist is a number, not a string");
        let current_float = state_file("current_float", Some(r#"{"setlist": "l1", "current": 1.5}"#));
        assert_eq!(read_state(&current_float), Some(("l1".to_string(), None)), "a fractional slot is not a slot");
        let current_negative = state_file("current_negative", Some(r#"{"setlist": "l1", "current": -1}"#));
        assert_eq!(read_state(&current_negative), Some(("l1".to_string(), None)), "a negative slot is not a slot");
        let current_text = state_file("current_text", Some(r#"{"setlist": "l1", "current": "two"}"#));
        assert_eq!(read_state(&current_text), Some(("l1".to_string(), None)), "a text slot is not a slot");
    }

    #[test]
    fn new_document_makes_a_first_page_enough() {
        let doc = new_document("My Set");
        assert_eq!(doc["format"], "oscairn.setlist");
        assert_eq!(doc["version"], 2);
        assert_eq!(doc["info"]["id"], "");
        assert_eq!(doc["info"]["name"], "My Set");
        assert_eq!(doc["display"]["mode"], "list");
        assert_eq!(doc["display"]["columns"], 4);
        assert_eq!(doc["display"]["rows"], 3);
        assert_eq!(doc["pages"].as_array().map(Vec::len), Some(1));
        assert_eq!(doc["pages"][0]["name"], "Page 1");
        assert_eq!(doc["pages"][0]["slots"].as_array().map(Vec::len), Some(0));
    }
}
