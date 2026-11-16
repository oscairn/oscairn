// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library app against the real engine (ui/LIBRARY-APP.md): the engine of ui/REAL-ENGINE.md with a small test
//! library made by `library.save`, `parts.saveSound` and `setlists.save` (invented names), the shell driving the
//! screen as the dial and a tap do, each action checked in the engine's own state.

use super::*;

fn items(r: &mut Real, kind: &str) -> Vec<(String, String)> {
    let found = r.engine.call("library.query", json!({"kind": kind, "limit": 1000}));
    found["items"].as_array().unwrap().iter().map(|i| (i["name"].as_str().unwrap_or_default().to_string(), i["id"].as_str().unwrap_or_default().to_string())).collect()
}

fn shown(r: &Real) -> Vec<String> {
    r.rows().into_iter().map(|x| x.0).filter(|l| l != "Filters").collect()
}

/// Whether row `label` is the cursor's.
fn press_item(r: &mut Real, label: &str) {
    if !r.press(label) {
        r.note(format!("no row {label:?}"));
    }
}

/// A bar action by label, through "More" where the bar folds them.
fn bar_do(r: &mut Real, label: &str) -> bool {
    r.screen.frame();
    let ui = r.screen.ui.clone_strong();
    let labels: Vec<String> = ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).collect();
    if ui.global::<Bar>().get_more() {
        ui.global::<Bar>().invoke_more_tapped();
        r.run(100);
        let menu: Vec<String> = ui.global::<crate::MenuState>().get_rows().iter().map(|x| x.label.to_string()).collect();
        let Some(at) = menu.iter().position(|l| l.starts_with(label)) else {
            r.note(format!("the bar's More has no {label:?}: {menu:?}"));
            return false;
        };
        ui.global::<crate::MenuState>().invoke_tapped(at as i32);
    } else {
        let Some(at) = labels.iter().position(|l| l.starts_with(label)) else {
            r.note(format!("the bar has no {label:?}: {labels:?}"));
            return false;
        };
        ui.global::<Bar>().invoke_action(at as i32);
    }
    r.settle();
    true
}

/// A choice in the menu open now, by its label.
fn choose(r: &mut Real, label: &str) -> bool {
    let ui = r.screen.ui.clone_strong();
    r.run(100);
    let menu: Vec<String> = ui.global::<crate::MenuState>().get_rows().iter().map(|x| x.label.to_string()).collect();
    let Some(at) = menu.iter().position(|l| l == label) else {
        r.note(format!("the menu has no {label:?}: {menu:?}"));
        return false;
    };
    ui.global::<crate::MenuState>().invoke_tapped(at as i32);
    r.settle();
    true
}

/// Text for the keyboard open now, typed on a computer's keys (the profile's text entry is the typed kind here): what was in
/// the field is deleted first, then Return.
fn typed(r: &mut Real, text: &str) {
    for _ in 0..64 {
        shell::key(&slint::SharedString::from(slint::platform::Key::Backspace));
    }
    for ch in text.chars() {
        shell::key(&ch.to_string());
    }
    shell::key(&slint::SharedString::from(slint::platform::Key::Return));
    r.settle();
}

/// The line under the list.
fn said(r: &Real) -> String {
    r.screen.ui.global::<crate::PartState>().get_message().to_string()
}

