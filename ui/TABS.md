# Tabs: one family for the whole UI

The owner (2026-11-15): "Plugin uis can have tabs too, like the modx does, they just need to be clear, and we
need a consistent themeable style for tabs across the whole ui"; "Yes plugins should be able to name and order
their tabs"; "if a plugin wants to draw their own tabs and their own completely separate gui, thats fine. but we
do want to provide predefined elements they can use to simplify development and keep things visually
consistent". Two levels, as a well-known performance keyboard's edit screens have them: a first level of pages,
a second level of tabs.

**A recorded difference from the reference (owner-approved, with the request above).** The JUCE parameter grid
(hosts/standalone/ParameterGrid.cpp `refresh`) lists pages as buttons down the left, over a search field, and
draws tabs as rows of the same plain buttons; here pages and tabs are one themed family (below), the search is
a button on the top bar, and the cursor does not turn the page as it walks off the end of the tiles (a page
or tab opens on a press). The rules that say which pages and tabs a plugin has (`tabsFor`: a page per
top-level module, a tab per second-level module on a page of more than 24 parameters) are the reference's and
unchanged.

## Inventory: where the user switches between sibling views of one thing

| Place | What it switches | Widget before | Tabs? | Why |
|---|---|---|---|---|
| Parameter pages (grid) | the pages of a plugin: first level of each parameter's module path | a list of buttons down the left, under the search field | **yes**, first level | siblings views of one plugin; the reference names them pages and shows them as a row of selectable buttons, one selected |
| Parameter tabs (grid) | a page's tabs: second level, above 24 parameters | a strip (the old `TabStrip`) | **yes**, second level | the reference's own word is "tabs" (`tabsFor`) |
| Play's knob row: Performance / the selected part's controls | what the six knobs follow | two stacked buttons at the right of the row | **yes**, a column of two | two views of one row, exactly one chosen at a time, the reference's `performanceControls_` and `partControlsButton_` are a radio pair; both always visible |
| Play's knob row: the bank button ("Bank 1/2", or a row's name) | which bank of the part's controls | one button | no | the reference cycles one button through the banks (`++page_`), there can be many, and it names the bank it is on: a pager, not a set of siblings shown together |
| Parts list: Mixer, Controls, Routing | opens another screen with its own title and Back | three buttons under the list | no | the reference's `mixerButton_`, `controlsButton_`, `routingButton_` are push buttons that show a page replacing the list (title "Mixer", Done); they are navigation to other screens, not siblings in one place |
| Routing's header toggles (Meters, Parts collapsed...) | a setting | toggle buttons | no | toggles, not sets of views |
| Controls: "By scene" | rows of mappings versus a table | one toggle button | no | the reference's `toggle(..."By scene")`: an on/off |
| Part screen's rows (Sound, Insert effects, Keyboard...) | nothing: one list of rows | rows | no | one scrolling list, not sibling views |
| Setlist: the setlists and a setlist's banks | which setlist is open, which bank of it shows | two dropdowns in the row under the top bar (the shared menu), Previous and Next beside the banks' | no | there can be many of either, each named where it is chosen: a list to pick from and a pager, not a few siblings shown together |
| Sounds chooser | one list with a search | a list and a search row | no | the reference's tags are a filter, several of them chosen at once (all must match), not siblings of which one shows: a Filters row opens a list of Favorites, Recent, Library and Tags, one scrolling list a level down each time, not tabs |
| Chain: a split's lanes, a lane's plugins and settings | the chain screen's levels: the part's chain, a split's lanes, a lane | rows | no | one scrolling list a level down each time (a press opens, Cancel goes back), as the reference's chain row and panel: the lane's plugins and settings are not alternatives (the reference shows both at once), so they are one list, not tabs |
| Mixer's paging buttons, the Parts and Setlist views of a long list | which slice of a list | ‹ › buttons | no | paging, not sibling views |
| Settings pages | Settings is a JUCE app: not in `ui/` | - | n/a | the same component is for it when it is ported |

So the family has three users today: the parameter pages (two levels), and Play's two-tab column.

## The family (slint/theme.slint)

