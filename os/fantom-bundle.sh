#!/bin/sh
# Builds the FANTOM-0 bundle (docs/fantom-0.md): Oscairn with a userland of its own, as a root
# filesystem to unpack on the instrument and enter with its start-chroot.sh. Built with
# os/build.sh (oscairn_fantom_bundle_defconfig); the result is build-os/oscairn-fantom-bundle.tar.gz.
#
#   os/fantom-bundle.sh [make options...]     e.g. BR2_JLEVEL=8
set -eu
here=$(cd "$(dirname "$0")" && pwd)

"$here/build.sh" oscairn_fantom_bundle_defconfig "$@"
docker run --rm -v "${OSCAIRN_OS_VOLUME:-oscairn-os}":/work oscairn-os-builder \
	cat /work/output/oscairn_fantom_bundle_defconfig/images/rootfs.tar.gz > "$here/../build-os/oscairn-fantom-bundle.tar.gz"
echo "$here/../build-os/oscairn-fantom-bundle.tar.gz"
