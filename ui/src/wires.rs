// SPDX-License-Identifier: AGPL-3.0-or-later
//! Routing's model and layout (hosts/standalone/PartsModel.cpp: `readRouting`, `inputsOf`, `wireState`,
//! `wireChannels`, `place`, `routeAround`, `layoutRouting`): the real graph as boxes in columns by signal
//! flow and its wires, laid out in pixels from (0, 0) and drawn by the Routing screen. Columns: the graph's
//! inputs first (the keys and MIDI, each audio input), the outputs last, each box one column after what
//! feeds it; a wire skipping columns keeps a slot in each it crosses; each column ordered to cross fewer
//! wires; each wire leaving and entering at its own place on a box's edge, so curves run only in the gaps
//! between columns. A collapsed part is one box with its plugins as chips; an expanded part is a frame
//! holding its plugins, laid out the same way inside it. A control line runs from a plugin's lane to the
//! parameter's plugin or to the control it drives, and from that control to each plugin it targets, in
//! straight runs and right-angle turns around the boxes.
//!
//! Pure functions on `graph.get`'s JSON: no window, no scale. The same points are what the screen draws.

use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet};

pub const GRAPH_INPUT: u64 = 0xFFFF_FFFE;
pub const GRAPH_OUTPUT: u64 = 0xFFFF_FFFF;
/// The keys and MIDI: the graph input's note port, in `NodeKey`s and wires' `fromPort`.
pub const NOTES_PORT: u64 = 0xFFFF_FFFF;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Port {
    pub id: u64,
    pub name: String,
    pub channels: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeInfo {
    pub id: u64,
    pub name: String,
    pub plugin: String,
    pub lanes: Vec<(u64, String)>,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub note_inputs: u64,
    pub note_outputs: u64,
    pub enabled: bool,
    pub ringing: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wire {
    pub id: u64,
    pub notes: bool,
    /// The part that made it (a wire the app made is `None`: a user's).
    pub part: Option<u64>,
    pub from: u64,
    pub to: u64,
    pub from_port: u64,
    pub to_port: u64,
    pub channels: Vec<(u64, u64)>,
    pub level: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub zones: Vec<Value>,
    pub split: u64,
    pub connected: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartInfo {
    pub id: u64,
    pub name: String,
    pub bus: bool,
    pub mute: bool,
    pub draining: bool,
    /// Every node of its chain and lanes, and its splits' own nodes, in id order.
    pub nodes: BTreeSet<u64>,
    /// Its plugins in chain order as a collapsed part's box shows them.
    pub chips: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapping {
    pub source: u64,
    pub lane: u64,
    pub target: u64,
    pub param: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Macro {
    pub id: u64,
    pub name: String,
    pub source: Option<(u64, u64)>,
    /// The nodes its parameter targets drive, and how many targets it has.
    pub nodes: Vec<u64>,
    pub targets: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<NodeInfo>,
    pub parts: Vec<PartInfo>,
    pub wires: Vec<Wire>,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub mappings: Vec<Mapping>,
    pub macros: Vec<Macro>,
    /// Note connections still sounding out (RFC 0021).
    pub draining: HashSet<(u64, u64)>,
}

fn ports(v: &Value) -> Vec<Port> {
    v.as_array().into_iter().flatten().map(|p| Port { id: p["id"].as_u64().unwrap_or(0), name: p["name"].as_str().unwrap_or_default().into(), channels: p["channels"].as_u64().unwrap_or(0) }).collect()
}

impl Graph {
    /// `graph.get` as the JUCE routing page reads it (`Control::graphState`).
    pub fn read(graph: &Value) -> Graph {
        let nodes: Vec<NodeInfo> = graph["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|n| NodeInfo {
                id: n["id"].as_u64().unwrap_or(0),
                name: n["name"].as_str().unwrap_or_default().into(),
                plugin: n["plugin"]["id"].as_str().unwrap_or_default().into(),
                lanes: n["plugin"]["lanes"].as_array().into_iter().flatten().map(|l| (l["lane"].as_u64().unwrap_or(0), l["name"].as_str().unwrap_or_default().to_string())).collect(),
                inputs: ports(&n["audioInputs"]),
                outputs: ports(&n["audioOutputs"]),
                note_inputs: n["noteInputs"].as_u64().unwrap_or(0),
                note_outputs: n["noteOutputs"].as_u64().unwrap_or(0),
                enabled: n["enabled"].as_bool().unwrap_or(true),
                ringing: n["ringing"].as_bool().unwrap_or(false),
            })
            .collect();
        let wires: Vec<Wire> = graph["wires"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|w| Wire {
                id: w["id"].as_u64().unwrap_or(0),
                notes: w["kind"] == "notes",
                part: (w["owner"] == "part").then(|| w["part"].as_u64().unwrap_or(0)),
                from: w["from"].as_u64().unwrap_or(0),
                to: w["to"].as_u64().unwrap_or(0),
                from_port: w["fromPort"].as_u64().unwrap_or(0),
                to_port: w["toPort"].as_u64().unwrap_or(0),
                channels: w["channels"].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_u64()?, p[1].as_u64()?))).collect(),
                level: w["level"].as_f64().unwrap_or(1.),
                pan: w["pan"].as_f64().unwrap_or(0.),
                mute: w["mute"].as_bool().unwrap_or(false),
                solo: w["solo"].as_bool().unwrap_or(false),
                zones: w["zones"].as_array().cloned().unwrap_or_default(),
                split: w["split"].as_u64().unwrap_or(0),
                connected: w["connected"].as_bool().unwrap_or(true),
                enabled: w["enabled"].as_bool().unwrap_or(true),
            })
            .collect();
        let by_id: HashMap<u64, &Value> = graph["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_u64()?, n))).collect();
        let ringing: HashSet<u64> = nodes.iter().filter(|n| n.ringing).map(|n| n.id).collect();
        let parts = graph["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| {
                let id = p["id"].as_u64().unwrap_or(0);
                let mut flat = Vec::new();
                crate::play::chain_nodes(&p["chain"], &p["splits"], 0, &mut flat);
                let mut owned: BTreeSet<u64> = flat.into_iter().collect();
                for s in p["splits"].as_array().into_iter().flatten() {
                    for key in ["splitter", "joiner"] {
                        if let Some(n) = s[key].as_u64().filter(|n| *n != 0) {
                            owned.insert(n);
                        }
                    }
                }
                let mute = p["mute"].as_bool().unwrap_or(false);
                let draining = wires.iter().any(|w| w.part == Some(id) && (ringing.contains(&w.from) || ringing.contains(&w.to)));
                PartInfo { id, name: p["name"].as_str().unwrap_or_default().into(), bus: p["input"]["kind"] == "bus", mute, draining, nodes: owned, chips: chips(&p["chain"], p, &by_id) }
            })
            .collect();
        let mut draining = HashSet::new();
        for c in graph["connections"].as_array().into_iter().flatten() {
            if c["kind"] == "notes" && c["draining"] == true {
                draining.insert((c["from"].as_u64().unwrap_or(0), c["to"].as_u64().unwrap_or(0)));
            }
        }
        Graph {
            nodes,
            parts,
            wires,
            inputs: ports(&graph["audioInputs"]),
            outputs: ports(&graph["audioOutputs"]),
            mappings: graph["mappings"].as_array().into_iter().flatten().map(|m| Mapping { source: m["from"].as_u64().unwrap_or(0), lane: m["lane"].as_u64().unwrap_or(0), target: m["to"].as_u64().unwrap_or(0), param: m["param"].as_u64().unwrap_or(0) }).collect(),
            macros: graph["controls"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|m| Macro {
                    id: m["id"].as_u64().unwrap_or(0),
                    name: m["name"].as_str().unwrap_or_default().into(),
                    source: m["source"].as_object().map(|_| (m["source"]["node"].as_u64().unwrap_or(0), m["source"]["lane"].as_u64().unwrap_or(0))).filter(|s| s.0 != 0),
                    nodes: m["targets"].as_array().into_iter().flatten().filter(|t| t["kind"] == "parameter").filter_map(|t| t["node"].as_u64()).collect(),
                    targets: m["targets"].as_array().map_or(0, Vec::len),
                })
                .collect(),
            draining,
        }
    }

    pub fn node(&self, id: u64) -> Option<&NodeInfo> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// A node's name, "a plugin" for one that is gone (`nameOf`).
    pub fn name_of(&self, id: u64) -> String {
        self.node(id).map_or("a plugin".to_string(), |n| n.name.clone())
    }

    pub fn disabled(&self, id: u64) -> bool {
        self.node(id).is_some_and(|n| !n.enabled)
    }

    pub fn ringing(&self, id: u64) -> bool {
        self.node(id).is_some_and(|n| n.ringing)
    }

    /// The part a node belongs to (in a chain or lane, or a split's own node).
    pub fn owner(&self, node: u64) -> Option<&PartInfo> {
        self.parts.iter().find(|p| p.nodes.contains(&node))
    }

    pub fn has_user_wires(&self, node: u64) -> bool {
        self.wires.iter().any(|w| w.part.is_none() && (w.from == node || w.to == node))
    }
}

/// A part's plugins in chain order as a collapsed part's box lists them: "Name", "Name · off", and a
/// split by its lanes' names with how many lanes it has (`layoutRouting`'s chips).
fn chips(chain: &Value, part: &Value, nodes: &HashMap<u64, &Value>) -> Vec<String> {
    let mut out = Vec::new();
    for entry in chain.as_array().into_iter().flatten() {
        if let Some(n) = entry.as_u64() {
            if let Some(node) = nodes.get(&n) {
                let name = node["name"].as_str().unwrap_or_default();
                out.push(if node["enabled"] == false { format!("{name} · off") } else { name.to_string() });
            }
        } else if let Some(split) = part["splits"].as_array().and_then(|s| s.iter().find(|s| s["id"] == entry["split"])) {
            let kind = split["kind"].as_str().unwrap_or("copy");
            let lanes = split["lanes"].as_array().cloned().unwrap_or_default();
            let names: Vec<String> = lanes.iter().enumerate().map(|(k, lane)| crate::parts::lane_name(kind, k, &crate::parts::boxes(&lane["chain"], part, nodes, 1))).collect();
            out.push(format!("{} · {} lanes", names.join(" | "), lanes.len()));
        }
    }
    out
}

/// How a node is told apart in the layout and the cursor: its role (`roleOf`).
pub fn role_of(n: &NodeInfo) -> &'static str {
    let (i, o) = (n.inputs.iter().map(|p| p.channels).sum::<u64>(), n.outputs.iter().map(|p| p.channels).sum::<u64>());
    if n.note_inputs > 0 && n.note_outputs > 0 && o == 0 {
        "Notes"
    } else if n.note_outputs > 0 && o == 0 {
        if n.note_inputs > 0 { "Notes" } else { "Sequencer" }
    } else if o > 0 && i == 0 {
        if n.note_inputs > 0 { "Instrument" } else { "Generator" }
    } else if i > 0 && o > 0 {
        "Effect"
    } else if i > 0 {
        "Analyzer"
    } else {
        "Notes"
    }
}

