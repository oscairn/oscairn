# RFC 0044: Panel sections: dedicated piano, organ and synth controls over flexible parts

Status: proposed, 2026-11-11, from the owner's question of that day. Design only: nothing here is
built. Builds on the physical controls, control groups, roles and banks of
[RFC 0011](0011-performance-ui-and-editors.md#physical-controls), device and controller profiles
([RFC 0024](0024-device-profiles.md#control-groups)), Sounds, Performances, the library and tags
([RFC 0021](0021-sounds-performances-and-library.md)), the system shell
([RFC 0030](0030-system-shell-launcher-and-navigation.md)), input selectors and zones
([RFC 0012](0012-sync-external-midi-and-multi-device.md#choosing-devices-selectors)) and, most of
all, [RFC 0042](0042-sliders-and-drawbars.md), "Sliders that follow a part, and organ drawbars"
(proposed), whose slider set, per-Sound role declaration, pins, pickup display and per-part
direction this RFC reuses rather than repeats ([below](#what-this-rfc-takes-from-rfc-0042)).

Roland, Yamaha and Nord products are named here only to describe the kind of panel Oscairn might
run on. Oscairn is not affiliated with or endorsed by Roland, Yamaha or Nord Keyboards.

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. The profile's `sections` and a group's `section`; a part's `section` binding in the
      Performance; `sections.get` and `sections.bind`; the part-property functions (on/off,
      volume, octave, split and layer); a test profile and panel layout standing in for a
      sectioned board on the desktop, its controls played through `controllers.play`
- [ ] 2. Declared-role functions with today's roles (cutoff, resonance, attack, release, EQ,
      mix) and guessed ones, dark controls, `takePositions`, shared groups with `select:`
      buttons, and RFC 0042's slider set, direction, pickup and places on a section's own
      controls
- [ ] 3. Auto-binding on open, the prompt when it is not obvious, `sections.auto`
- [ ] 4. Sound select within a section: its category, favorites, the section display, the
      section's default Sound, an empty section's on/off creating a part, and locked sections
- [ ] 5. `oscairn.ui` draft 7: the organ, piano and effect roles below; a test organ plugin
      declaring them
- [ ] 6. Effects per section: insertion effects on and off, their amount, and sends to the
      Performance's reverb and variation buses
- [ ] 7. Play for a sectioned device, on the panel and compact surfaces; the profile's lights and
      section displays driven where the hardware takes them
- [ ] 8. The same Performance on the desktop, on a board with other sections and on a board with
      none; a controller profile with a section of its own
- [ ] 9. Overriding section controls: a Performance's `assignments` by section identity, the
      device's remaps in `sections.json`, the Settings page

## Problem

Some stage keyboards have a panel cut into **sections**: a piano section, an organ section, a
synth section, each with its own sound select, volume, on/off and controls (drawbars on the
organ, envelope and filter knobs on the synth). The musician reaches for "the organ's drawbars"
without thinking about which part holds the organ. The hardware is described
[below](#the-hardware); the owner's examples are the Roland V-STAGE and the Yamaha CK.

Oscairn has no such thing. A Performance is any number of parts, each a chain of any plugins
([RFC 0011](0011-performance-ui-and-editors.md#parts)). Nothing in a part says "this is the organ".
The question (the owner, 2026-11-11): on a board like that, how do the sections map onto parts?

What exists today, read in the code and docs on 2026-11-11:

- **Control groups have four roles and no sections.** A group is `kind`, `count`, `sensing` and
  `role`: `notes`, `screen`, `assign` (plays banks) or `fixed` (a dedicated function each, named
  in `functions`) ([RFC 0024](0024-device-profiles.md#control-groups); `docs/profile.schema.json`,
  whose group fields are `count`, `functions`, `kind`, `midi`, `modes`, `paging`, `role`, `rows`,
  `sensing` and `target`; validated in `hosts/standalone/Profile.cpp` around line 179). Nothing
  groups controls by what part of the panel they sit in.
- **Only assign groups have control identities.** `knob:N`, `fader:N`, `button:N` and the rest
  are numbered across the assign groups, and overrides name them
  ([RFC 0024](0024-device-profiles.md#control-groups)). A `fixed` group's controls have names
  (`volume`, `superKnob`), and the FANTOM-0's panel reader notes the engine has nothing to give
  fixed controls to: "the controller layer plays assignable controls only"
  ([fantom-0.md](../fantom-0.md#what-it-does-not-do-yet)).
- **Roles exist, and banks follow the part.** `oscairn.ui` draft 5
  (`core/clap-ext/include/oscairn-clap/ui.h`) gives each parameter an optional `role`: cutoff,
  resonance, the filter envelope's amount, key tracking and stages, the amplitude envelope's
  stages, hold, velocity, amplitude key tracking, `LEVEL` (the instrument's output level), EQ
  gains, frequencies and Q, `LEVEL_1` to `LEVEL_8`, `MIX` (an effect's main control), `PAN` and
  `OUTPUT_BUS`. The role banks (Filter, Amp EG, EQ, Levels) are filled from the roles of a part's
  non-effect nodes (`roleBanks` in `core/control/include/oscairn/physical_controls.h`, filled in
  `core/control/src/physical_controls.cpp` around line 101); a role none has is blank. A plugin
  that declares no roles gets roles guessed from exact parameter names (`roleFromName`, same file,
  around line 297, used only when the plugin declares none, around line 347). There is no role
  for percussion, rotary speed, vibrato and chorus, or a piano's tone.
- **A part already has most of a section's state.** A `Part` has `name`, `level`, `pan`, `mute`,
  `solo`, `sends`, `from` (the library Sound loaded into it, its revision and whether it was edited
  since) and an input whose `zones` carry a key range, velocity range, channels and `transpose`
  (`core/control/include/oscairn/parts.h`, `Part` and `PartInput`;
  `core/engine/include/oscairn/note_zone.h`, `NoteZone`). Parts have **no color** yet (RFC 0011's
  [Play status](0011-performance-ui-and-editors.md#play-status): "parts have none yet"); the
  Performance's `info` has one. There is no octave field: octave is `transpose` in twelves.
- **Sounds carry free tags,** stored in the item's info block, searched by `library.query`'s
  `tags` (all must match), `favorites` and `kind`
  ([RFC 0021](0021-sounds-performances-and-library.md#oscairns-tags),
  [control protocol](../control-protocol.md#library)). There is no fixed category tree, by
  design. A part knows its Sound only through `from.sound`.
- **Loading and creating parts from Sounds exist:** `parts.loadSound` replaces a part's chain with
  a Sound's and keeps its input, output, sends, fader and solo; `parts.fromSound` makes a new part
  listening to every key with the Sound loaded ([control protocol](../control-protocol.md#sounds)).
- **Effects have node roles:** a node may carry `insertion-a`, `insertion-b`, `part-eq`, `reverb`,
  `variation`, `master-effect`, `master-eq` or `arpeggiator` (`graph.role`,
  [RFC 0016](0016-effects-suite.md#where-the-effects-go)); a part's sends are its levels into
  shared buses.
- **The engine can play a control by identity** (`Session::controllerControl`,
  `core/control/include/oscairn/session.h:271`), which the FANTOM-0's panel reader uses. No
  control-API method reaches it; `appGrants()` in `core/control/src/json_rpc.cpp` lists none.
- **Panel layouts have no faders and bind no control identities.** A layout's knobs bind
  `node:param`, `tempo`, `control:NAME`, `row:N` or `encoder:N`, and its buttons the transport and
  the shell (`hosts/standalone/layouts/README.md`, `hosts/standalone/Panel.cpp` around line 50).
  A `row:N` knob sets the parameter itself and reports `knob:N` as touched for learn.
- **Lights and displays on controls are an open question** in RFC 0024 ("how a profile declares
  controls with displays of their own"). Values go back only to a controller's group whose `midi`
  says `feedback: true`, and to Mackie Control surfaces.

So a part can hold a section's state, roles say what a parameter is, and RFC 0042 hands sliders
to a part and puts its slider set (drawbars, element levels) on them. What is missing is a name
for a place on the panel, a link from a part to it, controls that act on whatever part that is,
and the roles an organ and a piano need.

## Outcome

- A profile names its panel's sections. Each has its own controls, in ordinary control groups
  marked with the section.
- A Performance binds a part to a section. The section's controls then act on that part, whatever
  plugin it holds (decided 2026-11-11).
- A section's controls act by **declared role**, not by plugin: a drawbar slider is "slider 3
  of the part's set", a percussion button is "percussion", and the Sound (or, by default, its
  plugin) says which parameter that is. A control
  whose role the loaded Sound lacks goes dark and does nothing (decided).
- Nothing guesses what a Sound is: its tags decide its category, its roles what its controls do. A
  section's sound select browses Sounds of the section's category (decided).
- A Performance that arrives with no bindings binds itself when each section has exactly one
  obvious part, and asks otherwise. A wrong binding is quick to change (decided).
- Parts beyond the sections stay playable and are edited on the screen (decided).
- Split and layer buttons edit the bound part's keyboard zones, which stay the only split model
  (decided).
- A Performance can override a section control, sending the synth section's cutoff knob to a
  control with targets in two plugins, and a device can remap one for every Performance; the
  section default applies where neither does (decided).
- On the desktop, and on a board without those sections, a binding is only a label on a part;
  nothing breaks (decided).

## The hardware

From the manufacturers' published manuals, read on 2026-11-11. Only documented behavior;
what could not be confirmed is listed at the end. Yamaha's own site refused every request
(HTTP 403), so the Yamaha facts come from copies of Yamaha's owner's manuals hosted by a
retailer; their page numbers are the printed ones and were not compared with Yamaha's files.

**Roland V-STAGE 76 and 88** (one [Owner's Manual](https://static.roland.com/assets/media/pdf/V-Stage_Manual_eng02_W.pdf)
for both; [product page](https://www.roland.com/global/products/v-stage_88/)):

- Five "independent sound generator parts", ORGAN, A.PIANO, E.PIANO, SYNTH A and SYNTH B, plus
  TOTAL EFFECTS, MASTER and controller sections; every part but A.PIANO has its own effect (p. 8).
- **Every part:** a LEVEL knob with indicators around it showing the volume (Shift: pan),
  ON/OFF, SPLIT LEFT and RIGHT, OCTAVE DOWN and UP, and a TONE SELECT knob with category
  indicators above it; pressing it lists the tones on the display (pp. 14–22). Two parts on
  layer; a split has a left and a right region, or both; holding SPLIT POINT and pressing a key
  sets the point (p. 15).
- **ORGAN** (pp. 17–18): tone types WHEEL, TRANSISTOR 1 and 2, PIPE; DRIVE knob and ON/OFF;
  vibrato/chorus TYPE knob (V-1 to V-3, C-1 to C-3) and ON/OFF; percussion ON/OFF, VOLUME (soft,
  normal), DECAY (fast, slow), HARMONIC (2nd, 3rd); physical harmonic bars 16′ to 1′, louder
  "pulled out (down)"; PEDAL, LOWER and UPPER buttons choosing which organ part the bars edit
  (the product page's "three-part console model"); H-BAR MANUAL, which makes the bars'
  physical positions take over; rotary SPEED (slow, fast), BRAKE (stops the rotor) and ON/OFF.
- **A.PIANO** (p. 19): GRAND and UPRIGHT, PIANO DESIGNER, BRIGHTNESS, STEREO WIDTH, key touch.
- **E.PIANO** (pp. 20–21): TINE, REED, DIGITAL, CLAVINET; DETUNE, SOUND LIFT; an effect with
  three CTRL knobs; TREMOLO depth, rate, sync and on; AMP SIM drive, treble, bass.
- **SYNTH** (p. 22): STRINGS, PAD, LEAD, BASS, KEY, OTHER; ATTACK, RELEASE and CUTOFF knobs; an
  effect with two CTRL knobs; PART SELECT between A and B, and a dual mode where the knobs drive
  both.
- **TOTAL EFFECTS** (p. 23): an effect, DELAY and REVERB, each with a send mode. **MASTER**
  (p. 24): EQ and compressor.
- **Scenes** store each part's on/off, tones, edits and total effects; 512 of them (p. 9).

**Yamaha CK88 and CK61** (Owner's Manual, [retailer copy](https://www.fullcompass.com/common/files/86003-CK61UserGuide.pdf)):

- Three keyboard Parts, A to C, one Voice each (pp. 7–8). Each has a volume slider and ON/OFF;
  Part A to C select buttons route the shared category, organ, filter, EG, drive and effect
  controls to the selected Part, or to several pressed together (p. 12).
- Nine ORGAN FOOTAGE sliders, 16′ to 1′, separate from the part volume sliders; moved "downwards
  (toward you)" for more; they work only with Organ Flutes voices (pp. 11–12).
- SPLIT cycles A/BC, AB/C and A/B/C, with all three layered when off; SPLIT POINT sets the
  point; both are stored in the Live Set Sound (p. 13). The OCTAVE buttons are global and not
  stored; each Part's note shift is stored (p. 10).
- CATEGORY buttons: pressing one again steps through it (p. 13); an encoder and an LCD.
- Organ: vibrato/chorus ON/OFF and TYPE (V1–C3); percussion ON/OFF, Soft, Fast, 2nd/3rd (p. 11);
  rotary STOP and SPEED (p. 10).
- Per Part, through the select buttons: FILTER on, CUTOFF, RESONANCE; EG on, ATTACK, RELEASE
  (p. 15); DRIVE (overdrive, distortion, two rotaries, compressor) and two insertion effects
  with TYPE, DEPTH, RATE (p. 16). Global DELAY, REVERB and master EQ (p. 17).
- A Live Set stores the Parts' voices and settings; 160 places (pp. 8, 15).

**For breadth.** The **Nord Stage 4** ([User Manual v1.6X](https://www.nordkeyboards.com/wt/documents/951/Nord%20Stage%204%20User%20Manual%20v1.6X-Edition-N.pdf),
[specifications](https://www.nordkeyboards.com/products/nord-stage-4/specifications/)) has Organ
(two layers), Piano (two) and Synth (three) sections, each with ON (a long press solos), layer
buttons and per-layer faders with LED graphs; octave shift and keyboard zones per layer (up to
three split points); nine physical drawbars with LED graphs, a live mode and SYNC; percussion,
vibrato/chorus, rotary slow, stop and fast; per-layer effects (pp. 10–27, 52–53). The **Yamaha
YC88 and YC73** (Owner's Manual, [retailer copy](https://www.fullcompass.com/common/files/86020-YCSeriesUserGuide.pdf))
have an Organ section (ON/OFF, a LOWER/UPPER button picking which part the drawbars edit, SPLIT
for them, octave, a shared volume, physical drawbars with LEDs, percussion on the upper part
only), Key A and Key B sections (ON/OFF, category and voice select, split, octave, volume, TONE,
an EG/filter knob, two insertion effects), and shared EFFECT, SPEAKER/AMP, REVERB (a send per
section) and master EQ sections (pp. 11–19).

**What follows for the design:**

- Every board has per-section on/off, volume, octave and split, and sound select by category:
  part properties and tags, below.
- The organ controls agree across all four: nine drawbars pulled down for more, percussion
  on, level, decay and harmonic, vibrato/chorus on and type, rotary speed and stop, drive.
- **Sections share controls.** The V-STAGE's bars edit one of three organ parts and its synth
  knobs one of two, the YC's drawbars one of two, the CK's tone and effect controls one of three
  Parts, each picked by select buttons. A design with one fixed set of controls per section
  would fit none of them exactly ([shared section controls](#shared-section-controls)).
- Split is two or three regions with one or more split points, and the split point is set by
  holding a button and pressing a key (V-STAGE) or by a button (CK).

**Not confirmed:** no Yamaha page on yamaha.com (product pages, official manuals) could be read;
the YC61 was not read, nor the YC's later supplementary manual; neither the V-STAGE nor the YC
manual states its drawbar count in text (the range 16′ to 1′ only, which is nine on a tonewheel
organ), and the panel images were not inspected; whether the Nord Stage 4's two organ layers
share one physical set of drawbars; the V-STAGE's Reference Manual, which holds the effects'
routing details; the CK's category buttons' printed names (the Data List's ten categories were
read); and any differences between the 76- and 88-key panels.

### How others do it

The owner's question: the V-STAGE runs ZEN-Core, which has flexible parts, so how does that map
to its sections? From Roland's published documents, it does not run ZEN-Core under everything:

- **Four engines, one per kind of section.** The [product page](https://www.roland.com/global/products/v-stage_88/)
  names an acoustic piano engine "developed with V-Piano technology", an electric piano engine
  "with the latest SuperNATURAL advancements", an organ engine "driven by Virtual Tone Wheel
  technology", and a "ZEN-Core Synthesizer engine" with SDZ sound packs and Model Expansions:
  "Four independent sound engines". ZEN-Core is named for the synth only.
- **ZEN-Core is reached only through the synth section:** SYNTH A and SYNTH B, "two independent
  parts", layered or split, picked by PART SELECT, or both at once in dual mode
  ([Owner's Manual](https://static.roland.com/assets/media/pdf/V-Stage_Manual_eng02_W.pdf) pp. 8,
  22). Each has its own receive channel, level, pan, part EQ and velocity range
  ([Reference Manual](https://static.roland.com/manuals/v-stage_reference/en-US/), page
  207080715.html). Neither manual nor the product page mentions free zones like a FANTOM's 16,
  or more than two ZEN-Core parts.
- **Sections are tied to their tones.** "The V-STAGE features dedicated tones for each part"
  (Owner's Manual p. 9); the tone list has a menu per part, Organ, A.Piano, E.Piano, Synth A and
  Synth B, each by category, type and tone (Reference Manual, page 150510987.html). No statement
  was found that a part can load another part's tones, nor one that forbids it outright; whether
  the synth's KEY category ("Keyboard instrument") holds pianos was not confirmed.
- **A scene stores** each part's on/off, the tone combination, edits, the total effects and
  external device settings; 512 scenes, 128 chains (Owner's Manual pp. 9, 31). Split and layer are
  per part (each part's SPLIT LEFT and RIGHT, both for the whole keyboard, its own octave), with
  one split point; parts that are on together layer (pp. 14–15). The organ has its own split into
  upper, lower and pedal (p. 18).

The others, briefly (Yamaha's from the retailer copies of its manuals above):

- **Yamaha CK:** one engine, "AWM2, AWM (Organ Flutes)" (CK manual, specifications, p. 69).
  Parts A to C each select a voice by category from ten (p. 13); organ flutes voices sit under
  the Organ category and are the only ones the footage sliders, percussion and vibrato/chorus act
  on (pp. 11, 13). No restriction of category by Part was found; that any Part takes any category
  is implied, not stated. So the CK is flexible parts with shared controls, not fixed sections.
- **Yamaha YC88 and YC73:** "VCM Organ, AWM2, FM" (YC manual, specifications, p. 54). The Organ
  section holds organ types only, H1 to H3 on the VCM organ and F1 to F3 on FM (p. 14); Key A and
  Key B each choose from Piano, Electric Piano, Synth and Other (p. 16), FM voices marked by name
  (p. 23). That the other Keys voices are AWM2 is inferred from the specification, not stated.
- **Nord Stage 4:** fixed engines per section ([User Manual](https://www.nordkeyboards.com/wt/documents/951/Nord%20Stage%204%20User%20Manual%20v1.6X-Edition-N.pdf)):
  modeled organs in six models per layer (p. 18), the Nord Piano Library in six types (pp. 10,
  24), and a synth "based on the acclaimed Nord Wave 2" that also plays the Nord Sample Library,
  user samples included (p. 10). Presets load into "any Layer or Section" from the organ, piano
  and synth banks (p. 10); whether an organ preset can load into the synth section was not
  confirmed.

**How Oscairn's design relates.** A fixed-engine sectioned board (the V-STAGE, the YC, the Nord)
is one case of slots and bound parts: a profile whose new Performance has one part per section,
each holding only Sounds of its section's category. That is a [locked section](#locked-sections).
A flexible-part board with shared controls (the CK) is open sections whose shared groups follow
select buttons ([shared section controls](#shared-section-controls)). Oscairn's parts stay
flexible underneath in both cases; only the profile decides how much of that a product shows.

## Design

### What this RFC takes from RFC 0042

A section is RFC 0042's idea made permanent by hardware: RFC 0042 hands a set of sliders to the
selected part for a while; a section hands its own controls to its bound part for as long as the
binding stands. So this RFC defines no second way to find a part's drawbars or levels, to say
which parameter has a role, to reverse a slider, to wait for pickup or to show where a control
sits. It relies on exactly these parts of RFC 0042 as revised on 2026-11-11, and must be checked
against them if that text changes:

1. **The slider set:** a part's `level1` to `level9` roles in order, one model for an organ's
   drawbars and a sampled or FM Sound's element or operator levels; a set is one part's and never
   spans parts (RFC 0042,
   [the slider set](0042-sliders-and-drawbars.md#the-slider-set-the-parts-levels-nine-wide)).
2. **Who declares a role:** the part's own `roles` (saved in the part and in its Sound, written
   by the Sound's author or a converter), then the plugins' `oscairn.ui` hints, then a name guess
   where this RFC allows one, marked "guessed"; for the slider set the part's declaration is all
   or nothing (RFC 0042,
   [what a Sound declares](0042-sliders-and-drawbars.md#what-a-sound-declares-and-where)). The
   role map is not an assignment, so it does not touch the rule that a Sound's own assignments
   may not name section identities ([below](#performance-overrides)).
3. **Direction per set, applied per part:** the part's `slidersReversed` (or its set's first
   parameter's hint), taken from the part a set of controls follows; `ResolvedControl` gains a
   direction and `ControllerMapping::play` turns `x` into `1 − x` before pickup, for faders only
   (RFC 0042, [direction](0042-sliders-and-drawbars.md#direction-and-multiple-organ-parts)).
4. **The overflow rule:** one set's entries go to sliders in order, then to the first free
   assignable knobs, then to the screen only, and the screen shows the whole set (RFC 0042,
   [overflow](0042-sliders-and-drawbars.md#overflow-sliders-first-then-knobs-then-the-screen),
   options A and D).
5. **Places and the two-value display:** `controllers.get` and the `controllers` topic gain
   `places`, each `{control, position}`, the raw physical position, and the screen shows a
   control's value and where the physical control sits until pickup catches it, for every slider
   under pickup (RFC 0042,
   [pickup](0042-sliders-and-drawbars.md#pickup-and-the-two-value-display), its step 1).
6. **Pins:** a Performance or scene may pin a group's mode and part, saved beside `assignments`
   (RFC 0042, [the pin](0042-sliders-and-drawbars.md#the-pin)).
7. **Defaults stay:** the shared sliders default to the mixer, and the existing `element` fader
   mode hands them to the selected part's slider set, following `controllers.view`'s `faderPart`
   (RFC 0042, [who the sliders follow](0042-sliders-and-drawbars.md#who-the-sliders-follow-the-element-mode)).

Where a section's controls differ from RFC 0042's sliders, it is only in **who they follow**: a
section's controls follow its bound part always, and never the selection or a pin. A board
that has both a section and assignable sliders (a sectioned board with a row of mixer faders, or
an eight-fader controller beside it) keeps RFC 0042's behavior for the assignable sliders
unchanged; when they follow the organ part, they and the organ section's own faders drive the
same slider set, each under its own pickup. Controls a board shares among several sections,
picked by select buttons, are [shared section controls](#shared-section-controls), still bound
by the binding and never by the selection of Play.

Drawbars are roles, as every other section control is: a section's fader with function
`level`*n* plays entry *n* of the bound part's slider set, an organ's drawbar or a sampled
Sound's element level alike, in the set's direction.

### Sections in the profile

A profile gains an optional `sections` list, and a control group an optional `section`. Groups
stay where they are, in `controls`, with every rule they have now; a section only names a slot and
says which groups sit in it.

```json
"sections": [
  { "id": "piano", "name": "Piano", "category": "piano", "display": { "kind": "text", "lines": 1 } },
  { "id": "organ", "name": "Organ", "category": "organ" },
  { "id": "synth", "name": "Synth", "category": "synth", "display": { "kind": "text", "lines": 1 } }
],
"controls": [
  { "kind": "keys", "count": 76, "role": "notes" },
  { "kind": "faders", "count": 9, "role": "fixed", "section": "organ",
    "functions": ["level1", "level2", "level3", "level4", "level5", "level6", "level7", "level8", "level9"] },
  { "kind": "buttons", "count": 4, "role": "fixed", "section": "organ", "lights": "state",
    "functions": ["on", "percussion", "rotarySpeed", "vibratoChorus"] },
  { "kind": "knobs", "count": 1, "role": "fixed", "section": "organ", "functions": ["volume"] },
  { "kind": "knobs", "count": 4, "role": "fixed", "section": "synth",
    "functions": ["cutoff", "resonance", "attack", "release"] },
  { "kind": "encoders", "count": 1, "role": "fixed", "section": "synth", "functions": ["sound"] }
]
```

(An example, not a real product's profile.)

- **`sections`:** each `{id, name, category, subcategories, display, locked}`. `id` is the slot: a
  lowercase word from a shared vocabulary (`piano`, `epiano`, `organ`, `organ-lower`,
  `organ-pedal`, `synth`, `synth-b`, `strings`, `pad`, `bass`, `layer`, `upper`, `lower`), so the
  same Performance finds the same slot on another board
  ([below](#the-same-performance-elsewhere)); a profile may use another word, which then binds
  only on boards that use it too. `name` is what the screen and the panel say. `category` is the
  library tag its sound select browses ([below](#sound-select-within-a-section));
  `subcategories`, optional, up to eight more tags its `categoryN` buttons narrow to. At most 16
  sections; order is the panel's, left to right. `locked`, optional: [locked sections](#locked-sections).
- **A group's `section`:** the slot its controls belong to. Only `fixed` groups take one: a
  section's controls are dedicated, which is what `fixed` already means. Their `functions` come
  from the section vocabulary below instead of free names; a `fixed` group without `section`
  keeps free names as now. `notes`, `screen` and `assign` groups refuse `section`: the keys, the
  screen knobs and the assignable controls belong to the whole board.
- **Control identities inside a section:** `<section>/<kind identity>`, numbered within the
  section from 1 (`organ/fader:3`, `synth/knob:2`, `organ/button:1`). Pickup, RFC 0042's `places`
  and `controllers.play` name them so, and a Performance's `assignments` override them
  ([below](#overriding-a-section-control)). They never shift the numbering of `knob:N` and
  the rest, which stays across assign groups only.
- **`lights`** on a section's group, an answer for sections to RFC 0024's open question, kept
  small: `none` (the default), `state` (each control has a light showing its function's state:
  on/off, rotary fast, percussion on), `level` (a ring or meter showing its value). Whether the
  engine can drive them depends on the hardware's path back: a controller group's `midi` with
  `feedback: true` today; a panel reader that writes back is not built (the FANTOM-0's reads only,
  [fantom-0.md](../fantom-0.md#the-panel-reader)).
- **`display`** on a section: `{kind: "text", lines}`, a small character or graphic display
  showing the bound part's Sound name and the control being moved. Driven the same way as lights.

The section vocabulary of `functions`, by where each acts ([next section](#what-each-control-acts-on)):

| Function | Kind | Acts on |
| --- | --- | --- |
| `on` | button | The part's `mute` (on is not muted); on an empty section, creates a part |
| `volume` | knob, fader | The part's `level` |
| `octaveDown`, `octaveUp` | button | The part's zones' `transpose`, by 12, from −36 to +36 |
| `layer`, `lower`, `upper` | button | The part's zones: the whole keyboard, below the boundary, from it up ([below](#split-and-layer-on-one-keybed)) |
| `sound` | encoder | Steps through Sounds of the section's category, loading each |
| `soundPrevious`, `soundNext` | button | The same, one step |
| `category1` … `category8` | button | Narrows the sound select to the Nth extra tag the section lists |
| `favorite` | button | Steps through favorites of the section's category |
| `takePositions` | button | Sets each control of the section's groups to its physical position, ending pickup ([shared section controls](#shared-section-controls)) |
| `select:<slot>` | button | Picks which slot of a [shared group](#shared-section-controls) its groups act on |
| `level1` … `level9` | fader, knob | Entry *n* of the part's slider set, its drawbar or element level, a fader in the set's direction (RFC 0042) |
| A role's name (`cutoff`, `resonance`, `attack`, `release`, `lowGain`, `midGain`, `highGain`, `level`, `mix`, and draft 7's below) | knob, fader, encoder, button | The part's parameter with that role |
| `effect:insertion-a`, `effect:insertion-b` | button | Switches the part's node with that node role on and off |
| `effectAmount:insertion-a`, `effectAmount:insertion-b` | knob | That node's `mix` role |
| `send:reverb`, `send:variation` | knob | The part's send level into the bus whose chain holds a node with that role |

Role names are the strings the role banks already use (`roleBanks`, `physical_controls.h`), so one
spelling serves banks and sections.

### Shared section controls

The boards share controls among sections ([above](#the-hardware)): the V-STAGE's harmonic bars
edit its upper, lower or pedal organ and its synth knobs synth A or B; the YC's drawbars its
lower or upper organ; the CK's filter, envelope, drive and effect controls Part A, B or C. So a
group's `section` may be a **list** of slots, and the group then acts on one of them at a time:

```json
{ "kind": "buttons", "count": 3, "role": "fixed", "section": ["organ", "organ-lower", "organ-pedal"],
  "lights": "state", "functions": ["select:organ", "select:organ-lower", "select:organ-pedal"] },
{ "kind": "faders", "count": 9, "role": "fixed", "section": ["organ", "organ-lower", "organ-pedal"],
  "functions": ["level1", "level2", "level3", "level4", "level5", "level6", "level7", "level8", "level9"] }
```

- **`select:<slot>`** buttons pick which of a list the groups with that same list act on. Pressing
  several together picks all of them, so one move drives each (the CK's several Part buttons, the
  V-STAGE's dual synth mode). The first slot of the list is picked at start.
- **What is picked is the panel's state,** kept by the engine and pushed as group positions are
  now (`controllers.get`), not saved, like a group's page or mode. Its lights show it.
- **Still bound, not followed:** each slot is bound to its own part by the Performance, and the
  select buttons choose among those bindings. Selecting a part on Play's screen does not move them;
  that is RFC 0042's `element` mode on the assignable sliders, a different thing.
- **Pickup** is per control and target already, so moving from the upper to the lower organ makes
  each bar wait for the lower organ's value, and RFC 0042's position marker shows where each bar
  sits meanwhile. That is the V-STAGE's problem H-BAR MANUAL answers; Oscairn answers it with a
  `takePositions` function (decided 2026-11-11), built after RFC 0042's `places`: it sets each
  control of the group's target to where the physical control sits (its raw place, flipped for
  a reversed set as a move would be), ending pickup for the whole group, as one undoable edit
  to the parameters.
- Identities of a shared group are named by the list's first slot (`organ/fader:3`).

### What each control acts on

Three kinds, and nothing else:

- **Part properties,** which every part has whatever it holds: on/off (mute), volume (level),
  octave (transpose), split and layer (zones), sends. These never go dark while the section has a
  part.
- **Declared roles:** drawbars and element levels (the slider set), cutoff, percussion. A role
  is declared by the part's own `roles`, which come with its Sound, else by its plugins'
  `oscairn.ui` hints (RFC 0042's precedence). A control whose role neither the part nor any
  plugin in it declares goes **dark**: its light off, its tile on the screen blank with "None in
  this Sound", and moving it does nothing. It is looked up again whenever the part's chain, its
  `roles` or its parameter info changes (a Sound loaded, a plugin replaced, `parts.roles`, a
  rescan), as banks are now. For a plugin that declares no roles at all, the roles guessed from
  its parameters' names (`roleFromName`, as the banks use them) count, except a role the part's
  `roles` names, and the tile marks such a control "guessed" (decided 2026-11-11): the rule that
  nothing guesses is about what a Sound *is*, which the guess does not touch, and without it most
  third-party synths would be dark in the synth section. The guess never yields a `levelN`, so a
  slider set is never guessed.
- **Node roles,** for effects: the part's node carrying `insertion-a` or `insertion-b`, and the
  buses holding a `reverb` or `variation` node. Dark when the part or the Performance has none.

A role the part's `roles` names is that parameter, on whatever node. Otherwise roles are looked
up in the plugins' hints across the **whole chain** in chain order, instruments first, then
effects: unlike the role banks, which skip effect nodes (`physical_controls.cpp` around line
108), because a rotary speaker is often an effect after the organ. The first parameter declaring
a role wins, as in the banks. The slider set is the one exception: it is RFC 0042's set, found
as that RFC finds it, so a section's faders and the assignable sliders in `element` mode always
play the same entries.

Each part property and role, decided per row:

| Section control | What it is | Today | To add |
| --- | --- | --- | --- |
| Section on/off | Part property: `mute` | `Part::mute`, `parts.level`'s `mute` | Nothing; a scene's mutes switch sections, as they mute parts now |
| Section volume | Part property: `level` | `Part::level`, a live gain | Nothing |
| Octave | Part property: every zone's `transpose`, moved together by 12 | `NoteZone::transpose` | Nothing; held notes keep the pitch they started at, as a zone edit does now (to check in the engine when step 1 builds) |
| Split and layer buttons | Part property: zones | `PartInput::zones`, `parts.input` | Nothing in the format; a device default split key ([below](#split-and-layer-on-one-keybed)) |
| Drawbars or element levels 1 to 9, direction | Declared: RFC 0042's slider set, `level1` to `level9`, and its direction | `LEVEL_1` to `LEVEL_8`; the ninth, the direction and the part's `roles` not built (RFC 0042's draft 6 and format change) | Nothing beyond RFC 0042 |
| Percussion on, level, decay, harmonic | Declared role | None | `PERCUSSION`, `PERCUSSION_LEVEL`, `PERCUSSION_DECAY`, `PERCUSSION_HARMONIC` |
| Rotary slow/fast, stop | Declared role | None | `ROTARY_SPEED`, `ROTARY_STOP` |
| Vibrato/chorus on, type | Declared role | None | `VIBRATO_CHORUS`, `VIBRATO_CHORUS_TYPE` |
| Organ drive | Declared role | None | `DRIVE` |
| Attack, release, cutoff, resonance | Declared role | `ATTACK`, `RELEASE`, `CUTOFF`, `RESONANCE` | Nothing |
| Synth decay, sustain, filter envelope | Declared role | `DECAY`, `SUSTAIN`, `FILTER_ENVELOPE` | Nothing |
| Piano tone | Declared role | None (EQ gains are not a piano's one tone knob) | `TONE` |
| Piano EQ | Declared role | `LOW_GAIN`, `MID_GAIN`, `HIGH_GAIN` and the rest | Nothing |
| Insertion effect on/off | Node role, the node's `enabled` | `graph.role`, `graph.enable` | Nothing |
| Insertion effect amount | Node role plus that node's `MIX` | `OSCAIRN_ROLE_MIX` | Nothing |
| Effect sends | Part property: `sends`, to the bus whose chain holds a `reverb` or `variation` node | `parts.send` | Nothing |
| Master effects | Not a section's: the Performance's master bus, played by `fixed` or assign controls as now | `master` | Nothing |

### The roles to add: `oscairn.ui` draft 7

Draft 7, after RFC 0042's draft 6 (if both build in one change, one draft). The header is not
edited by this RFC. New `oscairn_role` values, continuing after draft 6's `LEVEL_9` (36):

| Role | Value | Meaning |
| --- | --- | --- |
| `OSCAIRN_ROLE_PERCUSSION` | 37 | Stepped, two values: the organ's percussion off and on |
| `OSCAIRN_ROLE_PERCUSSION_LEVEL` | 38 | Its level: two values (normal, soft), or continuous |
| `OSCAIRN_ROLE_PERCUSSION_DECAY` | 39 | Its decay: two values (slow, fast), or continuous, faster when higher |
| `OSCAIRN_ROLE_PERCUSSION_HARMONIC` | 40 | Stepped, two values: second, third |
| `OSCAIRN_ROLE_ROTARY_SPEED` | 41 | Stepped, two values: slow, fast; a plugin with a separate brake uses `ROTARY_STOP` |
| `OSCAIRN_ROLE_ROTARY_STOP` | 42 | Stepped, two values: turning, stopped (the brake) |
| `OSCAIRN_ROLE_VIBRATO_CHORUS` | 43 | Stepped, two values: the scanner vibrato off and on |
| `OSCAIRN_ROLE_VIBRATO_CHORUS_TYPE` | 44 | Stepped: its settings in order (V1, V2, V3, C1, C2, C3), named by value text |
| `OSCAIRN_ROLE_DRIVE` | 45 | An organ's or amp's overdrive amount |
| `OSCAIRN_ROLE_TONE` | 46 | One brightness control, darker when lower: a piano's or electric piano's tone |

Rules, in the header's own terms: a stepped role's values run from off or slow at its minimum; a
button playing a two-value role toggles it; a button playing a stepped role of more values steps
to the next and wraps. A plugin whose percussion is one parameter of several values (off, second,
third) declares it as `PERCUSSION` with its value texts, and the section's `percussion` button
then steps it; to check against the first organ plugin. A host that does not know draft 7
reads these as `NONE`, as the header already says for unknown values. A Sound can give these
roles to a plugin that does not declare them, through its part's `roles` (RFC 0042), so a
converted organ Sound's percussion and rotary reach the section without the plugin changing.

None of these join the role banks (Filter, Amp EG, EQ, Levels): they are section controls, and a
bank of organ switches on a synth's knobs would be blank on every other Sound. No "Organ" role bank
is added for boards without an organ section, for now (decided 2026-11-11): RFC 0042's slider
set on the sliders covers the main need.

### The binding on a part

The Performance format gains, on each part, **`section`**: a slot id, or absent. At most one part
of a Performance carries a given slot; binding a second part to a slot unbinds the first. A part
carries at most one slot. It is not in a Sound (a Sound is a part without name, input or output,
and a section is the Performance's layout, like the name), so loading a Sound into a bound part
keeps the binding.

- **Saved** with the part, in the next Performance format version, one forward as the
  [initial-development policy](../developer-guide.md#initial-development-compatibility-policy)
  allows; RFC 0042's part `roles` and pin and RFC 0043's `stateSavedBy` would share that version
  if they land together. `docs/performance.schema.json`'s `Part` gains `section` (a string of at
  most 32 bytes, lowercase letters, digits and `-`).
- **Kept whatever the device:** a slot this device does not have is kept, unread, and saved back
  as it was ([below](#the-same-performance-elsewhere)).
- **An edit:** `sections.bind` is undone by `edit.undo` and marks the Performance edited.
- **Scenes do not change bindings.** A scene changes what a section's controls act on through
  the part's own state, which scenes already record: its mute (the section's on/off), its level
  (the volume), its parameters (drawbars, percussion), its sends, and which plugins are on
  ([RFC 0011](0011-performance-ui-and-editors.md#scenes-the-full-model-decided-2026-10-31)). A
  binding is structure, like part order. A song whose chorus wants a different sound in the organ
  section uses two parts, both able to sit there, and a scene that mutes one and unmutes the
  other; the section shows the playing one (decided 2026-11-11).

### Auto-binding

Runs when a Performance opens on a device whose profile has sections, for the sections no part is
bound to. It never moves a part that already carries a slot this device has.

**Candidates.** A part is a candidate when its input is `notes`, it carries no slot this device
has, and it has a **category**: the tags of the library Sound in its `from.sound`, read from the
library index. A part with no `from`, or whose Sound is not in this device's library, has no
category and is never a candidate. The part's name and plugins are never read (decided: nothing
guesses).

A part **matches** a section when its Sound's tags hold the section's `category`.

**One obvious match.** For every unbound section and every candidate, build the match table. A
section and a part are an obvious pair when the section matches exactly one candidate **and**
that candidate matches exactly one unbound section. Every obvious pair is bound at once; the rule
looks at the whole table, so the result never depends on the order sections or parts are listed.

- A section that matches no candidate stays empty and asks nothing. Its controls are dark but for
  `on` and the sound select, which create a part ([below](#sound-select-within-a-section)).
- A section that matches two or more candidates, or whose one candidate also matches another
  section (a Sound tagged both `piano` and `epiano`), is **not obvious**, and asked about.
- Parts left unbound are extra parts: they play as they did, and the Play screen lists them.

**The prompt.** One question per section that is not obvious, as a sheet
([RFC 0030](0030-system-shell-launcher-and-navigation.md#overlays)), the Performance playing
underneath:

> **Which part plays the Organ section?**
>
> ○ Part 2 · Tonewheel Full · organ
> ○ Part 4 · Gospel B · organ
> ○ None for now
>
> **Cancel** · **Use Part 2**

- The first candidate in part order is chosen; one tap or one encoder push confirms it. **Cancel**
  and Back leave the section empty, as **None for now** does.
- The sheet comes up after the Performance is playing, never before, and never blocks a note.
- **Not during a show:** while the transport runs, or when the Performance came from a Setlist at
  `instrument` and above ([RFC 0027](0027-reliability-levels.md)), the sheet does not open; a
  banner says "2 sections not bound" with **Bind**, which opens it.
- **On the compact surface** (320 x 240, four encoders, [RFC 0019](0019-adaptive-ui.md#compact)):
  the same question as a list, one line per candidate, the first encoder going back (None for
  now) and the last confirming, as compact lists work now.
- **On the panel:** the section's own `on` light blinks while its section waits for an answer, and
  pressing the section's `on` button confirms the chosen candidate; turning the section's `sound`
  encoder moves the choice. So a board with only a small screen can still answer.
- What auto-binding and the prompt chose is an edit, as `sections.bind` is: Save keeps it; leaving
  without saving forgets it, and the next open asks again.

**Re-binding later.** From the part's menu (**Section › Organ**, or **None**), from the section's
tile on Play (**Change part**), or by holding a section's `on` button and pressing the selected
part's strip. **Bind again** in Play's menu runs auto-binding for every section with all bindings
cleared, and is undoable.

**Cases:**

| Case | What happens |
| --- | --- |
| Two parts of one category, one section | Asked; the other stays an extra part |
| A two-part organ (a MODX organ converted as two parts, both tagged `organ`), one `organ` section | The same case: not obvious, so asked. The part chosen is bound and the other stays an extra part, edited on screen, its own slider set (its ninth drawbar) reached as on the MODX, by selecting it so the assignable sliders follow it in `element` mode. Nothing joins the two parts (RFC 0042) |
| The same two-part organ, two organ-category slots sharing controls (`organ` and `organ-lower`, picked by `select:` buttons) | The next row's case: asked for each; each slot then binds one of the parts, and the select buttons switch the shared drawbars between their sets |
| Two parts of one category, two sections of that category (`upper`, `lower` both `synth`) | Asked for each; a part chosen for one leaves the other's list |
| A section with no part | Empty; its `on` or sound select creates one |
| More sections than parts | The rest empty, none asked |
| More parts than sections | Bound ones in their sections, the rest extra parts |
| A part with an audio input (a vocoder's mic) | Never a candidate; it can be bound by hand, and its on/off and volume work (a drawbar does not) |

### Sound select within a section

A section's sound select browses the library's Sounds whose tags hold the section's `category`:
`library.query` with `kind: "sound"` and `tags: [category]` as it works today. Choosing one is
`parts.loadSound` on the bound part, so it keeps the part's zones, level, sends and binding, and
switches seamlessly where the plugins allow
([RFC 0021](0021-sounds-performances-and-library.md#seamless-switching)).

- **Order:** favorites first (each item's `favorite` in `library.query`), then the rest by name, so the `sound`
  encoder starts on the musician's own picks. `favorite` steps favorites only.
- **Narrower categories:** a section may list extra tags, `subcategories`, each a `categoryN`
  button (a piano section's grand, upright, electric). Pressing one narrows to Sounds with both
  tags; pressing it again widens back.
- **The display:** the section's `display` and its Play tile show the Sound's name, then "edited"
  while the part differs from it (`from.modified`).
- **The section's default Sound** is what an empty section's `on` loads: the musician's choice per
  device and section, a setting kept beside favorites in the library's user file (never in an item
  or the Performance); else the first favorite of the category; else the first Sound of the
  category by name; else nothing, and the button says "No Organ Sounds in the library".
- **An empty section's `on`,** or turning its `sound` encoder: `parts.fromSound` with the default
  (or the chosen) Sound, a new part listening to every key, bound to the section. It is an edit,
  and it passes the budget check adding a Sound passes ([RFC 0025](0025-resource-budgets.md)):
  refused, with the shortfall on the section's display and the screen, when it does not fit.
- **Tags decide, and only tags.** A Sound with no category tag is reachable from the library as
  now and can be loaded into a section's part from there; the section's select does not list it.
  The tags a factory library gives its Sounds are the publisher's; the musician can add tags as
  now ([RFC 0021](0021-sounds-performances-and-library.md#where-tags-are-stored)).

### Overriding a section control

By default a section control acts on its bound part's property or declared role, as above. The
owner (2026-11-11): it "should be overrideable in device settings or performance controls so like
a performance could forward synth controls to 2 of its plugins." So each section control resolves
in three layers, the first that is set winning:

1. **The Performance's override,**
2. **the device's override,**
3. **the section default:** the bound part's property or role.

This is RFC 0011's layering ([defaults](0011-performance-ui-and-editors.md#defaults-so-nothing-must-be-programd))
with a device layer between the Performance and the defaults, for section controls only.

#### Performance overrides

A Performance overrides a section control the way it overrides an assign control today: an entry
in its `assignments`, by the control's identity (Performance format, the `assignments` row;
`AssignmentOverride` in `core/control/include/oscairn/parts.h`). No new field: `control` accepts a
section identity (`synth/knob:1`) beside `knob:N` and the rest, and `validPhysicalControl` and
`assignments.set` learn them. The kinds stay those `assignments` has:

- **`control`, the main case:** the knob drives one of the Performance's controls, which can have
  any number of targets, each with its own range (`start`, `end`, end below start reversing),
  curve, `relative` and the scenes it is off in (`controls.targets`,
  [control protocol](../control-protocol.md#requests-and-responses)); a relative target on a
  parameter that takes CLAP modulation is sent as modulation on top of the parameter's own value
  (`MacroTarget::delivery`, `parts.h`). So the synth section's cutoff knob drives a control whose
  two targets are two plugins' cutoffs, in the bound part or any other: the Super Knob mechanism,
  reached from a section control.
- `parameter`, `partLevel` and `partMute`, as for any override; `scene` and `panic` on a section's
  buttons, as on `button:N`.

What follows from it:

- **Dark or lit follows the override's target,** not the role: a control overridden to a
  Performance control is lit while that control exists (and an override whose target is removed is
  left out at save, as now, so the section default comes back); one overridden to a parameter is
  dark when its node is gone. A part-property override is never dark.
- **Pickup and RFC 0042's two-value display** apply to the override's target value, the
  Performance control's value for `control`, as pickup works for any target now: per control and
  target ([RFC 0011](0011-performance-ui-and-editors.md#the-controller-mapping-service)).
- **The screen says so.** The section's tile shows the control with its destination instead of its
  role: "Cutoff → Brightness (2 targets)", marked **Performance**; tapping it lists the targets and
  offers **Use the section's** (removing the override). The section's display shows the
  destination's name while the control moves.
- **Scenes** change which of a control's targets are active (`offIn`), as for any control; they do
  not change the override itself, as they do not change other overrides now.
- **A shared group** ([above](#shared-section-controls)): an override on `organ/fader:3` applies
  whichever slot the select buttons picked; the slots' bindings decide only the default layer.
- **Not in a Sound.** A Sound's own assignments (`part` nonzero) stay what they are: they may name
  `wheel:2` and the other standard controls, not a section's identities, since a Sound must not
  depend on one board's panel. What a Sound says for sections is its part's `roles` (RFC 0042),
  which name its parameters, never a control.

So a slot binds one part, and its controls may still reach other parts or plugins through a
Performance's override.

#### Device overrides

A per-device remap of a section control: "on this board, the synth section's fourth knob is
always decay". It names only what any Performance has, so it can apply to all of them:

- another section function or role name from the vocabulary (`decay` instead of `release`,
  `effectAmount:insertion-b` instead of `effectAmount:insertion-a`), acting on the bound part as a
  default does;
- a Performance control by name (`control:Super Knob`, as a layout's `control:NAME` binds one),
  dark in Performances without one of that name;
- `none`, to leave a control doing nothing.

It is stored in the data folder, per device, in `sections.json` beside `controllers.json`
(`controllers.choose`'s), never in the profile, which describes the product, and never in a
Performance. The same file keeps the default split key ([below](#split-and-layer-on-one-keybed))
and each section's default Sound is kept beside favorites, as above. Settings gains a **Panel
sections** page (an entry in the page list of `hosts/standalone/apps/settings/Pages.cpp`, shown
only where the device's profile or a connected controller's has sections, as other pages there are
shown by what the profile has): each section with its controls, each control's function, a changed
one marked with **Reset**, and the default split key.

### Locked sections

A fixed-engine sectioned board ([how others do it](#how-others-do-it)) is one case of this design:
a profile whose new Performance has one part per section, each holding only Sounds of the
section's category. So a section may be **locked** by its product's profile, for a simple stage
instrument; an open section, the default, suits a workstation (decided 2026-11-11: opt-in per
section, device profiles only, off by default; every built-in profile and the desktop stay open):

- **`locked: true`** on a section in the profile. Only a device profile may lock; a controller's
  sections are always open.
- **Only its category:** `parts.loadSound` and the section's sound select refuse a Sound without
  the section's `category` tag into a locked section's part ("Only Organ Sounds go in the Organ
  section"). Editing the part's chain by hand is refused too (`parts.insert`, `parts.removeNode`,
  `graph.replace` on its nodes), so the part stays what the category says; its parameters, level,
  zones and sends edit as any part's.
- **Always there:** its part cannot be unbound or removed (`sections.bind` with another part or
  0, and `parts.remove`, are refused, naming the section). Its `on` mutes it; that is how a
  section is turned off.
- **A new Performance** on such a device starts with one part per locked section, each loaded with
  the section's default Sound ([above](#sound-select-within-a-section)) and bound; open sections
  start empty, as now.
- **A Performance that arrives** without a fitting part for a locked section gets one made from
  the default Sound when it opens, and its own parts stay extra parts. That makes an edit, saved
  only if the musician saves.
- **Overrides still work.** A lock restricts the section's Sounds and its binding only; a
  Performance's and the device's overrides of its controls ([above](#overriding-a-section-control))
  apply as on an open section. No profile flag forbids them until a product needs one.
- **Nowhere else:** the lock is the device's, not the Performance's. On the desktop or an open
  board the same Performance has ordinary parts with labels, and every edit is allowed.

### Split and layer on one keybed

Sectioned boards play all their sections from one keybed, layered or split. In Oscairn that is
already the parts' keyboard zones: each part's input has zones with key ranges
(`PartInput::zones`, set by `parts.input`, saved with the part, [RFC 0012](0012-sync-external-midi-and-multi-device.md#choosing-devices-selectors)).
So a section adds no split model (decided 2026-11-11). Its buttons edit the bound part's zones,
and the zones stay the only truth about who plays which keys.

- **`layer`** sets the part's key range to the whole keyboard, 0 to 127; **`lower`** to 0 up to
  the boundary − 1; **`upper`** from the boundary to 127. Each keeps the zone's velocity range,
  channels, curve and transpose. Each press is a `parts.input` edit, undone by `edit.undo`.
- **Which button is lit** is read from the zones every time: `layer` when the part's range is
  0 to 127, `lower` when it starts at 0 and ends below 127, `upper` when it starts above 0 and
  ends at 127. Anything else lights none.
- **The boundary is not stored.** It is worked out from the zones when a button is pressed: the
  lowest key of the first bound part (in part order) whose range is `upper`-shaped, else one above
  the highest key of the first `lower`-shaped one, else the device's default split key. So it can
  never disagree with the zones; if two parts' zones disagree (one ends at B3, another starts at
  E4), each part plays what its zones say, Play shows both ranges, and the buttons use the first
  as above.
- **The device's default split key** is a device setting, kept with the section remaps
  ([above](#device-overrides)), 60 when unset (middle C, which Oscairn names C4: `noteName` in
  `core/control/src/pads.cpp`, used by `ZoneEditor`). It is never written into a Performance.
- **Setting the split point on the panel:** holding a section's `lower` or `upper` and pressing a
  key moves the boundary to that key, as the V-STAGE's SPLIT POINT does
  ([above](#the-hardware)). That is an edit of the zones themselves: every bound part whose range
  is `lower`- or `upper`-shaped at the old boundary gets its range moved to the new one, in one
  undoable step. The key pressed plays no note.
- **Zones the buttons cannot express** (several zones with different key ranges, a velocity limit
  on any zone, a range starting above 0 and ending below 127): the buttons light none, the
  section's display and Play's tile show the range as it is ("C2–F♯3, velocity 1–90"), and a press
  changes nothing and says "Zones set on the Part page". They show it and leave it (decided).
  Several zones sharing one key range and full velocity (one per channel) are one range to the
  buttons.
- **Three or more regions** need nothing more: each part's range is its own, so a lower, middle and
  upper part are three parts' zones, edited on the Part page; the buttons light none for the middle
  one, as above.

**An external keyboard.** A part already chooses which inputs play it (RFC 0012's selectors: the
built-in keys, all external, USB by name). A section's part played from a second keyboard is a
part whose input selects that keyboard; its section's buttons and knobs work the same. Its
`lower`/`upper` buttons edit its zones the same way, which is right for a second manual only when
it is split; the Part page edits its zones otherwise. No section rule knows about inputs.

**A controller with a section of its own.** A controller profile may declare `sections` and
section groups too (a drawbar controller, say). Its groups join the device's
([RFC 0024](0024-device-profiles.md#controllers)), and a section with the same `id` as one of the
device's is the same slot: both sets of controls act on the one bound part, each under its own
pickup. A slot only the controller has is a slot while it is connected.

### The same Performance elsewhere

| Where | What happens |
| --- | --- |
| The board it was saved on | Bindings as saved; no prompt |
| The desktop, no sectioned controller | Each bound part shows its slot as a label ("Organ") in the parts list and on Play's strip; nothing is driven, nothing is asked, and Save keeps the labels |
| The desktop with a controller profile that declares sections | As a sectioned board, for those sections |
| A board with other sections | Slots it shares by `id` bind as saved; parts bound to slots it lacks keep them, shown as labels, and are candidates for its unbound sections only through the prompt, never by the obvious rule; choosing one there rebinds the part (one slot per part), which a Save then keeps |
| A board with no sections | As the desktop |

A slot id kept but not on this board is never dropped by loading, by a scene or by an edit that
does not touch that part's binding.

### What the shell shows: Play on a sectioned device

Play (RFC 0011's [first view](0011-performance-ui-and-editors.md#play-the-performance-apps-first-view))
keeps its scenes along the top and its strips, and gains a **section row** above the strips when
the profile has sections:

- **One tile per section,** in the panel's order, each with: the section's name; the bound part's
  name and Sound, or "Empty"; on/off, volume and octave; layer, lower or upper; and the section's
  controls in the panel's order, each with its value as text and, for absolute controls, RFC 0042's
  position marker until pickup catches it. Dark controls show "None in this Sound". The tile's
  controls are touch controls as well as a mirror of the panel.
- **A section with level faders** (an organ's drawbars) draws RFC 0042's slider row inside its
  tile, the bound part's whole set, wherever the physical faders are.
- **Tapping a tile** selects its part, so the knob row's existing part switch and RFC 0042's
  `element` mode follow it. **Change part** and **Sound** on the tile open the binding menu and
  the section's sound select (a sheet listing the category, favorites first).
- **Extra parts** keep their strips after the bound parts', under "Other parts", and are edited on
  screen as now (Edit, the Part page).
- **The top bar** is unchanged (RFC 0030); its context line names the Performance and scene. A
  section waiting for the prompt shows as the banner above.
- **Small panel screens** (the FANTOM-07's 1280 x 720 at 140 mm, [RFC 0024](0024-device-profiles.md#what-a-profile-holds))
  show one tile per section side by side when they fit at 44-unit targets, else one at a time with
  the section names as tabs. **Compact** shows a list of sections, each opening its controls as a
  list of parameters, as compact editors do now.
- The design language's rules hold: every value as text, state by text and position and not color
  alone, a focus ring, keyboard operation ([design language](../design-language.md)).

### The test panel on the desktop

So that steps 1 to 9 are testable before any sectioned board runs Oscairn:

- **A test profile,** `hosts/standalone/profiles/sections-test.json`, marked `estimated` and named
  as a test board (not any product), with piano, organ and synth sections shaped like the boards
  [above](#the-hardware): a 76-key keybed, nine drawbar faders, the buttons and knobs per section.
- **A test layout,** `hosts/standalone/layouts/sections-test.json`, drawing those sections as
  panels with their controls.
- **Layouts gain a `fader` control type and an `as:` binding.** `control:` already names a
  control of the control tree, so a layout control names a control identity with `as:`:
  `"bind": "as:organ/fader:3"`, on a knob, fader or button. The panel sends its moves through
  `controllers.play` to the engine, which plays them through the controller-mapping layer a
  panel's or controller's controls go through, with pickup (`Session::controllerControl`). So the
  desktop exercises the same path the hardware will.

## Control API additions

New and changed, with permissions as `appGrants()` in `core/control/src/json_rpc.cpp` would give
them. Reading is `play` or `edit`, as `assignments.get` is; binding and creating parts is `edit`,
as `parts.*` is; the per-device default Sound is `library`, as favorites are.

| Method | Takes | Returns | Permission |
| --- | --- | --- | --- |
| `sections.get` (new) | none | `sections`: each `{id, name, category, part}` (`part` 0 for empty), with `controls`, each `{control, function, target, value, dark, guessed, layer}` (`control` its identity, `organ/fader:3`; `layer` which layer it resolved from: `performance`, `device` or `section`), and `pending`: the sections auto-binding left to ask about, each with its `candidates` | `play` or `edit` |
| `sections.bind` (new) | `section`, `part` (0 to empty it) | `{accepted:true}`; an edit. Refused: an unknown section, a part that does not exist, and on a locked section another part or 0, naming the section | `edit` |
| `sections.auto` (new) | optional `clear` (re-bind every section) | `{bound, pending}`: the pairs it bound and the sections still to ask about; an edit | `edit` |
| `sections.create` (new) | `section`, optional `sound` (a library ID; absent, the default) | `{part}`: `parts.fromSound`, bound; refused over the budget, with the shortfall | `edit` |
| `sections.device` (new) | none: the device's overrides and default split key; or `remaps` (each `{control, function}`, `function` a vocabulary function, a role name, `control:NAME` or `none`, or null to reset) and optional `splitKey` (0 to 127) | The device's `remaps` and `splitKey` after the change; kept in the data folder's `sections.json` | `system`, as `controllers.choose` |
| `assignments.set`, `assignments.get` | Accept and list section identities (`synth/knob:1`) as `control`, with the kinds they take now; a Sound's own assignment (`part` nonzero) to a section identity is refused | | `edit`; `play` or `edit`, as now |
| `sections.setDefault` (new) | `section`, `sound` (a library ID, or empty) | `{accepted:true}`; kept per device in the library's user file | `library` |
| `controllers.play` (new) | `control` (an identity, a section's too), and one of `position` (0 to 1), `steps` (a relative step) or `press` (true or false) | `{accepted:true}`: played as a panel control is, with pickup | `play` |
| `controllers.get` and the `controllers` topic | RFC 0042's `places` (raw positions) include sections' controls by identity | | as now |
| `parts.loadSound`, `parts.insert`, `parts.removeNode`, `graph.replace`, `parts.remove` | On a locked section's part: a Sound without the section's category, a chain edit, and removing the part are refused, naming the section | | as now |
| `graph.get` | Each part gains `section` (or `null`) | | as now |
| `session.hello` | Its `capabilities` list the new methods | | |

The `sections` topic is not added: `sections.get` changes when the graph does (`graph.changed`)
or a control moves (`controllers`), and clients read it again then.

## Build order and gates

| Step | Deliverable | Gate |
| --- | --- | --- |
| 1. Sections and bindings | `sections` and a group's `section` in `profile.schema.json` and `Profile.cpp`; the part's `section` in the format and schema; `sections.get`, `sections.bind`; part-property functions, the split and layer buttons editing zones, the device's default split key (`sections.device`'s `splitKey`); `controllers.play`; the layout's `fader` type and `as:` binding; the test profile and layout | On the desktop with `--profile sections-test --layout sections-test` and three parts: binding part 2 to the organ section makes the organ's volume fader move part 2's level and its `on` mute it; `octaveUp` transposes part 2's zones by 12; `lower` and `upper` set part 2's zones at the boundary worked out from the other parts' zones (or the default), holding `upper` and pressing a key moves every split-shaped part's range in one undoable edit, and a part with a velocity-limited zone keeps it untouched while the buttons light none; unbinding darkens the section; `edit.undo` restores each. `desktop-profile`: a `section` on an assign group, an unknown function, and two sections with one `id` are refused. `control-performance-format`: a saved binding reloads; a slot the profile lacks is kept byte for byte through load and save. `desktop-shell` snapshots the layout |
| 2. Roles and RFC 0042's mechanisms | Declared-role functions with today's roles; dark controls; guessed roles; shared groups and `select:`; RFC 0042's slider set, direction, pickup and places on section controls; `takePositions` | `control-physical-controls`: a synth section's `cutoff` knob plays the bound part's cutoff role, whatever plugin; a part with no cutoff role leaves it dark and a move changes nothing; a rotary role on an effect after the instrument is found. With the Oscairn Organ and a part whose `roles` declare its drawbars as a reversed slider set (RFC 0042): the nine section faders play them, reversed; pickup holds a fader until it reaches the value and `places` reports it meanwhile; the shared sliders in `element` mode and the section faders drive the same parameters; a part whose `roles` name a parameter for `cutoff` plays it in place of the plugin's own cutoff role; a plugin with no declared roles gets its guessed cutoff, reported `guessed`; `takePositions` sets each drawbar to its fader's position and a following move plays at once, undone by one `edit.undo` |
| 3. Auto-binding | The rule, the sheet, the compact list, the panel's blink and confirm, the banner, `sections.auto` | A new `control-sections` test: tables covering one obvious pair, two candidates, a part matching two sections, a part with no `from`, an audio part, and list order changed with the same result; the prompt is not opened while the transport runs at `instrument`; `desktop-gaps` renders the sheet at 1024 x 600 and the compact list at 320 x 240 |
| 4. Sound select | Category browse, favorites first, subcategories, the default Sound, `sections.create`, `sections.setDefault`, locked sections | `control-library`: the `sound` encoder steps tagged Sounds favorites first and loads each into the bound part keeping its zones, level and binding; an empty section's `on` creates a bound part with the default; over the budget it is refused naming the shortfall. With a locked organ section: a new Performance has its organ part; loading a pad into it, removing it and unbinding it are refused naming the section; on the desktop the same Performance allows all three |
| 5. Draft 7 roles | The roles in `ui.h` as draft 7; `roleName` and the role parser know them; a test organ plugin declares percussion, rotary, vibrato/chorus and drive | `control-physical-controls`: each section button toggles or steps its role's parameter; a draft-5 plugin reads as before; an unknown role value reads as none |
| 6. Effects per section | `effect:`, `effectAmount:` and `send:` functions | `control-physical-controls`: the button switches the part's `insertion-a` node off and on, draining as `graph.enable` does; `send:reverb` moves the part's send into the bus with a `reverb` node; with no such node, dark |
| 7. Play and feedback | The section row on panel and compact surfaces; lights and displays sent where a group's `midi` has `feedback: true` | `desktop-shell`: Play on the test profile in every built-in theme and both input modes, keyboard alone reaching every section control; a controller profile with a section's lights gets each light's state on its output when the state changes |
| 8. Elsewhere | Labels on the desktop and on boards without the slot; a controller profile with a section | Opening a Performance saved on the test profile on the desktop shows the labels and saves them back unchanged; on a second test profile with only `piano` and `synth`, the organ binding is kept and the prompt offers that part for an unbound section; a controller profile with an `organ` section drives the bound part beside the device's |
| 9. Overrides | Section identities in `assignments` and `assignments.set`; the device layer, `sections.json`, `sections.device`'s `remaps`; the tile's destination and **Use the section's**; the Settings page | `control-physical-controls`: the synth section's cutoff knob overridden to a Performance control with two targets in two plugins moves both, each in its range, pickup waiting on the control's value; removing the control brings the role default back at save; a device remap to `decay` applies in every Performance and yields to a Performance override; a locked section's control is overridden as an open one's; a Sound's assignment to a section identity is refused. `control-performance-format`: the override saves and reloads. `desktop-settings`: the page lists, changes and resets a remap |

## Decided

By the owner, 2026-11-11, as recommended in the first draft; recorded in the sections above.

- Roles guessed from parameter names count in sections, marked "guessed"
  ([what each control acts on](#what-each-control-acts-on)).
- A slot binds one part; its controls may reach other parts or plugins through a Performance's
  override ([overriding a section control](#overriding-a-section-control)). A board's several
  sounds in one section (the V-STAGE's upper, lower and pedal organ, its synth A and B) are
  several slots sharing controls.
- Scenes do not change bindings ([the binding on a part](#the-binding-on-a-part)).
- No "Organ" role bank for boards without an organ section, for now.
- The thirteen-id slot vocabulary, documented in RFC 0024 when step 1 builds, open to additions.
- `takePositions`, after RFC 0042's `places` ([shared section controls](#shared-section-controls)).
- A profile may lock a section: opt-in per section, device profiles only, off by default
  ([locked sections](#locked-sections)).

Changed by the owner the same day:

- **Splits are the parts' keyboard zones.** No split key in the Performance; the buttons edit the
  bound part's zones, and the boundary they use is worked out from the zones or a device default
  ([split and layer](#split-and-layer-on-one-keybed)).
- **Section controls can be overridden,** by a Performance and by the device, in that order before
  the section default; a lock does not stop overrides
  ([overriding a section control](#overriding-a-section-control)).

Decided by the owner later the same day, as recommended:

- **A device remap may name a Performance control** (`control:Super Knob`); the control is dark in
  a Performance with no control of that name, as layouts' `control:NAME` already works
  ([device overrides](#device-overrides)).
- **When parts' zones disagree on the boundary,** the buttons use the first bound part's, in part
  order, and Play shows both ranges; refusing the buttons until the zones agree would make a quick
  button slow ([split and layer](#split-and-layer-on-one-keybed)).
- **No profile flag forbidding overrides on a locked section** until a product needs one
  ([locked sections](#locked-sections)).

And with RFC 0042's revision of the same day:

- **Section controls use RFC 0042's slider set and role declaration:** a section's `level1` to
  `level9` faders play the bound part's slider set, and a Sound's part `roles` declare its roles
  before its plugins' hints ([what this RFC takes from RFC 0042](#what-this-rfc-takes-from-rfc-0042)).
- **A slider set never spans parts.** A two-part organ (as converted from a MODX) stays two parts:
  bound by the prompt to one organ section with the other an extra part, or to two slots sharing
  the section's controls ([auto-binding](#auto-binding)).

## Open questions

None open.

## Ruled out

- **Guessing a Sound's category** from its name, its plugin, or its parameters (decided
  2026-11-11): tags only.
- **Fixed section types in the engine** (a piano part, an organ part): a part stays any chain, and a
  section is a slot in the profile.
- **A second following mechanism beside RFC 0042's:** the slider set, the role declaration,
  direction, pickup and places are RFC 0042's, reused; a section is only who its controls follow.
- **A section's default reaching by plugin ID or parameter ID:** a section names roles, so it
  works with any Sound that declares them. Reaching a particular plugin is a Performance's
  override, which names its own nodes and controls.
- **A split model of sections' own** (a split key in the Performance, regions per section): the
  parts' zones already are the split (decided 2026-11-11).
- **Dropping bindings a device does not understand:** they are kept, so a Performance travels
  between boards and the desktop unchanged.
- **Auto-binding by order** (first organ part to the first organ section): the rule binds only
  what is obvious from the whole table, and asks the rest.
- **Blocking a Performance until its sections are bound:** it plays at once; the prompt comes after,
  or waits behind a banner on stage.
