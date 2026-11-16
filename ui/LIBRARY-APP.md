# The Library app: inventory and checklist

Owner's decision (2026-11-15, `.trail/learnings/2026-11-15-port-a-whole-app-to-the-jacob.md`) and request
(2026-11-16: "the library app and finishing the performance app are more important than more ui stuff for the
engines"): the whole Library app of the JUCE program (app id `oscairn.library`, Home's tile "Library: Performances
and Sounds") is rebuilt in the Rust shell, and the tile opens the Rust app only when every screen of it works.
This file is the checklist, in the form of `ui/PERFORMANCE-APP.md`, whose "Owner requirements" sections are binding
on every screen here too (a theme for everything, no touch needed, reflow for any size, nothing at rest costs a
request or a redraw, the UI never waits on the engine, one cursor at a time, tabs are the shared family).
An item is ticked `[x]` in the commit that builds it, with its test.

Sources: `hosts/standalone/LibraryPane.cpp` and `.h` (the pane, the install, export and import dialogs, Save and
the ask-to-save question), `Catalog.cpp` and `.h` (the package Catalog the Library's Install opens),
`SigningKeys.h`, `PerformanceColors.h`, and what they use of `Engine.cpp`, `SoundPickerModel.cpp`
(`applySoundAction`: what a job sends) and `Main.cpp` (`showLibraryApp`, `libraryPane`, `showPicker`, the Home
line); RFCs 0021 (sounds, Performances and the library), 0006 (packages, signatures), 0009 (assets), 0023
(item types), 0025 (resource budgets: "will it fit"), 0031 (repositories and the Catalog); and the handlers of
`core/control/src/json_rpc.cpp`, read for every request below.

## State

Written 2026-11-16, branch `rust-library` from `rust-app` (adcf9952). The tile still hands the display over to
the JUCE app: nothing is switched. See the last section for what stands.

## How to read a row

**Ref** is where the JUCE code does it (`LibraryPane.cpp` unless a file is named). **Request** is what the JUCE
code sends for it, with the parameters as the real handler in `json_rpc.cpp` takes them (line numbers there in
the "Verified shapes" section); a request in `[brackets]` is one the JUCE app sends through a second connection
that waits its turn behind another's long request (`RemoteControl` with `waits`, Engine.cpp `refreshSounds`).
**No touch** is how it is done with the dial, the knobs under the screen and the buttons only: the border cursor
moves, a press confirms, Cancel (the hardware EXIT button) is Back. Touch always works as well. **FIRST CUT**
rows (`1`) are what a player does most, on stage and at home; **second cut** rows (`2`) are installing and
managing packages, libraries, repositories, signatures and storage. The first cut is built completely before any
of the second, and the tile is switched only when both are done.

Status: `[x]` built and tested in the Rust shell, `[ ]` not yet, `n/a` the JUCE app has no such thing or the
difference is recorded in the row.

## A. The screen and its list

| # | Cut | Item | Ref | Request | No touch | Status |
|---|---|---|---|---|---|---|
| A1 | 1 | The Library tile on Home opens the app; its line says what is installed: "5 instruments and effects · 0 Performances · 3 Sounds" (the first part only when plugin packages are installed, the last only when there are Sounds) | Main.cpp:556-562 (`libraryLine`), :45-50, :38-43 (`installedPluginPackages`) | `library.query {kind, limit:1}` for each kind (`total`); `packages.installed {}` (the kind `plugin` entries counted) | cursor on the tile, press | [ ] |
| A2 | 1 | The kind shown: Performances, Sounds, Setlists (three sibling views of different kinds: the shared tab family, the theme's `tabPlacement` decides strips or rails); choosing another clears the tags chosen, the search stays | :293, :342-354, :344 | `library.query {kind: "performance"\|"sound"\|"setlist", ...}` again | dial or left knob to the tabs, press | [ ] |
| A3 | 1 | The list of items, sorted by name or, with Recent, latest first: name ("Untitled" when empty), then the tags joined with ` · ` and the description; first "Locked" for an item of an installed library, "Does not fit this device" when the engine says `unfit`, "Auditioning" for the one being tried; the item's own color as the theme puts a row's; the star; a status line "N Performances" (`total`, singular for 1) | :676-719, :447 | `library.query {kind, limit}` -> `{items:[{id,kind,type,name,description,path,tags,favorite,recent,color,revision,hash,library,unfit?,readOnly?,inPlace?,fields?}],tags:[{tag,count}],total,tagColors:[{tag,color}],hiddenTags:[]}` | dial or left knob moves the cursor over the rows (the list fills the height and scrolls by view) | [ ] |
| A4 | 1 | Search: typed text narrows by name, description and tags (case-insensitive; the engine matches); the placeholder says "Search Performances by name, tag or description" | :296, :357-361, :398 | `library.query {text}` | Search on the top bar (or its list row), the on-screen keyboard | [ ] |
| A5 | 1 | Favorites: only the starred ones | :297-298 | `library.query {favorites:true}` | a switch among the filters | [ ] |
| A6 | 1 | Recent: only the ones used lately, the latest first (at most 50) | :299-300 | `library.query {recent:true}` | a switch among the filters | [ ] |
| A7 | 1 | Where from: Everything, My own, each installed library with its item count; a library uninstalled while shown makes it Everything | :459-462, :363, :438-441 | `library.query {source:"mine"\|<library id>}`; `library.libraries {}` -> `{libraries:[{id,name,items}]}` | filters list, press | [ ] |
| A8 | 1 | Tags with their counts over every match (the user's hidden ones left out), the chosen first then the most used; several chosen all must match; a tag's own color shown beside it; choosing one narrows | :463-470, :400-405, :542-576 | `library.query {tags:[...]}`; `tagColors`, `hiddenTags`, `tags` in the answer | filters list, press ticks | [ ] |
| A9 | 1 | The star on a row: a tap toggles the item's favorite, the list is asked again | :674, :726-731, :849 | `library.favorite {id, favorite}` -> `{accepted:true}` (no commit, no notification) | the star's tap, or the item menu's entry (C1) | [ ] |
| A10 | 1 | Compact rows (one line a row) and the left column's list or pills views, kept in `library-view.txt` | :365-385, :476-481, :576 | none (a file in the theme's configuration folder) | - | n/a: every Rust list row is one line a touch target tall (the shared list, as the JUCE dense/device form), the libraries and tags are rows of the Filters list as in the sound chooser (`sounds.rs`), so neither choice exists; recorded difference, `library-view.txt` is not read or written |
| A11 | 1 | Nothing of the kind and nothing narrowing it: one line saying so ("No Performances yet. 5 instruments and effects are installed, to start one with.", "No Sounds yet. The presets of the 5 instruments and effects installed are in the sound picker, not here.", "No setlists yet. The Setlist app makes them.") and a button "Browse the Catalog" | :52-58, :448-450 | none beyond A1's `packages.installed` | the button is a list row, press | [ ] |
| A12 | 1 | While the packages that come with the release install (the engine's first start), one line "Setting up the instruments that come with Oscairn…" and, when one failed, why; the list is shown again when it ends | :413-425 | `packages.preinstalled {}` -> `{state:"running"\|..., release, error, job, packages:[{id,name,version,state,error}]}` asked at 300 ms in the JUCE app; here once on opening and each second only while `running` | - | [ ] |
| A13 | 1 | The selected row stays selected across a new query (by item ID); the list is asked again after each action the pane takes (`refresh`) and the item's actions keep the cursor | :433-444, :36 | `library.query` again | - | [ ] |
| A14 | 1 | A detail panel for the item under the cursor (name, kind, tags, description, which library it is from, the revision, "Does not fit this device"): the shared list's read-only side. Not in the JUCE pane (it has no panel) | - | none beyond A3's answer | the right knob scrolls it | [ ] |

## B. Opening and trying

| # | Cut | Item | Ref | Request | No touch | Status |
|---|---|---|---|---|---|---|
| B1 | 1 | A press on a Performance opens it in place of the open one: the engine is asked what it would cost and whether it fits beside the open one (RFC 0025), the Performance is prepared while the open one plays when it would fit (`seamless`), then opened; the line says "Opening NAME", then "Performance opened. Press Play or play a note.", or why not; a Performance that would not fit beside the outgoing one opens as a cut, and says "Opened at once: it and the last Performance ringing out would not fit this device together." | :649-663; SoundPickerModel.cpp:98, :106-116; Engine.cpp:499 | `[library.query {id, kind:"performance"}]` (its `path`), `[budget.estimate {directory: path}]` -> `{cpu,memory,fits,seamless,longest,cores,uncertain,budget:{cpu,memory},plugins:[...]}`, `[performance.prepare {directory: path}]` -> `{prepared}`, `[library.load {id}]` -> `{loaded:true}` | press on the row | [ ] |
| B2 | 1 | While it loads the player sees that it does: the status line "Opening NAME..." with the seconds so far, the row marked, the top bar's "Engine busy 3.2 s" once past 400 ms, and the screen stays alive (a cursor move, Back, another tab all work: the engine answers "busy" within milliseconds to anything else, the UI asks again); a second choice while one loads replaces the one waiting (the last chosen is opened), never queued behind it | :657-662; Engine.cpp:402-420; docs/control-protocol.md "Long requests" | the chain of B1 on the UI's one connection; `-32009` (`data:{busy,ms}`) is "not now": asked again, never a lost connection | the same | [ ] |
| B3 | 1 | Before opening, unsaved changes to the open Performance are asked about: "Save changes to NAME before opening X?" with Save, Don't Save, Cancel (Esc); Save saves over it (a name is asked when it was never saved), a failed save is said ("Not saved") and the open does not go on | :150-169, :656 | `performance.info {}` -> `{id,name,description,tags,color,unsaved}`; `library.save {name?, new?}` -> `{id}` | three choices, dial and press, Cancel = Cancel | [ ] |
| B4 | 1 | A Sound is made a new part ("New part"): the status says "Adding NAME as a new part", then "Sound loaded." | :389-396; SoundPickerModel.cpp:125; Engine.cpp:501 | `parts.fromSound {id}` -> `{part}` (two undo steps, REAL-ENGINE.md 6) | the item menu (C1) and a row press on a Sound | [ ] |
| B5 | 1 | Audition: a Sound is tried in the part the Part screen shows, a Performance in place of the open one; the row says "Auditioning", the button reads "Stop" with "Keep" beside it; "Still loading; try again in a moment" while a job runs, "Show a part in the Part view to audition a Sound in it." with no part; Space (a computer key) does the same; Stop puts back what was there ("Put back as it was."), Keep makes it the change ("Kept.") | :610-647, :694, :598-600 | `[audition.start {id, part}]` (`part` only for a Sound) -> `{accepted}`; `[audition.stop {keep}]` | the item menu (Audition / Stop / Keep) | [ ] |
| B6 | 1 | While one is being tried, choosing another row tries that one; picking, canceling or leaving the screen stops it | :603-608, :732, :740 | `audition.stop` on leaving (the JUCE pane only when the pane goes) | press on another row | [ ] |
| B7 | 1 | An audition ended elsewhere (a Performance opened keeps it) is noticed and the marks go | :426-428 | `audition.get {}` -> `{id,kind,part}`, asked after a load or an audition request is answered and on `graph.changed` while one is on (the JUCE pane polls each 300 ms; here nothing is polled at rest) | - | [ ] |
| B8 | 1 | Add to setlist: a Performance is added as a slot at the end of the current page of the current setlist, "Added to NAME"; offered only where an app takes Performances | :665-672; Setlist.cpp:217-234 | `setlists.get {id}`, `setlists.save {setlist}` (the JUCE app asks the Setlist app process to do it: `apps.request`) | the item menu | [ ] |
| B9 | 1 | Pick mode: the pane answers another app's pick of one kind ("Choose", "Cancel"; a double click picks) | :653, :794; Main.cpp:639-665 | `apps.request`/`apps.answer` with the asking app | - | n/a: no app in the Rust shell asks the Library to pick (the Part's Sound chooser is `sounds.rs`, the Setlist app adds no slot from the Rust Setlist); B8 does what the pick for the Setlist did |

## C. What can be done with an item

| # | Cut | Item | Ref | Request | No touch | Status |
|---|---|---|---|---|---|---|
| C1 | 1 | The item menu (a right click, a long press or Options for the selected one): Open or Choose; Add to setlist; New part; Audition or Stop audition; Add to or Remove from favorites; Edit name, description and tags... (own, writable); Color (own, writable); Duplicate (own) or "Copy into my own, to edit" (a library's; not read only); Export... (not indexed in place); Delete... (not indexed in place). The enabling rules are `own = library empty`, `writable = own && !readOnly` | :811-865 | none of its own (each entry is a row below) | Options on the top bar acts on the item under the cursor; a press opens or loads (B1) | [ ] |
| C2 | 1 | Edit name, description and tags: three texts (tags separated by commas, trimmed, repeats dropped); "NAME saved."; an item of a library is refused by the engine | :867-891 | `library.edit {id, name, description, tags:[...], color}` -> `{accepted:true}` (color kept) | three keyboard entries then Save | [ ] |
| C3 | 1 | Color: None and Red, Orange, Yellow, Green, Teal, Blue, Purple, Pink, Gray (`#e5484d #f76b15 #ffc53d #46a758 #12a594 #3e63dd #8e4ec6 #d6409f #8b8d98`), the current one ticked | :857-863; PerformanceColors.h:12-26 | `library.edit {id, name, description, tags, color}` | a list of colors | [ ] |
| C4 | 1 | Duplicate (the user's own: a copy named "NAME copy") or Copy into my own (an installed library's: keeps its name), "NAME duplicated." / "NAME copied into your own, to edit." | :893-904 | `library.copy {id}` -> `{id}` | menu entry | [ ] |
| C5 | 1 | Delete, asked first: "Delete NAME?" "It is removed from your library. Performances that used it keep their parts." (own) or "It is removed. Installing its library again brings it back." (a library's) with Delete and Cancel; "NAME deleted." | :979-994 | `library.delete {id}` -> `{accepted:true}` | menu entry, then two choices | [ ] |
| C6 | 1 | A tag's menu: Hide from the tag list (a hidden tag chosen as a filter stops filtering), Color (None, the nine) | :1282-1300 | `library.tag {tag, hidden, color}` -> `{accepted:true}` | Options on the top bar acts on the tag under the cursor in the Tags list (a press there ticks it) | [ ] |
| C7 | 1 | Show a hidden tag again (the Libraries menu's "Show a hidden tag" list) | :1000-1002, :1075-1081 | `library.tag {tag, hidden:false, color}`; `hiddenTags` of A3 | Libraries menu (D9) | [ ] |
| C8 | 2 | Export an item to an archive file (`.olib`): a file is chosen (name by default the item's), then the choices dialog: for a Performance "With edited parts' Sounds" (samples and the Sounds of unedited parts always go), Signature: "Not signed" or "Sign with KEY" or "Make a key..."; Export; an existing file is replaced only once the new archive is written; "Exported." plus what was left out and "Signed with KEY." | :908-928, :60-129; Engine.cpp:436-445, :471-476; SoundPickerModel.cpp:134-136 | `[library.export {id, file, editedSounds, sign?:{key,items:true,archive:true}}]` -> `{exported:true,notes:[...]}`; `keys.list {}`, `keys.make {name}` | file browser (D3), the choices as list rows | [ ] |

## D. Installing and managing (second cut)

| # | Cut | Item | Ref | Request | No touch | Status |
|---|---|---|---|---|---|---|
| D1 | 2 | Install... offers exactly two ways: Catalog and File | :1169-1184 | none | a menu of two | [ ] |
| D2 | 2 | Install from a file (`.olib`, a library, an exported Performance or Sound, a plugin package, an asset package): its contents are read, and what it holds is asked about: who signed it (Signed by NAME (fingerprint), a key you trust / not a key you trust / which is revoked / Not signed), how (Into my Sounds and Performances, to edit; or As a locked library: a library archive defaults to locked, the others to own; a plugin or asset package offers no choice), which items (all ticked, under a heading per kind), the plugin builds it adds with who signed each, the license and its source (the commit or tag), a warning for builds from an unknown source ("These plugins run code from an unknown source:" ... ", unsigned" / ", signed by unknown key X") and for a revoked key ("Signed by a revoked key, so not installed:"); Install (or "Install anyway" when unknown code would be accepted), Cancel; "Choose at least one item." for none; "Installing..." then "Installed N, M already here. NOTE." or the engine's refusal ("Not installed") | :1135-1166, :171-243, :281-288; SoundPickerModel.cpp:137-143; Engine.cpp:450 | `library.contents {file}` -> `{id,name,kind,items:[{kind,id,name}],code:[{kind,name,version,platform,status,key,keyName,publicKey}],signer:{...},license,source:null\|{url,revision}}`; `[library.install {file, asLibrary, items, acceptUnsignedCode}]` -> `{added,skipped,notes,code}` | list rows with switches, the dial, Install | [ ] |
| D3 | 2 | The file chooser: open a `.olib`, save one with a name, choose a folder, choose a plugin build (`.clap` file or folder), the user's Documents folder first | :914, :967, :1053, :1112, :1137 | none: a JUCE file dialog | a list of folders and files with "Up", Cancel | [ ] |
| D4 | 2 | The Catalog (RFC 0031): the enabled repositories' packages holding Performances, Sounds, plugins or content, in four tabs (Performances, Sounds, Plugins, Content); each row: name and version, then its state (Update to V, Built in, Installed (a different build), Installed [with X], or why it cannot be installed here, withdrawn included), its publisher and summary; searched by name, id, summary, tag or publisher; one repository after another is read, "Reading NAME...", "From A, B." "Not read: C (why)."; opened on a tab with nothing it moves to the first with something; why nothing is listed in words; the plugin scanner finds the plugins that come with the app | Catalog.cpp:174-311, :371-390, :100-112, :41-98; LibraryPane.cpp:1188-1222 | `packages.repositories {}` -> `{repositories:[{address,name,key,id,origin,enabled}]}`; per enabled one `packages.list {source}` -> `{job}` then `packages.job {job}` every 100 ms -> `{job,state,error,repository,name,claimedName,rootKey,serial,packages:[{id,name,summary,license,publisher,publisherKey,tags,versions:[{version,channel,yanked,incompatible,sha256,size,installed,types,dependencies,withdrawn,exports}]}],package,version,installed:{added,skipped,notes,code}}`; `packages.installed {}` -> `{packages:[{id,version,kind,repository,sha256,signers,otherBuild,withdrawn,dependency}]}` | tabs, list, press opens | [ ] |
| D5 | 2 | A package's details: Summary, Version (channel, yanked), License, Installed (version; built in; a different build; with X), Publisher and key fingerprint, Repository, Size (to download, installed), Needs, "On this device" (why not), Withdrawn (who, why, "Update to V replaces it", "To remove it, use the Library's list of ..."), and the download addresses for each platform | Catalog.cpp:342-369, :313-331, :127-133, :27-32 | the answers of D4 | list of facts | [ ] |
| D6 | 2 | Install, Update to V, or Install the repository's build over a different one (asked first with both builds' archive hashes and signers: `replaceQuestion`); an install is a job in the engine asked about every 100 ms: "Installing ID VERSION...", "NAME VERSION installed. NOTES." and the list asked again, or "Not installed" with why | LibraryPane.cpp:1210-1220, :1224-1272; Catalog.cpp:114-125 | `packages.install {source, id, version, acceptUnsignedCode, replace}` -> `{job}` (`-32007` `busy` while another job runs); `packages.job {job}` | buttons as rows | [ ] |
| D7 | 2 | Code this device does not trust asks first (`codeQuestion`): signed by the key the repository lists as the publisher's: "Trust PUBLISHER to install plugins?" Trust and install / Cancel; signed by a stranger: "Install code from an unknown key?" Trust this key / Install anyway / Cancel; unsigned: "Install unsigned code?" Install anyway / Cancel; trusting writes the key for plugins beside whatever else it is trusted for, then installs again; nothing is trusted unless the user says so | :1244-1265; Catalog.cpp:135-172 | `trust.list {}` -> `{keys:[{id,key,name,scope,revoked}]}`, `trust.set {key, name, scope:[..,"plugins"], revoked}`, then `packages.install` again | a dialog of choices | [ ] |
| D8 | 2 | Libraries...: Export library... (the user's own items and the setlists chosen, with a name, to an archive file; "Library exported."), Folders indexed in place (each "Stop indexing PATH", "Index a folder in place..."), Show a hidden tag, Plugins, and "Uninstall NAME (N items)" for each installed library | :996-1092, :932-977 | `library.query {type:["oscairn.setlist"], limit:1000}`; `[library.exportLibrary {file, name, setlists:[ids]}]`; `library.folders {}`, `library.addFolder {folder}`, `library.removeFolder {folder}` (`[120 s]`); `library.libraries {}`; `plugins.list {}` | a menu with submenus | [ ] |
| D9 | 2 | Plugins installed with archives: each with "Used by ..." (or "Nothing in the library uses it"), "Builds: linux-arm64, ... (SIZE)", Add a build for another platform..., Remove from this device...; and, under "In this Performance", the open Performance's CLAP plugins installed some other way, for their other platforms' builds | :1013-1043, :1094-1131 | `plugins.list {}` -> `{plugins:[{format,id,version,name,platforms,bytes,usedBy,open,license}]}`, `graph.get` (the nodes' `plugin.path`); `plugins.addBuild {format,id,version,platform,path}`; `plugins.remove {format,id,version,platform}` | a menu then a file browser then a platform list | [ ] |
| D10 | 2 | Remove a plugin, asked first: "Remove NAME VERSION?" "Nothing in the library uses it. Every platform's build is removed." / "N item(s) use it. They keep its place and saved state and play silence until it is installed again."; "NAME removed." | :1094-1107 | `plugins.remove {format,id,version,platform:""}` | two choices | [ ] |
| D11 | 2 | Uninstall a library, asked first: "Uninstall NAME?" "Its Sounds and Performances are removed. Your Performances keep their parts and samples, and your own copies stay."; "NAME uninstalled." | :1083-1090 | `library.uninstall {library}` | two choices | [ ] |
| D12 | 2 | Make a signing key: its name (letters, digits, '.', '-', '_'; "The private key stays on this device."), then chosen to sign with; the error is shown ("No key made") | SigningKeys.h:22-39 | `keys.make {name}` -> `{name,id,key}`; `keys.list {}` -> `{keys:[{name,id,key}]}` | keyboard entry | [ ] |
| D13 | 2 | The fingerprint of a key as people read it (16 hex digits in groups of four) | SigningKeys.h:14-20 | none | - | [ ] |
| D14 | 2 | Import a Performance's parts and scenes into the open one (Pick mode's other user, a command of the JUCE Performance menu): another Performance is chosen, its parts and scenes listed all ticked ("Choose at least one part, and one scene: the scene sets how the parts come."), the chosen ones come with what they need ("Performance imported.") | :245-279; Main.cpp:676-701 | `performance.contents {directory: path}` -> `{parts:[{id,name,bus}],scenes:[{id,name}]}`; `[performance.import {directory, parts, scenes}]` -> `{parts,notes}` | from a Performance's item menu | [ ] |

## Items without a request

Going through every call of `LibraryPane`, `Catalog`, their dialogs and the jobs `Engine::soundAction` runs for
them, the JUCE app on the keyboard is `--connect`ed and reaches the engine only through requests. What is not an
engine request at all, and what the Rust shell does instead (the Rust shell can only use the control socket; the
engine is not changed):

- **File choosers** (D3): JUCE's `FileChooser`; the shell's own file browser over the local file system (`std::fs`,
  a list of folders and files with Up). The paths it makes are the engine's own machine's (one socket on this
  device).
- **Writing an export over an existing file** (C8, D8): the engine refuses to write over a file; the JUCE `Engine`
  writes beside it and renames once written (`Engine.cpp:436-445`, `:471-476`): the shell does the same with
  `std::fs`.
- **The plugin scanner** for the Catalog's "Built in" (D4): `oscairn-plugin-scan` over the engine's plugin folder
  (`scanPlugins`), as the chain's scanner (`scan.rs`) already runs it.
- **Opening a download address** in a browser (D5): `juce::HyperlinkButton`; shown as text, a computer's shell may
  open it, a device has no browser.
- **The audition's part** (B5): the JUCE window's `selectedPart()`; the Part screen's last part here.
- **Add to setlist** (B8): an `apps.request` to a Setlist app process the JUCE launcher starts; the Rust shell has
  no such process and its Setlist screen does not edit, so the slot is added in the setlist document itself
  (`setlists.get`, `setlists.save`).
- **`library-view.txt`** (A10): a preference file; not used.
- **Settings' repositories page** (D4's "Settings lists them"): the app that adds, removes and turns on package
  repositories is Settings, not the Library; the Library only reads them.

## Not in the Library app

The JUCE Performance window's menu bar has Import Performance (D14's flow), Export Performance (the open one,
`performance.export`, the same choices as C8), Install Library... (D2), Color, Save to Folder and Open Folder
(`performance.save`, `performance.load` with a directory); a device's shell has no menu bar. Install and Import
are rows above; the rest are the Performance app's or the desktop's.

## Verified shapes: the real handlers against docs/control-protocol.md

Read in `core/control/src/json_rpc.cpp` (line numbers of this tree) and `remote_control.cpp` for what the JUCE
client sends. Where the real engine differs from the doc it is said here and in the test fake.

(filled in as each is built)

## Where it stands

(filled in as rows are built)
