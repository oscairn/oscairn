# The Performance app: inventory and checklist

Owner's decision (2026-11-15): the whole Performance app of the JUCE program is rebuilt in the Rust
shell, so nothing in it hands the display to JUCE. This file is the checklist; an item is ticked
`[x]` in the commit that builds it (with its test). Sources: `hosts/standalone/PlayView.cpp`,
`PartsView.cpp`, `PartsModel.cpp`, `ParameterGrid.cpp`, and what they use from `Engine.cpp`,
`Theme`, `Profile`, the knob and controller code, Learn.

How to read a row: **Request** is the control request(s) (docs/control-protocol.md) the JUCE
code sends for it (through `Engine::edit` and `RemoteControl`, which sends exactly these when the
app is connected to an engine, as it is on the keyboard). **In-proc** says whether the JUCE code
reaches the engine in-process with no request: `-` means it uses requests only; anything else is
explained in "Items without a request". **No touch** says how it is operated by the knobs under the
screen and the dial: a border cursor moves (left knob: left pane, right knob: right pane, or the
dial: the one list), a press confirms, the dial then changes a value, Cancel backs out one layer.
Touch always works as well (a tap does what a press does).

Status: `[x]` built and tested in the Rust shell, `[ ]` not yet.

## Items without a request (step 2 of the brief)

The brief asks for a new control request wherever the JUCE app calls the engine in-process with
no request. Going through every call of the four files and of what they use, there is **no
engine call without a request**: the app on the keyboard is `--connect`ed and every call goes
through `RemoteControl` to a method of the table. What is not an engine call at all:

- **Plugins a part can add** (`bundledPlugins`, PartsModel.cpp:677): the app runs the isolated
  scanner `oscairn-plugin-scan` beside the executable on the plugin folder and reads its output
  (`scanPlugins`, `oscairn/discovery.h`). The engine is not involved, so no engine request exists
  or is needed; the Rust shell runs the same helper the same way (item D1). If the owner wants
  the engine to do it instead, that is a new request (`plugins.scan`) and a decision for core/control.
- **The on-screen keyboards** (`Engine::openKeyboard`): the playing keyboard sends notes with
  `notes.send` (K2); the text keyboard sends nothing (K1).
- **A plugin's own window** (`openPluginWindow`, request `window.show`): exists as a request, but
  the owner decided that on the keyboard a plugin's own view is drawn into a surface the shell
  hands it (a separate project, not built). **This is the only item allowed to stay unported**
  (F9): the Rust screen shows the host-drawn parameter pages for every plugin and, where a plugin
  offers its own view (`window.info` `available`), a disabled entry labeled "Plugin view: not
  available in this shell yet".

Items needing a new request: **0** (so no change in core/control or docs/control-protocol.md).

