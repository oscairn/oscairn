# The grid framework: design notes and checklist

Notes from the sources, read once (2026-11-15), so nothing has to be read twice. The shell's grid
is `ui/src/grid.rs` (layout, navigation, the envelope) and `ui/slint/grid.slint` (the widgets);
the parameter pages on it are `ui/src/params.rs`.

## Where the data comes from

- Everything is in `params.list` (docs/control-protocol.md, the `params.list` row): per parameter
  `id`, `name`, `minimum`, `maximum`, `defaultValue`, `stepped`, `flags`, `module` (CLAP module
  path), and from the plugin's `oscairn.ui` hints (kept by the host only where they fit):
  `widget` (`auto`, `slider`, `knob`, `buttons`, `switch`, `list`, `range`), `priority` (1 first,
  0 none), `group`, `rangeHigh`, `role`, and for rich views `view`, `field` (`attack`, `level0`,
  `rate1`, `keyLow`, `breakpoint`, `algorithm`, `operatorLevel`...), `index`, `seconds` (a time or
  rate field's stage length at evenly spaced values), `algorithms` (FM routing per value). No
  engine code is needed. Values: `params.get`; text: `params.text`; edits: `params.set` inside
  `params.gesture` begin/end; pushes: `params.changed`.
- The hint struct (core/clap-ext/include/oscairn-clap/ui.h, same as oscairn-clap's; docs/ui.md
  there) has **no spans and no layout coordinates**: the layout is the host's, from the rules below.
  "Split button" in the owner's words is the `buttons` widget (2 to 8 values side by side, one
  stop per value); a range (`range` + `rangeHigh`) is one two-sided slider.
