// SPDX-License-Identifier: AGPL-3.0-or-later
//! Routing's model and layout (src/wires.rs): read as PartsModel.cpp reads graph.get, laid out in columns by
//! signal flow with the properties the JUCE test (hosts/standalone/tests/parts_model.cpp) holds it to: no
//! wire passes under a box, boxes do not overlap, a collapsed part is a box and an expanded one a frame holding
//! its plugins; wires leave and enter at the box edges; control lines run in right angles around the boxes.

use crate::wires::*;
use serde_json::json;
use std::collections::BTreeSet;

const INPUT: u64 = 0xFFFF_FFFE;
const OUTPUT: u64 = 0xFFFF_FFFF;

fn node(id: u64, name: &str, plugin: &str, inputs: u64, outputs: u64, notes_in: u64) -> serde_json::Value {
    let ports = |n: u64| if n > 0 { json!([{"id": 0, "name": "Main", "channels": n}]) } else { json!([]) };
    json!({"id": id, "name": name, "plugin": {"id": plugin, "name": name, "lanes": []}, "audioInputs": ports(inputs), "audioOutputs": ports(outputs), "noteInputs": notes_in, "noteOutputs": 0, "enabled": true, "ringing": false})
}

fn wire(id: u64, notes: bool, part: Option<u64>, from: u64, to: u64) -> serde_json::Value {
    let mut w = json!({"id": id, "kind": if notes { "notes" } else { "audio" }, "owner": "user", "from": from, "to": to, "fromPort": 0, "toPort": 0, "channels": [],
                       "level": 1.0, "pan": 0.0, "mute": false, "solo": false, "zones": [], "send": 0, "split": 0, "lane": 0, "fader": false, "connected": true, "enabled": true});
    if let Some(p) = part {
        w["owner"] = json!("part");
        w["part"] = json!(p);
    }
    w
}

/// An audio wire from the graph's input to the plugin outside the parts, two channels by their pairs.
fn pairs() -> serde_json::Value {
    let mut w = wire(9, false, None, INPUT, 8);
    w["channels"] = json!([[0, 0], [1, 1]]);
    w
}

