# RFC 0042: Sliders that follow a part, and organ drawbars

Status: proposed, 2026-11-11; revised the same day with the owner's decisions that a Sound
declares its own slider set and that a sampler's element levels are the same kind of set as an
organ's drawbars. Builds on the physical controls, control groups and banks of
[RFC 0011](0011-performance-ui-and-editors.md#physical-controls), the device profiles and their
controller profiles ([RFC 0024](0024-device-profiles.md#control-groups)), Sounds as saved parts
([RFC 0021](0021-sounds-performances-and-library.md#what-a-sound-holds)), the Play screen
([RFC 0011](0011-performance-ui-and-editors.md#play-the-performance-apps-first-view),
[RFC 0019](0019-adaptive-ui.md#plugin-editor-descriptions)) and the control protocol's control
targets and pickup ([control protocol](../control-protocol.md#requests-and-responses)).

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. The engine reports each physical control's last reported position, raw, beside the
      value it plays (`controllers.get` and the `controllers` topic); Play shows both for a
      control under pickup
- [ ] 2. The `element` fader mode follows its own part (`controllers.view`'s `faderPart`), a
      Play button switches the sliders between the mixer and it, part selection keeps up, and
      the default stays part levels
- [ ] 3. The slider set: `oscairn.ui` draft 6 (`OSCAIRN_ROLE_LEVEL_9` and `reversed`), a part's
      `roles` and `slidersReversed` in the next format versions, `parts.roles`, the part
      editor's **Sliders** step; Play draws the slider row for the followed part, value and
      physical position each
- [ ] 4. The sliders and the first free assignable knob take the set in overflow order;
      following a part with a reversed set reverses the sliders, and following another part
      puts them back
- [ ] 5. A Performance and a scene can pin the sliders to one part; a nine-fader controller
      profile maps all nine; the slider row meets the design language's keyboard and touch rules

## Problem

Keyboards such as the Roland FANTOM-07 and the Yamaha MODX M have eight sliders
([fantom-0.md](../fantom-0.md) counts Roland's own spec's eight; `hosts/standalone/profiles/
fantom-07.json` and `modx-m.json` both declare a group of eight faders). A tonewheel organ needs
nine drawbars. The two products solve the ninth two different ways, and the owner has felt both
on stage: the MODX splits an organ across two parts, and reaching the ninth drawbar mid-performance
is a pain; the FANTOM puts the ninth on its screen. The same sliders should also play a sampled
Sound's element levels, up to eight per part, as the MODX's do (the owner, 2026-11-11).

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
  the parameters a plugin gives `OSCAIRN_ROLE_LEVEL_1` to `LEVEL_8`. The part is the `part`
  `resolveControls` is given (`physical_controls.cpp:201`), which is the one `controllers.view`
  names: the knobs' part, 0 while they follow the Performance, and with no part the bank is
  blank (`bankOf`, `:182-189`). The Levels bank is eight wide, like every role bank
  (`RoleBank`'s `roles` array, `core/control/include/oscairn/physical_controls.h:59-64`), and
  is filled from non-effect nodes only. Nothing in the app moves a fader group's mode today:
  Play sends a position only for the knob group (`PlayView::refresh`), so the MODX M's
  `element` and `audio` modes are reachable only by a client sending `controllers.view`
  `positions`.
- Physical faders default to part levels — parts first, then buses
  (`core/control/src/physical_controls.cpp:72-76`, `:239-252`) — and a fader group's `modes`
  (`part`, `element`, `audio`) already make it page between what it plays: `groupPositions`
  counts the modes (`physical_controls.cpp:194`) and the mode at the group's position decides the
  bank (`:242`). The mode names are validated in `hosts/standalone/Profile.cpp:198` and only
  faders may have them (`:207`). `modx-m.json` lists `["part", "element", "audio"]`;
  `fantom-07.json` lists `["part"]`. Fader groups are per control identity `fader:N` across the
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
  plans three values a mapping keeps apart: the physical position (what the hardware reported,
  before takeover), the accepted mapped value (after takeover, scaling and range conversion) and
  the destination value. Only the last is reported today.
- `oscairn.ui` draft 5 (`core/clap-ext/include/oscairn-clap/ui.h`) is the plugin's say on how its
  parameters are shown and played: a widget, a priority, a group, a role, and a rich view with a
  field and an index (1 to 8) per parameter. `OSCAIRN_ROLE_LEVEL_1` to `LEVEL_8` (25 to 32) are
  "element, operator or oscillator levels, 1 to 8"; there is no ninth and no direction. The
  roles are a plugin's: a Sound cannot change them, and roles are guessed from exact parameter
  names only for a plugin that declares none (`physical_controls.cpp:345-347`), never a
  `levelN` (the guess table, `:298` on, leaves per-oscillator levels to priority).
- The two plugins this is first for, read in their forks (`forks/setBfree`, `forks/sfizz`, read
  only):
  - The **Oscairn Organ** (setBfree) has 27 drawbar parameters, upper, lower and pedal, nine
    each (`oscairn/params.h:104-130`), the upper manual's IDs 1 to 9 (16' first; IDs are the
    table's order from 1, `oscairn/plugin.cpp:851-856`). Each is stepped 0 to 8, 8 the loudest,
    with slider widgets and priorities but no role; its one role is `LEVEL` on Volume
    (`params.h:150`). So its drawbars are in no bank today.
  - The **Oscairn Sampler** (sfizz) exposes three parameters (`clap/plugin.cpp:1413`), and its
    only hint is the output bus's role (`:1495-1504`). Per-element parameters, Element Level
    among them as `OSCAIRN_ROLE_LEVEL_1 + n − 1`, are a proposal in its `doc/oscairn-plan.md`
    ("The element parameters the views need", not built), as are the AWM2-class elements
    themselves in [RFC 0013](0013-awm2-class-sample-engine.md).
- A control's target can already run backwards: `end` below `start` reverses it
  ([control protocol](../control-protocol.md#requests-and-responses), `MacroTarget::start`/`end`
  in `core/control/include/oscairn/parts.h:225-227`). That range belongs to the control tree's
  targets only: a physical control resolves to an `Assignment` with no range
  (`ResolvedControl`, `core/control/include/oscairn/physical_controls.h`), so a slider playing a
  parameter directly cannot be reversed today.
- A Sound is a saved part and carries its own controls and assignments
  ([RFC 0021](0021-sounds-performances-and-library.md#controls-owned-by-a-part)). Its document
  stores per-node data on each node (a node's `role` and `sound`) and the part's own structure in
  `part` (`chain` and `splits`) ([performance-format.md](../performance-format.md#sounds), Sound
  version 8). A `Part` has no slider-related field (`core/control/include/oscairn/parts.h:182-201`).
- Values can go back to hardware where a profile says so (`feedback: true` on a group's `midi`,
  [RFC 0024](0024-device-profiles.md#controllers)); how motorized faders are declared is still
  an open question there. Many keyboards have neither lights nor motors on their sliders (the
  owner, 2026-11-11), so the player cannot see where a silent slider sits.

So several pieces are there: a fader mode that follows a part, level roles, pickup, and reversed
ranges on a control's targets. What is missing is a ninth level, a way for a Sound (not only a
plugin) to say "these are my sliders, in this order, pulled down is louder", a button for the
mode, a direction on a physical control, a position report, and the screen row that shows it.

## Outcome

Eight-slider keyboards play nine-drawbar organs without splitting them in two, and play a sampled
Sound's element levels the same way: the sliders default to the mixer, one switch hands them to
the selected part's **slider set**, and the set is whatever the part's Sound declares — an
organ's nine drawbars, the ninth, 1', on the first free assignable knob; a two-element sample's
two levels, the other sliders dark — while the screen always shows the whole set, each entry
both at its value and at where the physical slider sits. Following a part whose set is reversed
reverses the sliders; following another part puts them back. Nothing guesses that a Sound is an
organ: the Sound declares what it has, the plugin's own declaration is the default, and the
player's selection decides who the sliders follow (decided 2026-11-11).

## Design

### The slider set: the part's levels, nine wide

A part's **slider set** is its Levels roles in order: the parameter with `level1` is slider 1,
`level2` slider 2, and so on up to `level9`, with one direction for the set. It is one model for
every case (decided 2026-11-11):

- an organ plugin's Sound: its drawbars, 16' as `level1` down to 1' as `level9`, reversed;
- an AWM2-class sampled Sound: its elements' levels, element order; a sampled organ whose
  elements are its drawbars reversed, a piano or a pad on the same plugin not;
- an FM Sound: its operators' levels, the same way.

Direction belongs to the Sound's set, never to a plugin or a kind of parameter: whichever plugin
plays it, a set is reversed because its Sound says so (the owner, 2026-11-11: "Sampler can have
its drawbars be reversed too").

This is the Levels bank and the `element` mode that exist, widened, not a second mechanism
beside them. The `LEVEL_n` roles already mean "element, operator or oscillator levels", and an
organ's drawbars are its footages' levels. What changes:

- **A ninth level.** `oscairn.ui` draft 6 adds `OSCAIRN_ROLE_LEVEL_9` = 36, after `OUTPUT_BUS`
  (35): `LEVEL_1` to `LEVEL_8` stay 25 to 32, so the ninth cannot follow them. The Levels bank
  becomes nine wide (`RoleBank`'s array grows to nine; the other banks keep a blank ninth, never
  shown, since a bank takes as many roles as its group is wide).
- **A direction.** Draft 6 adds one field at the end of the hint, the way drafts 4 and 5 added
  theirs: `reversed`, nonzero for a level a slider plays upside down, pulled down for more of
  it, the tonewheel convention; the parameter's own value still runs from low to high. The set
  has one direction, so the host reads it from the set's first parameter (`level1`'s hint), and
  a plugin sets it the same on all of them.
- A host that does not know draft 6 reads `LEVEL_9` as `NONE`, as the header already says for
  unknown values, and ignores `reversed`.

No drawbar view or drawbar roles: a drawbar is a level, and the screen draws the set the same way
whatever it holds (below).

### What a Sound declares, and where

The owner (2026-11-11): the Sound declares what it has, literally per Sound, with the plugin's
own declaration as the default. Two Sounds on the same plugin may differ (one organ preset
reversed, a pad on the same plugin not), and a plugin with no `oscairn.ui` at all can still
offer drawbars.

**The declaration is a role map on the part,** not an assignment and not a field per node:

- `roles`: a list of `{role, node, param}`, each naming a parameter by its node key and CLAP
  parameter ID and the role it plays in this part. `role` is any role name the host knows, as
  `params.list` spells them (`cutoff`, `attack`, `mix`, `level1` to `level9`, and RFC 0044's
  draft 7 names once they exist), except `outputBus`, which is the plugin's wiring and not a
  Sound's to change. Each role at most once in a part, each parameter at most once; the
  `levelN` entries run from `level1` with no gap, so slider *n* is always `level`*n*.
- `slidersReversed`: true when the slider set is reversed; absent, false.

Both sit on the part: a Sound's `part` (beside `chain` and `splits`) and a Performance's
`parts[]`. The part is where they belong, not a node, because a set has one direction and can
span the nodes of one part (never two parts: below), and because the slider and section controls already resolve per part. Node keys are the
document's own, as `chain` uses them, so loading a Sound into a part carries them over as its
chain is.

It is not an assignment. An assignment says what a physical control plays; a role map says what a
parameter *is*, and the physical controls then find it through the banks, the `element` mode and
RFC 0044's sections. That keeps RFC 0044's rule that a Sound's own assignments may not name
section identities: a Sound never names a control, only its parameters' roles.

An organ Sound on the Oscairn Organ, its upper manual's drawbars (IDs 1 to 9) as the set:

```json
"part": {
  "chain": ["node-1", "node-2"],
  "splits": [],
  "roles": [
    { "role": "level1", "node": "node-1", "param": 1 },
    { "role": "level2", "node": "node-1", "param": 2 },
    { "role": "level3", "node": "node-1", "param": 3 },
    { "role": "level4", "node": "node-1", "param": 4 },
    { "role": "level5", "node": "node-1", "param": 5 },
    { "role": "level6", "node": "node-1", "param": 6 },
    { "role": "level7", "node": "node-1", "param": 7 },
    { "role": "level8", "node": "node-1", "param": 8 },
    { "role": "level9", "node": "node-1", "param": 9 }
  ],
  "slidersReversed": true
}
```

An eight-element Sampler Sound (the Element Level IDs are not built; `1000 n + 14` is assumed
from the fork's proposal, `1000 n + k` per element, to check when the Sampler gains them):

```json
"part": {
  "chain": ["node-1"],
  "splits": [],
  "roles": [
    { "role": "level1", "node": "node-1", "param": 1014 },
    { "role": "level2", "node": "node-1", "param": 2014 },
    { "role": "level3", "node": "node-1", "param": 3014 },
    { "role": "level4", "node": "node-1", "param": 4014 },
    { "role": "level5", "node": "node-1", "param": 5014 },
    { "role": "level6", "node": "node-1", "param": 6014 },
    { "role": "level7", "node": "node-1", "param": 7014 },
    { "role": "level8", "node": "node-1", "param": 8014 }
  ]
}
```

A two-element Sampler Sound lists `level1` and `level2` only: two live sliders, the other six
dark.

A sampled organ on the Sampler, its elements its drawbars (eight here, so eight sliders and no
overflow; a set longer than the sliders overflows by the rule below, whatever plays it):

```json
"part": {
  "chain": ["node-1"],
  "splits": [],
  "roles": [
    { "role": "level1", "node": "node-1", "param": 1014 },
    { "role": "level2", "node": "node-1", "param": 2014 },
    { "role": "level3", "node": "node-1", "param": 3014 },
    { "role": "level4", "node": "node-1", "param": 4014 },
    { "role": "level5", "node": "node-1", "param": 5014 },
    { "role": "level6", "node": "node-1", "param": 6014 },
    { "role": "level7", "node": "node-1", "param": 7014 },
    { "role": "level8", "node": "node-1", "param": 8014 }
  ],
  "slidersReversed": true
}
```

**Precedence,** per part:

1. **The part's declaration.** A role the part's `roles` names is that parameter, whatever the
   plugins say. For the slider set it is all or nothing: a part whose `roles` names any `levelN`
   takes the whole set from it, with `slidersReversed`, and no plugin's level hint fills a slot
   past it (that is what keeps a two-element Sound's other six sliders dark when the plugin's
   hints declare all eight).
2. **The plugins' `oscairn.ui` hints,** for roles the part does not name: the first parameter
   declaring the role, in chain order, as the banks look today (the Levels bank on non-effect
   nodes; RFC 0044's section roles across the whole chain). The set's direction is its first
   parameter's `reversed`.
3. **A name guess,** only where RFC 0044 allows guessed roles (a plugin that declares no role at
   all, `roleFromName`), marked "guessed", and never for a role the part's `roles` names. The
   guess never yields a `levelN` today, so a slider set is never guessed.

A part with no slider set is not an organ and has no element levels, to everything in the
system: in `element` mode its sliders are dark.

**Who writes a part's declaration:**

- **The Sound's author, in the editor.** The part's editor (the Part page on the desktop, the
  part's editor on the panel) gains a **Sliders** step. It shows the part's slider set as a
  numbered list, each row a parameter's name and its plugin, marked "From the plugin" while the
  part has no `roles` of its own. **Add** picks a parameter from the part's plugins (grouped by
  plugin and module, searchable, as the assignment picker is); rows reorder by dragging or with
  up and down; a row's **Remove** takes it out. A switch, **Pulled down for more**, sets the
  direction. The first change copies the plugin's set into the part, so it becomes the part's
  own; **Use the plugin's** removes the part's set again. Other roles are set from a parameter's
  menu, **Role ›** and the role names, with **None** and **The plugin's**. Each change is an
  edit (`parts.roles`, below), undone by `edit.undo`, marks the part edited from its Sound
  (`from.modified`), and is saved with the part and in a Sound saved from it.
- **A converter.** Converted Sounds are the owner's main source, so what a converter writes is
  exact (below).
- **A plugin's presets** carry no declaration: a part made from a plugin preset, or a node a
  preset is loaded into, uses the plugin's hints. A node replaced or removed (`graph.replace`,
  a preset load through it included, or `parts.removeNode`) drops the part's entries naming it;
  entries whose node is gone are left out at save, as assignments are. Whether RFC 0043's move
  to a newer plugin version keeps them, where the parameter IDs stay, is to check against
  [RFC 0043](0043-moving-sounds-to-newer-plugin-versions.md).
- An entry whose parameter the loaded plugin does not have (a stand-in, another build) is kept
  and plays nothing: its slider is dark.

**What a converter writes.** On the converted part, `roles` and `slidersReversed`, and nothing
it cannot read from the source:

- **An organ voice on an organ plugin:** `level1` to `level9` for the drawbars the source's
  panel drawbars play, by footage: 16', 5⅓', 8', 4', 2⅔', 2', 1⅗', 1⅓', 1'. On the Oscairn Organ that is the upper
  manual's IDs 1 to 9 (the lower's and pedal's are not in the set; a source whose drawbars play
  the lower manual names IDs 10 to 18). `slidersReversed: true`, the tonewheel convention and
  the owner's MODX setting, unless the source stores a normal direction. Organ switches
  (percussion, vibrato and chorus, rotary) go in `roles` too once RFC 0044's draft 7 names
  them; a converter before that writes none.
- **An AWM2-class voice:** `level1` to `level`*n* for the elements the voice uses, in element
  order, each the Sampler's Element *n* Level parameter (IDs to check, above); an element the
  voice leaves off is not written, so a two-element voice gives two sliders.
  `slidersReversed: true` for an organ voice, one the source stores as an organ (its stored
  category, read as data, not guessed from the name), since its elements are drawbars; none for
  any other voice. An FM voice the same way with its operators' levels.
- **A source organ played from two parts** (the MODX's way to nine, the ninth drawbar an element
  of part 2): two parts, as on the source, each writing its own elements' levels as its own set,
  reversed. Nothing links the two parts (decided, below).
- **Never by name.** A source that does not say which parameters are the drawbars or levels
  gets no `roles`, and the plugin's hints apply.

### Who the sliders follow: the `element` mode

The sliders default to the mixer — part levels, then buses — as they do today
(`physical_controls.cpp:239-252`; decided 2026-11-11). Handing them to a part is the **`element`
fader mode** that exists, playing the part's slider set. The first draft proposed a new
`controls` mode; `element` is the smaller route (decided in this revision): it already follows a
part, already plays the Levels bank, is already validated (`Profile.cpp:198`) and already listed
by `modx-m.json`, so the mode machinery, the profiles and their validation stay as they are. A
`controls` mode would have played the same parameters through a second name. The mode keeps its
name; what it plays is now the slider set as defined above.

- `modes` lists what a group can do, `groupPositions` counts it, and `GroupPositions` (the
  per-group page/row/mode, `core/control/include/oscairn/control_groups.h:56-58`) already holds
  which mode is active. Profiles that want the mode list it: `modx-m.json` already does;
  `fantom-07.json`, whose sliders say `modes: ["part"]`, becomes `["part", "element"]`, its
  mode being the on-screen button.
- **The sliders' own part.** `element` plays the part `controllers.view` names, which is the
  knobs' part, so with the knobs on the Performance's controls the sliders would follow nothing.
  `controllers.view` gains `faderPart`: the part fader groups in `element` mode follow. Absent,
  they follow `part`, as today, so a client that sends only `part` sees no change. Play sends
  the selected part as `faderPart` always, so the sliders follow the selected part whatever the
  knob row follows. 0 is no part: the sliders are dark, as `element` is with no part today.
- The player switches modes with a **slider-mode button** in Play, beside the knob row's
  existing Performance/part switch (same place, same look; the mode is sent with
  `controllers.view`'s `positions`, the index of `element` in the group's `modes`, as
  `PlayView::refresh` sends the knob group's page). It is hidden when the group's `modes` lack
  `element`. While in `element` mode, selecting a part moves the sliders with it.
- A device whose panel has a real mode button (the MODX's mode keys, the FANTOM's CTRL/ASSIGN
  buttons) moves the group through its profile's `modes` and the screen follows. Whether a
  panel's button reaches a group's mode today is to check: nothing in the app moves a fader
  mode yet, and the FANTOM-0's panel controls are still estimated (`fantom-07.json`).
- **That button's slider mode is its default assignment, and it can be overridden** (the owner,
  2026-11-11). The profile declares it as a `fixed` button whose function is `sliderMode`
  (stepping the fader group through its `modes`). It is a control like any other: it resolves
  in the layers RFC 0044 gives a section control
  ([overriding a section control](0044-panel-sections.md#overriding-a-section-control)), with
  the profile's `sliderMode` as the default layer, so a Performance may send it to a scene or a
  control and a device may remap it. While it is overridden it does not switch the slider mode;
  the player reaches the mode with Play's on-screen slider-mode button, which stays, and which
  shows that the panel button is reassigned and to what ("CTRL → Super Knob, Performance").
- The mode a session is in is the engine's, like positions today: kept, pushed, not saved —
  until a pin saves it (below).

### Overflow: sliders first, then knobs, then the screen

A slider set may be longer than the device has sliders. Distribution is by order, always the same
rule, for any set, and always within one part's set (an organ plugin's nine drawbars on one part;
never a set made of two parts):

1. The device's sliders, `fader:1` up, take the set in order.
2. Further entries go to assignable knobs — the first ones not already taken by an entry — in
   identity order. Decided 2026-11-11 ("option A"): with eight sliders, sliders 1–8 take drawbars
   1–8 and the 9th, 1', goes to the first free assignable knob. That knob plays the entry in
   place of what its bank gives it, as an override does.
3. Anything past that is screen-only.

Decided 2026-11-11 ("option D", on top of A): **the screen always shows the whole set**,
wherever the physical controls landed. The knob copy and the slider copy both move the same
target, so both stay true. A set shorter than the sliders leaves the rest dark (a two-element
Sound: sliders 3 to 8), and the screen shows only its entries.

- **Nine sliders** (an external nine-fader controller, a future product): the profile declares
  `count: 9`, the rule maps all nine, no knob takes one. Controller profiles already join the
  device's groups and number on after them
  ([RFC 0024](0024-device-profiles.md#controllers)), so an eight-slider keyboard plus a one-fader
  unit is nine sliders too; `fader:N` identity numbers across groups make that work unchanged.
  A Mackie Control port's eight faders are not a fader group: the app plays them as the first
  eight parts' levels (`Controllers`, [RFC 0024](0024-device-profiles.md#mackie-control)), so
  whether they join `element` mode is to check.
- **No sliders** (the KICK, the Ignite, `kick.json` and `ignite.json` declare no fader group):
  step 1 is empty and the set goes where steps 2 and 3 send it — the first assignable knobs,
  then the screen. Nothing special-cases this; it is the same loop. The KICK's four assign
  encoders take entries 1 to 4; the Ignite has no assignable controls, so the set is on the
  screen only.

### Direction, and multiple organ parts

Reversal is new on a physical control: the reversed range exists only on a control's targets
(`MacroTarget`, `core/control/include/oscairn/parts.h:225-227`), and a slider playing a level
resolves to a plain `Assignment`. Step 4 gives `ResolvedControl` a direction, taken from the
followed part's set, and `ControllerMapping::play` turns an absolute position `x` into `1 − x`
before pickup compares it: architecture.md's pickup works "in the mapping's control-position
space", reversed mappings included, and the flip is part of the mapping. Only faders are
reversed: a knob that takes an overflow entry plays it clockwise for more, since a knob has no
"pulled down". The screen draws a reversed entry filled from the top, so "pulled down = louder"
reads the same on both surfaces.

Direction is taken from the part the sliders follow, because it is that part's set. Following the
organ part reverses the sliders; following a piano part puts them back. With several organ parts
the sliders are one physical set and follow one part at a time, as the knobs already do, so the
followed part's direction is the one in force. A scene change is a part-selection change in this
respect: scenes already store the mix and control values
([RFC 0011](0011-performance-ui-and-editors.md#scenes-the-full-model-decided-2026-10-31)), and
a scene that pins the sliders (below) hands them to its part, its direction with it.

### Pickup, and the two-value display

With no lights and no motors, a silent slider's screen value tells the player nothing about
where their hand must go. Pickup is already right — the control plays nothing until it reaches
the target's value within 1/127 or crosses it (`controller_mapping.cpp:137-147`) — but the
screen cannot *show* the waiting, because the engine keeps the hardware position private.

The report follows architecture.md's three values, and names which one it carries:

- **Physical position:** what the engine adds. Each physical control's last reported position,
  per control identity (not per target, since it is where the hardware is), kept beside the
  pickup state (`Pickup::knob`, same file, has it today, but per control and target). It is
  **raw**: 0 at the bottom or fully left, 1 at the top or fully right, as the hardware reported
  it, before pickup and before a reversed set's flip. A relative encoder has none, since a delta
  must not be shown as a position; a control never moved since the engine started is unknown,
  and the screen says so rather than guessing.
- **Accepted mapped value:** not reported. It is what reaches the destination, which the
  destination value then shows.
- **Destination value:** the parameter's or control's value, reported as now
  (`params.changed`, `controls.changed`).

The control-API addition is additive, on the interface that already carries control state
([control protocol](../control-protocol.md#requests-and-responses)): `controllers.get` and the
`controllers` topic gain `places`, each `{control, position}` (0 to 1, raw) for every control
whose position the engine has. `controls.changed` stays what it is — the controls' values and
part levels — because places move only when the player moves them, and the `controllers` topic
already fires "a control moved".

Raw is the one definition that needs no context: a client knows where the hardware is without
knowing which part is followed. Play draws the marker at the raw height on every bar, and the
fill of a reversed entry from the top, so on a reversed drawbar the marker meets the fill where
`1 − position` equals the value, which is where pickup catches it.

Play's slider row then shows, per entry: the parameter's value (from `params.changed`) and a
thin marker at the physical position (from the `controllers` topic), until they meet; while the
marker and the value differ, moving the slider drags the marker and nothing sounds. The same
two-value display applies to **any** slider under pickup, not only a slider set, in every mode: a
part strip's fader in the mixer shows both too while its slider has not caught up (decided
2026-11-11).

### The pin

A Performance, or one of its scenes, can pin the sliders to one part: an always-organ Performance
that opens in `element` mode on the organ part, a scene whose chorus hands them to the organ. The
pin is saved data, so it lives with the Performance's other mapping data — the assignments
already save per control identity and per part (`AssignmentOverride`,
`core/control/include/oscairn/parts.h:277`, Performance format version 13's `assignments`,
[performance-format.md](../performance-format.md)); a pin is the same shape naming a group's
mode and part instead of one control's target, and the format version current when step 5
builds carries it, one forward (initial-development policy). The pin outranks the session's
unsaved mode while the Performance (or, for a scene pin, the scene) is in force; clearing the pin
returns the mode to the player's session choice. Nothing pins a slider *set*: the pin picks who
is followed, the part says what they follow.

### The screen row, and accessibility

Play gains a slider row: the followed part's set in order, one vertical bar per entry (for an
organ, 16' left to 1' right, the tonewheel convention; the FANTOM's own screen layout to check
when step 3 builds), each with its parameter's name, its value as text and the position marker
described above. The value text is the plugin's own (`params.text`), with no host scale: one rule
for every set, and the Oscairn Organ's own text is already its 0 to 8 drawbar steps. The row
replaces the knob row's place while the sliders are in `element` mode and the part has a set; the
strips underneath stay the mixer. It uses the theme's slider style like every other control
([design language](../design-language.md)), and the design language's rules hold: text for every
value, state shown by position and text and not color alone, a focus ring, and 44-unit touch
targets, at least 6.5 mm across (`design-language.md:11-25`). Keyboard: focus moves along the
row, arrows move an entry, and the value text names it, so the row works with no touch and no
mouse; each entry's accessible title is its name and value ("Upper 16', 7").

### Format and control-API changes

- **Format:** a part's `roles` and `slidersReversed`, in a Performance's `parts[]` and a Sound's
  `part`, in the next versions, one forward as the
  [initial-development policy](../developer-guide.md#initial-development-compatibility-policy)
  allows: Performance 14 and Sound 9, unless another change takes those numbers first. RFC
  0031's `pluginRepositories` is specified for the same two numbers
  ([performance-format.md](../performance-format.md#where-plugins-are-published)); whichever
  builds first takes them, and the other joins that version if it lands with it or goes one
  forward. `docs/performance.schema.json`'s part gains both. The reader refuses an unknown role
  name, `outputBus`, a repeated role or parameter, a gap in the `levelN` entries and a node not
  in the part's chain or splits.
- **`parts.roles`** (new): `part`, `roles` (the list as above; empty for none, so the plugins'
  hints apply) and `slidersReversed`. `{accepted:true}`; an edit, undone by `edit.undo`.
  Refused as the reader refuses. Permission `edit`, as `parts.*`.
- **`graph.get`:** each part gains `roles`, `slidersReversed`, and `sliders`, the resolved set in
  order, each `{node, param, from}`, `from` being `part` or `plugin`, and `reversed`.
  `params.list`'s `role` stays the plugin's declaration, and gains `reversed` from draft 6.
- **`parts.loadSound`** replaces the part's `roles` and `slidersReversed` with the Sound's, as it
  replaces the part's own assignments; `parts.saveSound` writes them.
- **`controllers.view`:** optional `faderPart`, as above.
- **`controllers.get` and the `controllers` topic:** `places`, as above.

## Build order and gates

| Step | Gate |
| --- | --- |
| 1 | `controllers.get` and the `controllers` topic carry each moved control's raw position; a test moves a fader, changes its target from another client, and reads both the held position and the moved value while pickup is waiting; pickup itself still behaves as `control-physical-controls` and `control-controller-input` say. |
| 2 | On the `modx-m` layout with three parts: the sliders play part levels by default; the Play button (or the profile's mode position) switches them to `element` mode; with `faderPart` set, the sliders follow it while the knobs follow the Performance, and without it they follow `part` as today; a resolved assignment test names each `fader:N`'s target in each mode. |
| 3 | With the Oscairn Organ, whose hints give its drawbars no role: a part with no `roles` has no slider set and its sliders in `element` mode are dark; `parts.roles` naming the upper drawbars `level1` to `level9` makes Play draw nine entries, each showing its value and, for a fader under pickup, its marker, and the sliders play them; saved as a Sound and loaded into another part, the set comes with it; a plugin with draft-6 level hints and no part declaration fills the set from its hints, and a part declaring two levels on it shows two, the rest dark. `control-performance-format`: `roles` and `slidersReversed` save and reload, and each refusal above is refused. |
| 4 | On the eight-slider layout, sliders 1–8 drive drawbars 1–8 and one assignable knob drives the 9th, both it and the slider's screen copy reading the same target value; following the organ part reverses the sliders (a full-down fader plays the drawbar at its most, pickup catches it from that side, and `places` still reports the fader's raw position) while the knob is not reversed; following a piano part un-reverses them; checked in `control-physical-controls` and by rendering. |
| 5 | A Performance pinned to the organ part opens in `element` mode on a second engine over the socket, and selecting a scene pinned to another part hands the sliders to it; a controller profile with nine faders maps all nine with none to a knob; the slider row is reachable and operable by keyboard alone, and `desktop-gaps` renders it at the FANTOM-0's 1280 × 720 with 6.5 mm targets. |

## Decided

By the owner, 2026-11-11, in this revision; recorded in the sections above.

- **A Sound declares its slider set and roles,** per Sound, with the plugin's `oscairn.ui` hints
  as the default and a name guess last where RFC 0044 allows it
  ([what a Sound declares](#what-a-sound-declares-and-where)).
- **Drawbars and element levels are one model,** the slider set, played by the `element` mode
  ([the slider set](#the-slider-set-the-parts-levels-nine-wide)).
- **Direction is the Sound's set's,** whatever plugin plays it: a Sampler organ Sound is reversed
  as an organ plugin's is.
- **A slider set never spans parts.** A two-part organ, as the MODX plays one and as a converter
  brings it over, stays two parts, each with its own set; for the ninth drawbar the player selects
  part 2, and the sliders follow it, as on the MODX. The owner: "It'll still be annoying but not
  worse than the modx and an actual organ plugin will have better ui." No cross-part set, no
  merging of parts, no special case for converted organs. Overflow works within one set.
- **A panel's slider-mode button** (a CTRL/ASSIGN-style button) has slider mode as its default
  assignment, declared by the profile, and can be overridden like any control; Play's on-screen
  button then reaches the mode ([who the sliders follow](#who-the-sliders-follow-the-element-mode)).
  The owner: "If that button defaults to slider mode, that's its default assignment, and it can
  be overridden."

Decided in this revision, small enough not to need the owner, each with its reason in the text:
`element` rather than a new `controls` mode; `faderPart` in `controllers.view`; `places` raw;
only faders reversed, not a knob taking an overflow entry; value text the plugin's own.

## Open questions

None open.

## Ruled out

- Guessing that a Sound is an organ from its name, its plugin, or nine slider-widget parameters:
  the Sound declares, the selection follows (decided 2026-11-11).
- A global "reverse sliders" setting: direction is the part's, through its Sound (decided
  2026-11-11).
- Declaring drawbars only in the plugin's hints: two Sounds on one plugin could not differ, and a
  plugin without `oscairn.ui` could never offer drawbars (the owner, 2026-11-11).
- A new `controls` fader mode beside `element`, and drawbar roles or a drawbar view beside the
  Levels roles: both say what the existing mode and roles already say.
- A Sound's declaration as assignments: assignments name physical controls, which a Sound must
  not depend on; the role map names only the Sound's own parameters.
- Motorizing or auto-aligning sliders at scene changes: most of these panels have no motors;
  the two-value display is the answer that works everywhere.
- Splitting an organ across two parts as the design's answer: that is the problem, not the fix.
  An organ plugin declares its nine on one part.
- A slider set spanning parts, or a converter merging a two-part organ into one part (decided
  2026-11-11): each part's Sound has its own set, and the sliders follow the selected part.
