# Running Oscairn on a Roland FANTOM-0

Oscairn has never run on a FANTOM-06, -07 or -08, and nobody has tried it on one: there is no
unit yet. This page says what is known about the hardware, what Oscairn has for it, what of that
is estimated, and how to map the panel once a unit is here, as of November 8, 2026. The
repository holds Oscairn's own code and data only: no Roland files, binaries or firmware, and
nothing that changes Roland's firmware or update packages.

## What is known

From the owner's notes on the hardware (Roland's GPL release of the kernel, U-Boot and device
tree, and Roland's test tool), and Roland's published specifications and owner's manual:

- **SoC:** NXP i.MX8M Mini, four Cortex-A53 cores, 2 GB RAM: the KICK's SoC. The FANTOM-06, -07
  and -08 share one main board and firmware; the -07 has 76 keys, velocity only, no aftertouch,
  and weighs 7.0 kg.
- **Screen:** 5.5", 1280 × 720, MIPI-DSI, with touch (a Goodix GT1151). The panel is portrait,
  turned to landscape.
- **Audio:** the i.MX's SAI1 runs a TDM link of 16 slots of 32 bits at 48 kHz to Roland's two
  BMC chips, which are the bit clock and frame master. ALSA calls it `hw:1,0`; Roland's app
  opens 6 channels, S32_LE. **Which slots carry which outputs is unknown**, and so is whether
  audio played into `hw:1,0` reaches MAIN OUT without Roland's app setting the BMCs up first:
  that is the deciding test on a unit.
- **Panel:** Roland's GPL driver `i2c-slave-rx` makes the i.MX an I2C slave at 0x08 on I2C3 and
  gives `/dev/i2c-slave-2`. Messages are 4 bytes, with **bit 7 set on the first byte**. Roland's
  test tool names four kinds: `PMDWEncoder`, `PMDWADChange`, `PMDWSwitchOn` and `PMDWSwitchOff`
  (encoders, analog controls, switches pressed and released). **Where the kind, the ID, the
  value and the step sit in the bytes is unknown.**
- **Keys:** a separate keyboard CPU scans them. **Where they reach the i.MX is unknown**: they may
  come through the BMC rather than the panel.
- **Panel controls** (Roland's spec sheet and owner's manual, "Panel Descriptions"): the pitch
  bend/modulation lever, WHEEL1 and WHEEL2, S1 and S2; control knobs 1 to 8 and sliders 1 to 8
  with the PAN/LEVEL, CTRL and ASSIGN buttons choosing what they play; MASTER VOLUME; the
  CUTOFF and RESONANCE knobs; FUNCTION knobs E1 to E6 under the screen, which also push; the
  VALUE dial, DEC and INC; ZONE INT/EXT 1 to 8 (whether each zone sounds), ZONE 9-16 and ZONE
  SELECT; SPLIT, CHORD MEMORY, ARPEGGIO, TRANSPOSE, OCTAVE DOWN and UP; WRITE, MASTER FX,
  MOTIONAL PAD, DAW CTRL, MENU, TEMPO, the four cursor buttons, SHIFT, EXIT and ENTER; SCENE
  SELECT, SCENE CHAIN, ZONE VIEW and SINGLE TONE; OSC, FILTER TYPE, PARAM, AMP, FX and LFO;
  PATTERN, GROUP, SONG, TR-REC, RHYTHM PTN, STOP, PLAY and REC; tone category buttons 1 to 16;
  SAMPLING, PAD MODE, CLIP BOARD, BANK and HOLD; 16 pads; HOLD, CTRL1 and CTRL2 pedal jacks; MIDI
  IN and OUT; USB COMPUTER, USB MEMORY and an external USB device port. Roland's spec counts
  **eight** control knobs and **eight** sliders on the FANTOM-0 (not four).

## What Oscairn has

