#!/bin/sh
# oscairn-tickgen-reset off the unit: a missing device is nothing to do, --dry-run on a stand-in
# file names both ioctls and changes nothing, and a real run on a plain file (which refuses the
# ioctls) is an error.
#   tickgen_reset.sh TOOL
set -u
tool=$1
fail=0
check() { if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; fail=1; fi; }
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

out=$("$tool" "$dir/no-tickgen" 2>&1); status=$?
check "no device: nothing to reset, exit 0" "[ $status = 0 ] && echo \"\$out\" | grep -q 'no $dir/no-tickgen: nothing to reset'"
out=$("$tool" --dry-run "$dir/no-tickgen" 2>&1); status=$?
check "no device, dry: the same" "[ $status = 0 ] && echo \"\$out\" | grep -q 'nothing to reset'"

echo stand-in > "$dir/tickgen"
out=$("$tool" --dry-run "$dir/tickgen" 2>&1); status=$?
check "dry run: both ioctls named, exit 0" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would set u_pid to -1: ioctl($dir/tickgen, 0x80044B0E)' && echo \"\$out\" | grep -q 'would set u_dbg to -1: ioctl($dir/tickgen, 0x80044B0F)'"
check "and the file untouched" "[ \"\$(cat '$dir/tickgen')\" = stand-in ]"

out=$("$tool" "$dir/tickgen" 2>&1); status=$?
check "a real run where the ioctls are refused: both tried, exit 1" "[ $status = 1 ] && echo \"\$out\" | grep -q 'u_pid: ioctl($dir/tickgen, 0x80044B0E) returned -1' && echo \"\$out\" | grep -q 'u_dbg: ioctl($dir/tickgen, 0x80044B0F) returned -1'"
check "and the file untouched" "[ \"\$(cat '$dir/tickgen')\" = stand-in ]"

check "a wrong option is refused" "! '$tool' --now > /dev/null 2>&1"
exit $fail
