// SPDX-License-Identifier: AGPL-3.0-or-later
//! Importing another Performance's parts and scenes (ui/LIBRARY-APP.md D14) against the fake engine.

use super::library::*;
use super::library_items::options_for;
use super::*;

const IMPORT: usize = 10;

// A Performance's menu offers the import; its parts and scenes are listed all ticked; all of it is two empty lists, a part
// of it needs a part and a scene.
#[test]
fn parts_and_scenes_are_chosen_and_imported() {
    let mut r = Rig::new(library());
    let menu = options_for(&mut r, "Brass Corner");
    assert!(menu.iter().any(|m| m.0 == "Import into the open Performance..." && m.1));
    r.e.calls.clear();
    r.c.chosen(IMPORT);
    r.settle();
    assert_eq!(r.e.called("performance.contents"), [json!({"directory": "/library/performances/p4"})]);
    assert_eq!(r.c.path(), ["Library", "Performances", "Import from Brass Corner"]);
    assert_eq!(labels(r.ui()), ["Lead", "Bus A (bus)", "Verse", "Chorus", "Import", "Cancel"]);
    assert_eq!(rows(r.ui()).iter().map(|x| x.on).collect::<Vec<_>>(), [true, true, true, true, false, false]);
    // All of it: nothing named.
    r.e.calls.clear();
    r.press_row("Import");
    assert_eq!(r.e.called("performance.import"), [json!({"directory": "/library/performances/p4", "parts": [], "scenes": []})]);
    assert_eq!(footer(r.ui()).0, "Performance imported.");
    assert!(r.again, "the Performance changed under the screen");
    assert_eq!(r.c.path(), ["Library", "Performances"]);
    // Some: a part and a scene, by their IDs.
    options_for(&mut r, "Brass Corner");
    r.c.chosen(IMPORT);
    r.settle();
    r.press_row("Bus A (bus)");
    r.press_row("Chorus");
    r.e.calls.clear();
    r.press_row("Import");
    assert_eq!(r.e.called("performance.import"), [json!({"directory": "/library/performances/p4", "parts": [1], "scenes": [1]})]);
    // No scene: the scene sets how the parts come.
    options_for(&mut r, "Brass Corner");
    r.c.chosen(IMPORT);
    r.settle();
    r.press_row("Verse");
    r.press_row("Chorus");
    r.e.calls.clear();
    r.press_row("Import");
    assert!(r.e.calls.is_empty());
    assert_eq!(footer(r.ui()).0, "Choose at least one part, and one scene: the scene sets how the parts come.");
    r.press_row("Cancel");
    assert_eq!(r.c.path(), ["Library", "Performances"]);
}