- RFC 0042 (vertical sliders and organ drawbars) is proposed and unbuilt: its widgets are for
  Play's slider row (a drawbar fills from the top: a reversed set), not selected by any
  `params.list` field yet (draft 6's `reversed` is not defined). The shell has both widgets.
  Checked for the parameter pages (2026-11-15): the reference does not select them there either.
  `ParameterGrid::build` chooses among buttons, switch, list, slider (horizontal), knob and range
  only (ParameterGrid.cpp L1490-1507, `Control`), `enum oscairn_widget` (ui.h L72-80) has no
  vertical or drawbar value, and RFC 0042 says "no drawbar view or drawbar roles: a drawbar is a
  level, and the screen draws the set the same way". So there is nothing in the plugin's data to
  select them by, and inventing a hint would not be matching the reference: they stay widgets for
  Play's slider row until a hint (draft 6) is defined.

## Layout rules (hosts/standalone/ParameterGrid.cpp `build`, L1287-1588; RFC 0019 L420-490)

- Pages: one per top-level module ("Main" when none), numbered pages together; a plugin of more
  than 24 parameters without paths gets modules from leading name words; hidden parameters left
  out (params.rs already: `editor_parameters`, `editor_pages`, `tabs_for`).
- Tabs: a page of more than 24 parameters gets a tab per second-level module (two or more).
  Pages and tabs are one structure, src/tabs.rs `Model`, filled from the module paths or from what a
  plugin declares, and drawn by the shell's one tab family: ui/TABS.md (an owner-approved difference from
  the reference's lists of buttons).
- Headings: within a page (or tab) the rest of the module path is a heading ("A › B"), 28 high
  (30 with its gap); groups in first-appearance order.
- A range's high parameter has no tile (its low end's tile draws both).
- Order: priority (numbered first, then the plugin's order), stable sort.
- Columns: `clamp(width / 210, 2, 6)`; gap 6 (sectionGap); tile width `(width - gap*(cols-1))/cols`
  (the numbers at the devices' screens, Rust and reference: "Tile density" below).
- Tile height: 84 in touch mode (64 with a mouse); a page with any knob gets +36 (+30).
- Rich views first, across the full width, each a panel, in this order: an FM operator view where its
  algorithm parameter is (the top pane of a master and detail: it and the detail fill the area, see below), an envelope per (view, index) (180), a key map per
  (view, index) (180); then among parameters in no view, by name: a filter (160), an LFO (140), an
  envelope (180).
- Runs: a group (hint `group`), then each described envelope's parameters in linked order, then a
  name-recognized envelope, LFO, filter, are runs; others single tiles. A run that does not fit
  what is left of a row starts the next row, the tile before it widening to the row's end (when
  it is not itself in a run); a run wider than the row goes on in touching rows (no gap), its
  last tile filling the row. A run's tiles touch (width + gap) and are drawn as one card.
- Tile inside: name (small, bold, muted) left and the value text right (up to 90 wide or half)
  on an 18-high top line; the control below, 44 high in touch mode (28 mouse); a knob a square
  up to 64; read-only: value only.
- Control choice: hint knob, slider, switch, list (up to 64 values, else slider), buttons (up to
  8 values, up to 64 a list, else slider), range (slider when `rangeHigh` is not a parameter);
  `auto`: 2 to 4 values buttons, unless two values read Off/On (or 0/1): a switch; 2 values a
  switch; up to 64 a list; else a slider. A slider over a symmetric range with default 0 is
  bipolar (fills from the center).
- Value shown: the plugin's text (`params.text`), not shown on lists, switches and buttons
  (their control says it); a switch's button text is the value text ("1" On, "0" Off).
- Physical knobs (RFC 0019 step 6, RFC 0011, core/control/src/physical_controls.cpp L223-237):
  the editor publishes the shown tiles by priority (`controllers.view` `screen` {name, params as
  assignment targets}); a device's knob group with role `screen` (FANTOM-07: E1 to E6) plays
  its first N, engine-side. On-screen panels' `row:N` knobs play the selected line of tiles
  (a tap selects a line; knobs beside the screen take a column) - the JUCE Panel only; no
  device profile has `row:N` knobs, so the shell does not draw that strip.
- Navigation in the JUCE app: touch/mouse only. The panel map's E1 and E6 navigate panes 0 and 1
  only while `controllers.view` says lists show; on a grid page they play E1/E6's parameters.

## Envelope (L113-192, L537-560, L63-111)

- Named stages (attack and release needed): delay (to 0), attack (to full, or to sustain without
  decay), hold, decay (to sustain), held, release to 0. Points: segment k takes TIME_k or RATE_k to
  reach LEVEL_k; without a level it holds; from LEVEL_0, else the last level; the second-to-last
  point holds (sustain).
- Drawn: area inset 14,10 and 18 at the top; sustain hold 14% of the width; each timed segment
  the same room, a least width (20 touch) plus its time across the rest; time on a log axis
  `log1p(s/0.05)/log1p(longest/0.05)` when every time field has `seconds`, else the normalized
  value (a rate reversed). Levels by the parameter's normalized value.

## The reference's views (ParameterGrid.cpp; every graph or compound view it has)

Panel heights in touch mode (build L1352-1397): FM operators 300, envelope 180, key map 180, filter
160, LFO 140. Order in a group: FM operators, described envelopes, key maps, then (parameters in no
view, by name) filter, LFO, named envelope. Shapes recognized: `envelopeOf` L113-192, `mapOf`
L194-222, `fmOf` L224-247, `algorithmLayout` L249-285, `lfoOf`/`waveNamed` L287-310,
`filterOf`/`filterNamed` L312-331. Shipped plugins in plugins/: basic-synth (an envelope by `view`
fields), lfo (an LFO by name: Shape + Rate + Phase); none declares a filter, key map or FM view (those
come with the engines of other repos and third-party plugins), so the order built is LFO, filter,
key map, FM operators.

| View | Reference | Drag | Rust | Status |
|---|---|---|---|---|
| Envelope | `Envelope` L478-570 | a node: time across, level up | views.rs `draw_envelope`, grid.rs `envelope_layout`, `envelope_of_names` (ADSR, DX7 and FM-X names) | done |
| Filter response | `Filter` L359-415 | none (a preview) | views.rs `draw_filter`, `filter_of`, `filter_named`, `filter_db`; pixel test tests/views.rs | done |
| LFO waveform | `Lfo` L419-473 | none (a preview) | views.rs `draw_lfo`, `lfo_of`, `wave_named`; pixel test tests/views.rs | done |
| Key / velocity map | `KeyMap` L594-724 | zone edges, breakpoint, depths, velocity | views.rs `draw_map`, `map_of`, `map_drag`; tests/views.rs | done |
| FM operators | `Operators` L731-916 | algorithm, level bars, ratio | src/fm.rs (the owner's own design, below: not the reference's) with views.rs `View::Fm`; tests/fm*.rs | done, differing from the reference by the owner's decision (2026-11-15); since 2026-11-16 the top pane of a master and detail ("Master and detail"); the reference's two guards stay (operators past 8 are skipped, a feedback with no target operator is not drawn) |

## The FM operator view: the owner's design (2026-11-15), a difference from the reference

The owner looked at the reference's layout (the algorithm as a vertical stack in a box at the left, a column per
operator with a level bar, a ratio box and a tiny envelope) and asked: "fm algos should be horizontal, next to a
graph with different color lines for each operator to show their config. not sure if theres a way to show dots
moving along those lines as a note is played, some synth software does that." So this view is Oscairn's own
(src/fm.rs); the JUCE `Operators` (ParameterGrid.cpp ~731) is not matched, on purpose. Since the master and detail (below) the panel is the top pane of a block that
fills the area: the diagram, the graph and a list of the operators side by side.