## A. Play (PlayView.cpp)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| A1 | Scene tiles (eight), the current one marked; select | `scenes.select` | - | left pane cursor, press | [x] |
| A2 | "+ Scene" tile after the last: add a scene (named "Scene N") | `scenes.add` | - | cursor on the tile, press | [x] src/play.rs: the tile after the last scene (while there are fewer than eight) adds "Scene N" (JUCE PlayView mouseUp); the cursor reaches it, a press or a tap adds |
| A3 | Capture (store) the current state into a scene | `scenes.store` | - | action in the scene menu (A4) | [x] src/scene.rs: the Scene action of Play's top bar opens the playing scene's setup, whose first row is Store (`scenes.store`) |
| A4 | Rename a scene (and the scene menu: store, rename, remove, scope, transition) | `scenes.name`, `scenes.remove`, `scenes.scope`, `scenes.transition` | - | long press / the scene's Edit action; on-screen keyboard (K1) | [x] src/scene.rs: Rename (keyboard, K1), the six scope kinds switched, glide time, curve and "while playing" by a press and the dial or − and +, as the JUCE `sceneEditMenu`. CORRECTION: the JUCE menu has no remove and nothing in the JUCE app calls `scenes.remove`, so none is built; no long press either (the bar's action does it) |
| A5 | Strip per part and bus: name, sound, input, level fader, meters, M, S | `parts.level`, `parts.solo`, `parts.meters` | - | right pane cursor, press opens the part; dial on a fader after press | [x] |
| A6 | Master strip: level, pan | `master.set` | - | cursor on it, press then dial | [x] src/play.rs: the master is the last stop of the right pane (after the last part), a press then the dial moves its fader 1/40 of its travel a click (`master.set` then `graph.commit`, as the JUCE slider). The JUCE Play strip has the level only (it keeps the pan); the pan is in the Mixer (H2) |
| A7 | Selected part outline (press selects a part, Edit opens it) | - | - | press | [x] |
| A8 | Previous / Next of the setlist (when opened from it) | setlist load (`performance.load`/`library.load`) | - | cursor on the button, press | [x] |
| A9 | Knob row following the Performance's controls, or the selected part's own | `controls.set`, `params.set`, `params.gesture`, `params.text`, `controllers.view` | - | the device's knobs play it; shown with values | [ ] |
| A10 | Banks of the knob row (page button, "Performance" / part controls switch) | `controllers.view` (positions), `controllers.get` | - | row buttons / press on the page button | [ ] |
| A11 | Learn: move a knob, then touch a strip (level), its M (mute), a scene or a knob; Default removes the override; Done | `assignments.get`, `assignments.set`, `controllers.get` (the knob moved) | - | Learn in the top bar; then the cursor and press name the target | [ ] |
| A12 | The no-MIDI hint ("No MIDI device. Open the on-screen keyboard", Dismiss, kept) | `session.status` (`midiInputs`) | - | cursor on the link, press | [x] src/play.rs, settings.rs: shown when the profile offers the on-screen keyboard, the status notification's `midiInputs` has none but the built-in and it was not dismissed; its two links are the first stops of the right pane; Dismiss is kept in the JUCE app's keyboard.txt ("hint dismissed", the window places after it kept) |
| A13 | Empty state: "No parts yet. Edit adds them." | - | - | - | [x] slint/play.slint, tested in tests/play.rs (the words are the JUCE ones, which name Edit: they stay until Edit is a screen of this app, A14) |
| A14 | Edit / Play toggle in the top bar | - | - | top bar action | [ ] |
| A15 | Save (to the library; names the Performance when it has no id, or Save As New) | `performance.info`, `library.save` | - | top bar action; keyboard (K1) for a new name | [x] src/shell.rs: the Save button saves a Performance in the library at once, asks the name of one that is not (K1), a blank name keeps its own; the Performance's name on the bar is Save As New (JUCE: a tap on it); the engine's refusal and "Performance saved." are said in the name's place for 4 s |
| A16 | Learn in the top bar when the device has knobs | - | - | top bar action | [ ] |
| A17 | Meters and live values while shown only | `parts.meters`, `controls.changed` | - | - | [x] |

## B. Parts list (PartsView page 0)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| B1 | Rows per part: name, plays from, chain summary, level, mute, a send per bus | `graph.get` | - | list cursor, press opens the part | [ ] |
| B2 | Level and mute on the row | `parts.level` | - | press then dial | [ ] |
| B3 | A send's level per bus on the row | `parts.send` | - | press then dial | [ ] |
| B4 | Shared buses under the parts; "Fed by ... in Routing" rows | `graph.get` | - | cursor, press opens Routing | [ ] |
| B5 | + Add part, + Add bus (named "Part N", "Bus A..") | `parts.add`, `graph.commit` | - | cursor on the row, press | [ ] |
| B6 | Remove part | `parts.remove` | - | action on the part | [ ] |
| B7 | Reorder parts (move) | n/a: JUCE has no part reorder, only chain moves (see D3) | - | - | n/a |
| B8 | Page switches: Parts, Controls, Mixer, Routing; Done | - | - | list of pages; Cancel | [ ] |
| B9 | Failed plugins marked (plugin that failed and is silent) | `session.status` (`failures`) | - | - | [ ] |
| B10 | Notes given up by the shared budget shown on the row | `graph.get` (`givenUp`) / `parts.meters` | - | - | [ ] |

