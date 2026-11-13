# Running Oscairn on a Roland FANTOM-0

Oscairn has never run on a FANTOM-06, -07 or -08. A FANTOM-07 is now here, running Roland's
stock Linux with root login over SSH; Oscairn has not been tried on it yet. This page says what
is known about the hardware, what Oscairn has for it, what of that is estimated, how to run
Oscairn beside Roland's system in a chroot, and how to map the panel, as of November 12, 2026.
The repository holds Oscairn's own code and data only: no Roland files, binaries or firmware,
and nothing that changes Roland's firmware or update packages.

## What is known

From the owner's notes on the hardware (Roland's GPL release of the kernel, U-Boot and device
tree, and Roland's test tool), Roland's published specifications and owner's manual, and, on
November 12, 2026, a read-only survey of a running FANTOM-07 over SSH with Roland's app left
undisturbed and nobody at the panel: `/proc`, `/sys`, the live device tree, the kernel's own
configuration (`/proc/config.gz`), the boot log, the start-up scripts and the app's open
files, threads and memory maps. What that survey showed is in
[What the unit shows](#what-the-unit-shows) below; what it could not settle is in
[What needs the owner at the instrument](#what-needs-the-owner-at-the-instrument).

- **SoC:** NXP i.MX8M Mini, four Cortex-A53 cores at 1.2 to 1.8 GHz: the KICK's SoC. The notes
  said 2 GB of RAM; the kernel sees **992 MiB** (1,015,808 KiB at boot, 320 MiB of it a
  CMA pool the GPU and display draw from), and `free -m` reports 952 MB, with a 256 MB swap
  file. The FANTOM-06, -07 and -08 share one main board and firmware; the -07 has 76 keys,
  velocity only, no aftertouch, and weighs 7.0 kg.
- **Screen:** 5.5", MIPI-DSI, a 720 × 1280 portrait panel (`tianma,tm055jvzg18` in the device
  tree, 68 × 121 mm) that Weston turns 90° to 1280 × 720. The touch controller is a Goodix
  (the device tree says GT1151; the chip reports ID 1158) at 0x14 on I2C2.
- **Audio:** the i.MX's SAI1 runs a TDM link of 16 slots of 32 bits at 48 kHz, left-justified,
  to Roland's BMC chips, which are the bit clock and frame master: the i.MX makes no audio
  clock of its own (SAI1's master clock is off). ALSA calls it `hw:1,0`; Roland's app opens 6
  channels each way, S32_LE. **Which slots carry what is unknown**, and so is whether audio
  played into `hw:1,0` reaches MAIN OUT without Roland's app setting the BMCs up first: that is
  the deciding test on a unit.
- **Panel:** Roland's driver (`i2c-slave-coms`, built into the kernel) makes the i.MX an I2C
  slave at 0x08 on I2C3 and gives `/dev/i2c-slave-2`. Messages are 4 bytes, with **bit 7 set on
  the first byte**. Roland's test tool names four kinds: `PMDWEncoder`, `PMDWADChange`,
  `PMDWSwitchOn` and `PMDWSwitchOff` (encoders, analog controls, switches pressed and released),
  and analog sources `HOLD`, `WHEEL1`, `WHEEL2`, `BENDER` and `SLIDER1` to `SLIDER8`. **Where the
  kind, the ID, the value and the step sit in the bytes is unknown.** Three buttons are not on
  the panel bus at all: WRITE, MENU and TEMPO are GPIO keys (below).
- **Keys:** a separate keyboard CPU scans them. **Where they reach the i.MX is unknown**, but
  the survey narrows it: nothing on the i.MX reads a UART, the panel bus was quiet, and the one
  busy link to the BMCs is a USB serial line (below); the keys most likely reach the BMC first.
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

## What the unit shows

Read on the running unit, November 12, 2026, with Roland's app running and the panel
untouched. The raw outputs stay off the repository. "Inferred" marks a conclusion drawn from
what was read rather than read itself.

### How it starts

systemd starts `weston.service` (Weston 6.0.1, DRM backend, the G2D renderer, `--xwayland`),
then `weston-term.service`, which runs `/home/root/go.sh` as root. `go.sh`:

1. turns swap on (`/swapfile`);
2. reads GPIO 138 (GPIO5_10) once: high starts Roland's factory test tool (`qctest`) in an
   `xterm` instead of the app;
3. stops `systemd-timesyncd` and sets the clock from a saved date (the unit's clock starts at
   January 1, 2022);
4. writes one register through `/unit_tests/memtool`: `0x30360034`, which by the i.MX8M Mini's
   reference manual is the video PLL's spread-spectrum control (inferred: it is for the display,
   not audio; the audio link takes its clocks from the BMCs);
5. loads `libcomposite`, `g_serial` and `tickgen`;
6. runs `/home/root/mi078` with `DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-0` and
   `XDG_RUNTIME_DIR=/run/user/0`.

`weston-term.service` has no `Restart=`, so `systemctl stop weston-term` stops Roland's app and
it stays stopped until `systemctl start weston-term` or a reboot. `g_serial` and `tickgen` stay
loaded after it stops. Nothing in `go.sh` sets up the audio link, the panel bus or the BMCs:
whatever the BMCs need from the i.MX, Roland's app sends it itself, over the USB serial line.

### Roland's app

`mi078`, a 61 MB JUCE program, runs as root with 35 threads. What it holds open:

| Device | What it is | How |
|---|---|---|
| `/dev/ttyGS0` (twice) | the USB serial gadget: the BMC control line | read and write |
| `/dev/i2c-slave-2` | the panel | read and write; one thread blocks in the driver's ioctl |
| `/dev/input/event1` | `gpio-keys`: WRITE, MENU, TEMPO | read only |
| `/dev/snd/pcmC1D0p`, `pcmC1D0c` | `hw:1,0`, playback and capture | memory-mapped |
| `/dev/snd/timer` (three) | ALSA timers | |
| `/proc/tickgen` | Roland's `tickgen` module | memory-mapped |
| `/dev/mem` | one page at `0x30210000`: GPIO2's registers | memory-mapped |

It is an X11 client of Xwayland (`libX11`, MIT-SHM): it holds no GPU or DRM device, so it draws
in software and Weston composites it. It sets the backlight through
`/sys/class/backlight/backlight/brightness`. It imports ALSA's sequencer functions (JUCE's MIDI),
which find no sequencer on this kernel.

