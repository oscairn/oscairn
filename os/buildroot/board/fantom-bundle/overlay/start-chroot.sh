#!/bin/sh
# Runs COMMAND (a shell without one) in the Oscairn bundle this script sits in, on the running
# system's kernel (docs/fantom-0.md). Run as root. It first mounts what programs in the bundle
# need under the bundle: the system's /proc, /sys, /dev, /dev/pts and /dev/shm, and a tmpfs of
# its own on /run and /tmp. A mount already there is kept, so it can run again while the bundle
# is in use. Nothing else outside the bundle changes; stop-chroot.sh unmounts them.
#
#   ./start-chroot.sh [COMMAND [ARGUMENTS...]]
set -eu
root=$(cd "$(dirname "$0")" && pwd)

mounted() { grep -q "^[^ ]* $root$1 " /proc/mounts; }
for d in /proc /sys /dev /dev/pts /dev/shm; do
	if [ -d "$d" ] && ! mounted "$d"; then
		mkdir -p "$root$d"
		mount --bind "$d" "$root$d"
	fi
done
for d in /run /tmp; do
	mounted "$d" || mount -t tmpfs -o mode=1777,nosuid,nodev tmpfs "$root$d"
done

[ $# -gt 0 ] || set -- /bin/sh
# The bundle's own environment, none of the system's (its library paths above all).
exec chroot "$root" /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/root TERM="${TERM:-linux}" "$@"
