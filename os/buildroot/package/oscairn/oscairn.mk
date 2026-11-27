################################################################################
#
# oscairn
#
################################################################################

# Built from the Oscairn source tree this external tree belongs to.
OSCAIRN_VERSION = local
OSCAIRN_SITE = $(BR2_EXTERNAL_OSCAIRN_PATH)/../..
OSCAIRN_SITE_METHOD = local
OSCAIRN_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = --exclude '/build*' --exclude '/demo'
OSCAIRN_LICENSE = AGPL-3.0-or-later, MIT (CLAP), MIT (oscairn-clap headers), MIT (miniz), BSD-2-Clause or CC0-1.0 (Monocypher), \
	Unlicense (json.h), MIT (samplepack, in the oscairn-samplepack package's licenses), \
	the Rust UI's crates (in the oscairn-ui-crates package's licenses)
OSCAIRN_LICENSE_FILES = LICENSE core/clap-host/third_party/clap/LICENSE core/clap-ext/LICENSE \
	core/control/third_party/miniz/LICENSE core/control/third_party/monocypher/LICENCE.md \
	core/control/third_party/sheredom/json.h
OSCAIRN_DEPENDENCIES = host-pkgconf oscairn-samplepack
# Nothing is downloaded while building.
OSCAIRN_CONF_OPTS += -DFETCHCONTENT_FULLY_DISCONNECTED=ON
# samplepack comes from its own package and is built with the Rust release the build container
# installs (os/docker/Dockerfile: rustup in /opt/rust, RUSTUP_HOME and CARGO_HOME set there),
# for the target's triple; OSCAIRN_CARGO names another cargo of that release.
OSCAIRN_CARGO ?= /opt/rust/cargo/bin/cargo
OSCAIRN_CONF_OPTS += -DFETCHCONTENT_SOURCE_DIR_SAMPLEPACK=$(OSCAIRN_SAMPLEPACK_SRCDIR) -DSAMPLEPACK_CARGO=$(OSCAIRN_CARGO)
# The Rust crates (samplepack, the engine core in core/engine/rust) for the target's CPU, as the C and C++
# are built for it; release builds always (their own opt-level, whatever OSCAIRN_AUDIO_O3 says).
ifneq ($(call qstrip,$(BR2_GCC_TARGET_CPU)),)
OSCAIRN_CONF_OPTS += -DSAMPLEPACK_RUSTFLAGS="-C;target-cpu=$(call qstrip,$(BR2_GCC_TARGET_CPU))"
endif

ifeq ($(BR2_PACKAGE_OSCAIRN_LV2),y)
OSCAIRN_DEPENDENCIES += lilv
OSCAIRN_LICENSE += , ISC
OSCAIRN_LICENSE_FILES += core/lv2-host/third_party/jalv/ISC.txt
OSCAIRN_CONF_OPTS += -DOSCAIRN_LV2=ON
else
OSCAIRN_CONF_OPTS += -DOSCAIRN_LV2=OFF
endif

ifeq ($(BR2_PACKAGE_OSCAIRN_ROLAND_FANTOM0),y)
OSCAIRN_CONF_OPTS += -DOSCAIRN_ROLAND_FANTOM0=ON
# Its start, stop and boot scripts, run from the bundle's folder outside the chroot, beside
# start-chroot.sh (devices/roland-fantom0/bundle), and the splash the start script shows on the
# console's frame buffer, turned for the panel (Buildroot needs python3 on the build host).
define OSCAIRN_INSTALL_FANTOM0_SCRIPTS
	$(INSTALL) -D -m 0755 $(@D)/devices/roland-fantom0/bundle/oscairn-fantom-start $(TARGET_DIR)/oscairn-fantom-start
	$(INSTALL) -D -m 0755 $(@D)/devices/roland-fantom0/bundle/oscairn-fantom-stop $(TARGET_DIR)/oscairn-fantom-stop
	$(INSTALL) -D -m 0755 $(@D)/devices/roland-fantom0/bundle/oscairn-fantom-boot $(TARGET_DIR)/oscairn-fantom-boot
	mkdir -p $(TARGET_DIR)/usr/share/oscairn
	python3 $(@D)/devices/roland-fantom0/tools/splash_raw.py $(@D)/branding/splash.png $(TARGET_DIR)/usr/share/oscairn/splash-720x1280.bgrx.gz
endef
OSCAIRN_POST_INSTALL_TARGET_HOOKS += OSCAIRN_INSTALL_FANTOM0_SCRIPTS
else
OSCAIRN_CONF_OPTS += -DOSCAIRN_ROLAND_FANTOM0=OFF
endif

ifeq ($(BR2_PACKAGE_OSCAIRN_ENGINE),y)
OSCAIRN_DEPENDENCIES += oscairn-juce alsa-lib libcurl
# JUCE comes from the oscairn-juce package: nothing is downloaded while building.
OSCAIRN_CONF_OPTS += \
	-DOSCAIRN_ENGINE=ON \
	-DOSCAIRN_AUDIO_BUFFER_FRAMES=128 \
	-DOSCAIRN_AUDIO_O3=ON \
	-DFETCHCONTENT_SOURCE_DIR_JUCE=$(OSCAIRN_JUCE_SRCDIR)
endif

ifeq ($(BR2_PACKAGE_OSCAIRN_STANDALONE),y)
# The built-in themes carry the Inter font.
OSCAIRN_LICENSE += , OFL-1.1
OSCAIRN_LICENSE_FILES += hosts/standalone/themes/oscairn-dark/fonts/OFL.txt
OSCAIRN_DEPENDENCIES += freetype fontconfig \
	xlib_libX11 xlib_libXext xlib_libXinerama xlib_libXrandr \
	xlib_libXcursor xlib_libXrender
OSCAIRN_CONF_OPTS += -DOSCAIRN_STANDALONE=ON
endif

# The Rust UI (ui/, docs/rust-ui.md), built by cargo at the release above from the crates the
# oscairn-ui-crates package downloaded, offline; linked against the image's libudev, libinput,
# libxkbcommon and fontconfig. Installed as /usr/bin/oscairn-ui.
ifeq ($(BR2_PACKAGE_OSCAIRN_UI),y)
OSCAIRN_DEPENDENCIES += oscairn-ui-crates udev libinput libxkbcommon fontconfig
OSCAIRN_CONF_OPTS += -DOSCAIRN_UI=ON -DOSCAIRN_UI_VENDOR=$(OSCAIRN_UI_CRATES_SRCDIR)/vendor
else
OSCAIRN_CONF_OPTS += -DOSCAIRN_UI=OFF
endif

# The kernel follows the options (RFC 0035). REALTIME_KERNEL makes it real-time (step 2; nothing
# in Oscairn changes with it). A board's kernel config lists all of its hardware; an option that
# is off takes that subsystem out (its drivers depend on it), so nothing of it is left behind:
# the display without the app, the USB gadget without USB_GADGET.
define OSCAIRN_LINUX_CONFIG_FIXUPS
	$(if $(BR2_PACKAGE_OSCAIRN_REALTIME_KERNEL),$(call KCONFIG_ENABLE_OPT,CONFIG_EXPERT))
	$(if $(BR2_PACKAGE_OSCAIRN_REALTIME_KERNEL),$(call KCONFIG_ENABLE_OPT,CONFIG_PREEMPT_RT))
	$(if $(BR2_PACKAGE_OSCAIRN_STANDALONE),,$(call KCONFIG_DISABLE_OPT,CONFIG_DRM))
	$(if $(BR2_PACKAGE_OSCAIRN_STANDALONE),,$(call KCONFIG_DISABLE_OPT,CONFIG_FB))
	$(if $(BR2_PACKAGE_OSCAIRN_USB_GADGET),,$(call KCONFIG_DISABLE_OPT,CONFIG_USB_GADGET))
endef

# JUCE's modules compile as a few very large translation units, over a gigabyte of memory
# each; a job per core can run out of memory on an 8 GB build machine.
OSCAIRN_MAKE_OPTS = -j4

$(eval $(cmake-package))
