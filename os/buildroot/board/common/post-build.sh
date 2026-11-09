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
fi
