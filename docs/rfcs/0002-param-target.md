# RFC 0002: Parameter changes between plugins (oscairn.param-target)

**Status:** draft, implemented

## Progress

Checked when built and tested; the sections below say how.

- [x] The host offers `clap.event-registry` and accepts validated param-target events
- [x] Mappings in the graph, converted to the target's parameter events in the same block
- [x] Lock lanes in the step sequencer; Basic Synth parameters to lock
- [x] `oscairn-render --map`
- [x] A lane drives a control of [RFC 0011's control tree](0011-performance-ui-and-editors.md#the-control-tree), flattened into the graph at commit ([below](#lanes-driving-controls))

## Problem

A sequencer's per-step parameter locks, and automation lanes in general, need to change
parameters of *other* plugins: the synth's cutoff on step 3, the delay's feedback on step 9.
CLAP has no way to do this. A plugin's output `CLAP_EVENT_PARAM_VALUE` names only its own
parameters (`param_id` and `cookie`, no plugin instance), and no stable or draft extension
addresses another plugin's parameters (prior-art review, CLAP ecosystem notes).

## Decision

A small CLAP extension, defined in `core/clap-ext/include/oscairn-clap/param-target.h`.

- **Lanes, not addresses.** A plugin sends `oscairn_event_param_target` events on numbered
  output *lanes* (for the step sequencer, lanes 0/1 are the two track locks). It does not know what a lane
  controls. The host keeps a mapping from (plugin, lane) to (target plugin, parameter), set
  by the user or UI: `Graph::mapParamTarget(source, lane, target, param)`. Plugins stay
  independent of each other and of the host's node IDs, and one sequencer works with any
  instrument.
- **Values are relative to the target's range.** `SET` carries 0..1 over the target
  parameter's range; `MODULATE` carries an amount of -1..1 of that range. The host converts
  to the target's plain values (rounding stepped parameters).
- **Standard CLAP events at the target.** The host turns each lane event into a
  `CLAP_EVENT_PARAM_VALUE` (SET) or `CLAP_EVENT_PARAM_MOD` (MODULATE) for the target, at the
  same sample, with port, channel and key fields copied over. An explicit note ID is
  remapped through the source-to-target note connection; without that connection the
  per-note lane event is dropped and counted. Target plugins need
  no knowledge of the extension.
- **Same block, sample ordered.** A mapping is a dependency: the target is processed after
  the source in every block, so the converted events arrive in the block they were sent, at
  their exact sample. Global/channel/key locks are ordered before notes at the same sample,
  so a step's lock applies to that step's note. Explicit per-note events follow same-sample
  notes so the destination ID exists before the event is delivered.
- **Discovered through CLAP's own mechanisms.** The event space is obtained with the stable
  `clap.event-registry` extension (name `oscairn.param-target`); the Oscairn host returns
  space 1000. A host that does not know it returns false and the plugin sends nothing.
  Lanes are listed through the plugin extension `oscairn.param-target-lanes/draft` (count,
  and a name for each lane) so a UI can offer them: the host reads them when the plugin loads
  (in a worker too), and `graph.get` lists each plugin's `lanes`.

### Lanes driving controls

Decided 2026-11-08. Besides a direct mapping, a lane can drive a **control** (RFC 0011): the
control's `source` is (plugin, lane), and the lane's events reach every parameter under the
control, through its targets' ranges and curves, absolute or relative, and the summing rule,
as when the control's knob moves.

- **The lane gives the control its value, 0 to 1.** A SET event's value is the control's
  value. A MODULATE event's amount (-1..1) is added to the control's own value (its knob, saved
  in the Performance), clamped to 0..1: around the control's value rather than around 0.5, so
  the knob sets the center a modulator swings about, and a control left at 0.5 is swept whole
  by a full bipolar swing (a unipolar one, 0..1, with the control at 0).
- **The control fans out as its knob does**: to any number of targets, each with its own range,
  curve and absolute or relative setting.
- **Delivery is decided per target, not by the lane's mode.** An **absolute** target gets a
  `CLAP_EVENT_PARAM_VALUE`, which sets the parameter: the value the control gives it there, with
  every other control's offset on that parameter where it is (the summing rule). A **relative**
  target gets a `CLAP_EVENT_PARAM_MOD`: its offset (its point in its range less a half) on top of
  the parameter's own value, which stays as it is, so the saved Performance and undo are
  untouched and the parameter can still be turned by hand while it is modulated. A relative
  target of a lane-driven control is therefore left out of the summing rule's stored value, and
  moving that control's knob does not set it; it moves at the lane's next event. Relative
  targets of a plain knob's control, and those under a control the lane drives through another,
  still move the stored value as before (switching every relative target to CLAP modulation is
  a larger change, not made).

That is how CLAP itself separates the two: a parameter's value against modulation of it. A
MODULATE lane's natural targets are relative ones, which the Routing view makes when a lane is
connected to a parameter there (RFC 0011's control lines).

**Flattened at commit, sample-accurate.** The control thread is not in the per-sample path:
when the graph is committed, the Session flattens the tree under each lane's control into its
final parameters (`Graph::setControlTargets`), each a path of curves from the control down
(the control targets on the way, then the parameter's own) with the summing rule's rest (the
base and every other control's offset) folded into the last. The graph converts each lane
event at every one of them in the audio thread, in the same block and at the same sample,
with no allocation; the targets come after the source in the processing order. The flattening
is redone at every commit and when a control is played (its value, or another control's on the
same parameter, changes the rest), handed over with the plan as everything else is. Part levels,
mutes and morphs under such a control follow only its knob.

A direct mapping of a lane (`mapParamTarget`) is kept as it was, beside any control the lane
drives: the sequencer's locks use it.

## Implementation

- `core/clap-host`: the host offers `clap.event-registry`, accepts param-target events from
  plugins (validated: size, mode, finite value), returns them in `outputEvents()`, and never
  delivers events from non-core spaces to a plugin.
- `core/engine` (`Graph`): mappings in the graph, as dependencies in the processing order;
  conversion to CLAP parameter events for the target in the same block.
- `plugins/step-sequencer`: one lane per track, a lock value per step, sent at the step's sample before
  its note. `plugins/lfo`: one MODULATE (or SET) lane, a point every 32 samples.
- The host loads a plugin whose only outputs are its lanes (`oscairn.param-target-lanes` with a
  lane): a modulator needs no audio or note output. `plugins/basic-synth` has parameters to lock (Brightness, Level).
- `oscairn-render --map SRC.LANE=DST.PARAM`.
- `core/control` (`Session::setMacroSource`, `flattenControls`): a control's `source`, saved in
  the Performance and Sound, undoable, `controls.source` and each control's `source` in the
  control protocol; a lane that would reach its own plugin, through the controls and the
  graph's connections, is refused.

## Verification

`param-target` test: lane 0 mapped to the gain of a test instrument whose output while a note
is held is exactly its gain. Every step's output equals that step's lock value from the
step's first sample, with 256- and 37-sample blocks; without the mapping the gain stays at
its default.

`control-source` (and `control-source-workers`, every plugin in a worker): the sequencer's lane 0
drives a control whose two absolute targets are the gains of two test instruments, one range reversed
(0.2 to 0.6, 0.6 to 0.2); each output equals its mapped lock from every step's first sample, with
256- and 37-sample blocks; with a second control's relative target on one gain moved by its knob,
that output adds its offset (the summing rule) from the next step; the source is refused for a
lane reaching its own plugin (through the control, a target or a mapping back), survives a save
and load, and is undone.

`note-routing` checks explicit per-note lane ID remapping and ordering after same-sample
notes. Not yet verified by a test: the ordering of global locks before notes at the same sample (the test
instruments read parameters every sample, so the order does not change their output).

The `MODULATE` path has its first first-party sender, the Oscairn LFO (`plugins/lfo`).
`control-source` drives a control at 0.5 with its lane (sine and triangle, 3 Hz, depth 0.5), the
control's two relative targets the gains of two test instruments (0.3 to 0.7, and reversed),
which add their gain's CLAP modulation to their gain: at each of the LFO's points (every 32
samples) and up to the next, each output equals 0.5 plus that target's offset at the LFO's
value there, with 256- and 37-sample blocks and with every plugin in a worker; the swing is
±0.2, and the gains' own values stay at 0.5. The LFO on the Basic Synth's Brightness through a
relative target changes its sound while Brightness keeps its value.

## Open questions

- Should the header be licensed permissively (MIT) so other hosts and plugins can adopt it?
  It is AGPL-3.0-or-later like the rest of the repo for now.
- Whether to propose it to the CLAP project as a draft extension, and how it should relate to
  `CLAP_EVENT_PARAM_MOD`'s per-note modulation and to host-side modulation routing.
- Lane metadata beyond a name (units, default mapping hints, bipolar or unipolar).

The two-track sequencer exposes global (default) and channel-scoped locks. The desktop
Brightness locks action selects channel scope; generic LV2 control ports need global
scope. The graph does not silently widen a channel address into a global change. See [the parameter contract](../../plugins/README.md#step-sequencer-020).