/// A box of the view: a node of the graph, or one end of it (the keys and MIDI, an audio input or an output).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutingNode {
    pub node: u64,
    pub port: u64,
    pub name: String,
    pub role: String,
    /// The part or bus it belongs to; 0 none.
    pub part: u64,
    pub part_name: String,
    pub user_wires: bool,
    pub split_node: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Routing {
    pub nodes: Vec<RoutingNode>,
}

impl Routing {
    /// The ends by port, other nodes by node.
    pub fn find(&self, node: u64, port: u64) -> Option<&RoutingNode> {
        self.nodes.iter().find(|n| n.node == node && ((node != GRAPH_INPUT && node != GRAPH_OUTPUT) || n.port == port))
    }
}

/// The nodes the view shows (`readRouting`): the keys and MIDI and each audio input, then the nodes in
/// part order and the rest, then each output.
pub fn read_routing(g: &Graph) -> Routing {
    let mut nodes = vec![RoutingNode { node: GRAPH_INPUT, port: NOTES_PORT, name: "Keys & MIDI".into(), role: "Notes".into(), ..Default::default() }];
    for p in &g.inputs {
        nodes.push(RoutingNode { node: GRAPH_INPUT, port: p.id, name: if p.name.is_empty() { "Audio in".into() } else { p.name.clone() }, role: "Audio".into(), ..Default::default() });
    }
    let mut order: Vec<u64> = vec![];
    for p in &g.parts {
        order.extend(p.nodes.iter().copied());
    }
    for n in &g.nodes {
        if !order.contains(&n.id) {
            order.push(n.id);
        }
    }
    for id in order {
        let Some(n) = g.node(id) else { continue };
        let owner = g.owner(id);
        nodes.push(RoutingNode {
            node: id,
            port: 0,
            name: n.name.clone(),
            role: role_of(n).into(),
            part: owner.map_or(0, |p| p.id),
            part_name: owner.map_or(String::new(), |p| p.name.clone()),
            user_wires: g.has_user_wires(id),
            split_node: n.plugin == "org.oscairn.split" || n.plugin == "org.oscairn.join",
        });
    }
    for p in &g.outputs {
        nodes.push(RoutingNode { node: GRAPH_OUTPUT, port: p.id, name: if p.name.is_empty() { "Output".into() } else { p.name.clone() }, role: "Output".into(), ..Default::default() });
    }
    Routing { nodes }
}

