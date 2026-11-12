#!/bin/sh
# Builds Oscairn.app in Release from a clean checkout and packs it, with its licenses, into
# build-release/Oscairn-<version>-macos-arm64.dmg (docs/releasing.md). The version is the CMake
# project version and OSCAIRN_PRERELEASE in CMakeLists.txt.
#
#   tools/release-macos.sh
#
# Signed ad hoc unless OSCAIRN_SIGN_IDENTITY names a Developer ID Application identity in the
# keychain; then with the hardened runtime, and with OSCAIRN_NOTARY_PROFILE (a profile saved with
# xcrun notarytool store-credentials) the dmg is notarized and stapled too.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
build=$repo/build-release
identity=${OSCAIRN_SIGN_IDENTITY:--}

fail() { echo "release-macos: $*" >&2; exit 1; }

cd "$repo"
[ -z "$(git status --porcelain)" ] || fail "the tree has changes; a release is built from a commit"
version=$(sed -n 's/^project(oscairn VERSION \([0-9.]*\).*/\1/p' CMakeLists.txt)
label=$(sed -n 's/^set(OSCAIRN_PRERELEASE "\(.*\)")$/\1/p' CMakeLists.txt)
[ -n "$version" ] || fail "no project version in CMakeLists.txt"
version=$version${label:+-$label}
commit=$(git rev-parse HEAD)
dmg=$build/Oscairn-$version-macos-arm64.dmg
echo "Oscairn $version ($commit)"

rm -rf "$build"
cmake -S . -B "$build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DOSCAIRN_STANDALONE=ON -DBUILD_TESTING=OFF \
	-DCMAKE_OSX_ARCHITECTURES=arm64 -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3
cmake --build "$build" --target oscairn_standalone oscairn-sign

stage=$build/dmg
app=$stage/Oscairn.app
mkdir -p "$stage"
ditto "$build/hosts/standalone/oscairn_standalone_artefacts/Release/Oscairn.app" "$app"
# codesign takes only code in Contents/MacOS: the themes and the apps' manifests go to Resources,
# found where the app looks for them through a link.
# shortcut: links, until the app looks in Contents/Resources itself.
for folder in themes Apps; do
	mv "$app/Contents/MacOS/$folder" "$app/Contents/Resources/$folder"
	ln -s "../Resources/$folder" "$app/Contents/MacOS/$folder"
done

# Licenses: Oscairn's, and the notices of what is built into it (CREDITS.md says what and where).
licenses=$app/Contents/Resources/Licenses
mkdir -p "$licenses"
cp LICENSE "$licenses/Oscairn-AGPL-3.0.txt"
cp CREDITS.md "$licenses/CREDITS.md"
cp core/clap-host/third_party/clap/LICENSE "$licenses/CLAP-MIT.txt"
cp core/clap-ext/LICENSE "$licenses/oscairn-clap-MIT.txt"
cp core/control/third_party/miniz/LICENSE "$licenses/miniz-MIT.txt"
cp core/control/third_party/monocypher/LICENCE.md "$licenses/Monocypher-BSD-2-Clause-or-CC0.md"
cp hosts/standalone/icons/LICENSE "$licenses/Lucide-ISC.txt"
cp hosts/standalone/themes/oscairn-dark/fonts/OFL.txt "$licenses/Inter-OFL-1.1.txt"
juce=$build/_deps/juce-src
cp "$juce/LICENSE.md" "$licenses/JUCE.md"
cp "$juce/JUCE.spdx.json" "$licenses/JUCE.spdx.json"
# What JUCE's modules the app links carry from others (JUCE.spdx.json lists all of them).
mkdir "$licenses/JUCE"
while read -r name path; do cp "$juce/modules/$path" "$licenses/JUCE/$name.txt"; done << EOF
zlib juce_core/zip/zlib/LICENSE
HarfBuzz juce_graphics/fonts/harfbuzz/COPYING
SheenBidi-Apache-2.0 juce_graphics/unicode/sheenbidi/LICENSE
LunaSVG juce_graphics/drawables/lunasvg/LICENSE
PlutoVG juce_graphics/drawables/lunasvg/plutovg/LICENSE
libpng juce_graphics/image_formats/pnglib/LICENSE
libwebp juce_graphics/image_formats/libwebp/COPYING
FLAC juce_audio_formats/codecs/flac/Flac Licence.txt
libogg juce_audio_formats/codecs/ogg/COPYING
libvorbis juce_audio_formats/codecs/vorbis/COPYING
Opus juce_audio_formats/codecs/opus/opus/COPYING
opusfile juce_audio_formats/codecs/opus/opusfile/COPYING
libopusenc juce_audio_formats/codecs/opus/libopusenc/COPYING
VST3-SDK juce_audio_processors_headless/format_types/VST3_SDK/LICENSE.txt
EOF
echo "This software is based in part on the work of the Independent JPEG Group." > "$licenses/JUCE/libjpeg.txt"
cat > "$licenses/SOURCE.txt" << EOF
Oscairn $version is free software: you can redistribute it and/or modify it under the terms of
the GNU Affero General Public License, version 3 or later (Oscairn-AGPL-3.0.txt). It comes with
NO WARRANTY.

