#!/usr/bin/env python3
"""Boots an Oscairn QEMU image and checks that Oscairn starts.

Runs QEMU's generic ARM64 machine with a Cortex-A76, a 1024 x 600 virtio display and the
kernel's dummy sound card. Passes when, within the time limit:
- the session reports that Oscairn is running;
- Oscairn has not exited;
- the engine reports the audio device it opened;
- the session raised the locked-memory limit (RLIMIT_MEMLOCK) to unlimited, for the engine
  and its workers (RFC 0025);
- the screen shows a picture (Home, RFC 0030; it starts there, so nothing has to move);
- where the image's profile asks for audio cores (IMAGES_DIR/kernel-args has nohz_full=, RFC 0035):
  the kernel was booted with them, the session made an isolated cpuset partition of them, the
  engine and its plugin workers and nothing else are in it, every one of their threads may run
  only there, the engine's audio thread is SCHED_FIFO 80, the app and X are kept off the audio
  cores, no interrupt that can move is on them, and session.status, read in the guest with
  control-request.c, reports audioUnderruns.
Whether sound actually plays is what --record checks.
Writes the console log, the screenshots and audio-cores.txt (what was read in the guest) to the
output folder.

With --record SECONDS, the image gets a virtio sound card instead of the dummy one, and
QEMU writes everything it plays for that long to OUTPUT_DIR/oscairn.wav (44.1 kHz, 16-bit
stereo).

    boot-test.py IMAGES_DIR OUTPUT_DIR [--timeout SECONDS] [--record SECONDS]
"""
import argparse
import os
import re
import socket
import subprocess
import sys
import time

from PIL import Image

import qemu_guest


def monitor(sock_path, command):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(sock_path)
    s.settimeout(5)
    time.sleep(0.2)
    try:
        s.recv(4096)  # banner
    except socket.timeout:
        pass
    s.sendall((command + "\n").encode())
    time.sleep(1.0)
    s.close()


def screenshot(sock_path, out_dir, name):
    ppm = f"/tmp/{name}.ppm"
    if os.path.exists(ppm):
        os.remove(ppm)
    monitor(sock_path, f"screendump {ppm}")
    for _ in range(20):
        if os.path.exists(ppm) and os.path.getsize(ppm) > 0:
            break
        time.sleep(0.25)
    image = Image.open(ppm).convert("RGB")
    image.save(os.path.join(out_dir, f"{name}.png"))
    return image


def cpu_list(text):
    """'0-2,3' as {0, 1, 2, 3}."""
    cpus = set()
    if not re.fullmatch(r"[0-9,-]+", text.strip()):
        return cpus
    for part in filter(None, text.strip().split(",")):
        first, _, last = part.partition("-")
        cpus.update(range(int(first), int(last or first) + 1))
    return cpus


# Run in the guest: one line per fact, read by audio_cores().
AUDIO_CORES_SCRIPT = r"""
A=/sys/fs/cgroup/oscairn-audio
echo "cmdline $(cat /proc/cmdline)"
echo "partition $(cat $A/cpuset.cpus.partition) $(cat $A/cpuset.cpus.effective)"
for p in $(cat $A/cgroup.procs); do echo "member $p $(cat /proc/$p/comm)"; done
for p in $(pidof oscairn-engine oscairn-plugin-worker); do for t in /proc/$p/task/*; do echo "thread $(cat /proc/$p/comm) $(cat $t/comm | tr ' ' _) $(sed 's/.*) //' $t/stat | awk '{print $37, $38, $39}') $(awk '/^Cpus_allowed_list/ {print $2}' $t/status)"; done; done
for p in $(pidof Oscairn Xorg); do echo "other $(cat /proc/$p/comm) $(awk '/^Cpus_allowed_list/ {print $2}' /proc/$p/status)"; done
for i in /proc/irq/[0-9]*; do echo "irq ${i##*/} $(cat $i/effective_affinity_list 2>/dev/null) $(grep "^ *${i##*/}:" /proc/interrupts | tr -s ' ' | cut -d' ' -f7- | tr ' ' _)"; done
"""