/// How a wire is drawn: enabled; disabled (muted, switched off, or on a plugin switched off); or draining
/// (on a plugin switched off that is still sounding out, or a note connection draining, RFC 0021).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireState {
    Enabled,
    Disabled,
    Draining,
}

pub fn wire_state(w: &Wire, g: &Graph) -> WireState {
    if g.ringing(w.from) || g.ringing(w.to) {
        return WireState::Draining;
    }
    if w.mute || !w.enabled || g.disabled(w.from) || g.disabled(w.to) {
        return WireState::Disabled;
    }
    if w.notes && g.draining.contains(&(w.from, w.to)) {
        return WireState::Draining;
    }
    WireState::Enabled
}

/// How many channels an audio wire carries: the source channels its pairs take, or else all its source
/// port's (Routing draws a line each); 1 for a note wire or an unknown port.
pub fn wire_channels(w: &Wire, g: &Graph) -> u64 {
    if w.notes {
        return 1;
    }
    if !w.channels.is_empty() {
        return w.channels.iter().map(|c| c.0).collect::<BTreeSet<_>>().len() as u64;
    }
    let ports: &[Port] = if w.from == GRAPH_INPUT { &g.inputs } else { &[] };
    let ports = g.node(w.from).map_or(ports, |n| &n.outputs);
    ports.iter().find(|p| p.id == w.from_port).map_or(1, |p| p.channels.max(1))
}

/// One source into a node's input: a row of that input's implicit Mix.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MixRow {
    pub wire: u64,
    pub source: String,
    pub level: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub user: bool,
    /// The row is edited here (a lane's or split's wire takes its Mix from the lane).
    pub editable: bool,
    /// Notes from the graph input: the keys and MIDI.
    pub keys: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutingInput {
    pub port: u64,
    pub name: String,
    pub rows: Vec<MixRow>,
}

/// A node's audio inputs that have sources, with a row per source (its Mix), and its notes (`inputsOf`).
pub fn inputs_of(g: &Graph, node: u64) -> Vec<RoutingInput> {
    let mut out: Vec<RoutingInput> = vec![];
    let ports: &[Port] = if node == GRAPH_OUTPUT { &g.outputs } else { g.node(node).map_or(&[][..], |n| &n.inputs[..]) };
    for w in &g.wires {
        if w.to != node {
            continue;
        }
        let (port, name) = if w.notes {
            (NOTES_PORT, "Notes".to_string())
        } else {
            let mut name = format!("Input {}", w.to_port + 1);
            if let Some(p) = ports.iter().find(|p| p.id == w.to_port) {
                if !p.name.is_empty() {
                    name = p.name.clone();
                }
            }
            (w.to_port, name)
        };
        let at = match out.iter().position(|i| i.port == port) {
            Some(at) => at,
            None => {
                out.push(RoutingInput { port, name, rows: vec![] });
                out.len() - 1
            }
        };
        let mut source = if w.from == GRAPH_INPUT { if w.notes { "Keys & MIDI".to_string() } else { "Audio in".to_string() } } else { g.name_of(w.from) };
        if let Some(p) = w.part {
            if let Some(part) = g.parts.iter().find(|x| x.id == p) {
                source += &format!(" ({})", part.name);
            }
        }
        out[at].rows.push(MixRow { wire: w.id, source, level: w.level, pan: w.pan, mute: w.mute, solo: w.solo, user: w.part.is_none(), editable: w.split == 0, keys: w.from == GRAPH_INPUT });
    }
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub node: i32,
    pub node_height: i32,
    pub part: i32,
    pub part_height: i32,
    pub gap: i32,
    pub row: i32,
    pub slot: i32,
    pub frame: i32,
    pub frame_head: i32,
    pub control: i32,
    pub control_height: i32,
}

