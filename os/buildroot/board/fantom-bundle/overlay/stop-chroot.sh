#!/bin/sh
# Unmounts what start-chroot.sh mounted under the Oscairn bundle this script sits in. Run as
# root, after every program in the bundle has stopped: a mount still in use is left and named,
# and the script exits 1.
#
#   ./stop-chroot.sh
set -u
root=$(cd "$(dirname "$0")" && pwd)

status=0
for d in /tmp /run /dev/shm /dev/pts /dev /sys /proc; do
	if grep -q "^[^ ]* $root$d " /proc/mounts; then
		umount "$root$d" || { echo "stop-chroot.sh: $root$d is still in use" >&2; status=1; }
	fi
done
exit $status
