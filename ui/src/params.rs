// SPDX-License-Identifier: AGPL-3.0-or-later
//! A plugin's parameter pages, host-drawn (the JUCE ParameterGrid, hosts/standalone/ParameterGrid.cpp,
//! RFC 0019's generated editor): this is how most plugins are edited. The parameters (`params.list`)
//! without the hidden ones become pages, one per top-level CLAP module path ("Main" for parameters with
//! none; numbered pages of one kind together, Operator 1 to 8); a plugin of more than 24 parameters
//! that gives no module paths gets modules from its names' leading words; a page of more than 24 has a
//! tab per second-level module. The tiles of a page (or tab) are in the plugin's priority order
//! (`oscairn.ui`: numbered first, then the rest in its order), each with its name and its value as the
//! plugin writes it (`params.text`); a read-only parameter shows its value only, one of two values is a
//! switch, any other a value the dial changes.
//!
//! Without touch, two panes: the left (the left knob, or the dial) is the tiles' list: a press on a
//! value starts changing it (a gesture begins), the dial then sets it, each step sent (`params.set`),
//! and a press or Cancel ends the change (the gesture ends); a press on a switch flips it. The right
//! knob (pane 1) steps through the pages, tabs included, listed in the panel; its arrows do on touch.
//! Values follow the engine (`params.changed`, subscribed while this screen is shown), except the
//! one being changed.
//!
//! Not here yet: the rich views (envelopes, key and velocity maps, FM operators, LFOs, filters: every
//! one of their parameters is a tile), joined groups, the search, a plugin's own view (a disabled entry
//! in the Chain screen's actions says it is not available in this shell yet).

use crate::browser::{Browser, Nav};
use crate::nav::Action;
use crate::{InfoLine, SettingData, Shell};
use serde_json::{json, Value};
use std::collections::HashMap;

/// A page with more parameters than this has tabs.
const TABBED_ABOVE: usize = 24;
const FLAG_HIDDEN: u64 = 1 << 2;
const FLAG_READONLY: u64 = 1 << 3;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    List,
    Values,
    /// A change; its refusal is said.
    Set,
    Gesture,
    /// params.text for this parameter and value.
    Text(u32, u64),
}

pub type Out = (Request, &'static str, Value);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Back,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Param {
    pub id: u32,
    pub name: String,
    pub module: String,
    pub min: f64,
    pub max: f64,
    pub stepped: bool,
    pub flags: u64,
    pub priority: u64,
}

fn parse(list: &Value) -> Vec<Param> {
    list["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| Param {
            id: p["id"].as_u64().unwrap_or(0) as u32,
            name: p["name"].as_str().unwrap_or_default().into(),
            module: p["module"].as_str().unwrap_or_default().into(),
            min: p["minimum"].as_f64().unwrap_or(0.),
            max: p["maximum"].as_f64().unwrap_or(1.),
            stepped: p["stepped"].as_bool().unwrap_or(false),
            flags: p["flags"].as_u64().unwrap_or(0),
            priority: p["priority"].as_u64().unwrap_or(0),
        })
        .collect()
}

/// The nth level of a module path, or "".
fn level(module: &str, n: usize) -> &str {
    module.split('/').nth(n).filter(|_| !module.is_empty()).unwrap_or("")
}

/// A name's leading word with the number after it when it has one ("Step 6"); empty when nothing would be left.
fn lead(name: &str) -> String {
    let Some(end) = name.find(' ') else { return String::new() };
    if let Some(next) = name[end + 1..].find(' ').map(|n| n + end + 1) {
        if name[end + 1..next].chars().all(|c| c.is_ascii_digit()) && next > end + 1 {
            return name[..next].to_string();
        }
    }
    name[..end].to_string()
}

