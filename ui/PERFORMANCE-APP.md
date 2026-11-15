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
- **The on-screen keyboard** (`Engine::openKeyboard`, `openPads`): the shell's own window, no
  request (C14; the keyboard window is part of the shell, not of this app's list).
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
| A2 | "+ Scene" tile after the last: add a scene (named "Scene N") | `scenes.add` | - | cursor on the tile, press | [ ] |
| A3 | Capture (store) the current state into a scene | `scenes.store` | - | action in the scene menu (A4) | [ ] |
| A4 | Rename a scene (and the scene menu: store, rename, remove, scope, transition) | `scenes.name`, `scenes.remove`, `scenes.scope`, `scenes.transition` | - | long press / the scene's Edit action; on-screen keyboard (K1) | [ ] |
| A5 | Strip per part and bus: name, sound, input, level fader, meters, M, S | `parts.level`, `parts.solo`, `parts.meters` | - | right pane cursor, press opens the part; dial on a fader after press | [x] |
| A6 | Master strip: level, pan | `master.set` | - | cursor on it, press then dial | [ ] |
| A7 | Selected part outline (press selects a part, Edit opens it) | - | - | press | [x] |
| A8 | Previous / Next of the setlist (when opened from it) | setlist load (`performance.load`/`library.load`) | - | cursor on the button, press | [x] |
| A9 | Knob row following the Performance's controls, or the selected part's own | `controls.set`, `params.set`, `params.gesture`, `params.text`, `controllers.view` | - | the device's knobs play it; shown with values | [ ] |
| A10 | Banks of the knob row (page button, "Performance" / part controls switch) | `controllers.view` (positions), `controllers.get` | - | row buttons / press on the page button | [ ] |
| A11 | Learn: move a knob, then touch a strip (level), its M (mute), a scene or a knob; Default removes the override; Done | `assignments.get`, `assignments.set`, `controllers.get` (the knob moved) | - | Learn in the top bar; then the cursor and press name the target | [ ] |
| A12 | The no-MIDI hint ("No MIDI device. Open the on-screen keyboard", Dismiss, kept) | `session.status` (`midiInputs`) | - | cursor on the link, press | [ ] |
| A13 | Empty state: "No parts yet. Edit adds them." | - | - | - | [ ] |
| A14 | Edit / Play toggle in the top bar | - | - | top bar action | [ ] |
| A15 | Save (to the library; names the Performance when it has no id, or Save As New) | `performance.info`, `library.save` | - | top bar action; keyboard (K1) for a new name | [ ] |
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
| C5 | Rename the part | `parts.name` | - | press, keyboard (K1) | [ ] |
| C6 | Input channel (any, 1..16, several: shown, not picked) | `parts.input` | - | press then dial / list | [ ] |
| C7 | Notes at once (any, 1..N) | `parts.input` | - | press then dial | [ ] |
| C8 | Reserved notes (none .. limit) | `parts.input` | - | press then dial | [ ] |
| C9 | Priority 0..10 | `parts.input` | - | press then dial | [ ] |
| C10 | Velocities (low, high) | `parts.input` | - | press then dial | [ ] |
| C11 | Plays to (Main or a bus) | `parts.output` | - | press then dial / list | [ ] |
| C12 | Sends per bus (level, 0 removes) | `parts.send` | - | press then dial | [ ] |
| C13 | MIDI out (devices list: Off, connected outputs, rules not connected) and MIDI out channel (own, 1..16) | `parts.midiOut`, `session.status` (`midiOutputs`) | - | list, press ticks | [ ] |
| C14 | Open the on-screen keyboard of the input that plays the part | shell window, no request | n/a | action on the part | [ ] (shell: out of this list's engine scope, button hidden when the profile offers none) |
| C15 | Sound: choose from the library (D1/E) | see E | - | action | [ ] |
| C16 | Engine (the instrument's plugin) shown; replace instrument | `graph.get`; replace: `graph.replace` | - | action | [ ] |
| C17 | Part controls (a part's own macros) listed with targets | `graph.get` (`controls`) | - | see G | [ ] |
| C18 | Remove part | `parts.remove` | - | action, confirm | [ ] |
| C19 | The handover row "Oscairn app" | removed | - | - | [ ] |
| C20 | Key (harmony) of the part | `harmony.set` (`part` key) | - | press then dial | [ ] |

## D. Chain, insert effects, splits and lanes

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| D1 | The plugin chooser for Insert (instrument or effect, from the scanner, names without the vendor prefix, sorted) | none (helper process, see above) | helper | list, press | [ ] |
| D2 | Insert a plugin at a position (a new node named uniquely: "Basic Synth", "Basic Synth 2"), in a lane | `graph.add`, `parts.insert`, `graph.commit` (removes the node when insert fails) | - | cursor on the position, press, choose | [ ] |
| D3 | Move left / right in the chain | `parts.move` | - | action | [ ] |
| D4 | Remove a box (keeps user wires reconnected) | `parts.removeNode` | - | action | [ ] |
| D5 | Turn a plugin off / on | `graph.enable` | - | action | [ ] |
| D6 | Show the chain: IN, boxes, OUT, "Dry" lane, role colors (instrument, effect), failed and off marks | `graph.get` | - | cursor along the chain | [ ] |
| D7 | Insert a split (copy, mid/side, left/right, bands) with its Split/Join plugins | `parts.addSplit`, `graph.add` | - | action | [ ] |
| D8 | Add lane / band, remove lane (with its plugins), remove split (keeps the first lane) | `parts.addLane`, `parts.removeLane`, `parts.removeSplit`, `graph.remove` | - | action | [ ] |
| D9 | Lane settings: keys (zone editor), level, pan, mute, solo, crossovers | `parts.lane` | - | press then dial | [ ] |
| D10 | Scene controls of a plugin ("Scene controls...": which plugin parameters a scene touches) | `scenes.untouched` | - | action | [ ] |
| D11 | Routing... on a box (opens I) | - | - | action | [ ] |
| D12 | A box's plugin parameters (host-drawn pages, F) | see F | - | press | [ ] |

## E. Sound chooser

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| E1 | Library sounds list (query, search, tags, favorites) | `library.query` | - | list, press | [ ] |
| E2 | Load a sound into the part | `parts.loadSound` | - | press | [ ] |
| E3 | New part from a sound | `parts.fromSound` | - | action | [ ] |
| E4 | Save the part as a sound (name, as new) | `parts.saveSound` | - | action, keyboard | [ ] |
| E5 | Audition a sound | `audition.start`, `audition.stop`, `audition.get` | - | action | [ ] |
| E6 | Preset of the plugin / reload with a preset | `presets.list`, `graph.replace` | - | list | [ ] |

(Request names checked against docs/control-protocol.md "Sounds", "Library" and `RemoteControl`.)

## F. Plugin parameter pages (ParameterGrid.cpp), host-drawn

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| F1 | Read the plugin's parameters (`params.list`), hide hidden ones, read-only ones show the value only | `params.list` | - | - | [ ] |
| F2 | Pages: one per top-level module path ("Main" when none), numbered pages together; modules from the leading words of names when more than 24 parameters have no paths | derived | - | page list pane | [ ] |
| F3 | Tabs for the second level on a page of more than 24; a heading per module below | derived | - | tab list | [ ] |
| F4 | Tiles by priority (oscairn.ui `priority`), each with name, value text, a control: slider, two values a switch, a few values a list of them | `params.get`, `params.set`, `params.text`, `params.gesture` | - | cursor, press, dial; each step sent between a gesture begin and end | [ ] |
| F5 | Search a parameter by name or module on any page | derived | - | keyboard (K1) | [ ] |
| F6 | The selected row/column of tiles is what the device's knobs play ("row:N" knobs); on the page, the screen knobs play the page by priority | `controllers.view` (screen page), `params.set` | - | the knob row (A9) | [ ] |
| F7 | Rich views by field roles: envelopes (ADSR by name, DX7 rates/levels, FM-X), key and velocity maps, FM operator views and algorithm diagrams, LFOs, filters (response drawn) | `params.*` | - | the same tiles are always reachable; the rich view is drawn beside | [ ] |
| F8 | Values follow the engine as they change (`params.changed`/`controls.changed`) | notifications | - | - | [ ] |
| F9 | The plugin's own view | `window.info`, `window.show` | window | **disabled entry: not available in this shell yet (the only item allowed to stay unported)** | [ ] |

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
| J1 | Undo / Redo with the edit's name on the button | `edit.undo`, `edit.redo`, `edit.history` | - | top bar actions | [ ] |
| J2 | The refusal of an edit said on the screen | error replies | - | - | [x] |
| J3 | The shell opens the Performance app everywhere (Home tile, Setlist's Play, Play's part press), the handover entries and "Oscairn app" rows for this app gone | - | - | - | [ ] |

## K. Text entry

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| K1 | On-screen keyboard operable by dial and press as well as touch (used by rename of parts, scenes, controls, Save, search) | - | - | dial moves, press types, Cancel backs out; Done confirms | [ ] |

## Counts

Run `grep -c '^| [A-K][0-9]' ui/PERFORMANCE-APP.md` for the row total; the rows with `[x]` are done.
