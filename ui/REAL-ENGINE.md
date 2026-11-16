# The Performance app against the real engine

Date: 2026-11-16, second run (the first is 2026-11-15, kept below as history). Branch `rust-perf-finish` (local
`rust-app` adcf9952 rebased onto main 40003f71; one conflict, in docs/plugin-porting.md, resolved by keeping both sides).
Engine: built from this tree (`build-real`, RelWithDebInfo, `-DOSCAIRN_ENGINE=ON`, `BUILD_TESTING=OFF`); its sources are
main 40003f71's, the branch changes only `ui/` and docs. Mac, no sound.

## What was run

- **Engine:** `oscairn-engine --socket /tmp/oscrui2/engine.sock --audio null --buffer-frames 128 --plugins <scratch>/plugins
  --device-profile fantom-07`, `HOME`, `OSCAIRN_DATA_HOME` and `OSCAIRN_CONFIG_HOME` in the worktree's `.scratch/`, the socket in
  a private 0700 folder in /tmp. The log said `audio output "null", 48000 Hz, 128-sample buffer` and `0 MIDI inputs open`
  (a CoreMIDI client line comes first on this Mac; no port is opened). The plugin folder held links to the build's Basic Synth,
  Delay, Transpose, Splitters, Step Sequencer, LFO, MIDI Delay and the control fixture. Ended after the runs.
- **The Performance** "Real Engine Check", built by documented requests (Lead: Basic Synth + Delay and a send to Bus A;
  Groovebox: the step sequencer; Split: layers, lane 1 Low Synth on C-1 to B3, lane 2 Transpose + High Synth on C4 and up at
  80%; Empty; Bus A with a Delay; scenes Verse and Chorus; a control with two targets). The private library had no Sound at the
  start of the run (the Sounds section makes the first through the UI).
- **The harness** `src/tests/real_engine.rs` (ignored by default): the real shell on a headless window through a recording proxy,
  the engine's state read again on a connection of its own, every edit checked (state changed, Undo on the top bar enabled and
  returns it, the rows or tiles show it). Updated for the navigation of today: EXIT leaves a screen's layer in one step (the
  two-step `Cancel, Cancel` of before is gone), the loop that fed the shell one event per frame now hands it everything that
  arrived (the old one let a notification wait behind a burst of answers: a false fault), a new section measures idle, and the
  workarounds for engine problems that are fixed are gone (the reload before Sounds and before the top bar, the first Sound
  saved by another client, the scene tapped before the Scene screen). A full run takes about 8 minutes. Pictures:
  `ui/target/real-engine/`.

## Engine problems of the first run, checked against the engine of today

Each by a request script on the Performance above (`parts.saveSound`, `scenes.*`, `edit.history`, `harmony.set`, `params.set`,
`parts.fromSound`, `graph.commit` and a second connection subscribed to `graph.changed`):