- Flow convention, kept for every algorithm: signal runs left to right. An operator's column is its distance from
  the output: the carriers in the last column, what modulates a carrier one column left, and so on; the carriers'
  wires meet an output line at the right edge (labeled "Out"). Each wire has the color of the operator it comes
  from and ends on the target's left edge (an arrow), several into one box land at spread points on its edge;
  an operator feeding two shows one wire that forks. Wires are orthogonal and keep to gutters between columns.
  A wire that skips a column passes through a point reserved in it (`Flow::nodes` with no operator), so no wire
  runs through a box. Rows come from sweeps that put each node at the mean row of what it is wired to
  (`algorithm_layout`), which gives no crossings for a stack four deep, two stacks sharing a modulator, a wire that
  skips a column, and the eight-operator fixture. Feedback (and modulation that runs backward in a cycle) is a loop
  drawn out of the top of a box into the gap above its row, up a box's side to a lane above every row, across, and
  down into the box it feeds with an arrow: a self feedback is a small loop above its own box. Loops cross wires
  but never a box.
- Colors: a fixed palette of eight per theme (THEME.md "Operator colors"), operator n always the same color in a
  theme. The operator's box, its cell's number, level bar, the wires it feeds and its line in the graph use it;
  every box and cell is numbered and every line end in the graph carries the number as a tag, so color is never the
  only mark. An operator at level 0 (the data has no on/off field) is drawn dimmed, its line flat at the plot's foot.
- Graph: one plot of every operator's amplitude envelope on common axes (stage by stage left to right on one time
  scale, level up), each line scaled by the operator's output level (`operatorLevel`; an operator without one is
  drawn at full height: the normalized form, only where the data gives no level). The envelope comes from the same
  fields as the envelope view (`grid::envelope_layout_in`, the one layout `grid::envelope_layout` uses): named
  ADSR stages or rates and levels. Where every time field has `seconds` the stage widths share one scale across
  operators (the longest of all), else each uses its own normalized axis. The selected operator's line is thicker,
  on top, and the only one with handles (touch drags a node: its time across, its level up, for that operator
  only); the others are thin.