def audio_cores(guest, images, out):
    """RFC 0035 step 3 in the guest; True when every check passes. Writes audio-cores.txt."""
    want = cpu_list(qemu_guest.cmdline(images).split("nohz_full=")[1].split()[0])
    facts = guest.run(AUDIO_CORES_SCRIPT, 120)
    guest.install_client(images)
    status = guest.session_status()
    with open(os.path.join(out, "audio-cores.txt"), "w") as f:
        f.write(facts + "\n" + status + "\n")
    # Output lines can follow the shell's "# " prompts; typed commands start with other words.
    lines = [l.split() for l in (re.sub(r"^(# )+", "", l) for l in facts.splitlines()) if l.strip()]
    get = lambda kind: [l[1:] for l in lines if l[0] == kind]
    checks = {}
    checks["kernel booted with nohz_full= and irqaffinity="] = any("nohz_full=" in " ".join(l) and "irqaffinity=" in " ".join(l) for l in get("cmdline"))
    partition = get("partition")
    checks["an isolated partition on the audio cores"] = bool(partition) and partition[0][0] == "isolated" and cpu_list(partition[0][1]) == want
    members = {m[1] for m in get("member")}
    checks["only the engine and its workers in it"] = "oscairn-engine" in members and members <= {"oscairn-engine", "oscairn-plugin-"}
    threads = get("thread")
    checks["every engine and worker thread only on the audio cores"] = bool(threads) and all(cpu_list(t[5]) == want and int(t[2]) in want for t in threads)
    checks["the engine's audio thread SCHED_FIFO 80"] = any(t[0] == "oscairn-engine" and t[3:5] == ["80", "1"] for t in threads)
    checks["workers' audio threads SCHED_FIFO 70"] = not any(t[0] == "oscairn-plugin-" for t in threads) or any(t[0] == "oscairn-plugin-" and t[3:5] == ["70", "1"] for t in threads)
    others = get("other")
    checks["the app and X off the audio cores"] = len(others) >= 2 and all(not cpu_list(o[1]) & want for o in others)
    # Per-core interrupts (the timer, and the like) cannot move; every other one must be off.
    on_audio = [i for i in get("irq") if len(i) > 1 and cpu_list(i[1]) & want and "arch_timer" not in i[-1]]
    checks["no movable interrupt on the audio cores"] = not on_audio
    checks["session.status reports audioUnderruns"] = '"audioUnderruns":' in status
    for name, ok in checks.items():
        print(("ok: " if ok else "FAIL: ") + name)
    if on_audio:
        print("interrupts on the audio cores:", on_audio)
    print("threads:", "; ".join(" ".join(t) for t in threads))
    print("session.status audioUnderruns:", status.split('"audioUnderruns":')[1].split(",")[0] if '"audioUnderruns":' in status else "missing")
    return all(checks.values())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("images")
    parser.add_argument("out")
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--record", type=float, default=0)
    args = parser.parse_args()
    os.makedirs(args.out, exist_ok=True)
    console = os.path.join(args.out, "console.log")
    sock = "/tmp/oscairn-qemu-monitor.sock"
    serial = "/tmp/oscairn-qemu-serial.sock"
    for path in (console, sock, serial):
        if os.path.exists(path):
            os.remove(path)

    extra = ""
    audio = []
    if args.record > 0:
        wav = os.path.join(args.out, "oscairn.wav")
        if os.path.exists(wav):
            os.remove(wav)
        extra = "snd_dummy.enable=0"
        audio = ["-audiodev", f"wav,id=snd0,path={wav},out.frequency=44100,out.channels=2,out.format=s16",
                 "-device", "virtio-sound-pci,audiodev=snd0,romfile="]

    qemu = subprocess.Popen([
        "qemu-system-aarch64", "-M", "virt", "-cpu", "cortex-a76", "-smp", "4", "-m", "2048",
        "-kernel", os.path.join(args.images, "Image"),
        "-append", qemu_guest.cmdline(args.images, extra),
        "-drive", f"file={os.path.join(args.images, 'rootfs.ext4')},if=none,format=raw,id=hd0,snapshot=on",
        "-device", "virtio-blk-device,drive=hd0",
        "-device", "virtio-gpu-pci,xres=1024,yres=600,romfile=",
        "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci",
        "-nic", "none", "-display", "none",
        "-chardev", f"socket,id=s0,path={serial},server=on,wait=off", "-serial", "chardev:s0",
        "-monitor", f"unix:{sock},server,nowait",
    ] + audio)
    start = time.time()
    result = 1
    log = open(console, "w")
    try:
        guest = qemu_guest.Console(serial, log)
        running = False
        while time.time() - start < args.timeout:
            if qemu.poll() is not None:
                print("FAIL: QEMU exited", qemu.returncode)
                break
            try:
                guest.wait_for("oscairn-session: Oscairn running", 2)
                running = True
                break
            except TimeoutError:
                pass
        elapsed = time.time() - start
        if not running:
            print(f"FAIL: Oscairn did not report running within {elapsed:.0f} s")
        else:
            print(f"Oscairn running {elapsed:.0f} s after power-on")
            time.sleep(5)
            a = screenshot(sock, args.out, "screen-1")
            guest.login()
            text = guest.text
            lit = a.convert("L").getextrema()[1] > 64
            audio = "audio output" in text
            exited = "Oscairn exited" in text
            memlock = "oscairn-session: locked memory: unlimited" in text
            print(f"screen {a.size[0]}x{a.size[1]}, has content: {lit}, audio device opened: {audio}, Oscairn exited: {exited}, locked memory unlimited: {memlock}")
            cores = audio_cores(guest, args.images, args.out) if "nohz_full=" in qemu_guest.cmdline(args.images) else True
            result = 0 if lit and audio and memlock and not exited and cores else 1
            print("PASS" if result == 0 else "FAIL")
            if args.record > 0:
                time.sleep(args.record)
                monitor(sock, "quit")  # a clean exit, so the WAV header is complete
                qemu.wait(timeout=30)
                print("recorded", os.path.join(args.out, "oscairn.wav"))
    finally:
        if qemu.poll() is None:
            qemu.kill()
        qemu.wait()
        log.close()
        print("console log:", console)
        lines = open(console, errors="replace").read().splitlines() if os.path.exists(console) else []
        print("\n".join(lines[-25:]))
    return result


if __name__ == "__main__":
    sys.exit(main())
