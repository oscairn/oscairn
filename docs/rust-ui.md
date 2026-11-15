# The Rust UI

The owner decided on November 14, 2026 that Oscairn's UI moves to Rust ("Rust direction: yes on
ui and readers") and that the Rust UI replaces the JUCE app screen by screen, as each is ready
("yes want rust to replace juce as each one is ready"). The first screens are the stage ones:
the shell with its top bar, Home, then Setlist; Play comes next. Its toolkit is Slint 1.18.1 with
its software renderer, chosen by the Setlist prototype (branch `experiment/rust-setlist`, its
README has the measurements and the toolkit comparison).

This page records the decisions first, then how it is built.

## Decisions

### 1. Who owns the display while some screens are still JUCE

Without an X server only one program can own the display (one DRM master). On a FANTOM-07 the
prototype drawing straight to the display missed 0 audio blocks in a 10 s scroll; the same
scroll under X missed about 650. Three ways were weighed:

- **(a) The Rust shell owns the display; a screen not yet ported gets it handed over to X and
  the JUCE app, and gives it back.** Chosen.
- (b) Both under X until everything is ported: no handover, but every screen keeps the X cost,
  the stage screens too (the Rust Setlist under X still missed about 650 blocks scrolling). Ruled
  out: audio on stage comes first.
- (c) The Rust shell staying alive and only dropping DRM master while X runs, so its state
  survives and the way back is quicker. Slint's KMS backend has no way to let go of the display
  and take it again; adding one is more vendored code for a return that a restart already does
  in well under a second. Not now. (JUCE without X is not an option: JUCE draws only through X on
  Linux.)

How (a) works:

- **The engine runs on its own** (the start script's engine-only start, `oscairn-engine-run`
  with the profile's options), so neither UI program starting or ending touches the audio.
  Today the JUCE app starts and owns its engine; on the Rust path it only connects to it.
- **The Rust shell** (`oscairn-ui`) draws on the display with Slint's `linuxkms` backend and is
  a trusted client of `/run/oscairn/engine.sock`.
- **A screen not yet ported** (Performance, Library, Groovebox, Recordings, Settings today):
  confirming its tile shows "Opening Settings..." for one frame, writes the app's name to the
  handover file and ends the shell (exit status 3). The start script's UI keeper then starts the
  bundle's Xorg and `Oscairn --connect /run/oscairn/engine.sock --app settings` with
  `OSCAIRN_HOME_QUITS=1`, so that app's Home and Back end it. When it ends, for any reason, the
  keeper stops Xorg and starts the Rust shell again, which opens on Home.
- **Time, estimated, not measured on a unit:** in, about 2 to 3 s (the shell ending, about
  0.1 s; Xorg serving a client, about 1 s by the start script's own polling; the JUCE app's
  first paint, 0.2 to 0.3 s in the prototype's container on a fast CPU, taken as 1 to 2 s on the
  unit); back, under a second (Xorg ending, the shell starting, about 0.15 s in the container).
  The device run measures both.
- **What the musician sees:** Home looks the same with the screen either app draws; a tile of a
  JUCE screen opens after a dark screen of 2 to 3 seconds, and its Home button comes back to the
  Rust Home after a dark moment. Audio plays on throughout. That wait is acceptable for Settings,
  the Library and the editors and never needed on stage, where Home, Setlist and (next) Play are
  Rust and draw without X. While a JUCE screen is open it has the X cost as today.
- **One profile setting switches it:** a screen's `"ui": "rust"`; absent or `"juce"`, the start
  script starts the JUCE app as today. JUCE stays the default until the owner switches it.

### 2. Where it lives and how it builds

- `ui/`: one Cargo package, `oscairn-ui`, with its `Cargo.lock` and a `rust-toolchain.toml` at
  the release `OSCAIRN_RUST_VERSION` in `CMakeLists.txt` pins (the samplepack crate's). One crate,
  not a workspace: one program. Screens are modules of it.
- `ui/vendor/i-slint-backend-linuxkms-1.18.1`: Slint's KMS backend, vendored with a frame cap
  and frame statistics, through `[patch.crates-io]`; its README says where it came from and what
  changed.
- Backends: Slint's ordinary windowed backend (winit) on macOS and Linux desktops, so the same
  screens run on a computer; `linuxkms` (no seat daemon: `backend-linuxkms-noseat`) for a device,
  the cargo feature `kms`. (The panel needs no code of the device's in the UI: the engine reads
  it and counts what it does to lists, below.)
- CMake: `-DOSCAIRN_UI=ON` builds it with cargo and installs `bin/oscairn-ui`; off by default,
  as the JUCE options are, because cargo downloads its crates on the first build. The device
  build (`OSCAIRN_ROLAND_FANTOM0`) takes the `kms` feature. ctest's `ui-rust` runs its tests, with
  the engine this build made when there is one.
- The device bundle: `/usr/bin/oscairn-ui`, beside the JUCE app, built by the bundle's Buildroot
  `oscairn` package with `BR2_PACKAGE_OSCAIRN_UI` (on in `oscairn_fantom_bundle_defconfig`). It
  links the bundle's own `libxkbcommon` (Buildroot's package, MIT/X11, new to the bundle, which
  the option selects), `libinput` (MIT), `libudev` (eudev, LGPL-2.1+) and `fontconfig` (the
  fontconfig license), already in the bundle for X, and nothing of a GPU.
  Its crates are the `oscairn-ui-crates` package: every crate `ui/Cargo.lock` names (531, about
  90 MB to download, 700 MB unpacked), each a Buildroot extra download from crates.io checked
  against the lock's own checksum in the package's `.hash`, unpacked as a cargo directory source;
  cargo then runs `--offline` (`OSCAIRN_UI_VENDOR`). Buildroot's own cargo vendoring needs a
  downloaded source archive, which `ui/` in this tree is not. The list and hashes come from
  `os/scripts/ui-crates.py`, run again whenever `Cargo.lock` changes; ctest's `ui-crates` fails
  until it is. Cross-built for the target with the toolchain's compiler as the linker and
  Buildroot's pkg-config. The crates' licenses are in that package's licenses (MIT or Apache-2.0
  mostly; Slint GPL-3.0-only as chosen above).

