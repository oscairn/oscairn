// SPDX-License-Identifier: AGPL-3.0-or-later
//! Oscairn's UI in Rust (docs/rust-ui.md):
//!   oscairn-ui [--profile ID|FILE] [--socket PATH]
//! The profile: a built-in's id or a file (else OSCAIRN_PROFILE; else this computer's). The
//! engine's control socket: PATH, else /run/oscairn/engine.sock (a device's engine-only start).
//! It never ends because the engine is slow or gone: it says so and waits.

mod client;
mod profile;
mod settings;
mod shell;
mod theme;
mod topbar;
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
    let ui = Shell::new()?;
    let chosen = settings::chosen_theme();
    theme::apply(&ui, &theme::chosen([chosen.as_str(), profile.theme.as_str()]));
    shell::show_settings(&ui, settings::top_bar(profile.top_bar));
    let weak = ui.as_weak();
    let post = move |event| {
        let _ = weak.upgrade_in_event_loop(move |_| shell::dispatch(event));
    };
    shell::start(&ui, socket.into(), profile.memory, post).map_err(|e| slint::PlatformError::Other(e.to_string()))?;
    ui.run()
}
