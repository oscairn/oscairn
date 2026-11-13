# Running Oscairn on a Roland FANTOM-0

Oscairn has never run on a FANTOM-06, -07 or -08. A FANTOM-07 is now here, running Roland's
stock Linux with root login over SSH; Oscairn has not been tried on it yet. This page says what
is known about the hardware, what Oscairn has for it, what of that is estimated, how to run
Oscairn beside Roland's system in a chroot, and how to check the panel map, as of November 12,
2026.
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
  from Roland's test tool and app ([The map](#the-map)); none of it has been seen on a unit
  yet. Three buttons are not on the panel bus at all: WRITE, MENU and TEMPO are GPIO keys
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
  GPIO5_12 (139) and TEMPO on GPIO5_13 (152). Roland's app and Weston both read it.
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
- **The sound engine reports regardless:** long frames of kind 9 from 0xE1 to 0x60, sub-type 9,
  112 bytes each, about ten a second, at rest too. Their contents were not read.
- **What is not read waits.** With a program reading the link, Roland's engine went on playing
  the keys; when the reading stopped the last notes stayed on, and when it started again the
  queued frames came out in a burst (15 kB in 6 seconds). Whoever opens the link has to read it
  without pause, and expect stale key events as it opens.
- **Roland's engine sounds the keys by itself.** With its app stopped the BMC plays its tone from
  the keys, so Oscairn playing from them sounds together with it. Roland's Local Switch system
  parameter is the likely way to turn that off (read from the app, not tried); it would take a
  write to the link, which nothing in Oscairn does.