Its complete source code, and the script that built this app (tools/release-macos.sh), are at
https://github.com/oscairn/oscairn/tree/$commit
JUCE 9.0.4, which it is built with, is at https://github.com/juce-framework/JUCE/tree/9.0.4

The packages that come with it, in Oscairn.app/Contents/Resources/Preinstalled, are each under
their own license: Packages/<package ID>/ holds each one's license texts and notices, and
PACKAGE.txt there names its license and where its source is.
EOF

# The packages that come with the release (RFC 0031, docs/releasing.md): the official repository's
# signed root and index, and the archives tools/preinstalled.txt lists, each checked here and again
# by the engine when it installs them at its first start; their licenses beside the others.
python3 -I "$here/preinstall-fetch.py" --oscairn-sign "$build/core/control/oscairn-sign" --platform macos-arm64 \
	--out "$app/Contents/Resources/Preinstalled" --licenses "$licenses/Packages" || fail "a package to preinstall did not verify"

# Nothing may come from the build tree or Homebrew: only the system's libraries, and no rpaths.
find "$app" -type f | while read -r file; do
	file -b "$file" | grep -q '^Mach-O' || continue
	foreign=$(otool -L "$file" | tail -n +2 | awk '{print $1}' | grep -vE '^/(usr/lib|System/Library)/' || true)
	rpaths=$(otool -l "$file" | awk '$1 == "cmd" && $2 == "LC_RPATH" {getline; getline; print $2}')
	[ -z "$foreign$rpaths" ] || fail "$file links outside the system: $foreign $rpaths"
done

# Inside out: every program and plugin, then the apps in Apps/, then Oscairn.app.
sign() {
	if [ "$identity" = - ]; then codesign --force --sign - "$@"
	else codesign --force --sign "$identity" --options runtime --timestamp --entitlements "$here/release-macos.entitlements" "$@"
	fi
}
find "$app/Contents/MacOS" -type f -perm -u+x | while read -r file; do
	if file -b "$file" | grep -q '^Mach-O'; then sign "$file"; fi
done
find "$app/Contents/Resources/Apps" -type d -name '*.app' | while read -r nested; do sign "$nested"; done
sign "$app"
codesign --verify --deep --strict "$app"

cp -R "$licenses" "$stage/Licenses"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "Oscairn $version" -srcfolder "$stage" -format UDZO -ov "$dmg"

if [ -n "${OSCAIRN_NOTARY_PROFILE:-}" ]; then
	[ "$identity" != - ] || fail "notarizing needs OSCAIRN_SIGN_IDENTITY"
	codesign --force --sign "$identity" --timestamp "$dmg"
	xcrun notarytool submit "$dmg" --keychain-profile "$OSCAIRN_NOTARY_PROFILE" --wait
	xcrun stapler staple "$dmg"
fi
echo "$dmg ($(du -h "$dmg" | cut -f1))"
