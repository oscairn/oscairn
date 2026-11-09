#!/usr/bin/env python3
"""Boots an Oscairn QEMU image without a display and checks, through ALSA itself, that the engine
honors a profile's audio device (RFC 0024's audio.device). Nothing reaches a speaker: the device is
an ALSA PCM that writes what it is given to a file.

- A 16-channel device: an ALSA `file` PCM over `null`, named by its ALSA name. The engine runs
  with the options a profile gives (desktop-profile checks that) for
  { "name": "oscairn_tdm16", "channels": 16, "rate": 48000, "outputs": { "main": [6, 7] } }
  and the QEMU profile's block, plays its startup pattern for a few seconds, and the file must hold 16 channels with sound on 6 and 7 and exact silence on
  the other 14. The engine must report the device by its ALSA description, at 48 kHz.
- An ALSA hw: address: hw:0,0 (the kernel's dummy card) must open as that card.

    alsa-slots-test.py IMAGES_DIR OUTPUT_DIR
"""
import argparse
import os
import subprocess
import sys

import qemu_guest

ASOUNDRC = """pcm.oscairn_tdm16 {
	type file
	slave.pcm "null"
	file "/tmp/tdm16.raw"
	format "raw"
	hint { show on description "Oscairn test, 16 slots" }
}
"""
REQUEST = "/tmp/control-request /tmp/slots/engine.sock '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"session.hello\"}' "


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("images")
    parser.add_argument("out")
    args = parser.parse_args()
    os.makedirs(args.out, exist_ok=True)
    options = "--buffer-frames 128 --output-device oscairn_tdm16 --audio-outputs 6,7 --device-channels 16 --sample-rate 48000"
    serial = "/tmp/oscairn-qemu-slots.sock"
    if os.path.exists(serial):
        os.remove(serial)
    qemu = subprocess.Popen([
        "qemu-system-aarch64", "-M", "virt", "-cpu", "cortex-a76", "-smp", "4", "-m", "2048",
        "-kernel", os.path.join(args.images, "Image"), "-append", qemu_guest.cmdline(args.images),
        "-drive", f"file={os.path.join(args.images, 'rootfs.ext4')},if=none,format=raw,id=hd0,snapshot=on",
        "-device", "virtio-blk-device,drive=hd0", "-nic", "none", "-display", "none",
        "-chardev", f"socket,id=s0,path={serial},server=on,wait=off", "-serial", "chardev:s0"])
    log = open(os.path.join(args.out, "alsa-slots-console.log"), "w")
    result = 1
    try:
        guest = qemu_guest.Console(serial, log)
        guest.login()
        guest.install_client(args.images)
        guest.install_client(args.images, "channel-peaks")
        print("engine options:", options)
        out = guest.run(
            f"cat > /root/.asoundrc << 'EOF'\n{ASOUNDRC}EOF\n"
            "mkdir -p /tmp/slots && chmod 700 /tmp/slots\n"
            f"oscairn-engine --socket /tmp/slots/engine.sock --plugins /usr/lib/clap {options} > /tmp/slots/tdm.log 2>&1 & pid=$!\n"
            "for i in $(seq 120); do [ -S /tmp/slots/engine.sock ] && break; sleep 0.5; done\n"
            + REQUEST + "'{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"transport.set\",\"params\":{\"playing\":true}}'\n"
            "sleep 4; kill $pid; wait $pid\n"
            "cat /tmp/slots/tdm.log\n"
            "/tmp/channel-peaks /tmp/tdm16.raw 16\n"
            "rm -f /tmp/tdm16.raw /tmp/slots/engine.sock\n"
            "oscairn-engine --socket /tmp/slots/engine.sock --plugins /usr/lib/clap --output-device hw:0,0 > /tmp/slots/hw.log 2>&1 & pid=$!\n"
            "for i in $(seq 120); do [ -S /tmp/slots/engine.sock ] && break; sleep 0.5; done\n"
            "kill $pid; wait $pid\n"
            "sed 's/^/hw: /' /tmp/slots/hw.log\n", 600)
        open(os.path.join(args.out, "alsa-slots.txt"), "w").write(out)
        lines = [l.split() for l in out.splitlines()]
        peaks = {int(l[1]): float(l[2]) for l in lines if len(l) == 3 and l[0] == "peak"}
        frames = [int(l[1]) for l in lines if len(l) == 2 and l[0] == "frames"]
        checks = {
            "the engine opened the profile's device by its ALSA name, at 48 kHz":
                'audio output "Oscairn test, 16 slots", 48000 Hz' in out,
            "the device was written as 16 channels": bool(frames) and frames[0] > 0 and len(peaks) == 16,
            "main left and right sound on channels 6 and 7": peaks.get(6, 0) > 1e-3 and peaks.get(7, 0) > 1e-3,
            "the other 14 channels are exactly silent": len(peaks) == 16 and all(peaks[c] == 0 for c in peaks if c not in (6, 7)),
            "hw:0,0 opens the dummy card": any(l[:3] == ["hw:", "oscairn:", "audio"] and "Dummy" in " ".join(l) for l in lines),
        }
        for name, ok in checks.items():
            print(("ok: " if ok else "FAIL: ") + name)
        print("frames:", frames[0] if frames else 0, "peaks:", " ".join(f"{c}:{peaks[c]:.3g}" for c in sorted(peaks)))
        print("\n".join(l for l in out.splitlines() if "audio output" in l or "oscairn-engine:" in l))
        result = 0 if all(checks.values()) else 1
        print("PASS" if result == 0 else "FAIL")
    finally:
        qemu.kill()
        qemu.wait()
        log.close()
    return result


if __name__ == "__main__":
    sys.exit(main())
