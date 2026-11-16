// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opening a Performance, the question about unsaved changes, a Sound made a new part and trying one in place
//! (ui/LIBRARY-APP.md rows B1 to B7), against the fake engine, which refuses everything else as busy while a load runs.

use super::library::*;
use super::*;
use crate::client::Error;
use crate::library::{Outcome, Request, Step};
use std::time::Duration;

fn open_steps(r: &Rig) -> Vec<String> {
    r.e.methods().into_iter().filter(|m| m != "library.query").collect()
}

// A press on a Performance opens it as the JUCE job does: what is unsaved, what it would cost, prepared while the
// open one plays, then opened; it says what is loading and, when it is open, that it is; the list is asked again (it
// is now the most recent) and the Performance is said to have changed.
#[test]
fn a_press_opens_a_performance_in_four_steps() {
    let mut r = Rig::new(library());
    r.e.calls.clear();
    r.press_row("Brass Corner");
    assert_eq!(open_steps(&r), ["performance.info", "budget.estimate", "performance.prepare", "library.load"]);
    assert_eq!(r.e.called("budget.estimate"), [json!({"directory": "/library/performances/p4"})]);
    assert_eq!(r.e.called("performance.prepare"), [json!({"directory": "/library/performances/p4"})]);
    assert_eq!(r.e.called("library.load"), [json!({"id": "p4"})]);
    assert_eq!(footer(r.ui()).0, "Performance opened. Press Play or play a note.");
    assert!(r.again, "the shell reads the Performance again");
    assert_eq!(r.e.open.1, "Brass Corner");
    assert_eq!(r.e.methods().last().unwrap(), "library.query", "listed again: it is now the most recent");
    assert!(crate::library::Screen::waits(&Request::Open(Step::Load)), "a long request");
}

// One that would not fit beside the outgoing one opens as a cut: not prepared, and it says so.
#[test]
fn one_that_would_not_fit_beside_the_open_one_opens_as_a_cut() {
    let mut r = Rig::new(library());
    r.e.seamless = false;
    r.e.calls.clear();
    r.press_row("Brass Corner");
    assert_eq!(open_steps(&r), ["performance.info", "budget.estimate", "library.load"]);
    assert_eq!(footer(r.ui()).0, "Performance opened. Press Play or play a note. Opened at once: it and the last Performance ringing out would not fit this device together.");
}

// The engine's refusal is said, with what was asked for ("Not opened"): a Performance that does not fit is refused.
#[test]
fn a_refused_opening_is_said() {
    let mut r = Rig::new(library());
    r.e.calls.clear();
    r.press_row("Glass Choir");
    assert_eq!(footer(r.ui()).0, "Not opened: Glass Choir does not fit this device: needs 3 GB of memory");
    assert!(!r.again);
    // The next opens as usual: the screen is not stuck on the failed one.
    r.press_row("Brass Corner");
    assert_eq!(footer(r.ui()).0, "Performance opened. Press Play or play a note.");
}

// While it loads the engine answers everything else as busy: each step is asked again after a moment, the screen says
// what is opening, and a second one chosen meanwhile replaces any waiting and opens when the first has.
#[test]
fn a_busy_step_is_asked_again_and_the_last_chosen_opens_next() {
    let mut r = Rig::new(library());
    r.e.calls.clear();
    r.e.busy = 2; // performance.info refused twice
    r.press_row("Brass Corner");
    assert_eq!(footer(r.ui()).0, "Opening Brass Corner...");
    assert_eq!(r.e.methods(), ["performance.info"]);
    assert_eq!(r.delayed.last(), Some(&("performance.info".to_string(), Duration::from_millis(100))));
    // A second and a third chosen meanwhile: only the last is kept.
    r.c.tap(3); // Evening Strings (row 3)
    r.c.tap(4); // Glass Choir
    r.c.tap(1); // Untitled
    assert_eq!(footer(r.ui()).0, "Untitled opens next");
    r.later_all();
    assert_eq!(r.e.methods().iter().filter(|m| *m == "performance.info").count(), 4, "refused twice, then asked, then the next one asks: {:?}", r.e.methods());
    // After the first opened, the one chosen last opened.
    assert_eq!(r.e.called("library.load"), [json!({"id": "p4"}), json!({"id": "p2"})]);
    assert_eq!(footer(r.ui()).0, "Performance opened. Press Play or play a note.");
}

