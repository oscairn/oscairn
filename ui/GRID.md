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
| FM operators | `Operators` L731-916 | algorithm, level bars, ratio | src/fm.rs (the owner's own design, below: not the reference's) with views.rs `View::Fm`, `View::Stages`; tests/fm*.rs | done, differing from the reference by the owner's decision (2026-11-15); since 2026-11-16 the top pane of a master and detail ("Master and detail"), with an operator table ("The operator table") over a dense detail of named envelope stages and compact controls ("The detail"); the reference's two guards stay (operators past 8 are skipped, a feedback with no target operator is not drawn) |

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
- Table (the list of the first version, a line of number, level and ratio, is gone): one row for each operator in the middle of
  the overview, its columns and cells in "The operator table".
- Choosing an operator, a master and detail: see "Master and detail". The selected operator is told in four places at
  once: the ring round its box in the diagram, its thick line (the only one with handles) in the graph, its raised row
  with the text-colored edge in the table, and the detail's heading in its color. By touch a tap on its box, its line or its
  table row chooses it, the level bar and the number cells of a row are dragged as before, a drag across the diagram steps the
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
  operator's color with "Operator N" or the folded tab's name, a previous and a next arrow, and the rows it shows with two
  arrows that page them), the operator's envelope (panel 2: the graph and the labels of the Time and Level rows) and, over
  panel 2 and under it, the operator's parameters as compact tiles: the stages' controls in two rows, then the others. Parameters of the page that are in no view, and the view's with an index past 8, stay ordinary
  tiles under the block, by the rules above (the page scrolls to them).
- **Overview** (always all operators): three panes across, the graph at the left, the operator table in the middle and the
  algorithm diagram at the right (the owner's arrangement). Under the page's tabs the view's own parameters (index 0: algorithm, feedback, any other) are named in a line over
  the diagram ("Algorithm 3", "Feedback 5"), and are cursor stops and touch targets there; they are not tiles anywhere. The
  table's width is its columns' (scaled 0.8 to 1); what is left goes to the graph (45%) and the diagram (55%, never less
  than nine lines of type or its heads); a graph that would not get nine lines of type across is left out, so it is the
  diagram that shrinks, never the table's rows.
- **Detail** (the owner, 2026-11-16: "we can def fit more info on the screen than just the grid system, but we do want to
  keep it visually consistent with the rest of the design language"): top to bottom a heading bar in the selected operator's
  color (a previous and a next arrow, each a touch target wide, then "Operator N", and at the right "Row 1 of 3" with two
  arrows when the other controls have more rows than are in view), the operator's envelope (a graph over a **Time** row and
  a **Level** row of compact controls, "The detail" below) and the operator's other parameters as compact controls, up to
  eight to a row, in the plugin's priority order (compact: "Compact control"). Every parameter carrying the view and the
  selected operator's index is there once; those of the envelope's stages are in its rows, not again below.
- **Split.** One rule: the table's rows keep at least 1.3 lines of the small type; the detail's controls keep their size;
  the detail's graph appears when at least three lines of type are left for it; then the table's rows grow up to 1.7 lines
  of type, then the graph (to nine, then without limit), then the table's again (to 2.0). More rows of other controls
  than one are shown only while the table keeps 1.7 lines a row and the graph its four. Only when the table's least and the
  detail's two rows and one row do not fit does the block go past the area and the page scroll. Measured by
  tests/fm_table.rs `the_screens_hold_all_rows_the_stage_rows_and_a_row_of_controls` (dark theme, the engine-like fixture:
  area logical px, overview height with the table's row height in lines of type at 13 px, the detail's graph, a compact
  control 44 high; the stage rows are two and the controls one row of eight in every case below):

  | Screen | pages | area | 8 operators: overview (row), graph | 6: overview (row), graph | 4: overview (row), graph |
  |---|---|---|---|---|---|
  | 826 x 465 (1280 x 720 at 1.55) | none | 814 x 405 | 229 (23.0), none | 173 (21.9), 148 | 143 (26.0), 178 |
  | | rail at the side | 743 x 405 | 229 (23.0), none | 173 (21.9), 148 | 143 (26.0), 178 |
  | | strip over | 814 x 357 | 181 (17.0), none | 181 (23.3), none | 125 (21.9), 148 |
  | 1280 x 720 at 1 | none | 1268 x 660 | 253 (26.0), 227 | 197 (26.0), 283 | 143 (26.0), 337 |
  | | rail at the side | 1197 x 660 | 253 (26.0), 227 | 197 (26.0), 283 | 143 (26.0), 337 |
  | | strip over | 1268 x 612 | 222 (22.1), 210 | 197 (26.0), 235 | 143 (26.0), 289 |
  | 800 x 480 at 1 | none | 788 x 420 | 188 (17.9), 148 | 174 (22.1), 162 | 143 (26.0), 193 |
  | | rail at the side | 717 x 420 | 188 (17.9), 148 | 174 (22.1), 162 | 143 (26.0), 193 |
  | | strip over | 788 x 372 | 196 (18.9), none | 196 (25.9), none | 140 (25.8), 148 |

  So at the keyboard's screen with 8 operators every row of the table shows without scrolling (17.0 to 23.0 logical px a
  row, 1.3 to 1.8 lines of type: the least is 1.3), the Time and Level rows and the row of eight controls show, and the
  detail's own graph is left out (the shared graph at the top, with the selected operator's line thick and its handles, is the
  graph then); with 6 or 4 operators, or on a larger screen, the room that is freed goes to the detail's graph. With pages over
  the 357 high area the graph is left out for 6 as well. The diagram is the pane that shrinks (an 8-operator flow at the
  keyboard's screen is about 200 logical px across).
- **Rows.** The other controls page one row at a time (the stage rows do not). The heading's arrows (touch) and the dial or an
  arrow key (without touch) move the first row in view by one; past the last control in view the dial goes to the start of
  the next row, and back from the first to the end of the row before; down from the last row in view or up from the first
  with the arrow keys keeps the column.
- **Without touch.** The unit has a dial (and INC, DEC, CURSOR UP and CURSOR DOWN, which all step it by one: the device's panel map, devices/*/panels), two more encoders for panes 0 and 1,
  ENTER (confirm) and EXIT (cancel); it has no left or right key. So the dial walks in reading order: the page rail (when
  there is one), the tabs of the page when it has tabs, the overview's row (the view's own parameters, then the table's cells
  row by row, left to right in each row), then the detail's controls (the Time row, the Level row, the other controls), then the page's
  ordinary tiles. Pane 0's encoder (and a left or right arrow key) moves along a table row and pane 1's (and an up or down
  arrow key) down the rows in the same column; a keyboard's arrows do all four. Which operator to look at is not an edit: the
  detail and the knobs follow the cursor along the rows at once (a relayout of the detail and, when it changed, one
  `controllers.view`; no parameter is set). A view's own parameter or a cell is reached the same way but nothing changes until
  ENTER: it switches a mark (mute, solo, mode) or gives the cell's value the dial (a gesture begins; the dial changes it, ENTER or
  EXIT ends it). A cell that cannot be edited does nothing and its reason is in the table's heading line while the cursor is on it.
  Quick operator step: the dial or pane 1 down the rows, the page keys (Page Up, Page Down) step the operator at once from
  anywhere, keeping the cursor on its control and the row, and turn the page only past either end.