- `TabBar`: a row of tabs, or with `vertical` a column of them (each `item` tall). `current` is the selected
  tab (-1: none), `cursor` the one the border cursor is on (-1: none); a tap says `picked(i)`, nothing else
  changes. A tab's name shrinks to 70% of the type when it does not fit and is then cut (`FitText`, the
  reference's `drawFittedText` rule), never squeezed below that. A row's tab is as wide as its name in bold
  needs, at most four and a half touch targets; a column's tab is as wide as the column.
- Many tabs: the strip is a `Flickable` that scrolls (a touch drags it). Where it goes on, an arrow box
  (`TabMore`, the surface color, an arrow in the text color) sits over the cut end with the cut tab behind it,
  and a touch on it scrolls most of a view. The tab the cursor is on (else the selected one) is kept in view
  clear of the arrow boxes.
- `TabPages`: the two-level arrangement, from `pages`, `tabs` and their selection and cursors, over the whole of the screen
  below the top bar. The pages are a strip with the tabs a strip under it (`top`), or two rails at the left (`side`, below). A
  page with fewer than two tabs has no second row or rail; a plugin with one page has neither (no empty row). The content goes
  where `content-x`, `content-y`, `content-width` and `content-height` say.
- The arrangement is the theme's `tabPlacement` (THEME.md): `top` two strips, `side` two rails, `auto` whichever of the two
  leaves the content the larger area at the size at hand (two strips where they are equal: a plugin of one page). `TabPages`
  publishes it as `mode` (0 strips, 1 rails), the Grid as `GridState.layout`. The old third arrangement (the pages a column
  beside the tiles, the tabs a strip over them, chosen on a window 560 or more tall) is gone: a rail does what it did, narrower.
- A strip whose whole names do not fit (and whose names share a word) shows the shared word once at its start, in the dim
  color, and each tab only what differs: see "Shared words" below.

### Rails (`side`)

