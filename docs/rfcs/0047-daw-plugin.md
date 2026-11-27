# RFC 0047: The DAW plugin, one Performance in a wrapper

Status: accepted 2026-11-26 ([the owner's answers](#decisions), #291); not built. Tracker: #307.
Builds on the one UI of [RFC 0019](0019-adaptive-ui.md),
the license boundary of [RFC 0001](0001-license.md) and the controls work of #287. It probably
replaces #280 (standalone engine plugins): an engine alone is a one-part Performance.

VST is a trademark of Steinberg Media Technologies, Audio Units of Apple, AAX of Avid and CLAP
of u-he and Bitwig. Oscairn is not affiliated with or endorsed by any of them.

## Progress

Checked when built and tested; the sections below say how.

- [x] 1. Wrapper core around one Performance (#293)
- [ ] 2. CLAP format (#294)
- [ ] 3. VST3 format (#295)
- [ ] 4. AU format (#296)
- [ ] 5. AAX format, after the first public release if need be (#297)
- [ ] 6. Parameter slots and learn (#298)
- [ ] 7. Performance switching with slot handling (#299)
- [ ] 8. Project state save and collect (#300)
- [ ] 9. Offline render without shedding (#301)
- [ ] 10. Output buses and routing (#302)
- [ ] 11. Out-of-process workers and shared samples (#303)
- [ ] 12. Embedded UI (#304)
- [ ] 13. Host tempo and transport (#305)
- [ ] 14. CI and release builds of the formats (#306)

## Problem

The owner, 2026-11-26: Oscairn as a DAW plugin is a minimal wrapper around one Performance.
Inserting Oscairn inserts a Performance. Its editor picks a Performance from the library, and
the Performance's parameters are offered to the DAW. DAW users get every Oscairn Performance as
one plugin, without a second product to maintain per engine.

The hard parts are what a DAW expects: a fixed parameter list tied to automation IDs, a fixed set
of outputs, a project file that must open later, and a host that must not crash.

## Design

### One Performance per instance

Each plugin instance holds one Performance, picked in the editor from the library. Setlists,
Groovebox and Recordings are not in the plugin. The wrapper core (step 1) is shared by every
format; each format is a thin shell over it. It is `hosts/plugin-core`, in C++ over the engine's
Session: the instance's Session runs on a control thread of its own, where picks and restores
load while the DAW's blocks keep playing; the state is the Performance's identity and its folder
as Session::savePerformance writes it, versioned; `oscairn-plugin-core-test` is its test host.

### Parameter slots

DAWs tie automation to parameter IDs and handle a changing list unevenly (CLAP can announce a
rescan; VST3 and AU hosts mostly cannot take one). The plugin offers a fixed pool of slots with
stable IDs:

- The first slots are the universal logical controls (knobs, sliders, wheels, scene), which every
  Performance has by default (see #287).
- Learn fills the next slot: move a control in a plugin's editor and it takes the next free slot,
  so reaching a plugin parameter from the DAW stays one step.
- Learned slots are saved with the Performance.

The pool size is fixed in step 6 (the first proposal was 256).

### Switching Performances

- The universal-control slots keep working across a switch: Knob 1 stays Knob 1, whatever it does
  in the new Performance.
- Learned slots belong to the Performance they were learned in. After a switch they show as
  unassigned: the DAW keeps the lane, it drives nothing. They come back when that Performance is
  selected again.
- The editor warns before a switch that would leave automated learned slots unassigned.

### What the project saves

The project saves the whole Performance. Packages and samples are referenced by ID and version,
not embedded. On load the plugin reports what is missing and offers to collect what the project
uses into one place, like Kontakt's references, "save with samples" and missing-files search.

### Offline bounce

When the DAW renders faster than real time, voice shedding and load limits are off, so a bounce
sounds like playback on a machine that is not loaded.

### Outputs

MIDI comes in by part channel. Audio: a main stereo out plus a fixed set of 16 optional stereo
outs, declared up front because VST3 and AU mostly cannot change ports after load. Each part can
be routed to one of them.

### Inside the DAW's process

- Plugin workers stay out of process, so a crashing plugin cannot take the DAW down.
- Sample loading is shared between instances, to save memory.
- The plugin follows the host's tempo and transport.

### UI

The Rust UI is embedded in the DAW's window, scaling with it ([RFC 0019](0019-adaptive-ui.md)).
Plugins that draw their own editors open them in their own windows, as in the host.

### Formats and licenses

CLAP, VST3, AU, then AAX.

- CLAP is MIT and AU needs no license beyond Apple's tools.
- The VST3 SDK is GPLv3, compatible with the AGPL ([RFC 0001](0001-license.md)).
- AAX needs an Avid developer account, the AAX SDK (believed to be offered under GPLv3 now;
  verify in step 5) and PACE/iLok signing for release builds to load in Pro Tools.

The first public release includes the DAW plugin in CLAP, VST3 and AU. AAX is in the plan but
not necessarily first.

### The installer

The desktop installer (#292) installs the standalone and lets the user pick the plugin formats
(all selected by default), each into its standard folder. The release pipeline (#278) builds
them (step 14).

## Decisions

All by the owner, 2026-11-26 (#291):

1. Parameters: a fixed pool of stable slots, the Performance's controls always mapped, learn
   filling the next slot.
2. Which parameters by default: the universal controls; everything else by learn.
3. Saving: embed the Performance, reference packages and samples by ID and version, report what
   is missing, offer to collect.
4. Offline bounce: shedding and load limits off.
5. Outputs: main stereo plus 16 optional stereo outs, each part routable to one; MIDI by part
   channel.
6. Workers out of process, shared sample loading, host tempo and transport.
7. UI: Rust UI embedded; plugins' own editors in their own windows.
8. Formats: CLAP, VST3, AU and AAX; AAX not necessarily first.
9. Switching: the proposal above, accepted.
10. The DAW plugin is part of the first public release.

## Open questions

- AAX timing: whether AAX ships with the first public release or after it, once the SDK license
  and PACE signing are settled.
