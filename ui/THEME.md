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
| `fontScale` | number | 1 | 0.8 to 1.4 | type sizes | yes (clamped, not refused) | `font-scale` | `Theme.small/body/button/title` everywhere; Play's Edit button width |
| `radius` | number | 6 | 0 to 16 | corners | yes (clamped) | `radius` | every rounded Rectangle |
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
