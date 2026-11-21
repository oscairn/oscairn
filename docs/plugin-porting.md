# Porting a plugin to Oscairn

For developers bringing an existing CLAP plugin to Oscairn. A plugin that already follows
the CLAP specification usually needs **no changes** to load and play. This page lists what
the host requires, what it does not support yet, and the optional Oscairn extensions that
make a plugin fit the instrument better. LV2 plugins load too when Oscairn is built with
LV2 (see the [developer guide](developer-guide.md)); this page is about CLAP.

This page describes the current host and is updated as features land. Each fact below was
checked against the Rust CLAP host (`core/clap-host/rust`, the default; `core/clap-host/src/Host.cpp` is its
C++ reference, `-DOSCAIRN_CLAP_HOST=cpp`), `core/plugin-host/` and `core/control/`
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
   ([Editor hints and roles](#editor-hints-and-roles)).
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
| If it has `oscairn.memory`, `oscairn.sound-contexts`, `oscairn.sound-snapshots`, `oscairn.voice-budget` or `oscairn.asset-dependencies`, the functions each requires (each extension's [reference page](https://github.com/oscairn/oscairn-clap) says which); at most 256 sound contexts | "Incomplete memory interface", "Invalid sound-context interface", "Invalid sound-snapshot interface", "Invalid voice-budget interface", "Incomplete asset-dependency interface" |
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
  aftertouch and channel pressure (as CLAP notes and the pressure note expression, or as MIDI;
  key pressure only in a Performance whose `input.aftertouch` is `poly`: by default it arrives
  as channel pressure, the highest of the channel's keys'),
  and, as MIDI, pitch bend and the performance controllers: CC 1 mod wheel, CC 2 breath,
  CC 4 foot, CC 11 expression, CC 64–67 (sustain, portamento, sostenuto and soft pedals),
  CC 120 all sound off and CC 123 all notes off. Other controllers and program changes do
  not reach plugins; physical-control mapping routes them. Clock, start, continue, stop and
  song position drive the transport. To hear pitch bend and the mod wheel, a plugin offers
  the MIDI dialect on its note port.
- **Wheels:** pitch bend and the mod wheel reach a plugin as CLAP_EVENT_MIDI only (status 0xE0, 14 bits,
  8192 at rest; CC 1), never as a note expression, from an external keyboard's wheels and from a
  device's own (a FANTOM's lever), even for a plugin that prefers the CLAP dialect and declares MIDI
  too. The host does not scale the bend: full travel is 0 to 16383 and the bend range is the
  plugin's. A part whose Sound assigns the wheel itself (the converter's wheel assignments) gets no
  CC 1 from it unless the assignment says `passes`, and a Performance that sets the wheel to something else (a part's volume) sends none to any part unless its assignment says `passes`; a part can also ignore either (its `receive`). A lever or joystick that follows a wheel by default arrives as that wheel's messages; nothing about it reaches a plugin separately.
- **Pedals:** the sustain pedal (CC 64) and the others reach a plugin as MIDI, unchanged and
  per channel, half-pedal values included; the host holds no note for a CLAP-dialect plugin, so
  an instrument sustains its own voices (a note-off with CC 64 at 64 or more keeps the voice;
  the pedal's release ends those no key holds). A FANTOM's HOLD jack and a computer keyboard's
  pedal arrive the same way, and so does `notes.controller` from the control socket. A part says
  whose pedals it takes (its input's `receive`: every device's, the default, none, or those of
  the devices it plays from), at its input, which is its first plugin's, a note effect such as an
  arpeggiator included: a pedal it does not take reaches neither it nor the instrument after it.
  With several pedals the host is to send the largest value of those the part takes, so a part
  is held while any of them is down (the note router does this once the engine core's port
  does; until then each part gets the pedals of the devices its zones hear, the last one moved
  winning).
- **The transport's stop, for note effects:** a plugin whose notes feed another (an
  arpeggiator, a sequencer) gets, on every press of STOP or `transport.set` with
  `"playing":false`, whether or not the transport was playing, MIDI CC 123 (All Notes Off) on
  each channel of its note port 0, straight from the host after the graph's routing: stop
  generating, let go of latched or held input keys, and send note-offs for what sounds, so
  the notes ring through their release. Keep your settings (an arpeggiator's Hold stays on;
  the next key latches again). Do not choke there: CC 120 (All Sound Off), a `NOTE_CHOKE` with
  key or channel −1 and `reset()` (the player's Panic) are the cuts. The host's other policy
  (`Session::StopNoteEffects::Cut`) sends that all-notes choke at a stop instead. A note effect
  also sees the transport's playing flag fall when it was playing; treat both the same.
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
- **Modulation versus value changes:** a control's **relative** target, however the control is
  driven (a knob on screen, a hardware controller, a scene, another plugin's lane), arrives as
  `CLAP_EVENT_PARAM_MOD` on a parameter that declares `CLAP_PARAM_IS_MODULATABLE`: an amount in
  the parameter's plain units, on top of its own value, which the host never changes for it. The
  amount is the sum of every relative target on that parameter, sent again whenever it changes
  (0 when nothing modulates it any more): add it to the value, keep it until the next one, and
  leave it out of the state you save. From a knob it arrives at sample 0 of the next block (from
  a hardware controller, perhaps again a block or so later); from a lane, at the lane's samples.
  An **absolute** target arrives as `CLAP_EVENT_PARAM_VALUE`, which moves the parameter, as an
  edit on screen does. A parameter without `CLAP_PARAM_IS_MODULATABLE` gets no modulation: its
  relative targets fall back to value changes (the value plus every target's offset, as RFC
  0011's summing rule adds them), which the plugin saves like any value, so declare the flag on
  every parameter that can take modulation.
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

Oscairn's own extensions are optional: a plugin without them works as above, and gets what the
[fallback matrix](https://github.com/oscairn/oscairn-clap/blob/main/docs/fallback-matrix.md) lists.
Their headers and SDK helpers are MIT, so plugins of any license can include them, and live in
[oscairn-clap](https://github.com/oscairn/oscairn-clap), with a reference page for each: its ID and
draft, which side implements what, every function's thread and call order, what the host does
without it, an example, and how to test it. Oscairn keeps a copy at `core/clap-ext` (see the
[developer guide](developer-guide.md#the-clap-extension-headers)). A plugin in its own repository
vendors oscairn-clap at a pinned commit. The drafts change without notice during initial
development; rebuild against the current headers.

| Extension | Why | Reference | Bundled example |
| --- | --- | --- | --- |
| Editor hints (`oscairn.ui/draft-6`) | Puts the right parameters first on small screens and on the physical knobs, and draws envelopes, key maps and FM operator views to drag (an FM view draws its algorithm as a horizontal flow, one graph of every operator's envelope (each line scaled by the operator's `operatorLevel` field when it has one) and a table with a row per operator: a level bar, coarse, fine, ratio or frequency and detune, each column only where the data has the field, and mute and solo as audition switches (mute is a transient `operatorMute` field the host reads; solo is derived by the host muting the others, restored on release). The selected operator's detail names its envelope stages (Hold, Attack, Decay 1, Decay 2, Release) as a Time row over a Level row under a larger graph of that operator with the others dim behind it, and its other parameters as compact controls) | [ui](https://github.com/oscairn/oscairn-clap/blob/main/docs/ui.md) | Basic Synth (cutoff and level, its envelope as an ADSR view), Delay (Wet is its `mix`) |
| Declared memory (`oscairn.memory/draft`) | A device near its memory limit refuses the plugin before its large allocation, and a worker gets a memory limit that fits it | [memory](https://github.com/oscairn/oscairn-clap/blob/main/docs/memory.md) | KICK memory test fixture |
| Native sound contexts (`oscairn.sound-contexts/draft-5`) | Multitimbral playing and instant switching: prepared sounds in one instance (up to the plugin's capacity, at most 256; the SDK's default is 8), each MIDI channel choosing one, held notes keeping theirs, presets prepared while playing | [sound-contexts](https://github.com/oscairn/oscairn-clap/blob/main/docs/sound-contexts.md), SDK [sound-contexts-plugin](https://github.com/oscairn/oscairn-clap/blob/main/docs/sound-contexts-plugin.md) | Basic Synth, Legacy Sampler |
| Snapshots prepared beside (`oscairn.sound-snapshots/draft-4`) | Instant Performance changes without loading a snapshot into an instance; tails ringing out through the outgoing Performance's effects; parts sharing one instance | [sound-snapshots](https://github.com/oscairn/oscairn-clap/blob/main/docs/sound-snapshots.md) | Basic Synth |
| Voice budget (`oscairn.voice-budget/draft-2`, or draft 1 without the importance report) | The instrument gives up its own least important voices (a layer, an element) within a limit from the shared CPU budget, instead of the host fading whole notes, and reports which matter least so the host weighs them against every other part's | [voice-budget](https://github.com/oscairn/oscairn-clap/blob/main/docs/voice-budget.md), SDK [voice-budget-plugin](https://github.com/oscairn/oscairn-clap/blob/main/docs/voice-budget-plugin.md) | Basic Synth; test fixture (`voice_fixture.cpp`, layered) |
| Native effect contexts (`oscairn.effect-contexts/draft`) | Many effect slots sharing one engine's memory, with tails reported; the next Performance's effects as new contexts while the old ones ring out | [effect-contexts](https://github.com/oscairn/oscairn-clap/blob/main/docs/effect-contexts.md) | Delay |
| Parameter-target lanes (`oscairn.param-target`, `oscairn.param-target-lanes/draft`) | For sequencers and modulators: change other plugins' parameters at an exact sample in the same block, directly or through a control | [param-target](https://github.com/oscairn/oscairn-clap/blob/main/docs/param-target.md) | Step Sequencer, LFO |
| Free-running beat (`oscairn.beat`) | For arpeggiators and clocks: a beat position shared by every plugin that runs while the transport is stopped | [beat](https://github.com/oscairn/oscairn-clap/blob/main/docs/beat.md) | Test fixture (`follower_fixture.cpp`) |
| Harmony (`oscairn.harmony`, `oscairn.harmony/draft`) | For harmonizers, chord memories and scripts: the part's key, scale and held chord | [harmony](https://github.com/oscairn/oscairn-clap/blob/main/docs/harmony.md) | Test fixture (`follower_fixture.cpp`) |
| Sound assets (`oscairn.assets/draft-4`, `oscairn.asset-dependencies/draft-2`) | For samplers: the host resolves sample files, and Performances list their samples so they can be checked and moved between machines; draft 4 adds samples in Oscairn Sample Packs and waveforms by id (`sample:<id>`, `waveform:<id>`), a sample resolved to its pack and record offset by `resolve_sample` ([library archive](library-archive.md#samples-and-waveforms)) | [assets](https://github.com/oscairn/oscairn-clap/blob/main/docs/assets.md) | Legacy Sampler; asset libraries' preset providers (Oscairn Analog's factory patches); the sample fixture (`core/engine/tests/sample_fixture.cpp`) for draft 4 |
| Hardware backends (`oscairn.hardware/draft`) | A backend of your own for a device's DSP, FPGA region or sound chip, with your CPU path for every other device, offline rendering and failures. Draft | The header, `core/clap-ext/include/oscairn-clap/hardware.h`; [below](#hardware-backends-draft) | Test fixture (`core/hardware/tests/hardware_fixture.cpp`) |
| CLAP preset discovery | Presets listed in Oscairn's sound lists and prepared as instant-switch sounds | SDK [preset-discovery-plugin](https://github.com/oscairn/oscairn-clap/blob/main/docs/preset-discovery-plugin.md) | Basic Synth |

The sections below are what the host does around them, which no single extension's page covers.

### A plugin as a control source

A plugin that sends on parameter-target lanes can drive anything a knob can: the user maps a
lane straight to one parameter, or makes it a control's source, and the control then moves
every target it has, each with its own range and curve, absolute or relative, summing with the
other controls on the same parameter ([RFC 0011](rfcs/0011-performance-ui-and-editors.md#the-control-tree)).
The plugin never knows which. Routing's **Connect lane to...** lists the lanes and draws each
connection as a control line. A plugin whose only outputs are its lanes, with no audio or note
port, loads and runs as any node; the Oscairn LFO is one. What the plugin sends, the modes, the
timing and the 512-event limits are on the
[param-target](https://github.com/oscairn/oscairn-clap/blob/main/docs/param-target.md) page.

### Voice budget

The host shares a CPU budget by measured cost among the plugins that render on one thread (a
worker's or plugin host's audio thread, or the engine's process for plugins in it: a lane each).
A lane may take the budget set, but never more than 80% of the time its blocks have before they
are missed (a worker's allowance, the callback's time in the engine's process; RFC 0045's load
guard), budget or none, and the lanes together no more than the budget (with none, 80% of a
block) on each audio core, so notes give way before a block is ever missed and a worker is not
restarted for load (RFC 0018). A lane's notes give way only for its own limit; past the lanes'
total, or with a lane held off its core (its CPU time within its limit, its time not), the least
important voices of any lane give way (RFC 0046), as many as the excess. Notes fade (a few milliseconds); a note
is choked at once only when its lane's block would otherwise be missed. For a
plugin without `oscairn.voice-budget` it fades out and chokes whole notes, released and quiet ones
first ([Behaving well](#behaving-well)); after blocks are lost it also holds the notes it gives such a
plugin under the number that lost, a new note fading a released one, until a healthy stretch lets
them come back. A plugin taking notes as MIDI is in the budget too: a note
it gives up is sent its note-off, which no volume reaches, so its release plays out, and the time
it frees comes only as the release ends: keep releases short, or take CLAP notes. The host measures
each plugin's cost per note from its blocks; one whose report says no voices while notes sound (or
no cost) is costed by its notes as one without the report, and is held to its limit as one that
keeps within it only while the voices it reports, or else its notes, stay within it: a report of
zero never exempts a plugin from giving notes up. An effect is never given up, so its time is held back
for it: while it sleeps on silence it is still counted at its cost with signal, before the
instruments in front of it (their lanes leave room for the effects after them before the callback's
deadline), and a part's chain at admission counts the buses it sends to. An effect that sleeps
cheaply but costs much once woken should say so in its package estimate. A plugin with it gives up its own voices within a limit
the host sets between blocks; an instance one part plays alone gets one entry, for the selected
contexts (a plugin without sound contexts reads that entry), and parts sharing an instance get one
each with the part's reserve and priority. The limit can change before any block: it is at least the
voices of the notes the host has given the plugin and not taken back (a chord from silence is never
cut by the host's estimate), plus the room the lane has left at the plugin's cost per voice; it falls
below that only when the lane is measured over its limit, at most a quarter a block. When blocks are
lost or the callback runs out of time, the host lowers the limit by a voice rather than fading a note:
the plugin chooses which voice goes. The limit can also rise again while the plugin is playing, with
its notes still held, a voice at a time, and only where the host's measurements (its block's time and
the callback's) say the next voice fits: be ready to sound more voices at any block. The
guard never takes a part below one voice for lost blocks; where even that does not fit,
`engine.timing` says so (`unfit`).
The host fades a note from outside only as the backstop, when the plugin is not within its limit
by the block after it came in, or its limit is already 1, so keep within it at once. Parts sharing an
instance each get their own limit (their notes' voices and a share of the room); the host reads
`active_voices` for the instance as a whole. The host supports it in the engine's process and in a
worker alike. The calls and the order to give voices up in:
[voice-budget](https://github.com/oscairn/oscairn-clap/blob/main/docs/voice-budget.md).

#### Publishing the importance list (draft 2, RFC 0046)

So the host can weigh a pedaled piano note against a lead synth's held one in another part, an
instrument offering `oscairn.voice-budget/draft-2` (`OSCAIRN_EXT_VOICE_BUDGET_DRAFT_2`; the
unsuffixed `OSCAIRN_EXT_VOICE_BUDGET` stays draft 1's) reports after each block, through `importance`,
each sound context's 16 least important voices, least important first, and how many it sounds. The
host then takes the least important voices among those whose removal frees the time that is short
(the lane's own thread when one thread is over, any part's when the device's cores are), by
lowering a context's limit: never by fading your notes. Everything below is the SDK's; the Basic
Synth and the test fixture (`core/engine/tests/voice_fixture.cpp`) are the references.

**What a listed voice is.** Your unit of giving up: the same thing your voice limit and
`active_voices` count, since the host lowers that limit by the entries it takes. A sampler that
gives a note up whole (one slot starting all its layers) lists notes, and so does a VA whose
unison copies go only with their note; an FM engine whose voice is one note's oscillators lists
voices. Never list a part of a unit you cannot give up alone. Units
of unequal cost carry a `weight` (`Candidate::weight`): the unit's cost against your others, in any
measure you have (the layers or oscillators it runs: a 16-layer piano note 16, a one-layer note 1).
The host measures one cost per voice for the instance and shares it by weight; leave 1 where units
cost alike. **Units, not weight, are counted**: `voices`, your limits and the n the host lowers
them by all count units, and each listed entry is one unit whatever its weight, so `limit - n` is
exact. The unit must be what your limit counts. An engine whose notes hold several engine voices
it does not give up one by one (unison copies, a sampler's layers) counts notes: one entry per note,
its `weight` the engine voices or layers the note runs (a unison note of width w: w), and its limits
and `active_voices` in notes. One that gives up a layer or copy alone may count those instead, each
an entry of its own.

**What a voice's level is.** `Candidate::loudness`, linear: envelope level × velocity gain × volume
expression × the sound's own output level, 1 being the voice's sample or oscillator at nominal full
scale. Work it out from what the voice already holds; no pass over its samples. The host adds the
part's gain in the mix; the tier (released, pedaled, key held), the just-struck flag (under 50 ms)
and the age come from `held`, `pedaled` and `ageMs`, never from the level. The report's ages are
taken at the end of the block just rendered (the sound-contexts helper does it); the level is
clamped to -120 to +24 dB.

**The calls, with sound-contexts-plugin.hpp** (`oscairn::contexts::Engine`):

```cpp
static const oscairn_plugin_voice_budget_t budget{Engine::honorsVolume, Engine::setLimits,
    Engine::activeVoices, Engine::voiceNotes, Engine::importance};
// in get_extension: the same table for both drafts (OSCAIRN_EXT_VOICE_BUDGET is draft 1's ID)
if(!std::strcmp(id, OSCAIRN_EXT_VOICE_BUDGET_DRAFT_2) || !std::strcmp(id, OSCAIRN_EXT_VOICE_BUDGET)) return &budget;
// in your Engine (Derived), beside render:
double loudness(const Voice& v) const; // as above
float weight(const Voice& v) const;    // its layers or oscillators; default 1
```

The helper records on each voice whether a pedal holds it and when it started (from
`steady_time`), builds the list after each block, and keeps every context within its limit at the
start of the next: past a lowered limit it fades the voices it listed, in listed order, while they
still sound in their slots, then the least important by the order (`Importance::next()`), so a
context lowered by n gives up exactly the n it listed first. Past the instance limit it takes,
across contexts, the first listed of each by the order: the tier first (released or inaudible,
pedaled, key held), then the part's priority, so a released voice of a high-priority part goes
before a held key of a low-priority one.

**The calls, with your own voice pool** (voice-budget-plugin.hpp):

```cpp
oscairn::voices::Limits limits;          // set_limits: limits.set(limit, contexts, count)
oscairn::voices::Importance<> report;    // audio thread
oscairn::voices::Candidate candidate(const Unit& u) const
{   // context, selected, held (key down), ending (already fading), mono, loudness, age (larger newer),
    // pedaled (key up, a pedal holding it), ageMs (since its note-on, from your frame count), weight
    return {u.context, u.selected, u.keyDown, u.fading, false, u.envelope * u.velocity * u.volume * outputLevel,
            u.started, !u.keyDown && u.pedal, double(frames - u.startFrame) * 1000.0 / rate, float(u.layers)};
}
// At the start of each block: each context, then the instance, within its limit; what the last
// report listed goes first, in listed order (age MUST be a note-on serial unique to each note a
// slot holds, never a frame stamp or a time; a debug build asserts on a reused one).
for each context c: while(sounding(c) > limits.of(c, selected).limit) fadeOut(report.next(n, get, limits, c));
while(limits.limit && sounding() > limits.limit) fadeOut(report.next(n, get, limits));
// importance, called by the host right after each process() in which active_voices is not 0, on the audio thread
// (with no voice sounding no context is: the host takes that block's report as empty):
report.list(n, get, limits);             // get(i): &candidates[i], or nullptr for a free slot
return report.write(out, capacity);
```

`get` must describe the same units `victim()` sees, so build the candidates once per call from your
units' state at the end of the block. `list()` is one pass, no sort: each unit's key is worked out
once and kept only if it beats the 16th kept (1.1 µs a block for 128 units on the Mac).

**"Must drop exactly what it listed."** When the host lowers a context's limit from the voices you
reported (plus the notes it has sent you since) by n, at the start of your next block you give up
the n units you listed first, by fading them out (2 to 5 ms), and no others. `Importance::next()`
picks them: the units it listed, in listed order, while each still sounds in its slot, then the
order worked out again. Working the order out again alone (`victim()`) is not enough: levels, ages
and the 50 ms mark move between the list and the drop, so it can pick another unit. An engine that
keeps its own order instead must remember what it listed the same way. A unit already fading
is not counted in `voices` and is not listed. Never give up a unit the order protects (a held mono
key, held keys within the part's reserve): they are never listed, so the host never asks.

**The mismatch counter.** Two reports after it lowered a context's limit, the host checks that the
context sounds no more voices than any limit it was given since, while it still lists a voice it may
give up (a context over its limit with nothing listed is kept by rule 1: a part's reserve, a held
mono key). The host reads `voices` here, the units kept: `active_voices` counts the units fading out
too, for the fade's time after each drop, and the host does not judge it against the limit. A miss counts once in the node's
`importance_mismatch`, shown as `importanceMismatch` in `engine.timing`'s worker `guard`; Oscairn's tests fail
on any, and on the keyboard it should stay 0. The host goes on regardless: a plugin that does not
keep to its own list only costs its own sound, the host fading its notes from outside as the
backstop once it stays over its limit.

### Instant Performance changes

An instrument with native sound contexts and `oscairn.sound-snapshots` switches Performances
without a new instance or a state load: the next Performance's sounds are prepared beside the
playing ones and selected at the cue ([RFC 0021](rfcs/0021-sounds-performances-and-library.md#native-sound-contexts-across-a-change)).
With too few free contexts the change falls back to loading the snapshot into an instance. With
`ring_out`, the outgoing sounds ring out through the outgoing Performance's effects, within three
limits its users hear: one tail carries over (a second change cuts the first change's tail with a
3 ms fade), only the main output rings out, and an outgoing effect goes 30 seconds after the last
note through it. What the plugin must do:
[sound-snapshots](https://github.com/oscairn/oscairn-clap/blob/main/docs/sound-snapshots.md).

Any other plugin is usually reused across a change rather than loaded again: an instance of the
outgoing Performance, or one kept loaded since an earlier one, is reset (CLAP `reset`) and given
the incoming node's state, before Oscairn would start a new instance
([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#resident-instances-the-agreed-resident-engines)).
After that reset and state load it must sound exactly as a fresh instance given the same state:
nothing of what it played or held before may carry over.

A plugin carried over whole into a Performance change (one the next Performance does not take
over) rings out until it is idle, but no longer than 30 seconds after the change with no note
held through it, or 2 minutes with one held; a panic ends it at once
([RFC 0021](rfcs/0021-sounds-performances-and-library.md)). A voice it never ends is cut there,
with the instance.

### Parts sharing one instance

Parts whose instruments are the same plugin share one instance, on their own, each with its own
sound, MIDI channel and output bus ([RFC 0021](rfcs/0021-sounds-performances-and-library.md#parts-sharing-one-engine)):
adding the plugin where an instance of it has room puts the part's sound there; opening a
Performance fills instances with its parts of the plugin in order, each instance as many as it has
room for, the same way on every load whatever played before ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#the-same-layout-on-every-load-2026-11-13));
changing to one prepares its sounds beside a playing instance's, as above, selected at the cue on
channels of their own, only where all its parts of the plugin fit there. A part alone keeps an
instance of its own. An instance several parts share is measured once and its cost recorded per
sound, so admission counts each part at what one sound costs, shared or not: a package's
`--estimate`, measured on one part, is what each part of the plugin is counted at. The host sends a part's notes and its parameter edits on its channel, and
takes its sound from its output port. A plugin offers this with native sound contexts and their
snapshot with `save_sound` and `sound_values`, more than one output port, a stepped output-bus
parameter tagged `OSCAIRN_ROLE_OUTPUT_BUS`, no audio input and no note output; the full list,
with what the host checks at run time, is in the
[fallback matrix](https://github.com/oscairn/oscairn-clap/blob/main/docs/fallback-matrix.md#parts-sharing-one-instance).
The host puts a shared sound on its bus with one `CLAP_EVENT_PARAM_VALUE` for the output-bus
parameter: the plain port index as `value`, `channel` set to the part's MIDI channel, and
`note_id`, `key` and `port_index` all -1. It arrives in the first `process()` after the host
prepares the sound (from `prepare` values, or from the part's saved state through
`oscairn.sound-snapshots` `prepare`) and selects it on that channel, and again after each later
selection. Apply queued `select` calls at the start of `process()`, before input events; then
apply a channel-addressed parameter event to the sound that channel selects, never to a global
value or to every sound. The bus stored in a sound's saved state (0, as `save_sound` writes it)
is overwritten by this event: do not reapply saved state after preparing.
The host also activates each shared sound's bus port with `clap.audio-ports-activation` (see
[Several output ports](#several-output-ports)) before that sound plays, keeps it active while the
sound rings out after a change, and deactivates it once the sound is gone; main stays active.
An instance holds as many parts as it has sounds, ports and channels; the host takes another
instance past that. The Basic Synth (eight sounds, its Default one of them; eight sound buses
beside its main output) does all of it; the Legacy Sampler has one output, so each Legacy Sampler
part keeps its own instance, as every plugin without native contexts does. What its users hear
differently: the parts sharing an instance share its voice pool (the Basic Synth's 128, which the
shared voice budget rather than the pool limits in practice); a plugin meant to share should have
a pool sized for several parts.

### Several output ports

A part can take each of a plugin's audio output ports into a lane of its own, each with its own
chain of effects, joined again before the part's fader: a **ports split**
([RFC 0011](rfcs/0011-performance-ui-and-editors.md#split-and-join)). That is how a sampler sends
some elements through insertion A, others through insertion B and the rest past both: three
stereo outputs, main, "to insert A" and "to insert B", and a lane on each. The ports are found by
their stable IDs, so keep them, as CLAP requires.

The host tells the plugin which output ports it takes sound from with CLAP's
`clap.audio-ports-activation` (or its `draft-2` compat ID). Look at that and nothing else:

- Oscairn calls `set_active(plugin, false, index, on, 32)` for **every** output port of a plugin
  with more than one, once its wiring is first worked out (when it is added or its Performance
  loads), and again for each port whose state changes. A port is active when a
  wire takes sound from it (a lane of a ports split, or a wire drawn in Routing); the main ports
  are always active. On an instance several parts share, each part's sound bus is active while
  its sound plays or rings out, by the same rule. A part whose wires leave a port other than main
  never shares an instance, so a port is a lane or a sound bus, never both.
- Return true from `can_activate_while_processing`: then each call comes on the audio thread
  just before a `process` call, between blocks, and the change is heard in that block with no
  gap. A plugin that returns false is told while deactivated: Oscairn deactivates and activates
  it again (its sound stops and its voices end for that moment) at each change.
- Route what you would send to an inactive port into main instead. Treat a port the host has
  never named in `set_active` as inactive too: CLAP starts every port active, and a host that
  does not implement the extension (or one that only ever deactivates) would otherwise lose that
  sound. So with Oscairn, or any host, taking only main, main carries everything.
- The state is not saved: the host tells it again after every load, a worker restart included.

The element-ports fixture (`core/engine/tests/audio_fixture.cpp`, `org.oscairn.test.element-ports`)
does all of it in a few lines; `control-parts` plays it through a ports split. The Basic Synth
does it for its sound buses, and `control-shared-parts` checks each sharing part's own meter.

### Presets

Publish presets through CLAP preset discovery, and Oscairn lists them where sounds are picked and
prepares any of them as an instant-switch sound ([RFC 0005](rfcs/0005-sound-contexts.md#presets)):
a native instrument's through `prepare_preset`, any other plugin's with CLAP `preset-load` on a
spare instance. The protocol, the SDK helper, how the scan helper lists presets and its limits are
on the [preset-discovery-plugin](https://github.com/oscairn/oscairn-clap/blob/main/docs/preset-discovery-plugin.md)
page. The Basic Synth is the reference: five factory presets inside the plugin, load keys their
names in lower case and hyphenated. What each fork should provide (recommended; none is built
yet):

| Plugin | Locations | Load key |
| --- | --- | --- |
| Oscairn Organ (setBfree) | Inside the plugin: the 55 programs | The program number, `0` to `54` |
| Oscairn Analog (Surge) | Files: the factory and user patch folders, type `fxp` | None: each file is one patch |
| Oscairn FM (Dexed) | Files: the cartridge folders, type `syx` | The voice in the cartridge, `0` to `31`, each named from the cartridge |

What a Sound saves is the plugin's own state (for native contexts, the snapshot of every prepared
patch), never a reference to the preset, so it opens when the preset is gone.

### Editor hints and roles

`oscairn.ui` gives each parameter a control, a priority (which orders its tiles, the Compact list
and the physical knobs), a group, a role (its knob in the instrument role banks: Filter, Amp EG, EQ,
Levels; an effect's `mix` goes to the Effects bank) and its place in a rich view. A plugin that
declares no role gets roles guessed from exact parameter names, which misses most spellings, and
declaring any role turns the guessing off for its other parameters, so declare every role it has.
Every parameter stays reachable whatever its hint. The fields, the drafts and the view rules:
[ui](https://github.com/oscairn/oscairn-clap/blob/main/docs/ui.md).

### Pages, tabs and value icons

Draft 6 of `oscairn.ui` lets a plugin declare more than hints. With `tab_count`, `tab` and
`param_tab` it names and orders its own pages and tabs (two levels: pages, and under each a row of
tabs) and says which one each parameter is on, whatever its module path; with `value_icon` it names
an icon, from a closed set of waveform shapes the host draws in its own style, for each value of a
stepped parameter. All four are optional members after `seconds`, and a plugin gives names and
ids, never a look.

**Oscairn does not use them yet.** Both CLAP hosts ask for draft 6 first, then 5, 4 and 3, but read
a draft 6 table only through the members of draft 5: the pages and tabs are still made from the
module paths and the icons are still guessed from a value's text where it names a waveform. Declare
them now and they take effect when the host draws them; nothing breaks before. The rules, so you
can declare them correctly:

- Entries are `oscairn_ui_tab_t` (an `id`, a `parent`, a name of at most 63 bytes). A page has
  `parent` 0; a tab names its page. Ids are nonzero, unique and stable across rescans. The host
  leaves out an entry with a bad id, an empty name or a parent that is not a page, and reads at
  most 16 pages and 16 tabs under each.
- A parameter on a tab is drawn on that tab; on a page itself, above its tabs. A page with fewer
  than two tabs is drawn without tabs. A parameter you leave out, or a plugin that declares
  nothing, is placed from its module path as today, so declaring is never required.
- `value_icon` is asked only for stepped parameters of at most 128 values. Once any value has an
  icon, the host guesses none for the others.
- One table may answer the draft 6, 5, 4 and 3 IDs: a host reads only the members its draft has.
  Answer only the draft 6 ID if you want no host older than it.

The fields and rules in full:
[ui](https://github.com/oscairn/oscairn-clap/blob/main/docs/ui.md#pages-and-tabs-tab_count-tab-param_tab-draft-6-each-may-be-null).

### Rich views for Oscairn's engines

What the forks should declare, as worked examples of the views
([RFC 0019](rfcs/0019-adaptive-ui.md#step-5-status); the fields and rules are on the
[ui](https://github.com/oscairn/oscairn-clap/blob/main/docs/ui.md#rich-views) page). `param_hint`
must return true for **every** parameter in a view, also those with no role or widget: a plugin
that returns false for parameters without a role, as the forks do today, must change that.

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

With `oscairn.memory`, the host asks what activating would allocate before every activation, and
with a memory budget refuses a plugin that would not fit before it allocates ("Plugin needs N MiB
of memory where M MiB is left"). Oscairn counts the larger of the declaration and its own
measurement, and keeps its measurements per declared size, so a plugin's modes (a reverb's
algorithms) are not counted at the largest. In a worker process, declarations also set the worker's
memory limit ([In a worker process](#in-a-worker-process)). The numbers, the calls and how sound
and effect contexts are counted:
[memory](https://github.com/oscairn/oscairn-clap/blob/main/docs/memory.md).

### Hardware backends (draft)

`oscairn.hardware/draft` ([RFC 0004](rfcs/0004-hardware-services.md#step-1-status)) is for a
plugin that has, beside its CPU path, a backend for an accelerator a device offers. The host lists
the accelerators as the device's profile records them: the service ABI each speaks, its operations,
firmware, banks and capacity. In `activate`, pick one whose `abi` you have a backend for and
`reserve()` what you need (operation, voices, memory, an exclusive region, transfer sizes, and
`cpu_fallback_us`, your CPU path's time per block); on a refusal, use your CPU path. In `process`,
`submit()` a job for this block with `steady_time` and `collect()` its result. A late job: render
that block on your CPU path. A failed accelerator: continue on your CPU path if the lease says
`cpu_fallback`, else output silence; the host reports it. Offline (`clap.render`), use your CPU
path. `release()` in `deactivate`. Your state holds nothing of the accelerator, and your backends
must match your CPU path within a tolerance you state; a different sound is a saved mode of its
own. Every desktop and every product so far offers the extension with no accelerators, and a plugin
in a worker process sees none, so the CPU path is the one that plays today; Oscairn tests the
contract with a simulated accelerator. The header is in oscairn-clap (and Oscairn's copy of it)
and changes without notice.

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
  give a large page second levels of a dozen or so parameters each. The shell draws pages
  and tabs as one themed family of tabs, the pages a strip over the tabs (a column at the
  left on a tall window), and a plugin cannot change how they look. Draft 6 of the layout
  extension (`oscairn.ui/draft-6`, `tab_count`, `tab`, `param_tab`) lets a plugin name and
  order its own pages and tabs and place parameters on them, whatever the module paths; the
  UI is ready for it but the engine does not forward it yet, so declared tabs do not show
  until it does (ui/TABS.md). A parameter placed on a page that has tabs, but on no tab, is
  drawn above the tabs, whichever is open. `value_icon` names an icon for a stepped
  parameter's values (a waveform's shape) in the same way, with the name rule (Saw, Sine...)
  as today's fallback;
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
- a value the plugin changes itself (its own preset browser, a mode that resets other
  controls) reaches the editors only as CLAP asks: a `CLAP_EVENT_PARAM_VALUE` output event
  from `process()` or `flush()` (`request_flush` when it is not processing), or
  `clap_host_params.rescan(CLAP_PARAM_RESCAN_VALUES)` from the main thread after changing
  several at once. The host does not read every value on its own: a value changed without
  either stays stale on screen until the next rescan;
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
  `oscairn.ui` views ([ui](https://github.com/oscairn/oscairn-clap/blob/main/docs/ui.md#rich-views)), and dragged to
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
  in `reset()`: the player's Panic (`notes.panic`) resets every plugin that takes notes, behind
  a note effect too. Keep parameter values through `reset()`: the host drops the notes and
  controllers of the block it resets in, and still sends that block's parameter changes, after
  the reset.
- **Chokes and per-note volume:** treat a host `NOTE_CHOKE` as a fast fade of 2–5 ms, not a
  cut, and honor `CLAP_NOTE_EXPRESSION_VOLUME`. When a part's note limit, or the shared
  budget across parts, steals a note, the host lowers its volume expression to silence over
  5 ms in eight steps, then chokes it ([RFC 0018](rfcs/0018-voice-budget-and-stealing.md)); a
  plugin that does neither clicks. The shared budget costs each note by the plugin's measured
  time per block (`process`'s wall time above its time at rest, shared among the notes it holds
  until their `NOTE_END`), so a plugin that sends `NOTE_END` late, or never, is charged for
  voices it no longer plays (until its output has been silent for 2 s, when the host lets go of the
  released notes it never ended; a release still sounding is never cut); and it steals only CLAP notes, never a MIDI-only plugin's. An instrument built on the
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
- **Asleep means not called:** after a block that returned `CLAP_PROCESS_SLEEP`, the host does
  not call `process` again until something comes for the plugin: audio input that is not
  silent, an event (notes, parameters, MIDI), or a transport change (play, stop, tempo, a jump).
  The beat and harmony events do not wake it: a plugin that asked for them gets them with the
  block that does (the beat at the free-running position of that block, the part's harmony if
  it changed while it slept), so an idle arpeggiator that returns `CLAP_PROCESS_SLEEP` is not
  called while idle but on a transport change. Blocks it sleeps
  through still count in `steady_time`. This holds in the
  engine's process and in a plugin worker, where an uncalled block also saves the round trip.
  The host also calls `process` on the block after an accepted `select` (sound contexts), a
  queued `choke_retiring`, `request_process` or `request_flush`, a host command (a parameter
  set, a gesture, an event sent outside a block) or a panic, even if the last status was
  `CLAP_PROCESS_SLEEP`. New voice limits (`oscairn.voice-budget`'s `set_limits`) do not wake a
  plugin; it takes them with its next block. A plugin that owes `CLAP_EVENT_NOTE_END`s after
  a state load (notes it ended while asleep) calls `request_process` so it gets a block to send
  them in.
- **Loading while others play:** while a Performance loads, the player's notes keep reaching
  the Performance playing, and its meters are read
  ([Long requests](control-protocol.md#long-requests)), as the host creates the new instances
  and gives them their state. Every call to a plugin still comes on its main thread (the
  engine's control thread, or the worker's): the host never creates or restores an instance on
  another thread. A plugin playing may so get notes on the audio thread while another
  instance of it, in the same process, loads its state on the main thread: keep
  nothing shared between instances that a load changes without a lock of the plugin's own.
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
  ([RFC 0025](rfcs/0025-resource-budgets.md)). CPU is a running mean of its own CPU time per block
  (its worker's audio thread's; in the engine's process the time of its `process` call), as a share
  of the block period, so a stall of the thread is not counted against it, split into a
  cost at rest (no key held and its output silent for half a second: a release tail is not rest) and a cost per sounding note, with the busiest
  recent load for a part with no note limit; in a part with a note limit, the estimate is at rest plus that many
  notes, so keep your per-voice cost proportional to the voices actually sounding. In a worker,
  memory is what the worker holds of its own (on Linux, not the worker binary and libraries every
  worker shares, which count once); in the engine's process, it is what the process grew by
  while the plugin loaded. A plugin never measured on the device counts as 2% of the block's
  time until it is, and as memory what it declared through `oscairn.memory` when it last played
  on the device, else its package's estimate, else 8 MiB plus the size of its binary (a locked
  worker holds the binary's code resident); so declaring your memory makes the guess yours. A new version starts unmeasured, and so does a new build of the
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
- **Time limit:** in real time, a block may take half the block's time (the worker's allowance,
  `OSCAIRN_WORKER_BUDGET_US` or a profile's `compute.workerAllowance` to try others), counted
  from when the engine hands it over; a later one is silenced, and `session.status` counts it as
  a silent block apart from one played late. A late block is not a reset: the plugin finishes
  it, its sound is discarded (its output events come at the start of the next block heard), and
  its voices, envelopes and held notes go on; the dropout fades out and in over about 1 ms. While
  the plugin is still busy with a late block, the engine hands it no new one and keeps those
  blocks' events (at most one block's, releases kept first) for the next block it hands over,
  each at time 0: after a stall, notes and note offs may arrive one or more blocks late, dropped
  only past that bound, where a release that does not fit resets the plugin ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#the-miss-policy)). A plugin listed in the experimental
  `OSCAIRN_WORKER_EXTRA_BLOCK` gets a period more for each block and is heard one block late:
  it sees the same blocks, events and transport, so a plugin that follows the transport (an
  arpeggiator, a sequencer) sounds a block behind the others
  ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#one-block-late-for-one-part)). Plugins that do not feed each other render at
  the same time, each in its worker ([RFC 0034](rfcs/0034-parallel-graph-rendering.md)); effect
  contexts of one instance never do. Missing a quarter second of blocks in a row (at least 8), or leaving one
  unanswered for 250 ms, is a failure; late blocks in the first two seconds after a load, a Performance opened or
  the first restart within a minute are silenced but not counted (the start's own), and not
  measured as the plugin's cost; a plugin too slow in itself is given up in about 5 s.
  Offline rendering waits instead.
- **Output range:** a block with a sample past +120 dBFS (a million) is not heard, in a worker or in the
  engine: it fades out over 1 ms from the last good samples, the plugin playing on unrestarted,
  counted in `session.status`'s `badOutputBlocks`; a quarter second of such blocks in a row lists
  the plugin among `failures` until its output is back in range. A sample that is NaN or infinite
  stops the plugin at once, as before.
- **Memory limit:** 4 GiB by default (`OSCAIRN_WORKER_MEMORY_MB` changes it; 0 is none), read
  every 10 ms as the physical footprint on macOS and on Linux as resident memory less what the
  worker held at its start, before your plugin loaded. When every
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
  restarted. Each plugin gets back, in this order: its state as last loaded (with native sounds,
  a snapshot of the whole instance taken when sounds were last prepared beside the playing ones),
  the sounds prepared since and those discarded since, each channel's selection, its parameter
  values as last published (only where every channel selects the same sound: on an instance
  parts share they are one sound's), its active output ports and its voice limits. Then, at the
  start of its first block, before that block's own events, it gets again what it keeps only in
  memory: on each channel the last mod wheel, breath, foot, expression, sustain, sostenuto and soft
  controllers, pitch bend, channel pressure and channel-wide note expressions, and every
  parameter value and non-zero modulation sent to a channel (or to all) since that channel last
  selected a sound or a state was loaded: a shared sound's bus event, edits, values and
  modulation from controls. So apply queued selections before the block's events, and a
  channel-addressed event to the sound that channel selects then. Its output fades in. Held notes
  and tails are lost and notes are not played again: a note restarted from its attack would be
  heard as a wrong note, where the controllers keep the next notes as they were. After three
  restarts within a minute it stays silent.
- **Sharing a process (plugin hosts):** with `OSCAIRN_PLUGIN_HOSTS` (a device profile's
  `compute.pluginHosts`; 3 on the FANTOM-07) several instances, of any plugins, share one worker
  process, a plugin host ([RFC 0045](rfcs/0045-audio-scheduling-on-small-devices.md#plugin-hosts-and-resident-instances)).
  Nothing new may be assumed: a plugin already had to keep nothing per process in globals
  shared between instances, and that now covers other plugins too (another plugin's libraries,
  allocator use and threads are in the same process). Every instance in a host is processed on
  that host's one audio thread, one after another, never two at once, and on a device the time
  limit is the audio callback's deadline, which the host's instances share, not half a block each; instances are loaded into
  and unloaded from a host on its main thread while the others keep playing. A crash or hang in
  any plugin takes the whole host down: every instance in it is restarted as above, and the
  plugin it was put down to gets a worker of its own from then on: for a crash, the one the
  crashing thread was in (the host's audio thread's block or its main thread's call; none for a
  thread of a plugin's own); for a hang, the block or call not finished; for a load that never
  answered, the plugin being loaded; for blocks too slow, the costliest. A host failing three
  times in a minute for no plugin known (memory over its limit, a crash on a plugin's own thread)
  is not given up: its plugins each go to a worker of their own, which give up alone. The memory
  limit is the host's, from every declaration in it; `session.status` shares the host's CPU time
  and memory among its instances.
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
- **Locked memory and large files:** on a device (the instrument and pass-through levels, Linux)
  the worker locks its memory before it loads a plugin: `mlockall(MCL_CURRENT | MCL_FUTURE)`,
  `MCL_FUTURE` where `RLIMIT_MEMLOCK` is unlimited, as on an Oscairn image ([RFC 0025](rfcs/0025-resource-budgets.md)).
  So everything you allocate is resident and locked from the moment it is mapped, which is what
  the audio thread needs, and so is every file you `mmap` readable: the kernel reads the whole
  file in at the `mmap` call and keeps it locked, unevictable, counted in the worker's resident
  memory and its memory limit. A sample pack is up to 1 GiB; mapped, it cannot fit a 1 GB
  device, and the out-of-memory killer ends your worker or others. So:
  - **Do not map large files** (sample packs, the paths `resolve_sample` gives). Open a
    descriptor and read them with `pread` (after `posix_fadvise(POSIX_FADV_WILLNEED)` if you
    like): what you need at prepare (each region's head, loop bodies) into memory you allocate,
    and what you stream on a thread of your own, never the audio thread, into a small fixed
    buffer you allocate once. That memory is locked, as it should be: declare it with
    `oscairn.memory`. The host never locks the page cache, so what `pread` reads through it can be
    taken back by the kernel, and nothing in the worker stops you opening and reading the store's
    files from your own threads.
  - **A mapping you want unlocked** (a small table of contents, an index): `munlock` needs no
    privilege and works under the lock, but after `mmap` the file has already been read in
    whole. Map it `PROT_NONE`, `munlock` the range, then `mprotect` it `PROT_READ`: nothing is
    read in by the `mmap`, its pages are read as you touch them and are not locked. Never touch
    such pages on the audio thread: a page the kernel took back is a disk read there.
  The engine reads the store's files the same way.
- **How their CPU counts:** the budget ([Behaving well](#behaving-well), RFC 0025) measures
  each block's `process` call on the worker's audio thread, in wall time, as in the engine; the
  time limit is how long the engine waits for the block. Neither counts a helper thread's CPU
  time, unless that thread slows the audio thread by taking its core. The worker's whole CPU
  time, every thread's user and system time, is what `session.status` reports (shared among the
  plugins in that worker), and the budget does not use it. So work moved off the audio thread
  is not charged against the budget; on a device with few cores it still competes with audio.
  On an Oscairn image the engine and every worker run only on the profile's audio cores (one of
  four on the EVK and the QEMU image; RFC 0035), so your threads share those cores with the
  audio threads, at a lower priority: a helper thread there gets the time audio leaves, not a
  core of its own.

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
  `@rpath` into the bundle; Linux: [the allowlist](#the-linux-allowlist) below), so link
  Homebrew's or `/usr/local`'s statically or bundle them.
  `oscairn-sign plugin-package` and `oscairn-plugin-check` check it
  ([the rules](library-archive.md#plugin-packages)). When a device's store already has a different
  build of the same version for a platform, it keeps that one and notes that the incoming build
  was not installed, so give a changed build a new version. The Catalog shows such a package as
  Installed (a different build), not as the repository's, and replaces it with the repository's
  build only when the user chooses to. LV2 plugins
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
  `--estimate <platform> <cpu> <memory-bytes>` gives a build its resource estimate
  ([RFC 0025](rfcs/0025-resource-budgets.md#status)), as `oscairn-plugin-check` prints it
  on that platform: a device that has not measured your plugin itself counts it at this,
  not at the default of 2% of the audio time and its memory guess, when it checks what fits. Installing
  a Performance or Sound that will not fit says so, and the library marks it.
  Samples or patches your plugin reads ship as an asset package (`oscairn-sign asset-package`),
  named by your plugin package with `--asset-dependency <library-id> <least-version>`: the
  Catalog lists it under Content, and one installed only because your package needed it says
  Installed with your package's name.
  To publish to Oscairn's package repository, sign the archive too and follow
  [Publishing a package](developer-guide.md#publishing-a-package). Submit one archive holding
  every platform's builds, signed once; the repository stores it in pieces, so a device
  downloads only what all platforms share and its own platform's builds
  ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#pieces-the-format)), and makes a
  **ready-made archive per platform** of each version from them, with your manifest and
  signature unchanged and the other platforms' builds left out. Link to those from your release
  notes, never to a file in the submissions repository:
  `https://packages.oscairn.org/exports/<package ID>/<version>/<package ID>-<version>-<platform>.olib`
  (`<package ID>-<version>.olib` for a package with no builds), which sends a browser on to the
  file. An archive that leaves out builds its manifest lists is read only where the left-out builds
  are for other platforms: one for the reading device's platform must be there. `oscairn-sign
  verify` lists each left-out build as an `absent ...` line, and `--platform <platform>` requires
  that platform's builds; `oscairn-sign archive` refuses to sign such an archive again (its manifest
  would lose those builds), and the repository refuses it as a submission.
- **Installing code:** a device installs a build signed by a key its trust store trusts for
  plugins without asking. A build that is unsigned or signed by an unknown key is installed
  only after the user agrees (`acceptUnsignedCode`; the desktop's Install anyway). Installing
  from a repository a build signed by the key the repository lists as your publisher key, the
  desktop asks once whether to trust you to install plugins, naming you, the repository and
  your key's fingerprint, with no Install anyway; once trusted, your later builds install
  without asking. Nothing trusts a publisher on its own: the repository listing a package does
  not, and only the Oscairn project's own key is built in. A build
  signed by a revoked key is never installed. A repository's revocation list can withdraw a
  version of your package or revoke your key: that version, or everything your key signed, is
  then never offered or installed from it (from any repository when the official one revokes your
  key), and devices that have it installed keep it and tell their user who withdrew it and why
  ([RFC 0031](rfcs/0031-package-repository-and-distribution.md#the-revocation-list)). A revoked
  build stops only its own platform: devices on it refuse it, and that platform's ready-made
  archive is no longer made; other platforms keep the version. An archive installed from a file
  is checked against the lists the device last read from the repositories it has on. To retire a
  bad release yourself, yank it: a yanked version still installs where a lock names it.
- **A plugin that is not here,** in another version, or that fails to load opens as a
  stand-in: silent, its ports and parameters as saved, its state written back unchanged, so
  installing the plugin later brings the sound back.

### The Linux allowlist

A Linux build (`linux-arm64`, and `linux-x86_64` too) runs on the device's image, so it may
load only what every image carries and need no newer glibc or libstdc++ than the image's:

- **Libraries (`DT_NEEDED`):** glibc's `libc.so.6`, `libm.so.6`, `libmvec.so.1`, `libdl.so.2`,
  `libpthread.so.0`, `librt.so.1` and the loader, `ld-linux-aarch64.so.1`
  (`ld-linux-x86-64.so.2` on x86-64);
  GCC's `libgcc_s.so.1` and `libstdc++.so.6`; ALSA's `libasound.so.2`; and the windowing and
  font libraries `libX11.so.6`, `libXext.so.6`, `libXrandr.so.2`, `libXinerama.so.1`,
  `libXcursor.so.1`, `libXrender.so.1`, `libfreetype.so.6` and `libfontconfig.so.1`. Each is in
  both images below. Link everything else statically, even what an image happens to have for
  the host (`libsndfile` is in the QEMU image's and not the EVK's): not `libGL` (no image has it).
- **No RPATH or RUNPATH.** A Linux `.clap` is one file, with nothing beside it to find.
  (`oscairn-sign` accepts `$ORIGIN` entries only so a build folder can find its own libraries;
  any other entry is refused. Oscairn's own releases have none, and the package repository's
  import refuses any.)
- **Symbol versions (`DT_VERNEED`):** none newer than the image's, and no `GLIBC_PRIVATE`:

  | Image | glibc | Newest `GLIBC_` | libstdc++ | Newest `GLIBCXX_` |
  | --- | --- | --- | --- | --- |
  | QEMU ARM64 (`oscairn_qemu_aarch64_defconfig`) | 2.44 | `GLIBC_2.44` | GCC 15.3.0's, `libstdc++.so.6.0.34` | `GLIBCXX_3.4.34` |
  | i.MX8M Mini EVK (`oscairn_imx8mm_evk_defconfig`) | 2.44 | `GLIBC_2.44` | GCC 15.3.0's, `libstdc++.so.6.0.34` | `GLIBCXX_3.4.34` |

  Both are Buildroot 2026.08.1 (`os/scripts/container-build.sh`) with its own toolchain: its
  `package/glibc/glibc.mk` has glibc 2.44 (`2.44-57-g34106fee`), its default GCC is 15.3.0, and
  `readelf -V` of each built image's `/lib/libc.so.6` and `/usr/lib/libstdc++.so.6` gives the
  newest versions above. `oscairn_qemu_aarch64_rt_defconfig` has the same toolchain lines and
  was not read built. A build made on GitHub's `ubuntu-24.04-arm` runner (glibc 2.39, GCC 13)
  fits: Oscairn Arpeggiator and Oscairn Effects built as their CI builds them need only
  `libc.so.6`, `libm.so.6`, `libstdc++.so.6`, `libgcc_s.so.1` and the loader, up to `GLIBC_2.38`
  and `GLIBCXX_3.4.32`. A build made on a newer distribution than the image can need a newer
  version, and then does not load on a device.

`oscairn-sign plugin-package` refuses a build that breaks any of these, reading each ELF file
itself (no `readelf`); `oscairn-sign verify` lists each problem as an `outside` line, which the
package repository's checks refuse, and `oscairn-plugin-check` reports it. The libraries and
the newest versions are in `core/control/src/libraries.cpp`, tested by `control-libraries`;
raise the versions with the image's Buildroot, never ahead of it.

## Testing

Copy the reference plugins' tests in `core/engine/tests/` and `plugins/`: deterministic
offline rendering with `oscairn-render`, rendering that never allocates, state round trips,
and the same tests with the plugin in a worker process (the `*-workers` tests). Run them
under AddressSanitizer and ThreadSanitizer. Two checks run on a plugin as built, without
writing tests:

- `oscairn-plugin-check PLUGIN ID synth|effect [--param ID=VALUE] [--state FILE] [--asset NAME=FILE]` (built as
  `build/core/control/oscairn-plugin-check`): the path a musician's sound takes. The plugin
  plays in a part, is saved to the library and opened again with every parameter as saved, is
  exported in an archive with its build, and is installed on a fresh device, where it opens
  from the plugin store and plays the same. `--juce-state NAME=VALUE` sets an attribute in a
  JUCE plugin's saved state, as choosing a program in its window would. With
  `--add-build linux-arm64=PATH --keep-archive FILE`, the archive also carries another
  platform's build; `oscairn-plugin-check --install FILE synth|effect` on that platform installs
  it and checks it opens from the store and plays. A note effect (CLAP feature `note-effect`,
  or note ports in and out with no audio output) is played notes and must send notes on, through
  its part's MIDI out, instead of making sound. It prints one line per check, and what the
  plugin cost while it played (its busiest 256-frame block at 48 kHz as a share of the block's
  time, and its memory) as the `--estimate` for `oscairn-sign plugin-package`.
  **An instrument that is silent until it has content** (the Sampler with no sound) fails
  "plays" as any silent plugin does; the check has no allowance for it. Give it content:
  `--state FILE` is the plugin's state as it saves one (a CLAP state; for a plugin with sound
  contexts, a sound's `state.clap` from a library's `Sounds/.../nodes/<key>/state/`), and each
  `--asset NAME=FILE` is a sample (`.osp`, a one-record Oscairn Sample Pack named by its sample
  id) or waveform (`.owf`) that state names as `sample:<id>` or `waveform:<id>`. The check
  installs the files as a sample package installs them (with the sample item types), puts the
  state in the plugin's saved snapshot and opens it, so the state reaches the plugin as a saved
  Sound's does, before the first render. Every later step then runs with the content: the
  Performance is saved with the state, the export carries the samples and waveforms the plugin
  lists as its asset dependencies (so it must list them, `oscairn.asset-dependencies`), and
  "plays there" is on a fresh device that has only the archive. A state that names a sample not
  given fails with "the content (state and assets) did not produce sound". Only samples and
  waveforms are supported as assets, found through `resolve_sample` or `resolve` with an empty
  library; a plugin that reads files from an asset library by path is not covered.
  `--library FILE --sound ITEM` installs a library archive as a locked library and opens its
  Sound in the part as the library does. The check first installs the plugin under check in
  its temporary store as a plugin package, as a user has it, then the library; the Sound opens
  only when its node names that plugin ID and version, the audio ports the plugin reports and a
  `pathHint` that misses or names this build (the store finds it by ID and version). The export
  then carries the plugin, the Sound, its samples and the types. An archive checked with
  `--install` needs no content options: it holds the content.
- `core/lv2-host/tests/production/run.py PROBE OUT --case NAME,PATH,ID,synth|effect,PARAM,VALUE`:
  renders at 44.1 and 48 kHz, 37- and 512-frame blocks, offline and realtime, a fresh-instance
  repeat and a parameter edit that must change the sound
  ([how](../core/lv2-host/tests/production/README.md)). Its probe,
  `oscairn-lv2-production-probe`, is built only with `-DOSCAIRN_LV2=ON`, even for CLAP plugins.

Set `OSCAIRN_PLUGIN_WORKER=build/core/oscairn-plugin-worker` to run either check with the
plugin in a worker. On a running engine, `oscairn-engine-timing` prints each plugin in a worker
(an instrument with sound contexts once, with the parts it plays for), its block round trips as the engine sees them (p50 to the maximum), taken apart into the worker's
wake-up and its run, the CPU time its audio thread used in it and its page faults, beside the audio callback's own
([Tools](developer-guide.md#tools)). See
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
- **LFO** (`plugins/lfo/lfo.cpp`): a modulator with no audio or note output, a MODULATE lane
  sent every 32 samples or closer at high rates (to 1,357 Hz), a log-scaled rate with value texts
  in Hz, transport sync, and `oscairn.ui` hints.

## Known limits

- **The plugin's window** is a window of its own on macOS, beside the app's, never inside it.
  On Linux and Windows the host makes no windows yet, so only a plugin that floats its own
  shows one. A plugin placed in the engine's process rather than a worker is unverified.
- **Instant sound switching** for plugins without native sound contexts uses a whole extra
  instance per prepared sound (up to eight), and only for instruments: note input, audio
  output, no audio input, no note output.
- **Not implemented:** remote controls, voice info, note names,
  audio-ports configurations, and watching preset folders for changes (they are
  read when listed). (From the host's source; not exercised with a plugin.)
- **In a worker,** a restarted worker's parameter info shows only after its next rescan.
- **Declarations** are not yet distrusted after a plugin holds more than it declared, and
  `oscairn.memory`'s `current` is not used for budgets.

## Licenses

Keep a reused engine's license and notices with it and credit its authors in
[CREDITS](../CREDITS.md). Chillbeast's Modular shows the pattern: imported unchanged with its
history, wrapped as a CLAP plugin beside it ([plugins/modular](../plugins/modular/README.md)).
