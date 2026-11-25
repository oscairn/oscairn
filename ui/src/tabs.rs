// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pages and tabs of a plugin's parameters, as one structure the widgets read (ui/TABS.md): ordered
//! pages, each with ordered tabs, each parameter on a page and perhaps on one of its tabs. Today it is filled
//! by inference from the module paths, as the JUCE ParameterGrid does (hosts/standalone/ParameterGrid.cpp
//! `build`, `tabsFor`: a page per top-level module, a tab per second-level module on a page of more than 24
//! parameters); it can as well be filled from what a plugin declares (the layout extension's draft 6:
//! `tab_count`, `tab`, `param_tab`, in /oscairn-clap's ui.h), which changes no widget. A parameter on a page that
//! has tabs but on no tab is drawn above the tabs whichever tab is open (the header's rule).

use crate::grid::Param;
use crate::params::{after, editor_pages, level, tabs_for};
use serde_json::Value;

/// The most pages, and tabs under a page, the host reads (ui.h `tab_count`).
pub const MOST: usize = 16;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub pages: Vec<Page>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub name: String,
    /// The tabs' names in order: none, or two or more (a page with fewer is drawn without them). An empty name
    /// is drawn as "General".
    pub tabs: Vec<String>,
    /// The page's parameters in the plugin's order.
    pub params: Vec<Placed>,
    /// The page's tabs were the indexes of one view: they are not tabs, the view shows its siblings as a master and
    /// a detail (`Model::fold_indexed`), and these are what its indexes were called.
    pub indexed: Option<Indexed>,
}

/// The view a page's former tabs were the indexes of, and the tabs' names by index ("Operator 3" for 3).
#[derive(Clone, Debug, PartialEq)]
pub struct Indexed {
    pub view: u64,
    pub names: Vec<(u64, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// An index into the editor's parameters.
    pub param: usize,
    /// The tab it is on; None on a page that has tabs: above them, whichever is open.
    pub tab: Option<usize>,
    /// The heading its tile sits under on the page or tab: what is left of its module path.
    pub heading: String,
}

/// One entry of the page or tab list a plugin declares, and which entry each parameter is on.
/// PROVISIONAL: the engine's `params.list` carries none of this yet, so the names of the fields it is read
/// from (`uiTabs` {id, parent, name} at the top, `uiTab` on a parameter) are this UI's own until the engine's
/// protocol names them; see `declared_in`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declared {
    /// (id, parent: 0 a page, else a page's id, name), in the order the plugin lists them.
    pub entries: Vec<(u32, u32, String)>,
}

/// What a plugin declares in a `params.list` answer, or None when it declares nothing.
/// PROVISIONAL, see `Declared`.
pub fn declared_in(list: &Value) -> Option<Declared> {
    let entries: Vec<(u32, u32, String)> = list["uiTabs"]
        .as_array()?
        .iter()
        .map(|e| (e["id"].as_u64().unwrap_or(0) as u32, e["parent"].as_u64().unwrap_or(0) as u32, e["name"].as_str().unwrap_or_default().to_string()))
        .collect();
    (!entries.is_empty()).then_some(Declared { entries })
}

/// One position in the order the screen steps through: a page, or a tab of a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub page: usize,
    pub tab: Option<usize>,
}

impl Model {
    /// The model a plugin gets: its declared pages and tabs where it declares any, else the inferred ones.
    pub fn build(params: &[Param], declared: Option<&Declared>) -> Model {
        let mut model = match declared {
            Some(d) => Model::declared(params, d),
            None => Model::inferred(params),
        };
        model.fold_indexed(params);
        model
    }

    /// A page whose tabs are exactly the indexes of one view that has an overview is not a page of tabs: the tabs go
    /// and the view shows its indexes as a master and detail (ui/TABS.md "Tabs that are indexes"). The rule: two or
    /// more tabs; every tab but at most one with no name (the page's own, which keeps its parameters above where
    /// the tabs were) is a name ending in a number, all with the same words before it and no number twice; every
    /// parameter on such a tab is of one and the same view and carries the tab's number as its index; and that view
    /// has an overview (an algorithm: `grid::master_view`). Anything else stays tabs.
    pub fn fold_indexed(&mut self, params: &[Param]) {
        for page in &mut self.pages {
            let Some(found) = indexed_view(params, page) else { continue };
            page.tabs.clear();
            for x in &mut page.params {
                x.tab = None;
            }
            page.indexed = Some(found);
        }
    }

