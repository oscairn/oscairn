# SPDX-License-Identifier: AGPL-3.0-or-later
"""Installing from a package repository (RFC 0031 step 1), through RPC.

usage: repository.py RPC-SERVER PLUGIN OSCAIRN-SIGN FIXTURE.clap BUILD-INDEX.py [WEB-RPC-SERVER]

Makes throwaway keys, packs plugin packages with oscairn-sign, builds a repository with the
package repository template's index builder, signs its root, and installs from it as a folder:
an untrusted root, code from an unknown publisher, an incompatible build, a version needing a
newer system, a tampered blob, a root whose signature does not verify and an older root are each
refused, leaving nothing behind. An unpacked package folder installs, and one with a changed
file is refused. A root claiming the repository's ID under another root key keeps serials and a
kept index of its own, and serials kept by ID alone (version 1) still refuse an older root.

With WEB-RPC-SERVER, the same from a local web server instead. Over HTTPS (RFC 0039 step 1), a
certificate from an authority the system does not trust is refused. The rest runs over HTTPS
only where the system trusts a test authority, which the test cannot arrange itself:
OSCAIRN_TEST_CA names a folder holding that authority's ca.pem and ca.key (set up in a throwaway
VM or container); a wrong host name is then refused too.
"""
import hashlib
import http.server
import socket
import ssl
import urllib.request
import threading
import json
import os
import platform as host
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

try:
    import jsonschema
    protocol = json.loads((Path(__file__).resolve().parents[3] / 'docs/control-protocol.schema.json').read_text())
    request_validator = jsonschema.Draft202012Validator({**protocol, '$ref': '#/$defs/Request'})
except ImportError:
    jsonschema = None
    print('JSON Schema validator not installed; requests are not checked against the schema')

server, plugin, tool, fixture, builder = (Path(a).resolve() for a in sys.argv[1:6])
web = Path(sys.argv[6]).resolve() if len(sys.argv) > 6 else None  # a session that fetches from the web: the HTTP phase only
machine = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}[host.machine()]
here = ('macos' if sys.platform == 'darwin' else 'windows' if sys.platform == 'win32' else 'linux') + '-' + machine
elsewhere = 'linux-arm64' if here != 'linux-arm64' else 'macos-arm64'
failures = 0


def check(ok, what):
    global failures
    print(('ok   ' if ok else 'FAIL ') + what)
    failures += not ok


def run(*command):
    out = subprocess.run([str(c) for c in command], capture_output=True, text=True, timeout=300)
    assert out.returncode == 0, (command, out.stderr)
    return out.stdout


