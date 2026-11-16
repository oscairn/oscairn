// SPDX-License-Identifier: AGPL-3.0-or-later
//! What narrows the Library's list (LibraryPane.cpp `query_`): a search text, Favorites, Recent, the library the
//! items come from and the tags chosen (all of them must match). The rows that choose them are the sound chooser's
//! (sounds.rs: a Filters row opening Favorites, Recent, Library and Tags), and the query they make is the same.

use super::item::Kind;
use serde_json::{json, Value};

/// An installed library: its ID, name and the number of items it holds (`library.libraries`).
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub items: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filters {
    pub search: String,
    pub favorites: bool,
    pub recent: bool,
    /// "" everything, "mine" the user's own, or an installed library's ID.
    pub source: String,
    /// The tags chosen: all must match.
    pub chosen: Vec<String>,
}

impl Filters {
    /// `library.query`'s parameters for `kind`: the most it takes (1000) at a time.
    pub fn params(&self, kind: Kind) -> Value {
        let mut params = json!({"kind": kind.key(), "limit": 1000});
        if !self.search.is_empty() {
            params["text"] = json!(self.search);
        }
        if !self.chosen.is_empty() {
            params["tags"] = json!(self.chosen);
        }
        if self.favorites {
            params["favorites"] = json!(true);
        }
        if self.recent {
            params["recent"] = json!(true);
        }
        if !self.source.is_empty() {
            params["source"] = json!(self.source);
        }
        params
    }

    /// Whether anything narrows the list.
    pub fn narrowed(&self) -> bool {
        !self.search.is_empty() || !self.chosen.is_empty() || self.favorites || self.recent || !self.source.is_empty()
    }

    pub fn source_name(&self, id: &str, libraries: &[Installed]) -> String {
        match id {
            "" => "Everything".into(),
            "mine" => "My own".into(),
            _ => libraries.iter().find(|l| l.id == id).map_or(id.to_string(), |l| l.name.clone()),
        }
    }

    /// What the Filters row says: what is on, or none.
    pub fn summary(&self, libraries: &[Installed]) -> String {
        let mut on = Vec::new();
        if !self.search.is_empty() {
            on.push(format!("“{}”", self.search));
        }
        if self.favorites {
            on.push("Favorites".to_string());
        }
        if self.recent {
            on.push("Recent".into());
        }
        if !self.source.is_empty() {
            on.push(self.source_name(&self.source, libraries));
        }
        on.extend(self.chosen.iter().cloned());
        if on.is_empty() {
            "none".into()
        } else {
            on.join(", ")
        }
    }

    /// The libraries to choose from: everything, the user's own, then each installed one.
    pub fn sources(&self, libraries: &[Installed]) -> Vec<(String, String, Option<u64>)> {
        let mut out = vec![(String::new(), "Everything".to_string(), None), ("mine".into(), "My own".into(), None)];
        out.extend(libraries.iter().map(|l| (l.id.clone(), l.name.clone(), Some(l.items))));
        out
    }

    /// The tags to choose from: the chosen first, then the most used (stable).
    pub fn tag_list(&self, tags: &[(String, u64)]) -> Vec<(String, u64)> {
        let mut tags = tags.to_vec();
        tags.sort_by_key(|(t, count)| (!self.chosen.contains(t), std::cmp::Reverse(*count)));
        tags
    }

    /// A tag chosen or let go.
    pub fn toggle_tag(&mut self, tag: &str) {
        match self.chosen.iter().position(|t| t == tag) {
            Some(at) => {
                self.chosen.remove(at);
            }
            None => self.chosen.push(tag.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_is_the_sound_choosers() {
        let mut f = Filters::default();
        assert_eq!(f.params(Kind::Sound), json!({"kind": "sound", "limit": 1000}));
        f.search = "warm".into();
        f.favorites = true;
        f.recent = true;
        f.source = "mine".into();
        f.toggle_tag("keys");
        assert_eq!(f.params(Kind::Performance), json!({"kind": "performance", "limit": 1000, "text": "warm", "tags": ["keys"], "favorites": true, "recent": true, "source": "mine"}));
        assert!(f.narrowed());
        assert_eq!(f.summary(&[]), "“warm”, Favorites, Recent, My own, keys");
        f.toggle_tag("keys");
        assert!(f.chosen.is_empty());
    }

    #[test]
    fn tags_chosen_come_first_then_the_most_used() {
        let mut f = Filters::default();
        f.toggle_tag("pad");
        let tags = vec![("keys".to_string(), 5), ("bass".to_string(), 7), ("pad".to_string(), 1), ("lead".to_string(), 5)];
        let names: Vec<String> = f.tag_list(&tags).into_iter().map(|t| t.0).collect();
        assert_eq!(names, ["pad", "bass", "keys", "lead"]);
    }
}