    /// From the module paths, as ParameterGrid does: a page per top-level module (numbered pages together), a tab
    /// per second-level module on a page of more than 24 parameters when there are two or more.
    pub fn inferred(params: &[Param]) -> Model {
        let pages = editor_pages(params)
            .into_iter()
            .map(|(name, members)| {
                let tabs = tabs_for(params, &members);
                let placed = members
                    .iter()
                    .map(|&i| {
                        let m = &params[i].module;
                        if tabs.is_empty() {
                            Placed { param: i, tab: None, heading: after(m, 1) }
                        } else {
                            let at = tabs.iter().position(|t| t == level(m, 1));
                            Placed { param: i, tab: at, heading: after(m, 2) }
                        }
                    })
                    .collect();
                Page { name, tabs, params: placed, indexed: None }
            })
            .collect();
        Model { pages }
    }

    /// From what a plugin declares (ui.h): the pages are the entries with parent 0 in order, a page's tabs the
    /// entries naming it, in order; an entry with an empty name, an id used before, or (a tab) a parent that is not
    /// a page is left out; at most 16 pages and 16 tabs under each. A parameter is on the entry its `declared_tab`
    /// names (a page: above the tabs); one that names none, or one that was left out, goes where its module path
    /// puts it: on the declared page of its first module's name, on that page's tab of its second module's name
    /// where there is one (else above the tabs), or on a page of its own after the declared ones. A page left with
    /// no parameters, and a tab left with none, are dropped; a page left with fewer than two tabs has none.
    pub fn declared(params: &[Param], d: &Declared) -> Model {
        // Entries kept: (id, name) of the pages, then (id, page, name) of the tabs.
        let mut seen: Vec<u32> = Vec::new();
        let mut pages: Vec<(u32, String)> = Vec::new();
        for (id, parent, name) in &d.entries {
            if *parent == 0 && *id != 0 && !name.is_empty() && !seen.contains(id) && pages.len() < MOST {
                seen.push(*id);
                pages.push((*id, name.clone()));
            }
        }
        let mut tabs: Vec<(u32, usize, String)> = Vec::new();
        for (id, parent, name) in &d.entries {
            let Some(page) = pages.iter().position(|p| p.0 == *parent) else { continue };
            if *parent != 0 && *id != 0 && !name.is_empty() && !seen.contains(id) && tabs.iter().filter(|t| t.1 == page).count() < MOST {
                seen.push(*id);
                tabs.push((*id, page, name.clone()));
            }
        }
        // Where each parameter goes: a page and a tab (an index among the page's declared tabs).
        let mut at: Vec<(usize, Option<usize>)> = Vec::with_capacity(params.len());
        let mut own: Vec<String> = Vec::new(); // pages made for parameters the declaration did not place
        for p in params {
            let named = p.declared_tab.and_then(|id| {
                if let Some(page) = pages.iter().position(|e| e.0 == id) {
                    return Some((page, None));
                }
                let tab = tabs.iter().find(|t| t.0 == id)?;
                let index = tabs.iter().filter(|t| t.1 == tab.1).position(|t| t.0 == id)?;
                Some((tab.1, Some(index)))
            });
            at.push(named.unwrap_or_else(|| {
                let top = if p.module.is_empty() { "Main" } else { level(&p.module, 0) };
                let page = match pages.iter().position(|e| e.1 == top) {
                    Some(page) => page,
                    None => {
                        let k = own.iter().position(|n| n == top).unwrap_or_else(|| {
                            own.push(top.to_string());
                            own.len() - 1
                        });
                        pages.len() + k
                    }
                };
                let tab = tabs.iter().filter(|t| t.1 == page).position(|t| t.2 == level(&p.module, 1));
                (page, tab)
            }));
        }
        let mut model = Model::default();
        for k in 0..pages.len() + own.len() {
            let name = pages.get(k).map_or_else(|| own[k - pages.len()].clone(), |p| p.1.clone());
            let names: Vec<&String> = tabs.iter().filter(|t| t.1 == k).map(|t| &t.2).collect();
            // The tabs that have a parameter on them, in order; two or more, else none.
            let used: Vec<usize> = (0..names.len()).filter(|&t| at.contains(&(k, Some(t)))).collect();
            let keep = if used.len() >= 2 { used } else { vec![] };
            let page_tabs: Vec<String> = keep.iter().map(|&t| names[t].clone()).collect();
            let params_on: Vec<Placed> = params
                .iter()
                .enumerate()
                .filter(|(i, _)| at[*i].0 == k)
                .map(|(i, p)| {
                    let tab = at[i].1.and_then(|t| keep.iter().position(|&u| u == t));
                    Placed { param: i, tab, heading: heading_below(&p.module, &name, tab.map(|t| page_tabs[t].as_str())) }
                })
                .collect();
            if !params_on.is_empty() {
                model.pages.push(Page { name, tabs: page_tabs, params: params_on, indexed: None });
            }
        }
        model
    }

