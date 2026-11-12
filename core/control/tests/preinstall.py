# SPDX-License-Identifier: AGPL-3.0-or-later
"""The packages that come with a release (RFC 0031, "Preinstalled packages"), through RPC.

usage: preinstall.py OSCAIRN-SIGN FIXTURE.clap BUILD-INDEX.py SERVER [PLUGIN]

Builds a signed repository with throwaway keys, as repository.py does, and a snapshot of it
holding only some of its archives, one of them tampered with. SERVER is started on a fresh data
folder with the snapshot as the app's: with PLUGIN, SERVER is oscairn-json-rpc-test (started as
SERVER PLUGIN --stdio SNAPSHOT); without, it is oscairn-web-rpc-test (SERVER SNAPSHOT), which
fetches from the web as the engine does. The repository's root names mirrors and a blob base on a
local HTTP server that logs every request, and nothing may reach it.

Checked: the first start installs what the snapshot holds and records it; the tampered archive is
refused and reported while the rest still install; a second start does nothing; a package the user
removed is not installed again by the next release, and a failed one is tried again; a newer
version the user has installed is kept; a copy of a repository the device ships is recorded as
that repository; a snapshot older than the repository the device has seen is refused.
"""
import hashlib
import http.server
import json
import os
import platform as host
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

tool, fixture, builder, server = (Path(a).resolve() for a in sys.argv[1:5])
plugin = Path(sys.argv[5]).resolve() if len(sys.argv) > 5 else None
machine = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}[host.machine()]
here = ('macos' if sys.platform == 'darwin' else 'windows' if sys.platform == 'win32' else 'linux') + '-' + machine
failures = 0


def check(ok, what):
    global failures
    print(('ok   ' if ok else 'FAIL ') + what)
    failures += not ok


def run(*command):
    out = subprocess.run([str(c) for c in command], capture_output=True, text=True, timeout=300)
    assert out.returncode == 0, (command, out.stderr)
    return out.stdout


# Every connection to the root's mirrors and blob base (an https:// one too, which this plain
# server cannot answer): none is allowed.
requests = []


class Handler(http.server.BaseHTTPRequestHandler):
    def setup(self):
        requests.append('connection')
        super().setup()

    def log_message(self, *args):
        pass

    def do_GET(self):
        requests.append(self.path)
        self.send_response(404)
        self.send_header('Content-Length', '0')
        self.end_headers()


httpd = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=httpd.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{httpd.server_address[1]}/'

