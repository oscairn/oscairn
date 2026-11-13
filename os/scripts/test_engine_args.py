#!/usr/bin/env python3
"""Tests engine-args.py: a headless session gives the engine the options the app would (the same
lists desktop-profile checks for the app), and a profile without a screen is headless."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("engine_args", here / "engine-args.py")
engine = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engine)
profiles = here.parents[1] / "hosts/standalone/profiles"
load = lambda name: json.load(open(profiles / f"{name}.json"))

fantom = load("fantom-07")
assert engine.engine_args(fantom) == [
    "--buffer-frames", "128",
    "--output-device", "hw:1,0", "--audio-outputs", "0,1", "--device-channels", "6", "--sample-rate", "48000",
    "--panel-device", "/dev/i2c-slave-2", "--panel-map", "/usr/share/oscairn/panels/fantom-07.json", "--device-profile", "fantom-07",
    "--input-keys", "gpio-keys", "--input-key-map", "139=menu,152=tempo,157=write",
    "--keyboard-device", "/dev/ttyGS0", "--keyboard-local", "off"], engine.engine_args(fantom)
assert engine.settings(fantom)["OSCAIRN_HEADLESS"] == "0"
assert engine.settings(fantom)["OSCAIRN_RELIABILITY"] == "instrument"
assert engine.settings(fantom)["OSCAIRN_PROFILE"] == "fantom-07"

placed = {**fantom, "compute": {**fantom["compute"], "audioThreadCores": [1, 2, 3]}}
assert engine.settings(placed)["OSCAIRN_AUDIO_THREAD_CORES"] == "1,2,3", "the audio threads' cores as a CPU list"
allowed = {**fantom, "compute": {**fantom["compute"], "workerAllowance": 2000}}
assert engine.settings(allowed)["OSCAIRN_WORKER_BUDGET_US"] == "2000", "the workers' allowance in microseconds"
assert engine.settings(fantom)["OSCAIRN_WORKER_BUDGET_US"] == "", "none named: derived from the block"

kick = load("kick")
assert engine.settings(kick)["OSCAIRN_AUDIO_THREAD_CORES"] == "", "none named: anywhere"
assert engine.engine_args(kick) == ["--buffer-frames", "128"], "no device: the engine's default device, as before"

pedal = {**kick, "screens": []}
assert engine.settings(pedal)["OSCAIRN_HEADLESS"] == "1", "no screen: headless"
del pedal["screens"]
assert engine.settings(pedal)["OSCAIRN_HEADLESS"] == "1"

# Main first, the others in order; a name with spaces survives the shell.
mapped = {**kick, "audio": {"device": {"name": "My interface", "outputs": {"cue": [4, 5], "main": [7, 6]}, "inputs": {"main": [1, 0]}}}}
assert engine.engine_args(mapped) == ["--output-device", "My interface", "--audio-outputs", "7,6,4,5",
                                      "--input-device", "My interface", "--audio-inputs", "1,0"]
script = "\n".join(f"{k}={v}" for k, v in
                   (line.split("=", 1) for line in subprocess.run(
                       [sys.executable, str(here / "engine-args.py"), "/dev/stdin"], input=json.dumps(mapped),
                       capture_output=True, text=True, check=True).stdout.splitlines()))
out = subprocess.run(["/bin/sh", "-c", script + '\neval "set -- $OSCAIRN_ENGINE_ARGS"; printf "%s\\n" "$@"'],
                     capture_output=True, text=True, check=True).stdout.splitlines()
assert out == engine.engine_args(mapped), out
print("ok: the headless engine's options match the app's; no screen is headless")
