################################################################################
#
# oscairn-ui-crates
#
################################################################################

# Source only: the crates oscairn-ui (ui/) builds with, every one ui/Cargo.lock names, downloaded
# from crates.io at the download step and checked against the lock's checksums
# (oscairn-ui-crates.hash), unpacked as a cargo directory source the oscairn package builds the UI
# from offline (OSCAIRN_UI_VENDOR). crates.inc and the hashes come from os/scripts/ui-crates.py,
# run again after the lock changes (the ui-crates test says when). Nothing is installed on the target.
OSCAIRN_UI_CRATES_VERSION = 1
OSCAIRN_UI_CRATES_SOURCE =
include $(pkgdir)/crates.inc
OSCAIRN_UI_CRATES_INSTALL_TARGET = NO

# Each crate unpacked to vendor/<name>-<version>, with the checksum cargo checks a directory
# source's package by (its files are not listed: the download's hash covered them).
define OSCAIRN_UI_CRATES_EXTRACT_CMDS
	mkdir -p $(@D)/vendor
	for crate in $(notdir $(OSCAIRN_UI_CRATES_EXTRA_DOWNLOADS)); do \
		$(TAR) -xzf $(OSCAIRN_UI_CRATES_DL_DIR)/$$crate -C $(@D)/vendor || exit 1; \
		sum=$$(sed -n "s/^sha256  \([0-9a-f]*\)  $$crate$$/\1/p" $(OSCAIRN_UI_CRATES_PKGDIR)/oscairn-ui-crates.hash); \
		printf '{"files":{},"package":"%s"}' "$$sum" > $(@D)/vendor/$${crate%.crate}/.cargo-checksum.json; \
	done
endef

$(eval $(generic-package))
