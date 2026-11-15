# The Rust UI

The owner decided on November 14, 2026 that Oscairn's UI moves to Rust ("Rust direction: yes on
ui and readers") and that the Rust UI replaces the JUCE app screen by screen, as each is ready
("yes want rust to replace juce as each one is ready"). The first screens are the stage ones:
the shell with its top bar, Home, Setlist, Play and Part. Its toolkit is Slint 1.18.1 with
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
- **A screen not yet ported** (Library, Groovebox, Recordings, Settings, and the Performance's
  Edit, today; the Performance tile itself opens Play, in Rust):
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
  the Library and the editors and never needed on stage, where Home, Setlist and Play are
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
| The budget: `budget.set` with `cpu` (the reliability level's share, 0.75 at `instrument`), `memory` (80% of `compute.memory`), `available` (`compute.availableMemory`) and the memory classes (`device`, `system` = `osMemory`, `floor` = `performanceMemory`, `margin` 5%), off when they do not fit | the profile's `reliability` and `compute` | `setUpSession` (hosts/standalone/SessionSetup.cpp), applied by the engine at start with `--device-profile` (hosts/audio/AudioEngine.cpp) and by the JUCE app to an engine it starts (hosts/standalone/Main.cpp); before, Main.cpp's `started()` alone |
| The always-available apps: `memory.alwaysAvailable` | the profile's `alwaysAvailable` (default `oscairn.performance`, `oscairn.library`), each at its manifest's `memory.reserve` (the Apps folder, hosts/standalone/AppManifest.cpp) or Oscairn's placeholder (`Profile::appMemory`) | the same |
| The device's package repositories: `packages.setDeviceRepositories` | the profile's `repositories`, `hideOfficial` | the same |
| Capture's buffer: `record.retro` | the configuration folder's `capture` (Settings' choice), else `compute.captureSeconds`, else 120 or 600 s by memory (`readCaptureSeconds`, SessionSetup.cpp) | the engine at start (AudioEngine.cpp); the JUCE app for its own engine (Main.cpp, not for `--snapshot`) |
| The UI's own memory: `memory.shell` (new) | the UI process's resident memory | each UI, on connecting: the JUCE app with its setup (it was part of `budget.set`'s `system`), the Rust shell (ui/src/shell.rs) |
| The Performance last open: `performance.reopen` | the engine's own record | each UI (Main.cpp with a separate engine, not `--play`; shell.rs) |
| The device's control groups: `controllers.set` | the profile's controls | the engine with `--device-profile` already took them (AudioEngine.cpp); the JUCE app sends them again (Main.cpp) and its `Controllers` adds external controllers' and Mackie Control (Controllers.cpp), not ported |
| What the screen shows: `controllers.view` | the UI's lists, part and page | each UI (Engine.cpp; the Rust shell's `lists`) |
| The engine's options and environment: the audio device, panel, keys, keyboard (`Profile::configureAudio`, Main.cpp), `OSCAIRN_RELIABILITY`, the audio cores and worker settings (Main.cpp) | the profile | the JUCE app for the engine it starts; for the engine alone, os/scripts/engine-args.py and the start script, as before |
| On reconnecting (`Engine::attach`, Engine.cpp): `engine.attach` (a managed engine), `record.retro` when the length differs, `controllers.set`, `controllers.view` | the app's own state | the JUCE app; the budget, classes and repositories were never sent again, which an engine started with the profile now applies itself |
| Opening an app: `memory.launch`, `memory.closed` (Launcher.cpp); a process app's `apps.connect` | Home's tiles, the Apps menu, `--app` | the JUCE app; not at start. The Rust Setlist does not ask a reservation yet. A launch is now let go when the connection that asked it closes without `memory.closed` (a shell killed), as an app's own connections are |

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
left off. Play's Previous and Next step it (`step_by`, as the JUCE Play asks the Setlist app to).
Its Play button opens Play. Not yet: editing.

## Play

`play.rs` and `slint/play.slint`: the JUCE Performance app's first view (PlayView.cpp) for the stage.
Home's Performance tile and the Setlist's Play button open it (Cancel goes back to where it was
opened from); Edit is the handover.

- **Shows:** the open Performance's name and scene in the top bar as everywhere; eight scene tiles
  along the top (the one playing filled in the accent with an accent border); below them a strip per
  part, then per bus, each with its name, its sound (the first Instrument in its chain, "No
  instrument" otherwise) and where it plays from (`Any device · C2–C7 · −12`, `Audio in 1`, `Its own
  notes`; a bus: `Bus · its first plugin`) as PartsModel.cpp's `readParts` words them, a level
  fader (the JUCE faders' skew: a quarter of the way up is half the travel), a meter of one or two
  bars (the first channel, and the loudest of the others) along -60 to +6 dB in the three zones,
  mute (M) and solo (S); a strip is dimmed while its part is muted or another part's solo
  silences it. The master (Performance level) is pinned at the right. Previous and Next show at the
  ends of the scene row while the open Performance is the Setlist's slot playing (`performance.info`'s
  `id` against the Setlist's `current`), as the JUCE Play shows them. The strips show as many at a
  time as fit (as many as the screen holds at 110 px each); the cursor past them turns a whole view.
- **Without touch:** pane 0 (the left knob, E1) moves a border cursor over the scenes, pane 1 (E6)
  over the parts, the dial the pane last touched; a press (ENTER) on a scene selects it
  (`scenes.select`), on a part selects it (the Part screen, below); EXIT drops the cursors, then goes
  back. Keys: Left and Right move along the pane last touched, Up goes to the scenes, Down to the
  parts, Home and End the first and last, Page Up and Down are Previous and Next (as in the
  Setlist), Return confirms, Escape cancels. Touch: a scene tile selects, a tap on a strip's name
  selects the part, M and S toggle (the one-press mute on the panel is gone: a press opens the part,
  where mute is a row), a fader
  is touched or dragged (`parts.level`, `master.set`, sent each time it moves a hundredth).
- **From the engine:** `graph.get` (the shell already asks it on each `graph.changed`; Play adds
  no request to it), `controls.changed` for levels and mutes moved live (subscribed on opening Play
  and ended on leaving, because it comes up to 60 times a second), and `parts.meters`, asked by a
  100 ms timer that runs only while Play is shown, one request at a time (a slow engine never gets
  a second). The shell at rest still wakes for nothing. No new engine request.
- **Still handed over to the JUCE app**, from the "Edit, knob banks, Learn: Oscairn app" entry
  (the display goes to the Performance app, which opens on its own Play): the knob row and its
  banks (the Performance's controls or a part's, resolved through the panel's layers), Learn, the
  Performance/part switch and bank button, Edit itself (the part pages, effects,
  routing, saving), adding a scene (the "+ Scene" tile), the "No MIDI device" line with the
  on-screen keyboard, and Save. Not ported either: a part's pan and sends have no control in
  Play (the JUCE Play has none; the Part screen has the pan), and meters past two channels show two bars.
- **Not measured:** the 100 ms meter poll's cost on a unit, and its look at the FANTOM-07's scale.

## Part

`part.rs` and `slint/part.slint`: the JUCE Part page (PartsView.cpp, PartsModel.cpp's `partFields`)
for one part, opened from Play by selecting it (a press on it, a tap on its name). Back returns to
Play; Play's Cancel still goes to where Play was opened from.

- **Settings** (a list with a border cursor, a view of as many rows as fit): Level (dB), Pan, Mute,
  Solo; for a part that plays notes also "Plays from" (the devices, below), Lowest key, Highest
  key, Transpose (+-24, as the JUCE slider), and the Sustain, Sostenuto and Soft pedal each: Every
  pedal, None, or Its devices' pedals (the JUCE page has one setting for sustain and sostenuto
  together; here the three are apart, as `receive` has them). A bus or an audio part has the first
  four only. The last row hands over (below). Read only beside it: the sound (the first Instrument,
  and the library Sound it came from, "edited" when changed), its engine (the plugin), where it
  plays, its sends with their levels, its insert effects (each chain node but that instrument, "off"
  when switched off). It shows what fits; the right knob (pane 1) or the panel's arrows scroll it a view at a time.
- **Without touch:** the left knob (E1) or the dial moves the cursor (E6 scrolls the panel); a press on a value starts
  changing it (the row turns accent) and the dial then changes it, each step sent at once (level a
  fiftieth of the fader's travel through the JUCE skew, pan 0.05, keys and transpose a semitone,
  a pedal one choice); a press or Cancel ends the change. A press on Mute or Solo toggles it. A
  press on "Plays from" opens the devices list (Any, Built in, All external, All USB, Network, then
  each input of `session.status`), where a press ticks or unticks (Any clears), as the JUCE menu;
  Cancel closes it. Cancel otherwise: ends a change, then drops the cursor, then goes back. Keys:
  Up and Down move the cursor, Left and Right change the value, Return and Escape.
- **Touch:** a tap on a row is a press; the - and + beside a value step it; Back at the top left.
- **Requests:** only existing ones. `parts.level` (level and mute), `parts.pan`, `parts.solo`, and
  `parts.input` for the devices, the first zone's keys and transpose and the `receive` of each
  pedal, sent with the part's input as `graph.get` gave it and the one setting changed (a zone
  that passes everything is dropped and the highest key never goes under the lowest, as the JUCE
  `setZone`; a pedal at `all` is left out, as a save writes it). From the engine: `graph.get`
  (the shell's), `controls.changed` while Play or Part is shown (one subscription across both),
  `session.status` once on opening and its notifications (the inputs). A refusal is said and the
  graph asked again. The engine core's note router delivers each part's pedals by `receive`
  (`NoteZones::takesPedal`), so the pedal rows are heard.
- **Also here** (from the JUCE `partFields`): Input channel (any, 1 to 16; "Several channels" is
  shown, not picked), Lowest and Highest velocity (0 to 127), Notes at once, Reserved notes (within
  the limit), Priority (0 to 10), Plays to (Main or a bus), MIDI out (a list of the outputs: Off,
  each output of `session.status`, rules whose output is not connected) and its channel, a Send to
  each bus (0 removes it), Name (a press opens the on-screen keyboard, `keyboard.rs`), and Remove
  part (a second press removes it and goes back to Play). Requests: `parts.input`, `parts.midiOut`,
  `parts.output`, `parts.send`, `parts.name`, `parts.remove`. Every edit the engine stages (those
  and the input, the devices, keys, transpose and pedals before them) is followed by `graph.commit`,
  as each JUCE setter commits: before this the Part screen sent `parts.input` alone, which the
  engine only stages, so those edits were not compiled until something else committed. Level,
  pan, mute and solo are live and are not committed.
- **Still handed over:** nothing in this screen: the handover row is gone. Not yet built here (the
  checklist is ui/PERFORMANCE-APP.md): splits and lanes, scenes editing, the knob row and Learn,
  Save, undo and redo, the Parts list, Controls, Mixer and Routing; Play's "Edit" entry still hands
  over until those are built.

### Insert chain, parameter pages and the sound chooser

Three screens opened from the Part screen's "Insert effects", "Sound" rows and a plugin's
"Parameters" action, sharing the Part component through `browser.rs` (the list, its cursor and the
panel; `Nav` turns an action into a press, a dial step, the right knob or back). Their tests are
`src/tests/pages.rs` and `shellpages.rs`.

- **Chain** (`chain.rs`): the plugins in order with "+ Insert plugin here" before each and after
  the last; a press on a plugin lists its actions: Parameters, Turn off or on (`graph.enable`),
  Move earlier or later (`parts.move`), Remove (`parts.removeNode`, then `graph.remove` with
  `userWires: reconnect`, as `removeBox`), and, where `window.info` says the plugin has a window of its
  own, a disabled "Plugin view: not available in this shell yet" (the only part of the Performance app not
  ported: a plugin's own view is drawn into a surface the shell hands it, a separate project). Inserting
  lists the plugins the scanner found and sends `graph.add` (named "Basic Synth", "Basic Synth 2"), then
  `parts.insert`, taking the node out again with `graph.remove` if the insert is refused. Every edit is
  followed by `graph.commit`. Splits and lanes are listed read only (D7 to D9 are not built).
- **The plugin scanner** (`scan.rs`): as the JUCE app finds plugins, the helper `oscairn-plugin-scan`
  beside the program run once per plugin file (`--worker FILE`, 5 s, 4 MiB) over `--plugins DIR`
  (else `plugins` beside the program, else `../lib/clap`), CLAP then LV2, started the first time the
  chain is opened, its wire format read with every length checked. It is a helper process, not an
  engine call: no control request. A timed-out helper is killed with its process group, as the C++ one does.
- **Parameter pages** (`params.rs` on the grid, `grid.rs`, `slint/grid.slint`; ui/GRID.md): host-drawn
  from the plugin's own layout data in `params.list` (widget, priority, group, range, views), as the
  JUCE ParameterGrid: a page per top-level module, tabs on a page of more than 24, headings, tiles in
  priority order in two to six columns for the width given, linked runs on one card, an envelope's
  curve above its stages; knobs, sliders, switches, dropdowns, split buttons, ranges and values, each
  with the plugin's own text (`params.text`). `params.get`, `params.set` inside `params.gesture`;
  `params.changed` while shown; the page shown is published with `controllers.view` `screen` for the
  device's screen knobs and withdrawn on leaving. Without touch: dial in reading order (past the end
  the page turns), left knob columns, right knob rows, press selects, dial changes, Cancel backs out.
  Not yet: key maps, FM operators, LFO and filter views, dragging the envelope, the search.
- **Sound chooser** (`sounds.rs`): `library.query` of kind `sound` (a search typed on the keyboard
  narrows it), and for the one pressed `parts.loadSound`, `parts.fromSound`, `audition.start` and
  `audition.stop`, and `parts.saveSound` (over its Sound, or new with a name typed). The engine's refusals
  (staged edits, the library busy) are said. Tags, favorites and the sources are not shown.

### Playing keyboard

`src/piano.rs`, opened from the Part screen's "Keyboard" row (not shown where the profile's `onScreenKeyboard` is false). It plays as the input that plays the part (`notes.send` with `input`; the built-in keys when none), keeps the JUCE KeyboardWindow's behaviors (computer keys from middle C, Z and X, Fixed, Sustain, bands per part, the input's held keys lit from `status`'s `heldNotes`, closing with its input) and lays out two octaves and the next C for the touch screen. Without touch: left knob the octave, right knob or dial the cursor over keys then the header buttons, press plays or acts, Cancel drops the cursor then closes. Multi-touch chords on the real display are unverified.

### Text entry

`keyboard.rs`, `slint/keyboard.slint`: the JUCE TextKeyboard (hosts/standalone/TextKeyboard.cpp)
over the screen that asked, with its keys and behavior: the text with its caret, Left, Right,
Clear, Cancel, Done; q to p with 1 to 0 on the symbols layer and Del; a to l; Shift (the next
character) and z to m and the period; the layer key, Caps, space and , - / @. A tap types on
press; Del, Left and Right repeat while held. Where the profile's `screens[].textEntry` is
`keyboard` (the default, a computer) the rows of keys are left out and typed keys, Backspace, the
arrows, Return and Escape do the typing. Added for use without touch: a border cursor on a key
(the dial or left knob steps along the keys, the arrows move across and down, End finds Done, a
press types), the right knob moves the caret. Changed from the JUCE one: keys are as tall as the
screen allows (up to twice a touch target) and the text, Cancel and Done are in the first row
as proportions of the width, not fixed pixels.
- **Missing from the engine:** nothing found that the JUCE page uses in process and has no
  request: every setting above has one. (The sound picker and plugin insertion go through the
  JUCE app's own scanner and library views, not tried here.)

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

