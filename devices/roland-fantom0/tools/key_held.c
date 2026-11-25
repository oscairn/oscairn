/* SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * oscairn-key-held DEVICE CODE [PRESS-MS]: whether a key is held right now, or pressed within PRESS-MS
 * milliseconds, for oscairn-fantom-boot to read TEMPO on the FANTOM-0 (gpio-keys, code 152;
 * docs/fantom-0.md, "Starting at power-on"). DEVICE is an input device's path, or its name
 * ("gpio-keys"), looked for among /dev/input/event*. It asks the key state once (EVIOCGKEY), then,
 * with PRESS-MS, reads the device's key events until the key goes down or the time is up; read-only,
 * without grabbing the device. Exits 0 when the key is down (or went down), 1 when it is up (or was
 * not pressed), 2 on any error. Plain C, no dependencies. */
#include "key_held.h"

#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
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

static long long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}

/* The key goes down within `ms`: 0; not: 1; an error: 2. Events queue from the open on, so none since
 * the state was asked is missed. */
static int pressed_within(int fd, unsigned code, long ms)
{
    const long long end = now_ms() + ms;
    for(;;)
    {
        const long long left = end - now_ms();
        if(left <= 0) return 1;
        struct pollfd p = {fd, POLLIN, 0};
        const int n = poll(&p, 1, (int)left);
        if(n < 0) { perror("oscairn-key-held: poll"); return 2; }
        if(n == 0) return 1;
        struct input_event ev[16];
        const ssize_t got = read(fd, ev, sizeof ev);
        if(got < (ssize_t)sizeof ev[0]) { perror("oscairn-key-held: read"); return 2; }
        for(size_t i = 0; i < (size_t)got / sizeof ev[0]; ++i)
            if(ev[i].type == EV_KEY && ev[i].code == code && ev[i].value == 1) return 0;
    }
}

int main(int argc, char** argv)
{
    if(argc != 3 && argc != 4) { fprintf(stderr, "usage: oscairn-key-held DEVICE|NAME CODE [PRESS-MS]\n"); return 2; }
    char* end;
    const unsigned long code = strtoul(argv[2], &end, 10);
    if(*end || code > KEY_MAX) { fprintf(stderr, "oscairn-key-held: no key code %s\n", argv[2]); return 2; }
    long ms = -1;
    if(argc == 4)
    {
        ms = strtol(argv[3], &end, 10);
        if(*end || ms < 0 || ms > 600000) { fprintf(stderr, "oscairn-key-held: no time %s\n", argv[3]); return 2; }
    }
    const int fd = argv[1][0] == '/' ? open(argv[1], O_RDONLY | O_CLOEXEC) : open_named(argv[1]);
    if(fd < 0) { fprintf(stderr, "oscairn-key-held: cannot open %s\n", argv[1]); return 2; }
    unsigned char bits[KEY_MAX / 8 + 1] = {0};
    const int ok = ioctl(fd, EVIOCGKEY(sizeof bits), bits) >= 0;
    if(!ok) { perror("oscairn-key-held: EVIOCGKEY"); close(fd); return 2; }
    int r = key_down(bits, sizeof bits, (unsigned)code) ? 0 : 1;
    if(r == 1 && ms >= 0) r = pressed_within(fd, (unsigned)code, ms);
    close(fd);
    return r;
}
