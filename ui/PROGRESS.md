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
- CMake: option OSCAIRN_UI, ui/CMakeLists.txt (cargo with the pinned toolchain through
  SAMPLEPACK_CARGO, kms when OSCAIRN_ROLAND_FANTOM0, install bin/oscairn-ui, ctest `ui-rust`
  with OSCAIRN_ENGINE). Test against the real engine (build-ui's, `--audio null`, its own
  folders): passed once on the Mac. Docs: rust-ui.md (Setlist, adding a screen, building and
  testing, device run recipe), RFC 0030 section. Buildroot packaging not done (offline crates,
  cross linking): said in the docs.
- Checks (single runs, Mac unless said): full ctest of build-ui (app, engine, BUILD_TESTING=OFF,
  before OSCAIRN_UI was on): 226/226 passed, 2 skipped (desktop-phase3b-engines,
  desktop-window-order). With -DOSCAIRN_UI=ON: ctest `ui-rust` passed (30 Rust tests, the real
  engine included). Container (Ubuntu 24.04 arm64): kms binary built, NEEDED libxkbcommon.so.0,
  libudev.so.1, libinput.so.10, libfontconfig.so.1, glibc up to 2.35, sha256 c3fc7aae...;
  the desktop build under Xvfb showing Home with no engine, after 10 s: RSS 17.9 MB, PSS 15.2 MB
  (anon 2.3 MB), 4 threads. Not measured: the KMS build's memory (no display in the container).
  trail check: no conflicts, 7 worth a look (word matches with unrelated engine learnings).
- Branch `rust-ui-setup` from `main` (8a9fb79c): the two gaps keeping the shell from being the
  default on the keyboard, and the unit run's findings.
- Handover (unit finding 1): the shell writes the handover file as a whole line; the keeper reads
  one without a newline too (`handed_over`). fantom_start.sh in a Debian container: 94 ok, none
  failing (3 new); the Rust test checks the newline. Commit 0df9f11b.
- Stale `oscairn.settings` (finding 3): a real gap. The JUCE app's launches (`memory.launch`) were
  never let go when it was killed; now a connection's admitted launches go with it after the 3 s
  grace, unless another open connection launched them or the app has its own. control-local-stream
  extended, passed. Commit e3de668f.
- Session setup (finding 2): everything the JUCE app sends at start and on reconnecting is listed
  in docs/rust-ui.md "Session setup" with file:line. Budget, memory classes, always-available
  apps, repositories and capture moved to `setUpSession` (hosts/standalone/SessionSetup.cpp),
  applied by the engine at start with `--device-profile` and by the JUCE app to the engine it
  starts. The UI's resident memory, before part of `budget.set`'s `system`, is now `memory.shell`,
  sent by each UI; the Rust shell sends it on connecting. `session_setup_matches_the_juce_app`
  (ui-rust): real engine twice, Rust path vs the real JUCE app (`--connect --snapshot`, offscreen,
  plus the `record.retro` a snapshot leaves out); equal, and equal to the unit's figures. Passed on
  the Mac. Commit adafc7eb.
- Packaging: `BR2_PACKAGE_OSCAIRN_UI` and the `oscairn-ui-crates` package (531 crates from
  crates.io as Buildroot extra downloads against Cargo.lock's checksums: 87 MB downloaded, 699 MB
  unpacked, 1.2 GB cargo target dir). Built in Docker in my own volume `oscairn-os-rustui` (a copy
  of `oscairn-os-w15`, 9 min to copy): the whole bundle, `oscairn-dirclean` and the crates' dirclean
  then `all`, 7 min 37 s with ccache, oscairn-ui's cargo build 3 min 49 s of it. The binary: 9.7 MB,
  NEEDED libxkbcommon.so.0 (the bundle's own now), libudev.so.1, libinput.so.10,
  libfontconfig.so.1, glibc symbols up to 2.43 (the bundle's). Two fixes on the way: the linker
  as a rustflag (same host and target triple on arm64), the Theme dedupe dropped (desktop-theme
  builds Theme.cpp alone). Commits ddd73628, 0eb50431 (the vendored backend's one warning).
  Not run on a unit: the bundle `build-os/oscairn-fantom-bundle-0eb50431.tar.gz` (sha256 e07f8e10...).
- Checks: ui-rust 31 passed; `cargo clippy --all-targets` clean (desktop); the kms build in the
  bundle has no warning. Full suite (build, `-j3`, excluding audio devices, window order, MIDI
  monitor): 236/236 passed, 1 skipped (desktop-phase3b-engines), first run.
</content>
