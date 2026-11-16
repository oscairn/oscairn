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
        match declared {
            Some(d) => Model::declared(params, d),
            None => Model::inferred(params),
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
                Page { name, tabs, params: placed }
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
                model.pages.push(Page { name, tabs: page_tabs, params: params_on });
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