- **The knobs under the screen** play the selected row of the table, its columns left to right (level, coarse, fine, the
  ratio, detune: the switched cells, mute, solo and mode, are not knob material), then the operator's other parameters in the
  detail's order (the stages' controls first, then the others; not only those in view), then the view's own parameters, then
  the page's other tiles by priority: `controllers.view` `screen` is published again when the selection changes. A player selects
  an operator and has its level, coarse, fine and detune under the first four knobs at once.

## The operator table (owner, 2026-11-16: "i do want to use the table")

One row for each operator (src/fm.rs `Column`, `Row`, `geometry`, `draw`), the selected row lit (raised, with the text
color's edge): the operator's number chip in its color (dimmed while it is silent), **M** and **S**, the **level** as a bar in
the operator's color with its value, **coarse**, **fine**, the **ratio or fixed frequency** as text, **detune**. Columns appear
only for fields the plugin's data has (`FmShape::columns`): level for `operatorLevel`; coarse for `ratio` and fine for `fine`
where the operator has a `fine` of its own, with a **mode** column (`fixed`: Ratio or Fixed) beside them; with no `fine` the
`ratio` field is one column of text (with "Hz" added where the operator is fixed and the plugin's text has no unit; the cell
is edged in the accent then); detune for `detune`. Column headings once, in the small dim type over the first row; M and S need
none (the marks carry the letter). Coarse and fine are the plugin's text, nothing is computed: the extension has no field for a combined ratio or
frequency (notes/ui-fm-fields-proposal.md, outside the repository, asks for one).

Editing in place, by the dial and the buttons: the cursor moves by row and column (above); ENTER on a mark switches it, on
another cell gives its value the dial (the ring is the accent's while it has it), ENTER or EXIT ends it. By touch: a tap
on any cell selects its row, on M, S or the mode switches it when the finger lifts on the cell it went down on; a drag
up or down on a number's cell steps it (a step per 12 px as before); a touch on a level bar sets the level where it lands and
follows the finger across the bar. The level bar is the whole bar, not only its fill.

Mute and solo are the same marks as Play's and the mixer's (`Theme.mark-radius`, the theme's mark style, red-toned mute
`Theme.danger`, solo the accent, off `Theme.mark-off`): they are **audition switches** (owner's decision 2026-11-16, from the
reference's own parameter list: an operator has no stored mute, solo or on/off there; a silent operator is saved as level
0): transient, not saved with the sound, not undo steps, not automatable. What the data offers:

- a mute field (`operatorMute`, provisional: the extension has no such field yet) marked `transient`: M is one `params.set`
  with no gesture and no commit; S is the host's: it mutes every other operator (one `params.set` each, only those not
  muted) and gives the mutes back as they were when the last solo is released (several may be soloed: the un-soloed are
  muted, the soloed not); the state is the view's own (per UI instance, no process-wide state; a mute pressed during a
  solo is what the release restores), is gone when another sound opens in the part, and a small accent "solo" mark over
  the M and S columns tells that a solo is on; a muted operator's chip, line, box and bar are dimmed, so is every
  operator that is not soloed while any is;
