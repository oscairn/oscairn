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
a button in the header row, and the cursor does not turn the page as it walks off the end of the tiles (a page
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
| Setlist: the setlists pane and the slots pane | two panes shown together, the cursor goes between them | two lists | no | both are on screen at once; neither replaces the other |
| Sounds chooser | one list with a search | a list and a search row | no | there are no categories in the Rust screen or the reference's chooser (tags, E1, are not built); a tag filter would be tabs when it exists |
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
- `TabPages`: the two-level arrangement, from `pages`, `tabs` and their selection and cursors. `column` puts
  the pages as a column at the left with the tabs a strip over the content; otherwise the pages are a strip and
  the tabs a strip under it. A page with fewer than two tabs has no second row; a plugin with one page has
  neither (no empty row). The content goes where `content-x`, `content-y`, `content-width` and
  `content-height` say.
- The grid picks `column` from the window: `Theme.column-from` (560 logical px) of height or more. At
  826 x 465 (the keyboard at 1.55) and 800 x 480 it is two strips.

### The theme

Everything comes from the theme file: the `tabs` style, the colors, `radius`, `spacing`, `fontScale`; nothing
is a literal.

- `segmented`: the tabs lie in a track (the raised color, the radius + 4); the selected one is a piece lifted
  out of it (the soft shadow where `depth` is soft), rounded by the radius + 1. In a column the track is a
  column.
- `folder`: every tab is a folder piece, rounded at the side away from the page (the top of a row's tabs, the
  left of a column's); a rule runs along the page side. The selected one reaches the rule and the page past it;
  the others stop short of it and sit 4 px back, outlined by the hairline.
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
- The search is a button at the end of the header row (Back, the title, Search) for a plugin of more than 24
  parameters; while a search is on it shows the text, lit. It no longer costs a column.

### The arrangement for the keyboard's screen

826 x 465 logical (1280 x 720 at 1.55), the shell's top bar 48 above, measured by rendering both
(tests/tabs.rs `the_arrangements_on_the_keyboards_screen`, pictures `tabs-arrangement-*`), the tile area in logical
pixels (width x height):

| Plugin | Before (a plain list at the left, 180 wide) | Pages as a column | Two strips stacked (chosen) |
|---|---|---|---|
| 2 pages, no tabs | 628 x 357 = 224 196, 2 columns | 630 x 357 = 224 536 (a column at 630 has 2 columns: 2.99 of 210) | **814 x 309 = 251 072, 3 columns** |
| 5 pages, the first of 8 tabs | 628 x 309 = 194 052, 2 columns | 630 x 309 = 194 305, 2 columns | **814 x 261 = 212 010, 3 columns** |

(The "before" numbers are the old layout's: GRID.md "Tile density" pinned 628 x 357; the tab row took 48 more of the
height, so 309 is derived, the rest measured.) The strips leave 9% to 12% more area and a third column (tiles
267 wide instead of 311, three to a row instead of two); the pages are all in view at once, and the eight operators
scroll in one strip. The
column stays for windows 560 or more tall, where the height is spare and a long page list reads best: a 1280 x 720
window at scale 1 gets the column.

## Without touch

The border cursor walks the rows above the tiles in reading order, then the tiles: the search, the pages, the
tabs, the tiles. The dial (and DEC/INC, CURSOR UP/DOWN: one step) moves it by one; leaving a row at its end goes
to the next row's *selected* entry. Left and right (or the right knob, up and down) move along a row and
between rows: in a strip, left and right along it, up and down to the row above and below; in the pages'
column, up and down along it, right to the tabs (or the tiles), left from the first tab or the first column of
tiles back to the pages. Nothing opens by being moved to: a press opens the page or tab under the cursor (the
cursor stays on it), the tiles stop at either end and no longer turn the page. Cancel backs out: the cursor, the
search, the screen.

A direct way to step tabs from inside a page: the page keys (Page Up and Page Down on a computer keyboard,
`Action::Page`) step through the pages and tabs at once, tab by tab and then the next page. The reference has
no key for it (its grid is touch and mouse only: GRID.md "Navigation in the JUCE app") and no device profile
has a page key (the FANTOM-07's panel map navigates with the VALUE dial, E1 and E6 on lists only, ENTER, EXIT,
DEC, INC, CURSOR UP and DOWN; on the editors E1 to E6 play the page's parameters), so on a unit the way is the
dial: one step back from the first tile is the tabs, on the tab shown, then a step or two along and a press.
Unverified on a unit.

Touch: a tap on a tab opens it and lets the cursor go (one cursor at a time).

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
