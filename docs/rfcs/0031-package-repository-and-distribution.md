# RFC 0031: The package repository and distribution

Status: proposed, 2026-11-01. Fills in step 6 of
[RFC 0006](0006-packages-and-portability.md) ("Catalogs, multi-device preparation and
distribution polish") for packages. Builds on RFC 0006's packages, locks, offline repair and
[signatures](0006-packages-and-portability.md#signatures), the
[library archive](../library-archive.md), the library
([RFC 0021](0021-sounds-performances-and-library.md)), apps
([RFC 0022](0022-apps-and-requests.md)), item types ([RFC 0023](0023-item-types.md)), device
profiles ([RFC 0024](0024-device-profiles.md)), resource estimates
([RFC 0025](0025-resource-budgets.md)), paired devices and library sync
([RFC 0026](0026-network-control-and-web-ui.md)), system images
([RFC 0028](0028-system-image-updates-boot-and-power.md)), translations
([RFC 0029](0029-internationalization.md)), the system shell
([RFC 0030](0030-system-shell-launcher-and-navigation.md)) and sample assets
([RFC 0009](0009-sound-assets-and-dependencies.md)).

Revised 2026-11-01 with Jacob's input: [ideas from npm, pnpm and yarn](#ideas-from-npm-pnpm-and-yarn),
[installing in little space](#installing-in-little-space) (unpacked package folders and
streamed installs), [decisions](#decided-2026-11-01) on paid content, large libraries and
manufacturers' repositories, and [research and a proposal](#publisher-trust-and-review) on
publisher trust and review. Those two stay open until Jacob confirms the proposal.

Revised 2026-11-11 from the owner's request of that day:
[finding missing plugins](#finding-missing-plugins) in the repositories a device knows, a
repository named per plugin in archives, Performances and Sounds, and what the index must carry
for it. Proposed, not built; its [open questions](#open-questions-on-finding-missing-plugins)
are the owner's.

Revised 2026-11-12 from the owner's decision of that day: releases stored in pieces is the plan,
and beside the pieces the repository publishes a
[ready-made archive per platform](#ready-made-archives-per-platform) of each version, for a
person to install by hand. Planned, not built; its open questions are the owner's.

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. The repository format; installing from a folder or USB drive, from an unpacked package
      folder, and streamed
      (built 2026-11-06: the format as the template's index builder publishes it, whole archives
      as blobs; installing from a repository folder, its root, index key and index verified
      against a root key trusted for `repositories`, every blob checked by its SHA-256 and size,
      an older root refused, builds this device cannot run and versions needing a newer system
      not installed, only this device's builds installed; `packages.list`, `packages.install` and
      `packages.job`, reading, checking and unpacking off the control thread; unpacked package
      folders and archives read in place, a free-space check before writing, items and builds
      moved into place by renames; in the desktop Library, Install from a repository (a folder);
      `control-repository`, `desktop-gaps`. 2026-11-07: a device's list of repositories
      (`packages.repositories`, `addRepository`, `removeRepository`, `enableRepository`,
      `setDeviceRepositories`): the official one always listed, its root key trusted for
      repositories only and built in, never for plugins (the official publisher's key is built in
      for plugins since 2026-11-11, step 5); a profile's `repositories` and its
      discouraged `hideOfficial`, which turns the official one off without removing it; the
      user's, added by address and root key; each on or off; `control-repository`,
      `desktop-profile`. In Settings (the desktop's dialog, then from
      2026-11-11 the Settings app's Package repositories page, RFC 0022): the repositories by
      name, address and root key fingerprint, each with a switch, Remove for the user's, and Add
      a repository..., which reads the repository first, asks to trust its root key by
      fingerprint and the name that key signed (an https:// address alone in the app), and keeps
      neither the repository nor the key if it then does not verify; it replaces the Library's
      Install from a repository...; `desktop-settings`. 2026-11-11: each installed package recorded with its version, kind and
      repository (`packages.json` in the data folder, `packages.installed`), and forgotten when
      removed; `control-repository`. 2026-11-11: each repository shown by the device's name for
      it, never its root's, in the Catalog, the install questions and `packages.job`
      (`claimedName` beside it), names alike told apart by key
      ([names](#kept-state-keyed-by-the-root-key)); `control-repository`. 2026-11-11: each known
      address read only with the root key recorded for it, and the app's snapshot only with a
      shipped one's; another key refused naming both, nothing kept
      ([signatures](#signatures-and-freshness)); `control-repository`, `control-preinstall`. Not yet: releases stored in pieces (items, samples
      and builds as separate blobs, decompressed as they stream), `oscairn repo build`, deltas
      (step 2), `.orepo` files; the free-space refusal and a drive pulled midway are not tested.
      Planned, not built (owner, 2026-11-12): pieces, and with them a ready-made `.olib` per
      platform of each version, assembled from the pieces at publish time for downloading by hand,
      replacing the whole archive a person can fetch from the submissions repository's Git tree
      today; built together, with no interim archive per platform in the record
      ([ready-made archives](#ready-made-archives-per-platform)))
- [ ] 2. Fetching over HTTPS: refresh, incremental index updates, mirrors, resumable downloads
      (built 2026-11-06: fetching through the host's transport, JUCE's URL streams: the system's
      HTTPS on macOS, libcurl with Mbed TLS on Linux since RFC 0039's step 1 (2026-11-07, which
      says what it tested); the index kept per
      repository and brought up to date by deltas, which the index builder now writes from the
      root published last (`--previous-site`); blobs from the mirrors, the blob base and the
      repository's own address in turn, a bad copy refused and the next tried; a download cut
      short resumed with a range request; `control-repository-web` against a local HTTP server.
      2026-11-07: the desktop Library opens a repository's https:// address (since moved to
      Settings' Add a repository..., step 1; the official one
      offered; its key was not trusted until the user said so, and is built in since step 1's list
      of repositories; any other scheme refused);
      `desktop-gaps` drives it through a stand-in transport, up to the trust prompt and a second
      read, not an install from the web.
      Not yet: fetching the root from a mirror when the repository's own address
      is down, per-platform fetches (releases are still whole archives))
- [ ] 3. Publishing: the `oscairn publish` command, checks, immutable versions and yanking
- [ ] 4. Revocation lists and the takedown process
      (built 2026-11-11: the list's format specified ([the revocation list](#the-revocation-list));
      read with the root, by its hash, a missing or malformed one refusing the repository; kept per
      root key after the serial is taken; withdrawn versions and revoked keys' packages listed as
      withdrawn and never installed, the official list's keys and builds applying to every
      repository; what is installed reported (`packages.installed`'s `withdrawn`, the Catalog),
      never removed; `control-repository`, `desktop-gaps`. Not yet: the takedown policy, the
      official list in system images, revoked builds in what is installed, remembering revoked
      index keys; the official list applying to another repository is not tested)
- [ ] 5. Discovery: Install… in the Library, the Catalog app, compatibility filters
      (built 2026-11-07 on the desktop, [interim](#in-the-library) until the Catalog app:
      Install… with Catalog and File; the Catalog listing the enabled repositories' packages of
      the kind shown (since 2026-11-11 what is installed too, marked Installed with its version, or
      Update to a newer version; `desktop-gaps`), searched, grayed with the reason where this device
      cannot install them (its platform and system version), a package's details and Install,
      and trusting an unknown publisher for plugins from the install question; each version's
      `types` and the packages it needs (`requires.packages`, which the index builder now
      writes from the archive's asset libraries) in `packages.job`'s listing; `desktop-gaps`
      against a signed folder repository, `control-repository`. Not yet: the Catalog app and
      its actions, `packages.search` and `packages.get`, the index in the library's database,
      estimates and surfaces as filters, Show all, the Panel's own Catalog screen (the desktop
      dialog is drawn at touch sizes there); the gate's search on an instrument with no network
      is not tested: listing still fetches the root)
- [ ] 6. Updates: packages, Performances' locks moved on explicitly, apps
      (built 2026-11-11 on the desktop: the Catalog's Update to the newest stable version this
      device can install, newer than the one installed; `desktop-gaps`, `control-repository`.
      A locked library's new version replaces each item whose files differ, swapped in whole,
      and removes those it no longer has; the same version again changes nothing;
      `control-archives`. Each install records the archive's SHA-256 and signers: the Catalog
      says Installed only for the repository's build at that version, else Installed (a
      different build), with an explicit Install the repository's build after a confirmation
      naming both (`packages.install`'s `replace`); nothing updates on its own;
      `control-repository`, `desktop-gaps`. Not
      yet: `packages.updates`, update lists and a card on Home, Performances' locks, apps)
- [ ] 7. Offline and multi-device: fetching for a device, USB subsets, a paired computer as a source
- [ ] 8. The official repository goes public: hosting, mirrors, policy
- [ ] 9. Each version's plugins in the index; repositories' kept state keyed by their root key
      ([finding missing plugins](#finding-missing-plugins))
      (built 2026-11-11: serials seen and kept indexes keyed by root key and repository ID,
      version 1's moved once; `control-repository`. Not yet: each version's plugins, the index
      builder's checks, an address-to-ID map for lookups)
- [ ] 10. The repository hint, `pluginRepositories`, read, checked and kept in archives,
      Performances and Sounds
- [ ] 11. `packages.lookup`, and what is missing in installs, prepares, the library and stand-ins
- [ ] 12. Offering the install: the Library, the Parts view, a hinted repository, the Setlist
- [ ] 13. Newer versions whose publisher says they open the saved state (`opensStateFrom`,
      [RFC 0043](0043-moving-sounds-to-newer-plugin-versions.md#the-publishers-claim), if the
      owner accepts it)
- [ ] 14. [Preinstalled packages](#preinstalled-packages): a release ships a repository snapshot
      and the engine installs from it at its first start
      (built 2026-11-11: the snapshot in `Contents/Resources/Preinstalled` (macOS) or
      `Preinstalled/` beside the programs; `tools/preinstall-fetch.py` and
      `tools/preinstalled.txt`, run by `tools/release-macos.sh`, which fails when a listed package
      does not verify; installing at the engine's start through `packages.list` and
      `packages.install` jobs from the snapshot folder, offline; `preinstalled.json`;
      `packages.preinstalled`; a folder copy of a repository the device ships recorded as that
      repository; the desktop Library's "Setting up…" line; the packages' licenses in the dmg's
      `Licenses/Packages/`; a refusal said as "<name> was not set up"; an older root's index
      never kept over a newer one; `control-preinstall`, `control-preinstall-web`. The dmg (44.6
      MB, 31.7 MB without the packages) tried on a new scratch home and data folder with no
      `trust.json`: all seven installed in 3 to 7 s with the built-in publisher key, no question,
      recorded from `https://packages.oscairn.org` with the index's archive hashes (so the
      Catalog says Installed), and a Performance with FM saved. The "Setting up…" line was not
      looked at. Not yet: Linux builds of the packages and a Linux release ([known
      gaps](#what-ships)); the device image; the starter library of Performances and
      [its dependencies](../library-archive.md#package-dependencies), specified, not built)

## Problem

Musicians and publishers want one place to share plugins, Sounds, Performances, sample
libraries, themes, apps and translations, and Oscairn should find and install them there.

RFC 0006 has the pieces around this: packages and their identity, locks that never move on
their own, configured sources, offline repositories on USB or a paired computer, and
signatures. It leaves the repository itself open: what file a device downloads to learn what
exists, how a publisher puts a package there, who hosts it, how updates and search work, what
happens when something must be taken down, and how an instrument that never sees the internet
gets any of it.

## Outcome

- A **repository** is a set of static files: a small signed root file, a signed index and the
  package files, named by their hashes. Any static web host or CDN can serve it, anyone can
  mirror it, and the same layout works as a folder on a disk or a USB drive.
- A device can use **several repositories**: the official one, a manufacturer's, a
  publisher's own, and the user's.
- A **publisher** runs one command to check, sign and submit a release. Every package in a
  repository is signed by its publisher. Versions never change once published; a bad one is
  yanked, not overwritten.
- Musicians **browse and search** in the Library and in a Catalog app, filtered by what runs on
  their device. Updates never change a Performance's sound without the musician saying so.
- An instrument with no internet gets the same packages from a USB drive or a paired computer,
  checking the same signatures.
- The first version needs **no server-side code**: static hosting, a CDN, a Git repository for
  submissions and a CI job.

## Prior art

| System | What Oscairn borrows | What it does not |
| --- | --- | --- |
| [Debian/APT](https://wiki.debian.org/SecureApt) | A small signed root (`InRelease`) naming the index files by hash; untrusted mirrors; any HTTP server | Its dependency solver and maintainer scripts |
| [The Update Framework](https://theupdateframework.io) | An offline root key delegating to an online index key; refusing an older index (rollback) | Its expiry dates: instruments may have no clock |
| [Flathub](https://docs.flathub.org/docs/for-app-authors/submission) | Submissions as pull requests to a Git repository, built and checked in CI; publisher IDs verified by domain | Its runtime model and server-side build farm |
| [F-Droid](https://f-droid.org/docs/Reproducible_Builds/) | A signed index; reproducible builds checked against the publisher's; the index usable from a USB drive or a nearby device | Building every package from source on the project's machines |
| [npm](https://docs.npmjs.com/policies/unpublish) and [crates.io](https://doc.rust-lang.org/cargo/commands/cargo-yank.html) | Versions immutable once published; yanking hides a version from new installs while locks still fetch it | A live registry API and accounts to install |
| [VCV Rack Library](https://library.vcvrack.com) | One place for free and paid plugins of one host, per-platform builds, a plugin manifest checked on submission | A login required to install, and a central server |
| Bitwig and Arturia preset stores | Browsing sounds by type and tag inside the instrument, with audio previews | Accounts and closed formats |
| [MOD Audio's plugin store](https://mod.audio) | Plugins for a hardware device, filtered to what builds for it, installed from a computer or the device | A store that only its own server can serve |

The links are pointers for later reading; this RFC adopts none of their code.

## Ideas from npm, pnpm and yarn

Jacob asked on 2026-11-01 which ideas from the JavaScript package managers apply. Already
borrowed:

- **pnpm's content-addressed store:** RFC 0006's
  [shared store](0006-packages-and-portability.md#project-manifest-lockfile-and-shared-store).
- **`npm ci`:** installing exactly what the lock names, never a newer version (RFC 0006).
- **Immutable versions and yanking,** from npm and crates.io ([below](#versions-and-yanking)).

Considered:

| Idea | Decision | Why |
| --- | --- | --- |
| [Scoped names](https://docs.npmjs.com/cli/v11/using-npm/scope) (`@publisher/package`) | Not adopted | Package IDs are already reverse-DNS under the publisher's ID (`com.example.organ`), which is the same namespacing. Scopes fixed npm's flat namespace; Oscairn never had one |
| [Dist-tags](https://docs.npmjs.com/cli/v11/commands/npm-dist-tag) as release channels | Adopted, two channels | A version is published to `stable` (the default) or `beta`. The index lists each version's channel. A device offers beta versions of a package only when the musician turns them on for it. Locks name versions, never channels, so a channel never moves a Performance |
| [`npm deprecate`](https://docs.npmjs.com/cli/v11/commands/npm-deprecate) | Adopted | A publisher can mark a package, or a range of its versions, deprecated with a message and an optional replacement package ID. It stays installable and is shown with the message. Unlike yanking, it is advice. Nothing is replaced on its own |
| [npm provenance](https://docs.npmjs.com/generating-provenance-statements) (Sigstore attestations) | Not adopted on devices | It is a second signature system with its own services and trust root, which devices would have to carry and update, and npm supports it only from GitHub Actions and GitLab CI on cloud-hosted runners. Oscairn's Ed25519 signatures already say who. The goal, linking a build to its source, is met by the reproducibility check: the manifest names the source and builder image, CI rebuilds, and the index says whether it matched. A desktop tool may show a Sigstore bundle a publisher adds to the details; it grants no trust |
| [Integrity hashes in the lock](https://docs.npmjs.com/cli/v11/configuring-npm/package-lock-json#packages) (Subresource Integrity) | Already adopted | A Performance's lock records each build's digest, and a package's identity is its manifest's hash (RFC 0006). Any source, a mirror, a USB drive or a friend, is checked against them. Oscairn writes SHA-256 in hex everywhere, not SRI's `sha512-` base64 strings |
| [Yarn's offline mirror and zero-installs](https://yarnpkg.com/features/caching) | The mirror, yes; zero-installs, no | The offline mirror is a project's packages kept beside it so it installs with no registry. A USB repository (a signed root, index and the blobs needed) is the same idea, checked the same way. Zero-installs runs code straight from the committed cache. An instrument does not play from a removable drive, which can be pulled mid-show, so it always installs into its own storage. It can install by reading the drive in place ([below](#installing-in-little-space)) |
| [pnpm's hard-linked store](https://pnpm.io/motivation) | Already adopted, within one filesystem | RFC 0006's store keeps each file once, by hash, and links or clones it where it is used. Samples are stored once by hash already (RFC 0009). Links stay inside the data folder's filesystem: never to a removable drive or a share, which may be gone at the next start |
| [`engines`](https://docs.npmjs.com/cli/v11/configuring-npm/package-json#engines) | Already adopted as `requires.oscairn` | Both name the host version a package needs. npm's is advisory unless `engine-strict` is set. Oscairn's is enforced: a package needing a newer system is shown with "needs system update" and not installed. Platform builds and estimates cover what `os` and `cpu` cover in npm |
| [Workspaces](https://docs.npmjs.com/cli/v11/using-npm/workspaces) | Adopted for publishing | A publisher with many packages keeps them in one folder with a list of members. `oscairn publish` checks them together, publishes each changed one as its own version, and submits them in one pull request. The dependency check accepts a dependency submitted in the same pull request. Devices see only separate packages |

## A repository

### What is in it

```
repository.json        the root: who this is, its keys, mirrors, and the current index
repository.sig         the root's signature
index/<sha256>.json.gz     the full index, named by its hash
index/<sha256>.delta.gz    changes from one index to the next
details/<sha256>.json      a package's long description, screenshots and previews
revocations/<sha256>.json  the revocation list
blobs/<ab>/<sha256>        package files: manifests, items, samples, plugin builds
```

- **Everything but the root is named by its SHA-256** and never changes. A CDN can cache it
  forever, and a mirror copies it once.
- **The root is the only file that changes.** It is small (a few kilobytes) and names the
  current index, the deltas, the revocation list and the mirrors by hash and size.
- **Blobs are kept by the first two hex digits of their hash** (`<ab>`), so no folder or
  GitHub release holds too many: a blob is at the root's blob base, then `<ab>/<sha256>`. In a
  folder or on a USB drive it is at `blobs/<ab>/<sha256>` beside the root, whatever the blob
  base says.
- **Compressed with deflate (gzip),** which the vendored miniz already reads. No new library.

### Package files

A release is published as a library archive ([format](../library-archive.md)), signed as
RFC 0006 says. A package that carries only plugins is a
[plugin package](../library-archive.md#plugin-packages) (kind `plugin`, no items); apps are to
be another kind of the same archive. The repository stores it in pieces, each named by its hash:

- the archive's manifest (`library.json`) and its signature (`library.sig`);
- each item folder, as one zip;
- each sample, as itself, so a sample shared by several packages or kept between versions is
  stored and downloaded once (RFC 0009 stores samples by hash already);
- each plugin build, as one zip of its bundle, with the build's own digest and signatures as
  the manifest lists them.

A device fetches the manifest, checks its signature, and then fetches only what it needs: the
items chosen ([RFC 0006](0006-packages-and-portability.md#choosing-what-to-import)), the
samples it does not have, and its own platform's builds. It can put the pieces back together as
an ordinary `.olib` archive, byte for byte the archive's entries, to pass on by USB. For a
person with a browser, the repository also publishes a
[ready-made archive per platform](#ready-made-archives-per-platform), so nobody has to fetch the
pieces to get one.

A package can name a **blob base** of its own: an HTTPS address the publisher hosts, where its
large files are. Their hashes are in the signed manifest, so they need no trust in that host.
This keeps a 20 GB piano off the project's bandwidth.

### Ready-made archives per platform

Decided by the owner 2026-11-12: releases are stored in pieces, so that "a mac user doesn't have
to download the linux and accelerated binaries too", and next to the pieces the repository
publishes a ready-made `.olib` for each platform of each version. A person downloads one in a
browser and installs it by hand, or carries it to an offline device, with no Oscairn doing the
fetching. Today that person downloads a version's whole archive, `packages/<id>/<version>.olib`,
from the submissions repository's Git tree; once releases are stored in pieces that file is gone
and these replace it. Plugin forks' release notes link to them. **Planned, not built:** both come
together, and there is no interim format with one archive per platform in the record.

- **Derived, never a source.** An export is assembled from the pieces when a release is published,
  by the job that writes the index. Devices keep fetching pieces; none needs an export for a
  normal install, update or repair, and a repository, mirror, snapshot or USB drive without them
  works the same. The root does not name them and the index does not hold them (below).
- **What it holds:** the manifest and its signature (`library.json`, `library.sig`), what all
  platforms share (every item, sample and the other entries the manifest's `files` lists), that
  platform's plugin builds each with its signatures, and `Licenses/`. No other platform's builds.
  It is a normal `.olib` ([format](../library-archive.md)): any Oscairn installs it by the
  existing archive path, from the file browser or read in place from a USB drive
  ([installing one](../library-archive.md#installing-one)), with the same signature, trust and
  license checks as any archive.
- **A package with no plugin builds** (an asset library, a library of Sounds or Performances) has
  one archive, named without a platform: it is the whole release.
- **Identity and signatures.** A package's identity is its manifest's hash, and each build carries
  its own signatures, which are over the build's digest and do not depend on the archive. But
  `library.json` lists every platform's builds with their paths, and `library.sig` signs those
  exact bytes, and the reader today refuses an archive whose manifest lists a build that is not
  there ([reading one](../library-archive.md#reading-one)). So **an archive holding one platform's
  builds cannot keep the same manifest and still be read as the format stands**: that is an open
  question below, with its options. Who signs, either way: only the publisher. The publisher's key
  never leaves the publisher's machine ([publishing](#the-command)) and the repository's job holds
  the index key, so the job does not sign an export, and the index key and root key sign nothing
  they do not today: the root signs `repository.json` and nothing else names an export.
- **Naming.** `<package ID>-<version>-<platform>.olib`, such as
  `com.example.organ-2.1.0-macos-arm64.olib`, and `<package ID>-<version>.olib` for a package with
  no builds. The platform is one of the format's five. Every part is already plain
  (letters, digits, `.`, `-`, `_`), so the name needs no escaping.
- **Address.** One stable pattern from the same three parts, served from the same static hosting
  as the blobs: `<export base>/<package ID>/<version>/<file name>`. A link in a release note is
  built from the ID, version and platform with no lookup, and does not hold a hash. **The exact
  URL, the export base and how it maps onto GitHub Releases (whose assets are flat under a tag)
  are to be fixed when this is built:** [hosting](#hosting-and-cost) does not settle them.
- **Outside the index.** An export is not listed there. Its address follows from the package ID,
  the version and a platform the index's `platforms` already lists, so the Catalog can show
  "download for another device" with no new field; and a file no device needs for an install is
  not a promise the signed index should make, one more hash to keep right in an index meant to
  stay small ([size](#size)). The index's `platforms` sizes are what a device downloads, not the
  export's, so the Catalog shows an export's size only as about the common size plus the
  platform's.
- **Yanked and withdrawn.** A yanked version's exports stay, as a lock may still name it and the
  person chose it: yanking is advice ([versions](#versions-and-yanking)). A withdrawn version's
  exports are removed in the publish that adds the withdrawal to the revocation list, and so is
  the export of a revoked key's package. Copies already downloaded stay, as installed packages
  do; installing one is checked as any archive is ([the revocation list](#the-revocation-list)).
- **A revoked build inside an export.** Builds are revoked by SHA-256 and a device checks each by
  its hash when it installs, from a repository or from an archive; an export adds no way round it.
  An export holding a revoked build is removed.
- **Sizes.** An export copies bytes the pieces already hold: the shared part once per platform,
  plus that platform's builds. A package for four platforms holds its shared part five times: once
  as pieces and once in each export. That is the trade for a file a person can download and
  install with no Oscairn fetching it; it costs storage, not a device's downloads. Question 3
  limits the worst case.

**Open questions** (the owner's):

1. **The manifest of a one-platform archive.** (a) *The same manifest.* The reader learns that a
   listed build for another platform may be absent, a change to
   [the archive format](../library-archive.md#reading-one) (today it refuses a missing build).
   The identity, `library.sig` and the Catalog's same-build check stay as they are, and nothing
   needs signing; a half-present build or a wrong digest is still refused, and the rule for which
   absences are allowed is fixed when the reader changes. (b) *A manifest per platform,* with only
   that platform's builds, each signed by the publisher: no reader change, but each export has its
   own identity, which breaks "one version, one manifest hash" or makes a lock made from an export
   name an identity the index does not list, and the publisher's own tool must sign and submit
   every export, since the repository's job cannot. (Oscairn's own plugin packages are signed in
   the submissions repository's import workflow, which holds the official publisher's key in a
   protected environment, so for those the signing could happen there; other publishers sign on
   their own machines.) This RFC leans to (a); nothing is decided.
2. **The address after the move to object storage.** A release-asset address changes with the
   blob base, which would break links in release notes. Either the project puts a name it
   controls in front of the exports from the start (a CDN rule is not server-side code), or the
   links change once and the notes are corrected.
3. **Large packages.** A GitHub release file must be under 2 GiB, and the index builder refuses
   an archive over 1,900 MiB ([hosting](#github-first)); a package with a blob base of its own is
   kept off the project's bandwidth. Proposed: no export when it would be over that size or the
   package has a blob base, so the person uses the Catalog. Then the Catalog cannot know from the
   index that an export is missing, and either a missing link is acceptable or the index gets a
   flag.
4. **A revoked build among several platforms.** The manifest lists every platform's builds, so a
   revoked Linux build is listed in the Mac export's manifest too. Does it withdraw the whole
   version, taking every export, or only the platform whose build it is?
5. **Revocation on a file install.** This RFC says a withdrawn version, a revoked key and a
   revoked build are refused when installed from a repository; it does not say whether an
   archive installed from a file (every export is one) is checked against the official
   repository's kept list of keys and builds, beyond the trust store's own revoked keys.

### Several repositories

- A device keeps a list: the repositories its image ships (a product names them in its
  profile, RFC 0024's new `repositories`), and those the user adds.
- **A manufacturer may make its own repository the default, or hide the official one,** but it
  is heavily discouraged ([decided](#decided-2026-11-01)). The profile field that hides it,
  `repositories.hideOfficial`, is documented as discouraged in the profile schema and in the
  product documentation. Hidden means off by default, never removed: Settings always keeps the
  official repository one switch away, and no profile can take that switch out.
- **The official publisher's key is built in (decided 2026-11-11).** `org.oscairn`'s key (ID
  `b284be6bea2fadb1`) is trusted for plugins, apps and content out of the box, as the official
  repository's root key is for repositories: listed by `trust.list` without being in
  `trust.json`, so a fresh install installs Oscairn's own packages without asking. Signatures
  are checked all the same. The user can revoke it, or remove it, which leaves an entry in
  `trust.json` trusting it for nothing; either takes the built-in entry's place. A profile that
  hides the official repository leaves the key out too. No other publisher's key is built in:
  the first plugin from another publisher still asks, whatever repository lists it.
- **Adding one** takes its address and its root key's fingerprint, typed, scanned from a QR
  code, or read from a small `.orepo` file. Nothing is added by a package or a web page.
- **Each installed package records its repository.** Updates and repair come from there, as
  RFC 0006 requires: Oscairn never searches other repositories for a package of the same name.
- **The same package ID in two repositories** is one package only when its publisher key is
  the same. Otherwise both are listed as different, with their repositories named.
- **A folder or USB drive** is a repository with the same layout. It may hold a whole
  repository, or another repository's signed root and index with only some blobs; a package
  whose blobs are not there is shown as "not on this drive".
- **Your own repository:** a desktop can make one from its own exports, signed with its device
  key (RFC 0006's "your own devices"), for a band or a studio to share.

## Installing in little space

An instrument may not have room, in storage or in memory, for a zipped package and its
unzipped contents at once (Jacob, 2026-11-01). So no install needs the whole archive in the
instrument's storage or memory. **Peak storage is the installed size plus one entry,** not
twice the package.

### Unpacked package folders

A package can also come as a **folder**: the same tree as inside the archive, with
`library.json`, `library.sig` and every file the manifest lists by hash. It might be on a USB
drive or a desktop's share. This is a second form of the
[library archive format](../library-archive.md), not a new format:

- It is checked by the same rules as an archive: the manifest's signature, every path rule,
  the limits, and every file's hash. A file the manifest does not list is refused, as in an
  archive. Hidden files and folders (a name starting with `.`, such as `.DS_Store`) are skipped,
  since desktops add them.
- It is not the same as a folder repository ([above](#several-repositories)), which holds blobs
  named by hash. A folder package is what a person gets by unzipping an `.olib`.
- Each file is hashed **as it is copied** into place, never checked first and copied after: a
  share or a drive can change between the two. A move instead of a copy is offered only within
  one filesystem, and the moved file is hashed before it is used.

### Streamed installs

Every install writes entry by entry:

1. Read the manifest and its signature, and check them. Check free space for the installed
   size (the index's `installed` figure, or the sizes the archive or folder gives) plus the
   largest entry, and stop before writing anything if it does not fit.
2. For each entry needed, read or fetch it, decompress it as it arrives, hash it as it is
   written into a staging folder in the data folder, and compare. Memory use is a fixed buffer,
   not the entry's size.
3. When an item, a sample or a plugin build is whole and verified, move it into place by a
   rename in the same filesystem. Nothing is published half-written.
4. On a bad hash, a pulled drive or a dropped connection, remove what this install staged and
   leave what was there before, as reading an archive does now.

Sources:

- **From a repository,** this is the per-blob fetch ([package files](#package-files)): one blob
  at a time, each resumable. A blob that is itself a zip (an item folder, a plugin build) is
  unzipped as it streams, so it is never stored zipped beside its contents.
- **An `.olib` on a USB drive or a share** is read in place, without copying it to the
  instrument first. A zip's directory is at its end, so a seekable file is enough to find and
  decompress each entry straight into staging. A ready-made
  [archive per platform](#ready-made-archives-per-platform) is one.
- **A folder package** is copied file by file, as above.
- **An `.olib` already in the instrument's own storage** costs its own size as well. The
  device can install from it, but offers to remove it after; downloads Oscairn makes never land
  as a whole archive (one a person downloads by hand is theirs to put there).

Today, reading an archive extracts every entry into a new folder before installing from it
([format](../library-archive.md#reading-one)). Streaming replaces that for packages.

## The index

### What it holds

The index lists every package and version, with what a device needs to decide whether to show
it, whether it fits, and what to download. It does not hold long descriptions or media; those
are in `details/`, fetched when a package's page opens.

```json
{
  "format": "oscairn.repository-index",
  "version": 1,
  "repository": "org.oscairn.main",
  "serial": 1842,
  "packages": [
    {
      "id": "com.example.organ",
      "name": "Example Organ",
      "summary": "A tonewheel organ and rotary speaker",
      "publisher": { "id": "com.example", "name": "Example Audio", "key": "3f9a1c0d2e4b5a67" },
      "license": "GPL-3.0-or-later",
      "tags": ["organ", "keys"],
      "details": "sha256-…",
      "versions": [
        {
          "version": "2.1.0",
          "manifest": { "sha256": "…", "size": 18342 },
          "packageSignature": true,
          "requires": { "oscairn": "1.4", "packages": [{ "id": "com.example.dsp-runtime", "version": "2" }] },
          "types": { "oscairn.sound": 64, "oscairn.performance": 12, "clap": 2 },
          "platforms": {
            "linux-arm64": { "size": 9821004, "installed": 24117248 },
            "macos-arm64": { "size": 11003920, "installed": 27262976 }
          },
          "common": { "size": 412881, "installed": 1048576 },
          "estimates": { "org.example.keys61": { "memory": 96, "cpu": 0.12 } },
          "yanked": null
        }
      ]
    }
  ]
}
```

- **Identity:** the package ID (reverse-DNS, under its publisher's ID) and version, and the
  manifest's hash, which is the identity a lock records (RFC 0006's package identity).
- **`types`:** how many items of each type the package holds, by RFC 0023 type ID, and its
  plugins and apps by format, so a search by type needs no download.
- **`platforms`:** download and installed size per platform, apart from what every platform
  shares (`common`). A device shows its own platform's figures only.
- **`estimates`:** memory and CPU per device profile where measured (RFC 0025 step 5), from
  the publisher or the repository's checks. They decide "fits this device" before anything is
  downloaded; admission still measures on the device.
- **`license`:** an SPDX expression, or `LicenseRef-` and the package's own license file for
  anything else.
- **`publisher.key`:** the publisher's key fingerprint. The manifest must be signed by it.
- **Names and summaries** are in American English; translations of them may be added per
  locale (`names`, `summaries`, keyed by locale, RFC 0029).

### Size

The index must be small enough for an instrument to download over a slow link, store and
search:

- one record per package, a few hundred bytes compressed per version;
- the target is under 2 MB compressed for 5,000 packages with their versions;
- the device reads it into the library's database (RFC 0023 step 5), so search is local and
  works offline;
- if it grows past that, versions older than the newest few move into a per-package history
  file named in the record, fetched when an older version is asked for (a lock asking for it,
  or "show all versions").

### Incremental updates

- Each change to the repository makes a new index with a higher **serial**. The index builder
  writes a delta from the index published last (`--previous-site`, after checking that root's
  signature): `index/<sha256>.delta.gz`, `{format: "oscairn.repository-delta", version: 1,
  repository, from, serial, packages, removed}`, named in the root's `deltas` as `{from, path,
  sha256, size}`. A device keeps the index it read last and applies the one delta from its
  serial; any other gap fetches the whole index.
- The root names the full index and the deltas from recent serials to the current one. A
  delta lists the package records that changed, each whole.
- A device that has serial 1830 fetches the root, then the delta from 1830 if the root lists
  one, else the full index. A typical refresh is the root and a few kilobytes.

### Signatures and freshness

- **The root is signed** by the repository's **index key**. The root also lists the
  repository's **root key**, which signs the index key and is kept offline. A stolen index key
  is replaced by a new one signed by the root key, and the old one is revoked (as TUF does).
  Ed25519, as in [RFC 0006](0006-packages-and-portability.md#signatures), over
  `oscairn-index-1\0` and the signed file's exact bytes: `repository.json` for the root (its
  signatures in `repository.sig`, a list of `{key, signature}`), and the index key's 64 hex
  digits and a newline for the index key. `oscairn-sign file` signs either and
  `oscairn-sign verify-file` checks it.
- **The root's fields:** `format` (`oscairn.repository`), `version` (1), `repository`, `name`,
  `serial`, `rootKey`, `indexKey` (`{key, signature}`, the root key's signature over it),
  `index` and `revocations` (each `{path, sha256, size}`), `deltas` (each `{from, path, sha256,
  size}`), `blobBase` and `mirrors`. Until step
  1 stores a release in pieces, each version in the index also names its whole archive
  (`archive`, `{sha256, size}`), a blob like any other.
- Everything else is checked by its hash in the root.
- **Rollback:** a device refuses a root with a lower serial than one it has seen, so a mirror
  cannot hand it an older index with a version that has since been revoked.
- **An address is the repository recorded for it (built 2026-11-11).** Each repository a device
  knows (the official one, built in; a device image's; the user's, in `repositories.json`) is an
  address and a root key, and its address is read only with that key. Until then a listing or an
  install took a root signed by any key trusted for `repositories`, so whoever served at one
  trusted repository's address could present another trusted repository's root and listing there
  (`control-repository` showed it listed and installed). A root with another key is now refused
  before anything of it is checked further or kept, with both keys' IDs: "its root key is …, not
  the one this device has for it (…); nothing from it was used". The snapshot that comes with the
  app is read only with a shipped repository's key ([preinstalled packages](#preinstalled-packages)).
  Mirrors change nothing: the root is fetched from the repository's own address, and the index and
  blobs a mirror serves are checked by the hashes in that root. A folder or an address not added
  yet (Settings reads one before asking to trust its key) may have any key trusted for
  `repositories`; a folder copy is then named and recorded as the known repository with its key.
  This RFC has no rotation of the root key, only of the index key, which the root key signs: a
  repository with a new root key is a new repository to the device, trusted by its new fingerprint
  through a system image or by the user removing and adding it again
  ([keys](#keys-1)). So the rule stays strict. `control-repository`, `control-preinstall`.
- **Freezing:** a stale mirror can keep serving an old, valid root. Without a clock a device
  cannot tell. It fetches the root from the repository's own address when it can and blobs
  from mirrors; with a clock it shows the root's age.

## Publishing

### The command

`oscairn publish`, part of the `oscairn` tool ([architecture](../architecture.md#building-and-sharing-plugins)):

1. Packs the release into a library archive, or takes one already made.
2. Runs the checks below locally, so a publisher sees failures before submitting.
3. Signs: each plugin build and app (a build signature) and the archive (the package
   signature), with the publisher's key (`oscairn-sign`, RFC 0006). Items may carry their own
   signatures too. Signing stays on the publisher's machine; the key never leaves it.
4. Submits.

**Publishing to a repository requires a package signature.** That, and signing each component,
are the owner's decisions of 2026-11-01, written into
[RFC 0006's signatures](0006-packages-and-portability.md#signatures); this RFC does not restate
them. An unsigned archive can still be shared by hand and installed under RFC 0006's policy.

### Submitting, with no server

For the first version, the official repository takes submissions as **pull requests** to a Git
repository of submission records:

- `packages/<package>/<version>.json` holds where the signed archive is (a URL, often the
  publisher's own release page, or the archive beside it) and its hash, with the index's
  summary, license, tags, channel and claimed platforms.
- **A publisher's first submission** also adds `publishers/<publisher>.json`: the publisher's
  ID, name and key. A reverse-DNS publisher ID is verified by a file at
  `https://<domain>/.well-known/oscairn-publisher.json` naming the key, as Flathub verifies
  app IDs: `{"format": "oscairn.publisher-verification", "version": 1, "id", "name", "key"}`,
  the key as 64 hex digits, the domain the ID's reversed (`org.oscairn` at `oscairn.org`). Only
  that file needs the ID's own domain; the repository itself is served from anywhere (Oscairn's
  official one from `packages.oscairn.org`, decided 2026-11-07). Later submissions must be signed
  by that key or one it signed.
- **CI** fetches the files, checks every hash against the signed manifest, runs the checks, and
  posts the results on the pull request.
- **On merge**, CI copies the blobs into storage (unless the package uses its own blob base),
  writes the new index and root, signs the root with the index key, and uploads. Merges are
  serialized, so serials never collide.
- A template for the repository and its CI is kept in
  [tools/package-repository/template](../../tools/package-repository/template/README.md)
  ([hosting](#hosting-and-cost)).

A third party running its own repository uses the same tool: `oscairn repo build` turns a
folder of signed archives into a repository, ready to upload anywhere.

### Checks

| Check | What it proves |
| --- | --- |
| Schema | The manifest, item documents and type manifests are valid (RFC 0023) |
| Archive safety | Every rule for reading an untrusted archive ([format](../library-archive.md#reading-one)) |
| Libraries | No plugin build loads a library from outside the platform's system libraries and its own bundle, read from its Mach-O and ELF files ([rules](../library-archive.md#plugin-packages); built 2026-11-07, `package-repository-index`, `control-libraries`) |
| Signatures | The package signature and every build signature verify with the publisher's key |
| Publisher | Every release of a package names the same publisher, and no publisher's ID is another's followed by a `.` and more: otherwise the owner of `io.github` could publish a newer `io.github.alice.synth`, signed with its own key, and take the package over |
| Platforms | Each platform the package claims has its build; a first-party package has one for every supported profile ([RFC 0006](0006-packages-and-portability.md#platform-builds-and-hardware)) |
| Loading | Each plugin scans, instantiates, saves and restores state and processes silence in a sandboxed worker, per platform (Linux ARM64 under QEMU), with a timeout and memory limit; the measurements become estimates |
| Validation | The developer kit's conformance and realtime-safety tests, as warnings at first |
| Dependencies | Every required package and version is in this repository (built 2026-11-07 for asset libraries: each a plugin package names is an asset package here at that version or later, and the record names what its archive needs; `package-repository-index`) |
| Scanning | Files against a malware scanner's signatures (limits below) |
| Reproducibility | Where a package names its source and builder image, CI rebuilds it and compares, and the index says whether it matched (F-Droid's model; the builder is reproducible already) |

**Review:** data-only updates from a known publisher are merged once the checks pass. A new
publisher, and any package newly bringing plugin or app code, also needs a maintainer's review
([decided](#decided-2026-11-01)).

### Versions and yanking

- **A version is immutable.** Once published, the same package ID and version always mean the
  same manifest hash. A fix is a new version.
- **Yanking** marks a version with a reason. It is not offered for new installs or updates, and
  is shown as yanked where installed, but a lock that names it still fetches it, so a
  Performance that uses it keeps working. A publisher can yank their own versions.
- **Withdrawing** is the repository's, through its revocation list: a withdrawn version is never
  installed, even when a lock names it, and where it is installed the user is told
  ([the difference](#the-revocation-list)). Only maintainers withdraw.
- **Removal** of files happens only through a takedown (below), never as a publisher's routine
  act, as npm restricts unpublishing.

## Discovery

### In the Library

The Library's **Install…** offers exactly two things (decided 2026-11-07): **Catalog** and
**File**. File installs an archive (`.olib`) chosen from the file browser, as the Library always
has, including a ready-made archive a person downloaded from the repository
([per platform](#ready-made-archives-per-platform)). Catalog opens the catalog on the kind of
list shown: the enabled repositories' packages
holding items of that type (Performances, Sounds) or plugins, what is installed marked so, searched
like the rest, from each repository's index as `packages.list` reads it. Opening one shows its
page from the Catalog app's action (below). Opening a repository by its address or folder is not
on Install…; it belongs with the repositories in Settings.

**Interim, until the Catalog app exists (built 2026-11-07):** the Catalog is a view in the
desktop Library, labeled Catalog. Its rows name each package, its version, publisher and
summary; what this device cannot install is grayed with the reason. What is installed says
**Installed** with the version installed, or **Update to** the newest stable version this device
can install when that is newer. A plugin package whose plugins all come with the app, and
not installed from a repository, says **Built in**, with nothing to install, or **Update to** a
newer version the repository has, which installs beside the app's, as plugins install side by
side ([updates](#updates)); nothing changes on its own. Installed means the repository's build (decided 2026-11-11): the
same ID and version installed with different content (a user's own build that kept the ID, or
one installed some other way), told by the archive's SHA-256 recorded at install, says
**Installed (a different build)**, and its details say the installed build does not match this
repository's. It offers **Install the repository's build**, never on its own, which replaces it
after a confirmation naming both. An install recorded before the SHA-256 was says Installed, as
nothing tells its build. A package's details are its summary, version, the version
installed, license, publisher and key fingerprint, repository, download and installed size, and
the packages it needs, with Install (or Update; neither when what is installed is current), which
runs the install job and its questions. A
plugin whose code is not trusted here asks first, by what the refusal tells
apart. Signed by the key the repository's signed index lists as the publisher's: **Trust
<publisher> to install plugins?**, naming the publisher, the repository that lists the key and
its fingerprint, with **Trust and install** (the key trusted for plugins, then the install tried
again) or Cancel. Signed by a key no one vouches for: **Trust this key**, **Install anyway** this
once, or Cancel; unsigned: **Install anyway** or Cancel; both saying that a plugin runs as a
program with the user's access. Nothing else is trusted unless the user says so; the official
publisher's key is built in ([several repositories](#several-repositories)), so Oscairn's own
packages do not ask. No Show all yet, and no audio previews or screenshots
(`details/` is not read yet). The Catalog app replaces the view and keeps its behavior.

### The Catalog app

- An app (RFC 0022), `oscairn.catalog`, in a package, replaceable like any other. It offers
  `oscairn.open.package` (open a package's page) and `oscairn.install.package`, which the
  Library, Settings' Installed content and the missing-content repair flow use.
- A tile on Home, and a card when there are updates (RFC 0030).
- Browse by type (plugins, Sounds, Performances, sample libraries, apps, themes,
  translations), by tag and by publisher; search over names, summaries and tags.
- A package's page: its details, audio previews and screenshots (fetched on demand), its
  versions, size on this device, license, publisher and fingerprint, signatures, whether its
  build was reproduced, and what it needs.
- **Install** shows RFC 0006's one import plan: what will be installed, from whom, its size,
  and what runs code.

### Compatibility

Filtered by the device's profile (RFC 0024) by default, with **Show all** beside it:

- a build for the device's `platform`, or no code at all;
- the system version the package requires (below);
- estimates within the device's budget (RFC 0025);
- for apps, a surface the device has (RFC 0030's `surfaces`);
- for items, a type the device knows or that the package brings.

What does not fit is shown grayed with the reason, never hidden without saying so.

### Control API

A `packages` namespace: `packages.repositories`, `packages.addRepository`,
`packages.removeRepository`, `packages.enableRepository` (on or off), `packages.setDeviceRepositories`
(the app's, from its profile), `packages.refresh`, `packages.search`, `packages.get`,
`packages.install` and `packages.updates`, with notifications for progress. Searching and
installing content need the Library permission (RFC 0026); adding a repository or installing
plugin and app code needs System.

## Updates

- **Refreshing** the index is a network fetch, off the audio path, by hand or on a schedule the
  musician sets (off by default on instruments). It never installs anything.
- **Plugins and content** install side by side with the version in use (RFC 0006). A
  Performance's lock never moves on its own. Updating one is explicit: **Use version 2.1** on
  a Performance, or **Update Performances using this** from the package, listing each one with
  what changes. Rehearsed Setlists keep their pinned revisions.
- **Libraries:** a new version of a locked library replaces its items with their new
  revisions; Performances keep working, since their parts are copies (RFC 0021). Items imported
  into the musician's own storage are untouched.
- **Apps, themes and translations** are not locked by documents. An update replaces the
  version installed, keeping the previous one until the new one has started once, and
  applies only when the app is closed. Per device: ask (the default), or update automatically.
  Never while playing.
- **System images** ([RFC 0028](0028-system-image-updates-boot-and-power.md)) are not packages.
  They have their own signing and are offered apart, though they may be hosted beside a
  repository. A system update does not change installed packages. A package declares the
  oldest system it runs on (`requires.oscairn`); one needing a newer system is shown with
  "needs system update". System images also carry trust store entries and revocations, so an
  offline device learns them with its next image (RFC 0006).

## Offline and several devices

- **A desktop fetches for a device.** Choose a device (paired, or from an inventory it exported,
  RFC 0006), browse with that device's profile as the filter, and queue packages. They are
  delivered the next time the two are paired, or written to a USB drive. The desktop downloads
  only that device's platform builds.
- **USB:** the drive carries the repository's signed root and index and only the queued
  blobs. The device checks the root's signature and every hash, as from the network.
  RFC 0006's missing-content request is fulfilled the same way.
- **A paired computer as a source** (RFC 0026): with the Library permission, a device fetches
  the index and blobs through the paired connection, from the computer's cache or through the
  computer's internet. The computer is a mirror: the device still checks every signature
  and hash itself.
- **Device to device:** one paired instrument can serve another its cached blobs the same way.
- **By hand:** a person can download a version's ready-made
  [archive for the device's platform](#ready-made-archives-per-platform) in a browser and carry it
  there. It installs by the archive path, with the same checks, and needs no repository on the
  drive.

## Preinstalled packages

Decided by the owner 2026-11-11: the distributed macOS and Linux builds do not come blank. They
ship the published, signed packages of Oscairn's own instruments and effects (FM, Analog, the
Sampler, Effects, the Arpeggiator) with their asset libraries (`org.oscairn.fm-cartridges`,
`org.oscairn.analog-factory`), and later a starter library of Performances. One mechanism, no
"built in" kind: preinstalled packages are ordinary packages, recorded in `packages.json` like
any install, shown as Installed in the Catalog, updated the normal way; a hardware image preloads
the same way. Nothing installs or updates on its own after that; the first-run install of what
ships in the box is the one exception, and it is local. Built 2026-11-11 except as
[Progress](#progress) step 14 says.

### What ships

A **repository snapshot**, as a [USB drive](#offline-and-several-devices) carries one: the
official repository's `repository.json`, `repository.sig`, index, revocations and deltas exactly
as `packages.oscairn.org` published them at release time, and under `blobs/<ab>/<sha256>` only the
archives of the packages the release preinstalls. It is the whole signed index with a subset of
the blobs, not a signed subset: the release script does not hold the index key, and need not.
`tools/preinstalled.txt` lists the packages and versions; `tools/preinstall-fetch.py` downloads
them, checks the root against the official root key (the index key it signed, and the index key's
signature over the root, with `oscairn-sign verify-file`), the index by the root's hash, each
archive by the index's size and SHA-256, its manifest by the index's hash, and every signature in
it with `oscairn-sign verify`, signed throughout by the publisher key the index names; that each
version is stable, not yanked and has a build for the platform; and that each asset library a
listed package needs is listed at the version a device would pick. `tools/release-macos.sh` runs
it into `Oscairn.app/Contents/Resources/Preinstalled` and fails if anything does not verify; on
Linux the folder is `Preinstalled/` beside the programs. The archives are not in the Git
repository.

The 2026-11-11 set, at serial 28: FM 0.4.8, Analog 0.4.7, the Sampler 0.6.3, Effects 0.1.1, the
Arpeggiator 0.1.4, `fm-cartridges` 1.0.0 and `analog-factory` 1.0.0, 11.8 MB of archives
(`analog-factory` alone 7.4 MB).

**Known gaps** (owner, 2026-11-11):

- **No Linux plugin builds yet.** Every plugin package in the official repository has a
  `macos-arm64` build only, so a Linux release would preinstall only the asset libraries: each
  plugin package is reported as not set up, as it cannot be installed here.
- **`analog-factory`'s source** is named as upstream Surge XT's repository
  (`github.com/surge-synthesizer/surge` at a commit), which the project does not control. The
  GPL's directions to the source in `Licenses/Packages/` hold only while that repository keeps
  the commit; a copy under the project's own account would not depend on it.

### When it installs, and how

At the engine's start (`AudioEngine`, so the separate engine and the in-process one alike), with
a snapshot folder there, `Session::preinstall` reads `preinstalled.json` in the data folder. If
this release (the version with its pre-release label) is listed, nothing happens. Otherwise, on
the control thread and one job at a time, so neither audio nor the UI waits:

1. A `packages.list` job reads the snapshot folder, as any repository is read: root, index key and
   index verified against a root key trusted for `repositories`, the serial checked against the
   one this device has seen and then kept.
2. Each package whose archive the snapshot holds is a candidate, at that version. One installed
   already, at any version, is **kept**: the user's wins, newer or older (owner, 2026-11-11), and
   updates the normal way (a newer one is never joined by an older one beside it, as plugin versions install side
   by side). One that `preinstalled.json` lists from an earlier release and is no longer installed
   was **removed** by the user and stays removed.
3. Each other one is a `packages.install` job from the folder at that version: the archive read in
   place, its size and SHA-256 checked against the index, its manifest against the index, every
   signature in it, its publisher's key, and its plugin code judged against the trust store as
   always; its asset libraries come from the snapshot too. An asset library already brought by an
   earlier package counts as installed.
4. Each package installed or kept is added to `preinstalled.json` with the release, at once, so a
   run cut short is not repeated for what it did; the release is added when all have been tried.

`packages.preinstalled` says how it goes: `running` or `done`, the job running (so `packages.job`
shows its progress), and each package's state and error. The engine moves it on at each service
tick, and asking moves it on too. While another client's package job runs, it waits. The desktop
Library's status line says "Setting up the instruments that come with Oscairn…" meanwhile, and
the error, if any, after.

**Failures** leave the app usable and say why, per package: a damaged archive, a full disk or code
from an untrusted key fails that package (and a plugin package whose asset library failed), and
the others still install. Each reason reads "<name> was not set up: ...": refused code reads
"<name> was not set up: its signature is not from a publisher this device trusts", not the
Catalog's offer to accept unsigned code, as nobody asked; the run's error ends "The Library's
Catalog can install it." A failed package is not recorded, so the next release tries it again; it
is not retried at every start (a damaged file in the app would fail each time), and the Catalog
installs it meanwhile. A snapshot that cannot be read at all (an untrusted or revoked root key, a
broken signature) fails the whole run, recorded as done for this release.

**Recorded as the official repository.** An install from a folder whose verified root has the root
key of a repository this device ships (the official one, or a device image's) records that
repository's address in `packages.json`, not the folder's path, because that is where updates and
repair come from (and the same holds for a USB copy of it).

### Trust and rollback

Nothing is weakened for it. The snapshot is checked by the same code as a network install, against
the same trust store: the official root key is built in for `repositories`, so the root verifies.
Since 2026-11-11 the snapshot is read only with a shipped repository's root key (the official one,
or one the image's profile names): a snapshot signed by a key the user trusted for a repository
they added is refused, naming both keys ([an address is the repository recorded for
it](#signatures-and-freshness); `control-preinstall`).
Plugin code is judged as any install's is, against keys trusted for `plugins`; the official root
key is never trusted for plugins. The `org.oscairn` publisher key (ID `b284be6bea2fadb1`, public
key `db87e97d…0d69`) is built in for plugins, apps and content (`builtInKeys()` in `trust.cpp`),
except under a profile that hides the official repository, so the official packages install with
no question. Preinstall never passes `acceptUnsignedCode`. The engine starts preinstalling before
the app sets the profile's repositories (`setDeviceRepositories`), so a profile hiding the official
repository would still have its publisher trusted for the first packages; the desktop's profile does
not hide it, and an image that does must set its repositories before the engine preinstalls.

The **serial** is the one rollback rule there is. On a new data folder the snapshot's serial is the
first seen, and the live repository, at that serial or later, is accepted after it. A data folder
that has already seen the live repository at a higher serial (an earlier release's user who opened
the Catalog after this release's snapshot was taken) refuses the older snapshot like any older
root: nothing is preinstalled, the reason is reported, and the run is recorded so it does not
repeat. That user has the Catalog, online. `repository::open` keeps the index it read before
`see()` checks the serial; it no longer writes an older root's index over a newer one kept under
`Repositories/<root key>/`, so refusing the snapshot leaves the device's newer index in place.

Preinstall only calls `listRepository` and `installFromRepository`, so it follows the state keyed
by root key and repository ID ([above](#kept-state-keyed-by-the-root-key)); the change inside
`installFromRepository` compares the verified root key with the shipped repositories' keys.

### A hardware image

Not built. An image (RFC 0028, RFC 0035) puts the same snapshot in its read-only system partition,
beside the engine, and the engine installs it into the data partition at the first start, so the
packages stay updatable and removable like any. A device image whose profile names its own
repository (`repositories`) ships that repository's snapshot instead, or both in turn; its root
key is trusted once the app has set the device's repositories from its profile, so the image's
engine must have them before it preinstalls (today the app sets them after the engine starts:
to be ordered when the image is built). A factory reset that clears the user's data clears
`preinstalled.json` with it, so the packages come back; one that keeps the data keeps what the
user removed removed. The image's Licenses screen lists the packages' licenses from the same
`Licenses/Packages/` the dmg carries.

### The starter library

The starter library of Performances will be a library archive in the same snapshot, installed as
a locked library like any. Its Performances use FM and Analog, so it needs to say so: a library
archive cannot declare the packages it depends on today. The smallest addition is specified, not
built, in [the library archive's package dependencies](../library-archive.md#package-dependencies).

## Finding missing plugins

Proposed 2026-11-11, from the owner's request of that day. Not built: [Progress](#progress)
steps 9 to 13.

When a library, a Performance or a Sound needs a plugin that has no build on this device, the
device looks the plugin up in the repositories it knows and offers to install it with what it
needs, or says why it cannot. Nothing is installed, no repository is added and no address a
document names is fetched until the user says so (owner, 2026-11-11: every install is the
user's explicit action).

### What happens today

Read in the code on 2026-11-11:

- A document names each plugin by format, ID and version, its path only a hint
  ([format](../performance-format.md)). A node loads the build at its path hint or the store's
  build of exactly that version; any other version is not taken. Otherwise it opens as a
  stand-in that keeps its place, ports, parameters and saved state, plays silence and writes the
  state back byte for byte on save (`resolvePlugin` and `standIn` in
  `core/control/src/performance.cpp`, `core/plugin-host/src/unavailable_plugin.cpp`). Its
  reason reads "`<id> <version>` is not here (not installed); its place and saved state are
  kept", or "(version 2.0.0 installed)" when only another version is here.
- `session.status`'s `failures` lists each failed plugin as `node`, `name` and `reason`, the
  reason as text ([control protocol](../control-protocol.md)). The desktop's Parts view shows the
  first: "`<name>` is silent: `<reason>`" (`hosts/standalone/PartsView.cpp`).
- `library.install`'s `notes` say, as text, which plugin has no build for this device ("`<name>
  <version>` has no build for this device in the archive", `performance.cpp`).
- A Performance's lock records each plugin build's digest per platform, but loading does not
  compare it: any build of the right version loads (`resolvePlugin`).
- Nothing looks the plugin up anywhere. The Catalog lists packages by kind
  (`hosts/standalone/Catalog.cpp`).

### The index must say which plugins a version holds

**It does not today.** The index builder writes each version's `types`, counts of items by type
and plugins by format, and `platforms` for the version as a whole (`facts()` in
`tools/package-repository/template/scripts/build-index.py`); the device reads the same
(`repository::Version` in `core/control/include/oscairn/repository.h`). A package's ID is not its
plugins' IDs: `org.oscairn.splitters` holds `org.oscairn.split` and `org.oscairn.join`, and
setBfree's package holds its organ at 0.4.4 and its Leslie at 0.2.1
([plugin packages](../library-archive.md#plugin-packages)). So a lookup by plugin cannot be
answered from today's index.

Each version in the index gains `plugins`, written from its manifest's `plugins`:

```json
"plugins": [
  {"format": "clap", "id": "org.oscairn.setbfree", "version": "0.4.4", "platforms": ["linux-arm64", "macos-arm64"]},
  {"format": "clap", "id": "org.oscairn.setbfree.leslie", "version": "0.2.1", "platforms": ["linux-arm64", "macos-arm64"]}
]
```

- **Platforms per plugin,** since the version's `platforms` is the union over its plugins: a
  package claims a platform only when every plugin has a build for it, but may carry builds for
  platforms it does not claim (`read_release` in `build-index.py`).
- **No build digests in the index.** Each would be 64 hex digits per plugin, platform and version,
  which do not compress, against the index's target of 2 MB for 5,000 packages
  ([size](#size)). The lock's digest is compared with the manifest when installing
  ([versions](#versions)).
- **About 100 bytes per plugin per version** before compression. Deltas carry it inside each
  package record, as now.
- **New checks in the index builder:** in one repository, a plugin's format and ID are published
  by one publisher, the first to publish it (another publisher's package carrying it is refused,
  as a package keeps its publisher now); and an ID that differs from a published one only in
  letter case is refused, as the plugin store's folders for the two would be one on a
  case-insensitive file system (as for libraries, [installing one](../library-archive.md#installing-one)).
  A plugin ID new to a package is already a human review ([proposal](#proposal)).
- Proposed with it, for [newer versions](#versions): each plugin's `opensStateFrom`, from the
  manifest, and `checked` when the repository's state check ran, when the owner accepts them
  ([RFC 0043](0043-moving-sounds-to-newer-plugin-versions.md#where-it-lives-and-how-it-is-signed)).
  Not `soundChanges`, which the device reads from the installed manifest.

### Kept state keyed by the root key

A prerequisite, found by reading the code while designing this. Until 2026-11-11
`repositories.json`'s `seen` serials and the index kept in `Repositories/` were keyed by the
repository ID that the root itself gives. `control-repository` showed what that let a repository
the user added do, its root claiming another's ID under its own root key: at the same serial, the
real repository's listing came from the impostor's kept index (its packages under the real one's
name); at a higher serial, the real one's root was refused as older than one seen.

Built 2026-11-11: both are keyed by the root key and the repository ID together, `seen` as
`{"<root key>": {"<repository>": serial}}` (`repositories.json` version 2, `see()` in
`core/control/src/packages.cpp`) and the kept index as `Repositories/<root key>/<repository>.index.json`
(`repository::open`, `core/control/src/repository.cpp`). The whole key, not its 16-digit
fingerprint, which a forger could match with about 2^64 tries. Not by address: a mirror serves the
same signed root, and must not start a rollback count of its own. So two repositories claiming
one ID are both usable, each with its own serials and index, told apart by address, key and name
in the device's list; none is refused for the ID it claims, as the device need not read a root to
add a repository. Version 1's serials are given, on the first root read after the change, to every
key then trusted for repositories, so none is lost; its kept indexes are dropped and read again
whole. `control-repository` tests both.

Still to do before a lookup reads kept indexes: the address of each known repository maps to its
root key but not to the ID its root gives, so a lookup with no network cannot yet find the kept
index for an address; and a root refused as older has already been kept by `repository::open`,
so a lookup with no network would read that older index until the next listing.

**Names (built 2026-11-11).** A repository is shown by this device's name for it, never the name
its root gives. Until then the desktop Catalog labeled each listing, and the install questions and
a job's errors named the repository, by the root's `name`, so a repository the user added could
call itself "Oscairn" there. Now `packages.job`'s `name` is the device's name for the repository
read (by the source's address, or for a folder by its verified root key) and the root's own is
`claimedName`, shown only in Settings' trust question, as a claim: "It calls itself "…", signed by
the root key whose fingerprint is …". The device's name for a repository the user adds is the name
its root claimed when the user trusted it, kept in `repositories.json`, not editable (nothing has
needed it). `packages.repositories` lists a name that reads like an earlier one's (letter case and
spaces aside) with ` (key <ID>)` after it, or ` (<address>)` when both have the same key; the
official one is first, so it keeps "Oscairn". `control-repository` tests it. Names alike in other
ways (a Cyrillic letter for a Latin one) are not caught; Settings shows each one's fingerprint.

### Where a device looks

The **wanted plugin** is its format, ID and version and this device's platform, with the
document's [hint](#the-repository-hint) if it has one, and the lock's digest for this platform if
it has one.

1. **The repositories that are on,** each from the index it read last, with no network. Today
   every listing reads the root first (`repository::open`), so a lookup from the kept index alone
   is new. A kept index was checked against a signed root when kept; the answer says which serial
   it read, as an instrument may have no clock to date it.
2. **Read again when the user asks** (**Look again**), or for a repository this device has never
   read: the root, then the delta or the index, as a listing does now.
3. **Not a repository turned off.** When the hint names one, the answer says it is off.
4. **The hint's repository,** as [below](#the-repository-hint).

**Matching** is the format and ID, byte for byte. Never by name, a similar ID, or letter case. A
plugin is in a version when that version's `plugins` lists it.

**Several repositories with it:** each is a candidate, named with its repository and publisher.
They are listed in this order: the hint's repository; the repository this plugin's other
installed versions came from (`packages.installed`), as updates and repair come from there; then
the official, the device's and the user's, in `packages.repositories`' order. When the candidates'
publisher keys differ, none is chosen for the user: each is shown with its publisher and key
fingerprint and the user picks one.

**Dependencies:** a plugin package's asset libraries, as the index names them
(`requires.packages`), come from the same repository; `packages.install` does that today
([asset packages](../library-archive.md#asset-packages)). The offer gives their sizes from the
index and says which are installed already.

### Versions

**The exact version first.** As the index gives it:

- Published, not yanked, with a build for this platform: offered.
- **Yanked:** offered with the publisher's reason and a second question. A lock still fetches a
  yanked version ([versions and yanking](#versions-and-yanking)), but a document, which anyone
  can write, should not choose one silently.
- **Revoked** (the version, its build or its publisher's key,
  [takedown and revocation](#takedown-and-revocation)): never offered. Revocation lists are
  applied since 2026-11-11 ([the revocation list](#the-revocation-list)).
- **No build for this platform:** "not available for linux-arm64" (`incompatible()` in
  `repository.cpp` says "it has no build for this device (linux-arm64)" now).
- **Needs a newer system:** "it needs a system update: Oscairn 1.4 or later", as `incompatible()`
  says now.

**The same build.** When the document's lock names this platform's digest for the plugin, the
install compares it with the build in the package's manifest, read before anything is installed
(the whole archive today, the manifest blob once releases are stored in pieces). Another digest
installs only after a question: the version is the same, but not the build the document was saved
with. A document with no lock entry for this platform (one a converter wrote) skips this.

**Newer versions.** What the formats and plugins promise today about a newer version restoring
an older version's state:

- **CLAP:** nothing. `clap_plugin_state` has `save` and `load` and says nothing about versions
  (`core/clap-host/third_party/clap/include/clap/ext/state.h`), and a plugin's version is "an
  arbitrary string" (`clap/plugin.h`).
- **Oscairn's documents:** no silent upgrades and no untested state conversions
  ([format](../performance-format.md)). RFC 0006 never picks "an arbitrary newer release"
  ([state portability](0006-packages-and-portability.md#state-portability-and-format-migration))
  and never upgrades to fill a gap in repair
  ([repairing](0006-packages-and-portability.md#repairing-missing-dependencies-on-offline-instruments)).
- **Oscairn's own plugins:** their snapshots are versioned (`oscairn-sampler-2`, the sound
  snapshots' draft 4), but until the first public release an incompatible change replaces a
  format outright and old saves are not migrated
  ([compatibility policy](../developer-guide.md#initial-development-compatibility-policy)).
- **Packages:** the manifest has no field saying which older states a version restores
  ([format](../library-archive.md)).

So nothing makes a newer version safe for a saved state, and this design assumes nothing. A newer
version is offered only where its publisher says so in the signed manifest: each plugin's
**`opensStateFrom`** (proposed under the name `restoresStateFrom` first), the oldest version of the
same format and ID whose saved state this one opens, with what sounds different in
`soundChanges`. [RFC 0043](0043-moving-sounds-to-newer-plugin-versions.md#the-publishers-claim)
defines both: what they promise (loads, the same ports, every setting kept, and the same sound
except as `soundChanges` says), how they are signed (the package signature over the manifest), that
a device honors them only when the old version came from the same publisher key, and what a
machine can check of them (CI opening older versions' states in the new build and comparing
renders) and cannot. The index carries `opensStateFrom` so this lookup can offer the newer version
before downloading; the device trusts the claim only from the installed archive. The newer version
installs side by side with any other (RFC 0006), and the part moves to it only by RFC 0043's
explicit **Use 0.4.6** (`parts.useVersion`), which opens the part with it and marks it edited: the
saved document is unchanged until the user saves, and saving over a locked library item makes a
new one ([control protocol](../control-protocol.md)). Without the claim, the offer names the
newer version and says it does not say it opens this state; the Catalog still installs it, and RFC
0043 still lets the user move one item to it after a warning. Whether to add the field at all is
the owner's ([open questions](#open-questions-on-finding-missing-plugins)).

### The repository hint

An archive, a Performance or a Sound may name, for each plugin it uses, the repository the plugin
is published in: its `https://` address and its root key. The field is `pluginRepositories`,
specified in the [library archive format](../library-archive.md#where-plugins-are-published) and
the [Performance format](../performance-format.md#where-plugins-are-published); not read by the
engine yet. On a device:

- **The key is the repository, the address only where to look.** A hint whose key is a known
  repository's is that repository, read at the address the device has, whatever address the hint
  gives.
- **A known address with another key:** the hint is not used, and the known repository's key is
  never changed by it.
- **An unknown key:** nothing is fetched from the address until the user asks. A fetch alone tells
  whoever runs the address that this document was opened, and from where.
- **Look there** reads the root and index at the hint's address and checks them with the hint's
  key, without trusting it: the root's `rootKey` must be the hint's key, the index key signed by
  it and the root by the index key, as for a trusted repository. Nothing is kept: not the key, the
  serial or the index. It answers whether that repository lists the plugin, at which versions and
  from which publisher.
- **Add it** only then: `packages.addRepository` with the hint's address and key, after the same
  confirmation by fingerprint as Settings' **Add a repository...**, naming the repository by the
  name in its signed root, never by anything the document says. It trusts the key for
  `repositories` only.
- A hint the format refuses (an `http://` address, a folder, user info or a query in the address,
  a malformed key) refuses the document as any malformed field does, so it never reaches a lookup.

### Trust

- **A hint vouches for nothing.** Adding its repository is the user's choice, by fingerprint, and
  trusts it for its listing only, as any repository (RFC 0006's `repositories` purpose).
- **Plugin code** installs as from the Catalog now: the archive must be the release the index
  names, signed by the publisher key the index names, and a plugin from a publisher not trusted
  for plugins asks first: **Trust** the publisher, **Install anyway** this once, or Cancel
  ([in the Library](#in-the-library)).
- **The same plugin from another publisher:** when this plugin's format and ID are installed here
  from a publisher, or published by a publisher this device trusts for plugins in a repository it
  knows, and a candidate's publisher is another, the offer says so and chooses nothing.

### At each trigger

The prompts, in plain words. Names, versions and sizes are examples.

**Installing a library** (the Library's Install..., from File or the Catalog). When the install
is done, if its items use plugins this device does not have, one window for all of them:

> **Plugins these Sounds need**
>
> 12 Sounds in Vintage Keys use plugins that aren't installed.
>
> **Example Organ 1.2.0**: in Oscairn Packages, from Example Audio (3f9a 1c0d 2e4b 5a67).
> 9.8 MB, and Example Tonewheels 1.0, 40 MB.
>
> **Old Strings 3.0**: not available for linux-arm64.
>
> **Bridge 1.0**: not in any repository you use.
>
> **Example Pad 2.0**: the library says it's at packages.example.com, a repository you don't
> use. **Look there**
>
> **Install selected** · **Not now**

Each row the device can install has a checkbox, on; the others say why and have none.
**Not now** installs nothing; the items open with stand-ins, as now. **Look there**, once the
repository is read:

> **Add packages.example.com?**
>
> It calls itself Example Repository. Its key is 0b2e 28f9 0bb4 6adf (the full key under it).
> It has Example Pad 2.0 from Example Audio.
>
> Add a repository only if you trust whoever runs it. Adding it doesn't trust its plugins: you're
> asked about each new publisher.
>
> **Add and install** · **Cancel**

When the read does not hold: "packages.example.com didn't answer as the library says: its key
isn't the one the library names. It wasn't added."

**Opening a Performance** never shows a window, asks or fetches, at any level. Its stand-ins say
what the lookup found in the Parts view (below). With more than one, the Parts view's line says
"3 plugins aren't installed" with **Show**, which lists them as the library's window does.

**A stand-in in the Parts view.** The part's line, from the lookup of its plugin (each line
starts "Example Organ 1.2.0 isn't installed, so this part is silent." and ends "Its settings are
kept."):

| What the lookup found | The rest of the line |
| --- | --- |
| The exact version | It's in Oscairn Packages. **Install** |
| The exact version, yanked | It's in Oscairn Packages, but its publisher withdrew it: "reason". **Install anyway** |
| Another build of the version (found when installing) | This is version 1.2.0, but not the build the Performance was saved with. It may sound different. **Install** · **Cancel** |
| Only a newer version that restores its state (step 13) | Version 2.1.0 is published and says it opens sounds saved with 1.2.0. **Install and use 2.1.0** |
| Only a newer version, no such claim | Version 2.1.0 is published, but it doesn't say it can open sounds saved with 1.2.0. |
| No build here | It's published, but not for linux-arm64. |
| A newer system needed | It needs a system update: Oscairn 1.4 or later. |
| Revoked | It was revoked: "reason". |
| Not found | It isn't in any repository you use. |
| A repository never read | Oscairn Packages hasn't been read on this device yet. **Look again** |
| Offline, found in the last refresh | It's in Oscairn Packages. Connect to the internet to install it, or install it from a USB drive. |
| The hint's repository unknown | The Performance says it's at packages.example.com, a repository you don't use. **Look there** |
| The hint's key not the known one | The Performance names packages.example.com with a different key than yours, so it wasn't used. |
| The hint's repository turned off | It may be in packages.example.com, which is turned off in Settings. |

After an install: "Example Organ 1.2.0 is installed. **Reload the Performance**". Reloading is a
seamless open of the same Performance (RFC 0021); the stand-in is never replaced on its own.

**The Setlist preparing ahead** never looks up, fetches or asks. An entry whose prepared graph
has stand-ins reads "1 plugin isn't installed" in the list. Before the show, the Setlist's
**Check** lists every entry's missing plugins; **Find them** asks the Catalog
(`oscairn.install.plugins`, below), which shows the library's window for them. The Setlist app
has `play` and `library`, not `system` (`hosts/standalone/apps/setlist/app.json`), and needs no
more: `performance.prepare` and `library.query` are `library`'s, and its `asks` gains the action.

### Offline, playing, and reliability levels

- **No network is normal.** A lookup answers from the kept indexes and says which refresh it
  read; a repository never read says so. Never an error window: the stand-in's line and the
  library's window say it.
- **Time unknown:** a device fetches nothing from the web until its time is set (decided
  2026-11-08, [above](#build-order-and-gates)); the line says "Set the date and time to install
  from the internet." A folder or USB repository still installs.
- **An instrument that never goes online** carries the wanted plugins to a computer in RFC
  0006's missing-content request (not built); a lookup's candidates add the repository and
  package to it.
- **Opening, preparing and changing Performances** never look up, fetch, ask or wait for a
  lookup, at every level. A lookup runs as a job, off the control thread, as listings do.
- **At `instrument` and `pass-through`** ([RFC 0027](0027-reliability-levels.md)): an install
  is refused while the transport runs ("Stop the transport to install"). On every level, a
  plugin installed while a Performance is open replaces its stand-in only at **Reload the
  Performance**. Whether install jobs stay off the cores reserved for audio is to check once RFC
  0027 step 1 reserves them.

### Control API additions

All under `system`, with the rest of `packages.*` (`appGrants()` in
`core/control/src/json_rpc.cpp`); an app without it asks the Catalog app. No method takes a file
path.

- **`packages.lookup`** (new) takes optional `plugins`, `refresh` and `look`, and returns
  `{job}`; one job runs at a time, as now (`busy`).
  - `plugins`: each `{format,id,version}` and optional `repository` (`{address,key}`, a hint as a
    document gives it) and `sha256` (the lock's digest for this platform). Absent: the open
    Performance's stand-ins, with its hints and lock. At most 4096; a malformed entry is
    `-32602`.
  - `refresh`: read each repository's root first (the network); absent or false, the kept
    indexes only.
  - `look`: addresses of hints the user chose to read; each must be one of `plugins`' hints.
  - `packages.job` then gives `lookup`: per wanted plugin `{format,id,version,status,reason,
    candidates,hint}`. `status` is `available`, `newer`, `unavailable` (`reason`: no build for
    the platform, a system update, revoked), `notFound` or `notRead`. Each candidate is
    `{repository,repositoryName,serial,package,version,channel,yanked,publisher,publisherKey,
    publisherTrusted,incompatible,size,installed,dependencies,opensStateFrom,checked}`, each dependency
    `{id,version,size,installed}`. `hint` is `null` or `{address,key,state}`, `state` one of
    `known`, `off`, `unknown`, `read` (by `look`), `mismatch` (a known address, another key) or
    `refused` (read by `look`, and its root's key is not the hint's).
- **`packages.install`** installs a candidate as now (`source` its repository's address, `id` its
  package, `version`). New: optional `locked`, each `{format,id,version,sha256}`; a build whose
  digest is not the locked one refuses the install, listing each, unless `acceptOtherBuild` is
  true. New refusal `playing`: at `instrument` and above while the transport runs.
- **`packages.addRepository`** unchanged; the hint's flow calls it after the user's yes.
- **`session.status`'s `failures`:** a stand-in's entry gains `plugin` (`{format,id,version}`)
  and `repository` (the document's hint for it, or `null`).
- **`library.install`** returns `missing`: each plugin its items use that has no build here
  after the install, `{format,id,version,repository,items}`, `items` the IDs of those using it.
- **`performance.prepare`** returns `missing` the same way, without `items`.
- **`library.query`'s items** gain `missing`, for the Setlist's check: the plugins an item names
  that are not installed here. (`plugins.list`'s `usedBy` shows the library knows each item's
  plugins.)
- **`oscairn.install.plugins`** (new app action, RFC 0022), offered by the Catalog app: takes
  `{plugins}` as `packages.lookup` does and shows the library's window for them. Until the Catalog
  app exists, the desktop's Catalog view answers it.

### What the stand-in reports

The engine's reason for a stand-in does not change: it is made on the load path, which never
looks anything up. What changes is that `failures` carries the plugin and its hint, so a client
with `system` looks it up and shows the line [above](#at-each-trigger). A client without
`system` (an app granted `edit`) shows the reason as now. The stand-in keeps its state as now.

### Security

| Threat | What stops it | What remains |
| --- | --- | --- |
| A hostile archive or Performance names a hostile repository | Nothing is fetched from it until the user asks; it is added only after a confirmation by fingerprint that names it by its signed root, not the document; adding trusts it for its listing only; its plugins still need the publisher question, and are to run in workers at `instrument` and above (RFC 0027, not built) | A user who adds the repository and then trusts its publisher installs what it lists, as with **Install anyway** on a file today. Revocation (step 4) is the backstop |
| A typosquatted plugin ID | Matching is exact, never by name or similar ID; one publisher per plugin ID in a repository; an ID differing only in case refused; an ID new to a package reviewed by a person ([proposal](#proposal)); a candidate from a publisher other than the one this device already uses for that ID is shown as such, with no default | A repository the user added can list any ID; only the user's trust in its key protects |
| A homograph address (`pаckages.oscairn.org` with a Cyrillic `а`) or `https://packages.oscairn.org@evil.example` | The format allows only lowercase ASCII letters, digits, `.` and `-` in the host, so a lookalike must be written as `xn--…`, and no user info; the key, not the address, is the repository | A plain lookalike in ASCII (`packages-oscairn.org`); the fingerprint and the signed name are shown |
| A downgrade: a document naming an old version | Revoked versions are never offered; a yanked one asks again, showing why it was withdrawn; versions install side by side, so no other document's version changes; a lower root serial is refused, as now | An old version, neither yanked nor revoked, with a flaw found later: the publisher must yank or revoke it |
| A hint whose key does not match | A known address with another key: the hint is not used and the known key is kept; a hinted root whose key is not the hint's: refused, nothing kept | None found |
| A repository that claims another's ID | Kept serials and indexes keyed by root key and ID (built 2026-11-11, `control-repository`) | None found |
| Another trusted repository's root served at a known repository's address | Each known address is read only with the root key recorded for it; another key is refused naming both, nothing kept (built 2026-11-11, `control-repository`; the app's snapshot only with a shipped key, `control-preinstall`) | A folder or an address not added yet takes any key trusted for repositories, as nothing is recorded for it |
| A repository that calls itself by another's name ("Oscairn") | Shown everywhere by the device's name for it, a name like an earlier one's with its key's ID after it; its own name only as "it calls itself …" beside the fingerprint when adding it (built 2026-11-11, `control-repository`, [names](#kept-state-keyed-by-the-root-key)) | A lookalike name in other scripts; the fingerprint tells them apart |
| A version or a publisher key found harmful after publishing | The repository's revocation list, signed with its root: withdrawn versions and revoked keys' packages never offered or installed, the official list's keys and builds everywhere; what is installed reported (built 2026-11-11, `control-repository`) | Installed copies keep working until the user removes them, by design; a revoked build in what is installed is not reported; a device that never reads the repository learns nothing |
| An older revocation list replayed, or one stripped | The list rides the root's serial, refused when lower; a missing or malformed list refuses the repository (built 2026-11-11, `control-repository`) | A stolen index key can sign a newer root with a list leaving itself out: revoked index keys are not remembered yet |
| Tracking through a hint | No fetch from a hint's address on open, prepare or install of a library; only **Look there** | A user who looks tells that host they opened it |
| Hints as a weight | At most 4096 entries, an address at most 2048 bytes; one job at a time; one **Look there** per tap | None found |

A hint in a signed item is covered by the item's signature, since the hint is part of the
document's contents (its `hash`); unsigned documents can carry any hint, which grants nothing.

### Build steps

In the [build order](#build-order-and-gates) as steps 9 to 13:

| Step | Deliverable | Gate |
| --- | --- | --- |
| 9. The index's plugins; kept state by key | `plugins` per version in the index and the device's reader; the index builder's one-publisher and letter-case checks; `seen` and the kept index keyed by root key and ID | `package-repository-index`: the Splitters package lists Split and Join with their platforms; another publisher's package with `org.oscairn.split`, and `org.oscairn.Split`, are each refused. `control-repository`: a repository whose root names the official's ID with a higher serial leaves the official's kept serial and index as they were |
| 10. The hint | `pluginRepositories` in archives (version 3 reads it), Performances (version 14) and Sounds (version 9): read, checked, kept on save for plugins still used, carried by export, kept by install, and the document's own kept when a Sound or import brings another for the same plugin | `control-archives`, `control-parts`: a hint survives save, export and install both ways; each of an `http://` address, a folder path, user info, a query, a trailing `/`, an uppercase or 63-digit key, the same plugin twice and 4097 entries refuses the document; a hint for a plugin no node uses is gone after save |
| 11. Lookup | `packages.lookup`; `missing` in `library.install`, `performance.prepare` and `library.query`; `failures[].plugin`; `packages.install`'s `locked` and `playing` | `control-repository`: the exact version found; a plugin of a two-plugin package found by its ID; not for linux-arm64; a system update; not found; with every request failing, an answer from the kept indexes with their serials; a hint to an unknown repository fetches nothing until `look`, which keeps nothing; a mismatched key not used; another build refused without `acceptOtherBuild`; `playing` at `instrument`; an app without `system` refused (`-32008`); no request at all during `performance.load` and `performance.prepare` with stand-ins |
| 12. The offers | The library's window, the Parts view's line and **Reload the Performance**, **Look there** and adding, the Setlist's line and **Check**, `oscairn.install.plugins` | `desktop-gaps`: each kind of row in the library's window; **Not now** installs and adds nothing; **Install selected** installs the package and its asset library; **Look there** then **Add and install** after the fingerprint; a mismatched hint added by nothing; the stand-in's line and Reload; the Setlist's entry line and **Check** |
| 13. Newer versions | `opensStateFrom` in the index and the lookup's candidates; **Install and use** with RFC 0043's **Use 0.4.6** (the claim itself, its checks and the move are RFC 0043's steps 1, 3 and 5) | A 2.0 claiming 1.0 is offered for a 1.0 document and its part moves only on the click; a 2.0 claiming 1.0 from another publisher key is offered as without a claim; the saved document is unchanged until saved |

### Open questions on finding missing plugins

For the owner:

1. **Newer versions:** add the publisher's signed claim, renamed `opensStateFrom` with
   `soundChanges`, as [RFC 0043](0043-moving-sounds-to-newer-plugin-versions.md#the-publishers-claim)
   defines it, and offer **Install and use** from this flow only with it, from the same publisher
   key as the saved version? Or never offer a newer version from this flow, leaving the Catalog and
   RFC 0043's moves? Recommended: the first. The claim's own questions (what it promises, whether CI
   must check it, moves without it) are RFC 0043's
   [decisions](0043-moving-sounds-to-newer-plugin-versions.md#decisions-for-the-owner) 1, 3 and 7.
2. **Every repository that is on, or only some?** With no hint, this design looks in every
   repository that is on. RFC 0031 [rules out](#ruled-out) "looking in other repositories for a
   missing package of the same name"; this design reads that as about an installed package's
   updates and repair, which still come only from its repository, and looks up a plugin with no
   install here by its exact identity everywhere, naming each candidate's repository and
   publisher. Accept that reading, or limit it to the official and the device's repositories?
3. **Yanked exact versions:** offered with a second question (this design), or not offered?
4. **`packages.lookup`'s permission:** `system` with the rest of `packages.*` (this design), or
   `library`, as it only reads?
5. **Hints in Performances and Sounds** as well as archives (this design): they survive copies
   and a plugin removed later, but change the document's content hash. Archives only instead?
6. **One repository per plugin** in a hint (this design), or a list, a publisher's own and the
   official, say?
7. **Not now:** ask again at the next install of the same library (this design), or remember it?

## Trust and safety

### Keys

| Key | Held by | Signs | Means |
| --- | --- | --- | --- |
| Repository root key | The repository's maintainers, offline | The index key | This is the repository |
| Repository index key | CI | The root file | This repository lists these packages, versions and publishers |
| Publisher key | The publisher | Packages, plugin builds, apps, items | The publisher made this |
| Device key | Each Oscairn install | Your own exports and repository | One of your devices made this |

A repository cannot forge a publisher's package: the publisher's signature would not verify.
A publisher cannot appear in a repository without it. RFC 0006's trust store gains one purpose,
`repositories`, beside `plugins` and `content`. A repository's listing counts as trust in its
publishers' keys for content, not for code: a new code publisher is approved once on the device
([decided](#decided-2026-11-01)).

### Takedown and revocation

- **The revocation list** is named in the root and so signed with it: revoked publisher keys,
  revoked build digests and withdrawn package versions, each with a reason and a date. Specified
  and applied on devices 2026-11-11 ([below](#the-revocation-list)). Revised the same day from
  the owner's request: what is installed is never deleted or disabled silently, so a revoked build
  no longer stops loading on refresh; the user is told, where they look, and offered to remove or
  replace it, and nothing interrupts on stage. The official list is to travel in system images
  too (not built).
- **Takedown for safety** (malware, a build that harms devices): the version is revoked and its
  blobs removed.
- **Takedown for rights** (a copyright claim): the blobs are removed from the repository and
  its mirrors and the index marks the version removed. Copies already installed are not
  revoked: that is a legal matter between the parties, not a safety one.
- A published policy says who decides, how to report, and how a publisher appeals. Every
  takedown is a commit in the submissions repository, so the record is public.

### The revocation list

Specified and built on devices 2026-11-11 (step 4; the takedown policy and system images are
not). Until then the root named a list and the index builder wrote `revoked.json` into it, but
no entry format was specified beyond a `reason`, and nothing applied it.

**Format.** `revocations/<sha256>.json`, what the maintainers keep as `revoked.json`:

```json
{
  "format": "oscairn.repository-revocations",
  "version": 1,
  "keys": [
    {"key": "3f9a1c0d2e4b5a67…(64 hex digits)", "reason": "Its private key was published", "date": "2026-11-11"}
  ],
  "versions": [
    {"id": "com.example.organ", "version": "2.1.0", "reason": "This build deletes files in the user's home folder", "date": "2026-11-11"}
  ],
  "builds": [
    {"sha256": "9b1c…(64 hex digits)", "reason": "Malware found by a scanner", "date": "2026-11-12"}
  ]
}
```

- `keys`: a publisher key revoked, as its whole Ed25519 public key in lowercase hex, not its
  16-digit ID. An old index key goes here too when it is replaced ([keys](#keys-1)).
- `versions`: a package version withdrawn, by package ID and version.
- `builds` (optional to use, always present): a plugin build revoked by its digest, as an
  archive's manifest lists it (`bundleDigest`, the digest a Performance's lock records).
- Each entry has a `reason`, shown to users, never empty, and the `date` it was decided,
  `YYYY-MM-DD`. All three lists are present, empty when nothing is in them. Anything else (a
  list missing or not a list, an entry without its fields, a key or digest not 64 lowercase hex
  digits, a date in another form) is malformed.

**Signed.** The list is not signed on its own. The root names it by path, SHA-256 and size
(`revocations`, `{path, sha256, size}`), and the root is signed by the index key, which the root
key signs: the same chain that covers the index. A device reads the list from the repository's
address or a mirror, checks its size and hash against the root, then reads it.

**Withdrawn is not yanked.**

| | Yanked | Withdrawn |
| --- | --- | --- |
| Who | The publisher, in the release record (`yanked`, the index's) | The repository's maintainers, in the revocation list |
| Means | Advice: there is a better version | Do not install this |
| Offered for a new install or an update | No | No |
| Installed when a lock or `version` names it | Yes, so a Performance keeps working | No, never |
| Installed already | Shown as yanked | Reported with the repository and reason; kept, never removed or disabled, offered to remove or replace |

A revoked key withdraws every version of every package its key signed; a revoked build withdraws
every archive holding it.

**Whose list applies where.** A repository's list applies to its own packages: its versions, the
publisher keys it lists, the builds in them. The official repository's `keys` and `builds` apply
to every repository's packages too, as the project's way to stop a key or a build everywhere; its
`versions` apply to its own, since a package ID and version name a release only within one
repository. No other repository's list reaches beyond its own packages: otherwise a repository the
user added could withdraw the official one's packages, or any publisher's everywhere. The trust
store's own revoked keys (`trust.json`) apply as before, to everything.

**On a device.**

- **Read with the root**, after its signatures and trust: one missing or malformed refuses that
  repository ("its root names no revocation list", "the revocation list has no `keys` list"), never
  read as nothing revoked. A list revoking the root's own root or index key refuses the root.
- **Kept** beside the kept index, `Repositories/<root key>/<repository>.revocations.json`, only
  after the root's serial is taken (`see()`), so an older root never replaces it, and read again
  from there while the root names the same hash, so a refresh with nothing new still fetches the
  root alone. The official
  repository's kept list is what other repositories' installs check; a device that has never read
  the official repository has none to apply.
- **Listing:** each version says `withdrawn` (`{repository, reason, date}`, the repository by this
  device's name for it), or `null`. The Catalog never offers it, nor picks it as the newest or an
  update, and says "Cannot be installed here: withdrawn by <repository>: <reason>".
- **Installing:** a withdrawn version is refused even when named, a revoked key's packages and an
  archive holding a revoked build too: "refused: <id> <version> was withdrawn by <repository>:
  <reason>". The newest version and asset libraries skip withdrawn ones.
- **Installed already:** `packages.installed` gives each package `withdrawn` by the lists last read
  of the repository it came from and the official one: its version, or a key that signed its
  archive. Nothing removes or disables it, and nothing interrupts: the Catalog's row says
  "Withdrawn by <repository>: <reason>", with Update to a newer version when there is one, and its
  details say "<name> <version> was withdrawn by <repository>: <reason>", how to replace it, and to
  remove it from the Library's list. A revoked build in what is installed is not reported:
  `packages.json` does not record build digests.
- **Rollback:** the list rides the root's serial, which a device refuses to see go down per root
  key and repository ID ([kept state](#kept-state-keyed-by-the-root-key)), so an older list comes
  only with an older root, refused before anything is kept.

`control-repository` tests a withdrawn version listed so, refused when named and not chosen as the
newest; an installed one reported and kept; a revoked key's package refused; an older root (and its
list) refused, the newer list still kept; a list without its date, one whose `keys` is not a list
and a root naming none each refusing the repository, keeping nothing. `desktop-gaps` tests the
Catalog's offer and wording. Not tested: the official list applying to another repository (its key
cannot sign a test repository).

Not built: the takedown policy; the official list in system images; a revoked build stopping to
load, which the revision above drops; remembering an index key a list revoked, so that a stolen index key
signing a later root with a list that leaves itself out is still refused (today the serial alone
protects a device that saw the revocation only until the thief publishes a higher serial).

### What scanning can do

A malware scanner finds known malware. It cannot prove native code safe. A plugin is
arbitrary code, and a determined author can hide what it does. What protects a device is
knowing who made a build (signatures), being able to stop it everywhere (revocation), and
running it isolated in a worker (RFC 0010).

### What a repository cannot promise

- That code is safe, or bug-free: a trusted publisher's bug is still a bug.
- That a package sounds or performs as described beyond the checks it passed.
- That the publisher has the rights to what they publish: that rests on their declaration and
  takedown.
- That a package stays available forever. Locks fetch yanked versions, but removed files are
  gone; keep backups (RFC 0006's repair routes).
- That a device without a clock sees the newest index.

### Privacy

No account to browse, download or install. Requests carry no device ID. The static host sees
addresses and files, as any web server does; the project publishes download counts only in
aggregate, if at all.

## Publisher trust and review

Whether a repository's listing counts as trust in its publishers' keys for plugins, and how
much human review a package with code gets, were decided from this comparison of what other
repositories do ([decided](#decided-2026-11-01)). Sources were read on
2026-11-01.

### What others do

Confirmed from each source:

| Repository | Publisher identity | Review of a first submission | Review of updates |
| --- | --- | --- | --- |
| [Flathub](https://docs.flathub.org/docs/for-app-authors/submission) | [Verified](https://docs.flathub.org/docs/for-app-authors/verification) by a token at a well-known URL or a DNS record on the app ID's domain, or by owning the repository for `io.github.` and similar IDs | Reviewed by volunteer reviewers on the pull request; permissions are reviewed separately | [Maintainers](https://docs.flathub.org/docs/for-app-authors/maintenance) test and merge their own updates. A change of permissions must be justified, and a build can be held until a moderator approves it |
| [F-Droid](https://f-droid.org/docs/Inclusion_Policy/) | None needed: it builds every app from public source itself | Contributors review apps by hand, including for non-free dependencies and tracking; undisclosed anti-features are rejected | Built from source by F-Droid, as for the first |
| [Debian](https://www.debian.org/doc/manuals/developers-reference/pkgs.en.html) | Uploads must be signed by a key in the Debian Developers or Debian Maintainers keyring | A new package is placed in the archive by hand, after the archive administrators review it | Updates to existing packages in `unstable` are processed automatically |
| [VS Code Marketplace](https://code.visualstudio.com/docs/configure/extensions/extension-runtime-security) | A publisher can earn a verified badge by proving domain ownership | A malware scan of every package, with several antivirus engines | The same scan of every update. VS Code itself asks the user to trust a third-party publisher on the first install of one of its extensions; later ones from that publisher install without asking |
| [npm](https://docs.npmjs.com/threats-and-mitigations) | An account; [provenance](https://docs.npmjs.com/generating-provenance-statements) is optional | No review before publishing. Names close to popular packages are blocked | Scanning for known malicious content; reported packages are removed after publishing |
| [Apple App Store](https://developer.apple.com/distribute/app-review/) | Not covered by the source | Reviewed | Every update is reviewed |
| [Google Play](https://support.google.com/googleplay/android-developer/answer/9859751) | Not covered by the source | Reviewed | Updates are reviewed and published "as soon as possible"; some apps and accounts get extended reviews of up to 7 days |
| [Homebrew casks](https://docs.brew.sh/Acceptable-Casks) | Not covered by the source | Judged against the acceptance policy; macOS apps must pass Gatekeeper's checks without it being bypassed | Malware and removal are decided case by case, with evidence; Homebrew does not certify that cask apps are safe |
| [VCV Library](https://github.com/VCVRack/library) | None stated | An open-source plugin is requested in an issue with its source URL; a closed-source one by email. A maintainer handles each request. No formal code review is described; plugins must follow VCV's ethics guidelines and not harm the user's computer or privacy | The developer comments with the new version and commit hash, and a maintainer updates the build |

Inferred, not stated by the sources:

- VCV appears to build open-source plugins itself from the commit given. Its
  [licensing page](https://vcvrack.com/manual/PluginLicensing) says "a source package which we
  will build for you", but in its section on commercial plugins.
- Homebrew cask changes arrive as pull requests that maintainers merge, as for the rest of
  Homebrew. The acceptance page does not describe the review step.
- The pattern among the free repositories is human review at the door (a new package or a new
  publisher) and automation after it, with a second human look when an update asks for more
  power (Flathub's permission changes). Only the commercial stores, with paid staff, review
  every update.
- Domain verification is the common way to give a publisher a badge (Flathub, VS Code). This
  RFC's `.well-known` check already does it.

### Proposal

Accepted by Jacob on 2026-11-01 ([decided](#decided-2026-11-01)).

- **A repository's listing counts as trust for content,** not for code. Sounds, Performances and
  samples from a listed publisher install as signed by a known key. Content installs anyway
  under RFC 0006's policy; the listing changes what is shown, not what is allowed.
- **For code, trust on first use, then pinned.** The first plugin or app from a publisher new to
  the device asks once, naming the publisher, its fingerprint and its repository, as VS Code
  does. Yes adds that key to the trust store for `plugins`. Later builds from that publisher
  install without asking. Only that key is accepted for that publisher: a build under the same
  publisher ID with another key is refused unless the old key signed the new one (RFC 0006's
  key replacement). A product may skip the question for its own manufacturer's repository by
  shipping its publishers' keys in the image's trust store, as RFC 0006 already allows.
- **Human review** of a publisher's first submission, and whenever a package newly adds code: a
  first plugin or app, or a plugin ID the package did not have before. Everything else, code
  updates from a known publisher included, is merged on the automated checks. This is Flathub's
  shape, with "adds code" in place of "adds permissions". Revocation is the backstop for what
  review misses.
- **A verified publisher badge** where the publisher ID's domain names its key at
  `.well-known/oscairn-publisher.json`, shown on the package's page and in the install
  question. A badge says who, not that the code is safe.

## Hosting and cost

The design needs only static storage, a CDN, a Git repository and CI. No server-side code: no
upload API, no accounts, no database. Nothing depends on one provider, because the signed root
names the blob base and every file is checked by its hash.

### GitHub first

The owner's decision of 2026-11-01 ([decided](#decided-2026-11-01)): the official repository
starts on GitHub, with no server.

| Part | Where | What it holds |
| --- | --- | --- |
| Submissions | The `oscairn/packages` repository; a pull request is a submission | `publishers/`, `packages/`, `revoked.json`, `repository-config.json` and the scripts |
| Checks and publishing | GitHub Actions: `check.yml` on pull requests, `publish.yml` on each merge to `main` | Builds `oscairn-sign` at a pinned commit, checks, builds the index, signs the root |
| The signed root and index | GitHub Pages | `repository.json`, `repository.sig`, `index/`, `revocations/` |
| Package files | GitHub Releases assets: a release `blobs-<ab>` per first two hex digits, each asset named by its SHA-256 | The blob base is `https://github.com/oscairn/packages/releases/download/blobs-`, so a blob is at `blobs-<ab>/<sha256>` under it |

The template for the repository, its workflows and its index builder are in
[tools/package-repository/template](../../tools/package-repository/template/README.md).
Release downloads redirect to another GitHub host, so devices follow redirects; the hash is
what they trust, not the host.

GitHub's limits, as its documentation gives them (read 2026-11-01):

- **Pages:** a published site may be no larger than 1 GB, and the page has given a soft
  bandwidth limit of 100 GB a month; check the
  [current Pages limits](https://docs.github.com/en/pages/getting-started-with-github-pages/github-pages-limits)
  rather than treating that figure as fixed. Pages is not for running an online business,
  e-commerce or SaaS, and for a site over quota GitHub suggests a CDN in front, Releases, or
  another host. The index is small (the target is under 2 MB) and fits. Nothing in it is
  sold ([decided](#decided-2026-11-01)).
- **Releases:** each file must be under 2 GiB, with no limit on a release's total size or
  bandwidth, and at most 1000 assets in a release
  ([About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases),
  [About large files on GitHub](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)).
  Users report failures near 2 GiB, so the index builder refuses an archive over 1,900 MiB. A
  file in the repository itself must be under 100 MiB, so larger archives are linked from a
  submission, not committed.
- **Acceptable use:** bandwidth limits vary by feature, and usage significantly above similar
  users' can lead GitHub to throttle file hosting or suspend the account until it drops
  ([GitHub Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies),
  "Excessive Bandwidth Use"). That covers package files on Releases, not only Pages. The
  publish job reports total release downloads in its summary, so the project sees growth
  coming.
- **Malware:** the same policies forbid using GitHub to deliver malware. A malicious plugin
  that slips past the checks could get the repository's releases or the account disabled. That
  is one more reason for [revocation and fast takedown](#takedown-and-revocation), and for
  keeping the blob base movable.

### Moving package files to object storage

Package files move to object storage behind a CDN under a custom domain, such as Cloudflare
R2, when the first of these comes:

- a package file of 2 GiB or more that the official repository hosts (a large sample library);
- Pages bandwidth near its soft limit, or release downloads growing toward what GitHub's
  acceptable use allows;
- more blobs than the releases hold comfortably (256 releases of 1000 assets), once releases
  are stored in pieces.

[R2](https://developers.cloudflare.com/r2/pricing/) charges no egress fees. Its free tier is
10 GB-month of storage, 1 million Class A and 10 million Class B operations a month; beyond it,
$0.015 per GB-month, and $4.50 and $0.36 per million Class A and Class B operations. Moving is a
copy and one field: copy every blob to `blobs/<ab>/<sha256>` under the new domain, set
`blobBase` in `repository-config.json`, and merge. The next signed root names the new base.
Devices change nothing, because every file is checked by its hash. The root and index may stay
on Pages or move too.

### What costs what

| Part | On GitHub | After the move |
| --- | --- | --- |
| Submissions and CI | Free for a public repository | The same |
| Root and index | Pages, free within its limits | The same, or the CDN |
| Package files | Releases, free, within GitHub's acceptable use | R2: storage past 10 GB-month and operations past the free tier; no egress fees |
| A custom domain | Optional | A domain's yearly fee |

Most bandwidth is sample libraries. The official repository may host a few; the rest are
hosted by their publishers through blob bases ([decided](#decided-2026-11-01)). Sample
deduplication and per-platform downloads keep the rest down.

### Mirrors

Anyone can mirror the repository by copying the static tree: the root, its signature, `index/`,
`revocations/` and the blobs, with rsync, a download of the Pages site and the releases, or a
CDN's own copy. The root lists mirrors and devices try them in turn. Mirrors need no trust: a
device checks the root's signature and every file's hash, and refuses a root with a lower
serial than one it has seen ([freshness](#signatures-and-freshness)).

### What the project sets up

1. **The repository:** create `oscairn/packages` in the GitHub organization and copy in the
   template. Protect `main`: pull requests only, with review from Code Owners required (without
   it `CODEOWNERS` blocks nothing), the Check workflow required, no force pushes (the serial is
   the number of commits on `main`).
2. **The pinned tools:** set `OSCAIRN_REPOSITORY` and `OSCAIRN_COMMIT` in both workflows to the
   Oscairn repository and a commit of it that has `oscairn-sign file`.
3. **The Pages site:** Settings, Pages, Source: GitHub Actions. A custom domain is optional.
4. **The blob base:** nothing to create; the publish job makes each `blobs-<ab>` release on
   first use, and `repository-config.json`'s `blobBase` names them.
5. **The keys and the secret,** below.

### Keys

- **The root key** is made on an offline machine and stays offline with the maintainers, with a
  backup. It signs only the index key. Devices trust the repository by its fingerprint.
- **The index key** is made the same way and signed by the root key (`oscairn-sign file
  root.key index-key.pub index-key.sig`, the file holding its 64 hex digits and a newline). It
  is stored as the `OSCAIRN_INDEX_KEY` Actions secret, and the local copy deleted. The root
  key, the index key and its signature go in `repository-config.json`. The publish job writes
  the secret to a file only it can read, signs the root, deletes the file, and checks that the
  signer is the index key the config names.
- **Rotation:** a new index key is made and signed by the root key the same way, the secret and
  the config are replaced, and the old key goes in `revoked.json`. A stolen index key is
  rotated at once. Replacing the root key means devices trust a new fingerprint, through a
  system image's trust store or by the user; until then a device refuses the new root at the
  repository's address, as another key than the one recorded for it. Key replacement for publishers is not built yet,
  so the checks refuse a changed publisher key.

## Build order and gates

| Step | Deliverable | Gate |
| --- | --- | --- |
| 1. Format and folders | `repository.json`, index, deltas and blobs; `oscairn repo build`; installing from a folder repository, a folder package and an `.olib` read in place; streamed installs | A device with no network installs a package from a USB folder; a changed blob, a root with a bad signature and a root with a lower serial are each refused; a folder package with a changed file is refused and leaves nothing behind; an `.olib` on USB installs with free space of only its installed size plus its largest entry; pulling the drive midway leaves the device as it was |
| 2. HTTPS | Refresh with deltas, mirrors, resumable downloads, per-platform fetch | A refresh with one changed package downloads the root and one delta; a mirror serving a tampered blob is refused and the next mirror used; an interrupted download resumes |
| 3. Publishing | `oscairn publish`, the submissions repository, CI checks, immutable versions, yanking | A signed release published through a pull request installs on a device; an unsigned one, a re-used version and a missing claimed platform build each fail in CI; a yanked version is not offered but its lock still fetches it |
| 4. Revocation and takedown | Revocation lists, the takedown policy | A withdrawn version and a revoked key's package are not offered or installed, and one installed is reported and kept (revised 2026-11-11 from "a revoked build stops loading on refresh": nothing installed is disabled silently); the same from a system image on an offline device |
| 5. Discovery | Install…, the Catalog app and its actions, compatibility filters, `packages.*` | A search by type on an instrument answers from the local index with no network; a package without a build for the device is grayed with the reason |
| 6. Updates | Update lists, explicit lock updates, app updates | Installing a new version leaves every Performance's lock as it was until **Use version** is chosen; an app update applies only once the app is closed |
| 7. Offline and multi-device | Fetch for a device, USB subsets, a paired computer as source | A desktop queues packages for a KICK profile and writes a USB drive with only its builds; the device installs them; the same through a paired connection |
| 8. Public | Hosting, CDN, mirrors, the policy, the root key ceremony | The official repository serves from two mirrors, and a device survives one going away |
| 9 to 13. Finding missing plugins | The index's plugins, the repository hint, `packages.lookup`, the offers, newer versions | Each step's gate is in [finding missing plugins](#build-steps) |

**Left of steps 1 to 3 (2026-11-06).** Steps 1 and 2 are built in part (see Progress). Step 3
waits on them. What remains, in order:

1. Releases stored in pieces: the index builder splits each archive into its manifest, each
   item folder as a zip, each sample and each plugin build as blobs, and a device fetches only
   the items chosen, the samples it lacks and its own platform's builds, unzipping each as it
   streams, so peak storage is the installed size plus one entry from the web too. Decided
   2026-11-12: built together with it, the ready-made `.olib` per platform of each version
   ([above](#ready-made-archives-per-platform)), with no interim archive per platform.
2. HTTPS on Linux devices: done by RFC 0039's step 1 (libcurl with Mbed TLS in the image,
   `JUCE_USE_CURL` on); the core's `Transport` did not change. A device whose time is unknown
   fetches nothing from the web and says to set the time first (RFC 0035, decided 2026-11-08).
3. `.orepo` files, each installed package recording its repository, and the root fetched from a
   mirror when the repository's own address is down. (The desktop installing from an address and
   a device's list of repositories: done 2026-11-07, see Progress.)
4. Tests for the free-space refusal and a drive pulled midway.
5. Step 3's `oscairn publish`: pack (or take) the release, run the checks the submissions
   repository's `check.yml` runs (`build-index.py --check` with `--previous` against the
   published tag) locally on a copy of the submissions repository with the new record, sign each
   build and the archive with the publisher's key, and write the release record
   (`packages/<id>/<version>.json`) for the pull request. Immutable versions and yanking are
   already enforced by the index builder; the command adds `oscairn publish --yank <version>
   <reason>`, which changes only `yanked`.

Steps 1 and 3 need RFC 0006's signatures built. Step 5 needs RFC 0022 step 2 (apps in their own
processes) for the Catalog app, and RFC 0023 step 2 for types in queries. Step 4 comes before
step 8: nothing public without a way to stop it.

## Decided 2026-11-01

Jacob's decisions on six of the open questions:

- **Publisher trust:** a repository's listing counts as trust for content, not for code. Code is
  approved once per publisher on the device, then pinned to its key, as the
  [proposal](#proposal) describes. (2026-11-11: but for the official publisher's key, built in;
  see [several repositories](#several-repositories).)
- **Review:** human review of a publisher's first submission and whenever a package newly adds
  code; automated checks after that; a verified publisher badge for publishers who prove their
  domain ([proposal](#proposal)).

- **Paid content:** the official repository carries no paid, paywalled or DRM-protected
  packages, and nothing is sold through it (decided 2026-11-07, replacing "for now"). So the
  app that browses it is the **Catalog**, not a store.
- **No pressure to install more:** the Library's entry is **Install…**, not "Get more"; the
  Catalog has no featured or promoted packages, no counts or badges urging installs, and its
  only card on Home is for updates to what is installed (decided 2026-11-07).
- **Install… in the Library** offers exactly Catalog and File (decided 2026-11-07,
  [in the Library](#in-the-library)).
- **Large sample libraries:** the official repository may host a few. Others are hosted by their
  publishers through the blob base ([package files](#package-files)).
- **A manufacturer replacing or hiding the official repository:** allowed, but heavily
  discouraged. The official repository stays one switch away in Settings, and the
  documentation and the profile schema flag hiding it as discouraged
  ([several repositories](#several-repositories)).
- **Hosting:** GitHub first, with no server. Submissions are pull requests to
  `oscairn/packages`; GitHub Actions checks them and, on merge, builds and signs the index with
  the index key from Actions secrets, while the root key stays offline. The signed index is on
  GitHub Pages and package files are Releases assets. Package files move to object storage
  behind a CDN (Cloudflare R2) under a custom domain when files over 2 GiB or bandwidth require
  it; devices do not change ([hosting](#hosting-and-cost)).

## Decided 2026-11-12

The owner's decision: "We will plan on doing the pieces design along with the ready made olibs
per platform."

- **Releases stored in pieces** is the plan, so a device fetches the manifest, what all platforms
  share and its own platform's builds ([package files](#package-files)). More platforms are
  coming: linux-arm64 now, later Windows, x86_64 and hardware-accelerated builds.
- **A ready-made `.olib` per platform** of each version, a derived export for downloading by hand,
  replaces the whole archive in the Git tree ([the section](#ready-made-archives-per-platform)).
  Both are built together; there is no interim archive per platform in the record. Planned, not
  built; its open questions are the owner's.

## Open questions

- **Built-in apps:** do apps that ship in the system image also update from the repository
  between images, or only with the image?
- **Publisher IDs:** verified by domain (reverse-DNS), or names the project hands out?
- **Reproducible builds:** reported only, or required of open-source packages in the official
  repository?
- **Popularity:** no download counts, or aggregate counts from the CDN's logs to sort by?

## Ruled out

- **Server-side code in the first version:** accounts, upload APIs, ratings and reviews all
  need one. Static files and CI cover publishing and installing.
- **An account to browse or install.**
- **Unsigned packages in a repository.**
- **Changing a published version,** even by its publisher.
- **Updates that move a Performance's lock,** or that install while playing.
- **Install scripts:** packages are files; nothing runs on install (RFC 0006).
- **Looking in other repositories for a missing package of the same name.** (Proposed
  2026-11-11, for the owner: this stays true of an installed package's updates and repair, while
  a plugin with no install here is looked up by its exact identity in every repository that is
  on; [open question 2](#open-questions-on-finding-missing-plugins).)
- **The repository's signature standing in for the publisher's:** they say different things.
- **Telemetry** from devices to the repository.
