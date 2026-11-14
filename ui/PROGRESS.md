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
- Coordinator: `encoder-scroll` is merged to main with `navigate` on panel-map controls and the
  engine's navigation totals in `controllers`; take theirs, drop my `navigation` section and the
  panel-holder plan, rebase onto main.
- Step 3b, themes: `theme.rs` reads built-in and installed theme folders and `appearance.txt`
  as the JUCE app does, follows the choice once a second, applies a switch (one repaint), merges
  missing keys from the default, registers the theme's fonts at run time (fontique); top bar
  columns scale with the theme's type; bars `accent`/`warning`/`danger`, cursor `focus`. Format:
  nothing JUCE-specific found; optional `colors.warning` and `colors.focus` added (schema; JUCE
  accepts them). Tests: all built-in themes and an odd one (extreme colors, font scale 1.4,
  missing and bad keys) render top bar and Home, a switch repaints once, fixed width holds at
  1.4. 25 passed. The JUCE Theme.cpp change not yet compiled (build running).
- Settings' Top bar page committed (81200ba4 before the rebase). Rebased onto main (encoder-scroll
  in); clean.
- Navigation reconciled with main's: no `navigation` section of my own (never committed to the
  schema), no panel-holder reader; the shell subscribes to `controllers`, reads `navigation`
  totals (first read a baseline, fresh on each connection), hands the difference to the screen
  shown only, none while handing over; `controllers.view {lists}` on connect and around a
  handover. Keys and cursor semantics as ListCursor (no cursor until the first move, Cancel
  drops it then goes back, tap drops it, 2 px accent border). 25 passed.
- Step 4, Setlist: `setlist.rs` (the prototype's model and screen on the shell), `slint/setlist.slint`:
  two panes (setlists, slots), pane 0 / pane 1 / the pane last touched; a view of slots at a time,
  the cursor past it turning a view; Confirm or tap opens; while one opens the last chosen waits
  and opens next; a busy (-32009) load is tried again as the next; the next prepared; state in the
  JUCE Setlist app's AppData/oscairn.setlist/state.json. Test against a fake engine. 29 passed.
- MISTAKE found and fixed: before the fix, the Setlist test wrote the user's real
  `~/Library/Application Support/Oscairn/AppData/oscairn.setlist/state.json` (now
  `{"current":13,"setlist":"s1"}`, written 14:26 by a test run; what was there before is lost).
  The tests now set OSCAIRN_DATA_HOME and OSCAIRN_CONFIG_HOME to a temporary folder first;
  checked: a later run left the file's time unchanged. Nav totals now ordered moves, confirms,
  cancels within one read.
- JUCE build (build-ui, Release, app + engine, BUILD_TESTING=OFF): desktop-settings passed after
  counting six pages and sending the page's changes at once (commit 610fd975); desktop-theme passed.
- Steps 5 and 6, handover and start script: `--handover FILE` (status 3 and the app's name),
  "Opening X..." for one frame; a computer's shell says where the screen is instead.
  JUCE Main.cpp: OSCAIRN_HOME_QUITS=1 makes Home and Back quit (systemRequestedQuit). Start
  script: OSCAIRN_UI (profile screens[0].ui via engine-args.py), engine alone then keep_ui
  (Rust UI, handover to Xorg + `Oscairn --connect --app`, back), touch rule in
  /run/udev/rules.d from OSCAIRN_UI_ROTATION (screens[0].rotation; FANTOM-07 90). Stop script
  ends oscairn-ui and removes the rule. fantom_start.sh: new dry-run checks; in a Linux
  container 91 ok, none failing (main's script there: none failing either); on macOS 4 checks
  fail on main and branch alike (/proc). engine-args test passed.
</content>