Its real-time threads, all `SCHED_RR` but two, by priority: two unnamed at `SCHED_FIFO` 49 and
48 (one of them polls, inferred the USB serial line); "Tick Gen Task", "Cyclic
Thread" and three "JUCE ALSA" threads at 45; "Play Task", "RhyPtn Task", eight
"SmplPlayerTask" and one "SmplTask" at 40; a "SmplTask", "UserSmplTask" and "JUCE Timer" at 35;
"Kbd Task" and a "SmplTask" at 30; "Thread Sw Ad" and "JUCE OSC server" at 25; "Prm Task" at
20; "Seq Task" at 15. None is pinned to a core. RT throttling is the kernel default (950 ms of
every second). At the time of the survey most of the app was in swap (11 MB resident, 104 MB
swapped), with 100 KB locked.

### Audio

- **Two ALSA cards.** Card 0, `imx-audio-micfil`: the i.MX's PDM microphone interface, capture
  only, unused (closed, and no interrupt since boot); inferred, a leftover of NXP's evaluation
  board's device tree, as are its HDMI bridge and Wi-Fi entries. Card 1, `nocodec`: SAI1 through `simple-audio-card` with a dummy
  codec (`codec_bmc`), playback and capture on `hw:1,0`.
- **The link** (device tree `sound-bmc`): `left_j`, `dai-tdm-slot-num` 16,
  `dai-tdm-slot-width` 32, bit clock and frame master the codec side (the BMCs), RX synchronous
  to TX. SAI1's `fsl,dataline` enables eight data lines each way, but its pin group muxes only
  four pads, inferred bit clock, frame sync and one data line each way. Its root clock is off
  (`clk_summary`): the i.MX is a clock slave.
- **What Roland's app opens**, from `/proc/asound/card1/pcm0*/sub0/`:

  | | Playback | Capture |
  |---|---|---|
  | Access, format | MMAP_INTERLEAVED, S32_LE | MMAP_INTERLEAVED, S32_LE |
  | Channels, rate | 6, 48000 | 6, 48000 |
  | Period, buffer | 300, 1800 frames | 1200, 2400 frames |
  | `avail_min`, `start_threshold` | 300, 1 | 1200, 1 |
  | `stop_threshold` | the boundary (never stops) | the boundary |
  | `silence_size` | the boundary | 0 |

  Both run from boot without a stop; the DMA interrupt rate (200 a second) matches those
  periods. The application pointer of both stays at 0: the app never commits frames the
  ordinary way. Inferred: it writes and reads the memory-mapped ring directly at the hardware
  pointer, ALSA zeroing what has played. The stream's owner is a "SmplTask" thread, and the app
  holds its sample files open, so inferred: what the i.MX sends is **sample playback** (pads,
  the sampler) for the BMCs to mix, and what it receives is audio to **sample**; the synth
  voices and the main mix are the BMCs'. Nothing shows which of the six slots is which.