// Unsaved changes are asked about first: Save (over it, or with a name for one never saved), Don't Save, Cancel.
#[test]
fn unsaved_changes_are_asked_about_before_opening() {
    let mut r = Rig::new(library());
    r.e.open = ("p1".into(), "Evening Strings".into(), true);
    r.e.calls.clear();
    r.press_row("Brass Corner");
    let (title, message, choices) = r.c.take_notice().expect("the question");
    assert_eq!(title, "Save changes to Evening Strings before opening Brass Corner?");
    assert_eq!(message, "Your changes will be lost if you don't save them.");
    assert_eq!(choices, ["Save", "Don't Save", "Cancel"]);
    assert_eq!(open_steps(&r), ["performance.info"], "nothing else until it is answered");
    // Save: over the Performance, then it opens.
    r.c.noticed(Some(0));
    r.settle();
    assert_eq!(open_steps(&r), ["performance.info", "library.save", "budget.estimate", "performance.prepare", "library.load"]);
    assert_eq!(r.e.called("library.save"), [json!({})]);
    // Don't Save: it opens.
    r.e.open = ("p1".into(), "Evening Strings".into(), true);
    r.e.calls.clear();
    r.press_row("Brass Corner");
    r.c.take_notice().unwrap();
    r.c.noticed(Some(1));
    r.settle();
    assert_eq!(open_steps(&r), ["performance.info", "budget.estimate", "performance.prepare", "library.load"]);
    // Cancel: nothing opens, and the next press is not blocked.
    r.e.open = ("p1".into(), "Evening Strings".into(), true);
    r.e.calls.clear();
    r.press_row("Brass Corner");
    r.c.take_notice().unwrap();
    r.c.noticed(None);
    r.settle();
    assert_eq!(open_steps(&r), ["performance.info"]);
    assert_eq!(footer(r.ui()).0, "");
    r.press_row("Brass Corner");
    assert!(r.c.take_notice().is_some());
}

// One never saved has its name asked for, then saved with it, then the other opens; a failed save is said and nothing opens.
#[test]
fn a_performance_never_saved_is_named_before_it_is_left() {
    let mut r = Rig::new(library());
    r.e.open = (String::new(), "Sketch".into(), true);
    r.e.calls.clear();
    r.press_row("Brass Corner");
    r.c.take_notice().unwrap();
    assert_eq!(r.c.noticed(Some(0)), Outcome::Prompt("Save: the Performance's name".into(), "Sketch".into()));
    r.c.entered("Sketch One");
    r.settle();
    assert_eq!(r.e.called("library.save"), [json!({"name": "Sketch One"})]);
    assert_eq!(r.e.called("library.load"), [json!({"id": "p4"})]);
    // A save that fails is said, and nothing opens.
    r.e.open = ("p1".into(), "Evening Strings".into(), true);
    r.e.fail.push(("library.save".into(), "The disk is full".into()));
    r.e.calls.clear();
    r.press_row("Evening Strings");
    r.c.take_notice().unwrap();
    r.c.noticed(Some(0));
    r.settle();
    assert_eq!(footer(r.ui()).0, "Not saved: The disk is full");
    assert!(r.e.called("library.load").is_empty());
}

// New part from a Sound: said, then "Sound loaded."; the Performance changed.
#[test]
fn a_sound_is_made_a_new_part() {
    let mut r = Rig::new(library());
    r.c.show_page(1);
    r.settle();
    r.e.calls.clear();
    r.press_row("Soft Felt"); // a press on a Sound lists what can be done
    let (title, items) = r.c.take_menu().expect("the Sound's menu");
    assert_eq!(title, "Soft Felt");
    assert_eq!(items[0].label, "New part");
    r.c.chosen(3);
    assert_eq!(footer(r.ui()).0, "Adding Soft Felt as a new part");
    r.settle();
    assert_eq!(r.e.called("parts.fromSound"), [json!({"id": "s1"})]);
    assert_eq!(footer(r.ui()).0, "Sound loaded.");
    assert!(r.again);
}

