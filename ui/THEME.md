# The Rust UI and the theme system

The theme format is the JUCE app's (docs/theme.schema.json, hosts/standalone/Theme.cpp
`ThemeData::parse`, `Theme::load`): one format, the same folders, the same files. This file is the
audit made at 3a97dd63 (before the work) and, further down, what the work did.

## 1. Audit at 3a97dd63

### Properties of a theme file

"Rust loads" is `ui/src/theme.rs`; "exposed" is the `Theme` global in `ui/slint/theme.slint`.

| Property | Type | Default (Theme.h) | Range / choices | Styles | Rust loads | Exposed | Used by |
|---|---|---|---|---|---|---|---|
| `version` | int | required | 7 only | the format | no (not checked) | no | none |
| `id` | string | required | `[a-z0-9-]`, 1 to 64 | the folder name | yes (as a choice key) | no | none |
| `name` | string | required | 1 to 80 characters | the Settings list | no | no | none |
| `font.regular` | path | required | .ttf/.otf in the folder | text | yes, registered with fontique | `font-family` | every Text (window default) |
| `font.bold` | path | none | as above | bold text | yes | with the family | every bold Text |
| `font.license` | path | required | file in the folder | none (checked to exist) | no | no | none |
| `fontScale` | number | 1 | 0.8 to 1.4 | type sizes | yes (refused out of range) | `font-scale` | `Theme.small/body/button/title` everywhere; Play's Edit button width |
| `spacing` | number | 1 | 0.6 to 1.6 | paddings and gaps (a scale) | yes (refused out of range) | `spacing` | `Theme.gap`, `margin`, `pad*`, `section_gap`, the envelope's plot top |
| `radius` | number | 6 | 0 to 16 | corners | yes (refused out of range) | `radius` | every rounded Rectangle |
| `colors.background` | #RRGGBB | #15181D | required | window, wells, text on accent | yes | yes | all screens |
| `colors.surface` | #RRGGBB | #20252D | required | cards, tiles, fields | yes | yes | all screens |
| `colors.raised` | #RRGGBB | #2C333D | required | buttons, tracks | yes | yes | all screens |
| `colors.border` | #RRGGBB | #526070 | required | edges, rules, tracks | yes | yes | keyboard, grid, home, play, topbar |
| `colors.text` | #RRGGBB | #EDF1F5 | required, 4.5:1 on the three above | text | yes (no contrast check) | yes | all screens |
| `colors.muted` | #RRGGBB | #A3AEBB | required | secondary text | yes | yes | all screens |
| `colors.accent` | #RRGGBB | #FFA45B | required | selection, fills | yes | yes | grid, piano, play, part, keyboard, topbar, setlist |
| `colors.positive` | #RRGGBB | #5AD9A4 | required | meters | yes | yes | play |
| `colors.danger` | #RRGGBB | #FF8D87 | required | mute, errors | yes | yes | play, topbar, shell |
| `colors.warning` | #RRGGBB | none | optional (accepted, not stored by JUCE) | meters past 75% | yes | yes | play, topbar |
| `colors.focus` | #RRGGBB | none | optional (accepted, not stored by JUCE) | cursor border | yes (default: accent) | yes | all screens with a cursor |
| `style.buttons` | choice | pill | rounded, pill | button and field corners | no | no | none |
| `style.edges` | choice | hairline | none, hairline, outlined | outlines on controls and cards | no | no | none |
| `style.depth` | choice | soft | flat, soft | 1 px shadow under buttons, thumbs | no | no | none |
| `style.sliders` | choice | line | filled, line, cap | slider and knob look | no | no | none |
| `style.tabs` | choice | segmented | folder, segmented | tab strips | no | no | none |
| `style.marks` | choice | square | square, round | mute and solo | no | no | none |
| `style.sections` | choice | separate | separate, joined | cards apart or joined | no | no | none |
| `style.rows` | choice | left | left, right, dot, fill | where a row's color goes | no | no | none |
| `wires.audio` .. `wires.control` (6) | #RRGGBB | #8CC2FF #C29CFF #6B7480 #FF9A45 #FF6B63 #6FE0A8 | required | routing wires | no | no | none (no routing view yet) |
| `splash.image` | path | none | .svg or .png in the folder, 1 MiB | splash | no | no | none (no splash yet) |
| `splash.background`, `splash.ink` | #RRGGBB | background, text | 4.5:1 together | splash | no | no | none |

