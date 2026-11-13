# Releasing

A release is, for now, one macOS disk image for Apple silicon,
`Oscairn-<version>-macos-arm64.dmg`, holding `Oscairn.app` (with its engine, plugin worker,
scan helper, bundled plugins, the Setlist and Settings apps, the themes and the
[preinstalled packages](#preinstalled-packages)), an `Applications` link to drag it to, and
`Licenses/`; and one tarball for each Linux CPU, `oscairn-<version>-linux-x86_64.tar.gz` and
`oscairn-<version>-linux-arm64.tar.gz` ([below](#the-linux-tarball)), without the preinstalled
packages.

## The version

The version is `project(oscairn VERSION …)` in the top-level `CMakeLists.txt`, with
`OSCAIRN_PRERELEASE` beside it while there is a pre-release label: `0.1.0` and `alpha.1` name
`0.1.0-alpha.1`. The app's `Info.plist` and what packages' `requires.oscairn` is checked
against take the numeric version only. Change both in a commit of their own before tagging.
CMake passes the whole release (`0.1.0-alpha.1`) to the code as `OSCAIRN_RELEASE`: it is what the
app, the Setlist and Settings apps (and their manifests' `appVersion`), the CLAP host
(`clap_host.version`) and the engine report, what `Oscairn --version` prints and what Settings ›
About shows. A release build also passes its commit (`OSCAIRN_SOURCE_COMMIT`), which About shows;
any other build says it is a development build.

## Cut a release

1. On `main`, with CI green, set the version and commit it.
2. Tag that commit and push the tag:

   ```sh
   git tag v0.1.0-alpha.1
   git push origin v0.1.0-alpha.1
   ```

   The tag must be `v` and the version, or the workflow stops.
3. [The Release workflow](../.github/workflows/release.yml) runs `tools/release-macos.sh` on
   `macos-15`, and `tools/release-linux.sh` on `ubuntu-24.04` (x86_64) and `ubuntu-24.04-arm`
   (arm64). Each keeps its file as a workflow artifact and attaches it to a **draft** release
   for the tag (marked a pre-release when the version has a label), with notes generated from
   the commits since the last release; whichever job comes first creates the draft.
4. Download the dmg and the tarballs from the draft, try them on machines that have never built
   Oscairn (below), edit the notes, and publish the draft by hand. Nothing publishes it
   automatically.

The workflow can also be run by hand (Actions, Release, Run workflow) from any branch: it then
builds the files as artifacts only, with no release.

## The first run

Try the dmg on a scratch home and data folder, never your own: with the app copied out of the
dmg, run `Oscairn.app/Contents/MacOS/Oscairn` with `HOME`, `OSCAIRN_DATA_HOME` and
`OSCAIRN_RUNTIME_DIR` set to folders under one scratch folder. The Library says it is setting up
the instruments that come with Oscairn; `packages.preinstalled` on the engine's control socket
then ends `done`, each package `installed`.

## Try it on a clean Mac

Scripted first-run checks of the downloaded app (launching it, snapshots, driving it over its
socket) start it with `--audio null`, for example
`/Applications/Oscairn.app/Contents/MacOS/Oscairn --audio null --snapshot home.png`. Its engine
then opens the null device, which writes nowhere, and no MIDI device, and the app opens none of its
own (no control surface or controller feedback port): no virtual output device has
to be set up by hand, and nothing a script does can sound. Listening to it is a check of its own,
without the option.

## Build one locally

```sh
tools/release-macos.sh
```

It needs a clean tree (a release is built from a commit, so its source is that commit),
CMake, Ninja and Xcode's command-line tools, and the network the first time (JUCE). It deletes
and rebuilds `build-release/`, builds the app in Release for arm64 and macOS 13.3 or later
(the oldest with floating-point `std::to_chars`), checks that no program or plugin in it links
anything but the system's libraries or has an rpath, signs it, and writes
`build-release/Oscairn-<version>-macos-arm64.dmg`.

codesign accepts only code in `Contents/MacOS`, where the app looks for its `themes/` and
`Apps/`; the script moves them to `Contents/Resources` and leaves links in their place.

## The Linux tarball

```sh
tools/release-linux.sh
```

On a Linux machine (x86_64 or arm64; the tarball is for the CPU it is built on), with a clean
tree, CMake, Ninja, a C++20 compiler, Python 3, the network the first time (JUCE, and the
preinstalled packages) and the development packages JUCE needs: `libasound2-dev`,
`libcurl4-openssl-dev`, `libfreetype-dev`, `libfontconfig1-dev`, `libx11-dev`, `libxext-dev`,
`libxinerama-dev`, `libxrandr-dev`, `libxcursor-dev` and `libxi-dev`. `OSCAIRN_BUILD_JOBS` limits the build's
parallel jobs. It deletes and rebuilds `build-release/` and writes
`build-release/oscairn-<version>-linux-<arch>.tar.gz`, where `<arch>` is `x86_64` or `arm64`
(the package platforms `linux-x86_64` and `linux-arm64`).

The tarball unpacks to one folder, `oscairn-<version>-linux-<arch>/`, that runs from wherever it
is put:

```
bin/Oscairn, oscairn-engine, oscairn-plugin-worker, oscairn-plugin-scan
bin/plugins/       the bundled plugins (.clap)
bin/themes/        the built-in themes
bin/Apps/          the Setlist and Settings apps
share/applications/oscairn.desktop, share/icons/hicolor/1024x1024/apps/oscairn.png
Licenses/          as in the dmg
LICENSE, README.txt  (README.txt names the packages it needs to run)
```

This is the layout the build leaves beside the program, and the one the app looks in first
(`hosts/standalone/Main.cpp`, `Theme.cpp`, `AudioEngine.cpp`): it finds all of it from its own
file (`Executable.h`), however it was started. The binaries are stripped, and the script fails
when one needs a library the machine lacks or has a path into the build tree. Unlike the dmg,
the program links the system's libraries (ALSA, libcurl, FreeType, fontconfig, X11), which
`README.txt` lists as packages; the build must be on the oldest system the release is to run
on, which the GitHub runners' Ubuntu 24.04 sets.

No preinstalled packages: the official repository publishes `macos-arm64` builds only, so
`tools/preinstall-fetch.py` fails for either Linux platform ("has no build for linux-arm64").
The tarball's app starts with none, as a build from source does. Add the fetch to
`tools/release-linux.sh` once the repository has Linux builds of what
[`tools/preinstalled.txt`](../tools/preinstalled.txt) lists.

Not made: an AppImage, a .deb or a Flatpak, a 32-bit or Wayland-native build, and checksums
(the dmg has none either).

Try one as the macOS first run above: unpacked in a scratch folder, with `HOME`,
`OSCAIRN_DATA_HOME` and `OSCAIRN_RUNTIME_DIR` set to folders under it, and
`bin/Oscairn --audio null` (a desktop, or `xvfb-run` with `--snapshot home.png`).

## Preinstalled packages

A release ships Oscairn's own published packages, which the engine installs at its first start
on each data folder ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#preinstalled-packages)).
[`tools/preinstalled.txt`](../tools/preinstalled.txt) lists them, each a package ID and version
at https://packages.oscairn.org; change it in a commit of its own, to the newest stable versions,
before tagging. The release script runs [`tools/preinstall-fetch.py`](../tools/preinstall-fetch.py),
which downloads the official repository's signed root and index and the listed packages' pieces
for the platform ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#pieces-the-format))
into `Oscairn.app/Contents/Resources/Preinstalled` as a repository snapshot, and fails the release
when anything does not verify: the root against the official root key, the index and each piece
by hash, every signature in the archive the pieces make for the platform by the publisher key
the index names, a version yanked or without a `macos-arm64` build, or an asset library a listed
package needs and the list lacks. It needs the network, and the pieces are never committed. To
check a list without building:

```sh
python3 -I tools/preinstall-fetch.py --oscairn-sign build/core/control/oscairn-sign \
    --platform macos-arm64 --out /tmp/preinstalled --licenses /tmp/preinstalled-licenses
```

The Linux tarball has none yet ([above](#the-linux-tarball)).

The 2026-11-11 set adds 11.8 MB of archives to the app (Analog's factory patches 7.4 MB of it).

## Signing and notarization

Without settings the app is signed ad hoc (`codesign -s -`), which Apple silicon needs to run
it at all. Gatekeeper refuses an ad hoc signed app downloaded from the web. On macOS 15 and
later, Control-click and Open no longer gets past it: the release notes must say to open the
app once, then choose Open Anyway in System Settings, Privacy & Security, or to run
`xattr -dr com.apple.quarantine /Applications/Oscairn.app`.

Once there is an Apple Developer account:

| Variable | Effect |
| --- | --- |
| `OSCAIRN_SIGN_IDENTITY` | A Developer ID Application identity in the keychain (`security find-identity -v -p codesigning`). Signs every program, plugin and app with it, with the hardened runtime and `tools/release-macos.entitlements` (audio input, and loading plugins signed by others) |
| `OSCAIRN_NOTARY_PROFILE` | A profile saved with `xcrun notarytool store-credentials`. With an identity, the dmg is signed, notarized and stapled |

In the workflow these need the certificate imported into a temporary keychain and the
notary credentials stored from repository secrets first; that step is not written yet.
Neither path has been tried.

## Licenses

`Licenses/` (in the dmg, and in the app's `Contents/Resources/Licenses`) holds Oscairn's
AGPL-3.0, `CREDITS.md`, `SOURCE.txt` (where the source of this exact commit is), and the
notices of what is built in: CLAP (MIT), Oscairn's CLAP extensions (MIT), miniz (MIT),
Monocypher (BSD-2-Clause or CC0), Lucide (ISC), Inter (OFL 1.1, also in each theme's
`fonts/`), JUCE's license and its `JUCE.spdx.json`, and in `Licenses/JUCE/` the libraries
JUCE's modules compile into the app: zlib, HarfBuzz, SheenBidi, LunaSVG, PlutoVG, libpng,
libjpeg (the Independent JPEG Group's credit), libwebp, FLAC, libogg, libvorbis, Opus,
opusfile, libopusenc and the VST3 SDK. sheredom's json.h is public domain and needs none.
The preinstalled packages' own licenses go in `Licenses/Packages/<package ID>/`: each package's
license texts and notices as it carries them, and `PACKAGE.txt` with its license expression and
its source's address and revision, which `SOURCE.txt` points to; under the GPL that is the
directions to the source the license asks for, so a package's source repository must stay
available as long as the release is offered. The LV2 host (jalv's ISC) is not in the macOS
app or the Linux tarball. The tarball's `Licenses/` is made by the same
[`tools/release-licenses.sh`](../tools/release-licenses.sh). When the app starts using another JUCE module, check `JUCE.spdx.json` for what it vendors
and add those notices to the script.

`SOURCE.txt` points at the commit on GitHub, so a dmg is only complete once its commit is
pushed: build releases from a pushed tag.
