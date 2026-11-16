# The Performance app against the real engine

Date: 2026-11-15. Branch `rust-app-real` (local `rust-app` 4d6bbb85 rebased onto main bd14ad34; the
rebase was clean). Engine: built from this tree (`build-real`, RelWithDebInfo, `-DOSCAIRN_ENGINE=ON`); its
sources (core, hosts/engine, hosts/audio, plugins) are main bd14ad34's unchanged. Mac, one run of each
section unless said.

## What was run

- **Engine:** `oscairn-engine --socket /tmp/oscrui/engine.sock --audio null --buffer-frames 128 --plugins
  <scratch>/plugins --device-profile fantom-07`, with `HOME`, `OSCAIRN_DATA_HOME` and `OSCAIRN_CONFIG_HOME`
  in the worktree's `.scratch/` (the socket's folder is a private folder in /tmp: the scratch path is too
  long for a Unix socket and the engine refuses a symlinked folder). `--audio null` is the null device and
  no MIDI device (hosts/audio/AudioConfiguration.h `nullDevice`; AudioEngine.cpp lists no MIDI inputs or
  outputs with it); the log said `audio output "null", 48000 Hz, 128-sample buffer` and `0 MIDI inputs open`.
  The plugin folder held links to the build's Basic Synth, Delay, Transpose, Splitters, Step Sequencer, LFO,
  MIDI Delay and the control fixture. Without `--device-profile` the shell's `controllers.view` positions for
  the FANTOM-07 profile's groups were refused (`-32003 no such control group`); the device runs the engine
  with the profile, so every run below had it.