/// The parameters an editor shows: hidden ones left out; and for a plugin of more than 24 that gives no
/// module paths, modules from its names' leading words, in module order, as `editorParameters`.
pub fn editor_parameters(mut params: Vec<Param>) -> Vec<Param> {
    params.retain(|p| p.flags & FLAG_HIDDEN == 0);
    if params.len() <= TABBED_ABOVE || params.iter().any(|p| !p.module.is_empty()) {
        return params;
    }
    for depth in 0..2 {
        let mut count: HashMap<String, usize> = HashMap::new();
        let mut page: HashMap<String, usize> = HashMap::new();
        for p in &params {
            *page.entry(level(&p.module, 0).to_string()).or_default() += 1;
            let l = lead(&p.name);
            if !l.is_empty() {
                *count.entry(format!("{}/{}", p.module, l)).or_default() += 1;
            }
        }
        for p in &mut params {
            let l = lead(&p.name);
            if l.is_empty() || count[&format!("{}/{}", p.module, l)] < 2 || (depth == 1 && (p.module.is_empty() || page[level(&p.module, 0)] <= TABBED_ABOVE)) {
                continue;
            }
            p.module = if p.module.is_empty() { l.clone() } else { format!("{}/{}", p.module, l) };
            p.name = p.name[l.len() + 1..].to_string();
        }
    }
    params.sort_by_key(|p| natural(&p.module));
    params
}

/// A natural order: runs of digits compare as numbers.
fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut digits = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push((std::mem::take(&mut word), digits.parse().unwrap_or(0)));
                digits.clear();
            }
            word.push(c);
        }
    }
    out.push((word, digits.parse().unwrap_or(0)));
    out
}

/// "Operator 12" is ("Operator", 12); a name without a trailing number has none.
fn numbered(name: &str) -> (String, i64) {
    let end = name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if end == name.len() || end == 0 || !name[..end].ends_with(' ') {
        return (name.to_string(), -1);
    }
    (name[..end - 1].to_string(), name[end..].parse().unwrap_or(-1))
}

/// A page per top-level module in the plugin's order, numbered pages of one kind together in number order.
pub fn editor_pages(params: &[Param]) -> Vec<(String, Vec<usize>)> {
    let mut pages: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, p) in params.iter().enumerate() {
        let top = if p.module.is_empty() { "Main".to_string() } else { level(&p.module, 0).to_string() };
        match pages.iter_mut().find(|(n, _)| *n == top) {
            Some(page) => page.1.push(i),
            None => pages.push((top, vec![i])),
        }
    }
    let mut family: HashMap<String, usize> = HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        family.entry(numbered(&p.0).0).or_insert(i);
    }
    pages.sort_by_key(|(n, _)| {
        let (kind, number) = numbered(n);
        (family[&kind], number)
    });
    pages
}

/// The tabs of a page of more than 24 parameters: its second-level modules, when there are two or more.
fn tabs_for(params: &[Param], page: &[usize]) -> Vec<String> {
    if page.len() <= TABBED_ABOVE {
        return vec![];
    }
    let mut names: Vec<String> = Vec::new();
    for &i in page {
        let n = level(&params[i].module, 1).to_string();
        if !names.contains(&n) {
            names.push(n);
        }
    }
    if names.len() < 2 {
        names.clear();
    }
    names
}

/// The plugin's priority order: numbered first, then the rest in its order.
pub fn by_priority(params: &[Param], indices: &mut [usize]) {
    indices.sort_by_key(|&i| if params[i].priority == 0 { u64::MAX } else { params[i].priority });
}

