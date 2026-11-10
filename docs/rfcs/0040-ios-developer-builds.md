# RFC 0040: iOS and iPadOS developer builds

Status: proposed, 2026-11-10. Builds on the engine and the in-process plugin host, plugin worker
isolation ([RFC 0010](0010-plugin-worker-isolation.md)), reliability levels
([RFC 0027](0027-reliability-levels.md)), apps and their permissions
([RFC 0022](0022-apps-and-requests.md)), resource budgets ([RFC 0025](0025-resource-budgets.md)),
packages ([RFC 0006](0006-packages-and-portability.md), [RFC 0031](0031-package-repository-and-distribution.md))
and the responsive layout of the device profiles ([RFC 0024](0024-device-profiles.md)).

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. The engine, the in-process host and the core tests built for iOS, run in the Simulator
- [ ] 2. The app on an iPad: one process, bundled plugins, the audio session, CoreMIDI
- [ ] 3. Packages on iOS: data packages installed, plugin packages added before signing
- [ ] 4. iPhone: its profile and layout
- [ ] 5. Build and signing instructions for a free and a paid Apple account

## Problem

Oscairn runs on desktops and instruments. An iPad is a common, capable touch device that many
players already own, and a natural one to play and edit on. But the App Store is closed to
Oscairn as it is:

- App Review Guideline 2.5.2 forbids downloading or running code that adds or changes features,
  and iOS runs only code whose signature it trusts. A plugin imported as a `.clap` is exactly
  that, wherever the app comes from; the 2024 rule for emulators allows interpreted content, not
  native code.
- Oscairn is AGPL-3.0, and FM (Dexed) and Analog (Surge XT) are GPL-3.0 with many copyright
  holders. The FSF holds the App Store's terms incompatible with the GPL, and only every
  copyright holder could grant an exception.

An App Store build (an AUv3 host, plugins that are their own apps, the extensions carried over
AUv3's message channel) is a separate decision, deferred.

## Outcome

Oscairn builds for iOS and iPadOS for people who build and sign it themselves: developers, and
players willing to reinstall the app every 7 days with a free Apple account or once a year with a
paid developer account. No App Store or TestFlight build. Built from source and signed by its
user, the build is under the AGPL and the GPL alone, so every plugin whose license Oscairn
already meets can be in it.

## Design

### Accounts and signing

As Apple describes it at the time of writing (to check when step 5 is written):

| Account | Profile lasts | Limits |
| --- | --- | --- |
| A free Apple ID, through Xcode | 7 days, then the app does not open until reinstalled | 3 such apps per device |
| The paid Apple Developer Program | 1 year | 100 registered devices of each kind |

Tools that re-sign and refresh sideloaded apps automatically work with the same signed build.
Everything in the app is signed together, with the user's certificate.

### One process

An iOS app cannot start processes, sideloaded or not. So:

- **The engine runs in the app's process**, as the desktop tests already host the session.
  The desktop app's separate engine process and its local socket become direct calls.
- **Plugins run in the engine** (`OSCAIRN_PLUGIN_WORKER=` today), without worker isolation: a
  plugin's crash takes the app down. RFC 0027 asks for isolation at `instrument` and above, so an
  iOS build tops out at the level below, and says so in its status.
- **Apps run in the app's process or not at all**; apps as processes (RFC 0022) are not
  available. Which first-party apps come in-process is decided in step 2.

### Plugins

Plugins are in the app when it is signed, as signed frameworks in its `Frameworks` folder, which
discovery scans on iOS. A CLAP plugin needs no change to be one, beyond being built for iOS. A
plugin package is added by unpacking its builds into the app before signing (step 3); installing
one on the device says plainly that this build needs it added at build time.

Oscairn's extensions are unchanged, since the plugins are CLAP in the same process.

### Audio, MIDI and the device

- Audio through JUCE's iOS device (AVAudioSession), with the background audio mode so playing
  continues with the screen off. An interruption (a call, another app taking the session)
  silences the engine and resumes it after; a route or sample rate change goes through the
  existing sample rate change.
- MIDI through CoreMIDI, including USB class-compliant controllers and Bluetooth LE MIDI.
- iOS ends apps over their memory limit without warning, so the memory budget (RFC 0025) comes
  from the limit iOS reports (`os_proc_available_memory`), not the device's whole memory.
- iPad and iPhone profiles give the screen and its density; the responsive layout already scales
  to a profile's screen and keeps touch targets at least 6.5 mm.

## Build order and gates

| Step | Gate |
| --- | --- |
| 1 | The core libraries and the in-process host built with CMake for iOS; the core and control tests pass in the Simulator. |
| 2 | On an iPad, a Performance with bundled FM, Analog, the Sampler and the effects plays from a USB MIDI controller; an interruption and a route change resume without restarting the app; the status shows the reliability level below `instrument`. |
| 3 | Data packages (Sounds, cartridges, asset libraries) install from a repository on the device; a script adds a plugin package's iOS build to the app before signing, and installing one on the device is refused with that explanation. |
| 4 | On an iPhone, the Live and edit screens are usable at its profile's size. |
| 5 | The instructions, followed on a clean Mac, give a signed build on a device with a free account and with a paid one. |

## Open questions

- Can a plugin imported after install load, if it is signed on the Mac with the same certificate
  and copied onto the device? If iOS accepts it, step 3 could add plugins without rebuilding. To
  test, not to assume.
- Which of the first-party plugins build for iOS as they are, and at what CPU cost on an iPad.
- Whether the web UI and network control (RFC 0026) are useful on iOS, and under which rules.

## Ruled out

- An App Store or TestFlight build of this design: [Problem](#problem).
- Running imported native plugins in App Store builds by any means: iOS forbids it.
