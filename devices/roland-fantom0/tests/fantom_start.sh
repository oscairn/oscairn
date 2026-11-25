#!/bin/sh
# oscairn-fantom-start, -stop and -boot off the unit (Linux: they read /proc): all parse, and
# their --dry-run, in a stand-in bundle with a temporary file as the panel's device, prints the
# steps with the profile's engine options and refuses while another process has the device open;
# boot, with stand-ins for oscairn-key-held's three answers and the one-shot file, picks Roland's
# application alone or schedules the takeover (a stand-in systemd-run), and its --takeover, on a
# stand-in /proc, waits for mi078 and its hold on the panel, starts warm, stays while the engine
# runs, stops what it started when it ends unasked (never starting mi078), and after
# oscairn-fantom-stop waits on the holder.
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
n() { echo "$out" | grep -n "$1" | head -n 1 | cut -d: -f1; }
check "the tickgen pids reset after Roland's application has gone and the holder started, before the engine" "[ -n \"\$(n 'oscairn-tickgen-reset')\" ] && [ \$(n 'is not running') -lt \$(n 'oscairn-tickgen-reset') ] && [ \$(n 'oscairn-panel-holder') -lt \$(n 'oscairn-tickgen-reset') ] && [ \$(n 'oscairn-tickgen-reset') -lt \$(n 'oscairn-engine-run') ]"
check "the panel there: nothing about it missing" "! echo \"\$out\" | grep -q 'is not here'"
if [ -e /sys/devices/system/cpu/cpufreq/policy0/scaling_governor ]; then
	check "a governor here: nothing about one missing" "! echo \"\$out\" | grep -q 'no CPU frequency governor'"
else
	check "no governor here: it says so" "echo \"\$out\" | grep -q 'no CPU frequency governor here: nothing to set'"
fi

# Signal 42 (Roland's tickgen sends it to a stale pid): a child the script starts, here a cat of
# /proc/PID/comm standing in, has it ignored (SigIgn bit 41, mask 0x20000000000).
mkdir "$bundle/fakebin"
printf '#!/bin/sh\nsed -n "s/^SigIgn:[[:space:]]*//p" /proc/self/status >> "%s/sigign"\nexec /bin/cat "$@"\n' "$bundle" > "$bundle/fakebin/cat"
chmod +x "$bundle/fakebin/cat"
PATH="$bundle/fakebin:$PATH" "$bundle/oscairn-fantom-start" --dry-run > /dev/null 2>&1
check "a child of the start script has signal 42 ignored" "[ -s '$bundle/sigign' ] && [ \$(( 0x\$(head -n 1 '$bundle/sigign') >> 41 & 1 )) = 1 ]"