- **The Performance** ("Real Engine Check", saved with `library.save`, reopened by `library.load` at the
  start of every run), built with documented requests by a script: Lead (Basic Synth + Delay, a send to
  Bus A), Groovebox (the default step sequencer), Split (a layers split, lane 1 Low Synth on C-1–B3, lane 2
  Transpose + High Synth on C4–G9 at 80%), Empty (no plugins), Bus A (a bus with a Delay), scenes Verse and
  Chorus, a control Macro with two targets (synth Brightness, Split's level). Every request was accepted.
- **The harness:** `src/tests/real_engine.rs`, an ignored test that runs the real shell (`shell::start`,
  the screens, their callbacks and `shell::act`, the entry points touch and the dial use) on the headless
  window against the engine through a recording proxy, and reads the engine's state back on a connection
  of its own. Each edit is checked: the engine's state changed, Undo on the top bar is enabled and returns
  the state, and the screen's rows or tiles show it before and after. Pictures: `ui/target/real-engine/`.
  `OSCAIRN_REAL_SOCKET=... OSCAIRN_REAL_PLUGINS=... OSCAIRN_REAL_SCANNER=.../oscairn-plugin-scan cargo test
  real_engine -- --ignored --nocapture` (`OSCAIRN_REAL_ONLY=part,chain` for some sections). A full run takes
  about 7 minutes and edits only the engine's Performance and its private library.
- **One real window:** the release desktop build (`cargo build --release`, winit) started directly against
  the same engine through a logging proxy, not activated, then ended.

## Per screen

| Screen | Opened | Rendered | Actions exercised | Result |
| --- | --- | --- | --- | --- |
| Home | yes | 01-home | the Performance tile (touch) | ok: tiles' lines from the engine ("1 Performance, 0 Sounds", "No audio device (--audio null)") |
| Play | yes | 02, 03, 05 | scene tap, scene by the left knob and a press, M, S, a part fader, the master fader, the knob row's knob (controls.set), + Scene, the bank button | fixed: scene select/add showed nothing new (below); the rest ok and undone. No bank button: one control, one bank |
| Part | yes | 10–13 | Level, Pan, Lowest/Highest key, Transpose, Input channel, Lowest/Highest velocity, Notes at once, Priority, the three pedals, Plays to, Send to Bus A, Key, Mute, Solo, Plays from (devices list, Built in), MIDI out list, Name (text keyboard), Remove part (two presses) | ok, each undone, except Key (engine: below). MIDI out offers only Off (no outputs with the null device) |
| Chain | yes | 20–23 | a plugin's actions: Turn off, Move earlier, Remove, Presets... (Ask again, then "No presets listed"), Parameters; insert Delay after the last | fixed: after Move earlier and Undo, Remove removed the synth (below); then ok |
| Splits and lanes | yes | 40–44 | open a layers split, Add lane, a lane: its keys, level, pan, Mute, Solo, its plugin Turn off, insert Delay in the lane, Remove lane, Remove split; on Empty: each of the four kinds of split inserted (left/right and mid/side add Split and Join, bands adds Split and shows Crossover 1 200 Hz) | ok, each undone |
| Parameter pages | yes | 30-* | Basic Synth (2 pages, knob, buttons, list, the envelope view dragged, the dial), Delay, Step Sequencer (5 pages, search typed on the keyboard), Transpose, Bus Echo | ok but: one envelope drag sets two parameters and Undo takes back one (engine: below); the Output bus list reads "0.0 ", the engine's text |
| Sound chooser | yes | 50–53 | a Sound's actions: Audition and Stop, Add to favorites, Save over its Sound, Save as a new Sound (name typed), Load into this part, New part from it; Filters (Favorites); Search (typed) | ok but: an empty library offers no way to save the part (below); New part from it needs two Undos (engine) |
| Scene | yes | 60 | Store, Rename (keyboard), the six scope switches, Glide time, Curve, While playing | fixed: edits did not show and the next switch undid the last (below); then ok and undone |
| Parts list | yes | 70 | a level slider, M, a send's slider, a new send, + Part, + Bus, a tap opening a part | ok, each undone |
| Mixer | yes | 80 | a fader, a pan, M, S, a send, the master, the dial on a fader | ok, each undone |
| Controls | yes | 90–92 | the control's fader, a target's end, Relative, Curve (menu), + Target (menus), + Control, Rename (keyboard), By scene and a mapping off in a scene, Remove a target | fixed: a control added or renamed never showed, Undo off (below) |
| Routing | yes | A0, A1 | Parts collapsed off, choose delay, Turn off, Connect audio to... Bus Echo (menu), Meters on (wires.meters asked) | ok, each undone |
| Top bar | yes | B0–B2 | Undo, Redo, Save (over the library item, "Performance saved."), Save As New (the name typed), Learn on and off, the recording mark for a take started and stopped by another client | ok; the mark showed "REC 0:01" and went. The take's files went to the private data folder |

## Protocol disagreements found and fixed (the UI or its fake engine was wrong)

1. **scenes.select and scenes.add send no graph.changed** (neither commits). The fake engine's tests assumed
   the graph would be read; the top bar kept the old scene's name and a new scene never showed on Play.
   Play now reads the graph after either is accepted. Commit 4c7df5c3.
2. **A live level, pan or master (parts.level, parts.pan, master.set) and a parameter value set by a client
   are undoable with no graph.changed**, so Undo stayed disabled while the engine could undo them.
   edit.history is now asked on controls.changed with parts or the master, and on params.changed from a
   client. Commit 4c7df5c3.
3. **The chain's actions named a place, not a plugin.** After Move earlier and its Undo, the actions still
   named the old place and Remove removed the instrument beside the effect. The actions now follow the
   node. Commit 6237166b.
4. **No scenes.* request commits.** The Scene screen never read the graph after its own edits: a rename or
   scope switch did not show, and the next switch, computed from the stale scope, undid the one before. The
   fake engine's test had that wrong request as its expectation (`"scope": ["mix"]`). The screen now shows
   what it sent, ignores graph reads that may predate an unanswered edit, and reads the graph after each
   answer; reading the Performance on a screen with the top bar's actions now asks edit.history too.
   Commit 1ff13b30.
5. **No controls.* request commits.** A control added with + Control never showed, a rename kept the old
   name, Undo stayed off. An accepted edit now has the graph read again; controls.set is a request of its
   own that reads nothing. Commit 0bb3e149.

Fake engine disagreements, all in the tests' fake answers: (1) a scene select/add answered with no read
after it; (2) no edit.history after live edits; (3) chain actions tested only with the screen's own move;
(4) the Scene test's fake graph never changed with the edits, and its expectation encoded the stale scope;
(5) the Controls tests never answered an accepted edit. Each is corrected in the commit that fixes it.

## Engine-side problems or missing requests (for the coordinator; the engine was not changed)

1. **After a scenes.select every save is refused.** Repro: open "Real Engine Check" (2 scenes with 252 stored
   values, 203 of them the step sequencer's), `scenes.select {scene: 2}`; then `parts.saveSound`,
   `library.save` and `performance.save` answer `-32005 "Performance: parameter edits are still pending;
   retry after processing"` in a probe for over 6 s, and in later checks until a load; the two
   scenes' stored values were equal. A `library.load` clears it. One params.set on any node does not cause it.
   Docs: the refusal is for staged edits, retried "after processing".
2. **Another client's library.load sends subscribers no graph.changed.** Subscribed to graph.changed, a
   client gets nothing; subscribed to params.changed it gets `session.resync {topic: params.changed}`. Docs:
   "Successful publication emits a graph resync". The shell, subscribed to status, graph.changed and
   controllers, kept showing the old Performance's name (seen: the bar said "Check Copy" after another
   client loaded "Real Engine Check"). Either a graph.changed on load or a documented topic the UI should
   take is needed.
3. **edit.history lags edits that need no commit.** Right after scenes.name, controls.name,
   controls.targets or parts.level are answered, edit.history still names the step before; about 0.2 s later
   it names the new one (it lands at the service tick). params.set and controls.add are there at once. For
   scenes.* and controls.* no notification follows, so the top bar can name the previous step until the
   next refresh (seen: "Undo Split level" while Undo undid the scene rename). Needs the step recorded
   before the answer, or a notification when history changes.
4. **harmony.set is not undone.** It makes an undo step ("Edit Lead") that undoes nothing: the part's key
   stays. Docs list harmony.set as an edit, staged until commit; the undo list in the docs does not name it.
5. **One params.set with two values is two undo steps** (and so is the envelope view's drag, which moves
   decay and sustain): one Undo returns one of them. Docs: a change that needs no commit is one step.
6. **parts.fromSound is two undo steps** ("Add synth 2", then "Add Check Lead"): one Undo leaves an empty
   part named after the Sound.
7. **After an undone scenes.add, graph.get's `scene` still names the removed scene** (3, with scenes 1 and 2
   left), so Play highlights no scene, the bar's scene name is empty and the top bar's Scene opens an empty
   page. Docs: `scene` is the one last selected or stored, or 0.