    /// What the screen steps through, in order: a page without tabs once, a page with tabs once per tab.
    pub fn entries(&self) -> Vec<Entry> {
        self.pages
            .iter()
            .enumerate()
            .flat_map(|(page, p)| if p.tabs.is_empty() { vec![Entry { page, tab: None }] } else { (0..p.tabs.len()).map(|t| Entry { page, tab: Some(t) }).collect() })
            .collect()
    }

    /// The headings and parameters of a page (and the tab of it open): first those above the tabs, then the
    /// tab's, each heading's parameters together in order of first appearance.
    pub fn groups(&self, page: usize, tab: Option<usize>) -> Vec<(String, Vec<usize>)> {
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        let Some(p) = self.pages.get(page) else { return groups };
        let above = p.params.iter().filter(|x| !p.tabs.is_empty() && x.tab.is_none());
        let on = p.params.iter().filter(|x| p.tabs.is_empty() || (x.tab.is_some() && x.tab == tab));
        for x in above.chain(on) {
            match groups.iter_mut().find(|g| g.0 == x.heading) {
                Some(g) => g.1.push(x.param),
                None => groups.push((x.heading.clone(), vec![x.param])),
            }
        }
        groups
    }
}

/// The view a page's tabs are the indexes of, with their names, when they are (`Model::fold_indexed`).
pub fn indexed_view(params: &[Param], page: &Page) -> Option<Indexed> {
    if page.tabs.len() < 2 {
        return None;
    }
    let mut stem: Option<String> = None;
    let mut view: Option<u64> = None;
    let mut names: Vec<(u64, String)> = Vec::new();
    let mut numbered_tabs = 0;
    for (t, name) in page.tabs.iter().enumerate() {
        let on: Vec<&Param> = page.params.iter().filter(|x| x.tab == Some(t)).map(|x| &params[x.param]).collect();
        if on.is_empty() {
            return None;
        }
        if name.is_empty() {
            continue; // the page's own tab ("General"): kept above where the tabs were
        }
        let (words, number) = crate::params::numbered(name);
        if number < 1 || stem.as_ref().is_some_and(|s| *s != words) || names.iter().any(|n| n.0 == number as u64) {
            return None;
        }
        stem = Some(words);
        for p in &on {
            if p.view == 0 || p.index != number as u64 || view.is_some_and(|v| v != p.view) {
                return None;
            }
            view = Some(p.view);
        }
        names.push((number as u64, name.clone()));
        numbered_tabs += 1;
    }
    let view = view?;
    let members: Vec<usize> = page.params.iter().map(|x| x.param).collect();
    (numbered_tabs >= 2 && crate::grid::master_view(params, &members) == Some(view)).then_some(Indexed { view, names })
}

/// What is left of a module path below a declared page and tab: the page's name and the tab's name drop off its
/// front where they are its first levels.
fn heading_below(module: &str, page: &str, tab: Option<&str>) -> String {
    let mut skip = 0;
    if level(module, 0) == page {
        skip = 1;
        if tab.is_some_and(|t| level(module, 1) == t) {
            skip = 2;
        }
    }
    after(module, skip)
}

/// The names of one row or rail of tabs without what they all share: `heading` is the shared leading and trailing
/// words (shown once, where the tabs are), `shorts` what differs, one per name in order. The full names stay the
/// tabs' names everywhere else (the path in the top bar, what a screen reader says).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Abbreviated {
    pub heading: String,
    pub shorts: Vec<String>,
}

