################################################################################
#
# oscairn-samplepack
#
################################################################################

# Source only: the oscairn package builds the static library (cmake/SamplePack.cmake, cargo
# offline). The commit is OSCAIRN_SAMPLEPACK_COMMIT in Oscairn's CMakeLists.txt.
OSCAIRN_SAMPLEPACK_VERSION = c87aa70d2aec4445e70140172e7dfad038ae31d0
OSCAIRN_SAMPLEPACK_SITE = $(call github,oscairn,samplepack,$(OSCAIRN_SAMPLEPACK_VERSION))
OSCAIRN_SAMPLEPACK_LICENSE = MIT
OSCAIRN_SAMPLEPACK_LICENSE_FILES = LICENSE
OSCAIRN_SAMPLEPACK_INSTALL_TARGET = NO

$(eval $(generic-package))
