# SPDX-License-Identifier: AGPL-3.0-or-later
"""Launch the real desktop app and check it starts and renders: every run goes through the whole
executable (splash, engine, window), which the in-process desktop tests never reach. Each run gets
fresh OSCAIRN_DATA_HOME and OSCAIRN_CONFIG_HOME folders under the build tree, never the user's,
and a runtime folder of its own, so it starts its own engine helper and never reaches a running one.
--snapshot renders a window to a PNG and quits; the PNG must exist, have the size asked for and
show more than one color. Stdlib only: the PNG is decoded here (zlib; 8-bit RGB/RGBA, no interlace).

Every run passes --audio null: the helper opens the null device, which writes nowhere, and no MIDI
device, so nothing it does can sound (and a machine without audio hardware runs it). Its log must
say so: a helper that opened a real device or a MIDI input fails the run. The app's own process
opens no MIDI port either (its controllers' Mackie and feedback ports): its log must say 0. It stops when the app
quits; one left behind (the app crashed or timed out, which leaves it running by design) is
stopped here, by the PID in its lease, only when that process is this run's.

--snapshot renders off screen: on macOS, the window server's list of on-screen windows is read
throughout each run, and must never hold one of the app's (no splash, no window)."""
import ctypes
import fcntl
import glob
import os
import re
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib


