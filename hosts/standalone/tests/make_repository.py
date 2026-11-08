# SPDX-License-Identifier: AGPL-3.0-or-later
"""A signed package repository in a folder, for the desktop tests (RFC 0031), with no network.

usage: make_repository.py OUT OSCAIRN-SIGN FIXTURE.clap BUILD-INDEX.py PERFORMANCE.olib

Makes throwaway root, index and publisher keys in OUT, and with the package repository
template's index builder a repository in OUT/site holding: a plugin package with a build for
this platform, one whose only build is for another platform, and the exported Performance given,
renamed into the publisher's namespace and signed by it. OUT/keys.json gives the root and
publisher public keys.
"""
import hashlib
import json
import platform as host
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

out, tool, fixture, builder, performance = (Path(a).resolve() for a in sys.argv[1:6])
machine = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}[host.machine()]
here = ('macos' if sys.platform == 'darwin' else 'windows' if sys.platform == 'win32' else 'linux') + '-' + machine
elsewhere = 'linux-arm64' if here != 'linux-arm64' else 'macos-arm64'


def run(*command):
    done = subprocess.run([str(c) for c in command], capture_output=True, text=True, timeout=300)
    assert done.returncode == 0, (command, done.stderr)
    return done.stdout


out.mkdir(parents=True, exist_ok=True)
keys = {}
for name in ('publisher', 'root', 'index'):
    _, _, public = run(tool, 'key', out / f'{name}.key').split()
    keys[name] = public
repo = out / 'submissions'
shutil.copytree(builder.parents[1], repo)


def record(pid, version, olib, platforms, summary, tags):
    data = olib.read_bytes()
    (olib.parent / f'{version}.json').write_text(json.dumps({
        'format': 'oscairn.repository-release', 'version': 1, 'id': pid, 'release': version, 'publisher': 'org.kick',
        'summary': summary, 'license': 'MIT', 'tags': tags, 'platforms': platforms,
        'archive': {'path': olib.name, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}, 'yanked': None}))


def plugin(pid, name, platform, summary):
    folder = repo / 'packages' / pid
    folder.mkdir(parents=True)
    build = out / 'builds' / pid / platform / 'Synth.clap'
    build.parent.mkdir(parents=True)
    shutil.copyfile(fixture, build)
    run(tool, 'build', out / 'publisher.key', 'clap', pid, '1.0.0', platform, build)
    run(tool, 'plugin-package', '--license', 'MIT', '--key', out / 'publisher.key', folder / '1.0.0.olib', 'clap', pid, '1.0.0', name, f'{platform}={build}')
    record(pid, '1.0.0', folder / '1.0.0.olib', [platform], summary, ['synth'])


plugin('org.kick.test.synth', 'Test Synth', here, 'A test synth')
plugin('org.kick.other', 'Other Synth', elsewhere, 'A synth for another device')
# The exported Performance as a release of the publisher's: its ID in the publisher's namespace,
# without the plugin builds it carries (the publisher did not sign them; the device has its
# own), then signed whole by the publisher.
folder = repo / 'packages' / 'org.kick.test.set'
folder.mkdir(parents=True)
olib = folder / '1.0.0.olib'
with zipfile.ZipFile(performance) as source, zipfile.ZipFile(olib, 'w', zipfile.ZIP_DEFLATED) as target:
    manifest = json.loads(source.read('library.json'))
    builds = [b['path'] for p in manifest.get('plugins', []) for b in p['builds']]
    def build(path):
        return any(path == b or path.startswith(b + '/') for b in builds)
    manifest.update(id='org.kick.test.set', name='Test Set', plugins=[], files={k: v for k, v in manifest.get('files', {}).items() if not build(k)})
    for entry in source.infolist():
        if entry.filename == 'library.sig' or build(entry.filename):
            continue
        target.writestr(entry, json.dumps(manifest).encode() if entry.filename == 'library.json' else source.read(entry))
run(tool, 'archive', out / 'publisher.key', olib)
record('org.kick.test.set', '1.0.0', olib, [], 'A Performance to try', ['set'])

(out / 'index-key.pub').write_text(keys['index'] + '\n')
run(tool, 'file', out / 'root.key', out / 'index-key.pub', out / 'index-key.sig')
config = json.loads((repo / 'repository-config.json').read_text())
config.update(repository='org.example.test', name='Test repository', rootKey=keys['root'],
              indexKey={'key': keys['index'], 'signature': json.loads((out / 'index-key.sig').read_text())[0]['signature']})
(repo / 'repository-config.json').write_text(json.dumps(config))
(repo / 'publishers' / 'org.kick.json').write_text(json.dumps({
    'format': 'oscairn.repository-publisher', 'version': 1, 'id': 'org.kick', 'name': 'KICK', 'key': keys['publisher'],
    'verification': 'https://kick.example/.well-known/oscairn-publisher.json'}))
site = out / 'site'
built = subprocess.run([sys.executable, '-I', builder, '--repo', repo, '--oscairn-sign', tool, '--cache', out / 'cache', '--out', site, '--serial', '1'],
                       capture_output=True, text=True, timeout=300)
assert built.returncode == 0, built.stderr
run(tool, 'file', out / 'index.key', site / 'repository.json', site / 'repository.sig')
for blob in (site / 'blobs.txt').read_text().split():
    (site / 'blobs' / blob[:2]).mkdir(parents=True, exist_ok=True)
    shutil.copyfile(out / 'cache' / blob, site / 'blobs' / blob[:2] / blob)
(out / 'keys.json').write_text(json.dumps({'root': keys['root'], 'publisher': keys['publisher']}))
