# SPDX-License-Identifier: AGPL-3.0-or-later
"""macOS only: the real app opens Settings, its own process (RFC 0022), and Settings comes in front of
the shell's window as the active app. An app's window comes forward only when its process is made
active, which the shell hands over (activateApp, AppProcesses_mac.mm); without it Settings opened
behind Home. The windows' order front to back and the active app are read from the window server
(CGWindowListCopyWindowInfo, NSWorkspace) through osascript.

The shell can hand over only an activation it holds: one launched while the musician types in another
app may never become active (macOS 14 and later keep the app in use in front), and then there is
nothing to check, so the run is skipped (77). The shell active with Settings behind it fails.

It shows the app's windows and takes the active app (and keyboard focus) from whoever is working,
so it runs only when OSCAIRN_WINDOW_TESTS=1 is set; otherwise it starts nothing and is skipped (77).

Fresh data, config and HOME folders and a runtime folder of its own, --audio null, as desktop_launch;
the shell, its apps and its engine helper are stopped at the end."""
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from desktop_launch import engines, stop

WINDOWS = '''
ObjC.import("Cocoa"); ObjC.import("CoreGraphics");
const list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo(
    $.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, $.kCGNullWindowID)));
JSON.stringify({front: $.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier,
                windows: list.filter(w => w.kCGWindowLayer == 0).map(w => w.kCGWindowOwnerPID)});
'''


def windows():
    """The active app's PID, and the PIDs owning the on-screen windows, front to back."""
    out = json.loads(subprocess.run(['osascript', '-l', 'JavaScript', '-e', WINDOWS], capture_output=True, text=True, check=True).stdout)
    return out['front'], out['windows']


def children(pid, name):
    """The shell's child processes whose executable is `name`."""
    found = subprocess.run(['pgrep', '-P', str(pid)], capture_output=True, text=True).stdout.split()
    return [int(p) for p in found if subprocess.run(['ps', '-o', 'comm=', '-p', p], capture_output=True, text=True).stdout.strip().endswith('/' + name)]


def main():
    if os.environ.get('OSCAIRN_WINDOW_TESTS') != '1':
        print('skipped: it shows windows and takes focus; OSCAIRN_WINDOW_TESTS=1 runs it')
        return 77
    app, out = sys.argv[1], sys.argv[2]
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    folder = tempfile.mkdtemp(prefix='settings-', dir=out)
    for name in ('data', 'config', 'home'):
        os.makedirs(os.path.join(folder, name))
    runtime = tempfile.mkdtemp(prefix='oscairn-order-', dir='/tmp')
    shell = subprocess.Popen([app, '--audio', 'null', '--app', 'settings'], cwd=folder,
                             stdout=open(os.path.join(folder, 'shell.log'), 'w'), stderr=subprocess.STDOUT,
                             env={**os.environ, 'HOME': os.path.join(folder, 'home'),
                                  'OSCAIRN_DATA_HOME': os.path.join(folder, 'data'),
                                  'OSCAIRN_CONFIG_HOME': os.path.join(folder, 'config'),
                                  'OSCAIRN_RUNTIME_DIR': runtime})
    apps = []
    try:
        settings, shown, front, order = None, None, None, []
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            assert shell.poll() is None, f'the app exited with {shell.returncode}'
            time.sleep(.25)
            apps = children(shell.pid, 'Oscairn Settings') or apps
            if not apps:
                continue
            settings = apps[0]
            front, order = windows()
            if settings not in order:
                continue
            shown = shown or time.monotonic()
            ours = [p for p in order if p in (shell.pid, settings)]
            if front == settings and ours and ours[0] == settings:
                print(f'Settings (pid {settings}) is the active app, its window in front of the shell\'s (pid {shell.pid})')
                return 0
            if time.monotonic() - shown > 10:  # the show's answer, and the activation after it, have had their time
                break
        assert settings and shown, 'Settings never showed a window'
        assert front != shell.pid, f'the shell (pid {shell.pid}) is the active app and Settings (pid {settings}) is behind it: windows front to back {order}'
        print(f'skipped: the shell never became the active app (pid {front} is), so it had no activation to hand over')
        return 77
    finally:
        apps += children(shell.pid, 'Oscairn Settings')
        shell.terminate()
        try:
            shell.wait(10)
        except subprocess.TimeoutExpired:
            shell.kill()
            shell.wait()
        for pid in set(apps):  # left by a shell stopped by a signal; still Settings, not a reused PID
            if not subprocess.run(['ps', '-o', 'comm=', '-p', str(pid)], capture_output=True, text=True).stdout.strip().endswith('/Oscairn Settings'):
                continue
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        for pid in engines(runtime):
            stop(pid, runtime)
        shutil.rmtree(runtime, ignore_errors=True)


if __name__ == '__main__':
    sys.exit(main())
