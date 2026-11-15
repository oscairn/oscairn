// SPDX-License-Identifier: AGPL-3.0-or-later
//! The grid's rich views (src/views.rs, `ViewPanel` in slint/grid.slint) against the engine's words: touch
//! on a handle sets the parameters it stands for inside their gestures and nothing else does, the node the
//! cursor's tile sets is marked.

use super::*;

fn sent(out: &mut Vec<crate::params::Out>) -> Vec<(&'static str, serde_json::Value)> {
    std::mem::take(out).into_iter().map(|(_, m, p)| (m, p)).collect()
}

fn panels(ui: &Shell) -> Vec<crate::GridPanel> {
    ui.global::<crate::GridState>().get_panels().iter().collect()
}

fn set(id: u32, value: f64) -> (&'static str, serde_json::Value) {
    ("params.set", json!({"values": [{"node": 2, "param": id, "value": value}]}))
}

fn gesture(id: u32, begin: bool) -> (&'static str, serde_json::Value) {
    ("params.gesture", json!({"node": 2, "param": id, "begin": begin}))
}

// A node taken by touch and dragged sets its time (across) and its level (up and down) in gestures of its
// own; a touch away from every node takes nothing (the page scrolls) and changes nothing; the node the
// cursor's tile sets is marked, and only that.
#[test]
fn an_envelope_node_is_dragged_by_touch() {
    let mut s = Screen::new();
    let ui = &s.ui.clone_strong();
    let mut p = super::grid::open(&mut s, super::grid::every_kind(), &[(9, 0.1), (10, 0.4), (11, 0.6), (12, 0.5)]);
    sent(&mut p.out);
    let before = panels(ui)[0].clone();
    assert_eq!(before.points.row_count(), 3, "attack, decay, release");
    // Where a node is now.
    let node = |k: usize| {
        let q = panels(ui)[0].points.row_data(k).unwrap();
        (q.x, q.y)
    };
    // Away from every node: nothing is taken.
    assert!(!p.view_down(0, node(0).0 + 100., node(0).1 + 60.));
    assert!(sent(&mut p.out).is_empty(), "no gesture, no change");
    // The decay node carries the decay time and the sustain level; dragging it past the plot's top and right.
    let (x, y) = node(1);
    assert!(p.view_down(0, x + 3., y - 2.));
    p.view_move(0, 1000., -100.);
    p.view_up(0);
    let out = sent(&mut p.out);
    assert_eq!(out.first().unwrap(), &gesture(10, true));
    assert!(out.contains(&gesture(11, true)));
    assert!(out.contains(&set(10, 1.)), "the time at the end of its segment: {out:?}");
    assert!(out.contains(&set(11, 1.)), "the level at the top");
    assert!(out.contains(&gesture(10, false)) && out.contains(&gesture(11, false)), "the gestures end with the touch");
    assert_eq!(out.iter().filter(|o| o.0 == "params.set").count(), 2, "only what the node stands for");
    // And the other way: to the left and the bottom.
    let (x, y) = node(1);
    assert!(p.view_down(0, x, y));
    p.view_move(0, -1000., 1000.);
    p.view_up(0);
    let out = sent(&mut p.out);
    assert!(out.contains(&set(10, 0.)) && out.contains(&set(11, 0.)), "{out:?}");
    // A node with only a time (the attack: its level is fixed at full) sets only the time.
    let (x, y) = node(0);
    assert!(p.view_down(0, x, y));
    p.view_move(0, -1000., 0.);
    p.view_up(0);
    let out = sent(&mut p.out);
    assert_eq!(out.iter().filter(|o| o.0 == "params.set").collect::<Vec<_>>(), [&set(9, 0.)], "only the time: {out:?}");
    assert!(out.contains(&gesture(9, true)) && !out.iter().any(|o| o.1["param"] == 11), "no gesture on a parameter it does not stand for");
    // The cursor's tile marks its node: nothing is marked until a tile with a node has the cursor.
    assert!(panels(ui)[0].points.iter().all(|q| !q.lit));
    p.act(crate::nav::Action::Move { pane: None, by: 1 });
    p.act(crate::nav::Action::Move { pane: None, by: 8 });
    let tile = ui.global::<crate::GridState>().get_tiles().iter().find(|t| t.cursor).map(|t| t.name.to_string());
    assert_eq!(tile.as_deref(), Some("Attack"));
    let lit: Vec<bool> = panels(ui)[0].points.iter().map(|q| q.lit).collect();
    assert_eq!(lit, [true, false, false], "the attack's node");
    p.act(crate::nav::Action::Move { pane: None, by: 2 });
    let lit: Vec<bool> = panels(ui)[0].points.iter().map(|q| q.lit).collect();
    assert_eq!(lit, [false, true, false], "the sustain tile sets the decay node's level");
    s.frame();
    png(&s, "grid-envelope-cursor");
}