impl Default for Metrics {
    fn default() -> Metrics {
        Metrics { node: 170, node_height: 44, part: 280, part_height: 64, gap: 56, row: 16, slot: 10, frame: 14, frame_head: 40, control: 120, control_height: 30 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Element {
    w: i32,
    h: i32,
    first: bool,
    last: bool,
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    from: usize,
    to: usize,
}

#[derive(Clone, Debug, Default)]
struct Placed {
    rects: Vec<Rect>,
    paths: Vec<Vec<Pt>>,
    width: i32,
    height: i32,
}

/// One slot of a column: an element, or a wire passing through it (an edge).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot {
    element: i32,
    edge: i32,
}

/// One layered placement: elements with their sizes (first: the first column; last: the last), and edges
/// between them, each with its points from exit to entry (`place`).
fn place(elements: &[Element], edges: &[Edge], m: &Metrics) -> Placed {
    let n = elements.len();
    let mut out = Placed { rects: vec![Rect::default(); n], paths: vec![vec![]; edges.len()], ..Default::default() };
    if n == 0 {
        return out;
    }
    let (mut preds, mut succs) = (vec![vec![]; n], vec![vec![]; n]);
    for e in edges.iter().filter(|e| e.from != e.to) {
        preds[e.to].push(e.from);
        succs[e.from].push(e.to);
    }
    // Ranks: the longest path from the first column; a loop's elements follow what was ranked.
    let mut indegree: Vec<usize> = preds.iter().map(Vec::len).collect();
    let mut order: Vec<usize> = (0..n).filter(|v| indegree[*v] == 0).collect();
    let mut i = 0;
    while i < order.len() {
        for &s in &succs[order[i]] {
            indegree[s] -= 1;
            if indegree[s] == 0 {
                order.push(s);
            }
        }
        i += 1;
    }
    for v in 0..n {
        if !order.contains(&v) {
            order.push(v);
        }
    }
    let any_first = elements.iter().any(|e| e.first);
    let mut rank: Vec<usize> = (0..n).map(|v| if elements[v].first || !any_first { 0 } else { 1 }).collect();
    let at = |v: usize| order.iter().position(|x| *x == v).unwrap_or(usize::MAX);
    for &u in &order {
        for &s in &succs[u] {
            if !elements[s].first && at(s) > at(u) {
                rank[s] = rank[s].max(rank[u] + 1);
            }
        }
    }
    let top = (0..n).filter(|v| !elements[*v].last).map(|v| rank[v]).max().unwrap_or(0);
    for v in 0..n {
        if elements[v].last {
            rank[v] = top + 1;
        }
    }
    let columns = rank.iter().copied().max().unwrap_or(0) + 1;
    // Layers, with a slot for each wire in each column it crosses.
    let mut layers: Vec<Vec<Slot>> = vec![vec![]; columns];
    for v in 0..n {
        layers[rank[v]].push(Slot { element: v as i32, edge: -1 });
    }
    for (i, e) in edges.iter().enumerate() {
        for layer in layers.iter_mut().take(rank[e.to]).skip(rank[e.from] + 1) {
            layer.push(Slot { element: -1, edge: i as i32 });
        }
    }
    let position = |layers: &Vec<Vec<Slot>>, r: usize, s: Slot| layers[r].iter().position(|x| *x == s).map_or(-1.0, |i| i as f64);
    // What a slot is joined to in the column before (up) or after.
    let neighbors = |layers: &Vec<Vec<Slot>>, r: usize, s: Slot, up: bool| -> Vec<f64> {
        let mut at = vec![];
        let other = if up { r as i64 - 1 } else { r as i64 + 1 };
        if other < 0 || other >= columns as i64 {
            return at;
        }
        let other = other as usize;
        if s.element < 0 {
            let e = edges[s.edge as usize];
            let end = if up { e.from } else { e.to };
            at.push(if rank[end] == other { position(layers, other, Slot { element: end as i32, edge: -1 }) } else { position(layers, other, Slot { element: -1, edge: s.edge }) });
            return at;
        }
        for (i, e) in edges.iter().enumerate() {
            let (me, end) = if up { (e.to, e.from) } else { (e.from, e.to) };
            if me as i32 != s.element || e.from == e.to {
                continue;
            }
            let forward = if up { rank[end] < r } else { rank[end] > r };
            if !forward {
                continue;
            }
            at.push(if rank[end] == other { position(layers, other, Slot { element: end as i32, edge: -1 }) } else { position(layers, other, Slot { element: -1, edge: i as i32 }) });
        }
        at
    };
    for sweep in 0..8 {
        let up = sweep % 2 == 0;
        for step in 0..columns {
            let r = if up { step } else { columns - 1 - step };
            let mut keyed: Vec<(f64, Slot)> = layers[r]
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let at = neighbors(&layers, r, *s, up);
                    (if at.is_empty() { i as f64 } else { at.iter().sum::<f64>() / at.len() as f64 }, *s)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal)); // stable
            layers[r] = keyed.into_iter().map(|k| k.1).collect();
        }
    }
    // Pixels: columns left to right, each centered on the tallest.
    let (mut x, mut width, mut height) = (vec![0; columns], vec![0; columns], vec![0; columns]);
    for r in 0..columns {
        for s in &layers[r] {
            width[r] = width[r].max(if s.element < 0 { 0 } else { elements[s.element as usize].w });
        }
        for (i, s) in layers[r].iter().enumerate() {
            height[r] += (if i > 0 { m.row } else { 0 }) + if s.element < 0 { m.slot } else { elements[s.element as usize].h };
        }
        x[r] = if r > 0 { x[r - 1] + width[r - 1] + m.gap } else { 0 };
    }
    out.width = x[columns - 1] + width[columns - 1];
    out.height = height.iter().copied().max().unwrap_or(0);
    let mut dummy_y: BTreeMap<(i32, usize), f64> = BTreeMap::new(); // (edge, column) -> its slot's middle
    for r in 0..columns {
        let mut y = (out.height - height[r]) / 2;
        for s in &layers[r] {
            if s.element < 0 {
                dummy_y.insert((s.edge, r), f64::from(y) + f64::from(m.slot) / 2.0);
                y += m.slot + m.row;
                continue;
            }
            let e = elements[s.element as usize];
            out.rects[s.element as usize] = Rect { x: x[r], y, w: e.w, h: e.h };
            y += e.h + m.row;
        }
    }
    // Ports: each wire its own place on an edge, in the order of what is at its other end.
    let towards = |out: &Placed, i: usize, exit: bool| -> f64 {
        let e = edges[i];
        let r = rank[if exit { e.from } else { e.to }];
        let next = if exit { r as i64 + 1 } else { r as i64 - 1 };
        if next >= 0 {
            if let Some(d) = dummy_y.get(&(i as i32, next as usize)) {
                return *d;
            }
        }
        let o = out.rects[if exit { e.to } else { e.from }];
        f64::from(o.y) + f64::from(o.h) / 2.0
    };
    let (mut exit_y, mut entry_y) = (vec![0.; edges.len()], vec![0.; edges.len()]);
    for v in 0..n {
        for exit in [true, false] {
            let mut mine: Vec<usize> = (0..edges.len()).filter(|i| (if exit { edges[*i].from } else { edges[*i].to }) == v && edges[*i].from != edges[*i].to).collect();
            mine.sort_by(|a, b| towards(&out, *a, exit).partial_cmp(&towards(&out, *b, exit)).unwrap_or(Ordering::Equal));
            let rect = out.rects[v];
            for (k, i) in mine.iter().enumerate() {
                let y = f64::from(rect.y) + f64::from(rect.h) * (k + 1) as f64 / (mine.len() + 1) as f64;
                if exit { exit_y[*i] = y } else { entry_y[*i] = y }
            }
        }
    }
    let mut back = false;
    for (i, e) in edges.iter().enumerate() {
        if e.from == e.to {
            continue;
        }
        let (a, b) = (out.rects[e.from], out.rects[e.to]);
        let (ra, rb) = (rank[e.from], rank[e.to]);
        let mut p = vec![Pt { x: f64::from(a.x + a.w), y: exit_y[i] }];
        let right = f64::from(x[ra] + width[ra]);
        if right > f64::from(a.x + a.w) {
            p.push(Pt { x: right, y: exit_y[i] }); // along its own row to the gap
        }
        if rb > ra {
            for r in ra + 1..rb {
                let y = dummy_y[&(i as i32, r)];
                p.push(Pt { x: f64::from(x[r]), y });
                p.push(Pt { x: f64::from(x[r] + width[r]), y });
            }
        } else {
            // Back to an earlier column: below everything, through the gaps either side.
            back = true;
            let below = f64::from(out.height + m.row);
            let half = f64::from(m.gap) / 2.0;
            p.push(Pt { x: right + half, y: exit_y[i] });
            p.push(Pt { x: right + half, y: below });
            p.push(Pt { x: f64::from(b.x) - half, y: below });
            p.push(Pt { x: f64::from(b.x) - half, y: entry_y[i] });
        }
        p.push(Pt { x: f64::from(b.x), y: entry_y[i] });
        out.paths[i] = p;
    }
    if back {
        out.height += m.row * 2;
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
struct Cost(f64);
impl Eq for Cost {}
impl PartialOrd for Cost {
    fn partial_cmp(&self, o: &Cost) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cost {
    fn cmp(&self, o: &Cost) -> Ordering {
        o.0.total_cmp(&self.0) // a min-heap
    }
}

/// A way from `from` to `to` in straight runs and right-angle turns that keeps off `boxes`: out of `from`
/// along `out` and into `to` along `inn` (unit steps), each by `margin`, then through the lines `margin`
/// beside each box's edges, within `bounds`, the shortest with the fewest turns. With no such way it goes
/// straight across (`routeAround`).
fn route_around(from: Pt, out: Pt, to: Pt, inn: Pt, boxes: &[Rect], margin: f64, bounds: Rect) -> Vec<Pt> {
    let a = Pt { x: from.x + out.x * margin, y: from.y + out.y * margin };
    let b = Pt { x: to.x - inn.x * margin, y: to.y - inn.y * margin };
    let (mut xs, mut ys) = (vec![a.x, b.x], vec![a.y, b.y]);
    for r in boxes {
        xs.extend([f64::from(r.x) - margin, f64::from(r.x + r.w) + margin]);
        ys.extend([f64::from(r.y) - margin, f64::from(r.y + r.h) + margin]);
    }
    let keep = |v: &mut Vec<f64>, low: f64, high: f64, p: f64, q: f64| {
        v.retain(|x| !((*x < low || *x > high) && *x != p && *x != q));
        v.sort_by(f64::total_cmp);
        v.dedup();
    };
    keep(&mut xs, f64::from(bounds.x), f64::from(bounds.x + bounds.w), a.x, b.x);
    keep(&mut ys, f64::from(bounds.y), f64::from(bounds.y + bounds.h), a.y, b.y);
    // A run is clear when it misses every box grown by half the margin.
    let clear = |p: Pt, q: Pt| {
        let h = margin / 2.0;
        for r in boxes {
            let (l, t, rr, bb) = (f64::from(r.x) - h, f64::from(r.y) - h, f64::from(r.x + r.w) + h, f64::from(r.y + r.h) + h);
            let blocked = if p.y == q.y { p.y > t && p.y < bb && p.x.min(q.x).max(l) < p.x.max(q.x).min(rr) } else { p.x > l && p.x < rr && p.y.min(q.y).max(t) < p.y.max(q.y).min(bb) };
            if blocked {
                return false;
            }
        }
        true
    };
    let (nx, ny) = (xs.len(), ys.len());
    let index = |v: &[f64], x: f64| v.partition_point(|e| *e < x);
    let (ax, ay, bx, by) = (index(&xs, a.x), index(&ys, a.y), index(&xs, b.x), index(&ys, b.y));
    // Dijkstra over (column, row, heading): length, and a turn costs as much as two margins.
    const DX: [i64; 4] = [1, -1, 0, 0];
    const DY: [i64; 4] = [0, 0, 1, -1];
    let heading = |d: Pt| if d.x > 0. { 0 } else if d.x < 0. { 1 } else if d.y > 0. { 2 } else { 3 };
    let turn = 2. * margin;
    let mut cost = vec![f64::INFINITY; nx * ny * 4];
    let mut back = vec![usize::MAX; cost.len()];
    let state = |x: usize, y: usize, d: usize| (y * nx + x) * 4 + d;
    let mut queue: BinaryHeap<(Cost, usize)> = BinaryHeap::new();
    let first = state(ax, ay, heading(out));
    cost[first] = 0.;
    queue.push((Cost(0.), first));
    let mut goal = None;
    while let Some((Cost(c), s)) = queue.pop() {
        if c > cost[s] {
            continue;
        }
        let (d, x, y) = (s % 4, (s / 4) % nx, s / 4 / nx);
        if x == bx && y == by {
            goal = Some(s);
            break;
        }
        for e in 0..4usize {
            let (u, v) = (x as i64 + DX[e], y as i64 + DY[e]);
            if u < 0 || v < 0 || u >= nx as i64 || v >= ny as i64 || (e ^ 1) == d {
                continue; // no turning back
            }
            let (u, v) = (u as usize, v as usize);
            let (p, q) = (Pt { x: xs[x], y: ys[y] }, Pt { x: xs[u], y: ys[v] });
            if !clear(p, q) {
                continue;
            }
            // Arriving against `inn` costs the turn into the end.
            let next = c + (q.x - p.x).abs() + (q.y - p.y).abs() + if e != d { turn } else { 0. } + if u == bx && v == by && e != heading(inn) { turn } else { 0. };
            let t = state(u, v, e);
            if next < cost[t] {
                cost[t] = next;
                back[t] = s;
                queue.push((Cost(next), t));
            }
        }
    }
    let mut way: Vec<Pt> = vec![];
    match goal {
        None => way = vec![a, Pt { x: b.x, y: a.y }, b],
        Some(mut s) => loop {
            way.insert(0, Pt { x: xs[(s / 4) % nx], y: ys[s / 4 / nx] });
            if back[s] == usize::MAX {
                break;
            }
            s = back[s];
        },
    }
    way.insert(0, from);
    way.push(to);
    // Only the corners: a point in line with both its neighbors goes.
    let mut corners: Vec<Pt> = vec![];
    for p in way {
        if corners.last().is_some_and(|l| l.x == p.x && l.y == p.y) {
            continue;
        }
        if corners.len() >= 2 {
            let (o, q) = (corners[corners.len() - 2], corners[corners.len() - 1]);
            if (o.x == q.x && q.x == p.x) || (o.y == q.y && q.y == p.y) {
                corners.pop();
            }
        }
        corners.push(p);
    }
    corners
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoxKind {
    Node,
    Part,
    Frame,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LBox {
    pub kind: BoxKind,
    /// A node, or a graph input or output port.
    pub node: u64,
    pub port: u64,
    /// A part's box or frame, or the part a node is in.
    pub part: u64,
    pub name: String,
    pub detail: String,
    /// A collapsed part's plugins, in chain order.
    pub chips: Vec<String>,
    pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LPath {
    /// The wire: an index into `Graph::wires`.
    pub wire: usize,
    pub state: WireState,
    pub points: Vec<Pt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlNode {
    pub id: u64,
    pub name: String,
    pub bounds: Rect,
}

/// A control line: a plugin's lane straight to a parameter (macro 0), or to a control it drives, and that
/// control to each plugin it targets.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub source: u64,
    pub lane: u64,
    pub macro_id: u64,
    pub target: u64,
    pub from_control: bool,
    pub from: Pt,
    pub to: Pt,
    pub points: Vec<Pt>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Frames first, then what is drawn over them.
    pub boxes: Vec<LBox>,
    pub paths: Vec<LPath>,
    pub controls: Vec<ControlNode>,
    pub links: Vec<Link>,
    pub width: i32,
    pub height: i32,
}

#[derive(Default)]
struct Inner {
    nodes: Vec<u64>,
    elements: Vec<Element>,
    edges: Vec<Edge>,
    index: BTreeMap<u64, usize>,
    inn: usize,
    out: usize,
    placed: Placed,
}

#[derive(Clone, Copy)]
struct Top {
    node: Option<usize>,
    part: u64,
}

/// The whole view: graph inputs and outputs, loose plugins and parts in columns, an expanded part a frame
/// of its own plugins laid out the same way, the wires and the control lines (`layoutRouting`). With
/// `collapsed` parts are one box each but those in `expanded`; otherwise every part is a frame.
pub fn layout_routing(g: &Graph, collapsed: bool, expanded: &BTreeSet<u64>, m: &Metrics) -> Layout {
    let mut out = Layout::default();
    let routing = read_routing(g);
    let open = |part: u64| !collapsed || expanded.contains(&part);
    // The elements of the whole view.
    let mut tops: Vec<Top> = vec![];
    let mut elements: Vec<Element> = vec![];
    let mut top_of: BTreeMap<(u64, u64), usize> = BTreeMap::new(); // (node, port) of an end, (node, 0) of a plugin
    let mut top_of_part: BTreeMap<u64, usize> = BTreeMap::new();
    let mut part_of: BTreeMap<u64, u64> = BTreeMap::new();
    for (i, n) in routing.nodes.iter().enumerate() {
        if n.part != 0 {
            part_of.insert(n.node, n.part);
            if let std::collections::btree_map::Entry::Vacant(v) = top_of_part.entry(n.part) {
                v.insert(tops.len());
                tops.push(Top { node: None, part: n.part });
                elements.push(Element { w: m.part, h: m.part_height, ..Default::default() });
            }
            top_of.insert((n.node, 0), top_of_part[&n.part]);
            continue;
        }
        let (input, output) = (n.node == GRAPH_INPUT, n.node == GRAPH_OUTPUT);
        top_of.insert((n.node, if input || output { n.port } else { 0 }), tops.len());
        tops.push(Top { node: Some(i), part: 0 });
        elements.push(Element { w: m.node, h: m.node_height, first: input, last: output });
    }
    let end_of = |w: &Wire, from: bool| -> Option<usize> {
        let node = if from { w.from } else { w.to };
        if node == GRAPH_INPUT {
            return top_of.get(&(node, if w.notes { NOTES_PORT } else { w.from_port })).copied();
        }
        if node == GRAPH_OUTPUT {
            return top_of.get(&(node, w.to_port)).copied();
        }
        top_of.get(&(node, 0)).copied()
    };
    // Wires: between elements of the view, or inside an expanded part.
    let mut inner: BTreeMap<u64, Inner> = BTreeMap::new();
    for part in top_of_part.keys() {
        if !open(*part) {
            continue;
        }
        let mut inn = Inner::default();
        for n in routing.nodes.iter().filter(|n| n.part == *part) {
            inn.index.insert(n.node, inn.nodes.len());
            inn.nodes.push(n.node);
            inn.elements.push(Element { w: m.node, h: m.node_height, ..Default::default() });
        }
        inn.inn = inn.elements.len();
        inn.elements.push(Element { w: 0, h: m.node_height, first: true, last: false });
        inn.out = inn.elements.len();
        inn.elements.push(Element { w: 0, h: m.node_height, first: false, last: true });
        inner.insert(*part, inn);
    }
    let mut top_edges: Vec<Edge> = vec![];
    let mut top_wire: Vec<usize> = vec![];
    let (mut enter_edge, mut leave_edge, mut inner_edge): (BTreeMap<usize, usize>, BTreeMap<usize, usize>, BTreeMap<usize, usize>) = Default::default();
    for (i, w) in g.wires.iter().enumerate() {
        let (Some(a), Some(b)) = (end_of(w, true), end_of(w, false)) else { continue };
        if a == b {
            let part = tops[a].part;
            if part == 0 || !open(part) {
                continue; // inside a collapsed part
            }
            let inn = inner.get_mut(&part).unwrap();
            if let (Some(f), Some(t)) = (inn.index.get(&w.from).copied(), inn.index.get(&w.to).copied()) {
                inner_edge.insert(i, inn.edges.len());
                inn.edges.push(Edge { from: f, to: t });
            }
            continue;
        }
        top_wire.push(i);
        top_edges.push(Edge { from: a, to: b });
        let pa = tops[a].part;
        if pa != 0 && open(pa) {
            let inn = inner.get_mut(&pa).unwrap();
            if let Some(f) = inn.index.get(&w.from).copied() {
                leave_edge.insert(i, inn.edges.len());
                let to = inn.out;
                inn.edges.push(Edge { from: f, to });
            }
        }
        let pb = tops[b].part;
        if pb != 0 && open(pb) {
            let inn = inner.get_mut(&pb).unwrap();
            if let Some(t) = inn.index.get(&w.to).copied() {
                enter_edge.insert(i, inn.edges.len());
                let from = inn.inn;
                inn.edges.push(Edge { from, to: t });
            }
        }
    }
    // A frame's plugins sit closer.
    let inside = Metrics { gap: m.gap / 2, ..*m };
    for (part, inn) in inner.iter_mut() {
        inn.placed = place(&inn.elements, &inn.edges, &inside);
        elements[top_of_part[part]] = Element { w: inn.placed.width + 2 * m.frame, h: inn.placed.height + m.frame_head + m.frame, ..Default::default() };
    }
    let placed = place(&elements, &top_edges, m);
    out.width = placed.width;
    out.height = placed.height;
    // Boxes: frames first, then plugins, parts and ends over them.
    let origin = |part: u64| {
        let r = placed.rects[top_of_part[&part]];
        Pt { x: f64::from(r.x + m.frame), y: f64::from(r.y + m.frame_head) }
    };
    let part_name = |id: u64| g.parts.iter().find(|p| p.id == id).map_or(String::new(), |p| p.name.clone());
    for part in inner.keys() {
        out.boxes.push(LBox { kind: BoxKind::Frame, node: 0, port: 0, part: *part, name: part_name(*part), detail: String::new(), chips: vec![], bounds: placed.rects[top_of_part[part]] });
    }
    for (t, top) in tops.iter().enumerate() {
        if top.part != 0 && open(top.part) {
            continue;
        }
        let mut b = LBox { kind: BoxKind::Node, node: 0, port: 0, part: 0, name: String::new(), detail: String::new(), chips: vec![], bounds: placed.rects[t] };
        if top.part != 0 {
            b.kind = BoxKind::Part;
            b.part = top.part;
            if let Some(p) = g.parts.iter().find(|p| p.id == top.part) {
                b.name = p.name.clone();
                b.detail = if p.bus { "bus".into() } else { String::new() };
                b.chips = p.chips.clone();
            }
        } else if let Some(n) = top.node.map(|i| &routing.nodes[i]) {
            b.node = n.node;
            b.port = n.port;
            b.name = n.name.clone();
            b.detail = if n.node == GRAPH_INPUT || n.node == GRAPH_OUTPUT { String::new() } else { "no part".into() };
        }
        out.boxes.push(b);
    }
    for (part, inn) in &inner {
        let o = origin(*part);
        for (k, node) in inn.nodes.iter().enumerate() {
            let (name, detail) = routing.nodes.iter().find(|n| n.node == *node).map_or((String::new(), String::new()), |n| (n.name.clone(), n.part_name.clone()));
            let r = inn.placed.rects[k];
            out.boxes.push(LBox { kind: BoxKind::Node, node: *node, port: 0, part: *part, name, detail, chips: vec![], bounds: Rect { x: o.x as i32 + r.x, y: o.y as i32 + r.y, w: r.w, h: r.h } });
        }
    }
    // Paths: inside a part to its frame's edge, across the view, into the other part.
    let shifted = |part: u64, points: &[Pt]| -> Vec<Pt> {
        let o = origin(part);
        points.iter().map(|q| Pt { x: q.x + o.x, y: q.y + o.y }).collect()
    };
    for (k, e) in top_edges.iter().enumerate() {
        let i = top_wire[k];
        let w = &g.wires[i];
        let mut points: Vec<Pt> = vec![];
        let (pa, pb) = (tops[e.from].part, tops[e.to].part);
        if let Some(le) = leave_edge.get(&i) {
            let mut p = shifted(pa, &inner[&pa].placed.paths[*le]);
            if let Some(last) = p.last_mut() {
                last.x = f64::from(placed.rects[e.from].x + placed.rects[e.from].w);
            }
            points = p;
        }
        let across = &placed.paths[k];
        let skip = usize::from(!points.is_empty());
        let end = across.len() - usize::from(enter_edge.contains_key(&i));
        points.extend_from_slice(&across[skip.min(end)..end]);
        if let Some(ee) = enter_edge.get(&i) {
            let mut p = shifted(pb, &inner[&pb].placed.paths[*ee]);
            if let Some(first) = p.first_mut() {
                first.x = f64::from(placed.rects[e.to].x);
            }
            points.extend(p);
        }
        out.paths.push(LPath { wire: i, state: wire_state(w, g), points });
    }
    for (i, e) in &inner_edge {
        let part = part_of[&g.wires[*i].from];
        out.paths.push(LPath { wire: *i, state: wire_state(&g.wires[*i], g), points: shifted(part, &inner[&part].placed.paths[*e]) });
    }
    // Control lines: each plugin's box, or the collapsed part's that holds it.
    let box_of = |boxes: &[LBox], node: u64| -> Option<Rect> {
        if let Some(b) = boxes.iter().find(|b| b.kind == BoxKind::Node && b.node == node) {
            return Some(b.bounds);
        }
        let p = part_of.get(&node)?;
        boxes.iter().find(|b| b.kind == BoxKind::Part && b.part == *p).map(|b| b.bounds)
    };
    let bottom = |r: Rect, shift: f64| Pt { x: f64::from(r.x) + f64::from(r.w) / 2.0 + shift, y: f64::from(r.y + r.h) };
    for map in &g.mappings {
        if let (Some(a), Some(b)) = (box_of(&out.boxes, map.source), box_of(&out.boxes, map.target)) {
            out.links.push(Link { source: map.source, lane: map.lane, macro_id: 0, target: map.target, from_control: false, from: bottom(a, -8.), to: bottom(b, 8.), points: vec![] });
        }
    }
    let mut right = 0; // the row of control nodes, under the graph, each under its plugin where it can be
    let row = out.height + m.gap / 2;
    for c in &g.macros {
        let Some((source_node, source_lane)) = c.source else { continue };
        let Some(source) = box_of(&out.boxes, source_node) else { continue };
        let node = ControlNode { id: c.id, name: c.name.clone(), bounds: Rect { x: right.max(source.x + source.w / 2 - m.control / 2), y: row, w: m.control, h: m.control_height } };
        right = node.bounds.x + node.bounds.w + m.slot;
        let r = node.bounds;
        out.controls.push(node);
        out.links.push(Link { source: source_node, lane: source_lane, macro_id: c.id, target: 0, from_control: false, from: bottom(source, 0.), to: Pt { x: f64::from(r.x) + f64::from(r.w) / 4.0, y: f64::from(r.y) }, points: vec![] }); // into the left of its top
        let mut drawn: HashSet<u64> = HashSet::new();
        for t in &c.nodes {
            if drawn.insert(*t) {
                if let Some(b) = box_of(&out.boxes, *t) {
                    out.links.push(Link { source: source_node, lane: source_lane, macro_id: c.id, target: *t, from_control: true, from: Pt { x: f64::from(r.x) + f64::from(r.w) * 3.0 / 4.0, y: f64::from(r.y) }, to: bottom(b, 8.), points: vec![] }); // out of the right of its top
                }
            }
        }
    }
    if !out.controls.is_empty() {
        out.height = row + m.control_height;
        out.width = out.width.max(right);
    }
    // Each line's way around the boxes: plugins, collapsed parts, control nodes, and the frames of parts it
    // neither leaves nor enters.
    let within = |outer: Rect, inner: Option<Rect>| inner.is_some_and(|i| i.x >= outer.x && i.y >= outer.y && i.x + i.w <= outer.x + outer.w && i.y + i.h <= outer.y + outer.h);
    let (width, height) = (out.width, out.height);
    let boxes = out.boxes.clone();
    let controls = out.controls.clone();
    for l in &mut out.links {
        let source = box_of(&boxes, l.source);
        let target = if l.target != 0 { box_of(&boxes, l.target) } else { None };
        let mut obstacles: Vec<Rect> = boxes.iter().filter(|b| b.kind != BoxKind::Frame || !(within(b.bounds, source) || within(b.bounds, target))).map(|b| b.bounds).collect();
        obstacles.extend(controls.iter().map(|c| c.bounds));
        let (down, up) = (Pt { x: 0., y: 1. }, Pt { x: 0., y: -1. });
        l.points = route_around(l.from, if l.from_control { up } else { down }, l.to, if l.target != 0 { up } else { down }, &obstacles, f64::from(m.row) / 2.0, Rect { x: 0, y: 0, w: width, h: height }); // half the closest rows' gap
    }
    out
}
