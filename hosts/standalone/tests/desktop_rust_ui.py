# SPDX-License-Identifier: AGPL-3.0-or-later
"""The desktop app starts the Rust UI as a child and takes the display back from it
(Application::startRustUi, rustUiEnded; docs/rust-ui.md, "On a computer"), against the real app
executable and its real engine helper, with no window: --snapshot keeps every window off screen, and
OSCAIRN_UI_PROGRAM names a stub in place of oscairn-ui that does what the Rust UI does at the end of
its run. Three runs:
  handover  the stub names a screen in the --handover file and exits 3: the app opens that screen
            (it says so) and goes on to its snapshot;
  quit      the stub exits 0, its window closed: the app quits at once, before its snapshot;
  linger    the stub stays: the app ends at its snapshot and takes the child with it.
Each run has fresh data, config and runtime folders (as desktop_launch.py), --audio null (the
engine opens no audio or MIDI device) and --size; its engine helper is stopped at the end."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

from desktop_launch import engines, stop

STUB = '''#!%s
import json, os, sys, time
with open(os.environ['STUB_LOG'], 'w') as f:
    json.dump({'args': sys.argv[1:], 'profile': os.environ.get('OSCAIRN_PROFILE'), 'pid': os.getpid()}, f)
mode = os.environ['STUB_MODE']
if mode == 'handover':
    with open(sys.argv[sys.argv.index('--handover') + 1], 'w') as f:
        f.write('library')
    sys.exit(3)
if mode == 'quit':
    sys.exit(0)
time.sleep(120)
''' % sys.executable


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def main():
    app, out = sys.argv[1], sys.argv[2]
    shutil.rmtree(out, ignore_errors=True)  # only this run's folders
    os.makedirs(out)
    stub = os.path.join(out, 'oscairn-ui-stub')
    with open(stub, 'w') as f:
        f.write(STUB)
    os.chmod(stub, 0o755)
    for mode in ('handover', 'quit', 'linger'):
        folder = tempfile.mkdtemp(prefix=mode + '-', dir=out)
        os.makedirs(os.path.join(folder, 'data'))
        os.makedirs(os.path.join(folder, 'config'))
        runtime = tempfile.mkdtemp(prefix='oscairn-rustui-', dir='/tmp')  # short, for the socket's path
        log = os.path.join(folder, 'stub.json')
        child = None
        try:
            started = time.monotonic()
            run = subprocess.run([app, '--snapshot', 'home.png', '--play', '4', '--audio', 'null', '--size', '900x600'], cwd=folder,
                                 capture_output=True, text=True, timeout=120,
                                 env={**os.environ, 'OSCAIRN_DATA_HOME': os.path.join(folder, 'data'),
                                      'OSCAIRN_CONFIG_HOME': os.path.join(folder, 'config'), 'OSCAIRN_RUNTIME_DIR': runtime,
                                      'OSCAIRN_UI_PROGRAM': stub, 'STUB_MODE': mode, 'STUB_LOG': log, 'OSCAIRN_PROFILE': 'kick'})
            took = time.monotonic() - started
            assert run.returncode == 0, f'{mode}: the app exited with {run.returncode}\n{run.stderr}'
            assert os.path.isfile(log), f'{mode}: the app never started the Rust UI\n{run.stderr}'
            with open(log) as f:
                seen = json.load(f)
            child = seen['pid']
            args = seen['args']
            assert args[0] == '--socket' and args[1] and args[2] == '--handover' and args[3], f'{mode}: the child was given {args}'
            assert seen['profile'] == '', f"{mode}: the child's profile is {seen['profile']!r}, not cleared (a profile forces its scale)"
            png = os.path.join(folder, 'home.png')
            handed = 'oscairn: the Rust UI handed over library' in run.stderr
            assert handed == (mode == 'handover'), f'{mode}: handover said: {handed}\n{run.stderr}'
            assert not os.path.exists(args[3]), f'{mode}: the handover file was left behind'
            if mode == 'quit':
                assert not os.path.exists(png), f'{mode}: the app went on to its snapshot ({took:.1f} s) after the Rust UI quit'
            else:
                assert os.path.isfile(png), f'{mode}: no snapshot: the app did not go on after the Rust UI ended\n{run.stderr}'
            deadline = time.monotonic() + 5
            while alive(child) and time.monotonic() < deadline:
                time.sleep(.1)
            assert not alive(child), f'{mode}: the Rust UI outlived the app'
            deadline = time.monotonic() + 10
            while engines(runtime) and time.monotonic() < deadline:
                time.sleep(.1)
            assert not engines(runtime), f'{mode}: the engine helper outlived the app'
            print(f'{mode}: ok ({took:.1f} s)')
        finally:
            for pid in engines(runtime):
                stop(pid, runtime)
            if child and alive(child):
                os.kill(child, 9)  # a stub of this run
            shutil.rmtree(runtime, ignore_errors=True)


if __name__ == '__main__':
    main()
