// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a device's knobs play, resolved from the Performance as the engine resolves it (RFC 0011's
//! physical controls: core/control/src/physical_controls.cpp `resolveControls`, `knobBanks`, `bankOf`,
//! `groupPositions`, `assignmentSource`, `roleFromName`), for the knob row of Play (hosts/standalone/
//! PlayView.cpp). The engine has no request that returns what a group plays, so the app reads what the
//! engine reads (`graph.get`, `params.list`, `assignments.get`) and resolves it with the same rules, as
//! the JUCE app does with the library; the engine plays the same slots once the app publishes the part
//! and the group's position (`controllers.view`). Only the groups of knobs and encoders are resolved:
//! the knob row's.

use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Notes,
    Screen,
    Assign,
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankKind {
    Assign,
    Filter,
    AmpEg,
    Eq,
    Levels,
    Effects,
}

/// A group of a device's controls (its profile's `controls`, RFC 0024), as `ControlGroup`.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub role: Role,
    pub kind: String,
    pub count: usize,
    pub rows: Vec<(String, BankKind)>,
    pub modes: Vec<String>,
    pub functions: Vec<String>,
    pub paged: bool,
}

impl Group {
    /// A profile's `controls` entry (Profile.cpp `controlGroups`).
    pub fn from_profile(c: &Value) -> Group {
        let kind = |b: &str| match b {
            "filter" => BankKind::Filter,
            "ampEG" => BankKind::AmpEg,
            "eq" => BankKind::Eq,
            "levels" => BankKind::Levels,
            "effects" => BankKind::Effects,
            _ => BankKind::Assign,
        };
        let strings = |k: &str| c[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>();
        Group {
            role: match c["role"].as_str() {
                Some("notes") => Role::Notes,
                Some("screen") => Role::Screen,
                Some("fixed") => Role::Fixed,
                _ => Role::Assign,
            },
            kind: c["kind"].as_str().unwrap_or_default().into(),
            count: c["count"].as_u64().unwrap_or(0) as usize,
            rows: c["rows"].as_array().into_iter().flatten().map(|r| (r["name"].as_str().unwrap_or_default().to_string(), kind(r["bank"].as_str().unwrap_or_default()))).collect(),
            modes: strings("modes"),
            functions: if c["role"] == "fixed" { vec![] } else { strings("functions") },
            paged: c["paging"].as_str().is_some_and(|p| !p.is_empty()),
        }
    }

    fn is_knobs(&self) -> bool {
        self.kind == "knobs" || self.kind == "encoders"
    }
}

/// The group the knob row shows, by its place among the device's groups (PlayView.cpp `knobGroupIndex`):
/// the first screen group of knobs or encoders, else the first assign one.
pub fn knob_group(groups: &[Group]) -> Option<usize> {
    [Role::Screen, Role::Assign].into_iter().find_map(|role| groups.iter().position(|g| g.role == role && g.is_knobs()))
}

/// What a control plays: a parameter, a Performance or part control, a part's level or mute, a scene, or all notes off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    None,
    Parameter { node: u64, param: u32 },
    Control(u32),
    PartLevel(u32),
    PartMute(u32),
    Scene(u32),
    Panic,
}

impl Target {
    /// As `assignments.get` writes a target (and `controllers.view`'s `screen` takes one).
    pub fn json(&self) -> Value {
        match *self {
            Target::None => json!({"kind": "none"}),
            Target::Parameter { node, param } => json!({"kind": "parameter", "node": node, "param": param}),
            Target::Control(id) => json!({"kind": "control", "id": id}),
            Target::PartLevel(id) => json!({"kind": "partLevel", "id": id}),
            Target::PartMute(id) => json!({"kind": "partMute", "id": id}),
            Target::Scene(id) => json!({"kind": "scene", "id": id}),
            Target::Panic => json!({"kind": "panic", "id": 0}),
        }
    }

    pub fn from_json(a: &Value) -> Target {
        let id = a["id"].as_u64().unwrap_or(0) as u32;
        match a["kind"].as_str() {
            Some("parameter") => Target::Parameter { node: a["node"].as_u64().unwrap_or(0), param: a["param"].as_u64().unwrap_or(0) as u32 },
            Some("control") => Target::Control(id),
            Some("partLevel") => Target::PartLevel(id),
            Some("partMute") => Target::PartMute(id),
            Some("scene") => Target::Scene(id),
            Some("panic") => Target::Panic,
            _ => Target::None,
        }
    }
}

