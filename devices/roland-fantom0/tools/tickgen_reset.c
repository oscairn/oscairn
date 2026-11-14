/* SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * oscairn-tickgen-reset [--dry-run] [DEVICE]: clears the two pids Roland's tickgen module sends
 * signal 42 to every millisecond, which it keeps after Roland's application has ended
 * (docs/fantom-0.md, "Signal 42"). Opens DEVICE (default /proc/tickgen) read-only and sets each
 * pid to -1, the value the module loads with, with one ioctl each; a reboot, or Roland's
 * application registering itself at its next start, undoes it. Exits 0 when done or when DEVICE
 * is not there (another device, or a module not loaded), 1 on any other error, 2 on bad usage.
 * --dry-run prints what it would do and issues no ioctl. Plain C, no dependencies. */
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <unistd.h>

/* From the module's disassembly (docs/fantom-0.md, "Signal 42"): each takes a pointer to an int,
 * copied with copy_from_user and stored as is; neither does anything else. */
static const char* const tickgenDevice = "/proc/tickgen";
static const unsigned long tickgenSetPid = 0x80044B0EUL; /* u_pid */
static const unsigned long tickgenSetDebugPid = 0x80044B0FUL; /* u_dbg */

int main(int argc, char** argv)
{
    int dry = 0;
    const char* device = tickgenDevice;
    for(int i = 1; i < argc; ++i)
    {
        if(strcmp(argv[i], "--dry-run") == 0) dry = 1;
        else if(argv[i][0] != '-' && device == tickgenDevice) device = argv[i];
        else { fprintf(stderr, "usage: oscairn-tickgen-reset [--dry-run] [DEVICE]\n"); return 2; }
    }
    struct stat st;
    if(stat(device, &st) != 0)
    {
        if(errno == ENOENT) { printf("oscairn-tickgen-reset: no %s: nothing to reset\n", device); return 0; }
        fprintf(stderr, "oscairn-tickgen-reset: %s: %s\n", device, strerror(errno));
        return 1;
    }
    const struct { unsigned long command; const char* name; } pids[] = {
        { tickgenSetPid, "u_pid" }, { tickgenSetDebugPid, "u_dbg" }};
    if(dry)
    {
        for(int i = 0; i < 2; ++i)
            printf("oscairn-tickgen-reset: would set %s to -1: ioctl(%s, 0x%08lX) with a pointer to -1\n", pids[i].name, device, pids[i].command);
        return 0;
    }
    const int fd = open(device, O_RDONLY | O_CLOEXEC);
    if(fd < 0)
    {
        fprintf(stderr, "oscairn-tickgen-reset: cannot open %s: %s\n", device, strerror(errno));
        return 1;
    }
    int failed = 0;
    for(int i = 0; i < 2; ++i)
    {
        int unset = -1;
        const int r = ioctl(fd, pids[i].command, &unset);
        const int e = errno;
        /* Anything but 0 is a failure: the module returns a positive count on a partial copy. */
        if(r == 0) printf("oscairn-tickgen-reset: %s set to -1: ioctl(%s, 0x%08lX) returned 0\n", pids[i].name, device, pids[i].command);
        else
        {
            fprintf(stderr, "oscairn-tickgen-reset: %s: ioctl(%s, 0x%08lX) returned %d%s%s\n", pids[i].name, device, pids[i].command, r,
                    r < 0 ? ": " : "", r < 0 ? strerror(e) : "");
            failed = 1;
        }
    }
    close(fd);
    return failed;
}
