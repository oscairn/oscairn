# Panel layouts

A layout describes a device's screen: its size and where the screen sits. The
desktop app (`oscairn-standalone`) builds its window from one. Built in:

| File | Layout |
| --- | --- |
| `desktop.json` | The desktop app's own window (the Desktop surface): toolbar, parts list, and tabs for the part, Mixer, Routing and Controls; apps such as the Library open from the Apps menu |
| `kick-screen.json` | A device's screen alone, designed at the KICK's 1024 x 600 and stretching to any screen: on the device, the hardware panel supplies the controls. The KICK, the Ignite, the FANTOM-07 and the test boards show it |
| `modx-m-screen.json` | The MODX M profile's screen alone, designed at 800 x 480 (an estimate) and stretching the same way |

Others can be opened with View > Open Layout File... or `--layout path.json`.

## Format

```json
{
  "name": "Desktop",
  "size": [1280, 800],
  "resize": "stretch",
  "panels": [ {"rect": [870, 80, 370, 470], "label": "SOUND", "anchor": "right top"} ],
  "controls": [
    {"type": "screen", "rect": [40, 80, 800, 469], "anchor": "stretch stretch"},
    {"type": "label", "rect": [890, 110, 105, 20], "label": "SOUND", "anchor": "right top"}
  ]
}
```

- `surface` ([RFC 0019](../../../docs/rfcs/0019-adaptive-ui.md)): `panel` (the default), a
  device's screen drawn from `panels` and `controls`, or `desktop`, the desktop app's own
  window with menus and tabs, which uses neither and may leave `controls` out. Its controls
  follow the **View > Input** setting: Keyboard and mouse, Touch (larger targets) or Automatic.
- `size`: design units. The window starts with one unit per pixel when it fits. `rect` is
  `[x, y, width, height]`. A design unit is a pixel at the KICK's density (1024 x 600 at 7",
  about 6.7 pixels per millimeter), where a 44-unit touch target is 6.6 mm across.
- `resize`: `scale` (the default: the window keeps the panel's proportions and the panel
  scales with it; right for a real device's panel), `stretch` (any window shape: the panel
  scales to fit the tighter dimension and the space left over in the other goes to
  elements by their `anchor`) or `fixed`. On a device, a stretching layout is laid out at the
  screen's own size in design units: its pixels over the profile's scale
  ([RFC 0024](../../../docs/rfcs/0024-device-profiles.md)'s `diagonal` gives the density; a
  screen dense enough that 44 pixels would be under 6.5 mm scales up, others stay at one unit
  per pixel). The space it has may then be less than `size` as well as more, so a screen-only
  layout (`screen` anchored `stretch stretch`) fills any screen with no bars, and what is on it
  reflows: the views lay themselves out from the space they get (more columns where there is
  room, fewer where there is not, scrolling what does not fit). No breakpoints are needed in
  the layouts so far; the views choose their columns from their width.
- `anchor` (panels and controls, used by `stretch`): a horizontal and a vertical part,
  `"left|right|center|stretch top|bottom|center|stretch"`. `right bottom` keeps the
  element's distance from the right and bottom edges; `stretch stretch` grows it with the
  window. The default is `left top`.
- `panels`: background areas with a label.
- `controls`, by `type`:
  - `screen`: the display (at most one). The apps over it (Home and the rest) lay themselves
    out at its size in design units.
  - `label`: text; `"align"` is `left` (the default), `center` or `right`.

  An unknown type stops the layout loading, with the reason.

Colours, fonts and control styling come from the system [theme](../../../docs/design-language.md).
Layouts define geometry; they do not override the selected theme.
