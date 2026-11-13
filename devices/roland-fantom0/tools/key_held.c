/* SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * oscairn-key-held DEVICE CODE: whether a key is held right now, for oscairn-fantom-boot to read
 * MENU on the FANTOM-0 (gpio-keys, code 139; docs/fantom-0.md, "Starting at power-on"). DEVICE is
 * an input device's path, or its name ("gpio-keys"), looked for among /dev/input/event*. It asks
 * the key state once (EVIOCGKEY), read-only, without grabbing the device. Exits 0 when the key is
 * down, 1 when it is up, 2 on any error. Plain C, no dependencies. */
#include "key_held.h"

#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

static int open_named(const char* name)
{
    DIR* dir = opendir("/dev/input");
    if(!dir) return -1;
    struct dirent* e;
    int found = -1;
    while(found < 0 && (e = readdir(dir)))
    {
        if(strncmp(e->d_name, "event", 5) != 0) continue;
        char path[300], got[256] = {0};
        snprintf(path, sizeof path, "/dev/input/%s", e->d_name);
        const int fd = open(path, O_RDONLY | O_CLOEXEC);
        if(fd < 0) continue;
        if(ioctl(fd, EVIOCGNAME(sizeof got - 1), got) >= 0 && strcmp(got, name) == 0) found = fd;
        else close(fd);
    }
    closedir(dir);
    return found;
}

int main(int argc, char** argv)
{
    if(argc != 3) { fprintf(stderr, "usage: oscairn-key-held DEVICE|NAME CODE\n"); return 2; }
    char* end;
    const unsigned long code = strtoul(argv[2], &end, 10);
    if(*end || code > KEY_MAX) { fprintf(stderr, "oscairn-key-held: no key code %s\n", argv[2]); return 2; }
    const int fd = argv[1][0] == '/' ? open(argv[1], O_RDONLY | O_CLOEXEC) : open_named(argv[1]);
    if(fd < 0) { fprintf(stderr, "oscairn-key-held: cannot open %s\n", argv[1]); return 2; }
    unsigned char bits[KEY_MAX / 8 + 1] = {0};
    const int ok = ioctl(fd, EVIOCGKEY(sizeof bits), bits) >= 0;
    close(fd);
    if(!ok) { perror("oscairn-key-held: EVIOCGKEY"); return 2; }
    return key_down(bits, sizeof bits, (unsigned)code) ? 0 : 1;
}
