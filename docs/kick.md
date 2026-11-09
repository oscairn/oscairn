# Running Oscairn on a KICK or a KICK emulator

Oscairn has not yet run on a KICK or on any i.MX8M Mini board. An image for NXP's i.MX8M Mini
EVK builds, but has never been booted. This page says what exists for the KICK today, how to try
it on an emulator or an EVK, and what to expect, as of November 8, 2026.

## What exists

- **A device profile for the KICK** (`--profile kick`, [RFC 0024](rfcs/0024-device-profiles.md)):
  [hosts/standalone/profiles/kick.json](../hosts/standalone/profiles/kick.json). Its values are
  **estimates** (cores, memory, audio, controls, MIDI, network), not measured on a unit. It makes
  the app fill the display with no title or menu bar, show the KICK's screen layout
  (`kick-screen`, 1024 × 600), request 128-frame audio blocks, and offer the Library, Setlist
  and Groovebox apps.
- **Two layouts:** `kick-screen`, the screen alone as the device shows it, and `kick`, the whole
  front panel drawn on a computer screen (with the geometry of chillbeast's KickEM simulator
  board), for trying the KICK's flow on a desktop.
- **A Linux image** ([os/](../os/README.md)) built with Buildroot for QEMU's generic ARM64
  machine. It boots straight into Oscairn full screen under X11. It is **not** built for the
  KICK's board.
