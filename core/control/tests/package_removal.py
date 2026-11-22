# SPDX-License-Identifier: AGPL-3.0-or-later
"""Removing an installed package of any kind (packages.removal, packages.uninstall), through RPC.

usage: package_removal.py RPC-SERVER PLUGIN OSCAIRN-SIGN FIXTURE.clap

Packages installed from files: an engine (a plugin package needing an asset library) with its init
library, two libraries sharing an asset package, and a library alone with one. What uses a package
is said and a removal refused, with nothing removed, until asked for anyway; what nothing else
would use is listed, and removed only when named; the open Performance's plugin is never removed.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import platform as host
from pathlib import Path

server, plugin, tool, fixture = (Path(a).resolve() for a in sys.argv[1:5])
machine = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}[host.machine()]
here = ('macos' if sys.platform == 'darwin' else 'linux') + '-' + machine
SYNTH = 'org.kick.test.synth'  # the fixture's own CLAP ID, so a Performance that plays it names the package's plugin
failures = 0


def check(ok, what):
    global failures
    print(('ok   ' if ok else 'FAIL ') + what)
    failures += not ok


def run(*command):
    out = subprocess.run([str(c) for c in command], capture_output=True, text=True, timeout=300)
    assert out.returncode == 0, (command, out.stderr)
    return out.stdout


with tempfile.TemporaryDirectory(prefix='oscairn-removal-') as temporary:
    temp = Path(temporary).resolve()  # the store compares paths as the engine resolved them
    key = temp / 'publisher.key'
    public = run(tool, 'key', key).split()[2]

    def asset(package_id):
        folder = temp / 'trees' / package_id
        folder.mkdir(parents=True)
        (folder / 'wave.bin').write_bytes(package_id.encode() * 64)
        run(tool, 'asset-package', '--license', 'MIT', '--key', key, temp / f'{package_id}.olib', package_id, '1.0.0', package_id + ' content', folder)

    def library(package_id, name, needs, least='1.0.0'):
        tree = temp / 'trees' / package_id
        tree.mkdir(parents=True)
        run(tool, 'library-package', '--license', 'MIT', '--key', key, '--package-dependency', needs, least, temp / f'{package_id}.olib', package_id, name, tree)

    for a in ('org.kick.factory', 'org.kick.shared', 'org.kick.only'):
        asset(a)
    build = temp / 'builds' / 'Synth.clap'
    build.parent.mkdir()
    shutil.copyfile(fixture, build)
    run(tool, 'build', key, 'clap', SYNTH, '1', here, build)  # the version the fixture itself reports, which a Performance records
    run(tool, 'plugin-package', '--license', 'MIT', '--asset-dependency', 'org.kick.factory', '1.0.0', '--key', key, temp / f'{SYNTH}.olib', 'clap', SYNTH, '1', 'Test Synth', f'{here}={build}')
    library('org.kick.init', 'Synth Init', SYNTH, '1')
    library('org.kick.a', 'Pack A', 'org.kick.shared')
    library('org.kick.b', 'Pack B', 'org.kick.shared')
    library('org.kick.solo', 'Solo Pack', 'org.kick.only')

    home = temp / 'home'
    home.mkdir()
    p = subprocess.Popen([server, plugin, '--stdio'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8',
                         env={**os.environ, 'OSCAIRN_DATA_HOME': str(home)})

    def rpc(method, params=None, expect_error=False):
        p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}) + '\n')
        p.stdin.flush()
        response = json.loads(p.stdout.readline())
        assert ('error' in response) == expect_error, response
        return response['error']['message'] if expect_error else response.get('result')

    def install(package_id, as_library=False):
        rpc('library.install', {'file': str(temp / f'{package_id}.olib'), 'asLibrary': as_library})

    def installed():
        return sorted(r['id'] for r in rpc('packages.installed')['packages'])

    def removal(package_id):
        r = rpc('packages.removal', {'id': package_id})
        return [(d['kind'], d['id'], d['items']) for d in r['dependents']], [u['id'] for u in r['unused']], r

    try:
        rpc('trust.set', {'key': public, 'name': 'KICK', 'scope': ['plugins']})
        for a in ('org.kick.factory', 'org.kick.shared', 'org.kick.only', SYNTH):
            install(a)
        for a in ('org.kick.init', 'org.kick.a', 'org.kick.b', 'org.kick.solo'):
            install(a, True)
        everything = installed()
        names = {r['id']: (r['name'], r['kind']) for r in rpc('packages.installed')['packages']}
        check(len(everything) == 8 and names[SYNTH] == ('Test Synth', 'plugin') and names['org.kick.a'] == ('Pack A', 'library') and
              names['org.kick.only'] == ('org.kick.only content', 'asset'), 'each installed package has a name to show: ' + json.dumps(names))

        for method in ('packages.removal', 'packages.uninstall'):
            check('no package org.kick.nothing' in rpc(method, {'id': 'org.kick.nothing'}, expect_error=True), method + ' refuses an unknown ID')
        check('no package' in rpc('packages.removal', {'id': SYNTH, 'version': '9.9.9'}, expect_error=True), 'and a version not installed')

        # A library alone with its asset package; two libraries sharing one.
        dependents, unused, whole = removal('org.kick.only')
        check(dependents == [('library', 'org.kick.solo', 0)] and whole['dependents'][0]['name'] == 'Solo Pack' and
              whole['package'] == {'id': 'org.kick.only', 'version': '1.0.0', 'kind': 'asset', 'name': 'org.kick.only content', 'bytes': whole['package']['bytes']},
              'an asset package a library needs names the library: ' + json.dumps(whole))
        error = rpc('packages.uninstall', {'id': 'org.kick.only'}, expect_error=True)
        check('used by Solo Pack' in error and installed() == everything, 'and is not removed, nothing changed: ' + error)
        dependents, unused, whole = removal('org.kick.solo')
        check(dependents == [] and unused == ['org.kick.only'] and whole['unused'][0]['bytes'] > 0 and whole['unused'][0]['kind'] == 'asset',
              'a library whose asset package nothing else uses lists it as unused, with its size: ' + json.dumps(whole))
        check(removal('org.kick.a')[:2] == ([], []) and removal('org.kick.b')[:2] == ([], []), 'two libraries sharing an asset package: unused is empty for either')
        removed = rpc('packages.uninstall', {'id': 'org.kick.solo'})['removed']
        check([r['id'] for r in removed] == ['org.kick.solo'] and 'org.kick.only' in installed() and 'org.kick.solo' not in installed(),
              'without withUnused the library goes and its asset package stays, listed as before: ' + json.dumps(removed))
        install('org.kick.solo', True)
        removed = rpc('packages.uninstall', {'id': 'org.kick.solo', 'withUnused': ['org.kick.only', 'org.kick.shared']})['removed']
        check([r['id'] for r in removed] == ['org.kick.solo', 'org.kick.only'] and installed() == [e for e in everything if e not in ('org.kick.solo', 'org.kick.only')],
              'with it both go, and one named that something still uses is left: ' + json.dumps(removed))
        rpc('packages.uninstall', {'id': 'org.kick.a'})
        check(removal('org.kick.b')[1] == ['org.kick.shared'], 'once one of the two is gone, the other is the last to use it')
        removed = rpc('packages.uninstall', {'id': 'org.kick.b', 'withUnused': True})['removed']
        check([r['id'] for r in removed] == ['org.kick.b', 'org.kick.shared'], 'and withUnused true takes every unused one: ' + json.dumps(removed))

        # The engine: a plugin package, the asset library it needs, and its init library.
        dependents, unused, whole = removal(SYNTH)
        check(dependents == [('library', 'org.kick.init', 0)] and unused == ['org.kick.factory'], 'an engine is needed by its init library, and its asset library would be unused: ' + json.dumps(whole))
        check(removal('org.kick.init')[1] == [SYNTH, 'org.kick.factory'], 'the init library\'s unused dependencies are the engine and, through it, its asset library')
        check(removal('org.kick.factory')[0] == [('plugin', SYNTH, 0)], 'the asset library is needed by the engine')
        # A Performance that plays it: never removed under it; then one of the user's own, then a library's item.
        stored = next((home / 'Plugins' / 'clap').glob(f'*/{here}/*.clap'))
        node = rpc('graph.add', {'path': str(stored), 'pluginId': SYNTH, 'name': 'keys'})['node']
        rpc('graph.commit')
        check(('open', '', 0) in removal(SYNTH)[0], 'the open Performance that plays it is a dependent')
        error = rpc('packages.uninstall', {'id': SYNTH, 'evenIfUsed': True}, expect_error=True)
        check('open Performance uses Test Synth' in error and installed() == ['org.kick.factory', 'org.kick.init', SYNTH], 'and it is not removed under it, even when asked anyway: ' + error)
        item = rpc('library.save', {'name': 'Keys Split'})['id']
        rpc('library.exportLibrary', {'file': str(temp / 'mine.olib'), 'name': 'My Pack'})
        rpc('graph.remove', {'node': node})
        rpc('graph.commit')
        rpc('memory.release')
        dependents, unused, whole = removal(SYNTH)
        own = [d for d in whole['dependents'] if d['kind'] == 'own']
        check(len(own) == 1 and own[0]['items'] == 1 and own[0]['examples'] == ['Keys Split'] and ('open', '', 0) not in dependents,
              'a Performance of the user\'s own that uses it is counted and named: ' + json.dumps(whole['dependents']))
        rpc('library.delete', {'id': item})
        rpc('library.install', {'file': str(temp / 'mine.olib'), 'asLibrary': True, 'acceptUnsignedCode': True})  # the test server's own synth is unsigned
        dependents, unused, whole = removal(SYNTH)
        check(dependents == [('library', 'org.kick.init', 0), ('library', 'My-Pack', 1)], 'a library whose item uses it is named with its count: ' + json.dumps(whole['dependents']))
        before = installed()
        error = rpc('packages.uninstall', {'id': SYNTH, 'withUnused': True}, expect_error=True)
        check('Synth Init' in error and 'My Pack (1 item)' in error and installed() == before and rpc('plugins.list')['plugins'] != [],
              'removing a plugin package libraries need is refused, naming them and the count, nothing removed: ' + error)
        removed = rpc('packages.uninstall', {'id': SYNTH, 'withUnused': True, 'evenIfUsed': True})['removed']
        check([r['id'] for r in removed] == [SYNTH, 'org.kick.factory'] and installed() == ['My-Pack', 'org.kick.init'] and SYNTH not in [x['id'] for x in rpc('plugins.list')['plugins']],
              'asked anyway it goes, with its unused asset library; the libraries stay: ' + json.dumps(removed))
    finally:
        p.stdin.close()
        p.wait(timeout=30)
sys.exit(1 if failures else 0)
