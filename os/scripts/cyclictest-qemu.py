#!/usr/bin/env python3
"""Boots the real-time test image (oscairn_qemu_aarch64_rt_defconfig) in QEMU and runs cyclictest
on it: a smoke test that the PREEMPT_RT kernel boots and cyclictest runs (RFC 0035 steps 1-2).

QEMU runs the guest's CPUs as host threads, so its latency figures say nothing about hardware;
docs/kick.md ("Measuring latency") has the procedure for a device. Same machine as boot-test.py
(virt, Cortex-A76, 4 cores, 2 GB, the dummy sound card); Oscairn starts as usual and plays
beside the test. Prints the kernel's version, which says PREEMPT_RT on a real-time kernel, and
cyclictest's summary; the console goes to OUT/console.log.

    cyclictest-qemu.py IMAGES_DIR OUT [SECONDS]

In the build container (os/build.sh's image, with the oscairn-os volume at /work), after
os/build.sh oscairn_qemu_aarch64_rt_defconfig:
    cyclictest-qemu.py /work/output/oscairn_qemu_aarch64_rt_defconfig/images /out/cyclictest 60
"""
import os
import socket
import subprocess
import sys
import tempfile
import time

images, out = sys.argv[1:3]
seconds = int(sys.argv[3]) if len(sys.argv) > 3 else 60
os.makedirs(out, exist_ok=True)

sock = os.path.join(tempfile.mkdtemp(), "serial.sock")
qemu = subprocess.Popen([
    "qemu-system-aarch64", "-M", "virt", "-cpu", "cortex-a76", "-smp", "4", "-m", "2048",
    "-kernel", os.path.join(images, "Image"), "-append", "rootwait root=/dev/vda console=ttyAMA0",
    "-drive", f"file={os.path.join(images, 'rootfs.ext4')},if=none,format=raw,id=hd0,snapshot=on",
    "-device", "virtio-blk-device,drive=hd0",
    "-device", "virtio-gpu-pci,xres=1024,yres=600,romfile=",
    "-nic", "none", "-display", "none",
    "-chardev", f"socket,id=s0,path={sock},server=on,wait=off", "-serial", "chardev:s0",
])
while not os.path.exists(sock):
    time.sleep(0.05)
s = socket.socket(socket.AF_UNIX)
s.connect(sock)
s.settimeout(1)
log = open(os.path.join(out, "console.log"), "w")
text = ""


def wait_for(marker, timeout):
    global text
    end = time.time() + timeout
    while marker not in text:
        if time.time() > end:
            raise SystemExit(f"timed out waiting for {marker!r}")
        try:
            data = s.recv(65536).decode(errors="replace")
        except socket.timeout:
            continue
        text += data
        log.write(data)
        log.flush()


try:
    wait_for("login:", 900)
    s.sendall(b"root\n")
    time.sleep(3)
    # One measuring thread per core (-S), SCHED_FIFO 80 as the engine's audio thread, memory
    # locked, a 1 ms interval; -q prints only the summary at the end.
    s.sendall((f"uname -v; "
               f"cyclictest -m -S -p 80 -i 1000 -d 0 -D {seconds} -q; echo CYCLICTEST-DONE\n").encode())
    mark = len(text)
    wait_for("\nCYCLICTEST-DONE", seconds + 120)
    report = text[mark:]
    print(report)
    if " PREEMPT_RT " not in report or "Max:" not in report:
        raise SystemExit("FAIL: the kernel is not PREEMPT_RT, or cyclictest did not report")
finally:
    qemu.kill()
    qemu.wait()
