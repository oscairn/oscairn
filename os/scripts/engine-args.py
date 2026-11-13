#!/usr/bin/env python3
"""Prints, as shell assignments, what the session needs to run the engine without the app
(headless), from a device profile (RFC 0024):

    OSCAIRN_HEADLESS     1 when the profile has no screen: the session never starts X or the app
    OSCAIRN_RELIABILITY  the profile's reliability level, which the engine reads (RFC 0025)
    OSCAIRN_ENGINE_ARGS  oscairn-engine's audio options, as the app passes them to the engine it
                         starts: the block, the audio device and its channel map (audio.device),
                         the panel and the keyboard. Quoted for the shell: the session evaluates it.

    engine-args.py PROFILE.json
"""
import json
import shlex
import sys


def engine_args(profile, panels="/usr/share/oscairn/panels"):
    """oscairn-engine's options for a profile, as Main.cpp and Profile::configureAudio give them."""
    sound = profile.get("audio") or {}
    args = []
    if sound.get("block"):
        args += ["--buffer-frames", str(sound["block"])]
    device = sound.get("device")
    if device:
        def channels(logical):  # main first, then the others in the profile's order
            names = ["main"] + [n for n in logical if n != "main"]
            return ",".join(str(c) for n in names for c in logical[n])
        args += ["--output-device", device["name"]]
        if device.get("outputs"):
            args += ["--audio-outputs", channels(device["outputs"])]
        if device.get("inputs"):
            args += ["--input-device", device["name"], "--audio-inputs", channels(device["inputs"])]
        if device.get("channels"):
            args += ["--device-channels", str(device["channels"])]
        if device.get("rate"):
            args += ["--sample-rate", str(device["rate"])]
    panel = profile.get("panel")
    if panel:
        path = panel["map"] if panel["map"].startswith("/") else f"{panels}/{panel['map']}"
        args += ["--panel-device", panel["device"], "--panel-map", path]
    keyboard = profile.get("keyboard")
    if keyboard:
        args += ["--keyboard-device", keyboard["device"]]
        if keyboard.get("localOff"):
            args += ["--keyboard-local", "off"]
    return args


def settings(profile):
    return {
        "OSCAIRN_HEADLESS": "0" if profile.get("screens") else "1",
        "OSCAIRN_RELIABILITY": profile.get("reliability", "desktop"),
        "OSCAIRN_ENGINE_ARGS": " ".join(shlex.quote(a) for a in engine_args(profile)),
    }


if __name__ == "__main__":
    for key, value in settings(json.load(open(sys.argv[1]))).items():
        print(f"{key}={shlex.quote(value)}")
