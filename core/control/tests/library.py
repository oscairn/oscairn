# SPDX-License-Identifier: AGPL-3.0-or-later
"""The library through RPC (RFC 0021): info, save and load by ID, query, favorites, recents, the cache."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


try:
    import jsonschema
    protocol = json.loads((Path(__file__).resolve().parents[3] / 'docs/control-protocol.schema.json').read_text())
    # Built once: compiling the schema for every request made the test take over a minute.
    request_validator = jsonschema.Draft202012Validator({**protocol, '$ref': '#/$defs/Request'})
except ImportError:
    jsonschema = None
    print('JSON Schema validator not installed; requests are not checked against the schema')


def rpc(method, params=None, expect_error=False):
    request = {'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params or {}}
    if jsonschema and not expect_error:
        request_validator.validate(request)
    p.stdin.write(json.dumps(request) + '\n')
    p.stdin.flush()
    response = json.loads(p.stdout.readline())
    assert ('error' in response) == expect_error, response
    return response.get('result')


with tempfile.TemporaryDirectory(prefix='oscairn-library-') as temporary:
    home = Path(temporary)
    p = subprocess.Popen([sys.argv[1], sys.argv[2], '--stdio'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         text=True, encoding='utf-8', env={**os.environ, 'OSCAIRN_DATA_HOME': temporary})
    try:
        # The app's start (RFC 0028): nothing to reopen yet, and from now on what opens or saves is remembered.
        assert rpc('performance.reopen') == {'notice': ''}
        last = lambda: json.loads((home / 'library/last.json').read_text())['performance']
        assert rpc('library.query') == {'items': [], 'tags': [], 'total': 0, 'tagColors': [], 'hiddenTags': []}
        assert rpc('performance.info') == {'id': '', 'name': '', 'description': '', 'tags': [], 'color': '', 'unsaved': False}
        info = rpc('performance.setInfo', {'name': 'Warm Pad', 'description': 'slow strings', 'tags': ['pad', 'warm'], 'color': '#3a7bd5'})
        assert info['unsaved'] is True  # info a save would write
        pad = rpc('library.save')['id']
        assert len(pad) == 16 and rpc('performance.info')['id'] == pad and rpc('performance.info')['unsaved'] is False
        assert (home / 'Performances/Warm Pad.oscairn/performance.json').is_file()
        assert last() == str((home / 'Performances/Warm Pad.oscairn').resolve())
        # Saving again keeps the ID and the folder.
        assert rpc('library.save')['id'] == pad and len(list((home / 'Performances').iterdir())) == 1
        # Save with a name renames it in place: the same ID and folder.
        assert rpc('library.save', {'name': 'Warm Pad'})['id'] == pad
        # Save As New is a copy with its own ID and folder.
        copy = rpc('library.save', {'name': 'Warm Pad 2', 'new': True})['id']
        assert copy != pad and rpc('library.query')['total'] == 2
        assert (home / 'Performances/Warm Pad 2.oscairn/performance.json').is_file()
        assert last().endswith('Warm Pad 2.oscairn')
        rpc('library.load', {'id': pad})
        assert rpc('performance.info')['id'] == pad and last().endswith('Warm Pad.oscairn')
        assert rpc('performance.reopen') == {'notice': ''} and rpc('performance.info')['id'] == pad  # asked again, it keeps what is open
        manifest = home / 'Performances/Warm Pad 2.oscairn'
        for f in sorted(manifest.rglob('*'), reverse=True):
            f.unlink() if f.is_file() else f.rmdir()
        manifest.rmdir()
        assert rpc('library.rescan') == {'items': 1}
        # Info set but not saved is gone when the saved one is opened again.
        rpc('performance.setInfo', {'name': 'Bright Lead', 'description': '', 'tags': ['lead']})
        assert rpc('performance.info')['unsaved'] is True
        rpc('performance.load', {'directory': str(home / 'Performances/Warm Pad.oscairn')})
        assert rpc('performance.info')['name'] == 'Warm Pad' and rpc('performance.info')['unsaved'] is False

        everything = rpc('library.query')
        assert everything['total'] == 1 and everything['items'][0]['name'] == 'Warm Pad' and everything['items'][0]['color'] == '#3a7bd5'
        assert everything['tags'] == [{'tag': 'pad', 'count': 1}, {'tag': 'warm', 'count': 1}]
        assert rpc('library.query', {'text': 'STRINGS'})['total'] == 1
        assert rpc('library.query', {'tags': ['pad', 'lead']})['total'] == 0
        assert rpc('library.query', {'favorites': True})['total'] == 0
        rpc('library.favorite', {'id': pad, 'favorite': True})
        assert rpc('library.query', {'favorites': True})['items'][0]['favorite'] is True
        assert rpc('library.query', {'recent': True})['items'][0]['recent'] == 1
        # Tags the user hides leave the tag list but still match a search typed in full; tag colors
        # are the user's too, kept in the user file beside favorites.
        rpc('library.tag', {'tag': 'warm', 'hidden': True, 'color': '#e8a33d'})
        hidden = rpc('library.query')
        assert hidden['tags'] == [{'tag': 'pad', 'count': 1}] and hidden['hiddenTags'] == ['warm']
        assert hidden['tagColors'] == [{'tag': 'warm', 'color': '#e8a33d'}]
        assert rpc('library.query', {'text': 'warm'})['total'] == 1 and rpc('library.query', {'tags': ['warm']})['total'] == 1
        user = json.loads((home / 'library/user.json').read_text())
        assert user['hiddenTags'] == ['warm'] and user['tagColors'] == {'warm': '#e8a33d'}
        rpc('library.tag', {'tag': 'warm'})
        assert rpc('library.query')['tags'] == [{'tag': 'pad', 'count': 1}, {'tag': 'warm', 'count': 1}] and rpc('library.query')['tagColors'] == []
        rpc('library.tag', {'tag': 'warm', 'color': 'orange'}, True)
        rpc('library.tag', {'tag': ''}, True)
        rpc('library.favorite', {'id': 'missing', 'favorite': True}, True)
        rpc('library.load', {'id': 'missing'}, True)
        rpc('library.query', {'limit': 0}, True)
        rpc('performance.setInfo', {'name': 'x', 'description': '', 'tags': ['']}, True)
        rpc('performance.setInfo', {'name': 'x', 'description': '', 'tags': [], 'color': 'blue'}, True)

        # A file changed on disk is read again; one the library cannot read is skipped, and the cache lives.
        manifest = home / 'Performances/Warm Pad.oscairn/performance.json'
        value = json.loads(manifest.read_text())
        value['info']['tags'] = ['pad', 'dark']
        manifest.write_text(json.dumps(value, separators=(',', ':')))  # as Oscairn writes it
        (home / 'Performances/Broken.oscairn').mkdir()
        (home / 'Performances/Broken.oscairn/performance.json').write_text('{')
        assert rpc('library.rescan') == {'items': 1}
        assert rpc('library.query', {'tags': ['dark']})['total'] == 1
        assert json.loads((home / 'library/index.json').read_text())['items'][0]['tags'] == ['pad', 'dark']
        rpc('library.load', {'id': pad})
        assert rpc('performance.info')['tags'] == ['pad', 'dark']
        # An item's info edited in its file (and the open Performance's, being that item); a copy of
        # the user's own; the user's own items by source.
        rpc('library.edit', {'id': pad, 'name': 'Warm Pad', 'description': 'edited', 'tags': ['pad', 'dark'], 'color': '#e8a33d'})
        assert json.loads(manifest.read_text())['info']['description'] == 'edited' and rpc('performance.info')['description'] == 'edited'
        duplicate = rpc('library.copy', {'id': pad})['id']
        twin = next(i for i in rpc('library.query')['items'] if i['id'] == duplicate)
        assert twin['name'] == 'Warm Pad copy' and twin['library'] == '' and twin['tags'] == ['pad', 'dark'] and twin['color'] == '#e8a33d'
        assert rpc('library.query', {'source': 'mine'})['total'] == 2 and rpc('library.query', {'source': 'nonesuch'})['total'] == 0
        assert [i['id'] for i in rpc('library.query', {'id': duplicate, 'limit': 1})['items']] == [duplicate] and rpc('library.query', {'id': 'missing'})['total'] == 0
        rpc('library.query', {'id': ''}, True)
        rpc('library.delete', {'id': duplicate})
        rpc('library.edit', {'id': 'missing', 'name': '', 'description': '', 'tags': []}, True)
        rpc('library.edit', {'id': pad, 'name': '', 'description': '', 'tags': ['']}, True)
        rpc('library.copy', {'id': 'missing'}, True)
        # Audition (RFC 0021): a Performance tried in place of this one, then put back.
        assert rpc('audition.get') == {'id': '', 'kind': '', 'part': 0}
        rpc('audition.start', {'id': pad})
        assert rpc('audition.get') == {'id': pad, 'kind': 'performance', 'part': 0} and rpc('performance.info')['name'] == 'Warm Pad'
        rpc('audition.stop')
        assert rpc('audition.get')['id'] == '' and rpc('performance.info')['id'] == pad
        rpc('audition.start', {'id': 'missing'}, True)
        # A part saved as a Sound (RFC 0021), listed apart, loaded into a part and as a new part.
        part = rpc('parts.add', {'name': 'Lead'})['part']
        rpc('parts.insert', {'part': part, 'index': 0, 'node': 'synth'})
        rpc('graph.commit')
        tone = rpc('controls.add', {'name': 'Tone', 'part': part})['control']
        sound = rpc('parts.saveSound', {'part': part, 'name': 'Lead Sound'})
        # The harness wires its synth to the input and output by hand: those wires cross the part's edge.
        assert sound['left'] == ['the input to synth', 'synth to the output'] and (home / 'Sounds/Lead Sound.osound/sound.json').is_file()
        assert [i['id'] for i in rpc('library.query', {'kind': 'sound'})['items']] == [sound['id']]
        assert rpc('library.query', {'kind': 'performance'})['total'] == 1
        # By type (RFC 0023): one ID or a list; each item says its type; a type not known is invalid.
        assert [i['id'] for i in rpc('library.query', {'type': 'oscairn.sound'})['items']] == [sound['id']]
        assert rpc('library.query', {'type': ['oscairn.sound', 'oscairn.performance']})['total'] == 2
        assert rpc('library.query', {'type': 'oscairn.sound'})['items'][0]['type'] == 'oscairn.sound'
        rpc('library.query', {'type': 'org.example.none'}, expect_error=True)
        rpc('library.query', {'kind': 'oscairn.sound'}, expect_error=True)
        # Setlists (docs/setlist-format.md): the whole document, saved and read through the library.
        performance = rpc('library.query', {'kind': 'performance'})['items'][0]
        setlist = {'format': 'oscairn.setlist', 'version': 2,
                   'info': {'id': '', 'name': 'Friday', 'description': '', 'tags': ['live'], 'color': '', 'revision': '', 'parent': '', 'hash': '', 'origin': None},
                   'display': {'mode': 'grid', 'columns': 4, 'rows': 3},
                   'pages': [{'name': 'Set 1', 'slots': [{'id': performance['id'], 'name': performance['name'], 'color': '#e5484d', 'label': 'Opener'}, None]},
                             {'name': 'Encore', 'slots': [{'id': '', 'name': 'Piano Ballad', 'color': '', 'label': ''}]}]}
        friday = rpc('setlists.save', {'setlist': setlist})['id']
        got = rpc('setlists.get', {'id': friday})['setlist']
        assert got['info']['id'] == friday and got['info']['tags'] == ['live'] and got['pages'] == setlist['pages'] and got['display'] == setlist['display'], got
        assert len(got['info']['revision']) == 16 and len(got['info']['hash']) == 64
        listed = rpc('library.query', {'type': 'oscairn.setlist'})['items']
        assert [i['id'] for i in listed] == [friday] and listed[0]['kind'] == 'setlist' and listed[0]['type'] == 'oscairn.setlist'
        got['pages'][0]['name'] = 'First set'
        assert rpc('setlists.save', {'setlist': got})['id'] == friday and rpc('setlists.get', {'id': friday})['setlist']['pages'][0]['name'] == 'First set'
        got['pages'][0]['slots'][0]['color'] = 'red'
        rpc('setlists.save', {'setlist': got}, expect_error=True)
        rpc('setlists.get', {'id': performance['id']}, expect_error=True)
        rpc('library.delete', {'id': friday})
        assert rpc('library.query', {'type': 'oscairn.setlist'})['total'] == 0
        added = rpc('parts.fromSound', {'id': sound['id']})['part']
        loaded = rpc('parts.loadSound', {'part': part, 'id': sound['id']})
        assert loaded == {'accepted': True, 'flagged': ['the wire from the input to synth was removed', 'the wire from synth to the output was removed']}, loaded
        graph = rpc('graph.get')
        assert [p['name'] for p in graph['parts'] if p['id'] in (part, added)] == ['Lead Sound', 'Lead Sound']
        source = next(p for p in graph['parts'] if p['id'] == part)['from']
        assert source == {'sound': sound['id'], 'revision': sound['revision'], 'name': 'Lead Sound', 'modified': False}, source
        rpc('graph.commit')
        forked = rpc('parts.saveSound', {'part': part, 'name': 'Lead Fork', 'new': True})
        assert forked['id'] != sound['id']
        assert next(p for p in rpc('graph.get')['parts'] if p['id'] == part)['from']['sound'] == forked['id']
        owners = sorted(c['part'] for c in graph['controls'] if c['name'] == 'Tone')
        assert owners == sorted([part, added]) and tone not in [c['id'] for c in graph['controls']]
        rpc('parts.loadSound', {'part': part, 'id': pad}, True)
        rpc('library.query', {'kind': 'preset'}, True)
        # Importing a Performance (RFC 0021): what it holds, then one chosen part.
        exported = home / 'export.oscairn'
        rpc('performance.save', {'directory': str(exported)})
        assert last() == str(exported.resolve())
        contents = rpc('performance.contents', {'directory': str(exported)})
        held = contents['parts']
        assert contents['scenes'] == []
        assert [p['name'] for p in held] == [p['name'] for p in rpc('graph.get')['parts']] and not any(p['bus'] for p in held)
        count = len(rpc('graph.get')['parts'])
        imported = rpc('performance.import', {'directory': str(exported), 'parts': [held[0]['id']]})
        assert len(imported['parts']) == 1 and len(rpc('graph.get')['parts']) == count + 1, imported
        rpc('performance.import', {'directory': str(exported), 'parts': [9999]}, True)
        user = json.loads((home / 'library/user.json').read_text())
        assert user['favorites'] == [pad] and user['recent'][-2:] == [pad, copy]  # a gone item's stays, for if it returns
        # The trust store (RFC 0006): keys added, changed, revoked and removed, kept in trust.json;
        # the official repository's root key built in, for repositories only, and its publisher's
        # key for plugins, apps and content (RFC 0031).
        publisher = {'id': 'b284be6bea2fadb1', 'key': 'db87e97dba5b24564c782f473c543681bd87c0818bc2e41f37d1f100f3360d69', 'name': 'Oscairn', 'scope': ['apps', 'content', 'plugins'], 'revoked': False}
        root = {'id': '6a2dd1acf4d4e23c', 'key': '6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841', 'name': 'Oscairn', 'scope': ['repositories'], 'revoked': False}
        official = [publisher, root]
        assert rpc('trust.list') == {'keys': official}
        assert not (home / 'trust.json').exists()
        vendor = 'd75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a'
        fingerprint = hashlib.sha256(bytes.fromhex(vendor)).hexdigest()[:16]
        rpc('trust.set', {'key': vendor, 'name': 'Vendor', 'scope': ['plugins']})
        rpc('trust.set', {'key': vendor, 'name': 'Vendor Inc', 'scope': ['plugins', 'apps'], 'revoked': True})
        assert rpc('trust.list') == {'keys': [{'id': fingerprint, 'key': vendor, 'name': 'Vendor Inc', 'scope': ['apps', 'plugins'], 'revoked': True}, *official]}
        assert json.loads((home / 'trust.json').read_text())['keys'][0]['name'] == 'Vendor Inc'
        rpc('trust.set', {'key': 'ab', 'name': 'x', 'scope': []}, True)
        rpc('trust.set', {'key': vendor, 'name': 'x', 'scope': ['everything']}, True)
        rpc('trust.remove', {'id': fingerprint})
        assert rpc('trust.list') == {'keys': official}
        # The built-in publisher key, removed, stays off: trust.json keeps it trusted for nothing.
        rpc('trust.remove', {'id': publisher['id']})
        assert rpc('trust.list') == {'keys': [{**publisher, 'scope': []}, root]}
        assert json.loads((home / 'trust.json').read_text())['keys'] == [{'key': publisher['key'], 'name': 'Oscairn', 'scope': [], 'revoked': False}]
        rpc('trust.set', {'key': publisher['key'], 'name': 'Oscairn', 'scope': ['apps', 'content', 'plugins']})
        rpc('trust.remove', {'id': fingerprint}, True)
        # Signing keys in the data folder, and an export signed with one; the private key is never sent.
        made = rpc('keys.make', {'name': 'mine'})
        assert made['name'] == 'mine' and len(made['key']) == 64 and made['id'] == hashlib.sha256(bytes.fromhex(made['key'])).hexdigest()[:16]
        assert rpc('keys.list') == {'keys': [made]}
        assert (home / 'Keys/mine.key').stat().st_mode & 0o777 == 0o600
        rpc('keys.make', {'name': 'mine'}, True)
        rpc('keys.make', {'name': '../escape'}, True)
        assert rpc('library.export', {'id': pad, 'file': str(home / 'signed.olib'), 'sign': {'key': 'mine', 'items': True, 'archive': True}})['exported']
        rpc('library.export', {'id': pad, 'file': str(home / 'unsigned.olib'), 'sign': {'key': 'nobody', 'archive': True}}, True)
        assert not (home / 'unsigned.olib').exists()
    finally:
        p.stdin.close()
        p.wait(timeout=10)
    assert p.returncode == 0
print('ok: library info, save and load by ID, query, favorites, recents, rescan, Sounds and importing')