- **A device profile**, `--profile fantom-07`
  ([hosts/standalone/profiles/fantom-07.json](../hosts/standalone/profiles/fantom-07.json),
  [RFC 0024](rfcs/0024-device-profiles.md)). It declares the i.MX8M Mini (four cores, one for
  audio, 2048 MiB), a 1280 × 720 touch screen, 48 kHz at 128-frame blocks, instrument
  reliability, the Performance, Groovebox, Library and Setlist apps, and its panel (below).
  **Estimated** (`estimated`): `audio` (two inputs for MIC/LINE, four outputs for MAIN and SUB
  OUT, headphones: the TDM slots behind them are unknown, and the profile has no field for the
  ALSA device or its 16 slots), `controls` (Roland's counts, but whether each reaches the i.MX,
  and the roles below, are Oscairn's guesses) and `midi`. The controls, in order, with what
  they are in Oscairn ([RFC 0024's identities](rfcs/0024-device-profiles.md#default-targets)):
  the lever as `wheel:1` (pitch bend) and `wheel:2` (modulation); WHEEL1 and WHEEL2 as
  `wheel:3` and `wheel:4`, unassigned; S1 and S2 as switches `button:1` and `button:2`; control
  knobs 1 to 8 as `knob:1` to `knob:8`, paged by button; sliders 1 to 8 as `fader:1` to
  `fader:8` on part levels; CUTOFF and RESONANCE as `knob:9` and `knob:10`, a row playing the
  Filter bank (its first two roles are cutoff and resonance); E1 to E6 as screen knobs
  `screen:1` to `screen:6`; ZONE INT/EXT 1 to 8 as `button:101` to `button:108` on part mutes;
  the VALUE dial, MASTER VOLUME and every other button as fixed functions; the keys and pads as
  notes; the HOLD jack as sustain and CTRL1 and CTRL2 as expression pedals, unassigned.
- **The screen:** Oscairn has no layout made for 1280 × 720 and no scaling by pixel density, so
  the profile shows `kick-screen`, the KICK's 1024 × 600 screen layout and the closest there is:
  a panel layout scales to fit (720 / 600, 1.2 times, about 1229 pixels wide with narrow bars at
  the sides). At 5.5" that is a dense screen, so everything is physically smaller than on a
  larger screen of the same layout. Untried.
- **A panel reader** in the engine, built only with `OSCAIRN_PANEL_ROLAND` (below).
- **No image.** The i.MX8M Mini EVK image (`oscairn_imx8mm_evk_defconfig`,
  [kick.md](kick.md)) is the nearest; the FANTOM's own device tree, display, touch, audio and
  boot path are not there.

## The panel reader

`oscairn::PanelReader` ([core/control/include/oscairn/panel_reader.h](../core/control/include/oscairn/panel_reader.h))
reads the device on a thread of its own, polling with a 100 ms timeout, so it never blocks the
audio or message thread and stops within that. A byte with bit 7 set starts a message; bytes
before one are skipped and a message cut short by the next one's first byte is dropped, so it
finds its place again after garbage or a lost byte, and messages split across reads join up.
Each message is decoded through the **panel map** and handed to the engine, which plays the
control it names through the controller-mapping layer a MIDI controller's controls go through
(`Session::controllerControl`, RFC 0011's instant controls): analog controls are positions with
pickup, encoders step from the current value, switches toggle on each press. Messages no entry
names are logged with their bytes, at most ten a second. The map can be replaced while it runs.
If the device fails or ends, it is opened again after a second.

The profile's `panel` says which reader, which device and which map:

```json
"panel": { "protocol": "roland-i2c-slave", "device": "/dev/i2c-slave-2", "map": "fantom-07.json" }
```

The app passes them to the engine it starts (`--panel-device`, `--panel-map`, which the app
also takes on its own command line, to try another map); a map named without a path is found in
`share/oscairn/panels/` beside the installed program. A build without `OSCAIRN_PANEL_ROLAND`
(macOS, the desktop) leaves the reader out and says so when a profile has a panel.

### The map

[hosts/standalone/panels/fantom-07.json](../hosts/standalone/panels/fantom-07.json)
([schema](panel-map.schema.json)). `layout` says where each number sits: a field is a list of
`bytes` (0 to 3) joined most significant first, the low `bits` of each, then `shift`, `mask` and
`signed`; `kinds` gives the kind field's value for each of the four kinds. `controls` lists each
control's `kind` (`encoder`, `analog`, or `switch` for both switch kinds), `id` and `control`:
an identity above, or a fixed control's function from the profile (`enter`, `play`). An analog
value is divided by `max` (or the layout's `analogMax`) for 0 to 1; an encoder's delta is
multiplied by `step` (or `encoderStep`).

The shipped map is **entirely provisional**, marked `"estimated": ["layout", "controls"]`. Its
layout is a guess (the kind in the low four bits of byte 0, the ID in byte 1, a 14-bit value in
bytes 2 and 3 seven bits each, a signed 7-bit step in byte 2, analog values to 1023 after the
spec's "1,024 step" lever and wheels); none of it has been seen on a unit. Its `controls`
list every control the panel may send **without an `id`**, so none is mapped: every message
shows as unknown until the IDs are filled in from captures. Nothing reaches a control by a
guessed ID.

### What it does not do yet

- **Keys and pads:** not read; where they arrive is unknown.
- **Fixed controls** (the VALUE dial, cursor, ENTER, EXIT, transport, MENU and the rest) decode
  and show in the dump, but the engine has nothing to give them to: the controller layer plays
  assignable controls only.
- **The lever and pedals with nothing assigned:** a MIDI controller's wheel then reaches the
  parts as its own MIDI message; a panel message has none, so `wheel:1` and `wheel:2` play only
  what a Performance or Sound assigns them.

## Mapping the panel on a unit

On the FANTOM, with Oscairn's engine not reading the panel (Roland's app stopped too, so only
one program reads the device):

```sh
oscairn-panel-dump /dev/i2c-slave-2 /usr/share/oscairn/panels/fantom-07.json
```

Each message prints on a line: its four bytes, the kind, ID and value the map's layout reads,
and the control it maps to, or `-`. Ctrl-C (or `--seconds N`) ends it with a table of every kind
and ID seen, how often, and its lowest and highest value. Then:

1. **The layout first.** Press and release one button slowly, turn one encoder each way, move
   one slider end to end, and look at the raw bytes: which byte changes with the kind, which
   stays the control's own, which carries the value. Copy the map, change `layout` until the
   kinds and IDs read sensibly, and run the dump again with the copy.
2. **Then each control,** one at a time, in the order of the map's `controls`: move it, read its
   kind and ID from the table, and add the `id` to its entry. Note each analog control's highest
   value as its `max` if it is not the layout's `analogMax`, and give an encoder that turns the
   wrong way a negative `step`.
3. **Check:** with the filled map the dump shows a control on every line and `-` for nothing
   you moved. Run the app with `--panel-map` pointing at the copy and move the sliders: the
   part levels follow.

Send back the filled map and the dump's tables, and remove `"controls"` from `estimated`
(`"layout"` too once it is confirmed).

## Building it into an image

`-DOSCAIRN_PANEL_ROLAND=ON` links the reader into the engine (and the app's panel handling) and
installs `oscairn-panel-dump` and the map in `share/oscairn/panels/`. No Buildroot config
sets it yet: a FANTOM image would pass it from its defconfig through the oscairn package's
CMake options, as its profile has the panel.
