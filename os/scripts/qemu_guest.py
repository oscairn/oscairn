"""Shared by boot-test.py and cyclictest-qemu.py: the kernel's command line for an image, and a
shell on the guest's serial console to read its state and its engine's session.status."""
import base64
import glob
import os
import socket
import subprocess
import tempfile
import time

here = os.path.dirname(os.path.abspath(__file__))


def cmdline(images, extra=""):
    """QEMU's -append: the root and console, then what the image's profile asks of the kernel
    (kernel-args, written by board/common/post-build.sh: the audio cores' nohz_full= and
    irqaffinity=)."""
    path = os.path.join(images, "kernel-args")
    args = open(path).read().strip() if os.path.exists(path) else ""
    return " ".join(a for a in ("rootwait root=/dev/vda console=ttyAMA0", extra, args) if a)


def control_client(images, name="control-request"):
    """NAME.c (control-request.c, or another tool beside it) built with the image's own compiler
    (in images/../host), as base64."""
    gcc = glob.glob(os.path.join(images, "..", "host", "bin", "*-buildroot-linux-gnu-gcc"))[0]
    out = os.path.join(tempfile.mkdtemp(), name)
    subprocess.run([gcc, "-O2", "-s", "-o", out, os.path.join(here, name + ".c"), "-lm"], check=True)
    return base64.encodebytes(open(out, "rb").read()).decode()


class Console:
    """The guest's serial console, a QEMU socket chardev, logging everything it reads."""

    def __init__(self, path, log):
        while not os.path.exists(path):
            time.sleep(0.05)
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.connect(path)
        self.socket.settimeout(1)
        self.log = log
        self.text = ""

    def wait_for(self, marker, timeout, since=0):
        end = time.time() + timeout
        while marker not in self.text[since:]:
            if time.time() > end:
                raise TimeoutError(f"timed out waiting for {marker!r}")
            try:
                data = self.socket.recv(65536).decode(errors="replace")
            except socket.timeout:
                continue
            self.text += data
            self.log.write(data)
            self.log.flush()

    def login(self):
        self.wait_for("login:", 900)
        self.socket.sendall(b"root\n")
        self.wait_for("# ", 30)

    def run(self, commands, timeout=60):
        """Runs shell commands; returns what they printed (the typed commands left out)."""
        mark = len(self.text)
        # The end marker is split in what is typed, so only the output carries it whole.
        self.socket.sendall((commands.rstrip("\n") + "\necho '==DO''NE=='\n").encode())
        self.wait_for("==DONE==", timeout, mark)
        output = self.text[mark:self.text.index("==DONE==", mark)]
        return "\n".join(l.rstrip("\r") for l in output.splitlines() if "==DO''NE==" not in l)

    def install_client(self, images, name="control-request"):
        """Types the control client (or another tool beside it) into the guest as /tmp/NAME."""
        self.run(f"base64 -d > /tmp/{name} << 'EOF'\n" + control_client(images, name) + f"EOF\nchmod +x /tmp/{name}")

    def session_status(self):
        """The engine's session.status, read in the guest, as the JSON line it returned."""
        output = self.run("/tmp/control-request $(ls /tmp/oscairn-*/engine-*.sock | head -n 1) "
                          "'{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"session.hello\"}' "
                          "'{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"session.status\"}'")
        return [l for l in output.splitlines() if l.startswith("{") and '"id":2' in l][-1]