- **The panel is silent.** A plain read-only `read()` of `/dev/i2c-slave-2` returns 0 bytes at
  once (end of file), and moving a slider, the lever and a button raised no interrupt on the
  panel bus, where with Roland's app running a slider raises about 200 a second
  ([Panel, buttons and lights](#panel-buttons-and-lights)). Inferred, not confirmed: the bus
  receives only while a reader holds the device the way Roland's programs do
  ([Panel messages](#panel-messages)), and perhaps only after the host's start words (the two
  kind-0 words to 0x75 that Roland's app sends,
  [What the app does that the hardware depends on](#what-the-app-does-that-the-hardware-depends-on)).
- **The audio link runs without Roland's app.** `aplay -D hw:1,0` played a 6-channel, S32_LE,
  48 kHz file to its end, and the link's DMA interrupts kept their rate throughout: the BMCs
  clock the link by themselves.
- **Audio from Linux is heard by default, on every pair tried.** A quiet sine (about -30 dBFS)
  on channels 2 and 3, then 4 and 5, then 0 and 1, was heard in the headphones each time, with
  no routing or level command sent. Not measured: the pairs' levels against each other, which
  channel of a pair is left, MAIN OUT apart from the headphones, SUB OUT, and opening all 16
  channels.
- **Oscairn's engine played through it.** From the chroot bundle, `oscairn-engine
  --output-device hw:1,0 --device-channels 6 --sample-rate 48000 --audio-outputs 0,1
  --buffer-frames 128` opened the device (JUCE named it "nocodec ... Direct hardware device
  without any conversions") at 48,000 Hz with a 128-sample buffer; five notes sent with
  `notes.send` on its control socket were heard from the startup synth; `deadlineMisses` and
  `audioUnderruns` were 0 before and after, `cpuLoad` about 0.23 to 0.25 with the startup
  synth, sequencer and delay, and about 416 MB of memory was still available.
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
- **Whether anything must be sent regularly** to keep the BMC side talking was not
  determined; the 58 interrupts a second the survey saw at rest are unexplained.

### Keys

- **Key events reach the Linux side** (the app, and seen on the unit:
  [With Roland's app stopped](#with-rolands-app-stopped)): short frames of kind 3 from 0x02 to
  0x63 or 0x64 (on the unit, 0x61) holding a MIDI word, note on (byte 2 the note, byte 3 the velocity) or note off;
  control changes 64 (hold) and 120 (all sounds off) from the same address are taken the same
  way. The app's "Kbd Task" thread is what takes them. A second form, a panel word of kind 8
  (on) or 7 (off) from 0x02 with a 7-bit note and a 10-bit value, is recognized only by the
  app's debugging; the unit sends it with every key, with the same velocity.
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
  again. Inferred: the `ioctl` is the driver's wait for a message. **Oscairn's reader does not
  do that**: it opens read-only and non-blocking, polls, and reads up to 256 bytes. Whether
  the driver serves it that way is unknown, and is the first thing the dump on a unit shows.
- **Other kinds exist** (the app): kind 0 words go to the panel as commands, kinds 0x14 and
  0x15 are analog words the app also passes to the sound engine, and 0x0F another the app
  handles itself. What sends them, and when, was not read.

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
  the VALUE dial, MASTER VOLUME and every other button as fixed functions; the keys and pads as
  notes; the HOLD jack as sustain and CTRL1 and CTRL2 as expression pedals, unassigned. The
  profile's own notes on two of these are behind what Roland's programs show: the pads come
  over the panel bus ([The map](#the-map)), and MASTER VOLUME never reaches the i.MX as a panel message, so its
  `volume` function has nothing to drive it.
- **The audio device** (the profile's `audio.device`, [RFC 0024](rfcs/0024-device-profiles.md#what-a-profile-holds)):
  `{ "name": "hw:1,0", "channels": 6, "rate": 48000, "outputs": { "main": [0, 1] } }`, what
  played on the unit: the engine opens `hw:1,0` with six channels, as Roland's app does, at
  48 kHz (refusing to start at another rate), plays main left and right on channels 0 and 1 and
  keeps the other four silent. JUCE, which opens it, takes S32_LE when the device offers no
  float format. Heard on the unit: the device, the rate, the six channels and main on 0 and 1
  ([With Roland's app stopped](#with-rolands-app-stopped)). Not known yet: whether 0 and 1 are
  MAIN OUT or only the headphones, which of the pairs 0 and 1, 2 and 3, 4 and 5 is which output,
  and SUB OUT; no inputs are mapped until capture is checked. To try another pair, copy the
  profile, change `main` (for example to `[2, 3]`), start the app with `--profile` and the copy's
  path (the built-in one is compiled in), and listen at a low level; or run `oscairn-engine` with
  `--output-device hw:1,0 --device-channels 6 --sample-rate 48000 --audio-outputs 2,3`; the SUB
  OUT pair, once found, goes in as `"sub": [n, n + 1]`.
- **The screen:** the profile gives its diagonal (140 mm, 5.5"), so the UI scales by its density
  ([RFC 0019](rfcs/0019-adaptive-ui.md#screens-of-any-size)): 10.5 pixels per millimeter make a
  scale of 1.55, and `kick-screen` is laid out at the screen's own 826 × 465 design units and
  fills it, with no bars. A touch target is 68 pixels, 6.5 mm, as on the KICK. `desktop-gaps`
  renders Home, Play, the Parts views, an editor, the Library, the Catalog and Settings at
  1280 × 720 and checks every control fits, is that large and overlaps none. Untried on the
  instrument.
- **A panel reader** in the engine, built only with `OSCAIRN_PANEL_ROLAND` (below).
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
directly: stop Roland's app before anything opens `hw:1,0` or the panel. Start the engine with:

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
`signed`; or, where a number's bits are not the low bits of each byte, a list of `parts`, each
`bits` bits of a `byte` from bit `shift` up, joined most significant first. `analogId` is an
analog message's ID where it is not where `id` is. `kinds` gives the kind field's value for each
of the four kinds. `controls` lists each
control's `kind` (`encoder`, `analog`, or `switch` for both switch kinds), `id` and `control`:
an identity above, or a fixed control's function from the profile (`enter`, `play`). An analog
value is divided by `max` (or the layout's `analogMax`) for 0 to 1; an encoder's delta is
multiplied by `step` (or `encoderStep`).

The shipped map's layout and IDs are **read from Roland's programs, not yet seen on a unit**:
the layout from the factory test tool (`qctest`), which prints each message's kind, number and
value, and from the app (`mi078`), which reads the same bits; the IDs from the test tool's
tables of names, which the app's own table of switch numbers matches entry for entry. Its
`estimated` list is empty: no part of it is a guess any more, and the first run of
`oscairn-panel-dump` on the unit is what confirms it. The layout:

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
Oscairn's reading, to check with the dump. Still Oscairn's own choice: `analogMax`, 1023. The
value field holds 0 to 4095; Roland's pedal adjustment checks a reading against 500 to 1024,
which points at a 10-bit scale, but no top value has been seen.

Not in the map, with Roland's numbers for them:

- **The pads:** switches 88 to 103, on and off with no velocity, in the order pad 2, 3, 4, 1, 5,
  8, 6, 7, 11, 10, 9, 12, 16, 13, 15, 14. The reader has nothing to play a pad with yet.
- **The HOLD jack:** analog 2, a position and not a switch, so a half-damper pedal reads as one.
  The controller layer's footswitch takes a switch; nothing turns the position into one yet.
- **The CTRL jacks' plug sensing:** switches 35 and 36 (CTRL1 and CTRL2, Roland's "sense") and
  37 and 38 (their ring contacts). Roland's app passes a CTRL jack's position on only while its
  sense switch is on.
- **MASTER VOLUME:** Roland's tables have no number for it. It never reaches the i.MX as a panel
  message (inferred: it acts on the BMC side or in the analog path).
- **WRITE, MENU and TEMPO:** numbers 104 to 106 in Roland's tables and in the map, but the panel
  bus never carries them: they are GPIO keys, and Roland's programs make the messages
  themselves from `/dev/input/event1`.
- Switches 39, 51, 63, 81, 84 and 85 are unused; Roland's app drops them.

### What it does not do yet

- **Pads and the HOLD jack:** on the panel bus, decoded and shown by the dump, played by
  nothing ([The map](#the-map)).
- **Lamps:** none is set; they go over `/dev/ttyGS0` ([Lamps](#lamps)), which Oscairn only reads.
- **WRITE, MENU and TEMPO:** GPIO keys on `/dev/input/event1`, not panel messages; nothing reads
  them yet.
- **Fixed controls** (the VALUE dial, cursor, ENTER, EXIT, transport, MENU and the rest) decode
  and show in the dump, but the engine has nothing to give them to: the controller layer plays
  assignable controls only.
- **The lever and pedals with nothing assigned:** a MIDI controller's wheel then reaches the
  parts as its own MIDI message; a panel message has none, so `wheel:1` and `wheel:2` play only
  what a Performance or Sound assigns them.

## The key reader

`oscairn::host::BmcLinkReader` ([hosts/audio/BmcLink.h](../hosts/audio/BmcLink.h)), built only
with `OSCAIRN_PANEL_ROLAND`, reads `/dev/ttyGS0` on a thread of its own, polling with a 100 ms
timeout. **It never writes to the device:** it opens it read-only and, before reading, makes it
raw with echo and flow control off, and reads the settings back; where they do not hold it does
not read (with echo on, the tty layer would send each byte received back to the BMC side). Once
open it reads without pause, whatever the engine is doing and whether or not the input is
ignored, since what it does not read waits and Roland's engine then holds its notes. If the
device fails or ends it is opened again a second later.

- **Frames:** each key's MIDI word (kind 3) is one note, on or off with its velocity, the
  release velocity included. The panel word sent with it (kind 6 from 0x02) is the same key
  again and is passed over, and so are the sound engine's status frames (kind 9 from 0xE1).
  Kind-3 MIDI words from 0x81, 0xA1, 0xB2, 0xC8 and 0xD8, the addresses Roland's app takes MIDI
  from its MIDI ports on, are read the same way: notes, controllers, pitch bend, pressure and
  program changes. Anything else is counted and logged with its header bytes, at most ten a
  second.
- **Inputs:** the keys are a MIDI input named `FANTOM keys`, and each of the five addresses one
  named by its address (`FANTOM MIDI 0x81`) until it is known which port is which. They are
  listed in `session.status` and `controllers.inputs` after the devices' inputs, whether or not
  anything has arrived, each with a source number kept while the engine runs, and play like any
  other input: parts' device rules, zones, controller profiles, ignoring and learn. The channel
  is the word's own: the keys play on channel 2 (status 0x91), and a part takes them as it takes
  any input's channels.
- **Finding its place:** the link has no delimiter or checksum. The reader takes a position as a
  frame's start only where a frame it knows begins (a MIDI word whose code index agrees with its
  status, a panel word, a status frame no longer than 4 KiB) and the next frame's header after
  it is plausible too; then it follows the headers, and a header that is not (a kind of 0, more
  than seven payload bytes, a long frame over 1 MiB) has it search again from the next byte.
  Started at each of the first 2,048 byte offsets of the owner's capture (and, once, at every
  offset), it found every key event whose frame starts after the offset and nothing else, never
  skipping more than 127 bytes.
- **What was queued:** for the first half second after each open, and until a read leaves
  nothing waiting, MIDI words are dropped (counted and logged once), so stale notes do not all
  sound at once. When the device fails or ends, each note it passed on and has not seen end
  gets a note off, and a held sustain pedal a release, so nothing is left held across a reopen.

The profile's `keyboard` says which reader and which device:

```json
"keyboard": { "protocol": "roland-bmc-stock", "device": "/dev/ttyGS0" }
```

The protocol names the firmware on the other side: `roland-bmc-stock` is the link as Roland's
stock BMC firmware speaks it, so other firmware on the BMCs would be another value (and the
panel's `roland-i2c-slave` names what Roland's stock panel firmware sends). The app passes the
device to the engine it starts (`--keyboard-device`, which the app also takes on its own command
line). As with the panel, the engine reads it with `--audio null` too: it is a device named on
purpose, not one found by listing. A build without `OSCAIRN_PANEL_ROLAND` leaves the reader out
and says so when a profile has a keyboard. Tested on frames built in the test and on a pty in
place of the device (`engine-bmc-link`), and its parser on the owner's capture; it has not run on
the unit.

## Checking the panel map on a unit

On the FANTOM, with Oscairn's engine not reading the panel (Roland's app stopped too, with
`systemctl stop weston-term`, so only one program reads the device):

```sh
oscairn-panel-dump /dev/i2c-slave-2 /usr/share/oscairn/panels/fantom-07.json
```

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
3. **What is not in the map** should show with `-`: each pad as a switch from 88 to 103, a
   pedal in HOLD as analog 2, and a plug going into CTRL1 as switch 35. WRITE, MENU and TEMPO
   should show nothing at all.
4. **Check:** run the app with `--panel-map` pointing at the map (or a corrected copy) and move
   the sliders: the part levels follow.

Send back the dump's tables, and a corrected map if anything differed.

## What needs the owner at the instrument

Each of these needs someone at the panel, Roland's app stopped, or both; in this order, cheapest
first. Each says what [Roland's programs](#what-rolands-programs-show) lead one to expect, and
what result would confirm or refute it. The first four leave Roland's app running and only read
`/proc`: run each watch over SSH while someone at the instrument does the one thing named, and
compare the counts before and after.

```sh
watch -n1 'grep -E "30a40000|32e50000|32e40000|serial|Write|Menu|Tempo|gt1151" /proc/interrupts'
```

1. **Where the keys and controls enter.** Play a run of notes, then hold a chord, while
   watching. The panel bus is the `30a40000.i2c` line; the message link `32e50000.usb` (58 a
   second at rest). Expected: notes raise the `usb` line's rate and leave the `i2c` line alone
   (keys come from the BMC side). Notes that raise the `i2c` line, or neither, refute that. Then
   the pads, the lever, WHEEL1 and WHEEL2, and a pedal on HOLD and on CTRL1: expected, each
   raises the `i2c` line (it is a panel word) and the `usb` line too (Roland's app passes it on
   to the sound engine), as a slider was measured to on November 12
   ([Panel, buttons and lights](#panel-buttons-and-lights)).
2. **The GPIO keys.** Press WRITE, MENU and TEMPO: each should count on its own line (`Write`,
   `Menu`, `Tempo`) and not on the `i2c` line. Any other button that counts there is a GPIO key
   too.
3. **DIN MIDI.** Send notes into MIDI IN from another instrument while watching
   `cat /proc/tty/driver/IMX-uart` (each UART's `rx:` count) and the `serial` lines. Expected:
   no UART count moves (DIN MIDI is the BMC side's) and the `usb` line's rate rises (the BMC
   tells Roland's app of them). A UART count that moves refutes the first; a `usb` line that
   stays at rest refutes the second.
4. **USB COMPUTER.** Plug a computer into USB COMPUTER, watch `dmesg -w` and the `32e50000`
   and `32e40000` lines on the unit, and list USB devices on the computer. Whatever the computer
   enumerates (its vendor, product and interfaces) is the BMC's if the unit's Linux saw nothing.
5. **Stop Roland's app** (the rest needs it stopped): `systemctl stop weston-term`, then check
   that `/proc/asound/card1/pcm0p/sub0/status` and `pcm0c/sub0/status` say `closed` and that
   no process holds `/dev/i2c-slave-2` (`ls -l /proc/*/fd 2>/dev/null | grep i2c-slave`).
   Only a power cycle brings it back: `systemctl start weston-term` after a stop showed
   "program version error" on the unit ([With Roland's app stopped](#with-rolands-app-stopped)).
   Done once on November 12: the keys still sound Roland's tone with the app stopped. Still to
   note with it stopped:
   - **Move the lever while holding a key.** Expected, if the lever comes over I2C as step 1
     showed: the pitch no longer bends, because the app carried the lever to the sound engine.
     A bend then refutes that the app is the only path.
   - **Look at the lamps.** Expected: they stay as they were, and pressing buttons changes none.
6. **The panel's layout and IDs:** [Checking the panel map on a unit](#checking-the-panel-map-on-a-unit),
   with `oscairn-panel-dump` in the bundle. Expected now to show nothing: on November 12 a plain
   read of the device ended at once and the panel bus stayed quiet with Roland's app stopped
   ([With Roland's app stopped](#with-rolands-app-stopped)), so the dump's log will say the
   device ended and is being opened again. The reader has to read the device as Roland does, and
   the panel may need the host's start words first, before the map can be checked.
7. **Oscairn's key reader.** The format is confirmed (November 12, with Roland's app stopped:
   [With Roland's app stopped](#with-rolands-app-stopped)); what is left is Oscairn reading it.
   With nothing else reading `/dev/ttyGS0`, start the engine in the bundle with
   `--keyboard-device /dev/ttyGS0` (the app passes it from the profile). Expected: no line saying
   the device cannot be made raw; `session.status` lists `FANTOM keys` in `midiInputs`; a key
   played after the first second plays the startup Synth part, softly and loudly as played,
   under Roland's own tone. Keys held while the engine starts must not sound at once (the reader
   drops what was queued, and logs how many). Lines saying `unknown frame:` are frames not seen
   in the capture: send them back. Then send notes into MIDI IN from another instrument:
   whichever `FANTOM MIDI 0x..` input plays names the DIN socket's address.
8. **Capture:** the link runs without Roland's app (November 12: playback ran to the end,
   [With Roland's app stopped](#with-rolands-app-stopped)); what comes back is not checked.
   `arecord -D hw:1,0 -c 6 -f S32_LE -r 48000 -d 2 /tmp/link.wav`, and look at the file for
   which of the six channels carry signal while someone plays a key, and with a source on
   MIC/LINE IN. Roland's app samples from its first two inputs, taken to be slots 0 and 1, so sound
   there fits; sound on another pair only says which pair that is; silence everywhere says
   only that the BMC feeds its sampling bus when told to. Remove the file afterward.
9. **Which pair is which output.** Settled on November 12: audio from Linux is heard without
   Roland's app on the pairs 0 and 1, 2 and 3, and 4 and 5, and Oscairn's engine plays through
   `hw:1,0` ([With Roland's app stopped](#with-rolands-app-stopped)). Still to note, at low
   volume, one pair at a time (`--audio-outputs 0,1`, then `2,3`, then `4,5`): which reaches
   MAIN OUT, SUB OUT and the headphones, which channel of each is left, and whether their levels
   differ; and whether `--device-channels 16` plays as 6 does.
10. **A lamp** (the one step that sends the BMC side anything; skip it if in doubt). With
    Roland's app stopped:

    ```sh
    printf '\014\140\165\000\205\064\010\177\000\000\000\004' > /dev/ttyGS0
    ```

    is one frame as Roland's programs build it: a panel word to address 0x75 setting lamp 52
    (ENTER) to mode 1, level 127. ENTER lighting confirms the lamps' path and word; nothing
    happening leaves both open (the mode or level may be wrong, or the BMC may want more first).
11. **The touch screen's range:** `evtest /dev/input/event3` prints each axis's minimum and
    maximum before the first event (Ctrl-C after it); touch the four corners to see which way the
    axes run on the landscape screen. Neither the unit nor the bundle has `evtest`: bring a
    static aarch64 build.
12. **A USB MIDI keyboard on USB MEMORY:** plug it in, `modprobe snd-usb-audio` (on the unit,
    outside the chroot: the module is Roland's), and check `cat /proc/asound/cards` and
    `amidi -l`. A raw MIDI device there confirms the kernel side. Then start the engine in the
    chroot on a real audio device (`--audio null` opens no MIDI device). It should log
    `no ALSA sequencer; MIDI inputs are read as ALSA raw MIDI` (after ALSA's own line that
    `/dev/snd/seq` failed to open) and `1 MIDI inputs open (ALSA raw MIDI)` (one per input port
    the keyboard has), and `session.status` should list the keyboard in `midiInputs` by the
    name `amidi -l` gives its port. Play it; then pull it out and plug it back in while the engine runs: within a second
    or two the log says `0 MIDI inputs open (ALSA raw MIDI)`, then `1` again, and it plays again.
13. **The display from the chroot:** with Weston running and Roland's app stopped, start the app
    in the bundle with `DISPLAY=:0` and see whether it reaches Xwayland.

## Building it into an image

`-DOSCAIRN_PANEL_ROLAND=ON` links the reader into the engine (and the app's panel handling) and
installs `oscairn-panel-dump` and the map in `share/oscairn/panels/`. Buildroot passes it
when a defconfig sets `BR2_PACKAGE_OSCAIRN_PANEL_ROLAND`, as the chroot bundle's does.
