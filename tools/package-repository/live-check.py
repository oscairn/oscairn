# SPDX-License-Identifier: AGPL-3.0-or-later
"""Installs from the live package repository, as a user would.

usage: live-check.py BUILD-DIR [SOURCE]

BUILD-DIR is a desktop build (OSCAIRN_STANDALONE, which has the HTTPS transport); SOURCE is the
repository's address, https://packages.oscairn.org by default. It needs the network, so it is not
part of ctest. Everything goes into a throwaway data folder (OSCAIRN_DATA_HOME); no audio device
is opened.

Through the control API (oscairn-web-rpc-test): the official repository is known and its root key
trusted from the start, for repositories only, and its publisher's key for plugins, apps and
content (another source is refused until its root key is trusted, naming the key); it lists its
packages. VA, FM and the effects install with no yes, their signer shown as trusted. FM lists no factory presets, which oscairn-installed-play needs to play one, so nothing is played here.
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# What the official repository publishes (github.com/oscairn/packages: repository-config.json and
# publishers/org.oscairn.json), as a user would be given them.
ROOT_KEY = '6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841'
PUBLISHER_KEY = 'db87e97dba5b24564c782f473c543681bd87c0818bc2e41f37d1f100f3360d69'
# FM ships no asset library and lists no factory presets (an init patch only), so it is installed and checked in the store, not played here.
INSTRUMENTS = ['org.oscairn.fm']
VA = 'org.oscairn.va'
EFFECTS = 'org.oscairn.effects'

build = Path(sys.argv[1]).resolve()
source = sys.argv[2] if len(sys.argv) > 2 else 'https://packages.oscairn.org'
web = next(build.glob('hosts/audio/oscairn_web_rpc_test_artefacts/*/oscairn_web_rpc_test'))
failures = 0


def check(ok, what):
    global failures
    print(('ok   ' if ok else 'FAIL ') + what, flush=True)
    failures += not ok
    return ok


def key_id(key):
    return hashlib.sha256(bytes.fromhex(key)).hexdigest()[:16]


with tempfile.TemporaryDirectory(prefix='oscairn-live-') as temporary:
    home = Path(temporary)
    env = {**os.environ, 'OSCAIRN_DATA_HOME': str(home)}
    p = subprocess.Popen([web], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8', env=env)

    def rpc(method, params=None):
        p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}) + '\n')
        p.stdin.flush()
        response = json.loads(p.stdout.readline())
        assert 'error' not in response, response
        return response['result']

    def job(method, params):
        number = rpc(method, params)['job']
        for _ in range(6000):
            state = rpc('packages.job', {'job': number})
            if state['state'] != 'running':
                return state
            time.sleep(0.1)
        raise TimeoutError(method)

    def stored():
        return sorted(f.name for f in (home / 'Plugins' / 'clap').iterdir()) if (home / 'Plugins' / 'clap').is_dir() else []

    try:
        # 1. The official repository ships known, its root key trusted for repositories and nothing
        # else; another source is refused, naming its root key, until that key is trusted.
        if source == 'https://packages.oscairn.org':
            known = rpc('packages.repositories')['repositories']
            check(known and known[0]['address'] == source and known[0]['key'] == ROOT_KEY and known[0]['enabled'] and known[0]['origin'] == 'official',
                  f'a fresh data folder knows the official repository first, on: {known[:1]}')
            keys = rpc('trust.list')['keys']
            check([(k['key'], k['scope']) for k in keys] == [(PUBLISHER_KEY, ['apps', 'content', 'plugins']), (ROOT_KEY, ['repositories'])],
                  f'and trusts only its publisher key and its root key, built in: {keys}')
        else:
            for method, params in (('packages.list', {'source': source}), ('packages.install', {'source': source, 'id': VA})):
                state = job(method, params)
                check(state['state'] == 'failed' and 'not a repository this device trusts' in state['error'] and key_id(ROOT_KEY) in state['error'] and state['rootKey'] == ROOT_KEY,
                      f'{method} before trusting the root is refused, naming its key: ' + state['error'])
            check(stored() == [] and not (home / 'AssetLibraries').exists(), 'and nothing is installed')
            rpc('trust.set', {'key': ROOT_KEY, 'name': 'Oscairn', 'scope': ['repositories']})
        listing = job('packages.list', {'source': source})
        offered = {x['id']: x for x in listing['packages']}
        check(listing['state'] == 'done' and listing['repository'] == 'org.oscairn.main', f"once trusted, it lists {len(offered)} packages at serial {listing['serial']}: {listing['error']}")
        for wanted in [VA, *INSTRUMENTS, EFFECTS]:
            x = offered.get(wanted)
            check(x is not None and x['license'] and x['publisherKey'] == key_id(PUBLISHER_KEY) and any(not v['incompatible'] for v in x['versions']),
                  f"{wanted} {x['versions'][0]['version'] if x else ''} is offered, installable here, by {key_id(PUBLISHER_KEY)}, under {x['license'] if x else '?'}")

        # 2. The official publisher is trusted out of the box: its code installs with no yes, each signer trusted.
        for package in (VA, *INSTRUMENTS, EFFECTS):
            state = job('packages.install', {'source': source, 'id': package})
            codes = state['installed']['code']
            check(state['state'] == 'done' and codes and all(c['status'] == 'trusted' and c['keyName'] == 'Oscairn' for c in codes),
                  f"{package} {state['version']} installs with no yes, each signer shown as trusted: {state['error']} " + ', '.join(f"{c['name']} {c['status']}" for c in codes))

        # 3. The plugin store, its licenses, and each asset library file for file as its archive holds it.
        plugins = {x['id']: x for x in rpc('plugins.list')['plugins']}
        for x in plugins.values():
            folder = home / 'Plugins' / x['format'] / f"{x['id']}@{x['version']}"
            builds = list(folder.glob('*/*.clap'))
            check(builds and all(b.stat().st_size > 0 for b in builds) and any((folder / 'Licenses').iterdir()),
                  f"{x['name']} {x['version']} is in the store ({x['bytes']} bytes, {', '.join(x['platforms'])}) with {len(list((folder / 'Licenses').iterdir()))} license files")
        check(all(i in plugins for i in (VA, *INSTRUMENTS)) and any(i.startswith(EFFECTS + '.') for i in plugins), 'plugins.list has VA, FM and the effects')
    finally:
        p.stdin.close()
        p.wait(timeout=30)

print('PASS' if not failures else f'{failures} FAILED')
sys.exit(1 if failures else 0)
