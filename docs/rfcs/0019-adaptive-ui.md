# RFC 0019: One UI across desktop, touch panel and small screens

Status: in progress, 2026-11-09. Step 1 done; steps 2, 3, 4 and 5 started (see [Progress](#progress)); Panel screens of any size and density built ([Screens of any size](#screens-of-any-size)). Builds on the views and plugin editors of
[RFC 0011](0011-performance-ui-and-editors.md), the control API
([RFC 0003](0003-control-api.md)) and the layouts and themes of the
[design language](../design-language.md).

## Progress

Checked when built and tested; the sections below say how.

- [x] 1. Surfaces and view models ([status](#step-1-status))
- [ ] 2. Desktop shell: menu bar, toolbar, parts list, tabs and detached windows done; dockable
      panes with a saved arrangement, the inspector and shortcuts remain
- [ ] 3. Generated editors: parameter pages from module paths, `oscairn.ui` draft hints, panel
      row knobs, the Compact test layout and the gate's checks done (Odin 2 on Desktop, Panel
      and Compact); the editors for the engine plugins (Oscairn FM, Analog and Sampler) remain,
      once their engines are done
- [ ] 4. Plugin windows: built on macOS, from workers, reopened after a restart
      ([status](#step-4-status)); the gate's editing while audio plays, Linux and an engine-placed
      plugin remain
- [ ] 5. Rich widgets: envelopes, key and velocity maps and FM operator views that plugins
      describe (`oscairn.ui` draft 4), drawn and edited on Desktop and Panel, done, with LFO
      previews and filter responses by name ([status](#step-5-status)); waveform, step lane, the
      Modular grid and the gate (the Sampler's zones and envelope, once it declares them) remain
- [ ] 6. Front panels

Not a numbered step: [screens of any size](#screens-of-any-size), a Panel laid out at each
device screen's own size and density, built and checked by `desktop-gaps` (2026-11-09).

## Problem

Oscairn runs on very different screens:

- **A desktop or laptop**, with a large screen, a keyboard and a mouse, and sometimes a
  touchscreen. It should show the most information and the most options, and look and work
  like a desktop app, as Gig Performer does: menus, panes, several windows, shortcuts,
  right-click menus, drag and drop, and a plugin's own window when it has one.
- **A device's touch panel**, such as the KICK's 1024 × 600 screen with knobs and buttons
  around it. It must be enough to design sounds without a computer, in a touch interface
  like the MODX's and the Montage M's: tabs, large targets, the knobs following what is on
  screen.
- **A small screen**, such as a guitar pedal's, with a few encoders and perhaps no touch.
  We don't design it now, but nothing we build should stop it.

Today there is one JUCE app. The window is built from a device layout
(`hosts/standalone/layouts`), and the desktop layout is the device screen stretched, with
knobs and pads around it. Its screens are sized for 1024 × 600 and touch. Some of their
state is already kept apart from drawing and tested on its own (`PartsModel`,
`SoundPickerModel`); most is not. Plugin editors are generic knob pages. No plugin window
can be opened yet.

If each plugin and each screen is built once per surface, three surfaces triple the work, and
third-party plugins never reach the small ones. If one UI is stretched across all of them,
the desktop feels like a device and the pedal can't show it at all.

## Outcome

- **Surfaces:** a *surface* is a class of UI: **Desktop**, **Panel** (a device touch screen
  with physical controls) or **Compact** (a small screen with encoders). A layout names its
  surface. The desktop app can open any of them, so each can be tried without the hardware.
- **Shared view models:** every screen's state and actions live in view models that know
  nothing about drawing. Each surface draws them its own way. A change made on one surface
  shows on the others, because all of them go through the control API.
- **Plugin editors described, not drawn:** a plugin describes its editor by meaning: pages,
  groups, parameters with roles and priorities, and a few rich widgets. Each surface draws
  the description to suit it. Every CLAP plugin gets a description generated from its
  parameters; first-party plugins and willing third-party plugins write better ones. This
  is the "description format" RFC 0011 leaves for later.
- **The desktop looks like a desktop app,** with more views and options than the device, and
  an input mode: **Keyboard and mouse**, **Touch** or **Automatic**.
- **Compact is designed for, not built:** descriptions carry enough priority and structure
  that a small-screen renderer can be added later without changing any plugin.

## Surfaces

| | Desktop | Panel | Compact (later) |
| --- | --- | --- | --- |
| Example | Mac or Linux computer | KICK, Ignite, a MODX-style keyboard | Guitar pedal, rack unit |
| Screen | Any size, resizable, several windows | Fixed, any size and density (800 × 480 to 1280 × 720 so far), touch | About 320 × 240 down to 128 × 64; maybe no touch |
| Input | Keyboard and mouse, or touch (input mode) | Touch, the device's knobs and buttons | A few encoders and buttons |
| At once | Several panes side by side | One main view, with a strip of tabs and quick controls | One list or one parameter |
| Plugin editor | Description, generated pages, or the plugin's own window | Description or generated pages | The description's top-priority parameters, as a list |
| Built by | The desktop app | The desktop app full screen, on the device's Linux image | A renderer to be chosen |

A fourth surface, the **web UI** served by a device to a phone, tablet or laptop, draws the same
view models ([RFC 0026](0026-network-control-and-web-ui.md)); it is the only screen of a
headless product. Which surface a product uses, and its layouts, follow from its device profile
([RFC 0024](0024-device-profiles.md)); text comes from catalogs ([RFC 0029](0029-internationalization.md)).

### Desktop

Gig Performer is the model for how it feels. Not a device screen in a window, but:

- **A menu bar and toolbar,** with every action in a menu and a keyboard shortcut for the
  common ones. Edit already has undo and redo.
- **Panes** for Parts, Part, Mixer, Routing, the plugin editor, the browser and an inspector
  for the selection, which can be docked side by side, tabbed or closed, with the
  arrangement saved per user. Routing and Mixer can be open while the Part page is edited.
- **Several windows:** a plugin editor or the mixer can go in its own window, on a second
  screen.
- **More options than the device:** everything the device has, plus things a small screen
  hides behind pages, such as all of a node's ports in Routing, wire levels in a table, and
  every parameter of a plugin in one scrolling list with search.
- **Desktop habits:** right-click menus, drag and drop (plugins from the browser into a
  chain, wires in Routing), multiple selection, hover help, double-click to edit, scroll
  wheel and fine adjustment with a modifier key.
- **The plugin's own window,** through CLAP's GUI extension, as a window of its own beside
  the app's ([Plugin windows](#plugin-windows)); embedded in a pane later, where the platform
  allows it.
- **Front panels, later:** a performance can have panels of knobs, sliders and buttons the
  user places and binds to any parameter, like Gig Performer's panels. The layout format
  already places controls and binds them (`"bind": "synth:1"`), so a front panel is a
  layout saved with the performance, not a new format.

The device views remain available on the desktop: View > Layout opens the KICK, the Ignite or
any layout, for testing and for people who prefer them.

### Input mode

The desktop's input mode is a per-user, per-device preference, like the theme:

| | Keyboard and mouse | Touch |
| --- | --- | --- |
| Control size | The design language's 32–36 pixels, dense | At least 44 pixels (on a Panel, 44 design units: at least 6.5 mm, [Screens of any size](#screens-of-any-size)), with more spacing |
| More actions | Right-click menu, hover help | Long press, an info button |
| Adjust a value | Drag, scroll wheel, a modifier for fine steps, type a value | Drag with larger travel, a fine-adjust button, the on-screen keyboard to type |
| Lists | Scroll wheel, keyboard focus and arrow keys | Swipe with momentum |

**Automatic** chooses Touch on a computer whose only pointer is a touchscreen, and
otherwise Keyboard and mouse. Both modes keep keyboard shortcuts and multi-touch. The mode
changes size and gestures, not what the panes show: a touch laptop still gets the desktop's
panes and options. Panel uses Touch sizes always.

### Panel

This is the device experience RFC 0011 describes, and the mockups reviewed on 2026-10-30
show:

- one main view at a time, chosen from tabs (Parts, Part, Mixer, Routing, Editor);
- large targets and full-screen editors for one part or one plugin at a time;
- the physical knobs following the parameters on screen, by priority, with their names and
  values shown next to them;
- every action reachable by touch, with no right-click or hover.

### Screens of any size

Decided and built 2026-11-09. Panels come in many sizes: the KICK and the Ignite at 1024 × 600
(about 7"), the FANTOM-07 at 1280 × 720 on 5.5", a Kronos-sized 800 × 600 on 8", the MODX M's
800 × 480. The Panel is drawn for none of them in particular:

- **A design unit** is a pixel at the KICK's density (about 6.7 pixels per millimeter). The
  **minimum touch target** is 44 units, and physically **6.5 mm** across, on any screen.
- **Density from the profile:** a screen declares its diagonal in millimeters
  ([RFC 0024](0024-device-profiles.md)), which gives its pixels per millimeter. Its scale, pixels
  per design unit, is what keeps 44 units at least 6.5 mm, and never below 1: the FANTOM-07's
  10.5 pixels per millimeter give 1.55, the KICK, Ignite, MODX M and an 8" 800 × 600 stay at 1
  (their targets are 6.6 to 8.9 mm). A screen of unknown size stays at 1. On a device, the scale
  is JUCE's global scale, so dialogs and menus follow it too.
- **Reflow, not stretch:** a screen-only layout (`kick-screen`, `modx-m-screen`) is laid out at
  the screen's size in design units, its pixels over its scale (the FANTOM-07's 1280 × 720 is
  826 × 465 units), and fills it with no bars. The views over it lay themselves out from the space
  they get: Home's tiles and the editor's tiles in as many columns as fit, Play's strips scrolling
  sideways past six, the Library's search on a row of its own when narrow, Settings in two
  columns from 720 units wide, and the Part page scrolling (on the Desktop too) when its
  settings are taller than the screen. No layout needs breakpoints of its own
  yet; a layout says what goes where, and the views decide how it fits.
- **Touch sizes:** in Touch (every Panel), the controls a finger uses are at least 44 units
  across: parameter tiles' sliders and lists, Play's faders and mute and solo, the Parts list's
  mute and level and its scene buttons, the Part page's controls and its chain's add buttons.
- **Dense by design** (2026-11-13, after the FANTOM-07 showed the Library, the Catalog and
  Settings with a few huge rows): a device's screen keeps every touch target at 44 units
  (6.5 mm; not lower, for lists too: a keyboard player's finger is no smaller there) and spends
  nothing else. `setDense` marks the window (and the Settings app's frame when its profile's
  screen is not a desktop); the views then use `denseGap` (4 units) between controls and
  `denseMargin` (6) round their content, and a list's row is one touch target tall, on one line:
  the Library and the Catalog (which fills the Library under its dialog's title bar), the
  Settings list. Settings shows its list and the page side by side from 720 units wide, Home's
  tiles go four across at 120 units tall, and Play's rows close up. The desktop, in Touch too,
  keeps its spacing. On the FANTOM-07 the Library shows 7 rows where it showed 4, the Catalog 6
  where it showed 4.
- **The screen's theme:** a profile's screen may name a theme (`"theme": "oscairn-block"`, the
  FANTOM-07's: square, joined sections, no space between them), shown until the user chooses
  one, before the built-in default; the apps that are processes get it from the shell
  (`OSCAIRN_SCREEN_THEME`).

`desktop-gaps` renders every built-in device profile's screen at its size and density, and an
800 × 600 one at 8", over the Panel of its layout: Home, Play, the Parts list, a part's page, each
page of an editor with rich views (the views test plugin), the Library, the Catalog (sized as it
opens over the Library, without its dialog's title bar, and with no repository read) and Settings.
Each fills the screen, and no control falls outside what its parents show (a scrolling view's
content aside), is under 6.5 mm across at that density, or overlaps another. Each screen is
dense and in its profile's theme; on the FANTOM-07 the Library and the Catalog show at least 6
rows, the Groovebox has Play and Stop on the screen, and with the transport playing a Stop
shows in the top bar, touch-size, and stops it. A fader swept there as the panel's reader plays
it (`Session::controllerControl`, a step every 4 ms) is timed to Play's strip, with what each
frame invalidates; it shows within 50 ms on average and a frame redraws under a tenth of the
screen. The Desktop window is
checked the same way at 1,440 × 900 and 1,024 × 640 (and Touch at 1,024 × 640), without the
physical minimum, since a computer's density is not known. Given a path it writes
`SNAPSHOTS-screen-<size>-<view>.png`. Not yet: the Mixer, Routing and Controls pages on a Panel
(their mutes, sends and faders are not all 44 units yet); a split's lane buttons; the scale on a
device's own screen, which only the hardware shows.

#### One window on a device's screen

Built 2026-11-12, after the FANTOM-07 showed the app under its own display server (Weston with
Xwayland, whose window manager places and decorates X windows): the window came up offset, with
a border showing what was behind it and its right and bottom edges off the screen, and the
plugin browser and the Catalog came up as windows of their own with a close button too small to
tap. A profile whose screen's `surface` is not `desktop` (a panel or compact screen) now gets
one window and nothing else:

- **The window fills the display under any window manager.** On X11 it is at the display's
  origin and size, undecorated (`_MOTIF_WM_HINTS`), and asks for full screen in EWMH's way:
  `_NET_WM_STATE_FULLSCREEN` in its `_NET_WM_STATE` before it is mapped, and asked again on the
  root window once it is (JUCE 9's `setFullScreen` and kiosk mode only set the bounds on Linux,
  or maximize a window with a native title bar). Oscairn's images have no window manager and
  the window is where it is put; Weston's, a desktop's or any EWMH window manager makes it full
  screen, without the shadow margin Weston puts round other windows. It has no border of its
  own, and the layout follows the size the window is actually given. The splash does the same.
- **Everything else opens inside it.** The plugin browser (the screen's Plugins button) is an
  app on the screen, under the top bar, like the Library; the Library's pick (Load Sound, a
  setlist's Add) opens over the app that asked, in a frame whose Back cancels. Dialogs (the
  Catalog, Install, Export, Import, the Details of an app's log), alerts and text prompts are
  added to the window instead of the desktop (`keepInScreen`, the theme's `createAlertWindow`),
  centered and no larger than it, a dialog's title bar 44 units with only its close button,
  alerts' buttons 44 units tall; menus open inside it too. The apps that are processes (Settings
  and Setlist, RFC 0022) are placed in the frame under the top bar, at the shell's scale, and on
  X11 as a child window of the shell's, which no window manager moves, decorates or puts behind
  it; their own dialogs open inside them. Plugins' own windows are not built on Linux.
- **Not converted:** file choosers (Import sample or SFZ on the screen, the plugin browser's
  Install theme and Add folder, the Library's Export, Install from a file and a plugin's build
  for another platform). JUCE asks zenity or kdialog where one is installed, a window of another
  program's, and otherwise draws its own dialog as a window of its own; neither has a place on
  an instrument, which needs a file browser of its own for USB storage
  ([RFC 0037](0037-storage-and-file-transfer.md)).

`desktop-launch` checks it from the real app, off screen: with the FANTOM-07's profile, at Home,
in the Library and in the plugin browser, there is one top-level window, full screen and
undecorated (on the desktop, the Library is a second window). `desktop-gaps` puts the Library,
in its frame, on a screen of the FANTOM-07's size and opens the Catalog, an alert and a menu: each
opens inside it, nothing goes on the desktop, and Back, the Catalog's close and the alert's
buttons are 44 units on the screen. Under Weston 13 (headless, desktop shell without a panel)
with Xwayland in the CI image, by hand with `xprop` and `xwininfo` at 1280 × 720 and the
FANTOM-07's profile: before, the window was 32 pixels inside Weston's frame (its shadow margin),
1280 × 721, with no `_NET_WM_STATE`, and Settings a window of its own at 819 × 410; now it is at
0, 0, 1280 × 720, `_NET_WM_STATE_FULLSCREEN`, and Settings a child of it at 0, 74 (under the top
bar), 1280 × 646. On the FANTOM-07 itself (Weston 6.0.1), untried.

### Text entry

A screen with no typing keyboard says so in its profile (`textEntry: "onScreen"` on the screen,
[RFC 0024](0024-device-profiles.md); absent: a keyboard is there, as on every desktop, Touch mode
included). There, any editable text field that takes focus brings up a keyboard docked along the
bottom of its window, full width, five rows of touch-size keys (`TextKeyboard.h`, one component for
the shell and for the Settings and Setlist apps, which the shell tells through
`OSCAIRN_TEXT_ENTRY`): the Library's and the Catalog's search, the Save, Save As, Edit, Export
and signing-key names, the setlist, page and slot names, the repository address and the plugin
browser's searches. Nothing at those fields changes: the keyboard listens for focus, so a field
added later gets it.

It types through the editor's own key handling (`keyPressed`), so search-as-you-type, an input
filter, a length limit and a password mask act as for real typing; Done presses Return in the
editor and then in what holds it (an alert's default button), Cancel puts back the text the editor
had when the keyboard came up, and Clear empties it. It goes on Done, on Cancel, when focus leaves
the field, and on a tap on anything that is not a text field or the keyboard. The keyboard may
cover the field, so a line above the keys shows the text, its selection and caret (masked for a
password), and a dialog it covers moves up while it shows. Keys: letters, Shift (one letter) and
Caps, a numbers and symbols layer (`. , / : ; - _ @ # & ' " ? ! ( ) % +` and the digits), Space,
Backspace (it and the cursor keys repeat while held), the cursor keys, Clear, Cancel and Done. Each
key is named for the accessibility tree.

Skipped: other layouts than US QWERTY, autocorrect, suggestions, swipe typing, accented letters
and a numeric pad (no field here wants only digits yet). `text-keyboard` checks it on the
FANTOM-07's screen size (the Library's search filtering as keys are tapped, Shift, Caps,
Backspace, the cursor keys, the symbols layer, Done and Cancel, an alert over the Setlist pane),
and that a desktop profile shows nothing. Not tried on the device.

### Compact

Not built now. To keep it possible:

- every plugin description gives each parameter a priority, so the most important 4–8 can be
  shown alone and mapped to encoders;
- groups and pages give a menu structure that works as a list, one level at a time;
- every action in a view model has a text label and works without touch or a pointer;
- the system screens a pedal needs (choose a sound, edit its top parameters, save, tuner)
  are small, and are written for that device when it exists, from the same view models.

A test layout in the desktop app at 320 × 240 with four encoders, showing a plugin's
description as Compact would, is the check that this holds (step 3). It is built: View >
Layout > Compact (`layouts/compact.json`), see [Step 1 status](#step-1-status).

## Plugin editor descriptions

A plugin provides a description through an Oscairn CLAP extension, `oscairn.ui`, MIT
licensed like the other extensions
([RFC 0001](0001-license.md)). It is data, not code: the host draws it in the system theme,
on every surface, with the physical controls mapped. Without the extension, the host
generates one.

**Contents:**
- **Pages and groups,** in order, each with a name: "Oscillator", "Filter", "Amp".
- **Parameters,** by CLAP parameter ID, each with:
  - a **priority** (1 is most important), which decides what Panel puts on the knobs first
    and what Compact shows at all;
  - an optional **role** (cutoff, resonance, attack, level, pan …), which lets the host map
    physical controls by default ([RFC 0011](0011-performance-ui-and-editors.md#defaults-so-nothing-must-be-programd))
    and use the right widget;
  - an optional **widget hint**: knob, slider, switch, choice, or a field in a rich widget.
- **Rich widgets,** a small fixed set the host draws on every surface that has the room, each
  bound to parameters or to plugin operations (draft 4 describes the first three over
  parameters, [Step 5 status](#step-5-status)):
  - an envelope (attack, decay, sustain, release, with optional more segments, or the rates or
    times and levels of a DX7, AWM2 or FM-X envelope);
  - a keyboard and velocity map (sample zones, splits, key level scaling);
  - an FM operator view (the algorithm's routing, each operator's level, ratio and envelope);
  - a step lane;
  - a module grid ([chillbeast's Modular](../modular-engine-evaluation.md));
  - a waveform with markers (sample start, loop).
- **Operations** for edits that aren't parameter changes, such as loading a sample into a
  zone or moving a module: named actions with arguments, sent through the control API
  ([RFC 0011](0011-performance-ui-and-editors.md#plugin-editors)), never pixels.

**Generated descriptions:** one page per CLAP module path (`clap_param_info.module`), or one
page for a plugin with none; priority in the plugin's parameter order; roles guessed from
names only where unambiguous ("Cutoff", "Resonance", "Attack").

**How surfaces use it:**
- Desktop shows every page at once in a scrolling editor, with the rich widgets large.
- Panel shows one page at a time, rich widgets full width, knobs bound to the page's
  parameters by priority.
- Compact lists the pages, then each page's parameters by priority, one at a time.

## Plugin windows

Decided 2026-11-01. A plugin's own window is shown by **the process that hosts the plugin**:
its worker ([RFC 0010](0010-plugin-worker-isolation.md)), which is where plugins run in the
desktop app, or the engine for a plugin placed there. The app, a separate process, asks for
it through the control API (`window.show`), and the engine passes the request to the node's
host.

**Why not in the app's window:** macOS cannot put one process's view inside another's
window; there is no counterpart of X11's embedding there. And running plugins with windows in
the engine instead would give up isolation for exactly the plugins most likely to crash (a
third-party GUI), in a process that runs headless. So:

- **A window of the hosting process's own.** Plugins that embed their editor in a host window
  (CLAP's `set_parent`, which every JUCE-based plugin needs: Odin 2 and Airwindows Consolidated
  offer nothing else) get a plain titled window made by the CLAP host, sized as the plugin
  asks and resizable where the plugin is. Plugins that offer only a floating window of their
  own get it, with the title suggested (`suggest_title`). Embedded is preferred when a plugin
  offers both, so the host's window closes and titles the same way for every plugin.
- **The title names the node and its part,** as the app shows them ("Odin 2 · Lead").
- **No `set_transient`:** it needs the parent window in the plugin's own process, and the
  app's window is in another. The window is an ordinary one of its own.
- **The worker joins the window server only when a window opens.** A worker is a plain
  process; the first window gives it an application object (no Dock icon) and, from then on,
  it runs the platform's window events on its main thread between the engine's requests,
  waking as soon as a request arrives.
- **A window lives as long as its node.** It belongs to the node's plugin host, which destroys
  it before the plugin, so whatever removes the host (removing the node, replacing its plugin,
  loading another Performance) closes it; the person closing it destroys the plugin's editor;
  a worker restarted after a crash opens it again, as its engine last asked.
- **Linux: the device's Wayland shell** ([RFC 0022](0022-apps-and-requests.md#what-an-app-is),
  decided 2026-11-07) gives a worker showing a plugin's own window a surface of its own, as it
  gives apps: natively for plugins that speak Wayland (`CLAP_WINDOW_API_WAYLAND`), and through an
  X server of the worker's own for X11 plugins. Until it is built, on Linux and Windows only
  plugins that float their own window show one. A plugin's own window adds to its generated
  pages and declared widgets, never replacing them: the device's knobs and encoders always reach
  its parameters.

## Toolkit

**Desktop and Panel stay on JUCE.** The app is already JUCE, the device image already boots
into it on X11 (`os/buildroot/board/common/session-juce-x11`, so far in QEMU only), and JUCE 9
has multi-touch on Linux through XInput2 ([architecture](../architecture.md#touch)). One
codebase draws both, from the same view models and theme.

**Compact's renderer is chosen when a device needs it.** A small screen on a weaker processor
may not run X11 and JUCE well. Because the descriptions and view models don't depend on JUCE,
it can be something lighter, such as LVGL drawing straight to the display, without changing a
plugin.

**To check first:** JUCE's frame rate and touch latency on the KICK's i.MX8M Mini, under audio
load. If it fails, Panel's renderer changes, and the view models and descriptions do not.

## Build order and gates

| Step | Deliverable | Gate |
| --- | --- | --- |
| 1. Surfaces and view models | The surface a layout names; the input mode preference; the Parts, Part and Mixer views on view models, drawn for Desktop and Panel | The view models' tests run without drawing; each view renders in Desktop (both input modes) and in Panel at 1024 × 600 |
| 2. Desktop shell | Menu bar and shortcuts, dockable panes with a saved arrangement, the inspector, a second window | Parts, Mixer and an editor open side by side and edit one Performance; the arrangement survives a restart |
| 3. Generated editors and the description | `oscairn.ui` header and generated descriptions; Desktop, Panel and a Compact test layout drawing them; Basic Synth and Legacy Sampler descriptions | Basic Synth, Legacy Sampler and one third-party plugin editable on Desktop and Panel without a plugin window; the Compact test layout reaches each one's top parameters with four encoders |
| 4. Plugin windows | CLAP GUI on the desktop: floating windows, embedded where supported, and from worker processes | A third-party plugin's window opens, edits and closes without stopping audio, also when its worker restarts |
| 5. Rich widgets | Envelope, key and velocity map, waveform, step lane; then the Modular grid (RFC 0011 step 5) | The Sampler's zones and envelope edited by touch on Panel and by mouse on Desktop |
| 6. Front panels | User-placed controls bound to any parameter, saved with the Performance, on Desktop | A panel of controls for three plugins, built and played without the plugins' editors |

Steps 1–3 come before RFC 0011's step 5, and cover its knob pages.

## Step 1 status

Started 2026-10-31. A layout names its surface (`"surface": "desktop"`), and the desktop
layout is now the Desktop surface (`hosts/standalone/DesktopShell.cpp`):
- a toolbar with the tempo (the clock every plugin follows), the scene and Store, the audio
  device and MIDI inputs, and undo and redo as icons;
- the parts and buses down the left, each with what plays and from where, mute and solo
  marks, a meter per channel of its sound and a mark where its fader sits;
- tabs for the selected part, the Mixer, Routing and Controls. Each tab, or
  any tab from its right-click menu, opens in a window of its own as well.

The Groovebox is an app, not part of the sound editor: it opens from the Apps menu in a window
of its own, with its Play and Stop, which start the sequencer, not the window. A part's
meters have one bar per channel of its sound, its last plugin's main audio output as wired to
where it plays (`parts.meters`' `channels`), after its fader and pan. On the desktop, text
follows a type scale by role and every control shares the theme's corners
([design language](../design-language.md)); the parts views take tighter rows with a mouse.
The input mode (View > Input: Automatic, Keyboard and mouse, Touch) is saved beside the
theme selection; Automatic means keyboard and mouse for now. `desktop-shell` builds a small
Performance, plays a chord, checks the meters move and renders every tab in both input modes
(to PNGs when given a path). The device layouts are unchanged.

On the desktop the Part page's settings are a grid of tiles, each its name above its control,
several to a row, the last in a row widening to fill it; device screens keep one row each.
Themes choose how controls and sections are drawn (format 3: Soft, Classic and Block, the last
with sections joined edge to edge).

Step 3 started: selecting a plugin on the desktop's Part page shows all its parameters as
pages of tiles (`ParameterGrid`), built from their CLAP module paths, which `params.list` now
carries. `desktop-shell` renders the Basic Synth's pages, and those of any plugin given on its
command line (the FM and analog engines were checked: 595 and 294 parameters). A page of more
than 24 parameters gets a tab per second-level module, and a search box finds parameters on
any page by name or module. Step 5's envelope widget is drawn where a module's parameters
are named like an ADSR, a DX7 envelope or an FM-X one, until plugins can describe theirs;
dragging a point sets its time and level. LFOs get a preview of their waveform the same way.
The desktop's Part page edits keys and velocities on a keyboard (`ZoneEditor`): drag either
end of a range or the whole range; split lanes have the keys. The parts model now reads and
sets a part's velocity range.

Step 1's gate met, 2026-10-31. The views read their state from data that has no drawing in
it: `readParts` (the Parts list, Mixer strips and meters), `readRouting` and `partFields` (the
Part page's settings, in order and width), all in `PartsModel` and tested by
`desktop-parts-model` without a window. The Part page draws those fields as one grid of tiles
on every surface, with the keyboard, parameter pages, envelopes and LFOs; the device's own row
layout and key lists are gone, and its Parts view uses the touch sizes. `desktop-shell` renders
every view on the Desktop in both input modes, and `desktop-pad-editor` renders them on the
Panel at 1024 × 600.

Since then: parameters with 2 to 4 values are rows of buttons (Mono | Poly), lists hold up to
64 values (filter types), and a module with a cutoff and resonance gets its filter's response
drawn for the type its value text names.

**From the MODX's screens (2026-10-31), still to build:**
- *Knobs and ranges where a plugin wants them:* done in `oscairn.ui/draft-2`, now draft 3
  (`oscairn-clap/ui.h`), a hint per parameter: knob, slider, buttons, switch, list or range
  (with its high parameter), and a priority. Draft 3 adds a role per parameter (cutoff,
  attack, an effect's mix) for [RFC 0011](0011-performance-ui-and-editors.md#step-6-status)'s
  role banks, and a device's screen knobs play the editor page shown, its first parameters by
  priority (Desktop, Panel and Compact). The host reads the hints with the parameters'
  info, keeps each only where it fits, carries them through workers and `params.list`, and
  the pages draw them and order a section's tiles by priority. The Basic Synth gives them.
  `kick-clap-host` checks the host's checks on a fixture's good and bad hints, `worker-host`
  that they cross a worker, `control-json-protocol` the protocol, and `desktop-shell` the
  Basic Synth's knobs and order. A range's two-sided slider is drawn but not yet checked by a
  test; no plugin gives a range yet.
- *Linked tiles:* tiles that belong together (a hint's group, or an envelope's, LFO's or
  filter's parameters recognized by name) are one run, in order (Attack, Decay, Sustain,
  Release), drawn edge to edge in one card with a line between each. A run that does not fit
  what is left of a row starts the next one (the tile before it widening to the row's end); a
  run wider than the screen goes on in rows that touch, its last tile filling the row. The
  grid has as many columns as the width allows, two to six. `desktop-shell` checks the Basic
  Synth's linked LFO, and given the engines, that the analog filter envelope's stages stay
  together at 1,180 and 560 pixels and that no controls overlap on any of their pages.
- *Physical knobs following the screen* (step 6): done for parameter pages. A layout's knob
  bound `row:N` plays the Nth parameter of the line selected on screen; the panel counts them
  and works out the screen's edge they sit beyond. Knobs above or below the screen play a row,
  left to right; knobs beside it play a column, top to bottom. On a parameter page a tap on a
  tile selects its line (the first until then), its tiles take a tint and a border in the
  accent, and a strip along that edge
  names each knob's parameter (and its value, when the strip runs down a side). A device
  without such knobs shows no strip. On device layouts the Parts views now sit over the
  screen, scaled with it, so the panel's knobs stay in reach. The KICK's P1 to P4 and the
  concept keyboard's Mod 1 to 3 are row knobs. `desktop-shell` checks, on a panel with four
  knobs right of its screen, that the first column is theirs, that tapping a tile in the next
  column moves them there, that turning P1 sets its first parameter, and that the line and the
  knobs go with their page and panel; and on a panel with knobs under its screen, that they
  play the first row. These are sound-editing controls: what the knobs do while playing, with
  no page open, is RFC 0011 step 6's defaults. Not yet: rows on the Part page, Mixer and
  Routing; defaults for the knobs when no page is open; endless encoders' relative turns.
- *No fixed effect sends or EQ in the Mixer:* sends go to the Performance's own buses, and an
  EQ is a plugin put wherever it is needed (RFC 0016).

**The Compact test layout (2026-11-01):** a layout's `"surface": "compact"` draws its panel
as a device's, with its screen showing `CompactEditor` instead of the device screen: the
Performance's plugins as a list, then a plugin's pages, then a page's parameters by priority,
four at a time, as a column above each of four encoders (its name, its value as the plugin
writes it, and a bar). Knobs bound `encoder:N` are those encoders: in a list, turning chooses
and pressing encoder 4 opens; on a page each turns its parameter (a value per detent when
stepped, else a hundredth of the range) and pressing encoder 4 shows the next four; pressing
encoder 1 goes back. It draws the same description as Desktop and Panel: the pages and
priority order come from one function (`editorPages`, `byPriority`) that `ParameterGrid` uses
too. `desktop-shell` runs step 3's Compact gate on `layouts/compact.json`, with the encoders
alone: for the Basic Synth, the Legacy Sampler and, where `OSCAIRN_THIRD_PARTY_CLAP` names one (the
build sets it to Odin 2 beside the checkout when it is there; the test says when it skips),
a third-party plugin, it chooses the plugin from the list, checks that every page's encoders
hold its top four parameters, turns each and sees the value change, and pages on with encoder
4. Checked with Odin 2 (258 parameters, one page). The Legacy Sampler has no parameters yet (it is
edited through its sounds and the Part page's zones), so its part of the gate shows it as
having none. Loading Odin 2 needed a host fix: the CLAP host checked the plugin's functions
before `init`, which plugins built on clap-helpers only fill in during `init`, and then
destroyed it uninitialized, which aborts them.

**Editors for real plugins (2026-11-01):** checked with Odin 2, whose 258 parameters have no
module paths and come in its IDs' order (the host lists parameters by ID). A plugin of more
than 24 parameters with no module paths now gets them from its names' leading words, in name
order (`editorParameters`): "Osc2 Detune" is Detune on the Osc2 page, "Arp Step 6 Transpose"
Transpose on the Arp page's Step 6 tab; a word leading one name only stays in it. Odin 2 gets
26 pages, and its envelopes are recognized and drawn. The page list scrolls when it does not
fit, tabs wrap onto more rows, and a value text that is a bare number with more than three
decimals ("0.0100000", JUCE's default) is shortened ("0.01"). `desktop-shell` checks the
names' modules on a made-up plugin and, for each gate plugin, every page and tab on the
Desktop (1,180 × 820) and on a Panel's screen (1,024 × 600) with no controls overlapping, and
that a slider edits it (Odin 2: 26 pages; 87 of its pages' top parameters turned on Compact).

**Pages that change with a mode (2026-11-01):** a plugin whose mode hides or renames
controls calls CLAP's info rescan. The CLAP host now takes such a rescan in place (names,
modules, the hidden and periodic flags, and the `oscairn.ui` hints), without deactivating the
plugin, where it used to restart it; the session tells clients the graph changed when a
plugin's parameter info does, so remote editors read it again; and the parameter pages and
Compact compare the info twice a second and rebuild, keeping the page shown where it still
exists. `kick-clap-host` checks that the test fixture's mode hides its Toggle by an info
rescan without a restart, and `desktop-shell` that the Desktop's pages and Compact drop the
Toggle in mode 3 and show it again in mode 0. Plugins in worker processes do not pass such
rescans on yet.

Not yet: the Mixer and Routing views' layout is still in `PartsView`, and the device's chain
row, tabs and header are the older ones; Compact's real renderer waits for a device (by
design).

## Step 5 status

Started 2026-11-07. **The description** is `oscairn.ui` draft 4 (`oscairn-clap/ui.h`): a
parameter's hint names the view it is part of, its field there and whose field it is (an
operator, element or zone, 1 to 8). What a view is follows from its fields:
- *an envelope:* named stages (delay, attack, hold, decay, sustain, release: an ADSR or
  DAHDSR) or a multi-point envelope's levels 0 to 8 with each segment's time or rate (DX7,
  AWM2, FM-X), holding before its last segment, one envelope per index;
- *a key or velocity map:* each index's zone (key and velocity low and high), and an index's
  breakpoint, left and right depths and curves, velocity depth and curve;
- *an FM operator view:* an algorithm (stepped, up to 128 values) and feedback, and each
  operator's level, ratio, fine, detune and fixed mode, with its envelope and key scaling as
  envelope and map fields of its index. A call gives each algorithm's routing (the operators
  modulating each operator, the carriers, feedback from and to), so the host draws any
  engine's algorithms without knowing them.

The host keeps only fields it knows, operators up to the eighth and a stepped algorithm, reads
the routings with the hints, carries them through workers, and `params.list` writes them
(`view`, `field`, `index`, `algorithms`). Draft 5 adds a call giving how long an envelope stage
takes at a value (`seconds`), which the host reads with the hints for each time or rate field
at its values (every value of a stepped one up to 128, else 33) and carries the same way;
without it, the times come from value texts with units ("31.2 ms", "1.20 s"). Drafts 3 and 4
are still read, without views and without `seconds`.

**Drawn** by `ParameterGrid` on Desktop and Panel, where the view's parameters' tiles are, above
them; every parameter keeps its tile, and the device's knobs still bind to parameters, never to
a view. An FM operator view shows where its algorithm is: the routing (carriers on the bottom
row, each modulator over what it modulates, feedback as a loop) and a column per operator with
its level bar, its ratio and its envelope's shape; a tap on the routing's half steps the
algorithm back or on, a drag across it runs through them, a level bar drags, a ratio drags up or
down. An envelope is the curve drawn before, now from its fields, its stages by time where the
plugin gives their seconds: each segment's room filled on a log scale (about even below 50 ms)
up to the longest any of its stages can take, past a least width that keeps an instant stage's
point apart to grab, a drag mapped back through the same scale; else by the parameters' ranges,
a rate reversed. A key map draws its zones over
a keyboard (the index's own in the accent), the scaling around the breakpoint by its curves'
value texts (`-LIN`, `+EXP`), and velocity's effect; each edge, the breakpoint, the scaling's
ends and the velocity line drag. Every drag sets parameters as the tiles do, with a gesture, so
each change is undone as theirs are. Name recognition stays for parameters in no view.

**Checked:** `kick-clap-host` reads the fixture's views (an unknown field, an operator past the
eighth and a continuous algorithm are dropped) and its routings, and the same fixture offered as
draft 3 gives its hints and roles without views, and as draft 4 its views with its times from
its value texts, where draft 5's `seconds` gives them otherwise; `worker-host` that they cross a
worker. `desktop-gaps` reads ADSR, DX7 and AWM2 envelopes and DX7 algorithm 1's layout without
drawing, then loads a test plugin (`tests/views_plugin.cpp`: an FM view with DX7's 32 routings,
six operators' envelopes and key scaling, two AWM2-style elements' zones and envelopes) on the
Panel's screen at 1,024 × 600: every parameter still has its tile; a tap on the routing steps the
algorithm and undo steps it back; dragging operator 2's level bar to its top sets it to 99;
dragging the breakpoint moves only it; dragging element 2's zone's left edge lowers only its
lowest key; stages are drawn by time (an ADSR in log2 seconds, as Surge's: its 4 ms attack
narrower than its 31 ms release, narrower than its 1 s decay, the attack still apart; a DX7
envelope's rates 80, 60, 40, 60 narrower the faster), dragging the release point sets only the
release, the point landing where it was dropped, and dragging a DX7 envelope's first point
slows only rate 1, each undone; and the Desktop at 1,180 × 820 draws the same views. Given a
path, it writes `SNAPSHOTS-views-*.png`, with each page that has an envelope as the plugin
starts, also for a real engine named by `OSCAIRN_TEST_VIEWS` (its test plugin's own checks then
fail).

**The Basic Synth** declares its envelope this way: four new parameters (Attack, Decay, Sustain,
Release, defaulting to the envelope it had fixed) on an Envelope page, an ADSR view with the
stages' roles. `desktop-gaps` finds the curve and the stages' tiles linked in order on that page
on the Panel, its 2 ms attack drawn narrower than its 250 ms decay (times from its value texts,
"0.002 s"), drags the attack point (only Attack changes) and the decay point down (Sustain
falls, Decay stays), and undoes that; `control-json-protocol` checks the view in `params.list`;
`wheels` that a long attack and full sustain change the note, and that a sound saved before the
four parameters loads with the envelope it had.

Not yet: the real engines' declarations (their forks declare draft 3 roles; what each should
declare is in the [porting guide](../plugin-porting.md#rich-views-for-oscairns-engines));
a pitch envelope's levels around a center; Compact draws no views (it lists their parameters).

## Step 4 status

Started 2026-11-01, as decided in [Plugin windows](#plugin-windows):
- **The CLAP host implements the GUI extension** (`core/clap-host/src/Host.cpp`): it picks
  embedded in a window of its own on macOS (`src/Window.mm`, Cocoa) or else floating; creates,
  titles, shows and destroys on the main thread; sizes the window from `get_size`, makes it
  resizable when `can_resize` says so and passes resizes through `adjust_size` and `set_size`;
  and carries out `request_resize`, `request_show`, `request_hide`, `closed` and
  `resize_hints_changed`, those made off the main thread at its next service. Unloading
  destroys the editor before the plugin.
- **Workers show it:** a request (`Op::Window`) opens or closes it; from the first window the
  worker runs Cocoa's events between the engine's requests, woken by the engine's socket; a
  restarted worker opens the window again with the same title.
- **The control API** has `window.info` and `window.show`
  ([control protocol](../control-protocol.md)); the desktop offers **Open plugin window** on a
  selected plugin in the Part page and in Routing, titled with the plugin and its part, only
  when the plugin has a window.

Verified: `kick-clap-host` drives the fixture's floating window through creating, titling,
showing, the plugin's hide and show requests, closing, the plugin destroying it and unloading;
`worker-host` opens, retitles and closes it in a worker, which keeps answering;
`control-json-protocol` checks a plugin without one; `desktop-shell` checks the button shows
for the fixture's effect in the Part page and Routing, not for the Basic Synth, and that the
window opens and closes through a worker. By hand, Odin 2 and Airwindows Consolidated opened
their own editors in a worker's window, titled as asked, and Odin 2's came back after its
worker was killed and restarted.

Not yet: the gate's editing in the window while audio plays (no audio was played); a plugin
placed in the engine's process (the headless engine has no window events of its own); X11
embedding on Linux; `set_scale` (not needed on macOS).

## Open questions

- Whether a device's touch panel ever needs the desktop's panes (a large touchscreen
  keyboard), or whether Panel's one-view model always fits.
- How Desktop and Panel share one editing session when both are open on the same engine
  (also open in RFC 0011).
- Embedding a worker's plugin window in the app's window on Linux, through X11 (XEmbed); on
  macOS it stays a window of its own ([Plugin windows](#plugin-windows)).
- What Compact needs from the controller-mapping service when it has no touch.
- Following CLAP's own work on remote controls (`clap.remote-controls`), which covers part of
  the priority and paging above; the description should use it rather than duplicate it
  where it is enough.

## Ruled out

- **One UI stretched across every screen:** the desktop would feel like a device, and the
  pedal could not show it.
- **Plugin-drawn editors on devices:** as in RFC 0011, devices draw descriptions.
- **Code in descriptions:** they are data, so a plugin can't break or slow the host's UI.
- **Separate per-surface plugin editors written by each plugin:** third-party plugins would
  not write three.