| # | Problem (2026-11-15) | Now |
| --- | --- | --- |
| 1 | After a `scenes.select` every save is refused | **Fixed**: `parts.saveSound` accepted at +0.3, +1.0 and +3.0 s after `scenes.select` |
| 2 | Another client's `library.load` sends no `graph.changed` | **Fixed**: `graph.changed` then `session.resync` |
| 3 | `edit.history` lags edits that need no commit | **Fixed**: `scenes.name`, `controls.name` and `parts.level` are named by `edit.history` at once |
| 4 | `harmony.set` is not undone | **Fixed**: the part's own key comes back with one `edit.undo` |
| 5 | One `params.set` with two values is two steps | **Fixed** for one request (one undo returns both). **Still two steps in the UI** for an envelope drag, which sends one `params.set` for each of the two parameters it moves (below) |
| 6 | `parts.fromSound` is two steps | **Fixed**: 5 parts, 6 after, 5 after one undo |
| 7 | After an undone `scenes.add`, `scene` names the removed scene | **Fixed**: `scene` is 0. The UI had no answer to 0 (an empty Scene screen): its Scene action is now off while no scene is selected |
| 8 | `graph.commit` with nothing staged returns the same revision | **Fixed**: 207 then 208 |
| 9 | The Output bus text reads "0.0 " | Still there (the engine's text, shown as is) |
| 10 | Plugin workers accumulate | Not investigated again: 23 `oscairn-plugin-worker` processes of this engine after about fifty loads, undos and removals for a Performance of 7 nodes (4 of them playing alone); the history holds up to 8 removed plugins and there is a resident pool by design, so this is not shown to be a leak |

A new engine behavior met: the three synths of the Split part share one instance, and a client's `params.set` of a shared node's
Output bus is put back (the value reads 3 after a set of 1; the engine's history still names "synth Output bus"). The UI offered
it: it is now shown and not offered (read only) for a node with `shared`.

## Per screen

| Screen | Opened | Actions exercised | Result |
| --- | --- | --- | --- |
| Home | yes | the Performance tile | ok |
| Play | yes | scene tap, left knob and a press, M, S, a part fader, the master, the knob row's knob, + Scene (undone), the bank button | ok, each undone; idle below |
| Part | yes | Level, Pan, both keys, Transpose, Input channel, both velocities, Notes at once, Priority, the three pedals, Plays to, Send to Bus A, Key, MIDI out, Mute, Solo, Plays from | ok, each undone |
| Chain | yes | Turn off, Move earlier, Remove, Presets, Parameters, insert after the last | ok (the preset list is refused `-32005 Presets are being listed; ask again` once, and the screen's Ask again takes it) |
| Splits and lanes | yes | open a split, Add lane, a lane's keys, level, pan, Mute, Solo, its plugin's Turn off, insert, Remove lane, Remove split; the four kinds on Empty | ok, each undone |
| Parameter pages | yes | Basic Synth, Delay, Step Sequencer, Transpose, Bus Echo: every kind of tile, the dial, a view, search | ok but one: an envelope drag is two undo steps (below). The Output bus of a shared synth is read only now |
| Sound chooser | yes | the empty library's "Save this part as the first Sound" typed on the keyboard, audition, favorites, save over, save as new, load, new part, filters, search | ok |
| Scene | yes | Store, Rename, the six scopes, Glide, Curve, While playing | ok after the Scene action learned scene 0 |
| Parts list, Mixer, Controls, Routing | yes | as in the first run | ok |
| Top bar | yes | Undo, Redo, Save, Save As New, Learn, the recording mark of a take another client started | ok. Record and Capture were not pressed against the engine (they are tested against the fake engine only) |

## UI faults found by this run and fixed

1. **Nothing at rest.** Idle was measured on Home and on Play for 20 s each: before, 4.10 requests/s on Home
   (memory.state and transport.get on every status) and 13.45/s on Play (parts.meters 9.5/s on top), 28 and 31 frames in 20 s.
   Cause: the engine's `status` carries each worker's CPU and block times, which differ every time, so no status ever "said what
   the last said". Fixed: a status is compared by what the shell reads of it; RAM and the tempo are read when something besides the
   load changed (the tempo follows the `transport` topic); the meters stop being asked after two seconds of silence and wake on
   what may make a sound (docs/rust-ui.md). After: **0 requests in 20 s on Home and on Play** (the first 4 s after a connect are
   left out: a load and the first status ask what they ask once), 21 and 17 frames in 20 s, each of them the CPU or RAM figure
   on the bar changing (the harness counts the figure's changes: frames never exceed them). The null device's load figure moves
   by a percent or two about once a second.
2. **A second ask for what undo names was dropped while one was out**, and an edit the engine does not echo (the envelope drag
   got no `params.changed`) left Undo off on the bar. Asked again when the first answers; asked when a `params.set` is answered.
3. **The Scene action opened an empty screen with no scene selected** (engine problem 7): off now.
4. **A shared plugin's Output bus was offered** and the engine put the value back: read only now.
5. The sound chooser had no way to make the first Sound: it offers it.

## Left

- **An envelope drag is two undo steps**: the view's drag sends one `params.set` for each parameter it moves (Decay, then
  Sustain), and the engine takes the values of one request as one step. One request for what one move changes is the fix; it is
  in the generic view-drag path of `src/params.rs`, whose tests (tests/views.rs, tests/fm_touch.rs) assert two requests and
  belong to the FM view's branch, so it was not changed here.
- **Meters and a short note**: asleep meters wake on a status that lists a key held, and a status comes at most four times a
  second; a note shorter than that and in no status is not metered until the next wake. Needs an engine push of the levels to
  be exact.
- **Not exercised against the engine:** sound (no audio device, no MIDI: no meter showed a signal, the playing keyboard and
  Learn's assignment were not used), Setlist, a plugin's own view, the Controls' Now... and Scenes... menus, a crossover's dial,
  presets on a plugin that has some, Record and Capture buttons, the text keyboard overlay (headless pictures only), the lane key
  strip dragged (fake engine and pictures only), one window size and scale (1280x720 at 1), no touch panel, no KMS, no device.
- The harness waits fixed times: a slower machine may need longer waits.

---

# History: the first run (2026-11-15)

Branch `rust-app-real` (local `rust-app` 4d6bbb85 rebased onto main bd14ad34). The same engine setup and Performance; the table,
the five protocol disagreements the UI had (a scene select or add and a control add or rename send no `graph.changed`; a live
level or a value set is undoable with none; the chain's actions named a place, not a plugin; the Scene screen did not read after its
edits), the ten engine problems above as first recorded, the real desktop window (4.8% of a core and 8 requests/s at rest on Home,
since fixed and now measured: above), and what was not exercised, are in that commit's version of this file
(`git show adcf9952:ui/REAL-ENGINE.md`).
