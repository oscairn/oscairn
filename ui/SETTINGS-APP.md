# The Settings app: inventory and checklist

Owner's decision (2026-11-15, `.trail/learnings/2026-11-15-port-a-whole-app-to-the-jacob.md`): an app moves to the Rust
shell as a whole and Home's tile opens it only when every screen of it works. The order he gave: the Performance app,
the Library app, "after those apps do the settings app too". This file is the Settings app's checklist, in the form of
`ui/LIBRARY-APP.md` and `ui/PERFORMANCE-APP.md`, whose "Owner requirements" are binding on every screen here too (a
theme for everything, no touch needed, reflow for any size, nothing at rest costs a request or a redraw, the UI never
waits on the engine, one cursor at a time, tabs are the shared family). A row is ticked `[x]` in the commit that builds
it, with its test.

Sources: the JUCE Settings app, `hosts/standalone/apps/settings/` (`Pages.cpp` the one list of pages, `Frame.cpp`,
`AudioPage.cpp`, `MidiControllersPage.cpp`, `RepositoriesPage.cpp`, `SigningKeysPage.cpp`, `TopBarPage.cpp`,
`AboutPage.cpp`, `SettingsApp.cpp`, `README.md`), what they use (`Theme.cpp` and `Theme.h`, `SigningKeys.h`,
`Profile.h`, `PluginBrowser.cpp`'s theme chooser), RFCs 0024 (device profiles, the audio device and the controller
ports), 0031 (repositories), 0006 (signing), 0022 (the Settings app), 0036, 0037, 0039 (pages named and not built),
`docs/control-protocol.md`, and the handlers of `core/control/src/json_rpc.cpp`, read for every request below.

## State

Written 2026-11-16, branch `rust-settings` from `rust-library` (bca03b01). Nothing is switched: Home's Settings tile
still hands the display to the JUCE app (`PORTED` in `ui/src/shell/settings_glue.rs`); the Rust app is reached with
`OSCAIRN_UI_SETTINGS=1` while it is built, and the tests enter it directly.

## What the reference is

The JUCE Settings app is a program of its own (`oscairn.settings`, manifest `system` alone). Its pages, in the order
the list shows them (`Pages.cpp`): Audio, Package repositories, Signing keys, MIDI controllers, Top bar, About; each
page has `applies` (a profile test) and every one of these six is `everywhere`. Five more are named by RFCs and are
`notBuilt` (never shown, no class, no request in the engine): Appearance, Installed content, Storage, Network,
Bluetooth, Date and time (`Pages.cpp:34-43`). **The reference has no Appearance page**: where a player chooses a theme
today is the JUCE Performance window's "Plugins and appearance" (`PluginBrowser.cpp:15`, `:33`), which calls
`Theme::apply` and `Theme::install`. The owner's brief makes Settings the place for it, so the Appearance page here is
built from that code and from `Theme.cpp`, and is recorded as such (rows A*).

