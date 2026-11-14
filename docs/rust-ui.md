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
  the cargo feature `kms`. The cargo feature `roland-fantom0` adds the FANTOM-0 panel reader
  (optional hardware compiled out elsewhere).
- CMake: `-DOSCAIRN_UI=ON` builds it with cargo and installs `bin/oscairn-ui`; off by default,
  as the JUCE options are, because cargo downloads its crates on the first build. The device
  build (`OSCAIRN_ROLAND_FANTOM0`) takes the `kms` and `roland-fantom0` features. ctest runs its
  Rust tests.
- The device bundle: installed as `/usr/bin/oscairn-ui` by the bundle's Oscairn package, with
  `libxkbcommon` and `libinput` from the bundle's own libraries; no more `/opt/rust-setlist`.

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
- **A navigation layer of abstract actions**: move a pane's cursor (or the focused list's) by
  some steps, page, step (Previous/Next), confirm, cancel, home. Three sources produce them: touch
  (a tap moves the cursor there and confirms), a computer keyboard, and a device's panel through
  roles its panel map names (`navigation`, below). The abilities are the same on every device;
  only the map is the device's.
- **Themes are the JUCE app's** (below): the same folders and the same choice.

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
</content>
