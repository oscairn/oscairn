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

Branch `rust-play` from `main` (8a96bc0e): Play in the Rust shell, so a Performance is played without
leaving it (the owner: switching to a JUCE screen takes several seconds).
- Play (`src/play.rs`, `slint/play.slint`, docs/rust-ui.md "Play"): scenes, a strip per part and bus
  (name, sound, input, fader, meter, M, S), the master, Previous/Next with the setlist; panes 0 and 1
  with border cursors, a press selects a scene or mutes a part; touch on tiles, M, S and faders.
  Home's Performance tile and the Setlist's new Play button open it; Edit hands over to JUCE. Data:
  `graph.get` as before, `controls.changed` while shown, `parts.meters` every 100 ms while shown. No
  new engine request. Tests (headless, fake engine): panes, touch, live levels, meters, back, handover,
  themes; pictures in ui/target/screens.

Branch `rust-part` from `main` (fc9d6ba0): the Part screen in the Rust shell, so a part's level, pan,
mute, solo, devices, key range, transpose and pedals are set without leaving it.
- Part (`src/part.rs`, `slint/part.slint`, docs/rust-ui.md "Part"): opened from Play by a press on a
  part or a tap on its name (Play's press no longer mutes; M and S stay on the strip). A settings list
  with a border cursor, the dial changing a value after a press, a devices list as the JUCE menu, the
  three pedals' receive each (all, none, own), the sound, engine, sends and insert effects read only,
  and a labelled row that hands over to the JUCE app for the rest. Requests: `parts.level`,
  `parts.pan`, `parts.solo`, `parts.input`, `session.status`; none new. Tests (`src/tests/part.rs`):
  cursor per pane, each edit's request, pedal round trip, refusal, touch, back, handover, themes.
  Not run on a unit; no window was shown.

Branch `rust-performance` from `rust-part` (b72bb6ac): the whole Performance app in the Rust shell
(owner, 2026-11-15); the checklist is ui/PERFORMANCE-APP.md (94 items, 0 needing a new request;
the JUCE app on the keyboard is `--connect`ed, so every call is a request; the plugin scanner is a
helper process, not an engine call).
- Inventory written and committed. Part screen: the rest of the JUCE `partFields` (channel,
  velocities, notes at once, reserve, priority, plays to, MIDI out and channel, sends, name, remove);
  the "Oscairn app" row is gone. FOUND: the Part screen sent `parts.input` without `graph.commit`
  (the engine stages it; each JUCE setter commits): fixed for every staged edit. Text keyboard:
  `keyboard.rs`, a port of TextKeyboard.cpp with the cursor and knobs for use without touch;
  profile `textEntry` read. 46 Rust tests pass on the Mac (cargo test --offline, headless).
- NEXT (in order): the playing keyboard K2 (read KeyboardWindow.cpp and the profile's
  onScreenKeyboard first); the sound chooser; the chain (D1..D12: scanner helper for D1); parameter
  pages F; scenes A2..A4; knob row and Learn A9..A11; Save A15; undo/redo J1; Controls G, Mixer H,
  Routing I, Parts list B; then step 5 (remove Play's Edit entry in play.slint:328 and play.rs
  handover, update docs). Not done: all of those.

Checkpoint (c): Play's and the Part screen's live edits (level, solo, master, pan) now commit, as the
JUCE setters do (scenes.select commits itself). New: `browser.rs` (the shared list), `chain.rs`,
`params.rs`, `sounds.rs`, `scan.rs` (the plugin scanner helper, `--plugins DIR`), shell glue, rows
"Sound" and "Insert effects" on the Part screen. Tests: `sh ui/target/test.sh test` (OSCAIRN_DATA_HOME
and OSCAIRN_CONFIG_HOME inside the worktree) 58 passed, clippy --all-targets clean. Ticked in the
inventory: C15, D1-D5, D12, E2-E5, F1-F4, F8, F9. NEXT: K2 playing keyboard; D6-D11 splits and
lanes, E1 tags and favorites, E6 presets, C16 replace, F5-F7; scenes A2-A4; knob row and Learn
A9-A11; Save A15; undo/redo J1; B, G, H, I; then step 5.

Scan helper: a timeout now kills the helper's process group (libc kill on the group, process_group(0)), as the C++ scanner does.
Branch `rust-performance-2` (from rust-performance 3ba4c37e): K2 playing keyboard (src/piano.rs, slint/piano.slint, tests/piano.rs: 7 tests) and C14 (Part "Keyboard" row; profile `on_screen_keyboard`); parameter rows draw a real switch; tests fixed to render at the window's real row count (`fit_to_window`, `tall`) and at scale 1.55 (`at_fantom_scale`), PNGs in ui/target/screens; the Part footer overlap was a test artifact (rows-per-view forced to 30), not a shell bug, now covered. 66 tests passed, clippy --all-targets clean. Run: sh ui/target/test.sh test --offline.
NEXT, FIRST: the owner's design correction (end of ui/PERFORMANCE-APP.md): the grid framework with tiles, sliders, knobs, drawbars, dropdowns, split buttons and graph views, then parameter pages, the Play knob row and the mixer on it. Not started (context ran low); ParameterGrid.cpp, RFC 0011/0019/0042 and oscairn-clap ui.h still to be read. Then the order below.
Known small issue: the scene tile's number overlaps its name at scale 1.55 (play@1.55.png).

HOW TO RESUME (context ran low before c2): read ui/PERFORMANCE-APP.md (39 of 94 ticked) and this file. Patterns: a list screen = Browser (ui/src/browser.rs) + a module like chain.rs/sounds.rs with Request/Out/Outcome, shell glue in shell.rs (on_X, X_act, Screen enum, go(), list_tap/step/scroll, keyboard owner), tests in ui/src/tests/pages.rs and shellpages.rs (fake engine). Run tests: sh ui/target/test.sh test (and clippy --all-targets); ui/target/*.py are scratch (target is ignored); scripts must be written with the Write tool, bash refuses compound commands with heredocs and sed -i. Every staged edit is followed by graph.commit. Order still to do: K2 playing keyboard (read hosts/standalone/KeyboardWindow.cpp, profile onScreenKeyboard; notes.send), A2-A4 scenes add/store/rename/remove/scope/transition (scenes.*; PartsView.cpp sceneEditMenu line 1140), A9-A11 knob row/banks/Learn (PlayView.cpp, controllers.view, assignments.get/set), A12 no-MIDI hint, A13, A14, A15 Save (LibraryPane.cpp savePerformance: library.save, name on keyboard if no id), A16, J1 undo/redo (edit.undo/redo/history), B parts list, G controls, H mixer, I routing, D6-D11 splits and lanes, E1 tags/favorites, E6 presets, C16 replace (graph.replace), F5-F7, C14/C20, then step 5 (remove Play's Edit entry: ui/slint/play.slint:328 and play.rs handover, Home/Setlist already open Play; update docs/rust-ui.md).

Branch `rust-grid` (from rust-performance-2 56231a34): the grid framework, the owner's design correction.
- ui/GRID.md: the design read once (params.list carries every hint, no engine code needed; ui.h has no
  spans: the layout is ParameterGrid's rules; screen knobs play the published page engine side), the
  checklist, the state, and the universal UI debts.
- src/grid.rs + slint/grid.slint: tiles laid out from the plugin's data (columns 2 to 6 from the width
  the grid is given, runs kept together, views first), widgets: knob, slider across and upright,
  drawbar, switch, dropdown with its list, split buttons, two-sided range, value; envelope view.
  Cursor in two dimensions (dial reading order, pane 0 columns, pane 1 rows, arrows), press selects,
  dial changes, Cancel backs out a layer; touch drags and taps. Relayout when the area resizes
  (`GridState.resized`).
- src/params.rs rebuilt on the grid (the row version is gone); publishes `controllers.view` `screen`.
- Tests: `sh ui/target/test.sh test --offline` 76 passed (11 new: tests/grid.rs and grid.rs units),
  `cargo clippy --all-targets` clean. ui/target/test.sh (ignored, rewrite it on another machine) runs
  cargo with OSCAIRN_DATA_HOME and OSCAIRN_CONFIG_HOME in ui/target/home, wiped each run; the test
  harness now keeps those when given. Test windows can be any size (`Screen::sized`).
- Pictures (looked at): ui/target/screens/grid-*.png, copied to agent-notes/screens/: every kind at 1
  and 1.55, the dropdown, the Basic Synth's Main and Envelope pages at 1 and 1.55, desktop 700x700,
  1100x700 at 1 and 2, 1600x900, devices 800x480 and 1920x1080 at 2. The upright slider and drawbar
  are added to the picture by hand (no data selects them yet).

HOW TO RESUME (after rust-grid): read ui/GRID.md (state, checklist, debts) and the end of
ui/PERFORMANCE-APP.md (the owner's requirements received 2026-11-15). Next, in order:
1. The text keyboard as an overlay over about the lower half of the screen on hardware devices, as
   hosts/standalone/TextKeyboard.cpp positions it, the edited field kept in view (moved up if
   covered); keep cursor, knob and touch behavior and tests; render at 800x480, 1280x720@1.55 and
   1920x1080@2. Its own commit.
2. Play's knob row and banks on grid tiles (A9-A11: PlayView.cpp, `controllers.view`,
   `assignments.get/set`), then the mixer strips' faders as the grid's upright slider (H).
3. The other grid views (key map, FM operators, filter, LFO), dragging envelope points, search (F5).
4. The rest of the inventory as listed in the previous HOW TO RESUME, then step 5 (no handover).
Known: the scene tile number overlaps its name on Play at 1.55 (not fixed); the pages column stays
180 px on narrow windows.

Branch `rust-theme` (from rust-grid 3a97dd63): the owner's requirement (2026-11-15), "implement the theme system:
colors, button styles and corner rounding, spacing, all that stuff". Checklist and audit: ui/THEME.md.
- Baseline before: `sh ui/target/test.sh test` 76 passed, `cargo clippy --all-targets` clean (ran online: `--offline`
  found no crates in this worktree's cache).
- [x] Audit of the reference's properties and the Rust UI's hardcoded values (THEME.md section 1).
- [x] theme.rs loads the whole format under the reference's rules and words (strict: a partial file is refused whole,
  as `ThemeData::parse` does; defaults only for what it lets a file omit); `Theme` global carries colors, radius,
  type, the eight style choices, wires, splash; the fallback chain is `Theme::reload`'s.
- [x] Screens converted: Home, top bar, engine banner, Play, Part (and the chain and sounds lists on it), the text
  keyboard, the piano, Setlist, the grid (tiles, sliders, knobs, split buttons, dropdowns, tabs, pages, panels).
  Sections joined: the grid's layout has no gap (`theme::section_gap`).
- [x] Tests: 84 pass (76 before; new: loading every built-in file with its values, defaults, refusals, folder rules,
  the choice, row fill, apply sets every property, no literal color/radius/font size in a .slint file, the grid's
  gap per theme). Every screen under the four built-in themes and two odd ones: ui/target/theme-shots/ (ignored).
- [x] Task A, spacing (branch `rust-theme-spacing`, from `rust-theme` 112157a5; baseline 86 tests, clippy clean): one optional
  `spacing` (0.6 to 1.6, default 1) in the format, docs/theme.schema.json, docs/design-language.md, Theme.cpp and theme.rs
  with the same rules; scales the Rust UI's gaps, margins and paddings (ui/THEME.md "Spacing") and in JUCE `sectionGap` and
  Play's padding; the odd themes are roomy (1.6) and tight (0.6) and every screen was rendered and looked at in both
  (ui/target/theme-shots). Tests: 87 pass (a layout test that a larger spacing grows the gaps), clippy clean;
  desktop-theme passes. Home's tile grows with the padding and the Setlist is shown again when its view size changes.
- Not done: `wires` and `splash` are loaded and carried, drawn by nothing (no routing view or splash in the Rust UI);
  the desktop type scale (12/14/13/18); list rows as the reference's `drawListRow` bands; (the `fontScale` bound quirk, 0.8 and 1.4 refused, is fixed in both readers.)
- Envelope nodes off their line (owner, 2026-11-15): the line and fill Paths in slint/grid.slint used Slint's default fit, which
  scales the commands into the item's width less the stroke and offsets them by half of it, while the nodes are placed in plain
  pixels; both Paths are now `fit: preserve`. Pixel test (src/tests/envelope.rs): nodes' centroids against the fitted
  segments' meeting point, at 1280x720 and 1920x1080, scale 1 and 2: worst 7.68 px before, 0.50 px after. The reference's
  envelope (ParameterGrid.cpp `Envelope`) has a handle per timed stage end only, none at the start or the sustain's end; the same here.

Branch `rust-app-play` (from `rust-app` 85ce5fd5; baseline 88 tests, clippy clean): the Performance app's Play (A) and the top bar (J).
- Done (ticked in PERFORMANCE-APP.md): J4 the recording indicator (new: `record.takes` read with the status notifications every
  250 ms as the JUCE RecordButtons poll it; top bar and Play), J1 undo/redo and A15 save as top bar actions with a cursor
  (third pane, Tab, the dial up out of the list), A2-A4 the + Scene tile and the Scene screen (scene.rs), A6 the master by
  cursor and dial, A12 the no-MIDI hint, A13 the empty state. 101 tests, clippy clean.
- Left: A9, A10, A11, A16 (the knob row, banks and Learn: they need the grid's tiles and `row:N` knobs, which another branch
  owns: what Play needs from the grid is a way to lay a row of knob tiles out in a given area and to publish the row as
  `controllers.view`'s `screen` page), A14 and J3 (Edit needs the Parts list, B, which nothing builds yet: the footer button
  that hands over to the JUCE app stays until it does).

Branch `rust-app-grid` from `rust-app` (85ce5fd5): parameter pages, section F (ui/PERFORMANCE-APP.md F5 and F7 ticked).
- The reference selects no vertical slider or drawbar from plugin data (ParameterGrid.cpp `Control` has horizontal slider
  and knob only; `enum oscairn_widget` has neither; RFC 0042: no drawbar view), so that item has nothing to select by: they
  stay widgets for Play's slider row (ui/GRID.md).
- `src/views.rs` and the `ViewPanel` of slint/grid.slint draw every graph view from one set of points (paths unscaled,
  `fit: preserve`, boxes, labels, handles): envelope nodes dragged by touch (e05bff23), the LFO (02ffdb0c), the filter
  (967cd464), the key and velocity map (2fdad232), the FM operator view (ec760a14), the search (9ee154cb), and the DX7 and
  FM-X envelope names. Each view's lines are measured in the rendered pixels (tests/views.rs, tests/shots.rs) at two window
  sizes and two scales: worst offset of the drawn line from its path 0.16 to 0.20 px (limit 0.6); a path a pixel off is
  seen as that far off, so the measure can fail. Tile density (GRID.md): no change was needed.
- Tests: `sh ui/target/test.sh test --offline` 111 passed (baseline 88), `cargo clippy --offline --all-targets` clean.
  Pictures: ui/target/screens/grid-{lfo,filter,keymap,fm,search}*.png, ui/target/theme-shots/grid-*-*.png. Not run on a unit.

Branch `rust-app-knobs` (`rust-app` 79a277ee, then `rust-app-grid`'s seven commits cherry-picked on it; two conflicts, both
resolved keeping both sides: shell.rs's keyboard owner arms (Scene and Params) and this file): 124 tests after the join, clippy
clean.
- FM view (grid): the reference lays each operator column out as the bottom 26% for its envelope inside the panel; nothing was
  cut, and the operators without an envelope in the first fixture were the fixture's (two had parameters): the fixture now gives
  all four, and a test measures every shape, box and label inside the panel at 1280x720 and 1.55 under every theme. An eight-operator
  fixture (an FM-X-like routing: a stack four deep, an operator modulated by two, feedback into a different operator and into itself)
  is measured inside its panel and columns; a box's text shrinks to 70% before it is cut, as drawFittedText does. The reference at
  eight does what it does at four: the columns share the width, none wraps or scrolls.
- The recording mark is in the top bar only (the JUCE Launcher draws it once over every app; PlayView.cpp has none).
- Play's knob row (A9, A10, A11, A16; src/row.rs, src/knobs.rs, `TileRow` in slint/grid.slint, grid::row_layout): see
  docs/rust-ui.md "The knob row". Tests: 147 passed, `cargo clippy --offline --all-targets` clean. Not run on a unit, and not
  against the real engine: what the engine plays on the knobs is the port's resolution (knobs.rs) against the cases of the engine's
  own test, not a comparison with the engine.

Branch `rust-app-parts` (from `rust-app` 79a277ee; baseline 101 tests, clippy clean): the Parts list (B) and the Part page's rest (C).
- Two defects of the last round, own commit (40ef8f64): Play, Part, Chain, Sounds and Scene lay out again when the theme or the window
  changes (`PlayState.resized`, `PartState.resized`, the shell's `refit`; src/tests/layout.rs fails without it: Play kept 10 strips
  where the roomy theme holds 9); only one cursor shows (`Theme.bar-cursor`, set by the shell while the bar has it; a `Cursor`
  draws nothing then unless `always`, which the bar's own and the keyboards' do; counted in pixels of the odd theme).
- Parts list (B1 to B3, B5, B6, B9, B10) and Edit (A14, J3): src/parts.rs, slint/parts.slint, tests/parts.rs. Edit is the last action of
  Play's top bar, the footer's handover button is gone; the list's Play action goes back. B4 (a press on a shared row opens Routing)
  and B8 (Mixer, Controls and Routing buttons) are built but disabled until I, H and G exist: `Page::built()` in parts.rs.
- Part page (C): C16 the Engine is shown, "replace instrument" is not on the JUCE Part page (PluginBrowser's, the Plugins app), C17 n/a
  (the JUCE Part page lists no controls: G1), C20 built (Key and Scale rows, `harmony.get` and `harmony.set`) though the JUCE app has no
  control for the key.
- Seen, not fixed: the top bar at 800 px wide with a take recording and all five actions of Play (Undo, Redo, Save, Scene, Edit) is
  wider than the window (the resource section is cut); with four it already was.

Branch `rust-app-pages` (from `rust-app-parts` 1130dce5; baseline 115 tests, clippy clean): the Parts list's three pages, H, G and I of the inventory.
- Mixer (H1, H2): src/mixer.rs, slint/mixer.slint. The JUCE page is strips, so strips are built (name, fader and meter bars, dB, M, S, pan, a send per bus,
  the master pinned at the right); the grid's tiles are not used. Meter moved into theme.slint (Play's copy in play.slint stays: that file is the grid agent's).
- Controls (G1 to G4, G6) and the menu list the JUCE popup menus become (src/menu.rs, slint/menu.slint, shared with Routing): src/controls.rs,
  slint/controls.slint; Now... is a port of `setRangeToPresent`.
- Routing (I1 to I6, G5): src/wires.rs is a port of the layout (PartsModel.cpp) with the JUCE test's properties; src/routing.rs the screen; the wires are
  `fit: preserve` Paths from the layout's own points and a pixel test measures that they meet their ports (it fails without `fit: preserve`). Chain actions
  gained "Routing..." (D11) and "Scene controls..." (D10, `scenes.untouched`).
- Not verified: nothing here was run on a unit or against the real engine (fake engine only); Routing's layout was ported by reading, not compared with the C++
  output on the same graph.


Branch `rust-app-all` (`rust-app` 3cbfb25f, then `rust-app-pages`' seven commits cherry-picked; conflicts in play.slint, shell.slint, main.rs,
shell.rs, topbar.rs, tests.rs, tests/bar.rs, tests/part.rs and the three docs, each resolved keeping both sides: the bar's Play actions are
Undo, Redo, Save, Scene, Learn (where the device has knobs), Edit; Play's footer button and recording mark are gone; `lists` is false on Play
and the parameter pages; the Parts work's `Theme.bar-cursor` stays): 208 tests after the join, clippy clean.
- Play at the keyboard's scale (826x465 logical). The reference's `resized()` for a dense screen: scenes 44, the knob row a quarter of the
  height with its buttons in a column beside the tiles, no footer. Before: the knob row spent a button line plus its tiles, the footer a row,
  the faders traveled 38 px, scene names had 41 px ("Ver..."), empty knobs read "No con...". Now: scenes 98 wide with the name across the
  tile (`FitText`: 70% before a cut), the knob row 104 with its buttons beside it, empty knobs a dash, the line under the strips only when it
  says something, faders travel 131. tests/row.rs checks, in the built-in themes and the two odd ones at 1280x720 at 1 and 1.55 and at 800x480,
  that nothing overlaps, no scene name of 8 characters or fewer is cut, and the faders travel at least 72 (36 with one more line: the no-MIDI
  line, Learn's or the one under the strips; with two at once, 15 in the roomy theme at 1.55: not improved).
- Seen, not fixed (grid.slint is another branch's): a knob tile 92 wide puts the parameter's name without its plugin's (`Knob::narrow`, as the
  reference at that width), and the grid's tile cuts a long value ("40...") there.

- Routing at the keyboard's scale. The reference (PartsView.cpp, routing page) lays the graph out at its own size, then fits it to the space
  above a fixed 190 px panel with `scale = min(1, canvas / layout)` and no floor: it shrinks, it does not scroll or zoom. Here at 826x465 the
  test graph (1068x288 laid out) was drawn at a scale of 0.52, 150 logical px high (box names 7 px, details 5 px, measured from the picture). Now the drawing keeps the theme's type scale (at least 1), all
  box text is at least the theme's small size (the detail and chip text, which were 77% and 85% of it, are at it), and what does not fit scrolls
  (a drag; the cursor moving follows). The buttons share Back's line when they fit and the panel takes its rows, so the canvas is 260
  logical px high at that size, drawn at scale 1 (names 13 px, details 13 px), 1068 px wide in a canvas of 798. Tests: the pixel
  test now scrolls the drawing in steps and measures every wire end while in view; a new test checks the cursor's place is whole in the view at each
  dial step and a drag of 120 px moves it 40 or more, at 826x465 and 800x480.

- Top bar at narrow widths (the overflow noted above is fixed). The rule, in order (slint/topbar.slint, tests/bar.rs): Undo and Redo to icons, then all
  actions into one "More" (a menu; one stop for the cursor; `Bar.more` read by the shell) while the Performance's name keeps 16 small-type heights;
  then the name's room down to 9 (text 70% then cut: `FitText`); then the meters' names, then half their bars; last "disk slow" becomes "slow". The
  recording mark and the tempo always stay. Seen in the pictures (ui/target/theme-shots/bar-narrow-*): at 826 across (the keyboard's) and 800 the six
  Play actions are folded into More and the name is 200 to 312 wide (226 to 312 at 826); at 640 the name is 126 to 144 wide (least 117), and with a slow
  disk the meters have no names and short bars and the mark says "slow"; at 1280 across at scale 1 everything shows. Measured by the test in every built-in theme and the tight odd one at 826, 800, 640 and 1280 across, with and without a slow
  disk: the meters end inside the bar, the name has at least its least, the mark fits between the name and the tempo. NOT held in the roomy odd
  theme (type 1.39, spacing 1.6) below 1200 across: its fixed parts alone are wider (the meters end at 839 of 826 with a slow disk).

Branch `rust-app-fm` (from `rust-app` 3cbfb25f; baseline 147 tests, `cargo clippy --offline --all-targets` clean): the FM operator view
redesigned on the owner's request (2026-11-15; ui/GRID.md "The FM operator view", a recorded difference from the reference).
- src/fm.rs: the algorithm as a horizontal flow (carriers at the right meeting an output line, layers by distance from the output,
  rows by sweeps that minimize crossings, wires orthogonal through gutters and reserved points so none crosses a box, feedback as a
  loop), one graph of every operator's envelope scaled by its level on one time scale, a strip of operator cells, the operator chosen by
  tap or by the cursor's tile (the knobs play it first), dots from `Screen::set_positions` (no source yet: notes/ui-live-positions-proposal.md).
  src/palette.rs: eight operator colors per theme (ui/THEME.md section 3). `grid::envelope_layout_in` is the envelope's layout in any area,
  with a gain and a shared longest stage. The FM panel is 350 high (was 300).
- Tests (155 passed: the old FM tests are replaced by tests/fm.rs, fm_view.rs, fm_touch.rs, fm_dots.rs, fm_lines.rs, palette.rs): flows on
  the five routings (parallel, chain eight deep, two stacks sharing a modulator, a wire skipping a column, the eight-operator fixture)
  and the four-operator one; boxes apart and inside the panel, every wire endpoint on a box edge or the output line, no segment through a
  box, texts fitting at 70% (estimated at 0.6 em a character), at 1280x720 at 1 and 1.55 in six themes; every operator's line measured in the
  rendered pixels (worst 0.19 px of the 0.6 limit, at 1 and 2); a dot's centroid worst 0.16 px of 0.75 (measured at 1 and 2: the harness finds
  the panel's origin to a whole pixel, so the fractional 1.55 is not measured). Pictures: ui/target/screens/grid-fm-*.png (+ @1.55),
  ui/target/theme-shots/fm-*-THEME@1.55.png.
- Not done or not verified: not run on a unit; no operator on/off field exists in the data (level 0 is the only "off"); the dots have no source.

Branch `rust-app-tabs` (from `rust-app` f3470f25 with `rust-app-fm` joined: its two commits cherry-picked; the one conflict was this file, both
appended entries kept; after the join 221 tests passed and `cargo clippy --all-targets` was clean). The owner's tabs request (2026-11-15; ui/TABS.md):
- One tab family (slint/theme.slint `TabBar`, `TabPages`; replaces `TabStrip`): a strip or a column of tabs, two levels (pages, tabs), `folder` and
  `segmented` in both forms, overflow that scrolls with arrow boxes, names to 70% then cut. The selected tab is a fill (`Theme.tab-on`, derived in
  src/palette.rs `tabs`: at least 3 to 1 from an unselected tab's, text 4.5 to 1), bold; the cursor a ring with a gap. Users: the parameter pages
  (pages over tabs, or a column of pages beside them on a window 560 logical px or more tall; the Search button moved to the header row) and Play's
  Performance / part switch (a column of two). The inventory of every place that switches sibling views, with why not, is in ui/TABS.md.
- src/tabs.rs: pages and tabs as one model (`Model`), from the module paths (today's rules, compared against a copy of the old logic over seven
  fixtures) or from declared data (draft 6; PROVISIONAL fields `uiTabs`, `uiTab`, `valueIcons` of `params.list`, listed in PERFORMANCE-APP.md "Items
  without a request"); `grid::icon` takes a declared icon first. Cursor: the dial walks search, pages, tabs, tiles; a press opens; the tiles stop at their
  end (they used to turn the page); Page keys step at once.
- Tests: the model (tests/tabs_model.rs), cursor, press, column, contrast per theme in pixels, overflow scrolling and a long name's cap (tests/tabs.rs), Play's column
  (tests/row.rs). Pictures: ui/target/theme-shots/tabs-*, tabrows-* (enlarged), play-tabs-*.
- Not done or not verified: not run on a unit; no device profile has a page key, so on a unit stepping tabs is the dial (back, along, press); a view whose
  parameters a plugin spreads over tabs is drawn from the tab shown, not once on the first (ui.h); a long name's tab is capped at four and a half touch targets (measured in pixels, tests/tabs.rs), the shrink to 70% and the cut are FitText's and looked at in the pictures (tabs-long-*), not measured.

Branch `rust-app-finish` (from `rust-app` 84fe9e23): the inventory rows still open, D6 to D9, F6, E1, E6. Baseline: 234 of 235 passed (the
Routing test `routing_opens_from_the_parts_list_and_its_actions_reach_the_engine` failed once in the full run and passed alone and in the
runs after), clippy clean. Now 254 passed, `cargo clippy --offline --all-targets` clean (`sh ui/target/test.sh ... --offline`; target/test.sh
is written again as this file says: the data and config folders in ui/target/home, wiped each run).
- D6 (fae4b0f4): role words and color classes, off, failed marks and the message line on the chain; the shared rows' text and the panel's
  lines shrink to 70% before they are cut. D7 to D9 (e436ee3b): splits, lanes, lane settings and crossovers on the chain screen, no
  confirmation on any removal (the reference has none; Undo covers them: every edit is followed by `graph.commit`). F6 (ed86ba75): the screen
  page tested, `row:N` written down as not this shell's. E1 (22a5a73e): the sound chooser's Filters, tags, libraries, favorites, row details and
  color. E6 (2fe24a21): Presets... on a plugin's actions. Pictures in ui/target/screens: chain-marks-*, chain-split-*, chain-lane-*,
  chain-lane-settings-*, chain-presets-*, sounds-list-*, sounds-filters-* in the dark, odd and odd-joined themes at 1280x720, 1280x720@1.55, 800x480.
- State and what has never been run against the real engine or on a device: the top of ui/PERFORMANCE-APP.md.

Branch `rust-app-chrome` (from `rust-app` bc6e42d6; baseline 256 passed, 1 ignored, clippy clean): the top bar carries the chrome
(Back, the path title, the screens' own actions; Cancel is Back in one step; each screen has the header row's 48 logical px), a status
that says what the last said costs no request and no frame, and the theme's `tabPlacement` (auto, top, side) in both theme readers with
the side form of the tab family. Now 268 passed, 1 ignored, `cargo clippy --offline --all-targets` clean. The JUCE `desktop-theme`
test passed on the Mac (build-th). Not run on a unit; the ignored real-engine harness was not run and its Cancel sequences assume
the old layered Cancel.

Branch `rust-app-rails` (from `rust-app` 521cfe0b; baseline 268 passed, 1 ignored, clippy clean, confirmed): the `side` placement
redone as rails (ui/TABS.md "Rails"): each rail from under the top bar to the screen's bottom, its tabs sharing the height (a touch
target at least, then it scrolls), 71 logical px wide by the theme's type and spacing (at most 12% of the width) so the keyboard's
screen keeps three columns of tiles with both rails (668 x 405, was 593 x 405 and two), horizontal labels (the small type, or shrunk
alike to 70%): rotated text was not built because Slint 1.18.1's software renderer does not rotate it (rendered and checked); names
that share leading or trailing words show them once as a heading (src/tabs.rs `abbreviate`), in a rail always and in a strip when
the whole names overflow; `auto` is the placement with the larger content area; the pages-column arrangement and `Theme.column-from`
are gone. Now 274 passed, 1 ignored, `cargo clippy --offline --all-targets` clean. Pictures in ui/target/theme-shots: tabs-*,
tabrows-*, rail-*. Not run on a unit. docs/design-language.md (outside ui/) still describes `auto` as "by the screen's height".

Branch `rust-app-opdetail` (from `rust-app` 3de97271; baseline 274 passed, 1 ignored): the FM operators view as a master and detail on one page
(owner, 2026-11-16; ui/GRID.md "Master and detail", ui/TABS.md "Tabs that are indexes"). The overview (diagram, shared graph, a list line per
operator) over the selected operator's own tiles under a heading in its color; a page whose tabs are the indexes of one view folds into it
(no tab per operator); selection by the dial (the detail follows the cursor along the overview), by touch, and the page keys; rows paged;
the knobs under the screen follow the selection; fixtures' tab and page names made neutral. Tests: tests/fm_master.rs.

Branch `rust-perf-finish` (2026-11-16; local `rust-app` adcf9952 rebased onto main 40003f71, one conflict in docs/plugin-porting.md kept both sides): the Performance app finished but for what the State paragraph of PERFORMANCE-APP.md leaves. Baseline after the rebase: 281 passed, 1 ignored, clippy clean. Real engine again (ui/REAL-ENGINE.md): the eight engine problems fixed, idle 0 requests on Home and Play, four UI faults fixed. D9 key strip, D6 ends, E1 tag colors/hide/one line and the first Sound, Record and Capture, the text keyboard overlay, the bar and rail defects, and tests/handover.rs (the walk). Now 294 passed, 1 ignored, `cargo clippy --offline --all-targets` clean (`sh ui/target/test.sh test --offline`; recreate target/test.sh: it wipes target/home and runs cargo with OSCAIRN_DATA_HOME and OSCAIRN_CONFIG_HOME there). Not run on a device.

Branch `rust-fm-table` (from `rust-app` adcf9952; baseline 281 passed, 1 ignored, `cargo clippy --offline --all-targets` clean, confirmed): the FM
operators view's operator table and its dense detail (owner, 2026-11-16; ui/GRID.md "The operator table", "The detail").
- The overview is graph | table | diagram; the table has a row for each operator (number chip, M, S, level bar, coarse, fine, ratio or
  frequency, detune; columns only for fields the data has), every cell edited in place (dial in reading order, panes 0 and 1 along and down,
  ENTER edits or switches, EXIT ends; touch taps, drags, level bar); the knobs play the selected row. M and S are audition switches (the
  owner's decision): a transient mute field drives M, S is derived in the host (the others muted, restored on release), neither field: both
  disabled with the reason in the table's heading line. Provisional field names `operatorMute`, `operatorSolo` and `transient`: the
  extension has none yet (notes/ui-fm-fields-proposal.md, outside the repository).
- The detail: heading with an operator selector, the envelope as named stages (Hold, Attack, Decay 1, Decay 2, Release by the fields'
  place or names) in a Time row over a Level row under a graph that gives each stage a slot, times only where `seconds` is given (a rate's
  marked "/range", else "rate"), then compact controls (`Theme.compact-height` = `grid::compact_height`, the touch target at least, up to
  eight a row, paged). Labels in a view panel shrink to 70% before they are cut.
- Tests: tests/fm_table.rs (columns, cell cursor and edit, mute and solo for four kinds of data with no gesture or commit for a transient
  mute, knob row, stage names, times, controls under their points, the compact size held equal in every theme, the screens' numbers, touch)
  and the old FM tests moved onto the new layout (fm_master, fm_touch, fm_view). 290 passed, 1 ignored, clippy clean.
- Pictures: ui/target/theme-shots/table-ENVELOPE-OPERATORS-SELECTED-PLACEMENT-THEME@SCALE-WxH.png (engine-like data, named stages with
  and without seconds, numbered rates with and without; 8, 6 and 4 operators; dark, light, both odd themes, the joined block theme),
  table-crop-*.png enlarged. Not run on a unit. Not held: the roomy odd theme (type 1.39, spacing 1.6) at the keyboard's screen, where the
  block is taller than the area and the page scrolls; the detail's own graph is left out at 8 operators on the keyboard's 405.

Branch `rust-settings` (from `rust-library` bca03b01; baseline 371 passed, 2 ignored, `cargo clippy --offline --all-targets` clean): the Settings app, ported whole (ui/SETTINGS-APP.md: 54 rows, all built, and 8 that the JUCE app does not have). The shared list screen under the shared tab family, one page a tab: Appearance (themes chosen and installed, `appearance.txt` and `themes/`), Top bar (`top-bar.json`), MIDI controllers, Audio (only where the profile does not fix its device), Package repositories (read first, a root key trusted by its fingerprint), Signing keys and About. Home's tile still hands the display to the JUCE app (`OSCAIRN_UI_SETTINGS=1` opens the Rust one). Tests: tests/settings*.rs against `settings_fake.rs` (a fake engine that answers as the real handlers do), pictures in target/screens/settings-*.png; `real_settings` (ignored) ran against a real engine with `--audio null`, once with the keyboard's profile and once with a computer's. Findings of the real engine are in SETTINGS-APP.md "Verified shapes".
