#!/usr/bin/env python3
"""oscairn-engine-timing against a real engine: oscairn-engine on the null device (which writes
nowhere at real-time pace and opens no MIDI device), in fresh data and home folders; the tool
resets the window, waits, and prints every measurement, the startup Performance's workers' too.

    engine_timing.py ENGINE TOOL
"""
import os
import subprocess
import sys
import tempfile
import time

engine, tool = sys.argv[1], sys.argv[2]
with tempfile.TemporaryDirectory() as home:
    socket = os.path.join(home, "run", "engine.sock")
    os.mkdir(os.path.dirname(socket), 0o700)
    env = {**os.environ, "HOME": home, "OSCAIRN_DATA_HOME": os.path.join(home, "data"), "OSCAIRN_CONFIG_HOME": os.path.join(home, "config")}
    log = open(os.path.join(home, "engine.log"), "w+")
    process = subprocess.Popen([engine, "--socket", socket, "--audio", "null", "--buffer-frames", "128"], env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(300):
            if os.path.exists(socket) or process.poll() is not None:
                break
            time.sleep(0.1)
        assert os.path.exists(socket), "the engine did not listen"
        out = subprocess.run([tool, socket, "--seconds", "3"], capture_output=True, text=True, timeout=60)
        print(out.stdout, out.stderr)
        assert out.returncode == 0, out.returncode
        rows = {line.split(" steady ")[0].strip(): line.split(" steady ")[1].split() for line in out.stdout.splitlines() if " steady " in line}
        assert int(rows["callback duration"][0]) > 0 and int(rows["callback lateness"][0]) > 0, rows
        assert any(name.endswith("round trip") and int(row[0]) > 0 for name, row in rows.items()), "the startup Performance's workers are timed"
        assert "deadline misses" in out.stdout and "[1] callback duration" in out.stdout
        bad = subprocess.run([tool, os.path.join(home, "none.sock"), "--seconds", "0"], capture_output=True, text=True)
        assert bad.returncode == 1 and "none.sock" in bad.stderr, bad.stderr
    finally:
        process.terminate()
        process.wait(30)
        log.seek(0)
        if process.returncode not in (0, -15):
            print(log.read())
print("ok: oscairn-engine-timing reads a running engine's timing")