- **Height.** A rail runs from just under the top bar to the bottom of the screen, edge to edge, one continuous track (the
  screen's left edge to the content, the first rail from `x = 0`). Its tabs share that height equally: each is the rail's
  height (less its heading, if it has one; the folder style keeps the theme's gap between neighbor tabs) over the number of
  tabs, at least a touch target (`Theme.touch`); with more tabs than that, the rail scrolls (a touch drags it, the cursor's
  tab is kept in view) with an arrow box over each cut end, as the strips do. With 5 pages on the keyboard's screen each
  page tab is 83 logical px tall; that is intended.
- **Width, fixed by the theme and not by the names.** `Theme.rail-width` = 5 x the small type's size + 1.5 x the tight padding
  (13 x 5 + 1.5 x 4 = **71 logical px** at the touch scale, 110 physical px on the keyboard at 1.55), at most
  `Theme.side-share` (**12%**) of the screen's width (99 at 826, 77 at 640, 48 at 400). It grows with `fontScale` and
  `spacing` (74.6 at spacing 1.6, 97 at font 1.4, 55.6 at spacing 0.6 and font 0.8, tests/tabs.rs
  `a_rails_width_is_the_themes_and_at_most_twelve_percent_of_the_screen`). Between the pages' rail and the tabs' there is the
  theme's gap; the content starts one margin after the last rail.
  **A lone rail** (the Library's, the Settings': pages and no second level) is wider when its longest label needs it: the
  label on one line at the small type in bold (a word that cannot break; a label with a space may break as before) plus the
  tab's padding each side, at most `Theme.rail-label-share` (**16%**) of the width ("Performances" is 105 logical px at 826,
  102.6 at spacing 0.7); beyond that the shrink and the ellipsis below stay the fallback (a user-named tab). Two rails (a
  plugin's pages and tabs) keep the theme's width. The `auto` placement is decided at the theme's width, so a longer label
  never turns the rails into strips.
- **Text** (src/tabs.rs `abbreviate`, `two_lines`; slint/theme.slint `Rail`): every label of a rail is horizontal.
  (a) At the small type, on one line, or on two where it has a space to break at, when every label fits the rail's width that
  way ("Operators", "Voice Shaping": the keyboard's rails). (b) *Rotated text (reading bottom to top) was not built: Slint 1.18.1's
  software renderer, the one the device and the headless tests use, does not rotate text.* Checked by rendering: a `Text`
  given `transform-rotation: -90deg` (its `rotation-angle` is the deprecated alias) came out upright and unchanged, and
  i-slint-renderer-software's `rotate` is `// TODO (#6068)`. A rotated label would need its own glyph drawing. (c) Otherwise the
  same horizontal text, uniformly for the whole rail, shrunk by the least that fits every label and at most to 70% (the
  reference's `drawFittedText` rule), then each line cut with an ellipsis. One rail never mixes sizes. Each rail reports its
  factor (`GridState.page-fit`, `tab-fit`). On the keyboard's screen at 1.55: the five pages ("Operators" in bold, the widest,
  is 4% too wide for the rail) 0.95, the eight tabs "1" to "8" 1.0, "Voice" and "Output" 1.0, "Sound" and "Output" 1.0, and the
  unrelated names "Modulation Matrix", "Voice Shaping", "Macro Controls", "Global" 0.70 (the names are neutral long labels of one plugin's tabs, chosen only to be long; the test prints the factor).
  Shrunk text is as crisp as full-size text (compared enlarged: `rail-long-names-*`).
- **Shapes** follow the `tabs` style. `segmented`: the track is the raised color, edge to edge; the selected tab is a cell
  lifted out of it (the soft shadow where `depth` is soft), inset by the tab padding, rounded by the radius + 1. `folder`:
  the track is the surface color with the rule along its right edge; every tab is a piece rounded at the left, the unselected
  ones 4 px back from the rule, the selected one reaching over the rule to the page it opens (the first rail's selected tab
  to the second rail, the last rail's to the content). The cursor is the same ring round the tab's own part, in both.
- **Gap to the content, and the tile area** (826 x 465 logical, segmented, dark; tests/tabs.rs): see "Placement" below.

### Shared words

When all labels of a rail share their leading words ("Operator 1" to "Operator 8") or their trailing words, the shared part
is shown **once** as a heading at the top of the rail (a strip: a short cell at its start), in the dim color, and each tab
shows what differs ("1" to "8"). `abbreviate` takes off the longest common run of leading words and then of trailing words,
only with two or more names, never a name's last word, and only if what is left still tells the names apart; both ends
show as "Operator … Level". A mixed rail ("Operator 1", "Operator 2", "LFO"), a single tab and a rail of unrelated names are
not abbreviated. The names themselves are not changed: the model, `GridState.pages` and `GridState.tabs`, and anything that
reads them (the cursor's readout, a screen reader) keep the whole names; the short forms are `page-labels`, `tab-labels`
and the headings. A rail is always abbreviated; a strip only when its whole names would overflow (the eight operators then
fit: the shared word at the start, then 1 to 8). (The top bar's path today names the plugin, not the page or the tab; that
is unchanged.)

### The theme

Everything comes from the theme file: the `tabs` style, the colors, `radius`, `spacing`, `fontScale`; nothing
is a literal.

- `segmented`: the tabs lie in a track (the raised color, the radius + 4); the selected one is a piece lifted
  out of it (the soft shadow where `depth` is soft), rounded by the radius + 1. In a column the track is a
  column; in a rail it is the rail, edge to edge.
- `folder`: every tab is a folder piece, rounded at the side away from the page (the top of a row's tabs, the
  left of a column's or rail's); a rule runs along the page side. In a row every tab is the row's whole height, on the rule,
  the theme's gap between them, the first one at the row's left edge, rounded by the corner: the selected one differs by fill
  and ink only, not size, and the row is as tall as what stands beside it (the Library's search box: tests/library_tabs.rs, in
  every theme at 1.55 and 1). In a column or rail every tab is likewise the whole height of its row, the theme's gap
  between them (none in a segmented style); the selected one reaches the rule and the page past it, the others stop short
  of it and sit 4 px back, outlined by the hairline. The gap is designed: a strip of the screen between two tabs is not a
  seam (tests/vertical_tabs.rs measures each tab's height and the gap between neighbors in every theme at 1.55 and 1).
- The selected tab is told by its fill, not an outline: `Theme.tab-on` with its text `Theme.tab-on-ink` in
  bold, against an unselected tab's `Theme.tab-off` with `Theme.tab-off-ink` in regular weight. They are set
  from the theme by src/palette.rs `tabs`: the selected fill is the accent where the accent holds
  3 to 1 against the unselected fill (the raised color), the surface and the background, and 4.5 to 1 against
  its text; else the theme's text color (a theme file must keep the text 4.5 to 1 from all three, so this fits in any
  theme the loader accepts); the text on it is the text or the background color, whichever contrasts more.
- **Guaranteed: the selected tab's fill is at least 3 to 1 (WCAG 1.4.11's graphics) from an unselected tab's, and
  the text on it at least 4.5 to 1**, in every theme and both styles. Tested in the rendered pixels
  (tests/tabs.rs `the_selected_tab_is_told_by_its_fill_in_every_theme`): in the four built-in themes and the two
  odd ones (extreme colors; sections joined), both styles, a column and a strip, both fills are painted in
  quantity in the tab bars; the least ratio over all of them was 5.49 to 1 (printed by the test).
- The cursor is a ring (the `focus` color, 2 px) round the tab's whole box, with the fill inset by the ring's width
  and a hair, so the ring and the fill do not merge when the focus color is the accent.

## The parameter pages

- Pages (first level) and tabs (second level) come from one structure, src/tabs.rs `Model`: ordered pages, each
  with ordered tab names and its parameters, each parameter on a page and perhaps on one of its tabs, with the
  heading its tile sits under. It is filled today by inference from the module paths (`Model::inferred`: the
  reference's rules, `editor_pages` and `tabs_for`), or from what a plugin declares (`Model::declared`, below);
  the widgets and the screen's cursor read the model only. A parameter on a page that has tabs but on no tab is
  drawn above the tabs, whichever tab is open (the header's rule).
- The search is a button of the top bar (the screen's own action, with Back and the path title: docs/rust-ui.md "The top bar
  carries the chrome") for a plugin of more than 24 parameters; while a search is on it reads "Search: " and the text (not lit: the cursor's ring would vanish on the accent). It no longer costs a column.

### The arrangement for the keyboard's screen (history: the first choice, since replaced by "Placement" below)

826 x 465 logical (1280 x 720 at 1.55), the shell's top bar 48 above, measured by rendering both (a test since removed with
the arrangement it measured), the tile area in logical pixels (width x height):

| Plugin | Before (a plain list at the left, 180 wide) | Pages as a column | Two strips stacked (chosen) |
|---|---|---|---|
| 2 pages, no tabs | 628 x 357 = 224 196, 2 columns | 630 x 357 = 224 536 (a column at 630 has 2 columns: 2.99 of 210) | **814 x 309 = 251 072, 3 columns** |
| 5 pages, the first of 8 tabs | 628 x 309 = 194 052, 2 columns | 630 x 309 = 194 305, 2 columns | **814 x 261 = 212 010, 3 columns** |

(The "before" numbers are the old layout's: GRID.md "Tile density" pinned 628 x 357; the tab row took 48 more of the
height, so 309 is derived, the rest measured.) The strips leave 9% to 12% more area and a third column (tiles
267 wide instead of 311, three to a row instead of two); the pages are all in view at once, and the eight operators
scroll in one strip. (The column arrangement was kept then for windows 560 or more tall; it
is gone now, a rail is the column made narrow and the full height.)

## Without touch

The border cursor walks the rows above the tiles in reading order, then the tiles: the search, the pages, the
tabs, the tiles. The dial (and DEC/INC, CURSOR UP/DOWN: one step) moves it by one; leaving a row at its end goes
to the next row's *selected* entry. Left and right (or the right knob, up and down) move along a row and
between rows: in a strip, left and right along it, up and down to the row above and below. Nothing opens by being moved to: a
press opens the page or tab under the cursor (the cursor stays on it), the tiles stop at either end and no longer turn the
page. In `side` the pages' and the tabs' rails are two columns: up and down along each, right to the next rail or the tiles,
left from the first column of tiles to the tabs and from the tabs to the pages (the dial's order is the same: pages, tabs,
tiles). The ring is round the tab's own part, two logical pixels thick, and shows on a narrow tab (tests/tabs.rs
`the_cursor_in_the_rails` counts it in the pixels, both styles). Cancel is Back, the screen's one step up, from anywhere; the search
is the top bar's, reached by its own cursor stop.

A direct way to step tabs from inside a page: the page keys (Page Up and Page Down on a computer keyboard,
`Action::Page`) step through the pages and tabs at once, tab by tab and then the next page. The reference has
no key for it (its grid is touch and mouse only: GRID.md "Navigation in the JUCE app") and no device profile
has a page key (the FANTOM-07's panel map navigates with the VALUE dial, E1 and E6 on lists only, ENTER, EXIT,
DEC, INC, CURSOR UP and DOWN; on the editors E1 to E6 play the page's parameters), so on a unit the way is the
dial: one step back from the first tile is the tabs, on the tab shown, then a step or two along and a press.
Unverified on a unit.

Touch: a tap on a tab opens it and lets the cursor go (one cursor at a time).

## Tabs that are indexes (owner, 2026-11-16)

Numbered siblings of one structure are not pages or tabs. The owner, on seeing a tab per FM operator: "I think there's got to
be an interface design that would keep all operators on the same page." When a page's tabs are exactly the indexes of one
view, the host does not draw them as a tab row or rail: the page is that view's master and detail (GRID.md "Master and
detail"), the tab names are never shown as tabs (the detail's heading uses them: "Operator 5"), and the tab family shows
only what is left (the pages, if there are two or more).

**The rule** (src/tabs.rs `indexed_view`, `Model::fold_indexed`, run on every model, inferred from module paths or declared):
a page with tabs is folded when
- it has two or more tabs, and every one of them has a parameter;
- every tab but at most one with no name (the page's own tab, "General": its parameters stay on the page, above where the
  tabs were) has a name ending in a number, all with the same words before the number ("Operator 1", "Operator 2", ...), no
  number twice;
- every parameter on such a tab belongs to one view (the same for all the tabs) and carries the tab's number as its index
  (every parameter under tab "Operator k" has that view and index k);
- that view has an overview: today an FM operator view, one with an `algorithm` field (`grid::master_view`). A view without
  one (an envelope per index, a key map's zones: no overview is built for them yet) keeps its tabs.

Anything else stays tabs: a tab whose parameters have another index, names without numbers or with different words before
them, parameters in no view, two views among the tabs, a tab that is not an index ("Routing" beside "Operator 1" to 8).
Tested positive and negative: tests/fm_master.rs `tabs_that_are_the_indexes_of_one_view_fold_into_it_and_others_stay_tabs`.
A page that is folded has no tabs (`Page::tabs` is empty, every parameter on the page, `Page::indexed` has the view and the
names by index); a page of the view that was never tabs (a plugin of at most 24 parameters on one page, or without module
paths) is the same master and detail from the view's presence alone, with the heading "Operator N".

The same rule's dial order: pages' rail or strip, the master's overview, the tiles: the overview's row is one more stop
above the tiles (`Strip::Overview`), after the tabs' row where a page has one.

## What a plugin can declare (draft 6 of the layout extension)

`oscairn_ui_tab_t`, `tab_count()`, `tab()` and `param_tab()` let a plugin name and order its pages and tabs
and place parameters on them; `value_icon()` names an icon for a stepped parameter's value from a closed set.
The engine's `params.list` carries none of it yet (docs/control-protocol.md has no such field), so the UI reads
**provisional** fields, named by this UI and to be replaced by the protocol's:

- at the top of the `params.list` result `uiTabs`: `[{id, parent, name}]` (`parent` 0 a page, else a page's id);
- on a parameter `uiTab`: the id of the page or tab it is on; and `valueIcons`: for a stepped parameter, the
  `enum oscairn_icon` of each value from its minimum (0 none).

Read by src/tabs.rs `declared_in` and src/grid.rs `parse`. The rules are ui.h's: pages are the entries with parent
0 in order, a page's tabs the entries naming it; an entry with an empty name, an id used before or a tab under
anything but a page is left out; at most 16 pages and 16 tabs under a page; a page with fewer than two tabs is drawn
without them; a parameter on a page is above its tabs; one the plugin leaves out, or whose entry was left out, goes
where its module path puts it (the declared page of that name, else a page of its own after the declared ones).
Icons: `grid::icon` takes the declared icon first and falls back to the name rule (`wave_icon`); where any value
of a parameter has a declared icon the host guesses none for the others (`grid::icons_for`). Saw down and sample
and hold are drawn too.

Tested with a fake engine that gives the provisional fields (tests/tabs_model.rs). Not done: "a view is drawn
once, on the first of its parameters' tabs in the order of `tab()`, beside the tiles there": the grid draws a
view from the parameters of the tab open, so a view whose parameters the plugin spreads over tabs is drawn from
the ones on the tab shown.

A plugin that wants its own tabs and its own separate GUI draws them itself; the family is what it can use
instead, from `ui.h` (`oscairn_ui_tab_t`), without writing a line of drawing.

## Placement: the two forms on the keyboard's screen (2026-11-16)

With the screen's header row gone to the top bar (the grid starts under the bar), 826 x 465 logical (1280 x 720 at 1.55), the tile
area in logical pixels, measured by rendering both (tests/tabs.rs
`the_placements_on_the_keyboards_screen_leave_the_tiles_their_room`; segmented style, dark theme). `side` before is the first
version (two columns as wide as the longest name, 104 and 109 for the five pages, which was rejected), after is the rails:

| Plugin | `top` (two strips) | `side` before (wide columns) | `side` after (rails, 71 wide each) |
|---|---|---|---|
| 5 pages, the first of 8 tabs | 814 x 309 = 251 072, 3 columns of tiles | 593 x 405 = 239 800, 2 columns | **668 x 405 = 270 138, 3 columns** |
| 2 pages, no tabs | 814 x 357 = 290 135, 3 columns | 729 x 405 = 294 814 | **743 x 405 = 300 477, 3 columns** |
| the FM fixture (one page) | 814 x 405 = 329 198 (no rows: nothing to switch) | 814 x 405 = 329 198 | 814 x 405 = 329 198 (no rails) |

The strips' numbers did not change. `side` leaves the tiles the full height (465 less the bar's 48 and the 6 px margin above and
below) and now costs 142 + 4 + 6 + 6 of the width with both rails (the rails 2 x 71, the gap between them, the margin after
them and at the right edge): the content is 668 wide, three columns of 210 or more (the reference's rule, GRID.md), where the first
version's 593 had two. The rails leave 8% more area than two strips for the five pages and 4% more for two pages, and the tabs are
as tall as the screen.

**`auto`** is the placement that leaves the content the larger area at the size at hand (src/theme.slint `TabPages`: the content's
width times its height in each placement, from the screen's size and which rails there would be; two strips where they are
equal). Measured (tests/tabs.rs `auto_is_the_placement_with_the_larger_tile_area`, logical px, segmented, dark):

| Plugin | 826 x 465 (1280 x 720 at 1.55) top / side | 1280 x 720 at 1 top / side | 800 x 480 top / side |
|---|---|---|---|
| 5 pages, 8 tabs | 251 072 / 270 138: **side** (3 columns both) | 715 152 (4 columns) / 740 520 (4): **side** | 255 312 / 269 640 (3 both): **side** |
| 2 pages, no tabs | 290 135 / 300 477: **side** (3 both) | 776 016 (6) / 790 020 (5): **side** | 293 136 / 301 140: **side** |
| FM, one page | 329 198 / 329 198: **top** (equal) | 836 880 / 836 880: **top** | 330 960 / 330 960: **top** |

(Before, `auto` chose the strips at 826 x 465 and at 800 x 480, and the wide pages' column at 1280 x 720 at 1, whichever the plugin.)
Area alone decides; where `side` costs a column of tiles (the two pages at 1280 x 720 at 1: 5 columns against 6) it still wins on area. A
theme that wants the strips says `top`.

Both forms were looked at in both tab styles in the dark, light and the two odd themes at 826 x 465, 1280 x 720 at 1 and 800 x 480
(pictures `tabs-PLUGIN-PLACEMENT-THEME-STYLE@*` in target/theme-shots, rails enlarged `tabrows-*-rails@*`, the 14-tab and long-name
rails `rail-*`).