- **Slots:** NXP's SAI driver enables the first *n* slots for *n* channels, so Roland's six are
  inferred to be slots 0 to 5. Roland's `thru.sh` opens all 16 both ways
  (`arecord -D hw:1,0 -c 16 -f S32_LE --rate=48000 | aplay -D hw:1,0`), so 16 channels are
  accepted.
- **Mixer controls:** card 1 has only the SAI's timestamp and bit-counter controls; card 0 the
  micfil's gains and voice-detector settings. No volume or routing control exists on the i.MX
  side.

### USB

The i.MX has two USB 2.0 controllers (ChipIdea):

- **USB1 (`32e40000`), host:** EHCI, VBUS switched by GPIO1_12, bus 1. Roland's test tool
  drives this controller's port test mode as "USB-HOST (SOM)", beside a separate
  "USB-HOST (BMC)": this is **USB MEMORY**. The owner's USB Ethernet adapter was on it during
  the survey, and nothing else.
- **USB2 (`32e50000`), device only:** `g_serial`, one CDC ACM port, `/dev/ttyGS0`. It was
  configured by a host at high speed the moment `go.sh` loaded `g_serial` (the boot log's next
  line), and carries a steady 58 interrupts a second: inferred it is wired inside the
  instrument to a BMC, the host, and is the BMC control line, not the USB COMPUTER port.
- **USB COMPUTER and EXT DEVICE** are therefore not on the i.MX (inferred): the test tool's
  "USB-HOST (BMC)" and "BMC1 USB", Roland's update reading EXT DEVICE from the BMC side, and
  the owner's notes that custom BMC code answered over USB MIDI to a computer all put them on
  the BMCs. USB MIDI and audio to a computer are the BMC's, not Linux's.
- **Gadget functions in this kernel:** configfs is mounted at `/sys/kernel/config` with no
  gadget defined; MIDI (`f_midi`), UAC1, UAC2, HID, ACM, ECM, NCM, RNDIS, mass storage and
  FunctionFS are built in, and `g_audio` is a module. They only matter on a port a computer
  can reach, and the i.MX's device port is (inferred) taken by the BMC.

### MIDI

- **DIN MIDI IN and OUT are not Linux's** (inferred, strongly): the four i.MX UARTs are UART1
  and UART3 (with RTS/CTS), UART2 (the console, `ttymxc1`, 115,200) and UART4; no process holds
  UART1, 3 or 4, and none has sent or received a byte since boot. Roland's test tool's MIDI
  loopback test opens a raw MIDI device `hw:2,0,0`, a card that does not exist in normal use.
- **ALSA:** raw MIDI is built in (`CONFIG_SND_RAWMIDI=y`); the **sequencer is not**
  (`# CONFIG_SND_SEQUENCER is not set`, hence no `/dev/snd/seq`), nor `snd-aloop`, nor
  `snd-dummy`. `snd-usb-audio` and `snd-usbmidi-lib` are on the unit as modules, not loaded.
  So a USB MIDI keyboard on USB MEMORY would appear, after `modprobe snd-usb-audio`, as a raw
  MIDI device (`/dev/snd/midiC2D0`), **and Oscairn would not see it**: its MIDI input is JUCE's,
  which on Linux reads only the ALSA sequencer. Playing a USB MIDI keyboard needs Oscairn to
  read raw MIDI, or a kernel with the sequencer.

### Panel, buttons and lights

- **`/dev/i2c-slave-2`:** character major 235 (`I2cSlaveDev2`), class `i2c-slave-dev2`; the
  device-tree node `i2c@30a40000/i2c_rx@08`, compatible `slave-rxtx`, on I2C3 at 100 kHz; the
  kernel device `2-1008` (address 0x08 with the slave flag). Roland's app opens it read and
  write, so the i.MX may answer the panel too (inferred: lamp states). I2C3 had raised 142
  interrupts since boot with nobody touching the panel.
