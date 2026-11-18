// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the panel's counted steps do to a list (the owner, 2026-11-17: "the knob under the screen and the jogwheel jumped 3 or 4
//! list items at once"). The engine counts what the panel did to the lists (`controllers`' navigation: `move` for the VALUE
//! dial, `move:0` and `move:1` for the knobs under the screen at the left and the right) and the program moves the cursor by
//! as many rows as it was told: one counted step, one row; a message of N steps (the panel's messages carry up to 6: a fast
//! turn) N rows. How many counts one click of the dial sends is the panel's: nothing in this program multiplies them.

use super::library::rows;
use super::library_shell::Shelf;
use super::owner::scene;
use super::*;
use crate::LibraryState;

/// The totals the engine would give after the dial counted `dial`, the left knob `left` and the right knob `right` steps.
fn counted(s: &mut Shelf, dial: i64, left: i64, right: i64) {
    s.f.engine.send(&format!("{}\n", json!({"jsonrpc": "2.0", "method": "controllers", "params": {"navigation": {"move": dial, "move:0": left, "move:1": right}}})));
    s.f.pump();
}

fn row_at(ui: &Shell) -> Option<usize> {
    rows(ui).iter().position(|r| r.cursor)
}

fn side_at(ui: &Shell) -> Option<usize> {
    ui.global::<LibraryState>().get_side_rows().iter().position(|r| r.cursor)
}

// The dial: the first step shows the cursor on the first row, then one counted step is one row, and four in one message four.
// The left knob: the first step shows the cursor on the sidebar's first line (it does not also move it), then one line a step.
#[test]
fn one_counted_step_is_one_row_and_n_steps_n_rows() {
    let mut s = scene("panel-steps", 10);
    let ui = s.ui();
    counted(&mut s, 0, 0, 0); // the first read is the baseline
    counted(&mut s, 1, 0, 0);
    assert_eq!(row_at(&ui), Some(0), "the first step shows the cursor");
    counted(&mut s, 2, 0, 0);
    assert_eq!(row_at(&ui), Some(1), "one step, one row");
    counted(&mut s, 6, 0, 0);
    assert_eq!(row_at(&ui), Some(5), "a message of four steps, four rows");
    counted(&mut s, 5, 0, 0);
    assert_eq!(row_at(&ui), Some(4), "one step back, one row");
    // The left knob goes to the sidebar: its first step shows the cursor on the first line.
    counted(&mut s, 5, 1, 0);
    assert_eq!((side_at(&ui), row_at(&ui)), (Some(0), None), "the first step of the knob lands on the first line, not past it");
    counted(&mut s, 5, 2, 0);
    assert_eq!(side_at(&ui), Some(1), "one step, one line");
    counted(&mut s, 5, 5, 0);
    assert_eq!(side_at(&ui), Some(4), "three steps: Recent to the Libraries heading, All, My own");
}
