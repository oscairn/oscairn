// SPDX-License-Identifier: AGPL-3.0-or-later
//! Oscairn's UI in Rust (docs/rust-ui.md):
//!   oscairn-ui [--profile ID|FILE] [--socket PATH] [--handover FILE] [--plugins DIR]
//! The profile: a built-in's id or a file (else OSCAIRN_PROFILE; else this computer's). The
//! engine's control socket: PATH, else /run/oscairn/engine.sock (a device's engine-only start).
//! With --handover, a screen not yet ported ends the program with status 3 and its name (the
//! JUCE app's --app) in FILE, for the start script to give the display to the JUCE app.
//! It never ends because the engine is slow or gone: it says so and waits.

mod browser;
mod chain;
mod client;
mod controls;
mod grid;
mod home;
mod keyboard;
mod knobs;
mod menu;
mod mixer;
mod nav;
mod params;
mod piano;
mod part;
mod row;
mod parts;
mod play;
mod profile;
mod scan;
mod scene;
mod sounds;
mod setlist;
mod settings;
mod shell;
mod theme;
mod topbar;
mod views;
#[cfg(test)]
mod tests;

use profile::Profile;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let profile = Profile::load(arg("--profile").or_else(|| std::env::var("OSCAIRN_PROFILE").ok()).as_deref()).unwrap_or_else(|e| {
        eprintln!("oscairn-ui: {e}");
        std::process::exit(2)
    });
    let socket = arg("--socket").unwrap_or_else(|| "/run/oscairn/engine.sock".into());
    // The frame cap, which the KMS backend (vendor/README.md) reads as it makes the window; one in
    // the environment wins, for measuring. Set before any thread starts.
    if std::env::var_os("SLINT_KMS_MAX_FPS").is_none() && profile.frame_rate() > 0 {
        std::env::set_var("SLINT_KMS_MAX_FPS", profile.frame_rate().to_string());
    }
    // The JUCE app's screen scale, which the KMS backend reads too (its own SLINT_SCALE_FACTOR wins).
    if std::env::var_os("SLINT_SCALE_FACTOR").is_none() && profile.scale > 1. {
        std::env::set_var("SLINT_SCALE_FACTOR", profile.scale.to_string());
    }
    let ui = Shell::new()?;
    let places = theme::Places::found();
    theme::attach(&ui);
    theme::apply(&ui, &theme::chosen(&places, &settings::chosen_theme(), &profile.theme));
    let watch = slint::Timer::default();
    theme::follow(&ui, places, profile.theme.clone(), &watch);
    shell::show_settings(&ui, settings::top_bar(profile.top_bar));
    let weak = ui.as_weak();
    let post = move |event| {
        let _ = weak.upgrade_in_event_loop(move |_| shell::dispatch(event));
    };
    shell::set_plugin_dir(arg("--plugins"));
    let handover = arg("--handover");
    shell::start(&ui, socket.into(), &profile, handover.is_some(), post).map_err(|e| slint::PlatformError::Other(e.to_string()))?;
    ui.run()?;
    if let (Some(file), Some(app)) = (handover, shell::handover()) {
        if let Err(e) = shell::write_handover(file.as_ref(), app) {
            eprintln!("oscairn-ui: cannot write the handover file {file}: {e}");
            std::process::exit(1);
        }
        std::process::exit(3);
    }
    Ok(())
}