/// Two parts (Keys: a chorus then a Rhodes; Bass: a synth), a bus, a plugin outside every part that both parts
/// are wired into, and the output; the keys and MIDI into both parts.
pub fn graph() -> serde_json::Value {
    let mut chorus = node(2, "Chorus", "fx.chorus", 2, 2, 0);
    chorus["enabled"] = json!(true);
    json!({
        "audioInputs": [{"id": 0, "name": "Line in", "channels": 2}],
        "audioOutputs": [{"id": 0, "name": "Main out", "channels": 2}],
        "nodes": [node(1, "Rhodes", "syn.rhodes", 0, 2, 1), chorus, node(3, "Bass", "syn.bass", 0, 1, 1), node(4, "Hall", "fx.hall", 2, 2, 0), node(8, "Master FX", "fx.master", 2, 2, 0)],
        "parts": [
            {"id": 10, "name": "Keys", "level": 0.5, "mute": false, "solo": false, "chain": [2, 1], "bus": 0, "sends": [{"bus": 12, "level": 0.5}], "input": {"kind": "notes", "zones": [], "devices": []}},
            {"id": 11, "name": "Bass", "level": 1.0, "mute": false, "solo": false, "chain": [3], "bus": 0, "sends": [], "input": {"kind": "notes", "zones": [], "devices": []}},
            {"id": 12, "name": "Reverb", "level": 0.8, "mute": false, "solo": false, "chain": [4], "bus": 0, "sends": [], "input": {"kind": "bus"}},
        ],
        "wires": [
            wire(1, true, Some(10), INPUT, 1), wire(2, false, Some(10), 1, 2), wire(3, false, Some(10), 2, 8),
            wire(4, true, Some(11), INPUT, 3), wire(5, false, Some(11), 3, 8),
            wire(6, false, None, 8, OUTPUT), wire(7, false, Some(10), 2, 4), wire(8, false, Some(12), 4, OUTPUT),
            pairs(),
        ],
        "connections": [], "mappings": [], "controls": [],
    })
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// The curve a wire is drawn as: a cubic through each pair of points with its control points at their middle
/// in x (the JUCE drawing and the Routing screen's), sampled.
fn curve(points: &[Pt]) -> Vec<Pt> {
    let mut out = vec![];
    for (i, w) in points.windows(2).enumerate() {
        let (p, q) = (w[0], w[1]);
        let mid = (p.x + q.x) / 2.;
        for s in if i == 0 { 0 } else { 1 }..=16 {
            let t = f64::from(s) / 16.;
            let u = 1. - t;
            out.push(Pt { x: u * u * u * p.x + 3. * u * u * t * mid + 3. * u * t * t * mid + t * t * t * q.x, y: u * u * u * p.y + 3. * u * u * t * p.y + 3. * u * t * t * q.y + t * t * t * q.y });
        }
    }
    out
}

// readRouting: the keys and MIDI and each audio input first, then the nodes in part order (a part's in id
// order) and the rest, then each output; role, part and user wires of each.
#[test]
fn routing_reads_ends_then_parts_nodes_then_the_rest() {
    let g = Graph::read(&graph());
    let r = read_routing(&g);
    assert_eq!(r.nodes.iter().map(|n| (n.name.as_str(), n.role.as_str(), n.part)).collect::<Vec<_>>(), [
        ("Keys & MIDI", "Notes", 0), ("Line in", "Audio", 0), ("Rhodes", "Instrument", 10), ("Chorus", "Effect", 10), ("Bass", "Instrument", 11), ("Hall", "Effect", 12), ("Master FX", "Effect", 0), ("Main out", "Output", 0)
    ]);
    assert_eq!(r.nodes[3].part_name, "Keys");
    assert!(r.nodes[6].user_wires && !r.nodes[2].user_wires, "Master FX has wires drawn by hand");
    assert_eq!(r.find(INPUT, NOTES_PORT).unwrap().name, "Keys & MIDI");
    assert_eq!(r.find(INPUT, 0).unwrap().name, "Line in");
    assert_eq!(r.find(3, 7).unwrap().name, "Bass", "a plugin by node alone");
    assert_eq!(g.parts[0].chips, ["Chorus", "Rhodes"]);
}

// Wires: enabled; disabled when muted, off or on a plugin that is; draining on a plugin still ringing out or a
// note connection draining; an audio wire carries the channels its pairs take, else its source port's.
#[test]
fn wire_states_and_channels() {
    let mut j = graph();
    let mut g = Graph::read(&j);
    assert_eq!(wire_state(&g.wires[1], &g), WireState::Enabled);
    j["wires"][1]["mute"] = json!(true);
    j["nodes"][3]["enabled"] = json!(false); // Hall off: its wires
    j["nodes"][2]["ringing"] = json!(true); // Bass still sounding out
    j["connections"] = json!([{"kind": "notes", "from": INPUT, "to": 1, "draining": true}]);
    g = Graph::read(&j);
    assert_eq!(wire_state(&g.wires[1], &g), WireState::Disabled);
    assert_eq!(wire_state(&g.wires[6], &g), WireState::Disabled, "to a plugin that is off");
    assert_eq!(wire_state(&g.wires[3], &g), WireState::Draining, "from a plugin that rings out");
    assert_eq!(wire_state(&g.wires[0], &g), WireState::Draining, "a note connection draining");
    assert_eq!(wire_state(&g.wires[7], &g), WireState::Disabled, "from a plugin that is off");
    assert_eq!(wire_channels(&g.wires[1], &g), 2, "the source port's");
    assert_eq!(wire_channels(&g.wires[0], &g), 1, "notes");
    assert_eq!(wire_channels(&g.wires[8], &g), 2, "two source channels in its pairs");
    j["wires"][8]["channels"] = json!([[0, 0], [0, 1]]);
    assert_eq!(wire_channels(&Graph::read(&j).wires[8], &Graph::read(&j)), 1, "one source channel to two");
    j["wires"][8]["channels"] = json!([]);
    assert_eq!(wire_channels(&Graph::read(&j).wires[8], &Graph::read(&j)), 2, "from the graph's input port");
    assert!(g.parts[1].draining && !g.parts[0].draining, "the part holding the ringing plugin");
}

// inputsOf: a node's inputs with sources, a Mix row per source, named by the part that made the wire; the
// graph's output by its ports; notes apart.
#[test]
fn inputs_of_lists_each_source_into_an_input() {
    let g = Graph::read(&graph());
    let master = inputs_of(&g, 8);
    assert_eq!(master.len(), 1);
    assert_eq!((master[0].port, master[0].name.as_str()), (0, "Main"));
    assert_eq!(master[0].rows.iter().map(|r| (r.source.as_str(), r.user, r.keys)).collect::<Vec<_>>(), [("Chorus (Keys)", false, false), ("Bass (Bass)", false, false), ("Audio in", true, true)]);
    let out = inputs_of(&g, OUTPUT);
    assert_eq!((out[0].name.as_str(), out[0].rows.len()), ("Main out", 2));
    let notes = inputs_of(&g, 1);
    assert_eq!((notes[0].port, notes[0].name.as_str(), notes[0].rows[0].source.as_str()), (NOTES_PORT, "Notes", "Keys & MIDI (Keys)"));
    assert!(inputs_of(&g, 99).is_empty());
}

// The layout, collapsed and expanded, as the JUCE test holds it: no wire passes under a box, boxes do not
// overlap, a box per part collapsed and a frame per part with its plugins inside expanded, fewer wires collapsed.
#[test]
fn the_layout_has_no_overlap_no_wire_under_a_box_and_a_box_or_frame_per_part() {
    let g = Graph::read(&graph());
    let mut counts = vec![];
    for collapsed in [true, false] {
        let l = layout_routing(&g, collapsed, &BTreeSet::new(), &Metrics::default());
        let name = if collapsed { "collapsed" } else { "expanded" };
        for p in &l.paths {
            for pt in curve(&p.points) {
                for b in l.boxes.iter().filter(|b| b.kind != BoxKind::Frame) {
                    let r = b.bounds;
                    assert!(!(pt.x > f64::from(r.x) + 1e-6 && pt.x < f64::from(r.x + r.w) - 1e-6 && pt.y > f64::from(r.y) && pt.y < f64::from(r.y + r.h)), "{name}: a wire passes under {}", b.name);
                }
            }
        }
        for (i, a) in l.boxes.iter().enumerate() {
            for b in &l.boxes[i + 1..] {
                if a.kind == BoxKind::Frame && b.part == a.part {
                    continue; // a frame holds its plugins
                }
                assert!(!overlap(a.bounds, b.bounds), "{name}: {} and {} overlap", a.name, b.name);
            }
        }
        let parts = l.boxes.iter().filter(|b| b.kind == if collapsed { BoxKind::Part } else { BoxKind::Frame }).count();
        let in_parts = l.boxes.iter().filter(|b| b.kind == BoxKind::Node && b.part != 0).count();
        assert_eq!(parts, 3, "{name}: a box or frame per part and bus");
        assert_eq!(in_parts > 0, !collapsed, "{name}");
        for b in &l.boxes {
            assert!(b.bounds.x >= 0 && b.bounds.y >= 0 && b.bounds.x + b.bounds.w <= l.width && b.bounds.y + b.bounds.h <= l.height, "{name}: {} inside the drawing", b.name);
        }
        counts.push(l.paths.len());
    }
    assert!(counts[0] < counts[1], "collapsed, fewer wires: {counts:?}");
    // One part expanded, the rest collapsed.
    let one = layout_routing(&g, true, &BTreeSet::from([11]), &Metrics::default());
    assert_eq!(one.boxes.iter().filter(|b| b.kind == BoxKind::Frame && b.part == 11).count(), 1);
    assert!(!one.boxes.iter().any(|b| b.kind == BoxKind::Part && b.part == 11));
    assert_eq!(one.boxes.iter().filter(|b| b.kind == BoxKind::Part).count(), 2);
}

// Columns by signal flow: the keys and MIDI and the audio input first, the output last, each box after what
// feeds it; a wire leaves at the right edge of its source and enters at the left edge of its target at the
// heights of their sides, and runs only left to right through the gaps.
#[test]
fn boxes_are_in_columns_and_wires_meet_the_box_edges() {
    let g = Graph::read(&graph());
    let l = layout_routing(&g, true, &BTreeSet::new(), &Metrics::default());
    let bounds = |name: &str| l.boxes.iter().find(|b| b.name == name).unwrap().bounds;
    let (keys, line, keys_part, bass_part, hall, master, out) = (bounds("Keys & MIDI"), bounds("Line in"), bounds("Keys"), bounds("Bass"), bounds("Reverb"), bounds("Master FX"), bounds("Main out"));
    assert_eq!((keys.x, line.x), (0, 0), "the ends first");
    assert!(keys_part.x > keys.x && keys_part.x == bass_part.x, "the parts one column after the keys");
    assert!(master.x > keys_part.x, "the plugin they feed after them");
    assert!(out.x > master.x && out.x > hall.x, "the output last");
    for p in &l.paths {
        let w = &g.wires[p.wire];
        let (first, last) = (p.points[0], p.points[p.points.len() - 1]);
        let from = l.boxes.iter().find(|b| b.kind != BoxKind::Frame && (if w.from == INPUT { b.node == INPUT && b.port == if w.notes { NOTES_PORT } else { w.from_port } } else if let Some(n) = g.owner(w.from) { b.kind == BoxKind::Part && b.part == n.id } else { b.node == w.from })).unwrap();
        let to = l.boxes.iter().find(|b| b.kind != BoxKind::Frame && (if w.to == OUTPUT { b.node == OUTPUT } else if let Some(n) = g.owner(w.to) { b.kind == BoxKind::Part && b.part == n.id } else { b.node == w.to })).unwrap();
        assert!(near(first.x, f64::from(from.bounds.x + from.bounds.w)) && first.y > f64::from(from.bounds.y) && first.y < f64::from(from.bounds.y + from.bounds.h), "wire {} leaves its source's right edge: {first:?} {:?}", w.id, from.bounds);
        assert!(near(last.x, f64::from(to.bounds.x)) && last.y > f64::from(to.bounds.y) && last.y < f64::from(to.bounds.y + to.bounds.h), "wire {} enters its target's left edge", w.id);
        assert!(p.points.windows(2).all(|q| q[1].x >= q[0].x), "wire {} runs left to right", w.id);
    }
    // Wires wholly inside a collapsed part are not drawn; the ones that cross out of it are, from its box.
    assert!(l.paths.iter().all(|p| g.wires[p.wire].id != 2), "the wire inside Keys is not drawn collapsed");
    let e = layout_routing(&g, false, &BTreeSet::new(), &Metrics::default());
    assert!(e.paths.iter().any(|p| g.wires[p.wire].id == 2), "but it is expanded");
    // Two wires leaving one box leave at their own heights, in the order of what they go to.
    let ends: Vec<f64> = l.paths.iter().filter(|p| g.owner(g.wires[p.wire].from).is_some_and(|x| x.id == 10)).map(|p| p.points[0].y).collect();
    assert_eq!(ends.len(), 2, "the Keys part feeds Master FX and the Hall bus");
    assert!(ends[0] != ends[1], "at their own heights: {ends:?}");
}

// A wire back to an earlier column goes below everything, through the gaps either side, and the drawing grows
// to hold it.
#[test]
fn a_wire_back_to_an_earlier_column_goes_round_below() {
    let mut j = graph();
    j["wires"].as_array_mut().unwrap().push(wire(20, false, None, 8, 4)); // Master FX back to the Hall bus's plugin
    j["wires"].as_array_mut().unwrap().push(wire(21, false, None, 4, 8));
    let g = Graph::read(&j);
    let m = Metrics::default();
    let l = layout_routing(&g, true, &BTreeSet::new(), &m);
    let back = l.paths.iter().filter(|p| p.points.windows(2).any(|q| q[1].x < q[0].x)).count();
    assert!(back >= 1, "a loop has a wire that goes back");
    let before = layout_routing(&Graph::read(&graph()), true, &BTreeSet::new(), &m).height;
    assert!(l.height >= before, "the drawing holds it");
}

// Control lines: a plugin's lane to the control it drives (a node under the graph, below its plugin), and from
// the control to each plugin its parameters are in; a lane straight to a parameter's plugin. Each line runs
// in straight horizontal and vertical runs, from where it starts to where it ends, around every box.
#[test]
fn control_lines_run_in_right_angles_around_the_boxes() {
    let mut j = graph();
    j["nodes"][0]["plugin"]["lanes"] = json!([{"lane": 0, "name": "Mod"}]);
    j["controls"] = json!([{"id": 5, "part": 0, "name": "Wobble", "value": 0.5, "source": {"node": 1, "lane": 0}, "targets": [
        {"kind": "parameter", "node": 4, "param": 1}, {"kind": "parameter", "node": 4, "param": 2}, {"kind": "partLevel", "part": 11}]}]);
    j["mappings"] = json!([{"from": 3, "lane": 0, "to": 8, "param": 3}]);
    let g = Graph::read(&j);
    for collapsed in [true, false] {
        let l = layout_routing(&g, collapsed, &BTreeSet::new(), &Metrics::default());
        assert_eq!(l.controls.len(), 1, "a control node while a lane drives it");
        assert_eq!(l.links.len(), 3, "lane to control, control to the one plugin its parameters are in (twice, once), a lane to a parameter");
        let c = l.controls[0].bounds;
        assert!(c.y >= l.height - 30 - 1 && l.height >= c.y + c.h, "the control sits under the graph");
        for k in &l.links {
            assert!(k.points.len() >= 2 && k.points[0] == k.from && *k.points.last().unwrap() == k.to, "from where it starts to where it ends");
            for w in k.points.windows(2) {
                assert!(w[0].x == w[1].x || w[0].y == w[1].y, "{collapsed}: straight runs and right angles: {w:?}");
            }
        }
        let direct = l.links.iter().find(|k| k.macro_id == 0).unwrap();
        assert_eq!((direct.source, direct.target), (3, 8));
        let into = l.links.iter().find(|k| k.macro_id == 5 && !k.from_control).unwrap();
        assert_eq!((into.source, into.target), (1, 0));
        assert!(near(into.to.x, f64::from(c.x) + f64::from(c.w) / 4.) && near(into.to.y, f64::from(c.y)), "into the left of the control's top");
        let out = l.links.iter().find(|k| k.from_control).unwrap();
        assert_eq!(out.target, 4);
        assert!(near(out.from.x, f64::from(c.x) + f64::from(c.w) * 3. / 4.), "out of the right of its top");
        // No run of a line passes through a plugin or part box (a frame it neither leaves nor enters is kept off too).
        for k in &l.links {
            for w in k.points.windows(2) {
                for b in l.boxes.iter().filter(|b| b.kind != BoxKind::Frame) {
                    let r = b.bounds;
                    let (x0, x1, y0, y1) = (w[0].x.min(w[1].x), w[0].x.max(w[1].x), w[0].y.min(w[1].y), w[0].y.max(w[1].y));
                    let through = x1 > f64::from(r.x) && x0 < f64::from(r.x + r.w) && y1 > f64::from(r.y) && y0 < f64::from(r.y + r.h);
                    // The ends of a line touch the bottom edge of the box it starts or ends at.
                    let touches = (near(y0, f64::from(r.y + r.h)) || near(y1, f64::from(r.y + r.h))) && !(y0 < f64::from(r.y + r.h) - 1e-6 && y1 > f64::from(r.y + r.h) + 1e-6);
                    assert!(!through || touches || x0 == x1 && (near(x0, f64::from(r.x)) || near(x0, f64::from(r.x + r.w))), "{collapsed}: a line passes through {}: {w:?} {r:?}", b.name);
                }
            }
        }
    }
    // No lane driving a control: no control node, as the JUCE page keeps knob-only controls off the view.
    let mut j = graph();
    j["controls"] = json!([{"id": 5, "part": 0, "name": "Knob", "value": 0.5, "source": null, "targets": []}]);
    assert!(layout_routing(&Graph::read(&j), true, &BTreeSet::new(), &Metrics::default()).controls.is_empty());
}

// The same graph always lays out the same way, and an empty one has the two ends.
#[test]
fn the_layout_is_stable() {
    let g = Graph::read(&graph());
    let m = Metrics::default();
    assert_eq!(layout_routing(&g, true, &BTreeSet::new(), &m), layout_routing(&g, true, &BTreeSet::new(), &m));
    let empty = Graph::read(&json!({"audioInputs": [], "audioOutputs": [{"id": 0, "name": "", "channels": 2}], "nodes": [], "parts": [], "wires": []}));
    let l = layout_routing(&empty, true, &BTreeSet::new(), &m);
    assert_eq!(l.boxes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Keys & MIDI", "Output"]);
    assert!(l.paths.is_empty() && l.width > 0 && l.height > 0);
}