- List (the strip of the first version is gone: it was a row of cells under the diagram and the graph): one line per
  operator at the right of the overview, as many lines as the plugin has operators (none for a missing one), each its
  number chip in the operator's color, its level (the plugin's text over a thin bar in the operator's color) and its ratio
  (edged in the accent while the fixed switch is on, with the switch's text before the ratio's when the box is wide enough
  for both at 70% type). Fine, detune and every other parameter of an operator are in the detail (below), not here.
- Choosing an operator, a master and detail: see "Master and detail". The selected operator is told in four places at
  once: the ring round its box in the diagram, its thick line (the only one with handles) in the graph, its raised line
  with the text-colored edge in the list, and the detail's heading in its color. By touch a tap on its box, its line or its
  list line chooses it, the level bar and ratio box of a line are dragged as before, a drag across the diagram steps the
  algorithm and a tap on its left or right half steps it back or on; a tap on the algorithm's or feedback's name above the
  diagram steps it by its halves too.
- Dots (`Screen::set_positions`, `fm::Along`): the graph draws a dot (a halo and a dot in the operator's color)
  at a position along each operator's line when it is given: a fraction of the line's length or of one segment.
  Nothing calls it yet and no plugin gives positions (the layout extension has no such data;
  notes/ui-live-positions-proposal.md, outside the repository, proposes one). The view is whole without dots;
  with none given nothing runs: no timer, no repaint.

## Master and detail (owner, 2026-11-16: "an interface design that would keep all operators on the same page. Maybe a top
pane with the lines and the list of 8, and a pane under that with controls for the selected op")

The FM operators view is the full form of a master and detail on one page (src/grid.rs `layout_with`, `Master`, `Detail`;
src/fm.rs the overview; src/params.rs the selection and the cursor). Numbered siblings of one structure are not pages or
tabs: the rule that folds a page of tabs that are the indexes of one view into it is in TABS.md "Tabs that are indexes".

- **The block.** Where a page has a view with an overview (`grid::master_view`: a view with an `algorithm`), its parameters
  with an index of 1 to 8 are not tiles of the page: they are the detail. The block fills the area (the page's view height)
  and is, top to bottom: the overview (panel 0, the view's own panel), the detail's heading (panel 1: a bar in the selected
  operator's color with "Operator N" or the folded tab's name, the rows it shows and two arrows that page them), and the
  selected operator's tiles. Parameters of the page that are in no view, and the view's with an index past 8, stay ordinary
  tiles under the block, by the rules above (the page scrolls to them).
- **Overview** (always all operators): at the left the horizontal algorithm diagram, then the shared graph, then the list.
  Under the page's tabs the view's own parameters (index 0: algorithm, feedback, any other) are named in a line over the
  diagram ("Algorithm 3", "Feedback 5"), and are cursor stops and touch targets there; they are not tiles anywhere.
- **Detail.** Every parameter that carries the view and the selected operator's index, in the plugin's priority order
  (numbered first, then the plugin's own), as the grid's own tiles (knob, slider, switch, dropdown, split buttons at their
  usual size: a tile is 84 high, 120 with a knob on the page, a column at least 210 wide), row by row at the page's
  column count. Switching operator keeps the row.
- **Split.** The detail gets as many full rows as leave the overview its preferred height (14 lines of the small type, at
  least 1.6 lines for each operator's list line: `fm::preferred_height`), at least one and at most what the operator
  with most parameters needs; the overview gets the rest, never less than `fm::least_height` (a list line 1.1 lines of type
  high). Only when even one row does not leave the least does the block go past the area and the page scroll. Measured
  (tests/fm_master.rs `the_overview_and_the_detail_share_the_area_at_the_three_screens`, dark theme, tiles with knobs;
  area width x height, overview height, then the detail is the heading 26 and the row 120 and the gaps):

  | Screen | pages | area | overview | detail rows in view |
  |---|---|---|---|---|
  | 826 x 465 (1280 x 720 at 1.55) | none | 814 x 405 | 259 | 1 (3 tiles) |
  | | rail at the side | 743 x 405 | 259 | 1 (3 tiles) |
  | | strip over | 814 x 357 | 211 | 1 (3 tiles) |
  | 1280 x 720 at 1 | none | 1268 x 660 | 274 (8 and 6 operators), 394 (4) | up to 3 (6 tiles) |
  | | rail at the side | 1197 x 660 | 274, 394 | up to 3 |
  | | strip over | 1268 x 612 | 226, 346 | up to 3 |
  | 800 x 480 at 1 | none | 788 x 420 | 274 | 1 (3 tiles) |
  | | rail at the side | 717 x 420 | 274 | 1 |
  | | strip over | 788 x 372 | 226 | 1 |

  The overview is that tall because one row of knob tiles is 120 and the heading 26: with the preferred height at 8
  operators (220 at the small type's 13) a second row would cost more than the overview can give up at 405. Where an
  operator has more parameters than the rows in view, the rest are paged by row (below).
- **Rows.** One row at a time. The heading's arrows (touch) and the dial or an arrow key (without touch) move the first
  row in view by one; past the last tile in view the dial goes to the start of the next row, and back from the first to the
  end of the row before; down from the last row in view or up from the first with the arrow keys keeps the column.
- **Without touch.** The dial walks, in this order: the page rail (when there is one), the tabs of the page when it has
  tabs, the overview's row (the view's own parameters, then operators 1 to N), then the detail's tiles (and the page's
  ordinary tiles after them). Which operator to look at is not an edit: the detail and the knobs follow the cursor along the
  operators at once (a relayout of the detail and, when it changed, one `controllers.view`; no parameter is set). A view's own
  parameter is reached the same way but nothing changes until a press gives it the dial (a gesture begins) and a press or
  Cancel ends it. A press on an operator takes the cursor to the detail tile it last left (else the first). Quick operator
  step: from the detail the cursor up (an arrow key, CURSOR UP on a unit) goes to the operator's line, the dial steps the
  operator, the cursor down returns to the same tile, so one parameter is swept across the operators; the page keys
  (Page Up, Page Down) step the operator at once, keeping the cursor on its tile and the row, and turn the page only
  past either end.
- **The knobs under the screen** play the detail's whole list for the selected operator in the order of the detail (not only
  the tiles in view), then the view's own parameters, then the page's other tiles by priority: `controllers.view` `screen` is
  published again when the selection changes. Without a row of eight in view the knobs still play eight: the first row of
  tiles is a prefix of what they play.
- **What an element overview would plug into.** The mechanism is shared and not FM's: `grid::Master` (the area, the
  selected index, the first row) in, `grid::Detail` out (the view, the index, its parameters, the rows), `Screen::operator`
  (the selection), `select`, `page_rows`, the overview's cursor stops (`Strip::Overview`) and a page's folded tabs
  (`tabs::Indexed`). A key map's zones or a sampler's elements would add: a view with an overview panel for the
  zones or elements (drawn as panel 0 in place of the FM's), `grid::master_view` choosing it (today: has an `algorithm`),
  the entries of its overview row (today the view's own parameters and the operators) and the heading's color and noun (today
  the operator's color and "Operator"). Nothing else changes; no such overview is built now.

## Checklist (shell)

Control kinds:
- [x] tile with spans (a column span; the last of a row widens), headings, linked runs as one card
- [x] knob (drag up/down; dial)
- [x] slider horizontal (drag; bipolar fill)
- [x] slider vertical (widget only: no data selects it, nor does the reference's; Play's mixer faders not yet moved onto it)
- [x] drawbar (vertical, fills from the top; widget only: no data selects it, nor does the reference's)
- [x] switch (tap; press)
- [x] list / dropdown (opens a list; dial moves, press chooses, cancel closes)
- [x] buttons / split button (one stop per value; dial steps the stops)
- [x] range (two-sided; dial moves the low end, a second press the high end)
- [x] value display: read-only and every tile's plugin text
- [x] envelope view (named stages and points)
- [x] key or velocity map view (used by the FM and sample engines; touch: zone edges, the breakpoint, the depths and the velocity line are dragged)
- [x] FM operator view with algorithm diagram (FM, VA's and PCM's FM-style views; touch: drag across the routing or tap
      its halves for the algorithm, a level bar, a ratio up or down; `algorithms` of `params.list` parsed)
- [x] filter response view (recognized by name: Cutoff + Resonance + Type; a preview, its tiles edit it)
- [x] LFO waveform view (recognized by name: Wave/Shape + Rate/Speed; a preview, its tiles edit it)
- [x] dragging an envelope's points (touch: a node within 24 px is taken, its time across and its level up
      and down are set inside gestures; the reference's non-touch editing is the stages' tiles, as here,
      and the node the cursor's tile sets is marked; src/views.rs, tests/views.rs)

Layout rules:
- [x] pages, tabs, headings, priority order, range high hidden
- [x] columns 2 to 6 from the width, tile 84 (+36 with knobs), gap 6
- [x] runs: wrap to the next row, widen the tile before, touching rows
- [x] panels for views above the tiles, full width
- [x] cursor in two dimensions, press selects, dial changes, cancel backs out
- [x] screen page published (`controllers.view` screen) with the shown tiles by priority (the engine's
      screen knobs play the first N); withdrawn when the page hides. No knob badges drawn (the JUCE
      grid draws none for screen knobs)
- [x] search (F5): the field above the page list for a plugin of more than 24 parameters; the parameters of every page
      whose name or module has the text (any case, trimmed), under their module's whole path, no tabs, no page
      marked; a page chosen or turned ends it. Touch: a tap on the field opens the on-screen keyboard (K1). Without
      touch: back from the first tile or up from the first row is the field, a press opens the keyboard, Cancel backs
      out the field, the cursor, the search (the reference's Escape), then the screen

## State (2026-11-15, branch rust-grid)

Built: grid.rs (Param from params.list, control choice, layout port of ParameterGrid::build,
two-dimensional cursor moves, knob arcs, envelopes from fields and ADSR names, the curve);
slint/grid.slint (GridState, every widget, the page list, tabs, the dropdown overlay); params.rs
rebuilt on it (the old rows are gone); tests/grid.rs (5 tests) and grid.rs unit tests (6). The
upright slider and drawbar are drawn in the picture test by adding tiles to the model by hand: no
data path selects them yet.

Not built yet (in this order): Play's knob row and banks on grid tiles (A9-A11: PlayView.cpp,
`controllers.view`, `assignments.get/set`), Play's mixer strips' faders as the upright slider
(H). The key map, FM operator, filter and LFO views, dragging envelope points, the search (F5) and
the DX7/FM-X envelope names are built (branch rust-app-grid; the views table above). Also: at 700
logical px the grid has 2 columns. Nothing failed to fit at 800x480 (2 columns, 3 rows of tiles
in view, scrolling for the rest); the scene tile number overlap at 1.55 on Play is not fixed.

## Tile density at the devices' screens (checked 2026-11-15, tests/grid.rs `tile_density_...`)

The rules are the reference's (build in touch mode): columns `clamp(width / 210, 2, 6)`, a tile 84 high
(120 with a knob), gap 6, a heading 30, panels 140 to 300 (the views table). Nothing in the Rust grid
differed, so nothing was changed: the same logical pixels, since the shell and the JUCE app both scale
the screen by the profile's `Screen::scale()` (FANTOM-07 1280x720, diagonal 140: 1.55, so 826 x 465
logical). What differs is the room. In the JUCE app the grid is the Part page's panel (PartsView.cpp
750-788: 16 in from the sides, under the header 56, the chain strip 96 at least, the action row 44, gaps
12 + 12 + 8, above a 28 footer, under the 48 high top bar; its page list 200 wide, 8 apart, its scroll
bar 8: derived by reading, not measured on a running app); the Rust grid is a screen of its own with
Back and the title (48), the margins 6, and, until the tab work (ui/TABS.md), the page list 180 wide; the pages are
now a strip over the tiles on a window under 560 logical px tall (the keyboard's), so the numbers below are the
tiles' area with pages and no tabs, and a page of tabs takes 48 more of the height:

| FANTOM-07 (1.55) | Rust area | columns | tiles in view | JUCE panel (derived) | columns | tiles in view |
|---|---|---|---|---|---|---|
| knobs, two pages | 814 x 309 (was 628 x 357) | 3 (267 x 120) (was 2) | 6 (was 4) | 578 x 129 | 2 | 2 |
| knobs, one page | 814 x 357 | 3 (267 x 120) | 6 | 786 x 129 | 3 | 3 |
| sliders, one page | 814 x 357 | 3 (267 x 84) | 12 | 786 x 129 | 3 | 3 |

The one page rows are as before (6 and 12); two pages now have the width, 6 tiles in view instead of 4, for a
shorter area (tests/grid.rs pins 814 x 309). "Six tiles a page, two columns, small
knobs in large tiles" is the reference's too, and its knobs are the smaller: a 64 square (Theme.cpp
drawRotarySlider: the arc 56 across at its middle, the disc 39), ours a 76 box (the arc 61, the disc 47), in
a tile as wide as the column; at 628 the grid is 2 px short of a third column
(630), as the reference is at its width. The test pins the Rust numbers at the FANTOM-07, MODX-M and
Kick screens and that they are never fewer than the reference's room gives. One px differs inside a
tile: the control starts 29 below the top (8 + the small type's 13 + 8) where the JUCE tile has 30
(8 + a fixed 18 + 4); the small type scales with the theme's font scale, the 18 does not.

## Universal UI debts (owner: one UI for the keyboard, resizable desktop windows, later a DAW plugin)

The grid code itself has no statics and no window knowledge: it lays out for the width Slint
reports (`GridState.view-width`, `resized`), in the theme's logical pixels. What it sits on does not
yet meet the rule; not refactored now, to be scheduled:
- ui/src/shell.rs `thread_local! APP` and `with_app`: one shell per thread; every Slint callback
  (GridState's included) reaches the screens through it.
- ui/src/shell.rs owns the engine client and the event loop's timers; screens get `&Shell` (the
  whole window component) rather than their own component and a connection handed in.
- Slint globals (GridState, PartState...) are per component instance, so several windows each need
  their own Shell instance; screens are not yet separable components.
- The theme is applied to the window's Theme global (theme.rs) and the profile read in main.rs:
  passed in at start, but not per screen.
- ui/src/tests.rs sets OSCAIRN_* environment variables process-wide (tests only).

## Debts found while theming

- Waveform icons: the plugin data carries no per-value icon hint (`ui.h` and `params.list` have `widget`, `group`,
  `view`... and nothing for icons), and the JUCE grid draws none. `grid::wave_icon` recognizes well-known waveform
  names (Saw, Sawtooth, Square, Sqr, Pulse, Sine, Sin, Triangle, Tri, Noise) on split buttons and dropdowns and
  `grid.slint`'s `WaveIcon` draws them as paths. Draft 6 of the layout extension has the proper source (`value_icon`,
  `enum oscairn_icon`): `grid::icon` takes a declared icon first (provisional `valueIcons` of `params.list`, ui/TABS.md)
  and falls back to the name rule, which goes when the engine forwards the declaration.
- The switch tile: the JUCE grid draws a toggle `TextButton` at most 110 wide, left in the control row
  (ParameterGrid.cpp:1544). The Rust grid fills the control row's width instead, on the owner's remark that a pill
  floating in the tile looked wrong; a divergence from the reference to settle in both UIs.