/// The words every name begins with, and those every name ends with, taken off the names: "Operator 1" to
/// "Operator 8" are "1" to "8" under the heading "Operator". Two or more names are needed, a name keeps at least one
/// word, and what is left must still tell the names apart (else nothing is taken off). An empty name is "General".
pub fn abbreviate(names: &[String]) -> Abbreviated {
    let whole = || Abbreviated { heading: String::new(), shorts: names.iter().map(|n| if n.is_empty() { "General".to_string() } else { n.clone() }).collect() };
    let words: Vec<Vec<&str>> = names.iter().map(|n| n.split_whitespace().collect()).collect();
    let least = words.iter().map(Vec::len).min().unwrap_or(0);
    if names.len() < 2 || least < 2 {
        return whole();
    }
    let same_from_front = |k: usize| words.iter().all(|w| w[k] == words[0][k]);
    let same_from_back = |k: usize| words.iter().all(|w| w[w.len() - 1 - k] == words[0][words[0].len() - 1 - k]);
    let lead = (0..least - 1).take_while(|&k| same_from_front(k)).count();
    let trail = (0..least - 1 - lead).take_while(|&k| same_from_back(k)).count();
    if lead + trail == 0 {
        return whole();
    }
    let shorts: Vec<String> = words.iter().map(|w| w[lead..w.len() - trail].join(" ")).collect();
    if (1..shorts.len()).any(|i| shorts[..i].contains(&shorts[i])) {
        return whole();
    }
    let first = words[0][..lead].join(" ");
    let last = words[0][words[0].len() - trail..].join(" ");
    let heading = match (lead, trail) {
        (0, _) => last,
        (_, 0) => first,
        _ => format!("{first} \u{2026} {last}"),
    };
    Abbreviated { heading, shorts }
}

