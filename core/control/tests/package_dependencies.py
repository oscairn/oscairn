# SPDX-License-Identifier: AGPL-3.0-or-later
"""Packages that need packages (dependencies.packages, docs/library-archive.md), through RPC.

usage: package_dependencies.py RPC-SERVER PLUGIN OSCAIRN-SIGN FIXTURE.clap BUILD-INDEX.py TYPE-MANIFEST

A repository built with the template's index builder holds a chain of three packages of three
kinds: a plugin package needing a content library, which needs a types package declaring a type
kept by a handler. Installing the plugin package installs the other two first, in order; a pair of
packages needing each other is refused, naming the cycle; a package needing one the repository
lacks is refused naming it, with nothing installed; a package another needs is not uninstalled
while that one is there, and is once it is gone.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import platform as host
from pathlib import Path

server, plugin, tool, fixture, builder, type_manifest = (Path(a).resolve() for a in sys.argv[1:7])
machine = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}[host.machine()]
here = ('macos' if sys.platform == 'darwin' else 'linux') + '-' + machine
failures = 0


def check(ok, what):
    global failures
    print(('ok   ' if ok else 'FAIL ') + what)
    failures += not ok


def run(*command):
    out = subprocess.run([str(c) for c in command], capture_output=True, text=True, timeout=300)
    assert out.returncode == 0, (command, out.stderr)
    return out.stdout


with tempfile.TemporaryDirectory(prefix='oscairn-dependencies-') as temporary:
    temp = Path(temporary)
    keys = {}
    for name in ('publisher', 'root', 'index'):
        _, key_id, public = run(tool, 'key', temp / f'{name}.key').split()
        keys[name] = (key_id, public)
    repo = temp / 'submissions'
    shutil.copytree(builder.parents[1], repo)

    def record(package_id, olib, platforms, needs=(), requires=None):
        data = olib.read_bytes()
        dependencies = {'dependencies': {'assets': [], 'packages': [{'id': n, 'version': '1.0.0'} for n in needs]}} if needs else {}
        if requires:
            dependencies['requires'] = {'oscairn': requires}
        (olib.parent / f'{olib.stem}.json').write_text(json.dumps({**dependencies,
            'format': 'oscairn.repository-release', 'version': 1, 'id': package_id, 'release': olib.stem, 'publisher': 'org.kick',
            'summary': 'A test package', 'license': 'MIT', 'tags': [], 'platforms': platforms,
            'archive': {'path': olib.name, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}, 'yanked': None}))

    def library(package_id, needs=(), types=None, requires=None):
        """A library package with no items: the types it declares (as a type manifest's folder), needing `needs`."""
        olib = repo / 'packages' / package_id / '1.0.0.olib'
        olib.parent.mkdir(parents=True)
        tree = temp / 'trees' / package_id
        tree.mkdir(parents=True)
        if types:
            (tree / 'Types' / package_id).mkdir(parents=True)
            manifest = json.loads(types.read_text())
            manifest['id'] = package_id + '.sample'
            (tree / 'Types' / package_id / 'sample.json').write_text(json.dumps(manifest))
        depends = [a for n in needs for a in ('--package-dependency', n, '1.0.0')]
        run(tool, 'library-package', '--license', 'MIT', '--key', temp / 'publisher.key', *depends, olib, package_id, package_id, tree)
        record(package_id, olib, [], needs, requires)

    def plugin_package(package_id, needs):
        olib = repo / 'packages' / package_id / '1.0.0.olib'
        olib.parent.mkdir(parents=True)
        build = temp / 'builds' / package_id / 'Synth.clap'
        build.parent.mkdir(parents=True)
        shutil.copyfile(fixture, build)
        run(tool, 'build', temp / 'publisher.key', 'clap', package_id, '1.0.0', here, build)
        run(tool, 'plugin-package', '--license', 'MIT', '--package-dependency', needs, '1.0.0', '--key', temp / 'publisher.key',
            olib, 'clap', package_id, '1.0.0', 'Test Keys', f'{here}={build}')
        record(package_id, olib, [here], [needs])

    library('org.kick.types', types=type_manifest)
    library('org.kick.content', needs=['org.kick.types'])
    plugin_package('org.kick.keys', 'org.kick.content')
    library('org.kick.loop-a', needs=['org.kick.loop-b'])
    library('org.kick.loop-b', needs=['org.kick.loop-a'])
    # The repository's checks refuse a dependency it does not hold at all; one only for a newer system is as absent to this device.
    library('org.kick.absent', requires='99')
    library('org.kick.needy', needs=['org.kick.absent'])

    (temp / 'index-key.pub').write_text(keys['index'][1] + '\n')
    run(tool, 'file', temp / 'root.key', temp / 'index-key.pub', temp / 'index-key.sig')
    config = json.loads((repo / 'repository-config.json').read_text())
    config.update(repository='org.example.dependencies', name='Test repository', rootKey=keys['root'][1],
                  indexKey={'key': keys['index'][1], 'signature': json.loads((temp / 'index-key.sig').read_text())[0]['signature']})
    (repo / 'repository-config.json').write_text(json.dumps(config))
    (repo / 'publishers' / 'org.kick.json').write_text(json.dumps({
        'format': 'oscairn.repository-publisher', 'version': 1, 'id': 'org.kick', 'name': 'KICK', 'key': keys['publisher'][1],
        'verification': 'https://kick.example/.well-known/oscairn-publisher.json'}))
    site = temp / 'site'
    built = subprocess.run([sys.executable, '-I', builder, '--repo', repo, '--oscairn-sign', tool, '--cache', temp / 'cache',
                            '--out', site, '--serial', '1', '--exports', temp / 'exports'], capture_output=True, text=True, timeout=300)
    assert built.returncode == 0, built.stderr
    run(tool, 'file', temp / 'index.key', site / 'repository.json', site / 'repository.sig')
    for blob in (site / 'blobs.txt').read_text().split():
        (site / 'blobs' / blob[:2]).mkdir(parents=True, exist_ok=True)
        shutil.copyfile(temp / 'cache' / blob, site / 'blobs' / blob[:2] / blob)

    home = temp / 'home'
    home.mkdir()
    p = subprocess.Popen([server, plugin, '--stdio'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding='utf-8',
                         env={**os.environ, 'OSCAIRN_DATA_HOME': str(home)})

    def rpc(method, params=None, expect_error=False):
        p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}) + '\n')
        p.stdin.flush()
        response = json.loads(p.stdout.readline())
        assert ('error' in response) == expect_error, response
        return response.get('error') if expect_error else response.get('result')

    def job(params):
        number = rpc('packages.install', params)['job']
        for _ in range(600):
            state = rpc('packages.job', {'job': number})
            if state['state'] != 'running':
                return state
            time.sleep(0.05)
        raise TimeoutError(params)

    def installed():
        return [r['id'] for r in rpc('packages.installed')['packages']]

    try:
        rpc('trust.set', {'key': keys['root'][1], 'name': 'Test repository', 'scope': ['repositories']})
        rpc('trust.set', {'key': keys['publisher'][1], 'name': 'KICK', 'scope': ['plugins']})
        state = job({'source': str(site), 'id': 'org.kick.keys'})
        check(state['state'] == 'done' and installed() == ['org.kick.types', 'org.kick.content', 'org.kick.keys'],
              'a chain of three packages installs, each after what it needs: ' + json.dumps(state)[:300] + ' ' + json.dumps(installed()))
        check(state['installed']['dependencies'] == [{'id': 'org.kick.types', 'version': '1.0.0'}, {'id': 'org.kick.content', 'version': '1.0.0'}],
              'and its answer lists the packages it installed first: ' + json.dumps(state['installed']))
        before = installed()
        state = job({'source': str(site), 'id': 'org.kick.loop-a'})
        check(state['state'] == 'failed' and 'org.kick.loop-a -> org.kick.loop-b -> org.kick.loop-a' in state['error'] and installed() == before,
              'a cycle is refused, naming it, with nothing installed: ' + state.get('error', ''))
        state = job({'source': str(site), 'id': 'org.kick.needy'})
        check(state['state'] == 'failed' and 'org.kick.absent 1.0.0' in state['error'] and installed() == before,
              'a dependency the repository lacks is named before anything is installed: ' + state.get('error', ''))
        error = rpc('library.uninstall', {'library': 'org.kick.types'}, expect_error=True)
        check('needed by org.kick.content' in error['message'] and 'org.kick.types' in installed(), 'a package another needs is not uninstalled: ' + error['message'])
        error = rpc('library.uninstall', {'library': 'org.kick.content'}, expect_error=True)
        check('needed by org.kick.keys' in error['message'], 'nor one a plugin package needs: ' + error['message'])
        rpc('plugins.remove', {'format': 'clap', 'id': 'org.kick.keys', 'version': '1.0.0'})
        rpc('library.uninstall', {'library': 'org.kick.content'})
        rpc('library.uninstall', {'library': 'org.kick.types'})
        check(installed() == [], 'once what needs them is gone, they uninstall in turn')
    finally:
        p.stdin.close()
        p.wait(timeout=30)
sys.exit(1 if failures else 0)
