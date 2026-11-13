/* SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * oscairn-key-held's bit lookup (key_held.h): MENU (139) and its neighbors in a key-state array,
 * and a code past the array. The ioctl itself needs a real input device. */
#include "../tools/key_held.h"

#include <stdio.h>
#include <string.h>

int main(void)
{
    unsigned char bits[96];
    memset(bits, 0, sizeof bits);
    bits[139 / 8] |= 1u << (139 % 8); /* MENU down */
    int failures = 0;
    if(!key_down(bits, sizeof bits, 139)) { puts("FAIL: MENU down"); ++failures; }
    if(key_down(bits, sizeof bits, 138) || key_down(bits, sizeof bits, 140) || key_down(bits, sizeof bits, 157)) { puts("FAIL: only MENU"); ++failures; }
    bits[157 / 8] |= 1u << (157 % 8); /* WRITE too */
    if(!key_down(bits, sizeof bits, 157) || !key_down(bits, sizeof bits, 139)) { puts("FAIL: WRITE and MENU"); ++failures; }
    if(key_down(bits, 8, 139)) { puts("FAIL: a code past the array"); ++failures; }
    if(failures) return 1;
    puts("ok: key_down");
    return 0;
}
