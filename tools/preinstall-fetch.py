# SPDX-License-Identifier: AGPL-3.0-or-later
"""Fetches the packages a release ships (RFC 0031, "Preinstalled packages") and lays them out as
a repository snapshot: the official repository's signed root, index key and index exactly as
published, and as blobs only the listed packages' pieces for --platform (each one's pieces list,
what every platform shares and that platform's builds; RFC 0031, "Pieces: the format"). The engine
installs from it on first run through the normal installer, which checks every signature and hash
again.

usage: python3 -I tools/preinstall-fetch.py --oscairn-sign PATH --out DIR --platform macos-arm64
           [--list tools/preinstalled.txt] [--licenses DIR] [--repository URL --root-key HEX]

Everything is checked here first, and any failure stops it with a nonzero exit: the root is the
official root key's (the index key it signed, and the index key signed the root, both checked with
`oscairn-sign verify-file`); the index is the one the root names by hash; each listed version is in
the index, stable and not yanked, with a build for --platform when it brings code; each piece and
pieces list is the size and SHA-256 the index's chain gives, the manifest the one the index names,
and `oscairn-sign verify --platform` passes on the archive the pieces make for that platform, with
the archive and every build signed by the publisher key the index gives; each asset library a
listed package needs is listed at the version a device would pick. --licenses writes each package's
license texts, license and source there, one folder per package. --repository and --root-key read
another repository (a file:// address for a folder), for testing.
"""
import argparse
import gzip
import tempfile
import hashlib
import json
import subprocess
import sys
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

OFFICIAL = 'https://packages.oscairn.org'
OFFICIAL_KEY = '6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841'  # official in core/control/src/packages.cpp
REPOSITORY, ROOT_KEY = OFFICIAL, OFFICIAL_KEY


def fail(why):
    sys.exit('preinstall-fetch: ' + why)


def fetch(url, limit):
    request = urllib.request.Request(url, headers={'User-Agent': 'oscairn-preinstall-fetch'})
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        fail(f'{url} is larger than {limit} bytes')
    return data


def sign(tool, *args):
    out = subprocess.run([str(tool), *map(str, args)], capture_output=True, text=True, timeout=600)
    if out.returncode:
        fail(f'oscairn-sign {args[0]} {args[-1]}: {out.stderr.strip()}')
    return out.stdout


