include $(sort $(wildcard $(BR2_EXTERNAL_OSCAIRN_PATH)/package/*/*.mk))

# Buildroot's libcurl names a CA bundle for OpenSSL and wolfSSL but not for Mbed TLS, and curl's
# configure does not look for one when cross-compiling, so every certificate would be refused.
ifeq ($(BR2_PACKAGE_LIBCURL_MBEDTLS),y)
LIBCURL_CONF_OPTS += --with-ca-bundle=/etc/ssl/certs/ca-certificates.crt
endif

# Expat salts each parser's hash from arc4random_buf, else getentropy: both block in getrandom
# until the kernel's random pool is ready, about 70 s after power-on on a FANTOM (Linux 4.19),
# and fontconfig's first parse held the Rust UI that long. Without them Expat takes getrandom's
# non-blocking form, then /dev/urandom.
EXPAT_CONF_OPTS += --without-arc4random-buf --without-arc4random --without-getentropy