def rpc(method, params=None, expect_error=False):
    request = {'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}
    if jsonschema and not expect_error:
        request_validator.validate(request)
    p.stdin.write(json.dumps(request) + '\n')
    p.stdin.flush()
    response = json.loads(p.stdout.readline())
    assert ('error' in response) == expect_error, response
    return response.get('error') if expect_error else response.get('result')


def job(method, params):
    """Starts a package job and asks about it until it ends."""
    number = rpc(method, params)['job']
    for _ in range(600):
        state = rpc('packages.job', {'job': number})
        if state['state'] != 'running':
            return state
        time.sleep(0.05)
    raise TimeoutError(method)


with tempfile.TemporaryDirectory(prefix='oscairn-repository-') as temporary:
    temp = Path(temporary)
    keys = {}
    for name in ('publisher', 'root', 'index', 'other', 'impostor', 'impostor-index'):
        _, key_id, public = run(tool, 'key', temp / f'{name}.key').split()
        keys[name] = (key_id, public)

    # Plugin packages, each build and archive signed by the publisher: the synth for this
    # platform at 1.0.0 and 2.0.0 (which will need a newer system), and another plugin whose only
    # build is for another platform.
    repo = temp / 'submissions'
    shutil.copytree(builder.parents[1], repo)
    releases = []

    def package(plugin_id, version, platform, requires=None, needs=None, asset_library=None):
        folder = repo / 'packages' / plugin_id
        folder.mkdir(parents=True, exist_ok=True)
        olib = folder / f'{version}.olib'
        if asset_library:  # an asset package from that folder
            run(tool, 'asset-package', '--license', 'MIT', '--key', temp / 'publisher.key', olib, plugin_id, version, 'Test factory content', asset_library)
        else:
            build = temp / 'builds' / plugin_id / version / platform / 'Synth.clap'
            build.parent.mkdir(parents=True)
            shutil.copyfile(fixture, build)
            run(tool, 'build', temp / 'publisher.key', 'clap', plugin_id, version, platform, build)
            depends = ['--asset-dependency', *needs] if needs else []
            run(tool, 'plugin-package', '--license', 'MIT', *depends, '--key', temp / 'publisher.key', olib, 'clap', plugin_id, version, 'Test Synth', f'{platform}={build}')
        data = olib.read_bytes()
        record = {'format': 'oscairn.repository-release', 'version': 1, 'id': plugin_id, 'release': version, 'publisher': 'org.kick',
                  'summary': 'A test synth', 'license': 'MIT', 'tags': ['synth'], 'platforms': [] if asset_library else [platform],
                  'archive': {'path': olib.name, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}, 'yanked': None}
        if requires:
            record['requires'] = {'oscairn': requires}
        if needs:
            record['dependencies'] = {'assets': [{'id': needs[0], 'version': needs[1]}]}
        (folder / f'{version}.json').write_text(json.dumps(record))
        releases.append(olib)
        return olib

    synth = package('org.kick.test.synth', '1.0.0', here)
    package('org.kick.test.synth', '2.0.0', here, requires='99')
    package('org.kick.other', '1.0.0', elsewhere)
    # An asset library at 1.0.0 and 1.1.0, and a plugin package that needs at least 1.1.0 of it.
    factory = temp / 'factory'
    (factory / 'patches').mkdir(parents=True)
    (factory / 'patches' / 'Lead.fxp').write_bytes(b'a patch')
    package('org.kick.factory', '1.0.0', here, asset_library=factory)
    (factory / 'patches' / 'Pad.fxp').write_bytes(b'another patch')
    package('org.kick.factory', '1.1.0', here, asset_library=factory)
    package('org.kick.analog', '1.0.0', here, needs=('org.kick.factory', '1.1.0'))
    (temp / 'index-key.pub').write_text(keys['index'][1] + '\n')
    run(tool, 'file', temp / 'root.key', temp / 'index-key.pub', temp / 'index-key.sig')
    config = json.loads((repo / 'repository-config.json').read_text())
    config.update(repository='org.example.test', name='Test repository', rootKey=keys['root'][1],
                  indexKey={'key': keys['index'][1], 'signature': json.loads((temp / 'index-key.sig').read_text())[0]['signature']})
    (repo / 'repository-config.json').write_text(json.dumps(config))
    (repo / 'publishers' / 'org.kick.json').write_text(json.dumps({
        'format': 'oscairn.repository-publisher', 'version': 1, 'id': 'org.kick', 'name': 'KICK', 'key': keys['publisher'][1],
        'verification': 'https://kick.example/.well-known/oscairn-publisher.json'}))

    def site(name, serial, *extra, source=repo, index='index'):
        """The repository as the index builder makes it, signed, with its blobs under blobs/<ab>/."""
        out = temp / name
        built = subprocess.run([sys.executable, '-I', builder, '--repo', source, '--oscairn-sign', tool, '--cache', temp / 'cache',
                                '--out', out, '--serial', str(serial), *extra], capture_output=True, text=True, timeout=300)
        assert built.returncode == 0, built.stderr
        run(tool, 'file', temp / f'{index}.key', out / 'repository.json', out / 'repository.sig')
        for blob in (out / 'blobs.txt').read_text().split():
            (out / 'blobs' / blob[:2]).mkdir(parents=True, exist_ok=True)
            shutil.copyfile(temp / 'cache' / blob, out / 'blobs' / blob[:2] / blob)
        return out

    if web:
        # Over HTTP (RFC 0031 step 2), from a local server: the tree at /, a tampered mirror at
        # /bad/ and a good one at /good/. The root lists both mirrors, the bad one first, and a
        # blob base nothing answers. Every request is logged, and chosen ones are cut short.
        state = {'log': [], 'cut': set(), 'site': None, 'bad': None, 'fail': {}}

        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'

            def log_message(self, *args):
                pass

            def do_GET(self):
                state['log'].append((self.path, self.headers.get('Range')))
                if state['fail'].get(self.path):  # a server error, as a host's outage gives
                    self.send_response(state['fail'][self.path].pop(0))
                    self.send_header('Content-Length', '0')
                    self.end_headers()
                    return
                for prefix, root in (('/bad/', state['bad']), ('/good/', state['site']), ('/', state['site'])):
                    if self.path.startswith(prefix):
                        relative = self.path[len(prefix):]
                        break
                file = root / relative
                if '..' in relative or not file.is_file():
                    self.send_response(404)
                    self.send_header('Content-Length', '0')
                    self.end_headers()
                    return
                data = file.read_bytes()
                ranged = self.headers.get('Range', '')
                start = int(ranged[6:].split('-')[0]) if ranged.startswith('bytes=') else 0
                body = data[start:]
                self.send_response(206 if start else 200)
                self.send_header('Content-Length', str(len(body)))
                if start:
                    self.send_header('Content-Range', f'bytes {start}-{len(data) - 1}/{len(data)}')
                self.end_headers()
                if self.path in state['cut']:
                    state['cut'].discard(self.path)
                    self.wfile.write(body[:len(body) // 2])
                    self.wfile.flush()
                    time.sleep(0.3)  # the half arrives before the connection drops
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.close_connection = True
                    return
                self.wfile.write(body)

        openssl = shutil.which('openssl')
        trusted = Path(os.environ['OSCAIRN_TEST_CA']) if os.environ.get('OSCAIRN_TEST_CA') else None

        def authority(folder):
            folder.mkdir()
            run(openssl, 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '2', '-subj', '/CN=Oscairn test authority',
                '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign',
                '-keyout', folder / 'ca.key', '-out', folder / 'ca.pem')
            return folder

        def serve(ca=None):
            """A server for the tree on 127.0.0.1, and its address: over HTTPS for localhost, signed by ca, if given."""
            server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
            if ca:
                name = temp / f'localhost-{ca.name}'
                run(openssl, 'req', '-new', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost', '-keyout', name.with_suffix('.key'), '-out', name.with_suffix('.csr'))
                name.with_suffix('.ext').write_text('subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature,keyEncipherment\nauthorityKeyIdentifier=keyid\n')
                run(openssl, 'x509', '-req', '-days', '2', '-set_serial', str(int.from_bytes(os.urandom(8), 'big')), '-in', name.with_suffix('.csr'),
                    '-CA', ca / 'ca.pem', '-CAkey', ca / 'ca.key', '-extfile', name.with_suffix('.ext'), '-out', name.with_suffix('.pem'))
                context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                context.load_cert_chain(name.with_suffix('.pem'), name.with_suffix('.key'))
                server.socket = context.wrap_socket(server.socket, server_side=True)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            return server, f'{"https://localhost" if ca else "http://127.0.0.1"}:{server.server_address[1]}/'

        if trusted and not openssl:
            sys.exit('OSCAIRN_TEST_CA needs openssl')
        httpd, base = serve(trusted)
        print('over ' + base.split(':')[0].upper())
        config = json.loads((repo / 'repository-config.json').read_text())
        config.update(mirrors=[base + 'bad/', base + 'good/'], blobBase='https://127.0.0.1:9/nothing/')
        (repo / 'repository-config.json').write_text(json.dumps(config))
        state['site'] = site('web7', 7)
        state['bad'] = temp / 'web-bad'
        shutil.copytree(state['site'], state['bad'])
        synth_blob = hashlib.sha256(synth.read_bytes()).hexdigest()
        bad_blob = state['bad'] / 'blobs' / synth_blob[:2] / synth_blob
        bad_blob.write_bytes(bad_blob.read_bytes()[:-1] + b'!')
        blob_path = f'blobs/{synth_blob[:2]}/{synth_blob}'
        home = temp / 'web-home'
        home.mkdir()
        p = subprocess.Popen([web], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8', env={**os.environ, 'OSCAIRN_DATA_HOME': str(home)})

        def fetched():
            paths = [path for path, _ in state['log']]
            state['log'].clear()
            return paths

        try:
            rpc('trust.set', {'key': keys['root'][1], 'name': 'Test repository', 'scope': ['repositories']})
            rpc('trust.set', {'key': keys['publisher'][1], 'name': 'KICK', 'scope': ['plugins']})
            state['log'].clear()
            listing = job('packages.list', {'source': base})
            paths = fetched()
            check(listing['state'] == 'done' and listing['serial'] == 7 and len(listing['packages']) == 4, 'a repository on the web lists what it offers: ' + listing['error'])
            # The revocation list too, since 2026-11-11: read with the root, then kept, so later refreshes with the same list do not fetch it.
            check(paths[:2] == ['/repository.json', '/repository.sig'] and len(paths) == 4 and paths[2].startswith('/revocations/') and paths[3].startswith('/index/') and paths[3].endswith('.json.gz'),
                  'the first refresh fetches the root, its revocation list and the whole index: ' + json.dumps(paths))
            listing = job('packages.list', {'source': base})
            paths = fetched()
            check(listing['state'] == 'done' and paths == ['/repository.json', '/repository.sig'], 'a refresh with nothing new fetches the root alone: ' + json.dumps(paths))
            record = repo / 'packages' / 'org.kick.test.synth' / '2.0.0.json'
            record.write_text(json.dumps(dict(json.loads(record.read_text()), yanked='a test')))
            state['site'] = site('web8', 8, '--previous-site', state['site'])
            listing = job('packages.list', {'source': base})
            paths = fetched()
            yanked = {v['version']: v['yanked'] for p in listing['packages'] if p['id'] == 'org.kick.test.synth' for v in p['versions']}
            check(listing['state'] == 'done' and listing['serial'] == 8 and len(paths) == 3 and paths[2].endswith('.delta.gz') and yanked == {'1.0.0': '', '2.0.0': 'a test'},
                  'a refresh with one changed package fetches the root and one delta, and applies it: ' + json.dumps(paths) + ' ' + json.dumps(yanked))

            # A device that does not know the time fetches nothing from the web, and says why.
            q = subprocess.Popen([web], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8',
                                 env={**os.environ, 'OSCAIRN_DATA_HOME': str(home), 'OSCAIRN_TIME': 'unknown'})
            try:
                def ask(method, params):
                    q.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params}) + '\n')
                    q.stdin.flush()
                    return json.loads(q.stdout.readline())['result']
                number = ask('packages.list', {'source': base})['job']
                for _ in range(300):
                    refused = ask('packages.job', {'job': number})
                    if refused['state'] != 'running':
                        break
                    time.sleep(0.05)
            finally:
                q.stdin.close()
                q.wait(timeout=30)
            check(refused['state'] == 'failed' and 'does not know the time' in refused['error'] and fetched() == [],
                  'with the time unknown, nothing is fetched from the web, saying why: ' + refused['error'])

            def store():
                return rpc('plugins.list')['plugins']

            def leftovers():
                return sorted(e.name for e in home.iterdir() if e.name.startswith('.install-'))

            # An install whose every download is cut short keeps what it fetched, and the next
            # one resumes it. The mirrors serve the good tree for this.
            state['bad'] = state['site']
            state['cut'] = {'/bad/' + blob_path, '/good/' + blob_path, '/' + blob_path}
            first = job('packages.install', {'source': base, 'id': 'org.kick.test.synth'})
            kept = home / f'.install-{synth_blob}.partial'
            have = kept.stat().st_size if kept.is_file() else 0
            check(first['state'] == 'failed' and 'resumes' in first['error'] and 0 < have < synth.stat().st_size and store() == [],
                  'a download cut short fails, keeping what it fetched: ' + first['error'])
            fetched()
            second = job('packages.install', {'source': base, 'id': 'org.kick.test.synth'})
            ranges = [r for path, r in state['log'] if path.endswith(synth_blob)]
            check(second['state'] == 'done' and ranges == [f'bytes={have}-'] and len(store()) == 1 and leftovers() == [],
                  'and installing again resumes it with a range request: ' + json.dumps(ranges) + ' ' + second['error'])

            # A mirror serving a tampered blob is refused and the next one used; one cut short
            # midway is resumed from another place in the same install.
            rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.test.synth', 'version': '1.0.0'})
            state['bad'] = temp / 'web-bad'
            state['cut'] = {'/good/' + blob_path}
            fetched()
            third = job('packages.install', {'source': base, 'id': 'org.kick.test.synth'})
            log = [(path, r) for path, r in state['log'] if path.endswith(synth_blob)]
            check(third['state'] == 'done' and [path for path, _ in log][:2] == ['/bad/' + blob_path, '/good/' + blob_path] and log[-1][0] == '/' + blob_path and (log[-1][1] or '').startswith('bytes=')
                  and log[-1][1] != 'bytes=0-' and len(store()) == 1 and leftovers() == [],
                  'a tampered mirror is refused, the next used, and a cut download resumed elsewhere: ' + json.dumps(log) + ' ' + third['error'])

            # A server error is asked again once, and a place answering in time is used.
            rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.test.synth', 'version': '1.0.0'})
            state['bad'] = state['site']
            state['fail'] = {'/bad/' + blob_path: [503]}
            fetched()
            fourth = job('packages.install', {'source': base, 'id': 'org.kick.test.synth'})
            tried = [path for path, _ in state['log'] if path.endswith(synth_blob)]
            check(fourth['state'] == 'done' and tried == ['/bad/' + blob_path] * 2 and len(store()) == 1,
                  'a server error is asked again once, and the answer then used: ' + json.dumps(tried) + ' ' + fourth['error'])

            # When every place fails, the error names each one and why, not only the last.
            rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.test.synth', 'version': '1.0.0'})
            state['fail'] = {prefix + blob_path: [504, 504] for prefix in ('/bad/', '/good/', '/')}
            fetched()
            fifth = job('packages.install', {'source': base, 'id': 'org.kick.test.synth'})
            tried = [path for path, _ in state['log'] if path.endswith(synth_blob)]
            named = all(f'{base}{prefix}{blob_path} answered 504' in fifth['error'] for prefix in ('bad/', 'good/', '')) and '127.0.0.1:9/nothing/' in fifth['error']
            check(fifth['state'] == 'failed' and named and len(tried) == 6 and store() == [],
                  'when every place fails, each is asked twice and named with its reason: ' + fifth['error'])
            state['fail'] = {}

            # Certificates are checked: a refused one fetches nothing. Python's own client
            # reaching the server with the authority shows the refusal is the certificate's.
            if not openssl:
                print('openssl not found; certificate checks not run')
            else:
                unknown = authority(temp / 'unknown-authority')
                other, address = serve(unknown)
                served = urllib.request.urlopen(address + 'repository.json', timeout=10, context=ssl.create_default_context(cafile=unknown / 'ca.pem')).status == 200
                fetched()
                refused = job('packages.list', {'source': address})
                check(served and refused['state'] == 'failed' and fetched() == [], 'a certificate from an authority the system does not trust is refused: ' + refused['error'])
                other.shutdown()
                if trusted:
                    refused = job('packages.list', {'source': base.replace('localhost', '127.0.0.1')})
                    check(refused['state'] == 'failed' and fetched() == [], 'a certificate for another host name is refused: ' + refused['error'])
                else:
                    print('OSCAIRN_TEST_CA not set; HTTPS with a trusted certificate and a wrong host name not checked')
        finally:
            p.stdin.close()
            p.wait(timeout=30)
            httpd.shutdown()
        sys.exit(1 if failures else 0)

    good = site('site', 7)
    home = temp / 'home'
    home.mkdir()
    p = subprocess.Popen([server, plugin, '--stdio'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8',
                         env={**os.environ, 'OSCAIRN_DATA_HOME': str(home)})

    def leftovers():
        return sorted(e.name for e in home.iterdir() if e.name.startswith('.install-'))

    def store():
        return rpc('plugins.list')['plugins']

    def records():
        return rpc('packages.installed')['packages']

    def recorded():  # what each record names, without what it was installed from
        return [{k: r[k] for k in ('id', 'version', 'kind', 'repository')} for r in records()]

    try:
        # Not trusted yet: refused, naming its root key so the user can choose to trust it.
        state = job('packages.list', {'source': str(good)})
        check(state['state'] == 'failed' and 'not a repository this device trusts' in state['error'] and keys['root'][0] in state['error'] and state['rootKey'] == keys['root'][1] and
              state.get('claimedName') == 'Test repository' and state['name'] == '',
              'an untrusted repository is refused, naming its root key and the name it claims, signed; this device has no name for it: ' + state['error'])
        rpc('trust.set', {'key': keys['root'][1], 'name': 'Test repository', 'scope': ['repositories']})
        state = job('packages.list', {'source': str(good)})
        listed = {x['id']: x for x in state['packages']}
        check(state['state'] == 'done' and state['repository'] == 'org.example.test' and state['serial'] == 7 and set(listed) == {'org.kick.test.synth', 'org.kick.other', 'org.kick.factory', 'org.kick.analog'},
              'once trusted, it lists what it offers: ' + json.dumps(state)[:300])
        versions = {v['version']: v for v in listed['org.kick.test.synth']['versions']}
        check(versions['1.0.0']['incompatible'] == '' and versions['1.0.0']['installed'] >= fixture.stat().st_size and 'system update' in versions['2.0.0']['incompatible'],
              'each version says whether it fits this device: ' + json.dumps(versions))
        check('no build for this device' in listed['org.kick.other']['versions'][0]['incompatible'] and listed['org.kick.other']['publisherKey'] == keys['publisher'][0],
              'a package without a build for this device says so')
        analog = listed['org.kick.analog']['versions'][0]
        check(analog['types'] == {'clap': 1} and analog['dependencies'] == [{'id': 'org.kick.factory', 'version': '1.1.0'}] and
              listed['org.kick.factory']['versions'][0]['types'] == {'oscairn.asset': 1} and listed['org.kick.factory']['versions'][0]['dependencies'] == [],
              'each version lists what it holds by type and the packages it needs: ' + json.dumps(analog))

        # Code from a publisher not trusted for plugins needs a yes, as an archive's does.
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth'})
        check(state['state'] == 'failed' and 'unknown source' in state['error'] and store() == [] and leftovers() == [],
              'code by an unknown key is refused without a yes, leaving nothing: ' + state['error'])
        check(state['error'].startswith('it runs code from an unknown source'), 'the refusal reads as said to the user: ' + state['error'])
        refused = state['installed']['code']
        check(len(refused) == 1 and refused[0]['status'] == 'unknown' and refused[0]['key'] == keys['publisher'][0] and refused[0]['publicKey'] == keys['publisher'][1],
              'the refusal names the public key the code is signed by, to trust: ' + json.dumps(refused))
        # Refused code installs none of what it needs either: the asset library stays out.
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.analog'})
        check(state['state'] == 'failed' and 'unknown source' in state['error'] and state['installed']['added'] == 0 and
              not (home / 'AssetLibraries' / 'org.kick.factory').exists() and store() == [] and leftovers() == [],
              'refused code leaves out the asset library it needs: ' + json.dumps(state)[:400])
        rpc('trust.set', {'key': keys['publisher'][1], 'name': 'KICK', 'scope': ['plugins']})
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth'})
        plugins = store()
        check(state['state'] == 'done' and state['version'] == '1.0.0' and state['installed']['code'][0]['status'] == 'trusted' and leftovers() == [],
              'the newest version this device can install installs: ' + json.dumps(state)[:300])
        check(len(plugins) == 1 and plugins[0]['id'] == 'org.kick.test.synth' and plugins[0]['platforms'] == [here], 'its build is in the store: ' + json.dumps(plugins))
        check(plugins[0].get('license') == 'MIT', 'the store keeps its license: ' + json.dumps(plugins))
        check(recorded() == [{'id': 'org.kick.test.synth', 'version': '1.0.0', 'kind': 'plugin', 'repository': str(good)}] and (home / 'packages.json').is_file(),
              'the install is recorded with its version, kind and repository: ' + json.dumps(recorded()))

        # Refused, each with its reason, installing nothing.
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.other'})
        check(state['state'] == 'failed' and 'no build for this device' in state['error'] and len(store()) == 1, 'a build this device cannot run is not installed: ' + state['error'])
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth', 'version': '2.0.0'})
        check(state['state'] == 'failed' and 'system update' in state['error'], 'a version needing a newer system is not installed: ' + state['error'])
        state = job('packages.install', {'source': str(good), 'id': 'org.example.none'})
        check(state['state'] == 'failed' and 'no package' in state['error'], 'a package it does not have: ' + state['error'])

        tampered = temp / 'tampered'
        shutil.copytree(good, tampered)
        blob = next(b for b in (tampered / 'blobs').rglob('*') if b.is_file() and b.read_bytes() == synth.read_bytes())
        data = bytearray(blob.read_bytes())
        data[len(data) // 2] ^= 1
        blob.write_bytes(bytes(data))
        rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.test.synth', 'version': '1.0.0'})
        check(recorded() == [] and 'org.kick.test.synth' not in (home / 'packages.json').read_text(), 'removing its plugin removes the record: ' + json.dumps(recorded()))
        state = job('packages.install', {'source': str(tampered), 'id': 'org.kick.test.synth'})
        check(state['state'] == 'failed' and 'not the one the index names' in state['error'] and store() == [] and leftovers() == [],
              'a tampered blob is refused, leaving nothing: ' + state['error'])
        blob.unlink()
        state = job('packages.install', {'source': str(tampered), 'id': 'org.kick.test.synth'})
        check(state['state'] == 'failed' and 'missing' in state['error'], 'a blob not on the drive is refused: ' + state['error'])

        forged = temp / 'forged'
        shutil.copytree(good, forged)
        root = json.loads((forged / 'repository.json').read_text())
        root['name'] = 'Forged'
        (forged / 'repository.json').write_text(json.dumps(root))
        state = job('packages.list', {'source': str(forged)})
        check(state['state'] == 'failed' and 'signature does not verify' in state['error'], 'a root whose signature does not verify is refused: ' + state['error'])
        run(tool, 'file', temp / 'other.key', forged / 'repository.json', forged / 'repository.sig')
        state = job('packages.list', {'source': str(forged)})
        check(state['state'] == 'failed' and 'not signed by its index key' in state['error'], 'a root signed by another key is refused: ' + state['error'])

        older = site('older', 6)
        state = job('packages.list', {'source': str(older)})
        check(state['state'] == 'failed' and 'older than one this device has seen' in state['error'], 'an older root is refused: ' + state['error'])

        # A repository the user added whose root claims this one's ID, with another root key and
        # one package fewer (RFC 0031: kept state keyed by the root key). Each is its own
        # repository: neither takes the other's serial or kept index, and each refuses its own
        # older root.
        fake = temp / 'impostor-submissions'
        shutil.copytree(repo, fake)
        shutil.rmtree(fake / 'packages' / 'org.kick.other')
        (temp / 'impostor-index-key.pub').write_text(keys['impostor-index'][1] + '\n')
        run(tool, 'file', temp / 'impostor.key', temp / 'impostor-index-key.pub', temp / 'impostor-index-key.sig')
        fake_config = json.loads((fake / 'repository-config.json').read_text())
        fake_config.update(name='Impostor', rootKey=keys['impostor'][1],
                           indexKey={'key': keys['impostor-index'][1], 'signature': json.loads((temp / 'impostor-index-key.sig').read_text())[0]['signature']})
        (fake / 'repository-config.json').write_text(json.dumps(fake_config))

        def kept():  # each kept index: its path under Repositories/, serial and package count
            return {str(f.relative_to(home / 'Repositories')): (json.loads(f.read_text())['serial'], len(json.loads(f.read_text())['packages']))
                    for f in (home / 'Repositories').rglob('*.index.json')}

        def lists(source, serial, count, what):
            state = job('packages.list', {'source': str(source)})
            check(state['state'] == 'done' and state['serial'] == serial and len(state['packages']) == count,
                  what + ': ' + (state['error'] or f"serial {state['serial']}, {len(state['packages'])} packages, {state['name']}") + ' ' + json.dumps(kept()))

        rpc('packages.addRepository', {'address': str(temp / 'impostor7'), 'key': keys['impostor'][1], 'name': 'Impostor'})
        lists(site('impostor7', 7, source=fake, index='impostor-index'), 7, 3, 'a root claiming another repository\'s ID at its serial lists its own packages')
        lists(good, 7, 4, 'the repository it claims to be still lists its own packages, not the impostor\'s')
        lists(site('impostor9', 9, source=fake, index='impostor-index'), 9, 3, 'the impostor at a higher serial lists')
        lists(good, 7, 4, 'the repository it claims to be is not refused as older')
        check(kept().get(f"{keys['root'][1]}/org.example.test.index.json") == (7, 4) and kept().get(f"{keys['impostor'][1]}/org.example.test.index.json") == (9, 3),
              'each kept index is kept by its root key: ' + json.dumps(kept()))
        state = job('packages.list', {'source': str(site('impostor8', 8, source=fake, index='impostor-index'))})
        check(state['state'] == 'failed' and 'older than one this device has seen (9)' in state['error'], 'the impostor\'s own older root is refused: ' + state['error'])
        state = job('packages.list', {'source': str(older)})
        check(state['state'] == 'failed' and 'older than one this device has seen (7)' in state['error'], 'and so is the real one\'s, against its own serial: ' + state['error'])

        # A repository the user added that calls itself by the official repository's name: shown,
        # listed and named in its jobs by this device's name for it, told apart by its key; what
        # its root says is only a claim (claimedName).
        fake_config['name'] = 'Oscairn'
        (fake / 'repository-config.json').write_text(json.dumps(fake_config))
        named = site('impostor10', 10, source=fake, index='impostor-index')
        rpc('packages.addRepository', {'address': str(named), 'key': keys['impostor'][1], 'name': 'Oscairn'})
        shown = {r['address']: r['name'] for r in rpc('packages.repositories')['repositories']}
        told = f"Oscairn (key {keys['impostor'][0]})"
        state = job('packages.list', {'source': str(named)})
        refused = job('packages.install', {'source': str(named), 'id': 'org.example.none'})
        check(shown.get(str(named)) == told and shown['https://packages.oscairn.org'] == 'Oscairn', 'a repository calling itself "Oscairn" is listed apart from the official one: ' + json.dumps(shown))
        check(state['state'] == 'done' and state['name'] == told and state.get('claimedName') == 'Oscairn' and refused['name'] == told and refused['error'].endswith('in ' + told),
              'its listing and install jobs name it by this device\'s name, not the one it claims: ' + json.dumps({k: state.get(k) for k in ('name', 'claimedName')}) + ' ' + refused['error'])
        rpc('packages.removeRepository', {'address': str(named)})

        # An address is the repository recorded for it, by its key: another trusted repository's
        # root and listing served there are refused, naming both keys, and nothing of them is
        # read or kept (RFC 0031 has no root key rotation to allow for).
        swap = temp / 'swap'
        shutil.copytree(good, swap)
        rpc('packages.addRepository', {'address': str(swap), 'key': keys['root'][1], 'name': 'Test repository'})
        check(job('packages.list', {'source': str(swap)})['state'] == 'done', 'a repository added at a folder reads with its own root key')
        shutil.rmtree(swap)
        shutil.copytree(site('impostor11', 11, source=fake, index='impostor-index'), swap)
        seen, before = json.loads((home / 'repositories.json').read_text())['seen'], kept()
        state = job('packages.list', {'source': str(swap)})
        install = job('packages.install', {'source': str(swap), 'id': 'org.kick.test.synth'})
        check(state['state'] == 'failed' and keys['impostor'][0] in state['error'] and keys['root'][0] in state['error'] and state['packages'] == [] and
              install['state'] == 'failed' and keys['impostor'][0] in install['error'] and keys['root'][0] in install['error'] and store() == [] and leftovers() == [],
              'another trusted repository\'s root at a known address is neither listed nor installed from, naming both keys: ' + state['error'])
        check(json.loads((home / 'repositories.json').read_text())['seen'] == seen and kept() == before,
              'nothing of it is kept: the serials seen and the kept indexes are as they were: ' + json.dumps(kept()))
        rpc('packages.removeRepository', {'address': str(swap)})
        rpc('trust.set', {'key': keys['root'][1], 'name': 'Test repository', 'scope': ['repositories']})  # removing it took its key's trust for repositories
        rpc('packages.removeRepository', {'address': str(temp / 'impostor7')})

        # An unpacked package folder: the same rules as the archive; hidden files are skipped.
        unpacked = temp / 'unpacked'
        with zipfile.ZipFile(synth) as z:
            z.extractall(unpacked)
        (unpacked / '.DS_Store').write_text('desktop junk')
        contents = rpc('library.contents', {'file': str(unpacked)})
        check(contents['kind'] == 'plugin' and contents['code'][0]['status'] == 'trusted', 'an unpacked package folder is read: ' + json.dumps(contents)[:200])
        changed = temp / 'changed'
        shutil.copytree(unpacked, changed)
        bundle = next(f for f in changed.rglob('*.clap') if f.is_file())  # the fixture is one file
        bundle.write_bytes(bundle.read_bytes() + b'changed')
        error = rpc('library.install', {'file': str(changed)}, expect_error=True)
        check('not what the manifest says' in error['message'] and store() == [] and leftovers() == [], 'a folder package with a changed file is refused, leaving nothing: ' + error['message'])
        installed = rpc('library.install', {'file': str(unpacked)})
        check(installed['code'][0]['status'] == 'trusted' and len(store()) == 1 and leftovers() == [] and unpacked.is_dir() and any(unpacked.rglob('*.clap')),
              'and the unpacked one installs, read in place and left as it was')
        check(recorded() == [{'id': 'org.kick.test.synth', 'version': '1.0.0', 'kind': 'plugin', 'repository': ''}], 'installed from a file, it is recorded with no repository: ' + json.dumps(recorded()))

        # What an install came from (the owner's rule, 2026-11-11): the archive's SHA-256 as the
        # repository lists it, and its signer. The user's own build at the same ID and version is
        # told apart; an install from the repository keeps it unless asked to replace it.
        listing = {x['id']: x for x in job('packages.list', {'source': str(good)})['packages']}
        listed = {v['version']: v['sha256'] for v in listing['org.kick.test.synth']['versions']}
        check(listed['1.0.0'] == hashlib.sha256(synth.read_bytes()).hexdigest(), 'a listing gives each version its archive SHA-256: ' + json.dumps(listed))
        check(records()[0]['sha256'] == '' and records()[0]['signers'] == [keys['publisher'][0]] and not records()[0]['otherBuild'],
              'from a package folder there is no archive hash, only its signer: ' + json.dumps(records()))
        stored = home / 'Plugins' / 'clap' / 'org.kick.test.synth@1.0.0' / here / 'Synth.clap'
        rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.test.synth', 'version': '1.0.0'})
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth'})
        check(state['state'] == 'done' and records() == [{'id': 'org.kick.test.synth', 'version': '1.0.0', 'kind': 'plugin', 'repository': str(good),
                                                          'sha256': listed['1.0.0'], 'signers': [keys['publisher'][0]], 'otherBuild': False, 'withdrawn': None}],
              'from the repository, the listed archive hash and its signer are recorded: ' + json.dumps(records()))
        again = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth'})
        check(again['state'] == 'done' and again['installed']['notes'] == [] and records()[0]['sha256'] == listed['1.0.0'], 'the same build again changes nothing')
        # The user's own build, keeping the ID and version, signed by another key.
        own = temp / 'builds' / 'own' / 'Synth.clap'
        own.parent.mkdir(parents=True)
        own.write_bytes(fixture.read_bytes() + b'\0' * 16)  # past the end of the binary: it loads the same
        mine = temp / 'mine.olib'
        run(tool, 'build', temp / 'other.key', 'clap', 'org.kick.test.synth', '1.0.0', here, own)
        run(tool, 'plugin-package', '--license', 'MIT', '--key', temp / 'other.key', mine, 'clap', 'org.kick.test.synth', '1.0.0', 'Test Synth', f'{here}={own}')
        rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.test.synth', 'version': '1.0.0'})
        rpc('library.install', {'file': str(mine), 'acceptUnsignedCode': True})
        r = records()[0]
        check(r['sha256'] == hashlib.sha256(mine.read_bytes()).hexdigest() != listed['1.0.0'] and r['signers'] == [keys['other'][0]] and r['repository'] == '' and stored.read_bytes() == own.read_bytes(),
              'a build from a file is recorded by its own archive hash and signer: ' + json.dumps(r))
        # From the repository without asking: the different build is kept and said to be another build.
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth'})
        r = records()[0]
        check(state['state'] == 'done' and any('already installed; it was kept' in n for n in state['installed']['notes']) and stored.read_bytes() == own.read_bytes() and
              r['otherBuild'] and r['sha256'] == '' and r['signers'] == [] and r['repository'] == str(good),
              'an install from the repository keeps a different build, recorded as another build: ' + json.dumps(r))
        # Replacing it, asked for: refused while the open Performance plays it, then done and recorded.
        node = rpc('graph.add', {'path': str(stored), 'pluginId': 'org.kick.test.synth', 'name': 'mine'})['node']
        rpc('graph.commit')
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth', 'replace': True})
        check(state['state'] == 'failed' and 'open Performance plays' in state['error'] and stored.read_bytes() == own.read_bytes() and leftovers() == [],
              'the repository\'s build does not replace one the open Performance plays: ' + state['error'])
        rpc('graph.remove', {'node': node})
        rpc('graph.commit')
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.test.synth', 'replace': True})
        check(state['state'] == 'done' and state['installed']['notes'] == [] and stored.read_bytes() == fixture.read_bytes() and
              records() == [{'id': 'org.kick.test.synth', 'version': '1.0.0', 'kind': 'plugin', 'repository': str(good), 'sha256': listed['1.0.0'], 'signers': [keys['publisher'][0]], 'otherBuild': False, 'withdrawn': None}],
              'asked to, the repository\'s build replaces it and is recorded as the repository\'s: ' + json.dumps(records()))

        # A plugin package that needs an asset library: installing it from the repository pulls
        # the newest version of the library that is at least the one it needs (RFC 0009).
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.analog'})
        library = home / 'AssetLibraries' / 'org.kick.factory'
        check(state['state'] == 'done' and state['installed']['notes'] == [] and sorted(e.name for e in library.iterdir() if not e.name.startswith('.')) == ['1.1.0'] and (library / '.terms').read_text() == 'MIT\t1\t1\n' and
              (library / '1.1.0' / 'patches' / 'Pad.fxp').read_bytes() == b'another patch' and any(p['id'] == 'org.kick.analog' for p in store()) and leftovers() == [],
              'a plugin package installs with the asset library it needs: ' + json.dumps(state)[:400])
        # The library came only because the package needs it (RFC 0009 step 4): kept while the
        # package is installed, refused to uninstall, and collected once the package is removed.
        check(rpc('assets.collect')['libraries'] == [], 'a library a plugin package needs is not collected')
        error = rpc('assets.uninstall', {'library': 'org.kick.factory'}, expect_error=True)
        check(error['code'] == -32005 and 'used by' in error['message'], 'nor uninstalled while it needs it: ' + error['message'])
        analog = next(p for p in store() if p['id'] == 'org.kick.analog')
        rpc('plugins.remove', {'format': analog['format'], 'id': analog['id'], 'version': analog['version']})
        preview = rpc('assets.collect', {'remove': False})
        check(preview['libraries'] == ['org.kick.factory'] and preview['bytes'] > 0 and library.exists(), 'with the package gone, a preview names it: ' + json.dumps(preview))
        collected = rpc('assets.collect', {'remove': True})
        check(collected['libraries'] == ['org.kick.factory'] and not library.exists(), 'and collection removes it: ' + json.dumps(collected))
        check(not any((home / 'assets' / 'store').iterdir()), 'with its stored files')
        check([x['id'] for x in recorded()] == ['org.kick.test.synth'], 'the package removed and the library collected are no longer recorded: ' + json.dumps(recorded()))

        # An update (RFC 0031): an older version installed, the listing's newest stable version this
        # device can install is newer, and installing it replaces the old one and its record.
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.factory', 'version': '1.0.0'})
        check(state['state'] == 'done' and {'id': 'org.kick.factory', 'version': '1.0.0', 'kind': 'asset', 'repository': str(good)} in recorded(),
              'an asset package installed at an older version is recorded at it: ' + json.dumps(recorded()))
        listing = {x['id']: x for x in job('packages.list', {'source': str(good)})['packages']}
        def key(version):
            return tuple(int(n) for n in version.split('.'))
        newer = [v['version'] for v in listing['org.kick.factory']['versions'] if v['channel'] == 'stable' and not v['yanked'] and not v['incompatible'] and key(v['version']) > key('1.0.0')]
        check(newer == ['1.1.0'], 'the listing has a newer version to update to: ' + json.dumps(newer))
        state = job('packages.install', {'source': str(good), 'id': 'org.kick.factory'})
        check(state['state'] == 'done' and state['version'] == '1.1.0' and [x for x in recorded() if x['id'] == 'org.kick.factory'] == [{'id': 'org.kick.factory', 'version': '1.1.0', 'kind': 'asset', 'repository': str(good)}] and
              sorted(e.name for e in library.iterdir() if not e.name.startswith('.')) == ['1.1.0'], 'updated, the new version replaces the old one and its record: ' + json.dumps(recorded()))
        # Installed before installs were recorded: an asset library is derived at its version, a plugin package is not.
        # A record written before the archive hash was (no sha256) still reads, with none.
        (home / 'packages.json').write_text(json.dumps({'format': 'oscairn.packages', 'version': 1, 'packages': [
            {'id': 'org.kick.factory', 'version': '1.1.0', 'kind': 'asset', 'repository': str(good), 'plugins': []}]}))
        check(records() == [{'id': 'org.kick.factory', 'version': '1.1.0', 'kind': 'asset', 'repository': str(good), 'sha256': '', 'signers': [], 'otherBuild': False, 'withdrawn': None}],
              'an older record reads, with no archive hash: ' + json.dumps(records()))
        (home / 'packages.json').unlink()
        check(recorded() == [{'id': 'org.kick.factory', 'version': '1.1.0', 'kind': 'asset', 'repository': ''}] and any(x['id'] == 'org.kick.test.synth' for x in store()),
              'with no record, the asset library is derived and the plugin package is not: ' + json.dumps(recorded()))
        rpc('assets.uninstall', {'library': 'org.kick.factory'})
        check(recorded() == [], 'uninstalled, it is no longer listed: ' + json.dumps(recorded()))

        # The repositories this device knows (RFC 0031): the official one, built in, its root key
        # trusted for repositories only; the image's, from its profile; and the user's.
        official = 'https://packages.oscairn.org'
        official_key = '6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841'
        known = rpc('packages.repositories')['repositories']
        check(known == [{'address': official, 'name': 'Oscairn', 'key': official_key, 'id': '6a2dd1acf4d4e23c', 'origin': 'official', 'enabled': True}],
              'a device knows the official repository, on: ' + json.dumps(known))
        trusted = {k['key']: k for k in rpc('trust.list')['keys']}
        check(trusted.get(official_key, {}).get('scope') == ['repositories'] and not trusted[official_key]['revoked'] and official_key not in (home / 'trust.json').read_text(),
              'its root key is trusted for repositories only, built in rather than written to trust.json: ' + json.dumps(trusted.get(official_key)))
        publisher_key = 'db87e97dba5b24564c782f473c543681bd87c0818bc2e41f37d1f100f3360d69'
        check(trusted.get(publisher_key, {}).get('scope') == ['apps', 'content', 'plugins'] and publisher_key not in (home / 'trust.json').read_text(),
              'its publisher\'s key is trusted for plugins, apps and content, built in too: ' + json.dumps(trusted.get(publisher_key)))
        error = rpc('packages.removeRepository', {'address': official}, expect_error=True)
        check('not removed' in error['message'], 'the official repository cannot be removed: ' + error['message'])
        # The image's repositories, and hiding the official one (discouraged): off by default, still listed.
        device = {'address': 'https://packages.kick.example/', 'key': keys['other'][1], 'name': 'KICK'}
        rpc('packages.setDeviceRepositories', {'repositories': [device], 'hideOfficial': True})
        known = {r['address']: r for r in rpc('packages.repositories')['repositories']}
        check(known[official]['enabled'] is False and known['https://packages.kick.example'] == {**device, 'address': 'https://packages.kick.example', 'id': keys['other'][0], 'origin': 'device', 'enabled': True},
              'the image\'s repository is listed, and a hidden official one is off but still there: ' + json.dumps(known))
        trusted = {k['key']: k for k in rpc('trust.list')['keys']}
        check(trusted.get(keys['other'][1], {}).get('scope') == ['repositories'], 'the image\'s root key is trusted for repositories')
        check(publisher_key not in trusted, 'a profile hiding the official repository leaves its publisher\'s key out')
        error = rpc('packages.removeRepository', {'address': 'https://packages.kick.example'}, expect_error=True)
        check('not removed' in error['message'], 'nor can the image\'s: ' + error['message'])
        rpc('packages.enableRepository', {'address': official, 'enabled': True})
        rpc('packages.enableRepository', {'address': 'https://packages.kick.example', 'enabled': False})
        known = {r['address']: r['enabled'] for r in rpc('packages.repositories')['repositories']}
        check(known == {official: True, 'https://packages.kick.example': False}, 'one switch turns each on or off: ' + json.dumps(known))
        check(json.loads((home / 'repositories.json').read_text())['enabled'] == {official: True, 'https://packages.kick.example': False} and
              json.loads((home / 'repositories.json').read_text())['seen'].get(keys['root'][1], {}).get('org.example.test') == 7, 'kept by address beside the serials seen')
        rpc('packages.setDeviceRepositories', {'repositories': []})
        check(publisher_key in {k['key'] for k in rpc('trust.list')['keys']}, 'not hidden, it is back')
        # The user's: by address and root key, which is trusted for repositories with it and
        # taken away with it.
        rpc('trust.remove', {'id': keys['root'][0]})
        check(job('packages.list', {'source': str(good)})['state'] == 'failed', 'a repository whose key is not trusted is not read')
        for bad, why in (({'address': 'relative/folder', 'key': keys['root'][1]}, 'address'), ({'address': str(good), 'key': 'abc'}, 'root key')):
            error = rpc('packages.addRepository', bad, expect_error=True)
            check(error['code'] == -32602 and why in error['message'], 'a malformed repository is refused: ' + error['message'])
        rpc('packages.addRepository', {'address': str(good), 'key': keys['root'][1], 'name': 'Test repository'})
        known = rpc('packages.repositories')['repositories']
        trusted = {k['key']: k for k in rpc('trust.list')['keys']}
        check(known[-1] == {'address': str(good), 'name': 'Test repository', 'key': keys['root'][1], 'id': keys['root'][0], 'origin': 'user', 'enabled': True} and
              trusted.get(keys['root'][1], {}).get('scope') == ['repositories'] and job('packages.list', {'source': str(good)})['state'] == 'done',
              'a repository added is listed, its root key trusted for repositories, and it reads: ' + json.dumps(known[-1]))
        rpc('packages.removeRepository', {'address': str(good)})
        known = rpc('packages.repositories')['repositories']
        check(len(known) == 1 and keys['root'][1] not in {k['key'] for k in rpc('trust.list')['keys']} and job('packages.list', {'source': str(good)})['state'] == 'failed',
              'removed, its root key is no longer trusted and it does not read: ' + json.dumps(known))
    finally:
        p.stdin.close()
        p.wait(timeout=30)

    # From version 1's layout, serials and kept indexes by the repository ID alone: each serial
    # goes to every root key trusted for repositories, so an older root is still refused, and the
    # old kept indexes go (an empty one here, which a listing would show if it were read).
    home = temp / 'home-version-1'
    (home / 'Repositories').mkdir(parents=True)
    (home / 'repositories.json').write_text(json.dumps({'format': 'oscairn.repositories', 'version': 1, 'seen': {'org.example.test': 7},
                                                        'added': [{'address': str(good), 'name': 'Test repository', 'key': keys['root'][1]}], 'enabled': {}}))
    (home / 'Repositories' / 'org.example.test.index.json').write_text(json.dumps({'format': 'oscairn.repository-index', 'version': 1, 'repository': 'org.example.test', 'serial': 7, 'packages': []}))
    p = subprocess.Popen([server, plugin, '--stdio'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8',
                         env={**os.environ, 'OSCAIRN_DATA_HOME': str(home)})
    try:
        rpc('trust.set', {'key': keys['root'][1], 'name': 'Test repository', 'scope': ['repositories']})
        state = job('packages.list', {'source': str(older)})
        check(state['state'] == 'failed' and 'older than one this device has seen (7)' in state['error'], 'after the move, an older root is still refused: ' + state['error'])
        lists(good, 7, 4, 'and the current one lists, not from the old kept index')
        written = json.loads((home / 'repositories.json').read_text())
        check(written['version'] == 2 and written['seen'] == {keys['root'][1]: {'org.example.test': 7}, official_key: {'org.example.test': 7}} and
              written['added'][0]['address'] == str(good) and kept() == {f"{keys['root'][1]}/org.example.test.index.json": (7, 4)},
              'the serial is kept for each trusted root key, and the old kept index is gone: ' + json.dumps(written['seen']) + ' ' + json.dumps(kept()))
    finally:
        p.stdin.close()
        p.wait(timeout=30)

    # Revocation lists (RFC 0031): the list a repository's signed root names by its hash withdraws
    # versions and revokes publisher keys for that repository's packages. A withdrawn version is
    # never offered or installed, even named; one installed already is reported, never removed; a
    # revoked key's packages are refused; an older list comes only with an older root, which is
    # refused; a missing or malformed list refuses the repository.
    home = temp / 'home-revocations'
    home.mkdir()
    at = temp / 'revoking'  # the repository's address, holding each site in turn

    def publish(name, serial, revoked=None, change=None):
        (repo / 'revoked.json').write_text(json.dumps({'format': 'oscairn.repository-revocations', 'version': 1, 'keys': [], 'builds': [], 'versions': [], **(revoked or {})}))
        out = site(name, serial)
        if change:  # the root changed after it was built, and signed again
            root = json.loads((out / 'repository.json').read_text())
            change(out, root)
            (out / 'repository.json').write_text(json.dumps(root))
            run(tool, 'file', temp / 'index.key', out / 'repository.json', out / 'repository.sig')
        shutil.rmtree(at, ignore_errors=True)
        shutil.copytree(out, at)
        return out

    def withdrawn(state, package):
        return {v['version']: v.get('withdrawn', 'absent') for x in state['packages'] if x['id'] == package for v in x['versions']}

    def record(package):
        return next((r for r in records() if r['id'] == package), {})

    revocations = home / 'Repositories' / keys['root'][1] / 'org.example.test.revocations.json'

    def kept_list():  # the revocation list this device keeps for the repository, as last read
        return revocations.read_text() if revocations.is_file() else ''
    p = subprocess.Popen([server, plugin, '--stdio'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8',
                         env={**os.environ, 'OSCAIRN_DATA_HOME': str(home)})
    try:
        rpc('trust.set', {'key': keys['publisher'][1], 'name': 'KICK', 'scope': ['plugins']})
        publish('revoking20', 20)
        rpc('packages.addRepository', {'address': str(at), 'key': keys['root'][1], 'name': 'Test repository'})
        state = job('packages.install', {'source': str(at), 'id': 'org.kick.test.synth', 'version': '1.0.0'})
        check(state['state'] == 'done' and record('org.kick.test.synth').get('withdrawn', 'absent') is None, 'installed while nothing is revoked, nothing is withdrawn: ' + json.dumps(records()))

        withdrawal = {'reason': 'a test withdrawal', 'date': '2026-11-11'}
        publish('revoking21', 21, {'versions': [{'id': 'org.kick.test.synth', 'version': '1.0.0', **withdrawal}]})
        state = job('packages.list', {'source': str(at)})
        check(state['state'] == 'done' and withdrawn(state, 'org.kick.test.synth') == {'1.0.0': {'repository': 'Test repository', **withdrawal}, '2.0.0': None},
              'a version the list withdraws is listed as withdrawn, by this device\'s name for the repository, so it is not offered: ' + json.dumps(withdrawn(state, 'org.kick.test.synth')))
        named = job('packages.install', {'source': str(at), 'id': 'org.kick.test.synth', 'version': '1.0.0'})
        newest = job('packages.install', {'source': str(at), 'id': 'org.kick.test.synth'})
        check(named['state'] == 'failed' and 'org.kick.test.synth 1.0.0 was withdrawn by Test repository: a test withdrawal' in named['error'] and
              newest['state'] == 'failed' and 'withdrawn' not in newest['error'] and newest['version'] != '1.0.0',
              'a withdrawn version is never installed, even named, nor chosen as the newest: ' + named['error'] + ' / ' + newest['error'])
        check(record('org.kick.test.synth').get('withdrawn') == {'repository': 'Test repository', **withdrawal} and [x['id'] for x in store()] == ['org.kick.test.synth'],
              'the version installed already is reported as withdrawn, and not removed: ' + json.dumps(record('org.kick.test.synth')))

        revocation = {'reason': 'a test key revocation', 'date': '2026-11-12'}
        publish('revoking22', 22, {'versions': [{'id': 'org.kick.test.synth', 'version': '1.0.0', **withdrawal}], 'keys': [{'key': keys['publisher'][1], **revocation}]})
        state = job('packages.list', {'source': str(at)})
        refused = job('packages.install', {'source': str(at), 'id': 'org.kick.factory', 'version': '1.1.0'})
        check(state['state'] == 'done' and withdrawn(state, 'org.kick.factory') == {'1.0.0': {'repository': 'Test repository', **revocation}, '1.1.0': {'repository': 'Test repository', **revocation}} and
              refused['state'] == 'failed' and 'org.kick.factory 1.1.0 was withdrawn by Test repository: a test key revocation' in refused['error'] and
              not (home / 'AssetLibraries' / 'org.kick.factory').exists(),
              'a publisher key the list revokes withdraws every package it signs, and none installs: ' + refused['error'])

        shutil.rmtree(at)
        shutil.copytree(temp / 'revoking21', at)
        state = job('packages.list', {'source': str(at)})
        check(state['state'] == 'failed' and 'older than one this device has seen (22)' in state['error'] and keys['publisher'][1] in kept_list(),
              'an older list comes only with its older root, which is refused; the newer list stays kept: ' + state['error'])

        def malformed(out, root):
            bad = b'{"format":"oscairn.repository-revocations","version":1,"keys":"none","builds":[],"versions":[]}'
            sha = hashlib.sha256(bad).hexdigest()
            (out / 'revocations' / f'{sha}.json').write_bytes(bad)
            root['revocations'] = {'path': f'revocations/{sha}.json', 'sha256': sha, 'size': len(bad)}
        before = kept_list()
        for name, revoked, change, why in (('revoking23', {'versions': [{'id': 'org.kick.test.synth', 'version': '1.0.0', 'reason': 'no date'}]}, None, 'revocation list: date'),
                                           ('revoking23b', None, malformed, 'has no `keys` list'),
                                           ('revoking23c', None, lambda out, root: root.pop('revocations'), 'names no revocation list')):
            publish(name, 23, revoked, change)
            state = job('packages.list', {'source': str(at)})
            check(state['state'] == 'failed' and why in state['error'], 'a revocation list that is malformed or missing refuses the repository: ' + state['error'])
        check(json.loads((home / 'repositories.json').read_text())['seen'][keys['root'][1]]['org.example.test'] == 22 and kept_list() == before,
              'and nothing of such a root is kept: ' + json.dumps(json.loads((home / 'repositories.json').read_text())['seen']))
    finally:
        p.stdin.close()
        p.wait(timeout=30)
sys.exit(1 if failures else 0)