/// A value without trailing zeros ("0.0100000" is "0.01"), as the JUCE tidyNumber.
fn tidy(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub struct Screen {
    b: Browser,
    node: u64,
    name: String,
    params: Vec<Param>,
    pages: Vec<(String, Vec<usize>)>,
    /// What the right knob steps through: a page, or a tab of one.
    entries: Vec<(usize, Option<String>)>,
    entry: usize,
    values: HashMap<u32, f64>,
    /// The plugin's text for a parameter's value, by value's bits.
    texts: HashMap<(u32, u64), String>,
    /// The parameter whose gesture is open (the dial has it).
    gesture: Option<u32>,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen { b: Browser::new(ui), node: 0, name: String::new(), params: vec![], pages: vec![], entries: vec![], entry: 0, values: HashMap::new(), texts: HashMap::new(), gesture: None, out: vec![] }
    }

    pub fn open(&mut self, node: u64, name: &str) {
        self.node = node;
        self.name = name.into();
        self.params.clear();
        self.pages.clear();
        self.entries.clear();
        self.entry = 0;
        self.values.clear();
        self.texts.clear();
        self.gesture = None;
        self.b.reset();
        self.b.say("");
        self.out.push((Request::List, "params.list", json!({"node": node})));
        self.show();
    }

    fn tiles(&self) -> Vec<usize> {
        let Some((page, tab)) = self.entries.get(self.entry) else { return vec![] };
        let mut tiles: Vec<usize> = self.pages[*page].1.iter().copied().filter(|&i| tab.as_ref().is_none_or(|t| level(&self.params[i].module, 1) == t)).collect();
        by_priority(&self.params, &mut tiles);
        tiles
    }

    fn ask_values(&mut self) {
        let values: Vec<Value> = self.tiles().iter().map(|&i| json!({"node": self.node, "param": self.params[i].id})).collect();
        if !values.is_empty() {
            self.out.push((Request::Values, "params.get", json!({"values": values})));
        }
    }

    /// params.list's answer: the pages, and the values of the first.
    fn listed(&mut self, list: &Value) {
        self.params = editor_parameters(parse(list));
        self.pages = editor_pages(&self.params);
        self.entries = self
            .pages
            .iter()
            .enumerate()
            .flat_map(|(i, (_, members))| {
                let tabs = tabs_for(&self.params, members);
                if tabs.is_empty() {
                    vec![(i, None)]
                } else {
                    tabs.into_iter().map(|t| (i, Some(t))).collect()
                }
            })
            .collect();
        self.entry = 0;
        self.ask_values();
    }

    pub fn reply(&mut self, request: Request, result: Result<Value, crate::client::Error>) -> bool {
        match (request, result) {
            (Request::List, Ok(r)) => self.listed(&r),
            (Request::Values, Ok(r)) => {
                for v in r["values"].as_array().into_iter().flatten() {
                    if let (Some(p), Some(x)) = (v["param"].as_u64(), v["value"].as_f64()) {
                        self.value_arrived(p as u32, x);
                    }
                }
            }
            (Request::Text(p, bits), Ok(r)) => {
                if let Some(t) = r["text"].as_str() {
                    self.texts.insert((p, bits), t.to_string());
                }
            }
            (Request::Set | Request::List, Err(e)) if e.code != 0 => {
                self.b.say(&format!("The engine refused: {}", e.message));
                self.show();
                return true;
            }
            _ => {}
        }
        self.show();
        false
    }

    /// A value read or pushed: kept, its text asked for when not known, but not over the one the dial has.
    fn value_arrived(&mut self, param: u32, value: f64) {
        if self.gesture == Some(param) {
            return;
        }
        self.values.insert(param, value);
        self.ask_text(param, value);
    }

    fn ask_text(&mut self, param: u32, value: f64) {
        let key = (param, value.to_bits());
        if !self.texts.contains_key(&key) {
            self.out.push((Request::Text(param, value.to_bits()), "params.text", json!({"node": self.node, "param": param, "value": value})));
        }
    }

    /// params.changed: the values that moved, for this plugin.
    pub fn changed(&mut self, params: &Value) {
        for c in params["changes"].as_array().into_iter().flatten() {
            if c["node"].as_u64() == Some(self.node) && c["kind"] == "value" {
                if let (Some(p), Some(v)) = (c["param"].as_u64(), c["value"].as_f64()) {
                    self.value_arrived(p as u32, v);
                }
            }
        }
        self.show();
    }

    fn value_text(&self, p: &Param) -> String {
        let v = self.values.get(&p.id).copied();
        match v {
            Some(v) => self.texts.get(&(p.id, v.to_bits())).cloned().unwrap_or_else(|| tidy(v)),
            None => String::new(),
        }
    }

    fn readonly(p: &Param) -> bool {
        p.flags & FLAG_READONLY != 0
    }

    /// Two values only: a switch.
    fn switch(p: &Param) -> bool {
        p.stepped && (p.max - p.min - 1.).abs() < 1e-9
    }

    fn rows(&self) -> Vec<SettingData> {
        self.tiles()
            .into_iter()
            .map(|i| {
                let p = &self.params[i];
                let kind = if Self::readonly(p) {
                    4
                } else if Self::switch(p) {
                    1
                } else {
                    0
                };
                let on = self.values.get(&p.id).is_some_and(|v| *v > p.min);
                SettingData { label: p.name.as_str().into(), value: self.value_text(p).into(), kind, on: kind == 1 && on, switch: kind == 1, ..Default::default() }
            })
            .collect()
    }

    fn info(&self) -> Vec<InfoLine> {
        if self.entries.is_empty() {
            return vec![InfoLine { head: true, text: "Pages".into() }, InfoLine { head: false, text: if self.params.is_empty() { "Reading the parameters..." } else { "No parameters" }.into() }];
        }
        let mut lines = vec![InfoLine { head: true, text: "Pages: right knob".into() }];
        for (i, (page, tab)) in self.entries.iter().enumerate() {
            let name = match tab {
                Some(t) => format!("{} › {}", self.pages[*page].0, if t.is_empty() { "General" } else { t }),
                None => self.pages[*page].0.clone(),
            };
            lines.push(InfoLine { head: i == self.entry, text: if i == self.entry { format!("● {name}") } else { name }.into() });
        }
        lines
    }

    fn show(&mut self) {
        let rows = self.rows();
        let info = self.info();
        // The panel follows the page chosen.
        self.b.info_cursor.at = None;
        let per = self.b.per_info();
        self.b.info_cursor.first = (self.entry + 1) / per.max(1) * per.max(1);
        self.b.show(&self.name, "Parameters", "Parameters", rows, info);
    }

    fn end_gesture(&mut self) {
        if let Some(p) = self.gesture.take() {
            self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": p, "begin": false})));
        }
        self.b.editing = false;
    }

    /// The value under the cursor stepped `by`: a step of the stepped values, or of a hundredth of the range.
    fn dial(&mut self, at: usize, by: i64) {
        let Some(&i) = self.tiles().get(at) else { return };
        let p = self.params[i].clone();
        let now = self.values.get(&p.id).copied().unwrap_or(p.min);
        let step = if p.stepped { 1. } else { (p.max - p.min) / 100. };
        let to = (now + step * by as f64).clamp(p.min, p.max);
        let to = if p.stepped { to.round() } else { to };
        if to == now {
            return;
        }
        self.set(&p, to);
    }

    fn set(&mut self, p: &Param, to: f64) {
        self.values.insert(p.id, to);
        self.b.say("");
        self.out.push((Request::Set, "params.set", json!({"values": [{"node": self.node, "param": p.id, "value": to}]})));
        self.ask_text(p.id, to);
    }

    fn press(&mut self, at: usize) {
        let Some(&i) = self.tiles().get(at) else { return };
        let p = self.params[i].clone();
        if Self::readonly(&p) {
            return;
        }
        if Self::switch(&p) {
            // One gesture around the flip.
            let now = self.values.get(&p.id).copied().unwrap_or(p.min);
            let to = if now > p.min { p.min } else { p.max };
            self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": p.id, "begin": true})));
            self.set(&p, to);
            self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": p.id, "begin": false})));
        } else if self.b.editing {
            self.end_gesture();
        } else {
            self.b.editing = true;
            self.gesture = Some(p.id);
            self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": p.id, "begin": true})));
        }
    }

    fn page(&mut self, by: i64) {
        if self.entries.is_empty() {
            return;
        }
        self.end_gesture();
        self.entry = (self.entry as i64 + by).clamp(0, self.entries.len() as i64 - 1) as usize;
        self.b.reset_cursor();
        self.ask_values();
    }

    fn handle(&mut self, nav: Nav) -> Outcome {
        let outcome = match nav {
            Nav::Press(at) => {
                self.press(at);
                Outcome::Stay
            }
            Nav::Dial(at, by) => {
                self.dial(at, by);
                Outcome::Stay
            }
            Nav::Pane(by) => {
                self.page(by.signum());
                Outcome::Stay
            }
            Nav::Back => Outcome::Back,
            Nav::None => {
                // Cancel ended a change: the gesture with it.
                if !self.b.editing {
                    self.end_gesture();
                }
                Outcome::Stay
            }
        };
        self.show();
        outcome
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        let nav = self.b.nav(action);
        self.handle(nav)
    }

    pub fn tap(&mut self, row: i32) -> Outcome {
        let nav = self.b.tap(row);
        self.handle(nav)
    }

    /// The panel's up or down button: the page before or after.
    pub fn scroll_info(&mut self, by: i32) {
        self.page(by as i64);
        self.show();
    }

    /// - or + on a row: that value stepped, in a gesture of its own.
    pub fn step_row(&mut self, row: i32, by: i32) {
        let at = self.b.index(row);
        self.b.cursor = self.b.cursor.to(at);
        if let Some(&i) = self.tiles().get(at) {
            let p = self.params[i].clone();
            if !Self::readonly(&p) && !Self::switch(&p) {
                self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": p.id, "begin": true})));
                self.dial(at, by as i64);
                self.out.push((Request::Gesture, "params.gesture", json!({"node": self.node, "param": p.id, "begin": false})));
            }
        }
        self.show();
    }

    pub fn hidden(&mut self) {
        self.end_gesture();
        self.b.hidden();
        self.show();
    }

    pub fn shown(&mut self) {
        self.b.install();
        self.show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: u32, name: &str, module: &str, priority: u64) -> Param {
        Param { id, name: name.into(), module: module.into(), max: 1., priority, ..Default::default() }
    }

    #[test]
    fn pages_follow_the_modules_and_numbered_ones_sit_together() {
        let params = vec![p(0, "Gain", "", 0), p(1, "Level", "Operator 2", 0), p(2, "Level", "Filter/Main", 0), p(3, "Level", "Operator 1", 0), p(4, "Mix", "Operator 1/Env", 0)];
        let pages: Vec<(String, Vec<usize>)> = editor_pages(&params);
        let expected: Vec<(String, Vec<usize>)> = vec![("Main".into(), vec![0]), ("Operator 1".into(), vec![3]), ("Operator 2".into(), vec![1]), ("Filter".into(), vec![2])];
        // "Operator 1/Env" is on the page of its top-level module, "Operator 1".
        assert_eq!(pages.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["Main", "Operator 1", "Operator 2", "Filter"]);
        assert_eq!(pages[1].1, vec![3, 4]);
        assert_eq!(pages[0], expected[0]);
    }

    #[test]
    fn priorities_first_then_the_plugins_order_and_hidden_ones_go() {
        let mut params = vec![p(0, "A", "", 0), p(1, "B", "", 2), p(2, "C", "", 1), p(3, "D", "", 0)];
        params[3].flags = FLAG_HIDDEN;
        let shown = editor_parameters(params);
        assert_eq!(shown.len(), 3);
        let mut order: Vec<usize> = vec![0, 1, 2];
        by_priority(&shown, &mut order);
        assert_eq!(order, [2, 1, 0]);
    }

    #[test]
    fn a_big_plugin_without_modules_is_grouped_by_the_leading_words_of_its_names() {
        let mut params: Vec<Param> = (0..13).map(|i| p(i, &format!("Osc{} Detune", i % 2), "", 0)).collect();
        params.extend((13..30).map(|i| p(i, &format!("Filter Cutoff {i}"), "", 0)));
        params.push(p(30, "Master", "", 0));
        let shown = editor_parameters(params);
        let modules: std::collections::BTreeSet<_> = shown.iter().map(|p| p.module.clone()).collect();
        assert!(modules.contains("Filter") && modules.contains("Osc0") && modules.contains("Osc1"), "{modules:?}");
        assert!(shown.iter().any(|p| p.module == "Osc0" && p.name == "Detune"), "the leading word moves into the module");
        assert!(shown.iter().any(|p| p.module.is_empty() && p.name == "Master"), "a word that leads one name only stays in it");
    }

    #[test]
    fn tabs_only_on_a_page_of_more_than_24() {
        let big: Vec<Param> = (0..30).map(|i| p(i, "P", if i < 15 { "Osc/A" } else { "Osc/B" }, 0)).collect();
        let page: Vec<usize> = (0..30).collect();
        assert_eq!(tabs_for(&big, &page), ["A", "B"]);
        assert!(tabs_for(&big, &page[..20]).is_empty());
        assert_eq!(numbered("Operator 12"), ("Operator".to_string(), 12));
        assert_eq!(numbered("Cutoff"), ("Cutoff".to_string(), -1));
        assert_eq!(tidy(0.0100000), "0.01");
    }
}
