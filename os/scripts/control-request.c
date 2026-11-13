/* SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * control-request SOCKET REQUEST...: sends each JSON-RPC request line to an Oscairn control socket
 * and prints each response line. boot-test.py builds it for the image and types it into the guest
 * to read session.status there; it is not part of an image, but for the FANTOM-0's bundle, whose
 * scripts ask the engine for session.status with it (devices/roland-fantom0, as
 * oscairn-control-request). */
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

int main(int argc, char** argv)
{
    if(argc < 3) { fprintf(stderr, "usage: control-request SOCKET REQUEST...\n"); return 2; }
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    strncpy(address.sun_path, argv[1], sizeof(address.sun_path) - 1);
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    if(s < 0 || connect(s, (struct sockaddr*)&address, sizeof(address)) != 0) { perror(argv[1]); return 1; }
    static char line[1 << 20];
    for(int i = 2; i < argc; ++i)
    {
        if(write(s, argv[i], strlen(argv[i])) < 0 || write(s, "\n", 1) != 1) { perror("write"); return 1; }
        size_t used = 0; /* one response: up to its newline; notifications without an id are skipped */
        for(;;)
        {
            ssize_t n = read(s, line + used, 1);
            if(n <= 0) { fprintf(stderr, "connection closed\n"); return 1; }
            if(line[used] != '\n') { if(used + 1 < sizeof(line)) ++used; continue; }
            line[used] = 0;
            if(strstr(line, "\"id\"")) break;
            used = 0;
        }
        puts(line);
    }
    return 0;
}