## C. Part page (PartsView page = part id)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| C1 | Level, pan, mute, solo | `parts.level`, `parts.pan`, `parts.solo` | - | cursor, press, dial | [x] |
| C2 | Plays from (devices list) | `parts.input` | - | list, press ticks | [x] |
| C3 | Lowest key, highest key, transpose (first zone) | `parts.input` | - | press then dial | [x] |
| C4 | Sustain, sostenuto, soft pedal receive | `parts.input` | - | press then dial | [x] |
| C5 | Rename the part | `parts.name` | - | press, keyboard (K1) | [x] |
| C6 | Input channel (any, 1..16, several: shown, not picked) | `parts.input` | - | press then dial / list | [x] |
| C7 | Notes at once (any, 1..N) | `parts.input` | - | press then dial | [x] |
| C8 | Reserved notes (none .. limit) | `parts.input` | - | press then dial | [x] |
| C9 | Priority 0..10 | `parts.input` | - | press then dial | [x] |
| C10 | Velocities (low, high) | `parts.input` | - | press then dial | [x] |
| C11 | Plays to (Main or a bus) | `parts.output` | - | press then dial / list | [x] |
| C12 | Sends per bus (level, 0 removes) | `parts.send` | - | press then dial | [x] |
| C13 | MIDI out (devices list: Off, connected outputs, rules not connected) and MIDI out channel (own, 1..16) | `parts.midiOut`, `session.status` (`midiOutputs`) | - | list, press ticks | [x] |
| C14 | Open the on-screen playing keyboard (K2) of the input that plays the part | `notes.send` (K2) | - | action on the part | [x] (Part screen's "Keyboard" row; hidden when the profile's onScreenKeyboard is false) |
| C15 | Sound: choose from the library (D1/E) | see E | - | action | [x] |
| C16 | Engine (the instrument's plugin) shown; replace instrument | `graph.get`; replace: `graph.replace` | - | action | [ ] |
| C17 | Part controls (a part's own macros) listed with targets | `graph.get` (`controls`) | - | see G | [ ] |
| C18 | Remove part | `parts.remove` | - | action, confirm | [x] |
| C19 | The handover row "Oscairn app" | removed | - | - | [x] |
| C20 | Key (harmony) of the part | `harmony.set` (`part` key) | - | press then dial | [ ] |

## D. Chain, insert effects, splits and lanes

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| D1 | The plugin chooser for Insert (instrument or effect, from the scanner, names without the vendor prefix, sorted) | none (helper process, see above) | helper | list, press | [x] |
| D2 | Insert a plugin at a position (a new node named uniquely: "Basic Synth", "Basic Synth 2"), in a lane | `graph.add`, `parts.insert`, `graph.commit` (removes the node when insert fails) | - | cursor on the position, press, choose | [x] |
| D3 | Move left / right in the chain | `parts.move` | - | action | [x] |
| D4 | Remove a box (keeps user wires reconnected) | `parts.removeNode` | - | action | [x] |
| D5 | Turn a plugin off / on | `graph.enable` | - | action | [x] |
| D6 | Show the chain (plugins in order, with their role and off marks, built in chain.rs; splits listed read only; failed marks and role colors not yet): IN, boxes, OUT, "Dry" lane, role colors (instrument, effect), failed and off marks | `graph.get` | - | cursor along the chain | [ ] |
| D7 | Insert a split (copy, mid/side, left/right, bands) with its Split/Join plugins | `parts.addSplit`, `graph.add` | - | action | [ ] |
| D8 | Add lane / band, remove lane (with its plugins), remove split (keeps the first lane) | `parts.addLane`, `parts.removeLane`, `parts.removeSplit`, `graph.remove` | - | action | [ ] |
| D9 | Lane settings: keys (zone editor), level, pan, mute, solo, crossovers | `parts.lane` | - | press then dial | [ ] |
| D10 | Scene controls of a plugin ("Scene controls...": which plugin parameters a scene touches) | `scenes.untouched` | - | action | [ ] |
| D11 | Routing... on a box (opens I) | - | - | action | [ ] |
| D12 | A box's plugin parameters (host-drawn pages, F) | see F | - | press | [x] |

## E. Sound chooser

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| E1 | Library sounds list (query, search, tags, favorites) | `library.query` | - | list, press | [ ] |
| E2 | Load a sound into the part | `parts.loadSound` | - | press | [x] |
| E3 | New part from a sound | `parts.fromSound` | - | action | [x] |
| E4 | Save the part as a sound (name, as new) | `parts.saveSound` | - | action, keyboard | [x] |
| E5 | Audition a sound | `audition.start`, `audition.stop`, `audition.get` | - | action | [x] |
| E6 | Preset of the plugin / reload with a preset | `presets.list`, `graph.replace` | - | list | [ ] |

(Request names checked against docs/control-protocol.md "Sounds", "Library" and `RemoteControl`.)

## F. Plugin parameter pages (ParameterGrid.cpp), host-drawn

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| F1 | Read the plugin's parameters (`params.list`), hide hidden ones, read-only ones show the value only | `params.list` | - | - | [x] |
| F2 | Pages: one per top-level module path ("Main" when none), numbered pages together; modules from the leading words of names when more than 24 parameters have no paths | derived | - | page list pane | [x] |
| F3 | Tabs for the second level on a page of more than 24; a heading per module below | derived | - | tab list | [x] |
| F4 | Tiles by priority (oscairn.ui `priority`), each with name, value text, a control: slider, two values a switch, a few values a list of them | `params.get`, `params.set`, `params.text`, `params.gesture` | - | on the grid (ui/GRID.md, branch rust-grid): knob, slider, switch, dropdown, split buttons, range, value; cursor in two dimensions, press, dial; each step sent between a gesture begin and end | [x] |
| F5 | Search a parameter by name or module on any page | derived | - | keyboard (K1): back from the first tile (or up from the first row) is the search field, a press opens the on-screen keyboard, Cancel backs out the field, the cursor, the search, then the screen. Touch: a tap on the field. Built on rust-app-grid (9ee154cb; the field only for a plugin of more than 24 parameters, as the reference) | [x] |
| F6 | The selected row/column of tiles is what the device's knobs play ("row:N" knobs); on the page, the screen knobs play the page by priority | `controllers.view` (screen page), `params.set` | - | the knob row (A9). Done on rust-grid: the screen page published by priority, withdrawn on leaving. Not done: `row:N` (no device profile has such knobs) | [ ] |
| F7 | Rich views by field roles: envelopes (ADSR by name, DX7 rates/levels, FM-X), key and velocity maps, FM operator views and algorithm diagrams, LFOs, filters (response drawn) | `params.*` | - | the same tiles are always reachable; the rich view is drawn above them, and the node or handle a tile sets is marked where the cursor is on it. Every view of the reference is built on rust-app-grid (src/views.rs, ui/GRID.md has the list with file:line): the envelope (fields and, as the reference, ADSR, DX7 and FM-X names) with its nodes dragged by touch (e05bff23), the LFO (02ffdb0c) and filter (967cd464) previews, the key and velocity map (2fdad232) and the FM operator view (ec760a14), each dragged by touch where the reference drags it. Lines are drawn unscaled from one set of points and measured in the rendered pixels (tests/views.rs). Unverified on a unit | [x] |
| F8 | Values follow the engine as they change (`params.changed`/`controls.changed`) | notifications | - | - | [x] |
| F9 | The plugin's own view | `window.info`, `window.show` | window | **disabled entry: not available in this shell yet (the only item allowed to stay unported)** | [x] |

## G. Controls (macros) and scene mapping (PartsView controlsPage)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| G1 | List of controls with a fader each (Performance's, and a part's own) | `graph.get` (`controls`), `controls.set` | - | cursor, press, dial | [ ] |
| G2 | + Control, rename (double click), remove | `controls.add`, `controls.name`, `controls.remove` | - | actions; keyboard | [ ] |
| G3 | Targets: + Target menu (plugin parameter, part level, part mute, part send, another control, morph between scenes), remove, relative, curve (linear, exponential, logarithmic, S-curve, fader dB, power) | `controls.targets` | - | menus as lists | [ ] |
| G4 | Scenes a mapping works in ("Scenes..."), "Now..." (start/end = now) | `controls.targets` | - | list | [ ] |
| G5 | A control's source (a plugin's lane) | `controls.source` | - | list | [ ] |
| G6 | "By scene" table of mappings | derived | - | toggle | [ ] |
| G7 | Scene edit menu: store, rename, scope (plugins/wires on-off, mutes and solos, levels pans sends, parameters and controls, control mappings), transition (glide time, curve, launch while playing), untouched parameters | `scenes.store`, `scenes.name`, `scenes.scope`, `scenes.transition`, `scenes.untouched` | - | menu lists | [ ] |

## H. Mixer (PartsView mixerPage)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| H1 | Strip per part and bus: name, fader with meter, pan, M, S | `parts.level`, `parts.pan`, `parts.solo`, `parts.meters` | - | cursor, press, dial | [ ] |
| H2 | Master: level, pan | `master.set` | - | cursor, press, dial | [ ] |

## I. Routing (PartsView routingPage)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| I1 | The graph by signal flow: columns, wires, collapsed parts (Expand/Collapse, Open part), meters on wires | `graph.get`, `wires.meters` | - | cursor along boxes (list of nodes) | [ ] |
| I2 | Connect audio / notes to..., Connect lane to... (control or parameter) | `wires.connect`, `graph.map` | - | list menus | [ ] |
| I3 | Disconnect; per-source mix (level, pan, M, S, On); wire channel | `wires.disconnect`, `wires.mix`, `wires.enable`, `wires.zones` | - | press then dial | [ ] |
| I4 | + Plugin outside the parts; Remove it | `graph.add`, `graph.remove`, `graph.commit` | - | action | [ ] |
| I5 | Turn on/off a node; Open part | `graph.enable` | - | action | [ ] |
| I6 | Disconnect lane (parameter map) | `graph.unmap` | - | action | [ ] |

## J. Top bar and cross-cutting

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| J1 | Undo / Redo with the edit's name on the button | `edit.undo`, `edit.redo`, `edit.history` | - | top bar actions | [x] src/shell.rs, slint/topbar.slint: on every screen of the app (Play, Part, Chain, Sounds, Parameters; the JUCE buttons are in the Edit view header only), "Undo <edit>" and "Redo <edit>" off when empty; edit.history asked on entering a screen and on each graph.changed. The bar's cursor (all of A14-A16's top bar actions): the third pane (`move:2`), Tab, or the dial moving up out of the first row of Play, Part, Chain and Sounds (not the parameter grid: it needs a hook in grid.rs); Confirm does it, past the last stop or Cancel gives the cursor back |
| J2 | The refusal of an edit said on the screen | error replies | - | - | [x] |
| J3 | The shell opens the Performance app everywhere (Home tile, Setlist's Play, Play's part press), the handover entries and "Oscairn app" rows for this app gone | - | - | - | [ ] |
| J4 | Recording indicator (owner, 2026-11-15): a take running and its time, on every screen's top bar and on Play: a filled dot, "REC m:ss" and "disk slow" when the writer is behind, in the danger color. Learned as the JUCE RecordButtons learn it (`Engine::takes`): `record.takes` polled every 250 ms (here with the `status` notifications, so nothing wakes at rest); the first take `waiting` or `recording`, its frames over its sample rate. Not built: the Record and Capture buttons themselves (the hardware REC button starts and ends a take in the engine) | `record.takes` | - | shown only | [x] |

## K. Text entry

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| K1 | On-screen text keyboard: a port of the JUCE TextKeyboard (every key, the symbols layer, Shift, Caps, caret, Clear, Cancel, Done, held repeat, the profile's `textEntry`), used by rename of parts, scenes, controls, Save, search | - | - | dial or left knob steps the cursor over the keys, right knob moves the caret, press types, Cancel closes, End finds Done | [x] (used by C5; scene, control, Save and search users come with their items) |
| K2 | On-screen playing keyboard: a port of the JUCE KeyboardWindow (the keys that play when no MIDI keyboard is connected; Play's no-MIDI hint, A12, and each part's keyboard button, C14, lead to it; the profile's `onScreenKeyboard`), a range and key size that suit the 1280x720 touch screen, an octave shift on a knob | `notes.send` | - | knob for the octave, cursor over keys, press plays | [x] src/piano.rs: two octaves and the next C, octave on the left knob, cursor on the right knob and dial; the Play no-MIDI hint (A12) still to add as a second way in |

## Counts

Run `grep -c '^| [A-K][0-9]' ui/PERFORMANCE-APP.md` for the row total; the rows with `[x]` are done.

## What the keyboards change from the JUCE ones (owner: a better layout is welcome, behaviors kept)

K1 keeps every key and action of TextKeyboard.cpp (the rows, the symbols layer, one-shot Shift,
Caps, Left and Right, Clear, Cancel, Done as Return, held repeat 450/70 ms, US QWERTY, no on-screen
keys where `textEntry` is `keyboard`). Changed: keys up to twice a touch target tall (stage use);
the first row's keys are proportions of the width, not fixed pixels; Cancel returns nothing to the
caller (the JUCE one restores the editor's text, the same as nothing happening). Added for use
without touch: the border cursor and the knobs. No owner rule was found in the JUCE comments for
a layout; RFC 0019 is the source of the JUCE behavior. K2 keeps the JUCE KeyboardWindow's behaviors (typing keys from middle C, Z and X octaves at most 4, Fixed 100/127 else by finger height, Sustain held locally, bands per part, input's held keys lit, closes with its input) and changes the layout: two octaves C3-C5 (15 white keys, 85 px on the FANTOM-07 against the JUCE 56 px over three octaves), octave on the left knob.

## Design correction (owner, after the first pictures): the grid framework comes first

The owner: "the grid of tiles and proper sliders is a part of the design that we need to keep ... one of the options for plugin control is a layout framework that the plugin can control through data with knobs, sliders, graphs, dropdowns, split buttons etc in a grid". The row-list parameter pages (params.rs) are therefore NOT acceptable as the design and F4/F7 are to be redone (their [x] stands for the row version only). To do, before the rest of the list: (1) read RFC 0011, 0019, 0042, oscairn-clap/include/oscairn-clap/ui.h and docs, and ParameterGrid.cpp end to end, and list every control kind and layout rule here as checklist items; (2) a grid module of its own (tiles with spans; knob, vertical and horizontal slider, drawbar, switch, dropdown, split button, value display, envelope and other graph views; the plugin's own value text; cursor over tiles in two dimensions, press selects, dial changes, a dropdown opens a list, split-button halves are stops; the device's knobs control the visible row or bank as ParameterGrid and the profile say); (3) parameter pages, the Play knob row and banks, and the mixer strips rebuilt on it, each compared with the JUCE layout; (4) tests per widget and per navigation, PNGs at scale 1 and 1.55 of a page with every control kind (the harness has `at_fantom_scale` and `fit_to_window` in src/tests.rs).

### Status of the correction (branch rust-grid, 2026-11-15)

Done: (1) ui/GRID.md; (2) the grid module (src/grid.rs, slint/grid.slint); (3) the parameter pages on
it (F4 redone; F7 partly: envelopes). Not done: the Play knob row and banks on grid tiles (A9-A11),
the mixer strips' faders as the grid's upright slider (H), the other views.

### Owner requirements received during rust-grid (2026-11-15), binding on every screen

- The grid of tiles with real controls (knobs, sliders, drawbars, dropdowns, split buttons, graphs)
  is the design; the plugin controls its layout through data (`params.list`'s hints).
- One universal UI: the keyboard's shell, a desktop app with resizable, responsive windows (OS
  scaling 1x, 2x, fractional; separate windows where the JUCE app opens them; plan in
  agent-notes/desktop-ui.md, `surface.rs` not built), later a DAW plugin's window: screens assume
  no window size, scale factor or process-wide globals (the debts are listed in ui/GRID.md).
- Smaller and larger device screens (800x480 at 1, 1920x1080 at 2): screens reflow, touch sizes kept.
- The text keyboard on a hardware device overlays about the lower half of the screen as
  hosts/standalone/TextKeyboard.cpp does, the field being edited kept in view: not started.
- The whole Performance app moves to the shell; no handover for a half-ported app.
