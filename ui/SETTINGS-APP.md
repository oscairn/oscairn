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
`OSCAIRN_UI_SETTINGS=1`, and the tests enter it directly. All 54 rows are built and ticked, and the first row of the new
page P (Installed, P1) too; the tile flips when the whole app is judged working on the device, which no one has done.
See the last section.

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
| Audio | D, and H where a device chooses | every profile whose `audio.device` is absent: a computer (no `platform.runtime`) and every built-in device profile but the keyboard's (the devices leave the choice to the engine); **not** the keyboard, whose profile fixes the device and its channels (the engine says `fixed: true`) so nothing on the page could be changed |

The reference shows Audio on every profile (and says "Fixed by the device profile" on the keyboard); the owner's
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
| S1 | 1 | U | Home's Settings tile opens the app (the line "Audio, controllers, keys"); its bar says `Settings`, then the page; Back goes Home; the tile hands over as before until the switch | `SettingsApp.cpp`; Home `Main.cpp` | none | press the tile; Cancel on the first level goes Home | [x] src/shell/settings_glue.rs `PORTED`, `OSCAIRN_UI_SETTINGS`; tests/settings_shell.rs `the_tile_opens_the_rust_app_only_when_it_is_switched_on` |
| S2 | 1 | U | The pages as the shared tabs (a strip or a rail by the theme's `tabPlacement`), in the order Appearance, Top bar, MIDI controllers, Audio, Package repositories, Signing keys, About (the reference's order with Appearance new, the player's most used first) | `Pages.cpp:25-44`, `Frame.cpp:131-218` | none | the dial or the left knob walks the tabs then the rows, a press on a tab shows it | [x] src/settings_app.rs, slint/settings.slint; tests/settings.rs `the_pages_are_the_profiles_tabs`, `a_tab_shows_its_page_by_touch_and_by_the_dial` |
| S3 | 1 | U | Which pages show for the profile (the table above); a page for hardware the profile lacks is not there | `Pages.cpp:19-24`, `Frame.cpp:117-121` (`shownPages`) | profile | - | [x] `Id::applies`, `Profile::audio_fixed`; tests/settings.rs `the_pages_are_the_profiles_tabs` (the keyboard's profile has no Audio; every other built-in device profile has it) |
| S4 | 1 | U | Every page's reads and writes are asked without waiting: the screen answers the dial while a request is out, a request the engine refuses as busy (`-32009`) is asked again, a listing that fails is said under the page ("could not be read: why") | `Engine::job` (`README.md` "Adding a page" 2) | all | - | [x] `Screen::reply` (busy: again after 300 ms), every page's listing error; tests/settings.rs `a_busy_engine_is_asked_again`, `a_fixed_device_is_shown_so_and_cannot_be_changed`, `a_refused_choice_is_said_under_its_port` |
| S5 | 1 | U | Nothing at rest costs a request or a frame: a page lists again only when the status says its list changed (`audioDeviceList`, `midiPortList`), never on a timer | `AudioPage.cpp:104-108`, `MidiControllersPage.cpp:145-148` (a 250 ms timer there) | `status` notification | - | [x] tests/settings.rs `a_page_asks_when_shown_and_nothing_at_rest`, tests/settings_shell.rs `at_rest_it_costs_no_request_and_no_frame` |
| S6 | 1 | U | Reflow at 1280x720 scale 1, the keyboard's 826x465 logical (1.55), 800x480 and large windows; long names shrink to 70% before they are cut | owner | none | - | [x] tests/settings_shots.rs `pictures_of_the_settings_app`: every page in 3 windows (826x465 logical, 1280x720, 800x480), 3 themes and 2 profiles, rows and cursor checked, pictures in target/screens/settings-*.png |
| S7 | 1 | U | The path in the bar reads `Settings › Page` (and the open sub-list); the page's own actions are the bar's | `Frame.cpp:178-186` | none | - | [x] `Screen::path`; tests/settings.rs `a_tab_shows_its_page_by_touch_and_by_the_dial`, tests/settings_second.rs `a_theme_is_installed_from_its_folder` |

## A. Appearance (new in the Rust app: the reference's page is `notBuilt`)

The choice and its install are `Theme::apply` and `Theme::install` (`Theme.cpp:329-352`), reached from
`PluginBrowser.cpp:15`. The choice is the configuration folder's `appearance.txt` (a theme's id; empty: the profile's
theme, then the built-in default), the installed themes are folders `themes/<id>/` beside it; both are shared with the
JUCE app, which watches `appearance.txt` once a second (`Theme::timerCallback`), so a change here shows in a JUCE
screen and the other way round.

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| A1 | 1 | U | The installed themes listed by name (the built-in folder, then the installed one; a folder whose name is not its id, one that fails to parse, or an installed one with a built-in's id, is left out) with the one in use marked | `Theme.cpp:353-361` `installed`; `PluginBrowser.cpp:33` | files: built-in `themes/` and `<config>/themes/*/theme.json` | rows of a list | [x] src/settings_look.rs `installed`; tests/settings.rs `the_installed_themes_are_listed_with_what_each_sets` |
| A2 | 1 | U | Choosing one applies it at once to every screen (they lay out again for its spacing, font scale, tab placement and styles) and stores it: the theme is read and checked first (fonts, files, sizes), then `appearance.txt` is replaced atomically with the id; a theme that cannot be read says why under the list and nothing is written | `Theme.cpp:329-331` `apply`, `:28-57` `write`; `PluginBrowser.cpp:15` | file `<config>/appearance.txt` | press on a row | [x] `Appearance::pick`; tests/settings.rs `choosing_a_theme_shows_it_and_keeps_it`, `a_theme_that_cannot_be_used_is_said`; tests/settings_shell.rs `a_chosen_theme_is_the_shells` |
| A3 | 1 | U | What the theme under the cursor sets, in the panel: its name and id, spacing, font scale, tab placement, button, tab and row styles, corner radius (a preview of the look before applying is not in the reference: the choice applies at once and is chosen again to undo it) | `Theme.h` `ThemeData`; owner's list | files | the right knob scrolls the panel | [x] the panel (`Appearance::info`), tests/settings.rs `the_installed_themes_are_listed_with_what_each_sets`; the reference has no preview of the look: n/a, the choice applies at once |
| A4 | 1 | U | With no choice made the theme in use is the profile's `screens[].theme`, else the built-in default; the list marks it as in use | `Theme.cpp:322-327` `defaultId`, `:367-372` `reload` | files, profile | - | [x] tests/settings.rs `the_installed_themes_are_listed_with_what_each_sets` |
| A5 | 1 | U | The theme chosen that cannot be used (removed, broken) falls back to the default and the page says why, as `Theme::error()` | `Theme.cpp:367-372`, `Theme.h` `error()` | files | - | [x] tests/settings.rs `a_theme_that_cannot_be_used_is_said` |
| A6 | 2 | U | Install a theme from a folder or its `theme.json`: read and checked as `load`, a built-in's id refused, the files it names copied into a staging folder renamed into place (an id already installed must be identical), then applied | `Theme.cpp:332-352` `install`; `PluginBrowser.cpp` `themeButton_` | files; the shell's file chooser | the file chooser (Library's) | [x] `Appearance::install`, the shared chooser (`library/files.rs`); tests/settings_second.rs `a_theme_is_installed_from_its_folder`, `a_theme_that_cannot_be_installed_is_refused` |
| A7 | 1 | U | A change of theme from outside (the JUCE app, another shell) is followed by the page: its list marks the new one | `Theme.cpp:367` | file read once a second (`theme::follow`) | - | [x] `theme_followed` (src/theme.rs `follow`, which no longer applies a theme already shown); tests/settings.rs `a_theme_chosen_elsewhere_is_marked_when_the_shell_shows_it` |

## T. Top bar

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| T1 | 1 | U | CPU: how the top bar shows it, Bar and percent (the default) / Bar / Percent / Hidden, with the line "How full the CPU is: the share of each audio block's time the engine takes." | `TopBarPage.cpp:8-31` | file `<config>/top-bar.json` `cpu` (`bar`, `percent`, `none`; Bar and percent leaves the key out, and an old `device` or no value reads as it) | press, then the choices | [x] `TopBarPage`; tests/settings.rs `the_top_bar_choices_are_stored_and_shown` |
| T2 | 1 | U | RAM, the same choices, "How much of the device's memory is in use." | `TopBarPage.cpp:32-34` | `top-bar.json` `ram` | the same | [x] the same |
| T3 | 1 | U | A change is stored at once (the file made, its folder created, replaced whole); when it cannot be written the page says "Not saved: PATH cannot be written." in the danger color | `TopBarPage.cpp:36-44` | `top-bar.json` | - | [x] tests/settings.rs `the_top_bar_says_why_it_was_not_saved` |
| T4 | 1 | U | The bar changes at once (the reference: the next time the Rust Home opens) | `docs/rust-ui.md` "What shows" | `shell::show_settings` | - | [x] `Ctx::meters`; tests/settings.rs `the_top_bar_choices_are_stored_and_shown`, tests/settings_shell.rs `a_choice_is_a_menu_over_the_screen` |

## D. Audio (kind D; H where the device chooses; not on a profile that fixes the device)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| D1 | 1 | D | The devices listed when the page opens: outputs, inputs where the engine opens inputs, each with its sample rates and buffer sizes; the open one chosen in each list; "Reading..." until then | `AudioPage.cpp:41-65`, `:67-77` | `audio.devices {}` | rows | [x] src/settings_audio.rs; tests/settings.rs `the_audio_devices_are_listed` |
| D2 | 1 | D | Output device: a choice among the outputs; disabled when there is one, when the profile fixes it, while a change is out | `AudioPage.cpp:51-52`, `:117-118` | `audio.set {output}` | press, then the list | [x] tests/settings.rs `each_audio_choice_is_one_request`, `a_fixed_device_is_shown_so_and_cannot_be_changed` |
| D3 | 1 | D | Input device: the row is there only when the engine lists inputs; "The audio inputs' device." | `AudioPage.cpp:45`, `:121-122` | `audio.set {input}` | the same | [x] tests/settings.rs `each_audio_choice_is_one_request`, `the_audio_devices_are_listed` (no row without inputs) |
| D4 | 1 | D | Sample rate of the open output, "Samples a second. A new rate restarts every plugin at it, keeping its sound." ("48000 Hz") | `AudioPage.cpp:46`, `:53-56` | `audio.set {sampleRate}` | the same | [x] tests/settings.rs `each_audio_choice_is_one_request` |
| D5 | 1 | D | Buffer size of the open output with its time ("128 samples (2.7 ms)"), "Smaller answers your playing sooner; larger is safer from dropouts." | `AudioPage.cpp:47`, `:57-60`, `:101-103` | `audio.set {bufferSize}` | the same | [x] tests/settings.rs `each_audio_choice_is_one_request` |
| D6 | 1 | D | The line under Output says where the device came from: "Chosen here; it opens again at the next start." (`user`), "Set when the engine started, by this device's profile or its options." (`profile`), "The system's default device." (`default`), "No audio device: started with --audio null." (`null`), "Fixed by the device profile: its channels are wired to this device." (`fixed`), and the engine's `notice` first (a choice passed over), in the danger color | `AudioPage.cpp:8-15`, `:110-116` | `audio.devices` `source`, `notice`, `fixed` | the right knob scrolls the panel | [x] tests/settings.rs `the_audio_devices_are_listed`, `a_fixed_device_is_shown_so_and_cannot_be_changed` |
| D7 | 1 | D | While a change is out every choice is disabled and the row says "Opening it..." (output, input) or "Restarting the audio device..." (rate, size); a second change is ignored | `AudioPage.cpp:80-88` | `audio.set` (30 s limit there; asked without waiting here) | - | [x] tests/settings.rs `while_a_change_is_out_the_page_waits` |
| D8 | 1 | D | A change refused, or a device that did not open (the last one plays again): the engine's reason is said under what was changed, in the danger color, and the devices are listed again as they are | `AudioPage.cpp:89-98` | `audio.set` error `-32005` | - | [x] tests/settings.rs `a_device_that_does_not_open_leaves_the_last_one` |
| D9 | 1 | D | The page lists again when the status says the device list, the device, its rate or its size changed (here or elsewhere) | `AudioPage.cpp:104-108` | `status` `audioDeviceList`, `audioDevice`, `sampleRate`, `bufferSize` | - | [x] tests/settings.rs `a_new_rate_is_shown_when_the_status_says_it_changed`, tests/settings_shell.rs `the_status_reaches_the_page_shown` |
| D10 | 1 | D | The listing fails: "The devices could not be read: why" under Output | `AudioPage.cpp:115` | `audio.devices` error | - | [x] tests/settings.rs `a_fixed_device_is_shown_so_and_cannot_be_changed` |
| D11 | 1 | D | **Protection against a bad choice**: the reference asks no question and has no timed revert; the engine opens a new device and, if it fails, opens the last again and refuses with the reason (D8); the profile's fixed device cannot be changed (D2); one change at a time (D7). Matched exactly, no looser path | `AudioPage.cpp`; `control.h:170-173` | `audio.set` | - | [x] no question, no timed revert: tests/settings.rs `a_device_that_does_not_open_leaves_the_last_one`, `a_fixed_device_is_shown_so_and_cannot_be_changed`, `while_a_change_is_out_the_page_waits` |

## C. MIDI controllers (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| C1 | 1 | U | Each MIDI input listed by name, with the controller profile that plays it and an Ignore switch; "No MIDI input is connected" when none | `MidiControllersPage.cpp:61-69`, `:100-127` | `controllers.inputs {}` | rows | [x] src/settings_controllers.rs; tests/settings.rs `each_port_has_its_profile_and_an_ignore_switch` |
| C2 | 1 | U | Profile of a port: Automatic ("Automatic (first that fits)"), None, each profile that fits, and "Each of these, by MIDI channel" where several fit (identical controllers); the current choice selected | `MidiControllersPage.cpp:92-115` | `controllers.choose {input, profiles:[ids]}` or `{input, automatic:true}` | press, then the list | [x] tests/settings.rs `a_ports_profile_is_chosen_from_a_list` |
| C3 | 1 | U | Ignore a port: everything it sends is left out; the profile choice is disabled while ignored | `MidiControllersPage.cpp:116-124` | `controllers.ignore {input, ignored}` | press toggles | [x] tests/settings.rs `ignoring_a_port_leaves_out_what_it_sends` (the Ignore switch is the row after its port's profile) |
| C4 | 1 | U | The line under each port says what it is: "Played by X (and the others chosen, by MIDI channel): its controls join.", "Read as Mackie Control by X.", "Left out: X sends the same on another port.", "No controller profile fits it: it plays notes as any MIDI input does.", "No profile: it plays notes as any MIDI input does.", "Ignored: everything it sends is left out." | `MidiControllersPage.cpp:15-26` | `controllers.inputs` `use`, `playing`, `ignored`, `profiles` | the right knob scrolls the panel | [x] tests/settings.rs `each_port_has_its_profile_and_an_ignore_switch` |
| C5 | 1 | U | A change is applied by the engine at once (kept, and the ports played by it); controls are disabled while it is out; a refusal is said under its port in the danger color and the ports are listed again | `MidiControllersPage.cpp:71-91` | `controllers.*` error `-32005` | - | [x] tests/settings.rs `a_refused_choice_is_said_under_its_port` |
| C6 | 1 | U | The page lists again when the status says `midiPortList` changed (inputs came or went, a choice changed elsewhere) | `MidiControllersPage.cpp:145-148` | `status` `midiPortList` | - | [x] tests/settings.rs `the_ports_are_listed_again_when_the_status_says_so`; tests/settings_shell.rs `the_status_reaches_the_page_shown` |
| C7 | 1 | U | The listing fails: "The inputs could not be read: why" | `MidiControllersPage.cpp:139-140` | `controllers.inputs` error | - | [x] tests/settings.rs `a_refused_choice_is_said_under_its_port` |
| C8 | 1 | U | **Protection against a bad choice**: none asked in the reference (a choice is kept by the engine and reversible on the same row); the shell's own panel is not a MIDI input, so no row here can turn it off. Matched exactly | `MidiControllersPage.cpp` | - | - | [x] no question is asked: tests/settings.rs `a_ports_profile_is_chosen_from_a_list`, `ignoring_a_port_leaves_out_what_it_sends` |

## R. Package repositories (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| R1 | 2 | U | Each repository by name with its address, the fingerprint of its root key, and what it is ("Oscairn's own: it can be turned off, never removed", "This device's: ...", "turned off: the Catalog reads nothing from it") and its On switch | `RepositoriesPage.cpp:11-22`, `:166-188` | `packages.repositories {}` | rows | [x] src/settings_repos.rs; tests/settings_second.rs `each_repository_is_a_row_with_its_fingerprint` |
| R2 | 2 | U | Turn a repository on or off; listed again as the engine kept it, a refusal said under it ("Not changed: why") | `RepositoriesPage.cpp:166-181` | `packages.enableRepository {address, enabled}` | press toggles | [x] tests/settings_second.rs `a_repository_is_turned_on_and_off` |
| R3 | 2 | U | Remove (a repository the user added only), asked first: "Remove NAME?" "What you installed from it stays. Its root key is no longer trusted for repositories." (Remove, Cancel); a refusal "Not removed: why" | `RepositoriesPage.cpp:183-203` | `packages.removeRepository {address}` | two choices | [x] the bar's Remove...; tests/settings_second.rs `a_repository_is_removed_after_the_question` |
| R4 | 2 | U | Add a repository by its address: typed on the keyboard; only `https://` is taken ("Not an https address: a repository is added by its https:// address, such as https://packages.example.org. To install from a file, use the Library's Install... > File.") | `RepositoriesPage.cpp:108-113`, `:143-150` | none | the on-screen keyboard | [x] the bar's Add... or the last row; tests/settings_second.rs `only_an_https_address_is_added` |
| R5 | 2 | U | Read first, so only a repository that verifies is added: listed as a package job, asked about every 100 ms until it ends; "Reading ADDRESS..." then "Adding ADDRESS..." | `RepositoriesPage.cpp:79-106` | `packages.list {source}` -> `{job}`; `packages.job {job}` | - | [x] tests/settings_second.rs `a_repository_is_read_before_it_is_added` |
| R6 | 2 | U | A root key not trusted yet is offered by its fingerprint: "Trust this repository?" "It calls itself \"NAME\", signed by the root key whose fingerprint is XXXX XXXX XXXX XXXX. Trust it only if its publisher gave you that fingerprint." (Trust, Cancel); "Not added: its root key was not trusted." on Cancel | `RepositoriesPage.cpp:107-130` | the job's `error` "not a repository this device trusts", `rootKey`, `claimedName`; the key's ID is the first 16 hex of the SHA-256 of its 32 bytes (`signing.cpp:61`) | two choices | [x] `key_id` (its SHA-256 checked against published vectors and the real engine's IDs); tests/settings_second.rs `a_keys_id_is_what_the_engine_says`, `a_root_key_is_trusted_only_when_the_player_says_so` |
| R7 | 2 | U | Trusted: `packages.addRepository` trusts the key and lists it, the repository is read again for its name, then kept under the name its root claims; one that still does not verify is taken out again, key and all | `RepositoriesPage.cpp:131-146`, `:94-98` | `packages.addRepository {address, key, name}`, `packages.removeRepository` | - | [x] tests/settings_second.rs `a_root_key_is_trusted_only_when_the_player_says_so`, `a_repository_that_does_not_read_is_not_kept` |
| R8 | 2 | U | Errors: "Not read: why", "Not added: why"; the Add row and the address field are disabled while a job is out; Cancel on the field goes back to the Add button | `RepositoriesPage.cpp:79-146` | the requests | Cancel | [x] tests/settings_second.rs `a_repository_that_does_not_read_is_not_kept`, `a_repository_is_read_before_it_is_added`, `only_an_https_address_is_added` |
| R9 | 2 | U | **Protection**: the key of a new repository is trusted only after the question with its fingerprint; a repository that does not verify is not kept (R7); a repository added over http is refused by the app though the engine takes it. Matched exactly | `RepositoriesPage.cpp` | - | - | [x] tests/settings_second.rs `a_root_key_is_trusted_only_when_the_player_says_so`, `a_repository_that_does_not_read_is_not_kept`, `only_an_https_address_is_added` |

## P. Installed (kind U; the reference's `notBuilt` "Installed content" independent, RFC 0006)

What is installed on this device, read once when the page is shown and nothing at rest (the engine's `status`
reports no change to what is installed, so a re-listing comes only from a change the page itself made, as on the
whole app). A refusal or a lost connection is the app's usual failed row.

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| P1 | 2 | U | Everything installed in one list, by name whatever the case (the id where two share a name), no heading rows: every package `packages.installed` reports, and any library only `library.libraries` lists. Each row is one package: its name (`name`, else its id), the word for its kind (Plugin, Samples, Library) and its version; what an archive held is not listed apart, and the count under the list is of items. A long name is cut with an ellipsis in its row, whole on the panel, which says the kind, id, version, a library's item count and the publisher (the repository it came from, or a file; the handler reports no trust flag, so none is shown); the list scrolls with the dial and the panel cursor, and at rest it asks nothing and draws nothing | `notBuilt` (`Pages.cpp:34`, "Installed content"); RFC 0006 | `packages.installed` `{packages}`; `library.libraries` `{libraries}` | rows; a press opens a row's detail (P3) | [x] src/settings_installed.rs; tests/settings_second.rs `the_installed_is_one_list_by_name`, `every_installed_row_is_reached_and_nothing_at_rest`, `a_refused_installed_request_is_a_failed_row` |
| P2 | 2 | U | Remove... on every row: `packages.removal` is asked first and its answer words the questions. Something uses it: "Used by: NAME (N items), N of your own" with Cancel / Remove anyway; else the plain Remove? / Cancel. Then, when it has dependencies nothing else uses: "Also remove what nothing else uses?", each with its size, Keep them / Remove them too / Cancel. Then one `packages.uninstall` (`evenIfUsed` after Remove anyway, `withUnused` the ids after Remove them too; a plugin package by its `version`), and the lists are read again, 200 ms apart for at most 5 s, until the package is gone; a refusal (the open Performance uses it, say) is shown as the engine words it and the row stays | RFC 0006, RFC 0009; docs/control-protocol.md | `packages.removal {id,version?}`; `packages.uninstall {id,version?,withUnused?,evenIfUsed?}` | the bar's action; the questions' choices | [x] tests/settings_second.rs `a_package_nothing_uses_is_removed_after_the_question`, `a_used_package_names_what_uses_it_and_offers_its_unused_dependencies`, `a_removal_is_followed_until_the_package_is_gone`, `cancelling_a_remove_removes_nothing`, `a_refused_removal_says_why_and_the_row_stays`; tests/settings_shots.rs `pictures_of_the_installed_page` |
| P3 | 2 | U | A package's detail on confirming its row: id, version, publisher and key fingerprint, license, the platforms or size if `packages.installed` reports them, and which installed libraries or Performances name it as a dependency, if the protocol gives it | RFC 0006 | the answer to `packages.installed` | a press on a row | [x] `press` opens the detail (Back closes it, on the same row); `packages.installed` reports no license, platforms, size or dependency list, so the panel shows the kind, name, id, version, a library's item count, the publisher and the key fingerprint of what signed it, and nothing more (what uses it is asked only by Remove, P2); tests/settings_second.rs `a_packages_detail_shows_what_the_engine_reports`, `back_closes_a_packages_detail_on_the_same_row` |

## K. Signing keys (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| K1 | 2 | U | The keys listed by name with their fingerprint (16 hex digits in groups of four); "No keys yet" when none | `SigningKeysPage.cpp:23-50` | `keys.list {}` | rows | [x] src/settings_keys.rs; tests/settings_second.rs `the_keys_are_listed_by_fingerprint` |
| K2 | 2 | U | Make a signing key: its name typed ("Its name, which only you see: letters, digits, '.', '-' and '_'. The private key stays on this device."), the key made and listed | `SigningKeysPage.cpp:12-20`; `SigningKeys.h:22-39` | `keys.make {name}`; `keys.list` | the on-screen keyboard | [x] tests/settings_second.rs `a_key_is_made_from_a_name`; tests/settings_shell.rs `a_name_is_typed_and_a_refusal_is_a_question` |
| K3 | 2 | U | A name the engine refuses is shown ("No key made" and why) | `SigningKeys.h:32-35` | `keys.make` errors `-32602`, `-32005` | - | [x] tests/settings_second.rs `a_key_is_made_from_a_name`; tests/settings_shell.rs `a_name_is_typed_and_a_refusal_is_a_question` |
| K4 | 2 | U | The listing fails: "The keys could not be read: why" | `SigningKeysPage.cpp:54` | `keys.list` error | - | [x] tests/settings_second.rs `the_keys_are_listed_by_fingerprint` |

## B. About (kind U)

| # | Cut | Kind | Item | Ref | Request or file | No touch | Status |
|---|---|---|---|---|---|---|---|
| B1 | 2 | U | Oscairn's version (the release) | `AboutPage.cpp:9`, `:15` | build constant | rows | [x] `About`; tests/settings_second.rs `about_says_what_this_is`; the release comes from `OSCAIRN_RELEASE` in the cargo build's environment (ui/CMakeLists.txt, not built here) |
| B2 | 2 | U | Source: the first 12 characters of the commit, with the address of the tree at it, or "Development build" with "Built from a working copy, not a release's commit." | `AboutPage.cpp:10`, `:16` | build constant | - | [x] tests/settings_second.rs `about_says_what_this_is` |
| B3 | 2 | U | The engine's version ("Reading..." then the release; "Unknown" and why in the danger color when it cannot be read) | `AboutPage.cpp:11`, `:18-27` | `packages.preinstalled {}` `release` | - | [x] tests/settings_second.rs `about_says_what_this_is` |
| B4 | 2 | U | The license, AGPL-3.0-or-later, in its words, and where the texts are | `AboutPage.cpp:12`, `:19-21` | none | the right knob scrolls the panel | [x] tests/settings_second.rs `about_says_what_this_is` |

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
| N5 | Installed content (RFC 0006): the packages installed, withdrawn ones, removal | `notBuilt`; the Library's Libraries menu has the plugin store and uninstalling (LIBRARY-APP.md D) | page P |
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

Read in `core/control/src/json_rpc.cpp` (line numbers of this tree) and asked of the real engine (built in this worktree,
`--audio null`, the keyboard's device profile, every folder private to the worktree, a socket in a private folder in /tmp: nothing of
this Mac's audio or MIDI was opened or changed). The test fake (`ui/src/tests/settings_fake.rs`) answers as these do: a request
carries the fields its handler takes and no others (`fields` refuses an unknown one with -32602 "Unknown or duplicate parameter
field"), and the errors have the words the real engine gave. A request from the Rust shell is a client of the socket, not an app
(`apps.connect`), so no permission rule applies to it (`permitted`, json_rpc.cpp:589-616).

| Request | Takes | Answers (and what it leaves) |
|---|---|---|
| `audio.devices` (:1307-1334) | none (`{}`) | `{outputs:[{name,sampleRates:[n],bufferSizes:[n]}],inputs:[same],output,input,sampleRate,bufferSize,source,fixed,notice}`; `source` is `user`, `profile`, `default` or `null` |
| `audio.set` (:1307-1334) | any of `output`, `input`, `sampleRate` (an **integer**: `integer(v)`), `bufferSize` | as `audio.devices`; `{}` changes nothing and answers the list; a choice not offered is `-32602` with what is offered; a device that fails to open is `-32005` and the last one plays again |
| `controllers.inputs`, `.choose`, `.ignore` (:1910-1945) | none; `input` and one of `profiles` (ids, empty for none) or `automatic:true`; `input`, `ignored` | `{inputs:[{name,profiles:[{id,name}],automatic,chosen:[id],playing:[id],use,ignored}]}`; the same list after a change |
| `keys.list`, `keys.make` (:1124-1135) | none; `name` | `{keys:[{name,id,key}]}`; `{name,id,key}` |
| `packages.repositories` (:1071) | none | `{repositories:[{address,name,key,id,origin,enabled}]}`; `origin` `official`, `device` or `user` |
| `packages.enableRepository`, `.removeRepository`, `.addRepository` (:1078-1092) | `address`, `enabled`; `address`; `address`, `key`, optional `name` | `{accepted:true}` |
| `packages.list`, `packages.job` (:1038-1049, :1094-1122) | `source`; `job` | `{job}`; `{job,state,error,repository,name,claimedName,rootKey,serial,packages,package,version,installed:{added,skipped,notes,code}}`, `state` `running`, `done` or `failed` |
| `packages.preinstalled` (:1063-1070) | none | `{state,release,error,job,packages}`; Settings' About reads `release` |

What the real engine did that the doc or the handler's reading does not say, or differs from the doc:

- **A new sample rate shows later than the answer to the `audio.set` that asked for it.** On the null device (the only one
  that could be tried) `{sampleRate:44100}` from 48000 answered `sampleRate` 48000, `audio.devices` and `session.status` asked at
  once and for the next seconds said 48000, and the engine's log said "audio now at 44100 Hz" later still, at the next
  `audio.set` (a buffer size's) the answer had 44100. Requests sent in quick succession saw rates that lagged by one change
  (a set to 48000 answered 44100, the previous request's). A buffer size's answer is right at once. The doc says "As
  `audio.devices`, after applying it" and "A change reaches `status` subscribers as `audioDevice`, `sampleRate` and
  `bufferSize`". The page shows the answer and lists again when the status changes, so it shows the new rate once the engine
  has it; the fake answers so (`pending_rate`), and the harness checks the row against the engine after the status. Whether a
  real device lags the same way could not be tried.
- An error for a choice not offered is `-32602` (an `invalid_argument` the handler does not catch as `runtime_error`), not
  `-32005`: the null device says `--audio null fixes the audio device: null` for any output or input, and `999 is not a buffer
  size null offers (64, 128, 256, 512)`. `-32005` is for a device that does not open (the doc's `Broken did not open: ...`).
- `audio.set {output:"null"}` (the device already open) makes `source` `user`: the choice is kept in the data folder's `audio.json`.
- With `--audio null` (the harness) the null device is `fixed: true` and lists one output, rates 48000, 44100, 88200 and 96000 and
  sizes 64 to 512, no inputs; its rate and size can still be chosen.
- `controllers.inputs` lists only connected inputs; with no MIDI device (`--audio null` has none) it is `{inputs:[]}`.
  `controllers.choose` and `.ignore` of a name not connected are accepted and kept (`{inputs:[]}`); an unknown profile is `-32602`
  "No controller profile is named nonesuch"; neither or both of `profiles` and `automatic` is `-32602` "Give the profiles, or
  automatic: true".
- `keys.make` refuses a name with `-32602` "a key's name is letters, digits, '.', '-' and '_', at most 64, not starting with '.'" or
  "there is a key named X already", and `{}` "Expected a string".
- `packages.list` while another listing runs is **`-32007`** "a package job is running; wait for it to finish" with
  `data.reason` `busy` (a `logic_error`), not the `-32009` of a long request: the reference shows it as "Not read: ..." and so does
  the Rust page. A listing that cannot reach its address ends `failed` with "not a usable repository: cannot fetch
  repository.json: cannot reach https://127.0.0.1:1/none/repository.json: Could not connect to the server." after a few seconds.
  An unknown job is `-32602` "no package job 99".
- A root key not trusted is a failed job whose `error` begins "not a repository this device trusts: its root key is ID; trust
  that key for repositories to use it" (repository.cpp:365) and which carries `rootKey` and `claimedName`; the ID is the one
  `signing::keyId` makes (the first 16 hex digits of the SHA-256 of the 32 bytes of the key), which the shell computes itself
  for the question (it was checked against the real engine's own IDs: key 6a7eceba...0841 is 6a2dd1acf4d4e23c, and a key
  made by `keys.make` 30fa7828...1ed2 is 89074b634c0de731).
- `packages.enableRepository` of an unknown address is `-32602` "no repository X on this device"; `packages.removeRepository` of the
  official one is `-32602` "the official repository can be turned off but not removed"; `packages.addRepository` with a key that is
  not 64 lowercase hex digits is `-32602` "a repository's root key is 64 lowercase hex digits".
- Not asked of the real engine, so the fake's wording for them is the handler's reading or invented, and said so here: an output or
  input device not offered on an engine that does not fix its device ("X is not an output device"), a rate not offered (modeled on the
  buffer size's), and the whole trust path of a repository (no repository server can be reached from the harness).

## Where it stands

54 rows: S 7, A 7, T 4, D 11, C 8, R 9, K 4, B 4; first cut 36 (S, A except A6, T, D, C), second cut 18 (A6, R, K, B);
by kind 43 universal, 11 desktop-OS (the Audio page) and none hardware-specific; plus 8 rows of N, all n/a.

2026-11-16: all 54 are built and ticked, against a fake engine that answers as the real handlers do, and the real-engine harness
(`real_engine/settings.rs`, ignored by default, run only against an engine built in the worktree with its own folders and
`--audio null`) has run every page twice, once with the keyboard's profile and once with a computer's, with no finding of the
Rust side. The first cut and the second were built together, in one commit, because the pages share the frame; the
checklist is the record of which rows are which. Home's Settings tile still hands the display to the JUCE app: the owner's rule is
that the tile flips when the whole app is judged working on the device.

What the real engine could not show, said here and not hidden: with `--audio null` the audio device is fixed, so choosing an
output or input device was exercised only against the fake (and its refusal against the null device's own words); no MIDI input
exists, so a port's rows and menus were exercised only against the fake (the harness checked the empty page and the
engine's refusals); no repository server can be reached, so the trust question, the add and the removal of a key that does not
verify were exercised only against the fake (the harness checked that an unreachable address is "Not read: ..." and that nothing
is kept); and what a real audio device does with a sample rate (the null device's answer lags, see above) was not seen. The
shell's `kms` build, the CMake change that gives About its release and commit, and a run on the keyboard were not done.