- **GPIO keys** (`gpio-keys`, `/dev/input/event1`): WRITE on GPIO5_11 (key code 157), MENU on
  GPIO5_12 (139) and TEMPO on GPIO5_13 (152). Roland's app and Weston both read it.
- **Other input devices:** `event0` and `event2` are the SoC's and the PMIC's power keys;
  `event3` the touch screen.
- **Lights:** one GPIO LED, `status` (GPIO3_16), on. The panel's lamps are not Linux LEDs.
  The backlight is `pwm-backlight` on PWM3 (1 kHz), brightness 0 to 255, at 255.
- **No ADC** (no IIO device), no other PWM, and no exported GPIO. GPIO5_10 picks the factory
  test at boot. Roland's app maps GPIO2's registers; what it drives there is unknown (GPIO2_6
  is held high by the kernel, unlabeled).
- **The wheels, lever, sliders and pedals** are panel-bus analog sources by the test tool's
  names (above). Roland's app reaches none of them another way.

### Display and touch

- **DRM:** `imx-drm`, LCDIF to the Samsung MIPI-DSI bridge, connector `DSI-1`, one mode
  720 × 1280 at 60 Hz (76 MHz). `/dev/fb0` is DRM's emulated frame buffer, 720 × 1280, 32-bit.
  Weston owns `/dev/dri/card0` and composites with the Vivante GPU (`galcore` 6.4.0, `libGAL`,
  G2D).
- **Weston's configuration** (`/etc/xdg/weston/weston.ini`): output `DSI-1`, `mode=720x1280@60`,
  `transform=90`, idle timeout off, no panel, a black background; touch calibration on. Xwayland
  serves `:0`, on `/tmp/.X11-unix/X0` and also on the abstract socket of that name.
- **Touch:** the Goodix driver (module `goodix`), `/dev/input/event3`, multitouch (slots,
  tracking IDs, position, touch and width major). Its coordinate range is not readable without
  opening the device.
- **Backlight:** `/sys/class/backlight/backlight`, which Roland's app writes.

### Kernel