cp "$bundle/etc/default/oscairn-engine" "$bundle/engine-defaults"
printf "OSCAIRN_HEADLESS=0\nOSCAIRN_PROFILE=fantom-07\n" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "a profile with a screen: the app, by its full path, on the display" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh setsid env DISPLAY=:0 sh -c .ulimit -l unlimited; exec' && echo \"\$out\" | grep -q 'sh /usr/bin/Oscairn --profile fantom-07 --plugins /usr/lib/clap' && ! echo \"\$out\" | grep -q oscairn-engine-run"
check "and the display its own first: Weston stopped, the bundle's X server on :0" "echo \"\$out\" | grep -q 'would run: systemctl stop weston.service' && echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh /usr/bin/Xorg :0 vt7'"
out=$(OSCAIRN_AUDIO_THREAD_CORES=1-3 "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the app with the audio threads' cores" "echo \"\$out\" | grep -q 'setsid env DISPLAY=:0 OSCAIRN_AUDIO_THREAD_CORES=1-3 sh -c'"
out=$("$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1); status=$?
check "--engine-only: the engine alone" "[ $status = 0 ] && echo \"\$out\" | grep -q 'setsid env oscairn-engine-run --output-device' && ! echo \"\$out\" | grep -q Oscairn"
check "and the display left as it is" "! echo \"\$out\" | grep -q -e weston.service -e Xorg"
# The Rust UI (docs/rust-ui.md): the engine alone, then oscairn-ui on the display without X,
# turned by the screen's rotation, touch with it; never the JUCE app at the start.
out=$(OSCAIRN_UI=rust OSCAIRN_UI_ROTATION=90 OSCAIRN_UDEV_RULES="$bundle/rules" "$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "ui rust: the engine alone, then the Rust UI" "[ $status = 0 ] && echo \"\$out\" | grep -q 'setsid env oscairn-engine-run --output-device' && echo \"\$out\" | grep -q 'SLINT_KMS_ROTATION=90 /usr/bin/oscairn-ui --profile fantom-07 --socket /run/oscairn/engine.sock --handover' && ! echo \"\$out\" | grep -q -e /usr/bin/Oscairn -e /usr/bin/Xorg"
check "and touch turned with it, by a rule at run time" "echo \"\$out\" | grep -q 'would write $bundle/rules/99-oscairn-touch.rules (LIBINPUT_CALIBRATION_MATRIX 0 1 0 -1 0 1)' && [ ! -e \"$bundle/rules\" ]"
# The touch rule written for the Rust UI and removed for the X server (which turns touch itself).
eval "$(sed -n '/^touch_rule()/,/^}/p' "$scripts/oscairn-fantom-start")"
say() { :; }; rules=$bundle/rules2; matrix="0 1 0 -1 0 1"
touch_rule; [ -e "$rules/99-oscairn-touch.rules" ] && touch_rule off
check "the touch rule is removed for the X server" "[ ! -e \"$rules/99-oscairn-touch.rules\" ]"
# The handover file read as the keeper reads it, with and without its newline (an unterminated
# line once made the keeper start the Rust UI again in place of the JUCE app).
eval "$(grep '^handed_over()' "$scripts/oscairn-fantom-start")"
mkdir -p "$bundle/tmp"; root=$bundle
printf settings > "$bundle/tmp/oscairn-handover"
check "a handover file without a newline names its app" "handed_over && [ \"\$app\" = settings ]"
printf 'library\n' > "$bundle/tmp/oscairn-handover"
check "and one with it" "handed_over && [ \"\$app\" = library ]"
: > "$bundle/tmp/oscairn-handover"
check "an empty one names none" "! handed_over"
rm -rf "$bundle/tmp"
out=$(OSCAIRN_UI=rust "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "ui rust with --engine-only: no UI" "! echo \"\$out\" | grep -q oscairn-ui"
out=$(OSCAIRN_UI=qt "$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "ui neither: refused" "[ $status != 0 ] && echo \"\$out\" | grep -q 'OSCAIRN_UI must be juce or rust'"
mv "$bundle/engine-defaults" "$bundle/etc/default/oscairn-engine"
out=$(OSCAIRN_AUDIO_THREAD_CORES=1-3 "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the audio threads' cores from the environment, handed to the engine" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_AUDIO_THREAD_CORES=1-3 oscairn-engine-run --output-device'"
echo "OSCAIRN_AUDIO_THREAD_CORES='2,3'" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "or from the profile's file" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_AUDIO_THREAD_CORES=2,3 oscairn-engine-run'"
out=$(OSCAIRN_AUDIO_THREAD_CORES=1-3 "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the environment's first" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_AUDIO_THREAD_CORES=1-3 oscairn-engine-run'"
out=$(OSCAIRN_WORKER_BUDGET_US=2000 OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.analog,org.oscairn.sampler "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the worker budget and extra block from the environment, handed to the engine" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_AUDIO_THREAD_CORES=2,3 OSCAIRN_WORKER_BUDGET_US=2000 OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.analog,org.oscairn.sampler oscairn-engine-run'"
printf "OSCAIRN_WORKER_BUDGET_US='2400'\nOSCAIRN_WORKER_EXTRA_BLOCK='org.oscairn.fm'\n" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "or from the profile's file" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_BUDGET_US=2400 OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.fm oscairn-engine-run'"
out=$(OSCAIRN_WORKER_BUDGET_US=2000 "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the environment's first" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_BUDGET_US=2000 OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.fm oscairn-engine-run'"
printf "OSCAIRN_HEADLESS=0\nOSCAIRN_PROFILE=fantom-07\n" >> "$bundle/etc/default/oscairn-engine"
out=$(OSCAIRN_WORKER_BUDGET_US=2000 OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.analog "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the app: the worker budget and extra block, after the cores" "echo \"\$out\" | grep -q 'setsid env DISPLAY=:0 OSCAIRN_AUDIO_THREAD_CORES=2,3 OSCAIRN_WORKER_BUDGET_US=2000 OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.analog sh -c'"
out=$(OSCAIRN_THP=keep "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "OSCAIRN_THP=keep handed to the app, last" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.fm OSCAIRN_THP=keep sh -c'"
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the app: the profile's file when the environment has none" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_BUDGET_US=2400 OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.fm sh -c'"
out=$(OSCAIRN_THP=keep "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_THP=keep handed to the engine, last" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.fm OSCAIRN_THP=keep oscairn-engine-run'"
check "and not set when the environment has none" "! \"$bundle/oscairn-fantom-start\" --dry-run --engine-only 2>&1 | grep -q OSCAIRN_THP"
out=$(OSCAIRN_THP=keep OSCAIRN_CPU_LATENCY=keep "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_CPU_LATENCY=keep handed to the engine, after OSCAIRN_THP" "echo \"\$out\" | grep -q 'OSCAIRN_THP=keep OSCAIRN_CPU_LATENCY=keep oscairn-engine-run'"
out=$(OSCAIRN_CPU_LATENCY=keep "$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "and to the app" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.fm OSCAIRN_CPU_LATENCY=keep sh -c'"
check "and not set when the environment has none" "! \"$bundle/oscairn-fantom-start\" --dry-run --engine-only 2>&1 | grep -q OSCAIRN_CPU_LATENCY"
out=$(OSCAIRN_WORKER_SPIN_US=0 "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_WORKER_SPIN_US from the environment, handed to the engine" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_EXTRA_BLOCK=org.oscairn.fm OSCAIRN_WORKER_SPIN_US=0 oscairn-engine-run'"
echo "OSCAIRN_WORKER_SPIN_US='200'" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "or from the profile's file" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_SPIN_US=200 oscairn-engine-run'"
out=$(OSCAIRN_WORKER_DEADLINE=1 "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_WORKER_DEADLINE handed to the engine, after the spin" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_SPIN_US=200 OSCAIRN_WORKER_DEADLINE=1 oscairn-engine-run'"
out=$(OSCAIRN_SHARING=cpu "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_SHARING handed to the engine" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_SPIN_US=200 OSCAIRN_SHARING=cpu oscairn-engine-run'"
echo "OSCAIRN_PLUGIN_HOSTS='3'" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_PLUGIN_HOSTS from the profile's file" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_SPIN_US=200 OSCAIRN_PLUGIN_HOSTS=3 oscairn-engine-run'"
out=$(OSCAIRN_PLUGIN_HOSTS=0 OSCAIRN_OWN_HOST=org.oscairn.analog "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "or 0 from the environment, a worker per instance to compare; OSCAIRN_OWN_HOST after it" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_SPIN_US=200 OSCAIRN_PLUGIN_HOSTS=0 OSCAIRN_OWN_HOST=org.oscairn.analog oscairn-engine-run'"
echo "OSCAIRN_WORKER_REST='0'" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_WORKER_REST from the profile's file, last" "echo \"\$out\" | grep -q 'OSCAIRN_PLUGIN_HOSTS=3 OSCAIRN_WORKER_REST=0 oscairn-engine-run'"
out=$(OSCAIRN_WORKERS=chain OSCAIRN_ENGINE_AUDIO_CORES=3 OSCAIRN_WORKER_AUDIO_CORES=0-2 "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "OSCAIRN_WORKERS and the engine's and workers' core lists handed to the engine, after it" "echo \"\$out\" | grep -q 'OSCAIRN_WORKER_REST=0 OSCAIRN_WORKERS=chain OSCAIRN_ENGINE_AUDIO_CORES=3 OSCAIRN_WORKER_AUDIO_CORES=0-2 oscairn-engine-run'"
echo "OSCAIRN_ENGINE_ARGS='--output-device hw:1,0 --device-channels 6 --keyboard-device /dev/ttyGS0'" > "$bundle/etc/default/oscairn-engine"

# The first-party plugins' device settings: OSCAIRN_SAMPLER_*, _ANALOG_*, _FM_*, _ARPEGGIATOR_*, _EFFECTS_*.
out=$(OSCAIRN_SAMPLER_MAX_VOICES=28 OSCAIRN_FM_POLYPHONY=16 FOO=1 OSCAIRN_SECRET=x "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "a plugin variable from the environment reaches the engine line" "echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh setsid env OSCAIRN_FM_POLYPHONY=16 OSCAIRN_SAMPLER_MAX_VOICES=28 oscairn-engine-run --output-device'"
check "an unrelated variable does not" "! echo \"\$out\" | grep -q -e FOO -e OSCAIRN_SECRET"
out=$(OSCAIRN_VA_MAX_VOICES=24 OSCAIRN_SAMPLER_MAX_VOICES_PER_SOUND=8 "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "the first-party engines' settings too, OSCAIRN_VA_* and OSCAIRN_SAMPLER_*" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_SAMPLER_MAX_VOICES_PER_SOUND=8 OSCAIRN_VA_MAX_VOICES=24 oscairn-engine-run'"
printf "OSCAIRN_SAMPLER_MAX_VOICES=40\nOSCAIRN_ANALOG_FAST_OSC=1\n" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "a plugin variable from the defaults file reaches the engine line" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_ANALOG_FAST_OSC=1 OSCAIRN_SAMPLER_MAX_VOICES=40 oscairn-engine-run'"
out=$(OSCAIRN_SAMPLER_MAX_VOICES=28 "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "the environment wins over the file" "echo \"\$out\" | grep -q 'setsid env OSCAIRN_ANALOG_FAST_OSC=1 OSCAIRN_SAMPLER_MAX_VOICES=28 oscairn-engine-run'"
out=$(OSCAIRN_SAMPLER_MAX_VOICES= "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1)
check "an empty one in the environment leaves the file's" "echo \"\$out\" | grep -q 'OSCAIRN_SAMPLER_MAX_VOICES=40 oscairn-engine-run'"
printf "OSCAIRN_HEADLESS=0\nOSCAIRN_PROFILE=fantom-07\n" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run 2>&1)
check "the app too" "echo \"\$out\" | grep -q 'setsid env DISPLAY=:0 OSCAIRN_ANALOG_FAST_OSC=1 OSCAIRN_SAMPLER_MAX_VOICES=40 sh -c'"
for bad in 'a b' 'a;b' "a'b" 'a"b' 'a$b' 'a`b`' 'a|b' 'a&b' 'a>b' 'a*b'; do
	out=$(OSCAIRN_EFFECTS_MODE="$bad" "$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1); status=$?
	check "the value '$bad' is refused" "[ $status = 1 ] && echo \"\$out\" | grep -q 'OSCAIRN_EFFECTS_MODE: the value must hold only' && ! echo \"\$out\" | grep -q 'would run: $bundle/start-chroot.sh setsid'"
done
echo "OSCAIRN_ENGINE_ARGS='--output-device hw:1,0 --device-channels 6 --keyboard-device /dev/ttyGS0'" > "$bundle/etc/default/oscairn-engine"
printf "OSCAIRN_FM_POLYPHONY='a b'\n" >> "$bundle/etc/default/oscairn-engine"
out=$("$bundle/oscairn-fantom-start" --dry-run --engine-only 2>&1); status=$?
check "a value from the file with a space is refused too" "[ $status = 1 ] && echo \"\$out\" | grep -q 'OSCAIRN_FM_POLYPHONY: the value must hold only'"
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

quiet=$bundle/quietproc
mkdir -p "$quiet/sys/kernel" && printf '7\t4\t1\t7\n' > "$quiet/sys/kernel/printk"
out=$(OSCAIRN_PROC=$quiet "$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "the kernel's console would be quietened, and nothing changed" "[ $status = 0 ] && echo \"\$out\" | grep -q \"would set the kernel's console log level to 4 4 1 7 (now 7.4.1.7)\" && grep -q '^7.4.1.7$' '$quiet/sys/kernel/printk'"
chmod a-w "$quiet/sys/kernel/printk"
[ -w "$quiet/sys/kernel/printk" ] || check "a console level that cannot be written: nothing said about it" "! OSCAIRN_PROC=$quiet '$bundle/oscairn-fantom-start' --dry-run 2>&1 | grep -q 'console log level'"

out=$("$bundle/oscairn-fantom-stop" --dry-run 2>&1); status=$?
check "the stop's dry run" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would run: $bundle/stop-chroot.sh' && echo \"\$out\" | grep -q 'program version error'"
check "and Roland's display server started again" "echo \"\$out\" | grep -q 'would run: systemctl start weston.service'"
check "and changes nothing" "[ ! -e '$bundle/ran' ]"
check "a wrong option is refused" "! '$bundle/oscairn-fantom-start' --now > /dev/null 2>&1"
out=$("$bundle/oscairn-fantom-start" --dry-run --cold 2>&1); status=$?
check "--cold: nothing to stop, the rest as usual" "[ $status = 0 ] && ! echo \"\$out\" | grep -q weston-term && echo \"\$out\" | grep -q 'oscairn-panel-holder'"

# SysV shared memory left by ended processes, on a stand-in /proc: only 101 qualifies (key 0, no
# attachment, this user's, its creator gone); 102 is attached, 103 has a key, 104's creator runs,
# 105 is another user's.
shm=$bundle/shmproc me=$(id -u)
mkdir -p "$shm/sysvipc" "$shm/4244"
{
	echo "       key      shmid perms                  size  cpid  lpid nattch   uid   gid  cuid  cgid      atime      dtime      ctime                   rss                  swap"
	for row in "0 101 777 3686400 999991 0" "0 102 777 3686400 999992 1" "1234 103 600 4096 999993 0" "0 104 777 3686400 4244 0"; do
		set -- $row
		echo "$1 $2 $3 $4 $5 $5 $6 $me 0 $me 0 0 0 0 0 0"
	done
	echo "0 105 777 3686400 999994 999994 0 $((me + 1)) 0 $((me + 1)) 0 0 0 0 0 0"
} > "$shm/sysvipc/shm"
out=$(OSCAIRN_PROC=$shm "$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "a dry run removes the shared memory an ended process left, and only that" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would run: ipcrm -m 101$' && [ \$(echo \"\$out\" | grep -c 'ipcrm -m') = 1 ]"
out=$(OSCAIRN_PROC=$shm OSCAIRN_IPCRM=no-such-ipcrm "$bundle/oscairn-fantom-start" --dry-run 2>&1); status=$?
check "without ipcrm: says so and goes on" "[ $status = 0 ] && echo \"\$out\" | grep -q 'no no-such-ipcrm here: not removing the shared memory ended processes left (1 segments)' && ! echo \"\$out\" | grep -q 'ipcrm -m'"
rm -rf "$shm"

export OSCAIRN_BOOT_DIR="$bundle/bootdir" OSCAIRN_BOOT_RUN="$bundle/bootrun"
mkdir -p "$OSCAIRN_BOOT_RUN"
boot() { out=$(OSCAIRN_BOOT_RELEASE=2 OSCAIRN_KEY_HELD="sh -c 'exit $1'" OSCAIRN_PROC="$bundle/proc" "$bundle/oscairn-fantom-boot" --dry-run 2>&1); status=$?; }
roland='go.sh runs /home/root/mi078'
boot 0
check "boot, TEMPO held: Roland's application" "[ $status = 3 ] && echo \"\$out\" | grep -q 'TEMPO was held' && echo \"\$out\" | grep -q '$roland' && ! echo \"\$out\" | grep -q 'starting Oscairn'"
check "boot logs to its folder outside the bundle" "grep -q 'TEMPO was held' '$OSCAIRN_BOOT_DIR/boot.log'"
# Held for the first 3 reads, then let go: Roland's app waits for the release (4 reads), not the bound (10).
reads="$bundle/key-reads"; rm -f "$reads"
out=$(OSCAIRN_BOOT_RELEASE=10 OSCAIRN_KEY_HELD="sh -c 'n=\$(cat $reads 2>/dev/null || echo 0); echo \$((n + 1)) > $reads; [ \$n -lt 3 ]'" OSCAIRN_PROC="$bundle/proc" "$bundle/oscairn-fantom-boot" --dry-run 2>&1); status=$?
check "boot, TEMPO held: Roland's app starts once it is let go" "[ $status = 3 ] && [ \"\$(cat '$reads')\" = 4 ] && echo \"\$out\" | grep -q 'TEMPO was held'"
boot 2
check "boot, TEMPO unreadable: Roland's application" "[ $status = 3 ] && echo \"\$out\" | grep -q 'TEMPO cannot be read' && echo \"\$out\" | grep -q '$roland'"
boot 1
check "boot, TEMPO up: the takeover scheduled through systemd-run (dry: printed), Roland's application first" "[ $status = 3 ] && echo \"\$out\" | grep -q 'would write $OSCAIRN_BOOT_DIR/attempt and run: systemd-run --unit=oscairn-takeover --collect --no-block .* $bundle/oscairn-fantom-boot --takeover' && echo \"\$out\" | grep -q 'a dry run' && [ ! -e '$OSCAIRN_BOOT_DIR/attempt' ]"
: > "$OSCAIRN_BOOT_DIR/stock-next-boot"
boot 1
check "boot, stock-next-boot: Roland's application, the file removed when it is honored" "[ $status = 3 ] && echo \"\$out\" | grep -q 'would remove' && echo \"\$out\" | grep -q 'stock-next-boot was set' && [ -e '$OSCAIRN_BOOT_DIR/stock-next-boot' ]"
rm -f "$OSCAIRN_BOOT_DIR/stock-next-boot"
sh -c 'exec 3< "$1"; exec sleep 30' sh "$OSCAIRN_PANEL_DEVICE" &
holder=$!
sleep 0.3
mkdir -p "$bundle/proc/$!/fd"
ln -s "$OSCAIRN_PANEL_DEVICE" "$bundle/proc/$!/fd/3"
boot 0
check "boot never leaves Roland's application to go.sh while the panel is held (exit 0)" "[ $status = 0 ] && echo \"\$out\" | grep -q 'still holds' && ! echo \"\$out\" | grep -q '$roland'"
rm -rf "$bundle/proc"
kill $holder
sleep 0.3
boot 0
check "boot, the panel free again: Roland's application, nothing still holds" "[ $status = 3 ] && echo \"\$out\" | grep -q '$roland' && ! echo \"\$out\" | grep -q 'still holds'"

# The decision for real (not --dry-run) in a stand-in bundle whose systemd-run, start and stop
# only log (the start plays Roland's application stopped and the engine started), and go.sh's
# line around it.
fb=$bundle/fakeboot
mkdir -p "$fb/b" "$fb/run" "$fb/dir" "$fb/proc"
cp "$scripts/oscairn-fantom-boot" "$fb/b/"
printf '#!/bin/sh\necho systemd-run "$@" >> "$(dirname "$0")/ran"\n[ ! -e "$(dirname "$0")/sysrun-fails" ]\n' > "$fb/b/systemd-run"
cat > "$fb/b/oscairn-fantom-start" <<'EOF'
#!/bin/sh
d=$(dirname "$0")
echo start "$@" >> "$d/ran"
[ ! -e "$d/fails-early" ] || exit 1
rm -rf "$OSCAIRN_PROC/500"
[ ! -e "$d/hang" ] || exec sleep 30
[ ! -e "$d/fails" ] || exit 1
mkdir -p "$OSCAIRN_PROC/4242" && echo oscairn-engine > "$OSCAIRN_PROC/4242/comm"
EOF
printf '#!/bin/sh\necho stop "$@" >> "$(dirname "$0")/ran"\n' > "$fb/b/oscairn-fantom-stop"
chmod +x "$fb/b"/*
real() { rm -f "$fb/b/ran"; out=$(OSCAIRN_BOOT_RELEASE=2 OSCAIRN_KEY_HELD="sh -c 'exit ${1:-1}'" OSCAIRN_PROC="$fb/proc" OSCAIRN_BOOT_DIR="$fb/dir" OSCAIRN_BOOT_RUN="$fb/run" OSCAIRN_SYSTEMD_RUN="$fb/b/systemd-run" "$fb/b/oscairn-fantom-boot" 2>&1); status=$?; }
fresh() { rm -rf "$fb/run"/* "$fb/dir/attempt" "$fb/b/fails" "$fb/b/fails-early" "$fb/b/hang" "$fb/b/sysrun-fails" "$fb/proc"/*; }
fresh; real
check "real: the takeover scheduled in a unit of its own, attempt left, then Roland's application without the started marker" "[ $status = 3 ] && grep -q 'systemd-run --unit=oscairn-takeover --collect --no-block --description=Oscairn takes over from Roland.s application $fb/b/oscairn-fantom-boot --takeover' '$fb/b/ran' && [ -e '$fb/dir/attempt' ] && [ ! -e '$fb/run/oscairn-boot-started' ] && echo \"\$out\" | grep -q 'Oscairn takes over once it has set up the sound' && ! grep -q '^start' '$fb/b/ran'"
real
check "real: a second run in the same boot is Roland's application, nothing scheduled" "[ $status = 3 ] && echo \"\$out\" | grep -q 'already ran this boot' && [ ! -e '$fb/b/ran' ]"
fresh; : > "$fb/b/sysrun-fails"; real
check "real: systemd-run failing: Roland's application, attempt removed" "[ $status = 3 ] && echo \"\$out\" | grep -q 'systemd-run failed; no takeover this boot' && [ ! -e '$fb/dir/attempt' ]"
fresh; : > "$fb/dir/attempt"; real
check "real: an attempt left by the last power-on gives this one to Roland's application and is removed" "[ $status = 3 ] && echo \"\$out\" | grep -q 'did not finish' && [ ! -e '$fb/dir/attempt' ] && [ ! -e '$fb/b/ran' ]"
fresh; : > "$fb/dir/dry-run"; real
check "real: the dry-run file logs the decision and schedules nothing" "[ $status = 3 ] && grep -q 'a dry run' '$fb/dir/boot.log' && [ ! -e '$fb/b/ran' ] && [ ! -e '$fb/dir/attempt' ]"
rm -f "$fb/dir/dry-run"
fresh; real 0
check "real: TEMPO held, nothing scheduled" "[ $status = 3 ] && [ ! -e '$fb/b/ran' ]"
gosh() { sh -c '[ -x "$1" ] && "$1" && [ -e "$2/oscairn-boot-started" ] && exit 0; echo mi078' sh "$1" "$fb/run" 2>/dev/null; }
fresh
check "go.sh: the takeover scheduled, mi078 runs" "[ \"\$(OSCAIRN_KEY_HELD=\"sh -c 'exit 1'\" OSCAIRN_PROC='$fb/proc' OSCAIRN_BOOT_DIR='$fb/dir' OSCAIRN_BOOT_RUN='$fb/run' OSCAIRN_SYSTEMD_RUN='$fb/b/systemd-run' gosh '$fb/b/oscairn-fantom-boot' | tail -n 1)\" = mi078 ]"
check "go.sh: no bundle runs mi078" "[ \"\$(gosh '$fb/none/oscairn-fantom-boot')\" = mi078 ]"
printf '#!/bin/sh\nexit 0\n' > "$fb/zero"; chmod +x "$fb/zero"; fresh
check "go.sh: exit 0 without having started Oscairn runs mi078" "[ \"\$(gosh '$fb/zero')\" = mi078 ]"
: > "$fb/run/oscairn-boot-started"
check "go.sh: exit 0 once Oscairn ran starts nothing" "[ -z \"\$(gosh '$fb/zero')\" ]"

# --takeover for real on the stand-in /proc: mi078 as pid 500, its hold on the panel a link.
mi078() { mkdir -p "$fb/proc/500/fd" && echo mi078 > "$fb/proc/500/comm"; [ -z "${1:-}" ] || ln -sf "$OSCAIRN_PANEL_DEVICE" "$fb/proc/500/fd/3"; }
tk() { rm -f "$fb/b/ran"; : > "$fb/dir/attempt"; OSCAIRN_KEY_HELD="sh -c 'exit ${1:-1}'" OSCAIRN_PROC="$fb/proc" OSCAIRN_BOOT_DIR="$fb/dir" OSCAIRN_BOOT_RUN="$fb/run" OSCAIRN_BOOT_LIMIT=1 OSCAIRN_BOOT_POLL=0.1 OSCAIRN_TAKEOVER_SETTLE=0 OSCAIRN_TAKEOVER_WAIT=${2:-5} "$fb/b/oscairn-fantom-boot" --takeover > "$fb/tk.out" 2>&1; }
until_done() { i=0; while kill -0 $1 2>/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done; ! kill -0 $1 2>/dev/null; }
fresh; tk & watcher=$!
sleep 0.5; mi078; sleep 1.5
check "takeover: waits while mi078 does not hold the panel" "kill -0 $watcher && [ ! -e '$fb/b/ran' ]"
mi078 fd
i=0; until grep -q 'Oscairn runs' "$fb/tk.out" || [ $i -ge 50 ]; do sleep 0.1; i=$((i + 1)); done
check "takeover: once mi078 holds the panel, the warm start (no --cold), attempt removed, the marker left, staying while the engine runs" "kill -0 $watcher && [ \"\$(cat '$fb/b/ran')\" = start ] && [ ! -e '$fb/dir/attempt' ] && [ -e '$fb/run/oscairn-boot-started' ]"
rm -rf "$fb/proc/4242"
check "takeover: the engine ended unasked: the stop, never mi078" "until_done $watcher && grep -q 'stop --panel' '$fb/b/ran' && grep -q 'cannot be started again this boot' '$fb/tk.out' && ! grep -q 'go.sh runs' '$fb/tk.out'"
fresh; mi078 fd; : > "$fb/b/fails"; tk; status=$?
check "takeover: a failed warm start runs the stop and does not start mi078" "[ $status = 0 ] && grep -q 'stop --panel' '$fb/b/ran' && grep -q 'cannot be started again this boot' '$fb/tk.out' && ! grep -q 'go.sh runs' '$fb/tk.out' && [ ! -e '$fb/dir/attempt' ]"
fresh; mi078 fd; : > "$fb/b/hang"; tk; status=$?
check "takeover: a warm start that hangs is ended after its time, then the stop" "[ $status = 0 ] && grep -q 'ran out of its 1 seconds' '$fb/tk.out' && grep -q 'stop --panel' '$fb/b/ran'"
fresh; mi078 fd; : > "$fb/b/fails-early"; tk; status=$?
check "takeover: a start failing before Roland's application stopped leaves it, no stop" "[ $status = 0 ] && grep -q 'was not stopped; Roland.s application stays' '$fb/tk.out' && ! grep -q stop '$fb/b/ran' && [ ! -e '$fb/dir/attempt' ]"
fresh; mi078 fd; tk 0; status=$?
check "takeover: TEMPO held at takeover time leaves Roland's application" "[ $status = 0 ] && grep -q 'TEMPO held or unreadable at takeover' '$fb/tk.out' && [ ! -e '$fb/b/ran' ] && [ ! -e '$fb/dir/attempt' ]"
fresh; tk 1 1; status=$?
check "takeover: mi078 never starting: gives up, attempt removed" "[ $status = 0 ] && grep -q 'mi078 did not start within 1 seconds' '$fb/tk.out' && [ ! -e '$fb/b/ran' ] && [ ! -e '$fb/dir/attempt' ]"
fresh; out=$(OSCAIRN_KEY_HELD="sh -c 'exit 1'" OSCAIRN_PROC="$fb/proc" OSCAIRN_BOOT_DIR="$fb/dir" OSCAIRN_BOOT_RUN="$fb/run" "$bundle/oscairn-fantom-boot" --takeover --dry-run 2>&1); status=$?
check "takeover --dry-run: the waits and the warm start's dry run, nothing changed" "[ $status = 0 ] && echo \"\$out\" | grep -q 'would wait (at most 60 s) for mi078 to run' && echo \"\$out\" | grep -q 'oscairn-fantom-start: would run' && ! echo \"\$out\" | grep -q -e '--cold' && [ ! -e '$fb/run/oscairn-boot-started' ]"

# Step 4, the boot script staying in the foreground (--supervise, dry), on a stand-in /proc.
fake=$bundle/fakeproc
running() { mkdir -p "$fake/$2" && echo "$1" > "$fake/$2/comm"; }
gone() { rm -rf "$fake/$1"; }
supervise() { OSCAIRN_PROC=$fake OSCAIRN_BOOT_POLL=0.1 "$bundle/oscairn-fantom-boot" --dry-run --supervise > "$bundle/watch.out" 2>&1 & watcher=$!; }
check "the takeover's dry run says it would stay in the foreground" "OSCAIRN_KEY_HELD=\"sh -c 'exit 1'\" OSCAIRN_PROC='$bundle/proc' '$bundle/oscairn-fantom-boot' --takeover --dry-run 2>&1 | grep -q 'stay in the foreground while Oscairn runs'"
running oscairn-engine 4242
supervise
sleep 0.5
check "the engine running: boot stays" "kill -0 $watcher && grep -q 'Oscairn runs; staying in the foreground' '$bundle/watch.out'"
gone 4242
check "the engine ended without oscairn-fantom-stop: the fallback, then Roland's application" "until_done $watcher && grep -q 'ended without oscairn-fantom-stop' '$bundle/watch.out' && grep -q 'would run: $bundle/oscairn-fantom-stop --panel' '$bundle/watch.out' && grep -q '$roland' '$bundle/watch.out'"
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

# A start for real, in the stand-in bundle on a stand-in /proc (the panel holder and a display
# already there) and an empty /sys: start-chroot.sh plays the app, which makes its engine's socket
# and ends with status 3 once $bundle/crash exists, and the control request.
live=$bundle/liveproc
mkdir -p "$live/sys/kernel" "$live/net"
printf '7\t4\t1\t7\n' > "$live/sys/kernel/printk"
printf '0: 00000002 0 0 0001 01 1 @oscairn-panel\n0: 00000002 0 0 0001 01 2 @/tmp/.X11-unix/X0\n' > "$live/net/unix"
cat > "$bundle/start-chroot.sh" <<'EOF'
#!/bin/sh
root=$(dirname "$0")
case "$*" in
*oscairn-control-request*) echo '{"jsonrpc":"2.0","id":1,"result":{}}' ;;
*oscairn-tickgen-reset*)
	echo tickgen-reset >> "$root/ran"
	[ ! -e "$root/reset-fails" ] || { echo "tickgen stand-in: refused" >&2; exit 1; }
	echo "tickgen stand-in: reset" ;;
*/usr/bin/Oscairn*)
	echo app started >> "$root/ran"
	mkdir -p "$root/tmp/oscairn-0"
	[ -S "$root/tmp/oscairn-0/engine-1.sock" ] || python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$root/tmp/oscairn-0/engine-1.sock"
	until [ -e "$root/crash" ]; do sleep 0.1; done
	exit 3 ;;
*) echo start-chroot ran >> "$root/ran" ;;
esac
EOF
printf "OSCAIRN_HEADLESS=0\nOSCAIRN_PROFILE=fantom-07\n" >> "$bundle/etc/default/oscairn-engine"
rm -rf "${bundle:?}/var" "${bundle:?}/ran"
log=$bundle/var/oscairn/log/oscairn-fantom.log
wait_for() { i=0; until grep -q "$1" "$log" 2>/dev/null || [ $i -ge $2 ]; do sleep 0.5; i=$((i + 1)); done; }
out=$(OSCAIRN_PROC=$live OSCAIRN_SYS=$bundle/nosys "$bundle/oscairn-fantom-start" 2>&1); status=$?
check "a start for real: the app runs and its engine answers" "[ $status = 0 ] && echo \"\$out\" | grep -q 'the app runs (pid'"
check "the tickgen pids reset before the app started, its output logged" "[ \"\$(grep -e tickgen-reset -e 'app started' '$bundle/ran' | head -n 2 | tr '\\n' ' ')\" = 'tickgen-reset app started ' ] && grep -q 'tickgen stand-in: reset' '$log'"
check "the kernel's console quiet while it runs, the level it had recorded" "[ \"\$(cat '$live/sys/kernel/printk')\" = '4 4 1 7' ] && grep -q '^7.4.1.7$' '$bundle/var/oscairn/run/printk'"
mkdir -p "$live/4242" && echo oscairn-engine > "$live/4242/comm"
: > "$bundle/crash"
wait_for 'a fourth time within a minute' 120
check "the app ended alone, its engine playing: started again, its exit status logged" "grep -q 'the app ended (status 3) while its engine plays on: starting it again' '$log'"
check "a crash loop: started three times more, then left ended" "[ \$(grep -c 'app started' '$bundle/ran') = 4 ] && grep -q 'the app ended (status 3) a fourth time within a minute of starting: not started again' '$log'"
rm -r "${live:?}/4242"
out=$(OSCAIRN_PROC=$live OSCAIRN_SYS=$bundle/nosys "$bundle/oscairn-fantom-stop" 2>&1)
check "the stop puts the kernel's console level back" "grep -q '^7.4.1.7$' '$live/sys/kernel/printk' && [ ! -e '$bundle/var/oscairn/run/printk' ]"
rm -f "$bundle/crash" "$bundle/ran"
: > "$bundle/reset-fails"
out=$(OSCAIRN_PROC=$live OSCAIRN_SYS=$bundle/nosys "$bundle/oscairn-fantom-start" 2>&1); status=$?
check "a failing tickgen reset is logged and the start goes on" "[ $status = 0 ] && grep -q 'oscairn-tickgen-reset failed (status 1); continuing' '$log' && echo \"\$out\" | grep -q 'the app runs (pid'"
: > "$bundle/crash"
wait_for 'its engine is not running' 20
check "the app ended with its engine: not started again" "[ $status = 0 ] && grep -q 'the app ended (status 3) and its engine is not running: not started again' '$log' && [ \$(grep -c 'app started' '$bundle/ran') = 1 ]"
# The handover (keep_ui) on a stand-in bundle and /proc: the Rust UI ends with status 3, the X
# server (a stand-in, serving only after 0.5 s) starts, the app runs once it serves, the X server
# is signaled the moment the app ends, and the Rust UI starts only after it is gone, with the
# touch rule back; the keeper logs each phase with milliseconds.
keep=$bundle/keeper
mkdir -p "$keep/var/oscairn/log" "$keep/var/oscairn/run" "$keep/tmp" "$keep/proc/net" "$keep/rules"
: > "$keep/proc/net/unix"
cat > "$keep/start-chroot.sh" <<'EOF'
#!/bin/sh
k=$(dirname "$0")
case "$*" in
*xwininfo*) [ -e "$k/x-serves" ] ;;
*/usr/bin/Xorg*)
	[ ! -e "$k/rules/99-oscairn-touch.rules" ] && echo "x: no rule" >> "$k/order" || echo "x: RULE" >> "$k/order"
	mkdir -p "$k/proc/$$"; echo Xorg > "$k/proc/$$/comm"
	trap 'rm -rf "$k/proc/$$"; : > "$k/proc/net/unix"; rm -f "$k/x-serves"; sleep 0.3; echo "x: gone" >> "$k/order"; exit 0' TERM
	echo '0: 00000002 0 0 0001 01 2 @/tmp/.X11-unix/X0' > "$k/proc/net/unix"
	( sleep 0.5; : > "$k/x-serves" ) &
	while :; do sleep 0.05; done ;;
*/usr/bin/Oscairn*)
	[ -e "$k/x-serves" ] && echo "app: X serving" >> "$k/order" || echo "app: X NOT serving" >> "$k/order"
	sleep 0.2; exit 0 ;;
*/usr/bin/oscairn-ui*)
	[ -e "$k/rules/99-oscairn-touch.rules" ] && r=rule || r=norule
	grep -q Xorg "$k"/proc/[0-9]*/comm 2>/dev/null && r="$r X-STILL-UP"
	echo "ui: $r" >> "$k/order"
	if [ ! -e "$k/ui-ran" ]; then : > "$k/ui-ran"; echo settings > "$k/tmp/oscairn-handover"; exit 3; fi
	: > "$k/var/oscairn/run/stopped"; exit 0 ;;
esac
EOF
chmod +x "$keep/start-chroot.sh"
(
	root=$keep state=$keep/var/oscairn proc=$keep/proc log=$keep/var/oscairn/log/oscairn-fantom.log rotation=90 rules=$keep/rules matrix="0 1 0 -1 0 1" dry= OSCAIRN_PROFILE=fantom-07
	for f in say touch_rule keep_ui display_serves; do eval "$(sed -n "/^$f()/,/^}/p" "$scripts/oscairn-fantom-start")"; done
	for f in fnap now_ms lap display_up named xorg_ours handed_over nap; do eval "$(grep "^$f()" "$scripts/oscairn-fantom-start")"; done
	touch_rule
	keep_ui
) > "$keep/keeper.out" 2>&1
klog=$keep/var/oscairn/log/oscairn-fantom.log
lapms() { sed -n "s/.*timing $1[^:]*: ms=[0-9]* (+\\([0-9]*\\) ms).*/\\1/p" "$klog"; }
check "the handover: X before the app, X gone before the Rust UI, the rule off for X and back for the UI" "[ \"\$(tr '\\n' ' ' < '$keep/order')\" = 'ui: rule x: no rule app: X serving x: gone ui: rule ' ]"
check "the keeper waited on the X server serving (0.5 s in the stand-in), not a fixed time" "[ \$(lapms 'X ready') -ge 500 ] && [ \$(lapms 'X ready') -lt 3000 ]"
check "each phase logged with milliseconds, in order" "[ \"\$(sed -n 's/.*timing \\([A-Za-z]* [A-Za-z]*\\).*/\\1/p' '$klog' | tr '\\n' ',')\" = 'handover requested,X ready,app started,app exited,X gone,shell started,' ]"
check "the X server waited for to end (its 0.3 s), not given up on" "[ \$(( \$(lapms 'X gone') - \$(lapms 'app exited') )) -ge 280 ]"
exit $fail
