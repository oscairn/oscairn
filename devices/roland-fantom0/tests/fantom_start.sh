#!/bin/sh
# oscairn-fantom-start, -stop and -boot off the unit (Linux: they read /proc): all parse, and
# their --dry-run, in a stand-in bundle with a temporary file as the panel's device, prints the
# steps with the profile's engine options and refuses while another process has the device open;
# boot, with stand-ins for oscairn-key-held's three answers and the one-shot file, picks Roland's
# application or Oscairn, and, on a stand-in /proc, stays while the engine runs, falls back to
# Roland's application when it ends unasked, and after oscairn-fantom-stop waits on the holder.
#   fantom_start.sh SCRIPTS_DIR
set -u
scripts=$1
fail=0
check() { if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; fail=1; fi; }

for s in oscairn-fantom-start oscairn-fantom-stop oscairn-fantom-boot; do
	check "$s parses" "sh -n '$scripts/$s'"
done

bundle=$(mktemp -d)
trap 'kill $holder ${watcher:-} 2>/dev/null; rm -rf "$bundle"' EXIT
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

cp "$bundle/etc/default/oscairn-engine" "$bundle/engine-defaults"
printf "OSCAIRN_HEADLESS=0\nOSCAIRN_PROFILE=fantom-07\n" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "a profile with a screen: the app, by its full path, on the display" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh setsid env DISPLAY=:0 sh -c .ulimit -l unlimited; exec' && echo \"\$out\" | grep -q 'sh /usr/bin/Oscairn --profile fantom-07 --plugins /usr/lib/clap' && ! echo \"\$out\" | grep -q oscairn-engine-run"
out=$(OSCAIRN_AUDIO_THREAD_CORES=1-3 "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the app with the audio threads' cores" "echo \"\$out\" | grep -q 'setsid env DISPLAY=:0 OSCAIRN_AUDIO_THREAD_CORES=1-3 sh -c'"
out=$("$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1); status=$?
check "--engine-only: the engine alone" "[ $status = 0 ] && echo \"\$out\" | grep -q 'setsid env oscairn-engine-run --output-device' && ! echo \"\$out\" | grep -q Oscairn"
mv "$bundle/engine-defaults" "$bundle/etc/default/oscairn-engine"
out=$(OSCAIRN_AUDIO_THREAD_CORES=1-3 "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the audio threads' cores from the environment, handed to the engine" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_AUDIO_THREAD_CORES=1-3 oscairn-engine-run --output-device'"
echo "OSCAIRN_AUDIO_THREAD_CORES='2,3'" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "or from the profile's file" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_AUDIO_THREAD_CORES=2,3 oscairn-engine-run'"
out=$(OSCAIRN_AUDIO_THREAD_CORES=1-3 "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the environment's first" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_AUDIO_THREAD_CORES=1-3 oscairn-engine-run'"
echo "OSCAIRN_ENGINE_ARGS='--output-device hw:1,0 --device-channels 6 --keyboard-device /dev/ttyGS0'" > "$bundle/etc/default/oscairn-engine"

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

# Step 4, the boot script staying in the foreground (--supervise, dry), on a stand-in /proc.
fake=$bundle/fakeproc
running() { mkdir -p "$fake/$2" && echo "$1" > "$fake/$2/comm"; }
gone() { rm -rf "$fake/$1"; }
until_done() { i=0; while kill -0 $1 2>/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done; ! kill -0 $1 2>/dev/null; }
supervise() { OSCAIRN_PROC=$fake OSCAIRN_BOOT_POLL=0.1 "$bundle/oscairn-fantom-boot" --dry-run --supervise > "$bundle/watch.out" 2>&1 & watcher=$!; }
check "boot's dry run says it would stay in the foreground" "OSCAIRN_KEY_HELD=\"sh -c 'exit 1'\" OSCAIRN_PROC='$bundle/proc' '$bundle/oscairn-fantom-boot' --dry-run 2>&1 | grep -q 'would stay in the foreground while the engine runs'"
running oscairn-engine 4242
supervise
sleep 0.5
check "the engine running: boot stays" "kill -0 $watcher && grep -q 'Oscairn runs; staying in the foreground' '$bundle/watch.out'"
gone 4242
check "the engine ended without oscairn-fantom-stop: the fallback, then Roland's application" "until_done $watcher && grep -q 'ended without oscairn-fantom-stop' '$bundle/watch.out' && grep -q 'would run: $bundle/oscairn-fantom-stop --panel' '$bundle/watch.out' && grep -q 'would run: /home/root/mi078' '$bundle/watch.out'"
running Oscairn 4240
supervise
sleep 0.5
check "the app running, without its engine yet: boot stays" "kill -0 $watcher && grep -q 'Oscairn runs; staying in the foreground' '$bundle/watch.out'"
gone 4240
check "the app ended: the fallback" "until_done $watcher && grep -q 'the app and its engine ended without oscairn-fantom-stop' '$bundle/watch.out'"
running oscairn-engine 4242
running oscairn-panel-h 4243
supervise
sleep 0.3
mkdir -p "$bundle/var/oscairn/run" && : > "$bundle/var/oscairn/run/stopped"
gone 4242
sleep 0.5
check "stopped on purpose: boot stays while the holder runs, and starts nothing" "kill -0 $watcher && grep -q 'stopped on purpose (oscairn-fantom-stop); staying while the panel holder runs' '$bundle/watch.out' && ! grep -q mi078 '$bundle/watch.out'"
gone 4243
check "and ends once the holder has gone too" "until_done $watcher && grep -q 'nothing left to keep' '$bundle/watch.out' && ! grep -q mi078 '$bundle/watch.out'"
rm -f "$bundle/var/oscairn/run/stopped"
out=$("$bundle/oscairn-fantom-stop" --dry-run 2>&1)
check "the stop marks itself on purpose (dry: says so)" "echo \"\$out\" | grep -q 'would mark the stop as on purpose' && [ ! -e '$bundle/var/oscairn/run/stopped' ]"
exit $fail