- a mute field that is not transient: M is an edit as any other (a gesture around its `params.set`); S is disabled: a host
  solo would record edits as the player's and save them, and the readout says so;
- a mute and a solo field (`operatorSolo`, provisional): M and S are one edit each of their fields;
- neither: M and S are drawn disabled (outlined, faint) and the readout says "This plugin has no mute for its operators".

The readout is the table's heading line: while the cursor (or a tap) is on a disabled cell the column headings give way to
its reason. The proposal for the extension (new `oscairn_field` values, the transient flag) is in
notes/ui-fm-fields-proposal.md, outside the repository.

## The detail: named stages in real units, compact controls

The envelope of an operator is shown and edited as **named stages**, not numbered rates and levels (src/grid.rs `stages`):

- A numbered envelope (`rateK` or `timeK`, `levelK`): a segment with a time and no level of its own is a **Hold**; the others in
  order are the **Attack**, the **Decay** (**Decay 1**, **Decay 2**... where there are more than one) and, the last, the
  **Release**. Four rates and four levels read Attack, Decay 1, Decay 2 (level 3 is the sustain), Release (level 4 the
  start and end level). A level is shown once under the stage whose own field it is (`levelK` under segment K): the
  engine's hold has no level of its own, and the release level it starts from is the release's.
- A named envelope (`delay`, `attack`, `hold`, `decay`, `sustain`, `release`): by its fields, the sustain level under the
  decay as "Sustain".
- **Times.** A time stage shows the plugin's `seconds` where it gives them, in ms or s ("12 ms", "1.3 s"), else the
  parameter's own text. A **rate** (the plugin's `rateK`, faster when higher: the time a stage takes depends on the levels
  it runs between, only the plugin knows it) shows, with `seconds`, the time a segment across the whole range takes,
  marked "/range" ("180 ms /range"): never presented as the stage's duration; without `seconds` the raw value and the word
  "rate" ("57 rate"). A made-up time is never shown.
- The graph (a larger view of the selected operator's envelope: bright over a wash with handles; every other operator's line
  dim behind it) gives **each stage a slot of its own**, as wide as the others (a hold after the sustain between two
  slots); a stage's point lies in its slot at the stage's own place on the slot's axis: a time by its seconds on the
  scale (about even below 50 ms, logarithmic past it, up to the longest stage of all the operators), else by its range.
  The axis is one slot wide and read stage by stage, with a time scale under the graph (ticks and the labels that fit:
  10 s, 1 s, 100 ms, 10 ms; where a stage is a rate the widths are told "nominal", and where there are no seconds "by
  value"). The top pane's shared graph keeps its stage-after-stage axis on one time scale where every stage has
  seconds, with the same scale and notes. The slots are why a control can sit under its point wherever the values put it.
- **The Time row and the Level row** (`View::Stages`, one panel with the graph and the labels "Time" and "Level" at the
  left, and compact controls over it): each control is in the slot of the stage it moves (the slot is the control's
  width; a stage's point lies within it, within a node's radius: tests/fm_table.rs `each_stage_control_is_under_its_point`),
  a knob with its name over it and its value beside it, so the Time row's Hold, Attack, Decay 1, Decay 2 and Release sit under
  the graph's stage boundaries and the Level row's Attack, Decay 1, Decay 2 and Release under theirs; the hold has no level
  control. Slots narrower than a touch target (an envelope of more than about eight stages at 826) shrink the controls to the slot.
- **Compact control** (owner: a denser view of the same design language; `grid::compact_height`, `Theme.compact-height`,
  `Theme.compact-width`, defined once and held equal by a test): the touch target high (44) or what its name line (1.15 lines
  of the small type), a control 0.45 of a touch target high and the tight paddings (twice `Theme.pad-tight`) need, if
  that is more, and 1.9 touch targets wide at least (so eight fit across the narrowest screen with both rails: 743 across).
  Never below the touch target. It is the grid's tile in a smaller drawing: the same section background, edge and radius,
  type sizes (the name in the small bold dim type, the value in the small type), knob, slider, switch and list drawing,
  cursor ring and accent while editing; a knob at the left of the cell with its value beside it (shrunk to 70% before it is
  cut). The gaps inside the detail are two thirds of the sections' (none in a theme whose sections join). Split buttons are a
  list in it, a range a slider. Another view that wants a dense row sets `GridTile.compact` and lays its tiles out at
  `grid::compact_height` high.
- The pitch envelope and the other parameters an engine has (about twenty an operator: level velocity, scaling, form,
  key-on reset, fixed key follow, pitch velocity, the pitch envelope's levels and times...) are plain controls of the row of
  eight, paged by row; a second small envelope as a second graph is not drawn: the extension has no way to say one
  operator has two envelopes beyond a view number of its own (notes/ui-fm-fields-proposal.md).

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
