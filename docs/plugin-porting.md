# Porting a plugin to Oscairn

For developers bringing an existing CLAP plugin to Oscairn. A plugin that already follows
the CLAP specification usually needs **no changes** to load and play. This page lists what
the host requires, what it does not support yet, and the optional Oscairn extensions that
make a plugin fit the instrument better. LV2 plugins load too when Oscairn is built with
LV2 (see the [developer guide](developer-guide.md)); this page is about CLAP.

This page describes the current host and is updated as features land. Each fact below was
checked against `core/clap-host/src/Host.cpp`, `core/plugin-host/` and `core/control/`
unless it says otherwise.

## Start here

1. **Build for each platform you support:** `macos-arm64` for the desktop app on a current
   Mac, `linux-arm64` for the device. Keep the plugin ID and version the same on every
   platform, and keep everything the plugin needs inside its bundle.
2. **Pass the load checks** in [What the host checks](#what-the-host-checks). Most CLAP
   plugins already do.
3. **Implement the state extension**, deterministic and independent of CPU architecture.
   Without it a Performance keeps only parameter values.
4. **Make every sound-relevant setting a parameter,** with a clear name, a module path, value
   text and accurate stepped and hidden flags. Oscairn draws editors from the parameters
   everywhere ([Editing without the plugin's window](#editing-without-the-plugins-window));
   the plugin's own window opens on the desktop only ([The plugin's window](#the-plugins-window)).
5. **Add `oscairn.ui` hints and roles** so the right parameters land on the physical knobs,
   and views for envelopes, key maps and FM operators so they are drawn to drag
   ([Rich views](#rich-views-what-each-engine-should-declare)).
6. **Declare memory with `oscairn.memory`** if activating allocates a lot (delay lines, sample
   pools, reverb tables).
7. **Be real-time safe and say when you are silent:** no allocation, locks or file access in
   `process`; return `CLAP_PROCESS_SLEEP` when idle; treat a host `NOTE_CHOKE` as a short fade.
8. **Test** with `oscairn-plugin-check` and the production runner, with the plugin in a worker
   process and in the engine ([Testing](#testing)).
9. **Sign each build** with `oscairn-sign` and ship the `.sig` beside the bundle
   ([Packages and signing](#packages-and-signing)).

## What the host checks

A plugin that fails one of these is refused with the message shown.

| Requirement | Refused with |
| --- | --- |
| A `clap_entry` with a plugin factory | "No CLAP plugin factory" |
| `init` and `destroy` before `init`, and `init` returning true; once `init` returns true, `activate`, `deactivate`, `start_processing`, `stop_processing`, `reset`, `process`, `get_extension` and `on_main_thread`, as plugins built on clap-helpers fill them in during `init` | "Incomplete CLAP plugin interface", "Plugin initialization failed" |
| Audio outputs, note outputs or both (a pure tuner or meter has neither) | "Plugin has neither audio nor note outputs" |
| At most 64 audio channels in each direction across all ports; no port without channels | "Audio bus count exceeds the 64-channel budget", "Invalid or unsupported audio channel layout" |
| At most 16 note ports in each direction, each one described | "More than 16 note ports are unsupported", "Invalid CLAP note port" |
| If it has the params extension: `count`, `get_info`, `get_value` and `flush` (`value_to_text` and `text_to_value` are optional); every parameter with info, a unique ID and a finite minimum ≤ maximum; at most 32,768. A default outside the range is brought into it, and one that is not a number becomes the minimum: it only misplaces a reset | "Incomplete CLAP parameter interface", "Invalid CLAP parameter metadata", "Duplicate CLAP parameter ID", "Plugin advertises too many parameters" |
| If it has the state or audio-ports extension, all its functions | "Incomplete plugin state interface", "Incomplete CLAP audio-ports interface" |
| If it has `oscairn.memory`, `oscairn.sound-contexts`, `oscairn.sound-snapshots` or `oscairn.asset-dependencies`, all their functions; at most 256 sound contexts | "Incomplete memory interface", "Invalid sound-context interface", "Invalid sound-snapshot interface", "Incomplete asset-dependency interface" |
| With a memory budget, a declared size (`oscairn.memory`) that fits what is left | "Plugin needs N MiB of memory where M MiB is left" |
| A declared size a worker could hold: its three parts add up without overflowing, and in a worker to at most four times the policy's memory limit (1 TiB with none) | "Plugin's memory declaration is not credible" |
| Activates at the host's sample rate and maximum block size | "Plugin activation failed" |
| An audio port layout that does not change while loaded (parameter and note-port changes need a restart request) | "CLAP audio port layout changed; reload this graph node before reconnecting audio" |
| A build for the machine: macOS for the desktop, Linux ARM64 for the device | the scanner does not list it |

An `oscairn.ui` extension without `param_hint` is ignored, not refused.

## What the host gives you

- **Notes:** the host offers CLAP notes, MIDI and MPE, and converts at the plugin's
  boundary. A plugin that only reads MIDI, or only CLAP notes, works.
- **Hardware MIDI input:** from a keyboard or controller, a plugin gets notes, polyphonic
  aftertouch and channel pressure (as CLAP notes and the pressure note expression, or as MIDI),
  and, as MIDI, pitch bend and the performance controllers: CC 1 mod wheel, CC 2 breath,
  CC 4 foot, CC 11 expression, CC 64–67 (sustain, portamento, sostenuto and soft pedals),
  CC 120 all sound off and CC 123 all notes off. Other controllers and program changes do
  not reach plugins; physical-control mapping routes them. Clock, start, continue, stop and
  song position drive the transport. To hear pitch bend and the mod wheel, a plugin offers
  the MIDI dialect on its note port.
- **Hardware MIDI output:** a part can send its MIDI to hardware outputs too ([RFC 0012](rfcs/0012-sync-external-midi-and-multi-device.md#step-1-status)).
  A sequencer's or MIDI effect's note output reaches them as MIDI: notes, pressure (as
  aftertouch) and its MIDI events' channel messages; parameter events and other CLAP events
  stay inside. MIDI clock out follows the transport; a plugin does nothing for it.
- **SysEx between plugins:** a plugin may send `CLAP_EVENT_MIDI_SYSEX` on a note output, and
  receives it on a note input whose dialects include MIDI, at its sample and in order with the
  block's other events ([RFC 0032](rfcs/0032-arpeggiator.md#step-1-status)). A message must
  be whole (`F0` to `F7`, 7-bit data), at most 64 KiB, and a block carries at most 256 KiB:
  past either, `try_push` returns false and the message is dropped, never truncated. The host
  copies the bytes during `try_push`, so the plugin's buffer may be reused at once. SysEx does
  not reach hardware MIDI outputs or LV2 plugins.
- **Recording:** the engine records a plugin's output buffers and note events from outside
  the plugin ([RFC 0020](rfcs/0020-recording.md)), in a take or in the retroactive buffer that is
  always kept for Capture. The plugin is not told and needs nothing.
- **Audio:** 32-bit float buffers. CLAP requires every plugin and host to support 32-bit
  audio; 64-bit is optional (`CLAP_AUDIO_PORT_SUPPORTS_64BITS`) and Oscairn does not offer
  it, so no plugin is excluded by it.
- **Blocks:** any length up to the maximum, with events at their sample offsets; at most 512
  events from the graph per block.
- **Parameter values** set by Oscairn (on screen, by a control or a scene) arrive while the
  plugin processes as `CLAP_EVENT_PARAM_VALUE` events at sample 0 of a block. One played on a hardware controller
  (on its own, by a control, by a control's morph between scenes or by a scene button) arrives at the next block and may arrive again, the same value, a block or so later: take a
  repeated value as no change.
- **Render mode:** a plugin that offers CLAP's `clap.render` is told the mode with `set` once
  it is initialized, before it activates, on the main thread, at every load (a worker restart
  included) and for each effect context made from it: `CLAP_RENDER_OFFLINE` in offline renders
  (`oscairn-render`, a `Graph` or `Chain` made offline), `CLAP_RENDER_REALTIME` otherwise (the
  app, `Session`, `oscairn-plugin-check`), in the engine or a worker. A plugin whose
  `has_hard_realtime_requirement` returns true is told `CLAP_RENDER_REALTIME` in offline renders
  too, and still renders faster than real time. The mode does not change while a plugin is loaded.
- **Audio threads:** plugins that do not feed each other render at the same time
  ([RFC 0034](rfcs/0034-parallel-graph-rendering.md)): in the engine's process, `process` may
  run on the audio device's thread or on one of the engine's real-time helper threads, block to
  block, while other plugins render on the others; thread-check's `is_audio_thread` is true on
  each. One instance is never processed on two threads at once, and neither are effect contexts
  made from one instance. Keep nothing per process in globals shared between instances.
- **Host extensions:** thread-check, timer support, POSIX file descriptors, tail, log,
  latency, event registry, params, state, audio ports, note ports, preset-load,
  `oscairn.assets` and `oscairn.harmony`.
- **Presets:** a plugin's library that offers CLAP preset discovery has its presets listed in
  Oscairn's sound lists: presets inside the plugin, and preset files of its declared types in
  its declared folders, each only for the plugins it names, within limits ([Presets](#presets)).
  Each becomes an instant-switch sound: prepared in a native context, or loaded with the
  plugin's preset-load extension into a spare instance, never during `process`. Plugins that
  only offer presets in their own window have none listed. Presets are tested with a fixture
  plugin (`clap-presets`) and the Basic Synth (`control-sound-presets`); no third-party
  plugin's presets have been tried yet.

## Extensions to implement

### State (CLAP)

Implement `clap.state`. Saving a Performance, instant switching, crash recovery in a worker
and moving Performances between the desktop and the device all depend on it. Make it
deterministic and independent of CPU architecture, and keep ports, note dialect and parameter
metadata the same whatever state is loaded. In a worker process the host saves the state
right after every load, as the recovery record, so `save` must work straight after `load`.
Without the state extension, a Performance stores only parameter values, for plugins with up
to 2,048 writable parameters.

### Oscairn extensions

Headers are in `core/clap-ext/include/oscairn-clap/`, under the MIT license, so plugins of
any license can include them. Each is optional; a plugin without them works as above. For native
sound contexts, `sound-contexts-plugin.hpp` (also MIT) implements the extension's slot,
selection, voice-ownership and snapshot rules; an instrument supplies its patch and voice DSP,
as the Basic Synth and Legacy Sampler do. Its optional `voiceFinished(voice)` is called whenever a
voice ends (released out, choked, stolen or reset), so an instrument need not poll its voices.
A snapshot load that succeeds resets every context's pins and ends every voice: an instrument
that pins a context itself, as for a ringing tail, forgets those pins in `restored()`, which a
refused load never calls. `buildPreset(context, error)` runs with a preset's or a snapshot's
patch in place before the context is published, for an instrument that builds per-sound state. A plugin in its own repository vendors the folder at a
pinned commit. The drafts change without notice during initial development; rebuild against
the current headers.

| Extension | Why | Example |
| --- | --- | --- |
| Editor hints (`ui.h`, `oscairn.ui/draft-5`, [RFC 0019](rfcs/0019-adaptive-ui.md)) | Puts the right parameters first on small screens and on the physical knobs, and draws envelopes, key maps and FM operator views to drag. Details [below](#editor-hints-and-roles). | Basic Synth (cutoff and level, its envelope as an ADSR view), Delay (Wet is its `mix`) |
| Declared memory (`memory.h`, `oscairn.memory/draft`, [RFC 0025](rfcs/0025-resource-budgets.md#declarations)) | Lets a device near its memory limit refuse the plugin before its large allocation, and sets a tighter worker memory limit. Details [below](#declared-memory). | KICK memory test fixture |
| Native sound contexts (`sound-contexts.h`, [RFC 0005](rfcs/0005-sound-contexts.md)) | Multitimbral playing: up to 8 prepared sounds in one instance, each MIDI channel choosing one, held notes keeping theirs, without an extra instance per sound. New native Oscairn instruments implement it. | Basic Synth, Legacy Sampler |
| Snapshots prepared beside (`sound-contexts.h`, `oscairn.sound-snapshots/draft-4`, [RFC 0005](rfcs/0005-sound-contexts.md#preparing-a-snapshot-beside)) | Instant Performance changes: the next Performance's sounds for an instrument already playing are prepared in that instance, beside the playing ones, and selected at the cue, instead of loading its whole snapshot into an instance. Without it a native instrument is taken over whole, its snapshot loaded (for a sampler, its samples again). With its optional `ring_out` and `choke_retiring`, the outgoing sounds ring out through the outgoing Performance's effects and level instead of the incoming one's. Details [below](#instant-performance-changes). | Basic Synth |
| Voice budget (`voice-budget.h`, `oscairn.voice-budget/draft-1`, [RFC 0018](rfcs/0018-voice-budget-and-stealing.md#step-4-status)) | For instruments that can give up one voice of a note (a layer, an element, a unison copy): the host gives the instance a voice limit from the shared CPU budget, and each sound context its part's reserve and priority, and the instrument chooses which of its voices to give up, instead of the host fading whole notes. Details [below](#voice-budget). | Basic Synth; test fixture (`voice_fixture.cpp`, layered) |
| Native effect contexts (`effect-contexts.h`, [RFC 0007](rfcs/0007-effect-contexts.md)) | Many independent effect slots sharing one engine's memory, with tails reported. The next Performance's effects become new contexts of an engine already playing while the old ones ring out, so an engine outlives its first context and must refuse `create` past its capacity. Without `oscairn.memory`, the bytes its `resources` call reports are counted toward the memory budget. | Delay |
| Parameter-target lanes (`param-target.h`, [RFC 0002](rfcs/0002-param-target.md)) | For sequencers and modulators: change other plugins' parameters at an exact sample in the same block (parameter locks). | Step Sequencer |
| Free-running beat (`beat.h`, event space `oscairn.beat`, [RFC 0032](rfcs/0032-arpeggiator.md#step-3-status)) | For arpeggiators and clocks: a beat position the same for every plugin in a block that runs at the tempo while the transport is stopped, so parts keep time and start together. Query the event space through CLAP's event registry; an event arrives at sample 0 of every block. | Test fixture (`follower_fixture.cpp`) |
| Harmony (`harmony.h`, event space `oscairn.harmony` and host extension `oscairn.harmony/draft`, [RFC 0033](rfcs/0033-script-block.md#host-side-status)) | For harmonizers, chord memories and scripts: the part's key and scale, and the chord held on the part with its root, type, bass and keys. Query the event space for an event in the first block and at each change, at the sample of the note that made it; read the last on the main thread through the host extension. | Test fixture (`follower_fixture.cpp`) |
| Sound assets (`assets.h`, [RFC 0009](rfcs/0009-sound-assets-and-dependencies.md)) | For samplers: the host resolves sample files, and Performances list their samples so they can be checked and moved between machines. `oscairn.asset-dependencies/draft-2` adds `optional(index)`: an asset the plugin can do without (a fallback it embeds) does not stop a Performance loading when missing; the plugin's resolve of it fails and the host shows a notice. The host still accepts draft 1, every asset required. `oscairn.assets/draft-2` adds `library_root(owner, library_id, min_version, path, capacity)`: the read-only folder of an installed [asset library](library-archive.md#asset-packages) (`AssetLibraries/<id>/<version>/` in the data folder) when a version at least `min_version` is installed, else false. A preset-discovery provider gets it too: Oscairn's indexer answers `get_extension(OSCAIRN_EXT_ASSETS)` with a table whose `resolve` is null, so `provider->init`, which has an indexer and no `clap_host`, can declare the library's folder as a location (`owner` is the indexer, or the `clap_host_t` at runtime). The host still answers draft 1. | Legacy Sampler; asset libraries' preset providers (Oscairn Analog's factory patches) |

### Voice budget

With a CPU budget set, the host shares it among every plugin by measured cost (RFC 0018): for a
plugin without this extension it fades out and chokes whole notes, released and quiet ones
first. A plugin with `oscairn.voice-budget/draft-1` (`voice-budget.h`) gives up its own voices
instead, within a limit the host sets between blocks. What it implements, in
`oscairn_plugin_voice_budget_t` (every entry required):

- `honors_volume` (main thread): true when every voice follows `CLAP_NOTE_EXPRESSION_VOLUME`.
  Under pressure the host fades a held note of a plugin that says so before one that does not.
- `set_limits(limit, contexts, count)` (audio thread, never during `process`): keep at most
  `limit` voices from the next block (0: the whole pool), and per sound context its own `limit`,
  `reserve` (voices never taken for another context's), `priority` (0 to 10, lower gives way
  first) and `flags` (`OSCAIRN_VOICE_BUDGET_MONO`, `_LEGATO`). An entry for
  `OSCAIRN_VOICE_BUDGET_SELECTED` covers every context a MIDI channel selects; a context named by
  none has no limit or reserve and priority 0. Copy what you need; at the next block's start,
  fade out voices past a lowered limit; a new note past it takes a voice the same way. Fade what
  you give up (2 to 5 ms); cut only when the pool has no free slot, a voice already fading first.
- `active_voices(&cost)` (audio thread, after `process`): the voices sounding, fading ones
  included, and your estimate of one voice's cost in microseconds a block (0: none). The host
  measures your block's time either way and shares it per voice.
- `notes(out, capacity)` (audio thread, after `process`): each note still sounding, by the
  address the host gave it (note ID, port, channel, key), with its voices and its loudness now:
  the largest of its voices' gains, linear, 1 at full level (envelope, velocity and volume
  expression; not the patch's output level). The host compares notes across plugins by it.
- Send `NOTE_END` when a note's last voice ends, not each voice's.

The order to give voices up in: never a held voice of a context marked mono or legato, nor one
of a context at or under its reserve unless the new voice is that context's; released voices
before held ones; the new voice's own context first; lower priority first; the quietest, then
the oldest. `voice-budget-plugin.hpp` (MIT) implements it: keep a `voices::Limits`, describe
each voice as a `voices::Candidate` (context, selected, held, fading, mono, loudness, age) and
call `voices::victim`, which returns the voice to give up or -1; `voices::Report` collects
`notes`. An engine playing several regions or elements a note (a sfizz-style sampler) gives
each its own `Candidate` and loudness, so a quiet element goes before another note's only one.
An instrument built on `sound-contexts-plugin.hpp` gets all of it from its `Engine`: return
`oscairn_plugin_voice_budget_t{Engine::honorsVolume, Engine::setLimits, Engine::activeVoices,
Engine::voiceNotes}` from `get_extension`, and optionally provide `loudness(voice)` (default:
velocity, volume expression and choke fade), `mono(context)` (a sound that plays one note at a
time), `voiceCost()`, and `static constexpr bool volumeExpressions = true` when `render`
multiplies by `amplitude()`. The Engine also uses this order when its pool is full, with or
without the extension.

Parts sharing one instance ([below](#parts-sharing-one-instance)) get an entry each, for the
sound their channel selects, with the part's reserve and priority, so one pool across parts
keeps each part's reserve. An instance one part plays alone gets one entry, for the selected
contexts, which also covers a Performance change (the outgoing sounds ringing out beside the
incoming ones). No entry carries a limit of its own (the host keeps a part's note limit itself)
or mode flags (parts have no mono or legato mode yet). The host supports the extension
for plugins in the engine's process and in a worker process (RFC 0010) alike: in a worker,
`honors_volume` is asked once at load, and the limits and the report ride each block's shared
memory, with the same threads and timing. The engine keeps at most 512 notes of a report and
65,536 voices.

### Instant Performance changes

An instrument with native sound contexts switches Performances without a new instance or a state
load when it also offers `oscairn.sound-snapshots/draft-4` ([RFC 0021](rfcs/0021-sounds-performances-and-library.md#native-sound-contexts-across-a-change)).
On `sound-contexts-plugin.hpp` that is one table and one `get_extension` line:

```cpp
const oscairn_plugin_sound_snapshots_t soundSnapshots{Engine::prepareSnapshot, Engine::setRingOut, Engine::chokeRetiring, Engine::saveSound, Engine::soundValues};
// in get_extension:
if(std::strcmp(id, OSCAIRN_EXT_SOUND_SNAPSHOTS) == 0) return &soundSnapshots;
```

What the plugin must then do:

- `readPatch` runs on the main thread while the plugin renders, into a patch not yet published;
  it must not touch anything a voice uses.
- `buildPreset(context, error)`, if it has one, builds each prepared sound the same way; to be
  fast it reuses what other contexts hold (a sample already loaded) instead of loading it again.
- Enough contexts: the incoming sounds are prepared beside the playing ones (the snapshot's
  Default counts), and each outgoing sound is discarded only once it is idle. With too few free
  contexts the change falls back to loading the snapshot into an instance.
- Report idleness honestly: a sound with a voice or a tail must refuse `discard`, so it rings out.
- Ring out into the hidden buffer (`ring_out` with `choke_retiring`, optional together; [RFC 0005](rfcs/0005-sound-contexts.md#ringing-out-through-the-outgoing-chain)):
  while `Engine::ringOut` is set, `render` adds the main output of every context for which
  `Engine::retiring(slot)` returns two channels into those channels instead of the main output
  port; `Engine` clears it when each `process()` returns. Other ports are unchanged. A plugin built
  against draft 1 or 2 must be rebuilt against draft 3 (neither is read any more): add
  `Engine::chokeRetiring` to the table, and take the channels from `retiring(slot)`, which
  returns a pointer (null when not retiring) instead of a bool, rather than always writing
  `ringOut[0]` and `ringOut[1]`. Leaving both calls null keeps today's behavior, its tails
  playing through the incoming chain.
  With `ring_out`, three limits, which a plugin needs no code for beyond the SDK's but its users hear:
  - **One tail carries over.** A second Performance change while a first change's tail still
    rings cuts that tail: the host calls `choke_retiring` just before selecting the incoming
    sounds, and the SDK chokes every voice of a sound retiring then, with the 3 ms choke fade,
    rendering the fade into `ringOut[2]` and `ringOut[3]`, which the host still routes through
    the first change's chain. The second change's outgoing sounds take `ringOut[0]` and
    `ringOut[1]` and ring out through theirs. So no tail jumps to another chain, and no click,
    but a long tail (a held pad, an instrument's own delay) ends at the second cue. A plugin
    with tails outside its voices, which the SDK cannot choke, fades them itself in that
    context while `retiring(slot)` returns `ringOut + 2`.
  - **Main output only.** Only a retiring sound's main output port rings out through the
    outgoing chain. Its other output ports (the Basic Synth's sound buses, for example) keep
    playing through the instance's ports, which follow the incoming Performance's chain.
  - **30 seconds.** An outgoing effect fed only by the ring-out buffer goes once 30 seconds pass
    with no note through it. A note held through the change for longer loses the outgoing
    effects, and its tail moves back to the main output, through the incoming chain.

A plugin carried over whole into a Performance change (one the next Performance does not take
over) rings out until it is idle, but no longer than 30 seconds after the change with no note
held through it, or 2 minutes with one held; a panic ends it at once
([RFC 0021](rfcs/0021-sounds-performances-and-library.md)). A voice it never ends is cut there,
with the instance.

### Parts sharing one instance

Parts whose instruments are the same plugin share one instance, on their own, each with its own
sound, MIDI channel and output bus ([RFC 0021](rfcs/0021-sounds-performances-and-library.md#parts-sharing-one-engine)):
adding the plugin where an instance of it has room, opening a Performance, or changing to one
(whose sounds are then prepared beside the playing ones, as above, and selected at the cue on
channels of their own) all put a part's sound into an instance that has room; a part alone keeps
an instance of its own.
The host sends a part's notes and its parameter edits on its channel, and takes its sound from
its output port. A plugin offers this with native sound contexts and their snapshot (above), more
than one output port, no audio input and no note output, draft 4's `save_sound` and
`sound_values` (a part's sound saved as a snapshot of its own, for a Sound, and the values its
part shows; on the SDK, the instrument's `alone(Patch&)`, which puts a patch on the main output,
and `patchValues`, which lists a patch's values; [RFC 0005](rfcs/0005-sound-contexts.md#one-sound-on-its-own)),
and one stepped parameter, from 0,
whose value is the output port a sound plays on by the port's index (0 the main port), tagged
with oscairn.ui's `OSCAIRN_ROLE_OUTPUT_BUS` ([below](#editor-hints-and-roles)). The host sets it
per sound, with a parameter event on the part's channel, so it must follow per-channel parameter
events like any other (`sound-contexts-plugin.hpp`'s Engine does: a parameter event with a
channel edits the sound that channel selects). Voices keep the port they started on. An instance
holds as many parts as it has sounds, ports and channels; the host takes another instance past
that. The Basic Synth (eight sounds, its Default one of them; eight sound buses beside its main
output) does all of it; the Legacy Sampler has one output, so each Legacy Sampler part keeps its own instance,
as every plugin without native contexts does. Per-part voice budgets follow ([above](#voice-budget)).
What its users hear differently: the parts sharing an instance share its voice pool (the Basic
Synth's 128, which the shared voice budget rather than the pool limits in practice); a plugin
meant to share should have a pool sized for several parts.

### Presets

Publish presets through CLAP preset discovery, and Oscairn lists them where sounds are picked
and prepares any of them as an instant-switch sound ([RFC 0005](rfcs/0005-sound-contexts.md#presets)).
A plugin provides:

1. **The factory:** `clap_entry.get_factory(CLAP_PRESET_DISCOVERY_FACTORY_ID)` (the `draft-2`
   compatibility ID also works) returning a factory with one provider or more.
2. **A provider** that declares its locations in `init` (inside the plugin, or a file or
   folder with the file types it reads) and reports each preset in `get_metadata` with
   `begin_preset(name, load_key)` and `add_plugin_id` for the plugins it is for.
3. **A load-key scheme.** A preset is its location kind, location and load key. Inside the
   plugin, every preset needs a name and a load key; a file holding several presets names each
   and gives each a load key; a file that is one preset may give neither (it is named after the
   file).
4. **Loading by that triple:** for native sound contexts, draft 5's `prepare_preset` (the
   preset becomes a new context, from the plugin's defaults, while the playing sound plays
   on); otherwise CLAP `preset-load`'s `from_location`, which Oscairn calls on a spare
   instance. A native instrument without `prepare_preset` has its presets listed but refused.

The SDK does the protocol for an instrument on `sound-contexts-plugin.hpp`:
`preset-discovery-plugin.hpp` makes the factory and provider from `declare` (where the presets
are: `d.inside("Factory")`, `d.files("Patches", folder, "fxp")`) and `list` (the presets in
one location: `l.add(name, loadKey)`), and `Engine::preparePreset` implements `prepare_preset`
from one hook, `readPreset(kind, location, loadKey, patch, error)`: read the preset into a
`Patch`, which starts at the defaults. The Basic Synth is the reference: five factory presets
inside the plugin, load keys their names in lower case and hyphenated. What each fork should
provide (recommended; none is built yet):

| Plugin | Locations | Load key |
| --- | --- | --- |
| Oscairn Organ (setBfree) | Inside the plugin: the 55 programs | The program number, `0` to `54` |
| Oscairn Analog (Surge) | Files: the factory and user patch folders, type `fxp` | None: each file is one patch |
| Oscairn FM (Dexed) | Files: the cartridge folders, type `syx` | The voice in the cartridge, `0` to `31`, each named from the cartridge |

Limits: 4096 presets per plugin library, 64 locations and file types per provider, 10,000
files per folder and 16 folder levels, names up to 255 bytes, locations and load keys up to
4096; a preset over a size limit is skipped. A scan indexes presets in a separate helper
process with a 15-second timeout, and a listing for a loaded plugin reads them through that
same helper process (`oscairn-plugin-scan`, found beside the engine or the desktop app, or
named by `OSCAIRN_PLUGIN_SCAN`). The listing, or its failure, is kept per plugin binary until
the binary changes, so a provider that hangs costs one 15-second timeout, not one per listing;
a helper that could not be started is tried again at the next listing.
The helper runs off the caller's thread: until its listing is in, `presets.list` answers
`Presets are being listed; ask again`.
A plugin in a worker lists presets only through the helper: without one it lists none, and
`presets.list` says why. Only a plugin hosted in the engine's own process with no helper
configured has its provider run there, on the control thread, as the rest of that plugin
does. `get_metadata` must be fast and never interactive, as CLAP says.
Nothing watches preset folders, and the listing is kept, so preset files added later show once
the plugin's binary changes or the engine restarts. What a Sound saves is the plugin's
own state (for native contexts, the snapshot of every prepared patch), never a reference to the
preset, so it opens when the preset is gone.

### Editor hints and roles

`oscairn.ui` gives a hint per parameter, asked when the host reads parameter info (at every
activation and after an info rescan):

- **The control it wants:** knob, slider, buttons, switch, list, or a range with its high
  parameter. The host keeps a hint only where it fits (buttons and lists on a stepped
  parameter, a switch on two values, a range whose high end exists); otherwise it decides.
- **A priority,** 1 first. It orders the parameter's tiles, the Compact list, and the
  physical knobs: a device's screen knobs play an editor page's first parameters in this
  order; a part with no controls of its own whose roles fill no bank gets two Assign banks
  from its instrument's first parameters in this order; an effect without a `mix` role has its
  first continuous parameter (not `CLAP_PARAM_IS_STEPPED`, such as an EQ's Mode) as its knob in
  the Effects bank, or its first parameter where all are stepped
  ([RFC 0011](rfcs/0011-performance-ui-and-editors.md#step-6-status)).
- **A group:** parameters of one module in the same group are drawn joined, edge to edge,
  and kept together on narrow screens.
- **A role** (`enum oscairn_role`): cutoff, resonance, filter envelope amount, key tracking
  and the filter envelope's stages; attack, decay, sustain, release, hold, velocity, amp key
  tracking and level; the EQ's gains, frequencies, mid Q and output; levels 1 to 8 (elements,
  operators, oscillators); `mix`, an effect's main control; pan; and `OSCAIRN_ROLE_OUTPUT_BUS`,
  the parameter choosing the output port a sound plays on, which lets parts share the instance
  ([above](#parts-sharing-one-instance)) and fills no bank. Roles put a parameter on
  the same knob in every instrument's role banks (Filter, Amp EG, EQ, Levels); a role the
  plugin lacks is a blank knob. Only instruments fill role banks; an effect's `mix` goes to
  the Effects bank. The banks serve every set of physical knobs: a controller profile's knob
  rows and a Mackie Control surface's V-pots
  ([RFC 0024](rfcs/0024-device-profiles.md#mackie-control)).

A plugin that declares no role gets roles from its parameters' names: exact names only, such
as "Cutoff", "Resonance", "Attack" (a filter module's "Attack" is the filter envelope's),
"Level", "Wet" or "Mix". That misses most spellings, so declare roles. Declaring any role
turns the guessing off for the plugin's other parameters, so declare every role it has.
Every parameter stays reachable whatever its hint. A module grid and a waveform with markers
come later. Draft 3 added the role, draft 4 a view and draft 5 `seconds` (below), so the
structs grew each time: a plugin built against draft 2 must rebuild; one built against draft 3
still gets its hints and roles, without views, and one built against draft 4 its views, its
envelopes' times read from value texts with units, until it rebuilds against draft 5.

### Rich views: what each engine should declare

Draft 4 lets a plugin say which of its parameters make an envelope, a key or velocity map, or
an FM operator view; Desktop and Panel then draw that view above the parameters' tiles, from
their values, and dragging it sets them, undone as any parameter edit
([RFC 0019](rfcs/0019-adaptive-ui.md#step-5-status)). Three more fields at the end of the hint:

- **`view`:** any nonzero number, the same for every parameter of one view, and stable across
  info rescans. A view may span modules (an FM view's operators are on their own pages).
- **`field`** (`enum oscairn_field`): the parameter's place in it.
- **`index`:** whose field it is, 1 to 8 (an operator's, an element's, a zone's), or 0 for the
  view's own (the algorithm).

What a view is follows from its fields:

| View | Fields | Drawn |
| --- | --- | --- |
| Envelope, named stages | `DELAY`, `ATTACK`, `HOLD`, `DECAY`, `SUSTAIN`, `RELEASE` (attack and release needed) | From zero: delay, attack to full, hold, decay to the sustain level, held, release to zero |
| Envelope, points | `LEVEL_0`…`LEVEL_8`, `TIME_1`…`TIME_8` or `RATE_1`…`RATE_8` (a rate: faster when higher) | Segment k reaches `LEVEL_k`; without one it holds, and the last falls to zero. From `LEVEL_0`, else the last segment's level. Holds before the last segment, which is the release |
| Key or velocity map | `KEY_LOW`, `KEY_HIGH`, `VELOCITY_LOW`, `VELOCITY_HIGH` per index (a zone); `BREAKPOINT`, `LEFT_DEPTH`, `RIGHT_DEPTH`, `LEFT_CURVE`, `RIGHT_CURVE`, `VELOCITY_DEPTH`, `VELOCITY_CURVE` | Every index's zone over a keyboard, the page's own in the accent; level scaling around the breakpoint, a curve's value text saying its sign and shape (`-Linear`, `+EXP`); velocity's effect |
| FM operator view | `ALGORITHM` (stepped, up to 128 values; index 0) and `FEEDBACK`; per operator `OPERATOR_LEVEL`, `RATIO` (or its coarse part), `FINE`, `DETUNE`, `FIXED`, and its envelope and scaling fields with its index | Where the algorithm is: the routing, and a column per operator (level, ratio, envelope); an operator's page draws its envelope and scaling |

An envelope is drawn per view and index, a map per view and index (with all the view's zones),
each where its parameters' tiles are. Rules:

- `param_hint` must return true for **every** parameter in a view, also those with no role or
  widget (`OSCAIRN_WIDGET_AUTO`, priority 0, `CLAP_INVALID_ID`, group 0, their role or none).
  A plugin that returns false for parameters without a role, as the forks do today, must change
  that.
- Fill `algorithm` in `oscairn_plugin_ui_t` (or leave it null): the routing of each algorithm
  value, `inputs[n]` the mask of operators modulating operator n + 1, `carriers` the mask
  heard, `feedback_from` and `feedback_to` (0: none). The host asks for every value of the
  parameter when it reads the hints, and drops all routings if one is missing.
- The host keeps a field only where it knows it, on an index up to 8, and an algorithm only on
  a stepped parameter of at most 128 values; anything else leaves its view and is a tile.
- An envelope's stages are drawn by time: each segment has the same room, filled on a log
  scale (about even below 50 ms) up to the longest any of its stages can take, past a least
  width that keeps an instant stage's point apart to grab. Dragging a point maps back through
  the same scale. The times come from `seconds` (draft 5): fill it in `oscairn_plugin_ui_t`
  (or leave it null) to give how long a `DELAY`, `ATTACK`, `HOLD`, `DECAY`, `RELEASE`, `TIME_n`
  or `RATE_n` parameter's stage takes at a value, and for a rate the time a segment across the
  envelope's whole range takes; approximate is enough. The host asks at each value of a stepped
  parameter of at most 128 values, else at 33 evenly spaced values, when it reads the hints,
  and drops them all if one is missing. Without `seconds` it reads the value texts at those
  values, when every one is a number with `ms`, `s` or `sec` ("31.2 ms", "1.20 s"). When any of
  an envelope's stages has neither, the envelope is drawn by its parameters' ranges, a rate
  reversed.
- Levels and other positions are by each parameter's range (0 at the left or bottom); value
  texts label them. A pitch envelope's levels are drawn from the bottom, not about a center,
  for now.
- Views never replace tiles or physical controls; the knobs bind to the parameters.

**Oscairn FM** (IDs from `source/oscairn/Parameters.cpp`): the Classic voice as view 1, the
Extended voice as view 2, the pitch EG as view 3. `OperatorGraph` already has
`oscairn_fm_algorithm_t`'s layout.

```cpp
// Draft 4 (vendor the new ui.h): a view field for every parameter in a view.
uint32_t fmField(clap_id id, uint32_t &view, uint32_t &index)
{
    const uint32_t n = id / 1000, k = id % 1000;
    index = 0;
    if (id == 1 || id == 2) { view = 1; return id == 1 ? OSCAIRN_FIELD_ALGORITHM : OSCAIRN_FIELD_FEEDBACK; }
    if (id == 40 || id == 41) { view = 2; return id == 40 ? OSCAIRN_FIELD_ALGORITHM : OSCAIRN_FIELD_FEEDBACK; }
    if (id >= 20 && id <= 27) { view = 3; return id < 24 ? OSCAIRN_FIELD_RATE_1 + (id - 20) : OSCAIRN_FIELD_LEVEL_0 + 1 + (id - 24); }
    if (n < 1 || n > 8) return OSCAIRN_FIELD_NONE;
    index = n;
    if (k < 30 && n <= 6) { // Classic operator n: Rate 1-4, Level 1-4 (it starts at level 4)
        view = 1;
        if (k >= 1 && k <= 4) return OSCAIRN_FIELD_RATE_1 + (k - 1);
        if (k >= 5 && k <= 8) return OSCAIRN_FIELD_LEVEL_0 + (k - 4);
        switch (k) {
        case 9: return OSCAIRN_FIELD_BREAKPOINT;   case 10: return OSCAIRN_FIELD_LEFT_DEPTH;
        case 11: return OSCAIRN_FIELD_RIGHT_DEPTH; case 12: return OSCAIRN_FIELD_LEFT_CURVE;
        case 13: return OSCAIRN_FIELD_RIGHT_CURVE; case 16: return OSCAIRN_FIELD_VELOCITY_DEPTH;
        case 17: return OSCAIRN_FIELD_OPERATOR_LEVEL; case 18: return OSCAIRN_FIELD_FIXED;
        case 19: return OSCAIRN_FIELD_RATIO;      case 20: return OSCAIRN_FIELD_FINE;
        case 21: return OSCAIRN_FIELD_DETUNE;
        }
        return OSCAIRN_FIELD_NONE;
    }
    view = 2; // Extended operator n: Hold, then Attack, Decay 1, Decay 2 and Release, back to
              // the Release (Hold) Level, which it starts at (no LEVEL_0)
    if (k == 49) return OSCAIRN_FIELD_TIME_1;
    if (k >= 45 && k <= 48) return OSCAIRN_FIELD_TIME_1 + 1 + (k - 45);
    if (k >= 41 && k <= 44) return OSCAIRN_FIELD_LEVEL_0 + 2 + (k - 41);
    switch (k) {
    case 31: return OSCAIRN_FIELD_FIXED;      case 32: return OSCAIRN_FIELD_RATIO;
    case 33: return OSCAIRN_FIELD_FINE;       case 34: return OSCAIRN_FIELD_DETUNE;
    case 51: return OSCAIRN_FIELD_OPERATOR_LEVEL; case 52: return OSCAIRN_FIELD_BREAKPOINT;
    case 53: return OSCAIRN_FIELD_LEFT_DEPTH; case 54: return OSCAIRN_FIELD_RIGHT_DEPTH;
    case 55: return OSCAIRN_FIELD_LEFT_CURVE; case 56: return OSCAIRN_FIELD_RIGHT_CURVE;
    case 57: return OSCAIRN_FIELD_VELOCITY_DEPTH;
    }
    return OSCAIRN_FIELD_NONE;
}

bool paramHint(const clap_plugin_t *, clap_id id, oscairn_param_hint_t *hint)
{
    uint32_t view = 0, index = 0;
    const uint32_t field = fmField(id, view, index), role = roleOf(id); // roleOf: the roles paramHint declares today
    if ((role == OSCAIRN_ROLE_NONE && field == OSCAIRN_FIELD_NONE) || indexOf(id) < 0)
        return false;
    *hint = { id == outputBusParam ? (uint32_t)OSCAIRN_WIDGET_LIST : (uint32_t)OSCAIRN_WIDGET_AUTO, 0, CLAP_INVALID_ID, 0, role,
              field ? view : 0, field, index };
    return true;
}

bool algorithm(const clap_plugin_t *, clap_id id, uint32_t value, oscairn_fm_algorithm_t *out)
{
    const OperatorGraph *g = id == 1 && value >= 1 && value <= 32 ? &dx7Algorithms[value - 1]
                           : id == 40 && value >= 1 && value <= 88 ? &extAlgorithms[value - 1] : nullptr;
    if (!g) return false;
    std::memcpy(out->inputs, g->inputs, 8);
    out->carriers = g->carriers; out->feedback_from = g->feedbackFrom; out->feedback_to = g->feedbackTo;
    return true;
}

const oscairn_plugin_ui_t uiExt = { paramHint, algorithm }; // returned for OSCAIRN_EXT_UI ("oscairn.ui/draft-4")
```

Its rates' value texts are bare numbers, so draft 5's `seconds` is what draws the Classic
envelopes by time. MSFA's rate to time, as a full-range decay (its rises are quicker, and
keyboard rate scaling is left out; close enough to draw by). The Extended voice's times and
the pitch EG return false, and are drawn by their ranges, until their own tables are added:

```cpp
// Draft 5 (vendor the new ui.h): a Classic operator rate's time, as MSFA's Env runs it at 44.1 kHz.
bool seconds(const clap_plugin_t *, clap_id id, double value, double *out)
{
    uint32_t view = 0, index = 0;
    const uint32_t field = fmField(id, view, index);
    if (view != 1 || field < OSCAIRN_FIELD_RATE_1 || field > OSCAIRN_FIELD_RATE_8)
        return false;
    const int qrate = std::min(63, (int)std::lround(value) * 41 / 64);                  // Env::advance
    const double inc = (4 + (qrate & 3)) * std::ldexp(1.0, 2 + LG_N + (qrate >> 2)); // per 64-sample block
    *out = 3824.0 * 65536 * 64 / (inc * 44100);                                       // the whole level range
    return true;
}

const oscairn_plugin_ui_t uiExt = { paramHint, algorithm, seconds }; // OSCAIRN_EXT_UI is now "oscairn.ui/draft-5"
```

The Extended operators' pitch EGs (37 to 40) can be views of their own the same way
(`LEVEL_0`, `TIME_1` with `LEVEL_1`, `TIME_2`); the profile's hidden parameters drop out of the
views as they drop out of the pages.

**Oscairn Analog** (Surge, `src/oscairn-clap/OscairnAnalog.cpp`): its two ADSRs, scene A's as
its roles are, as views 1 (Amp Envelope) and 2 (Filter Envelope). The stages' shapes and the
envelope's mode stay tiles.

```cpp
// The view and field of an ADSR stage, beside roleOf; 0: in no view.
uint32_t viewOf(const Parameter &p, uint32_t &field) const
{
    auto &sc = synth->storage.getPatch().scene[0];
    for (uint32_t e = 0; e < 2; ++e) {
        const std::pair<const Parameter *, uint32_t> stages[] = {
            {&sc.adsr[e].a, OSCAIRN_FIELD_ATTACK}, {&sc.adsr[e].d, OSCAIRN_FIELD_DECAY},
            {&sc.adsr[e].s, OSCAIRN_FIELD_SUSTAIN}, {&sc.adsr[e].r, OSCAIRN_FIELD_RELEASE}};
        for (auto &[q, f] : stages)
            if (q == &p) { field = f; return e + 1; }
    }
    return 0;
}

bool uiParamHint(const clap_plugin_t *p, clap_id id, oscairn_param_hint_t *hint)
{
    auto pi = self(p).find(id, nullptr);
    uint32_t field = OSCAIRN_FIELD_NONE;
    const uint32_t view = pi && pi->p ? self(p).viewOf(*pi->p, field) : 0;
    if (!pi || (pi->role == OSCAIRN_ROLE_NONE && !view))
        return false;
    *hint = {pi->bus ? (uint32_t)OSCAIRN_WIDGET_LIST : (uint32_t)OSCAIRN_WIDGET_AUTO, 0, CLAP_INVALID_ID, 0, pi->role, view, field, 0};
    return true;
}
const oscairn_plugin_ui_t uiExt{uiParamHint, nullptr};
```

Its stages' value texts carry units ("250.0 ms"), so its envelopes are drawn by time without
more. Draft 5's `seconds` gives the times exactly; the stages are log2 seconds (`ct_envtime`):

```cpp
// Draft 5 (vendor the new ui.h): an ADSR stage's time; the sustain is a level.
bool uiSeconds(const clap_plugin_t *p, clap_id id, double value, double *seconds)
{
    auto pi = self(p).find(id, nullptr);
    uint32_t field = OSCAIRN_FIELD_NONE;
    if (!pi || !pi->p || !self(p).viewOf(*pi->p, field) || field == OSCAIRN_FIELD_SUSTAIN)
        return false;
    *seconds = std::exp2(value);
    return true;
}
const oscairn_plugin_ui_t uiExt{uiParamHint, nullptr, uiSeconds}; // OSCAIRN_EXT_UI is now "oscairn.ui/draft-5"
```

**An AWM2 element** (the sfizz-based Oscairn Sampler, once its elements are parameters; it has
only Output bus and the Assign Switches today): every element n (1 to 8) in one view, index n,
so each element's page draws its envelope and one map of all the elements' zones, its own in the
accent. With element n's parameters on page `Element n`:

| Parameter | Field | Index |
| --- | --- | --- |
| Note Limit Low, Note Limit High (MIDI notes, note-named value texts) | `KEY_LOW`, `KEY_HIGH` | n |
| Velocity Limit Low, Velocity Limit High | `VELOCITY_LOW`, `VELOCITY_HIGH` | n |
| Level Velocity Sensitivity | `VELOCITY_DEPTH` | n |
| AEG Initial Level | `LEVEL_0` | n |
| AEG Attack Time, Attack Level | `TIME_1`, `LEVEL_1` | n |
| AEG Decay 1 Time, Decay 1 Level | `TIME_2`, `LEVEL_2` | n |
| AEG Decay 2 Time, Decay 2 Level (held while the key is down) | `TIME_3`, `LEVEL_3` | n |
| AEG Release Time (to silence) | `TIME_4` | n |

The element amp EG has no hold stage; only the pitch and filter EGs do.

```cpp
// Element n's parameters, in this order from element n's first ID; one view (1) for all.
static const uint32_t elementFields[] = {
    OSCAIRN_FIELD_KEY_LOW, OSCAIRN_FIELD_KEY_HIGH, OSCAIRN_FIELD_VELOCITY_LOW, OSCAIRN_FIELD_VELOCITY_HIGH,
    OSCAIRN_FIELD_VELOCITY_DEPTH, OSCAIRN_FIELD_LEVEL_0,
    OSCAIRN_FIELD_TIME_1, OSCAIRN_FIELD_LEVEL_0 + 1, OSCAIRN_FIELD_TIME_1 + 1, OSCAIRN_FIELD_LEVEL_0 + 2,
    OSCAIRN_FIELD_TIME_1 + 2, OSCAIRN_FIELD_LEVEL_0 + 3, OSCAIRN_FIELD_TIME_1 + 3 };
// in paramHint, for element n's k-th parameter:
*hint = {OSCAIRN_WIDGET_AUTO, 0, CLAP_INVALID_ID, 0, OSCAIRN_ROLE_NONE, 1, elementFields[k], n};
```

Its times draw by time with no code when they are seconds whose value texts say so
("0.25 s"). If they are AWM2's 0 to 127 instead, `seconds` gives each value's time from the
engine's own table (AWM2's is Yamaha's and not published; the test plugin's stand-in is
1 ms × 2^(value / 9)):

```cpp
// Draft 5: an element's AEG time; awm2Seconds is the engine's table of 128 times.
bool seconds(const clap_plugin_t *, clap_id id, double value, double *out)
{
    const uint32_t k = elementParameter(id); // its place in elementFields, as paramHint finds it
    if (k >= std::size(elementFields) || elementFields[k] < OSCAIRN_FIELD_TIME_1 || elementFields[k] > OSCAIRN_FIELD_TIME_8)
        return false;
    *out = awm2Seconds[std::clamp((int)std::lround(value), 0, 127)];
    return true;
}
```

The element's level takes `OSCAIRN_ROLE_LEVEL_1 + n - 1` as today. Its filter EG, with a
release level, is a second view per element (`LEVEL_0` to `LEVEL_5`), drawn from the bottom
until a centered field exists. The test plugin `hosts/standalone/tests/views_plugin.cpp`
declares all three kinds and is what `desktop-gaps` edits.

### Declared memory

`oscairn.memory` has two calls, and the host refuses a plugin that offers one without the
other:

- `estimate(rate, max_frames)`, called after the state loads and before every activation (a
  load, a state load, a new effect context, a new sample rate or block size, and a restart the
  plugin asks for), says what activating would allocate: shared
  bytes (once per instance or engine), bytes per context (an instance without contexts is
  one) and decoded asset bytes for the current state. A new effect context costs its context
  and asset bytes. With a memory budget, the host refuses to activate a plugin declaring more
  than the budget leaves, saying how much it needs. With native sound contexts, the estimate
  covers every sound the state holds, and each sound prepared after activating (`prepare`,
  `prepare_preset`, or a snapshot's sounds prepared beside) costs its context bytes, so report
  one sound's memory as `context_bytes` (the Oscairn Analog counts the state's other sounds in
  `shared_bytes`). The host refuses a prepare whose context bytes are more than the memory the
  budget leaves, before calling the plugin ("Plugin needs N MiB of memory where M MiB is
  left"), and a snapshot whose prepared sounds' bytes are, discarding them again.
- `current`, after activating, says what the plugin holds. The host requires it but does not
  use it for budgets yet; it counts its own measurement.

Oscairn counts the larger of the declaration and its own measurement, and keeps its
measurements per declared size, so a plugin's modes (a reverb's algorithms) are not counted at
the largest. In a worker process, a declaration also sets the worker's memory limit
([In a worker process](#in-a-worker-process)). A plugin that holds more than it declared is
counted at what it holds; distrusting its later declarations is planned.

## The plugin's window

On the desktop, a plugin with CLAP's GUI extension gets **Open plugin window** on its
selected box in the Part page and in Routing. The window is shown by the process hosting the
plugin, its worker, not by the app ([RFC 0019](rfcs/0019-adaptive-ui.md#plugin-windows)), so:

- **Embedded is what the host asks for first** (`is_api_supported("cocoa", false)` on macOS).
  The host makes a plain window titled with the node and its part ("Odin2 · Lead"), sized from
  `get_size`, and gives the plugin its content view with `set_parent`. It is resizable when
  `can_resize` says so; a resize goes through `adjust_size` and `set_size`, and
  `request_resize` resizes the window. JUCE-based plugins work this way unchanged.
- **Floating** is used when the plugin offers only that: the host calls `create(api, true)`,
  `suggest_title` and `show`, and honors `request_show`, `request_hide` and `closed`. There is
  no `set_transient`: the app's window is in another process.
- **`set_scale` is not called:** on macOS the view works in points and the system scales it.
- All calls are on the plugin's main thread (the worker's main thread). Requests made from
  another thread are carried out at the host's next main-thread service, within a few
  milliseconds.
- The window closes when the node goes away (removed, replaced, another Performance loaded) or
  the person closes it, which destroys the editor (`destroy`); it is created again the next
  time. A worker restarted after a crash opens it again.
- **Do not run a modal loop** in the editor (a blocking dialog): the worker answers the engine
  between window events, and a loop that does not return holds its requests.
- Devices never show plugin windows; they draw editors from the parameters (below).

## Editing without the plugin's window

Every plugin gets editors generated from its parameters, so anything a sound depends on must
be a parameter (or a structured editor, below) to be editable on a device. On the desktop an
editor is pages of tiles: a page per top-level CLAP module path, a heading per module below
that, and a tile per parameter. Every parameter that is not hidden is reachable there. On the
device, editors are always Oscairn's, from the parameters or from a description the plugin
gives ([RFC 0011](rfcs/0011-performance-ui-and-editors.md#plugin-editors)).

Parameter metadata to give: clear names, CLAP module paths for grouping, value text, and the
stepped, hidden and read-only flags. How the editors use them:

- the first level of the module path names the page (`Operator 1`, `LFO 2`); numbered pages
  of one kind are kept together in number order. A page of more than 24 parameters gets a
  tab for each second level (`Operator 1/Envelope`); the rest of the path is a heading. So
  give a large page second levels of a dozen or so parameters each;
- without module paths, a plugin of more than 24 parameters gets pages from its names'
  leading words (`Osc2 Detune` is `Detune` on page `Osc2`; `Arp Step 6 Transpose` is
  `Transpose` on page `Arp`, tab `Step 6`), pages in name order; give module paths to choose
  the pages and their order yourself. The host lists parameters in ID order, so number IDs
  in reading order where you can;
- list parameters in the order a player reads them (attack before decay), since tiles follow
  the plugin's order within a page;
- a stepped parameter with 2 to 4 values is a row of buttons (Off and On, or 0 and 1: a
  switch), with up to 64 a list of their value texts, otherwise a slider; a buttons hint asks
  for buttons up to 8 values. A symmetric range with a default of 0 fills from the center;
- a mode or algorithm (a reverb's Hall, Room and Plate) is best a stepped parameter with its
  names as value texts, first by priority. When a mode changes which controls matter, hide
  the others (`CLAP_PARAM_IS_HIDDEN`) or change names or modules, and call
  `clap_host_params.rescan(CLAP_PARAM_RESCAN_INFO)` from the main thread: the host takes the
  new info in place, without deactivating the plugin, so the sound does not stop, and the
  editors redraw their pages within half a second. Adding or removing parameters still needs
  `CLAP_PARAM_RESCAN_ALL` and a restart. A plugin in a worker process gets the same: the
  worker forwards the new info with its next service;
- value texts are shown as written, except a bare number with more than three decimals,
  which is shortened (`0.0100000` shows as `0.01`), so write units and names where they help;
- a search box finds parameters on any page by name or module, so keep names specific;
- a small screen with a few encoders (RFC 0019's Compact surface, tried with the Compact
  layout) lists the same pages, then a page's parameters four at a time in priority order,
  so a page's first four by priority (or in your order, without hints) are what a pedal-sized
  device reaches first;
- a device's screen knobs follow the editor page shown, taking its first parameters by
  priority;
- an envelope's parameters (described, or recognized by name below), and an LFO's or
  filter's recognized by name, are linked into one group of tiles, as a hint's group would;
- an envelope, a key or velocity map and an FM operator view are drawn from a plugin's
  `oscairn.ui` views ([above](#rich-views-what-each-engine-should-declare)), and dragged to
  edit them. For parameters in no view, an envelope is still drawn as a curve you can drag
  when one module holds its parameters under one of these sets of names: `Attack`, `Decay`,
  `Sustain`, `Release` (optionally `Delay` and `Hold`); `Rate 1`–`4` with `Level 1`–`4`
  (DX7); or `Attack Time`, `Attack Level`, `Decay 1 Time` and so on with `Release Time`
  (FM-X);
- an LFO's waveform is previewed when one module has a `Wave` (or `Type`, `Shape`) with a
  `Speed` (or `Rate`, `Frequency`), and optionally `Delay`, `Fade In`, `Phase` and
  `User Step 1`… The wave's value text picks the drawing (sine, triangle, square, saw up or
  down, sample and hold or noise, user steps); others show as drawn by the plugin;
- a filter's response is drawn when one module has `Cutoff` and `Resonance`, and its
  `Type`'s value text names it (`LP 24 dB`, `LPF24D`, `HPF12`, `BEF6`; `Off` or `Thru`).

## Behaving well

- **Real-time safety:** no allocation, locks, file access or waiting in `process`. In a
  worker process, the time limit enforces this.
- **CLAP notes,** not only MIDI: note IDs keep repeated notes on one key apart, per-note
  expression works, and `NOTE_END` tells the host when a voice and its tail have finished.
  A plugin need not send `NOTE_END` for the notes it ends in `reset()`: the host treats a reset
  as ending every note it sent (CLAP's `reset()` has no output events). End every voice at once
  in `reset()`.
- **Chokes and per-note volume:** treat a host `NOTE_CHOKE` as a fast fade of 2–5 ms, not a
  cut, and honor `CLAP_NOTE_EXPRESSION_VOLUME`. When a part's note limit, or the shared
  budget across parts, steals a note, the host lowers its volume expression to silence over
  5 ms in eight steps, then chokes it ([RFC 0018](rfcs/0018-voice-budget-and-stealing.md)); a
  plugin that does neither clicks. The shared budget costs each note by the plugin's measured
  time per block (`process`'s wall time above its time at rest, shared among the notes it holds
  until their `NOTE_END`), so a plugin that sends `NOTE_END` late, or never, is charged for
  voices it no longer plays; and it steals only CLAP notes, never a MIDI-only plugin's. An instrument built on the
  SDK's `sound-contexts-plugin.hpp` gets both: it multiplies each voice's samples by
  `amplitude(voice, offset)`, and a choked voice fades over 3 ms before it ends.
- **Pitch bend and the mod wheel:** apply MIDI pitch bend per channel, to held and released
  voices, without zipper noise. An instrument built on the SDK keeps both per channel; calling
  `followControls(voice)` each sample moves the voice's `bend` (-1 to 1 of the instrument's
  range) and `modWheel` (0 to 1) smoothly toward its channel's. The instrument sets the range
  and what the wheel does; Basic Synth bends ±2 semitones by default and uses the wheel for
  vibrato. Per-note bend (`CLAP_NOTE_EXPRESSION_TUNING`, MPE) is not applied by the SDK.
- **Say when you are silent:** return `CLAP_PROCESS_SLEEP` from `process` when the plugin will
  make no sound until it gets input or events (an instrument with no voice playing, an effect
  whose tail has ended). A plugin switched off by a scene, or left behind by a Performance
  change ([RFC 0021](rfcs/0021-sounds-performances-and-library.md#seamless-switching)), plays
  out its held notes and tail and stops once it is idle: one that sleeps stops as soon as it
  is done; one that never does waits for three seconds of silence. Until then it runs beside
  the new Performance, so memory and CPU for both are needed for a moment; when both would not
  fit the device's budget, the change is a cut instead. A setlist prepares its next
  Performance while one plays: that one's plugins are created, activated and given their saved
  state, and get their main-thread callbacks, perhaps minutes before they first process.
- **Expect a second life:** an instance that has played may be given another Performance's
  state instead of the host loading a new instance
  ([RFC 0021](rfcs/0021-sounds-performances-and-library.md#free-instances-across-a-change-and-instances-kept-warm)):
  once it is silent, the host resets it (`reset`), deactivates it, loads the state and activates
  it, under a new node. More often it is kept loaded, out of the graph, and later reset and played again with
  its own state; it is given another's at a cut, or when keeping it would not fit. It must then sound exactly as a fresh instance given that state, sample for sample (the
  host's render comparison checks the bundled plugins for it): clear voices, delay lines, filter
  memory, envelopes, LFO phases and random seeds on `activate` (and on `reset`), and keep nothing
  from the last state that the new one does not set. `reset` must take effect at once: a note
  in the next block after it must sound, so a voice fade it defers to later blocks would cut the
  notes played at the change. Before giving it the new state the host
  saves its own, and loads that back if the change fails, so saving state must work on an
  instance that has played, and loading it back must restore its sound.
- **A new sample rate while loaded:** when the audio device's rate changes, the host
  deactivates the plugin and activates it again at the new rate, on the main thread and never
  while it processes, in the engine or in its worker, without saving or loading its state
  (parameters and sounds stay as they were). Size rate-dependent buffers and coefficients in
  `activate`, not in `init`, and clear voices and tails there; a plugin that keeps its first
  rate plays out of tune. The maximum block size stays the same.
- **Keep your cost steady:** the host measures each plugin's CPU per block and its memory on
  the device, by plugin ID, version and build, and refuses a Performance, or
  an added plugin, that would go over the device's budget, saying why
  ([RFC 0025](rfcs/0025-resource-budgets.md)). CPU is a slowly falling high-water mark of its
  block times, so a heavy passage counts for minutes after it, split into a cost at rest and a
  cost per sounding note; in a part with a note limit, the estimate is at rest plus that many
  notes, so keep your per-voice cost proportional to the voices actually sounding. In a worker,
  memory is what the worker holds; in the engine's process, it is what the process grew by
  while the plugin loaded. A plugin never measured on the device counts as 2% of the block's
  time and 32 MiB until it is. A new version starts unmeasured, and so does a new build of the
  same version: the build is the SHA-256 digest of the plugin's files (its bundle on macOS), as
  the Performance lock records it, taken again when any of them is written, so a rebuilt 0.1.0
  is measured again without a version bump. A Performance not open is estimated from the build
  at its saved path; where none is there, from the most any build of that version measured.

## In a worker process

The desktop app runs each CLAP plugin instance in a worker process of its own
([RFC 0010](rfcs/0010-plugin-worker-isolation.md)), so a crash, hang or overrun silences only
that plugin. Effect contexts made from an instance share its worker. LV2 plugins stay in the
engine. The worker is chosen by the app's engine helper (`oscairn-engine`);
`OSCAIRN_PLUGIN_WORKER` names another worker program, and set but empty runs plugins in the
engine. Other hosts (`oscairn-render`, the desktop's `--in-process` mode, a program using
`Session` directly) run plugins in the engine unless `OSCAIRN_PLUGIN_WORKER` names the worker.
Test your plugin both ways.

What changes for the plugin:

- **Its main thread is the worker's.** Calls arrive over a socket; audio and events through
  shared memory.
- **Time limit:** in real time, a block may take half the block's time, counted from when the
  engine hands it over; a later one is silenced. Plugins that do not feed each other render at
  the same time, each in its worker ([RFC 0034](rfcs/0034-parallel-graph-rendering.md)); effect
  contexts of one instance never do. Missing a quarter second of blocks in a row (at least 8), or leaving one
  unanswered for 250 ms, is a failure. Offline rendering waits instead.
- **Memory limit:** 4 GiB by default (`OSCAIRN_WORKER_MEMORY_MB` changes it; 0 is none), read
  every 10 ms as the physical footprint on macOS and resident memory on Linux. When every
  plugin in the worker declares its memory (`oscairn.memory`), the limit is what they declare
  plus half again and 64 MiB. Before each activation the worker sends the engine the new
  declaration and waits while the engine raises the limit, so a mode that needs more (a restart
  the plugin asked for included) is allowed it before it allocates. A smaller mode lowers it
  after the operation, no further than what the worker then holds plus 64 MiB. Each sound
  context prepared after activating adds its declared context bytes to the declaration, and the
  limit is raised for it before the plugin prepares it (for a snapshot prepared beside, for
  every free context until the plugin has said how many it made). A declaration over four
  times the flat limit (1 TiB with none), its sounds included, is refused before the plugin activates or prepares. Past the
  limit the worker is stopped like a crash. On Linux the worker also sets `RLIMIT_AS` to four times the flat limit, and with `OSCAIRN_CGROUP` the
  limit is the worker's cgroup `memory.max`.
- **Restart:** after a crash, hang, too many misses or too much memory, the worker is
  restarted. Each plugin gets back its state as last loaded, the sounds prepared since and
  their selections, and its current parameter values, and its output fades in. Held notes and
  tails are lost. After three restarts within a minute it stays silent.
- **Parameter info:** an info rescan is forwarded with the worker's next service. A rescan
  that changes the parameters' IDs or order is ignored until the plugin is loaded again.
- **Reporting:** `session.status` lists each plugin in a worker with its CPU time and memory.
- **Your own threads:** a plugin may start threads of its own in a worker, as in the engine (an
  EQ matching its coefficients, a sampler streaming from disk). Only the worker's audio thread
  has real-time priority; yours run at normal priority. Their memory counts toward the worker's
  memory limit. Thread-check reports them as neither the main nor the audio thread, so call the
  host from them only through CLAP's thread-safe functions (such as `request_callback`), and
  stop them by `destroy`. A restart ends the process and them with it. `process` must not wait
  for them: a block that does is late like any other.
- **How their CPU counts:** the budget ([Behaving well](#behaving-well), RFC 0025) measures
  each block's `process` call on the worker's audio thread, in wall time, as in the engine; the
  time limit is how long the engine waits for the block. Neither counts a helper thread's CPU
  time, unless that thread slows the audio thread by taking its core. The worker's whole CPU
  time, every thread's user and system time, is what `session.status` reports (shared among the
  plugins in that worker), and the budget does not use it. So work moved off the audio thread
  is not charged against the budget; on a device with few cores it still competes with audio.

## Packages and signing

- **Moving to another device:** an exported Performance or Sound
  ([archive](library-archive.md), format 3) carries every build of each CLAP plugin it uses
  that the exporting machine has: its own, and other platforms' added to its plugin store.
  Installing it puts them in the store (`Plugins/clap/<id>@<version>/<platform>/` in the data
  folder). A Performance records each plugin build's digest in its `lock`
  ([format](performance-format.md)). A Performance made on a Mac opens on Linux with a stand-in
  unless a Linux build of the same version went with it or is installed. Keep the ID and
  version stable across platforms, and keep everything the plugin needs inside its bundle;
  files it installs elsewhere do not go with it. Libraries too: a build may load only the
  system's libraries and its bundle's own (macOS: `/usr/lib/`, `/System/`, `@loader_path` and
  `@rpath` into the bundle; Linux: glibc, libstdc++, libgcc_s, ALSA and the image's X11 and font
  libraries, or `$ORIGIN`), so link Homebrew's or `/usr/local`'s statically or bundle them.
  `oscairn-sign plugin-package` and `oscairn-plugin-check` check it
  ([the rules](library-archive.md#plugin-packages)). When a device's store already has a different
  build of the same version for a platform, it keeps that one and notes that the incoming build
  was not installed, so give a changed build a new version. LV2 plugins
  are not packaged; they must be installed on each device.
- **Signing your builds** ([RFC 0006](rfcs/0006-packages-and-portability.md#signatures)):
  `oscairn-sign key author.key` makes your key (keep it private; it is readable only by you),
  and `oscairn-sign build author.key clap <id> <version> <platform> MyPlugin.clap` writes
  `MyPlugin.clap.sig` beside the build, over its ID, version, platform and digest. Platforms
  are `linux-arm64`, `linux-x86_64`, `macos-arm64`, `macos-x86_64` and `windows-x86_64`. Ship
  the `.sig` beside the bundle: exports carry it. Re-sign after every rebuild; a signature over
  another build is left out. Publish your key's ID (shown by `oscairn-sign key`) so users can
  check it. `oscairn-sign verify archive.olib` lists who signed what in an archive.
  To ship the plugin alone, `oscairn-sign plugin-package --key author.key --license <spdx>
  --license-file COPYING --source <url> <commit-or-tag> MyPlugin.olib clap <id> <version>
  <name> <platform>=MyPlugin.clap...` packs your signed builds into a
  [plugin package](library-archive.md#plugin-packages), which installs them into the store,
  with its license texts and notices and where its source is (the store records the license
  expression, which `plugins.list` gives as `license`); a GPL, LGPL or AGPL plugin
  package is refused without its source and a license text
  ([license and source](library-archive.md#license-and-source)).
  A bundle holding several plugins is signed for all of them at once (`oscairn-sign build`
  takes the IDs comma-separated) and packed once, each plugin named with `--plugin <id>
  <name>`; a plugin whose version is not the package's is `<id>@<version>` to both. A preset-discovery provider in it is not a plugin and is neither listed nor signed.
  To publish to Oscairn's package repository, sign the archive too and follow
  [Publishing a package](developer-guide.md#publishing-a-package).
- **Installing code:** a device installs a build signed by a key its trust store trusts for
  plugins without asking. A build that is unsigned or signed by an unknown key is installed
  only after the user agrees (`acceptUnsignedCode`; the desktop's Install anyway). Installing
  from a repository, the desktop also offers to trust the publisher for plugins, showing its
  name, key fingerprint and full key; once trusted, its later builds install without asking.
  Nothing trusts a publisher on its own: the repository listing a package does not. A build
  signed by a revoked key is never installed.
- **A plugin that is not here,** in another version, or that fails to load opens as a
  stand-in: silent, its ports and parameters as saved, its state written back unchanged, so
  installing the plugin later brings the sound back.

## Testing

Copy the reference plugins' tests in `core/engine/tests/` and `plugins/`: deterministic
offline rendering with `oscairn-render`, rendering that never allocates, state round trips,
and the same tests with the plugin in a worker process (the `*-workers` tests). Run them
under AddressSanitizer and ThreadSanitizer. Two checks run on a plugin as built, without
writing tests:

- `oscairn-plugin-check PLUGIN ID synth|effect [--param ID=VALUE]` (built as
  `build/core/control/oscairn-plugin-check`): the path a musician's sound takes. The plugin
  plays in a part, is saved to the library and opened again with every parameter as saved, is
  exported in an archive with its build, and is installed on a fresh device, where it opens
  from the plugin store and plays the same. `--juce-state NAME=VALUE` sets an attribute in a
  JUCE plugin's saved state, as choosing a program in its window would. With
  `--add-build linux-arm64=PATH --keep-archive FILE`, the archive also carries another
  platform's build; `oscairn-plugin-check --install FILE synth|effect` on that platform installs
  it and checks it opens from the store and plays. A note effect (CLAP feature `note-effect`,
  or note ports in and out with no audio output) is played notes and must send notes on, through
  its part's MIDI out, instead of making sound. It prints one line per check.
- `core/lv2-host/tests/production/run.py PROBE OUT --case NAME,PATH,ID,synth|effect,PARAM,VALUE`:
  renders at 44.1 and 48 kHz, 37- and 512-frame blocks, offline and realtime, a fresh-instance
  repeat and a parameter edit that must change the sound
  ([how](../core/lv2-host/tests/production/README.md)). Its probe,
  `oscairn-lv2-production-probe`, is built only with `-DOSCAIRN_LV2=ON`, even for CLAP plugins.

Set `OSCAIRN_PLUGIN_WORKER=build/core/oscairn-plugin-worker` to run either check with the
plugin in a worker. See
[Test before publishing a plugin](developer-guide.md#test-before-publishing-a-plugin).

## Examples

The bundled plugins are working references ([plugins](../plugins/README.md)):

- **Basic Synth** (`plugins/basic-synth/basic_synth.cpp`): an instrument on the SDK's
  `sound-contexts-plugin.hpp`. Native sound contexts with their snapshot as its state, factory
  presets through CLAP preset discovery prepared as contexts (`preset-discovery-plugin.hpp`), CLAP
  and MIDI note dialects, `oscairn.ui` hints with roles (Brightness is its cutoff, Level its
  level, the envelope's stages attack to release) and its envelope as an ADSR view, pitch bend and the mod wheel through `followControls`, choke fades through
  `amplitude`.
- **Delay** (`plugins/delay/delay.cpp`): an effect with native effect contexts (`create` and
  `resources` with its tail), the state extension, and Wet declared as its `mix` role.
- **Step Sequencer** (`plugins/step-sequencer/step_sequencer.cpp`): a note-only plugin with
  parameter-target lanes.

## Known limits

- **The plugin's window** is a window of its own on macOS, beside the app's, never inside it.
  On Linux and Windows the host makes no windows yet, so only a plugin that floats its own
  shows one. A plugin placed in the engine's process rather than a worker is unverified.
- **Instant sound switching** for plugins without native sound contexts uses a whole extra
  instance per prepared sound (up to eight), and only for instruments: note input, audio
  output, no audio input, no note output.
- **Not implemented:** remote controls, voice info, note names,
  audio-ports configurations, render mode, and watching preset folders for changes (they are
  read when listed). (From the host's source; not exercised with a plugin.)
- **In a worker,** a restarted worker's parameter info shows only after its next rescan.
- **Declarations** are not yet distrusted after a plugin holds more than it declared, and
  `oscairn.memory`'s `current` is not used for budgets.

## Licenses

Keep a reused engine's license and notices with it and credit its authors in
[CREDITS](../CREDITS.md). Chillbeast's Modular shows the pattern: imported unchanged with its
history, wrapped as a CLAP plugin beside it ([plugins/modular](../plugins/modular/README.md)).