- **An image for the i.MX8M Mini EVK** (`oscairn_imx8mm_evk_defconfig`,
  [os/](../os/README.md#imx8m-mini-evk)), NXP's reference board for the KICK's SoC: the same
  userspace on Buildroot's EVK boot chain (TF-A, U-Boot, `flash.bin` on the SD card), mainline
  Linux 6.18 with the EVK's device tree, its WM8524 codec and HDMI bridge drivers, built for the
  Cortex-A53, with its own profile (`--profile imx8mm-evk`, estimates). It **builds; it has not
  been booted**, so whether it starts, makes sound or shows a picture is unknown. It is not the
  KICK's own board: the KICK's display, codec, panel and device tree are still missing.

## What you see

The app starts at **Home** ([RFC 0030](rfcs/0030-system-shell-launcher-and-navigation.md)): a top
bar and the apps as tiles.

- **Performance:** plays and edits the open Performance. For now it opens the parts editor
  (parts, mixer, routing, controls); a live-controls view for playing comes later.
- **Library:** Performances and Sounds, by source and tags, with audition.
- **Setlist:** songs in order, for the stage.
- **Groovebox:** the step sequencer (two tracks, A/B patterns, per-step locks).

On the `kick-screen` layout (no buttons around the screen), each app's top bar has a Home icon.
On the `kick` layout, the panel's **MAIN** button goes Home, so the screen shows no Home icon;
**BACK** goes Home too for now (every app's top screen is one level down; going back within an
app is the app's own). The demo session plays the step
sequencer through the Basic Synth and the Delay.

## What does not work yet

- **The KICK's physical controls:** no driver reads its pads, encoders or buttons (its panel
  protocol is not implemented). On a device or an emulator, what works is what Linux sees: the
  touchscreen as a pointer, MIDI inputs ALSA lists (every one plays), and a mouse or keyboard. On a computer, the `kick` layout's
  drawn controls work with the mouse.
- **The KICK's audio codec, display and touch controller:** nothing is specific to them; the app
  uses what ALSA and X11 provide. It opens ALSA's default device; the profile asks for
  128-frame blocks, the device may choose another size (the log says which), and the profile's
  sample rate is not applied yet.
- **Live controls in the Performance app, the system shell's full top bar, quick settings and
  the running-apps switcher:** designed in RFC 0030, not built.
- **Measured CPU and memory budgets on the KICK:** the budget comes from the profile's estimates.

## Running it

### On your emulator's own Linux

If the emulator runs an ARM64 Linux with X11 and ALSA, build Oscairn there or cross-compile it,
then start the app with the KICK's profile:

```sh
cmake -S . -B build-app -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DOSCAIRN_STANDALONE=ON
ninja -C build-app oscairn_standalone
build-app/hosts/standalone/oscairn_standalone_artefacts/RelWithDebInfo/Oscairn --profile kick
```

The app starts its audio engine (`oscairn-engine`, beside it) and keeps it running if the UI
exits; see [launching](../hosts/README.md#launching). It loads the demo plugins from the build
tree, or from `--plugins DIR`. A native Linux build of the desktop app is untested; the image's
cross-compiled build is the tested Linux path.

### With the Oscairn image

`os/build.sh` builds the QEMU image (needs Docker; see [os/](../os/README.md)). To use the
KICK's profile, set `OSCAIRN_ARGS="--profile kick"` in `/etc/default/oscairn` (in the image's
overlay, `os/buildroot/board/common/session-juce-x11/etc/default/oscairn`, before building).

Two things to change for an emulator of the KICK's hardware, neither tried yet:

- **The CPU:** the QEMU image is built for Cortex-A76/A55 (`BR2_cortex_a76_a55` in
  `os/buildroot/configs/oscairn_qemu_aarch64_defconfig`), which is ARMv8.2. The KICK's Cortex-A53
  is ARMv8.0, so those binaries may stop with an illegal instruction there. The EVK image is
  built for the Cortex-A53 (`BR2_cortex_a53`); its binaries run under
  `qemu-aarch64 -cpu cortex-a53` (an offline render, checked).
- **The kernel:** the QEMU image's kernel is configured for QEMU's `virt` machine. An emulator
  that models the i.MX8M Mini needs a kernel and device tree for that SoC; the EVK image has the
  EVK's.

### On an i.MX8M Mini EVK

Build `os/build.sh oscairn_imx8mm_evk_defconfig`, write `sdcard.img` to a microSD card, set the
board to boot from it and watch the console on the micro-USB debug port (UART2, the second of
its two serial ports, 115200 baud): [flashing and booting](../os/README.md#flashing-and-booting-an-evk).
Nobody has done this yet; what the console shows, `aplay -l` and the logs below are what to
report.

## Measuring latency

For the owner to run on hardware ([RFC 0035](rfcs/0035-system-base.md#measuring), steps 1 and 2).
Nothing here has been run on a device yet.

**The image.** A product defconfig gets a real-time kernel with
`BR2_PACKAGE_OSCAIRN_REALTIME_KERNEL=y` (it turns on `CONFIG_EXPERT` and `CONFIG_PREEMPT_RT`;
mainline since Linux 6.12, no patch), and `cyclictest` with `BR2_PACKAGE_RT_TESTS=y`, which
belongs in a test image, not the product. `oscairn_qemu_aarch64_rt_defconfig` is the QEMU image
with both; a board image adds the same two lines. On a board running its own Linux, install
`rt-tests` from its distribution and check the kernel the same way.

**Check the kernel** first: `uname -v` shows `PREEMPT_RT` (as on the QEMU variant:
`#1 SMP PREEMPT_RT ...`). Without it, the numbers below measure the wrong kernel. The image's 6.18
kernel has no `/sys/kernel/realtime` to check instead (seen on the QEMU variant).

**Run**, as root, with Oscairn playing (the image starts it with `--play`) and a load beside it:

```sh
# load: scheduler and memory traffic, and storage writes, for the whole run
while true; do hackbench -g 4 -l 10000 > /dev/null; done &
while true; do dd if=/dev/zero of=/root/load bs=1M count=256 conv=fsync 2> /dev/null; done &
# one thread per core at SCHED_FIFO 80 (the engine's audio thread's priority), memory locked,
# 1 ms interval, a histogram to 400 µs; 10 minutes first, then the 12-hour run
cyclictest -m -S -p 80 -i 1000 -d 0 -h 400 -D 10m -q > cyclictest-10m.txt
cyclictest -m -S -p 80 -i 1000 -d 0 -h 400 -D 12h -q > cyclictest-12h.txt
kill %1 %2; rm -f /root/load
```

Run the same on the board's non-real-time kernel (`CONFIG_PREEMPT`, the option off) for the
baseline step 1 asks for. Watch the engine meanwhile: `session.status`'s `audioUnderruns` should
not move ([control API](control-protocol.md)).

**What to accept.** The figure is each thread's `Max:` line, in microseconds. RFC 0035's proposed
gate is a maximum below 100 µs over 12 hours under load, and lower than the non-real-time
kernel's on the same board; the figure is to be set from these first measurements. A maximum
over 100 µs is worth reporting with the histogram (which cores, how often); one in the
milliseconds means the real-time kernel is not in effect or a driver holds the CPU. QEMU's
figures (`os/scripts/cyclictest-qemu.py`) mean nothing for hardware: QEMU runs the guest's
cores as host threads.

**Report:** the board and image (`uname -a`, `/etc/os-release` or the image's commit), the
kernel's preemption (both checks above), the load, the duration, each thread's Min, Avg and Max,
the histogram files, `audioUnderruns` before and after, and the room's temperature if the board
throttles.

## What to report

The session logs to `/var/log/oscairn.log` (the app and engine) and `/var/log/oscairn-xorg.log`.
Useful to know: whether the app reaches Home, the audio block size it reports, whether touch
reaches the app, whether the demo plays without dropouts, and the `budget.estimate` the
[control API](control-protocol.md) gives for the open Performance.
