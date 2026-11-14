// SPDX-License-Identifier: AGPL-3.0-or-later
//! Oscairn's UI in Rust (docs/rust-ui.md):
//!   oscairn-ui [--profile ID|FILE]
//! The profile: a built-in's id or a file (else OSCAIRN_PROFILE; else this computer's).

mod profile;

use profile::Profile;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().collect();
    let given = args.iter().position(|a| a == "--profile").and_then(|i| args.get(i + 1)).cloned().or_else(|| std::env::var("OSCAIRN_PROFILE").ok());
    let profile = Profile::load(given.as_deref()).unwrap_or_else(|e| {
        eprintln!("oscairn-ui: {e}");
        std::process::exit(2)
    });
    // The frame cap, which the KMS backend (vendor/README.md) reads as it makes the window; one in
    // the environment wins, for measuring. Set before any thread starts.
    if std::env::var_os("SLINT_KMS_MAX_FPS").is_none() && profile.frame_rate() > 0 {
        std::env::set_var("SLINT_KMS_MAX_FPS", profile.frame_rate().to_string());
    }
    let ui = Shell::new()?;
    ui.run()
}
