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

- **What a device's knobs play now** (A9): no request returns the resolved slots of a control group.
  The JUCE app resolves them in-process with the library (`resolveControls`, `knobBanks`,
  `groupPositions`) from `graph.get`, `params.list` and `assignments.get`, which are requests; the
  shell resolves them with a port of those rules (ui/src/knobs.rs, tested against the cases of
  core/control/tests/physical_controls.cpp). A request such as `controllers.resolved` would make the
  port unnecessary and the tiles exact by construction: a decision for core/control, not taken here.

- **Declared pages, tabs and value icons** (draft 6 of the layout extension, ui.h `tab_count`, `tab`, `param_tab`,
  `value_icon`; ui/TABS.md): the engine's `params.list` has no field for them yet. What the UI would read, named
  provisionally by the UI until the protocol names them: at the top of the result `uiTabs`, an array of
  `{id, parent, name}` (`parent` 0 a page, else a page's id; the plugin's order); on each parameter `uiTab`, the
  id of the page or tab it is on (absent: the module path puts it); and `valueIcons`, for a stepped parameter of
  at most 128 values, the `enum oscairn_icon` of each value from its minimum (0 none). The UI fills its pages and
  tabs model from these where present and from the module paths otherwise, and takes a declared icon before the
  name rule; with none of it the screens are as they were. This is a request for core/control to forward the
  fields (a decision not taken here), not a new method.

