#!/bin/sh
# Runs after the target filesystem is assembled. $1 is the target directory.
set -eu
target=$1

# The Oscairn session starts its own X server (/usr/libexec/oscairn-session); Buildroot's
# X.org package would start a second one.
rm -f "$target/etc/init.d/S40xorg"

# The image must agree with the device profile it starts the app with (RFC 0035, optional
# hardware): nothing of a feature the profile lacks, everything of one it declares.
here=$(cd "$(dirname "$0")" && pwd)
profile=$(sed -n 's/.*--profile \([A-Za-z0-9._-]*\).*/\1/p' "$target/etc/default/oscairn" 2>/dev/null | tail -n 1)
kernel=$(ls "$BUILD_DIR"/linux-[0-9]*/.config 2>/dev/null | head -n 1)
if [ -n "$profile" ]; then
	python3 "$here/../../../scripts/check-image-profile.py" --br2-config "$BR2_CONFIG" \
		--profile "$here/../../../../hosts/standalone/profiles/$profile.json" \
		${kernel:+--kernel-config "$kernel"} --target "$target"
	# The profile's audio cores (RFC 0035): for the session (oscairn-session), and nohz_full= and
	# irqaffinity= for the kernel: in extlinux.conf where the board boots with it, and in
	# kernel-args beside the images for the QEMU scripts, which give the command line themselves.
	python3 "$here/../../../scripts/audio-cores.py" "$here/../../../../hosts/standalone/profiles/$profile.json" \
		> "$target/etc/default/oscairn-cores"
	# What the session runs the engine with when there is no display (headless).
	python3 "$here/../../../scripts/engine-args.py" "$here/../../../../hosts/standalone/profiles/$profile.json" \
		> "$target/etc/default/oscairn-engine"
	# A board's own defaults for it (the FANTOM bundle's overlay), after the profile's.
	if [ -f "$target/etc/default/oscairn-engine-device" ]; then
		cat "$target/etc/default/oscairn-engine-device" >> "$target/etc/default/oscairn-engine"
		rm -f "$target/etc/default/oscairn-engine-device"
	fi
	args=$(sed -n 's/^OSCAIRN_KERNEL_ARGS="\(.*\)"$/\1/p' "$target/etc/default/oscairn-cores")
	printf '%s\n' "$args" > "$BINARIES_DIR/kernel-args"
	extlinux=$target/boot/extlinux/extlinux.conf
	if [ -f "$extlinux" ]; then
		sed -i -e '/^ *append /s/ nohz_full=[^ ]*//; /^ *append /s/ irqaffinity=[^ ]*//' \
			-e "/^ *append /s/\$/ $args/" -e '/^ *append /s/ *$//' "$extlinux"
	fi
fi
