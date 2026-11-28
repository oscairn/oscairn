#!/bin/sh
# The FANTOM bundle's Buildroot base: everything the bundle's configuration builds but Oscairn's own packages
# and the root filesystem (the toolchain, the base root filesystem), built once per configuration and reused, so
# that a bundle build compiles only Oscairn (docs/releasing.md "A prebuilt base for the bundle"). Nothing here
# publishes: pushing the image to a registry is a step of .github/workflows/fantom-base.yml, which is off.
#
#   os/base-image.sh key                 the base's key: what decides it (os/scripts/base-key.sh)
#   os/base-image.sh build [make opts]   build the base into the volume (OSCAIRN_OS_VOLUME, as os/build.sh) and prune it
#   os/base-image.sh image               make the local image oscairn-os-base:<key> from the volume
#   os/base-image.sh seed [IMAGE]        copy the base of IMAGE (default: the local one for the current key) into the
#                                        volume, unless the volume already holds a build; then os/fantom-bundle.sh
#                                        builds only Oscairn. os/fantom-bundle.sh does this itself when
#                                        OSCAIRN_OS_BASE_IMAGE names an image.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
volume=${OSCAIRN_OS_VOLUME:-oscairn-os}
key=$("$here/scripts/base-key.sh")
repo=${OSCAIRN_BASE_REPO:-oscairn-os-base}

case ${1:-} in
key) echo "$key" ;;
build)
	shift
	OSCAIRN_BASE_MODE=base "$here/build.sh" oscairn_fantom_bundle_defconfig "$@"
	;;
image)
	docker build -q -t oscairn-os-builder "$here/docker" > /dev/null
	ctx=$(mktemp -d)
	trap 'rm -rf "$ctx"' EXIT
	docker run --rm -v "$volume":/work oscairn-os-builder tar -C /work -cf - buildroot output > "$ctx/base.tar"
	cp "$here/docker/Dockerfile.base" "$ctx/Dockerfile"
	docker build -t "$repo:$key" "$ctx"
	echo "$repo:$key"
	;;
seed)
	image=${2:-$repo:$key}
	case $volume in /*) mkdir -p "$volume" ;; *) docker volume create "$volume" > /dev/null ;; esac
	docker run --rm -v "$volume":/dst "$image" sh -c \
		'if [ -d /dst/output ]; then echo "seed: the volume already holds a build; left alone"; else cp -a /work/. /dst/ && chown -R 1000:1000 /dst && echo "seed: the base is in the volume"; fi'
	;;
*)
	sed -n '2,/^set -eu/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//' >&2
	exit 1
	;;
esac