Behavior of the reference that the Rust loader did not have: a file is refused whole (not merged
over the default) for an unknown field anywhere, a missing required field, a wrong type, an
out-of-range number, a bad color, text contrast under 4.5:1, a folder not named after its id, a font
that is missing, too large (4 MiB), not a .ttf/.otf or not a font container; a refused choice
falls to the default theme (screen theme, `themes/default`, Oscairn Dark), then to the emergency
theme. The Rust loader merged missing keys from the default and clamped numbers.

### Hardcoded values in the Rust UI at 3a97dd63

Colors:
- `piano.slint:112` white key `#f4f4f0`, `:118` its label `#444`, `:146` black key `#1b1b1d`
  (the JUCE keyboard's own piano colors, KeyboardWindow.cpp: none is a theme color; no theme property).
- Alpha steps on theme colors, everywhere (see below).

Shape (radius, edge, depth, sliders, marks, tabs, sections, rows); no screen read any of the eight `style` choices:
- Home: tiles `home.slint:34-41` (radius, 1px border, 2px cursor), icon chip `:44-48` (`min(12px, radius + 4px)`).
- Top bar: Performance pill `topbar.slint:147-149` (`min(height/2, radius)`), meter bars `:70` (6 px, not rounded), rule `:118-120`.
- Setlist: Button `setlist.slint:47-52` (radius only; no pill, no shadow, no edge), list rows `:90-96`, slots `:136-155`, playing edge `:146-151`.
- Play: scene tiles `play.slint:241-272`, strips `:107-111`, fader `:143-160`, thumbs none, meters `:51-85`, Mark `:99-111` (always `radius`, never round).
- Part: rows `part.slint:44-111`, the switch `:75-90` (12 px, 10 px radii), the info panel `:150-156`.
- Grid: SliderBar `grid.slint:96-137` (track 8 px, fill 8 px, thumb 22 px disc with an accent ring: one look for all three slider styles), Knob `:140-170` (6 px and 3 px strokes, no disc), Tile `:172-338` (radius, fields use `raised` not the field radius/edge), cards `:513-519`, tabs `:452-473`, pages `:424-449`, dropdown overlay `:547-598`, envelope panel `:340-382`.
- Keyboard: key caps `keyboard.slint:73-107`, text field `:53-72`. Piano: control keys `:71-95`, keys `:104-162`.
- Shell: engine banner `shell.slint:99-113`.

Paddings and spacings (px):
- Grid: `grid.slint:175-177` (pad 10, control row 8/8), `:183-195` name/value line y 8, `:204-217` control insets 11/4/10, `:224` knob 76 max, `:236` switch 110 max, `:255-264` dropdown text 10/36/24, `:282-289` stops 1/4/8, `:303-304` drag math 11/4/14, `:317` drag 150, `:342-343` panel title 14/6, `:371-374` envelope point 12, `:385` pages column 180, `:408-414` Back 110, `:439-445` page text 10/20, `:457` tab 28, `:492` heading 28, `:527-530` link lines 1/10/20, `:557` dropdown 420, `:564` `:587-588` 12/10/20.
- Setlist: `setlist.slint:70-72` list 280, footer 24, `:77-79` row 56, `:99-100` 12/24, `:148` playing edge 6, `:158-167` 14/6/64/10, `:187` 24.
- Play: `play.slint:114-116` (8, 20, 10, 18, 12, 7, 3), `:120-132` 10/8/20/6, `:139` 4, `:167-177` 12/8/7/10/16, `:181-192` 4/8, `:211` 110, `:219` 110, `:227-235` 110, `:250-258` 8/3/16, `:324-326` 300.
- Part: `part.slint:47` 4/56, `:54-65` 10, `:76-87` 48/24/38/22/20/2, `:92` 4, `:119-120` 6/16, `:131-139` 110, `:166-168` 10/8/20, `:176-185` 8/4.
- Home: `home.slint:21-23` 200/120, `:43-51` 12/48/24, `:57-69` 12/18/24.
- Top bar: `topbar.slint:11-16` columns by `small` (derived, fine), `:123-125` 6/10/8, `:130-131` 24, `:148-153` 36/12/24, `:163` 6.5 small, `:171-191` 8/4.
- Keyboard: `keyboard.slint:60-65` 1/8/16. Piano: `piano.slint:46` (4, 2, 6), `:97-101` 1/2/4/6, `:109` 1, `:144` 0.6, `:134` `:158` 3.
- Shell: `shell.slint:18-19` window 1280 x 720 (a preferred size), `:107-108` 12/24.

Font sizes: all through `Theme.small/body/button/title` (13, 16, 15, 20 times the scale: the
JUCE touch scale, `textSize`); no literal font sizes in the .slint files. `font-weight: 700`
appears wherever the JUCE UI draws bold. `Theme.touch` 44, `bar-height` 48, `gap` 4, `margin` 6 are constants.

Stroke widths: `grid.slint:133` 2, `:148` `:157` 6, `:167` 3, `:331` 2, `:367` 2, `:377` 2, `:433` `:460` `:581` 2; cursors 2 px in
every screen (`home.slint:40`, `setlist.slint:96,154`, `play.slint:203,270`, `part.slint:107`, `keyboard.slint:102`, `piano.slint:92,134,158` 3 px).

Alpha and brightness steps on theme colors: `grid.slint:122,156` (.25), `:356` (.8), `:432,459,580` (.75), `:550` (.3); `play.slint:57,148` (.65), `:154` (.35),
`:246` (.83), `:248` (.65), `:118` (.45); `part.slint:48` (.83); `keyboard.slint:80` (.6, brighter .15), `:81` (.4); `topbar.slint:120` (.35);
`piano.slint:112` (.5), `:146` (darker .3); `setlist.slint:144` (mix .85/.45).

Rust: `grid.rs:18-22` layout sizes (gap 6, tile 84, knob extra 36, heading 30, envelope 180: ParameterGrid's
rules, not theme values; the gap must be 0 in a joined theme: `sectionGap`), `grid.rs:520` (20), the knob arc
geometry in `grid.rs`; `theme.rs` defaults for missing colors (`apply`), the `Inter` fallback family.
No other Rust file draws.

## 2. After

### What loads and what it styles

Every property above is read by `ui/src/theme.rs` (`ThemeData::parse`, `load_folder`, `chosen`:
the reference's `parse`, `load`, `inside`, `reload` and `defaultId`, with its words) and set on the
window's `Theme` global by `theme::apply`: 11 colors (the nine and `warning`, `focus`), `radius`,
`fontScale`, `spacing`, the eight `style` choices (Slint enums), the six `wires`, `splash` (its two colors, and
its image when the theme's folder has one) and the font files. All 35 properties the reference
stores or checks (`version`, `id`, `name`, `font` x3, `fontScale`, `spacing`, `radius`, 9 colors, 8 style, 6
wires, 3 splash) are loaded and checked, and the two optional colors the reference accepts without
storing (`warning`, `focus`) are used.

Used: colors by every screen; `radius` through `Theme.section-radius` (radius + 2, 0 joined),
`button-radius`, `field-radius`, `mark-radius` (the reference's `buttonRadius`, `fieldRadius`, mark
corner); `fontScale` in the four type sizes; `spacing` (see "Spacing" below); `style.buttons` Button, text keyboard keys, Part's
step buttons, the grid's switch and split buttons, Home's icon chip, Back/Previous/Next; `edges`
every Section, Button, Field, key; `depth` the 1 px shadow under buttons, keys, split buttons,
thumbs, knob discs and the lifted tab; `sliders` the grid's sliders and range thumbs, the knob's disc
and pointer, Play's faders (a drawbar and the upright slider are the same widget); `tabs` the shell's
tab family (TabBar, TabPages: the parameter pages and Play's Performance / part switch; ui/TABS.md), its fill
and text colors derived by src/palette.rs `tabs` (the selected fill 3 to 1 from an unselected one); `marks` Play's M and S; `sections` every card, tile, strip and panel, and the gap between
them (the grid's layout reads it, Slint's screens use `Theme.gap-between`); `rows` the colors of
Setlist slots (a mark left or right, a dot, or the slot's fill with the reference's contrast rule,
`Theme.row-fill`); the piano keys' colors come from the text and surface inks as Panel.cpp's board
has them. `wires` and `splash` are carried and tested but no screen draws them yet (no routing view,
no splash in the Rust UI).

### Hardcoded values: gone, or named once

No .slint file but `theme.slint` has a color, a corner radius or a font size as a literal
(`no_screen_hardcodes_what_the_theme_sets` fails the test run otherwise). Left as named
constants in `theme.slint` (the `Theme` global), listed here as "no theme property": touch target 44
and bar height 48 (RFC 0019), a small button's width 110, the dropdown 420 and pages column 180, cursor 2 px, key cursor 3 px,
hairline 1 px, mark 6 px, thumb 16 px, knob arc 6 px, the alphas of theme colors (wash .25 and .17
and .4, track .35, hairline .6, slider track .55, dim .45, disabled .4), the piano key corner 4, the
type scale 13/16/15/20 (the touch scale; the desktop scale 12/14/13/18 is not in this UI yet). A
screen's own layout rule stays a named property at the top of its component (Home 200 x 120 and its
icon chip, a strip's meters, a switch 38 x 24, the heading 28, a knob at most 76, a drag of 150, the
envelope point 12, a band 4 high). `shell.slint`'s 1280 x 720 is a preferred window size. In
`grid.rs`, `SECTION_GAP` (6), tile 84, knob extra 36, heading 30, envelope 180 are ParameterGrid's
rules; the gap passed to `layout` is `theme::section_gap`. The dense gap 4 and margin 6 (Theme.h) and the
inner paddings 10, 12, 8, 4 were in this list; they are now scaled by `spacing`.

### Spacing

One optional number, `spacing`: a scale on the interface's paddings and gaps, 1 when omitted, 0.6 to 1.6
with both ends in, refused outside it ("Theme metric is out of range", "Theme metric must be a number"
for a non-number, as `fontScale`). Not a per-widget key and not a choice (the reference has no
tight/normal/roomy choice anywhere: its dense and touch gaps follow the device, RFC 0019, so a scale on
top is the one shape that fits). Both readers refuse and default identically; the tests pin the same
values: 0.6 and 1.6 in, 0.59 and 1.61 out, a string, null and a boolean refused (the JUCE test adds 0, -1, 100 out and 1 in). Joined sections still have no gap.

Rust UI: `Theme.spacing`; times it, `Theme.gap` (4), `margin` (6), `pad` (10), `pad-wide` (12),
`pad-narrow` (8), `pad-tight` (4), `theme::section_gap` (the grid's 6, so the grid lays out again when
only the spacing changes), the envelope plot's room under its title (`grid::envelope_top`) and Home's
tile height (120 plus twice the padding's growth, so the chip, name and line keep their room). Kept
fixed, as sizes of things and not space: touch target 44, bar height 48, button width 110, dropdown 420,
pages column 180, cursor and hairline widths, mark 6, thumb 16, knob arc 6, a knob at most 76, a drag of
150, the switch 38 x 24, the heading 28, a strip's meters 7 and pitch 10, the top bar's columns (they
follow the type), a Setlist slot row 56, list width 280, the piano's bands, the grid's tile 84, knob
extra 36, heading 30 and envelope 180. A screen that counts how many rows or strips a view holds
reads it when it is shown, and again when that count changes: the Setlist (`SetlistState.resized`), the
grid (`GridState.resized`), Play (`PlayState.resized`) and the Part, Chain, Sounds and Scene lists
(`PartState.resized`, which the shell hands to the one shown), so a live theme switch or a window resize
lays them out again at once (src/tests/layout.rs). The cursor drawn is one: while the top bar has it
(`Theme.bar-cursor`, set by the shell, not by a theme) a `Cursor` draws nothing unless it is the bar's
own or an overlay's (`always`).

JUCE app: `spaced(px)` in Theme.cpp scales a padding by the theme's `spacing`. Applied to `sectionGap`
(so the gaps between sections in PlayView, PartsView and the parameter grid) and Play's outer
padding and the space between its controls. Not applied: `denseGap` and `denseMargin` used elsewhere,
every other literal padding, row height and inset in the JUCE app (it is being replaced), and anything
that does not go through those; the JUCE views do not lay out again by themselves on a live switch.
Checked in renders only on the Rust side (the odd themes: "Odd" spacing 1.6, "Odd joined" 0.6, in
`ui/target/theme-shots`); the JUCE layout was compiled, not looked at.

### Where the reference's behavior is not (yet) matched

- `fontScale` 0.8 and 1.4 exactly were refused by both readers (the reference compared the double with
  float bounds); fixed in both: the ends of a range are in it, as docs/theme.schema.json's `minimum` and
  `maximum` say.
- A partial file is refused whole, not merged over the default (the previous Rust loader merged and
  clamped; the reference does neither). What a file may omit is `fontScale`, `radius`,
  `colors.warning`, `colors.focus`, `font.bold` and `splash`.
- Font checking is the reference's container check (`fontContainer`); not JUCE's
  `createSystemTypefaceFor` and the 'A' glyph test. A splash image is checked for its form (PNG
  signature, an `<svg` root) and size, not decoded.
- Cursors are rounded in the shape of what they mark (`focus` color, 2 px), not the reference's square
  accent `drawRect`. Rows apart (Setlist, Part, chain and sounds lists) are drawn as sections, not as
  `drawListRow` bands (alternate row lift, the colored row only where a theme has a row color).
  Tabs are drawn on the parameter pages and Play's knob row; marks on Play only. The reference's hover shades are drawn on
  buttons; text boxes and the Settings widgets do not exist in the Rust UI yet.

## 3. Operator colors (the FM view)

The FM operator view needs eight colors a player can tell apart, one per operator, the same for operator n in a theme
every time. The theme format has six wire colors (one is a gray, and a theme may repeat them) and one accent, which is
too few distinct usable colors, so the palette is derived (`ui/src/palette.rs`, the `Theme.operators` and
`Theme.operators-on` properties `theme::apply` sets; no theme file key, nothing stored):

- Space: OKLCH (lightness, chroma, hue as the eye ranks them). Operator 1 takes the accent's hue; operator n the hue
  turned by 45 degrees times n - 1, so the eight are evenly spaced round the wheel.
- Chroma: the accent's, at least 0.12 (a gray accent still gives colors), reduced where sRGB cannot hold it.
- Lightness: the accent's, kept in 0.3 to 0.86 (past that hues stop differing). Every second operator is 0.07
  toward the text's side of the panel (lighter on a dark panel, darker on a light one; away from it when the accent is
  already at the end of the range), so neighbors on the wheel differ in lightness too. Then each color is moved
  toward the text until it has 3 to 1 against the panel (the surface) and 4.5 to 1 against the text drawn on it, as
  far as the range allows.
- The text on a fill (`operators-on`): the theme's text or background, whichever contrasts more.
- Dimmed (an operator at level 0): the color at the theme's dim alpha (0.45).

Measured by `tests/palette.rs` in the built-in themes and the two odd ones (OKLab distance between the two nearest
colors, least contrast against the panel, least text on a fill; the test fails under 0.07, 3 and 4.5): least distance
0.113 (block), 0.115 (classic), 0.113 (dark), 0.080 (light), 0.100 (odd), 0.090 (odd joined); least contrast on the panel
7.91, 5.96, 7.49, 5.12, 4.85, 7.19; least text on a fill 8.61, 6.89, 8.61, 4.52, 10.00, 6.24 (run 2026-11-15). Color is never the only mark: every box, cell and line end carries the number.
