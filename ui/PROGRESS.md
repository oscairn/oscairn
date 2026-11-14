# Rust UI: progress

Appended after every step and commit (branch `rust-ui`). Times are the Mac's (MST).

- 2026-11-14: branch `rust-ui` from `main` (053d3613). Design note written: docs/rust-ui.md,
  "Decisions" (display handover chosen: (a); `ui/` single crate; one process, one client,
  abstract navigation actions). `encoder-scroll` does not exist yet: nothing to reconcile with.
- Step 1: `ui/vendor/i-slint-backend-linuxkms-1.18.1` vendored from crates.io (checked identical to
  the published crate but for the patch), `frame-cap.patch` (frame cap, frame statistics; the
  shadow-buffer experiment left out), README with origin, hash, commit, license. `ui/` crate
  skeleton: profile reading (built-in profiles from hosts/standalone/profiles), the frame cap
  from `screens[].frameRate` (default 30 on a device, none on a computer) put in
  SLINT_KMS_MAX_FPS before the window is made. Schema, RFC 0024 and the JUCE profile reader
  (accepts the field) updated. Mac: `cargo test` 2 passed; container (Ubuntu 24.04 arm64,
  Rust 1.95): `cargo build --release --features kms,roland-fantom0` built (8.1 MB). Not run on a
  display.
- Step 2 (Rust side): the shell (`shell.rs`), the client (prototype's, generic over what a
  request is for, notifications, error codes), the theme (built-in theme.json, chosen as the
  JUCE app does), the top bar (`topbar.rs`, `slint/topbar.slint`): Home, where, Performance and
  scene, tempo, CPU over RAM with fixed-width columns and digit cells; profile `screens[].topBar`
  and the user's `top-bar.json` over it. Headless scene tests (`src/tests.rs`, Slint's software
  renderer into a buffer, MinimalSoftwareWindow): fixed width over all 16 settings with 1%/100%,
  tempo 5/999.9, a long name; a meter update repaints only inside the section, no pixel outside
  changes, nothing drawn at rest; the shell against a fake engine on a socket. Mac: 16 passed,
  clippy clean. Pictures in ui/target/screens looked at by eye. The JUCE Settings page for it is
  written and waits on the JUCE build (build-ui, started 14:2x) to compile and run its test.
- Interim report sent. Owner (via coordinator): cpuLoad confirmed as the CPU figure; themes must
  be built in properly (installed themes, live switch, neutral format, tests): a step of its own
  after Home, before Setlist.
- Step 3: navigation (`nav.rs`: actions, keyboard map, panel-map roles `navigation`, a paging
  cursor), Home (`home.rs`, `slint/home.slint`): the profile's apps in the JUCE order with icon,
  name and state line (Performance name, library totals, audio device), border cursor (theme
  `focus`, the accent by default), Confirm/tap opens: Setlist a Rust screen, the rest a JUCE
  handover (asked for, not yet carried out: step 5). Tests: keys, panel roles and taps; a cursor
  move repaints within the two tiles; nothing at rest. 22 passed on the Mac.
</content>
