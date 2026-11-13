# Running Oscairn on a Roland FANTOM-0

Oscairn has never run on a FANTOM-06, -07 or -08. A FANTOM-07 is now here, running Roland's
stock Linux with root login over SSH; Oscairn has not been tried on it yet. This page says what
is known about the hardware, what Oscairn has for it, what of that is estimated, how to run
Oscairn beside Roland's system in a chroot, and how to map the panel, as of November 12, 2026.
The repository holds Oscairn's own code and data only: no Roland files, binaries or firmware,
and nothing that changes Roland's firmware or update packages.

## What is known

From the owner's notes on the hardware (Roland's GPL release of the kernel, U-Boot and device
tree, and Roland's test tool), and Roland's published specifications and owner's manual:

- **SoC:** NXP i.MX8M Mini, four Cortex-A53 cores: the KICK's SoC. The notes said 2 GB of RAM;
  the unit's Linux reports 952 MB (`free -m`), with 255 MB of swap. The FANTOM-06, -07
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
  OUT, headphones, and the audio device below, whose slots are a guess), `controls` (Roland's counts, but whether each reaches the i.MX,
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
- **The audio device** (the profile's `audio.device`, [RFC 0024](rfcs/0024-device-profiles.md#what-a-profile-holds)):
  `{ "name": "hw:1,0", "channels": 16, "rate": 48000, "outputs": { "main": [0, 1] } }`. The
  engine opens `hw:1,0` with all 16 slots at 48 kHz (refusing to start at another rate), plays
  main left and right on slots 0 and 1 and keeps the other 14 silent. JUCE, which opens it, takes
  S32_LE when the device offers no float format. **Slots 0 and 1 are a guess**, and so are 16
  channels where Roland's app opens 6 (if `hw:1,0` refuses 16, try `"channels": 6`); no inputs and
  no SUB OUT are mapped until their slots are known. To find MAIN OUT on a unit, copy the profile, change `main`
  (for example to `[2, 3]`), start the app with `--profile` and the copy's path (the built-in
  one is compiled in), and listen at a low level; or run `oscairn-engine` with
  `--output-device hw:1,0 --device-channels 16 --sample-rate 48000 --audio-outputs 2,3`; the SUB OUT pair, once found,
  goes in as `"sub": [n, n + 1]`.
- **The screen:** the profile gives its diagonal (140 mm, 5.5"), so the UI scales by its density
  ([RFC 0019](rfcs/0019-adaptive-ui.md#screens-of-any-size)): 10.5 pixels per millimeter make a
  scale of 1.55, and `kick-screen` is laid out at the screen's own 826 × 465 design units and
  fills it, with no bars. A touch target is 68 pixels, 6.5 mm, as on the KICK. `desktop-gaps`
  renders Home, Play, the Parts views, an editor, the Library, the Catalog and Settings at
  1280 × 720 and checks every control fits, is that large and overlaps none. Untried on the
  instrument.
- **A panel reader** in the engine, built only with `OSCAIRN_PANEL_ROLAND` (below).
- **A chroot bundle** (below): Oscairn with a userland of its own, to run on Roland's kernel
  and beside Roland's root filesystem without changing either. There is no image of Oscairn's
  own for the FANTOM: the i.MX8M Mini EVK image (`oscairn_imx8mm_evk_defconfig`,
  [kick.md](kick.md)) is the nearest; the FANTOM's own device tree, display, touch, audio and
  boot path are not there.

## Running in a chroot on Roland's Linux

What the unit's stock system is, read on it: Linux 4.19.35 (`PREEMPT`, aarch64), an NXP Yocto
userland (warrior) with glibc 2.29, too old for Oscairn's programs; the root filesystem on eMMC,
writable, with 25 GB free; ALSA card 1, `hw:1,0`, for SAI1; `/dev/i2c-slave-2`, `/dev/fb0`,
`/dev/dri/card0`, and the touch screen among `/dev/input/event*`. Roland's app (`mi078`) runs;
no X server or Weston does.

So Oscairn brings its own userland and leaves Roland's kernel, boot chain and root filesystem as
they are: no flashing and no boot change. The bundle is a root filesystem tree, with no kernel,
bootloader or device tree, unpacked in a directory on the unit (for example
`/home/root/oscairn`) and entered with `chroot`. It holds glibc, libstdc++, ALSA's library,
BusyBox, the X server the image's session uses, and Oscairn: `oscairn-engine`, the app
(`Oscairn`), the CLAP plugins in `/usr/lib/clap`, `oscairn-panel-dump` and the panel map in
`/usr/share/oscairn/panels`, with the `fantom-07` profile built in.

Build it (Docker; the first build takes an hour or more):

```sh
os/fantom-bundle.sh       # build-os/oscairn-fantom-bundle.tar.gz
```

It builds `oscairn_fantom_bundle_defconfig`: the EVK image's userland for the Cortex-A53
(ARMv8.0), with `BR2_PACKAGE_OSCAIRN_PANEL_ROLAND`, and the toolchain's kernel headers from
Linux 4.19.35, the unit's kernel, since glibc refuses to start on a kernel older than its
headers ("FATAL: kernel too old"). `readelf -n` on the bundle's `libc.so.6` shows the oldest
kernel it accepts (`OS: Linux, ABI: 4.19.0`).

On the unit, as root, in the directory it was unpacked in:

```sh
./start-chroot.sh oscairn-engine --help
./start-chroot.sh             # a shell in the bundle
./stop-chroot.sh
```

`start-chroot.sh` mounts the system's `/proc`, `/sys`, `/dev`, `/dev/pts` and `/dev/shm` under
the bundle, and a tmpfs of its own on its `/run` and `/tmp`, then runs the command (a shell
without one) in the bundle with an environment of its own. Running it again keeps the mounts
already there. `stop-chroot.sh` unmounts them, once nothing in the bundle runs. Nothing outside
the bundle's directory changes but those mounts. Programs in the bundle use Roland's devices
directly: stop Roland's app before anything opens `hw:1,0` or the panel. Start the engine by
its full path, `/usr/bin/oscairn-engine`: by its bare name it does not find its plugin worker
beside it, and runs plugins in the engine.

Checked in Docker on a Mac (an ARM64 container, so on a far newer kernel than 4.19): the bundle
unpacks, `start-chroot.sh` enters it, the engine starts with `--audio null` and answers
`session.status` on its control socket, and `oscairn-panel-dump` decodes messages fed to it from
a FIFO. That shows the userland is complete, not that it runs on the unit's kernel: nothing has
run on the unit yet. Idle there with `--audio null`, the engine held 125 MiB resident (its
memory locked, at the profile's `instrument` reliability) and its three plugin workers 52 MiB
together. On 4.19 the engine can have no memory pressure trigger (PSI is Linux 4.20 and later):
by its code it then logs that it has none and goes on without. Showing the app on the FANTOM's
screen is not solved.

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
installs `oscairn-panel-dump` and the map in `share/oscairn/panels/`. Buildroot passes it
when a defconfig sets `BR2_PACKAGE_OSCAIRN_PANEL_ROLAND`, as the chroot bundle's does.