/// Which layer a control's target came from (RFC 0011), the first to give one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layer {
    #[default]
    None,
    Override,
    Sound,
    Role,
    Convention,
    Effects,
}

/// A Performance override (`assignments.get`): the control's identity ("knob:1"), its target, and the part
/// whose Sound it belongs to (0: the Performance's).
#[derive(Clone, Debug, PartialEq)]
pub struct Override {
    pub control: String,
    pub target: Target,
    pub part: u32,
}

impl Override {
    pub fn parse(result: &Value) -> Vec<Override> {
        result["assignments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| Override { control: a["control"].as_str().unwrap_or_default().into(), target: Target::from_json(a), part: a["part"].as_u64().unwrap_or(0) as u32 })
            .collect()
    }
}

/// A parameter as resolution sees it: its role (empty: none declared) and priority (1 first; 0 none).
#[derive(Clone, Debug, PartialEq)]
pub struct SParam {
    pub id: u32,
    pub name: String,
    pub role: String,
    pub priority: u64,
    pub stepped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SNode {
    pub name: String,
    /// Takes audio in and gives it out; an instrument only gives it.
    pub effect: bool,
    pub instrument: bool,
    pub params: Vec<SParam>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SPart {
    pub id: u32,
    pub name: String,
    /// The chain's nodes in order, each split's lanes one after another.
    pub nodes: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SControl {
    pub id: u32,
    pub name: String,
    pub part: u32,
}

/// What resolution reads (`AssignmentSource`): the Performance's parts, controls, scenes and overrides, and
/// each part's nodes with their parameters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub parts: Vec<SPart>,
    pub controls: Vec<SControl>,
    pub scenes: Vec<(u32, String)>,
    pub nodes: HashMap<u64, SNode>,
    pub overrides: Vec<Override>,
}

/// The role a parameter's name suggests, for plugins that declare none (`roleFromName`): exact names only,
/// and a filter's envelope only by its module.
pub fn role_from_name(name: &str, module: &str) -> &'static str {
    let key = name.to_lowercase();
    let last = module.rsplit('/').next().unwrap_or(module).to_lowercase();
    let key = if last.contains("filter") && matches!(key.as_str(), "attack" | "decay" | "sustain" | "release") { format!("filter {key}") } else { key };
    match key.as_str() {
        "cutoff" | "filter cutoff" | "cutoff frequency" | "filter frequency" => "cutoff",
        "resonance" | "filter resonance" | "reso" => "resonance",
        "filter envelope" | "filter env" | "filter env amount" | "env amount" | "envelope amount" => "filterEnvelope",
        "key tracking" | "filter key tracking" | "key track" => "keyTracking",
        "filter attack" => "filterAttack",
        "filter decay" => "filterDecay",
        "filter sustain" => "filterSustain",
        "filter release" => "filterRelease",
        "attack" | "amp attack" => "attack",
        "decay" | "amp decay" => "decay",
        "sustain" | "amp sustain" => "sustain",
        "release" | "amp release" => "release",
        "hold" => "hold",
        "velocity" => "velocity",
        "level" | "volume" => "level",
        "low gain" => "lowGain",
        "mid gain" => "midGain",
        "high gain" => "highGain",
        "low frequency" => "lowFrequency",
        "mid frequency" => "midFrequency",
        "high frequency" => "highFrequency",
        "mid q" => "midQ",
        "mix" | "dry/wet" | "wet" => "mix",
        "pan" => "pan",
        _ => "",
    }
}

/// The role banks, the same on every instrument (`roleBanks`): each role in order.
const ROLE_BANKS: [(BankKind, &str, [&str; 8]); 4] = [
    (BankKind::Filter, "Filter", ["cutoff", "resonance", "filterEnvelope", "keyTracking", "filterAttack", "filterDecay", "filterSustain", "filterRelease"]),
    (BankKind::AmpEg, "Amp EG", ["attack", "decay", "sustain", "release", "hold", "velocity", "ampKeyTracking", "level"]),
    (BankKind::Eq, "EQ", ["lowGain", "midGain", "highGain", "midFrequency", "lowFrequency", "highFrequency", "midQ", "eqOutput"]),
    (BankKind::Levels, "Levels", ["level1", "level2", "level3", "level4", "level5", "level6", "level7", "level8"]),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Bank {
    pub name: String,
    pub kind: BankKind,
    pub layer: Layer,
    pub slots: Vec<Target>,
    pub labels: Vec<String>,
}

fn blank(name: &str, kind: BankKind, width: usize) -> Bank {
    Bank { name: name.into(), kind, layer: Layer::None, slots: vec![Target::None; width], labels: vec![String::new(); width] }
}

type Item = (Target, String);

/// Items in banks as wide as the group: "Assign 1–6", "Assign 7–12".
fn chunk(banks: &mut Vec<Bank>, name: &str, kind: BankKind, layer: Layer, items: &[Item], width: usize) {
    for (n, items) in items.chunks(width).enumerate() {
        let from = n * width;
        let mut b = blank(&format!("{name} {}\u{2013}{}", from + 1, from + width), kind, width);
        b.layer = layer;
        for (k, (target, label)) in items.iter().enumerate() {
            b.slots[k] = *target;
            b.labels[k] = label.clone();
        }
        banks.push(b);
    }
}

#[derive(Default)]
struct PartBanks {
    assign: Vec<Bank>,
    roles: Vec<Bank>,
    effects: Vec<Bank>,
}

impl Source {
    fn part(&self, id: u32) -> Option<&SPart> {
        self.parts.iter().find(|p| p.id == id)
    }

    fn nodes_of(&self, p: &SPart) -> Vec<(u64, &SNode)> {
        p.nodes.iter().filter_map(|id| self.nodes.get(id).map(|n| (*id, n))).collect()
    }

    /// A node's parameters in the order knobs take them: by priority, then the plugin's order.
    fn by_priority(n: &SNode) -> Vec<&SParam> {
        let mut out: Vec<&SParam> = n.params.iter().collect();
        out.sort_by_key(|p| if p.priority == 0 { u64::MAX } else { p.priority });
        out
    }

    fn parameter(id: u64, n: &SNode, p: &SParam) -> Item {
        (Target::Parameter { node: id, param: p.id }, format!("{}: {}", n.name, p.name))
    }

    /// A part's banks by kind, `width` wide (`banksOf`).
    fn banks_of(&self, part: u32, width: usize) -> PartBanks {
        let mut out = PartBanks::default();
        let width = width.max(1);
        let Some(part_ref) = self.part(part) else {
            // No part selected: the Performance's own controls.
            let items: Vec<Item> = self.controls.iter().filter(|m| m.part == 0).map(|m| (Target::Control(m.id), m.name.clone())).collect();
            chunk(&mut out.assign, "Controls", BankKind::Assign, Layer::Convention, &items, width);
            return out;
        };
        let nodes = self.nodes_of(part_ref);
        // 2. The sound's own assignments: its controls.
        let own: Vec<Item> = self.controls.iter().filter(|m| m.part == part).map(|m| (Target::Control(m.id), m.name.clone())).collect();
        chunk(&mut out.assign, "Assign", BankKind::Assign, Layer::Sound, &own, width);
        // 3. The role banks, from its instruments' roles, each taking as many roles as the group is wide; a
        // role none has is blank, and a bank with none of its roles left out.
        for (kind, name, roles) in ROLE_BANKS {
            let mut b = blank(name, kind, width);
            b.layer = Layer::Role;
            let mut any = false;
            for (k, role) in roles.iter().enumerate().take(width) {
                for (id, n) in &nodes {
                    if n.effect || b.slots[k] != Target::None {
                        continue;
                    }
                    if let Some(p) = n.params.iter().find(|p| p.role == *role) {
                        (b.slots[k], b.labels[k]) = Source::parameter(*id, n, p);
                        any = true;
                    }
                }
            }
            if any {
                out.roles.push(b);
            }
        }
        if out.roles.is_empty() && own.is_empty() {
            // No role bank filled and nothing assigned: two Assign banks from its instrument's first parameters.
            let first = nodes
                .iter()
                .find(|(_, n)| n.instrument && !n.params.is_empty())
                .or_else(|| nodes.iter().find(|(_, n)| !n.effect && !n.params.is_empty()))
                .or_else(|| nodes.iter().find(|(_, n)| !n.params.is_empty()));
            let items: Vec<Item> = first.map(|(id, n)| Source::by_priority(n).into_iter().take(2 * width).map(|p| Source::parameter(*id, n, p)).collect()).unwrap_or_default();
            chunk(&mut out.assign, "Assign", BankKind::Assign, Layer::Role, &items, width);
        }
        // 5. The effects bank: each effect's main control, in chain order: its mix role, else its first
        // continuous parameter by priority (not a stepped one such as an EQ's Mode), else its first.
        let mut effects: Vec<Item> = Vec::new();
        for (id, n) in &nodes {
            if !n.effect || n.params.is_empty() {
                continue;
            }
            let ordered = Source::by_priority(n);
            let main = n.params.iter().find(|p| p.role == "mix").or_else(|| ordered.iter().copied().find(|p| !p.stepped)).unwrap_or(ordered[0]);
            effects.push(Source::parameter(*id, n, main));
        }
        chunk(&mut out.effects, "Effects", BankKind::Effects, Layer::Effects, &effects, width);
        out
    }

    /// The banks a group `width` wide pages through: the part's (Assign, roles, effects), or for part 0 the
    /// Performance's own controls.
    pub fn knob_banks(&self, part: u32, width: usize) -> Vec<Bank> {
        let b = self.banks_of(part, width);
        let mut out = b.assign;
        out.extend(b.roles);
        out.extend(b.effects);
        out
    }

    /// The bank of one kind (the `index`th Assign or Effects bank), blank and named for its kind when
    /// there is none: what a hardware row shows.
    pub fn bank_of(&self, part: u32, width: usize, kind: BankKind, index: usize) -> Bank {
        let b = self.banks_of(part, width);
        let list = match kind {
            BankKind::Assign => b.assign,
            BankKind::Effects => b.effects,
            _ => b.roles,
        };
        if let Some(bank) = list.into_iter().filter(|bank| bank.kind == kind).nth(index) {
            return bank;
        }
        let name = ROLE_BANKS.iter().find(|r| r.0 == kind).map(|r| r.1).unwrap_or(if kind == BankKind::Effects { "Effects" } else { "Assign" });
        blank(name, kind, width.max(1))
    }

    /// A target's name as the views show it ("Piano level", "Basic Synth: Cutoff"); empty when it is gone.
    pub fn name_of(&self, t: &Target) -> String {
        match *t {
            Target::None => String::new(),
            Target::Parameter { node, param } => self.nodes.get(&node).and_then(|n| n.params.iter().find(|p| p.id == param).map(|p| format!("{}: {}", n.name, p.name))).unwrap_or_default(),
            Target::Control(id) => self.controls.iter().find(|m| m.id == id).map(|m| m.name.clone()).unwrap_or_default(),
            Target::PartLevel(id) => self.part(id).map(|p| format!("{} level", p.name)).unwrap_or_default(),
            Target::PartMute(id) => self.part(id).map(|p| format!("{} mute", p.name)).unwrap_or_default(),
            Target::Scene(id) => self.scenes.iter().find(|s| s.0 == id).map(|s| s.1.clone()).unwrap_or_default(),
            Target::Panic => "Panic".into(),
        }
    }

    /// How many positions a group has: its pages, rows or modes (1 for the rest), with no editor page shown.
    pub fn positions(&self, g: &Group, part: u32) -> usize {
        if g.role != Role::Screen && g.role != Role::Assign {
            return 1;
        }
        if g.kind == "faders" {
            return g.modes.len().max(1);
        }
        if g.kind == "buttons" || !g.functions.is_empty() {
            return 1;
        }
        if !g.rows.is_empty() {
            return g.rows.len();
        }
        if !g.paged && !g.is_knobs() {
            return 1;
        }
        self.knob_banks(part, g.count).len().max(1)
    }

    /// What group `which` of `groups` plays at `position` with `part` selected: a slot per control, as
    /// `resolveControls` gives them for a group of knobs or encoders. Nothing for other groups.
    pub fn resolve(&self, groups: &[Group], which: usize, part: u32, position: usize) -> Vec<Slot> {
        let Some(g) = groups.get(which).filter(|g| matches!(g.role, Role::Screen | Role::Assign) && g.is_knobs()) else { return vec![] };
        let screen = g.role == Role::Screen;
        let mut b = blank("", BankKind::Assign, g.count.max(1));
        if !g.functions.is_empty() {
            // Passed through: nothing until assigned.
        } else if !g.rows.is_empty() {
            // A hardware row: its bank, the Nth of its kind for the Nth row naming that kind.
            let row = position % g.rows.len();
            let index = g.rows[..row].iter().filter(|r| r.1 == g.rows[row].1).count();
            b = self.bank_of(part, g.count, g.rows[row].1, index);
            b.name = g.rows[row].0.clone();
        } else {
            let banks = self.knob_banks(part, g.count);
            if !banks.is_empty() {
                b = banks[position % banks.len()].clone();
            }
        }
        // Controls are numbered by identity across groups: assign knobs and encoders "knob:N".
        let before: usize = groups[..which].iter().filter(|o| o.role == Role::Assign && o.is_knobs()).map(|o| o.count).sum();
        (0..g.count)
            .map(|i| {
                let mut slot = Slot { target: b.slots.get(i).copied().unwrap_or_default(), layer: Layer::None, label: b.labels.get(i).cloned().unwrap_or_default(), bank: b.name.clone() };
                if slot.target != Target::None {
                    slot.layer = b.layer;
                }
                if i < g.functions.len() {
                    slot.label = g.functions[i].clone();
                }
                if !screen {
                    self.overridden(&mut slot, &format!("knob:{}", before + i + 1), part, !g.functions.is_empty());
                }
                slot
            })
            .collect()
    }

    /// 1. Overrides, where what they name is still there; 2. the selected part's Sound's own.
    fn overridden(&self, r: &mut Slot, control: &str, part: u32, function: bool) {
        for layer in [Layer::Override, Layer::Sound] {
            for o in &self.overrides {
                if o.control != control || (layer == Layer::Override) != (o.part == 0) || (o.part != 0 && self.part(o.part).is_none()) {
                    continue;
                }
                let name = self.name_of(&o.target);
                if name.is_empty() || (layer == Layer::Sound && function) || (layer == Layer::Sound && o.part != part) {
                    continue;
                }
                *r = Slot { target: o.target, layer, label: name, bank: String::new() };
                return;
            }
        }
    }

    /// The Source of a `graph.get`: the parts, controls, scenes and each part's nodes with their parameters
    /// (`params_of` gives a node's, when read), as `assignmentSource`.
    pub fn from_graph(graph: &Value, overrides: Vec<Override>, params_of: &dyn Fn(u64) -> Option<Vec<SParam>>) -> Source {
        let mut s = Source { overrides, ..Source::default() };
        let by_id: HashMap<u64, &Value> = graph["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_u64()?, n))).collect();
        for p in graph["parts"].as_array().into_iter().flatten() {
            let mut nodes = Vec::new();
            crate::play::chain_nodes(&p["chain"], &p["splits"], 0, &mut nodes);
            for &id in &nodes {
                if s.nodes.contains_key(&id) {
                    continue;
                }
                let n = by_id.get(&id);
                let channels = |key: &str| n.and_then(|n| n[key].as_array()).map_or(0, |p| p.iter().map(|p| p["channels"].as_u64().unwrap_or(0)).sum::<u64>());
                let (i, o) = (channels("audioInputs"), channels("audioOutputs"));
                s.nodes.insert(id, SNode { name: n.and_then(|n| n["name"].as_str()).unwrap_or("Plugin").into(), effect: i > 0 && o > 0, instrument: o > 0 && i == 0, params: params_of(id).unwrap_or_default() });
            }
            s.parts.push(SPart { id: p["id"].as_u64().unwrap_or(0) as u32, name: p["name"].as_str().unwrap_or_default().into(), nodes });
        }
        s.controls = graph["controls"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|m| SControl { id: m["id"].as_u64().unwrap_or(0) as u32, name: m["name"].as_str().unwrap_or_default().into(), part: m["part"].as_u64().unwrap_or(0) as u32 })
            .collect();
        s.scenes = graph["scenes"].as_array().into_iter().flatten().map(|c| (c["id"].as_u64().unwrap_or(0) as u32, c["name"].as_str().unwrap_or_default().into())).collect();
        s
    }
}

/// A node's parameters as resolution sees them: their declared roles, and where the plugin declares none the
/// roles their names suggest.
pub fn assignable(list: &[crate::grid::Param]) -> Vec<SParam> {
    let declared = list.iter().any(|p| !p.role.is_empty());
    list.iter()
        .map(|p| SParam { id: p.id, name: p.name.clone(), role: if declared { p.role.clone() } else { role_from_name(&p.name, &p.module).into() }, priority: p.priority, stepped: p.stepped })
        .collect()
}

/// One knob of a group: what it plays and where that came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub target: Target,
    pub layer: Layer,
    /// What it plays, by name.
    pub label: String,
    /// The bank, row or mode it is in.
    pub bank: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(id: u32, name: &str, role: &str) -> SParam {
        SParam { id, name: name.into(), role: role.into(), priority: 0, stepped: false }
    }

    // The fixture of core/control/tests/physical_controls.cpp: Keys (a synth, then a delay), Lead (a bass with a
    // control of its own), two Performance controls, a scene; the synth's first parameters are Gain (7) and 99.
    fn source(roles: &dyn Fn(u64, u32) -> &'static str) -> Source {
        let mut s = Source::default();
        let synth = |n: &str| SNode { name: n.into(), effect: false, instrument: true, params: [(7, "Gain"), (99, "Tune")].iter().map(|(i, name)| param(*i, name, "")).collect() };
        s.nodes.insert(1, synth("synth"));
        s.nodes.insert(3, synth("bass"));
        s.nodes.insert(2, SNode { name: "fx".into(), effect: true, instrument: false, params: vec![SParam { stepped: true, ..param(1, "Mode", "") }, param(3, "Time", ""), param(4, "Wet", "")] });
        for (id, node) in s.nodes.iter_mut() {
            for p in &mut node.params {
                p.role = roles(*id, p.id).into();
            }
        }
        s.parts = vec![SPart { id: 10, name: "Keys".into(), nodes: vec![1, 2] }, SPart { id: 11, name: "Lead".into(), nodes: vec![3] }];
        s.controls = vec![SControl { id: 20, name: "Super".into(), part: 0 }, SControl { id: 21, name: "Swell".into(), part: 0 }, SControl { id: 22, name: "Brightness".into(), part: 11 }];
        s.scenes = vec![(30, "Verse".into())];
        s
    }

    fn none(_: u64, _: u32) -> &'static str {
        ""
    }

    fn parameter(node: u64, param: u32) -> Target {
        Target::Parameter { node, param }
    }

    fn group(role: Role, kind: &str, count: usize) -> Group {
        Group { role, kind: kind.into(), count, rows: vec![], modes: vec![], functions: vec![], paged: false }
    }

    #[test]
    fn with_no_part_the_knobs_follow_the_performances_controls_in_banks_as_wide_as_the_group() {
        let s = source(&none);
        let banks = s.knob_banks(0, 4);
        assert_eq!(banks.len(), 1);
        assert_eq!((banks[0].name.as_str(), banks[0].layer, banks[0].slots.len()), ("Controls 1\u{2013}4", Layer::Convention, 4));
        assert_eq!(banks[0].slots[..3], [Target::Control(20), Target::Control(21), Target::None]);
        assert_eq!(s.knob_banks(0, 6)[0].name, "Controls 1\u{2013}6");
    }

    #[test]
    fn a_sound_with_controls_of_its_own_has_them_as_its_assign_bank() {
        let b = source(&none).knob_banks(11, 4);
        assert_eq!((b.len(), b[0].layer, b[0].slots[0], b[0].labels[0].as_str()), (1, Layer::Sound, Target::Control(22), "Brightness"));
    }

    #[test]
    fn nothing_assigned_and_no_roles_is_assign_from_the_first_parameters_then_the_effects_bank() {
        let b = source(&none).knob_banks(10, 4);
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].name.as_str(), b[0].layer, b[0].slots[0], b[0].labels[0].as_str()), ("Assign 1\u{2013}4", Layer::Role, parameter(1, 7), "synth: Gain"));
        assert_eq!((b[0].slots[1], b[0].slots[3]), (parameter(1, 99), Target::None));
        // The effect's main control: its first continuous parameter, not the stepped Mode; a mix role wins.
        assert_eq!((b[1].layer, b[1].slots[0], b[1].slots[1]), (Layer::Effects, parameter(2, 3), Target::None));
        let mixed = source(&|n, p| if n == 2 && p == 4 { "mix" } else { "" }).knob_banks(10, 4);
        assert_eq!(mixed.last().unwrap().slots[0], parameter(2, 4));
    }

    #[test]
    fn declared_roles_fill_the_role_banks_up_to_the_groups_width() {
        let roles = |n: u64, p: u32| if n == 1 && p == 7 { "resonance" } else if n == 1 && p == 99 { "velocity" } else { "" };
        let s = source(&roles);
        let b = s.knob_banks(10, 4);
        assert_eq!((b.len(), b[0].name.as_str(), b[0].slots[1], b[0].slots[0]), (2, "Filter", parameter(1, 7), Target::None));
        assert_eq!(b[1].layer, Layer::Effects, "Amp EG left out: velocity is past the fourth role");
        let b = s.knob_banks(10, 6);
        assert_eq!((b.len(), b[0].name.as_str(), b[1].name.as_str(), b[1].slots[5], b[1].slots[0]), (3, "Filter", "Amp EG", parameter(1, 99), Target::None));
    }

    #[test]
    fn roles_come_from_the_names_where_a_plugin_declares_none() {
        assert_eq!((role_from_name("Cutoff", ""), role_from_name("Filter Cutoff", ""), role_from_name("Gain", "")), ("cutoff", "cutoff", ""));
        assert_eq!((role_from_name("Attack", "Filter"), role_from_name("Attack", "Osc/Filter 1"), role_from_name("Attack", "Amp")), ("filterAttack", "filterAttack", "attack"));
        let reso = crate::grid::Param { id: 1, name: "Reso".into(), ..Default::default() };
        let mix = crate::grid::Param { id: 2, name: "Other".into(), role: "mix".into(), ..Default::default() };
        assert_eq!(assignable(std::slice::from_ref(&reso))[0].role, "resonance", "no role declared by any: by name");
        assert_eq!(assignable(&[reso, mix])[0].role, "", "a plugin that declares any role gets none from names");
    }

    #[test]
    fn the_kicks_four_knobs_page_through_assign_then_effects_and_pages_wrap() {
        let s = source(&none);
        let kick = [Group { paged: true, ..group(Role::Assign, "knobs", 4) }, group(Role::Fixed, "knobs", 3)];
        let r = s.resolve(&kick, 0, 10, 0);
        assert_eq!((r[0].target, r[0].bank.as_str(), r[2].layer), (parameter(1, 7), "Assign 1\u{2013}4", Layer::None));
        assert_eq!(s.positions(&kick[0], 10), 2);
        let r = s.resolve(&kick, 0, 10, 1);
        assert_eq!((r[0].target, r[0].layer), (parameter(2, 3), Layer::Effects));
        assert_eq!(s.resolve(&kick, 0, 10, 2)[0].target, parameter(1, 7));
    }

    #[test]
    fn the_modx_screen_knobs_play_banks_six_wide_and_its_rows_name_their_banks() {
        let s = source(&|n, p| if n == 1 && p == 7 { "resonance" } else { "" });
        let rows = ["Assign 1", "Assign 2", "Tone", "EG", "EQ", "Effects"];
        let kinds = [BankKind::Assign, BankKind::Assign, BankKind::Filter, BankKind::AmpEg, BankKind::Eq, BankKind::Effects];
        let modx = [group(Role::Screen, "knobs", 6), Group { rows: rows.iter().zip(kinds).map(|(n, k)| (n.to_string(), k)).collect(), ..group(Role::Assign, "knobs", 4) }];
        let r = s.resolve(&modx, 0, 10, 0);
        assert_eq!(r.len(), 6);
        assert_eq!((r[0].bank.as_str(), r[0].target), ("Filter", Target::None));
        assert_eq!(r[1].target, parameter(1, 7));
        assert_eq!(s.positions(&modx[0], 10), 2, "Filter, then Effects");
        assert_eq!(s.positions(&modx[1], 10), 6);
        // Row "Tone": the Filter roles; row "Effects": the delay.
        let r = s.resolve(&modx, 1, 10, 2);
        assert_eq!((r[1].target, r[0].bank.as_str()), (parameter(1, 7), "Tone"));
        let r = s.resolve(&modx, 1, 10, 5);
        assert_eq!((r[0].target, r[0].bank.as_str()), (parameter(2, 3), "Effects"));
        // Row "Assign 2": blank, two parameters fill one bank.
        let plain = source(&none);
        let r = plain.resolve(&modx, 1, 10, 1);
        assert_eq!((r[0].target, r[0].bank.as_str()), (Target::None, "Assign 2"));
    }

    #[test]
    fn an_eight_knob_device_has_banks_eight_wide() {
        let s = source(&none);
        let eight = [group(Role::Assign, "encoders", 8)];
        let r = s.resolve(&eight, 0, 10, 0);
        assert_eq!((r[0].target, r[1].target, r[0].bank.as_str()), (parameter(1, 7), parameter(1, 99), "Assign 1\u{2013}8"));
        assert_eq!(s.positions(&eight[0], 10), 2);
    }

    #[test]
    fn overrides_take_an_assign_knob_but_not_the_screen_knob_of_the_same_number() {
        let mut s = source(&|n, p| if n == 1 && p == 7 { "resonance" } else { "" });
        let modx = [group(Role::Screen, "knobs", 6), Group { rows: vec![("Assign 1".into(), BankKind::Assign)], ..group(Role::Assign, "knobs", 4) }];
        s.overrides = vec![Override { control: "knob:2".into(), target: Target::Control(20), part: 0 }];
        let assign = s.resolve(&modx, 1, 10, 0);
        assert_eq!((assign[1].target, assign[1].layer, assign[1].label.as_str()), (Target::Control(20), Layer::Override, "Super"));
        assert_eq!(assign[0].layer, Layer::None, "the row's other knobs: its Assign bank is blank while the roles fill the Filter bank");
        let screen = s.resolve(&modx, 0, 10, 0);
        assert_eq!(screen[1].layer, Layer::Role, "a screen knob is never overridden");
        // An override whose target is gone falls through; a Sound's own override plays only with its part selected.
        s.overrides = vec![Override { control: "knob:1".into(), target: Target::Scene(99), part: 0 }, Override { control: "knob:3".into(), target: Target::Control(20), part: 11 }];
        let r = s.resolve(&modx, 1, 10, 0);
        assert_eq!((r[0].layer, r[2].target), (Layer::None, Target::None));
        let r = s.resolve(&modx, 1, 11, 0);
        assert_eq!((r[2].target, r[2].layer), (Target::Control(20), Layer::Sound));
    }

    #[test]
    fn the_knob_row_is_the_screen_groups_else_the_first_assign_ones() {
        let groups = [group(Role::Notes, "keys", 61), group(Role::Assign, "knobs", 4), group(Role::Screen, "encoders", 6)];
        assert_eq!(knob_group(&groups), Some(2));
        assert_eq!(knob_group(&groups[..2]), Some(1));
        assert_eq!(knob_group(&groups[..1]), None);
    }

    #[test]
    fn a_graph_makes_the_source_and_a_profile_the_groups() {
        let graph = json!({
            "nodes": [
                {"id": 1, "name": "Rhodes", "audioInputs": [], "audioOutputs": [{"channels": 2}]},
                {"id": 2, "name": "Chorus", "audioInputs": [{"channels": 2}], "audioOutputs": [{"channels": 2}]},
                {"id": 4, "name": "Bass", "audioInputs": [], "audioOutputs": [{"channels": 1}]},
            ],
            "parts": [
                {"id": 10, "name": "Keys", "chain": [2, 1]},
                {"id": 11, "name": "Low", "chain": [{"split": 1}], "splits": [{"id": 1, "lanes": [{"chain": [4]}]}]},
            ],
            "controls": [{"id": 7, "name": "Super", "part": 0}],
            "scenes": [{"id": 1, "name": "Verse"}],
        });
        let s = Source::from_graph(&graph, vec![], &|n| Some(vec![param(1, &format!("P{n}"), "")]));
        assert_eq!(s.parts.iter().map(|p| p.nodes.clone()).collect::<Vec<_>>(), [vec![2, 1], vec![4]]);
        assert_eq!((s.nodes[&2].effect, s.nodes[&1].instrument, s.nodes[&4].name.as_str()), (true, true, "Bass"));
        assert_eq!((s.controls.len(), s.scenes[0].1.as_str()), (1, "Verse"));
        let g = Group::from_profile(&json!({"kind": "knobs", "count": 4, "role": "assign", "rows": [{"name": "Tone", "bank": "filter"}], "paging": "button"}));
        assert_eq!((g.role, g.count, g.rows[0].1, g.paged), (Role::Assign, 4, BankKind::Filter, true));
        assert_eq!(Target::from_json(&Target::Parameter { node: 3, param: 4 }.json()), Target::Parameter { node: 3, param: 4 });
        assert_eq!(Override::parse(&json!({"assignments": [{"control": "knob:1", "kind": "partLevel", "id": 5, "part": 0}]})), [Override { control: "knob:1".into(), target: Target::PartLevel(5), part: 0 }]);
    }
}
