#!/usr/bin/env python3
"""The engine opens the Performance last open at its start (performance.reopen, asked by itself):
oscairn-engine on the null device in fresh data and home folders, a Performance saved and
remembered in library/last.json; started again it opens that one while answering session.status;
with --startup-performance it keeps the startup Performance; one no longer there leaves the startup
Performance, saying why.

    reopen_last.py ENGINE
"""
import json
import os
import shutil
import socket as sockets
import subprocess
import sys
import tempfile
import time

engine = sys.argv[1]


def call(path, method, params=None):
    with sockets.socket(sockets.AF_UNIX, sockets.SOCK_STREAM) as s:
        s.settimeout(30)
        s.connect(path)
        s.sendall((json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}) + '\n').encode())
        data = b''
        while b'\n' not in data:
            chunk = s.recv(65536)
            assert chunk, 'the engine closed the connection'
            data += chunk
        return json.loads(data.split(b'\n', 1)[0])


def rpc(path, method, params=None):
    reply = call(path, method, params)
    assert 'error' not in reply, reply
    return reply['result']


def until(done, seconds=60):
    end = time.time() + seconds
    while time.time() < end:
        try:
            if done():
                return True
        except AssertionError:  # refused as busy while it opens
            pass
        time.sleep(0.05)
    return False


with tempfile.TemporaryDirectory() as home:
    path = os.path.join(home, 'run', 'engine.sock')
    os.mkdir(os.path.dirname(path), 0o700)
    data = os.path.join(home, 'data')
    env = {**os.environ, 'HOME': home, 'OSCAIRN_DATA_HOME': data, 'OSCAIRN_CONFIG_HOME': os.path.join(home, 'config')}
    log = open(os.path.join(home, 'engine.log'), 'w+')

    def run(*extra):
        if os.path.exists(path):
            os.remove(path)
        process = subprocess.Popen([engine, '--socket', path, '--audio', 'null', '--buffer-frames', '128', *extra], env=env, stdout=log, stderr=subprocess.STDOUT)
        for _ in range(300):
            if os.path.exists(path) or process.poll() is not None:
                break
            time.sleep(0.1)
        assert os.path.exists(path), 'the engine did not listen'
        return process

    def stop(process):
        process.terminate()
        process.wait(30)

    saved = os.path.join(home, 'Saved')
    last = os.path.join(data, 'library', 'last.json')
    try:
        process = run()
        try:
            assert until(lambda: rpc(path, 'performance.info')['name'] == ''), 'nothing remembered: the startup Performance'
            rpc(path, 'performance.setInfo', {'name': 'Reopened', 'description': '', 'tags': []})
            rpc(path, 'performance.save', {'directory': saved})
            remembered = json.load(open(last))
            assert remembered['performance'] == os.path.realpath(saved) and remembered['id'] == rpc(path, 'performance.info')['id'], remembered
            # The same one opened again leaves the record as it is: written only when it changes.
            os.utime(last, (1, 1))
            rpc(path, 'performance.load', {'directory': saved})
            assert os.stat(last).st_mtime == 1, 'the same Performance again is not written again'
        finally:
            stop(process)

        process = run()
        try:
            status = call(path, 'session.status')  # at once: answered, or refused as busy while it reads
            assert 'result' in status or status['error']['code'] == -32009, status
            assert until(lambda: rpc(path, 'performance.info')['name'] == 'Reopened'), 'started again, the last one opens'
            assert rpc(path, 'performance.reopen') == {'notice': ''} and rpc(path, 'performance.info')['name'] == 'Reopened', 'a UI asking again keeps it'
        finally:
            stop(process)

        process = run('--startup-performance')
        try:
            time.sleep(2)
            assert rpc(path, 'performance.info')['name'] == '', '--startup-performance keeps the startup Performance'
        finally:
            stop(process)

        shutil.rmtree(saved)
        process = run()
        try:
            assert until(lambda: any('no longer there' in n for n in rpc(path, 'session.status')['notices'])), 'one no longer there is said'
            assert rpc(path, 'performance.info')['name'] == '', 'and the startup Performance stays'
        finally:
            stop(process)
    finally:
        log.seek(0)
        output = log.read()
        if 'Traceback' in output or sys.exc_info()[0]:
            print(output)
print('ok: the engine opens the Performance last open at its start, the startup one when told or when it is gone')
