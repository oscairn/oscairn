#!/usr/bin/env python3
"""Prints, as shell assignments, the audio cores a device profile asks for (RFC 0035):

    OSCAIRN_AUDIO_CORES        the last compute.realtimeCores of compute.cores, e.g. 3
    OSCAIRN_HOUSEKEEPING_CORES the others, e.g. 0-2
    OSCAIRN_IDLE_LIMIT_US      a tenth of the profile's block at its lowest rate: idle states
                               slower to leave are turned off on the audio cores
    OSCAIRN_KERNEL_ARGS        nohz_full= and irqaffinity= for the kernel's command line

Only at the instrument and pass-through reliability levels, with at least one core left for
housekeeping; otherwise nothing is printed and nothing is isolated.

    audio-cores.py PROFILE.json
"""
import json
import sys


def cores(profile):
    compute = profile.get("compute") or {}
    total, audio = compute.get("cores", 0), compute.get("realtimeCores", 0)
    if profile.get("reliability") not in ("instrument", "pass-through") or audio < 1 or total - audio < 1:
        return {}
    span = lambda a, b: str(a) if a == b else f"{a}-{b}"
    audio_cores, housekeeping = span(total - audio, total - 1), span(0, total - audio - 1)
    sound = profile.get("audio") or {}
    block, rate = sound.get("block") or 128, min(sound.get("rates") or [48000])
    return {
        "OSCAIRN_AUDIO_CORES": audio_cores,
        "OSCAIRN_HOUSEKEEPING_CORES": housekeeping,
        "OSCAIRN_IDLE_LIMIT_US": str(int(block / rate * 1e6 / 10)),
        "OSCAIRN_KERNEL_ARGS": f"nohz_full={audio_cores} irqaffinity={housekeeping}",
    }


if __name__ == "__main__":
    for key, value in cores(json.load(open(sys.argv[1]))).items():
        print(f'{key}="{value}"')