// Audition: a Performance in place of the open one, a Sound in the part the Part screen shows; the row says so, the
// bar's action reads Stop with Keep beside it; another row pressed meanwhile is tried; Stop puts back, Keep keeps.
#[test]
fn an_item_is_tried_in_place() {
    let mut r = Rig::new(library());
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: 2 }); // Brass Corner
    r.e.calls.clear();
    r.tool("Audition");
    assert_eq!(r.e.called("audition.start"), [json!({"id": "p4"})], "a Performance needs no part");
    assert_eq!(footer(r.ui()).0, "Auditioning Brass Corner in place of the open Performance.");
    assert_eq!(value_of(r.ui(), "Brass Corner"), "Auditioning  ·  brass  ·  warm");
    let tools: Vec<String> = r.c.bar_tools().into_iter().map(|t| t.label).collect();
    assert_eq!(tools, ["Search", "Stop", "Keep", "Options", "Install...", "Libraries..."]);
    // Browsing while one is tried: a press tries that one.
    r.press_row("Evening Strings");
    assert_eq!(r.e.called("audition.start").last().unwrap(), &json!({"id": "p1"}));
    assert!(r.e.called("library.load").is_empty(), "a press while trying does not open");
    r.tool("Keep");
    assert_eq!(r.e.called("audition.stop"), [json!({"keep": true})]);
    assert_eq!(footer(r.ui()).0, "Kept.");
    assert_eq!(r.c.bar_tools().iter().map(|t| t.label.clone()).collect::<Vec<_>>(), ["Search", "Audition", "Options", "Install...", "Libraries..."]);
    // Stop puts it back.
    r.tool("Audition");
    r.tool("Stop");
    assert_eq!(r.e.called("audition.stop").last().unwrap(), &json!({"keep": false}));
    assert_eq!(footer(r.ui()).0, "Put back as it was.");
}

// A Sound is tried in the part shown; with none shown it says to show one; while a Performance loads it says so.
#[test]
fn a_sound_needs_a_part_and_nothing_is_tried_while_loading() {
    let mut r = Rig::new(library());
    r.c.show_page(1);
    r.settle();
    r.c.part = 0;
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: 2 }); // Soft Felt
    r.e.calls.clear();
    r.tool("Audition");
    assert_eq!(footer(r.ui()).0, "Show a part in the Part view to audition a Sound in it.");
    assert!(r.e.calls.is_empty());
    r.c.part = 10;
    r.tool("Audition");
    assert_eq!(r.e.called("audition.start"), [json!({"id": "s1", "part": 10})]);
    assert_eq!(footer(r.ui()).0, "Auditioning Soft Felt in the part shown.");
    // Stopped elsewhere (a Performance opened keeps it): the marks go once the engine says so.
    r.e.auditioning = None;
    r.c.changed();
    r.settle();
    assert_eq!(value_of(r.ui(), "Soft Felt"), "muted piano".to_string().replace("muted piano", "keys  ·  warm   Muted piano"));
    // While one loads: "Still loading".
    r.c.show_page(0);
    r.settle();
    r.e.busy = 5;
    r.press_row("Brass Corner");
    r.e.calls.clear();
    r.tool("Audition");
    assert_eq!(footer(r.ui()).0, "Still loading; try again in a moment.");
    assert!(r.e.calls.is_empty());
}

// A refused audition is said and nothing is marked.
#[test]
fn a_refused_audition_is_said() {
    let mut r = Rig::new(library());
    r.e.fail.push(("audition.start".into(), "It would not fit".into()));
    r.act(Action::Move { pane: None, by: 1 });
    r.act(Action::Move { pane: None, by: 2 });
    r.tool("Audition");
    assert_eq!(footer(r.ui()).0, "Not auditioned: It would not fit");
    assert_eq!(value_of(r.ui(), "Brass Corner"), "brass  ·  warm");
    let _ = Error { code: 0, message: String::new() };
}
