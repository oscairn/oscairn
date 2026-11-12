# RFC 0042: Sliders that follow a part, and organ drawbars

Status: proposed, 2026-11-11. Builds on the physical controls, control groups and banks of
[RFC 0011](0011-performance-ui-and-editors.md#physical-controls), the device profiles and their
controller profiles ([RFC 0024](0024-device-profiles.md#control-groups)), Sounds as saved parts
([RFC 0021](0021-sounds-performances-and-library.md#what-a-sound-holds)), the Play screen
([RFC 0011](0011-performance-ui-and-editors.md#play-the-performance-apps-first-view),
[RFC 0019](0019-adaptive-ui.md#plugin-editor-descriptions)) and the control protocol's control
targets and pickup ([control protocol](../control-protocol.md#requests-and-responses)).

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. The engine reports each physical control's last reported position beside the value it
      plays (`controllers.get` and the `controllers` topic); Play shows both for a control under
      pickup
- [ ] 2. A `controls` fader mode: the sliders follow the selected part's own controls through the
      existing mode machinery, a Play button switches between the mixer and it, part selection
      keeps up, and the default stays part levels
- [ ] 3. `oscairn.ui` draft 6: a plugin declares its drawbars (which parameters, their order,
      their direction); Play draws a drawbar row for the selected part with all of them, value
      and physical position each
- [ ] 4. The sliders and the first free assignable knob take the declared drawbars in overflow
      order; selecting an organ part reverses the sliders and selecting another part un-reverses
      them
- [ ] 5. A Performance and a scene can pin the sliders to one part's controls; a nine-fader
      controller profile maps all nine; the drawbar row meets the design language's keyboard and
      touch rules

## Problem

Keyboards such as the Roland FANTOM-07 and the Yamaha MODX M have eight sliders
([fantom-0.md](../fantom-0.md) counts Roland's own spec's eight; `hosts/standalone/profiles/
fantom-07.json` and `modx-m.json` both declare a group of eight faders). A tonewheel organ needs
nine drawbars. The two products solve the ninth two different ways, and the owner has felt both
on stage: the MODX splits an organ across two parts, and reaching the ninth drawbar mid-performance
is a pain; the FANTOM puts the ninth on its screen.

What exists today, checked in the code:

- Play's knob row already follows either the Performance's controls or the selected part's own
  controls, through a two-button switch that appears when a part is selected (the tooltips in
  `PlayView`'s constructor, `hosts/standalone/PlayView.cpp`; the owner chosen in
  `PlayView::refresh` and sent to the engine there with `setKnobPart`; `showPartControls` in
  `hosts/standalone/PlayView.h`). Sliders have no such switch: Play shows no slider row of their
  own, only part strips whose faders are part levels (the strip class at the top of
  `PlayView.cpp`).
- The engine already has one fader mode that follows a part: `element` plays the part's Levels
  bank (`bankOf(..., BankKind::Levels, ...)`, `core/control/src/physical_controls.cpp:243`),
  the parameters a plugin gives `OSCAIRN_ROLE_LEVEL_1` to `LEVEL_8`. The part is the one
  `controllers.view` names, which is the knobs' part (0 while they follow the Performance).
  Nothing in the app moves a fader group's mode today: Play sends a position only for the knob
  group (`PlayView::refresh`), so the MODX M's `element` and `audio` modes are reachable only by
  a client sending `controllers.view` `positions`.
- Physical faders default to part levels — parts first, then buses
  (`core/control/src/physical_controls.cpp:72-76`, `:239-252`) — and a fader group's `modes`
  (`part`, `element`, `audio`) already make it page between what it plays: `groupPositions`
  counts the modes (`physical_controls.cpp:194`) and the mode at the group's position decides the
  bank (`:242`). The mode names are validated in `hosts/standalone/Profile.cpp:198` and only
  faders may have them (`:207`). Fader groups are per control identity `fader:N` across the
  device and its controllers ([RFC 0024](0024-device-profiles.md#control-groups)).
- Pickup for absolute controls is per control and per target, and not saved
  ([RFC 0011](0011-performance-ui-and-editors.md#the-controller-mapping-service));
  `ControllerMapping::play` keeps, per control and target, whether the control has caught up and
  the last position the control sent (`Pickup`, `core/control/src/controller_mapping.cpp:137-147`).
  That last position is private to the pickup check: nothing reads it back, and nothing keeps a
  control's position apart from a target.
- The engine's control state is group positions, the control touched last and a move count
  (`controllers.get`, [control protocol](../control-protocol.md#requests-and-responses), and
  `ControllerInput::state` in `core/control/include/oscairn/controller_input.h`). **The engine
  does not report a physical control's last position.** `controls.changed` pushes the controls'
  values and the part levels, and `params.changed` the parameters' values: what plays, not where
  the hardware sits. [architecture.md](../architecture.md#physical-controls-takeover-and-direct-position-access-planned)
  plans the physical position as one of three values a mapping keeps; that value is not built.
- `oscairn.ui` draft 5 (`core/clap-ext/include/oscairn-clap/ui.h`) is the plugin's say on how its
  parameters are shown and played: a widget, a priority, a group, a role, and a rich view with a
  field and an index (1 to 8) per parameter. There is no drawbar widget, no drawbar view and no
  direction; the closest roles, `OSCAIRN_ROLE_LEVEL_1` to `LEVEL_8`, are element and operator
  levels, eight wide.
- A control's target can already run backwards: `end` below `start` reverses it
  ([control protocol](../control-protocol.md#requests-and-responses), `MacroTarget::start`/`end`
  in `core/control/include/oscairn/parts.h:225-227`). That range belongs to the control tree's
  targets only: a physical control resolves to an `Assignment` with no range
  (`ResolvedControl`, `core/control/include/oscairn/physical_controls.h`), so a slider playing a
  parameter directly cannot be reversed today.
- A Sound is a saved part and carries its own controls and assignments
  ([RFC 0021](0021-sounds-performances-and-library.md#controls-owned-by-a-part)); a `Part` has no
  slider- or drawbar-related field (`core/control/include/oscairn/parts.h:182-201`).
- Values can go back to hardware where a profile says so (`feedback: true` on a group's `midi`,
  [RFC 0024](0024-device-profiles.md#controllers)); how motorized faders are declared is still
  an open question there. Many keyboards have neither lights nor motors on their sliders (the
  owner, 2026-11-11), so the player cannot see where a silent slider sits.

So several pieces are there: fader modes, part-following, pickup, and reversed ranges on a
control's targets. What is missing is a slider mode that hands the sliders to the selected
part and a button for it, a way for a Sound to say "these nine parameters are my drawbars, in
this order, pulled down is louder", a direction on a physical control, a position report, and
the screen row that shows it.

## Outcome

Eight-slider keyboards play nine-drawbar organs without splitting them in two: the sliders
default to the mixer, one switch hands them to the selected part's own controls, and an organ
Sound's declaration puts its nine drawbars on them in order — the ninth, 1', on the first free
assignable knob — while the screen always shows all nine, each both at its value and at where
the physical slider sits. Selecting an organ part reverses the slider direction; selecting
another part puts it back. Nothing guesses that a Sound is an organ: the Sound declares what it
has, and the player's selection decides who the sliders follow (decided 2026-11-11).

## Design

### What a Sound declares, and where

The declaration belongs with the parameter description the plugin already gives, not beside the
part. RFC 0021's part controls are assignable values a *musician* edits
(`controls` in the Performance format); drawbars are the *author's* statement about parameters
that already exist, the same kind of fact `oscairn.ui` carries for envelopes and FM operators.
A parallel declaration on the part would be a second mechanism saying what a hint already says
per parameter. So: extend `oscairn.ui` ([RFC 0019](0019-adaptive-ui.md#plugin-editor-descriptions))
to draft 6:

- `OSCAIRN_FIELD_DRAWBAR` (a new `oscairn_field` value): a parameter that is one drawbar of a
  drawbar view, its `index` (1 to 9) the drawbar's number, 16' first down to 1'. Parameters with
  the same nonzero `view` and a DRAWBAR field are one drawbar set; the index is the order. This
  reuses the view machinery as it works today (a view is data, and what it is follows from its
  fields), so a host that does not know draft 6 draws the parameters as ordinary sliders
  (`OSCAIRN_WIDGET_SLIDER` already asks for one).
- Direction rides on the hint struct as one new field at the end, the way drafts 4 and 5 added
  `view`/`field`/`index` and `seconds()`: `reversed`, for a drawbar a slider plays upside down,
  pulled down for more of it, the tonewheel convention; the parameter's own value still runs
  from low to high. Per parameter, so a
  set may hold one reversed and nine normal, but for an organ all nine say the same thing.

A Sound needs nothing of its own: a Sound *is* its part with its plugins
([RFC 0011 step 1](0011-performance-ui-and-editors.md)), so the part reaches its drawbars
through the plugins in its chain, and the direction comes with them — which is why direction is
per part and not a global setting (decided 2026-11-11, as on the MODX). A part whose chain
declares no drawbar view is not an organ, to everything in the system.

### Who the sliders follow: a mode, not a new mechanism

The sliders default to the mixer — part levels, then buses — as they do today
(`physical_controls.cpp:239-252`; decided 2026-11-11). Handing them to a part is a new **fader
mode**, `controls`, which resolves like a knob row follows a part: the selected part's banks,
its declared drawbars first when it has them. The mode machinery already carries everything
this needs:

- `modes` lists what a group can do, `groupPositions` counts it, and `GroupPositions` (the
  per-group page/row/mode, `core/control/include/oscairn/control_groups.h:56-58`) already holds
  which mode is active. `Profile.cpp:198` and the `controllers.set` group
  ([control protocol](../control-protocol.md#requests-and-responses)) accept the new name
  beside `part`, `element` and `audio`.
- The player switches modes with a **slider-mode button** in Play, beside the knob row's
  existing Performance/part switch (same place, same look; the mode is sent to the engine with
  `controllers.view`'s `positions`, as `PlayView::refresh` sends the knob group's page).
  While in `controls` mode, selecting a part moves the sliders with it. `controllers.view`'s
  `part` is the knobs' part today, 0 while they follow the Performance, so step 2 names the
  sliders' part in `controllers.view` beside it: the sliders follow the selected part whatever
  the knob row follows.
- A device whose panel has a real mode button (the MODX's mode keys, the FANTOM's CTRL/ASSIGN
  buttons) moves the group through its profile's `modes` and the screen follows. Whether a
  panel's button reaches a group's mode today is to check: nothing in the app moves a fader
  mode yet, and the FANTOM-0's panel controls are still estimated (`fantom-07.json`). Profiles
  that want the new mode list it: `modx-m.json` becomes
  `["part", "controls", "element", "audio"]`; `fantom-07.json`, whose sliders say
  `modes: ["part"]`, gains it too, its mode being the on-screen button.
- The mode a session is in is the engine's, like positions today: kept, pushed, not saved —
  until a pin saves it (below).

### Overflow: sliders first, then knobs, then the screen

A part may declare more slider-style controls than the device has sliders. Distribution is by
order, always the same rule (drawbars are just its first case):

1. The device's sliders, `fader:1` up, take the declared controls in index order.
2. Further declared controls go to assignable knobs — the first ones not already taken by a
   declared control — in identity order. Decided 2026-11-11 ("option A"): with eight sliders,
   sliders 1–8 take drawbars 1–8 and the 9th, 1', goes to the first free assignable knob. That
   knob plays the drawbar in place of what its bank gives it, as an override does.
3. Anything past that is screen-only.

Decided 2026-11-11 ("option D", on top of A): **the screen always shows all nine**, wherever
the physical controls landed. The knob copy and the slider copy both move the same target, so
both stay true.

- **Nine sliders** (an external nine-fader controller, a future product): the profile declares
  `count: 9`, the rule maps all nine, no knob takes one. Controller profiles already join the
  device's groups and number on after them
  ([RFC 0024](0024-device-profiles.md#controllers)), so an eight-slider keyboard plus a one-fader
  unit is nine sliders too; `fader:N` identity numbers across groups make that work unchanged.
  A Mackie Control port's eight faders are not a fader group: the app plays them as the first
  eight parts' levels (`Controllers`, [RFC 0024](0024-device-profiles.md#mackie-control)), so
  whether they join `controls` mode is to check.
- **No sliders** (the KICK, the Ignite, `kick.json` and `ignite.json` declare no fader group):
  step 1 is empty and all nine go where step 2 and 3 send them — the first assignable knobs,
  then the screen. Nothing special-cases this; it is the same loop. The KICK's four assign
  encoders take drawbars 1 to 4; the Ignite has no assignable controls, so all nine are on the
  screen only.

### Direction, and multiple organ parts

Reversal is new on a physical control: the reversed range exists only on a control's targets
(`MacroTarget`, `core/control/include/oscairn/parts.h:225-227`), and a slider playing a drawbar
resolves to a plain `Assignment`. Step 4 gives `ResolvedControl` a direction, taken from the
drawbar's `reversed`, and `ControllerMapping::play` turns an absolute position `x` into `1 − x`
before pickup compares it. The screen's drawbar draws filled from the bottom for a reversed
one, so "pulled down = louder" reads the same on both surfaces.

Direction is taken from the part in focus — the part the sliders follow — because the part's
chain declares it. Selecting the organ part reverses the sliders; selecting a piano part puts
them back. With several organ parts, all in a drawbar chain, they agree or the selected one wins:
the sliders are one physical set and follow one part at a time, as the knobs already do.
A scene change is a part-selection change in this respect: scenes already store the mix and
control values ([RFC 0011](0011-performance-ui-and-editors.md#scenes-the-full-model-decided-2026-10-31)),
and a scene that pins the sliders (below) hands them to its part, its direction with it.

### Pickup, and the two-value display

With no lights and no motors, a silent slider's screen value tells the player nothing about
where their hand must go. Pickup is already right — the control plays nothing until it reaches
the target's value within 1/127 or crosses it (`controller_mapping.cpp:137-147`) — but the
screen cannot *show* the waiting, because the engine keeps the hardware position private.

What the engine must report: each physical control's last reported position, per control
identity, kept beside the pickup state (`Pickup::knob`, same file, has it today, but per
control and target). This is the physical position of
[architecture.md](../architecture.md#physical-controls-takeover-and-direct-position-access-planned)'s
three values: what the hardware last reported, before pickup. A relative encoder has none; a
control never moved is unknown, and the screen says so rather than guessing.

The control-API addition is additive, on the interface that already carries control state
([control protocol](../control-protocol.md#requests-and-responses)): `controllers.get` and the
`controllers` topic gain `places`, each `{control, position}` (0 to 1) for every control whose
position the engine has. `controls.changed` stays what it is — the controls' values and part levels — because
places move only when the player moves them, and the `controllers` topic already fires "a
control moved".

Play's drawbar row then shows, per drawbar: the parameter's value (from `params.changed`)
and a thin marker at the physical position
(from the `controllers` topic), until they meet; while the marker and the value differ, moving
the slider drags the marker and nothing sounds. The same two-value display applies to **any**
slider under pickup, not only drawbars, in every mode: a part strip's fader in the mixer shows
both too while its slider has not caught up (decided 2026-11-11).

### The pin

A Performance, or one of its scenes, can pin the sliders to one part's controls: an always-organ
Performance that opens in drawbar mode, a scene whose chorus hands them to the organ. The pin is
saved data, so it lives with the Performance's other mapping data — the assignments already save
per control identity and per part (`AssignmentOverride`,
`core/control/include/oscairn/parts.h:277`, Performance format version 13's `assignments`,
[performance-format.md](../performance-format.md)); a pin is the same shape naming a group's
mode and part instead of one control's target, and the next format version carries it
(initial-development policy: one version forward). The pin outranks the session's unsaved mode
while the Performance (or, for a scene pin, the scene) is in force; clearing the pin returns the
mode to the player's session choice. Nothing pins a drawbar *set*: the pin picks who is followed,
the Sound says what they follow.

### The screen row, and accessibility

Play gains a drawbar row: nine vertical bars in drawbar order (16' left to 1' right, the
tonewheel convention; the FANTOM's own screen layout to check when step 3 builds), each with
its name, its value as text, and the position marker described above. It replaces the knob
row's place while the sliders are in `controls` mode and the part has a declaration; the strips
underneath stay the mixer. It uses the theme's slider style like every
other control ([design language](../design-language.md)), and the design language's rules hold:
text for every value, state shown by position and text and not color alone, a focus ring, and
44-unit touch targets, at least 6.5 mm across (`design-language.md:11-25`). Keyboard: focus moves along the
row, arrows move a drawbar, and the value text names it, so the row works with no touch and no
mouse; each drawbar's accessible title is its name ("16' drawbar, 7 of 8").

## Build order and gates

| Step | Gate |
| --- | --- |
| 1 | `controllers.get` and the `controllers` topic carry each moved control's position; a test moves a fader, changes its target from another client, and reads both the held position and the moved value while pickup is waiting; pickup itself still behaves as `control-physical-controls` and `control-controller-input` say. |
| 2 | On the `modx-m` layout with three parts: the sliders play part levels by default; the Play button (or the profile's mode position) switches them to `controls` mode; selecting a part moves the sliders with it, also while the knobs follow the Performance; a resolved assignment test names each `fader:N`'s target in each mode. |
| 3 | A new organ test plugin (none exists yet; draft-6 hints: nine DRAWBAR parameters, direction set) makes Play draw nine drawbars for its part, each showing its value and, for a fader under pickup, its marker; a non-organ part shows the knob row as before. |
| 4 | On the eight-slider layout, sliders 1–8 drive drawbars 1–8 and one assignable knob drives the 9th, both it and the slider's screen copy reading the same target value; selecting the organ part reverses the sliders (a full-down fader plays the drawbar at its most, and pickup catches it from that side) and selecting a piano part un-reverses them; checked in `control-physical-controls` and by rendering. |
| 5 | A Performance pinned to the organ part opens in `controls` mode on a second engine over the socket, and selecting a scene pinned to another part hands the sliders to it; a controller profile with nine faders maps all nine with none to a knob; the drawbar row is reachable and operable by keyboard alone, and `desktop-gaps` renders it at the FANTOM-0's 1280 × 720 with 6.5 mm targets. |

## Open questions

- Whether a mode button that exists on the panel (the FANTOM's CTRL/ASSIGN) should also be
  assignable as a switch while it switches modes, or the mode always wins.
- How a reversed drawbar's value text reads on a plugin whose parameter runs the other way (0
  to 127 with 127 quiet): plain numbers, or a drawbar scale ("8 of 8", the classic eight-step
  click). The screen design (step 3) decides with a real plugin in front of it.
- Whether `OSCAIRN_ROLE_DRAWBAR_1` to `_9` should join the role banks, so a plugin may declare
  drawbars by role instead of a view. The view covers it; roles only matter if a second host
  wants them, to check when the first third-party organ Sound ships.

## Ruled out

- Guessing that a Sound is an organ from its name, its plugin, or nine slider-widget parameters:
  the Sound declares, the selection follows (decided 2026-11-11).
- A global "reverse sliders" setting: direction is the part's, through its Sound (decided
  2026-11-11).
- A declaration beside the part (a `Part` field listing drawbar parameters): a second mechanism
  for what `oscairn.ui` states per parameter, and it would not survive a plugin swap.
- A second fader-following mechanism beside the existing modes: modes already carry pages,
  engine state and profile validation.
- Motorizing or auto-aligning sliders at scene changes: most of these panels have no motors;
  the two-value display is the answer that works everywhere.
- Splitting an organ across two parts as the design's answer: that is the problem, not the fix.