with tempfile.TemporaryDirectory(prefix='oscairn-preinstall-') as temporary:
    temp = Path(temporary)
    keys = {}
    for name in ('publisher', 'root', 'index'):
        _, key_id, public = run(tool, 'key', temp / f'{name}.key').split()
        keys[name] = (key_id, public)
    repo = temp / 'submissions'
    shutil.copytree(builder.parents[1], repo)
    blobs = {}

    def package(plugin_id, version, needs=None, asset_library=None):
        folder = repo / 'packages' / plugin_id
        folder.mkdir(parents=True, exist_ok=True)
        olib = folder / f'{version}.olib'
        if asset_library:
            run(tool, 'asset-package', '--license', 'MIT', '--key', temp / 'publisher.key', olib, plugin_id, version, 'Test factory content', asset_library)
        else:
            build = temp / 'builds' / plugin_id / version / here / 'Synth.clap'
            build.parent.mkdir(parents=True)
            shutil.copyfile(fixture, build)
            run(tool, 'build', temp / 'publisher.key', 'clap', plugin_id, version, here, build)
            depends = ['--asset-dependency', *needs] if needs else []
            run(tool, 'plugin-package', '--license', 'MIT', *depends, '--key', temp / 'publisher.key', olib, 'clap', plugin_id, version, 'Test Synth', f'{here}={build}')
        data = olib.read_bytes()
        record = {'format': 'oscairn.repository-release', 'version': 1, 'id': plugin_id, 'release': version, 'publisher': 'org.kick',
                  'summary': 'A test package', 'license': 'MIT', 'tags': [], 'platforms': [] if asset_library else [here],
                  'archive': {'path': olib.name, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}, 'yanked': None}
        if needs:
            record['dependencies'] = {'assets': [{'id': needs[0], 'version': needs[1]}]}
        (folder / f'{version}.json').write_text(json.dumps(record))
        blobs[(plugin_id, version)] = hashlib.sha256(data).hexdigest()

    package('org.kick.synth', '1.0.0')
    package('org.kick.synth', '2.0.0')
    package('org.kick.other', '1.0.0')
    factory = temp / 'factory'
    (factory / 'patches').mkdir(parents=True)
    (factory / 'patches' / 'Lead.fxp').write_bytes(b'a patch')
    package('org.kick.factory', '1.0.0', asset_library=factory)
    package('org.kick.analog', '1.0.0', needs=('org.kick.factory', '1.0.0'))
    (temp / 'index-key.pub').write_text(keys['index'][1] + '\n')
    run(tool, 'file', temp / 'root.key', temp / 'index-key.pub', temp / 'index-key.sig')
    config = json.loads((repo / 'repository-config.json').read_text())
    config.update(repository='org.example.test', name='Test repository', rootKey=keys['root'][1], mirrors=[base + 'mirror/'], blobBase=base.replace('http:', 'https:') + 'blobs-',
                  indexKey={'key': keys['index'][1], 'signature': json.loads((temp / 'index-key.sig').read_text())[0]['signature']})
    (repo / 'repository-config.json').write_text(json.dumps(config))
    (repo / 'publishers' / 'org.kick.json').write_text(json.dumps({
        'format': 'oscairn.repository-publisher', 'version': 1, 'id': 'org.kick', 'name': 'KICK', 'key': keys['publisher'][1],
        'verification': 'https://kick.example/.well-known/oscairn-publisher.json'}))

    def published(name, serial):
        out = temp / name
        built = subprocess.run([sys.executable, '-I', builder, '--repo', repo, '--oscairn-sign', tool, '--cache', temp / 'cache', '--out', out, '--serial', str(serial)],
                               capture_output=True, text=True, timeout=300)
        assert built.returncode == 0, built.stderr
        run(tool, 'file', temp / 'index.key', out / 'repository.json', out / 'repository.sig')
        for blob in (out / 'blobs.txt').read_text().split():
            (out / 'blobs' / blob[:2]).mkdir(parents=True, exist_ok=True)
            shutil.copyfile(temp / 'cache' / blob, out / 'blobs' / blob[:2] / blob)
        return out
    site = published('site', 5)
    later = published('later', 6)  # the same repository published again since the snapshot was taken

    # The snapshot: the signed root, index key and index as published, and the archives of the
    # synth 1.0.0, the analog synth and the asset library it needs, and the other synth, tampered.
    snapshot = temp / 'Preinstalled'
    shutil.copytree(site, snapshot, ignore=shutil.ignore_patterns('blobs'))
    for key in (('org.kick.synth', '1.0.0'), ('org.kick.analog', '1.0.0'), ('org.kick.factory', '1.0.0'), ('org.kick.other', '1.0.0')):
        sha = blobs[key]
        (snapshot / 'blobs' / sha[:2]).mkdir(parents=True, exist_ok=True)
        shutil.copyfile(site / 'blobs' / sha[:2] / sha, snapshot / 'blobs' / sha[:2] / sha)
    tampered = snapshot / 'blobs' / blobs[('org.kick.other', '1.0.0')][:2] / blobs[('org.kick.other', '1.0.0')]
    data = bytearray(tampered.read_bytes())
    data[len(data) // 2] ^= 1
    tampered.write_bytes(bytes(data))

    class Server:
        """SERVER on a data folder, the snapshot given when `preinstalled`."""
        def __init__(self, home, preinstalled=True):
            command = [server, plugin, '--stdio'] if plugin else [server]
            self.p = subprocess.Popen([str(c) for c in command + ([snapshot] if preinstalled else [])], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      text=True, encoding='utf-8', env={**os.environ, 'OSCAIRN_DATA_HOME': str(home)})

        def rpc(self, method, params=None):
            self.p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}) + '\n')
            self.p.stdin.flush()
            response = json.loads(self.p.stdout.readline())
            assert 'error' not in response, response
            return response['result']

        def settled(self):
            """packages.preinstalled, asked until it is no longer running; each ask moves it on, as service() does."""
            seen = []
            for _ in range(1200):
                state = self.rpc('packages.preinstalled')
                if state['job']:
                    seen.append(self.rpc('packages.job', {'job': state['job']})['state'])
                if state['state'] != 'running':
                    return state, seen
                time.sleep(0.05)
            raise TimeoutError('preinstall')

        def close(self):
            self.p.stdin.close()
            self.p.wait(timeout=30)

    def trusting(home):
        """A data folder trusting the test repository and publisher, as a device trusts the official ones."""
        home.mkdir()
        s = Server(home, preinstalled=False)
        s.rpc('trust.set', {'key': keys['root'][1], 'name': 'Test repository', 'scope': ['repositories']})
        s.rpc('trust.set', {'key': keys['publisher'][1], 'name': 'KICK', 'scope': ['plugins']})
        return s

    def states(result):
        return {p['id']: p['state'] for p in result['packages']}

    def record(home):
        return json.loads((home / 'preinstalled.json').read_text())

    try:
        # The first start: what the snapshot holds is installed, the tampered archive refused.
        home = temp / 'home'
        trusting(home).close()
        s = Server(home)
        result, jobs = s.settled()
        release = result['release']
        check(result['state'] == 'done' and states(result) == {'org.kick.synth': 'installed', 'org.kick.analog': 'installed', 'org.kick.factory': 'installed', 'org.kick.other': 'failed'},
              'the first start installs each package the snapshot holds: ' + json.dumps(result))
        other = next(p for p in result['packages'] if p['id'] == 'org.kick.other')
        check('refused' in other['error'] and other['error'] in result['error'] and 'Catalog' in result['error'],
              'a tampered archive is refused and reported, and the rest still install: ' + other['error'])
        check(jobs and set(jobs) <= {'running', 'done', 'failed'}, 'its progress is visible through packages.job: ' + json.dumps(jobs))
        installed = {(p['id'], p['version']): p for p in s.rpc('packages.installed')['packages']}
        check(set(installed) == {('org.kick.synth', '1.0.0'), ('org.kick.analog', '1.0.0'), ('org.kick.factory', '1.0.0')} and
              all(p['sha256'] for p in installed.values()), 'they are recorded in packages.json like any install: ' + json.dumps(list(installed.values())))
        plugins = sorted((p['id'], p['version']) for p in s.rpc('plugins.list')['plugins'])
        check(plugins == [('org.kick.analog', '1.0.0'), ('org.kick.synth', '1.0.0')], 'their plugins are in the store: ' + json.dumps(plugins))
        r = record(home)
        check(r['releases'] == [release] and sorted(p['id'] for p in r['packages']) == ['org.kick.analog', 'org.kick.factory', 'org.kick.synth'] and
              all(p['release'] == release for p in r['packages']), 'preinstalled.json records what was preinstalled, and from which release: ' + json.dumps(r))
        check(not any(e.name.startswith('.install-') for e in home.iterdir()), 'nothing is left behind')
        s.close()

        # A second start does nothing.
        before = (home / 'packages.json').read_bytes()
        s = Server(home)
        result, jobs = s.settled()
        check(result['state'] == 'none' and result['packages'] == [] and not jobs and (home / 'packages.json').read_bytes() == before and record(home) == r,
              'a second start does nothing: ' + json.dumps(result))

        # The user removes the synth; the next release does not bring it back, and tries the failed one again.
        s.rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.synth', 'version': '1.0.0'})
        s.close()
        r['releases'] = ['0.0.0-an-earlier-release']
        (home / 'preinstalled.json').write_text(json.dumps(r))
        s = Server(home)
        result, jobs = s.settled()
        check(states(result) == {'org.kick.synth': 'removed', 'org.kick.analog': 'kept', 'org.kick.factory': 'kept', 'org.kick.other': 'failed'},
              'at the next release, a removed package stays removed and a failed one is tried again: ' + json.dumps(states(result)))
        check([p['id'] for p in s.rpc('plugins.list')['plugins']] == ['org.kick.analog'], 'the removed synth is not in the store')
        check(record(home)['releases'] == ['0.0.0-an-earlier-release', release], 'that release is recorded too')
        s.close()

        # A user who has a newer version keeps it, and the snapshot's older one is not added beside it.
        home = temp / 'newer'
        s = trusting(home)
        # Installed from a folder copy of a repository this device ships, as the snapshot is of the
        # official one: recorded as that repository, where updates come from.
        s.rpc('packages.setDeviceRepositories', {'repositories': [{'address': 'https://packages.example.org', 'key': keys['root'][1], 'name': 'Test'}]})
        number = s.rpc('packages.install', {'source': str(site), 'id': 'org.kick.synth', 'version': '2.0.0'})['job']
        for _ in range(600):
            done = s.rpc('packages.job', {'job': number})
            if done['state'] != 'running':
                break
            time.sleep(0.05)
        assert done['state'] == 'done', done
        recorded = [p['repository'] for p in s.rpc('packages.installed')['packages']]
        check(recorded == ['https://packages.example.org'], 'a folder copy of a repository this device ships is recorded as that repository: ' + json.dumps(recorded))
        s.close()
        s = Server(home)
        result, _ = s.settled()
        plugins = sorted((p['id'], p['version']) for p in s.rpc('plugins.list')['plugins'])
        check(states(result)['org.kick.synth'] == 'kept' and ('org.kick.synth', '1.0.0') not in plugins and ('org.kick.synth', '2.0.0') in plugins,
              'a newer version installed already is kept: ' + json.dumps(plugins))
        s.close()

        # A device that has seen the repository at a newer serial refuses the older snapshot, as it
        # would refuse any older root: nothing is installed, it says why, and it is not tried again.
        home = temp / 'seen'
        s = trusting(home)
        number = s.rpc('packages.list', {'source': str(later)})['job']
        for _ in range(600):
            if s.rpc('packages.job', {'job': number})['state'] != 'running':
                break
            time.sleep(0.05)
        s.close()
        s = Server(home)
        result, _ = s.settled()
        check(result['state'] == 'done' and 'older than one this device has seen' in result['error'] and result['packages'] == [] and
              s.rpc('packages.installed')['packages'] == [] and record(home)['releases'] == [release],
              'a snapshot older than the repository this device has seen is refused, saying why: ' + result['error'])
        kept = json.loads((home / 'Repositories' / keys['root'][1] / 'org.example.test.index.json').read_text())
        check(kept['serial'] == 6, 'and the newer index this device kept stays: serial ' + str(kept['serial']))
        s.close()

        # Code from a publisher this device does not trust is not set up, said plainly; the asset
        # library, which is not code, is. Nothing accepts unsigned code on the user's behalf.
        home = temp / 'untrusted'
        home.mkdir()
        s = Server(home, preinstalled=False)
        s.rpc('trust.set', {'key': keys['root'][1], 'name': 'Test repository', 'scope': ['repositories']})
        s.close()
        s = Server(home)
        result, _ = s.settled()
        errors = {p['id']: p['error'] for p in result['packages']}
        check(states(result) == {'org.kick.synth': 'failed', 'org.kick.analog': 'failed', 'org.kick.factory': 'installed', 'org.kick.other': 'failed'} and
              errors['org.kick.synth'] == 'Test Synth was not set up: its signature is not from a publisher this device trusts' and
              'unsigned' not in result['error'] and s.rpc('plugins.list')['plugins'] == [],
              'code from an untrusted publisher is not set up, said plainly: ' + json.dumps(errors))
        s.close()

        check(requests == [], 'nothing reaches the network: ' + json.dumps(requests))
    finally:
        httpd.shutdown()
sys.exit(1 if failures else 0)