def png_pixels(path):
    """The PNG's width, height and a sample of its pixels' colors."""
    with open(path, 'rb') as f:
        data = f.read()
    assert data[:8] == b'\x89PNG\r\n\x1a\n', f'{path} is not a PNG'
    width = height = color = None
    idat = b''
    at = 8
    while at + 8 <= len(data):
        length, kind = struct.unpack_from('>I4s', data, at)
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', body)
            assert depth == 8 and color in (2, 6) and not interlace, f'{path}: not an 8-bit RGB/RGBA PNG'
        elif kind == b'IDAT':
            idat += body
        elif kind == b'IEND':
            break
        at += 12 + length
    assert width and height, f'{path}: no IHDR'
    channels = 3 if color == 2 else 4
    stride = width * channels
    raw = zlib.decompress(idat)

    def paeth(a, b, c):
        p, pa, pb, pc = a + b - c, abs(b - c), abs(a - c), abs(a + b - 2 * c)
        return a if pa <= pb and pa <= pc else (b if pb <= pc else c)

    lines = bytearray()
    previous = bytearray(stride)
    for row in range(height):
        line = bytearray(raw[row * (stride + 1) + 1:(row + 1) * (stride + 1)])
        filter_type = raw[row * (stride + 1)]
        for i in range(stride):
            left = line[i - channels] if i >= channels else 0
            up = previous[i]
            corner = previous[i - channels] if i >= channels else 0
            if filter_type == 1:
                line[i] = (line[i] + left) & 0xFF
            elif filter_type == 2:
                line[i] = (line[i] + up) & 0xFF
            elif filter_type == 3:
                line[i] = (line[i] + (left + up) // 2) & 0xFF
            elif filter_type == 4:
                line[i] = (line[i] + paeth(left, up, corner)) & 0xFF
            else:
                assert filter_type == 0, f'{path}: unknown PNG row filter {filter_type}'
        lines += line
        previous = line
    step = max(1, (width * height) // 4000) * channels
    return width, height, {bytes(lines[i:i + channels]) for i in range(0, len(lines) - channels + 1, step)}


def on_screen(pid):
    """The bounds of the windows on screen that the process with this PID owns (macOS), from
    CGWindowListCopyWindowInfo; None elsewhere."""
    if sys.platform != 'darwin':
        return None
    cf = ctypes.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
    cg = ctypes.CDLL('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
    for name, result, args in (('CFArrayGetCount', ctypes.c_long, [ctypes.c_void_p]),
                               ('CFArrayGetValueAtIndex', ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_long]),
                               ('CFDictionaryGetValue', ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
                               ('CFStringCreateWithCString', ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint32]),
                               ('CFNumberGetValue', ctypes.c_bool, [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]),
                               ('CFRelease', None, [ctypes.c_void_p])):
        getattr(cf, name).restype, getattr(cf, name).argtypes = result, args
    cg.CGWindowListCopyWindowInfo.restype, cg.CGWindowListCopyWindowInfo.argtypes = ctypes.c_void_p, [ctypes.c_uint32, ctypes.c_uint32]
    key = lambda text: cf.CFStringCreateWithCString(None, text, 0x08000100)  # UTF-8
    owner, bounds = key(b'kCGWindowOwnerPID'), key(b'kCGWindowBounds')
    numbers = [key(k) for k in (b'X', b'Y', b'Width', b'Height')]

    def number(dictionary, k):
        value = ctypes.c_double()
        found = cf.CFDictionaryGetValue(dictionary, k)
        return value.value if found and cf.CFNumberGetValue(found, 13, ctypes.byref(value)) else None  # kCFNumberDoubleType

    found = []
    windows = cg.CGWindowListCopyWindowInfo(1, 0)  # kCGWindowListOptionOnScreenOnly, every window
    for i in range(cf.CFArrayGetCount(windows) if windows else 0):
        window = cf.CFArrayGetValueAtIndex(windows, i)
        if number(window, owner) == pid:
            box = cf.CFDictionaryGetValue(window, bounds)
            found.append(tuple(number(box, k) for k in numbers) if box else ())
    if windows:
        cf.CFRelease(windows)
    for k in [owner, bounds] + numbers:
        cf.CFRelease(k)
    return found


def engines(runtime):
    """The engine helpers still holding their lease in this runtime folder (EngineLease), by PID."""
    held = []
    for path in glob.glob(os.path.join(runtime, 'engine-*.sock.lock')):
        with open(path) as lease:
            try:
                fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                held.append(int(lease.read().strip() or 0))
    return [pid for pid in held if pid > 0]


def stop(pid, runtime):
    """Stops a helper of this run (its command names the runtime folder) and its workers, its group."""
    command = subprocess.run(['ps', '-o', 'command=', '-p', str(pid)], capture_output=True, text=True).stdout
    if runtime not in command:
        return
    group = os.getpgid(pid) == pid
    for sig, wait in ((signal.SIGTERM, 5), (signal.SIGKILL, 5)):
        try:
            os.killpg(pid, sig) if group else os.kill(pid, sig)
        except ProcessLookupError:
            return
        deadline = time.monotonic() + wait
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return
            time.sleep(.05)


def main():
    app, out = sys.argv[1], sys.argv[2]
    shutil.rmtree(out, ignore_errors=True)  # only this run's folders and PNGs
    os.makedirs(out)
    # A crash at launch (as with the Apps menu and the null app processes) shows here as a nonzero
    # or signal exit before a PNG is ever written.
    runs = [('home', ['--size', '900x600']),
            ('library', ['--app', 'library', '--size', '900x600']),
            ('kick', ['--profile', 'kick', '--size', '1024x600'])]
    for name, extra in runs:
        folder = tempfile.mkdtemp(prefix=name + '-', dir=out)
        os.makedirs(os.path.join(folder, 'data'))
        os.makedirs(os.path.join(folder, 'config'))
        # Short, for the socket's path (104 bytes on macOS), as the engine's own tests keep theirs.
        runtime = tempfile.mkdtemp(prefix='oscairn-launch-', dir='/tmp')
        wanted = tuple(int(v) for v in extra[extra.index('--size') + 1].split('x'))
        try:
            child = subprocess.Popen([app, '--snapshot', name + '.png', '--audio', 'null'] + extra, cwd=folder,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                     env={**os.environ,
                                          'OSCAIRN_DATA_HOME': os.path.join(folder, 'data'),
                                          'OSCAIRN_CONFIG_HOME': os.path.join(folder, 'config'),
                                          'OSCAIRN_RUNTIME_DIR': runtime})
            # Its output is read on threads, so the window list is read throughout the run.
            run = subprocess.CompletedProcess(child.args, None, '', '')
            readers = [threading.Thread(target=lambda: setattr(run, 'stdout', child.stdout.read())),
                       threading.Thread(target=lambda: setattr(run, 'stderr', child.stderr.read()))]
            for reader in readers:
                reader.start()
            shown, deadline = [], time.monotonic() + 120
            while child.poll() is None:
                if time.monotonic() > deadline:
                    child.kill()
                    break
                shown += on_screen(child.pid) or []
                time.sleep(.02)
            run.returncode = child.wait()
            for reader in readers:
                reader.join()
            assert run.returncode == 0, f'{name}: the app exited with {run.returncode}\n{run.stderr}'
            assert not shown, f'{name}: a window was on screen during the snapshot: {sorted(set(shown))}'
            # A helper whose app quit normally stops on its own, soon after.
            deadline = time.monotonic() + 10
            while engines(runtime) and time.monotonic() < deadline:
                time.sleep(.1)
            assert not engines(runtime), f'{name}: the engine helper outlived the app'
            log = ''.join(open(path).read() for path in glob.glob(os.path.join(runtime, '*.log')))
            assert 'oscairn: audio output "null"' in log and 'oscairn: 0 MIDI inputs open' in log and \
                'audio output "' not in log.replace('audio output "null"', '') and \
                not re.search(r'oscairn: [1-9][0-9]* MIDI inputs open', log), f'{name}: the helper did not run on the null device alone\n{log}'
            assert 'oscairn: 0 MIDI ports open in the app' in run.stderr and \
                not re.search(r'oscairn: [1-9][0-9]* MIDI ports open in the app|MIDI (input|output) "', run.stderr), \
                f'{name}: the app opened a MIDI port of its own\n{run.stderr}'
            png = os.path.join(folder, name + '.png')
            assert os.path.isfile(png) and os.path.getsize(png) > 0, f'{name}: no PNG written\n{run.stderr}'
            width, height, colors = png_pixels(png)
            assert (width, height) == wanted, f'{name}: the PNG is {width}x{height}, not {wanted[0]}x{wanted[1]}'
            assert len(colors) > 1, f'{name}: the PNG is one flat color: the window never drew\n{run.stderr}'
            print(f'{name}: {width}x{height}, {len(colors)} colors')
        finally:
            for pid in engines(runtime):
                stop(pid, runtime)
            for log in glob.glob(os.path.join(runtime, '*.log')):
                shutil.copy(log, folder)  # the helper's output, beside the PNG
            shutil.rmtree(runtime, ignore_errors=True)


if __name__ == '__main__':
    main()
