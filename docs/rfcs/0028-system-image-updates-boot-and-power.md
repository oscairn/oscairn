# RFC 0028: The system image: updates, boot, storage and power

Status: proposed, 2026-11-01. Required for every product Oscairn runs on as its own operating
system. Builds on the [Oscairn OS images](../../os/README.md) (Buildroot, an ARM64 QEMU image that
boots into Oscairn), device profiles ([RFC 0024](0024-device-profiles.md)) and reliability levels
([RFC 0027](0027-reliability-levels.md)).

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. A read-only system partition, with the user's data on its own (#203)
- [ ] 2. Signed A/B updates, falling back to the previous system if the new one does not start (#217 install methods and #220 user content: [the section below](#install-methods-and-user-content); #218, #264)
- [ ] 3. Factory reset that keeps or clears the user's data, as asked (#205)
- [ ] 4. Boot time measured and reduced to each product's target (#207)
- [ ] 5. Safe power loss: no corrupted data, checked by cutting power in tests (#209)
- [ ] 6. Power management for battery products (#210)

## Problem

The OS image boots into Oscairn and restarts it if it exits, and its next steps name signed A/B
updates. A product needs more: it must survive an update that goes wrong, a power cut in the
middle of saving, years of use without its system drifting, and, for a pedal, starting in a few
seconds. None of that is decided.

## Outcome

Every product runs the same kind of image: a read-only system that updates as a whole, the user's
data apart, a boot measured against the product's target, and power loss that loses at most what
was being saved.

## Partitions

- **System, A and B:** read-only, one running and one for the next update. Nothing writes to the
  running one.
- **User data:** Performances, Sounds, libraries, samples, settings, paired clients and
  measurements (RFC 0025): the data folder Oscairn already uses (`OSCAIRN_DATA_HOME`), on its own
  partition with a journaling file system.
- **Factory:** the factory library (RFC 0021) and the factory settings, read-only, for reset and
  reinstalling.

## Updates

- An update is a whole system image, signed by its publisher, written to the partition not
  running, then booted once on trial. If Oscairn does not reach a healthy state (the engine
  plays, the UI or web UI answers) the boot loader goes back to the previous system on its own.
- Never during a performance: offered only while nothing plays, and never applied on its own.
- Plugins and content are not part of the system image: they are packages (RFC 0006), updated
  apart, so a system update does not change a sound.
- From a USB stick, from a paired computer (RFC 0026), or downloaded when online.

## Factory reset

Restores the system to the factory partition's settings and reinstalls the factory library.
It asks whether to keep the user's data (the default) or clear it; clearing it is a second
confirmation.

## Boot

Each profile states a boot target: seconds to sound for a pedal, longer for a workstation.
The path to first sound: kernel, the engine and its audio, the last Performance; the UI and the
rest load after. Plugins are prepared in the order the Performance needs them; worker processes
(RFC 0010) start in parallel. Measured on each product, on every release.

The last Performance, built 2026-11-11: the engine remembers the one last opened or saved in the
data folder (`library/last.json`) and opens it when the app asks (`performance.reopen`), once the
app has set the budget (RFC 0025), so it is admitted as any open is; the app asks as its window
opens, off its message thread, and the window shows Untitled until it has opened (not with
`--snapshot` or `--play`, nor `--in-process`, whose engine opens no Performances). One that is gone, unreadable, over the budget or
needing a plugin that is not installed leaves Untitled open, and the app says why over the screen
until OK (a notice that went by itself after 8 s could be missed at a start). An engine still
running when the app comes back keeps what it has. It opens what was saved, not edits made since:
the desktop app asks before a quit would lose them ([RFC 0030](0030-system-shell-launcher-and-navigation.md)). Until a budget is set the engine knows none, so
it opens nothing before the app has connected; the boot ordering above (the last Performance before
the UI) waits for the engine to know its budget at start. Since 2026-11-24 an engine not
started by a desktop app opens the last one itself as it starts listening (the FANTOM with the
Rust UI), admitted by the budget its device profile set (none without one);
`--startup-performance` keeps the startup Performance for measurements and tests.

The image also sets up memory for the engine (proposed in
[RFC 0025](0025-resource-budgets.md#linux)): a cgroup v2 subtree delegated to it, a locked-memory
limit (`RLIMIT_MEMLOCK`) high enough for the engine and its workers, their out-of-memory
scores, and compressed swap (zram) that apps may use and the engine and workers may not.
What the engine reads, built (RFC 0025 step 10, in part), not yet wired in `os/`: the session
moves its own processes out of the subtree's root, enables the memory controller for it and
passes its path in `OSCAIRN_CGROUP` (the engine then moves itself into `engine/` and puts each
worker in `workers/<pid>`); the app passes the profile's reliability level in
`OSCAIRN_RELIABILITY`, at which the engine and workers lock their memory and set their
out-of-memory scores. `oscairn-session` sets `RLIMIT_MEMLOCK` to unlimited before it starts the
app, so the engine and its workers inherit it, and reports it on the console, which the boot
test checks; it would also make the subtree, not wired yet. zram is not set up.

## Power loss

Saving is already atomic (written beside, then renamed into place); the data partition's
journal and syncing after each save make it survive a cut. Tests cut power during saves,
installs and updates, and check that what was saved before is intact and that nothing half
written is shown.

## Power management

For battery products (RFC 0024's `power`): plugins that sleep (CLAP sleep) and Performances at
rest let the cores slow down; the screen dims; a warning before the battery runs out leaves time
to save. Never at the cost of a missed audio deadline: the cores reserved for audio keep their
speed while anything sounds.

## Install methods and user content

Added 2026-11-27 for #217 and #220. What exists or is decided is stated; the rest is marked open.

### What a profile says

A device profile (RFC 0024) has an `install` block with four fields, all required when the block
is there; a profile without it states nothing (`hosts/standalone/Profile.h`, `Profile::Install`):

| Field | Values | Meaning |
| --- | --- | --- |
| `method` | `desktop`, `app-and-boot-hook`, `image` | How Oscairn gets onto the device |
| `update` | `none`, `bundle-swap`, `ab` | How it is updated |
| `systems` | `1`, `2` | How many systems the device holds at once; `ab` needs 2 |
| `userData` | `with-system`, `separate` | Whether the user's content lives inside what an update replaces, or apart from it |

- **`desktop`:** the app on a computer, installed by the user (an installer is #292, not built).
  The data folder is the platform's, outside the app: `separate`. Oscairn does not update
  itself: `none`. The desktop profile says `desktop`, `none`, 1, `separate`.
- **`app-and-boot-hook`:** an app with a userland of its own that runs beside the product's own
  system, started by a line the owner adds to the product's start script, ahead of its own. The
  FANTOM's, the first and so far the only one: its profile says `app-and-boot-hook`,
  `bundle-swap`, 2, `with-system`. Its install and update, as they are done today, are in
  [docs/fantom-0.md](../fantom-0.md#installing-and-updating-oscairn).
- **`image`:** a whole system image written to the device, which is the product's system
  (the images in `os/`). Not yet in a profile: the QEMU and EVK images exist, but nothing
  updates them and their data folder is on the one root file system, so their profiles will say
  `image`, `none`, 1, `with-system` when a step needs them to.
- **`bundle-swap`:** a new system unpacked beside the running one, switched to, and the old one
  deleted once the new one is confirmed. It needs room for two, which is what `systems` states.
  Not atomic and not signed: the link is switched after the new one ran under a timer, and the
  steps are by hand.
- **`ab`:** the signed A/B update above (Updates). Decided, not built (#218); it waits on the
  package format and signing (#264), which are the owner's.

Open: a device installed through debug pins or another flashing path (the issue named it): no
such device is in the lineup yet, so no value for it; it is added to `method` with the first
device that needs it, with the steps of its install written down as the FANTOM's are. Also open:
whether `systems` should say how much room (a size) rather than how many, once a device has too
little for a whole second copy; and whether a client shows the block (Settings' About page could;
not now).

### What is user content

User content is everything the user made, bought or chose, which an update must keep. All of it
is in the data folder (`OSCAIRN_DATA_HOME`; [RFC 0037](0037-storage-and-file-transfer.md) has
the layout): Performances and Sounds (`Performances/`, `Sounds/`), Setlists (`Setlists/`), the
library, its index and user file (`Libraries/`, `library/`), imported samples (`assets/`),
settings (`audio.json`, `controllers.json`, and the files beside them), recordings
(`recordings/`), installed packages and their stores (`Plugins/`, `packages.json`,
`preinstalled.json`), what was learned (`measurements.json`), and trust and repository settings
(`Keys/`, `trust.json`, `Repositories/`, `repositories.json`). On the FANTOM two files choosing
how the next power-on starts (`roland-first` and the others in `/home/root/oscairn-boot`) are
the owner's choices too, and already outside every bundle.

Not user content: the programs, plugins that came with the system, the profile built into the
programs, the files in `etc/default` (written from the profile at build time), logs and running
state (`var/oscairn`, on the FANTOM). An update replaces these.

### The rule

**An update replaces the system only.** Nothing in an install or update path deletes, rewrites
or moves a file of user content; what must change in it (a format step) is done by the engine
after the update, on its own copy-then-rename, as a save is. A device whose data is not apart
from the system (`userData` `with-system`) carries the folder across by copying it before the
old system goes, and may delete the old system only after the copy was compared and the new
system ran.

### Where the FANTOM's data folder should live

Today it is inside the bundle (`root/.local/share/oscairn`: the engine runs with `HOME=/root` in
the chroot) and each install copies it, 2.6 GB on the test unit. That is why a second bundle
costs a copy, why a bundle's deletion deletes the data in it, and why two bundles
that both ran diverge.

**It should live in one folder outside every bundle that every bundle uses**, and the name for
it is the existing `/home/root/oscairn-data`: it is empty on the unit today and no script or
program of Oscairn reads or writes it, so it can be taken for this without moving anything.
With that, an update is a new bundle and a switch of the link, and the profile says `separate`.

How is open, and not built, because the chroot sees only the bundle: a link to
`/home/root/oscairn-data` would point at nothing inside the chroot, and the engine's
`OSCAIRN_DATA_HOME` is read there. The way that works is a bind mount of the outside folder on
the bundle's `root/.local/share/oscairn`, made by `start-chroot.sh` (as it mounts `/proc` and
the rest) when it is asked to, and unmounted first by `stop-chroot.sh`. It is not built in this
step because a mistake in it loses content, and it cannot be tried on the unit without the
owner: a bundle's deletion with the mount still up would delete the data folder through it, the
mount would hide the bundle's own copy (an empty library on the next start if it were made
before the copy was moved), and the move of 2.6 GB is once, by hand, with room for it. The next
step, in this order: `start-chroot.sh` and `stop-chroot.sh` handle the mount, only when an
outside folder is named (unset, today's layout is unchanged), refusing when both the outside
and the bundle's folder hold content; a test of the pair with a stand-in `mount`; the one move
of a unit's data to the outside folder, by hand, checked with `diff -rq`; the profile's
`userData` becomes `separate`; the install steps in docs/fantom-0.md lose the copy.

What is built for #220 is the test (`devices/roland-fantom0/tests/fantom_start.sh`) that the
start, stop and restart scripts leave the data folder as it was (names, contents, inodes and
modification times), and that the copy-based install gives a new bundle's start the same content
as the old one's, with the old bundle's data deleted afterward.

## Open questions

- The update package format and signing keys: who signs a third party's product's images.
- Whether the UI process moves off X11 to a direct display path for faster boot (the OS README
  already keeps that open).

## Ruled out

- **Updating files in place on a running system.**
- **Updates applied without the user's say**, or while playing.
