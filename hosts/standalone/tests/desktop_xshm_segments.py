# SPDX-License-Identifier: AGPL-3.0-or-later
"""Linux only: the app's X11 window image (JUCE's MIT-SHM XBitmapImage) leaves no SysV shared-memory
segment behind, however the app ends. On the FANTOM-07 each start left one 3.7 MB segment that
nothing freed, until they filled swap (docs/fantom-0.md); the segment is now marked for removal as
soon as the X server has attached it (hosts/juce-patches).

The real app runs on an Xvfb of its own (skipped, 77, without Xvfb), with --audio null and fresh data,
config and runtime folders, as desktop_launch. Each run waits for the app's segment
(/proc/sysvipc/shm, created by the app's PID), which must already be marked for removal (SHM_DEST),
then ends the app: SIGKILL, SIGTERM, its window closed (WM_DELETE_WINDOW, a normal quit), or its
window made larger (a new, larger image) and then SIGKILL. Afterwards no segment of this user's
that was not there before may be left with a dead creator."""
import ctypes
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from desktop_launch import engines, stop

SHM_DEST = 0o1000


def segments():
    """The SysV segments, from /proc/sysvipc/shm, by id."""
    with open('/proc/sysvipc/shm') as f:
        names = f.readline().split()
        rows = [dict(zip(names, line.split())) for line in f]
    return {int(r['shmid']): {k: int(r[k], 8 if k == 'perms' else 10) for k in ('key', 'perms', 'size', 'cpid', 'nattch', 'uid')} for r in rows}


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


class XEvent(ctypes.Structure):
    """XClientMessageEvent, padded to an XEvent (24 longs)."""
    _fields_ = [('type', ctypes.c_int), ('serial', ctypes.c_ulong), ('send_event', ctypes.c_int),
                ('display', ctypes.c_void_p), ('window', ctypes.c_ulong), ('message_type', ctypes.c_ulong),
                ('format', ctypes.c_int), ('data', ctypes.c_long * 5), ('pad', ctypes.c_long * 12)]


class X:
    """The few Xlib calls this needs, through ctypes: the app's top-level windows, closed or resized."""

    def __init__(self, display):
        self.x = ctypes.CDLL('libX11.so.6')
        self.x.XOpenDisplay.restype = ctypes.c_void_p
        self.x.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x.XDefaultRootWindow.restype = ctypes.c_ulong
        self.x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        self.x.XInternAtom.restype = ctypes.c_ulong
        self.x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
        self.x.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong),
                                      ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint)]
        self.x.XGetWindowProperty.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_long, ctypes.c_long, ctypes.c_int,
                                              ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_int),
                                              ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_void_p)]
        self.x.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.POINTER(XEvent)]
        self.x.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
        self.x.XFree.argtypes = [ctypes.c_void_p]
        self.x.XFlush.argtypes = [ctypes.c_void_p]
        self.x.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.d = self.x.XOpenDisplay(display.encode())
        assert self.d, f'cannot open display {display}'
        # Made before the app starts: without a window manager they do not exist, and JUCE, which
        # only looks them up, would then neither set nor answer WM_DELETE_WINDOW.
        for name in ('WM_PROTOCOLS', 'WM_DELETE_WINDOW'):
            self.atom(name)

    def atom(self, name):
        return self.x.XInternAtom(self.d, name.encode(), 0)

    def windows(self, pid):
        """The app's main window ("Oscairn - ..."; not the splash) among the top-level windows whose
        _NET_WM_PID is pid."""
        root, parent, children, n = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.POINTER(ctypes.c_ulong)(), ctypes.c_uint()
        self.x.XQueryTree(self.d, self.x.XDefaultRootWindow(self.d), ctypes.byref(root), ctypes.byref(parent), ctypes.byref(children), ctypes.byref(n))
        found = [children[i] for i in range(n.value)
                 if self.property(children[i], '_NET_WM_PID', 6) == bytes(ctypes.c_long(pid))  # XA_CARDINAL
                 and self.property(children[i], 'WM_NAME', 0).startswith(b'Oscairn - ')]
        if children:
            self.x.XFree(children)
        return found

    def property(self, window, name, kind):
        """A window property's bytes (a format-32 item as a C long), b'' when it has none."""
        actual, fmt, items, after, value = ctypes.c_ulong(), ctypes.c_int(), ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_void_p()
        if self.x.XGetWindowProperty(self.d, window, self.atom(name), 0, 256, 0, kind, ctypes.byref(actual), ctypes.byref(fmt),
                                     ctypes.byref(items), ctypes.byref(after), ctypes.byref(value)) != 0 or not value.value:
            return b''
        data = ctypes.string_at(value, items.value * {8: 1, 16: 2, 32: ctypes.sizeof(ctypes.c_long)}.get(fmt.value, 0))
        self.x.XFree(value)
        return data

    def close(self, window):
        event = XEvent(type=33, send_event=1, window=window, message_type=self.atom('WM_PROTOCOLS'), format=32)  # ClientMessage
        event.data[0], event.data[1] = self.atom('WM_DELETE_WINDOW'), 0  # CurrentTime
        self.x.XSendEvent(self.d, window, 0, 0, ctypes.byref(event))
        self.x.XFlush(self.d)

    def resize(self, window, width, height):
        self.x.XResizeWindow(self.d, window, width, height)
        self.x.XFlush(self.d)

    def __del__(self):
        if getattr(self, 'd', None):
            self.x.XCloseDisplay(self.d)


