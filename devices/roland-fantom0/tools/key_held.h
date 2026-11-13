/* SPDX-License-Identifier: AGPL-3.0-or-later */
#pragma once
/* Whether key `code` is down in a key-state bit array as EVIOCGKEY fills it (bit code % 8 of byte
 * code / 8). A code past the array is not down. */
#include <stddef.h>

static inline int key_down(const unsigned char* bits, size_t size, unsigned code)
{
    return code / 8 < size && (bits[code / 8] >> (code % 8) & 1);
}