def older(a, b):
    """Dotted versions compared part by part, numbers as numbers, as the device does."""
    def parts(v):
        return [(0, int(p), '') if p.isdigit() else (1, 0, p) for p in v.split('.')]
    return parts(a) < parts(b)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--oscairn-sign', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--list', type=Path, default=Path(__file__).resolve().parent / 'preinstalled.txt')
    parser.add_argument('--platform', required=True)
    parser.add_argument('--licenses', type=Path)
    parser.add_argument('--repository', default=OFFICIAL)
    parser.add_argument('--root-key', default=OFFICIAL_KEY)
    a = parser.parse_args()
    global REPOSITORY, ROOT_KEY
    REPOSITORY, ROOT_KEY = a.repository.rstrip('/'), a.root_key
    if a.out.exists():
        fail(f'{a.out} exists; give a folder that does not')
    wanted = []
    for line in a.list.read_text().splitlines():
        line = line.split('#')[0].split()
        if line:
            if len(line) != 2:
                fail(f'{a.list}: each line is a package ID and a version')
            wanted.append(tuple(line))
    a.out.mkdir(parents=True)

    def keep(path, data):
        file = a.out / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(data)
        return file

    # The root, signed by the index key, which the official root key signed.
    root_bytes = fetch(REPOSITORY + '/repository.json', 1 << 20)
    keep('repository.json', root_bytes)
    keep('repository.sig', fetch(REPOSITORY + '/repository.sig', 64 << 10))
    root = json.loads(root_bytes)
    if root.get('format') != 'oscairn.repository' or root.get('version') != 1:
        fail('repository.json is not format oscairn.repository, version 1')
    if root['rootKey'] != ROOT_KEY:
        fail(f'the root names root key {root["rootKey"]}, not the official one')
    index_key = root['indexKey']['key']
    work = a.out.parent / (a.out.name + '.check')
    work.mkdir()
    try:
        (work / 'index-key').write_text(index_key + '\n')
        (work / 'index-key.sig').write_text(json.dumps([{'key': ROOT_KEY, 'signature': root['indexKey']['signature']}]))
        if sign(a.oscairn_sign, 'verify-file', work / 'index-key', work / 'index-key.sig').split()[2:] != [ROOT_KEY]:
            fail('the index key is not signed by the official root key')
        signers = [line.split()[2] for line in sign(a.oscairn_sign, 'verify-file', a.out / 'repository.json', a.out / 'repository.sig').splitlines()]
        if index_key not in signers:
            fail('the root is not signed by its index key')
    finally:
        for f in work.iterdir():
            f.unlink()
        work.rmdir()

    # Every file the root names, by its hash and size: the index, revocations and deltas.
    def named(entry):
        data = fetch(REPOSITORY + '/' + entry['path'], entry['size'])
        if len(data) != entry['size'] or hashlib.sha256(data).hexdigest() != entry['sha256']:
            fail(f'{entry["path"]} is not the one the root names')
        keep(entry['path'], data)
        return data
    index = json.loads(gzip.decompress(named(root['index'])))
    for entry in [root[k] for k in ('revocations',) if root.get(k)] + root.get('deltas', []):
        named(entry)
    if index.get('repository') != root['repository'] or index.get('serial') != root['serial']:
        fail('the index is not the root\'s')
    packages = {p['id']: p for p in index['packages']}
    listed = dict(wanted)

    def newest(package, least):
        best = None
        for v in package['versions']:
            if not v.get('yanked') and v.get('channel', 'stable') in ('', 'stable') and not older(v['version'], least) and (not best or older(best['version'], v['version'])):
                best = v
        return best

    for package_id, version in wanted:
        package = packages.get(package_id)
        if not package:
            fail(f'no package {package_id} in the repository')
        release = next((v for v in package['versions'] if v['version'] == version), None)
        if not release:
            fail(f'no version {version} of {package_id} in the repository')
        what = f'{package_id} {version}'
        if release.get('yanked') or release.get('channel', 'stable') not in ('', 'stable'):
            fail(f'{what} is yanked or not stable')
        code = any(release.get('types', {}).get(t) for t in ('clap', 'lv2'))
        if code and a.platform not in release.get('platforms', {}):
            fail(f'{what} has no build for {a.platform}')
        for need in release.get('requires', {}).get('packages', []):
            pick = newest(packages.get(need['id'], {'versions': []}), need['version'])
            if not pick or listed.get(need['id']) != pick['version']:
                fail(f'{what} needs {need["id"]} {need["version"]} or later; list it at {pick["version"] if pick else "(none in the repository)"}')

        # Its pieces: the pieces list, then what every platform shares and this platform's builds,
        # each from the blob base, else the repository's own blobs/, by the size and SHA-256 named.
        def blob(sha, size):
            path = f'blobs/{sha[:2]}/{sha}'
            data = None
            for url in ([root['blobBase'] + f'{sha[:2]}/{sha}'] if root.get('blobBase') else []) + [REPOSITORY + '/' + path]:
                try:
                    data = fetch(url, size)
                    break
                except OSError as e:
                    print(f'preinstall-fetch: {url}: {e}; trying the next place', file=sys.stderr)
            if data is None or len(data) != size or hashlib.sha256(data).hexdigest() != sha:
                fail(f'a piece of {what} is missing or not the one the index names: {path}')
            return keep(path, data)
        pieces_list = json.loads(blob(release['pieces']['sha256'], release['pieces']['size']).read_bytes())
        if pieces_list.get('format') != 'oscairn.repository-pieces' or pieces_list.get('version') != 1:
            fail(f'the pieces list of {what} is not format oscairn.repository-pieces, version 1')
        pieces = [p for p in pieces_list['pieces'] if p.get('platform') in (None, a.platform)]
        files = {p['sha256']: blob(p['sha256'], p['size']) for p in pieces}
        size = sum(p['size'] for p in pieces)
        with tempfile.TemporaryDirectory() as work:
            # The archive these pieces make, as a ready-made archive for the platform is: what is checked.
            archive = Path(work) / 'release.olib'
            with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as out:
                for p in pieces:
                    if p['type'] == 'file':
                        out.write(files[p['sha256']], p['path'])
                    else:
                        with zipfile.ZipFile(files[p['sha256']]) as z:
                            for name in z.namelist():
                                out.writestr(name, z.read(name))
            with zipfile.ZipFile(archive) as z:
                if hashlib.sha256(z.read('library.json')).hexdigest() != release['manifest']['sha256']:
                    fail(f'the pieces of {what} are not the release the index names')
            publisher = package['publisher']['key']
            lines = [l.split() for l in sign(a.oscairn_sign, 'verify', '--platform', a.platform, archive).splitlines()]
            signed = [l for l in lines if l[0] == 'archive']
            builds = [l for l in lines if l[0] == 'build']
            if signed != [['archive', package_id, publisher]] or any(l[-1] != publisher for l in builds):
                fail(f'{what} is not signed throughout by its publisher\'s key {publisher}: ' + '; '.join(' '.join(l) for l in signed + builds))
            print(f'{what}: {size} bytes in {len(pieces)} pieces for {a.platform}, signed by {publisher}')

            if a.licenses:
                folder = a.licenses / package_id
                folder.mkdir(parents=True)
                with zipfile.ZipFile(archive) as z:
                    manifest = json.loads(z.read('library.json'))
                    for name in z.namelist():
                        p = PurePosixPath(name)
                        if len(p.parts) == 2 and p.parts[0] == 'Licenses' and p.parts[1] not in ('.', '..'):
                            (folder / p.parts[1]).write_bytes(z.read(name))
                source = manifest.get('source') or {}
                (folder / 'PACKAGE.txt').write_text(
                    f'{package["name"]} ({package_id}) {version}, published by {package["publisher"]["name"]} at {REPOSITORY}\n'
                    f'License: {manifest.get("license") or package.get("license", "")}\n'
                    + (f'Source: {source["url"]} at {source["revision"]}\n' if source.get('url') else 'Source: not named in the package\n'))
    print(f'{len(wanted)} packages from {root["name"]} serial {root["serial"]} in {a.out}')


if __name__ == '__main__':
    main()
