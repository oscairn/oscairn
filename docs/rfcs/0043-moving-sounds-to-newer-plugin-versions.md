# RFC 0043: Moving sounds to newer plugin versions, and removing unused ones

Status: proposed, 2026-11-11, from the owner's request of that day. Design only: nothing here is
built. Builds on packages, locks and installed content ([RFC 0006](0006-packages-and-portability.md)),
sound assets' retention roots ([RFC 0009](0009-sound-assets-and-dependencies.md)), plugin workers
([RFC 0010](0010-plugin-worker-isolation.md)), the library and audition
([RFC 0021](0021-sounds-performances-and-library.md)), resource budgets
([RFC 0025](0025-resource-budgets.md)), reliability levels ([RFC 0027](0027-reliability-levels.md))
and the package repository ([RFC 0031](0031-package-repository-and-distribution.md)), whose
[finding missing plugins](0031-package-repository-and-distribution.md#finding-missing-plugins)
offers a newer version only with the publisher's claim defined here.

**Why a new RFC, not a section of RFC 0031 or RFC 0006.** Moving a saved sound is about
documents, the library and what plays, not about distributing packages: it touches RFC 0021's
library and audition, RFC 0025's memory and RFC 0006's cleanup as much as RFC 0031's index. RFC
0031 is over 1,300 lines already; RFC 0006 is the broad packaging contract. RFC 0031 keeps what belongs to the repository (the claim carried in the
index, the offer at a stand-in) and points here.

## Progress

Checked when built and tested; the sections below say how.

- [ ] 1. The publisher's claim, `opensStateFrom` and `soundChanges`: written by `oscairn-sign`,
      read from archives, carried into the index, kept in the plugin store, shown by `plugins.list`
- [ ] 2. Moving library items: `versions.plan`, `versions.move`, `versions.job`, undo and
      `stateSavedBy`
- [ ] 3. Moving the open Performance's parts (`parts.useVersion`) and the mixed-versions line
- [ ] 4. Comparing the two versions by audition (`audition.start`'s `versions`, `audition.switch`)
- [ ] 5. Checking the claim by machine: `oscairn-plugin-check --state-from`, the import
      workflow's state check in oscairn/packages, then the repository's Loading check
- [ ] 6. Removing unused versions: retention roots for plugins, `plugins.list`'s `keptBy`,
      `plugins.removeUnused`, Settings' Installed content page
- [ ] 7. The offers: the Library, the Catalog's **Update sounds using this**, the Setlist's scope

## Problem

A bug-fix release cannot reach the sounds that need it. The owner's case: Oscairn Analog 0.4.5
has a stuck-note bug, fixed in 0.4.6, and every Performance the owner has is locked to 0.4.5 (as
the owner reports it; the Analog plugin's sources are not in this repository, so not checked
here). Installing 0.4.6 changes nothing they play, by design.

Read in the code and docs on 2026-11-11:

- **Versions sit side by side.** The plugin store is `Plugins/<format>/<id>@<version>/<platform>/<bundle>`
  in the data folder, with `plugin.json` (`format`, `id`, `version`, `name`, `license`) and each
  build's publisher signatures beside it as `<bundle>.sig` (`storeFolder` and `storeBuild` in
  `core/control/src/performance.cpp`; [the plugin store](../library-archive.md#the-plugin-store)).
- **A document names the exact version.** Each node's `plugin` is format, ID and version, its
  path only a hint; the `lock` records each build's digest per platform
  ([Performance format](../performance-format.md), the `lock` row). Loading takes the path hint or
  the store's build of exactly that version; any other version makes a stand-in that keeps the
  state ("version 0.4.6 installed") (`resolvePlugin` in `performance.cpp`). There are "no silent
  upgrades or untested state conversions" ([format](../performance-format.md)).
- **Updates never move a lock on their own** (RFC 0031 [updates](0031-package-repository-and-distribution.md#updates),
  [ruled out](0031-package-repository-and-distribution.md#ruled-out)). RFC 0031 names **Use
  version 2.1** and **Update Performances using this** but specifies neither, and its Progress
  step 6 lists "Performances' locks" as not built.
- **Installs are recorded** in `packages.json` (`packages.installed`), and the Catalog says
  Installed, Update to a newer version, or Installed (a different build); nothing installs or
  updates on its own (RFC 0031 Progress step 6, `hosts/standalone/Catalog.cpp`, versions ordered
  by `repository::older`, `core/control/src/repository.cpp`).
- **Nothing says a newer version reads an older one's state.** CLAP's state extension has `save`
  and `load` and nothing about versions; the archive manifest has no such field
  (RFC 0031 [versions](0031-package-repository-and-distribution.md#versions), which left a
  publisher-signed `restoresStateFrom` as an open question).
- **Cleanup is planned, not built.** RFC 0006's
  [installed content and removal](0006-packages-and-portability.md#installed-content-and-removal)
  plans **Remove all unused**, grouped prompts and retention settings; its Progress says
  "nothing collects" plugins. `plugins.list` shows each stored version with the library items
  using it (`usedBy`) and whether the open Performance plays it (`open`), and `plugins.remove`
  removes one, refused while the open Performance plays it (`Session::installedPlugins`,
  `Session::removePlugin`, `performance.cpp`). Settings' **Installed content** page is a hidden
  placeholder (`{"content", "Installed content", everywhere, notBuilt}`,
  `hosts/standalone/apps/settings/Pages.cpp`).
- **Assets already have roots.** Library items, the open Performance, Performances saved outside
  the library (`performance.save` records each folder, `assets_.addRoot` in `performance.cpp`) and
  plugin packages keep asset libraries installed; `assets.collect` removes what nothing uses
  (RFC 0009 [status](0009-sound-assets-and-dependencies.md)).

## Outcome

- The musician moves a Sound, a Performance, everything from one library, or everything using
  Analog 0.4.5, to an installed newer version, by an explicit action. Before anything changes,
  they see each item, what the publisher says changes, which settings would read differently,
  and what cannot move and why. They can hear old against new first. They can undo it.
- A publisher says, in the signed manifest, which older versions' saved state a version opens,
  and what sounds different. A machine checks what it can of that before the repository lists
  it, and the musician is told what was checked.
- Settings' Installed content lists each plugin's versions with what keeps each one, and removes
  the unused ones only when the musician says so.

## What "the same sound" can mean

From strongest to weakest, and who can tell:

| Level | Meaning | Who can check it |
| --- | --- | --- |
| Identical | The same notes render the same samples | A machine, for the notes it plays; rare across versions (a changed filter's rounding breaks it) |
| Within a tolerance | Renders differ by less than a stated level (below) | A machine, for the notes and settings it plays |
| The same, except as said | Every setting keeps its value and meaning; it sounds the same except the publisher's listed changes (the stuck note no longer sticks) | Partly a machine (settings read back the same); the rest is the publisher's word and the musician's ears |
| Opens | The state loads, no stand-in, the same ports | A machine, completely, per item, on the device |

A bug fix is by nature not "identical" where the bug was. So the claim (below) promises the third
level, and says what changes; the device and CI check the fourth level completely and the first
two on what they render; the musician's ears are the last check, which is what **Compare** is for.

## Moving on purpose

### The action

**Use a newer version** names a plugin, the version the items have now and the version to move
to, and a scope:

- **one item:** a Sound or Performance in the library, from its menu;
- **the open Performance:** one part's node, or every node of that plugin (the default, so the
  Performance does not end up mixing versions, [below](#mixed-versions-and-memory));
- **a library:** the musician's own items whose `origin` is that library ([RFC 0021](0021-sounds-performances-and-library.md#libraries));
- **a Setlist:** the Performances its entries name ([below](#setlists));
- **everything using it:** every library item using that version (what `plugins.list`'s `usedBy`
  counts), from
  the plugin's row in Installed content, or from the Catalog after an update (**Update sounds
  using this**).

The target must be installed. A newer version not installed is installed first through the
Catalog, as now, which never moves anything itself. Only a version newer than the one saved
(`repository::older`) is offered; moving to an older version is only **Undo**, below.

### What it does to a document

The move changes, per node using the plugin at the old version:

- `plugin.version` to the new version, and `pathHint` to the store's build of it here;
- the `lock`: this plugin's entries for the old version are replaced by the new version's, one
  per platform the store holds a build of it for, with that build's digest; other platforms' get
  none, so a device there takes any build of the new version, as a document with no lock entry
  does now ([RFC 0031](0031-package-repository-and-distribution.md#versions));
- new, `plugin.stateSavedBy`: the version whose state the node's snapshot holds (the old one).
  Saving from the plugin drops it, since the state is then the new version's own.

**The node's saved state is not rewritten.** Its bytes under `nodes/` stay exactly as they were;
the new version reads them when the item next opens, and writes its own state at the next save.
So the move never loses anything the old version saved, and undoing it is restoring one JSON
file. The content `hash` changes, as the document changes.

**Before writing, each node is opened by the new version:** a job, off the control thread, loads
the new build (in a worker where workers are used, [RFC 0010](0010-plugin-worker-isolation.md),
with a time limit and memory limit, its stderr captured, the worker recycled after each item),
restores the node's snapshot, and reads back its ports and parameters. An item moves only when
every node of it opens there without a stand-in and with the same audio and note ports as the
document saved (compared as taking over a loaded instance compares them now, `checkPorts` in
`performance.cpp`). Its parameters, read back by ID, are compared with
the document's: those whose value reads differently, those gone and those new are the item's
**settings that read differently**.

### What the musician sees first

The plan, before anything moves (wording an example):

> **Use Oscairn Analog 0.4.6 for 214 items?**
>
> Example Audio says 0.4.6 opens sounds saved with 0.4.0 or later. What changes:
> "Notes no longer stick when a chord is released during the glide."
>
> 211 open in 0.4.6 with every setting the same.
> 2 open with settings that read differently: **Warm Pad** (Drift 0.30 → 0.25). **Show**
> 1 can't move: **Factory Brass** is in the locked library Analog Factory.
>
> **Compare one** · **Move 213** · **Cancel**

Without the publisher's claim, the line reads "Example Audio doesn't say 0.4.6 opens sounds saved
with 0.4.5. Settings may be lost or sound different." and only one item, or the open
Performance, moves at a time, after **Compare** or **Move without comparing**
([decision 3](#decisions-for-the-owner)).

### Undo

Each move keeps, per item, the document as it was before (`performance.json` or `sound.json`
only, as nothing else changed) in the data folder under `Moves/<move>/`, with the revision the
move wrote. **Undo** puts back each item whose revision is still the one the move wrote; an item
saved since is not touched and is listed ("Warm Pad was saved since; open it to move it back by
hand"). Kept moves are listed in Installed content and are a retention root for the old version
([below](#what-keeps-a-version)) until the musician chooses **Forget** on them; nothing forgets
them on its own ([decision 6](#decisions-for-the-owner)).

For the open Performance the move is an edit: `edit.undo` puts the old version back with its
state, as the undo history holds a removed plugin now (RFC 0021's audition), and **Save** writes
the move; nothing goes in `Moves/`.

### Locked library items

A locked library's items are read-only to the musician, and replaced whole when its next version
installs (RFC 0031 Progress step 6: "replaces each item whose files differ"). So:

- **Moving them is the publisher's job:** a new library version whose items were moved and saved
  with the new plugin. A move by the musician would be overwritten by that update, or would have
  to fork the item.
- **The musician's own work is never stuck:** a part loaded from a locked Sound is a copy in the
  Performance ([RFC 0021](0021-sounds-performances-and-library.md#libraries)), so the
  Performance moves. A part just loaded from a locked item whose plugin has a newer installed
  version that claims to open it says so in the Parts view, with **Use 0.4.6** for that
  Performance.
- The plan lists locked items as "can't move" with their library's name, never silently skipped.
- **Save As New** of a locked item, then moving the copy, stays possible, as today for any edit.

For Oscairn's own libraries the owner is the publisher: the fix ships as a library version.

### Setlists

A Setlist entry names a Performance by its library ID only ([setlist format](../setlist-format.md));
it pins no revision. So moving a Performance moves it in every Setlist that names it, as saving
it does today. RFC 0031's "Rehearsed Setlists keep their pinned revisions" is not what the
format does; pinning revisions belongs to the multi-device setlists of RFC 0012 that will replace
the format, not to this RFC. What a Setlist adds here:

- **A scope:** **Use a newer version** from the Setlist moves the Performances it names (and
  only those).
- **Not during a show:** at `instrument` and `pass-through` ([RFC 0027](0027-reliability-levels.md))
  a move is refused while the transport runs ("Stop the transport to move sounds"), as RFC 0031
  refuses installs. On every level, a move of an entry the Setlist has prepared ahead drops that
  preparation, which the Setlist makes again from the moved document.
- The Setlist's **Check** (RFC 0031) lists entries that mix versions of one plugin.

### Comparing old against new

**Compare** is audition with the target version, so both stay loaded and the musician switches
between them on the same notes:

- **A Sound** (or one part of the open Performance): RFC 0021's audition already sets the part's
  chain aside, loaded and silent, while the tried chain plays in its place
  ([audition](0021-sounds-performances-and-library.md#audition)). Compare tries the same item with
  the moved nodes at the new version; **A** and **B** switch which chain takes the notes, without
  loading either again; **Keep B** is the move, **Keep A** stops.
- **A Performance:** audition opens it in place of the open one, which stays loaded and silent.
  Comparing a whole Performance is the same with both copies loaded.
- **Memory:** both versions are loaded at once, and cannot share an instance (below). Audition
  already checks the budget ([RFC 0025](0025-resource-budgets.md)): a Performance must fit beside
  the open one when a budget is set (`Session::auditionStart`, `core/control/src/audition.cpp`),
  and a Sound passes the same check as adding it (RFC 0021). Each plugin build is counted at its
  own measurement (`costOf`'s key is `<id>@<version>` and the build's
  digest, `core/control/src/budget.cpp`). When the pair does not fit, Compare is refused, naming
  both and the shortfall ("Comparing needs 180 MB more than is free"), and offers to compare one
  part instead of the Performance. Nothing is evicted to make room for a Compare.
- **On the device, without ears:** the plan's settings check above, and (later, step 4's open
  part) an offline render of a short phrase by each version in turn, one loaded at a time, so it
  needs no extra memory. Whether the engine can render a document offline outside the playing
  session is to check; `oscairn-plugin-check` renders a `Session` without a device
  (`core/control/tests/plugin_check.cpp`).

## The publisher's claim

### The fields

Two optional fields on each plugin in an archive manifest's `plugins`
([library archive](../library-archive.md)), beside `format`, `id`, `version`, `name` and `builds`:

```json
{"format": "clap", "id": "org.oscairn.analog", "version": "0.4.6", "name": "Oscairn Analog",
 "opensStateFrom": "0.4.0",
 "soundChanges": "Notes no longer stick when a chord is released during the glide.",
 "builds": [ … ]}
```

- **`opensStateFrom`** (replacing the name `restoresStateFrom`, since "restore" is CLAP's word for
  loading and says nothing about sound): the oldest version of the same format and ID whose saved
  state this version opens. It promises, for a state saved by any version from that one up to
  this one, published under the same publisher key:
  1. it loads without error;
  2. the audio and note ports are the same;
  3. every parameter keeps its ID, value and meaning, and nothing the state holds is dropped;
  4. it sounds the same as the version that saved it, **except as `soundChanges` says**.
- **`soundChanges`**: plain text, at most 1,000 bytes, American English, what may sound different
  from the older versions (the fixed bug). Absent: the claim includes the same sound.
- **Versions must be ordered:** both this version and `opensStateFrom` are dotted numbers (`0.4.6`,
  at most four parts), compared numerically, and `opensStateFrom` is not newer than this version.
  CLAP calls a version "an arbitrary string" (`clap/plugin.h`), so a plugin whose versions are not
  dotted numbers cannot make the claim; `oscairn-sign` and the index builder refuse it.

### Where it lives and how it is signed

| Place | What it holds | Signed by |
| --- | --- | --- |
| The archive manifest's `plugins` | Both fields | The publisher's package signature, `library.sig`, over the manifest's exact bytes ([signatures](../library-archive.md)). Build signatures (`signing::buildMessage`: format, ID, version, platform, digest) do not cover it |
| The repository index's `plugins` per version ([RFC 0031](0031-package-repository-and-distribution.md#the-index-must-say-which-plugins-a-version-holds)) | `opensStateFrom` only, so a lookup can offer a newer version before downloading; about 25 bytes when present | The repository's index key, repeating the manifest |
| The plugin store's `plugin.json` | Both fields and the key that signed them (`claimKey`) | Kept from the verified archive at install |
| A fork's `oscairn-release.json` (oscairn/packages' `schemas/plugin-release.schema.json`) | Both fields per plugin, passed to `oscairn-sign plugin-package` | The publisher, when the import workflow signs the archive |

The device trusts the claim only from the installed archive, never from the index alone: the index
decides what to offer for install, the manifest decides what may move. It keeps the claim only
when the archive's package signature verifies and its key is a key that signed the plugin's
builds in the same archive. **And it honors it only for the same publisher:** a node saved with
the old version moves under the claim only when the old version's build in the store carries a
signature by that same key (its `<bundle>.sig`), or, with the old build not installed, when a
repository's index lists the old version under the same publisher key. Otherwise the move is
offered as without a claim.

`oscairn-sign plugin-package` gains `--opens-state-from <id> <version>` and
`--sound-changes <id> <text>`.

### What the UI offers

| | With the claim | Without |
| --- | --- | --- |
| A stand-in for an old version, newer one in a repository (RFC 0031) | **Install and use 0.4.6** | Names 0.4.6 and says it doesn't say it opens sounds saved with 0.4.5; the Catalog still installs it |
| Moving one item or the open Performance | **Use 0.4.6**, with the plan | **Use 0.4.6** after the warning, with **Compare** first |
| Moving a library, a Setlist or everything | Offered | Not offered ([decision 3](#decisions-for-the-owner)) |
| After installing an update in the Catalog | **Update sounds using this (214)** | The count, with "doesn't say it opens them" |
| The plan's lines | The claim and its `soundChanges`, and how it was checked (below) | The warning |

A plugin installed by hand, or from a scan outside the store, has no manifest and so no claim.

### What a machine can check

**Today:** the import workflow in oscairn/packages (`scripts/import-release.py`, merged in PR #18,
run by `.github/workflows/import.yml`) plays each plugin of a fork's release through
`oscairn-plugin-check` on each platform's runner. `oscairn-plugin-check` plays a plugin in a part,
saves and reopens it, exports and installs it, and prints how far the reopened render differs
("not a failure": state that is not saved, such as an LFO's phase, makes renders differ)
(`core/control/tests/plugin_check.cpp`). It loads one build; it does not compare two versions.
The submissions repository's own Loading check is "Not built yet" (`check.yml`).

**Proposed:** `oscairn-plugin-check --state-from <old build> <old version>`: for each test state,
play and save it with the old build, then open the saved document with the new build and render
the same notes with each. The test states are the plugin's defaults, each `--param` setting given,
and the publisher's own (the Sounds of a library it names, `--states <archive>`; for Oscairn's own
plugins, their factory libraries). It reports per state: opened or not, ports, every parameter
read back, and the render difference in dB below the old render's level. The import workflow gains
a `state-check` step: for each plugin with `opensStateFrom`, every version this repository
publishes from that version up to the new one (`packages/<id>/*.json`, by `version_key`), each
fetched for the runner's platform and checked. It fails when a state does not open, ports change,
a parameter reads back differently, or, with no `soundChanges`, a render differs by more than
-60 dB; with `soundChanges`, render differences are reported on the pull request for the reviewer.
The index records that the claim was checked (`opensStateFrom` with `checked: true`), which the
plan shows ("checked by Oscairn Packages against 0.4.0 to 0.4.5"). Third-party submissions get
it with the Loading check.

**What it proves:** for the versions the repository has, and the states it played, on each
platform it ran on: the states open, keep their ports and parameters, and render the same notes
the same or within the level, or differently as the publisher said.

**What it does not prove:**

- states it did not play: a real musician's Sound can sit anywhere in a space no test set covers,
  and private state that parameters do not show is checked only through the render;
- sound under other notes, tempos, sample rates or block sizes than those played, or over time
  (the stuck note is exactly a bug a short render can miss);
- versions the repository never published (a user's own build with the same ID and version), or
  builds for platforms with no runner;
- that `soundChanges` is complete or true: a machine cannot tell an intended change from an
  unintended one;
- anything about a plugin with randomness, unless its renders are seeded the same.

That is why the device checks each item again (the plan's settings check) and the musician can
Compare, and why nothing moves on its own.

## Mixed versions and memory

Two versions of one plugin in one Performance are two separately loaded libraries (each its own
`dlopen` with `RTLD_LOCAL`, `core/clap-host/src/Internal.h`, or its own worker), and never one
instance: RFC 0021's shared engines need the same format, ID, version and path
([fewer instances](0021-sounds-performances-and-library.md#fewer-instances-reusing-what-is-loaded)).
Each costs its own memory, counted per build (`budget.cpp`). Whether two versions of a JUCE plugin
built with the same Objective-C class names load side by side on macOS without clashing is to
check; in workers they cannot clash.

What the musician sees:

- In the Parts view, a line when the open Performance uses two or more versions of one plugin:
  "Oscairn Analog 0.4.5 and 0.4.6 are both loaded (2 parts and 1 part): 96 MB more than one.
  **Use 0.4.6 for all**", offered as the move above (with the claim; without it, the line has no
  button). Informational: it never blocks, asks or appears on stage at `instrument` and above.
- `session.status` gives the mixed plugins (`mixedVersions`), so another client can show it.
- Moving one node of the open Performance offers the whole Performance by default, so a move does
  not make a mix.

Oscairn suggests aligning, but never aligns on its own: a mix may be on purpose (one part keeps
the old sound).

## Removing unused versions

### What keeps a version

A plugin version is **kept** while any of these uses it, reusing RFC 0009's retention roots for
assets:

| Root | Today |
| --- | --- |
| A library item, the musician's own or a locked library's | Read now for `plugins.list`'s `usedBy`, by reading every item's document on each call |
| The open Performance, its undo history and an audition's set-aside chain | `open` says whether the open Performance plays it; the undo history and audition are not counted yet |
| A Performance saved outside the library | `performance.save` records the folder for assets only (`assets_.addRoot`); it is to record plugins too, so a drive that is away keeps its pins, as for assets |
| A Setlist's Performances, and its prepared entry | By ID through the library, so a library item; the prepared graph, while prepared |
| A kept move (`Moves/`) | New: the version the move left |
| A kept-installed mark | New: **Keep installed** on a version, as RFC 0006 plans |
| The newest installed version of each plugin | New: never offered for removal, so new sounds have one |

What a device cannot see stays the musician's call, as RFC 0006 says: a Performance copied by
hand, an export on a USB drive, another device. Installed content says so ("Sounds saved outside
Oscairn may still need these").

### Where the musician sees them

Settings' **Installed content** page, built from `plugins.list` (in Settings' app, replacing
`notBuilt` in `Pages.cpp`):

- Each plugin once, with its versions, newest first; each version's size on this device, where it
  came from (`packages.installed`'s repository), and what keeps it: "Used by 12 Sounds, the open
  Performance", "Kept for undoing a move (2026-11-11)", "Newest", "Kept installed", or **Not used**.
- **Remove unused versions (3, 210 MB)** at the top, and **Remove** per version, each showing
  exactly what goes first. **Remove** on a used version is RFC 0006's deliberate removal: it lists
  what will open with a stand-in and asks, and is refused while the open Performance plays it (as
  `plugins.remove` is now).
- The kept moves, each with **Undo** and **Forget**.
- Never automatic, and no prompt after a move. RFC 0006's grouped prompt after deleting the last
  user of a version and its per-device retention setting stay RFC 0006's.

Reachability is computed again just before removing, so an item saved between the preview and
**Remove** keeps its version, which is then left out and said.

## Control API additions

New and changed, with permissions as `appGrants()` in `core/control/src/json_rpc.cpp` would give
them. Moving documents is `library`, as `library.save` and `library.delete` are; it installs and
removes no code. Removing plugins stays `system`, with `plugins.remove`. No method takes a file
path.

| Method | Takes | Returns | Permission |
| --- | --- | --- | --- |
| `versions.plan` (new) | `plugin` `{format,id,version}` (the version saved), `to` (installed), `scope`: `{items}`, `{library}`, `{setlist}` or `{all:true}` | `{job}`; one job at a time (`busy`) | `library` |
| `versions.move` (new) | As `versions.plan`, with `items` (the IDs to move, from the plan) and optional `withoutClaim` (one item only) | `{job}`; refused `playing` at `instrument` and above while the transport runs | `library` |
| `versions.job` (new) | `job` | `{state, items}`: per item `{id,name,kind,status,locked,nodes,changes,reason}`; `status` one of `movable`, `changed` (settings read differently, `changes` each `{node,param,name,before,after}`, `after` null when gone), `locked`, `noClaim`, `refused` (`reason`: no build here, the state did not open, ports changed, a time or memory limit), `moved`; and `claim` `{opensStateFrom,soundChanges,key,checked}` or null | `library` |
| `versions.moves` (new) | none | Kept moves: `{move,date,plugin,to,items,undoable}` | `library` |
| `versions.undo` (new) | `move` | Per item: put back, or `savedSince` | `library` |
| `versions.forget` (new) | `move` | `{accepted:true}` | `library` |
| `parts.useVersion` (new) | `plugin` `{format,id,version}`, `to`, optional `nodes` (absent: every node of it in the open Performance), optional `withoutClaim` | `{revision, changes}`; an edit, undone by `edit.undo` | `edit` |
| `audition.start` | Gains optional `versions`: each `{format,id,from,to}` | As now; refused when both do not fit, naming both | `library` |
| `audition.switch` (new) | `side`: `a` or `b` | `{side}` | `library` |
| `plugins.list` | Each version gains `opensStateFrom`, `soundChanges`, `claimKey`, `newest` and `keptBy` (each `{kind,name}`: `item`, `open`, `outside`, `move`, `kept`) | | `system`, as now |
| `plugins.keep` (new) | `format`, `id`, `version`, `keep` | `{accepted:true}` | `system` |
| `plugins.removeUnused` (new) | none: the preview; with `remove` (the versions from the preview) | Preview: each unused version and its bytes. With `remove`: per version removed, or kept and why | `system` |
| `session.status` | Gains `mixedVersions`: each `{format,id,versions}` | | as now |
| `packages.lookup` (RFC 0031) | Its candidates' `restoresStateFrom` becomes `opensStateFrom` | | `system` |

## Format changes

All before the first public release, so they break freely (the
[compatibility policy](../developer-guide.md#initial-development-compatibility-policy)):

- **Library archive:** `plugins[].opensStateFrom` and `plugins[].soundChanges`, optional. In the
  archive version that also brings `pluginRepositories` (RFC 0031 step 10) when they land together.
- **Repository index:** `plugins[].opensStateFrom` and `checked`, optional, in RFC 0031's
  per-version `plugins`.
- **Performance version 14, Sound version 9** (the versions RFC 0031 gives `pluginRepositories`):
  `plugin.stateSavedBy`, optional, a version.
- **The plugin store's `plugin.json`:** `opensStateFrom`, `soundChanges`, `claimKey`, `kept`.
- **New in the data folder:** `Moves/<move>/`, each with a `move.json` (`{plugin, to, date, items:
  [{id, kind, revision}]}`) and the documents as they were.
- **oscairn/packages:** `schemas/plugin-release.schema.json`'s plugins gain the two fields (its
  `additionalProperties: false` refuses them now).

## Security

| Threat | What stops it | What remains |
| --- | --- | --- |
| A false claim: a version says it opens old states and does not, or sounds different without saying | Nothing moves without the musician; the plan opens each item with the new version on the device first and lists settings that read differently; Compare; Undo keeps the exact old documents; the node's old state bytes are kept until the next save; the repository's state check where it runs; a false claim is grounds to yank (RFC 0031) | A claim true for the tested states and false for one musician's Sound; a difference only the ear finds. Undo is the remedy |
| A claim made by someone else: another publisher's package carrying the same plugin ID, claiming to open its states | Honored only when the old version's build is signed by the same key, or listed under it; one publisher per plugin ID in a repository (RFC 0031 step 9); the claim read only from a package-signed manifest whose key signed the builds | A publisher whose key is stolen; revocation (RFC 0031 step 4) |
| Downgrade: a move to an older version, perhaps one with a known flaw | Only versions newer than the saved one are offered, by `repository::older`; `opensStateFrom` newer than the version is refused; going back is only Undo, which restores the musician's own earlier document | A newer version that is worse; it is the musician's choice, with Compare |
| A hostile state from an old document crashing or exploiting the new version during the plan | Each item opened in a worker where workers run, with a time and memory limit, recycled, stderr captured; a failure refuses that item only | In-process plugins (workers off) share the engine's fate, as for any document opened today |
| A move as a weight: "everything" over thousands of items | One job at a time, cancelable, run off the control thread; at most 10,000 items per plan | Time; the job reports progress |
| Cleanup removing what is still needed | Never automatic; reachability recomputed before removing; the newest version kept; removal refused while played; unknown roots said | A Performance on a USB drive or another device; the musician is told, and RFC 0031 finds the version again |
| A move during a show | Refused while the transport runs at `instrument` and above; never on the open, prepare or change path | None found |

## Build steps

| Step | Deliverable | Gate |
| --- | --- | --- |
| 1. The claim | The fields in `oscairn-sign plugin-package`, the archive reader, the index builder, `plugin.json`, `plugins.list` | `control-archives`, `package-repository-index`: a claim survives packing, install and index; refused: a non-numeric version, `opensStateFrom` newer than the version, `soundChanges` over 1,000 bytes; a claim in an unsigned archive or one signed by a key that did not sign the builds is not kept |
| 2. Library moves | `versions.plan`, `move`, `job`, `moves`, `undo`, `forget`; `stateSavedBy`; `Moves/` | `control-library`, with the test synth at versions 1 and 2 (2 claiming 1): the plan lists a moved parameter's change; the move writes the version, the lock and `stateSavedBy` and leaves `nodes/` byte for byte; the item opens with 2, not a stand-in; Undo restores the document exactly; Undo leaves an item saved since; a locked item is listed and untouched; a version 2 that refuses the state, and one with other ports, refuse that item and move the rest; without a claim, more than one item is refused and one moves with `withoutClaim`; `playing` at `instrument`; an app with `edit` only is refused (`-32008`) |
| 3. The open Performance | `parts.useVersion`; `mixedVersions`; the Parts view's line | `control-parts`: two parts on version 1, one moved: two instances and `mixedVersions`; all moved: one; `edit.undo` puts 1 back with its state; Save drops `stateSavedBy` |
| 4. Compare | `audition.start`'s `versions`, `audition.switch` | `control-parts`: switching changes which version sounds without a new instance; over the budget, refused naming both; **Keep B** equals the move |
| 5. Checking the claim | `oscairn-plugin-check --state-from`; the import workflow's `state-check`; `checked` in the index | In oscairn/packages' tests: a version 2 that reads 1's state passes; one that drops a parameter fails naming it; a render difference fails without `soundChanges` and is reported with it |
| 6. Cleanup | Plugin roots for Performances saved outside the library, kept moves and keep marks; `keptBy`, `plugins.keep`, `plugins.removeUnused`; the Installed content page | `control-package`: a version used by an item, by the open Performance, by a Performance saved outside (its drive away), by a kept move and the newest are each kept; the rest removed only with `remove`; one used between preview and remove is kept and said. `desktop-gaps`: the page lists, removes and undoes |
| 7. The offers | The Library's **Use a newer version**, the Catalog's **Update sounds using this**, the Setlist's scope, RFC 0031's **Install and use** | `desktop-gaps`: each offer opens the plan; Cancel changes nothing |

## Decisions for the owner

1. **The claim's name and promise:** `opensStateFrom` (the oldest version whose state it opens:
   loads, same ports, every setting kept) plus `soundChanges` (what sounds different; absent means
   the same sound). Recommended. The alternative, a bare "loads" claim, is easy to check and tells
   the musician too little.
2. **Moving keeps the old state bytes** and changes only the version, the lock and
   `stateSavedBy`, until the next save. Recommended: lossless, cheap, exact undo. The alternative,
   re-saving every item's state through the new version during the move, proves more up front
   but loses the old bytes and costs a plugin save per item.
3. **Without a claim:** one item or the open Performance at a time, after a warning, never in bulk.
   Recommended. Stricter: never without a claim; that leaves third-party plugins' fixes out of
   reach.
4. **Locked library items** move only by their publisher's next library version, with the Parts
   view offering the move for the Performance a locked Sound was loaded into. Recommended. No
   per-device overlay over locked items.
5. **Setlists follow the Performance** by ID, as the format does today, and RFC 0031's "Rehearsed
   Setlists keep their pinned revisions" (its [updates](0031-package-repository-and-distribution.md#updates),
   and RFC 0021's "Rehearsed work stays put") is to be corrected to say that pinning comes with
   RFC 0012's setlists. Recommended. Not edited here.
6. **Kept moves stay until forgotten by hand,** keeping their old versions installed. Recommended,
   as nothing removes on its own. The alternative is a retention period, which is automatic
   removal by another name.
7. **The machine check** required for the claim in the import workflow for Oscairn's forks, and
   for everyone once the repository's Loading check runs; a render difference without
   `soundChanges` fails, with it is reported. Recommended, with -60 dB as the first level, to be
   tuned on real plugins.
8. **The newest installed version of a plugin is never offered for removal,** even unused.
   Recommended.

## Ruled out

- **Moving on its own,** on install, on update or on open, at any level, even with a checked claim
  (the owner's rule; RFC 0031 [ruled out](0031-package-repository-and-distribution.md#ruled-out)).
- **Converting state between versions in Oscairn.** The plugin reads its own older state or it
  does not; Oscairn does not translate it ([RFC 0006](0006-packages-and-portability.md#state-portability-and-format-migration)).
- **Picking a version by name or by "newest".** A move names the exact target, installed.
- **Two versions of one plugin sharing an instance.**
- **Removing versions without the musician,** including by a retention period.