Items needing a new request: **0** (so no change in core/control or docs/control-protocol.md); one *field* in
`params.list`, above, would let plugins declare tabs and icons.

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
| A9 | Knob row following the Performance's controls, or the selected part's own | `controls.set`, `params.set`, `params.gesture`, `params.text`, `controllers.view` | - | the device's knobs play it; shown with values | [x] src/row.rs, knobs.rs, slint/play.slint: a grid tile for each knob of the profile's screen group (else the first assign group, else four), what the engine plays on it resolved as physical_controls.cpp resolves it, the value text from params.text; touch turns a knob (gesture, params.set, controls.set, parts.level), a press then the dial changes it, Cancel ends. The part and page are published with controllers.view (`part`, `positions`), the screen page stays empty, as PlayView does; lists is false while Play shows the row so the panel's knobs play |
| A10 | Banks of the knob row (page button, "Performance" / part controls switch) | `controllers.view` (positions), `controllers.get` | - | row buttons / press on the page button | [x] the Performance and part buttons (while a part opened from Play is selected) and the bank button (the row's name on a device with rows) are stops of the row and touch targets; the page is published |
| A11 | Learn: move a knob, then touch a strip (level), its M (mute), a scene or a knob; Default removes the override; Done | `assignments.get`, `assignments.set`, `controllers.get` (the knob moved) | - | Learn in the top bar; then the cursor and press name the target | [x] src/row.rs, play.rs: `controllers` says the control moved, the strips, their M (a second stop on each strip while Learn waits), a scene or a knob is the target, Default (`assignments.set` none), Done, Cancel; the overrides are read again after |
| A12 | The no-MIDI hint ("No MIDI device. Open the on-screen keyboard", Dismiss, kept) | `session.status` (`midiInputs`) | - | cursor on the link, press | [x] src/play.rs, settings.rs: shown when the profile offers the on-screen keyboard, the status notification's `midiInputs` has none but the built-in and it was not dismissed; its two links are the first stops of the right pane; Dismiss is kept in the JUCE app's keyboard.txt ("hint dismissed", the window places after it kept) |
| A13 | Empty state: "No parts yet. Edit adds them." | - | - | - | [x] slint/play.slint, tested in tests/play.rs (the words are the JUCE ones, which name Edit: they stay until Edit is a screen of this app, A14) |
| A14 | Edit / Play toggle in the top bar | - | - | top bar action | [x] src/topbar.rs, shell.rs: Edit is the last action of Play's top bar and opens the Parts list in this UI; the list's bar has Play, the toggle back (the list's Back and Cancel too); the footer's handover button is gone |
| A15 | Save (to the library; names the Performance when it has no id, or Save As New) | `performance.info`, `library.save` | - | top bar action; keyboard (K1) for a new name | [x] src/shell.rs: the Save button saves a Performance in the library at once, asks the name of one that is not (K1), a blank name keeps its own; the Performance's name on the bar is Save As New (JUCE: a tap on it); the engine's refusal and "Performance saved." are said in the name's place for 4 s |
| A16 | Learn in the top bar when the device has knobs | - | - | top bar action | [x] the bar's Learn action where the profile has an assign group of knobs (JUCE: `rowKnobs() > 0`, a panel with row:N knobs, true on no profile, so Learn would never show) |
| A17 | Meters and live values while shown only | `parts.meters`, `controls.changed` | - | - | [x] |

## B. Parts list (PartsView page 0)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| B1 | Rows per part: name, plays from, chain summary, level, mute, a send per bus | `graph.get` | - | list cursor, press opens the part | [x] src/parts.rs, slint/parts.slint: a row per part then per bus, as PartsModel.cpp's readParts reads graph.get (name, sound, input with "No device:", key bar, chain boxes, shared rows); the rows a view holds fill the height; the cursor is two-dimensional (dial cell by cell, left knob along the row, right knob down), a press on the part opens its Part screen |
| B2 | Level and mute on the row | `parts.level` | - | press then dial | [x] src/parts.rs: a press on the level starts the change, the dial steps it a fortieth of the slider (the JUCE slider is linear, 0 to 1), `parts.level` then `graph.commit`; mute is a press; touch drags and taps |
| B3 | A send's level per bus on the row | `parts.send` | - | press then dial | [x] src/parts.rs: the same on a send's cell; taken to 0 it is removed (`parts.send` with no level), as setSendLevel; buses have none |
| B4 | Shared buses under the parts; "Fed by ... in Routing" rows | `graph.get` | - | cursor, press opens Routing | [x] src/parts.rs: the shared rows and "Fed by ... in Routing" are built and tested; a press says "Routing is not in this shell yet" until Routing (I) exists, then `Page::Routing.built()` and the shell's `Outcome::Routing` take it there src/parts.rs, src/routing.rs: a press (or a tap) on a shared row opens Routing with that plugin chosen (`Outcome::Routing`). |
| B5 | + Add part, + Add bus (named "Part N", "Bus A..") | `parts.add`, `graph.commit` | - | cursor on the row, press | [x] src/parts.rs: the + Part and + Bus buttons under the list, named "Part N" (N the parts after it) and "Bus A" and on, with the input the JUCE addPart gives (notes, or bus) |
| B6 | Remove part | `parts.remove` | - | action on the part | [x] the JUCE page 0 has no remove: it is on the part's own page, the Part screen's Remove part (C18); the list opens it |
| B7 | Reorder parts (move) | n/a: JUCE has no part reorder, only chain moves (see D3) | - | - | n/a |
| B8 | Page switches: Parts, Controls, Mixer, Routing; Done | - | - | list of pages; Cancel | [x] src/parts.rs: the buttons Mixer, Controls and Routing open their pages (the shell's `Outcome::Page`); Done is Back, Cancel, and the top bar's Play (the bar's Play and Cancel on each page go back as well) |
| B9 | Failed plugins marked (plugin that failed and is silent) | `session.status` (`failures`) | - | - | [x] src/parts.rs: the chain box of a plugin named in `failures` is outlined in the danger color and the line under the list says "X is silent: reason" (unless an edit was refused), as PartsView::refresh |
| B10 | Notes given up by the shared budget shown on the row | `graph.get` (`givenUp`) / `parts.meters` | - | - | [x] src/parts.rs: `parts.meters` every 200 ms while the list shows (the JUCE timer): the second line gets "· N stolen, M cut" from `stolen` and `hardChokes` |

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
| C16 | Engine (the instrument's plugin) shown; replace instrument | `graph.get`; replace: `graph.replace` | - | action | [x] the Engine (the first instrument's plugin) is a line of the Part screen's panel. CORRECTION: the JUCE Part page has no "replace instrument": it is PluginBrowser's ("Replace instrument" / "Reload with preset", `Engine::replacePlugin` on a named slot such as "synth"), a page of the Plugins app, not of the Performance app; nothing built for it here (the preset half is E6) |
| C17 | Part controls (a part's own macros) listed with targets | `graph.get` (`controls`) | - | see G | n/a: the JUCE Part page lists no controls (PartsView.cpp: `macros_` is read by the Controls page only, G1 lists a part's own beside the Performance's) |
| C18 | Remove part | `parts.remove` | - | action, confirm | [x] |
| C19 | The handover row "Oscairn app" | removed | - | - | [x] |
| C20 | Key (harmony) of the part | `harmony.set` (`part` key) | - | press then dial | [x] src/part.rs: "Key" (follows the Performance's, named from `harmony.get`, or a root of its own, in the scale it had) and, with a key of its own, "Scale" (the names `harmony.get` gives); each step `harmony.set` with the part then `graph.commit`, the dial back before the first root follows again. NOT in the JUCE app (hosts/standalone has no control for the key), built from RFC 0033 and the protocol: an addition, to be confirmed by the owner |

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
| D10 | Scene controls of a plugin ("Scene controls...": which plugin parameters a scene touches) | `scenes.untouched` | - | action | [x] src/chain.rs: "Scene controls..." among a plugin's actions lists its parameters (`params.list`) ticked where scenes touch them (automatable, not read only, not set aside); choosing one sets it aside or takes it back with the others kept (`scenes.untouched`, from graph.get's `sceneUntouched`, no commit as the JUCE `sceneControlsMenu`); the menu is menu.rs's. |
| D11 | Routing... on a box (opens I) | - | - | action | [x] src/chain.rs: "Routing..." among a plugin's actions opens Routing with it chosen. |
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
| F2 | Pages: one per top-level module path ("Main" when none), numbered pages together; modules from the leading words of names when more than 24 parameters have no paths | derived | - | the first level of the shell's tab family (a strip or a column), the cursor walks it, a press opens (ui/TABS.md) | [x] |
| F3 | Tabs for the second level on a page of more than 24; a heading per module below | derived | - | the second level of the tab family; plugins may name and order their own (provisional fields, ui/TABS.md) | [x] |
| F4 | Tiles by priority (oscairn.ui `priority`), each with name, value text, a control: slider, two values a switch, a few values a list of them | `params.get`, `params.set`, `params.text`, `params.gesture` | - | on the grid (ui/GRID.md, branch rust-grid): knob, slider, switch, dropdown, split buttons, range, value; cursor in two dimensions, press, dial; each step sent between a gesture begin and end | [x] |
| F5 | Search a parameter by name or module on any page | derived | - | keyboard (K1): back from the first tile reaches the search through the tabs and pages (or up from the first row), a press opens the on-screen keyboard, Cancel backs out the cursor, the search, then the screen. Touch: a tap on the Search button of the header row (it moved from a field above the pages: ui/TABS.md). Built on rust-app-grid (9ee154cb; the field only for a plugin of more than 24 parameters, as the reference) | [x] |
| F6 | The selected row/column of tiles is what the device's knobs play ("row:N" knobs); on the page, the screen knobs play the page by priority | `controllers.view` (screen page), `params.set` | - | the knob row (A9). Done on rust-grid: the screen page published by priority, withdrawn on leaving. Not done: `row:N` (no device profile has such knobs) | [ ] |
| F7 | Rich views by field roles: envelopes (ADSR by name, DX7 rates/levels, FM-X), key and velocity maps, FM operator views and algorithm diagrams, LFOs, filters (response drawn) | `params.*` | - | the same tiles are always reachable; the rich view is drawn above them, and the node or handle a tile sets is marked where the cursor is on it. Every view of the reference is built on rust-app-grid (src/views.rs, ui/GRID.md has the list with file:line): the envelope (fields and, as the reference, ADSR, DX7 and FM-X names) with its nodes dragged by touch (e05bff23), the LFO (02ffdb0c) and filter (967cd464) previews, the key and velocity map (2fdad232) and the FM operator view (ec760a14), each dragged by touch where the reference drags it. The FM operator view was then redesigned on the owner's request (2026-11-15) and no longer matches the reference (ParameterGrid.cpp ~731), by his decision: the algorithm horizontal (carriers at the right, an output line), next to one graph with a line per operator in its own color scaled by its level, over a strip of operator cells; ui/GRID.md "The FM operator view" says how. Lines are drawn unscaled from one set of points and measured in the rendered pixels (tests/views.rs). Unverified on a unit | [x] |
| F8 | Values follow the engine as they change (`params.changed`/`controls.changed`) | notifications | - | - | [x] |
| F9 | The plugin's own view | `window.info`, `window.show` | window | **disabled entry: not available in this shell yet (the only item allowed to stay unported)** | [x] |

## G. Controls (macros) and scene mapping (PartsView controlsPage)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| G1 | List of controls with a fader each (Performance's, and a part's own) | `graph.get` (`controls`), `controls.set` | - | cursor, press, dial | [x] src/controls.rs, slint/controls.slint: a row per control (name, fader, + Target, Remove) with a row under it per target, as the JUCE page lists `graph.get`'s `controls` (a part's own are in the same list: the JUCE page does not tell them apart); the fader's press then dial steps a fortieth (`controls.set`, live), `controls.changed` moves them; the cursor is two-dimensional as the Parts list's; how many rows a view holds and how many lines a target takes (one, two or three) follow the window |
| G2 | + Control, rename (double click), remove | `controls.add`, `controls.name`, `controls.remove` | - | actions; keyboard | [x] src/controls.rs: + Control ("Control N", N the count it makes), a press or a tap on the name asks the new one on the text keyboard (K1; a name that did not change sends nothing), Remove is immediate as the JUCE button; none of them is followed by `graph.commit`: the JUCE page sends none |
| G3 | Targets: + Target menu (plugin parameter, part level, part mute, part send, another control, morph between scenes), remove, relative, curve (linear, exponential, logarithmic, S-curve, fader dB, power) | `controls.targets` | - | menus as lists | [x] src/controls.rs, src/menu.rs, slint/menu.slint: the menus are lists over the page (menu.rs, shared with Routing): + Target with the JUCE submenus (a node's parameters from `params.list`, read-only ones left out; parts and buses; a part's sends; the other controls; a morph from one scene to another), the curve's six items with the target's ticked, Relative a toggle, Remove; each edit sends the control's whole list of targets again, as `setTargets` does, with a new target's whole range, linear, in every scene; the dB and power curves keep their numbers (-96..0, exponent 2 as the JUCE defaults) |
| G4 | Scenes a mapping works in ("Scenes..."), "Now..." (start/end = now) | `controls.targets` | - | list | [x] src/controls.rs: Scenes... is a ticked list of the scenes (a press switches the mapping there: `offIn`), "Scenes: all" when it works in every one; Now... is Start = now / End = now, ported from `setRangeToPresent` (parts.cpp): the point that puts the destination where it is now, every other target where it is (RFC 0011's summing rule), a stepped parameter rounded to a step, a relative one on a modulatable parameter left at the middle; a parameter's value is asked with `params.get`; disabled on a morph |
| G5 | A control's source (a plugin's lane) | `controls.source` | - | list | [x] NOTE: the JUCE Controls page does not set a source: it is `laneMenu` and `laneToControl` of the Routing page (I2), so it is built with Routing src/routing.rs: Connect lane to... (a plugin that has lanes): a submenu per lane (one: no submenu) with Control (the existing ones the lane does not drive, and New control) and every other plugin's parameters (not read only, not hidden); a new control is `controls.add` (named after the plugin), `controls.source`, `graph.commit`, `controls.set` 0.5; an existing one is `controls.source` and a commit; a parameter becomes a relative target of the lane's control (made when it has none): `controls.targets`, a commit, and `controls.set` 0.5 for a new one (`laneToControl`, `laneToParameter`). The Controls page itself sets no source, as the JUCE one. |
| G6 | "By scene" table of mappings | derived | - | toggle | [x] src/controls.rs, slint/controls.slint: By scene (shown when there are scenes) swaps the rows for a table, a row per mapping (control and target) and a column per scene, ticked where it works, a tick toggling it (`offIn`); the cursor goes over the ticks (dial in reading order, left knob along, right knob down); scenes beyond the width are paged (the cursor turns them) |
| G7 | Scene edit menu: store, rename, scope (plugins/wires on-off, mutes and solos, levels pans sends, parameters and controls, control mappings), transition (glide time, curve, launch while playing), untouched parameters | `scenes.store`, `scenes.name`, `scenes.scope`, `scenes.transition`, `scenes.untouched` | - | menu lists | [x] the scene setup is scene.rs (A4: Store, Rename, the six scope kinds, glide, curve, "while playing"); the untouched parameters (`scenes.untouched`) are the plugin actions' "Scene controls..." (D10), where the JUCE app has them. |

## H. Mixer (PartsView mixerPage)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| H1 | Strip per part and bus: name, fader with meter, pan, M, S | `parts.level`, `parts.pan`, `parts.solo`, `parts.meters` | - | cursor, press, dial | [x] src/mixer.rs, slint/mixer.slint: the JUCE mixer page is strips (not tiles), so strips are built: name, fader (the skew of the JUCE slider, -12 dB halfway up) beside a meter bar per channel, the level in dB, M, S, pan (bipolar, a double tap centers it), and a send slider per bus on parts (0 removes the send), faint when muted or silenced; the parts then the buses with a gap, the master pinned at the right. The cursor sits on one control: the dial goes strip by strip, control by control, the left knob along the strips, the right knob down the strip; a press on a fader, pan or send starts a change the dial makes (a fortieth of the travel a click), on M or S toggles. Strips and sends a view holds follow the window (paging buttons in the header for touch, Page keys, the cursor turns them). `parts.meters` every 50 ms while shown, `controls.changed` for levels, mutes and pans. Not in the JUCE page: nothing added; its mouse-only parts (double click) are the double tap |
| H2 | Master: level, pan | `master.set` | - | cursor, press, dial | [x] src/mixer.rs: the master is the last strip: a fader over 0 to 2 (the JUCE slider's skew) and a pan, `master.set` with the other kept, then `graph.commit`; no meter (the JUCE page has none either) |

## I. Routing (PartsView routingPage)

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| I1 | The graph by signal flow: columns, wires, collapsed parts (Expand/Collapse, Open part), meters on wires | `graph.get`, `wires.meters` | - | cursor along boxes (list of nodes) | [x] src/wires.rs, src/routing.rs, slint/routing.slint: the graph by signal flow, ported from PartsModel.cpp's `layoutRouting` (columns, a slot in each column a wire crosses, orderings to cross fewer wires, a place on the edge for each wire, frames of expanded parts with their plugins laid out inside, wires that go back below everything, control lines in right angles around the boxes), drawn from one set of points: the wires are Paths with `fit: preserve` in the pixels of the canvas, the boxes at the same points, the drawing fitted to the canvas (never above 1) so a wire meets its port at any window size and scale (pixel test at 1280x720, 800x480, 1.55, 2 and 700x700). Boxes: plugins, the ends, collapsed parts with chips and a chevron, frames with a chevron, control pills; what is chosen and what its wires reach stay bright. "Parts collapsed" is kept in routing-view.txt as the JUCE app keeps it; Expand and Collapse by the chevron or the panel; Meters asks `wires.meters` every 50 ms only while on and takes the wire's color from its level (green, yellow from -12 dB, red from -3). Not drawn as the JUCE does: a bus is a line per channel by offsetting one curve along its normals (JUCE nests strokes), a collapsed part's chips are cut at the box's edge (JUCE ends with an ellipsis) |
| I2 | Connect audio / notes to..., Connect lane to... (control or parameter) | `wires.connect`, `graph.map` | - | list menus | [x] src/routing.rs: Connect audio to... and Connect notes to... (menus of every node that takes the kind, the output once, by part: `graph.connect` and a commit); Connect lane to... is G5 |
| I3 | Disconnect; per-source mix (level, pan, M, S, On); wire channel | `wires.disconnect`, `wires.mix`, `wires.enable`, `wires.zones` | - | press then dial | [x] src/routing.rs: each input with sources has a title and a row per source: On (`wires.enable`), the level slider (the dial steps a fortieth; off for a lane's or split's wire), M and S (`wires.mix` with the level and pan kept), Disconnect for a wire drawn by hand (`wires.disconnect`), and for notes from the keys the wire channel, any or 1 to 16 (`wires.zones`); each followed by `graph.commit`. The JUCE rows have no pan control (only the mix keeps it), so none is built |
| I4 | + Plugin outside the parts; Remove it | `graph.add`, `graph.remove`, `graph.commit` | - | action | [x] src/routing.rs: + Plugin lists the scanner's plugins (not Split and Join), "Finding plugins..." while it looks, `graph.add` named uniquely then `graph.commit`; Remove on a plugin outside the parts is `graph.remove` with its wires reconnected, then a commit |
| I5 | Turn on/off a node; Open part | `graph.enable` | - | action | [x] src/routing.rs: Turn off and Turn on (`graph.enable`, a commit) and Open part (the Part screen, whose Back returns to Routing) |
| I6 | Disconnect lane (parameter map) | `graph.unmap` | - | action | [x] src/routing.rs: Disconnect on a lane's line: `graph.unmap` for the mapping and `controls.source` with no node for each control the lane drives, then a commit |

## J. Top bar and cross-cutting

| # | Item | Request | In-proc | No touch | Status |
|---|---|---|---|---|---|
| J1 | Undo / Redo with the edit's name on the button | `edit.undo`, `edit.redo`, `edit.history` | - | top bar actions | [x] src/shell.rs, slint/topbar.slint: on every screen of the app (Play, Part, Chain, Sounds, Parameters; the JUCE buttons are in the Edit view header only), "Undo <edit>" and "Redo <edit>" off when empty; edit.history asked on entering a screen and on each graph.changed. The bar's cursor (all of A14-A16's top bar actions): the third pane (`move:2`), Tab, or the dial moving up out of the first row of Play, Part, Chain and Sounds (not the parameter grid: it needs a hook in grid.rs); Confirm does it, past the last stop or Cancel gives the cursor back |
| J2 | The refusal of an edit said on the screen | error replies | - | - | [x] |
| J3 | The shell opens the Performance app everywhere (Home tile, Setlist's Play, Play's part press), the handover entries and "Oscairn app" rows for this app gone | - | - | - | [x] nothing of this app hands the display over now: Play's footer button went with A14 (the Part screen's row went with C19); tests.rs asserts Edit leaves no handover |
| J4 | Recording indicator (owner, 2026-11-15): a take running and its time, on every screen's top bar, once (Play has no mark of its own: the reference draws it in the Launcher's RecordButtons over every app, and PlayView.cpp has none): a filled dot, "REC m:ss" and "disk slow" when the writer is behind, in the danger color. Learned as the JUCE RecordButtons learn it (`Engine::takes`): `record.takes` polled every 250 ms (here with the `status` notifications, so nothing wakes at rest); the first take `waiting` or `recording`, its frames over its sample rate. Not built: the Record and Capture buttons themselves (the hardware REC button starts and ends a take in the engine) | `record.takes` | - | shown only | [x] |

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
it (F4 redone; F7 partly: envelopes). The Play knob row and banks are on grid tiles (A9-A11, done on rust-app-knobs). Not done:
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
- Pages and tabs are not the reference's lists of buttons (2026-11-15, the owner: "Plugin uis can have tabs too,
  like the modx does, they just need to be clear, and we need a consistent themeable style for tabs across the
  whole ui"; "Yes plugins should be able to name and order their tabs"): one themed tab family on the parameter
  pages (two levels) and Play's Performance / part switch, the search a header button, a page opening on a press and
  not by the cursor walking off the tiles' end; a recorded, approved difference from ParameterGrid.cpp `refresh`
  (ui/TABS.md).
- The FM operator view is not the reference's (2026-11-15, the owner): the algorithm drawn horizontally, next to one graph
  with a line per operator in its own color, dots along the lines where a plugin can say where a note is (not yet);
  a recorded, approved difference from ParameterGrid.cpp ~731 (ui/GRID.md "The FM operator view").
