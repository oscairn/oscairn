# Oscairn OS images

Linux images that boot straight into Oscairn, built with
[Buildroot](https://buildroot.org) 2026.08.1 as an external tree (`os/buildroot`), so
Buildroot itself stays unmodified and easy to upgrade.

| Image | Defconfig | Status |
| --- | --- | --- |
| QEMU, generic ARM64 machine | `oscairn_qemu_aarch64_defconfig` | Builds and boots; Oscairn starts and plays |
| The same, real-time test variant | `oscairn_qemu_aarch64_rt_defconfig` | See [real-time kernel](#real-time-kernel) |
| NXP i.MX8M Mini EVK (`imx8mm-evk`), the KICK's SoC | `oscairn_imx8mm_evk_defconfig` | Builds; never booted (no board here). See [i.MX8M Mini EVK](#imx8m-mini-evk) |

QEMU has no model of the RK3588 or the KICK's i.MX8M Mini, so the QEMU image uses the
generic `virt` machine with a Cortex-A76 CPU (code is built for the RK3588's
Cortex-A76/A55). It tests the toolchain, packaging and startup, not board-specific parts
(display controller, audio codec, panel).

The QEMU image does not suit the KICK: Cortex-A76/A55 code is ARMv8.2, and the KICK's
Cortex-A53 is ARMv8.0, so its binaries may stop with an illegal instruction there. The EVK
image is built for the Cortex-A53. See [running on a KICK](../docs/kick.md).

## What is in the image

- GCC 15, glibc, Linux 6.18 (LTS) with a low-latency (`PREEMPT`) kernel, BusyBox init, eudev.
  Each image's kernel has only its own board's drivers: see [kernels](#kernels).
- Oscairn: the core, the CLAP plugins in `/usr/lib/clap`, `oscairn-render`, and the
  desktop app as `/usr/bin/Oscairn`.
- The session (`board/common/session-juce-x11`): at boot, `/etc/init.d/S95oscairn` starts
  `/usr/libexec/oscairn-session`, which raises the locked-memory limit (`RLIMIT_MEMLOCK`) to
  unlimited for the engine and its workers, starts an X server and then Oscairn full screen,
  and restarts Oscairn if it exits. Its arguments are in `/etc/default/oscairn`
  (`--profile qemu-aarch64 --play`: the image's [device profile](../docs/rfcs/0024-device-profiles.md),
  which fills the screen with the KICK's screen layout, starting at Home, and asks for
  128-frame audio blocks). Logs: `/var/log/oscairn.log` and
  `/var/log/oscairn-xorg.log`.
- HTTPS for package repositories: Mbed TLS, libcurl built against it, `ca-certificates`, and
  the `curl` tool for development ([RFC 0039](../docs/rfcs/0039-network-security.md#progress)).
- Sound through ALSA. The QEMU image uses the kernel's dummy sound card, which plays
  silently. Audio requests a 128-frame buffer, through the
  same engine as the desktop app; see [audio buffer configuration](../hosts/README.md#audio-buffer-size).
  The backend chooses a supported size if 128 is unavailable; check the startup log
  for the actual size. Reliability at 128 frames still needs validation on real hardware.

## Changing the UI later

The JUCE app needs X11, and that is confined to two places:

- the `BR2_PACKAGE_OSCAIRN_STANDALONE` option (the JUCE app and its X11, ALSA and font
  dependencies), and
- the `board/common/session-juce-x11` overlay with `board/common/post-build.sh` (the
  session that starts X and the app).

A device UI built another way (for example Qt or Slint drawing directly to the display
through DRM/KMS) is a new package and a new session overlay; the core, plugins, kernel and
the rest of the image do not change. Once the UI is its own process talking to the core
through the control API, the core will run as a separate service that does not depend on
any UI toolkit.

## Building

Needs Docker. Buildroot runs in a Debian container (`os/docker`), and its work (Buildroot
itself, downloads, compiler cache and output) lives in the Docker volume `oscairn-os`,
because Buildroot needs a case-sensitive Linux file system.

```sh
os/build.sh                       # build oscairn_qemu_aarch64_defconfig
os/build.sh test                  # boot it in QEMU and check that Oscairn starts
```

A clean build took 12 minutes on a 16-core Mac (Docker with 8 GB of memory) with the
downloads and the compiler cache already in place; the first build, which also downloads
everything and fills the cache, takes longer (not timed). The image builds from
the working tree: committed files and new files git does not ignore.

`os/build.sh test` boots the image (4 cores, 2 GB, a 1024 x 600 display) and passes when
the session reports Oscairn running, the engine reports the audio device it opened, and a
screenshot shows a picture (Home). Whether sound actually plays is what `os/build.sh record`
checks.
Results: `build-os/test` (console log and screenshots).

To hear what the image plays, `os/build.sh record [SECONDS]` boots it with a virtio sound
card and QEMU writes everything Oscairn plays to `build-os/record/oscairn.wav` (44.1 kHz,
the rate the app opens the card at). `os/scripts/compare-audio.py` checks the recording
against an offline render of the same pattern:

```sh
os/build.sh record 20
build/hosts/oscairn-render build-os/record/reference.wav --seconds 40 --rate 44100 \
    --map 0.0=1.Brightness build/plugins/oscairn-step-sequencer.clap \
    build/plugins/oscairn-basic-synth.clap build/plugins/oscairn-delay.clap
python3 os/scripts/compare-audio.py build-os/record/oscairn.wav build-os/record/reference.wav
```

QEMU records the stream the VM sends, so this checks the audio itself (every note in place,
nothing dropped or repeated), not whether the VM kept up in real time.

Other Buildroot targets go after the defconfig, for example
`os/build.sh oscairn_qemu_aarch64_defconfig oscairn-rebuild all`.

## Kernels

Each board's kernel config lists only what that board's hardware needs, so an image carries no
drivers for hardware it does not have ([RFC 0035](../docs/rfcs/0035-system-base.md#optional-hardware-built-per-product)):

- `board/<board>/linux.config`: the board's drivers, as a defconfig over the kernel's defaults
  (options not named keep their default), so it lists only what differs. Buildroot uses it as
  the custom config (`BR2_LINUX_KERNEL_USE_CUSTOM_CONFIG`).
- `board/common/linux.fragment`, added to every board's: what the session and engine need anywhere
  (the low-latency kernel, `NO_HZ_FULL` and cpusets for the audio cores, ext4, devtmpfs, local
  and IP sockets, System V shared memory for X, evdev, ALSA and its sequencer), and a few defaults
  turned off that no board needs (PC keyboards and mice, USB HID, EFI boot).
- The `oscairn` package's options take out what the image's profile lacks: without
  `BR2_PACKAGE_OSCAIRN_STANDALONE` (the app, the only user of a screen) the kernel has no DRM or
  framebuffer, and without `BR2_PACKAGE_OSCAIRN_USB_GADGET` no USB gadget, whatever the board's
  config lists. Their drivers depend on those options, so none is left behind.

To change a board's kernel, edit its `linux.config` and rebuild with
`os/build.sh <defconfig> linux-dirclean all`; check a new option landed in the build's `.config`
(an option whose dependencies are missing is dropped without a word; `scripts/kconfig/merge_config.sh`
in the kernel tree names them). `linux-update-defconfig` would rewrite the file without its
comments.

Measured on 2026-11-08, before (the EVK on the arm64 `defconfig`, QEMU on Buildroot's
`aarch64-virt` config) and after:

| Image | Kernel options set | `Image` | Modules | Target folder |
| --- | --- | --- | --- | --- |
| EVK, before | 4,569 | 39.5 MiB | 1,424 (79 MiB) | 239.0 MB |
| EVK, after | 984 | 11.1 MiB | 30 (2.0 MiB) | 133.4 MB |
| QEMU, before | 1,103 | 12.6 MiB | 5 | 103.2 MB |
| QEMU, after | 869 | 9.7 MiB | 2 | 102.8 MB |

The real-time QEMU variant's `Image` went from 12.6 to 9.8 MiB (879 options). The target folder includes the EVK's copy of `Image` in `/boot`. The QEMU image still passes
`os/build.sh test`. Not in either kernel: Wi-Fi, Bluetooth, touch screens, cameras, batteries,
USB storage and FAT or exFAT (RFC 0037 adds them per product), netfilter (RFC 0039).

## Real-time kernel

`BR2_PACKAGE_OSCAIRN_REALTIME_KERNEL` (in the `oscairn` package's options) builds the kernel with
`PREEMPT_RT`: it sets `CONFIG_EXPERT` and `CONFIG_PREEMPT_RT` through the package's kernel
config fixups, so any product defconfig turns it on with one line. Linux 6.18 has it in mainline
on ARM64; no patch is needed ([RFC 0035](../docs/rfcs/0035-system-base.md#the-kernel)).
`oscairn_qemu_aarch64_rt_defconfig` is the QEMU image with it and `rt-tests` (`cyclictest`,
`hackbench`), a test variant; the product image stays as it is until a board has been measured.

```sh
os/build.sh oscairn_qemu_aarch64_rt_defconfig
# in the build container, as os/build.sh runs it: boot it and run cyclictest for 60 s
python3 /src/os/scripts/cyclictest-qemu.py /work/output/oscairn_qemu_aarch64_rt_defconfig/images /out/cyclictest 60
```

Built and booted on 2026-11-07: the kernel reports `SMP PREEMPT_RT`, `boot-test.py` passes on
it (Oscairn running, audio opened, Home on screen), and a 60 s `cyclictest` run beside the
playing app reported maxima of 3.5 to 4.5 ms per core. QEMU's figures mean nothing for
hardware (its guest cores are host threads); `compare-audio.py` was not run on it. The
procedure for a device is in [running on a KICK](../docs/kick.md#measuring-latency).

## i.MX8M Mini EVK

`oscairn_imx8mm_evk_defconfig` is the first image for real hardware: NXP's i.MX8M Mini EVK,
the reference board for the SoC of the KICK and the Ignite. **It has been built, not booted:**
there is no EVK here, and QEMU has no i.MX8M Mini model.

- **Boot chain:** Buildroot's own `imx8mm-evk_defconfig`, unchanged: TF-A v2.12, U-Boot 2025.01
  (`imx8mm_evk`) with SPL and the LPDDR4 training firmware from `firmware-imx`, packed into
  `flash.bin`; `board/freescale/imx8mm-evk/post-build.sh` writes `/boot/extlinux/extlinux.conf`
  and the `genimage` layout (GPT, `flash.bin` at 33 KiB outside the partition table, the root
  filesystem as the only partition at 8 MiB). U-Boot boots the kernel and
  `freescale/imx8mm-evk.dtb` from `/boot` on the root filesystem.
- **Kernel:** Linux 6.18.7, as in the QEMU image, from `board/imx8mm-evk/linux.config` (see
  [kernels](#kernels)): the SoC's clocks, pins, GPIO, power domains, thermal sensor, watchdog,
  frequency scaling and idle states, the BD71847 PMIC, the UART2 console, the microSD and eMMC;
  the EVK's WM8524 codec on SAI3 through `simple-audio-card` (the device tree's `wm8524-audio`
  card), SDMA as a module so its firmware (`imx/sdma/sdma-imx7d.bin`, from `firmware-imx`) loads
  from the root filesystem, the ALSA sequencer; the USB-C port (ChipIdea, with its PTN5110 port
  controller switching roles) as a host for USB MIDI and audio devices and as a device with
  configfs MIDI and UAC2 functions; Ethernet (FEC with its AR8031 PHY); the HDMI path (LCDIF, the
  Samsung DSIM MIPI-DSI host and the ADV7535 bridge, with a framebuffer for X); and
  `CONFIG_PREEMPT`. Audio and display drivers are modules, for eudev to load from the device
  tree. Left out, since the profile does not use them: PCIe, the camera, the PDM microphones,
  S/PDIF, the Bluetooth SCO link, the IR receiver, the SPI NOR flash and the power key.
- **Userspace:** the QEMU image's, unchanged: the same packages, the `board/common` session
  overlay and post-build script. `board/imx8mm-evk/overlay` replaces only
  `/etc/default/oscairn`, to pass `--profile imx8mm-evk --play` (the
  [profile](../hosts/standalone/profiles/imx8mm-evk.json): every guessed section listed in
  `estimated`). Built for the Cortex-A53 (`BR2_cortex_a53`; the compiler gets
  `-mcpu=cortex-a53`), so ARMv8.0, the [RFC 0035](../docs/rfcs/0035-system-base.md) SDK baseline.
- **Display:** mainline's device tree routes the EVK's MIPI-DSI output to HDMI through an
  ADV7535 bridge (an accessory card on some boards). The drivers are in the image;
  whether X gets a picture on a monitor is untried, and so is whether X starts with no monitor.
  If it does not, the app restarts in a loop: the session has no headless mode yet.
- **Audio:** ALSA's default device is the first card. The device tree also describes S/PDIF, a
  PDM microphone card and a Bluetooth SCO card, but their drivers are not in the kernel, so the
  WM8524 should be the only card unless a USB audio device is plugged in (untried; `aplay -l` on
  the board says). Whether the engine opens it at 128 frames is untried.
- **Real-time kernel:** `BR2_PACKAGE_OSCAIRN_REALTIME_KERNEL=y` applies here as on QEMU. With it,
  this kernel config resolves to `CONFIG_PREEMPT_RT=y` (checked with `olddefconfig`); the
  real-time kernel was not built for the EVK.

```sh
os/build.sh oscairn_imx8mm_evk_defconfig
```

Built on 2026-11-08 on a 16-core Mac (Docker with 8 GB of memory) in about 37 minutes of build
time over three runs (the first stopped at a Linux-only compile error in the app, since fixed),
with the QEMU builds' downloads and compiler cache already in the volume; the toolchain was
rebuilt for the Cortex-A53. Outputs, in `/work/output/oscairn_imx8mm_evk_defconfig/images` in
the volume: `sdcard.img` (520 MiB), `flash.bin` (1.1 MiB), `Image` (40 MiB), `freescale/imx8mm-evk.dtb`,
`rootfs.ext4` (512 MiB, 159 MiB used, 79 MiB of it kernel modules). With the board's own kernel
config (2026-11-08, [kernels](#kernels)): `Image` 11.1 MiB, 2.0 MiB of modules, and 147.7 MiB of
`rootfs.ext4` in use by `dumpe2fs`'s count (blocks less free blocks, the journal included; how
the 159 MiB was counted is not recorded, so the two are not compared).

Checked without hardware: `flash.bin` sits byte for byte at 33 KiB in `sdcard.img`, after the
GPT (which ends at 17 KiB), and the root filesystem at 8 MiB, with the partition UUID
`extlinux.conf` names as `root=`; the kernel config has the drivers above (checked again with the
board's own config); the Oscairn binaries
are aarch64 and, under `qemu-aarch64 -cpu cortex-a53` with the image's own libraries,
`oscairn-engine --help` runs and `oscairn-render` renders the step sequencer, Basic Synth and
Delay offline (4 s at 48 kHz, peak 0.45).

### Flashing and booting an EVK

1. Write the image to a microSD card (all its contents are lost; check the device name first):

   ```sh
   docker run --rm -v oscairn-os:/work -v "$PWD":/out debian:trixie-slim \
       cp /work/output/oscairn_imx8mm_evk_defconfig/images/sdcard.img /out/
   sudo dd if=sdcard.img of=/dev/<your-sd-device> bs=4M conv=fsync   # macOS: /dev/rdiskN
   ```

2. Set the boot switches to boot from the microSD card (Buildroot's
   `board/freescale/imx8mmevk/readme.txt`: SW1101 `0110110010`, SW1102 `0001101000`, D1 to D10;
   check them against NXP's quick start guide for your board revision).
3. Connect the micro-USB **debug** port to a computer. It shows two serial ports; the console
   (the SoC's UART2, `ttymxc1` in Linux) is the second, for example `/dev/ttyUSB1`. Open it at
   115200 baud, 8N1: `picocom -b 115200 /dev/ttyUSB1`.
4. Optionally connect an HDMI monitor and headphones to the audio jack, then power on.

**What to watch on the console:** U-Boot SPL and the DDR training, then U-Boot, which should
find `/boot/extlinux/extlinux.conf` on the card and load `Image` and the device tree; the kernel's
log; then the session's lines: `oscairn-session: locked memory: unlimited`,
`X server up` (or `X server did not start`), `Oscairn running (pid N)` and the engine's
`audio output` line naming the device it opened; and a login prompt (`root`, no password).
Report what stops where, and the output of `aplay -l`,
`dmesg | grep -i -E "sai|wm8524|sdma|dsim|adv7511|mxsfb"`, `/var/log/oscairn.log` and
`/var/log/oscairn-xorg.log`.

## Next

- A board image for an RK3588 development board (mainline kernel), with a real audio
  interface and a real-time kernel, and latency measurements.
- Booting the i.MX8M Mini EVK image on a board, then the KICK, once units exist: its kernel,
  display, audio codec, and a driver for the panel ([what exists today](../docs/kick.md)).
- Signed updates with A/B system images.

## Optional LV2 hosting

`BR2_PACKAGE_OSCAIRN_LV2` selects Lilv and enables the core's LV2 adapter. It requires a
dynamic-library toolchain. It is off in the existing QEMU defconfig; this change does not
add LV2 bundles to the image. Install native target builds of bundles separately. The
initial [host subset](../core/lv2-host/README.md) applies on devices too. The LV2-enabled
Buildroot image has not yet been rebuilt or boot-tested.
