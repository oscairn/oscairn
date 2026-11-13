# Library archive format, version 3

A library, an exported Performance or Sound, or a plugin package is one archive file
([RFC 0021](rfcs/0021-sounds-performances-and-library.md#libraries)): a zip holding

| Entry | What it is |
| --- | --- |
| `library.json` | The manifest (below) |
| `library.sig` | Optional: the whole archive's signature, `{key, signature}` over `oscairn-archive-1\0` and the manifest's exact bytes ([signatures](#signatures)) |
| `Performances/<folder>/` | Each Performance as the library keeps it: `performance.json` ([format](performance-format.md)) and `nodes/` |
| `Sounds/<folder>/` | Each Sound the same way, `sound.json` and `nodes/` |
| `Assets/<first 16 hex digits of the SHA-256>/<file>` | Each sample the items' plugins use, once; in an [asset package](#asset-packages), `Assets/<path>`, the library's files at their paths |
| `Plugins/<format>/<id>@<version>/<platform>/<bundle>` | Each plugin build the items use ([RFC 0006](rfcs/0006-packages-and-portability.md)), a file or a bundle folder as installed; characters other than letters, digits, `.`, `-`, `@` and `_` in the ID and version are `_` |
| `Licenses/<file>` | Its license texts and notices ([license and source](#license-and-source)) |

The manifest:

| Field | Meaning |
| --- | --- |
| `format`, `version` | Exactly `oscairn.library` and integer `3` |
| `id` | The library's ID: its folder's name when installed as a library, and what imported copies record as their `origin`; 1 to 64 letters, digits, `.`, `-` and `_`, the first one a letter or digit and the last not a `.` |
| `name` | Its name |
| `kind` | `library`, `performance` for an exported Performance, `sound` for an exported Sound (its ID and name are that item's), `plugin` for a [plugin package](#plugin-packages): plugin builds and no items, or `asset` for an [asset package](#asset-packages). Any other kind is refused |
| `assets` | An asset package's library: its `version` and `files` ([asset packages](#asset-packages)); `null` for every other kind |
| `dependencies` | `{"assets": [...]}`: the asset libraries a plugin package needs, each `{id, version}`, the least version; empty for every other kind. `packages`, beside it: the packages a library, Performance or Sound needs ([below](#package-dependencies)). Not built: no reader takes it yet |
| `license` | Its license, an SPDX expression (at most 256 bytes); empty when it names none. A plugin package names one |
| `source` | `null`, or where its source is: `{url, revision}`, an `https://` address and the exact commit or tag (at most 2048 and 128 bytes, no spaces or control characters) |
| `items` | Each item's `kind` (`performance` or `sound`), `id` (as plain as the library's: an exported item's ID is its archive's), `name`, `path`, its folder in the archive, and `signatures` (optional) |
| `plugins` | Each plugin's `format`, `id`, `version`, `name` and `builds`: each build's `platform` (such as `macos-arm64` or `linux-arm64`), `path` in the archive, `sha256`, its digest as a [Performance's lock](performance-format.md) gives it, `signatures` (optional) and `estimate` (optional, [RFC 0025](rfcs/0025-resource-budgets.md#status): `{cpu,memory}`, its busiest block's share of the block's time, 0 to 64, and the bytes it holds, at most 1 TiB, as `oscairn-plugin-check` measures it on that platform; installing keeps it beside the build as `estimate.json`, and a device that has not measured the plugin counts it at this) |
| `files` | Every other entry's SHA-256, by its path |
| `pluginRepositories` | Optional: where the plugins its items use are published ([below](#where-plugins-are-published)). Not built: no reader takes it yet |

An export holds the Performance, the library Sound of each part that is still that Sound (the
part unedited since it was loaded or saved, and the library holding that revision), and with
**edited Sounds**, the Sound of each part edited since, as the library has it. Every sample their
plugins list goes in once, by its hash, except a file of an installed [asset library](#asset-packages)
that may not be passed on: that one stays out, the export's notes say how many files of which
library, and the destination needs the library installed. The license texts of each asset
library whose files go in go with them, under `Licenses/<library ID>/`. Each plugin they use goes in with every build there is:
this device's from its `pathHint` while that build still has the document's locked digest, else
from the plugin store, and the store's builds for other platforms (installed from archives, or
added with `plugins.addBuild`). Only CLAP plugins go in: LV2 plugins are installed on each device (many need libraries
outside their bundles), so an LV2 plugin, like one not on this device, is left out, and
the export says which.

## Plugin packages

A plugin package carries plugins without Sounds or Performances: `kind` is `plugin`, `items`
is empty and `plugins` lists at least one plugin, each with its builds as in any archive, under
the same signature rules. Its ID and name are the package's, given to `oscairn-sign
plugin-package`:

```sh
oscairn-sign plugin-package --key author.key --license MIT Organ.olib clap com.example.organ 2.1.0 "Example Organ" \
    macos-arm64=build/mac/Organ.clap linux-arm64=build/linux/Organ.clap
```

It packs each build with the signatures in `<build>.sig` beside it, refusing one that is not
over that build, and with `--key` signs the whole archive. Installing one puts every build of
each of its plugins into the [plugin store](#the-plugin-store), whether or not anything uses
them yet, and adds no items: there is no choice of a locked library or the user's own storage.

**A bundle holding several plugins** (Oscairn's Splitters: Split and Join in
`oscairn-splitters.clap`) goes in once, at its first plugin's path, and every plugin it holds
is listed in `plugins` with a build at that same path, each with its own signatures. A
manifest listing one plugin (the same format, ID and version) twice is refused. Each plugin is
named with `--plugin <id> <name>`, and the bundle is signed for all of them at once
(`oscairn-sign build` takes the IDs comma-separated, and replaces that key's earlier signatures
in the `.sig`, so signing them one at a time would keep only the last):

```sh
oscairn-sign build author.key clap org.oscairn.split,org.oscairn.join 0.1.0 macos-arm64 oscairn-splitters.clap
oscairn-sign plugin-package --key author.key --license AGPL-3.0-or-later --license-file LICENSE \
    --source https://github.com/oscairn/oscairn v0.1.0 --plugin org.oscairn.split "Oscairn Split" \
    --plugin org.oscairn.join "Oscairn Join" Splitters.olib clap org.oscairn.splitters 0.1.0 \
    "Oscairn Splitters" macos-arm64=oscairn-splitters.clap
```

Each plugin keeps its own version, the package's unless named as `<id>@<version>`, to both
`build` and `--plugin` (setBfree's organ is 0.4.4 and its Leslie 0.2.1); the bundle still goes
in once, at its first plugin's path, and each plugin installs into its own store folder:

```sh
oscairn-sign build author.key clap org.oscairn.setbfree,org.oscairn.setbfree.leslie@0.2.1 0.4.4 macos-arm64 setBfree.clap
oscairn-sign plugin-package --key author.key ... --plugin org.oscairn.setbfree setBfree \
    --plugin org.oscairn.setbfree.leslie@0.2.1 "setBfree Leslie" setBfree.olib clap org.oscairn.setbfree 0.4.4 \
    setBfree macos-arm64=setBfree.clap
```

Installing it puts the bundle into the store under each plugin's own folder, so `plugins.list`
shows each and each can be removed alone. **Preset-discovery providers are not plugins:** a
provider in the bundle (setBfree's `org.oscairn.setbfree.programs` beside its organ and Leslie)
is neither listed nor signed. Each plugin's signature is over the bundle's digest, which covers
every byte of the provider too, and the host finds the provider by scanning the installed
bundle. A signature in `<build>.sig` over an ID that is not packed, such as a provider's, is
refused like one over another build.

**Libraries a build loads.** A plugin build must load nothing from outside the platform's system
libraries and its own bundle: a device has neither Homebrew nor the author's other libraries.
`oscairn-sign` reads each Mach-O (thin or fat) and ELF file in a build itself, with no `otool`
or `readelf`, so a macOS build is checked on Linux too:

- **macOS:** every `LC_LOAD_DYLIB`, `LC_LOAD_WEAK_DYLIB`, `LC_REEXPORT_DYLIB`,
  `LC_LAZY_LOAD_DYLIB` and `LC_LOAD_UPWARD_DYLIB` names a library under `/usr/lib/` or
  `/System/`, or one in the bundle through `@loader_path` or `@rpath` (an `@rpath` library is
  the system's when an rpath is a system folder), and every `LC_RPATH` is a system folder or
  under `@loader_path`. `@executable_path` is the host application's folder, never the plugin's
  bundle, so it is outside; so are `/opt/homebrew`, `/usr/local` and every other path.
- **Linux:** every `DT_NEEDED` is one of the libraries every Oscairn image carries, or a file in
  the bundle found through an `$ORIGIN` rpath, and every `DT_RPATH` and `DT_RUNPATH` entry starts
  with `$ORIGIN`. The libraries: glibc's (`libc.so.6`, `libm.so.6`, `libmvec.so.1`,
  `libdl.so.2`, `libpthread.so.0`, `librt.so.1`, `ld-linux-aarch64.so.1`,
  `ld-linux-x86-64.so.2`), GCC's
  (`libgcc_s.so.1`, `libstdc++.so.6`), `libasound.so.2` (the host does audio and MIDI, but JUCE
  plugins link ALSA through `juce_audio_devices` even when they never open a device), and the
  desktop's windowing and font libraries the image builds with it (`libX11.so.6`,
  `libXext.so.6`, `libXrandr.so.2`, `libXinerama.so.1`, `libXcursor.so.1`, `libXrender.so.1`,
  `libfreetype.so.6`, `libfontconfig.so.1`). Not `libGL`: the image has none. And no symbol
  version it needs (`DT_VERNEED`) is newer than every image has: up to `GLIBC_2.44` and
  `GLIBCXX_3.4.34`, and no `GLIBC_PRIVATE`
  ([the Linux allowlist](plugin-porting.md#the-linux-allowlist)). A sanitizer build of Oscairn
  (tests, never a release) also accepts its sanitizer's runtime (`libtsan.so`, `libasan.so`,
  `libubsan.so`, `liblsan.so`, `libclang_rt.*`) and the rpaths macOS adds to find it, since it
  links that runtime into every binary it makes; a normal build refuses them.
- A Mach-O or ELF file that does not parse is a problem too; other files (resources, scripts)
  are not checked, and Windows builds are not checked yet.

`oscairn-sign plugin-package` refuses such a build, listing each library, unless given
`--allow-outside-libraries`, which packs it and says what it packed anyway; `oscairn-sign
verify` lists each as an `outside` line, the package repository's checks refuse it, and
`oscairn-plugin-check` reports it.

Apps are to be packaged the same way ([RFC 0022](rfcs/0022-apps-and-requests.md) step 2): a
kind of their own, `app`, with an `apps` list beside `plugins`, each app signed as
[RFC 0006](rfcs/0006-packages-and-portability.md#signatures) describes. Not built: a reader
refuses the kind until then.

## Asset packages

An asset package carries an asset library ([RFC 0009](rfcs/0009-sound-assets-and-dependencies.md)):
files plugins read, such as a synth's factory patches and wavetables, and nothing else. `kind`
is `asset`, `items` and `plugins` are empty, its `id` is the library's ID, it names a `license`,
and the manifest's `assets` gives the library's `version` (a plain name, as an ID is) and each
file under `Assets/` by its `path` there, `sha256` and `size`:

```json
"assets": {"version": "1.9.0", "files": [{"path": "wavetables/basic.wt", "sha256": "<64 hex digits>", "size": 16384}]}
```

**Redistribution** ([RFC 0009](rfcs/0009-sound-assets-and-dependencies.md#where-files-live)): whether
an export made on a device may carry the library's files. `assets.redistribute`, `true` or
`false`, says so when present; without it the license decides, as
[RFC 0031](rfcs/0031-package-repository-and-distribution.md)'s license rules do: an SPDX license
grants redistribution, while a `LicenseRef-` license is the publisher's own terms, which Oscairn
cannot read, so such a library stays out of exports unless its package says `true`. A package
(plugin or asset) whose license names a `LicenseRef-` must carry its text under `Licenses/`.
`oscairn-sign asset-package --redistribute yes|no` writes the field.

Every other kind has `"assets": null`. `oscairn-sign asset-package` packs every file in a
folder, hidden ones aside:

```sh
oscairn-sign asset-package --key author.key --license GPL-3.0-or-later --license-file NOTICE \
    --license-file AUTHORS Factory.olib org.oscairn.analog-factory 1.9.0 "Analog factory content" factory/
```

Installing one keeps each file once, by its SHA-256, in the asset store (`assets/store/<sha256>`
in the data folder), and builds a real folder of the library from it,
`AssetLibraries/<id>/<version>/<path>`: hard links to the stored files (a copy where a link
cannot be made), read-only, registered as the library `<id>` so references resolve by its ID and
path. Beside the version folder, `.terms` keeps its license, whether it may be passed on and
whether a plugin package brought it (a line `license<TAB>1|0<TAB>1|0`), and `.licenses/` its
license texts. Preset discovery crawls folders, so a library needs that tree. One version of a library is
installed at a time: a new one is built beside the old one, moved into place, and the old one
removed. A plugin finds the folder through `oscairn.assets`' `library_root`, from its preset
provider too ([plugin porting](plugin-porting.md)).

**Dependencies.** A plugin package names the asset libraries it needs, each by ID and the least
version, in `dependencies.assets` (every kind has `"dependencies": {"assets": [...]}`; only a
plugin package's may list any), packed with `oscairn-sign plugin-package --asset-dependency
<id> <version>`. Installed from a file, a plugin package whose library is missing, or older,
says so in its notes. Installed from a package repository, it brings the newest stable version of
each it lacks that is at least the one named, from the same repository, installed first; the
install fails when the repository has none. The plugin store keeps the libraries a plugin
package needs in `assets.tsv` (`id<TAB>version` lines) in each of its plugins' folders.

**Removal** ([RFC 0009](rfcs/0009-sound-assets-and-dependencies.md) step 4, from explicit roots):
an installed asset library is used by a library item (Sound or Performance) whose document
references it, by ID or by bytes it holds; by the open Performance; by a Performance saved with
`performance.save` outside the library (the engine keeps each such folder in `assets/roots.tsv`
with the references it had when saved, so one on a drive that is away still counts, until its
folder is gone from a folder that is there); or by a plugin package in the store that needs it.
`assets.uninstall` removes one only when nothing uses it, naming what does otherwise.
`assets.collect` removes each library a plugin package brought (not one installed by hand) that
nothing uses, and the stored files no library's tree links. Anything else on the device, such
as a copy of a Performance made by hand or an archive on a USB drive, cannot be seen, so a
library installed by hand is never collected.

## License and source

A package carries its license texts and notices under `Licenses/` (`COPYING`, `NOTICE`,
`AUTHORS`...) and names its license and source in the manifest. A plugin package must name a
license. Object code under the GPL, LGPL or AGPL, any version, must go with its source or
directions to it next to it (their §6, and the AGPL's §13), so a plugin package whose license
expression names any of them is refused unless it gives its `source` and holds at least one
file under `Licenses/`:

```sh
oscairn-sign plugin-package --key author.key --license GPL-3.0-or-later --license-file COPYING \
    --license-file NOTICE --source https://github.com/example/organ v2.1.0 \
    Organ.olib clap com.example.organ 2.1.0 "Example Organ" linux-arm64=Organ.clap
```

`library.contents` gives the license and source, and the desktop's install dialog shows them
under **License**, the source as a link. Installing a plugin package copies `Licenses/` into
each of its plugins' folders in the [plugin store](#the-plugin-store), beside the builds. A
package repository's release record repeats the archive's `source`, and its checks refuse a
record whose `source` or `license` is not the archive's.

## Where plugins are published

Specified 2026-11-11 ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#the-repository-hint)).
**Not built:** no reader takes it yet. Today's archive reader looks up only the manifest fields it
knows and passes over others (`core/control/src/package.cpp`), so an archive carrying it installs,
and nothing reads it.

`pluginRepositories` names, for each plugin the archive's items use, the package repository it is
published in, so a device that lacks the plugin can offer to install it from there. It is advice:
it adds no repository and installs nothing on its own, and a device asks before fetching anything
from the address. The same field, with the same shape, goes in a Performance and a Sound
([format](performance-format.md#where-plugins-are-published)); an export writes the union of its
items' entries, and an install keeps each item's own.

```json
"pluginRepositories": [
  {
    "format": "clap",
    "id": "com.example.organ",
    "repository": {
      "address": "https://packages.example.com",
      "key": "9f2c4e81a07b3d5566e1f0c2b8a94d37e6015cfa28b4d9e3a7c1f60b52d8e4a9"
    }
  }
]
```

| Field | Meaning |
| --- | --- |
| `pluginRepositories` | Absent, or a list of at most 4096 entries; at most one per `format` and `id` |
| `format` | `clap`, the only format repositories carry |
| `id` | The plugin's ID exactly as the items' nodes name it, at least 1 byte |
| `repository.address` | The repository's `https://` address, as `packages.repositories` lists it: at most 2048 bytes, the host in lowercase ASCII letters, digits, `.` and `-` (an internationalized name as `xn--`), an optional `:port`, then optional path segments; no user info, query, fragment, spaces or `/` at the end |
| `repository.key` | The repository's root key: 64 lowercase hex digits, the Ed25519 public key itself, as the repository's `repository.json` gives it in `rootKey`. Not the 16-digit fingerprint, which a device shows people and derives from it |

No version: the version is each node's. No name: a device names a repository by its signed root.
No package ID: the repository's index says which package holds the plugin. The pattern for the
address and key is in [performance.schema.json](performance.schema.json)'s `$defs`,
`PluginRepositories`, which this field shares.

Once built, a reader refuses the archive, as for any malformed field, when an entry is not this
shape, a plugin is named twice, or there are more than 4096.

### For converters

A converter writing libraries that use published, signed packages:

- Write `pluginRepositories` in `library.json`, one entry per plugin the items use that is
  published, with the format, ID, address and key above. Today's reader passes over it.
- Copy `key` from the repository's own `repository.json` (`rootKey`) after checking it against the
  key the repository's owner publishes, or from `packages.repositories` on a device that uses it.
- Do **not** write it in `performance.json` or `sound.json` yet. Those readers refuse a field they
  do not know (`fields()` in `core/control/src/performance.cpp`), so a version 13 Performance or
  version 8 Sound carrying it does not open. It goes there from Performance version 14 and Sound
  version 9, when built.
- Do not write an `http://` address, a folder path, a mirror or blob base instead of the
  repository's address, a `/` at the end, a fingerprint instead of the key, a name, a package ID
  or a version.
- Do not write entries for plugins the items do not use, or two for one plugin.
- Do not change a plugin's ID or version to match what is published: write those the saved state
  is from.
- Do not write a lock entry (`lock` in a document) for a build the converter has not hashed
  itself; write none rather than guess. A device compares a locked digest with the published
  build before installing it.

## Package dependencies

Specified 2026-11-11 ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#the-starter-library)),
for the starter library of Performances that comes with Oscairn and any library whose items use FM,
Analog or another published plugin. **Not built:** no reader or writer takes it yet. Today's reader
reads only `assets` in `dependencies` (`core/control/src/package.cpp`), so an archive carrying it
installs, and nothing reads it. No format version change: an older reader passes over it.

A library, an exported Performance or an exported Sound names the packages its items need,
plugin packages and asset libraries alike, each by package ID and the least version:

```json
"dependencies": {
  "assets": [],
  "packages": [
    {"id": "org.oscairn.fm", "version": "0.4.8"},
    {"id": "org.oscairn.analog-factory", "version": "1.0.0",
     "repository": {"address": "https://packages.oscairn.org", "key": "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841"}}
  ]
}
```

| Field | Meaning |
| --- | --- |
| `packages` | Absent, or a list of at most 256 entries, one per `id`; only in kinds `library`, `performance` and `sound` (a plugin package keeps naming its asset libraries in `assets`) |
| `id` | The package's ID in its repository, as plain as a library's ID |
| `version` | The least version that works, as plain as an asset library's version. The items' own plugin versions stay in their documents; this says which package brings them |
| `repository` | Optional: where the package is published, with the shape and rules of `pluginRepositories`' `repository` (an `https://` address and the root key). Absent: the repositories the device knows, in `packages.repositories`' order |

Once built, a reader refuses the archive, as for any malformed field, when an entry is not this
shape, an ID is named twice, or there are more than 256.

**Writing it.** `library.export` writes one entry for each package recorded in `packages.json`
that holds a plugin the items use (by format, ID and version) or the asset library their documents
reference, at the version installed, with the repository it was recorded from when that is an
`https://` address. A plugin or library not installed from a package gets no entry; its plugin is
still named in the items, as now.

**Installing with it.** `library.contents` lists the entries, each with the version installed
here, if any (`packages.installed`; a plugin package at that version or later, an asset library at
that version or later). `library.install` installs the items as now, whatever is missing, which
open as stand-ins until it is there, and its notes name each missing package. Nothing more is
installed on its own: the install dialog offers each missing one ("Needs Oscairn FM 0.4.8 or later
from Oscairn: Install"), which is a `packages.install` from the named repository, or the first
known one whose index has the ID, at the newest stable version that is at least the one named, its
own asset libraries coming with it as now. A package repository's index builder writes the entries
into the release's `requires.packages`, so the Catalog shows what a library needs before it is
installed and the Dependencies check covers it. In a preinstalled snapshot the starter library's
packages are in the same snapshot, and libraries are installed after the packages they need.

## Reading one

An archive is untrusted. Reading extracts it into a new folder of its own, refusing, and leaving
nothing behind:

- an entry outside `library.json`, `Performances/`, `Sounds/`, `Assets/`, `Plugins/` and
  `Licenses/`;
- a plugin package naming no license, or under the GPL, LGPL or AGPL without its `source` or a
  file under `Licenses/`, and a `source` that is not an `https://` address and a revision;
- a plugin or asset package whose license names a `LicenseRef-` without a file under
  `Licenses/`, and an asset package whose `redistribute` is not `true` or `false`;
- an asset package naming no license, with items or plugins, whose version is not plain, that
  lists no file, a file twice, one whose path is not safe or that is not under `Assets/`, or
  holds a file under `Assets/` it does not list; a file whose SHA-256 or size is not the one it
  lists; `assets` in any other kind; and asset dependencies anywhere but in a plugin package, or
  whose ID or version is not plain;
- a path that is absolute, climbs out (`..`), names a drive (`:`), uses a backslash, is hidden
  (a component starting with `.`), has a control character, or is longer than 1024 bytes;
- a link, or an entry twice;
- more than 1,048,576 entries or 64 MiB of their names, more than 8 GiB in all, an entry over 2 GiB or a manifest over
  64 MiB, as declared; and a
  file that extracts to another size than it declares;
- a manifest that is not version 3 (there is no reader for version 2), whose ID or an item's
  is not plain (as `id` above says), whose kind is not one above, a plugin package that lists an
  item or no plugin, or that names an item whose folder or document is not there, or
  whose document's ID is not the one listed, or a plugin build not under `Plugins/`, not there,
  or for a platform other than `linux-arm64`, `linux-x86_64`, `macos-arm64`, `macos-x86_64` and
  `windows-x86_64`, or whose path is not a `.clap` or `.lv2` bundle (the store finds builds by
  that extension);
- a file whose SHA-256 is not the one `files` lists, a file `files` does not list or one it
  lists that is not there, and a plugin build whose digest is not its own;
- a signature that does not verify, at any level, or a malformed one.

Reading only the manifest (an archive's contents, before installing) checks the archive's and
the builds' signatures; items' signatures are checked when the archive is extracted.

## Signatures

Every signature is optional, and what is signed is the publisher's choice
([RFC 0006](rfcs/0006-packages-and-portability.md#signatures)). Each is `{key, signature}`:
the signer's Ed25519 public key (64 hex digits) and the signature (128), over a message that
says what it signs:

| Where | Signs | Message |
| --- | --- | --- |
| `items[].signatures` | The item: a Sound or a Performance | `oscairn-item-1\0` and its content hash, the library's `hash` (its document without `info`, and its `nodes/` files) |
| `plugins[].builds[].signatures` | A plugin build, by its publisher | `oscairn-build-1\0` and the plugin's format, ID, version, the build's platform and digest, each followed by `\0` |
| `library.sig` | The whole archive | `oscairn-archive-1\0` and `library.json`'s exact bytes, which list every file's SHA-256 and every build's digest |

A key is shown by its ID, the first 16 hex digits of the SHA-256 of its 32 bytes. An export
signs its items, the whole archive, or both, only when the exporter asks, with a key of theirs
from the data folder's `Keys/`. A build's signatures travel with it: an export carries those in
`<bundle>.sig` beside the build it takes (a JSON list of `{key, signature}`, as
`oscairn-sign build` writes it), whether at the plugin's own path or in the store, leaving out
and noting any that is not over that build; an install, and `plugins.addBuild`, keep them in the
store the same way, beside the build. A signature that does not verify is refused like a wrong
hash. Who signed is reported; whether that is
trusted is the installing device's [trust store](rfcs/0006-packages-and-portability.md#signatures).

## As a folder

A package can also be an unpacked folder
([RFC 0031](rfcs/0031-package-repository-and-distribution.md#installing-in-little-space)): the
same tree as inside the zip, with `library.json` and every file it lists by hash. It is a second
form of this format, read by the same rules, except that hidden files a desktop adds are
skipped. Each file is copied into the install's folder in the data folder and hashed there, so
a drive that changes underneath is caught. An archive on a USB drive or a share is read in place,
never copied first. Before anything is written, the install checks that the data folder's file
system has room for every entry and the largest once more; what it unpacked is then moved into
place by renames, so an install needs room for the installed files plus one entry, not twice
that. A pulled drive or a bad hash leaves nothing behind.

## Installing one

- **As a library:** locked, in `Libraries/<id>/` in the data folder with its manifest. Items
  keep their IDs. Installing again copies only the items that are missing. Uninstalling removes
  the folder; deleting an item removes its folder, and installing again brings it back. An ID
  that differs from an installed library's only in letter case names the same folder on a
  case-insensitive file system (macOS's and Windows's by default), so there it is refused: two
  libraries never share a folder.
- **Into the user's own storage:** each item is copied into `Performances/` or `Sounds/` under a
  new ID and a new revision, its info's `origin` recording the archive's ID, the item's ID there
  and the copy's content hash. A Performance's parts that came from a Sound in the archive point
  at that Sound's copy. Installing again skips an item whose copy is still there with that hash;
  one edited or deleted since is copied again.
- **All of it or chosen items.**
- **Samples** go into the user's sample library (RFC 0009) by their hash either way, so they stay
  when a library is uninstalled. A sample an item needs that is in neither the archive nor the
  device is reported.
- **Plugins:** every build of each plugin the installed items use, or of each plugin in a
  plugin package, goes into the plugin store,
  each whole or not at all, so this device can pass the others on in its own exports (a desktop
  carrying a device's builds). A Performance whose plugin is not at its `pathHint` finds this
  device's build there. A build already there with the same digest is left; a different one is
  kept and reported; a plugin with no build for this device is reported.
- **Signatures** ([RFC 0006](rfcs/0006-packages-and-portability.md#signatures)): before anything
  is installed, each plugin build that would go into the store is judged against the device's
  trust store. One unsigned or signed by an unknown key refuses the install, listing them, until
  the user says yes (`acceptUnsignedCode`); one signed by a revoked key is never installed, and
  is noted. Sounds, Performances and samples install whether signed or not.

## The plugin store

`Plugins/<format>/<id>@<version>/` in the data folder (characters other than letters, digits,
`.`, `-`, `@` and `_` made `_`) holds `plugin.json`, `{format, id, version, name}`, as the folder
name cannot say them exactly, and a folder per platform with its build: `macos-arm64/Odin2.clap`.
The desktop's plugin scan looks in each plugin's folder for its own platform, so what an
archive installed is found there.
`plugins.list` shows each plugin with the library items that use it; removing one is refused
while the open Performance plays it, and what used it opens with a stand-in until it is back.

Two items with one ID are listed once, the user's own first: so a Performance exported and
installed as a library on the device it came from shows the original, not the locked copy.
Installed into the user's own storage (the default for an export) it is a new copy.

Not yet: a Sound from a locked library exported as that library's item, and grouped cleanup of plugins nothing uses (RFC 0006).
