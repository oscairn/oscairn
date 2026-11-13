# Running Oscairn on a Roland FANTOM-0

A FANTOM-07 is here, running Roland's stock Linux with root login over SSH, and Oscairn first
ran on it on November 12, 2026 ([The first run on the unit](#the-first-run-on-the-unit)). This
page says what is known about the hardware, what Oscairn has for it, what of that is estimated,
how to run Oscairn beside Roland's system in a chroot, and how to check the panel map, as of
November 12, 2026.
The repository holds Oscairn's own code and data only: no Roland files, binaries or firmware,
and nothing that changes Roland's firmware or update packages.

## What is known

From the owner's notes on the hardware (Roland's GPL release of the kernel, U-Boot and device
tree, and Roland's test tool), Roland's published specifications and owner's manual, and, on
November 12, 2026, a read-only survey of a running FANTOM-07 over SSH with Roland's app left
undisturbed and nobody at the panel: `/proc`, `/sys`, the live device tree, the kernel's own
configuration (`/proc/config.gz`), the boot log, the start-up scripts and the app's open
files, threads and memory maps. What that survey showed is in
[What the unit shows](#what-the-unit-shows) below. The same day Roland's app and factory test
tool were read as files, without touching the unit:
[What Roland's programs show](#what-rolands-programs-show) has what they say about the keys,
the audio link, the panel and the link to the BMCs. Later that day the owner stopped Roland's
app and read the devices by hand: [With Roland's app stopped](#with-rolands-app-stopped). What
none of it settles is in
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
  channels each way, S32_LE. Read from the app: it plays its sampler on channels 2 to 5,
  leaves 0 and 1 silent, and samples from its first two input channels
  ([The audio link's channels](#the-audio-links-channels)). **Audio played into `hw:1,0`
  is heard without Roland's app**, on every pair tried, and Oscairn's engine has played
  through it ([With Roland's app stopped](#with-rolands-app-stopped)).
- **Panel:** Roland's driver (`i2c-slave-coms`, built into the kernel) makes the i.MX an I2C
  slave at 0x08 on I2C3 and gives `/dev/i2c-slave-2`. Messages are 4 bytes, with **bit 7 set on
  the first byte**. Roland's test tool names four kinds: `PMDWEncoder`, `PMDWADChange`,
  `PMDWSwitchOn` and `PMDWSwitchOff` (encoders, analog controls, switches pressed and released),
  and analog sources `HOLD`, `WHEEL1`, `WHEEL2`, `BENDER` and `SLIDER1` to `SLIDER8`. Where the
  kind, the ID, the value and the step sit in the bytes, and each control's ID, are now read
  from Roland's test tool and app ([The map](#the-map)); every control the owner touched on the
  unit decoded to the map's names. The panel reports only while a program holds the device
  ([With Roland's app stopped](#with-rolands-app-stopped)). Three buttons are not on the panel bus at all: WRITE, MENU and TEMPO are GPIO keys
  (below).
- **Keys:** a separate keyboard CPU scans them. Seen on the unit with Roland's app stopped and
  nothing sent to the BMC side: key events arrive on the USB serial line `/dev/ttyGS0`, as MIDI
  note messages with velocity, to whatever program reads it
  ([With Roland's app stopped](#with-rolands-app-stopped)). Oscairn reads them
  ([The key reader](#the-key-reader)).
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
  voices and the main mix are the BMCs'. The app's code agrees, and shows which of the six
  channels it uses ([The audio link's channels](#the-audio-links-channels)).
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
  UART1, 3 or 4, and none has sent or received a byte since boot. Roland's test tool names a
  raw MIDI device `hw:2,0,0`, a card that does not exist in normal use, in code it never calls
  ([MIDI ports](#midi-ports)).
- **ALSA:** raw MIDI is built in (`CONFIG_SND_RAWMIDI=y`); the **sequencer is not**
  (`# CONFIG_SND_SEQUENCER is not set`, hence no `/dev/snd/seq`), nor `snd-aloop`, nor
  `snd-dummy`. `snd-usb-audio` and `snd-usbmidi-lib` are on the unit as modules, not loaded.
  So a USB MIDI keyboard on USB MEMORY would appear, after `modprobe snd-usb-audio`, as a raw
  MIDI device (`/dev/snd/midiC2D0`). JUCE's MIDI input reads only the ALSA sequencer, so where
  the sequencer does not open the engine reads raw MIDI itself (`hosts/audio/RawMidi.h`),
  decided once at startup: every input subdevice of every card, named as the sequencer would
  name its port (a USB keyboard's "SMK-37 MIDI 1"), so the same controller choices apply as on
  a desktop. It lists the cards again once a second for a keyboard plugged in or pulled out.
  None of this has run on the unit or on any real ALSA card yet: only its reader, on pipes and
  a pty in a container.

### Panel, buttons and lights

- **`/dev/i2c-slave-2`:** character major 235 (`I2cSlaveDev2`), class `i2c-slave-dev2`; the
  device-tree node `i2c@30a40000/i2c_rx@08`, compatible `slave-rxtx`, on I2C3 at 100 kHz; the
  kernel device `2-1008` (address 0x08 with the slave flag). Roland's app opens it read and
  write, but neither it nor the test tool ever writes to it: the lamps go another way
  ([Lamps](#lamps)). I2C3 had raised 142 interrupts since boot with nobody touching the panel.
- **Panel words travel the panel bus** (measured later that day, Roland's app running, one
  slider moved for 5 seconds and left for 5, in turn): I2C3 took about 200 interrupts a second
  while it moved and none at rest. Meanwhile the app's writes rose from about 1.6 to about
  4 kB a second and the message link's interrupts from 63 to about 180 a second: the app
  passing each word on to the sound engine, as its code does
  ([The lever, wheels, pedals, sliders, knobs and pads](#the-lever-wheels-pedals-sliders-knobs-and-pads)).
  The bus also takes a burst at start-up: 3,277 interrupts in the first two minutes after boot.
- **GPIO keys** (`gpio-keys`, `/dev/input/event1`): WRITE on GPIO5_11 (key code 157), MENU on
  GPIO5_12 (139) and TEMPO on GPIO5_13 (152). Roland's app and Weston both read it. Pressed on
  the unit, they sent nothing on the panel bus: this device is the only place they come from.
  Oscairn's engine reads it too ([WRITE, MENU and TEMPO](#write-menu-and-tempo)).
- **Other input devices:** `event0` and `event2` are the SoC's and the PMIC's power keys;
  `event3` the touch screen.
- **Lights:** one GPIO LED, `status` (GPIO3_16), on. The panel's lamps are not Linux LEDs.
  The backlight is `pwm-backlight` on PWM3 (1 kHz), brightness 0 to 255, at 255.
- **No ADC** (no IIO device), no other PWM, and no exported GPIO. GPIO5_10 picks the factory
  test at boot. Roland's app maps GPIO2's registers to make GPIO2_12 an output and set it
  high once it has started ([What the app does that the hardware depends on](#what-the-app-does-that-the-hardware-depends-on));
  GPIO2_6 is held high by the kernel (the device tree's `modem-reset`, a name left from NXP's
  board).
- **The wheels, lever, sliders and pedals** are panel analog sources by the test tool's
  names (above). Roland's app opens no other device for them; a slider's words were measured on
  the panel bus (above), and the rest are expected there too
  ([The lever, wheels, pedals, sliders, knobs and pads](#the-lever-wheels-pedals-sliders-knobs-and-pads)).

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

### With Roland's app stopped

Seen on the unit on November 12, 2026, with Roland's app stopped (`systemctl stop
weston-term`), with nothing sent over the message link: the devices read by hand, and audio
played into `hw:1,0` while a program kept reading `/dev/ttyGS0`.

- **The keys arrive on `/dev/ttyGS0` with nothing sent first.** Opened raw with echo off and
  only read, it gives 18 seconds of playing as 26,096 bytes, every byte in a frame of the shape
  [The message link](#the-message-link) gives. Each key event comes twice: a kind-3 frame from
  0x02 to **0x61** (not 0x63 or 0x64, as read from the app) holding a MIDI word (first byte 0x29
  on, 0x28 off: port 2; then 0x91 or 0x81, the note and the velocity; a note off carries a
  release velocity), and a kind-6 frame from 0x02 to 0x60 holding a panel word (0x88 on, 0x87
  off, then the note, 0 and the velocity). In that capture the two always agree, and the panel
  word's third byte is always 0: it adds no resolution. The MIDI word's frame has a stale byte
  past its count (the note again).
- **The MIDI words are Roland's zones' notes, the panel word is the key** (a trace later that
  evening, Roland's app running, while the owner switched Local Switch off and on). With Local
  Switch on, each key event came as three kind-3 MIDI words from 0x02 to 0x61, status 0x90, 0x91
  and 0x92 for the same note and velocity, one per active zone on its channel (the earlier
  capture had one, 0x91, because one zone was active then), and then the one kind-6 frame
  `0c 02 60 00 | 88 nn 00 vv ...` (0x87 for off). With Local Switch off, only the kind-6 frame
  came. So the kind-6 frame is the key itself, one per key event whatever the zones and Local
  Switch are, and the MIDI words echo what Roland's engine plays.
- **The sound engine reports regardless:** long frames of kind 9 from 0xE1 to 0x60, sub-type 9,
  112 bytes each, about ten a second, at rest too. Their contents were not read.
- **What is not read waits.** With a program reading the link, Roland's engine went on playing
  the keys; when the reading stopped the last notes stayed on, and when it started again the
  queued frames came out in a burst (15 kB in 6 seconds). Whoever opens the link has to read it
  without pause, and expect stale key events as it opens.
- **Roland's engine sounds the keys by itself, unless Local Switch is off.** With its app stopped
  the BMC plays its tone from the keys, so Oscairn playing from them sounds together with it.
  With Local Switch set to OFF in Roland's own menu (MENU > SYSTEM > MIDI) and then the app
  stopped, the instrument stayed silent while the owner played: Roland's engine keeps the
  setting after its app stops. It does not survive a power cycle (it was on again after one, and
  Roland's sound with it). On a fresh boot with Local Switch on, after Roland's app was stopped,
  writing the two Local Switch frames ([The message link](#the-message-link)) to `/dev/ttyGS0`,
  raw with echo off, turned it off: in the 12 seconds of playing after, the link carried only the
  keys' own frames, and no zone's MIDI word. The engine now sends them when it starts
  ([The key reader](#the-key-reader)).
- **The panel reports only while a program holds its device** (measured that evening, and
  what Roland's driver does: [The panel's driver](#the-panels-driver)). Taken over within about
  a second of Roland's app letting go of `/dev/i2c-slave-2`, the panel went on sending with
  nothing sent to the instrument at all: 2,736 slider messages in a minute, 863 of them in the
  first 20 seconds, before anything else was done. Left unheld for about a minute, it went silent
  (no interrupt at all on the panel bus) until the next power-on, and the start word Roland's
  app sends when it starts did not bring it back in several tries. A reader that was killed and
  opened again seconds to minutes later also got nothing until a reboot; whether opening again
  at once after a kill works was not tried. So the device is taken over at once and then held
  for as long as Oscairn runs ([The panel holder](#the-panel-holder)); no start word is needed
  after a prompt takeover. Earlier that day the panel had seemed silent to a plain read-only
  `read()`: by the driver, that read gives 0 whenever nothing is queued, and by the measurement
  the panel had by then been left unheld too long.
- **Reading it as Roland's programs do:** opened for reading and writing, `ioctl(fd, 0, 0)`
  waits until the panel has sent a message, then `read(fd, buf, 4)` gives one 4-byte message
  per call until it gives 0. No start words were sent. The first message read, `84 09 00 02`,
  as slider 1 had just been pulled down, decodes through the shipped map as analog ID 9,
  `fader:1`, value 2 ([The map](#the-map)). The wait seemed to give way to no ordinary signal
  (a SIGALRM with a handler did not end it); the driver's source says any signal not blocked
  for the waiting thread ends it, so the signal was most likely blocked there or delivered to
  another thread.
- **What the controls send** (the owner at the panel, read through the map): sliders 1 to 8
  are analog IDs 9 to 16 (`fader:1` to `fader:8`; slider 8 confirmed as 16) and control knobs
  1 to 8 analog IDs 17 to 24, all from 0 to 127; the lever (pitch bend and modulation, analog 7
  and 8) and WHEEL1 and WHEEL2 (5 and 6) run from 0 to 1023; E1 to E6 and the VALUE dial are
  encoders whose messages carry steps of up to ±6; the switches send on and off. Everything the
  owner touched decoded to the map's names. One slider chattered by one step at rest: 8,519
  messages from `fader:4` in 3.5 minutes. The 16 pads came as switches 88 to 103 and the four
  pedal-jack sense switches as 35 to 38, as Roland's tables number them; both are in the map now.
- **A tour of the panel** (later that evening, through `oscairn-panel-dump` attached to the panel
  holder beside the engine, about 1,600 bus interrupts in three minutes): CUTOFF and RESONANCE
  (analog 0 and 1) run from 0 to 1023, like the lever and the wheels; sliders and knobs from 0
  to 127, as before. SAMPLING, PAD MODE, CLIP BOARD, BANK and HOLD, top to bottom, sent switches
  23, 22, 21, 20 and 19: SAMPLING is 23 and PAD MODE 22, the other way round from the map until
  then. The pads, pressed from the bottom left, left to right and row by row upward, sent 101,
  103, 102, 100, then 98, 97, 96, 99, then 92, 94, 95, 93, then 91, 88, 89, 90: Roland's pad 1
  (by its tables, and as its panel is taken to be printed: the owner has not yet checked the
  printed labels) is the top left. S1 and S2 are switches 86 and 87. **WRITE, MENU and TEMPO sent
  nothing on the panel bus**: they are gpio-keys only ([Panel, buttons and lights](#panel-buttons-and-lights)).
  E1 to E6 were turned and not pushed, so their push switches (40, 41, 48, 49, 56 and 57) are still
  unseen; the pedal jacks (HOLD as analog 2, CTRL1 and CTRL2 as 3 and 4) were not tried.
- **The audio link runs without Roland's app.** `aplay -D hw:1,0` played a 6-channel, S32_LE,
  48 kHz file to its end, and the link's DMA interrupts kept their rate throughout: the BMCs
  clock the link by themselves.
- **Audio from Linux is heard by default, on every pair tried.** A quiet sine (about -30 dBFS)
  on channels 2 and 3, then 4 and 5, then 0 and 1, was heard in the headphones each time, with
  no routing or level command sent. Then a tone on each of the six channels in turn, heard on
  the PHONES jack: channels 0, 2 and 4 in the left ear and 1, 3 and 5 in the right, so each pair
  (0 and 1, 2 and 3, 4 and 5) is a stereo pair, even left and odd right, mixed to the headphones
  by default. The same tones, about 12 dB louder, were silent on SUB OUT: none of the six reaches
  SUB OUT by default. Not measured: MAIN OUT L/MONO and R (no cable), the pairs' levels against
  each other, capture, and opening all 16 channels.
- **Oscairn's engine played through it.** From the chroot bundle, `oscairn-engine
  --output-device hw:1,0 --device-channels 6 --sample-rate 48000 --audio-outputs 0,1
  --buffer-frames 128` opened the device (JUCE named it "nocodec ... Direct hardware device
  without any conversions") at 48,000 Hz with a 128-sample buffer; five notes sent with
  `notes.send` on its control socket were heard from the startup synth; `deadlineMisses` and
  `audioUnderruns` were 0 before and after, `cpuLoad` about 0.23 to 0.25 with the startup
  synth, sequencer and delay, and about 416 MB of memory was still available.
- **The engine's load** (later that evening, the same command at 128 and 256 frames, the default
  session: sequencer, synth and delay in their workers; `cpuLoad` from `session.status`, at rest
  and holding an eight-note chord on the built-in synth):

  | Governor | Buffer | At rest | 8 notes | `deadlineMisses` | `audioUnderruns` | Synth worker block, last / most (µs) |
  |---|---|---|---|---|---|---|
  | `ondemand` (1.6 GHz) | 128 | 0.23 | about 0.34 | 2 | 0 | 406 / 3,151 |
  | `performance` (1.8 GHz) | 128 | 0.20 | about 0.25 | 2 | 0 | 160 / 5,814 |
  | `performance` | 256 | 0.13 | about 0.21 | 0 | 0 | 733 / 822 |
  | `ondemand` | 256 | 0.17 | about 0.29 | 0 | 0 | 1,076 / 2,118 |

  The owner heard no click in any run. Read from it: most of the load is a fixed cost per block,
  there with nothing playing; the misses come from the worst worker block, 3 to 6 ms against a
  block of 2.67 ms at 128 frames. 256 frames removed the misses, at 2.7 ms more latency; the
  profile keeps 128 until the owner decides. `oscairn-fantom-start` sets the `performance`
  governor ([Starting Oscairn on the unit](#starting-oscairn-on-the-unit)). Not known: other
  plugins' load, and where the time at rest goes.
- **Roland's app does not come back without a power cycle.** Started again after being stopped,
  it showed "program version error" and did nothing, with nothing else holding the link. Treat
  stopping it as needing a power cycle to get it back.

## What Roland's programs show

Read on November 12, 2026 from files, with no contact with the unit: Roland's app (`mi078`, the
file the unit runs) and factory test tool (`qctest`) as machine code, the start script, the
device tree from the boot partition, and the names in the BMCs' unencrypted program images.
Each point says where it was read: "the app", "the test tool", "both", or "inferred" for a
conclusion drawn from what was read. **None of it has been seen happen on a unit**; what would
confirm or refute each is in
[What needs the owner at the instrument](#what-needs-the-owner-at-the-instrument). Roland's
files stay off the repository.

### The message link

`/dev/ttyGS0` carries every message between the Linux side and the BMC side.

- **How it is opened** (both): twice, once to read and once to write, raw, with the speed set
  to 31,250, which a USB serial line ignores.
- **Frames** (both; the app's sender and receiver agree, and the test tool holds two ready-made
  frames of the same shape): a stream with no delimiter and no checksum. A short frame is 12
  bytes: byte 0 the kind times two; byte 1 the sender's address; byte 2 the destination's;
  byte 3 a sub-type for the kinds that have one; bytes 4 to 10 up to seven bytes of payload;
  byte 11 how many. A long frame has bit 0 of byte 0 set: its first 8 bytes as above, 4 more
  bytes of header, the payload's length as 32 bits, low byte first, then the payload.
- **Kinds** (the app): 3 is one MIDI word and 6 one panel word, 4 bytes each in a short frame;
  9 is a request or reply told apart by its sub-type; the others, parameter blocks among
  them, were not read in detail.
- **A MIDI word** (the app) is laid out like a USB MIDI event: byte 0's low four bits say what
  it is (8 note off, 9 note on, 0xB control change, 0xE pitch bend) and its high four a port;
  bytes 1 to 3 are the MIDI message.
- **Addresses** (the app, unless said): the Linux program sends as 0x60 (the test tool's
  commands as 0x6F). 0xE1 is the sound engine (the test tool also commands 0xE2; inferred,
  the two BMCs). 0x75 is where panel words for the lamps go. 0x02 is the keyboard: the app's own
  debugging names what comes from it the sub-processor's keys. 0x63 and 0x64 are the two
  places on the Linux side the keyboard's messages are addressed to. 0x81, 0x91 and 0xA1 are
  three MIDI outputs the app picks between by number, and it treats 0x81, 0xA1, 0xB2, 0xC8 and
  0xD8 as MIDI inputs; which is the DIN socket and which a USB port was not determined.
- **Local Switch** (traced November 12, 2026, as Roland's app turned it off and on in its menu,
  with the stock BMC firmware): system parameter 0xCF, sent to address 0xEF as
  `0e 00 ef 01 | 00 00 00 cf vv 00 00 | 05` (vv 0 off, 1 on), each followed by the companion
  frame `10 00 ef 00 | 01 00 00 00 0c 0c ff | 07`.
- **Whether anything must be sent regularly** to keep the BMC side talking was not
  determined; the 58 interrupts a second the survey saw at rest are unexplained.

### Keys

- **Key events reach the Linux side** (the app, and seen on the unit:
  [With Roland's app stopped](#with-rolands-app-stopped)): short frames of kind 3 from 0x02 to
  0x63 or 0x64 (on the unit, 0x61) holding a MIDI word, note on (byte 2 the note, byte 3 the velocity) or note off;
  control changes 64 (hold) and 120 (all sounds off) from the same address are taken the same
  way. The app's "Kbd Task" thread is what takes them. A second form, a panel word of kind 8
  (on) or 7 (off) from 0x02 with a 7-bit note and a 10-bit value, is recognized only by the
  app's debugging; the unit sends it with every key, with the same velocity, and it is the one
  that comes whatever Roland's zones and Local Switch are
  ([With Roland's app stopped](#with-rolands-app-stopped)).
- **The test tool gets them too** (the test tool): its keyboard test shows each key's note
  and velocity from note-on and note-off messages, with Roland's app not running. That they
  come over the same link is inferred: its only other MIDI code, for an ALSA raw MIDI device,
  is never called. Before that it has sent one command to 0xE1 when it started (inferred
  from the BMC's own names: a request for its test mode) and one more at the start of the
  test (inferred: unmute).
- **The BMC plays the keys by itself, and tells the Linux side as well** (heard on the unit
  with Roland's app stopped; before that, inferred from the app): the app's two key handlers keep records of the keys held
  (inferred: for chord memory and the arpeggio) and, as far as their direct calls go, send
  nothing back to the sound engine; notes from the MIDI inputs are handled the same way; and
  Roland's system parameters include a Local Switch. So Oscairn playing from these keys
  sounds together with Roland's engine until that is turned off or its level brought
  down.
- **The BMC does not wait for the Linux side to speak first** (seen on the unit). The app, when
  its parameter thread starts, sends 0xE1 one kind-9 frame of sub-type 3 with no payload; the
  test tool sends its command. With Roland's app stopped and nothing sent, the keys came anyway.

### The lever, wheels, pedals, sliders, knobs and pads

- **All of them are panel words** (both): pitch bend, modulation, WHEEL1 and WHEEL2, the HOLD
  and CTRL jacks, the sliders, the control knobs, CUTOFF and RESONANCE are analog controls in
  Roland's numbering; the pads are switches, with no velocity.
- **Which of them come over I2C is settled for some only.** A panel word says in two bits of
  its first byte whether the panel's own processor made it or a BMC did. A slider's words were
  measured on the panel bus ([Panel, buttons and lights](#panel-buttons-and-lights)). Read from
  the app: the CTRL jacks and their
  plug-sense switches are handled where I2C words are, and so are the pads (the app's pad
  code uses the numbers only I2C words are renumbered to). For the lever, the wheels and the
  HOLD jack nothing read says which side sends them; inferred, the panel's, like the rest.
- **For what comes over I2C, Roland's app is the only path to Roland's sound engine** (the
  app): its "Thread Sw Ad" thread takes each word read from `/dev/i2c-slave-2`, and sends
  every switch and analog word on to 0xE1 over the message link (a CTRL jack's only while its
  plug-sense switch is on); encoders it keeps to itself. So with Roland's app stopped those
  controls no longer reach Roland's engine (inferred), and they reach whichever program reads
  `/dev/i2c-slave-2`.
- The app renumbers each switch through a table of its own before using it; the numbers on
  the bus are the test tool's ([The map](#the-map)).

### Panel messages

- **Layout and numbers:** [The map](#the-map). The layout is the test tool's decoder, bit for
  bit; the app's handling of the same words agrees (the kinds, the 10-bit switch number, the
  9-bit analog number and 12-bit value, the signed 8-bit encoder step). The test tool's list of
  switch names and the app's renumbering table are both 107 long, and the app's table sends
  the three GPIO keys' numbers and the sixteen pads' to the numbers its own code uses for
  them, which is what ties the test tool's order to the bus.
- **How Roland reads the device** (both): open it for reading and writing, call `ioctl` with
  request 0, then read 4 bytes at a time for as long as a read gives 4, and call `ioctl`
  again. Seen on the unit and in the driver's source: the `ioctl` is the driver's wait for a
  message, and each read gives one message until none is queued
  ([The panel's driver](#the-panels-driver)). Oscairn's panel holder does the same
  ([The panel holder](#the-panel-holder)).
- **Other kinds exist** (the app): kind 0 words go to the panel as commands, kinds 0x14 and
  0x15 are analog words the app also passes to the sound engine, and 0x0F another the app
  handles itself. What sends them, and when, was not read.

### The panel's driver

Read from Roland's published GPL source of the unit's kernel (`drivers/i2c/i2c-slave-rx.c`,
Roland's own, and the i.MX I2C driver's slave mode, which Roland added); none of its code is in
Oscairn. Checked on the unit where it says so.

- **`open` registers the i.MX as the I2C slave at 0x08, and `close` unregisters it.** While
  nobody has the device open, the bus does not acknowledge the panel. The open flags make no
  difference.
- **One holder only.** A second `open` while it is held fails with `EFAULT` and, by the code,
  clears the first holder's callback, so the panel's next byte calls a null pointer in the
  interrupt handler: a kernel panic. Not tried, and never to be: Oscairn opens the device in one
  place only, and only while no other process has it open.
- **`ioctl(fd, 0, 0)`** waits until a message is queued. The wait is interruptible and always
  returns 0, also when a signal woke it. Any other request gives `ENOTTY`.
- **`read`** gives one 4-byte message per call, or 0 when the queue is empty. It never blocks and
  ignores the count: a buffer under 4 bytes is overrun, so always pass at least 4. The queue
  holds 255 messages; past that the newest is dropped (and the kernel logs
  `i2c_slave_rx: Rx fifo over flow`).
- **No `poll`** (it always says ready) and **`O_NONBLOCK` is ignored**. **`write`** exists but
  writes into the receive queue and corrupts it: never write.
- **A killed reader is clean:** a process that exits while waiting, killed or not, closes the
  device like any other. What was seen after a killed reader on the unit is explained by the
  gap alone ([With Roland's app stopped](#with-rolands-app-stopped)).

### Lamps

- **Not over I2C** (both): neither program writes to `/dev/i2c-slave-2`. A lamp is set with a
  panel word of kind 5 in a kind-6 frame to address 0x75 on the message link, so the BMC side
  passes it to the panel (inferred).
- **The word** (the app): byte 0 is 0x85; byte 1 the lamp's number, low seven bits; byte 2 its
  top three bits, then a 3-bit mode from bit 3, then bit 7 of the level in bit 6; byte 3 the
  level's low seven bits.
- **The numbers** (the test tool): 0 WHEEL1; 1 and 2 TEMPO (green, and its other color); 3
  to 18 the tone category buttons; 19 PLAY, 20 REC, 21 PAD MODE, 22 SAMPLING, 23 CLIP BOARD, 24
  BANK, 25 HOLD; 26 OSC, 27 AMP, 28 FX, 29 LFO; 30 PATTERN, 31 GROUP, 32 SONG, 33 TR-REC, 34
  RHYTHM PTN; 35 PARAM, 36 FILTER TYPE and 37 to 40 its four type lamps; 41 WRITE, 42 MASTER
  FX, 43 MOTIONAL PAD, 44 DAW CTRL, 45 MENU; 46 SCENE SELECT, 47 SCENE CHAIN, 48 ZONE VIEW, 49
  SINGLE TONE; 50 SHIFT, 51 EXIT, 52 ENTER; 53 PAN/LEVEL, 54 CTRL, 56 ASSIGN; 58 ZONE 9-16, 59
  ZONE SELECT, 60 SPLIT, 61 CHORD MEMORY, 62 ARPEGGIO, 63 TRANSPOSE, 64 and 65 OCTAVE DOWN and
  UP; 66 to 73 ZONE 1 to 8 in green and 74 to 81 in their other color; 82 S1, 83 S2; 84 to
  99 the pads, 100 to 115 their green and 116 to 131 their blue.
- Oscairn sets no lamp yet. With Roland's app stopped the lamps keep whatever it last set
  (inferred).

### The audio link's channels

- **Playback** (the app): its audio callback clears all six channels, then mixes its eight
  sample players, 50 frames at a time, into **channels 2, 3, 4 and 5**; channels 0 and 1 stay
  silent. At least one path writes the same left and right to both pairs (inferred: two stereo
  feeds of the sampler, to two places in the BMC's mix).
- **Capture** (the app): its recording callback takes the first two input channels it is
  given, the stereo sound being sampled, and reads no others. Those are **channels 0 and 1**
  if the app turns on all six inputs, as its playback side must have all six outputs on; that
  was not checked. The BMC's own names include a sampling bus beside its main, sub and USB
  ones (inferred: that bus feeds these two channels).
- **The test tool plays on channels 0 and 1** (the test tool): its "SOM Audio" test starts
  ALSA's example sine player on `hw:1,0` with two channels (the first two slots, by the
  survey's reading of NXP's driver), after sending 0xE1 a command that picks that test and
  its level, and one that it sends at the start of every sound test (inferred: unmute).
  Inferred, weakly, from the test having a command of its own: outside it the BMC does not
  pass channels 0 and 1 to the outputs. Nothing read says the same of channels 2 to 5, which
  carry the pads in normal use; whether they sound at once, or only at a level Roland's app
  has set over the message link, is unknown.
- **Clocks:** the device tree makes the BMC side the bit clock and frame master, and the test
  tool is written to play and record on the link with Roland's app not running at all, so the
  clocks do not come from Roland's app (inferred). Whether they keep running when no program
  has ever opened `/dev/ttyGS0` since power-on is unknown.
- **No level or route for this link is set on the Linux side** (the survey: no mixer
  control). Whatever the BMC does with these channels is set by messages or by its own
  defaults.

### MIDI ports

- **DIN MIDI and the USB COMPUTER port are the BMC side's** (inferred, with the survey): the
  app names no UART, and the only MIDI device code in it is JUCE's, for the ALSA sequencer
  this kernel lacks; what it has are the addresses above, MIDI words in and out over the
  message link.
- **MIDI from those ports does reach the app** (the app): MIDI words from the five input
  addresses are handed to its per-channel handlers (notes, controllers and the rest) for its
  own use; the paths read do not send them back to the sound engine, so the BMC is taken to
  play them itself (inferred).
- **The app can send MIDI out of them** (the app): MIDI words addressed to 0x81, 0x91 or 0xA1.
- The test tool has code to open an ALSA raw MIDI device, `hw:2,0,0`, a card that is not
  there in normal use, but nothing in the tool calls it (the test tool): its MIDI tests go
  over the message link like everything else (inferred).

### What the app does that the hardware depends on

What stops when Roland's app is stopped, from what it was seen to do:

- **Panel words to the sound engine:** switches and analog controls stop reaching 0xE1 (above).
- **Lamps** stop changing.
- **GPIO2_12** (the app): once everything else has started, it maps GPIO2's registers through
  `/dev/mem` and makes bit 12 an output driven high. The device tree gives that pin no name
  or use, and the app never drives it low again. The test tool does not touch it, and its
  sound, keyboard and panel tests are written to work without it, so it is not what lets
  audio or keys through (inferred); what it does is unknown. It stays high after the app
  stops (inferred: nothing resets the register).
- **Start words** (the app): when its parameter thread starts it sends the panel two kind-0
  words through 0x75 (inferred: asking the panel where its controls stand) and the sound
  engine the kind-9 frame above.
- **The sampler and sequencer:** pad and sample playback are the app's own audio (channels 2
  to 5), sampling its capture, and its sequencer's clock comes from Roland's `tickgen`
  module (inferred: it signals the app's "Tick Gen Task"). None of it runs without the app.
- **Not found:** whether the app unmutes the outputs at start, as the test tool does at the
  start of each sound test. If it does, the BMC is taken to stay unmuted after the app stops
  (inferred, untested).

## What Oscairn has

- **A device profile**, `--profile fantom-07`
  ([hosts/standalone/profiles/fantom-07.json](../hosts/standalone/profiles/fantom-07.json),
  [RFC 0024](rfcs/0024-device-profiles.md)). It declares the i.MX8M Mini (four cores, one for
  audio, 992 MiB, what the kernel sees), a 1280 × 720 touch screen, 48 kHz at 128-frame blocks, instrument
  reliability, the Performance, Groovebox, Library and Setlist apps, and its panel (below).
  Memory classes ([RFC 0025](rfcs/0025-resource-budgets.md)): `osMemory` 250 MiB and
  `performanceMemory` 300 MiB, so plugin instances kept loaded for the next change are let go by
  memory, not only past 32 (RFC 0045's resident instances). The arithmetic, from the unit: of the
  992 MiB the kernel sees, `free -m` gives 952, so 40 are never ours; the engine's own resident
  memory, about 133 MB with ST Final open, is counted nowhere else (the budget counts the app's own
  process and each plugin instance), so it goes in `osMemory`; the remaining 77 MiB are for the
  kernel's caches, Weston, Xwayland and the other daemons beyond the headroom the app's reservations
  already hold (the Performance and Library apps, always available, reserve 150 and 120 MiB at
  their placeholders while the whole app measured 38 MB). With the margin (5 %, 50 MiB) and the
  app's own memory at start (about 40 MB), plugin instances, in use and kept warm, get about
  380 MiB. ST Final's workers held about 347 MB (480 MB with the engine) when swap filled to 240
  of 256 MB, so it fits with about 35 MiB to keep anything warm, and a change between it and
  another Performance of more than about 35 MiB is a cut (both cannot ring together). The UI's
  room is the app's reservations' headroom and those 77 MiB: estimated, not measured on the unit,
  and only in effect when the app sets the budget (an engine started with `--engine-only` has none,
  so it keeps instances warm by count). To check on the unit: `free -m` and swap use with ST Final
  open and after switching to and from it, and `oscairn-engine-timing`'s instances.
  **Estimated** (`estimated`): `audio` (two inputs for MIC/LINE, four outputs for MAIN and SUB
  OUT, headphones; the audio device below is verified as far as it goes, its inputs and the
  outputs beyond main are not), `controls` (Roland's counts, but whether each reaches the i.MX,
  and the roles below, are Oscairn's guesses) and `midi`. The controls, in order, with what
  they are in Oscairn ([RFC 0024's identities](rfcs/0024-device-profiles.md#default-targets)):
  the lever as `wheel:1` (pitch bend) and `wheel:2` (modulation); WHEEL1 and WHEEL2 as
  `wheel:3` and `wheel:4`, unassigned; S1 and S2 as switches `button:1` and `button:2`; control
  knobs 1 to 8 as `knob:1` to `knob:8`, paged by button; sliders 1 to 8 as `fader:1` to
  `fader:8` on part levels; CUTOFF and RESONANCE as `knob:9` and `knob:10`, a row playing the
  Filter bank (its first two roles are cutoff and resonance); E1 to E6 as screen knobs
  `screen:1` to `screen:6`; ZONE INT/EXT 1 to 8 as `button:101` to `button:108` on part mutes;
  the VALUE dial, MASTER VOLUME and every other button as fixed functions; the keys as notes and
  the pads through the Performance's pad map; the CTRL jacks' plug sensing as fixed functions
  nothing plays; the HOLD jack as sustain and CTRL1 and CTRL2 as expression pedals, unassigned.
  Each group's `sensing` says what was seen on a unit (the ranges, the pads on the panel bus).
  `controls` stays estimated: MASTER VOLUME is taken never to reach the i.MX (Roland's tables give
  it no panel number), so its `volume` function has nothing to drive it, and the pedal jacks and
  E1 to E6 pushed are not yet seen. `inputKeys` names WRITE, MENU and TEMPO on `gpio-keys`
  ([WRITE, MENU and TEMPO](#write-menu-and-tempo)).
- **The audio device** (the profile's `audio.device`, [RFC 0024](rfcs/0024-device-profiles.md#what-a-profile-holds)):
  `{ "name": "hw:1,0", "channels": 6, "rate": 48000, "outputs": { "main": [0, 1] } }`, what
  played on the unit: the engine opens `hw:1,0` with six channels, as Roland's app does, at
  48 kHz (refusing to start at another rate), plays main left and right on channels 0 and 1 and
  keeps the other four silent. JUCE, which opens it, takes S32_LE when the device offers no
  float format. Heard on the unit: the device, the rate, the six channels and main on 0 and 1
  ([With Roland's app stopped](#with-rolands-app-stopped)), and 0 and 1 as left and right on
  PHONES, where all three pairs are mixed by default. No channel reaches SUB OUT by default, so
  `sub` stays unmapped until routing on Roland's side is found; MAIN OUT is not yet measured, and
  no inputs are mapped until capture is checked. To try another pair, copy the
  profile, change `main` (for example to `[2, 3]`), start the app with `--profile` and the copy's
  path (the built-in one is compiled in), and listen at a low level; or run `oscairn-engine` with
  `--output-device hw:1,0 --device-channels 6 --sample-rate 48000 --audio-outputs 2,3`; the SUB
  OUT pair, once found, goes in as `"sub": [n, n + 1]`.
- **The screen:** the profile gives its diagonal (140 mm, 5.5"), so the UI scales by its density
  ([RFC 0019](rfcs/0019-adaptive-ui.md#screens-of-any-size)): 10.5 pixels per millimeter make a
  scale of 1.55, and `kick-screen` is laid out at the screen's own 826 × 465 design units and
  fills it, with no bars. A touch target is 68 pixels, 6.5 mm, as on the KICK. `desktop-gaps`
  renders Home, Play, the Parts views, an editor, the Library, the Catalog and Settings at
  1280 × 720 and checks every control fits, is that large and overlaps none. On the instrument
  (November 12): the app, started from the chroot with `DISPLAY=:0` (Xwayland's abstract socket
  is reachable from the chroot), drew, played its groovebox through the outputs, and took touch;
  the owner heard glitches while it was drawing, and about 310 MB of memory remained available
  with the app, the engine and three workers running.
- **Typing:** the unit has no typing keyboard, so its screen's profile says `textEntry: "onScreen"`
  and every text field (the Library's search, names, tags, repository addresses) brings up an
  on-screen keyboard along the bottom of the screen, taking under 55% of its height with 44-unit
  (6.5 mm) keys ([RFC 0019](rfcs/0019-adaptive-ui.md#text-entry)). Checked off screen by
  `text-keyboard`; on the instrument, untried.
- **One window on the screen.** Under Roland's Weston 6.0.1 with Xwayland, the first run showed
  the window offset with a border at the top and left and its right and bottom edges off the
  screen: Weston's window manager put it in a frame with a 32-pixel shadow margin. The Plugins
  button and the Catalog also came up as windows of their own, with a close button too small to
  tap. With the FANTOM-07's profile the app now asks for full screen in the standard way
  (`_NET_WM_STATE_FULLSCREEN` before it is mapped, no decorations, at 0, 0 at the screen's size),
  and everything else opens inside that window
  ([RFC 0019](rfcs/0019-adaptive-ui.md#one-window-on-a-devices-screen)). The plugin browser and
  the Library's pick open under the top bar, dialogs and alerts open inside the window with
  44-unit (6.5 mm) controls, and Settings and the Setlist are child windows of the app's own.
  Checked under Weston 13 with Xwayland in the CI image (full screen at 1280 x 720, Settings a
  child at 0, 74); untried on the unit. File choosers still open a window of their own and are
  not for use on the instrument.
- **A panel holder and reader**, built only with `OSCAIRN_ROLAND_FANTOM0` (below). Their code,
  the key reader's, `oscairn-panel-dump`, the panel map and the start and stop scripts are in
  [devices/roland-fantom0/](../devices/roland-fantom0/); the profile stays with the other built-in
  profiles in `hosts/standalone/profiles`, and the bundle's files with the other boards' in
  `os/buildroot/board/fantom-bundle`.
- **A key reader** in the engine, built with it ([The key reader](#the-key-reader)).
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
(`Oscairn`), the CLAP plugins in `/usr/lib/clap`, `oscairn-panel-holder`, `oscairn-panel-dump`
and the panel map in `/usr/share/oscairn/panels`, `oscairn-engine-timing` and `cyclictest`, with the `fantom-07` profile built in; and, in
its top folder beside `start-chroot.sh`, `oscairn-fantom-start` and `oscairn-fantom-stop`
([Starting Oscairn on the unit](#starting-oscairn-on-the-unit)).

Build it (Docker; the first build takes an hour or more):

```sh
os/fantom-bundle.sh       # build-os/oscairn-fantom-bundle.tar.gz
```

It builds `oscairn_fantom_bundle_defconfig`: the EVK image's userland for the Cortex-A53
(ARMv8.0), with `BR2_PACKAGE_OSCAIRN_ROLAND_FANTOM0`, and the toolchain's kernel headers from
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
directly: stop Roland's app before anything opens `hw:1,0` or the panel, and take the panel over
at once ([Starting Oscairn on the unit](#starting-oscairn-on-the-unit), which does all of it).
The engine alone, with no device:

```sh
./start-chroot.sh oscairn-engine-run --audio null --buffer-frames 128
```

`oscairn-engine-run` makes `/run/oscairn` private (mode 700), as the engine requires of the
folder holding its control socket, raises the locked-memory limit with `ulimit -l unlimited`
(the default of 16 MiB leaves memory unlocked), and runs `/usr/bin/oscairn-engine` by its full
path: by its bare name it does not find its plugin worker beside it. It passes
`--plugins /usr/lib/clap` itself, sets `OSCAIRN_RELIABILITY=instrument` unless the environment
has it, and hands its own arguments on to the engine, audio options among them.

Checked in Docker on a Mac (an ARM64 container, so on a far newer kernel than 4.19): the bundle
unpacks, `start-chroot.sh` enters it, the engine starts with `--audio null` and answers
`session.status` on its control socket, and `oscairn-panel-dump` decoded messages fed to it from
a FIFO (before the panel holder; it now reads from a holder only). That shows the userland is complete, not that it runs on the unit's kernel: nothing has
run on the unit yet. Idle there with `--audio null`, the engine held 125 MiB resident (its
memory locked, at the profile's `instrument` reliability) and its three plugin workers 52 MiB
together. On 4.19 the engine can have no memory pressure trigger (PSI is Linux 4.20 and later):
by its code it then logs that it has none and goes on without. The app shows on the FANTOM's
screen as one more X11 client on `:0`, as Roland's is: Weston and Xwayland keep running when
Roland's app stops, and though the bundle's own `/tmp` hides Xwayland's socket file, Xwayland
also listens on the abstract socket of the same name, which a chroot does not hide (seen on the
unit on November 12: [The screen](#what-oscairn-has)).

## Starting Oscairn on the unit

`oscairn-fantom-start` ([source](../devices/roland-fantom0/bundle/oscairn-fantom-start)) is the
one way in: run by hand, as root, from the bundle's folder, outside the chroot. Nothing starts it
at boot and Roland's `go.sh` is not touched. In order, each step printed and logged to
`var/oscairn/log/oscairn-fantom.log` in the bundle (the bundle's `var/log` is Buildroot's link to
its `/tmp`, which `stop-chroot.sh` unmounts):

1. It refuses to run while another start runs or Oscairn's app or engine already runs.
2. It mounts the chroot (`start-chroot.sh`), before anything else changes, so that the next two
   steps follow each other at once.
3. If Roland's app runs (`systemctl is-active weston-term`, or a process `mi078`), it stops it
   with `systemctl stop weston-term` and waits until `mi078` is gone (at most 15 seconds).
4. At once it takes the panel: it checks that no process has `/dev/i2c-slave-2` open, starts
   `oscairn-panel-holder`, and waits for its log (`var/oscairn/log/oscairn-panel-holder.log`) to
   say it holds the device. The holder checks again before each open and retries only a
   refused open, for two seconds ([The panel holder](#the-panel-holder)). A holder already
   running is kept.
5. It sets every CPU's `scaling_governor` to `performance`, recording what each had in
   `var/oscairn/run/governors` ([the engine's load](#with-rolands-app-stopped)).
6. It starts **the app** on the display, as the instrument's normal start: `/usr/bin/Oscairn
   --profile fantom-07 --plugins /usr/lib/clap` in the chroot, with `DISPLAY=:0` (Weston and
   Xwayland keep running when Roland's app stops, and Xwayland's abstract socket reaches into
   the chroot) and the locked-memory limit raised, its output in
   `var/oscairn/log/oscairn-app.log`. The app starts its own engine with the profile's audio
   device, keys, panel and input keys, and sends it the profile's control groups. It does so for
   a profile with a screen (`OSCAIRN_HEADLESS=0` and `OSCAIRN_PROFILE` in
   `etc/default/oscairn-engine`, written from the profile when the bundle is built). With
   `--engine-only`, or for a profile without a screen, it starts **the engine alone** through
   `oscairn-engine-run` with the profile's options from the same file (`hw:1,0`, six channels,
   48 kHz, main on 0 and 1, 128 frames, the keys on `/dev/ttyGS0`, the panel and its map, the
   profile's id for its control groups, WRITE, MENU and TEMPO on `gpio-keys`), its output in
   `var/oscairn/log/oscairn-engine.log`: the startup Performance with nothing on the screen,
   played by the keys and the panel, for measuring the engine alone and for tests. Either way it
   clears `var/oscairn/run/stopped` first (below).
7. It waits for the engine's control socket (the app's engine's is `engine-*.sock` in the
   bundle's `/tmp/oscairn-0`; the engine alone's `run/oscairn/engine.sock`) and asks it for
   `session.status` (`oscairn-control-request`), within 20 seconds for the engine alone and 40
   for the app, and prints the answer, or the end of the log and an error.

Between Roland's app stopping and the engine opening `/dev/ttyGS0` nothing reads the keys' link,
and Roland's sound engine queues what is not read; after a long gap on the unit it was left with
notes stuck on. The few seconds the script takes were harmless. Roland's engine is
told Local Switch off by the engine as it opens the keys' link (the profile's
`keyboard.localOff`), after Roland's app has stopped; the script has a marked place for what is
not done yet: USB MIDI (`modprobe snd-usb-audio`, outside the chroot; on November 12 the kernel
loaded it by itself when a keyboard was plugged in). `--dry-run` checks what it can without
changing anything (another start, Roland's app, who holds the panel) and prints the rest.
`--cold` is for a boot on which Roland's app never ran ([Starting at power-on](#starting-at-power-on-not-done)):
nothing to stop, Roland's app running is a failure, and the panel is taken at once. It has a
marked, disabled place for Roland's start word: when Roland's app starts it sends the panel
`0c 00 75 00 80 30 00 00 .. 04` (to 0x75 over the link). A takeover after Roland's app had run
needed none, and the word did not revive a panel that had gone silent; whether a panel that
Roland's app never woke waits for it is not known, and is to be tried on the unit before the
step is enabled.

`oscairn-fantom-stop` stops the app (which stops its engine) and any engine left, puts the
governors back, and unmounts the chroot. Before stopping them it leaves `var/oscairn/run/stopped`, which says the stop was on purpose, for
`oscairn-fantom-boot` ([Starting at power-on](#starting-at-power-on-not-done)). It
does not turn Local Switch back on: a power cycle does. It
leaves the panel holder running, so the panel goes on reporting; `--panel` stops the holder too,
after which the panel stays silent until the next power-on. While the holder runs the bundle's
`/dev` stays mounted (the holder has the device open through it). It never starts Roland's app:
started again after a stop it shows "program version error". A power cycle returns the
instrument to Roland's system.

```sh
cd /home/root/oscairn
./oscairn-fantom-start --dry-run
./oscairn-fantom-start              # the app on the display, and its engine
./oscairn-fantom-start --engine-only    # or the engine alone
./oscairn-fantom-stop               # the holder keeps the panel
./oscairn-fantom-stop --panel       # the panel silent until power-on
```

## Starting at power-on (not done)

What the owner means to do once Oscairn is stable enough: Roland's start script
(`/home/root/go.sh`) runs Oscairn instead of Roland's app, with a way back to Roland's app at
every power-on. Nothing is installed for it and `go.sh` is not touched; the pieces are in the
bundle.

`go.sh` does, in order: `swapon /swapfile`; reads GPIO138 (1 starts the factory test tool
instead of the app); stops `systemd-timesyncd` and sets the date from `/home/root/date.txt`; one
`memtool` register write; `modprobe libcomposite`, `g_serial` and `tickgen`; `sleep 1`; then,
with `XDG_RUNTIME_DIR=/run/user/0 WAYLAND_DISPLAY=wayland-0 DISPLAY=:0`, runs `/home/root/mi078`.
`oscairn-fantom-boot` ([source](../devices/roland-fantom0/bundle/oscairn-fantom-boot)) is meant
to take that last line's place, after everything before it has run:

1. **Roland's app when asked for:** MENU held at power-on (read with `oscairn-key-held`, which
   asks `gpio-keys` for key 139's state once, read-only; WRITE held at power-on is the
   bootloader's update mode, so MENU is the choice key), or the file
   `var/oscairn/stock-next-boot` in the bundle (removed as it is honored: one boot of Roland's
   app), or MENU that cannot be read. It then starts Roland's app as `go.sh` does.
2. **Otherwise Oscairn:** `oscairn-fantom-start --cold`, which starts the app on the display
   and, through it, the engine.
3. **The fallback:** if that fails, the engine not answering `session.status` within 20 seconds
   among the failures, it runs `oscairn-fantom-stop --panel` and starts Roland's app, logging
   why. It never starts Roland's app while a process still holds `/dev/i2c-slave-2`.
4. **It stays while Oscairn runs.** `go.sh` is the main process of `weston-term.service`
   (`ExecStart=/home/root/go.sh`, `After=weston.service`). If the boot script returned, `go.sh`
   would end, and systemd, with a unit's default settings (`KillMode=control-group`), would stop
   the unit and kill everything left in its control group: the panel holder, the app and the engine,
   which `setsid` takes out of the session but not out of the control group. The panel would
   then stay silent until the next power-on. So it waits in the foreground, looking each second
   for the app and the engine (by their process names). If both have ended and
   `var/oscairn/run/stopped` is not there, Oscairn is taken to have failed: the fallback, as in 3. If it is there,
   `oscairn-fantom-stop` was run by hand: Roland's app is not started, and it goes on waiting
   while the panel holder runs (Oscairn started again by hand with `oscairn-fantom-start`
   clears the file and is watched the same way). Once neither runs (`oscairn-fantom-stop
   --panel`), it returns: nothing is left for systemd to stop. A loop in the unit's own process
   was chosen over `systemd-run --scope`, which would move Oscairn into a scope of its own but
   needs systemd's bus and more of systemd than `go.sh` uses; the loop needs nothing.

The unit's real settings were not read: before changing `go.sh`, read them with

```sh
systemctl show weston-term -p Type -p KillMode -p RemainAfterExit
```

`Type=simple` (or none, which is simple), `KillMode=control-group` and `RemainAfterExit=no` are
the defaults the paragraph above assumes; `KillMode=process` or `none` would leave Oscairn
running when `go.sh` ends, which the loop does not mind either way.

The fallback is best effort: started again after it had run earlier in the same boot, Roland's
app showed "program version error", and whether it starts cleanly after Oscairn has used the
link first (Local Switch among it) is not tried. The sure way back is holding MENU at the next
power-on. `oscairn-key-held` itself has not run on the unit (its bit lookup is tested); it
answers 2, and so Roland's app, when it cannot read MENU.

The change the owner would make by hand, when he decides to: in `/home/root/go.sh`, replace the
last line, `/home/root/mi078` (with its environment), by

```sh
XDG_RUNTIME_DIR=/run/user/0 WAYLAND_DISPLAY=wayland-0 DISPLAY=:0 /home/root/oscairn/oscairn-fantom-boot
```

(the bundle's folder as unpacked). To undo it, put the line back; Roland's update procedure also
reinstalls `go.sh`. `./oscairn-fantom-boot --dry-run` reads MENU (mounting the chroot to run
`oscairn-key-held`) and the one-shot file, and prints what it would do. `--supervise` is step 4
alone, for an Oscairn already running. `devices-fantom-start` checks each path off the unit,
dry, the waiting on a stand-in `/proc`: staying while the engine runs, the fallback when it ends
unasked, waiting on the holder after a stop on purpose, and returning once the holder is gone.
None of it has run under systemd.

## The panel reader

### The panel holder

`oscairn-panel-holder` (`holdPanel` in [panel_reader.h](../devices/roland-fantom0/include/oscairn/panel_reader.h))
is the only program that opens `/dev/i2c-slave-2`. One small process with one thread, started
once by `oscairn-fantom-start` and left running until the system goes down or it is sent
SIGTERM: it outlives the engine, so the engine can stop and start again without the panel being
left unheld ([With Roland's app stopped](#with-rolands-app-stopped)).

- **One only.** It first binds the abstract socket `@oscairn-panel`; a second holder finds it
  taken and ends without touching the device. The kernel frees the name when the holder ends.
- **The takeover guard.** It opens the device only while no other process has it open (it reads
  `/proc/*/fd`, comparing the files themselves, so a path through the chroot's `/dev` counts);
  while one does, it waits, and after two seconds gives up. A refused open (`EFAULT`, the last
  holder not gone yet) is tried again within the same two seconds. It opens it once, for reading
  and writing as Roland's programs do, and never writes, polls or opens it again.
- **The loop:** `ioctl(fd, 0, 0)`, then 4-byte reads until one gives less, each message sent to
  every reader attached; with none attached, messages are dropped. A wait that fails (a device
  that is not Roland's driver) ends the holder; there is no other way of reading it.
- **Readers** attach to `@oscairn-panel` (abstract sockets are not hidden by the chroot). A reader
  attaching raises SIGIO, which ends the wait so the holder takes it in at once; failing that,
  the next message does. A reader going raises SIGIO too (each reader's socket is set to signal
  on its hang-up), and the holder drops it then and logs `a reader detached`; one that goes
  just before a wait begins is dropped at the next message, when sending to it fails, and costs
  nothing meanwhile. After any wake the holder reads every message queued, so a wake for a
  reader loses none of the device's. A reader that falls behind loses messages.
  `oscairn-panel-dump` is one reader: its first connection, which checks that a holder runs, is
  the one it reads from (on November 12 it showed as two, a check and then a reader).
- **Stopping:** SIGTERM or SIGINT, handled with every signal unblocked, end the wait (it returns
  0) and the holder closes the device and ends. A signal that comes just before the wait begins
  is only seen at the next message; SIGKILL is as clean for the driver.

Tested off the unit, through a seam that stands in for the driver (`control-panel-reader`): the
wait and the reads until nothing is left, the second holder refusing, the guard refusing while
another process has the file open (on Linux), SIGTERM ending a waiting holder within half a
second, a reader attaching after another went while the device stays open, a reader that goes
dropped at once with no message from the device, and the dump attached to a holder as one
reader. On the unit (November 12) it held the device from two seconds after Roland's app
stopped and passed the panel on to the engine and the dump; the prompt detaching is not yet run
there. The second `oscairn-engine` process seen that night was the panel reader's forked helper
of the time, a copy-on-write copy of the engine; the holder replaced that reader, and the second
process is gone with it.

### The reader

`oscairn::PanelReader` ([panel_reader.h](../devices/roland-fantom0/include/oscairn/panel_reader.h))
attaches to the holder and reads its stream on a thread of its own, polling with a 100 ms
timeout, so it never blocks the audio or message thread and stops within that. It never opens
the device. Where no holder answers, it says so once and tries again each second; when the
holder ends, it attaches again a second later. A byte with bit 7 set starts a message; bytes
before one are skipped and a message cut short by the next one's first byte is dropped, so it
finds its place again after garbage or a lost byte, and messages split across reads join up.
Each message is decoded through the **panel map** and handed to the engine, which plays the
control it names through the controller-mapping layer a MIDI controller's controls go through
(`Session::controllerControl`, RFC 0011's instant controls): analog controls are positions with
pickup, encoders step from the current value, switches toggle on each press. A pad (`pad:N`)
plays through the Performance's pad map, on the control thread, at full velocity unless its
assignment sets one: the panel's pads have none. An analog control's chatter (the map's
`analogDeadband`: the same value again, or a turn back of that many steps or fewer, short of
either end) is not played; a move on in one direction always is, step by step, and the ends are
always reached. Messages no entry names are logged with their bytes, at most ten a second. The
map can be replaced while it runs.

The profile's `panel` says which reader, which device and which map:

```json
"panel": { "protocol": "roland-i2c-slave", "device": "/dev/i2c-slave-2", "map": "fantom-07.json" }
```

The app passes them to the engine it starts (`--panel-device`, `--panel-map`, which the app
also takes on its own command line, to try another map); a map named without a path is found in
`share/oscairn/panels/` beside the installed program. The engine reads the panel from the holder
whatever `--panel-device` names; `oscairn-engine-run` warns when it is given a panel and no
holder runs. A build without `OSCAIRN_ROLAND_FANTOM0` (macOS, the desktop) leaves the reader out and says so when a profile has a panel.

**What a control plays comes from the device's control groups** (the profile's `controls`, RFC
0024's default targets: sliders on part levels, knobs on the banks, ZONE INT/EXT on part mutes).
The app sends them to its engine (`controllers.set`). Without the app nothing did, so on November
12 the engine started by `oscairn-fantom-start` alone read slider 1 (`fader:1`, 0 to 127) and
played nothing: no group named `fader:1`, so the control resolved to no target at all, and the
pickup and the dead band never came into it. The engine now takes the groups from the profile
itself, `--device-profile fantom-07` (built in under its id), which `engine-args.py` adds to
the engine's options for a profile with a panel, until an app sends its own. The engine's
start-up Performance has two parts, Synth (the Basic Synth into the Delay, which the keys play)
and Groovebox (the step sequencer, playing the same synth): it has parts, and slider 1 is the
Synth part's level. A slider picks up: the Synth part starts at full level, so slider 1 does
nothing until it has been brought up to the top, and then follows. `engine-panel-startup`
checks it: the engine on the null device, slider 1's messages decoded through the shipped map
by a reader on a pipe, nothing played without the profile, the Synth part's level following
with it. No default Performance needs opening for this. The engine alone with the profile's
groups is not yet run on the unit; with the app, which sends them, the sliders played the part
levels on November 13. The instrument's normal start is the app, which `oscairn-fantom-start`
now starts ([Starting Oscairn on the unit](#starting-oscairn-on-the-unit)); the engine alone
(`--engine-only`) is the headless engine with the startup Performance, for measurements and
tests.

### The app's own file

The app finds its engine (`oscairn-engine`), its plugin scanner, its themes and, without
`--plugins`, its plugins (`../lib/clap`) beside its own program file. JUCE takes that file on
Linux from `argv[0]` against the working directory, so the app started by its bare name through
`PATH` looked for `oscairn-engine` in the working directory, and for its plugins at a build
machine's path compiled in as the last resort. It now reads Linux's own record of it
(`/proc/self/exe`, [Executable.h](../hosts/standalone/Executable.h)), which in the chroot is
`/usr/bin/Oscairn`, so it finds `/usr/bin/oscairn-engine` and `/usr/lib/clap` however it is
started (`desktop-executable-file` starts a program by its bare name from `/`). The start script
still gives the full path and `--plugins /usr/lib/clap`.

### The map

[devices/roland-fantom0/panels/fantom-07.json](../devices/roland-fantom0/panels/fantom-07.json)
([schema](panel-map.schema.json)). `layout` says where each number sits: a field is a list of
`bytes` (0 to 3) joined most significant first, the low `bits` of each, then `shift`, `mask` and
`signed`; or, where a number's bits are not the low bits of each byte, a list of `parts`, each
`bits` bits of a `byte` from bit `shift` up, joined most significant first. `analogId` is an
analog message's ID where it is not where `id` is. `kinds` gives the kind field's value for each
of the four kinds. `controls` lists each
control's `kind` (`encoder`, `analog`, or `switch` for both switch kinds), `id` and `control`:
an identity above, or a fixed control's function from the profile (`enter`, `play`). An analog
value is divided by `max` (or the layout's `analogMax`) for 0 to 1; an encoder's delta is
multiplied by `step` (or `encoderStep`).

The shipped map's layout and IDs are **read from Roland's programs**, and on the unit every
control the owner touched decoded to its name
([With Roland's app stopped](#with-rolands-app-stopped)): the layout from the factory test tool (`qctest`), which prints each message's kind, number and
value, and from the app (`mi078`), which reads the same bits; the IDs from the test tool's
tables of names, which the app's own table of switch numbers matches entry for entry. Its
`estimated` list is empty. The layout:

| Byte | Bits | What |
|---|---|---|
| 0 | 7 | always set: a message starts here |
| 0 | 6 and 5 | who sent it: 0 the panel's own processor, otherwise a BMC |
| 0 | 4 to 0 | the kind: 1 switch off, 2 switch on, 3 encoder, 4 analog |
| 1 | 6 to 0 | the ID's low seven bits |
| 2 | 2 to 0 | a switch's or encoder's ID, top three bits (an analog ID: bits 1 and 0, its top two) |
| 2 | 6 to 3 | an encoder's step, top four bits |
| 2 | 6 to 2 | an analog value, top five bits |
| 3 | 3 to 0 | an encoder's step, low four bits: eight bits in all, signed |
| 3 | 6 to 0 | an analog value, low seven bits: twelve bits in all |

`controls` gives each control Roland's number for it, in Oscairn's order. What the names rest
on: Roland's test tool names every number (its `STEP01` to `STEP16` are the tone category
buttons, `C1` to `C6` the FUNCTION knobs E1 to E6 and their pushes, `VAL` the VALUE dial,
`VOLUME1` to `VOLUME8` the control knobs, `SELECT` and `CHAIN` SCENE SELECT and SCENE CHAIN,
`ZONE1` to `ZONE8` the ZONE INT/EXT buttons); matching those names to the panel's legends is
Oscairn's reading, and what the owner touched agreed, but for SAMPLING and PAD MODE, whose
numbers were the other way round until the tour of November 12 (SAMPLING 23, PAD MODE 22). The
scale, each seen on the unit and given in each entry: sliders and control knobs have `max` 127;
the lever, the wheels, CUTOFF and RESONANCE `max` 1023. The pedal jacks, not yet seen, keep the
layout's `analogMax` of 1023 (Roland's pedal adjustment checks a reading against 500 to 1024).
`analogDeadband` is 1: one slider chattered by one step at rest. The pads are `pad:1` to
`pad:16` of the Performance's pad map, which counts from the bottom left (as on the KICK): pad 1
is the bottom left, switch 101, and pad 16 the top right, switch 90, as the owner's presses
showed. Roland numbers them the other way up, its pad 1 the top left (switch 91), as its panel
is taken to be printed; the printed labels are not yet checked. The CTRL jacks' plug sensing is switches 35 and 36 (CTRL1 and
CTRL2, Roland's "sense") and 37 and 38 (their ring contacts), as fixed controls nothing plays
(`ctrl1Sense`, `ctrl2Sense`, `ctrl1Ring`, `ctrl2Ring`); Roland's app passes a CTRL jack's position
on only while its sense switch is on.

Not in the map, with Roland's numbers for them:

- **The HOLD jack:** analog 2, a position and not a switch, so a half-damper pedal reads as one.
  The controller layer's footswitch takes a switch; nothing turns the position into one yet.
- **MASTER VOLUME:** Roland's tables have no number for it. It never reaches the i.MX as a panel
  message (inferred: it acts on the BMC side or in the analog path).
- **WRITE, MENU and TEMPO:** numbers 104 to 106 in Roland's tables, but the panel bus never
  carries them (pressed on the unit, they sent nothing there): they are GPIO keys, and Roland's
  programs make the messages themselves from `/dev/input/event1`. The engine reads them there
  ([WRITE, MENU and TEMPO](#write-menu-and-tempo)).
- Switches 39, 51, 63, 81, 84 and 85 are unused; Roland's app drops them.

### WRITE, MENU and TEMPO

`oscairn::host::InputKeys` ([devices/roland-fantom0/src/InputKeys.h](../devices/roland-fantom0/src/InputKeys.h)),
built only with `OSCAIRN_ROLAND_FANTOM0` and only on Linux, reads them from `gpio-keys` on a
thread of its own, polling with a 100 ms timeout: it finds the input device by its name among
`/dev/input/event*` (the number may change), opens it read-only and never grabs it, so Weston
(and Roland's app, when it runs) go on getting every event. Each press and release of a key the
profile names is played as the panel's switches are, by its fixed function (`write`, `menu`,
`tempo`), through the same controller layer; a key's autorepeat and every other key are passed
over. A device that cannot be found is said once and looked for again each second, and so is
one that fails. The profile's `inputKeys` says which:

```json
"inputKeys": { "protocol": "linux-input", "name": "gpio-keys", "keys": { "write": 157, "menu": 139, "tempo": 152 } }
```

The app passes it to the engine it starts, and `engine-args.py` to the headless one
(`--input-keys gpio-keys --input-key-map 139=menu,152=tempo,157=write`). Like the other fixed
controls, they play nothing yet ([What it does not do yet](#what-it-does-not-do-yet)). Tested
on `struct input_event` records through a pipe (`devices-input-keys`, Linux; the unit's kernel
has no uinput); not yet run on the unit. MENU held at power-on is still read on its own, by
`oscairn-key-held`, before the engine starts ([Starting at power-on](#starting-at-power-on-not-done)).

### What it does not do yet

- **The HOLD jack:** on the panel bus, decoded and shown by the dump, played by nothing
  ([The map](#the-map)).
- **Lamps:** none is set; they go over `/dev/ttyGS0` ([Lamps](#lamps)), which Oscairn only reads.
- **Fixed controls** (the VALUE dial, cursor, ENTER, EXIT, transport and the rest) decode and show
  in the dump, and WRITE, MENU and TEMPO reach the engine from `gpio-keys`, but the engine has
  nothing to give them to: the controller layer plays assignable controls only.
- **The lever and pedals with nothing assigned:** a MIDI controller's wheel then reaches the
  parts as its own MIDI message; a panel message has none, so `wheel:1` and `wheel:2` play only
  what a Performance or Sound assigns them.

## The key reader

`oscairn::host::BmcLinkReader` ([devices/roland-fantom0/src/BmcLink.h](../devices/roland-fantom0/src/BmcLink.h)), built only
with `OSCAIRN_ROLAND_FANTOM0`, reads `/dev/ttyGS0` on a thread of its own, polling with a 100 ms
timeout. Before reading or writing anything it makes the device raw with echo and flow control
off, and reads the settings back; where they do not hold it neither reads nor writes (with echo
on, the tty layer would send each byte received back to the BMC side). **It writes one thing
only:** with the profile's `keyboard.localOff` (`--keyboard-local off`), the two Local Switch off
frames ([The message link](#the-message-link)), once, the first time the device is open and raw;
it then opens the device for reading and writing, and otherwise read-only. A failed write is
logged (`Local Switch off not sent: ...`, or `Local Switch off sent` when it went) and reading
goes on. The frames are the stock BMC firmware's, as Roland's app sends them; the setting holds
until a power cycle, and nothing turns it back on. Once
open it reads without pause, whatever the engine is doing and whether or not the input is
ignored, since what it does not read waits and Roland's engine then holds its notes. If the
device fails or ends it is opened again a second later.

- **Frames:** each key's own frame (kind 6 from 0x02: 0x88 on or 0x87 off, the note, 0, the
  velocity) is one note, on or off with its velocity, the release velocity included. It comes
  once per key event whatever Roland's zones and Local Switch are. The MIDI words from 0x02 are
  Roland's zones' notes for the key (one per active zone, none with Local Switch off) and are
  passed over, so are the sound engine's status frames (kind 9 from 0xE1). Before November 12's
  trace the reader took the keys from those MIDI words, which gave a note per active zone for
  each key and none with Local Switch off. Kind-3 MIDI words from 0x81, 0xA1, 0xB2, 0xC8 and
  0xD8, the addresses Roland's app takes MIDI from its MIDI ports on, are read as MIDI: notes, controllers, pitch bend, pressure and
  program changes. Anything else is counted and logged with its header bytes, at most ten a
  second.
- **Inputs:** the keys are a MIDI input named `FANTOM keys`, and each of the five addresses one
  named by its address (`FANTOM MIDI 0x81`) until it is known which port is which. They are
  listed in `session.status` and `controllers.inputs` after the devices' inputs, whether or not
  anything has arrived, each with a source number kept while the engine runs, and play like any
  other input: parts' device rules, zones, controller profiles, ignoring and learn. A key's own
  frame carries no channel, so the keys play on channel 1, the channel a keyboard sends on by
  default; a part takes them as it takes any input's channels. The other inputs' channels are
  their words' own.
- **Finding its place:** the link has no delimiter or checksum. The reader takes a position as a
  frame's start only where a frame it knows begins (a MIDI word whose code index agrees with its
  status, a panel word, a status frame no longer than 4 KiB) and the next frame's header after
  it is plausible too; then it follows the headers, and a header that is not (a kind of 0, more
  than seven payload bytes, a long frame over 1 MiB) has it search again from the next byte.
  Started at each of the first 2,048 byte offsets of the owner's capture (and, once, at every
  offset), it found every key event whose frame starts after the offset and nothing else, never
  skipping more than 127 bytes; read from the keys' own frames, that capture gives its 122 note
  events. With Local Switch off no MIDI word flows at all: the keys' own frames and the status
  frames are enough to find the place, on a stream of that shape built in the test and on a
  capture of the unit with Local Switch off (300 key frames in 18 seconds, all found from each
  of the first 2,048 offsets; the capture stays off the repository).
- **What was queued:** for the first half second after each open, and until a read leaves
  nothing waiting, MIDI words are dropped (counted and logged once), so stale notes do not all
  sound at once. When the device fails or ends, each note it passed on and has not seen end
  gets a note off, and a held sustain pedal a release, so nothing is left held across a reopen.

The profile's `keyboard` says which reader and which device:

```json
"keyboard": { "protocol": "roland-bmc-stock", "device": "/dev/ttyGS0", "localOff": true }
```

The protocol names the firmware on the other side: `roland-bmc-stock` is the link as Roland's
stock BMC firmware speaks it, so other firmware on the BMCs would be another value (and the
panel's `roland-i2c-slave` names what Roland's stock panel firmware sends). The app passes the
device to the engine it starts (`--keyboard-device`, which the app also takes on its own command
line). As with the panel, the engine reads it with `--audio null` too: it is a device named on
purpose, not one found by listing. A build without `OSCAIRN_ROLAND_FANTOM0` leaves the reader out
and says so when a profile has a keyboard. Tested on frames built in the test and on a pty in
place of the device (`engine-bmc-link`: the Local Switch frames byte for byte, written once,
after the device is raw, nothing else written, and none to a device that cannot be made raw),
and its parser on the owner's captures; it has not run on the unit.

## Checking the panel map on a unit

On the FANTOM, with the panel holder running (`oscairn-fantom-start` starts it; the engine may
run too, both read the holder's stream), in the bundle:

```sh
./start-chroot.sh oscairn-panel-dump /usr/share/oscairn/panels/fantom-07.json
```

The dump never opens the device: without a holder it says so and ends.

Each message prints on a line: its four bytes, the kind, ID and value the map's layout reads,
and the control it maps to, or `-`. Ctrl-C (or `--seconds N`) ends it with a table of every kind
and ID seen, how often, and its lowest and highest value. Then:

1. **The layout first.** Press and release ENTER slowly, turn the VALUE dial one click each
   way, and move slider 1 end to end. The map is right if the lines read `switchOn` then
   `switchOff` with ID 62 and `enter`; `encoder` with ID 6, `data` and a small value, positive
   one way and negative the other; and `analog` with ID 9, `fader:1` and a value that rises
   steadily with the slider, the ID never changing as it moves. A kind of `unknown`, an ID that
   jumps while one control moves, or a value that wraps mid-travel refutes the layout: send
   back the raw bytes.
2. **Then each control,** one at a time, in the order of the map's `controls`: move it and
   check the line names it. A control that shows another's name refutes that ID: note the kind
   and ID the table gives it. Note each analog control's lowest and highest value (the table's
   `low` and `high`): they settle `analogMax`, or a `max` of its own for a control that differs.
   Give an encoder that turns the wrong way a negative `step`.
3. **What is not in the map** should show with `-`: a pedal in HOLD as analog 2. Each pad
   should name its `pad:N` and a plug going into CTRL1 `ctrl1Sense`. WRITE, MENU and TEMPO
   show nothing at all (seen on November 12).
4. **Check:** with the engine running (`oscairn-fantom-start`), hold a key and move slider 1 to
   the top, then down: the Synth part's level follows from the top on (it picks up there). With
   the app, run it with `--panel-map` pointing at the map (or a corrected copy).

Send back the dump's tables, and a corrected map if anything differed.

## The first run on the unit

November 12, 2026, late evening, with a bundle built from the commit that added
`oscairn-fantom-boot`. One BMC ran a modified diagnostic build of its CORE1 firmware from the
owner's research (mostly stock), so these measurements are not on pure stock firmware.

- **`./oscairn-fantom-start` worked the first time,** run soon after power-on with Roland's app
  running: Roland's app stopped, the panel holder held `/dev/i2c-slave-2` two seconds later, the
  governors went to `performance`, and the engine opened `hw:1,0` (6 channels, 48 kHz, 128
  frames), logged `Local Switch off sent` and `attached to the panel holder`, and answered
  `session.status`. The keys arrived on `FANTOM keys` as one note each (their kind-6 frames),
  Roland's own engine stayed silent, there was no underrun, and the load was about 0.3.
- **The panel reported:** about 1,600 bus interrupts while sliders and knobs moved, and
  `oscairn-panel-dump`, attached to the holder beside the engine, decoded a three-minute tour of
  the panel ([With Roland's app stopped](#with-rolands-app-stopped)). Slider 1 moved the whole way
  and played nothing ([The reader](#the-reader) says why and what changed).
- **Interrupts:** every interrupt on the system lands on core 0.
- **Real-time priorities are granted:** the engine's audio thread at `SCHED_FIFO` 80, its helpers
  and the workers' audio threads at 70.
- **The CPU's 1.5 ms idle state,** disabled, made no difference.
- **A voice ramp on the built-in synth,** at full speed and 128 frames, held notes against the
  block's budget, with no underrun:

  | Notes | Load |
  |---|---|
  | 8 | 0.28 |
  | 16 | 0.35 |
  | 32 | 0.53 |
  | 64 | 0.81 |

  The notes were sent at a very low velocity and the owner could not hear them, so that they
  sounded rests on the synth's block time, which rose from 0.45 to 1.8 ms.
- **`OSCAIRN_AUDIO_THREAD_CORES`** (the audio threads kept off core 0) made no difference the
  hand tests that night could show; it stays off ([Next session](#next-session)).
- **A USB MIDI keyboard on USB MEMORY** worked through raw MIDI: the kernel loaded
  `snd-usb-audio` by itself, the engine opened three inputs, and notes were heard. With that hub
  connected, one of the link's MIDI-input addresses sent frames the key reader logged as unknown:
  `04 a1 60 00 f0 f7 ...` (a short frame of kind 2 from 0xA1) and a 32-byte long frame from 0xA1.
  They are recorded as seen; what they are is not known.
- **The app on the screen:** [The screen](#what-oscairn-has).

The next evening (November 13), with a bundle built from the commit that made the app's window
full screen, and the app started by hand from the chroot (`/usr/bin/Oscairn --profile fantom-07
--play --plugins /usr/lib/clap`, the panel holder already running):

- **The window fills the screen** under Roland's Weston: the offset and border of the first run
  are gone.
- **The sliders play the part levels** with the app running; the slider on the screen follows
  with a little lag. (The engine alone had no control groups then: [The reader](#the-reader).)
- **Started by its bare name** through `PATH`, the app failed with `configure helper launch: No
  such file or directory`, and without `--plugins` it looked for its plugins at a path of the
  build machine; both came from one cause, fixed since ([the app's own file](#the-apps-own-file)).
  `Oscairn --help` printed `Unknown argument: --help` and then crashed; it now prints the
  usage and quits. Why an unknown argument crashed it, beyond `--help`, was not looked into.
- **Timing** (`oscairn-engine-timing`, 128 frames, the `performance` governor):

  | | Callback p50 | Callback max | Lateness max | Deadline misses | Underruns |
  |---|---|---|---|---|---|
  | The engine alone | | 1.4 ms | 0.36 ms | 0 | 0 |
  | With the app drawing, 40 s | about 1 ms | 6.3 ms | 3.75 ms | 2 | 0 |

  With the app drawing, the engine and two workers all stalled at the same instant, for about 3
  to 6 ms. The kernel alone (`cyclictest -m -p 80 -i 2667 -D 30`): at most 48 µs on core 0 and
  38 µs on core 1, 6 µs on average.
- **Memory,** with the threads' 1 MiB stacks: the engine alone 47 MB resident (142 MB before);
  as the app's engine, playing the Groovebox, 96 MB; the app 38 MB; each worker about 10 MB.
  Available: 517 MB with the engine alone, 440 MB with the app.
- **The engine restarted** with the holder kept running: the new engine attached, and the holder
  never let go of the device.

## Next session

From the bundle's folder, as root, soon after power-on with Roland's app running. The timing
tools are in the bundle: `oscairn-engine-timing` (installed with the engine) and `cyclictest`
(`BR2_PACKAGE_RT_TESTS`), both run inside the chroot. `oscairn-fantom-start` now starts the app
on the display; `--engine-only` the engine alone.

1. **The engine alone, then with its audio threads off core 0,** a minute each, holding a chord
   on the keys (every interrupt is on core 0):

   ```sh
   ./oscairn-fantom-start --engine-only
   ./start-chroot.sh oscairn-engine-timing /run/oscairn/engine.sock --seconds 60
   ./oscairn-fantom-stop
   OSCAIRN_AUDIO_THREAD_CORES=1-3 ./oscairn-fantom-start --engine-only
   ./start-chroot.sh oscairn-engine-timing /run/oscairn/engine.sock --seconds 60
   ./oscairn-fantom-stop
   ```

2. **The app drawing, then the same with its engine's audio threads off core 0,** touching and
   scrolling the screen while it measures; the app's engine listens in the bundle's
   `/tmp/oscairn-0` (the start script prints the socket):

   ```sh
   ./oscairn-fantom-start
   ./start-chroot.sh sh -c 'oscairn-engine-timing /tmp/oscairn-0/engine-*.sock --seconds 60'
   ./oscairn-fantom-stop
   OSCAIRN_AUDIO_THREAD_CORES=1-3 ./oscairn-fantom-start
   ./start-chroot.sh sh -c 'oscairn-engine-timing /tmp/oscairn-0/engine-*.sock --seconds 60'
   ./oscairn-fantom-stop
   ```

   The start script hands the variable to the app, which hands it to its engine and workers (it
   also takes it from `etc/default/oscairn-engine`, where `engine-args.py` writes the profile's
   `compute.audioThreadCores`, empty for the FANTOM-07). The profile does not set it until a
   difference is measured: to keep it, add `"audioThreadCores": [1, 2, 3]` to the profile's
   `compute` (or set `OSCAIRN_AUDIO_THREAD_CORES='1-3'` in `etc/default/oscairn-engine`).

   The start script hands `OSCAIRN_WORKER_BUDGET_US` and `OSCAIRN_WORKER_EXTRA_BLOCK` (plugin IDs
   separated by commas; [RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md)) on in the same
   way, for the app and for `--engine-only`: its own environment's value first, else the file's.
   For example `OSCAIRN_WORKER_BUDGET_US=2000 ./oscairn-fantom-start`.

   The first-party plugins read their own device settings from the environment when an instance
   is created (`OSCAIRN_SAMPLER_MAX_VOICES`, `OSCAIRN_SAMPLER_MEMORY`, `OSCAIRN_ANALOG_FAST_OSC`,
   `OSCAIRN_FM_POLYPHONY` and so on), and the workers inherit the engine's environment, which
   `start-chroot.sh` empties. So the start script hands on every variable named
   `OSCAIRN_SAMPLER_*`, `OSCAIRN_ANALOG_*`, `OSCAIRN_FM_*`, `OSCAIRN_ARPEGGIATOR_*` or
   `OSCAIRN_EFFECTS_*` in the same way: its own environment's value first, else the file's (an
   empty one counts as unset), for the app and for `--engine-only`; no other variable passes. A
   value with anything but letters, digits and `. , : / = @ % + _ -` (a space, a quote, `;`) is
   refused with a message and a nonzero exit, not quoted. For example
   `OSCAIRN_SAMPLER_MAX_VOICES=64 ./oscairn-fantom-start --dry-run` shows it on the engine's line.

   The bundle's `etc/default/oscairn-engine` also holds the FANTOM's device defaults, from
   `os/buildroot/board/fantom-bundle/overlay/etc/default/oscairn-engine-device` (which
   `post-build.sh` appends after the profile's): `OSCAIRN_SAMPLER_MAX_VOICES=28` (the Sampler
   costs about 45 us a voice on this CPU against a 1.33 ms allowance) and
   `OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.analog` (arpeggiated Analog patches overlap about 8
   voices at about 2 ms per block, until its faster oscillator ships).

   Oscairn's engine and workers turn transparent huge pages off for themselves at their start
   (`prctl(PR_SET_THP_DISABLE)`; khugepaged's scan, every 10.24 s, stalled a worker's audio thread
   for 5 to 23 ms, [RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md); confirmation trial
   pending). `OSCAIRN_THP=keep ./oscairn-fantom-start` hands the variable on the same way and
   leaves them as the kernel has them, for comparing.

   While its audio device runs, the engine holds a request for no CPU wake-up latency
   (`/dev/cpu_dma_latency`, [RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#cpu-wake-up-latency)):
   cores 1 to 3 otherwise enter `cpu-sleep` (listed exit latency 1500 us) about 40 times a second
   each while playing, and a worker woken for a block started 330 to 400 us after its hand-over.
   `OSCAIRN_CPU_LATENCY=keep ./oscairn-fantom-start` hands the variable on the same way and asks for
   nothing, for comparing.

   The profile sets `compute.workerSpin` to 200: the engine's thread spins up to 200 us for each
   worker's result before it sleeps, and, while every worker's audio thread has a core of its own
   (three workers or fewer beside the engine), each worker spins from 200 us before its next block
   is due ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#spinning-before-sleeping)).
   `OSCAIRN_WORKER_SPIN_US=0 ./oscairn-fantom-start` turns it off for comparing.

   No wait on a worker goes past the audio callback's deadline (its start plus the period less
   15 %); with the extra block a worker's block is waited for, after the callback's other workers,
   until the deadline of the callback after the one that handed it, so it has at least a period.
   `OSCAIRN_WORKER_DEADLINE=1 ./oscairn-fantom-start` makes each worker wait to its wave's deadline
   instead of its allowance
   ([RFC 0045 step 6](rfcs/0045-audio-scheduling-on-small-devices.md#the-allowance-from-the-deadline-step-6-status)),
   off in the profile until measured.

3. **The kernel beside them,** for comparison, with nothing of Oscairn running but the holder:

   ```sh
   ./start-chroot.sh cyclictest -m -q -p 80 -i 2667 -t 1 -a 0 -D 60
   ./start-chroot.sh cyclictest -m -q -p 80 -i 2667 -t 1 -a 1 -D 60
   ```

4. **The panel and the slider without the app:** `./oscairn-fantom-start --engine-only`, hold a
   key and move slider 1 to the top, then down: the Synth part's level follows from the top on.
   The engine's log should say `control groups from the profile fantom-07`. Press WRITE, MENU
   and TEMPO: nothing is played by them yet, and nothing about them should be logged. Run
   `oscairn-panel-dump` beside the engine and stop it: the holder's log
   (`var/oscairn/log/oscairn-panel-holder.log`) should show one reader attaching and, as it
   stops, `a reader detached` at once, not at the next message. Push E1 to E6 in the dump: each
   should name `pushE1` to `pushE6`. Read the pads' printed labels, and press the pads in the
   app: pad 1 of the pad map is the bottom left.
5. **Before changing `go.sh`:** `systemctl show weston-term -p Type -p KillMode -p RemainAfterExit`
   ([Starting at power-on](#starting-at-power-on-not-done)).

Send back each command's output and the logs in `var/oscairn/log`.

## What needs the owner at the instrument

Each of these needs someone at the panel, Roland's app stopped, or both. Run on November 12, and
done: Oscairn's start ([The first run on the unit](#the-first-run-on-the-unit)), Local Switch
off from the engine, the app on the display, and a USB MIDI keyboard.

1. **The start word on a cold start.** Whether the panel reports to a holder after a power-on
   in which Roland's app never ran, so never sent its start words (kind 0 to 0x75 on the message
   link). Trying it needs Roland's app kept from starting (for example `go.sh` changed for one
   boot); if the panel stays silent, the start words are what wakes it, which would need a
   write. All the takeovers so far followed Roland's app.
2. **The buttons not yet pressed:** E1 to E6 pushed, and the pedal jacks; each should show in
   the dump.
3. **Levels and outputs:** which channel pair feeds which jack (MAIN OUT is not measured; SUB
   OUT gets none by default), and the pairs' levels against each other, at low volume, one pair
   at a time (`--audio-outputs 0,1`, then `2,3`, then `4,5`); and whether `--device-channels 16`
   plays as 6 does.

Older, still open:

4. **DIN MIDI.** Send notes into MIDI IN from another instrument while watching
   `cat /proc/tty/driver/IMX-uart` (each UART's `rx:` count) and the `serial` lines. Expected:
   no UART count moves (DIN MIDI is the BMC side's) and the `usb` line's rate rises (the BMC
   tells Roland's app of them). A UART count that moves refutes the first; a `usb` line that
   stays at rest refutes the second.
5. **USB COMPUTER.** Plug a computer into USB COMPUTER, watch `dmesg -w` and the `32e50000`
   and `32e40000` lines on the unit, and list USB devices on the computer. Whatever the computer
   enumerates (its vendor, product and interfaces) is the BMC's if the unit's Linux saw nothing.
6. **DIN MIDI into Oscairn.** The key reader plays the keys (November 12); send notes into
   MIDI IN from another instrument while the engine runs: whichever `FANTOM MIDI 0x..` input
   plays names the DIN socket's address.
7. **Capture:** the link runs without Roland's app (November 12: playback ran to the end,
    [With Roland's app stopped](#with-rolands-app-stopped)); what comes back is not checked.
    `arecord -D hw:1,0 -c 6 -f S32_LE -r 48000 -d 2 /tmp/link.wav`, and look at the file for
    which of the six channels carry signal while someone plays a key, and with a source on
    MIC/LINE IN. Roland's app samples from its first two inputs, taken to be slots 0 and 1, so sound
    there fits; sound on another pair only says which pair that is; silence everywhere says
    only that the BMC feeds its sampling bus when told to. Remove the file afterward.
8. **A lamp** (the one step that sends the BMC side anything; skip it if in doubt). With
    Roland's app stopped:

    ```sh
    printf '\014\140\165\000\205\064\010\177\000\000\000\004' > /dev/ttyGS0
    ```

    is one frame as Roland's programs build it: a panel word to address 0x75 setting lamp 52
    (ENTER) to mode 1, level 127. ENTER lighting confirms the lamps' path and word; nothing
    happening leaves both open (the mode or level may be wrong, or the BMC may want more first).
9. **The touch screen's range:** `evtest /dev/input/event3` prints each axis's minimum and
    maximum before the first event (Ctrl-C after it); touch the four corners to see which way the
    axes run on the landscape screen. Neither the unit nor the bundle has `evtest`: bring a
    static aarch64 build.

## Building it into an image

`-DOSCAIRN_ROLAND_FANTOM0=ON` links the readers from `devices/roland-fantom0/` into the engine
(and the app's panel handling) and installs `oscairn-panel-holder`, `oscairn-panel-dump`,
`oscairn-key-held`, `oscairn-control-request` and the map in `share/oscairn/panels/`; Buildroot
also puts `oscairn-fantom-start`, `oscairn-fantom-stop` and `oscairn-fantom-boot` in the
bundle's top folder. Buildroot passes it
when a defconfig sets `BR2_PACKAGE_OSCAIRN_ROLAND_FANTOM0`, as the chroot bundle's does.