pub(super) fn library(r: &mut Real) {
    let ui = r.screen.ui.clone_strong();
    shell::set_text_on_screen(false);
    r.step("Library: open");
    let connected = r.until(|ui| ui.global::<Bar>().get_cpu_known());
    r.check(connected, || "connect: the top bar never filled in".into());
    let home: Vec<String> = ui.global::<HomeState>().get_tiles().iter().map(|t| format!("{}: {}", t.name, t.line)).collect();
    eprintln!("   home tiles {home:?}");
    shell::goto(shell::Screen::Library);
    r.settle();
    r.run(600);
    r.png("lib-01-list");
    let sent = r.sent("Library open");
    eprintln!("   asked {:?}", sent.iter().map(|s| s.0.as_str()).collect::<Vec<_>>());
    let want: Vec<String> = items(r, "performance").into_iter().map(|i| i.0).collect();
    let got = shown(r);
    r.check(got == want, || format!("Library: the list shows {got:?}, the engine has {want:?}"));
    eprintln!("   rows {:?}; footer {:?}", r.rows(), ui.global::<crate::PartState>().get_view());

    // Search.
    r.step("Library: Search for warm");
    bar_do(r, "Search");
    typed(r, "warm");
    r.settle();
    r.sent("Search");
    let found = r.engine.call("library.query", json!({"kind": "performance", "text": "warm", "limit": 1000}));
    let names: Vec<String> = found["items"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap_or_default().to_string()).collect();
    let got = shown(r);
    r.check(got == names && !names.is_empty(), || format!("Search: the list shows {got:?}, the engine {names:?}"));
    r.png("lib-02-search");
    bar_do(r, "Search");
    typed(r, "");
    r.settle();

    // The filters: tags, favorites, library.
    r.step("Library: Filters, Tags");
    press_item(r, "Filters");
    r.press("Tags");
    r.settle();
    eprintln!("   tags {:?}", r.rows());
    r.png("lib-03-tags");
    r.press("warm");
    r.sent("Tag warm");
    let found = r.engine.call("library.query", json!({"kind": "performance", "tags": ["warm"], "limit": 1000}));
    r.check(found["total"].as_u64() == Some(2), || format!("Tags: warm matches {}", found["total"]));
    r.act(Action::Cancel(None));
    r.press("Favorites");
    r.sent("Favorites");
    r.png("lib-04-favorites");
    eprintln!("   with Favorites and warm: {:?}", shown(r));
    r.press("Favorites");
    r.act(Action::Cancel(None));
    r.press("Filters");
    r.press("Tags");
    r.press("warm"); // let go
    r.act(Action::Cancel(None));
    r.act(Action::Cancel(None));
    let rows = r.rows();
    let got = shown(r);
    r.check(rows.first().map(|x| x.0.as_str()) == Some("Filters") && got == want, || format!("Filters: the list did not come back: {rows:?}"));

    // A favorite, from the item's menu.
    r.step("Library: favorite");
    let id = items(r, "performance").into_iter().find(|i| i.0 == "Brass Hall").map(|i| i.1).unwrap_or_default();
    r.goto("Brass Hall");
    bar_do(r, "Options");
    choose(r, "Add to favorites");
    r.sent("Favorite");
    let now = r.engine.call("library.query", json!({"id": id}));
    r.check(now["items"][0]["favorite"] == true, || format!("Favorite: the engine has {}", now["items"][0]["favorite"]));
    r.check(r.rows().iter().any(|x| x.0 == "Brass Hall" && x.1.contains('★')), || "Favorite: the row has no star".into());
    r.goto("Brass Hall");
    bar_do(r, "Options");
    choose(r, "Remove from favorites");
    let now = r.engine.call("library.query", json!({"id": id}));
    r.check(now["items"][0]["favorite"] == false, || "Favorite: not taken out".into());

    // Edit, color, duplicate, delete.
    r.step("Library: edit Brass Hall");
    r.goto("Brass Hall");
    bar_do(r, "Options");
    choose(r, "Edit name, description and tags...");
    r.press("Name");
    typed(r, "Brass Hall Two");
    r.press("Tags");
    typed(r, "brass, loud");
    r.press("Save");
    r.sent("Edit");
    let now = r.engine.call("library.query", json!({"id": id}));
    r.check(now["items"][0]["name"] == "Brass Hall Two" && now["items"][0]["tags"] == json!(["brass", "loud"]), || format!("Edit: the engine has {}", now["items"][0]));
    let line = said(r);
    r.check(line == "Brass Hall Two saved.", || format!("Edit: the line says {line:?}"));
    r.png("lib-05-edited");
    r.step("Library: color");
    r.goto("Brass Hall Two");
    bar_do(r, "Options");
    choose(r, "Color");
    choose(r, "Blue");
    let now = r.engine.call("library.query", json!({"id": id}));
    r.check(now["items"][0]["color"] == "#3e63dd", || format!("Color: the engine has {}", now["items"][0]["color"]));
    r.step("Library: duplicate, then delete the copy");
    r.goto("Brass Hall Two");
    bar_do(r, "Options");
    choose(r, "Duplicate");
    r.sent("Duplicate");
    let all = items(r, "performance");
    r.check(all.iter().any(|i| i.0 == "Brass Hall Two copy"), || "Duplicate: no copy in the engine".into());
    r.goto("Brass Hall Two copy");
    bar_do(r, "Options");
    choose(r, "Delete...");
    r.png("lib-06-delete");
    let q = ui.global::<crate::NoticeState>().get_title().to_string();
    r.check(q == "Delete Brass Hall Two copy?", || format!("Delete: the question is {q:?}"));
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None));
    r.settle();
    let all = items(r, "performance");
    r.check(!all.iter().any(|i| i.0 == "Brass Hall Two copy"), || "Delete: the copy is still in the engine".into());
    // Put the original back as it was.
    r.goto("Brass Hall Two");
    bar_do(r, "Options");
    choose(r, "Edit name, description and tags...");
    r.press("Name");
    typed(r, "Brass Hall");
    r.press("Save");
    r.goto("Brass Hall");
    bar_do(r, "Options");
    choose(r, "Color");
    choose(r, "None");

    // Open a Performance: what the engine answers to each step, how long it takes, what it pushes.
    r.step("Library: open Glass Choir");
    r.goto("Glass Choir");
    r.run(100);
    let started = Instant::now();
    r.act(Action::Confirm(None));
    r.until(|ui| ui.global::<Bar>().get_performance() == "Glass Choir");
    r.settle();
    let opened = started.elapsed();
    r.run(4000); // the engine's graph.changed for a load comes about three seconds after its reply
    let sent = r.sent("Open");
    eprintln!("   opened in {opened:?}; steps {:?}", sent.iter().map(|s| s.0.as_str()).collect::<Vec<_>>());
    let now = r.engine.call("performance.info", json!({}));
    r.check(now["name"] == "Glass Choir", || format!("Open: the engine's Performance is {}", now["name"]));
    let line = said(r);
    r.check(line.starts_with("Performance opened."), || format!("Open: the line says {line:?}"));
    let notes: Vec<String> = r.lines().iter().skip(r.mark).filter_map(|l| if let Line::Note { method } = l { Some(method.clone()) } else { None }).filter(|m| m != "status" && m != "controllers").collect();
    eprintln!("   notifications after the load: {notes:?}");
    r.check(notes.iter().any(|n| n == "graph.changed"), || format!("Open: the engine pushed no graph.changed after the load (notifications {notes:?})"));
    r.png("lib-07-opened");
    let recent = r.engine.call("library.query", json!({"kind": "performance", "recent": true, "limit": 5}));
    r.check(recent["items"][0]["name"] == "Glass Choir", || format!("Open: the most recent is {}", recent["items"][0]["name"]));

    // The question about unsaved changes: an edit, then another Performance.
    r.step("Library: unsaved changes asked about");
    let part = r.engine.graph()["parts"][0]["id"].clone();
    r.engine.call("parts.name", json!({"part": part, "name": "Renamed"}));
    r.engine.call("graph.commit", json!({}));
    r.run(300);
    let info = r.engine.call("performance.info", json!({}));
    r.check(info["unsaved"] == true, || format!("Unsaved: the engine says {}", info["unsaved"]));
    r.goto("Night Strings");
    r.act(Action::Confirm(None));
    r.run(500);
    let q = ui.global::<crate::NoticeState>().get_title().to_string();
    r.check(q == "Save changes to Glass Choir before opening Night Strings?", || format!("Unsaved: the question is {q:?}"));
    r.png("lib-08-question");
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Move { pane: None, by: 1 });
    shell::act(Action::Confirm(None)); // Don't Save
    r.until(|ui| ui.global::<Bar>().get_performance() == "Night Strings");
    r.settle();
    let sent = r.sent("Don't Save");
    eprintln!("   steps {:?}", sent.iter().map(|s| s.0.as_str()).collect::<Vec<_>>());

    // Audition a Performance, keep nothing; a Sound in a part.
    r.step("Library: audition a Performance, stop");
    r.goto("Brass Hall");
    bar_do(r, "Audition");
    r.run(1500);
    let aud = r.engine.call("audition.get", json!({}));
    r.check(aud["kind"] == "performance", || format!("Audition: the engine has {aud}"));
    r.check(r.rows().iter().any(|x| x.0 == "Brass Hall" && x.1.starts_with("Auditioning")), || "Audition: the row does not say so".into());
    r.png("lib-09-audition");
    bar_do(r, "Stop");
    r.run(1500);
    let aud = r.engine.call("audition.get", json!({}));
    r.check(aud["id"] == "", || format!("Audition: stopped, the engine has {aud}"));
    r.sent("Audition");

    r.step("Library: Sounds, new part, audition in a part");
    let before = r.engine.graph()["parts"].as_array().unwrap().len();
    ui.global::<crate::LibraryState>().invoke_show_page(1);
    r.settle();
    r.png("lib-10-sounds");
    let sounds: Vec<String> = items(r, "sound").into_iter().map(|i| i.0).collect();
    let got = shown(r);
    r.check(got == sounds, || format!("Sounds: the list shows {got:?}, the engine {sounds:?}"));
    r.goto("Soft Felt");
    r.act(Action::Confirm(None)); // the Sound's menu
    choose(r, "New part");
    r.until(|_| said_is(&ui, "Sound loaded."));
    r.sent("New part");
    let after = r.engine.graph()["parts"].as_array().unwrap().len();
    r.check(after == before + 1, || format!("New part: the engine has {after} parts, was {before}"));
    r.engine.call("edit.undo", json!({}));
    r.engine.call("edit.undo", json!({}));
    ui.global::<crate::LibraryState>().invoke_show_page(2);
    r.settle();
    eprintln!("   setlists {:?}", r.rows());
    ui.global::<crate::LibraryState>().invoke_show_page(0);
    r.settle();

    // Add to setlist: the state file in the data folder says which one is open.
    r.step("Library: add to a setlist");
    let setlists = items(r, "setlist");
    let id = setlists.first().map(|s| s.1.clone()).unwrap_or_default();
    let dir = std::path::PathBuf::from(std::env::var("OSCAIRN_DATA_HOME").unwrap_or_default()).join("AppData/oscairn.setlist");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("state.json"), json!({"setlist": id}).to_string()).unwrap();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    r.settle();
    let count = |r: &mut Real| r.engine.call("setlists.get", json!({"id": id}))["setlist"]["pages"][0]["slots"].as_array().map_or(0, Vec::len);
    let was = count(r);
    r.goto("Glass Choir");
    bar_do(r, "Options");
    choose(r, "Add to setlist");
    r.sent("Add to setlist");
    let slots = count(r);
    r.check(slots == was + 1, || format!("Add to setlist: the setlist has {slots} slots, had {was}"));
    let line = said(r);
    r.check(line == "Added to Friday Set", || format!("Add to setlist: the line says {line:?}"));
    // With no setlist open and none called "Setlist": one is made.
    std::fs::remove_file(dir.join("state.json")).unwrap();
    shell::goto(shell::Screen::Home);
    shell::goto(shell::Screen::Library);
    r.settle();
    r.goto("Night Strings");
    bar_do(r, "Options");
    choose(r, "Add to setlist");
    let made = items(r, "setlist");
    r.check(made.iter().any(|s| s.0 == "Setlist"), || format!("Add to setlist: no setlist called Setlist was made: {made:?}"));
    let line = said(r);
    r.check(line == "Added to Setlist", || format!("Add to setlist (new): the line says {line:?}"));
    r.png("lib-11-end");
}

fn said_is(ui: &Shell, text: &str) -> bool {
    ui.global::<crate::PartState>().get_message() == text
}