### 3. The shape of the code

- **One process, the shell, hosting screens as modules** (`home.rs`, `setlist.rs`), each a plain
  model (tested without a window) and the code that shows it.
- **One control-socket client** with a reader thread of its own (the prototype's: a slow answer
  never closes the connection, a lost one is made again each second, the screen says busy or
  lost). Subscriptions (`status`, `graph.changed`) feed the top bar; what the screens ask for
  comes back as events on the UI thread.
- **Nothing is drawn at rest, and only what changed when something does.** Slint's software
  renderer repaints the bounding box of what changed; the models set a property only when its
  shown value changes (a percent, a bar's pixel width), and a list sets only the rows that
  changed. Lists move by pages, never smooth scrolling.
- **A navigation layer of abstract actions**, the JUCE lists' (`ListCursor`, `ListAction`):
  move a pane's cursor (pane N from the left, or the pane last touched) by some steps, confirm,
  cancel; and from keys also a page and the first or last item. Three sources produce them: touch
  (a tap does what confirming does, and drops the cursor), a computer keyboard (arrows, Page Up
  and Down, Home and End, Return, Escape), and a device's panel: its panel map's `navigate`
  entries say which control does what in a list, the engine counts what they did, and the shell
  reads the totals from the `controllers` push (`navigation`) and acts on the difference. The
  abilities are the same on every device; only the map is the device's.
- **Only the screen shown takes the panel's navigation.** The shell is one process: it reads
  every `controllers` push so its totals stay current, and hands the difference to the screen
  shown alone; while the display goes to the JUCE app it takes nothing, and a new connection
  (the shell started again after a JUCE screen) begins with a fresh baseline, so presses made in
  the JUCE app are never played again in the shell. It tells the engine that lists are on screen
  (`controllers.view` `lists`) on connecting and when it hands the display over.
- **The cursor** is apart from any selection: none until the first move, which shows it on the
  first item shown; a 2 px `focus` border, as the JUCE lists draw it; moving opens nothing; Cancel drops it, and with none
  goes back; it drops on a tap and when its screen goes; past the visible items the list turns a
  page.
- **Themes are the JUCE app's** (below): the same folders and the same choice.

## Session setup

What the JUCE app sends the engine when it starts and when it reconnects, what each depends on,
and where it is applied now. The rule: what must hold whichever UI runs (engine policy, from the
device profile and the user's settings) is applied by the engine itself when it starts with the
profile, through one shared function; only what is a UI's own is sent by that UI.

| What (request) | From | Where it lives now |
| --- | --- | --- |
| The budget: `budget.set` with `cpu` (the reliability level's share, 0.75 at `instrument`), `memory` (80% of `compute.memory`), `available` (`compute.availableMemory`) and the memory classes (`device`, `system` = `osMemory`, `floor` = `performanceMemory`, `margin` 5%), off when they do not fit | the profile's `reliability` and `compute` | `setUpSession` (hosts/standalone/SessionSetup.cpp:14), applied by the engine at start with `--device-profile` (hosts/audio/AudioEngine.cpp:201) and by the JUCE app to an engine it starts (hosts/standalone/Main.cpp:1058); before, Main.cpp's `started()` alone |
| The always-available apps: `memory.alwaysAvailable` | the profile's `alwaysAvailable` (default `oscairn.performance`, `oscairn.library`), each at its manifest's `memory.reserve` (the Apps folder, hosts/standalone/AppManifest.cpp) or Oscairn's placeholder (`Profile::appMemory`) | the same |
| The device's package repositories: `packages.setDeviceRepositories` | the profile's `repositories`, `hideOfficial` | the same |
| Capture's buffer: `record.retro` | the configuration folder's `capture` (Settings' choice), else `compute.captureSeconds`, else 120 or 600 s by memory (`readCaptureSeconds`, SessionSetup.cpp:49) | the engine at start (AudioEngine.cpp:202); the JUCE app for its own engine (Main.cpp:1061, not for `--snapshot`) |
| The UI's own memory: `memory.shell` (new) | the UI process's resident memory | each UI, on connecting: the JUCE app with its setup (it was part of `budget.set`'s `system`), the Rust shell (ui/src/shell.rs:263) |
| The Performance last open: `performance.reopen` | the engine's own record | each UI (Main.cpp:1098 with a separate engine, not `--play`; shell.rs:265) |
| The device's control groups: `controllers.set` | the profile's controls | the engine with `--device-profile` already took them (AudioEngine.cpp:194); the JUCE app sends them again (Main.cpp:152) and its `Controllers` adds external controllers' and Mackie Control (Controllers.cpp:91), not ported |
| What the screen shows: `controllers.view` | the UI's lists, part and page | each UI (Engine.cpp:177; the Rust shell's `lists`) |
| The engine's options and environment: the audio device, panel, keys, keyboard (`Profile::configureAudio`, Main.cpp:947), `OSCAIRN_RELIABILITY`, the audio cores and worker settings (Main.cpp:997-1010) | the profile | the JUCE app for the engine it starts; for the engine alone, os/scripts/engine-args.py and the start script, as before |
| On reconnecting (`Engine::attach`, Engine.cpp:166): `engine.attach` (a managed engine), `record.retro` when the length differs, `controllers.set`, `controllers.view` | the app's own state | the JUCE app; the budget, classes and repositories were never sent again, which an engine started with the profile now applies itself |
| Opening an app: `memory.launch`, `memory.closed` (Launcher.cpp:336); a process app's `apps.connect` | Home's tiles, the Apps menu, `--app` | the JUCE app; not at start. The Rust Setlist does not ask a reservation yet. A launch is now let go when the connection that asked it closes without `memory.closed` (a shell killed), as an app's own connections are |

After a Rust-path start (the engine started by the start script with the profile, then the Rust
shell) the engine reports the same budget, memory classes, capture and repositories as after a
JUCE-path start; only each UI's own memory differs. `session_setup_matches_the_juce_app`
(ui/src/tests.rs) checks it with the real engine and the real JUCE app (offscreen: `--connect`
with `--snapshot`, which leaves capture out, so the test sends the app's `record.retro` itself)
and the FANTOM-07's figures measured on a unit: `cpu` 0.75, `device` 1,040,187,392 B, `floor`
300 MiB, `margin` 49.6 MiB, `available` 200 MiB, the two always-available apps, capture 30 s; the
repositories are compared between the two paths. The engine applies the setup only with `--device-profile`, which engine-args.py
passes for a profile with a panel (the FANTOM-07 has one).

## Themes

`theme.rs` reads themes as the JUCE app does ([design-language.md](design-language.md#one-format-for-both-uis)):
built-in folders beside the program (`share/oscairn/themes`, or `themes/` beside it), installed
ones in the configuration folder's `themes/<id>/`, the choice in `appearance.txt`, else the
profile's screen theme, else `themes/default`, else Oscairn Dark. A theme chosen in either UI
shows in both; the shell reads the choice once a second (a small file; nothing is drawn unless it
changed) and a switch repaints the window once. Every color, the radius, the type scale and the
font family on the Rust screens come from the `Theme` global `theme.rs` fills (`slint/theme.slint`
holds only what shows before that); a key a theme lacks or cannot use is the default theme's.
The resource bars fill with `accent`, then `warning` and `danger`; cursors are a `focus` border.
Without a theme folder on disk (a development build) the themes this program was built with are
used, with the built-in Inter font.

## The top bar

From the left: Home (not on Home itself), where the musician is, the open Performance and its
scene (`performance.info`, `graph.get`, asked again on each `graph.changed`), the tempo
(`transport.get`) and the resource section. Everything right of the Performance has a fixed
width, so a changing figure never moves anything.

The resource section shows two meters, CPU over RAM:

- **CPU** is how full the CPU is for the audio: `session.status`'s `cpuLoad`, the audio device
  callback's share of each block's time (JUCE's `getCpuUsage`), the figure the JUCE top bar
  shows beside the tempo. 100% is a callback that takes the whole block: the next one is late
  and the audio breaks up. It is not the whole machine's CPU use.
- **RAM** is the device's memory in use: the profile's `compute.memory` (992 MiB on a FANTOM-07;
  else the budget's `device`) less what the system has available (`memory.state`'s
  `systemAvailable`, Linux's MemAvailable), as a share of it. 100% is nothing left available.
  Shown as "–" where the engine cannot tell (macOS reports 0).
- Each is yellow from 75% and red from 90%, as the JUCE bar colors the load.
- **What shows:** for each of `cpu` and `ram`, `both` (a bar and its percent), `bar`, `percent`
  or `none`. The profile sets it on its screen (`screens[].topBar`, absent: both), and the user
  over it in Settings' Top bar page, which keeps it in the configuration folder's `top-bar.json`
  (beside `appearance.txt`; `OSCAIRN_CONFIG_HOME`, else `~/.config/Oscairn` or
  `~/Library/Application Support/Oscairn`). While Settings is a JUCE screen, the Rust UI reads it
  when it starts, which it does after every JUCE screen.
- **Fixed width:** the names' column, a bar column when either meter has a bar, a percent column
  when either has a percent; the percent is three fixed digit cells and the sign, so "1%" and
  "100%" take the same room whatever the font. The tests render all sixteen settings with 1%
  and 100%, tempos of 5 and 999.9 and a very long Performance name and check that the section
  and the tempo never move.
- **Cost:** `status` comes at most four times a second; the meters move at most twice a second
  (`memory.state` and `transport.get` are asked then), and a property is set only when what it
  shows changes (a whole percent, the tempo's text). The tests check that an update repaints a
  rectangle inside the section and that no pixel outside it changes, and that nothing is drawn at
  rest.

## The Setlist

`setlist.rs` and `slint/setlist.slint`, from the prototype: the setlists on the left (pane 0, E1 on
a FANTOM-07), the open setlist's slots on the right (pane 1, E6), the VALUE dial on the pane last
touched. The slots show a view at a time (the setlist's grid, or as many list rows as fit); the
cursor leaving the view turns a whole view. Confirm or a tap opens a slot (`library.load`); a slot
chosen while one opens waits and opens next, the last chosen winning, as the JUCE engine client
queues library loads; a load refused as busy (`-32009`, another client's long request) is tried
again as the next; the next slot is prepared (`performance.prepare`). Left, Right, Page Up and
Page Down are Previous and Next, as in the JUCE Setlist (page turners send them); Up and Down move
the cursor. The setlist open and the slot playing are kept in the JUCE Setlist app's own state
file (the data folder's `AppData/oscairn.setlist/state.json`), so either opens where the other
left off. Not yet: editing, the setlist's own Previous/Next asked from Play (`oscairn.step.setlist`,
which needs Play in Rust).

## Adding a screen

1. **The view:** `slint/foo.slint` with a global `FooState` holding what it shows (its models,
   names) and its callbacks (taps), and a component drawing it from `Theme` only. Import and export
   both in `shell.slint`, place the component under the top bar, shown for its `screen` number.
2. **The model:** `src/foo.rs`, a `Screen` that keeps what it shows, sets a row or property only
   when it changed (`update` in `setlist.rs` does it for a model), and puts what it asks the engine
   in `out` as `(Request, method, params)`. It takes navigation as `act(Action) -> Outcome`, with
   its own `Cursor` per pane; a tap does what Confirm does.
3. **The shell:** a `Screen` value and a `Request` variant in `shell.rs`; route its replies, its
   actions (only while it is shown) and its taps through `on_foo` as `on_setlist` does; a Home tile
   opens it in `open` instead of handing the display over.
4. **Tests** in `src/tests.rs`: headless renders (`Screen::new`, `frame` gives what was repainted),
   against `Fixture`'s fake engine on a socket, and a picture saved to `target/screens`.

## Building, running and testing

- `cd ui && cargo build` (a window, on a computer); `cargo build --release --no-default-features
  --features kms` for a device. `oscairn-ui --socket PATH` against a running engine; without a
  `--handover` file a JUCE screen's tile says where that screen is instead.
- `cargo test` (or ctest's `ui-rust`): every test renders headless through Slint's software
  renderer into a buffer (no window opens), with data and configuration folders of their own;
  `OSCAIRN_ENGINE=…/oscairn-engine cargo test` adds the test against the real engine, started with
  `--audio null`. `cargo clippy --all-targets` is clean.

## On a device: the run recipe (not yet run on a unit)

1. Build the bundle (`os/fantom-bundle.sh`): it carries `usr/bin/oscairn-ui` and its libraries,
   with the start and stop scripts and the JUCE app of the same tree.
2. Unpack it as any bundle; nothing is copied in by hand.
3. Start: `OSCAIRN_UI=rust ./oscairn-fantom-start` (the profile still says JUCE); stop with
   `./oscairn-fantom-stop`. `SLINT_KMS_FRAME_STATS=1` in `etc/default/oscairn-engine` is not passed
   on; for frame statistics run `oscairn-ui` by hand through `start-chroot.sh` with it set.
4. Measure, with ST Final playing and keys held, as the prototype's runs: the engine's deadline
   misses and silenced blocks (`oscairn-engine-timing`) and `oscairn-ui`'s CPU (`/proc/PID/stat`)
   over 10 s of moving the cursor on Home and through a long Setlist (the dial held turning; a view
   turns each nine or so slots), and at rest; the top bar at rest and updating (CPU of the shell
   with nothing touched: the meters move twice a second); resident memory
   (`/proc/PID/smaps_rollup`).
5. By hand: a tap lands where it is drawn (the touch rule); the VALUE dial, E1 and E6, ENTER and
   EXIT move, confirm and cancel on Home and the Setlist; confirming Settings hands the display
   over and its Home comes back: time both ways from the keeper's lines in `oscairn-fantom.log`
   (whole seconds) or a stopwatch.

