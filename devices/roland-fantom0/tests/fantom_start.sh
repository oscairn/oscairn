#!/bin/sh
# oscairn-fantom-start, -stop and -boot off the unit (Linux: they read /proc): all parse, and
# their --dry-run, in a stand-in bundle with a temporary file as the panel's device, prints the
# steps with the profile's engine options and refuses while another process has the device open;
# boot, with stand-ins for oscairn-key-held's three answers and the one-shot file, picks Roland's
# application or Oscairn.
#   fantom_start.sh SCRIPTS_DIR
set -u
scripts=$1
fail=0
check() { if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; fail=1; fi; }

for s in oscairn-fantom-start oscairn-fantom-stop oscairn-fantom-boot; do
	check "$s parses" "sh -n '$scripts/$s'"
done

bundle=$(mktemp -d)
trap 'kill $holder 2>/dev/null; rm -rf "$bundle"' EXIT
cp "$scripts/oscairn-fantom-start" "$scripts/oscairn-fantom-stop" "$scripts/oscairn-fantom-boot" "$bundle/"
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
check "and the engine with the profile's options, in its own session" "echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh setsid env oscairn-engine-run --output-device hw:1,0 --device-channels 6 --keyboard-device /dev/ttyGS0'"
check "and changes nothing" "[ ! -e '$bundle/ran' ] && [ ! -e '$bundle/var' ]"
check "the panel there: nothing about it missing" "! echo \"\$out\" | grep -q 'is not here'"
if [ -e /sys/devices/system/cpu/cpufreq/policy0/scaling_governor ]; then
	check "a governor here: nothing about one missing" "! echo \"\$out\" | grep -q 'no CPU frequency governor'"
else
	check "no governor here: it says so" "echo \"\$out\" | grep -q 'no CPU frequency governor here: nothing to set'"
fi

out=$(OSCAIRN_PANEL_DEVICE="$bundle/no-panel" "$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "the panel absent: it says so and goes on" "[ $status = 0 ] && echo \"\$out\" | grep -q 'the panel device $bundle/no-panel is not here: the panel will not be read' && echo \"\$out\" | grep -q 'oscairn-panel-holder $bundle/no-panel'"

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
out=$("$bundle/oscairn-fantom-start" --dry-run --cold 2>&1); status=$?
check "--cold: nothing to stop, the rest as usual" "[ $status = 0 ] && ! echo \"\$out\" | grep -q weston-term && echo \"\$out\" | grep -q 'oscairn-panel-holder'"

boot() { out=$(OSCAIRN_KEY_HELD="sh -c 'exit $1'" OSCAIRN_PROC="$bundle/proc" "$bundle/oscairn-fantom-boot" --dry-run 2>&1); status=$?; }
boot 0
check "boot, MENU held: Roland's application" "[ $status = 0 ] && echo \"\$out\" | grep -q 'MENU is held' && echo \"\$out\" | grep -q 'would run: /home/root/mi078' && ! echo \"\$out\" | grep -q 'starting Oscairn'"
boot 2
check "boot, MENU unreadable: Roland's application" "[ $status = 0 ] && echo \"\$out\" | grep -q 'MENU cannot be read' && echo \"\$out\" | grep -q 'would run: /home/root/mi078'"
boot 1
check "boot, MENU up: Oscairn, cold" "[ $status = 0 ] && echo \"\$out\" | grep -q 'starting Oscairn' && echo \"\$out\" | grep -q 'oscairn-engine-run' && ! echo \"\$out\" | grep -q mi078"
mkdir -p "$bundle/var/oscairn" && : > "$bundle/var/oscairn/stock-next-boot"
boot 1
check "boot, stock-next-boot: Roland's application, the file removed when it is honored" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would remove' && echo \"\$out\" | grep -q 'stock-next-boot was set' && [ -e '$bundle/var/oscairn/stock-next-boot' ]"
rm -f "$bundle/var/oscairn/stock-next-boot"
sh -c 'exec 3< "$1"; exec sleep 30' sh "$OSCAIRN_PANEL_DEVICE" &
holder=$!
sleep 0.3
mkdir -p "$bundle/proc/$!/fd"
ln -s "$OSCAIRN_PANEL_DEVICE" "$bundle/proc/$!/fd/3"
boot 0
check "boot never starts Roland's application while the panel is held (from the fake proc)" "[ $status = 1 ] && echo \"\$out\" | grep -q 'still holds' && ! echo \"\$out\" | grep -q 'would run: /home/root/mi078'"
rm -rf "$bundle/proc"
kill $holder
sleep 0.3
boot 0
check "boot, the panel free again: Roland's application, nothing still holds" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would run: /home/root/mi078' && ! echo \"\$out\" | grep -q 'still holds'"
exit $fail