def wait_for(what, seconds, check):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        found = check()
        if found:
            return found
        time.sleep(.02)
    raise AssertionError(f'timed out waiting for {what}')


def run(app, out, display, x, name, end):
    folder = tempfile.mkdtemp(prefix=name + '-', dir=out)
    for sub in ('data', 'config'):
        os.makedirs(os.path.join(folder, sub))
    runtime = tempfile.mkdtemp(prefix='oscairn-xshm-', dir='/tmp')
    uid, before = os.getuid(), set(segments())
    child = subprocess.Popen([app, '--audio', 'null'], cwd=folder,
                             stdout=open(os.path.join(folder, 'stdout'), 'w'), stderr=open(os.path.join(folder, 'stderr'), 'w'),
                             env={**os.environ, 'DISPLAY': display, 'OSCAIRN_DATA_HOME': os.path.join(folder, 'data'),
                                  'OSCAIRN_CONFIG_HOME': os.path.join(folder, 'config'), 'OSCAIRN_RUNTIME_DIR': runtime})
    try:
        def ours():
            assert child.poll() is None, f'{name}: the app exited with {child.returncode} before drawing\n' + open(os.path.join(folder, 'stderr')).read()
            return {i: s for i, s in segments().items() if s['cpid'] == child.pid}

        def kept():
            """The app's segments that last half a second: not JUCE's MIT-SHM check (a 50 x 50 image,
            removed at once) nor one caught between its creation and the server's attaching it."""
            then = ours()
            time.sleep(.5)
            return {i: s for i, s in ours().items() if i in then}

        first = wait_for(f'{name}: a segment of the app\'s', 120, kept)
        for i, s in first.items():
            assert s['perms'] & SHM_DEST, f'{name}: segment {i} ({s["size"]} bytes) is not marked for removal: perms {s["perms"]:o}'
        if end == 'resize-kill':
            window = wait_for(f'{name}: the app\'s window', 30, lambda: x.windows(child.pid))[0]
            x.resize(window, 1500, 950)  # larger than its own 1280 x 800
            larger = wait_for(f'{name}: a larger segment after the resize', 30,
                              lambda: {i: s for i, s in kept().items() if i not in first and s['size'] > max(t['size'] for t in first.values())})
            for i, s in larger.items():
                assert s['perms'] & SHM_DEST, f'{name}: segment {i} after the resize is not marked for removal'
        if end == 'close':
            for window in wait_for(f'{name}: the app\'s window', 30, lambda: x.windows(child.pid)):
                x.close(window)
            assert child.wait(timeout=60) == 0, f'{name}: the app exited with {child.returncode}\n' + open(os.path.join(folder, 'stderr')).read()
        else:
            child.send_signal(signal.SIGTERM if end == 'term' else signal.SIGKILL)
            child.wait(timeout=60)
        # The server detaches a gone client's segments as it notices; then the kernel frees them.
        left = lambda: {i: s for i, s in segments().items() if i not in before and s['uid'] == uid and not alive(s['cpid'])}
        deadline = time.monotonic() + 10
        while left() and time.monotonic() < deadline:
            time.sleep(.05)
        assert not left(), f'{name}: segments left behind by dead processes: {left()}'
        print(f'{name}: the app\'s segment(s) {sorted(first)} marked for removal, exit {child.returncode}, none left behind')
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        for pid in engines(runtime):
            stop(pid, runtime)
        shutil.rmtree(runtime, ignore_errors=True)


def main():
    app, out = sys.argv[1], sys.argv[2]
    xvfb = shutil.which('Xvfb')
    if not xvfb:
        print('Xvfb not found: skipped')
        sys.exit(77)
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    read, write = os.pipe()
    server = subprocess.Popen([xvfb, '-displayfd', str(write), '-nolisten', 'tcp', '-screen', '0', '1600x1000x24'],
                              pass_fds=[write], stdout=subprocess.DEVNULL, stderr=open(os.path.join(out, 'xvfb.log'), 'w'))
    os.close(write)
    try:
        number = b''
        while not number.endswith(b'\n'):
            chunk = os.read(read, 16)
            assert chunk, 'Xvfb ended before saying its display'
            number += chunk
        display = ':' + number.decode().strip()
        x = X(display)
        for name, end in (('kill', 'kill'), ('term', 'term'), ('close', 'close'), ('resize', 'resize-kill')):
            run(app, out, display, x, name, end)
        del x
    finally:
        os.close(read)
        server.terminate()
        server.wait()


if __name__ == '__main__':
    main()