The frame (`Frame.cpp`): a list of pages and the page chosen, scrolling; wide with a mouse the first page is chosen at
once, narrow the list is a page of its own with a back button. Here the pages are the shared tab family on the shared
list screen (the Library's way), `tabPlacement` of the theme putting them in a strip or a rail.

## The three kinds, and where each page shows

Kinds: **U** universal (every Oscairn device and desktop), **D** per desktop OS (only a computer has it), **H** per
hardware target (only a given instrument has it). A page shows only where the device profile says it applies
(`ui/src/profile.rs`); hardware a product does not have is not shown at all.

| Page | Kind | Shown on |
|---|---|---|
| Appearance | U | every profile |
| Top bar | U | every profile |
| MIDI controllers | U | every profile |
| Package repositories | U | every profile |
| Signing keys | U | every profile |
| About | U | every profile |
| Audio | D, and H where a device chooses | every profile whose `audio.device` is absent: a computer (no `platform.runtime`) and the devices that leave the choice to the engine (`ignite`, `imx8mm-evk`, `kick`, `modx-m`, `qemu-aarch64`); **not** the FANTOM-07, whose profile fixes the device and its channels (the engine says `fixed: true`) so nothing on the page could be changed |

The reference shows Audio on every profile (and says "Fixed by the device profile" on the FANTOM-07); the owner's
rule (a page appears only where it applies) removes it there. Nothing in the reference is H: the pages the owner's
examples name (the panel, the audio routing, pedals, the boot behavior, Bluetooth) have no page, no request and no
file in the JUCE Settings app; they are listed under "Not in the Settings app" with what exists instead.

## How to read a row

**Ref** is where the reference does it (`apps/settings/` unless a file is named). **Request** is what it sends, with
the parameters as the real handler takes them, or the file it reads and writes; **No touch** is how it is done with
the dial, the knobs under the screen and the buttons only: the border cursor moves, a press confirms, Cancel (the
hardware EXIT button) backs out of an edit, then of the page. Touch always works as well. **Cut** `1` is what a player
changes most (appearance, the top bar, controllers, the audio device where there is one); `2` is the rest.
Status: `[x]` built and tested in the Rust shell, `[ ]` not yet, `n/a` the reference has no such thing or the
difference is recorded in the row.

## S. The frame

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| S1 | 1 | U | Home's Settings tile opens the app (the line "Audio, controllers, keys"); its bar says `Settings`, then the page; Back goes Home; the tile hands over as before until the switch | `SettingsApp.cpp`; Home `Main.cpp` | none | press the tile; Cancel on the first level goes Home | [ ] |
| S2 | 1 | U | The pages as the shared tabs (a strip or a rail by the theme's `tabPlacement`), in the order Appearance, Top bar, MIDI controllers, Audio, Package repositories, Signing keys, About (the reference's order with Appearance new, the player's most used first) | `Pages.cpp:25-44`, `Frame.cpp:131-218` | none | the dial or the left knob walks the tabs then the rows, a press on a tab shows it | [ ] |
| S3 | 1 | U | Which pages show for the profile (the table above); a page for hardware the profile lacks is not there | `Pages.cpp:19-24`, `Frame.cpp:117-121` (`shownPages`) | profile | - | [ ] |
| S4 | 1 | U | Every page's reads and writes are asked without waiting: the screen answers the dial while a request is out, a request the engine refuses as busy (`-32009`) is asked again, a listing that fails is said under the page ("could not be read: why") | `Engine::job` (`README.md` "Adding a page" 2) | all | - | [ ] |
| S5 | 1 | U | Nothing at rest costs a request or a frame: a page lists again only when the status says its list changed (`audioDeviceList`, `midiPortList`), never on a timer | `AudioPage.cpp:104-108`, `MidiControllersPage.cpp:145-148` (a 250 ms timer there) | `status` notification | - | [ ] |
| S6 | 1 | U | Reflow at 1280x720 scale 1, the keyboard's 826x465 logical (1.55), 800x480 and large windows; long names shrink to 70% before they are cut | owner | none | - | [ ] |
| S7 | 1 | U | The path in the bar reads `Settings › Page` (and the open sub-list); the page's own actions are the bar's | `Frame.cpp:178-186` | none | - | [ ] |

## A. Appearance (new in the Rust app: the reference's page is `notBuilt`)

The choice and its install are `Theme::apply` and `Theme::install` (`Theme.cpp:329-352`), reached from
`PluginBrowser.cpp:15`. The choice is the configuration folder's `appearance.txt` (a theme's id; empty: the profile's
theme, then the built-in default), the installed themes are folders `themes/<id>/` beside it; both are shared with the
JUCE app, which watches `appearance.txt` once a second (`Theme::timerCallback`), so a change here shows in a JUCE
screen and the other way round.

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| A1 | 1 | U | The installed themes listed by name (the built-in folder, then the installed one; a folder whose name is not its id, one that fails to parse, or an installed one with a built-in's id, is left out) with the one in use marked | `Theme.cpp:353-361` `installed`; `PluginBrowser.cpp:33` | files: built-in `themes/` and `<config>/themes/*/theme.json` | rows of a list | [ ] |
| A2 | 1 | U | Choosing one applies it at once to every screen (they lay out again for its spacing, font scale, tab placement and styles) and stores it: the theme is read and checked first (fonts, files, sizes), then `appearance.txt` is replaced atomically with the id; a theme that cannot be read says why under the list and nothing is written | `Theme.cpp:329-331` `apply`, `:28-57` `write`; `PluginBrowser.cpp:15` | file `<config>/appearance.txt` | press on a row | [ ] |
| A3 | 1 | U | What the theme under the cursor sets, in the panel: its name and id, spacing, font scale, tab placement, button, tab and row styles, corner radius (a preview of the look before applying is not in the reference: the choice applies at once and is chosen again to undo it) | `Theme.h` `ThemeData`; owner's list | files | the right knob scrolls the panel | [ ] |
| A4 | 1 | U | With no choice made the theme in use is the profile's `screens[].theme`, else the built-in default; the list marks it as in use | `Theme.cpp:322-327` `defaultId`, `:367-372` `reload` | files, profile | - | [ ] |
| A5 | 1 | U | The theme chosen that cannot be used (removed, broken) falls back to the default and the page says why, as `Theme::error()` | `Theme.cpp:367-372`, `Theme.h` `error()` | files | - | [ ] |
| A6 | 2 | U | Install a theme from a folder or its `theme.json`: read and checked as `load`, a built-in's id refused, the files it names copied into a staging folder renamed into place (an id already installed must be identical), then applied | `Theme.cpp:332-352` `install`; `PluginBrowser.cpp` `themeButton_` | files; the shell's file chooser | the file chooser (Library's) | [ ] |
| A7 | 1 | U | A change of theme from outside (the JUCE app, another shell) is followed by the page: its list marks the new one | `Theme.cpp:367` | file read once a second (`theme::follow`) | - | [ ] |

## T. Top bar

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| T1 | 1 | U | CPU: how the top bar shows it, As the device has it / Bar and percent / Bar / Percent / Hidden, with the line "How full the CPU is: the share of each audio block's time the engine takes." | `TopBarPage.cpp:8-31` | file `<config>/top-bar.json` `cpu` (`both`, `bar`, `percent`, `none`; the first choice leaves the key out) | press, then the choices | [ ] |
| T2 | 1 | U | RAM, the same choices, "How much of the device's memory is in use." | `TopBarPage.cpp:32-34` | `top-bar.json` `ram` | the same | [ ] |
| T3 | 1 | U | A change is stored at once (the file made, its folder created, replaced whole); when it cannot be written the page says "Not saved: PATH cannot be written." in the danger color | `TopBarPage.cpp:36-44` | `top-bar.json` | - | [ ] |
| T4 | 1 | U | The bar changes at once (the reference: the next time the Rust Home opens) | `docs/rust-ui.md` "What shows" | `shell::show_settings` | - | [ ] |

## D. Audio (kind D; H where the device chooses; not on a profile that fixes the device)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| D1 | 1 | D | The devices listed when the page opens: outputs, inputs where the engine opens inputs, each with its sample rates and buffer sizes; the open one chosen in each list; "Reading..." until then | `AudioPage.cpp:41-65`, `:67-77` | `audio.devices {}` | rows | [ ] |
| D2 | 1 | D | Output device: a choice among the outputs; disabled when there is one, when the profile fixes it, while a change is out | `AudioPage.cpp:51-52`, `:117-118` | `audio.set {output}` | press, then the list | [ ] |
| D3 | 1 | D | Input device: the row is there only when the engine lists inputs; "The audio inputs' device." | `AudioPage.cpp:45`, `:121-122` | `audio.set {input}` | the same | [ ] |
| D4 | 1 | D | Sample rate of the open output, "Samples a second. A new rate restarts every plugin at it, keeping its sound." ("48000 Hz") | `AudioPage.cpp:46`, `:53-56` | `audio.set {sampleRate}` | the same | [ ] |
| D5 | 1 | D | Buffer size of the open output with its time ("128 samples (2.7 ms)"), "Smaller answers your playing sooner; larger is safer from dropouts." | `AudioPage.cpp:47`, `:57-60`, `:101-103` | `audio.set {bufferSize}` | the same | [ ] |
| D6 | 1 | D | The line under Output says where the device came from: "Chosen here; it opens again at the next start." (`user`), "Set when the engine started, by this device's profile or its options." (`profile`), "The system's default device." (`default`), "No audio device: started with --audio null." (`null`), "Fixed by the device profile: its channels are wired to this device." (`fixed`), and the engine's `notice` first (a choice passed over), in the danger color | `AudioPage.cpp:8-15`, `:110-116` | `audio.devices` `source`, `notice`, `fixed` | the right knob scrolls the panel | [ ] |
| D7 | 1 | D | While a change is out every choice is disabled and the row says "Opening it..." (output, input) or "Restarting the audio device..." (rate, size); a second change is ignored | `AudioPage.cpp:80-88` | `audio.set` (30 s limit there; asked without waiting here) | - | [ ] |
| D8 | 1 | D | A change refused, or a device that did not open (the last one plays again): the engine's reason is said under what was changed, in the danger color, and the devices are listed again as they are | `AudioPage.cpp:89-98` | `audio.set` error `-32005` | - | [ ] |
| D9 | 1 | D | The page lists again when the status says the device list, the device, its rate or its size changed (here or elsewhere) | `AudioPage.cpp:104-108` | `status` `audioDeviceList`, `audioDevice`, `sampleRate`, `bufferSize` | - | [ ] |
| D10 | 1 | D | The listing fails: "The devices could not be read: why" under Output | `AudioPage.cpp:115` | `audio.devices` error | - | [ ] |
| D11 | 1 | D | **Protection against a bad choice**: the reference asks no question and has no timed revert; the engine opens a new device and, if it fails, opens the last again and refuses with the reason (D8); the profile's fixed device cannot be changed (D2); one change at a time (D7). Matched exactly, no looser path | `AudioPage.cpp`; `control.h:170-173` | `audio.set` | - | [ ] |

## C. MIDI controllers (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| C1 | 1 | U | Each MIDI input listed by name, with the controller profile that plays it and an Ignore switch; "No MIDI input is connected" when none | `MidiControllersPage.cpp:61-69`, `:100-127` | `controllers.inputs {}` | rows | [ ] |
| C2 | 1 | U | Profile of a port: Automatic ("Automatic (first that fits)"), None, each profile that fits, and "Each of these, by MIDI channel" where several fit (identical controllers); the current choice selected | `MidiControllersPage.cpp:92-115` | `controllers.choose {input, profiles:[ids]}` or `{input, automatic:true}` | press, then the list | [ ] |
| C3 | 1 | U | Ignore a port: everything it sends is left out; the profile choice is disabled while ignored | `MidiControllersPage.cpp:116-124` | `controllers.ignore {input, ignored}` | press toggles | [ ] |
| C4 | 1 | U | The line under each port says what it is: "Played by X (and the others chosen, by MIDI channel): its controls join.", "Read as Mackie Control by X.", "Left out: X sends the same on another port.", "No controller profile fits it: it plays notes as any MIDI input does.", "No profile: it plays notes as any MIDI input does.", "Ignored: everything it sends is left out." | `MidiControllersPage.cpp:15-26` | `controllers.inputs` `use`, `playing`, `ignored`, `profiles` | the right knob scrolls the panel | [ ] |
| C5 | 1 | U | A change is applied by the engine at once (kept, and the ports played by it); controls are disabled while it is out; a refusal is said under its port in the danger color and the ports are listed again | `MidiControllersPage.cpp:71-91` | `controllers.*` error `-32005` | - | [ ] |
| C6 | 1 | U | The page lists again when the status says `midiPortList` changed (inputs came or went, a choice changed elsewhere) | `MidiControllersPage.cpp:145-148` | `status` `midiPortList` | - | [ ] |
| C7 | 1 | U | The listing fails: "The inputs could not be read: why" | `MidiControllersPage.cpp:139-140` | `controllers.inputs` error | - | [ ] |
| C8 | 1 | U | **Protection against a bad choice**: none asked in the reference (a choice is kept by the engine and reversible on the same row); the shell's own panel is not a MIDI input, so no row here can turn it off. Matched exactly | `MidiControllersPage.cpp` | - | - | [ ] |

## R. Package repositories (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| R1 | 2 | U | Each repository by name with its address, the fingerprint of its root key, and what it is ("Oscairn's own: it can be turned off, never removed", "This device's: ...", "turned off: the Catalog reads nothing from it") and its On switch | `RepositoriesPage.cpp:11-22`, `:166-188` | `packages.repositories {}` | rows | [ ] |
| R2 | 2 | U | Turn a repository on or off; listed again as the engine kept it, a refusal said under it ("Not changed: why") | `RepositoriesPage.cpp:166-181` | `packages.enableRepository {address, enabled}` | press toggles | [ ] |
| R3 | 2 | U | Remove (a repository the user added only), asked first: "Remove NAME?" "What you installed from it stays. Its root key is no longer trusted for repositories." (Remove, Cancel); a refusal "Not removed: why" | `RepositoriesPage.cpp:183-203` | `packages.removeRepository {address}` | two choices | [ ] |
| R4 | 2 | U | Add a repository by its address: typed on the keyboard; only `https://` is taken ("Not an https address: a repository is added by its https:// address, such as https://packages.example.org. To install from a file, use the Library's Install... > File.") | `RepositoriesPage.cpp:108-113`, `:143-150` | none | the on-screen keyboard | [ ] |
| R5 | 2 | U | Read first, so only a repository that verifies is added: listed as a package job, asked about every 100 ms until it ends; "Reading ADDRESS..." then "Adding ADDRESS..." | `RepositoriesPage.cpp:79-106` | `packages.list {source}` -> `{job}`; `packages.job {job}` | - | [ ] |
| R6 | 2 | U | A root key not trusted yet is offered by its fingerprint: "Trust this repository?" "It calls itself \"NAME\", signed by the root key whose fingerprint is XXXX XXXX XXXX XXXX. Trust it only if its publisher gave you that fingerprint." (Trust, Cancel); "Not added: its root key was not trusted." on Cancel | `RepositoriesPage.cpp:107-130` | the job's `error` "not a repository this device trusts", `rootKey`, `claimedName`; the key's ID is the first 16 hex of the SHA-256 of its 32 bytes (`signing.cpp:61`) | two choices | [ ] |
| R7 | 2 | U | Trusted: `packages.addRepository` trusts the key and lists it, the repository is read again for its name, then kept under the name its root claims; one that still does not verify is taken out again, key and all | `RepositoriesPage.cpp:131-146`, `:94-98` | `packages.addRepository {address, key, name}`, `packages.removeRepository` | - | [ ] |
| R8 | 2 | U | Errors: "Not read: why", "Not added: why"; the Add row and the address field are disabled while a job is out; Cancel on the field goes back to the Add button | `RepositoriesPage.cpp:79-146` | the requests | Cancel | [ ] |
| R9 | 2 | U | **Protection**: the key of a new repository is trusted only after the question with its fingerprint; a repository that does not verify is not kept (R7); a repository added over http is refused by the app though the engine takes it. Matched exactly | `RepositoriesPage.cpp` | - | - | [ ] |

## K. Signing keys (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| K1 | 2 | U | The keys listed by name with their fingerprint (16 hex digits in groups of four); "No keys yet" when none | `SigningKeysPage.cpp:23-50` | `keys.list {}` | rows | [ ] |
| K2 | 2 | U | Make a signing key: its name typed ("Its name, which only you see: letters, digits, '.', '-' and '_'. The private key stays on this device."), the key made and listed | `SigningKeysPage.cpp:12-20`; `SigningKeys.h:22-39` | `keys.make {name}`; `keys.list` | the on-screen keyboard | [ ] |
| K3 | 2 | U | A name the engine refuses is shown ("No key made" and why) | `SigningKeys.h:32-35` | `keys.make` errors `-32602`, `-32005` | - | [ ] |
| K4 | 2 | U | The listing fails: "The keys could not be read: why" | `SigningKeysPage.cpp:54` | `keys.list` error | - | [ ] |

## B. About (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| B1 | 2 | U | Oscairn's version (the release) | `AboutPage.cpp:9`, `:15` | build constant | rows | [ ] |
| B2 | 2 | U | Source: the first 12 characters of the commit, with the address of the tree at it, or "Development build" with "Built from a working copy, not a release's commit." | `AboutPage.cpp:10`, `:16` | build constant | - | [ ] |
| B3 | 2 | U | The engine's version ("Reading..." then the release; "Unknown" and why in the danger color when it cannot be read) | `AboutPage.cpp:11`, `:18-27` | `packages.preinstalled {}` `release` | - | [ ] |
| B4 | 2 | U | The license, AGPL-3.0-or-later, in its words, and where the texts are | `AboutPage.cpp:12`, `:19-21` | none | the right knob scrolls the panel | [ ] |

## N. Not in the Settings app

What the owner's scope examples or an RFC name that the JUCE Settings app does not have, with no request to build
it on (`json_rpc.cpp` has no `network.*`, `bluetooth.*`, `drives.*`, `time.*` or `updates.*` handler; the engine is not
changed here). None is shown.

| # | Item | Why not | Status |
|---|---|---|---|
| N1 | Network (RFC 0026, 0039) | `notBuilt` in `Pages.cpp:41`, no request | n/a |
| N2 | Bluetooth (RFC 0036) | `notBuilt` (`#if OSCAIRN_BLUETOOTH`), no request; a product without it must not show it | n/a |
| N3 | Date and time (RFC 0035) | `notBuilt`, no request | n/a |
| N4 | Storage (RFC 0037) | `notBuilt`, no `drives.*` request | n/a |
| N5 | Installed content (RFC 0006): the packages installed, withdrawn ones, removal | `notBuilt`; the Library's Libraries menu has the plugin store and uninstalling (LIBRARY-APP.md D) | n/a |
| N6 | The panel, the audio routing, pedals and keys of a given instrument, boot behavior, window scaling | no page in the JUCE Settings app; routing and levels are the Performance app's Mixer and Routing; the profile fixes the panel; the boot choice is the start script's | n/a |
| N7 | The capture buffer's length (`<config>/capture`) | read by the engine's start (`SessionSetup.cpp:11`); no Settings page writes it | n/a |
| N8 | The trust store (`trust.list`, `trust.set`, `trust.remove`) | `Control` has it; no Settings page lists it (RFC 0037 puts it in Settings later) | n/a |

## Items without a request

What the JUCE Settings app does that is not an engine request, and what the Rust shell does instead (it can only use
the control socket and its own files; the engine is not changed):

- **The chosen theme** (`appearance.txt`), **the installed themes** (`themes/`) and **the top bar's choice**
  (`top-bar.json`): files in the configuration folder (`OSCAIRN_CONFIG_HOME`, else `~/Library/Application
  Support/Oscairn` or `~/.config/Oscairn`), shared with the JUCE app; the shell reads and writes them with `std::fs`
  (A1-A7, T1-T4). `Theme::apply` writes through a temporary file (`Theme.cpp:50-54`); so does the shell.
- **The theme install's file chooser** (A6): JUCE's `FileChooser`; the shell's own, over `std::fs`
  (`library/files.rs`).
- **A key's ID from its public key** (R6): `signing::keyId`, the first 16 hex digits of the SHA-256 of the 32 raw bytes
  of the key; no request gives it for the root key of a repository job (the repositories list does give `id`), so the
  shell computes it.
- **Oscairn's version and source commit** (B1, B2): JUCE compile definitions (`OSCAIRN_RELEASE`,
  `OSCAIRN_SOURCE_COMMIT`); the shell takes them from the environment of the cargo build, else its crate's version.
- **The page list and which pages apply** (S3): the app's own `Pages.cpp` and the profile; the shell has the profile.
- **The app's window and the shell's show request** (`SettingsApp.cpp`): a separate process there; one screen of the
  shell here.

## Verified shapes: the real handlers against docs/control-protocol.md

Filled in as each request is built (step 2): read in `core/control/src/json_rpc.cpp`, run against the real engine.

## Where it stands

54 rows: S 7, A 7, T 4, D 11, C 8, R 9, K 4, B 4; first cut 36 (S, A except A6, T, D, C), second cut 18 (A6, R, K, B);
by kind 43 universal, 11 desktop-OS (the Audio page) and none hardware-specific; plus 8 rows of N, all n/a.
Not started: 0 of the 54 are built.
