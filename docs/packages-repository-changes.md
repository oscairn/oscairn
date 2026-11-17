# Changes the packages repository needs (2026-11-14, 2026-11-16)

For whoever edits the separate packages repository (the one published at
`https://packages.oscairn.org`, made from `tools/package-repository/template/`). Four changes,
each already made and tested in this repository's template, so the simplest way is to bring its
`scripts/build-index.py` over; each is also described here on its own.

## 1. Dependencies between packages of any kind

Archives may now name packages of any kind they need, in `dependencies.packages` beside
`dependencies.assets` ([library archive](library-archive.md#package-dependencies)): a plugin
package may need a types package, a sample package the types package, a Performance pack a sample
package. Devices install them depth first from the repository the install is from.

In `scripts/build-index.py`:

- **The release record.** Where a record's `dependencies` is compared with the archive's, compare it
  with `{"assets": <archive's assets>, "packages": <archive's packages>}`, the `packages` member
  only when the archive names any; a record without `dependencies` stays accepted when the archive
  names none of either. The refusal names the expected value as today.
- **The index.** A version's `requires.packages` lists the archive's asset dependencies followed by
  its package dependencies (each `{id, version}` as the archive gives it).
- **The check.** Beside "every asset library a release needs is an asset package here", add: every
  package a release names in `dependencies.packages` is a package here, of any kind, not yanked, at
  that version or later; otherwise refuse the release with `needs the package <id> <version> or
  later, which this repository does not have`.
- Records in `packages/<id>/<version>.json` of packages that need others carry
  `"dependencies": {"assets": [...], "packages": [{"id": ..., "version": ...}]}`.

This repository's commit `Repository template: dependencies on packages of any kind` has the exact
code; `core/control/tests/package_dependencies.py` builds a repository with it.

## 2. A sample or waveform file is a piece of its own, whatever its size

Today a file under `Assets/` of 1 MiB or more is a `file` piece and smaller files share their top
folder's `zip` piece. Samples and waveforms of the types a handler keeps are now in their type's
folder, `Samples/<id>.osp` and `Waveforms/<id>.owf` (not under `Assets/`), and each must be its own
`file` piece however small, so that packages and versions share them by blob.

In `pieces()`: read the archive's declared item types first (`Types/<owner>/<file>.json`, at most
64 KiB each, JSON objects); every one with a `handler` member names a folder (`folder`). An entry
`<that folder>/<file>` (exactly two path parts) is a `file` piece. RFC 0031's pieces table has the
added row. This repository's commit `Repository pieces: a handler's file is its own piece whatever
its size` has the code and a test in `tools/package-repository/test_build_index.py`.

## 3. The types package's record

`org.oscairn.sample-types`, version `1.0.0`: a library package (kind `library`) holding only
`Types/org.oscairn.sample-types/sample.json` and `waveform.json`, copied from
`core/control/types/org.oscairn.sample-types/` in this repository, no items and no code. Pack and
sign it with:

```sh
mkdir -p tree/Types/org.oscairn.sample-types
cp core/control/types/org.oscairn.sample-types/*.json tree/Types/org.oscairn.sample-types/
oscairn-sign library-package --key oscairn.key --license MIT 1.0.0.olib org.oscairn.sample-types "Sample types" tree/
```

and give it the record (sizes and hash from the archive):

```json
{"format": "oscairn.repository-release", "version": 1, "id": "org.oscairn.sample-types", "release": "1.0.0",
 "publisher": "org.oscairn", "summary": "The sample and waveform item types, for samplers and sample content",
 "license": "MIT", "tags": ["types"], "platforms": [],
 "archive": {"path": "1.0.0.olib", "sha256": "<SHA-256 of the archive>", "size": <bytes>}, "yanked": null}
```

The Sampler's plugin package then names it (`oscairn-sign plugin-package --package-dependency
org.oscairn.sample-types 1.0.0 ...`), as does every sample content package (`oscairn-sign
library-package --type core/control/types/org.oscairn.sample-types/sample.json --package-dependency
org.oscairn.sample-types 1.0.0 ...`), and its record lists it in `dependencies.packages`. Once it is
published, add `org.oscairn.sample-types 1.0.0` to this repository's `tools/preinstalled.txt` so a
fresh device can take sample content.

## 4. Each version's plugins, with the publisher's claim (2026-11-16)

A plugin package may now carry its publisher's claim that a version opens older versions' saved
state ([RFC 0043](rfcs/0043-moving-sounds-to-newer-plugin-versions.md#the-publishers-claim),
[library archive](library-archive.md#the-publishers-claim)): `opensStateFrom` and `soundChanges`
on a plugin in the manifest's `plugins`, written by `oscairn-sign plugin-package --opens-state-from
<id> <version> --sound-changes <id> <text>`. `oscairn-sign verify` refuses a claim out of shape, so
an `oscairn-sign` built from this repository must check submissions from then on.

In `scripts/build-index.py`, each version in the index gains `plugins`, from the manifest
([RFC 0031](rfcs/0031-package-repository-and-distribution.md#the-index-must-say-which-plugins-a-version-holds)):

```json
"plugins": [{"format": "clap", "id": "org.oscairn.analog", "version": "0.4.6", "platforms": ["linux-arm64", "macos-arm64"], "opensStateFrom": "0.4.0"}]
```

each plugin's format, ID, version and the platforms it has builds for (sorted), and
`opensStateFrom` only when the manifest has one; never `soundChanges`, which a device reads from
the installed manifest. The template's `plugins_of()` is the exact code;
`tools/package-repository/test_build_index.py` checks it. Devices do not read it yet (RFC 0031's
step 11), so the index grows by about 100 bytes per plugin per version and nothing else changes.

oscairn/packages' import workflow: `schemas/plugin-release.schema.json`'s plugins gain the two
optional fields (`opensStateFrom`, a dotted version; `soundChanges`, text of at most 1,000 bytes),
which its `additionalProperties: false` refuses now, and `scripts/import-release.py` passes them to
`oscairn-sign plugin-package` as `--opens-state-from` and `--sound-changes`.
