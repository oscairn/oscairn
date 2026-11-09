#!/usr/bin/env python3
"""Boots the real-time test image (oscairn_qemu_aarch64_rt_defconfig) in QEMU and runs cyclictest
on it: a smoke test that the PREEMPT_RT kernel boots and cyclictest runs (RFC 0035 steps 1-2).

QEMU runs the guest's CPUs as host threads, so its latency figures say nothing about hardware;
docs/kick.md ("Measuring latency") has the procedure for a device. Same machine as boot-test.py
(virt, Cortex-A76, 4 cores, 2 GB, the dummy sound card); Oscairn starts as usual and plays
beside the test. Prints the kernel's version, which says PREEMPT_RT on a real-time kernel,
cyclictest's summary (run on the profile's audio cores where it has them) and the engine's
audioUnderruns from session.status; the console goes to OUT/console.log.

    cyclictest-qemu.py IMAGES_DIR OUT [SECONDS]

In the build container (os/build.sh's image, with the oscairn-os volume at /work), after
os/build.sh oscairn_qemu_aarch64_rt_defconfig:
    cyclictest-qemu.py /work/output/oscairn_qemu_aarch64_rt_defconfig/images /out/cyclictest 60
"""
import os
import subprocess
import sys
import tempfile

import qemu_guest

images, out = sys.argv[1:3]
seconds = int(sys.argv[3]) if len(sys.argv) > 3 else 60
os.makedirs(out, exist_ok=True)

sock = os.path.join(tempfile.mkdtemp(), "serial.sock")
qemu = subprocess.Popen([
    "qemu-system-aarch64", "-M", "virt", "-cpu", "cortex-a76", "-smp", "4", "-m", "2048",
    "-kernel", os.path.join(images, "Image"), "-append", qemu_guest.cmdline(images),
    "-drive", f"file={os.path.join(images, 'rootfs.ext4')},if=none,format=raw,id=hd0,snapshot=on",
    "-device", "virtio-blk-device,drive=hd0",
    "-device", "virtio-gpu-pci,xres=1024,yres=600,romfile=",
    "-nic", "none", "-display", "none",
    "-chardev", f"socket,id=s0,path={sock},server=on,wait=off", "-serial", "chardev:s0",
])
try:
    guest = qemu_guest.Console(sock, open(os.path.join(out, "console.log"), "w"))
    guest.login()
    guest.wait_for("oscairn-session: Oscairn running", 300)
    # Where the profile has audio cores (RFC 0035), the shell joins their partition, so cyclictest
    # measures them beside the engine. One measuring thread per core it may use (-S), SCHED_FIFO 80
    # as the engine's audio thread, memory locked, a 1 ms interval; -q prints only the summary.
    report = guest.run("uname -v; A=/sys/fs/cgroup/oscairn-audio; [ -d $A ] && echo $$ > $A/cgroup.procs && "
                       "echo \"on the audio cores: $(cat $A/cpuset.cpus.effective)\"; "
                       f"cyclictest -m -S -p 80 -i 1000 -d 0 -D {seconds} -q", seconds + 120)
    print(report)
    guest.install_client(images)
    status = guest.session_status()
    print("session.status audioUnderruns:", status.split('"audioUnderruns":')[1].split(",")[0] if '"audioUnderruns":' in status else "missing")
    if " PREEMPT_RT " not in report or "Max:" not in report or '"audioUnderruns":' not in status:
        raise SystemExit("FAIL: the kernel is not PREEMPT_RT, cyclictest did not report, or session.status has no audioUnderruns")
finally:
    qemu.kill()
    qemu.wait()
