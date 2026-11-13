#!/bin/sh
# oscairn-fantom-start and -stop off the unit (Linux: they read /proc): both parse, and their
# --dry-run, in a stand-in bundle with a temporary file as the panel's device, prints the steps
# with the profile's engine options, and refuses while another process has the device open.
#   fantom_start.sh SCRIPTS_DIR
set -u
scripts=$1
fail=0
check() { if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; fail=1; fi; }

for s in oscairn-fantom-start oscairn-fantom-stop; do
	check "$s parses" "sh -n '$scripts/$s'"
done

bundle=$(mktemp -d)
trap 'kill $holder 2>/dev/null; rm -rf "$bundle"' EXIT
cp "$scripts/oscairn-fantom-start" "$scripts/oscairn-fantom-stop" "$bundle/"
printf '#!/bin/sh\necho start-chroot ran >> "$(dirname "$0")/ran"\n' > "$bundle/start-chroot.sh"
printf '#!/bin/sh\necho stop-chroot ran >> "$(dirname "$0")/ran"\n' > "$bundle/stop-chroot.sh"
chmod +x "$bundle"/*
mkdir -p "$bundle/etc/default"
echo "OSCAIRN_ENGINE_ARGS='--output-device hw:1,0 --device-channels 6 --keyboard-device /dev/ttyGS0'" > "$bundle/etc/default/oscairn-engine"
export OSCAIRN_PANEL_DEVICE="$bundle/panel"
: > "$OSCAIRN_PANEL_DEVICE"

out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "a dry run with the panel free succeeds" "[ $status = 0 ]"
check "it would start the holder on the device" "echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh /usr/bin/oscairn-panel-holder $bundle/panel'"
check "and the engine with the profile's options" "echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh oscairn-engine-run --output-device hw:1,0 --device-channels 6 --keyboard-device /dev/ttyGS0'"
check "and changes nothing" "[ ! -e '$bundle/ran' ] && [ ! -e '$bundle/var' ]"

sh -c 'exec 3< "$1"; exec sleep 30' sh "$OSCAIRN_PANEL_DEVICE" &
holder=$!
sleep 0.3
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "another process holding the device: refused" "[ $status = 1 ] && echo \"\$out\" | grep -q 'process $holder holds $bundle/panel; not opening it'"
check "before the holder would start" "! echo \"\$out\" | grep -q oscairn-panel-holder"
kill $holder

out=$("$bundle/oscairn-fantom-stop" --dry-run 2>&1); status=$?
check "the stop's dry run" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would run: $bundle/stop-chroot.sh' && echo \"\$out\" | grep -q 'program version error'"
check "and changes nothing" "[ ! -e '$bundle/ran' ]"
check "a wrong option is refused" "! '$bundle/oscairn-fantom-start' --now > /dev/null 2>&1"
exit $fail
