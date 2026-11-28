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
- **A screen not yet ported** (Library, Recordings and Settings, today; the Performance
  tile opens Play and Play's Edit the Parts list, both in Rust):
  confirming its tile shows "Opening Settings..." for one frame, writes the app's name to the
  handover file and ends the shell (exit status 3). The start script's UI keeper then starts the
  bundle's Xorg and `Oscairn --connect /run/oscairn/engine.sock --app settings` with
  `OSCAIRN_HOME_QUITS=1`, so that app's Home and Back end it. When it ends, for any reason, the
  keeper stops Xorg and starts the Rust shell again, which opens on Home.
- **Time, estimated, not measured on a unit:** in, about 2 to 3 s (the shell ending, about
  0.1 s; Xorg serving a client, about 1 s by the start script's own polling; the JUCE app's
  first paint, 0.2 to 0.3 s in the prototype's container on a fast CPU, taken as 1 to 2 s on the
  unit); back, under a second (Xorg ending, the shell starting, about 0.15 s in the container).
  Measured on a FANTOM-07 from the logs: back took about 3.3 s, 2.5 s of it from the app's exit
  to Xorg being gone. Each phase is now logged with wall-clock milliseconds: the keeper's
  `timing` lines in `var/oscairn/log/oscairn-fantom.log` (handover requested, X ready, app
  started, app exited, X gone, shell started, each with the time since the request), and
  `oscairn-timing:` lines in `oscairn-app.log` (app started, engine connected, first frame, quit
  requested, shut down) and `oscairn-ui.log` (shell started, shell first frame). The keeper
  polls the X server every 20 ms and signals it by its own pid, with no scan of `/proc`.
  The device run measures both.
- **What the musician sees:** Home looks the same with the screen either app draws; a tile of a
  JUCE screen opens after a dark screen of 2 to 3 seconds, and its Home button comes back to the
  Rust Home after a dark moment. Audio plays on throughout. That wait is acceptable for Settings,
  the Library and the editors and never needed on stage, where Home, Setlist and Play are
  Rust and draw without X. While a JUCE screen is open it has the X cost as today.
- **One profile setting switches it:** a screen's `"ui"`: absent or `"rust"`, the Rust UI;
  `"juce"`, the start script starts the JUCE app as before. The Rust UI is the default since
  2026-11-25 (the owner: "make rust the default now"); `OSCAIRN_UI=juce` in the start script's
  environment forces the JUCE app, for a fault. Screens not yet ported still hand over to JUCE.

### 2. Where it lives and how it builds

- `ui/`: one Cargo package, `oscairn-ui`, with its `Cargo.lock` and a `rust-toolchain.toml` at
  the release `OSCAIRN_RUST_VERSION` in `CMakeLists.txt` pins (the samplepack crate's). One crate,
  not a workspace: one program. Screens are modules of it.
- `ui/vendor/i-slint-backend-linuxkms-1.18.1`: Slint's KMS backend, vendored with a frame cap,
  frame statistics and two display buffers in place of three (with three every frame was painted
  whole on a device), through `[patch.crates-io]`; its README says where it came from and what
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
  first item shown; a 2 px `focus` border, as the JUCE lists draw it; moving opens nothing; Cancel is Back (below), not a step that drops it first; it drops on a tap and when its screen goes; past the visible items the list turns a
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
| The Performance last open: `performance.reopen` | the engine's own record | the engine itself at its start, not managed nor with `--startup-performance` (hosts/engine/main.cpp); each UI too (Main.cpp with a separate engine, not `--play`; shell.rs), which keeps what an engine that asked already opened |
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

## The top bar carries the chrome

Every screen but Home has Back on the bar, and no screen draws a Back button or a title row of its own (a test reads
the screens' .slint for one). The bar's left side is Home, Back and the title: the path of what is open (the
Performance's name and scene in a field on Play, where a tap is Save As New; "Keys", "Keys › Insert effects › Chorus",
"Parts › Mixer" below it), shortened from the left (`… › `), then its last part shrunk to 70% of the type, then cut. Then
the screen's own actions (the parameter pages' Search, Routing's "+ Plugin", "Parts collapsed" and "Meters", the Mixer's
strip and send pagers), then Undo, Redo, Save and the rest, then the recording mark, the tempo and the meters. A narrow bar
folds all the actions to icons and then into one "More" before it cuts anything; Back and the recording mark stay. "More" is
an icon button (the three dots, `ellipsis-vertical.svg`, labeled "More" for accessibility) at the right end of the action
area, directly left of the meters, in the cursor's same order (after the title); the room it gives up goes to the title.
Without touch: Back is the bar's first cursor stop, and Cancel (Escape; a device's back button, `cancel` in its panel
map, which the engine counts in `controllers`' `navigation`) is Back in one step from anywhere on a screen, whatever the
screen's cursor or a value being changed is doing; a menu, a keyboard or a dropdown over the screen takes the Cancel
first. Back goes one step up the path: out of a screen to the one it was opened from, up a level of the chain or the
sound chooser, closing the Part's list of devices. Each screen has the old header row's height (a touch target and a gap)
for its content.

## The top bar

From the left: Home (not on Home itself), where the musician is, the open Performance and its
scene (`performance.info`, `graph.get`, asked again on each `graph.changed`), the tempo
(`transport.get`) and the resource section. Everything right of the Performance has a fixed
width, so a changing figure never moves anything.

A narrow bar gives up, in this order, only as much as it must (`slint/topbar.slint`: the widths of the items are
measured, the stage is chosen by what the Performance's name would be left with): the actions' words (Undo and Redo
become icons, then all the actions fold into one "More", a menu of them: the cursor's one stop, a press or a tap
opens it, a disabled action cannot be chosen; the icon button is `Columns.action` wide, at the bar's right end before the meters)
while the name keeps 16 small-type heights of room; then the name's
room goes down to 9 (its text shrinks to 70%, then is cut); then the meters lose their names (CPU, RAM), then half
their bars' length; last, a slow disk's "disk slow" is shortened to "slow". The recording mark and the tempo are never
dropped. In the roomy odd test theme (type 1.39, spacing 1.6) the parts that never give way are wider than 640 or 826
with a slow disk: it overflows there (tested only from 1200 across).

A take recording (the hardware REC button, or any app's) shows between the Performance and the
tempo on every screen: a dot, "REC m:ss" and "disk slow" when the writer is behind, in the danger
color (once: Play has no mark of its own). It is what the JUCE record buttons show (`RecordButtons`), read as
they read it: `record.get` once with the first `status` of a connection (it also says how many seconds the retroactive buffer keeps), then the engine's `record`
topic (a take's state or time changed: at most ten a second while one runs, none at rest), the first take
`waiting` or `recording`, its `frames` over its `sampleRate`. Record and Capture are the last two actions of every
screen's bar, Home's included, as the reference draws them over every app: Record starts a take (`record.start`) or ends
the one recording (`record.stop`); Capture saves the last 30 seconds the buffer keeps (`record.start` from `secondsAgo`,
named for the Performance; off while the buffer keeps nothing). They are icons, and words in More.
A `status` is compared by what the shell reads of it (the inputs and outputs, the failures, the keys held, the audio
device, the memory the workers hold in whole MiB, and the load as the whole percent the bar shows: the engine's per-worker
figures change with every one and are not compared); one that says what the last said asks nothing and draws nothing, a
changed load moves the figure only, and something else changed asks `memory.state` and `transport.get`, at most twice a
second. The tempo follows the `transport` topic.

On the Performance app's screens (Play, Part, the chain, the sound chooser, the parameter pages, the
scene's setup) the bar also has the screen's actions after the Performance's name: "Undo <edit>" and
"Redo <edit>" (`edit.history`, asked on entering a screen and on each `graph.changed`; `edit.undo`,
`edit.redo`), "Save" (`library.save`: over a Performance in the library at once, else the name asked
on the text keyboard; the name on the bar is Save As New) and, on Play, "Scene". What an action says
(Saved, the engine's refusal) replaces the Performance's name for four seconds. Without touch the
bar has a cursor: the third list pane (`move:2`), Tab, or the dial moving up out of the first row of
Play, Part, the chain or the sounds list; it moves along the name and the actions, Confirm does one,
past the last or Cancel gives the cursor back to the screen. The parameter pages (grid.rs) are
reached by `move:2` and Tab only: they need a hook for the dial.

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
  or `none`. The default is both on every device (the profile's `screens[].topBar` is no longer read for it), and the user
  chooses in Settings' Top bar page (Bar and percent, Bar, Percent, Hidden), which keeps it in the configuration folder's `top-bar.json`
  (beside `appearance.txt`; `OSCAIRN_CONFIG_HOME`, else `~/.config/Oscairn` or
  `~/Library/Application Support/Oscairn`). While Settings is a JUCE screen, the Rust UI reads it
  when it starts, which it does after every JUCE screen.
- **Fixed width:** the names' column, a bar column when either meter has a bar, a percent column
  when either has a percent; the percent is three fixed digit cells and the sign, so "1%" and
  "100%" take the same room whatever the font. The tests render all sixteen settings with 1%
  and 100%, tempos of 5 and 999.9 and a very long Performance name and check that the section
  and the tempo never move.
- **Cost:** `status` comes at most four times a second; the meters move at most twice a second
  (`memory.state` and `transport.get` are asked only when something besides the load changed), and a property is set only when what it
  shows changes (a whole percent, the tempo's text). The tests check that an update repaints a
  rectangle inside the section and that no pixel outside it changes, and that nothing is drawn at
  rest.

## Waits

The rule: **an expected wait shows the loading indicator; the banner is for unexplained faults.** Opening a Performance is a
normal wait (a fraction of a second to a few seconds; 4 to 20 s for a big one on the FANTOM, docs/control-protocol.md,
"Long requests"), so it never reads as a fault.

- **The indicator** is `slint/loading.slint`, one component with a text and an optional progress (0..1; negative: none,
  and the bar sweeps; a progress fed in later needs no change of layout), its thickness and sweep period the theme's
  (`Theme.loading-bar`, `loading-segment`, `loading-period`; a test reads the file for a literal). As the top bar's chip
  (`filled`) it is a yellow pill: `Theme.loading-background` (the theme's `warning` unless a theme restyles it) with
  `loading-text` (`on-warning`, black or white, whichever reads better: a test holds 4.5 to 1 in every theme) in the bar's
  label size, the bar inside it in the text's color; unfilled it is the bar alone (a tile's or a row's foot). Others use it as is (a library opening, a package
  install, an export). It sweeps through `animation-tick()`, so the window redraws only while it exists: the shell makes it
  while a wait lasts and removes it when it ends, and a test holds that no frame follows (`tests/loading.rs`).
- **Where:** the top bar's title area, as a chip at its right edge, "Loading NAME", beside the Performance that is still
  shown (the title's room shortens for it; the chip is at most half of it and its name is cut with an ellipsis, never
  wrapped, and nothing else on the bar moves). On the Setlist the slot being opened, and in the Library the row, also have the
  bar along their foot (the row's old "Opening" text is gone). Nothing covers the screen, nothing blocks Back, EXIT or the dial.
- **What names it** (`shell/wait_glue.rs`): the name the UI itself asked for (the Setlist's slot, the Library's row: not while
  the question about unsaved changes is open, as nothing has been asked of the engine); "Loading the last Performance" for
  `performance.reopen`; "Loading a Performance" when the engine's busy answer names a load another client asked for
  (`data.busy`, then `engine.busy` ends it: the engine tells no name); "Working" for another long request. The engine reports
  no progress, so the bar sweeps. A protocol addition that tells the name and progress is wanted (see ui/PROGRESS.md).
- **Busy answers** during a load are never errors: reads and steps are asked again as before (after `client::BUSY_RETRY`,
  100 ms, the Library's and the Setlist's alike: the Setlist's refused load stays "opening" meanwhile), and a screen that shows a refusal
  says "Still opening a Performance. Try again in a moment." (`client::refusal`), not the engine's text. An edit refused during a
  load is not retried: it would land on the Performance that opens next.
- **The banner** under the bar stays for three cases and says what is happening in plain words, in the danger color so it
  reads as something other than the yellow chip; it takes its own room (the screen starts under it, nothing is hidden) and the
  chip steps aside while it shows: the connection lost, a wait far past its usual (30 s, half as long again as the slowest
  measured load: "Opening X is taking much longer than usual. Wait for it, or restart the instrument if it never ends."), and a request that is no load, no
  install, with no answer after 2 s (reading a setlist: "The sound engine is slow to answer...").

## The Setlist

`setlist.rs` and `slint/setlist.slint`, from the prototype, in the owner's layout (2026-11-21): no
sidebar and no footer.

- **The top bar** is the system's, with the screen's two actions, Play, which opens Play, and Manage (reached by
  the bar's cursor, `move:2` or Tab, like any screen's actions).
- **The setlist bar** is one row directly under it, where the Library has its row but a quarter
  taller than a touch target (`Theme.tall-row`, the touch target times `tall-row-scale`). **Previous**
  and **Next** are at its right end, the theme's small button each; the **setlists' dropdown** (it
  names the setlist open; "No setlist" and off with none) and the **banks' dropdown** (a setlist's
  `pages` are its banks: the bank's name, or "Bank N" without one, and "n/m") share the rest equally.
  Nothing else is in the bar. The footer's texts: "Set 1 (1/2)" is the banks' dropdown's; "1–9 of 30"
  shows only when a bank has more slots than a view, as a line directly under the last row (where the
  Library has its count); "32 slots" and the name of the slot chosen are gone (the loading chip and
  the slot's own marks say what opens); why a slot did not open, and a refusal, are said in the top
  bar's title for four seconds, where the bar's actions say theirs (`Bar.notice`), on the Setlist and
  on Play.
- **Previous and Next turn the bank**, as on the reference keyboard: the bank before or after is
  shown, nothing is asked of the engine and the slot playing stays the one it was; a song opens only
  when it is chosen. At the first and last bank the button is off (no wrap). With one bank the banks'
  dropdown and both buttons are shown off, so the row never moves.
- **A dropdown** opens the shared menu (`menu.rs`) over the screen: its list with a border cursor, the
  one open ticked and the cursor on it; the dial moves, nothing is read or shown until the press (`setlists.get` for a
  setlist; a bank asks nothing), Cancel closes it unchanged.
- **The slots** of the bank shown have all the room under the bar, a view at a time (the setlist's
  grid, or as many list rows as fit); the cursor leaving the view turns a whole view, and past the
  bank's end goes on to the next bank. Confirm or a tap opens a slot (`library.load`); a slot
  chosen while one opens waits and opens next, the last chosen winning, as the JUCE engine client
  queues library loads; a load refused as busy (`-32009`, another client's long request) is tried
  again as the next; the next slot is prepared (`performance.prepare`).
- **A slot's color** (its own, else its Performance's) is its whole box, in the grid and in the list,
  whatever the theme's `rows` says; its number, name and note are white on it, black only on a color
  too light for white (`theme::white_first`: white while it is 3 to 1, the large-text threshold). A slot without a color is the theme's
  surface as before, an empty slot the background with nothing in it, a placeholder its name with
  "(placeholder)" under it. The slot playing has its name bold and, without a color, the accent edge
  and the raised fill; on a colored box, a bar of the box's ink set inside its left edge and its
  number on a chip of that ink. The cursor is the focus border, with a hairline of the box's ink
  inside it on a colored box, so neither mark depends on the color under it.
- **The text is a stage screen's**, read from playing distance. A tile's name is the theme's
  `display` size (32) on up to two lines, broken at a word and then cut, under the band the number
  has at the tile's top; a name that does not fit the lines the tile has steps down to 0.8 and then
  0.65 of it (a 4 x 4 grid at the keyboard's scale has one line). A list row's name is one line of
  the `headline` size (26) in a row as high as before: a view holds 9 rows at 1280 x 720 and 5 at the
  keyboard's 1.55 (a bank longer than a view, with its line under the rows). The number is the body
  size, bold; the note (why a slot does not open) the small size, under the name. The bar's dropdowns
  and buttons are the title size.
- **Without touch**, one cursor: pane 0 (E1 on a FANTOM-07) moves it along the setlist bar's four
  stops, pane 1 (E6) over the slots, and each drops the other's. The VALUE dial and Up and Down walk
  through both: up out of the first bank's first slot is the bar's last stop, down past it the slots
  again, and up out of the bar's first stop is the top bar. A stop that is off can hold the cursor;
  a press on it does nothing. **DEC and INC** turn the bank, as Previous and Next do (nothing opens,
  no wrap): the panel map gives them `step`, its own total in `controllers.navigation`; with a menu
  or an overlay over the Setlist, and on every other screen, a step is the `move` it was before.
- **The previous and next song** are not on the screen any more but work as before: Left, Right, Page
  Up and Page Down here, as in the JUCE Setlist (page turners send them), and Play's Previous and
  Next and its page keys (`step_by`, as the JUCE Play asks the Setlist app to). The bank shown follows
  the song stepped to.
- **The slot playing follows the engine's open Performance**, whoever opened it (the Library, Home, a
  program change, a panel action): each `performance.info` the shell asks (at a connection and after
  each `graph.changed`, so nothing more at rest) gives its library `id`, and every slot whose `id` is
  that one is marked playing (a song twice in the setlist is marked twice); none is when it is not in
  the setlist or the Performance is not from the library. Unsaved edits keep the `id`, so the mark
  stays. The slot stepped from (`current`, the one kept in the state file) is the slot chosen here
  while it holds that Performance, else the first slot that does, else none (the next song is then
  the first). While a slot opens here the report of the Performance before it is not followed. The
  screen opens on the bank of that slot when there is one, else on the bank last shown.

- **Manage** (`setlist/manage.rs`; setlist management was the Library's Setlists tab, moved here by issue #41) opens the shared
  menu of three submenus: **Setlist** (New setlist..., Rename..., Tags..., Color, Duplicate, Delete...), **Bank** (New bank,
  Rename..., Move earlier, Move later, Delete...) and **Slots** (Add a Performance..., Add an empty slot, Move up, Move down,
  Remove). Names and tags are asked on the keyboard, a delete of a setlist or of a bank that holds slots is asked first. Slot
  entries act on the slot under the cursor, else the last one the cursor or a tap was on; Add puts the Performance (chosen by name
  from the library's) last in the bank shown. An edit changes the document `setlists.get` gave (so nothing the screen does not
  show is lost), saves it whole with `setlists.save`, then asks for the setlists and the setlist again and shows it with the cursor
  where the edit left it; a new setlist is a save of a document with no id and one empty bank, a duplicate is `library.copy`, a
  delete `library.delete`; an engine refusal is said in the top bar. Adding a Performance to a setlist from the Library
  (an action on a Performance) stays in the Library. Not here: exporting one setlist, a setlist's description, moving a slot to
  another bank, the stepping position (`current`) is not re-mapped when slots or banks move.

The setlist open and the slot playing are kept in the JUCE Setlist app's own state file (the data
folder's `AppData/oscairn.setlist/state.json`), so either opens where the other left off.

## Play

`play.rs` and `slint/play.slint`: the JUCE Performance app's first view (PlayView.cpp) for the stage.
Home's Performance tile and the Setlist's Play (its top bar action) open it (Cancel goes back to where it was
opened from); Edit, the last action of the top bar, opens the Parts list (below).

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
  a second), and that stops after two seconds of answers with no signal in them (the peaks hold and fall, so a tail is
  seen out) until something wakes it: a status whose keys held or inputs changed, `graph.changed`, `controls.changed`,
  `controllers`, a request the shell sends that may play (`notes.*`, a scene, a level, a value, undo), or a `transport`
  that plays. The shell at rest wakes for nothing and asks nothing (measured against the real engine,
  ui/REAL-ENGINE.md). A note shorter than a status interval (a quarter of a second) that is in no `status` is not
  metered until the next wake; an engine push of the levels would remove that. No new engine request.
- **In Play now** (ui/PERFORMANCE-APP.md): the "+ Scene" tile (`scenes.add`) and, from the top bar's
  Scene action, the Scene screen (`scene.rs`: store, rename, the scope's six kinds, glide, curve and
  launch); the master as the last stop of the right pane, a press then the dial moving its fader; the
  "No MIDI device" line with its two links (the keyboard, Dismiss, kept in the JUCE app's
  `keyboard.txt`), shown where the profile offers the on-screen keyboard; a take recording, in the
  top bar only (the JUCE Launcher draws it once over every app).
- **Layout** (`slint/play.slint`, reflowing for any window and theme): as PlayView.cpp divides a dense
  screen. The scenes take a touch target's height (more where the theme's type is large) and eight tiles across,
  each name across the tile under its number, shrunk to 70% before it is cut (`FitText`); the knob row takes a
  quarter of the height (the grid's knob tile at most, 70% of it at least) with its tiles side by side and the
  buttons in a column at its right (Performance and the part stacked, the bank button beside them), not a row
  of their own; a knob that plays nothing shows a dash. Under the knob row come the no-MIDI line and Learn's
  line when they apply, then the strips, and a line under them only when it has something to say (the engine's
  refusal, or "Parts 1-6 of 14" when some parts are out of view). A strip's name and sound shrink to 70% before
  they are cut; its marks are a touch target tall, three quarters of that on a short strip. At the FANTOM-07's
  826x465 logical pixels (1280x720 at 1.55) the scenes are 98 wide, the knob row 104 and a strip's fader travels
  131 (before: 38; the tests: at least 72 in every theme at 1280x720 at 1 and 1.55 and 800x480, 36 with one of the
  lines shown; with two at once, only what is left). `PlayState.layout` publishes the areas for the tests.
- **The knob row** (`row.rs`, `knobs.rs`, drawn by the grid's tiles: `grid::row_layout`, `TileRow`): a
  tile for each knob of the profile's screen group (else the first assign group, else a computer's
  four), showing what the engine plays on it. The engine has no request that returns that, so the
  shell resolves it as `core/control/src/physical_controls.cpp` does (`knobs.rs`: the banks, roles from
  names, overrides) from `graph.get`, `params.list` (only the plugins of the part whose own controls
  the knobs follow) and, for a group that is not a screen group, `assignments.get`; the part and the
  group's page are published with `controllers.view` (`part`, `positions`), as the JUCE `Engine::
  setKnobPart` and `setControlPosition` do, and the screen page stays empty (with none a device's
  screen knobs play Play's row). Because the knobs then play, `lists` is false while Play (and a
  parameter page) is shown: the panel's navigate controls (E1/E6) play knobs there, and the dial and
  the cursor keys move the cursor (scenes, the knob row, parts). A part opened from Play is the one
  selected: the Performance/part buttons and the bank button are stops of the row. Learn is the top
  bar's action where the profile has an assign group of knobs (the JUCE condition, a panel with `row:N`
  knobs, holds on no profile): `controllers` says which control moved, a part's level (or its M, a
  second stop on each strip while Learn waits), a scene or a knob is the target (`assignments.set`).
- **Nothing is handed over to the JUCE app from here any more:** Edit is a top bar action and opens the
  Parts list in this UI (the footer's "Edit, knob banks, Learn: Oscairn app" button is gone). Not ported
  either: a part's pan and sends have no control in Play (the JUCE Play has none; the Part screen has
  the pan), and meters past two channels show two bars.
- **Not measured:** the 100 ms meter poll's cost on a unit, and its look at the FANTOM-07's scale.

## Parts list

`parts.rs` and `slint/parts.slint`: page 0 of the JUCE Edit view (PartsView.cpp, PartsModel.cpp's
`readParts`), opened by Edit on Play's top bar (the bar's last action on the list is Play, the Edit /
Play toggle). Not handed over: the whole screen is Rust.

- **Shows:** a row per part, then per bus ("Shared buses" above the first), then each node outside
  the parts that several parts are wired into in Routing: its name and sound (the notes the shared
  budget gave up, `· 3 stolen, 1 cut`, from `parts.meters`), where it plays from (`No device:` before it
  when none of its devices is connected, from `session.status`) with its key range drawn as a bar, its
  chain as one box per plugin (colored by role, faint when off, outlined in the danger color when
  `session.status`'s `failures` name it; a split is one box named after its lanes), a level slider, mute
  (M), and a send slider per bus on parts. A bus says who feeds it, a shared row "Fed by ... in Routing".
  The line under the list says the first failed plugin ("X is silent: why") unless an edit was refused.
  Under the rows: Mixer, Controls, Routing (disabled until those screens exist: pressing says so),
  + Part and + Bus, named "Part N" and "Bus A" as the JUCE view names them (`parts.add`, `graph.commit`).
  The scenes row of the JUCE page is Play's (A1 to A4). A part has no remove here: the JUCE page has it
  on the part's own page, as the Part screen does.
- **Layout:** columns by the room: the controls (level, mute, sends) take up to 60% of the width and
  their sliders shrink to it; with 520 px (times the type scale) left for the part's words they get
  columns (name, plays from, chain), else one column with the chain and input on the second line. The
  rows a view holds fill the height: the screen reads the list's height and a row's pitch from its
  component, so the window, the theme's spacing and the type scale decide (`PartsState.resized`);
  a view is cut from the first row, the cursor's view shown.
- **Without touch:** the cursor has two dimensions as the grid's: the dial goes cell by cell (the
  part, its level, mute, each send), row by row, then through the buttons; the left knob moves along
  the row, the right knob down the rows; arrows likewise; Page Up and Down turn a view, Home and End
  the first and the last. A press on the part opens its Part screen, on a level or a send starts a
  change the dial then makes (a fortieth of the slider a click), a press or Cancel ends it, on mute
  toggles it, on a button does it; Cancel drops the cursor, then goes back to Play. Touch: a tap on the
  words opens the part, sliders drag, M and the buttons tap.
- **Requests:** `graph.get`, `session.status` (on opening, and its notifications), `parts.meters` every
  200 ms while shown (the JUCE view's timer), `controls.changed` while shown; edits `parts.level`
  (level, mute) and `parts.send` (no level removes it), each then `graph.commit`, and `parts.add`. None
  new. A regular expression in a device rule cannot be tried here: such a part is taken as connected.

## Mixer

`mixer.rs` and `slint/mixer.slint`: the mixer page of the JUCE Edit view (PartsView.cpp, `mixerShown`),
opened by the Mixer button under the Parts list; Back, Cancel and the bar's Play leave it. The JUCE
page is strips, so the Mixer is strips: a strip per part, then per bus (a wider gap between), the
master pinned at the right. A strip: name, fader beside a meter bar per channel of the sound, the level
in dB, M and S, pan, a send slider per bus (parts only; every strip keeps the rows so faders line up).
Faders use the JUCE skew (a part's 0 to 1 with -12 dB halfway, the master's 0 to 2); a send taken to
nothing is removed. How many strips a view holds and how many sends a strip shows is what fills the
window (`MixerState.strips-per-view`, `sends-per-view`, set by the component), so the window, spacing
and type scale decide; the header has paging buttons for touch when something is out of view.

- **Without touch:** the cursor is one control of one strip. The dial goes strip by strip, control by
  control (fader, M, S, pan, sends); the left knob along the strips, the right knob up and down the
  strip; Page Up and Down turn a view of strips. A press on a fader, pan or send starts a change the dial
  then makes (a fortieth of the travel a click, held at the ends; a press or Cancel ends it), on M or S
  toggles. Touch: faders and sliders drag, a double tap centers a pan or removes a send.
- **Requests:** `graph.get`, `parts.meters` every 50 ms while shown (the JUCE timer), `controls.changed`
  (levels, mutes, pans, the master); edits `parts.level` (level, mute), `parts.pan`, `parts.solo`,
  `parts.send`, `master.set`, each then `graph.commit`. None new.
- The fader, marks and meter are Play's strip widgets drawn again (`Meter` moved to theme.slint for it;
  Play's own copy in play.slint still stands until that file is next edited).

## Controls

`controls.rs`, `slint/controls.slint`, `menu.rs`, `slint/menu.slint`: the controls page of the JUCE Edit view
(PartsView.cpp, `controlsShown`), opened by the Controls button under the Parts list. A control is a
knob (0 to 1) that drives targets: a plugin's parameter, a part's level, mute or send, another control,
or a morph between two scenes, each with a range (start and end sliders, the end below the start reverses
it), a curve (linear, exponential, logarithmic, S-curve, fader dB, power), Relative and the scenes it works
in. A row per control (name, fader, + Target, Remove), a row under it per target (what it drives, the two
sliders, curve, Relative, Now..., Scenes..., Remove: in one line, or two or three when the width is short).
By scene swaps the rows for a table of every mapping against every scene.

- **Menus** (menu.rs, shared with Routing): the JUCE popup menus are lists over the screen: a row is a
  choice (ticked or not), a submenu or a heading; the cursor starts on the first row that can be chosen and
  nothing is chosen until a press; Cancel goes up a level and closes at the first; how many rows show is what
  fills the height. The screen that opened a menu gets the choice (`chosen`).
- **Without touch:** the cursor is two-dimensional as the Parts list's (dial cell by cell, left knob along a
  row, right knob down the rows); a press on a name asks the text keyboard for the new one, on a fader or a
  slider starts a change the dial makes (a fortieth a click), on a button or Relative does it, on the curve,
  Now..., Scenes... or + Target opens a menu. Touch: sliders drag, the rest tap.
- **Requests** (none new): `controls.set` (live), `controls.add`, `controls.name`, `controls.remove`,
  `controls.targets` (the control's whole list again, as `setTargets` sends it), `params.list` (names and the
  + Target menu), `params.get` (Now...). The JUCE page sends no `graph.commit` after these, and neither does
  this. Now... is a port of `setRangeToPresent` (core/control/src/parts.cpp).

## Routing

`wires.rs`, `routing.rs`, `slint/routing.slint`: the routing page of the JUCE Edit view (PartsView.cpp,
`routingShown`, and PartsModel.cpp's `readRouting`, `inputsOf`, `layoutRouting`), opened by the Routing button under
the Parts list, a shared row of it, or "Routing..." among a plugin's actions in the chain. The real graph as boxes in
columns by signal flow (the keys and MIDI and each audio input first, the outputs last), its wires between them, and
a panel under it for what is chosen.

- **Drawing:** `wires.rs` is pure (graph.get's JSON to pixels from (0, 0)), a port of the JUCE layout: columns by rank, a
  slot in each column a wire crosses, eight sweeps ordering each column to cross fewer wires, each wire its own place
  on a box's edge, an expanded part a frame whose plugins are laid out the same way inside it, a wire back to an
  earlier column going below everything, control lines (a plugin's lane to a control or a parameter, the control to
  the plugins it drives) routed in right angles around the boxes by a shortest-path search. The JUCE view fits it to
  the canvas with no floor (scale at most 1: at the keyboard's 826x465 its text is a few pixels); here the drawing keeps
  a legible size, the theme's type scale (at least 1), so no text of a box is under the theme's small size, and what
  does not fit the canvas scrolls: a drag (a `Flickable`), or the cursor reaching a place out of view (the dial, the
  left knob and the arrows move it over the places and the view follows to hold the place whole). The header's
  buttons share the line of Back and the title when they all fit, and the panel takes the rows it has (a view of at
  most two fifths of the height, two rows at least), the drawing the rest. The component gets one set of points:
  wires are `Path`s with `fit: preserve` in the pixels of the drawing, boxes sit at the same points, so a wire meets
  its port wherever the window is and wherever it is scrolled (`src/tests/routing.rs` measures every end in rendered
  pixels at five sizes and scales, scrolling the drawing in steps; without `fit: preserve` it fails). The theme's wire properties are used as the JUCE view uses them: `audio` and `notes` for enabled wires,
  `disabled` and `draining` (dashed), `unconnected` for a wire that no longer fits its ports, `control` for the control
  lines; a wire a user drew is thicker; a bus is a line per channel, eight at most; with Meters on a wire that carries
  sound (above -60 dB) takes the level's color: positive, `warning` from -12 dB, `danger` from -3.
- **Panel:** for a plugin: Connect audio to..., Connect notes to..., Keyboard (the keys and MIDI, where the profile
  offers the on-screen keyboard), Connect lane to..., Turn off or on, Open part or Remove, a disabled "Plugin view" when
  it has a window of its own; a row per lane that reaches a control or a parameter with Disconnect; each input with
  sources and a row per source: On, level, M, S, Disconnect (wires drawn by hand), and the channel of notes from the
  keys. For a part: Open part, Expand or Collapse. Cells wrap onto more rows by the width and the rows are paged.
- **Without touch:** the left knob moves the cursor over the boxes (frames, parts, plugins and ends, then the control
  pills, in columns from the left; the arrows cross columns), the right knob over the panel's cells, the dial over the
  buttons, the boxes and the panel in one list; a press chooses (again lets go) or does what a cell says, a slider or
  the channel is changed by the dial after a press; Cancel ends a change, drops the cursor, then goes back.
- **Requests:** `graph.get`, `session.status` (a plugin that failed is outlined), `wires.meters` every 50 ms while
  Meters is on, `params.list`, `window.info`, and `graph.connect`, `wires.disconnect`, `wires.mix`, `wires.enable`,
  `wires.zones`, `graph.enable`, `graph.add`, `graph.remove`, `graph.unmap`, `controls.add`, `controls.source`,
  `controls.targets`, `controls.set`, each edit followed by `graph.commit`. None new. The choice of "Parts collapsed" is kept
  in `routing-view.txt` beside the theme choice, as the JUCE app keeps it.

## Part

`part.rs` and `slint/part.slint`: the JUCE Part page (PartsView.cpp, PartsModel.cpp's `partFields`)
for one part, opened from Play by selecting it (a press on it, a tap on its name) or from the Parts
list. Back returns to the one it was opened from; Play's Cancel goes to where Play was opened from.

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
- **Key** (the JUCE app has no control for it; RFC 0033, `harmony.get` and `harmony.set`, asked on
  opening): the part follows the Performance's key (shown named, "Performance's (D Minor)") or has a
  root of its own, the dial stepping from "Performance's" through C to B; with a key of its own a
  Scale row steps through the scales' names (a scale with no name shows as "Custom (0 2 7)"). Each step
  is `harmony.set` with the part, then `graph.commit`.
- **Still handed over:** nothing in this screen: the handover row is gone. Not yet built here (the
  checklist is ui/PERFORMANCE-APP.md): splits and lanes, scenes editing, the knob row and Learn,
  Controls, Mixer and Routing (the Parts list has their buttons, disabled, saying so).

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
  followed by `graph.commit`. A plugin's role word and color class are the reference's `roleOf`, it is faint when
  off and outlined (and named in the message line) when `session.status` lists it as failed. An insert row of the
  part's own chain lists the four kinds of split first (copy: `parts.addSplit`; left / right, mid / side, bands:
  `graph.add` of the Split plugin, and of the Join plugin for the first two, then `parts.addSplit` with their nodes,
  taken out again if it is refused). A split is a row opening its lanes, a band split's crossovers (`params.get`,
  `params.set`, a sixth of an octave a step), Add lane or Add band (`parts.addLane`) and Remove split
  (`parts.removeSplit`, then `graph.remove` of the other lanes' nodes); a lane opens its own plugins (inserted,
  moved and removed with `split` and `lane` in the request) and its settings (lowest and highest key, level, pan,
  mute, solo: `parts.lane` with the whole lane, as `editLane`) and Remove lane (`parts.removeLane`, then
  `graph.remove` of its nodes). Like the reference, none of the removals asks first; Undo (`edit.undo`) takes them back.
  A plugin's actions also have Presets...: `presets.list` for the node, and a press on one is `graph.replace`
  with the plugin's own `path`, `pluginId` and the preset's `uri` (no commit follows: the request commits).
  Their tests are `src/tests/chain.rs`, `chain_splits.rs` and `chain_presets.rs`.
- **The plugin scanner** (`scan.rs`): as the JUCE app finds plugins, the helper `oscairn-plugin-scan`
  beside the program run once per plugin file (`--worker FILE`, 5 s, 4 MiB) over `--plugins DIR`
  (else `plugins` beside the program, else `../lib/clap`), CLAP then LV2, started the first time the
  chain is opened, its wire format read with every length checked. It is a helper process, not an
  engine call: no control request. A timed-out helper is killed with its process group, as the C++ one does.
- **Parameter pages** (`params.rs` on the grid, `grid.rs`, `slint/grid.slint`; ui/GRID.md): host-drawn
  from the plugin's own layout data in `params.list` (widget, priority, group, range, views), as the
  JUCE ParameterGrid: a page per top-level module (the first level of a two-level family of tabs, ui/TABS.md), tabs on a page of more than 24, headings, tiles in
  priority order in two to six columns for the width given, linked runs on one card, an envelope's
  curve above its stages; knobs, sliders, switches, dropdowns, split buttons, ranges and values, each
  with the plugin's own text (`params.text`). `params.get`, `params.set` inside `params.gesture`;
  `params.changed` while shown; the page shown is published with `controllers.view` `screen` for the
  device's screen knobs and withdrawn on leaving. Without touch: dial in reading order over the search, the
  pages, the tabs and the tiles (a page or tab opens on a press; the tiles stop at their end), left knob columns, right knob rows, press selects, dial changes, Cancel backs out.
  The views of the reference are drawn above the tiles (`views.rs`: envelope, key and velocity map, FM
  operators, LFO, filter; the node or handle a tile sets is marked under the cursor) and dragged by touch
  where the reference drags them; a plugin of more than 24 parameters has the search field above the page
  list (the on-screen keyboard for its text; back from the first tile reaches it). An FM operator view is the
  top pane of a master and detail (an overview of every operator, with a table of them (mute, solo, level, coarse,
  fine, ratio or frequency, detune: each cell edited in place), over the selected one's envelope as named stages
  in a Time row and a Level row under a larger graph of that operator (the others dim behind it, where the height allows
  it: at eight operators on the keyboard's screen it is a few lines high) and its other parameters as compact controls
  (a knob under 28 px is a value cell with a thin bar, the whole tile the touch target; the operator's number is left off
  their names under the heading "Operator N"); the overview's graph has a third of the width at least and the diagram's
  share before the table's columns give up theirs; a handle that moves two parameters is one `params.set`, so one undo
  step; a page higher than its view shows a mark at its right edge; a page whose tabs were the
  operators' indexes is folded into it, no tab per operator: ui/GRID.md "Master and detail", "The operator table",
  "The detail", ui/TABS.md "Tabs that are indexes"). The grid framework and the views are in ui/GRID.md.
- **Sound chooser** (`sounds.rs`): `library.query` of kind `sound` (a search typed on the keyboard
  narrows it), and for the one pressed `parts.loadSound`, `parts.fromSound`, `audition.start` and
  `audition.stop`, and `parts.saveSound` (over its Sound, or new with a name typed). The engine's refusals
  (staged edits, the library busy) are said. As the library pane narrows the list: a Filters row opens
  Favorites and Recent (switches), Library (everything, my own, or an installed library from
  `library.libraries`) and Tags (those of the matches with their counts, the chosen first, all of the chosen
  must match), each change asking `library.query` again with `favorites`, `recent`, `source`, `tags`; a Sound's
  row has its tags and description, "Locked", "Does not fit this device", "Auditioning" and a star, its color as
  the theme puts a row's, and its actions add it to the favorites or take it out (`library.favorite`).
  Tests: `src/tests/sounds_filters.rs`.

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

## Library

The Library app (`ui/src/library/`, `ui/slint/library.slint`, `ui/LIBRARY-APP.md` is the checklist, 45 rows) is ported
whole and **Home's tile opens it** (`library_glue.rs` `PORTED`). **To fall back to the JUCE Library app from the device,
set `OSCAIRN_UI_LIBRARY=0`** (any other non-empty value opens the Rust one whatever `PORTED` says). All 45 rows are built (43 ticked, 2 n/a). The first cut:

- **The screen** is the shared list screen (`browser.rs`, the Part screen's component) under one row below the top bar: the
  kinds (Performances, Sounds, Setlists) as the shared tab family in a strip, the search box (a tap or a press asks for the text on the
  keyboard; the text shows in the box; its cross takes it away; the engine's own match narrows the list and the line under it counts
  the matches) and, at the right, the button that shows the panel. Under the row, the **sidebar** at the left (`slint/library.slint`,
  `src/library/side.rs`: Favorites and Recent, the libraries (All, My own, each installed library with its count), the tags with their
  counts; one line each, a touch target tall, the chosen ticked, several tags all of which must match; its own scroll bar; its width
  `Theme.sidebar-share`, 29% of the content) and the items at the right: name ("Untitled" without one), tags, "Locked",
  "Auditioning", at the trailing end a warning icon (the engine's `unfit`: the Performance does not fit this device; the reason is the
  panel's, the line under the list's and the row's accessible label; the list answer has no finer level and no missing plugins) and
  then the star, the item's color as the theme puts a row's (a mark at the leading edge by default). A choice in the sidebar applies at once (the engine is asked
  again); only opening a Performance waits for a press, as before. The **panel** (what the item under the cursor is) is hidden at first;
  its button shows it in the sidebar's place and hides it again, for the session. What only the panel said (why a Performance does not fit
  this device, which library it is locked in, a folder indexed in place) is in the line under the list while the cursor is on the row and
  the panel is hidden. Audition and Options are the top bar's actions (Options with the cursor in the sidebar on a tag is the tag's
  menu: color, hide). Three more buttons sit in that row, left of the panel's: **Bubbles** (the sidebar's entries as pills that wrap along a
  line, as the JUCE pane's pills view; arrows go to the neighbor where they point) and **Compact** (the spacing scale's 0.7 step on the gaps of
  rows and entries and a footer line of the type's height; every row stays a touch target tall), both kept in `library-view.txt` as the JUCE
  pane keeps them; a sidebar section's heading folds it (a press or a touch; the session's). Without touch: the dial walks the row under the top bar (the tabs, the search box, its cross, the three buttons)
  and then the list; left and right (the left knob and the right knob) go between the sidebar and the list; up from the first row of
  either reaches the row; one cursor in one place at a time. The sound chooser (Part screen) keeps its Filters row: it is a picker over
  the Part screen with no room for a column.
- **A row shares its width by priority** (`part.slint` `Row`, a `fluid` row): the name first (it keeps what it needs, at least
  70% of the type, before the tags do), then the tags and states (their natural width, at least a quarter of the row), then
  the description (what is left, muted, only if that is six small-type heights or more); the name is cut only when the row
  has no room (a test: the 42-character name with two tags is drawn whole at 1280x720). Sounds rows are the same rows.
- **The scroll bar** (`part.slint` `Part`, every list of the shared list screen that is longer than its view; `Theme.scroll-bar`,
  `scroll-lane`, `scroll-thumb-min`): a thin bar in its own lane at the list's right edge (the rows are narrower by the
  lane), always shown, its thumb the view's share of the list (at least a touch target long) at the view's place. A touch
  only: a tap on the track or a drag on it asks for the view at that place (`PartState.scrolled`, `Browser::scroll_to`,
  a whole view at a time; its lane is a touch target wide, `Theme.scroll-lane`, the visible bar a third of that), drops the cursor
  (scrolling selects nothing), keeps the footer in step, and asks the engine
  for nothing (the list is held whole); it is not a cursor stop.
- **Requests:** `library.query` (`kind`, `limit` 1000, `text`, `tags`, `favorites`, `recent`, `source`), `library.libraries`,
  `packages.installed` and `packages.preinstalled` on opening (the preinstall line is asked again each second only
  while it runs); `library.favorite`, `library.tag`, `library.edit`, `library.copy`, `library.delete`, `parts.fromSound`,
  `audition.start`, `audition.stop`, `audition.get`; `setlists.get` and `setlists.save` for Add to setlist.
- **Opening a Performance** is the JUCE job's chain, `performance.info` (a question about unsaved changes, `notice.rs`),
  `budget.estimate`, `performance.prepare` when it fits beside the open one, `library.load`. The engine answers every
  other request as busy (-32009) while one of them runs, so each step is asked again after 100 ms; the line under the list
  says "Opening X... N s", the row says "Opening", the top bar shows "Loading X" (see "Waits" below), and the screen keeps
  answering the dial. A Performance chosen meanwhile waits and replaces any waiting.
- **A question over the screen** (`notice.rs`, `slint/notice.slint`): the JUCE app's alert window as a title, a wrapping
  message and buttons down a panel; the border cursor starts nowhere, a press chooses, Cancel closes with no choice.
- **Home's Library line** is the reference's ("5 instruments and effects · 24 Performances · 3 Sounds"): the shell asks
  `packages.installed` on connecting.
- Tests: `src/tests/library*.rs` against `library_fake.rs`, a fake engine that answers as the real handlers do
  (`ui/LIBRARY-APP.md` "Verified shapes").

The second cut: a file chooser over the local file system (`files.rs`: Up, folders, files with sizes, Save here, Choose
this folder); installing an archive (`install.rs`: signer, own or locked, which items, the plugin builds, the warning for
code from an unknown source and the refusal for a revoked key, Install anyway); exporting an item and the library
(`export.rs`: edited parts' Sounds, signature or a new key, written beside and renamed over an old file); the Libraries
menu (`libraries.rs`: folders indexed in place, a hidden tag shown again, plugins in the store with builds and removal,
a library uninstalled); the Catalog (`catalog.rs`, `catalog_flow.rs`: repositories read with `packages.list` jobs polled
by `packages.job`, tabs, a package's details, install and update, trust questions); importing parts and scenes
(`import.rs`). Long requests go through `jobs.rs`, each asked again while the engine is busy.

## Settings

The Settings app (`ui/src/settings_app.rs` and `settings_*.rs`, `ui/slint/settings.slint`; `ui/SETTINGS-APP.md` is the
checklist, 54 rows and 8 that are not in the app) is ported whole and **Home's tile opens it**
(`shell/settings_glue.rs` `PORTED`); **`OSCAIRN_UI_SETTINGS=0` hands the tile back to the JUCE Settings app** (any other
non-empty value opens the Rust one). Appearance writes `appearance.txt` and `themes/` in the configuration folder the JUCE
app reads (Theme.cpp's root: `OSCAIRN_CONFIG_HOME`, else the platform's place), so a theme chosen in either shows in the
other within a second. It is the shared list
screen under the shared tab family (a strip or a rail by the theme's `tabPlacement`), as the Library is, one page a tab, each
page a `Page` that makes its rows and the panel's lines and takes a press, a menu's choice, a question's answer and the
keyboard's text.

- **Pages and where they show:** Appearance, Top bar, MIDI controllers, Package repositories, Signing keys and About on every
  profile; **Audio** only where the profile does not fix the audio device (`audio.device` absent: a computer, and the devices
  that leave the choice to the engine), so not on the keyboard, where the engine says `fixed`. A page for hardware a product
  lacks is not there at all (the reference's Bluetooth, network, storage and date pages have no request and are not built).
- **Kept outside the engine, in the configuration folder the JUCE app reads** (`OSCAIRN_CONFIG_HOME`, else
  `~/Library/Application Support/Oscairn` or `~/.config/Oscairn`): `appearance.txt` (the theme chosen, replaced whole through a
  temporary file, applied to every screen at once), `themes/<id>/` (an installed theme: Install... on the Appearance page copies
  one in as `Theme::install` does) and `top-bar.json` (what the CPU and RAM meters show, shown at once). Everything else is the
  engine's, by `audio.devices`/`audio.set`, `controllers.inputs`/`choose`/`ignore`, `packages.repositories`, `enableRepository`,
  `removeRepository`, `addRepository`, `packages.list` and `packages.job` (a repository is read first, a root key trusted only
  after the question with its fingerprint), `keys.list`/`keys.make` and `packages.preinstalled` (About's engine version).
- **A page lists again only when the engine says its list changed** (the status's `audioDeviceList`, `audioDevice`,
  `sampleRate`, `bufferSize`, `midiPortList`), never on a timer; a request refused as busy is asked again after 300 ms.
- **No question before a change, as in the reference:** the engine opens the last audio device again if a new one does not open,
  a fixed device cannot be changed, a controller choice is reversible on its row; a repository is removed only after a question
  and a new one is trusted only by its fingerprint.
- Tests: `src/tests/settings*.rs` against `settings_fake.rs`, a fake engine that answers as the real handlers do
  (`ui/SETTINGS-APP.md` "Verified shapes"), and `real_settings` in `src/tests/real_engine.rs`, ignored unless an engine is given
  (`OSCAIRN_REAL_SOCKET`, `OSCAIRN_REAL_PROFILE`).

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
  `--handover` file a JUCE screen's tile says where that screen is instead. The desktop app
  starts it itself (On a computer, below).
- `cargo test` (or ctest's `ui-rust`): every test renders headless through Slint's software
  renderer into a buffer (no window opens), with data and configuration folders of their own;
  `OSCAIRN_ENGINE=…/oscairn-engine cargo test` adds the test against the real engine, started with
  `--audio null`. `cargo clippy --all-targets` is clean.

## The three apps and Home's tiles

The Performance app (Play, Part, the chain, the sound chooser, the parameter pages, Scene, Parts, Mixer, Controls, Routing),
the Library app and the Settings app are in this shell, as is the Setlist: nothing in them hands the display over (the walk
`tests/handover.rs` presses every stop and action of the three, from Home's tiles, with a handover file as on the device). The
Recordings tile (where a profile offers it) still hands over to the JUCE app. Each ported tile can be given back
to its JUCE app by an environment variable of the UI process, for a fault found on a unit: `OSCAIRN_UI_LIBRARY=0` and
`OSCAIRN_UI_SETTINGS=0` (a value other than `0` forces the Rust app on). A rail label of one word too wide for the rail at 70% of
the type goes across two lines before it is cut; a list row keeps the theme's padding between its label and its value; the
Catalog's rows are columns (name, version, publisher, state and summary) each shrunk and cut on its own.

## On a computer

Since 2026-11-25 the desktop app's interface is the Rust UI ("make rust the default now"). The app
(`Oscairn`, hosts/standalone/Main.cpp) starts its engine as before, then `oscairn-ui` from beside
itself (`Contents/MacOS/oscairn-ui` in the macOS bundle, `bin/oscairn-ui` installed on Linux; the
build copies it there with `-DOSCAIRN_UI=ON`) with `--socket` the engine's and a `--handover`
file, in a window of its own, and hides its JUCE window (`Application::startRustUi`). No profile
is given (`OSCAIRN_PROFILE=` is cleared), so no screen scale is forced: Slint's winit backend takes
the window's own scale factor from the OS. A screen not yet ported ends the Rust UI with status 3
and its name, as on a device: the JUCE window comes out on that screen, and its Home hides it and
starts the Rust UI again. Its window closed (status 0) quits the app, asking about unsaved changes
over the JUCE window; any other end shows the JUCE window, as before.

The JUCE app stays reachable: `OSCAIRN_UI=juce` in the app's environment starts it as before (the
JUCE window, no Rust UI); so does a build without `oscairn-ui` beside the app, `--in-process`,
`--app`, `--view` and `--snapshot`, and a profile with a panel screen (a device). On macOS the
app's menu bar (with Oscairn in front) brings the JUCE window out beside the Rust one; its Home
hides it again.

### Desktop: still to do

The window structure first (the owner, 2026-11-25: "make sure the things that are missing are on
the todo list, including window structure"); none of this is built:

1. **Separate windows where the JUCE app opens them**, each resizable and responsive, in place of
   one Rust window and the JUCE one in turn: the Library app (`showLibraryApp`, an `AppWindow` of
   900 by 700, Main.cpp); Plugins and appearance (`LibraryWindow`, Main.cpp, the Plugins menu);
   Recordings (`showRecordings`, 760 by 600); the
   Library's picker (`showPicker`, 760 by 620, for Import Performance and Load Sound); Settings
   and the Setlist (app processes in windows of their own, AppProcesses.cpp, apps/settings and
   apps/setlist); the Performance window's tabs detached into windows (`DesktopShell::detach`:
   Part, Mixer, Routing, Controls); the on-screen keyboards (KeyboardWindow.cpp, `Keyboards`).
2. **Plugin editor windows**: a plugin's own GUI (`Engine::openPluginWindow`, the control's
   `showPluginWindow`, opened from the JUCE Parts view, PartsView.cpp). The Rust UI cannot open
   one yet; it must, for plugins that draw their own editors.
3. **One app, not two processes**: the Rust UI runs as a child with its own Dock icon and menu;
   the JUCE process holds the engine, the MIDI ports (Controllers.cpp) and the menu bar.
4. **Native scaling verified** on a Retina and a non-Retina display, and a window moved between
   them (main.rs sets `SLINT_SCALE_FACTOR` only from a profile's screen); not yet checked.
5. **Resizable and responsive**: every Rust screen at small and large window sizes, and its least
   size (the JUCE main window's limits are 900 by 620 to 1800 by 1200).
6. **Menus**: the JUCE menu bar (`getMenuBarNames`, Main.cpp): Performance (Save, Save As New,
   Import, Export, Install Library, Color, Save to Folder, Open Folder), Edit (Undo, Redo), View
   (layouts), Plugins, Appearance (themes, Manage themes), Apps, Part (Load Sound, Update from
   Library, Save Sound, Save Sound As New). The Rust UI has none; on Linux the JUCE menu is inside
   its hidden window, so only the handover reaches it.
7. **File dialogs** for import and export: `juce::FileChooser` in Main.cpp (Save to Folder, Open
   Folder, Export), LibraryPane.cpp, PluginBrowser.cpp, Screen.cpp and Setlist.cpp.
8. **Drag and drop**: neither UI takes dropped files yet (no `FileDragAndDropTarget` in the JUCE
   app); decide what a drop does (a Performance, a Sound, a library, a sample).
9. **Keyboard shortcuts**: Cmd-Z and Shift-Cmd-Z (`Window::keyPressed`, Main.cpp) and the menus'
   keys; the Rust UI has Escape for Back and the playing keyboard's keys.
10. **The Mac's audio and MIDI device choice**: the Rust Settings app's Audio and MIDI controllers
    pages (settings_audio.rs, settings_controllers.rs; AudioPage.cpp the reference) are not yet
    tried on a computer with the Rust UI as the interface.
11. **Multiple monitors**: the JUCE app places its windows by the displays (DesktopProfile.cpp,
    KeyboardWindow.cpp, DeviceScreen.cpp); the Rust window opens where winit puts it, its place
    and size not kept.
12. **Screens still JUCE**: Recordings and the Plugins window (above), handed over. (The JUCE Groovebox was removed, #358.)
13. **Quitting**: the Rust UI has no question about unsaved changes of its own; the JUCE window
    asks it when the Rust window closes.

## On a device: the run recipe (not yet run on a unit)

1. Build the bundle (`os/fantom-bundle.sh`): it carries `usr/bin/oscairn-ui` and its libraries,
   with the start and stop scripts and the JUCE app of the same tree.
2. Unpack it as any bundle; nothing is copied in by hand.
3. Start: `./oscairn-fantom-start` (the Rust UI is the default; `OSCAIRN_UI=juce` for the JUCE app); stop with
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

