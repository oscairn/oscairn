#!/usr/bin/env python3
"""A plugin worker failed and given up leaves the engine running: oscairn-engine on the null
device, a second Basic Synth joining the startup synth's instance (a share, whose service once
threw for a failed worker and ended the engine with SIGABRT), every worker killed until the
supervisor gives up; the engine still answers, and session.status names the failure.

    worker_given_up.py ENGINE BASIC_SYNTH
"""
import json
import os
import socket as sockets
import subprocess
import sys
import tempfile
import time

engine, synth = sys.argv[1], sys.argv[2]


def rpc(path, method, params=None):
    with sockets.socket(sockets.AF_UNIX, sockets.SOCK_STREAM) as s:
        s.settimeout(30)
        s.connect(path)
        s.sendall((json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}) + '\n').encode())
        data = b''
        while True:
            chunk = s.recv(65536)
            assert chunk, 'the engine closed the connection'
            data += chunk
            while b'\n' in data:
                line, data = data.split(b'\n', 1)
                reply = json.loads(line)
                if reply.get('id') == 1:
                    assert 'error' not in reply, reply
                    return reply['result']


with tempfile.TemporaryDirectory() as home:
    path = os.path.join(home, 'run', 'engine.sock')
    os.mkdir(os.path.dirname(path), 0o700)
    env = {**os.environ, 'HOME': home, 'OSCAIRN_DATA_HOME': os.path.join(home, 'data'), 'OSCAIRN_CONFIG_HOME': os.path.join(home, 'config')}
    log = open(os.path.join(home, 'engine.log'), 'w+')
    process = subprocess.Popen([engine, '--socket', path, '--audio', 'null', '--buffer-frames', '128'], env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(300):
            if os.path.exists(path) or process.poll() is not None:
                break
            time.sleep(0.1)
        assert os.path.exists(path), 'the engine did not listen'
        rpc(path, 'graph.add', {'name': 'second synth', 'path': synth})
        rpc(path, 'graph.commit')
        assert '"shared"' in json.dumps(rpc(path, 'graph.get')), 'the second synth shares the first one\'s instance'
        why = ''
        for _ in range(8):
            workers = subprocess.run(['pgrep', '-P', str(process.pid), '-f', 'oscairn-plugin-worker'], capture_output=True, text=True).stdout.split()
            for pid in workers:
                subprocess.run(['kill', '-9', pid])
            for i in range(300):  # restarted, or given up
                time.sleep(0.01)
                failures = rpc(path, 'session.status')['failures']
                why = ' '.join(f['reason'] for f in failures)
                if 'not restarted' in why or (i > 20 and not failures):
                    break
            if 'not restarted' in why:
                break
        assert 'not restarted' in why, why
        time.sleep(0.5)  # several service ticks with the share's worker given up
        assert process.poll() is None, 'the engine ended'
        assert 'not restarted' in json.dumps(rpc(path, 'session.status')['failures']), 'session.status names the failure'
    finally:
        process.terminate()
        process.wait(30)
        log.seek(0)
        text = log.read()
        if process.returncode not in (0, -15):
            print(text)
            sys.exit(f'the engine ended with {process.returncode}')
print('ok: a worker given up leaves the engine running and is named in session.status')