Linux 4.19.35, `CONFIG_PREEMPT` (not PREEMPT_RT), 250 Hz, high-resolution timers; cgroups v1
with cpusets, memory, cpu and hugetlb controllers mounted (no RT group scheduling, no CFS
bandwidth); CPU isolation available but no `isolcpus` on the command line; huge pages (2 MiB,
none reserved; transparent ones on `madvise`); **no PSI** (4.19 predates it); **no uinput**;
no hidraw. `cpufreq` governor `ondemand`. Modules can be loaded (177 on the unit).

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
  S32_LE when the device offers no float format. **Slots 0 and 1 are a guess**. Sixteen channels
  where Roland's app opens 6 are accepted (Roland's `thru.sh` opens 16), but whether the BMCs
  listen beyond the six slots Roland's app fills is unknown (if nothing sounds, try
  `"channels": 6`); no inputs and
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
`/dev/dri/card0`, and the touch screen as `/dev/input/event3`. Roland's app (`mi078`) runs as an
X11 client of Xwayland under Weston ([How it starts](#how-it-starts)).

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
screen is not solved. Weston and Xwayland keep running when Roland's app stops, so the likeliest
way is for the app to be one more X11 client on `:0`, as Roland's is: the bundle's own `/tmp`
hides Xwayland's socket file, but Xwayland also listens on the abstract socket of the same
name, which a chroot does not hide (untried).

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

- **Keys and pads:** not read; where they arrive is unknown (most likely the BMC control line,
  [Panel, buttons and lights](#panel-buttons-and-lights)).
- **WRITE, MENU and TEMPO:** GPIO keys on `/dev/input/event1`, not panel messages; nothing reads
  them yet.
- **Fixed controls** (the VALUE dial, cursor, ENTER, EXIT, transport, MENU and the rest) decode
  and show in the dump, but the engine has nothing to give them to: the controller layer plays
  assignable controls only.
- **The lever and pedals with nothing assigned:** a MIDI controller's wheel then reaches the
  parts as its own MIDI message; a panel message has none, so `wheel:1` and `wheel:2` play only
  what a Performance or Sound assigns them.

## Mapping the panel on a unit

On the FANTOM, with Oscairn's engine not reading the panel (Roland's app stopped too, with
`systemctl stop weston-term`, so only one program reads the device):

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

## What needs the owner at the instrument

Each of these needs someone at the panel, Roland's app stopped, or both; in this order, cheapest
first. The first four leave Roland's app running and only read `/proc`: run each watch over SSH
while someone at the instrument does the one thing named, and compare the counts before and
after.

```sh
watch -n1 'grep -E "30a40000|32e50000|32e40000|serial|Write|Menu|Tempo|gt1151" /proc/interrupts'
```

1. **Where the keys enter.** Play a run of notes, then hold a chord, while watching. The panel
   bus is the `30a40000.i2c` line; the BMC control line `32e50000.usb` (58 a second at rest).
   If notes raise neither rate, the BMC plays them without a message to the i.MX per note. Then
   do the same with
   the pads, the lever, WHEEL1 and WHEEL2, a slider, and a pedal on HOLD and on CTRL1.
2. **The GPIO keys.** Press WRITE, MENU and TEMPO: each should count on its own line (`Write`,
   `Menu`, `Tempo`). Any other button that counts there is a GPIO key too.
3. **DIN MIDI.** Send notes into MIDI IN from another instrument while watching
   `cat /proc/tty/driver/IMX-uart` (each UART's `rx:` count) and the `serial` lines. If no count
   moves, DIN MIDI is the BMC's, as inferred.
4. **USB COMPUTER.** Plug a computer into USB COMPUTER, watch `dmesg -w` and the `32e50000`
   and `32e40000` lines on the unit, and list USB devices on the computer. Whatever the computer
   enumerates (its vendor, product and interfaces) is the BMC's if the unit's Linux saw nothing.
5. **Stop Roland's app** (the rest needs it stopped): `systemctl stop weston-term`, then check
   that `/proc/asound/card1/pcm0p/sub0/status` and `pcm0c/sub0/status` say `closed` and that
   no process holds `/dev/i2c-slave-2` (`ls -l /proc/*/fd 2>/dev/null | grep i2c-slave`).
   `systemctl start weston-term` (or a reboot) brings it back.
6. **The panel's layout and IDs:** [Mapping the panel on a unit](#mapping-the-panel-on-a-unit),
   with `oscairn-panel-dump` in the bundle.
7. **Whether the link still runs:** `arecord -D hw:1,0 -c 16 -f S32_LE -r 48000 -d 2
   /tmp/link.wav`. It finishing in two seconds means the BMCs still drive the bit clock without
   Roland's app; hanging until killed, or an input/output error, means they stopped it. Look at
   the file for which of the 16 slots carry signal while someone plays a key (the BMCs' audio
   to the i.MX, which Roland samples). Remove the file afterward.
8. **The deciding audio test,** at low volume, headphones first: play into `hw:1,0` one pair of
   slots at a time, `oscairn-engine --output-device hw:1,0 --device-channels 16
   --sample-rate 48000 --audio-outputs 0,1`, then `2,3` and so on to `14,15`, and note which
   pair reaches MAIN OUT, SUB OUT or the headphones, if any. If none does, repeat with
   `--device-channels 6` and the pairs 0 to 5, then with Roland's app started and stopped once
   more (in case its BMC set-up outlives it). If nothing ever sounds, the BMCs mix what the i.MX
   sends only when Roland's app tells them to, over `/dev/ttyGS0`.
9. **The touch screen's range:** `evtest /dev/input/event3` prints each axis's minimum and
   maximum before the first event (Ctrl-C after it); touch the four corners to see which way the
   axes run on the landscape screen. Neither the unit nor the bundle has `evtest`: bring a
   static aarch64 build.
10. **A USB MIDI keyboard on USB MEMORY:** plug it in, `modprobe snd-usb-audio`, and check
    `cat /proc/asound/cards` and `amidi -l` (on the unit). A raw MIDI device there confirms the
    kernel side; Oscairn will not see it until it reads raw MIDI (above).
11. **The display from the chroot:** with Weston running and Roland's app stopped, start the app
    in the bundle with `DISPLAY=:0` and see whether it reaches Xwayland.

## Building it into an image

`-DOSCAIRN_PANEL_ROLAND=ON` links the reader into the engine (and the app's panel handling) and
installs `oscairn-panel-dump` and the map in `share/oscairn/panels/`. Buildroot passes it
when a defconfig sets `BR2_PACKAGE_OSCAIRN_PANEL_ROLAND`, as the chroot bundle's does.
