// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app against the real engine (ui/SETTINGS-APP.md): the engine of ui/REAL-ENGINE.md started with
//! `--audio null`, folders of its own and a device profile, the shell on a profile of its own driving every page as the dial and a
//! tap do, each action checked in the engine's own state. The null device means the audio page can only list, validate and
//! choose a rate and a buffer size (the device is fixed), no MIDI input exists so the controllers page lists none, and no
//! repository server is reachable, so a repository can only fail to read here: said in the report, not hidden.

use super::*;
use crate::{KeyboardState, MenuState, NoticeState, SettingsState};

/// The shell on `profile` (a built-in's id; none: a computer's), with folders of its own for the choices it keeps.
pub fn start_on(profile: Option<&str>) -> Option<(Real, std::path::PathBuf)> {
    let socket = std::path::PathBuf::from(std::env::var_os("OSCAIRN_REAL_SOCKET")?);
    let trace: Trace = Arc::default();
    let path = std::env::temp_dir().join(format!("oscairn-ui-real-settings-{}.sock", std::process::id()));
    proxy(&path, socket.clone(), trace.clone());
    fit_to_window();
    let engine = Direct::new(&socket);
    let screen = Screen::new();
    let (tx, events) = mpsc::channel();
    shell::start(&screen.ui, path, &crate::profile::Profile::load(profile).unwrap(), false, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let config = std::env::temp_dir().join(format!("oscairn-ui-real-settings-config-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    std::fs::create_dir_all(&config).unwrap();
    shell::settings_folders(Some(config.clone()), crate::theme::Places { built_in: None, installed: Some(config.join("themes")) });
    Some((Real { screen, events, trace, engine, findings: vec![], mark: 0, frames: 0, figures: 0, shown_figures: (-1, -1) }, config))
}

/// A finding when `ok` is not so, the message made from what the shell shows then.
fn ensure(r: &mut Real, ok: bool, message: impl FnOnce(&Real) -> String) {
    if !ok {
        let m = message(r);
        r.note(m);
    }
}

fn tabs(r: &Real) -> Vec<String> {
    r.screen.ui.global::<SettingsState>().get_pages().iter().map(|p| p.to_string()).collect()
}

fn page(r: &mut Real, title: &str) -> bool {
    let Some(i) = tabs(r).iter().position(|t| t == title) else {
        r.note(format!("no tab {title:?} in {:?}", tabs(r)));
        return false;
    };
    r.screen.ui.global::<SettingsState>().invoke_show_page(i as i32);
    r.settle();
    true
}

fn value(r: &Real, label: &str) -> String {
    r.rows().into_iter().find(|x| x.0 == label).map(|x| x.1).unwrap_or_else(|| format!("<no row {label}>"))
}

/// A tap on the row labeled `label`.
fn tap_label(r: &mut Real, label: &str) {
    match r.rows().iter().position(|x| x.0 == label) {
        Some(at) => tap(r, at as i32),
        None => r.note(format!("no row {label:?} in {:?}", r.rows())),
    }
}

fn tap(r: &mut Real, row: i32) {
    r.screen.ui.global::<crate::PartState>().invoke_tapped(row);
    r.settle();
}

/// A choice of the menu open now, by its label.
fn choose(r: &mut Real, label: &str) -> bool {
    r.screen.frame();
    let ui = r.screen.ui.clone_strong();
    ui.global::<MenuState>().set_rows_per_view(30);
    r.run(100);
    let menu: Vec<String> = ui.global::<MenuState>().get_rows().iter().map(|x| x.label.to_string()).collect();
    let Some(at) = menu.iter().position(|l| l == label) else {
        r.note(format!("the menu has no {label:?}: {menu:?}"));
        return false;
    };
    ui.global::<MenuState>().invoke_tapped(at as i32);
    r.settle();
    true
}

fn typed(r: &mut Real, text: &str) {
    for ch in text.chars() {
        shell::key(&ch.to_string());
    }
    shell::key(&slint::SharedString::from(slint::platform::Key::Return));
    r.settle();
}

fn danger(r: &Real) -> Vec<String> {
    r.screen.ui.global::<crate::PartState>().get_info().iter().filter(|l| l.failed).map(|l| l.text.to_string()).collect()
}

/// A bar action by label.
fn bar(r: &mut Real, label: &str) -> bool {
    r.screen.frame();
    let ui = r.screen.ui.clone_strong();
    let labels: Vec<(String, bool)> = ui.global::<Bar>().get_actions().iter().map(|a| (a.label.to_string(), a.enabled)).collect();
    let Some(at) = labels.iter().position(|l| l.0.starts_with(label) && l.1) else {
        r.note(format!("the bar has no enabled {label:?}: {labels:?}"));
        return false;
    };
    ui.global::<Bar>().invoke_action(at as i32);
    r.settle();
    true
}

/// Every page, as the dial and a tap do, against the engine; `profile` is the shell's, the engine runs its own.
pub fn settings(r: &mut Real, config: &std::path::Path, profile: Option<&str>) {
    let ui = r.screen.ui.clone_strong();
    shell::settings_in_rust(true);
    shell::set_text_on_screen(false);
    r.step("open the app");
    r.until(|ui| ui.global::<Bar>().get_cpu_known());
    shell::goto(shell::Screen::Settings);
    r.settle();
    let shown = tabs(r);
    eprintln!("   tabs {shown:?}");
    let wants_audio = profile != Some("fantom-07");
    ensure(r, shown.contains(&"Audio".to_string()) == wants_audio, |_| format!("the Audio tab on {profile:?}: {shown:?}"));
    r.sent("open");
    r.png("settings-appearance");
    super::idle(r, "the Settings' Appearance page", 20);

    r.step("Appearance: choose a theme");
    ensure(r, !r.rows().is_empty(), |_| "no themes listed".into());
    tap(r, 3);
    let chosen = std::fs::read_to_string(config.join("appearance.txt")).unwrap_or_default();
    ensure(r, !chosen.is_empty() && ui.global::<crate::Theme>().get_id().as_str() == chosen, |_| format!("appearance.txt {chosen:?}, shown {:?}", ui.global::<crate::Theme>().get_id()));
    r.sent("appearance");

    r.step("Top bar");
    page(r, "Top bar");
    tap(r, 0);
    choose(r, "Percent");
    ensure(r, std::fs::read_to_string(config.join("top-bar.json")).unwrap_or_default() == "{\"cpu\":\"percent\"}\n", |_| "top-bar.json".into());
    ensure(r, !ui.global::<Bar>().get_cpu_bar() && ui.global::<Bar>().get_cpu_percent(), |_| "the bar did not change".into());
    r.sent("top bar");

    r.step("MIDI controllers (none connected)");
    page(r, "MIDI controllers");
    let ports = r.engine.call("controllers.inputs", json!({}));
    eprintln!("   the engine's inputs: {ports}");
    ensure(r, ports["inputs"].as_array().is_some_and(|a| a.is_empty()) == (value(r, "MIDI inputs") == "No MIDI input is connected"), |r| format!("the page says {:?} and the engine {ports}", r.rows()));
    let refused = r.engine.raw("controllers.choose", json!({"input": "Pad One", "profiles": ["nonesuch"]}));
    eprintln!("   choose an unknown profile: {}", refused["error"]);
    ensure(r, refused["error"]["code"] == -32602 && refused["error"]["message"] == "No controller profile is named nonesuch", |_| format!("{refused}"));
    r.sent("controllers");

    if wants_audio {
        r.step("Audio");
        // Whatever an earlier run left: this one starts from 128 samples.
        let start = r.engine.call("audio.set", json!({"bufferSize": 128}));
        eprintln!("   start: {} Hz, {} samples", start["sampleRate"], start["bufferSize"]);
        r.settle();
        page(r, "Audio");
        let listed = r.engine.call("audio.devices", json!({}));
        eprintln!("   the engine's devices: {listed}");
        ensure(r, value(r, "Output").starts_with(listed["output"].as_str().unwrap_or("?")), |r| format!("Output {:?} against {listed}", value(r, "Output")));
        let output_faint = r.screen.ui.global::<crate::PartState>().get_rows().iter().next().map(|x| x.off);
        ensure(r, output_faint == Some(listed["fixed"].as_bool().unwrap_or(false)), |_| format!("the output row is faint {output_faint:?}, the engine says fixed {}", listed["fixed"]));
        r.step("Audio: buffer size, then sample rate");
        tap_label(r, "Buffer size");
        let rate = listed["sampleRate"].as_f64().unwrap_or(48000.);
        choose(r, &format!("256 samples ({:.1} ms)", 256_000. / rate));
        let now = r.engine.call("audio.devices", json!({}));
        ensure(r, now["bufferSize"] == 256, |_| format!("the engine's buffer size is {}", now["bufferSize"]));
        eprintln!("   Buffer size row after the answer: {:?}", value(r, "Buffer size"));
        ensure(r, value(r, "Buffer size").starts_with("256 samples"), |r| format!("Buffer size {:?}", value(r, "Buffer size")));
        tap_label(r, "Sample rate");
        choose(r, "96000 Hz");
        let at_once = value(r, "Sample rate");
        r.run(1500);
        let now = r.engine.call("audio.devices", json!({}));
        eprintln!("   asked for 96000 Hz: the row said {at_once:?} when the answer came, {:?} after the status; the engine says {}", value(r, "Sample rate"), now["sampleRate"]);
        // What the engine did with the rate is its own (the null device was seen to apply another one for 44100 and 48000,
        // SETTINGS-APP.md); the page must say what the engine says.
        ensure(r, value(r, "Sample rate").starts_with(&format!("{} Hz", now["sampleRate"])), |r| format!("the rate shown {:?}, the engine says {}", value(r, "Sample rate"), now["sampleRate"]));
        ensure(r, now["sampleRate"] == 96000, |_| format!("engine: asked for 96000 Hz, the device is at {} Hz", now["sampleRate"]));
        // A refusal, as the engine words it.
        let bad = r.engine.raw("audio.set", json!({"bufferSize": 999}));
        eprintln!("   an unoffered buffer size: {}", bad["error"]);
        ensure(r, bad["error"]["code"] == -32602, |_| format!("{bad}"));
        // Put the buffer size back.
        let back = r.engine.call("audio.set", json!({"bufferSize": 128}));
        ensure(r, back["bufferSize"] == 128, |_| format!("restored {back}"));
        r.sent("audio");
        r.png("settings-audio");
    }

    r.step("Package repositories");
    page(r, "Package repositories");
    let repos = r.engine.call("packages.repositories", json!({}));
    let official = repos["repositories"][0].clone();
    eprintln!("   the engine's repositories: {repos}");
    ensure(r, value(r, "Add a repository") == "address ›", |r| format!("{:?}", r.rows()));
    let name = official["name"].as_str().unwrap_or("?").to_string();
    ensure(r, value(r, &name) == "On", |r| format!("{name}: {:?}", value(r, &name)));
    tap(r, 0);
    let off = r.engine.call("packages.repositories", json!({}));
    ensure(r, off["repositories"][0]["enabled"] == false && value(r, &name) == "Off", |r| format!("turned off: {off}; row {:?}", value(r, &name)));
    tap(r, 0);
    let on = r.engine.call("packages.repositories", json!({}));
    ensure(r, on["repositories"][0]["enabled"] == true, |_| format!("turned on again: {on}"));
    // Remove is never offered for the official one.
    r.act(Action::Move { pane: None, by: 1 });
    let removable = ui.global::<Bar>().get_actions().iter().any(|a| a.label.starts_with("Remove") && a.enabled);
    ensure(r, !removable, |_| "Remove is offered for the official repository".into());
    // Adding one that cannot be reached: read first, said, nothing kept.
    if bar(r, "Add") {
        ensure(r, ui.global::<KeyboardState>().get_open(), |_| "no keyboard for the address".into());
        typed(r, "https://127.0.0.1:1/none");
        let ended = r.until(|ui| ui.global::<crate::PartState>().get_message().is_empty() || !ui.global::<crate::PartState>().get_message().starts_with("Reading"));
        ensure(r, ended, |_| "the listing job never ended".into());
        r.run(500);
        let said = danger(r).join(" ");
        eprintln!("   Add of an unreachable address says: {said}");
        ensure(r, said.starts_with("Not read: not a usable repository: cannot fetch repository.json"), |_| format!("said {said:?}"));
        let after = r.engine.call("packages.repositories", json!({}));
        ensure(r, after["repositories"].as_array().map(Vec::len) == repos["repositories"].as_array().map(Vec::len), |_| format!("a repository was kept: {after}"));
    }
    r.sent("repositories");

    r.step("Signing keys");
    page(r, "Signing keys");
    ensure(r, value(r, "Signing keys") == "No keys yet" || r.rows().len() >= 2, |r| format!("{:?}", r.rows()));
    let key = format!("Real-Check-{}", std::process::id());
    let made_before = r.engine.call("keys.list", json!({}))["keys"].as_array().map(Vec::len).unwrap_or(0);
    let last = r.rows().len() as i32 - 1;
    tap(r, last);
    ensure(r, ui.global::<KeyboardState>().get_open(), |_| "no keyboard for the name".into());
    typed(r, &key);
    let listed = r.engine.call("keys.list", json!({}));
    ensure(r, listed["keys"].as_array().map(Vec::len) == Some(made_before + 1), |_| format!("{listed}"));
    ensure(r, r.rows().iter().any(|x| x.0 == key && x.1.len() == 19), |r| format!("{:?}", r.rows()));
    let again = r.rows().len() as i32 - 1;
    tap(r, again);
    typed(r, &key);
    let n = ui.global::<NoticeState>();
    eprintln!("   a key made twice: {} / {}", n.get_title(), n.get_message());
    ensure(r, n.get_open() && n.get_title() == "No key made" && n.get_message() == format!("there is a key named {key} already"), |_| format!("{} / {}", n.get_title(), n.get_message()));
    shell::act(Action::Cancel(None));
    let again = r.rows().len() as i32 - 1;
    tap(r, again);
    typed(r, "bad name");
    eprintln!("   a bad name: {}", n.get_message());
    ensure(r, n.get_open() && n.get_message().contains("letters, digits"), |_| format!("{}", n.get_message()));
    shell::act(Action::Cancel(None));
    r.sent("keys");
    // The two refusals above were provoked and their words checked; they are not findings.
    r.findings.retain(|f| !(f.starts_with("keys: keys.make") && f.contains("-32602")));

    r.step("About");
    page(r, "About");
    let preinstalled = r.engine.call("packages.preinstalled", json!({}));
    ensure(r, value(r, "Engine") == preinstalled["release"].as_str().unwrap_or("?"), |r| format!("Engine {:?} against {preinstalled}", value(r, "Engine")));
    ensure(r, value(r, "Source") == "Development build" && value(r, "License") == "AGPL-3.0-or-later", |r| format!("{:?}", r.rows()));
    r.png("settings-about");
    r.sent("about");
}