8. **graph.commit with nothing staged returns the same revision** (56 then 56). Docs: "Even an unchanged
   commit produces a new revision."
9. The Basic Synth's Output bus text is "0.0 " ... "8.0 " (params.text): shown as is.
10. Not a defect, recorded: plugin workers accumulated over the session (17 `oscairn-plugin-worker`
    processes after many loads, undos and removals); not investigated.

UI items noted, not changed (not protocol disagreements): with no Sound in the library the sound chooser
has no row to save the part as a Sound (the save actions are only in an existing Sound's actions), so the
first Sound cannot be made from this screen; the harness made it with `parts.saveSound`.

## The real window

The release desktop shell opened a 1280x752-point window (CGWindowList), connected and asked what the
shell asks. At rest on Home over 20 s it used 4.8% of a core (3.7% over another 10 s) and sent 8 requests
a second: record.takes 4/s, memory.state and transport.get 2/s each, carried by the null device's `status`
notifications, which come 4 times a second because its load figure changes. It was ended with SIGTERM.
**Since fixed (2026-11-16):** a `status` that says what the last said costs no request and no frame (`record.takes` now comes from the engine's `record` topic, `memory.state` and `transport.get` only after a changed status); tested against the fake engine, not re-measured against the real one. **Not done:** no screenshot (this session has no screen recording permission: `screencapture -l` and `-R`
both failed with "could not create image") and no navigation in the window (keys and clicks posted to the
process did nothing; it was not the key window and was not activated), so neither Play nor a parameter
page was shown in the real window; the headless pictures are the only pictures.

## Not exercised or not verified

- No sound and no MIDI: no meter showed signal, no note was played (the playing keyboard, the no-MIDI hint
  links, Learn's assignment from a moved control and the device's knobs were not exercised).
- Not opened: Setlist, the playing keyboard, a plugin's own view, the Routing panel's lane rows, + Plugin
  in Routing, wires' mix rows, the Controls' Now... and Scenes... menus, a crossover's dial, the Library
  and Tags filters, presets on a plugin that has some (Delay and Basic Synth listed none).
- Only one window size and scale (1280x720 at 1); no touch panel, no KMS, no device.
- Timing: the harness waits fixed times; edit.history was asked in time in these runs, but a slower
  machine may need longer waits.
