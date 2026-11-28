#!/bin/sh
# Prints the key of the FANTOM bundle's Buildroot base: fantom-base-<16 hex digits of the SHA-256 of what decides
# that base>-<arch>. docs/releasing.md "A prebuilt base for the bundle". What decides it: the defconfig, the external
# tree's own configuration and make files, every package's Config.in (they select the dependencies, so they change
# what is built), the patches and kernel fragment, the builder image's recipe (host packages, the Rust release) and
# the script that names the Buildroot version. Left out because the base holds nothing from them: the overlays,
# post-build.sh and the four Oscairn packages' .mk, hash and patch files (they act when the root filesystem is made).
# A change to anything listed is a new key, so a base can never be older than its configuration.
#
#   os/scripts/base-key.sh [arch]       arch: arm64 (default)
#   os/scripts/base-key.sh --self-test
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)

files() {
  (
    cd "$repo"
    for f in os/buildroot/configs/oscairn_fantom_bundle_defconfig os/buildroot/external.desc os/buildroot/external.mk \
      os/buildroot/Config.in os/docker/Dockerfile os/scripts/container-build.sh os/buildroot/board/common/linux.fragment; do
      echo "$f"
    done
    ls os/buildroot/package/*/Config.in
    find os/buildroot/board/fantom-bundle/patches -type f 2> /dev/null
  ) | sort
}

hash() {
  if command -v sha256sum > /dev/null; then sha256sum; else shasum -a 256; fi | cut -c1-16
}

key() { # arch
  # each file's own hash and name, so that a renamed or moved file changes the key as well as an edited one
  list=$(cd "$repo" && files | while read -r f; do
    [ -f "$f" ] || { echo "base-key: $f is missing" >&2; exit 1; }
    echo "$(if command -v sha256sum > /dev/null; then sha256sum "$f"; else shasum -a 256 "$f"; fi)"
  done)
  [ -n "$list" ] || exit 1
  echo "fantom-base-$(printf '%s\n' "$list" | hash)-$1"
}

if [ "${1:-}" = "--self-test" ]; then
  a=$(key arm64)
  case $a in fantom-base-????????????????-arm64) ;; *) echo "self-test: odd key $a" >&2; exit 1 ;; esac
  [ "$a" = "$(key arm64)" ] || { echo "self-test: the key is not stable" >&2; exit 1; }
  [ "$a" != "$(key amd64)" ] || { echo "self-test: the architecture is not in the key" >&2; exit 1; }
  echo "base-key: ok ($a)"
  exit 0
fi
key "${1:-arm64}"
