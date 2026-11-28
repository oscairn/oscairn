#!/bin/sh
# Runs inside the build container (os/build.sh). /src is the Oscairn checkout (read only);
# /work is a Docker volume holding Buildroot, downloads, ccache and outputs.
set -eu
defconfig=$1
shift
BUILDROOT_VERSION=2026.08.1

cd /work
if [ ! -d buildroot ]; then
	git clone -q --depth 1 --branch "$BUILDROOT_VERSION" https://gitlab.com/buildroot.org/buildroot.git buildroot
fi

# Snapshot the files git knows about (committed or not, ignoring build output), so the
# image builds from the working tree without its build folders.
rm -rf src && mkdir src
git config --global --add safe.directory /src
git -C /src ls-files -z --cached --others --exclude-standard | rsync -a --from0 --files-from=- /src/ src/

out=/work/output/$defconfig
export BR2_DL_DIR=/work/dl BR2_CCACHE_DIR=/work/ccache
make -C buildroot O="$out" BR2_EXTERNAL=/work/src/os/buildroot "$defconfig"

# OSCAIRN_BASE_MODE=base (os/base-image.sh): build the base, everything the configuration builds but Oscairn's own
# packages and the root filesystem, and prune the output to what a later build of those needs (docs/releasing.md,
# "A prebuilt base for the bundle"). The same defconfig and .config as the bundle's, so no package differs.
if [ "${OSCAIRN_BASE_MODE:-}" = base ]; then
	# A base holds no Oscairn: a volume that built the bundle has its packages' stamps (and files in target/), which
	# a later build on the base would take for built.
	if ls -d "$out"/build/oscairn-* > /dev/null 2>&1; then
		echo "base: $out already built Oscairn's packages; build the base in a fresh OSCAIRN_OS_VOLUME" >&2
		exit 1
	fi
	targets=$(make -s -C buildroot O="$out" show-targets | tr ' ' '\n' | grep -v '^$' | grep -v -e '^oscairn' -e '^rootfs-' | tr '\n' ' ')
	echo "base: building $(echo "$targets" | wc -w) targets"
	make -C buildroot O="$out" "$@" $targets
	if [ "${OSCAIRN_BASE_PRUNE:-1}" = 1 ]; then
		# Buildroot finds a package built by its stamps and file list, and needs neither its sources nor its
		# objects again: keep those and host/, staging/, target/ whole. (The first thing to check, if a build on
		# a pruned base builds a base package again: OSCAIRN_BASE_PRUNE=0 keeps build/ whole.)
		for d in "$out"/build/*/; do
			case $d in */buildroot-*) continue ;; esac
			find "$d" -mindepth 1 ! -name '.stamp_*' ! -name '.files-list*' -delete 2> /dev/null || true
		done
		du -sh "$out"/build "$out"/host "$out"/staging "$out"/target 2> /dev/null || true
	fi
	exit 0
fi

make -C buildroot O="$out" "$@"
