/* SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * channel-peaks FILE CHANNELS: reads raw interleaved 32-bit float audio of CHANNELS channels and
 * prints each channel's peak (absolute) value and the frame count. alsa-slots-test.py builds it
 * for the image and types it into the guest; it is not part of the image. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char** argv)
{
    if(argc != 3) { fprintf(stderr, "usage: channel-peaks FILE CHANNELS\n"); return 2; }
    FILE* f = fopen(argv[1], "rb");
    const int channels = atoi(argv[2]);
    if(!f || channels < 1 || channels > 256) { perror(argv[1]); return 1; }
    float peak[256] = {0}, frame[256];
    long frames = 0;
    while(fread(frame, sizeof(float), (size_t)channels, f) == (size_t)channels)
    {
        for(int c = 0; c < channels; ++c)
            if(!(fabsf(frame[c]) <= peak[c])) peak[c] = isnan(frame[c]) ? INFINITY : fabsf(frame[c]);
        ++frames;
    }
    printf("frames %ld\n", frames);
    for(int c = 0; c < channels; ++c) printf("peak %d %g\n", c, peak[c]);
    return 0;
}
