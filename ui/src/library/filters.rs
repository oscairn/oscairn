// SPDX-License-Identifier: AGPL-3.0-or-later
//! What narrows the Library's list (LibraryPane.cpp `query_`): a search text, Favorites, Recent, the library the
//! items come from and the tags chosen (all of them must match). The Library's sidebar (side.rs) chooses them; the sound
//! chooser (sounds.rs) has its own rows for the same choices, and the query they make is the same.

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

    #[test]
    fn a_chosen_tag_comes_first_even_rare_and_nothing_chosen_is_most_used() {
        let mut f = Filters::default();
        f.toggle_tag("rare");
        let tags = vec![("hot".to_string(), 99), ("rare".to_string(), 1)];
        let names: Vec<String> = f.tag_list(&tags).into_iter().map(|t| t.0).collect();
        assert_eq!(names, ["rare", "hot"]);
        // nothing chosen: the most used lead, the ties keep their order.
        let tags = vec![("a".to_string(), 2), ("b".to_string(), 9), ("c".to_string(), 2)];
        let names: Vec<String> = Filters::default().tag_list(&tags).into_iter().map(|t| t.0).collect();
        assert_eq!(names, ["b", "a", "c"]);
        assert!(Filters::default().tag_list(&[]).is_empty(), "no tags at all lists none");
    }

    #[test]
    fn a_sources_name_is_everything_my_own_or_the_librarys() {
        let libraries = [Installed { id: "factory".into(), name: "Factory".into(), items: 40 }];
        let f = Filters::default();
        assert_eq!(f.source_name("", &libraries), "Everything");
        assert_eq!(f.source_name("mine", &libraries), "My own");
        assert_eq!(f.source_name("factory", &libraries), "Factory");
        // a source the engine no longer lists is shown by its id.
        assert_eq!(f.source_name("gone", &libraries), "gone");
    }

    #[test]
    fn the_libraries_to_choose_are_everything_my_own_then_each_installed() {
        let f = Filters::default();
        assert_eq!(f.sources(&[]), vec![
            (String::new(), "Everything".to_string(), None),
            ("mine".into(), "My own".to_string(), None),
        ]);
        let libraries = [Installed { id: "factory".into(), name: "Factory".into(), items: 40 }];
        assert_eq!(f.sources(&libraries), vec![
            (String::new(), "Everything".to_string(), None),
            ("mine".into(), "My own".to_string(), None),
            ("factory".to_string(), "Factory".to_string(), Some(40)),
        ]);
    }

    #[test]
    fn a_chosen_alone_narrows_and_leaves_the_other_fields_out_of_the_query() {
        assert_eq!(Filters { favorites: true, ..Default::default() }.params(Kind::Performance), json!({"kind": "performance", "limit": 1000, "favorites": true}));
        assert_eq!(Filters { source: "factory".into(), ..Default::default() }.params(Kind::Setlist), json!({"kind": "setlist", "limit": 1000, "source": "factory"}));
        assert_eq!(Filters { source: "factory".into(), chosen: vec!["keys".into()], ..Default::default() }.params(Kind::Setlist), json!({"kind": "setlist", "limit": 1000, "source": "factory", "tags": ["keys"]}));
    }

    #[test]
    fn anything_chosen_narrows_the_list() {
        assert!(!Filters::default().narrowed());
        assert!(Filters { search: "x".into(), ..Default::default() }.narrowed());
        assert!(Filters { recent: true, ..Default::default() }.narrowed());
        let mut f = Filters::default();
        f.toggle_tag("keys");
        assert!(f.narrowed());
    }
}