/// A label on two lines: split at the space that leaves the two the most even, or on one line (the second empty)
/// where there is no space.
pub fn two_lines(label: &str) -> (String, String) {
    let chars: Vec<char> = label.chars().collect();
    let at = chars.iter().enumerate().filter(|(_, c)| **c == ' ').min_by_key(|(i, _)| (*i as i64 - (chars.len() - 1 - *i) as i64).abs()).map(|(i, _)| i);
    match at {
        Some(i) => (chars[..i].iter().collect(), chars[i + 1..].iter().collect()),
        None => (label.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Param;
    use serde_json::json;

    fn placed(param: usize, tab: Option<usize>, heading: &str) -> Placed {
        Placed { param, tab, heading: heading.into() }
    }

    /// A page whose tabs are exactly the indexes of one view (all of its parameters are of that view, each on the
    /// tab carrying its index, with an algorithm field for the view's overview).
    fn voices(params: &[Param], tabs: &[&str]) -> Model {
        let params_on: Vec<Placed> = params.iter().enumerate().map(|(i, _)| placed(i, Some(i), "")).collect();
        Model { pages: vec![Page { name: "Voices".into(), tabs: tabs.iter().map(|t| t.to_string()).collect(), params: params_on, indexed: None }] }
    }

    /// The model folds the two tabs (each the whole of one view's parameters, named "N", carrying index N) into a
    /// master and a detail; the tabs go and every parameter sits above.
    #[test]
    fn a_pages_tabs_that_are_one_views_indexes_fold_into_a_master_and_detail() {
        let params = vec![
            Param { id: 0, view: 7, index: 1, field: "algorithm".into(), ..Default::default() },
            Param { id: 1, view: 7, index: 2, field: "algorithm".into(), ..Default::default() },
        ];
        let mut model = voices(&params, &["Voices 1", "Voices 2"]);
        model.fold_indexed(&params);
        assert_eq!(model.pages[0].tabs, Vec::<String>::new());
        assert!(model.pages[0].params.iter().all(|x| x.tab.is_none()), "a folded view's parameters sit above the tabs");
        assert_eq!(model.pages[0].indexed, Some(Indexed { view: 7, names: vec![(1, "Voices 1".into()), (2, "Voices 2".into())] }));
    }

    /// A page whose own unnamed tab ("General", see `Declared`) is among the others still folds when two named
    /// tabs are whole views; the unnamed tab's parameters have no index and stay above like the rest.
    #[test]
    fn the_pages_own_unnamed_tab_stays_above_a_folded_view() {
        let params = vec![
            Param { id: 0, view: 9, index: 1, field: "algorithm".into(), ..Default::default() },
            Param { id: 1, view: 9, index: 2, field: "algorithm".into(), ..Default::default() },
            Param { id: 2, ..Default::default() },
        ];
        let mut model = voices(&params, &["Voices 1", "Voices 2", ""]);
        model.fold_indexed(&params);
        assert_eq!(model.pages[0].indexed, Some(Indexed { view: 9, names: vec![(1, "Voices 1".into()), (2, "Voices 2".into())] }));
        assert!(model.pages[0].params.iter().all(|x| x.tab.is_none()));
    }

    /// Tabs that are not each a whole of one view stay tabs: a parameter carrying the wrong index, parameters of
    /// different views, and a tab whose name has no number.
    #[test]
    fn tabs_that_are_not_one_views_indexes_stay_tabs() {
        // The second tab's parameter carries the wrong index.
        let wrong_index = vec![
            Param { id: 0, view: 7, index: 1, field: "algorithm".into(), ..Default::default() },
            Param { id: 1, view: 7, index: 5, field: "algorithm".into(), ..Default::default() },
        ];
        let mut a = voices(&wrong_index, &["Voices 1", "Voices 2"]);
        a.fold_indexed(&wrong_index);
        assert_eq!(a.pages[0].indexed, None);
        assert_eq!(a.pages[0].tabs, ["Voices 1", "Voices 2"]);
        // The two tabs carry different views.
        let mixed_views = vec![
            Param { id: 0, view: 7, index: 1, field: "algorithm".into(), ..Default::default() },
            Param { id: 1, view: 8, index: 2, field: "algorithm".into(), ..Default::default() },
        ];
        let mut b = voices(&mixed_views, &["Voices 1", "Voices 2"]);
        b.fold_indexed(&mixed_views);
        assert_eq!(b.pages[0].indexed, None);
        // A tab whose name has no trailing number.
        let unnamed = vec![
            Param { id: 0, view: 7, index: 1, field: "algorithm".into(), ..Default::default() },
            Param { id: 1, view: 7, index: 2, field: "algorithm".into(), ..Default::default() },
        ];
        let mut c = voices(&unnamed, &["Voices 1", "Mixed"]);
        c.fold_indexed(&unnamed);
        assert_eq!(c.pages[0].indexed, None);
    }

    /// `declared_in` reads nothing when a `params.list` answer declares no tabs: `uiTabs`
    /// absent, not an array or empty.
    #[test]
    fn declared_in_reads_ui_tabs_or_none() {
        assert_eq!(declared_in(&json!({})), None);
        assert_eq!(declared_in(&json!({"parameters": []})), None);
        assert_eq!(declared_in(&json!({"uiTabs": []})), None);
        assert_eq!(declared_in(&json!({"uiTabs": "not an array"})), None);
        let two = json!({"uiTabs": [{"id": 2, "parent": 1, "name": "Env"}, {"id": 3, "parent": 1, "name": "Op"}]});
        assert_eq!(declared_in(&two), Some(Declared { entries: vec![(2, 1, "Env".into()), (3, 1, "Op".into())] }));
    }

    /// A page's groups read those above the tabs first, then the tab's, each heading's parameters together; a
    /// page out of range has nothing.
    #[test]
    fn groups_put_what_is_above_a_tab_first_and_a_page_out_of_range_has_nothing() {
        let model = Model { pages: vec![Page {
            name: "Env".into(),
            tabs: vec!["A".into(), "B".into()],
            params: vec![placed(0, None, "h1"), placed(1, Some(0), "h1"), placed(2, Some(1), "h2"), placed(3, Some(0), "h2")],
            indexed: None,
        }] };
        assert_eq!(model.groups(0, Some(0)), vec![("h1".to_string(), vec![0, 1]), ("h2".to_string(), vec![3])]);
        assert_eq!(model.groups(0, Some(1)), vec![("h1".to_string(), vec![0]), ("h2".to_string(), vec![2])]);
        assert_eq!(model.groups(9, None), Vec::<(String, Vec<usize>)>::new());
    }

    /// The heading of a parameter is its module path below the page it is on and, where its tab's name is the
    /// path's second level, below that too; a page or tab the path does not start with leaves the path alone.
    #[test]
    fn a_parameters_heading_is_its_module_path_below_the_page_and_tab() {
        assert_eq!(heading_below("Env/A/Beat", "Env", Some("A")), "Beat");
        assert_eq!(heading_below("Env/A/Beat", "Env", None), "A/Beat");
        assert_eq!(heading_below("Env/Other", "Env", None), "Other");
        assert_eq!(heading_below("Env/Other/Odd", "Env", Some("A")), "Other/Odd");
        assert_eq!(heading_below("Amp/Out", "Env", None), "Amp/Out");
    }

    /// What a row of names all share is taken off whole words, so a shared leading word is still a full word
    /// when the names are not ASCII.
    #[test]
    fn abbreviate_takes_off_a_shared_word_whole_across_unicode() {
        let row = vec!["Громкость 1".to_string(), "Громкость 2".to_string()];
        let a = abbreviate(&row);
        assert_eq!((a.heading.as_str(), a.shorts), ("Громкость", vec!["1".to_string(), "2".to_string()]));
    }
}
