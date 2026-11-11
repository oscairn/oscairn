# Releasing

A release is, for now, one macOS disk image for Apple silicon:
`Oscairn-<version>-macos-arm64.dmg`, holding `Oscairn.app` (with its engine, plugin worker,
scan helper, bundled plugins, the Setlist and Settings apps and the themes), an
`Applications` link to drag it to, and `Licenses/`.

## The version

The version is `project(oscairn VERSION …)` in the top-level `CMakeLists.txt`, with
`OSCAIRN_PRERELEASE` beside it while there is a pre-release label: `0.1.0` and `alpha.1` name
`0.1.0-alpha.1`. The app's `Info.plist` and what packages' `requires.oscairn` is checked
against take the numeric version only. Change both in a commit of their own before tagging.

## Cut a release

1. On `main`, with CI green, set the version and commit it.
2. Tag that commit and push the tag:

   ```sh
   git tag v0.1.0-alpha.1
   git push origin v0.1.0-alpha.1
   ```

   The tag must be `v` and the version, or the workflow stops.
3. [The Release workflow](../.github/workflows/release.yml) runs `tools/release-macos.sh` on
   `macos-15`, keeps the dmg as a workflow artifact, and attaches it to a **draft** release
   for the tag (marked a pre-release when the version has a label), with notes generated from
   the commits since the last release.
4. Download the dmg from the draft, try it on a Mac that has never built Oscairn (below), edit
   the notes, and publish the draft by hand. Nothing publishes it automatically.

The workflow can also be run by hand (Actions, Release, Run workflow) from any branch: it then
builds the dmg as an artifact only, with no release.

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
The LV2 host (jalv's ISC) is not in the macOS app. When the app starts using another JUCE
module, check `JUCE.spdx.json` for what it vendors and add those notices to the script.

`SOURCE.txt` points at the commit on GitHub, so a dmg is only complete once its commit is
pushed: build releases from a pushed tag.
