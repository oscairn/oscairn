#!/bin/sh
# Builds Oscairn in Release from a clean checkout, for the machine's own CPU (x86_64 or arm64), and
# packs it, with its licenses, into build-release/oscairn-<version>-linux-<arch>.tar.gz
# (docs/releasing.md). The version is the CMake project version and OSCAIRN_PRERELEASE in
# CMakeLists.txt.
#
#   tools/release-linux.sh
#
# OSCAIRN_BUILD_JOBS limits the build's parallel jobs (default: Ninja's own choice).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
build=$repo/build-release

fail() { echo "release-linux: $*" >&2; exit 1; }

cd "$repo"
[ -z "$(git status --porcelain)" ] || fail "the tree has changes; a release is built from a commit"
case $(uname -m) in
	x86_64) arch=x86_64 ;;
	aarch64) arch=arm64 ;;
	*) fail "no release for $(uname -m)" ;;
esac
version=$(sed -n 's/^project(oscairn VERSION \([0-9.]*\).*/\1/p' CMakeLists.txt)
label=$(sed -n 's/^set(OSCAIRN_PRERELEASE "\(.*\)")$/\1/p' CMakeLists.txt)
[ -n "$version" ] || fail "no project version in CMakeLists.txt"
version=$version${label:+-$label}
commit=$(git rev-parse HEAD)
folder=oscairn-$version-linux-$arch
echo "Oscairn $version ($commit)"

rm -rf "$build"
cmake -S . -B "$build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DOSCAIRN_STANDALONE=ON -DBUILD_TESTING=OFF \
	-DOSCAIRN_SOURCE_COMMIT="$commit"
cmake --build "$build" --target oscairn_standalone oscairn-sign ${OSCAIRN_BUILD_JOBS:+-j "$OSCAIRN_BUILD_JOBS"}

# The build leaves the program with everything it looks for beside it (the engine, the worker and
# scan helpers, plugins/, themes/, Apps/): that folder is bin/, and the app finds the rest from there.
stage=$build/stage
root=$stage/$folder
mkdir -p "$root/bin"
cp -R "$build/hosts/standalone/oscairn_standalone_artefacts/Release/." "$root/bin/"
find "$root/bin" -type f | while read -r file; do
	file -b "$file" | grep -q '^ELF' || continue
	strip "$file"
	# Only the system's libraries, found by the loader, and no path into the build tree.
	ldd "$file" | grep -q 'not found' && fail "$file needs a library that is not there"
	readelf -d "$file" | grep -E 'RPATH|RUNPATH' | grep -q "$repo" && fail "$file has a path into the build tree"
	true
done

licenses=$root/Licenses
mkdir -p "$licenses"
. "$here/release-licenses.sh"
copy_licenses "$licenses" "$build/_deps/juce-src"
cp LICENSE "$root/LICENSE"
cat > "$licenses/SOURCE.txt" << EOF
Oscairn $version is free software: you can redistribute it and/or modify it under the terms of
the GNU Affero General Public License, version 3 or later (Oscairn-AGPL-3.0.txt). It comes with
NO WARRANTY.

Its complete source code, and the script that built this folder (tools/release-linux.sh), are at
https://github.com/oscairn/oscairn/tree/$commit
JUCE 9.0.4, which it is built with, is at https://github.com/juce-framework/JUCE/tree/9.0.4
EOF

# shortcut: no preinstalled packages (RFC 0031): the official repository publishes macos-arm64 builds
# only, so tools/preinstall-fetch.py fails for linux-*. Add it here, with --platform linux-$arch, when it
# lists Linux builds of tools/preinstalled.txt.

mkdir -p "$root/share/applications" "$root/share/icons/hicolor/1024x1024/apps"
cp tools/linux/oscairn.desktop "$root/share/applications/oscairn.desktop"
cp branding/app-icon-1024.png "$root/share/icons/hicolor/1024x1024/apps/oscairn.png"
cp tools/linux/README.txt "$root/README.txt"

tar -C "$stage" --owner=0 --group=0 --numeric-owner -czf "$build/$folder.tar.gz" "$folder"
echo "$build/$folder.tar.gz ($(du -h "$build/$folder.tar.gz" | cut -f1))"
